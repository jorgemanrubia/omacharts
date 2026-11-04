//! The trading half of a login's connection: the order plant, which lists
//! the login's accounts, and the PnL plant, which pushes their balances.
//!
//! Lent by [`Connection::trading`], beside the data half
//! ([`Client`](crate::Client)) of the same login: they are different
//! plants, so they sit side by side on one connection, and what must not
//! happen — two of either for one login, because Rithmic signs the first
//! out when the same login signs in to a plant again — cannot, because a
//! login has one connection.
//!
//! The order plant also places, moves, links, cancels and flattens, and
//! pushes what becomes of every order; the PnL plant pushes positions and
//! balances. Both land in one [`Book`] the half keeps, which a caller reads
//! whenever a [`Pulse`] says something moved. Nothing here polls.
//!
//! Signed in once and kept until [`Trading::sign_out`]. A drop is
//! reconnected by the connection, as the data half's is, and orders are
//! refused meanwhile: an order is never sent again behind a trader's back.
//! Once back it says so ([`Pulse::Back`]), with the book asked for again.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rithmic_rs::rti::messages::RithmicMessage;
use rithmic_rs::rti::{
    AccountPnLPositionUpdate, ExchangeOrderNotification, InstrumentPnLPositionUpdate, ResponseAccountList,
    RithmicOrderNotification,
};
use rithmic_rs::{
    ConnectStrategy, ManualOrAutoEntry, OrderSide, OrderStatus, OrderType, RithmicAccount, RithmicCancelAllOrders,
    RithmicCancelOrder, RithmicExitPosition, RithmicLinkOrders, RithmicModifyOrder, RithmicOrder, RithmicOrderPlant,
    RithmicOrderPlantHandle, RithmicPnlPlant, RithmicPnlPlantHandle, RithmicResponse, TimeInForce,
};
use tokio::sync::{broadcast, oneshot, watch};

use crate::client::{config, error_of, signed_in, within};
use crate::connection::{Connection, Inner, Opened, Phase};
use crate::{App, Credentials, Error, Side};

/// How long one request may take.
const REQUEST: Duration = Duration::from_secs(15);

/// One account inside a login, as the order plant lists it.
#[derive(Clone, PartialEq)]
pub struct Account {
    /// The account number, such as "951".
    pub id: String,
    /// The name the firm gave it, when it gave one.
    pub name: Option<String>,
    /// Its currency, such as "USD".
    pub currency: Option<String>,
    /// The futures commission merchant and introducing broker it is under,
    /// which an order names beside the account.
    pub fcm: String,
    pub ib: String,
}

impl std::fmt::Debug for Account {
    /// Without the number: it is somebody's account, and a log is no place
    /// for it.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Account").field("currency", &self.currency).finish_non_exhaustive()
    }
}

/// How an order is priced. A stop is a stop-market: it becomes a market
/// order when its price trades.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Market,
    Limit,
    Stop,
}

/// An order to place: `qty` of `contract` on `exchange`, for the account
/// `account`, good for the day, entered by hand.
#[derive(Clone, Debug, PartialEq)]
pub struct Order {
    pub account: String,
    pub contract: String,
    pub exchange: String,
    pub side: Side,
    pub qty: u32,
    pub kind: Kind,
    /// The limit or stop price; none at the market.
    pub price: Option<f64>,
    /// What the order is tagged with, to know it again in what comes back.
    pub tag: String,
}

/// Where an order stands, from what Rithmic last said of it.
#[derive(Clone, Debug, PartialEq)]
pub enum Status {
    /// Sent, and not yet working at the exchange.
    Pending,
    /// Working: resting at its price, or waiting for its stop.
    Working,
    /// Some filled, the rest still working.
    PartiallyFilled,
    Filled,
    Cancelled,
    /// Refused, in Rithmic's words.
    Rejected(String),
}

impl Status {
    /// Still to fill or be cancelled.
    pub fn working(&self) -> bool {
        matches!(self, Status::Pending | Status::Working | Status::PartiallyFilled)
    }
}

/// One order this session has heard of: placed here, placed elsewhere on
/// the same account, or working when it signed in.
#[derive(Clone, Debug, PartialEq)]
pub struct WorkingOrder {
    pub account: String,
    /// Rithmic's id for it, which cancelling names.
    pub basket_id: String,
    pub contract: String,
    pub exchange: String,
    pub side: Side,
    pub qty: u32,
    /// How much of it has filled so far.
    pub filled: u32,
    pub kind: Kind,
    pub price: Option<f64>,
    pub status: Status,
    /// The average price it filled at, once it has.
    pub fill_price: Option<f64>,
    /// When Rithmic last said anything of it, in unix seconds.
    pub at: i64,
    pub tag: Option<String>,
    /// What Rithmic last said of it in words, when it said anything.
    pub said: Option<String>,
}

/// A position in one contract, as the PnL plant pushed it.
#[derive(Clone, Debug, PartialEq)]
pub struct Position {
    pub account: String,
    pub contract: String,
    pub exchange: String,
    /// Contracts held: positive long, negative short.
    pub net: i32,
    pub avg: Option<f64>,
    /// Its profit or loss now, in the account's currency, as Rithmic
    /// worked it out.
    pub open_pnl: Option<f64>,
}

/// An account's money, as the PnL plant pushed it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AccountPnl {
    pub account: String,
    pub balance: Option<f64>,
    /// Closed today.
    pub realized_day: Option<f64>,
    /// Open now.
    pub unrealized: Option<f64>,
}

