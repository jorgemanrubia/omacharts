//! Rithmic's order and PnL plants, as a [`Broker`] and its [`Session`].

use std::collections::HashMap;
use std::sync::Arc;

use omacharts_engine::providers::rithmic::session;

use crate::trading::*;

/// How long closing waits for the trading sessions to sign out: what the
/// data session waits.
pub const SIGN_OUT_WAIT: std::time::Duration = rithmic::SIGN_OUT_WAIT;

/// What a login's system and gateway are kept under in its values, as its
/// dialog types them and the data session reads them.
const SYSTEM: &str = "system";
const GATEWAY: &str = "gateway";

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

/// Rithmic's order and PnL plants, through `rithmic::Trading`.
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

    fn sign_in(&self, login: &Login, changed: Box<dyn Fn() + Send + Sync>) -> Result<Arc<dyn Session>, String> {
        let app = session::app().map_err(|error| error.to_string())?;
        // A developer's login file wins over the keyring, which it never
        // touches, as it does for the data session.
        let signing = match rithmic::Login::from_env(system_of(login), gateway_of(login)) {
            Some(development) => development.map_err(|error| error.to_string())?,
            None => signing_of(login),
        };
        let trading = rithmic::Trading::sign_in(app, &signing, changed).map_err(said)?;
        Ok(Arc::new(RithmicSession {
            login: login.id.clone(),
            simulated: system_of(login) == rithmic::TEST_SYSTEM,
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
                .map(|money| Money {
                    key: money.account,
                    balance: money.balance,
                    day_pnl: match (money.realized_day, money.unrealized) {
                        (Some(closed), Some(open)) => Some(closed + open),
                        (closed, open) => closed.or(open),
                    },
                    realized_day: money.realized_day,
                    unrealized: money.unrealized,
                    limits: None,
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
                .map(|order| Order {
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
                        rithmic::Status::Pending => Status { stage: Stage::Pending, said: order.said },
                        rithmic::Status::Working => Status { stage: Stage::Working, said: order.said },
                        rithmic::Status::PartiallyFilled => Status { stage: Stage::PartiallyFilled, said: order.said },
                        rithmic::Status::Filled => Status { stage: Stage::Filled, said: order.said },
                        rithmic::Status::Cancelled => Status { stage: Stage::Cancelled, said: order.said },
                        rithmic::Status::Rejected(why) => Status::said(Stage::Rejected, why),
                    },
                    fill_price: order.fill_price,
                    fills: Vec::new(),
                    at: order.at,
                    tag: order.tag,
                })
                .collect(),
        }
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
        self.trading.sign_out();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
