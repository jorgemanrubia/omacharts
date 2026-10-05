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
