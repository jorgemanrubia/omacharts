//! Large orders for the charts of one feed: each contract's single trades,
//! kept a trading day at a time, filtered once into a [`Book`] of the
//! orders bigger than an indicator's thresholds, and handed to the charts
//! in one swap.
//!
//! **Never in the chart's way.** Nothing here runs on the thread drawing
//! the window. A contract's trades are asked for only once its bars and
//! ladders have come ([`LargeOrders::want`] is told whether they are still
//! loading, and waits), on a thread of its own, behind nothing else, so a
//! slow history plant delays no bar and no ladder. A chart is drawn at once
//! without bubbles, and the one chart that wants a book is drawn again when
//! it comes: no spinner, no hint.
//!
//! **What is asked.** Days newest first, from today back to the chart's
//! first bar: a day the store keeps whole is read from it; a day it keeps
//! part of, today, is asked for only from where it stops; any other day is
//! asked for whole. A feed that publishes no depth for its history is walked
//! back until a weekday comes back empty, and no further. A day is written
//! down as it comes, sixteen bytes a trade ([`crate::store`]'s print
//! section).
//!
//! **Live.** A feed with a stream of trades has them put in a mailbox on
//! its own thread, and drained here; a trade from before what the history
//! read is known to is left out, so none is counted twice. A feed without
//! one is asked for the minute just closed, each time one closes: what it
//! has then is whole.
//!
//! **Books.** One per contract and settings (thresholds, and whether
//! same-time prints are combined), built on the thread from every trade
//! held, and built again there when a setting changes; live trades go into
//! it as they come, and a new book is handed over each drain. A chart whose
//! thresholds change keeps the book it had beside the new one, so going
//! back is instant; any older is let go. A threshold stepped through on the
//! way to another is never built: the thread skips a build nobody wants by
//! the time it gets to it.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::time::{Duration, Instant};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};

use omacharts_engine::ladders::Days;
use omacharts_engine::studies::orders::{Book, Builder, Print, Thresholds};
use omacharts_engine::{Instrument, Provider, Subscription};

use crate::store::prints::PrintDay;
use crate::store::Store;

/// How long after a minute closes the provider is taken to have it whole.
const SETTLED: i64 = crate::at_price::SETTLED;

/// How far back a walk goes at most, in days, for a feed that says nothing
/// of its depth and never answers a weekday empty.
const DEEPEST: i64 = 400;

/// What a book is built by: an indicator's thresholds and whether prints of
/// one moment and side are one order.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Settings {
    pub regular: u64,
    pub extended: u64,
    pub combine: bool,
}

impl Settings {
    fn thresholds(&self) -> Thresholds {
        Thresholds { regular: self.regular, extended: self.extended }
    }

    fn key(&self) -> (u64, u64, bool) {
        (self.regular, self.extended, self.combine)
    }
}

#[allow(clippy::large_enum_variant)]
enum Job {
    /// Read and ask for `stored`'s trades from `from` to `now`, and build
    /// the books of `settings`.
    Load { key: String, stored: String, symbol: String, instrument: Instrument, tick_size: f64, from: i64, now: i64, settings: Vec<Settings> },
    /// Build a book of other settings from what is held.
    Build { key: String, settings: Settings },
    /// A minute closed, on a feed without a stream of trades: ask for what
    /// is new.
    Refresh { key: String, now: i64 },
    /// Trades arrived in `key`'s mailbox.
    Live { key: String },
    /// No chart shows `key` any more.
    Forget { key: String },
    /// One more day of `key`'s walk back: never sent, taken by the thread
    /// from its own list between the jobs that are.
    Walk(Walk),
}

/// A contract's walk back to its chart's first bar, a day at a time: `day`
/// is the next to read, `first` the last, `now` when the load was asked.
#[derive(Clone, Debug)]
struct Walk {
    key: String,
    day: i64,
    first: i64,
    now: i64,
}

/// What the thread hands back: a book, whole.
pub struct Done {
    pub key: String,
    pub settings: Settings,
    pub book: Arc<Book>,
}

/// One contract, on the main thread.
struct Entry {
    /// What its trades are kept under.
    stored: String,
    watchers: HashMap<u32, Settings>,
    /// The settings each chart last drew a book of before they changed:
    /// that book is kept, and kept current, so changing back draws at once.
    before: HashMap<u32, Settings>,
    from: i64,
    /// Asked for, at least once.
    asked: bool,
    books: HashMap<Settings, Arc<Book>>,
    /// Asked of the thread and not back yet.
    building: BTreeSet<(u64, u64, bool)>,
    tick_size: f64,
    live: Option<Box<dyn Subscription>>,
}

impl Entry {
    fn watched(&self, settings: &Settings) -> bool {
        self.watchers.values().any(|s| s == settings)
    }

    fn kept(&self, settings: &Settings) -> bool {
        self.watched(settings) || self.before.values().any(|s| s == settings)
    }

    /// Let go of the books no chart draws or has just drawn, and forget
    /// builds no chart wants any more, so wanting them again asks again.
    fn prune(&mut self) {
        let kept: Vec<Settings> = self.watchers.values().chain(self.before.values()).copied().collect();
        self.books.retain(|settings, _| kept.contains(settings));
        let watched: Vec<Settings> = self.watchers.values().copied().collect();
        self.building.retain(|key| watched.iter().any(|s| s.key() == *key));
    }

    /// What the thread is told of this contract's settings.
    fn wanted(&self) -> Wanted {
        Wanted {
            watched: self.watchers.values().copied().collect(),
            kept: self.watchers.values().chain(self.before.values()).copied().collect(),
        }
    }
}

/// A contract's settings, as the thread reads them: those a chart draws,
/// built when asked, and those it keeps current besides.
#[derive(Clone, Default)]
struct Wanted {
    watched: std::collections::HashSet<Settings>,
    kept: std::collections::HashSet<Settings>,
}

/// Every contract's, shared with the thread.
#[derive(Clone, Default)]
struct Wants(Arc<Mutex<HashMap<String, Wanted>>>);

impl Wants {
    fn set(&self, key: &str, wanted: Wanted) {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).insert(key.to_string(), wanted);
    }

    fn forget(&self, key: &str) {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).remove(key);
    }

    fn of(&self, key: &str) -> Wanted {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).get(key).cloned().unwrap_or_default()
    }
}

pub struct LargeOrders {
    provider: Arc<dyn Provider>,
    entries: RefCell<HashMap<String, Entry>>,
    by_pane: RefCell<HashMap<u32, String>>,
    jobs: Mutex<Option<mpsc::Sender<Job>>>,
    done: async_channel::Receiver<Done>,
    sender: mpsc::Sender<Job>,
    mailboxes: Mailboxes,
    wants: Wants,
    /// The last settled minute contracts were refreshed for.
    refreshed: std::cell::Cell<i64>,
    clock: std::cell::Cell<fn() -> i64>,
}

fn wall_clock() -> i64 {
    chrono::Utc::now().timestamp()
}

impl LargeOrders {
    /// Large orders for `provider`, kept in the database at `store`.
    pub fn new(provider: Arc<dyn Provider>, store: Option<PathBuf>) -> Option<LargeOrders> {
        provider.ladders()?;
        let path = store?;
        let (finished, done) = async_channel::unbounded();
        let (jobs, inbox) = mpsc::channel::<Job>();
        let worker = provider.clone();
        let mailboxes = Mailboxes::default();
        let shared = mailboxes.clone();
        let wants = Wants::default();
        let told = wants.clone();
        std::thread::Builder::new()
            .name("omacharts large orders".into())
            .spawn(move || work(&path, worker.as_ref(), &inbox, &finished, &shared, &told))
            .ok()?;
        Some(LargeOrders {
            mailboxes,
            wants,
            refreshed: std::cell::Cell::new(i64::MIN),
            provider,
            entries: RefCell::new(HashMap::new()),
            by_pane: RefCell::new(HashMap::new()),
            sender: jobs.clone(),
            jobs: Mutex::new(Some(jobs)),
            done,
            clock: std::cell::Cell::new(wall_clock),
        })
    }

    pub fn set_clock(&self, clock: fn() -> i64) {
        self.clock.set(clock);
    }

    pub fn done(&self) -> async_channel::Receiver<Done> {
        self.done.clone()
    }

