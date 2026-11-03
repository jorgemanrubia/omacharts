//! Trading: the broker logins on this machine, the accounts inside them, and
//! which account a chart is about.
//!
//! Three things, kept apart so the broker an order goes to and the feed a
//! chart is drawn from can be combined freely:
//!
//! - **A login** is a broker connection: a name the trader chooses, the
//!   Rithmic user ID, system and gateway, and the password in the keyring
//!   under the login's id. Any number of them, in [`LOGINS`], `Local`
//!   because a login belongs to the machine it was typed on.
//! - **An account** is one of the account numbers inside a login, listed by
//!   the order plant once it signs in. They are cached in [`ACCOUNTS`] as
//!   bookkeeping, never in a backup, so the settings page and the header
//!   draw without connecting and account numbers stay on this machine.
//! - **A data feed** is a source on the Data page. Exactly one login is the
//!   data login, [`DATA_LOGIN`], the first added unless somebody picks
//!   another: Rithmic's data source signs in through it, and the others sign
//!   in for orders and P&L only. Several logins streaming quotes could count
//!   as several data sessions under CME's two-session rule.
//!
//! Each login has at most one trading session, because a second sign-in on
//! the same login forces the first one out: [`Desk`] holds them by login and
//! refuses to open a second.
//!
//! Which account a chart is about is decided in one place, [`account_of`],
//! for the header now and for the order ticket later, so the two can never
//! disagree.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use omacharts_engine::providers::rithmic::session::{self, Secrets};
use serde::{Deserialize, Serialize};

use crate::store::Store;

/// The logins, as a JSON list in the order they were added.
pub const LOGINS: &str = "rithmic_logins";

/// The id of the login market data comes through. Missing, or naming a
/// login that is gone, is the first login.
pub const DATA_LOGIN: &str = "rithmic_data_login";

/// Every login's accounts as last listed, balances included.
pub const ACCOUNTS: &str = "trading_accounts";

/// The account a chart is about when nobody chose one for it, as
/// `<login id>/<account number>`. One that is no longer listed is ignored.
pub const DEFAULT_ACCOUNT: &str = "default_account";

/// The system and gateway of a login with none named.
pub const TEST_SYSTEM: &str = "Rithmic Test";
pub const TEST_GATEWAY: &str = "wss://rituz00100.rithmic.com:443";

/// One broker login, without its password.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct Login {
    /// The app's own name for it, which never changes. Its password is kept
    /// under this, and its accounts are known by it.
    pub id: String,
    /// What the trader calls it: "Lucid", "Tradeify", "Rithmic Test".
    pub name: String,
    pub user: String,
    pub system: String,
    pub gateway: String,
    /// Signed out: the password was forgotten, and nothing signs in with it
    /// until somebody types it again.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub signed_out: bool,
}

impl std::fmt::Debug for Login {
    /// Everything but the user ID, which is never printed.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Login")
            .field("id", &self.id)
            .field("name", &self.name)
            .field("system", &self.system)
            .field("signed_out", &self.signed_out)
            .finish_non_exhaustive()
    }
}

impl Login {
    /// The login as the Rithmic crate signs in with it: the password from
    /// the keyring, under this login's id.
    pub fn rithmic(&self) -> rithmic::Login {
        rithmic::Login::Keyring {
            login: self.id.clone(),
            user: self.user.clone(),
            system: self.system.clone(),
            url: self.gateway.clone(),
        }
    }
}

/// One account inside a login, as last listed.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Account {
    /// The login it is in, by id.
    pub login: String,
    /// The account number.
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub currency: Option<String>,
    #[serde(default)]
    pub fcm: String,
    #[serde(default)]
    pub ib: String,
    /// What the PnL plant last said it holds; `None` until it has.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub balance: Option<f64>,
}

impl Account {
    /// How a chart and the default name it.
    pub fn key(&self) -> String {
        format!("{}/{}", self.login, self.id)
    }

    /// Its balance in its currency, in whole units: "$50,210".
    pub fn balance_text(&self) -> Option<String> {
        let balance = self.balance?;
        let whole = grouped(balance.abs().round() as u64);
        let sign = if balance < 0.0 { "-" } else { "" };
        Some(match self.currency.as_deref() {
            None | Some("USD") => format!("{sign}${whole}"),
            Some(currency) => format!("{sign}{whole} {currency}"),
        })
    }
}

fn grouped(value: u64) -> String {
    let digits = value.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (at, digit) in digits.chars().enumerate() {
        if at > 0 && (digits.len() - at).is_multiple_of(3) {
            out.push(',');
        }
        out.push(digit);
    }
    out
}

/// A login as it is typed into the dialog.
///
/// No `Debug`, so that it cannot be printed by accident: the password is in
/// it.
pub struct Typed {
    pub name: String,
    pub user: String,
    pub password: String,
    pub system: String,
    pub gateway: String,
}

// -- what is stored ---------------------------------------------------------

