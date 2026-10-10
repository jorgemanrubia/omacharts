//! The colours a drawing can wear, built from the theme the chart is wearing.
//!
//! A drawing preset is a role the theme fills, not a hex. The app already
//! works this way for everything else that sits on a chart — candles come
//! from [`theme_bars`], indicator colours from the theme's named swatches —
//! and a drawing saved as *Amber* should be amber on Nord and amber on Paper,
//! the way an Amber indicator is. The alternative, nine fixed hexes, was
//! measured and lost: the best fixed seven over the twenty-six shipped themes
//! land within 0.08 ΔE of a candle colour thirteen times and manage 3.1:1
//! contrast at worst, where these manage 3.7:1 and never touch a candle.
//!
//! Nine roles, the same for a line and a box so preset four is amber whether
//! it is drawn as either. **Up** and **Down** are the theme's own candle
//! colours, to the byte. **Blue, Amber, Violet, Teal, Orange, Cyan** are the
//! theme's swatches in the order indicators already get them, each placed at
//! a drawing's contrast and clear of everything placed before it. **Ink** is
//! the chart's text colour with the tint taken out: a neutral that claims
//! nothing.
//!
//! The rules and every number are in `doc/themes/drawings.html`, drawn over
//! candles on every theme, by `examples/drawing_sheet.rs`.

use serde::{Deserialize, Serialize};

use crate::theme::{ContrastBand, Oklch, Theme, contrast_ratio, delta_e, held_to, mix, theme_bars};

/// One of the nine, in the order the picker shows them.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Preset {
    Up,
    Down,
    Blue,
    Amber,
    Violet,
    Teal,
    Orange,
    Cyan,
    Ink,
}

impl Preset {
    pub const ALL: [Preset; 9] = [
        Preset::Up,
        Preset::Down,
        Preset::Blue,
        Preset::Amber,
        Preset::Violet,
        Preset::Teal,
        Preset::Orange,
        Preset::Cyan,
        Preset::Ink,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Preset::Up => "Up",
            Preset::Down => "Down",
            Preset::Blue => "Blue",
            Preset::Amber => "Amber",
            Preset::Violet => "Violet",
            Preset::Teal => "Teal",
            Preset::Orange => "Orange",
            Preset::Cyan => "Cyan",
            Preset::Ink => "Ink",
        }
    }

    /// The swatch this preset starts from, for the six that start from one.
    pub fn swatch(self) -> Option<&'static str> {
        match self {
            Preset::Blue => Some("Blue"),
            Preset::Amber => Some("Amber"),
            Preset::Violet => Some("Violet"),
            Preset::Teal => Some("Teal"),
            Preset::Orange => Some("Orange"),
            Preset::Cyan => Some("Cyan"),
            Preset::Up | Preset::Down | Preset::Ink => None,
        }
    }

    /// Up and Down are the candles' colours and are never moved.
    pub fn is_direction(self) -> bool {
        matches!(self, Preset::Up | Preset::Down)
    }
}

/// How much of the colour a rectangle's fill gets, over whatever is under it.
///
/// The alpha the VWAP bands already shade with. At 0.12 the faintest tint on
/// any theme drops to the grid's floor; at 0.20 the candle bodies under it
/// start losing a third of their contrast. Nothing a drawing paints is opaque.
pub const FILL_ALPHA: f64 = 0.16;

/// How much of the colour a rectangle's border gets.
///
/// The border is the edge of the shape, not a line drawn around it. At this
/// strength it has, as drawn, the weight of an axis line — the quietest line
/// the chart draws that is still a line — and is just a visible step up from
/// its own fill on every theme. One notch lower the faintest edges fall out
/// of the axis band and the step from fill to edge stops being seen. What it
/// costs is the border as an identifier: the nine presets are told apart by
/// hue, never by their edges.
pub const BORDER_ALPHA: f64 = 0.35;

/// The border's width, in pixels. A hairline, like the chart's own.
pub const BORDER_WIDTH: f64 = 1.0;

/// A drawing is something the user put there and means to see, so it is held
/// above the indicator palette's 3.2:1. The ceiling is no ceiling: a line is
/// not a field of candles, and White's ink is black.
pub const DRAWING_CONTRAST: ContrastBand = ContrastBand::new(4.0, 21.0);

/// The indicator palette's own floor, which a swatch falls back to when the
/// drawing floor would cost more than [`REACH`]: faithful beats loud.
pub const PALETTE_CONTRAST: f64 = 3.2;

/// What a drawing's text is held to, against whatever it is actually sitting
/// on.
///
/// Higher than [`DRAWING_CONTRAST`], and for a reason: a line is a stroke
/// several pixels wide that the eye finds at 4:1, while text is a mesh of
/// hairlines at the size the chart's own labels use and falls apart there.
/// 4.5:1 is the floor that small text is readable at, and the only number in
/// this file taken from outside it. A drawing's colour is not: the same amber
/// that is a good amber line is an unreadable amber word.
///
/// There is no ceiling for the same reason the drawing band has none, and the
/// floor is a floor rather than a target: a colour already clear of it is
/// left exactly where the palette put it, so Amber text on an amber box is
/// the box's amber wherever that is readable, and only moves when it is not.
pub const TEXT_CONTRAST: ContrastBand = ContrastBand::new(4.5, 21.0);

/// Text is held higher than a stroke is. Checked here rather than in a test,
/// because it is a relation between two constants and the compiler can hold
/// it: lowering the text floor under the drawing floor stops the build.
const _: () = assert!(TEXT_CONTRAST.floor > DRAWING_CONTRAST.floor);

/// The default size of a drawing's text, in pixels.
///
/// The chart sets its axis labels at 11 and its symbol at 13, and a drawing's
/// words started there — which was reasoning from the furniture and wrong.
/// A label is not furniture: it is the one thing on the chart somebody put
/// there on purpose, usually to be seen from across a desk, and at the size
/// of an axis label it reads as another tick. Clearly above everything the
/// chart writes for itself, and still a note on a level rather than a
/// headline.
pub const TEXT_SIZE: f64 = 18.0;

/// How small and how large the font may be set, by the dialog, the keyboard
/// or the terminal. Below the floor the hairlines close up at any weight;
/// above the ceiling a drawing is a banner.
pub const MIN_TEXT_SIZE: f64 = 6.0;
pub const MAX_TEXT_SIZE: f64 = 96.0;

/// How thin and how thick a stroke may be set: a line's thickness, and a
/// box's or an ellipse's edge.
///
/// Below the floor a line is not a line on any display; above the ceiling it
/// is a band. Named here rather than written out at each of the three places
/// that enforce it — the dialog's spin, the terminal's check, and the key
/// that steps it — because three copies of a range is two chances to
/// disagree.
pub const MIN_WIDTH: f64 = 0.5;
pub const MAX_WIDTH: f64 = 12.0;

/// A step of the width keys, and of the dialog's spin: half a pixel, which
/// is the smallest change that shows on a hairline.
pub const WIDTH_STEP: f64 = 0.5;

/// The four widths a menu offers, for the hand that wants a thickness
/// rather than a number.
///
/// The first two are the ones the app already ships: a box's edge is drawn
/// at 1 and a line at 1.5, so picking from this row can always get back to
/// how a drawing started. The other two step up by about half again each
/// time, which is the smallest ratio at which two strokes beside each other
/// are plainly different. Four, because the row sits in a menu beside the
/// arrowheads and has to fit the same width — and because the number box in
/// the properties is there for anybody who wants 3.5.
pub const WIDTHS: [f64; 4] = [BORDER_WIDTH, DEFAULT_WIDTH, 2.5, 4.0];

/// A step of Ctrl+= and Ctrl+-, in pixels. One pixel is a change nobody can
/// see and ten overshoots everything; a chart's own type ladder runs 10, 11,
/// 13, so the step is the ladder's.
pub const TEXT_SIZE_STEP: f64 = 1.0;

/// Text's colour on this theme, over the ground it will actually be drawn on.
///
/// The ground matters, which is why this takes one. The same Amber word is
/// readable on the chart's background and marginal on an amber box's fill,
/// and the caller is the only one who knows which of those it is about: the
/// chart composites the fill before it sets any text. Pass what the eye will
/// see behind the glyphs — [`fill_over`] of a filled figure, the chart's
/// background otherwise.
pub fn text_colour(paint: &Paint, theme: &Theme, ground: &str) -> String {
    held_to(&paint.hex(theme), ground, TEXT_CONTRAST, None)
}

/// The ground a drawing's text is read against, given the drawing's own look.
///
/// Inside a filled box or ellipse it is the fill as composited; everywhere
/// else — a line, an unfilled figure, a word on its own — it is the chart's
/// background. The candles under it are not counted: they move, and a colour
/// that followed them would never hold still.
pub fn text_ground(kind: Kind, style: &Style, theme: &Theme) -> String {
    let bg = &theme.ui.background;
    match kind.is_bounded() && style.alpha > 0.0 {
        true => mix(&style.fill.hex(theme), bg, 1.0 - style.alpha),
        false => bg.clone(),
    }
}

/// How far in lightness a preset may travel from the swatch it came from
/// before the swatch's own lightness is the better answer. The palette
/// generator's own reach.
const REACH: f64 = 0.12;

/// Below this a colour has no hue anyone could name, which is what Ink wants.
const INK_CHROMA: f64 = 0.03;

/// How different two presets have to look, and how far one must sit from a
/// candle colour: the distance at which two lines that cross are told apart.
pub const MIN_SEPARATION: f64 = 0.08;

/// How far a preset must sit from the grid, the axis and the crosshair.
pub const MIN_FROM_FURNITURE: f64 = 0.06;

/// The crosshair is drawn dashed at this alpha, and that is what a drawing
/// has to be told apart from, not the hex it is drawn with.
pub const CROSSHAIR_ALPHA: f64 = 0.55;

/// A step of the lightness walk.
const STEP: f64 = 0.005;

/// The nine presets for a theme, in [`Preset::ALL`] order.
///
/// Resolved together because each is placed clear of the ones before it, so
/// asking for one means placing all nine; it is nine short walks and nothing
/// a caller should cache.
pub fn palette(theme: &Theme) -> Vec<(Preset, String)> {
    let bars = theme_bars(theme);
    let ui = &theme.ui;
    let mut placed: Vec<(Preset, String)> = Vec::with_capacity(Preset::ALL.len());
    for preset in Preset::ALL {
        let hex = match preset {
            Preset::Up => bars.up.clone(),
            Preset::Down => bars.down.clone(),
            Preset::Ink => {
                // De-tint first. `held_to` with a chroma cap does that and
                // keeps the lightness when the contrast is already fine,
                // which for a text colour it always is.
                let ink = held_to(&ui.text, &ui.background, DRAWING_CONTRAST, Some(INK_CHROMA));
                place(&ink, theme, &placed)
            }
            swatch => {
                let name = swatch
                    .swatch()
                    .expect("the six colour presets start from a swatch");
                let start = theme
                    .swatch(name)
                    .map(|s| s.hex.clone())
                    .unwrap_or(ui.accent.clone());
                place(&start, theme, &placed)
            }
        };
        placed.push((preset, hex));
    }
    placed
}

/// One preset's colour on a theme.
pub fn colour(theme: &Theme, preset: Preset) -> String {
    palette(theme)
        .into_iter()
        .find(|(p, _)| *p == preset)
        .map(|(_, hex)| hex)
        .expect("every preset is placed")
}

/// A rectangle's fill and border, as the chart composites them over its
/// background: for previews and tests. The chart itself paints the colour at
/// the alpha, over whatever is there.
pub fn fill_over(hex: &str, ground: &str) -> String {
    mix(hex, ground, 1.0 - FILL_ALPHA)
}

pub fn border_over(hex: &str, ground: &str) -> String {
    mix(hex, ground, 1.0 - BORDER_ALPHA)
}

/// Clear of everything placed before it, and of the furniture as drawn.
fn clear(hex: &str, theme: &Theme, placed: &[(Preset, String)]) -> bool {
    let ui = &theme.ui;
    let crosshair = mix(&ui.crosshair, &ui.background, 1.0 - CROSSHAIR_ALPHA);
    placed
        .iter()
        .all(|(_, other)| delta_e(hex, other) >= MIN_SEPARATION)
        && [&ui.grid, &ui.axis, &crosshair]
            .iter()
            .all(|f| delta_e(hex, f) >= MIN_FROM_FURNITURE)
}

/// Where a swatch lands as a drawing colour.
///
/// The smallest move in lightness, from the swatch itself, that clears the
/// drawing floor and everything already placed; the swatch's own lightness is
/// a legitimate answer and the first one tried, so most presets are the
/// swatch to the byte. When nothing within reach qualifies, the palette's own
/// floor is tried within the same reach, because a slightly quieter orange
/// is a better orange than a peach: Gruvbox's Orange stays at 3.7:1 since
/// every orange at 4:1 that is not Gruvbox's red candle is one. Only when
/// neither works does the walk go wherever it must; no shipped theme gets
/// there.
fn place(origin: &str, theme: &Theme, placed: &[(Preset, String)]) -> String {
    let bg = &theme.ui.background;
    let ok = |c: &str, floor: f64| contrast_ratio(c, bg) >= floor && clear(c, theme, placed);
    walk(origin, REACH, |c| ok(c, DRAWING_CONTRAST.floor))
        .or_else(|| walk(origin, REACH, |c| ok(c, PALETTE_CONTRAST)))
        .or_else(|| walk(origin, 1.0, |c| ok(c, DRAWING_CONTRAST.floor)))
        .unwrap_or_else(|| origin.to_string())
}

