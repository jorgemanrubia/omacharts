//! What is stored: the logins, their accounts as last listed and the
//! default account, and changing them.
//!
//! A login names its trading service, and whatever is particular to that trading service —
//! where its password goes, which data feed its data comes through — is
//! asked of the trading service's adapter and of that feed's [`SignIn`], never
//! decided here.

use omacharts_engine::providers::{self, Listed, SignIn};
pub use omacharts_engine::providers::Secrets;

use super::*;
use crate::store::Store;

/// The logins, as a JSON list in the order they were added.
pub const LOGINS: &str = "logins";

/// The id of the login market data came through when there was one such
/// login, before each login had a data feed of its own. Read, never
/// written: what a feed stored then by its own id alone stands for. See
/// [`legacy_login_id`].
pub const DATA_LOGIN: &str = "data_login";

/// Every login's accounts as last listed, balances included.
pub const ACCOUNTS: &str = "trading_accounts";

/// The account a chart is about when nobody chose one for it, as
/// `<login id>/<account number>`. One that is no longer listed is ignored.
pub const DEFAULT_ACCOUNT: &str = "default_account";

/// A login as it is typed into its trading service's dialog.
///
/// No `Debug`, so that it cannot be printed by accident: the password is in
/// it.
pub struct Typed {
    /// The trading service whose dialog it was typed into, by its id.
    pub service: String,
    pub name: String,
    pub user: String,
    pub password: String,
    /// The rest of what the dialog asks, by name, as its trading service reads it.
    pub values: Vec<(String, String)>,
}

/// The logins, in the order they were added, less those of a trading service that
/// is closed now — behind a flag that is off — which are kept as they are
/// and are nobody's business until it is on.
///
/// One stored with no name shows the name it would be given now, though
/// what is stored is left as it is.
pub fn logins(store: &Store) -> Vec<Login> {
    let mut logins = kept(store);
    let services = super::registry();
    logins.retain(|login| !services.closed(&login.service));
    for at in 0..logins.len() {
        if logins[at].name.trim().is_empty() {
            let taken = names_but(&logins, &logins[at].id);
            logins[at].name = numbered(&named_after(&logins[at]), &taken);
        }
    }
    logins
}

/// The names of the logins shown, but the one called `except`: the ones a
/// login typed in now must not repeat.
pub fn names_taken(store: &Store, except: Option<&str>) -> Vec<String> {
    names_but(&logins(store), except.unwrap_or_default())
}

fn names_but(logins: &[Login], except: &str) -> Vec<String> {
    logins
        .iter()
        .filter(|login| login.id != except && !login.name.trim().is_empty())
        .map(|login| login.name.trim().to_string())
        .collect()
}

/// `base`, or, when another login is called that, the first of "`base` 2",
/// "`base` 3"… none is.
pub fn numbered(base: &str, taken: &[String]) -> String {
    let free = |name: &str| !taken.iter().any(|other| other.eq_ignore_ascii_case(name));
    if free(base) {
        return base.to_string();
    }
    (2..).map(|n| format!("{base} {n}")).find(|name| free(name)).unwrap_or_default()
}

/// What `login` is called before it is numbered, when nobody named it:
/// what its trading service names it after.
fn named_after(login: &Login) -> String {
    service_for(&login.service).map_or_else(|| login.service.clone(), |service| service.named_after(login))
}

/// Every login kept, open or closed: what a change is made to and written
/// back, so that a closed trading service's logins survive it.
fn kept(store: &Store) -> Vec<Login> {
    let logins = store
        .setting(LOGINS)
        .and_then(|json| serde_json::from_str::<Vec<Login>>(&json).ok())
        .unwrap_or_default();
    // Whatever a trading service or a feed repeats back, a user ID never reaches the
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

/// The data feed of `login`'s own, when its trading service has one: one
/// per login, "Rithmic (Acme Futures)", its market data signed in with that
/// login alone.
pub fn data_feed_of(login: &Login) -> Option<&'static Listed> {
    let kind = providers::entry(&login.service).filter(|feed| feed.per_login())?;
    Some(providers::login_feed(kind, &login.id, &login.name))
}

