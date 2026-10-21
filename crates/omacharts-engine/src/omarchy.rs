//! The theme that follows the desktop.
//!
//! Omarchy writes a flat semantic palette to
//! `~/.local/state/omarchy/current/theme/colors.toml` and the theme's name to
//! `theme.name` beside it, rewriting both whenever the desktop theme changes.
//! We read those two files and map them onto a [`Theme`] — surfaces, text,
//! chart structure and the full indicator palette, since an Omarchy theme
//! already ships sixteen colours chosen to sit together.
//!
//! Polling beats inotify here: Omarchy swaps a symlinked directory, which is
//! exactly the case where watching a path either misses the change or needs
//! re-arming. Two `stat` calls on a timer cost nothing and never miss.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::palette;
use crate::theme::{
    distance, ensure_distinct, held_to, mix, ContrastBand, Mode, Oklch, Source, Theme, UiColors,
    OMARCHY_ID,
};

const COLORS: &str = ".local/state/omarchy/current/theme/colors.toml";
const NAME: &str = ".local/state/omarchy/current/theme.name";

pub fn colors_path(home: &Path) -> PathBuf {
    home.join(COLORS)
}

pub fn name_path(home: &Path) -> PathBuf {
    home.join(NAME)
}

/// Is Omarchy present on this machine at all? Decides whether the Omarchy
/// theme is offered, and whether it is the default.
pub fn available(home: &Path) -> bool {
    colors_path(home).is_file()
}

/// A cheap value that changes whenever the desktop theme does, so the app can
/// poll on a timer and only rebuild when something actually moved.
pub fn fingerprint(home: &Path) -> Option<String> {
    let name = std::fs::read_to_string(name_path(home)).ok()?;
    let colors = std::fs::read_to_string(colors_path(home)).ok()?;
    Some(format!("{}:{:x}", name.trim(), hash(&colors)))
}

/// FNV-1a. Not cryptographic — it only has to change when the file does.
fn hash(s: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100_0000_01b3);
    }
    h
}

/// Read the live Omarchy palette and map it onto a theme.
pub fn current(home: &Path) -> Option<Theme> {
    let text = std::fs::read_to_string(colors_path(home)).ok()?;
    let name = std::fs::read_to_string(name_path(home))
        .ok()
        .map(|n| pretty_name(n.trim()))
        .unwrap_or_else(|| "Omarchy".to_string());
    Some(derive(&parse(&text), &name))
}

/// Where Omarchy keeps the themes it ships, and where people install their
/// own. A theme in the second shadows one of the same name in the first,
/// the way Omarchy itself resolves them.
const INSTALLED: [&str; 2] = [".local/share/omarchy/themes", ".config/omarchy/themes"];

/// Every Omarchy theme installed on this machine, each as a fixed theme of
/// its own, sorted by name.
///
/// Offered beside the one that follows the desktop, so a chart can wear a
/// theme other than the desktop's. Read once rather than polled: a theme
/// installed while the app is open shows up next time it starts.
pub fn installed(home: &Path) -> Vec<Theme> {
    let mut found: HashMap<String, Theme> = HashMap::new();
    for dir in INSTALLED {
        let Ok(entries) = std::fs::read_dir(home.join(dir)) else { continue };
        for entry in entries.flatten() {
            let slug = entry.file_name().to_string_lossy().into_owned();
            let Ok(text) = std::fs::read_to_string(entry.path().join("colors.toml")) else {
                continue;
            };
            let mut theme = derive(&parse(&text), &pretty_name(&slug));
            theme.id = format!("{OMARCHY_ID}:{slug}");
            theme.source = Source::BuiltIn;
            found.insert(slug, theme);
        }
    }
    let mut themes: Vec<Theme> = found.into_values().collect();
    themes.sort_by(|a, b| a.name.cmp(&b.name));
    themes
}

/// `key = "value"`, one per line; comments and blanks ignored.
///
/// The file is flat, so a short parser beats a TOML dependency.
pub fn parse(text: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = value.trim().trim_matches('"').trim_matches('\'').trim();
        if value.is_empty() {
            continue;
        }
        out.insert(key.trim().to_string(), value.to_string());
    }
    out
}

/// "everforest" -> "Everforest", "tokyo-night" -> "Tokyo Night".
fn pretty_name(raw: &str) -> String {
    let words: Vec<String> = raw
        .split(['-', '_', ' '])
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut chars = w.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect();
    if words.is_empty() {
        "Omarchy".to_string()
    } else {
        words.join(" ")
    }
}

