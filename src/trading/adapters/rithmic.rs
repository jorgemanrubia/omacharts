//! Rithmic's order and PnL plants, as a [`Broker`] and its [`Session`].

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use omacharts_engine::providers::rithmic::session::{self, GATEWAY, SYSTEM};
use omacharts_engine::specs::{self, root_of};

use crate::trading::*;

/// How long closing waits for the trading sessions to sign out: what the
/// data session waits.
pub const SIGN_OUT_WAIT: std::time::Duration = rithmic::SIGN_OUT_WAIT;

fn system_of(login: &Login) -> &str {
    login.value(SYSTEM).filter(|system| !system.is_empty()).unwrap_or(rithmic::TEST_SYSTEM)
}

fn gateway_of(login: &Login) -> &str {
    login.value(GATEWAY).filter(|gateway| !gateway.is_empty()).unwrap_or(rithmic::TEST_URL)
}

/// The login as the Rithmic crate signs in with it: the password from the
/// keyring, under the login's id.
fn signing_of(login: &Login) -> rithmic::Login {
    rithmic::Login::Keyring {
        login: login.id.clone(),
        user: login.user.clone(),
        system: system_of(login).to_string(),
        url: gateway_of(login).to_string(),
    }
}

/// Rithmic's order and PnL plants: the trading half of the login's one
/// `rithmic::Connection`.
pub struct Rithmic;

impl Broker for Rithmic {
    fn id(&self) -> &'static str {
        omacharts_engine::providers::rithmic::LISTED.id
    }

    fn label(&self) -> &'static str {
        omacharts_engine::providers::rithmic::LISTED.label
    }

    /// Its system: "Rithmic Test", "LucidTrading".
    fn signs_in_to(&self, login: &Login) -> Option<String> {
        Some(system_of(login).to_string())
    }

    fn keyring(&self) -> Option<&'static dyn Secrets> {
        Some(&session::Keyring)
    }

    /// Under the login's id, with its user and system beside it for
    /// `secret-tool`; with a development login, nowhere.
    fn keep_password(&self, secrets: &dyn Secrets, login: &Login, password: &str) -> Result<(), String> {
        session::save_password(secrets, &login.id, &login.user, system_of(login), password)
    }

    fn forget_password(&self, secrets: &dyn Secrets, login: &Login, data: bool) -> Result<(), String> {
        match data {
            true => session::sign_out_login(secrets, &login.id, &login.user, system_of(login)),
            false => session::forget_password(secrets, &login.id, &login.user, system_of(login)),
        }
    }

    fn sign_in(&self, login: &Login, changed: Box<dyn Fn(Change) + Send + Sync>) -> Result<Arc<dyn Session>, String> {
        let app = session::app().map_err(|error| error.to_string())?;
        // A developer's login file wins over the keyring, which it never
        // touches, as it does for the data session.
        let (system, gateway, saved) =
            (system_of(login).to_string(), gateway_of(login).to_string(), signing_of(login));
        let source = move || rithmic::Login::from_env(&system, &gateway).unwrap_or_else(|| Ok(saved.clone()));
        // The login's one connection, which its data session shares when it
        // is the data login. Any pulse is the window's cue to read the book
        // and how the session stands again; coming back after a drop is the
        // cue to refresh it too.
        session::hear_specs();
        let connection = rithmic::connection(app, &login.id, source).map_err(said)?;
        let changed: Arc<dyn Fn(Change) + Send + Sync> = Arc::from(changed);
        let told = changed.clone();
        let trading = connection
            .trading(move |pulse| match pulse {
                rithmic::Pulse::Back => told(Change::Back),
                rithmic::Pulse::Moved | rithmic::Pulse::Lost(_) => told(Change::Moved),
            })
            .map_err(said)?;
        // A root's front month moving, whether asked for or pushed by
        // Rithmic at the roll, is the window's cue to ask again where an
        // order goes; only while this session lives.
        let alive = Arc::new(AtomicBool::new(true));
        {
            let (alive, changed) = (alive.clone(), changed.clone());
            rithmic::on_front_month(move |_, _, _| {
                if alive.load(Ordering::Acquire) {
                    changed(Change::Moved);
                }
            });
        }
        Ok(Arc::new(RithmicSession {
            login: login.id.clone(),
            simulated: system_of(login) == rithmic::TEST_SYSTEM,
            trading,
            changed,
            connection,
            prices: Mutex::default(),
            held: Arc::default(),
            told: Arc::default(),
            alive,
        }))
    }
}

