//! What is stored: the logins, their accounts as last listed, the default
//! account and the data login, and changing them.
//!
//! A login names its broker, and whatever is particular to that broker —
//! where its password goes, which data feed its data comes through — is
//! asked of the broker's adapter and of that feed's [`SignIn`], never
//! decided here.

use omacharts_engine::providers::{self, Listed, SignIn};
pub use omacharts_engine::providers::Secrets;

use super::*;
use crate::store::Store;

/// The logins, as a JSON list in the order they were added.
pub const LOGINS: &str = "logins";

/// The id of the login market data comes through. Missing, or naming a
/// login that is gone, is the first login with a data feed.
pub const DATA_LOGIN: &str = "data_login";

/// Every login's accounts as last listed, balances included.
pub const ACCOUNTS: &str = "trading_accounts";

/// The account a chart is about when nobody chose one for it, as
/// `<login id>/<account number>`. One that is no longer listed is ignored.
pub const DEFAULT_ACCOUNT: &str = "default_account";

/// A login as it is typed into its broker's dialog.
///
/// No `Debug`, so that it cannot be printed by accident: the password is in
/// it.
pub struct Typed {
    /// The broker whose dialog it was typed into, by its id.
    pub broker: String,
    pub name: String,
    pub user: String,
    pub password: String,
    /// The rest of what the dialog asks, by name, as its broker reads it.
    pub values: Vec<(String, String)>,
}

/// The logins, in the order they were added, less those of a broker that
/// is closed now — behind a flag that is off — which are kept as they are
/// and are nobody's business until it is on.
pub fn logins(store: &Store) -> Vec<Login> {
    let mut logins = kept(store);
    let brokers = super::registry();
    logins.retain(|login| !brokers.closed(&login.broker));
    logins
}

/// Every login kept, open or closed: what a change is made to and written
/// back, so that a closed broker's logins survive it.
fn kept(store: &Store) -> Vec<Login> {
    let logins = store
        .setting(LOGINS)
        .and_then(|json| serde_json::from_str::<Vec<Login>>(&json).ok())
        .unwrap_or_default();
    // Whatever a broker or a feed repeats back, a user ID never reaches the
    // Log.
    crate::log_words::conceal_logins(&logins);
    logins
}

fn save_logins(store: &Store, logins: &[Login]) {
    match logins.is_empty() {
        true => store.remove_setting(LOGINS),
        false => store.set_setting(LOGINS, &serde_json::to_string(logins).unwrap_or_default()),
    }
}

pub(crate) fn login_in<'a>(logins: &'a [Login], id: &str) -> Option<&'a Login> {
    logins.iter().find(|login| login.id == id)
}

/// The data feed `login`'s market data comes through, when its broker has
/// one: the feed listed under the broker's id whose login is a broker
/// connection.
pub fn data_feed_of(login: &Login) -> Option<&'static Listed> {
    providers::listed(&login.broker).filter(|feed| feed.setup.is_some_and(|setup| setup.trades()))
}

fn sign_in_of(login: &Login) -> Option<&'static dyn SignIn> {
    data_feed_of(login).and_then(|feed| feed.setup)
}

/// The id of the data login: the one chosen if it is still there, else the
/// first whose broker has a data feed.
pub fn data_login_id(store: &Store, logins: &[Login]) -> Option<String> {
    let chosen = store.setting(DATA_LOGIN);
    let feeds_data = |login: &&Login| data_feed_of(login).is_some();
    chosen
        .filter(|id| login_in(logins, id).is_some_and(|login| feeds_data(&login)))
        .or_else(|| logins.iter().find(feeds_data).map(|login| login.id.clone()))
}

/// The login market data comes through, if there is one.
pub fn data_login(store: &Store) -> Option<Login> {
    let logins = logins(store);
    let id = data_login_id(store, &logins)?;
    login_in(&logins, &id).cloned()
}

/// Every login's accounts, as last listed, in the order of the logins.
/// Accounts of a login that is gone are left out.
pub fn accounts(store: &Store) -> Vec<Account> {
    let cached: Vec<Account> = store
        .setting(ACCOUNTS)
        .and_then(|json| serde_json::from_str(&json).ok())
        .unwrap_or_default();
    let logins = logins(store);
    let mut ordered = Vec::with_capacity(cached.len());
    for login in &logins {
        ordered.extend(cached.iter().filter(|account| account.login == login.id).cloned());
    }
    ordered
}

