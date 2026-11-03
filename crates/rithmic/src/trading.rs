//! A login's trading session: the order plant, which lists the login's
//! accounts, and the PnL plant, which pushes their balances.
//!
//! Apart from [`Client`](crate::Client), which is market data: only one
//! login a process has supplies that, and every login can trade. They are
//! different plants, so the data session and a trading session on the same
//! login sit side by side. What must not happen is two of either for one
//! login, because Rithmic signs the first out when the same login signs in
//! to a plant again; keeping to one is the caller's, which holds them by
//! login.
//!
//! The order plant also places, cancels and flattens, and pushes what
//! becomes of every order; the PnL plant pushes positions and balances.
//! Both land in one [`Book`] the session keeps, which a caller reads
//! whenever `changed` says something moved. Nothing here polls.
//!
//! Signed in once and kept until [`Trading::sign_out`]. It does not sign in
//! again by itself after a drop, unlike the data session: an order is never
//! sent again behind a trader's back, and a trader signing in again is one
//! press.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rithmic_rs::rti::messages::RithmicMessage;
use rithmic_rs::rti::{
    AccountPnLPositionUpdate, ExchangeOrderNotification, InstrumentPnLPositionUpdate, ResponseAccountList,
    RithmicOrderNotification,
};
use rithmic_rs::{
    ConnectStrategy, ManualOrAutoEntry, OrderSide, OrderStatus, OrderType, RithmicAccount, RithmicCancelAllOrders,
    RithmicCancelOrder, RithmicExitPosition, RithmicOrder, RithmicOrderPlant, RithmicOrderPlantHandle,
    RithmicPnlPlant, RithmicPnlPlantHandle, RithmicResponse, TimeInForce,
};
use tokio::sync::broadcast::error::RecvError;

use crate::client::{config, error_of, signed_in, within};
use crate::{App, Error, Login, Side};

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
    Filled,
    Cancelled,
    /// Refused, in Rithmic's words.
    Rejected(String),
}

