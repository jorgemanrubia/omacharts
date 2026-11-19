//! A market profile and a volume profile on one chart pair up only when
//! they reset on the same period: then they are drawn from one edge, the
//! market profile's, and merge and split as one, so a session's two always
//! cover the same span. A market profile alone, or beside a volume profile
//! of another period, merges and splits on its own, as a volume profile
//! alone does.

use omacharts_engine::bars::{Bar, Timeframe};
use omacharts_engine::indicators::profile::{self, Align, Merge};
use omacharts_engine::indicators::{self, Indicator, Kind, Params, Reset};
use omacharts_engine::symbols::{Instrument, InstrumentKind};

const HOUR: i64 = 3_600;
const DAY: i64 = 86_400;
/// A Monday, at midnight UTC.
const MONDAY: i64 = 20_409 * DAY;

fn spy() -> Instrument {
    Instrument {
        symbol: "SPY".into(),
        name: "An index fund".into(),
        kind: InstrumentKind::Etf,
        suffix: None,
        currency: Some("USD".into()),
        tier: 0,
        session_origin: 0,
        overrides: Vec::new(),
        exchange: None,
        popularity: 0,
        local_name: None,
        formula: None,
    }
}

/// Four sessions of hourly bars, Monday to Thursday.
fn bars() -> Vec<Bar> {
    (0..4 * 24)
        .map(|i| {
            let mid = 500.0 + (i % 9) as f64 * 0.5 + (i / 24) as f64 * 3.0;
            Bar { ts: MONDAY + i * HOUR, open: mid, high: mid + 1.0, low: mid - 1.0, close: mid + 0.5, volume: 100.0 }
        })
        .collect()
}

fn volume_profile(id: u32, reset: Reset, edge: Align) -> Indicator {
    let mut indicator = Indicator::new(id, Kind::VolumeProfile);
    if let Params::VolumeProfile { reset: r, align, .. } = &mut indicator.params {
        *r = reset;
        *align = edge;
    }
    indicator
}

fn market_profile(id: u32, reset: Reset, edge: Align) -> Indicator {
    let mut indicator = Indicator::new(id, Kind::MarketProfile);
    if let Params::MarketProfile { reset: r, align, .. } = &mut indicator.params {
        *r = reset;
        *align = edge;
    }
    indicator
}

const HOURLY: Timeframe = Timeframe::hours(1);

fn reset_of(indicator: &Indicator) -> Reset {
    match indicator.params {
        Params::VolumeProfile { reset, .. } | Params::MarketProfile { reset, .. } => reset.effective_for(HOURLY),
        _ => panic!("a profile"),
    }
}

/// Each profile's periods, as the bars they span, drawn as the window
/// draws them: with every merge of the profiles that merge with it.
fn spans(all: &[Indicator], merges: &[Merge]) -> Vec<Vec<(usize, usize)>> {
    let bars = bars();
    all.iter()
        .map(|indicator| {
            let mut indicator = indicator.clone();
            let together = indicators::merged_together(&indicator, all, Some(HOURLY));
            let reset = reset_of(&indicator);
            let starts = profile::merged_starts(merges, &together, &spy(), reset);
            match &mut indicator.params {
                Params::VolumeProfile { merged, .. } | Params::MarketProfile { merged, .. } => *merged = starts,
                _ => unreachable!(),
            }
            indicators::compute(&indicator, &bars, 0, HOURLY, None)
                .periods()
                .expect("profiles")
                .iter()
                .map(|p| (p.first_bar, p.last_bar))
                .collect()
        })
        .collect()
}

/// Merge (or split) the profile of `id` that starts at `start`, as the
/// menu and `chart profile merge` do.
fn change(all: &[Indicator], merges: &mut Vec<Merge>, id: u32, parts: &[i64], merge: bool) {
    let me = all.iter().find(|i| i.id == id).expect("the profile");
    let together = indicators::merged_together(me, all, Some(HOURLY));
    profile::change_merges(merges, &together, &spy(), reset_of(me), parts, merge);
}

const SESSIONS: [(usize, usize); 4] = [(0, 23), (24, 47), (48, 71), (72, 95)];

