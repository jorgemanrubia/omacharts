//! Choosing a theme and a bar scheme, and making the whole app wear them.
//!
//! On Omarchy the desktop theme is the default for both, and it is followed
//! live: the palette is re-read on a timer and the app repaints when it
//! changes. Everything is overridable in settings, and a custom theme is just
//! a palette somebody saved.
//!
//! "The whole app" is meant literally. The theme's colours are pushed into
//! libadwaita's own named colours, so headers, popovers, lists, dialogs and
//! the chart all come from one palette rather than the chart being a themed
//! island in a default-grey window.

use std::path::PathBuf;

use std::fmt::Write as _;

use omacharts_engine::frame::{GUTTER_WIDTH, RING_WIDTH};
use omacharts_engine::theme::{
    builtin_themes, BarColouring, BarScheme, Mode, Theme, FALLBACK_THEME_ID, OMARCHY_ID,
    THEME_BARS_ID,
};
use omacharts_engine::Ink;
use omacharts_engine::omarchy;

use crate::store::{home, Store};
use crate::ui::colors;

pub const SETTING_THEME: &str = "theme";
pub const SETTING_BARS: &str = "bar_scheme";

pub struct Theming {
    home: PathBuf,
    /// Rebuilt from the desktop whenever its fingerprint moves.
    omarchy: Option<Theme>,
    omarchy_fingerprint: Option<String>,
    builtin_themes: Vec<Theme>,
    /// Every Omarchy theme installed, each fixed rather than following.
    installed_themes: Vec<Theme>,
    custom_themes: Vec<Theme>,
    theme_id: String,
    scheme_id: String,
    provider: Option<gtk::CssProvider>,
}

impl Theming {
    pub fn load(store: &Store) -> Theming {
        let home = home();
        let omarchy = omarchy::current(&home);
        let omarchy_fingerprint = omarchy::fingerprint(&home);

        // Omarchy's theme is the default when the desktop has one, so a fresh
        // install already matches everything else on screen.
        let default_theme = if omarchy.is_some() { OMARCHY_ID } else { FALLBACK_THEME_ID };

        let theming = Theming {
            theme_id: store.setting(SETTING_THEME).unwrap_or_else(|| default_theme.to_string()),
            scheme_id: store.setting(SETTING_BARS).unwrap_or_else(|| THEME_BARS_ID.to_string()),
            omarchy,
            omarchy_fingerprint,
            builtin_themes: builtin_themes(),
            installed_themes: omarchy::installed(&home),
            custom_themes: store.custom_themes(),
            home,
            provider: None,
        };
        theming
    }

    /// Bar schemes used to carry colours of their own; now a theme does. A
    /// scheme in use whose colours are not the theme's — the blue and orange
    /// one, the classic pair, one somebody made — becomes a copy of the theme
    /// wearing them, so nobody opens the app to different candles. Once: the
    /// setting then names a colouring, and this finds nothing to do.
    ///
    /// Run by the window as it opens, not by every reader: a command that only
    /// asks for colours should not write a theme.
    pub fn carry_over_candle_colours(&mut self, store: &Store) {
        let Some(bars) = legacy_bar_colours(&self.scheme_id)
            .or_else(|| store.legacy_bar_scheme(&self.scheme_id))
        else {
            return;
        };
        let base = self.theme();
        let name = format!("{} with {}", base.name, bars.name);
        let id = format!("custom-{}", self.scheme_id);
        let hollow = bars.up_fill.len() == 9 && bars.up_fill.ends_with("00");
        let mut theme = base.duplicate(id.clone(), name);
        theme.bars = Some(omacharts_engine::BarScheme { mono: String::new(), ..bars });
        store.save_theme(&theme);
        self.custom_themes = store.custom_themes();
        self.select_theme(&id, store);
        let colouring = if hollow { BarColouring::Hollow } else { BarColouring::Filled };
        self.select_bar_scheme(colouring.key(), store);
    }

    pub fn omarchy_available(&self) -> bool {
        self.omarchy.is_some()
    }

    /// Every theme on offer: the desktop's, then ours, then every Omarchy
    /// theme installed, then the user's.
    pub fn themes(&self) -> Vec<&Theme> {
        self.omarchy
            .iter()
            .chain(self.builtin_themes.iter())
            .chain(self.installed_themes.iter())
            .chain(self.custom_themes.iter())
            .collect()
    }

    /// Every bar scheme on offer. The first is built from the active theme, so
    /// candles match the desktop without the user choosing anything.
    /// The active theme, owned.
    ///
    /// A theme can vanish — Omarchy uninstalled, a custom one deleted from
    /// another window — so this falls back rather than refusing to draw.
    pub fn theme(&self) -> Theme {
        let themes = self.themes();
        themes
            .iter()
            .find(|t| t.id == self.theme_id)
            .or_else(|| themes.first())
            .map(|t| (*t).clone())
            .unwrap_or_else(|| self.builtin_themes[0].clone())
    }

    /// What new charts paint their bars in, and what the watchlist and the
    /// bar widget say up and down in.
    pub fn bar_scheme(&self) -> BarScheme {
        self.bar_scheme_for(&self.theme())
    }

    fn bar_scheme_for(&self, theme: &Theme) -> BarScheme {
        BarColouring::from_key(&self.scheme_id).unwrap_or_default().scheme(theme)
    }

