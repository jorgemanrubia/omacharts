//! A trading session on Sierra Chart's DTC server: its accounts, what they
//! hold and have working, and the orders placed, moved and cancelled
//! through it.
//!
//! **One connection, one thread.** The thread reads, and in the same loop
//! sends the heartbeat when one is due and notices a server that has gone
//! quiet; a caller's request writes on the same socket under a lock and
//! waits on the [`State`] the thread folds every message into. Nothing
//! polls: what Sierra pushes lands in the book and a [`Pulse`] says so.
//!
//! **Prices are the server's own on the wire, and real in the book.**
//! Sierra's futures service sends CME prices as hundredths, an MES fill at
//! 7651.25 as 765125, and says so in each contract's
//! `DisplayPriceMultiplier`. So the session learns every contract's
//! multiplier from its definition before it shows or sends a price for it,
//! and everything this module hands out or takes in is a real price.
//!
//! **Orders are never sent behind a trader's back.** A drop is reported
//! ([`Pulse::Lost`]), reconnected with a doubling wait, and everything the
//! accounts hold and have working is asked for again before it says
//! [`Pulse::Back`]; nothing placed is resent, and placing while the
//! connection is down fails at once.

use std::collections::{HashMap, HashSet};
use std::io::{self, Read};
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::link::{self, Link, Sender};
use crate::reference::Definition;
use crate::wire::{self, Message};
use crate::{Endpoint, Error, Server, now};

/// How long each part of a session may take. [`Default`] is what Sierra is
/// spoken to with; the tests shrink it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Timing {
    /// How often each side sends a heartbeat. DTC wants 5 to 60 seconds of
    /// a real server; the server is taken to be gone after twice this with
    /// nothing heard.
    pub heartbeat: Duration,
    /// How long the connect and the logon may take.
    pub connect: Duration,
    /// How long a request may wait for its answer.
    pub request: Duration,
    /// The first wait before reconnecting, doubled after each failure.
    pub backoff: Duration,
    /// The longest wait before reconnecting.
    pub backoff_cap: Duration,
}

impl Default for Timing {
    fn default() -> Timing {
        Timing {
            heartbeat: Duration::from_secs(10),
            connect: Duration::from_secs(5),
            request: Duration::from_secs(10),
            backoff: Duration::from_millis(500),
            backoff_cap: Duration::from_secs(30),
        }
    }
}

/// How often the thread looks up from the socket: to send a heartbeat, to
/// notice a silent server, to see it has been asked to stop.
const TICK: Duration = Duration::from_millis(50);

/// How long a contract's definition may take before a priced order for it
/// is refused rather than sent at a price that might be off a hundredfold.
const DEFINITION: Duration = Duration::from_secs(3);

/// How many recent fills the book keeps.
const FILLS_KEPT: usize = 500;

/// One trade account, as TRADE_ACCOUNT_RESPONSE lists it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Account {
    /// `TradeAccount`: "Sim1", or the broker's account number.
    pub id: String,
    /// `TradingIsDisabled`.
    pub trading_disabled: bool,
    /// Sierra's simulated accounts are named "Sim" and a number; anything
    /// else is taken to trade for real.
    pub simulated: bool,
}

impl Account {
    fn named(id: String, trading_disabled: bool) -> Account {
        let simulated = id.get(..3).is_some_and(|prefix| prefix.eq_ignore_ascii_case("sim"));
        Account { id, trading_disabled, simulated }
    }
}

/// An account's money, from ACCOUNT_BALANCE_UPDATE. Sierra gives these for
/// a broker's accounts; for its own simulated ones it says it has none.
#[derive(Clone, Debug, PartialEq)]
pub struct Balance {
    pub account: String,
    /// `CashBalance`.
    pub cash: f64,
    /// `AccountCurrency`.
    pub currency: Option<String>,
    /// `BalanceAvailableForNewPositions`.
    pub available: Option<f64>,
    /// `OpenPositionsProfitLoss`.
    pub open_pnl: Option<f64>,
    /// `DailyProfitLoss`.
    pub day_pnl: Option<f64>,
    /// `MarginRequirement`.
    pub margin: Option<f64>,
    /// `DailyNetLossLimit`, as the server writes it (Sierra: negative).
    pub daily_loss_limit: Option<f64>,
    /// `DailyNetLossLimitReached`.
    pub loss_limit_reached: bool,
    /// `TradingIsDisabled`.
    pub trading_disabled: bool,
}

/// Which way an order goes: `BuySellEnum`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Buy,
    Sell,
}

impl Side {
    fn code(self) -> i64 {
        match self {
            Side::Buy => wire::BUY,
            Side::Sell => wire::SELL,
        }
    }

    fn of(code: i64) -> Option<Side> {
        match code {
            wire::BUY => Some(Side::Buy),
            wire::SELL => Some(Side::Sell),
            _ => None,
        }
    }
}

/// How an order is priced: the four of `OrderTypeEnum` a trading UI uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OrderKind {
    Market,
    /// At `price1` or better.
    Limit,
    /// A stop-market, triggered at `price1`.
    Stop,
    /// Triggered at `price1`, then a limit at `price2`.
    StopLimit,
}

impl OrderKind {
    fn code(self) -> i64 {
        match self {
            OrderKind::Market => wire::ORDER_TYPE_MARKET,
            OrderKind::Limit => wire::ORDER_TYPE_LIMIT,
            OrderKind::Stop => wire::ORDER_TYPE_STOP,
            OrderKind::StopLimit => wire::ORDER_TYPE_STOP_LIMIT,
        }
    }

    fn of(code: i64) -> Option<OrderKind> {
        match code {
            wire::ORDER_TYPE_MARKET => Some(OrderKind::Market),
            wire::ORDER_TYPE_LIMIT => Some(OrderKind::Limit),
            wire::ORDER_TYPE_STOP => Some(OrderKind::Stop),
            wire::ORDER_TYPE_STOP_LIMIT => Some(OrderKind::StopLimit),
            _ => None,
        }
    }
}

/// How long an order lasts: `TimeInForceEnum`'s day and good-till-cancelled.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TimeInForce {
    #[default]
    Day,
    GoodTillCanceled,
}

impl TimeInForce {
    fn code(self) -> i64 {
        match self {
            TimeInForce::Day => wire::TIF_DAY,
            TimeInForce::GoodTillCanceled => wire::TIF_GOOD_TILL_CANCELED,
        }
    }

    fn of(code: i64) -> Option<TimeInForce> {
        match code {
            wire::TIF_DAY => Some(TimeInForce::Day),
            wire::TIF_GOOD_TILL_CANCELED => Some(TimeInForce::GoodTillCanceled),
            _ => None,
        }
    }
}

/// An order to place.
#[derive(Clone, Debug, PartialEq)]
pub struct NewOrder {
    pub account: String,
    /// Sierra's name for the contract: "ESZ26_FUT_CME".
    pub symbol: String,
    /// Sierra ignores it; another server may want it.
    pub exchange: Option<String>,
    pub side: Side,
    pub qty: u32,
    pub kind: OrderKind,
    /// The limit price, or the stop's trigger.
    pub price1: Option<f64>,
    /// A stop-limit's limit.
    pub price2: Option<f64>,
    pub time_in_force: TimeInForce,
    /// Free text, `FreeFormText`, which comes back on every update of the
    /// order: at most 47 characters are sent.
    pub tag: Option<String>,
}

/// Where an order stands, from `OrderStatusEnum`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Status {
    /// Sent, and not yet working: order sent, pending open, pending child.
    Pending,
    /// Working at its price, or waiting for its trigger.
    Working,
    /// A cancel or a change was asked for and is not yet done.
    Changing,
    /// Part filled, the rest working.
    PartiallyFilled,
    Filled,
    Cancelled,
    /// Refused, in the server's words (`InfoText`).
    Rejected(String),
}

impl Status {
    /// Still to fill or be cancelled.
    pub fn working(&self) -> bool {
        matches!(self, Status::Pending | Status::Working | Status::Changing | Status::PartiallyFilled)
    }

