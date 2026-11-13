//! Choosing a theme and how candles look, and making the whole app wear them.
//!
//! On Omarchy the desktop theme is the default for both, and it is followed
//! live: the palette is re-read on a timer and the app repaints when it
//! changes. Any other Omarchy theme can be picked instead, and then stays put
//! whatever the desktop does; a machine without Omarchy picks from a few of
//! Omarchy's palettes built into the binary.
//!
//! "The whole app" is meant literally. The theme's colours are pushed into
//! libadwaita's own named colours, so headers, popovers, lists, dialogs and
//! the chart all come from one palette rather than the chart being a themed
//! island in a default-grey window.

use std::collections::BTreeSet;
use std::path::PathBuf;

use std::fmt::Write as _;

use omacharts_engine::frame::{GUTTER_WIDTH, RING_WIDTH};
use omacharts_engine::theme::{
    parse_bar_colour, spell_bar_colour, BarLook, BarScheme, Body, Mode, Theme,
    Thickness, TradingLook, FALLBACK_THEME_ID, OMARCHY_ID,
};
use omacharts_engine::Ink;
use omacharts_engine::omarchy::{self, Palette};

use crate::chart_look::ChartLook;
use crate::store::{home, Store};
use crate::ui::colors;

pub const SETTING_THEME: &str = "theme";

/// What the theme that follows the desktop is called on the command line:
/// the word every other setting on this platform uses, rather than its
/// stored id.
pub const SYSTEM_THEME: &str = "system";

/// A theme's id as the command line says it.
pub fn spell_theme(id: &str) -> &str {
    match id == OMARCHY_ID {
        true => SYSTEM_THEME,
        false => id,
    }
}

/// The id a theme typed on the command line is stored by: [`spell_theme`]
/// the other way round.
pub fn theme_id(word: &str) -> &str {
    match word.eq_ignore_ascii_case(SYSTEM_THEME) {
        true => OMARCHY_ID,
        false => word,
    }
}
/// The answers a candle's look is made of. The window and `config bars`
/// both write these, so there is no second spelling to drift.
pub const SETTING_BAR_UP: &str = "bar_up";
pub const SETTING_BAR_DOWN: &str = "bar_down";
/// A line's two shades: at its lowest close on screen, and at its highest.
const SETTING_BAR_LINE_LOW: &str = "bar_line_low";
const SETTING_BAR_LINE_HIGH: &str = "bar_line_high";
/// A body is stored as two switches, hollow and bordered, so the hollow
/// one an older version wrote still reads. Hollow wins where both are on,
/// which only a hand-edited row can make.
const SETTING_BAR_UP_HOLLOW: &str = "bar_up_hollow";
const SETTING_BAR_DOWN_HOLLOW: &str = "bar_down_hollow";
const SETTING_BAR_UP_BORDER: &str = "bar_up_border";
const SETTING_BAR_DOWN_BORDER: &str = "bar_down_border";
/// How many pixels wide each direction's OHLC bars are drawn.
const SETTING_BAR_UP_THICKNESS: &str = "bar_up_thickness";
const SETTING_BAR_DOWN_THICKNESS: &str = "bar_down_thickness";
/// Where an older version stored one thickness for both directions, read
/// until the bars are next changed, which writes the two above instead.
const LEGACY_OHLC_THICKNESS: &str = "ohlc_thickness";
/// Where an older version stored a named bar scheme, and the two switches
/// it wore over any scheme: up and down exchanged, and a rise drawn
/// hollow. Read while the rows above are unwritten, so an upgrade keeps
/// the bars somebody chose.
const LEGACY_BAR_SCHEME: &str = "bar_scheme";
const LEGACY_SWAP_COLOURS: &str = "swap_colours";
const LEGACY_HOLLOW_UP: &str = "hollow_up";
/// How orders and positions are drawn, as a [`TradingLook`] in JSON: one
/// for every theme, since a theme is Omarchy's and not ours to edit.
pub const SETTING_TRADING_LOOK: &str = "trading_look";

/// How orders and positions are drawn, as stored. Unset or unreadable, every
/// slot follows the theme.
pub fn trading_look(store: &Store) -> TradingLook {
    store.setting(SETTING_TRADING_LOOK).and_then(|json| serde_json::from_str(&json).ok()).unwrap_or_default()
}

/// How candles look, as stored.
pub fn bar_look(store: &Store) -> BarLook {
    let legacy = legacy_bar_look(store);
    let colour = |key: &str, fallback: omacharts_engine::ColorChoice| {
        store.setting(key).and_then(|text| parse_bar_colour(&text)).unwrap_or(fallback)
    };
    let body = |hollow: &str, border: &str, fallback: Body| {
        match (store.setting_bool(hollow, fallback == Body::Hollow), store.setting_bool(border, false)) {
            (true, _) => Body::Hollow,
            (false, true) => Body::Bordered,
            (false, false) => Body::Filled,
        }
    };
    let px = |key: &str| store.setting(key).and_then(|text| Thickness::parse(&text));
    let one = px(LEGACY_OHLC_THICKNESS).unwrap_or(legacy.up_thickness);
    BarLook {
        up: colour(SETTING_BAR_UP, legacy.up),
        down: colour(SETTING_BAR_DOWN, legacy.down),
        up_body: body(SETTING_BAR_UP_HOLLOW, SETTING_BAR_UP_BORDER, legacy.up_body),
        down_body: body(SETTING_BAR_DOWN_HOLLOW, SETTING_BAR_DOWN_BORDER, legacy.down_body),
        line_low: colour(SETTING_BAR_LINE_LOW, legacy.line_low),
        line_high: colour(SETTING_BAR_LINE_HIGH, legacy.line_high),
        up_thickness: px(SETTING_BAR_UP_THICKNESS).unwrap_or(one),
        down_thickness: px(SETTING_BAR_DOWN_THICKNESS).unwrap_or(one),
    }
}

/// Write them all, so what is stored is what is on screen whatever an older
/// version left behind.
pub fn save_bar_look(store: &Store, look: &BarLook) {
    store.set_setting(SETTING_BAR_UP, &spell_bar_colour(&look.up));
    store.set_setting(SETTING_BAR_DOWN, &spell_bar_colour(&look.down));
    store.set_setting(SETTING_BAR_LINE_LOW, &spell_bar_colour(&look.line_low));
    store.set_setting(SETTING_BAR_LINE_HIGH, &spell_bar_colour(&look.line_high));
    store.set_setting_bool(SETTING_BAR_UP_HOLLOW, look.up_body == Body::Hollow);
    store.set_setting_bool(SETTING_BAR_DOWN_HOLLOW, look.down_body == Body::Hollow);
    store.set_setting_bool(SETTING_BAR_UP_BORDER, look.up_body == Body::Bordered);
    store.set_setting_bool(SETTING_BAR_DOWN_BORDER, look.down_body == Body::Bordered);
    store.set_setting(SETTING_BAR_UP_THICKNESS, look.up_thickness.key());
    store.set_setting(SETTING_BAR_DOWN_THICKNESS, look.down_thickness.key());
    store.remove_setting(LEGACY_OHLC_THICKNESS);
}

/// The look an older version's named scheme stood for, with its switches
/// worn over it the way that version wore them: a switch and a scheme
/// saying the same thing say it once. A scheme somebody made themselves is
/// not carried over: those were copies with colour pickers, and the
/// default is a better guess than a half-read one.
fn legacy_bar_look(store: &Store) -> BarLook {
    use omacharts_engine::theme::{GRAPHITE, SAND, SILVER, SLATE};
    let swatches = |up: &str, down: &str| BarLook {
        up: omacharts_engine::ColorChoice::swatch(up),
        down: omacharts_engine::ColorChoice::swatch(down),
        ..BarLook::default()
    };
    let (mut look, swapped, hollow_up) = match store.setting(LEGACY_BAR_SCHEME).as_deref() {
        Some("theme-red-up") => (BarLook::default(), true, false),
        Some("theme-hollow" | "hollow") => (BarLook::default(), false, true),
        Some("theme-mono" | "monochrome") => (BarLook::monochrome(), false, false),
        Some("theme-soft") => (swatches(SLATE, SAND), false, false),
        Some("theme-quiet") => (swatches(SILVER, GRAPHITE), false, false),
        Some("accessible") => (swatches("Blue", "Orange"), false, false),
        _ => (BarLook::default(), false, false),
    };
    if swapped || store.setting_bool(LEGACY_SWAP_COLOURS, false) {
        std::mem::swap(&mut look.up, &mut look.down);
    }
    if hollow_up || store.setting_bool(LEGACY_HOLLOW_UP, false) {
        look.up_body = Body::Hollow;
    }
    look
}

pub struct Theming {
    home: PathBuf,
    /// Rebuilt from the desktop whenever its fingerprint moves.
    omarchy: Option<Theme>,
    omarchy_fingerprint: Option<String>,
    /// Every other theme on offer, each fixed at its palette: the machine's
    /// Omarchy themes, or the bundled ones where there are none. Derived
    /// as they are worn or listed.
    named: Vec<Palette>,
    theme_id: String,
    bars: BarLook,
    trading: TradingLook,
    provider: Option<gtk::CssProvider>,
    /// The themes charts have worn since the window opened, each given its
    /// colours under its class by `scoped` ([`Theming::cover`]). Only
    /// grows: a theme no chart wears any more costs a few lines, and keeps
    /// the sheet from being reloaded as charts change theme back and forth.
    worn: BTreeSet<String>,
    scoped: Option<gtk::CssProvider>,
}

