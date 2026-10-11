//! The numbers a performance regression would show up in.
//!
//!     cargo run --release --example perf
//!     cargo run --release --example perf -- --tsv
//!
//! Three areas, because three things are what this app is for: how long it
//! takes to be looking at a chart, what a frame costs once you are, and how
//! long the app takes to answer a hand or a keyboard. A row per scenario, and
//! three numbers on each — the cheapest run, and what one run allocated.
//!
//! Offline and deterministic, every row of it. A synthetic series stands in
//! for a market, `HOME` and `XDG_DATA_HOME` are pointed at a scratch
//! directory, and nothing here opens a socket: a benchmark that fetched real
//! bars would be measuring Yahoo's afternoon, and one that read the real
//! profile would measure whatever happens to be in it.
//!
//! Release, always, for the reason `frame_bench` gives: a debug build of the
//! drawing is several times slower than the one people run.
//!
//! `bin/perf-compare` is what reads this, and `--tsv` is what it reads. It
//! runs the whole table twice — once at the revision a branch came off, once
//! at the branch — and puts the two beside each other. See that script for
//! why the comparison is shaped that way and which half of the table is worth
//! failing a build over.
//!
//! What is not here, and would need a window:
//!
//! - **Everything after cairo.** GSK's render node, the texture upload, the
//!   compositor's own work. `frame_bench` says the same thing: what is here is
//!   the part that grows with the number of bars.
//! - **The constant floor under a launch.** Bringing up GTK and a renderer
//!   costs more than all of the work below put together — 191ms for the GL
//!   renderer against 8ms of our own, measured in `src/main.rs` — and none of
//!   it is ours to make faster. Timing it on a runner with no GPU would be
//!   measuring the runner.
//! - **How many times a frame is asked for.** A change that queues a second
//!   redraw per event doubles the work while every individual frame stays
//!   fast, and `queue_draw` is a method on a widget. Counting those needs a
//!   real window and belongs in a local run, not here.

use std::alloc::{GlobalAlloc, Layout, System};
use std::hint::black_box;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use gtk::cairo;
use omacharts::store::Store;
use omacharts::theming::Theming;
use omacharts::ui::chart::bench::{self, Scene};
use omacharts_engine::indicators::{self, Kind};
use omacharts_engine::{Bar, Indicator, SearchIndex, Timeframe};

/// A window's worth of chart, the size `frame_bench` uses, so the frame rows
/// here and the table there are about the same picture.
const WIDTH: f64 = 1600.0;
const HEIGHT: f64 = 900.0;

/// How many bars a frame scenario puts on screen: what a chart opens at, and
/// the most it will draw.
const SCREENFULS: [usize; 4] = [160, 500, 1500, 3000];

/// Long enough that the clock's own grain does not show and a lazily built
/// table has already been built, short enough that the whole run is seconds.
///
/// The measuring window is what decides how many runs the cheapest of them is
/// the cheapest of, and the expensive rows are the ones that get the fewest:
/// ten milliseconds a frame is forty runs here and a dozen on a slow runner.
/// Which is why this is longer than it needs to be for the quick rows — the
/// whole table is twenty seconds, against the several minutes the two release
/// builds around it take, so there is nothing to win by trimming it.
const WARMUP: Duration = Duration::from_millis(80);
const MEASURE: Duration = Duration::from_millis(400);

/// The series a chart opens from the cache, and the one the startup rows read.
const CACHED: &str = "bench:SPY";

fn main() {
    let tsv = std::env::args().any(|arg| arg == "--tsv");

    let home = scratch();
    // Both of them. The store reads `XDG_DATA_HOME` and the theme reads
    // `HOME`, so setting one leaves the other pointed at somebody's real
    // profile — which this would then measure, and could write to.
    //
    // SAFETY: before GTK, before any thread, and nothing has read either yet.
    unsafe {
        std::env::set_var("HOME", &home);
        std::env::set_var("XDG_DATA_HOME", home.join(".local/share"));
    }

    let mut rows = Vec::new();
    startup(&home, &mut rows);
    drawing(&mut rows);
    interactivity(&home, &mut rows);
    let _ = std::fs::remove_dir_all(&home);

    match tsv {
        true => print_tsv(&rows),
        false => print_table(&rows),
    }
}

