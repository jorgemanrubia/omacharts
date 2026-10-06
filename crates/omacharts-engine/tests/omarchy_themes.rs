//! Every theme Omarchy ships must produce a chart you can read.
//!
//! The Omarchy theme is derived at runtime from the desktop's own
//! `colors.toml`, so it works with themes that do not exist yet. The risk in
//! that is a palette we never looked at deriving something illegible — a grid
//! indistinguishable from the background, two overlay colours nobody can tell
//! apart.
//!
//! So every shipped palette is a fixture here, and the invariants are the same
//! ones our own themes are held to. A new Omarchy release adding a theme means
//! dropping its `colors.toml` in beside these.

use std::collections::HashMap;
use std::path::Path;

use omacharts_engine::frame::{MIN_FROM_CANDLE, MIN_FROM_FURNITURE, MIN_GUTTER, RING_CONTRAST};
use omacharts_engine::omarchy::{AXIS_CONTRAST, CROSSHAIR_CONTRAST, GRID_CONTRAST};
use omacharts_engine::theme::{
    contrast_ratio, delta_e, hue_gap, theme_bars, theme_mono_bars, Direction, Oklch,
    COMPANION_GAP, MONO_CONTRAST, SWATCH_SEQUENCE,
};
use omacharts_engine::link::{self, LinkGroup};
use omacharts_engine::{omarchy, palette, Theme};

fn fixtures() -> Vec<(String, Theme)> {
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
        out.push((name.clone(), omarchy::derive(&keys, &name)));
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    assert!(out.len() >= 20, "expected the shipped themes, found {}", out.len());
    out
}

/// Euclidean distance in RGB, normalised to 0..1.
fn distance(a: &str, b: &str) -> f64 {
    let parse = |hex: &str| -> (f64, f64, f64) {
        let h = hex.trim_start_matches('#');
        let byte = |i: usize| {
            u8::from_str_radix(h.get(i..i + 2).unwrap_or("00"), 16).unwrap_or(0) as f64 / 255.0
        };
        (byte(0), byte(2), byte(4))
    };
    let (ar, ag, ab) = parse(a);
    let (br, bg, bb) = parse(b);
    (((ar - br).powi(2) + (ag - bg).powi(2) + (ab - bb).powi(2)) / 3.0).sqrt()
}

#[test]
fn every_shipped_theme_derives_a_complete_palette() {
    for (name, theme) in fixtures() {
        assert!(!theme.ui.background.is_empty(), "{name}: no background");
        assert!(!theme.ui.text.is_empty(), "{name}: no text");
        assert_eq!(theme.swatches.len(), 8, "{name}: incomplete palette");
        for swatch in &theme.swatches {
            assert!(
                swatch.hex.starts_with('#') && swatch.hex.len() >= 7,
                "{name}/{}: {:?} is not a colour",
                swatch.name,
                swatch.hex
            );
        }
    }
}

#[test]
fn text_is_readable_on_the_background_in_every_theme() {
    for (name, theme) in fixtures() {
        let d = distance(&theme.ui.text, &theme.ui.background);
        assert!(d > 0.25, "{name}: text {} on {} ({d:.3})", theme.ui.text, theme.ui.background);
    }
}

/// The grid is felt rather than read, on every theme, and the band it is
/// held to has a ceiling as well as a floor.
///
/// It used to have only a floor that mattered: the grid had to be a
/// different pixel value from the background (RGB 0.004, a hundredth of a
/// step) and quieter than the text. On a light theme the text is near-black
/// on near-white, so "quieter than the text" allowed anything up to a mid
/// grey, and White's `#c0c0c0` grid on white, a lattice at 1.8:1, passed. So
/// did Lupine's at 1.04:1, which is no grid at all. The band is the same
/// one the derivation holds the grid to, so this is the check that no
/// shipped theme makes the derivation give up.
#[test]
fn the_grid_is_visible_but_not_loud_in_every_theme() {
    for (name, theme) in fixtures() {
        let ratio = contrast_ratio(&theme.ui.grid, &theme.ui.background);
        assert!(
            GRID_CONTRAST.holds(ratio),
            "{name}: grid {} on {} is {ratio:.3}:1, outside {:.2}..{:.2}",
            theme.ui.grid,
            theme.ui.background,
            GRID_CONTRAST.floor,
            GRID_CONTRAST.ceiling
        );
        // And it is lightness, not colour: never much more vivid than the
        // chart it sits on.
        let (ground, grid) = (Oklch::of(&theme.ui.background).unwrap(), Oklch::of(&theme.ui.grid).unwrap());
        assert!(
            grid.c <= ground.c + 0.04 + 1e-6,
            "{name}: grid {} has chroma {:.3} on a chart of {:.3}",
            theme.ui.grid,
            grid.c,
            ground.c
        );
    }
}

