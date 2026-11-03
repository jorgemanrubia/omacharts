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

/// How long closing waits for the trading sessions to sign out: what the
/// data session waits.
pub const SIGN_OUT_WAIT: std::time::Duration = rithmic::SIGN_OUT_WAIT;

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
    // The login is named rather than read back from what the engine was
    // last told: that is the process's, and another test's beside this one.
    let forgotten = match data.as_deref() == Some(id) {
        true => session::sign_out_login(secrets, &login.id, &login.user, &login.system),
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

/// What removing a login does to the Rithmic data feed, which the dialog
/// asking to remove it says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Removal {
    /// Not the data login: the data feed is not touched.
    Login,
    /// The data login, and another login takes the data over: this one.
    HandOver { to: String },
    /// The data login, and the last: whether the Rithmic data feed goes too
    /// is asked.
    Ask,
    /// The data login, the last, and Rithmic is the last data feed, which
    /// cannot be removed: it stays, not signed in.
    KeepFeed,
}

/// What removing the login `id` would do to the data feed.
pub fn removal(store: &Store, id: &str) -> Removal {
    let logins = logins(store);
    if data_login_id(store, &logins).as_deref() != Some(id) {
        return Removal::Login;
    }
    if let Some(next) = logins.iter().find(|login| login.id != id) {
        return Removal::HandOver { to: next.name.clone() };
    }
    let fed = crate::feeds::stored_order(store).iter().any(|known| known.eq_ignore_ascii_case(RITHMIC));
    match fed && crate::feeds::removable(store, RITHMIC) {
        true => Removal::Ask,
        false => Removal::KeepFeed,
    }
}

/// Remove a login, its password and its accounts. If market data came
/// through it, it comes through the next login now; with none left the
/// Rithmic data feed goes too when `feed_too` says so and it is not the last
/// data feed, and otherwise stays, not signed in, to be signed in again.
pub fn remove_login(store: &Store, secrets: &dyn Secrets, id: &str, feed_too: bool) -> Result<(), String> {
    let decided = removal(store, id);
    let mut logins = logins(store);
    let Some(at) = logins.iter().position(|login| login.id == id) else { return Ok(()) };
    let was_data = decided != Removal::Login;
    let login = &logins[at];
    let forgotten = match was_data {
        true => session::sign_out_login(secrets, &login.id, &login.user, &login.system),
        false => session::forget_password(secrets, &login.id, &login.user, &login.system),
    };
    logins.remove(at);
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
            if let Some(next) = logins.first() {
                store.set_setting(DATA_LOGIN, &next.id);
            }
            session::hand_over();
        }
        Removal::Ask | Removal::KeepFeed => {
            store.remove_setting(DATA_LOGIN);
            if decided == Removal::Ask && feed_too {
                crate::feeds::remove_source(store, RITHMIC);
            } else {
                session::hand_over();
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

/// What a broker's sessions can do, so that the menu offers only that: a
/// broker without stop orders has no Stop rows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Capabilities {
    pub kinds: Vec<OrderKind>,
    pub flatten: bool,
    pub cancel_all: bool,
    /// A working order's price can be changed where it stands.
    pub modify: bool,
    /// Working orders can be linked on the broker's side, so that one
    /// filling or cancelled cancels the others: a stop and a target that
    /// do not both outlive the position.
    pub link: bool,
}

/// What an order on an instrument is sent as: the contract trading now,
/// its exchange, and the step its price moves in.
#[derive(Clone, Debug, PartialEq)]
pub struct Contract {
    pub symbol: String,
    pub exchange: String,
    pub tick: Option<f64>,
}

/// Why an instrument cannot be traded through a session.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The broker does not trade it at all.
    NotTradable,
    /// It does, and which contract is not known yet; the session will not
    /// stop to ask.
    NotKnownYet,
}

/// A login's signed-in trading session.
///
/// **The seam for brokers.** [`Broker`] and this are everything the window,
/// the panel, the chart menu, the marks and the theme know of a broker:
/// what is above them deals only in the neutral types here ([`Book`],
/// [`Order`], [`OrderKind`], [`Status`], [`Refusal`]), so Interactive
/// Brokers or Tradovate is a second implementation of the two traits and
/// nothing else. Rithmic is the first, in [`Rithmic`] below; the tests'
/// session is another.
///
/// Accounts are named by their number here: a session is one login's, and
/// [`Desk::book`] puts the login in front.
///
/// Placing, moving, linking, cancelling and flattening block until the broker has answered,
/// so they are for a thread of their own; what becomes of an order arrives
/// later on the book, with `changed`. [`Session::contract_for`] does not
/// block: it is asked on the main thread, as a menu opens.
pub trait Session: Send + Sync {
    /// The accounts, as the broker listed them at sign-in.
    fn accounts(&self) -> Vec<Account>;
    fn balance(&self, account: &str) -> Option<f64>;
    /// What its orders can be.
    fn capabilities(&self) -> Capabilities;
    /// Is this a simulated account, where an order costs nothing? Then
    /// nothing asks before placing one.
    fn is_simulated(&self) -> bool;
    /// The contract an order on `instrument` goes to, from what the
    /// session already knows. Never waits on the broker.
    fn contract_for(&self, instrument: &omacharts_engine::Instrument) -> Result<Contract, Refusal>;
    /// Every order, position and balance it has heard of.
    fn book(&self) -> Book;
    /// Place `order` and return the broker's id for it. Never retried.
    fn place(&self, order: &Order) -> Result<String, String>;
    fn cancel(&self, account: &str, basket: &str) -> Result<(), String>;
    /// Move the working order `basket` to `price`, keeping everything else
    /// about it. Only when [`Capabilities::modify`] says so.
    fn modify(&self, account: &str, basket: &str, price: f64) -> Result<(), String>;
    /// Link working orders so that one filling or cancelled cancels the
    /// rest, held by the broker. Only when [`Capabilities::link`] says so.
    fn link(&self, account: &str, baskets: &[String]) -> Result<(), String>;
    fn cancel_all(&self, account: &str) -> Result<(), String>;
    fn flatten(&self, account: &str, contract: &str, exchange: &str) -> Result<(), String>;
    /// Why it is no longer signed in, if it ended by itself.
    fn trouble(&self) -> Option<String>;
    fn sign_out(&self);
}

/// Rithmic's order and PnL plants, through `rithmic::Trading`.
pub struct Rithmic;

impl Broker for Rithmic {
    fn sign_in(&self, login: &Login, changed: Box<dyn Fn() + Send + Sync>) -> Result<Arc<dyn Session>, String> {
        let app = session::app().map_err(|error| error.to_string())?;
        // A developer's login file wins over the keyring, which it never
        // touches, as it does for the data session.
        let signing = match rithmic::Login::from_env(&login.system, &login.gateway) {
            Some(development) => development.map_err(|error| error.to_string())?,
            None => login.rithmic(),
        };
        let trading = rithmic::Trading::sign_in(app, &signing, changed).map_err(said)?;
        Ok(Arc::new(RithmicSession {
            login: login.id.clone(),
            simulated: login.system == TEST_SYSTEM,
            trading,
        }))
    }
}

/// The step a Rithmic contract's price moves in, from its root: "ES" for
/// "ESZ6", "6E" for "6EZ26".
fn rithmic_tick(contract: &str) -> Option<f64> {
    let digits = contract.trim_end_matches(|c: char| c.is_ascii_digit());
    let root = match digits.len() < contract.len() && digits.len() > 1 {
        true => &digits[..digits.len() - 1],
        false => contract,
    };
    omacharts_engine::exchanges::registry().contract_tick(root)
}

/// Rithmic's words, without "Rithmic refused…" in front of a refusal.
fn said(error: rithmic::Error) -> String {
    match error {
        rithmic::Error::Refused(why) | rithmic::Error::Rejected(why) => why,
        error => error.to_string(),
    }
}