/// Everything a session knows of its accounts' trading.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Book {
    pub accounts: Vec<AccountPnl>,
    /// Only contracts held, flat ones left out.
    pub positions: Vec<Position>,
    /// Newest first.
    pub orders: Vec<WorkingOrder>,
}

/// What the listener keeps from the plants' pushes, and [`Trading::book`]
/// copies out.
#[derive(Default)]
pub struct State {
    accounts: Vec<AccountPnl>,
    positions: Vec<Position>,
    orders: Vec<WorkingOrder>,
}

fn number(text: Option<&str>) -> Option<f64> {
    text.and_then(|text| text.trim().parse::<f64>().ok())
}

fn side_of(code: Option<i32>) -> Option<Side> {
    match code {
        Some(1) => Some(Side::Buy),
        Some(2 | 3) => Some(Side::Sell),
        _ => None,
    }
}

fn kind_of(code: Option<i32>) -> Option<Kind> {
    match code {
        Some(1) => Some(Kind::Limit),
        Some(2) => Some(Kind::Market),
        Some(3 | 4) => Some(Kind::Stop),
        _ => None,
    }
}

/// Rithmic's reason, from the first of its texts that says anything.
fn reason(texts: [Option<&String>; 2]) -> Option<String> {
    texts.into_iter().flatten().map(|text| text.trim()).find(|text| !text.is_empty()).map(str::to_string)
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|since| since.as_secs() as i64)
        .unwrap_or_default()
}

impl State {
    pub fn book(&self) -> Book {
        Book {
            accounts: self.accounts.clone(),
            positions: self.positions.iter().filter(|position| position.net != 0).cloned().collect(),
            orders: self.orders.iter().rev().cloned().collect(),
        }
    }

    /// Signed in again after a drop: what is still working, and what is
    /// held, is listed afresh by the requests that follow. An order that
    /// finished meanwhile is not listed, and goes, rather than be shown
    /// working when it is not.
    fn signed_in_again(&mut self) {
        self.orders.retain(|order| !order.status.working());
        self.positions.clear();
    }

    fn order_mut(&mut self, basket: &str) -> Option<&mut WorkingOrder> {
        self.orders.iter_mut().find(|order| order.basket_id == basket)
    }

    fn account_mut(&mut self, account: &str) -> &mut AccountPnl {
        let at = match self.accounts.iter().position(|known| known.account == account) {
            Some(at) => at,
            None => {
                self.accounts.push(AccountPnl { account: account.to_string(), ..AccountPnl::default() });
                self.accounts.len() - 1
            }
        };
        &mut self.accounts[at]
    }

    /// Note an order just placed here, before Rithmic says anything of it.
    pub fn placed(&mut self, order: &Order, basket: &str, at: i64) {
        if self.order_mut(basket).is_some() {
            return;
        }
        self.orders.push(WorkingOrder {
            account: order.account.clone(),
            basket_id: basket.to_string(),
            contract: order.contract.clone(),
            exchange: order.exchange.clone(),
            side: order.side,
            qty: order.qty,
            filled: 0,
            kind: order.kind,
            price: order.price,
            status: Status::Pending,
            fill_price: None,
            at,
            tag: Some(order.tag.clone()),
            said: None,
        });
    }

    /// Take one push from either plant. True when the book changed.
    pub fn apply(&mut self, message: &RithmicMessage) -> bool {
        match message {
            RithmicMessage::RithmicOrderNotification(update) => self.rithmic_order(update),
            RithmicMessage::ExchangeOrderNotification(update) => self.exchange_order(update),
            RithmicMessage::InstrumentPnLPositionUpdate(update) => self.instrument(update),
            RithmicMessage::AccountPnLPositionUpdate(update) => self.account(update),
            _ => false,
        }
    }

