//! The neutral types: a login, an account, an order and what a broker says
//! of it. Every broker's session maps its own into these.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

pub use super::sizes::{Quantity, SizeTemplate};

/// One broker login, without its password.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct Login {
    /// The app's own name for it, which never changes. Its password is kept
    /// under this, and its accounts are known by it.
    pub id: String,
    /// What the trader calls it: "Lucid", "Tradeify", "Rithmic Test".
    pub name: String,
    /// The broker it signs in to, by the id the broker is listed under.
    pub broker: String,
    pub user: String,
    /// The rest of what its broker's dialog asks that is not secret, by
    /// name: which of the broker's systems, through which gateway.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub values: Vec<(String, String)>,
    /// Signed out: the password was forgotten, and nothing signs in with it
    /// until somebody types it again.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub signed_out: bool,
}

impl Login {
    /// What was typed for `key`, if anything.
    pub fn value(&self, key: &str) -> Option<&str> {
        self.values.iter().find(|(known, _)| known == key).map(|(_, value)| value.as_str())
    }
}

impl std::fmt::Debug for Login {
    /// Everything but the user ID, which is never printed.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Login")
            .field("id", &self.id)
            .field("name", &self.name)
            .field("broker", &self.broker)
            .field("values", &self.values)
            .field("signed_out", &self.signed_out)
            .finish_non_exhaustive()
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

pub(crate) fn grouped(value: u64) -> String {
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
    /// Past orders can be asked for, with [`Session::history`].
    pub history: bool,
    /// What is held and working now can be asked for, with
    /// [`Session::refresh`], rather than only heard of as it moves.
    pub snapshot: bool,
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
    /// It does, and which contract is not known yet: the session is asking,
    /// without stopping for the answer, and says so with a change.
    NotKnownYet,
}

/// Why a request to a broker did not go through, which decides what the
/// window shows for it: a refusal undoes what was drawn for it, and a
/// request nobody answered stays drawn as pending until the book says what
/// became of it, since it may well have arrived.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Failure {
    /// The broker answered no, or nothing was sent: in its words.
    Refused(String),
    /// Sent, and no answer came: a drop, a timeout, a session ending.
    Unanswered(String),
}

impl Failure {
    pub fn words(&self) -> &str {
        match self {
            Failure::Refused(why) | Failure::Unanswered(why) => why,
        }
    }
}

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.words())
    }
}

/// How a login stands, as its row and the header say it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Standing {
    /// Its trading session is signed in, in this run.
    Live,
    /// Not signed in, with its password kept: Sign in needs nothing typed.
    Ready,
    /// Signed out, or never signed in: there is no password to sign in with.
    SignedOut,
    /// The broker turned it down, or ended its session, in these words.
    Refused(String),
}

impl Standing {
    /// The words for its row: "live", "not signed in", the broker's own.
    pub fn words(&self) -> String {
        match self {
            Standing::Live => "live".into(),
            Standing::Ready => "not signed in".into(),
            Standing::SignedOut => "not signed in".into(),
            Standing::Refused(why) => why.clone(),
        }
    }

    /// What the header says about an account in a login that stands so,
    /// when it is in trouble.
    pub fn trouble(&self) -> Option<String> {
        match self {
            Standing::Live | Standing::Ready => None,
            Standing::SignedOut => Some("signed out".into()),
            Standing::Refused(why) => Some(why.clone()),
        }
    }
}

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

/// How far an order has got, whichever broker it is with: each session
/// maps its own words into these.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    /// Sent, not yet working.
    Pending,
    Working,
    PartiallyFilled,
    Filled,
    Cancelled,
    Rejected,
}

impl Stage {
    /// Still to fill or be cancelled, so it can be cancelled.
    pub fn working(self) -> bool {
        matches!(self, Stage::Pending | Stage::Working | Stage::PartiallyFilled)
    }
}

/// Where an order stands: its [`Stage`], and what the broker last said of
/// it in its own words, whatever the stage: why it was rejected, who
/// cancelled it.
#[derive(Clone, Debug, PartialEq)]
pub struct Status {
    pub stage: Stage,
    pub said: Option<String>,
}

impl Status {
    pub fn said(stage: Stage, said: impl Into<String>) -> Status {
        Status { stage, said: Some(said.into()) }
    }

    /// Still to fill or be cancelled, so it can be cancelled.
    pub fn working(&self) -> bool {
        self.stage.working()
    }

    pub fn word(&self) -> &'static str {
        match self.stage {
            Stage::Pending => "Pending",
            Stage::Working => "Working",
            Stage::PartiallyFilled => "Part filled",
            Stage::Filled => "Filled",
            Stage::Cancelled => "Cancelled",
            Stage::Rejected => "Rejected",
        }
    }
}

impl From<Stage> for Status {
    fn from(stage: Stage) -> Status {
        Status { stage, said: None }
    }
}

impl PartialEq<Stage> for Status {
    fn eq(&self, stage: &Stage) -> bool {
        self.stage == *stage
    }
}

/// An account's money, as its broker pushed it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Money {
    /// [`Account::key`] on the desk; the number inside a session.
    pub key: String,
    pub balance: Option<f64>,
    /// Open now.
    pub unrealized: Option<f64>,
    /// Today's, closed and open, as the broker counts it: what a daily loss
    /// limit is held to.
    pub day_pnl: Option<f64>,
    /// What the broker's risk system holds the account to, once it has said.
    pub limits: Option<Limits>,
}

/// What a broker's risk system holds an account to, each `None` where it
/// sets no limit or does not say.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Limits {
    /// How far today's P&L may fall before the account stops trading.
    pub loss_limit: Option<f64>,
    /// The most one order may be for.
    pub max_order_qty: Option<u32>,
    /// The most that may be held long, and short.
    pub buy_limit: Option<u32>,
    pub sell_limit: Option<u32>,
    /// The balance below which the account is liquidated, as it stands now.
    pub liquidate_at: Option<f64>,
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