struct RithmicSession {
    login: String,
    /// Signed in to Rithmic Test, which fills against a simulation.
    simulated: bool,
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

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            kinds: vec![OrderKind::Market, OrderKind::Limit, OrderKind::Stop],
            flatten: true,
            cancel_all: true,
            modify: true,
            link: true,
        }
    }

    fn is_simulated(&self) -> bool {
        self.simulated
    }

    /// A futures root on a CME group exchange, as the front month the data
    /// session last found for it.
    fn contract_for(&self, instrument: &omacharts_engine::Instrument) -> Result<Contract, Refusal> {
        if instrument.kind != omacharts_engine::InstrumentKind::FutureRoot {
            return Err(Refusal::NotTradable);
        }
        let (symbol, exchange) =
            omacharts_engine::providers::rithmic::contract_of(&instrument.symbol).ok_or(Refusal::NotKnownYet)?;
        Ok(Contract { tick: rithmic_tick(&symbol), symbol, exchange: exchange.to_string() })
    }

    fn book(&self) -> Book {
        let book = self.trading.book();
        let side = |side| match side {
            rithmic::Side::Buy => Side::Buy,
            rithmic::Side::Sell => Side::Sell,
        };
        Book {
            capabilities: HashMap::new(),
            accounts: book
                .accounts
                .into_iter()
                .map(|money| AccountPnl {
                    key: money.account,
                    balance: money.balance,
                    realized_day: money.realized_day,
                    unrealized: money.unrealized,
                })
                .collect(),
            positions: book
                .positions
                .into_iter()
                .map(|held| Position {
                    tick: rithmic_tick(&held.contract),
                    account: held.account,
                    contract: held.contract,
                    exchange: held.exchange,
                    net: held.net,
                    avg: held.avg,
                    open_pnl: held.open_pnl,
                })
                .collect(),
            orders: book
                .orders
                .into_iter()
                .map(|order| WorkingOrder {
                    said: order.said.clone(),
                    tick: rithmic_tick(&order.contract),
                    account: order.account,
                    basket_id: order.basket_id,
                    contract: order.contract,
                    exchange: order.exchange,
                    side: side(order.side),
                    qty: order.qty,
                    kind: match order.kind {
                        rithmic::Kind::Market => OrderKind::Market,
                        rithmic::Kind::Limit => OrderKind::Limit,
                        rithmic::Kind::Stop => OrderKind::Stop,
                    },
                    price: order.price,
                    status: match order.status {
                        rithmic::Status::Pending => Status::Pending,
                        rithmic::Status::Working => Status::Working,
                        rithmic::Status::PartiallyFilled => Status::PartiallyFilled,
                        rithmic::Status::Filled => Status::Filled,
                        rithmic::Status::Cancelled => Status::Cancelled,
                        rithmic::Status::Rejected(why) => Status::Rejected(why),
                    },
                    fill_price: order.fill_price,
                    at: order.at,
                    tag: order.tag,
                })
                .collect(),
        }
    }

    fn place(&self, order: &Order) -> Result<String, String> {
        let placed = rithmic::Order {
            account: order.account.clone(),
            contract: order.contract.clone(),
            exchange: order.exchange.clone(),
            side: match order.side {
                Side::Buy => rithmic::Side::Buy,
                Side::Sell => rithmic::Side::Sell,
            },
            qty: order.qty,
            kind: match order.kind {
                OrderKind::Market => rithmic::Kind::Market,
                OrderKind::Limit => rithmic::Kind::Limit,
                OrderKind::Stop => rithmic::Kind::Stop,
            },
            price: order.price,
            tag: order.tag.clone(),
        };
        self.trading.place(&placed).map_err(said)
    }

    fn cancel(&self, account: &str, basket: &str) -> Result<(), String> {
        self.trading.cancel(account, basket).map_err(said)
    }

    fn modify(&self, account: &str, basket: &str, price: f64) -> Result<(), String> {
        self.trading.modify(account, basket, price).map_err(said)
    }

    fn link(&self, account: &str, baskets: &[String]) -> Result<(), String> {
        self.trading.link(account, baskets).map_err(said)
    }

    fn cancel_all(&self, account: &str) -> Result<(), String> {
        self.trading.cancel_all(account).map_err(said)
    }

    fn flatten(&self, account: &str, contract: &str, exchange: &str) -> Result<(), String> {
        self.trading.flatten(account, contract, exchange).map_err(said)
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

    /// Sign every session out, as the window closes, each on a thread of
    /// its own, and wait until they all have or `wait` has passed. A
    /// session that has not finished by then is left to the process
    /// ending; nothing stays signed in that was told to sign out.
    pub fn sign_out_all(&self, wait: std::time::Duration) {
        self.opening.borrow_mut().clear();
        let sessions: Vec<Arc<dyn Session>> = self.sessions.borrow_mut().drain().map(|(_, session)| session).collect();
        if sessions.is_empty() {
            return;
        }
        let (done, finished) = std::sync::mpsc::channel();
        for session in &sessions {
            let session = session.clone();
            let done = done.clone();
            std::thread::spawn(move || {
                session.sign_out();
                let _ = done.send(());
            });
        }
        let until = std::time::Instant::now() + wait;
        for _ in &sessions {
            let left = until.saturating_duration_since(std::time::Instant::now());
            if finished.recv_timeout(left).is_err() {
                break;
            }
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

    /// Every live session's book, one, its accounts named by key: the
    /// login's id before the number. Logins in the order given.
    pub fn book(&self, logins: &[Login]) -> Book {
        let mut book = Book::default();
        for login in logins {
            let Some(session) = self.session(&login.id) else { continue };
            let theirs = session.book();
            book.capabilities.insert(login.id.clone(), session.capabilities());
            let key = |number: String| format!("{}/{number}", login.id);
            book.accounts.extend(theirs.accounts.into_iter().map(|money| AccountPnl { key: key(money.key), ..money }));
            book.positions
                .extend(theirs.positions.into_iter().map(|held| Position { account: key(held.account), ..held }));
            book.orders.extend(theirs.orders.into_iter().map(|order| WorkingOrder { account: key(order.account), ..order }));
        }
        book.orders.sort_by_key(|order| std::cmp::Reverse(order.at));
        book
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
    /// What the live sessions know of orders, positions and money; empty
    /// with none signed in. The window fills it from [`Desk::book`].
    pub book: Book,
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
            book: Book::default(),
        }
    }

    /// Is trading anywhere to be seen? Only with a login: without one there
    /// is no panel, no toggle, no order in a menu and nothing on a chart.
    pub fn trades(&self) -> bool {
        !self.logins.is_empty()
    }

    pub fn login(&self, id: &str) -> Option<&Login> {
        login_in(&self.logins, id)
    }

    /// The account a chart that chose `chosen` is about.
    pub fn account_of(&self, chosen: Option<&str>) -> Option<&Account> {
        account_of(&self.accounts, self.default.as_deref(), chosen)
    }

    /// What `account` is called here: see [`account_name`].
    pub fn account_name(&self, account: &Account) -> String {
        account_name(account, &self.logins, &self.accounts)
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

// -- orders -----------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Buy,
    Sell,
}

impl Side {
    pub fn word(self) -> &'static str {
        match self {
            Side::Buy => "Buy",
            Side::Sell => "Sell",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Side::Buy => "buy",
            Side::Sell => "sell",
        }
    }

    /// A shape for the side, so it is not told by colour alone: ▲ to buy,
    /// ▼ to sell.
    pub fn glyph(self) -> &'static str {
        match self {
            Side::Buy => "▲",
            Side::Sell => "▼",
        }
    }

    pub fn from_key(key: &str) -> Option<Side> {
        [Side::Buy, Side::Sell].into_iter().find(|side| side.key() == key)
    }
}

