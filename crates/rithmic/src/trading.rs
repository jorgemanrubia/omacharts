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
//! Once back it says so ([`Pulse::Back`]), the cue for [`Trading::refresh`],
//! which asks for everything the accounts hold, are working and did today.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rithmic_rs::rti::messages::RithmicMessage;
use rithmic_rs::rti::{
    AccountPnLPositionUpdate, AccountRmsUpdates, ExchangeOrderNotification, InstrumentPnLPositionUpdate,
    ResponseAccountList, ResponseAccountRmsInfo, ResponseShowFillHistory,
    RithmicOrderNotification,
};
use rithmic_rs::{
    ConnectStrategy, ManualOrAutoEntry, OrderSide, OrderStatus, OrderType, RithmicAccount, RithmicCancelAllOrders,
    RithmicCancelOrder, RithmicExitPosition, RithmicLinkOrders, RithmicModifyOrder, RithmicOrder, RithmicOrderPlant,
    RithmicOrderPlantHandle, RithmicPnlPlant, RithmicPnlPlantHandle, RithmicResponse, RmsUpdateBits, TimeInForce,
};
use rithmic_rs::FillHistoryRange;
use tokio::sync::{broadcast, oneshot, watch};

use crate::client::{self, config, error_of, signed_in, within};
use crate::connection::{Connection, Inner, Opened, Phase};
use crate::{App, Credentials, Error, Live, Side};

/// How long one request may take.
const REQUEST: Duration = Duration::from_secs(15);

/// How long the trading half waits on a data half to say which contract a
/// root trades as: a sign-in and two requests.
const FRONT_PATIENCE: Duration = Duration::from_secs(45);

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
    /// Each fill, oldest first, from the fill history and the exchange's
    /// notifications.
    pub fills: Vec<Fill>,
}

/// Part or all of an order, filled.
#[derive(Clone, Debug, PartialEq)]
pub struct Fill {
    /// The order it is part of.
    pub basket_id: String,
    /// Rithmic's id for the fill, by which the history and a notification
    /// of the same fill are known to be one.
    pub id: Option<String>,
    /// Unix seconds.
    pub at: i64,
    pub qty: u32,
    pub price: f64,
}

/// What Rithmic's risk system holds an account to, from its RMS info.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Limits {
    /// How far the day may lose before the account stops trading.
    pub loss_limit: Option<f64>,
    pub max_order_qty: Option<u32>,
    pub buy_limit: Option<u32>,
    pub sell_limit: Option<u32>,
    /// The balance below which the account is liquidated, as it stands now:
    /// a trailing drawdown moves it.
    pub liquidate_at: Option<f64>,
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
    /// Today's, closed and open, as Rithmic counts it.
    pub day_pnl: Option<f64>,
    /// What the risk system holds it to, once asked.
    pub limits: Option<Limits>,
}

/// Everything a session knows of its accounts' trading.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Book {
    pub accounts: Vec<AccountPnl>,
    /// Only contracts held, flat ones left out.
    pub positions: Vec<Position>,
    /// Newest first.
    pub orders: Vec<WorkingOrder>,
    /// A [`Trading::refresh`] has settled since the half last signed in:
    /// what is held, working and done today is all here, not only what was
    /// pushed since.
    pub refreshed: bool,
}

