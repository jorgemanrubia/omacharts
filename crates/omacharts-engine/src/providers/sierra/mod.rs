//! Charts from Sierra Chart's own data files: whatever its data service has
//! put on this disk, read as it is written.
//!
//! Sierra Chart keeps every contract it has charted in its `Data` folder:
//! an intraday file of every trade, `ESZ26_FUT_CME.scid`, and a daily one,
//! `ESZ26_FUT_CME.dly`. That is the whole of this feed. Nothing is asked of
//! a server, so a chart of a contract Sierra Chart has is on screen as fast
//! as the disk gives up the part of the file the chart covers, and nothing
//! here costs anybody who has not set the feed up: its folder is read the
//! first time a chart asks.
//!
//! Every intraday resolution, and volume at price, is folded from the
//! trades themselves ([`sierra::fold`], over a mapped file), so the bars and
//! the ladders of a chart agree to the contract. Daily bars are Sierra
//! Chart's own. Live is the intraday file growing: a stream follows it as
//! Sierra Chart flushes, which it does as often as its "Intraday File Flush
//! Time in Milliseconds" says.
//!
//! A root is its front month, as for every feed: the contract the rule in
//! [`crate::contracts`] says trades now, when Sierra Chart has its trades,
//! and otherwise the soonest one it does have that has not expired. Not
//! stitched: a long chart starts where that contract's file does.
//!
//! What the files cannot say — which contracts a root lists and when each
//! expires — is asked of Sierra Chart's DTC server, when it is listening:
//! see [`setup`].

pub mod ladders;
pub mod setup;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use crate::bars::{Bar, Timeframe, Unit};
use crate::provider::{Capability, FetchFailure, Ladders, Provider, ProviderError, Sink, Stream, Subscription, Update};
use crate::specs::root_of;
use crate::symbols::{Instrument, InstrumentKind};

/// How the feed is offered.
pub const LISTED: crate::providers::Listed = crate::providers::Listed {
    id: "sierra",
    label: "Sierra Chart",
    serves: "Futures from your Sierra Chart's data files, every trade",
    covers: "Futures in your Sierra Chart",
    build: || Box::new(Sierra),
    setup: Some(&setup::Setup),
    experimental: false,
    gate: Some("sierra"),
};

/// Every resolution is folded from the trades, so any of these is as cheap
/// as another; the presets and the common ones in between.
const CAPABILITIES: &[Capability] = &[
    Capability { timeframe: Timeframe::minutes(1), history_days: None, max_request_days: None },
    Capability { timeframe: Timeframe::minutes(2), history_days: None, max_request_days: None },
    Capability { timeframe: Timeframe::minutes(3), history_days: None, max_request_days: None },
    Capability { timeframe: Timeframe::minutes(5), history_days: None, max_request_days: None },
    Capability { timeframe: Timeframe::minutes(10), history_days: None, max_request_days: None },
    Capability { timeframe: Timeframe::minutes(15), history_days: None, max_request_days: None },
    Capability { timeframe: Timeframe::minutes(30), history_days: None, max_request_days: None },
    Capability { timeframe: Timeframe::hours(1), history_days: None, max_request_days: None },
    Capability { timeframe: Timeframe::hours(2), history_days: None, max_request_days: None },
    Capability { timeframe: Timeframe::hours(4), history_days: None, max_request_days: None },
    Capability { timeframe: Timeframe::days(1), history_days: None, max_request_days: None },
];

/// The feed: a value with nothing in it. The folder is [`setup`]'s.
pub struct Sierra;

/// The folder as it was last read: the install, what its data folder
/// holds, and when that was listed.
pub(crate) struct Opened {
    pub install: sierra::Install,
    pub listing: sierra::Listing,
    listed_at: Instant,
}

impl Opened {
    /// The file of `root`'s contract of `month` in `year`.
    fn contract(&self, root: &str, month: char, year: i32) -> Option<&sierra::Entry> {
        let wanted = sierra::Contract { year, month };
        self.listing.of_root(root).iter().find(|entry| entry.symbol.contract == Some(wanted))
    }