fn save_accounts(store: &Store, accounts: &[Account]) {
    match accounts.is_empty() {
        true => store.remove_setting(ACCOUNTS),
        false => store.set_setting(ACCOUNTS, &serde_json::to_string(accounts).unwrap_or_default()),
    }
}

/// Keep `listed` as the accounts of the login `login`, in place of what was
/// listed before. A balance not known yet keeps the one last known.
pub fn cache_accounts(store: &Store, login: &str, listed: Vec<Account>) {
    let mut accounts = accounts(store);
    let before: Vec<Account> = accounts.iter().filter(|account| account.login == login).cloned().collect();
    accounts.retain(|account| account.login != login);
    for mut account in listed {
        account.login = login.to_string();
        if account.balance.is_none() {
            account.balance = before.iter().find(|known| known.id == account.id).and_then(|known| known.balance);
        }
        accounts.push(account);
    }
    save_accounts(store, &accounts);
}

/// Note a balance the broker pushed. True when it changed anything.
pub fn note_balance(store: &Store, login: &str, account: &str, balance: f64) -> bool {
    let mut accounts = accounts(store);
    let Some(found) = accounts.iter_mut().find(|known| known.login == login && known.id == account) else {
        return false;
    };
    if found.balance == Some(balance) {
        return false;
    }
    found.balance = Some(balance);
    save_accounts(store, &accounts);
    true
}

/// The account a chart nobody chose for is about: the one made default if
/// it is still listed, else the first.
pub fn default_account<'a>(accounts: &'a [Account], stored: Option<&str>) -> Option<&'a Account> {
    stored
        .and_then(|key| accounts.iter().find(|account| account.key() == key))
        .or_else(|| accounts.first())
}

pub fn set_default_account(store: &Store, key: &str) {
    store.set_setting(DEFAULT_ACCOUNT, key);
}

/// An id no login has had on this machine: the time, in microseconds.
fn new_id(logins: &[Login]) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|since| since.as_micros())
        .unwrap_or_default();
    let mut id = format!("{now:x}");
    while login_in(logins, &id).is_some() {
        id.push('x');
    }
    id
}

/// What was typed, onto `login`. A login typed with no name is called what
/// its broker is.
fn typed_into(login: &mut Login, typed: &Typed) {
    login.user = typed.user.trim().to_string();
    login.values = typed.values.iter().map(|(key, value)| (key.clone(), value.trim().to_string())).collect();
    login.name = match typed.name.trim() {
        "" => broker_for(&login.broker).map_or_else(|| login.broker.clone(), |broker| broker.label().to_string()),
        name => name.to_string(),
    };
    login.signed_out = false;
}

fn no_broker(login: &Login) -> String {
    format!("there is no broker called {}", login.broker)
}

fn keep_password(secrets: &dyn Secrets, login: &Login, password: &str) -> Result<(), String> {
    if password.is_empty() || login.user.is_empty() {
        return Ok(());
    }
    broker_for(&login.broker).ok_or_else(|| no_broker(login))?.keep_password(secrets, login, password)
}

/// Forget `login`'s password; for the data login, end its data session
/// too.
fn forget_password(secrets: &dyn Secrets, login: &Login, data: bool) -> Result<(), String> {
    match broker_for(&login.broker) {
        Some(broker) => broker.forget_password(secrets, login, data),
        None => Ok(()),
    }
}

/// The data login is another now, or none: the data session signed in
/// through the one before, `login`'s, ends.
fn hand_over(login: &Login) {
    if let Some(setup) = sign_in_of(login) {
        setup.hand_over();
    }
}

/// Add a login, its password into `secrets`. The first with a data feed is
/// the data login, and puts its feed on top of the data feeds. Returns
/// its id.
pub fn add_login(store: &Store, secrets: &dyn Secrets, typed: &Typed) -> Result<String, String> {
    let mut logins = kept(store);
    let mut login = Login {
        id: new_id(&logins),
        name: String::new(),
        broker: typed.broker.clone(),
        user: String::new(),
        values: Vec::new(),
        signed_out: false,
    };
    typed_into(&mut login, typed);
    crate::log_words::conceal_logins(std::slice::from_ref(&login));
    let first = data_login_id(store, &logins).is_none();
    let feed = data_feed_of(&login);
    let id = login.id.clone();
    let kept = keep_password(secrets, &login, &typed.password);
    logins.push(login);
    save_logins(store, &logins);
    if first && let Some(feed) = feed {
        store.set_setting(DATA_LOGIN, &id);
        crate::feeds::put_on_top(store, feed);
    }
    crate::feeds::remember_logins(store);
    kept.map(|_| id)
}

