//! The developing market profile, read a bar at a time, ends each period on
//! the chart's market profile of it: the same point of control and value
//! area, to the bit, at every row height and TPO period.

use omacharts_engine::bars::{Bar, Timeframe};
use omacharts_engine::indicators::{self, market, Indicator, Kind, Output, Params, Reset};
use omacharts_studies::{DevelopingMarketProfile, Study};

/// Sessions open at 22:00 UTC.
const ORIGIN: i64 = 79_200;
const TICK: f64 = 0.25;

/// Three and a half sessions of minutes, a quarter a tick, with a gap.
fn minutes() -> Vec<Bar> {
    let mut state = 21u64;
    let mut next = move || {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (state >> 11) as f64 / (1u64 << 53) as f64
    };
    let start = 1_767_045_600 - 1_767_045_600 % 86_400 + ORIGIN;
    let mut close = 6_000.0f64;
    (0..(3 * 1_380 + 600))
        .filter(|minute| !(2_000..2_090).contains(minute))
        .map(|minute| {
            let open = close;
            close = open + ((next() - 0.5) * 10.0).round() * TICK;
            let high = open.max(close) + (next() * 3.0).round() * TICK;
            let low = open.min(close) - (next() * 3.0).round() * TICK;
            Bar { ts: start + minute as i64 * 60, open, high, low, close, volume: 1.0 + (next() * 50.0).round() }
        })
        .collect()
}

#[test]
fn a_developing_market_profile_ends_its_period_as_the_periods_profile() {
    let bars = minutes();
    for (minutes, row_ticks, value_area) in [(30, 1, 0.7), (30, 4, 0.7), (15, 2, 0.6), (60, 1, 0.8)] {
        let profiles = market::compute(&bars, Reset::Session, ORIGIN, minutes, Some(row_ticks), value_area, TICK, &[]);
        assert!(profiles.len() >= 4, "{} profiles", profiles.len());

        let mut study = DevelopingMarketProfile::new(TICK, row_ticks, minutes).value_area(value_area);
        let readings: Vec<_> = bars
            .iter()
            .enumerate()
            .map(|(i, bar)| {
                let new_period =
                    i == 0 || Reset::Session.bucket(bar.ts, ORIGIN) != Reset::Session.bucket(bars[i - 1].ts, ORIGIN);
                study.step(bar, new_period)
            })
            .collect();

        for profile in &profiles {
            let reading = readings[profile.profile.last_bar].expect("a reading");
            assert_eq!(reading, profile.reading, "{minutes}m, {row_ticks} ticks, from {}", profile.profile.start_ts);
            // And the rows the chart draws say the same.
            let (va_low, va_high) = profile.profile.value_area;
            assert_eq!(profile.profile.rows[va_low].low + TICK / 2.0, reading.val);
            assert_eq!(profile.profile.rows[va_high].high - TICK / 2.0, reading.vah);
            assert!((profile.profile.poc_price() - reading.poc).abs() < 1e-9);
        }
    }
}

#[test]
fn the_chart_draws_a_market_profile_from_its_defaults() {
    let bars = minutes();
    let indicator = Indicator::new(1, Kind::MarketProfile);
    let Output::MarketProfiles(profiles) =
        indicators::compute(&indicator, &bars, ORIGIN, Timeframe::minutes(1), None)
    else {
        panic!("a market profile");
    };
    assert!(profiles.len() >= 4);
    for profile in &profiles {
        assert_eq!(profile.starts.len(), profile.profile.rows.len() + 1);
        assert_eq!(profile.letters.len() as f64, profile.profile.total_volume);
        // Thirty-minute periods of a day: forty-eight letters at most, the
        // first an A.
        assert!(profile.letters.iter().all(|letter| (*letter as usize) < 48));
        assert!(profile.letters.contains(&0));
    }
}

#[test]
fn a_market_profile_stored_with_nothing_but_its_kind_reads_at_its_defaults() {
    let json = r#"{"id": 1, "kind": "market_profile", "visible": true, "color": null,
        "params": {"kind": "market_profile", "reset": "session"}}"#;
    let stored: Indicator = serde_json::from_str(json).expect("it reads");
    assert_eq!(stored.params, Indicator::new(1, Kind::MarketProfile).params);
    let Params::MarketProfile { minutes, row_ticks, value_area, .. } = stored.params else { unreachable!() };
    assert_eq!((minutes, row_ticks, value_area), (30, None, 0.7));
}