fn sign_in_of(login: &Login) -> Option<&'static dyn SignIn> {
    data_feed_of(login).and_then(|feed| feed.setup)
}

/// The login a feed stored by its own id alone, `rithmic`, before each
/// login had one, stands for: the data login chosen then, if it is still
/// there, else the first login with a data feed, which was the data login
/// when none was chosen. An order, or a chart, saved then charts through
/// the same login now.
pub fn legacy_login_id(store: &Store, logins: &[Login]) -> Option<String> {
    let chosen = store.setting(DATA_LOGIN);
    let feeds_data = |login: &&Login| data_feed_of(login).is_some();
    chosen
        .filter(|id| login_in(logins, id).is_some_and(|login| feeds_data(&login)))
        .or_else(|| logins.iter().find(feeds_data).map(|login| login.id.clone()))
}

/// Is `login`'s own feed on the order of data feeds: charted from, so
/// that its data session, and anything it was refused, is the charts'?
pub fn feeds_charts(store: &Store, login: &Login) -> bool {
    data_feed_of(login).is_some_and(|feed| crate::feeds::stored_order(store).iter().any(|id| id == feed.id))
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

/// Note a balance the trading service pushed. True when it changed anything.
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
/// its trading service names it after, numbered apart from the `others`.
fn typed_into(login: &mut Login, typed: &Typed, others: &[Login]) {
    login.user = typed.user.trim().to_string();
    login.values = typed.values.iter().map(|(key, value)| (key.clone(), value.trim().to_string())).collect();
    login.name = match typed.name.trim() {
        "" => numbered(&named_after(login), &names_but(others, &login.id)),
        name => name.to_string(),
    };
    login.signed_out = false;
}

fn no_service(login: &Login) -> String {
    format!("there is no trading service called {}", login.service)
}

fn keep_password(secrets: &dyn Secrets, login: &Login, password: &str) -> Result<(), String> {
    if password.is_empty() || login.user.is_empty() {
        return Ok(());
    }
    service_for(&login.service).ok_or_else(|| no_service(login))?.keep_password(secrets, login, password)
}

/// Forget `login`'s password, and end its data session too.
fn forget_password(secrets: &dyn Secrets, login: &Login) -> Result<(), String> {
    match service_for(&login.service) {
        Some(service) => service.forget_password(secrets, login, true),
        None => Ok(()),
    }
}

/// `login`'s data session ends: its feed is not charted from any more.
fn hand_over(login: &Login) {
    if let Some(setup) = sign_in_of(login) {
        setup.hand_over();
    }
}

/// Add a login, its password into `secrets`, and its data feed to the
/// order: one of its own, on top for the first of its kind and after the
/// others for any other, or its trading service's if it is not there.
/// Returns its id.
pub fn add_login(store: &Store, secrets: &dyn Secrets, typed: &Typed) -> Result<String, String> {
    let mut logins = kept(store);
    let mut login = Login {
        id: new_id(&logins),
        name: String::new(),
        service: typed.service.clone(),
        user: String::new(),
        values: Vec::new(),
        signed_out: false,
    };
    typed_into(&mut login, typed, &self::logins(store));
    crate::log_words::conceal_logins(std::slice::from_ref(&login));
    let id = login.id.clone();
    let kept = keep_password(secrets, &login, &typed.password);
    logins.push(login.clone());
    save_logins(store, &logins);
    // Before the order is read: an order naming a feed of its kind alone,
    // stored before there were several, reads as the first login's.
    crate::feeds::remember_logins(store);
    crate::feeds::follow_service(store, &login);
    crate::feeds::remember_logins(store);
    kept.map(|_| id)
}

/// Retype a login: what the dialog had when somebody signed it in again. An
/// empty password keeps the one the keyring holds. A new name renames its
/// feed, whose id stays.
pub fn retype_login(store: &Store, secrets: &dyn Secrets, id: &str, typed: &Typed) -> Result<(), String> {
    let mut logins = kept(store);
    let others = self::logins(store);
    let Some(login) = logins.iter_mut().find(|login| login.id == id) else {
        return Err("that login has been removed".into());
    };
    typed_into(login, typed, &others);
    let kept = keep_password(secrets, login, &typed.password);
    save_logins(store, &logins);
    crate::feeds::remember_logins(store);
    kept
}

/// Sign a login out: its password leaves the keyring, and it stays listed,
/// signed out, with its accounts. Its data session ends too.
pub fn sign_out_login(store: &Store, secrets: &dyn Secrets, id: &str) -> Result<(), String> {
    let mut logins = kept(store);
    let Some(login) = logins.iter_mut().find(|login| login.id == id) else { return Ok(()) };
    // The login is named rather than read back from what the engine was
    // last told.
    let forgotten = forget_password(secrets, login);
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

/// Put the login `id`'s own data feed on top of the order, adding it if it
/// was taken off: what was "the data login" when there could be one, and
/// is now one feed among the others. False for a login with no feed.
pub fn feed_on_top(store: &Store, id: &str) -> bool {
    let logins = logins(store);
    let Some(feed) = login_in(&logins, id).and_then(data_feed_of) else { return false };
    crate::feeds::put_on_top(store, feed);
    true
}

/// What removing a login does to the data feeds, which the dialog asking
/// to remove it says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Removal {
    /// Its trading service has no feed of its own, or its feed is not on
    /// the order: the data feeds are not touched.
    Login,
    /// Its own data feed, called `feed`, goes with it, which charts drawn
    /// from it notice: they chart from the next feed that serves them.
    Feed { feed: String },
}

/// What removing the login `id` would do to the data feeds.
pub fn removal(store: &Store, id: &str) -> Removal {
    let logins = logins(store);
    match login_in(&logins, id).filter(|login| feeds_charts(store, login)).and_then(data_feed_of) {
        Some(feed) => Removal::Feed { feed: feed.label.to_string() },
        None => Removal::Login,
    }
}

/// Remove a login, its password, its accounts and its own data feed, which
/// has nothing to sign in with after it: its charts chart from the next
/// feed that serves them, and with no feed left, from the default.
pub fn remove_login(store: &Store, secrets: &dyn Secrets, id: &str) -> Result<(), String> {
    let mut logins = kept(store);
    let Some(at) = logins.iter().position(|login| login.id == id) else { return Ok(()) };
    let login = logins.remove(at);
    let forgotten = forget_password(secrets, &login);
    let own = data_feed_of(&login);
    save_logins(store, &logins);

    let mut accounts = accounts(store);
    accounts.retain(|account| account.login != id);
    save_accounts(store, &accounts);
    if store.setting(DEFAULT_ACCOUNT).is_some_and(|key| key.split_once('/').is_some_and(|(login, _)| login == id)) {
        store.remove_setting(DEFAULT_ACCOUNT);
    }
    if store.setting(DATA_LOGIN).as_deref() == Some(id) {
        store.remove_setting(DATA_LOGIN);
    }
    match own {
        Some(feed) => crate::feeds::drop_feed(store, feed.id),
        None => hand_over(&login),
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
    /// setting, and brings its own data feed: the first on top of the
    /// order, above a feed already there, and a second after the others.
    #[test]
    fn each_login_brings_its_own_feed_and_the_first_goes_on_top() {
        if skip() {
            return;
        }
        let store = store();
        crate::feeds::add_feed(&store, "tos");
        let kept = Kept::default();
        let acme = add_login(&store, &kept, &typed("Acme Futures", "trader")).unwrap();
        let acme_feed = format!("rithmic@{acme}");
        assert_eq!(crate::feeds::stored_order(&store), vec![acme_feed.as_str(), "tos", "yahoo"]);
        assert_eq!(kept.0.borrow().as_slice(), &[(acme.clone(), "PLANTED-PASSWORD".to_string())]);
        assert!(!store.setting(LOGINS).unwrap().contains("PLANTED"));
        let login = &logins(&store)[0];
        assert_eq!((login.name.as_str(), login.value("system")), ("Acme Futures", Some(TEST_SYSTEM)));
        assert!(!format!("{login:?}").contains("trader"), "a user ID in Debug");
        let feed = data_feed_of(login).expect("a feed of its own");
        assert_eq!((feed.id, feed.label), (acme_feed.as_str(), "Rithmic (Acme Futures)"));

        let other = add_login(&store, &kept, &typed("", "other")).unwrap();
        assert_ne!(other, acme);
        let other_feed = format!("rithmic@{other}");
        assert_eq!(crate::feeds::stored_order(&store), vec![acme_feed.as_str(), "tos", other_feed.as_str(), "yahoo"]);
        assert_eq!(logins(&store)[1].name, "Rithmic Test", "no name is what its system is called");
        assert_eq!(omacharts_engine::providers::listed(&other_feed).map(|feed| feed.label), Some("Rithmic (Rithmic Test)"));
        assert_eq!(store.setting(DATA_LOGIN), None, "no login is the data login");
    }

    /// Each login's feed is handed that login and nobody else's, and a feed
    /// stored by the service's name alone, before there were several, reads
    /// as the login that was the data login then.
    #[test]
    fn each_feed_is_handed_its_own_login() {
        if skip() {
            return;
        }
        let store = store();
        let kept = Kept::default();
        let acme = add_login(&store, &kept, &typed("Acme Futures", "acme-user")).unwrap();
        let demo = add_login(&store, &kept, &typed("Demo Prop", "demo-user")).unwrap();
        assert_eq!(crate::feeds::saved_of(&store, &acme).user.as_deref(), Some("acme-user"));
        assert_eq!(crate::feeds::saved_of(&store, &demo).user.as_deref(), Some("demo-user"));
        assert_eq!(crate::feeds::saved_of(&store, &demo).login.as_deref(), Some(demo.as_str()));
        assert_eq!(crate::feeds::saved_of(&store, "gone"), Default::default());

        store.set_setting(crate::feeds::SETTING, r#"["rithmic","yahoo"]"#);
        assert_eq!(crate::feeds::stored_order(&store), vec![format!("rithmic@{acme}"), "yahoo".into()], "the first");
        store.set_setting(DATA_LOGIN, &demo);
        crate::feeds::remember_logins(&store);
        assert_eq!(crate::feeds::stored_order(&store), vec![format!("rithmic@{demo}"), "yahoo".into()], "the data login");
        assert_eq!(crate::feeds::current_id(&store, "rithmic"), format!("rithmic@{demo}"));
        assert_eq!(crate::feeds::current_id(&store, "tos"), "tos");

        assert!(feed_on_top(&store, &acme));
        assert_eq!(crate::feeds::stored_order(&store), vec![format!("rithmic@{acme}"), format!("rithmic@{demo}"), "yahoo".into()]);
        assert!(!feed_on_top(&store, "gone"));
    }

    /// Removing a login takes its feed off the order and leaves the others'
    /// alone, with its accounts, its default account and its password.
    #[test]
    fn removing_a_login_takes_its_feed_with_it() {
        if skip() {
            return;
        }
        let store = store();
        let kept = Kept::default();
        let acme = add_login(&store, &kept, &typed("Acme Futures", "a")).unwrap();
        let demo = add_login(&store, &kept, &typed("Demo Prop", "b")).unwrap();
        cache_accounts(&store, &acme, vec![account(&acme, "ACME-0001")]);
        cache_accounts(&store, &demo, vec![account(&demo, "DEMO-1234"), account(&demo, "DEMO-5678")]);
        set_default_account(&store, &format!("{acme}/ACME-0001"));
        store.set_setting(DATA_LOGIN, &acme);

        assert_eq!(removal(&store, &acme), Removal::Feed { feed: "Rithmic (Acme Futures)".into() });
        remove_login(&store, &kept, &acme).unwrap();
        assert_eq!(accounts(&store).iter().map(|a| a.id.as_str()).collect::<Vec<_>>(), vec!["DEMO-1234", "DEMO-5678"]);
        assert_eq!(store.setting(DEFAULT_ACCOUNT), None, "the default went with its login");
        assert_eq!(store.setting(DATA_LOGIN), None);
        assert_eq!(kept.0.borrow().iter().map(|(l, _)| l.clone()).collect::<Vec<_>>(), vec![demo.clone()]);
        assert_eq!(crate::feeds::stored_order(&store), vec![format!("rithmic@{demo}"), "yahoo".into()]);
        assert_eq!(crate::feeds::saved_of(&store, &acme).user, None, "nobody to sign in with");

        // A login whose feed was taken off the order has nothing to drop.
        crate::feeds::save_order(&store, &["yahoo".into()]);
        assert_eq!(removal(&store, &demo), Removal::Login);
    }

    /// The last login's feed goes with it even when it is the last feed:
    /// with nothing left to chart through, the order is the default's.
    #[test]
    fn the_last_feed_goes_with_its_login() {
        if skip() {
            return;
        }
        let store = store();
        let kept = Kept::default();
        let acme = add_login(&store, &kept, &typed("Acme Futures", "a")).unwrap();
        assert!(crate::feeds::remove_feed(&store, "yahoo"));
        assert_eq!(crate::feeds::stored_order(&store), vec![format!("rithmic@{acme}")]);
        remove_login(&store, &kept, &acme).unwrap();
        assert!(logins(&store).is_empty());
        assert!(kept.0.borrow().is_empty());
        assert_eq!(store.setting(ACCOUNTS), None);
        assert_eq!(crate::feeds::stored_order(&store), vec!["yahoo"]);
    }

    /// Signing a login out forgets its password and keeps it listed, with
    /// its accounts; its feed's charts are told there is no login.
    #[test]
    fn signing_out_keeps_the_login_and_forgets_the_password() {
        if skip() {
            return;
        }
        let store = store();
        let kept = Kept::default();
        let acme = add_login(&store, &kept, &typed("Acme Futures", "a")).unwrap();
        cache_accounts(&store, &acme, vec![account(&acme, "ACME-0001")]);
        sign_out_login(&store, &kept, &acme).unwrap();
        assert!(kept.0.borrow().is_empty());
        assert!(logins(&store)[0].signed_out);
        assert_eq!(accounts(&store).len(), 1);
        assert_eq!(crate::feeds::saved_of(&store, &acme).user, None, "its feed has no login to sign in with");
        let desk = Desk::new(Registry::of(vec![Arc::new(FakeService::default())]));
        assert_eq!(desk.standing(&logins(&store)[0], true), Standing::SignedOut);

        retype_login(&store, &kept, &acme, &typed("Acme Futures", "a")).unwrap();
        assert!(!logins(&store)[0].signed_out);
        assert_eq!(kept.0.borrow().len(), 1);
    }

    /// A login typed with no name is called what its system is, numbered
    /// past the names the others have; signed in again with none, it keeps
    /// its own rather than numbering itself. One stored with no name shows
    /// it too, and is left stored as it was.
    #[test]
    fn a_login_with_no_name_is_numbered_past_the_others() {
        if skip() {
            return;
        }
        assert_eq!(numbered("Sierra Chart", &[]), "Sierra Chart");
        assert_eq!(numbered("Sierra Chart", &["sierra chart".into(), "Sierra Chart 2".into()]), "Sierra Chart 3");
        assert_eq!(numbered("Sierra Chart", &["Sierra Chart 2".into()]), "Sierra Chart");

        let store = store();
        let secrets = Kept::default();
        let first = add_login(&store, &secrets, &typed("", "a")).unwrap();
        add_login(&store, &secrets, &typed(" ", "b")).unwrap();
        retype_login(&store, &secrets, &first, &typed("", "a")).unwrap();
        let names: Vec<String> = logins(&store).into_iter().map(|login| login.name).collect();
        assert_eq!(names, ["Rithmic Test", "Rithmic Test 2"]);

        let mut stored = kept(&store);
        stored[1].name = String::new();
        save_logins(&store, &stored);
        assert_eq!(logins(&store)[1].name, "Rithmic Test 2", "shown with the name it would be given");
        assert_eq!(kept(&store)[1].name, "", "and stored as it was");
    }
}
