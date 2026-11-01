//! Themes, how bars look, and the swatch palettes they carry.
//!
//! Three things are chosen independently, because people want them
//! independently:
//!
//! * a **[`Theme`]** — the whole app's look: surfaces, text, chart structure,
//!   accent. On Omarchy this follows the desktop by default.
//! * a **[`BarLook`]** — what candles look like: a colour for each direction,
//!   from the theme's palette, and whether its body is filled, bordered or
//!   hollow.
//! * a **[`Swatch`]** from the active theme's palette, whenever a colour is
//!   picked for an indicator or overlay.
//!
//! Every theme ships a named palette so picking an indicator colour means
//! choosing from a handful that already look right together, rather than
//! hunting in a colour wheel. Picking from the wheel stays possible — see
//! [`ColorChoice`].

use serde::{Deserialize, Serialize};

use crate::bars::BarStyle;
use crate::indicators::LineStyle;

/// Which way the platform stylesheet should lean.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    #[default]
    Dark,
    Light,
}

impl Mode {
    pub fn label(self) -> &'static str {
        match self {
            Mode::Dark => "Dark",
            Mode::Light => "Light",
        }
    }
}

/// Where a theme came from: one palette fixed at what it was read as, or the
/// live Omarchy one.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    BuiltIn,
    /// Derived from the live Omarchy palette; follows it as it changes.
    Omarchy,
}

// ---------------------------------------------------------------------------
// Theme
// ---------------------------------------------------------------------------

/// The app's own colours: everything that is not a candle.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct UiColors {
    pub background: String,
    pub surface: String,
    pub surface_variant: String,
    pub border: String,
    pub text: String,
    pub text_muted: String,
    pub grid: String,
    pub axis: String,
    pub crosshair: String,
    pub accent: String,
}

/// One entry in a theme's indicator palette.
///
/// Names are shared across themes on purpose: an indicator stored as "Amber"
/// stays amber-ish when the theme changes, picking up whatever that theme
/// thinks amber should be.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Swatch {
    pub name: String,
    pub hex: String,
}

impl Swatch {
    pub fn new(name: &str, hex: &str) -> Swatch {
        Swatch { name: name.to_string(), hex: hex.to_string() }
    }
}

/// The swatch names every shipped theme provides, in display order — how the
/// palette is laid out when you go looking for a colour.
pub const SWATCH_NAMES: [&str; 8] = [
    "Blue", "Amber", "Violet", "Teal", "Rose", "Green", "Orange", "Cyan",
];

/// The order colours are handed out in when nobody picks one.
///
/// Two rules, both about not making the chart harder to read:
///
/// * **Direction colours are never handed out.** Green and Rose are what
///   candles use for up and down, so an overlay wearing them reads as price
///   action. They stay in the palette for anyone who deliberately wants them;
///   they are simply never assigned automatically. That is why this is six
///   names and not eight.
/// * **Neighbours are far apart in hue.** Consecutive overlays are the ones
///   most likely to sit on top of each other, so blue is followed by amber,
///   not by teal.
///
/// Past six, colours repeat. Every charting tool does; six distinguishable
/// overlays is already more than a chart can carry.
pub const SWATCH_SEQUENCE: [&str; 6] =
    ["Blue", "Amber", "Violet", "Teal", "Orange", "Cyan"];

/// The palette's one colour with no hue of its own to pick: a very light
/// tone of the theme's own hue on a dark theme and a very dark one on a
/// light theme, close to the text in weight but tinted, for an outline, a
/// profile or a line that should read plainly without taking a colour. It
/// is not one of a theme's [`Swatch`]es, because nobody sets it: it is
/// worked out from the theme each time it is asked for, so it follows the
/// theme as the others do. See [`Theme::neutral`].
pub const NEUTRAL: &str = "Neutral";

/// Every name a colour can be picked by, in the order the palette shows
/// them: the theme's swatches, then [`NEUTRAL`].
pub const PALETTE_NAMES: [&str; 9] = [
    "Blue", "Amber", "Violet", "Teal", "Rose", "Green", "Orange", "Cyan", NEUTRAL,
];

/// The OKLCh lightness of [`Theme::neutral`] on a dark theme: very light,
/// near the text of most themes, short of white.
const NEUTRAL_LIGHT: f64 = 0.89;

/// Its lightness on a light theme: very dark, short of black.
const NEUTRAL_DARK: f64 = 0.32;

/// The least and the most of the theme's tint it carries, in OKLCh chroma:
/// enough at the ends of the scale to read as the theme's own tone rather
/// than a grey, not so much that it reads as one more hue of the palette.
const NEUTRAL_MIN_CHROMA: f64 = 0.035;
const NEUTRAL_MAX_CHROMA: f64 = 0.06;

/// A theme's text tinted at least this much, in OKLCh chroma, gives the
/// neutral its hue; text greyer than this leaves the hue to the panels.
const NEUTRAL_TEXT_TINT: f64 = 0.015;

/// Below this a panel's chroma is too slight to take a hue from.
const NEUTRAL_TINT: f64 = 0.004;

/// The least the neutral is from the theme's text, as [`delta_e`] sees it,
/// so that it is never just the text colour over again.
const NEUTRAL_FROM_TEXT: f64 = 0.04;

/// How far from the text towards the chart it is moved, in OKLCh
/// lightness, when the usual lightness would land on the text.
const NEUTRAL_FROM_TEXT_STEP: f64 = 0.05;

/// The least a companion may be from what it accompanies, as `delta_e` sees
/// it — the floor consecutive entries of the sequence are held to, and so
/// the one the rule that usually hands out the next entry has to meet when
/// it does not.
pub const COMPANION_GAP: f64 = 0.12;

/// How a colour was chosen for an indicator or overlay.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum ColorChoice {
    /// A named swatch, re-resolved against whichever theme is active.
    Swatch { name: String },
    /// An exact colour the user picked. Never re-resolved.
    Fixed { hex: String },
}

impl ColorChoice {
    pub fn swatch(name: &str) -> ColorChoice {
        ColorChoice::Swatch { name: name.to_string() }
    }

    /// Resolve against a theme, falling back to its accent if the swatch name
    /// is one this theme does not carry.
    pub fn resolve(&self, theme: &Theme) -> String {
        match self {
            ColorChoice::Fixed { hex } => hex.clone(),
            ColorChoice::Swatch { name } => match theme.swatch(name) {
                Some(swatch) => swatch.hex.clone(),
                None if name == NEUTRAL => theme.neutral(),
                None => theme.ui.accent.clone(),
            },
        }
    }
}

#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Theme {
    pub id: String,
    pub name: String,
    pub mode: Mode,
    pub source: Source,
    pub ui: UiColors,
    /// The theme's own indicator palette.
    pub swatches: Vec<Swatch>,
    /// How orders and positions are drawn, as somebody set it. Absent, every
    /// slot follows the theme; see [`theme_trading`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    /// Boxed: most themes have none, and every theme is carried about.
    pub trading: Option<Box<TradingLook>>,
}

/// How a theme draws orders and positions, as somebody set it. Every slot
/// left unset follows the theme: a buy and a long its Blue swatch, a sell
/// and a short its Amber, a profit the bars' up colour and a loss their
/// down; a limit and a stop dashed, only a position solid. Setting one
/// leaves the others following.
#[derive(Clone, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub struct TradingLook {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub buy: Option<ColorChoice>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sell: Option<ColorChoice>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub long: Option<ColorChoice>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub short: Option<ColorChoice>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profit: Option<ColorChoice>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loss: Option<ColorChoice>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<LineStyle>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop: Option<LineStyle>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub position: Option<LineStyle>,
}

/// One of the trading colours, by what it marks.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TradingColour {
    Buy,
    Sell,
    Long,
    Short,
    Profit,
    Loss,
}

impl TradingColour {
    pub const ALL: [TradingColour; 6] = [
        TradingColour::Buy,
        TradingColour::Sell,
        TradingColour::Long,
        TradingColour::Short,
        TradingColour::Profit,
        TradingColour::Loss,
    ];

    pub fn label(self) -> &'static str {
        match self {
            TradingColour::Buy => "Buy",
            TradingColour::Sell => "Sell",
            TradingColour::Long => "Long",
            TradingColour::Short => "Short",
            TradingColour::Profit => "Profit",
            TradingColour::Loss => "Loss",
        }
    }
}

/// One of the trading lines' patterns, by what it draws.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TradingLine {
    Limit,
    Stop,
    Position,
}

impl TradingLine {
    pub const ALL: [TradingLine; 3] = [TradingLine::Limit, TradingLine::Stop, TradingLine::Position];

    pub fn label(self) -> &'static str {
        match self {
            TradingLine::Limit => "Limit order",
            TradingLine::Stop => "Stop order",
            TradingLine::Position => "Position",
        }
    }
}

impl TradingLook {
    pub fn colour_mut(&mut self, slot: TradingColour) -> &mut Option<ColorChoice> {
        match slot {
            TradingColour::Buy => &mut self.buy,
            TradingColour::Sell => &mut self.sell,
            TradingColour::Long => &mut self.long,
            TradingColour::Short => &mut self.short,
            TradingColour::Profit => &mut self.profit,
            TradingColour::Loss => &mut self.loss,
        }
    }

    pub fn line_mut(&mut self, slot: TradingLine) -> &mut Option<LineStyle> {
        match slot {
            TradingLine::Limit => &mut self.limit,
            TradingLine::Stop => &mut self.stop,
            TradingLine::Position => &mut self.position,
        }
    }
}

/// The trading look resolved: a colour for each role and a pattern for
/// each kind of line.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct TradingColours {
    pub buy: String,
    pub sell: String,
    pub long: String,
    pub short: String,
    pub profit: String,
    pub loss: String,
    pub limit: LineStyle,
    pub stop: LineStyle,
    pub position: LineStyle,
}

impl TradingColours {
    pub fn colour(&self, slot: TradingColour) -> &str {
        match slot {
            TradingColour::Buy => &self.buy,
            TradingColour::Sell => &self.sell,
            TradingColour::Long => &self.long,
            TradingColour::Short => &self.short,
            TradingColour::Profit => &self.profit,
            TradingColour::Loss => &self.loss,
        }
    }

    pub fn line(&self, slot: TradingLine) -> LineStyle {
        match slot {
            TradingLine::Limit => self.limit,
            TradingLine::Stop => self.stop,
            TradingLine::Position => self.position,
        }
    }
}