/// The logins, in the order they were added.
pub fn logins(store: &Store) -> Vec<Login> {
    store
        .setting(LOGINS)
        .and_then(|json| serde_json::from_str::<Vec<Login>>(&json).ok())
        .unwrap_or_default()
}

fn save_logins(store: &Store, logins: &[Login]) {
    match logins.is_empty() {
        true => store.remove_setting(LOGINS),
        false => store.set_setting(LOGINS, &serde_json::to_string(logins).unwrap_or_default()),
    }
}

fn login_in<'a>(logins: &'a [Login], id: &str) -> Option<&'a Login> {
    logins.iter().find(|login| login.id == id)
}

/// The id of the data login: the one chosen if it is still there, else the
/// first.
pub fn data_login_id(store: &Store, logins: &[Login]) -> Option<String> {
    let chosen = store.setting(DATA_LOGIN);
    chosen
        .filter(|id| login_in(logins, id).is_some())
        .or_else(|| logins.first().map(|login| login.id.clone()))
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

/// Note a balance the PnL plant pushed. True when it changed anything.
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

/// Which account a chart is about: the one chosen for it while it is still
/// listed, else the default. The header asks this, and the order ticket
/// will, so they can never disagree.
pub fn account_of<'a>(accounts: &'a [Account], default: Option<&str>, chosen: Option<&str>) -> Option<&'a Account> {
    chosen
        .and_then(|key| accounts.iter().find(|account| account.key() == key))
        .or_else(|| default_account(accounts, default))
}

/// Is a chart's chosen account one that is no longer listed, say because
/// its login was removed? Then it is forgotten and the chart follows the
/// default.
pub fn stale(accounts: &[Account], chosen: Option<&str>) -> bool {
    chosen.is_some_and(|key| !accounts.iter().any(|account| account.key() == key))
}

// -- changing what is stored ------------------------------------------------

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

fn typed_into(login: &mut Login, typed: &Typed) {
    let or = |text: &str, fallback: &str| match text.trim() {
        "" => fallback.to_string(),
        text => text.to_string(),
    };
    login.user = typed.user.trim().to_string();
    login.system = or(&typed.system, TEST_SYSTEM);
    login.gateway = or(&typed.gateway, TEST_GATEWAY);
    login.name = or(&typed.name, &login.system);
    login.signed_out = false;
}

fn keep_password(secrets: &dyn Secrets, login: &Login, password: &str) -> Result<(), String> {
    if password.is_empty() || login.user.is_empty() {
        return Ok(());
    }
    session::save_password(secrets, &login.id, &login.user, &login.system, password)
}

/// Add a login, its password into `secrets`. The first one is the data
/// login, and puts Rithmic on top of the data sources. Returns its id.
pub fn add_login(store: &Store, secrets: &dyn Secrets, typed: &Typed) -> Result<String, String> {
    let mut logins = logins(store);
    let mut login = Login {
        id: new_id(&logins),
        name: String::new(),
        user: String::new(),
        system: String::new(),
        gateway: String::new(),
        signed_out: false,
    };
    typed_into(&mut login, typed);
    let first = logins.is_empty();
    let id = login.id.clone();
    let kept = keep_password(secrets, &login, &typed.password);
    logins.push(login);
    save_logins(store, &logins);
    if first {
        store.set_setting(DATA_LOGIN, &id);
        if let Some(rithmic) = omacharts_engine::providers::listed(RITHMIC) {
            crate::feeds::put_on_top(store, rithmic);
        }
    }
    crate::feeds::remember_logins(store);
    kept.map(|_| id)
}

const RITHMIC: &str = "rithmic";

/// Retype a login: what the dialog had when somebody signed it in again. An
/// empty password keeps the one the keyring holds.
pub fn retype_login(store: &Store, secrets: &dyn Secrets, id: &str, typed: &Typed) -> Result<(), String> {
    let mut logins = logins(store);
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
    let mut logins = logins(store);
    let data = data_login_id(store, &logins);
    let Some(login) = logins.iter_mut().find(|login| login.id == id) else { return Ok(()) };
    let forgotten = match data.as_deref() == Some(id) {
        true => {
            crate::feeds::remember_logins(store);
            session::sign_out_of(secrets)
        }
        false => session::forget_password(secrets, &login.id, &login.user, &login.system),
    };
    login.signed_out = true;
    save_logins(store, &logins);
    crate::feeds::remember_logins(store);
    forgotten
}

/// Mark a login signed out, its password already forgotten by whoever
/// signed it out.
pub fn mark_signed_out(store: &Store, id: &str) {
    let mut logins = logins(store);
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
    if login_in(&logins, id).is_none() || data_login_id(store, &logins).as_deref() == Some(id) {
        return;
    }
    store.set_setting(DATA_LOGIN, id);
    session::hand_over();
    crate::feeds::remember_logins(store);
}