/// Nearest first, both directions at each distance, up to `reach` in
/// lightness. Hue and chroma never move.
fn walk(origin: &str, reach: f64, ok: impl Fn(&str) -> bool) -> Option<String> {
    if ok(origin) {
        return Some(origin.to_string());
    }
    let base = Oklch::of(origin)?;
    let mut step = STEP;
    while step <= reach {
        for away in [1.0, -1.0] {
            let l = base.l + away * step;
            if (0.2..=0.97).contains(&l) {
                let candidate = base.with_lightness(l).hex();
                if ok(&candidate) {
                    return Some(candidate);
                }
            }
        }
        step += STEP;
    }
    None
}

// ---------------------------------------------------------------------------
// What a drawing is
// ---------------------------------------------------------------------------

/// A point on the chart a drawing is pinned to: a moment and a price.
///
/// Time and price rather than a bar index and a pixel, because a drawing
/// has to survive everything the chart does around it. Scroll, and it stays
/// on the bars it was drawn on; zoom, and it stretches with them; switch
/// the resolution, and a line through two daily closes still passes through
/// the same two moments on the hourly chart. The moment is the bar's own
/// timestamp, so a drawing dropped on a candle lands exactly on it.
#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
pub struct Anchor {
    pub ts: i64,
    pub price: f64,
}

impl Anchor {
    pub fn new(ts: i64, price: f64) -> Anchor {
        Anchor { ts, price }
    }
}

/// The kinds of drawing. All of them are two anchors; what differs is what
/// is drawn between them — and, for text, that the second anchor is the
/// first, because a word sits at a point rather than spanning two.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    /// A straight line from one anchor to the other.
    Line,
    /// A line held level: a price, drawn across the span its two anchors
    /// mark out. Both ends are at the same price and no gesture can put them
    /// anywhere else, which is the whole of what it is for — a level read
    /// off a chart is a number, and a support line that came out a tenth of
    /// a percent out of true is a lie you cannot see.
    Horizontal,
    /// A run of joined segments, with as many corners as the hand puts in.
    /// The only kind that is not two anchors: its points are its own, and
    /// `from` and `to` are kept as the first and the last so that
    /// everything which only wants the span of a drawing goes on working.
    Zigzag,
    /// The same line, pointing: an arrowhead where it ends. Its own kind
    /// rather than a line that happens to be configured with one, so that
    /// the tool is a tool and its nine configurations are about arrows —
    /// reaching for an arrow and then setting a line's arrow property is
    /// two steps to say one thing.
    Arrow,
    /// A box with the two anchors at opposite corners.
    Rect,
    /// An ellipse inscribed in the box the two anchors make. Dragged to
    /// whatever shape the hand wants, round only when the hand makes it
    /// round: the same freedom a box has, which is what the drawing tools
    /// in a slide editor give and what anybody reaching for "a circle on
    /// this chart" actually wants.
    Ellipse,
    /// Words on the chart, at the first anchor. The only kind whose size
    /// comes from what it says rather than from where its anchors are.
    Text,
}

impl Kind {
    /// Every kind, in the order the tools are offered: the plain line
    /// first, then the two that are a line with one thing added, then the
    /// shapes, then the words.
    pub const ALL: [Kind; 7] = [
        Kind::Line,
        Kind::Horizontal,
        Kind::Arrow,
        Kind::Zigzag,
        Kind::Rect,
        Kind::Ellipse,
        Kind::Text,
    ];

    /// The kinds that are a shape, which is every kind that can carry text
    /// of its own.
    pub const FIGURES: [Kind; 6] =
        [Kind::Line, Kind::Horizontal, Kind::Arrow, Kind::Zigzag, Kind::Rect, Kind::Ellipse];

    /// Whether this kind is a stroke rather than a shape with an inside.
    pub fn is_line(self) -> bool {
        matches!(self, Kind::Line | Kind::Horizontal | Kind::Arrow | Kind::Zigzag)
    }

    /// Whether it is laid down a point at a time rather than in one gesture.
    pub fn is_path(self) -> bool {
        self == Kind::Zigzag
    }

    pub fn label(self) -> &'static str {
        match self {
            Kind::Line => "Line",
            Kind::Horizontal => "Horizontal line",
            Kind::Arrow => "Arrow",
            Kind::Zigzag => "Zig-zag",
            Kind::Rect => "Rectangle",
            Kind::Ellipse => "Circle",
            Kind::Text => "Text",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Kind::Line => "line",
            Kind::Horizontal => "hline",
            Kind::Arrow => "arrow",
            Kind::Zigzag => "zigzag",
            Kind::Rect => "rect",
            Kind::Ellipse => "ellipse",
            Kind::Text => "text",
        }
    }

    pub fn from_key(key: &str) -> Option<Kind> {
        Kind::ALL
            .into_iter()
            .find(|k| k.key().eq_ignore_ascii_case(key))
            // "circle" is what the tool is called, so it is what somebody
            // types; the shape it draws is an ellipse. "horizontal" is what
            // the tool is called, and "hline" what it is quickest to type.
            .or_else(|| key.eq_ignore_ascii_case("circle").then_some(Kind::Ellipse))
            .or_else(|| {
                ["horizontal", "horizontal-line", "level"]
                    .iter()
                    .any(|name| key.eq_ignore_ascii_case(name))
                    .then_some(Kind::Horizontal)
            })
    }

    /// Whether this kind has an inside: a fill, and room for text in it.
    pub fn is_bounded(self) -> bool {
        matches!(self, Kind::Rect | Kind::Ellipse)
    }

    /// Whether the drawing is the text itself, rather than a shape that may
    /// carry some.
    pub fn is_text(self) -> bool {
        self == Kind::Text
    }
}

/// What a drawing is painted with: one of the nine roles the theme fills, or
/// a colour of the user's own that the theme never touches.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Paint {
    Preset { preset: Preset },
    Fixed { hex: String },
}

impl Paint {
    pub fn preset(preset: Preset) -> Paint {
        Paint::Preset { preset }
    }

    pub fn fixed(hex: &str) -> Paint {
        Paint::Fixed { hex: hex.to_string() }
    }

    /// The colour on this theme.
    pub fn hex(&self, theme: &Theme) -> String {
        match self {
            Paint::Preset { preset } => colour(theme, *preset),
            Paint::Fixed { hex } => hex.clone(),
        }
    }

    /// How it is written on a command line and said back: a preset's name
    /// in lower case, or the hex.
    pub fn spell(&self) -> String {
        match self {
            Paint::Preset { preset } => preset.name().to_lowercase(),
            Paint::Fixed { hex } => hex.clone(),
        }
    }

    /// The reverse: a preset's name, or `#rrggbb`.
    pub fn parse(text: &str) -> Option<Paint> {
        if let Some(hex) = text.strip_prefix('#') {
            let valid = hex.len() == 6 && hex.chars().all(|c| c.is_ascii_hexdigit());
            return valid.then(|| Paint::fixed(&format!("#{}", hex.to_lowercase())));
        }
        Preset::ALL
            .into_iter()
            .find(|p| p.name().eq_ignore_ascii_case(text))
            .map(Paint::preset)
    }
}

/// Where a drawing's text sits against the drawing.
///
/// A nine-cell grid, which is both what the dialog shows as a picture and
/// everything the five named places can mean: *centre* is centred on both
/// axes, *top* is centred across and against the top, and the corners are
/// the pairs. Inside the figure in every case, inset by a hair, the way text
/// in a shape works in a slide editor — a label that floated outside the box
/// it belongs to would have to be dragged back to it.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Place {
    TopLeft,
    Top,
    TopRight,
    Left,
    #[default]
    Center,
    Right,
    BottomLeft,
    Bottom,
    BottomRight,
}

impl Place {
    /// Reading order, which is the order the grid is drawn in.
    pub const ALL: [Place; 9] = [
        Place::TopLeft,
        Place::Top,
        Place::TopRight,
        Place::Left,
        Place::Center,
        Place::Right,
        Place::BottomLeft,
        Place::Bottom,
        Place::BottomRight,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Place::TopLeft => "Top left",
            Place::Top => "Top",
            Place::TopRight => "Top right",
            Place::Left => "Left",
            Place::Center => "Centre",
            Place::Right => "Right",
            Place::BottomLeft => "Bottom left",
            Place::Bottom => "Bottom",
            Place::BottomRight => "Bottom right",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Place::TopLeft => "top-left",
            Place::Top => "top",
            Place::TopRight => "top-right",
            Place::Left => "left",
            Place::Center => "center",
            Place::Right => "right",
            Place::BottomLeft => "bottom-left",
            Place::Bottom => "bottom",
            Place::BottomRight => "bottom-right",
        }
    }

    pub fn from_key(key: &str) -> Option<Place> {
        let key = key.replace('_', "-");
        Place::ALL
            .into_iter()
            .find(|p| p.key().eq_ignore_ascii_case(&key))
            .or_else(|| key.eq_ignore_ascii_case("centre").then_some(Place::Center))
    }

    /// How far along each axis, 0 to 1: left to right, and top to bottom.
    pub fn fractions(self) -> (f64, f64) {
        let across = match self {
            Place::TopLeft | Place::Left | Place::BottomLeft => 0.0,
            Place::Top | Place::Center | Place::Bottom => 0.5,
            Place::TopRight | Place::Right | Place::BottomRight => 1.0,
        };
        let down = match self {
            Place::TopLeft | Place::Top | Place::TopRight => 0.0,
            Place::Left | Place::Center | Place::Right => 0.5,
            Place::BottomLeft | Place::Bottom | Place::BottomRight => 1.0,
        };
        (across, down)
    }

    /// How the lines of a multi-line label line up with each other, which
    /// follows where the block sits: a block against the left edge reads
    /// ragged-right, one against the right edge ragged-left, a centred one
    /// centred.
    pub fn alignment(self) -> Align {
        let across = self.fractions().0;
        if across == 0.0 {
            Align::Start
        } else if across == 1.0 {
            Align::End
        } else {
            Align::Center
        }
    }
}

/// Which edge the lines of a label are flush with.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Align {
    Start,
    Center,
    End,
}

/// A run of characters that share a weight and a slope.
///
/// Rich text, kept as the spans it is rather than as markup: a string with
/// tags in it would have to be escaped, parsed and validated at every
/// boundary — the store, the terminal, the editor — and one malformed tag
/// would take a drawing with it. Spans cannot be malformed. Bold and italic
/// are all there is for now, which is what a note on a chart needs; a third
/// attribute is a field here and a key in the editor and nothing else.
#[derive(Clone, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub struct Span {
    pub text: String,
    #[serde(default, skip_serializing_if = "is_false")]
    pub bold: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub italic: bool,
}

fn is_false(flag: &bool) -> bool {
    !*flag
}

impl Span {
    pub fn plain(text: &str) -> Span {
        Span { text: text.to_string(), bold: false, italic: false }
    }

    /// Whether two spans differ in anything but their characters, and so
    /// cannot be run together.
    pub fn same_style(&self, other: &Span) -> bool {
        self.bold == other.bold && self.italic == other.italic
    }
}

/// What a drawing says, and where.
///
/// Every kind can have one: the text kind *is* its text, and a line, a box
/// or an ellipse carries one as a label. Empty is the normal state for a
/// figure and means it has nothing to say, which is not the same as having
/// an empty line of text — a figure with no text draws none, and takes no
/// room for it.
#[derive(Clone, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub struct Text {
    /// The content, as the runs it is made of. Newlines live in the runs.
    #[serde(default)]
    pub spans: Vec<Span>,
    /// Where it sits against the drawing.
    #[serde(default)]
    pub at: Place,
}

impl Text {
    pub fn plain(text: &str) -> Text {
        Text { spans: vec![Span::plain(text)], at: Place::default() }.tidied()
    }

    /// Whether there is anything to draw.
    pub fn is_empty(&self) -> bool {
        self.spans.iter().all(|s| s.text.is_empty())
    }

    /// The characters, with the styling dropped: what the terminal prints
    /// and what a plain editor shows.
    pub fn plain_text(&self) -> String {
        self.spans.iter().map(|s| s.text.as_str()).collect()
    }

    /// The same text with empty runs dropped and neighbours that share a
    /// style run together, so the spans never grow from editing alone.
    pub fn tidied(mut self) -> Text {
        self.spans.retain(|s| !s.text.is_empty());
        let mut runs: Vec<Span> = Vec::with_capacity(self.spans.len());
        for span in self.spans {
            match runs.last_mut() {
                Some(last) if last.same_style(&span) => last.text.push_str(&span.text),
                _ => runs.push(span),
            }
        }
        self.spans = runs;
        self
    }

    /// How many lines it is, which is what a figure needs to leave room for.
    pub fn lines(&self) -> usize {
        self.plain_text().lines().count().max(1)
    }
}

/// The shape of an arrowhead: a filled triangle, an open chevron, or a
/// swept barb.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ArrowHead {
    #[default]
    Filled,
    Open,
    Barb,
}

impl ArrowHead {
    pub const ALL: [ArrowHead; 3] = [ArrowHead::Filled, ArrowHead::Open, ArrowHead::Barb];

    pub fn label(self) -> &'static str {
        match self {
            ArrowHead::Filled => "Filled",
            ArrowHead::Open => "Open",
            ArrowHead::Barb => "Barb",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            ArrowHead::Filled => "filled",
            ArrowHead::Open => "open",
            ArrowHead::Barb => "barb",
        }
    }

    pub fn from_key(key: &str) -> Option<ArrowHead> {
        ArrowHead::ALL.into_iter().find(|h| h.key().eq_ignore_ascii_case(key))
    }
}

/// Which end of a line wears an arrowhead.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Arrow {
    #[default]
    None,
    End,
    Start,
    Both,
}

impl Arrow {
    pub const ALL: [Arrow; 4] = [Arrow::None, Arrow::End, Arrow::Start, Arrow::Both];

