//! What a chart wears: its theme, how its bars are drawn and coloured, its
//! header, and the two lines across it.
//!
//! Two layers and no more. Every chart follows the look kept in the settings
//! rows, unless it holds a copy of its own, whole: the first change made to
//! one chart alone copies every chart's look onto it and changes the copy,
//! and from then on changes to every chart's leave it as it is. The
//! crosshair across Follow charts is not in it: a crosshair between charts
//! is not one chart's.
//!
//! A copy starts on the window's theme and follows it, until a theme is
//! picked for the chart alone. The window keeps one theme for everything
//! that is not a chart.
//!
//! Colours are kept as the choices they were (a swatch's name, Foreground,
//! a hex), so a chart's copy follows a change of theme the way every chart
//! does.

use omacharts_engine::theme::{parse_bar_colour, spell_bar_colour, BarLook, Body, ColorChoice, Thickness};
use omacharts_engine::BarStyle;

use crate::chart_defaults::{SETTING_BAR_STYLE, SETTING_PRICE_LINE, SETTING_SHOW_GRID};
use crate::header::{self, Layout};
use crate::store::Store;
use crate::theming;

/// How a look on the window's theme spells its theme, and how
/// `chart look --theme` puts a chart back on it.
pub const WINDOW_THEME: &str = "window";

#[derive(Clone, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
#[serde(from = "Spelled", into = "Spelled")]
pub struct ChartLook {
    /// The theme's id, or `None` for the window's.
    pub theme: Option<String>,
    pub style: BarStyle,
    pub bars: BarLook,
    pub header: Layout,
    pub grid: bool,
    pub price_line: bool,
}

/// What a chart wears before anybody has chosen anything.
impl Default for ChartLook {
    fn default() -> ChartLook {
        ChartLook {
            theme: None,
            style: BarStyle::default(),
            bars: BarLook::default(),
            header: Layout::standard(),
            grid: false,
            price_line: true,
        }
    }
}

/// One answer of a look, changed.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum LookEdit {
    Theme(Option<String>),
    Style(BarStyle),
    Up(ColorChoice),
    Down(ColorChoice),
    UpBody(Body),
    DownBody(Body),
    UpThickness(Thickness),
    DownThickness(Thickness),
    Header(Layout),
    Grid(bool),
    PriceLine(bool),
}

impl ChartLook {
    /// Every chart's look, as the settings rows hold it: a row nobody has
    /// written is the default's answer.
    pub fn global(store: &Store) -> ChartLook {
        let default = ChartLook::default();
        ChartLook {
            theme: None,
            style: crate::chart_defaults::bar_style(store),
            bars: theming::bar_look(store),
            header: header::load(store),
            grid: store.setting_bool(SETTING_SHOW_GRID, default.grid),
            price_line: store.setting_bool(SETTING_PRICE_LINE, default.price_line),
        }
    }

    pub fn set(&mut self, edit: LookEdit) {
        match edit {
            LookEdit::Theme(id) => self.theme = id,
            LookEdit::Style(style) => self.style = style,
            LookEdit::Up(colour) => self.bars.up = colour,
            LookEdit::Down(colour) => self.bars.down = colour,
            LookEdit::UpBody(body) => self.bars.up_body = body,
            LookEdit::DownBody(body) => self.bars.down_body = body,
            LookEdit::UpThickness(px) => self.bars.up_thickness = px,
            LookEdit::DownThickness(px) => self.bars.down_thickness = px,
            LookEdit::Header(layout) => self.header = layout,
            LookEdit::Grid(on) => self.grid = on,
            LookEdit::PriceLine(on) => self.price_line = on,
        }
    }

    /// How the bars are drawn, the way `config bars` and `chart look` say
    /// it: `candles, up green filled 2px, down rose hollow 2px`, each
    /// direction's colour, body and OHLC thickness.
    pub fn spell_bars(&self) -> String {
        let bars = &self.bars;
        format!(
            "{}, up {} {} {}px, down {} {} {}px",
            self.style.key(),
            spell_bar_colour(&bars.up),
            bars.up_body.key(),
            bars.up_thickness.key(),
            spell_bar_colour(&bars.down),
            bars.down_body.key(),
            bars.down_thickness.key()
        )
    }

