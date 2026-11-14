//! Sierra Chart's DTC server, as a [`TradingService`] and its [`Session`].
//!
//! Sierra Chart is signed in to its own trading service or simulator; its DTC server,
//! with "Allow Trading" on, relays orders to whichever it is. So a login is
//! where the server is — on this machine, port 11099 — and its accounts are
//! whatever Sierra Chart trades: its simulated `Sim1`, `Sim2`, and the
//! trading service's own. Behind the `sierra` flag: closed while it is off.
//!
//! The server sends no CME market data, so a session prices nothing: its
//! positions are marked to the charts of their contracts, as the desk does
//! for any session that cannot.
//!
//! Contracts are the exchange's spelling above this, `ESZ6`, and the
//! server's on the wire; this is the one place that turns one into the
//! other. The server's depends on the service Sierra Chart trades through:
//! its own spells December's E-mini `ESZ26_FUT_CME`, Rithmic `ESZ6.CME`. So
//! a contract goes out as the server spells it in a position or a working
//! order; failing that, under the first name it confirms of those it may
//! know it by, all asked at once: the file Sierra Chart is writing for it,
//! the spelling of its fills, and the likeliest spellings
//! ([`SierraSession::spelling`]). What it confirmed holds for one
//! connection: a drop forgets it, and until the book is asked for again
//! nothing in it is taken for the server's spelling.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use omacharts_engine::providers::sierra::{self as feed, setup};
use omacharts_engine::specs::{self, root_of};

use crate::trading::*;

/// How the app signs in, as Sierra Chart's DTC Protocol Clients list shows.
const CLIENT_NAME: &str = "Omacharts";

/// Where `login`'s server is.
fn endpoint_of(login: &Login) -> dtc::Endpoint {
    dtc::Endpoint {
        host: login.value(setup::HOST).filter(|host| !host.trim().is_empty()).unwrap_or(setup::DEFAULT_HOST).to_string(),
        port: login.value(setup::PORT).and_then(|port| port.trim().parse().ok()).unwrap_or(setup::DEFAULT_PORT),
        ..dtc::Endpoint::default()
    }
}

/// Sierra Chart's DTC server.
pub struct Sierra;

impl TradingService for Sierra {
    fn id(&self) -> &'static str {
        feed::LISTED.id
    }

    fn label(&self) -> &'static str {
        feed::LISTED.label
    }

    fn has_feed(&self) -> bool {
        true
    }

    /// Where its server is, when it is not this machine's default.
    fn signs_in_to(&self, login: &Login) -> Option<String> {
        let endpoint = endpoint_of(login);
        Some(format!("{}:{}", endpoint.host, endpoint.port))
    }

    /// No password: the server is on this machine, where Sierra Chart's
    /// "Local Computer Only" lets nothing else reach it.
    fn keep_password(&self, _: &dyn Secrets, _: &Login, _: &str) -> Result<(), String> {
        Ok(())
    }

    fn forget_password(&self, _: &dyn Secrets, _: &Login, _: bool) -> Result<(), String> {
        Ok(())
    }

    fn sign_in(&self, login: &Login, changed: Box<dyn Fn(Change) + Send + Sync>) -> Result<Arc<dyn Session>, String> {
        let changed: Arc<dyn Fn(Change) + Send + Sync> = Arc::from(changed);
        let told = changed.clone();
        let trading = dtc::Trading::sign_in(&endpoint_of(login), CLIENT_NAME, move |pulse| match pulse {
            dtc::Pulse::Back => told(Change::Back),
            dtc::Pulse::Moved | dtc::Pulse::Lost(_) => told(Change::Moved),
        })
        .map_err(said)?;
        Ok(Arc::new(SierraSession::new(login.id.clone(), trading)))
    }

    fn open(&self) -> bool {
        crate::features::on(crate::features::Feature::Sierra)
    }

    /// Nothing is typed to sign in, so a saved login always can.
    fn signs_in_saved(&self, _: &Login) -> bool {
        true
    }
}

/// The server's words for what went wrong.
fn said(error: dtc::Error) -> String {
    match error {
        dtc::Error::Refused(why) | dtc::Error::Rejected(why) => why,
        error => error.to_string(),
    }
}

/// A request's failure: a drop or a timeout is no answer, not a no.
fn failed(error: dtc::Error) -> Failure {
    match error {
        dtc::Error::Unreachable(_) | dtc::Error::Timeout | dtc::Error::Closed => Failure::Unanswered(said(error)),
        error => Failure::Refused(said(error)),
    }
}

fn side_of(side: dtc::Side) -> Side {
    match side {
        dtc::Side::Buy => Side::Buy,
        dtc::Side::Sell => Side::Sell,
    }
}

/// The exchange `contract` trades on, as the app names it: what the server
/// said, else what its symbol for it, `symbol`, names (`NYMEX` of
/// `CLZ6.NYMEX`), else the exchange the root is listed on.
fn exchange_of(contract: &str, said: Option<&str>, symbol: &str) -> String {
    match said.map(str::trim).filter(|exchange| !exchange.is_empty()) {
        Some(exchange) => exchange.to_string(),
        None => feed::symbol_exchange(symbol).unwrap_or_else(|| {
            omacharts_engine::exchanges::registry().contract_exchange(root_of(contract)).unwrap_or("CME").to_string()
        }),
    }
}

/// An order as the server told of it, in the neutral words.
fn order_of(order: &dtc::Order) -> Order {
    let contract = feed::exchange_contract(&order.symbol);
    let (stage, said) = match &order.status {
        dtc::Status::Pending => (Stage::Pending, order.said.clone()),
        dtc::Status::Working | dtc::Status::Changing => (Stage::Working, order.said.clone()),
        dtc::Status::PartiallyFilled => (Stage::PartiallyFilled, order.said.clone()),
        dtc::Status::Filled => (Stage::Filled, order.said.clone()),
        dtc::Status::Cancelled => (Stage::Cancelled, order.said.clone()),
        dtc::Status::Rejected(why) => (Stage::Rejected, Some(why.clone())),
    };
    Order {
        tick: specs::tick(root_of(&contract)),
        account: order.account.clone(),
        // The id it was placed under here, which is what placing it
        // returned and what a link names it by; the server's for one placed
        // elsewhere. Either reaches it to cancel or move.
        basket_id: order.client_id.clone().or_else(|| order.server_id.clone()).unwrap_or_default(),
        exchange: exchange_of(&contract, order.exchange.as_deref(), &order.symbol),
        contract,
        side: side_of(order.side),
        qty: order.qty,
        filled: order.filled,
        kind: match order.kind {
            dtc::OrderKind::Market => OrderKind::Market,
            dtc::OrderKind::Limit => OrderKind::Limit,
            dtc::OrderKind::Stop | dtc::OrderKind::StopLimit => OrderKind::Stop,
        },
        price: order.price1,
        status: Status { stage, said, bound: None },
        fill_price: order.avg_fill_price,
        fills: Vec::new(),
        at: order.at,
        tag: order.tag.clone(),
    }
}

/// A fill from the server's history, as an order filled at once.
fn filled_order(fill: &dtc::Fill) -> Order {
    let contract = feed::exchange_contract(&fill.symbol);
    let basket = fill.server_id.clone().or_else(|| fill.execution_id.clone()).unwrap_or_default();
    Order {
        tick: specs::tick(root_of(&contract)),
        account: fill.account.clone(),
        basket_id: basket.clone(),
        exchange: exchange_of(&contract, fill.exchange.as_deref(), &fill.symbol),
        contract,
        side: side_of(fill.side),
        qty: fill.qty,
        filled: fill.qty,
        kind: OrderKind::Market,
        price: None,
        status: Stage::Filled.into(),
        fill_price: Some(fill.price),
        fills: vec![Fill { basket, at: fill.at, qty: fill.qty, price: fill.price }],
        at: fill.at,
        tag: None,
    }
}

/// How long a contract the server knew by no name is remembered as such,
/// so that clicking again says so at once rather than asking again.
const UNSPELLED_KEPT: Duration = Duration::from_secs(30);

/// How the server spells a contract, as [`SierraSession::spelling`] found.
#[derive(Clone, Debug, PartialEq)]
enum Spelling {
    /// As the server spells it: in a position or a working order of its,
    /// or confirmed by it knowing a contract by that name.
    Confirmed(String),
    /// As the file Sierra Chart writes the contract's trades to is named,
    /// the server silent on every name asked: for a market order alone.
    Unconfirmed(String),
}

