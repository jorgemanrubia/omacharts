//! A feed that invents its prices, for exercising the streaming path without
//! an account.
//!
//! Not listed, so nothing in the UI or on the command line can choose it:
//! it exists for `examples/live_window.rs`, for the benchmarks, and for the
//! tests that need a provider which streams. Everything it serves is a
//! deterministic function of the symbol and the clock, so the history it
//! answers a one-shot request with and the snapshot its stream opens with
//! are the same bars — a chart painted from one and then fed by the other
//! does not jump.
//!
//! The stream is the honest shape of the contract in [`crate::Stream`]: a
//! thread per subscription that sends a snapshot first and then one bar per
//! tick, nudging the forming bar and opening a new one when the clock crosses
//! a boundary; dropping the handle ends it without blocking.

use std::sync::mpsc;
use std::time::{Duration, SystemTime};

use crate::bars::{Bar, Timeframe};
use crate::provider::{
    Capability, Provider, ProviderError, Sink, Stream, Subscription, Update,
};
use crate::symbols::{Instrument, InstrumentKind};

/// How many bars of history a request with no lower bound gets.
const HISTORY: i64 = 2_000;

const CAPABILITIES: &[Capability] = &[
    Capability { timeframe: Timeframe::minutes(1), history_days: None, max_request_days: None },
    Capability { timeframe: Timeframe::minutes(2), history_days: None, max_request_days: None },
    Capability { timeframe: Timeframe::minutes(5), history_days: None, max_request_days: None },
    Capability { timeframe: Timeframe::minutes(15), history_days: None, max_request_days: None },
    Capability { timeframe: Timeframe::minutes(30), history_days: None, max_request_days: None },
    Capability { timeframe: Timeframe::minutes(90), history_days: None, max_request_days: None },
    Capability { timeframe: Timeframe::hours(1), history_days: None, max_request_days: None },
    Capability { timeframe: Timeframe::days(1), history_days: None, max_request_days: None },
];

pub struct Synthetic {
    /// How often the forming bar moves. `None` streams the snapshot and then
    /// nothing, which is what a market that is shut looks like.
    tick: Option<Duration>,
}

impl Synthetic {
    pub fn new(tick: Option<Duration>) -> Synthetic {
        Synthetic { tick }
    }
}

impl Provider for Synthetic {
    fn id(&self) -> &'static str {
        "synthetic"
    }

    fn label(&self) -> &'static str {
        "Synthetic"
    }

    fn delay_minutes(&self, _: InstrumentKind) -> u32 {
        0
    }

    fn symbol_for(&self, instrument: &Instrument) -> Option<String> {
        Some(instrument.symbol.clone())
    }

    fn capabilities(&self) -> &'static [Capability] {
        CAPABILITIES
    }

    fn bars(
        &self,
        symbol: &str,
        timeframe: Timeframe,
        since: Option<i64>,
    ) -> Result<Vec<Bar>, ProviderError> {
        if !self.serves(timeframe) {
            return Err(ProviderError::Unsupported(timeframe.label()));
        }
        Ok(history(symbol, timeframe, since, now()))
    }

    fn stream(&self) -> Option<&dyn Stream> {
        Some(self)
    }
}

impl Stream for Synthetic {
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
        let tick = self.tick;
        std::thread::Builder::new()
            .name(format!("synthetic {symbol}"))
            .spawn(move || serve(&symbol, timeframe, since, tick, &sink, &stopped))
            .map_err(|e| ProviderError::Network(e.to_string()))?;
        Ok(Box::new(Handle { _stop: stop }))
    }
}

/// Dropping it drops the sender, which is what the thread is waiting on.
struct Handle {
    _stop: mpsc::Sender<()>,
}

impl Subscription for Handle {}

/// The thread: a snapshot, then a bar per tick until the handle goes.
fn serve(
    symbol: &str,
    timeframe: Timeframe,
    since: Option<i64>,
    tick: Option<Duration>,
    sink: &Sink,
    stopped: &mpsc::Receiver<()>,
) {
    let step = timeframe.seconds();
    let mut forming = history(symbol, timeframe, since, now()).pop();
    sink(Update::Snapshot(history(symbol, timeframe, since, now())));
    let Some(tick) = tick else {
        // A snapshot and then silence, for as long as somebody watches.
        let _ = stopped.recv();
        return;
    };
    let mut ticks = 0u64;
    loop {
        match stopped.recv_timeout(tick) {
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            // The handle was dropped, or sent something; either way, done.
            _ => return,
        }
        ticks += 1;
        let now = now();
        let open_at = now.div_euclid(step) * step;
        let bar = match forming {
            Some(bar) if bar.ts == open_at => nudged(symbol, bar, now, ticks),
            _ => nudged(symbol, opened(symbol, open_at, step), now, ticks),
        };
        forming = Some(bar);
        sink(Update::Bar(bar));
    }
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or_default()
}

/// `HISTORY` bars ending at the bar that is forming at `now`, or from `since`
/// if that is later.
fn history(symbol: &str, timeframe: Timeframe, since: Option<i64>, now: i64) -> Vec<Bar> {
    let step = timeframe.seconds();
    let last = now.div_euclid(step) * step;
    let mut first = last - (HISTORY - 1) * step;
    if let Some(since) = since {
        first = first.max(since.div_euclid(step) * step);
    }
    let mut bars = Vec::with_capacity(((last - first) / step + 1) as usize);
    let mut ts = first;
    while ts <= last {
        bars.push(opened(symbol, ts, step));
        ts += step;
    }
    // The forming bar has only run as far as the clock has.
    if let Some(forming) = bars.last_mut() {
        *forming = closed_at(symbol, *forming, now);
    }
    bars
}

