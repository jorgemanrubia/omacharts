/// Published session-manager host. A session file may name another in
/// `TOS_TSM_URL`. Saving a session keeps that cookie; this crate does not
/// call the host, and it does not switch this session onto the live gateway.
pub const TSM_URL: &str = "https://tosweb-sm.thinkorswim.com/api/v1/tsm";
