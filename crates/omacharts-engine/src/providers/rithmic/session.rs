//! Signing in to Rithmic, as the settings, the command line and the charts
//! see it.
//!
//! The app keeps any number of Rithmic logins, and one of them is the data
//! login: the one charts are fed through. Its id, user ID, system and
//! gateway are settings the app keeps, `Local` because a login belongs to
//! the machine it was typed on, and hands to this module with [`remember`]:
//! the engine has no database of its own. The password is in the desktop
//! keyring under the login's id, and is read only when the session signs
//! in.
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

pub use crate::providers::{Saved, Secrets};
use crate::providers::{Access, How, Sessions, SignIn};

/// The app name Rithmic is told, the one place it is set.
///
/// Rithmic Test takes any name; production takes only the one Rithmic
/// registers for the app when it passes conformance, prefixed with four
/// characters of theirs (`abcd:omacharts`). That name goes here. A build
/// can set it instead with `OMACHARTS_RITHMIC_APP_NAME`.
pub const APP_NAME: &str = match option_env!("OMACHARTS_RITHMIC_APP_NAME") {
    Some(name) => name,
    None => "omacharts",
};

/// The app as Rithmic is told it: the release
/// [`crate::providers::identify`] was handed. Without it there is none, and
/// no session starts, rather than one that reports a wrong version.
pub fn app() -> Result<rithmic::App, rithmic::Error> {
    match crate::providers::app_version() {
        Some(version) => Ok(rithmic::App { name: APP_NAME, version }),
        None => Err(rithmic::Error::Connection("the app never said which version it is".into())),
    }
}

/// What a login's system and gateway are kept under in its values: what its
/// dialog writes, and what the data session and the broker read.
pub const SYSTEM: &str = "system";
pub const GATEWAY: &str = "gateway";

/// Signing in to Rithmic, as the boundary asks it of every feed.
#[derive(Debug)]
pub struct Signing;

impl SignIn for Signing {
    fn how(&self) -> How {
        How::Dialog
    }

    /// CME bills a second concurrent session as a professional
    /// subscription.
    fn sessions(&self) -> Sessions {
        Sessions::OnePerLogin
    }

    /// A prop firm's login is for its orders as much as for its charts.
    fn trades(&self) -> bool {
        true
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

    fn connect(&self) -> Result<(), String> {
        connect()
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

    fn remember(&self, saved: Saved) {
        remember(saved)
    }

    fn hand_over(&self) {
        hand_over()
    }

    fn refusal(&self) -> Option<String> {
        refusal()
    }

    fn keyring(&self) -> Option<&'static dyn Secrets> {
        Some(&Keyring)
    }
}

static SAVED: RwLock<Option<Saved>> = RwLock::new(None);

/// Why Rithmic last turned the login down in this process, in its words,
/// until somebody signs in again.
static REFUSED: Mutex<Option<String>> = Mutex::new(None);

/// The exchanges the signed-in login is entitled to, as the window last
/// adopted them: `None` until a session has said, which charts as if every
/// CME group exchange were. See [`adopt_entitlements`].
static ENTITLED: RwLock<Option<Vec<String>>> = RwLock::new(None);

/// May this login chart from `exchange`? Yes until a session has said
/// otherwise. Pure and offline: a read of what was adopted.
pub fn entitled(exchange: &str) -> bool {
    ENTITLED
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
        .is_none_or(|exchanges| exchanges.iter().any(|e| e == exchange))
}

/// Take on what the session last heard about entitlements, so that
/// [`entitled`], and with it routing, follows it. Separate from the session
/// hearing it, so the window can note where every chart is served from
/// first, and move only the charts whose source changes. True when it
/// changed anything.
pub fn adopt_entitlements() -> bool {
    adopt(rithmic::entitled_exchanges())
}

/// Call `listener`, on the session's thread, whenever the session hears a
/// different set of entitlements; [`adopt_entitlements`] takes it on.
pub fn on_entitlements(listener: impl Fn() + Send + Sync + 'static) {
    rithmic::on_entitlements(listener);
}

fn adopt(exchanges: Option<Vec<String>>) -> bool {
    let mut held = ENTITLED.write().unwrap_or_else(|e| e.into_inner());
    let changed = *held != exchanges;
    *held = exchanges;
    changed
}

/// Signed out in this process with a development login, which is a file
/// and cannot be forgotten: until the next sign-in it counts as no login.
static SIGNED_OUT: AtomicBool = AtomicBool::new(false);

/// The desktop keyring, through the Secret Service. Tests hand in a
/// [`Secrets`] of their own, so that nothing they do reaches anybody's
/// keyring.
pub struct Keyring;

impl Secrets for Keyring {
    fn save(&self, login: &str, user: &str, system: &str, password: &str) -> Result<(), String> {
        rithmic::keyring::save(login, user, system, password).map_err(|error| error.to_string())
    }

