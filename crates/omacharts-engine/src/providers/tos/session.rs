//! Signing in to thinkorswim, as the settings panel and the command line
//! both see it.
//!
//! Everything here is about the session rather than about bars: what is
//! saved, whether it still works, how to get one, how to throw one away.
//! It is in the feed's own folder because every word of it is specific to
//! this feed — the browser, the gateway, the file the session
//! lands in — and because the next feed that needs an account will have its
//! own answers to the same four questions and no reason to share these.
//!
//! Nothing in here connects to a gateway. Reading the state is a file read,
//! which is what lets a settings panel show it without charging somebody a
//! network round trip for opening Preferences.

use crate::providers::{Access, How, SignIn};

/// Signing in to thinkorswim, as the boundary asks it of every feed: in a
/// browser the person drives.
#[derive(Debug)]
pub struct Signing;

impl SignIn for Signing {
    fn how(&self) -> How {
        How::Browser
    }

    fn access(&self) -> Access {
        access()
    }

    fn can_sign_in(&self) -> Result<String, String> {
        can_sign_in()
    }

    fn sign_in(&self, log: Box<dyn Fn(&str) + Send + Sync>) -> Result<(), String> {
        sign_in(log)
    }

    fn sign_out(&self) -> Result<(), String> {
        sign_out()
    }

    fn connected(&self) -> bool {
        connected()
    }

    fn places(&self) -> Vec<(&'static str, String)> {
        places()
    }
}

/// What is saved, without connecting to anything.
pub fn access() -> Access {
    match tos_market::session_state() {
        tos_market::SessionState::Missing => Access::Missing,
        tos_market::SessionState::Saved { account, saved } => {
            Access::Signed(describe(account.as_deref(), saved))
        }
        tos_market::SessionState::Expired { at } => {
            Access::Expired(format!("Schwab ended the session {}", when(at)))
        }
    }
}

/// `Ok` with the browser a sign-in would open, or why it cannot start.
///
/// Asked before the button is drawn rather than after it is pressed. A
/// machine with no Chromium-family browser cannot do this at all, and an
/// enabled button that fails is a worse answer than a disabled one that
/// says what to install.
pub fn can_sign_in() -> Result<String, String> {
    match tos_market::browser() {
        Some(path) => Ok(path.display().to_string()),
        None => Err("No Chromium-family browser found. Install Chromium, Chrome, Brave or \
                     Edge — the sign-in opens a real browser window for you to use."
            .into()),
    }
}

/// Opens the browser and waits for the person to finish.
///
/// Blocking, for as long as it takes somebody to find their phone: callers
/// run it on a thread of its own and show `log` as it arrives.
pub fn sign_in(log: impl Fn(&str) + Send + Sync + 'static) -> Result<(), String> {
    // Prefixed rather than classified. What comes back is the client's own
    // sentence about a browser or a socket, which on its own reads like
    // something the app did wrong; saying which act it belongs to is worth
    // the four words, and guessing at the cause from the text of an error
    // is how this feed used to report every failure as the wrong one.
    tos_market::sign_in(log).map_err(|error| format!("the sign-in did not finish: {error}"))
}

/// Forgets the saved session. The browser profile stays, so signing in
/// again is usually one click rather than another one-time code.
pub fn sign_out() -> Result<(), String> {
    tos_market::sign_out().map_err(|error| error.to_string())
}

/// Has this process opened a gateway connection?
pub fn connected() -> bool {
    tos_market::connected()
}

/// The files this feed keeps, for a panel that would rather show them than
/// have somebody guess.
pub fn places() -> Vec<(&'static str, String)> {
    vec![
        ("Session", tos_market::session_file().display().to_string()),
        ("Browser profile", tos_market::profile_file().display().to_string()),
    ]
}

/// The account and when, joined, or "session saved" when the file says
/// neither: the line a status row shows after "Signed in · ", so it has to
/// say something.
fn describe(account: Option<&str>, saved: Option<i64>) -> String {
    let mut parts: Vec<String> = Vec::new();
    if let Some(account) = account.filter(|a| !a.is_empty()) {
        parts.push(format!("account {account}"));
    }
    if let Some(saved) = saved {
        parts.push(format!("signed in {}", when(saved)));
    }
    if parts.is_empty() {
        return "session saved".into();
    }
    parts.join(" · ")
}

/// A unix second as somebody would say it. Local time, because the only
/// person reading it is sitting at this machine.
fn when(ts: i64) -> String {
    match chrono::DateTime::from_timestamp(ts, 0) {
        Some(utc) => chrono::DateTime::<chrono::Local>::from(utc).format("%-d %b %Y").to_string(),
        None => "at an unknown time".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Four states, four different things to do about them. Two that read
    /// the same would be a panel that cannot tell somebody why their charts
    /// are empty.
    #[test]
    fn every_session_state_reads_differently() {
        let states = [
            Access::Missing,
            Access::Signed("session saved".into()),
            Access::Expired("Schwab ended the session".into()),
        ];
        let mut lines: Vec<String> = states.iter().map(|state| state.line()).collect();
        let total = lines.len();
        lines.sort();
        lines.dedup();
        assert_eq!(lines.len(), total, "{lines:?}");
    }

    #[test]
    fn a_saved_session_says_which_account_and_since_when() {
        let line = describe(Some("D-12345"), Some(1_700_000_000));
        assert!(line.contains("D-12345"), "{line}");
        assert!(line.contains("signed in"), "{line}");
        // Which account is the account's business, not a kind of account:
        // nothing here says what sort of session it is.
        assert!(!line.to_lowercase().contains("paper"), "{line}");
        // Nothing to say still says something, since the line follows
        // "Signed in · " and an empty field there reads as a bug.
        assert_eq!(describe(None, None), "session saved");
        assert_eq!(describe(Some(""), None), "session saved");
    }
}
