//! What a new chart opens with.
//!
//! Set in the preferences, and nowhere else: changing one chart changes that
//! chart. A split is the exception, because it copies the chart it divides —
//! two of what you were looking at is the point of splitting.

use omacharts_engine::theme::StoredColouring;
use omacharts_engine::{BarColouring, BarStyle, Session};

use crate::store::Store;

pub const SETTING_BAR_STYLE: &str = "bar_style";
pub const SETTING_SESSION: &str = "chart_session";
pub const SETTING_SHOW_GRID: &str = "show_grid";
/// The two switches worn over the colouring: up and down exchanged, and a
/// rise drawn as an outline. Beside `bar_scheme` rather than part of it, so
/// each applies to any palette and the palette stays what it is.
pub const SETTING_SWAP_COLOURS: &str = "swap_colours";
pub const SETTING_HOLLOW_UP: &str = "hollow_up";
/// How many pixels wide an OHLC bar's line and ticks are drawn.
pub const SETTING_OHLC_THICKNESS: &str = "ohlc_thickness";
/// The range it takes is the chart's own.
pub use omacharts_chart::{clamp_thickness, DEFAULT_OHLC_THICKNESS, MAX_OHLC_THICKNESS, MIN_OHLC_THICKNESS};
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
    pub swapped: bool,
    /// Whether a rising body is drawn as an outline.
    pub hollow_up: bool,
    /// How many pixels wide an OHLC bar is drawn.
    pub ohlc_thickness: u8,
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
            swapped: false,
            hollow_up: false,
            ohlc_thickness: DEFAULT_OHLC_THICKNESS,
            session: Session::default(),
            show_grid: false,
            log_scale: false,
            shows_drawings: true,
            sends_drawings: true,
            template: None,
        }
    }
}

/// The colouring new charts open with and the switches worn over it, as
/// the three settings say together. An older version wrote a hollow rise
/// and the colours the other way round as schemes of their own
/// (`theme-hollow`, `theme-red-up`), which read back as Vivid with that
/// switch on, so nobody opens the app to candles other than the ones they
/// chose.
pub fn stored_colouring(store: &Store) -> StoredColouring {
    stored_colouring_of(store, store.setting(crate::theming::SETTING_BARS).as_deref())
}

/// [`stored_colouring`] for a `bar_scheme` setting already read, so a
/// caller that keeps the stored id as written reads it once.
pub fn stored_colouring_of(store: &Store, key: Option<&str>) -> StoredColouring {
    let written = key.and_then(BarColouring::from_stored).unwrap_or_default();
    StoredColouring {
        colouring: written.colouring,
        swapped: written.swapped || store.setting_bool(SETTING_SWAP_COLOURS, false),
        hollow_up: written.hollow_up || store.setting_bool(SETTING_HOLLOW_UP, false),
    }
}