    /// How many trades the intraday file `stem` holds, as its size says.
    pub(crate) fn records(&self, stem: &str) -> u64 {
        self.listing.get(stem).and_then(|entry| entry.intraday.as_ref()).map_or(0, |intraday| intraday.records)
    }

    /// The contract of `root` whose intraday file was written last.
    fn newest_with_trades(&self, root: &str) -> Option<&sierra::Entry> {
        self.listing
            .contracts_of(root)
            .filter(|entry| entry.has_intraday_records())
            .max_by_key(|entry| entry.intraday.as_ref().and_then(|intraday| intraday.modified))
    }

    /// Is there anything of `root` to chart: trades, or daily bars?
    fn has_root(&self, root: &str) -> bool {
        self.listing.of_root(root).iter().any(|entry| entry.has_intraday_records() || entry.daily.is_some())
    }

    pub(crate) fn scid_path(&self, stem: &str) -> Option<PathBuf> {
        self.listing.get(stem)?.intraday.as_ref().map(|intraday| intraday.path.clone())
    }

    fn dly_path(&self, stem: &str) -> Option<PathBuf> {
        self.listing.get(stem)?.daily.clone()
    }
}

/// How long a listing of the data folder stands before it is read again:
/// a contract Sierra Chart starts recording is charted within this.
const RELIST: Duration = Duration::from_secs(30);

static OPENED: RwLock<Option<Arc<Opened>>> = RwLock::new(None);

/// The folder set up, opened and listed, or `None` while there is none or
/// it is not a Sierra Chart folder. Lists again once the listing is older
/// than [`RELIST`]; one `read_dir`, a millisecond for a thousand files.
pub(crate) fn opened() -> Option<Arc<Opened>> {
    let fresh = OPENED.read().unwrap_or_else(|e| e.into_inner()).clone();
    if let Some(opened) = fresh.filter(|opened| opened.listed_at.elapsed() < RELIST) {
        return Some(opened);
    }
    let root = setup::folder()?;
    let install = sierra::Install::open(&root).ok()?;
    let listing = install.listing();
    let opened = Arc::new(Opened { install, listing, listed_at: Instant::now() });
    *OPENED.write().unwrap_or_else(|e| e.into_inner()) = Some(opened.clone());
    Some(opened)
}

/// Forget the folder as it was read: it is another one now.
pub(crate) fn forget_opened() {
    *OPENED.write().unwrap_or_else(|e| e.into_inner()) = None;
}

/// A contract as the app names it, `ESZ6` or `ESZ26`, as Sierra Chart's
/// file stem says it: `ESZ26_FUT_CME`, if the folder has one.
fn stem_of_contract(opened: &Opened, symbol: &str) -> Option<String> {
    let (root, month) = crate::contracts::parse(symbol, crate::contracts::today())?;
    opened.contract(&root, month.code(), month.year).map(|entry| entry.symbol.stem.clone())
}

/// The contract a root charts as: the rule's front month when Sierra Chart
/// has its trades, else the soonest contract it has trades of that has not
/// expired, else the one it has the most recent trades of.
fn front_stem(opened: &Opened, root: &str) -> Option<String> {
    let today = crate::contracts::today();
    let populated = |stem: &str| opened.records(stem) > 0;
    if let Some(front) = crate::contracts::front(root, today)
        && let Some(stem) = stem_of_contract(opened, &front.symbol)
        && populated(&stem)
    {
        return Some(stem);
    }
    for listed in crate::contracts::listing(root, today) {
        if let Some(stem) = stem_of_contract(opened, &listed.symbol).filter(|stem| populated(stem)) {
            return Some(stem);
        }
    }
    opened.newest_with_trades(root).map(|entry| entry.symbol.stem.clone())
}

/// The file stem `symbol` is charted from: a dated contract's own, or a
/// root's front month.
pub(crate) fn stem_for(opened: &Opened, symbol: &str) -> Option<String> {
    match root_of(symbol) == symbol {
        true => front_stem(opened, symbol),
        false => stem_of_contract(opened, symbol),
    }
}

