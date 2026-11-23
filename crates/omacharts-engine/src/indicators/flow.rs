//! What the order flow indicators share, besides the arithmetic in
//! `omacharts_studies`: how each paints what was bought and what was sold,
//! and what a large order's bubble and a footprint's rows are measured
//! against.
//!
//! Order flow reads who took liquidity, trade by trade or price by price,
//! which only a feed with volume at price says: on any other chart the
//! footprint and the large orders draw nothing but the line saying so, and
//! the volume delta is estimated from each bar's direction, as the
//! cumulative delta is.

use serde::{Deserialize, Serialize};

use crate::colouring::Colouring;
use crate::theme::ColorChoice;

/// How an order flow indicator paints buying and selling: in a colouring
/// (unset, the chart's bars', followed as they change), or in
/// its own two colours, Custom (see [`crate::colouring`]); each side as
/// solid as set, whichever.
#[derive(Clone, PartialEq, Debug, Default, Serialize, Deserialize)]
pub struct Inks {
    /// One of [`super::profile::DELTA_COLOURINGS`], Custom, or unset for
    /// the house colouring.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub colouring: Option<Colouring>,
    /// Buying's colour, while Custom.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub buy: Option<ColorChoice>,
    /// Selling's colour, while Custom.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sell: Option<ColorChoice>,
    /// How solid buying is drawn, 0 to 1, or unset for the indicator's
    /// designed opacity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub buy_alpha: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sell_alpha: Option<f64>,
}

impl Inks {
    /// Its colours, the way every study's are changed.
    pub fn colours_mut(&mut self) -> crate::colouring::ColoursMut<'_> {
        crate::colouring::ColoursMut {
            colouring: &mut self.colouring,
            labels: &super::FLOW_ROLES,
            colours: vec![&mut self.buy, &mut self.sell],
            alphas: vec![&mut self.buy_alpha, &mut self.sell_alpha],
            opacity: None,
        }
    }

    /// Buying's and selling's opacity, each its own or `designed`.
    pub fn alphas(&self, designed: f64) -> (f64, f64) {
        let held = |alpha: Option<f64>| alpha.filter(|a| a.is_finite()).map_or(designed, |a| a.clamp(0.0, 1.0));
        (held(self.buy_alpha), held(self.sell_alpha))
    }
}

/// A footprint's volume profile: beside each bar, on the side away from its
/// delta rows, everything that traded at each row's prices, in one colour
/// that says nothing of up or down. Off unless turned on.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct BarProfile {
    #[serde(default)]
    pub on: bool,
    /// Its colour, or unset for [`PROFILE_SWATCH`] of the theme, followed
    /// as the theme changes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub colour: Option<ColorChoice>,
    /// How solid it is drawn, 0 to 1, or unset for [`PROFILE_ALPHA`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alpha: Option<f64>,
    /// Whether each row's total is written in it.
    #[serde(default = "yes")]
    pub numbers: bool,
}

impl Default for BarProfile {
    fn default() -> BarProfile {
        BarProfile { on: false, colour: None, alpha: None, numbers: true }
    }
}

impl BarProfile {
    pub fn is_default(&self) -> bool {
        *self == BarProfile::default()
    }

    /// How solid it is drawn: its own, or [`PROFILE_ALPHA`].
    pub fn drawn_alpha(&self) -> f64 {
        self.alpha.filter(|a| a.is_finite()).map_or(PROFILE_ALPHA, |a| a.clamp(0.0, 1.0))
    }

    /// Its colour: its own, or the theme's [`PROFILE_SWATCH`].
    pub fn chosen(&self) -> ColorChoice {
        self.colour.clone().unwrap_or(ColorChoice::Swatch { name: PROFILE_SWATCH.to_string() })
    }
}

/// The theme swatch a footprint's profile is drawn in unless told: a hue
/// that is neither buying's nor selling's, so it reads as volume alone.
pub const PROFILE_SWATCH: &str = "Blue";
/// How solid its strongest row is unless told: under the delta's strongest,
/// so the volume beside a bar never out-shouts who took it.
pub const PROFILE_ALPHA: f64 = 0.55;

