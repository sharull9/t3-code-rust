//! Minimal Effect RPC client over WebSocket with JSON serialization.
//!
//! Wire format (effect `unstable/rpc/RpcMessage.ts`, `RpcSerialization.json`):
//! - client → server: `Request {id, tag, payload, headers}`, `Ack {requestId}`,
//!   `Interrupt {requestId}`, `Ping`, `Eof`
//! - server → client: `Chunk {requestId, values}`, `Exit {requestId, exit}`,
//!   `Defect {defect}`, `Pong`, `ClientProtocolError`
//!
//! A frame may hold a single message or a JSON array of messages. Streaming
//! RPCs are flow controlled: the server waits for an `Ack` after every `Chunk`.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures::{SinkExt, StreamExt};
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use tokio::sync::{mpsc, oneshot, watch};
use tokio_tungstenite::tungstenite::Message;

use crate::RpcError;

const PING_INTERVAL: Duration = Duration::from_secs(5);

enum Pending {
    Unary(oneshot::Sender<Result<Value, RpcError>>),
    Stream(mpsc::UnboundedSender<Result<Value, RpcError>>),
}

type PendingMap = Arc<Mutex<HashMap<String, Pending>>>;

pub struct RpcSession {
    outgoing: mpsc::UnboundedSender<Value>,
    pending: PendingMap,
    next_id: AtomicU64,
    closed: watch::Receiver<Option<String>>,
}

impl RpcSession {
    pub async fn connect(url: &str) -> Result<Arc<Self>, crate::Error> {
        let (socket, _) = tokio_tungstenite::connect_async(url).await?;
        let (mut sink, mut stream) = socket.split();
        let (outgoing, mut outgoing_rx) = mpsc::unbounded_channel::<Value>();
        let pending: PendingMap = Arc::default();
        let (closed_tx, closed) = watch::channel(None);

        tokio::spawn(async move {
            while let Some(message) = outgoing_rx.recv().await {
                if sink.send(Message::Text(message.to_string().into())).await.is_err() {
                    break;
                }
            }
            let _ = sink.close().await;
        });

        let pinger = outgoing.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(PING_INTERVAL);
            interval.tick().await;
            loop {
                interval.tick().await;
                if pinger.send(json!({ "_tag": "Ping" })).is_err() {
                    break;
                }
            }
        });

        let reader_pending = pending.clone();
        let acks = outgoing.clone();
        tokio::spawn(async move {
            let reason = loop {
                match stream.next().await {
                    Some(Ok(Message::Text(text))) => {
                        handle_frame(text.as_str(), &reader_pending, &acks)
                    }
                    Some(Ok(Message::Binary(bytes))) => match std::str::from_utf8(&bytes) {
                        Ok(text) => handle_frame(text, &reader_pending, &acks),
                        Err(error) => break format!("non-utf8 frame: {error}"),
                    },
                    Some(Ok(Message::Close(frame))) => {
                        break match frame {
                            Some(frame) => format!("closed ({}): {}", frame.code, frame.reason.as_str()),
                            None => "closed".into(),
                        };
                    }
                    Some(Ok(_)) => {}
                    Some(Err(error)) => break error.to_string(),
                    None => break "connection closed".into(),
                }
            };
            fail_all(&reader_pending, RpcError::Disconnected(reason.clone()));
            let _ = closed_tx.send(Some(reason));
        });

        Ok(Arc::new(Self {
            outgoing,
            pending,
            next_id: AtomicU64::new(0),
            closed,
        }))
    }

    /// Resolves with the disconnect reason once the socket is gone.
    pub async fn closed(&self) -> String {
        let mut closed = self.closed.clone();
        loop {
            if let Some(reason) = closed.borrow().clone() {
                return reason;
            }
            if closed.changed().await.is_err() {
                return "connection closed".into();
            }
        }
    }

    pub fn is_closed(&self) -> bool {
        self.closed.borrow().is_some()
    }

    pub async fn call<T: DeserializeOwned>(&self, tag: &str, payload: Value) -> Result<T, RpcError> {
        let (tx, rx) = oneshot::channel();
        self.send_request(tag, payload, Pending::Unary(tx))?;
        let value = rx
            .await
            .map_err(|_| RpcError::Disconnected("request dropped".into()))??;
        serde_json::from_value(value).map_err(|error| RpcError::Decode(error.to_string()))
    }

    pub fn subscribe<T: DeserializeOwned>(
        &self,
        tag: &str,
        payload: Value,
    ) -> Result<Subscription<T>, RpcError> {
        let (tx, rx) = mpsc::unbounded_channel();
        let id = self.send_request(tag, payload, Pending::Stream(tx))?;
        Ok(Subscription {
            id,
            items: rx,
            outgoing: self.outgoing.clone(),
            pending: self.pending.clone(),
            _item: std::marker::PhantomData,
        })
    }

    fn send_request(&self, tag: &str, payload: Value, pending: Pending) -> Result<String, RpcError> {
        if let Some(reason) = self.closed.borrow().clone() {
            return Err(RpcError::Disconnected(reason));
        }
        let id = self.next_id.fetch_add(1, Ordering::Relaxed).to_string();
        self.pending.lock().unwrap().insert(id.clone(), pending);
        let request = json!({
            "_tag": "Request",
            "id": id,
            "tag": tag,
            "payload": payload,
            "headers": [],
        });
        if self.outgoing.send(request).is_err() {
            self.pending.lock().unwrap().remove(&id);
            return Err(RpcError::Disconnected("socket writer stopped".into()));
        }
        Ok(id)
    }
}