    /// The ink the bar widget writes in.
    ///
    /// The desktop's theme, because the widget is drawn on the desktop's
    /// surfaces rather than the window's, with the bar scheme chosen here, so
    /// the panel says up the way the charts do — red, if that is what was
    /// picked. A scheme built from a theme is built from the desktop's: "the
    /// theme's colours" in the bar means the theme the bar is wearing. With
    /// no Omarchy theme there is no bar to draw in, and the window's theme
    /// stands in.
    pub fn bar_widget_ink(&self) -> Ink {
        let theme = self.omarchy.clone().unwrap_or_else(|| self.theme());
        theme.ink(&self.bar_scheme_for(&theme))
    }

    pub fn theme_id(&self) -> &str {
        &self.theme_id
    }

    pub fn scheme_id(&self) -> &str {
        &self.scheme_id
    }

    pub fn select_theme(&mut self, id: &str, store: &Store) {
        self.theme_id = id.to_string();
        store.set_setting(SETTING_THEME, id);
    }

    pub fn select_bar_scheme(&mut self, id: &str, store: &Store) {
        self.scheme_id = id.to_string();
        store.set_setting(SETTING_BARS, id);
    }

    pub fn reload_custom(&mut self, store: &Store) {
        self.custom_themes = store.custom_themes();
    }

    /// Re-read which theme and bar scheme are chosen, for a choice made
    /// outside this process.
    ///
    /// A command from a terminal writes the setting and nothing else; the
    /// window is still wearing whatever it started in, and would write its own
    /// choice back over the new one the next time anything touched it.
    pub fn reload_selection(&mut self, store: &Store) {
        let default_theme = if self.omarchy.is_some() { OMARCHY_ID } else { FALLBACK_THEME_ID };
        self.theme_id = store.setting(SETTING_THEME).unwrap_or_else(|| default_theme.to_string());
        self.scheme_id =
            store.setting(SETTING_BARS).unwrap_or_else(|| THEME_BARS_ID.to_string());
        self.reload_custom(store);
    }

    /// Re-read the desktop palette. `true` when something moved and the app
    /// should restyle and repaint.
    ///
    /// Called on a timer rather than from a file watch because Omarchy swaps a
    /// symlinked directory, which is exactly where watches miss changes.
    pub fn poll_omarchy(&mut self) -> bool {
        let fingerprint = omarchy::fingerprint(&self.home);
        if fingerprint == self.omarchy_fingerprint {
            return false;
        }
        self.omarchy_fingerprint = fingerprint;
        self.omarchy = omarchy::current(&self.home);
        true
    }

    /// Push the active theme into libadwaita's named colours.
    pub fn apply(&mut self) {
        let theme = self.theme();

        adw::StyleManager::default().set_color_scheme(match theme.mode {
            Mode::Dark => adw::ColorScheme::ForceDark,
            Mode::Light => adw::ColorScheme::ForceLight,
        });

        let css = stylesheet(&theme, &self.bar_scheme());
        let provider = self.provider.get_or_insert_with(|| {
            let provider = gtk::CssProvider::new();
            if let Some(display) = gtk::gdk::Display::default() {
                gtk::style_context_add_provider_for_display(
                    &display,
                    &provider,
                    // Above the platform stylesheet, so our named colours win.
                    gtk::STYLE_PROVIDER_PRIORITY_APPLICATION + 1,
                );
            }
            provider
        });
        provider.load_from_data(&css);
    }
}

/// The two fixed schemes whose colours were not the theme's, as they shipped.
fn legacy_bar_colours(id: &str) -> Option<BarScheme> {
    let (name, up, down, volume_up, volume_down) = match id {
        "classic" => ("Classic", "#3fb950", "#f85149", "#2d6e3b", "#8c322e"),
        "accessible" => ("Blue / Orange", "#4493f8", "#ff9b50", "#2a5a8f", "#8f5a2a"),
        _ => return None,
    };
    Some(BarScheme {
        id: id.to_string(),
        name: name.to_string(),
        source: omacharts_engine::theme::Source::Custom,
        up: up.to_string(),
        up_fill: up.to_string(),
        down: down.to_string(),
        down_fill: down.to_string(),
        volume_up: volume_up.to_string(),
        volume_down: volume_down.to_string(),
        neutral: "#8b949e".to_string(),
        mono: String::new(),
    })
}

/// How far in from its left edge the window's corner starts its buttons:
/// the `padding-left` of `.window-corner` below, which is what centres the
/// pair on the axis's column, and what the rail's + is placed by. The two
/// change together.
pub(crate) const CORNER_PAD_START: i32 = 8;