/// How `theme` draws orders and positions on a chart whose bars wear
/// `bars`: the look chosen for every chart, turned by the chart's own
/// colouring ([`BarColouring::look`]).
///
/// A side and money are told apart by hue: a buy and a long are the
/// theme's Blue swatch and a sell and a short its Amber, which every theme
/// has and none paints its candles in, so that green and red mean money
/// and nothing else. Profit and loss are the bars' up and down colours, so
/// red up turns them round with the candles, for the trader who reads red
/// as up; a buy and a sell are what they are. Bars that carry no direction,
/// Monochrome's, leave money in the theme's own green and rose rather than
/// in one colour for both.
pub fn theme_trading(theme: &Theme, bars: &BarLook) -> TradingColours {
    let worn = bars.scheme(theme);
    let money = if worn.up != worn.down { worn } else { theme_bars(theme) };
    let look = theme.trading.as_deref().cloned().unwrap_or_default();
    let pick = |chosen: &Option<ColorChoice>, otherwise: &str| match chosen {
        Some(choice) => choice.resolve(theme),
        None => otherwise.to_string(),
    };
    let (buy, sell) = (ColorChoice::swatch("Blue").resolve(theme), ColorChoice::swatch("Amber").resolve(theme));
    TradingColours {
        buy: pick(&look.buy, &buy),
        sell: pick(&look.sell, &sell),
        long: pick(&look.long, &buy),
        short: pick(&look.short, &sell),
        profit: pick(&look.profit, &money.up),
        loss: pick(&look.loss, &money.down),
        limit: look.limit.unwrap_or(LineStyle::Dashed),
        stop: look.stop.unwrap_or(LineStyle::Dashed),
        position: look.position.unwrap_or(LineStyle::Solid),
    }
}

/// What a theme holds as set by hand, which an edit starts from.
pub fn editable_trading(theme: &Theme) -> TradingLook {
    theme.trading.as_deref().cloned().unwrap_or_default()
}

impl Theme {
    pub fn swatch(&self, name: &str) -> Option<&Swatch> {
        self.swatches.iter().find(|s| s.name == name)
    }

    /// Every colour the palette offers, as the picker shows them: the
    /// theme's swatches and then [`NEUTRAL`], worked out from it now.
    pub fn palette(&self) -> Vec<Swatch> {
        let mut all = self.swatches.clone();
        if self.swatch(NEUTRAL).is_none() {
            all.push(Swatch { name: NEUTRAL.to_string(), hex: self.neutral() });
        }
        all
    }

    /// The theme's neutral: a very light tone of its own hue on a dark
    /// chart, a very dark one on a light chart.
    ///
    /// The hue and the tint come from the theme's text when it has any, so
    /// Tokyo Night's is a pale lavender and Gruvbox's a cream; from its
    /// background or panels when the text is a plain grey; and from its
    /// accent when the whole theme is grey, so no theme gets a white or a
    /// black that belongs to no theme. The tint is held to a narrow band
    /// of chroma: a tone, not a colour. Whichever side of the background
    /// the text is on decides light or dark, and when that lightness would make it the text
    /// colour over again, as on a theme whose text is already that light,
    /// it is the text's own lightness toned a step towards the chart.
    pub fn neutral(&self) -> String {
        let (Some(ground), Some(text)) = (Oklch::of(&self.ui.background), Oklch::of(&self.ui.text)) else {
            return self.ui.text.clone();
        };
        let panels = [&self.ui.background, &self.ui.surface, &self.ui.surface_variant, &self.ui.border]
            .into_iter()
            .filter_map(|hex| Oklch::of(hex))
            .find(|colour| colour.c >= NEUTRAL_TINT);
        let source = Some(text)
            .filter(|text| text.c >= NEUTRAL_TEXT_TINT)
            .or(panels)
            .or_else(|| Oklch::of(&self.ui.accent))
            .unwrap_or(text);
        let c = source.c.clamp(NEUTRAL_MIN_CHROMA, NEUTRAL_MAX_CHROMA);
        let light = text.l > ground.l;
        let tone = |l: f64| Oklch { l, c, h: source.h }.hex();
        let first = tone(if light { NEUTRAL_LIGHT } else { NEUTRAL_DARK });
        if delta_e(&first, &self.ui.text) >= NEUTRAL_FROM_TEXT {
            return first;
        }
        // Back from the text towards the chart, which keeps the tint: past
        // a near-white text there is too little room left for any.
        tone(if light { text.l - NEUTRAL_FROM_TEXT_STEP } else { text.l + NEUTRAL_FROM_TEXT_STEP })
    }

    /// The nth colour to hand an overlay that has not been given one.
    ///
    /// Follows [`SWATCH_SEQUENCE`], so the first few indicators on a chart are
    /// far apart in hue and none of them is wearing the candles' colours.
    pub fn series(&self, n: usize) -> String {
        if self.swatches.is_empty() {
            return self.ui.accent.clone();
        }
        let name = SWATCH_SEQUENCE[n % SWATCH_SEQUENCE.len()];
        self.swatch(name)
            .map(|s| s.hex.clone())
            .unwrap_or_else(|| self.swatches[n % self.swatches.len()].hex.clone())
    }

    /// The fill that belongs under a line of colour `hex`.
    ///
    /// Bands, clouds and shaded zones need the same hue sitting quietly behind
    /// the line rather than competing with it. Deriving the fill from the line
    /// and the theme's own background — rather than storing a second colour —
    /// is what keeps it harmonious in every theme, including one the user
    /// edited at midnight.
    pub fn fill_for(&self, hex: &str) -> String {
        mix(hex, &self.ui.background, 0.82)
    }

    /// A softer line, for the second half of a pair — a signal line against
    /// its indicator, the slow half of a cross.
    pub fn muted_for(&self, hex: &str) -> String {
        mix(hex, &self.ui.background, 0.38)
    }

    /// A colour to set against `hex`, from this theme's own palette.
    ///
    /// The next one along the sequence that reads as a different colour, so
    /// a mark drawn on top of something in `hex` — %D over %K, a point of
    /// control over its profile — is a different thing rather than a brighter
    /// patch of the same one.
    ///
    /// Consecutive entries were ordered to be far apart in hue, so that is
    /// nearly always the very next one. The exception is the wrap: the last
    /// entry's neighbour is the first, Cyan beside Blue, and in a fair share
    /// of palettes those two are near enough to be taken for each other. The
    /// sequence is not reordered to fix that, because its order is what every
    /// chart's first few overlays wear; the companion skips ahead instead, to
    /// the first that clears [`COMPANION_GAP`], and only when nothing does
    /// settles for the neighbour. A `hex` from outside the sequence is held
    /// to the same test, from the top.
    pub fn companion(&self, hex: &str) -> String {
        let len = SWATCH_SEQUENCE.len();
        let at = (0..len).find(|n| self.series(*n).eq_ignore_ascii_case(hex)).unwrap_or(0);
        (1..len)
            .map(|step| self.series(at + step))
            .find(|candidate| delta_e(hex, candidate) >= COMPANION_GAP)
            .unwrap_or_else(|| self.series(at + 1))
    }

    /// A second line's colour: the one somebody chose, or else the companion
    /// to the main line's `main`.
    pub fn companion_or(&self, choice: Option<&ColorChoice>, main: &str) -> String {
        choice.map(|choice| choice.resolve(self)).unwrap_or_else(|| self.companion(main))
    }

    /// The nth overlay's line and fill together.
    pub fn series_pair(&self, n: usize) -> (String, String) {
        let line = self.series(n);
        let fill = self.fill_for(&line);
        (line, fill)
    }
}

// ---------------------------------------------------------------------------
// Bar scheme
// ---------------------------------------------------------------------------

/// Which way a thing moved.
///
/// The single definition of up and down in the app. Everything that colours by
/// direction — candles, volume, a watchlist's change column — asks a
/// [`BarScheme`] for the colour of a `Direction` rather than comparing numbers
/// and reaching for its own green. One definition, one palette, no drift.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Direction {
    Up,
    Down,
    Flat,
}

impl Direction {
    /// A bar's direction: a close above its open is up, equal is flat.
    pub fn of_bar(open: f64, close: f64) -> Direction {
        Direction::of_change(close - open)
    }

    /// A move's direction. Exact zero is flat; anything else commits.
    pub fn of_change(delta: f64) -> Direction {
        if delta > 0.0 {
            Direction::Up
        } else if delta < 0.0 {
            Direction::Down
        } else {
            Direction::Flat
        }
    }

    /// Stable CSS class, so widgets colour themselves the same way the chart
    /// does.
    pub fn css_class(self) -> &'static str {
        match self {
            Direction::Up => "change-up",
            Direction::Down => "change-down",
            Direction::Flat => "change-flat",
        }
    }
}

/// The colours candles are drawn in, resolved against a theme from a
/// [`BarLook`].
///
/// Colour, edge and body are separate so a candle can be hollow (a
/// transparent body), solid (body == edge), or bordered (edge != body) with
/// no other machinery.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct BarScheme {
    /// The direction's colour: what an OHLC bar, a line or a price tag is
    /// drawn in.
    pub up: String,
    /// A candle's wick and the edge of its body.
    pub up_edge: String,
    pub up_fill: String,
    pub down: String,
    pub down_edge: String,
    pub down_fill: String,
    /// Used when open == close.
    pub neutral: String,
}

impl BarScheme {
    /// The outline colour for a direction.
    pub fn outline(&self, direction: Direction) -> &str {
        match direction {
            Direction::Up => &self.up,
            Direction::Down => &self.down,
            Direction::Flat => &self.neutral,
        }
    }

    /// A candle's wick and body edge for a direction.
    pub fn edge(&self, direction: Direction) -> &str {
        match direction {
            Direction::Up => &self.up_edge,
            Direction::Down => &self.down_edge,
            Direction::Flat => &self.neutral,
        }
    }

    /// The body colour for a direction. Fully transparent means hollow.
    pub fn body(&self, direction: Direction) -> &str {
        match direction {
            Direction::Up => &self.up_fill,
            Direction::Down => &self.down_fill,
            Direction::Flat => &self.neutral,
        }
    }

    /// The volume colour for a direction: the bar's own outline, never
    /// hollow. A volume pane's opacity is what sets it back from the candles.
    pub fn volume(&self, direction: Direction) -> &str {
        self.outline(direction)
    }
}

// ---------------------------------------------------------------------------
// What we ship
// ---------------------------------------------------------------------------

/// The theme that follows the desktop. Default wherever Omarchy is present.
pub const OMARCHY_ID: &str = "omarchy";
/// Fallback theme when Omarchy is not installed: Omarchy's own default.
pub const FALLBACK_THEME_ID: &str = "tokyo-night";

/// The themes offered on a machine without Omarchy: some of Omarchy's own,
/// built into the binary. [`FALLBACK_THEME_ID`] first.
pub fn builtin_themes() -> Vec<Theme> {
    crate::omarchy::bundled()
}

/// Candles built from the active theme's own palette.
///
/// The default, and what makes the chart match the desktop on Omarchy: the
/// theme supplies green and rose, so candles shift with it instead of staying
/// a fixed pair of hexes that clash with half the themes available.
pub fn theme_bars(theme: &Theme) -> BarScheme {
    BarLook::default().scheme(theme)
}

/// What somebody chose candles to look like, per direction: a colour, and
/// how the body is drawn in it.
///
/// Separate answers rather than a list of schemes, because they are the
/// questions people arrive with — red for up, no colour at all, hollow rises,
/// TradingView's outline — and a list can only answer the combinations
/// somebody thought to put in it. The colours are roles the theme fills, so
/// candles follow the theme.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct BarLook {
    pub up: ColorChoice,
    pub down: ColorChoice,
    pub up_body: Body,
    pub down_body: Body,
}

