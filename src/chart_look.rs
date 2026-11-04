//! What a chart wears: how its bars are drawn and coloured, its header, and
//! the two lines across it.
//!
//! Two layers and no more. Every chart follows the look kept in the settings
//! rows, unless it holds a copy of its own, whole: the first change made to
//! one chart alone copies every chart's look onto it and changes the copy,
//! and from then on changes to every chart's leave it as it is. The theme and
//! the crosshair across Follow charts are not in it: a window wears one
//! theme, and a crosshair between charts is not one chart's.
//!
//! Colours are kept as the choices they were (a swatch's name, Foreground,
//! a hex), so a chart's copy follows a change of theme the way every chart
//! does.

use omacharts_engine::theme::{parse_bar_colour, spell_bar_colour, BarLook, Body, ColorChoice};
use omacharts_engine::BarStyle;

use crate::chart_defaults::{chart_switch, SETTING_BAR_STYLE, SETTING_PRICE_LINE, SETTING_SHOW_GRID};
use crate::header::{self, Header, Part, Section};
use crate::store::Store;
use crate::theming;

#[derive(Clone, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
#[serde(from = "Spelled", into = "Spelled")]
pub struct ChartLook {
    pub style: BarStyle,
    pub bars: BarLook,
    pub header: Header,
    pub grid: bool,
    pub price_line: bool,
}

/// What a chart wears before anybody has chosen anything.
impl Default for ChartLook {
    fn default() -> ChartLook {
        ChartLook {
            style: BarStyle::default(),
            bars: BarLook::default(),
            header: Header::standard(),
            grid: false,
            price_line: true,
        }
    }
}

/// One answer of a look, changed.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum LookEdit {
    Style(BarStyle),
    Up(ColorChoice),
    Down(ColorChoice),
    UpBody(Body),
    DownBody(Body),
    /// One section of the header, taking its parts out of the others.
    Header(Section, Vec<Part>),
    Grid(bool),
    PriceLine(bool),
}

impl ChartLook {
    /// Every chart's look, as the settings rows hold it.
    pub fn global(store: &Store) -> ChartLook {
        ChartLook {
            style: crate::chart_defaults::bar_style(store),
            bars: theming::bar_look(store),
            header: header::load(store),
            grid: chart_switch(store, SETTING_SHOW_GRID),
            price_line: chart_switch(store, SETTING_PRICE_LINE),
        }
    }

    pub fn set(&mut self, edit: LookEdit) {
        match edit {
            LookEdit::Style(style) => self.style = style,
            LookEdit::Up(colour) => self.bars.up = colour,
            LookEdit::Down(colour) => self.bars.down = colour,
            LookEdit::UpBody(body) => self.bars.up_body = body,
            LookEdit::DownBody(body) => self.bars.down_body = body,
            LookEdit::Header(section, parts) => self.header.set_shown(section, parts, crate::features::trading()),
            LookEdit::Grid(on) => self.grid = on,
            LookEdit::PriceLine(on) => self.price_line = on,
        }
    }

    /// How the bars are drawn, the way `config bars` and `chart look` say
    /// it: `candles, up green filled, down rose hollow`.
    pub fn spell_bars(&self) -> String {
        let bars = &self.bars;
        format!(
            "{}, up {} {}, down {} {}",
            self.style.key(),
            spell_bar_colour(&bars.up),
            bars.up_body.key(),
            spell_bar_colour(&bars.down),
            bars.down_body.key()
        )
    }

    /// The whole look, said: the bars, then the lines and the header.
    pub fn spell(&self) -> String {
        let on_off = |on: bool| if on { "on" } else { "off" };
        format!(
            "{}, grid {}, price line {}, header {}",
            self.spell_bars(),
            on_off(self.grid),
            on_off(self.price_line),
            self.header.spell()
        )
    }
}