/// What the session has learned of the server's spellings, for as long as
/// the connection and the style the server spells in stay the same.
#[derive(Default)]
struct Spelled {
    /// The names it confirmed, by the exchange's spelling.
    known: HashMap<String, String>,
    /// The contracts it knew by no name lately: when, why, and the file's
    /// name when it was silent on that too.
    failed: HashMap<String, (Instant, String, Option<String>)>,
    /// The connection they were learned on, as [`dtc::Trading::connection`]
    /// counts: one more the moment the connection drops.
    connection: u64,
    /// The style the server's positions and working orders were in.
    style: Option<feed::Style>,
}

/// What the session reads of the Sierra Chart folder: a contract's file's
/// name, and the style of the file written last. The folder's own, but for
/// a test's.
#[derive(Clone, Copy)]
struct Folder {
    stem: fn(&str) -> Option<String>,
    style: fn() -> Option<feed::Style>,
}

impl Default for Folder {
    fn default() -> Folder {
        Folder { stem: feed::file_stem, style: feed::folder_style }
    }
}

struct SierraSession {
    login: String,
    trading: dtc::Trading,
    folder: Folder,
    /// How the server spells contracts it has not been heard to name.
    spelled: Mutex<Spelled>,
    /// The server's symbols that name no dated contract, said once each.
    unread: Mutex<HashSet<String>>,
}

impl SierraSession {
    fn new(login: String, trading: dtc::Trading) -> SierraSession {
        SierraSession { login, trading, folder: Folder::default(), spelled: Mutex::default(), unread: Mutex::default() }
    }

    /// The symbols the server names what is held and working by now:
    /// its own spelling of each, as it trades now. None while the book is
    /// being asked for again after a drop, since what it holds until then
    /// is the last connection's, maybe in another service's spelling.
    fn current(book: &dtc::Book) -> Vec<String> {
        if !book.synced {
            return Vec::new();
        }
        let working = book.orders.iter().filter(|order| order.status.working() && order.status != dtc::Status::Pending);
        book.positions.iter().map(|held| held.symbol.clone()).chain(working.map(|order| order.symbol.clone())).collect()
    }

    /// The symbols the server named what was filled or finished in, latest
    /// first: its spelling then, which may not be its spelling now. None
    /// while the book is being asked for again, as [`Self::current`].
    fn past(book: &dtc::Book) -> Vec<String> {
        if !book.synced {
            return Vec::new();
        }
        let mut orders: Vec<&dtc::Order> = book.orders.iter().filter(|order| !order.status.working()).collect();
        orders.sort_by_key(|order| std::cmp::Reverse(order.at));
        let mut fills: Vec<&dtc::Fill> = book.fills.iter().collect();
        fills.sort_by_key(|fill| std::cmp::Reverse(fill.at));
        orders.into_iter().map(|order| order.symbol.clone()).chain(fills.into_iter().map(|fill| fill.symbol.clone())).collect()
    }

    /// How the server spells the contract `contract`, `ESZ6`: as it spells
    /// it in a position or a working order of its own. Failing that, the
    /// first it confirms of the names it may know it by, all asked at
    /// once: the stem of the file Sierra Chart writes it to, how it spelled
    /// it in what was filled, and the ways it may spell it, in the style it
    /// spells what is held and working in now, else the style of the file
    /// Sierra Chart wrote last. Never a name it has not confirmed, since an
    /// order under a name it does not know goes nowhere and a flatten finds
    /// nothing to close, but for one case: a `market` order, the server
    /// silent on every name, the file's name too, and that name in the
    /// style the server spells what it holds and has working in, goes out
    /// under the file's name, since that is Sierra Chart's own name for the
    /// contract on the service it is on. With nothing held or working, or
    /// the book not yet asked for again after a drop, the server's style is
    /// not known, and the folder's is only the folder's: refused.
    ///
    /// What it confirmed is kept, and what it confirmed nothing of is, for
    /// [`UNSPELLED_KEPT`], so a click again fails at once; both forgotten
    /// the moment the connection drops or the server spells in another
    /// style, and nothing learned on a connection that dropped while it was
    /// asked is kept.
    fn spelling(&self, contract: &str, market: bool) -> Result<Spelling, Failure> {
        let connection = self.trading.connection();
        let book = self.trading.book();
        let current = Self::current(&book);
        if let Some(own) = current.iter().find(|symbol| feed::exchange_contract(symbol) == contract) {
            return Ok(Spelling::Confirmed(own.clone()));
        }
        // The style the server spells in, as what it holds and has working
        // says; to ask in, failing that, the folder's.
        let held = current.iter().find_map(|symbol| feed::symbol_style(symbol));
        let style = held.or_else(self.folder.style);
        // The file's name goes out unconfirmed only in the server's style.
        let unconfirmed = |stem: &String| market && held.is_some_and(|held| feed::symbol_style(stem) == Some(held));
        let past: Vec<String> = Self::past(&book).into_iter().filter(|symbol| feed::exchange_contract(symbol) == contract).collect();
        {
            let mut spelled = self.spelled.lock().unwrap_or_else(|e| e.into_inner());
            let restyled = held.is_some() && spelled.style.is_some() && held != spelled.style;
            if connection != spelled.connection || restyled {
                *spelled = Spelled { connection, ..Spelled::default() };
            }
            if held.is_some() {
                spelled.style = held;
            }
            if let Some(kept) = spelled.known.get(contract) {
                return Ok(Spelling::Confirmed(kept.clone()));
            }
            if let Some((at, why, stem)) = spelled.failed.get(contract).filter(|(at, ..)| at.elapsed() < UNSPELLED_KEPT) {
                return match stem.clone().filter(unconfirmed) {
                    Some(stem) => Ok(Spelling::Unconfirmed(stem)),
                    None => Err(Failure::Refused(format!("{why}, {}s ago", at.elapsed().as_secs()))),
                };
            }
        }
        // Kept only while the connection it was learned on is the one now.
        let remember = |learned: &dyn Fn(&mut Spelled)| {
            let mut spelled = self.spelled.lock().unwrap_or_else(|e| e.into_inner());
            if spelled.connection == connection && self.trading.connection() == connection {
                learned(&mut spelled);
            }
        };
        let stem = (self.folder.stem)(contract);
        let mut guesses: Vec<String> = Vec::new();
        let spellings = feed::spellings(contract, style.or_else(|| past.iter().find_map(|symbol| feed::symbol_style(symbol))).unwrap_or(feed::Style::Underscored));
        for guess in stem.iter().chain(&past).chain(&spellings) {
            if !guesses.contains(guess) {
                guesses.push(guess.clone());
            }
        }
        // The style it trades in first, each style's in the order above.
        if let Some(style) = style {
            guesses.sort_by_key(|guess| feed::symbol_style(guess) != Some(style));
        }
        if guesses.is_empty() {
            return Err(Failure::Refused(format!("{contract} is not a futures contract Sierra Chart can be asked for")));
        }
        let answers = self.trading.knows_any(&guesses).map_err(failed)?;
        if let Some(confirmed) = guesses.iter().zip(&answers).find_map(|(guess, known)| matches!(known, dtc::Known::Defined(_)).then_some(guess)) {
            remember(&|spelled| {
                spelled.known.insert(contract.to_string(), confirmed.clone());
            });
            crate::log_words::say(crate::log_words::spelled_as(feed::LISTED.label, contract, confirmed));
            return Ok(Spelling::Confirmed(confirmed.clone()));
        }
        let tried: Vec<String> = guesses
            .iter()
            .zip(&answers)
            .map(|(guess, known)| match known {
                dtc::Known::Unknown(why) => format!("{guess}: {why}"),
                _ => format!("{guess}: no answer"),
            })
            .collect();
        crate::log_words::say(crate::log_words::unspelled(feed::LISTED.label, contract, &tried));
        let unanswered = answers.iter().all(|known| *known == dtc::Known::Unanswered);
        // Silent on the file's name, as on every other: what Sierra Chart
        // calls the contract, when the server spells in the file's style.
        let silent = stem.filter(|stem| unanswered && guesses.contains(stem));
        let why = match unanswered {
            true => format!("Sierra Chart didn't confirm {contract}: no answer for {}", guesses.join(", ")),
            false => format!("Sierra Chart knows {contract} by none of {}", guesses.join(", ")),
        };
        remember(&|spelled| {
            spelled.failed.insert(contract.to_string(), (Instant::now(), why.clone(), silent.clone()));
        });
        match silent.filter(unconfirmed) {
            Some(stem) => Ok(Spelling::Unconfirmed(stem)),
            None => Err(Failure::Refused(why)),
        }
    }

    /// The name to send `contract` under, said in the Log when it is not
    /// confirmed.
    fn symbol(&self, contract: &str, market: bool) -> Result<String, Failure> {
        match self.spelling(contract, market)? {
            Spelling::Confirmed(symbol) => Ok(symbol),
            Spelling::Unconfirmed(symbol) => {
                crate::log_words::say(crate::log_words::sent_unconfirmed(feed::LISTED.label, contract, &symbol));
                Ok(symbol)
            }
        }
    }