/// The step a Rithmic contract's price moves in, from its root's spec.
fn rithmic_tick(contract: &str) -> Option<f64> {
    specs::tick(root_of(contract))
}

/// What a point of a Rithmic contract's price is worth, from its root's spec.
fn rithmic_point(contract: &str) -> Option<f64> {
    specs::point(root_of(contract))
}

/// Rithmic's words, without "Rithmic refused…" in front of a refusal.
fn said(error: rithmic::Error) -> String {
    match error {
        rithmic::Error::Refused(why) | rithmic::Error::Rejected(why) => why,
        error => error.to_string(),
    }
}

/// A request's failure, as the window needs it: a drop, a timeout or a
/// session ending on the way is no answer, not a no.
fn failed(error: rithmic::Error) -> Failure {
    match error {
        rithmic::Error::Connection(_) | rithmic::Error::Timeout | rithmic::Error::Closed => {
            Failure::Unanswered(said(error))
        }
        error => Failure::Refused(said(error)),
    }
}

/// The price an exchange stopped a rejected order at, from Rithmic's words:
/// a band around the market, "Order price is outside bands 'Ask of
/// 748000.00 violates Low Band 752125.00 using Delta .00, CrvBnd equals
/// 754125.00'", or the day's price limit, "Order price is outside the
/// limits 'Bid of 694075.00 violates …'". Those numbers come scaled up by
/// a power of ten; the order's own `price` gives the scale.
fn bound_of(why: &str, price: f64) -> Option<Bound> {
    let first_number = |rest: &str| -> Option<(usize, f64)> {
        let start = rest.find(|c: char| c.is_ascii_digit())?;
        let digits = rest[start..].split(|c: char| !(c.is_ascii_digit() || c == '.')).next()?;
        Some((start, digits.parse().ok()?))
    };
    let (_, asked) = first_number(&why[why.find(" of ")? + 4..])?;
    let violates = &why[why.find("violates")? + "violates".len()..];
    let (named, stopped) = first_number(violates)?;
    let kind = match &violates[..named] {
        words if words.contains("Band") => BoundKind::Band,
        _ if why.to_ascii_lowercase().contains("limit") => BoundKind::DailyLimit,
        _ => return None,
    };
    if price <= 0.0 || asked <= 0.0 {
        return None;
    }
    let scale = 10f64.powi((asked / price).log10().round() as i32);
    Some(Bound { price: stopped / scale, kind })
}

/// An order as Rithmic told of it, in the neutral words.
fn order_of(order: rithmic::WorkingOrder) -> Order {
    Order {
        tick: rithmic_tick(&order.contract),
        account: order.account,
        contract: order.contract,
        exchange: order.exchange,
        side: match order.side {
            rithmic::Side::Buy => Side::Buy,
            rithmic::Side::Sell => Side::Sell,
        },
        qty: order.qty,
        filled: order.filled,
        kind: match order.kind {
            rithmic::Kind::Market => OrderKind::Market,
            rithmic::Kind::Limit => OrderKind::Limit,
            rithmic::Kind::Stop => OrderKind::Stop,
        },
        price: order.price,
        status: match order.status {
            rithmic::Status::Pending => Status { stage: Stage::Pending, said: order.said, bound: None },
            rithmic::Status::Working => Status { stage: Stage::Working, said: order.said, bound: None },
            rithmic::Status::PartiallyFilled => Status { stage: Stage::PartiallyFilled, said: order.said, bound: None },
            rithmic::Status::Filled => Status { stage: Stage::Filled, said: order.said, bound: None },
            rithmic::Status::Cancelled => Status { stage: Stage::Cancelled, said: order.said, bound: None },
            rithmic::Status::Rejected(why) => {
                Status { stage: Stage::Rejected, bound: order.price.and_then(|price| bound_of(&why, price)), said: Some(why) }
            }
        },
        fill_price: order.fill_price,
        fills: order
            .fills
            .into_iter()
            .map(|fill| Fill { basket: fill.basket_id, at: fill.at, qty: fill.qty, price: fill.price })
            .collect(),
        basket_id: order.basket_id,
        at: order.at,
        tag: order.tag,
    }
}

