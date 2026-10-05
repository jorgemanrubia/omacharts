//! What a chart frame costs, in milliseconds.
//!
//!     cargo run --release -p omacharts --example frame_bench
//!
//! prints one row per scenario: how many bars are on screen, how many charts
//! are on it, and what each of the two kinds of frame costs — the one a pan or
//! a zoom asks for, which draws the body, and the one a pointer motion asks
//! for, which is whatever moving a crosshair repaints.
//!
//! Release, always. A debug build of the drawing is several times slower than
//! the one people run and the ratios between the rows change, which is the
//! whole of what the table is for.
//!
//! The chart is drawn onto a cairo image surface the size of a window, by the
//! same `draw` the widget calls. Four charts means the window divided in four
//! and each quarter drawn, which is what a 2×2 chartbook does. The numbers are
//! the mean over a fixed stretch of frames after a warm-up, with the fastest
//! and the slowest single frames beside them, because a mean alone hides a
//! chart that is quick on average and drops a frame.
//!
//! What this does not measure is everything after cairo: GSK turning the frame
//! into a render node, the texture upload, the compositor's own work. Those
//! are real and they are not here. What is here is the part that grows with
//! the number of bars.

use std::time::{Duration, Instant};

use gtk::cairo;
use omacharts::ui::chart::bench::Scene;

/// A window's worth of chart.
const WIDTH: f64 = 1600.0;
const HEIGHT: f64 = 900.0;

/// Long enough that the clock's own grain does not show, short enough that the
/// whole table runs in a few seconds.
const WARMUP: Duration = Duration::from_millis(100);
const MEASURE: Duration = Duration::from_millis(500);

/// How many bars a scenario puts on screen. The first is what a chart opens
/// at; the last is the most it will draw.
const SCREENFULS: [usize; 4] = [160, 500, 1500, 3000];

fn main() {
    println!("{WIDTH:.0}x{HEIGHT:.0}, release\n");
    println!(
        "{:>6}  {:>6}  {:>22}  {:>22}",
        "bars", "charts", "pan/zoom frame (ms)", "hover frame (ms)"
    );
    for charts in [1usize, 4] {
        for bars in SCREENFULS {
            let row = measure(bars, charts);
            println!(
                "{:>6}  {:>6}  {:>22}  {:>22}",
                bars,
                charts,
                row.view.show(),
                row.pointer.show()
            );
        }
    }
}

struct Row {
    view: Timing,
    pointer: Timing,
}

/// One scenario: `bars` on screen, across `charts` charts tiled in the window.
fn measure(bars: usize, charts: usize) -> Row {
    // Twice what is on screen, so there is history to the left and the view is
    // a window onto a series rather than the whole of one.
    let mut scene = Scene::new(bars * 2, bars);
    let tiles = tiles(charts);
    // In the middle of the price plot of the first chart, which is where a
    // hand holds it, and which is a crosshair with both labels to draw.
    let (first_w, first_h) = (tiles[0].2, tiles[0].3);
    scene.point_at(first_w, first_h, 0.5, 0.45);

    let surface = cairo::ImageSurface::create(cairo::Format::ARgb32, WIDTH as i32, HEIGHT as i32)
        .expect("an image surface the size of a window");
    let cr = cairo::Context::new(&surface).expect("a cairo context");

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

    Row {
        view: time(&|| frame(&|cr, w, h| scene.view_frame(cr, w, h))),
        pointer: time(&|| frame(&|cr, w, h| scene.pointer_frame(cr, w, h))),
    }
}

/// Where each chart sits when `charts` of them share the window. One fills it;
/// four make a 2×2, which is the arrangement people actually open.
fn tiles(charts: usize) -> Vec<(f64, f64, f64, f64)> {
    let across = if charts > 1 { 2 } else { 1 };
    let down = charts.div_ceil(across);
    let (w, h) = (WIDTH / across as f64, HEIGHT / down as f64);
    (0..charts)
        .map(|at| ((at % across) as f64 * w, (at / across) as f64 * h, w, h))
        .collect()
}

struct Timing {
    mean: f64,
    best: f64,
    worst: f64,
}

impl Timing {
    fn show(&self) -> String {
        format!("{:.3}  ({:.3}–{:.3})", self.mean, self.best, self.worst)
    }
}

/// Run `frame` for a fixed stretch of time and say what one frame cost.
fn time(frame: &dyn Fn()) -> Timing {
    let warm = Instant::now();
    while warm.elapsed() < WARMUP {
        frame();
    }

    let mut frames = 0u32;
    let mut best = f64::MAX;
    let mut worst = 0.0f64;
    let started = Instant::now();
    while started.elapsed() < MEASURE {
        let one = Instant::now();
        frame();
        let took = one.elapsed().as_secs_f64() * 1000.0;
        best = best.min(took);
        worst = worst.max(took);
        frames += 1;
    }
    let total = started.elapsed().as_secs_f64() * 1000.0;
    Timing { mean: total / frames as f64, best, worst }
}
