//! Volume at price for the charts of one feed: the ladders each contract's
//! charts share, loaded from the store as far as they need, filled from the
//! provider's history only where the store has a hole, and kept current
//! from its live feed.
//!
//! A feed has one of these only when its provider answers
//! [`Provider::ladders`], and a chart asks it for anything only when one of
//! its indicators uses volume at price. A chart of anything else holds no
//! ladder and costs nothing here.
//!
//! **What is loaded.** A chart's ladders start as the stored total of every
//! trading day its bars reach: a session profile over a month is answered
//! from those alone. The minutes of a day are read only when a period cuts
//! through it, and only for the newest [`MAX_WHOLE_DAYS`] such days; an
//! older one keeps the estimate.
//!
//! **What is asked.** On the first chart of a contract the store is asked
//! which minutes from the chart's first bar to now it knows nothing about —
//! [`Store::missing_ladders`] — and only those spans go to the provider, one
//! request each, which is what spares a provider with a data quota. A span
//! the store knows complete is never asked for again. A disconnect leaves a
//! hole, and the feed coming back fills it at once: the store is asked again
//! which minutes it is missing, and the provider's minutes replace the ones
//! built live. The minute the feed came back in was joined part way, and is
//! asked for once more by the first update after it has closed and
//! [`SETTLED`] seconds have passed.
//!
//! **Live.** Each contract with a chart watching has one live ladder feed.
//! Its updates land in a mailbox on the provider's thread, keeping only the
//! newest forming minute, and wake the main loop at most once per drain, as
//! a stream's ticks do. A minute that seals goes into memory and to the
//! store, as one small row, on this registry's own thread.
//!
//! Nothing here runs on a timer.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};

use omacharts_engine::ladders::{Day, Days, Held, Minute};
use omacharts_engine::{Instrument, LadderUpdate, Provider, Subscription};

use crate::store::Store;

/// The most days whose minutes one contract's charts hold at once. A day is
/// about a megabyte decoded; past this a period that cuts an older day is
/// drawn from the estimate rather than from more memory.
pub const MAX_WHOLE_DAYS: usize = 12;

/// How long after a minute closes the provider is taken to have it whole.
pub const SETTLED: i64 = 30;

/// What the registry's thread is asked to do, in order.
enum Job {
    /// Fill the store's holes in `from..now` from the provider, then read
    /// back what a chart holds from `from` on.
    Load { key: String, symbol: String, instrument: Instrument, tick_size: f64, from: i64, now: i64 },
    /// The feed is back: fill the store's holes in `from..now` from the
    /// provider, and read back whole the days that changed.
    Fill { key: String, symbol: String, instrument: Instrument, tick_size: f64, from: i64, now: i64 },
    /// One day's minutes.
    Day { key: String, day: i64 },
    /// A minute sealed live, and the span the feed knows complete.
    Append { key: String, tick_size: f64, day: i64, minute: Minute, covered: Option<(i64, i64)> },
}

/// What the thread hands back.
pub enum Done {
    Loaded { key: String, from: i64, held: Held, asked: Vec<(i64, i64)> },
    Day { key: String, day: Day },
    /// A fill after the feed came back: the days it changed, whole.
    Filled { key: String, days: Vec<Day>, asked: Vec<(i64, i64)> },
}

/// What the provider's thread leaves for the next drain: updates in order,
/// a run of forming minutes kept as only the newest.
#[derive(Default)]
struct Mailbox {
    updates: Vec<LadderUpdate>,
}

struct Entry {
    symbol: String,
    instrument: Instrument,
    days: Days,
    tick_size: f64,
    held: Held,
    loaded: bool,
    /// The earliest time a chart has asked for, loaded or on its way.
    from: i64,
    watchers: HashSet<u32>,
    mailbox: Arc<Mutex<Mailbox>>,
    /// Held to keep the live feed open; dropped, it ends.
    _live: Option<Box<dyn Subscription>>,
    /// Minutes that sealed before the load came back, put in after it.
    early: Vec<Minute>,
    /// Days whose minutes have been asked for.
    asked: HashSet<i64>,
    /// Fills on their way.
    filling: usize,
    /// Minutes sealed live while a fill was on its way, put back on top of
    /// the days it reads back.
    sealed_meanwhile: Vec<Minute>,
    /// When the minute the feed came back in will have settled: the first
    /// update from then on asks for it once more.
    refill_at: Option<i64>,
}

