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

/// The two kinds of drawing. Both are two anchors; what differs is what is
/// drawn between them.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    /// A straight line from one anchor to the other.
    Line,
    /// A box with the two anchors at opposite corners.
    Rect,
}

impl Kind {
    pub const ALL: [Kind; 2] = [Kind::Line, Kind::Rect];

    pub fn label(self) -> &'static str {
        match self {
            Kind::Line => "Line",
            Kind::Rect => "Rectangle",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Kind::Line => "line",
            Kind::Rect => "rect",
        }
    }

    pub fn from_key(key: &str) -> Option<Kind> {
        Kind::ALL.into_iter().find(|k| k.key().eq_ignore_ascii_case(key))
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
}

impl Style {
    /// The configuration a line starts from: a preset's colour at the
    /// chart's default stroke, no arrow.
    pub fn line(preset: Preset) -> Style {
        Style {
            colour: Paint::preset(preset),
            width: DEFAULT_WIDTH,
            arrow: Arrow::None,
            border: true,
            fill: Paint::preset(preset),
            alpha: FILL_ALPHA,
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
            border: true,
            fill: Paint::preset(preset),
            alpha: FILL_ALPHA,
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
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Configurations {
    pub line: Vec<Style>,
    pub rect: Vec<Style>,
}

/// How many configurations each kind has.
pub const CONFIGURATIONS: u8 = 9;

impl Default for Configurations {
    fn default() -> Configurations {
        Configurations {
            line: Preset::ALL.into_iter().map(Style::line).collect(),
            rect: Preset::ALL.into_iter().map(Style::rect).collect(),
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
        let list = match kind {
            Kind::Line => &mut self.line,
            Kind::Rect => &mut self.rect,
        };
        if let Some(slot) = list.get_mut(at) {
            *slot = style;
        }
    }

    /// Put one kind's nine back to what shipped.
    pub fn reset(&mut self, kind: Kind) {
        let fresh = Configurations::default();
        match kind {
            Kind::Line => self.line = fresh.line,
            Kind::Rect => self.rect = fresh.rect,
        }
    }

    pub fn list(&self, kind: Kind) -> &[Style] {
        let list = match kind {
            Kind::Line => &self.line,
            Kind::Rect => &self.rect,
        };
        // A stored list that is short — written by a build with fewer — is
        // read as if it were the defaults for the rest.
        if list.len() >= CONFIGURATIONS as usize {
            list
        } else {
            DEFAULTS.get_or_init(Configurations::default).list_or_default(kind)
        }
    }

    fn list_or_default(&self, kind: Kind) -> &[Style] {
        match kind {
            Kind::Line => &self.line,
            Kind::Rect => &self.rect,
        }
    }

    /// Whether configuration `n` of a kind is as shipped.
    pub fn is_default(&self, kind: Kind, n: u8) -> bool {
        DEFAULTS.get_or_init(Configurations::default).of(kind, n) == self.of(kind, n)
    }
}

static DEFAULTS: std::sync::OnceLock<Configurations> = std::sync::OnceLock::new();

/// Who a drawing is shared with.
///
/// Every chart showing the symbol sees a global drawing; the charts in group
/// N see group N's; a local drawing belongs to the chart it was drawn on and
/// to nothing else. Which of these a new drawing gets is the chart's own
/// setting, so a chart in group 3 draws into group 3 without being asked.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Scope {
    Local,
    #[default]
    Global,
    Group(u8),
}

impl Scope {
    pub fn key(self) -> String {
        match self {
            Scope::Local => "local".to_string(),
            Scope::Global => "global".to_string(),
            Scope::Group(n) => format!("group-{n}"),
        }
    }

    pub fn label(self) -> String {
        match self {
            Scope::Local => "This chart only".to_string(),
            Scope::Global => "Every chart".to_string(),
            Scope::Group(n) => format!("Drawing group {n}"),
        }
    }

    pub fn from_key(key: &str) -> Option<Scope> {
        match key.to_lowercase().as_str() {
            "local" => Some(Scope::Local),
            "global" => Some(Scope::Global),
            other => other
                .strip_prefix("group-")
                .and_then(|n| n.parse::<u8>().ok())
                .filter(|n| (1..=9).contains(n))
                .map(Scope::Group),
        }
    }

    /// Every scope a drawing can have, in the order a menu lists them.
    pub fn all() -> Vec<Scope> {
        let mut all = vec![Scope::Local, Scope::Global];
        all.extend((1..=9).map(Scope::Group));
        all
    }
}

impl Serialize for Scope {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.key())
    }
}

impl<'de> Deserialize<'de> for Scope {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Scope, D::Error> {
        let key = String::deserialize(d)?;
        Scope::from_key(&key).ok_or_else(|| serde::de::Error::custom(format!("{key:?} is not a drawing scope")))
    }
}

/// What a chart shares its drawings with: the global group, one of the nine,
/// or nothing, in which case what is drawn on it stays on it.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Sharing {
    #[default]
    Global,
    Group(u8),
    Off,
}

