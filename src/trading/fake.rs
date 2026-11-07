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
    /// What the broker holds for a login, by the login's name, from before
    /// it signed in: a session's refresh finds it.
    pub held: Mutex<HashMap<String, Book>>,
    /// How long signing in takes, as a network would.
    pub takes: std::time::Duration,
    /// Every session it signed in, in order.
    pub sessions: Mutex<Vec<Arc<FakeSession>>>,
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

/// A day's orders on the one account every login has, one at every
/// stage, as the broker of a build with the `fakes` feature holds them
/// for any login: something to look at in the panel without placing any.
pub fn day_of_orders() -> Book {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|since| since.as_secs() as i64)
        .unwrap_or(0);
    let order = |basket: &str, ago: i64, side: Side, kind: OrderKind, price: Option<f64>, status: Status| Order {
        account: "FAKE-1".into(),
        basket_id: basket.into(),
        contract: "ESZ6".into(),
        exchange: "CME".into(),
        side,
        qty: 2,
        filled: 0,
        kind,
        price,
        status,
        fill_price: None,
        fills: Vec::new(),
        at: now - ago * 60,
        tag: None,
        tick: Some(0.25),
    };
    let filled = |basket: &str, ago: i64, side: Side, at: f64| Order {
        filled: 2,
        fill_price: Some(at),
        fills: vec![Fill { basket: basket.into(), at: now - ago * 60, qty: 2, price: at }],
        ..order(basket, ago, side, OrderKind::Market, None, Stage::Filled.into())
    };
    Book {
        orders: vec![
            order("B6", 2, Side::Buy, OrderKind::Limit, Some(7_598.25), Stage::Pending.into()),
            order("B5", 9, Side::Sell, OrderKind::Stop, Some(7_571.00), Stage::Working.into()),
            Order {
                filled: 1,
                fills: vec![Fill { basket: "B4".into(), at: now - 16 * 60, qty: 1, price: 7_636.50 }],
                ..order("B4", 17, Side::Sell, OrderKind::Limit, Some(7_636.50), Stage::PartiallyFilled.into())
            },
            order("B3", 31, Side::Buy, OrderKind::Limit, Some(7_540.00), Status::said(Stage::Cancelled, "by the trader")),
            order("B2", 48, Side::Buy, OrderKind::Limit, Some(7_000.00), Status::said(Stage::Rejected, "too far from the market")),
            filled("B1", 63, Side::Buy, 7_612.25),
        ],
        positions: vec![Position {
            account: "FAKE-1".into(),
            contract: "ESZ6".into(),
            exchange: "CME".into(),
            net: 1,
            avg: Some(7_612.25),
            open_pnl: Some(112.5),
            tick: Some(0.25),
            point: Some(50.0),
        }],
        accounts: vec![Money {
            key: "FAKE-1".into(),
            balance: Some(50_112.5),
            unrealized: Some(112.5),
            day_pnl: Some(112.5),
            limits: Some(Limits { loss_limit: Some(2_000.0), ..Limits::default() }),
        }],
        ..Book::default()
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
    /// What the broker holds that the session has not been told of: what
    /// [`Session::refresh`] brings into the book. Its orders are added to
    /// the book's, its positions and money replace the book's.
    pub held: Mutex<Book>,
    /// How many times it was refreshed.
    pub refreshes: AtomicUsize,
    pub last: Mutex<f64>,
    /// What [`Session::last_price`] and [`Session::price_of`] say: the
    /// broker's own market, which need not be the chart's.
    pub market: Mutex<Option<f64>>,
    /// What [`Session::price_limits`] says.
    pub limits: Mutex<(Option<f64>, Option<f64>)>,
    pub changed: Box<dyn Fn(Change) + Send + Sync>,
    baskets: AtomicUsize,
    /// The contract each future root trades as, by root.
    pub contracts: Mutex<HashMap<String, String>>,
    pub simulated: bool,
    pub kinds: Vec<OrderKind>,
    /// What it says it can do beyond placing and cancelling.
    pub modifies: bool,
    /// How it links a stop and a target: every way a broker can is one
    /// of these, so each is tested here.
    pub linking: Linking,
    /// The groups linked, each cancelled whole when one of it ends: after
    /// the fact, by [`Session::link`], or as a pair placed together.
    pub links: Mutex<Vec<Vec<String>>>,
    /// How many pairs were placed together, [`Session::place_pair`]; a part
    /// of one of a pair filled comes off the other, as DTC's do.
    pub pairs: AtomicUsize,
    /// What it was asked, in order: "place", "pair", "cancel", "modify",
    /// "link".
    pub heard: Mutex<Vec<&'static str>>,
    /// Refuse the next pair, or the next link, in these words.
    pub refuse_pairs: Mutex<Option<String>>,
    pub refuse_links: Mutex<Option<String>>,
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
        FakeSession::told(accounts, Box::new(move |_| changed()))
    }

    /// A session that tells `changed` what kind of change it was.
    pub fn told(accounts: Vec<Account>, changed: Box<dyn Fn(Change) + Send + Sync>) -> FakeSession {
        FakeSession {
            accounts,
            signed_out: Arc::default(),
            gone: Default::default(),
            book: Mutex::default(),
            held: Mutex::default(),
            refreshes: AtomicUsize::new(0),
            last: Mutex::new(7_612.25),
            market: Mutex::default(),
            limits: Mutex::default(),
            changed,
            baskets: AtomicUsize::new(1),
            contracts: Mutex::default(),
            simulated: true,
            kinds: vec![OrderKind::Market, OrderKind::Limit, OrderKind::Stop],
            modifies: true,
            linking: Linking::AfterTheFact,
            links: Mutex::default(),
            pairs: AtomicUsize::new(0),
            heard: Mutex::default(),
            refuse_pairs: Mutex::default(),
            refuse_links: Mutex::default(),
            refuse: Mutex::default(),
            silent: Default::default(),
            holds: Default::default(),
        }
    }

    pub fn moved(&self) {
        (self.changed)(Change::Moved);
    }

    /// Drop and come back, as a network would: what was working is no
    /// longer in the book until a refresh says what became of it, and the
    /// desk is told it is back. `meanwhile` is what the broker did while it
    /// was down, to what it holds.
    pub fn drop_and_return(&self, meanwhile: impl FnOnce(&mut Book)) {
        {
            let mut book = self.book.lock().unwrap();
            let mut held = self.held.lock().unwrap();
            *held = book.clone();
            meanwhile(&mut held);
            book.orders.retain(|order| !order.status.working());
            book.positions.clear();
            book.refreshed = false;
        }
        (self.changed)(Change::Back);
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
        self.moved();
    }

    /// `qty` of the working order `basket` fills at its price, the rest
    /// still working.
    pub fn fill_part(&self, basket: &str, qty: u32) {
        let mut book = self.book.lock().unwrap();
        let Some(order) = book.orders.iter_mut().find(|o| o.basket_id == basket && o.status.working()) else {
            return;
        };
        let qty = qty.min(order.qty - order.filled);
        let basket = basket.to_string();
        order.filled += qty;
        order.status = match order.filled == order.qty {
            true => Stage::Filled.into(),
            false => Stage::PartiallyFilled.into(),
        };
        order.fill_price = order.price;
        order.fills.push(Fill { basket: basket.clone(), at: 0, qty, price: order.price.unwrap_or(0.0) });
        let order = order.clone();
        let bought = if order.side == Side::Buy { qty as i32 } else { -(qty as i32) };
        FakeSession::hold(&mut book, &order.account, &order.contract, &order.exchange, bought, order.price.unwrap_or(0.0));
        match order.status.working() {
            false => self.end_linked(&mut book, &basket),
            // The other of a pair placed together is for as much less.
            true if self.linking == Linking::AtSubmission => {
                let links = self.links.lock().unwrap();
                for group in links.iter().filter(|group| group.contains(&basket)) {
                    for other in group.iter().filter(|b| **b != basket) {
                        if let Some(other) = book.orders.iter_mut().find(|o| o.basket_id == *other && o.status.working()) {
                            other.qty = other.qty.saturating_sub(qty).max(other.filled);
                        }
                    }
                }
            }
            true => {}
        }
        drop(book);
        self.moved();
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
        self.moved();
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
                point: Some(50.0),
            }),
        }
        book.positions.retain(|held| held.net != 0);
    }
}