/// How a candle's body is drawn in its colour.
///
/// One answer of three rather than a hollow and a border switch: a hollow
/// candle with a border is an outline in the border colour, which is the
/// colour [`FOREGROUND`] already offers.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Body {
    #[default]
    Filled,
    /// The wick and the body's edge in [`foreground`], the theme's text, the way
    /// TradingView outlines a candle.
    Bordered,
    Hollow,
}

impl Body {
    pub const ALL: [Body; 3] = [Body::Filled, Body::Bordered, Body::Hollow];

    /// As it is stored and typed.
    pub fn key(self) -> &'static str {
        match self {
            Body::Filled => "filled",
            Body::Bordered => "bordered",
            Body::Hollow => "hollow",
        }
    }

    pub fn from_key(key: &str) -> Option<Body> {
        Body::ALL.into_iter().find(|body| body.key().eq_ignore_ascii_case(key))
    }
}

/// The neutral a bar can be drawn in when it should carry no direction: the
/// theme's text with its tint taken out. Offered beside the theme's swatches,
/// under Omarchy's name for the text, so that picking it on a dark theme
/// plainly means light candles and on a light one dark.
pub const FOREGROUND: &str = "Foreground";

/// What [`FOREGROUND`] was called before, still read from a stored setting.
const FOREGROUND_WAS: &str = "ink";

/// A body that is not painted: the outline is drawn and the chart shows
/// through.
pub const HOLLOW: &str = "#00000000";

impl Default for BarLook {
    /// Green up, Rose down, both filled: "typically red/green" is what people
    /// expect, and the theme palette is where those two live.
    fn default() -> BarLook {
        BarLook {
            up: ColorChoice::swatch("Green"),
            down: ColorChoice::swatch("Rose"),
            up_body: Body::Filled,
            down_body: Body::Filled,
        }
    }
}

impl BarLook {
    /// The theme's colours the other way round: red for a rise, green for a
    /// fall, the way Taiwan, mainland China, Japan and Korea read a chart.
    pub fn red_up() -> BarLook {
        BarLook { up: ColorChoice::swatch("Rose"), down: ColorChoice::swatch("Green"), ..BarLook::default() }
    }

    /// One neutral for every bar, for people who would rather read a chart
    /// than be signalled at by one. Direction is simply not shown, unless a
    /// hollow body is asked to show it.
    pub fn monochrome() -> BarLook {
        BarLook { up: ColorChoice::swatch(FOREGROUND), down: ColorChoice::swatch(FOREGROUND), ..BarLook::default() }
    }

    /// The colours on a theme.
    pub fn scheme(&self, theme: &Theme) -> BarScheme {
        let up = bar_colour(theme, &self.up);
        let down = bar_colour(theme, &self.down);
        let fill = |colour: &String, body: Body| match body {
            Body::Hollow => HOLLOW.to_string(),
            _ => colour.clone(),
        };
        let edge = |colour: &String, body: Body| match body {
            Body::Bordered => foreground(theme),
            _ => colour.clone(),
        };
        BarScheme {
            up_edge: edge(&up, self.up_body),
            down_edge: edge(&down, self.down_body),
            up_fill: fill(&up, self.up_body),
            down_fill: fill(&down, self.down_body),
            // An unchanged bar between two directions that look the same
            // would otherwise be the one bar with a colour.
            neutral: match up == down {
                true => up.clone(),
                false => theme.ui.text_muted.clone(),
            },
            up,
            down,
        }
    }
}

/// A bar colour as it is stored and typed: a swatch's name in lower case,
/// `foreground`, or a hex.
pub fn spell_bar_colour(choice: &ColorChoice) -> String {
    match choice {
        ColorChoice::Swatch { name } => name.to_lowercase(),
        ColorChoice::Fixed { hex } => hex.clone(),
    }
}

/// The other way: any case of a swatch's name or `foreground`, or a
/// `#rrggbb` hex.
pub fn parse_bar_colour(text: &str) -> Option<ColorChoice> {
    let text = text.trim();
    if text.starts_with('#') {
        return rgb(text).map(|_| ColorChoice::Fixed { hex: text.to_lowercase() });
    }
    if text.eq_ignore_ascii_case(FOREGROUND_WAS) {
        return Some(ColorChoice::swatch(FOREGROUND));
    }
    SWATCH_NAMES
        .iter()
        .chain([&FOREGROUND])
        .find(|name| name.eq_ignore_ascii_case(text))
        .map(|name| ColorChoice::swatch(name))
}

/// A bar colour on a theme: one of its swatches, [`FOREGROUND`], or a fixed hex.
pub fn bar_colour(theme: &Theme, choice: &ColorChoice) -> String {
    match choice {
        ColorChoice::Swatch { name } if name == FOREGROUND => foreground(theme),
        choice => choice.resolve(theme),
    }
}

/// What a monochrome bar is held to against the chart behind it.
///
/// A floor well above the grid's, because this is not furniture: with the
/// direction colours spent, this one colour is the entire price series, and
/// everything the chart is for is read off it. The ceiling exists because a
/// chart is a dense field of these rather than a line of text, and a theme
/// whose foreground is near-white on near-black would otherwise hand back a
/// wall of glare.
pub const MONO_CONTRAST: ContrastBand = ContrastBand::new(4.5, 10.0);

/// How much colour a monochrome bar keeps.
///
/// Not none, which is what makes it the theme's monochrome rather than a
/// generic grey. A foreground is rarely a dead neutral — Gruvbox's is a warm
/// sand, Nord's a cool slate — and keeping a trace of that is what stops a
/// quiet chart looking like one pasted in from another application.
///
/// It is a cap and not a target: a theme whose text is already neutral keeps
/// its neutral, and only the tinted ones are pulled back to here. Staying
/// clear of the direction colours is [`set_apart`]'s job rather than this
/// one's — some palettes derive a Green with less chroma than this, so a cap
/// alone could never have promised it.
const MONO_CHROMA: f64 = 0.03;

/// How far a monochrome bar must sit from the colours it replaces.
///
/// The usual threshold for two colours reading as one. It matters here
/// because some palettes have no green at all — Omarchy's `vantablack` and
/// `white` are both of them — and the Green swatch derived for those is a
/// plain grey. Left alone, the neutral would land on it, and every bar in a
/// monochrome chart would be the exact colour the coloured chart uses for
/// *up*: not a quieter chart, a chart quietly claiming the market only ever
/// rose.
const MONO_APART: f64 = 0.06;

/// Move `colour`'s lightness until it is at least `minimum` from everything in
/// `others`, without leaving `band` against `ground`.
///
/// Lightness rather than hue, because the thing being moved is meant to be
/// neutral and a hue is exactly what it must not acquire. Nearer steps are
/// tried before further ones and both directions at each distance, so the
/// answer is the smallest move that works rather than the first direction
/// that happens to. A colour with nowhere to go inside the band is returned
/// unchanged: being slightly too close to a swatch is a smaller failure than
/// being too dark to see.
fn set_apart(
    colour: &str,
    others: &[&str],
    ground: &str,
    band: ContrastBand,
    minimum: f64,
) -> String {
    let clears = |hex: &str| others.iter().all(|other| delta_e(hex, other) >= minimum);
    if clears(colour) {
        return colour.to_string();
    }
    let Some(base) = Oklch::of(colour) else { return colour.to_string() };
    for step in 1..=60 {
        for away in [-1.0, 1.0] {
            let candidate = base.with_lightness(base.l + away * step as f64 * LIGHTNESS_STEP).hex();
            if band.holds(contrast_ratio(&candidate, ground)) && clears(&candidate) {
                return candidate;
            }
        }
    }
    colour.to_string()
}

/// Candles in [`FOREGROUND`] both ways, filled.
///
/// Direction is not shown. Hollow rises keep it at the cost of some of the
/// quiet, and that is a separate answer, since it vanishes anyway below about
/// three pixels a bar, where candles are drawn as hairlines with no body to
/// fill. An OHLC chart loses nothing at all, since its open and close ticks
/// say which way the bar went without reference to colour.
pub fn theme_mono_bars(theme: &Theme) -> BarScheme {
    BarLook::monochrome().scheme(theme)
}

/// How one chart paints the bars: the look chosen for every chart, as it is
/// or turned one of three ways. The colours stay the [`BarLook`]'s; this is
/// only what a chart does with them.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum BarColouring {
    #[default]
    Filled,
    Hollow,
    RedUp,
    /// A slate against a sand, from the theme's palette.
    Soft,
    /// Two greys made from the theme's text.
    Quiet,
    Monochrome,
}

/// The ids a colouring is stored under, the same ids bar schemes were.
pub const THEME_BARS_ID: &str = "theme";
pub const THEME_HOLLOW_ID: &str = "theme-hollow";
pub const THEME_RED_UP_ID: &str = "theme-red-up";
pub const THEME_MONO_ID: &str = "theme-mono";
pub const THEME_SOFT_ID: &str = "theme-soft";
pub const THEME_QUIET_ID: &str = "theme-quiet";

impl BarColouring {
    pub const ALL: [BarColouring; 6] = [
        BarColouring::Filled,
        BarColouring::Hollow,
        BarColouring::RedUp,
        BarColouring::Soft,
        BarColouring::Quiet,
        BarColouring::Monochrome,
    ];

    pub fn key(self) -> &'static str {
        match self {
            BarColouring::Filled => THEME_BARS_ID,
            BarColouring::Hollow => THEME_HOLLOW_ID,
            BarColouring::RedUp => THEME_RED_UP_ID,
            BarColouring::Soft => THEME_SOFT_ID,
            BarColouring::Quiet => THEME_QUIET_ID,
            BarColouring::Monochrome => THEME_MONO_ID,
        }
    }

    /// Schemes an older version stored come back as the colouring they
    /// painted; anything else unknown is not a colouring.
    pub fn from_key(key: &str) -> Option<BarColouring> {
        match key {
            "hollow" => Some(BarColouring::Hollow),
            "monochrome" => Some(BarColouring::Monochrome),
            key => BarColouring::ALL.into_iter().find(|c| c.key() == key),
        }
    }

    /// An OHLC bar has no body to fill or leave hollow, so filled is simply
    /// "coloured" there.
    pub fn label(self, style: BarStyle) -> &'static str {
        match self {
            BarColouring::Filled if style == BarStyle::Ohlc => "Coloured",
            BarColouring::Filled => "Filled",
            BarColouring::Hollow => "Hollow",
            BarColouring::RedUp => "Red up",
            BarColouring::Soft => "Soft",
            BarColouring::Quiet => "Quiet",
            BarColouring::Monochrome => "Monochrome",
        }
    }

    /// What a bar of this style can be painted as.
    pub fn offered(style: BarStyle) -> &'static [BarColouring] {
        match style {
            BarStyle::Candles => &BarColouring::ALL,
            BarStyle::Ohlc => &[
                BarColouring::Filled,
                BarColouring::RedUp,
                BarColouring::Soft,
                BarColouring::Quiet,
                BarColouring::Monochrome,
            ],
        }
    }

    /// What a bar of this style is actually painted as.
    pub fn for_style(self, style: BarStyle) -> BarColouring {
        if BarColouring::offered(style).contains(&self) { self } else { BarColouring::Filled }
    }

    /// The look, turned the way this colouring turns it. Soft and Quiet
    /// put their own pair on the theme in place of the look's colours,
    /// keeping its hollow bodies.
    pub fn look(self, theme: &Theme, look: &BarLook) -> BarLook {
        let pair = |(up, down): (String, String)| BarLook {
            up: ColorChoice::Fixed { hex: up },
            down: ColorChoice::Fixed { hex: down },
            ..look.clone()
        };
        match self {
            BarColouring::Soft => pair(slate_and_sand(theme)),
            BarColouring::Quiet => pair(quiet_greys(theme)),
            BarColouring::Filled => look.clone(),
            BarColouring::Hollow => BarLook { up_body: Body::Hollow, ..look.clone() },
            BarColouring::RedUp => BarLook {
                up: look.down.clone(),
                down: look.up.clone(),
                up_body: look.down_body,
                down_body: look.up_body,
            },
            BarColouring::Monochrome => {
                BarLook { up: ColorChoice::swatch(FOREGROUND), down: ColorChoice::swatch(FOREGROUND), ..look.clone() }
            }
        }
    }

    pub fn scheme(self, theme: &Theme, look: &BarLook) -> BarScheme {
        self.look(theme, look).scheme(theme)
    }
}

