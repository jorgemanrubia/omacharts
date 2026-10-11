//! A centre line with an envelope either side of it.
//!
//! Two indicators here are that picture: a VWAP's bands are standard
//! deviations of typical price around the volume-weighted average, and a
//! Bollinger's are standard deviations of close around the simple average.
//! Past that one difference they are the same thing, so they are the same
//! type, written by the same arithmetic and drawn by the same code — which is
//! what makes the two read as one idea on a chart rather than as two people's
//! idea of a band.
//!
//! What is shared is the envelope, not the maths that produces it. An
//! indicator works out its own centre and its own spread, one bar at a time,
//! and hands each pair to [`Bands::set`]. Nothing here holds a window or
//! allocates per bar.

use serde::{Deserialize, Serialize};

use crate::theme::ColorChoice;

use super::Stroke;

/// One band's definition: how far out it sits, whether it is drawn, and what
/// colour it is.
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

/// A centre line and its envelope, per bar.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct Bands {
    /// The line the bands are measured from: a VWAP, a Bollinger's basis.
    pub center: Vec<Option<f64>>,
    /// Innermost first. Disabled bands are still here, so a band's styling
    /// follows its position rather than shifting when a neighbour is hidden.
    pub bands: Vec<BandSeries>,
}

impl Bands {
    /// Room for `n` bars and nothing computed yet: every bar `None`, which is
    /// what a bar the indicator never reaches has to stay.
    pub fn blank(n: usize, bands: &[Band]) -> Bands {
        Bands {
            center: vec![None; n],
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
        }
    }

    /// Write bar `i`: the centre, and each band at its own multiple of
    /// `spread`.
    ///
    /// Every band is written even when it is switched off, because turning one
    /// on should not have to wait for a refetch and the arithmetic is already
    /// done. A spread of zero is a real answer — a window that did not move
    /// has no dispersion — and it puts the bands exactly on the line.
    pub fn set(&mut self, i: usize, center: f64, spread: f64) {
        self.center[i] = Some(center);
        for band in self.bands.iter_mut() {
            band.upper[i] = Some(center + spread * band.deviations);
            band.lower[i] = Some(center - spread * band.deviations);
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::indicators::LineStyle;

    fn band(deviations: f64) -> Band {
        Band {
            deviations,
            enabled: true,
            color: None,
            fill: false,
            fill_alpha: None,
            stroke: Stroke::default(),
        }
    }

    #[test]
    fn a_blank_set_has_a_slot_per_bar_and_no_values() {
        let out = Bands::blank(3, &[band(1.0), band(2.0)]);
        assert_eq!(out.center, vec![None, None, None]);
        assert_eq!(out.bands.len(), 2);
        assert!(out.bands.iter().all(|b| b.upper.len() == 3 && b.lower.len() == 3));
        assert!(out.bands[0].upper.iter().all(Option::is_none));
    }

    #[test]
    fn a_band_sits_its_own_multiple_of_the_spread_either_side() {
        let mut out = Bands::blank(1, &[band(1.0), band(2.5)]);
        out.set(0, 100.0, 4.0);
        assert_eq!(out.center[0], Some(100.0));
        assert_eq!(out.bands[0].upper[0], Some(104.0));
        assert_eq!(out.bands[0].lower[0], Some(96.0));
        assert_eq!(out.bands[1].upper[0], Some(110.0));
        assert_eq!(out.bands[1].lower[0], Some(90.0));
    }

    /// A flat window has no dispersion, which is a real answer rather than an
    /// error: the bands collapse onto the line they are measured from.
    #[test]
    fn no_spread_puts_the_bands_on_the_line() {
        let mut out = Bands::blank(1, &[band(2.0)]);
        out.set(0, 50.0, 0.0);
        assert_eq!(out.bands[0].upper[0], Some(50.0));
        assert_eq!(out.bands[0].lower[0], Some(50.0));
    }

    #[test]
    fn a_band_that_is_switched_off_is_still_computed_and_keeps_its_slot() {
        let mut off = band(1.5);
        off.enabled = false;
        let mut out = Bands::blank(1, &[off, band(2.0)]);
        out.set(0, 10.0, 1.0);
        assert!(!out.bands[0].enabled);
        assert_eq!(out.bands[0].upper[0], Some(11.5), "computed anyway");
        assert_eq!(out.bands[1].deviations, 2.0, "the outer band did not shift inward");
    }

    /// Both ends belong to whoever owns the chart, so neither is pulled in.
    /// A band may be shaded clear and a band may be shaded solid; only a
    /// figure that is not an alpha at all is brought back.
    #[test]
    fn shading_keeps_whatever_opacity_it_was_given() {
        let mut band = band(1.0);

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
        let mut band = band(1.0);
        for nonsense in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            band.fill_alpha = Some(nonsense);
            assert_eq!(band.alpha(), FILL_ALPHA, "{nonsense} reached the chart");
        }
    }

    #[test]
    fn a_zero_width_line_is_not_drawn() {
        assert!(Stroke::new(0.0, LineStyle::Solid).is_hidden());
        assert!(!Stroke::default().is_hidden());
    }
}