/// How solid each indicator draws buying and selling unless told: the
/// volume delta's candles as solid as the cumulative delta's, a footprint's
/// strongest row strong without hiding its number, and bubbles see-through
/// enough to show the candles under them.
pub const VOLUME_DELTA_ALPHA: f64 = 1.0;
pub const FOOTPRINT_ALPHA: f64 = 0.85;
pub const LARGE_ORDERS_ALPHA: f64 = 0.35;


/// What a large order's bubble is sized against: the largest order it is
/// compared with is the biggest bubble. A footprint's rows are measured
/// the same ways, against the biggest row: on screen unless told, which is
/// [`footprint_compare`].
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Compare {
    /// The large orders on screen. Bubbles change size as the chart scrolls.
    Visible,
    /// The large orders of the same session.
    #[default]
    Session,
    /// The large orders of the same session's regular or extended hours,
    /// which have thresholds of their own.
    Hours,
}

impl Compare {
    pub const ALL: [Compare; 3] = [Compare::Visible, Compare::Session, Compare::Hours];

    pub fn label(self) -> &'static str {
        match self {
            Compare::Visible => "Visible bars",
            Compare::Session => "Session",
            Compare::Hours => "RTH or ETH",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Compare::Visible => "visible",
            Compare::Session => "session",
            Compare::Hours => "hours",
        }
    }

    pub fn from_key(key: &str) -> Option<Compare> {
        Compare::ALL.into_iter().find(|compare| compare.key() == key)
    }

    /// Whether it is a footprint's own unless told, so not written down.
    pub fn is_footprint_default(&self) -> bool {
        *self == footprint_compare()
    }
}

/// What a footprint's rows are measured against unless told: the biggest
/// on screen, as they always were.
pub fn footprint_compare() -> Compare {
    Compare::Visible
}

/// The sizes a large order has to be bigger than, unless told: a print of
/// more than fifty contracts in regular hours, more than twenty outside them.
pub const REGULAR_THRESHOLD: u64 = 50;
pub const EXTENDED_THRESHOLD: u64 = 20;

pub(super) fn regular_threshold() -> u64 {
    REGULAR_THRESHOLD
}

pub(super) fn extended_threshold() -> u64 {
    EXTENDED_THRESHOLD
}

pub(super) fn yes() -> bool {
    true
}

pub(super) fn volume_delta_height() -> f64 {
    0.16
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inks_left_alone_read_and_write_nothing() {
        let inks: Inks = serde_json::from_str("{}").unwrap();
        assert_eq!(inks, Inks::default());
        assert_eq!(serde_json::to_string(&inks).unwrap(), "{}");
        assert_eq!(inks.alphas(0.4), (0.4, 0.4));
        let set = Inks { buy_alpha: Some(2.0), sell_alpha: Some(f64::NAN), ..Inks::default() };
        assert_eq!(set.alphas(0.4), (1.0, 0.4));
    }

    #[test]
    fn a_profile_left_alone_is_off_and_writes_nothing() {
        let read: BarProfile = serde_json::from_str("{}").unwrap();
        assert_eq!(read, BarProfile::default());
        assert!(!read.on && read.numbers);
        assert_eq!(read.drawn_alpha(), PROFILE_ALPHA);
        assert_eq!(read.chosen(), ColorChoice::Swatch { name: PROFILE_SWATCH.into() });
        let on: BarProfile = serde_json::from_str(r#"{"on":true,"numbers":false,"alpha":0.5}"#).unwrap();
        assert_eq!(serde_json::from_str::<BarProfile>(&serde_json::to_string(&on).unwrap()).unwrap(), on);
    }

    #[test]
    fn every_comparison_reads_back_from_its_key() {
        for compare in Compare::ALL {
            assert_eq!(Compare::from_key(compare.key()), Some(compare));
            let json = serde_json::to_string(&compare).unwrap();
            assert_eq!(json, format!("\"{}\"", compare.key()));
        }
    }
}
