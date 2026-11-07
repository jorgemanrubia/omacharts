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
/// Whether the colouring is worn the other way round, its up and down
/// colours exchanged. Beside `bar_scheme` rather than part of it, so the
/// switch applies to any colouring and the colouring stays what it is.
pub const SETTING_INVERT_BARS: &str = "invert_bars";
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
    /// Whether the colouring's up and down are exchanged: red for a rise.
    pub inverted: bool,
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
            inverted: false,
            session: Session::default(),
            show_grid: false,
            log_scale: false,
            shows_drawings: true,
            sends_drawings: true,
            template: None,
        }
    }
}

/// The colouring new charts open with, and whether it is worn the other way
/// round, as the two settings say together. An older version wrote the
/// turned-round colourings as schemes of their own (`theme-red-up`), which
/// read back as the plain colouring with the switch on, so nobody opens the
/// app to candles the other way round from the ones they chose.
pub fn stored_colouring(store: &Store) -> (BarColouring, bool) {
    let (colouring, turned_round) = store
        .setting(crate::theming::SETTING_BARS)
        .and_then(|key| BarColouring::from_stored(&key))
        .unwrap_or_default();
    (colouring, turned_round || store.setting_bool(SETTING_INVERT_BARS, false))
}

impl ChartDefaults {
    pub fn load(store: &Store) -> ChartDefaults {
        let fallback = ChartDefaults::default();
        let (colouring, inverted) = stored_colouring(store);
        ChartDefaults {
            bar_style: store
                .setting(SETTING_BAR_STYLE)
                .and_then(|key| BarStyle::from_key(&key))
                .unwrap_or(fallback.bar_style),
            colouring,
            inverted,
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
        store.set_setting_bool(SETTING_INVERT_BARS, self.inverted);
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
        assert!(!fresh.inverted && !fresh.show_grid && fresh.shows_drawings && fresh.sends_drawings);

        let chosen = ChartDefaults {
            bar_style: BarStyle::Ohlc,
            colouring: BarColouring::Monochrome,
            inverted: true,
            session: Session::ALL[Session::ALL.len() - 1],
            show_grid: true,
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

    /// An older version wrote the colours the other way round as a scheme
    /// of its own; it is read as the colouring it was with the switch on,
    /// and saving writes the colouring and the switch.
    #[test]
    fn a_turned_round_scheme_an_older_version_wrote_is_the_switch_on() {
        let store = Store::memory().unwrap();
        store.set_setting(crate::theming::SETTING_BARS, omacharts_engine::theme::THEME_RED_UP_ID);
        let read = ChartDefaults::load(&store);
        assert_eq!((read.colouring, read.inverted), (BarColouring::Filled, true));
        assert_eq!(stored_colouring(&store), (BarColouring::Filled, true));

        read.save(&store);
        assert_eq!(store.setting(crate::theming::SETTING_BARS).as_deref(), Some("theme"));
        assert!(store.setting_bool(SETTING_INVERT_BARS, false));
        assert_eq!(ChartDefaults::load(&store), read);

        // The switch is its own setting: off with a plain colouring, and on
        // over any colouring.
        store.set_setting_bool(SETTING_INVERT_BARS, false);
        assert!(!ChartDefaults::load(&store).inverted);
        store.set_setting(crate::theming::SETTING_BARS, omacharts_engine::theme::THEME_SOFT_ID);
        store.set_setting_bool(SETTING_INVERT_BARS, true);
        assert_eq!(stored_colouring(&store), (BarColouring::Soft, true));
    }
}
