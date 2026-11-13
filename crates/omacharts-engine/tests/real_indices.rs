//! Real index history through the rules that keep junk off a chart.
//!
//! An index reports no volume, so every bar of one is a bar
//! [`drop_phantoms`](omacharts_engine::bars::drop_phantoms) and
//! [`clip_stray_wicks`](omacharts_engine::bars::clip_stray_wicks) may judge,
//! and a real panic in one — ^VIX from 23 to 65.73 and back to 38.57 on
//! 2024-08-05, a T-bill yield moving a basis point off zero — looks, to a
//! careless rule, like a print nobody made. So these are Yahoo's own daily
//! bars, as its chart endpoint served them, and every one of them has to come
//! through [`drawable`] exactly as it went in.
//!
//! Not the whole of 36 years: around the days in each that come closest to
//! being judged a stray, the 2024 ^VIX spike and the March 2020 crash among
//! them, eight bars either side. A window is judged on its own, which only
//! gives a wick less around it to be measured against, never more. The
//! closest any of them comes is under half the way to a cut.
//!
//! A rule that changes how one of these bars is drawn has to show that the
//! bar was wrong, not loosen this test.

use std::path::Path;

use omacharts_engine::{drawable, Bar, InstrumentKind, Timeframe};

/// The windows of `tests/fixtures/indices/<name>.csv`, each a run of days,
/// every bar with its date.
fn served(name: &str) -> Vec<Vec<(String, Bar)>> {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/indices").join(format!("{name}.csv"));
    let text = std::fs::read_to_string(&path).expect("fixture");
    let mut windows: Vec<Vec<(String, Bar)>> = Vec::new();
    for line in text.lines().skip_while(|line| !line.starts_with("date")).skip(1) {
        if line.starts_with('#') {
            windows.push(Vec::new());
            continue;
        }
        let fields: Vec<&str> = line.split(',').collect();
        let number = |at: usize| fields[at].parse::<f64>().expect("number");
        let Some(window) = windows.last_mut() else { panic!("a bar before any window") };
        let bar = Bar {
            ts: window.len() as i64 * 86_400,
            open: number(1),
            high: number(2),
            low: number(3),
            close: number(4),
            volume: number(5),
        };
        window.push((fields[0].to_string(), bar));
    }
    windows
}

fn assert_drawn_as_served(symbol: &str) {
    let windows = served(symbol);
    assert!(windows.len() >= 10, "{symbol}: {} windows", windows.len());
    for served in windows {
        let mut drawn: Vec<Bar> = served.iter().map(|(_, bar)| *bar).collect();
        drawable(&mut drawn, Timeframe::days(1), InstrumentKind::Index);
        let changed: Vec<String> = served
            .iter()
            .filter(|(_, bar)| drawn.binary_search_by_key(&bar.ts, |b| b.ts).map(|at| drawn[at]) != Ok(*bar))
            .map(|(day, bar)| format!("{day} {} {} {} {}", bar.open, bar.high, bar.low, bar.close))
            .collect();
        assert!(changed.is_empty(), "{symbol} drawn unlike it was served: {changed:#?}");
    }
}

#[test]
fn every_vix_bar_is_drawn_as_served() {
    assert_drawn_as_served("VIX");
}

#[test]
fn every_three_month_bill_yield_is_drawn_as_served() {
    assert_drawn_as_served("IRX");
}

#[test]
fn every_ten_year_yield_is_drawn_as_served() {
    assert_drawn_as_served("TNX");
}