// ---------------------------------------------------------------------------
// Startup
// ---------------------------------------------------------------------------

/// The work between typing `omacharts` and having a chart to look at.
///
/// Which is the definition this settles on, and it is worth saying why.
/// "Process start to window mapped" and "to the first frame" are both
/// dominated by GTK and the renderer — a constant this app pays and cannot
/// change — so measuring either would mostly track the machine. What is left
/// is every piece of work the app does before a chart is on screen, and that
/// is what these rows are, one per piece. A regression names itself.
///
/// It is the already-installed launch rather than the first one ever: the
/// database exists, the watchlists are seeded, an arrangement is stored and
/// there are bars in the cache. That is the launch somebody does every day.
fn startup(home: &Path, rows: &mut Vec<Row>) {
    let path = home.join("omacharts.db");
    let store = fixture(&path);
    drop(store);

    rows.push(row("startup", "open the database", || {
        black_box(Store::open_at(&path).expect("a database"));
    }));

    let store = Store::open_at(&path).expect("a database");
    rows.push(row("startup", "load the theme", || {
        black_box(Theming::load(&store));
    }));
    rows.push(row("startup", "restore the arrangement", || {
        black_box(omacharts::cli::charts::Workspace::load(&store));
    }));
    rows.push(row("startup", "read 3000 bars out of the cache", || {
        black_box(store.load_bars(CACHED, Timeframe::days(1)));
    }));

    rows.push(row("startup", "index the curated instruments", || {
        black_box(omacharts::inventory::Inventory::curated());
    }));
    // On a thread, behind the first frame, and still the biggest piece of
    // work a launch does: eleven thousand listings parsed and indexed. It is
    // here because "behind the first frame" is not "free" — until it lands,
    // a symbol somebody stored cannot be resolved.
    rows.push(row("startup", "index every listing", || {
        black_box(omacharts::inventory::everything());
    }));

    let bars = bench::series(3_000);
    let indicators: Vec<Indicator> = [Kind::Volume, Kind::Sma, Kind::Ema, Kind::Rsi]
        .into_iter()
        .enumerate()
        .map(|(at, kind)| Indicator::new(at as u32 + 1, kind))
        .collect();
    rows.push(row("startup", "first indicator pass over 3000 bars", || {
        for indicator in &indicators {
            black_box(indicators::compute(indicator, &bars, 0, Timeframe::days(1), None));
        }
    }));

    let scene = Scene::new(320, 160);
    let (surface, cr) = canvas();
    rows.push(row("startup", "first frame, 160 bars", || {
        scene.view_frame(&cr, WIDTH, HEIGHT);
        surface.flush();
    }));
}

/// A database that looks like somebody's, rather than an empty one.
///
/// The default watchlists, three chartbooks of four charts each, and a series
/// in the price cache. An empty database opens faster than anybody's real one
/// and would make every row below it look better than it is.
fn fixture(path: &Path) -> Store {
    let store = Store::open_at(path).expect("a database");
    store.seed_watchlist_if_empty(omacharts::ui::watchlist::DEFAULTS);
    store.set_setting("workspace", &arrangement());
    store.write_bars(CACHED, Timeframe::days(1), &bench::series(3_000));
    store
}