/// Remove a login, its password and its accounts. If market data came
/// through it, it comes through the next login now, or with none left
/// Rithmic leaves the data sources.
pub fn remove_login(store: &Store, secrets: &dyn Secrets, id: &str) -> Result<(), String> {
    let mut logins = logins(store);
    let Some(at) = logins.iter().position(|login| login.id == id) else { return Ok(()) };
    let was_data = data_login_id(store, &logins).as_deref() == Some(id);
    let forgotten = match was_data {
        true => {
            crate::feeds::remember_logins(store);
            session::sign_out_of(secrets)
        }
        false => {
            let login = &logins[at];
            session::forget_password(secrets, &login.id, &login.user, &login.system)
        }
    };
    logins.remove(at);
    save_logins(store, &logins);

    let mut accounts = accounts(store);
    accounts.retain(|account| account.login != id);
    save_accounts(store, &accounts);
    if store.setting(DEFAULT_ACCOUNT).is_some_and(|key| key.split_once('/').is_some_and(|(login, _)| login == id)) {
        store.remove_setting(DEFAULT_ACCOUNT);
    }

    if was_data {
        match logins.first() {
            Some(next) => {
                store.set_setting(DATA_LOGIN, &next.id);
                session::hand_over();
            }
            None => {
                store.remove_setting(DATA_LOGIN);
                let order: Vec<String> = crate::feeds::stored_order(store)
                    .into_iter()
                    .filter(|known| !known.eq_ignore_ascii_case(RITHMIC))
                    .collect();
                crate::feeds::save_order(store, &order);
            }
        }
    }
    crate::feeds::remember_logins(store);
    forgotten
}

// -- sessions ---------------------------------------------------------------

/// What signs a login in for trading: Rithmic, or a fake under test.
pub trait Broker: Send + Sync {
    /// Sign in, blocking until the broker has answered. `changed` is called,
    /// on any thread, whenever a balance moves or the session ends by
    /// itself.
    fn sign_in(&self, login: &Login, changed: Box<dyn Fn() + Send + Sync>) -> Result<Arc<dyn Session>, String>;
}

/// A login's signed-in trading session.
pub trait Session: Send + Sync {
    /// The accounts, as the broker listed them at sign-in.
    fn accounts(&self) -> Vec<Account>;
    fn balance(&self, account: &str) -> Option<f64>;
    /// Why it is no longer signed in, if it ended by itself.
    fn trouble(&self) -> Option<String>;
    fn sign_out(&self);
}

/// Rithmic's order and PnL plants, through `rithmic::Trading`.
pub struct Rithmic;

impl Broker for Rithmic {
    fn sign_in(&self, login: &Login, changed: Box<dyn Fn() + Send + Sync>) -> Result<Arc<dyn Session>, String> {
        let app = session::app().map_err(|error| error.to_string())?;
        let trading = rithmic::Trading::sign_in(app, &login.rithmic(), changed).map_err(|error| match error {
            rithmic::Error::Refused(why) => why,
            error => error.to_string(),
        })?;
        Ok(Arc::new(RithmicSession { login: login.id.clone(), trading }))
    }
}

struct RithmicSession {
    login: String,
    trading: rithmic::Trading,
}

impl Session for RithmicSession {
    fn accounts(&self) -> Vec<Account> {
        self.trading
            .accounts()
            .into_iter()
            .map(|account| Account {
                login: self.login.clone(),
                balance: self.trading.balance(&account.id),
                id: account.id,
                name: account.name,
                currency: account.currency,
                fcm: account.fcm,
                ib: account.ib,
            })
            .collect()
    }

    fn balance(&self, account: &str) -> Option<f64> {
        self.trading.balance(account)
    }

    fn trouble(&self) -> Option<String> {
        self.trading.trouble().map(|error| match error {
            rithmic::Error::Refused(why) => why,
            error => error.to_string(),
        })
    }

    fn sign_out(&self) {
        self.trading.sign_out();
    }
}

/// How a login stands, as its row and the header say it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Standing {
    /// Its trading session is signed in, in this run.
    Live,
    /// Its password is kept, and nothing has signed in with it this run.
    Saved,
    /// Signed out, or never signed in: there is no password to sign in with.
    SignedOut,
    /// Rithmic turned it down, or ended its session, in these words.
    Refused(String),
}

impl Standing {
    /// The words for its row: "live", "not signed in", Rithmic's own.
    pub fn words(&self) -> String {
        match self {
            Standing::Live => "live".into(),
            Standing::Saved => "signed in".into(),
            Standing::SignedOut => "not signed in".into(),
            Standing::Refused(why) => why.clone(),
        }
    }

    /// What the header says about an account in a login that stands so,
    /// when it is in trouble.
    pub fn trouble(&self) -> Option<String> {
        match self {
            Standing::Live | Standing::Saved => None,
            Standing::SignedOut => Some("signed out".into()),
            Standing::Refused(why) => Some(why.clone()),
        }
    }
}

