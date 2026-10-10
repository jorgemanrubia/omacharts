//! What a tick costs, from the provider's thread to the frame.
//!
//!     cargo run --release -p omacharts --example stream_bench
//!
//! Three numbers, because a tick has three places to cost something.
//!
//! On the provider's thread, a tick is a push into a mailbox: a lock, one
//! bar overwritten, and — once per drain — a wake. That is measured by
//! pushing a hundred thousand ticks at one forming bar.
//!
//! On the main loop, a drain folds what waited into the series in memory
//! and hands each watching chart its tail. That is measured through the real
//! registry with a fake provider: a burst of ticks on one series watched by
//! 1, 4 and 16 charts, and the same burst spread over 16 series.
//!
//! On each chart, the tail is put in place and the indicators recomputed —
//! the one cost that grows with the chart, and it is per frame, not per
//! tick. That is measured on the same scene `frame_bench` draws: a volume
//! strip, two averages and an RSI over a series of the given length.
//!
//! Release, always, for the same reason as `frame_bench`.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use omacharts::live::{Live, Series};
use omacharts::ui::chart::bench::Scene;
use omacharts_engine::stream::Mailbox;
use omacharts_engine::{
    Bar, Capability, Instrument, InstrumentKind, Provider, ProviderError, Sink, Stream,
    Subscription, Timeframe, Update,
};

fn main() {
    println!("release\n");
    mailbox();
    println!();
    drain();
    println!();
    chart();
}

fn bar(ts: i64, close: f64) -> Bar {
    Bar { ts, open: close, high: close, low: close, close, volume: 1.0 }
}

/// The provider's side: pushing a tick.
fn mailbox() {
    let mailbox = Mailbox::new();
    let ticks = 100_000u32;
    let started = Instant::now();
    for i in 0..ticks {
        mailbox.push(Update::Bar(bar(60, i as f64)));
    }
    let took = started.elapsed();
    println!(
        "push one tick into a mailbox: {:.0} ns  ({ticks} ticks on one forming bar, no allocation after the first)",
        took.as_nanos() as f64 / ticks as f64
    );
}

/// A provider that streams nothing by itself; the benchmark pushes.
struct Pushed {
    sinks: Arc<Mutex<Vec<Arc<Sink>>>>,
}

struct Handle;
impl Subscription for Handle {}

impl Stream for Pushed {
    fn subscribe(
        &self,
        _: &str,
        _: Timeframe,
        _: Option<i64>,
        sink: Sink,
    ) -> Result<Box<dyn Subscription>, ProviderError> {
        self.sinks.lock().unwrap().push(Arc::new(sink));
        Ok(Box::new(Handle))
    }
}

impl Provider for Pushed {
    fn id(&self) -> &'static str {
        "bench"
    }
    fn label(&self) -> &'static str {
        "Bench"
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
    fn stream(&self) -> Option<&dyn Stream> {
        Some(self)
    }
}

/// The main loop's side: a drain through the real registry.
fn drain() {
    const BURST: usize = 64;
    println!(
        "{:>7}  {:>7}  {:>26}  {:>22}",
        "series", "charts", "drain of a tick (us)", "burst of 64 (us)"
    );
    for (series_count, charts_per) in [(1usize, 1usize), (1, 4), (1, 16), (16, 1)] {
        let sinks: Arc<Mutex<Vec<Arc<Sink>>>> = Arc::new(Mutex::new(Vec::new()));
        let provider: Arc<dyn Provider> = Arc::new(Pushed { sinks: sinks.clone() });
        let live = Live::new(provider, None);
        let mut pane = 1u32;
        for n in 0..series_count {
            let series = Series { key: format!("bench:S{n}"), native: Timeframe::minutes(1) };
            for _ in 0..charts_per {
                live.watch(pane, series.clone(), &format!("S{n}"), None);
                pane += 1;
            }
        }
        let sinks: Vec<Arc<Sink>> = sinks.lock().unwrap().clone();
        let history: Vec<Bar> = (0..2_000).map(|i| bar(i * 60, 100.0)).collect();
        for sink in &sinks {
            sink(Update::Snapshot(history.clone()));
        }
        live.drain();
        let _ = live.nudges().try_recv();

        let one = time(|| {
            for sink in &sinks {
                sink(Update::Bar(bar(1_999 * 60, 101.0)));
            }
            let drained = live.drain();
            assert_eq!(drained.changes.len(), sinks.len());
        });
        let burst = time(|| {
            for i in 0..BURST {
                for sink in &sinks {
                    sink(Update::Bar(bar(1_999 * 60, 100.0 + i as f64)));
                }
            }
            let drained = live.drain();
            assert_eq!(drained.changes.len(), sinks.len());
        });
        println!(
            "{:>7}  {:>7}  {:>26.2}  {:>22.2}",
            series_count,
            series_count * charts_per,
            one.as_nanos() as f64 / 1_000.0,
            burst.as_nanos() as f64 / 1_000.0
        );
    }
}

/// The chart's side: the tail in place and the indicators recomputed.
fn chart() {
    println!("{:>6}  {:>34}", "bars", "tail + indicators per chart (us)");
    for bars in [500usize, 2_000, 8_000] {
        let mut scene = Scene::new(bars, 160);
        let last = scene.last_bar().expect("a series");
        let mut close = last.close;
        let took = time(|| {
            close += 0.01;
            scene.tick(&[Bar { close, ..last }]);
        });
        println!("{:>6}  {:>34.1}", bars, took.as_nanos() as f64 / 1_000.0);
    }
}

/// The mean of `work` over a fixed stretch after a warm-up.
fn time(mut work: impl FnMut()) -> Duration {
    let warm = Instant::now();
    while warm.elapsed() < Duration::from_millis(100) {
        work();
    }
    let mut runs = 0u32;
    let started = Instant::now();
    while started.elapsed() < Duration::from_millis(500) {
        work();
        runs += 1;
    }
    started.elapsed() / runs
}
