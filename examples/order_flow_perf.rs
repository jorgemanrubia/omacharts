//! What order flow costs the thread that draws the window.
//!
//!     cargo run --release --example order_flow_perf --features fakes
//!
//! Two tables. The first is frames, as `perf` measures them — the cheapest
//! run and what one run allocated — for a chart with each order flow
//! indicator on, beside the same chart with it off. The second is a replay:
//! some three million trades pushed through the real large orders registry
//! as fast as a thread can push them, while this thread draws frames and
//! answers a pointer, against the same loop with nothing arriving.
//!
//! Apart from `perf` because `bin/perf-compare` builds that file on the
//! revision a branch came off, where none of this exists. The comparison
//! here is within one build: with and without.
//!
//! Offline: a synthetic contract, a scratch `HOME`, and a feed of the
//! example's own.

use std::alloc::{GlobalAlloc, Layout, System};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use gtk::cairo;
use omacharts::large_orders::{LargeOrders, Settings};
use omacharts::ui::chart::flow_bench::{FlowScene, TICK};
use omacharts_engine::ladders::Minute;
use omacharts_engine::studies::orders::{Aggressor, Print};
use omacharts_engine::{
    Bar, Capability, Instrument, InstrumentKind, LadderSink, Ladders, PrintSink, Provider, ProviderError, Subscription,
    Timeframe,
};

const WIDTH: f64 = 1600.0;
const HEIGHT: f64 = 900.0;
const WARMUP: Duration = Duration::from_millis(80);
const MEASURE: Duration = Duration::from_millis(400);

/// Trades a synthetic minute: a busy ES minute is a few thousand.
const TRADES: usize = 400;

fn main() {
    let home = scratch();
    // SAFETY: before any thread, and nothing has read either yet.
    unsafe {
        std::env::set_var("HOME", &home);
        std::env::set_var("XDG_DATA_HOME", home.join(".local/share"));
    }
    frames();
    replay(&home);
    history(&home);
    let _ = std::fs::remove_dir_all(&home);
}

// ---------------------------------------------------------------------------
// Frames
// ---------------------------------------------------------------------------