    fn send(&self, job: Job) {
        if let Some(jobs) = self.jobs.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
            let _ = jobs.send(job);
        }
    }

    /// Does the feed have `symbol`'s single trades?
    pub fn serves(&self, symbol: &str, instrument: &Instrument) -> bool {
        instrument.tick_size().is_some() && self.provider.ladders().is_some_and(|ladders| ladders.has_prints(symbol))
    }

    /// Chart `pane` shows `key` from `from` on with a large orders indicator
    /// of `settings`. Nothing is asked while `waiting` — the chart's bars or
    /// ladders are still on their way — and the chart asks again when they
    /// have come.
    #[allow(clippy::too_many_arguments)]
    pub fn want(&self, pane: u32, key: &str, stored: &str, symbol: &str, instrument: &Instrument, from: i64, settings: Settings, waiting: bool) {
        let Some(tick_size) = instrument.tick_size().filter(|_| self.serves(symbol, instrument)) else { return };
        if self.by_pane.borrow().get(&pane).is_some_and(|was| was != key) {
            self.unwatch(pane);
        }
        self.by_pane.borrow_mut().insert(pane, key.to_string());
        let mut entries = self.entries.borrow_mut();
        let entry = entries.entry(key.to_string()).or_insert_with(|| Entry {
            stored: stored.to_string(),
            watchers: HashMap::new(),
            before: HashMap::new(),
            from,
            asked: false,
            books: HashMap::new(),
            building: BTreeSet::new(),
            tick_size,
            live: None,
        });
        stored.clone_into(&mut entry.stored);
        // The settings a chart moves from are kept only if they have a
        // book: stepped through ten values, it keeps the one it last drew.
        if let Some(was) = entry.watchers.insert(pane, settings)
            && was != settings
            && entry.books.contains_key(&was)
        {
            entry.before.insert(pane, was);
        }
        entry.prune();
        self.wants.set(key, entry.wanted());
        if waiting {
            return;
        }
        // Whether this chart's settings have a book, or one on its way:
        // asked of the book itself, never of what was asked before, so a
        // chart that changed them while it waited, or as a change of paint
        // alone, still gets one.
        let new_settings = !entry.books.contains_key(&settings) && !entry.building.contains(&settings.key());
        let reach = from < entry.from;
        if !entry.asked || reach {
            entry.asked = true;
            entry.from = entry.from.min(from);
            // The stream first, so nothing printed while the history is read
            // is missed; what it delivers from before is left out there.
            if entry.live.is_none() {
                entry.live = self.open(key, symbol, tick_size);
            }
            let wanted: Vec<Settings> = {
                let mut all: Vec<Settings> = entry.watchers.values().chain(entry.before.values()).copied().collect();
                all.sort_by_key(Settings::key);
                all.dedup();
                all
            };
            entry.building.extend(entry.watchers.values().map(Settings::key));
            let (from, now) = (entry.from, (self.clock.get())());
            drop(entries);
            self.send(Job::Load {
                key: key.to_string(),
                stored: stored.to_string(),
                symbol: symbol.to_string(),
                instrument: instrument.clone(),
                tick_size,
                from,
                now,
                settings: wanted,
            });
        } else if new_settings {
            entry.building.insert(settings.key());
            drop(entries);
            self.send(Job::Build { key: key.to_string(), settings });
        }
    }

    /// The live stream of `symbol`'s trades, into `key`'s mailbox, waking
    /// the thread once per batch it has not drained yet.
    fn open(&self, key: &str, symbol: &str, tick_size: f64) -> Option<Box<dyn Subscription>> {
        let ladders = self.provider.ladders()?;
        let mailbox = self.mailboxes.of(key);
        let wake = self.sender.clone();
        let woken = key.to_string();
        let sink: omacharts_engine::PrintSink = Box::new(move |prints: &[Print]| {
            mailbox.prints.lock().unwrap_or_else(|e| e.into_inner()).extend_from_slice(prints);
            // One wake per batch the thread has not drained yet: it disarms
            // as it takes them.
            if !mailbox.armed.swap(true, Ordering::AcqRel) {
                let _ = wake.send(Job::Live { key: woken.clone() });
            }
        });
        match ladders.watch_prints(symbol, tick_size, sink) {
            Ok(live) => live,
            Err(error) => {
                eprintln!("omacharts: large orders of {key}: no live trades: {error}");
                None
            }
        }
    }

    /// The live data moved on: once a minute, settled, every contract of a
    /// feed without a stream of trades is asked for what is new.
    pub fn tick(&self) {
        let now = (self.clock.get())();
        let minute = (now - SETTLED).div_euclid(60);
        if self.refreshed.replace(minute) == minute {
            return;
        }
        let keys: Vec<String> =
            self.entries.borrow().iter().filter(|(_, entry)| entry.asked && entry.live.is_none()).map(|(key, _)| key.clone()).collect();
        for key in keys {
            self.send(Job::Refresh { key, now });
        }
    }

    /// What the charts draw of the trades kept: each contract's stored key,
    /// and the first moment wanted of it. The cache sweep leaves it be.
    pub fn in_use(&self) -> Vec<(String, i64)> {
        self.entries.borrow().values().map(|entry| (entry.stored.clone(), entry.from)).collect()
    }

    /// The book chart `pane`'s indicator of `settings` draws, and the tick
    /// its prices are counted in.
    pub fn book(&self, key: &str, settings: Settings) -> Option<(Arc<Book>, f64)> {
        let entries = self.entries.borrow();
        let entry = entries.get(key)?;
        entry.books.get(&settings).map(|book| (book.clone(), entry.tick_size))
    }

    /// Put a finished book in place, in one swap. The charts showing it.
    pub fn finished(&self, done: Done) -> Vec<u32> {
        let mut entries = self.entries.borrow_mut();
        let Some(entry) = entries.get_mut(&done.key) else { return Vec::new() };
        entry.building.remove(&done.settings.key());
        // A book of settings every chart has moved on from is let go.
        if !entry.kept(&done.settings) {
            return Vec::new();
        }
        entry.books.insert(done.settings, done.book);
        entry.watchers.iter().filter(|(_, settings)| **settings == done.settings).map(|(pane, _)| *pane).collect()
    }

    pub fn unwatch(&self, pane: u32) {
        let Some(key) = self.by_pane.borrow_mut().remove(&pane) else { return };
        let mut entries = self.entries.borrow_mut();
        let empty = entries.get_mut(&key).is_some_and(|entry| {
            entry.watchers.remove(&pane);
            entry.before.remove(&pane);
            entry.prune();
            self.wants.set(&key, entry.wanted());
            entry.watchers.is_empty()
        });
        if empty {
            entries.remove(&key);
            self.wants.forget(&key);
            // Let go here, with the stream that fed it, and never on the
            // thread: a chart wanting the contract again straight after opens
            // a new stream into a new mailbox before the thread gets to the
            // forget, which took that new one away and left its trades where
            // nothing ever read them.
            self.mailboxes.forget(&key);
            drop(entries);
            self.send(Job::Forget { key });
        }
    }

    pub fn unwatch_all(&self) {
        let panes: Vec<u32> = self.by_pane.borrow().keys().copied().collect();
        for pane in panes {
            self.unwatch(pane);
        }
    }

    pub fn shutdown(&self) {
        self.by_pane.borrow_mut().clear();
        self.entries.borrow_mut().clear();
        self.jobs.lock().unwrap_or_else(|e| e.into_inner()).take();
    }
}

/// One contract's live trades, waiting for the thread.
#[derive(Default)]
struct Mailbox {
    prints: Mutex<Vec<Print>>,
    armed: AtomicBool,
}

/// Every contract's.
#[derive(Clone, Default)]
struct Mailboxes(Arc<Mutex<HashMap<String, Arc<Mailbox>>>>);

impl Mailboxes {
    fn of(&self, key: &str) -> Arc<Mailbox> {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).entry(key.to_string()).or_default().clone()
    }

    fn take(&self, key: &str) -> Vec<Print> {
        let mailbox = self.0.lock().unwrap_or_else(|e| e.into_inner()).get(key).cloned();
        mailbox
            .map(|m| {
                m.armed.store(false, Ordering::Release);
                std::mem::take(&mut *m.prints.lock().unwrap_or_else(|e| e.into_inner()))
            })
            .unwrap_or_default()
    }

    fn forget(&self, key: &str) {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).remove(key);
    }
}