    fn of(code: i64, said: Option<&str>) -> Status {
        match code {
            wire::ORDER_STATUS_OPEN => Status::Working,
            wire::ORDER_STATUS_PENDING_CANCEL_REPLACE | wire::ORDER_STATUS_PENDING_CANCEL => Status::Changing,
            wire::ORDER_STATUS_FILLED => Status::Filled,
            wire::ORDER_STATUS_CANCELED => Status::Cancelled,
            wire::ORDER_STATUS_REJECTED => Status::Rejected(said.unwrap_or("rejected").to_string()),
            wire::ORDER_STATUS_PARTIALLY_FILLED => Status::PartiallyFilled,
            _ => Status::Pending,
        }
    }
}

/// One order the session has heard of: placed here, placed in Sierra or
/// elsewhere on the same account, or working when it signed in.
#[derive(Clone, Debug, PartialEq)]
pub struct Order {
    pub account: String,
    /// `ServerOrderID`, once the server has given one.
    pub server_id: Option<String>,
    /// `ClientOrderID`: the one [`Trading::place`] returned, for an order
    /// placed here.
    pub client_id: Option<String>,
    pub symbol: String,
    pub exchange: Option<String>,
    pub side: Side,
    pub kind: OrderKind,
    pub qty: u32,
    /// How much has filled.
    pub filled: u32,
    pub price1: Option<f64>,
    pub price2: Option<f64>,
    pub time_in_force: Option<TimeInForce>,
    pub status: Status,
    /// `AverageFillPrice`, once some has filled.
    pub avg_fill_price: Option<f64>,
    /// `FreeFormText`, as the order was tagged.
    pub tag: Option<String>,
    /// `InfoText`: what the server last said of it in words.
    pub said: Option<String>,
    /// `OCOLinkedOrderServerOrderID`.
    pub oco_linked: Option<String>,
    /// `ParentServerOrderID`, for a bracket's child.
    pub parent: Option<String>,
    /// When the server last said anything of it, in unix seconds.
    pub at: i64,
}

impl Order {
    /// Is this the order `id` names, by the server's id or the client's?
    pub fn is(&self, id: &str) -> bool {
        self.server_id.as_deref() == Some(id) || self.client_id.as_deref() == Some(id)
    }

    /// The id to name it by: the server's when there is one.
    pub fn id(&self) -> &str {
        self.server_id.as_deref().or(self.client_id.as_deref()).unwrap_or("")
    }
}

/// What an account holds of one contract, from POSITION_UPDATE.
#[derive(Clone, Debug, PartialEq)]
pub struct Position {
    pub account: String,
    pub symbol: String,
    pub exchange: Option<String>,
    /// Positive long, negative short.
    pub qty: i32,
    /// `AveragePrice`.
    pub avg_price: Option<f64>,
    /// `OpenProfitLoss`, in the account's currency, when the server works it
    /// out.
    pub open_pnl: Option<f64>,
}

/// Part or all of an order, filled.
#[derive(Clone, Debug, PartialEq)]
pub struct Fill {
    pub account: String,
    pub symbol: String,
    pub exchange: Option<String>,
    /// The order it filled, by the server's id.
    pub server_id: Option<String>,
    pub side: Side,
    pub qty: u32,
    pub price: f64,
    /// Unix seconds.
    pub at: i64,
    /// `UniqueExecutionID` or `LastFillExecutionID`, by which the same fill
    /// told twice is known to be one.
    pub execution_id: Option<String>,
}

/// Everything the session knows of its accounts' trading, prices real.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Book {
    pub accounts: Vec<Account>,
    /// One per account that has a balance, in the accounts' order.
    pub balances: Vec<Balance>,
    /// Contracts held; flat ones are left out.
    pub positions: Vec<Position>,
    /// Newest first: working ones, and finished ones heard of this session.
    pub orders: Vec<Order>,
    /// Oldest first: today's, and every one since.
    pub fills: Vec<Fill>,
    /// Everything was asked for and answered since the last connect: what
    /// is held and working is all here, not only what was pushed since.
    pub synced: bool,
}

/// What the session tells its caller, from its own thread.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Pulse {
    /// An order, a position, a balance or the account list changed: read the
    /// book again.
    Moved,
    /// The connection dropped, and why. It is being reconnected unless the
    /// error says it cannot be ([`Error::Refused`]).
    Lost(Error),
    /// Connected again, and everything asked for afresh: the book is whole
    /// again.
    Back,
}

/// An answer being waited for.
#[derive(Default)]
struct Slot {
    answers: Vec<Message>,
    done: bool,
    rejected: Option<String>,
    /// Its answers go into the book as well as to whoever waits.
    fold: bool,
}

/// What the thread keeps from the server's messages, and [`Trading::book`]
/// copies out. Prices are kept as they travelled, and made real on the way
/// out with each contract's multiplier.
#[derive(Default)]
pub struct State {
    accounts: Vec<Account>,
    balances: HashMap<String, Balance>,
    positions: Vec<Position>,
    orders: Vec<Order>,
    /// How many times each order has been told of, by its place in `orders`,
    /// and the reason it was told last, for whoever waits on one.
    told: Vec<(u64, i64)>,
    fills: Vec<Fill>,
    /// `DisplayPriceMultiplier` by symbol.
    multipliers: HashMap<String, f64>,
    /// Symbols whose definition has been asked for.
    asked: HashSet<String>,
    requests: HashMap<i64, Slot>,
    synced: bool,
    connected: bool,
}

fn quantity(value: Option<f64>) -> u32 {
    value.map(|qty| qty.max(0.0).round() as u32).unwrap_or(0)
}

/// A price as it travels, made real: rounded to the tenth decimal, because
/// a multiplier that travelled as a 32-bit float is not quite 0.01.
fn real(price: f64, multiplier: f64) -> f64 {
    ((price * multiplier) * 1e10).round() / 1e10
}

fn wire_price(price: f64, multiplier: f64) -> f64 {
    ((price / multiplier) * 1e8).round() / 1e8
}

impl State {
    fn multiplier(&self, symbol: &str) -> f64 {
        self.multipliers.get(symbol).copied().unwrap_or(1.0)
    }

    /// The book, with every price made real.
    pub fn book(&self) -> Book {
        let scale = |symbol: &str, price: Option<f64>| price.map(|price| real(price, self.multiplier(symbol)));
        Book {
            accounts: self.accounts.clone(),
            balances: self.accounts.iter().filter_map(|account| self.balances.get(&account.id).cloned()).collect(),
            positions: self
                .positions
                .iter()
                .filter(|position| position.qty != 0)
                .map(|position| Position { avg_price: scale(&position.symbol, position.avg_price), ..position.clone() })
                .collect(),
            orders: self
                .orders
                .iter()
                .rev()
                .map(|order| Order {
                    price1: scale(&order.symbol, order.price1),
                    price2: scale(&order.symbol, order.price2),
                    avg_fill_price: scale(&order.symbol, order.avg_fill_price),
                    ..order.clone()
                })
                .collect(),
            fills: self.fills.iter().map(|fill| Fill { price: real(fill.price, self.multiplier(&fill.symbol)), ..fill.clone() }).collect(),
            synced: self.synced,
        }
    }

    fn order_at(&self, id: &str) -> Option<usize> {
        self.orders.iter().position(|order| order.is(id))
    }

    /// Fold in one message the server sent. True when the book changed.
    fn fold(&mut self, message: &Message) -> bool {
        match message.kind() {
            wire::TRADE_ACCOUNT_RESPONSE => self.account(message),
            wire::ACCOUNT_BALANCE_UPDATE => self.balance(message),
            wire::POSITION_UPDATE => self.position(message),
            wire::ORDER_UPDATE => self.order(message),
            wire::HISTORICAL_ORDER_FILL_RESPONSE => self.history_fill(message).is_some_and(|fill| self.fill(fill)),
            wire::SECURITY_DEFINITION_RESPONSE => self.definition(message),
            _ => false,
        }
    }

    fn account(&mut self, message: &Message) -> bool {
        let Some(id) = message.text("TradeAccount") else { return false };
        let account = Account::named(id, message.flag("TradingIsDisabled"));
        match self.accounts.iter_mut().find(|known| known.id == account.id) {
            Some(known) if *known == account => false,
            Some(known) => {
                *known = account;
                true
            }
            None => {
                self.accounts.push(account);
                true
            }
        }
    }