impl Status {
    /// Still to fill or be cancelled.
    pub fn working(&self) -> bool {
        matches!(self, Status::Pending | Status::Working)
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
    pub kind: Kind,
    pub price: Option<f64>,
    pub status: Status,
    /// The average price it filled at, once it has.
    pub fill_price: Option<f64>,
    /// When Rithmic last said anything of it, in unix seconds.
    pub at: i64,
    pub tag: Option<String>,
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
            kind: order.kind,
            price: order.price,
            status: Status::Pending,
            fill_price: None,
            at,
            tag: Some(order.tag.clone()),
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
            kind: Kind::Market,
            price: None,
            status: Status::Pending,
            fill_price: None,
            at: 0,
            tag: None,
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
        if let Some(tag) = given(&update.user_tag) {
            order.tag = Some(tag);
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
        match update.report_type.as_deref().map(str::to_ascii_lowercase).as_deref() {
            Some("fill") => {
                if let Some(price) = update.avg_fill_price.or(update.fill_price).filter(|price| *price != 0.0) {
                    order.fill_price = Some(price);
                }
                if update.total_unfilled_size == Some(0) {
                    order.status = Status::Filled;
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

type Changed = Arc<dyn Fn() + Send + Sync>;

struct Inner {
    handle: tokio::runtime::Handle,
    runtime: Mutex<Option<tokio::runtime::Runtime>>,
    order: RithmicOrderPlantHandle,
    pnl: RithmicPnlPlantHandle,
    plants: Mutex<Option<(RithmicOrderPlant, RithmicPnlPlant)>>,
    accounts: Vec<Account>,
    /// One order handle per account, by number: an order names its account
    /// through the handle it is sent on.
    desks: HashMap<String, RithmicOrderPlantHandle>,
    state: Arc<Mutex<State>>,
    /// Why the session ended by itself: a drop, or Rithmic signing it out.
    trouble: Arc<Mutex<Option<Error>>>,
    closed: AtomicBool,
}

impl Drop for Inner {
    fn drop(&mut self) {
        if let Some(runtime) = self.runtime.get_mut().unwrap_or_else(|e| e.into_inner()).take() {
            runtime.shutdown_background();
        }
    }
}

/// A login's trading session. Cheap to clone; every clone is the same
/// session.
#[derive(Clone)]
pub struct Trading {
    inner: Arc<Inner>,
}

impl Trading {
    /// Sign `login` in to the order and PnL plants, as `app`, list its
    /// accounts, and ask for their working orders, positions and balances.
    /// Blocks until Rithmic has answered, so it is for a thread of its own.
    ///
    /// `changed` is called on the session's thread whenever an order,
    /// position or balance moves, or the session ends by itself.
    pub fn sign_in(app: App, login: &Login, changed: impl Fn() + Send + Sync + 'static) -> Result<Trading, Error> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .thread_name("rithmic-trading")
            .enable_all()
            .build()
            .map_err(|error| Error::Connection(error.to_string()))?;
        let changed: Changed = Arc::new(changed);
        let signed = runtime.block_on(connect(app, login));
        let (order_plant, pnl_plant, order, pnl, accounts) = match signed {
            Ok(signed) => signed,
            Err(error) => {
                runtime.shutdown_background();
                return Err(error);
            }
        };
        let state = Arc::new(Mutex::new(State::default()));
        let trouble = Arc::new(Mutex::new(None));
        runtime.spawn(listen(
            pnl_plant.subscribe_all(),
            order_plant.subscribe_all(),
            state.clone(),
            trouble.clone(),
            changed,
        ));
        // What is working, held and banked comes on the updates the listener
        // now hears; the requests only acknowledge.
        let desks: HashMap<String, RithmicOrderPlantHandle> = accounts
            .iter()
            .map(|account| (account.id.clone(), order_plant.get_handle(&rithmic_account(account))))
            .collect();
        runtime.block_on(async {
            for account in &accounts {
                if let Some(desk) = desks.get(&account.id) {
                    let _ = within(REQUEST, desk.subscribe_order_updates()).await;
                    let _ = within(REQUEST, desk.show_orders()).await;
                }
                let handle = pnl_plant.get_handle(&rithmic_account(account));
                let _ = within(REQUEST, handle.subscribe_pnl_updates()).await;
                let _ = within(REQUEST, handle.get_pnl_position_snapshot()).await;
            }
        });
        Ok(Trading {
            inner: Arc::new(Inner {
                handle: runtime.handle().clone(),
                runtime: Mutex::new(Some(runtime)),
                order,
                pnl,
                plants: Mutex::new(Some((order_plant, pnl_plant))),
                accounts,
                desks,
                state,
                trouble,
                closed: AtomicBool::new(false),
            }),
        })
    }

    /// The login's accounts, as the order plant listed them at sign-in.
    pub fn accounts(&self) -> Vec<Account> {
        self.inner.accounts.clone()
    }

    /// The balance of the account `id`, once the PnL plant has said.
    pub fn balance(&self, id: &str) -> Option<f64> {
        let state = self.inner.state.lock().unwrap_or_else(|e| e.into_inner());
        state.accounts.iter().find(|known| known.account == id).and_then(|known| known.balance)
    }

    /// Every order, position and balance the session has heard of.
    pub fn book(&self) -> Book {
        self.inner.state.lock().unwrap_or_else(|e| e.into_inner()).book()
    }

    /// Why the session is no longer signed in, if it ended by itself.
    pub fn trouble(&self) -> Option<Error> {
        if self.inner.closed.load(Ordering::Acquire) {
            return Some(Error::Closed);
        }
        self.inner.trouble.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// The order handle for `account`, while the session is signed in.
    fn desk(&self, account: &str) -> Result<&RithmicOrderPlantHandle, Error> {
        if let Some(trouble) = self.trouble() {
            return Err(trouble);
        }
        self.inner.desks.get(account).ok_or_else(|| Error::NotFound("such account on this login".into()))
    }

    /// Place `order`, blocking until Rithmic has taken it, and return its
    /// basket id. Taken is not filled: what becomes of it arrives on the
    /// book. Never sent twice: a drop on the way is an error, not a retry.
    pub fn place(&self, order: &Order) -> Result<String, Error> {
        let desk = self.desk(&order.account)?;
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
        let replies = self.inner.handle.block_on(within(REQUEST, desk.place_order(built)))?;
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
        self.inner.state.lock().unwrap_or_else(|e| e.into_inner()).placed(order, &basket, now());
        Ok(basket)
    }

    /// Cancel the order `basket` on `account`. What becomes of it arrives
    /// on the book.
    pub fn cancel(&self, account: &str, basket: &str) -> Result<(), Error> {
        let desk = self.desk(account)?;
        let cancel =
            RithmicCancelOrder::new().id(basket).manual_or_auto(ManualOrAutoEntry::Manual).build().map_err(error_of)?;
        let replies = self.inner.handle.block_on(within(REQUEST, desk.cancel_order(cancel)))?;
        match replies.iter().find_map(|reply| reply.error.clone()) {
            Some(error) => Err(error_of(error)),
            None => Ok(()),
        }
    }

    /// Cancel every working order on `account`.
    pub fn cancel_all(&self, account: &str) -> Result<(), Error> {
        let desk = self.desk(account)?;
        let cancel = RithmicCancelAllOrders::new().manual_or_auto(ManualOrAutoEntry::Manual).build().map_err(error_of)?;
        self.inner.handle.block_on(within(REQUEST, desk.cancel_all_orders(cancel))).map(|_| ())
    }

    /// Close `account`'s position in `contract`: one request, and Rithmic
    /// places the closing order itself.
    pub fn flatten(&self, account: &str, contract: &str, exchange: &str) -> Result<(), Error> {
        let desk = self.desk(account)?;
        let exit = RithmicExitPosition::new()
            .symbol(contract)
            .exchange(exchange)
            .manual_or_auto(ManualOrAutoEntry::Manual)
            .build()
            .map_err(error_of)?;
        let replies = self.inner.handle.block_on(within(REQUEST, desk.exit_position(exit)))?;
        match replies.iter().find_map(|reply| reply.error.clone()) {
            Some(error) => Err(error_of(error)),
            None => Ok(()),
        }
    }

    /// Sign out of both plants, waiting a few seconds at most.
    pub fn sign_out(&self) {
        if self.inner.closed.swap(true, Ordering::AcqRel) {
            return;
        }
        let plants = self.inner.plants.lock().unwrap_or_else(|e| e.into_inner()).take();
        let order = &self.inner.order;
        let pnl = &self.inner.pnl;
        self.inner.handle.block_on(async {
            let _ = within(Duration::from_secs(2), order.disconnect()).await;
            let _ = within(Duration::from_secs(2), pnl.disconnect()).await;
            order.abort();
            pnl.abort();
            if let Some((order_plant, pnl_plant)) = plants {
                let _ = tokio::time::timeout(Duration::from_secs(3), order_plant.await_shutdown()).await;
                let _ = tokio::time::timeout(Duration::from_secs(3), pnl_plant.await_shutdown()).await;
            }
        });
    }
}

fn rithmic_account(account: &Account) -> RithmicAccount {
    RithmicAccount::new(&account.fcm, &account.ib, &account.id)
}

type Signed = (RithmicOrderPlant, RithmicPnlPlant, RithmicOrderPlantHandle, RithmicPnlPlantHandle, Vec<Account>);

async fn connect(app: App, login: &Login) -> Result<Signed, Error> {
    let credentials = login.credentials().await?;
    let config = config(app, &credentials)?;
    drop(credentials);

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
    let accounts = accounts_of(&listed)?;
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
    Ok((order_plant, pnl_plant, order, pnl, accounts))
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

/// Hear orders, positions and balances, and the session ending, until it
/// does.
async fn listen(
    mut pnl: tokio::sync::broadcast::Receiver<RithmicResponse>,
    mut order: tokio::sync::broadcast::Receiver<RithmicResponse>,
    state: Arc<Mutex<State>>,
    trouble: Arc<Mutex<Option<Error>>>,
    changed: Changed,
) {
    let ended = |error: Error| {
        *trouble.lock().unwrap_or_else(|e| e.into_inner()) = Some(error);
        changed();
    };
    loop {
        let update = tokio::select! {
            update = pnl.recv() => update,
            update = order.recv() => update,
        };
        let update = match update {
            Ok(update) => update,
            Err(RecvError::Lagged(_)) => continue,
            Err(RecvError::Closed) => return ended(Error::Connection("the session closed".into())),
        };
        match &update.message {
            RithmicMessage::ForcedLogout(_) => {
                return ended(Error::Refused(
                    "Rithmic signed this session out, as it does when the same login signs in somewhere else"
                        .into(),
                ));
            }
            RithmicMessage::ConnectionError => {
                return ended(Error::Connection(match &update.error {
                    Some(error) => error.to_string(),
                    None => "the connection dropped".into(),
                }));
            }
            message => {
                let moved = state.lock().unwrap_or_else(|e| e.into_inner()).apply(message);
                if moved {
                    changed();
                }
            }
        }
    }
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
