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
//! from those alone. A period that cuts through a day — a regular session
//! cuts every one — has that part of the day read on this registry's thread,
//! summed there, and held as one ladder, a piece, rather than the day's
//! minutes: every period a chart shows is drawn from what traded, however
//! far back, in a few kilobytes a period. Pieces are let go only past
//! [`PIECE_BYTES`], and never one the charts just drew from.
//!
//! **Which contract.** A root served as its front month is kept under the
//! month it is read from, so the ladders of the month before a roll are
//! never drawn under the month after's bars. Each time a chart asks, the
//! month is looked up again: a roll while it is charted reads the new month
//! afresh, and the month before is let go, from memory and from the store.
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
//! store, as one small row, on this registry's own thread. It goes into
//! the pieces of its day it falls in, and into any load, piece or flow of
//! that day on its way back from the store, which was read before the
//! minute was written: a period running on past the newest minute keeps one
//! piece of today for the whole session rather than reading the day every
//! minute. A piece or a flow the store turns out not to have is asked for
//! again later, each time a while longer after.
//!
//! Nothing here runs on a timer.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};

use omacharts_engine::continuous::{Adjustment, Spelled};
use omacharts_engine::ladders::{Day, Days, Held, Ladder, Minute, Piece};
use omacharts_engine::{Instrument, LadderUpdate, Provider, Subscription};

use crate::store::Store;

/// The most one contract's pieces hold, in bytes. A session's piece is some
/// ten kilobytes, so this is thousands of periods; past it pieces no chart
/// drew from last go first, and a period past what that frees keeps the
/// estimate rather than taking more memory.
pub const PIECE_BYTES: usize = 64 << 20;

/// How long after a minute closes the provider is taken to have it whole.
pub const SETTLED: i64 = 30;

/// How long a piece or a day's flow the store did not have waits before it
/// is asked for again, the `tries`th time: a minute, doubling, at most an
/// hour.
fn retry_after(tries: u32) -> i64 {
    (60_i64 << tries.saturating_sub(1).min(6)).min(3_600)
}

/// What the registry's thread is asked to do, in order.
///
/// `key` is the charts' name for a contract, `stored` the store's: the
/// contract it is read from, which for a root is the month it rolled to.
enum Job {
    /// Fill the store's holes in `from..now` from the provider, then read
    /// back what a chart holds from `from` on.
    /// `seq` counts the entry's loads, so that its answer knows which
    /// minutes sealed after it was asked.
    Load { key: String, stored: String, symbol: String, instrument: Instrument, tick_size: f64, from: i64, now: i64, seq: u64 },
    /// The feed is back: fill the store's holes in `from..now` from the
    /// provider, and read back whole the days that changed.
    Fill { key: String, stored: String, symbol: String, instrument: Instrument, tick_size: f64, from: i64, now: i64 },
    /// The parts of one day that spans cut, each summed.
    Pieces { key: String, stored: String, day: i64, pieces: Vec<Piece> },
    /// The flow of each of these days: every minute's ask volume less its
    /// bid volume, for a cumulative delta.
    Flows { key: String, stored: String, days: Vec<i64> },
    /// A minute sealed live, and the span the feed knows complete.
    Append { stored: String, tick_size: f64, day: i64, minute: Minute, covered: Option<(i64, i64)> },
}

/// What the thread hands back.
pub enum Done {
    Loaded { key: String, stored: String, from: i64, held: Box<Held>, asked: Vec<(i64, i64)>, seq: u64 },
    /// Pieces of one day, summed; `missing`, those of a day the store does
    /// not have.
    Pieces { key: String, stored: String, pieces: Vec<(Piece, Ladder)>, missing: Vec<Piece> },
    /// A fill after the feed came back: the days it changed, whole.
    Filled { key: String, stored: String, days: Vec<Day>, asked: Vec<(i64, i64)> },
    /// Days' flows; `missing`, the days the store does not have.
    Flows { key: String, stored: String, flows: Vec<(i64, Vec<(i64, i64)>)>, missing: Vec<i64> },
}

/// What the provider's thread leaves for the next drain: updates in order,
/// a run of forming minutes kept as only the newest.
#[derive(Default)]
struct Mailbox {
    updates: Vec<LadderUpdate>,
}

