//! Sierra Chart's DTC server, as a [`Broker`] and its [`Session`].
//!
//! Sierra Chart is signed in to its own broker or simulator; its DTC server,
//! with "Allow Trading" on, relays orders to whichever it is. So a login is
//! where the server is — on this machine, port 11099 — and its accounts are
//! whatever Sierra Chart trades: its simulated `Sim1`, `Sim2`, and the
//! broker's own. Behind both the `sierra` and the `trading` flags: closed
//! while `sierra` is off, and nothing signs in to a broker while `trading`
//! is.
//!
//! The server sends no CME market data, so a session prices nothing: its
//! positions are marked to the charts of their contracts, as the desk does
//! for any session that cannot.
//!
//! Contracts are the exchange's spelling above this, `ESZ6`, and Sierra
//! Chart's on the wire, `ESZ26_FUT_CME`; this is the one place that turns
//! one into the other.

use std::collections::HashMap;
use std::sync::Arc;

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

impl Broker for Sierra {
    fn id(&self) -> &'static str {
        feed::LISTED.id
    }

    fn label(&self) -> &'static str {
        feed::LISTED.label
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
        Ok(Arc::new(SierraSession { login: login.id.clone(), trading }))
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

/// The exchange an order on `contract` goes to, as the app names it.
fn exchange_of(contract: &str, said: Option<&str>) -> String {
    match said.map(str::trim).filter(|exchange| !exchange.is_empty()) {
        Some(exchange) => exchange.to_string(),
        None => omacharts_engine::exchanges::registry().contract_exchange(root_of(contract)).unwrap_or("CME").to_string(),
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
        exchange: exchange_of(&contract, order.exchange.as_deref()),
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
        exchange: exchange_of(&contract, fill.exchange.as_deref()),
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

struct SierraSession {
    login: String,
    trading: dtc::Trading,
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
        let sierra = feed::sierra_contract(&instrument.symbol).ok_or(Refusal::NotKnownYet)?;
        let symbol = feed::exchange_contract(&sierra);
        Ok(Contract { tick: specs::tick(root_of(&symbol)), exchange: exchange_of(&symbol, None), symbol })
    }

    fn book(&self) -> Book {
        let book = self.trading.book();
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
                        exchange: exchange_of(&contract, held.exchange.as_deref()),
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
        self.trading.place(&new_order(order)?).map_err(failed)
    }

    /// One SUBMIT_NEW_OCO_ORDER, with no parent: see
    /// [`dtc::Trading::place_pair`].
    fn place_pair(&self, orders: &[OrderRequest; 2]) -> Result<[String; 2], Failure> {
        self.trading.place_pair(&[new_order(&orders[0])?, new_order(&orders[1])?]).map_err(failed)
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

    fn flatten(&self, account: &str, contract: &str, _exchange: &str) -> Result<(), Failure> {
        let symbol = feed::sierra_contract(contract)
            .ok_or_else(|| Failure::Refused(format!("Sierra Chart has no name for {contract}")))?;
        self.trading.flatten(account, &symbol).map_err(failed)
    }

    fn trouble(&self) -> Option<String> {
        self.trading.trouble().map(said)
    }

    fn sign_out(&self) {
        self.trading.sign_out();
    }
}

/// An order as the server takes it, in Sierra Chart's spelling.
fn new_order(order: &OrderRequest) -> Result<dtc::NewOrder, Failure> {
    let symbol = feed::sierra_contract(&order.contract)
        .ok_or_else(|| Failure::Refused(format!("Sierra Chart has no name for {}", order.contract)))?;
    Ok(dtc::NewOrder {
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
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn login(port: u16) -> Login {
        Login {
            id: "sierra-1".into(),
            name: "Sierra Chart".into(),
            broker: "sierra".into(),
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
        assert_eq!(link.holder, Holder::Broker);
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

    /// Closed while its flag is off: offered nowhere, and a saved login of
    /// it shown nowhere, though it is kept.
    #[test]
    fn the_broker_is_closed_while_its_flag_is_off() {
        let store = crate::store::Store::memory().unwrap();
        let kept = crate::trading::fake::Kept::default();
        let typed = Typed {
            broker: "sierra".into(),
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