/// How an order is priced. A stop is a stop-market.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OrderKind {
    Market,
    Limit,
    Stop,
}

impl OrderKind {
    pub fn word(self) -> &'static str {
        match self {
            OrderKind::Market => "Market",
            OrderKind::Limit => "Limit",
            OrderKind::Stop => "Stop",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            OrderKind::Market => "market",
            OrderKind::Limit => "limit",
            OrderKind::Stop => "stop",
        }
    }

    pub fn from_key(key: &str) -> Option<OrderKind> {
        [OrderKind::Market, OrderKind::Limit, OrderKind::Stop].into_iter().find(|kind| kind.key() == key)
    }
}

/// Where an order stands, whichever broker it is with: each session maps
/// its own words into these.
#[derive(Clone, Debug, PartialEq)]
pub enum Status {
    /// Sent, not yet working.
    Pending,
    Working,
    PartiallyFilled,
    Filled,
    Cancelled,
    /// Refused, in the broker's own words.
    Rejected(String),
}

impl Status {
    /// Still to fill or be cancelled, so it can be cancelled.
    pub fn working(&self) -> bool {
        matches!(self, Status::Pending | Status::Working | Status::PartiallyFilled)
    }

    pub fn word(&self) -> &'static str {
        match self {
            Status::Pending => "Pending",
            Status::Working => "Working",
            Status::PartiallyFilled => "Part filled",
            Status::Filled => "Filled",
            Status::Cancelled => "Cancelled",
            Status::Rejected(_) => "Rejected",
        }
    }
}

/// An account's money, as its broker pushed it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AccountPnl {
    /// [`Account::key`] on the desk; the number inside a session.
    pub key: String,
    pub balance: Option<f64>,
    pub realized_day: Option<f64>,
    pub unrealized: Option<f64>,
}

/// A position held in one contract.
#[derive(Clone, Debug, PartialEq)]
pub struct Position {
    /// The account's key on the desk; its number inside a session.
    pub account: String,
    pub contract: String,
    pub exchange: String,
    /// Positive long, negative short.
    pub net: i32,
    pub avg: Option<f64>,
    /// As the broker worked it out, never from point values here.
    pub open_pnl: Option<f64>,
    /// The step its price moves in, when the broker knows it.
    pub tick: Option<f64>,
}

/// An order the broker has told of, working or done.
#[derive(Clone, Debug, PartialEq)]
pub struct WorkingOrder {
    /// The account's key on the desk; its number inside a session.
    pub account: String,
    pub basket_id: String,
    pub contract: String,
    pub exchange: String,
    pub side: Side,
    pub qty: u32,
    pub kind: OrderKind,
    pub price: Option<f64>,
    pub status: Status,
    pub fill_price: Option<f64>,
    /// Unix seconds.
    pub at: i64,
    pub tag: Option<String>,
    /// The step its price moves in, when the broker knows it.
    pub tick: Option<f64>,
    /// What the broker last said of it in its own words, for display.
    pub said: Option<String>,
}

/// What every live session knows of orders, positions and money.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Book {
    pub accounts: Vec<AccountPnl>,
    pub positions: Vec<Position>,
    /// Newest first.
    pub orders: Vec<WorkingOrder>,
    /// What each live login's session can do, by login id.
    pub capabilities: HashMap<String, Capabilities>,
}

impl Book {
    /// What the session of the account with the key `account` can do.
    pub fn capabilities_of(&self, account: &str) -> Option<&Capabilities> {
        self.capabilities.get(account.split_once('/').map_or(account, |(login, _)| login))
    }

    pub fn money(&self, key: &str) -> Option<&AccountPnl> {
        self.accounts.iter().find(|money| money.key == key)
    }

    /// `account`'s position in `contract`, while one is held.
    pub fn position(&self, account: &str, contract: &str) -> Option<&Position> {
        self.positions.iter().find(|held| held.account == account && held.contract == contract && held.net != 0)
    }

    /// `account`'s working orders in `contract`.
    pub fn working<'a>(&'a self, account: &'a str, contract: &'a str) -> impl Iterator<Item = &'a WorkingOrder> {
        self.orders
            .iter()
            .filter(move |order| order.account == account && order.contract == contract && order.status.working())
    }

    pub fn order(&self, basket: &str) -> Option<&WorkingOrder> {
        self.orders.iter().find(|order| order.basket_id == basket)
    }
}

/// An order to place: one contract, good for the day, entered by hand.
#[derive(Clone, Debug, PartialEq)]
pub struct Order {
    /// The account's number, inside its login's session.
    pub account: String,
    pub contract: String,
    pub exchange: String,
    pub side: Side,
    pub qty: u32,
    pub kind: OrderKind,
    pub price: Option<f64>,
    /// `omacharts:<nanos>`, to know it again.
    pub tag: String,
}

/// A fresh tag for an order placed now.
pub fn new_tag() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|since| since.as_nanos())
        .unwrap_or_default();
    format!("omacharts:{nanos}")
}

/// Where an order from a chart goes: its account, inside a login that is
/// signed in, and the contract its instrument trades as now.
#[derive(Clone, Debug, PartialEq)]
pub struct Target {
    pub account: Account,
    pub login: Login,
    pub contract: String,
    pub exchange: String,
    /// The step the contract's price moves in, when the broker knows it.
    pub tick: Option<f64>,
    /// The session says its account is simulated.
    pub simulated: bool,
    pub capabilities: Capabilities,
}

impl Target {
    /// Money is at stake: anything not simulated.
    pub fn live(&self) -> bool {
        !self.simulated
    }
}

/// Where an order from a chart of `instrument`, on the account `chosen`,
/// would go, or why it cannot: the one rule the menu and the click both
/// ask. `session` is a login's session while it is signed in.
pub fn order_target(
    view: &View,
    session: impl Fn(&str) -> Option<Arc<dyn Session>>,
    chosen: Option<&str>,
    instrument: Option<&omacharts_engine::Instrument>,
) -> Result<Target, String> {
    // A login whose accounts were never listed has not signed in yet.
    let account = view.account_of(chosen).ok_or_else(|| match view.logins.first() {
        Some(login) => format!("{} not signed in", login.name),
        None => "No trading account".to_string(),
    })?;
    let name = view.account_name(account);
    let login = view.login(&account.login).ok_or_else(|| format!("{name} has no login"))?;
    let instrument = instrument.ok_or_else(|| "Nothing on this chart to trade".to_string())?;
    let session = session(&login.id).ok_or_else(|| format!("{name} not signed in"))?;
    let contract = session.contract_for(instrument).map_err(|refusal| match refusal {
        Refusal::NotTradable => format!("{} is not tradable on {name}", instrument.symbol),
        Refusal::NotKnownYet => format!("{}'s contract not known yet", instrument.symbol),
    })?;
    Ok(Target {
        account: account.clone(),
        login: login.clone(),
        contract: contract.symbol,
        exchange: contract.exchange,
        tick: contract.tick,
        simulated: session.is_simulated(),
        capabilities: session.capabilities(),
    })
}

/// The orders the chart menu offers, in its order: buy and sell at the
/// market, then a limit and a stop each way, and only the kinds the broker
/// takes.
pub fn menu_orders(capabilities: &Capabilities) -> Vec<(Side, OrderKind)> {
    [OrderKind::Market, OrderKind::Limit, OrderKind::Stop]
        .into_iter()
        .filter(|kind| capabilities.kinds.contains(kind))
        .flat_map(|kind| [(Side::Buy, kind), (Side::Sell, kind)])
        .collect()
}

