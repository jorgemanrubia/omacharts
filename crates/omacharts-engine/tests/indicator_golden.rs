//! Every indicator's output, held to the bit.
//!
//! Each case below computes an indicator over a deterministic series and
//! reduces its whole output to a fingerprint of its `Debug` form, which
//! prints every `f64` in its shortest round-trip spelling — so two outputs
//! fingerprint alike only if every value is the same to the bit, signed
//! zeros and infinities included.
//!
//! The fingerprints live in `tests/fixtures/indicator_golden.tsv`. Run with
//! `INDICATOR_GOLDEN=record` to write them afresh, which is only right when
//! a change to an indicator's numbers is the point of the change.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::PathBuf;

use omacharts_engine::bars::{Bar, Timeframe};
use omacharts_engine::indicators::{self, bands::Band, cvd, Indicator, Kind, LineStyle, Params, Reset, Stroke};
use omacharts_engine::InstrumentKind;

/// FNV-1a, 64 bits: small, stable, and dependency-free.
fn fingerprint(text: &str) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in text.bytes() {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

/// A deterministic generator, so a series is the same on every machine.
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> f64 {
        self.0 = self.0.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }
}

/// A walk on a 0.25 tick, starting at `start` and `step` seconds apart.
fn walk(seed: u64, n: usize, start: i64, step: i64) -> Vec<Bar> {
    let mut rng = Lcg(seed);
    let mut close = 5000.0;
    (0..n)
        .map(|i| {
            let open = close;
            let moves = ((rng.next() - 0.5) * 16.0).round() * 0.25;
            close = open + moves;
            let high = open.max(close) + (rng.next() * 6.0).round() * 0.25;
            let low = open.min(close) - (rng.next() * 6.0).round() * 0.25;
            let volume = (rng.next() * 900.0).round() + 1.0;
            Bar { ts: start + i as i64 * step, open, high, low, close, volume }
        })
        .collect()
}

/// Prices that are not exact binary fractions, so running sums round.
fn ragged(seed: u64, n: usize, start: i64, step: i64) -> Vec<Bar> {
    let mut rng = Lcg(seed);
    let mut close = 101.37;
    (0..n)
        .map(|i| {
            let open = close;
            close = open * (1.0 + (rng.next() - 0.5) * 0.013);
            let high = open.max(close) * (1.0 + rng.next() * 0.004);
            let low = open.min(close) * (1.0 - rng.next() * 0.004);
            let volume = rng.next() * 12_345.678;
            Bar { ts: start + i as i64 * step, open, high, low, close, volume }
        })
        .collect()
}

/// Every awkward thing at once: time gaps, flat bars, zero and negative and
/// missing volume, a non-finite close, signed zeros.
fn awkward() -> Vec<Bar> {
    let mut bars = walk(7, 400, 1_772_000_000, 60);
    for (i, bar) in bars.iter_mut().enumerate() {
        if i % 37 == 0 {
            bar.ts += 3_600 * 5; // a gap
        }
        if i % 11 == 0 {
            let c = bar.close;
            *bar = Bar { open: c, high: c, low: c, close: c, ..*bar };
        }
        if i % 13 == 0 {
            bar.volume = 0.0;
        }
        if i % 29 == 0 {
            bar.volume = -5.0;
        }
        if i % 31 == 0 {
            bar.volume = f64::NAN;
        }
    }
    // Times must still run forward after the gaps.
    for i in 1..bars.len() {
        if bars[i].ts <= bars[i - 1].ts {
            bars[i].ts = bars[i - 1].ts + 60;
        }
    }
    bars[150].close = f64::NAN;
    bars[151].close = f64::INFINITY;
    bars
}

fn zeros() -> Vec<Bar> {
    (0..60)
        .map(|i| Bar {
            ts: 1_700_000_000 + i * 60,
            open: if i % 2 == 0 { 0.0 } else { -0.0 },
            high: 0.0,
            low: -0.0,
            close: if i % 3 == 0 { -0.0 } else { 0.0 },
            volume: 0.0,
        })
        .collect()
}