/// How far a panel has to sit off the chart to read as a panel.
const MIN_SURFACE: f64 = 0.012;

/// The grid's contrast against the chart, as WCAG counts it.
///
/// The grid is felt rather than read: enough to judge alignment by, not
/// enough to notice. The four hand-tuned built-in themes put theirs between
/// 1.11 and 1.16, and that is the band's middle. Below 1.08 a hairline is
/// not reliably there (Lupine's 1.04 was a grid in name only); above 1.20 it
/// is a lattice the candles have to compete with, which is what Flexoki
/// Light's 1.24 and White's 1.82 were. The band is the same on light and
/// dark themes, because the ratio already accounts for the ground.
pub const GRID_CONTRAST: ContrastBand = ContrastBand::new(1.08, 1.20);

/// The axis lines' contrast against the chart. Louder than the grid, since
/// they mark where the plot ends, and still a line rather than a bar: the
/// built-ins sit between 1.48 and 1.72, where Ethereal's and Vantablack's
/// "muted" greys taken as they were made an axis at nearly 5:1, as visible
/// as a candle.
pub const AXIS_CONTRAST: ContrastBand = ContrastBand::new(1.35, 2.20);

/// The crosshair's contrast against the chart. It is a pointer, so the floor
/// is WCAG's 3:1 for graphics that carry meaning, the same as the focus
/// ring's. The ceiling is wide, because a pointer may be loud; 10:1 is
/// where it stops being a pointer and becomes the text colour, which is
/// what Kanagawa's accent literally is and Hackerman's nearly is.
pub const CROSSHAIR_CONTRAST: ContrastBand = ContrastBand::new(3.0, 10.0);

/// How much more vivid than the chart the grid may be. A grid is lightness,
/// not colour: a theme whose selection colour is a saturated blue would
/// otherwise produce a grid at the right contrast that still reads as blue
/// lines. No shipped theme comes within a third of this; it bounds the
/// theme we have not seen.
const GRID_CHROMA_ABOVE_GROUND: f64 = 0.04;