impl Session for FakeSession {
    fn accounts(&self) -> Vec<Account> {
        self.accounts.clone()
    }
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            kinds: self.kinds.clone(),
            flatten: true,
            cancel_all: true,
            modify: self.modifies,
            link: self.linking,
            history: true,
            snapshot: true,
        }
    }
    fn is_simulated(&self) -> bool {
        self.simulated
    }
    fn contract_for(&self, instrument: &omacharts_engine::Instrument) -> Result<Contract, Refusal> {
        if instrument.kind != omacharts_engine::InstrumentKind::FutureRoot {
            return Err(Refusal::NotTradable);
        }
        // A dated contract trades as itself; a root as the contract the
        // broker says is trading now.
        let symbol = match instrument.is_contract() {
            true => instrument.symbol.clone(),
            false => self.contracts.lock().unwrap().get(&instrument.symbol).cloned().ok_or(Refusal::NotKnownYet)?,
        };
        Ok(Contract { symbol, exchange: "CME".into(), tick: Some(0.25) })
    }
    fn last_price(&self, _instrument: &omacharts_engine::Instrument) -> Option<f64> {
        *self.market.lock().unwrap()
    }
    fn price_limits(&self, _instrument: &omacharts_engine::Instrument) -> (Option<f64>, Option<f64>) {
        *self.limits.lock().unwrap()
    }
    fn price_of(&self, _contract: &str, _exchange: &str) -> Option<f64> {
        *self.market.lock().unwrap()
    }
    fn book(&self) -> Book {
        self.book.lock().unwrap().clone()
    }
    fn refresh(&self) -> Result<(), String> {
        if self.gone.load(Ordering::Acquire) {
            return Err("the session was signed out".into());
        }
        let held = self.held.lock().unwrap().clone();
        let mut book = self.book.lock().unwrap();
        for order in held.orders {
            match book.orders.iter_mut().find(|known| known.basket_id == order.basket_id) {
                Some(known) => *known = order,
                None => book.orders.push(order),
            }
        }
        book.orders.sort_by_key(|order| std::cmp::Reverse(order.at));
        book.positions = held.positions;
        // Money the broker never said is the balance the account was listed
        // with.
        book.accounts = match held.accounts.is_empty() {
            true => self
                .accounts
                .iter()
                .map(|account| Money { key: account.id.clone(), balance: account.balance, ..Money::default() })
                .collect(),
            false => held.accounts,
        };
        book.refreshed = true;
        drop(book);
        self.refreshes.fetch_add(1, Ordering::AcqRel);
        self.moved();
        Ok(())
    }
    fn history(&self, account: &str, from: i64, to: i64) -> Result<Vec<Order>, Failure> {
        self.refused()?;
        let book = self.book.lock().unwrap();
        let mut orders: Vec<Order> = book
            .orders
            .iter()
            .filter(|order| order.account == account && (from..=to).contains(&order.at))
            .cloned()
            .collect();
        orders.sort_by_key(|order| order.at);
        Ok(orders)
    }
    fn place(&self, order: &OrderRequest) -> Result<String, Failure> {
        if self.gone.load(Ordering::Acquire) {
            return Err(Failure::Refused("the session was signed out".into()));
        }
        self.refused()?;
        self.heard.lock().unwrap().push("place");
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
        self.moved();
        Ok(basket)
    }
    fn cancel(&self, _: &str, basket: &str) -> Result<(), Failure> {
        self.refused()?;
        self.heard.lock().unwrap().push("cancel");
        let mut book = self.book.lock().unwrap();
        let order = book.orders.iter_mut().find(|order| order.basket_id == basket && order.status.working());
        let Some(order) = order else { return Err(Failure::Refused("not working".into())) };
        order.status = Stage::Cancelled.into();
        self.end_linked(&mut book, basket);
        drop(book);
        self.moved();
        Ok(())
    }
    fn modify(&self, _: &str, basket: &str, price: f64) -> Result<(), Failure> {
        self.refused()?;
        self.heard.lock().unwrap().push("modify");
        let mut book = self.book.lock().unwrap();
        let order = book.orders.iter_mut().find(|order| order.basket_id == basket && order.status.working());
        let Some(order) = order else { return Err(Failure::Refused("not working".into())) };
        order.price = Some(price);
        drop(book);
        self.moved();
        Ok(())
    }
    fn link(&self, _: &str, baskets: &[String]) -> Result<(), Failure> {
        self.refused()?;
        self.heard.lock().unwrap().push("link");
        if self.linking != Linking::AfterTheFact {
            return Err(Failure::Refused("this broker does not link working orders".into()));
        }
        if let Some(why) = self.refuse_links.lock().unwrap().take() {
            return Err(Failure::Refused(why));
        }
        self.links.lock().unwrap().push(baskets.to_vec());
        Ok(())
    }
    fn place_pair(&self, orders: &[OrderRequest; 2]) -> Result<[String; 2], Failure> {
        self.refused()?;
        self.heard.lock().unwrap().push("pair");
        if self.linking != Linking::AtSubmission {
            return Err(Failure::Refused("this broker does not place orders as a pair".into()));
        }
        if let Some(why) = self.refuse_pairs.lock().unwrap().take() {
            return Err(Failure::Refused(why));
        }
        // Each as a single order would be, then linked whole.
        let mut heard = self.heard.lock().unwrap();
        let before = heard.len();
        drop(heard);
        let placed = [self.place(&orders[0])?, self.place(&orders[1])?];
        heard = self.heard.lock().unwrap();
        heard.truncate(before);
        drop(heard);
        self.links.lock().unwrap().push(placed.to_vec());
        self.pairs.fetch_add(1, Ordering::AcqRel);
        Ok(placed)
    }
    fn cancel_all(&self, account: &str) -> Result<(), Failure> {
        self.refused()?;
        let mut book = self.book.lock().unwrap();
        for order in book.orders.iter_mut().filter(|order| order.account == account && order.status.working()) {
            order.status = Stage::Cancelled.into();
        }
        drop(book);
        self.moved();
        Ok(())
    }
    fn flatten(&self, account: &str, contract: &str, exchange: &str) -> Result<(), Failure> {
        self.refused()?;
        let last = *self.last.lock().unwrap();
        let mut book = self.book.lock().unwrap();
        let net = book.position(account, contract).map_or(0, |held| held.net);
        FakeSession::hold(&mut book, account, contract, exchange, -net, last);
        drop(book);
        self.moved();
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

    /// Who signed in, as Rithmic's does: what the Log is shown to keep
    /// concealed.
    fn heard_at_sign_in(&self, login: &Login) -> Option<Heard> {
        Some(Heard { warning: None, detail: format!("User: {}", login.user) })
    }

    fn sign_in(&self, login: &Login, changed: Box<dyn Fn(Change) + Send + Sync>) -> Result<Arc<dyn Session>, String> {
        if !self.takes.is_zero() {
            std::thread::sleep(self.takes);
        }
        self.signed_in.fetch_add(1, Ordering::AcqRel);
        let named = self.accounts.lock().unwrap().get(&login.name).cloned();
        let accounts = named.unwrap_or_else(|| self.every_login.clone());
        let accounts = accounts.into_iter().map(|a| Account { login: login.id.clone(), ..a }).collect();
        let session = FakeSession { signed_out: self.signed_out.clone(), ..FakeSession::told(accounts, changed) };
        *session.contracts.lock().unwrap() = self.contracts.clone();
        let held = self.held.lock().unwrap();
        // What a login named in neither holds is under no name at all.
        *session.held.lock().unwrap() = held.get(&login.name).or_else(|| held.get("")).cloned().unwrap_or_default();
        drop(held);
        let session = Arc::new(session);
        self.sessions.lock().unwrap().push(session.clone());
        Ok(session)
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