/// Every login's trading session, at most one each.
///
/// Opening one is two halves, because signing in blocks and runs on a
/// thread of its own: [`Desk::begin`] says whether one may be opened for a
/// login and marks it as opening, and [`Desk::opened`] takes what the
/// thread came back with. Neither half lets a second session start for a
/// login that has one or is getting one.
pub struct Desk {
    broker: Arc<dyn Broker>,
    sessions: RefCell<HashMap<String, Arc<dyn Session>>>,
    opening: RefCell<HashSet<String>>,
    refused: RefCell<HashMap<String, String>>,
    /// Woken from a session's thread when a balance moves or a session ends;
    /// whoever holds the receiver takes it from there on the main loop.
    wake: async_channel::Sender<()>,
    woken: async_channel::Receiver<()>,
    /// Told on the main loop when anything here changed: the window's
    /// headers, and the Trading page while it is open.
    listeners: RefCell<Vec<std::rc::Weak<dyn Fn()>>>,
}

impl Desk {
    pub fn new(broker: Arc<dyn Broker>) -> Desk {
        let (wake, woken) = async_channel::unbounded();
        Desk {
            broker,
            sessions: RefCell::new(HashMap::new()),
            opening: RefCell::new(HashSet::new()),
            refused: RefCell::new(HashMap::new()),
            wake,
            woken,
            listeners: RefCell::new(Vec::new()),
        }
    }

    /// What a session calls from its own thread when something moved.
    pub fn waker(&self) -> Box<dyn Fn() + Send + Sync> {
        let wake = self.wake.clone();
        Box::new(move || {
            let _ = wake.try_send(());
        })
    }

    /// What the main loop waits on for [`Desk::waker`]'s calls.
    pub fn woken(&self) -> async_channel::Receiver<()> {
        self.woken.clone()
    }

    /// Call `listener` whenever [`Desk::tell`] is, while it is alive.
    pub fn listen(&self, listener: &std::rc::Rc<dyn Fn()>) {
        self.listeners.borrow_mut().push(std::rc::Rc::downgrade(listener));
    }

    /// Tell everybody listening that logins, sessions or accounts changed.
    pub fn tell(&self) {
        let alive: Vec<std::rc::Rc<dyn Fn()>> = {
            let mut listeners = self.listeners.borrow_mut();
            listeners.retain(|listener| listener.strong_count() > 0);
            listeners.iter().filter_map(std::rc::Weak::upgrade).collect()
        };
        for listener in alive {
            listener();
        }
    }

    pub fn broker(&self) -> Arc<dyn Broker> {
        self.broker.clone()
    }

    /// May a session be opened for `login` now? Not while it has one, or
    /// one is being opened. True marks it as opening.
    pub fn begin(&self, login: &str) -> bool {
        if self.live(login) || self.opening.borrow().contains(login) {
            return false;
        }
        // One that ended by itself is let go before another opens.
        self.sessions.borrow_mut().remove(login);
        self.opening.borrow_mut().insert(login.to_string());
        self.refused.borrow_mut().remove(login);
        true
    }

    /// Take what a sign-in for `login` came back with. A session for a
    /// login that is no longer opening — removed meanwhile — is signed out
    /// at once rather than kept.
    pub fn opened(&self, login: &str, outcome: Result<Arc<dyn Session>, String>) {
        let wanted = self.opening.borrow_mut().remove(login);
        match outcome {
            Ok(session) if wanted => {
                self.sessions.borrow_mut().insert(login.to_string(), session);
            }
            Ok(session) => session.sign_out(),
            Err(why) if wanted => {
                self.refused.borrow_mut().insert(login.to_string(), why);
            }
            Err(_) => {}
        }
    }

    /// End `login`'s session, if it has one, and forget any refusal.
    pub fn end(&self, login: &str) {
        self.opening.borrow_mut().remove(login);
        self.refused.borrow_mut().remove(login);
        if let Some(session) = self.sessions.borrow_mut().remove(login) {
            session.sign_out();
        }
    }

    pub fn opening(&self, login: &str) -> bool {
        self.opening.borrow().contains(login)
    }

    pub fn live(&self, login: &str) -> bool {
        self.sessions.borrow().get(login).is_some_and(|session| session.trouble().is_none())
    }

    pub fn session(&self, login: &str) -> Option<Arc<dyn Session>> {
        self.sessions.borrow().get(login).cloned()
    }

    /// How `login` stands. The data login's own refusal, which the data
    /// session keeps, counts too.
    pub fn standing(&self, login: &Login, data: bool) -> Standing {
        if let Some(why) = self.refused.borrow().get(&login.id) {
            return Standing::Refused(why.clone());
        }
        if let Some(session) = self.sessions.borrow().get(&login.id) {
            return match session.trouble() {
                None => Standing::Live,
                Some(why) => Standing::Refused(why),
            };
        }
        if login.signed_out || login.user.is_empty() {
            return Standing::SignedOut;
        }
        if data && let Some(why) = session::refusal() {
            return Standing::Refused(why);
        }
        Standing::Saved
    }