impl LookEdit {
    /// Make this every chart's answer: written to the settings row it is
    /// kept in, and to no other, so a row nobody chose stays unchosen.
    pub fn save_global(self, store: &Store) {
        match self {
            LookEdit::Style(style) => store.set_setting(SETTING_BAR_STYLE, style.key()),
            LookEdit::Header(section, parts) => {
                let mut header = header::load(store);
                header.set_shown(section, parts, crate::features::trading());
                header::save(store, &header);
            }
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
        matches!(self, LookEdit::Up(_) | LookEdit::Down(_) | LookEdit::UpBody(_) | LookEdit::DownBody(_))
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
    /// Each section's parts by key. A plain list is how a look was written
    /// before the header had sections, and is its left, the others as they
    /// ship, as the header setting itself reads.
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
        ChartLook {
            style: spelled.style.and_then(|key| BarStyle::from_key(&key)).unwrap_or(fallback.style),
            bars: BarLook {
                up: colour(spelled.up, fallback.bars.up),
                down: colour(spelled.down, fallback.bars.down),
                up_body: body(spelled.up_body, fallback.bars.up_body),
                down_body: body(spelled.down_body, fallback.bars.down_body),
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
            style: Some(look.style.key().to_string()),
            up: Some(spell_bar_colour(&look.bars.up)),
            down: Some(spell_bar_colour(&look.bars.down)),
            up_body: Some(look.bars.up_body.key().to_string()),
            down_body: Some(look.bars.down_body.key().to_string()),
            header: Some(serde_json::json!({
                "left": header::keys(&look.header.left),
                "center": header::keys(&look.header.center),
                "right": header::keys(&look.header.right),
            })),
            grid: Some(look.grid),
            price_line: Some(look.price_line),
        }
    }
}

/// A header as [`Spelled`] holds it, read leniently: a part this build
/// does not know is left out, and anything that is not a header is the
/// standard one.
fn read_header(value: serde_json::Value) -> Header {
    let keys = |value: &serde_json::Value| -> Vec<Part> {
        let keys: Vec<&str> = value.as_array().into_iter().flatten().filter_map(serde_json::Value::as_str).collect();
        header::from_keys(&keys)
    };
    match &value {
        serde_json::Value::Array(_) => Header { left: keys(&value), ..Header::standard() },
        serde_json::Value::Object(sections) => {
            let mut read = Header { left: Vec::new(), center: Vec::new(), right: Vec::new() };
            for section in Section::ALL {
                let key = section.title().to_lowercase();
                let parts = sections.get(&key).map_or_else(|| header::standard(section), keys);
                // In the first section that names it, as the setting reads.
                let placed: Vec<Part> = Section::ALL.into_iter().flat_map(|s| read.section(s).to_vec()).collect();
                read.set(section, parts.into_iter().filter(|part| !placed.contains(part)).collect());
            }
            read
        }
        _ => Header::standard(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        LookEdit::Header(Section::Left, vec![Part::Change]).save_global(&store);
        let look = ChartLook::global(&store);
        assert_eq!(look.bars.up, ColorChoice::swatch("Blue"));
        assert_eq!(look.bars.down, BarLook::default().down);
        assert!(!look.grid && look.price_line);
        assert_eq!(look.header.left, vec![Part::Change]);
        assert_eq!(look.header.right, Header::standard().right);
        assert!(!store.has_setting(SETTING_BAR_STYLE));
    }

    #[test]
    fn a_look_is_written_down_as_it_is_typed_and_comes_back_whole() {
        let mut look = ChartLook::default();
        look.set(LookEdit::Style(BarStyle::Ohlc));
        look.set(LookEdit::Down(ColorChoice::Fixed { hex: "#e66100".to_string() }));
        look.set(LookEdit::UpBody(Body::Hollow));
        look.set(LookEdit::Header(Section::Left, vec![Part::Symbol, Part::Change]));
        look.set(LookEdit::PriceLine(false));
        let json = serde_json::to_value(&look).unwrap();
        assert_eq!(json["style"], "ohlc");
        assert_eq!(json["up"], "green");
        assert_eq!(json["down"], "#e66100");
        assert_eq!(json["header"]["left"], serde_json::json!(["symbol", "change"]));
        assert_eq!(serde_json::from_value::<ChartLook>(json).unwrap(), look);
    }

    /// A newer build's words cost the one answer, not the chart.
    #[test]
    fn a_word_this_build_does_not_know_is_the_default() {
        let json = serde_json::json!({"style": "renko", "up": "Blue", "header": {"left": ["volume", "change"]}, "price_line": false});
        let look: ChartLook = serde_json::from_value(json).unwrap();
        assert_eq!(look.style, BarStyle::default());
        assert_eq!(look.bars.up, ColorChoice::swatch("Blue"));
        assert_eq!(look.bars.down, BarLook::default().down);
        assert_eq!(look.header.left, vec![Part::Change]);
        assert_eq!(look.header.center, Header::standard().center, "a section left out ships as it is");
        assert!(!look.grid && !look.price_line);
    }

    /// A look written before the header had sections held one list, its
    /// line, which is the left; the others come as they ship.
    #[test]
    fn a_header_written_as_a_list_is_its_left() {
        let look: ChartLook = serde_json::from_value(serde_json::json!({"header": ["symbol", "change"]})).unwrap();
        assert_eq!(look.header.left, vec![Part::Symbol, Part::Change]);
        assert_eq!(look.header.right, Header::standard().right);
        let look: ChartLook = serde_json::from_value(serde_json::json!({"header": 7})).unwrap();
        assert_eq!(look.header, Header::standard());
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