impl Sharing {
    pub fn key(self) -> String {
        match self {
            Sharing::Global => "global".to_string(),
            Sharing::Group(n) => format!("group-{n}"),
            Sharing::Off => "off".to_string(),
        }
    }

    pub fn label(self) -> String {
        match self {
            Sharing::Global => "Global drawing group".to_string(),
            Sharing::Group(n) => format!("Drawing group {n}"),
            Sharing::Off => "Not shared".to_string(),
        }
    }

    pub fn from_key(key: &str) -> Option<Sharing> {
        match key.to_lowercase().as_str() {
            "global" => Some(Sharing::Global),
            "off" | "none" => Some(Sharing::Off),
            other => other
                .strip_prefix("group-")
                .and_then(|n| n.parse::<u8>().ok())
                .filter(|n| (1..=9).contains(n))
                .map(Sharing::Group),
        }
    }

    pub fn all() -> Vec<Sharing> {
        let mut all = vec![Sharing::Global];
        all.extend((1..=9).map(Sharing::Group));
        all.push(Sharing::Off);
        all
    }

    /// The scope a drawing made on a chart with this sharing gets.
    pub fn scope_for_new(self) -> Scope {
        match self {
            Sharing::Global => Scope::Global,
            Sharing::Group(n) => Scope::Group(n),
            Sharing::Off => Scope::Local,
        }
    }

    /// Whether a chart with this sharing shows a shared drawing of its
    /// symbol. Local drawings are not shared and are not asked.
    pub fn shows(self, scope: Scope) -> bool {
        match (self, scope) {
            (_, Scope::Local) => false,
            (Sharing::Off, _) => false,
            (Sharing::Global, Scope::Global) => true,
            (Sharing::Global, Scope::Group(_)) => false,
            (Sharing::Group(_), Scope::Global) => true,
            (Sharing::Group(mine), Scope::Group(theirs)) => mine == theirs,
        }
    }
}

impl Serialize for Sharing {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.key())
    }
}

impl<'de> Deserialize<'de> for Sharing {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Sharing, D::Error> {
        let key = String::deserialize(d)?;
        Sharing::from_key(&key).ok_or_else(|| serde::de::Error::custom(format!("{key:?} is not a sharing setting")))
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
    #[serde(default)]
    pub scope: Scope,
    /// Where it sits among the others: higher is nearer the front. All of
    /// them sit over the candles; this only orders drawings among
    /// themselves, for "bring to front" and "send to back".
    #[serde(default)]
    pub order: i64,
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
        Drawing { id: 0, kind, from, to, config: Some(1), style: None, scope: Scope::Global, order: 0 }
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
        self.config = None;
        self.style = Some(own);
    }

    /// Follow configuration `n` again.
    pub fn follow(&mut self, n: u8) {
        self.config = Some(n.clamp(1, CONFIGURATIONS));
        self.style = None;
    }

    /// Whether this drawing lives in the store (shared) rather than with a
    /// chart (local).
    pub fn is_local(&self) -> bool {
        self.scope == Scope::Local
    }

    /// The anchor a grip stands for, to move it, for the grips that are an
    /// anchor. A rectangle's other two corners are half of each.
    pub fn anchor_mut(&mut self, grip: Grip) -> Option<&mut Anchor> {
        match grip {
            Grip::From => Some(&mut self.from),
            Grip::To => Some(&mut self.to),
            Grip::FromTo | Grip::ToFrom | Grip::Body => None,
        }
    }