/// A completed bar at `ts`: its open is the price then, its close the price
/// at the end of its step, and its range reaches a little past both.
fn opened(symbol: &str, ts: i64, step: i64) -> Bar {
    closed_at(symbol, Bar { ts, open: price(symbol, ts), high: 0.0, low: 0.0, close: 0.0, volume: 0.0 }, ts + step - 1)
}

/// `bar` as it stands with the clock at `at`.
fn closed_at(symbol: &str, bar: Bar, at: i64) -> Bar {
    let close = price(symbol, at);
    let reach = noise(symbol, bar.ts ^ 0x5bd1_e995) * 0.004;
    let high = bar.open.max(close) * (1.0 + reach);
    let low = bar.open.min(close) * (1.0 - reach);
    let elapsed = (at - bar.ts).max(1) as f64;
    Bar { close, high, low, volume: 1_000.0 * elapsed.sqrt() * (0.5 + noise(symbol, bar.ts)), ..bar }
}

/// The forming bar after one more tick: the close moves a touch, the range
/// widens if it has to, and a little volume prints.
fn nudged(symbol: &str, bar: Bar, now: i64, ticks: u64) -> Bar {
    let wobble = (noise(symbol, now ^ (ticks as i64).wrapping_mul(0x9e37_79b9)) - 0.5) * 0.002;
    let close = price(symbol, now) * (1.0 + wobble);
    Bar {
        close,
        high: bar.high.max(close),
        low: bar.low.min(close),
        volume: bar.volume + 10.0,
        ..bar
    }
}

/// The price of `symbol` at `ts`: a few slow waves and a little noise, so a
/// chart of it has the shape of a market. Stateless on purpose — see the
/// module docs.
fn price(symbol: &str, ts: i64) -> f64 {
    let base = 20.0 + (seed(symbol) % 480) as f64;
    let t = ts as f64;
    let waves = 0.06 * (t / 97_919.0).sin() + 0.03 * (t / 8_771.0).sin() + 0.012 * (t / 1_313.0).sin();
    base * (1.0 + waves + (noise(symbol, ts) - 0.5) * 0.004)
}

/// A number in 0..1 that depends on the symbol and `ts` and nothing else.
fn noise(symbol: &str, ts: i64) -> f64 {
    let mut x = seed(symbol) ^ (ts as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15);
    x ^= x >> 33;
    x = x.wrapping_mul(0xff51_afd7_ed55_8ccd);
    x ^= x >> 33;
    (x >> 11) as f64 / (1u64 << 53) as f64
}

fn seed(symbol: &str) -> u64 {
    symbol.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ b as u64).wrapping_mul(0x0100_0000_01b3))
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;
    use crate::provider::Delivery;

    /// The point of the double: it streams, so a window built on it installs
    /// no timer and subscribes instead.
    #[test]
    fn it_streams() {
        assert_eq!(Synthetic::new(None).delivery(), Delivery::Streamed);
    }

    /// A chart paints from `bars()` and is then fed by the stream. The two
    /// have to agree on every bar they both cover, or the chart jumps the
    /// moment the subscription opens.
    #[test]
    fn the_history_and_the_snapshot_are_the_same_bars() {
        let now = 1_700_000_000;
        let a = history("ES", Timeframe::minutes(5), None, now);
        let b = history("ES", Timeframe::minutes(5), Some(now - 3_600), now);
        assert_eq!(a.len(), HISTORY as usize);
        assert_eq!(b.len(), 13);
        assert_eq!(&a[a.len() - 13..], &b[..]);
        // Bars are well-formed.
        for bar in &a {
            assert!(bar.low <= bar.open.min(bar.close) && bar.high >= bar.open.max(bar.close));
        }
        assert_eq!(a.last().unwrap().ts, now.div_euclid(300) * 300);
    }

    #[test]
    fn different_symbols_are_different_prices() {
        assert_ne!(price("ES", 1_700_000_000), price("NQ", 1_700_000_000));
    }

    /// The contract: a snapshot first, bars after, and dropping the handle
    /// ends it.
    #[test]
    fn a_subscription_opens_with_a_snapshot_and_then_ticks() {
        let heard: Arc<Mutex<Vec<Update>>> = Arc::new(Mutex::new(Vec::new()));
        let into = heard.clone();
        let feed = Synthetic::new(Some(Duration::from_millis(5)));
        let handle = feed
            .subscribe("ES", Timeframe::minutes(1), None, Box::new(move |u| into.lock().unwrap().push(u)))
            .expect("a subscription");
        let started = std::time::Instant::now();
        while heard.lock().unwrap().len() < 3 && started.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(5));
        }
        drop(handle);
        let heard = heard.lock().unwrap();
        assert!(matches!(heard.first(), Some(Update::Snapshot(bars)) if bars.len() == HISTORY as usize));
        assert!(matches!(heard.get(1), Some(Update::Bar(_))));
    }

    #[test]
    fn a_resolution_it_does_not_serve_is_refused_before_any_thread_starts() {
        let feed = Synthetic::new(None);
        assert!(feed.subscribe("ES", Timeframe::hours(4), None, Box::new(|_| {})).is_err());
        assert!(matches!(feed.bars("ES", Timeframe::hours(4), None), Err(ProviderError::Unsupported(_))));
    }
}