/// What a soft bar is held to against the chart behind it.
///
/// Well below the coloured pair, which sits at 5:1 to 9:1 on most themes,
/// and below the monochrome floor too: a soft chart is one in which the
/// bars sit back, and a soft bar has a second colour beside it to be read
/// against. Still at WCAG's 3:1 for a graphic that carries meaning, since
/// a wick is a hairline of the same colour. The ceiling is where it stops
/// sitting back.
pub const SOFT_CONTRAST: ContrastBand = ContrastBand::new(3.0, 3.8);

/// The least a soft rise may be from a soft fall, as `delta_e` sees it:
/// above the 0.02 the eye can only just separate, below the 0.08 overlays
/// are held to, since a rise and a fall may be allowed a second look.
pub const SOFT_APART: f64 = 0.05;

/// How much colour a soft bar carries: enough to be a slate rather than a
/// grey, not enough to be a blue. Raised a little at a time, only for a
/// pair whose hues are nearer each other than [`SOFT_APART`] allows.
const SOFT_CHROMA: f64 = 0.055;

/// Below this a colour has no hue worth keeping.
const GREY: f64 = 0.01;

/// Where a slate blue and a sand sit on the wheel, for a theme that has
/// neither to offer.
const SLATE: f64 = 255.0;
const SAND: f64 = 75.0;

/// The soft pair, up first: the coolest of the theme's blues against the
/// warmest of its ambers, neither saying good or bad, both at the one
/// lightness of the theme's text brought into [`SOFT_CONTRAST`], so they
/// differ by hue alone. A theme whose blue and amber lie near each other
/// gets a little more chroma until the pair clears [`SOFT_APART`].
fn slate_and_sand(theme: &Theme) -> (String, String) {
    let slate = nearest_hue(theme, &["Blue", "Cyan", "Teal"], SLATE);
    let sand = nearest_hue(theme, &["Amber", "Orange"], SAND);
    let grey = held_to(&theme.ui.text, &theme.ui.background, SOFT_CONTRAST, Some(0.0));
    let Some(grey) = Oklch::of(&grey) else {
        return (theme.ui.text.clone(), theme.ui.text_muted.clone());
    };
    let mut more = 0.0;
    loop {
        let paint = |h: f64| Oklch { l: grey.l, c: SOFT_CHROMA + more, h }.hex();
        let pair = (paint(slate), paint(sand));
        if delta_e(&pair.0, &pair.1) >= SOFT_APART || more >= 0.08 {
            return pair;
        }
        more += 0.005;
    }
}

/// The hue of a named swatch, if the theme has it and it has one.
fn swatch_hue(theme: &Theme, name: &str) -> Option<f64> {
    let swatch = theme.swatch(name)?;
    let colour = Oklch::of(&swatch.hex)?;
    (colour.c >= GREY).then_some(colour.h)
}

/// Of the swatches named, the hue nearest `ideal`; `ideal` itself when the
/// theme has none of them.
fn nearest_hue(theme: &Theme, names: &[&str], ideal: f64) -> f64 {
    names
        .iter()
        .filter_map(|name| swatch_hue(theme, name))
        .min_by(|a, b| hue_gap(*a, ideal).total_cmp(&hue_gap(*b, ideal)))
        .unwrap_or(ideal)
}

/// Where the nearer of the two quiet greys sits against the chart: WCAG's
/// 3:1 for a graphic that carries meaning, and no further.
pub const QUIET_NEAR: ContrastBand = ContrastBand::new(3.0, 3.15);

/// Where the further grey sits: under the coloured pair and Monochrome's
/// ceiling, but a decent contrast, about 1.65:1 from the nearer one on
/// every theme, since both are derived from the same ground.
pub const QUIET_FAR: ContrastBand = ContrastBand::new(5.0, 5.2);

/// The most colour a quiet grey keeps: a trace of the theme's text, so a
/// warm theme's greys lean warm without being a colour.
pub const QUIET_GREY: f64 = 0.012;

/// The quiet pair, up first: two greys of the theme's text, a rise the
/// lighter and a fall the darker, as a black-and-white chart has them.
fn quiet_greys(theme: &Theme) -> (String, String) {
    let grey = |band| held_to(&theme.ui.text, &theme.ui.background, band, Some(QUIET_GREY));
    let (near, far) = (grey(QUIET_NEAR), grey(QUIET_FAR));
    let lightness = |hex: &str| Oklch::of(hex).map_or(0.0, |c| c.l);
    if lightness(&far) > lightness(&near) { (far, near) } else { (near, far) }
}

/// Written down as its key, the id the settings have always stored, so an
/// indicator that chose a colouring is read back by the same name a chart's
/// is.
impl Serialize for BarColouring {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.key())
    }
}

impl<'de> Deserialize<'de> for BarColouring {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let key = String::deserialize(deserializer)?;
        BarColouring::from_key(&key)
            .ok_or_else(|| serde::de::Error::custom(format!("{key:?} is not a bar colouring")))
    }
}

/// The [`FOREGROUND`] colour on a theme: the text, held to a bar's contrast
/// and moved clear of the colours it stands in for.
pub fn foreground(theme: &Theme) -> String {
    let green = ColorChoice::swatch("Green").resolve(theme);
    let rose = ColorChoice::swatch("Rose").resolve(theme);
    let ink = held_to(&theme.ui.text, &theme.ui.background, MONO_CONTRAST, Some(MONO_CHROMA));
    set_apart(&ink, &[&green, &rose], &theme.ui.background, MONO_CONTRAST, MONO_APART)
}

/// Blend `a` toward `b`. `t` of 0 is all `a`, 1 is all `b`.
pub fn mix(a: &str, b: &str, t: f64) -> String {
    let (Some((ar, ag, ab)), Some((br, bg, bb))) = (rgb(a), rgb(b)) else {
        return a.to_string();
    };
    let lerp = |x: u8, y: u8| (x as f64 + (y as f64 - x as f64) * t).round() as u8;
    format!("#{:02x}{:02x}{:02x}", lerp(ar, br), lerp(ag, bg), lerp(ab, bb))
}

/// How far apart two colours are, 0 to 1.
///
/// Euclidean in RGB. Crude next to [`delta_e`], and kept only for the
/// furniture: nudging a grid line or a panel just off the background is a
/// question of "is this a different pixel value at all", where the tuned
/// thresholds in RGB do the job. Anything that asks whether two colours
/// *look* alike belongs on [`delta_e`].
pub fn distance(a: &str, b: &str) -> f64 {
    let (Some(a), Some(b)) = (rgb(a), rgb(b)) else { return 0.0 };
    let d = |x: u8, y: u8| (x as f64 - y as f64) / 255.0;
    ((d(a.0, b.0).powi(2) + d(a.1, b.1).powi(2) + d(a.2, b.2).powi(2)) / 3.0).sqrt()
}

/// Perceived lightness of a colour, 0 to 1.
pub fn luminance(hex: &str) -> f64 {
    let Some((r, g, b)) = rgb(hex) else { return 0.0 };
    (0.2126 * r as f64 + 0.7152 * g as f64 + 0.0722 * b as f64) / 255.0
}

/// Black or white, whichever can be read on `hex`.
///
/// Anything that paints text on a colour taken from a theme has to ask: a
/// theme is free to make its up and down dark, and eighteen of the twenty-two
/// shipped ones do for at least one direction. A fixed near-black label on a
/// dark pill is a number nobody can read.
pub fn readable_on(hex: &str) -> &'static str {
    if luminance(hex) > 0.55 { "#000000" } else { "#ffffff" }
}

/// Nudge `colour` away from `from` by blending it toward `toward`, until it is
/// at least `minimum` away or we run out of room.
///
/// Used where a theme gives us a colour that collides with another — a grid
/// line the same hex as the background, say. Blending rather than substituting
/// keeps the theme's character.
pub fn ensure_distinct(colour: &str, from: &str, minimum: f64, toward: &str) -> String {
    if distance(colour, from) >= minimum {
        return colour.to_string();
    }
    let mut blended = colour.to_string();
    for step in 1..=12 {
        blended = mix(colour, toward, step as f64 * 0.06);
        if distance(&blended, from) >= minimum {
            return blended;
        }
    }
    blended
}

// ---------------------------------------------------------------------------
// Perceptual colour
// ---------------------------------------------------------------------------

/// A colour as lightness, chroma and hue in OKLCh.
///
/// This is the space the palette is reasoned about in, because it is the one
/// where the numbers mean what the eye sees. Moving a colour's `h` keeps it
/// exactly as bright and as vivid; two colours the same distance apart here
/// look the same distance apart, whether they are two blues or a yellow and a
/// grey. RGB and HSL both lie about that: in HSL a pure yellow and a pure
/// blue have the same "lightness", and one glows while the other is nearly
/// black.
///
/// `l` runs 0 to 1, `c` from 0 (grey) to roughly 0.37 at the most vivid the
/// screen can show, `h` in degrees.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Oklch {
    pub l: f64,
    pub c: f64,
    pub h: f64,
}

impl Oklch {
    pub fn of(hex: &str) -> Option<Oklch> {
        let (l, a, b) = oklab(hex)?;
        let c = (a * a + b * b).sqrt();
        // Below this the hue is numerical noise, and a grey rotated by any
        // angle must stay the same grey.
        let h = if c < 0.0005 { 0.0 } else { b.atan2(a).to_degrees().rem_euclid(360.0) };
        Some(Oklch { l, c, h })
    }

    pub fn with_lightness(self, l: f64) -> Oklch {
        Oklch { l: l.clamp(0.0, 1.0), ..self }
    }