    fn rithmic_order(&mut self, update: &RithmicOrderNotification) -> bool {
        let Some(basket) = update.basket_id.as_deref().filter(|id| !id.is_empty()) else { return false };
        let before = self.order_mut(basket).cloned();
        let mut order = before.clone().unwrap_or_else(|| WorkingOrder {
            account: String::new(),
            basket_id: basket.to_string(),
            contract: String::new(),
            exchange: String::new(),
            side: Side::Buy,
            qty: 0,
            filled: 0,
            kind: Kind::Market,
            price: None,
            status: Status::Pending,
            fill_price: None,
            at: 0,
            tag: None,
            said: None,
        });
        let given = |text: &Option<String>| text.as_ref().filter(|text| !text.is_empty()).cloned();
        if let Some(account) = given(&update.account_id) {
            order.account = account;
        }
        if let Some(symbol) = given(&update.symbol) {
            order.contract = symbol;
        }
        if let Some(exchange) = given(&update.exchange) {
            order.exchange = exchange;
        }
        if let Some(side) = side_of(update.transaction_type) {
            order.side = side;
        }
        if let Some(quantity) = update.quantity.and_then(|qty| u32::try_from(qty).ok()).filter(|qty| *qty > 0) {
            order.qty = quantity;
        }
        if let Some(kind) = kind_of(update.price_type) {
            order.kind = kind;
        }
        let price = match order.kind {
            Kind::Stop => update.trigger_price.or(update.price),
            Kind::Limit => update.price,
            Kind::Market => None,
        };
        if price.is_some_and(|price| price != 0.0) {
            order.price = price;
        }
        if let Some(fill) = update.avg_fill_price.filter(|price| *price != 0.0) {
            order.fill_price = Some(fill);
        }
        if let Some(at) = update.ssboe {
            order.at = i64::from(at);
        }
        if let Some(said) = reason([update.report_text.as_ref(), update.text.as_ref()]) {
            order.said = Some(said);
        }
        if let Some(tag) = given(&update.user_tag) {
            order.tag = Some(tag);
        }
        if let Some(done) = update.total_fill_size.and_then(|size| u32::try_from(size).ok()).filter(|size| *size > 0) {
            order.filled = done;
        }
        let filled = update.total_fill_size.unwrap_or(0) > 0;
        // The exchange's notification carries the reason and comes first;
        // Rithmic's completion after it says only "R". The reason stands.
        let refused = |known: &Status| match (known, reason([update.report_text.as_ref(), update.text.as_ref()])) {
            (Status::Rejected(said), None) => Status::Rejected(said.clone()),
            (_, said) => Status::Rejected(said.unwrap_or_else(|| "no reason given".into())),
        };
        if let Some(status) = update.status.as_deref() {
            let status: OrderStatus = status.parse().unwrap_or_default();
            let completion = update.completion_reason.as_deref().unwrap_or("").trim();
            order.status = match status {
                OrderStatus::Rejected => refused(&order.status),
                OrderStatus::Cancelled => Status::Cancelled,
                OrderStatus::Complete | OrderStatus::Expired => match completion {
                    "F" | "Fill" | "Filled" => Status::Filled,
                    "C" | "Cancel" | "Canceled" | "Cancelled" => Status::Cancelled,
                    _ if filled => Status::Filled,
                    "R" | "Reject" | "Rejected" => refused(&order.status),
                    _ => match order.status {
                        Status::Rejected(_) => order.status.clone(),
                        _ => Status::Cancelled,
                    },
                },
                OrderStatus::Unknown => order.status.clone(),
                // Pending again once working is a modify or a cancel on its
                // way: still working.
                OrderStatus::OpenPending | OrderStatus::Pending if order.status == Status::Pending => Status::Pending,
                _ if order.status.working() && filled => Status::PartiallyFilled,
                _ if order.status.working() => Status::Working,
                _ => order.status.clone(),
            };
        }
        let changed = before.as_ref() != Some(&order);
        match self.order_mut(basket) {
            Some(known) => *known = order,
            None => self.orders.push(order),
        }
        changed
    }

    fn exchange_order(&mut self, update: &ExchangeOrderNotification) -> bool {
        let Some(basket) = update.basket_id.as_deref().filter(|id| !id.is_empty()) else { return false };
        let Some(order) = self.order_mut(basket) else { return false };
        let before = order.clone();
        if let Some(said) = reason([update.report_text.as_ref(), update.text.as_ref()]) {
            order.said = Some(said);
        }
        match update.report_type.as_deref().map(str::to_ascii_lowercase).as_deref() {
            Some("fill") => {
                if let Some(price) = update.avg_fill_price.or(update.fill_price).filter(|price| *price != 0.0) {
                    order.fill_price = Some(price);
                }
                if let Some(left) = update.total_unfilled_size.and_then(|left| u32::try_from(left).ok()) {
                    order.filled = order.qty.saturating_sub(left);
                }
                match update.total_unfilled_size {
                    Some(0) => order.status = Status::Filled,
                    Some(_) if order.status.working() => order.status = Status::PartiallyFilled,
                    _ => {}
                }
            }
            Some("reject") => {
                let said = reason([update.report_text.as_ref(), update.text.as_ref()]);
                order.status = Status::Rejected(said.unwrap_or_else(|| "no reason given".into()));
            }
            Some("cancel") => order.status = Status::Cancelled,
            Some("status") if order.status == Status::Pending => order.status = Status::Working,
            _ => {}
        }
        *order != before
    }

    fn instrument(&mut self, update: &InstrumentPnLPositionUpdate) -> bool {
        let (Some(account), Some(symbol)) = (update.account_id.as_deref(), update.symbol.as_deref()) else {
            return false;
        };
        let net = update.net_quantity.or_else(|| Some(update.buy_qty? - update.sell_qty?)).unwrap_or(0);
        let fresh = Position {
            account: account.to_string(),
            contract: symbol.to_string(),
            exchange: update.exchange.clone().unwrap_or_default(),
            net,
            avg: update.avg_open_fill_price.filter(|price| *price != 0.0 && net != 0),
            open_pnl: match net {
                0 => None,
                _ => update.day_open_pnl.or_else(|| number(update.open_position_pnl.as_deref())),
            },
        };
        match self.positions.iter_mut().find(|known| known.account == account && known.contract == symbol) {
            Some(known) if *known == fresh => false,
            Some(known) => {
                let shown = known.net != 0 || net != 0;
                *known = fresh;
                shown
            }
            None => {
                let shown = net != 0;
                self.positions.push(fresh);
                shown
            }
        }
    }

    fn account(&mut self, update: &AccountPnLPositionUpdate) -> bool {
        let Some(account) = update.account_id.as_deref() else { return false };
        let known = self.account_mut(account);
        let before = known.clone();
        if let Some(balance) = number(update.account_balance.as_deref()) {
            known.balance = Some(balance);
        }
        if let Some(closed) = number(update.day_closed_pnl.as_deref()).or(number(update.closed_position_pnl.as_deref()))
        {
            known.realized_day = Some(closed);
        }
        if let Some(open) = number(update.day_open_pnl.as_deref()).or(number(update.open_position_pnl.as_deref())) {
            known.unrealized = Some(open);
        }
        *known != before
    }
}