    /// Say, once, a symbol of the server's that names no dated contract:
    /// what is held or working in it is on no chart.
    fn unreadable(&self, symbol: &str) {
        if !symbol.is_empty() && feed::symbol_style(symbol).is_none() && self.unread.lock().unwrap_or_else(|e| e.into_inner()).insert(symbol.to_string()) {
            crate::log_words::say(crate::log_words::unread_symbol(feed::LISTED.label, symbol));
        }
    }

    /// An order as the server takes it, in its own spelling.
    fn new_order(&self, order: &OrderRequest) -> Result<dtc::NewOrder, Failure> {
        let symbol = self.symbol(&order.contract, order.kind == OrderKind::Market)?;
        Ok(new_order(order, symbol))
    }
}

impl Session for SierraSession {
    fn accounts(&self) -> Vec<Account> {
        self.trading
            .accounts()
            .into_iter()
            .map(|account| {
                let balance = self.trading.balance(&account.id);
                Account {
                    login: self.login.clone(),
                    currency: balance.as_ref().and_then(|balance| balance.currency.clone()),
                    balance: balance.map(|balance| balance.cash),
                    name: account.simulated.then(|| format!("{} (simulated)", account.id)),
                    id: account.id,
                }
            })
            .collect()
    }

    /// No linking after the fact: DTC makes two orders one-cancels-other
    /// only as they are submitted together, as one OCO pair, when the
    /// server says it takes them (`OCOOrdersSupported`, as this machine's
    /// Sierra Chart does); with a server that does not, the app holds the
    /// link.
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            kinds: vec![OrderKind::Market, OrderKind::Limit, OrderKind::Stop],
            flatten: true,
            flatten_cancels: true,
            cancel_all: true,
            modify: true,
            link: match self.trading.server().oco {
                true => Linking::AtSubmission,
                false => Linking::None,
            },
            history: true,
            snapshot: true,
        }
    }

    /// Simulated only when every account Sierra Chart trades is.
    fn is_simulated(&self) -> bool {
        let accounts = self.trading.accounts();
        !accounts.is_empty() && accounts.iter().all(|account| account.simulated)
    }

    /// A futures root as the contract its Sierra Chart charts are drawn
    /// from, a dated contract as itself.
    fn contract_for(&self, instrument: &omacharts_engine::Instrument) -> Result<Contract, Refusal> {
        if instrument.kind != omacharts_engine::InstrumentKind::FutureRoot {
            return Err(Refusal::NotTradable);
        }
        let symbol = feed::traded_contract(&instrument.symbol).ok_or(Refusal::NotKnownYet)?;
        let stem = feed::file_stem(&symbol).unwrap_or_default();
        Ok(Contract { tick: specs::tick(root_of(&symbol)), exchange: exchange_of(&symbol, None, &stem), symbol })
    }

    fn book(&self) -> Book {
        let book = self.trading.book();
        for symbol in book.positions.iter().map(|held| &held.symbol).chain(book.orders.iter().map(|order| &order.symbol)) {
            self.unreadable(symbol);
        }
        let mut accounts: HashMap<String, Money> = HashMap::new();
        for balance in &book.balances {
            accounts.insert(
                balance.account.clone(),
                Money {
                    key: balance.account.clone(),
                    balance: Some(balance.cash),
                    unrealized: balance.open_pnl,
                    day_pnl: balance.day_pnl,
                    limits: balance.daily_loss_limit.map(|limit| Limits { loss_limit: Some(limit), ..Limits::default() }),
                },
            );
        }
        Book {
            capabilities: HashMap::new(),
            refreshed: book.synced,
            accounts: book
                .accounts
                .iter()
                .filter_map(|account| accounts.remove(&account.id))
                .collect(),
            positions: book
                .positions
                .iter()
                .map(|held| {
                    let contract = feed::exchange_contract(&held.symbol);
                    Position {
                        tick: specs::tick(root_of(&contract)),
                        point: specs::point(root_of(&contract)),
                        account: held.account.clone(),
                        exchange: exchange_of(&contract, held.exchange.as_deref(), &held.symbol),
                        contract,
                        net: held.qty,
                        avg: held.avg_price,
                        open_pnl: held.open_pnl,
                    }
                })
                .collect(),
            orders: book.orders.iter().map(order_of).collect(),
        }
    }

    fn refresh(&self) -> Result<(), String> {
        self.trading.refresh().map_err(said)
    }

    fn history(&self, account: &str, from: i64, to: i64) -> Result<Vec<Order>, Failure> {
        Ok(self.trading.history(account, from, to).map_err(failed)?.iter().map(filled_order).collect())
    }

    fn place(&self, order: &OrderRequest) -> Result<String, Failure> {
        self.trading.place(&self.new_order(order)?).map_err(failed)
    }

    /// One SUBMIT_NEW_OCO_ORDER, with no parent: see
    /// [`dtc::Trading::place_pair`].
    fn place_pair(&self, orders: &[OrderRequest; 2]) -> Result<[String; 2], Failure> {
        self.trading.place_pair(&[self.new_order(&orders[0])?, self.new_order(&orders[1])?]).map_err(failed)
    }

    fn cancel(&self, _account: &str, basket: &str) -> Result<(), Failure> {
        self.trading.cancel(basket).map_err(failed)
    }

    fn modify(&self, _account: &str, basket: &str, price: f64) -> Result<(), Failure> {
        self.trading.modify(basket, Some(price), None).map_err(failed)
    }

    /// Never asked: [`Plan`] links Sierra Chart's orders as a pair.
    fn link(&self, _account: &str, _baskets: &[String]) -> Result<(), Failure> {
        Err(Failure::Refused("Sierra Chart links orders only as they are placed together".into()))
    }

    fn cancel_all(&self, account: &str) -> Result<(), Failure> {
        self.trading.cancel_all(account).map_err(failed)
    }

    /// Under a name the server confirmed alone. Nothing held or working
    /// under it is a fill that closed the position as the flatten went:
    /// already flat, which is what was asked.
    fn flatten(&self, account: &str, contract: &str, _exchange: &str) -> Result<usize, Failure> {
        let symbol = self.symbol(contract, false)?;
        match self.trading.flatten(account, &symbol) {
            Err(dtc::Error::Flat(_)) => {
                crate::log_words::say(crate::log_words::already_flat(feed::LISTED.label, account, contract));
                Ok(0)
            }
            outcome => outcome.map_err(failed),
        }
    }

    fn trouble(&self) -> Option<String> {
        self.trading.trouble().map(said)
    }

    fn sign_out(&self) {
        self.trading.sign_out();
    }
}