/// Three chartbooks of four charts, as the window writes them.
///
/// Hand-written rather than built through `Workspace`, because the shape this
/// has to be is the shape the window stores — and a fixture that went through
/// the reader would agree with the reader by construction.
fn arrangement() -> String {
    let books: Vec<String> = ["Macro", "Semis", "Energy"]
        .into_iter()
        .enumerate()
        .map(|(at, name)| {
            let base = at as u32 * 4 + 1;
            let panes: Vec<String> = ["SPY", "QQQ", "NVDA", "CL"]
                .into_iter()
                .enumerate()
                .map(|(n, symbol)| {
                    format!(
                        r#"{{"id":{},"symbol":"{symbol}","suffix":null,"timeframe":"1D",
                            "indicators":[{{"id":1,"kind":"volume"}},
                                          {{"id":2,"kind":"sma","params":{{"period":200}}}}],
                            "bar_style":"candle","session":"extended","show_grid":true,
                            "linked":1,"shows_drawings":true,"sends_drawings":true,
                            "drawing_uid":"","drawings":[],"auto_scale":true}}"#,
                        base + n as u32
                    )
                })
                .collect();
            format!(
                r#"{{"name":"{name}","focused":{base},
                     "layout":{{"split":{{"horizontal":true,"ratio":0.5,
                       "first":{{"split":{{"horizontal":false,"ratio":0.5,
                         "first":{{"leaf":{}}},"second":{{"leaf":{}}}}}}},
                       "second":{{"split":{{"horizontal":false,"ratio":0.5,
                         "first":{{"leaf":{}}},"second":{{"leaf":{}}}}}}}}}}},
                     "panes":[{}],"watchlist":1,"sidebar_shown":true,
                     "sidebar_width":280,"maximized":null}}"#,
                base,
                base + 1,
                base + 2,
                base + 3,
                panes.join(",")
            )
        })
        .collect();
    format!(r#"{{"books":[{}],"active":0}}"#, books.join(","))
}

// ---------------------------------------------------------------------------
// Drawing
// ---------------------------------------------------------------------------

/// What a frame costs: the body a pan or a zoom repaints, and the crosshair
/// layer a pointer motion repaints.
///
/// The same scenarios `frame_bench` tables, drawn through the same `Scene`, so
/// the two cannot drift apart. The difference is what comes out: a mean over
/// half a second there, for somebody reading it; the cheapest run and the
/// allocations here, for a machine comparing two revisions.
fn drawing(rows: &mut Vec<Row>) {
    for charts in [1usize, 4] {
        for bars in SCREENFULS {
            let mut scene = Scene::new(bars * 2, bars);
            let tiles = tiles(charts);
            scene.point_at(tiles[0].2, tiles[0].3, 0.5, 0.45);
            let (surface, cr) = canvas();

            let frame = |paint: &dyn Fn(&cairo::Context, f64, f64)| {
                for (x, y, w, h) in &tiles {
                    cr.save().ok();
                    cr.rectangle(*x, *y, *w, *h);
                    cr.clip();
                    cr.translate(*x, *y);
                    paint(&cr, *w, *h);
                    cr.restore().ok();
                }
                surface.flush();
            };

            rows.push(row("draw", &format!("pan/zoom frame, {bars} bars x{charts}"), || {
                frame(&|cr, w, h| scene.view_frame(cr, w, h));
            }));
            rows.push(row("draw", &format!("crosshair frame, {bars} bars x{charts}"), || {
                frame(&|cr, w, h| scene.pointer_frame(cr, w, h));
            }));
        }
    }
}

/// Where each chart sits when `charts` of them share the window. One fills it;
/// four make a 2x2, which is the arrangement people actually open.
fn tiles(charts: usize) -> Vec<(f64, f64, f64, f64)> {
    let across = if charts > 1 { 2 } else { 1 };
    let down = charts.div_ceil(across);
    let (w, h) = (WIDTH / across as f64, HEIGHT / down as f64);
    (0..charts)
        .map(|at| ((at % across) as f64 * w, (at / across) as f64 * h, w, h))
        .collect()
}

// ---------------------------------------------------------------------------
// Interactivity
// ---------------------------------------------------------------------------