/// The axis is a line at the edge of the plot: louder than the grid, since
/// it marks where the plot ends, and never a bar.
#[test]
fn the_axis_is_a_line_and_not_a_bar_in_every_theme() {
    for (name, theme) in fixtures() {
        let ratio = contrast_ratio(&theme.ui.axis, &theme.ui.background);
        assert!(
            AXIS_CONTRAST.holds(ratio),
            "{name}: axis {} on {} is {ratio:.2}:1, outside {:.2}..{:.2}",
            theme.ui.axis,
            theme.ui.background,
            AXIS_CONTRAST.floor,
            AXIS_CONTRAST.ceiling
        );
        assert_eq!(theme.ui.border, theme.ui.axis, "{name}: the border is not the axis");
    }
}

#[test]
fn up_and_down_are_distinguishable_in_every_theme() {
    for (name, theme) in fixtures() {
        let bars = theme_bars(&theme);
        let up = bars.outline(Direction::Up);
        let down = bars.outline(Direction::Down);
        let d = distance(up, down);
        assert!(d > 0.1, "{name}: up {up} and down {down} are too close ({d:.3})");

        // And both must be visible against the chart.
        for (label, colour) in [("up", up), ("down", down)] {
            let against = distance(colour, &theme.ui.background);
            assert!(
                against > 0.1,
                "{name}: {label} {colour} vanishes into {} ({against:.3})",
                theme.ui.background
            );
        }
    }
}

#[test]
fn overlay_colours_are_legible_and_separable_in_every_theme() {
    for (name, theme) in fixtures() {
        for slot in 0..SWATCH_SEQUENCE.len() {
            let colour = theme.series(slot);
            let against = distance(&colour, &theme.ui.background);
            assert!(
                against > 0.1,
                "{name}: overlay {slot} ({colour}) vanishes into {} ({against:.3})",
                theme.ui.background
            );
        }
        for slot in 0..SWATCH_SEQUENCE.len() - 1 {
            let (a, b) = (theme.series(slot), theme.series(slot + 1));
            let d = distance(&a, &b);
            assert!(d > 0.08, "{name}: overlays {slot} and {} are too close: {a} vs {b} ({d:.3})", slot + 1);
        }
    }
}

#[test]
fn fills_sit_between_their_line_and_the_background_in_every_theme() {
    for (name, theme) in fixtures() {
        let line = theme.series(0);
        let fill = theme.fill_for(&line);
        let to_background = distance(&fill, &theme.ui.background);
        let line_to_background = distance(&line, &theme.ui.background);
        assert!(
            to_background < line_to_background,
            "{name}: the fill is not quieter than its line"
        );
        assert!(to_background > 0.002, "{name}: the fill is invisible");
    }
}

/// The chart's furniture has an order to it: text is the loudest thing on the
/// ground, the axis is quieter, the grid quieter still. A theme that inverts
/// that reads as a grid with some prices on it.
#[test]
fn the_chart_furniture_keeps_its_order_in_every_theme() {
    for (name, theme) in fixtures() {
        let text = distance(&theme.ui.text, &theme.ui.background);
        let axis = distance(&theme.ui.axis, &theme.ui.background);
        let grid = distance(&theme.ui.grid, &theme.ui.background);

        assert!(grid < axis, "{name}: the grid is louder than the axis");
        assert!(axis < text, "{name}: the axis is louder than the text");
        assert!(
            grid < text * 0.5,
            "{name}: the grid competes with the text ({grid:.3} vs {text:.3})"
        );
    }
}

