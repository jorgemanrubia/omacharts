//! The seam for brokers, [`Broker`] and [`Session`], the [`Desk`] that holds
//! every login's session, and what the window keeps of it.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use super::*;
use crate::store::Store;

/// What signs a login in for trading: Rithmic, or a fake under test.
pub trait Broker: Send + Sync {
    /// Sign in, blocking until the broker has answered. `changed` is called,
    /// on any thread, whenever a balance moves or the session ends by
    /// itself.
    fn sign_in(&self, login: &Login, changed: Box<dyn Fn() + Send + Sync>) -> Result<Arc<dyn Session>, String>;
}

/// A login's signed-in trading session.
///
/// **The seam for brokers.** [`Broker`] and this are everything the window,
/// the panel, the chart menu, the marks and the theme know of a broker:
/// what is above them deals only in the neutral types here ([`Book`],
/// [`OrderRequest`], [`OrderKind`], [`Status`], [`Refusal`]), so Interactive
/// Brokers or Tradovate is a second implementation of the two traits and
/// nothing else. Rithmic is the first, in [`super::adapters`]; the tests'
/// session is another.
///
/// Accounts are named by their number here: a session is one login's, and
/// [`Desk::book`] puts the login in front.
///
/// Placing, moving, linking, cancelling and flattening block until the broker has answered,
/// so they are for a thread of their own; what becomes of an order arrives
/// later on the book, with `changed`. [`Session::contract_for`] does not
/// block: it is asked on the main thread, as a menu opens.
pub trait Session: Send + Sync {
    /// The accounts, as the broker listed them at sign-in.
    fn accounts(&self) -> Vec<Account>;
    fn balance(&self, account: &str) -> Option<f64>;
    /// What its orders can be.
    fn capabilities(&self) -> Capabilities;
    /// Is this a simulated account, where an order costs nothing?
    fn is_simulated(&self) -> bool;
    /// The contract an order on `instrument` goes to, from what the
    /// session already knows. Never waits on the broker.
    fn contract_for(&self, instrument: &omacharts_engine::Instrument) -> Result<Contract, Refusal>;
    /// Every order, position and balance it has heard of.
    fn book(&self) -> Book;
    /// Place `order` and return the broker's id for it. Never retried.
    fn place(&self, order: &OrderRequest) -> Result<String, Failure>;
    fn cancel(&self, account: &str, basket: &str) -> Result<(), Failure>;
    /// Move the working order `basket` to `price`, keeping everything else
    /// about it. Only when [`Capabilities::modify`] says so.
    fn modify(&self, account: &str, basket: &str, price: f64) -> Result<(), Failure>;
    /// Link working orders so that one filling or cancelled cancels the
    /// rest, held by the broker. Only when [`Capabilities::link`] says so.
    fn link(&self, account: &str, baskets: &[String]) -> Result<(), Failure>;
    fn cancel_all(&self, account: &str) -> Result<(), Failure>;
    fn flatten(&self, account: &str, contract: &str, exchange: &str) -> Result<(), Failure>;
    /// Why it is no longer signed in, if it ended by itself.
    fn trouble(&self) -> Option<String>;
    fn sign_out(&self);
}

/// Every login's trading session, at most one each.
///
/// Opening one is two halves, because signing in blocks and runs on a
/// thread of its own: [`Desk::begin`] says whether one may be opened for a
/// login and marks it as opening, and [`Desk::opened`] takes what the
/// thread came back with. Neither half lets a second session start for a
/// login that has one or is getting one.
pub struct Desk {
    broker: Arc<dyn Broker>,
    sessions: RefCell<HashMap<String, Arc<dyn Session>>>,
    opening: RefCell<HashSet<String>>,
    refused: RefCell<HashMap<String, String>>,
    /// Woken from a session's thread when a balance moves or a session ends;
    /// whoever holds the receiver takes it from there on the main loop.
    wake: async_channel::Sender<()>,
    woken: async_channel::Receiver<()>,
    /// Told on the main loop when anything here changed: the window's
    /// headers, and the Trading page while it is open.
    listeners: RefCell<Vec<std::rc::Weak<dyn Fn()>>>,
}

