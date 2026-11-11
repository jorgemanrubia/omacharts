//! What a new chart opens with.
//!
//! Set in the preferences, and nowhere else, which change every chart with
//! them: changing one chart changes that chart. A split is the exception,
//! because it copies the chart it divides — two of what you were looking at
//! is the point of splitting.

use omacharts_engine::continuous::Adjustment;
use omacharts_engine::{BarStyle, Session};

use crate::store::Store;

/// How every chart draws its bars: candles, OHLC bars or a line. Every
/// chart's, not a new chart's default. See [`bar_style`].
pub const SETTING_BAR_STYLE: &str = "bar_style";
pub const SETTING_SESSION: &str = "chart_session";
/// How a futures root's months are joined at the rolls, on a feed that
/// stitches them: back-adjusted or as traded.
pub const SETTING_CONTINUOUS: &str = "chart_continuous";
/// Gridlines and the last-price line are every chart's look, not a
/// default: see [`crate::chart_look`].
pub const SETTING_SHOW_GRID: &str = "show_grid";
pub const SETTING_PRICE_LINE: &str = "price_line";
const SETTING_LOG_SCALE: &str = "chart_log_scale";

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

/// One row of a chart's settings, changed: what Preferences › Charts hands
/// to every chart or to the focused one.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChartSetting {
    Session(Session),
    LogScale(bool),
    ShowsDrawings(bool),
    SendsDrawings(bool),
    Template(Option<u32>),
    Continuous(Adjustment),
    Legend(bool),
}

impl ChartDefaults {
    pub fn set(&mut self, setting: ChartSetting) {
        match setting {
            ChartSetting::Session(session) => self.session = session,
            ChartSetting::LogScale(on) => self.log_scale = on,
            ChartSetting::ShowsDrawings(on) => self.shows_drawings = on,
            ChartSetting::SendsDrawings(on) => self.sends_drawings = on,
            ChartSetting::Template(template) => self.template = template,
            ChartSetting::Continuous(join) => self.continuous = join,
            ChartSetting::Legend(on) => self.shows_legend = on,
        }
    }
}

/// How every chart draws its bars.
pub fn bar_style(store: &Store) -> BarStyle {
    store.setting(SETTING_BAR_STYLE).and_then(|key| BarStyle::from_key(&key)).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_is_saved_is_what_comes_back() {
        let store = Store::memory().unwrap();
        let fresh = ChartDefaults::load(&store);
        assert!(fresh.shows_drawings && fresh.sends_drawings);
        assert!(fresh.shows_legend, "the legend is shown unless somebody turns it off");

        let chosen = ChartDefaults {
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
}
