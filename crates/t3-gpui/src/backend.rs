//! Owns the T3 connection on a dedicated tokio thread.
//!
//! GPUI runs its own executor, so the UI talks to this thread through channels:
//! `Command`s in, `Event`s out. The backend handles pairing, credential storage
//! and reconnects; after a reconnect it resubscribes and the fresh snapshots
//! replace the UI state.

use std::path::PathBuf;
use std::time::Duration;

use futures::channel::mpsc as ui_channel;
use t3_client::reqwest;
use t3_client::{
    Connection, Credentials, PairingLink, RpcError, ShellStreamItem, ThreadShell,
    ThreadStreamItem,
};
use tokio::sync::mpsc;

const CLIENT_LABEL: &str = "T3 GPUI";
const THREAD_TURN_LIMIT: u32 = 30;
const MAX_BACKOFF: Duration = Duration::from_secs(30);

pub enum Command {
    Pair(String),
    OpenThread(String),
    SendMessage { thread: ThreadShell, text: String },
    Interrupt { thread_id: String, turn_id: Option<String> },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    NeedsPairing,
    Connecting(String),
    Connected(String),
    Reconnecting { server: String, reason: String },
}

pub enum Event {
    Status(Status),
    Shell(ShellStreamItem),
    Thread { thread_id: String, item: ThreadStreamItem },
    Error(String),
}

pub struct Backend {
    commands: mpsc::UnboundedSender<Command>,
}

impl Backend {
    pub fn spawn() -> (Self, ui_channel::UnboundedReceiver<Event>) {
        let (commands, command_rx) = mpsc::unbounded_channel();
        let (events, event_rx) = ui_channel::unbounded();
        std::thread::Builder::new()
            .name("t3-backend".into())
            .spawn(move || {
                tokio::runtime::Builder::new_multi_thread()
                    .enable_all()
                    .build()
                    .expect("failed to start tokio runtime")
                    .block_on(run(command_rx, Emitter(events)));
            })
            .expect("failed to spawn backend thread");
        (Self { commands }, event_rx)
    }

    pub fn send(&self, command: Command) {
        let _ = self.commands.send(command);
    }
}

#[derive(Clone)]
struct Emitter(ui_channel::UnboundedSender<Event>);

impl Emitter {
    fn emit(&self, event: Event) {
        let _ = self.0.unbounded_send(event);
    }

    fn error(&self, message: impl Into<String>) {
        self.emit(Event::Error(message.into()));
    }
}

/// Aborts the task (and so drops its subscription, interrupting it server-side).
struct TaskGuard(tokio::task::JoinHandle<()>);

impl Drop for TaskGuard {
    fn drop(&mut self) {
        self.0.abort();
    }
}

enum SessionEnd {
    Disconnected(String),
    Repair(String),
    Quit,
}

async fn run(mut commands: mpsc::UnboundedReceiver<Command>, events: Emitter) {
    let http = reqwest::Client::new();
    let mut credentials = load_credentials().filter(|c| !c.is_expired());
    let mut open_thread: Option<String> = None;
    let mut pending_pair: Option<String> = None;
    let mut backoff = Duration::from_secs(1);

    loop {
        if let Some(link) = pending_pair.take() {
            match pair(&http, &link).await {
                Ok(paired) => {
                    save_credentials(&paired);
                    credentials = Some(paired);
                }
                Err(error) => events.error(format!("Pairing failed: {error}")),
            }
        }

        let Some(current) = credentials.clone() else {
            events.emit(Event::Status(Status::NeedsPairing));
            match wait_offline(&mut commands, &mut open_thread, None).await {
                Offline::Pair(link) => pending_pair = Some(link),
                Offline::Quit => return,
                Offline::Elapsed => {}
            }
            continue;
        };

        let server = current.base_url.to_string();
        events.emit(Event::Status(Status::Connecting(server.clone())));
        let reason = match Connection::connect(&http, &current).await {
            Ok(connection) => {
                backoff = Duration::from_secs(1);
                events.emit(Event::Status(Status::Connected(server.clone())));
                match run_session(&connection, &mut commands, &events, &mut open_thread).await {
                    SessionEnd::Disconnected(reason) => reason,
                    SessionEnd::Repair(link) => {
                        pending_pair = Some(link);
                        continue;
                    }
                    SessionEnd::Quit => return,
                }
            }
            Err(error) if error.is_unauthorized() => {
                clear_credentials();
                credentials = None;
                events.error("The server rejected this client. Pair again.");
                continue;
            }
            Err(error) => error.to_string(),
        };

        events.emit(Event::Status(Status::Reconnecting { server, reason }));
        match wait_offline(&mut commands, &mut open_thread, Some(backoff)).await {
            Offline::Pair(link) => pending_pair = Some(link),
            Offline::Quit => return,
            Offline::Elapsed => {}
        }
        backoff = (backoff * 2).min(MAX_BACKOFF);
    }
}

enum Offline {
    Pair(String),
    Quit,
    Elapsed,
}