struct Entry {
    symbol: String,
    /// What the store keeps it under: see [`Job`].
    stored: String,
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
    /// Minutes that sealed while a load was on its way, each with how many
    /// loads had been asked for by then: put in after the loads asked
    /// before them, which read the store before they were written to it.
    early: Vec<(u64, Minute)>,
    /// Loads asked for, and the last of them answered.
    loads: u64,
    answered: u64,
    /// Pieces and days' flows the store did not have when asked: when each
    /// may be asked for again, and how many times it has been.
    retry: HashMap<Piece, (i64, u32)>,
    retry_flows: HashMap<i64, (i64, u32)>,
    /// Pieces asked for and not back yet, each with what traded in the
    /// minutes sealed into it since: the thread reads the store before it
    /// writes those, so they go on top of the piece when it comes back.
    asked: HashMap<Piece, Vec<Ladder>>,
    /// Days whose flows were asked for and are not back yet, each with the
    /// minutes sealed into it since, as the flow holds them.
    asked_flows: HashMap<i64, Vec<(i64, i64)>>,
    /// The pieces held, in the order they came.
    pieces: VecDeque<Piece>,
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
        // A root rolls while it is charted: the month it is read from now is
        // kept apart, read afresh, and the month before let go.
        let stored = stored_key(key, symbol, self.provider.ladders().and_then(|ladders| ladders.contract(symbol)));
        let mut entries = self.entries.borrow_mut();
        let (mut watchers, mut from) = (HashSet::from([pane]), from);
        if entries.get(key).is_some_and(|entry| entry.stored != stored)
            && let Some(rolled) = entries.remove(key)
        {
            watchers.extend(rolled.watchers);
            from = from.min(rolled.from);
        }
        if let Some(entry) = entries.get_mut(key) {
            entry.watchers.insert(pane);
            if from < entry.from {
                entry.from = from;
                entry.loads += 1;
                let (stored, seq) = (entry.stored.clone(), entry.loads);
                drop(entries);
                self.send(Job::Load {
                    key: key.to_string(),
                    stored,
                    symbol: symbol.to_string(),
                    instrument: instrument.clone(),
                    tick_size,
                    from,
                    now: (self.clock.get())(),
                    seq,
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
                stored: stored.clone(),
                instrument: instrument.clone(),
                days: Days::new(instrument),
                tick_size,
                held: Held::new(tick_size),
                loaded: false,
                from,
                watchers,
                mailbox,
                _live: live,
                early: Vec::new(),
                loads: 1,
                answered: 0,
                retry: HashMap::new(),
                retry_flows: HashMap::new(),
                asked: HashMap::new(),
                asked_flows: HashMap::new(),
                pieces: VecDeque::new(),
                filling: 0,
                sealed_meanwhile: Vec::new(),
                refill_at: None,
            },
        );
        drop(entries);
        self.send(Job::Load {
            key: key.to_string(),
            stored,
            symbol: symbol.to_string(),
            instrument: instrument.clone(),
            tick_size,
            from,
            now: (self.clock.get())(),
            seq: 1,
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

    /// Ask for the pieces the last computation of `key` found it could not
    /// answer without, a job a day, newest first; first making room for
    /// them, if the pieces are over [`PIECE_BYTES`], by letting go of the
    /// oldest that computation did not draw from.
    pub fn load_wanted(&self, key: &str) {
        self.load_wanted_flows(key);
        let mut asks: Vec<(i64, Vec<Piece>)> = Vec::new();
        let stored = {
            let mut entries = self.entries.borrow_mut();
            let Some(entry) = entries.get_mut(key) else { return };
            let used = entry.held.take_used();
            let wanted = entry.held.take_wanted();
            if wanted.is_empty() {
                return;
            }
            entry.make_room(&used);
            if entry.held.piece_bytes() >= PIECE_BYTES {
                // Everything held was just drawn from: the rest keep the
                // estimate rather than push it out.
                return;
            }
            let now = (self.clock.get())();
            for piece in wanted {
                // Wanted by a computation before it came back, or not had
                // by the store a moment ago.
                if entry.held.pieces.contains_key(&piece)
                    || entry.asked.contains_key(&piece)
                    || entry.retry.get(&piece).is_some_and(|(at, _)| now < *at)
                {
                    continue;
                }
                entry.asked.insert(piece, Vec::new());
                match asks.last_mut() {
                    Some((day, pieces)) if *day == piece.day => pieces.push(piece),
                    _ => asks.push((piece.day, vec![piece])),
                }
            }
            entry.stored.clone()
        };
        for (day, pieces) in asks {
            self.send(Job::Pieces { key: key.to_string(), stored: stored.clone(), day, pieces });
        }
    }

    /// Ask for the flows of the days the last computation of `key` reached
    /// for and found held by their totals alone, in one job, newest first.
    /// A flow is some twenty kilobytes a day, so nothing is let go for them.
    fn load_wanted_flows(&self, key: &str) {
        let (stored, days) = {
            let mut entries = self.entries.borrow_mut();
            let Some(entry) = entries.get_mut(key) else { return };
            let wanted = entry.held.take_wanted_flows();
            let now = (self.clock.get())();
            let days: Vec<i64> = wanted
                .into_iter()
                .filter(|day| !entry.held.flows.contains_key(day) && !entry.asked_flows.contains_key(day))
                .filter(|day| entry.retry_flows.get(day).is_none_or(|(at, _)| now >= *at))
                .collect();
            for day in &days {
                entry.asked_flows.insert(*day, Vec::new());
            }
            if days.is_empty() {
                return;
            }
            (entry.stored.clone(), days)
        };
        self.send(Job::Flows { key: key.to_string(), stored, days });
    }

    /// The thread's answer, put in place. The charts to compute again.
    pub fn finished(&self, done: Done) -> Vec<u32> {
        let now = (self.clock.get())();
        let mut entries = self.entries.borrow_mut();
        match done {
            Done::Loaded { key, stored, from, mut held, asked, seq } => {
                // Of the month before a roll.
                let Some(entry) = entries.get_mut(&key).filter(|entry| entry.stored == stored) else { return Vec::new() };
                entry.answered = entry.answered.max(seq);
                if from > entry.from && entry.loaded {
                    // A later load from further back is on its way.
                    return Vec::new();
                }
                if !asked.is_empty() {
                    let spans: Vec<String> = asked.iter().map(|(a, b)| format!("{a}..{b}")).collect();
                    eprintln!("omacharts: ladders of {key}: asked the provider for {}", spans.join(", "));
                }
                held.forming = entry.held.forming.take();
                // What sealed after this load was asked for, which it read
                // the store without; a load asked for after it reads them.
                for (_, minute) in entry.early.iter().filter(|(asked, _)| *asked >= seq) {
                    held.seal(entry.days.of(minute.ts), minute.clone());
                }
                entry.early.retain(|(asked, _)| *asked > seq);
                // Days whose minutes were loaded are kept whole.
                for day in entry.held.days.drain(..).filter(|d| d.whole) {
                    if held.days.iter().any(|h| h.day.day == day.day.day && !h.whole) {
                        held.load(day.day);
                    }
                }
                // Pieces are kept but for days the provider was just asked
                // about, which may have changed.
                let changed = |day: i64| asked.iter().any(|(a, b)| (entry.days.of(*a)..=entry.days.of(b - 1)).contains(&day));
                let order = std::mem::take(&mut entry.pieces);
                for piece in order {
                    if changed(piece.day) {
                        continue;
                    }
                    if let Some(ladder) = entry.held.pieces.remove(&piece)
                        && held.put_piece(piece, ladder)
                    {
                        entry.pieces.push_back(piece);
                    }
                }
                entry.asked.retain(|piece, _| !changed(piece.day));
                // Flows likewise, but for days that came whole.
                for (day, flow) in std::mem::take(&mut entry.held.flows) {
                    if !changed(day) {
                        held.put_flow(day, flow);
                    }
                }
                entry.asked_flows.retain(|day, _| !changed(*day));
                if held.tick_size <= 0.0 {
                    held.tick_size = entry.tick_size;
                }
                entry.held = *held;
                entry.loaded = true;
                entry.watchers.iter().copied().collect()
            }
            Done::Pieces { key, stored, pieces, missing } => {
                let Some(entry) = entries.get_mut(&key).filter(|entry| entry.stored == stored) else { return Vec::new() };
                // Asked for again later, each time later still, rather than
                // left asked for and estimated until the chart is opened again.
                for piece in missing {
                    if entry.asked.remove(&piece).is_some() {
                        let tries = entry.retry.get(&piece).map_or(0, |(_, tries)| *tries) + 1;
                        entry.retry.insert(piece, (now + retry_after(tries), tries));
                    }
                }
                let mut any = false;
                for (piece, mut ladder) in pieces {
                    // Not asked for any more: a load since may have changed it.
                    let Some(since) = entry.asked.remove(&piece) else { continue };
                    entry.retry.remove(&piece);
                    for minute in &since {
                        ladder.add_ladder(minute);
                    }
                    if entry.held.put_piece(piece, ladder) {
                        entry.pieces.push_back(piece);
                        any = true;
                    }
                }
                // Pieces a day let go of when it came whole leave the order.
                if entry.pieces.len() > 2 * entry.held.pieces.len() + 64 {
                    let held = &entry.held.pieces;
                    entry.pieces.retain(|piece| held.contains_key(piece));
                }
                if !any {
                    return Vec::new();
                }
                entry.watchers.iter().copied().collect()
            }
            Done::Flows { key, stored, flows, missing } => {
                let Some(entry) = entries.get_mut(&key).filter(|entry| entry.stored == stored) else { return Vec::new() };
                for day in missing {
                    if entry.asked_flows.remove(&day).is_some() {
                        let tries = entry.retry_flows.get(&day).map_or(0, |(_, tries)| *tries) + 1;
                        entry.retry_flows.insert(day, (now + retry_after(tries), tries));
                    }
                }
                let mut any = false;
                for (day, mut flow) in flows {
                    // Not asked for any more: a load since may have changed it.
                    let Some(since) = entry.asked_flows.remove(&day) else { continue };
                    entry.retry_flows.remove(&day);
                    for (ts, delta) in since {
                        match flow.binary_search_by_key(&ts, |(at, _)| *at) {
                            Ok(at) => flow[at] = (ts, delta),
                            Err(at) => flow.insert(at, (ts, delta)),
                        }
                    }
                    any |= entry.held.put_flow(day, flow);
                }
                if !any {
                    return Vec::new();
                }
                entry.watchers.iter().copied().collect()
            }
            Done::Filled { key, stored, days, asked } => {
                let Some(entry) = entries.get_mut(&key).filter(|entry| entry.stored == stored) else { return Vec::new() };
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
                        stored: entry.stored.clone(),
                        tick_size: entry.tick_size,
                        day,
                        minute: minute.clone(),
                        covered,
                    });
                    if entry.filling > 0 {
                        entry.sealed_meanwhile.push(minute.clone());
                    }
                    // A load on its way read the store before this was
                    // written: it goes on top of that load too.
                    if entry.loaded && entry.answered < entry.loads {
                        entry.early.push((entry.loads, minute.clone()));
                    }
                    if entry.loaded {
                        entry.seal(day, minute);
                    } else {
                        entry.early.push((entry.loads, minute));
                    }
                }
                if fill {
                    if entry.refill_at.is_some_and(|at| now >= at) {
                        entry.refill_at = None;
                    }
                    entry.filling += 1;
                    fills.push(Job::Fill {
                        key: key.clone(),
                        stored: entry.stored.clone(),
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

/// What the store keeps `key`'s ladders under: the key itself, or, for a
/// symbol the feed reads from a contract of another name — a root's front
/// month — the key and that contract. A stitched root, `ES~ESZ26~as-traded`,
/// is kept under its root's key with the month it is drawn up to and its
/// adjustment, `sierra:ES@ESZ26~as-traded`: one of the root's months, let go
/// with the others once it has rolled.
fn stored_key(key: &str, symbol: &str, contract: Option<String>) -> String {
    if let Some(spelled) = Spelled::parse(symbol) {
        return stitched_key(&family_of(key, symbol), &spelled);
    }
    match contract {
        Some(contract) if contract != symbol => format!("{key}@{contract}"),
        _ => key.to_string(),
    }
}

/// The root's key a stitched symbol's ladders are kept under the family
/// of: `sierra:ES` for `sierra:ES~ESZ26~as-traded`. Any other key is its own.
fn family_of(key: &str, symbol: &str) -> String {
    match (Spelled::parse(symbol), key.strip_suffix(symbol)) {
        (Some(spelled), Some(feed)) => format!("{feed}{}", spelled.root),
        _ => key.to_string(),
    }
}

fn stitched_key(family: &str, spelled: &Spelled) -> String {
    format!("{family}@{}~{}", omacharts_engine::continuous::contract(&spelled.root, spelled.front), spelled.adjustment.key())
}

/// What of `family` stays when `stored` is read: `stored`, and for a
/// stitched root the month it is drawn up to in the other adjustment too,
/// which another chart of it may be showing.
fn kept_with(family: &str, symbol: &str, stored: &str) -> Vec<String> {
    match Spelled::parse(symbol) {
        Some(spelled) => Adjustment::ALL.into_iter().map(|adjustment| stitched_key(family, &Spelled { adjustment, ..spelled.clone() })).collect(),
        None => vec![stored.to_string()],
    }
}

impl Entry {
    /// A minute sealed live into trading day `day`, kept too for whatever
    /// of that day is on its way back from the store without it.
    fn seal(&mut self, day: i64, minute: Minute) {
        if !self.held.days.binary_search_by_key(&day, |d| d.day.day).is_ok_and(|at| self.held.days[at].whole) {
            for (piece, since) in self.asked.iter_mut() {
                if piece.day == day && piece.holds(minute.ts) {
                    since.push(minute.ladder.clone());
                }
            }
            if let Some(since) = self.asked_flows.get_mut(&day) {
                since.push((minute.ts, minute.ladder.sides.delta()));
            }
        }
        self.held.seal(day, minute);
    }

    /// Let go of the oldest pieces not in `used` until what is held is under
    /// [`PIECE_BYTES`], or nothing unused is left to let go.
    fn make_room(&mut self, used: &HashSet<Piece>) {
        let mut bytes = self.held.piece_bytes();
        if bytes < PIECE_BYTES {
            return;
        }
        let mut kept = VecDeque::with_capacity(self.pieces.len());
        while let Some(piece) = self.pieces.pop_front() {
            if bytes >= PIECE_BYTES && !used.contains(&piece) {
                if let Some(ladder) = self.held.pieces.remove(&piece) {
                    bytes = bytes.saturating_sub(
                        ladder.levels.capacity() * std::mem::size_of::<omacharts_engine::ladders::Level>()
                            + std::mem::size_of::<(Piece, Ladder)>(),
                    );
                }
            } else {
                kept.push_back(piece);
            }
        }
        self.pieces = kept;
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
            Job::Load { key, stored, symbol, instrument, tick_size, from, now, seq } => {
                let days = Days::new(&instrument);
                let from = from.max(now - i64::from(ladders.history_days()) * 86_400);
                let _ = store.compact_ladders(&stored, days.of(now));
                let family = family_of(&key, &symbol);
                if stored != family {
                    let _ = store.forget_other_months(&family, &kept_with(&family, &symbol, &stored));
                }
                let asked = fill(&store, ladders, &stored, &symbol, tick_size, &days, from, now);
                let held = store.load_ladder_held(&stored, &days, days.of(from), days.of(now) + 1);
                Done::Loaded { key, stored, from, held: Box::new(held), asked, seq }
            }
            Job::Fill { key, stored, symbol, instrument, tick_size, from, now } => {
                let days = Days::new(&instrument);
                let from = from.max(now - i64::from(ladders.history_days()) * 86_400);
                let asked = fill(&store, ladders, &stored, &symbol, tick_size, &days, from, now);
                let mut numbers: Vec<i64> = asked.iter().flat_map(|(a, b)| days.of(*a)..=days.of(b - 1)).collect();
                numbers.sort_unstable();
                numbers.dedup();
                let changed = numbers.into_iter().filter_map(|day| store.load_ladder_day(&stored, day)).collect();
                Done::Filled { key, stored, days: changed, asked }
            }
            // The day is read and dropped here; only its pieces cross.
            Job::Pieces { key, stored, day, pieces } => match store.load_ladder_day(&stored, day) {
                Some(day) => {
                    let pieces = pieces.into_iter().map(|piece| (piece, day.piece(piece.from, piece.to))).collect();
                    Done::Pieces { key, stored, pieces, missing: Vec::new() }
                }
                None => Done::Pieces { key, stored, pieces: Vec::new(), missing: pieces },
            },
            // Each day read and dropped here; only its flow crosses.
            Job::Flows { key, stored, days } => {
                let (mut flows, mut missing) = (Vec::new(), Vec::new());
                for day in days {
                    match store.load_ladder_day(&stored, day) {
                        Some(read) => flows.push((day, read.flow())),
                        None => missing.push(day),
                    }
                }
                Done::Flows { key, stored, flows, missing }
            }
            Job::Append { stored, tick_size, day, minute, covered } => {
                let _ = store.append_ladder_minute(&stored, tick_size, day, &minute, covered);
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

    /// Everything back from the thread within a few seconds, put in place.
    fn settle_all(at_price: &AtPrice, until: impl Fn() -> bool) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while !until() {
            assert!(std::time::Instant::now() < deadline, "the thread did not answer in time");
            match at_price.done().try_recv() {
                Ok(done) => {
                    at_price.finished(done);
                }
                Err(_) => std::thread::sleep(std::time::Duration::from_millis(5)),
            }
        }
    }

    /// A regular session cuts every trading day it is in. Twenty days back,
    /// far more than a chart ever held the minutes of, every one is drawn
    /// from what traded once its piece is read — a few kilobytes each, and
    /// never the days' minutes.
    #[test]
    fn every_period_that_cuts_a_day_is_answered_however_far_back() {
        let path = scratch("pieces");
        let asked = Arc::new(Mutex::new(Asked::default()));
        let at_price = AtPrice::new(Arc::new(Fake(asked.clone())), Some(path.clone())).unwrap();
        at_price.set_clock(now);
        let back = 20 * 86_400;
        at_price.want(1, KEY, "ES", &es(), NOW - back);
        settle(&at_price);

        // Three hours from 03:20 UTC on each of the twenty days before:
        // inside a CME day, never the whole of one.
        let spans: Vec<(i64, i64)> = (1..=20).map(|k| (NOW - k * 86_400, NOW - k * 86_400 + 3 * HOUR)).collect();
        let answers = || {
            at_price.with_held(KEY, |held| {
                let held = held.unwrap();
                spans.iter().map(|(a, b)| held.between(*a, *b).map(|l| l.sides.volume())).collect::<Vec<_>>()
            })
        };
        assert!(answers().iter().all(Option::is_none), "the days are held by their totals alone");
        at_price.load_wanted(KEY);
        settle_all(&at_price, || answers().iter().all(Option::is_some));
        assert!(answers().iter().all(|volume| *volume == Some(180 * 5)), "three hours of minutes, five each");
        at_price.with_held(KEY, |held| {
            let held = held.unwrap();
            assert!(held.days.iter().all(|d| !d.whole), "no day's minutes were read into memory");
            assert_eq!(held.pieces.len(), 20);
            assert!(held.piece_bytes() < 20 * 4_096, "{} bytes", held.piece_bytes());
        });

        // Asked again, nothing more goes to the thread: all are held.
        answers();
        at_price.load_wanted(KEY);
        assert!(at_price.entries.borrow().get(KEY).unwrap().asked.is_empty());
        at_price.shutdown();
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    /// Today held by its total, a cumulative delta and a profile running on
    /// past the newest minute each ask for what they need of it; a minute
    /// seals before the answers are back, read from the store before that
    /// minute was written to it. Each answer has the minute all the same,
    /// and the profile's piece keeps taking the minutes that seal after.
    #[test]
    fn a_minute_sealed_while_today_is_being_read_is_not_lost() {
        let path = scratch("meanwhile");
        let asked = Arc::new(Mutex::new(Asked::default()));
        let at_price = AtPrice::new(Arc::new(Fake(asked.clone())), Some(path.clone())).unwrap();
        at_price.set_clock(now);
        at_price.want(1, KEY, "ES", &es(), NOW - HOUR);
        settle(&at_price);
        let from = NOW - 30 * 60;
        let flow = || {
            at_price.with_held(KEY, |held| {
                let mut flow = Vec::new();
                held.unwrap().minute_deltas(from, NOW + HOUR, |ts, delta| flow.push((ts, delta))).then_some(flow)
            })
        };
        let volume = || at_price.with_held(KEY, |held| held.unwrap().between(from, NOW + HOUR).map(|l| l.sides.volume()));
        assert_eq!((flow(), volume()), (None, None), "today is held by its total");
        at_price.load_wanted(KEY);

        let big = |ts: i64| {
            let mut minute = Minute::opening(ts, 7_625.0);
            minute.add_trade(30_500, 7_625.0, 40, Side::Buy, 1);
            minute
        };
        let say = |update: LadderUpdate| (asked.lock().unwrap().sinks[0])(update);
        say(LadderUpdate::Sealed { minute: big(NOW), covered: Some((NOW, NOW + 60)) });
        at_price.drain();
        settle_all(&at_price, || flow().is_some() && volume().is_some());
        let flow = flow().unwrap();
        assert_eq!(flow.len(), 31, "the half hour read and the minute sealed meanwhile");
        assert_eq!(flow.last(), Some(&(NOW, 40)));
        assert_eq!(volume(), Some(30 * 5 + 40));

        say(LadderUpdate::Sealed { minute: big(NOW + 60), covered: Some((NOW + 60, NOW + 120)) });
        at_price.drain();
        assert_eq!(volume(), Some(30 * 5 + 80), "the piece takes the next minute too, with no read");
        at_price.shutdown();
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    /// A chart reaching further back loads its contract again, and a minute
    /// seals before that load is back: the load read the store before the
    /// minute was written to it, and the minute is in what it brings all
    /// the same.
    #[test]
    fn a_minute_sealed_while_a_reload_is_on_its_way_is_not_lost() {
        let path = scratch("reload");
        let asked = Arc::new(Mutex::new(Asked::default()));
        let at_price = AtPrice::new(Arc::new(Fake(asked.clone())), Some(path.clone())).unwrap();
        at_price.set_clock(now);
        at_price.want(1, KEY, "ES", &es(), NOW - HOUR);
        settle(&at_price);
        let volume = |from: i64| {
            at_price.with_held(KEY, |held| held.unwrap().between(from, NOW + 120).map(|ladder| ladder.sides.volume()))
        };
        assert_eq!(volume(NOW - HOUR), Some(60 * 5));

        at_price.want(2, KEY, "ES", &es(), NOW - 2 * HOUR);
        (asked.lock().unwrap().sinks[0])(LadderUpdate::Sealed { minute: minute(NOW), covered: Some((NOW, NOW + 60)) });
        at_price.drain();
        assert_eq!(volume(NOW - HOUR), Some(60 * 5 + 5), "sealed into what is held now");
        let mut charts = settle(&at_price);
        charts.sort_unstable();
        assert_eq!(charts, vec![1, 2], "the reload back");
        assert_eq!(volume(NOW - 2 * HOUR), Some(2 * 60 * 5 + 5), "two hours read, and the minute sealed meanwhile");
        // The next load asked for reads the minute from the store: it is
        // not put in twice.
        at_price.want(3, KEY, "ES", &es(), NOW - 3 * HOUR);
        settle(&at_price);
        assert_eq!(volume(NOW - 3 * HOUR), Some(3 * 60 * 5 + 5));
        at_price.shutdown();
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    static RETRY_CLOCK: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(NOW);

    fn retry_clock() -> i64 {
        RETRY_CLOCK.load(Ordering::SeqCst)
    }

    /// The store no longer has the days a period's pieces are read from:
    /// they are not left asked for until the chart is opened again, but
    /// asked for once more after a while, and not before.
    #[test]
    fn a_piece_the_store_does_not_have_is_asked_for_again_later() {
        let path = scratch("retry");
        let asked = Arc::new(Mutex::new(Asked::default()));
        let at_price = AtPrice::new(Arc::new(Fake(asked.clone())), Some(path.clone())).unwrap();
        RETRY_CLOCK.store(NOW, Ordering::SeqCst);
        at_price.set_clock(retry_clock);
        at_price.want(1, KEY, "ES", &es(), NOW - 2 * 86_400);
        settle(&at_price);
        {
            let store = crate::store::connect(&path).unwrap();
            store.execute("DELETE FROM ladder_days WHERE key = ?1", [KEY]).unwrap();
            store.execute("DELETE FROM ladder_tail WHERE key = ?1", [KEY]).unwrap();
        }
        let (a, b) = (NOW - 86_400, NOW - 86_400 + 3 * HOUR);
        let wanted = || at_price.with_held(KEY, |held| held.unwrap().between(a, b).is_none());
        let in_flight = || !at_price.entries.borrow().get(KEY).unwrap().asked.is_empty();
        assert!(wanted(), "the day is held by its total");
        at_price.load_wanted(KEY);
        assert!(in_flight());
        settle(&at_price);
        assert!(!in_flight(), "left asked for while the store has nothing to give");

        assert!(wanted());
        at_price.load_wanted(KEY);
        assert!(!in_flight(), "asked again at once");

        RETRY_CLOCK.store(NOW + 61, Ordering::SeqCst);
        assert!(wanted());
        at_price.load_wanted(KEY);
        assert!(in_flight(), "never asked again");
        settle(&at_price);
        assert_eq!(at_price.entries.borrow().get(KEY).unwrap().retry.values().next().map(|(_, tries)| *tries), Some(2));
        at_price.shutdown();
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    /// A feed that reads a root from its front month: the month is part of
    /// what is kept, so the month after a roll never draws the one before.
    struct Monthly(Fake, Mutex<&'static str>);

    impl Ladders for Monthly {
        fn serves(&self, symbol: &str) -> bool {
            Ladders::serves(&self.0, symbol)
        }
        fn history_days(&self) -> u32 {
            self.0.history_days()
        }
        fn contract(&self, _: &str) -> Option<String> {
            Some(self.1.lock().unwrap().to_string())
        }
        fn minutes(&self, symbol: &str, tick: f64, from: i64, to: i64) -> Result<Vec<Minute>, ProviderError> {
            self.0.minutes(symbol, tick, from, to)
        }
        fn watch(&self, symbol: &str, tick: f64, sink: LadderSink) -> Result<Box<dyn Subscription>, ProviderError> {
            self.0.watch(symbol, tick, sink)
        }
    }

    impl Provider for Monthly {
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

    /// A root rolls between one start and the next, and while it is
    /// charted: each time the new month is read afresh and kept apart, and
    /// the month before is let go, in memory and in the store.
    #[test]
    fn a_root_is_kept_by_the_month_it_is_read_from_and_a_roll_reads_afresh() {
        let path = scratch("roll");
        let asked = Arc::new(Mutex::new(Asked::default()));
        for month in ["ESZ26_FUT_CME", "ESH27_FUT_CME"] {
            let at_price =
                AtPrice::new(Arc::new(Monthly(Fake(asked.clone()), Mutex::new(month))), Some(path.clone())).unwrap();
            at_price.set_clock(now);
            at_price.want(1, KEY, "ES", &es(), NOW - HOUR);
            settle(&at_price);
            at_price.shutdown();
        }
        assert_eq!(asked.lock().unwrap().spans, vec![(NOW - HOUR, NOW); 2], "the new month asked for afresh");
        {
            let store = Store::open_at(&path).unwrap();
            assert!(store.ladder_days("fake:ES@ESZ26_FUT_CME").is_empty(), "the month rolled from goes");
            assert!(!store.ladder_days("fake:ES@ESH27_FUT_CME").is_empty());
            assert!(store.ladder_days(KEY).is_empty());
        }

        // Rolled while two charts hold it: both go on to the new month, from
        // as far back as either reached.
        let monthly = Arc::new(Monthly(Fake(asked.clone()), Mutex::new("ESH27_FUT_CME")));
        let at_price = AtPrice::new(monthly.clone(), Some(path.clone())).unwrap();
        at_price.set_clock(now);
        at_price.want(1, KEY, "ES", &es(), NOW - HOUR);
        at_price.want(2, KEY, "ES", &es(), NOW - 2 * HOUR);
        let loaded_from = |from: i64| at_price.entries.borrow().get(KEY).is_some_and(|e| e.loaded && e.from == from);
        // Both loads answered, the second's asks of the provider made,
        // before what is asked next is counted.
        settle(&at_price);
        settle(&at_price);
        assert!(loaded_from(NOW - 2 * HOUR));
        asked.lock().unwrap().spans.clear();
        *monthly.1.lock().unwrap() = "ESM27_FUT_CME";
        at_price.want(1, KEY, "ES", &es(), NOW - HOUR);
        {
            let entries = at_price.entries.borrow();
            let entry = entries.get(KEY).unwrap();
            assert_eq!(entry.stored, "fake:ES@ESM27_FUT_CME");
            assert_eq!((entry.from, entry.loaded), (NOW - 2 * HOUR, false), "read afresh from the furthest back");
            assert_eq!(entry.watchers, HashSet::from([1, 2]));
        }
        settle_all(&at_price, || loaded_from(NOW - 2 * HOUR));
        assert_eq!(asked.lock().unwrap().spans, vec![(NOW - 2 * HOUR, NOW)]);
        at_price.shutdown();
        let store = Store::open_at(&path).unwrap();
        assert!(store.ladder_days("fake:ES@ESH27_FUT_CME").is_empty());
        assert_eq!(stored_key(KEY, "ESZ26", Some("ESZ26".into())), KEY, "a dated contract is its own");
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    /// A stitched root's ladders are one of its root's months, kept by the
    /// month they are drawn up to and the adjustment: once it rolls, what
    /// was kept of the month before goes, in either adjustment, and so does
    /// what a chart of the root's own front month kept, while the month
    /// drawn now stays in both.
    #[test]
    fn a_stitched_root_s_ladders_go_with_the_month_it_rolled_from() {
        let key = "sierra:ES~ESH27~back-adjusted";
        let symbol = "ES~ESH27~back-adjusted";
        let stored = stored_key(key, symbol, None);
        assert_eq!(stored, "sierra:ES@ESH27~back-adjusted");
        let family = family_of(key, symbol);
        assert_eq!(family, "sierra:ES");
        assert_eq!(kept_with(&family, symbol, &stored), ["sierra:ES@ESH27~back-adjusted", "sierra:ES@ESH27~as-traded"]);

        let store = Store::memory().unwrap();
        let days = omacharts_engine::ladders::Days::fixed(0);
        let mut series = omacharts_engine::ladders::Series::new(0.25);
        series.insert(&days, minute(60));
        let earlier = ["sierra:ES@ESZ26~back-adjusted", "sierra:ES@ESZ26~as-traded", "sierra:ES@ESZ26_FUT_CME", "sierra:ES"];
        let now = ["sierra:ES@ESH27~back-adjusted", "sierra:ES@ESH27~as-traded"];
        for kept in earlier.iter().chain(&now).chain(&["sierra:ESZ26"]) {
            store.save_ladder_days(kept, 0.25, &series.days).unwrap();
        }
        assert!(store.forget_other_months(&family, &kept_with(&family, symbol, &stored)).unwrap());
        for gone in earlier {
            assert!(store.ladder_days(gone).is_empty(), "{gone} kept");
        }
        for kept in now.iter().chain(&["sierra:ESZ26"]) {
            assert!(!store.ladder_days(kept).is_empty(), "{kept} gone");
        }
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