/// Retype a login: what the dialog had when somebody signed it in again. An
/// empty password keeps the one the keyring holds.
pub fn retype_login(store: &Store, secrets: &dyn Secrets, id: &str, typed: &Typed) -> Result<(), String> {
    let mut logins = kept(store);
    let Some(login) = logins.iter_mut().find(|login| login.id == id) else {
        return Err("that login has been removed".into());
    };
    typed_into(login, typed);
    let kept = keep_password(secrets, login, &typed.password);
    save_logins(store, &logins);
    crate::feeds::remember_logins(store);
    kept
}

/// Sign a login out: its password leaves the keyring, and it stays listed,
/// signed out, with its accounts. The data login's data session ends too.
pub fn sign_out_login(store: &Store, secrets: &dyn Secrets, id: &str) -> Result<(), String> {
    let mut logins = kept(store);
    let data = data_login_id(store, &logins);
    let Some(login) = logins.iter_mut().find(|login| login.id == id) else { return Ok(()) };
    // The login is named rather than read back from what the engine was
    // last told: that is the process's, and another test's beside this one.
    let forgotten = forget_password(secrets, login, data.as_deref() == Some(id));
    login.signed_out = true;
    save_logins(store, &logins);
    crate::feeds::remember_logins(store);
    forgotten
}

/// Mark a login signed out, its password already forgotten by whoever
/// signed it out.
pub fn mark_signed_out(store: &Store, id: &str) {
    let mut logins = kept(store);
    if let Some(login) = logins.iter_mut().find(|login| login.id == id)
        && !login.signed_out
    {
        login.signed_out = true;
        save_logins(store, &logins);
    }
}

/// Make `id` the login market data comes through. The data session signed
/// in through the one before ends, and the next chart signs in through this.
pub fn make_data_login(store: &Store, id: &str) {
    let logins = logins(store);
    let before = data_login_id(store, &logins);
    let Some(login) = login_in(&logins, id).filter(|login| data_feed_of(login).is_some()) else { return };
    if before.as_deref() == Some(id) {
        return;
    }
    store.set_setting(DATA_LOGIN, id);
    hand_over(before.as_deref().and_then(|before| login_in(&logins, before)).unwrap_or(login));
    crate::feeds::remember_logins(store);
}

/// What removing a login does to its data feed, which the dialog asking to
/// remove it says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Removal {
    /// Not the data login: the data feed is not touched.
    Login,
    /// The data login, and another login takes the data over: this one.
    HandOver { to: String },
    /// The data login, and the last: whether its data feed goes too is
    /// asked.
    Ask,
    /// The data login, the last, and its feed is the last data feed, which
    /// cannot be removed: it stays, not signed in.
    KeepFeed,
}

/// What removing the login `id` would do to the data feed.
pub fn removal(store: &Store, id: &str) -> Removal {
    let logins = logins(store);
    if data_login_id(store, &logins).as_deref() != Some(id) {
        return Removal::Login;
    }
    let Some(feed) = login_in(&logins, id).and_then(data_feed_of) else { return Removal::Login };
    if let Some(next) = logins.iter().find(|login| login.id != id && data_feed_of(login).is_some()) {
        return Removal::HandOver { to: next.name.clone() };
    }
    let fed = crate::feeds::stored_order(store).iter().any(|known| known.eq_ignore_ascii_case(feed.id));
    match fed && crate::feeds::removable(store, feed.id) {
        true => Removal::Ask,
        false => Removal::KeepFeed,
    }
}