    /// Write what every live session knows now — its accounts and their
    /// balances — into the cache. True when anything changed.
    pub fn note(&self, store: &Store) -> bool {
        let sessions: Vec<(String, Arc<dyn Session>)> =
            self.sessions.borrow().iter().map(|(id, session)| (id.clone(), session.clone())).collect();
        let mut changed = false;
        for (login, session) in sessions {
            let listed = session.accounts();
            let known: Vec<Account> = accounts(store).into_iter().filter(|a| a.login == login).collect();
            if listed != known {
                cache_accounts(store, &login, listed);
                changed = true;
            }
        }
        changed
    }
}

/// What the window keeps of Trading to draw headers from without asking
/// the database each time: read when it starts and whenever Trading
/// changes.
#[derive(Clone, Debug, Default)]
pub struct View {
    pub logins: Vec<Login>,
    pub accounts: Vec<Account>,
    /// The default account, as stored; [`account_of`] ignores one that is
    /// stale.
    pub default: Option<String>,
    pub data_login: Option<String>,
}

impl View {
    pub fn load(store: &Store) -> View {
        let logins = logins(store);
        if logins.is_empty() {
            return View::default();
        }
        View {
            accounts: accounts(store),
            default: store.setting(DEFAULT_ACCOUNT),
            data_login: data_login_id(store, &logins),
            logins,
        }
    }

    pub fn login(&self, id: &str) -> Option<&Login> {
        login_in(&self.logins, id)
    }

    /// The account a chart that chose `chosen` is about.
    pub fn account_of(&self, chosen: Option<&str>) -> Option<&Account> {
        account_of(&self.accounts, self.default.as_deref(), chosen)
    }

    /// The default account, or the first when none is.
    pub fn default_account(&self) -> Option<&Account> {
        default_account(&self.accounts, self.default.as_deref())
    }

    /// What the header says is wrong with `account`'s login, if anything.
    pub fn trouble(&self, desk: &Desk, account: &Account) -> Option<String> {
        let login = self.login(&account.login)?;
        desk.standing(login, self.data_login.as_deref() == Some(login.id.as_str())).trouble()
    }
}

// -- the header's account part -----------------------------------------------

/// What the account part offers a chart: an ACCOUNT list when there is more
/// than one account, a DATA list when there is more than one feed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Offer {
    pub accounts: bool,
    pub feeds: bool,
}

impl Offer {
    pub fn of(accounts: usize, feeds: usize) -> Offer {
        Offer { accounts: accounts > 1, feeds: feeds > 1 }
    }

    /// Is the part there at all? Otherwise it is blank, takes no room and
    /// cannot be clicked.
    pub fn any(self) -> bool {
        self.accounts || self.feeds
    }

    /// Captions over the lists only when both show.
    pub fn captions(self) -> bool {
        self.accounts && self.feeds
    }

    pub fn tooltip(self) -> &'static str {
        match (self.accounts, self.feeds) {
            (true, true) => "Account and data: click to choose this chart's account and where its data comes from",
            (true, false) => "Account: click to choose this chart's account",
            _ => "Account: click to choose where this chart's data comes from",
        }
    }
}

/// How an account is named where several logins could hold it: "Lucid
/// 1234" once there are several logins, "1234" with one.
pub fn account_name(account: &Account, logins: &[Login]) -> String {
    match logins.len() > 1 {
        true => match login_in(logins, &account.login) {
            Some(login) => format!("{} {}", login.name, account.id),
            None => account.id.clone(),
        },
        false => account.id.clone(),
    }
}

