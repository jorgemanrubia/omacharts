//! Signing in to Rithmic, as the settings, the command line and the charts
//! see it.
//!
//! A login is three settings and a password. The user ID, the system and the
//! gateway are settings the app keeps, `Local` because a login belongs to
//! the machine it was typed on, and hands to this module with [`remember`]:
//! the engine has no database of its own. The password is in the desktop
//! keyring and is read only when the session signs in.
//!
//! A developer's login is a file instead, named by
//! [`rithmic::DEV_LOGIN`], and when that is set it wins: work on the app
//! never reads or writes anybody's keyring.
//!
//! Nothing here connects. What is saved is read from memory, the
//! environment and at most one file, which is what lets a chart be routed,
//! and a settings row drawn, without a round trip to Chicago.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, RwLock};

use crate::providers::{Access, Setup};

/// The settings a login is kept in.
pub const SETTING_USER: &str = "rithmic_user";
pub const SETTING_SYSTEM: &str = "rithmic_system";
pub const SETTING_GATEWAY: &str = "rithmic_gateway";

/// What signing in involves, in the words a settings page shows.
pub const SETUP: Setup = Setup {
    explain: "Charts of CME futures come from your own Rithmic login, the one a prop firm \
              gives you. Omacharts signs in to Rithmic directly from this machine: the \
              password stays in your desktop keyring and is sent to Rithmic's gateway and \
              nowhere else.",
    requires: &[
        "A Rithmic user ID and password, and the system they are for",
        "A desktop keyring (gnome-keyring) to keep the password in",
    ],
    scope: "Market data only: front-month futures on CME, CBOT, NYMEX and COMEX, live. \
            Every chart shares one session, because CME counts sessions. Nothing here can \
            place an order.",
};

/// The login as the app has it saved, without the password.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Saved {
    pub user: Option<String>,
    pub system: Option<String>,
    pub gateway: Option<String>,
}

static SAVED: RwLock<Option<Saved>> = RwLock::new(None);

/// Why Rithmic last turned the login down in this process, in its words,
/// until somebody signs in again.
static REFUSED: Mutex<Option<String>> = Mutex::new(None);

/// Does this process chart from Rithmic? Only the one holding the window
/// does, so that the bar widget and a command run with no window open never
/// open a second session beside it: see [`serve_from_here`].
static HERE: AtomicBool = AtomicBool::new(false);

/// What the app has saved. Called whenever it reads its settings for a
/// feed list that names Rithmic, and after a sign-in or a sign-out.
pub fn remember(saved: Saved) {
    *SAVED.write().unwrap_or_else(|e| e.into_inner()) = Some(saved);
}

fn saved() -> Saved {
    SAVED.read().unwrap_or_else(|e| e.into_inner()).clone().unwrap_or_default()
}

/// This process holds the window, and is the one that charts from Rithmic.
///
/// Any other — the bar widget asking from a fresh process, a command run
/// with no window open — passes Rithmic over and charts from the next feed,
/// rather than signing in a session of its own: CME bills a second
/// concurrent session as a professional subscription.
pub fn serve_from_here() {
    HERE.store(true, Ordering::Release);
}

/// May this process chart from Rithmic at all?
pub fn here() -> bool {
    HERE.load(Ordering::Acquire)
}

fn system() -> String {
    saved().system.filter(|system| !system.trim().is_empty()).unwrap_or_else(|| rithmic::TEST_SYSTEM.into())
}

fn gateway() -> String {
    saved().gateway.filter(|url| !url.trim().is_empty()).unwrap_or_else(|| rithmic::TEST_URL.into())
}

fn user() -> Option<String> {
    saved().user.map(|user| user.trim().to_string()).filter(|user| !user.is_empty())
}

/// The login to sign in with, read again every time the session signs in.
pub fn login() -> Result<rithmic::Login, rithmic::Error> {
    if let Some(development) = rithmic::Login::from_env(&system(), &gateway()) {
        return development;
    }
    match user() {
        Some(user) => Ok(rithmic::Login::Keyring { user, system: system(), url: gateway() }),
        None => Err(rithmic::Error::NoLogin("no Rithmic user ID is saved".into())),
    }
}

/// The session, started on first use.
pub fn client() -> Result<rithmic::Client, rithmic::Error> {
    rithmic::session(login)
}