    fn balance(&mut self, message: &Message) -> bool {
        if message.flag("NoAccountBalances") {
            return false;
        }
        let Some(account) = message.text("TradeAccount") else { return false };
        let balance = Balance {
            account: account.clone(),
            cash: message.number("CashBalance").unwrap_or(0.0),
            currency: message.text("AccountCurrency"),
            available: message.number("BalanceAvailableForNewPositions"),
            open_pnl: message.number("OpenPositionsProfitLoss"),
            day_pnl: message.number("DailyProfitLoss"),
            margin: message.number("MarginRequirement"),
            daily_loss_limit: message.nonzero("DailyNetLossLimit"),
            loss_limit_reached: message.flag("DailyNetLossLimitReached"),
            trading_disabled: message.flag("TradingIsDisabled"),
        };
        self.balances.insert(account, balance.clone()).as_ref() != Some(&balance)
    }

    fn position(&mut self, message: &Message) -> bool {
        if message.flag("NoPositions") {
            return false;
        }
        let (Some(account), Some(symbol)) = (message.text("TradeAccount"), message.text("Symbol")) else { return false };
        let position = Position {
            account,
            symbol,
            exchange: message.text("Exchange"),
            qty: message.number("Quantity").unwrap_or(0.0).round() as i32,
            avg_price: message.nonzero("AveragePrice"),
            open_pnl: message.number("OpenProfitLoss"),
        };
        match self.positions.iter_mut().find(|known| known.account == position.account && known.symbol == position.symbol) {
            Some(known) if *known == position => false,
            Some(known) => {
                *known = position;
                true
            }
            None => {
                self.positions.push(position);
                true
            }
        }
    }

    fn order(&mut self, message: &Message) -> bool {
        if message.flag("NoOrders") {
            return false;
        }
        let server_id = message.text("ServerOrderID");
        let client_id = message.text("ClientOrderID");
        let previous = message.text("PreviousServerOrderID");
        let at = [server_id.as_deref(), client_id.as_deref(), previous.as_deref()]
            .into_iter()
            .flatten()
            .find_map(|id| self.order_at(id));
        let said = message.text("InfoText");
        let reason = message.int_or_zero("OrderUpdateReason");
        let side = Side::of(message.int_or_zero("BuySell"));
        let kind = OrderKind::of(message.int_or_zero("OrderType"));
        let at = match at {
            Some(at) => at,
            None => {
                let (Some(account), Some(symbol), Some(side), Some(kind)) =
                    (message.text("TradeAccount"), message.text("Symbol"), side, kind)
                else {
                    return false;
                };
                self.orders.push(Order {
                    account,
                    server_id: None,
                    client_id: None,
                    symbol,
                    exchange: None,
                    side,
                    kind,
                    qty: 0,
                    filled: 0,
                    price1: None,
                    price2: None,
                    time_in_force: None,
                    status: Status::Pending,
                    avg_fill_price: None,
                    tag: None,
                    said: None,
                    oco_linked: None,
                    parent: None,
                    at: now(),
                });
                self.told.push((0, 0));
                self.orders.len() - 1
            }
        };
        let order = &mut self.orders[at];
        let before = order.clone();
        order.server_id = server_id.or(order.server_id.take());
        order.client_id = client_id.or(order.client_id.take());
        if let Some(account) = message.text("TradeAccount") {
            order.account = account;
        }
        if let Some(symbol) = message.text("Symbol") {
            order.symbol = symbol;
        }
        order.exchange = message.text("Exchange").or(order.exchange.take());
        order.side = side.unwrap_or(order.side);
        order.kind = kind.unwrap_or(order.kind);
        if let Some(qty) = message.number("OrderQuantity") {
            order.qty = quantity(Some(qty));
        }
        if let Some(filled) = message.number("FilledQuantity") {
            order.filled = quantity(Some(filled));
        }
        // A market order has no price; a stop-limit is the only one with a
        // second. An unset price says nothing of one that is set.
        match message.number("Price1") {
            _ if order.kind == OrderKind::Market => order.price1 = None,
            Some(price) => order.price1 = Some(price),
            None => {}
        }
        match message.number("Price2") {
            _ if order.kind != OrderKind::StopLimit => order.price2 = None,
            Some(price) => order.price2 = Some(price),
            None => {}
        }
        order.time_in_force = TimeInForce::of(message.int_or_zero("TimeInForce")).or(order.time_in_force);
        order.avg_fill_price = message.nonzero("AverageFillPrice").or(order.avg_fill_price);
        // A tag given here stands: the two of a pair share one
        // `FreeFormText` on the wire, and each keeps its own.
        order.tag = order.tag.take().or(message.text("FreeFormText"));
        order.oco_linked = message.text("OCOLinkedOrderServerOrderID").or(order.oco_linked.take());
        order.parent = message.text("ParentServerOrderID").or(order.parent.take());
        if said.is_some() {
            order.said = said.clone();
        }
        // A cancel or a change refused leaves the order as it was, and says
        // why only in the text; the status stays what the server says it is.
        let status = message.int_or_zero("OrderStatus");
        if status != wire::ORDER_STATUS_UNSPECIFIED || reason == wire::NEW_ORDER_REJECTED {
            let code = if reason == wire::NEW_ORDER_REJECTED { wire::ORDER_STATUS_REJECTED } else { status };
            order.status = Status::of(code, said.as_deref().or(order.said.as_deref()));
        }
        order.at = message
            .nonzero("LatestTransactionDateTime")
            .map(|at| at as i64)
            .unwrap_or_else(now);
        let changed = *order != before;
        let told = &mut self.told[at];
        told.0 += 1;
        told.1 = reason;

        let fill = (reason == wire::ORDER_FILLED || reason == wire::ORDER_FILLED_PARTIALLY)
            .then(|| self.update_fill(message, at))
            .flatten();
        let filled = fill.is_some_and(|fill| self.fill(fill));
        changed || filled
    }

    /// The fill an ORDER_UPDATE tells of, when it tells of one.
    fn update_fill(&self, message: &Message, at: usize) -> Option<Fill> {
        let order = &self.orders[at];
        let qty = quantity(message.number("LastFillQuantity"));
        let price = message.number("LastFillPrice")?;
        (qty > 0).then(|| Fill {
            account: order.account.clone(),
            symbol: order.symbol.clone(),
            exchange: order.exchange.clone(),
            server_id: order.server_id.clone(),
            side: order.side,
            qty,
            price,
            at: message.nonzero("LastFillDateTime").map(|at| (at / 1000.0) as i64).unwrap_or_else(now),
            execution_id: message.text("LastFillExecutionID"),
        })
    }

    fn history_fill(&self, message: &Message) -> Option<Fill> {
        if message.flag("NoOrderFills") {
            return None;
        }
        Some(Fill {
            account: message.text("TradeAccount")?,
            symbol: message.text("Symbol")?,
            exchange: message.text("Exchange"),
            server_id: message.text("ServerOrderID").filter(|id| id != "0"),
            side: Side::of(message.int_or_zero("BuySell"))?,
            qty: quantity(message.number("Quantity")),
            price: message.number("Price")?,
            at: message.int_or_zero("DateTime"),
            execution_id: message.text("UniqueExecutionID"),
        })
    }

    /// Note a fill, once however many times it is told. True when it is new.
    fn fill(&mut self, fill: Fill) -> bool {
        let same = |known: &Fill| match (&known.execution_id, &fill.execution_id) {
            (Some(a), Some(b)) => a == b,
            _ => known.server_id == fill.server_id && known.at == fill.at && known.qty == fill.qty && known.price == fill.price,
        };
        if self.fills.iter().any(same) {
            return false;
        }
        let at = self.fills.partition_point(|known| known.at <= fill.at);
        self.fills.insert(at, fill);
        if self.fills.len() > FILLS_KEPT {
            self.fills.remove(0);
        }
        true
    }

    fn definition(&mut self, message: &Message) -> bool {
        let Some(definition) = Definition::from_message(message) else { return false };
        let before = self.multipliers.insert(definition.symbol.clone(), definition.price_multiplier);
        before != Some(definition.price_multiplier) && definition.price_multiplier != 1.0
    }