/// What Sierra Chart calls the contract `symbol` trades as, for an order:
/// a dated contract, `ESZ6`, as `ESZ26_FUT_CME`, and a root as its front
/// month — the one its charts here are drawn from when the folder has it,
/// else the rule's. `None` for a root the rule knows no month of.
pub fn sierra_contract(symbol: &str) -> Option<String> {
    if let Some(stem) = opened().and_then(|opened| stem_for(&opened, symbol)) {
        return Some(stem);
    }
    let today = crate::contracts::today();
    let dated = match root_of(symbol) == symbol {
        true => crate::contracts::front(symbol, today)?.symbol,
        false => symbol.to_string(),
    };
    let (root, month) = crate::contracts::parse(&dated, today)?;
    Some(format!("{root}{}{:02}_FUT_CME", month.code(), month.year.rem_euclid(100)))
}

/// A contract of Sierra Chart's, `ESZ26_FUT_CME`, as the exchange spells
/// it, `ESZ6`; a name that is not a dated future as it is.
pub fn exchange_contract(sierra: &str) -> String {
    let Some(file) = sierra::symbols::parse(sierra) else { return sierra.to_string() };
    match file.contract.and_then(|contract| crate::contracts::Month::of(contract.month, contract.year)) {
        Some(month) => crate::contracts::symbol(&file.root, month),
        None => sierra.to_string(),
    }
}

/// Does Sierra Chart have anything of this root on disk?
fn has_root(opened: &Opened, root: &str) -> bool {
    opened.has_root(root)
}

/// The file's prices times this are real prices: Sierra Chart keeps them
/// as its service sends them, ES at a hundred times the price.
pub(crate) fn multiplier(opened: &Opened, stem: &str) -> f64 {
    opened.install.scale(stem).map_or(1.0, |scale| scale.multiplier)
}

/// The mapped intraday files, by path, kept open while anything charts
/// them: a second chart of a contract maps nothing again.
static MAPPED: Mutex<Option<HashMap<PathBuf, Arc<Mutex<sierra::Scid>>>>> = Mutex::new(None);

/// The intraday file at `path`, mapped, grown to what is on disk now.
pub(crate) fn mapped(path: &Path) -> Result<Arc<Mutex<sierra::Scid>>, ProviderError> {
    let mut all = MAPPED.lock().unwrap_or_else(|e| e.into_inner());
    let all = all.get_or_insert_with(HashMap::new);
    if let Some(scid) = all.get(path) {
        let scid = scid.clone();
        scid.lock().unwrap_or_else(|e| e.into_inner()).refresh().map_err(read_failed)?;
        return Ok(scid);
    }
    let scid = Arc::new(Mutex::new(sierra::Scid::open(path).map_err(read_failed)?));
    all.insert(path.to_path_buf(), scid.clone());
    Ok(scid)
}

fn read_failed(error: std::io::Error) -> ProviderError {
    ProviderError::Malformed(format!("Sierra Chart's file could not be read: {error}"))
}

fn not_set_up() -> ProviderError {
    ProviderError::NeedsSetup(setup::NOT_SET_UP.into())
}

fn bar_of(bar: &sierra::fold::Bar) -> Bar {
    Bar { ts: bar.start, open: bar.open, high: bar.high, low: bar.low, close: bar.close, volume: bar.volume as f64 }
}

/// The seconds a bar of `timeframe` spans, for one folded from trades.
fn step_of(timeframe: Timeframe) -> Result<i64, ProviderError> {
    match timeframe.unit {
        Unit::Minute | Unit::Hour => Ok(timeframe.seconds()),
        _ => Err(ProviderError::Unsupported(format!("{} is folded, not fetched", timeframe.label()))),
    }
}