/// One contract, on the thread.
struct Held {
    stored: String,
    /// Where the feed reads the trades from now: what is kept of them is
    /// kept with it, and a day kept from anywhere else is read again.
    source: String,
    /// The first moment a chart wants, for reading again from.
    from: i64,
    symbol: String,
    instrument: Instrument,
    tick_size: f64,
    days: Days,
    /// The days kept whole in the store, read from it a day at a time when
    /// a book is built: a quarter of ES is a quarter of a billion trades,
    /// and none of them is held here.
    whole: BTreeSet<i64>,
    /// The days still trading, or not read to their end: today, mostly.
    open: BTreeMap<i64, PrintDay>,
    /// Open days with trades the store has not had yet.
    dirty: BTreeSet<i64>,
    /// The empty weekdays a walk back has come to since the last day with
    /// trades, newest first: kept once an older day says they were holidays.
    quiet: Vec<(i64, PrintDay)>,
    saved_at: Instant,
    builders: HashMap<Settings, Builder>,
}

impl Held {
    /// What the hours say of a moment: its trading day, and whether it is
    /// in regular hours. An instrument with no regular hours is all of one
    /// kind, taken as regular.
    fn hours(&self) -> impl FnMut(i64) -> (i64, bool) + use<> {
        let schedule = omacharts_engine::indicators::time::schedule_for(Some(&self.instrument));
        let regular = schedule.filter(|s| s.has_regular());
        let days = self.days.clone();
        move |seconds| (days.of(seconds), regular.as_ref().is_none_or(|s| s.is_regular(seconds)))
    }

    /// A book of `settings` from every day held, oldest first, the whole
    /// days read from the store one at a time.
    fn build(&self, store: &Store, settings: Settings) -> Builder {
        let mut builder = Builder::new(settings.combine, settings.thresholds());
        let mut hours = self.hours();
        let mut every: Vec<i64> = self.whole.iter().chain(self.open.keys()).copied().collect();
        every.sort_unstable();
        every.dedup();
        for day in every {
            match self.open.get(&day) {
                Some(open) => open.prints.iter().for_each(|print| builder.push(*print, &mut hours)),
                None => {
                    if let Some(kept) = store.load_print_day(&self.stored, day, self.tick_size, &self.source) {
                        kept.prints.iter().for_each(|print| builder.push(*print, &mut hours));
                    }
                }
            }
        }
        builder
    }

    fn newest_upto(&self) -> i64 {
        self.open.values().next_back().map_or(i64::MIN, |day| day.upto)
    }

    /// Write the open days down, if they have changed and it has been a
    /// while, or `now` when told: a day is one blob, and a busy one is tens
    /// of megabytes, too much to write on every trade. What a crash loses is
    /// asked for again, from where the store has the day to.
    fn save(&mut self, store: &Store, now: bool) {
        if self.dirty.is_empty() || (!now && self.saved_at.elapsed() < SAVE_EVERY) {
            return;
        }
        for day in std::mem::take(&mut self.dirty) {
            if let Some(open) = self.open.get(&day) {
                let _ = store.save_print_day(&self.stored, day, open);
            }
        }
        self.saved_at = Instant::now();
    }
}

/// How often a live day is written down at most.
const SAVE_EVERY: Duration = Duration::from_secs(60);

/// The days a chart's first book reads before the rest: the trades of the
/// day it is most likely looking at come first, and the walk back after.
const FIRST_DAYS: i64 = 2;

fn work(
    path: &std::path::Path,
    provider: &dyn Provider,
    inbox: &mpsc::Receiver<Job>,
    done: &async_channel::Sender<Done>,
    mailboxes: &Mailboxes,
    wants: &Wants,
) {
    let Ok(store) = Store::open_at(path) else { return };
    let Some(ladders) = provider.ladders() else { return };
    let mut held: HashMap<String, Held> = HashMap::new();
    // How many large orders the book it handed over holds.
    let publish = |key: &str, held: &Held, settings: Settings, builder: &Builder| -> usize {
        let book = builder.book(&mut held.hours());
        let orders = book.orders.len();
        let _ = done.send_blocking(Done { key: key.to_string(), settings, book: Arc::new(book) });
        orders
    };
    let rebuild = |key: &str, contract: &mut Held, settings: &[Settings]| -> Vec<String> {
        contract.builders.clear();
        let mut told = Vec::new();
        for settings in settings {
            let builder = contract.build(&store, *settings);
            let orders = publish(key, contract, *settings, &builder);
            told.push(format!("{orders} over {}/{}", settings.regular, settings.extended));
            contract.builders.insert(*settings, builder);
        }
        told
    };
    // The walks back, a day of one taken after each job: a walk is a day of
    // trades from the provider at a time, which from Rithmic's history plant
    // is minutes, and a month of them is hours. Done in one job, as it was,
    // everything else for the contract waited behind it: the book of a
    // threshold changed meanwhile, a chart's load asked again, and every live
    // trade, while the chart drew no bubbles at all.
    let mut walks: VecDeque<Walk> = VecDeque::new();
    let mut walk_turn = false;
    loop {
        let turn = if walk_turn { walks.pop_front() } else { None };
        let job = match turn {
            Some(walk) => Job::Walk(walk),
            None => match inbox.try_recv() {
                Ok(job) => job,
                Err(mpsc::TryRecvError::Disconnected) => break,
                Err(mpsc::TryRecvError::Empty) => match walks.pop_front() {
                    Some(walk) => Job::Walk(walk),
                    None => match inbox.recv() {
                        Ok(job) => job,
                        Err(_) => break,
                    },
                },
            },
        };
        walk_turn = !matches!(job, Job::Walk(_));
        match job {
            Job::Load { key, stored, symbol, instrument, tick_size, from, now, settings } => {
                let days = Days::new(&instrument);
                let source = ladders.source(&symbol).unwrap_or_default();
                let contract = held.remove(&key).filter(|h| h.stored == stored && h.tick_size == tick_size && h.source == source);
                let mut contract = contract.unwrap_or_else(|| Held {
                    stored: stored.clone(),
                    source: source.clone(),
                    from,
                    symbol: symbol.clone(),
                    instrument: instrument.clone(),
                    tick_size,
                    days: days.clone(),
                    whole: BTreeSet::new(),
                    open: BTreeMap::new(),
                    dirty: BTreeSet::new(),
                    quiet: Vec::new(),
                    saved_at: Instant::now(),
                    builders: HashMap::new(),
                });
                // The last days first, and a book of them straight away; then
                // back to the chart's first bar a day at a time, between the
                // other jobs, and the book again at the end. Days are counted,
                // never found again from a moment: a weekend's moments are
                // Monday's trading date, and a walk begun from one skipped
                // the weekend's days, leaving one read part way for good.
                let last = days.of(now);
                let near = last - (FIRST_DAYS - 1);
                let first = days.of(from).max(last - DEEPEST);
                contract.quiet.clear();
                let walked = read_days(&store, ladders, &mut contract, first.max(near), last, now);
                let fresh = mailboxes.take(&key);
                append(&mut contract, fresh);
                let books = rebuild(&key, &mut contract, &settings);
                contract.save(&store, true);
                contract.from = contract.from.min(from);
                walks.retain(|walk| walk.key != key);
                if walked && first < near {
                    walks.push_back(Walk { key: key.clone(), day: near - 1, first, now });
                } else {
                    told(&key, &contract, &books);
                }
                held.insert(key, contract);
            }
            Job::Build { key, settings } => {
                let Some(contract) = held.get_mut(&key) else { continue };
                // Stepped past on the way to other settings: a whole history
                // read for a book nobody draws.
                let wanted = wants.of(&key);
                if !wanted.watched.contains(&settings) {
                    continue;
                }
                contract.builders.retain(|s, _| wanted.kept.contains(s));
                let builder = contract.build(&store, settings);
                publish(&key, contract, settings, &builder);
                contract.builders.insert(settings, builder);
            }
            Job::Refresh { key, now } => {
                let Some(contract) = held.get_mut(&key) else { continue };
                // The feed reads the contract from elsewhere now — Sierra
                // Chart went over to another service, and writes another
                // file of it: everything is read again from there.
                let source = ladders.source(&contract.symbol).unwrap_or_default();
                if source != contract.source {
                    contract.source = source;
                    contract.whole.clear();
                    contract.open.clear();
                    contract.dirty.clear();
                    contract.quiet.clear();
                    let settings: Vec<Settings> = contract.builders.keys().copied().collect();
                    let (first, last) = (contract.days.of(contract.from), contract.days.of(now));
                    read_days(&store, ladders, contract, first.max(last - DEEPEST), last, now);
                    let fresh = mailboxes.take(&key);
                    append(contract, fresh);
                    rebuild(&key, contract, &settings);
                    contract.save(&store, true);
                    continue;
                }
                // Only whole minutes, settled: what the feed has of them is
                // what it will always have.
                let to = (now - SETTLED).div_euclid(60) * 60;
                let since = contract.newest_upto().div_euclid(1_000_000_000).max(to - 86_400);
                if to <= since {
                    continue;
                }
                let fresh = match ladders.prints(&contract.symbol, contract.tick_size, since, to) {
                    Ok(fresh) => fresh,
                    Err(error) => {
                        eprintln!("omacharts: large orders of {key}: trades {since}..{to}: {error}");
                        continue;
                    }
                };
                let added = append(contract, fresh);
                // Known to here now, whatever came: the day of the minute
                // asked, open or not. Over a weekend that is Monday's, which
                // nothing has opened yet, and a day not noted here was asked
                // for again, a minute longer each minute, all weekend.
                let day = contract.days.of(to - 1);
                let (tick_size, source) = (contract.tick_size, contract.source.clone());
                let open = contract.open.entry(day).or_insert_with(|| PrintDay {
                    tick_size,
                    upto: i64::MIN,
                    complete: false,
                    prints: Vec::new(),
                    source,
                });
                open.upto = open.upto.max(to * 1_000_000_000);
                contract.dirty.insert(day);
                if !added.is_empty() {
                    let wanted = wants.of(&key);
                    contract.builders.retain(|s, _| wanted.kept.contains(s));
                    feed_builders(contract, &added);
                    for (settings, builder) in &contract.builders {
                        publish(&key, contract, *settings, builder);
                    }
                }
                contract.save(&store, false);
            }
            Job::Live { key } => {
                // Not loaded yet: the trades wait in the mailbox for the load,
                // which takes them once it has read the history.
                let Some(contract) = held.get_mut(&key) else { continue };
                // Every wake that queued up behind this one is this drain's.
                let fresh = mailboxes.take(&key);
                let added = append(contract, fresh);
                if !added.is_empty() {
                    let wanted = wants.of(&key);
                    contract.builders.retain(|s, _| wanted.kept.contains(s));
                    feed_builders(contract, &added);
                    for (settings, builder) in &contract.builders {
                        publish(&key, contract, *settings, builder);
                    }
                }
                contract.save(&store, false);
            }
            Job::Forget { key } => {
                if let Some(mut contract) = held.remove(&key) {
                    contract.save(&store, true);
                }
                walks.retain(|walk| walk.key != key);
            }
            Job::Walk(walk) => {
                let Some(contract) = held.get_mut(&walk.key) else { continue };
                let more = read_days(&store, ladders, contract, walk.day, walk.day, walk.now);
                if more && walk.day > walk.first {
                    walks.push_back(Walk { day: walk.day - 1, ..walk });
                    continue;
                }
                // Back as far as it goes: every book again, from it all.
                let fresh = mailboxes.take(&walk.key);
                append(contract, fresh);
                let wanted = wants.of(&walk.key);
                let mut settings: Vec<Settings> = wanted.kept.iter().copied().collect();
                settings.sort_by_key(Settings::key);
                let books = rebuild(&walk.key, contract, &settings);
                contract.save(&store, true);
                told(&walk.key, contract, &books);
            }
        }
    }
    // The window is closing: what the live feeds brought is kept.
    for contract in held.values_mut() {
        contract.save(&store, true);
    }
}