/// One feed's volume at price.
pub struct AtPrice {
    provider: Arc<dyn Provider>,
    entries: RefCell<HashMap<String, Entry>>,
    by_pane: RefCell<HashMap<u32, String>>,
    nudge: async_channel::Sender<()>,
    nudges: async_channel::Receiver<()>,
    armed: Arc<AtomicBool>,
    jobs: Mutex<Option<mpsc::Sender<Job>>>,
    done: async_channel::Receiver<Done>,
    /// Unix seconds now: the wall clock, or a test's.
    clock: std::cell::Cell<fn() -> i64>,
}

fn wall_clock() -> i64 {
    chrono::Utc::now().timestamp()
}

impl AtPrice {
    /// Volume at price for `provider`, which must have ladders, kept in the
    /// database at `store`. `None` for a database with no file — the tests'
    /// in-memory one — where nothing could be kept.
    pub fn new(provider: Arc<dyn Provider>, store: Option<PathBuf>) -> Option<AtPrice> {
        provider.ladders()?;
        let path = store?;
        let (nudge, nudges) = async_channel::bounded(1);
        let (finished, done) = async_channel::unbounded();
        let (jobs, inbox) = mpsc::channel::<Job>();
        let worker = provider.clone();
        std::thread::Builder::new()
            .name("omacharts ladders".into())
            .spawn(move || work(&path, worker.as_ref(), &inbox, &finished))
            .ok()?;
        Some(AtPrice {
            provider,
            entries: RefCell::new(HashMap::new()),
            by_pane: RefCell::new(HashMap::new()),
            nudge,
            nudges,
            armed: Arc::new(AtomicBool::new(false)),
            jobs: Mutex::new(Some(jobs)),
            done,
            clock: std::cell::Cell::new(wall_clock),
        })
    }

    /// Read the time from `clock` rather than the wall: for tests.
    pub fn set_clock(&self, clock: fn() -> i64) {
        self.clock.set(clock);
    }

    /// Wakes, one per pending drain.
    pub fn nudges(&self) -> async_channel::Receiver<()> {
        self.nudges.clone()
    }

    /// What the thread finished: a load or a day to put in, with
    /// [`Self::finished`].
    pub fn done(&self) -> async_channel::Receiver<Done> {
        self.done.clone()
    }

    /// Are there ladders of `instrument`, `symbol` to the provider? Offline.
    ///
    /// Two questions, asked of whoever answers each: whether the feed has
    /// ladders of the symbol, and whether the instrument has a tick for them
    /// to be counted in. A stock has no one tick, so a feed that served its
    /// trades would still have no ladders of it here.
    pub fn serves(&self, symbol: &str, instrument: &Instrument) -> bool {
        instrument.tick_size().is_some() && self.provider.ladders().is_some_and(|ladders| ladders.serves(symbol))
    }