impl Desk {
    pub fn new(broker: Arc<dyn Broker>) -> Desk {
        let (wake, woken) = async_channel::unbounded();
        Desk {
            broker,
            sessions: RefCell::new(HashMap::new()),
            opening: RefCell::new(HashSet::new()),
            refused: RefCell::new(HashMap::new()),
            wake,
            woken,
            listeners: RefCell::new(Vec::new()),
        }
    }

    /// What a session calls from its own thread when something moved.
    pub fn waker(&self) -> Box<dyn Fn() + Send + Sync> {
        let wake = self.wake.clone();
        Box::new(move || {
            let _ = wake.try_send(());
        })
    }

    /// What the main loop waits on for [`Desk::waker`]'s calls.
    pub fn woken(&self) -> async_channel::Receiver<()> {
        self.woken.clone()
    }

    /// Call `listener` whenever [`Desk::tell`] is, while it is alive.
    pub fn listen(&self, listener: &std::rc::Rc<dyn Fn()>) {
        self.listeners.borrow_mut().push(std::rc::Rc::downgrade(listener));
    }

    /// Tell everybody listening that logins, sessions or accounts changed.
    pub fn tell(&self) {
        let alive: Vec<std::rc::Rc<dyn Fn()>> = {
            let mut listeners = self.listeners.borrow_mut();
            listeners.retain(|listener| listener.strong_count() > 0);
            listeners.iter().filter_map(std::rc::Weak::upgrade).collect()
        };
        for listener in alive {
            listener();
        }
    }

    pub fn broker(&self) -> Arc<dyn Broker> {
        self.broker.clone()
    }

    /// May a session be opened for `login` now? Not while it has one, or
    /// one is being opened. True marks it as opening.
    pub fn begin(&self, login: &str) -> bool {
        if self.live(login) || self.opening.borrow().contains(login) {
            return false;
        }
        // One that ended by itself is let go before another opens.
        self.sessions.borrow_mut().remove(login);
        self.opening.borrow_mut().insert(login.to_string());
        self.refused.borrow_mut().remove(login);
        true
    }

    /// Take what a sign-in for `login` came back with. A session for a
    /// login that is no longer opening — removed meanwhile — is signed out
    /// at once rather than kept.
    pub fn opened(&self, login: &str, outcome: Result<Arc<dyn Session>, String>) {
        let wanted = self.opening.borrow_mut().remove(login);
        match outcome {
            Ok(session) if wanted => {
                self.sessions.borrow_mut().insert(login.to_string(), session);
            }
            Ok(session) => session.sign_out(),
            Err(why) if wanted => {
                self.refused.borrow_mut().insert(login.to_string(), why);
            }
            Err(_) => {}
        }
    }

    /// End `login`'s session, if it has one, and forget any refusal.
    pub fn end(&self, login: &str) {
        self.opening.borrow_mut().remove(login);
        self.refused.borrow_mut().remove(login);
        if let Some(session) = self.sessions.borrow_mut().remove(login) {
            session.sign_out();
        }
    }

    /// Sign every session out, as the window closes, each on a thread of
    /// its own, and wait until they all have or `wait` has passed. A
    /// session that has not finished by then is left to the process
    /// ending; nothing stays signed in that was told to sign out.
    pub fn sign_out_all(&self, wait: std::time::Duration) {
        self.opening.borrow_mut().clear();
        let sessions: Vec<Arc<dyn Session>> = self.sessions.borrow_mut().drain().map(|(_, session)| session).collect();
        if sessions.is_empty() {
            return;
        }
        let (done, finished) = std::sync::mpsc::channel();
        for session in &sessions {
            let session = session.clone();
            let done = done.clone();
            std::thread::spawn(move || {
                session.sign_out();
                let _ = done.send(());
            });
        }
        let until = std::time::Instant::now() + wait;
        for _ in &sessions {
            let left = until.saturating_duration_since(std::time::Instant::now());
            if finished.recv_timeout(left).is_err() {
                break;
            }
        }
    }

    pub fn opening(&self, login: &str) -> bool {
        self.opening.borrow().contains(login)
    }

    pub fn live(&self, login: &str) -> bool {
        self.sessions.borrow().get(login).is_some_and(|session| session.trouble().is_none())
    }

    pub fn session(&self, login: &str) -> Option<Arc<dyn Session>> {
        self.sessions.borrow().get(login).cloned()
    }