    /// The whole look, said: the bars, the lines and the header, then the
    /// theme, the way `chart look --theme` takes it.
    pub fn spell(&self) -> String {
        let on_off = |on: bool| if on { "on" } else { "off" };
        format!(
            "{}, grid {}, price line {}, header {}, theme {}",
            self.spell_bars(),
            on_off(self.grid),
            on_off(self.price_line),
            header::spell(&self.header.seen(crate::features::trading())),
            self.theme.as_deref().map_or(WINDOW_THEME, theming::spell_theme),
        )
    }
}

impl LookEdit {
    /// Make this every chart's answer: written to the settings row it is
    /// kept in, and to no other, so a row nobody chose stays unchosen.
    pub fn save_global(self, store: &Store) {
        match self {
            LookEdit::Theme(Some(id)) => store.set_setting(theming::SETTING_THEME, &id),
            // Every chart already wears the window's.
            LookEdit::Theme(None) => {}
            LookEdit::Style(style) => store.set_setting(SETTING_BAR_STYLE, style.key()),
            LookEdit::Header(layout) => header::save(store, &layout),
            LookEdit::Grid(on) => store.set_setting_bool(SETTING_SHOW_GRID, on),
            LookEdit::PriceLine(on) => store.set_setting_bool(SETTING_PRICE_LINE, on),
            bars => {
                let mut look = ChartLook { bars: theming::bar_look(store), ..ChartLook::default() };
                look.set(bars);
                theming::save_bar_look(store, &look.bars);
            }
        }
    }

    /// Whether it changes a colour, which the window's own stylesheet and
    /// the rail wear too, rather than only what the charts draw.
    pub fn colours(&self) -> bool {
        matches!(self, LookEdit::Theme(_) | LookEdit::Up(_) | LookEdit::Down(_) | LookEdit::UpBody(_) | LookEdit::DownBody(_))
    }
}

/// A look as it is written into a chart, spelled the way the settings rows
/// and the commands spell it.
///
/// Read leniently: a word a newer build wrote, or a field it left out, is
/// what a chart wears by default rather than a reason to refuse the whole
/// arrangement it is stored in.
#[derive(Default, serde::Serialize, serde::Deserialize)]
struct Spelled {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    theme: Option<String>,
    #[serde(default)]
    style: Option<String>,
    #[serde(default)]
    up: Option<String>,
    #[serde(default)]
    down: Option<String>,
    #[serde(default)]
    up_body: Option<String>,
    #[serde(default)]
    down_body: Option<String>,
    /// Its pixels as text; read as a number too, which is how an older
    /// build wrote them.
    #[serde(default)]
    up_thickness: Option<serde_json::Value>,
    #[serde(default)]
    down_thickness: Option<serde_json::Value>,
    /// As `config header` types it. A list of parts is how a look was
    /// written before the header had zones, and reads as that header did.
    #[serde(default)]
    header: Option<serde_json::Value>,
    #[serde(default)]
    grid: Option<bool>,
    #[serde(default)]
    price_line: Option<bool>,
}

impl From<Spelled> for ChartLook {
    fn from(spelled: Spelled) -> ChartLook {
        let fallback = ChartLook::default();
        let colour = |text: Option<String>, fallback: ColorChoice| {
            text.and_then(|text| parse_bar_colour(&text)).unwrap_or(fallback)
        };
        let body = |text: Option<String>, fallback: Body| text.and_then(|text| Body::from_key(&text)).unwrap_or(fallback);
        let thickness = |value: Option<serde_json::Value>, fallback: Thickness| {
            let read = match value {
                Some(serde_json::Value::Number(px)) => px.as_f64().map(Thickness::nearest),
                Some(serde_json::Value::String(text)) => Thickness::parse(&text),
                _ => None,
            };
            read.unwrap_or(fallback)
        };
        ChartLook {
            theme: spelled.theme,
            style: spelled.style.and_then(|key| BarStyle::from_key(&key)).unwrap_or(fallback.style),
            bars: BarLook {
                up: colour(spelled.up, fallback.bars.up),
                down: colour(spelled.down, fallback.bars.down),
                up_body: body(spelled.up_body, fallback.bars.up_body),
                down_body: body(spelled.down_body, fallback.bars.down_body),
                up_thickness: thickness(spelled.up_thickness, fallback.bars.up_thickness),
                down_thickness: thickness(spelled.down_thickness, fallback.bars.down_thickness),
            },
            header: spelled.header.map_or(fallback.header, read_header),
            grid: spelled.grid.unwrap_or(fallback.grid),
            price_line: spelled.price_line.unwrap_or(fallback.price_line),
        }
    }
}

