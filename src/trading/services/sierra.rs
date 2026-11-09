//! Sierra Chart's DTC server, as a [`TradingService`] and its [`Session`].
//!
//! Sierra Chart is signed in to its own trading service or simulator; its DTC server,
//! with "Allow Trading" on, relays orders to whichever it is. So a login is
//! where the server is — on this machine, port 11099 — and its accounts are
//! whatever Sierra Chart trades: its simulated `Sim1`, `Sim2`, and the
//! trading service's own. Behind both the `sierra` and the `trading` flags: closed
//! while `sierra` is off, and nothing signs in to a trading service while `trading`
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

    fn flatten(&self, account: &str, contract: &str, _exchange: &str) -> Result<usize, Failure> {
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
        let timing = dtc::Timing { request: std::time::Duration::from_millis(700), ..dtc::Timing::default() };
        let trading = dtc::Trading::sign_in_with(&fake.endpoint(), CLIENT_NAME, timing, |_| {}).expect("signed in");
        let session: Arc<dyn Session> = Arc::new(SierraSession { login: "sierra-1".into(), trading });
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