/// Secondary text has to be legible and still read as secondary.
#[test]
fn muted_text_is_between_the_background_and_the_text_in_every_theme() {
    for (name, theme) in fixtures() {
        let text = distance(&theme.ui.text, &theme.ui.background);
        let muted = distance(&theme.ui.text_muted, &theme.ui.background);
        assert!(muted > 0.12, "{name}: secondary text is unreadable ({muted:.3})");
        assert!(muted < text, "{name}: secondary text is louder than primary");
    }
}

/// Panels sit on the chart, so they have to be visible against it without
/// becoming another bright object.
#[test]
fn panels_are_distinguishable_from_the_chart_in_every_theme() {
    for (name, theme) in fixtures() {
        let surface = distance(&theme.ui.surface, &theme.ui.background);
        let text = distance(&theme.ui.text, &theme.ui.background);
        assert!(surface > 0.008, "{name}: panels vanish into the chart ({surface:.3})");
        assert!(surface < text, "{name}: panels are louder than the text");
    }
}

/// The crosshair and the accent are things you look for, so they have to be
/// findable against the chart. The crosshair is also held below the text:
/// it is a pointer, and a pointer as bright as the price labels reads as a
/// mark the chart made rather than where the hand is.
#[test]
fn the_crosshair_and_accent_are_visible_in_every_theme() {
    for (name, theme) in fixtures() {
        for (what, colour) in [("crosshair", &theme.ui.crosshair), ("accent", &theme.ui.accent)] {
            let d = distance(colour, &theme.ui.background);
            assert!(d > 0.15, "{name}: the {what} ({colour}) is lost on the chart ({d:.3})");
        }
        let ratio = contrast_ratio(&theme.ui.crosshair, &theme.ui.background);
        assert!(
            CROSSHAIR_CONTRAST.holds(ratio),
            "{name}: crosshair {} on {} is {ratio:.2}:1, outside {:.1}..{:.1}",
            theme.ui.crosshair,
            theme.ui.background,
            CROSSHAIR_CONTRAST.floor,
            CROSSHAIR_CONTRAST.ceiling
        );
    }
}

/// A candle must not be mistakable for the text or the grid around it.
#[test]
fn candles_stand_apart_from_the_furniture_in_every_theme() {
    for (name, theme) in fixtures() {
        let bars = theme_bars(&theme);
        for direction in [Direction::Up, Direction::Down] {
            let colour = bars.outline(direction);
            assert!(
                distance(colour, &theme.ui.grid) > 0.12,
                "{name}: a {direction:?} candle is the colour of the grid"
            );
        }
    }
}

/// Anything painting a label on a direction colour — the bar widget's pill,
/// the chart's price chip — has to pick its text against that colour.
#[test]
fn a_label_on_a_direction_colour_is_readable_in_every_theme() {
    use omacharts_engine::theme::{luminance, readable_on};
    for (name, theme) in fixtures() {
        let bars = theme_bars(&theme);
        for direction in [Direction::Up, Direction::Down, Direction::Flat] {
            let pill = bars.outline(direction);
            let text = readable_on(pill);
            let gap = (luminance(pill) - luminance(text)).abs();
            assert!(
                gap > 0.35,
                "{name}: {text} on {pill} ({direction:?}) is {gap:.2} apart"
            );
        }
    }
}

/// The watchlist's change column is direction-coloured text on a panel, not on
/// the chart, so clearing the chart background is not enough.
#[test]
fn direction_coloured_text_is_readable_on_panels_in_every_theme() {
    for (name, theme) in fixtures() {
        let bars = theme_bars(&theme);
        for direction in [Direction::Up, Direction::Down] {
            for (where_, ground) in
                [("panels", &theme.ui.surface), ("toolbars", &theme.ui.surface_variant)]
            {
                let colour = bars.outline(direction);
                let d = distance(colour, ground);
                assert!(
                    d > 0.1,
                    "{name}: {direction:?} ({colour}) is lost on {where_} ({ground}) at {d:.3}"
                );
            }
        }
    }
}

