//! A broker in memory, for the tests: a session that fills a market order
//! at once and holds a limit or a stop until it is cancelled, and a keyring
//! that is a list.

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use super::*;

/// A password store that is a list in memory, so that nothing a test
/// does reaches a keyring.
#[derive(Default)]
pub struct Kept(pub std::cell::RefCell<Vec<(String, String)>>);

impl Secrets for Kept {
    fn save(&self, login: &str, _user: &str, _system: &str, password: &str) -> Result<(), String> {
        let mut kept = self.0.borrow_mut();
        kept.retain(|(l, _)| l != login);
        kept.push((login.into(), password.into()));
        Ok(())
    }
    fn forget(&self, login: &str, _user: &str, _system: &str) -> Result<(), String> {
        self.0.borrow_mut().retain(|(l, _)| l != login);
        Ok(())
    }
}

/// A broker that signs anybody in with the accounts it was given, and
/// counts how often it was asked. Its logins name it `fake`, and it has
/// no data feed.
#[derive(Default)]
pub struct FakeBroker {
    /// The accounts a login has, by the login's name.
    pub accounts: Mutex<HashMap<String, Vec<Account>>>,
    /// The accounts a login named in neither has.
    pub every_login: Vec<Account>,
    /// The contract each future root trades as, by root, in every session.
    pub contracts: HashMap<String, String>,
    pub signed_in: AtomicUsize,
    pub signed_out: Arc<AtomicUsize>,
}

impl FakeBroker {
    /// The broker as the registry lists it: every login has one account,
    /// and ES trades as ESZ6.
    pub fn listed() -> FakeBroker {
        FakeBroker {
            every_login: vec![Account {
                login: String::new(),
                id: "FAKE-1".into(),
                name: None,
                currency: Some("USD".into()),
                balance: Some(50_000.0),
            }],
            contracts: HashMap::from([("ES".to_string(), "ESZ6".to_string())]),
            ..FakeBroker::default()
        }
    }
}

/// A second broker, in memory: a session that fills a market order at
/// once, at `last`, and holds a limit or a stop until it is cancelled.
/// Its book is the one a real session would push, with `changed` called
/// after each move. That the window trades through it as it does
/// through Rithmic is the proof that nothing above [`Session`] knows
/// which broker it is.
pub struct FakeSession {
    pub accounts: Vec<Account>,
    signed_out: Arc<AtomicUsize>,
    gone: std::sync::atomic::AtomicBool,
    pub book: Mutex<Book>,
    pub last: Mutex<f64>,
    pub changed: Box<dyn Fn() + Send + Sync>,
    baskets: AtomicUsize,
    /// The contract each future root trades as, by root.
    pub contracts: Mutex<HashMap<String, String>>,
    pub simulated: bool,
    pub kinds: Vec<OrderKind>,
    /// What it says it can do beyond placing and cancelling.
    pub modifies: bool,
    pub links_orders: bool,
    /// The groups linked, each cancelled whole when one of it ends.
    pub links: Mutex<Vec<Vec<String>>>,
    /// Refuse whatever is asked next, in these words.
    pub refuse: Mutex<Option<String>>,
    /// Answer nothing at all from now on: every request is lost on the
    /// way, and nothing in the book moves.
    pub silent: std::sync::atomic::AtomicBool,
    /// Hold what is placed as the broker's Pending, as an order is
    /// before the exchange has it, until [`FakeSession::open`] or
    /// [`FakeSession::fill`]; a market order too.
    pub holds: std::sync::atomic::AtomicBool,
}

impl FakeSession {
    pub fn new(accounts: Vec<Account>, changed: Box<dyn Fn() + Send + Sync>) -> FakeSession {
        FakeSession {
            accounts,
            signed_out: Arc::default(),
            gone: Default::default(),
            book: Mutex::default(),
            last: Mutex::new(7_612.25),
            changed,
            baskets: AtomicUsize::new(1),
            contracts: Mutex::default(),
            simulated: true,
            kinds: vec![OrderKind::Market, OrderKind::Limit, OrderKind::Stop],
            modifies: true,
            links_orders: true,
            links: Mutex::default(),
            refuse: Mutex::default(),
            silent: Default::default(),
            holds: Default::default(),
        }
    }

    fn refused(&self) -> Result<(), Failure> {
        if self.silent.load(Ordering::Acquire) {
            return Err(Failure::Unanswered("no answer in time".into()));
        }
        match self.refuse.lock().unwrap().take() {
            Some(why) => Err(Failure::Refused(why)),
            None => Ok(()),
        }
    }

