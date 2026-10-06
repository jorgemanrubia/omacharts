//! Volume-weighted average price, with standard-deviation bands.
//!
//! Accumulates within a reset period and starts over at the next one. The
//! bands come from the volume-weighted variance of typical price around the
//! running VWAP, which is the usual construction and the one that makes the
//! first band mean "a normal distance from fair value today".

use serde::{Deserialize, Serialize};

use crate::bars::Bar;
use crate::theme::ColorChoice;

use super::{LineStyle, Stroke};

use super::periods::Reset;

/// One band's definition: how far out it sits, whether it is drawn, and what
/// colour it is.
///
/// Bands are off by default. A VWAP is useful on its own, and three shaded
/// envelopes appearing the moment you add one is more chart than anybody asked
/// for.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Band {
    /// Standard deviations from the line.
    pub deviations: f64,
    pub enabled: bool,
    /// `None` follows the indicator's own colour.
    pub color: Option<ColorChoice>,
    /// Shade the area between this band's edges.
    pub fill: bool,
    /// How much of the colour that shading gets. `None` is [`FILL_ALPHA`],
    /// which is what every band shipped with before it could be set.
    #[serde(default)]
    pub fill_alpha: Option<f64>,
    pub stroke: Stroke,
}

impl Band {
    /// The shading's opacity, anywhere from fully clear to fully solid.
    ///
    /// What is left of the guard is only what alpha means — a fraction of the
    /// colour — and it no longer defends legibility. Solid shading does cover
    /// the grid behind the band, and clear shading draws nothing; both are the
    /// chart owner's call, and `fill` is what says whether the shading is on
    /// at all.
    ///
    /// A figure that is not an alpha at all still never reaches the renderer.
    /// Something past either end is pulled back to it, and a NaN or an
    /// infinity — which is not faint shading or strong shading but no
    /// shading — is dropped for the default, because there is no end to pull
    /// it towards.
    pub fn alpha(&self) -> f64 {
        self.fill_alpha
            .filter(|wanted| wanted.is_finite())
            .unwrap_or(FILL_ALPHA)
            .clamp(MIN_FILL_ALPHA, MAX_FILL_ALPHA)
    }
}

/// What a VWAP ships with: three bands at the usual multiples, none drawn.
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

/// One band, computed.
#[derive(Clone, PartialEq, Debug)]
pub struct BandSeries {
    pub deviations: f64,
    pub enabled: bool,
    pub color: Option<ColorChoice>,
    pub fill: bool,
    pub fill_alpha: f64,
    pub stroke: Stroke,
    pub upper: Vec<Option<f64>>,
    pub lower: Vec<Option<f64>>,
}

#[derive(Clone, PartialEq, Debug, Default)]
pub struct Bands {
    pub vwap: Vec<Option<f64>>,
    /// Innermost first. Disabled bands are still here, so a band's styling
    /// follows its position rather than shifting when a neighbour is hidden.
    pub bands: Vec<BandSeries>,
}

/// How much of the colour a shaded band gets, unless it says otherwise.
///
/// A backdrop: enough to read as a region, not enough to compete with the
/// bars drawn over it.
pub const FILL_ALPHA: f64 = 0.16;

/// The faintest and the strongest a shading may be set to.
///
/// The whole of what an alpha is, and nothing narrower: how heavy a backdrop
/// should be is taste, and taste is not the engine's to decide. These are
/// named rather than written out at each end so that the slider, the CLI's
/// validation and its help text can all read the figures this clamps to
/// instead of a pair typed in beside them.
pub const MIN_FILL_ALPHA: f64 = 0.0;
pub const MAX_FILL_ALPHA: f64 = 1.0;

/// Typical price — the midpoint the weighting is applied to.
fn typical(bar: &Bar) -> f64 {
    (bar.high + bar.low + bar.close) / 3.0
}