/// How long the app takes to answer something somebody did.
///
/// A hand on the chart, a character typed into the search box, a tick arriving
/// from the provider, a command typed at a terminal. Each is work that runs on
/// the main loop between an event and the next frame, so each is a place where
/// the app stops feeling immediate.
fn interactivity(home: &Path, rows: &mut Vec<Row>) {
    // A hand on the chart. The hit test walks every drawing, so the number of
    // them is the axis that matters, not the number of bars.
    for marks in [0usize, 50, 200] {
        let mut scene = Scene::new(3_000, 1_500);
        scene.point_at(WIDTH, HEIGHT, 0.5, 0.45);
        scene.mark_up(marks);
        rows.push(row("interact", &format!("pointer motion, {marks} drawings"), || {
            black_box(scene.motion(WIDTH, HEIGHT));
        }));
    }

    // A character typed into the search box, against the index the window has
    // once the long tail has landed. One character is the worst case: it
    // matches the most and narrows the least.
    let index: SearchIndex = omacharts::inventory::everything();
    for query in ["n", "nvi", "semiconductor"] {
        rows.push(row("interact", &format!("search {:?}", query), || {
            black_box(index.search(query, 10));
        }));
    }

    // A tick arriving. Per frame rather than per tick — the window coalesces
    // what waited — and the cost is the tail in place and the indicators
    // recomputed over the whole series.
    for bars in [2_000usize, 8_000] {
        let mut scene = Scene::new(bars, 160);
        let last = scene.last_bar().expect("a series");
        let mut close = last.close;
        rows.push(row("interact", &format!("tick into a chart of {bars} bars"), || {
            close += 0.01;
            scene.tick(&[Bar { close, ..last }]);
        }));
    }

    // A command. With a window open these run on its main loop, so what a
    // command costs is what the window stops doing while it runs. The bar
    // widget sends one every couple of minutes without anybody asking.
    let store = Store::open_at(&home.join("omacharts.db")).expect("a database");
    for args in [
        ["omacharts", "watchlist", "list"].as_slice(),
        ["omacharts", "watchlist", "show"].as_slice(),
        ["omacharts", "symbol", "search", "nvidia"].as_slice(),
    ] {
        let owned: Vec<String> = args.iter().map(|arg| arg.to_string()).collect();
        rows.push(row("interact", &format!("command `{}`", args[1..].join(" ")), || {
            let outcome = omacharts::cli::run(&owned, &store, None, &omacharts::cli::Here);
            assert_eq!(outcome.code, 0, "{}", outcome.err);
        }));
    }
}

// ---------------------------------------------------------------------------
// Measuring
// ---------------------------------------------------------------------------

/// One scenario's three numbers.
struct Row {
    area: &'static str,
    scenario: String,
    /// The cheapest run, in nanoseconds.
    ns: u64,
    allocs: u64,
    bytes: u64,
}

/// Measure `work` and make a row of it.
///
/// The cheapest run rather than the mean, and this is the whole reason the
/// table is trustworthy on a shared runner: another tenant on the core can
/// only ever make a sample slower, never faster, so the minimum over a few
/// hundred runs converges on what the work costs while the mean converges on
/// how busy the machine was.
fn row(area: &'static str, scenario: &str, mut work: impl FnMut()) -> Row {
    let warm = Instant::now();
    let mut runs = 0u32;
    while warm.elapsed() < WARMUP || runs < 2 {
        work();
        runs += 1;
    }

    // Counted around one run, after the warm-up, so a table built on first use
    // or a `Vec` that has grown to its working size is not charged to the
    // scenario. Exact, and the same on every machine.
    let (allocs, bytes) = (ALLOCS.load(Ordering::Relaxed), BYTES.load(Ordering::Relaxed));
    work();
    let allocs = ALLOCS.load(Ordering::Relaxed) - allocs;
    let bytes = BYTES.load(Ordering::Relaxed) - bytes;

    let mut best = u64::MAX;
    let started = Instant::now();
    let mut runs = 0u32;
    while started.elapsed() < MEASURE || runs < 3 {
        let one = Instant::now();
        work();
        best = best.min(one.elapsed().as_nanos() as u64);
        runs += 1;
    }
    Row { area, scenario: scenario.to_string(), ns: best, allocs, bytes }
}

