//! Signing in to thinkorswim, as the settings panel and the command line
//! both see it.
//!
//! Everything here is about the session rather than about bars: what is
//! saved, whether it still works, how to get one, how to throw one away.
//! It is in the feed's own folder because every word of it is specific to
//! this feed — the browser, the paperMoney gateway, the file the session
//! lands in — and because the next feed that needs an account will have its
//! own answers to the same four questions and no reason to share these.
//!
//! Nothing in here connects to a gateway. Reading the state is a file read,
//! which is what lets a settings panel show it without charging somebody a
//! network round trip for opening Preferences.

use crate::providers::{Access, Setup};

/// What signing in to this feed involves, in the words the panel shows.
///
/// Written here, beside the code that does it, rather than in the UI: the
/// panel renders whatever the feed says, and a feed added later cannot end
/// up with its instructions in somebody else's file.
pub const SETUP: Setup = Setup {
    explain: "Charts come from your own Schwab paperMoney session. Signing in opens a real \
              Chrome window at thinkorswim, where you type your password and your one-time \
              code yourself — Omacharts never sees either, and types nothing into the page. \
              What it keeps is the session the browser ends up with.",
    requires: &[
        "A Schwab account with thinkorswim",
        "Chromium, Chrome, Brave or Edge installed on this machine",
    ],
    scope: "paperMoney charts only. No orders are ever placed, and a live-trading gateway \
            is refused. Schwab expires a session after a while: when yours goes, charts stop \
            updating and say so, and signing in again here is the fix.",
};

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
        // A sign-in that worked and landed somewhere this feed will not
        // follow. Saying so matters: reported as "no session" it looks like
        // a login that failed, and somebody goes round the same loop again.
        tos_market::SessionState::RefusedLive => Access::Refused(
            "The saved session is a live-trading one. This feed charts paperMoney only — \
             switch thinkorswim to paperMoney and sign in again."
                .into(),
        ),
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
    tos_market::sign_in(log).map_err(|error| error.to_string())
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

fn describe(account: Option<&str>, saved: Option<i64>) -> String {
    let mut out = String::from("paperMoney");
    if let Some(account) = account.filter(|a| !a.is_empty()) {
        out.push_str(&format!(" · account {account}"));
    }
    if let Some(saved) = saved {
        out.push_str(&format!(" · signed in {}", when(saved)));
    }
    out
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

    /// The panel shows these three sentences and nothing else, so they have
    /// to carry the facts somebody needs *before* pressing the button: that
    /// a browser opens and they sign in themselves, that a Schwab account
    /// and a browser are prerequisites, and what expires.
    #[test]
    fn the_instructions_say_what_a_person_has_to_know() {
        assert!(SETUP.explain.contains("you type your password"));
        assert!(SETUP.explain.contains("Omacharts never sees"));
        assert!(SETUP.requires.iter().any(|line| line.contains("Schwab")));
        assert!(SETUP.requires.iter().any(|line| line.contains("Chromium")));
        assert!(SETUP.scope.contains("No orders"));
        assert!(SETUP.scope.contains("expires"));
    }

    /// Four states, four different things to do about them. Two that read
    /// the same would be a panel that cannot tell somebody why their charts
    /// are empty.
    #[test]
    fn every_session_state_reads_differently() {
        let states = [
            Access::Missing,
            Access::Signed("paperMoney".into()),
            Access::Expired("Schwab ended the session".into()),
            Access::Refused("live".into()),
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
        assert!(line.contains("paperMoney"), "{line}");
        assert!(line.contains("D-12345"), "{line}");
        assert!(line.contains("signed in"), "{line}");
        // Nothing to say is said with nothing, not with an empty field.
        assert_eq!(describe(None, None), "paperMoney");
        assert_eq!(describe(Some(""), None), "paperMoney");
    }
}