    pub fn with_hue(self, h: f64) -> Oklch {
        Oklch { h: h.rem_euclid(360.0), ..self }
    }

    /// Back to a hex colour the screen can show.
    ///
    /// Not every lightness and hue can be had at every chroma: there is no
    /// very light saturated blue, and no very dark saturated yellow. When the
    /// asked-for colour falls outside what sRGB can display, the chroma is
    /// pulled in until it fits, which keeps the lightness and hue, the two
    /// things the caller actually chose, and only makes it a little quieter.
    pub fn hex(self) -> String {
        let mut c = self.c;
        for _ in 0..24 {
            if let Some(hex) = from_oklab(self.l, c * self.h.to_radians().cos(), c * self.h.to_radians().sin()) {
                return hex;
            }
            c *= 0.9;
        }
        from_oklab(self.l, 0.0, 0.0).unwrap_or_else(|| "#808080".to_string())
    }
}

/// The angle from hue `a` to hue `b`, going the short way round. 0 to 180.
pub fn hue_gap(a: f64, b: f64) -> f64 {
    let d = (a - b).rem_euclid(360.0);
    d.min(360.0 - d)
}

/// How different two colours look, 0 to about 1.
///
/// Euclidean distance in OKLab, so a step of the same size looks the same
/// size wherever on the wheel it happens. Black to white is exactly 1; two
/// colours the eye can only just separate are about 0.02 apart, and the
/// distance between a dark blue and a dark purple that read as "the same line"
/// on a chart is around 0.06. Those are the numbers the palette's thresholds
/// are written in.
pub fn delta_e(a: &str, b: &str) -> f64 {
    let (Some(a), Some(b)) = (oklab(a), oklab(b)) else { return 0.0 };
    ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2) + (a.2 - b.2).powi(2)).sqrt()
}

/// WCAG contrast ratio between two colours, 1 to 21.
///
/// The one number accessibility guidance actually sets floors for: 3:1 is the
/// minimum for graphics that carry meaning, which is exactly what a thin
/// indicator line is. OKLab says how *different* two colours look; this says
/// whether a line can be *seen* on its ground, which is a question about
/// light, not about hue.
pub fn contrast_ratio(a: &str, b: &str) -> f64 {
    let (Some(a), Some(b)) = (relative_luminance(a), relative_luminance(b)) else { return 1.0 };
    let (hi, lo) = if a > b { (a, b) } else { (b, a) };
    (hi + 0.05) / (lo + 0.05)
}

/// A range of contrast a piece of the chart's furniture is held to against
/// its ground: enough to be seen, not enough to be noticed.
///
/// The floor is the usual question — can this line be seen at all. The
/// ceiling is the one the grid used to lack: a theme is free to hand us a
/// grey that is a perfectly good terminal selection colour and a lattice
/// across the chart, and a floor alone lets it through.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct ContrastBand {
    pub floor: f64,
    pub ceiling: f64,
}

impl ContrastBand {
    pub const fn new(floor: f64, ceiling: f64) -> ContrastBand {
        ContrastBand { floor, ceiling }
    }

    pub fn holds(&self, ratio: f64) -> bool {
        (self.floor..=self.ceiling).contains(&ratio)
    }
}

/// How far a lightness walk moves per step. Near the quiet end of the scale
/// one step is about 0.01 of contrast, which is finer than any band is.
pub const LIGHTNESS_STEP: f64 = 0.005;

/// Hold `colour` to `band` of contrast against `ground`, with its chroma at
/// most `max_chroma` if one is given.
///
/// A colour already inside the band is returned exactly as it was: the
/// theme's own colour is the right answer whenever it is an acceptable one.
/// Otherwise only its lightness moves, and only as far as it must. A colour
/// that is too loud walks toward the ground until it is just inside the
/// ceiling, so a repaired theme stays as close as it can to what the theme
/// asked for. A colour that is too quiet is rebuilt from the ground's own
/// lightness, walking away from it — lighter on a dark theme, darker on a
/// light one — until it reaches the floor, which is also what happens to a
/// colour that sat on the wrong side of the ground, where walking away would
/// only have found the end of the scale.
///
/// WCAG contrast rather than OKLab distance, for the same reason the overlay
/// floors use it: the question is whether a line can be seen on its ground,
/// which is a question about light. OKLab has no toe at black, so it calls
/// a near-black grid on a black chart a tenth of the way to white, which is
/// not what the eye sees.
pub fn held_to(colour: &str, ground: &str, band: ContrastBand, max_chroma: Option<f64>) -> String {
    let (Some(ground_l), Some(mut c)) = (Oklch::of(ground), Oklch::of(colour)) else {
        return colour.to_string();
    };
    let chroma_ok = max_chroma.is_none_or(|max| c.c <= max);
    if band.holds(contrast_ratio(colour, ground)) && chroma_ok {
        return colour.to_string();
    }
    if let Some(max) = max_chroma {
        c.c = c.c.min(max);
    }
    let ratio = contrast_ratio(&c.hex(), ground);
    if ratio > band.ceiling {
        let toward = if c.l > ground_l.l { -LIGHTNESS_STEP } else { LIGHTNESS_STEP };
        for _ in 0..200 {
            if contrast_ratio(&c.hex(), ground) <= band.ceiling {
                break;
            }
            c = c.with_lightness(c.l + toward);
        }
    } else if ratio < band.floor {
        let away = if ground_l.l < 0.5 { LIGHTNESS_STEP } else { -LIGHTNESS_STEP };
        c = c.with_lightness(ground_l.l);
        for _ in 0..200 {
            if contrast_ratio(&c.hex(), ground) >= band.floor {
                break;
            }
            c = c.with_lightness(c.l + away);
        }
    }
    c.hex()
}

/// APCA lightness contrast, Lc, of `text` on `ground`: the WCAG 3 candidate
/// measure, version 0.0.98G-4g.
///
/// WCAG 2's ratio is symmetric and blind to polarity, so on a mid-tone,
/// saturated colour it prefers black even where white is what the eye reads
/// more easily. APCA models that: it weighs dark-on-light and light-on-dark
/// differently, and it gives a red or a green the light label people see as
/// the clearer one. Positive for dark text on a light ground, negative for
/// light on dark; |Lc| 60 is its line for small bold text such as a figure on
/// a pill, and 90 is near black on white.
pub fn apca_contrast(text: &str, ground: &str) -> f64 {
    let y = |hex: &str| -> Option<f64> {
        let (r, g, b) = rgb(hex)?;
        let ch = |v: u8| (v as f64 / 255.0).powf(2.4);
        let y = 0.2126729 * ch(r) + 0.7151522 * ch(g) + 0.0721750 * ch(b);
        // Soft-clamp near black, where screens and eyes flatten out.
        Some(if y < 0.022 { y + (0.022 - y).powf(1.414) } else { y })
    };
    let (Some(txt), Some(bg)) = (y(text), y(ground)) else { return 0.0 };
    if (bg - txt).abs() < 0.0005 {
        return 0.0;
    }
    let lc = if bg > txt {
        let sapc = (bg.powf(0.56) - txt.powf(0.57)) * 1.14;
        if sapc < 0.1 { 0.0 } else { sapc - 0.027 }
    } else {
        let sapc = (bg.powf(0.65) - txt.powf(0.62)) * 1.14;
        if sapc > -0.1 { 0.0 } else { sapc + 0.027 }
    };
    lc * 100.0
}

/// Luminance as physics counts it: linear light, not gamma-encoded bytes.
fn relative_luminance(hex: &str) -> Option<f64> {
    let (r, g, b) = linear_rgb(hex)?;
    Some(0.2126 * r + 0.7152 * g + 0.0722 * b)
}

fn linear_rgb(hex: &str) -> Option<(f64, f64, f64)> {
    let (r, g, b) = rgb(hex)?;
    let lin = |v: u8| {
        let c = v as f64 / 255.0;
        if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
    };
    Some((lin(r), lin(g), lin(b)))
}

/// sRGB to OKLab, by Björn Ottosson's published matrices.
pub fn oklab(hex: &str) -> Option<(f64, f64, f64)> {
    let (r, g, b) = linear_rgb(hex)?;
    let l = (0.412_221_470_8 * r + 0.536_332_536_3 * g + 0.051_445_992_9 * b).cbrt();
    let m = (0.211_903_498_2 * r + 0.680_699_545_1 * g + 0.107_396_956_6 * b).cbrt();
    let s = (0.088_302_461_9 * r + 0.281_718_837_6 * g + 0.629_978_700_5 * b).cbrt();
    Some((
        0.210_454_255_3 * l + 0.793_617_785 * m - 0.004_072_046_8 * s,
        1.977_998_495_1 * l - 2.428_592_205 * m + 0.450_593_709_9 * s,
        0.025_904_037_1 * l + 0.782_771_766_2 * m - 0.808_675_766 * s,
    ))
}

/// OKLab back to sRGB. `None` when the colour cannot be displayed, so the
/// caller can decide what to give up.
fn from_oklab(big_l: f64, a: f64, b: f64) -> Option<String> {
    let l = (big_l + 0.396_337_777_4 * a + 0.215_803_757_3 * b).powi(3);
    let m = (big_l - 0.105_561_345_8 * a - 0.063_854_172_8 * b).powi(3);
    let s = (big_l - 0.089_484_177_5 * a - 1.291_485_548 * b).powi(3);
    let r = 4.076_741_662_1 * l - 3.307_711_591_3 * m + 0.230_969_929_2 * s;
    let g = -1.268_438_004_6 * l + 2.609_757_401_1 * m - 0.341_319_396_5 * s;
    let b = -0.004_196_086_3 * l - 0.703_418_614_7 * m + 1.707_614_701 * s;
    // A hair outside the gamut is rounding, not a colour that cannot exist.
    let slack = 0.0005;
    if [r, g, b].iter().any(|v| *v < -slack || *v > 1.0 + slack) {
        return None;
    }
    let enc = |c: f64| {
        let c = c.clamp(0.0, 1.0);
        let v = if c <= 0.003_130_8 { 12.92 * c } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 };
        (v * 255.0).round() as u8
    };
    Some(format!("#{:02x}{:02x}{:02x}", enc(r), enc(g), enc(b)))
}