/// What the trading half tells whoever opened it, on the connection's
/// thread.
#[derive(Clone, Debug, PartialEq)]
pub enum Pulse {
    /// An order, position or balance moved: read the book again.
    Moved,
    /// The half dropped, and is reconnecting by itself unless the error says
    /// it cannot ([`Error::Refused`], [`Error::NoLogin`]). Orders are
    /// refused until it is back.
    Lost(Error),
    /// Signed in again after a drop. The accounts were listed again, and
    /// their working orders, positions and balances asked for afresh, so an
    /// order that finished while it was down is gone from the book rather
    /// than shown working: the cue to reconcile what was being waited for.
    Back,
}

pub(crate) type PulseSink = Arc<dyn Fn(Pulse) + Send + Sync>;

/// The order and PnL plants' handles, for requests from any thread.
pub(crate) struct Desks {
    pub(crate) order: RithmicOrderPlantHandle,
    pub(crate) pnl: RithmicPnlPlantHandle,
    /// One order handle per account, by number: an order names its account
    /// through the handle it is sent on.
    by_account: HashMap<String, RithmicOrderPlantHandle>,
}

#[derive(Clone)]
pub(crate) enum Standing {
    Connecting,
    /// Signed in. No desks only in a test, whose plants are fakes.
    Up(Option<Arc<Desks>>),
    /// Lost, and reconnecting by itself.
    Down(Error),
    /// Will not try again by itself.
    Stopped(Error),
}

/// A login's trading: the order and PnL plants, lent by
/// [`Connection::trading`](crate::Connection::trading). Cheap to clone;
/// every clone is the same half, until it is signed out: a handle from
/// before stays signed out.
#[derive(Clone)]
pub struct Trading {
    inner: Arc<Inner>,
    generation: u64,
}

impl Trading {
    pub(crate) fn of(inner: Arc<Inner>, generation: u64) -> Trading {
        Trading { inner, generation }
    }