/// The app's CSS: libadwaita's named colours, redefined from the theme, plus
/// the few classes we add ourselves.
pub fn stylesheet(theme: &Theme, scheme: &BarScheme) -> String {
    let ui = &theme.ui;
    let accent_fg = colors::readable_on(&ui.accent);
    let frame = theme.frame(scheme);

    let mut css = String::with_capacity(2048);
    let mut define = |name: &str, value: &str| {
        css.push_str(&format!("@define-color {name} {value};\n"));
    };

    // Windows and views. `view_bg_color` is the chart's own ground, so lists
    // and the chart agree.
    define("window_bg_color", &ui.surface);
    define("window_fg_color", &ui.text);
    define("view_bg_color", &ui.background);
    define("view_fg_color", &ui.text);
    define("headerbar_bg_color", &ui.surface_variant);
    define("headerbar_fg_color", &ui.text);
    define("headerbar_border_color", &ui.border);
    define("headerbar_backdrop_color", &ui.surface);
    define("sidebar_bg_color", &ui.surface);
    define("sidebar_fg_color", &ui.text);
    define("sidebar_border_color", &ui.border);
    define("sidebar_backdrop_color", &ui.surface);
    define("secondary_sidebar_bg_color", &ui.surface);
    define("secondary_sidebar_fg_color", &ui.text);
    define("card_bg_color", &ui.surface_variant);
    define("card_fg_color", &ui.text);
    define("dialog_bg_color", &ui.surface);
    define("dialog_fg_color", &ui.text);
    define("popover_bg_color", &ui.surface_variant);
    define("popover_fg_color", &ui.text);
    define("borders", &ui.border);

    // Accent. libadwaita 1.6 split the standalone accent from the filled one.
    define("accent_bg_color", &ui.accent);
    define("accent_fg_color", accent_fg);
    define("accent_color", &ui.accent);

    // Rising and falling text follows the bar scheme, so a watchlist agrees
    // with the candles beside it.
    define("omacharts_up", &scheme.up);
    define("omacharts_down", &scheme.down);

    // The frame around tiled charts. Derived, not taken: the engine explains
    // why in `frame.rs`, and `doc/themes/frames.html` shows the result on
    // every theme.
    define("omacharts_gutter", &frame.gutter);
    define("omacharts_gutter_hover", &frame.gutter_hover);
    define("omacharts_focus", &frame.focus);
    // The chart's price-axis rule, for the corner that stands on it.
    define("omacharts_axis", &ui.axis);

    css.push_str(
        "
.change-up { color: @omacharts_up; }
.change-down { color: @omacharts_down; }
.change-flat { opacity: 0.55; }

.numeric { font-feature-settings: 'tnum'; font-variant-numeric: tabular-nums; }

/* The symbol palette: a quiet list where the row, not a border, carries the
   selection. */
.symbol-row-ticker { font-weight: 700; }
.symbol-row-name { font-size: 0.9em; opacity: 0.7; }
.symbol-kind {
  font-size: 0.75em;
  padding: 1px 6px;
  border-radius: 6px;
  background: alpha(@accent_bg_color, 0.18);
  color: @accent_color;
}

/* The readout above the chart. Tabular figures so values do not jitter as the
   crosshair moves. */
.readout { font-size: 0.85em; }
.readout-symbol { font-size: 1.05em; font-weight: 700; }
.delay-chip {
  font-size: 0.75em;
  padding: 1px 6px;
  border-radius: 6px;
  background: alpha(@card_fg_color, 0.08);
  opacity: 0.75;
}

/* Timeframe buttons: flat strip, the active one filled. */
.timeframe-strip { padding: 2px; }
.timeframe-strip button { min-width: 34px; padding: 2px 6px; }

/* Where a symbol trades: quieter than its name, beside what it is. */
.symbol-venue {
  font-size: 0.8em;
  opacity: 0.45;
  font-feature-settings: 'tnum';
}

/* The row offering a symbol nobody listed. Quieter than a real match, because
   it is a guess rather than an answer. */
.symbol-row-unlisted .symbol-row-name { opacity: 0.55; font-style: italic; }
.symbol-row-unlisted .symbol-kind { opacity: 0.4; }

/* Thickness and line style, picked by looking rather than by naming a number.
   A segmented strip, so the options sit beside each other and the one in use
   is the one pressed. */
.stroke-picker button {
  padding: 2px 4px;
  min-width: 0;
  min-height: 0;
}

.swatch-button { min-width: 26px; min-height: 26px; padding: 0; border-radius: 6px; }

/* The link toggle is a glyph, not a button: no frame, no fill, not even when
   it is on — a chart that follows the rail says so by the icon being there at
   full strength, and one that does not says so by the icon being nearly gone.
   The only time it wears anything is under the pointer, which is the moment
   you are about to click it. */
.legend-link,
.legend-link:checked,
.legend-link:checked:not(:hover),
.legend-link > button,
.legend-link > button:hover,
.legend-link > button:checked {
  background: none;
  background-image: none;
  border: none;
  box-shadow: none;
  outline: none;
}
/* A menu button's own button inside it keeps Adwaita's padding, which made
   the link's hover box half as wide again as its neighbours'. The outer one
   carries the box; this one only holds the glyph. */
.legend-link > button { min-width: 0; min-height: 0; padding: 0; }
/* A Pinned chart that just refused a rail click, for a moment: the pin is the
   reason nothing moved, so the pin is what lights up. */
.legend-link.refused {
  background: alpha(@accent_color, 0.35);
  border-radius: 5px;
}

/* One mode in the chain's popover. A menu row rather than a button: the icon
   carries the colour, so the row itself should stay out of the way and let
   the three read as a list. */
.link-row {
  padding: 3px 10px;
  min-height: 0;
  border-radius: 5px;
  font-size: 0.9em;
}
.link-row:hover { background: alpha(currentColor, 0.09); }
/* A mode's icon in that menu, for a mode other than the default: the accent,
   as the chart's button wears it. */
.link-mode-set { color: @accent_color; }

/* The symbol over the chart is a button, because clicking the name of the
   thing you are looking at to change it is the shortest route there is — but
   it should read as the title it replaced until you reach for it. */
.legend-symbol {
  background: none;
  border: none;
  box-shadow: none;
  padding: 0 4px;
  min-height: 0;
}
.legend-symbol:hover { background: alpha(currentColor, 0.12); }

/* The legend sits over the drawing, and the drawing is the point. The gear,
   and the link while the chart is in Focus (the default, so nothing to
   report), strike the maximize corner's bargain: not there until the pointer
   is on the chart. A Follow or Pinned chart keeps its link showing, since
   then the icon is telling you something. The link stays up while its menu is open,
   when the pointer has left the chart for the menu. */
.legend-gear,
.legend-link.quiet {
  opacity: 0;
  transition: opacity 120ms ease-out;
}
.chart-pane:hover .legend-gear,
.chart-pane:hover .legend-link.quiet,
.legend-link.quiet.open { opacity: 0.45; }
.chart-pane:hover .legend-gear:hover,
.chart-pane:hover .legend-link.quiet:hover { opacity: 1; }

/* The maximize corner is the bargain the gear strikes: it is not there at
   all until the pointer is on the chart, which is also the only moment it
   could be clicked. Four charts with a permanent button in each
   corner is four more things between you and the prices. */
.pane-expand {
  opacity: 0;
  background: none;
  background-image: none;
  border: none;
  box-shadow: none;
  transition: opacity 120ms ease-out;
}
.chart-pane:hover .pane-expand { opacity: 0.45; }
.chart-pane:hover .pane-expand:hover { opacity: 1; }
/* All three wait for a hand on the mouse: after the keyboard moves the
   focus, the window hushes them until the pointer moves, whatever it
   happens to be resting over. */
.hushed .chart-pane:hover .legend-gear,
.hushed .chart-pane:hover .legend-gear:hover,
.hushed .chart-pane:hover .legend-link.quiet:not(.open),
.hushed .chart-pane:hover .legend-link.quiet:not(.open):hover,
.hushed .chart-pane:hover .pane-expand,
.hushed .chart-pane:hover .pane-expand:hover { opacity: 0; }

/* The window's own controls — the watchlist toggle and the main menu — sit
   in the top-right corner, over whatever is there, instead of on a header
   bar. A header bar is a 47px band across the whole window; it held two
   buttons, a close button the window manager already provides for, and a
   title every chart writes in its own corner, and the band cost the charts
   that height. This is the HIG's overlaid-controls pattern: controls over
   content, attached to the window's edge, quiet until the pointer is near
   them. The box is also the window's drag handle, so the blank space at its
   start is what a header bar would have offered to be grabbed by.

   Three more pixels on top than below put the buttons on the line a chart
   writes its symbol and resolutions on, rather than above it. The window
   holds the box to the price axis's width, so its left edge is the axis's
   rule; see `CORNER_WIDTH`. Inside it, the two buttons touch and stand
   centred on the axis's column, more padding at the start than at the end
   because the end has the box's margin beyond it; the start is
   `CORNER_PAD_START`, which the rail's + is placed by.

   It stands a focus ring's width in from the window's edges: in a split the
   chart under it wears that ring, and a ground laid over the ring cut a
   notch out of the one thing saying which chart keys go to. */
.window-corner {
  margin: 2px 2px 0 0;
  padding: 8px 4px 7px 8px;
}
/* The ground the cluster stands on, for the half of the time it needs one.
   The chart draws its price axis right up into this corner — the last price is
   a filled tag, and a gridline label lands here as often as not — and two
   translucent things in the same pixels read as neither of them. Being on top
   is something a control has to look like, not only something it is.

   The chart's own colour rather than a panel colour: over an empty stretch of
   chart there is nothing to see, and where it does cover something it reads as
   the corner being cut out of the drawing rather than as a card laid on it.
   Translucent was tried first and is worse — a tenth of a filled price tag is
   a smudge under the icons, which is the same two-things-one-pixel problem in
   a quieter voice.

   Square, with the axis's rule down its left edge: it stands exactly on the
   axis's column, so the rule runs on up to the window's edge instead of
   stopping short under the buttons, and the corner reads as the top of the
   axis rather than a card laid across it.

   Only while the corner is over a chart. Over the rail there is nothing
   behind it to separate from: the rail's header has already stepped down out
   from under it, and a patch of the chart's colour there would be a square of
   the wrong grey. The window adds the class, because only it knows which. */
.window-corner.over-chart {
  background: @view_bg_color;
  box-shadow: inset 1px 0 @omacharts_axis;
}
/* The drawing tools' corner on the left, the mirror of the one on the right:
   centred over the bar it opens, which is as wide as one of its tools, and
   at the same height, so beside the chart's name it centres on the name. */
.window-corner.start {
  margin: 2px 0 0 2px;
  padding: 8px 9px 7px 7px;
}
.window-corner.start.over-chart {
  border-bottom-right-radius: 12px;
  box-shadow: none;
}
/* Beside a chart's name it is one more glyph in the row, and the name is
   what that corner is for: so it stays well under the name until the
   pointer comes to it. */
.window-corner.start.over-chart button { opacity: 0.25; }
.window-corner.start.over-chart:hover button { opacity: 1; }
/* Quiet until the pointer is near: on the buttons rather than the box, which
   is what leaves the ground at its own strength. */
.window-corner button {
  opacity: 0.75;
  transition: opacity 120ms ease-out;
}
.window-corner:hover button { opacity: 1; }

/* The drawing tools, down the left edge: a column of two, quiet until one is
   in hand, which lights it in the accent the way a pressed tool should. */
.drawing-bar { padding: 0; }
/* The rules between the bands of tools: present, and no louder than that. */
.drawing-tool-rule { background: alpha(@window_fg_color, 0.14); min-height: 1px; margin: 3px 7px; }
.drawing-tool { padding: 2px; border-radius: 8px; opacity: 0.8; }
.drawing-tool:hover { opacity: 1; }
.drawing-tool:checked { opacity: 1; background: alpha(@accent_bg_color, 0.25); box-shadow: inset 0 0 0 1px @accent_bg_color; }
/* A tool that stays in hand: the same lit button, held in a solid ring. The
   chart otherwise goes on drawing and nothing says why. */
.drawing-tool.drawing-tool-stuck:checked { background: alpha(@accent_bg_color, 0.38); box-shadow: inset 0 0 0 2px @accent_bg_color; }
.drawing-config-badge { font-size: 9px; font-weight: 700; min-width: 12px; min-height: 12px; padding: 0 2px; border-radius: 6px; background: @accent_bg_color; color: @accent_fg_color; margin: 0 -4px -4px 0; }
.drawing-preview { border-radius: 6px; }
.drawing-config-edited { font-size: 11px; font-weight: 700; padding: 1px 8px; border-radius: 9px; background: @accent_bg_color; color: @accent_fg_color; }
.drawing-config-row-edited { box-shadow: inset 3px 0 0 @accent_bg_color; }
.drawing-config-picker { padding: 0; }
/* A configuration row in the drawing's menu: a menu item's own measure,
   with a picture where the glyph would be. */
.drawing-config-row { padding: 4px 10px; min-height: 0; border-radius: 6px; }
/* The arrowhead row: four pictures side by side in a menu row, the one in
   use ringed. Tight, because four of them and a key have to fit the width a
   menu item already has. */
.drawing-head-row { padding: 2px 10px; }
.drawing-head-cell { padding: 2px; min-width: 0; min-height: 0; border-radius: 5px; opacity: 0.75; }
.drawing-head-cell:hover { opacity: 1; }
/* The Custom tile is the one in use: a button, so it is not greyed, but
   nothing happens on it. */
.drawing-preview-chosen { opacity: 1; }
.drawing-preview-hover { border-radius: 6px; transition: background 120ms ease-out, box-shadow 120ms ease-out; }
.drawing-config-picker:hover .drawing-preview-hover { background: alpha(@accent_bg_color, 0.14); box-shadow: inset 0 0 0 2px @accent_bg_color; }
.drawing-config-picker:active .drawing-preview-hover, .drawing-config-picker:checked .drawing-preview-hover { background: alpha(@accent_bg_color, 0.22); }
.drawing-save-as { font-size: 12px; opacity: 0.7; padding: 2px 6px; min-height: 0; }
.drawing-save-as:hover { opacity: 1; }
.drawing-config-picker > * { padding: 0; }
/* The tag on the picture is the chart's own palette turned inside out: the
   text colour as its ground and the chart colour as its ink, so it reads on
   the picture whatever the picture is. */
.drawing-config-tag { padding: 2px 10px; border-radius: 10px; font-weight: 700; font-size: 12px; background: @window_fg_color; color: @view_bg_color; }
.drawing-preview-current { box-shadow: 0 0 0 2px @accent_bg_color; }
/* Typing on the chart: nothing behind the glyphs, so what is under them is
   the chart. The caret is the accent, which is the one thing on a text
   widget that has to be found rather than read, and a ring in the same
   colour says where the words will end up without drawing a field around
   them. */
/* The view and its text layer are transparent so the chart shows through.
   The selection is NOT — it was in this list, under `selection:focus`, which
   is the selection you have while you are typing: every range you took was
   taken invisibly. */
.drawing-text-editor, .drawing-text-editor text { background: none; background-color: transparent; }
/* The caret is the accent, which is the one thing on a text widget that has
   to be found rather than read. Nothing else: the words are already in the
   drawing's own face, size and ink, over the drawing's own ground, and a
   border or a highlight round them would be the one thing on screen that
   changes when the edit is committed. A chart you can type on should not
   flinch when you press Enter. */
.drawing-text-editor { caret-color: @accent_bg_color; }
.drawing-text-editor text selection, .drawing-text-editor text selection:focus { background-color: alpha(@accent_bg_color, 0.45); color: @window_fg_color; }
/* The nine places, as a block of nine cells rather than nine buttons: no
   padding between them, one ring around the lot, and the lit one filled. */
.drawing-place-grid { padding: 2px; border-radius: 7px; box-shadow: inset 0 0 0 1px alpha(@window_fg_color, 0.18); }
.drawing-place-cell { padding: 1px; min-width: 0; min-height: 0; border-radius: 4px; opacity: 0.55; }
.drawing-place-cell:hover { opacity: 0.85; }
.drawing-place-cell:checked { opacity: 1; background: alpha(@accent_bg_color, 0.28); }

/* The corner sits over the rail when the rail is open, which is where the
   HIG puts a sidebar's menu (above the sidebar list), so the rail's band
   steps down out from under it. The band is the corner's height: a 26px
   button with 8px of padding above and 7px below, two pixels in from the
   window's edge, 43 in all. The step is 37 of that, to a pixel under the
   buttons.

   Under the step, the band's six pixels more, so the separator under the
   band clears the corner's hover pill and the name's descenders rather than
   touching them. The column headings used to fill this band; they are gone, and the
   band is as short as it can be without the line crowding what is above. */
.rail-header { margin-top: 37px; min-height: 6px; }

/* The two states of one mark on the heading row: where a column being
   dragged would land, and which boundary the pointer could drag to resize.
   Both are the colour the headings are written in — the window's foreground
   at the dimming Adwaita gives a `.dim-label` — so the mark belongs to the
   row rather than sitting on top of it. The hover one is the same grey,
   quieter, because it is the same thing not yet happening. */
/* The mark that says where a dragged column or symbol will land, in the
   colour the rail's own text is written in — the theme's foreground, which
   is what `window_fg_color` and `sidebar_fg_color` are both defined from —
   so it belongs to the row rather than sitting on top of it. */
.column-drop { border-radius: 1px; background: alpha(@window_fg_color, 0.42); }

/* And lying down, between the symbols, for the same reason: a line where
   the dragged one will be inserted says more than a frame round whichever
   row the pointer happens to be over, so the rows lose the frame. */
.row-drop { border-radius: 1px; background: alpha(@window_fg_color, 0.42); }
/* Nothing lights up under a drag. The row the pointer happens to be over
   is not where the thing is going — the line is — and a box drawn round it
   says otherwise, as well as giving the line an edge to look misaligned
   against. Hover only: a row that was selected before the drag stays
   selected through it. */
.dragging row:hover {
  background: none;
  background-image: none;
  box-shadow: none;
}

/* One line a symbol. Adwaita's sidebar row is 36px tall, sized for an icon
   beside a label, and a row here is one line of text; at that height a list
   of thirty did not fit on a laptop's screen. The radius comes down with the
   height, or a selected row reads as a pill. */
.navigation-sidebar > row.symbol-row,
.navigation-sidebar > row.list-row {
  min-height: 26px;
  margin-bottom: 1px;
  border-radius: 6px;
}

/* The chart's symbol, in the accent, filled. Adwaita marks it with a tenth
   of the foreground and lays its keyboard-focus ring on top, the accent at
   half strength: on a dark theme that is a grey-blue frame round a row
   barely lighter than its neighbours. Arrows move the selection with the
   focus, so the fill already says where the keys are and the ring goes —
   except on a row the focus reached without selecting it. */
.navigation-sidebar > row.symbol-row:selected {
  background-color: alpha(@accent_color, 0.22);
}
.navigation-sidebar > row.symbol-row:selected:hover {
  background-color: alpha(@accent_color, 0.28);
}
.navigation-sidebar > row.symbol-row:selected:focus-visible {
  outline: none;
}

/* The fold arrow on a section's header, at the height of the title beside
   it, so a header is no longer the tallest row in a rail of one-line rows. */
.section-fold { min-height: 0; min-width: 0; padding: 2px 4px; }

/* The lists put in order by dragging one entry onto another — a chart's
   indicators and header, the chartbook tabs. The frame GTK draws round the
   entry under the pointer says onto this; the drop puts the dragged one
   next to it, so the frame goes and a line on the edge it will land against
   takes its place, in the accent the frame was drawn in. Inset, so it is
   drawn inside the entry and moves nothing. */
.drop-marked:drop(active) {
  outline: none;
  box-shadow: none;
  background-image: none;
}
row.drop-marked.drop-before { box-shadow: inset 0 2px @accent_bg_color; }
row.drop-marked.drop-after { box-shadow: inset 0 -2px @accent_bg_color; }
/* Square on the marked side, or a tab's rounded corners bend the line into
   a bracket. */
:not(row).drop-marked.drop-before {
  box-shadow: inset 2px 0 @accent_bg_color;
  border-top-left-radius: 0;
  border-bottom-left-radius: 0;
}
:not(row).drop-marked.drop-after {
  box-shadow: inset -2px 0 @accent_bg_color;
  border-top-right-radius: 0;
  border-bottom-right-radius: 0;
}

.symbol-row:drop(active),
.section-row:drop(active) {
  outline: none;
  box-shadow: none;
  background: none;
  background-image: none;
}

/* The tick is the whole of the answer, so the headings do not also wear the
   frame GTK draws around a drop target. Only this row: the symbols below
   keep theirs, which is how you can tell a dropped symbol will land. */
.column-headings:drop(active) {
  outline: none;
  box-shadow: none;
  background: none;
  background-image: none;
}

/* The watchlist's name, in the band the corner controls already reserved,
   after the button that steps out to every list. Bold at the rail's own size:
   it is the title of what the rail is showing. */
.rail-title { font-weight: bold; }

/* The pencil and the bin a list's row shows under the pointer, held inside
   the row's height so the row does not grow when they appear. */
button.list-row-action { min-height: 0; min-width: 0; padding: 3px; }

/* A way back to a default, without shouting about it. */
.subtle-link {
  font-size: 0.85em;
  opacity: 0.6;
  padding: 2px 6px;
  min-height: 0;
}
.subtle-link:hover { opacity: 1; }
.subtle-link:disabled { opacity: 0.25; }

/* The two offers an empty rail makes. Bigger than a row on purpose: this is
   the first thing somebody sees after making a watchlist, and it should read
   as an invitation with some presence rather than as a caption apologising
   for the space. Buttons, so the keyboard can reach them, but Adwaita draws
   button text bold and bold here would shout. Only ever opacity on the text's
   own colour, so light and dark need no separate answer. */
.rail-empty-action {
  font-weight: normal;
  font-size: 1.05em;
  opacity: 0.7;
  padding: 10px 8px;
  min-height: 0;
  background: none;
  background-image: none;
  border: none;
  box-shadow: none;
  transition: opacity 120ms ease-out;
}
/* The sentence the block is built around, at the offers' own size so it does
   not read as a footnote to them. */
.rail-empty-blurb { font-size: 1.05em; }

.rail-empty-action:hover,
.rail-empty-action:focus-visible {
  opacity: 1;
  background: alpha(currentColor, 0.10);
  border-radius: 8px;
}

/* Keys read as keys. */
.keycap {
  font-size: 0.85em;
  font-feature-settings: 'tnum';
  padding: 2px 8px;
  border-radius: 6px;
  background: alpha(@card_fg_color, 0.08);
  border: 1px solid alpha(@card_fg_color, 0.10);
}

/* The chartbook strip, along the bottom, and only once there is a second
   book to switch to. A status line rather than a tab bar: the window manager
   puts its workspaces on a thin band at an edge and so does this, which is
   the idiom an Omarchy user reads without thinking. The bottom rather than
   the top because the top was given back to the charts when the header bar
   went, and because the time axis is already down here — the eye is not
   pulled off the prices to find it. One hairline above it, and nothing else:
   at this height a border on every side is a box, and a box is furniture. */
.chartbook-strip {
  /* No padding on the left: the drawing tools' tab sits flush with the
     window's edge, the way a tab on a drawer does. */
  padding: 1px 4px 1px 0;
  border-top: 1px solid alpha(currentColor, 0.08);
}
.chartbook-tab {
  padding: 2px 10px;
  margin: 1px;
  border-radius: 5px;
  font-size: 0.82em;
  opacity: 0.45;
  transition: opacity 120ms ease-out, background-color 120ms ease-out;
}
.chartbook-tab:hover { opacity: 0.8; background: alpha(currentColor, 0.06); }
/* The one you are in is the only one at full strength, which is the whole
   job of the strip — the rest are there to say they exist. */
.chartbook-tab.active { opacity: 1; background: alpha(currentColor, 0.10); }
.chartbook-rename {
  font-size: 0.82em;
  min-height: 0;
  padding: 0 4px;
  margin: 0;
}

.legend-row { padding: 0 0 1px 1px; }
.legend-indicator { font-size: 0.78em; opacity: 0.75; }
.legend-indicator-hidden { opacity: 0.35; text-decoration: line-through; }
.legend-button {
  opacity: 0;
  min-width: 18px;
  min-height: 18px;
  padding: 0;
  -gtk-icon-size: 12px;
}
.legend-row:hover .legend-button { opacity: 0.55; }
.legend-button:hover { opacity: 1; }

/* One box for every glyph button along the top of the window: a chart's
   link, gear and maximize, the window's toggle and menu, the rail's list
   button and +. They stand in a row and light under the same pointer, so
   three sizes and three corner radii in it read as three apps. 26px
   around a 16px icon, the same rounding, the same wash on hover, and a
   firmer one for the one that is on. Last in the sheet, so it settles each
   one's size whatever its own rule says. */
.window-corner button,
.legend-link,
.legend-gear,
.pane-expand,
button.rail-switcher,
button.rail-add {
  min-width: 22px;
  min-height: 22px;
  padding: 2px;
  border-radius: 6px;
}
.window-corner button:hover,
.legend-link:hover,
.legend-gear:hover,
.chart-pane:hover .pane-expand:hover,
button.rail-switcher:hover,
button.rail-add:hover { background: alpha(currentColor, 0.12); }
.window-corner button:checked,
button.rail-switcher.open { background: alpha(currentColor, 0.15); }
.window-corner button:checked:hover,
button.rail-switcher.open:hover { background: alpha(currentColor, 0.2); }
",
    );

    // Tiled charts borrow the window manager's idiom, since an Omarchy user
    // reads it without thinking: panes sit on a gap, the focused one wears a
    // thin border in the accent, and nothing else is drawn.
    let _ = write!(
        css,
        "
/* The gap between two charts is the window surface showing through, the way
   the gap between two windows shows the desktop, and it is also the handle
   that resizes them. libadwaita gives a wide handle a hairline along each of
   its edges, as inset shadows in its own border colour, which under a
   chart's time axis reads as a hairline, a band and a second hairline: a
   double separator. The shadows go, so the handle is one band of one colour.
   It stays wide because the band is then the whole hit area; the narrow
   handle is grabbed through an invisible extension over the first pixels of
   the charts beside it, which is exactly where the focus ring is. */
paned.chart-split > separator {{
  min-width: {gutter}px;
  min-height: {gutter}px;
  margin: 0;
  padding: 0;
  border: none;
  box-shadow: none;
  background: @omacharts_gutter;
  transition: background-color 120ms ease-out;
}}
paned.chart-split > separator:hover,
paned.chart-split > separator:active {{ background-color: @omacharts_gutter_hover; }}

/* The focused pane is the one keys, menus and the symbol search act on, so
   with four charts open it has to be told apart at a glance: a ring inside
   its edge, in the accent's hue at a lightness the engine chooses so that it
   is seen the same amount on every theme and is never the colour of the
   axis, the crosshair or a candle. Every pane carries the border, in the
   chart's own colour when unfocused, so focus moving between panes changes a
   colour and not a layout. Only inside a split: a chart on its own has
   nothing to be told apart from. */
.chart-pane {{
  background-color: @view_bg_color;
  border: {ring}px solid transparent;
}}
.chart-split .chart-pane.focused {{ border-color: @omacharts_focus; }}
/* The drawing tools stand beside the charts on the window's own ground,
   with the ring's width of it before the tools, so the toggle above stands
   centred over them. Their edge against the charts is the rail's against
   them: Adwaita's paned separator, a sixth of the text colour over the
   window's ground. */
.drawing-tools {{
  background: @window_bg_color;
  padding: {ring}px 0 0 {ring}px;
  box-shadow: inset -1px 0 alpha(@window_fg_color, 0.15);
}}
",
        gutter = GUTTER_WIDTH,
        ring = RING_WIDTH,
    );
    css
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_stylesheet_redefines_the_platform_colours() {
        let theme = &builtin_themes()[0];
        let css = stylesheet(theme, &omacharts_engine::theme::theme_bars(theme));
        for name in [
            "window_bg_color",
            "view_bg_color",
            "headerbar_bg_color",
            "card_bg_color",
            "popover_bg_color",
            "accent_bg_color",
            "borders",
        ] {
            assert!(css.contains(&format!("@define-color {name} ")), "missing {name}");
        }
        assert!(css.contains(&theme.ui.background));
        assert!(css.contains(&theme.ui.accent));
        assert!(css.contains("@define-color omacharts_up "));
        assert!(css.contains("@define-color omacharts_down "));
        // The marks on the heading row are the headings' own colour rather
        // than a colour of their own.
        assert!(css.contains(".column-drop { border-radius: 1px; background: alpha(@window_fg_color"));
        // The headings carry the drop tick alone; a frame as well is noise.
        assert!(css.contains(".column-headings:drop(active)"));
        assert!(css.contains(".symbol-row:drop(active)"));
        assert!(css.contains(".section-row:drop(active)"));
        assert!(css.contains(".dragging row:hover"));
    }

    #[test]
    fn the_frame_is_styled_from_the_engines_colours_and_widths() {
        let theme = &builtin_themes()[0];
        let scheme = omacharts_engine::theme::theme_bars(theme);
        let css = stylesheet(theme, &scheme);
        let frame = theme.frame(&scheme);
        for (name, value) in [
            ("omacharts_gutter", &frame.gutter),
            ("omacharts_gutter_hover", &frame.gutter_hover),
            ("omacharts_focus", &frame.focus),
        ] {
            assert!(css.contains(&format!("@define-color {name} {value};")), "missing {name}");
        }
        assert!(css.contains(&format!("min-width: {GUTTER_WIDTH}px;")));
        assert!(css.contains(&format!("border: {RING_WIDTH}px solid transparent;")));
        // Only a split shows focus; a lone chart has nothing to be told from.
        assert!(css.contains(".chart-split .chart-pane.focused { border-color: @omacharts_focus; }"));
        // And the wide handle's two hairlines are gone.
        assert!(css.contains("box-shadow: none;"));
    }

    #[test]
    fn accent_text_contrasts_with_the_accent() {
        assert_eq!(colors::readable_on("#ffffff"), "#000000");
        assert_eq!(colors::readable_on("#0d1117"), "#ffffff");
    }

    #[test]
    fn omarchy_is_the_default_when_present_and_midnight_when_not() {
        // Exercised through Theming::load against a store with no settings,
        // which is the only place the choice is made.
        let store = Store::memory().unwrap();
        let theming = Theming::load(&store);
        let expected = if omarchy::available(&home()) { OMARCHY_ID } else { FALLBACK_THEME_ID };
        assert_eq!(theming.theme_id(), expected);
        assert_eq!(theming.scheme_id(), THEME_BARS_ID);
    }

    #[test]
    fn a_missing_theme_falls_back_rather_than_panicking() {
        let store = Store::memory().unwrap();
        store.set_setting(SETTING_THEME, "deleted-by-another-window");
        let theming = Theming::load(&store);
        assert!(!theming.theme().id.is_empty());
    }

    #[test]
    fn a_scheme_with_colours_of_its_own_becomes_a_theme_wearing_them() {
        let store = Store::memory().unwrap();
        store.set_setting(SETTING_BARS, "accessible");
        let mut theming = Theming::load(&store);
        theming.carry_over_candle_colours(&store);
        let theme = theming.theme();
        assert_eq!(theme.source, omacharts_engine::theme::Source::Custom);
        assert!(theme.name.ends_with("with Blue / Orange"), "{}", theme.name);
        assert_eq!(theming.bar_scheme().up, "#4493f8");
        assert_eq!(theming.scheme_id(), THEME_BARS_ID);
        // And only once: loading again finds a colouring, not a scheme.
        let mut again = Theming::load(&store);
        again.carry_over_candle_colours(&store);
        assert_eq!(again.theme().id, theme.id);
        assert_eq!(store.custom_themes().len(), 1);
    }

    #[test]
    fn schemes_written_before_colourings_come_back_painted_the_same_way() {
        let store = Store::memory().unwrap();
        for (stored, read) in [
            ("hollow", omacharts_engine::theme::THEME_HOLLOW_ID),
            ("monochrome", omacharts_engine::theme::THEME_MONO_ID),
            ("theme-red-up", omacharts_engine::theme::THEME_RED_UP_ID),
            ("no-such-scheme", THEME_BARS_ID),
        ] {
            store.set_setting(SETTING_BARS, stored);
            assert_eq!(Theming::load(&store).bar_scheme().id, read, "{stored}");
        }
    }
}