/// Handles commands while there is no connection, until `timeout` (if any).
async fn wait_offline(
    commands: &mut mpsc::UnboundedReceiver<Command>,
    open_thread: &mut Option<String>,
    timeout: Option<Duration>,
) -> Offline {
    let sleep = async {
        match timeout {
            Some(timeout) => tokio::time::sleep(timeout).await,
            None => std::future::pending().await,
        }
    };
    tokio::pin!(sleep);
    loop {
        tokio::select! {
            () = &mut sleep => return Offline::Elapsed,
            command = commands.recv() => match command {
                None => return Offline::Quit,
                Some(Command::Pair(link)) => return Offline::Pair(link),
                Some(Command::OpenThread(thread_id)) => *open_thread = Some(thread_id),
                // Sending needs a live server; the UI keeps the draft disabled.
                Some(Command::SendMessage { .. } | Command::Interrupt { .. }) => {}
            },
        }
    }
}

async fn run_session(
    connection: &Connection,
    commands: &mut mpsc::UnboundedReceiver<Command>,
    events: &Emitter,
    open_thread: &mut Option<String>,
) -> SessionEnd {
    let _shell = TaskGuard(tokio::spawn(forward_shell(connection.clone(), events.clone())));
    let mut _thread = open_thread
        .clone()
        .map(|thread_id| spawn_thread(connection, thread_id, events));

    loop {
        tokio::select! {
            reason = connection.closed() => return SessionEnd::Disconnected(reason),
            command = commands.recv() => match command {
                None => return SessionEnd::Quit,
                Some(Command::Pair(link)) => return SessionEnd::Repair(link),
                Some(Command::OpenThread(thread_id)) => {
                    *open_thread = Some(thread_id.clone());
                    // Replacing the guard interrupts the previous thread's stream.
                    _thread = Some(spawn_thread(connection, thread_id, events));
                }
                Some(Command::SendMessage { thread, text }) => {
                    let connection = connection.clone();
                    let events = events.clone();
                    tokio::spawn(async move {
                        if let Err(error) = connection.send_message(&thread, &text).await {
                            events.error(format!("Send failed: {}", describe(&error)));
                        }
                    });
                }
                Some(Command::Interrupt { thread_id, turn_id }) => {
                    let connection = connection.clone();
                    let events = events.clone();
                    tokio::spawn(async move {
                        if let Err(error) = connection.interrupt(&thread_id, turn_id.as_deref()).await {
                            events.error(format!("Stop failed: {}", describe(&error)));
                        }
                    });
                }
            },
        }
    }
}

async fn forward_shell(connection: Connection, events: Emitter) {
    let mut stream = match connection.subscribe_shell() {
        Ok(stream) => stream,
        Err(error) => return events.error(format!("Shell subscription failed: {error}")),
    };
    while let Some(item) = stream.next().await {
        match item {
            Ok(item) => events.emit(Event::Shell(item)),
            Err(RpcError::Decode(error)) => eprintln!("skipping shell item: {error}"),
            Err(error) => return events.error(format!("Shell stream ended: {}", describe(&error))),
        }
    }
}

fn spawn_thread(connection: &Connection, thread_id: String, events: &Emitter) -> TaskGuard {
    let connection = connection.clone();
    let events = events.clone();
    TaskGuard(tokio::spawn(async move {
        let mut stream = match connection.subscribe_thread(&thread_id, THREAD_TURN_LIMIT) {
            Ok(stream) => stream,
            Err(error) => return events.error(format!("Thread subscription failed: {error}")),
        };
        while let Some(item) = stream.next().await {
            match item {
                Ok(item) => events.emit(Event::Thread { thread_id: thread_id.clone(), item }),
                Err(RpcError::Decode(error)) => eprintln!("skipping thread item: {error}"),
                Err(error) => {
                    return events.error(format!("Thread stream ended: {}", describe(&error)));
                }
            }
        }
    }))
}

async fn pair(http: &reqwest::Client, link: &str) -> Result<Credentials, t3_client::Error> {
    let link = PairingLink::parse(link)?;
    t3_client::auth::pair(http, &link, CLIENT_LABEL).await
}

/// Typed failures are tagged structs; show their tag and message if present.
fn describe(error: &RpcError) -> String {
    match error {
        RpcError::Failure(value) => {
            let tag = value.get("_tag").and_then(|v| v.as_str()).unwrap_or("Error");
            match value.get("message").and_then(|v| v.as_str()) {
                Some(message) => format!("{tag}: {message}"),
                None => format!("{tag}: {value}"),
            }
        }
        other => other.to_string(),
    }
}

fn credentials_path() -> Option<PathBuf> {
    Some(dirs::config_dir()?.join("t3-gpui").join("credentials.json"))
}

fn load_credentials() -> Option<Credentials> {
    let contents = std::fs::read_to_string(credentials_path()?).ok()?;
    serde_json::from_str(&contents).ok()
}

fn save_credentials(credentials: &Credentials) {
    let Some(path) = credentials_path() else { return };
    let result = path
        .parent()
        .map_or(Ok(()), std::fs::create_dir_all)
        .and_then(|()| {
            let json = serde_json::to_string_pretty(credentials).map_err(std::io::Error::other)?;
            std::fs::write(&path, json)
        });
    if let Err(error) = result {
        eprintln!("could not save credentials to {}: {error}", path.display());
    }
}

fn clear_credentials() {
    if let Some(path) = credentials_path() {
        let _ = std::fs::remove_file(path);
    }
}
