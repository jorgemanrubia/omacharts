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
//! it as they come, and a new book is handed over each drain.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, HashMap};
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
}

/// What the thread hands back: a book, whole.
pub struct Done {
    pub key: String,
    pub settings: Settings,
    pub book: Arc<Book>,
}

/// One contract, on the main thread.
struct Entry {
    watchers: HashMap<u32, Settings>,
    from: i64,
    /// Asked for, at least once.
    asked: bool,
    books: HashMap<Settings, Arc<Book>>,
    tick_size: f64,
    live: Option<Box<dyn Subscription>>,
}

pub struct LargeOrders {
    provider: Arc<dyn Provider>,
    entries: RefCell<HashMap<String, Entry>>,
    by_pane: RefCell<HashMap<u32, String>>,
    jobs: Mutex<Option<mpsc::Sender<Job>>>,
    done: async_channel::Receiver<Done>,
    sender: mpsc::Sender<Job>,
    mailboxes: Mailboxes,
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
        std::thread::Builder::new()
            .name("omacharts large orders".into())
            .spawn(move || work(&path, worker.as_ref(), &inbox, &finished, &shared))
            .ok()?;
        Some(LargeOrders {
            mailboxes,
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
            watchers: HashMap::new(),
            from,
            asked: false,
            books: HashMap::new(),
            tick_size,
            live: None,
        });
        let new_settings = !entry.watchers.values().any(|had| *had == settings) && !entry.books.contains_key(&settings);
        entry.watchers.insert(pane, settings);
        if waiting {
            return;
        }
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
                let mut all: Vec<Settings> = entry.watchers.values().copied().collect();
                all.sort_by_key(|s| (s.regular, s.extended, s.combine));
                all.dedup();
                all
            };
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
        let key = key.to_string();
        let sink: omacharts_engine::PrintSink = Box::new(move |prints: &[Print]| {
            mailbox.prints.lock().unwrap_or_else(|e| e.into_inner()).extend_from_slice(prints);
            // One wake per batch the thread has not drained yet: it disarms
            // as it takes them.
            if !mailbox.armed.swap(true, Ordering::AcqRel) {
                let _ = wake.send(Job::Live { key: key.clone() });
            }
        });
        ladders.watch_prints(symbol, tick_size, sink).ok().flatten()
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
        entry.books.insert(done.settings, done.book);
        entry.watchers.iter().filter(|(_, settings)| **settings == done.settings).map(|(pane, _)| *pane).collect()
    }

    pub fn unwatch(&self, pane: u32) {
        let Some(key) = self.by_pane.borrow_mut().remove(&pane) else { return };
        let mut entries = self.entries.borrow_mut();
        let empty = entries.get_mut(&key).is_some_and(|entry| {
            entry.watchers.remove(&pane);
            entry.books.retain(|settings, _| entry.watchers.values().any(|s| s == settings));
            entry.watchers.is_empty()
        });
        if empty {
            entries.remove(&key);
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
                    if let Some(kept) = store.load_print_day(&self.stored, day, self.tick_size) {
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
) {
    let Ok(store) = Store::open_at(path) else { return };
    let Some(ladders) = provider.ladders() else { return };
    let mut held: HashMap<String, Held> = HashMap::new();
    let publish = |key: &str, held: &Held, settings: Settings, builder: &Builder| {
        let book = builder.book(&mut held.hours());
        let _ = done.send_blocking(Done { key: key.to_string(), settings, book: Arc::new(book) });
    };
    let rebuild = |key: &str, contract: &mut Held, settings: &[Settings]| {
        contract.builders.clear();
        for settings in settings {
            let builder = contract.build(&store, *settings);
            publish(key, contract, *settings, &builder);
            contract.builders.insert(*settings, builder);
        }
    };
    while let Ok(job) = inbox.recv() {
        match job {
            Job::Load { key, stored, symbol, instrument, tick_size, from, now, settings } => {
                let days = Days::new(&instrument);
                let contract = held.remove(&key).filter(|h| h.stored == stored && h.tick_size == tick_size);
                let mut contract = contract.unwrap_or_else(|| Held {
                    stored: stored.clone(),
                    symbol: symbol.clone(),
                    instrument: instrument.clone(),
                    tick_size,
                    days: days.clone(),
                    whole: BTreeSet::new(),
                    open: BTreeMap::new(),
                    dirty: BTreeSet::new(),
                    saved_at: Instant::now(),
                    builders: HashMap::new(),
                });
                // The last days first, and a book of them straight away; then
                // back to the chart's first bar, and the book again.
                let near = days.start(days.of(now) - (FIRST_DAYS - 1)).max(from);
                let walked = read_days(&store, ladders, &mut contract, near, now);
                let fresh = mailboxes.take(&key);
                append(&mut contract, fresh);
                rebuild(&key, &mut contract, &settings);
                if walked && from < near {
                    read_days(&store, ladders, &mut contract, from, days.start(days.of(near)) - 1);
                    let fresh = mailboxes.take(&key);
                    append(&mut contract, fresh);
                    rebuild(&key, &mut contract, &settings);
                }
                contract.save(&store, true);
                held.insert(key, contract);
            }
            Job::Build { key, settings } => {
                let Some(contract) = held.get_mut(&key) else { continue };
                let builder = contract.build(&store, settings);
                publish(&key, contract, settings, &builder);
                contract.builders.insert(settings, builder);
            }
            Job::Refresh { key, now } => {
                let Some(contract) = held.get_mut(&key) else { continue };
                // Only whole minutes, settled: what the feed has of them is
                // what it will always have.
                let to = (now - SETTLED).div_euclid(60) * 60;
                let since = contract.newest_upto().div_euclid(1_000_000_000).max(to - 86_400);
                if to <= since {
                    continue;
                }
                let Ok(fresh) = ladders.prints(&contract.symbol, contract.tick_size, since, to) else { continue };
                let added = append(contract, fresh);
                // Known to here now, whatever came.
                let day = contract.days.of(to - 1);
                if let Some(open) = contract.open.get_mut(&day) {
                    open.upto = open.upto.max(to * 1_000_000_000);
                    contract.dirty.insert(day);
                }
                if !added.is_empty() {
                    feed_builders(contract, &added);
                    for (settings, builder) in &contract.builders {
                        publish(&key, contract, *settings, builder);
                    }
                }
                contract.save(&store, false);
            }
            Job::Live { key } => {
                // Every wake that queued up behind this one is this drain's.
                let fresh = mailboxes.take(&key);
                let Some(contract) = held.get_mut(&key) else { continue };
                let added = append(contract, fresh);
                if !added.is_empty() {
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
                mailboxes.forget(&key);
            }
        }
    }
    // The window is closing: what the live feeds brought is kept.
    for contract in held.values_mut() {
        contract.save(&store, true);
    }
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

/// Read `contract`'s days from `from` to `now`, newest first: a day the
/// store has whole is only noted; any other is asked of the provider from
/// where it is known to. Stops at the first weekday the provider has
/// nothing of, answering false: there is nothing further back.
fn read_days(store: &Store, ladders: &dyn omacharts_engine::Ladders, contract: &mut Held, from: i64, now: i64) -> bool {
    let days = contract.days.clone();
    let (first, last) = (days.of(from), days.of(now));
    let settled = (now - SETTLED).div_euclid(60) * 60;
    let mut day = last;
    while day >= first && last - day <= DEEPEST {
        let (start, end) = (days.start(day), days.start(day + 1));
        if contract.whole.contains(&day) || store.print_day_complete(&contract.stored, day, contract.tick_size) {
            contract.whole.insert(day);
            day -= 1;
            continue;
        }
        let mut open = contract
            .open
            .remove(&day)
            .or_else(|| store.load_print_day(&contract.stored, day, contract.tick_size))
            .unwrap_or(PrintDay { tick_size: contract.tick_size, upto: i64::MIN, complete: false, prints: Vec::new() });
        let since = open.upto.div_euclid(1_000_000_000).max(start);
        let to = end.min(settled);
        if since < to {
            let Ok(fresh) = ladders.prints(&contract.symbol, contract.tick_size, since, to) else { return false };
            let floor = open.upto;
            open.prints.extend(fresh.into_iter().filter(|p| p.ts_nanos >= floor && p.ts_nanos < to * 1_000_000_000));
            open.prints.sort_by_key(|p| p.ts_nanos);
            open.upto = open.upto.max(to * 1_000_000_000);
        }
        open.complete = end <= settled;
        let empty = open.prints.is_empty();
        // A weekday with nothing, before today, is past what the feed keeps:
        // not written down, so a feed that later keeps more is asked again.
        if empty && open.complete && is_weekday(start) {
            return false;
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
    true
}

/// Does a trading day beginning at `start` fall on a weekday? A day that
/// begins on a Sunday evening is Monday's.
fn is_weekday(start: i64) -> bool {
    use chrono::Datelike;
    let Some(begins) = chrono::DateTime::from_timestamp(start + 12 * 3_600, 0) else { return true };
    !matches!(begins.weekday(), chrono::Weekday::Sat | chrono::Weekday::Sun)
}

#[cfg(test)]
mod tests {
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
    struct Asked {
        spans: Vec<(i64, i64)>,
        sinks: Vec<PrintSink>,
        /// The feed has nothing from before here.
        depth: i64,
    }

    struct Fake(Arc<Mutex<Asked>>);

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
            Ok(Vec::new())
        }
        fn watch(&self, _: &str, _: f64, _: LadderSink) -> Result<Box<dyn Subscription>, ProviderError> {
            Ok(Box::new(Open))
        }
        fn has_prints(&self, _: &str) -> bool {
            true
        }
        fn prints(&self, _: &str, _: f64, from: i64, to: i64) -> Result<Vec<Print>, ProviderError> {
            let mut asked = self.0.lock().unwrap();
            asked.spans.push((from, to));
            Ok(hourly(from.max(asked.depth), to))
        }
        fn watch_prints(&self, _: &str, _: f64, sink: PrintSink) -> Result<Option<Box<dyn Subscription>>, ProviderError> {
            self.0.lock().unwrap().sinks.push(sink);
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

    fn es() -> Instrument {
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

    fn scratch(name: &str) -> PathBuf {
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

    #[test]
    fn a_walk_back_stops_at_the_first_weekday_the_feed_has_nothing_of() {
        let asked = Arc::new(Mutex::new(Asked { depth: NOW - 3 * DAY, ..Asked::default() }));
        let large = LargeOrders::new(Arc::new(Fake(asked.clone())), Some(scratch("depth"))).unwrap();
        large.set_clock(now);
        large.want(1, KEY, KEY, "ES", &es(), NOW - 60 * DAY, SETTINGS, false);
        settle(&large);
        let spans = asked.lock().unwrap().spans.clone();
        assert!(spans.len() <= 6, "stopped near the feed's depth, not sixty days back: {}", spans.len());
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
}