struct RithmicSession {
    login: String,
    /// Signed in to Rithmic Test, which fills against a simulation.
    simulated: bool,
    trading: rithmic::Trading,
    /// What the window is told when the book moves, or when a contract
    /// asked for is known.
    changed: Arc<dyn Fn(Change) + Send + Sync>,
    connection: rithmic::Connection,
    /// The last trade on each contract an order was asked about or a
    /// position is held in, by contract, heard on the login's own data
    /// half.
    prices: Mutex<HashMap<String, Priced>>,
    /// The contracts a position is held in, as the book last said: a new
    /// price on one of them is told, so the position is marked to it.
    held: Arc<Mutex<HashSet<String>>>,
    /// A new price was told and the book not read since: told once until
    /// it is, however busy the market.
    told: Arc<AtomicBool>,
    /// Cleared when the session signs out, so that a front month heard
    /// after is nobody's news.
    alive: Arc<AtomicBool>,
}

/// A contract's trades being watched for their price.
struct Priced {
    _watch: rithmic::Watch,
    last: Arc<Mutex<Option<f64>>>,
    /// The watch ended for good: closed, or refused. Watched afresh when
    /// asked again.
    ended: Arc<AtomicBool>,
}

impl RithmicSession {
    /// Start hearing `contract`'s trades, unless they are heard already.
    /// The contract itself, never a root's front month: a position in a
    /// month that has rolled off is marked to that month's price.
    fn watch_price(&self, contract: &str, exchange: &str) {
        let mut prices = self.prices.lock().unwrap_or_else(|e| e.into_inner());
        if prices.get(contract).is_some_and(|priced| !priced.ended.load(Ordering::Acquire)) {
            return;
        }
        let last = Arc::new(Mutex::new(None));
        let ended = Arc::new(AtomicBool::new(false));
        let watch = self.connection.data().watch_contract(contract, exchange, {
            let (last, ended) = (last.clone(), ended.clone());
            let (held, told, changed, contract) =
                (self.held.clone(), self.told.clone(), self.changed.clone(), contract.to_string());
            move |live| match live {
                // The last trade before the watch began is the market's
                // price until it trades again.
                rithmic::Live::Trade(trade) | rithmic::Live::Last(trade) => {
                    let before = last.lock().unwrap_or_else(|e| e.into_inner()).replace(trade.price);
                    let holding = held.lock().unwrap_or_else(|e| e.into_inner()).contains(&contract);
                    if before != Some(trade.price) && holding && !told.swap(true, Ordering::AcqRel) {
                        changed(Change::Priced);
                    }
                }
                rithmic::Live::Lost(rithmic::Error::Connection(_) | rithmic::Error::Timeout) => {}
                rithmic::Live::Lost(_) => ended.store(true, Ordering::Release),
                rithmic::Live::Watching { .. } => {}
            }
        });
        prices.insert(contract.to_string(), Priced { _watch: watch, last, ended });
    }

    /// The contract `instrument` trades as: a dated contract itself, a root
    /// its front month, when a data session has found it; `None` until one
    /// has, with `found` called once it is known.
    fn contract_of(&self, instrument: &omacharts_engine::Instrument, found: impl FnOnce() + Send + 'static) -> Option<String> {
        match instrument.is_contract() {
            true => Some(instrument.symbol.clone()),
            false => self.trading.front_month(&instrument.symbol, exchange_of(instrument), found),
        }
    }
}