    pub fn label(self) -> &'static str {
        match self {
            Arrow::None => "None",
            Arrow::End => "At the end",
            Arrow::Start => "At the start",
            Arrow::Both => "Both ends",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Arrow::None => "none",
            Arrow::End => "end",
            Arrow::Start => "start",
            Arrow::Both => "both",
        }
    }

    pub fn from_key(key: &str) -> Option<Arrow> {
        Arrow::ALL.into_iter().find(|a| a.key().eq_ignore_ascii_case(key))
    }

    pub fn at_end(self) -> bool {
        matches!(self, Arrow::End | Arrow::Both)
    }

    pub fn at_start(self) -> bool {
        matches!(self, Arrow::Start | Arrow::Both)
    }
}

/// Everything about how a drawing looks.
///
/// One struct for both kinds, so a configuration and a drawing's own
/// properties are the same thing and a dialog edits one of them with one
/// set of rows. A line reads `colour`, `width` and `arrow`; a rectangle
/// reads `border`, `width` and `colour` for its edge, and `fill` and `alpha`
/// for what is inside it.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Style {
    /// A line's colour; a rectangle's edge.
    pub colour: Paint,
    /// A line's thickness; a rectangle's edge, when it has one.
    pub width: f64,
    #[serde(default)]
    pub arrow: Arrow,
    /// The shape of the arrowhead, where there is one.
    #[serde(default)]
    pub head: ArrowHead,
    /// Whether a rectangle has an edge at all.
    #[serde(default = "Style::default_border")]
    pub border: bool,
    /// What a rectangle is filled with.
    #[serde(default = "Style::default_fill")]
    pub fill: Paint,
    /// How much of the fill shows: 0 is nothing, 1 is solid. The
    /// transparency level, the other way up.
    #[serde(default = "Style::default_alpha")]
    pub alpha: f64,
    /// How the drawing's words are set, whether it is the text itself or a
    /// figure carrying a label. Part of the configuration, so a box saved as
    /// *Amber* labels itself in the amber that reads on amber; what the words
    /// actually say never is.
    #[serde(default)]
    pub text: TextStyle,
}

/// How a drawing's text is set: its colour, its face and its size.
///
/// Three properties and no more, because they are the three that change how
/// a chart reads. The weight and the slope are not here: those belong to a
/// run of characters inside the text, not to every drawing that follows a
/// configuration, and live on [`Span`].
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct TextStyle {
    /// The ink. Held to [`TEXT_CONTRAST`] against whatever it is drawn on,
    /// so a preset that is a fine line colour is still readable as a word.
    pub colour: Paint,
    /// The face, by family name. `None` is the system's own font, which is
    /// what the rest of the window is set in and the right default: a chart
    /// annotated in the desktop's font looks like part of the desktop.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub family: Option<String>,
    /// The size in pixels, to match the chart's own type rather than the
    /// printer's points.
    #[serde(default = "TextStyle::default_size")]
    pub size: f64,
}

impl Default for TextStyle {
    fn default() -> TextStyle {
        TextStyle::of(Preset::Ink)
    }
}

impl TextStyle {
    /// The text of a drawing in a preset: the preset's own colour, at the
    /// chart's reading size, in the system's font.
    ///
    /// The preset's own colour rather than a neutral, because a label belongs
    /// to the figure it is on and saying so in its colour is what makes a
    /// chart of four boxes readable. It is only *held* to the text floor at
    /// paint time, against the ground it lands on, so where the preset is
    /// already readable the word is the figure's colour exactly.
    pub fn of(preset: Preset) -> TextStyle {
        TextStyle { colour: Paint::preset(preset), family: None, size: TEXT_SIZE }
    }

    fn default_size() -> f64 {
        TEXT_SIZE
    }

    /// The family as a font system wants it: the name, or nothing at all for
    /// the system's own.
    pub fn family_name(&self) -> Option<&str> {
        self.family.as_deref().filter(|name| !name.trim().is_empty())
    }

    /// The size, kept inside what can be drawn.
    pub fn clamped_size(&self) -> f64 {
        self.size.clamp(MIN_TEXT_SIZE, MAX_TEXT_SIZE)
    }
}

impl Style {
    /// The configuration a line starts from: a preset's colour at the
    /// chart's default stroke, no arrow.
    pub fn line(preset: Preset) -> Style {
        Style {
            colour: Paint::preset(preset),
            width: DEFAULT_WIDTH,
            arrow: Arrow::None,
            head: ArrowHead::Filled,
            border: true,
            fill: Paint::preset(preset),
            alpha: FILL_ALPHA,
            text: TextStyle::of(preset),
        }
    }

    /// The configuration a rectangle starts from: a preset's colour as a
    /// translucent fill under a hairline edge of the same colour, the pair
    /// measured over every theme.
    pub fn rect(preset: Preset) -> Style {
        Style {
            colour: Paint::preset(preset),
            width: BORDER_WIDTH,
            arrow: Arrow::None,
            head: ArrowHead::Filled,
            border: true,
            fill: Paint::preset(preset),
            alpha: FILL_ALPHA,
            text: TextStyle::of(preset),
        }
    }

    /// An arrow is a line that points: the same stroke, with an open head
    /// where it ends. Open rather than filled, because a filled triangle at
    /// a line's own weight reads as a blob on a chart, and because the
    /// filled one is what a line configured by hand already gets.
    pub fn arrow(preset: Preset) -> Style {
        Style { arrow: Arrow::End, head: ArrowHead::Open, ..Style::line(preset) }
    }

    /// A level is a line, and looks like one.
    pub fn horizontal(preset: Preset) -> Style {
        Style::line(preset)
    }

    /// A zig-zag is a line that turns corners, and it points: a run of
    /// swings is read in the direction it was drawn, so the head says which
    /// way that was. Every arrow setting is still a setting — it is the
    /// same `arrow` and `head` every stroke has — this is only where it
    /// starts.
    pub fn zigzag(preset: Preset) -> Style {
        Style::arrow(preset)
    }

    /// An ellipse starts from exactly what a box does. The two are the same
    /// drawing with a different outline, they share the nine presets, and a
    /// preset that was measured over every theme as a fill under a hairline
    /// is measured for both.
    pub fn ellipse(preset: Preset) -> Style {
        Style::rect(preset)
    }

    /// Text starts from the same preset, and nothing a shape has.
    ///
    /// No border, no fill: a word on a chart is the word. The rest of the
    /// fields are still here and still the preset's, so a text drawing
    /// switched to a box later is a box in the same colour rather than a
    /// box in nothing.
    pub fn text(preset: Preset) -> Style {
        Style { border: false, alpha: 0.0, ..Style::rect(preset) }
    }

    /// The configuration this kind starts from.
    pub fn of(kind: Kind, preset: Preset) -> Style {
        match kind {
            Kind::Line => Style::line(preset),
            Kind::Horizontal => Style::horizontal(preset),
            Kind::Arrow => Style::arrow(preset),
            Kind::Zigzag => Style::zigzag(preset),
            Kind::Rect => Style::rect(preset),
            Kind::Ellipse => Style::ellipse(preset),
            Kind::Text => Style::text(preset),
        }
    }

    fn default_border() -> bool {
        true
    }

    fn default_fill() -> Paint {
        Paint::preset(Preset::Blue)
    }

    fn default_alpha() -> f64 {
        FILL_ALPHA
    }
}

/// The nine configurations of each kind.
///
/// Editable, stored as one setting, and every drawing that follows
/// configuration N looks like N as it is now — change N and they all change,
/// which is the point of a configuration over a copy. The defaults are the
/// nine presets in order, so configuration 1 is Up and 2 is Down, as the
/// brief asks.
///
/// One list per kind, and every list is nine long. A kind added later reads
/// back as absent from anything already written down, which [`list`] treats
/// as "the defaults" — so a store written before the ellipse and the text
/// existed opens with their nine as shipped rather than with none.
///
/// [`list`]: Configurations::list
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Configurations {
    pub line: Vec<Style>,
    pub rect: Vec<Style>,
    #[serde(default)]
    pub ellipse: Vec<Style>,
    #[serde(default)]
    pub text: Vec<Style>,
    #[serde(default)]
    pub arrow: Vec<Style>,
    #[serde(default)]
    pub horizontal: Vec<Style>,
    #[serde(default)]
    pub zigzag: Vec<Style>,
}

/// How many configurations each kind has.
pub const CONFIGURATIONS: u8 = 9;

impl Default for Configurations {
    fn default() -> Configurations {
        let nine = |make: fn(Preset) -> Style| Preset::ALL.into_iter().map(make).collect();
        Configurations {
            line: nine(Style::line),
            horizontal: nine(Style::horizontal),
            zigzag: nine(Style::zigzag),
            arrow: nine(Style::arrow),
            rect: nine(Style::rect),
            ellipse: nine(Style::ellipse),
            text: nine(Style::text),
        }
    }
}

impl Configurations {
    /// Configuration `n`, 1 to 9, of a kind. Out of range falls back to the
    /// first, because a number nobody can reach from the keyboard is not a
    /// configuration.
    pub fn of(&self, kind: Kind, n: u8) -> &Style {
        let list = self.list(kind);
        list.get(n.clamp(1, CONFIGURATIONS) as usize - 1).unwrap_or(&list[0])
    }

    pub fn set(&mut self, kind: Kind, n: u8, style: Style) {
        let at = n.clamp(1, CONFIGURATIONS) as usize - 1;
        // A list that was never written — a kind this store predates — is
        // filled in from the defaults first, or the write would land in an
        // empty vector and vanish.
        if self.stored(kind).len() < CONFIGURATIONS as usize {
            let fresh = Configurations::default().stored_owned(kind);
            *self.stored_mut(kind) = fresh;
        }
        if let Some(slot) = self.stored_mut(kind).get_mut(at) {
            *slot = style;
        }
    }

    /// Put one kind's nine back to what shipped.
    pub fn reset(&mut self, kind: Kind) {
        let fresh = Configurations::default().stored_owned(kind);
        *self.stored_mut(kind) = fresh;
    }

    pub fn list(&self, kind: Kind) -> &[Style] {
        let list = self.stored(kind);
        // A stored list that is short — written by a build with fewer, or
        // with none because the kind did not exist — is read as if it were
        // the defaults.
        if list.len() >= CONFIGURATIONS as usize {
            list
        } else {
            DEFAULTS.get_or_init(Configurations::default).stored(kind)
        }
    }

    /// Whether configuration `n` of a kind is as shipped.
    pub fn is_default(&self, kind: Kind, n: u8) -> bool {
        DEFAULTS.get_or_init(Configurations::default).of(kind, n) == self.of(kind, n)
    }

    /// The list exactly as stored, short or empty included.
    fn stored(&self, kind: Kind) -> &[Style] {
        match kind {
            Kind::Line => &self.line,
            Kind::Horizontal => &self.horizontal,
            Kind::Zigzag => &self.zigzag,
            Kind::Arrow => &self.arrow,
            Kind::Rect => &self.rect,
            Kind::Ellipse => &self.ellipse,
            Kind::Text => &self.text,
        }
    }

    fn stored_mut(&mut self, kind: Kind) -> &mut Vec<Style> {
        match kind {
            Kind::Line => &mut self.line,
            Kind::Horizontal => &mut self.horizontal,
            Kind::Zigzag => &mut self.zigzag,
            Kind::Arrow => &mut self.arrow,
            Kind::Rect => &mut self.rect,
            Kind::Ellipse => &mut self.ellipse,
            Kind::Text => &mut self.text,
        }
    }

    fn stored_owned(&self, kind: Kind) -> Vec<Style> {
        self.stored(kind).to_vec()
    }
}

static DEFAULTS: std::sync::OnceLock<Configurations> = std::sync::OnceLock::new();

/// Whether a drawing belongs to the symbol or to the chart it was drawn on.
///
/// Two answers, and there were four: a drawing used to be able to belong to
/// one of nine numbered groups as well, with each chart choosing a group to
/// draw into and to read from. That bought a kind of sharing nobody asked
/// twice for and cost a question — *which group?* — on every drawing, every
/// chart and every command. What is left is the distinction that was doing
/// the work: a drawing is the symbol's, or it is this chart's.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Scope {
    /// Drawn on a chart that does not send, and stays with it.
    Local,
    /// The symbol's, and on every chart of it that shows what others send.
    #[default]
    Shared,
}

impl Scope {
    pub fn key(self) -> &'static str {
        match self {
            Scope::Local => "local",
            Scope::Shared => "shared",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Scope::Local => "This chart only",
            Scope::Shared => "Every chart of the symbol",
        }
    }

    pub fn from_key(key: &str) -> Option<Scope> {
        match key.to_lowercase().as_str() {
            "local" => Some(Scope::Local),
            // "global" is what shared drawings were written down as, and a
            // "group-N" is one that belonged to a group that no longer
            // exists. Both were drawings somebody meant to share, so both
            // come back shared rather than quietly becoming private to
            // whichever chart happens to open first.
            "shared" | "global" => Some(Scope::Shared),
            other => other.starts_with("group-").then_some(Scope::Shared),
        }
    }

    pub fn all() -> [Scope; 2] {
        [Scope::Shared, Scope::Local]
    }
}

impl Serialize for Scope {
    fn serialize<S: serde::Serializer>(&self, out: S) -> Result<S::Ok, S::Error> {
        out.serialize_str(self.key())
    }
}

impl<'de> Deserialize<'de> for Scope {
    fn deserialize<D: serde::Deserializer<'de>>(input: D) -> Result<Scope, D::Error> {
        let key = String::deserialize(input)?;
        Ok(Scope::from_key(&key).unwrap_or_default())
    }
}