/// What the account part says: only what can be chosen, plus any trouble.
///
/// The account when there are several to choose from, or when its login is
/// in trouble ("951 signed out"); the feed when there are several, in the
/// words `feed` makes ("Rithmic", "Yahoo · Rithmic not signed in"). Nothing
/// when nothing can be chosen.
pub fn part_text(
    offer: Offer,
    account: Option<(&Account, &[Login])>,
    trouble: Option<&str>,
    feed: impl FnOnce() -> String,
) -> Option<String> {
    if !offer.any() {
        return None;
    }
    let mut words: Vec<String> = Vec::with_capacity(2);
    if let Some((account, logins)) = account
        && (offer.accounts || trouble.is_some())
    {
        let name = account_name(account, logins);
        words.push(match trouble {
            Some(trouble) => format!("{name} {trouble}"),
            None => name,
        });
    }
    if offer.feeds {
        words.push(feed());
    }
    (!words.is_empty()).then(|| words.join(" · "))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;

    fn store() -> Store {
        Store::memory().expect("an empty database")
    }

    fn typed(name: &str, user: &str) -> Typed {
        Typed {
            name: name.into(),
            user: user.into(),
            password: "PLANTED-PASSWORD".into(),
            system: String::new(),
            gateway: "wss://example.invalid:443".into(),
        }
    }

    fn account(login: &str, id: &str) -> Account {
        Account {
            login: login.into(),
            id: id.into(),
            name: None,
            currency: Some("USD".into()),
            fcm: "FCM".into(),
            ib: "IB".into(),
            balance: None,
        }
    }

    /// A password store that is a list in memory, so that nothing a test
    /// does reaches a keyring.
    #[derive(Default)]
    pub(crate) struct Kept(pub std::cell::RefCell<Vec<(String, String)>>);

    impl Secrets for Kept {
        fn save(&self, login: &str, _user: &str, _system: &str, password: &str) -> Result<(), String> {
            let mut kept = self.0.borrow_mut();
            kept.retain(|(l, _)| l != login);
            kept.push((login.into(), password.into()));
            Ok(())
        }
        fn forget(&self, login: &str, _user: &str, _system: &str) -> Result<(), String> {
            self.0.borrow_mut().retain(|(l, _)| l != login);
            Ok(())
        }
    }

    /// A broker that signs anybody in with the accounts it was given, and
    /// counts how often it was asked.
    #[derive(Default)]
    pub(crate) struct Fake {
        pub accounts: Mutex<HashMap<String, Vec<Account>>>,
        pub signed_in: AtomicUsize,
        pub signed_out: Arc<AtomicUsize>,
    }

    struct FakeSession {
        accounts: Vec<Account>,
        signed_out: Arc<AtomicUsize>,
        gone: std::sync::atomic::AtomicBool,
    }

    impl Session for FakeSession {
        fn accounts(&self) -> Vec<Account> {
            self.accounts.clone()
        }
        fn balance(&self, account: &str) -> Option<f64> {
            self.accounts.iter().find(|a| a.id == account).and_then(|a| a.balance)
        }
        fn trouble(&self) -> Option<String> {
            self.gone.load(Ordering::Acquire).then(|| "signed out".into())
        }
        fn sign_out(&self) {
            self.gone.store(true, Ordering::Release);
            self.signed_out.fetch_add(1, Ordering::AcqRel);
        }
    }

    impl Broker for Fake {
        fn sign_in(&self, login: &Login, _: Box<dyn Fn() + Send + Sync>) -> Result<Arc<dyn Session>, String> {
            self.signed_in.fetch_add(1, Ordering::AcqRel);
            let accounts = self.accounts.lock().unwrap().get(&login.name).cloned().unwrap_or_default();
            Ok(Arc::new(FakeSession {
                accounts: accounts.into_iter().map(|a| Account { login: login.id.clone(), ..a }).collect(),
                signed_out: self.signed_out.clone(),
                gone: Default::default(),
            }))
        }
    }

    fn skip() -> bool {
        // A development login is in charge of the keyring, and the store's
        // passwords are never written.
        session::development()
    }

    /// Adding a login keeps its password under its own id and nowhere in a
    /// setting. The first is the data login and puts Rithmic on top of the
    /// data sources, above one already there.
    #[test]
    fn the_first_login_is_the_data_login_and_puts_rithmic_on_top() {
        if skip() {
            return;
        }
        let store = store();
        crate::feeds::add_source(&store, "tos");
        let kept = Kept::default();
        let lucid = add_login(&store, &kept, &typed("Lucid", "trader")).unwrap();
        assert_eq!(crate::feeds::stored_order(&store), vec!["rithmic", "tos", "yahoo"]);
        assert_eq!(data_login(&store).map(|l| l.id), Some(lucid.clone()));
        assert_eq!(kept.0.borrow().as_slice(), &[(lucid.clone(), "PLANTED-PASSWORD".to_string())]);
        assert!(!store.setting(LOGINS).unwrap().contains("PLANTED"));
        let login = &logins(&store)[0];
        assert_eq!((login.name.as_str(), login.system.as_str()), ("Lucid", TEST_SYSTEM));
        assert!(!format!("{login:?}").contains("trader"), "a user ID in Debug");

        // A second login is not the data login, and leaves the order alone.
        crate::feeds::save_order(&store, &["tos".into(), "rithmic".into()]);
        let tradeify = add_login(&store, &kept, &typed("", "other")).unwrap();
        assert_ne!(tradeify, lucid);
        assert_eq!(data_login(&store).map(|l| l.id), Some(lucid));
        assert_eq!(crate::feeds::stored_order(&store), vec!["tos", "rithmic"]);
        assert_eq!(logins(&store)[1].name, TEST_SYSTEM, "no name is the system's");
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

    /// Removing the data login hands data to the next one; removing the
    /// last takes Rithmic off the data sources. Each takes its accounts and
    /// its password with it.
    #[test]
    fn removing_the_data_login_hands_data_over_and_the_last_takes_rithmic_away() {
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

        remove_login(&store, &kept, &lucid).unwrap();
        assert_eq!(data_login(&store).map(|l| l.id), Some(test.clone()));
        assert_eq!(accounts(&store).iter().map(|a| a.id.as_str()).collect::<Vec<_>>(), vec!["951"]);
        assert_eq!(store.setting(DEFAULT_ACCOUNT), None, "the default went with its login");
        assert_eq!(kept.0.borrow().iter().map(|(l, _)| l.clone()).collect::<Vec<_>>(), vec![test.clone()]);
        assert_eq!(crate::feeds::stored_order(&store), vec!["rithmic", "yahoo"]);

        remove_login(&store, &kept, &test).unwrap();
        assert!(logins(&store).is_empty());
        assert!(kept.0.borrow().is_empty());
        assert_eq!(store.setting(crate::feeds::SETTING), None, "Rithmic leaves the sources");
        assert_eq!(store.setting(ACCOUNTS), None);
        assert_eq!(store.setting(DATA_LOGIN), None);
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
        let desk = Desk::new(Arc::new(Fake::default()));
        assert_eq!(desk.standing(&logins(&store)[0], true), Standing::SignedOut);

        retype_login(&store, &kept, &lucid, &typed("Lucid", "a")).unwrap();
        assert!(!logins(&store)[0].signed_out);
        assert_eq!(kept.0.borrow().len(), 1);
    }

    /// One session per login: a second sign-in while one is open or being
    /// opened is refused before it reaches the broker, and one is opened
    /// again only after the first has ended.
    #[test]
    fn a_login_never_has_two_sessions() {
        let fake = Arc::new(Fake::default());
        let desk = Desk::new(fake.clone());
        let login = Login {
            id: "1".into(),
            name: "Lucid".into(),
            user: "a".into(),
            system: TEST_SYSTEM.into(),
            gateway: TEST_GATEWAY.into(),
            signed_out: false,
        };
        let open = |desk: &Desk| {
            if desk.begin(&login.id) {
                let session = desk.broker().sign_in(&login, Box::new(|| {}));
                desk.opened(&login.id, session);
            }
        };
        assert!(desk.begin(&login.id));
        assert!(!desk.begin(&login.id), "a second while the first is opening");
        let session = desk.broker().sign_in(&login, Box::new(|| {}));
        desk.opened(&login.id, session);
        assert!(desk.live(&login.id));
        open(&desk);
        open(&desk);
        assert_eq!(fake.signed_in.load(Ordering::Acquire), 1);
        assert_eq!(desk.standing(&login, false), Standing::Live);

        desk.end(&login.id);
        assert_eq!(fake.signed_out.load(Ordering::Acquire), 1);
        assert_eq!(desk.standing(&login, false), Standing::Saved);
        open(&desk);
        assert_eq!(fake.signed_in.load(Ordering::Acquire), 2);

        // A session that comes back for a login removed while it signed in
        // is signed straight out, not kept.
        desk.end(&login.id);
        assert!(desk.begin(&login.id));
        desk.end(&login.id);
        let late = desk.broker().sign_in(&login, Box::new(|| {}));
        desk.opened(&login.id, late);
        assert!(!desk.live(&login.id));
        assert_eq!(fake.signed_out.load(Ordering::Acquire), 3);
    }

    /// A live session's accounts land in the cache, under their login, and
    /// a balance not known yet keeps the one known before.
    #[test]
    fn a_sessions_accounts_are_cached_with_their_balances() {
        let store = store();
        let kept = Kept::default();
        if skip() {
            return;
        }
        let lucid = add_login(&store, &kept, &typed("Lucid", "a")).unwrap();
        let fake = Arc::new(Fake::default());
        let mut funded = account("", "1234");
        funded.balance = Some(50_210.4);
        fake.accounts.lock().unwrap().insert("Lucid".into(), vec![funded, account("", "1235")]);
        let desk = Desk::new(fake);
        let login = logins(&store)[0].clone();
        assert!(desk.begin(&lucid));
        let session = desk.broker().sign_in(&login, Box::new(|| {}));
        desk.opened(&lucid, session);
        assert!(desk.note(&store));
        assert!(!desk.note(&store), "nothing new the second time");
        let cached = accounts(&store);
        assert_eq!(cached.iter().map(|a| a.key()).collect::<Vec<_>>(), vec![format!("{lucid}/1234"), format!("{lucid}/1235")]);
        assert_eq!(cached[0].balance_text().as_deref(), Some("$50,210"));
        assert_eq!(cached[1].balance_text(), None);

        assert!(note_balance(&store, &lucid, "1235", 48_900.0));
        cache_accounts(&store, &lucid, vec![account("", "1235")]);
        assert_eq!(accounts(&store)[0].balance, Some(48_900.0), "kept across a listing without one");
    }

    /// The default is the one made default while it is listed, else the
    /// first; a chart's own account wins while it is listed, and one that is
    /// not is stale and falls back to the default.
    #[test]
    fn a_charts_account_is_its_own_or_the_default() {
        let accounts = vec![account("a", "1234"), account("a", "1235"), account("b", "951")];
        let key = |account: Option<&Account>| account.map(Account::key);
        assert_eq!(key(default_account(&accounts, None)).as_deref(), Some("a/1234"));
        assert_eq!(key(default_account(&accounts, Some("b/951"))).as_deref(), Some("b/951"));
        assert_eq!(key(default_account(&accounts, Some("gone/1"))).as_deref(), Some("a/1234"), "stale is ignored");
        assert_eq!(key(account_of(&accounts, Some("b/951"), None)).as_deref(), Some("b/951"));
        assert_eq!(key(account_of(&accounts, Some("b/951"), Some("a/1235"))).as_deref(), Some("a/1235"));
        assert_eq!(key(account_of(&accounts, Some("b/951"), Some("gone/1"))).as_deref(), Some("b/951"));
        assert!(stale(&accounts, Some("gone/1")));
        assert!(!stale(&accounts, Some("a/1235")));
        assert!(!stale(&accounts, None));
        assert_eq!(account_of(&[], None, Some("a/1")), None);
    }

    /// The header's rule, for every case it has: offered when there is more
    /// than one account or more than one feed, naming only what can be
    /// chosen, plus trouble.
    #[test]
    fn the_account_part_names_only_what_can_be_chosen() {
        let one_login = vec![Login {
            id: "a".into(),
            name: "Rithmic Test".into(),
            user: "u".into(),
            system: TEST_SYSTEM.into(),
            gateway: TEST_GATEWAY.into(),
            signed_out: false,
        }];
        let mut two_logins = one_login.clone();
        two_logins.push(Login { id: "b".into(), name: "Lucid".into(), ..one_login[0].clone() });
        let a951 = account("a", "951");
        let b1234 = account("b", "1234");
        let feed = || "Rithmic".to_string();
        let text = |accounts: usize, feeds: usize, account: Option<(&Account, &[Login])>, trouble: Option<&str>| {
            part_text(Offer::of(accounts, feeds), account, trouble, feed)
        };

        // 0 or 1 account and one feed: nothing to choose, nothing drawn.
        assert_eq!(text(0, 1, None, None), None);
        assert_eq!(text(1, 1, Some((&a951, &one_login)), None), None);
        assert_eq!(text(1, 1, Some((&a951, &one_login)), Some("signed out")), None);
        // 0 or 1 account and several feeds: the feed, as today.
        assert_eq!(text(0, 2, None, None).as_deref(), Some("Rithmic"));
        assert_eq!(text(1, 2, Some((&a951, &one_login)), None).as_deref(), Some("Rithmic"));
        // ...with trouble on the account said.
        assert_eq!(text(1, 2, Some((&a951, &one_login)), Some("signed out")).as_deref(), Some("951 signed out · Rithmic"));
        // Several accounts and one feed: the account alone.
        assert_eq!(text(2, 1, Some((&a951, &one_login)), None).as_deref(), Some("951"));
        // Several accounts and several feeds: both.
        assert_eq!(text(2, 2, Some((&a951, &one_login)), None).as_deref(), Some("951 · Rithmic"));
        assert_eq!(text(3, 2, Some((&a951, &one_login)), Some("signed out")).as_deref(), Some("951 signed out · Rithmic"));
        // Several logins: the login's name before the number.
        assert_eq!(text(2, 2, Some((&b1234, &two_logins)), None).as_deref(), Some("Lucid 1234 · Rithmic"));
        assert_eq!(text(2, 1, Some((&b1234, &two_logins)), None).as_deref(), Some("Lucid 1234"));

        // Which lists the popover shows, and when they are captioned.
        for (accounts, feeds, offer) in [
            (0, 1, Offer { accounts: false, feeds: false }),
            (1, 1, Offer { accounts: false, feeds: false }),
            (2, 1, Offer { accounts: true, feeds: false }),
            (0, 2, Offer { accounts: false, feeds: true }),
            (1, 3, Offer { accounts: false, feeds: true }),
            (2, 2, Offer { accounts: true, feeds: true }),
        ] {
            assert_eq!(Offer::of(accounts, feeds), offer, "{accounts} accounts, {feeds} feeds");
            assert_eq!(offer.captions(), offer.accounts && offer.feeds);
        }
        assert_ne!(Offer::of(2, 1).tooltip(), Offer::of(1, 2).tooltip());
    }

    #[test]
    fn balances_read_in_whole_units() {
        let mut held = account("a", "1");
        held.balance = Some(1_000_000.0);
        assert_eq!(held.balance_text().as_deref(), Some("$1,000,000"));
        held.balance = Some(-950.6);
        assert_eq!(held.balance_text().as_deref(), Some("-$951"));
        held.currency = Some("EUR".into());
        held.balance = Some(999.0);
        assert_eq!(held.balance_text().as_deref(), Some("999 EUR"));
    }
}
