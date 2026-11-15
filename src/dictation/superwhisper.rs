//! Superwhisper, the dictation Omarchy sets up, and how a dictation that
//! landed in this window is told from an ordinary paste.
//!
//! Omarchy owns dictation: Right Alt held starts Superwhisper recording,
//! released stops it, and Superwhisper puts the text into whatever window is
//! in front. On Hyprland it does that by pasting: the text goes on the
//! clipboard, Ctrl+V is sent to the window, and the clipboard is put back a
//! moment later. In a text box here that is an ordinary paste, and stays one.
//! Anywhere else the window would take it for pasting a copied drawing.
//!
//! So a paste outside a text box asks Superwhisper whether it has just
//! delivered a dictation, over the control socket its own bar panel reads
//! (`PROTOCOL.md` in its package). It is a dictation when Superwhisper's
//! last one finished a moment ago, did not fail, and says the very words on
//! the clipboard. Anything else is a paste, as it always was. The words are
//! the evidence that matters: someone who dictates elsewhere and then pastes
//! a drawing here pastes the drawing, because the clipboard has been put back.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::Value;

/// How long after Superwhisper finishes its paste can still arrive. It
/// pastes the moment the text is ready; the margin is for a busy machine.
pub const FRESH_SECONDS: f64 = 30.0;

/// How long to wait for the daemon. It answers from memory, in a
/// millisecond or two; one that does not answer is not one to wait for.
pub const TIMEOUT: Duration = Duration::from_millis(400);

/// The control socket, where Superwhisper's own panel looks for it.
pub fn socket() -> Option<PathBuf> {
    let set = |name: &str| std::env::var_os(name).filter(|value| !value.is_empty()).map(PathBuf::from);
    if let Some(runtime) = set("SUPERWHISPER_RUNTIME") {
        return Some(runtime.join("control.sock"));
    }
    set("XDG_RUNTIME_DIR").map(|runtime| runtime.join("superwhisper").join("control.sock"))
}

/// Superwhisper's status, or nothing when it is not running, not installed,
/// or does not answer in time.
pub fn status(socket: &Path) -> Option<Value> {
    let mut stream = UnixStream::connect(socket).ok()?;
    stream.set_read_timeout(Some(TIMEOUT)).ok()?;
    stream.set_write_timeout(Some(TIMEOUT)).ok()?;
    stream.write_all(b"{\"version\":1,\"command\":\"status\"}\n").ok()?;
    let mut line = String::new();
    BufReader::new(stream).read_line(&mut line).ok()?;
    let value: Value = serde_json::from_str(&line).ok()?;
    match value.get("error") {
        Some(error) if !error.is_null() => None,
        _ => Some(value),
    }
}

/// A dictation Superwhisper delivered.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Heard {
    /// Superwhisper's number for it, so the same one is never sent twice.
    pub session: u64,
    /// The words, as they were pasted.
    pub text: String,
}

/// Is what was just pasted, `pasted`, the dictation Superwhisper's `status`
/// says it has just finished? `now` is the Unix time in seconds.
pub fn heard(status: &Value, pasted: &str, now: f64) -> Option<Heard> {
    let last = status.get("last")?;
    if last.get("error").is_some_and(|error| !error.is_null()) {
        return None;
    }
    let said = last.get("text")?.as_str()?.trim();
    let pasted = pasted.trim();
    if said.is_empty() || pasted.is_empty() {
        return None;
    }
    // Written when the words are ready, which is when they are pasted.
    let at = last.get("createdAt")?.as_f64()?;
    if !(-5.0..=FRESH_SECONDS).contains(&(now - at)) {
        return None;
    }
    // The status carries at most the first two thousand characters.
    let truncated = last.get("truncated").and_then(Value::as_bool).unwrap_or(false);
    let same = words(said) == words(pasted) || (truncated && words(pasted).starts_with(&words(said)));
    if !same {
        return None;
    }
    let session = last
        .get("session_id")
        .or_else(|| status.get("session_id"))
        .and_then(Value::as_u64)
        .unwrap_or(at as u64);
    Some(Heard { session, text: pasted.to_string() })
}

/// The words of `text`, however it was spaced.
fn words(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const NOW: f64 = 1_794_618_670.0;

    fn finished(text: &str, at: f64) -> Value {
        json!({
            "ready": true,
            "phase": "idle",
            "session_id": 7,
            "last": {"text": text, "createdAt": at, "session_id": 7, "delivery": "pasted"}
        })
    }

    #[test]
    fn a_paste_of_the_words_just_dictated_is_a_dictation() {
        let status = finished("Add an RSI to this chart.", NOW - 1.0);
        let heard = heard(&status, "Add an RSI to this chart. ", NOW).unwrap();
        assert_eq!(heard, Heard { session: 7, text: "Add an RSI to this chart.".into() });
    }

    #[test]
    fn a_paste_of_anything_else_is_a_paste() {
        let status = finished("Add an RSI to this chart.", NOW - 1.0);
        assert_eq!(heard(&status, "ESZ6", NOW), None);
        assert_eq!(heard(&status, "", NOW), None);
    }

    #[test]
    fn a_dictation_from_a_while_ago_is_not_this_paste() {
        let status = finished("Add an RSI.", NOW - FRESH_SECONDS - 1.0);
        assert_eq!(heard(&status, "Add an RSI.", NOW), None);
    }

    #[test]
    fn a_failed_dictation_is_nothing() {
        let status = json!({"last": {"createdAt": NOW, "error": "Audio capture failed", "session_id": 24}});
        assert_eq!(heard(&status, "anything", NOW), None);
    }

    #[test]
    fn a_long_dictation_is_matched_on_what_the_status_carries() {
        let long = "word ".repeat(600);
        let preview: String = long.chars().take(2000).collect();
        let mut status = finished(&preview, NOW);
        status["last"]["truncated"] = json!(true);
        assert_eq!(heard(&status, &long, NOW).unwrap().text, long.trim());
    }

    #[test]
    fn spacing_does_not_matter() {
        let status = finished("Chart  gold\non the 15 minute.", NOW);
        assert!(heard(&status, "Chart gold on the 15 minute.", NOW).is_some());
    }

    #[test]
    fn nothing_listening_is_no_status() {
        let missing = std::env::temp_dir().join(format!("omacharts-no-superwhisper-{}.sock", std::process::id()));
        assert_eq!(status(&missing), None);
    }

    #[test]
    fn the_status_is_read_off_the_socket() {
        use std::os::unix::net::UnixListener;
        let path = std::env::temp_dir().join(format!("omacharts-superwhisper-{}.sock", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let listener = UnixListener::bind(&path).unwrap();
        let server = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut asked = String::new();
            BufReader::new(&stream).read_line(&mut asked).unwrap();
            let mut stream = stream;
            stream.write_all(b"{\"ready\":true,\"last\":{\"text\":\"hi\",\"createdAt\":1}}\n").unwrap();
            asked
        });
        let status = status(&path).unwrap();
        assert_eq!(status["last"]["text"], "hi");
        let asked: Value = serde_json::from_str(&server.join().unwrap()).unwrap();
        assert_eq!(asked, json!({"version": 1, "command": "status"}));
        let _ = std::fs::remove_file(&path);
    }
}