    /// How `login` stands. The data login's own refusal, which the data
    /// session keeps, counts too.
    pub fn standing(&self, login: &Login, data: bool) -> Standing {
        if let Some(why) = self.refused.borrow().get(&login.id) {
            return Standing::Refused(why.clone());
        }
        if let Some(session) = self.sessions.borrow().get(&login.id) {
            return match session.trouble() {
                None => Standing::Live,
                Some(why) => Standing::Refused(why),
            };
        }
        if login.signed_out || login.user.is_empty() {
            return Standing::SignedOut;
        }
        if data && let Some(why) = data_refusal() {
            return Standing::Refused(why);
        }
        Standing::Saved
    }

    /// Write what every live session knows now — its accounts and their
    /// balances — into the cache. True when anything changed.
    pub fn note(&self, store: &Store) -> bool {
        let sessions: Vec<(String, Arc<dyn Session>)> =
            self.sessions.borrow().iter().map(|(id, session)| (id.clone(), session.clone())).collect();
        let mut changed = false;
        for (login, session) in sessions {
            let listed = session.accounts();
            let known: Vec<Account> = accounts(store).into_iter().filter(|a| a.login == login).collect();
            if listed != known {
                cache_accounts(store, &login, listed);
                changed = true;
            }
        }
        changed
    }

    /// Every live session's book, one, its accounts named by key: the
    /// login's id before the number. Logins in the order given.
    pub fn book(&self, logins: &[Login]) -> Book {
        let mut book = Book::default();
        for login in logins {
            let Some(session) = self.session(&login.id) else { continue };
            let theirs = session.book();
            book.capabilities.insert(login.id.clone(), session.capabilities());
            let key = |number: String| format!("{}/{number}", login.id);
            book.accounts.extend(theirs.accounts.into_iter().map(|money| Money { key: key(money.key), ..money }));
            book.positions
                .extend(theirs.positions.into_iter().map(|held| Position { account: key(held.account), ..held }));
            book.orders.extend(theirs.orders.into_iter().map(|order| Order { account: key(order.account), ..order }));
        }
        book.orders.sort_by_key(|order| std::cmp::Reverse(order.at));
        book
    }
}

/// What the window keeps of Trading to draw headers from without asking
/// the database each time: read when it starts and whenever Trading
/// changes.
#[derive(Clone, Debug, Default)]
pub struct View {
    pub logins: Vec<Login>,
    pub accounts: Vec<Account>,
    /// The default account, as stored; [`account_of`] ignores one that is
    /// stale.
    pub default: Option<String>,
    pub data_login: Option<String>,
    /// What the live sessions know of orders, positions and money; empty
    /// with none signed in. The window fills it from [`Desk::book`].
    pub book: Book,
}

impl View {
    pub fn load(store: &Store) -> View {
        let logins = logins(store);
        if logins.is_empty() {
            return View::default();
        }
        View {
            accounts: accounts(store),
            default: store.setting(DEFAULT_ACCOUNT),
            data_login: data_login_id(store, &logins),
            logins,
            book: Book::default(),
        }
    }

    /// Is trading anywhere to be seen? Only with a login: without one there
    /// is no panel, no toggle, no order in a menu and nothing on a chart.
    pub fn trades(&self) -> bool {
        !self.logins.is_empty()
    }

    pub fn login(&self, id: &str) -> Option<&Login> {
        login_in(&self.logins, id)
    }

    /// The account a chart that chose `chosen` is about.
    pub fn account_of(&self, chosen: Option<&str>) -> Option<&Account> {
        account_of(&self.accounts, self.default.as_deref(), chosen)
    }

    /// What `account` is called here: see [`account_name`].
    pub fn account_name(&self, account: &Account) -> String {
        account_name(account, &self.logins, &self.accounts)
    }

    /// The default account, or the first when none is.
    pub fn default_account(&self) -> Option<&Account> {
        default_account(&self.accounts, self.default.as_deref())
    }

    /// What the header says is wrong with `account`'s login, if anything.
    pub fn trouble(&self, desk: &Desk, account: &Account) -> Option<String> {
        let login = self.login(&account.login)?;
        desk.standing(login, self.data_login.as_deref() == Some(login.id.as_str())).trouble()
    }
}

/// Something the window asked of a broker, by what it was asked about.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Request {
    /// A new order, by its tag.
    Place(String),
    /// A working order to another price, by basket.
    Move(String),
    /// A working order cancelled, by basket.
    Cancel(String),
    /// A position closed, by the account's key and the contract.
    Flatten(String, String),
}