impl From<ChartLook> for Spelled {
    fn from(look: ChartLook) -> Spelled {
        Spelled {
            theme: look.theme,
            style: Some(look.style.key().to_string()),
            up: Some(spell_bar_colour(&look.bars.up)),
            down: Some(spell_bar_colour(&look.bars.down)),
            up_body: Some(look.bars.up_body.key().to_string()),
            down_body: Some(look.bars.down_body.key().to_string()),
            up_thickness: Some(look.bars.up_thickness.key().into()),
            down_thickness: Some(look.bars.down_thickness.key().into()),
            header: Some(serde_json::Value::String(header::spell(&look.header))),
            grid: Some(look.grid),
            price_line: Some(look.price_line),
        }
    }
}

/// A header as [`Spelled`] holds it, read leniently: a part or a zone this
/// build does not know is left out, and anything that is not a header is the
/// standard one.
fn read_header(value: serde_json::Value) -> Layout {
    match value {
        serde_json::Value::String(text) => header::read(&text, true),
        serde_json::Value::Array(keys) => {
            let keys: Vec<&str> = keys.iter().filter_map(serde_json::Value::as_str).collect();
            header::read(&keys.join(","), true)
        }
        _ => Layout::standard(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::header::{Part, Zone};

    #[test]
    fn a_store_nobody_has_chosen_in_wears_the_default() {
        let store = Store::memory().unwrap();
        assert_eq!(ChartLook::global(&store), ChartLook::default());
    }

    /// Each answer goes to its own row, so the others stay as they were.
    #[test]
    fn an_edit_for_every_chart_writes_its_row_and_no_other() {
        let store = Store::memory().unwrap();
        LookEdit::Up(ColorChoice::swatch("Blue")).save_global(&store);
        LookEdit::Grid(false).save_global(&store);
        LookEdit::Header(header::parse_for("left=change", true).unwrap()).save_global(&store);
        let look = ChartLook::global(&store);
        assert_eq!(look.bars.up, ColorChoice::swatch("Blue"));
        assert_eq!(look.bars.down, BarLook::default().down);
        assert!(!look.grid && look.price_line);
        assert_eq!(look.header, header::parse_for("left=change", true).unwrap());
        assert!(!store.has_setting(SETTING_BAR_STYLE));
    }

    #[test]
    fn a_look_is_written_down_as_it_is_typed_and_comes_back_whole() {
        let mut look = ChartLook::default();
        look.set(LookEdit::Style(BarStyle::Ohlc));
        look.set(LookEdit::Down(ColorChoice::Fixed { hex: "#e66100".to_string() }));
        look.set(LookEdit::UpBody(Body::Hollow));
        look.set(LookEdit::DownThickness(Thickness::OneAndAHalf));
        look.set(LookEdit::Header(header::parse_for("left=symbol,change left2=name", true).unwrap()));
        look.set(LookEdit::PriceLine(false));
        let json = serde_json::to_value(&look).unwrap();
        assert_eq!(json["style"], "ohlc");
        assert_eq!(json["up"], "green");
        assert_eq!(json["down"], "#e66100");
        assert_eq!((&json["up_thickness"], &json["down_thickness"]), (&serde_json::json!("2"), &serde_json::json!("1.5")));
        assert_eq!(json["header"], "left=symbol,change left2=name");
        assert_eq!(serde_json::from_value::<ChartLook>(json).unwrap(), look);
    }

    /// A newer build's words cost the one answer, not the chart.
    #[test]
    fn a_word_this_build_does_not_know_is_the_default() {
        let json = serde_json::json!({"style": "renko", "up": "Blue", "header": "left=volume,change top=link", "price_line": false});
        let look: ChartLook = serde_json::from_value(json).unwrap();
        assert_eq!(look.style, BarStyle::default());
        assert_eq!(look.bars.up, ColorChoice::swatch("Blue"));
        assert_eq!(look.bars.down, BarLook::default().down);
        assert_eq!(look.header, header::parse_for("left=change", true).unwrap());
        assert!(!look.grid && !look.price_line);
    }

    /// A look written before the header had zones held it as a list of
    /// parts, which comes forward the way the old header setting does: on
    /// the first row's left, the name under it, the rest as it ships.
    #[test]
    fn a_header_written_as_a_list_comes_forward() {
        let json = serde_json::json!({"header": ["symbol", "change", "name"]});
        let look: ChartLook = serde_json::from_value(json).unwrap();
        assert_eq!(look.header.zone(Zone::Left), [Part::Symbol, Part::Change]);
        assert_eq!(look.header.zone(Zone::Left2), [Part::Name]);
        assert!(look.header.contains(Part::Settings));
        let look: ChartLook = serde_json::from_value(serde_json::json!({"header": 7})).unwrap();
        assert_eq!(look.header, Layout::standard());
    }

    /// An older build wrote a thickness as a number, and offered 4.
    #[test]
    fn a_thickness_written_as_a_number_reads_as_the_nearest_there_is() {
        let json = serde_json::json!({"up_thickness": 4, "down_thickness": 1});
        let look: ChartLook = serde_json::from_value(json).unwrap();
        assert_eq!((look.bars.up_thickness, look.bars.down_thickness), (Thickness::Three, Thickness::One));
    }

    /// A look on the window's theme writes no theme down, so a look stored
    /// before charts had themes of their own reads as on the window's.
    #[test]
    fn a_look_names_its_theme_only_when_it_has_one_of_its_own() {
        let mut look = ChartLook::default();
        assert!(serde_json::to_value(&look).unwrap().get("theme").is_none());
        assert!(look.spell().ends_with(", theme window"), "{}", look.spell());
        look.set(LookEdit::Theme(Some(omacharts_engine::theme::OMARCHY_ID.to_string())));
        let json = serde_json::to_value(&look).unwrap();
        assert_eq!(json["theme"], omacharts_engine::theme::OMARCHY_ID);
        assert_eq!(serde_json::from_value::<ChartLook>(json).unwrap(), look);
        assert!(look.spell().ends_with(", theme system"), "{}", look.spell());
    }

    /// Every chart's theme is the window's, kept where `config theme`
    /// keeps it.
    #[test]
    fn a_theme_for_every_chart_is_the_windows() {
        let store = Store::memory().unwrap();
        LookEdit::Theme(None).save_global(&store);
        assert!(!store.has_setting(theming::SETTING_THEME));
        LookEdit::Theme(Some("nord".to_string())).save_global(&store);
        assert_eq!(store.setting(theming::SETTING_THEME).as_deref(), Some("nord"));
        assert_eq!(ChartLook::global(&store).theme, None);
    }

    /// A copy holds the swatch, not the colour it was on the theme it was
    /// made under: another theme paints it that theme's blue.
    #[test]
    fn a_copy_keeps_its_choices_across_a_change_of_theme() {
        let mut look = ChartLook::default();
        look.set(LookEdit::Up(ColorChoice::swatch("Blue")));
        let back: ChartLook = serde_json::from_value(serde_json::to_value(&look).unwrap()).unwrap();
        assert_eq!(back.bars.up, ColorChoice::swatch("Blue"));
        let themes = omacharts_engine::theme::builtin_themes();
        let painted: Vec<String> = themes.iter().map(|theme| back.bars.scheme(theme).up).collect();
        for (theme, up) in themes.iter().zip(&painted) {
            assert_eq!(up, &theme.swatch("Blue").unwrap().hex, "{}", theme.name);
        }
        assert!(painted.windows(2).any(|pair| pair[0] != pair[1]), "every theme painted the same blue");
    }
}
