//! Themes, bar schemes, and the swatch palettes they carry.
//!
//! Three things are chosen independently, because people want them
//! independently:
//!
//! * a **[`Theme`]** — the whole app's look: surfaces, text, chart structure,
//!   accent. On Omarchy this follows the desktop by default.
//! * a **[`BarScheme`]** — what candles look like. Classic green and red is
//!   one option among several; monochrome is another.
//! * a **[`Swatch`]** from the active theme's palette, whenever a colour is
//!   picked for an indicator or overlay.
//!
//! Every theme ships a named palette so picking an indicator colour means
//! choosing from a handful that already look right together, rather than
//! hunting in a colour wheel. Picking from the wheel stays possible — see
//! [`ColorChoice`].

use serde::{Deserialize, Serialize};

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

/// Where a theme or bar scheme came from. Built-ins and the Omarchy theme can
/// be duplicated but not edited in place; custom ones can be edited and
/// deleted.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    BuiltIn,
    /// Derived from the live Omarchy palette; follows it as it changes.
    Omarchy,
    Custom,
}

impl Source {
    pub fn is_editable(self) -> bool {
        matches!(self, Source::Custom)
    }
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
    fn new(name: &str, hex: &str) -> Swatch {
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
            ColorChoice::Swatch { name } => theme
                .swatch(name)
                .map(|s| s.hex.clone())
                .unwrap_or_else(|| theme.ui.accent.clone()),
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
}

impl Theme {
    pub fn swatch(&self, name: &str) -> Option<&Swatch> {
        self.swatches.iter().find(|s| s.name == name)
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

    pub fn duplicate(&self, id: impl Into<String>, name: impl Into<String>) -> Theme {
        Theme {
            id: id.into(),
            name: name.into(),
            source: Source::Custom,
            ..self.clone()
        }
    }
}

/// One editable colour of a theme, so the settings page can be generic: it
/// walks `ALL`, groups by [`UiSlot::group`], and needs no edit when a colour
/// is added here.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum UiSlot {
    Background,
    Surface,
    SurfaceVariant,
    Border,
    Text,
    TextMuted,
    Grid,
    Axis,
    Crosshair,
    Accent,
}

impl UiSlot {
    pub const ALL: [UiSlot; 10] = [
        UiSlot::Background,
        UiSlot::Surface,
        UiSlot::SurfaceVariant,
        UiSlot::Border,
        UiSlot::Text,
        UiSlot::TextMuted,
        UiSlot::Grid,
        UiSlot::Axis,
        UiSlot::Crosshair,
        UiSlot::Accent,
    ];

    pub const GROUPS: [&'static str; 4] = ["Surfaces", "Text", "Chart structure", "Accent"];

    pub fn label(self) -> &'static str {
        match self {
            UiSlot::Background => "Chart background",
            UiSlot::Surface => "Panels",
            UiSlot::SurfaceVariant => "Toolbars",
            UiSlot::Border => "Borders",
            UiSlot::Text => "Text",
            UiSlot::TextMuted => "Secondary text",
            UiSlot::Grid => "Grid lines",
            UiSlot::Axis => "Axes",
            UiSlot::Crosshair => "Crosshair",
            UiSlot::Accent => "Accent",
        }
    }

    pub fn group(self) -> &'static str {
        match self {
            UiSlot::Background | UiSlot::Surface | UiSlot::SurfaceVariant | UiSlot::Border => {
                "Surfaces"
            }
            UiSlot::Text | UiSlot::TextMuted => "Text",
            UiSlot::Grid | UiSlot::Axis | UiSlot::Crosshair => "Chart structure",
            UiSlot::Accent => "Accent",
        }
    }

    pub fn get(self, c: &UiColors) -> &str {
        match self {
            UiSlot::Background => &c.background,
            UiSlot::Surface => &c.surface,
            UiSlot::SurfaceVariant => &c.surface_variant,
            UiSlot::Border => &c.border,
            UiSlot::Text => &c.text,
            UiSlot::TextMuted => &c.text_muted,
            UiSlot::Grid => &c.grid,
            UiSlot::Axis => &c.axis,
            UiSlot::Crosshair => &c.crosshair,
            UiSlot::Accent => &c.accent,
        }
    }