/// Rithmic turned the login down: remembered, so that the settings and
/// `provider status` can say so in Rithmic's words, until a sign-in.
pub fn refused(why: &str) {
    *REFUSED.lock().unwrap_or_else(|e| e.into_inner()) = Some(why.to_string());
}

fn refusal() -> Option<String> {
    REFUSED.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

/// What is saved, without connecting to anything. The user ID is never
/// part of the line: it is read back on screens and in command output.
pub fn access() -> Access {
    let system = system();
    if let Some(development) = rithmic::Login::from_env(&system, &gateway()) {
        return match (development, refusal()) {
            (Err(error), _) => Access::Expired(error.to_string()),
            (Ok(_), Some(why)) => Access::Expired(format!("Rithmic said: {why}")),
            (Ok(_), None) => Access::Signed(format!("{system} · development login")),
        };
    }
    match (user(), refusal()) {
        (None, _) => Access::Missing,
        (Some(_), Some(why)) => Access::Expired(format!("Rithmic said: {why}")),
        (Some(_), None) => Access::Signed(system),
    }
}

/// How a sign-in would go, or why it cannot. Nothing opens a browser here.
pub fn can_sign_in() -> Result<String, String> {
    if rithmic::Login::from_env(&system(), &gateway()).is_some() {
        return Ok(format!("the development login {} names", rithmic::DEV_LOGIN));
    }
    match user() {
        Some(_) => Ok("the password in the desktop keyring".into()),
        None => Err(format!(
            "No Rithmic user ID is saved. Save one with `omacharts config set {SETTING_USER} <ID>`."
        )),
    }
}

/// Check that what is saved can sign in, without signing in: the session
/// is the window's, and a second one opened here to try the password would
/// be a second session as far as CME is concerned. Forgets any earlier
/// refusal, and ends this process's session so that the next chart signs in
/// afresh with what is saved now.
pub fn sign_in(log: impl Fn(&str)) -> Result<(), String> {
    let system = system();
    if let Some(development) = rithmic::Login::from_env(&system, &gateway()) {
        development.map_err(|error| error.to_string())?;
        log(&format!("using the development login {} names", rithmic::DEV_LOGIN));
    } else {
        let Some(user) = user() else {
            return Err(can_sign_in().err().unwrap_or_default());
        };
        let held = rithmic::keyring::holds(&user, &system).map_err(|error| error.to_string())?;
        if !held {
            return Err(format!(
                "the keyring holds no Rithmic password for {system}. Save it with: secret-tool store \
                 --label='Omacharts: Rithmic login' application omacharts service rithmic system \
                 '{system}' user <your user ID>"
            ));
        }
        log(&format!("found the password for {system} in the keyring"));
    }
    *REFUSED.lock().unwrap_or_else(|e| e.into_inner()) = None;
    rithmic::shutdown();
    Ok(())
}

/// Sign out: the session ends, and the password leaves the keyring. The
/// user ID is the app's to forget, which it does beside this.
pub fn sign_out() -> Result<(), String> {
    rithmic::shutdown();
    *REFUSED.lock().unwrap_or_else(|e| e.into_inner()) = None;
    let forgotten = match (rithmic::Login::from_env(&system(), &gateway()), user()) {
        // A development login is a file of the developer's own, and is
        // never the keyring's.
        (Some(_), _) | (None, None) => Ok(()),
        (None, Some(user)) => rithmic::keyring::forget(&user, &system()).map(|_| ()).map_err(|error| error.to_string()),
    };
    let mut saved = SAVED.write().unwrap_or_else(|e| e.into_inner());
    if let Some(saved) = saved.as_mut() {
        saved.user = None;
    }
    forgotten
}

/// Has this process a Rithmic session?
pub fn connected() -> bool {
    rithmic::running()
}

/// Where the login is kept, for somebody who would rather see it than
/// guess.
pub fn places() -> Vec<(&'static str, String)> {
    let mut places = vec![(
        "Keyring item",
        format!("application=omacharts service=rithmic system={}", system()),
    )];
    if let Some(path) = std::env::var_os(rithmic::DEV_LOGIN).filter(|path| !path.is_empty()) {
        places.push(("Development login", std::path::PathBuf::from(path).display().to_string()));
    }
    places
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_instructions_say_where_the_password_goes() {
        assert!(SETUP.explain.contains("keyring"));
        assert!(SETUP.explain.contains("nowhere else"));
        assert!(SETUP.scope.contains("one session"));
        assert!(SETUP.scope.contains("cannot") || SETUP.scope.contains("Nothing here can"));
    }
}