/// A live streaming RPC. Dropping it interrupts the stream on the server.
pub struct Subscription<T> {
    id: String,
    items: mpsc::UnboundedReceiver<Result<Value, RpcError>>,
    outgoing: mpsc::UnboundedSender<Value>,
    pending: PendingMap,
    _item: std::marker::PhantomData<fn() -> T>,
}

impl<T: DeserializeOwned> Subscription<T> {
    /// `None` once the server ends the stream. A decode error only affects
    /// that item, so callers can skip items from a newer server they don't know.
    pub async fn next(&mut self) -> Option<Result<T, RpcError>> {
        let item = self.items.recv().await?;
        Some(item.and_then(|value| {
            serde_json::from_value(value).map_err(|error| RpcError::Decode(error.to_string()))
        }))
    }
}

impl<T> Drop for Subscription<T> {
    fn drop(&mut self) {
        if self.pending.lock().unwrap().remove(&self.id).is_some() {
            let _ = self
                .outgoing
                .send(json!({ "_tag": "Interrupt", "requestId": self.id }));
        }
    }
}

fn request_key(value: &Value) -> Option<String> {
    match value {
        Value::String(id) => Some(id.clone()),
        Value::Number(id) => Some(id.to_string()),
        _ => None,
    }
}

fn handle_frame(text: &str, pending: &PendingMap, acks: &mpsc::UnboundedSender<Value>) {
    let Ok(decoded) = serde_json::from_str::<Value>(text) else {
        return;
    };
    match decoded {
        Value::Array(messages) => messages
            .into_iter()
            .for_each(|message| handle_message(message, pending, acks)),
        message => handle_message(message, pending, acks),
    }
}