/// May a `side` `kind` order go in at `price` with the market at `last`?
/// A limit waits for a better price and a stop for a worse one, so each is
/// only on one side of the market; at the market itself neither is.
pub fn side_allowed(kind: OrderKind, side: Side, price: f64, last: f64) -> bool {
    match (kind, side) {
        (OrderKind::Market, _) => true,
        (OrderKind::Limit, Side::Buy) | (OrderKind::Stop, Side::Sell) => price < last,
        (OrderKind::Limit, Side::Sell) | (OrderKind::Stop, Side::Buy) => price > last,
    }
}

/// Which side of the market a refused price was on, for the toast.
pub fn crossed_text(price: &str, kind: OrderKind, side: Side) -> String {
    let above = matches!((kind, side), (OrderKind::Limit, Side::Buy) | (OrderKind::Stop, Side::Sell));
    let which = if above { "above" } else { "below" };
    format!("{price} is now {which} the market · not placed")
}

/// How far from an order's line, in pixels, the pointer may be for the
/// menu to offer cancelling it.
pub const NEAR: f64 = 6.0;

/// The working order whose line is nearest `y`, within [`NEAR`]: `to_y`
/// puts a price on the chart.
pub fn order_near<'a>(
    orders: impl IntoIterator<Item = &'a WorkingOrder>,
    y: f64,
    to_y: impl Fn(f64) -> f64,
) -> Option<&'a WorkingOrder> {
    orders
        .into_iter()
        .filter(|order| order.status.working())
        .filter_map(|order| Some((order, (to_y(order.price?) - y).abs())))
        .filter(|(_, off)| *off <= NEAR)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(order, _)| order)
}

/// "Buy Limit", "Sell Stop".
pub fn order_words(side: Side, kind: OrderKind) -> String {
    format!("{} {}", side.word(), kind.word())
}

/// A row of the chart menu: "Buy Market · 1 ESZ6 · 951", "Sell Limit
/// 7,620.00".
pub fn menu_text(side: Side, kind: OrderKind, price: &str, contract: &str, account: &str) -> String {
    match kind {
        OrderKind::Market => format!("{} · 1 {contract} · {account}", order_words(side, kind)),
        _ => format!("{} {price}", order_words(side, kind)),
    }
}

/// The question before an order on a live account: "Buy 1 ESZ6 at market
/// on Lucid 1234 · LucidTrading?".
pub fn confirm_text(side: Side, kind: OrderKind, price: &str, target: &Target) -> String {
    let at = match kind {
        OrderKind::Market => "at market".to_string(),
        OrderKind::Limit => format!("limit {price}"),
        OrderKind::Stop => format!("stop {price}"),
    };
    format!(
        "{} 1 {} {at} on {} {} · {}?",
        side.word(),
        target.contract,
        target.login.name,
        target.account.id,
        target.login.system
    )
}

/// What a toast says when an order placed here moves on: filled, working,
/// or rejected. Nothing for a step that is neither.
pub fn outcome_text(order: &WorkingOrder, before: &Status, decimals: usize, account: &str) -> Option<String> {
    if *before == order.status {
        return None;
    }
    let price = |price: Option<f64>| price.map(|price| omacharts_engine::grouped_decimals(price, decimals));
    match &order.status {
        Status::Filled => {
            let done = match order.side {
                Side::Buy => "Bought",
                Side::Sell => "Sold",
            };
            Some(match price(order.fill_price) {
                Some(at) => format!("{done} {} {} at {at} · {account}", order.qty, order.contract),
                None => format!("{done} {} {} · {account}", order.qty, order.contract),
            })
        }
        Status::Working if order.kind != OrderKind::Market && *before == Status::Pending => {
            Some(format!("Working: {} {}", order_words(order.side, order.kind), price(order.price).unwrap_or_default()))
        }
        Status::Rejected(why) => Some(format!("Rejected: {why}")),
        _ => None,
    }
}

/// Money as a P&L reads: "+$125.00", "-$3.94", "+12.50 EUR".
pub fn pnl_text(value: f64, currency: Option<&str>) -> String {
    let sign = if value < 0.0 { "-" } else { "+" };
    let whole = grouped(value.abs().trunc() as u64);
    let cents = ((value.abs().fract() * 100.0).round() as u64).min(99);
    match currency {
        None | Some("USD") => format!("{sign}${whole}.{cents:02}"),
        Some(currency) => format!("{sign}{whole}.{cents:02} {currency}"),
    }
}

/// Money as a balance reads: "$1,000,000.00".
pub fn money_text(value: f64, currency: Option<&str>) -> String {
    let text = pnl_text(value, currency);
    match value < 0.0 {
        true => text,
        false => text.trim_start_matches('+').to_string(),
    }
}

/// A position's size: "+1", "-2".
pub fn net_text(net: i32) -> String {
    format!("{net:+}")
}

/// What a position's pill says: its size and Rithmic's open P&L, or its
/// price until the P&L has arrived.
pub fn position_text(held: &Position, decimals: usize, currency: Option<&str>) -> String {
    match (held.open_pnl, held.avg) {
        (Some(pnl), _) => format!("{} · {}", net_text(held.net), pnl_text(pnl, currency)),
        (None, Some(avg)) => format!("{} @ {}", net_text(held.net), omacharts_engine::grouped_decimals(avg, decimals)),
        (None, None) => net_text(held.net),
    }
}

/// The accounts with an order on its way, so a second click does not send
/// a second order before the first is acknowledged.
#[derive(Default)]
pub struct Placing(RefCell<HashSet<String>>);

impl Placing {
    /// Start placing on `account`; refused while one is on its way.
    pub fn begin(&self, account: &str) -> Result<(), String> {
        match self.0.borrow_mut().insert(account.to_string()) {
            true => Ok(()),
            false => Err("Still placing".into()),
        }
    }

    /// The broker answered, either way.
    pub fn done(&self, account: &str) {
        self.0.borrow_mut().remove(account);
    }
}

// -- the header's account part -----------------------------------------------

/// The feed a login's market data comes through: the data login's, the one
/// a chart on one of its accounts is drawn from unless somebody chose
/// another.
pub const LOGIN_FEED: &str = "rithmic";

/// Whether a chart's header has an account part: when there is more than
/// one account or more than one feed to choose from.
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
}

/// What the account part says it opens.
pub const PART_TOOLTIP: &str = "Account: click to choose this chart's account and data feed";

/// How an account is named: its login's name, "Lucid", with the account
/// number after it only when the name alone would not say which, because
/// the login holds several accounts or another login has the same name.
pub fn account_name(account: &Account, logins: &[Login], accounts: &[Account]) -> String {
    let Some(login) = login_in(logins, &account.login) else {
        return account.id.clone();
    };
    let several = accounts.iter().filter(|other| other.login == account.login).count() > 1;
    let shared = logins.iter().any(|other| other.id != login.id && other.name == login.name);
    match several || shared {
        true => format!("{} {}", login.name, account.id),
        false => login.name.clone(),
    }
}

/// The feed a chart on `account` is drawn from when nobody chose one: the
/// account's own login's feed while that login is the data login and the
/// feed serves `instrument`, else the first feed in the order that serves
/// it. Whether a feed can be used now does not move it: this is the one
/// the popover marks "(default)", and a chart falls back from it only for
/// as long as it cannot be used.
pub fn default_feed<'a>(
    view: &View,
    account: Option<&Account>,
    feeds: impl IntoIterator<Item = &'a dyn omacharts_engine::Provider> + Clone,
    instrument: &omacharts_engine::Instrument,
) -> Option<&'static str> {
    own_feed(view, account, feeds.clone().into_iter().map(|provider| (provider, true)), instrument).or_else(|| {
        feeds.into_iter().find(|provider| provider.symbol_for(instrument).is_some()).map(|provider| provider.id())
    })
}

