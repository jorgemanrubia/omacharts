//! What the chart draws for every indicator is what `omacharts-studies`
//! answers, to the bit: the same per-bar reading, in the library's own
//! type, as anything that links the library gets.
//!
//! One test, one arm per [`Kind`], and the match is exhaustive, so a new
//! indicator does not compile here until it says how its drawn values are
//! held to the library.

use omacharts_engine::bars::{Bar, Timeframe};
use omacharts_engine::indicators::{self, Indicator, Kind, Output, Params, Reset};
use omacharts_studies::volume_profile::{spread, Profile};
use omacharts_studies::{
    over, Atr, BarDeltaEstimate, Bollinger, CvdEstimate, Ema, Macd, MacdValue, Rsi, Sma, Stochastic, StochasticValue,
    Volume, Vwap,
};

/// Sessions open at 22:00 UTC.
const ORIGIN: i64 = 79_200;

/// Three sessions and a half of minutes, a quarter a tick, with a gap, a
/// flat bar now and then and a bar with no volume.
fn minutes() -> Vec<Bar> {
    let mut state = 7u64;
    let mut next = move || {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (state >> 11) as f64 / (1u64 << 53) as f64
    };
    let start = 1_767_045_600 - 1_767_045_600 % 86_400 + ORIGIN;
    let mut close = 6_000.0f64;
    (0..(3 * 1_380 + 600))
        .filter(|minute| !(2_000..2_090).contains(minute))
        .map(|minute: i64| {
            let open = close;
            close = open + ((next() - 0.5) * 10.0).round() * 0.25;
            let high = open.max(close) + (next() * 3.0).round() * 0.25;
            let low = open.min(close) - (next() * 3.0).round() * 0.25;
            let volume = if minute % 97 == 0 { 0.0 } else { 1.0 + (next() * 50.0).round() };
            let bar = Bar { ts: start + minute * 60, open, high, low, close, volume };
            if minute % 41 == 0 { Bar { open: close, high: close, low: close, ..bar } } else { bar }
        })
        .collect()
}

fn starts(bars: &[Bar], reset: Option<Reset>) -> impl FnMut(usize) -> bool + '_ {
    move |i| {
        i == 0
            || reset.is_some_and(|reset| reset.bucket(bars[i].ts, ORIGIN) != reset.bucket(bars[i - 1].ts, ORIGIN))
    }
}

fn pane(output: Output) -> indicators::Pane {
    match output {
        Output::Pane(pane) => pane,
        other => panic!("not a pane: {other:?}"),
    }
}

/// The same value to the bit, `None` where the other is.
fn same(drawn: &[Option<f64>], library: impl Iterator<Item = Option<f64>>, what: &str) {
    let library: Vec<Option<f64>> = library.collect();
    assert_eq!(drawn.len(), library.len(), "{what}");
    for (i, (drawn, library)) in drawn.iter().zip(&library).enumerate() {
        assert_eq!(drawn.map(f64::to_bits), library.map(f64::to_bits), "{what} at bar {i}");
    }
}