/// An order as the server takes it, for the contract it calls `symbol`.
fn new_order(order: &OrderRequest, symbol: String) -> dtc::NewOrder {
    dtc::NewOrder {
        account: order.account.clone(),
        symbol,
        exchange: None,
        side: match order.side {
            Side::Buy => dtc::Side::Buy,
            Side::Sell => dtc::Side::Sell,
        },
        qty: order.qty,
        kind: match order.kind {
            OrderKind::Market => dtc::OrderKind::Market,
            OrderKind::Limit => dtc::OrderKind::Limit,
            OrderKind::Stop => dtc::OrderKind::Stop,
        },
        price1: order.price,
        price2: None,
        time_in_force: dtc::TimeInForce::Day,
        tag: Some(order.tag.clone()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn login(port: u16) -> Login {
        Login {
            id: "sierra-1".into(),
            name: "Sierra Chart".into(),
            service: "sierra".into(),
            user: String::new(),
            values: vec![(setup::PORT.into(), port.to_string())],
            signed_out: false,
        }
    }

    /// A login's book comes from the server in the app's words: its
    /// accounts with their balances, a position in the exchange's spelling
    /// with its tick and point, and an order working.
    #[test]
    fn a_sessions_book_is_the_servers_in_the_apps_words() {
        let fake = dtc::fake::Fake::builder()
            .account("Sim1")
            .balance("Sim1", 50_000.0, "USD")
            .position("Sim1", "ESZ26_FUT_CME", 2, 7_600.25)
            .order("Sim1", "ESZ26_FUT_CME", dtc::Side::Buy, 1, dtc::OrderKind::Limit, 7_550.0)
            .start();
        let session = Sierra.sign_in(&login(fake.port()), Box::new(|_| {})).expect("signed in");
        session.refresh().expect("refreshed");
        let accounts = session.accounts();
        assert_eq!(accounts.len(), 1);
        assert_eq!((accounts[0].id.as_str(), accounts[0].balance), ("Sim1", Some(50_000.0)));
        assert!(session.is_simulated());
        let book = session.book();
        let held = book.position("Sim1", "ESZ6").expect("the position, as the exchange spells it");
        assert_eq!((held.net, held.avg, held.tick, held.point), (2, Some(7_600.25), Some(0.25), Some(50.0)));
        let working: Vec<&Order> = book.working("Sim1", "ESZ6").collect();
        assert_eq!(working.len(), 1);
        assert_eq!((working[0].kind, working[0].price, working[0].side), (OrderKind::Limit, Some(7_550.0), Side::Buy));
        assert_eq!(session.capabilities().link, Linking::AtSubmission, "linked only as submitted together");
        session.sign_out();
    }

    /// A dated contract's chart trades that contract, as itself; what is
    /// not a future does not trade at all.
    #[test]
    fn a_dated_contract_trades_as_itself() {
        use crate::trading::fake::tests::instrument;
        use omacharts_engine::InstrumentKind;
        let fake = dtc::fake::Fake::builder().account("Sim1").start();
        let session = Sierra.sign_in(&login(fake.port()), Box::new(|_| {})).expect("signed in");
        let contract = session.contract_for(&instrument("MESZ6", InstrumentKind::FutureRoot)).expect("MESZ6 trades");
        assert_eq!((contract.symbol.as_str(), contract.exchange.as_str(), contract.tick), ("MESZ6", "CME", Some(0.25)));
        assert_eq!(session.contract_for(&instrument("AAPL", InstrumentKind::Equity)).err(), Some(Refusal::NotTradable));
        session.sign_out();
    }

    /// An order goes to the server as Sierra Chart names the contract, and
    /// a refusal comes back in its words.
    #[test]
    fn an_order_goes_out_in_sierra_charts_spelling_and_a_refusal_comes_back() {
        let fake = dtc::fake::Fake::builder().account("Sim1").reject_orders("Price is too far from the market").start();
        let session = Sierra.sign_in(&login(fake.port()), Box::new(|_| {})).expect("signed in");
        let order = OrderRequest {
            account: "Sim1".into(),
            contract: "ESZ6".into(),
            exchange: "CME".into(),
            side: Side::Sell,
            qty: 1,
            kind: OrderKind::Limit,
            price: Some(9_000.0),
            tag: new_tag(),
        };
        let refused = session.place(&order).expect_err("refused");
        assert_eq!(refused, Failure::Refused("Price is too far from the market".into()));
        let sent = fake.received_of(208);
        assert_eq!(sent.len(), 1, "one SUBMIT_NEW_SINGLE_ORDER");
        assert_eq!(sent[0].0.get("Symbol").and_then(|symbol| symbol.as_str()), Some("ESZ26_FUT_CME"));
        session.sign_out();
    }

    /// A stop added to a target working on Sierra Chart: the target is
    /// cancelled first and the two go back as one OCO submission, each
    /// known by its own tag, the pair for the whole position. A server that
    /// takes no OCO leaves the link to the app.
    #[test]
    fn a_second_exit_reforms_the_pair_as_one_oco_submission() {
        let fake = dtc::fake::Fake::builder().account("Sim1").position("Sim1", "ESZ26_FUT_CME", 2, 7_600.0).start();
        let session = Sierra.sign_in(&login(fake.port()), Box::new(|_| {})).expect("signed in");
        session.refresh().expect("refreshed");
        let exit = |kind, price| OrderRequest {
            account: "Sim1".into(),
            contract: "ESZ6".into(),
            exchange: "CME".into(),
            side: Side::Sell,
            qty: 2,
            kind,
            price: Some(price),
            tag: new_tag(),
        };
        let target = session.place(&exit(OrderKind::Limit, 7_650.0)).expect("the target");
        let working = session.book().order(&target).cloned();
        let plan = Plan::exit(session.capabilities().link, exit(OrderKind::Stop, 7_580.0), working.as_ref());
        let ran = plan.run(session.as_ref(), "Sim1");
        let link = ran.link.expect("linked").expect("linked");
        assert_eq!(link.holder, Holder::TradingService);
        assert_eq!(fake.received_of(dtc::wire::CANCEL_ORDER).len(), 1, "the target cancelled first");
        let pairs = fake.received_of(dtc::wire::SUBMIT_NEW_OCO_ORDER);
        assert_eq!(pairs.len(), 1, "one OCO submission");
        assert_eq!((pairs[0].int("Quantity_1"), pairs[0].int("Quantity_2")), (Some(2), Some(2)));
        assert_eq!((pairs[0].int("OrderType_1"), pairs[0].int("OrderType_2")), (Some(3), Some(2)));
        let book = session.book();
        let tags: Vec<Option<String>> = link.baskets.iter().map(|basket| book.order(basket).and_then(|o| o.tag.clone())).collect();
        let asked: Vec<Option<String>> = plan.placing().iter().map(|order| Some(order.tag.clone())).collect();
        assert_eq!(tags, asked, "each of the pair known by its own tag");
        session.sign_out();

        let fake = dtc::fake::Fake::builder().account("Sim1").oco_unsupported().start();
        let session = Sierra.sign_in(&login(fake.port()), Box::new(|_| {})).expect("signed in");
        assert_eq!(session.capabilities().link, Linking::None, "the app holds the link");
        session.sign_out();
    }

    /// Long `qty` ES at 7,600 on Sim1 with a stop working at 7,580 for all
    /// of it, and a target at 7,650 for all of it asked for: the session,
    /// the stop as the book has it, and the plan for the target.
    fn stop_then_target(fake: &dtc::fake::Fake, qty: u32) -> (Arc<dyn Session>, Order, Plan) {
        let session = Sierra.sign_in(&login(fake.port()), Box::new(|_| {})).expect("signed in");
        session.refresh().expect("refreshed");
        let exit = |kind, price| OrderRequest {
            account: "Sim1".into(),
            contract: "ESZ6".into(),
            exchange: "CME".into(),
            side: Side::Sell,
            qty,
            kind,
            price: Some(price),
            tag: new_tag(),
        };
        let basket = session.place(&exit(OrderKind::Stop, 7_580.0)).expect("the stop");
        let stop = session.book().order(&basket).cloned().expect("the stop in the book");
        let plan = Plan::exit(session.capabilities().link, exit(OrderKind::Limit, 7_650.0), Some(&stop));
        (session, stop, plan)
    }

    /// Wait for the session's book to say `done`, as the server's updates
    /// reach it.
    fn until(session: &dyn Session, done: impl Fn(&Book) -> bool) -> bool {
        for _ in 0..400 {
            if done(&session.book()) {
                return true;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        false
    }

    fn fake_long(qty: i64) -> dtc::fake::Fake {
        dtc::fake::Fake::builder().account("Sim1").position("Sim1", "ESZ26_FUT_CME", qty, 7_600.0).start()
    }

    /// A target added to a stop working on Sierra Chart: the stop is
    /// cancelled, and only once the server says so do the stop, at its
    /// price and quantity, and the target go as one OCO pair the server
    /// holds, the stop first.
    #[test]
    fn a_target_added_to_a_working_stop_reforms_the_pair_after_the_stop_is_cancelled() {
        let fake = fake_long(2);
        let (session, stop, plan) = stop_then_target(&fake, 2);
        assert!(matches!(plan, Plan::Reform { .. }), "{plan:?}");
        assert_eq!((plan.cancelling(), plan.holder()), (Some(stop.basket_id.as_str()), Some(Holder::TradingService)));
        let ran = plan.run(session.as_ref(), "Sim1");
        assert_eq!(ran.cancelled, Some(Ok(())));
        let link = ran.link.clone().expect("linked").expect("linked");
        assert_eq!(link.holder, Holder::TradingService);
        let kinds: Vec<u16> = fake
            .received()
            .iter()
            .map(|message| message.kind())
            .filter(|kind| [dtc::wire::CANCEL_ORDER, dtc::wire::SUBMIT_NEW_OCO_ORDER].contains(kind))
            .collect();
        assert_eq!(kinds, [dtc::wire::CANCEL_ORDER, dtc::wire::SUBMIT_NEW_OCO_ORDER], "the cancel answered, then the pair");
        let pair = &fake.received_of(dtc::wire::SUBMIT_NEW_OCO_ORDER)[0];
        assert_eq!((pair.int("OrderType_1"), pair.int("OrderType_2")), (Some(3), Some(2)), "the stop first");
        assert_eq!((pair.int("Quantity_1"), pair.int("Quantity_2")), (Some(2), Some(2)));
        assert!(until(session.as_ref(), |book| book.working("Sim1", "ESZ6").count() == 2));
        let book = session.book();
        assert!(!book.order(&stop.basket_id).is_some_and(|old| old.status.working()), "two stops working");
        let stops: Vec<&Order> = book.working("Sim1", "ESZ6").filter(|order| order.kind == OrderKind::Stop).collect();
        assert_eq!(stops.len(), 1, "one stop, never two");
        assert_eq!((stops[0].price, stops[0].qty, stops[0].side), (Some(7_580.0), 2, Side::Sell), "the stop as it was");
        assert!(link.baskets.contains(&stops[0].basket_id));
        assert_eq!((ran.restored.clone(), ran.resized), (None, None));
        session.sign_out();
    }

    /// The stop filled before its cancel was answered: the position is
    /// closed, the cancel refused, and nothing is placed, which is said.
    #[test]
    fn a_stop_filled_while_it_is_being_replaced_places_nothing() {
        let fake = fake_long(1);
        let (session, stop, plan) = stop_then_target(&fake, 1);
        fake.fill(&stop.basket_id, 1, 7_580.0);
        assert!(until(session.as_ref(), |book| book.order(&stop.basket_id).is_some_and(|order| !order.status.working())));
        let ran = plan.run(session.as_ref(), "Sim1");
        assert!(matches!(ran.cancelled, Some(Err(Failure::Refused(_)))), "{:?}", ran.cancelled);
        let said = Failure::Refused("nothing left to close: the stop filled meanwhile".into());
        assert!(ran.placed.iter().all(|(_, placed)| *placed == Err(said.clone())), "{:?}", ran.placed);
        assert!(fake.received_of(dtc::wire::SUBMIT_NEW_OCO_ORDER).is_empty(), "a pair for a closed position");
        assert_eq!(fake.received_of(208).len(), 1, "only the stop was ever placed alone");
        assert!(ran.restored.is_none());
        session.sign_out();
    }

    /// The stop filled in part before it was cancelled: the pair is for
    /// what is still held, and says so.
    #[test]
    fn a_stop_filled_in_part_while_it_is_being_replaced_sizes_the_pair_down() {
        let fake = fake_long(3);
        let (session, stop, plan) = stop_then_target(&fake, 3);
        fake.fill(&stop.basket_id, 1, 7_580.0);
        assert!(until(session.as_ref(), |book| {
            book.order(&stop.basket_id).is_some_and(|order| order.filled == 1)
                && book.position("Sim1", "ESZ6").is_some_and(|held| held.net == 2)
        }));
        let ran = plan.run(session.as_ref(), "Sim1");
        assert_eq!(ran.cancelled, Some(Ok(())));
        assert!(ran.placed.iter().all(|(order, placed)| placed.is_ok() && order.qty == 2), "{:?}", ran.placed);
        assert_eq!(ran.resized, Some((3, 2)));
        let pair = &fake.received_of(dtc::wire::SUBMIT_NEW_OCO_ORDER)[0];
        assert_eq!((pair.int("Quantity_1"), pair.int("Quantity_2")), (Some(2), Some(2)));
        assert!(ran.link.is_some_and(|link| link.is_ok()));
        session.sign_out();
    }

    /// The pair refused after the stop was cancelled: the stop goes back in
    /// alone at once, at its price and quantity, and the refusal is said.
    #[test]
    fn a_refused_pair_puts_the_stop_back_at_once() {
        let fake = fake_long(2);
        let (session, stop, plan) = stop_then_target(&fake, 2);
        fake.set_reject_pairs(Some("Rejected at RMS"));
        let ran = plan.run(session.as_ref(), "Sim1");
        assert_eq!(ran.cancelled, Some(Ok(())));
        assert!(ran.placed.iter().all(|(_, placed)| *placed == Err(Failure::Refused("Rejected at RMS".into()))), "{:?}", ran.placed);
        assert_eq!(ran.link, None);
        let (again, placed) = ran.restored.expect("the stop placed again");
        let basket = placed.expect("the stop taken back");
        assert_eq!((again.kind, again.price, again.qty), (OrderKind::Stop, Some(7_580.0), 2));
        assert!(until(session.as_ref(), |book| book.order(&basket).is_some_and(|order| order.status.working())));
        let book = session.book();
        let working: Vec<&Order> = book.working("Sim1", "ESZ6").collect();
        assert_eq!(working.len(), 1, "{working:?}");
        assert_eq!((working[0].kind, working[0].price, working[0].qty), (OrderKind::Stop, Some(7_580.0), 2));
        assert!(!book.order(&stop.basket_id).is_some_and(|old| old.status.working()));
        session.sign_out();
    }

    /// A session on `fake` that waits only a moment for an answer, so that
    /// a server that never answers is found out quickly.
    fn impatient(fake: &dtc::fake::Fake) -> Arc<dyn Session> {
        impatient_on(fake, NO_FOLDER)
    }

    /// No Sierra Chart folder, for a test that must not see another's.
    const NO_FOLDER: Folder = Folder { stem: |_| None, style: || None };

    /// [`impatient`], reading `folder` for files' names.
    fn impatient_on(fake: &dtc::fake::Fake, folder: Folder) -> Arc<SierraSession> {
        let timing = dtc::Timing { request: std::time::Duration::from_millis(700), ..dtc::Timing::default() };
        let trading = dtc::Trading::sign_in_with(&fake.endpoint(), CLIENT_NAME, timing, |_| {}).expect("signed in");
        let session = Arc::new(SierraSession { folder, ..SierraSession::new("sierra-1".into(), trading) });
        session.refresh().expect("refreshed");
        session
    }

    /// A sell exit of ES on Sim1, for `qty`, at `price`.
    fn sell(kind: OrderKind, qty: u32, price: f64) -> OrderRequest {
        OrderRequest {
            account: "Sim1".into(),
            contract: "ESZ6".into(),
            exchange: "CME".into(),
            side: Side::Sell,
            qty,
            kind,
            price: Some(price),
            tag: new_tag(),
        }
    }

    /// Orders working on the server, as `(quantity, price)`.
    type Working = Vec<(u32, f64)>;

    /// What the server has working, stops and targets apart.
    fn on_the_server(fake: &dtc::fake::Fake) -> (Working, Working) {
        let working = fake.working();
        let of = |kind| working.iter().filter(|order| order.kind == kind).map(|order| (order.qty, order.price1)).collect();
        (of(dtc::OrderKind::Stop), of(dtc::OrderKind::Limit))
    }

    /// Long 2 with a stop for 1, from before the position was added to: a
    /// target pulled out for the whole of it goes with the stop again as
    /// one pair, both for the 2 held, and the stop's new size is said.
    #[test]
    fn a_target_beside_a_smaller_stop_sends_both_for_what_is_held() {
        let fake = fake_long(2);
        let session = impatient(&fake);
        let basket = session.place(&sell(OrderKind::Stop, 1, 7_580.0)).expect("the stop");
        let stop = session.book().order(&basket).cloned().expect("the stop in the book");
        let ran = Plan::exit(Linking::AtSubmission, sell(OrderKind::Limit, 2, 7_650.0), Some(&stop)).run(session.as_ref(), "Sim1");
        assert!(ran.link.clone().is_some_and(|link| link.is_ok()), "{ran:?}");
        let pair = &fake.received_of(dtc::wire::SUBMIT_NEW_OCO_ORDER)[0];
        assert_eq!((pair.int("Quantity_1"), pair.int("Quantity_2")), (Some(2), Some(2)), "half the position left without a target");
        assert_eq!(ran.again, Some((OrderKind::Stop, 1, 2)), "the stop's new size not said");
        assert_eq!(on_the_server(&fake), (vec![(2, 7_580.0)], vec![(2, 7_650.0)]));
        session.sign_out();
    }

    /// The stop is one of a pair already, with a target for 1 of the 2
    /// held. Cancelling it takes that target with it, as Sierra Chart does
    /// for an OCO pair, and the new pair covers the whole position: one
    /// stop and one target working, never two targets, and the target that
    /// went is said. The same whether or not the app knew of the pair.
    #[test]
    fn a_stop_already_paired_takes_its_target_along_and_the_new_pair_covers_everything() {
        for known in [true, false] {
            let fake = fake_long(2);
            let session = impatient(&fake);
            let [stop, old] = session
                .place_pair(&[sell(OrderKind::Stop, 1, 7_580.0), sell(OrderKind::Limit, 1, 7_640.0)])
                .expect("the first pair");
            let book = session.book();
            let stop = book.order(&stop).cloned().expect("the stop");
            let old = book.order(&old).cloned().expect("the target");
            let partners = if known { vec![old.clone()] } else { Vec::new() };
            let plan = Plan::exit(Linking::AtSubmission, sell(OrderKind::Limit, 2, 7_660.0), Some(&stop)).paired_with(partners);
            let ran = plan.run(session.as_ref(), "Sim1");
            assert!(ran.link.clone().is_some_and(|link| link.is_ok()), "known {known}: {ran:?}");
            assert!(
                until(session.as_ref(), |book| book.order(&old.basket_id).is_some_and(|order| !order.status.working())),
                "known {known}: the old target still working"
            );
            assert_eq!(on_the_server(&fake), (vec![(2, 7_580.0)], vec![(2, 7_660.0)]), "known {known}");
            if known {
                assert_eq!(ran.went_with.iter().map(|order| order.basket_id.clone()).collect::<Vec<_>>(), std::slice::from_ref(&old.basket_id));
            }
            session.sign_out();
        }
    }

    /// The pair is lost on its way, after the stop was cancelled: no
    /// answer, and nothing of it on the server. The stop goes back in
    /// alone, at its price, and is the one stop working.
    #[test]
    fn a_pair_never_answered_puts_the_stop_back_alone() {
        let fake = fake_long(2);
        let session = impatient(&fake);
        let basket = session.place(&sell(OrderKind::Stop, 2, 7_580.0)).expect("the stop");
        let stop = session.book().order(&basket).cloned().expect("the stop in the book");
        fake.set_pairs(dtc::fake::Pairs::Lost);
        let ran = Plan::exit(Linking::AtSubmission, sell(OrderKind::Limit, 2, 7_650.0), Some(&stop)).run(session.as_ref(), "Sim1");
        assert!(ran.placed.iter().all(|(_, placed)| matches!(placed, Err(Failure::Unanswered(_)))), "{:?}", ran.placed);
        let (again, placed) = ran.restored.clone().expect("the stop placed again");
        assert!(placed.is_ok(), "{placed:?}");
        assert_eq!((again.kind, again.price, again.qty), (OrderKind::Stop, Some(7_580.0), 2));
        assert_eq!(on_the_server(&fake), (vec![(2, 7_580.0)], vec![]), "{ran:?}");
        assert_eq!(ran.unsure, None);
        session.sign_out();
    }

    /// The pair reached the server and its answer was lost: both working
    /// there, unheard of here. What the book holds of it is cancelled
    /// before the stop goes back, so one stop is working, never two.
    #[test]
    fn a_pair_taken_unheard_is_cancelled_before_the_stop_goes_back() {
        let fake = fake_long(2);
        let session = impatient(&fake);
        let basket = session.place(&sell(OrderKind::Stop, 2, 7_580.0)).expect("the stop");
        let stop = session.book().order(&basket).cloned().expect("the stop in the book");
        fake.set_pairs(dtc::fake::Pairs::TakenSilently);
        let ran = Plan::exit(Linking::AtSubmission, sell(OrderKind::Limit, 2, 7_650.0), Some(&stop)).run(session.as_ref(), "Sim1");
        assert!(ran.restored.as_ref().is_some_and(|(_, placed)| placed.is_ok()), "{ran:?}");
        assert_eq!(on_the_server(&fake), (vec![(2, 7_580.0)], vec![]), "two stops, or none: {ran:?}");
        session.sign_out();
    }

    /// The pair reached the server and its answer was lost, and the server
    /// will not cancel it: it does not know the orders by the ids they were
    /// sent under, or it refuses. Either way the pair is still working, so
    /// the stop is not put back beside it — never two stops — and the
    /// position is said to need looking at.
    #[test]
    fn a_pair_taken_unheard_that_will_not_cancel_keeps_the_stop_out() {
        for cancels in [dtc::fake::Cancels::ClientIdUnknown, dtc::fake::Cancels::RefusedInPairs("Cancel refused".into())] {
            let fake = fake_long(2);
            let session = impatient(&fake);
            let basket = session.place(&sell(OrderKind::Stop, 2, 7_580.0)).expect("the stop");
            let stop = session.book().order(&basket).cloned().expect("the stop in the book");
            fake.set_pairs(dtc::fake::Pairs::TakenSilently);
            fake.set_cancels(cancels.clone());
            let ran = Plan::exit(Linking::AtSubmission, sell(OrderKind::Limit, 2, 7_650.0), Some(&stop)).run(session.as_ref(), "Sim1");
            assert_eq!(ran.restored, None, "{cancels:?}: put back beside the pair");
            assert!(ran.unsure.as_deref().is_some_and(|why| why.contains("still working")), "{cancels:?}: {ran:?}");
            assert_eq!(on_the_server(&fake), (vec![(2, 7_580.0)], vec![(2, 7_650.0)]), "{cancels:?}: one stop, the pair's");
            session.sign_out();
        }
    }

    /// The pair reached the server unheard, and its target filled at once,
    /// closing the position: the stop is not put back on nothing held.
    #[test]
    fn a_pair_taken_unheard_whose_target_filled_puts_nothing_back() {
        let fake = fake_long(2);
        let session = impatient(&fake);
        let basket = session.place(&sell(OrderKind::Stop, 2, 7_580.0)).expect("the stop");
        let stop = session.book().order(&basket).cloned().expect("the stop in the book");
        fake.set_pairs(dtc::fake::Pairs::FilledSilently(1));
        let ran = Plan::exit(Linking::AtSubmission, sell(OrderKind::Limit, 2, 7_650.0), Some(&stop)).run(session.as_ref(), "Sim1");
        assert_eq!(ran.restored, None, "a stop put back on a flat position: {ran:?}");
        assert!(ran.unsure.as_deref().is_some_and(|why| why.contains("target filled")), "{ran:?}");
        assert_eq!(on_the_server(&fake), (vec![], vec![]));
        assert!(session.book().position("Sim1", "ESZ6").is_none_or(|held| held.net == 0));
        session.sign_out();
    }

    /// Flattening where the server will not cancel the stop on the
    /// position: it is flat all the same, and the flatten answers the one
    /// order still working, for the window to say so.
    #[test]
    fn a_flatten_whose_cancel_is_refused_is_flat_and_counts_the_order_left() {
        let fake = fake_long(2);
        let session = impatient(&fake);
        session.place(&sell(OrderKind::Stop, 2, 7_580.0)).expect("the stop");
        fake.set_cancels(dtc::fake::Cancels::Refused("Cancel refused".into()));
        assert_eq!(session.flatten("Sim1", "ESZ6", "CME"), Ok(1));
        let flat = || session.book().position("Sim1", "ESZ6").is_none_or(|held| held.net == 0);
        let until = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while !flat() && std::time::Instant::now() < until {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(flat(), "not flat: {:?}", session.book().positions);
        assert_eq!(session.book().working("Sim1", "ESZ6").count(), 1, "the stop left working");
        session.sign_out();
    }

    /// Only the target of the pair is taken: the stop goes back in alone at
    /// once, and the app links it with the target, while it runs.
    #[test]
    fn a_pair_with_only_its_target_taken_puts_the_stop_back_and_links_the_two() {
        let fake = fake_long(2);
        let session = impatient(&fake);
        let basket = session.place(&sell(OrderKind::Stop, 2, 7_580.0)).expect("the stop");
        let stop = session.book().order(&basket).cloned().expect("the stop in the book");
        fake.set_pairs(dtc::fake::Pairs::OneRejected(0, "Stop price too close".into()));
        let ran = Plan::exit(Linking::AtSubmission, sell(OrderKind::Limit, 2, 7_650.0), Some(&stop)).run(session.as_ref(), "Sim1");
        let (again, placed) = ran.restored.clone().expect("the stop placed again");
        let back = placed.expect("taken back");
        assert_eq!((again.kind, again.price, again.qty), (OrderKind::Stop, Some(7_580.0), 2));
        let link = ran.link.clone().expect("linked").expect("linked");
        assert_eq!(link.holder, Holder::App);
        assert!(link.baskets.contains(&back), "{link:?}");
        assert_eq!(on_the_server(&fake), (vec![(2, 7_580.0)], vec![(2, 7_650.0)]));
        session.sign_out();
    }

    /// With Rithmic as Sierra Chart's trading service the server names
    /// contracts as Rithmic does: a position of ESZ6.CME is the app's ESZ6
    /// on CME, with ES's tick and point, and NQ, CL, GC and NG's two-digit
    /// year read the same way, each on the exchange its symbol names.
    #[test]
    fn on_rithmic_a_position_is_the_contract_its_symbol_names() {
        let fake = dtc::fake::Fake::builder()
            .account("Sim1")
            .rithmic_symbols()
            .position("Sim1", "ESZ6.CME", -2, 7_578.0)
            .position("Sim1", "NQZ6.CME", 1, 26_000.0)
            .position("Sim1", "CLZ6.NYMEX", 3, 61.5)
            .position("Sim1", "GCZ6.COMEX", 1, 4_100.0)
            .position("Sim1", "NGZ26.NYMEX", -1, 4.25)
            .start();
        let session = Sierra.sign_in(&login(fake.port()), Box::new(|_| {})).expect("signed in");
        session.refresh().expect("refreshed");
        let book = session.book();
        let es = book.position("Sim1", "ESZ6").expect("ESZ6.CME as ESZ6");
        assert_eq!((es.net, es.avg, es.exchange.as_str(), es.tick, es.point), (-2, Some(7_578.0), "CME", Some(0.25), Some(50.0)));
        let held = |contract: &str| book.position("Sim1", contract).map(|held| (held.net, held.exchange.clone(), held.tick));
        assert_eq!(held("NQZ6"), Some((1, "CME".into(), Some(0.25))));
        assert_eq!(held("CLZ6"), Some((3, "NYMEX".into(), Some(0.01))));
        assert_eq!(held("GCZ6"), Some((1, "COMEX".into(), Some(0.1))));
        assert_eq!(held("NGZ6"), Some((-1, "NYMEX".into(), Some(0.001))));
        session.sign_out();
    }

    fn sell_one(contract: &str, kind: OrderKind, price: Option<f64>) -> OrderRequest {
        OrderRequest {
            account: "Sim1".into(),
            contract: contract.into(),
            exchange: "CME".into(),
            side: Side::Sell,
            qty: 1,
            kind,
            price,
            tag: new_tag(),
        }
    }

    /// The symbols every order of `kind` reached the fake under.
    fn sent_as(fake: &dtc::fake::Fake, kind: u16) -> Vec<String> {
        fake.received_of(kind).iter().filter_map(|message| message.text("Symbol")).collect()
    }

    /// An order from a chart of ES, on a server on Rithmic that has named
    /// nothing yet, goes out as ESZ6.CME, the name the server confirms;
    /// NG's as its two-digit NGZ26.NYMEX. Neither is ever sent under a
    /// name the server does not know.
    #[test]
    fn on_rithmic_an_order_from_an_es_chart_goes_out_as_the_server_spells_it() {
        use crate::trading::fake::tests::instrument;
        let fake = dtc::fake::Fake::builder().account("Sim1").rithmic_symbols().start();
        let session = Sierra.sign_in(&login(fake.port()), Box::new(|_| {})).expect("signed in");
        session.refresh().expect("refreshed");
        let contract = session.contract_for(&instrument("ES", omacharts_engine::InstrumentKind::FutureRoot)).expect("ES trades");
        let front = omacharts_engine::contracts::front("ES", omacharts_engine::contracts::today()).expect("a front month");
        assert_eq!((contract.symbol.as_str(), contract.exchange.as_str()), (front.symbol.as_str(), "CME"));
        let basket = session.place(&sell_one(&contract.symbol, OrderKind::Limit, Some(9_000.0))).expect("taken");
        let wanted = format!("{}.CME", contract.symbol);
        assert_eq!(sent_as(&fake, dtc::wire::SUBMIT_NEW_SINGLE_ORDER), std::slice::from_ref(&wanted));
        assert_eq!(session.book().order(&basket).map(|order| order.contract.clone()), Some(contract.symbol.clone()));
        session.place(&sell_one("NGZ6", OrderKind::Limit, Some(9.0))).expect("NG taken");
        assert_eq!(sent_as(&fake, dtc::wire::SUBMIT_NEW_SINGLE_ORDER), [wanted, "NGZ26.NYMEX".to_string()]);
        let pair = session.place_pair(&[sell_one("CLZ6", OrderKind::Stop, Some(50.0)), sell_one("CLZ6", OrderKind::Limit, Some(70.0))]);
        assert!(pair.is_ok(), "{pair:?}");
        assert_eq!(sent_as(&fake, dtc::wire::SUBMIT_NEW_OCO_ORDER), ["CLZ6.NYMEX"]);
        session.sign_out();
    }

    /// Flattening ESZ6 held as ESZ6.CME sends ESZ6.CME, the server's own
    /// name for it, and closes it.
    #[test]
    fn on_rithmic_a_flatten_goes_out_as_the_server_named_the_position() {
        let fake = dtc::fake::Fake::builder().account("Sim1").rithmic_symbols().position("Sim1", "ESZ6.CME", -2, 7_578.0).market_price(7_570.0).start();
        let session = impatient(&fake);
        assert_eq!(session.flatten("Sim1", "ESZ6", "CME"), Ok(0));
        assert_eq!(sent_as(&fake, dtc::wire::SUBMIT_FLATTEN_POSITION_ORDER), ["ESZ6.CME"]);
        assert!(until(session.as_ref(), |book| book.position("Sim1", "ESZ6").is_none()), "not flat");
        assert!(fake.received_of(dtc::wire::SECURITY_DEFINITION_FOR_SYMBOL_REQUEST).iter().all(|asked| asked.text("Symbol").as_deref() != Some("ESZ26_FUT_CME")));
        session.sign_out();
    }

    /// A flatten that finds nothing held or working under the name the
    /// server confirmed, as when a fill closed the position as it went, is
    /// already flat, and says so. Under no name the server confirms it is
    /// refused, never taken for done.
    #[test]
    fn a_flatten_of_nothing_held_is_already_flat_unless_no_name_is_known() {
        let fake = dtc::fake::Fake::builder().account("Sim1").rithmic_symbols().start();
        let session = impatient(&fake);
        assert_eq!(session.flatten("Sim1", "ESZ6", "CME"), Ok(0));
        assert!(fake.received_of(dtc::wire::SUBMIT_FLATTEN_POSITION_ORDER).is_empty());
        session.sign_out();

        let fake = dtc::fake::Fake::builder().account("Sim1").rithmic_symbols().unknown_contracts_unanswered().start();
        let session = impatient(&fake);
        assert!(matches!(session.flatten("Sim1", "ESZ6", "CME"), Err(Failure::Refused(_))));
        assert!(fake.received_of(dtc::wire::SUBMIT_FLATTEN_POSITION_ORDER).is_empty());
        session.sign_out();
    }

    /// Unix seconds now.
    fn now() -> i64 {
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64
    }

    /// Sim1 filled ES as Sierra Chart's own service spells it, and Sierra
    /// Chart is on Rithmic now: that spelling is asked of like any other,
    /// and the order goes out as the server confirms, ESZ6.CME.
    #[test]
    fn a_spelling_from_old_fills_is_confirmed_before_it_is_used() {
        let fake = dtc::fake::Fake::builder()
            .account("Sim1")
            .fill("Sim1", "ESZ26_FUT_CME", dtc::Side::Buy, 1, 7_600.0, now() - 60)
            .rithmic_symbols()
            .start();
        let session = impatient(&fake);
        session.place(&sell_one("ESZ6", OrderKind::Limit, Some(9_000.0))).expect("taken");
        assert_eq!(sent_as(&fake, dtc::wire::SUBMIT_NEW_SINGLE_ORDER), ["ESZ6.CME"]);
        session.sign_out();
    }

    /// Sierra Chart moved from its own service to Rithmic while signed in:
    /// the spelling it confirmed before is forgotten once the connection
    /// is back, and the next order goes out as it spells the contract now.
    #[test]
    fn a_spelling_confirmed_is_forgotten_when_the_connection_comes_back() {
        let fake = dtc::fake::Fake::builder().account("Sim1").start();
        let session = impatient_on(&fake, NO_FOLDER);
        let basket = session.place(&sell_one("ESZ6", OrderKind::Limit, Some(9_000.0))).expect("taken");
        session.cancel("Sim1", &basket).expect("cancelled");
        fake.set_rithmic_symbols(true);
        fake.drop_connections();
        assert!(again(&fake, &session), "never reconnected");
        assert!(until(session.as_ref(), |book| book.refreshed), "never back");
        session.place(&sell_one("ESZ6", OrderKind::Limit, Some(9_000.0))).expect("taken on Rithmic");
        assert_eq!(sent_as(&fake, dtc::wire::SUBMIT_NEW_SINGLE_ORDER), ["ESZ26_FUT_CME", "ESZ6.CME"]);
        session.sign_out();
    }

    /// Wait for `session` to be on a connection after its first, and to
    /// have begun asking `fake` for its book again on it.
    fn again(fake: &dtc::fake::Fake, session: &SierraSession) -> bool {
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while std::time::Instant::now() < deadline {
            if session.trading.connection() > 0 && fake.received_of(dtc::wire::TRADE_ACCOUNTS_REQUEST).len() > 1 {
                return true;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        false
    }

    /// Sierra Chart held ES and had confirmed NQ in its own spelling, and
    /// comes back on Rithmic, the book never asked for again in full: the
    /// last connection's names, the position's among them, are not the
    /// server's now. Each order goes out under the name the new connection
    /// confirms, never one from before the drop.
    #[test]
    fn names_from_before_a_drop_are_not_used_while_the_book_is_asked_for_again() {
        let fake = dtc::fake::Fake::builder().account("Sim1").position("Sim1", "ESZ26_FUT_CME", 1, 7_600.0).start();
        let session = impatient_on(&fake, NO_FOLDER);
        session.place(&sell_one("NQZ6", OrderKind::Limit, Some(30_000.0))).expect("taken");
        fake.set_rithmic_symbols(true);
        fake.set_unanswered(Some(dtc::wire::CURRENT_POSITIONS_REQUEST));
        fake.drop_connections();
        assert!(again(&fake, &session), "never reconnected");
        assert!(!session.trading.book().synced);
        session.place(&sell_one("ESZ6", OrderKind::Limit, Some(9_000.0))).expect("ES taken on Rithmic");
        session.place(&sell_one("NQZ6", OrderKind::Limit, Some(30_000.0))).expect("NQ taken on Rithmic");
        assert_eq!(sent_as(&fake, dtc::wire::SUBMIT_NEW_SINGLE_ORDER), ["NQZ26_FUT_CME", "ESZ6.CME", "NQZ6.CME"]);
        session.sign_out();
    }

    /// A server silent on the names it does not know, as Sierra is, and
    /// the style Sierra Chart's folder is in not the one it trades in: the
    /// first order of a contract with no file, MES, and of NG, each wait a
    /// moment, not a wait per name. A contract it knows by no name is
    /// refused after one wait, and again at once, unasked.
    #[test]
    fn names_are_confirmed_in_one_wait_and_an_unknown_contract_is_refused_at_once_again() {
        let fake = dtc::fake::Fake::builder()
            .account("Sim1")
            .rithmic_symbols()
            .unknown_contracts_unanswered()
            .definition("MESZ6.CME", "MES", 0.25, 1.25, 1797206400, 1.0)
            .definition("NGZ26.NYMEX", "NG", 0.001, 10.0, 1797206400, 1.0)
            .start();
        let session = impatient_on(&fake, Folder { stem: |_| None, style: || Some(feed::Style::Underscored) });
        for (contract, wanted) in [("MESZ6", "MESZ6.CME"), ("NGZ6", "NGZ26.NYMEX")] {
            let started = std::time::Instant::now();
            session.place(&sell_one(contract, OrderKind::Limit, Some(9_000.0))).expect("taken");
            assert!(started.elapsed() < Duration::from_millis(600), "{contract}: {:?}", started.elapsed());
            assert_eq!(sent_as(&fake, dtc::wire::SUBMIT_NEW_SINGLE_ORDER).last().map(String::as_str), Some(wanted));
        }
        let started = std::time::Instant::now();
        let refused = session.place(&sell_one("NQZ6", OrderKind::Limit, Some(9_000.0)));
        let waited = started.elapsed();
        assert!(matches!(&refused, Err(Failure::Refused(why)) if why.contains("NQZ6")), "{refused:?}");
        assert!(waited >= Duration::from_millis(650) && waited < Duration::from_millis(1_400), "{waited:?}: one wait, not three");
        let asked = fake.received_of(dtc::wire::SECURITY_DEFINITION_FOR_SYMBOL_REQUEST).len();
        let started = std::time::Instant::now();
        assert!(session.place(&sell_one("NQZ6", OrderKind::Limit, Some(9_000.0))).is_err());
        assert!(started.elapsed() < Duration::from_millis(100), "{:?} to refuse again", started.elapsed());
        assert_eq!(fake.received_of(dtc::wire::SECURITY_DEFINITION_FOR_SYMBOL_REQUEST).len(), asked, "asked again");
        assert_eq!(sent_as(&fake, dtc::wire::SUBMIT_NEW_SINGLE_ORDER).len(), 2);
        session.sign_out();
    }

    /// The folder of Sierra Chart on its own service, its December E-mini
    /// file `ESZ26_FUT_CME`.
    const OWN_FOLDER: Folder =
        Folder { stem: |contract| (contract == "ESZ6").then(|| "ESZ26_FUT_CME".to_string()), style: || Some(feed::Style::Underscored) };

    /// Sierra Chart on its own service, silent on every name, its own
    /// included, and holding NQ in its own spelling: a market order of ES
    /// goes out under the name of the file the chart is drawn from, and
    /// the Log says so. What it fills at shows no price, rather than one in
    /// the server's hundredths, until the definition comes, then its real
    /// one. A priced order, whose price needs the definition, does not go.
    #[test]
    fn a_market_order_goes_out_under_its_file_s_name_when_the_server_answers_for_no_name() {
        let fake = dtc::fake::Fake::builder()
            .account("Sim1")
            .unknown_contracts_unanswered()
            .position("Sim1", "NQZ26_FUT_CME", 1, 26_000.0)
            .market_price(765_025.0)
            .start();
        let session = impatient_on(&fake, OWN_FOLDER);
        let basket = session.place(&sell_one("ESZ6", OrderKind::Market, None)).expect("taken");
        assert_eq!(sent_as(&fake, dtc::wire::SUBMIT_NEW_SINGLE_ORDER), ["ESZ26_FUT_CME"]);
        assert!(session.place(&sell_one("ESZ6", OrderKind::Limit, Some(9_000.0))).is_err());
        session.place(&sell_one("ESZ6", OrderKind::Market, None)).expect("taken again, at once");
        assert_eq!(sent_as(&fake, dtc::wire::SUBMIT_NEW_SINGLE_ORDER), ["ESZ26_FUT_CME", "ESZ26_FUT_CME"]);
        fake.fill(&basket, 1, 765_025.0);
        assert!(until(session.as_ref(), |book| book.position("Sim1", "ESZ6").is_some()), "never held");
        let book = session.book();
        assert_eq!(book.position("Sim1", "ESZ6").map(|held| (held.net, held.avg)), Some((-1, None)), "a price a hundred times its own");
        assert_eq!(book.order(&basket).and_then(|order| order.average_fill()), None);
        fake.define("ESZ26_FUT_CME", "ES", 0.25, 12.5, 1797206400, 0.009999999776482582);
        session.refresh().expect("refreshed");
        assert!(until(session.as_ref(), |book| book.position("Sim1", "ESZ6").and_then(|held| held.avg) == Some(7_650.25)), "never real");
        assert_eq!(session.book().order(&basket).and_then(|order| order.average_fill()), Some(7_650.25));
        session.sign_out();
    }

    /// The same, but nothing held or working, so nothing says how the
    /// server spells: the folder's style is the folder's own, and the
    /// market order is refused, saying Sierra Chart didn't confirm the
    /// contract. Refused too when what is held is in another style.
    #[test]
    fn a_market_order_under_an_unconfirmed_name_needs_the_server_s_own_style() {
        let fake = dtc::fake::Fake::builder().account("Sim1").unknown_contracts_unanswered().market_price(7_600.0).start();
        let session = impatient_on(&fake, OWN_FOLDER);
        for _ in 0..2 {
            let refused = session.place(&sell_one("ESZ6", OrderKind::Market, None));
            assert!(matches!(&refused, Err(Failure::Refused(why)) if why.contains("didn't confirm ESZ6")), "{refused:?}");
        }
        assert!(sent_as(&fake, dtc::wire::SUBMIT_NEW_SINGLE_ORDER).is_empty());
        session.sign_out();

        let fake =
            dtc::fake::Fake::builder().account("Sim1").unknown_contracts_unanswered().position("Sim1", "NQZ6.CME", 1, 26_000.0).market_price(7_600.0).start();
        let session = impatient_on(&fake, OWN_FOLDER);
        assert!(session.place(&sell_one("ESZ6", OrderKind::Market, None)).is_err(), "sent in another style than the server's");
        assert!(sent_as(&fake, dtc::wire::SUBMIT_NEW_SINGLE_ORDER).is_empty());
        session.sign_out();
    }

    /// Closed while its flag is off: offered nowhere, and a saved login of
    /// it shown nowhere, though it is kept.
    #[test]
    fn the_service_is_closed_while_its_flag_is_off() {
        let store = crate::store::Store::memory().unwrap();
        let kept = crate::trading::fake::Kept::default();
        let typed = Typed {
            service: "sierra".into(),
            name: "Sierra Chart".into(),
            user: String::new(),
            password: String::new(),
            values: vec![(setup::PORT.into(), "11099".into())],
        };
        crate::features::force(crate::features::Feature::Sierra, true);
        assert!(registry().get("sierra").is_some());
        add_login(&store, &kept, &typed).expect("added");
        assert_eq!(logins(&store).len(), 1);
        crate::features::force(crate::features::Feature::Sierra, false);
        assert!(registry().get("sierra").is_none());
        assert!(registry().closed("sierra"));
        assert!(logins(&store).is_empty(), "shown nowhere");
        assert!(store.setting(LOGINS).is_some_and(|json| json.contains("sierra")), "kept");
    }

    /// Nothing is reachable: no session, and the words say so.
    #[test]
    fn a_server_that_is_not_there_is_no_session() {
        let unused = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        assert!(Sierra.sign_in(&login(unused), Box::new(|_| {})).is_err());
    }
}
