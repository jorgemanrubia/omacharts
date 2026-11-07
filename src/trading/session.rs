//! The seam for brokers, [`Broker`] and [`Session`], the [`Desk`] that holds
//! every login's session, and what the window keeps of it.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use super::*;
use crate::store::Store;

/// A broker: what signs a login in for trading, and what keeps its
/// password. Listed once, in [`super::adapters::registry`].
pub trait Broker: Send + Sync {
    /// The id its logins name it by, which is its data feed's too when it
    /// has one. Stored, so never changed.
    fn id(&self) -> &'static str;
    /// What it is called in "Add a trading account…".
    fn label(&self) -> &'static str;
    /// Where `login` signs in to, in a few words for its row, when there is
    /// more than one place it could.
    fn signs_in_to(&self, _login: &Login) -> Option<String> {
        None
    }
    /// Where its passwords are kept: the desktop keyring, in the end.
    fn keyring(&self) -> Option<&'static dyn Secrets> {
        None
    }
    /// Keep `password` for `login` in `secrets`.
    fn keep_password(&self, secrets: &dyn Secrets, login: &Login, password: &str) -> Result<(), String> {
        secrets.save(&login.id, &login.user, "", password)
    }
    /// Forget `login`'s password in `secrets`. For the `data` login, its
    /// data session ends with it.
    fn forget_password(&self, secrets: &dyn Secrets, login: &Login, _data: bool) -> Result<(), String> {
        secrets.forget(&login.id, &login.user, "")
    }
    /// Sign in, blocking until the broker has answered. `changed` is called,
    /// on any thread, whenever an order, a position or a balance moves or
    /// the session ends by itself, and when it is back after a drop.
    fn sign_in(&self, login: &Login, changed: Box<dyn Fn(Change) + Send + Sync>) -> Result<Arc<dyn Session>, String>;
    /// What `login`'s last sign-in heard from the broker, for its entry in
    /// the Log, whether it signed in or not: `None` for a broker that says
    /// nothing more.
    fn heard_at_sign_in(&self, _login: &Login) -> Option<Heard> {
        None
    }
    /// Is it there now? Not while a feature flag it is behind is off: then
    /// it is offered nowhere, its logins are shown nowhere and none signs
    /// in. Open by default.
    fn open(&self) -> bool {
        true
    }
    /// Can `login`, as saved, sign in again at startup with nothing typed?
    /// By default when it has a user, whose password is kept.
    fn signs_in_saved(&self, login: &Login) -> bool {
        !login.user.is_empty()
    }
}

/// What a sign-in heard, beyond whether it signed in: something to warn of
/// though it did, and every word of it for the entry's detail. Never a
/// password.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Heard {
    pub warning: Option<String>,
    pub detail: String,
}

/// What a session tells its desk, from its own thread.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Change {
    /// An order, a position or a balance moved, or the session ended: read
    /// the book again.
    Moved,
    /// Signed in again after a drop. What was working may have finished
    /// meanwhile: the cue to [`Session::refresh`] and then reconcile.
    Back,
    /// A contract held traded at a new price: the positions and the money
    /// to mark again, and nothing else to read.
    Priced,
}

/// What wakes the main loop, from a session's thread or a desk's.
pub enum Wake {
    /// A session's book moved.
    Moved,
    /// A contract held traded at a new price.
    Priced,
    /// The login's session is back after a drop, and wants refreshing.
    Back(String),
    /// The login's session was refreshed, after it came back.
    Refreshed(String),
    /// A sign-in the desk started for the login came back, refreshed when
    /// it signed in; with how its data half went, when it was asked to sign
    /// that in too.
    /// With what the sign-in heard, for the Log.
    Opened(Login, Result<Arc<dyn Session>, String>, Option<Result<(), String>>, Option<Heard>),
}

/// The brokers a desk signs logins in through, each found by the id its
/// logins name.
#[derive(Clone)]
pub struct Registry(Vec<Arc<dyn Broker>>);