    fn forget(&self, login: &str, user: &str, system: &str) -> Result<(), String> {
        rithmic::keyring::forget(login, user, system).map(|_| ()).map_err(|error| error.to_string())
    }
}

/// Is a development login in charge? Then the keyring is never read or
/// written, whatever is typed.
pub fn development() -> bool {
    std::env::var_os(rithmic::DEV_LOGIN).is_some_and(|path| !path.is_empty())
}

/// Keep `password` for the login `login`, `user` on `system`, in `secrets`.
/// With a development login nothing is written: the file wins, and working
/// on the app must never put a login in anybody's keyring.
pub fn save_password(
    secrets: &dyn Secrets,
    login: &str,
    user: &str,
    system: &str,
    password: &str,
) -> Result<(), String> {
    if development() {
        return Ok(());
    }
    secrets.save(login, user.trim(), system.trim(), password)
}

/// Forget the password of the login `login` in `secrets`. With a
/// development login nothing is touched, as nothing was written.
pub fn forget_password(secrets: &dyn Secrets, login: &str, user: &str, system: &str) -> Result<(), String> {
    if development() {
        return Ok(());
    }
    secrets.forget(login, user.trim(), system.trim())
}

/// What the app has saved. Called whenever it reads its settings for a
/// feed list that names Rithmic, and after a sign-in or a sign-out.
pub fn remember(saved: Saved) {
    *SAVED.write().unwrap_or_else(|e| e.into_inner()) = Some(saved);
}

fn saved() -> Saved {
    SAVED.read().unwrap_or_else(|e| e.into_inner()).clone().unwrap_or_default()
}

/// This process holds the window: see [`crate::providers::serve_from_here`].
pub fn serve_from_here() {
    crate::providers::serve_from_here();
}

/// May this process chart from Rithmic at all? Only the one holding the
/// window does, so that the bar widget and a command run with no window open
/// never open a second session beside it.
pub fn here() -> bool {
    crate::providers::here()
}

fn system() -> String {
    let saved = saved();
    match saved.value(SYSTEM).map(str::trim) {
        Some(system) if !system.is_empty() => system.to_string(),
        _ => rithmic::TEST_SYSTEM.into(),
    }
}

fn gateway() -> String {
    let saved = saved();
    match saved.value(GATEWAY).map(str::trim) {
        Some(url) if !url.is_empty() => url.to_string(),
        _ => rithmic::TEST_URL.into(),
    }
}

fn user() -> Option<String> {
    saved().user.map(|user| user.trim().to_string()).filter(|user| !user.is_empty())
}

fn login_id() -> String {
    saved().login.unwrap_or_default()
}

/// The login to sign in with, read again every time the session signs in.
pub fn login() -> Result<rithmic::Login, rithmic::Error> {
    if SIGNED_OUT.load(Ordering::Acquire) {
        return Err(rithmic::Error::NoLogin("signed out".into()));
    }
    if let Some(development) = rithmic::Login::from_env(&system(), &gateway()) {
        return development;
    }
    match user() {
        Some(user) => Ok(rithmic::Login::Keyring { login: login_id(), user, system: system(), url: gateway() }),
        None => Err(rithmic::Error::NoLogin("no Rithmic user ID is saved".into())),
    }
}

/// The data login's market data, started on first use: the data half of
/// its one connection, which its trading shares.
pub fn client() -> Result<rithmic::Client, rithmic::Error> {
    hear_specs();
    Ok(rithmic::connection(app()?, &login_id(), login)?.data())
}

/// The data login's market data as the charts have it open now, borrowed:
/// `None` while no chart has opened it, and never opened here. What a
/// trading login hears prices on, so that no login has a second data
/// session beside the charts' (CME bills a third concurrent one as a
/// professional's).
pub fn shared_client() -> Option<rithmic::Client> {
    rithmic::open_data(&login_id())
}

/// Have what Rithmic says a contract is reach [`crate::specs`], and what
/// it says a root's front month is reach [`crate::contracts`], from any
/// connection in this process: once, before the first one opens. And the
/// other way: when Rithmic will not say which contract is the front month,
/// the engine's roll rule chooses among the ones it lists.
pub fn hear_specs() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        rithmic::on_reference(|contract, _exchange, said| {
            let heard = crate::specs::Heard { tick: said.tick, point: said.point, currency: said.currency.clone() };
            crate::specs::supply("Rithmic", crate::specs::root_of(contract), heard);
        });
        rithmic::on_front_month(|root, _exchange, contract| crate::contracts::supply_front(root, contract));
        rithmic::choose_front_month_with(|root, _exchange, listed| {
            let listed: Vec<(String, chrono::NaiveDate)> = listed
                .iter()
                .filter_map(|(symbol, (year, month, day))| {
                    Some((symbol.clone(), chrono::NaiveDate::from_ymd_opt(*year, *month, *day)?))
                })
                .collect();
            crate::contracts::choose(root, &listed, crate::contracts::today())
        });
    });
}