impl Theming {
    pub fn load(store: &Store) -> Theming {
        let home = home();
        let omarchy = omarchy::current(&home);
        let omarchy_fingerprint = omarchy::fingerprint(&home);

        // Omarchy's theme is the default when the desktop has one, so a fresh
        // install already matches everything else on screen.
        let default_theme = if omarchy.is_some() { OMARCHY_ID } else { FALLBACK_THEME_ID };

        Theming {
            theme_id: store.setting(SETTING_THEME).unwrap_or_else(|| default_theme.to_string()),
            bars: bar_look(store),
            trading: trading_look(store),
            named: named_themes(&home),
            omarchy,
            omarchy_fingerprint,
            home,
            provider: None,
            worn: BTreeSet::new(),
            scoped: None,
        }
    }

    /// Every theme on offer: the one following the desktop, then the fixed
    /// ones.
    pub fn themes(&self) -> Vec<&Theme> {
        self.omarchy.iter().chain(self.named.iter().map(Palette::theme)).collect()
    }

    /// The theme following the desktop, while there is a desktop to follow.
    pub fn desktop(&self) -> Option<&Theme> {
        self.omarchy.as_ref()
    }

    /// The fixed themes on offer.
    pub fn named(&self) -> Vec<Theme> {
        self.named.iter().map(|p| p.theme().clone()).collect()
    }

    /// The active theme, owned, wearing the trading look.
    ///
    /// A theme can vanish — Omarchy uninstalled, a theme removed from it, or
    /// a name stored by an older version — so this falls back rather than
    /// refusing to draw.
    pub fn theme(&self) -> Theme {
        self.resolve(None)
    }

    /// The theme `look` is drawn on: its own, or the window's. A chart's
    /// theme that has vanished falls back to the window's.
    pub fn theme_of(&self, look: &ChartLook) -> Theme {
        self.resolve(look.theme.as_deref())
    }

    fn resolve(&self, id: Option<&str>) -> Theme {
        id.and_then(|id| self.find(id))
            .or_else(|| self.find(&self.theme_id))
            .or(self.omarchy.as_ref())
            .or_else(|| self.named.first().map(Palette::theme))
            .cloned()
            .unwrap_or_else(|| omarchy::bundled().swap_remove(0).into_theme())
    }

    /// The theme on offer called `id`, deriving that one and no other.
    fn find(&self, id: &str) -> Option<&Theme> {
        self.omarchy
            .as_ref()
            .filter(|t| t.id == id)
            .or_else(|| self.named.iter().find(|p| p.id() == id).map(Palette::theme))
    }

    /// The candles' colours on the active theme.
    pub fn bar_scheme(&self) -> BarScheme {
        self.bars.scheme(&self.theme())
    }

    pub fn bar_look(&self) -> &BarLook {
        &self.bars
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
        theme.ink(&self.bars.scheme(&theme))
    }

    /// How orders and positions are drawn as somebody set it: what an edit
    /// starts from, since a slot left unset keeps following the theme.
    pub fn trading_look(&self) -> &TradingLook {
        &self.trading
    }

    pub fn set_trading_look(&mut self, look: TradingLook, store: &Store) {
        if let Ok(json) = serde_json::to_string(&look) {
            store.set_setting(SETTING_TRADING_LOOK, &json);
        }
        self.trading = look;
    }

    /// Re-read which theme and bars are chosen, for a choice made
    /// outside this process.
    ///
    /// A command from a terminal writes the setting and nothing else; the
    /// window is still wearing whatever it started in, and would write its own
    /// choice back over the new one the next time anything touched it.
    /// Whether any of it changed, which the stylesheet wears.
    pub fn reload_selection(&mut self, store: &Store) -> bool {
        let default_theme = if self.omarchy.is_some() { OMARCHY_ID } else { FALLBACK_THEME_ID };
        let theme_id = store.setting(SETTING_THEME).unwrap_or_else(|| default_theme.to_string());
        let (bars, trading) = (bar_look(store), trading_look(store));
        let changed = (&theme_id, &bars, &trading) != (&self.theme_id, &self.bars, &self.trading);
        (self.theme_id, self.bars, self.trading) = (theme_id, bars, trading);
        changed
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
        // A theme installed or removed shows the next time this is asked,
        // which is when somebody switches themes and is looking.
        self.named = named_themes(&self.home);
        true
    }

    /// Push the active theme into libadwaita's named colours, and the
    /// themes charts wear under their classes, on the bars as they are now.
    pub fn apply(&mut self) {
        let theme = self.theme();

        adw::StyleManager::default().set_color_scheme(match theme.mode {
            Mode::Dark => adw::ColorScheme::ForceDark,
            Mode::Light => adw::ColorScheme::ForceLight,
        });

        let mut css = stylesheet(&theme, &self.bar_scheme());
        css.push_str(&trading_css(&theme, &self.trading, &self.bars));
        // Above the platform stylesheet, so our named colours win.
        provider(&mut self.provider, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION + 1).load_from_data(&css);
        self.load_scoped();
    }

    /// Give a chart wearing theme `id` that theme's colours under its
    /// class: the sheet is reloaded only the first time a chart wears it.
    pub fn cover(&mut self, id: &str) {
        if !self.worn.contains(id) {
            self.worn.insert(id.to_string());
            self.load_scoped();
        }
    }

    /// The worn themes' colours, apart from the window's sheet so a chart
    /// changing theme reloads only these few lines. One above it, so under
    /// a chart's class its theme wins, as it did when both were one sheet
    /// and this came last.
    fn load_scoped(&mut self) {
        let css = scoped_stylesheet(self.worn.iter().filter_map(|id| self.find(id)), &self.bars);
        provider(&mut self.scoped, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION + 2).load_from_data(&css);
    }
}

/// The provider in `slot`, added to the display at `priority` the first
/// time it is asked for.
fn provider(slot: &mut Option<gtk::CssProvider>, priority: u32) -> &gtk::CssProvider {
    slot.get_or_insert_with(|| {
        let provider = gtk::CssProvider::new();
        popovers_without_arrows();
        if let Some(display) = gtk::gdk::Display::default() {
            gtk::style_context_add_provider_for_display(&display, &provider, priority);
        }
        provider
    })
}

/// Every popover opens without an arrow: a menu opens at the pointer and
/// has nothing to point at, and one popover with an arrow beside one
/// without reads as two apps. GTK sizes the arrow in code, not CSS, so it
/// is turned off as each popover is realized, before it is measured. That
/// reaches the popovers inside library widgets too (a combo row's list, a
/// menu button's menu), which no call where a popover is built could.
fn popovers_without_arrows() {
    use gtk::glib::{self, gobject_ffi::{GSignalInvocationHint, GValue, g_signal_add_emission_hook, g_signal_lookup}};
    use gtk::prelude::*;
    use gtk::glib::translate::IntoGlib;

    unsafe extern "C" fn realized(
        _hint: *mut GSignalInvocationHint,
        _count: u32,
        params: *const GValue,
        _data: glib::ffi::gpointer,
    ) -> glib::ffi::gboolean {
        // The first parameter of a signal is the instance it was emitted on.
        let instance = unsafe { &*params.cast::<glib::Value>() };
        if let Ok(popover) = instance.get::<gtk::Popover>() {
            popover.set_has_arrow(false);
        }
        // Stay installed.
        glib::ffi::GTRUE
    }

    // A signal exists once its class does, and the first theme is applied
    // before any widget has been built. Once, whoever applies a theme.
    static ONCE: std::sync::Once = std::sync::Once::new();
    let Some(_class) = glib::Class::<gtk::Popover>::from_type(gtk::Popover::static_type()) else { return };
    ONCE.call_once(|| unsafe {
        let realize = g_signal_lookup(c"realize".as_ptr(), gtk::Popover::static_type().into_glib());
        g_signal_add_emission_hook(realize, 0, Some(realized), std::ptr::null_mut(), None);
    });
}

/// How far in from its left edge the window's corner starts its buttons:
/// the `padding-left` of `.window-corner` below, which is what centres the
/// pair on the axis's column, and what the rail's + is placed by. The two
/// change together.
pub(crate) const CORNER_PAD_START: i32 = 8;

/// How far down the window's corner stands its buttons: the `margin-top`
/// and `padding-top` of `.window-corner` below, 2 + 8. The rail's + stands
/// at the same height, beside the toggle. The two change together.
pub(crate) const CORNER_BUTTON_TOP: i32 = 10;

/// The trading look's colours as named colours, and the classes a signed
/// P&L, a side and a position are written in with them: the one pair for
/// money, the other for a side, as the chart's marks wear them, on bars
/// that look like `bars`.
pub fn trading_css(theme: &Theme, trading: &TradingLook, bars: &BarLook) -> String {
    let look = omacharts_engine::theme_trading(theme, trading, bars);
    format!(
        "@define-color omacharts_profit {};\n@define-color omacharts_loss {};\n\
         @define-color omacharts_buy {};\n@define-color omacharts_sell {};\n\
         @define-color omacharts_long {};\n@define-color omacharts_short {};\n\
         .pnl-gain {{ color: @omacharts_profit; }}\n.pnl-loss {{ color: @omacharts_loss; }}\n\
         .trade-buy {{ color: @omacharts_buy; }}\n.trade-sell {{ color: @omacharts_sell; }}\n\
         .trade-long {{ color: @omacharts_long; }}\n.trade-short {{ color: @omacharts_short; }}\n",
        look.profit, look.loss, look.buy, look.sell, look.long, look.short
    )
}