fn series() -> Vec<(&'static str, Vec<Bar>, Timeframe)> {
    // 2026-03-08 is the US spring-forward Sunday; minutes either side of it,
    // and of the turn of a month, a quarter and a year.
    let dst = 1_772_928_000 - 3 * 86_400;
    let year_end = 1_798_675_200 - 2 * 86_400;
    vec![
        ("empty", Vec::new(), Timeframe::minutes(1)),
        ("one", walk(1, 1, 1_772_000_000, 60), Timeframe::minutes(1)),
        ("two", walk(2, 2, 1_772_000_000, 60), Timeframe::minutes(1)),
        ("short", walk(3, 13, 1_772_000_000, 60), Timeframe::minutes(1)),
        ("m1_dst", walk(4, 9_000, dst, 60), Timeframe::minutes(1)),
        ("m5_year_end", walk(5, 2_500, year_end, 300), Timeframe::minutes(5)),
        ("m15_ragged", ragged(6, 3_000, dst, 900), Timeframe::minutes(15)),
        ("h1_ragged", ragged(8, 2_000, 1_760_000_000, 3_600), Timeframe::hours(1)),
        ("d1", walk(9, 1_500, 1_500_000_000 - 1_500_000_000 % 86_400, 86_400), Timeframe::days(1)),
        ("awkward", awkward(), Timeframe::minutes(1)),
        ("zeros", zeros(), Timeframe::minutes(1)),
    ]
}

fn band(deviations: f64, enabled: bool) -> Band {
    Band { deviations, enabled, color: None, fill: false, fill_alpha: None, stroke: Stroke::new(1.0, LineStyle::Solid) }
}

/// Every indicator, at its defaults and across the parameters that change
/// its arithmetic.
fn indicators() -> Vec<(String, Indicator)> {
    let mut out = Vec::new();
    for kind in Kind::ALL {
        out.push((format!("{}:default", kind.key()), Indicator::new(1, kind)));
    }
    let with = |kind: Kind, params: Params| {
        let mut indicator = Indicator::new(1, kind);
        indicator.params = params;
        indicator
    };
    for period in [0usize, 1, 2, 3, 5, 9, 14, 20, 50, 200] {
        out.push((format!("sma:{period}"), with(Kind::Sma, Params::MovingAverage { period })));
        out.push((format!("ema:{period}"), with(Kind::Ema, Params::MovingAverage { period })));
        out.push((
            format!("rsi:{period}"),
            with(Kind::Rsi, Params::Rsi { period, height: 0.2, overbought: 70.0, oversold: 30.0 }),
        ));
        out.push((format!("atr:{period}"), with(Kind::Atr, Params::Atr { period, height: 0.2 })));
        out.push((
            format!("bollinger:{period}"),
            with(Kind::Bollinger, Params::Bollinger { period, bands: vec![band(2.0, true)] }),
        ));
    }
    for (fast, slow, signal) in [(12, 26, 9), (3, 10, 16), (26, 12, 9), (1, 1, 1), (0, 26, 9), (5, 35, 5)] {
        out.push((
            format!("macd:{fast}-{slow}-{signal}"),
            with(
                Kind::Macd,
                Params::Macd { fast, slow, signal, height: 0.2, signal_color: None, signal_style: None },
            ),
        ));
    }
    for (period, k_smooth, d_period) in [(14, 1, 3), (14, 3, 3), (5, 3, 3), (1, 1, 1), (21, 5, 7), (0, 3, 3)] {
        out.push((
            format!("stochastic:{period}-{k_smooth}-{d_period}"),
            with(
                Kind::Stochastic,
                Params::Stochastic {
                    period,
                    k_smooth,
                    d_period,
                    height: 0.2,
                    overbought: 80.0,
                    oversold: 20.0,
                    d_color: None,
                    d_style: None,
                },
            ),
        ));
    }
    for reset in Reset::ALL {
        out.push((
            format!("vwap:{}", reset.key()),
            with(
                Kind::Vwap,
                Params::Vwap { reset, bands: vec![band(1.0, true), band(1.5, false), band(2.5, true)] },
            ),
        ));
    }
    for reset in [None, Some(Reset::Session), Some(Reset::Week), Some(Reset::Month), Some(Reset::Year)] {
        let key = reset.map_or("never", Reset::key);
        out.push((
            format!("cvd:{key}"),
            with(Kind::Cvd, Params::Cvd { reset, style: cvd::Style::Line, height: 0.2, colouring: None }),
        ));
    }
    out
}