#[test]
fn light_themes_are_detected_as_light() {
    let themes = fixtures();
    let light: Vec<&String> = themes
        .iter()
        .filter(|(_, t)| t.mode == omacharts_engine::Mode::Light)
        .map(|(name, _)| name)
        .collect();
    // The shipped light themes, by name. If this list changes, the detection
    // is what to check first.
    for expected in ["catppuccin-latte", "flexoki-light", "white"] {
        assert!(
            light.iter().any(|n| n.as_str() == expected),
            "{expected} should be detected as light; got {light:?}"
        );
    }
    assert!(light.len() < themes.len(), "not every theme is light");
}

// ---------------------------------------------------------------------------
// The indicator palette, measured as the eye sees it
// ---------------------------------------------------------------------------
//
// The tests above use RGB distance, which is fine for "is the grid a
// different pixel value from the background". The palette is held to OKLab
// distance (`delta_e`, where 0.02 is just noticeable and 1.0 is black to
// white) and WCAG contrast, because the question there is whether two thin
// lines *look* different, and RGB is wrong about that in exactly the cases
// that matter: it thinks two blues are as far apart as a blue and a yellow.

/// The six automatically assigned colours, in order.
fn series(theme: &Theme) -> Vec<String> {
    (0..SWATCH_SEQUENCE.len()).map(|n| theme.series(n)).collect()
}

/// Every pair of automatic overlays must be told apart at a glance, not only
/// the consecutive ones. Thirteen shipped themes used to hand out the same
/// hex for Teal and Cyan, which no consecutive-only check ever saw.
///
/// The companion is checked through the rule rather than as the next entry,
/// because the rule wraps where the sequence does not: the last swatch's
/// neighbour is the first, and seven shipped palettes put Cyan and Blue
/// closer than the floor. The rule skips past those; this is what catches it
/// no longer doing so.
#[test]
fn every_pair_of_overlays_is_perceptibly_different_in_every_theme() {
    for (name, theme) in fixtures() {
        let colours = series(&theme);
        for i in 0..colours.len() {
            for j in i + 1..colours.len() {
                let d = delta_e(&colours[i], &colours[j]);
                assert!(
                    d >= 0.08,
                    "{name}: {} {} and {} {} are {d:.3} apart",
                    SWATCH_SEQUENCE[i], colours[i], SWATCH_SEQUENCE[j], colours[j]
                );
            }
        }
        for i in 0..colours.len() - 1 {
            let d = delta_e(&colours[i], &colours[i + 1]);
            assert!(
                d >= COMPANION_GAP,
                "{name}: consecutive {} and {} are {d:.3} apart",
                colours[i], colours[i + 1]
            );
        }
        for (i, colour) in colours.iter().enumerate() {
            let companion = theme.companion(colour);
            let d = delta_e(colour, &companion);
            assert!(
                d >= COMPANION_GAP,
                "{name}: {} {colour} and its companion {companion} are {d:.3} apart",
                SWATCH_SEQUENCE[i]
            );
        }
    }
}

/// A 1.5px line needs WCAG's 3:1 for meaningful graphics against the chart,
/// and must not be mistakable for the grid, the axis or the crosshair.
#[test]
fn every_overlay_is_legible_on_the_chart_in_every_theme() {
    for (name, theme) in fixtures() {
        for (n, colour) in series(&theme).iter().enumerate() {
            let slot = SWATCH_SEQUENCE[n];
            let ratio = contrast_ratio(colour, &theme.ui.background);
            assert!(ratio >= 3.0, "{name}: {slot} {colour} is {ratio:.2}:1 on {}", theme.ui.background);
            // The generator guarantees 0.06 from each piece of furniture. The
            // grid is held higher because it is static and everywhere; every
            // shipped theme clears this comfortably, and the one with a dark
            // grid on white (White itself) is where this floor was set.
            let grid = delta_e(colour, &theme.ui.grid);
            assert!(grid >= 0.15, "{name}: {slot} {colour} is {grid:.3} from the grid");
            let axis = delta_e(colour, &theme.ui.axis);
            assert!(axis >= 0.06, "{name}: {slot} {colour} is {axis:.3} from the axis");
            let crosshair = delta_e(colour, &theme.ui.crosshair);
            assert!(crosshair >= 0.06, "{name}: {slot} {colour} is {crosshair:.3} from the crosshair");
        }
    }
}