    /// The login's accounts, as the order plant listed them at the last
    /// sign-in.
    pub fn accounts(&self) -> Vec<Account> {
        self.inner.accounts.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// The balance of the account `id`, once the PnL plant has said.
    pub fn balance(&self, id: &str) -> Option<f64> {
        let state = self.inner.book.lock().unwrap_or_else(|e| e.into_inner());
        state.accounts.iter().find(|known| known.account == id).and_then(|known| known.balance)
    }

    /// Every order, position and balance the half has heard of.
    pub fn book(&self) -> Book {
        self.inner.book.lock().unwrap_or_else(|e| e.into_inner()).book()
    }

    /// Why the half is not signed in now: dropped and coming back, signed
    /// out from elsewhere, or signed out here.
    pub fn trouble(&self) -> Option<Error> {
        if !self.inner.trading_open(self.generation) {
            return Some(Error::Closed);
        }
        match &*self.inner.trading.borrow() {
            Standing::Down(error) | Standing::Stopped(error) => Some(error.clone()),
            Standing::Up(_) | Standing::Connecting => None,
        }
    }

    /// The plants' handles, while the half is signed in.
    ///
    /// Not signed in, nothing is sent, which is a refusal and not a request
    /// left unanswered: said as one.
    fn desks(&self) -> Result<Arc<Desks>, Error> {
        if let Some(trouble) = self.trouble() {
            return Err(Error::Rejected(trouble.to_string()));
        }
        match &*self.inner.trading.borrow() {
            Standing::Up(Some(desks)) => Ok(desks.clone()),
            _ => Err(Error::Rejected("the trading session is still signing in".into())),
        }
    }

    /// Run `request` on the order handle of `account`.
    fn on_desk<T>(
        &self,
        account: &str,
        request: impl AsyncFnOnce(&RithmicOrderPlantHandle) -> Result<T, Error>,
    ) -> Result<T, Error> {
        let desks = self.desks()?;
        let desk = desks.by_account.get(account).ok_or_else(|| Error::NotFound("such account on this login".into()))?;
        self.inner.handle.block_on(request(desk))
    }

    /// Place `order`, blocking until Rithmic has taken it, and return its
    /// basket id. Taken is not filled: what becomes of it arrives on the
    /// book. Never sent twice: a drop on the way is an error, not a retry.
    pub fn place(&self, order: &Order) -> Result<String, Error> {
        let side = match order.side {
            Side::Buy => OrderSide::Buy,
            Side::Sell => OrderSide::Sell,
        };
        let built = RithmicOrder::new()
            .symbol(&order.contract)
            .exchange(&order.exchange)
            .quantity(i32::try_from(order.qty).unwrap_or(1))
            .transaction_type(side)
            .duration(TimeInForce::Day)
            .manual_or_auto(ManualOrAutoEntry::Manual)
            .user_tag(&order.tag);
        let built = match (order.kind, order.price) {
            (Kind::Market, _) => built.price_type(OrderType::Market),
            (Kind::Limit, Some(price)) => built.price_type(OrderType::Limit).price(price),
            (Kind::Stop, Some(price)) => built.price_type(OrderType::StopMarket).trigger_price(price),
            (_, None) => return Err(Error::Rejected("a limit or stop order needs a price".into())),
        };
        let built = built.build().map_err(error_of)?;
        let replies = self.on_desk(&order.account, async |desk| within(REQUEST, desk.place_order(built)).await)?;
        if let Some(error) = replies.iter().find_map(|reply| reply.error.clone()) {
            return Err(error_of(error));
        }
        let basket = replies
            .iter()
            .find_map(|reply| match &reply.message {
                RithmicMessage::ResponseNewOrder(placed) => placed.basket_id.clone().filter(|id| !id.is_empty()),
                _ => None,
            })
            .ok_or_else(|| Error::Rejected("Rithmic took the order without naming it".into()))?;
        self.inner.book.lock().unwrap_or_else(|e| e.into_inner()).placed(order, &basket, now());
        Ok(basket)
    }

    /// Cancel the order `basket` on `account`. What becomes of it arrives
    /// on the book.
    pub fn cancel(&self, account: &str, basket: &str) -> Result<(), Error> {
        let cancel =
            RithmicCancelOrder::new().id(basket).manual_or_auto(ManualOrAutoEntry::Manual).build().map_err(error_of)?;
        let replies = self.on_desk(account, async |desk| within(REQUEST, desk.cancel_order(cancel)).await)?;
        match replies.iter().find_map(|reply| reply.error.clone()) {
            Some(error) => Err(error_of(error)),
            None => Ok(()),
        }
    }

    /// Move the working order `basket` on `account` to `price`, blocking
    /// until Rithmic has taken the request. A modify restates the order, so
    /// everything else is carried over from what the book has of it. What
    /// becomes of it arrives on the book.
    pub fn modify(&self, account: &str, basket: &str, price: f64) -> Result<(), Error> {
        self.desks()?;
        let known = self.inner.book.lock().unwrap_or_else(|e| e.into_inner()).order_mut(basket).cloned();
        let order = known
            .filter(|order| order.status.working())
            .ok_or_else(|| Error::NotFound("such working order".into()))?;
        let modify = RithmicModifyOrder::new()
            .id(basket)
            .symbol(&order.contract)
            .exchange(&order.exchange)
            .quantity(i32::try_from(order.qty).unwrap_or(1))
            .manual_or_auto(ManualOrAutoEntry::Manual);
        let modify = match order.kind {
            Kind::Limit => modify.price_type(OrderType::Limit).price(price),
            Kind::Stop => modify.price_type(OrderType::StopMarket).trigger_price(price),
            Kind::Market => return Err(Error::Rejected("a market order has no price to move".into())),
        };
        let modify = modify.build().map_err(error_of)?;
        let replies = self.on_desk(account, async |desk| within(REQUEST, desk.modify_order(modify)).await)?;
        match replies.iter().find_map(|reply| reply.error.clone()) {
            Some(error) => Err(error_of(error)),
            None => Ok(()),
        }
    }

    /// Link working orders on `account` into one group, held on Rithmic's
    /// servers: when one of them fills or is cancelled, Rithmic cancels the
    /// rest. A stop and a target on one position, so that whichever goes
    /// first takes the other with it.
    pub fn link(&self, account: &str, baskets: &[String]) -> Result<(), Error> {
        let link = RithmicLinkOrders::new().basket_ids(baskets.iter().cloned()).build().map_err(error_of)?;
        let reply = self.on_desk(account, async |desk| within(REQUEST, desk.link_orders(link)).await)?;
        match reply.error {
            Some(error) => Err(error_of(error)),
            None => Ok(()),
        }
    }

    /// Cancel every working order on `account`.
    pub fn cancel_all(&self, account: &str) -> Result<(), Error> {
        let cancel = RithmicCancelAllOrders::new().manual_or_auto(ManualOrAutoEntry::Manual).build().map_err(error_of)?;
        self.on_desk(account, async |desk| within(REQUEST, desk.cancel_all_orders(cancel)).await).map(|_| ())
    }

    /// Close `account`'s position in `contract`: one request, and Rithmic
    /// places the closing order itself.
    pub fn flatten(&self, account: &str, contract: &str, exchange: &str) -> Result<(), Error> {
        let exit = RithmicExitPosition::new()
            .symbol(contract)
            .exchange(exchange)
            .manual_or_auto(ManualOrAutoEntry::Manual)
            .build()
            .map_err(error_of)?;
        let replies = self.on_desk(account, async |desk| within(REQUEST, desk.exit_position(exit)).await)?;
        match replies.iter().find_map(|reply| reply.error.clone()) {
            Some(error) => Err(error_of(error)),
            None => Ok(()),
        }
    }

    /// Sign out of both plants, waiting a few seconds at most. The data
    /// half of the same login, if it is open, stays.
    pub fn sign_out(&self) {
        Connection::of(self.inner.clone()).close_trading_as(self.generation);
    }

    /// Cut the login's plants the way a dropped network does, for proving
    /// that trading comes back. Nothing in the app calls it.
    #[doc(hidden)]
    pub fn sever(&self) {
        Connection::of(self.inner.clone()).sever();
    }
}

fn rithmic_account(account: &Account) -> RithmicAccount {
    RithmicAccount::new(&account.fcm, &account.ib, &account.id)
}

/// The trading plants of a signed-in trading half: what the supervisor
/// hears from them, and what is closed with them.
pub(crate) struct TradingPlants {
    /// Every account's pushes, and the plants' health.
    pub(crate) order_updates: broadcast::Receiver<RithmicResponse>,
    pub(crate) pnl_updates: broadcast::Receiver<RithmicResponse>,
    accounts: Vec<Account>,
    link: TradingLink,
}

enum TradingLink {
    Rithmic { order_plant: RithmicOrderPlant, pnl_plant: RithmicPnlPlant, desks: Arc<Desks> },
    /// A test's, holding its channels open. Never made outside one.
    #[cfg_attr(not(test), allow(dead_code))]
    Fake(#[allow(dead_code)] Vec<broadcast::Sender<RithmicResponse>>),
}

impl TradingPlants {
    #[cfg(test)]
    pub(crate) fn fake(order_updates: broadcast::Receiver<RithmicResponse>, accounts: Vec<Account>) -> TradingPlants {
        let (pnl, pnl_updates) = broadcast::channel(16);
        TradingPlants { order_updates, pnl_updates, accounts, link: TradingLink::Fake(vec![pnl]) }
    }

