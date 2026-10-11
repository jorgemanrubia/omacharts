//! Volume-weighted average price, with standard-deviation bands.
//!
//! Accumulates within a reset period and starts over at the next one. The
//! bands come from the volume-weighted variance of typical price around the
//! running VWAP, which is the usual construction and the one that makes the
//! first band mean "a normal distance from fair value today".
//!
//! The envelope itself is [`super::bands`], shared with the Bollinger bands,
//! so the two look like one idea applied to different maths.

use crate::bars::Bar;

use super::bands::{Band, Bands};
use super::{LineStyle, Stroke};

use super::periods::Reset;

/// What a VWAP ships with: three bands at the usual multiples, none drawn.
///
/// Bands are off by default. A VWAP is useful on its own, and three shaded
/// envelopes appearing the moment you add one is more chart than anybody asked
/// for.
///
/// Their appearance differs by position, which is what the convention looks
/// like: the inner band is a shaded area with no outline — a width of zero,
/// rather than an outline painted in the background colour — the middle one is
/// dashed, and the outer one is a plain line.
pub fn default_bands() -> Vec<Band> {
    vec![
        Band {
            deviations: 1.0,
            enabled: false,
            color: None,
            fill: true,
            fill_alpha: None,
            stroke: Stroke::new(0.0, LineStyle::Solid),
        },
        Band {
            deviations: 1.5,
            enabled: false,
            color: None,
            fill: false,
            fill_alpha: None,
            stroke: Stroke::new(1.0, LineStyle::Dashed),
        },
        Band {
            deviations: 2.0,
            enabled: false,
            color: None,
            fill: false,
            fill_alpha: None,
            stroke: Stroke::new(1.0, LineStyle::Solid),
        },
    ]
}

/// Typical price — the midpoint the weighting is applied to.
fn typical(bar: &Bar) -> f64 {
    (bar.high + bar.low + bar.close) / 3.0
}