/// An overlay in the candles' colours reads as price action.
#[test]
fn no_overlay_wears_a_direction_colour_in_every_theme() {
    for (name, theme) in fixtures() {
        let bars = theme_bars(&theme);
        for (n, colour) in series(&theme).iter().enumerate() {
            for direction in [Direction::Up, Direction::Down] {
                let candle = bars.outline(direction);
                let d = delta_e(colour, candle);
                assert!(
                    d >= 0.08,
                    "{name}: {} {colour} is {d:.3} from the {direction:?} candle {candle}",
                    SWATCH_SEQUENCE[n]
                );
            }
        }
    }
}

/// The six colours must look like one palette: lightness and chroma in a
/// band, hue doing the work. A set that passes every contrast test can still
/// be a box of crayons, and this is the test for that.
#[test]
fn the_overlays_are_one_family_in_every_theme() {
    for (name, theme) in fixtures() {
        let colours: Vec<Oklch> = series(&theme).iter().map(|h| Oklch::of(h).unwrap()).collect();
        let (l_lo, l_hi) = colours.iter().fold((1.0f64, 0.0f64), |(lo, hi), c| (lo.min(c.l), hi.max(c.l)));
        let (c_lo, c_hi) = colours.iter().fold((1.0f64, 0.0f64), |(lo, hi), c| (lo.min(c.c), hi.max(c.c)));
        assert!(l_hi - l_lo <= 0.25, "{name}: lightness runs {l_lo:.2} to {l_hi:.2}");
        assert!(c_lo >= 0.06, "{name}: a line is nearly grey ({c_lo:.3})");
        assert!(c_hi <= 0.21, "{name}: a line is garish ({c_hi:.3})");
        assert!(c_hi / c_lo <= 2.5, "{name}: chroma runs {c_lo:.3} to {c_hi:.3}");
    }
}

/// A colour stored by name has to mean the same thing under every theme, or
/// an indicator saved as Amber on Nord comes back blue on Lupine.
#[test]
fn every_named_overlay_keeps_its_promised_hue_in_every_theme() {
    for (name, theme) in fixtures() {
        for slot in SWATCH_SEQUENCE {
            let hex = &theme.swatch(slot).unwrap().hex;
            let hue = Oklch::of(hex).unwrap().h;
            let promised = palette::promised_hue(slot).unwrap();
            let gap = hue_gap(hue, promised);
            assert!(gap <= 32.0, "{name}: {slot} {hex} has hue {hue:.0}, promised {promised:.0}");
        }
    }
}

/// Where the theme has the colour a name means, that colour is used, not a
/// synthetic one: a Nord user should see Nord's blue.
#[test]
fn a_theme_that_offers_a_colour_keeps_it() {
    let (_, nord) = fixtures().into_iter().find(|(n, _)| n == "nord").unwrap();
    let blue = Oklch::of(&nord.swatch("Blue").unwrap().hex).unwrap();
    let theirs = Oklch::of("#81a1c1").unwrap();
    assert!(hue_gap(blue.h, theirs.h) < 1.0, "Blue is {blue:?}, Nord's is {theirs:?}");
    // Everforest's "cyan" is a mint nearer green than teal, so Teal is not
    // allowed it; Everforest's "blue" is a teal, and that is what Teal gets.
    let (_, everforest) = fixtures().into_iter().find(|(n, _)| n == "everforest").unwrap();
    let teal = Oklch::of(&everforest.swatch("Teal").unwrap().hex).unwrap();
    assert!(hue_gap(teal.h, Oklch::of("#7fbbb3").unwrap().h) < 1.0, "Teal is {teal:?}");
}

/// The palette follows the desktop: two themes must not produce the same
/// lines, or the Omarchy theme is a fixed palette in disguise.
#[test]
fn the_palette_changes_with_the_theme() {
    let themes = fixtures();
    for i in 0..themes.len() {
        for j in i + 1..themes.len() {
            let (a, b) = (series(&themes[i].1), series(&themes[j].1));
            assert!(
                a.iter().zip(&b).any(|(x, y)| delta_e(x, y) > 0.05),
                "{} and {} derived the same palette: {a:?}",
                themes[i].0,
                themes[j].0
            );
        }
    }
}