/// Rithmic turned the login down: remembered, so that the settings and
/// `provider status` can say so in Rithmic's words, until a sign-in.
pub fn refused(why: &str) {
    *REFUSED.lock().unwrap_or_else(|e| e.into_inner()) = Some(why.to_string());
}

/// Rithmic's own words for the last refusal, such as "permission denied".
pub fn refusal() -> Option<String> {
    REFUSED.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

/// What is saved, without connecting to anything. The user ID is never
/// part of the line: it is read back on screens and in command output.
pub fn access() -> Access {
    let system = system();
    if let Some(development) = rithmic::Login::from_env(&system, &gateway()) {
        if SIGNED_OUT.load(Ordering::Acquire) {
            return Access::Missing;
        }
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
        None => Err("No Rithmic login is signed in. Add one, or sign one in, in Preferences.".into()),
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
        let login = login_id();
        let held = rithmic::keyring::holds(&login, &user, &system).map_err(|error| error.to_string())?;
        if !held {
            return Err(format!(
                "the keyring holds no Rithmic password for {system}. Sign the login in again in \
                 Preferences, or save it with: secret-tool store --label='Omacharts: \
                 Rithmic login' application omacharts service rithmic login '{login}' system \
                 '{system}' user <your user ID>"
            ));
        }
        log(&format!("found the password for {system} in the keyring"));
    }
    *REFUSED.lock().unwrap_or_else(|e| e.into_inner()) = None;
    SIGNED_OUT.store(false, Ordering::Release);
    rithmic::shutdown();
    Ok(())
}

/// Sign the process's one session in now, and wait for Rithmic's answer:
/// what the settings do after [`sign_in`], so that the row can say how it
/// went. The session is the one every chart shares, never a second; a
/// process that does not hold the window has none to sign in, and is told
/// so. A refusal is remembered in Rithmic's words, which is what comes
/// back.
pub fn connect() -> Result<(), String> {
    if !here() {
        return Err("only the window signs in to Rithmic".into());
    }
    match client().and_then(|client| client.sign_in()) {
        Ok(()) => Ok(()),
        Err(rithmic::Error::Refused(why)) => {
            refused(&why);
            Err(why)
        }
        Err(error) => Err(error.to_string()),
    }
}

/// Sign out: the session ends, and the password leaves the keyring. The
/// user ID is the app's to forget, which it does beside this.
pub fn sign_out() -> Result<(), String> {
    sign_out_of(&Keyring)
}

/// [`sign_out`], forgetting the password in `secrets`.
pub fn sign_out_of(secrets: &dyn Secrets) -> Result<(), String> {
    let remembered = user().map(|user| (login_id(), user, system()));
    end_and_forget(secrets, remembered)
}

/// [`sign_out_of`] for the login `login`, `user` on `system`, named by the
/// caller rather than read from what was last remembered — which is the
/// process's, and so somebody else's in a test running beside this one.
pub fn sign_out_login(secrets: &dyn Secrets, login: &str, user: &str, system: &str) -> Result<(), String> {
    let named = (!user.trim().is_empty()).then(|| (login.to_string(), user.trim().to_string(), system.trim().to_string()));
    end_and_forget(secrets, named)
}

fn end_and_forget(secrets: &dyn Secrets, login: Option<(String, String, String)>) -> Result<(), String> {
    // Signed out, the login's connection ends, its trading with its data.
    if let Some((login, _, _)) = &login {
        rithmic::close(login);
    }
    rithmic::shutdown();
    *REFUSED.lock().unwrap_or_else(|e| e.into_inner()) = None;
    let forgotten = match (development(), login) {
        // A development login is a file of the developer's own, and is
        // never the keyring's. It counts as no login until the next sign-in.
        (true, _) => {
            SIGNED_OUT.store(true, Ordering::Release);
            Ok(())
        }
        (false, None) => Ok(()),
        (false, Some((login, user, system))) => secrets.forget(&login, &user, &system),
    };
    let mut saved = SAVED.write().unwrap_or_else(|e| e.into_inner());
    if let Some(saved) = saved.as_mut() {
        saved.user = None;
    }
    forgotten
}

/// The data login is another login now, or Rithmic is no longer a data
/// source: the session signed in through the old one ends, and its refusal
/// with it, so the next chart signs in through what [`remember`] is told.
pub fn hand_over() {
    rithmic::shutdown();
    *REFUSED.lock().unwrap_or_else(|e| e.into_inner()) = None;
    SIGNED_OUT.store(false, Ordering::Release);
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
        format!("application=omacharts service=rithmic login={}", login_id()),
    )];
    if let Some(path) = std::env::var_os(rithmic::DEV_LOGIN).filter(|path| !path.is_empty()) {
        places.push(("Development login", std::path::PathBuf::from(path).display().to_string()));
    }
    places
}