    /// The exchange has the order `basket`: it is working.
    pub fn open(&self, basket: &str) {
        let mut book = self.book.lock().unwrap();
        if let Some(order) = book.orders.iter_mut().find(|o| o.basket_id == basket && o.status == Stage::Pending) {
            order.status = Stage::Working.into();
        }
        drop(book);
        (self.changed)();
    }

    /// `qty` of the working order `basket` fills at its price, the rest
    /// still working.
    pub fn fill_part(&self, basket: &str, qty: u32) {
        let mut book = self.book.lock().unwrap();
        let Some(order) = book.orders.iter_mut().find(|o| o.basket_id == basket && o.status.working()) else {
            return;
        };
        let qty = qty.min(order.qty - order.filled);
        order.filled += qty;
        order.status = match order.filled == order.qty {
            true => Stage::Filled.into(),
            false => Stage::PartiallyFilled.into(),
        };
        order.fill_price = order.price;
        order.fills.push(Fill { basket: basket.into(), at: 0, qty, price: order.price.unwrap_or(0.0) });
        let order = order.clone();
        let bought = if order.side == Side::Buy { qty as i32 } else { -(qty as i32) };
        FakeSession::hold(&mut book, &order.account, &order.contract, &order.exchange, bought, order.price.unwrap_or(0.0));
        drop(book);
        (self.changed)();
    }

    /// End `basket` and cancel every order linked to it, as a broker
    /// holding the link would.
    fn end_linked(&self, book: &mut Book, basket: &str) {
        let links = self.links.lock().unwrap();
        for group in links.iter().filter(|group| group.iter().any(|b| b == basket)) {
            for other in group.iter().filter(|b| *b != basket) {
                if let Some(order) = book.orders.iter_mut().find(|o| o.basket_id == *other && o.status.working()) {
                    order.status = Stage::Cancelled.into();
                }
            }
        }
    }

    /// The market reaches the working order `basket`: it fills at its
    /// price, and whatever is linked to it is cancelled.
    pub fn fill(&self, basket: &str) {
        let mut book = self.book.lock().unwrap();
        let last = *self.last.lock().unwrap();
        let Some(order) = book.orders.iter_mut().find(|o| o.basket_id == basket && o.status.working()) else {
            return;
        };
        let left = order.qty - order.filled;
        order.status = Stage::Filled.into();
        order.filled = order.qty;
        order.fill_price = order.price.or(Some(last));
        order.fills.push(Fill { basket: basket.into(), at: 0, qty: left, price: order.fill_price.unwrap_or(last) });
        let order = order.clone();
        let bought = if order.side == Side::Buy { left as i32 } else { -(left as i32) };
        FakeSession::hold(&mut book, &order.account, &order.contract, &order.exchange, bought, order.fill_price.unwrap_or(last));
        self.end_linked(&mut book, basket);
        drop(book);
        (self.changed)();
    }

    fn hold(book: &mut Book, account: &str, contract: &str, exchange: &str, bought: i32, at: f64) {
        match book.positions.iter_mut().find(|held| held.account == account && held.contract == contract) {
            Some(held) => {
                held.net += bought;
                held.avg = (held.net != 0).then_some(at);
            }
            None => book.positions.push(Position {
                account: account.into(),
                contract: contract.into(),
                exchange: exchange.into(),
                net: bought,
                avg: Some(at),
                open_pnl: None,
                tick: Some(0.25),
            }),
        }
        book.positions.retain(|held| held.net != 0);
    }
}

