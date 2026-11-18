//! The studies of volume at price answer what the chart draws from the same
//! ladders: the developing profile, at the end of its period, is the
//! period's profile, and the cumulative delta is the ladders' delta summed.

use omacharts_engine::bars::Bar;
use omacharts_engine::indicators::profile::{compute_with, AtPrice, Method};
use omacharts_engine::indicators::Reset;
use omacharts_engine::ladders::{to_price, Ladder, Minute, Side};
use omacharts_studies::{AtPriceStudy, DevelopingProfile, VolumeDelta};

const TICK: f64 = 0.25;
/// Sessions open at 22:00 UTC.
const ORIGIN: i64 = 79_200;

/// Three sessions of minutes with ladders, deterministic.
fn minutes() -> Vec<Minute> {
    let mut state = 11u64;
    let mut next = move |below: u64| {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (state >> 33) % below
    };
    let start = 1_767_045_600 - 1_767_045_600 % 86_400 + ORIGIN;
    let mut centre = 24_000i32;
    let mut out = Vec::new();
    for minute in 0..(3 * 1_380) {
        let ts = start + minute * 60;
        centre += next(7) as i32 - 3;
        let first = centre + next(5) as i32 - 2;
        let mut bar = Minute::opening(ts, to_price(first, TICK));
        for _ in 0..1 + next(12) {
            let tick = centre + next(9) as i32 - 4;
            let side = match next(5) {
                0 => Side::Unknown,
                1 | 2 => Side::Buy,
                _ => Side::Sell,
            };
            bar.add_trade(tick, to_price(tick, TICK), 1 + next(30), side, 1);
        }
        out.push(bar);
    }
    out
}

fn as_bar(minute: &Minute) -> Bar {
    Bar {
        ts: minute.ts,
        open: minute.open,
        high: minute.high,
        low: minute.low,
        close: minute.close,
        volume: minute.volume as f64,
    }
}

#[test]
fn a_developing_profile_ends_its_period_as_the_periods_profile() {
    let minutes = minutes();
    let bars: Vec<Bar> = minutes.iter().map(as_bar).collect();
    let ladder_of = |slice: &[Bar]| -> Option<Ladder> {
        let (from, to) = (slice.first()?.ts, slice.last()?.ts);
        let mut sum = Ladder::new();
        for minute in minutes.iter().filter(|m| (from..=to).contains(&m.ts)) {
            sum.add(minute);
        }
        Some(sum)
    };
    let at = AtPrice { tick_size: TICK, of: &ladder_of, coming: &|_| false };
    let profiles = compute_with(&bars, Reset::Session, ORIGIN, None, 0.7, None, &[], Some(&at), Method::Total, false);
    assert!(profiles.len() >= 3, "{} profiles", profiles.len());

    let mut study = DevelopingProfile::new(TICK);
    let mut readings = Vec::new();
    for (i, minute) in minutes.iter().enumerate() {
        let new_period = i == 0 || Reset::Session.bucket(minute.ts, ORIGIN) != Reset::Session.bucket(minutes[i - 1].ts, ORIGIN);
        readings.push(study.step(minute, new_period));
    }

    for profile in &profiles {
        let reading = readings[profile.last_bar].expect("a profile");
        // A chart row is a tick tall and reaches from its price to the next.
        let (low, high) = profile.value_area_bounds();
        assert_eq!(reading.poc + TICK / 2.0, profile.poc_price(), "point of control of {}", profile.start_ts);
        assert_eq!(reading.val, low, "value area low of {}", profile.start_ts);
        assert_eq!(reading.vah + TICK, high, "value area high of {}", profile.start_ts);
    }
}

#[test]
fn the_cumulative_delta_is_the_ladders_delta_summed() {
    let minutes = minutes();
    let mut study = VolumeDelta::new();
    let mut sum = Ladder::new();
    for minute in &minutes {
        sum.add(minute);
        let reading = study.step(minute, false).expect("always a reading");
        assert_eq!(reading.cvd, sum.delta() as f64);
        assert_eq!(reading.delta, minute.ladder.delta() as f64);
    }
}