/// What the listener keeps from the plants' pushes, and [`Trading::book`]
/// copies out.
#[derive(Default)]
pub struct State {
    accounts: Vec<AccountPnl>,
    positions: Vec<Position>,
    orders: Vec<WorkingOrder>,
    /// Oldest first.
    fills: Vec<Fill>,
    refreshed: bool,
    /// When the plants last pushed anything, which a refresh waits to go
    /// quiet.
    heard: Option<Instant>,
    /// Positions were pushed since the last refresh began: the PnL plant's
    /// snapshot said something, so they need not be worked out from fills.
    positions_pushed: bool,
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

/// Does what Rithmic said of an order say it was refused? Its risk desk
/// cancels what it refuses and says so only here: "Rejected at RMS - …".
fn refusal(said: &str) -> bool {
    said.trim_start().get(..6).is_some_and(|word| word.eq_ignore_ascii_case("reject"))
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
            orders: self
                .orders
                .iter()
                .rev()
                .map(|order| WorkingOrder {
                    fills: self.fills.iter().filter(|fill| fill.basket_id == order.basket_id).cloned().collect(),
                    ..order.clone()
                })
                .collect(),
            refreshed: self.refreshed,
        }
    }

    /// Signed in again after a drop: what is still working, and what is
    /// held, is listed afresh by the refresh that follows. An order that
    /// finished meanwhile is not listed, and goes, rather than be shown
    /// working when it is not.
    fn signed_in_again(&mut self) {
        self.orders.retain(|order| !order.status.working());
        self.positions.clear();
        self.refreshed = false;
    }

    /// A refresh begins: nothing it asks for has been heard yet.
    fn refreshing(&mut self) {
        self.positions_pushed = false;
        self.heard = Some(Instant::now());
    }

    /// The refresh has settled.
    fn refreshed(&mut self) {
        self.positions_from_fills();
        self.refreshed = true;
    }

    /// Note a fill, once however many times it is told. True when it is
    /// new.
    pub fn fill(&mut self, fill: Fill) -> bool {
        let same = |known: &Fill| match (&known.id, &fill.id) {
            (Some(a), Some(b)) => a == b && known.basket_id == fill.basket_id,
            _ => {
                known.basket_id == fill.basket_id
                    && known.at == fill.at
                    && known.qty == fill.qty
                    && known.price == fill.price
            }
        };
        if self.fills.iter().any(same) {
            return false;
        }
        let at = self.fills.partition_point(|known| known.at <= fill.at);
        self.fills.insert(at, fill);
        true
    }

    /// What the risk system said of `account`. The liquidation level comes
    /// from its own updates, and is kept.
    pub fn limits(&mut self, account: &str, limits: Limits) -> bool {
        let known = self.account_mut(account);
        let liquidate_at = known.limits.as_ref().and_then(|known| known.liquidate_at);
        let limits = Limits { liquidate_at: limits.liquidate_at.or(liquidate_at), ..limits };
        let changed = known.limits.as_ref() != Some(&limits);
        known.limits = Some(limits);
        changed
    }

    /// When the PnL plant said nothing of positions since the refresh
    /// began, work out what is held from today's fills, oldest first, the
    /// average being what the open part cost. Nothing from before today is
    /// known this way, so a position carried overnight is not seen.
    fn positions_from_fills(&mut self) -> bool {
        if self.positions_pushed {
            return false;
        }
        let mut held: Vec<Position> = Vec::new();
        for fill in &self.fills {
            let Some(order) = self.orders.iter().find(|order| order.basket_id == fill.basket_id) else { continue };
            let signed = match order.side {
                Side::Buy => fill.qty as i32,
                Side::Sell => -(fill.qty as i32),
            };
            let at = match held.iter().position(|p| p.account == order.account && p.contract == order.contract) {
                Some(at) => at,
                None => {
                    held.push(Position {
                        account: order.account.clone(),
                        contract: order.contract.clone(),
                        exchange: order.exchange.clone(),
                        net: 0,
                        avg: None,
                        open_pnl: None,
                    });
                    held.len() - 1
                }
            };
            let position = &mut held[at];
            let net = position.net + signed;
            position.avg = match (position.net, position.avg) {
                (0, _) | (_, None) => Some(fill.price),
                // Adding to it: the average of the two.
                (before, Some(avg)) if before.signum() == signed.signum() => Some(
                    (avg * f64::from(before.abs()) + fill.price * f64::from(signed.abs())) / f64::from(net.abs()),
                ),
                // Through flat to the other side: what the rest cost.
                (before, Some(_)) if net != 0 && net.signum() != before.signum() => Some(fill.price),
                // Taking some off: what is left cost what it did.
                (_, avg) => avg,
            };
            position.net = net;
        }
        held.retain(|position| position.net != 0);
        if held == self.positions {
            return false;
        }
        self.positions = held;
        true
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
            fills: Vec::new(),
        });
    }

    /// Take one push from either plant. True when the book changed.
    pub fn apply(&mut self, message: &RithmicMessage) -> bool {
        self.heard = Some(Instant::now());
        match message {
            RithmicMessage::RithmicOrderNotification(update) => self.rithmic_order(update),
            RithmicMessage::ExchangeOrderNotification(update) => self.exchange_order(update),
            RithmicMessage::InstrumentPnLPositionUpdate(update) => self.instrument(update),
            RithmicMessage::AccountPnLPositionUpdate(update) => self.account(update),
            RithmicMessage::AccountRmsUpdates(update) => self.rms_update(update),
            _ => false,
        }
    }

    /// One fill from the fill history: noted, with the order it is part of
    /// made from it when nothing else has told of that order.
    fn history_fill(&mut self, fill: &ResponseShowFillHistory) {
        let Some(basket) = fill.basket_id.as_deref().filter(|id| !id.is_empty()) else { return };
        let (Some(price), Some(qty)) = (
            fill.fill_price.filter(|price| *price != 0.0),
            fill.fill_size.and_then(|qty| u32::try_from(qty).ok()).filter(|qty| *qty > 0),
        ) else {
            return;
        };
        let at = match (fill.fill_date.as_deref(), fill.fill_time.as_deref()) {
            (Some(date), Some(time)) => moment_of(date, time),
            _ => None,
        }
        .or(fill.source_ssboe.map(i64::from))
        .or(fill.ssboe.map(i64::from))
        .unwrap_or_else(now);
        if self.order_mut(basket).is_none() {
            let word = |text: &Option<String>| text.as_deref().unwrap_or("").trim().to_ascii_lowercase();
            let side = match word(&fill.transaction_type).as_str() {
                "1" | "buy" | "b" => Side::Buy,
                _ => Side::Sell,
            };
            let kind = match word(&fill.price_type).as_str() {
                "1" | "limit" => Kind::Limit,
                "3" | "4" | "stop market" | "stop limit" | "stop_market" | "stop_limit" => Kind::Stop,
                _ => Kind::Market,
            };
            // Oldest first, as the orders are kept.
            let place = self.orders.partition_point(|order| order.at <= at);
            self.orders.insert(place, WorkingOrder {
                account: fill.account_id.clone().unwrap_or_default(),
                basket_id: basket.to_string(),
                contract: fill.symbol.clone().unwrap_or_default(),
                exchange: fill.exchange.clone().unwrap_or_default(),
                side,
                qty: u32::try_from(fill.total_fill_size.unwrap_or(0) + fill.total_unfilled_size.unwrap_or(0))
                    .ok()
                    .filter(|qty| *qty > 0)
                    .unwrap_or(qty),
                filled: 0,
                kind,
                price: match kind {
                    Kind::Market => None,
                    Kind::Limit => fill.price,
                    Kind::Stop => fill.trigger_price.or(fill.price),
                },
                status: Status::Filled,
                fill_price: None,
                at,
                tag: None,
                said: None,
                fills: Vec::new(),
            });
        }
        self.fill(Fill { basket_id: basket.to_string(), id: fill.fill_id.clone().filter(|id| !id.is_empty()), at, qty, price });
        // An order made from its fills is as filled as they say.
        let filled: u32 = self.fills.iter().filter(|known| known.basket_id == basket).map(|known| known.qty).sum();
        let cost: f64 = self.fills.iter().filter(|known| known.basket_id == basket).map(|known| known.price * f64::from(known.qty)).sum();
        if let Some(order) = self.order_mut(basket)
            && order.filled < filled
        {
            order.filled = filled.min(order.qty.max(filled));
            if order.fill_price.is_none() && filled > 0 {
                order.fill_price = Some(cost / f64::from(filled));
            }
        }
    }

    fn rms_update(&mut self, update: &AccountRmsUpdates) -> bool {
        let at = number(update.auto_liq_threshold_current_value.as_deref());
        let (Some(account), Some(at)) = (update.account_id.as_deref(), at) else { return false };
        let known = self.account_mut(account);
        let mut limits = known.limits.clone().unwrap_or_default();
        if limits.liquidate_at == Some(at) {
            return false;
        }
        limits.liquidate_at = Some(at);
        known.limits = Some(limits);
        true
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
            fills: Vec::new(),
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
        let said = reason([update.report_text.as_ref(), update.text.as_ref()]);
        let refused = |known: &Status| match (known, said.clone()) {
            (Status::Rejected(said), None) => Status::Rejected(said.clone()),
            (_, said) => Status::Rejected(said.unwrap_or_else(|| "no reason given".into())),
        };
        // Rithmic's own risk desk refuses an order by cancelling it, the
        // refusal only in the text: "Rejected at RMS - Loss limit for
        // product code exceeded". A cancel that says it was rejected is a
        // rejection, so the reason is not lost with the order.
        let cancelled = |known: &Status| match said.as_deref().filter(|said| refusal(said)) {
            Some(_) => refused(known),
            None => Status::Cancelled,
        };
        if let Some(status) = update.status.as_deref() {
            let status: OrderStatus = status.parse().unwrap_or_default();
            let completion = update.completion_reason.as_deref().unwrap_or("").trim();
            order.status = match status {
                OrderStatus::Rejected => refused(&order.status),
                OrderStatus::Cancelled => cancelled(&order.status),
                OrderStatus::Complete | OrderStatus::Expired => match completion {
                    "F" | "Fill" | "Filled" => Status::Filled,
                    "C" | "Cancel" | "Canceled" | "Cancelled" => cancelled(&order.status),
                    _ if filled => Status::Filled,
                    "R" | "Reject" | "Rejected" => refused(&order.status),
                    _ => match order.status {
                        Status::Rejected(_) => order.status.clone(),
                        _ => cancelled(&order.status),
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

    /// What the exchange said of an order. One not heard of before is taken
    /// from it whole: today's orders, asked for by a refresh, come back on
    /// Rithmic Test as the exchange's notifications alone, each saying how
    /// the order ended.
    fn exchange_order(&mut self, update: &ExchangeOrderNotification) -> bool {
        let Some(basket) = update.basket_id.as_deref().filter(|id| !id.is_empty()) else { return false };
        if self.order_mut(basket).is_none() {
            let Some(order) = told_of(basket, update) else { return false };
            let at = self.orders.partition_point(|known| known.at <= order.at);
            self.orders.insert(at, order);
        }
        let Some(order) = self.order_mut(basket) else { return false };
        let before = order.clone();
        let mut fill = None;
        if let Some(said) = reason([update.report_text.as_ref(), update.text.as_ref()]) {
            order.said = Some(said);
        }
        match update.report_type.as_deref().map(str::to_ascii_lowercase).as_deref() {
            Some("fill") => {
                if let Some(price) = update.avg_fill_price.or(update.fill_price).filter(|price| *price != 0.0) {
                    order.fill_price = Some(price);
                }
                let size = update.fill_size.and_then(|qty| u32::try_from(qty).ok()).filter(|qty| *qty > 0);
                if let (Some(price), Some(qty)) = (update.fill_price.filter(|price| *price != 0.0), size) {
                    fill = Some(Fill {
                        basket_id: basket.to_string(),
                        id: update.fill_id.clone().filter(|id| !id.is_empty()),
                        at: update.ssboe.map(i64::from).unwrap_or_else(now),
                        qty,
                        price,
                    });
                }
                if let Some(left) = update.total_unfilled_size.and_then(|left| u32::try_from(left).ok()) {
                    order.filled = order.qty.saturating_sub(left);
                } else if let Some(done) = update.total_fill_size.and_then(|done| u32::try_from(done).ok()) {
                    order.filled = done.max(order.filled);
                }
                match update.total_unfilled_size {
                    Some(0) => order.status = Status::Filled,
                    Some(_) if order.status.working() => order.status = Status::PartiallyFilled,
                    None if order.filled >= order.qty => order.status = Status::Filled,
                    _ => {}
                }
            }
            Some("reject") => {
                let said = reason([update.report_text.as_ref(), update.text.as_ref()]);
                order.status = Status::Rejected(said.unwrap_or_else(|| "no reason given".into()));
            }
            Some("cancel") => {
                let said = reason([update.report_text.as_ref(), update.text.as_ref()]);
                order.status = match said.filter(|said| refusal(said)) {
                    Some(said) => Status::Rejected(said),
                    None => Status::Cancelled,
                };
            }
            Some("status") if order.status == Status::Pending => order.status = Status::Working,
            _ => {}
        }
        let changed = *order != before;
        let filled = fill.is_some_and(|fill| self.fill(fill));
        changed || filled
    }

    fn instrument(&mut self, update: &InstrumentPnLPositionUpdate) -> bool {
        let (Some(account), Some(symbol)) = (update.account_id.as_deref(), update.symbol.as_deref()) else {
            return false;
        };
        self.positions_pushed = true;
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
        if let Some(day) = number(update.day_pnl.as_deref()) {
            known.day_pnl = Some(day);
        }
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
    /// Signed in again after a drop. The accounts were listed again, and an
    /// order that was working is gone from the book rather than shown
    /// working when it may have finished meanwhile: the cue to
    /// [`Trading::refresh`], and then to reconcile what was being waited
    /// for.
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
    /// And one PnL handle per account, for its snapshot.
    pnl_by_account: HashMap<String, RithmicPnlPlantHandle>,
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

    /// The contract an order on the futures root `root` goes to on
    /// `exchange`: the front month any data half last found, even one closed
    /// since. When none has, or not lately, this login's data half is asked
    /// in the background, opened for it if no chart has, and `found` is
    /// called once it answers. Never waits: `None` until then.
    pub fn front_month(&self, root: &str, exchange: &str, found: impl FnOnce() + Send + 'static) -> Option<String> {
        let known = client::found_front_month(root, exchange);
        if !matches!(known, Some((_, true))) {
            self.ask_front_month(root, exchange, found);
        }
        known.map(|(contract, _)| contract)
    }

    /// Watch `root` until the data half says which contract it subscribed
    /// to, which remembers it as the front month, and stop watching. One
    /// question at a time for each root.
    fn ask_front_month(&self, root: &str, exchange: &str, found: impl FnOnce() + Send + 'static) {
        static ASKING: Mutex<Option<HashSet<(String, String)>>> = Mutex::new(None);
        let key = (root.to_string(), exchange.to_string());
        let done = {
            let key = key.clone();
            move || {
                if let Some(asking) = ASKING.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
                    asking.remove(&key);
                }
            }
        };
        if !ASKING.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(HashSet::new).insert(key) {
            return;
        }
        let (told, heard) = std::sync::mpsc::channel();
        let told = Mutex::new(told);
        let watch = Connection::of(self.inner.clone()).data().watch(root, exchange, move |live| {
            let _ = told.lock().unwrap_or_else(|e| e.into_inner()).send(live);
        });
        let asking = std::thread::Builder::new().name("rithmic-front".into()).spawn({
            let done = done.clone();
            move || {
                let deadline = Instant::now() + FRONT_PATIENCE;
                let answered = loop {
                    match heard.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
                        Ok(Live::Watching { .. }) => break true,
                        // Reconnecting by itself: the answer may still come.
                        Ok(Live::Lost(Error::Connection(_) | Error::Timeout) | Live::Trade(_) | Live::Last(_)) => continue,
                        Ok(Live::Lost(_)) | Err(_) => break false,
                    }
                };
                drop(watch);
                done();
                if answered {
                    found();
                }
            }
        });
        if asking.is_err() {
            done();
        }
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

    /// Ask for everything the accounts hold now, and wait for it: working
    /// orders, today's orders whatever became of them, today's fills, the
    /// risk system's limits and the PnL plant's positions and balances. What
    /// is pushed back lands on the book as any push does; the fills and the
    /// limits come back as answers and are put there here. Returns once the
    /// plants have been quiet for [`SETTLE`] after the last answer, with
    /// [`Book::refreshed`] set.
    ///
    /// When the PnL plant says nothing of positions, as Rithmic Test's
    /// snapshot does not, what is held is worked out from today's fills,
    /// and the balance stays unknown until the first push.
    pub fn refresh(&self) -> Result<(), Error> {
        let desks = self.desks()?;
        let accounts = self.accounts();
        self.inner.book.lock().unwrap_or_else(|e| e.into_inner()).refreshing();
        let book = self.inner.book.clone();
        let started = Instant::now();
        self.inner.handle.block_on(async {
            let dates = match within(REQUEST, desks.order.show_order_history_dates()).await {
                Ok(replies) => today_of(&replies),
                Err(_) => Vec::new(),
            };
            let (from, to) = today_window();
            for account in &accounts {
                let Some(desk) = desks.by_account.get(&account.id) else { continue };
                within(REQUEST, desk.show_orders()).await?;
                for date in &dates {
                    let _ = within(REQUEST, desk.show_order_history_summary(date)).await;
                }
                if let Ok(replies) = within(REQUEST, desk.show_fill_history(FillHistoryRange::ssboe(from, to), None)).await {
                    let mut state = book.lock().unwrap_or_else(|e| e.into_inner());
                    for fill in replies.iter().filter_map(|reply| match &reply.message {
                        RithmicMessage::ResponseShowFillHistory(fill) => Some(fill),
                        _ => None,
                    }) {
                        state.history_fill(fill);
                    }
                }
                if let Some(pnl) = desks.pnl_by_account.get(&account.id) {
                    let _ = within(REQUEST, pnl.get_pnl_position_snapshot()).await;
                }
            }
            if let Ok(replies) = within(REQUEST, desks.order.get_account_rms_info()).await {
                let mut state = book.lock().unwrap_or_else(|e| e.into_inner());
                for info in replies.iter().filter_map(|reply| match &reply.message {
                    RithmicMessage::ResponseAccountRmsInfo(info) => Some(info),
                    _ => None,
                }) {
                    if let Some(account) = info.account_id.as_deref().filter(|id| !id.is_empty()) {
                        state.limits(account, limits_of(info));
                    }
                }
            }
            Ok::<(), Error>(())
        })?;
        // What the requests set off arrives after their answers: wait for
        // the plants to go quiet, a little at most.
        loop {
            let heard = book.lock().unwrap_or_else(|e| e.into_inner()).heard.unwrap_or(started);
            let quiet = heard.max(started) + SETTLE;
            let now = Instant::now();
            if now >= quiet || now >= started + SETTLE_AT_MOST {
                break;
            }
            std::thread::sleep(quiet.min(started + SETTLE_AT_MOST) - now);
        }
        book.lock().unwrap_or_else(|e| e.into_inner()).refreshed();
        Ok(())
    }

    /// The orders on `account` that filled between `from` and `to`, unix
    /// seconds, each with its fills, oldest first: what Rithmic's fill
    /// history says. Nothing of it goes into the book.
    pub fn history(&self, account: &str, from: i64, to: i64) -> Result<Vec<WorkingOrder>, Error> {
        let clamp = |at: i64| i32::try_from(at).unwrap_or(if at < 0 { 0 } else { i32::MAX });
        let range = FillHistoryRange::ssboe(clamp(from), clamp(to));
        let replies = self.on_desk(account, async |desk| within(REQUEST, desk.show_fill_history(range, None)).await)?;
        let mut state = State::default();
        for fill in replies.iter().filter_map(|reply| match &reply.message {
            RithmicMessage::ResponseShowFillHistory(fill) => Some(fill),
            _ => None,
        }) {
            state.history_fill(fill);
        }
        let mut orders = state.book().orders;
        orders.reverse();
        Ok(orders)
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

/// Sign in to the order and PnL plants, list the login's accounts, and
/// subscribe to what they push.
pub(crate) async fn connect(app: App, login: &str, credentials: &Credentials) -> Result<TradingPlants, Error> {
    let heard = Listing::of(credentials);
    LISTINGS.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(HashMap::new).insert(login.to_string(), heard);
    let outcome = sign_in_trading(app, credentials, login).await;
    if let Err(error) = &outcome {
        note_listing(login, |listing| {
            listing.failed.get_or_insert_with(|| error.to_string());
        });
    }
    outcome
}

async fn sign_in_trading(
    app: App,
    credentials: &Credentials,
    login: &str,
) -> Result<TradingPlants, Error> {
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
            note_listing(login, |listing| listing.failed = Some(format!("the account list was not answered: {error}")));
            let _ = within(REQUEST, order.disconnect()).await;
            return Err(error);
        }
    };
    note_listing(login, |listing| listing.hear(&listed));
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
    let pnl_by_account: HashMap<String, RithmicPnlPlantHandle> = accounts
        .iter()
        .map(|account| (account.id.clone(), pnl_plant.get_handle(&rithmic_account(account))))
        .collect();
    // Subscribed here, again after every drop; what is held and working now
    // is asked for by `Trading::refresh`.
    for account in &accounts {
        if let Some(desk) = by_account.get(&account.id) {
            let _ = within(REQUEST, desk.subscribe_order_updates()).await;
            let _ = within(REQUEST, desk.subscribe_account_rms_updates(true, vec![RmsUpdateBits::AutoLiqThresholdCurrentValue])).await;
        }
        if let Some(handle) = pnl_by_account.get(&account.id) {
            let _ = within(REQUEST, handle.subscribe_pnl_updates()).await;
        }
    }
    Ok(TradingPlants {
        order_updates,
        pnl_updates,
        accounts,
        link: TradingLink::Rithmic {
            order_plant,
            pnl_plant,
            desks: Arc::new(Desks { order, pnl, by_account, pnl_by_account }),
        },
    })
}

/// How long the plants must be quiet after a refresh's last answer before
/// it is taken as settled, and the longest it waits for that.
const SETTLE: Duration = Duration::from_millis(300);
const SETTLE_AT_MOST: Duration = Duration::from_secs(5);

/// Today, as Rithmic dates its history: the dates it lists that are today's
/// in UTC or later, the session that opened last evening being dated by the
/// day it closes.
fn today_of(replies: &[RithmicResponse]) -> Vec<String> {
    let today = date_of(now());
    let mut dates: Vec<String> = replies
        .iter()
        .filter_map(|reply| match &reply.message {
            RithmicMessage::ResponseShowOrderHistoryDates(listed) => Some(listed.date.clone()),
            _ => None,
        })
        .flatten()
        .filter(|date| date.as_str() >= today.as_str())
        .collect();
    dates.sort();
    dates.dedup();
    dates
}

/// Since the start of today, in UTC, until an hour from now, in unix
/// seconds.
fn today_window() -> (i32, i32) {
    let now = now();
    let start = now - now.rem_euclid(86_400);
    (i32::try_from(start).unwrap_or(0), i32::try_from(now + 3_600).unwrap_or(i32::MAX))
}

/// `at`, unix seconds, as the UTC date Rithmic writes: "20261104".
fn date_of(at: i64) -> String {
    // Howard Hinnant's days-to-civil.
    let z = at.div_euclid(86_400) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}{month:02}{day:02}")
}

/// "20261104" and "14:03:07", in UTC, as unix seconds.
fn moment_of(date: &str, time: &str) -> Option<i64> {
    let (year, month, day) = (date.get(0..4)?.parse::<i64>().ok()?, date.get(4..6)?.parse::<i64>().ok()?, date.get(6..8)?.parse::<i64>().ok()?);
    let mut clock = time.split(':').map(|part| part.trim().parse::<i64>().ok());
    let (hours, minutes, seconds) = (clock.next()??, clock.next().flatten().unwrap_or(0), clock.next().flatten().unwrap_or(0));
    // Howard Hinnant's days-from-civil.
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some(days * 86_400 + hours * 3_600 + minutes * 60 + seconds)
}

/// An order first heard of from the exchange, as far as `update` says: its
/// account, contract, side, size, kind and price, working until the report
/// says otherwise.
fn told_of(basket: &str, update: &ExchangeOrderNotification) -> Option<WorkingOrder> {
    let given = |text: &Option<String>| text.as_ref().filter(|text| !text.is_empty()).cloned();
    let kind = kind_of(update.price_type)?;
    Some(WorkingOrder {
        account: given(&update.account_id)?,
        basket_id: basket.to_string(),
        contract: given(&update.symbol)?,
        exchange: given(&update.exchange).unwrap_or_default(),
        side: side_of(update.transaction_type)?,
        qty: update
            .quantity
            .or_else(|| Some(update.total_fill_size? + update.total_unfilled_size?))
            .and_then(|qty| u32::try_from(qty).ok())
            .filter(|qty| *qty > 0)
            .unwrap_or(1),
        filled: 0,
        kind,
        price: match kind {
            Kind::Stop => update.trigger_price.or(update.price),
            Kind::Limit => update.price,
            Kind::Market => None,
        }
        .filter(|price| *price != 0.0),
        status: Status::Working,
        fill_price: None,
        at: update.ssboe.map(i64::from).unwrap_or_else(now),
        tag: given(&update.user_tag),
        said: None,
        fills: Vec::new(),
    })
}

/// What an RMS info answer says the account is held to.
fn limits_of(info: &ResponseAccountRmsInfo) -> Limits {
    let count = |limit: Option<i32>| limit.and_then(|limit| u32::try_from(limit).ok()).filter(|limit| *limit > 0);
    Limits {
        loss_limit: info.loss_limit.filter(|limit| *limit != 0.0),
        max_order_qty: count(info.max_order_quantity),
        buy_limit: count(info.buy_limit),
        sell_limit: count(info.sell_limit),
        liquidate_at: None,
    }
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
    accounts_in(&replies_of(responses))
}

/// One account list reply: the account it lists, if it is one, and what
/// it refused, if anything.
type Reply<'a> = (Option<&'a ResponseAccountList>, Option<Error>);

fn replies_of(responses: &[RithmicResponse]) -> Vec<Reply<'_>> {
    responses
        .iter()
        .map(|response| {
            let found = match &response.message {
                RithmicMessage::ResponseAccountList(found) => Some(found),
                _ => None,
            };
            (found, response.error.clone().map(error_of))
        })
        .collect()
}

fn accounts_in(replies: &[Reply<'_>]) -> Result<Vec<Account>, Error> {
    let accounts = listed(replies.iter().filter_map(|(found, _)| *found));
    // Any reply's refusal, not only the last's: with no account listed it
    // is why.
    if accounts.is_empty()
        && let Some(error) = replies.iter().rev().find_map(|(_, error)| error.clone())
    {
        return Err(error);
    }
    Ok(accounts)
}

/// What the order plant said when it was last asked for a login's
/// accounts, and who asked, for the Log: the user, the system and the
/// gateway, never the password, and each reply's codes and account.
#[derive(Clone, Default, PartialEq)]
pub struct Listing {
    pub user: String,
    pub system: String,
    pub url: String,
    pub replies: Vec<ListReply>,
    /// Why signing in to trading went no further, when it did not.
    pub failed: Option<String>,
}

/// One account list reply, as Rithmic sent it.
#[derive(Clone, Default, PartialEq)]
pub struct ListReply {
    pub rp_code: Vec<String>,
    pub rq_handler_rp_code: Vec<String>,
    pub account_id: Option<String>,
    pub fcm_id: Option<String>,
    pub ib_id: Option<String>,
    /// What the reply refused, when it carried an error.
    pub error: Option<String>,
}

/// Without the account numbers, as [`Account`]'s is.
impl std::fmt::Debug for Listing {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Listing")
            .field("system", &self.system)
            .field("replies", &self.replies.len())
            .field("failed", &self.failed)
            .finish_non_exhaustive()
    }
}

impl Listing {
    fn of(credentials: &Credentials) -> Listing {
        Listing {
            user: credentials.user.clone(),
            system: credentials.system.clone(),
            url: credentials.url.clone(),
            ..Listing::default()
        }
    }

    /// The account list's replies, every one of them.
    pub(crate) fn hear(&mut self, responses: &[RithmicResponse]) {
        self.hear_replies(&replies_of(responses));
    }

    fn hear_replies(&mut self, replies: &[Reply<'_>]) {
        let given = |value: &Option<String>| value.clone().filter(|value| !value.is_empty());
        self.replies = replies
            .iter()
            .map(|(found, error)| {
                let error = error.as_ref().map(Error::to_string);
                match found {
                    Some(found) => ListReply {
                        rp_code: found.rp_code.clone(),
                        rq_handler_rp_code: found.rq_handler_rp_code.clone(),
                        account_id: given(&found.account_id),
                        fcm_id: given(&found.fcm_id),
                        ib_id: given(&found.ib_id),
                        error,
                    },
                    None => ListReply { error, ..ListReply::default() },
                }
            })
            .collect();
    }

    /// The first refusal any reply carried, listed accounts or not.
    pub fn refused(&self) -> Option<&str> {
        self.replies.iter().find_map(|reply| reply.error.as_deref())
    }

    /// Every line of it, for the Log's detail. `own` is the user the login
    /// is saved with, to say whether this is it.
    pub fn words(&self, own: &str) -> String {
        let whose = match self.user.trim() == own.trim() {
            true => "the login's own",
            false => "not the login's",
        };
        let mut lines = vec![
            format!("User: {} ({whose})", self.user),
            format!("System: {}", self.system),
            format!("Gateway: {}", self.url),
            format!("Account list replies: {}", self.replies.len()),
        ];
        for (at, reply) in self.replies.iter().enumerate() {
            let mut parts = vec![
                format!("rp_code [{}]", reply.rp_code.join(", ")),
                format!("rq_handler_rp_code [{}]", reply.rq_handler_rp_code.join(", ")),
            ];
            let fields = [("account", &reply.account_id), ("FCM", &reply.fcm_id), ("IB", &reply.ib_id)];
            parts.extend(fields.iter().filter_map(|(name, value)| value.as_ref().map(|value| format!("{name} {value}"))));
            if let Some(error) = &reply.error {
                parts.push(format!("refused: {error}"));
            }
            lines.push(format!("{}. {}", at + 1, parts.join(" · ")));
        }
        if let Some(failed) = &self.failed {
            lines.push(format!("Went no further: {failed}"));
        }
        lines.join("\n")
    }
}

/// The last account listing of each login, by its id, kept after the
/// connection is gone so that a sign-in that failed can still say it.
static LISTINGS: Mutex<Option<HashMap<String, Listing>>> = Mutex::new(None);

fn note_listing(login: &str, change: impl FnOnce(&mut Listing)) {
    let mut listings = LISTINGS.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(listing) = listings.get_or_insert_with(HashMap::new).get_mut(login) {
        change(listing);
    }
}

/// What the order plant said when `login` last signed in to trading, if it
/// has this run.
pub fn listing(login: &str) -> Option<Listing> {
    LISTINGS.lock().unwrap_or_else(|e| e.into_inner()).as_ref()?.get(login).cloned()
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

    fn coded(mut found: ResponseAccountList, rp_code: &[&str], handler: &[&str]) -> ResponseAccountList {
        found.rp_code = rp_code.iter().map(|code| code.to_string()).collect();
        found.rq_handler_rp_code = handler.iter().map(|code| code.to_string()).collect();
        found
    }

    fn refused(why: &str) -> Error {
        Error::Rejected(why.into())
    }

    /// A refusal on any account list reply counts, not only on the last:
    /// with nothing listed it is why the sign-in failed; with accounts
    /// listed they stand, and the listing says what was refused.
    #[test]
    fn a_refusal_on_any_account_list_reply_is_heard() {
        let end = coded(ResponseAccountList::default(), &["0"], &[]);
        let refused_first = coded(ResponseAccountList::default(), &["1", "permission denied"], &["1"]);
        let replies: Vec<Reply> = vec![(Some(&refused_first), Some(refused("permission denied"))), (Some(&end), None)];
        assert!(matches!(accounts_in(&replies), Err(Error::Rejected(why)) if why == "permission denied"));

        let one = coded(found("951", "USD"), &[], &["0"]);
        let replies: Vec<Reply> = vec![(Some(&one), None), (Some(&refused_first), Some(refused("permission denied"))), (Some(&end), None)];
        let accounts = accounts_in(&replies).expect("the account listed stands");
        assert_eq!(accounts.len(), 1);
        let mut listing = Listing { user: "trader".into(), system: "Rithmic Test".into(), url: "wss://g".into(), ..Listing::default() };
        listing.hear_replies(&replies);
        assert_eq!(listing.refused(), Some("Rithmic refused the request: permission denied"));

        let replies: Vec<Reply> = vec![(Some(&end), None)];
        assert!(accounts_in(&replies).unwrap().is_empty(), "nothing listed and nothing refused");
    }

    /// The listing says who signed in where and every reply, in lines for
    /// the Log's detail, and never the password.
    #[test]
    fn a_listing_says_every_reply_and_no_password() {
        let credentials = Credentials {
            user: "trader".into(),
            password: "secret".into(),
            system: "Rithmic Test".into(),
            url: "wss://rituz00100.rithmic.com:443".into(),
        };
        let mut listing = Listing::of(&credentials);
        let one = coded(found("951", "USD"), &[], &["0"]);
        let end = coded(ResponseAccountList::default(), &["0"], &[]);
        listing.hear_replies(&[(Some(&one), None), (Some(&end), None)]);
        assert_eq!(
            listing.words("trader"),
            "User: trader (the login's own)\nSystem: Rithmic Test\nGateway: wss://rituz00100.rithmic.com:443\n\
             Account list replies: 2\n\
             1. rp_code [] · rq_handler_rp_code [0] · account 951 · FCM Rithmic-FCM · IB Prospects\n\
             2. rp_code [0] · rq_handler_rp_code []"
        );
        assert!(listing.words("someone-else").starts_with("User: trader (not the login's)"));
        assert!(!listing.words("trader").contains("secret"));
        assert!(!format!("{listing:?}").contains("951"), "an account number in a log");

        let mut empty = Listing::of(&credentials);
        empty.failed = Some("timed out".into());
        assert!(empty.words("trader").ends_with("Account list replies: 0\nWent no further: timed out"));
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

    /// Rithmic's risk desk refuses an order by cancelling it, the refusal
    /// only in the text; the order is rejected, with that reason, however
    /// the cancel came: as a status, as a completion, or from the
    /// exchange. A cancel that says nothing of the kind is a cancel.
    #[test]
    fn a_cancel_that_says_it_was_rejected_is_a_rejection() {
        let rms = "Rejected at RMS - Loss limit for product code exceeded";
        let mut state = State::default();
        state.apply(&step("S1", "open pending", |_| {}));
        state.apply(&step("S1", "cancelled", |n| n.report_text = Some(rms.into())));
        assert_eq!(state.book().orders[0].status, Status::Rejected(rms.into()));

        state.apply(&step("S2", "open pending", |_| {}));
        state.apply(&step("S2", "complete", |n| {
            n.completion_reason = Some("C".into());
            n.text = Some(rms.into());
        }));
        assert_eq!(state.book().orders[0].status, Status::Rejected(rms.into()));

        state.apply(&step("S3", "open pending", |_| {}));
        let mut exchange = ExchangeOrderNotification::default();
        exchange.basket_id = Some("S3".into());
        exchange.report_type = Some("cancel".into());
        exchange.text = Some(rms.into());
        state.apply(&RithmicMessage::ExchangeOrderNotification(exchange));
        assert_eq!(state.book().orders[0].status, Status::Rejected(rms.into()));

        state.apply(&step("S4", "open", |_| {}));
        state.apply(&step("S4", "cancelled", |n| n.text = Some("Cancelled by user".into())));
        assert_eq!(state.book().orders[0].status, Status::Cancelled);
        assert!(refusal("rejected: no route") && !refusal("Cancel requested"));
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

    fn history(basket: &str, id: &str, side: &str, qty: u64, price: f64, time: &str) -> ResponseShowFillHistory {
        let mut fill = ResponseShowFillHistory::default();
        fill.basket_id = Some(basket.into());
        fill.fill_id = Some(id.into());
        fill.account_id = Some("951".into());
        fill.symbol = Some("ESZ6".into());
        fill.exchange = Some("CME".into());
        fill.transaction_type = Some(side.into());
        fill.price_type = Some("MARKET".into());
        fill.fill_size = Some(qty);
        fill.total_fill_size = Some(qty);
        fill.fill_price = Some(price);
        fill.fill_date = Some("20261104".into());
        fill.fill_time = Some(time.into());
        fill
    }

    /// Today's fills, from the history and from the exchange's
    /// notifications alike, each once, under their orders; an order heard
    /// of only through its fills is there, filled. With the PnL plant
    /// silent about positions, what is held is worked out from them: two
    /// bought at 7600 and 7610 and one sold leaves one at 7605.
    #[test]
    fn a_refresh_reads_todays_fills_and_works_out_what_is_held() {
        let mut state = State::default();
        state.refreshing();
        state.history_fill(&history("B1", "F1", "BUY", 1, 7_600.0, "14:00:00"));
        state.history_fill(&history("B2", "F2", "BUY", 1, 7_610.0, "14:05:00"));
        state.history_fill(&history("B3", "F3", "SELL", 1, 7_620.0, "14:10:00"));
        state.history_fill(&history("B3", "F3", "SELL", 1, 7_620.0, "14:10:00"));
        let mut told = ExchangeOrderNotification::default();
        told.basket_id = Some("B3".into());
        told.report_type = Some("fill".into());
        told.fill_id = Some("F3".into());
        told.fill_size = Some(1);
        told.fill_price = Some(7_620.0);
        assert!(!state.apply(&RithmicMessage::ExchangeOrderNotification(told)), "the same fill, told again");
        assert!(!state.book().refreshed);
        state.refreshed();
        let book = state.book();
        assert!(book.refreshed);
        assert_eq!(book.orders.len(), 3);
        assert_eq!(book.orders[0].basket_id, "B3", "newest first");
        assert_eq!((book.orders[0].side, book.orders[0].status.clone()), (Side::Sell, Status::Filled));
        assert_eq!(book.orders[0].fills.len(), 1);
        assert_eq!(book.orders[0].fills[0].at, moment_of("20261104", "14:10:00").unwrap());
        assert_eq!(book.positions.len(), 1);
        assert_eq!((book.positions[0].net, book.positions[0].avg), (1, Some(7_605.0)));
        assert!(book.accounts.iter().all(|money| money.balance.is_none()), "no balance until one is pushed");

        // Pushed positions are the PnL plant's word, and stand.
        let mut held = InstrumentPnLPositionUpdate::default();
        held.account_id = Some("951".into());
        held.symbol = Some("ESZ6".into());
        held.net_quantity = Some(2);
        held.avg_open_fill_price = Some(7_601.0);
        state.refreshing();
        state.apply(&RithmicMessage::InstrumentPnLPositionUpdate(held));
        state.refreshed();
        assert_eq!(state.book().positions[0].net, 2);
    }

    /// Today's orders as Rithmic Test answers a refresh with them: the
    /// exchange's notifications alone, flagged as a snapshot, for orders
    /// never heard of here. Each becomes an order as it ended, a fill with
    /// its fill.
    #[test]
    fn todays_orders_are_read_from_the_exchanges_snapshot() {
        let mut state = State::default();
        let told = |basket: &str, report: &str, fields: &dyn Fn(&mut ExchangeOrderNotification)| {
            let mut update = ExchangeOrderNotification::default();
            update.basket_id = Some(basket.into());
            update.report_type = Some(report.into());
            update.is_snapshot = Some(true);
            update.account_id = Some("951".into());
            update.symbol = Some("ESZ6".into());
            update.exchange = Some("CME".into());
            update.quantity = Some(2);
            update.transaction_type = Some(1);
            update.price_type = Some(2);
            fields(&mut update);
            RithmicMessage::ExchangeOrderNotification(update)
        };
        assert!(state.apply(&told("M1", "fill", &|n| {
            n.fill_size = Some(2);
            n.fill_price = Some(7_610.75);
            n.fill_id = Some("F1".into());
            n.total_unfilled_size = Some(0);
            n.ssboe = Some(100);
        })));
        assert!(state.apply(&told("L1", "cancel", &|n| {
            n.transaction_type = Some(2);
            n.price_type = Some(1);
            n.price = Some(7_700.0);
            n.ssboe = Some(200);
        })));
        assert!(state.apply(&told("R1", "reject", &|n| n.text = Some("price out of band".into()))));
        let book = state.book();
        let filled = book.orders.iter().find(|order| order.basket_id == "M1").unwrap();
        assert_eq!((filled.status.clone(), filled.filled, filled.fills.len()), (Status::Filled, 2, 1));
        let cancelled = book.orders.iter().find(|order| order.basket_id == "L1").unwrap();
        assert_eq!((cancelled.side, cancelled.kind, cancelled.price), (Side::Sell, Kind::Limit, Some(7_700.0)));
        assert_eq!(cancelled.status, Status::Cancelled);
        let rejected = book.orders.iter().find(|order| order.basket_id == "R1").unwrap();
        assert_eq!(rejected.status, Status::Rejected("price out of band".into()));
    }

    /// The risk system's limits and its liquidation level, which come in
    /// two answers, are kept together.
    #[test]
    fn limits_and_the_liquidation_level_are_kept_together() {
        let mut state = State::default();
        let mut info = ResponseAccountRmsInfo::default();
        info.loss_limit = Some(999_999.0);
        info.max_order_quantity = Some(40);
        info.buy_limit = Some(20);
        info.sell_limit = Some(20);
        assert!(state.limits("951", limits_of(&info)));
        let mut update = AccountRmsUpdates::default();
        update.account_id = Some("951".into());
        update.auto_liq_threshold_current_value = Some("48000.00".into());
        assert!(state.apply(&RithmicMessage::AccountRmsUpdates(update)));
        assert!(!state.limits("951", limits_of(&info)), "the same limits again");
        let limits = state.book().accounts[0].limits.clone().unwrap();
        assert_eq!(
            limits,
            Limits {
                loss_limit: Some(999_999.0),
                max_order_qty: Some(40),
                buy_limit: Some(20),
                sell_limit: Some(20),
                liquidate_at: Some(48_000.0)
            }
        );
    }

    #[test]
    fn dates_are_rithmics_and_in_utc() {
        assert_eq!(date_of(0), "19700101");
        let at = moment_of("20261104", "14:03:07").unwrap();
        assert_eq!(date_of(at), "20261104");
        assert_eq!(at % 86_400, 14 * 3_600 + 3 * 60 + 7);
        assert_eq!(moment_of("2026", "1"), None);
    }
}