impl Session for FakeSession {
    fn accounts(&self) -> Vec<Account> {
        self.accounts.clone()
    }
    fn balance(&self, account: &str) -> Option<f64> {
        self.accounts.iter().find(|a| a.id == account).and_then(|a| a.balance)
    }
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            kinds: self.kinds.clone(),
            flatten: true,
            cancel_all: true,
            modify: self.modifies,
            link: self.links_orders,
        }
    }
    fn is_simulated(&self) -> bool {
        self.simulated
    }
    fn contract_for(&self, instrument: &omacharts_engine::Instrument) -> Result<Contract, Refusal> {
        if instrument.kind != omacharts_engine::InstrumentKind::FutureRoot {
            return Err(Refusal::NotTradable);
        }
        let symbol = self.contracts.lock().unwrap().get(&instrument.symbol).cloned().ok_or(Refusal::NotKnownYet)?;
        Ok(Contract { symbol, exchange: "CME".into(), tick: Some(0.25) })
    }
    fn book(&self) -> Book {
        self.book.lock().unwrap().clone()
    }
    fn place(&self, order: &OrderRequest) -> Result<String, Failure> {
        if self.gone.load(Ordering::Acquire) {
            return Err(Failure::Refused("the session was signed out".into()));
        }
        self.refused()?;
        let basket = format!("B{}", self.baskets.fetch_add(1, Ordering::AcqRel));
        let last = *self.last.lock().unwrap();
        let held = self.holds.load(Ordering::Acquire);
        let filled = order.kind == OrderKind::Market && !held;
        let mut book = self.book.lock().unwrap();
        book.orders.insert(
            0,
            Order {
                account: order.account.clone(),
                basket_id: basket.clone(),
                contract: order.contract.clone(),
                exchange: order.exchange.clone(),
                side: order.side,
                qty: order.qty,
                filled: if filled { order.qty } else { 0 },
                kind: order.kind,
                price: order.price,
                status: match (filled, held) {
                    (true, _) => Stage::Filled.into(),
                    (false, true) => Stage::Pending.into(),
                    (false, false) => Stage::Working.into(),
                },
                fill_price: filled.then_some(last),
                fills: match filled {
                    true => vec![Fill { basket: basket.clone(), at: 0, qty: order.qty, price: last }],
                    false => Vec::new(),
                },
                at: 0,
                tag: Some(order.tag.clone()),
                tick: Some(0.25),
            },
        );
        if filled {
            let bought = if order.side == Side::Buy { order.qty as i32 } else { -(order.qty as i32) };
            FakeSession::hold(&mut book, &order.account, &order.contract, &order.exchange, bought, last);
        }
        drop(book);
        (self.changed)();
        Ok(basket)
    }
    fn cancel(&self, _: &str, basket: &str) -> Result<(), Failure> {
        self.refused()?;
        let mut book = self.book.lock().unwrap();
        let order = book.orders.iter_mut().find(|order| order.basket_id == basket && order.status.working());
        let Some(order) = order else { return Err(Failure::Refused("not working".into())) };
        order.status = Stage::Cancelled.into();
        self.end_linked(&mut book, basket);
        drop(book);
        (self.changed)();
        Ok(())
    }
    fn modify(&self, _: &str, basket: &str, price: f64) -> Result<(), Failure> {
        self.refused()?;
        let mut book = self.book.lock().unwrap();
        let order = book.orders.iter_mut().find(|order| order.basket_id == basket && order.status.working());
        let Some(order) = order else { return Err(Failure::Refused("not working".into())) };
        order.price = Some(price);
        drop(book);
        (self.changed)();
        Ok(())
    }
    fn link(&self, _: &str, baskets: &[String]) -> Result<(), Failure> {
        self.refused()?;
        self.links.lock().unwrap().push(baskets.to_vec());
        Ok(())
    }
    fn cancel_all(&self, account: &str) -> Result<(), Failure> {
        self.refused()?;
        let mut book = self.book.lock().unwrap();
        for order in book.orders.iter_mut().filter(|order| order.account == account && order.status.working()) {
            order.status = Stage::Cancelled.into();
        }
        drop(book);
        (self.changed)();
        Ok(())
    }
    fn flatten(&self, account: &str, contract: &str, exchange: &str) -> Result<(), Failure> {
        self.refused()?;
        let last = *self.last.lock().unwrap();
        let mut book = self.book.lock().unwrap();
        let net = book.position(account, contract).map_or(0, |held| held.net);
        FakeSession::hold(&mut book, account, contract, exchange, -net, last);
        drop(book);
        (self.changed)();
        Ok(())
    }
    fn trouble(&self) -> Option<String> {
        self.gone.load(Ordering::Acquire).then(|| "signed out".into())
    }
    fn sign_out(&self) {
        self.gone.store(true, Ordering::Release);
        self.signed_out.fetch_add(1, Ordering::AcqRel);
    }
}

