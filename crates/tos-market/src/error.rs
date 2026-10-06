use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("LiveTrading is disabled. Set allow_live_trading to enable live routing.")]
    LiveTradingDisabled,
    #[error("config: {0}")]
    Config(String),
    #[error("websocket: {0}")]
    WebSocket(#[from] tokio_tungstenite::tungstenite::Error),
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
    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, Error>;