/// Intraday bars of the file `stem`, from `since` on, folded from its
/// trades.
fn intraday(opened: &Opened, stem: &str, timeframe: Timeframe, since: Option<i64>) -> Result<Vec<Bar>, ProviderError> {
    let step = step_of(timeframe)?;
    let path = opened.scid_path(stem).ok_or(ProviderError::NotFound)?;
    let scid = mapped(&path)?;
    let scid = scid.lock().unwrap_or_else(|e| e.into_inner());
    // From the start of the bar `since` falls in, so that bar is whole.
    let records = match since {
        Some(since) => scid.from(since.div_euclid(step) * step),
        None => scid.records(),
    };
    let multiplier = multiplier(opened, stem);
    Ok(sierra::fold::bars(records, step, multiplier).iter().map(bar_of).collect())
}

/// Daily bars of `stem`, Sierra Chart's own, stamped at midnight UTC of
/// their date as the other feeds' are.
fn daily(opened: &Opened, stem: &str, since: Option<i64>) -> Result<Vec<Bar>, ProviderError> {
    let path = opened.dly_path(stem).ok_or(ProviderError::NotFound)?;
    let days = sierra::dly::read(&path).map_err(read_failed)?;
    let multiplier = multiplier(opened, stem);
    let from = since.unwrap_or(i64::MIN);
    Ok(days
        .iter()
        .map(|day| Bar {
            ts: sierra::unix_seconds_from_civil(day.year, day.month, day.day),
            open: day.open * multiplier,
            high: day.high * multiplier,
            low: day.low * multiplier,
            close: day.close * multiplier,
            volume: day.volume as f64,
        })
        .filter(|bar| bar.ts >= from && bar.close.is_finite() && bar.close > 0.0)
        .collect())
}

impl Provider for Sierra {
    fn id(&self) -> &'static str {
        LISTED.id
    }

    fn label(&self) -> &'static str {
        LISTED.label
    }

    fn delay_minutes(&self, _kind: InstrumentKind) -> u32 {
        0
    }

    /// A futures root, or a dated contract, Sierra Chart has files of.
    /// Read from the listing of its data folder, never from a file.
    fn symbol_for(&self, instrument: &Instrument) -> Option<String> {
        if let Some(explicit) = instrument.override_for(LISTED.id) {
            return Some(explicit.to_string());
        }
        if instrument.kind != InstrumentKind::FutureRoot {
            return None;
        }
        let opened = opened()?;
        has_root(&opened, instrument.root()).then(|| instrument.symbol.clone())
    }

    fn capabilities(&self) -> &'static [Capability] {
        CAPABILITIES
    }

    fn bars(&self, symbol: &str, timeframe: Timeframe, since: Option<i64>) -> Result<Vec<Bar>, ProviderError> {
        let opened = opened().ok_or_else(not_set_up)?;
        let stem = stem_for(&opened, symbol).ok_or(ProviderError::NotFound)?;
        match timeframe.unit {
            Unit::Day if timeframe.count == 1 => daily(&opened, &stem, since),
            _ => intraday(&opened, &stem, timeframe, since),
        }
    }

    fn stream(&self) -> Option<&dyn Stream> {
        Some(self)
    }

    fn ladders(&self) -> Option<&dyn Ladders> {
        Some(self)
    }

    /// The contracts of `root` Sierra Chart's DTC server lists, with when
    /// each expires: what the files cannot say. Unsupported while it is not
    /// listening, when the rule stands in.
    fn contracts(&self, root: &str) -> Result<Vec<(String, chrono::NaiveDate)>, ProviderError> {
        setup::contracts(root)
    }

    /// Unmap the files: what a stream still follows stays mapped until it
    /// ends.
    fn retire(&self) {
        *MAPPED.lock().unwrap_or_else(|e| e.into_inner()) = None;
        forget_opened();
    }
}

/// How often a stream looks again for the file it follows having been
/// replaced by another contract's, the root having rolled.
const ROLL_CHECK: Duration = Duration::from_secs(600);

/// A stream's end: the flag its thread reads, and the handle that wakes
/// the file watch it is blocked on.
struct Following {
    stop: Arc<AtomicBool>,
    wake: Arc<Mutex<Option<sierra::StopHandle>>>,
}