/// The exchange an instrument's orders go to.
fn exchange_of(instrument: &omacharts_engine::Instrument) -> &'static str {
    omacharts_engine::providers::rithmic::exchange_of(instrument.root())
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
            })
            .collect()
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            kinds: vec![OrderKind::Market, OrderKind::Limit, OrderKind::Stop],
            flatten: true,
            cancel_all: true,
            modify: true,
            link: true,
            history: true,
            snapshot: true,
        }
    }

    fn is_simulated(&self) -> bool {
        self.simulated
    }

    /// A futures root on a CME group exchange, as its front month: the one
    /// a data session last found, whatever feeds the chart, or asked for now
    /// and told with a change once it is known. A dated contract as itself.
    fn contract_for(&self, instrument: &omacharts_engine::Instrument) -> Result<Contract, Refusal> {
        if instrument.kind != omacharts_engine::InstrumentKind::FutureRoot {
            return Err(Refusal::NotTradable);
        }
        let exchange = exchange_of(instrument);
        let changed = self.changed.clone();
        let symbol = self.contract_of(instrument, move || changed(Change::Moved)).ok_or(Refusal::NotKnownYet)?;
        // Its price from here on, for judging an order on a chart fed from
        // elsewhere: only contracts somebody charts, as only they are asked.
        self.watch_price(&symbol, exchange);
        Ok(Contract { tick: rithmic_tick(&symbol), symbol, exchange: exchange.to_string() })
    }

    fn last_price(&self, instrument: &omacharts_engine::Instrument) -> Option<f64> {
        let contract = self.contract_of(instrument, || {})?;
        let prices = self.prices.lock().unwrap_or_else(|e| e.into_inner());
        let priced = prices.get(&contract)?;
        *priced.last.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn price_limits(&self, instrument: &omacharts_engine::Instrument) -> (Option<f64>, Option<f64>) {
        match self.contract_of(instrument, || {}) {
            Some(contract) => rithmic::price_limits(&contract, exchange_of(instrument)),
            None => (None, None),
        }
    }

    /// The contract's own last trade, heard on the login's own data half:
    /// a position in a month that has rolled off is marked to that month.
    fn price_of(&self, contract: &str) -> Option<f64> {
        self.watch_price(contract, omacharts_engine::providers::rithmic::exchange_of(contract));
        let prices = self.prices.lock().unwrap_or_else(|e| e.into_inner());
        *prices.get(contract)?.last.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn book(&self) -> Book {
        let book = self.trading.book();
        // Read from here on: the next new price is told again.
        self.told.store(false, Ordering::Release);
        *self.held.lock().unwrap_or_else(|e| e.into_inner()) =
            book.positions.iter().filter(|held| held.net != 0).map(|held| held.contract.clone()).collect();
        Book {
            capabilities: HashMap::new(),
            refreshed: book.refreshed,
            accounts: book
                .accounts
                .into_iter()
                .map(|money| Money {
                    key: money.account,
                    balance: money.balance,
                    day_pnl: money.day_pnl.or(match (money.realized_day, money.unrealized) {
                        (Some(closed), Some(open)) => Some(closed + open),
                        (closed, open) => closed.or(open),
                    }),
                    unrealized: money.unrealized,
                    limits: money.limits.map(|limits| Limits {
                        loss_limit: limits.loss_limit,
                        max_order_qty: limits.max_order_qty,
                        buy_limit: limits.buy_limit,
                        sell_limit: limits.sell_limit,
                        liquidate_at: limits.liquidate_at,
                    }),
                })
                .collect(),
            positions: book
                .positions
                .into_iter()
                .map(|held| Position {
                    tick: rithmic_tick(&held.contract),
                    point: rithmic_point(&held.contract),
                    account: held.account,
                    contract: held.contract,
                    exchange: held.exchange,
                    net: held.net,
                    avg: held.avg,
                    open_pnl: held.open_pnl,
                })
                .collect(),
            orders: book.orders.into_iter().map(order_of).collect(),
        }
    }

    fn refresh(&self) -> Result<(), String> {
        self.trading.refresh().map_err(said)
    }

    fn history(&self, account: &str, from: i64, to: i64) -> Result<Vec<Order>, Failure> {
        let orders = self.trading.history(account, from, to).map_err(failed)?;
        Ok(orders.into_iter().map(order_of).collect())
    }

    fn place(&self, order: &OrderRequest) -> Result<String, Failure> {
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
        self.trading.place(&placed).map_err(failed)
    }

    fn cancel(&self, account: &str, basket: &str) -> Result<(), Failure> {
        self.trading.cancel(account, basket).map_err(failed)
    }

    fn modify(&self, account: &str, basket: &str, price: f64) -> Result<(), Failure> {
        self.trading.modify(account, basket, price).map_err(failed)
    }

    fn link(&self, account: &str, baskets: &[String]) -> Result<(), Failure> {
        self.trading.link(account, baskets).map_err(failed)
    }

    fn cancel_all(&self, account: &str) -> Result<(), Failure> {
        self.trading.cancel_all(account).map_err(failed)
    }

    fn flatten(&self, account: &str, contract: &str, exchange: &str) -> Result<(), Failure> {
        self.trading.flatten(account, contract, exchange).map_err(failed)
    }

    fn trouble(&self) -> Option<String> {
        self.trading.trouble().map(|error| match error {
            rithmic::Error::Refused(why) => why,
            error => error.to_string(),
        })
    }

    fn sign_out(&self) {
        self.alive.store(false, Ordering::Release);
        self.trading.sign_out();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A contract is known by its root for its tick and what a point of it
    /// is worth, which is what marks a position to the market.
    #[test]
    fn a_contract_s_tick_and_point_come_from_its_root() {
        assert_eq!(root_of("ESZ6"), "ES");
        assert_eq!(root_of("6EZ26"), "6E");
        assert_eq!(root_of("MNQH7"), "MNQ");
        assert_eq!((rithmic_tick("ESZ6"), rithmic_point("ESZ6")), (Some(0.25), Some(50.0)));
        assert_eq!((rithmic_tick("NQH7"), rithmic_point("NQH7")), (Some(0.25), Some(20.0)));
        assert_eq!(rithmic_point("XXZ6"), None);
    }

    #[test]
    fn a_band_rejection_gives_the_band_at_the_order_s_scale() {
        let band = |price| Some(Bound { price, kind: BoundKind::Band });
        let why = "Order price is outside bands 'Ask of 748000.00 violates Low Band 752125.00 \
                   using Delta .00, CrvBnd equals 754125.00'";
        assert_eq!(bound_of(why, 7_480.0), band(7_521.25));
        let high = "Order price is outside bands 'Bid of 760000.00 violates High Band 755000.00'";
        assert_eq!(bound_of(high, 7_600.0), band(7_550.0));
        assert_eq!(bound_of("Insufficient margin", 7_480.0), None);
    }

    /// A buy limit dragged far below a short's entry, past the day's limit.
    #[test]
    fn a_limit_rejection_gives_the_day_s_limit() {
        let why = "Order price is outside the limits 'Bid of 694075.00 violates Low Limit 711725.00'";
        assert_eq!(bound_of(why, 6_940.75), Some(Bound { price: 7_117.25, kind: BoundKind::DailyLimit }));
        let worded = "Order price is outside the limits 'Bid of 694075.00 violates Low Price Limit of 711725.00'";
        assert_eq!(bound_of(worded, 6_940.75).map(|bound| bound.price), Some(7_117.25), "whatever names it");
    }

    /// A login as migration 9 leaves one signs in with the password the
    /// keyring has kept under its id since before it was migrated, which is
    /// also where a new password for it goes.
    #[test]
    fn a_migrated_login_finds_its_password_where_it_was_kept() {
        let migrated = r#"{"id":"1","name":"Lucid","broker":"rithmic","user":"trader",
                           "values":[["system","LucidTrading"],["gateway","wss://x"]]}"#;
        let login: Login = serde_json::from_str(migrated).unwrap();
        assert_eq!(crate::trading::broker_for(&login.broker).map(|broker| broker.id()), Some("rithmic"));
        assert_eq!(
            signing_of(&login),
            rithmic::Login::Keyring {
                login: "1".into(),
                user: "trader".into(),
                system: "LucidTrading".into(),
                url: "wss://x".into(),
            }
        );
        assert_eq!(Rithmic.signs_in_to(&login).as_deref(), Some("LucidTrading"));
        let bare = Login { values: Vec::new(), ..login };
        assert_eq!(system_of(&bare), rithmic::TEST_SYSTEM, "a login with no system is on Rithmic Test");
    }
}