fn frames() {
    let (surface, cr) = canvas();
    let rows: std::cell::RefCell<Vec<(String, u64, u64, u64)>> = Default::default();
    let frame = |name: &str, scene: &FlowScene| {
        let (ns, allocs, bytes) = measure(|| {
            scene.view_frame(&cr, WIDTH, HEIGHT);
            surface.flush();
        });
        rows.borrow_mut().push((name.to_string(), ns, allocs, bytes));
    };

    // Zoomed in: 20 bars across 1600 pixels is 80 a bar, past the
    // footprint's 64.
    frame("indicators off, 20 bars zoomed in", &FlowScene::new(400, 20, &["volume"], TRADES, 0));
    frame("footprint, 20 bars zoomed in", &FlowScene::new(400, 20, &["volume", "footprint"], TRADES, 0));
    frame(
        "footprint, numbers off, 20 bars zoomed in",
        &FlowScene::new(400, 20, &["volume", "footprint"], TRADES, 0).footprint_numbers(false),
    );
    frame("footprint, 30 bars, just too narrow for its figures", &FlowScene::new(400, 30, &["volume", "footprint"], TRADES, 0));
    frame("footprint on but zoomed out, 400 bars", &FlowScene::new(1_000, 400, &["volume", "footprint"], TRADES, 0));
    frame("indicators off, 400 bars", &FlowScene::new(1_000, 400, &["volume"], TRADES, 0));
    // A footprint's volume profile: twice the room, so fewer bars; and
    // zoomed out it should cost what no footprint costs.
    frame("footprint, 10 bars zoomed in", &FlowScene::new(400, 10, &["volume", "footprint"], TRADES, 0));
    frame(
        "footprint with its volume profile, 10 bars zoomed in",
        &FlowScene::new(400, 10, &["volume", "footprint"], TRADES, 0).footprint_profile(true),
    );
    frame(
        "footprint with its volume profile, numbers off, 20 bars",
        &FlowScene::new(400, 20, &["volume", "footprint"], TRADES, 0).footprint_numbers(false).footprint_profile(false),
    );
    frame(
        "footprint with its volume profile but zoomed out, 400 bars",
        &FlowScene::new(1_000, 400, &["volume", "footprint"], TRADES, 0).footprint_profile(true),
    );

    // Two charts of footprints, one over the other.
    let tiles = [(0.0, 0.0), (0.0, HEIGHT / 2.0)];
    for (name, kinds) in [("indicators off", &["volume"][..]), ("footprint", &["volume", "footprint"][..])] {
        let scene = FlowScene::new(400, 16, kinds, TRADES, 0);
        let (ns, allocs, bytes) = measure(|| {
            for (x, y) in tiles {
                cr.save().ok();
                cr.rectangle(x, y, WIDTH, HEIGHT / 2.0);
                cr.clip();
                cr.translate(x, y);
                scene.view_frame(&cr, WIDTH, HEIGHT / 2.0);
                cr.restore().ok();
            }
            surface.flush();
        });
        rows.borrow_mut().push((format!("{name}, 16 bars x2 charts"), ns, allocs, bytes));
    }

    // A dense tick stream: a trade on the forming minute, then the frame.
    for (name, kinds) in [("indicators off", &["volume"][..]), ("footprint", &["volume", "footprint"][..])] {
        let mut scene = FlowScene::new(400, 20, kinds, TRADES, 0);
        let (ns, allocs, bytes) = measure_mut(&mut scene, |scene| {
            scene.trade();
            scene.view_frame(&cr, WIDTH, HEIGHT);
            surface.flush();
        });
        rows.borrow_mut().push((format!("{name}, a trade then a frame, 20 bars"), ns, allocs, bytes));
    }
    // The same stream zoomed out, where a footprint has no room for its
    // rows: it should cost exactly what no footprint costs.
    for (name, kinds) in [("indicators off", &["volume"][..]), ("footprint on but zoomed out", &["volume", "footprint"][..])] {
        let mut scene = FlowScene::new(1_000, 400, kinds, TRADES, 0);
        let (ns, allocs, bytes) = measure_mut(&mut scene, |scene| {
            scene.trade();
            scene.view_frame(&cr, WIDTH, HEIGHT);
            surface.flush();
        });
        rows.borrow_mut().push((format!("{name}, a trade then a frame, 400 bars"), ns, allocs, bytes));
    }

    // A day with many large orders: one trade in forty, all of them on
    // screen.
    let busy = FlowScene::new(240, 240, &["volume", "large_orders"], TRADES, 40);
    let bubbles = busy.large_orders();
    frame("indicators off, 240 bars", &FlowScene::new(240, 240, &["volume"], TRADES, 40));
    frame(&format!("large orders, 240 bars, {bubbles} bubbles"), &busy);
    let mut visible = FlowScene::new(240, 240, &["volume", "large_orders"], TRADES, 40);
    visible.compare_against_visible();
    frame(&format!("large orders against visible, {bubbles} bubbles"), &visible);

    // The volume delta strip, every bar of a wide chart.
    frame("indicators off, 1500 bars", &FlowScene::new(3_000, 1_500, &["volume"], 40, 0));
    frame("volume delta, 1500 bars", &FlowScene::new(3_000, 1_500, &["volume", "volume_delta"], 40, 0));

    // A chart's bars arriving: the indicators worked out and the first
    // frame drawn, with large orders on and its book not come yet, and off.
    for (name, kinds) in [("off", &["volume"][..]), ("on", &["volume", "large_orders"][..])] {
        let mut scene = FlowScene::new(1_000, 160, kinds, TRADES, 40);
        scene.without_book();
        let (ns, allocs, bytes) = measure_mut(&mut scene, |scene| {
            scene.recompute();
            scene.view_frame(&cr, WIDTH, HEIGHT);
            surface.flush();
        });
        rows.borrow_mut().push((format!("first frame after the bars, large orders {name}"), ns, allocs, bytes));
    }

    let rows = rows.into_inner();
    let width = rows.iter().map(|row| row.0.len()).max().unwrap_or(0);
    println!("Frames, {WIDTH:.0}x{HEIGHT:.0}, release, best of many\n");
    println!("{:<width$}  {:>10}  {:>7}  {:>9}", "scenario", "best", "allocs", "bytes");
    for (name, ns, allocs, bytes) in rows {
        println!("{name:<width$}  {:>10}  {allocs:>7}  {bytes:>9}", took(ns));
    }
    println!();
}