impl Broker for FakeBroker {
    fn id(&self) -> &'static str {
        "fake"
    }

    fn label(&self) -> &'static str {
        "Fake Broker"
    }

    fn sign_in(&self, login: &Login, changed: Box<dyn Fn() + Send + Sync>) -> Result<Arc<dyn Session>, String> {
        self.signed_in.fetch_add(1, Ordering::AcqRel);
        let named = self.accounts.lock().unwrap().get(&login.name).cloned();
        let accounts = named.unwrap_or_else(|| self.every_login.clone());
        let accounts = accounts.into_iter().map(|a| Account { login: login.id.clone(), ..a }).collect();
        let session = FakeSession { signed_out: self.signed_out.clone(), ..FakeSession::new(accounts, changed) };
        *session.contracts.lock().unwrap() = self.contracts.clone();
        Ok(Arc::new(session))
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::store::Store;
    use omacharts_engine::providers::rithmic::session;

    /// Rithmic Test, where a login typed with no system signs in to.
    pub(crate) const TEST_SYSTEM: &str = rithmic::TEST_SYSTEM;
    pub(crate) const TEST_GATEWAY: &str = rithmic::TEST_URL;

    pub(crate) fn store() -> Store {
        Store::memory().expect("an empty database")
    }

    pub(crate) fn typed(name: &str, user: &str) -> Typed {
        Typed {
            broker: "rithmic".into(),
            name: name.into(),
            user: user.into(),
            password: "PLANTED-PASSWORD".into(),
            values: vec![("system".into(), TEST_SYSTEM.into()), ("gateway".into(), "wss://example.invalid:443".into())],
        }
    }

    pub(crate) fn account(login: &str, id: &str) -> Account {
        Account {
            login: login.into(),
            id: id.into(),
            name: None,
            currency: Some("USD".into()),
            balance: None,
        }
    }

    pub(crate) fn skip() -> bool {
        // A development login is in charge of the keyring, and the store's
        // passwords are never written.
        session::development()
    }

    pub(crate) fn instrument(symbol: &str, kind: omacharts_engine::InstrumentKind) -> omacharts_engine::Instrument {
        omacharts_engine::Instrument {
            symbol: symbol.into(),
            name: symbol.into(),
            kind,
            suffix: None,
            currency: Some("USD".into()),
            tier: 0,
            session_origin: 0,
            overrides: Vec::new(),
            exchange: None,
            popularity: 0,
            local_name: None,
        }
    }

    pub(crate) fn test_login(id: &str, name: &str, system: &str) -> Login {
        Login {
            id: id.into(),
            name: name.into(),
            user: "u".into(),
            broker: "rithmic".into(),
            values: vec![("system".into(), system.into()), ("gateway".into(), TEST_GATEWAY.into())],
            signed_out: false,
        }
    }

    pub(crate) fn order(basket: &str, side: Side, kind: OrderKind, price: Option<f64>, status: impl Into<Status>) -> Order {
        Order {
            account: "t/951".into(),
            basket_id: basket.into(),
            contract: "ESZ6".into(),
            exchange: "CME".into(),
            side,
            qty: 1,
            filled: 0,
            kind,
            price,
            status: status.into(),
            fill_price: None,
            fills: Vec::new(),
            at: 0,
            tag: None,
            tick: Some(0.25),
        }
    }

    /// The tests' session moves a working order and refuses one that is
    /// not; linked orders go together, by a cancel or a fill.
    #[test]
    fn the_tests_session_moves_links_and_cancels_together() {
        let session = FakeSession::new(vec![account("1", "951")], Box::new(|| {}));
        let place = |side, kind, price| {
            session
                .place(&OrderRequest {
                    account: "951".into(),
                    contract: "ESZ6".into(),
                    exchange: "CME".into(),
                    side,
                    qty: 1,
                    kind,
                    price: Some(price),
                    tag: new_tag(),
                })
                .unwrap()
        };
        let target = place(Side::Sell, OrderKind::Limit, 7_640.0);
        let stop = place(Side::Sell, OrderKind::Stop, 7_600.0);
        session.modify("951", &target, 7_645.0).unwrap();
        assert_eq!(session.book().order(&target).and_then(|o| o.price), Some(7_645.0));
        session.link("951", &[target.clone(), stop.clone()]).unwrap();
        session.cancel("951", &target).unwrap();
        assert_eq!(session.book().order(&stop).map(|o| o.status.stage), Some(Stage::Cancelled));
        assert!(session.modify("951", &stop, 7_590.0).is_err(), "moved after it was cancelled");

        let target = place(Side::Sell, OrderKind::Limit, 7_640.0);
        let stop = place(Side::Sell, OrderKind::Stop, 7_600.0);
        session.link("951", &[target.clone(), stop.clone()]).unwrap();
        session.fill(&stop);
        assert_eq!(session.book().order(&target).map(|o| o.status.stage), Some(Stage::Cancelled));
        *session.refuse.lock().unwrap() = Some("price out of band".into());
        assert_eq!(session.cancel("951", &target), Err(Failure::Refused("price out of band".into())));
    }
}
