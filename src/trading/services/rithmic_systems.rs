//! The Rithmic systems a login can be on, as Rithmic's production gateway
//! names them: Rithmic 01, Rithmic Paper Trading, and each prop firm's own,
//! such as Tradeify or LucidTrading.
//!
//! Asked of the gateway with no login (see `rithmic::systems`) and kept in
//! `$XDG_DATA_HOME/omacharts/rithmic-systems.json`, so the sign-in dialog
//! shows them at once and asks again only once the list is a week old.

use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// The gateway the list is asked of. Every production gateway serves the
/// same systems; Rithmic Test's serves only its own.
const GATEWAY: &str = "wss://rprotocol.rithmic.com:443";

/// How long a list is good for before it is asked for again.
const FRESH: Duration = Duration::from_secs(7 * 24 * 60 * 60);

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Kept {
    /// When it was asked, in seconds since the epoch.
    asked: u64,
    systems: Vec<String>,
}

fn path() -> PathBuf {
    crate::store::data_dir().join("rithmic-systems.json")
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |since| since.as_secs())
}

fn read() -> Option<Kept> {
    let text = std::fs::read_to_string(path()).ok()?;
    serde_json::from_str::<Kept>(&text).ok().filter(|kept| !kept.systems.is_empty())
}

/// The systems last asked for, if they ever were.
pub fn kept() -> Option<Vec<String>> {
    read().map(|kept| kept.systems)
}

/// Whether the list is missing or a week old, and worth asking for again.
pub fn stale() -> bool {
    read().is_none_or(|kept| now().saturating_sub(kept.asked) >= FRESH.as_secs())
}

/// Ask the gateway for its systems and keep them. Blocks for a dial and a
/// request, so it belongs off the GTK thread.
pub fn ask() -> Result<Vec<String>, rithmic::Error> {
    let app = omacharts_engine::providers::rithmic::session::app()?;
    let systems = rithmic::systems(app, GATEWAY)?;
    if !systems.is_empty() {
        keep(&systems);
    }
    Ok(systems)
}

fn keep(systems: &[String]) {
    let path = path();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let kept = Kept { asked: now(), systems: systems.to_vec() };
    if let Ok(text) = serde_json::to_string_pretty(&kept) {
        let _ = std::fs::write(path, text);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What is kept is read back, fresh, until a week has passed.
    #[test]
    fn a_list_is_kept_and_goes_stale_after_a_week() {
        let _ = std::fs::remove_file(path());
        assert!(kept().is_none() && stale());

        keep(&["Rithmic 01".into(), "Tradeify".into()]);
        assert_eq!(kept().unwrap(), ["Rithmic 01", "Tradeify"]);
        assert!(!stale());

        let old = Kept { asked: now() - FRESH.as_secs(), systems: vec!["Tradeify".into()] };
        std::fs::write(path(), serde_json::to_string(&old).unwrap()).unwrap();
        assert!(stale());
        assert_eq!(kept().unwrap(), ["Tradeify"], "a stale list is still shown");
        let _ = std::fs::remove_file(path());
    }
}