impl Registry {
    pub fn of(brokers: Vec<Arc<dyn Broker>>) -> Registry {
        Registry(brokers)
    }

    /// The broker logins call `id`, while it is open.
    pub fn get(&self, id: &str) -> Option<Arc<dyn Broker>> {
        self.iter().find(|broker| broker.id() == id).cloned()
    }

    /// Every broker that is open, in the order they are offered.
    pub fn iter(&self) -> impl Iterator<Item = &Arc<dyn Broker>> {
        self.0.iter().filter(|broker| broker.open())
    }

    /// Is `id` a broker of this build's that is closed now: behind a flag
    /// that is off? Its logins are kept and shown nowhere.
    pub fn closed(&self, id: &str) -> bool {
        self.0.iter().any(|broker| broker.id() == id && !broker.open())
    }
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
/// later on the book, with `changed`. So do [`Session::refresh`] and
/// [`Session::history`]. [`Session::contract_for`] does not block: it is
/// asked on the main thread, as a menu opens.
pub trait Session: Send + Sync {
    /// The accounts, as the broker listed them at sign-in.
    fn accounts(&self) -> Vec<Account>;
    /// What its orders can be.
    fn capabilities(&self) -> Capabilities;
    /// Is this a simulated account, where an order costs nothing?
    fn is_simulated(&self) -> bool;
    /// The contract an order on `instrument` goes to, whatever feed charts
    /// it. Never waits on the broker: what it does not know yet it asks for,
    /// and tells with a change once it does.
    fn contract_for(&self, instrument: &omacharts_engine::Instrument) -> Result<Contract, Refusal>;
    /// The last price the broker's own market data has traded `instrument`
    /// at, for one [`Session::contract_for`] has been asked about: what an
    /// order there is judged against, whatever feed charts it. Never waits:
    /// `None` until a trade has been heard, and for a broker with no data.
    fn last_price(&self, _instrument: &omacharts_engine::Instrument) -> Option<f64> {
        None
    }
    /// The lowest and highest prices the exchange takes an order on
    /// `instrument` at today, as far as the broker has said: an order past
    /// either is refused. Never waits: `None` for a side not known.
    fn price_limits(&self, _instrument: &omacharts_engine::Instrument) -> (Option<f64>, Option<f64>) {
        (None, None)
    }
    /// The last trade the broker's market data has heard in `contract` on
    /// `exchange`, one of the book's positions, for marking it to the
    /// market: [`Desk::book`] asks for every position held each time it is
    /// read. Never waits: asking starts listening on a market-data session
    /// the app already has, never a new one, and a new price is told with
    /// [`Change::Priced`] while the contract is held. `None` until a trade
    /// has been heard, while there is no such session, and for a broker
    /// with no data; then a chart of that very contract is marked to
    /// instead, else the position keeps the broker's figure.
    fn price_of(&self, _contract: &str, _exchange: &str) -> Option<f64> {
        None
    }
    /// Every order, position and balance it has heard of.
    fn book(&self) -> Book;
    /// Ask the broker for what the accounts hold now — the money and the
    /// limits, the positions, the working orders and today's orders with
    /// their fills — and block until it has come back and gone quiet, with
    /// [`Book::refreshed`] set. Signing in does it once, and so does coming
    /// back after a drop. A broker that cannot ([`Capabilities::snapshot`]
    /// false) has nothing to do and says it is done.
    fn refresh(&self) -> Result<(), String>;
    /// The orders on the account numbered `account` from `from` to `to`,
    /// unix seconds, with their fills, oldest first. Only when
    /// [`Capabilities::history`] says so. Part of the seam so that every
    /// broker serves past orders the same way; nothing in the window asks
    /// for them yet.
    fn history(&self, account: &str, from: i64, to: i64) -> Result<Vec<Order>, Failure>;
    /// Place `order` and return the broker's id for it. Never retried.
    fn place(&self, order: &OrderRequest) -> Result<String, Failure>;
    fn cancel(&self, account: &str, basket: &str) -> Result<(), Failure>;
    /// Move the working order `basket` to `price`, keeping everything else
    /// about it. Only when [`Capabilities::modify`] says so.
    fn modify(&self, account: &str, basket: &str, price: f64) -> Result<(), Failure>;
    /// Link working orders so that one filling or cancelled cancels the
    /// rest, held by the broker. Only when [`Capabilities::link`] is
    /// [`Linking::AfterTheFact`]; [`Plan`] is what asks.
    fn link(&self, account: &str, baskets: &[String]) -> Result<(), Failure>;
    /// Place two orders as one one-cancels-other pair, held by the broker,
    /// a part of one filled taken off the other, and return their ids in
    /// order. Only when [`Capabilities::link`] is [`Linking::AtSubmission`];
    /// [`Plan`] is what asks.
    fn place_pair(&self, _orders: &[OrderRequest; 2]) -> Result<[String; 2], Failure> {
        Err(Failure::Refused("this broker does not place orders as a pair".into()))
    }
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
    brokers: Registry,
    sessions: RefCell<HashMap<String, Arc<dyn Session>>>,
    opening: RefCell<HashSet<String>>,
    refused: RefCell<HashMap<String, String>>,
    /// Why a login's data half did not sign in when its trading half did,
    /// until it is signed in again or out.
    data_refused: RefCell<HashMap<String, String>>,
    /// How many times each login's session has been refreshed this run: a
    /// count, so that a refresh is never missed between two reads.
    refreshes: RefCell<HashMap<String, u32>>,
    /// Woken from a session's thread when something moved; whoever holds
    /// the receiver hands each to [`Desk::take`] on the main loop.
    wake: async_channel::Sender<Wake>,
    woken: async_channel::Receiver<Wake>,
    /// Told on the main loop when anything here changed: the window's
    /// headers, and the Trading page while it is open.
    listeners: RefCell<Vec<std::rc::Weak<dyn Fn()>>>,
    /// The links made this run, each with its login and the account's name
    /// for the Log: the broker's, to draw while they stand, and the app's
    /// own, to keep — see [`Desk::keep_links`].
    links: RefCell<Vec<(String, String, Link)>>,
}

impl Desk {
    pub fn new(brokers: Registry) -> Desk {
        let (wake, woken) = async_channel::unbounded();
        Desk {
            brokers,
            sessions: RefCell::new(HashMap::new()),
            opening: RefCell::new(HashSet::new()),
            refused: RefCell::new(HashMap::new()),
            data_refused: RefCell::new(HashMap::new()),
            refreshes: RefCell::new(HashMap::new()),
            wake,
            woken,
            listeners: RefCell::new(Vec::new()),
            links: RefCell::new(Vec::new()),
        }
    }