pub fn compute(bars: &[Bar], reset: Reset, session_origin: i64, bands: &[Band]) -> Bands {
    let n = bars.len();
    let mut out = Bands {
        vwap: vec![None; n],
        bands: bands
            .iter()
            .map(|band| BandSeries {
                deviations: band.deviations,
                enabled: band.enabled,
                color: band.color.clone(),
                fill: band.fill,
                fill_alpha: band.alpha(),
                stroke: band.stroke,
                upper: vec![None; n],
                lower: vec![None; n],
            })
            .collect(),
    };
    if n == 0 {
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
        out.vwap[i] = Some(vwap);

        // Floating point can push this a hair below zero on a flat period.
        let variance = (sum_wp2 / sum_w - vwap * vwap).max(0.0);
        let deviation = variance.sqrt();
        for band in out.bands.iter_mut() {
            // Computed even when disabled: turning one on should not have to
            // wait for a refetch, and the arithmetic is already done.
            band.upper[i] = Some(vwap + deviation * band.deviations);
            band.lower[i] = Some(vwap - deviation * band.deviations);
        }
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
        assert_eq!(out.vwap, vec![Some(10.0), Some(10.0)]);
        // No dispersion, so the bands sit on the line.
        assert_eq!(out.bands[0].upper[1], Some(10.0));
        assert_eq!(out.bands[0].lower[1], Some(10.0));
    }

    #[test]
    fn vwap_is_weighted_by_volume() {
        // 10 on 1 lot and 20 on 3 lots averages to 17.5, not 15.
        let bars = vec![bar(0, 10.0, 1.0), bar(60, 20.0, 3.0)];
        let out = compute(&bars, Reset::Session, 0, &[]);
        assert_eq!(out.vwap[1], Some(17.5));
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
        assert_eq!(out.vwap[1], Some(10.0));
        assert_eq!(out.vwap[2], Some(50.0), "a new session starts clean");
    }

    #[test]
    fn the_session_origin_moves_the_boundary() {
        let origin = 79_200; // 18:00 ET
        // Two bars either side of midnight belong to the same session.
        let bars = vec![bar(DAY - 600, 10.0, 1.0), bar(DAY + 600, 20.0, 1.0)];
        let same_session = compute(&bars, Reset::Session, origin, &[]);
        assert_eq!(same_session.vwap[1], Some(15.0), "one session, so it averages");

        // With no origin they are two different days.
        let split = compute(&bars, Reset::Session, 0, &[]);
        assert_eq!(split.vwap[1], Some(20.0), "a fresh day starts clean");
    }

    #[test]
    fn bands_widen_with_dispersion() {
        let bars = vec![bar(0, 10.0, 1.0), bar(60, 20.0, 1.0), bar(120, 30.0, 1.0)];
        let out = compute(&bars, Reset::Session, 0, &bands(&[1.0, 2.0]));
        let vwap = out.vwap[2].unwrap();
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
        assert_eq!(out.vwap[1], Some(15.0));
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
        assert!(out.vwap.is_empty());
        assert_eq!(out.bands.len(), 1);
        assert!(out.bands[0].upper.is_empty());
    }

    /// Both ends belong to whoever owns the chart, so neither is pulled in.
    /// A band may be shaded clear and a band may be shaded solid; only a
    /// figure that is not an alpha at all is brought back.
    #[test]
    fn shading_keeps_whatever_opacity_it_was_given() {
        let mut band = bands(&[1.0]).remove(0);

        band.fill_alpha = None;
        assert_eq!(band.alpha(), FILL_ALPHA, "an unset band shades as it always has");

        for wanted in [0.0, 0.02, 0.6, 1.0] {
            band.fill_alpha = Some(wanted);
            assert_eq!(band.alpha(), wanted);
        }

        // Only what alpha cannot mean is pulled in.
        band.fill_alpha = Some(1.4);
        assert_eq!(band.alpha(), MAX_FILL_ALPHA);
        band.fill_alpha = Some(-0.2);
        assert_eq!(band.alpha(), MIN_FILL_ALPHA);
    }

    /// A hand-edited settings file or a caller doing its own arithmetic can
    /// hand over something that is not a figure. Whatever it is, the renderer
    /// is given an alpha.
    #[test]
    fn shading_that_is_not_a_figure_shades_as_the_default() {
        let mut band = bands(&[1.0]).remove(0);
        for nonsense in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            band.fill_alpha = Some(nonsense);
            assert_eq!(band.alpha(), FILL_ALPHA, "{nonsense} reached the chart");
        }
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

    #[test]
    fn a_zero_width_line_is_not_drawn() {
        assert!(Stroke::new(0.0, LineStyle::Solid).is_hidden());
        assert!(!Stroke::default().is_hidden());
    }
}
