//! The record of Rithmic sessions: one line in
//! `$XDG_DATA_HOME/omacharts/rithmic-sessions.log` for each plant that signs
//! in, with Rithmic's `unique_user_id` for it and when, in local time with
//! its offset. Rithmic's conformance review asks for these to look at a
//! session from its side.
//!
//! No password or account is written: the plant, the system, the gateway
//! and Rithmic's own id for the session.

use std::io::Write;
use std::path::PathBuf;

use chrono::{DateTime, Local};

/// Where the sessions are kept.
pub fn path() -> PathBuf {
    crate::store::data_dir().join("rithmic-sessions.log")
}

/// Keep a line for every Rithmic session from now on.
pub fn keep() {
    rithmic::on_session(|session| {
        let path = path();
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
            let _ = writeln!(file, "{}", line(session));
        }
    });
}

fn line(session: &rithmic::Session) -> String {
    let at: DateTime<Local> = session.at.into();
    format!(
        "{}  {:<7}  unique_user_id={}  system={}  gateway={}",
        at.format("%Y-%m-%d %H:%M:%S UTC%:z"),
        session.plant,
        session.id,
        session.system,
        session.gateway,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_line_names_the_plant_its_session_and_when() {
        let session = rithmic::Session {
            plant: "order",
            id: "8f2c-11".into(),
            system: "Rithmic Test".into(),
            gateway: "wss://rituz00100.rithmic.com:443".into(),
            at: std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_800_000_000),
        };
        let line = line(&session);
        assert!(line.contains("order    unique_user_id=8f2c-11  system=Rithmic Test"), "{line}");
        assert!(line.contains(" UTC+") || line.contains(" UTC-"), "with its offset: {line}");
    }
}