    fn send(&self, job: Job) {
        if let Some(jobs) = self.jobs.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
            let _ = jobs.send(job);
        }
    }

    /// Chart `pane` shows `key` (`symbol` to the provider) from `from` on,
    /// and one of its indicators wants its ladders. The first chart of a
    /// contract loads it and opens its live feed; a chart reaching further
    /// back than anything loaded so far loads again from there.
    pub fn want(&self, pane: u32, key: &str, symbol: &str, instrument: &Instrument, from: i64) {
        let Some(tick_size) = instrument.tick_size().filter(|_| self.serves(symbol, instrument)) else {
            return;
        };
        let moved = self.by_pane.borrow().get(&pane).map(String::as_str) != Some(key);
        if moved {
            self.unwatch(pane);
            self.by_pane.borrow_mut().insert(pane, key.to_string());
        }
        let mut entries = self.entries.borrow_mut();
        if let Some(entry) = entries.get_mut(key) {
            entry.watchers.insert(pane);
            if from < entry.from {
                entry.from = from;
                drop(entries);
                self.send(Job::Load {
                    key: key.to_string(),
                    symbol: symbol.to_string(),
                    instrument: instrument.clone(),
                    tick_size,
                    from,
                    now: (self.clock.get())(),
                });
            }
            return;
        }
        let mailbox = Arc::new(Mutex::new(Mailbox::default()));
        let live = self.open(symbol, tick_size, &mailbox);
        entries.insert(
            key.to_string(),
            Entry {
                symbol: symbol.to_string(),
                instrument: instrument.clone(),
                days: Days::new(instrument),
                tick_size,
                held: Held::new(tick_size),
                loaded: false,
                from,
                watchers: HashSet::from([pane]),
                mailbox,
                _live: live,
                early: Vec::new(),
                asked: HashSet::new(),
                filling: 0,
                sealed_meanwhile: Vec::new(),
                refill_at: None,
            },
        );
        drop(entries);
        self.send(Job::Load {
            key: key.to_string(),
            symbol: symbol.to_string(),
            instrument: instrument.clone(),
            tick_size,
            from,
            now: (self.clock.get())(),
        });
    }

    fn open(&self, symbol: &str, tick_size: f64, mailbox: &Arc<Mutex<Mailbox>>) -> Option<Box<dyn Subscription>> {
        let ladders = self.provider.ladders()?;
        let into = mailbox.clone();
        let armed = self.armed.clone();
        let nudge = self.nudge.clone();
        let sink = Box::new(move |update: LadderUpdate| {
            let first = {
                let mut mailbox = into.lock().unwrap_or_else(|e| e.into_inner());
                let first = mailbox.updates.is_empty();
                match (mailbox.updates.last_mut(), update) {
                    (Some(LadderUpdate::Forming(waiting)), LadderUpdate::Forming(newer)) => *waiting = newer,
                    (_, update) => mailbox.updates.push(update),
                }
                first
            };
            if first && !armed.swap(true, Ordering::AcqRel) {
                let _ = nudge.try_send(());
            }
        });
        ladders.watch(symbol, tick_size, sink).ok()
    }

    /// Chart `pane` no longer wants ladders. The last chart of a contract to
    /// go closes its live feed and lets its ladders go.
    pub fn unwatch(&self, pane: u32) {
        let Some(key) = self.by_pane.borrow_mut().remove(&pane) else { return };
        let mut entries = self.entries.borrow_mut();
        let empty = entries.get_mut(&key).is_some_and(|entry| {
            entry.watchers.remove(&pane);
            entry.watchers.is_empty()
        });
        if empty {
            entries.remove(&key);
        }
    }

    /// Every chart let go: a chartbook is coming down, or the feed.
    pub fn unwatch_all(&self) {
        self.by_pane.borrow_mut().clear();
        self.entries.borrow_mut().clear();
    }

    /// The feed is retired: everything let go, and the thread finishes what
    /// it was given and ends.
    pub fn shutdown(&self) {
        self.unwatch_all();
        self.jobs.lock().unwrap_or_else(|e| e.into_inner()).take();
    }

    /// Read `key`'s ladders, if they are loaded.
    pub fn with_held<T>(&self, key: &str, read: impl FnOnce(Option<&Held>) -> T) -> T {
        let entries = self.entries.borrow();
        read(entries.get(key).filter(|entry| entry.loaded).map(|entry| &entry.held))
    }

    /// Ask for the minutes of the days the last computation of `key` found
    /// it could not answer without, newest first, up to [`MAX_WHOLE_DAYS`].
    pub fn load_wanted(&self, key: &str) {
        let mut asks = Vec::new();
        {
            let mut entries = self.entries.borrow_mut();
            let Some(entry) = entries.get_mut(key) else { return };
            let whole = entry.held.days.iter().filter(|d| d.whole).count();
            let room = MAX_WHOLE_DAYS.saturating_sub(whole.max(entry.asked.len()));
            let fresh: Vec<i64> =
                entry.held.take_wanted().into_iter().filter(|d| !entry.asked.contains(d)).take(room).collect();
            for day in fresh {
                entry.asked.insert(day);
                asks.push(day);
            }
        }
        for day in asks {
            self.send(Job::Day { key: key.to_string(), day });
        }
    }

    /// The thread's answer, put in place. The charts to compute again.
    pub fn finished(&self, done: Done) -> Vec<u32> {
        let mut entries = self.entries.borrow_mut();
        match done {
            Done::Loaded { key, from, mut held, asked } => {
                let Some(entry) = entries.get_mut(&key) else { return Vec::new() };
                if from > entry.from && entry.loaded {
                    // A later load from further back is on its way.
                    return Vec::new();
                }
                if !asked.is_empty() {
                    let spans: Vec<String> = asked.iter().map(|(a, b)| format!("{a}..{b}")).collect();
                    eprintln!("omacharts: ladders of {key}: asked the provider for {}", spans.join(", "));
                }
                held.forming = entry.held.forming.take();
                for minute in entry.early.drain(..) {
                    held.seal(entry.days.of(minute.ts), minute);
                }
                // Days whose minutes were loaded are kept whole.
                for day in entry.held.days.drain(..).filter(|d| d.whole) {
                    if held.days.iter().any(|h| h.day.day == day.day.day && !h.whole) {
                        held.load(day.day);
                    }
                }
                if held.tick_size <= 0.0 {
                    held.tick_size = entry.tick_size;
                }
                entry.held = held;
                entry.loaded = true;
                entry.watchers.iter().copied().collect()
            }
            Done::Day { key, day } => {
                let Some(entry) = entries.get_mut(&key) else { return Vec::new() };
                entry.held.load(day);
                entry.watchers.iter().copied().collect()
            }
            Done::Filled { key, days, asked } => {
                let Some(entry) = entries.get_mut(&key) else { return Vec::new() };
                entry.filling = entry.filling.saturating_sub(1);
                let meanwhile = if entry.filling == 0 {
                    std::mem::take(&mut entry.sealed_meanwhile)
                } else {
                    entry.sealed_meanwhile.clone()
                };
                if asked.is_empty() || !entry.loaded {
                    return Vec::new();
                }
                let spans: Vec<String> = asked.iter().map(|(a, b)| format!("{a}..{b}")).collect();
                eprintln!("omacharts: ladders of {key}: back after a drop, asked the provider for {}", spans.join(", "));
                crate::log_words::say(crate::log_words::ladders_back(&key, spans.len()));
                let numbers: Vec<i64> = days.iter().map(|day| day.day).collect();
                for day in days {
                    entry.held.load(day);
                }
                // What sealed after the fill read the store goes back onto
                // the days it replaced; the others hold those minutes already.
                for minute in meanwhile {
                    let day = entry.days.of(minute.ts);
                    if numbers.contains(&day) {
                        entry.held.seal(day, minute);
                    }
                }
                entry.watchers.iter().copied().collect()
            }
        }
    }

    /// Fold what the live feeds sent since the last drain into memory, and
    /// hand each sealed minute to the store. The charts to compute again.
    ///
    /// The feed coming back asks at once for the minutes the store is
    /// missing, behind the minutes sealed before it.
    pub fn drain(&self) -> Vec<u32> {
        self.armed.store(false, Ordering::Release);
        let mut touched = Vec::new();
        let mut appends = Vec::new();
        let mut fills = Vec::new();
        let now = (self.clock.get())();
        {
            let mut entries = self.entries.borrow_mut();
            for (key, entry) in entries.iter_mut() {
                let updates = std::mem::take(&mut entry.mailbox.lock().unwrap_or_else(|e| e.into_inner()).updates);
                if updates.is_empty() {
                    continue;
                }
                let mut fill = entry.refill_at.is_some_and(|at| now >= at);
                for update in updates {
                    let (minute, covered) = match update {
                        LadderUpdate::Forming(minute) => {
                            entry.held.forming = Some(minute);
                            continue;
                        }
                        LadderUpdate::Resumed => {
                            fill = true;
                            entry.refill_at = Some(now.div_euclid(60) * 60 + 60 + SETTLED);
                            continue;
                        }
                        LadderUpdate::Sealed { minute, covered } => (minute, covered),
                        // The partial minute is drawn, and stored as what it
                        // is: not complete, so the next start asks for it.
                        LadderUpdate::Dropped(Some(minute)) => (minute, None),
                        LadderUpdate::Dropped(None) => continue,
                    };
                    let day = entry.days.of(minute.ts);
                    appends.push(Job::Append {
                        key: key.clone(),
                        tick_size: entry.tick_size,
                        day,
                        minute: minute.clone(),
                        covered,
                    });
                    if entry.filling > 0 {
                        entry.sealed_meanwhile.push(minute.clone());
                    }
                    if entry.loaded {
                        entry.held.seal(day, minute);
                    } else {
                        entry.early.push(minute);
                    }
                }
                if fill {
                    if entry.refill_at.is_some_and(|at| now >= at) {
                        entry.refill_at = None;
                    }
                    entry.filling += 1;
                    fills.push(Job::Fill {
                        key: key.clone(),
                        symbol: entry.symbol.clone(),
                        instrument: entry.instrument.clone(),
                        tick_size: entry.tick_size,
                        from: entry.from,
                        now,
                    });
                }
                touched.extend(entry.watchers.iter().copied());
            }
        }
        for job in appends.into_iter().chain(fills) {
            self.send(job);
        }
        touched
    }

    /// The symbol a key is served as, for tests and reports.
    pub fn symbol_of(&self, key: &str) -> Option<String> {
        self.entries.borrow().get(key).map(|entry| entry.symbol.clone())
    }
}