/// What was asked, as it is drawn until the broker's book says what became
/// of it: faint, never to be taken for something the broker has confirmed.
#[derive(Clone, Debug, PartialEq)]
pub enum Pending {
    /// The order as sent, on the account with the key `key`.
    Place { key: String, order: OrderRequest },
    /// To this price.
    Move(f64),
    Cancel,
    Flatten,
}

/// The accounts with an order on its way, so a second click does not send
/// a second order before the first is acknowledged. Placing, moving and
/// cancelling share it: one request to an account at a time.
#[derive(Default)]
pub struct Placing(RefCell<HashSet<String>>);

impl Placing {
    /// Start placing on `account`; refused while one is on its way.
    pub fn begin(&self, account: &str) -> Result<(), String> {
        match self.0.borrow_mut().insert(account.to_string()) {
            true => Ok(()),
            false => Err("The last order is still on its way".into()),
        }
    }

    /// Is something on its way to `account`?
    pub fn busy(&self, account: &str) -> bool {
        self.0.borrow().contains(account)
    }

    /// The broker answered, either way.
    pub fn done(&self, account: &str) {
        self.0.borrow_mut().remove(account);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trading::fake::tests::*;
    use crate::trading::fake::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// One session per login: a second sign-in while one is open or being
    /// opened is refused before it reaches the broker, and one is opened
    /// again only after the first has ended.
    #[test]
    fn a_login_never_has_two_sessions() {
        let fake = Arc::new(FakeBroker::default());
        let desk = Desk::new(fake.clone());
        let login = Login {
            id: "1".into(),
            name: "Lucid".into(),
            user: "a".into(),
            system: TEST_SYSTEM.into(),
            gateway: TEST_GATEWAY.into(),
            signed_out: false,
        };
        let open = |desk: &Desk| {
            if desk.begin(&login.id) {
                let session = desk.broker().sign_in(&login, Box::new(|| {}));
                desk.opened(&login.id, session);
            }
        };
        assert!(desk.begin(&login.id));
        assert!(!desk.begin(&login.id), "a second while the first is opening");
        let session = desk.broker().sign_in(&login, Box::new(|| {}));
        desk.opened(&login.id, session);
        assert!(desk.live(&login.id));
        open(&desk);
        open(&desk);
        assert_eq!(fake.signed_in.load(Ordering::Acquire), 1);
        assert_eq!(desk.standing(&login, false), Standing::Live);

        desk.end(&login.id);
        assert_eq!(fake.signed_out.load(Ordering::Acquire), 1);
        assert_eq!(desk.standing(&login, false), Standing::Saved);
        open(&desk);
        assert_eq!(fake.signed_in.load(Ordering::Acquire), 2);

        // A session that comes back for a login removed while it signed in
        // is signed straight out, not kept.
        desk.end(&login.id);
        assert!(desk.begin(&login.id));
        desk.end(&login.id);
        let late = desk.broker().sign_in(&login, Box::new(|| {}));
        desk.opened(&login.id, late);
        assert!(!desk.live(&login.id));
        assert_eq!(fake.signed_out.load(Ordering::Acquire), 3);
    }

    /// Closing signs every session out, and waits for them.
    #[test]
    fn closing_signs_every_session_out() {
        let fake = Arc::new(FakeBroker::default());
        let desk = Desk::new(fake.clone());
        let logins = [test_login("1", "Lucid", TEST_SYSTEM), test_login("2", "Tradeify", TEST_SYSTEM)];
        for login in &logins {
            assert!(desk.begin(&login.id));
            desk.opened(&login.id, desk.broker().sign_in(login, Box::new(|| {})));
        }
        assert!(desk.begin("3"), "one still signing in");
        desk.sign_out_all(SIGN_OUT_WAIT);
        assert_eq!(fake.signed_out.load(Ordering::Acquire), 2);
        assert!(logins.iter().all(|login| !desk.live(&login.id)));
        assert!(!desk.opening("3"), "and one signing in is not kept when it comes back");
        desk.sign_out_all(SIGN_OUT_WAIT);
        assert_eq!(fake.signed_out.load(Ordering::Acquire), 2, "each once");
    }

    /// A live session's accounts land in the cache, under their login, and
    /// a balance not known yet keeps the one known before.
    #[test]
    fn a_sessions_accounts_are_cached_with_their_balances() {
        let store = store();
        let kept = Kept::default();
        if skip() {
            return;
        }
        let lucid = add_login(&store, &kept, &typed("Lucid", "a")).unwrap();
        let fake = Arc::new(FakeBroker::default());
        let mut funded = account("", "1234");
        funded.balance = Some(50_210.4);
        fake.accounts.lock().unwrap().insert("Lucid".into(), vec![funded, account("", "1235")]);
        let desk = Desk::new(fake);
        let login = logins(&store)[0].clone();
        assert!(desk.begin(&lucid));
        let session = desk.broker().sign_in(&login, Box::new(|| {}));
        desk.opened(&lucid, session);
        assert!(desk.note(&store));
        assert!(!desk.note(&store), "nothing new the second time");
        let cached = accounts(&store);
        assert_eq!(cached.iter().map(|a| a.key()).collect::<Vec<_>>(), vec![format!("{lucid}/1234"), format!("{lucid}/1235")]);
        assert_eq!(cached[0].balance_text().as_deref(), Some("$50,210"));
        assert_eq!(cached[1].balance_text(), None);

        assert!(note_balance(&store, &lucid, "1235", 48_900.0));
        cache_accounts(&store, &lucid, vec![account("", "1235")]);
        assert_eq!(accounts(&store)[0].balance, Some(48_900.0), "kept across a listing without one");
    }

    #[test]
    fn a_second_order_waits_for_the_first_to_be_answered() {
        let placing = Placing::default();
        assert!(placing.begin("t/951").is_ok());
        assert_eq!(placing.begin("t/951").unwrap_err(), "The last order is still on its way");
        assert!(placing.begin("l/1234").is_ok(), "another account is not held up");
        placing.done("t/951");
        assert!(placing.begin("t/951").is_ok());
    }

    /// The desk's book is every live session's, under the accounts' keys:
    /// a market order fills and is held, a limit works until cancelled, and
    /// flattening leaves nothing held. Each move calls `changed`.
    #[test]
    fn the_desks_book_follows_what_the_sessions_push() {
        let desk = Desk::new(Arc::new(FakeBroker::default()));
        let login = test_login("t", "Rithmic Test", TEST_SYSTEM);
        let moved = Arc::new(AtomicUsize::new(0));
        let counted = moved.clone();
        let session = Arc::new(FakeSession::new(
            vec![account("t", "951")],
            Box::new(move || {
                counted.fetch_add(1, Ordering::AcqRel);
            }),
        ));
        assert!(desk.begin("t"));
        desk.opened("t", Ok(session.clone()));
        let logins = vec![login];
        let empty = desk.book(&logins);
        assert!(empty.orders.is_empty() && empty.positions.is_empty() && empty.accounts.is_empty());
        assert!(empty.capabilities_of("t/951").is_some_and(|can| can.flatten));

        let buy = OrderRequest {
            account: "951".into(),
            contract: "ESZ6".into(),
            exchange: "CME".into(),
            side: Side::Buy,
            qty: 1,
            kind: OrderKind::Market,
            price: None,
            tag: new_tag(),
        };
        session.place(&buy).unwrap();
        let book = desk.book(&logins);
        let held = book.position("t/951", "ESZ6").expect("held");
        assert_eq!((held.net, held.avg), (1, Some(7_612.25)));
        assert_eq!(book.orders[0].status, Stage::Filled);
        let basket = book.orders[0].basket_id.clone();
        assert_eq!(book.orders[0].fills, vec![Fill { basket, at: 0, qty: 1, price: 7_612.25 }]);

        let limit = OrderRequest { side: Side::Sell, kind: OrderKind::Limit, price: Some(7_620.0), ..buy };
        let basket = session.place(&limit).unwrap();
        assert_eq!(desk.book(&logins).working("t/951", "ESZ6").count(), 1);
        session.cancel("951", &basket).unwrap();
        assert_eq!(desk.book(&logins).working("t/951", "ESZ6").count(), 0);
        session.flatten("951", "ESZ6", "CME").unwrap();
        assert!(desk.book(&logins).position("t/951", "ESZ6").is_none());
        assert_eq!(moved.load(Ordering::Acquire), 4);

        session.sign_out();
        assert!(session.place(&limit).is_err(), "a session that is gone refuses, and nothing retries");
    }
}