/// `#rgb`, `#rrggbb` or `#rrggbbaa` to bytes. Alpha is dropped.
pub fn rgb(hex: &str) -> Option<(u8, u8, u8)> {
    let h = hex.trim().trim_start_matches('#');
    let byte = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).ok();
    match h.len() {
        3 => {
            let d = |i: usize| {
                u8::from_str_radix(&h[i..i + 1], 16)
                    .ok()
                    .map(|v| v * 17)
            };
            Some((d(0)?, d(1)?, d(2)?))
        }
        6 | 8 => Some((byte(0)?, byte(2)?, byte(4)?)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A dark theme and a light one, from what ships.
    fn dark() -> Theme {
        builtin_themes().into_iter().find(|t| t.id == FALLBACK_THEME_ID).unwrap()
    }

    /// A theme of the given ground and text, with nothing else that matters
    /// to its neutral: the panels and the grid sit on the ground itself.
    fn grounded(mode: Mode, background: &str, text: &str, grid: &str) -> Theme {
        let mut theme = dark();
        theme.mode = mode;
        theme.ui.background = background.into();
        theme.ui.text = text.into();
        theme.ui.grid = grid.into();
        theme.ui.surface = background.into();
        theme.ui.surface_variant = background.into();
        theme.ui.border = background.into();
        theme
    }

    /// The neutral is a name like the others, stored as a name and
    /// resolved against whichever theme is showing: on a navy chart a very
    /// light lavender, the text's own hue a step lighter than the text, and
    /// on a warm white one a very dark warm brown, so it stands out from
    /// the chart rather than sitting on it.
    #[test]
    fn the_neutral_is_worked_out_from_a_dark_theme_and_from_a_light_one() {
        let neutral = ColorChoice::swatch(NEUTRAL);

        // Tokyo Night's ground and text.
        let dark = grounded(Mode::Dark, "#1a1b26", "#a9b1d6", "#24283b");
        let hex = neutral.resolve(&dark);
        assert_eq!(hex, dark.neutral());
        let (text, tone) = (Oklch::of("#a9b1d6").unwrap(), Oklch::of(&hex).unwrap());
        assert!(tone.l > 0.85, "{hex} is not very light");
        assert!(delta_e(&hex, "#1a1b26") >= 0.5, "{hex} does not stand out from the chart");
        assert!(delta_e(&hex, "#24283b") >= 0.5, "{hex} could be taken for the grid");
        assert!(delta_e(&hex, "#a9b1d6") >= NEUTRAL_FROM_TEXT, "{hex} is just the text colour");
        assert!(hue_gap(tone.h, text.h) < 10.0 && tone.c >= NEUTRAL_MIN_CHROMA, "{hex} lost the theme's lavender");

        // Paper: a warm white with warm near-black text, so a very dark
        // warm tone, not the text and not a black.
        let light = grounded(Mode::Light, "#fbf8f3", "#2d2a26", "#efe9de");
        let hex = neutral.resolve(&light);
        assert_eq!(hex, light.neutral());
        let (ground, tone) = (Oklch::of(&light.ui.background).unwrap(), Oklch::of(&hex).unwrap());
        assert!(tone.l < 0.4, "{hex} is not very dark");
        assert!(delta_e(&hex, &light.ui.background) >= 0.5, "{hex} does not stand out from the chart");
        assert!(delta_e(&hex, &light.ui.grid) >= 0.5, "{hex} could be taken for the grid");
        assert!(delta_e(&hex, &light.ui.text) >= NEUTRAL_FROM_TEXT, "{hex} is just the text colour");
        assert!(hue_gap(tone.h, ground.h) < 30.0 && tone.c >= 0.03, "{hex} lost Paper's warmth");

        // The same stored name is a different colour on each.
        assert_ne!(neutral.resolve(&dark), neutral.resolve(&light));
    }

    /// Text already as light as the neutral would be is not handed back as
    /// the neutral: it is toned a step towards the chart, keeping its tint.
    #[test]
    fn a_neutral_that_would_be_the_text_is_toned_back_from_it() {
        let theme = grounded(Mode::Dark, "#1e1e2e", "#cdd6f4", "#2b2c3d");
        let (hex, text) = (theme.neutral(), Oklch::of("#cdd6f4").unwrap());
        let tone = Oklch::of(&hex).unwrap();
        assert!(delta_e(&hex, "#cdd6f4") >= NEUTRAL_FROM_TEXT, "{hex}");
        assert!(tone.l < text.l && tone.l > 0.8, "{hex} is not a light tone just under the text");
        assert!(hue_gap(tone.h, text.h) < 10.0 && tone.c > 0.03, "{hex} lost the tint");
    }

    /// A theme that is grey all through, black and white, takes its tint
    /// from its accent rather than ending on a white or a black that
    /// belongs to no theme: very light on the black chart, very dark on
    /// the white one, and far from the grid either way.
    #[test]
    fn a_grey_theme_takes_its_neutral_tint_from_its_accent() {
        let black = grounded(Mode::Dark, "#000000", "#ffffff", "#191919");
        let accent = Oklch::of(&black.ui.accent).unwrap();
        let hex = black.neutral();
        let tone = Oklch::of(&hex).unwrap();
        assert!(tone.l > 0.85 && delta_e(&hex, "#ffffff") >= NEUTRAL_FROM_TEXT, "{hex}");
        assert!(delta_e(&hex, "#191919") >= 0.5, "{hex} reads as a grid line");
        assert!(hue_gap(tone.h, accent.h) < 10.0 && tone.c > 0.03, "{hex} is a plain grey");

        let white = grounded(Mode::Light, "#ffffff", "#000000", "#ececec");
        let hex = white.neutral();
        let tone = Oklch::of(&hex).unwrap();
        assert!(tone.l < 0.4 && delta_e(&hex, "#000000") >= NEUTRAL_FROM_TEXT, "{hex}");
        assert!(delta_e(&hex, "#ececec") >= 0.5, "{hex} reads as a grid line");
        assert!(hue_gap(tone.h, accent.h) < 10.0 && tone.c > 0.03, "{hex} is a plain grey");
    }

    /// The picker shows the palette in the order the names list it, the
    /// neutral last, on every built-in theme.
    #[test]
    fn the_palette_is_the_swatches_and_then_the_neutral() {
        for theme in builtin_themes() {
            let names: Vec<String> = theme.palette().into_iter().map(|s| s.name).collect();
            assert_eq!(names, PALETTE_NAMES, "{}", theme.name);
            assert_eq!(theme.palette().last().unwrap().hex, theme.neutral());
        }
    }

    fn light() -> Theme {
        builtin_themes().into_iter().find(|t| t.mode == Mode::Light).unwrap()
    }

    #[test]
    fn every_builtin_theme_carries_the_full_swatch_palette() {
        for theme in builtin_themes() {
            assert_eq!(theme.swatches.len(), SWATCH_NAMES.len(), "{}", theme.name);
            for name in SWATCH_NAMES {
                assert!(theme.swatch(name).is_some(), "{} lacks {name}", theme.name);
            }
        }
    }

    #[test]
    fn a_swatch_choice_follows_the_theme() {
        let choice = ColorChoice::swatch("Amber");
        let (dark, light) = (dark(), light());
        assert_eq!(choice.resolve(&dark), dark.swatch("Amber").unwrap().hex);
        assert_eq!(choice.resolve(&light), light.swatch("Amber").unwrap().hex);
        assert_ne!(choice.resolve(&dark), choice.resolve(&light));
    }

    #[test]
    fn a_fixed_choice_ignores_the_theme() {
        let choice = ColorChoice::Fixed { hex: "#abcdef".into() };
        assert_eq!(choice.resolve(&dark()), "#abcdef");
        assert_eq!(choice.resolve(&light()), "#abcdef");
    }

    #[test]
    fn an_unknown_swatch_falls_back_to_the_accent() {
        let choice = ColorChoice::swatch("Chartreuse");
        assert_eq!(choice.resolve(&dark()), dark().ui.accent);
    }

    /// What the rule hands out has to be told from what it was asked about,
    /// in every shipped theme and for every entry — the last one included,
    /// whose neighbour is the first and is not always far enough away.
    #[test]
    fn a_companion_reads_as_a_different_colour() {
        for theme in builtin_themes() {
            for n in 0..SWATCH_SEQUENCE.len() {
                let colour = theme.series(n);
                let companion = theme.companion(&colour);
                let apart = delta_e(&colour, &companion);
                assert!(
                    apart >= COMPANION_GAP,
                    "{}: {colour} and its companion {companion} are {apart:.3} apart",
                    theme.name
                );
            }
        }
    }

    /// A line in a colour of its own still gets a companion from the
    /// palette, and one it can be told from: a hand-picked near-Amber is not
    /// handed Amber.
    #[test]
    fn a_companion_to_a_colour_outside_the_sequence_is_still_far_from_it() {
        let theme = dark();
        let amber = theme.swatch("Amber").unwrap().hex.clone();
        let near = mix(&amber, &theme.ui.background, 0.05);
        let companion = theme.companion(&near);
        assert!(theme.swatches.iter().any(|s| s.hex == companion), "{companion} is not a swatch");
        assert!(delta_e(&near, &companion) >= COMPANION_GAP);
    }

    #[test]
    fn series_colours_wrap_without_panicking() {
        let theme = dark();
        assert_eq!(theme.series(0), theme.swatch("Blue").unwrap().hex);
        assert_eq!(theme.series(SWATCH_SEQUENCE.len()), theme.series(0));
    }

    #[test]
    fn the_sequence_is_drawn_from_the_palette() {
        for name in SWATCH_SEQUENCE {
            assert!(SWATCH_NAMES.contains(&name), "{name} is not a palette colour");
        }
        let mut seen = SWATCH_SEQUENCE.to_vec();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), SWATCH_SEQUENCE.len(), "a colour is handed out twice");
    }

    #[test]
    fn overlays_never_wear_the_candles_colours() {
        // Green and Rose are what up and down look like. An overlay in those
        // reads as price action, so they are never assigned automatically —
        // though they stay pickable by hand.
        assert!(!SWATCH_SEQUENCE.contains(&"Green"));
        assert!(!SWATCH_SEQUENCE.contains(&"Rose"));
        assert!(SWATCH_NAMES.contains(&"Green") && SWATCH_NAMES.contains(&"Rose"));
    }

    /// Two swatches that look alike are two indicators you cannot tell apart,
    /// and the palettes where this went wrong were the light ones: the gamut
    /// is narrowest between green and blue, so Teal and Cyan ended up as the
    /// same colour at slightly different saturations.
    #[test]
    fn no_two_swatches_in_a_palette_look_alike() {
        for theme in builtin_themes() {
            for (i, a) in theme.swatches.iter().enumerate() {
                for b in &theme.swatches[i + 1..] {
                    let distance = delta_e(&a.hex, &b.hex);
                    assert!(
                        distance > 0.06,
                        "{}: {} {} reads as {} {} ({distance:.3})",
                        theme.name,
                        a.name,
                        a.hex,
                        b.name,
                        b.hex
                    );
                }
            }
        }
    }

    #[test]
    fn every_swatch_is_legible_on_its_theme_background() {
        for theme in builtin_themes() {
            for swatch in &theme.swatches {
                let distance = rgb_distance(&swatch.hex, &theme.ui.background);
                assert!(
                    distance > 0.25,
                    "{} / {}: {} on {} ({distance:.3})",
                    theme.name,
                    swatch.name,
                    swatch.hex,
                    theme.ui.background
                );
            }
        }
    }

    #[test]
    fn fills_sit_between_the_line_and_the_background() {
        for theme in builtin_themes() {
            let (line, fill) = theme.series_pair(0);
            assert_ne!(fill, line, "a fill must be quieter than its line");
            assert!(
                rgb_distance(&fill, &theme.ui.background) < rgb_distance(&line, &theme.ui.background),
                "{}: the fill must be closer to the background than the line",
                theme.name
            );
            // But still visible against it.
            assert!(rgb_distance(&fill, &theme.ui.background) > 0.01, "{}", theme.name);
        }
    }

    #[test]
    fn a_muted_line_is_between_the_fill_and_the_line() {
        let theme = dark();
        let line = theme.series(0);
        let muted = theme.muted_for(&line);
        let fill = theme.fill_for(&line);
        let to_bg = |hex: &str| rgb_distance(hex, &theme.ui.background);
        assert!(to_bg(&fill) < to_bg(&muted) && to_bg(&muted) < to_bg(&line));
    }

    /// Euclidean distance in RGB, normalised. Crude next to a perceptual
    /// space, but enough to catch two colours nobody could tell apart.
    fn rgb_distance(a: &str, b: &str) -> f64 {
        let (Some(a), Some(b)) = (rgb(a), rgb(b)) else { return 0.0 };
        let d = |x: u8, y: u8| (x as f64 - y as f64) / 255.0;
        (d(a.0, b.0).powi(2) + d(a.1, b.1).powi(2) + d(a.2, b.2).powi(2)).sqrt() / 3f64.sqrt()
    }

    #[test]
    fn theme_bars_take_their_colours_from_the_theme() {
        let theme = light();
        let bars = theme_bars(&theme);
        assert_eq!(bars.up, theme.swatch("Green").unwrap().hex);
        assert_eq!(bars.down, theme.swatch("Rose").unwrap().hex);
    }

    #[test]
    fn red_up_is_the_theme_colours_the_other_way_round() {
        for theme in builtin_themes() {
            let (green, red) = (theme_bars(&theme), BarLook::red_up().scheme(&theme));
            assert_eq!((&red.up, &red.down), (&green.down, &green.up), "{}", theme.name);
            assert_eq!((&red.up_fill, &red.down_fill), (&green.down_fill, &green.up_fill));
            assert_eq!(red.neutral, green.neutral, "unchanged stays unchanged");
        }
    }

    #[test]
    fn the_monochrome_scheme_spends_every_direction_colour() {
        // The point of it: nothing about a bar's colour says which way it
        // went, so a chart cannot half-signal by leaving one of the six
        // slots behind.
        for theme in builtin_themes() {
            let mono = theme_mono_bars(&theme);
            assert_eq!(mono.up, mono.down, "{}", theme.name);
            assert_eq!(mono.up, mono.up_fill, "{}", theme.name);
            assert_eq!(mono.down, mono.down_fill, "{}", theme.name);
            assert_eq!(mono.up, mono.neutral, "{}", theme.name);
            assert_eq!(mono.volume(Direction::Up), mono.volume(Direction::Down), "{}", theme.name);
        }
    }

    #[test]
    fn a_monochrome_bar_never_wears_the_colour_it_replaced() {
        // A neutral that landed on the up colour would not read as the
        // absence of a signal; it would read as every bar having risen.
        for theme in builtin_themes() {
            let coloured = theme_bars(&theme);
            let ink = theme_mono_bars(&theme).up;
            for (what, hex) in [("up", &coloured.up), ("down", &coloured.down)] {
                let apart = delta_e(&ink, hex);
                assert!(apart >= MONO_APART, "{} {what}: {ink} vs {hex} is {apart:.3}", theme.name);
            }
        }
    }

    #[test]
    fn a_monochrome_bar_can_be_seen_on_its_own_chart() {
        for theme in builtin_themes() {
            let ink = theme_mono_bars(&theme).up;
            let ratio = contrast_ratio(&ink, &theme.ui.background);
            assert!(ratio >= MONO_CONTRAST.floor, "{}: {ratio:.2}", theme.name);
        }
    }

    #[test]
    fn volume_is_painted_in_the_bars_own_colours() {
        for theme in builtin_themes() {
            for colouring in BarColouring::ALL {
                let bars = colouring.scheme(&theme, &BarLook::default());
                for direction in [Direction::Up, Direction::Down, Direction::Flat] {
                    assert_eq!(bars.volume(direction), bars.outline(direction), "{} {colouring:?}", theme.name);
                }
            }
        }
    }

    /// A side is the theme's Blue and Amber, money the bars' up and down, so
    /// the two are never told in the same colours. Red up turns profit and
    /// loss round with its candles and nothing else; Monochrome bars leave
    /// money in the theme's green and rose.
    #[test]
    fn the_trading_colours_take_money_from_the_bars_and_a_side_from_the_theme() {
        let theme = dark();
        assert_eq!(theme.trading, None);
        let bars = theme_bars(&theme);
        let (blue, amber) = (theme.swatch("Blue").unwrap().hex.as_str(), theme.swatch("Amber").unwrap().hex.as_str());
        let filled = theme_trading(&theme, &BarLook::default());
        assert_eq!((filled.buy.as_str(), filled.sell.as_str()), (blue, amber));
        assert_eq!((filled.long.as_str(), filled.short.as_str()), (blue, amber));
        assert_eq!((filled.profit.as_str(), filled.loss.as_str()), (bars.up.as_str(), bars.down.as_str()));
        assert_eq!((filled.limit, filled.stop, filled.position), (LineStyle::Dashed, LineStyle::Dashed, LineStyle::Solid));

        let red_up = theme_trading(&theme, &BarColouring::RedUp.look(&theme, &BarLook::default()));
        assert_eq!((red_up.buy.as_str(), red_up.sell.as_str()), (filled.buy.as_str(), filled.sell.as_str()));
        assert_eq!((red_up.long.as_str(), red_up.short.as_str()), (filled.long.as_str(), filled.short.as_str()));
        assert_eq!((red_up.profit.as_str(), red_up.loss.as_str()), (filled.loss.as_str(), filled.profit.as_str()));

        let mono = theme_trading(&theme, &BarColouring::Monochrome.look(&theme, &BarLook::default()));
        assert_eq!(mono, filled, "money keeps its colours on a chart with none");

        // Colours somebody chose for the bars are money's too.
        let own = BarLook {
            up: ColorChoice::Fixed { hex: "#00aaff".into() },
            down: ColorChoice::Fixed { hex: "#ffaa00".into() },
            ..BarLook::default()
        };
        let painted = theme_trading(&theme, &own);
        assert_eq!((painted.profit.as_str(), painted.loss.as_str()), ("#00aaff", "#ffaa00"));
        assert_eq!((painted.buy.as_str(), painted.short.as_str()), (blue, amber), "a side is not the candles'");
    }

    /// In every built-in theme, light or dark, a side's colour is far
    /// enough from both of money's to be told from them at a glance.
    #[test]
    fn a_side_is_never_the_colour_of_money_in_any_theme() {
        for theme in builtin_themes() {
            let look = theme_trading(&theme, &BarLook::default());
            for side in [&look.buy, &look.sell, &look.long, &look.short] {
                for money in [&look.profit, &look.loss] {
                    assert!(delta_e(side, money) >= 0.08, "{}: {side} against {money}", theme.name);
                }
            }
            assert!(delta_e(&look.buy, &look.sell) >= 0.08, "{}: buy {} against sell {}", theme.name, look.buy, look.sell);
        }
    }

    /// Setting one slot sets that one: the rest keep following the theme
    /// and the bars, and follow them when those change later.
    #[test]
    fn editing_one_trading_slot_leaves_the_others_derived() {
        let mut theme = dark();
        let mut look = editable_trading(&theme);
        *look.colour_mut(TradingColour::Buy) = Some(ColorChoice::Fixed { hex: "#123456".into() });
        *look.line_mut(TradingLine::Stop) = Some(LineStyle::Dotted);
        theme.trading = Some(Box::new(look));
        let set = theme_trading(&theme, &BarLook::default());
        assert_eq!(set.buy, "#123456");
        assert_eq!(set.stop, LineStyle::Dotted);
        assert_eq!(set.sell, theme.swatch("Amber").unwrap().hex);
        assert_eq!(set.loss, theme_bars(&theme).down);
        assert_eq!(set.limit, LineStyle::Dashed);
        // The bars move; what was left unset moves with them.
        let moved = theme_trading(&theme, &BarLook { down: ColorChoice::Fixed { hex: "#ff00ff".into() }, ..BarLook::default() });
        assert_eq!((moved.loss.as_str(), moved.buy.as_str()), ("#ff00ff", "#123456"));
        assert_eq!(moved.sell, set.sell, "a side follows the swatches, not the candles");
        for slot in TradingColour::ALL {
            assert!(!set.colour(slot).is_empty(), "{}", slot.label());
        }
    }

    /// A theme without a trading look writes nothing for it; one with writes
    /// only what was set.
    #[test]
    fn a_theme_without_a_trading_look_reads_and_writes_as_before() {
        let theme = dark();
        let json = serde_json::to_string(&theme).unwrap();
        assert!(!json.contains("trading"), "{json}");
        let back: Theme = serde_json::from_str(&json).unwrap();
        assert_eq!(back, theme);

        let mut set = theme.clone();
        set.trading = Some(Box::new(TradingLook {
            profit: Some(ColorChoice::swatch("Blue")),
            stop: Some(LineStyle::Dotted),
            ..TradingLook::default()
        }));
        let json = serde_json::to_string(&set).unwrap();
        assert!(json.contains("\"profit\"") && json.contains("\"dotted\""), "{json}");
        assert!(!json.contains("\"buy\""), "an unset slot written: {json}");
        assert_eq!(serde_json::from_str::<Theme>(&json).unwrap(), set);
    }

    /// A colouring is written as the key a chart stores it under, and read
    /// back from it, upstream's soft and quiet palettes included.
    #[test]
    fn a_colouring_is_written_as_its_key() {
        for colouring in BarColouring::ALL {
            let json = serde_json::to_string(&colouring).unwrap();
            assert_eq!(json, format!("\"{}\"", colouring.key()));
            assert_eq!(serde_json::from_str::<BarColouring>(&json).unwrap(), colouring);
        }
        assert_eq!(BarColouring::from_key("theme-soft"), Some(BarColouring::Soft));
        assert_eq!(BarColouring::from_key("theme-quiet"), Some(BarColouring::Quiet));
        assert!(serde_json::from_str::<BarColouring>("\"nonsense\"").is_err());
    }

    #[test]
    fn hex_parsing_handles_every_length() {
        assert_eq!(rgb("#fff"), Some((255, 255, 255)));
        assert_eq!(rgb("#0d1117"), Some((13, 17, 23)));
        assert_eq!(rgb("#0d1117ff"), Some((13, 17, 23)));
        assert_eq!(rgb("nonsense"), None);
    }

    #[test]
    fn mixing_the_extremes_returns_the_endpoints() {
        assert_eq!(mix("#000000", "#ffffff", 0.0), "#000000");
        assert_eq!(mix("#000000", "#ffffff", 1.0), "#ffffff");
        assert_eq!(mix("#000000", "#ffffff", 0.5), "#808080");
    }

    #[test]
    fn direction_is_defined_once_and_agrees_with_itself() {
        assert_eq!(Direction::of_bar(10.0, 11.0), Direction::Up);
        assert_eq!(Direction::of_bar(11.0, 10.0), Direction::Down);
        assert_eq!(Direction::of_bar(10.0, 10.0), Direction::Flat);
        assert_eq!(Direction::of_change(0.01), Direction::Up);
        assert_eq!(Direction::of_change(-0.01), Direction::Down);
        assert_eq!(Direction::of_change(0.0), Direction::Flat);
    }

    #[test]
    fn a_hollow_direction_keeps_its_outline_and_loses_its_body() {
        let theme = dark();
        let look = BarLook { up_body: Body::Hollow, ..BarLook::default() };
        let (hollow, filled) = (look.scheme(&theme), theme_bars(&theme));
        assert_eq!(hollow.up, filled.up);
        assert_eq!(rgba_alpha(&hollow.up_fill), 0);
        assert_eq!(hollow.down_fill, filled.down_fill, "the other direction is untouched");
        assert_eq!(hollow.volume(Direction::Up), filled.volume(Direction::Up), "volume is never hollow");
    }

    #[test]
    fn a_border_is_the_colour_a_bar_in_foreground_is_drawn_in() {
        let look = BarLook { up_body: Body::Bordered, ..BarLook::default() };
        let theme = dark();
        let scheme = look.scheme(&theme);
        assert_eq!(scheme.up_edge, foreground(&theme));
        assert_eq!(scheme.up, theme_bars(&theme).up, "the colour stays for everything but the candle");
        assert_eq!(scheme.up_fill, scheme.up, "a border leaves the body filled");
        assert_eq!(scheme.down_edge, scheme.down, "the other direction is untouched");
    }

    #[test]
    fn red_up_swaps_the_bodies_with_the_colours() {
        let look = BarLook { up_body: Body::Bordered, ..BarLook::default() };
        let turned = BarColouring::RedUp.look(&dark(), &look);
        assert_eq!((turned.up_body, turned.down_body), (Body::Filled, Body::Bordered));
    }

    #[test]
    fn foreground_is_offered_beside_the_swatches_and_resolves_to_the_monochrome_colour() {
        let theme = dark();
        let look = BarLook { up: parse_bar_colour("foreground").unwrap(), ..BarLook::default() };
        assert_eq!(look.scheme(&theme).up, theme_mono_bars(&theme).up);
    }

    #[test]
    fn a_bar_colour_is_spelled_the_way_it_is_typed() {
        for text in ["green", "rose", "foreground", "#123abc"] {
            let choice = parse_bar_colour(text).unwrap();
            assert_eq!(spell_bar_colour(&choice), text);
        }
        assert_eq!(parse_bar_colour("ink"), parse_bar_colour("foreground"), "the old name still reads");
        assert_eq!(parse_bar_colour("Rose"), Some(ColorChoice::swatch("Rose")));
        assert_eq!(parse_bar_colour("chartreuse"), None);
        assert_eq!(parse_bar_colour("#12"), None);
    }

    fn rgba_alpha(hex: &str) -> u8 {
        let h = hex.trim_start_matches('#');
        u8::from_str_radix(h.get(6..8).unwrap_or("ff"), 16).unwrap()
    }

    #[test]
    fn the_default_scheme_is_green_up_red_down() {
        // "Typically red/green" is what people expect, and the theme palette
        // is where those two live.
        let bars = theme_bars(&dark());
        assert_eq!(bars.outline(Direction::Up), dark().swatch("Green").unwrap().hex);
        assert_eq!(bars.outline(Direction::Down), dark().swatch("Rose").unwrap().hex);
    }

    #[test]
    fn css_classes_are_stable() {
        assert_eq!(Direction::Up.css_class(), "change-up");
        assert_eq!(Direction::Down.css_class(), "change-down");
        assert_eq!(Direction::Flat.css_class(), "change-flat");
    }

    #[test]
    fn text_is_chosen_against_what_it_sits_on() {
        assert_eq!(readable_on("#ffffff"), "#000000");
        assert_eq!(readable_on("#000000"), "#ffffff");
        // Lupine's up and down: dark enough that near-black on them is
        // unreadable, which is what sent this into the engine.
        assert_eq!(readable_on("#4a2fd0"), "#ffffff");
        assert_eq!(readable_on("#c900c4"), "#ffffff");
        // And a pale green takes dark text.
        assert_eq!(readable_on("#a7c080"), "#000000");
    }

    #[test]
    fn apca_matches_the_reference_values() {
        // From the APCA 0.0.98G-4g reference implementation's own checks.
        assert!((apca_contrast("#888888", "#ffffff") - 63.06).abs() < 0.1);
        assert!((apca_contrast("#ffffff", "#888888") + 68.54).abs() < 0.1);
        assert!((apca_contrast("#000000", "#aaaaaa") - 58.15).abs() < 0.1);
        assert!((apca_contrast("#aaaaaa", "#000000") + 56.24).abs() < 0.1);
        assert!((apca_contrast("#112233", "#ddeeff") - 91.67).abs() < 0.1);
        assert!((apca_contrast("#ddeeff", "#112233") + 93.07).abs() < 0.1);
        assert_eq!(apca_contrast("#777777", "#777777"), 0.0);
    }

    #[test]
    fn a_colliding_colour_is_nudged_until_it_separates() {
        // A grid line the theme made identical to the background.
        let repaired = ensure_distinct("#0c0b0c", "#0c0b0c", 0.02, "#e0e0e0");
        assert!(distance(&repaired, "#0c0b0c") >= 0.02, "{repaired}");
        // Something already separate is left exactly as it was.
        assert_eq!(ensure_distinct("#ffffff", "#000000", 0.02, "#888888"), "#ffffff");
    }

    #[test]
    fn a_colour_inside_its_band_is_kept_to_the_byte() {
        // Everforest's grid, which was always fine.
        let band = ContrastBand::new(1.08, 1.20);
        assert_eq!(held_to("#343f44", "#2d353b", band, None), "#343f44");
    }

    #[test]
    fn a_loud_colour_walks_toward_its_ground_and_stops_just_inside() {
        // White's grid: a mid grey on white, 1.8:1, a lattice.
        let band = ContrastBand::new(1.08, 1.20);
        let grid = held_to("#c0c0c0", "#ffffff", band, None);
        let ratio = contrast_ratio(&grid, "#ffffff");
        assert!(band.holds(ratio), "{grid} is {ratio:.3}:1");
        // Just inside: it did not overshoot into the quiet half of the band.
        assert!(ratio > 1.17, "{grid} is {ratio:.3}:1");
        // And it kept its side of the ground.
        assert!(Oklch::of(&grid).unwrap().l < 1.0);
    }

    #[test]
    fn a_quiet_colour_is_rebuilt_away_from_its_ground() {
        let band = ContrastBand::new(1.08, 1.20);
        // Identical to the ground, on a dark theme: lifted.
        let lifted = held_to("#0c0b0c", "#0c0b0c", band, None);
        assert!(band.holds(contrast_ratio(&lifted, "#0c0b0c")), "{lifted}");
        assert!(Oklch::of(&lifted).unwrap().l > Oklch::of("#0c0b0c").unwrap().l);
        // On the wrong side of a light ground: a grid lighter than a
        // near-white chart has nowhere to go but down.
        let turned = held_to("#fcfcfc", "#fafafa", band, None);
        assert!(band.holds(contrast_ratio(&turned, "#fafafa")), "{turned}");
        assert!(Oklch::of(&turned).unwrap().l < Oklch::of("#fafafa").unwrap().l);
    }

    #[test]
    fn a_colour_too_vivid_for_its_ground_is_toned_down() {
        let band = ContrastBand::new(1.08, 1.20);
        let vivid = held_to("#1a3a8a", "#1a1b26", band, Some(0.06));
        assert!(Oklch::of(&vivid).unwrap().c <= 0.06 + 1e-6, "{vivid}");
        assert!(band.holds(contrast_ratio(&vivid, "#1a1b26")), "{vivid}");
    }

    #[test]
    fn a_colour_that_cannot_be_parsed_passes_through_held_to() {
        assert_eq!(held_to("nonsense", "#ffffff", ContrastBand::new(1.0, 2.0), None), "nonsense");
    }

    #[test]
    fn a_colour_survives_the_trip_through_oklch() {
        for hex in ["#7fbbb3", "#0d1117", "#f9e2af", "#ffffff", "#000000"] {
            let back = Oklch::of(hex).unwrap().hex();
            assert!(delta_e(hex, &back) < 0.005, "{hex} came back as {back}");
        }
    }

    #[test]
    fn turning_the_hue_keeps_the_lightness_and_chroma() {
        // Rotating in OKLCh is the whole reason for having it: a green turned
        // to blue must stay exactly as bright and as vivid, which HSL cannot
        // promise.
        let green = Oklch::of("#4fe88f").unwrap();
        let turned = Oklch::of(&green.with_hue(green.h + 150.0).hex()).unwrap();
        assert!((green.l - turned.l).abs() < 0.02, "{} vs {}", green.l, turned.l);
        assert!(delta_e("#4fe88f", &turned.hex()) > 0.1);
    }

    #[test]
    fn grey_has_no_hue_to_turn() {
        let grey = Oklch::of("#808080").unwrap();
        assert_eq!(grey.h, 0.0);
        assert!(delta_e("#808080", &grey.with_hue(120.0).hex()) < 0.01);
    }

    #[test]
    fn an_impossible_colour_gives_up_chroma_not_lightness() {
        // There is no vivid blue at lightness 0.95. Asking for one must yield
        // a pale blue of that lightness, not a darker vivid one.
        let asked = Oklch { l: 0.95, c: 0.2, h: 262.0 };
        let got = Oklch::of(&asked.hex()).unwrap();
        assert!((got.l - 0.95).abs() < 0.02, "{}", got.l);
        assert!(got.c < 0.2);
    }

    #[test]
    fn delta_e_runs_from_nothing_to_black_and_white() {
        assert!(delta_e("#123456", "#123456").abs() < 1e-9);
        assert!((delta_e("#000000", "#ffffff") - 1.0).abs() < 0.01);
        // Two blues a terminal theme would call different are nearly nothing
        // apart; a blue and an amber are far.
        assert!(delta_e("#7aa2f7", "#7da6ff") < 0.03);
        assert!(delta_e("#7aa2f7", "#e0af68") > 0.2);
    }

    #[test]
    fn contrast_ratio_is_wcags() {
        assert!((contrast_ratio("#000000", "#ffffff") - 21.0).abs() < 0.01);
        assert!((contrast_ratio("#ffffff", "#000000") - 21.0).abs() < 0.01);
        assert!((contrast_ratio("#777777", "#777777") - 1.0).abs() < 1e-9);
        // The textbook pair: 4.5:1 is where #767676 on white lands.
        assert!((contrast_ratio("#767676", "#ffffff") - 4.54).abs() < 0.02);
    }

    #[test]
    fn hue_gaps_go_the_short_way_round() {
        assert_eq!(hue_gap(10.0, 350.0), 20.0);
        assert_eq!(hue_gap(0.0, 180.0), 180.0);
        assert_eq!(hue_gap(90.0, 90.0), 0.0);
    }
}