pub fn compute(bars: &[Bar], reset: Reset, session_origin: i64, bands: &[Band]) -> Bands {
    let mut out = Bands::blank(bars.len(), bands);
    if bars.is_empty() {
        return out;
    }

    let mut bucket = i64::MIN;
    let (mut sum_w, mut sum_wp, mut sum_wp2) = (0.0f64, 0.0f64, 0.0f64);

    for (i, bar) in bars.iter().enumerate() {
        let this_bucket = reset.bucket(bar.ts, session_origin);
        if this_bucket != bucket {
            bucket = this_bucket;
            sum_w = 0.0;
            sum_wp = 0.0;
            sum_wp2 = 0.0;
        }

        // Indexes report no volume at all. Weighting every bar equally keeps
        // the line meaningful there instead of producing nothing; with real
        // volume this has no effect.
        let weight = if bar.volume > 0.0 { bar.volume } else { 1.0 };
        let price = typical(bar);

        sum_w += weight;
        sum_wp += weight * price;
        sum_wp2 += weight * price * price;

        if sum_w <= 0.0 {
            continue;
        }
        let vwap = sum_wp / sum_w;
        // Floating point can push this a hair below zero on a flat period.
        let variance = (sum_wp2 / sum_w - vwap * vwap).max(0.0);
        out.set(i, vwap, variance.sqrt());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bar(ts: i64, price: f64, volume: f64) -> Bar {
        Bar { ts, open: price, high: price, low: price, close: price, volume }
    }

    /// Bands at the given multiples, all drawn, for tests that check values.
    fn bands(multiples: &[f64]) -> Vec<Band> {
        multiples
            .iter()
            .map(|m| Band {
                deviations: *m,
                enabled: true,
                color: None,
                fill: false,
                fill_alpha: None,
                stroke: Stroke::default(),
            })
            .collect()
    }

    const DAY: i64 = 86_400;

    #[test]
    fn vwap_of_one_price_is_that_price() {
        let bars = vec![bar(0, 10.0, 5.0), bar(60, 10.0, 7.0)];
        let out = compute(&bars, Reset::Session, 0, &bands(&[1.0]));
        assert_eq!(out.center, vec![Some(10.0), Some(10.0)]);
        // No dispersion, so the bands sit on the line.
        assert_eq!(out.bands[0].upper[1], Some(10.0));
        assert_eq!(out.bands[0].lower[1], Some(10.0));
    }

    #[test]
    fn vwap_is_weighted_by_volume() {
        // 10 on 1 lot and 20 on 3 lots averages to 17.5, not 15.
        let bars = vec![bar(0, 10.0, 1.0), bar(60, 20.0, 3.0)];
        let out = compute(&bars, Reset::Session, 0, &[]);
        assert_eq!(out.center[1], Some(17.5));
    }

    #[test]
    fn it_starts_over_at_the_period_boundary() {
        let bars = vec![
            bar(0, 10.0, 1.0),
            bar(3600, 10.0, 1.0),
            // Next day: the previous day's prices must not carry over.
            bar(DAY, 50.0, 1.0),
        ];
        let out = compute(&bars, Reset::Session, 0, &[]);
        assert_eq!(out.center[1], Some(10.0));
        assert_eq!(out.center[2], Some(50.0), "a new session starts clean");
    }

    #[test]
    fn the_session_origin_moves_the_boundary() {
        let origin = 79_200; // 18:00 ET
        // Two bars either side of midnight belong to the same session.
        let bars = vec![bar(DAY - 600, 10.0, 1.0), bar(DAY + 600, 20.0, 1.0)];
        let same_session = compute(&bars, Reset::Session, origin, &[]);
        assert_eq!(same_session.center[1], Some(15.0), "one session, so it averages");

        // With no origin they are two different days.
        let split = compute(&bars, Reset::Session, 0, &[]);
        assert_eq!(split.center[1], Some(20.0), "a fresh day starts clean");
    }

    #[test]
    fn bands_widen_with_dispersion() {
        let bars = vec![bar(0, 10.0, 1.0), bar(60, 20.0, 1.0), bar(120, 30.0, 1.0)];
        let out = compute(&bars, Reset::Session, 0, &bands(&[1.0, 2.0]));
        let vwap = out.center[2].unwrap();
        let one = out.bands[0].upper[2].unwrap();
        let two = out.bands[1].upper[2].unwrap();
        assert!(one > vwap, "a band sits above the line");
        assert!((two - vwap) > (one - vwap), "two deviations is wider than one");
        // And symmetric.
        assert!(((vwap - out.bands[0].lower[2].unwrap()) - (one - vwap)).abs() < 1e-9);
    }

    #[test]
    fn a_volumeless_instrument_still_gets_a_line() {
        // Indexes report no volume; equal weighting keeps VWAP meaningful.
        let bars = vec![bar(0, 10.0, 0.0), bar(60, 20.0, 0.0)];
        let out = compute(&bars, Reset::Session, 0, &[]);
        assert_eq!(out.center[1], Some(15.0));
    }

    #[test]
    fn variance_never_goes_negative_on_a_flat_period() {
        // Large prices with no movement are where the naive variance formula
        // goes slightly negative and the square root produces NaN.
        let bars: Vec<Bar> = (0..50).map(|i| bar(i * 60, 48_000.125, 3.0)).collect();
        let out = compute(&bars, Reset::Session, 0, &bands(&[1.0]));
        for value in out.bands[0].upper.iter().flatten() {
            assert!(value.is_finite(), "band went non-finite: {value}");
        }
    }

    #[test]
    fn empty_input_produces_empty_output() {
        let out = compute(&[], Reset::Session, 0, &bands(&[1.0]));
        assert!(out.center.is_empty());
        assert_eq!(out.bands.len(), 1);
        assert!(out.bands[0].upper.is_empty());
    }

    /// Shading a band clear is not the same as not shading it, and a chart
    /// that treated them alike would have no way back from 0%.
    #[test]
    fn clear_shading_is_still_shading_that_is_on() {
        let bars = vec![bar(0, 10.0, 1.0), bar(60, 20.0, 1.0)];
        let mut set = bands(&[1.0]);
        set[0].fill = true;
        set[0].fill_alpha = Some(0.0);

        let out = compute(&bars, Reset::Session, 0, &set);
        assert!(out.bands[0].fill, "the fill is on");
        assert_eq!(out.bands[0].fill_alpha, 0.0, "and invisible, which is a different fact");
    }

    #[test]
    fn a_fresh_vwap_draws_no_bands() {
        let defaults = default_bands();
        assert_eq!(defaults.len(), 3);
        assert_eq!(
            defaults.iter().map(|b| b.deviations).collect::<Vec<_>>(),
            vec![1.0, 1.5, 2.0]
        );
        assert!(defaults.iter().all(|b| !b.enabled), "bands start off");
        assert!(defaults.iter().all(|b| b.color.is_none()), "and follow the line's colour");
    }

    #[test]
    fn disabled_bands_are_still_computed_and_still_hold_their_place() {
        let bars = vec![bar(0, 10.0, 1.0), bar(60, 20.0, 1.0), bar(120, 30.0, 1.0)];
        let mut set = bands(&[1.0, 1.5, 2.0]);
        set[1].enabled = false;

        let out = compute(&bars, Reset::Session, 0, &set);
        assert_eq!(out.bands.len(), 3, "a hidden band keeps its slot");
        assert!(!out.bands[1].enabled);
        // Computed anyway, so switching it on needs no refetch.
        assert!(out.bands[1].upper[2].is_some());
        assert_eq!(out.bands[2].deviations, 2.0, "the outer band did not shift inward");
    }

    #[test]
    fn the_default_bands_follow_the_convention() {
        let bands = default_bands();
        // Inner: a shaded area with no outline at all, rather than one painted
        // in the background colour.
        assert!(bands[0].fill);
        assert!(bands[0].stroke.is_hidden());
        // Middle: dashed, so it reads as a marker rather than a boundary.
        assert!(!bands[1].fill);
        assert_eq!(bands[1].stroke.style, LineStyle::Dashed);
        // Outer: a plain line.
        assert!(!bands[2].fill);
        assert_eq!(bands[2].stroke.style, LineStyle::Solid);
        // And only one of them is shaded.
        assert_eq!(bands.iter().filter(|b| b.fill).count(), 1);
    }

    #[test]
    fn dash_patterns_scale_with_the_line() {
        assert!(LineStyle::Solid.dashes(1.0).is_empty());
        let thin = LineStyle::Dashed.dashes(1.0);
        let thick = LineStyle::Dashed.dashes(3.0);
        assert!(thick[0] > thin[0], "a thick dashed line needs longer dashes");
        assert!(!LineStyle::Dotted.dashes(1.0).is_empty());
    }
}