impl Subscription for Following {}

impl Drop for Following {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(wake) = self.wake.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
            wake.stop();
        }
    }
}

/// Where a stream's bars start: every so many seconds, or a trading day.
#[derive(Clone)]
enum Buckets {
    Every(i64),
    /// Midnight UTC of the trading date, as daily bars are stamped.
    Days(String),
}

impl Buckets {
    fn of(timeframe: Timeframe, symbol: &str) -> Result<Buckets, ProviderError> {
        match timeframe.unit {
            Unit::Day if timeframe.count == 1 => Ok(Buckets::Days(root_of(symbol).to_string())),
            _ => step_of(timeframe).map(Buckets::Every),
        }
    }

    fn start(&self, ts: i64) -> i64 {
        match self {
            Buckets::Every(step) => ts.div_euclid(*step) * step,
            Buckets::Days(root) => trading_day(root, ts),
        }
    }
}

/// Midnight UTC of the date `ts` trades on, for `root`'s exchange.
fn trading_day(root: &str, ts: i64) -> i64 {
    let instrument = Instrument {
        symbol: root.to_string(),
        name: String::new(),
        kind: InstrumentKind::FutureRoot,
        suffix: None,
        currency: None,
        tier: 0,
        session_origin: 0,
        overrides: Vec::new(),
        exchange: None,
        popularity: 0,
        local_name: None,
    };
    crate::exchanges::registry()
        .for_instrument(&instrument)
        .and_then(|schedule| schedule.trading_date(ts))
        .and_then(|date| date.and_hms_opt(0, 0, 0))
        .map(|midnight| midnight.and_utc().timestamp())
        .unwrap_or(ts.div_euclid(86_400) * 86_400)
}

/// What a stream folds the trades it hears into: the bar forming.
struct Forming {
    buckets: Buckets,
    multiplier: f64,
    bar: Option<Bar>,
}

impl Forming {
    /// Fold `records` in, one at a time, as Sierra Chart wrote them; the
    /// bars they changed, oldest first, each as it stands now. A wake
    /// brings what one flush wrote, a few hundred records at most.
    fn fold(&mut self, records: &[sierra::Record]) -> Vec<Bar> {
        let mut changed: Vec<Bar> = Vec::new();
        for record in records {
            let close = f64::from(record.close) * self.multiplier;
            if !close.is_finite() || close <= 0.0 {
                continue;
            }
            // A trade's open, high and low are not trade prices: its high
            // and low are the ask and the bid.
            let (open, high, low) = match record.is_tick() {
                true => (close, close, close),
                false => (
                    f64::from(record.open) * self.multiplier,
                    f64::from(record.high) * self.multiplier,
                    f64::from(record.low) * self.multiplier,
                ),
            };
            let start = self.buckets.start(record.unix_seconds());
            let volume = f64::from(record.volume);
            let bar = match self.bar {
                Some(forming) if forming.ts == start => Bar {
                    high: forming.high.max(high),
                    low: forming.low.min(low),
                    close,
                    volume: forming.volume + volume,
                    ..forming
                },
                // Sierra Chart writes in time order: a record behind the
                // bar forming is one already counted.
                Some(forming) if start < forming.ts => continue,
                _ => Bar { ts: start, open, high, low, close, volume },
            };
            self.bar = Some(bar);
            match changed.last_mut() {
                Some(last) if last.ts == bar.ts => *last = bar,
                _ => changed.push(bar),
            }
        }
        changed
    }
}

fn lost(error: &ProviderError) -> Update {
    Update::Lost(FetchFailure::from(error))
}

