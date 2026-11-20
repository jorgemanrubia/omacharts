//! Every colour the palette offers, and every quiet variant of it a chart
//! draws by default, can be seen on every theme that ships.
//!
//! A palette name is a promise that holds across themes: "Neutral" picked on
//! a dark theme is near white there and near black on a light one, because
//! the name is stored and resolved again against whichever theme is active.
//! What this holds is the other half of the promise — that what a name
//! resolves to is never a mark the chart's own background swallows. The
//! themes are the built-in ones and every Omarchy theme in the fixtures,
//! light and dark alike.
//!
//! Two floors, because a chart draws a colour two ways:
//!
//! * **As a mark** — a line, an outline, a point of control, a market
//!   profile's open and close arrows. Held to WCAG 2's 3:1 for graphical
//!   objects that carry meaning (success criterion 1.4.11), which is
//!   [`MARK_CONTRAST`], the floor the candles are held to as well. A line is
//!   thin, and contrast in luminance is what lets a thin thing be found;
//!   hue alone does not.
//! * **As a fill** — a volume profile's value area, drawn at
//!   [`DESIGNED_ALPHA`] over the background. A wash at 30% cannot reach 3:1
//!   and is not meant to: it is an area, and an area is seen by how different
//!   it looks from its ground, not by edge contrast. So it is held to a
//!   distance in OKLab, [`FILL_DISTANCE`], the same scale the palette's own
//!   thresholds are written in. 0.02 is about the least the eye can separate;
//!   0.08 is the distance at which the palette holds two lines apart at a
//!   glance, and is what a filled profile has to keep from the chart behind
//!   it. A near-white fill on a cream chart, the case this test was written
//!   for, is about 0.01.
//!
//! The rows outside the value area are drawn weaker still, on purpose, and
//! are not held to anything: they recede so the value area stands out.
//!
//! Text is not tested here: whatever writes text in a palette colour holds
//! it to 4.5:1 against its ground first (see `drawings::text_colour` and
//! `ink::legible`), so a swatch never has to be legible as text by itself.

use std::path::Path;

use omacharts_engine::drawings;
use omacharts_engine::indicators::profile::DESIGNED_ALPHA;
use omacharts_engine::ink::MARK_CONTRAST;
use omacharts_engine::theme::{builtin_themes, contrast_ratio, delta_e, mix, Theme, PALETTE_NAMES};
use omacharts_engine::{omarchy, ColorChoice};

/// How far a filled profile's value area has to sit from the background, in
/// OKLab. See the module's comment.
const FILL_DISTANCE: f64 = 0.08;

/// What the Neutral keeps from the background. See its test.
const NEUTRAL_CONTRAST: f64 = 7.0;

/// The built-in themes, and every Omarchy theme the fixtures hold.
fn shipped() -> Vec<(String, Theme)> {
    let mut themes: Vec<(String, Theme)> = builtin_themes().into_iter().map(|t| (t.name.clone(), t)).collect();
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/omarchy");
    let mut paths: Vec<_> = std::fs::read_dir(&dir)
        .expect("fixtures directory")
        .map(|entry| entry.expect("entry").path())
        .filter(|path| path.extension().and_then(|e| e.to_str()) == Some("toml"))
        .collect();
    paths.sort();
    for path in paths {
        let name = path.file_stem().unwrap().to_string_lossy().to_string();
        let keys = omarchy::parse(&std::fs::read_to_string(&path).expect("read fixture"));
        themes.push((name.clone(), omarchy::derive(&keys, &name)));
    }
    assert!(themes.len() >= 24, "expected the built-ins and the Omarchy themes, found {}", themes.len());
    themes
}

/// Each colour by the name it is stored under, resolved as a chart resolves it.
fn named(theme: &Theme) -> Vec<(&'static str, String)> {
    PALETTE_NAMES.iter().map(|name| (*name, ColorChoice::swatch(name).resolve(theme))).collect()
}