/// What came of a load, once it has walked back as far as it goes: a chart
/// with no bubbles says here whether no trades came, or none was large.
fn told(key: &str, contract: &Held, books: &[String]) {
    let open: usize = contract.open.values().map(|day| day.prints.len()).sum();
    eprintln!(
        "omacharts: large orders of {key}: {} whole days kept, {open} trades of open days, large orders {}",
        contract.whole.len(),
        books.join(", ")
    );
}

/// Fold `added` into every book of `contract`.
fn feed_builders(contract: &mut Held, added: &[Print]) {
    let mut hours = contract.hours();
    for builder in contract.builders.values_mut() {
        for print in added {
            builder.push(*print, &mut hours);
        }
    }
}

/// Put `fresh` trades into their days, leaving out any from before what a
/// day is known to — already counted — and any of a day kept whole. The
/// trades that went in, oldest first.
fn append(contract: &mut Held, mut fresh: Vec<Print>) -> Vec<Print> {
    fresh.sort_by_key(|p| p.ts_nanos);
    let mut added = Vec::with_capacity(fresh.len());
    for print in fresh {
        let day = contract.days.of(print.ts_nanos.div_euclid(1_000_000_000));
        if contract.whole.contains(&day) {
            continue;
        }
        let open = contract.open.entry(day).or_insert_with(|| PrintDay {
            tick_size: contract.tick_size,
            upto: i64::MIN,
            complete: false,
            prints: Vec::new(),
            source: contract.source.clone(),
        });
        if print.ts_nanos < open.upto {
            continue;
        }
        open.prints.push(print);
        open.upto = print.ts_nanos + 1;
        contract.dirty.insert(day);
        added.push(print);
    }
    added
}

/// Read `contract`'s days `first..=last`, newest first, as they stand at
/// `now`: a day the store has whole is only noted; any other is asked of the
/// provider from where it is known to. Answers false when there is nothing
/// further back: [`EMPTY_RUN`] weekdays in a row the provider has nothing
/// of, with no day kept before them.
///
/// One empty weekday is not the end: an exchange holiday is one, and the
/// days before it are still there. An empty weekday is kept, as empty and
/// whole, once a day older than it is found with trades, so a holiday is
/// asked about once. Nor is a run of them the end when older days are kept:
/// the days between are ones the cache let go, and the provider no longer
/// has.
fn read_days(store: &Store, ladders: &dyn omacharts_engine::Ladders, contract: &mut Held, first: i64, last: i64, now: i64) -> bool {
    let days = contract.days.clone();
    let settled = (now - SETTLED).div_euclid(60) * 60;
    // The empty weekdays since the last day with trades, kept as holidays.
    let keep = |store: &Store, contract: &mut Held| {
        for (day, held) in std::mem::take(&mut contract.quiet) {
            let _ = store.save_print_day(&contract.stored, day, &held);
            contract.whole.insert(day);
        }
    };
    let mut day = last;
    while day >= first {
        let (start, end) = (days.start(day), days.start(day + 1));
        // Asked of the store every time, never of what was noted: a day the
        // cache sweep let go is read again.
        if store.print_day_complete(&contract.stored, day, contract.tick_size, &contract.source) {
            keep(store, contract);
            contract.whole.insert(day);
            day -= 1;
            continue;
        }
        contract.whole.remove(&day);
        let mut open = contract
            .open
            .remove(&day)
            .or_else(|| store.load_print_day(&contract.stored, day, contract.tick_size, &contract.source))
            .unwrap_or_else(|| PrintDay {
                tick_size: contract.tick_size,
                upto: i64::MIN,
                complete: false,
                prints: Vec::new(),
                source: contract.source.clone(),
            });
        // A weekday known to some moment with not one trade in it was asked
        // before the feed had it: asked whole again, never taken as quiet.
        if open.prints.is_empty() && is_weekday(start) {
            open.upto = i64::MIN;
        }
        let since = open.upto.div_euclid(1_000_000_000).max(start);
        let to = end.min(settled);
        if since < to {
            let fresh = match ladders.prints(&contract.symbol, contract.tick_size, since, to) {
                Ok(fresh) => fresh,
                Err(error) => {
                    eprintln!("omacharts: large orders of {}: trades {since}..{to}: {error}", contract.stored);
                    return false;
                }
            };
            let floor = open.upto;
            open.prints.extend(fresh.into_iter().filter(|p| p.ts_nanos >= floor && p.ts_nanos < to * 1_000_000_000));
            open.prints.sort_by_key(|p| p.ts_nanos);
            open.upto = open.upto.max(to * 1_000_000_000);
        }
        open.complete = end <= settled;
        // A weekday with nothing, before today: a holiday, a day let go, or
        // past what the feed keeps. Not written down until an older day says
        // which, so a feed that later keeps more is asked again.
        if open.prints.is_empty() && open.complete && is_weekday(start) {
            contract.quiet.push((day, open));
            if contract.quiet.len() >= EMPTY_RUN
                && !store.print_days_kept_before(&contract.stored, day, contract.tick_size, &contract.source)
            {
                return false;
            }
            day -= 1;
            continue;
        }
        if !open.prints.is_empty() {
            keep(store, contract);
        }
        let _ = store.save_print_day(&contract.stored, day, &open);
        match open.complete {
            true => {
                contract.whole.insert(day);
            }
            false => {
                contract.open.insert(day, open);
            }
        }
        day -= 1;
    }
    // Back to the chart's first day on a holiday: one with days kept before
    // it is one too.
    if let Some(&(oldest, _)) = contract.quiet.last()
        && store.print_days_kept_before(&contract.stored, oldest, contract.tick_size, &contract.source)
    {
        keep(store, contract);
    }
    true
}