    /// The symbols named in the book whose definition nobody has asked for.
    fn unasked(&mut self) -> Vec<String> {
        let named = self
            .orders
            .iter()
            .map(|order| &order.symbol)
            .chain(self.positions.iter().map(|position| &position.symbol))
            .chain(self.fills.iter().map(|fill| &fill.symbol));
        let mut fresh = Vec::new();
        for symbol in named {
            if !self.multipliers.contains_key(symbol) && !self.asked.contains(symbol) {
                fresh.push(symbol.clone());
            }
        }
        fresh.sort();
        fresh.dedup();
        self.asked.extend(fresh.iter().cloned());
        fresh
    }

    /// An open orders snapshot for `account` is done: what it did not list
    /// is no longer working, and goes, rather than be shown working when it
    /// is not.
    fn open_orders_listed(&mut self, account: &str, answers: &[Message]) {
        let listed: HashSet<String> = answers.iter().filter_map(|message| message.text("ServerOrderID")).collect();
        let mut at = 0;
        while at < self.orders.len() {
            let order = &self.orders[at];
            let unlisted = order.account == account
                && order.status.working()
                && order.server_id.as_ref().is_some_and(|id| !listed.contains(id));
            if unlisted {
                self.orders.remove(at);
                self.told.remove(at);
            } else {
                at += 1;
            }
        }
    }

    /// A positions snapshot for `account` is done: what it did not list is
    /// flat.
    fn positions_listed(&mut self, account: &str, answers: &[Message]) {
        let listed: HashSet<String> = answers.iter().filter(|message| !message.flag("NoPositions")).filter_map(|message| message.text("Symbol")).collect();
        self.positions.retain(|position| position.account != account || listed.contains(&position.symbol));
    }
}

#[cfg(test)]
impl State {
    /// Fold in one message as the thread would, for a replayed session.
    pub(crate) fn fold_for_test(&mut self, message: &Message) -> bool {
        self.fold(message)
    }
}

/// Everything the handle and the thread share.
struct Shared {
    endpoint: Endpoint,
    client_name: String,
    timing: Timing,
    pulse: Box<dyn Fn(Pulse) + Send + Sync>,
    state: Mutex<State>,
    changed: Condvar,
    sender: Mutex<Option<Sender>>,
    server: Mutex<Server>,
    trouble: Mutex<Option<Error>>,
    stop: AtomicBool,
    ids: AtomicI64,
    /// Where the next ClientOrderID starts: the sign-in's milliseconds, so
    /// one session's ids are not another's.
    orders: AtomicI64,
    threads: Mutex<Vec<JoinHandle<()>>>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

impl Shared {
    fn stopped(&self) -> bool {
        self.stop.load(Ordering::SeqCst)
    }

    fn send(&self, message: &Message) -> Result<(), Error> {
        let sender = lock(&self.sender).clone();
        match sender {
            Some(sender) => sender.send(message),
            None if self.stopped() => Err(Error::Closed),
            None => Err(Error::Unreachable("not connected to Sierra Chart".into())),
        }
    }

    /// Wait until `check` finds what it looks for in the state, the session
    /// stops, the connection drops, or `timeout` passes.
    fn wait_for<T>(&self, timeout: Duration, mut check: impl FnMut(&mut State) -> Option<T>) -> Result<T, Error> {
        let deadline = Instant::now() + timeout;
        let mut state = lock(&self.state);
        loop {
            if let Some(found) = check(&mut state) {
                return Ok(found);
            }
            if self.stopped() {
                return Err(Error::Closed);
            }
            if !state.connected {
                return Err(Error::Unreachable("the connection to Sierra Chart dropped".into()));
            }
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(Error::Timeout);
            }
            state = self.changed.wait_timeout(state, left.min(TICK * 4)).unwrap_or_else(|poisoned| poisoned.into_inner()).0;
        }
    }

    /// Send a request and note that its answers are awaited; [`finish`]
    /// waits for them. Its answers go into the book when `fold` is set.
    fn begin(&self, kind: u16, mut fields: Value, fold: bool) -> Result<i64, Error> {
        let id = self.ids.fetch_add(1, Ordering::SeqCst);
        fields["RequestID"] = json!(id);
        lock(&self.state).requests.insert(id, Slot { fold, ..Slot::default() });
        if let Err(error) = self.send(&Message::new(kind, fields)) {
            lock(&self.state).requests.remove(&id);
            return Err(error);
        }
        Ok(id)
    }

    fn finish(&self, id: i64, timeout: Duration) -> Result<Vec<Message>, Error> {
        let found = self.wait_for(timeout, |state| {
            let done = state.requests.get(&id).is_some_and(|slot| slot.done);
            done.then(|| state.requests.remove(&id)).flatten()
        });
        let slot = match found {
            Ok(slot) => slot,
            Err(error) => {
                lock(&self.state).requests.remove(&id);
                return Err(error);
            }
        };
        match slot.rejected {
            Some(why) => Err(Error::Rejected(why)),
            None => Ok(slot.answers),
        }
    }

    fn ask(&self, kind: u16, fields: Value, fold: bool) -> Result<Vec<Message>, Error> {
        let id = self.begin(kind, fields, fold)?;
        self.finish(id, self.timing.request)
    }

    fn told(&self) {
        self.changed.notify_all();
        (self.pulse)(Pulse::Moved);
    }

    /// Ask for the definitions of every contract the book names that nobody
    /// has asked of yet. Their answers are folded as they come.
    fn ask_definitions(&self) {
        let fresh = lock(&self.state).unasked();
        for symbol in fresh {
            let _ = self.begin(wire::SECURITY_DEFINITION_FOR_SYMBOL_REQUEST, json!({ "Symbol": symbol, "Exchange": "" }), true);
        }
    }

    /// The multiplier for `symbol`'s prices, asking for its definition if
    /// it is not known: `None` when the server does not say in time.
    fn multiplier(&self, symbol: &str) -> Option<f64> {
        if let Some(multiplier) = lock(&self.state).multipliers.get(symbol) {
            return Some(*multiplier);
        }
        let id = self.begin(wire::SECURITY_DEFINITION_FOR_SYMBOL_REQUEST, json!({ "Symbol": symbol, "Exchange": "" }), true).ok()?;
        let _ = self.finish(id, DEFINITION.min(self.timing.request));
        lock(&self.state).multipliers.get(symbol).copied()
    }

    /// Ask for everything: the accounts, then each one's balance, positions,
    /// working orders and today's fills, and the definition of every
    /// contract they name.
    fn sync(&self) -> Result<(), Error> {
        let listed = self.ask(wire::TRADE_ACCOUNTS_REQUEST, json!({}), true)?;
        let accounts: Vec<String> = listed.iter().filter_map(|message| message.text("TradeAccount")).collect();
        {
            let mut state = lock(&self.state);
            state.accounts.retain(|account| accounts.contains(&account.id));
        }
        self.snapshot(&accounts, true)?;
        let symbols: Vec<String> = {
            let state = lock(&self.state);
            let mut symbols: Vec<String> = state
                .orders
                .iter()
                .map(|order| order.symbol.clone())
                .chain(state.positions.iter().map(|position| position.symbol.clone()))
                .filter(|symbol| !state.multipliers.contains_key(symbol))
                .collect();
            symbols.sort();
            symbols.dedup();
            symbols
        };
        for symbol in symbols {
            lock(&self.state).asked.insert(symbol.clone());
            let _ = self.multiplier(&symbol);
        }
        self.ask_definitions();
        lock(&self.state).synced = true;
        self.told();
        Ok(())
    }