/// Remove a login, its password and its accounts. If market data came
/// through it, it comes through the next login now; with none left its data
/// feed goes too when `feed_too` says so and it is not the last data feed,
/// and otherwise stays, not signed in, to be signed in again.
pub fn remove_login(store: &Store, secrets: &dyn Secrets, id: &str, feed_too: bool) -> Result<(), String> {
    let decided = removal(store, id);
    let mut logins = kept(store);
    let Some(at) = logins.iter().position(|login| login.id == id) else { return Ok(()) };
    let was_data = decided != Removal::Login;
    let login = logins.remove(at);
    let forgotten = forget_password(secrets, &login, was_data);
    save_logins(store, &logins);

    let mut accounts = accounts(store);
    accounts.retain(|account| account.login != id);
    save_accounts(store, &accounts);
    if store.setting(DEFAULT_ACCOUNT).is_some_and(|key| key.split_once('/').is_some_and(|(login, _)| login == id)) {
        store.remove_setting(DEFAULT_ACCOUNT);
    }

    match decided {
        Removal::Login => {}
        Removal::HandOver { .. } => {
            if let Some(next) = logins.iter().find(|login| data_feed_of(login).is_some()) {
                store.set_setting(DATA_LOGIN, &next.id);
            }
            hand_over(&login);
        }
        Removal::Ask | Removal::KeepFeed => {
            store.remove_setting(DATA_LOGIN);
            match (decided == Removal::Ask && feed_too, data_feed_of(&login)) {
                (true, Some(feed)) => {
                    crate::feeds::remove_feed(store, feed.id);
                }
                _ => hand_over(&login),
            }
        }
    }
    crate::feeds::remember_logins(store);
    forgotten
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trading::fake::tests::*;
    use crate::trading::fake::*;
    use std::sync::Arc;

    /// Adding a login keeps its password under its own id and nowhere in a
    /// setting. The first is the data login and puts Rithmic on top of the
    /// data feeds, above one already there.
    #[test]
    fn the_first_login_is_the_data_login_and_puts_rithmic_on_top() {
        if skip() {
            return;
        }
        let store = store();
        crate::feeds::add_feed(&store, "tos");
        let kept = Kept::default();
        let lucid = add_login(&store, &kept, &typed("Lucid", "trader")).unwrap();
        assert_eq!(crate::feeds::stored_order(&store), vec!["rithmic", "tos", "yahoo"]);
        assert_eq!(data_login(&store).map(|l| l.id), Some(lucid.clone()));
        assert_eq!(kept.0.borrow().as_slice(), &[(lucid.clone(), "PLANTED-PASSWORD".to_string())]);
        assert!(!store.setting(LOGINS).unwrap().contains("PLANTED"));
        let login = &logins(&store)[0];
        assert_eq!((login.name.as_str(), login.value("system")), ("Lucid", Some(TEST_SYSTEM)));
        assert!(!format!("{login:?}").contains("trader"), "a user ID in Debug");

        // A second login is not the data login, and leaves the order alone.
        crate::feeds::save_order(&store, &["tos".into(), "rithmic".into()]);
        let tradeify = add_login(&store, &kept, &typed("", "other")).unwrap();
        assert_ne!(tradeify, lucid);
        assert_eq!(data_login(&store).map(|l| l.id), Some(lucid));
        assert_eq!(crate::feeds::stored_order(&store), vec!["tos", "rithmic"]);
        assert_eq!(logins(&store)[1].name, "Rithmic", "no name is the broker's");
    }

    /// Exactly one login is the data login, whichever was picked, and the
    /// engine signs charts in through it alone.
    #[test]
    fn one_login_is_the_data_login_and_the_session_signs_in_through_it() {
        if skip() {
            return;
        }
        let store = store();
        let kept = Kept::default();
        let lucid = add_login(&store, &kept, &typed("Lucid", "lucid-user")).unwrap();
        let test = add_login(&store, &kept, &typed("Rithmic Test", "test-user")).unwrap();
        // What the engine is handed, rather than what it holds: that is
        // one per process, and other tests hand it theirs.
        let through = || crate::feeds::saved_of(&store).login.unwrap_or_default();
        assert_eq!(through(), lucid);
        make_data_login(&store, &test);
        assert_eq!(data_login_id(&store, &logins(&store)), Some(test.clone()));
        assert_eq!(through(), test);
        make_data_login(&store, "gone");
        assert_eq!(data_login_id(&store, &logins(&store)), Some(test.clone()), "only a login there is");
        // A stored choice that names nobody is the first login.
        store.set_setting(DATA_LOGIN, "nobody");
        assert_eq!(data_login_id(&store, &logins(&store)), Some(lucid));
    }

    /// Removing the data login hands data to the next one, with nothing to
    /// ask. Each takes its accounts and its password with it.
    #[test]
    fn removing_the_data_login_hands_data_to_another() {
        if skip() {
            return;
        }
        let store = store();
        let kept = Kept::default();
        let lucid = add_login(&store, &kept, &typed("Lucid", "a")).unwrap();
        let test = add_login(&store, &kept, &typed("Rithmic Test", "b")).unwrap();
        cache_accounts(&store, &lucid, vec![account(&lucid, "1234"), account(&lucid, "1235")]);
        cache_accounts(&store, &test, vec![account(&test, "951")]);
        set_default_account(&store, &format!("{lucid}/1235"));

        assert_eq!(removal(&store, &test), Removal::Login, "not the data login");
        assert_eq!(removal(&store, &lucid), Removal::HandOver { to: "Rithmic Test".into() });
        remove_login(&store, &kept, &lucid, true).unwrap();
        assert_eq!(data_login(&store).map(|l| l.id), Some(test.clone()));
        assert_eq!(accounts(&store).iter().map(|a| a.id.as_str()).collect::<Vec<_>>(), vec!["951"]);
        assert_eq!(store.setting(DEFAULT_ACCOUNT), None, "the default went with its login");
        assert_eq!(kept.0.borrow().iter().map(|(l, _)| l.clone()).collect::<Vec<_>>(), vec![test.clone()]);
        assert_eq!(crate::feeds::stored_order(&store), vec!["rithmic", "yahoo"], "the feed stays, through the other");
    }

    /// The last login, asked: keeping the data feed leaves Rithmic on the
    /// Data page with nobody signed in; removing both takes it off.
    #[test]
    fn removing_the_last_login_keeps_the_data_feed_or_removes_both() {
        if skip() {
            return;
        }
        for feed_too in [false, true] {
            let store = store();
            let kept = Kept::default();
            let lucid = add_login(&store, &kept, &typed("Lucid", "a")).unwrap();
            assert_eq!(removal(&store, &lucid), Removal::Ask);
            remove_login(&store, &kept, &lucid, feed_too).unwrap();
            assert!(logins(&store).is_empty());
            assert!(kept.0.borrow().is_empty());
            assert_eq!(store.setting(ACCOUNTS), None);
            assert_eq!(store.setting(DATA_LOGIN), None);
            assert_eq!(crate::feeds::saved_of(&store).user, None, "nobody to sign in with");
            let order = crate::feeds::stored_order(&store);
            match feed_too {
                false => assert_eq!(order, vec!["rithmic", "yahoo"], "kept, not signed in"),
                true => assert_eq!(order, vec!["yahoo"], "removed both"),
            }
        }
    }

    /// When Rithmic is the last data feed, removing it is not offered: the
    /// login goes and the feed stays, even if asked to go.
    #[test]
    fn the_last_data_feed_stays_when_its_login_goes() {
        if skip() {
            return;
        }
        let store = store();
        let kept = Kept::default();
        let lucid = add_login(&store, &kept, &typed("Lucid", "a")).unwrap();
        assert!(crate::feeds::remove_feed(&store, "yahoo"));
        assert_eq!(removal(&store, &lucid), Removal::KeepFeed);
        remove_login(&store, &kept, &lucid, true).unwrap();
        assert!(logins(&store).is_empty());
        assert_eq!(crate::feeds::stored_order(&store), vec!["rithmic"]);
    }

    /// Signing a login out forgets its password and keeps it listed, with
    /// its accounts; the data login's charts are told there is no login.
    #[test]
    fn signing_out_keeps_the_login_and_forgets_the_password() {
        if skip() {
            return;
        }
        let store = store();
        let kept = Kept::default();
        let lucid = add_login(&store, &kept, &typed("Lucid", "a")).unwrap();
        cache_accounts(&store, &lucid, vec![account(&lucid, "1234")]);
        sign_out_login(&store, &kept, &lucid).unwrap();
        assert!(kept.0.borrow().is_empty());
        assert!(logins(&store)[0].signed_out);
        assert_eq!(accounts(&store).len(), 1);
        assert_eq!(crate::feeds::saved_of(&store).user, None, "the data session has no login to sign in with");
        let desk = Desk::new(Registry::of(vec![Arc::new(FakeBroker::default())]));
        assert_eq!(desk.standing(&logins(&store)[0], true), Standing::SignedOut);

        retype_login(&store, &kept, &lucid, &typed("Lucid", "a")).unwrap();
        assert!(!logins(&store)[0].signed_out);
        assert_eq!(kept.0.borrow().len(), 1);
    }
}
