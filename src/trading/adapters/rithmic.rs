//! Rithmic's order and PnL plants, as a [`Broker`] and its [`Session`].

use std::collections::HashMap;
use std::sync::Arc;

use omacharts_engine::providers::rithmic::session;

use crate::trading::*;

/// How long closing waits for the trading sessions to sign out: what the
/// data session waits.
pub const SIGN_OUT_WAIT: std::time::Duration = rithmic::SIGN_OUT_WAIT;

/// Why the data login's own sign-in was refused, which the data session
/// keeps.
pub fn data_refusal() -> Option<String> {
    session::refusal()
}

/// The system and gateway of a login with none named.
pub const TEST_SYSTEM: &str = "Rithmic Test";
pub const TEST_GATEWAY: &str = "wss://rituz00100.rithmic.com:443";

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
                    filled: order.filled,
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

    fn place(&self, order: &Order) -> Result<String, Failure> {
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
        self.trading.sign_out();
    }
}