fn handle_message(mut message: Value, pending: &PendingMap, acks: &mpsc::UnboundedSender<Value>) {
    match message.get("_tag").and_then(Value::as_str) {
        Some("Chunk") => {
            let Some(id) = message.get("requestId").and_then(request_key) else {
                return;
            };
            let values = match message.get_mut("values").map(Value::take) {
                Some(Value::Array(values)) => values,
                _ => return,
            };
            let map = pending.lock().unwrap();
            if let Some(Pending::Stream(tx)) = map.get(&id) {
                for value in values {
                    let _ = tx.send(Ok(value));
                }
                let _ = acks.send(json!({ "_tag": "Ack", "requestId": message["requestId"] }));
            }
        }
        Some("Exit") => {
            let Some(id) = message.get("requestId").and_then(request_key) else {
                return;
            };
            let Some(entry) = pending.lock().unwrap().remove(&id) else {
                return;
            };
            let exit = message.get_mut("exit").map(Value::take).unwrap_or(Value::Null);
            let result = decode_exit(exit);
            match entry {
                Pending::Unary(tx) => {
                    let _ = tx.send(result);
                }
                Pending::Stream(tx) => {
                    // Success ends the stream by dropping the sender.
                    if let Err(error) = result {
                        let _ = tx.send(Err(error));
                    }
                }
            }
        }
        Some("Defect") => {
            let defect = message.get_mut("defect").map(Value::take).unwrap_or(Value::Null);
            fail_all(pending, RpcError::Defect(defect));
        }
        Some("ClientProtocolError") => {
            let error = message.get_mut("error").map(Value::take).unwrap_or(Value::Null);
            fail_all(pending, RpcError::Defect(error));
        }
        _ => {}
    }
}

fn decode_exit(mut exit: Value) -> Result<Value, RpcError> {
    if exit.get("_tag").and_then(Value::as_str) == Some("Success") {
        return Ok(exit.get_mut("value").map(Value::take).unwrap_or(Value::Null));
    }
    let causes = match exit.get_mut("cause").map(Value::take) {
        Some(Value::Array(causes)) => causes,
        _ => Vec::new(),
    };
    // Prefer a typed failure over defects/interrupts when a cause holds several.
    let mut fallback = RpcError::Interrupted;
    for mut cause in causes {
        match cause.get("_tag").and_then(Value::as_str) {
            Some("Fail") => {
                return Err(RpcError::Failure(
                    cause.get_mut("error").map(Value::take).unwrap_or(Value::Null),
                ));
            }
            Some("Die") => {
                fallback = RpcError::Defect(
                    cause.get_mut("defect").map(Value::take).unwrap_or(Value::Null),
                );
            }
            _ => {}
        }
    }
    Err(fallback)
}

fn fail_all(pending: &PendingMap, error: RpcError) {
    for (_, entry) in pending.lock().unwrap().drain() {
        match entry {
            Pending::Unary(tx) => {
                let _ = tx.send(Err(error.clone()));
            }
            Pending::Stream(tx) => {
                let _ = tx.send(Err(error.clone()));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn success_exit_yields_value() {
        let exit = json!({ "_tag": "Success", "value": { "ok": true } });
        assert_eq!(decode_exit(exit).unwrap(), json!({ "ok": true }));
    }

    #[test]
    fn typed_failure_wins_over_defect() {
        let exit = json!({
            "_tag": "Failure",
            "cause": [
                { "_tag": "Die", "defect": "boom" },
                { "_tag": "Fail", "error": { "_tag": "NotFound" } }
            ]
        });
        match decode_exit(exit) {
            Err(RpcError::Failure(error)) => assert_eq!(error["_tag"], "NotFound"),
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn chunk_forwards_values_and_acks() {
        let pending: PendingMap = Arc::default();
        let (item_tx, mut item_rx) = mpsc::unbounded_channel();
        pending.lock().unwrap().insert("7".into(), Pending::Stream(item_tx));
        let (ack_tx, mut ack_rx) = mpsc::unbounded_channel();

        handle_frame(
            r#"[{"_tag":"Chunk","requestId":"7","values":[1,2]},{"_tag":"Pong"}]"#,
            &pending,
            &ack_tx,
        );

        assert_eq!(item_rx.try_recv().unwrap().unwrap(), json!(1));
        assert_eq!(item_rx.try_recv().unwrap().unwrap(), json!(2));
        assert_eq!(ack_rx.try_recv().unwrap(), json!({ "_tag": "Ack", "requestId": "7" }));
    }
}
