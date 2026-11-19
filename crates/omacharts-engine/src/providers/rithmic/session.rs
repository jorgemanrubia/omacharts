//! Signing in to Rithmic, as the settings, the command line and the charts
//! see it.
//!
//! The app keeps any number of Rithmic logins, and each is a data feed of
//! its own ([`crate::providers::login_feed`]): its own market data
//! session, its own entitlements and its own refusal, so that one login
//! turned down for data says so on its own row and the others chart on.
//! Each login's id, user ID, system and gateway are settings the app keeps,
//! `Local` because a login belongs to the machine it was typed on, and
//! hands to this module with [`remember`], by login: the engine has no
//! database of its own. The password is in the desktop keyring under the
//! login's id, and is read only when the session signs in.
//!
//! A session is opened only when a chart asks its feed for something, so a
//! login whose feed no chart is drawn from has no data session at all: CME
//! bills a second concurrent session on a login as a professional's, and
//! each login here has one at most.
//!
//! A developer's login is a file instead, named by
//! [`rithmic::DEV_LOGIN`], and when that is set it wins: work on the app
//! never reads or writes anybody's keyring.
//!
//! Nothing here connects. What is saved is read from memory, the
//! environment and at most one file, which is what lets a chart be routed,
//! and a settings row drawn, without a round trip to Chicago.

use std::collections::{HashMap, HashSet};
use std::sync::{Mutex, RwLock};

pub use crate::providers::{Saved, Secrets};
use crate::providers::{Access, How, Sessions, SignIn};

/// The app name Rithmic is told, the one place it is set.
///
/// Rithmic Test takes any name; production takes only the one Rithmic
/// agreed for the app when it passed conformance, which is `omacharts`,
/// with no prefix. A build can set another with
/// `OMACHARTS_RITHMIC_APP_NAME`.
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

/// Signing in to Rithmic, as the boundary asks it of every feed: for one
/// login, or, for the entry in the list of feeds, for none, which has
/// nobody to sign in with unless a development login says otherwise.
#[derive(Debug)]
pub struct Signing {
    login: Option<&'static str>,
}

/// The entry's: no login.
pub static SIGNING: Signing = Signing { login: None };

impl Signing {
    fn login(&self) -> &str {
        self.login.unwrap_or_default()
    }
}