/// An image surface the size of a window, and a context on it.
fn canvas() -> (cairo::ImageSurface, cairo::Context) {
    let surface = cairo::ImageSurface::create(cairo::Format::ARgb32, WIDTH as i32, HEIGHT as i32)
        .expect("an image surface the size of a window");
    let cr = cairo::Context::new(&surface).expect("a cairo context");
    (surface, cr)
}

/// A directory of our own to be `HOME` in, at a fixed path.
///
/// Fixed rather than unique so that two runs of this table — which is what a
/// comparison is — use paths of the same length. A longer temp directory name
/// is a longer string allocated, and the allocation counts are compared to the
/// byte.
fn scratch() -> PathBuf {
    let dir = std::env::temp_dir().join("omacharts-perf");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join(".local/share")).expect("a scratch directory");
    dir
}

// ---------------------------------------------------------------------------
// Reporting
// ---------------------------------------------------------------------------

/// What `bin/perf-compare` reads: one row per line, four fields, no header.
fn print_tsv(rows: &[Row]) {
    for row in rows {
        println!("{}\t{}\t{}\t{}\t{}", row.area, row.scenario, row.ns, row.allocs, row.bytes);
    }
}

fn print_table(rows: &[Row]) {
    let width = rows.iter().map(|row| row.scenario.len()).max().unwrap_or(0);
    println!("{WIDTH:.0}x{HEIGHT:.0}, release, best of many\n");
    println!(
        "{:<9}  {:<width$}  {:>12}  {:>9}  {:>12}",
        "area", "scenario", "best", "allocs", "bytes"
    );
    for row in rows {
        println!(
            "{:<9}  {:<width$}  {:>12}  {:>9}  {:>12}",
            row.area,
            row.scenario,
            took(row.ns),
            row.allocs,
            row.bytes
        );
    }
}

/// A duration in whichever unit reads as a number rather than as a column of
/// zeroes.
fn took(ns: u64) -> String {
    match ns {
        ns if ns < 10_000 => format!("{ns} ns"),
        ns if ns < 10_000_000 => format!("{:.1} us", ns as f64 / 1_000.0),
        ns => format!("{:.2} ms", ns as f64 / 1_000_000.0),
    }
}

// ---------------------------------------------------------------------------
// Counting allocations
// ---------------------------------------------------------------------------

static ALLOCS: AtomicU64 = AtomicU64::new(0);
static BYTES: AtomicU64 = AtomicU64::new(0);

/// The system allocator, counting as it goes.
///
/// This is the half of the table a shared CI runner cannot move. For a
/// deterministic workload the counts come out identical on every machine and
/// every run, so a change in one is a change in the code and nothing else —
/// which is why `bin/perf-compare` fails a build on these and not on the
/// clock. They also catch the shapes that make this app slow and that a
/// wall-clock number hides in the noise: an allocation inside a per-bar loop,
/// a map rebuilt in a draw callback, a `Vec` collected once per frame where
/// once per redraw would do.
///
/// What it does not see is cairo's own allocations: the drawing calls into C,
/// which has a malloc of its own. So these counts are about the code in this
/// repository, which is the code a pull request changes.
struct Counting;

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCS.fetch_add(1, Ordering::Relaxed);
        BYTES.fetch_add(layout.size() as u64, Ordering::Relaxed);
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        ALLOCS.fetch_add(1, Ordering::Relaxed);
        BYTES.fetch_add(layout.size() as u64, Ordering::Relaxed);
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }

    /// Counted as an allocation of the new size, which is what it is: a
    /// growing `Vec` that reallocates eight times has done eight allocations,
    /// and a loop that grows one is exactly the shape worth catching.
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        ALLOCS.fetch_add(1, Ordering::Relaxed);
        BYTES.fetch_add(new_size as u64, Ordering::Relaxed);
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static COUNTED: Counting = Counting;
