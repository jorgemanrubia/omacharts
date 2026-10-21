//! What a new chart opens with.
//!
//! Set in the preferences, and nowhere else: changing one chart changes that
//! chart. A split is the exception, because it copies the chart it divides —
//! two of what you were looking at is the point of splitting.

use omacharts_engine::{BarColouring, BarStyle, Session};

use crate::store::Store;

pub const SETTING_BAR_STYLE: &str = "bar_style";
pub const SETTING_SESSION: &str = "chart_session";
pub const SETTING_SHOW_GRID: &str = "show_grid";
const SETTING_LOG_SCALE: &str = "chart_log_scale";
const SETTING_SHOWS_DRAWINGS: &str = "chart_shows_drawings";
const SETTING_SENDS_DRAWINGS: &str = "chart_sends_drawings";
/// The chart template a new chart opens with, by id, or [`NO_TEMPLATE`].
pub const SETTING_TEMPLATE: &str = "chart_template";
/// How "no template" is written down, so that it reads as a choice rather
/// than as a setting nobody has made yet.
pub const NO_TEMPLATE: &str = "none";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ChartDefaults {
    pub bar_style: BarStyle,
    pub colouring: BarColouring,
    pub session: Session,
    pub show_grid: bool,
    pub log_scale: bool,
    pub shows_drawings: bool,
    pub sends_drawings: bool,
    /// The chart template it shows, if any. See [`crate::templates`].
    pub template: Option<u32>,
}

/// What a chart opens with when nobody has said otherwise.
impl Default for ChartDefaults {
    fn default() -> ChartDefaults {
        ChartDefaults {
            bar_style: BarStyle::default(),
            colouring: BarColouring::default(),
            session: Session::default(),
            show_grid: true,
            log_scale: false,
            shows_drawings: true,
            sends_drawings: true,
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
                .setting(crate::theming::SETTING_BARS)
                .and_then(|key| BarColouring::from_key(&key))
                .unwrap_or(fallback.colouring),
            session: store
                .setting(SETTING_SESSION)
                .and_then(|key| Session::from_key(&key))
                .unwrap_or(fallback.session),
            show_grid: store.setting_bool(SETTING_SHOW_GRID, fallback.show_grid),
            log_scale: store.setting_bool(SETTING_LOG_SCALE, fallback.log_scale),
            shows_drawings: store.setting_bool(SETTING_SHOWS_DRAWINGS, fallback.shows_drawings),
            sends_drawings: store.setting_bool(SETTING_SENDS_DRAWINGS, fallback.sends_drawings),
            template: store.setting(SETTING_TEMPLATE).and_then(|id| id.parse().ok()),
        }
    }

    pub fn save(&self, store: &Store) {
        store.set_setting(SETTING_BAR_STYLE, self.bar_style.key());
        store.set_setting(crate::theming::SETTING_BARS, self.colouring.key());
        store.set_setting(SETTING_SESSION, self.session.key());
        store.set_setting_bool(SETTING_SHOW_GRID, self.show_grid);
        store.set_setting_bool(SETTING_LOG_SCALE, self.log_scale);
        store.set_setting_bool(SETTING_SHOWS_DRAWINGS, self.shows_drawings);
        store.set_setting_bool(SETTING_SENDS_DRAWINGS, self.sends_drawings);
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
        assert!(fresh.show_grid && fresh.shows_drawings && fresh.sends_drawings);

        let chosen = ChartDefaults {
            bar_style: BarStyle::Ohlc,
            colouring: BarColouring::Monochrome,
            session: Session::ALL[Session::ALL.len() - 1],
            show_grid: false,
            log_scale: true,
            shows_drawings: false,
            sends_drawings: false,
            template: None,
        };
        chosen.save(&store);
        assert_eq!(ChartDefaults::load(&store), chosen);
        let on_one = ChartDefaults { template: Some(7), ..chosen };
        on_one.save(&store);
        assert_eq!(ChartDefaults::load(&store), on_one);
    }
}