/// Follow `symbol` at `timeframe`: a snapshot from the file, then each
/// flush Sierra Chart makes to it, until `stop`.
fn follow(symbol: String, timeframe: Timeframe, since: Option<i64>, sink: Sink, stop: Arc<AtomicBool>, wake: Arc<Mutex<Option<sierra::StopHandle>>>) {
    let say = |update: Update| {
        if !stop.load(Ordering::Acquire) {
            sink(update);
        }
    };
    let Ok(buckets) = Buckets::of(timeframe, &symbol) else { return };
    let mut held = since;
    while !stop.load(Ordering::Acquire) {
        let Some(opened) = opened() else { return say(lost(&not_set_up())) };
        let Some(stem) = stem_for(&opened, &symbol) else { return say(lost(&ProviderError::NotFound)) };
        let Some(path) = opened.scid_path(&stem) else { return say(lost(&ProviderError::NotFound)) };
        // The snapshot, and where in the file it ends: folded under the
        // file's lock, so every record is either in it or the tail's.
        let snapshot = mapped(&path).and_then(|scid| {
            let scid = scid.lock().unwrap_or_else(|e| e.into_inner());
            let bars = match &buckets {
                Buckets::Days(_) => daily(&opened, &stem, held)?,
                Buckets::Every(step) => {
                    let records = match held {
                        Some(since) => scid.from(since.div_euclid(*step) * step),
                        None => scid.records(),
                    };
                    sierra::fold::bars(records, *step, multiplier(&opened, &stem)).iter().map(bar_of).collect()
                }
            };
            Ok((bars, scid.len()))
        });
        let (bars, position) = match snapshot {
            Ok(snapshot) => snapshot,
            Err(error) => return say(lost(&error)),
        };
        let mut forming = Forming { buckets: buckets.clone(), multiplier: multiplier(&opened, &stem), bar: bars.last().copied() };
        if let Some(last) = bars.last() {
            held = Some(held.map_or(last.ts, |held| held.max(last.ts)));
        }
        say(Update::Snapshot(bars));
        let mut tail = match sierra::Tail::open(&path, position) {
            Ok(tail) => tail,
            Err(error) => return say(lost(&read_failed(error))),
        };
        *wake.lock().unwrap_or_else(|e| e.into_inner()) = Some(tail.stop_handle());
        if stop.load(Ordering::Acquire) {
            return;
        }
        let mut checked = Instant::now();
        loop {
            match tail.wait(Duration::from_secs(5)) {
                Ok(sierra::Appended::Records(records)) => {
                    for bar in forming.fold(records) {
                        held = Some(held.map_or(bar.ts, |held| held.max(bar.ts)));
                        say(Update::Bar(bar));
                    }
                }
                Ok(sierra::Appended::Idle) => {}
                Ok(sierra::Appended::Stopped) => return,
                // Started over or replaced: a fresh snapshot.
                Ok(sierra::Appended::Truncated | sierra::Appended::Replaced | sierra::Appended::Removed) => break,
                Err(error) => {
                    say(lost(&read_failed(error)));
                    std::thread::sleep(Duration::from_secs(5));
                    break;
                }
            }
            if stop.load(Ordering::Acquire) {
                return;
            }
            // A root follows its front month: once it has moved, the new
            // contract's file from the start.
            if checked.elapsed() >= ROLL_CHECK {
                checked = Instant::now();
                let now = self::opened().and_then(|opened| stem_for(&opened, &symbol));
                if now.is_some_and(|now| now != stem) {
                    held = None;
                    break;
                }
            }
        }
    }
}

impl Stream for Sierra {
    fn subscribe(
        &self,
        symbol: &str,
        timeframe: Timeframe,
        since: Option<i64>,
        sink: Sink,
    ) -> Result<Box<dyn Subscription>, ProviderError> {
        Buckets::of(timeframe, symbol)?;
        let stop = Arc::new(AtomicBool::new(false));
        let wake = Arc::new(Mutex::new(None));
        let symbol = symbol.to_string();
        let (stopping, waking) = (stop.clone(), wake.clone());
        std::thread::Builder::new()
            .name("omacharts sierra stream".into())
            .spawn(move || follow(symbol, timeframe, since, sink, stopping, waking))
            .map_err(|error| ProviderError::Network(error.to_string()))?;
        Ok(Box::new(Following { stop, wake }))
    }
}