/// The account's own login's feed, when that login is the data login and
/// the feed serves `instrument` and can be used.
fn own_feed<'a>(
    view: &View,
    account: Option<&Account>,
    feeds: impl IntoIterator<Item = (&'a dyn omacharts_engine::Provider, bool)>,
    instrument: &omacharts_engine::Instrument,
) -> Option<&'static str> {
    let account = account?;
    if view.data_login.as_deref() != Some(account.login.as_str()) {
        return None;
    }
    feeds
        .into_iter()
        .find(|(provider, _)| provider.id().eq_ignore_ascii_case(LOGIN_FEED))
        .filter(|(provider, usable)| *usable && provider.symbol_for(instrument).is_some())
        .map(|(provider, _)| provider.id())
}

/// Which feed a chart on `account` is pointed at: the one `chosen` for it,
/// else the account's own login's feed while that login is the data login
/// and the feed can serve `instrument` now. `None` leaves it to the order,
/// [`omacharts_engine::providers::route`]. The header and the order ticket
/// both ask this, through the window's one route, so they never disagree.
pub fn feed_of<'a>(
    view: &View,
    account: Option<&Account>,
    chosen: Option<&'a str>,
    feeds: impl IntoIterator<Item = (&'a dyn omacharts_engine::Provider, bool)>,
    instrument: &omacharts_engine::Instrument,
) -> Option<&'a str> {
    chosen.or_else(|| own_feed(view, account, feeds, instrument))
}