#[test]
fn every_indicator_draws_the_librarys_values() {
    let bars = minutes();
    let tf = Timeframe::minutes(1);
    for kind in Kind::ALL {
        let indicator = Indicator::new(1, kind);
        let output = indicators::compute(&indicator, &bars, ORIGIN, tf, None);
        let what = kind.key();
        // No `_`: a new kind fails to compile here until it has an arm below.
        match kind {
            Kind::Sma | Kind::Ema | Kind::Vwap | Kind::Bollinger | Kind::VolumeProfile | Kind::MarketProfile
            | Kind::Volume | Kind::Cvd | Kind::VolumeDelta | Kind::Footprint | Kind::LargeOrders | Kind::Rsi
            | Kind::Macd | Kind::Atr | Kind::Stochastic | Kind::SessionTimes => {}
        }
        match (kind, &indicator.params) {
            (Kind::Sma, Params::MovingAverage { period }) => {
                let Output::Line(line) = output else { panic!("{what}") };
                same(&line, over(Sma::new(*period), &bars, |_| false).into_iter(), what);
            }
            (Kind::Ema, Params::MovingAverage { period }) => {
                let Output::Line(line) = output else { panic!("{what}") };
                same(&line, over(Ema::new(*period), &bars, |_| false).into_iter(), what);
            }
            (Kind::Rsi, Params::Rsi { period, .. }) => {
                same(&pane(output).values, over(Rsi::new(*period), &bars, |_| false).into_iter(), what);
            }
            (Kind::Atr, Params::Atr { period, .. }) => {
                same(&pane(output).values, over(Atr::new(*period), &bars, |_| false).into_iter(), what);
            }
            (Kind::Volume, _) => {
                let Output::Volume { values, .. } = output else { panic!("{what}") };
                let drawn: Vec<Option<f64>> = values.into_iter().map(Some).collect();
                same(&drawn, over(Volume::new(), &bars, |_| false).into_iter(), what);
            }
            (Kind::Macd, Params::Macd { fast, slow, signal, .. }) => {
                let pane = pane(output);
                let library: Vec<Option<MacdValue>> = over(Macd::new(*fast, *slow, *signal), &bars, |_| false);
                same(&pane.values, library.iter().map(|v| v.map(|v| v.line)), "macd line");
                same(pane.signal.as_deref().unwrap(), library.iter().map(|v| v.and_then(|v| v.signal)), "macd signal");
                same(pane.histogram.as_deref().unwrap(), library.iter().map(|v| v.and_then(|v| v.histogram)), "macd histogram");
            }
            (Kind::Stochastic, Params::Stochastic { period, k_smooth, d_period, .. }) => {
                let pane = pane(output);
                let library: Vec<Option<StochasticValue>> =
                    over(Stochastic::new(*period, *k_smooth, *d_period), &bars, |_| false);
                same(&pane.values, library.iter().map(|v| v.map(|v| v.k)), "stochastic %K");
                same(pane.signal.as_deref().unwrap(), library.iter().map(|v| v.and_then(|v| v.d)), "stochastic %D");
            }
            (Kind::Vwap | Kind::Bollinger, Params::Vwap { .. } | Params::Bollinger { .. }) => {
                let Output::Bands(bands) = output else { panic!("{what}") };
                let library = match &indicator.params {
                    Params::Vwap { reset, .. } => over(
                        Vwap::new(omacharts_studies::Source::Hlc3),
                        &bars,
                        starts(&bars, Some(reset.effective_for(tf))),
                    ),
                    Params::Bollinger { period, .. } => over(Bollinger::new(*period), &bars, |_| false),
                    _ => unreachable!(),
                };
                same(&bands.center, library.iter().map(|s| s.map(|s| s.center)), what);
                for band in &bands.bands {
                    let k = band.deviations;
                    same(&band.upper, library.iter().map(|s| s.map(|s| s.upper(k))), what);
                    same(&band.lower, library.iter().map(|s| s.map(|s| s.lower(k))), what);
                }
            }
            (Kind::Cvd, Params::Cvd { reset, .. }) => {
                // Bars alone, so estimated; from volume at price the chart
                // reads `VolumeDelta`, held to the ladders in
                // `at_price_studies.rs`.
                let drawn = pane(output).delta.expect("a running delta's bars");
                let reset = reset.map(|reset| reset.effective_for(tf));
                let library = over(CvdEstimate::new(), &bars, starts(&bars, reset));
                assert_eq!(drawn, library.into_iter().map(Option::unwrap).collect::<Vec<_>>(), "{what}");
            }
            (Kind::VolumeDelta, _) => {
                let drawn = pane(output).delta.expect("each bar's delta");
                let library = over(BarDeltaEstimate::new(), &bars, |_| false);
                assert_eq!(drawn, library.into_iter().map(Option::unwrap).collect::<Vec<_>>(), "{what}");
            }
            (Kind::VolumeProfile, Params::VolumeProfile { value_area, .. }) => {
                let Output::Profiles(profiles) = output else { panic!("{what}") };
                assert!(profiles.len() >= 3, "{} profiles", profiles.len());
                for drawn in &profiles {
                    // Where the rows sit is the chart's (a tick, a round step);
                    // what is in them, and what is read from that, the
                    // library's.
                    let n = drawn.rows.len();
                    let low = drawn.rows[0].low;
                    let step = (drawn.rows[n - 1].high - low) / n as f64;
                    let volumes = spread(&bars[drawn.first_bar..=drawn.last_bar], low, step, n);
                    for (row, library) in drawn.rows.iter().zip(&volumes) {
                        assert!((row.volume - library).abs() <= 1e-6 * library.abs().max(1.0), "{what} row at {}", row.low);
                    }
                    let library = Profile::of_rows(
                        (drawn.start_ts, drawn.end_ts),
                        (drawn.first_bar, drawn.last_bar),
                        low,
                        step,
                        drawn.rows.iter().map(|row| row.volume).collect(),
                        *value_area,
                        false,
                    )
                    .expect("a profile");
                    assert_eq!(
                        (library.poc, library.value_area, library.max_volume.to_bits(), library.total_volume.to_bits()),
                        (drawn.poc, drawn.value_area, drawn.max_volume.to_bits(), drawn.total_volume.to_bits()),
                        "{what} from {}",
                        drawn.start_ts
                    );
                }
            }
            // Its reading at each period's last bar is the developing study's,
            // to the bit, at every row height and TPO period:
            // `market_profile_study.rs`.
            (Kind::MarketProfile, _) => assert!(matches!(output, Output::MarketProfiles(_)), "{what}"),
            // Drawn from the chart's ladders as `footprint::BarFootprint`
            // folds them, and read out by `chart values` through the same
            // fold: the chart crate's footprint tests.
            (Kind::Footprint, _) => assert!(matches!(output, Output::Footprint | Output::Unmet(_)), "{what}"),
            // Drawn from an `orders::Book` the window builds with
            // `orders::Builder`, found by `Book::within`: the chart crate's
            // bubble tests.
            (Kind::LargeOrders, _) => assert!(matches!(output, Output::LargeOrders | Output::Unmet(_)), "{what}"),
            // A calendar, with no arithmetic on bars.
            (Kind::SessionTimes, _) => assert!(matches!(output, Output::Time(_)), "{what}"),
            (kind, params) => panic!("{} has params {params:?}", kind.key()),
        }
    }
}
