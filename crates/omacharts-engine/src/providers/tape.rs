//! A simulated market, for filming: a feed that charts a tape of trades
//! written by `video/sim/tape`, in a build with the `fakes` feature only.
//!
//! The directory named by [`DIR_ENV`] holds, for each future root it serves,
//! `<ROOT>.history.csv` (one-minute bars, `ts,open,high,low,close,volume`)
//! and `<ROOT>.ticks`, one trade per line, `ts_ms price size side`, appended
//! to as the tape trades. Bars are the history plus the trades so far; the
//! stream sends a snapshot and then tails the trades every few
//! milliseconds, moving the forming bar on each. With the variable unset it
//! serves nothing.
//!
//! Its clock is the tape's: every bar and trade is stamped in the tape's
//! sim time, which is no wall clock's. The app is run under faketime set to
//! the same clock when it is filmed.

use std::collections::BTreeMap;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

use crate::bars::{Bar, Timeframe};
use crate::provider::{Capability, Provider, ProviderError, Sink, Stream, Subscription, Update};
use crate::providers::Listed;
use crate::symbols::{Instrument, InstrumentKind};

/// Where the tape is: a directory, set only when filming.
pub const DIR_ENV: &str = "OMACHARTS_TAPE";

/// How often the stream looks for new trades.
const POLL: Duration = Duration::from_millis(8);

pub const LISTED: Listed = Listed {
    id: "tape",
    label: "Simulated tape",
    serves: "Futures, simulated, from a tape on this machine",
    covers: "Futures, simulated",
    build: || Box::new(Tape),
    setup: None,
    experimental: false,
    gate: None,
};

const CAPABILITIES: &[Capability] =
    &[Capability { timeframe: Timeframe::minutes(1), history_days: None, max_request_days: None }];

/// The tape's directory, when one is set.
pub fn dir() -> Option<PathBuf> {
    std::env::var_os(DIR_ENV).filter(|dir| !dir.is_empty()).map(PathBuf::from)
}

/// Is a tape being served?
pub fn on() -> bool {
    dir().is_some()
}

/// One trade on the tape.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Trade {
    /// Sim time, in milliseconds since the epoch.
    pub ts_ms: i64,
    pub price: f64,
    pub size: f64,
    /// Bought at the ask, rather than sold at the bid.
    pub buy: bool,
}

/// Reads a root's trades as they are appended, from where it last stopped.
pub struct Tail {
    path: PathBuf,
    at: u64,
    partial: String,
}

impl Tail {
    /// From the first trade.
    pub fn new(root: &str) -> Option<Tail> {
        let path = dir()?.join(format!("{root}.ticks"));
        Some(Tail { path, at: 0, partial: String::new() })
    }

    /// From the trades appended after now.
    pub fn from_end(root: &str) -> Option<Tail> {
        let mut tail = Tail::new(root)?;
        tail.at = std::fs::metadata(&tail.path).map(|m| m.len()).unwrap_or(0);
        Some(tail)
    }

    /// Whatever was appended since the last call.
    pub fn read(&mut self) -> Vec<Trade> {
        let Ok(mut file) = std::fs::File::open(&self.path) else { return Vec::new() };
        if file.seek(SeekFrom::Start(self.at)).is_err() {
            return Vec::new();
        }
        let mut text = String::new();
        let Ok(read) = file.read_to_string(&mut text) else { return Vec::new() };
        self.at += read as u64;
        self.partial.push_str(&text);
        let mut trades = Vec::new();
        while let Some(end) = self.partial.find('\n') {
            let line: String = self.partial.drain(..=end).collect();
            if let Some(trade) = parse(&line) {
                trades.push(trade);
            }
        }
        trades
    }
}

fn parse(line: &str) -> Option<Trade> {
    let mut words = line.split_whitespace();
    Some(Trade {
        ts_ms: words.next()?.parse().ok()?,
        price: words.next()?.parse().ok()?,
        size: words.next()?.parse().ok()?,
        buy: words.next()? == "B",
    })
}

fn history(dir: &Path, root: &str) -> Vec<Bar> {
    let Ok(text) = std::fs::read_to_string(dir.join(format!("{root}.history.csv"))) else { return Vec::new() };
    text.lines()
        .skip(1)
        .filter_map(|line| {
            let mut v = line.split(',');
            Some(Bar {
                ts: v.next()?.parse().ok()?,
                open: v.next()?.parse().ok()?,
                high: v.next()?.parse().ok()?,
                low: v.next()?.parse().ok()?,
                close: v.next()?.parse().ok()?,
                volume: v.next()?.parse().ok()?,
            })
        })
        .collect()
}

/// Fold `trade` into the one-minute bars.
fn fold(bars: &mut BTreeMap<i64, Bar>, trade: &Trade) -> Bar {
    let ts = trade.ts_ms.div_euclid(60_000) * 60;
    let bar = bars.entry(ts).or_insert(Bar {
        ts,
        open: trade.price,
        high: trade.price,
        low: trade.price,
        close: trade.price,
        volume: 0.0,
    });
    bar.high = bar.high.max(trade.price);
    bar.low = bar.low.min(trade.price);
    bar.close = trade.price;
    bar.volume += trade.size;
    *bar
}