// ---------------------------------------------------------------------------
// Replay
// ---------------------------------------------------------------------------

/// A feed of the example's own: no history, and a stream of trades the
/// example pushes.
struct Pushed(Mutex<Option<PrintSink>>, Vec<Print>);

struct Open;
impl Subscription for Open {}

impl Ladders for Pushed {
    fn serves(&self, _: &str) -> bool {
        true
    }
    fn history_days(&self) -> u32 {
        1
    }
    fn minutes(&self, _: &str, _: f64, _: i64, _: i64) -> Result<Vec<Minute>, ProviderError> {
        Ok(Vec::new())
    }
    fn watch(&self, _: &str, _: f64, _: LadderSink) -> Result<Box<dyn Subscription>, ProviderError> {
        Ok(Box::new(Open))
    }
    fn has_prints(&self, _: &str) -> bool {
        true
    }
    fn prints(&self, _: &str, _: f64, from: i64, to: i64) -> Result<Vec<Print>, ProviderError> {
        let (from, to) = (from.saturating_mul(1_000_000_000), to.saturating_mul(1_000_000_000));
        let start = self.1.partition_point(|print| print.ts_nanos < from);
        let end = self.1.partition_point(|print| print.ts_nanos < to);
        Ok(self.1[start..end].to_vec())
    }
    fn watch_prints(&self, _: &str, _: f64, sink: PrintSink) -> Result<Option<Box<dyn Subscription>>, ProviderError> {
        *self.0.lock().unwrap() = Some(sink);
        Ok(Some(Box::new(Open)))
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

/// How many trades the replay pushes, and how many at a time.
const REPLAYED: usize = 3_000_000;
const BATCH: usize = 500;

/// `count` synthetic trades, one in five hundred a large one, spread evenly
/// over `from..from + span`, unix seconds.
fn trades(count: usize, from: i64, span: i64) -> Vec<Print> {
    let started = from * 1_000_000_000;
    let mut seed = 0x9e37_79b9_7f4a_7c15u64;
    let mut tick = 24_000i32;
    (0..count)
        .map(|n| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            tick += (seed % 3) as i32 - 1;
            let large = seed.is_multiple_of(500);
            let size = if large { 60 + seed % 300 } else { 1 + seed % 8 };
            let side = if seed & 8 == 0 { Aggressor::Buy } else { Aggressor::Sell };
            let at = started + (n as i64) * (span * 1_000_000_000 / count as i64);
            Print::new(at, tick, size, side, false)
        })
        .collect()
}

/// The registry reading a day of three million trades from the feed, as a
/// replay of Rithmic's does, keeping it and building its book, while this
/// thread draws.
fn history(home: &std::path::Path) {
    let now = chrono::Utc::now().timestamp();
    // Today's trades, up to a minute ago, so the whole of them is one
    // request of the walk.
    let span = (now - 60).rem_euclid(86_400).max(3_600);
    let feed = Arc::new(Pushed(Mutex::new(None), trades(REPLAYED, now - 60 - span, span)));
    let store = home.join(".local/share/omacharts-history.db");
    drop(omacharts::store::Store::open_at(&store).expect("a store"));
    let large = LargeOrders::new(feed.clone(), Some(store)).expect("a feed with trades");
    let settings = Settings { regular: 50, extended: 50, combine: true };
    let mut scene = FlowScene::ending(now, 240, 240, &["volume", "large_orders"], TRADES, 0);
    scene.point_at(WIDTH, HEIGHT, 0.5, 0.45);
    let (surface, cr) = canvas();
    let idle = run_loop(&large, &scene, &cr, &surface, Duration::from_secs(3), None);
    let asked = Instant::now();
    large.want(1, "bench:ES", "bench:ES", "ES", &es(), now - 86_400, settings, false);
    let reading = AtomicBool::new(false);
    let busy = run_loop(&large, &scene, &cr, &surface, Duration::from_secs(120), Some(&reading));
    let orders = large.book("bench:ES", settings).map_or(0, |(book, _)| book.orders.len());
    let took = asked.elapsed().saturating_sub(Duration::from_millis(1_500));
    large.shutdown();
    println!(
        "History: {REPLAYED} trades of today read, kept and built on the registry's thread, the last book in {:.2}s, {orders} large orders in it\n",
        took.as_secs_f64()
    );
    report(&idle, &busy, "during the read");
}

fn report(idle: &Loop, busy: &Loop, during: &str) {
    println!("{:<34}  {:>8}  {:>8}  {:>8}  {:>8}  {:>7}", "this thread", "p50", "p99", "max", "mean", "frames");
    print_stats("frame, idle", &idle.frames);
    print_stats(&format!("frame, {during}"), &busy.frames);
    print_stats("input, idle", &idle.inputs);
    print_stats(&format!("input, {during}"), &busy.inputs);
    println!("\nbooks swapped in: idle {}, {during} {}\n", idle.swaps, busy.swaps);
}

/// Three million trades arriving live, pushed through the registry's
/// stream as fast as a thread can push them, while this thread draws.
fn replay(home: &std::path::Path) {
    let now = chrono::Utc::now().timestamp();
    let feed = Arc::new(Pushed(Mutex::new(None), Vec::new()));
    let store = home.join(".local/share/omacharts.db");
    drop(omacharts::store::Store::open_at(&store).expect("a store"));
    let large = LargeOrders::new(feed.clone(), Some(store)).expect("a feed with trades");
    let settings = Settings { regular: 50, extended: 50, combine: true };
    large.want(1, "bench:ES", "bench:ES", "ES", &es(), now - 4 * 3_600, settings, false);
    // The history read (none) and the stream open.
    let opened = Instant::now();
    while feed.0.lock().unwrap().is_none() && opened.elapsed() < Duration::from_secs(10) {
        std::thread::sleep(Duration::from_millis(5));
    }

    let mut scene = FlowScene::ending(now + 60, 240, 240, &["volume", "large_orders"], TRADES, 0);
    scene.point_at(WIDTH, HEIGHT, 0.5, 0.45);
    let (surface, cr) = canvas();
    let idle = run_loop(&large, &scene, &cr, &surface, Duration::from_secs(3), None);

    let pushing = Arc::new(AtomicBool::new(true));
    let feeder = {
        let feed = feed.clone();
        let pushing = pushing.clone();
        std::thread::spawn(move || {
            let sink = feed.0.lock().unwrap().take().expect("a stream");
            let all = trades(REPLAYED, now + 1, 4 * 3_600);
            for batch in all.chunks(BATCH) {
                sink(batch);
            }
            pushing.store(false, Ordering::Release);
        })
    };
    let busy = run_loop(&large, &scene, &cr, &surface, Duration::from_secs(60), Some(&pushing));
    let _ = feeder.join();
    let orders = large.book("bench:ES", settings).map_or(0, |(book, _)| book.orders.len());
    let took = busy.elapsed.saturating_sub(Duration::from_millis(1_500));
    large.shutdown();
    println!(
        "Live: {REPLAYED} trades pushed through the stream on another thread, the last book in {:.2}s, {orders} large orders in it\n",
        took.as_secs_f64()
    );
    report(&idle, &busy, "during the replay");
}

struct Loop {
    frames: Vec<u64>,
    inputs: Vec<u64>,
    swaps: usize,
    elapsed: Duration,
}

/// Draw frames as a window would, at most one per 4 ms, each after putting
/// in whatever books came, and answer a pointer motion between them: for
/// `most`, or until `until` goes false.
fn run_loop(
    large: &LargeOrders,
    scene: &FlowScene,
    cr: &cairo::Context,
    surface: &cairo::ImageSurface,
    most: Duration,
    until: Option<&AtomicBool>,
) -> Loop {
    let settings = Settings { regular: 50, extended: 50, combine: true };
    let (mut frames, mut inputs, mut swaps) = (Vec::new(), Vec::new(), 0);
    let started = Instant::now();
    let mut last_swap = Instant::now();
    let swaps_so_far = std::cell::Cell::new(0usize);
    // While the trades are arriving, and after the last of them for as long
    // as the registry is still handing over books: the catching up is the
    // part of a replay this thread could feel.
    let going = |last_swap: Instant| match until {
        None => true,
        Some(flag) => flag.load(Ordering::Acquire) || swaps_so_far.get() == 0 || last_swap.elapsed() < Duration::from_millis(1_500),
    };
    while started.elapsed() < most && going(last_swap) {
        let frame = Instant::now();
        let mut came = false;
        while let Ok(done) = large.done().try_recv() {
            large.finished(done);
            came = true;
        }
        if came && let Some((book, _)) = large.book("bench:ES", settings) {
            scene.swap_book(book);
            swaps += 1;
            swaps_so_far.set(swaps);
            last_swap = Instant::now();
        }
        scene.view_frame(cr, WIDTH, HEIGHT);
        surface.flush();
        frames.push(frame.elapsed().as_nanos() as u64);

        let input = Instant::now();
        std::hint::black_box(scene.motion(WIDTH, HEIGHT));
        scene.pointer_frame(cr, WIDTH, HEIGHT);
        surface.flush();
        inputs.push(input.elapsed().as_nanos() as u64);

        let spent = frame.elapsed();
        if spent < Duration::from_millis(4) {
            std::thread::sleep(Duration::from_millis(4) - spent);
        }
    }
    Loop { frames, inputs, swaps, elapsed: started.elapsed() }
}

fn print_stats(name: &str, samples: &[u64]) {
    let mut sorted = samples.to_vec();
    sorted.sort_unstable();
    let at = |q: f64| sorted.get(((sorted.len() as f64 - 1.0) * q).round() as usize).copied().unwrap_or(0);
    let mean = sorted.iter().sum::<u64>() / sorted.len().max(1) as u64;
    println!(
        "{name:<34}  {:>8}  {:>8}  {:>8}  {:>8}  {:>7}",
        took(at(0.5)),
        took(at(0.99)),
        took(*sorted.last().unwrap_or(&0)),
        took(mean),
        sorted.len()
    );
}

// ---------------------------------------------------------------------------
// Measuring
// ---------------------------------------------------------------------------

fn measure(mut work: impl FnMut()) -> (u64, u64, u64) {
    let mut scratch = ();
    measure_mut(&mut scratch, |_| work())
}

fn measure_mut<T>(on: &mut T, mut work: impl FnMut(&mut T)) -> (u64, u64, u64) {
    let warm = Instant::now();
    while warm.elapsed() < WARMUP {
        work(on);
    }
    let before = MINE.with(|mine| mine.get());
    work(on);
    let after = MINE.with(|mine| mine.get());
    let mut best = u64::MAX;
    let started = Instant::now();
    let mut runs = 0;
    while started.elapsed() < MEASURE || runs < 3 {
        let one = Instant::now();
        work(on);
        best = best.min(one.elapsed().as_nanos() as u64);
        runs += 1;
    }
    (best, after.0 - before.0, after.1 - before.1)
}

fn canvas() -> (cairo::ImageSurface, cairo::Context) {
    let surface = cairo::ImageSurface::create(cairo::Format::ARgb32, WIDTH as i32, HEIGHT as i32).expect("a surface");
    let cr = cairo::Context::new(&surface).expect("a context");
    (surface, cr)
}

fn scratch() -> PathBuf {
    let dir = std::env::temp_dir().join("omacharts-order-flow-perf");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join(".local/share")).expect("a scratch directory");
    dir
}

fn took(ns: u64) -> String {
    match ns {
        ns if ns < 10_000 => format!("{ns} ns"),
        ns if ns < 10_000_000 => format!("{:.1} us", ns as f64 / 1_000.0),
        ns => format!("{:.2} ms", ns as f64 / 1_000_000.0),
    }
}

// ---------------------------------------------------------------------------
// Counting allocations, on this thread
// ---------------------------------------------------------------------------

thread_local! {
    static MINE: std::cell::Cell<(u64, u64)> = const { std::cell::Cell::new((0, 0)) };
}

struct Counting;

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let _ = MINE.try_with(|mine| {
            let (allocs, bytes) = mine.get();
            mine.set((allocs + 1, bytes + layout.size() as u64));
        });
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let _ = MINE.try_with(|mine| {
            let (allocs, bytes) = mine.get();
            mine.set((allocs + 1, bytes + new_size as u64));
        });
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

const _: f64 = TICK;