impl ChartDefaults {
    pub fn load(store: &Store) -> ChartDefaults {
        let fallback = ChartDefaults::default();
        let look = stored_colouring(store);
        ChartDefaults {
            bar_style: store
                .setting(SETTING_BAR_STYLE)
                .and_then(|key| BarStyle::from_key(&key))
                .unwrap_or(fallback.bar_style),
            colouring: look.colouring,
            swapped: look.swapped,
            hollow_up: look.hollow_up,
            ohlc_thickness: store
                .setting(SETTING_OHLC_THICKNESS)
                .and_then(|n| n.trim().parse::<u8>().ok())
                .map_or(fallback.ohlc_thickness, clamp_thickness),
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
        store.set_setting_bool(SETTING_SWAP_COLOURS, self.swapped);
        store.set_setting_bool(SETTING_HOLLOW_UP, self.hollow_up);
        store.set_setting(SETTING_OHLC_THICKNESS, &self.ohlc_thickness.to_string());
        store.set_setting(SETTING_SESSION, self.session.key());
        store.set_setting_bool(SETTING_SHOW_GRID, self.show_grid);
        store.set_setting_bool(SETTING_LOG_SCALE, self.log_scale);
        store.set_setting_bool(SETTING_SHOWS_DRAWINGS, self.shows_drawings);
        store.set_setting_bool(SETTING_SENDS_DRAWINGS, self.sends_drawings);
        let template = self.template.map(|id| id.to_string());
        store.set_setting(SETTING_TEMPLATE, template.as_deref().unwrap_or(NO_TEMPLATE));
    }

    /// The colouring and its switches, as a chart is given them.
    pub fn look(&self) -> StoredColouring {
        StoredColouring { colouring: self.colouring, swapped: self.swapped, hollow_up: self.hollow_up }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_is_saved_is_what_comes_back() {
        let store = Store::memory().unwrap();
        let fresh = ChartDefaults::load(&store);
        assert_eq!(fresh.colouring, BarColouring::Vivid);
        assert!(!fresh.swapped && !fresh.hollow_up && !fresh.show_grid);
        assert_eq!(fresh.ohlc_thickness, DEFAULT_OHLC_THICKNESS);
        assert!(fresh.shows_drawings && fresh.sends_drawings);

        let chosen = ChartDefaults {
            bar_style: BarStyle::Ohlc,
            colouring: BarColouring::Monochrome,
            swapped: true,
            hollow_up: true,
            ohlc_thickness: 3,
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

    /// An older version wrote a hollow rise and the colours the other way
    /// round as schemes of their own; each is read as Vivid with that switch
    /// on, and saving writes the palette and the switches.
    #[test]
    fn a_scheme_an_older_version_wrote_is_vivid_with_its_switch_on() {
        use omacharts_engine::theme::{THEME_HOLLOW_ID, THEME_MONO_ID, THEME_RED_UP_ID, THEME_SOFT_ID};
        let store = Store::memory().unwrap();
        for (written, swapped, hollow_up) in [(THEME_RED_UP_ID, true, false), (THEME_HOLLOW_ID, false, true), ("hollow", false, true)] {
            store.set_setting(crate::theming::SETTING_BARS, written);
            store.set_setting_bool(SETTING_SWAP_COLOURS, false);
            store.set_setting_bool(SETTING_HOLLOW_UP, false);
            let read = ChartDefaults::load(&store);
            assert_eq!((read.colouring, read.swapped, read.hollow_up), (BarColouring::Vivid, swapped, hollow_up), "{written}");
            assert_eq!(stored_colouring(&store), read.look());

            read.save(&store);
            assert_eq!(store.setting(crate::theming::SETTING_BARS).as_deref(), Some("theme"));
            assert_eq!(store.setting_bool(SETTING_SWAP_COLOURS, false), swapped);
            assert_eq!(store.setting_bool(SETTING_HOLLOW_UP, false), hollow_up);
            assert_eq!(ChartDefaults::load(&store), read);
        }
        store.set_setting(crate::theming::SETTING_BARS, "monochrome");
        assert_eq!(ChartDefaults::load(&store).colouring, BarColouring::Monochrome);
        store.set_setting(crate::theming::SETTING_BARS, THEME_MONO_ID);
        assert_eq!(ChartDefaults::load(&store).colouring, BarColouring::Monochrome);

        // The switches are settings of their own: off with a plain palette,
        // and on over any palette.
        store.set_setting_bool(SETTING_SWAP_COLOURS, false);
        store.set_setting_bool(SETTING_HOLLOW_UP, false);
        let plain = ChartDefaults::load(&store);
        assert!(!plain.swapped && !plain.hollow_up);
        store.set_setting(crate::theming::SETTING_BARS, THEME_SOFT_ID);
        store.set_setting_bool(SETTING_SWAP_COLOURS, true);
        store.set_setting_bool(SETTING_HOLLOW_UP, true);
        assert_eq!(
            stored_colouring(&store),
            StoredColouring { colouring: BarColouring::Soft, swapped: true, hollow_up: true }
        );
    }
}