/// The fixed themes on offer: this machine's Omarchy themes, or where it has
/// none, the ones built in.
fn named_themes(home: &std::path::Path) -> Vec<Palette> {
    let installed = omarchy::installed(home);
    match installed.is_empty() {
        true => omarchy::bundled(),
        false => installed,
    }
}

/// The colours a theme hands the window, by libadwaita's names and our own.
///
/// One list. The window gets it as named colours and as properties at its
/// root; a chart on a theme of its own gets it again as properties under
/// its class ([`scoped_stylesheet`]). The rules read the properties
/// ([`read_as_properties`]), so either answer reaches them.
fn named_colours(theme: &Theme, scheme: &BarScheme) -> Vec<(&'static str, String)> {
    let ui = &theme.ui;
    let accent_fg = colors::readable_on(&ui.accent);
    let frame = theme.frame(scheme);

    let mut colours = Vec::with_capacity(32);
    let mut define = |name: &'static str, value: &str| colours.push((name, value.to_string()));

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
    // Menus, popovers, the picker and the question cards on the rail's own
    // ground: a lighter slab read as something pasted on from another app.
    // The hairline and the shadow set them apart, and a breath of the text
    // colour lifts them off it just enough to read as on top.
    define("popover_bg_color", &format!("mix({}, {}, 0.03)", ui.surface, ui.text));
    define("popover_fg_color", &ui.text);
    define("borders", &ui.border);

    // Accent. libadwaita 1.6 split the standalone accent from the filled one.
    define("accent_bg_color", &ui.accent);
    define("accent_fg_color", accent_fg);
    define("accent_color", &ui.accent);

    // Rising and falling text follows the bars, so a watchlist agrees
    // with the candles beside it.
    define("omacharts_up", &scheme.up);
    define("omacharts_down", &scheme.down);
    // The palette's own red, orange and green, for anything that stands
    // for bad, pending and good rather than for down and up: the data
    // feed's dot wears them. The chart's trouble dot is the same Rose.
    let swatch = |name: &str, otherwise: &str| {
        theme.swatch(name).map(|swatch| swatch.hex.clone()).unwrap_or_else(|| otherwise.to_string())
    };
    define("omacharts_rose", &swatch("Rose", &scheme.down));
    define("omacharts_orange", &swatch("Orange", &ui.accent));
    define("omacharts_green", &swatch("Green", &scheme.up));

    // The frame around tiled charts. Derived, not taken: the engine explains
    // why in `frame.rs`, and `doc/themes/frames.html` shows the result on
    // every theme.
    define("omacharts_gutter", &frame.gutter);
    define("omacharts_gutter_hover", &frame.gutter_hover);
    define("omacharts_focus", &frame.focus);
    // The chart's price-axis rule, for the corner that stands on it.
    define("omacharts_axis", &ui.axis);
    colours
}

/// `window_bg_color` as a custom property: `--window-bg-color`, the name
/// libadwaita's own rules read.
fn property(name: &str) -> String {
    format!("--{}", name.replace('_', "-"))
}

/// The class a chart wearing theme `id` carries, under which
/// [`scoped_stylesheet`] gives that theme's colours.
pub fn theme_class(id: &str) -> String {
    let slug: String = id.chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' }).collect();
    format!("theme-{slug}")
}

/// Every theme's colours under its [`theme_class`], so a chart can wear a
/// theme the window does not: its header, legend and controls take the
/// chart's theme the way its canvas does. Custom properties inherit, and
/// libadwaita's rules and ours read them, so one block per theme is all.
pub fn scoped_stylesheet<'a>(themes: impl IntoIterator<Item = &'a Theme>, bars: &BarLook) -> String {
    let mut css = String::new();
    for theme in themes {
        let _ = write!(css, ".{} {{", theme_class(&theme.id));
        for (name, value) in named_colours(theme, &bars.scheme(theme)) {
            let _ = write!(css, " {}: {value};", property(name));
        }
        css.push_str(" }\n");
    }
    css
}