    /// Remember `link`, made on `login`'s session for the account called
    /// `name`: one the broker holds is drawn while it stands, and one the
    /// app holds is kept by [`Desk::keep_links`].
    pub fn linked(&self, login: &str, name: &str, link: Link) {
        self.links.borrow_mut().push((login.to_string(), name.to_string(), link));
    }

    /// Every link standing, as of the last change, its account by key.
    pub fn links(&self) -> Vec<Link> {
        self.links.borrow().iter().map(|(_, _, link)| link.clone()).collect()
    }

    /// Keep each link as the books now stand, on the main loop after every
    /// change. One the broker holds is forgotten once one of it is done:
    /// the broker has dealt with the rest. One the app holds is the app's
    /// to keep ([`due`]): once one of it filled or went, the rest are
    /// cancelled, off the main loop, and said in the Log. A login signed
    /// out takes its links with it: nothing holds the app's any more. Not
    /// at all while trading is switched off.
    pub fn keep_links(&self) {
        if !crate::features::trading() || self.links.borrow().is_empty() {
            return;
        }
        let mut cancels = Vec::new();
        self.links.borrow_mut().retain(|(login, name, link)| {
            let Some(session) = self.session(login).filter(|_| self.live(login)) else { return false };
            let book = session.book();
            match link.holder {
                Holder::Broker => standing(link, &book),
                Holder::App => match due(link, &book) {
                    Due::Keep => true,
                    Due::Drop => false,
                    Due::Cancel(rest) => {
                        cancels.push((session, name.clone(), link.account.clone(), rest));
                        false
                    }
                },
            }
        });
        for (session, name, key, rest) in cancels {
            let number = key.split_once('/').map_or(key.as_str(), |(_, number)| number).to_string();
            std::thread::spawn(move || {
                for basket in rest {
                    crate::log_words::say(match session.cancel(&number, &basket) {
                        Ok(()) => crate::log_words::confirmed(&name, "Cancelled the other exit · its link was held by Omacharts"),
                        Err(why) => crate::log_words::refused(&name, format!("The other exit is still working · {why}")),
                    });
                }
            });
        }
    }

