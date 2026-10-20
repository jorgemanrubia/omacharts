//! What a new chart opens with.
//!
//! Set in the preferences, and nowhere else: changing one chart changes that
//! chart. A split is the exception, because it copies the chart it divides —
//! two of what you were looking at is the point of splitting.

use omacharts_engine::continuous::Adjustment;
use omacharts_engine::{BarColouring, BarStyle, Session};

use crate::store::Store;

pub const SETTING_BAR_STYLE: &str = "bar_style";
/// How a new chart paints the bar look every chart shares. Its own key,
/// not the old `bar_scheme`: that one now only says what the look was before
/// it had four answers, and reading it here too would turn a chart twice.
pub const SETTING_COLOURING: &str = "chart_colouring";
pub const SETTING_SESSION: &str = "chart_session";
/// How a futures root's months are joined at the rolls, on a feed that
/// stitches them: back-adjusted or as traded.
pub const SETTING_CONTINUOUS: &str = "chart_continuous";
/// Gridlines and the last-price line are every chart's, not a default:
/// see [`crate::ui::appearance`].
pub const SETTING_SHOW_GRID: &str = "show_grid";
pub const SETTING_PRICE_LINE: &str = "price_line";
/// How many pixels wide an OHLC bar's line and ticks are drawn.
pub const SETTING_OHLC_THICKNESS: &str = "ohlc_thickness";
/// The range it takes is the chart's own.
pub use omacharts_chart::{clamp_thickness, DEFAULT_OHLC_THICKNESS, MAX_OHLC_THICKNESS, MIN_OHLC_THICKNESS};
const SETTING_LOG_SCALE: &str = "chart_log_scale";

/// Whether one of the switches every chart shares, stored under `key`, is
/// on: as stored, or before anybody sets it, gridlines off and the rest on.
/// The window, the Overlays page and `config` all read it here, so none of
/// them can open to a different answer.
pub fn chart_switch(store: &Store, key: &str) -> bool {
    store.setting_bool(key, key != SETTING_SHOW_GRID)
}

const SETTING_SHOWS_DRAWINGS: &str = "chart_shows_drawings";
const SETTING_SENDS_DRAWINGS: &str = "chart_sends_drawings";
/// Whether a chart draws its indicator legend at all.
pub const SETTING_SHOWS_LEGEND: &str = "chart_shows_legend";
/// The chart template a new chart opens with, by id, or [`NO_TEMPLATE`].
pub const SETTING_TEMPLATE: &str = "chart_template";
/// How "no template" is written down, so that it reads as a choice rather
/// than as a setting nobody has made yet.
pub const NO_TEMPLATE: &str = "none";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ChartDefaults {
    pub bar_style: BarStyle,
    pub colouring: BarColouring,
    /// How many pixels wide an OHLC bar is drawn.
    pub ohlc_thickness: u8,
    pub session: Session,
    /// How a root's months are joined, on a feed that stitches them.
    pub continuous: Adjustment,
    pub log_scale: bool,
    pub shows_drawings: bool,
    pub sends_drawings: bool,
    /// Whether the chart draws its indicator legend. Each indicator's own
    /// `in_legend` still decides whether it is named while this is on.
    pub shows_legend: bool,
    /// The chart template it shows, if any. See [`crate::templates`].
    pub template: Option<u32>,
}

/// What a chart opens with when nobody has said otherwise.
impl Default for ChartDefaults {
    fn default() -> ChartDefaults {
        ChartDefaults {
            bar_style: BarStyle::default(),
            colouring: BarColouring::default(),
            ohlc_thickness: DEFAULT_OHLC_THICKNESS,
            session: Session::default(),
            continuous: Adjustment::default(),
            log_scale: false,
            shows_drawings: true,
            sends_drawings: true,
            shows_legend: true,
            template: None,
        }
    }
}