/// Weekdays in a row with no trades that end a walk back: more than a
/// holiday, or a holiday beside a weekend, ever makes.
const EMPTY_RUN: usize = 3;

/// Does a trading day beginning at `start` fall on a weekday? A day that
/// begins on a Sunday evening is Monday's.
fn is_weekday(start: i64) -> bool {
    use chrono::Datelike;
    let Some(begins) = chrono::DateTime::from_timestamp(start + 12 * 3_600, 0) else { return true };
    !matches!(begins.weekday(), chrono::Weekday::Sat | chrono::Weekday::Sun)
}

#[cfg(test)]
pub(crate) mod tests {
    use std::time::{Duration, Instant};

    use omacharts_engine::studies::orders::Aggressor;
    use omacharts_engine::{Bar, Capability, InstrumentKind, Ladders, LadderSink, PrintSink, ProviderError, Timeframe};

    use super::*;

    const KEY: &str = "fake:ES";
    /// 2026-11-03 03:20 UTC, a Tuesday: an ES session in progress.
    const NOW: i64 = 1_793_700_000;
    const HOUR: i64 = 3_600;
    const DAY: i64 = 86_400;

    fn now() -> i64 {
        NOW
    }

    #[derive(Default)]
    pub(crate) struct Asked {
        spans: Vec<(i64, i64)>,
        sinks: Vec<PrintSink>,
        /// The feed has nothing from before here.
        depth: i64,
        /// Nor from here on, when set: a file no longer written.
        stops: Option<i64>,
        /// Where it says it reads the trades from.
        source: Option<String>,
        /// When set, a request for minute ladders waits until this hears
        /// something or its sender goes: a history plant slow to answer.
        pub(crate) ladders_held: Option<std::sync::mpsc::Receiver<()>>,
        /// How long a day from before `slow_before` takes to come, as a day
        /// of ES takes from Rithmic's history plant.
        slow: Duration,
        slow_before: i64,
        /// Spans the feed has nothing of: an exchange holiday.
        quiet: Vec<(i64, i64)>,
        /// No stream of trades: asked a minute at a time instead.
        unstreamed: bool,
    }

    /// A feed of single trades, one an hour: what the window's tests chart
    /// large orders from too.
    pub(crate) struct Fake(pub(crate) Arc<Mutex<Asked>>);

    struct Open;
    impl Subscription for Open {}

    /// A trade an hour on the hour: 100 lots at even hours, 10 at odd.
    fn hourly(from: i64, to: i64) -> Vec<Print> {
        let first = from.div_euclid(HOUR) * HOUR + if from.rem_euclid(HOUR) == 0 { 0 } else { HOUR };
        (first..to)
            .step_by(HOUR as usize)
            .map(|ts| {
                let size = if (ts / HOUR) % 2 == 0 { 100 } else { 10 };
                Print::new(ts * 1_000_000_000, 30_000, size, Aggressor::Buy, false)
            })
            .collect()
    }

    impl Ladders for Fake {
        fn serves(&self, _: &str) -> bool {
            true
        }
        fn history_days(&self) -> u32 {
            30
        }
        fn minutes(&self, _: &str, _: f64, _: i64, _: i64) -> Result<Vec<omacharts_engine::ladders::Minute>, ProviderError> {
            let held = self.0.lock().unwrap().ladders_held.take();
            if let Some(held) = held {
                let _ = held.recv();
            }
            Ok(Vec::new())
        }
        fn watch(&self, _: &str, _: f64, _: LadderSink) -> Result<Box<dyn Subscription>, ProviderError> {
            Ok(Box::new(Open))
        }
        fn has_prints(&self, _: &str) -> bool {
            true
        }
        fn prints(&self, _: &str, _: f64, from: i64, to: i64) -> Result<Vec<Print>, ProviderError> {
            let slow = {
                let asked = self.0.lock().unwrap();
                if from < asked.slow_before { asked.slow } else { Duration::ZERO }
            };
            std::thread::sleep(slow);
            let mut asked = self.0.lock().unwrap();
            asked.spans.push((from, to));
            let mut prints = hourly(from.max(asked.depth), asked.stops.map_or(to, |stops| to.min(stops)));
            prints.retain(|print| !asked.quiet.iter().any(|(a, b)| (a * 1_000_000_000..b * 1_000_000_000).contains(&print.ts_nanos)));
            Ok(prints)
        }
        fn source(&self, _: &str) -> Option<String> {
            self.0.lock().unwrap().source.clone()
        }
        fn watch_prints(&self, _: &str, _: f64, sink: PrintSink) -> Result<Option<Box<dyn Subscription>>, ProviderError> {
            let mut asked = self.0.lock().unwrap();
            if asked.unstreamed {
                return Ok(None);
            }
            asked.sinks.push(sink);
            Ok(Some(Box::new(Open)))
        }
    }