    fn desks(&self) -> Option<Arc<Desks>> {
        match &self.link {
            TradingLink::Rithmic { desks, .. } => Some(desks.clone()),
            TradingLink::Fake(_) => None,
        }
    }

    /// Cut the plants from underneath, as a network failure would.
    pub(crate) fn abort(&self) {
        if let TradingLink::Rithmic { desks, .. } = &self.link {
            desks.order.abort();
            desks.pnl.abort();
        }
    }

    /// Sign out, or with `graceful` false only cut the plants, and wait for
    /// them to stop.
    pub(crate) async fn close(self, graceful: bool) {
        let TradingLink::Rithmic { order_plant, pnl_plant, desks } = self.link else { return };
        if graceful {
            let _ = within(Duration::from_secs(2), desks.order.disconnect()).await;
            let _ = within(Duration::from_secs(2), desks.pnl.disconnect()).await;
        }
        desks.order.abort();
        desks.pnl.abort();
        let _ = tokio::time::timeout(Duration::from_secs(3), order_plant.await_shutdown()).await;
        let _ = tokio::time::timeout(Duration::from_secs(3), pnl_plant.await_shutdown()).await;
    }
}

/// Sign in to the order and PnL plants, list the login's accounts, and ask
/// for their working orders, positions and balances.
pub(crate) async fn connect(app: App, credentials: &Credentials) -> Result<TradingPlants, Error> {
    let config = config(app, credentials)?;

    // No account is known before the list, and the login and the list are
    // the plant's, not an account's.
    let nobody = RithmicAccount::new("", "", "");
    let order_plant = within(REQUEST, RithmicOrderPlant::connect(&config, ConnectStrategy::Simple)).await?;
    let order = order_plant.get_handle(&nobody);
    if let Err(error) = signed_in(within(REQUEST, order.login()).await) {
        order.abort();
        return Err(error);
    }
    let listed = match within(REQUEST, order.get_account_list()).await {
        Ok(listed) => listed,
        Err(error) => {
            let _ = within(REQUEST, order.disconnect()).await;
            return Err(error);
        }
    };
    let accounts = match accounts_of(&listed) {
        Ok(accounts) => accounts,
        Err(error) => {
            let _ = within(REQUEST, order.disconnect()).await;
            return Err(error);
        }
    };
    let pnl_plant = match within(REQUEST, RithmicPnlPlant::connect(&config, ConnectStrategy::Simple)).await {
        Ok(plant) => plant,
        Err(error) => {
            let _ = within(REQUEST, order.disconnect()).await;
            return Err(error);
        }
    };
    let pnl = pnl_plant.get_handle(&nobody);
    if let Err(error) = signed_in(within(REQUEST, pnl.login()).await) {
        pnl.abort();
        let _ = within(REQUEST, order.disconnect()).await;
        return Err(error);
    }
    // Heard from before the requests, so that what they push waits for the
    // supervisor; the requests themselves only acknowledge.
    let order_updates = order_plant.subscribe_all();
    let pnl_updates = pnl_plant.subscribe_all();
    let by_account: HashMap<String, RithmicOrderPlantHandle> = accounts
        .iter()
        .map(|account| (account.id.clone(), order_plant.get_handle(&rithmic_account(account))))
        .collect();
    for account in &accounts {
        if let Some(desk) = by_account.get(&account.id) {
            let _ = within(REQUEST, desk.subscribe_order_updates()).await;
            let _ = within(REQUEST, desk.show_orders()).await;
        }
        let handle = pnl_plant.get_handle(&rithmic_account(account));
        let _ = within(REQUEST, handle.subscribe_pnl_updates()).await;
        let _ = within(REQUEST, handle.get_pnl_position_snapshot()).await;
    }
    Ok(TradingPlants {
        order_updates,
        pnl_updates,
        accounts,
        link: TradingLink::Rithmic { order_plant, pnl_plant, desks: Arc::new(Desks { order, pnl, by_account }) },
    })
}

/// The trading half, as the connection's supervisor keeps it.
pub(crate) struct TradingHalf {
    pub(crate) phase: Phase,
    state: watch::Sender<Standing>,
    pub(crate) pulse: Option<PulseSink>,
    /// Who is waiting for it to sign in.
    pub(crate) waiting: Vec<oneshot::Sender<Opened>>,
    book: Arc<Mutex<State>>,
    accounts: Arc<Mutex<Vec<Account>>>,
    pub(crate) live: Option<TradingPlants>,
    /// Signed in since it was opened, so that a failure is a drop to come
    /// back from rather than a sign-in that did not work.
    pub(crate) been_up: bool,
}

impl TradingHalf {
    pub(crate) fn new(
        state: watch::Sender<Standing>,
        book: Arc<Mutex<State>>,
        accounts: Arc<Mutex<Vec<Account>>>,
    ) -> TradingHalf {
        TradingHalf { phase: Phase::Off, state, pulse: None, waiting: Vec::new(), book, accounts, live: None, been_up: false }
    }