impl ChartDefaults {
    pub fn load(store: &Store) -> ChartDefaults {
        let fallback = ChartDefaults::default();
        ChartDefaults {
            bar_style: store
                .setting(SETTING_BAR_STYLE)
                .and_then(|key| BarStyle::from_key(&key))
                .unwrap_or(fallback.bar_style),
            colouring: store
                .setting(SETTING_COLOURING)
                .and_then(|key| BarColouring::from_key(&key))
                .unwrap_or(fallback.colouring),
            ohlc_thickness: store
                .setting(SETTING_OHLC_THICKNESS)
                .and_then(|n| n.trim().parse::<u8>().ok())
                .map_or(fallback.ohlc_thickness, clamp_thickness),
            session: store
                .setting(SETTING_SESSION)
                .and_then(|key| Session::from_key(&key))
                .unwrap_or(fallback.session),
            continuous: store
                .setting(SETTING_CONTINUOUS)
                .and_then(|key| Adjustment::from_key(&key))
                .unwrap_or(fallback.continuous),
            log_scale: store.setting_bool(SETTING_LOG_SCALE, fallback.log_scale),
            shows_drawings: store.setting_bool(SETTING_SHOWS_DRAWINGS, fallback.shows_drawings),
            sends_drawings: store.setting_bool(SETTING_SENDS_DRAWINGS, fallback.sends_drawings),
            shows_legend: store.setting_bool(SETTING_SHOWS_LEGEND, fallback.shows_legend),
            template: store.setting(SETTING_TEMPLATE).and_then(|id| id.parse().ok()),
        }
    }

    pub fn save(&self, store: &Store) {
        store.set_setting(SETTING_BAR_STYLE, self.bar_style.key());
        store.set_setting(SETTING_COLOURING, self.colouring.key());
        store.set_setting(SETTING_OHLC_THICKNESS, &self.ohlc_thickness.to_string());
        store.set_setting(SETTING_SESSION, self.session.key());
        store.set_setting(SETTING_CONTINUOUS, self.continuous.key());
        store.set_setting_bool(SETTING_LOG_SCALE, self.log_scale);
        store.set_setting_bool(SETTING_SHOWS_DRAWINGS, self.shows_drawings);
        store.set_setting_bool(SETTING_SENDS_DRAWINGS, self.sends_drawings);
        store.set_setting_bool(SETTING_SHOWS_LEGEND, self.shows_legend);
        let template = self.template.map(|id| id.to_string());
        store.set_setting(SETTING_TEMPLATE, template.as_deref().unwrap_or(NO_TEMPLATE));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_is_saved_is_what_comes_back() {
        let store = Store::memory().unwrap();
        let fresh = ChartDefaults::load(&store);
        assert_eq!(fresh.colouring, BarColouring::Filled);
        assert_eq!(fresh.ohlc_thickness, DEFAULT_OHLC_THICKNESS);
        assert!(fresh.shows_drawings && fresh.sends_drawings);
        assert!(fresh.shows_legend, "the legend is shown unless somebody turns it off");

        let chosen = ChartDefaults {
            bar_style: BarStyle::Ohlc,
            colouring: BarColouring::Monochrome,
            ohlc_thickness: 3,
            session: Session::ALL[Session::ALL.len() - 1],
            continuous: Adjustment::AsTraded,
            log_scale: true,
            shows_drawings: false,
            sends_drawings: false,
            shows_legend: false,
            template: None,
        };
        chosen.save(&store);
        assert_eq!(ChartDefaults::load(&store), chosen);
        let on_one = ChartDefaults { template: Some(7), ..chosen };
        on_one.save(&store);
        assert_eq!(ChartDefaults::load(&store), on_one);
    }

    /// Gridlines are off until somebody turns them on; the last-price line
    /// is on. What is stored wins either way.
    #[test]
    fn a_fresh_store_draws_no_gridlines_and_the_last_price_line() {
        let store = Store::memory().unwrap();
        assert!(!chart_switch(&store, SETTING_SHOW_GRID));
        assert!(chart_switch(&store, SETTING_PRICE_LINE));
        store.set_setting_bool(SETTING_SHOW_GRID, true);
        store.set_setting_bool(SETTING_PRICE_LINE, false);
        assert!(chart_switch(&store, SETTING_SHOW_GRID));
        assert!(!chart_switch(&store, SETTING_PRICE_LINE));
    }

    /// A thickness typed into the settings by hand comes back inside the
    /// range a chart can draw.
    #[test]
    fn a_stored_thickness_is_held_to_the_range() {
        let store = Store::memory().unwrap();
        store.set_setting(SETTING_OHLC_THICKNESS, "40");
        assert_eq!(ChartDefaults::load(&store).ohlc_thickness, MAX_OHLC_THICKNESS);
        store.set_setting(SETTING_OHLC_THICKNESS, "thick");
        assert_eq!(ChartDefaults::load(&store).ohlc_thickness, DEFAULT_OHLC_THICKNESS);
    }
}