/// The cases, name to fingerprint.
fn cases() -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let origins = [0i64, 79_200, -21_600];
    for (name, bars, timeframe) in series() {
        for (label, indicator) in indicators() {
            // Where a session starts and what is charted only reach the
            // indicators that cut periods or size rows.
            let anchored = matches!(
                indicator.kind,
                Kind::Vwap | Kind::Cvd | Kind::VolumeProfile | Kind::SessionTimes
            );
            let origins: &[i64] = if anchored { &origins } else { &origins[..1] };
            let kinds: &[Option<InstrumentKind>] = if anchored {
                &[None, Some(InstrumentKind::FutureRoot), Some(InstrumentKind::Equity)]
            } else {
                &[None]
            };
            for &origin in origins {
                for &kind in kinds {
                    let output = indicators::compute(&indicator, &bars, origin, timeframe, kind);
                    out.insert(
                        format!("{name}\t{label}\t{origin}\t{kind:?}"),
                        fingerprint(&format!("{output:?}")),
                    );
                }
            }
        }

        // The free functions other code calls directly.
        let mut text = String::new();
        for period in [0usize, 1, 2, 14, 50] {
            let _ = write!(text, "{:?}", indicators::sma(&bars, period));
            let _ = write!(text, "{:?}", indicators::ema(&bars, period));
            let _ = write!(text, "{:?}", indicators::oscillators::rsi(&bars, period));
            let _ = write!(text, "{:?}", indicators::oscillators::atr(&bars, period));
        }
        for source in indicators::vwap::Source::ALL {
            let _ = write!(text, "{:?}", indicators::vwap::anchored(&bars, source));
        }
        out.insert(format!("{name}\tfree functions"), fingerprint(&text));
    }
    out.extend(live());
    out
}

/// A bar forming print by print and then sealed, as a chart sees it live:
/// every indicator computed again over the whole series after each print,
/// and the cumulative delta through its memo, which keeps what it worked
/// out from one tick to the next.
fn live() -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let all = walk(11, 1_200, 1_772_928_000 - 86_400, 60);
    let mut rng = Lcg(99);
    let indicators = indicators();
    let memos = cvd::Memos::new();
    let mut texts: BTreeMap<String, String> = BTreeMap::new();
    for sealed in 1_100..all.len() {
        let mut bars = all[..sealed].to_vec();
        let at = all[sealed];
        let mut forming = Bar { open: at.open, high: at.open, low: at.open, close: at.open, volume: 0.0, ..at };
        bars.push(forming);
        for _print in 0..6 {
            forming.close = at.open + ((rng.next() - 0.5) * 12.0).round() * 0.25;
            forming.high = forming.high.max(forming.close);
            forming.low = forming.low.min(forming.close);
            forming.volume += (rng.next() * 40.0).round() + 1.0;
            *bars.last_mut().unwrap() = forming;
            for (label, indicator) in &indicators {
                let output = indicators::compute(indicator, &bars, 79_200, Timeframe::minutes(1), None);
                let _ = write!(texts.entry(label.clone()).or_default(), "{output:?}");
                if indicator.kind == Kind::Cvd {
                    let Params::Cvd { reset, .. } = indicator.params else { continue };
                    let delta = memos.with(1, |memo| {
                        cvd::compute(&bars, reset, 79_200, Timeframe::minutes(1), None, Some(memo))
                    });
                    let _ = write!(texts.entry(format!("{label}:memo")).or_default(), "{delta:?}");
                }
            }
        }
        // Sealed: the bar as it closed.
        *bars.last_mut().unwrap() = at;
        for (label, indicator) in &indicators {
            let output = indicators::compute(indicator, &bars, 79_200, Timeframe::minutes(1), None);
            let _ = write!(texts.entry(label.clone()).or_default(), "{output:?}");
        }
    }
    for (label, text) in texts {
        out.insert(format!("live\t{label}"), fingerprint(&text));
    }
    out
}

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/indicator_golden.tsv")
}

#[test]
fn every_indicator_computes_the_same_values_to_the_bit() {
    let now = cases();
    if std::env::var("INDICATOR_GOLDEN").as_deref() == Ok("record") {
        let mut text = String::new();
        for (case, print) in &now {
            let _ = writeln!(text, "{case}\t{print}");
        }
        std::fs::write(fixture(), text).expect("write the fixture");
        return;
    }
    let kept = std::fs::read_to_string(fixture()).expect("the fixture; record it with INDICATOR_GOLDEN=record");
    let mut was = BTreeMap::new();
    for line in kept.lines() {
        let (case, print) = line.rsplit_once('\t').expect("case<TAB>fingerprint");
        was.insert(case.to_string(), print.to_string());
    }
    let differ: Vec<&String> = now.iter().filter(|(case, print)| was.get(*case) != Some(print)).map(|(case, _)| case).collect();
    let missing: Vec<&String> = was.keys().filter(|case| !now.contains_key(*case)).collect();
    assert!(
        differ.is_empty() && missing.is_empty(),
        "{} of {} cases differ, {} missing:\n{}",
        differ.len(),
        now.len(),
        missing.len(),
        differ.iter().chain(missing.iter()).take(40).map(|c| c.as_str()).collect::<Vec<_>>().join("\n")
    );
}