    fn tell(&self, pulse: Pulse) {
        if let Some(sink) = &self.pulse {
            sink(pulse);
        }
    }

    /// Opened afresh: nothing is known yet.
    pub(crate) fn reset(&mut self) {
        self.been_up = false;
        *self.book.lock().unwrap_or_else(|e| e.into_inner()) = State::default();
        self.accounts.lock().unwrap_or_else(|e| e.into_inner()).clear();
        let _ = self.state.send(Standing::Connecting);
    }

    /// Signed in, for the first time since it was opened or again after a
    /// drop.
    pub(crate) fn up(&mut self, plants: TradingPlants) {
        let back = self.been_up;
        if back {
            self.book.lock().unwrap_or_else(|e| e.into_inner()).signed_in_again();
        }
        *self.accounts.lock().unwrap_or_else(|e| e.into_inner()) = plants.accounts.clone();
        let _ = self.state.send(Standing::Up(plants.desks()));
        self.been_up = true;
        self.live = Some(plants);
        for waiting in self.waiting.drain(..) {
            let _ = waiting.send(Ok(()));
        }
        if back {
            self.tell(Pulse::Back);
        }
    }

    /// Dropped: say so, and come back.
    pub(crate) fn lose(&mut self, error: Error) {
        let _ = self.state.send(Standing::Down(error.clone()));
        self.tell(Pulse::Lost(error));
    }

    /// Signing in did not work, and the phase says what comes of it: down
    /// and tried again, stopped, or closed (`closed`) because it never was
    /// up. Whoever waits hears why.
    pub(crate) fn fail(&mut self, error: Error, closed: bool) {
        let down = matches!(self.phase, Phase::Down);
        let was_down = matches!(&*self.state.borrow(), Standing::Down(_));
        let _ = self.state.send(match down {
            true => Standing::Down(error.clone()),
            false => Standing::Stopped(error.clone()),
        });
        for waiting in self.waiting.drain(..) {
            let _ = waiting.send(Err((error.clone(), closed)));
        }
        // A retry that failed again is no news.
        if self.been_up && !(down && was_down) {
            self.tell(Pulse::Lost(error));
        }
    }

    /// One push from either plant, into the book.
    pub(crate) fn apply(&mut self, message: &RithmicMessage) {
        let moved = self.book.lock().unwrap_or_else(|e| e.into_inner()).apply(message);
        if moved {
            self.tell(Pulse::Moved);
        }
    }

    /// Sign out. Whoever was waiting hears it closed.
    pub(crate) async fn close(&mut self) {
        if let Some(plants) = self.live.take() {
            plants.close(true).await;
        }
        self.phase = Phase::Off;
        self.pulse = None;
        for waiting in self.waiting.drain(..) {
            let _ = waiting.send(Err((Error::Closed, true)));
        }
        self.reset();
        let _ = self.state.send(Standing::Stopped(Error::Closed));
    }
}

/// The accounts in an account list reply, in the order listed.
fn accounts_of(responses: &[RithmicResponse]) -> Result<Vec<Account>, Error> {
    let accounts = listed(responses.iter().filter_map(|response| match &response.message {
        RithmicMessage::ResponseAccountList(found) => Some(found),
        _ => None,
    }));
    if accounts.is_empty()
        && let Some(error) = responses.last().and_then(|end| end.error.as_ref())
    {
        return Err(error_of(error.clone()));
    }
    Ok(accounts)
}

fn listed<'a>(found: impl Iterator<Item = &'a ResponseAccountList>) -> Vec<Account> {
    let mut accounts: Vec<Account> = Vec::new();
    for found in found {
        let Some(id) = found.account_id.clone().filter(|id| !id.is_empty()) else { continue };
        if accounts.iter().any(|known| known.id == id) {
            continue;
        }
        accounts.push(Account {
            id,
            name: found.account_name.clone().filter(|name| !name.is_empty()),
            currency: found.account_currency.clone().filter(|currency| !currency.is_empty()),
            fcm: found.fcm_id.clone().unwrap_or_default(),
            ib: found.ib_id.clone().unwrap_or_default(),
        });
    }
    accounts
}

#[cfg(test)]
mod tests {
    use super::*;

    fn found(id: &str, currency: &str) -> ResponseAccountList {
        let mut found = ResponseAccountList::default();
        found.fcm_id = Some("Rithmic-FCM".into());
        found.ib_id = Some("Prospects".into());
        found.account_id = Some(id.into());
        found.account_currency = Some(currency.into());
        found
    }

    #[test]
    fn an_account_list_is_read_in_order_once_each() {
        let replies = [found("1234", "USD"), found("1235", "USD"), found("1234", "USD")];
        let accounts = listed(replies.iter());
        let ids: Vec<&str> = accounts.iter().map(|a| a.id.as_str()).collect();
        assert_eq!(ids, vec!["1234", "1235"]);
        assert_eq!(accounts[0].fcm, "Rithmic-FCM");
        assert_eq!(accounts[0].currency.as_deref(), Some("USD"));
        assert!(!format!("{:?}", accounts[0]).contains("1234"), "an account number in a log");
    }