/// The window's stylesheet, on `theme` with `scheme`'s candles.
pub fn stylesheet(theme: &Theme, scheme: &BarScheme) -> String {
    let colours = named_colours(theme, scheme);
    let mut css = String::with_capacity(2048);
    // Named colours too, for libadwaita's rules that read a name rather
    // than its property.
    for (name, value) in &colours {
        let _ = writeln!(css, "@define-color {name} {value};");
    }
    css.push_str(":root {");
    for (name, value) in &colours {
        let _ = write!(css, " {}: {value};", property(name));
    }
    css.push_str(" }\n");

    // Everything after the root block is rules, whoever adds them, and
    // reads the colours as properties.
    let rules_from = css.len();
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
/* The instrument's name under the header: the symbol says which, this says
   what, so it stays out of the way. Indented to the symbol's own text, past
   the button's padding. */
.legend-name { font-size: 0.85em; opacity: 0.55; margin: 0 0 2px 5px; }
.delay-chip {
  font-size: 0.75em;
  padding: 1px 6px;
  border-radius: 6px;
  background: alpha(@card_fg_color, 0.08);
  opacity: 0.75;
}

/* The Look part's mark, with the room the words beside it keep. */
.look-mark { margin: 0 5px; }

/* Timeframe buttons: flat strip, the active one filled. */
.timeframe-strip { padding: 2px; }
.timeframe-strip button { min-width: 34px; padding: 2px 6px; }

/* Where a symbol trades: quieter than its name, beside what it is. */
.pick-venue {
  font-family: monospace;
  font-size: 0.78em;
  opacity: 0.45;
}

/* The row offering a symbol nobody listed. Quieter than a real match, because
   it is a guess rather than an answer. */
.symbol-row-unlisted .pick-name { font-style: italic; }
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

/* Windows that hold settings or a form (Preferences, and any form big
   enough to get a window of its own): rows on the page's own ground with
   a hairline between them, rather than libadwaita's rounded cards. A card says this is an object;
   a list of settings is not a pile of objects, and a card round every pair
   of rows is what made the window look busy and dated. */
.form-window list.boxed-list {
  background: none;
  box-shadow: none;
  border-radius: 0;
}
.form-window list.boxed-list > row {
  background: none;
  border-radius: 0;
  border-bottom: 1px solid alpha(@window_fg_color, 0.08);
}
.form-window list.boxed-list > row:last-child { border-bottom: none; }
/* No margin under a page's last group: it is scrolled content, so a page
   whose rows fit scrolled by however much of it did not. */
.form-window preferencespage > scrolledwindow > viewport > clamp > box { margin-bottom: 0; }
/* The group's title and description start where the rows' text does. */
.form-window preferencesgroup > box > box.header {
  padding-left: 12px;
  padding-right: 12px;
}
.form-window list.boxed-list > row.activatable:hover {
  background: alpha(@window_fg_color, 0.04);
}
/* Group titles as small labels, so the settings are what the eye lands on. */
.form-window preferencesgroup > box > box.header label.heading {
  font-size: 0.75em;
  font-weight: 700;
  letter-spacing: 0.08em;
  text-transform: uppercase;
  opacity: 0.6;
}
/* A row's action is a word in the accent colour, not a grey slab. Opted
   into, so a choice or a control that only happens to carry words keeps
   its own look. */
.form-window row button.action-word {
  background: none;
  box-shadow: none;
  color: @accent_color;
  font-weight: normal;
  padding: 4px 10px;
}
.form-window row button.action-word:hover {
  background: alpha(@accent_color, 0.12);
}
.form-window row button.action-word.destructive-action { color: @destructive_color; }
.form-window row button.action-word.destructive-action:hover {
  background: alpha(@destructive_color, 0.12);
}
/* A colour is its dot, its name and a chevron, as quiet as a dropdown's
   value until pointed at; a bar's body and thickness beside it, their
   picture, their word and a chevron. */
menubutton.colour-chip > button,
dropdown.bar-choice > button {
  background: none;
  border-radius: 6px;
  padding: 0 6px;
  min-height: 26px;
  font-weight: normal;
  color: alpha(@window_fg_color, 0.60);
}
menubutton.colour-chip > button:hover,
menubutton.colour-chip > button:checked,
dropdown.bar-choice > button:hover,
dropdown.bar-choice > button:checked {
  background: alpha(@window_fg_color, 0.08);
  color: @window_fg_color;
}
menubutton.colour-chip image,
dropdown.bar-choice > button image { -gtk-icon-size: 10px; min-width: 10px; min-height: 10px; }
/* In the list, only the answer picked shows its tick. */
popover.menu listview > row:not(:selected) image.tick { opacity: 0; }
/* The Bars card's head: its styles as tabs, a size up from the rows they
   decide, the one in use underlined in the accent and the others dimmed,
   as every tab is. On the row's bottom edge, so the underline sits on the
   hairline under it. */
row.bars-head { padding: 6px 12px 0; }
row.bars-head button {
  min-height: 0;
  padding: 8px 2px 9px;
  margin-left: 22px;
  border-radius: 0;
  border-bottom: 2px solid transparent;
  box-shadow: none;
  background: none;
  font-size: 15px;
  font-weight: 600;
  color: alpha(@window_fg_color, 0.60);
}
row.bars-head button:hover { color: @window_fg_color; }
row.bars-head button:checked { color: @window_fg_color; border-bottom-color: @accent_color; }
/* The header as a picture of a chart's top: on the charts' own ground, its
   parts lit on hover, and the parts it does not show waiting underneath,
   drawn the same way. While a part is dragged the empty places that would
   take it say their names, and the one it would land in turns to the
   accent; no boxes. One that would not stays shut. */
.header-card {
  background-color: @view_bg_color;
  border: 1px solid alpha(@window_fg_color, 0.12);
  border-radius: 12px;
}
.header-card .header-zone { border-radius: 8px; }
/* The picture is a chart's top at a settings column's width, a fraction of
   a chart's, so its parts are drawn small enough to keep their places
   apart; the ones waiting underneath match them. */
.header-card .header-zone, .header-card .header-spare { font-size: 0.8em; }
.header-card .timeframe-strip button { min-width: 22px; min-height: 20px; padding: 0 4px; }
.header-card .trade-button { min-height: 20px; padding: 0 6px; }
.header-card.dragging .header-zone:not(.shut) { min-width: 100px; }
.header-card .zone-name {
  font-size: 0.75em;
  letter-spacing: 0.08em;
  color: alpha(@window_fg_color, 0.35);
}
.header-card .header-zone.drop-over .zone-name { color: @accent_color; }
.header-card .header-chip { border-radius: 6px; }
.header-card .header-chip:hover { background: alpha(@window_fg_color, 0.06); }
.header-card .lifted { opacity: 0.25; }
.header-card .header-icon { min-width: 20px; min-height: 20px; color: alpha(@window_fg_color, 0.6); -gtk-icon-size: 13px; }
.header-card .header-spare {
  border: 1px solid alpha(@window_fg_color, 0.14);
  border-radius: 8px;
  padding: 0 4px;
  min-height: 26px;
}
.header-card .header-spare:hover {
  border-color: alpha(@window_fg_color, 0.3);
  background: alpha(@window_fg_color, 0.04);
}
/* A part's name in small caps, set apart from the picture beside it, which
   is often words too. Dropped onto the picture's baseline by a fixed nudge,
   the same on every card: an icon has no baseline to align to. */
.header-card .spare-name {
  font-size: 0.64em;
  font-weight: 700;
  letter-spacing: 0.08em;
  text-transform: uppercase;
  color: alpha(@window_fg_color, 0.5);
  margin: 4px 4px 0 0;
}
.header-card .header-spares.drop-over { background: alpha(@window_fg_color, 0.04); }
/* The sidebar sits on the charts' own ground, a step darker than the page. */
.form-window .sidebar-pane { background-color: @view_bg_color; }
/* The two theme cards, and the catalogue's tiles. */
.preferences-window button.theme-card {
  padding: 10px;
  border-radius: 12px;
  background: alpha(@window_fg_color, 0.04);
  box-shadow: inset 0 0 0 1px alpha(@window_fg_color, 0.08);
}
.preferences-window button.theme-card:hover { background: alpha(@window_fg_color, 0.07); }
.preferences-window button.theme-card.selected,
.preferences-window button.theme-tile.selected {
  background: alpha(@accent_color, 0.10);
  box-shadow: inset 0 0 0 2px @accent_color;
}
.preferences-window .theme-card .card-dot {
  min-width: 16px;
  min-height: 16px;
  border-radius: 50%;
  box-shadow: inset 0 0 0 1.5px alpha(@window_fg_color, 0.35);
}
.preferences-window .theme-card.selected .card-dot { box-shadow: inset 0 0 0 5px @accent_color; }
.preferences-window .theme-card label,
.preferences-window .theme-tile label { font-weight: normal; }
.preferences-window .theme-card label.title { font-weight: 600; }
.preferences-window .theme-card .detail,
.preferences-window .theme-catalogue .detail { font-size: 0.9em; opacity: 0.6; }
.preferences-window .theme-preview { border-radius: 8px; }
.preferences-window .theme-card grid .theme-preview { border-radius: 5px; }
.preferences-window button.theme-tile { padding: 6px; border-radius: 10px; }
/* The scope rule over the visual sections, in Preferences and in a chart's
   settings alike, a size under the other rules so its four words keep some
   air. The choice is two of its words; the picked one reads, the other
   fades, and Local, once picked, is in the accent, since it is the
   exception to every other page. */
.scope-rule label { font-size: 0.68rem; }
.scope-rule button {
  min-height: 0;
  min-width: 0;
  padding: 2px 3px;
  border-radius: 5px;
}
.scope-rule button label { opacity: 0.45; }
.scope-rule button:checked { background: none; }
.scope-rule button:checked label { opacity: 1; color: @window_fg_color; }
.scope-rule button.scope-local:checked label { color: @accent_color; }
.scope-rule button:hover { background: alpha(@window_fg_color, 0.06); }
/* The reset after LOCAL: as quiet as the words beside it until pointed at. */
.scope-rule button.scope-reset { padding: 1px; color: alpha(@window_fg_color, 0.55); }
.scope-rule button.scope-reset:hover { color: @accent_color; }

/* Menus and popovers are one surface. A right-click menu and the link
   popover open beside each other often enough that two looks for one kind
   of thing read as two apps. The window's own ground, a hairline edge, a
   soft shadow, and no arrow (`popovers_without_arrows`, since GTK sizes
   the arrow in code). */
popover.menu > contents,
popover.background > contents {
  background-color: @popover_bg_color;
  border: 1px solid alpha(@window_fg_color, 0.10);
  border-radius: 10px;
  box-shadow: 0 6px 18px alpha(black, 0.35), 0 1px 3px alpha(black, 0.2);
}
popover.background:not(.menu) > contents { padding: 6px; }
popover.menu > contents > stack > box,
popover.menu > contents > scrolledwindow > viewport > stack > box { padding: 4px; }
/* A menu row is a rail row: 26px, where Adwaita's 32px and its padding made
   the chart's menu taller than a laptop's chart. The hover is the rail's
   selection, so the row under the pointer reads the way a chosen symbol
   does. */
popover.menu modelbutton {
  min-height: 26px;
  padding: 0 8px 0 6px;
  border-radius: 6px;
}
popover.menu modelbutton:hover,
popover.menu modelbutton:selected { background-color: alpha(@accent_color, 0.22); }
popover.menu modelbutton:active { background-color: alpha(@accent_color, 0.30); }
/* Sections part on a hairline, the one the settings rows use. */
popover.menu separator {
  margin: 4px 6px;
  min-height: 1px;
  background: alpha(@window_fg_color, 0.08);
}
/* The key is a reminder, not part of the name: dim, small, and in the face
   keys are written in, so a column of them lines up. */
popover.menu accelerator {
  font-family: monospace;
  font-size: 0.8em;
  color: alpha(@popover_fg_color, 0.55);
}
/* A choice is a tick in the accent, whether the item is a toggle or one of
   several: a ring beside every option in a submenu is a column of noise
   round the one that matters. */
popover.menu check:checked,
popover.menu radio:checked { color: @accent_color; }
popover.menu radio { border: none; padding: 2px; }
popover.menu radio:checked { -gtk-icon-source: -gtk-icontheme('object-select-symbolic'); }
popover.menu radio:not(:checked) { -gtk-icon-source: none; }
/* A dropdown's list is the same menu: its rows are menu rows, and the one
   picked carries the same tick. Only the hover fills a row: the picked one
   is the list's selection, and filling it too would say it twice. */
popover.menu listview > row {
  min-height: 26px;
  padding: 0 8px;
  border-radius: 6px;
}
popover.menu listview > row:selected { background: none; }
popover.menu listview > row:hover { background-color: alpha(@accent_color, 0.22); }
popover.menu listview > row image { color: @accent_color; }

/* Every control speaks quietly, in one voice: what is picked reads, what is
   not fades, and the accent marks the choice. A control that shouts (a
   bright slab of a switch, a filled pill of a tab) pulls the eye off the
   setting it changes. Some selectors reach into libadwaita's own widgets
   (a combo row's list, a view switcher's box, a switch's glyphs), checked
   against libadwaita 1.10.
   A dropdown is its value and a chevron, a step dimmer than the title, so
   the row reads as a setting rather than a field to fill in. */
row.combo listview.inline,
row.combo .dropdown-arrow { color: alpha(@window_fg_color, 0.60); }
row.combo:hover listview.inline,
row.combo:hover .dropdown-arrow { color: @window_fg_color; }
/* A switch is the accent only when on. Off, its track is bright enough
   to read as a control on the row rather than a smudge beside it. */
switch {
  min-width: 0;
  min-height: 0;
  padding: 3px;
  border: none;
  border-radius: 99px;
  box-shadow: none;
  background-color: alpha(@window_fg_color, 0.22);
}
switch:hover { background-color: alpha(@window_fg_color, 0.28); }
switch:checked { background-color: @accent_bg_color; }
switch > slider {
  min-width: 16px;
  min-height: 16px;
  margin: 0;
  border: none;
  border-radius: 50%;
  box-shadow: none;
  background-color: @window_fg_color;
}
switch:checked > slider { background-color: @view_bg_color; }
/* Its on and off glyphs stay hidden, and take no room: GTK sizes a switch
   by the larger of them and its knob. */
switch > image { color: transparent; -gtk-icon-size: 1px; }
/* A menu of actions behind a word (Add a data feed…, Save as…) is that
   word in the accent, as a row's action is, its chevron saying it opens.
   An Add… word under a group's rows is the same word, its text lined up
   with the rows' text, 12px in, 8px of which is the word's own padding. */
.add-word { margin-left: 4px; }
button.add-word,
menubutton > button.text-button {
  min-height: 28px;
  padding: 0 8px;
  border-radius: 6px;
  box-shadow: none;
  background: none;
  font-weight: normal;
  color: @accent_color;
}
button.add-word:hover,
menubutton > button.text-button:hover,
menubutton > button.text-button:checked { background-color: alpha(@accent_color, 0.12); }
/* Tabs are words: the one shown reads, underlined in the accent, the
   others fade. Both kinds of switcher give every tab the widest one's
   width, so the underline is the word's (and its icon's), to hug it. */
stackswitcher.linked > button,
viewswitcher button {
  min-height: 26px;
  padding: 0 9px;
  border-radius: 0;
  box-shadow: none;
  background: none;
  font-weight: 600;
  color: alpha(@window_fg_color, 0.38);
}
stackswitcher.linked > button > label,
viewswitcher button > stack > box.wide {
  padding: 3px 1px;
  border-bottom: 2px solid transparent;
}
stackswitcher.linked > button:hover,
viewswitcher button:hover { color: alpha(@window_fg_color, 0.60); }
stackswitcher.linked > button:checked,
viewswitcher button:checked { color: @window_fg_color; }
stackswitcher.linked > button:checked > label,
viewswitcher button:checked > stack > box.wide { border-bottom-color: @accent_color; }
/* A choice of two or three, all in view (see `controls::Hours`): the same
   words as the tabs, a size down, without the underline. */
.words { border-spacing: 2px; }
.words button {
  min-height: 26px;
  min-width: 0;
  padding: 0 6px;
  border-radius: 5px;
  color: alpha(@window_fg_color, 0.38);
}
.words button:hover { background-color: alpha(@window_fg_color, 0.08); }
.words button:checked { background: none; color: @window_fg_color; font-weight: 600; }
.words button:checked:hover { background-color: alpha(@window_fg_color, 0.08); }
.words label.dot { color: alpha(@window_fg_color, 0.38); }
/* A setting's slider is a hairline: a 2px rail, the accent up to a small
   knob, a tick where it started, and its value after it in a dropdown's
   dim. The trough is the knob's height so all three centre on the rail;
   the tick is GTK's own mark, set over the trough rather than beside it,
   and drawn before it, so the knob covers it. */
scale.hairline { padding: 4px 0; }
scale.hairline > trough {
  min-width: 150px;
  min-height: 10px;
  margin: 0;
  padding: 0;
  border: none;
  border-radius: 0;
  box-shadow: none;
  outline: none;
  background: linear-gradient(alpha(@window_fg_color, 0.22), alpha(@window_fg_color, 0.22)) center / 100% 2px no-repeat;
}
scale.hairline > trough > highlight {
  min-height: 0;
  margin: 4px 0;
  border: none;
  border-radius: 1px;
  background: @accent_color;
}
scale.hairline > trough > slider {
  min-width: 10px;
  min-height: 10px;
  margin: 0;
  border: none;
  border-radius: 50%;
  box-shadow: none;
  background: @window_fg_color;
}
scale.hairline:focus-visible > trough > slider { outline: 2px solid alpha(@accent_color, 0.5); outline-offset: 1px; }
scale.hairline > marks.top { margin: 0 0 -10px; }
scale.hairline > marks > mark > indicator {
  min-width: 1px;
  min-height: 10px;
  background-color: alpha(@window_fg_color, 0.30);
}
scale.hairline > value {
  min-width: 36px;
  margin: 0 0 0 10px;
  color: alpha(@window_fg_color, 0.60);
  font-variant-numeric: tabular-nums;
}

/* A picker over a chart (see `pick_frame`) is the same surface with no title
   bar: the search box is its top edge, on a hairline, and the chart it will
   change is named at the end of it. The box takes no frame of its own, the
   surface already being one. */
popover.pick-frame > contents { padding: 0; }
.pick-query {
  padding: 4px 12px 4px 4px;
  border-bottom: 1px solid alpha(@window_fg_color, 0.08);
}
.pick-query entry,
.pick-query entry:focus-within {
  background: none;
  box-shadow: none;
  outline: none;
  font-size: 1.08em;
}
.pick-target { font-family: monospace; font-size: 0.78em; opacity: 0.55; }
.pick-list { padding: 4px; background: none; }
/* A picker's rows are the rail's: 26px, the ticker in the face numbers and
   keys are written in, the name a dim reminder beside it, and the rail's
   tint for the one Enter would take. */
.pick-list.navigation-sidebar > row {
  min-height: 26px;
  padding: 0 8px;
  margin: 0 0 1px 0;
  border-radius: 6px;
}
.pick-list.navigation-sidebar > row:selected { background-color: alpha(@accent_color, 0.22); }
.pick-list.navigation-sidebar > row:selected:hover { background-color: alpha(@accent_color, 0.28); }
.pick-list.navigation-sidebar > row:selected:focus-visible { outline: none; }
.pick-ticker { font-family: monospace; font-weight: 700; font-size: 0.9em; }
.pick-name { font-size: 0.9em; opacity: 0.55; }
/* A futures root's fold, at the row's far edge: no padding of its own, so
   the row keeps its height, and quiet until pointed at. Under an open root,
   its contracts; Front, on the one trading now, sits where a kind would,
   in the chip's grey rather than the kind's accent, being a date's doing
   and not a category. */
.pick-expander { min-height: 0; min-width: 0; padding: 0 2px; opacity: 0.45; -gtk-icon-size: 14px; }
.pick-expander:hover { opacity: 1; }
.pick-front {
  font-size: 0.75em;
  padding: 1px 6px;
  border-radius: 6px;
  background: alpha(@card_fg_color, 0.08);
  opacity: 0.75;
}
/* Every chart while a picker is open, faded under it; the one it will
   change wears the wide ring. */
.pick-shade { background-color: alpha(black, 0.4); }

/* A picker that takes something typed — Ctrl+G's range, a digit's resolution:
   a centred title with an ⓘ for the syntax, a framed box with the text
   centred, and under it what the answer comes to. Nothing under the box
   comes or goes with typing; the list drops down over it in a popover of
   its own. */
popover.pick-bare > contents { padding: 0; }
popover.pick-bare > contents > box { padding-bottom: 22px; }
popover.pick-bare .pick-header { padding: 14px 20px 0 20px; }
popover.pick-bare .pick-title { font-weight: 700; font-size: 1.2em; }
popover.pick-bare .pick-info { opacity: 0.35; -gtk-icon-size: 14px; }
popover.pick-bare .pick-info:hover { opacity: 0.8; }
popover.pick-bare .pick-query { padding: 14px 20px 0 20px; border-bottom: none; }
popover.pick-bare .pick-query entry {
  min-height: 42px;
  border-radius: 7px;
  font-family: monospace;
  font-size: 1.1em;
  background: alpha(@window_fg_color, 0.05);
  box-shadow: inset 0 0 0 1px alpha(@window_fg_color, 0.14);
}
popover.pick-bare .pick-query entry:focus-within { box-shadow: inset 0 0 0 2px @accent_color; }
popover.pick-bare .pick-query entry.error { box-shadow: inset 0 0 0 2px @error_color; }
popover.range-goto .pick-query entry { min-height: 48px; }
popover.pick-drop > contents { padding: 0; }
/* The month under the Go to card, as wide as it: one header between two
   arrows, From to To shaded, the end being set filled and the other ringed.
   The band stops under the two ends' squares rather than past them. */
popover.range-month-pop > contents { padding: 12px 14px 14px 14px; }
.range-month-title { font-weight: 700; padding: 4px 10px; min-height: 0; border-radius: 7px; }
.range-month-step { min-width: 30px; min-height: 30px; padding: 0; border-radius: 7px; -gtk-icon-size: 16px; }
.range-month-pick { min-height: 40px; padding: 0; border-radius: 8px; font-size: 0.9em; font-weight: 400; font-feature-settings: 'tnum'; }
.range-month-pick.today { box-shadow: inset 0 0 0 1px alpha(currentColor, 0.35); }
.range-month-pick.setting { background: @accent_color; color: @accent_fg_color; font-weight: 700; }
.range-month-name { font-size: 0.78em; opacity: 0.55; min-height: 22px; }
.range-month-cell.between { background: alpha(@accent_color, 0.14); }
.range-month-cell.start { background-image: linear-gradient(to right, transparent 50%, alpha(@accent_color, 0.14) 50%); }
.range-month-cell.end { background-image: linear-gradient(to left, transparent 50%, alpha(@accent_color, 0.14) 50%); }
.range-month-day {
  min-width: 30px;
  min-height: 30px;
  padding: 0;
  border-radius: 8px;
  font-size: 0.9em;
  font-weight: 400;
  font-feature-settings: 'tnum';
}
.range-month-day.out { opacity: 0.3; }
.range-month-day.today { box-shadow: inset 0 0 0 1px alpha(currentColor, 0.35); }
/* The ringed end is filled with the band's own colour, opaque, so the band
   seems to end under it rather than cut through its middle. */
.range-month-day.other {
  background-color: mix(@popover_bg_color, @accent_color, 0.14);
  box-shadow: inset 0 0 0 1.5px @accent_color;
  font-weight: 600;
}
.range-month-day.setting { background: @accent_color; color: @accent_fg_color; font-weight: 700; }

/* The ⓘ's help, beside the card on its own ground: examples as key caps in
   the accent, what they mean right-aligned. */
popover.background.pick-help-pop > contents { padding: 14px 16px; }
.pick-help { min-width: 268px; }
.pick-help-section { font-size: 0.78em; letter-spacing: 0.06em; color: alpha(currentColor, 0.5); }
.pick-help-group.later,
.pick-help-foot { margin-top: 9px; padding-top: 9px; border-top: 1px solid alpha(currentColor, 0.13); }
.pick-help-key,
.pick-help-cap {
  font-family: monospace;
  font-weight: 700;
  font-size: 0.85em;
  padding: 1px 5px;
  border-radius: 4px;
  background: alpha(@window_fg_color, 0.07);
  box-shadow: inset 0 -1px 0 alpha(@window_fg_color, 0.18), inset 0 0 0 1px alpha(@window_fg_color, 0.12);
}
.pick-help-key { color: @accent_color; }
.pick-help-what { color: alpha(currentColor, 0.75); font-size: 0.9em; }
.pick-help-foot { font-size: 0.85em; color: alpha(currentColor, 0.5); }
.pick-help-cap { color: @window_fg_color; }
.pick-help-note { font-size: 0.85em; color: alpha(currentColor, 0.6); margin-top: 4px; }
.pick-help-note.first { margin-top: 9px; padding-top: 9px; border-top: 1px solid alpha(currentColor, 0.13); }

.range-hint { padding: 8px 20px 0 20px; font-size: 0.9em; opacity: 0.6; }
.range-hint.error { color: @error_color; opacity: 1; }
.range-dates { padding: 12px 20px 0 20px; }
.range-cell-name { font-size: 0.78em; opacity: 0.45; }
.range-today { font-size: 0.78em; color: @success_color; }
.range-arrow { opacity: 0.4; margin-bottom: 8px; }
/* From and To: plain boxes a day is typed into, with GTK's calendar in the
   corner for the pointer. */
entry.range-date {
  min-height: 0;
  padding: 0 6px 0 9px;
  font-family: monospace;
  font-size: 0.88em;
  font-feature-settings: 'tnum';
  border-radius: 7px;
  background: alpha(@window_fg_color, 0.05);
  box-shadow: inset 0 0 0 1px alpha(@window_fg_color, 0.14);
  outline: none;
}
entry.range-date > text { padding: 7px 0; }
entry.range-date:focus-within { box-shadow: inset 0 0 0 2px @accent_color; outline: none; }
entry.range-date.idle { color: alpha(@window_fg_color, 0.7); }
entry.range-date.error { box-shadow: inset 0 0 0 2px @error_color; }
entry.range-date.unread { color: @error_color; }
entry.range-date > image { opacity: 0.45; -gtk-icon-size: 13px; }
entry.range-date > image:hover { opacity: 0.9; }

/* The resolution, pinned in the range box's bottom-right corner. */
.range-chip-row { padding: 8px 20px 0 20px; }
.range-chip {
  margin: 0;
  padding: 1px 6px;
  border-radius: 5px;
  font-family: monospace;
  font-size: 0.78em;
  font-weight: 700;
  background: alpha(@window_fg_color, 0.05);
  box-shadow: inset 0 0 0 1px alpha(@window_fg_color, 0.18);
}
.range-chip:focus { box-shadow: inset 0 0 0 2px @accent_color; outline: none; }
.range-chip:hover { box-shadow: inset 0 0 0 1px alpha(@window_fg_color, 0.35); }
/* Nothing typed: just `auto`, faded, with nothing to open and no hover. */
.range-chip.inert, .range-chip.inert:hover { opacity: 0.5; box-shadow: inset 0 0 0 1px alpha(@window_fg_color, 0.18); }
.range-caret { font-size: 0.8em; opacity: 0.5; }
.range-auto { font-family: sans-serif; font-weight: 400; font-size: 0.9em; opacity: 0.55; }
/* A question is a small card on the menus' surface: the title and the line
   under it centred (see `dialogs::card`), the answers under them as
   words, and only the answer that does something stands out: filled in the
   accent, or, when it takes something away, red on a red tint, as
   libadwaita draws a destructive button. Two equal slabs across the bottom
   made Cancel look like as much of a decision as Delete. */
dialog.alert.floating sheet {
  background-color: @popover_bg_color;
  border-radius: 10px;
  outline: none;
  box-shadow: 0 0 0 1px alpha(@window_fg_color, 0.10), 0 14px 40px alpha(black, 0.45);
}
dialog.alert .message-area { padding: 20px 24px 0; }
dialog.alert .message-area > .child { padding-left: 0; padding-right: 0; margin-top: 0; }
dialog.alert .card-title { font-weight: 600; font-size: 1.05em; }
dialog.alert .card-body { font-size: 0.9em; color: alpha(@popover_fg_color, 0.65); }
/* `dialogs::card` holds the row centred at its own width. */
dialog.alert .response-area {
  padding: 16px 16px 14px 16px;
  border-spacing: 6px;
}
dialog.alert .response-area > button {
  min-height: 0;
  padding: 4px 12px;
  border-radius: 6px;
  background: none;
  box-shadow: none;
  font-weight: normal;
  color: alpha(@popover_fg_color, 0.7);
}
dialog.alert .response-area > button:hover { background: alpha(@popover_fg_color, 0.07); color: @popover_fg_color; }
dialog.alert .response-area > button.suggested-action,
dialog.alert .response-area > button.destructive-action { font-weight: 600; }
dialog.alert .response-area > button.suggested-action { background: @accent_bg_color; color: @accent_fg_color; }
dialog.alert .response-area > button.destructive-action { background: alpha(@destructive_color, 0.15); color: @destructive_color; }
dialog.alert .response-area > button.suggested-action:hover { background: alpha(@accent_bg_color, 0.85); }
dialog.alert .response-area > button.destructive-action:hover { background: alpha(@destructive_color, 0.22); }

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
/* A Pinned chart that just refused a rail click, for a moment: the pin is the
   reason nothing moved, so the pin is what lights up. */
.legend-link.refused {
  background: alpha(@accent_color, 0.35);
  border-radius: 5px;
}

/* A row of the header's hand-built lists. */
.link-row {
  padding: 3px 10px;
  min-height: 0;
  border-radius: 5px;
  font-size: 0.9em;
}
.link-row:hover { background: alpha(currentColor, 0.09); }

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
/* The chart's title, centred over it: the symbol's button, its words left
   undrawn while there are none, the room kept so the pointer can find it,
   and under the pointer a quiet Add title. Typed into, the entry wears no
   field: the same words, at the same weight, with a caret in them. */
.chart-title.chart-title-untitled label { opacity: 0; transition: opacity 120ms ease-out; }
.chart-title.chart-title-untitled:hover label { opacity: 0.5; font-weight: 400; }
/* Untitled, it spans the top of the chart: the words answer the pointer, not
   a wash the width of the chart. */
.chart-title.chart-title-untitled:hover { background: none; }
entry.chart-title-entry {
  background: alpha(currentColor, 0.12);
  border: none;
  box-shadow: none;
  outline: none;
  padding: 0 4px;
  min-height: 0;
  border-radius: 5px;
}
entry.chart-title-entry > text { margin: 0; }

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
/* The data feed's dot, beside the chart's name: eight pixels across, so it
   is a light and not a stray wick, in the palette's red, orange or green
   for not connected, connecting and connected. The button around it is
   the same flat button the resolution is, and with one feed to be on it
   has nothing to open and no hover to promise one. */
.feed-dot {
  min-width: 8px;
  min-height: 8px;
  border-radius: 4px;
  background: @omacharts_rose;
}
.feed-dot.connecting { background: @omacharts_orange; }
.feed-dot.connected { background: @omacharts_green; }
.feed-warning { color: @omacharts_rose; }
/* Buy and Sell in the header: the line's own flat buttons, in the trading
   colours, a word each, with the hover wash the others have. */
.trade-button { padding: 2px 6px; min-width: 0; min-height: 0; font-weight: bold; }
.feed-part > button { padding: 2px 6px; min-width: 0; }
/* First on the line, the button reaches into the legend's margin so the
   dot's centre is the legend dots' centre, and keeps only a pixel past the
   dot on its right so the symbol starts where the legend's names do: the
   dot's box, the row's gap and the name's edge of every legend row (see
   pane::legend_row), here the dot, this pixel, the line's spacing and the
   symbol button's own padding. */
.feed-part.lead { margin-left: -3px; }
.feed-part.lead > button { padding-right: 1px; }
/* With nothing to open, GTK makes the button insensitive, and an
   insensitive button is drawn faded: the dot is the same light either way,
   only without the hover. */
.feed-part.inert > button:hover { background: none; }
.feed-part.inert > button:disabled,
.feed-part.inert > button:disabled > * { opacity: 1; filter: none; }

/* A hidden layer's glyph, on every chart and in every state: a bare chart
   with nothing saying why reads as one whose data never came. */
.legend-hidden { opacity: 0.75; }
.legend-hidden:hover { opacity: 1; }

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
   `CORNER_PAD_START`, which the rail's + is placed by, and its top edge is
   `CORNER_BUTTON_TOP`.

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
/* While a picker dims a chart, no ground: it is the chart's colour undimmed,
   and over the shade it was a lit square in the corner. */
.picking .window-corner.over-chart { background: none; }
/* The drawing tools' toggle, in the bottom-left corner at the time axis's
   height, and the trading panel's, in the bottom-right under the prices:
   quiet while what each opens is shut, at the corners' usual strength
   while it is open, full under the pointer. It is each toggle's own state
   that says which. One rule, so the two cannot drift apart. */
button.tools-toggle:not(:checked),
button.trading-toggle:not(:checked) { opacity: 0.25; }
button.tools-toggle:hover,
button.trading-toggle:hover { opacity: 1; }
/* Quiet until the pointer is near: on the buttons rather than the box, which
   is what leaves the ground at its own strength. */
.window-corner button,
button.tools-toggle,
button.trading-toggle {
  opacity: 0.75;
  transition: opacity 120ms ease-out;
}
.window-corner:hover button { opacity: 1; }
/* The main menu's dot: an error dismissed from the corner notice and not
   yet read in the Log. Drawn on the button rather than a widget beside it,
   so a window with nothing unread has nothing more in it. */
.window-corner menubutton.unread > button {
  opacity: 1;
  background-image: radial-gradient(circle 3px at 74% 28%, @error_color 99%, transparent 100%);
}

/* The corner notice, over the bottom-left of the charts. */
.log-notice { padding: 8px 8px 8px 14px; margin: 0 0 0 0; }
/* The Log's rows: room between them, and none taken from the text. */
.log-list > row { padding: 6px 12px; }

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
.menu-choice { padding: 2px 6px; min-width: 0; min-height: 0; border-radius: 5px; font-size: 0.9em; }
",
    );
    // Typing on the chart, which the chart styles itself.
    css.push_str(omacharts_chart::STYLESHEET);
    css.push_str(
        "/* The nine places, as a block of nine cells rather than nine buttons: no
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

/* The box naming a new watchlist is the row it will become: edge to edge
   across it, its text where a list's name starts (the row's own 8px and the
   12 a list row's name stands in by). */
.navigation-sidebar > row.new-list { padding: 0; }
.navigation-sidebar > row.new-list entry { padding-left: 20px; }

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

/* A data feed's place in the order, its number, which gives way to the
   handle under the pointer. Faded in rather
   than swapped, so the row does not flicker as the pointer crosses it. */
.feed-rank > label { font-weight: 600; transition: opacity 120ms; }
.feed-rank > image { opacity: 0; transition: opacity 120ms; }
row:hover .feed-rank > image { opacity: 0.55; }
row:hover .feed-rank > label { opacity: 0; }

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
/* The trading panel under the charts, set like the strip: one hairline
   above it, and its tables in the rail's quiet type. */
.trading-panel {
  border-top: 1px solid alpha(currentColor, 0.08);
}
.trading-tabs {
  padding: 3px 6px;
}
.trading-table {
  padding: 4px 12px 8px 12px;
  font-size: 0.9em;
}
.trading-heading {
  opacity: 0.55;
  font-size: 0.85em;
}
.trading-table button {
  padding: 0 8px;
  min-height: 22px;
}
.chartbook-strip {
  /* No padding on the left: the first tab sits flush with the window's
     edge, the way a tab on a drawer does. */
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
/* The indicator selected on the chart: its name at full strength. Its dot
   wears a ring, which the dot draws. */
.legend-row.selected .legend-indicator { opacity: 1; }
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
button.tools-toggle,
button.trading-toggle,
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
button.tools-toggle:hover,
button.trading-toggle:hover,
.legend-link:hover,
.legend-gear:hover,
.chart-pane:hover .pane-expand:hover,
button.rail-switcher:hover,
button.rail-add:hover { background: alpha(currentColor, 0.12); }
.window-corner button:checked,
button.tools-toggle:checked,
button.trading-toggle:checked,
button.rail-switcher.open { background: alpha(currentColor, 0.15); }
.window-corner button:checked:hover,
button.tools-toggle:checked:hover,
button.trading-toggle:checked:hover,
button.rail-switcher.open:hover { background: alpha(currentColor, 0.2); }
/* The link is a menu button, and only the button inside it opens the menu:
   padding on the outer one drew a box whose rim did nothing when clicked.
   The outer keeps the size and the wash; the inner fills all of it. */
.legend-link { padding: 0; }
.legend-link > button { min-width: 22px; min-height: 22px; padding: 2px; }
/* At the time axis's height both corner toggles have its 24 pixels to
   stand in: no padding above or below them, so each is the glyph's 22 tall
   and clear of the axis's rule above. */
button.tools-toggle,
button.trading-toggle { padding-top: 0; padding-bottom: 0; }
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
  /* Text inherits the colour its parent worked out, so a chart on a theme
     of its own says its own again. */
  color: @view_fg_color;
  border: {ring}px solid transparent;
}}
.chart-split .chart-pane.focused {{ border-color: @omacharts_focus; }}
/* While a picker is open every chart fades, and the one it will change
   wears the ring twice as wide: a second ring inside the border, as an
   outline, so nothing inside the pane moves. A settings window under LOCAL
   rings the chart its rows change the same way, without the fade. Two classes,
   so a picker closing does not take LOCAL's ring with it. */
