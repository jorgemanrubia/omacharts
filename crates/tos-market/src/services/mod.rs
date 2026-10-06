//! Chart candles, and the login that opens the socket.

pub mod chart;
pub mod login;

/// A chart patch with no base document is safe to ask for again.
pub const REPLAYABLE_SERVICES: &[&str] = &["chart"];

pub fn is_replayable(service: &str) -> bool {
    REPLAYABLE_SERVICES.contains(&service)
}