    fn step(basket: &str, status: &str, fields: impl FnOnce(&mut RithmicOrderNotification)) -> RithmicMessage {
        let mut update = RithmicOrderNotification::default();
        update.basket_id = Some(basket.into());
        update.status = Some(status.into());
        fields(&mut update);
        RithmicMessage::RithmicOrderNotification(update)
    }

    /// A market buy, the way Rithmic Test reports one: placed, each step on
    /// its way, then complete with a fill. Nothing that says nothing new
    /// counts as a change.
    #[test]
    fn an_order_is_followed_from_placed_to_filled() {
        let mut state = State::default();
        let order = Order {
            account: "951".into(),
            contract: "ESZ6".into(),
            exchange: "CME".into(),
            side: Side::Buy,
            qty: 1,
            kind: Kind::Market,
            price: None,
            tag: "omacharts:1".into(),
        };
        state.placed(&order, "B1", 100);
        assert_eq!(state.book().orders[0].status, Status::Pending);
        assert!(!state.apply(&step("B1", "open pending", |_| {})), "still pending");
        assert!(state.apply(&step("B1", "complete", |n| {
            n.completion_reason = Some("F".into());
            n.avg_fill_price = Some(7612.25);
            n.total_fill_size = Some(1);
            n.ssboe = Some(160);
        })));
        let filled = &state.book().orders[0];
        assert_eq!((filled.status.clone(), filled.fill_price, filled.at), (Status::Filled, Some(7612.25), 160));
        assert_eq!(filled.contract, "ESZ6");
    }

    /// A limit heard of first from Rithmic (working at sign-in): its side,
    /// price and kind are read off the notification, it works, and a cancel
    /// completes it as cancelled. A stop's price is its trigger.
    #[test]
    fn a_working_order_is_read_from_its_notifications() {
        let mut state = State::default();
        assert!(state.apply(&step("L1", "open", |n| {
            n.account_id = Some("951".into());
            n.symbol = Some("ESZ6".into());
            n.exchange = Some("CME".into());
            n.transaction_type = Some(2);
            n.price_type = Some(1);
            n.price = Some(7620.0);
            n.quantity = Some(1);
        })));
        let working = &state.book().orders[0];
        assert_eq!((working.side, working.kind, working.price), (Side::Sell, Kind::Limit, Some(7620.0)));
        assert_eq!(working.status, Status::Working);
        assert!(state.apply(&step("L1", "complete", |n| n.completion_reason = Some("C".into()))));
        assert_eq!(state.book().orders[0].status, Status::Cancelled);

        state.apply(&step("S1", "trigger pending", |n| {
            n.price_type = Some(4);
            n.trigger_price = Some(7600.0);
        }));
        let stop = &state.book().orders[0];
        assert_eq!((stop.kind, stop.price, stop.status.clone()), (Kind::Stop, Some(7600.0), Status::Working));
        assert_eq!(state.book().orders.len(), 2, "newest first, both kept");
    }

    #[test]
    fn a_rejection_keeps_rithmics_words() {
        let mut state = State::default();
        state.apply(&step("R1", "rejected", |n| n.text = Some("Order would exceed position limit".into())));
        assert_eq!(state.book().orders[0].status, Status::Rejected("Order would exceed position limit".into()));
        let mut exchange = ExchangeOrderNotification::default();
        exchange.basket_id = Some("R2".into());
        exchange.report_type = Some("reject".into());
        exchange.report_text = Some("price out of band".into());
        state.apply(&step("R2", "open pending", |_| {}));
        assert!(state.apply(&RithmicMessage::ExchangeOrderNotification(exchange)));
        assert_eq!(state.book().orders[0].status, Status::Rejected("price out of band".into()));
        // Rithmic completes it after, with only a reason code.
        state.apply(&step("R2", "complete", |n| n.completion_reason = Some("R".into())));
        assert_eq!(state.book().orders[0].status, Status::Rejected("price out of band".into()));
    }

    /// Positions and balances as the PnL plant pushes them: a position
    /// shows while held, with Rithmic's open P&L, and goes when flat.
    #[test]
    fn positions_and_balances_are_kept_as_pushed() {
        let mut state = State::default();
        let mut held = InstrumentPnLPositionUpdate::default();
        held.account_id = Some("951".into());
        held.symbol = Some("ESZ6".into());
        held.exchange = Some("CME".into());
        held.net_quantity = Some(1);
        held.avg_open_fill_price = Some(7612.25);
        held.day_open_pnl = Some(125.0);
        assert!(state.apply(&RithmicMessage::InstrumentPnLPositionUpdate(held.clone())));
        assert!(!state.apply(&RithmicMessage::InstrumentPnLPositionUpdate(held.clone())));
        let position = &state.book().positions[0];
        assert_eq!((position.net, position.avg, position.open_pnl), (1, Some(7612.25), Some(125.0)));
        held.net_quantity = Some(0);
        assert!(state.apply(&RithmicMessage::InstrumentPnLPositionUpdate(held)));
        assert!(state.book().positions.is_empty(), "flat is not shown");

        let mut money = AccountPnLPositionUpdate::default();
        money.account_id = Some("951".into());
        money.account_balance = Some("1000125.00".into());
        money.day_closed_pnl = Some("-3.94".into());
        assert!(state.apply(&RithmicMessage::AccountPnLPositionUpdate(money.clone())));
        assert!(!state.apply(&RithmicMessage::AccountPnLPositionUpdate(money)));
        let account = &state.book().accounts[0];
        assert_eq!((account.balance, account.realized_day), (Some(1_000_125.0), Some(-3.94)));
    }
}