.chart-split .chart-pane.picked,
.chart-split .chart-pane.local-target {{
  border-color: @omacharts_focus;
  outline: {ring}px solid @omacharts_focus;
  outline-offset: -{double}px;
}}
/* The drawing tools stand beside the charts on the window's own ground,
   with the ring's width of it before the tools, which the toggle at their
   foot centres under, open or shut. Their edge against the charts is the rail's against
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
        double = 2 * RING_WIDTH,
    );
    let rules = read_as_properties(&css[rules_from..], &colours);
    css.truncate(rules_from);
    css.push_str(&rules);
    css
}

/// `rules` with every one of `colours` read as its property rather than by
/// name: `@view_bg_color` becomes `var(--view-bg-color)`. A name is fixed
/// for the whole window and a property can be set again under a chart, so
/// this is what lets a chart on a theme of its own reach every rule. The
/// rules are written with names, as libadwaita's guide writes them.
fn read_as_properties(rules: &str, colours: &[(&'static str, String)]) -> String {
    let mut out = String::with_capacity(rules.len() + rules.len() / 8);
    let mut rest = rules;
    while let Some(at) = rest.find('@') {
        out.push_str(&rest[..at]);
        let tail = &rest[at + 1..];
        let end = tail.find(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).unwrap_or(tail.len());
        let name = &tail[..end];
        match colours.iter().any(|(known, _)| *known == name) {
            true => {
                let _ = write!(out, "var({})", property(name));
            }
            false => {
                out.push('@');
                out.push_str(name);
            }
        }
        rest = &tail[end..];
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use omacharts_engine::theme::builtin_themes;

    /// A signed P&L is written in the trading look's profit and loss, which
    /// red up turns round with the bars.
    #[test]
    fn profit_and_loss_are_named_colours_from_the_trading_look() {
        let theme = &builtin_themes()[0];
        let bars = BarLook::default().scheme(theme);
        let css = trading_css(theme, &TradingLook::default(), &BarLook::default());
        assert!(css.contains(&format!("@define-color omacharts_profit {};", bars.up)), "{css}");
        assert!(css.contains(&format!("@define-color omacharts_loss {};", bars.down)), "{css}");
        assert!(css.contains(".pnl-gain { color: @omacharts_profit; }"));
        let red_up = trading_css(theme, &TradingLook::default(), &BarLook::red_up());
        assert!(red_up.contains(&format!("@define-color omacharts_profit {};", bars.down)), "{red_up}");
        // A side and a position are the theme's Blue and Amber, never money's
        // colours, so the header's Buy and Sell match the chart's entries.
        let (blue, amber) = (&theme.swatch("Blue").unwrap().hex, &theme.swatch("Amber").unwrap().hex);
        assert!(css.contains(&format!("@define-color omacharts_buy {blue};")), "{css}");
        assert!(css.contains(&format!("@define-color omacharts_sell {amber};")), "{css}");
        assert!(css.contains(&format!("@define-color omacharts_long {blue};")), "{css}");
        assert!(css.contains(&format!("@define-color omacharts_short {amber};")), "{css}");
        assert!(css.contains(".trade-buy { color: @omacharts_buy; }") && css.contains(".trade-short { color: @omacharts_short; }"));
    }

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
        assert!(css.contains(".column-drop { border-radius: 1px; background: alpha(var(--window-fg-color)"));
        // The headings carry the drop tick alone; a frame as well is noise.
        assert!(css.contains(".column-headings:drop(active)"));
        assert!(css.contains(".symbol-row:drop(active)"));
        assert!(css.contains(".section-row:drop(active)"));
        assert!(css.contains(".dragging row:hover"));
        // The drawing tools' toggle and the trading panel's are quiet
        // while what they open is shut, by one rule.
        assert!(css.contains(
            "button.tools-toggle:not(:checked),\nbutton.trading-toggle:not(:checked) { opacity: 0.25; }"
        ));
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
        assert!(css.contains(".chart-split .chart-pane.focused { border-color: var(--omacharts-focus); }"));
        // And the wide handle's two hairlines are gone.
        assert!(css.contains("box-shadow: none;"));
    }

    /// The rules read the colours as properties, which the window sets at
    /// the root and a chart on a theme of its own sets again under its
    /// class: so its header and controls wear its theme, not the window's.
    #[test]
    fn a_chart_on_another_theme_gets_that_themes_colours_under_its_class() {
        let themes = builtin_themes();
        let (window, other) = (&themes[0], &themes[1]);
        let css = stylesheet(window, &omacharts_engine::theme::theme_bars(window));
        assert!(css.contains(&format!(":root {{ --window-bg-color: {};", window.ui.surface)));
        let rules = &css[css.find(" }\n").expect("the root block")..];
        assert!(!rules.contains("@window_fg_color"), "a rule reads a named colour, which no chart can change");
        assert!(rules.contains(".change-up { color: var(--omacharts-up); }"));
        // A colour libadwaita works out for itself stays a name, and a name
        // that starts like one of ours is not taken for it.
        assert!(rules.contains("@destructive_color"));
        assert!(rules.contains("var(--omacharts-gutter-hover)"));

        let scoped = scoped_stylesheet(&themes, &BarLook::default());
        let class = theme_class(&other.id);
        let block = scoped.lines().find(|line| line.starts_with(&format!(".{class} {{"))).expect("a block for the theme");
        assert!(block.contains(&format!("--view-bg-color: {};", other.ui.background)), "{block}");
        assert!(block.contains(&format!("--accent-color: {};", other.ui.accent)), "{block}");
        assert_eq!(scoped.lines().count(), themes.len());
    }

    /// A menu and a popover are one surface, and a menu row is a rail row.
    #[test]
    fn menus_and_popovers_share_one_surface() {
        let theme = &builtin_themes()[0];
        let css = stylesheet(theme, &omacharts_engine::theme::theme_bars(theme));
        assert!(css.contains("popover.menu > contents,\npopover.background > contents {"));
        assert!(css.contains("popover.menu modelbutton {\n  min-height: 26px;"));
        assert!(css.contains("popover.menu accelerator {\n  font-family: monospace;"));
    }

    #[test]
    fn accent_text_contrasts_with_the_accent() {
        assert_eq!(colors::readable_on("#ffffff"), "#000000");
        assert_eq!(colors::readable_on("#0d1117"), "#ffffff");
    }

    #[test]
    fn omarchy_is_the_default_when_present_and_tokyo_night_when_not() {
        // Exercised through Theming::load against a store with no settings,
        // which is the only place the choice is made.
        let store = Store::memory().unwrap();
        let theming = Theming::load(&store);
        let expected = if omarchy::available(&home()) { OMARCHY_ID } else { FALLBACK_THEME_ID };
        assert_eq!(theming.theme().id, expected);
        assert_eq!(theming.bar_look(), &BarLook::default());
    }

    /// The trading look is a setting of its own, worn by whichever theme
    /// is chosen, and read again when a command changes it under a window.
    #[test]
    fn the_trading_look_is_stored_and_worn_by_the_theme() {
        use omacharts_engine::indicators::LineStyle;
        let store = Store::memory().unwrap();
        let mut theming = Theming::load(&store);

        let look = TradingLook {
            buy: Some(omacharts_engine::ColorChoice::Fixed { hex: "#123456".into() }),
            stop: Some(LineStyle::Solid),
            ..TradingLook::default()
        };
        theming.set_trading_look(look.clone(), &store);
        assert_eq!(trading_look(&store), look);
        let worn = omacharts_engine::theme_trading(&theming.theme(), theming.trading_look(), theming.bar_look());
        assert_eq!((worn.buy.as_str(), worn.stop), ("#123456", LineStyle::Solid));

        store.set_setting(SETTING_TRADING_LOOK, "{}");
        theming.reload_selection(&store);
        assert_eq!(theming.trading_look(), &TradingLook::default());
        // Nonsense written by hand follows the theme rather than failing.
        store.set_setting(SETTING_TRADING_LOOK, "not json");
        assert_eq!(trading_look(&store), TradingLook::default());
    }

    #[test]
    fn a_missing_theme_falls_back_rather_than_panicking() {
        let store = Store::memory().unwrap();
        store.set_setting(SETTING_THEME, "deleted-by-another-window");
        let theming = Theming::load(&store);
        assert!(!theming.theme().id.is_empty());
    }

    #[test]
    fn the_bars_an_older_version_stored_are_the_bars_this_one_draws() {
        let store = Store::memory().unwrap();
        store.set_setting(LEGACY_BAR_SCHEME, "hollow");
        assert_eq!(bar_look(&store), BarLook { up_body: Body::Hollow, ..BarLook::default() });

        // Once anything is written in the new terms, those win.
        store.set_setting_bool(SETTING_BAR_UP_HOLLOW, false);
        assert_eq!(bar_look(&store), BarLook::default());

        store.set_setting(LEGACY_BAR_SCHEME, "theme-red-up");
        assert_eq!(bar_look(&store).up, BarLook::red_up().up);
    }

    /// Every scheme v0.2.20 could have stored, with its two switches, comes
    /// back as the bars it drew.
    #[test]
    fn every_scheme_an_older_version_stored_is_carried_over() {
        use omacharts_engine::theme::{GRAPHITE, SAND, SILVER, SLATE};
        let pair = |up: &str, down: &str| BarLook {
            up: omacharts_engine::ColorChoice::swatch(up),
            down: omacharts_engine::ColorChoice::swatch(down),
            ..BarLook::default()
        };
        let hollow = |look: BarLook| BarLook { up_body: Body::Hollow, ..look };
        let cases = [
            (Some("theme"), false, false, BarLook::default()),
            (Some("theme-red-up"), false, false, BarLook::red_up()),
            (Some("theme-hollow"), false, false, hollow(BarLook::default())),
            (Some("hollow"), false, false, hollow(BarLook::default())),
            (Some("theme-mono"), false, false, BarLook::monochrome()),
            (Some("monochrome"), false, false, BarLook::monochrome()),
            (Some("theme-soft"), false, false, pair(SLATE, SAND)),
            (Some("theme-quiet"), false, false, pair(SILVER, GRAPHITE)),
            (Some("accessible"), false, false, pair("Blue", "Orange")),
            (Some("theme"), true, false, BarLook::red_up()),
            (Some("theme-soft"), true, false, pair(SAND, SLATE)),
            (Some("theme-quiet"), false, true, hollow(pair(SILVER, GRAPHITE))),
            (None, true, true, hollow(BarLook::red_up())),
            // A switch and a scheme that say the same thing say it once.
            (Some("theme-red-up"), true, false, BarLook::red_up()),
        ];
        for (scheme, swapped, hollow_up, bars) in cases {
            let store = Store::memory().unwrap();
            if let Some(scheme) = scheme {
                store.set_setting(LEGACY_BAR_SCHEME, scheme);
            }
            store.set_setting_bool(LEGACY_SWAP_COLOURS, swapped);
            store.set_setting_bool(LEGACY_HOLLOW_UP, hollow_up);
            assert_eq!(crate::chart_look::ChartLook::global(&store).bars, bars, "{scheme:?} swapped {swapped} hollow {hollow_up}");
        }
    }

    #[test]
    fn the_bars_round_trip_through_the_store() {
        let store = Store::memory().unwrap();
        let look = BarLook {
            up: parse_bar_colour("foreground").unwrap(),
            down: parse_bar_colour("#123456").unwrap(),
            up_body: Body::Hollow,
            down_body: Body::Bordered,
            line_low: parse_bar_colour("amber").unwrap(),
            line_high: parse_bar_colour("#abcdef").unwrap(),
            up_thickness: Thickness::One,
            down_thickness: Thickness::OneAndAHalf,
        };
        save_bar_look(&store, &look);
        assert_eq!(bar_look(&store), look);
        // And nonsense typed into the row falls back rather than failing.
        store.set_setting(SETTING_BAR_DOWN, "chartreuse");
        assert_eq!(bar_look(&store).down, BarLook::default().down);
        store.set_setting(SETTING_BAR_UP_THICKNESS, "4");
        assert_eq!(bar_look(&store).up_thickness, Thickness::Three, "4, which an older build offered, is 3");
        store.set_setting(SETTING_BAR_UP_THICKNESS, "thick");
        assert_eq!(bar_look(&store).up_thickness, Thickness::default());
    }

    /// One thickness for both directions, as an older version stored it,
    /// is both directions' until the bars are next changed, which writes
    /// each its own and drops the old row.
    #[test]
    fn one_thickness_an_older_version_stored_is_both_directions() {
        let store = Store::memory().unwrap();
        store.set_setting(LEGACY_OHLC_THICKNESS, "3");
        let look = bar_look(&store);
        assert_eq!((look.up_thickness, look.down_thickness), (Thickness::Three, Thickness::Three));
        save_bar_look(&store, &BarLook { down_thickness: Thickness::One, ..look });
        assert!(!store.has_setting(LEGACY_OHLC_THICKNESS));
        let look = bar_look(&store);
        assert_eq!((look.up_thickness, look.down_thickness), (Thickness::Three, Thickness::One));
    }
}
