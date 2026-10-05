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
    pub fn label(self) -> &'static str {
        match self {
            Kind::Line => "Line",
            Kind::Rect => "Rectangle",
        }
    }
}

/// Something a person drew on a symbol's chart.
///
/// Stored by symbol, not by pane: a trend line on AAPL is a fact about AAPL,
/// and it belongs on every chart of AAPL in every chartbook, at every
/// resolution. The colour is a [`Preset`] — a role the theme fills — so the
/// drawing follows the desktop theme the way the candles do.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Drawing {
    /// The store's row id. Zero until it has been written down.
    #[serde(default)]
    pub id: i64,
    pub kind: Kind,
    pub from: Anchor,
    pub to: Anchor,
    #[serde(default = "Drawing::default_preset")]
    pub preset: Preset,
    /// A line's stroke width. The chart's default when absent; a rectangle
    /// ignores it, since its border is a hairline by design.
    #[serde(default = "Drawing::default_width")]
    pub width: f64,
}

/// The width a line is drawn at when nobody chose one: the chart's own
/// default stroke.
pub const DEFAULT_WIDTH: f64 = 1.5;

impl Drawing {
    pub fn new(kind: Kind, from: Anchor, to: Anchor) -> Drawing {
        Drawing { id: 0, kind, from, to, preset: Preset::Blue, width: DEFAULT_WIDTH }
    }

    /// The preset a new drawing gets. Blue is the first colour the app hands
    /// out to anything, and it is nobody's candle.
    fn default_preset() -> Preset {
        Preset::Blue
    }

    fn default_width() -> f64 {
        DEFAULT_WIDTH
    }

    /// The anchor a grip stands for, to move it.
    pub fn anchor_mut(&mut self, grip: Grip) -> Option<&mut Anchor> {
        match grip {
            Grip::From => Some(&mut self.from),
            Grip::To => Some(&mut self.to),
            Grip::Body => None,
        }
    }

    /// Shift the whole drawing by a span of time and a difference in price.
    pub fn shift(&mut self, by_ts: i64, by_price: f64) {
        for anchor in [&mut self.from, &mut self.to] {
            anchor.ts += by_ts;
            anchor.price += by_price;
        }
    }
}

/// What part of a drawing the pointer is on.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grip {
    From,
    To,
    /// The line itself, or the inside of the box: drag to move the whole
    /// thing.
    Body,
}

/// How near the pointer has to be, in pixels, to pick a line or an edge.
pub const PICK_REACH: f64 = 6.0;
/// And to pick an anchor, which is a smaller target and worth more.
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
    /// What the pointer at (`x`, `y`) is on, if anything. Grips win over the
    /// body, because an anchor sits on the body and is the harder target.
    pub fn hit(&self, x: f64, y: f64) -> Option<Grip> {
        if distance(self.from, (x, y)) <= GRIP_REACH {
            return Some(Grip::From);
        }
        if distance(self.to, (x, y)) <= GRIP_REACH {
            return Some(Grip::To);
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
        drawing.preset = Preset::Amber;
        let json = serde_json::to_string(&drawing).unwrap();
        assert_eq!(serde_json::from_str::<Drawing>(&json).unwrap(), drawing);

        let bare = r#"{"kind":"line","from":{"ts":1,"price":2.0},"to":{"ts":3,"price":4.0}}"#;
        let old: Drawing = serde_json::from_str(bare).unwrap();
        assert_eq!(old.preset, Preset::Blue);
        assert_eq!(old.width, DEFAULT_WIDTH);
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