    /// Balances, positions, working orders and, with `fills`, today's fills
    /// for each of `accounts`: every request sent, then every answer waited
    /// for.
    fn snapshot(&self, accounts: &[String], fills: bool) -> Result<(), Error> {
        let since = now() - 24 * 3600;
        let mut asked = Vec::new();
        for account in accounts {
            asked.push((account, 0, self.begin(wire::ACCOUNT_BALANCE_REQUEST, json!({ "TradeAccount": account }), true)?));
            asked.push((account, 1, self.begin(wire::CURRENT_POSITIONS_REQUEST, json!({ "TradeAccount": account }), true)?));
            asked.push((
                account,
                2,
                self.begin(wire::OPEN_ORDERS_REQUEST, json!({ "TradeAccount": account, "RequestAllOrders": 1, "ServerOrderID": "" }), true)?,
            ));
            if fills {
                asked.push((
                    account,
                    3,
                    self.begin(
                        wire::HISTORICAL_ORDER_FILLS_REQUEST,
                        json!({ "TradeAccount": account, "NumberOfDays": 1, "StartDateTime": since, "ServerOrderID": "" }),
                        true,
                    )?,
                ));
            }
        }
        let mut first = None;
        for (account, what, id) in asked {
            match self.finish(id, self.timing.request) {
                Ok(answers) => {
                    let mut state = lock(&self.state);
                    match what {
                        1 => state.positions_listed(account, &answers),
                        2 => state.open_orders_listed(account, &answers),
                        _ => {}
                    }
                }
                // A request the server refuses for one account (a balance for
                // a simulated one, say) leaves that part empty; the rest stands.
                Err(Error::Rejected(_)) => {}
                Err(error) => {
                    first.get_or_insert(error);
                }
            }
        }
        match first {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    fn next_client_id(&self) -> String {
        let n = self.orders.fetch_add(1, Ordering::SeqCst);
        format!("oma{}", radix36(n as u64))
    }
}

/// A tag as `FreeFormText` carries it: at most 47 characters.
fn clipped(tag: Option<&str>) -> Option<String> {
    tag.map(|tag| tag.chars().take(47).collect())
}

fn radix36(mut n: u64) -> String {
    const DIGITS: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let mut out = Vec::new();
    loop {
        out.push(DIGITS[(n % 36) as usize]);
        n /= 36;
        if n == 0 {
            break;
        }
    }
    out.reverse();
    String::from_utf8(out).unwrap_or_default()
}

/// A trading session, shared: every clone is the same session.
#[derive(Clone)]
pub struct Trading {
    shared: Arc<Shared>,
}

impl std::fmt::Debug for Trading {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Trading").field("endpoint", &self.shared.endpoint).finish_non_exhaustive()
    }
}

/// How a connection ended, as the thread saw it.
enum Ended {
    /// [`Trading::sign_out`] asked it to.
    Stopped,
    /// It dropped: reconnect.
    Dropped(Error),
    /// The server said not to come back.
    Final(Error),
}

/// The words for a server that will not trade.
const NO_TRADING: &str =
    "Trading isn't allowed — turn on Allow Trading in Sierra Chart Server Settings, [DTC Protocol Server]";

impl Trading {
    /// Sign in to `endpoint` as `client_name` and ask for everything its
    /// accounts hold, blocking until it has been answered. `pulse` is called
    /// on the session's thread whenever the book moves or the connection
    /// drops or comes back.
    pub fn sign_in(endpoint: &Endpoint, client_name: &str, pulse: impl Fn(Pulse) + Send + Sync + 'static) -> Result<Trading, Error> {
        Trading::sign_in_with(endpoint, client_name, Timing::default(), pulse)
    }

    /// [`Trading::sign_in`], with its own [`Timing`].
    pub fn sign_in_with(
        endpoint: &Endpoint,
        client_name: &str,
        timing: Timing,
        pulse: impl Fn(Pulse) + Send + Sync + 'static,
    ) -> Result<Trading, Error> {
        let link = open_trading(endpoint, client_name, timing)?;
        let started = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|since| since.as_millis() as i64)
            .unwrap_or_default();
        let shared = Arc::new(Shared {
            endpoint: endpoint.clone(),
            client_name: client_name.to_string(),
            timing,
            pulse: Box::new(pulse),
            state: Mutex::new(State { connected: true, ..State::default() }),
            changed: Condvar::new(),
            sender: Mutex::new(Some(link.sender.clone())),
            server: Mutex::new(link.server.clone()),
            trouble: Mutex::new(None),
            stop: AtomicBool::new(false),
            ids: AtomicI64::new(1),
            orders: AtomicI64::new(started),
            threads: Mutex::new(Vec::new()),
        });
        let thread = {
            let shared = shared.clone();
            std::thread::Builder::new()
                .name("dtc".into())
                .spawn(move || run(&shared, link))
                .map_err(|error| Error::Unreachable(error.to_string()))?
        };
        lock(&shared.threads).push(thread);
        let trading = Trading { shared };
        if let Err(error) = trading.shared.sync() {
            trading.sign_out();
            return Err(error);
        }
        Ok(trading)
    }

    /// What the logon said the server is and will do.
    pub fn server(&self) -> Server {
        lock(&self.shared.server).clone()
    }

    /// The accounts the server listed.
    pub fn accounts(&self) -> Vec<Account> {
        lock(&self.shared.state).accounts.clone()
    }

    /// `account`'s money, when the server gives it.
    pub fn balance(&self, account: &str) -> Option<Balance> {
        lock(&self.shared.state).balances.get(account).cloned()
    }

    /// Everything the session knows, as it stands.
    pub fn book(&self) -> Book {
        lock(&self.shared.state).book()
    }

    /// Why the session is not connected, while it is not: the drop it is
    /// reconnecting from, or the refusal that stopped it.
    pub fn trouble(&self) -> Option<Error> {
        lock(&self.shared.trouble).clone()
    }

    /// Place `order`, and wait until the server has taken it or refused it:
    /// past the order sent and pending open statuses to working, filled or
    /// any other it gives, or to rejected, which is [`Error::Rejected`] with
    /// the server's words. Returns the order's ClientOrderID. On
    /// [`Error::Timeout`] the order may yet be working: it stays in the book
    /// as pending, and what the server says of it later lands there.
    pub fn place(&self, order: &NewOrder) -> Result<String, Error> {
        let priced = self.priced(order)?;
        let id = self.shared.next_client_id();
        let tag = clipped(order.tag.as_deref());
        let message = Message::new(
            wire::SUBMIT_NEW_SINGLE_ORDER,
            json!({
                "Symbol": order.symbol,
                "Exchange": order.exchange.clone().unwrap_or_default(),
                "TradeAccount": order.account,
                "ClientOrderID": id,
                "OrderType": order.kind.code(),
                "BuySell": order.side.code(),
                "Price1": priced.1.unwrap_or(0.0),
                "Price2": priced.2.unwrap_or(0.0),
                "Quantity": order.qty,
                "TimeInForce": order.time_in_force.code(),
                "FreeFormText": tag.clone().unwrap_or_default(),
                "IsAutomatedOrder": 0,
            }),
        );
        self.expect(&[(order, &id, priced, tag)]);
        self.submit(&message, &[&id])?;
        match self.taken(&id)? {
            Status::Rejected(why) => Err(Error::Rejected(why)),
            _ => Ok(id),
        }
    }