/// The frame around tiled charts: the gutter must read as a gap, and the
/// focus ring must be seen the same amount everywhere and never be the colour
/// of a line the chart draws. The derivation gives up on the last of those
/// when no lightness in range is clear, so this is also the check that no
/// shipped theme makes it give up.
#[test]
fn the_frame_is_visible_and_unmistakable_in_every_theme() {
    for (name, theme) in fixtures() {
        let bars = theme_bars(&theme);
        let frame = theme.frame(&bars);
        let ui = &theme.ui;

        let gutter = delta_e(&frame.gutter, &ui.background);
        assert!(gutter >= MIN_GUTTER, "{name}: gutter {} vanishes into the chart ({gutter:.3})", frame.gutter);

        let contrast = contrast_ratio(&frame.focus, &ui.background);
        assert!(contrast >= RING_CONTRAST, "{name}: ring {} is {contrast:.2}:1", frame.focus);

        for (what, colour) in [("axis", &ui.axis), ("grid", &ui.grid), ("border", &ui.border), ("crosshair", &ui.crosshair)] {
            let d = delta_e(&frame.focus, colour);
            assert!(d >= MIN_FROM_FURNITURE, "{name}: ring {} reads as the {what} {colour} ({d:.3})", frame.focus);
        }
        for (what, colour) in [("up", bars.outline(Direction::Up)), ("down", bars.outline(Direction::Down))] {
            let d = delta_e(&frame.focus, colour);
            assert!(d >= MIN_FROM_CANDLE, "{name}: ring {} reads as an {what} candle {colour} ({d:.3})", frame.focus);
        }
    }
}

/// Link groups are told apart by colour and nothing else — the badge is a few
/// pixels of tint with no label on it — so two groups the eye reads as one
/// colour is the whole affordance gone. The derivation has to hold that for
/// themes nobody has written yet, including the neutral first group, which is
/// the one at risk: a theme whose muted text is a lavender grey hands it a hue
/// close enough to be mistaken for a coloured group.
#[test]
fn every_link_group_is_a_different_colour_in_every_theme() {
    for (name, theme) in fixtures() {
        let colours: Vec<(LinkGroup, String)> = link::ALL
            .iter()
            .filter_map(|group| group.colour(&theme).map(|hex| (*group, hex)))
            .collect();
        assert_eq!(colours.len(), usize::from(link::GROUP_COUNT), "{name}: a group has no colour");

        for (i, (a, a_hex)) in colours.iter().enumerate() {
            for (b, b_hex) in &colours[i + 1..] {
                let d = delta_e(a_hex, b_hex);
                assert!(
                    d > 0.06,
                    "{name}: {} {a_hex} and {} {b_hex} read as one colour ({d:.3})",
                    a.label(),
                    b.label(),
                );
            }
        }
    }
}


/// Monochrome bars are one colour carrying the whole price series, so the
/// derivation has to get two things right for palettes nobody has written yet:
/// the colour has to be readable on that theme's chart, and it must not be the
/// colour that theme already uses for a rising bar.
///
/// The second is the one that bites. Omarchy ships palettes with no green in
/// them at all — `vantablack` and `white` are both monochrome to begin with —
/// and the Green swatch derived for those is a plain grey sitting exactly
/// where a neutral wants to be.
#[test]
fn every_monochrome_bar_is_legible_and_unlike_the_colours_it_replaces() {
    for (name, theme) in fixtures() {
        let coloured = theme_bars(&theme);
        let mono = theme_mono_bars(&theme);
        assert_eq!(mono.up, mono.down, "{name}: a direction colour survived");

        let ratio = contrast_ratio(&mono.up, &theme.ui.background);
        assert!(ratio >= MONO_CONTRAST.floor, "{name}: {} is {ratio:.2} on its chart", mono.up);

        for (what, hex) in [("up", &coloured.up), ("down", &coloured.down)] {
            let apart = delta_e(&mono.up, hex);
            assert!(apart > 0.06, "{name}: monochrome {} reads as {what} {hex} ({apart:.3})", mono.up);
        }
    }
}