/// Ask the provider for the minutes of `from..now` the store knows nothing
/// about, one request a hole, and keep them. The spans it got.
#[allow(clippy::too_many_arguments)]
fn fill(
    store: &Store,
    ladders: &dyn omacharts_engine::Ladders,
    key: &str,
    symbol: &str,
    tick_size: f64,
    days: &Days,
    from: i64,
    now: i64,
) -> Vec<(i64, i64)> {
    // The minute trading now is the live feed's.
    let to = now.div_euclid(60) * 60;
    let mut asked = Vec::new();
    for (a, b) in store.missing_ladders(key, from, to) {
        match ladders.minutes(symbol, tick_size, a, b) {
            Ok(minutes) => {
                if store.save_ladder_history(key, tick_size, days, a, b, &minutes).is_ok() {
                    asked.push((a, b));
                }
            }
            // Not recorded as known: asked again the next time.
            Err(error) => {
                eprintln!("omacharts: ladders of {key} for {a}..{b}: {error}");
                crate::log_words::say(crate::log_words::ladders_failed(key, &error));
            }
        }
    }
    asked
}

/// The registry's thread: one connection to the store, and the jobs in the
/// order they were given.
fn work(path: &std::path::Path, provider: &dyn Provider, inbox: &mpsc::Receiver<Job>, done: &async_channel::Sender<Done>) {
    let Ok(store) = Store::open_at(path) else { return };
    let Some(ladders) = provider.ladders() else { return };
    while let Ok(job) = inbox.recv() {
        let answer = match job {
            Job::Load { key, symbol, instrument, tick_size, from, now } => {
                let days = Days::new(&instrument);
                let from = from.max(now - i64::from(ladders.history_days()) * 86_400);
                let _ = store.compact_ladders(&key, days.of(now));
                let asked = fill(&store, ladders, &key, &symbol, tick_size, &days, from, now);
                let held = store.load_ladder_held(&key, &days, days.of(from), days.of(now) + 1);
                Done::Loaded { key, from, held, asked }
            }
            Job::Fill { key, symbol, instrument, tick_size, from, now } => {
                let days = Days::new(&instrument);
                let from = from.max(now - i64::from(ladders.history_days()) * 86_400);
                let asked = fill(&store, ladders, &key, &symbol, tick_size, &days, from, now);
                let mut numbers: Vec<i64> = asked.iter().flat_map(|(a, b)| days.of(*a)..=days.of(b - 1)).collect();
                numbers.sort_unstable();
                numbers.dedup();
                let changed = numbers.into_iter().filter_map(|day| store.load_ladder_day(&key, day)).collect();
                Done::Filled { key, days: changed, asked }
            }
            Job::Day { key, day } => match store.load_ladder_day(&key, day) {
                Some(day) => Done::Day { key, day },
                None => continue,
            },
            Job::Append { key, tick_size, day, minute, covered } => {
                let _ = store.append_ladder_minute(&key, tick_size, day, &minute, covered);
                continue;
            }
        };
        if done.send_blocking(answer).is_err() {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use omacharts_engine::ladders::Side;
    use omacharts_engine::{Bar, Capability, InstrumentKind, LadderSink, Ladders, ProviderError, Timeframe};

    use super::*;

    const KEY: &str = "fake:ES";
    /// A whole minute, 2026-11-03 03:20 UTC: an ES session in progress.
    const NOW: i64 = 1_793_700_000;
    const HOUR: i64 = 3_600;

    fn now() -> i64 {
        NOW
    }

    fn later() -> i64 {
        NOW + 180
    }

    #[derive(Default)]
    struct Asked {
        spans: Vec<(i64, i64)>,
        sinks: Vec<LadderSink>,
    }

    struct Fake(Arc<Mutex<Asked>>);

    struct Open;
    impl Subscription for Open {}

    fn minute(ts: i64) -> Minute {
        let mut minute = Minute::opening(ts, 7_625.0);
        minute.add_trade(30_500, 7_625.0, 3, Side::Buy, 1);
        minute.add_trade(30_499, 7_624.75, 2, Side::Sell, 1);
        minute
    }

    /// A feed with ladders of anything it is asked about, so whether a
    /// chart gets them is the instrument's to say.
    impl Ladders for Fake {
        fn serves(&self, _: &str) -> bool {
            true
        }
        fn history_days(&self) -> u32 {
            30
        }
        fn minutes(&self, _: &str, _: f64, from: i64, to: i64) -> Result<Vec<Minute>, ProviderError> {
            self.0.lock().unwrap().spans.push((from, to));
            Ok((from..to).step_by(60).map(minute).collect())
        }
        fn watch(&self, _: &str, _: f64, sink: LadderSink) -> Result<Box<dyn Subscription>, ProviderError> {
            self.0.lock().unwrap().sinks.push(sink);
            Ok(Box::new(Open))
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
        fn symbol_for(&self, instrument: &Instrument) -> Option<String> {
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
        }
    }

    fn aapl() -> Instrument {
        Instrument { symbol: "AAPL".into(), name: "Apple".into(), kind: InstrumentKind::Equity, session_origin: 0, ..es() }
    }

    /// The feed says it has ladders of both; only the root has a tick to
    /// count them in, so only the root's chart gets them, and the stock's
    /// asks the feed for nothing.
    #[test]
    fn ladders_are_served_for_a_root_with_a_tick_and_not_for_a_stock() {
        let path = scratch("serves");
        let asked = Arc::new(Mutex::new(Asked::default()));
        let at_price = AtPrice::new(Arc::new(Fake(asked.clone())), Some(path.clone())).unwrap();
        at_price.set_clock(now);
        assert!(at_price.serves("ES", &es()));
        assert!(!at_price.serves("AAPL", &aapl()));

        at_price.want(1, "fake:AAPL", "AAPL", &aapl(), NOW - HOUR);
        at_price.want(2, KEY, "ES", &es(), NOW - HOUR);
        assert_eq!(settle(&at_price), vec![2], "the root's load, and nothing before it");
        assert_eq!(asked.lock().unwrap().spans, vec![(NOW - HOUR, NOW)]);
        assert_eq!(asked.lock().unwrap().sinks.len(), 1, "one live feed, the root's");
        assert!(at_price.with_held("fake:AAPL", |held| held.is_none()));
        assert_eq!(at_price.with_held(KEY, |held| held.unwrap().tick_size), 0.25, "the contract's tick");
        at_price.shutdown();
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("omacharts-at-price-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("omacharts.db")
    }

    fn settle(at_price: &AtPrice) -> Vec<u32> {
        let done = at_price.done().recv_blocking().expect("the thread answers");
        at_price.finished(done)
    }

    /// The store knows two hours of the last three; a chart reaching back
    /// three hours asks the provider for the one hour between, once. Live
    /// minutes are stored as they seal, so the next start asks for nothing
    /// but the minutes since, and draws the live one from the store.
    #[test]
    fn only_what_the_store_does_not_know_is_asked_for_and_live_minutes_persist() {
        let path = scratch("missing");
        let days = Days::new(&es());
        {
            let store = Store::open_at(&path).unwrap();
            for (a, b) in [(NOW - 3 * HOUR, NOW - 2 * HOUR), (NOW - HOUR, NOW)] {
                let minutes: Vec<Minute> = (a..b).step_by(60).map(minute).collect();
                store.save_ladder_history(KEY, 0.25, &days, a, b, &minutes).unwrap();
            }
        }
        let asked = Arc::new(Mutex::new(Asked::default()));
        let provider: Arc<dyn Provider> = Arc::new(Fake(asked.clone()));

        let first = AtPrice::new(provider.clone(), Some(path.clone())).expect("a feed with ladders");
        first.set_clock(now);
        first.want(1, KEY, "ES", &es(), NOW - 3 * HOUR);
        assert_eq!(settle(&first), vec![1]);
        assert_eq!(asked.lock().unwrap().spans, vec![(NOW - 2 * HOUR, NOW - HOUR)]);
        let whole = first.with_held(KEY, |held| held.unwrap().between(NOW - 3 * HOUR, NOW).unwrap().sides.volume());
        assert_eq!(whole, 180 * 5, "three hours of minutes, five contracts each");

        // A second chart of the same contract shares it and asks nothing.
        first.want(2, KEY, "ES", &es(), NOW - 3 * HOUR);
        assert_eq!(asked.lock().unwrap().spans.len(), 1);
        assert_eq!(asked.lock().unwrap().sinks.len(), 1, "one live feed per contract");

        // Live: the minute at NOW seals, complete, and the next is forming.
        {
            let asked = asked.lock().unwrap();
            let sink = &asked.sinks[0];
            sink(LadderUpdate::Forming(minute(NOW)));
            sink(LadderUpdate::Sealed { minute: minute(NOW), covered: Some((NOW, NOW + 60)) });
            sink(LadderUpdate::Forming(minute(NOW + 60)));
        }
        let mut touched = first.drain();
        touched.sort_unstable();
        assert_eq!(touched, vec![1, 2]);
        let live = first.with_held(KEY, |held| held.unwrap().between(NOW - 3 * HOUR, NOW + 120).unwrap().sides.volume());
        assert_eq!(live, 180 * 5 + 10, "the sealed minute and the forming one on top");
        // A load of another contract queued behind the append proves it written.
        first.want(3, "fake:NQ", "NQ", &es(), NOW - 60);
        settle(&first);
        first.shutdown();
        asked.lock().unwrap().spans.clear();

        // The next start: nothing it knows is asked for again.
        let second = AtPrice::new(provider, Some(path.clone())).unwrap();
        second.set_clock(later);
        second.want(1, KEY, "ES", &es(), NOW - 3 * HOUR);
        settle(&second);
        assert_eq!(asked.lock().unwrap().spans, vec![(NOW + 60, NOW + 180)], "only the minutes since");
        let back = second.with_held(KEY, |held| held.unwrap().between(NOW - 3 * HOUR, NOW + 180).unwrap().sides.volume());
        // The live minute read back from the store, and the two since.
        assert_eq!(back, 180 * 5 + 5 + 2 * 5);
        second.shutdown();
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    static CLOCK: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(NOW);

    fn clock() -> i64 {
        CLOCK.load(Ordering::SeqCst)
    }

    /// A minute the feed saw only part of: one contract, where the
    /// provider's has five.
    fn partial(ts: i64) -> Minute {
        let mut minute = Minute::opening(ts, 7_625.0);
        minute.add_trade(30_500, 7_625.0, 1, Side::Buy, 1);
        minute
    }

    /// Wait for everything given to the thread so far: a load that asks
    /// for nothing, of a contract nothing else touches, queued behind it.
    fn sync(at_price: &AtPrice, pane: u32) {
        at_price.want(pane, &format!("fake:SYNC{pane}"), "ES", &es(), clock().div_euclid(60) * 60);
        assert!(settle(at_price).contains(&pane), "the sync's own load, after anything queued before it");
    }

    /// The feed drops and comes back four minutes later: the gap is asked
    /// for at once, only the gap, and once; the chart's ladders are whole
    /// again with the provider's minutes in place of the partial one. The
    /// minute the feed came back in is asked for once more after it has
    /// settled, and nothing after that.
    #[test]
    fn coming_back_after_a_drop_fills_the_gap_at_once_and_only_the_gap() {
        let path = scratch("reconnect");
        let asked = Arc::new(Mutex::new(Asked::default()));
        let provider: Arc<dyn Provider> = Arc::new(Fake(asked.clone()));
        let at_price = AtPrice::new(provider, Some(path.clone())).unwrap();
        CLOCK.store(NOW, Ordering::SeqCst);
        at_price.set_clock(clock);
        at_price.want(1, KEY, "ES", &es(), NOW - HOUR);
        settle(&at_price);
        assert_eq!(asked.lock().unwrap().spans, vec![(NOW - HOUR, NOW)]);
        let say = |update: LadderUpdate| (asked.lock().unwrap().sinks[0])(update);

        // A minute whole, and the drop part way through the next.
        say(LadderUpdate::Sealed { minute: minute(NOW), covered: Some((NOW, NOW + 60)) });
        say(LadderUpdate::Forming(partial(NOW + 60)));
        CLOCK.store(NOW + 90, Ordering::SeqCst);
        say(LadderUpdate::Dropped(Some(partial(NOW + 60))));
        at_price.drain();
        sync(&at_price, 7);
        assert_eq!(asked.lock().unwrap().spans.len(), 1, "a drop alone asks for nothing");
        let volume = |to: i64| {
            at_price.with_held(KEY, |held| held.unwrap().between(NOW - HOUR, to).unwrap().sides.volume())
        };
        assert_eq!(volume(NOW + 300), 60 * 5 + 5 + 1, "the gap, before it is filled");

        // Back, ten seconds into a minute.
        CLOCK.store(NOW + 310, Ordering::SeqCst);
        say(LadderUpdate::Resumed);
        assert!(at_price.drain().contains(&1));
        assert_eq!(settle(&at_price), vec![1], "the fill comes back to the chart");
        assert_eq!(asked.lock().unwrap().spans[1..], [(NOW + 60, NOW + 300)], "only the gap");
        assert_eq!(volume(NOW + 300), 60 * 5 + 5 + 4 * 5, "whole again, the provider's minute in place of the partial");

        // More from the feed before the minute it came back in has settled.
        say(LadderUpdate::Forming(partial(NOW + 300)));
        CLOCK.store(NOW + 320, Ordering::SeqCst);
        at_price.drain();
        sync(&at_price, 8);
        assert_eq!(asked.lock().unwrap().spans.len(), 2, "nothing asked twice");

        // That minute sealed, joined part way, and settled: asked once more.
        CLOCK.store(NOW + 400, Ordering::SeqCst);
        say(LadderUpdate::Sealed { minute: partial(NOW + 300), covered: None });
        say(LadderUpdate::Forming(partial(NOW + 360)));
        at_price.drain();
        assert_eq!(settle(&at_price), vec![1]);
        assert_eq!(asked.lock().unwrap().spans[2..], [(NOW + 300, NOW + 360)]);
        assert_eq!(volume(NOW + 360), 60 * 5 + 5 + 5 * 5);

        CLOCK.store(NOW + 500, Ordering::SeqCst);
        say(LadderUpdate::Forming(partial(NOW + 360)));
        at_price.drain();
        sync(&at_price, 9);
        assert_eq!(asked.lock().unwrap().spans.len(), 3, "and nothing more");
        at_price.shutdown();
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    /// A provider without ladders gets no registry at all: no thread, no
    /// query, nothing to ask.
    #[test]
    fn a_feed_without_ladders_has_none() {
        struct Plain;
        impl Provider for Plain {
            fn id(&self) -> &'static str {
                "plain"
            }
            fn label(&self) -> &'static str {
                "Plain"
            }
            fn delay_minutes(&self, _: InstrumentKind) -> u32 {
                0
            }
            fn symbol_for(&self, instrument: &Instrument) -> Option<String> {
                Some(instrument.symbol.clone())
            }
            fn capabilities(&self) -> &'static [Capability] {
                &[]
            }
            fn bars(&self, _: &str, _: Timeframe, _: Option<i64>) -> Result<Vec<Bar>, ProviderError> {
                Ok(Vec::new())
            }
        }
        assert!(AtPrice::new(Arc::new(Plain), Some(scratch("plain"))).is_none());
        let yahoo: Arc<dyn Provider> = Arc::new(omacharts_engine::providers::Yahoo::new());
        assert!(yahoo.ladders().is_none());
        assert!(AtPrice::new(yahoo, Some(scratch("yahoo"))).is_none());
    }
}
