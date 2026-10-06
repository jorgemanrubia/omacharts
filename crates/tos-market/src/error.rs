use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("config: {0}")]
    Config(String),
    /// A request for a service this crate does not send. The allowlist in
    /// `services::ALLOWED_SERVICES` is the whole of what can reach the
    /// socket, so this is a programming error rather than anything a person
    /// can act on — and the reason "there is no order-entry code here" is
    /// checkable instead of merely true at the moment somebody last read the
    /// file.
    #[error("{0} is not a service this client sends; it charts and nothing else")]
    ForbiddenService(String),
    /// Nothing is signed in, and this crate will not sign in behind
    /// somebody's back. Carries the session file it looked in.
    #[error("no thinkorswim session in {0}; sign in first")]
    NoSession(String),
    /// Nothing on this machine could reach the network: the gateway's name
    /// did not resolve, or there is no route to it. A different sentence on
    /// screen from a gateway that is simply not answering, and the only one
    /// of the two that is not about the gateway at all.
    #[error("offline: {0}")]
    Offline(String),
    /// The network is there and the gateway did not answer: refused, timed
    /// out, or failed the TLS handshake.
    #[error("gateway unreachable: {0}")]
    Unreachable(String),
    #[error("websocket: {0}")]
    WebSocket(#[from] tungstenite::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("login failed: {0}")]
    Login(String),
    #[error("gateway error on {service} ({id}): {message}")]
    Gateway {
        service: String,
        id: String,
        message: String,
    },
    #[error("connection closed")]
    Closed,
    /// The socket died while this request was in flight. `sent` means dispatch
    /// was attempted — the request *may* have been sent. `SinkExt::send` writes
    /// then flushes, so bytes can leave before the write future succeeds.
    /// `sent: false` only if a write was never attempted.
    #[error("connection lost while waiting for a reply (request sent: {sent})")]
    ConnectionLost { sent: bool },
    #[error("timed out: {0}")]
    Timeout(String),
    /// The subscriber let go of the stream from another thread. Not a
    /// failure of anything; the thread waiting on the stream reads it as
    /// "stop".
    #[error("subscription interrupted")]
    Interrupted,
    /// The gateway kept patching this id after being asked for a snapshot
    /// and never sent one — which is what it does for an id it considers
    /// already served on this connection. Only a fresh connection fixes it.
    #[error("the gateway will not snapshot {0} on this connection")]
    NoSnapshot(String),
    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, Error>;