#[test]
fn the_two_pair_only_on_the_same_period() {
    use Align::{Left, Right};
    let ids = |pair: Vec<&Indicator>| pair.iter().map(|i| i.id).collect::<Vec<_>>();
    for (tpo, vp) in [(Left, Left), (Left, Right), (Right, Left), (Right, Right)] {
        let all = [volume_profile(1, Reset::Session, vp), market_profile(2, Reset::Session, tpo)];
        let shown: Vec<&Indicator> = all.iter().collect();
        for me in &all {
            assert_eq!(indicators::paired_with(&me.params, &all, Some(HOURLY)).map(|i| i.id), Some(2));
            // Each keeps the edge it is set to: paired, not led.
            assert_eq!(ids(indicators::pair(me, &shown, Some(HOURLY))), vec![1, 2]);
        }
        // Another period: no pair.
        let all = [volume_profile(1, Reset::Week, vp), market_profile(2, Reset::Session, tpo)];
        let shown: Vec<&Indicator> = all.iter().collect();
        assert_eq!(indicators::paired_with(&all[0].params, &all, Some(HOURLY)), None);
        assert!(indicators::pair(&all[0], &shown, Some(HOURLY)).is_empty());
        assert!(indicators::pair(&all[1], &shown, Some(HOURLY)).is_empty());
    }
    // Two volume profiles and no market profile pair with nothing.
    let all = [volume_profile(1, Reset::Session, Right), volume_profile(2, Reset::Session, Left)];
    let shown: Vec<&Indicator> = all.iter().collect();
    assert!(indicators::pair(&all[0], &shown, Some(HOURLY)).is_empty());
    // A second market profile of the period is drawn on its own.
    let all = [
        market_profile(1, Reset::Session, Left),
        volume_profile(2, Reset::Session, Right),
        market_profile(3, Reset::Session, Right),
    ];
    let shown: Vec<&Indicator> = all.iter().collect();
    assert_eq!(ids(indicators::pair(&all[1], &shown, Some(HOURLY))), vec![1, 2]);
    assert!(indicators::pair(&all[2], &shown, Some(HOURLY)).is_empty());
}

#[test]
fn merging_either_merges_both_on_the_same_period() {
    for from in [1, 2] {
        let all = [volume_profile(1, Reset::Session, Align::Right), market_profile(2, Reset::Session, Align::Left)];
        let mut merges = Vec::new();
        // Tuesday into Monday.
        change(&all, &mut merges, from, &[MONDAY + DAY], true);
        let drawn = spans(&all, &merges);
        let joined = vec![(0, 47), SESSIONS[2], SESSIONS[3]];
        assert_eq!(drawn, vec![joined.clone(), joined], "merged from {from}");
        // Split from either, back to the sessions apart.
        for split in [1, 2] {
            let mut merges = merges.clone();
            change(&all, &mut merges, split, &[MONDAY, MONDAY + DAY], false);
            assert_eq!(spans(&all, &merges), vec![SESSIONS.to_vec(), SESSIONS.to_vec()], "split from {split}");
            assert!(merges.is_empty(), "nothing left held");
        }
    }
}

#[test]
fn a_market_profile_alone_merges_and_splits_on_its_own() {
    let all = [market_profile(2, Reset::Session, Align::Left)];
    let mut merges = Vec::new();
    change(&all, &mut merges, 2, &[MONDAY + 2 * DAY], true);
    assert_eq!(spans(&all, &merges), vec![vec![SESSIONS[0], (24, 71), SESSIONS[3]]]);
    change(&all, &mut merges, 2, &[MONDAY + DAY, MONDAY + 2 * DAY], false);
    assert_eq!(spans(&all, &merges), vec![SESSIONS.to_vec()]);
}

/// Beside a volume profile of another period, a market profile merges
/// alone, and the volume profile's merges are its own.
#[test]
fn on_different_periods_each_merges_alone() {
    let all = [volume_profile(1, Reset::Week, Align::Left), market_profile(2, Reset::Session, Align::Left)];
    let mut merges = Vec::new();
    change(&all, &mut merges, 2, &[MONDAY + DAY], true);
    let drawn = spans(&all, &merges);
    assert_eq!(drawn[1], vec![(0, 47), SESSIONS[2], SESSIONS[3]]);
    assert_eq!(drawn[0], vec![(0, 95)], "the week is as it was");
    assert_eq!(merges.len(), 1);
    assert_eq!(merges[0].indicator, 2);
}