/// Something a person drew on a symbol's chart.
///
/// Its look is either a configuration it follows — change the configuration
/// and the drawing changes — or, once a property has been set by hand, its
/// own. Its scope says who else sees it.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Drawing {
    /// The row id in the store for a shared drawing; a negative number for
    /// a drawing local to a chart, which lives with the chart; zero until it
    /// has been written down anywhere.
    #[serde(default)]
    pub id: i64,
    pub kind: Kind,
    pub from: Anchor,
    pub to: Anchor,
    /// The configuration, 1 to 9, this follows. `None` once a property was
    /// set by hand, at which point `style` is the look.
    #[serde(default = "Drawing::default_config")]
    pub config: Option<u8>,
    /// The look set by hand. Ignored while `config` is some.
    #[serde(default)]
    pub style: Option<Style>,
    /// Which chart drew it, for a drawing that is the symbol's.
    ///
    /// A chart that stops sending has to take back the drawings it sent,
    /// and "the ones it sent" is a question nothing could answer: a shared
    /// drawing sits with the symbol, and the symbol does not remember who
    /// put it there. So it is written down. Nothing reads it to draw
    /// anything, and a drawing written before charts had names has none —
    /// which means no chart claims it, and no chart can withdraw it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<String>,
    /// The corners of a zig-zag, first to last, and empty for every other
    /// kind.
    ///
    /// `from` and `to` are kept as the first and the last of these, so a
    /// zig-zag still answers every question that is about the span of a
    /// drawing — where it starts, where it ends, how to shift it — without
    /// each of those having to know this field exists.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub points: Vec<Anchor>,
    /// The configuration this was following when a property was first set
    /// by hand, for a drawing that has its own look now.
    ///
    /// Kept because losing it loses the one thing that makes "Custom"
    /// legible: a drawing whose look is its own came from somewhere, and
    /// *where* is what somebody needs to know both to put it back and to
    /// decide which of the nine to write it over. Nothing reads it to draw
    /// anything — it is a note about where this look started.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_from: Option<u8>,
    #[serde(default)]
    pub scope: Scope,
    /// Where it sits among the others: higher is nearer the front. All of
    /// them sit over the candles; this only orders drawings among
    /// themselves, for "bring to front" and "send to back".
    #[serde(default)]
    pub order: i64,
    /// What it says. The whole of a text drawing; a label on any other kind,
    /// and empty on most of those. Never part of a configuration — a
    /// configuration says how words are set, never which words — so it sits
    /// here beside the anchors rather than in [`Style`].
    #[serde(default, skip_serializing_if = "Text::is_empty")]
    pub text: Text,
}

/// Put drawings in the order they are painted: back to front, and the older
/// first among equals.
pub fn sort_for_painting(drawings: &mut [Drawing]) {
    drawings.sort_by(|a, b| a.order.cmp(&b.order).then(a.id.abs().cmp(&b.id.abs())));
}

/// The width a line is drawn at when nobody chose one: the chart's own
/// default stroke.
pub const DEFAULT_WIDTH: f64 = 1.5;

impl Drawing {
    pub fn new(kind: Kind, from: Anchor, to: Anchor) -> Drawing {
        Drawing {
            id: 0,
            kind,
            from,
            to,
            config: Some(1),
            style: None,
            points: Vec::new(),
            origin: None,
            started_from: None,
            scope: Scope::Shared,
            order: 0,
            text: Text::default(),
        }
    }

    /// A zig-zag beginning at one point, with a second following the hand.
    pub fn zigzag_from(at: Anchor) -> Drawing {
        let mut drawing = Drawing::new(Kind::Zigzag, at, at);
        drawing.points = vec![at, at];
        drawing
    }

    /// The corners it is drawn through: its own for a zig-zag, its two
    /// anchors for everything else.
    pub fn corners(&self) -> Vec<Anchor> {
        match self.kind.is_path() && self.points.len() >= 2 {
            true => self.points.clone(),
            false => vec![self.from, self.to],
        }
    }

    /// Put another corner on the end, for the next click of a zig-zag.
    pub fn add_corner(&mut self, at: Anchor) {
        if !self.kind.is_path() {
            return;
        }
        self.points.push(at);
        self.settle();
    }

    /// Move the corner the hand is on — the last, while one is being laid
    /// down.
    pub fn move_last_corner(&mut self, at: Anchor) {
        if let Some(last) = self.points.last_mut() {
            *last = at;
        }
        self.settle();
    }

    /// Drop the corner that was only following the hand, at the end of
    /// laying one down. Says whether what is left is still a drawing.
    pub fn finish_path(&mut self) -> bool {
        if !self.kind.is_path() {
            return true;
        }
        self.points.pop();
        self.settle();
        self.points.len() >= 2
    }

    /// Keep `from` and `to` on the ends of the path.
    fn settle(&mut self) {
        if let (Some(first), Some(last)) = (self.points.first(), self.points.last()) {
            self.from = *first;
            self.to = *last;
        }
    }

    /// A text drawing at one point, saying nothing yet: what a click with the
    /// text tool makes, before anything is typed into it.
    pub fn text_at(at: Anchor) -> Drawing {
        Drawing::new(Kind::Text, at, at)
    }

    /// Whether there is anything to draw as words.
    pub fn has_text(&self) -> bool {
        !self.text.is_empty()
    }

    /// Set what it says, tidying the runs. The text is not a style, so this
    /// does not take the drawing off its configuration the way
    /// [`edit_style`] does: a labelled amber box still follows amber.
    ///
    /// [`edit_style`]: Drawing::edit_style
    pub fn set_text(&mut self, text: Text) {
        self.text = text.tidied();
    }

    fn default_config() -> Option<u8> {
        Some(1)
    }

    /// How it looks: the configuration it follows, or its own look.
    pub fn style<'a>(&'a self, configs: &'a Configurations) -> &'a Style {
        match (self.config, &self.style) {
            (None, Some(own)) => own,
            (Some(n), _) => configs.of(self.kind, n),
            (None, None) => configs.of(self.kind, 1),
        }
    }

    /// Change a property by hand: the drawing stops following its
    /// configuration and keeps its own look from here on.
    pub fn edit_style(&mut self, configs: &Configurations, edit: impl FnOnce(&mut Style)) {
        let mut own = self.style(configs).clone();
        edit(&mut own);
        // Only on the way out of following one. A drawing already wearing
        // its own look keeps the number it originally left, however many
        // times it is edited after that.
        if let Some(following) = self.config {
            self.started_from = Some(following);
        }
        self.config = None;
        self.style = Some(own);
    }

    /// Follow configuration `n` again.
    pub fn follow(&mut self, n: u8) {
        self.config = Some(n.clamp(1, CONFIGURATIONS));
        self.style = None;
        // Following again, so there is no look of its own to have come from.
        self.started_from = None;
    }

    /// Whether this drawing lives with the chart it was drawn on rather
    /// than with the symbol.
    pub fn is_local(&self) -> bool {
        self.scope == Scope::Local
    }

    /// The anchor a grip stands for, to move it, for the grips that are an
    /// anchor. A rectangle's other two corners are half of each.
    pub fn anchor_mut(&mut self, grip: Grip) -> Option<&mut Anchor> {
        match grip {
            Grip::From => Some(&mut self.from),
            Grip::To => Some(&mut self.to),
            // Half of one anchor each, so there is no whole anchor to hand
            // back; `move_grip` is the way to move them.
            Grip::Top | Grip::Bottom | Grip::Left | Grip::Right => None,
            Grip::FromTo | Grip::ToFrom | Grip::Body => None,
            Grip::Corner(n) => self.points.get_mut(n),
        }
    }

    /// Put the grip where the hand is.
    pub fn move_grip(&mut self, grip: Grip, at: Anchor) {
        // A text drawing is one point wearing two anchors, so whichever is
        // moved both go: nothing reads the second, and letting them drift
        // apart would leave a drawing whose box depends on which grip was
        // last dragged.
        if self.kind.is_text() {
            self.from = at;
            self.to = at;
            return;
        }
        // A level stays level. Whichever end is dragged carries the price
        // for both, so pulling an end up moves the line and pulling it
        // sideways changes how far it reaches — and nothing anybody can do
        // with a pointer leaves it sloping by a pixel.
        if self.kind == Kind::Horizontal {
            match grip {
                Grip::To | Grip::ToFrom => self.to.ts = at.ts,
                _ => self.from.ts = at.ts,
            }
            self.from.price = at.price;
            self.to.price = at.price;
            return;
        }
        if let Grip::Corner(n) = grip {
            if let Some(corner) = self.points.get_mut(n) {
                *corner = at;
            }
            self.settle();
            return;
        }
        match grip {
            Grip::From => self.from = at,
            Grip::To => self.to = at,
            Grip::FromTo => {
                self.from.ts = at.ts;
                self.to.price = at.price;
            }
            Grip::ToFrom => {
                self.to.ts = at.ts;
                self.from.price = at.price;
            }
            // One edge, which is one field of one anchor: whichever of the
            // two is on that side.
            Grip::Top | Grip::Bottom => {
                let top = self.from.price >= self.to.price;
                let anchor = match (grip == Grip::Top) == top {
                    true => &mut self.from,
                    false => &mut self.to,
                };
                anchor.price = at.price;
            }
            Grip::Left | Grip::Right => {
                let left = self.from.ts <= self.to.ts;
                let anchor = match (grip == Grip::Left) == left {
                    true => &mut self.from,
                    false => &mut self.to,
                };
                anchor.ts = at.ts;
            }
            // Answered above, before the kinds that have fixed ends.
            Grip::Corner(_) | Grip::Body => {}
        }
    }

    /// Shift the whole drawing by a span of time and a difference in price.
    pub fn shift(&mut self, by_ts: i64, by_price: f64) {
        for anchor in [&mut self.from, &mut self.to] {
            anchor.ts += by_ts;
            anchor.price += by_price;
        }
        for anchor in self.points.iter_mut() {
            anchor.ts += by_ts;
            anchor.price += by_price;
        }
    }

    /// The grips this drawing wears when selected.
    pub fn grips(&self) -> Vec<Grip> {
        // One per corner, however many there are: a zig-zag with no grip on
        // the corner you want to move is a zig-zag you have to redraw.
        if self.kind.is_path() {
            return (0..self.points.len()).map(Grip::Corner).collect();
        }
        self.fixed_grips().to_vec()
    }

    fn fixed_grips(&self) -> &'static [Grip] {
        match self.kind {
            Kind::Line | Kind::Horizontal | Kind::Arrow => &[Grip::From, Grip::To],
            // The ellipse is dragged by the corners of the box it is drawn
            // in, exactly as the box is, which is what lets it be shaped to
            // anything rather than held round.
            // The four corners, which move two edges at once, and the
            // middle of each edge, which moves one.
            Kind::Rect | Kind::Ellipse => &[
                Grip::From,
                Grip::To,
                Grip::FromTo,
                Grip::ToFrom,
                Grip::Top,
                Grip::Bottom,
                Grip::Left,
                Grip::Right,
            ],
            // None. A word's size is its font size, set from the keyboard or
            // its properties, and a grip on its corner would promise a
            // stretch that is not on offer.
            Kind::Text => &[],
            // Answered above, where the number of them is known.
            Kind::Zigzag => &[],
        }
    }
}

/// What part of a drawing the pointer is on.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grip {
    From,
    To,
    /// A rectangle's corner at `from`'s moment and `to`'s price.
    FromTo,
    /// And the one at `to`'s moment and `from`'s price.
    ToFrom,
    /// The middle of one of a box's four edges: drag it and only that edge
    /// moves, so the shape changes in one direction and holds still in the
    /// other.
    ///
    /// Named for where they sit on screen rather than for an anchor, since
    /// which anchor is the top one depends on which way the box was drawn
    /// and nobody dragging its top edge is thinking about that.
    Top,
    Bottom,
    Left,
    Right,
    /// One corner of a zig-zag, by its place in the run.
    Corner(usize),
    /// The line itself, or the inside of the box: drag to move the whole
    /// thing.
    Body,
}

/// How near the pointer has to be, in pixels, to pick a line or an edge.
pub const PICK_REACH: f64 = 6.0;
/// And to pick a grip, which is a smaller target and worth more.
pub const GRIP_REACH: f64 = 8.0;

/// A drawing projected onto the screen: its two anchors as pixels.
///
/// The chart does the projecting, since only it knows where a moment and a
/// price are on screen; everything about hitting the result is geometry and
/// lives here, where it can be tested without a window.
#[derive(Clone, PartialEq, Debug)]
pub struct Projected {
    pub kind: Kind,
    pub from: (f64, f64),
    pub to: (f64, f64),
    /// A zig-zag's corners in pixels, and empty for every other kind, which
    /// is why this is a plain list rather than an option: "no corners of its
    /// own" and "two anchors" are the same thing to everything below.
    pub corners: Vec<(f64, f64)>,
    /// The box a drawing's words fill, in pixels, for the kinds whose size
    /// is their text: measured by the chart, which is the only place that
    /// can measure text, and `None` until it has.
    ///
    /// A text drawing with no box has no size — nothing to hit and nothing
    /// to select — which is the right answer for a word nobody has laid out
    /// yet, and a state that lasts exactly one frame.
    pub words: Option<(f64, f64, f64, f64)>,
}

impl Projected {
    pub fn new(kind: Kind, from: (f64, f64), to: (f64, f64)) -> Projected {
        Projected { kind, from, to, corners: Vec::new(), words: None }
    }

    /// A projection through a run of corners: the first and the last are
    /// also its two ends, so everything that only wants those goes on
    /// working.
    pub fn through(kind: Kind, corners: Vec<(f64, f64)>) -> Projected {
        let from = corners.first().copied().unwrap_or((0.0, 0.0));
        let to = corners.last().copied().unwrap_or(from);
        Projected { kind, from, to, corners, words: None }
    }

    /// The same projection, told where its words landed.
    pub fn with_words(self, words: (f64, f64, f64, f64)) -> Projected {
        Projected { words: Some(words), ..self }
    }

    /// The segments it is drawn as, end to end.
    pub fn segments(&self) -> Vec<((f64, f64), (f64, f64))> {
        match self.corners.len() >= 2 {
            true => self.corners.windows(2).map(|pair| (pair[0], pair[1])).collect(),
            false => vec![(self.from, self.to)],
        }
    }

