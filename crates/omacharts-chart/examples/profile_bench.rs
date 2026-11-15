//! What an automatic volume profile costs: how many rows it is drawn with,
//! what computing it takes, and what painting those rows takes, on a busy
//! ES-like session at 1h and at 2m, drawn from ladders and estimated from
//! the bars.
//!
//! ```sh
//! cargo run --release -p omacharts-chart --example profile_bench
//! ```
//!
//! Not part of `examples/perf.rs`: that table is a regression gate built
//! from a synthetic daily series with no ladders, and these are the numbers
//! behind choosing how many rows an automatic profile gets. Run it on two
//! revisions to compare them; it uses nothing either side lacks.
//!
//! The paint is the study's own: the rows, filled in one path a strength, then the outline
//! round the rows and the point of control, onto an image surface the size
//! of a chart at scale 2. What it leaves out is everything else in a frame.

use std::time::{Duration, Instant};

use gtk::cairo;
use omacharts_engine::Bar;
use omacharts_engine::indicators::Reset;
use omacharts_engine::indicators::profile::{self, AtPrice, Method};
use omacharts_engine::ladders::{to_price, Ladder, Level, Minute, Series, Sides};
use omacharts_engine::InstrumentKind;

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
}

const TICK: f64 = 0.25;
const SESSIONS: i64 = 20;
const DAY: i64 = 86_400;
/// 17:00 Chicago in UTC seconds into the day, the session's origin.
const ORIGIN: i64 = 79_200;
/// 17:00 to 16:00 Chicago.
const SESSION_MINUTES: i64 = 23 * 60;
const RTH: std::ops::Range<i64> = (15 * 60 + 30)..(22 * 60);

/// One ES-like minute about `center`: a few to a dozen prices, heavy in the
/// cash session and thin overnight.
fn minute(rng: &mut Rng, ts: i64, center: i32, rth: bool) -> Minute {
    let width = 3 + rng.below(if rth { 10 } else { 4 }) as usize;
    let low_tick = center - width as i32 / 2;
    let per_side = if rth { 400 } else { 40 };
    let mut levels = Vec::with_capacity(width);
    let mut sides = Sides::default();
    for _ in 0..width {
        let (bid, ask) = (rng.below(per_side), rng.below(per_side));
        let level = Level { at_bid: bid, at_ask: ask, unknown: 0, bid_trades: bid as u32, ask_trades: ask as u32 };
        sides.at_bid += bid;
        sides.at_ask += ask;
        sides.bid_trades += bid;
        sides.ask_trades += ask;
        levels.push(level);
    }
    let high_tick = low_tick + width as i32 - 1;
    let pick = |rng: &mut Rng| low_tick + rng.below(width as u64) as i32;
    let (open, close) = (pick(rng), pick(rng));
    Minute {
        ts,
        open: to_price(open, TICK),
        high: to_price(high_tick, TICK),
        low: to_price(low_tick, TICK),
        close: to_price(close, TICK),
        volume: sides.volume(),
        ladder: Ladder { low_tick, levels, sides },
    }
}

/// How far each session swings either way, in ticks: a month of ES, most
/// sessions 60 to 100 points from low to high, one wild one of 200, and the
/// last the author's, 7547 to 7606, about 236 ticks.
const REACH: [i32; SESSIONS as usize] =
    [90, 140, 180, 120, 200, 160, 110, 170, 150, 130, 190, 100, 175, 145, 400, 165, 125, 185, 155, 118];

/// Twenty sessions, each a slow swing up and back by its [`REACH`].
fn sessions() -> Series {
    let mut rng = Rng(0x5eed);
    let mut series = Series::new(TICK);
    let mut center: i32 = 30_200;
    for day in 0..SESSIONS {
        let start = 20_000 * DAY + day * DAY + ORIGIN;
        let reach = REACH[day as usize];
        let base = center;
        for i in 0..SESSION_MINUTES {
            // A slow swing up and back over the session, with noise.
            let phase = (i as f64 / SESSION_MINUTES as f64) * std::f64::consts::TAU;
            let swing = (phase.sin() * reach as f64) as i32;
            center = base + swing + rng.below(9) as i32 - 4;
            let m = minute(&mut rng, start + i * 60, center, RTH.contains(&i));
            series.insert_minute(20_000 + day, m);
        }
    }
    series
}

fn bars_of(series: &Series, seconds: i64) -> Vec<Bar> {
    let (first, last) = {
        let mut minutes = series.minutes();
        let first = minutes.next().map(|m| m.ts).unwrap_or(0);
        let last = series.minutes().last().map(|m| m.ts).unwrap_or(0);
        (first, last + 60)
    };
    series
        .bars_by(first, last, |ts| ts - (ts - ORIGIN).rem_euclid(seconds))
        .into_iter()
        .map(|bar| Bar { ts: bar.ts, open: bar.open, high: bar.high, low: bar.low, close: bar.close, volume: bar.volume as f64 })
        .collect()
}

