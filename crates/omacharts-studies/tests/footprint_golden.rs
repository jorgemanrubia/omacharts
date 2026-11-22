//! A footprint's rows, held to the value.
//!
//! Deterministic bars of ticks are folded at every round step, and the
//! automatic and fixed steps are walked over a price scale that drifts and
//! jumps as a chart's does while it is scrolled and zoomed. Each case is
//! written out in full and reduced to a fingerprint; the fingerprints live in
//! `tests/fixtures/footprint_golden.tsv`. Run with `FOOTPRINT_GOLDEN=record`
//! to write them afresh, which is only right when changing the rows is the
//! point of the change.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::PathBuf;

use omacharts_studies::footprint::{Sides, Tick, auto_ticks, fixed_ticks, fold, point_of_control};

const UNTRADED: i64 = i64::MIN;

/// FNV-1a, 64 bits.
fn fingerprint(text: &str) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in text.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 33
    }
}

/// Bars of ticks, each from its own low tick, some ticks untraded, some
/// bars below nought, one empty and one of a single tick.
fn bars() -> Vec<(i32, Vec<i64>)> {
    let mut rng = Lcg(19);
    let mut out: Vec<(i32, Vec<i64>)> = (0..40)
        .map(|n| {
            let low = 24_000 + (rng.next() % 40) as i32 - 20 + if n % 9 == 0 { -24_050 } else { 0 };
            let len = 1 + (rng.next() % 30) as usize;
            let cells = (0..len)
                .map(|_| match rng.next() % 7 {
                    0 => UNTRADED,
                    1 => 0,
                    k => (rng.next() % 400) as i64 * if k % 2 == 0 { 1 } else { -1 },
                })
                .collect();
            (low, cells)
        })
        .collect();
    out.push((10, Vec::new()));
    out.push((-1, vec![7]));
    out
}

/// A bar's ticks with each side: the delta `cells` says, over a little
/// traded both ways, and now and then some on neither side.
fn sided(cells: &[i64]) -> Vec<Sides> {
    cells
        .iter()
        .enumerate()
        .map(|(i, delta)| match *delta {
            UNTRADED => Sides::default(),
            delta => {
                let both = (i % 5) as u64 + 1;
                Sides {
                    at_bid: (-delta).max(0) as u64 + both,
                    at_ask: delta.max(0) as u64 + both,
                    unknown: u64::from(i % 3 == 0),
                    trades: (i % 4) as u64 + 1,
                }
            }
        })
        .collect()
}

fn cases() -> BTreeMap<String, String> {
    let mut texts: BTreeMap<String, String> = BTreeMap::new();
    for step in [1, 2, 4, 5, 10, 20, 25, 50, 3, 7] {
        let text = texts.entry(format!("fold:{step}")).or_default();
        for (low, cells) in bars() {
            let ticks: Vec<Tick> = sided(&cells).iter().map(Sides::tick).collect();
            fold(low, &ticks, step, |at, tick| {
                let _ = write!(text, "{at}:{} ", tick.delta);
            });
            text.push('\n');
        }
    }
    // Each row's sides and total, and each bar's point of control: what
    // the profile beside a footprint draws and what it is read out as.
    for step in [1, 2, 4, 5, 10, 3, 7] {
        let text = texts.entry(format!("totals:{step}")).or_default();
        for (low, cells) in bars() {
            let sides = sided(&cells);
            fold(low, &sides, step, |at, row: Sides| {
                let _ = write!(text, "{at}:{}/{}/{}/{}={} ", row.at_bid, row.at_ask, row.unknown, row.trades, row.total());
            });
            let poc = point_of_control(low, &sides, step, Sides::total);
            let _ = writeln!(text, "poc {poc:?}");
        }
    }
    // A scale breathing slowly, then jumping: what an automatic row holds
    // at each frame, carried from one frame to the next as the chart does.
    let mut held = None;
    let text = texts.entry("auto:drift".into()).or_default();
    let mut tick_px = 24.0;
    for frame in 0..400 {
        tick_px *= if frame < 200 { 0.985 } else { 1.012 };
        if frame == 300 {
            tick_px *= 4.0;
        }
        let ticks = auto_ticks(tick_px, held);
        held = Some(ticks);
        let _ = writeln!(text, "{tick_px:?} {ticks}");
    }
    let text = texts.entry("auto:fresh".into()).or_default();
    for tick_px in [0.0, -1.0, f64::NAN, 0.01, 0.3, 1.0, 3.2, 4.0, 7.9, 8.0, 15.9, 16.0, 120.0] {
        let _ = writeln!(text, "{tick_px:?} {}", auto_ticks(tick_px, None));
    }
    let text = texts.entry("fixed".into()).or_default();
    for chosen in [0, 1, 2, 4, 5, 10, 13, 5000, 9999] {
        for tick_px in [0.0, 0.05, 0.25, 1.0, 3.0, 40.0] {
            let _ = write!(text, "{} ", fixed_ticks(chosen, tick_px, 3.0));
        }
        text.push('\n');
    }
    texts
}

#[test]
fn footprint_rows_hold() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/footprint_golden.tsv");
    let live: BTreeMap<String, String> = cases().into_iter().map(|(name, text)| (name, fingerprint(&text))).collect();
    if std::env::var("FOOTPRINT_GOLDEN").as_deref() == Ok("record") {
        let body: String = live.iter().map(|(name, print)| format!("{name}\t{print}\n")).collect();
        std::fs::write(&path, body).unwrap();
        return;
    }
    let stored = std::fs::read_to_string(&path).expect("the fixture: record it with FOOTPRINT_GOLDEN=record");
    let stored: BTreeMap<String, String> = stored
        .lines()
        .filter_map(|line| line.split_once('\t'))
        .map(|(name, print)| (name.to_string(), print.to_string()))
        .collect();
    let wrong: Vec<&String> = live.keys().filter(|name| stored.get(*name) != live.get(*name)).collect();
    assert!(wrong.is_empty() && stored.len() == live.len(), "changed: {wrong:?}");
}

/// The rows drawn and the rows read out are one fold: the same rows, each
/// with the same delta and total, at every step.
#[test]
fn the_rows_drawn_are_the_rows_read_out() {
    for step in [1, 2, 3, 4, 5, 7, 10, 25] {
        for (low, cells) in bars() {
            let sides = sided(&cells);
            let ticks: Vec<Tick> = sides.iter().map(Sides::tick).collect();
            let (mut drawn, mut read) = (Vec::new(), Vec::new());
            fold(low, &ticks, step, |at, tick| drawn.push((at, tick.delta, tick.total)));
            fold(low, &sides, step, |at, row: Sides| read.push((at, row.delta(), row.total())));
            assert_eq!(drawn, read, "{step} ticks a row from {low}");
        }
    }
}

/// What the fingerprints stand for, spelled out where it can be read: the
/// automatic step never flickers, a frame to the next, while the scale only
/// breathes.
#[test]
fn an_automatic_step_holds_while_the_scale_breathes() {
    let mut held = None;
    let mut changes = 0;
    let mut tick_px: f64 = 6.0;
    for frame in 0..200 {
        // Up and down by a few percent, as an autoscaled chart does while
        // it is scrolled.
        tick_px *= if (frame / 5) % 2 == 0 { 1.03 } else { 0.97 };
        let ticks = auto_ticks(tick_px, held);
        if held.is_some_and(|held| held != ticks) {
            changes += 1;
        }
        held = Some(ticks);
    }
    assert_eq!(changes, 0);
}