/// An order the broker has told of, at any stage.
#[derive(Clone, Debug, PartialEq)]
pub struct Order {
    /// The account's key on the desk; its number inside a session.
    pub account: String,
    pub basket_id: String,
    pub contract: String,
    pub exchange: String,
    pub side: Side,
    pub qty: u32,
    /// How much of it has filled so far, as the broker counts it.
    pub filled: u32,
    pub kind: OrderKind,
    pub price: Option<f64>,
    pub status: Status,
    /// The average price it filled at, as the broker works it out.
    pub fill_price: Option<f64>,
    /// Each fill, oldest first, when the broker tells them one by one.
    pub fills: Vec<Fill>,
    /// Unix seconds.
    pub at: i64,
    pub tag: Option<String>,
    /// The step its price moves in, when the broker knows it.
    pub tick: Option<f64>,
}

/// Part or all of an order, filled.
#[derive(Clone, Debug, PartialEq)]
pub struct Fill {
    /// The order it is part of, by the broker's id.
    pub basket: String,
    /// Unix seconds.
    pub at: i64,
    pub qty: u32,
    pub price: f64,
}

/// What every live session knows of orders, positions and money.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Book {
    pub accounts: Vec<Money>,
    pub positions: Vec<Position>,
    /// Newest first.
    pub orders: Vec<Order>,
    /// What each live login's session can do, by login id.
    pub capabilities: HashMap<String, Capabilities>,
    /// What the session holds now has been asked for and has come back
    /// ([`Session::refresh`]), so this is all of it and not only what moved
    /// since signing in. On the desk's book, true when it is so for every
    /// live session.
    pub refreshed: bool,
}

impl Book {
    /// What the session of the account with the key `account` can do.
    pub fn capabilities_of(&self, account: &str) -> Option<&Capabilities> {
        self.capabilities.get(account.split_once('/').map_or(account, |(login, _)| login))
    }

    pub fn money(&self, key: &str) -> Option<&Money> {
        self.accounts.iter().find(|money| money.key == key)
    }

    /// `account`'s position in `contract`, while one is held.
    pub fn position(&self, account: &str, contract: &str) -> Option<&Position> {
        self.positions.iter().find(|held| held.account == account && held.contract == contract && held.net != 0)
    }

    /// `account`'s working orders in `contract`.
    pub fn working<'a>(&'a self, account: &'a str, contract: &'a str) -> impl Iterator<Item = &'a Order> {
        self.orders
            .iter()
            .filter(move |order| order.account == account && order.contract == contract && order.status.working())
    }

    pub fn order(&self, basket: &str) -> Option<&Order> {
        self.orders.iter().find(|order| order.basket_id == basket)
    }
}

/// An order to place: one contract, good for the day, entered by hand.
#[derive(Clone, Debug, PartialEq)]
pub struct OrderRequest {
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

impl OrderRequest {
    /// The order a trader asks for, before it goes to a broker: `quantity`
    /// of `contract` on the account numbered `account`, `side` and `kind`,
    /// at `price` for a limit or a stop and at the market for a market
    /// order. Refused in words when the price and the kind disagree: a
    /// limit with no price is not an order.
    pub fn request(
        account: &str,
        contract: &Contract,
        side: Side,
        kind: OrderKind,
        quantity: Quantity,
        price: Option<f64>,
    ) -> Result<OrderRequest, String> {
        let price = match (kind, price) {
            (OrderKind::Market, _) => None,
            (_, Some(price)) if price.is_finite() => Some(price),
            (_, _) => return Err(format!("a {} order needs a price", kind.key())),
        };
        Ok(OrderRequest {
            account: account.to_string(),
            contract: contract.symbol.clone(),
            exchange: contract.exchange.clone(),
            side,
            qty: quantity.get(),
            kind,
            price,
            tag: new_tag(),
        })
    }
}

/// A fresh tag for an order placed now.
pub fn new_tag() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|since| since.as_nanos())
        .unwrap_or_default();
    format!("omacharts:{nanos}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trading::fake::tests::*;

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

    /// What a trader asks for becomes an order the same way from a key, a
    /// menu or a drag: the account, the contract and its exchange, the side,
    /// the kind and the size, with a price only where the kind takes one.
    #[test]
    fn an_order_request_is_built_from_what_was_asked() {
        let contract = Contract { symbol: "ESZ6".into(), exchange: "CME".into(), tick: Some(0.25) };
        let two = Quantity::new(2).unwrap();
        let market = OrderRequest::request("951", &contract, Side::Buy, OrderKind::Market, two, Some(7_600.0)).unwrap();
        assert_eq!(
            (market.account.as_str(), market.contract.as_str(), market.exchange.as_str()),
            ("951", "ESZ6", "CME")
        );
        assert_eq!((market.side, market.kind, market.qty, market.price), (Side::Buy, OrderKind::Market, 2, None));
        assert!(market.tag.starts_with("omacharts:"));
        let limit = OrderRequest::request("951", &contract, Side::Sell, OrderKind::Limit, two, Some(7_620.0)).unwrap();
        assert_eq!((limit.kind, limit.price), (OrderKind::Limit, Some(7_620.0)));
        assert_eq!(
            OrderRequest::request("951", &contract, Side::Sell, OrderKind::Stop, two, None),
            Err("a stop order needs a price".to_string())
        );
        assert!(OrderRequest::request("951", &contract, Side::Sell, OrderKind::Limit, two, Some(f64::NAN)).is_err());
    }
}