    /// Where a grip is on screen.
    pub fn grip(&self, grip: Grip) -> Option<(f64, f64)> {
        if let Grip::Corner(n) = grip {
            return self.corners.get(n).copied();
        }
        match (self.kind, grip) {
            (Kind::Text, _) => None,
            (_, Grip::From) => Some(self.from),
            (_, Grip::To) => Some(self.to),
            (Kind::Rect | Kind::Ellipse, Grip::FromTo) => Some((self.from.0, self.to.1)),
            (Kind::Rect | Kind::Ellipse, Grip::ToFrom) => Some((self.to.0, self.from.1)),
            (Kind::Rect | Kind::Ellipse, edge) => {
                let (left, top, w, h) = self.bounds();
                let (mid_x, mid_y) = (left + w / 2.0, top + h / 2.0);
                match edge {
                    Grip::Top => Some((mid_x, top)),
                    Grip::Bottom => Some((mid_x, top + h)),
                    Grip::Left => Some((left, mid_y)),
                    Grip::Right => Some((left + w, mid_y)),
                    _ => None,
                }
            }
            _ => None,
        }
    }

    /// What the pointer at (`x`, `y`) is on, if anything. Grips win over the
    /// body, because a grip sits on the body and is the harder target.
    pub fn hit(&self, x: f64, y: f64) -> Option<Grip> {
        let corners = (0..self.corners.len()).map(Grip::Corner);
        let fixed = [
            Grip::From,
            Grip::To,
            Grip::FromTo,
            Grip::ToFrom,
            Grip::Top,
            Grip::Bottom,
            Grip::Left,
            Grip::Right,
        ];
        for grip in fixed.into_iter().chain(corners) {
            if let Some(at) = self.grip(grip)
                && distance(at, (x, y)) <= GRIP_REACH
            {
                return Some(grip);
            }
        }
        let reach = PICK_REACH;
        // A label is part of the drawing it is on. Clicking the words picks
        // the drawing — which on a box or an ellipse happens anyway, since
        // the words are inside the shape, but on a line they can sit well
        // clear of the stroke and used to be the one part of a drawing you
        // could not take hold of.
        if self.words.is_some_and(|box_| within(box_, (x, y), reach)) {
            return Some(Grip::Body);
        }
        let on_body = match self.kind {
            // Every kind that is a stroke is picked the same way: near the
            // segment. What differs between them is what is drawn on it, not
            // where it is.
            Kind::Line | Kind::Horizontal | Kind::Arrow | Kind::Zigzag => self
                .segments()
                .iter()
                .any(|(a, b)| distance_to_segment((x, y), *a, *b) <= reach),
            Kind::Rect => {
                let (left, right) = ordered(self.from.0, self.to.0);
                let (top, bottom) = ordered(self.from.1, self.to.1);
                (left - reach..=right + reach).contains(&x) && (top - reach..=bottom + reach).contains(&y)
            }
            // The ellipse itself, not the box it is drawn in: the empty
            // corners belong to whatever is under them, which is the whole
            // reason to reach for an ellipse over a box.
            Kind::Ellipse => inside_ellipse((x, y), self.bounds(), reach),
            // A word is hit on the box its glyphs fill. The box is generous
            // by the same reach every other kind is, since a word is a
            // scatter of thin strokes and aiming between two of them is not
            // a miss.
            // Nothing but its words, which were tried above.
            Kind::Text => false,
        };
        on_body.then_some(Grip::Body)
    }

    /// Whether any of the drawing lies inside a box: the box a hand drags
    /// out to select what it touches. A line counts when the segment
    /// crosses the box, not when its own bounding box does, so a long
    /// diagonal is not taken by a box in the empty corner beside it.
    pub fn touches(&self, (left, top, width, height): (f64, f64, f64, f64)) -> bool {
        let (right, bottom) = (left + width, top + height);
        let overlaps = |(l, t, w, h): (f64, f64, f64, f64)| {
            l <= right && l + w >= left && t <= bottom && t + h >= top
        };
        // A box dragged over a drawing's label takes the drawing, for the
        // same reason clicking the label does.
        if self.words.is_some_and(overlaps) {
            return true;
        }
        match self.kind {
            Kind::Line | Kind::Horizontal | Kind::Arrow | Kind::Zigzag => self
                .segments()
                .iter()
                .any(|(a, b)| segment_meets_box(*a, *b, left, top, right, bottom)),
            // The box the shape fills. A dragged selection is a rough
            // gesture over a region, not a click: catching an ellipse whose
            // bounding box the hand swept is what the hand meant.
            Kind::Rect | Kind::Ellipse => overlaps(self.bounds()),
            Kind::Text => false,
        }
    }

    /// The drawing's box as (left, top, width, height), for drawing it and
    /// for placing text in it. For a text drawing it is the words' own box,
    /// and an empty box at the anchor until they have been measured.
    pub fn bounds(&self) -> (f64, f64, f64, f64) {
        if self.kind.is_text() {
            return self.words.unwrap_or((self.from.0, self.from.1, 0.0, 0.0));
        }
        // Every corner, not only the ends: a zig-zag's box is the box round
        // all of it, which is what a label on one is placed against.
        let mut left = self.from.0.min(self.to.0);
        let mut right = self.from.0.max(self.to.0);
        let mut top = self.from.1.min(self.to.1);
        let mut bottom = self.from.1.max(self.to.1);
        for (x, y) in &self.corners {
            left = left.min(*x);
            right = right.max(*x);
            top = top.min(*y);
            bottom = bottom.max(*y);
        }
        (left, top, right - left, bottom - top)
    }
}

/// Whether a point is in the ellipse inscribed in `bounds`, allowing `reach`
/// pixels of slack all round so the outline is as easy to pick as a line is.
///
/// The slack is applied to the radii rather than to the answer, which is why
/// it is not simply a distance: an ellipse has no single distance to a
/// point, and growing the shape by the reach is both the cheap way and the
/// one that behaves at the ends of a very flat ellipse.
/// Whether a point is in a box, with `reach` pixels of slack all round.
fn within((left, top, w, h): (f64, f64, f64, f64), (x, y): (f64, f64), reach: f64) -> bool {
    (left - reach..=left + w + reach).contains(&x) && (top - reach..=top + h + reach).contains(&y)
}

fn inside_ellipse(point: (f64, f64), bounds: (f64, f64, f64, f64), reach: f64) -> bool {
    let (left, top, width, height) = bounds;
    let (rx, ry) = (width / 2.0 + reach, height / 2.0 + reach);
    if rx <= 0.0 || ry <= 0.0 {
        return false;
    }
    let (cx, cy) = (left + width / 2.0, top + height / 2.0);
    let (dx, dy) = ((point.0 - cx) / rx, (point.1 - cy) / ry);
    dx * dx + dy * dy <= 1.0
}

/// How far a label is held off the edge of the figure it is in, in pixels.
///
/// Enough that the glyphs are not touching the border, little enough that a
/// label in the corner of a small box still reads as being in that corner.
pub const TEXT_INSET: f64 = 4.0;

/// Where the top-left of a block of text goes, given the figure's box on
/// screen and how big the block is.
///
/// Geometry rather than drawing, so the placement can be tested without a
/// font: the chart measures the block and asks where to put it.
///
/// Each kind places text the way that kind wants it:
///
/// - A **box** holds it inside, inset from the edge, which is what text in a
///   shape means everywhere else.
/// - An **ellipse** holds it inside the curve, not inside the corner of the
///   box the curve is drawn in — a label placed at the top-left of an
///   ellipse's bounding box is outside the ellipse. The corners and edges
///   are placed in the largest rectangle the ellipse contains; the centre,
///   which is in the ellipse by definition, uses the whole box.
/// - A **line** has no inside, so the block goes against the segment's box
///   and clear of the stroke: above it for the top row, below for the
///   bottom, and just above the line for the middle one, since a label
///   written across a trendline hides the trendline.
/// - **Text** is its own block: the anchor is its top-left, because a word
///   typed on a chart grows right and down from where the caret was.
pub fn text_origin(
    kind: Kind,
    bounds: (f64, f64, f64, f64),
    (block_w, block_h): (f64, f64),
    at: Place,
) -> (f64, f64) {
    let (left, top, width, height) = bounds;
    if kind.is_text() {
        return (left, top);
    }
    let (across, down) = at.fractions();
    if kind.is_line() {
        let x = left + across * width - across * block_w;
        // Above the box for the top row, below it for the bottom, and
        // clear of the stroke for the middle.
        let y = if down == 0.0 {
            top - block_h - TEXT_INSET
        } else if down == 1.0 {
            top + height + TEXT_INSET
        } else {
            top + height / 2.0 - block_h - TEXT_INSET
        };
        return (x, y);
    }
    // The box the block is placed in: the figure's, pulled in by the inset,
    // and pulled in further for an ellipse's corners and edges so the words
    // stay under the curve.
    let (mut box_w, mut box_h) = (width, height);
    if kind == Kind::Ellipse && !(across == 0.5 && down == 0.5) {
        // The largest rectangle inside an ellipse has sides the axes over
        // root two.
        const INSCRIBED: f64 = std::f64::consts::FRAC_1_SQRT_2;
        box_w *= INSCRIBED;
        box_h *= INSCRIBED;
    }
    let (box_x, box_y) = (left + (width - box_w) / 2.0, top + (height - box_h) / 2.0);
    let inner_w = (box_w - 2.0 * TEXT_INSET).max(0.0);
    let inner_h = (box_h - 2.0 * TEXT_INSET).max(0.0);
    let x = box_x + TEXT_INSET + across * (inner_w - block_w);
    let y = box_y + TEXT_INSET + down * (inner_h - block_h);
    (x, y)
}

fn ordered(a: f64, b: f64) -> (f64, f64) {
    if a <= b { (a, b) } else { (b, a) }
}

/// Whether the segment `a`–`b` passes through the box, by clipping it to
/// the box's four edges (Liang–Barsky): what is left of the segment after
/// the four cuts is inside, and nothing left means it missed.
fn segment_meets_box(a: (f64, f64), b: (f64, f64), left: f64, top: f64, right: f64, bottom: f64) -> bool {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let (mut enter, mut leave) = (0.0f64, 1.0f64);
    for (p, q) in [(-dx, a.0 - left), (dx, right - a.0), (-dy, a.1 - top), (dy, bottom - a.1)] {
        if p == 0.0 {
            if q < 0.0 {
                return false;
            }
            continue;
        }
        let t = q / p;
        if p < 0.0 {
            if t > leave {
                return false;
            }
            enter = enter.max(t);
        } else {
            if t < enter {
                return false;
            }
            leave = leave.min(t);
        }
    }
    true
}

fn distance(a: (f64, f64), b: (f64, f64)) -> f64 {
    (a.0 - b.0).hypot(a.1 - b.1)
}

/// How far `p` is from the segment `a`–`b`.
pub fn distance_to_segment(p: (f64, f64), a: (f64, f64), b: (f64, f64)) -> f64 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let length2 = dx * dx + dy * dy;
    if length2 == 0.0 {
        return distance(p, a);
    }
    let t = (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / length2).clamp(0.0, 1.0);
    distance(p, (a.0 + t * dx, a.1 + t * dy))
}

#[cfg(test)]
mod shape_tests {
    use super::*;

    /// A box takes a line it crosses and leaves one whose bounding box it
    /// only shares a corner of; a rectangle counts as soon as they overlap.
    #[test]
    fn a_box_takes_what_it_touches() {
        let diagonal = Projected::new(Kind::Line, (0.0, 0.0), (100.0, 100.0));
        assert!(diagonal.touches((40.0, 40.0, 20.0, 20.0)));
        assert!(diagonal.touches((90.0, 50.0, 30.0, 60.0)));
        assert!(!diagonal.touches((60.0, 0.0, 30.0, 30.0)));
        assert!(!diagonal.touches((120.0, 120.0, 10.0, 10.0)));
        let flat = Projected::new(Kind::Line, (10.0, 50.0), (90.0, 50.0));
        assert!(flat.touches((0.0, 40.0, 20.0, 20.0)));
        assert!(!flat.touches((0.0, 60.0, 200.0, 20.0)));
        let rect = Projected::new(Kind::Rect, (10.0, 10.0), (50.0, 50.0));
        assert!(rect.touches((45.0, 45.0, 20.0, 20.0)));
        assert!(rect.touches((20.0, 20.0, 5.0, 5.0)));
        assert!(!rect.touches((51.0, 0.0, 20.0, 20.0)));
    }

    fn line() -> Projected {
        Projected::new(Kind::Line, (100.0, 100.0), (300.0, 200.0))
    }

    fn rect() -> Projected {
        Projected::new(Kind::Rect, (300.0, 200.0), (100.0, 100.0))
    }

    #[test]
    fn an_anchor_is_picked_before_the_body_under_it() {
        assert_eq!(line().hit(102.0, 101.0), Some(Grip::From));
        assert_eq!(line().hit(297.0, 203.0), Some(Grip::To));
        assert_eq!(rect().hit(101.0, 101.0), Some(Grip::To));
    }

    /// A rectangle has four corners to take hold of, and a line only its
    /// two ends.
    #[test]
    fn a_box_has_four_corners_and_a_line_two_ends() {
        assert_eq!(rect().hit(300.0, 100.0), Some(Grip::FromTo));
        assert_eq!(rect().hit(100.0, 200.0), Some(Grip::ToFrom));
        assert_eq!(line().grip(Grip::FromTo), None);
        let mut drawing = Drawing::new(Kind::Rect, Anchor::new(10, 5.0), Anchor::new(20, 1.0));
        drawing.move_grip(Grip::FromTo, Anchor::new(12, 0.5));
        assert_eq!((drawing.from, drawing.to), (Anchor::new(12, 5.0), Anchor::new(20, 0.5)));
        drawing.move_grip(Grip::ToFrom, Anchor::new(25, 6.0));
        assert_eq!((drawing.from, drawing.to), (Anchor::new(12, 6.0), Anchor::new(25, 0.5)));
    }