#[test]
fn every_palette_colour_is_a_mark_on_every_theme() {
    let mut faint = Vec::new();
    for (theme_name, theme) in shipped() {
        let bg = &theme.ui.background;
        for (name, hex) in named(&theme) {
            let ratio = contrast_ratio(&hex, bg);
            if ratio < MARK_CONTRAST {
                faint.push(format!("{theme_name}: {name} {hex} is {ratio:.2}:1 on {bg}"));
            }
        }
    }
    assert!(faint.is_empty(), "lines nobody can find:\n{}", faint.join("\n"));
}

#[test]
fn every_palette_colour_fills_a_profile_that_stands_off_every_theme() {
    let mut faint = Vec::new();
    for (theme_name, theme) in shipped() {
        let bg = &theme.ui.background;
        for (name, hex) in named(&theme) {
            let fill = mix(&hex, bg, 1.0 - DESIGNED_ALPHA);
            let distance = delta_e(&fill, bg);
            if distance < FILL_DISTANCE {
                faint.push(format!("{theme_name}: {name} {hex} fills as {fill}, {distance:.3} from {bg}"));
            }
        }
    }
    assert!(faint.is_empty(), "value areas that vanish:\n{}", faint.join("\n"));
}

/// The colours a chart draws in when nobody picked one, quieter than the
/// palette on purpose: a market profile's open and close arrows, and the
/// grey a study that takes no palette colour is drawn in. Quiet, not gone.
#[test]
fn the_quiet_defaults_still_read_on_every_theme() {
    let mut faint = Vec::new();
    for (theme_name, theme) in shipped() {
        let bg = &theme.ui.background;
        for (role, hex) in [
            ("market profile open", theme.tpo_open()),
            ("market profile close", theme.tpo_close()),
            ("an unpaletted study", theme.ui.text_muted.clone()),
        ] {
            let ratio = contrast_ratio(&hex, bg);
            if ratio < MARK_CONTRAST {
                faint.push(format!("{theme_name}: {role} {hex} is {ratio:.2}:1 on {bg}"));
            }
        }
    }
    assert!(faint.is_empty(), "quiet defaults that vanish:\n{}", faint.join("\n"));
}

/// A drawing's palette is its own nine, Ink last where a study's has
/// Neutral; it is held to the same floor.
#[test]
fn every_drawing_colour_is_a_mark_on_every_theme() {
    let mut faint = Vec::new();
    for (theme_name, theme) in shipped() {
        let bg = &theme.ui.background;
        for (preset, hex) in drawings::palette(&theme) {
            let ratio = contrast_ratio(&hex, bg);
            if ratio < MARK_CONTRAST {
                faint.push(format!("{theme_name}: {} {hex} is {ratio:.2}:1 on {bg}", preset.name()));
            }
        }
    }
    assert!(faint.is_empty(), "drawing colours nobody can find:\n{}", faint.join("\n"));
}

/// The last colour of the palette is the one that has no hue to keep, so it
/// is the one most easily left on the wrong side of a theme: near white
/// picked on a dark theme has to come back near black on a light one. It is
/// on the text's side of the background, and it is the strongest mark the
/// palette has, so it is held well above the floor: 7:1, WCAG's enhanced
/// level for text.
#[test]
fn the_neutral_sits_on_the_far_side_of_the_background_on_every_theme() {
    for (theme_name, theme) in shipped() {
        let neutral = ColorChoice::swatch("Neutral").resolve(&theme);
        let lightness = |hex: &str| omacharts_engine::theme::Oklch::of(hex).expect("a colour").l;
        let (bg, text, neutral_l) = (lightness(&theme.ui.background), lightness(&theme.ui.text), lightness(&neutral));
        assert_eq!(neutral_l > bg, text > bg, "{theme_name}: Neutral {neutral} is on the background's side");
        let ratio = contrast_ratio(&neutral, &theme.ui.background);
        assert!(ratio >= NEUTRAL_CONTRAST, "{theme_name}: Neutral {neutral} is only {ratio:.2}:1");
    }
}