    /// Put the grip where the hand is.
    pub fn move_grip(&mut self, grip: Grip, at: Anchor) {
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
            Grip::Body => {}
        }
    }

    /// Shift the whole drawing by a span of time and a difference in price.
    pub fn shift(&mut self, by_ts: i64, by_price: f64) {
        for anchor in [&mut self.from, &mut self.to] {
            anchor.ts += by_ts;
            anchor.price += by_price;
        }
    }

    /// The grips this kind of drawing wears when selected.
    pub fn grips(&self) -> &'static [Grip] {
        match self.kind {
            Kind::Line => &[Grip::From, Grip::To],
            Kind::Rect => &[Grip::From, Grip::To, Grip::FromTo, Grip::ToFrom],
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
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Projected {
    pub kind: Kind,
    pub from: (f64, f64),
    pub to: (f64, f64),
}

impl Projected {
    /// Where a grip is on screen.
    pub fn grip(&self, grip: Grip) -> Option<(f64, f64)> {
        match (self.kind, grip) {
            (_, Grip::From) => Some(self.from),
            (_, Grip::To) => Some(self.to),
            (Kind::Rect, Grip::FromTo) => Some((self.from.0, self.to.1)),
            (Kind::Rect, Grip::ToFrom) => Some((self.to.0, self.from.1)),
            _ => None,
        }
    }

    /// What the pointer at (`x`, `y`) is on, if anything. Grips win over the
    /// body, because a grip sits on the body and is the harder target.
    pub fn hit(&self, x: f64, y: f64) -> Option<Grip> {
        for grip in [Grip::From, Grip::To, Grip::FromTo, Grip::ToFrom] {
            if let Some(at) = self.grip(grip)
                && distance(at, (x, y)) <= GRIP_REACH
            {
                return Some(grip);
            }
        }
        let on_body = match self.kind {
            Kind::Line => distance_to_segment((x, y), self.from, self.to) <= PICK_REACH,
            Kind::Rect => {
                let (left, right) = ordered(self.from.0, self.to.0);
                let (top, bottom) = ordered(self.from.1, self.to.1);
                let reach = PICK_REACH;
                (left - reach..=right + reach).contains(&x) && (top - reach..=bottom + reach).contains(&y)
            }
        };
        on_body.then_some(Grip::Body)
    }

    /// Whether any of the drawing lies inside a box: the box a hand drags
    /// out to select what it touches. A line counts when the segment
    /// crosses the box, not when its own bounding box does, so a long
    /// diagonal is not taken by a box in the empty corner beside it.
    pub fn touches(&self, (left, top, width, height): (f64, f64, f64, f64)) -> bool {
        let (right, bottom) = (left + width, top + height);
        match self.kind {
            Kind::Line => segment_meets_box(self.from, self.to, left, top, right, bottom),
            Kind::Rect => {
                let (l, r) = ordered(self.from.0, self.to.0);
                let (t, b) = ordered(self.from.1, self.to.1);
                l <= right && r >= left && t <= bottom && b >= top
            }
        }
    }

    /// The box's corners as (left, top, width, height), for drawing it.
    pub fn bounds(&self) -> (f64, f64, f64, f64) {
        let (left, right) = ordered(self.from.0, self.to.0);
        let (top, bottom) = ordered(self.from.1, self.to.1);
        (left, top, right - left, bottom - top)
    }
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
        let diagonal = Projected { kind: Kind::Line, from: (0.0, 0.0), to: (100.0, 100.0) };
        assert!(diagonal.touches((40.0, 40.0, 20.0, 20.0)));
        assert!(diagonal.touches((90.0, 50.0, 30.0, 60.0)));
        assert!(!diagonal.touches((60.0, 0.0, 30.0, 30.0)));
        assert!(!diagonal.touches((120.0, 120.0, 10.0, 10.0)));
        let flat = Projected { kind: Kind::Line, from: (10.0, 50.0), to: (90.0, 50.0) };
        assert!(flat.touches((0.0, 40.0, 20.0, 20.0)));
        assert!(!flat.touches((0.0, 60.0, 200.0, 20.0)));
        let rect = Projected { kind: Kind::Rect, from: (10.0, 10.0), to: (50.0, 50.0) };
        assert!(rect.touches((45.0, 45.0, 20.0, 20.0)));
        assert!(rect.touches((20.0, 20.0, 5.0, 5.0)));
        assert!(!rect.touches((51.0, 0.0, 20.0, 20.0)));
    }

    fn line() -> Projected {
        Projected { kind: Kind::Line, from: (100.0, 100.0), to: (300.0, 200.0) }
    }

    fn rect() -> Projected {
        Projected { kind: Kind::Rect, from: (300.0, 200.0), to: (100.0, 100.0) }
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
        drawing.scope = Scope::Group(3);
        let json = serde_json::to_string(&drawing).unwrap();
        assert!(json.contains("\"scope\":\"group-3\""), "{json}");
        assert_eq!(serde_json::from_str::<Drawing>(&json).unwrap(), drawing);

        let bare = r#"{"kind":"line","from":{"ts":1,"price":2.0},"to":{"ts":3,"price":4.0}}"#;
        let old: Drawing = serde_json::from_str(bare).unwrap();
        assert_eq!(old.config, Some(1));
        assert_eq!(old.style, None);
        assert_eq!(old.scope, Scope::Global);
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

    #[test]
    fn sharing_decides_what_a_chart_shows_and_what_it_draws_into() {
        assert!(Sharing::Global.shows(Scope::Global));
        assert!(!Sharing::Global.shows(Scope::Group(2)));
        assert!(Sharing::Group(2).shows(Scope::Global));
        assert!(Sharing::Group(2).shows(Scope::Group(2)));
        assert!(!Sharing::Group(2).shows(Scope::Group(3)));
        assert!(!Sharing::Off.shows(Scope::Global));
        assert!(!Sharing::Group(1).shows(Scope::Local), "local is never shared");
        assert_eq!(Sharing::Off.scope_for_new(), Scope::Local);
        assert_eq!(Sharing::Group(5).scope_for_new(), Scope::Group(5));
        for scope in Scope::all() {
            assert_eq!(Scope::from_key(&scope.key()), Some(scope));
        }
        for sharing in Sharing::all() {
            assert_eq!(Sharing::from_key(&sharing.key()), Some(sharing));
        }
        assert_eq!(Scope::from_key("group-0"), None);
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
    fn every_theme() -> Vec<Theme> {
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