    /// Place `orders` as one one-cancels-other pair, SUBMIT_NEW_OCO_ORDER:
    /// whichever fills or is cancelled first cancels the other, and a part
    /// of one filled takes as much off the other
    /// (`PARTIAL_FILL_HANDLING_REDUCE_QUANTITY`), so the two never close
    /// more than one of them would. No parent: the pair stands on its own,
    /// for a position already held. Both are on one account and one
    /// contract, good for one time in force; the message carries one
    /// `FreeFormText`, the first's tag, and each order keeps its own tag
    /// here.
    ///
    /// Waits until the server has taken or refused both, as
    /// [`Trading::place`] does one, and returns their ClientOrderIDs in
    /// order. DTC rejects a pair whole; a server that took one and
    /// refused the other is [`Error::Rejected`] too, saying so, and the one
    /// it took stays working in the book.
    pub fn place_pair(&self, orders: &[NewOrder; 2]) -> Result<[String; 2], Error> {
        let [first, second] = orders;
        if first.account != second.account || first.symbol != second.symbol || first.time_in_force != second.time_in_force {
            return Err(Error::Rejected("a pair is on one account and one contract, for one time in force".into()));
        }
        let priced = [self.priced(first)?, self.priced(second)?];
        let ids = [self.shared.next_client_id(), self.shared.next_client_id()];
        let tags = [clipped(first.tag.as_deref()), clipped(second.tag.as_deref())];
        let message = Message::new(
            wire::SUBMIT_NEW_OCO_ORDER,
            json!({
                "Symbol": first.symbol,
                "Exchange": first.exchange.clone().unwrap_or_default(),
                "ClientOrderID_1": ids[0],
                "OrderType_1": first.kind.code(),
                "BuySell_1": first.side.code(),
                "Price1_1": priced[0].1.unwrap_or(0.0),
                "Price2_1": priced[0].2.unwrap_or(0.0),
                "Quantity_1": first.qty,
                "ClientOrderID_2": ids[1],
                "OrderType_2": second.kind.code(),
                "BuySell_2": second.side.code(),
                "Price1_2": priced[1].1.unwrap_or(0.0),
                "Price2_2": priced[1].2.unwrap_or(0.0),
                "Quantity_2": second.qty,
                "TimeInForce": first.time_in_force.code(),
                "TradeAccount": first.account,
                "IsAutomatedOrder": 0,
                "ParentTriggerClientOrderID": "",
                "FreeFormText": tags[0].clone().unwrap_or_default(),
                "PartialFillHandling": wire::PARTIAL_FILL_HANDLING_REDUCE_QUANTITY,
            }),
        );
        let [tag_1, tag_2] = tags;
        self.expect(&[(first, &ids[0], priced[0], tag_1), (second, &ids[1], priced[1], tag_2)]);
        self.submit(&message, &[&ids[0], &ids[1]])?;
        let taken = [self.taken(&ids[0])?, self.taken(&ids[1])?];
        match taken {
            [Status::Rejected(why), Status::Rejected(_)] => Err(Error::Rejected(why)),
            [Status::Rejected(why), _] | [_, Status::Rejected(why)] => {
                Err(Error::Rejected(format!("Sierra Chart took only one of the pair: {why}")))
            }
            _ => Ok(ids),
        }
    }

    /// `order`'s prices checked against its kind and put in the server's
    /// units: the contract's multiplier, Price1 and Price2 on the wire.
    fn priced(&self, order: &NewOrder) -> Result<(f64, Option<f64>, Option<f64>), Error> {
        if order.qty == 0 {
            return Err(Error::Rejected("an order needs a quantity".into()));
        }
        let (price1, price2) = match order.kind {
            OrderKind::Market => (None, None),
            OrderKind::Limit | OrderKind::Stop => {
                (Some(order.price1.ok_or_else(|| Error::Rejected("this order needs a price".into()))?), None)
            }
            OrderKind::StopLimit => (
                Some(order.price1.ok_or_else(|| Error::Rejected("a stop-limit needs its stop price".into()))?),
                Some(order.price2.ok_or_else(|| Error::Rejected("a stop-limit needs its limit price".into()))?),
            ),
        };
        let multiplier = match price1 {
            None => 1.0,
            Some(_) => self.shared.multiplier(&order.symbol).ok_or_else(|| {
                Error::Rejected(format!("Sierra Chart did not say how {} is priced, so the order was not sent", order.symbol))
            })?,
        };
        Ok((multiplier, price1.map(|price| wire_price(price, multiplier)), price2.map(|price| wire_price(price, multiplier))))
    }

    /// Put each order about to be sent in the book as pending, under its
    /// ClientOrderID, so that what the server says of it lands on it.
    #[allow(clippy::type_complexity)]
    fn expect(&self, orders: &[(&NewOrder, &String, (f64, Option<f64>, Option<f64>), Option<String>)]) {
        let mut state = lock(&self.shared.state);
        for (order, id, (multiplier, wire1, wire2), tag) in orders {
            state.multipliers.entry(order.symbol.clone()).or_insert(*multiplier);
            state.orders.push(Order {
                account: order.account.clone(),
                server_id: None,
                client_id: Some((*id).clone()),
                symbol: order.symbol.clone(),
                exchange: order.exchange.clone(),
                side: order.side,
                kind: order.kind,
                qty: order.qty,
                filled: 0,
                price1: *wire1,
                price2: *wire2,
                time_in_force: Some(order.time_in_force),
                status: Status::Pending,
                avg_fill_price: None,
                tag: tag.clone(),
                said: None,
                oco_linked: None,
                parent: None,
                at: now(),
            });
            state.told.push((0, 0));
        }
    }

    /// Send `message`; when it cannot go, the orders `ids` it carried are
    /// taken out of the book again.
    fn submit(&self, message: &Message, ids: &[&String]) -> Result<(), Error> {
        if let Err(error) = self.shared.send(message) {
            let mut state = lock(&self.shared.state);
            for id in ids {
                if let Some(at) = state.order_at(id) {
                    state.orders.remove(at);
                    state.told.remove(at);
                }
            }
            return Err(error);
        }
        self.shared.told();
        Ok(())
    }

    /// Wait until the server has taken the order `id` or refused it: past
    /// order sent and pending open, to whatever status it gives.
    fn taken(&self, id: &str) -> Result<Status, Error> {
        self.shared.wait_for(self.shared.timing.request, |state| {
            let at = state.order_at(id)?;
            match &state.orders[at].status {
                Status::Pending if state.told[at].0 == 0 || state.told[at].1 != wire::NEW_ORDER_ACCEPTED => None,
                status => Some(status.clone()),
            }
        })
    }

    /// The order `id` names, by the server's id or the client's.
    fn find(&self, id: &str) -> Result<(Order, u64), Error> {
        let state = lock(&self.shared.state);
        let at = state.order_at(id).ok_or_else(|| Error::Rejected(format!("there is no order {id}")))?;
        Ok((state.orders[at].clone(), state.told[at].0))
    }

    /// Wait for the server to tell of order `id` again with one of `done`
    /// as its reason, or a refusal; the order's words for a refusal.
    fn answered(&self, id: &str, since: u64, done: &[i64], refused: &[i64], finished: impl Fn(&Status) -> bool) -> Result<(), Error> {
        self.shared.wait_for(self.shared.timing.request, |state| {
            let at = state.order_at(id)?;
            let (count, reason) = state.told[at];
            let order = &state.orders[at];
            if count <= since {
                return None;
            }
            if refused.contains(&reason) {
                return Some(Err(Error::Rejected(order.said.clone().unwrap_or_else(|| "the server refused".into()))));
            }
            (done.contains(&reason) || finished(&order.status)).then_some(Ok(()))
        })?
    }

    /// Cancel the order `id` names, by the server's id or the client's, and
    /// wait until the server says it is cancelled, or why not.
    pub fn cancel(&self, id: &str) -> Result<(), Error> {
        let (order, since) = self.find(id)?;
        if !order.status.working() {
            return Err(Error::Rejected(format!("order {id} is no longer working")));
        }
        self.send_cancel(&order)?;
        self.answered(id, since, &[wire::ORDER_CANCELED], &[wire::ORDER_CANCEL_REJECTED], |status| !status.working())
    }

    fn send_cancel(&self, order: &Order) -> Result<(), Error> {
        self.shared.send(&Message::new(
            wire::CANCEL_ORDER,
            json!({
                "ServerOrderID": order.server_id.clone().unwrap_or_default(),
                "ClientOrderID": order.client_id.clone().unwrap_or_default(),
                "TradeAccount": order.account,
            }),
        ))
    }