    /// What `login`'s session calls from its own thread when something
    /// moved.
    pub fn waker(&self, login: &str) -> Box<dyn Fn(Change) + Send + Sync> {
        let wake = self.wake.clone();
        let login = login.to_string();
        Box::new(move |change| {
            let _ = wake.try_send(match change {
                Change::Moved => Wake::Moved,
                Change::Priced => Wake::Priced,
                Change::Back => Wake::Back(login.clone()),
            });
        })
    }

    /// What the main loop waits on, each to be handed to [`Desk::take`].
    pub fn woken(&self) -> async_channel::Receiver<Wake> {
        self.woken.clone()
    }

    /// Do what `wake` asks of the desk, on the main loop: refresh a session
    /// that is back, count one that was refreshed, keep one that signed in
    /// and say how it went in the Log, once. Whoever called then reads the
    /// book again.
    pub fn take(&self, wake: Wake) {
        match wake {
            Wake::Moved => self.keep_links(),
            Wake::Priced => {}
            Wake::Back(login) => self.refresh(&login),
            Wake::Refreshed(login) => *self.refreshes.borrow_mut().entry(login).or_default() += 1,
            Wake::Opened(login, outcome, data, heard) => {
                let accounts = outcome.as_ref().map(|session| session.accounts().len()).map_err(String::as_str);
                crate::log_words::say(crate::log_words::signed_in_heard(&login.name, accounts, heard.as_ref()));
                if let Some(Err(why)) = data {
                    crate::log_words::say(crate::log_words::data_not_signed_in(&login.name, &why));
                    self.data_refused.borrow_mut().insert(login.id.clone(), why);
                }
                self.opened(&login.id, outcome);
            }
        }
    }

    /// Refresh `login`'s session on a thread of its own, and say so on the
    /// main loop when it is done.
    pub fn refresh(&self, login: &str) {
        let Some(session) = self.session(login) else { return };
        let wake = self.wake.clone();
        let login = login.to_string();
        std::thread::spawn(move || {
            // A refresh that did not work leaves the book as it was pushed,
            // and the reconciling to the next.
            let _ = session.refresh();
            let _ = wake.try_send(Wake::Refreshed(login));
        });
    }

    /// How many times `login`'s session has been refreshed this run: once
    /// it signed in, and again each time it came back.
    pub fn refreshes(&self, login: &str) -> u32 {
        self.refreshes.borrow().get(login).copied().unwrap_or(0)
    }