    #[test]
    fn a_line_is_picked_near_it_and_not_away_from_it() {
        // The midpoint, and a few pixels off it.
        assert_eq!(line().hit(200.0, 150.0), Some(Grip::Body));
        assert_eq!(line().hit(200.0, 154.0), Some(Grip::Body));
        assert_eq!(line().hit(200.0, 170.0), None);
        // Beyond either end is not on the line.
        assert_eq!(line().hit(60.0, 80.0), None);
    }

    #[test]
    fn a_box_is_picked_anywhere_inside_it_whichever_way_it_was_drawn() {
        assert_eq!(rect().hit(200.0, 150.0), Some(Grip::Body));
        assert_eq!(rect().hit(150.0, 120.0), Some(Grip::Body));
        assert_eq!(rect().hit(50.0, 150.0), None);
        assert_eq!(rect().bounds(), (100.0, 100.0, 200.0, 100.0));
    }

    #[test]
    fn a_drawing_round_trips_through_json_and_fills_in_what_an_old_one_lacks() {
        let mut drawing = Drawing::new(Kind::Rect, Anchor::new(1_700_000_000, 101.5), Anchor::new(1_700_086_400, 99.0));
        drawing.follow(4);
        drawing.scope = Scope::Local;
        let json = serde_json::to_string(&drawing).unwrap();
        assert!(json.contains("\"scope\":\"local\""), "{json}");
        assert_eq!(serde_json::from_str::<Drawing>(&json).unwrap(), drawing);

        let bare = r#"{"kind":"line","from":{"ts":1,"price":2.0},"to":{"ts":3,"price":4.0}}"#;
        let old: Drawing = serde_json::from_str(bare).unwrap();
        assert_eq!(old.config, Some(1));
        assert_eq!(old.style, None);
        assert_eq!(old.scope, Scope::Shared);
        assert_eq!(old.id, 0);
    }

    #[test]
    fn shifting_moves_both_anchors_together() {
        let mut drawing = Drawing::new(Kind::Line, Anchor::new(10, 1.0), Anchor::new(20, 2.0));
        drawing.shift(5, 0.5);
        assert_eq!((drawing.from, drawing.to), (Anchor::new(15, 1.5), Anchor::new(25, 2.5)));
        *drawing.anchor_mut(Grip::To).unwrap() = Anchor::new(30, 3.0);
        assert_eq!(drawing.to, Anchor::new(30, 3.0));
        assert!(drawing.anchor_mut(Grip::Body).is_none());
    }

    /// A drawing follows its configuration until a property is set by
    /// hand, and then keeps its own look whatever the configuration does.
    #[test]
    fn a_drawing_follows_its_configuration_until_edited() {
        let mut configs = Configurations::default();
        let mut drawing = Drawing::new(Kind::Line, Anchor::new(1, 1.0), Anchor::new(2, 2.0));
        drawing.follow(4);
        assert_eq!(drawing.style(&configs).colour, Paint::preset(Preset::Amber));

        configs.set(Kind::Line, 4, Style { width: 4.0, ..Style::line(Preset::Teal) });
        assert_eq!(drawing.style(&configs).colour, Paint::preset(Preset::Teal));
        assert_eq!(drawing.style(&configs).width, 4.0);
        assert!(!configs.is_default(Kind::Line, 4));

        drawing.edit_style(&configs, |s| s.arrow = Arrow::End);
        assert_eq!(drawing.config, None);
        configs.reset(Kind::Line);
        assert!(configs.is_default(Kind::Line, 4));
        let own = drawing.style(&configs);
        assert_eq!((own.colour.clone(), own.width, own.arrow), (Paint::preset(Preset::Teal), 4.0, Arrow::End));
    }

    #[test]
    fn the_default_configurations_are_the_nine_presets_in_order() {
        let configs = Configurations::default();
        for (n, preset) in Preset::ALL.into_iter().enumerate() {
            let n = n as u8 + 1;
            assert_eq!(configs.of(Kind::Line, n).colour, Paint::preset(preset));
            assert_eq!(configs.of(Kind::Rect, n).fill, Paint::preset(preset));
            assert_eq!(configs.of(Kind::Rect, n).alpha, FILL_ALPHA);
            assert_eq!(configs.of(Kind::Line, n).arrow, Arrow::None);
            assert_eq!(configs.of(Kind::Line, n).width, DEFAULT_WIDTH);
        }
        assert_eq!(configs.of(Kind::Line, 1).colour, Paint::preset(Preset::Up));
        assert_eq!(configs.of(Kind::Rect, 2).fill, Paint::preset(Preset::Down));
        // Out of range is the first, never a panic.
        assert_eq!(configs.of(Kind::Rect, 0), configs.of(Kind::Rect, 1));
        assert_eq!(configs.of(Kind::Rect, 40), configs.of(Kind::Rect, 9));
    }

    /// Two scopes, and the words every old one was written down as still
    /// read: a drawing somebody meant to share comes back shared rather
    /// than quietly becoming private to whichever chart opens first.
    #[test]
    fn a_scope_is_spelled_the_way_it_is_written_down() {
        for scope in Scope::all() {
            assert_eq!(Scope::from_key(scope.key()), Some(scope));
            let json = serde_json::to_string(&scope).unwrap();
            assert_eq!(serde_json::from_str::<Scope>(&json).unwrap(), scope);
        }
        // What the groups left behind.
        assert_eq!(Scope::from_key("global"), Some(Scope::Shared));
        assert_eq!(Scope::from_key("group-3"), Some(Scope::Shared));
        assert_eq!(Scope::from_key("group-nonsense"), Some(Scope::Shared));
        assert_eq!(Scope::from_key("LOCAL"), Some(Scope::Local));
        assert_eq!(Scope::from_key("whatever"), None);
        // And a drawing written down with one of them reads back shared.
        let old = r#"{"kind":"line","from":{"ts":1,"price":2.0},"to":{"ts":3,"price":4.0},"scope":"group-7"}"#;
        assert_eq!(serde_json::from_str::<Drawing>(old).unwrap().scope, Scope::Shared);
    }

    #[test]
    fn paint_is_spelled_the_way_it_is_typed() {
        assert_eq!(Paint::parse("amber"), Some(Paint::preset(Preset::Amber)));
        assert_eq!(Paint::parse("#FFaa00"), Some(Paint::fixed("#ffaa00")));
        assert_eq!(Paint::parse("#fa0"), None);
        assert_eq!(Paint::parse("red"), None);
        assert_eq!(Paint::preset(Preset::Ink).spell(), "ink");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::omarchy;
    use crate::theme::builtin_themes;
    use std::collections::HashMap;
    use std::path::Path;

    /// Every theme the app can wear: the Omarchy fixtures through the same
    /// derivation the app runs, and the built-ins.
    pub(super) fn every_theme() -> Vec<Theme> {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/omarchy");
        let mut out = Vec::new();
        for entry in std::fs::read_dir(&dir).expect("fixtures directory") {
            let path = entry.expect("entry").path();
            if path.extension().and_then(|e| e.to_str()) != Some("toml") {
                continue;
            }
            let name = path.file_stem().unwrap().to_string_lossy().to_string();
            let text = std::fs::read_to_string(&path).expect("read fixture");
            let keys: HashMap<String, String> = omarchy::parse(&text);
            out.push(omarchy::derive(&keys, &name));
        }
        out.extend(builtin_themes());
        assert!(out.len() >= 26, "the fixtures went missing");
        out
    }

    #[test]
    fn up_and_down_are_the_candles_to_the_byte() {
        for theme in every_theme() {
            let bars = theme_bars(&theme);
            assert_eq!(colour(&theme, Preset::Up), bars.up, "{}", theme.name);
            assert_eq!(colour(&theme, Preset::Down), bars.down, "{}", theme.name);
        }
    }

    /// Two presets that look alike are two drawings the user cannot tell
    /// apart, and a preset that looks like a candle reads as price action.
    #[test]
    fn no_two_presets_look_alike_and_none_looks_like_a_candle() {
        for theme in every_theme() {
            let palette = palette(&theme);
            for (i, (a, hex_a)) in palette.iter().enumerate() {
                for (b, hex_b) in &palette[i + 1..] {
                    let d = delta_e(hex_a, hex_b);
                    assert!(
                        d >= MIN_SEPARATION,
                        "{}: {} {hex_a} reads as {} {hex_b} ({d:.3})",
                        theme.name,
                        a.name(),
                        b.name()
                    );
                }
            }
        }
    }

    #[test]
    fn every_preset_can_be_seen_on_its_chart() {
        for theme in every_theme() {
            for (preset, hex) in palette(&theme) {
                let floor = if preset.is_direction() {
                    3.0
                } else {
                    PALETTE_CONTRAST
                };
                let ratio = contrast_ratio(&hex, &theme.ui.background);
                assert!(
                    ratio >= floor,
                    "{}: {} {hex} is {ratio:.2}:1",
                    theme.name,
                    preset.name()
                );
            }
        }
    }

    /// Most presets reach the drawing floor; the ones that do not took the
    /// faithful tier on purpose, and there are few of them.
    #[test]
    fn nearly_every_colour_preset_reaches_the_drawing_floor() {
        let mut quiet = Vec::new();
        for theme in every_theme() {
            for (preset, hex) in palette(&theme) {
                if !preset.is_direction()
                    && contrast_ratio(&hex, &theme.ui.background) < DRAWING_CONTRAST.floor
                {
                    quiet.push(format!("{} {}", theme.name, preset.name()));
                }
            }
        }
        assert!(
            quiet.len() <= 3,
            "too many presets below {}:1: {quiet:?}",
            DRAWING_CONTRAST.floor
        );
    }

    #[test]
    fn no_colour_preset_can_be_mistaken_for_the_furniture() {
        for theme in every_theme() {
            let ui = &theme.ui;
            let crosshair = mix(&ui.crosshair, &ui.background, 1.0 - CROSSHAIR_ALPHA);
            for (preset, hex) in palette(&theme) {
                if preset.is_direction() {
                    continue;
                }
                for (what, f) in [
                    ("grid", &ui.grid),
                    ("axis", &ui.axis),
                    ("crosshair", &crosshair),
                ] {
                    let d = delta_e(&hex, f);
                    assert!(
                        d >= MIN_FROM_FURNITURE,
                        "{}: {} {hex} sits on the {what} ({d:.3})",
                        theme.name,
                        preset.name()
                    );
                }
            }
        }
    }

    /// A preset keeps the hue the name promises: a drawing saved as Amber is
    /// amber on every theme, which is the whole point of storing a role.
    #[test]
    fn a_placed_preset_keeps_its_swatch_hue() {
        for theme in every_theme() {
            for (preset, hex) in palette(&theme) {
                let Some(name) = preset.swatch() else {
                    continue;
                };
                let swatch = theme.swatch(name).unwrap();
                let (a, b) = (Oklch::of(&hex).unwrap(), Oklch::of(&swatch.hex).unwrap());
                if a.c > 0.01 && b.c > 0.01 {
                    let gap = crate::theme::hue_gap(a.h, b.h);
                    assert!(
                        gap < 2.0,
                        "{}: {} turned from {} to {hex} ({gap:.1}°)",
                        theme.name,
                        preset.name(),
                        swatch.hex
                    );
                }
            }
        }
    }

    #[test]
    fn ink_is_a_neutral() {
        for theme in every_theme() {
            let ink = Oklch::of(&colour(&theme, Preset::Ink)).unwrap();
            assert!(
                ink.c <= INK_CHROMA + 0.005,
                "{}: ink has a hue ({:.3})",
                theme.name,
                ink.c
            );
        }
    }

    /// The fill goes over the candles, and they must still read through it:
    /// up from down, on every theme.
    #[test]
    fn candles_still_read_through_a_fill() {
        for theme in every_theme() {
            let bars = theme_bars(&theme);
            for (preset, hex) in palette(&theme) {
                let up = fill_over(&hex, &bars.up);
                let down = fill_over(&hex, &bars.down);
                let d = delta_e(&up, &down);
                assert!(
                    d >= 0.06,
                    "{}: under a {} fill up and down read as one ({d:.3})",
                    theme.name,
                    preset.name()
                );
            }
        }
    }

    /// The fill is felt and the border is an edge: neither fainter than the
    /// chart's own grid, neither louder than its axis.
    #[test]
    fn a_fill_is_felt_and_a_border_is_an_edge() {
        for theme in every_theme() {
            let bg = &theme.ui.background;
            for (preset, hex) in palette(&theme) {
                let fill = contrast_ratio(&fill_over(&hex, bg), bg);
                assert!(
                    (1.08..=2.2).contains(&fill),
                    "{}: {} fill is {fill:.2}:1",
                    theme.name,
                    preset.name()
                );
                let border = contrast_ratio(&border_over(&hex, bg), bg);
                assert!(
                    border >= 1.35,
                    "{}: {} border is {border:.2}:1",
                    theme.name,
                    preset.name()
                );
                let step = delta_e(&border_over(&hex, bg), &fill_over(&hex, bg));
                assert!(
                    step >= 0.06,
                    "{}: {} border is not a step up from its fill ({step:.3})",
                    theme.name,
                    preset.name()
                );
            }
        }
    }

    #[test]
    fn presets_round_trip_through_serde() {
        for preset in Preset::ALL {
            let json = serde_json::to_string(&preset).unwrap();
            assert_eq!(serde_json::from_str::<Preset>(&json).unwrap(), preset);
        }
        assert_eq!(serde_json::to_string(&Preset::Ink).unwrap(), "\"ink\"");
    }
}

#[cfg(test)]
mod ellipse_tests {
    use super::*;

    fn ellipse(from: (f64, f64), to: (f64, f64)) -> Projected {
        Projected::new(Kind::Ellipse, from, to)
    }