    /// Move the order `id` names to `price`, or change its quantity to `qty`,
    /// with CANCEL_REPLACE_ORDER, and wait until the server says it is done.
    /// The price is `Price1`: a limit's limit, a stop's trigger, a
    /// stop-limit's trigger — Sierra moves a stop-limit's limit with it, the
    /// same distance away, and changes nothing else of one.
    pub fn modify(&self, id: &str, price: Option<f64>, qty: Option<u32>) -> Result<(), Error> {
        let (order, since) = self.find(id)?;
        if !order.status.working() {
            return Err(Error::Rejected(format!("order {id} is no longer working")));
        }
        if price.is_none() && qty.is_none() {
            return Ok(());
        }
        if price.is_some() && order.kind == OrderKind::Market {
            return Err(Error::Rejected("a market order has no price to move".into()));
        }
        if qty == Some(0) {
            return Err(Error::Rejected("an order needs a quantity".into()));
        }
        let multiplier = match price {
            None => 1.0,
            Some(_) => self
                .shared
                .multiplier(&order.symbol)
                .ok_or_else(|| Error::Rejected(format!("Sierra Chart did not say how {} is priced", order.symbol)))?,
        };
        self.shared.send(&Message::new(
            wire::CANCEL_REPLACE_ORDER,
            json!({
                "ServerOrderID": order.server_id.clone().unwrap_or_default(),
                "ClientOrderID": order.client_id.clone().unwrap_or_default(),
                "TradeAccount": order.account,
                "Price1": price.map(|price| wire_price(price, multiplier)).unwrap_or(0.0),
                "Price1IsSet": u8::from(price.is_some()),
                "Price2": 0.0,
                "Price2IsSet": 0,
                "Quantity": qty.unwrap_or(order.qty),
            }),
        ))?;
        self.answered(id, since, &[wire::ORDER_CANCEL_REPLACE_COMPLETE], &[wire::ORDER_CANCEL_REPLACE_REJECTED], |status| {
            !status.working()
        })
    }

    /// Cancel every order working on `account`, each with its own
    /// CANCEL_ORDER, and wait for them all: the first refusal, if any.
    pub fn cancel_all(&self, account: &str) -> Result<(), Error> {
        self.cancel_where(|order| order.account == account)
    }

    fn cancel_where(&self, which: impl Fn(&Order) -> bool) -> Result<(), Error> {
        let working: Vec<(Order, u64)> = {
            let state = lock(&self.shared.state);
            state
                .orders
                .iter()
                .zip(&state.told)
                .filter(|(order, _)| order.status.working() && which(order))
                .map(|(order, told)| (order.clone(), told.0))
                .collect()
        };
        for (order, _) in &working {
            self.send_cancel(order)?;
        }
        let mut first = None;
        for (order, since) in &working {
            let outcome =
                self.answered(order.id(), *since, &[wire::ORDER_CANCELED], &[wire::ORDER_CANCEL_REJECTED], |status| !status.working());
            if let Err(error) = outcome {
                first.get_or_insert(error);
            }
        }
        first.map_or(Ok(()), Err)
    }

    /// Close `account`'s position in `symbol`: its working orders there are
    /// cancelled first, then SUBMIT_FLATTEN_POSITION_ORDER has the server
    /// close what is held at the market. Waits until the server has taken
    /// the flatten, or refused it.
    pub fn flatten(&self, account: &str, symbol: &str) -> Result<(), Error> {
        self.cancel_where(|order| order.account == account && order.symbol == symbol)?;
        let held = lock(&self.shared.state)
            .positions
            .iter()
            .find(|position| position.account == account && position.symbol == symbol)
            .map(|position| position.qty)
            .unwrap_or(0);
        if held == 0 {
            return Ok(());
        }
        let id = self.shared.next_client_id();
        self.shared.send(&Message::new(
            wire::SUBMIT_FLATTEN_POSITION_ORDER,
            json!({ "Symbol": symbol, "Exchange": "", "TradeAccount": account, "ClientOrderID": id, "FreeFormText": "", "IsAutomatedOrder": 0 }),
        ))?;
        let status = self.shared.wait_for(self.shared.timing.request, |state| {
            let at = state.order_at(&id)?;
            let order = &state.orders[at];
            (order.status != Status::Pending).then(|| order.status.clone())
        })?;
        match status {
            Status::Rejected(why) => Err(Error::Rejected(why)),
            _ => Ok(()),
        }
    }

    /// Ask again for every account's balance, positions and working orders,
    /// and wait for the answers.
    pub fn refresh(&self) -> Result<(), Error> {
        let accounts: Vec<String> = self.accounts().into_iter().map(|account| account.id).collect();
        self.shared.snapshot(&accounts, false)?;
        self.shared.ask_definitions();
        self.shared.told();
        Ok(())
    }

    /// `account`'s fills from `from` to `to`, unix seconds, oldest first:
    /// HISTORICAL_ORDER_FILLS_REQUEST, which takes a start and a number of
    /// days back; what is past `to` is left out here. Not added to the book.
    pub fn history(&self, account: &str, from: i64, to: i64) -> Result<Vec<Fill>, Error> {
        let days = ((now() - from).max(0) / 86_400 + 1).max(1);
        let answers = self.shared.ask(
            wire::HISTORICAL_ORDER_FILLS_REQUEST,
            json!({ "TradeAccount": account, "NumberOfDays": days, "StartDateTime": from, "ServerOrderID": "" }),
            false,
        )?;
        let state = lock(&self.shared.state);
        let mut fills: Vec<Fill> = answers
            .iter()
            .filter_map(|message| state.history_fill(message))
            .filter(|fill| fill.at >= from && (to <= 0 || fill.at <= to))
            .collect();
        drop(state);
        for symbol in fills.iter().map(|fill| fill.symbol.clone()).collect::<HashSet<_>>() {
            if let Some(multiplier) = self.shared.multiplier(&symbol) {
                for fill in fills.iter_mut().filter(|fill| fill.symbol == symbol) {
                    fill.price = real(fill.price, multiplier);
                }
            }
        }
        fills.sort_by_key(|fill| fill.at);
        Ok(fills)
    }

    /// `symbol`'s definition, over this session's connection.
    pub fn security_definition(&self, symbol: &str) -> Result<Definition, Error> {
        let answers = self.shared.ask(wire::SECURITY_DEFINITION_FOR_SYMBOL_REQUEST, json!({ "Symbol": symbol, "Exchange": "" }), true)?;
        answers
            .iter()
            .find_map(Definition::from_message)
            .ok_or_else(|| Error::Rejected(format!("Sierra Chart has no contract named {symbol}")))
    }

    /// Log off and close, and wait for the session's threads: a tick or two,
    /// never more than about a second.
    pub fn sign_out(&self) {
        self.shared.stop.store(true, Ordering::SeqCst);
        if let Some(sender) = lock(&self.shared.sender).take() {
            link::log_off(&sender, "signed out");
        }
        self.shared.changed.notify_all();
        let threads: Vec<_> = lock(&self.shared.threads).drain(..).collect();
        let current = std::thread::current().id();
        for thread in threads {
            if thread.thread().id() != current {
                let _ = thread.join();
            }
        }
    }
}

/// Open a connection that will trade: one whose server said trading is
/// supported.
fn open_trading(endpoint: &Endpoint, client_name: &str, timing: Timing) -> Result<Link, Error> {
    let link = link::open(endpoint, client_name, timing.heartbeat, timing.connect).map_err(Error::from)?;
    if !link.server.trading {
        link::log_off(&link.sender, "trading is not supported");
        return Err(Error::Refused(NO_TRADING.into()));
    }
    Ok(link)
}

/// The session's thread: read until the connection ends, then reconnect
/// unless it may not.
fn run(shared: &Arc<Shared>, first: Link) {
    let mut link = Some(first);
    let mut wait = shared.timing.backoff;
    loop {
        if let Some(open) = link.take() {
            let ended = read(shared, open);
            *lock(&shared.sender) = None;
            {
                let mut state = lock(&shared.state);
                state.connected = false;
                state.synced = false;
                for slot in state.requests.values_mut() {
                    slot.done = true;
                    slot.rejected.get_or_insert_with(|| "the connection dropped".into());
                }
            }
            shared.changed.notify_all();
            match ended {
                Ended::Stopped => return,
                Ended::Final(error) => {
                    *lock(&shared.trouble) = Some(error.clone());
                    (shared.pulse)(Pulse::Lost(error));
                    return;
                }
                Ended::Dropped(error) => {
                    *lock(&shared.trouble) = Some(error.clone());
                    (shared.pulse)(Pulse::Lost(error));
                }
            }
            wait = shared.timing.backoff;
        }
        if pause(shared, wait) {
            return;
        }
        match open_trading(&shared.endpoint, &shared.client_name, shared.timing) {
            Ok(open) => {
                *lock(&shared.server) = open.server.clone();
                *lock(&shared.sender) = Some(open.sender.clone());
                lock(&shared.state).connected = true;
                let resync = shared.clone();
                let spawned = std::thread::Builder::new().name("dtc-resync".into()).spawn(move || {
                    if resync.sync().is_ok() && !resync.stopped() {
                        *lock(&resync.trouble) = None;
                        (resync.pulse)(Pulse::Back);
                    }
                });
                if let Ok(thread) = spawned {
                    lock(&shared.threads).push(thread);
                }
                link = Some(open);
            }
            Err(error @ Error::Refused(_)) => {
                *lock(&shared.trouble) = Some(error.clone());
                (shared.pulse)(Pulse::Lost(error));
                return;
            }
            Err(error) => {
                *lock(&shared.trouble) = Some(error);
                wait = (wait * 2).min(shared.timing.backoff_cap);
            }
        }
    }
}