/// Every one-minute bar of `root`: the history, then the trades so far, and
/// where in the trades it stopped reading.
fn minutes(root: &str) -> (BTreeMap<i64, Bar>, Option<Tail>) {
    let Some(dir) = dir() else { return (BTreeMap::new(), None) };
    let mut bars: BTreeMap<i64, Bar> = history(&dir, root).into_iter().map(|bar| (bar.ts, bar)).collect();
    let mut tail = Tail::new(root);
    if let Some(tail) = tail.as_mut() {
        for trade in tail.read() {
            fold(&mut bars, &trade);
        }
    }
    (bars, tail)
}

pub struct Tape;

impl Tape {
    fn serves_root(&self, root: &str) -> bool {
        dir().is_some_and(|dir| dir.join(format!("{root}.history.csv")).exists())
    }
}

impl Provider for Tape {
    fn id(&self) -> &'static str {
        LISTED.id
    }

    fn label(&self) -> &'static str {
        LISTED.label
    }

    fn delay_minutes(&self, _: InstrumentKind) -> u32 {
        0
    }

    fn native_symbol(&self, instrument: &Instrument) -> Option<String> {
        (instrument.kind == InstrumentKind::FutureRoot && self.serves_root(&instrument.symbol))
            .then(|| instrument.symbol.clone())
    }

    fn capabilities(&self) -> &'static [Capability] {
        CAPABILITIES
    }

    fn bars(&self, symbol: &str, timeframe: Timeframe, since: Option<i64>) -> Result<Vec<Bar>, ProviderError> {
        if !self.serves(timeframe) {
            return Err(ProviderError::Unsupported(timeframe.label()));
        }
        let (bars, _) = minutes(symbol);
        Ok(bars.into_values().filter(|bar| since.is_none_or(|since| bar.ts >= since)).collect())
    }

    fn stream(&self) -> Option<&dyn Stream> {
        Some(self)
    }

    fn contracts(&self, root: &str) -> Result<Vec<(String, chrono::NaiveDate)>, ProviderError> {
        let today = crate::contracts::today();
        Ok(crate::contracts::listing(root, today).into_iter().take(3).map(|l| (l.symbol, l.expires)).collect())
    }
}

impl Stream for Tape {
    fn subscribe(
        &self,
        symbol: &str,
        timeframe: Timeframe,
        since: Option<i64>,
        sink: Sink,
    ) -> Result<Box<dyn Subscription>, ProviderError> {
        if !self.serves(timeframe) {
            return Err(ProviderError::Unsupported(timeframe.label()));
        }
        let (stop, stopped) = mpsc::channel::<()>();
        let symbol = symbol.to_string();
        std::thread::Builder::new()
            .name(format!("tape {symbol}"))
            .spawn(move || serve(&symbol, since, &sink, &stopped))
            .map_err(|e| ProviderError::Network(e.to_string()))?;
        Ok(Box::new(Handle { _stop: stop }))
    }
}

struct Handle {
    _stop: mpsc::Sender<()>,
}

impl Subscription for Handle {}

fn serve(symbol: &str, since: Option<i64>, sink: &Sink, stopped: &mpsc::Receiver<()>) {
    let (mut bars, tail) = minutes(symbol);
    sink(Update::Snapshot(bars.values().filter(|bar| since.is_none_or(|since| bar.ts >= since)).copied().collect()));
    let Some(mut tail) = tail else {
        let _ = stopped.recv();
        return;
    };
    let mut forming = bars.values().next_back().map(|bar| bar.ts);
    loop {
        match stopped.recv_timeout(POLL) {
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            _ => return,
        }
        let trades = tail.read();
        if trades.is_empty() {
            continue;
        }
        for trade in &trades {
            let bar = fold(&mut bars, trade);
            // The minute turned: the bar before is closed, as it last stood.
            if forming.is_some_and(|ts| ts < bar.ts) {
                if let Some(closed) = forming.and_then(|ts| bars.get(&ts)) {
                    sink(Update::Bar(*closed));
                }
            }
            forming = Some(bar.ts);
        }
        if let Some(bar) = forming.and_then(|ts| bars.get(&ts)) {
            sink(Update::Bar(*bar));
        }
        // Keep what a snapshot needs and no more.
        while bars.len() > 20_000 {
            bars.pop_first();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_trade_line_reads() {
        assert_eq!(
            parse("1794494400250 7621.25 3 B\n"),
            Some(Trade { ts_ms: 1_794_494_400_250, price: 7621.25, size: 3.0, buy: true })
        );
        assert_eq!(parse("nonsense"), None);
    }

    #[test]
    fn trades_fold_into_minutes() {
        let mut bars = BTreeMap::new();
        let at = |s: i64, price: f64| Trade { ts_ms: s * 1000, price, size: 2.0, buy: true };
        fold(&mut bars, &at(1_794_494_400, 7621.0));
        fold(&mut bars, &at(1_794_494_430, 7623.5));
        fold(&mut bars, &at(1_794_494_459, 7620.25));
        let bar = fold(&mut bars, &at(1_794_494_460, 7620.5));
        assert_eq!(bars.len(), 2);
        let first = bars[&1_794_494_400];
        assert_eq!((first.open, first.high, first.low, first.close, first.volume), (7621.0, 7623.5, 7620.25, 7620.25, 6.0));
        assert_eq!(bar.ts, 1_794_494_460);
    }

    #[test]
    fn with_no_tape_set_it_serves_nothing() {
        if on() {
            return;
        }
        assert!(!Tape.serves_root("ES"));
        assert!(Tail::new("ES").is_none());
    }
}