    /// The point of reaching for an ellipse over a box is that its corners
    /// are not part of it: a click in one belongs to whatever is underneath.
    #[test]
    fn an_ellipse_is_picked_on_the_curve_and_not_in_the_corners() {
        let oval = ellipse((0.0, 0.0), (200.0, 100.0));
        assert_eq!(oval.hit(100.0, 50.0), Some(Grip::Body), "the middle missed");
        // The ends of the two axes are where the edge grips sit, so they
        // answer as those rather than as the shape.
        assert_eq!(oval.hit(4.0, 50.0), Some(Grip::Left));
        assert_eq!(oval.hit(100.0, 4.0), Some(Grip::Top));
        // The curve between them is the shape.
        let quarter = std::f64::consts::FRAC_1_SQRT_2;
        let on_curve = (100.0 + 100.0 * quarter, 50.0 - 50.0 * quarter);
        assert_eq!(oval.hit(on_curve.0, on_curve.1), Some(Grip::Body), "the curve missed");
        // The bounding box's corners, which are well outside the curve.
        for corner in [(0.0, 0.0), (200.0, 0.0), (0.0, 100.0), (200.0, 100.0)] {
            // The corners are also where two grips sit, and a grip is a hit.
            // Step a little inside the box and away from both.
            let (x, y) = (
                corner.0 + if corner.0 == 0.0 { 14.0 } else { -14.0 },
                corner.1 + if corner.1 == 0.0 { 9.0 } else { -9.0 },
            );
            assert_eq!(oval.hit(x, y), None, "the corner at {corner:?} was taken");
        }
    }

    /// An ellipse is dragged by the corners of the box it is drawn in, the
    /// same four a box has, which is what lets it be shaped freely.
    #[test]
    fn an_ellipse_wears_a_box_s_four_grips() {
        let drawing = Drawing::new(Kind::Ellipse, Anchor::new(0, 1.0), Anchor::new(10, 2.0));
        assert_eq!(drawing.grips(), Drawing::new(Kind::Rect, Anchor::new(0, 1.0), Anchor::new(10, 2.0)).grips());
        let oval = ellipse((0.0, 0.0), (200.0, 100.0));
        assert_eq!(oval.grip(Grip::FromTo), Some((0.0, 100.0)));
        assert_eq!(oval.grip(Grip::ToFrom), Some((200.0, 0.0)));
    }

    /// Drawn backwards — the hand went right to left — and it is the same
    /// ellipse.
    #[test]
    fn an_ellipse_is_the_same_drawn_either_way() {
        let forward = ellipse((0.0, 0.0), (200.0, 100.0));
        let backward = ellipse((200.0, 100.0), (0.0, 0.0));
        assert_eq!(forward.bounds(), backward.bounds());
        assert_eq!(backward.hit(100.0, 50.0), Some(Grip::Body));
    }
}

#[cfg(test)]
mod text_tests {
    use super::*;

    fn words(text: &str) -> Text {
        Text::plain(text)
    }

    /// Runs that share a style are one run, and empty runs are not runs at
    /// all: editing must not grow the spans without changing the text.
    #[test]
    fn tidying_runs_neighbours_together_and_drops_the_empty() {
        let text = Text {
            spans: vec![
                Span::plain("Held "),
                Span::plain(""),
                Span::plain("at "),
                Span { text: "192".into(), bold: true, italic: false },
                Span { text: "".into(), bold: true, italic: false },
            ],
            at: Place::Center,
        }
        .tidied();
        assert_eq!(text.spans.len(), 2, "{:?}", text.spans);
        assert_eq!(text.spans[0], Span::plain("Held at "));
        assert_eq!(text.plain_text(), "Held at 192");
    }

    /// A figure with nothing typed into it has nothing to draw, which is not
    /// the same as having an empty line.
    #[test]
    fn text_is_empty_until_something_is_typed() {
        assert!(Text::default().is_empty());
        assert!(words("").is_empty());
        assert!(!words(" ").is_empty(), "a space is something");
    }

    /// The text never rides on a configuration: two drawings following the
    /// same number say different things.
    #[test]
    fn what_a_drawing_says_is_not_part_of_its_configuration() {
        let configs = Configurations::default();
        let mut a = Drawing::new(Kind::Rect, Anchor::new(0, 1.0), Anchor::new(10, 2.0));
        let mut b = a.clone();
        a.set_text(words("support"));
        b.set_text(words("resistance"));
        assert_eq!(a.config, Some(1), "saying something took the drawing off its configuration");
        assert_eq!(a.style(&configs), b.style(&configs));
        assert_ne!(a.text, b.text);
    }

    /// A word has no grips: its size is its font size, and a grip on its
    /// corner would promise a stretch that is not on offer.
    #[test]
    fn a_text_drawing_has_nothing_to_drag_but_itself() {
        let drawing = Drawing::text_at(Anchor::new(0, 1.0));
        assert!(drawing.grips().is_empty());
        let projected = Projected::new(Kind::Text, (50.0, 50.0), (50.0, 50.0));
        assert_eq!(projected.grip(Grip::From), None);
        // And nothing to hit until the words have been measured.
        assert_eq!(projected.hit(50.0, 50.0), None);
        let measured = projected.with_words((50.0, 50.0, 60.0, 16.0));
        assert_eq!(measured.hit(60.0, 55.0), Some(Grip::Body));
        assert_eq!(measured.hit(200.0, 55.0), None);
    }

    /// Both anchors follow, whichever is moved, so the box a word is hit on
    /// never depends on which grip was dragged last.
    #[test]
    fn moving_a_word_moves_both_its_anchors() {
        let mut drawing = Drawing::text_at(Anchor::new(100, 5.0));
        drawing.move_grip(Grip::To, Anchor::new(200, 9.0));
        assert_eq!(drawing.from, drawing.to);
        assert_eq!(drawing.from, Anchor::new(200, 9.0));
    }

    /// The nine places, in a box: each lands the block against the edges its
    /// name says, inside the figure.
    #[test]
    fn the_nine_places_put_a_block_where_they_say() {
        let bounds = (0.0, 0.0, 200.0, 100.0);
        let block = (40.0, 20.0);
        let at = |place| text_origin(Kind::Rect, bounds, block, place);
        assert_eq!(at(Place::TopLeft), (TEXT_INSET, TEXT_INSET));
        let (x, y) = at(Place::Center);
        assert!((x - 80.0).abs() < 0.01 && (y - 40.0).abs() < 0.01, "centre landed at {x},{y}");
        let (x, y) = at(Place::BottomRight);
        assert!(
            (x - (200.0 - TEXT_INSET - 40.0)).abs() < 0.01
                && (y - (100.0 - TEXT_INSET - 20.0)).abs() < 0.01,
            "bottom right landed at {x},{y}"
        );
        // Top is centred across and against the top; left is centred down
        // and against the left.
        assert_eq!(at(Place::Top).1, TEXT_INSET);
        assert_eq!(at(Place::Left).0, TEXT_INSET);
        assert!((at(Place::Top).0 - 80.0).abs() < 0.01);
    }

    /// A label in the corner of an ellipse goes under the curve, not into
    /// the corner of the box the curve is drawn in — which is outside it.
    #[test]
    fn an_ellipse_keeps_its_corner_labels_under_the_curve() {
        let bounds = (0.0, 0.0, 200.0, 100.0);
        let block = (30.0, 14.0);
        let in_box = text_origin(Kind::Rect, bounds, block, Place::TopLeft);
        let in_oval = text_origin(Kind::Ellipse, bounds, block, Place::TopLeft);
        assert!(in_oval.0 > in_box.0 && in_oval.1 > in_box.1, "{in_oval:?} was not pulled in from {in_box:?}");
        // And the block's own corner is inside the ellipse.
        let corner = (in_oval.0, in_oval.1);
        assert!(
            inside_ellipse(corner, bounds, 0.0),
            "the label's corner at {corner:?} fell outside the curve"
        );
        // The centre is in the ellipse by definition, so it uses the whole
        // box and lands where a box would put it.
        assert_eq!(
            text_origin(Kind::Ellipse, bounds, block, Place::Center),
            text_origin(Kind::Rect, bounds, block, Place::Center)
        );
    }

    /// A label on a line goes clear of the stroke: a note written across a
    /// trendline hides the trendline.
    #[test]
    fn a_label_on_a_line_sits_off_the_line() {
        // A flat line: its box has no height at all.
        let bounds = (0.0, 50.0, 200.0, 0.0);
        let block = (40.0, 20.0);
        let (_, y) = text_origin(Kind::Line, bounds, block, Place::Center);
        assert!(y + block.1 <= 50.0, "the label at {y} crossed the line at 50");
        let (_, below) = text_origin(Kind::Line, bounds, block, Place::Bottom);
        assert!(below >= 50.0, "the bottom label at {below} crossed the line at 50");
    }

    /// A word hangs from its anchor by the top-left corner: typing on a
    /// chart grows right and down from where the caret was.
    #[test]
    fn a_word_hangs_from_the_caret() {
        let bounds = (120.0, 80.0, 0.0, 0.0);
        for place in Place::ALL {
            assert_eq!(text_origin(Kind::Text, bounds, (50.0, 16.0), place), (120.0, 80.0));
        }
    }

    /// Text is held higher than a line is, and against what it actually
    /// lands on: inside a filled box that is the composited fill, not the
    /// chart's background.
    #[test]
    fn a_label_is_held_to_the_ground_it_lands_on() {
        let theme = crate::theme::builtin_themes()[0].clone();
        let mut style = Style::rect(Preset::Amber);
        let on_fill = text_ground(Kind::Rect, &style, &theme);
        assert_ne!(on_fill, theme.ui.background, "a filled box read as bare background");
        // With nothing shown of the fill there is nothing but the background.
        style.alpha = 0.0;
        assert_eq!(text_ground(Kind::Rect, &style, &theme), theme.ui.background);
        // And a line has no inside at all.
        assert_eq!(
            text_ground(Kind::Line, &Style::line(Preset::Amber), &theme),
            theme.ui.background
        );
    }