/// What the account part says.
///
/// The account whenever there is one, `account` being its name, with any
/// trouble after it ("Lucid 1234 signed out"); the feed, in the words
/// `feed` makes, when there is no account, or when there are several feeds
/// and it is not the account's default. Nothing when there is nothing to
/// choose.
pub fn part_text(
    offer: Offer,
    account: Option<&str>,
    trouble: Option<&str>,
    feed_is_default: bool,
    feed: impl FnOnce() -> String,
) -> Option<String> {
    if !offer.any() {
        return None;
    }
    let Some(name) = account else {
        return Some(feed());
    };
    let named = match trouble {
        Some(trouble) => format!("{name} {trouble}"),
        None => name.to_string(),
    };
    Some(match offer.feeds && !feed_is_default {
        true => format!("{named} · {}", feed()),
        false => named,
    })
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

    /// A second broker, in memory: a session that fills a market order at
    /// once, at `last`, and holds a limit or a stop until it is cancelled.
    /// Its book is the one a real session would push, with `changed` called
    /// after each move. That the window trades through it as it does
    /// through Rithmic is the proof that nothing above [`Session`] knows
    /// which broker it is.
    pub(crate) struct FakeSession {
        pub accounts: Vec<Account>,
        signed_out: Arc<AtomicUsize>,
        gone: std::sync::atomic::AtomicBool,
        pub book: Mutex<Book>,
        pub last: Mutex<f64>,
        pub changed: Box<dyn Fn() + Send + Sync>,
        baskets: AtomicUsize,
        /// The contract each future root trades as, by root.
        pub contracts: Mutex<HashMap<String, String>>,
        pub simulated: bool,
        pub kinds: Vec<OrderKind>,
        /// What it says it can do beyond placing and cancelling.
        pub modifies: bool,
        pub links_orders: bool,
        /// The groups linked, each cancelled whole when one of it ends.
        pub links: Mutex<Vec<Vec<String>>>,
        /// Refuse whatever is asked next, in these words.
        pub refuse: Mutex<Option<String>>,
    }

    impl FakeSession {
        pub(crate) fn new(accounts: Vec<Account>, changed: Box<dyn Fn() + Send + Sync>) -> FakeSession {
            FakeSession {
                accounts,
                signed_out: Arc::default(),
                gone: Default::default(),
                book: Mutex::default(),
                last: Mutex::new(7_612.25),
                changed,
                baskets: AtomicUsize::new(1),
                contracts: Mutex::default(),
                simulated: true,
                kinds: vec![OrderKind::Market, OrderKind::Limit, OrderKind::Stop],
                modifies: true,
                links_orders: true,
                links: Mutex::default(),
                refuse: Mutex::default(),
            }
        }

        fn refused(&self) -> Result<(), String> {
            match self.refuse.lock().unwrap().take() {
                Some(why) => Err(why),
                None => Ok(()),
            }
        }

        /// End `basket` and cancel every order linked to it, as a broker
        /// holding the link would.
        fn end_linked(&self, book: &mut Book, basket: &str) {
            let links = self.links.lock().unwrap();
            for group in links.iter().filter(|group| group.iter().any(|b| b == basket)) {
                for other in group.iter().filter(|b| *b != basket) {
                    if let Some(order) = book.orders.iter_mut().find(|o| o.basket_id == *other && o.status.working()) {
                        order.status = Status::Cancelled;
                    }
                }
            }
        }

        /// The market reaches the working order `basket`: it fills at its
        /// price, and whatever is linked to it is cancelled.
        pub(crate) fn fill(&self, basket: &str) {
            let mut book = self.book.lock().unwrap();
            let Some(order) = book.orders.iter_mut().find(|o| o.basket_id == basket && o.status.working()) else {
                return;
            };
            order.status = Status::Filled;
            order.fill_price = order.price;
            let order = order.clone();
            let bought = if order.side == Side::Buy { order.qty as i32 } else { -(order.qty as i32) };
            FakeSession::hold(&mut book, &order.account, &order.contract, &order.exchange, bought, order.price.unwrap_or(0.0));
            self.end_linked(&mut book, basket);
            drop(book);
            (self.changed)();
        }

        fn hold(book: &mut Book, account: &str, contract: &str, exchange: &str, bought: i32, at: f64) {
            match book.positions.iter_mut().find(|held| held.account == account && held.contract == contract) {
                Some(held) => {
                    held.net += bought;
                    held.avg = (held.net != 0).then_some(at);
                }
                None => book.positions.push(Position {
                    account: account.into(),
                    contract: contract.into(),
                    exchange: exchange.into(),
                    net: bought,
                    avg: Some(at),
                    open_pnl: None,
                    tick: Some(0.25),
                }),
            }
            book.positions.retain(|held| held.net != 0);
        }
    }

    impl Session for FakeSession {
        fn accounts(&self) -> Vec<Account> {
            self.accounts.clone()
        }
        fn balance(&self, account: &str) -> Option<f64> {
            self.accounts.iter().find(|a| a.id == account).and_then(|a| a.balance)
        }
        fn capabilities(&self) -> Capabilities {
            Capabilities {
                kinds: self.kinds.clone(),
                flatten: true,
                cancel_all: true,
                modify: self.modifies,
                link: self.links_orders,
            }
        }
        fn is_simulated(&self) -> bool {
            self.simulated
        }
        fn contract_for(&self, instrument: &omacharts_engine::Instrument) -> Result<Contract, Refusal> {
            if instrument.kind != omacharts_engine::InstrumentKind::FutureRoot {
                return Err(Refusal::NotTradable);
            }
            let symbol = self.contracts.lock().unwrap().get(&instrument.symbol).cloned().ok_or(Refusal::NotKnownYet)?;
            Ok(Contract { symbol, exchange: "CME".into(), tick: Some(0.25) })
        }
        fn book(&self) -> Book {
            self.book.lock().unwrap().clone()
        }
        fn place(&self, order: &Order) -> Result<String, String> {
            if self.gone.load(Ordering::Acquire) {
                return Err("the session was signed out".into());
            }
            self.refused()?;
            let basket = format!("B{}", self.baskets.fetch_add(1, Ordering::AcqRel));
            let last = *self.last.lock().unwrap();
            let filled = order.kind == OrderKind::Market;
            let mut book = self.book.lock().unwrap();
            book.orders.insert(
                0,
                WorkingOrder {
                    said: None,
                    account: order.account.clone(),
                    basket_id: basket.clone(),
                    contract: order.contract.clone(),
                    exchange: order.exchange.clone(),
                    side: order.side,
                    qty: order.qty,
                    kind: order.kind,
                    price: order.price,
                    status: if filled { Status::Filled } else { Status::Working },
                    fill_price: filled.then_some(last),
                    at: 0,
                    tag: Some(order.tag.clone()),
                    tick: Some(0.25),
                },
            );
            if filled {
                let bought = if order.side == Side::Buy { order.qty as i32 } else { -(order.qty as i32) };
                FakeSession::hold(&mut book, &order.account, &order.contract, &order.exchange, bought, last);
            }
            drop(book);
            (self.changed)();
            Ok(basket)
        }
        fn cancel(&self, _: &str, basket: &str) -> Result<(), String> {
            self.refused()?;
            let mut book = self.book.lock().unwrap();
            let order = book.orders.iter_mut().find(|order| order.basket_id == basket && order.status.working());
            let Some(order) = order else { return Err("not working".into()) };
            order.status = Status::Cancelled;
            self.end_linked(&mut book, basket);
            drop(book);
            (self.changed)();
            Ok(())
        }
        fn modify(&self, _: &str, basket: &str, price: f64) -> Result<(), String> {
            self.refused()?;
            let mut book = self.book.lock().unwrap();
            let order = book.orders.iter_mut().find(|order| order.basket_id == basket && order.status.working());
            let Some(order) = order else { return Err("not working".into()) };
            order.price = Some(price);
            drop(book);
            (self.changed)();
            Ok(())
        }
        fn link(&self, _: &str, baskets: &[String]) -> Result<(), String> {
            self.refused()?;
            self.links.lock().unwrap().push(baskets.to_vec());
            Ok(())
        }
        fn cancel_all(&self, account: &str) -> Result<(), String> {
            let mut book = self.book.lock().unwrap();
            for order in book.orders.iter_mut().filter(|order| order.account == account && order.status.working()) {
                order.status = Status::Cancelled;
            }
            drop(book);
            (self.changed)();
            Ok(())
        }
        fn flatten(&self, account: &str, contract: &str, exchange: &str) -> Result<(), String> {
            let last = *self.last.lock().unwrap();
            let mut book = self.book.lock().unwrap();
            let net = book.position(account, contract).map_or(0, |held| held.net);
            FakeSession::hold(&mut book, account, contract, exchange, -net, last);
            drop(book);
            (self.changed)();
            Ok(())
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
        fn sign_in(&self, login: &Login, changed: Box<dyn Fn() + Send + Sync>) -> Result<Arc<dyn Session>, String> {
            self.signed_in.fetch_add(1, Ordering::AcqRel);
            let accounts = self.accounts.lock().unwrap().get(&login.name).cloned().unwrap_or_default();
            let accounts = accounts.into_iter().map(|a| Account { login: login.id.clone(), ..a }).collect();
            Ok(Arc::new(FakeSession { signed_out: self.signed_out.clone(), ..FakeSession::new(accounts, changed) }))
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
        assert!(crate::feeds::remove_source(&store, "yahoo"));
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

    /// Closing signs every session out, and waits for them.
    #[test]
    fn closing_signs_every_session_out() {
        let fake = Arc::new(Fake::default());
        let desk = Desk::new(fake.clone());
        let logins = [test_login("1", "Lucid", TEST_SYSTEM), test_login("2", "Tradeify", TEST_SYSTEM)];
        for login in &logins {
            assert!(desk.begin(&login.id));
            desk.opened(&login.id, desk.broker().sign_in(login, Box::new(|| {})));
        }
        assert!(desk.begin("3"), "one still signing in");
        desk.sign_out_all(SIGN_OUT_WAIT);
        assert_eq!(fake.signed_out.load(Ordering::Acquire), 2);
        assert!(logins.iter().all(|login| !desk.live(&login.id)));
        assert!(!desk.opening("3"), "and one signing in is not kept when it comes back");
        desk.sign_out_all(SIGN_OUT_WAIT);
        assert_eq!(fake.signed_out.load(Ordering::Acquire), 2, "each once");
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

    /// The header's rule, case by case: nothing unless there is more than
    /// one account or more than one feed; the account whenever there is
    /// one; the feed only when it is not the account's default, or when
    /// there is no account; trouble after the thing in trouble.
    #[test]
    fn the_account_part_names_only_what_can_be_chosen() {
        let text = |accounts: usize, feeds: usize, account: Option<&str>, trouble: Option<&str>, default: bool, feed: &str| {
            let feed = feed.to_string();
            part_text(Offer::of(accounts, feeds), account, trouble, default, || feed)
        };

        // At most one account and one feed: blank, and not there at all.
        assert_eq!(text(0, 1, None, None, true, "Yahoo"), None);
        assert_eq!(text(1, 1, Some("Rithmic"), None, true, "Rithmic"), None);
        assert_eq!(text(1, 1, Some("Rithmic"), Some("signed out"), true, "Rithmic"), None);
        // No account, several feeds: the feed, and what it passed over.
        assert_eq!(text(0, 2, None, None, true, "Yahoo").as_deref(), Some("Yahoo"));
        assert_eq!(
            text(0, 2, None, None, true, "Yahoo · Rithmic not signed in").as_deref(),
            Some("Yahoo · Rithmic not signed in")
        );
        // One account on its default feed: the account alone.
        assert_eq!(text(1, 2, Some("Rithmic"), None, true, "Rithmic").as_deref(), Some("Rithmic"));
        // Overridden, or fallen back: the feed after it.
        assert_eq!(text(1, 2, Some("Rithmic"), None, false, "Yahoo").as_deref(), Some("Rithmic · Yahoo"));
        // Several accounts and one feed: the account, whatever the feed.
        assert_eq!(text(2, 1, Some("Lucid 1234"), None, false, "Yahoo").as_deref(), Some("Lucid 1234"));
        // Several of both.
        assert_eq!(text(2, 2, Some("Lucid 1234"), None, true, "Rithmic").as_deref(), Some("Lucid 1234"));
        assert_eq!(text(2, 2, Some("Lucid 1234"), None, false, "Yahoo").as_deref(), Some("Lucid 1234 · Yahoo"));
        // A login in trouble.
        assert_eq!(text(1, 2, Some("Rithmic"), Some("signed out"), true, "Rithmic").as_deref(), Some("Rithmic signed out"));
        assert_eq!(
            text(2, 2, Some("Lucid 1234"), Some("signed out"), false, "Yahoo").as_deref(),
            Some("Lucid 1234 signed out · Yahoo")
        );
        // A chosen feed that refused, and nothing serving at all.
        assert_eq!(
            text(1, 2, Some("Rithmic"), None, false, "Yahoo not signed in").as_deref(),
            Some("Rithmic · Yahoo not signed in")
        );
        assert_eq!(text(1, 2, Some("Rithmic"), None, false, "no data feed").as_deref(), Some("Rithmic · no data feed"));

        for (accounts, feeds, any) in [(0, 1, false), (1, 1, false), (2, 1, true), (0, 2, true), (1, 3, true)] {
            assert_eq!(Offer::of(accounts, feeds).any(), any, "{accounts} accounts, {feeds} feeds");
        }
    }

    /// An account goes by its login's name, with its number only when the
    /// name alone would not say which: the login holds several, or another
    /// login has the same name.
    #[test]
    fn an_account_is_named_by_its_login() {
        let rithmic = test_login("a", "Rithmic", TEST_SYSTEM);
        let lucid = test_login("b", "Lucid", "LucidTrading");
        let a951 = account("a", "951");
        let b1234 = account("b", "1234");
        let b1235 = account("b", "1235");

        assert_eq!(account_name(&a951, std::slice::from_ref(&rithmic), std::slice::from_ref(&a951)), "Rithmic");
        let logins = [rithmic.clone(), lucid.clone()];
        let accounts = [a951.clone(), b1234.clone(), b1235.clone()];
        assert_eq!(account_name(&a951, &logins, &accounts), "Rithmic");
        assert_eq!(account_name(&b1234, &logins, &accounts), "Lucid 1234");
        assert_eq!(account_name(&b1235, &logins, &accounts), "Lucid 1235");
        // Two logins called the same.
        let twin = Login { id: "c".into(), ..rithmic.clone() };
        let c7 = account("c", "7");
        assert_eq!(account_name(&a951, &[rithmic, twin], &[a951.clone(), c7.clone()]), "Rithmic 951");
        // A login that is gone: the number is all there is.
        assert_eq!(account_name(&c7, &[lucid], std::slice::from_ref(&c7)), "7");
    }

    /// A chart's feed: the one chosen for it; else its account's own,
    /// while that account's login is the data login and the feed serves the
    /// instrument and can be used; else the order decides. The default the
    /// popover marks is the account's own whether or not it can be used
    /// now, and the order's first that serves it otherwise.
    #[test]
    fn a_charts_feed_is_its_own_the_accounts_or_the_order() {
        use omacharts_engine::providers::built;
        use omacharts_engine::InstrumentKind;
        let yahoo = built("yahoo").expect("Yahoo");
        let rithmic = built("rithmic").expect("Rithmic");
        let es = instrument("ES", InstrumentKind::FutureRoot);
        let spy = instrument("SPY", InstrumentKind::Etf);
        assert!(rithmic.symbol_for(&spy).is_none() && rithmic.symbol_for(&es).is_some());
        let data = account("a", "951");
        let other = account("b", "1234");
        let view = View {
            logins: vec![test_login("a", "Rithmic", TEST_SYSTEM), test_login("b", "Lucid", "LucidTrading")],
            accounts: vec![data.clone(), other.clone()],
            data_login: Some("a".into()),
            ..View::default()
        };
        // Yahoo on top of the order, so the account's own is seen to win.
        let order = |rithmic_usable: bool| [(yahoo.as_ref(), true), (rithmic.as_ref(), rithmic_usable)];
        let plain = [yahoo.as_ref(), rithmic.as_ref()];

        assert_eq!(feed_of(&view, Some(&data), Some("yahoo"), order(true), &es), Some("yahoo"), "chosen wins");
        assert_eq!(feed_of(&view, Some(&data), None, order(true), &es), Some("rithmic"), "the account's own");
        assert_eq!(feed_of(&view, Some(&data), None, order(false), &es), None, "falls back to the order");
        assert_eq!(feed_of(&view, Some(&data), None, order(true), &spy), None, "its own does not serve it");
        assert_eq!(feed_of(&view, Some(&other), None, order(true), &es), None, "not the data login");
        assert_eq!(feed_of(&view, None, None, order(true), &es), None, "no account");

        assert_eq!(default_feed(&view, Some(&data), plain, &es), Some("rithmic"));
        assert_eq!(default_feed(&view, Some(&data), plain, &spy), Some("yahoo"), "the order's pick");
        assert_eq!(default_feed(&view, Some(&other), plain, &es), Some("yahoo"));
        assert_eq!(default_feed(&view, None, [rithmic.as_ref(), yahoo.as_ref()], &es), Some("rithmic"));
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

    fn instrument(symbol: &str, kind: omacharts_engine::InstrumentKind) -> omacharts_engine::Instrument {
        omacharts_engine::Instrument {
            symbol: symbol.into(),
            name: symbol.into(),
            kind,
            suffix: None,
            currency: Some("USD".into()),
            tier: 0,
            session_origin: 0,
            overrides: Vec::new(),
            exchange: None,
            popularity: 0,
            local_name: None,
        }
    }

    fn test_login(id: &str, name: &str, system: &str) -> Login {
        Login {
            id: id.into(),
            name: name.into(),
            user: "u".into(),
            system: system.into(),
            gateway: TEST_GATEWAY.into(),
            signed_out: false,
        }
    }

    fn order(basket: &str, side: Side, kind: OrderKind, price: Option<f64>, status: Status) -> WorkingOrder {
        WorkingOrder {
            said: None,
            account: "t/951".into(),
            basket_id: basket.into(),
            contract: "ESZ6".into(),
            exchange: "CME".into(),
            side,
            qty: 1,
            kind,
            price,
            status,
            fill_price: None,
            at: 0,
            tag: None,
            tick: Some(0.25),
        }
    }

    /// A limit waits for a better price and a stop for a worse one; at the
    /// market neither is allowed, and a market order always is.
    #[test]
    fn a_limit_and_a_stop_are_each_on_one_side_of_the_market() {
        let last = 7_612.25;
        assert!(side_allowed(OrderKind::Limit, Side::Buy, 7_600.0, last));
        assert!(!side_allowed(OrderKind::Limit, Side::Buy, 7_620.0, last));
        assert!(side_allowed(OrderKind::Limit, Side::Sell, 7_620.0, last));
        assert!(!side_allowed(OrderKind::Limit, Side::Sell, 7_600.0, last));
        assert!(side_allowed(OrderKind::Stop, Side::Buy, 7_620.0, last));
        assert!(!side_allowed(OrderKind::Stop, Side::Buy, 7_600.0, last));
        assert!(side_allowed(OrderKind::Stop, Side::Sell, 7_600.0, last));
        assert!(!side_allowed(OrderKind::Stop, Side::Sell, 7_620.0, last));
        for kind in [OrderKind::Limit, OrderKind::Stop] {
            for side in [Side::Buy, Side::Sell] {
                assert!(!side_allowed(kind, side, last, last), "{kind:?} {side:?} at the market");
            }
        }
        assert!(side_allowed(OrderKind::Market, Side::Sell, 0.0, last));

        // Only what the broker takes is offered.
        let all = Capabilities {
            kinds: vec![OrderKind::Stop, OrderKind::Market, OrderKind::Limit],
            flatten: true,
            cancel_all: true,
            modify: true,
            link: true,
        };
        assert_eq!(menu_orders(&all).len(), 6);
        assert_eq!(menu_orders(&all)[..2], [(Side::Buy, OrderKind::Market), (Side::Sell, OrderKind::Market)]);
        let no_stops = Capabilities { kinds: vec![OrderKind::Market, OrderKind::Limit], ..all };
        assert!(menu_orders(&no_stops).iter().all(|(_, kind)| *kind != OrderKind::Stop));
        assert_eq!(menu_orders(&no_stops).len(), 4);
        assert_eq!(crossed_text("7,612.50", OrderKind::Limit, Side::Buy), "7,612.50 is now above the market · not placed");
        assert_eq!(crossed_text("7,600.00", OrderKind::Limit, Side::Sell), "7,600.00 is now below the market · not placed");
    }

    /// The menu offers to cancel the working order whose line the pointer
    /// is on, within six pixels, the nearest of two, and never a done one.
    #[test]
    fn the_order_near_the_pointer_is_the_nearest_working_one() {
        let orders = vec![
            order("a", Side::Sell, OrderKind::Limit, Some(7_620.0), Status::Working),
            order("b", Side::Sell, OrderKind::Stop, Some(7_622.0), Status::Working),
            order("c", Side::Buy, OrderKind::Limit, Some(7_600.0), Status::Cancelled),
            order("d", Side::Buy, OrderKind::Market, None, Status::Pending),
        ];
        // A point a pixel.
        let to_y = |price: f64| 8_000.0 - price;
        let near = |y: f64| order_near(&orders, y, to_y).map(|order| order.basket_id.as_str());
        assert_eq!(near(380.0), Some("a"));
        assert_eq!(near(379.5), Some("a"));
        assert_eq!(near(378.4), Some("b"), "nearer the stop");
        assert_eq!(near(386.0), Some("a"), "six pixels is near");
        assert_eq!(near(386.5), None);
        assert_eq!(near(400.0), None, "a cancelled order is not offered");
    }

    #[test]
    fn what_the_menu_toasts_and_dialog_say() {
        assert_eq!(menu_text(Side::Buy, OrderKind::Market, "", "ESZ6", "951"), "Buy Market · 1 ESZ6 · 951");
        assert_eq!(menu_text(Side::Sell, OrderKind::Limit, "7,620.00", "ESZ6", "951"), "Sell Limit 7,620.00");
        assert_eq!(menu_text(Side::Buy, OrderKind::Stop, "7,630.25", "ESZ6", "951"), "Buy Stop 7,630.25");
        let target = Target {
            account: account("l", "1234"),
            login: test_login("l", "Lucid", "LucidTrading"),
            contract: "ESZ6".into(),
            exchange: "CME".into(),
            tick: Some(0.25),
            simulated: false,
            capabilities: Capabilities {
                kinds: vec![OrderKind::Market],
                flatten: true,
                cancel_all: true,
                modify: true,
                link: true,
            },
        };
        assert!(target.live());
        assert_eq!(confirm_text(Side::Buy, OrderKind::Market, "", &target), "Buy 1 ESZ6 at market on Lucid 1234 · LucidTrading?");
        assert_eq!(
            confirm_text(Side::Sell, OrderKind::Limit, "7,620.00", &target),
            "Sell 1 ESZ6 limit 7,620.00 on Lucid 1234 · LucidTrading?"
        );
        assert!(!Target { simulated: true, ..target }.live(), "the session says so, not the system's name");

        let mut bought = order("a", Side::Buy, OrderKind::Market, None, Status::Filled);
        bought.fill_price = Some(7_612.25);
        assert_eq!(outcome_text(&bought, &Status::Pending, 2, "951").as_deref(), Some("Bought 1 ESZ6 at 7,612.25 · 951"));
        assert_eq!(outcome_text(&bought, &Status::Filled, 2, "951"), None, "said once");
        let working = order("b", Side::Sell, OrderKind::Limit, Some(7_620.0), Status::Working);
        assert_eq!(outcome_text(&working, &Status::Pending, 2, "951").as_deref(), Some("Working: Sell Limit 7,620.00"));
        let refused = order("c", Side::Buy, OrderKind::Limit, Some(1.0), Status::Rejected("price out of band".into()));
        assert_eq!(outcome_text(&refused, &Status::Pending, 2, "951").as_deref(), Some("Rejected: price out of band"));
        assert_eq!(outcome_text(&order("d", Side::Buy, OrderKind::Limit, Some(1.0), Status::Cancelled), &Status::Working, 2, "951"), None);

        let mut held = Position {
            account: "t/951".into(),
            contract: "ESZ6".into(),
            exchange: "CME".into(),
            net: 1,
            avg: Some(7_612.25),
            open_pnl: None,
            tick: Some(0.25),
        };
        assert_eq!(position_text(&held, 2, Some("USD")), "+1 @ 7,612.25");
        held.open_pnl = Some(125.0);
        assert_eq!(position_text(&held, 2, Some("USD")), "+1 · +$125.00");
        held.net = -2;
        held.open_pnl = Some(-3.94);
        assert_eq!(position_text(&held, 2, None), "-2 · -$3.94");
        assert_eq!(money_text(1_000_000.0, Some("USD")), "$1,000,000.00");
        assert_eq!(pnl_text(12.5, Some("EUR")), "+12.50 EUR");
    }

    /// Every way a chart cannot take an order, in the words the menu shows.
    #[test]
    fn an_order_target_says_why_it_cannot_be_had() {
        use omacharts_engine::InstrumentKind;
        let view = View {
            logins: vec![test_login("t", "Rithmic Test", TEST_SYSTEM)],
            accounts: vec![account("t", "951")],
            ..View::default()
        };
        let es = instrument("ES", InstrumentKind::FutureRoot);
        let aapl = instrument("AAPL", InstrumentKind::Equity);
        let fake = FakeSession::new(vec![account("t", "951")], Box::new(|| {}));
        fake.contracts.lock().unwrap().insert("ES".into(), "ESZ6".into());
        let session: Arc<dyn Session> = Arc::new(fake);
        let target = |live: bool, what: &omacharts_engine::Instrument| {
            order_target(&view, |_| live.then(|| session.clone()), None, Some(what))
        };
        let found = target(true, &es).unwrap();
        assert_eq!((found.contract.as_str(), found.account.id.as_str()), ("ESZ6", "951"));
        assert!(found.simulated, "as the session says");
        assert_eq!(target(true, &aapl).unwrap_err(), "AAPL is not tradable on Rithmic Test");
        assert_eq!(target(false, &es).unwrap_err(), "Rithmic Test not signed in");
        let nq = instrument("NQ", InstrumentKind::FutureRoot);
        assert_eq!(target(true, &nq).unwrap_err(), "NQ's contract not known yet");
        let unlisted = View { accounts: Vec::new(), ..view.clone() };
        assert_eq!(order_target(&unlisted, |_| None, None, Some(&es)).unwrap_err(), "Rithmic Test not signed in");
        assert_eq!(order_target(&View::default(), |_| None, None, Some(&es)).unwrap_err(), "No trading account");
    }

    #[test]
    fn a_second_order_waits_for_the_first_to_be_answered() {
        let placing = Placing::default();
        assert!(placing.begin("t/951").is_ok());
        assert_eq!(placing.begin("t/951").unwrap_err(), "Still placing");
        assert!(placing.begin("l/1234").is_ok(), "another account is not held up");
        placing.done("t/951");
        assert!(placing.begin("t/951").is_ok());
    }

    /// The desk's book is every live session's, under the accounts' keys:
    /// a market order fills and is held, a limit works until cancelled, and
    /// flattening leaves nothing held. Each move calls `changed`.
    #[test]
    fn the_desks_book_follows_what_the_sessions_push() {
        let desk = Desk::new(Arc::new(Fake::default()));
        let login = test_login("t", "Rithmic Test", TEST_SYSTEM);
        let moved = Arc::new(AtomicUsize::new(0));
        let counted = moved.clone();
        let session = Arc::new(FakeSession::new(
            vec![account("t", "951")],
            Box::new(move || {
                counted.fetch_add(1, Ordering::AcqRel);
            }),
        ));
        assert!(desk.begin("t"));
        desk.opened("t", Ok(session.clone()));
        let logins = vec![login];
        let empty = desk.book(&logins);
        assert!(empty.orders.is_empty() && empty.positions.is_empty() && empty.accounts.is_empty());
        assert!(empty.capabilities_of("t/951").is_some_and(|can| can.flatten));

        let buy = Order {
            account: "951".into(),
            contract: "ESZ6".into(),
            exchange: "CME".into(),
            side: Side::Buy,
            qty: 1,
            kind: OrderKind::Market,
            price: None,
            tag: new_tag(),
        };
        session.place(&buy).unwrap();
        let book = desk.book(&logins);
        let held = book.position("t/951", "ESZ6").expect("held");
        assert_eq!((held.net, held.avg), (1, Some(7_612.25)));
        assert_eq!(book.orders[0].status, Status::Filled);

        let limit = Order { side: Side::Sell, kind: OrderKind::Limit, price: Some(7_620.0), ..buy };
        let basket = session.place(&limit).unwrap();
        assert_eq!(desk.book(&logins).working("t/951", "ESZ6").count(), 1);
        session.cancel("951", &basket).unwrap();
        assert_eq!(desk.book(&logins).working("t/951", "ESZ6").count(), 0);
        session.flatten("951", "ESZ6", "CME").unwrap();
        assert!(desk.book(&logins).position("t/951", "ESZ6").is_none());
        assert_eq!(moved.load(Ordering::Acquire), 4);

        session.sign_out();
        assert!(session.place(&limit).is_err(), "a session that is gone refuses, and nothing retries");
    }
}