    impl Provider for Fake {
        fn id(&self) -> &'static str {
            "fake"
        }
        fn label(&self) -> &'static str {
            "Fake"
        }
        fn delay_minutes(&self, _: InstrumentKind) -> u32 {
            0
        }
        fn native_symbol(&self, instrument: &Instrument) -> Option<String> {
            Some(instrument.symbol.clone())
        }
        fn capabilities(&self) -> &'static [Capability] {
            &[]
        }
        fn bars(&self, _: &str, _: Timeframe, _: Option<i64>) -> Result<Vec<Bar>, ProviderError> {
            Ok(Vec::new())
        }
        fn ladders(&self) -> Option<&dyn Ladders> {
            Some(self)
        }
    }

    pub(crate) fn es() -> Instrument {
        Instrument {
            symbol: "ES".into(),
            name: "E-mini S&P 500".into(),
            kind: InstrumentKind::FutureRoot,
            suffix: None,
            currency: Some("USD".into()),
            tier: 0,
            session_origin: 79_200,
            overrides: Vec::new(),
            exchange: None,
            popularity: 0,
            local_name: None,
            formula: None,
        }
    }

    pub(crate) fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("omacharts-large-orders-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("omacharts.db");
        drop(Store::open_at(&path).unwrap());
        path
    }

    const SETTINGS: Settings = Settings { regular: 50, extended: 50, combine: true };

    /// The next book handed back, put in place.
    fn next(large: &LargeOrders) -> Vec<u32> {
        let until = Instant::now() + Duration::from_secs(10);
        loop {
            if let Ok(done) = large.done().try_recv() {
                return large.finished(done);
            }
            assert!(Instant::now() < until, "no book came");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// Every book handed back until the thread goes quiet.
    fn settle(large: &LargeOrders) -> Vec<u32> {
        let mut panes = next(large);
        loop {
            std::thread::sleep(Duration::from_millis(100));
            let Ok(done) = large.done().try_recv() else { break };
            panes.extend(large.finished(done));
        }
        panes.sort_unstable();
        panes.dedup();
        panes
    }

    fn sizes(large: &LargeOrders) -> Vec<u64> {
        large.book(KEY, SETTINGS).map(|(book, _)| book.orders.iter().map(|o| o.order.size).collect()).unwrap_or_default()
    }

    #[test]
    fn nothing_is_asked_while_the_chart_waits_and_then_the_days_come_newest_first() {
        let asked = Arc::new(Mutex::new(Asked::default()));
        let large = LargeOrders::new(Arc::new(Fake(asked.clone())), Some(scratch("first"))).unwrap();
        large.set_clock(now);
        large.want(1, KEY, KEY, "ES", &es(), NOW - DAY, SETTINGS, true);
        std::thread::sleep(Duration::from_millis(50));
        assert!(asked.lock().unwrap().spans.is_empty(), "bars and ladders first");
        assert!(asked.lock().unwrap().sinks.is_empty());

        large.want(1, KEY, KEY, "ES", &es(), NOW - DAY, SETTINGS, false);
        assert_eq!(settle(&large), vec![1]);
        let spans = asked.lock().unwrap().spans.clone();
        assert!(spans.len() >= 2 && spans.windows(2).all(|w| w[0].0 > w[1].0), "newest first: {spans:?}");
        let (book, tick) = large.book(KEY, SETTINGS).unwrap();
        assert_eq!(tick, 0.25);
        assert!(book.orders.iter().all(|o| o.order.size == 100), "only the orders over the threshold");
        assert!(!book.orders.is_empty());
    }

    #[test]
    fn days_kept_are_not_asked_again_and_live_trades_are_never_counted_twice() {
        let path = scratch("kept");
        let asked = Arc::new(Mutex::new(Asked::default()));
        {
            let large = LargeOrders::new(Arc::new(Fake(asked.clone())), Some(path.clone())).unwrap();
            large.set_clock(now);
            large.want(1, KEY, KEY, "ES", &es(), NOW - 2 * DAY, SETTINGS, false);
            settle(&large);
            large.shutdown();
        }
        asked.lock().unwrap().spans.clear();
        let large = LargeOrders::new(Arc::new(Fake(asked.clone())), Some(path)).unwrap();
        large.set_clock(now);
        large.want(1, KEY, KEY, "ES", &es(), NOW - 2 * DAY, SETTINGS, false);
        settle(&large);
        let had = sizes(&large).len();
        let spans = asked.lock().unwrap().spans.clone();
        assert!(spans.iter().all(|(from, _)| *from >= NOW - DAY), "only today's tail is asked again: {spans:?}");

        // The stream delivers one trade the history read already, and one
        // new: only the new one goes in.
        let sink = asked.lock().unwrap().sinks.pop().expect("a stream");
        let old = hourly(NOW - 2 * HOUR, NOW - 2 * HOUR + 1);
        let fresh = Print::new((NOW + 10) * 1_000_000_000, 30_000, 500, Aggressor::Sell, false);
        sink(&[old[0], fresh]);
        settle(&large);
        let now_sizes = sizes(&large);
        assert_eq!(now_sizes.len(), had + 1);
        assert_eq!(now_sizes.last(), Some(&500));
    }

    /// Days read from a file Sierra Chart stopped writing part way are read
    /// again whole once it reads the contract from the file written now:
    /// the empty and the cut-short ones alike.
    #[test]
    fn days_read_from_a_file_given_up_are_read_again_from_the_one_written_now() {
        let path = scratch("source");
        let stale = Asked { stops: Some(NOW - DAY), source: Some("ESZ6.CME".into()), ..Asked::default() };
        let asked = Arc::new(Mutex::new(stale));
        {
            let large = LargeOrders::new(Arc::new(Fake(asked.clone())), Some(path.clone())).unwrap();
            large.set_clock(now);
            large.want(1, KEY, KEY, "ES", &es(), NOW - 3 * DAY, SETTINGS, false);
            settle(&large);
            assert!(sizes(&large).len() < 3 * 12, "the stale file had all of it");
            large.shutdown();
        }
        let short = {
            let mut asked = asked.lock().unwrap();
            *asked = Asked { source: Some("ESZ26_FUT_CME".into()), ..Asked::default() };
            asked.spans.len()
        };
        let large = LargeOrders::new(Arc::new(Fake(asked.clone())), Some(path)).unwrap();
        large.set_clock(now);
        large.want(1, KEY, KEY, "ES", &es(), NOW - 3 * DAY, SETTINGS, false);
        settle(&large);
        let spans = asked.lock().unwrap().spans.clone();
        assert_eq!(short, 0);
        assert!(spans.iter().any(|(from, _)| *from < NOW - 2 * DAY), "the kept days were taken as this file's: {spans:?}");
        let whole: usize = hourly(NOW - 3 * DAY, NOW - SETTLED).iter().filter(|print| print.size() == 100).count();
        assert!(sizes(&large).len() + 2 >= whole, "{} of {whole} orders", sizes(&large).len());
    }

    #[test]
    fn a_walk_back_stops_at_a_run_of_weekdays_the_feed_has_nothing_of() {
        let asked = Arc::new(Mutex::new(Asked { depth: NOW - 3 * DAY, ..Asked::default() }));
        let large = LargeOrders::new(Arc::new(Fake(asked.clone())), Some(scratch("depth"))).unwrap();
        large.set_clock(now);
        large.want(1, KEY, KEY, "ES", &es(), NOW - 60 * DAY, SETTINGS, false);
        settle(&large);
        let spans = asked.lock().unwrap().spans.clone();
        assert!(spans.len() <= 8, "stopped near the feed's depth, not sixty days back: {}", spans.len());
    }

    /// A weekday `back` trading days before today's.
    fn weekday_back(back: i64) -> (i64, i64) {
        let days = Days::new(&es());
        let mut day = days.of(NOW) - back;
        while !is_weekday(days.start(day)) {
            day -= 1;
        }
        (days.start(day), days.start(day + 1))
    }

    /// An exchange holiday is one empty weekday, not the end of history:
    /// the days before it are read, and the holiday is kept as empty, so
    /// the next load asks for none of it again.
    #[test]
    fn a_holiday_is_walked_past_and_asked_about_once() {
        let path = scratch("holiday");
        let holiday = weekday_back(8);
        let asked = Arc::new(Mutex::new(Asked { quiet: vec![holiday], ..Asked::default() }));
        {
            let large = LargeOrders::new(Arc::new(Fake(asked.clone())), Some(path.clone())).unwrap();
            large.set_clock(now);
            large.want(1, KEY, KEY, "ES", &es(), NOW - 14 * DAY, SETTINGS, false);
            settle(&large);
            let spans = asked.lock().unwrap().spans.clone();
            assert!(spans.iter().any(|(from, _)| *from < holiday.0), "stopped at the holiday: {spans:?}");
            large.shutdown();
        }
        asked.lock().unwrap().spans.clear();
        let large = LargeOrders::new(Arc::new(Fake(asked.clone())), Some(path)).unwrap();
        large.set_clock(now);
        large.want(1, KEY, KEY, "ES", &es(), NOW - 14 * DAY, SETTINGS, false);
        settle(&large);
        let spans = asked.lock().unwrap().spans.clone();
        assert!(spans.iter().all(|(from, _)| *from >= NOW - DAY), "asked about again: {spans:?}");
    }

    /// Days the cache let go, older than the feed still has: a gap, not the
    /// end of history. The days kept before it still draw their bubbles.
    #[test]
    fn days_let_go_past_the_feed_s_depth_are_not_the_end_of_history() {
        let path = scratch("let-go");
        let asked = Arc::new(Mutex::new(Asked::default()));
        {
            let large = LargeOrders::new(Arc::new(Fake(asked.clone())), Some(path.clone())).unwrap();
            large.set_clock(now);
            large.want(1, KEY, KEY, "ES", &es(), NOW - 14 * DAY, SETTINGS, false);
            settle(&large);
            large.shutdown();
        }
        // The sweep let a week go, and the feed now keeps only the last week.
        let (gap_from, gap_to) = (weekday_back(12).0, weekday_back(7).1);
        let days = Days::new(&es());
        let store = Store::open_at(&path).unwrap();
        for day in days.of(gap_from)..days.of(gap_to) {
            store.forget_print_day(KEY, day).unwrap();
        }
        drop(store);
        asked.lock().unwrap().depth = NOW - 6 * DAY;
        let large = LargeOrders::new(Arc::new(Fake(asked.clone())), Some(path)).unwrap();
        large.set_clock(now);
        large.want(1, KEY, KEY, "ES", &es(), NOW - 14 * DAY, SETTINGS, false);
        settle(&large);
        let orders = of(&large, SETTINGS).unwrap_or_default();
        assert!(
            orders.iter().any(|(ts, _)| *ts < gap_from * 1_000_000_000),
            "the days kept before the gap were left out"
        );
    }

    static CLOCK: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(0);

    fn clock() -> i64 {
        CLOCK.load(Ordering::SeqCst)
    }

    /// A feed with no stream of trades, over a weekend: each minute asks
    /// for the minute, not for everything since Friday's close again.
    #[test]
    fn over_a_weekend_a_feed_without_a_stream_asks_a_minute_at_a_time() {
        // Friday 10:00 UTC, the last trade at 20:00, then Saturday noon:
        // a trading day nothing has opened, as Monday's is on Sunday.
        let friday = NOW + 3 * DAY;
        let close = NOW + 3 * DAY + 10 * HOUR;
        let saturday = NOW + 4 * DAY + 2 * HOUR;
        let asked = Arc::new(Mutex::new(Asked { unstreamed: true, stops: Some(close), ..Asked::default() }));
        let large = LargeOrders::new(Arc::new(Fake(asked.clone())), Some(scratch("weekend"))).unwrap();
        CLOCK.store(friday, Ordering::SeqCst);
        large.set_clock(clock);
        large.want(1, KEY, KEY, "ES", &es(), friday - DAY, SETTINGS, false);
        settle(&large);
        for minute in 0..3 {
            CLOCK.store(saturday + minute * 60, Ordering::SeqCst);
            large.tick();
            let until = Instant::now() + Duration::from_secs(5);
            while asked.lock().unwrap().spans.last().is_none_or(|(_, to)| *to < saturday + minute * 60 - 120) {
                assert!(Instant::now() < until, "not asked");
                std::thread::sleep(Duration::from_millis(5));
            }
        }
        let spans = asked.lock().unwrap().spans.clone();
        let (from, to) = spans.last().copied().unwrap();
        assert!(to - from <= 60, "asked for {} seconds again: {spans:?}", to - from);
    }

    #[test]
    fn other_thresholds_are_built_from_what_is_held_without_asking_again() {
        let asked = Arc::new(Mutex::new(Asked::default()));
        let large = LargeOrders::new(Arc::new(Fake(asked.clone())), Some(scratch("rebuild"))).unwrap();
        large.set_clock(now);
        large.want(1, KEY, KEY, "ES", &es(), NOW - DAY, SETTINGS, false);
        settle(&large);
        let spans = asked.lock().unwrap().spans.len();
        let lower = Settings { regular: 5, extended: 5, combine: true };
        large.want(1, KEY, KEY, "ES", &es(), NOW - DAY, lower, false);
        assert_eq!(settle(&large), vec![1]);
        assert_eq!(asked.lock().unwrap().spans.len(), spans, "nothing asked of the feed");
        let (book, _) = large.book(KEY, lower).unwrap();
        assert!(book.orders.iter().any(|o| o.order.size == 10));
    }

    /// `chart set` puts a new pane in the old one's place, over ladders
    /// already held: nothing loads, so nothing works the chart out again,
    /// and its first computation must ask for the trades itself.
    #[test]
    fn a_chart_put_back_over_held_ladders_gets_its_bubbles() {
        let asked = Arc::new(Mutex::new(Asked::default()));
        let path = scratch("put-back");
        let provider: Arc<dyn Provider> = Arc::new(Fake(asked));
        let at_price = std::rc::Rc::new(crate::at_price::AtPrice::new(provider.clone(), Some(path.clone())).unwrap());
        at_price.set_clock(now);
        let large = std::rc::Rc::new(LargeOrders::new(provider, Some(path)).unwrap());
        large.set_clock(now);
        let source = crate::ui::pane::LadderSource {
            at_price: at_price.clone(),
            large_orders: Some(large.clone()),
            key: KEY.into(),
            symbol: "ES".into(),
        };
        let bars = [Bar { ts: NOW - DAY, open: 1.0, high: 1.0, low: 1.0, close: 1.0, volume: 0.0 }];
        let mut indicator = omacharts_engine::Indicator::new(1, omacharts_engine::indicators::Kind::LargeOrders);
        if let omacharts_engine::Params::LargeOrders { regular, extended, combine, .. } = &mut indicator.params {
            (*regular, *extended, *combine) = (SETTINGS.regular, SETTINGS.extended, SETTINGS.combine);
        }
        let all = [indicator];

        source.want(1, &es(), &bars, &all, true);
        let done = at_price.done().recv_blocking().expect("the ladders come");
        at_price.finished(done);
        source.want(1, &es(), &bars, &all, true);
        assert_eq!(settle(&large), vec![1]);
        let before = sizes(&large);
        assert!(!before.is_empty());

        large.unwatch(1);
        source.want(2, &es(), &bars, &all, true);
        assert_eq!(settle(&large), vec![2], "asked once, by its first computation");
        assert_eq!(sizes(&large), before);
        at_price.shutdown();
        large.shutdown();
    }

    #[test]
    fn hidden_and_shown_again_a_chart_gets_its_bubbles_back() {
        let asked = Arc::new(Mutex::new(Asked::default()));
        let large = LargeOrders::new(Arc::new(Fake(asked.clone())), Some(scratch("again"))).unwrap();
        large.set_clock(now);
        large.want(1, KEY, KEY, "ES", &es(), NOW - DAY, SETTINGS, false);
        settle(&large);
        let before = sizes(&large);
        assert!(!before.is_empty());
        large.unwatch(1);
        assert!(large.book(KEY, SETTINGS).is_none());
        large.want(1, KEY, KEY, "ES", &es(), NOW - DAY, SETTINGS, false);
        assert_eq!(settle(&large), vec![1]);
        assert_eq!(sizes(&large), before);
    }

    fn of(large: &LargeOrders, settings: Settings) -> Option<Vec<(i64, u64)>> {
        large.book(KEY, settings).map(|(book, _)| book.orders.iter().map(|o| (o.order.ts_nanos, o.order.size)).collect())
    }

    /// The author's report: bubbles at 5 and 5, none at all with either
    /// threshold at 6, and none again once the old ones were put back. A
    /// threshold changed gets its own book, a subset, and the one before
    /// it is still there to go back to.
    #[test]
    fn thresholds_changed_and_changed_back_each_get_their_book() {
        let asked = Arc::new(Mutex::new(Asked::default()));
        let large = LargeOrders::new(Arc::new(Fake(asked)), Some(scratch("changed"))).unwrap();
        large.set_clock(now);
        let five = Settings { regular: 5, extended: 5, combine: true };
        let six = Settings { regular: 5, extended: 60, combine: true };
        large.want(1, KEY, KEY, "ES", &es(), NOW - DAY, five, false);
        settle(&large);
        let at_five = of(&large, five).expect("the first book");
        assert!(!at_five.is_empty());

        large.want(1, KEY, KEY, "ES", &es(), NOW - DAY, six, false);
        assert_eq!(settle(&large), vec![1], "the chart is told its book came");
        let at_six = of(&large, six).expect("a book for the new thresholds");
        assert!(!at_six.is_empty() && at_six.len() < at_five.len(), "a subset, not a cliff: {} of {}", at_six.len(), at_five.len());
        assert!(at_six.iter().all(|order| at_five.contains(order)));

        // Back: drawn at once from the book kept, nothing to wait for.
        large.want(1, KEY, KEY, "ES", &es(), NOW - DAY, five, false);
        assert_eq!(of(&large, five), Some(at_five));
    }

    /// Changed while the chart waits for its bars or ladders, the chart's
    /// settings were taken as asked for already, and their book was never
    /// built when it stopped waiting.
    #[test]
    fn thresholds_changed_while_the_chart_waits_still_get_their_book() {
        let asked = Arc::new(Mutex::new(Asked::default()));
        let large = LargeOrders::new(Arc::new(Fake(asked)), Some(scratch("waiting"))).unwrap();
        large.set_clock(now);
        large.want(1, KEY, KEY, "ES", &es(), NOW - DAY, SETTINGS, false);
        settle(&large);
        let lower = Settings { regular: 5, extended: 5, combine: true };
        large.want(1, KEY, KEY, "ES", &es(), NOW - DAY, lower, true);
        large.want(1, KEY, KEY, "ES", &es(), NOW - DAY, lower, false);
        assert_eq!(settle(&large), vec![1]);
        assert!(of(&large, lower).is_some_and(|book| book.iter().any(|(_, size)| *size == 10)));
    }

    /// A box stepped through ten values builds the last, keeps the one the
    /// chart drew before them, and holds no book of the values in between.
    #[test]
    fn thresholds_stepped_through_keep_only_the_last_two_books() {
        let asked = Arc::new(Mutex::new(Asked::default()));
        let large = LargeOrders::new(Arc::new(Fake(asked)), Some(scratch("stepped"))).unwrap();
        large.set_clock(now);
        let at = |regular: u64| Settings { regular, extended: 50, combine: true };
        large.want(1, KEY, KEY, "ES", &es(), NOW - DAY, at(50), false);
        settle(&large);
        for regular in (1..10).rev().map(|n| n * 5) {
            large.want(1, KEY, KEY, "ES", &es(), NOW - DAY, at(regular), false);
        }
        settle(&large);
        assert!(of(&large, at(5)).is_some(), "the last built");
        assert!(of(&large, at(50)).is_some(), "the one drawn before kept");
        assert!((10..50).step_by(5).all(|regular| of(&large, at(regular)).is_none()), "none of those stepped through held");
    }

    /// Orders by when and how big.
    type Orders = Vec<(i64, u64)>;

    /// A book's regular hours' orders and its extended hours'.
    fn split(large: &LargeOrders, settings: Settings) -> (Orders, Orders) {
        let (book, _) = large.book(KEY, settings).expect("a book");
        let of = |regular: bool| {
            book.orders.iter().filter(|o| o.regular == regular).map(|o| (o.order.ts_nanos, o.order.size)).collect::<Vec<_>>()
        };
        (of(true), of(false))
    }

    /// Each threshold answers for its own hours: the author's change of the
    /// overnight one took the day's bubbles with it.
    #[test]
    fn a_threshold_changed_leaves_the_other_sessions_bubbles_as_they_were() {
        let asked = Arc::new(Mutex::new(Asked::default()));
        let large = LargeOrders::new(Arc::new(Fake(asked)), Some(scratch("sessions"))).unwrap();
        large.set_clock(now);
        let both = Settings { regular: 5, extended: 5, combine: true };
        large.want(1, KEY, KEY, "ES", &es(), NOW - DAY, both, false);
        settle(&large);
        let (day, night) = split(&large, both);
        assert!(!day.is_empty() && !night.is_empty());

        let overnight = Settings { extended: 50, ..both };
        large.want(1, KEY, KEY, "ES", &es(), NOW - DAY, overnight, false);
        settle(&large);
        let (day_now, night_now) = split(&large, overnight);
        assert_eq!(day_now, day, "the day's bubbles untouched");
        assert!(!night_now.is_empty() && night_now.len() < night.len());

        let daytime = Settings { regular: 50, ..both };
        large.want(1, KEY, KEY, "ES", &es(), NOW - DAY, daytime, false);
        settle(&large);
        let (day_now, night_now) = split(&large, daytime);
        assert_eq!(night_now, night, "the night's bubbles untouched");
        assert!(!day_now.is_empty() && day_now.len() < day.len());
    }

    /// 2026-11-23 15:00 UTC, a Monday, ES trading.
    const MONDAY: i64 = 1_795_446_000;

    fn monday() -> i64 {
        MONDAY
    }

    /// The store as a Rithmic chart of ES left it: weekdays kept whole, a
    /// Saturday read a minute short of its end and never finished, Sunday
    /// whole and empty, and Monday read to early morning. A chart of it the
    /// next time is drawn with its bubbles, and the Saturday finished.
    #[test]
    fn a_store_left_part_read_by_a_streaming_feed_still_draws_its_bubbles() {
        let path = scratch("left");
        let days = Days::new(&es());
        let today = days.of(MONDAY);
        {
            let store = Store::open_at(&path).unwrap();
            for day in today - 19..=today {
                let (start, end) = (days.start(day), days.start(day + 1));
                let weekend = !is_weekday(start);
                let (upto, complete) = match day - today {
                    0 => (MONDAY - 10 * HOUR, false),
                    -2 => (end - 60, false),
                    _ => (end, true),
                };
                let prints = if weekend { Vec::new() } else { hourly(start, upto) };
                let kept = PrintDay { tick_size: 0.25, upto: upto * 1_000_000_000, complete, prints, source: String::new() };
                store.save_print_day(KEY, day, &kept).unwrap();
            }
        }
        let asked = Arc::new(Mutex::new(Asked::default()));
        let large = LargeOrders::new(Arc::new(Fake(asked.clone())), Some(path.clone())).unwrap();
        large.set_clock(monday);
        large.want(4, KEY, KEY, "ES", &es(), MONDAY - 18 * DAY, SETTINGS, false);
        assert!(settle(&large).contains(&4));
        let (book, _) = large.book(KEY, SETTINGS).expect("a book");
        assert!(book.orders.len() > 100, "{} orders", book.orders.len());
        let store = Store::open_at(&path).unwrap();
        assert!(store.print_day_complete(KEY, today - 2, 0.25, ""), "the Saturday is finished");
    }

    /// The book of a threshold changed while the walk back is still asking
    /// a feed for its days, slowly, as Rithmic's history plant answers a
    /// day of ES, comes at once, and so do live trades: the walk is a day
    /// at a time between them, never one job everything waits behind.
    #[test]
    fn a_slow_walk_back_never_holds_up_a_new_threshold_or_live_trades() {
        let asked = Arc::new(Mutex::new(Asked {
            slow: Duration::from_millis(400),
            slow_before: NOW - 2 * DAY,
            ..Asked::default()
        }));
        let large = LargeOrders::new(Arc::new(Fake(asked.clone())), Some(scratch("slow"))).unwrap();
        large.set_clock(now);
        large.want(1, KEY, KEY, "ES", &es(), NOW - 20 * DAY, SETTINGS, false);
        assert_eq!(next(&large), vec![1], "the last days' book");
        let started = Instant::now();
        let lower = Settings { regular: 5, extended: 5, combine: true };
        large.want(1, KEY, KEY, "ES", &es(), NOW - 20 * DAY, lower, false);
        let sink = asked.lock().unwrap().sinks.pop().expect("a stream");
        sink(&[Print::new((NOW + 10) * 1_000_000_000, 30_000, 700, Aggressor::Buy, false)]);
        let until = Instant::now() + Duration::from_secs(5);
        while large.book(KEY, lower).is_none_or(|(book, _)| !book.orders.iter().any(|o| o.order.size == 700)) {
            if let Ok(done) = large.done().try_recv() {
                large.finished(done);
            }
            assert!(Instant::now() < until, "held up behind the walk");
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(started.elapsed() < Duration::from_secs(2), "{:?}", started.elapsed());
        let walked = asked.lock().unwrap().spans.iter().filter(|(from, _)| *from < NOW - 2 * DAY).count();
        assert!(walked < 10, "the walk was still going: {walked} days");
        // And it goes on to the end, and the book has all of it then.
        let until = Instant::now() + Duration::from_secs(30);
        let oldest = |large: &LargeOrders| large.book(KEY, lower).and_then(|(book, _)| book.orders.first().map(|o| o.order.ts_nanos));
        while oldest(&large).is_none_or(|ts| ts >= (NOW - 18 * DAY) * 1_000_000_000) {
            if let Ok(done) = large.done().try_recv() {
                large.finished(done);
            }
            assert!(Instant::now() < until, "the walk never reached the book");
            std::thread::sleep(Duration::from_millis(20));
        }
        let (book, _) = large.book(KEY, lower).unwrap();
        assert!(book.orders.iter().any(|o| o.order.size == 700), "the live trade too");
    }

    /// A chart that lets go of a contract and wants it again before the
    /// thread has got to the forget — busy reading — still gets its live
    /// trades: its new stream's mailbox is never the one forgotten.
    #[test]
    fn a_chart_wanting_its_contract_again_at_once_still_hears_live_trades() {
        let asked = Arc::new(Mutex::new(Asked { slow: Duration::from_millis(300), slow_before: i64::MAX, ..Asked::default() }));
        let large = LargeOrders::new(Arc::new(Fake(asked.clone())), Some(scratch("again-live"))).unwrap();
        large.set_clock(now);
        large.want(1, KEY, KEY, "ES", &es(), NOW - DAY, SETTINGS, false);
        large.unwatch(1);
        large.want(1, KEY, KEY, "ES", &es(), NOW - DAY, SETTINGS, false);
        settle(&large);
        let sink = asked.lock().unwrap().sinks.pop().expect("the new stream");
        sink(&[Print::new((NOW + 10) * 1_000_000_000, 30_000, 900, Aggressor::Sell, false)]);
        let until = Instant::now() + Duration::from_secs(5);
        while !sizes(&large).contains(&900) {
            if let Ok(done) = large.done().try_recv() {
                large.finished(done);
            }
            assert!(Instant::now() < until, "the live trade never reached the book");
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}
