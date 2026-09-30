use serde_json::Value;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("invalid pairing link: {0}")]
    InvalidPairingLink(String),

    /// The server rejected our credentials. The client has to pair again.
    #[error("unauthorized ({status}): {body}")]
    Unauthorized { status: u16, body: String },

    #[error("http {status}: {body}")]
    Http { status: u16, body: String },

    #[error(transparent)]
    Request(#[from] reqwest::Error),

    #[error(transparent)]
    WebSocket(#[from] tokio_tungstenite::tungstenite::Error),

    #[error(transparent)]
    Url(#[from] url::ParseError),

    #[error(transparent)]
    Rpc(#[from] RpcError),
}

/// Failure of a single RPC, mirroring Effect's encoded `Exit` causes.
#[derive(Debug, Clone, thiserror::Error)]
pub enum RpcError {
    /// Typed failure declared in the contract (`error` schema of the RPC).
    #[error("rpc failed: {0}")]
    Failure(Value),

    /// Unexpected server defect.
    #[error("rpc defect: {0}")]
    Defect(Value),

    #[error("rpc interrupted")]
    Interrupted,

    #[error("disconnected: {0}")]
    Disconnected(String),

    #[error("could not decode response: {0}")]
    Decode(String),
}

impl Error {
    pub fn is_unauthorized(&self) -> bool {
        match self {
            Error::Unauthorized { .. } => true,
            Error::WebSocket(tokio_tungstenite::tungstenite::Error::Http(response)) => {
                matches!(response.status().as_u16(), 401 | 403)
            }
            _ => false,
        }
    }
}