    /// Every preset's text clears the floor on every shipped theme, over the
    /// ground it is actually drawn on — which is the whole promise of the
    /// text band.
    #[test]
    fn every_preset_s_text_is_readable_on_every_theme() {
        for theme in super::tests::every_theme() {
            for kind in Kind::ALL {
                for preset in Preset::ALL {
                    let style = Style::of(kind, preset);
                    let ground = text_ground(kind, &style, &theme);
                    let ink = text_colour(&style.text.colour, &theme, &ground);
                    let ratio = contrast_ratio(&ink, &ground);
                    assert!(
                        ratio >= TEXT_CONTRAST.floor - 0.01,
                        "{} {} text is {ratio:.2}:1 on {}",
                        kind.key(),
                        preset.name(),
                        theme.name
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod configuration_tests {
    use super::*;

    /// A store written before a kind existed has no list for it, and must
    /// open on that kind's defaults rather than on nothing.
    #[test]
    fn a_kind_the_stored_settings_predate_reads_as_shipped() {
        let json = r#"{"line":[],"rect":[]}"#;
        let configs: Configurations = serde_json::from_str(json).unwrap();
        for n in 1..=CONFIGURATIONS {
            assert_eq!(configs.of(Kind::Ellipse, n), Configurations::default().of(Kind::Ellipse, n));
            assert_eq!(configs.of(Kind::Text, n), Configurations::default().of(Kind::Text, n));
            assert!(configs.is_default(Kind::Text, n));
        }
    }

    /// And a write to such a list lands rather than vanishing into an empty
    /// vector.
    #[test]
    fn writing_a_configuration_of_a_fresh_kind_sticks() {
        let mut configs: Configurations = serde_json::from_str(r#"{"line":[],"rect":[]}"#).unwrap();
        let mut style = Style::text(Preset::Cyan);
        style.text.size = 31.0;
        configs.set(Kind::Text, 4, style.clone());
        assert_eq!(configs.of(Kind::Text, 4), &style);
        assert!(!configs.is_default(Kind::Text, 4));
        // The rest of the nine are still as shipped.
        assert!(configs.is_default(Kind::Text, 5));
        configs.reset(Kind::Text);
        assert!(configs.is_default(Kind::Text, 4));
    }

    /// Nine of every kind, named by the preset of that number, so Alt+4 is
    /// amber whichever tool is in hand.
    #[test]
    fn the_nine_are_the_nine_presets_for_every_kind() {
        let configs = Configurations::default();
        for kind in Kind::ALL {
            assert_eq!(configs.list(kind).len(), CONFIGURATIONS as usize);
            for (at, preset) in Preset::ALL.into_iter().enumerate() {
                assert_eq!(configs.of(kind, at as u8 + 1), &Style::of(kind, preset));
            }
        }
    }

    /// The names a command line uses round-trip, and "circle" is taken for
    /// the ellipse because that is what the tool is called.
    #[test]
    fn every_kind_is_spelled_and_read_back() {
        for kind in Kind::ALL {
            assert_eq!(Kind::from_key(kind.key()), Some(kind));
            let json = serde_json::to_string(&kind).unwrap();
            assert_eq!(serde_json::from_str::<Kind>(&json).unwrap(), kind);
        }
        assert_eq!(Kind::from_key("circle"), Some(Kind::Ellipse));
        assert_eq!(Kind::from_key("CIRCLE"), Some(Kind::Ellipse));
        assert_eq!(Kind::from_key("blob"), None);
    }

    /// A drawing written down before text existed reads back with none, and
    /// one with nothing to say does not write the field at all.
    #[test]
    fn a_drawing_without_text_round_trips_without_it() {
        let drawing = Drawing::new(Kind::Rect, Anchor::new(1, 1.0), Anchor::new(2, 2.0));
        let json = serde_json::to_string(&drawing).unwrap();
        assert!(!json.contains("text"), "{json}");
        assert_eq!(serde_json::from_str::<Drawing>(&json).unwrap(), drawing);

        let mut labelled = drawing.clone();
        labelled.set_text(Text { spans: vec![Span { text: "hi".into(), bold: true, italic: false }], at: Place::Top });
        let json = serde_json::to_string(&labelled).unwrap();
        assert_eq!(serde_json::from_str::<Drawing>(&json).unwrap(), labelled);
    }

    /// The places a command line writes round-trip too, British spelling
    /// included.
    #[test]
    fn every_place_is_spelled_and_read_back() {
        for place in Place::ALL {
            assert_eq!(Place::from_key(place.key()), Some(place));
            let json = serde_json::to_string(&place).unwrap();
            assert_eq!(serde_json::from_str::<Place>(&json).unwrap(), place);
        }
        assert_eq!(Place::from_key("centre"), Some(Place::Center));
        assert_eq!(Place::from_key("top_left"), Some(Place::TopLeft));
    }
}

#[cfg(test)]
mod stroke_tests {
    use super::*;

    /// A level stays level, however it is dragged: the whole point of the
    /// kind is that a price read off it is the price.
    #[test]
    fn a_horizontal_line_cannot_be_made_to_slope() {
        let mut level = Drawing::new(Kind::Horizontal, Anchor::new(0, 100.0), Anchor::new(10, 100.0));
        // Pull the right end up and away: it carries the whole line with it.
        level.move_grip(Grip::To, Anchor::new(20, 140.0));
        assert_eq!(level.from.price, level.to.price);
        assert_eq!(level.to.price, 140.0);
        assert_eq!(level.from.ts, 0, "the end that was not dragged moved in time");
        assert_eq!(level.to.ts, 20);
        // And the other end, the same way.
        level.move_grip(Grip::From, Anchor::new(-5, 90.0));
        assert_eq!(level.from.price, level.to.price);
        assert_eq!(level.to.ts, 20);
        // Shifting the whole thing keeps it level too.
        level.shift(3, -10.0);
        assert_eq!(level.from.price, level.to.price);
    }

    /// An arrow is a line that points, and is one everywhere it matters:
    /// picked like a line, dragged like a line, labelled like a line.
    #[test]
    fn an_arrow_is_a_line_that_points() {
        let arrow = Style::arrow(Preset::Blue);
        assert_eq!(arrow.arrow, Arrow::End);
        assert_eq!(arrow.head, ArrowHead::ALL[1]);
        assert_eq!(Style { arrow: Arrow::None, head: ArrowHead::Filled, ..arrow.clone() }, Style::line(Preset::Blue));

        let drawing = Drawing::new(Kind::Arrow, Anchor::new(0, 1.0), Anchor::new(10, 2.0));
        assert_eq!(drawing.grips(), vec![Grip::From, Grip::To]);
        let projected = Projected::new(Kind::Arrow, (0.0, 0.0), (100.0, 100.0));
        assert_eq!(projected.hit(50.0, 50.0), Some(Grip::Body));
        assert_eq!(projected.hit(10.0, 90.0), None);
    }

    /// Every kind that is a stroke says so, and no kind that is not does.
    #[test]
    fn the_strokes_know_they_are_strokes() {
        for kind in Kind::ALL {
            let stroke =
                matches!(kind, Kind::Line | Kind::Horizontal | Kind::Arrow | Kind::Zigzag);
            assert_eq!(kind.is_line(), stroke, "{}", kind.key());
            assert!(!(kind.is_line() && kind.is_bounded()), "{}", kind.key());
        }
    }

    /// A label is part of the drawing it is on: clicking the words takes
    /// hold of it, which on a line is the only way to reach them.
    #[test]
    fn a_label_is_part_of_what_it_labels() {
        // A flat line across the middle, with its words well above it.
        let line = Projected::new(Kind::Line, (0.0, 100.0), (200.0, 100.0))
            .with_words((80.0, 40.0, 50.0, 18.0));
        assert_eq!(line.hit(100.0, 48.0), Some(Grip::Body), "the words were not part of it");
        assert_eq!(line.hit(100.0, 100.0), Some(Grip::Body), "the stroke stopped answering");
        assert_eq!(line.hit(190.0, 48.0), None, "beside the words is not on them");
        // And a box dragged over only the words takes it.
        assert!(line.touches((70.0, 30.0, 20.0, 20.0)));
    }
}

#[cfg(test)]
mod started_from_tests {
    use super::*;

    fn amber_box() -> (Configurations, Drawing) {
        let configs = Configurations::default();
        let mut drawing = Drawing::new(Kind::Rect, Anchor::new(0, 1.0), Anchor::new(10, 2.0));
        drawing.follow(4);
        (configs, drawing)
    }

    /// A drawing that leaves its configuration remembers which one it left.
    #[test]
    fn a_custom_look_remembers_where_it_started() {
        let (configs, mut drawing) = amber_box();
        assert_eq!(drawing.started_from, None, "a drawing that follows one has not left one");
        drawing.edit_style(&configs, |style| style.width = 3.0);
        assert_eq!(drawing.config, None);
        assert_eq!(drawing.started_from, Some(4));
    }

    /// And goes on remembering the one it left, not the last edit.
    #[test]
    fn editing_again_does_not_move_where_it_started() {
        let (configs, mut drawing) = amber_box();
        drawing.edit_style(&configs, |style| style.width = 3.0);
        drawing.edit_style(&configs, |style| style.width = 5.0);
        drawing.edit_style(&configs, |style| style.alpha = 0.5);
        assert_eq!(drawing.started_from, Some(4));
    }

    /// Following one again is a fresh start: there is no look of its own
    /// left to have come from.
    #[test]
    fn following_again_forgets_it() {
        let (configs, mut drawing) = amber_box();
        drawing.edit_style(&configs, |style| style.width = 3.0);
        drawing.follow(7);
        assert_eq!(drawing.started_from, None);
        drawing.edit_style(&configs, |style| style.width = 3.0);
        assert_eq!(drawing.started_from, Some(7));
    }

    /// It survives being written down, and costs nothing on a drawing that
    /// never left a configuration.
    #[test]
    fn it_round_trips_and_is_not_written_when_there_is_none() {
        let (configs, mut drawing) = amber_box();
        let json = serde_json::to_string(&drawing).unwrap();
        assert!(!json.contains("started_from"), "{json}");
        drawing.edit_style(&configs, |style| style.width = 3.0);
        let json = serde_json::to_string(&drawing).unwrap();
        assert!(json.contains("started_from"), "{json}");
        assert_eq!(serde_json::from_str::<Drawing>(&json).unwrap(), drawing);
    }
}

#[cfg(test)]
mod zigzag_tests {
    use super::*;

    fn at(ts: i64, price: f64) -> Anchor {
        Anchor::new(ts, price)
    }

    /// The gesture, as the chart performs it: a press starts it, every
    /// press after that pins the corner the hand is on and starts another
    /// following it, and finishing drops the one still following.
    fn drawn_through(corners: &[Anchor]) -> Drawing {
        let mut path = Drawing::zigzag_from(corners[0]);
        for corner in &corners[1..] {
            path.move_last_corner(*corner);
            path.add_corner(*corner);
        }
        path.finish_path();
        path
    }

    /// Laid down a corner at a time: the last one follows the hand until
    /// the next click pins it.
    #[test]
    fn a_zigzag_grows_a_corner_at_a_time() {
        let mut path = Drawing::zigzag_from(at(0, 100.0));
        assert_eq!(path.points.len(), 2, "it starts with the corner and the one following");
        path.move_last_corner(at(5, 120.0));
        path.add_corner(at(5, 120.0));
        path.move_last_corner(at(9, 90.0));
        assert_eq!(path.points.len(), 3);
        // The ends follow the path, so everything that wants a span works.
        assert_eq!(path.from, at(0, 100.0));
        assert_eq!(path.to, at(9, 90.0), "the end is the corner under the hand");
    }

    /// Finishing drops the corner that was only following the hand, and
    /// says whether what is left is a drawing at all.
    #[test]
    fn finishing_drops_the_corner_under_the_hand() {
        let path = drawn_through(&[at(0, 100.0), at(5, 120.0)]);
        assert_eq!(path.points, vec![at(0, 100.0), at(5, 120.0)]);
        assert_eq!(path.to, at(5, 120.0));

        // One click and nothing else is not: the second corner was never
        // anywhere but under the hand.
        let mut barely = Drawing::zigzag_from(at(0, 100.0));
        assert!(!barely.finish_path());
    }

    /// A grip on every corner, and moving one moves only that one.
    #[test]
    fn every_corner_has_a_grip_of_its_own() {
        let mut path = drawn_through(&[at(0, 100.0), at(5, 120.0), at(9, 90.0)]);
        assert_eq!(path.grips(), vec![Grip::Corner(0), Grip::Corner(1), Grip::Corner(2)]);
        path.move_grip(Grip::Corner(1), at(6, 130.0));
        assert_eq!(path.points[1], at(6, 130.0));
        assert_eq!(path.points[0], at(0, 100.0));
        // Moving an end carries the drawing's own end with it.
        path.move_grip(Grip::Corner(0), at(-2, 95.0));
        assert_eq!(path.from, at(-2, 95.0));
    }

    /// Picked anywhere along it, not only on the straight line between its
    /// ends — which for a zig-zag passes through nothing.
    #[test]
    fn a_zigzag_is_picked_on_any_of_its_segments() {
        let path = Projected::through(
            Kind::Zigzag,
            vec![(0.0, 0.0), (50.0, 100.0), (100.0, 0.0)],
        );
        assert_eq!(path.hit(25.0, 50.0), Some(Grip::Body), "the first leg missed");
        assert_eq!(path.hit(75.0, 50.0), Some(Grip::Body), "the second leg missed");
        // The straight line between the two ends runs along y = 0, where
        // there is nothing drawn but the ends themselves.
        assert_eq!(path.hit(50.0, 2.0), None, "it was picked where nothing is drawn");
        // Its box is the box round all of it, not round its ends.
        assert_eq!(path.bounds(), (0.0, 0.0, 100.0, 100.0));
        // And a box over the peak takes it.
        assert!(path.touches((40.0, 80.0, 20.0, 20.0)));
    }

    /// The whole of it moves together.
    #[test]
    fn shifting_moves_every_corner() {
        let mut path = drawn_through(&[at(0, 100.0), at(5, 120.0)]);
        path.shift(10, -5.0);
        assert_eq!(path.points, vec![at(10, 95.0), at(15, 115.0)]);
        assert_eq!(path.from, at(10, 95.0));
    }

    /// It points where it was drawn, out of the box.
    #[test]
    fn a_zigzag_ships_pointing() {
        let style = Style::zigzag(Preset::Blue);
        assert_eq!(style.arrow, Arrow::End);
        assert_eq!(style.head, ArrowHead::ALL[1]);
    }
}

#[cfg(test)]
mod edge_grip_tests {
    use super::*;

    fn boxed() -> Drawing {
        Drawing::new(Kind::Rect, Anchor::new(0, 100.0), Anchor::new(10, 80.0))
    }

    /// A box and an ellipse wear eight: four corners and the middle of each
    /// edge.
    #[test]
    fn a_bounded_figure_wears_eight_grips() {
        for kind in [Kind::Rect, Kind::Ellipse] {
            let drawing = Drawing::new(kind, Anchor::new(0, 1.0), Anchor::new(10, 2.0));
            assert_eq!(drawing.grips().len(), 8, "{}", kind.key());
        }
        let projected = Projected::new(Kind::Rect, (0.0, 0.0), (200.0, 100.0));
        assert_eq!(projected.grip(Grip::Top), Some((100.0, 0.0)));
        assert_eq!(projected.grip(Grip::Bottom), Some((100.0, 100.0)));
        assert_eq!(projected.grip(Grip::Left), Some((0.0, 50.0)));
        assert_eq!(projected.grip(Grip::Right), Some((200.0, 50.0)));
        // A stroke has none of them.
        let line = Projected::new(Kind::Line, (0.0, 0.0), (200.0, 100.0));
        assert_eq!(line.grip(Grip::Top), None);
    }

    /// An edge moves in one direction and holds still in the other.
    #[test]
    fn an_edge_grip_moves_one_side_only() {
        let mut drawing = boxed();
        // `from` is the higher price, so it is the top edge.
        drawing.move_grip(Grip::Top, Anchor::new(999, 120.0));
        assert_eq!(drawing.from, Anchor::new(0, 120.0), "the top edge took the moment too");
        assert_eq!(drawing.to, Anchor::new(10, 80.0), "the bottom edge moved");

        drawing.move_grip(Grip::Bottom, Anchor::new(999, 60.0));
        assert_eq!(drawing.to, Anchor::new(10, 60.0));
        assert_eq!(drawing.from, Anchor::new(0, 120.0));

        // `from` is the earlier moment, so it is the left edge.
        drawing.move_grip(Grip::Left, Anchor::new(-5, 999.0));
        assert_eq!(drawing.from, Anchor::new(-5, 120.0), "the left edge took the price too");
        drawing.move_grip(Grip::Right, Anchor::new(20, 999.0));
        assert_eq!(drawing.to, Anchor::new(20, 60.0));
    }

    /// Which anchor is on which side follows the shape, not the order it
    /// was drawn in: a box dragged out right-to-left has its edges where
    /// they look.
    #[test]
    fn the_edges_are_where_they_look_however_it_was_drawn() {
        let mut backwards = Drawing::new(Kind::Rect, Anchor::new(10, 80.0), Anchor::new(0, 100.0));
        backwards.move_grip(Grip::Top, Anchor::new(999, 120.0));
        // `to` carries the higher price here, so that is what moved.
        assert_eq!(backwards.to, Anchor::new(0, 120.0));
        assert_eq!(backwards.from, Anchor::new(10, 80.0));
        backwards.move_grip(Grip::Left, Anchor::new(-5, 999.0));
        assert_eq!(backwards.to, Anchor::new(-5, 120.0), "the earlier anchor is the left edge");
    }
}