/// Wait `wait` before reconnecting, or until signed out: true when signed
/// out.
fn pause(shared: &Shared, wait: Duration) -> bool {
    let deadline = Instant::now() + wait;
    let mut state = lock(&shared.state);
    loop {
        if shared.stopped() {
            return true;
        }
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return false;
        }
        state = shared.changed.wait_timeout(state, left).unwrap_or_else(|poisoned| poisoned.into_inner()).0;
    }
}

/// Read one connection until it ends, sending a heartbeat whenever one is
/// due and folding every message into the state.
fn read(shared: &Arc<Shared>, mut link: Link) -> Ended {
    let interval = shared.timing.heartbeat;
    let silence = interval * 2;
    let _ = link.reader.set_read_timeout(Some(TICK));
    let mut heard = Instant::now();
    let mut beat = Instant::now();
    let mut pending: Vec<Message> = std::mem::take(&mut link.queued);
    let mut buffer = vec![0u8; 64 * 1024];
    loop {
        for message in pending.drain(..) {
            if let Some(ended) = take(shared, &message) {
                link.sender.shut();
                return ended;
            }
        }
        if shared.stopped() {
            return Ended::Stopped;
        }
        if beat.elapsed() >= interval {
            beat = Instant::now();
            let heartbeat = Message::new(wire::HEARTBEAT, json!({ "NumDroppedMessages": 0, "CurrentDateTime": now() }));
            if link.sender.send(&heartbeat).is_err() && !shared.stopped() {
                return Ended::Dropped(Error::Unreachable("the connection to Sierra Chart dropped".into()));
            }
        }
        if heard.elapsed() > silence {
            link.sender.shut();
            return Ended::Dropped(Error::Unreachable(format!(
                "Sierra Chart stopped answering: nothing for {} seconds",
                silence.as_secs()
            )));
        }
        match link.reader.read(&mut buffer) {
            Ok(0) => {
                if shared.stopped() {
                    return Ended::Stopped;
                }
                return Ended::Dropped(Error::Unreachable("Sierra Chart closed the connection".into()));
            }
            Ok(read) => {
                heard = Instant::now();
                match link.framer.push(&buffer[..read]) {
                    Ok(messages) => pending = messages,
                    Err(why) => {
                        link.sender.shut();
                        return Ended::Dropped(Error::Protocol(why));
                    }
                }
            }
            Err(error) if matches!(error.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut | io::ErrorKind::Interrupted) => {}
            Err(_) if shared.stopped() => return Ended::Stopped,
            Err(error) => return Ended::Dropped(Error::Unreachable(format!("the connection to Sierra Chart dropped ({error})"))),
        }
    }
}

/// Take in one message: into a waiting request's answers, into the book, or
/// as the end of the connection.
fn take(shared: &Arc<Shared>, message: &Message) -> Option<Ended> {
    let kind = message.kind();
    if kind == wire::LOGOFF {
        if shared.stopped() {
            return Some(Ended::Stopped);
        }
        let reason = message.text("Reason").unwrap_or_else(|| "Sierra Chart logged the session off".into());
        return Some(if message.flag("DoNotReconnect") {
            Ended::Final(Error::Refused(format!("Sierra Chart ended the session: {reason}")))
        } else {
            Ended::Dropped(Error::Unreachable(reason))
        });
    }
    if kind == wire::HEARTBEAT {
        return None;
    }
    let rejects = [
        wire::OPEN_ORDERS_REJECT,
        wire::CURRENT_POSITIONS_REJECT,
        wire::HISTORICAL_ORDER_FILLS_REJECT,
        wire::ACCOUNT_BALANCE_REJECT,
        wire::SECURITY_DEFINITION_REJECT,
    ];
    let changed = {
        let mut state = lock(&shared.state);
        let id = message.int("RequestID").filter(|id| *id != 0);
        let fold = match id.and_then(|id| state.requests.get_mut(&id)) {
            Some(slot) => {
                if rejects.contains(&kind) {
                    slot.rejected = Some(message.text("RejectText").unwrap_or_else(|| "the server refused the request".into()));
                    slot.done = true;
                } else {
                    slot.done = message.final_in_batch();
                    slot.answers.push(message.clone());
                }
                slot.fold
            }
            None => true,
        };
        fold && state.fold(message)
    };
    if changed {
        if matches!(kind, wire::ORDER_UPDATE | wire::POSITION_UPDATE | wire::HISTORICAL_ORDER_FILL_RESPONSE) {
            shared.ask_definitions();
        }
        shared.told();
    } else {
        shared.changed.notify_all();
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(value: Value) -> Message {
        Message(value.as_object().unwrap().clone())
    }

    #[test]
    fn a_client_order_id_fits_dtc_s_thirty_two_characters() {
        assert!(format!("oma{}", radix36(i64::MAX as u64)).len() < 32);
        assert_eq!(radix36(35), "z");
    }

    #[test]
    fn a_fill_sierra_sent_in_hundredths_is_a_real_price_in_the_book() {
        let mut state = State::default();
        // The definition, as Sierra writes its 32-bit 0.01.
        state.fold(&message(serde_json::json!({
            "Type":507,"RequestID":9,"SecurityType":1,"MinPriceIncrement":0.25,"IsFinalMessage":1,
            "DisplayPriceMultiplier":0.009999999776482582,"Symbol":"MESZ26_FUT_CME"
        })));
        state.fold(&message(serde_json::json!({
            "Type":304,"RequestID":41,"TotalNumberMessages":72,"MessageNumber":1,"BuySell":1,"Price":765125,
            "DateTime":1793629945,"Quantity":1,"OpenClose":1,"NoOrderFills":0,"Symbol":"MESZ26_FUT_CME",
            "Exchange":"","ServerOrderID":"0","UniqueExecutionID":"6884383774554","TradeAccount":"LIVE1"
        })));
        let book = state.book();
        assert_eq!(book.fills.len(), 1);
        assert_eq!(book.fills[0].price, 7651.25);
        assert_eq!(book.fills[0].server_id, None);
    }

    #[test]
    fn a_real_price_goes_out_in_the_server_s_hundredths() {
        assert_eq!(wire_price(7651.25, 0.01), 765125.0);
        assert_eq!(real(765125.0, 0.01), 7651.25);
        assert_eq!(wire_price(7651.25, 1.0), 7651.25);
    }

    #[test]
    fn the_empty_snapshots_sierra_sends_change_nothing() {
        let mut state = State::default();
        assert!(!state.fold(&message(serde_json::json!({"Type":306,"RequestID":20,"TotalNumberMessages":1,"MessageNumber":1,"Symbol":"","NoPositions":1,"TradeAccount":"Sim1"}))));
        assert!(!state.fold(&message(serde_json::json!({"Type":301,"RequestID":30,"TotalNumMessages":1,"MessageNumber":1,"OrderStatus":0,"Price1":null,"NoOrders":1}))));
        assert!(!state.fold(&message(serde_json::json!({"Type":600,"RequestID":10,"NoAccountBalances":1,"TradeAccount":""}))));
        assert!(!state.fold(&message(serde_json::json!({"Type":304,"RequestID":40,"NoOrderFills":1,"TradeAccount":"Sim1","InfoText":"[final]"}))));
        assert_eq!(state.book(), Book::default());
    }
}