    /// Sign `login` in on a thread of its own and refresh it there, then,
    /// with `data`, sign its data feed in too. Nothing here waits: what it
    /// comes back with arrives through [`Desk::woken`] as [`Wake::Opened`].
    /// False when it cannot start: no broker for it in this build, or a
    /// session open or opening already. The Trading page's Sign in and
    /// startup both come here.
    pub fn sign_in(&self, login: &Login, data: bool) -> bool {
        // Nothing signs in to a broker's order plant while trading is
        // switched off, whoever asks.
        if !crate::features::trading() {
            return false;
        }
        let Some(broker) = self.broker_for(login) else { return false };
        if !self.begin(&login.id) {
            return false;
        }
        let (login, changed, wake) = (login.clone(), self.waker(&login.id), self.wake.clone());
        let feed = data.then(|| data_feed_of(&login).map(|feed| feed.id)).flatten();
        std::thread::spawn(move || {
            let session = broker.sign_in(&login, changed);
            if let Ok(session) = &session {
                let _ = session.refresh();
            }
            let fed = match (&session, feed) {
                (Ok(_), Some(feed)) => Some(crate::feeds::sign_in_now(feed, |_| {})),
                _ => None,
            };
            let heard = broker.heard_at_sign_in(&login);
            let _ = wake.try_send(Wake::Opened(login, session, fed, heard));
        });
        true
    }

    /// Sign in every login in `logins` that has a password kept and no
    /// session, its trading half alone: the data half signs in by itself as
    /// the charts ask for it. What startup calls, after the first frame.
    pub fn sign_in_saved(&self, logins: &[Login]) {
        for login in logins.iter().filter(|login| !login.signed_out) {
            if self.broker_for(login).is_some_and(|broker| broker.signs_in_saved(login)) {
                self.sign_in(login, false);
            }
        }
    }