/// The pairing follows the period: a merge made while the two shared one
/// is kept by each, so set apart they keep it, and one made apart reaches
/// the other once they share a period again.
#[test]
fn a_period_change_after_merging_takes_the_pairing_with_it() {
    let mut all = [volume_profile(1, Reset::Session, Align::Left), market_profile(2, Reset::Session, Align::Left)];
    let mut merges = Vec::new();
    change(&all, &mut merges, 1, &[MONDAY + DAY], true);

    // The market profile goes weekly: no longer paired.
    all[1] = market_profile(2, Reset::Week, Align::Left);
    let drawn = spans(&all, &merges);
    assert_eq!(drawn[0], vec![(0, 47), SESSIONS[2], SESSIONS[3]], "the volume profile keeps its merge");
    assert_eq!(drawn[1], vec![(0, 95)]);
    // Merged apart: Thursday into Wednesday on the volume profile alone.
    change(&all, &mut merges, 1, &[MONDAY + 3 * DAY], true);
    assert_eq!(spans(&all, &merges)[0], vec![(0, 47), (48, 95)]);

    // Back on sessions, the two are paired again, over the same spans.
    all[1] = market_profile(2, Reset::Session, Align::Left);
    let drawn = spans(&all, &merges);
    assert_eq!(drawn[0], vec![(0, 47), (48, 95)]);
    assert_eq!(drawn[1], drawn[0], "the market profile covers what the volume profile does");
    // And split as one.
    change(&all, &mut merges, 2, &[MONDAY + 2 * DAY, MONDAY + 3 * DAY], false);
    let drawn = spans(&all, &merges);
    assert_eq!(drawn[0], vec![(0, 47), SESSIONS[2], SESSIONS[3]]);
    assert_eq!(drawn[1], drawn[0]);
}

/// A merge is the chart's, kept with it, and a market profile's reads back
/// as a volume profile's does; an indicator stored before market profiles
/// could merge reads with none.
#[test]
fn a_market_profile_stored_before_merging_reads_with_no_merge() {
    let stored = r#"{"id": 2, "kind": "market_profile", "color": null, "visible": true,
        "params": {"kind": "market_profile", "reset": "session"}}"#;
    let indicator: Indicator = serde_json::from_str(stored).expect("reads");
    match &indicator.params {
        Params::MarketProfile { merged, .. } => assert!(merged.is_empty()),
        _ => panic!("a market profile"),
    }
    let mut written = indicator.clone();
    if let Params::MarketProfile { merged, .. } = &mut written.params {
        *merged = vec![MONDAY];
    }
    let json = serde_json::to_string(&written).expect("writes");
    assert!(!json.contains("merged"), "merges are the chart's, never the indicator's: {json}");
}

/// The first and last TPO highlight is off in a chart saved before it, and
/// what was chosen for it is kept: the switch, and each colour, a swatch
/// following the theme or a colour of somebody's own. Unchosen, a colour
/// is not written, so it keeps following the theme.
#[test]
fn a_market_profiles_first_and_last_tpo_are_kept_with_it() {
    use omacharts_engine::theme::ColorChoice;
    let stored = r#"{"id": 2, "kind": "market_profile", "color": null, "visible": true,
        "params": {"kind": "market_profile", "reset": "session"}}"#;
    let mut indicator: Indicator = serde_json::from_str(stored).expect("reads");
    match &indicator.params {
        Params::MarketProfile { first_last, first_color, last_color, .. } => {
            assert!(!first_last && first_color.is_none() && last_color.is_none());
        }
        _ => panic!("a market profile"),
    }
    let json = serde_json::to_string(&indicator).expect("writes");
    assert!(!json.contains("first_color") && !json.contains("last_color"), "{json}");

    if let Params::MarketProfile { first_last, first_color, last_color, .. } = &mut indicator.params {
        *first_last = true;
        *first_color = Some(ColorChoice::swatch("Teal"));
        *last_color = Some(ColorChoice::Fixed { hex: "#aa5e57".into() });
    }
    let json = serde_json::to_string(&indicator).expect("writes");
    let read: Indicator = serde_json::from_str(&json).expect("reads back");
    assert_eq!(read, indicator);
}