    pub fn set(self, c: &mut UiColors, hex: String) {
        let field = match self {
            UiSlot::Background => &mut c.background,
            UiSlot::Surface => &mut c.surface,
            UiSlot::SurfaceVariant => &mut c.surface_variant,
            UiSlot::Border => &mut c.border,
            UiSlot::Text => &mut c.text,
            UiSlot::TextMuted => &mut c.text_muted,
            UiSlot::Grid => &mut c.grid,
            UiSlot::Axis => &mut c.axis,
            UiSlot::Crosshair => &mut c.crosshair,
            UiSlot::Accent => &mut c.accent,
        };
        *field = hex;
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

/// What candles look like, chosen independently of the theme.
///
/// Outline and body are separate so a scheme can be hollow (body == chart
/// background) or solid (body == outline) with no other machinery.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct BarScheme {
    pub id: String,
    pub name: String,
    pub source: Source,
    pub up: String,
    pub up_fill: String,
    pub down: String,
    pub down_fill: String,
    pub volume_up: String,
    pub volume_down: String,
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

    /// The body colour for a direction. Fully transparent means hollow.
    pub fn body(&self, direction: Direction) -> &str {
        match direction {
            Direction::Up => &self.up_fill,
            Direction::Down => &self.down_fill,
            Direction::Flat => &self.neutral,
        }
    }

    /// The volume colour for a direction.
    pub fn volume(&self, direction: Direction) -> &str {
        match direction {
            Direction::Up => &self.volume_up,
            Direction::Down => &self.volume_down,
            Direction::Flat => &self.neutral,
        }
    }

    pub fn duplicate(&self, id: impl Into<String>, name: impl Into<String>) -> BarScheme {
        BarScheme {
            id: id.into(),
            name: name.into(),
            source: Source::Custom,
            ..self.clone()
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BarSlot {
    Up,
    UpFill,
    Down,
    DownFill,
    VolumeUp,
    VolumeDown,
    Neutral,
}

impl BarSlot {
    pub const ALL: [BarSlot; 7] = [
        BarSlot::Up,
        BarSlot::UpFill,
        BarSlot::Down,
        BarSlot::DownFill,
        BarSlot::VolumeUp,
        BarSlot::VolumeDown,
        BarSlot::Neutral,
    ];

    pub const GROUPS: [&'static str; 3] = ["Candles", "Volume", "Other"];

    pub fn label(self) -> &'static str {
        match self {
            BarSlot::Up => "Up outline",
            BarSlot::UpFill => "Up body",
            BarSlot::Down => "Down outline",
            BarSlot::DownFill => "Down body",
            BarSlot::VolumeUp => "Up volume",
            BarSlot::VolumeDown => "Down volume",
            BarSlot::Neutral => "Unchanged",
        }
    }

    pub fn group(self) -> &'static str {
        match self {
            BarSlot::Up | BarSlot::UpFill | BarSlot::Down | BarSlot::DownFill => "Candles",
            BarSlot::VolumeUp | BarSlot::VolumeDown => "Volume",
            BarSlot::Neutral => "Other",
        }
    }

    pub fn get(self, s: &BarScheme) -> &str {
        match self {
            BarSlot::Up => &s.up,
            BarSlot::UpFill => &s.up_fill,
            BarSlot::Down => &s.down,
            BarSlot::DownFill => &s.down_fill,
            BarSlot::VolumeUp => &s.volume_up,
            BarSlot::VolumeDown => &s.volume_down,
            BarSlot::Neutral => &s.neutral,
        }
    }

    pub fn set(self, s: &mut BarScheme, hex: String) {
        let field = match self {
            BarSlot::Up => &mut s.up,
            BarSlot::UpFill => &mut s.up_fill,
            BarSlot::Down => &mut s.down,
            BarSlot::DownFill => &mut s.down_fill,
            BarSlot::VolumeUp => &mut s.volume_up,
            BarSlot::VolumeDown => &mut s.volume_down,
            BarSlot::Neutral => &mut s.neutral,
        };
        *field = hex;
    }
}

// ---------------------------------------------------------------------------
// What we ship
// ---------------------------------------------------------------------------

/// The theme that follows the desktop. Default wherever Omarchy is present.
pub const OMARCHY_ID: &str = "omarchy";
/// The bar scheme that takes its colours from the active theme's palette.
/// Default, so candles match the desktop too.
pub const THEME_BARS_ID: &str = "theme";
/// The same, with the direction colours spent: one neutral for every bar.
pub const THEME_MONO_ID: &str = "theme-mono";
/// The same, the other way round: red for a rise, green for a fall.
pub const THEME_RED_UP_ID: &str = "theme-red-up";
/// Fallback theme when Omarchy is not installed.
pub const FALLBACK_THEME_ID: &str = "midnight";

pub fn builtin_themes() -> Vec<Theme> {
    vec![midnight(), carbon(), paper(), daylight()]
}

/// The fixed schemes. [`theme_bars`] is offered alongside these but built
/// per-theme, so it is not in the list.
pub fn builtin_bar_schemes() -> Vec<BarScheme> {
    vec![classic(), monochrome(), accessible(), hollow()]
}

fn swatches(hexes: [&str; 8]) -> Vec<Swatch> {
    SWATCH_NAMES
        .iter()
        .zip(hexes)
        .map(|(name, hex)| Swatch::new(name, hex))
        .collect()
}

/// Deep slate blue. The fallback default, and the house look.
fn midnight() -> Theme {
    Theme {
        id: FALLBACK_THEME_ID.into(),
        name: "Midnight".into(),
        mode: Mode::Dark,
        source: Source::BuiltIn,
        ui: UiColors {
            background: "#0d1117".into(),
            surface: "#151b23".into(),
            surface_variant: "#1c232c".into(),
            border: "#2a323d".into(),
            text: "#e6edf3".into(),
            text_muted: "#8b949e".into(),
            grid: "#1a2029".into(),
            axis: "#30363d".into(),
            crosshair: "#58a6ff".into(),
            accent: "#58a6ff".into(),
        },
        swatches: swatches([
            "#58a6ff", "#d29922", "#bc8cff", "#39c5cf", "#ff7b72", "#3fb950", "#ff9b50", "#76e4f7",
        ]),
    }
}

/// Neutral greys with GNOME's own accent, for an app that disappears into the
/// desktop.
fn carbon() -> Theme {
    Theme {
        id: "carbon".into(),
        name: "Carbon".into(),
        mode: Mode::Dark,
        source: Source::BuiltIn,
        ui: UiColors {
            background: "#121212".into(),
            surface: "#1a1a1a".into(),
            surface_variant: "#222222".into(),
            border: "#2e2e2e".into(),
            text: "#ededed".into(),
            text_muted: "#8f8f8f".into(),
            grid: "#1d1d1d".into(),
            axis: "#333333".into(),
            crosshair: "#d0d0d0".into(),
            accent: "#3584e4".into(),
        },
        swatches: swatches([
            "#3584e4", "#f5c211", "#9141ac", "#33d17a", "#e01b24", "#26a269", "#ff7800", "#62a0ea",
        ]),
    }
}

/// Warm light: off-white stock, ink text. Easy in daylight.
fn paper() -> Theme {
    Theme {
        id: "paper".into(),
        name: "Paper".into(),
        mode: Mode::Light,
        source: Source::BuiltIn,
        ui: UiColors {
            background: "#fbf8f3".into(),
            surface: "#f4efe7".into(),
            surface_variant: "#ebe4d8".into(),
            border: "#ddd3c3".into(),
            text: "#2d2a26".into(),
            text_muted: "#7a7268".into(),
            grid: "#efe9de".into(),
            axis: "#c9bfae".into(),
            crosshair: "#a85b2a".into(),
            accent: "#a85b2a".into(),
        },
        // Teal is darker than the rest on purpose. sRGB is at its narrowest
        // between green and blue, so a teal and a cyan at the lightness
        // everything else sits at cannot be given enough chroma to tell apart
        // — the old #0f766e and Cyan read as one colour, and so did it and
        // Green. Lightness is the only axis left, and spending it downwards
        // buys contrast on pale stock rather than costing it.
        swatches: swatches([
            "#1f6feb", "#b8860b", "#7d3c98", "#0d665f", "#b03a2e", "#2f7d52", "#c2601c", "#1f7a8c",
        ]),
    }
}

/// Crisp light: white stock, cool neutrals.
fn daylight() -> Theme {
    Theme {
        id: "daylight".into(),
        name: "Daylight".into(),
        mode: Mode::Light,
        source: Source::BuiltIn,
        ui: UiColors {
            background: "#ffffff".into(),
            surface: "#f6f8fa".into(),
            surface_variant: "#eef1f4".into(),
            border: "#d8dee4".into(),
            text: "#1f2328".into(),
            text_muted: "#656d76".into(),
            grid: "#f0f3f6".into(),
            axis: "#c9d1d9".into(),
            crosshair: "#0969da".into(),
            accent: "#0969da".into(),
        },
        // Teal is darker than the rest for the reason Paper's is: at a shared
        // lightness the gamut has no chroma to spare between green and blue,
        // and the old #1b7c83 was a cyan with the saturation turned down.
        swatches: swatches([
            "#0969da", "#9a6700", "#8250df", "#0f6a71", "#cf222e", "#1a7f37", "#bc4c00", "#0a7ea4",
        ]),
    }
}

/// Candles built from the active theme's own palette.
///
/// The default, and what makes the chart match the desktop on Omarchy: the
/// theme supplies green and rose, so candles shift with it instead of staying
/// a fixed pair of hexes that clash with half the themes available.
pub fn theme_bars(theme: &Theme) -> BarScheme {
    let up = theme
        .swatch("Green")
        .map(|s| s.hex.clone())
        .unwrap_or_else(|| "#3fb950".to_string());
    let down = theme
        .swatch("Rose")
        .map(|s| s.hex.clone())
        .unwrap_or_else(|| "#f85149".to_string());
    BarScheme {
        id: THEME_BARS_ID.to_string(),
        name: "Theme".to_string(),
        source: Source::BuiltIn,
        volume_up: mix(&up, &theme.ui.background, 0.45),
        volume_down: mix(&down, &theme.ui.background, 0.45),
        up_fill: up.clone(),
        down_fill: down.clone(),
        up,
        down,
        neutral: theme.ui.text_muted.clone(),
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

/// The theme's own bar colours with up and down exchanged.
///
/// Green for a rise is a Western habit, not a fact. Taiwan, mainland China,
/// Japan and Korea read a chart the other way round — red is up, green is
/// down — and a chart that disagrees with every other screen somebody trades
/// from is one they misread. Built from the theme like [`theme_bars`], so it
/// follows the desktop theme the same way, and it is a scheme rather than a
/// switch laid over every scheme: what the scheme list previews is what the
/// chart paints.
pub fn theme_red_up_bars(theme: &Theme) -> BarScheme {
    let bars = theme_bars(theme);
    BarScheme {
        id: THEME_RED_UP_ID.to_string(),
        name: "Red up".to_string(),
        up: bars.down.clone(),
        up_fill: bars.down_fill.clone(),
        down: bars.up.clone(),
        down_fill: bars.up_fill.clone(),
        volume_up: bars.volume_down.clone(),
        volume_down: bars.volume_up.clone(),
        ..bars
    }
}

/// Candles in one neutral colour, for people who would rather read a chart
/// than be signalled at by one.
///
/// Direction is simply not shown. That is the whole of the trade and it is
/// deliberate: the alternative — hollow for up, filled for down — keeps the
/// information but costs the quiet, and it vanishes anyway below about three
/// pixels a bar, where candles are drawn as hairlines with no body to fill.
/// An OHLC chart loses nothing at all, since its open and close ticks say
/// which way the bar went without reference to colour.
pub fn theme_mono_bars(theme: &Theme) -> BarScheme {
    let coloured = theme_bars(theme);
    let ink = held_to(&theme.ui.text, &theme.ui.background, MONO_CONTRAST, Some(MONO_CHROMA));
    let ink = set_apart(
        &ink,
        &[&coloured.up, &coloured.down],
        &theme.ui.background,
        MONO_CONTRAST,
        MONO_APART,
    );
    BarScheme {
        id: THEME_MONO_ID.to_string(),
        name: "Monochrome".to_string(),
        source: Source::BuiltIn,
        // The same dimming the coloured scheme gives volume, so the two read
        // as the same chart with the colour taken out rather than as two
        // different ones.
        volume_up: mix(&ink, &theme.ui.background, 0.45),
        volume_down: mix(&ink, &theme.ui.background, 0.45),
        up: ink.clone(),
        up_fill: ink.clone(),
        down: ink.clone(),
        down_fill: ink.clone(),
        neutral: ink,
    }
}

/// Blend `a` toward `b`. `t` of 0 is all `a`, 1 is all `b`.
///
/// Volume bars want to be the candle colour pushed most of the way into the
/// background so they read as a dimmer echo rather than a second signal.
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
const LIGHTNESS_STEP: f64 = 0.005;

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

/// Green up, red down. What most people expect.
fn classic() -> BarScheme {
    BarScheme {
        id: "classic".into(),
        name: "Classic".into(),
        source: Source::BuiltIn,
        up: "#3fb950".into(),
        up_fill: "#3fb950".into(),
        down: "#f85149".into(),
        down_fill: "#f85149".into(),
        volume_up: "#2d6e3b".into(),
        volume_down: "#8c322e".into(),
        neutral: "#8b949e".into(),
    }
}

/// No colour at all: light grey throughout, direction read from whether the
/// body is filled. Quiet, and it never fights the theme.
fn monochrome() -> BarScheme {
    BarScheme {
        id: "monochrome".into(),
        name: "Monochrome".into(),
        source: Source::BuiltIn,
        up: "#c9d1d9".into(),
        up_fill: "#00000000".into(),
        down: "#c9d1d9".into(),
        down_fill: "#c9d1d9".into(),
        volume_up: "#555f6a".into(),
        volume_down: "#8b949e".into(),
        neutral: "#8b949e".into(),
    }
}

/// Blue and orange rather than green and red, which the most common forms of
/// colour blindness cannot separate.
fn accessible() -> BarScheme {
    BarScheme {
        id: "accessible".into(),
        name: "Blue / Orange".into(),
        source: Source::BuiltIn,
        up: "#4493f8".into(),
        up_fill: "#4493f8".into(),
        down: "#ff9b50".into(),
        down_fill: "#ff9b50".into(),
        volume_up: "#2a5a8f".into(),
        volume_down: "#8f5a2a".into(),
        neutral: "#8b949e".into(),
    }
}

/// Outlined up, solid down — the convention that keeps charts airy.
fn hollow() -> BarScheme {
    BarScheme {
        id: "hollow".into(),
        name: "Hollow".into(),
        source: Source::BuiltIn,
        up: "#3fb950".into(),
        up_fill: "#00000000".into(),
        down: "#f85149".into(),
        down_fill: "#f85149".into(),
        volume_up: "#2d6e3b".into(),
        volume_down: "#8c322e".into(),
        neutral: "#8b949e".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(choice.resolve(&midnight()), "#d29922");
        assert_eq!(choice.resolve(&paper()), "#b8860b");
    }

    #[test]
    fn a_fixed_choice_ignores_the_theme() {
        let choice = ColorChoice::Fixed { hex: "#abcdef".into() };
        assert_eq!(choice.resolve(&midnight()), "#abcdef");
        assert_eq!(choice.resolve(&paper()), "#abcdef");
    }

    #[test]
    fn an_unknown_swatch_falls_back_to_the_accent() {
        let choice = ColorChoice::swatch("Chartreuse");
        assert_eq!(choice.resolve(&midnight()), midnight().ui.accent);
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
        let theme = midnight();
        let amber = theme.swatch("Amber").unwrap().hex.clone();
        let near = mix(&amber, &theme.ui.background, 0.05);
        let companion = theme.companion(&near);
        assert!(theme.swatches.iter().any(|s| s.hex == companion), "{companion} is not a swatch");
        assert!(delta_e(&near, &companion) >= COMPANION_GAP);
    }

    #[test]
    fn series_colours_wrap_without_panicking() {
        let theme = midnight();
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

    #[test]
    fn consecutive_overlay_colours_are_far_apart() {
        // Consecutive overlays are the ones most likely to overlap, so they
        // must be separable at a glance in every theme we ship.
        for theme in builtin_themes() {
            for n in 0..SWATCH_SEQUENCE.len() - 1 {
                let (a, b) = (theme.series(n), theme.series(n + 1));
                let distance = rgb_distance(&a, &b);
                assert!(distance > 0.25, "{} {n}: {a} vs {b} ({distance:.3})", theme.name);
            }
        }
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
        let theme = midnight();
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
    fn slots_round_trip() {
        let mut theme = midnight();
        for slot in UiSlot::ALL {
            slot.set(&mut theme.ui, "#123456".into());
            assert_eq!(slot.get(&theme.ui), "#123456");
        }
        let mut bars = classic();
        for slot in BarSlot::ALL {
            slot.set(&mut bars, "#654321".into());
            assert_eq!(slot.get(&bars), "#654321");
        }
    }


    #[test]
    fn theme_bars_take_their_colours_from_the_theme() {
        let theme = paper();
        let bars = theme_bars(&theme);
        assert_eq!(bars.up, theme.swatch("Green").unwrap().hex);
        assert_eq!(bars.down, theme.swatch("Rose").unwrap().hex);
        assert_eq!(bars.id, THEME_BARS_ID);
    }

    #[test]
    fn red_up_is_the_theme_colours_the_other_way_round() {
        for theme in builtin_themes() {
            let (green, red) = (theme_bars(&theme), theme_red_up_bars(&theme));
            assert_eq!((&red.up, &red.down), (&green.down, &green.up), "{}", theme.name);
            assert_eq!((&red.up_fill, &red.down_fill), (&green.down_fill, &green.up_fill));
            assert_eq!((&red.volume_up, &red.volume_down), (&green.volume_down, &green.volume_up));
            assert_eq!(red.neutral, green.neutral, "unchanged stays unchanged");
            assert_eq!(red.id, THEME_RED_UP_ID);
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
            assert_eq!(mono.volume_up, mono.volume_down, "{}", theme.name);
            assert_eq!(mono.id, THEME_MONO_ID, "{}", theme.name);
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
    fn volume_sits_between_the_candle_and_the_background() {
        let theme = midnight();
        let bars = theme_bars(&theme);
        assert_ne!(bars.volume_up, bars.up);
        assert_ne!(bars.volume_up, theme.ui.background);
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
    fn every_scheme_answers_for_every_direction() {
        let mut schemes = builtin_bar_schemes();
        schemes.push(theme_bars(&midnight()));
        for scheme in schemes {
            for direction in [Direction::Up, Direction::Down, Direction::Flat] {
                assert!(!scheme.outline(direction).is_empty(), "{}", scheme.name);
                assert!(!scheme.body(direction).is_empty(), "{}", scheme.name);
                assert!(!scheme.volume(direction).is_empty(), "{}", scheme.name);
            }
        }
    }

    #[test]
    fn up_and_down_are_never_the_same_colour() {
        // Monochrome is the deliberate exception: it separates direction by
        // whether the body is filled, not by hue.
        for scheme in builtin_bar_schemes() {
            if scheme.id == "monochrome" {
                assert_ne!(scheme.body(Direction::Up), scheme.body(Direction::Down));
                continue;
            }
            assert_ne!(
                scheme.outline(Direction::Up),
                scheme.outline(Direction::Down),
                "{}",
                scheme.name
            );
        }
    }

    #[test]
    fn the_default_scheme_is_green_up_red_down() {
        // "Typically red/green" is what people expect, and the theme palette
        // is where those two live.
        let bars = theme_bars(&midnight());
        assert_eq!(bars.outline(Direction::Up), midnight().swatch("Green").unwrap().hex);
        assert_eq!(bars.outline(Direction::Down), midnight().swatch("Rose").unwrap().hex);
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

    #[test]
    fn only_custom_things_are_editable() {
        assert!(!Source::BuiltIn.is_editable());
        assert!(!Source::Omarchy.is_editable());
        assert!(Source::Custom.is_editable());
    }
}