fn time<T>(runs: u32, mut f: impl FnMut() -> T) -> (Duration, T) {
    let mut best = Duration::MAX;
    let mut out = None;
    for _ in 0..runs {
        let at = Instant::now();
        let value = f();
        best = best.min(at.elapsed());
        out = Some(value);
    }
    (best, out.expect("at least one run"))
}

/// Paint every profile's rows, outline and point of control as the study
/// does, onto a 2560×1440 surface: the busiest period filling the height.
fn paint(profiles: &[profile::Profile]) -> Duration {
    let surface = cairo::ImageSurface::create(cairo::Format::ARgb32, 2560, 1440).expect("a surface");
    let cr = cairo::Context::new(&surface).expect("a context");
    cr.scale(2.0, 2.0);
    let (low, high) = profiles.iter().flat_map(|p| p.rows.iter()).fold((f64::MAX, f64::MIN), |(lo, hi), row| {
        (lo.min(row.low), hi.max(row.high))
    });
    let to_y = |price: f64| 20.0 + (high - price) / (high - low).max(f64::EPSILON) * 680.0;
    let (best, _) = time(30, || {
        for (n, p) in profiles.iter().enumerate() {
            let edge = 10.0 + n as f64 * 60.0;
            let span = 50.0;
            let (area_low, area_high) = p.value_area_bounds();
            // As the study fills them: the value area's rows in one fill,
            // the rest in another.
            for strength in [0.30, 0.14] {
                for row in &p.rows {
                    let inside = row.low >= area_low && row.high <= area_high;
                    if (if inside { 0.30 } else { 0.14 }) != strength {
                        continue;
                    }
                    let length = span * row.volume.abs() / p.max_volume;
                    let top = to_y(row.high);
                    let height = (to_y(row.low) - top).max(1.0);
                    cr.rectangle(edge, top, length, height);
                }
                cr.set_source_rgba(0.4, 0.6, 0.9, strength);
                let _ = cr.fill();
            }
            cr.set_source_rgba(0.4, 0.6, 0.9, 0.45);
            cr.set_line_width(1.0);
            cr.move_to(edge, to_y(p.rows[0].low));
            for row in &p.rows {
                let x = edge + span * row.volume.abs() / p.max_volume;
                cr.line_to(x, to_y(row.low));
                cr.line_to(x, to_y(row.high));
            }
            cr.line_to(edge, to_y(p.rows[p.rows.len() - 1].high));
            let _ = cr.stroke();
            let y = to_y(p.poc_price()).round() + 0.5;
            cr.set_source_rgba(0.9, 0.7, 0.3, 0.9);
            cr.move_to(edge, y);
            cr.line_to(edge + span, y);
            let _ = cr.stroke();
        }
    });
    best
}

fn main() {
    let series = sessions();
    println!(
        "{:<6} {:<8} {:<10} {:>9} {:>9} {:>9} {:>12} {:>12}",
        "bars", "reset", "source", "last rows", "typical", "most", "compute", "paint"
    );
    for (label, seconds) in [("1h", 3_600), ("2m", 120)] {
        let bars = bars_of(&series, seconds);
        // Up to the next bar's open: a bar's ladder is every minute in it.
        let of = |slice: &[Bar]| -> Option<Ladder> {
            let (first, last) = (slice.first()?, slice.last()?);
            Some(series.volume_at_price(first.ts, last.ts + seconds))
        };
        let at = AtPrice { tick_size: TICK, of: &of };
        for ((source, at_price), (period, reset)) in [("ladders", Some(&at)), ("estimated", None)]
            .into_iter()
            .flat_map(|source| [("session", Reset::Session), ("week", Reset::Week)].map(|reset| (source, reset)))
        {
            let (compute, profiles) = time(20, || {
                profile::compute_with(
                    &bars,
                    reset,
                    ORIGIN,
                    None,
                    0.70,
                    Some(InstrumentKind::FutureRoot),
                    &[],
                    at_price,
                    Method::Total,
                    false,
                )
            });
            let mut counts: Vec<usize> = profiles.iter().map(|p| p.rows.len()).collect();
            let last = counts.last().copied().unwrap_or(0);
            counts.sort_unstable();
            let typical = counts.get(counts.len() / 2).copied().unwrap_or(0);
            let most = counts.last().copied().unwrap_or(0);
            let painted = paint(&profiles);
            println!(
                "{:<6} {:<8} {:<10} {:>9} {:>9} {:>9} {:>10.2}ms {:>10.2}ms",
                label,
                period,
                source,
                last,
                typical,
                most,
                compute.as_secs_f64() * 1e3,
                painted.as_secs_f64() * 1e3
            );
        }
    }
}