/// Each login's [`Signing`], made once.
fn signing_for(login: &'static str) -> &'static Signing {
    static MADE: Mutex<Vec<&'static Signing>> = Mutex::new(Vec::new());
    let mut made = MADE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(signing) = made.iter().find(|signing| signing.login == Some(login)) {
        return signing;
    }
    let signing: &'static Signing = Box::leak(Box::new(Signing { login: Some(login) }));
    made.push(signing);
    signing
}

impl SignIn for Signing {
    fn how(&self) -> How {
        How::Dialog
    }

    /// CME bills a second concurrent session on a login as a professional
    /// subscription.
    fn sessions(&self) -> Sessions {
        Sessions::OnePerLogin
    }

    /// A prop firm's login is for its orders as much as for its charts.
    fn trades(&self) -> bool {
        true
    }

    fn access(&self) -> Access {
        access(self.login())
    }

    fn can_sign_in(&self) -> Result<String, String> {
        can_sign_in(self.login())
    }

    fn sign_in(&self, log: Box<dyn Fn(&str) + Send + Sync>) -> Result<(), String> {
        sign_in(self.login(), log)
    }

    fn connect(&self) -> Result<(), String> {
        connect(self.login())
    }

    fn sign_out(&self) -> Result<(), String> {
        sign_out(self.login())
    }

    fn connected(&self) -> bool {
        connected(self.login())
    }

    fn places(&self) -> Vec<(&'static str, String)> {
        places(self.login())
    }

    fn remember(&self, saved: Saved) {
        remember(self.login(), saved)
    }

    fn hand_over(&self) {
        hand_over(self.login())
    }

    fn refusal(&self) -> Option<String> {
        refusal(self.login())
    }

    fn keyring(&self) -> Option<&'static dyn Secrets> {
        Some(&Keyring)
    }

    fn for_login(&self, login: &'static str) -> Option<&'static dyn SignIn> {
        Some(signing_for(login))
    }
}

/// What the app has saved, by login.
static SAVED: RwLock<Option<HashMap<String, Saved>>> = RwLock::new(None);

/// Why Rithmic last turned each login down in this process, in its words,
/// until somebody signs it in again.
static REFUSED: Mutex<Option<HashMap<String, String>>> = Mutex::new(None);

/// The exchanges each login is entitled to, as the window last adopted
/// them: a login missing is one no session has said of yet, which charts
/// as if every CME group exchange were. See [`adopt_entitlements`].
static ENTITLED: RwLock<Option<HashMap<String, Vec<String>>>> = RwLock::new(None);

/// May the login `login` chart from `exchange`? Yes until a session of its
/// own has said otherwise. Pure and offline: a read of what was adopted.
pub fn entitled(login: &str, exchange: &str) -> bool {
    ENTITLED
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
        .and_then(|all| all.get(login))
        .is_none_or(|exchanges| exchanges.iter().any(|e| e == exchange))
}

/// Take on what every login's session last heard about entitlements, so
/// that [`entitled`], and with it routing, follows it. Separate from the
/// sessions hearing it, so the window can note where every chart is served
/// from first, and move only the charts whose source changes. True when it
/// changed anything.
pub fn adopt_entitlements() -> bool {
    let mut logins: Vec<String> =
        SAVED.read().unwrap_or_else(|e| e.into_inner()).as_ref().map(|all| all.keys().cloned().collect()).unwrap_or_default();
    logins.push(String::new());
    let heard: HashMap<String, Vec<String>> = logins
        .into_iter()
        .filter_map(|login| rithmic::entitled_exchanges(&login).map(|exchanges| (login, exchanges)))
        .collect();
    adopt(heard)
}

/// Call `listener`, on the session's thread, whenever a session hears a
/// different set of entitlements; [`adopt_entitlements`] takes it on.
pub fn on_entitlements(listener: impl Fn() + Send + Sync + 'static) {
    rithmic::on_entitlements(listener);
}

fn adopt(heard: HashMap<String, Vec<String>>) -> bool {
    let mut held = ENTITLED.write().unwrap_or_else(|e| e.into_inner());
    let changed = held.as_ref().map_or(!heard.is_empty(), |known| *known != heard);
    *held = Some(heard);
    changed
}

/// The logins signed out in this process with a development login, which
/// is a file and cannot be forgotten: until the next sign-in each counts as
/// no login.
static SIGNED_OUT: Mutex<Option<HashSet<String>>> = Mutex::new(None);

fn signed_out(login: &str) -> bool {
    SIGNED_OUT.lock().unwrap_or_else(|e| e.into_inner()).as_ref().is_some_and(|out| out.contains(login))
}

fn mark_signed_out(login: &str, out: bool) {
    let mut held = SIGNED_OUT.lock().unwrap_or_else(|e| e.into_inner());
    let set = held.get_or_insert_with(HashSet::new);
    if out {
        set.insert(login.to_string());
    } else {
        set.remove(login);
    }
}

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

/// What the app has saved for the login `login`. Called whenever it reads
/// its settings for a feed list that names Rithmic, and after a sign-in or
/// a sign-out.
pub fn remember(login: &str, saved: Saved) {
    SAVED.write().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(HashMap::new).insert(login.to_string(), saved);
}

fn saved(login: &str) -> Saved {
    SAVED.read().unwrap_or_else(|e| e.into_inner()).as_ref().and_then(|all| all.get(login).cloned()).unwrap_or_default()
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

fn system(login: &str) -> String {
    let saved = saved(login);
    match saved.value(SYSTEM).map(str::trim) {
        Some(system) if !system.is_empty() => system.to_string(),
        _ => rithmic::TEST_SYSTEM.into(),
    }
}

fn gateway(login: &str) -> String {
    let saved = saved(login);
    match saved.value(GATEWAY).map(str::trim) {
        Some(url) if !url.is_empty() => url.to_string(),
        _ => rithmic::TEST_URL.into(),
    }
}

fn user(login: &str) -> Option<String> {
    saved(login).user.map(|user| user.trim().to_string()).filter(|user| !user.is_empty())
}

/// The login `login` to sign in with, read again every time its session
/// signs in.
pub fn login(login: &str) -> Result<rithmic::Login, rithmic::Error> {
    if signed_out(login) {
        return Err(rithmic::Error::NoLogin("signed out".into()));
    }
    if let Some(development) = rithmic::Login::from_env(&system(login), &gateway(login)) {
        return development;
    }
    match user(login) {
        Some(user) => {
            Ok(rithmic::Login::Keyring { login: login.to_string(), user, system: system(login), url: gateway(login) })
        }
        None => Err(rithmic::Error::NoLogin("no Rithmic user ID is saved".into())),
    }
}

/// The login `login`'s market data, started on first use: the data half of
/// its one connection, which its trading shares.
pub fn client(login: &str) -> Result<rithmic::Client, rithmic::Error> {
    hear_specs();
    let owned = login.to_string();
    Ok(rithmic::connection(app()?, login, move || self::login(&owned))?.data())
}

/// Market data as the charts have it open now, borrowed: the login
/// `login`'s own session when its feed is charted from, else another
/// login's that is; `None` while no chart has opened one, and never opened
/// here. What a login's trading hears prices on, so that no login has a
/// second data session beside the charts' (CME bills a second concurrent
/// one as a professional's).
pub fn shared_client(login: &str) -> Option<rithmic::Client> {
    rithmic::open_data_or_any(login)
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

/// Rithmic turned the login `login` down: remembered, so that its row in
/// the settings and `provider status` can say so in Rithmic's words, until
/// it signs in again. The other logins chart on.
pub fn refused(login: &str, why: &str) {
    REFUSED.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(HashMap::new).insert(login.to_string(), why.to_string());
}

fn forgive(login: &str) {
    if let Some(all) = REFUSED.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
        all.remove(login);
    }
}

/// Rithmic's own words for the login's last refusal, such as "permission
/// denied".
pub fn refusal(login: &str) -> Option<String> {
    REFUSED.lock().unwrap_or_else(|e| e.into_inner()).as_ref().and_then(|all| all.get(login).cloned())
}

/// What is saved for the login, without connecting to anything. The user
/// ID is never part of the line: it is read back on screens and in command
/// output.
pub fn access(login: &str) -> Access {
    let system = system(login);
    if let Some(development) = rithmic::Login::from_env(&system, &gateway(login)) {
        if signed_out(login) {
            return Access::Missing;
        }
        return match (development, refusal(login)) {
            (Err(error), _) => Access::Expired(error.to_string()),
            (Ok(_), Some(why)) => Access::Expired(format!("Rithmic said: {why}")),
            (Ok(_), None) => Access::Signed(format!("{system} · development login")),
        };
    }
    match (user(login), refusal(login)) {
        (None, _) => Access::Missing,
        (Some(_), Some(why)) => Access::Expired(format!("Rithmic said: {why}")),
        (Some(_), None) => Access::Signed(system),
    }
}

/// How a sign-in would go, or why it cannot. Nothing opens a browser here.
pub fn can_sign_in(login: &str) -> Result<String, String> {
    if rithmic::Login::from_env(&system(login), &gateway(login)).is_some() {
        return Ok(format!("the development login {} names", rithmic::DEV_LOGIN));
    }
    match user(login) {
        Some(_) => Ok("the password in the desktop keyring".into()),
        None => Err("No Rithmic login is signed in. Add one, or sign one in, in Preferences.".into()),
    }
}

/// Check that what is saved for the login can sign in, without signing
/// in: the session is the window's, and a second one opened here to try
/// the password would be a second session as far as CME is concerned.
/// Forgets the login's earlier refusal, and ends its connection so that its
/// next chart signs in afresh with what is saved now.
pub fn sign_in(login: &str, log: impl Fn(&str)) -> Result<(), String> {
    let system = system(login);
    if let Some(development) = rithmic::Login::from_env(&system, &gateway(login)) {
        development.map_err(|error| error.to_string())?;
        log(&format!("using the development login {} names", rithmic::DEV_LOGIN));
    } else {
        let Some(user) = user(login) else {
            return Err(can_sign_in(login).err().unwrap_or_default());
        };
        let held = rithmic::keyring::holds(login, &user, &system).map_err(|error| error.to_string())?;
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
    forgive(login);
    mark_signed_out(login, false);
    rithmic::close(login);
    Ok(())
}

/// Sign the login's one session in now, and wait for Rithmic's answer:
/// what the settings do after [`sign_in`], so that the row can say how it
/// went. The session is the one the login's charts share, never a second;
/// a process that does not hold the window has none to sign in, and is
/// told so. A refusal is remembered in Rithmic's words, which is what comes
/// back.
pub fn connect(login: &str) -> Result<(), String> {
    if !here() {
        return Err("only the window signs in to Rithmic".into());
    }
    match client(login).and_then(|client| client.sign_in()) {
        Ok(()) => Ok(()),
        Err(rithmic::Error::Refused(why)) => {
            refused(login, &why);
            Err(why)
        }
        Err(error) => Err(error.to_string()),
    }
}

/// Sign the login out: its session ends, and the password leaves the
/// keyring. The user ID is the app's to forget, which it does beside this.
pub fn sign_out(login: &str) -> Result<(), String> {
    sign_out_of(&Keyring, login)
}

/// [`sign_out`], forgetting the password in `secrets`.
pub fn sign_out_of(secrets: &dyn Secrets, login: &str) -> Result<(), String> {
    let user = user(login).unwrap_or_default();
    let system = system(login);
    sign_out_login(secrets, login, &user, &system)
}

/// [`sign_out_of`] for the login `login`, `user` on `system`, named by the
/// caller rather than read from what was last remembered.
pub fn sign_out_login(secrets: &dyn Secrets, login: &str, user: &str, system: &str) -> Result<(), String> {
    // Signed out, the login's connection ends, its trading with its data.
    rithmic::close(login);
    forgive(login);
    let forgotten = if development() {
        // A development login is a file of the developer's own, and is
        // never the keyring's. It counts as no login until the next sign-in.
        mark_signed_out(login, true);
        Ok(())
    } else if user.trim().is_empty() {
        Ok(())
    } else {
        secrets.forget(login, user.trim(), system.trim())
    };
    if let Some(saved) = SAVED.write().unwrap_or_else(|e| e.into_inner()).as_mut().and_then(|all| all.get_mut(login)) {
        saved.user = None;
    }
    forgotten
}

/// The login's feed is no longer drawn from: its data session ends, and
/// its refusal with it, so that the next chart signs in afresh. Its
/// trading, which has its own half of the connection, carries on.
pub fn hand_over(login: &str) {
    rithmic::close_data(login);
    forgive(login);
    mark_signed_out(login, false);
}

/// Has this process a data session through the login?
pub fn connected(login: &str) -> bool {
    rithmic::feeding_login(login)
}

/// Where the login is kept, for somebody who would rather see it than
/// guess.
pub fn places(login: &str) -> Vec<(&'static str, String)> {
    let mut places = vec![("Keyring item", format!("application=omacharts service=rithmic login={login}"))];
    if let Some(path) = std::env::var_os(rithmic::DEV_LOGIN).filter(|path| !path.is_empty()) {
        places.push(("Development login", std::path::PathBuf::from(path).display().to_string()));
    }
    places
}

#[cfg(test)]
mod tests {
    /// Production is told `omacharts`, the name Rithmic agreed at
    /// conformance, unless a build names another.
    #[test]
    fn rithmic_is_told_the_name_agreed_at_conformance() {
        if option_env!("OMACHARTS_RITHMIC_APP_NAME").is_none() {
            assert_eq!(super::APP_NAME, "omacharts");
        }
    }

    /// Each login is entitled to what its own session said: one turned
    /// down for NYMEX leaves another login's NYMEX alone, and a login no
    /// session has spoken for yet charts as if entitled to all of it.
    #[test]
    fn entitlements_are_each_logins_own() {
        let heard = [("acme-entitled".to_string(), vec!["CME".to_string(), "CBOT".to_string()])].into();
        super::adopt(heard);
        assert!(super::entitled("acme-entitled", "CME"));
        assert!(!super::entitled("acme-entitled", "NYMEX"));
        assert!(super::entitled("demo-unheard", "NYMEX"));
    }

    /// A refusal is the login's own, and the other logins' rows stay clear.
    #[test]
    fn a_refusal_is_the_logins_own() {
        super::refused("acme-refused", "permission denied");
        assert_eq!(super::refusal("acme-refused").as_deref(), Some("permission denied"));
        assert_eq!(super::refusal("demo-clear"), None);
        assert!(matches!(super::access("acme-refused"), super::Access::Missing | super::Access::Expired(_)));
        super::forgive("acme-refused");
        assert_eq!(super::refusal("acme-refused"), None);
    }
}