/// Map the semantic keys onto our theme.
///
/// Omarchy guarantees only a core set of keys, so every lookup carries a
/// fallback chain ending in a literal. A theme that omits
/// `lighter_background` still produces a readable chart rather than a void.
pub fn derive(keys: &HashMap<String, String>, name: &str) -> Theme {
    let mode = match keys.get("mode").or_else(|| keys.get("theme_type")) {
        Some(m) if m.eq_ignore_ascii_case("light") => Mode::Light,
        _ => Mode::Dark,
    };
    let dark = mode == Mode::Dark;

    let pick = |names: &[&str], fallback: &str| -> String {
        for n in names {
            if let Some(v) = keys.get(*n) {
                return v.clone();
            }
        }
        fallback.to_string()
    };

    let background = pick(&["background"], if dark { "#101010" } else { "#ffffff" });
    let text = pick(&["foreground"], if dark { "#e6e6e6" } else { "#1f1f1f" });
    let muted = pick(&["muted", "selection"], &text);
    let accent = pick(&["accent", "blue", "cyan"], &text);

    // Secondary text has to read as secondary. Some themes put their
    // "light_foreground" further from the background than the foreground
    // itself, which inverts the hierarchy; derive it instead when that
    // happens.
    let text_muted = {
        let picked = pick(&["light_foreground", "dark_foreground"], &muted);
        let contrast = |colour: &str| distance(colour, &background);
        if contrast(&picked) >= contrast(&text) || contrast(&picked) < 0.12 {
            mix(&text, &background, 0.42)
        } else {
            picked
        }
    };

    // A panel the same colour as the chart is not a panel. Halfway to the
    // theme's lighter ground, not all the way: `lighter_background` is a
    // terminal's selection colour, and a rail and a tool column painted in it
    // stood out as a slab of grey-blue beside the chart.
    let surface = ensure_distinct(
        &mix(&background, &pick(&["lighter_background", "dark_background"], &background), 0.5),
        &background,
        MIN_SURFACE,
        &text,
    );

    // The chart's furniture is held to a contrast band rather than taken as
    // it comes. `lighter_background` is where Omarchy themes usually put the
    // grid, but it was chosen for a terminal's selection, not for a hairline
    // across a chart: some themes set it to the background itself, and the
    // light ones set it to a grey that is a lattice on a pale chart. The
    // theme's own colour is kept whenever it sits inside the band, and moved
    // in lightness only as far as it must when it does not.
    let grid_chroma = Oklch::of(&background).map(|b| b.c + GRID_CHROMA_ABOVE_GROUND);
    let grid = held_to(
        &pick(&["lighter_background", "selection"], &background),
        &background,
        GRID_CONTRAST,
        grid_chroma,
    );

    // The furniture is decided before the palette, because an overlay must
    // stay clear of it: the crosshair is the accent, which is also the first
    // colour most themes call blue, and the axis is the muted colour, which
    // in Ethereal is a blue of its own. Each keeps its hue and loses only
    // the loudness.
    let crosshair = held_to(&accent, &background, CROSSHAIR_CONTRAST, None);
    let axis = held_to(&muted, &background, AXIS_CONTRAST, None);
    let swatches = palette::generate(
        keys,
        &palette::Ground { background: &background, grid: &grid, axis: &axis, crosshair: &crosshair },
    );

    Theme {
        id: OMARCHY_ID.to_string(),
        name: name.to_string(),
        mode,
        source: Source::Omarchy,
        ui: UiColors {
            surface_variant: ensure_distinct(
                &pick(&["selection", "lighter_background"], &surface),
                &background,
                MIN_SURFACE,
                &text,
            ),
            surface,
            // The border is the axis: it rules the edge above an indicator
            // strip and the edges of the window's panels, and a rule heavier
            // than the axis beside it reads as a second axis.
            border: axis.clone(),
            text_muted,
            grid,
            axis,
            crosshair,
            background,
            text,
            accent,
        },
        swatches,
        bars: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::{contrast_ratio, theme_bars, SWATCH_NAMES};

    const EVERFOREST: &str = r##"
mode = "dark"
accent = "#7fbbb3"
selection = "#3d484d"
muted = "#475258"
background = "#2d353b"
dark_background = "#21272c"
lighter_background = "#343f44"
foreground = "#d3c6aa"
light_foreground = "#9da9a0"
red = "#e67e80"
yellow = "#dbbc7f"
orange = "#e09d7f"
green = "#a7c080"
cyan = "#83c092"
blue = "#7fbbb3"
magenta = "#d699b6"
"##;

    #[test]
    fn parses_the_real_file_shape() {
        let keys = parse(EVERFOREST);
        assert_eq!(keys.get("accent").unwrap(), "#7fbbb3");
        assert_eq!(keys.get("mode").unwrap(), "dark");
        assert_eq!(keys.len(), 16);
    }

    #[test]
    fn derives_a_complete_theme() {
        let theme = derive(&parse(EVERFOREST), "Everforest");
        assert_eq!(theme.name, "Everforest");
        assert_eq!(theme.mode, Mode::Dark);
        assert_eq!(theme.source, Source::Omarchy);
        assert_eq!(theme.ui.background, "#2d353b");
        assert_eq!(theme.ui.text, "#d3c6aa");
        assert_eq!(theme.ui.grid, "#343f44");
        assert_eq!(theme.ui.crosshair, "#7fbbb3");
        for name in SWATCH_NAMES {
            assert!(theme.swatch(name).is_some(), "missing {name}");
        }
        assert_eq!(theme.swatch("Green").unwrap().hex, "#a7c080");
        assert_eq!(theme.swatch("Rose").unwrap().hex, "#e67e80");
    }

    #[test]
    fn candles_follow_the_desktop() {
        let theme = derive(&parse(EVERFOREST), "Everforest");
        let bars = theme_bars(&theme);
        assert_eq!(bars.up, "#a7c080");
        assert_eq!(bars.down, "#e67e80");
    }

    #[test]
    fn a_sparse_theme_still_yields_every_colour() {
        let theme = derive(&parse("mode = \"light\"\nbackground = \"#ffffff\"\n"), "Bare");
        assert_eq!(theme.mode, Mode::Light);
        assert_eq!(theme.ui.background, "#ffffff");
        assert!(!theme.ui.text.is_empty());
        assert!(!theme.ui.grid.is_empty());
        assert_eq!(theme.swatches.len(), SWATCH_NAMES.len());
        assert!(theme.swatches.iter().all(|s| !s.hex.is_empty()));
    }

    #[test]
    fn a_grid_the_theme_made_loud_is_quietened_and_a_quiet_one_is_kept() {
        // White ships a mid grey as its lighter background, which on a white
        // chart is a lattice at 1.8:1.
        let white = derive(&parse("mode = \"light\"\nbackground = \"#ffffff\"\nlighter_background = \"#c0c0c0\"\nforeground = \"#000000\"\n"), "White");
        let ratio = contrast_ratio(&white.ui.grid, &white.ui.background);
        assert!(GRID_CONTRAST.holds(ratio), "grid {} is {ratio:.3}:1", white.ui.grid);
        assert_ne!(white.ui.grid, "#c0c0c0");
        // Everforest's grid was always inside the band and is kept to the byte.
        assert_eq!(derive(&parse(EVERFOREST), "Everforest").ui.grid, "#343f44");
    }

    #[test]
    fn a_grid_the_theme_set_to_the_background_is_lifted_off_it() {
        // Solitude and Last Horizon both do this.
        let theme = derive(&parse("mode = \"dark\"\nbackground = \"#101315\"\nlighter_background = \"#101315\"\nforeground = \"#cacccc\"\n"), "Solitude");
        let ratio = contrast_ratio(&theme.ui.grid, &theme.ui.background);
        assert!(GRID_CONTRAST.holds(ratio), "grid {} is {ratio:.3}:1", theme.ui.grid);
    }

    #[test]
    fn the_axis_and_the_crosshair_keep_their_hue_and_lose_their_loudness() {
        // Ethereal: a saturated blue "muted" at nearly 5:1, and an accent
        // that is fine as it is.
        let theme = derive(
            &parse("mode = \"dark\"\nbackground = \"#060B1E\"\nmuted = \"#6d7db6\"\naccent = \"#7d82d9\"\nforeground = \"#ffcead\"\n"),
            "Ethereal",
        );
        let axis = contrast_ratio(&theme.ui.axis, &theme.ui.background);
        assert!(AXIS_CONTRAST.holds(axis), "axis {} is {axis:.2}:1", theme.ui.axis);
        let (was, now) = (Oklch::of("#6d7db6").unwrap(), Oklch::of(&theme.ui.axis).unwrap());
        assert!((was.h - now.h).abs() < 2.0, "the axis changed hue: {} vs {}", was.h, now.h);
        assert_eq!(theme.ui.border, theme.ui.axis);
        assert_eq!(theme.ui.crosshair, "#7d82d9");
    }

    #[test]
    fn a_crosshair_the_colour_of_the_text_is_brought_below_it() {
        // Kanagawa's accent is its foreground.
        let theme = derive(
            &parse("mode = \"dark\"\nbackground = \"#1f1f28\"\naccent = \"#dcd7ba\"\nforeground = \"#dcd7ba\"\n"),
            "Kanagawa",
        );
        let ratio = contrast_ratio(&theme.ui.crosshair, &theme.ui.background);
        assert!(CROSSHAIR_CONTRAST.holds(ratio), "crosshair {} is {ratio:.2}:1", theme.ui.crosshair);
        assert_ne!(theme.ui.crosshair, theme.ui.text);
    }

    #[test]
    fn light_mode_is_detected() {
        assert_eq!(derive(&parse("mode = \"light\""), "x").mode, Mode::Light);
        assert_eq!(derive(&parse("theme_type = \"light\""), "x").mode, Mode::Light);
        assert_eq!(derive(&parse(""), "x").mode, Mode::Dark);
    }

    #[test]
    fn every_installed_theme_is_offered_once_and_a_users_own_wins() {
        let home = std::env::temp_dir().join(format!("omacharts-installed-{}", std::process::id()));
        let write = |dir: &str, slug: &str, background: &str| {
            let path = home.join(dir).join(slug);
            std::fs::create_dir_all(&path).unwrap();
            std::fs::write(path.join("colors.toml"), format!("background = \"{background}\"\n"))
                .unwrap();
        };
        write(INSTALLED[0], "tokyo-night", "#1a1b26");
        write(INSTALLED[0], "everforest", "#2d353b");
        write(INSTALLED[1], "everforest", "#000000");
        std::fs::create_dir_all(home.join(INSTALLED[0]).join("no-colours")).unwrap();

        let themes = installed(&home);
        let _ = std::fs::remove_dir_all(&home);

        let names: Vec<&str> = themes.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, ["Everforest", "Tokyo Night"]);
        assert_eq!(themes[0].id, "omarchy:everforest");
        assert_eq!(themes[0].source, Source::BuiltIn);
        assert_eq!(themes[0].ui.background, derive(&parse("background = \"#000000\""), "").ui.background);
    }

    #[test]
    fn names_are_prettified() {
        assert_eq!(pretty_name("tokyo-night"), "Tokyo Night");
        assert_eq!(pretty_name("everforest"), "Everforest");
        assert_eq!(pretty_name(""), "Omarchy");
    }

    #[test]
    fn the_fingerprint_moves_with_the_content() {
        assert_ne!(hash("a"), hash("b"));
        assert_eq!(hash(EVERFOREST), hash(EVERFOREST));
    }
}