    /// Why `login`'s data half did not sign in when its trading half did,
    /// if it did not.
    pub fn data_refused(&self, login: &str) -> Option<String> {
        self.data_refused.borrow().get(login).cloned()
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

    /// The broker `login` signs in through, if this build has it.
    pub fn broker_for(&self, login: &Login) -> Option<Arc<dyn Broker>> {
        self.brokers.get(&login.broker)
    }

    /// Every broker a login can be added for.
    pub fn brokers(&self) -> &Registry {
        &self.brokers
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
        self.data_refused.borrow_mut().remove(login);
        true
    }

    /// Take what a sign-in for `login` came back with, refreshed on its
    /// thread when it signed in. A session for a login that is no longer
    /// opening — removed meanwhile — is signed out at once rather than kept.
    pub fn opened(&self, login: &str, outcome: Result<Arc<dyn Session>, String>) {
        let wanted = self.opening.borrow_mut().remove(login);
        match outcome {
            Ok(session) if wanted => {
                self.sessions.borrow_mut().insert(login.to_string(), session);
                *self.refreshes.borrow_mut().entry(login.to_string()).or_default() += 1;
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
        self.links.borrow_mut().retain(|(of, _, _)| of != login);
        self.opening.borrow_mut().remove(login);
        self.refused.borrow_mut().remove(login);
        self.data_refused.borrow_mut().remove(login);
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

    /// How many sessions are open.
    pub fn sessions_open(&self) -> usize {
        self.sessions.borrow().len()
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
        if data && let Some(why) = data_feed_of(login).and_then(|feed| feed.setup).and_then(|setup| setup.refusal()) {
            return Standing::Refused(why);
        }
        Standing::Ready
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
    /// login's id before the number. Logins in the order given. Positions
    /// are marked to the broker's market data alone: see [`Desk::marked`].
    pub fn book(&self, logins: &[Login]) -> Book {
        self.marked(logins, |_| None)
    }

    /// [`Desk::book`], each position marked to the last trade the broker's
    /// market data has heard in its contract, else, until one is heard, to
    /// `charted`: the last price of a chart showing that very contract,
    /// [`charted_price`]. With neither, it keeps the broker's figure, and
    /// its account's open P&L is the broker's too.
    pub fn marked(&self, logins: &[Login], charted: impl Fn(&Position) -> Option<f64>) -> Book {
        let mut book = Book { refreshed: true, ..Book::default() };
        for login in logins {
            let Some(session) = self.session(&login.id) else { continue };
            let mut theirs = session.book();
            theirs.mark(|held| session.price_of(&held.contract, &held.exchange).or_else(|| charted(held)));
            book.refreshed &= theirs.refreshed;
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
        // Switched off, the window knows of no login to trade through:
        // every trading piece is left out, as it is with none.
        if !crate::features::trading() {
            return View::default();
        }
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

    /// What the account with the key `key` is called here: "Lucid", or
    /// "Lucid 1234" when the login holds several; its number alone when it
    /// is no longer listed.
    pub fn name_of(&self, key: &str) -> String {
        self.accounts
            .iter()
            .find(|account| account.key() == key)
            .map(|account| self.account_name(account))
            .unwrap_or_else(|| key.rsplit('/').next().unwrap_or(key).to_string())
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
            false => Err("Not sent · the last order is still on its way".into()),
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
        let desk = Desk::new(Registry::of(vec![fake.clone()]));
        let login = Login {
            id: "1".into(),
            name: "Lucid".into(),
            user: "a".into(),
            broker: "fake".into(),
            values: Vec::new(),
            signed_out: false,
        };
        let open = |desk: &Desk| {
            if desk.begin(&login.id) {
                let session = fake.sign_in(&login, Box::new(|_| {}));
                desk.opened(&login.id, session);
            }
        };
        assert!(desk.begin(&login.id));
        assert!(!desk.begin(&login.id), "a second while the first is opening");
        let session = fake.sign_in(&login, Box::new(|_| {}));
        desk.opened(&login.id, session);
        assert!(desk.live(&login.id));
        open(&desk);
        open(&desk);
        assert_eq!(fake.signed_in.load(Ordering::Acquire), 1);
        assert_eq!(desk.standing(&login, false), Standing::Live);

        desk.end(&login.id);
        assert_eq!(fake.signed_out.load(Ordering::Acquire), 1);
        assert_eq!(desk.standing(&login, false), Standing::Ready);
        open(&desk);
        assert_eq!(fake.signed_in.load(Ordering::Acquire), 2);

        // A session that comes back for a login removed while it signed in
        // is signed straight out, not kept.
        desk.end(&login.id);
        assert!(desk.begin(&login.id));
        desk.end(&login.id);
        let late = fake.sign_in(&login, Box::new(|_| {}));
        desk.opened(&login.id, late);
        assert!(!desk.live(&login.id));
        assert_eq!(fake.signed_out.load(Ordering::Acquire), 3);
    }

    /// Closing signs every session out, and waits for them.
    #[test]
    fn closing_signs_every_session_out() {
        let fake = Arc::new(FakeBroker::default());
        let desk = Desk::new(Registry::of(vec![fake.clone()]));
        let logins = [test_login("1", "Lucid", TEST_SYSTEM), test_login("2", "Tradeify", TEST_SYSTEM)];
        for login in &logins {
            assert!(desk.begin(&login.id));
            desk.opened(&login.id, fake.sign_in(login, Box::new(|_| {})));
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
        let desk = Desk::new(Registry::of(vec![fake.clone()]));
        let login = logins(&store)[0].clone();
        assert!(desk.begin(&lucid));
        let session = fake.sign_in(&login, Box::new(|_| {}));
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
        assert_eq!(placing.begin("t/951").unwrap_err(), "Not sent · the last order is still on its way");
        assert!(placing.begin("l/1234").is_ok(), "another account is not held up");
        placing.done("t/951");
        assert!(placing.begin("t/951").is_ok());
    }

    /// The desk's book is every live session's, under the accounts' keys:
    /// a market order fills and is held, a limit works until cancelled, and
    /// flattening leaves nothing held. Each move calls `changed`.
    #[test]
    fn the_desks_book_follows_what_the_sessions_push() {
        let desk = Desk::new(Registry::of(vec![Arc::new(FakeBroker::default())]));
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

    /// What a broker holds for a login before it signs in: one ESZ6 long, a
    /// limit working, and earlier today a market buy filled and a limit
    /// cancelled; the account's money and its risk limits.
    pub(crate) fn held_before() -> Book {
        let at = |order: Order, at: i64| Order { account: "FAKE-1".into(), at, ..order };
        let mut filled = order("B1", Side::Buy, OrderKind::Market, None, Stage::Filled);
        (filled.filled, filled.fill_price) = (1, Some(7_600.0));
        filled.fills = vec![Fill { basket: "B1".into(), at: 100, qty: 1, price: 7_600.0 }];
        Book {
            orders: vec![
                at(order("B3", Side::Sell, OrderKind::Limit, Some(7_700.0), Stage::Working), 300),
                at(order("B2", Side::Buy, OrderKind::Limit, Some(7_500.0), Stage::Cancelled), 200),
                at(filled, 100),
            ],
            positions: vec![Position {
                account: "FAKE-1".into(),
                contract: "ESZ6".into(),
                exchange: "CME".into(),
                net: 1,
                avg: Some(7_600.0),
                open_pnl: Some(62.5),
                tick: Some(0.25),
                point: Some(50.0),
            }],
            accounts: vec![Money {
                key: "FAKE-1".into(),
                balance: Some(50_062.5),
                unrealized: Some(62.5),
                day_pnl: Some(62.5),
                limits: Some(Limits { loss_limit: Some(2_000.0), max_order_qty: Some(5), ..Limits::default() }),
            }],
            ..Book::default()
        }
    }

    /// Wait for the desk to be woken with what `until` looks for, taking
    /// every wake on the way as the window's loop would.
    pub(crate) fn wait_for(desk: &Desk, until: impl Fn(&Wake) -> bool) {
        let woken = desk.woken();
        loop {
            let wake = woken.recv_blocking().expect("the desk is alive");
            let done = until(&wake);
            desk.take(wake);
            if done {
                return;
            }
        }
    }

    /// With trading switched off nothing signs in to a broker, whoever
    /// asks: not startup, not the Trading page.
    #[test]
    fn nothing_signs_in_while_trading_is_off() {
        let fake = Arc::new(FakeBroker::listed());
        let desk = Desk::new(Registry::of(vec![fake.clone()]));
        let login = Login { broker: "fake".into(), ..test_login("l", "Lucid", TEST_SYSTEM) };
        assert!(!crate::features::trading());
        assert!(!desk.sign_in(&login, true));
        desk.sign_in_saved(std::slice::from_ref(&login));
        assert!(!desk.opening("l") && !desk.live("l"));
        std::thread::sleep(std::time::Duration::from_millis(20));
        assert_eq!(fake.signed_in.load(Ordering::Acquire), 0);
    }

    /// A saved login signs in on a thread of its own without anything
    /// waiting for it, and comes back with everything its account holds:
    /// the position, the working order, today's orders whatever became of
    /// them, with their fills, the money and the limits. One signed out is
    /// not signed in. After a drop the session is refreshed again, and what
    /// filled meanwhile is in the book.
    #[test]
    fn a_saved_login_signs_in_with_what_its_accounts_hold() {
        crate::features::force(crate::features::Feature::Trading, true);
        let fake = Arc::new(FakeBroker { takes: std::time::Duration::from_millis(50), ..FakeBroker::listed() });
        fake.held.lock().unwrap().insert("Lucid".into(), held_before());
        let desk = Desk::new(Registry::of(vec![fake.clone()]));
        let login = Login { broker: "fake".into(), ..test_login("l", "Lucid", TEST_SYSTEM) };
        let gone = Login { id: "g".into(), signed_out: true, ..login.clone() };

        let asked = std::time::Instant::now();
        desk.sign_in_saved(&[login.clone(), gone.clone()]);
        assert!(asked.elapsed() < std::time::Duration::from_millis(50), "nothing waits for the sign-in");
        assert!(desk.opening("l") && !desk.opening("g"));
        assert!(desk.book(std::slice::from_ref(&login)).orders.is_empty());
        wait_for(&desk, |wake| matches!(wake, Wake::Opened(..)));
        assert_eq!(fake.signed_in.load(Ordering::Acquire), 1, "the signed-out login is left alone");
        assert_eq!(desk.refreshes("l"), 1);

        let book = desk.book(std::slice::from_ref(&login));
        assert!(book.refreshed);
        assert_eq!(book.position("l/FAKE-1", "ESZ6").map(|held| held.net), Some(1));
        assert_eq!(book.working("l/FAKE-1", "ESZ6").map(|o| o.basket_id.as_str()).collect::<Vec<_>>(), ["B3"]);
        let stages: Vec<Stage> = book.orders.iter().map(|order| order.status.stage).collect();
        assert_eq!(stages, [Stage::Working, Stage::Cancelled, Stage::Filled], "today's, newest first");
        assert_eq!(book.order("B1").map(|order| order.fills.len()), Some(1));
        let money = book.money("l/FAKE-1").expect("its money");
        assert_eq!((money.balance, money.day_pnl), (Some(50_062.5), Some(62.5)));
        assert_eq!(money.limits.as_ref().and_then(|limits| limits.loss_limit), Some(2_000.0));

        // Dropped, and the limit filled meanwhile.
        let fake_session = fake.sessions.lock().unwrap()[0].clone();
        fake_session.drop_and_return(|held| {
            let limit = held.orders.iter_mut().find(|order| order.basket_id == "B3").unwrap();
            limit.status = Stage::Filled.into();
            held.positions.clear();
        });
        assert!(desk.book(std::slice::from_ref(&login)).working("l/FAKE-1", "ESZ6").next().is_none());
        assert!(!desk.book(std::slice::from_ref(&login)).refreshed, "not until it is asked again");
        wait_for(&desk, |wake| matches!(wake, Wake::Refreshed(_)));
        assert_eq!(desk.refreshes("l"), 2);
        let book = desk.book(std::slice::from_ref(&login));
        assert_eq!(book.order("B3").map(|order| order.status.stage), Some(Stage::Filled));
        assert!(book.position("l/FAKE-1", "ESZ6").is_none());
        desk.sign_out_all(SIGN_OUT_WAIT);
    }

    /// A position is marked to the broker's market data; while that has
    /// heard nothing, to a chart of that very contract, never a continuous
    /// series; with neither, the broker's figure stands and so does its
    /// account's.
    #[test]
    fn a_position_is_marked_to_its_contract_s_chart_until_the_market_data_has_a_trade() {
        use omacharts_engine::InstrumentKind::FutureRoot;
        let desk = Desk::new(Registry::of(vec![Arc::new(FakeBroker::default())]));
        let login = test_login("t", "Rithmic Test", TEST_SYSTEM);
        let session = Arc::new(FakeSession::new(vec![account("t", "951")], Box::new(|| {})));
        assert!(desk.begin("t"));
        desk.opened("t", Ok(session.clone()));
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
        let logins = vec![login];
        let (continuous, december) = (instrument("ES", FutureRoot), instrument("ESZ6", FutureRoot));
        let marked = |charts: &[(&omacharts_engine::Instrument, Option<f64>)]| {
            let book = desk.marked(&logins, |held| charted_price(held, charts.iter().copied()));
            let open = book.position("t/951", "ESZ6").and_then(|held| held.open_pnl);
            (open, book.money("t/951").and_then(|money| money.unrealized))
        };

        let broker = desk.book(&logins).position("t/951", "ESZ6").and_then(|held| held.open_pnl);
        assert_eq!(marked(&[(&continuous, Some(7_622.25))]), (broker, None), "a continuous chart is not the contract");
        assert_eq!(marked(&[(&continuous, Some(7_000.0)), (&december, Some(7_622.25))]), (Some(500.0), Some(500.0)));

        *session.market.lock().unwrap() = Some(7_602.25);
        assert_eq!(
            marked(&[(&december, Some(7_622.25))]),
            (Some(-500.0), Some(-500.0)),
            "the market data's trade wins over the chart once it has one"
        );
    }
}
