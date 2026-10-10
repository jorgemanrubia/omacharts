//! Render the nine drawing presets over candles, as every shipped theme
//! resolves them, onto one self-contained HTML page: `doc/themes/drawings.html`.
//!
//! The tests say whether a preset is *acceptable*. This page is for the
//! question the thresholds cannot answer: does a box in Amber look like it
//! belongs on Gruvbox, and can a line in Teal be told from one in Cyan on
//! Paper? Each theme is drawn at one CSS pixel per pixel: the nine lines
//! cutting through candles on the left, the nine rectangles over runs of them
//! in the middle, and on the right the nine as ellipses with a label in each
//! — which is where the halo under a label's glyphs can be judged, since a
//! 16% fill means what is really behind them is a candle. Under each picture
//! are the numbers it is judged by. Before the themes, the same boxes are
//! drawn at three border strengths, which is how the border's alpha was
//! chosen.
//!
//!     cargo run -p omacharts-engine --example drawing_sheet
//!
//! writes the page in place. Pass a path to write it elsewhere; a gallery of
//! the pictures alone, short enough to screenshot, is written beside it. The
//! decision numbers — the fill and border sweeps, the weakest cases and what
//! a fixed palette could have done — go to stderr.
//!
//! The colours come from `drawings::palette`, the same call the chart makes;
//! nothing here is a copy of what the app does.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use omacharts_engine::drawings::{
    self, BORDER_ALPHA, BORDER_WIDTH, CROSSHAIR_ALPHA, DRAWING_CONTRAST, FILL_ALPHA,
    MIN_FROM_FURNITURE, MIN_SEPARATION, Preset, TEXT_CONTRAST,
};
use omacharts_engine::omarchy;
use omacharts_engine::theme::{
    BarScheme, ContrastBand, Direction, Oklch, Theme, builtin_themes, contrast_ratio, delta_e,
    held_to, mix, theme_bars,
};

/// The nine, by name, in the order the picker shows them.
const PRESETS: [&str; 9] = [
    "Up", "Down", "Blue", "Amber", "Violet", "Teal", "Orange", "Cyan", "Ink",
];

/// The strengths shown side by side so the border can be judged by eye.
const BORDER_COMPARE: [f64; 3] = [0.45, 0.35, 0.25];

/// A border as drawn must be at least as loud as the faintest axis line the
/// app draws, or over busy candles the shape has no edge.
const BORDER_CONTRAST: f64 = 1.35;
/// And it must be a visible step up from the fill it encloses.
const MIN_BORDER_FROM_TINT: f64 = 0.06;
/// The chart's default stroke.
const LINE_WIDTH: f64 = 1.5;

// What is marked. The floors the module holds, plus the fill's own.
const MIN_CONTRAST: f64 = 3.0;
const MIN_PAIR: f64 = MIN_SEPARATION;
const MIN_FROM_CANDLE: f64 = MIN_SEPARATION;
/// A fill fainter than the grid is not there. Louder than an axis is a
/// slab.
const TINT_CONTRAST: ContrastBand = ContrastBand::new(1.08, 2.20);
/// Up and down must still be told apart under the fill.
const MIN_DIR_UNDER: f64 = 0.06;
/// A body under the fill must still be a body.
const MIN_BODY_UNDER: f64 = 3.0;

/// A preset as the engine resolved it, with what it started from.
struct Resolved {
    name: &'static str,
    hex: String,
    /// The swatch or text colour it started as, when it started as one.
    origin: Option<String>,
}

fn drawing_palette(theme: &Theme, _bars: &BarScheme) -> Vec<Resolved> {
    drawings::palette(theme)
        .into_iter()
        .map(|(preset, hex)| Resolved {
            name: preset.name(),
            hex,
            origin: match preset {
                Preset::Ink => Some(theme.ui.text.clone()),
                p => p
                    .swatch()
                    .and_then(|s| theme.swatch(s))
                    .map(|s| s.hex.clone()),
            },
        })
        .collect()
}

/// Cairo composites in sRGB bytes, so a translucent fill over a colour is a
/// plain lerp.
fn over(colour: &str, ground: &str, alpha: f64) -> String {
    mix(colour, ground, 1.0 - alpha)
}

// ---------------------------------------------------------------------------
// Measurement
// ---------------------------------------------------------------------------

struct LineRow {
    name: &'static str,
    hex: String,
    swatch: Option<String>,
    lch: Oklch,
    contrast: f64,
    nearest: (&'static str, f64),
    from_up: f64,
    from_down: f64,
    from_grid: f64,
    from_axis: f64,
    from_crosshair: f64,
    on_up: f64,
    on_down: f64,
}

struct RectRow {
    name: &'static str,
    tint: String,
    /// The border as drawn over the chart.
    border: String,
    border_contrast: f64,
    border_from_tint: f64,
    nearest_border: (&'static str, f64),
    tint_de: f64,
    tint_contrast: f64,
    up_under: f64,
    down_under: f64,
    dir_bare: f64,
    dir_under: f64,
    nearest_tint: (&'static str, f64),
}

struct Row {
    name: String,
    theme: Theme,
    bars: BarScheme,
    lines: Vec<LineRow>,
    rects: Vec<RectRow>,
}

impl Row {
    fn of(name: &str, theme: &Theme) -> Row {
        let bars = theme_bars(theme);
        let ui = &theme.ui;
        let resolved = drawing_palette(theme, &bars);
        let hexes: Vec<String> = resolved.iter().map(|r| r.hex.clone()).collect();
        let lines = resolved
            .iter()
            .enumerate()
            .map(|(n, res)| {
                let hex = &res.hex;
                let pname = res.name;
                let nearest = hexes
                    .iter()
                    .enumerate()
                    .filter(|(m, _)| *m != n)
                    .map(|(m, other)| (PRESETS[m], delta_e(hex, other)))
                    .min_by(|a, b| a.1.total_cmp(&b.1))
                    .unwrap();
                LineRow {
                    name: pname,
                    swatch: res.origin.clone(),
                    lch: Oklch::of(hex).unwrap(),
                    contrast: contrast_ratio(hex, &ui.background),
                    nearest,
                    from_up: delta_e(hex, &bars.up),
                    from_down: delta_e(hex, &bars.down),
                    from_grid: delta_e(hex, &ui.grid),
                    from_axis: delta_e(hex, &ui.axis),
                    from_crosshair: delta_e(
                        hex,
                        &over(&ui.crosshair, &ui.background, CROSSHAIR_ALPHA),
                    ),
                    on_up: contrast_ratio(hex, &bars.up),
                    on_down: contrast_ratio(hex, &bars.down),
                    hex: hex.clone(),
                }
            })
            .collect();
        let rects = rects_at(theme, &bars, &hexes, FILL_ALPHA, BORDER_ALPHA);
        Row {
            name: name.to_string(),
            theme: theme.clone(),
            bars,
            lines,
            rects,
        }
    }

    fn line_failures(&self) -> usize {
        self.lines
            .iter()
            .enumerate()
            .map(|(n, l)| {
                let direction = n < 2;
                usize::from(l.contrast < MIN_CONTRAST)
                    + usize::from(l.nearest.1 < MIN_PAIR)
                    + usize::from(
                        !direction
                            && (l.from_up < MIN_FROM_CANDLE || l.from_down < MIN_FROM_CANDLE),
                    )
                    + usize::from(
                        l.from_grid < MIN_FROM_FURNITURE
                            || l.from_axis < MIN_FROM_FURNITURE
                            || l.from_crosshair < MIN_FROM_FURNITURE,
                    )
            })
            .sum()
    }

    fn rect_failures(&self) -> usize {
        self.rects
            .iter()
            .map(|r| {
                usize::from(!TINT_CONTRAST.holds(r.tint_contrast))
                    + usize::from(r.border_contrast < BORDER_CONTRAST)
                    + usize::from(r.border_from_tint < MIN_BORDER_FROM_TINT)
                    + usize::from(r.dir_under < MIN_DIR_UNDER)
                    + usize::from(r.up_under < MIN_BODY_UNDER || r.down_under < MIN_BODY_UNDER)
            })
            .sum()
    }
}

fn rects_at(
    theme: &Theme,
    bars: &BarScheme,
    hexes: &[String],
    alpha: f64,
    border_alpha: f64,
) -> Vec<RectRow> {
    let bg = &theme.ui.background;
    let tints: Vec<String> = hexes.iter().map(|h| over(h, bg, alpha)).collect();
    let borders: Vec<String> = hexes.iter().map(|h| over(h, bg, border_alpha)).collect();
    PRESETS
        .iter()
        .enumerate()
        .map(|(n, pname)| {
            let hex = &hexes[n];
            let tint = &tints[n];
            let border = &borders[n];
            let nearest_border = borders
                .iter()
                .enumerate()
                .filter(|(m, _)| *m != n)
                .map(|(m, other)| (PRESETS[m], delta_e(border, other)))
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .unwrap();
            let up_t = over(hex, &bars.up, alpha);
            let down_t = over(hex, &bars.down, alpha);
            let nearest_tint = tints
                .iter()
                .enumerate()
                .filter(|(m, _)| *m != n)
                .map(|(m, other)| (PRESETS[m], delta_e(tint, other)))
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .unwrap();
            RectRow {
                name: pname,
                border: border.clone(),
                border_contrast: contrast_ratio(border, bg),
                border_from_tint: delta_e(border, tint),
                nearest_border,
                tint_de: delta_e(tint, bg),
                tint_contrast: contrast_ratio(tint, bg),
                up_under: contrast_ratio(&up_t, tint),
                down_under: contrast_ratio(&down_t, tint),
                dir_bare: delta_e(&bars.up, &bars.down),
                dir_under: delta_e(&up_t, &down_t),
                nearest_tint,
                tint: tint.clone(),
            }
        })
        .collect()
}

fn themes() -> Vec<(String, Theme)> {
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
    for theme in builtin_themes() {
        out.push((format!("{} (built in)", theme.name), theme));
    }
    out
}

// ---------------------------------------------------------------------------
// Decision numbers, to stderr
// ---------------------------------------------------------------------------

fn report(rows: &[Row]) {
    eprintln!("== lines: per preset, across {} themes ==", rows.len());
    eprintln!(
        "{:8} {:>8} {:>8} {:>8}  {:>7} {:>7} {:>7} {:>7}  moved",
        "preset", "min ctr", "med ctr", "max ctr", "minpair", "mincand", "minfurn", "min on candle"
    );
    for (n, pname) in PRESETS.iter().enumerate() {
        let mut ctr: Vec<f64> = rows.iter().map(|r| r.lines[n].contrast).collect();
        ctr.sort_by(|a, b| a.total_cmp(b));
        let minpair = rows
            .iter()
            .map(|r| r.lines[n].nearest.1)
            .fold(f64::MAX, f64::min);
        let mincand = rows
            .iter()
            .map(|r| r.lines[n].from_up.min(r.lines[n].from_down))
            .fold(f64::MAX, f64::min);
        let minfurn = rows
            .iter()
            .map(|r| {
                r.lines[n]
                    .from_grid
                    .min(r.lines[n].from_axis)
                    .min(r.lines[n].from_crosshair)
            })
            .fold(f64::MAX, f64::min);
        let onc = rows
            .iter()
            .map(|r| r.lines[n].on_up.min(r.lines[n].on_down))
            .fold(f64::MAX, f64::min);
        let moved = rows
            .iter()
            .filter(|r| {
                r.lines[n]
                    .swatch
                    .as_deref()
                    .is_some_and(|s| s != r.lines[n].hex)
            })
            .count();
        eprintln!(
            "{:8} {:8.2} {:8.2} {:8.2}  {:7.3} {:7.3} {:7.3} {:7.2}  {}",
            pname,
            ctr[0],
            ctr[ctr.len() / 2],
            ctr[ctr.len() - 1],
            minpair,
            mincand,
            minfurn,
            onc,
            moved
        );
    }
    eprintln!();
    eprintln!("== lines: weakest cases ==");
    let mut worst: Vec<(String, &'static str, &str, f64)> = Vec::new();
    for r in rows {
        for l in &r.lines {
            worst.push((r.name.clone(), l.name, "contrast", l.contrast));
        }
    }
    worst.sort_by(|a, b| a.3.total_cmp(&b.3));
    for w in worst.iter().take(10) {
        eprintln!("  {:28} {:7} {} {:.2}", w.0, w.1, w.2, w.3);
    }
    let mut pairs: Vec<(String, &'static str, &'static str, f64)> = Vec::new();
    for r in rows {
        for l in &r.lines {
            pairs.push((r.name.clone(), l.name, l.nearest.0, l.nearest.1));
        }
    }
    pairs.sort_by(|a, b| a.3.total_cmp(&b.3));
    eprintln!("  closest pairs:");
    for p in pairs.iter().take(10) {
        eprintln!("  {:28} {:7} ~ {:7} {:.3}", p.0, p.1, p.2, p.3);
    }
    let mut furn: Vec<(String, &'static str, f64)> = Vec::new();
    for r in rows {
        for l in &r.lines {
            furn.push((
                r.name.clone(),
                l.name,
                l.from_grid.min(l.from_axis).min(l.from_crosshair),
            ));
        }
    }
    furn.sort_by(|a, b| a.2.total_cmp(&b.2));
    eprintln!("  closest to furniture:");
    for p in furn.iter().take(8) {
        eprintln!("  {:28} {:7} {:.3}", p.0, p.1, p.2);
    }
    let mut cand: Vec<(String, &'static str, f64)> = Vec::new();
    for r in rows {
        for l in r.lines.iter().skip(2) {
            cand.push((r.name.clone(), l.name, l.from_up.min(l.from_down)));
        }
    }
    cand.sort_by(|a, b| a.2.total_cmp(&b.2));
    eprintln!("  closest to a candle (presets 3-9):");
    for p in cand.iter().take(8) {
        eprintln!("  {:28} {:7} {:.3}", p.0, p.1, p.2);
    }
    eprintln!("  presets moved from their origin:");
    for r in rows {
        for l in &r.lines {
            if let Some(s) = &l.swatch
                && s != &l.hex
            {
                eprintln!(
                    "  {:28} {:7} {} -> {}  (was {:.2}:1, now {:.2}:1, ΔE {:.3})",
                    r.name,
                    l.name,
                    s,
                    l.hex,
                    contrast_ratio(s, &r.theme.ui.background),
                    l.contrast,
                    delta_e(s, &l.hex)
                );
            }
        }
    }

    eprintln!();
    eprintln!("== fill alpha sweep (all themes x presets) ==");
    eprintln!(
        "{:>5} {:>8} {:>8} {:>8} {:>8} {:>8} {:>8} {:>8}",
        "alpha", "tintΔE-", "tintΔE+", "tintC-", "tintC+", "dirΔE-", "body-", "tint~-"
    );
    for alpha in [0.08, 0.10, 0.12, 0.14, 0.16, 0.18, 0.20, 0.25, 0.30] {
        let mut tde = (f64::MAX, 0.0f64);
        let mut tc = (f64::MAX, 0.0f64);
        let mut dir = f64::MAX;
        let mut body = f64::MAX;
        let mut near = f64::MAX;
        for r in rows {
            let hexes: Vec<String> = r.lines.iter().map(|l| l.hex.clone()).collect();
            for rect in rects_at(&r.theme, &r.bars, &hexes, alpha, BORDER_ALPHA) {
                tde = (tde.0.min(rect.tint_de), tde.1.max(rect.tint_de));
                tc = (tc.0.min(rect.tint_contrast), tc.1.max(rect.tint_contrast));
                dir = dir.min(rect.dir_under);
                body = body.min(rect.up_under.min(rect.down_under));
                near = near.min(rect.nearest_tint.1);
            }
        }
        eprintln!(
            "{:5.2} {:8.3} {:8.3} {:8.2} {:8.2} {:8.3} {:8.2} {:8.3}",
            alpha, tde.0, tde.1, tc.0, tc.1, dir, body, near
        );
    }

    eprintln!();
    eprintln!("== border alpha sweep (fill at {FILL_ALPHA}; all themes x presets) ==");
    eprintln!(
        "{:>5} {:>9} {:>9} {:>9} {:>9} {:>9} {:>9}",
        "alpha", "ctr-", "ctr med", "fromtint-", "pair-", "cand-", "furn-"
    );
    for border_alpha in [
        0.25, 0.30, 0.35, 0.40, 0.45, 0.50, 0.55, 0.60, 0.70, 0.85, 1.00,
    ] {
        let mut ctr: Vec<f64> = Vec::new();
        let mut from_tint = f64::MAX;
        let mut pair = f64::MAX;
        let mut cand = f64::MAX;
        let mut furn = f64::MAX;
        for r in rows {
            let hexes: Vec<String> = r.lines.iter().map(|l| l.hex.clone()).collect();
            let ui = &r.theme.ui;
            let crosshair = over(&ui.crosshair, &ui.background, CROSSHAIR_ALPHA);
            for (n, rect) in rects_at(&r.theme, &r.bars, &hexes, FILL_ALPHA, border_alpha)
                .iter()
                .enumerate()
            {
                ctr.push(rect.border_contrast);
                from_tint = from_tint.min(rect.border_from_tint);
                pair = pair.min(rect.nearest_border.1);
                if n >= 2 {
                    cand = cand.min(
                        delta_e(&rect.border, &r.bars.up).min(delta_e(&rect.border, &r.bars.down)),
                    );
                }
                furn = furn.min(
                    delta_e(&rect.border, &ui.grid)
                        .min(delta_e(&rect.border, &ui.axis))
                        .min(delta_e(&rect.border, &crosshair)),
                );
            }
        }
        ctr.sort_by(|a, b| a.total_cmp(b));
        eprintln!(
            "{:5.2} {:9.2} {:9.2} {:9.3} {:9.3} {:9.3} {:9.3}",
            border_alpha,
            ctr[0],
            ctr[ctr.len() / 2],
            from_tint,
            pair,
            cand,
            furn
        );
    }

    eprintln!();
    eprintln!("== rectangles at fill {FILL_ALPHA}, border {BORDER_ALPHA}: weakest ==");
    let mut borders: Vec<(String, &'static str, f64, f64)> = Vec::new();
    for r in rows {
        for x in &r.rects {
            borders.push((
                r.name.clone(),
                x.name,
                x.border_contrast,
                x.border_from_tint,
            ));
        }
    }
    borders.sort_by(|a, b| a.2.total_cmp(&b.2));
    eprintln!("  faintest borders against the chart:");
    for t in borders.iter().take(8) {
        eprintln!(
            "  {:28} {:7} {:.2}:1  from tint ΔE {:.3}",
            t.0, t.1, t.2, t.3
        );
    }
    borders.sort_by(|a, b| a.3.total_cmp(&b.3));
    eprintln!("  borders closest to their own fill:");
    for t in borders.iter().take(6) {
        eprintln!("  {:28} {:7} ΔE {:.3}  ({:.2}:1)", t.0, t.1, t.3, t.2);
    }
    let mut bpairs: Vec<(String, &'static str, &'static str, f64)> = Vec::new();
    for r in rows {
        for x in &r.rects {
            bpairs.push((
                r.name.clone(),
                x.name,
                x.nearest_border.0,
                x.nearest_border.1,
            ));
        }
    }
    bpairs.sort_by(|a, b| a.3.total_cmp(&b.3));
    eprintln!("  closest two borders:");
    for t in bpairs.iter().take(8) {
        eprintln!("  {:28} {:7} ~ {:7} ΔE {:.3}", t.0, t.1, t.2, t.3);
    }
    let mut tints: Vec<(String, &'static str, f64, f64)> = Vec::new();
    for r in rows {
        for x in &r.rects {
            tints.push((r.name.clone(), x.name, x.tint_contrast, x.tint_de));
        }
    }
    tints.sort_by(|a, b| a.2.total_cmp(&b.2));
    eprintln!("  faintest tints:");
    for t in tints.iter().take(8) {
        eprintln!("  {:28} {:7} {:.3}:1 ΔE {:.3}", t.0, t.1, t.2, t.3);
    }
    eprintln!("  loudest tints:");
    for t in tints.iter().rev().take(5) {
        eprintln!("  {:28} {:7} {:.3}:1 ΔE {:.3}", t.0, t.1, t.2, t.3);
    }
    let mut dirs: Vec<(String, &'static str, f64, f64)> = Vec::new();
    for r in rows {
        for x in &r.rects {
            dirs.push((r.name.clone(), x.name, x.dir_under, x.dir_bare));
        }
    }
    dirs.sort_by(|a, b| a.2.total_cmp(&b.2));
    eprintln!("  direction least readable under fill:");
    for t in dirs.iter().take(6) {
        eprintln!("  {:28} {:7} {:.3} (bare {:.3})", t.0, t.1, t.2, t.3);
    }
    let mut bodies: Vec<(String, &'static str, f64)> = Vec::new();
    for r in rows {
        for x in &r.rects {
            bodies.push((r.name.clone(), x.name, x.up_under.min(x.down_under)));
        }
    }
    bodies.sort_by(|a, b| a.2.total_cmp(&b.2));
    eprintln!("  body least visible under fill:");
    for t in bodies.iter().take(6) {
        eprintln!("  {:28} {:7} {:.2}:1", t.0, t.1, t.2);
    }

    eprintln!();
    eprintln!("== totals ==");
    let lf: usize = rows.iter().map(Row::line_failures).sum();
    let rf: usize = rows.iter().map(Row::rect_failures).sum();
    eprintln!("line guarantees broken: {lf}; rectangle: {rf}");

    fixed_comparison(rows);
}

/// What a fixed palette could do at best: seven hexes (presets 3–9) at one
/// lightness, the lightness chosen to maximise the worst contrast over every
/// theme.
fn fixed_comparison(rows: &[Row]) {
    eprintln!();
    eprintln!("== a fixed palette, best case ==");
    let hues = [262.0, 90.0, 310.0, 180.0, 50.0, 220.0];
    let mut best: Option<(f64, f64, Vec<String>)> = None;
    for step in 40..=85 {
        let l = step as f64 / 100.0;
        let mut hexes: Vec<String> = hues
            .iter()
            .map(|h| Oklch { l, c: 0.14, h: *h }.hex())
            .collect();
        hexes.push(Oklch { l, c: 0.0, h: 0.0 }.hex());
        let min = rows
            .iter()
            .flat_map(|r| {
                hexes
                    .iter()
                    .map(move |h| contrast_ratio(h, &r.theme.ui.background))
            })
            .fold(f64::MAX, f64::min);
        if best.as_ref().is_none_or(|b| min > b.1) {
            best = Some((l, min, hexes));
        }
    }
    let (l, min, hexes) = best.unwrap();
    eprintln!(
        "best lightness L={l:.2}: worst contrast {min:.2}:1  hexes {}",
        hexes.join(" ")
    );
    let mut all: Vec<f64> = rows
        .iter()
        .flat_map(|r| {
            hexes
                .iter()
                .map(move |h| contrast_ratio(h, &r.theme.ui.background))
        })
        .collect();
    all.sort_by(|a, b| a.total_cmp(b));
    eprintln!(
        "  median contrast {:.2}, below 3:1 in {} of {} cases",
        all[all.len() / 2],
        all.iter().filter(|c| **c < 3.0).count(),
        all.len()
    );
    let candle = rows
        .iter()
        .flat_map(|r| {
            hexes
                .iter()
                .map(move |h| delta_e(h, &r.bars.up).min(delta_e(h, &r.bars.down)))
        })
        .filter(|d| *d < MIN_FROM_CANDLE)
        .count();
    let furn = rows
        .iter()
        .flat_map(|r| {
            hexes.iter().map(move |h| {
                delta_e(h, &r.theme.ui.grid)
                    .min(delta_e(h, &r.theme.ui.axis))
                    .min(delta_e(h, &r.theme.ui.crosshair))
            })
        })
        .filter(|d| *d < MIN_FROM_FURNITURE)
        .count();
    eprintln!(
        "  within {MIN_FROM_CANDLE} of a candle colour: {candle} cases; within {MIN_FROM_FURNITURE} of furniture: {furn} cases"
    );
    let mut dall: Vec<f64> = rows
        .iter()
        .flat_map(|r| r.lines.iter().skip(2).map(|l| l.contrast))
        .collect();
    dall.sort_by(|a, b| a.total_cmp(b));
    eprintln!(
        "  derived presets 3-9 for comparison: worst {:.2}, median {:.2}, below 3:1 in {} of {}",
        dall[0],
        dall[dall.len() / 2],
        dall.iter().filter(|c| **c < 3.0).count(),
        dall.len()
    );
}

// ---------------------------------------------------------------------------
// The page
// ---------------------------------------------------------------------------

fn main() {
    let out = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../doc/themes/drawings.html")
        });
    let rows: Vec<Row> = themes().iter().map(|(n, t)| Row::of(n, t)).collect();
    report(&rows);

    let mut html = String::new();
    html.push_str(HEAD);
    intro(&mut html, rows.len());
    summary(&mut html, &rows);
    border_compare(&mut html, &rows);
    for row in &rows {
        section(&mut html, row);
    }
    html.push_str(FOOT);
    std::fs::write(&out, html).expect("write sheet");
    eprintln!("wrote {}", out.display());

    // A gallery of the pictures alone, short enough to screenshot.
    let mut gallery = String::new();
    gallery.push_str(HEAD);
    gallery.push_str(r#"<header style="padding-top:40px"><h1>Drawing presets</h1><p class="lede">Nine line colours and nine rectangle styles, over candles, on every theme omacharts ships. Numbers are in presets.html.</p></header>"#);
    for row in &rows {
        let _ = write!(
            gallery,
            r#"<section class="theme" style="margin-top:28px"><h2>{} <span class="mode">{}</span></h2><div class="mock">"#,
            row.name,
            row.theme.mode.label()
        );
        mock(&mut gallery, row);
        gallery.push_str("</div></section>\n");
    }
    gallery.push_str("</body></html>\n");
    let gallery_path = out.with_file_name("drawings-gallery.html");
    std::fs::write(&gallery_path, gallery).expect("write gallery");
    eprintln!("wrote {}", gallery_path.display());
}

/// Three themes, each with its rectangles drawn at the three border
/// strengths, so "subtle" can be judged by eye rather than by ratio.
fn border_compare(html: &mut String, rows: &[Row]) {
    let _ = write!(
        html,
        r#"<section class="theme" style="max-width:1760px"><h2>The border, at three strengths</h2>
<p class="caption">The same rectangles with the border at {}, at {} (what ships), and at {}. The fill is the same in all three.</p>"#,
        BORDER_COMPARE[0], BORDER_COMPARE[1], BORDER_COMPARE[2]
    );
    for name in [
        "gruvbox",
        "tokyo-night",
        "Paper (built in)",
        "catppuccin-latte",
    ] {
        let Some(row) = rows.iter().find(|r| r.name == name) else {
            continue;
        };
        let _ = write!(
            html,
            r#"<h3>{name}</h3><div class="mock"><svg width="{w}" height="{PANE_H}" viewBox="0 0 {w} {PANE_H}" shape-rendering="crispEdges">"#,
            w = PANE_W * 3.0 + 16.0
        );
        for (i, alpha) in BORDER_COMPARE.iter().enumerate() {
            let x = i as f64 * (PANE_W + 8.0);
            let closes = pane(html, row, x, 2);
            rects_over(html, row, x, &closes, *alpha);
            let _ = write!(
                html,
                r#"<text x="{}" y="{}" fill="{}" font-size="11" opacity="0.8">border {alpha}</text>"#,
                x + 8.0,
                PANE_H - TIME_AXIS_H - 48.0,
                row.theme.ui.text
            );
        }
        html.push_str("</svg></div>");
    }
    html.push_str("</section>\n");
}

fn anchor(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect()
}

fn mark(value: f64, floor: f64, decimals: usize) -> String {
    let class = if value < floor { " class=\"bad\"" } else { "" };
    format!("<td{class}>{value:.decimals$}</td>")
}

fn band_mark(value: f64, band: ContrastBand, decimals: usize) -> String {
    let class = if band.holds(value) {
        ""
    } else {
        " class=\"bad\""
    };
    format!("<td{class}>{value:.decimals$}</td>")
}

fn intro(html: &mut String, count: usize) {
    let _ = write!(
        html,
        r#"<header>
<h1>Drawing presets</h1>
<p class="lede">Nine colours for lines and nine styles for rectangles, resolved from the theme the chart is wearing, shown over candles on all {count} themes omacharts ships.</p>
</header>
<section class="prose">
<h2>The design</h2>
<p>A preset is not a hex. It is a <em>role</em> the theme fills, the way candles and indicator colours already are: <b>Up</b> and <b>Down</b> are the theme's own candle colours (<code>theme_bars</code>), <b>Blue, Amber, Violet, Teal, Orange, Cyan</b> are the theme's named swatches, and <b>Ink</b> is the neutral the monochrome bar scheme uses (<code>theme_mono_bars</code>). The same nine roles serve both drawings, so preset 4 is amber whether it is a line or a box.</p>
<p>A swatch is held to <b>{floor}:1</b> against the chart before it is used for a drawing, a floor above the indicator palette's 3.2. Only lightness moves, only when it must, and the hue the name promises is kept. Up and Down are never moved: the brief says they match the candles, and they do, to the byte.</p>
<p>A <strong>line</strong> is the colour at {lw}px. A <strong>rectangle</strong> is the colour as a fill at <b>{alpha}</b> alpha — the alpha the VWAP bands already shade with — and a {bw}px border in the same colour at <b>{balpha}</b> alpha, pulled back until it is the edge of the shape and not a line drawn around it: as drawn it has the weight of an axis line, the quietest line the chart draws that is still a line. At this strength the nine presets are told apart by hue alone, not by their edges. Nothing is opaque.</p>
<h2>How to read it</h2>
<p>Each theme is drawn at one CSS pixel per pixel: the nine lines on the left, cutting through candles, and the nine rectangles on the right, each covering a run of them. The <button type="button" onclick="document.body.classList.toggle('zoom')">2× button</button> doubles everything.</p>
<p><strong>Contrast</strong> is the WCAG ratio against the chart background; 3:1 is the floor for graphics that carry meaning and everything here is held to {floor}:1 except the two candle colours. <strong>ΔE</strong> is OKLab distance, black to white being 1.0, where 0.02 is just visible and two lines closer than 0.06 read as one; presets must be <b>{pair}</b> from each other, presets 3–9 <b>{candle}</b> from either candle, and all of them <b>{furniture}</b> from the grid, axis and crosshair. For a rectangle the questions are different: is the <strong>tint</strong> (the fill over the background) there at all — held to the same band as an axis line, {t_lo}–{t_hi}:1 — is the <strong>border</strong> as drawn still an edge: at least as loud as an axis line against the chart, {bctr}:1, and at least {btint} ΔE from the fill it encloses — and do the candles under it still read: the body against the tinted ground at least {body}:1, and up from down at least {dir} ΔE. Numbers outside a guarantee are marked <span class="bad">like this</span>.</p>
</section>
"#,
        floor = DRAWING_CONTRAST.floor,
        lw = LINE_WIDTH,
        bw = BORDER_WIDTH,
        alpha = FILL_ALPHA,
        balpha = BORDER_ALPHA,
        bctr = BORDER_CONTRAST,
        btint = MIN_BORDER_FROM_TINT,
        pair = MIN_PAIR,
        candle = MIN_FROM_CANDLE,
        furniture = MIN_FROM_FURNITURE,
        t_lo = TINT_CONTRAST.floor,
        t_hi = TINT_CONTRAST.ceiling,
        body = MIN_BODY_UNDER,
        dir = MIN_DIR_UNDER,
    );
}

fn summary(html: &mut String, rows: &[Row]) {
    let _ = write!(
        html,
        r#"<section class="prose wide"><h2>Summary</h2>
<p>For each theme: the weakest line preset against the chart, the closest two presets, the closest any of presets 3–9 comes to a candle colour, the closest any comes to the furniture; then for the rectangles, the faintest tint, the faintest border as drawn, the border closest to its own fill, the least readable direction under a fill, the dimmest body under one, and how many guarantees were broken.</p>
<div class="scroll"><table class="summary">
<tr><th rowspan="2">Theme</th><th rowspan="2">Mode</th><th colspan="5">Lines</th><th colspan="4">Rectangles</th></tr>
<tr><th>weakest</th><th>contrast</th><th>closest pair ΔE</th><th>to candle ΔE</th><th>to furniture ΔE</th><th>faintest tint</th><th>faintest border</th><th>border from tint ΔE</th><th>direction ΔE</th><th>body</th><th>broken</th></tr>
"#
    );
    for row in rows {
        let weakest = row
            .lines
            .iter()
            .min_by(|a, b| a.contrast.total_cmp(&b.contrast))
            .unwrap();
        let pair = row
            .lines
            .iter()
            .map(|l| l.nearest.1)
            .fold(f64::MAX, f64::min);
        let candle = row
            .lines
            .iter()
            .skip(2)
            .map(|l| l.from_up.min(l.from_down))
            .fold(f64::MAX, f64::min);
        let furn = row
            .lines
            .iter()
            .map(|l| l.from_grid.min(l.from_axis).min(l.from_crosshair))
            .fold(f64::MAX, f64::min);
        let tint = row
            .rects
            .iter()
            .map(|r| r.tint_contrast)
            .fold(f64::MAX, f64::min);
        let border = row
            .rects
            .iter()
            .map(|r| r.border_contrast)
            .fold(f64::MAX, f64::min);
        let from_tint = row
            .rects
            .iter()
            .map(|r| r.border_from_tint)
            .fold(f64::MAX, f64::min);
        let dir = row
            .rects
            .iter()
            .map(|r| r.dir_under)
            .fold(f64::MAX, f64::min);
        let body = row
            .rects
            .iter()
            .map(|r| r.up_under.min(r.down_under))
            .fold(f64::MAX, f64::min);
        let _ = writeln!(
            html,
            "<tr><td><a href=\"#{id}\">{name}</a></td><td>{mode}</td><td>{weak}</td>{}{}{}{}{}{}{}{}{}<td>{}</td></tr>",
            mark(
                weakest.contrast,
                if weakest.name == "Up" || weakest.name == "Down" {
                    MIN_CONTRAST
                } else {
                    DRAWING_CONTRAST.floor
                },
                2
            ),
            mark(pair, MIN_PAIR, 3),
            mark(candle, MIN_FROM_CANDLE, 3),
            mark(furn, MIN_FROM_FURNITURE, 3),
            band_mark(tint, TINT_CONTRAST, 2),
            mark(border, BORDER_CONTRAST, 2),
            mark(from_tint, MIN_BORDER_FROM_TINT, 3),
            mark(dir, MIN_DIR_UNDER, 3),
            mark(body, MIN_BODY_UNDER, 2),
            row.line_failures() + row.rect_failures(),
            id = anchor(&row.name),
            name = row.name,
            mode = row.theme.mode.label(),
            weak = weakest.name,
        );
    }
    html.push_str("</table></div></section>\n");
}

fn section(html: &mut String, row: &Row) {
    let ui = &row.theme.ui;
    let _ = write!(
        html,
        r#"<section class="theme" id="{id}">
<h2>{name} <span class="mode">{mode}</span></h2>
<div class="mock">"#,
        id = anchor(&row.name),
        name = row.name,
        mode = row.theme.mode.label(),
    );
    mock(html, row);
    html.push_str("</div>\n");

    // Chips: what each preset resolves to on this theme.
    let _ = write!(
        html,
        r#"<div class="chips" style="background:{};color:{}">"#,
        ui.background, ui.text
    );
    for (l, r) in row.lines.iter().zip(&row.rects) {
        let _ = write!(
            html,
            r#"<div class="chip"><span style="background:{hex}"></span><span class="tint" style="background:{tint};border-color:{border}"></span><div><b>{name}</b><code>{hex}</code><small>L {:.2} · C {:.3} · H {:.0}° · fill → {tint} · edge → {border}</small></div></div>"#,
            l.lch.l,
            l.lch.c,
            l.lch.h,
            hex = l.hex,
            tint = r.tint,
            border = r.border,
            name = l.name,
        );
    }
    html.push_str("</div>\n");

    html.push_str(
        r#"<div class="scroll"><table class="metrics">
<tr><th>line</th><th>hex</th><th>swatch</th><th>contrast</th><th>nearest</th><th>ΔE</th><th>ΔE up</th><th>ΔE down</th><th>on up body</th><th>on down body</th><th>ΔE grid</th><th>ΔE axis</th><th>ΔE crosshair as drawn</th></tr>
"#,
    );
    for (n, l) in row.lines.iter().enumerate() {
        let direction = n < 2;
        let floor = if direction {
            MIN_CONTRAST
        } else {
            DRAWING_CONTRAST.floor
        };
        let candle_floor = if direction { 0.0 } else { MIN_FROM_CANDLE };
        let _ = writeln!(
            html,
            "<tr><td><i style=\"background:{hex}\"></i>{}</td><td><code>{hex}</code></td><td>{}</td>{}<td>{}</td>{}{}{}<td>{:.2}</td><td>{:.2}</td>{}{}{}</tr>",
            l.name,
            l.swatch
                .as_deref()
                .map(|s| if s == l.hex {
                    "kept".to_string()
                } else {
                    format!("<code>{s}</code> lifted")
                })
                .unwrap_or_default(),
            mark(l.contrast, floor, 2),
            l.nearest.0,
            mark(l.nearest.1, MIN_PAIR, 3),
            mark(l.from_up, candle_floor, 3),
            mark(l.from_down, candle_floor, 3),
            l.on_up,
            l.on_down,
            mark(l.from_grid, MIN_FROM_FURNITURE, 3),
            mark(l.from_axis, MIN_FROM_FURNITURE, 3),
            mark(l.from_crosshair, MIN_FROM_FURNITURE, 3),
            hex = l.hex,
        );
    }
    html.push_str("</table></div>\n");

    let _ = write!(
        html,
        r#"<div class="scroll"><table class="metrics">
<tr><th>rectangle</th><th>tint over chart</th><th>tint contrast</th><th>tint ΔE</th><th>border as drawn</th><th>border contrast</th><th>ΔE from tint</th><th>nearest border</th><th>ΔE</th><th>up body under</th><th>down body under</th><th>up vs down under</th><th>bare</th><th>nearest tint</th><th>ΔE</th></tr>
"#
    );
    for r in &row.rects {
        let _ = writeln!(
            html,
            "<tr><td><i style=\"background:{tint}\"></i>{}</td><td><code>{tint}</code></td>{}<td>{:.3}</td><td><i style=\"background:{border}\"></i><code>{border}</code></td>{}{}<td>{}</td><td>{:.3}</td>{}{}{}<td>{:.3}</td><td>{}</td><td>{:.3}</td></tr>",
            r.name,
            band_mark(r.tint_contrast, TINT_CONTRAST, 2),
            r.tint_de,
            mark(r.border_contrast, BORDER_CONTRAST, 2),
            mark(r.border_from_tint, MIN_BORDER_FROM_TINT, 3),
            r.nearest_border.0,
            r.nearest_border.1,
            mark(r.up_under, MIN_BODY_UNDER, 2),
            mark(r.down_under, MIN_BODY_UNDER, 2),
            mark(r.dir_under, MIN_DIR_UNDER, 3),
            r.dir_bare,
            r.nearest_tint.0,
            r.nearest_tint.1,
            tint = r.tint,
            border = r.border,
        );
    }
    html.push_str("</table></div></section>\n");
}

const PANE_W: f64 = 560.0;
const PANE_H: f64 = 300.0;
const PRICE_AXIS_W: f64 = 52.0;
const TIME_AXIS_H: f64 = 20.0;

fn mock(html: &mut String, row: &Row) {
    let w = PANE_W * 3.0 + 16.0;
    let _ = write!(
        html,
        r#"<svg width="{w}" height="{PANE_H}" viewBox="0 0 {w} {PANE_H}" shape-rendering="crispEdges" role="img" aria-label="Drawing presets on {name}">"#,
        name = row.name,
    );
    let _ = write!(
        html,
        r#"<rect x="0" y="0" width="{w}" height="{PANE_H}" fill="{}"/>"#,
        row.theme.ui.surface
    );
    let closes = pane(html, row, 0.0, 1);
    lines_over(html, row, 0.0, &closes);
    let closes = pane(html, row, PANE_W + 8.0, 2);
    rects_over(html, row, PANE_W + 8.0, &closes, BORDER_ALPHA);
    let closes = pane(html, row, (PANE_W + 8.0) * 2.0, 3);
    ellipses_over(html, row, (PANE_W + 8.0) * 2.0, &closes);
    html.push_str("</svg>\n");
}

/// Nine ellipses, each with a label in it.
///
/// The third pane answers the question the first two cannot: a label is read
/// against the fill it sits in, and that fill is a 16% tint, so what is
/// actually behind the glyphs is a candle. Here the words are drawn the way
/// the chart draws them — the ink at the text floor, over a halo of exactly
/// the ground it was held against — so the halo can be judged by eye rather
/// than only by the numbers under the picture. The ellipse is the same fill
/// under the same edge a box has, which is the other thing worth seeing side
/// by side with the pane to its left.
fn ellipses_over(html: &mut String, row: &Row, x: f64, closes: &[(f64, f64)]) {
    let plot_w = PANE_W - PRICE_AXIS_W;
    let count = 9.0;
    let slot = (plot_w - 12.0) / count;
    let rw = (slot - 8.0).floor();
    for (i, (l, r)) in row.lines.iter().zip(&row.rects).enumerate() {
        let rx = (x + 6.0 + i as f64 * slot).floor() + 0.5;
        let first = ((rx - x - 8.0) / 9.0).max(0.0) as usize;
        let last = (first + (rw / 9.0) as usize).min(closes.len() - 1);
        let window = &closes[first.min(closes.len() - 1)..=last];
        let lo = window.iter().map(|c| c.1).fold(f64::MAX, f64::min);
        let hi = window.iter().map(|c| c.1).fold(f64::MIN, f64::max);
        let top = (lo - 26.0).max(14.0).round() + 0.5;
        let bottom = (hi + 26.0).min(PANE_H - TIME_AXIS_H - 48.0).round() + 0.5;
        let (cx, cy) = (rx + rw / 2.0, (top + bottom) / 2.0);
        let (ex, ey) = (rw / 2.0, (bottom - top) / 2.0);
        let _ = write!(
            html,
            r#"<ellipse cx="{cx:.1}" cy="{cy:.1}" rx="{ex:.1}" ry="{ey:.1}" fill="{}" fill-opacity="{FILL_ALPHA}" stroke="{}" stroke-opacity="{BORDER_ALPHA}" stroke-width="{BORDER_WIDTH}" shape-rendering="geometricPrecision"/>"#,
            l.hex, l.hex
        );
        // Which preset this is, over the shape, where the boxes in the pane
        // to the left say it — so the nine can be read down the row without
        // the names crowding each other inside shapes this narrow.
        let _ = write!(
            html,
            r#"<text x="{:.1}" y="{:.1}" fill="{}" font-size="10" font-weight="600">{}</text>"#,
            rx + 2.0,
            top - 4.0,
            l.hex,
            r.name
        );
        // And a word inside it, as the chart sets one: held to the text floor
        // against the composited fill, and stroked in that same colour before
        // it is filled. `paint-order` is what puts the stroke under the glyph
        // rather than over it, which is the whole of the halo. Short, because
        // what is being judged is whether the glyphs survive the candle
        // behind them, not how a sentence wraps.
        let ground = mix(&l.hex, &row.theme.ui.background, 1.0 - FILL_ALPHA);
        let ink = held_to(&l.hex, &ground, TEXT_CONTRAST, None);
        let _ = write!(
            html,
            r#"<text x="{cx:.1}" y="{:.1}" fill="{ink}" stroke="{ground}" stroke-width="2" paint-order="stroke" font-size="13" font-weight="600" text-anchor="middle">Note</text>"#,
            cy + 5.0,
        );
    }
}

/// One chart pane: grid, axes, labels, candles, volume. Returns the candle
/// closes in pixel space so overlays can sit where the price is.
fn pane(html: &mut String, row: &Row, x: f64, seed: u64) -> Vec<(f64, f64)> {
    let ui = &row.theme.ui;
    let (cx, cy, cw, ch) = (x, 0.0, PANE_W, PANE_H);
    let _ = write!(
        html,
        r#"<rect x="{cx}" y="{cy}" width="{cw}" height="{ch}" fill="{}"/>"#,
        ui.background
    );
    let plot_w = cw - PRICE_AXIS_W;
    let plot_h = ch - TIME_AXIS_H;
    let muted = mix(&ui.text_muted, &ui.background, 0.1);

    let mut gy = cy + 28.0;
    while gy < cy + plot_h - 8.0 {
        let _ = write!(
            html,
            r#"<rect x="{cx}" y="{gy}" width="{plot_w}" height="1" fill="{}"/>"#,
            ui.grid
        );
        let _ = write!(
            html,
            r#"<text x="{}" y="{}" fill="{muted}" font-size="10">{:.2}</text>"#,
            cx + plot_w + 6.0,
            gy + 3.5,
            190.0 - (gy - cy) / 10.0
        );
        gy += 40.0;
    }
    let mut gx = cx + 60.0;
    while gx < cx + plot_w - 20.0 {
        let _ = write!(
            html,
            r#"<rect x="{gx}" y="{cy}" width="1" height="{plot_h}" fill="{}"/>"#,
            ui.grid
        );
        let _ = write!(
            html,
            r#"<text x="{gx}" y="{}" fill="{muted}" font-size="10" text-anchor="middle">{:02} Sep</text>"#,
            cy + ch - 6.0,
            ((gx - cx) / 10.0) as u32 % 28 + 1
        );
        gx += 90.0;
    }
    let _ = write!(
        html,
        r#"<rect x="{}" y="{cy}" width="1" height="{plot_h}" fill="{}"/>"#,
        cx + plot_w,
        ui.axis
    );
    let _ = write!(
        html,
        r#"<rect x="{cx}" y="{}" width="{plot_w}" height="1" fill="{}"/>"#,
        cy + plot_h,
        ui.axis
    );

    let strip_h = 40.0;
    let _ = write!(
        html,
        r#"<rect x="{cx}" y="{}" width="{plot_w}" height="1" fill="{}"/>"#,
        cy + plot_h - strip_h,
        mix(&ui.border, &ui.background, 0.1)
    );
    let price_h = plot_h - strip_h - 4.0;

    let mut state = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
    let mut next = || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        (state % 1000) as f64 / 1000.0
    };
    let bar_w = 9.0;
    let count = ((plot_w - 16.0) / bar_w) as usize;
    let mut price = price_h * 0.5;
    let mut closes = Vec::with_capacity(count);
    for i in 0..count {
        let open = price;
        let close = (open + (next() - 0.5) * price_h * 0.14).clamp(price_h * 0.18, price_h * 0.92);
        let hi = close.max(open) + next() * price_h * 0.05;
        let lo = close.min(open) - next() * price_h * 0.05;
        let dir = Direction::of_bar(open, close);
        let bx = cx + 8.0 + i as f64 * bar_w;
        let _ = write!(
            html,
            r#"<rect x="{}" y="{}" width="1" height="{}" fill="{}"/>"#,
            bx + 4.0,
            (cy + price_h - hi).round(),
            (hi - lo).round().max(1.0),
            row.bars.outline(dir)
        );
        let (top, height) = (close.max(open), (close - open).abs().max(1.0));
        let _ = write!(
            html,
            r#"<rect x="{}" y="{}" width="7" height="{}" fill="{}"/>"#,
            bx + 1.0,
            (cy + price_h - top).round(),
            height.round().max(1.0),
            row.bars.body(dir)
        );
        let volume = 5.0 + next() * (strip_h - 10.0);
        let _ = write!(
            html,
            r#"<rect x="{}" y="{}" width="7" height="{volume}" fill="{}"/>"#,
            bx + 1.0,
            cy + plot_h - volume,
            row.bars.volume(dir)
        );
        closes.push((bx + 4.5, cy + price_h - close));
        price = close;
    }
    // The crosshair, so a preset can be judged against it.
    let (hx, hy) = (cx + plot_w * 0.71, cy + price_h * 0.36);
    let _ = write!(
        html,
        r#"<line x1="{hx}" y1="{cy}" x2="{hx}" y2="{}" stroke="{}" stroke-opacity="0.55" stroke-dasharray="3 3"/><line x1="{cx}" y1="{hy}" x2="{}" y2="{hy}" stroke="{}" stroke-opacity="0.55" stroke-dasharray="3 3"/>"#,
        cy + plot_h,
        ui.crosshair,
        cx + plot_w,
        ui.crosshair
    );
    closes
}

/// Nine lines fanned through the candles, each labelled.
fn lines_over(html: &mut String, row: &Row, x: f64, closes: &[(f64, f64)]) {
    let plot_w = PANE_W - PRICE_AXIS_W;
    let _ = closes;
    for (i, l) in row.lines.iter().enumerate() {
        // Nine levels through the candles, sloped three ways so they cross
        // each other and the bodies the way trend lines do.
        let x1 = x + 6.0;
        let x2 = x + plot_w - 6.0;
        let y1 = 34.0 + i as f64 * 22.0;
        let y2 = y1 + [-26.0, 0.0, 26.0][i % 3];
        let _ = write!(
            html,
            r#"<line x1="{x1:.1}" y1="{y1:.1}" x2="{x2:.1}" y2="{y2:.1}" stroke="{}" stroke-width="{LINE_WIDTH}" shape-rendering="geometricPrecision"/>"#,
            l.hex
        );
        let _ = write!(
            html,
            r#"<text x="{:.1}" y="{:.1}" fill="{}" font-size="10" font-weight="600" text-anchor="end">{}</text>"#,
            x2 - 2.0,
            (y2 - 3.0).clamp(10.0, PANE_H - TIME_AXIS_H - 46.0),
            l.hex,
            l.name
        );
    }
}

/// Nine rectangles, each over a run of candles.
fn rects_over(html: &mut String, row: &Row, x: f64, closes: &[(f64, f64)], border_alpha: f64) {
    let plot_w = PANE_W - PRICE_AXIS_W;
    let count = 9.0;
    let slot = (plot_w - 12.0) / count;
    let rw = (slot - 8.0).floor();
    for (i, (l, r)) in row.lines.iter().zip(&row.rects).enumerate() {
        let rx = (x + 6.0 + i as f64 * slot).floor() + 0.5;
        // Height follows the price under it so the box sits on candles.
        let first = ((rx - x - 8.0) / 9.0).max(0.0) as usize;
        let last = (first + (rw / 9.0) as usize).min(closes.len() - 1);
        let window = &closes[first.min(closes.len() - 1)..=last];
        let lo = window.iter().map(|c| c.1).fold(f64::MAX, f64::min);
        let hi = window.iter().map(|c| c.1).fold(f64::MIN, f64::max);
        let top = (lo - 22.0).max(14.0).round() + 0.5;
        let bottom = (hi + 22.0).min(PANE_H - TIME_AXIS_H - 48.0).round() + 0.5;
        let _ = write!(
            html,
            r#"<rect x="{rx}" y="{top}" width="{rw}" height="{}" fill="{}" fill-opacity="{FILL_ALPHA}" stroke="{}" stroke-opacity="{border_alpha}" stroke-width="{BORDER_WIDTH}"/>"#,
            bottom - top,
            l.hex,
            l.hex
        );
        let _ = write!(
            html,
            r#"<text x="{:.1}" y="{:.1}" fill="{}" font-size="10" font-weight="600">{}</text>"#,
            rx + 2.0,
            top - 4.0,
            l.hex,
            r.name
        );
    }
}

const HEAD: &str = r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Drawing presets</title>
<style>
:root { --bg: #141518; --panel: #1b1c21; --text: #d4d4d6; --muted: #8e8f95; --rule: #2a2b31; --bad: #c47a7a; }
* { box-sizing: border-box; }
body { margin: 0; background: var(--bg); color: var(--text); font: 15px/1.6 -apple-system, "Inter", "Segoe UI", system-ui, sans-serif; }
header, .prose { max-width: 760px; margin: 0 auto; padding: 0 24px; }
.prose.wide { max-width: 1180px; }
.prose.wide p { max-width: 712px; }
header { padding-top: 72px; }
h1 { font-size: 34px; line-height: 1.15; font-weight: 600; letter-spacing: -0.01em; margin: 0 0 16px; }
.lede { font-size: 19px; color: var(--muted); margin: 0 0 48px; }
.prose h2 { font-size: 22px; font-weight: 600; margin: 40px 0 12px; }
.prose p { margin: 0 0 16px; }
.prose button { font: inherit; font-size: 13px; color: var(--text); background: var(--panel); border: 1px solid var(--rule); border-radius: 6px; padding: 1px 8px; cursor: pointer; }
code { font-family: ui-monospace, "JetBrains Mono", Menlo, monospace; font-size: 0.92em; }
.bad { color: var(--bad); }
.scroll { overflow-x: auto; }
table { border-collapse: collapse; font-size: 13px; width: 100%; }
th, td { text-align: right; padding: 5px 10px; border-bottom: 1px solid var(--rule); white-space: nowrap; }
th { color: var(--muted); font-weight: 500; }
td:first-child, th:first-child { text-align: left; }
td i { display: inline-block; width: 10px; height: 10px; border-radius: 2px; margin-right: 7px; vertical-align: -1px; border: 1px solid rgba(255,255,255,0.08); }
.summary { margin-top: 8px; }
.summary th[colspan] { text-align: center; border-bottom: none; padding-top: 12px; }
.summary a { color: var(--text); text-decoration: none; }
.theme { max-width: 1180px; margin: 64px auto 0; padding: 0 24px; }
.theme h2 { font-size: 24px; font-weight: 600; margin: 0 0 16px; padding-top: 24px; border-top: 1px solid var(--rule); }
.mode { color: var(--muted); font-weight: 400; font-size: 16px; margin-left: 8px; }
.mock { overflow-x: auto; margin-bottom: 12px; }
.mock svg { display: block; font-family: -apple-system, "Inter", "Segoe UI", system-ui, sans-serif; }
.zoom .mock svg { zoom: 2; }
.chips { display: grid; grid-template-columns: repeat(3, 1fr); gap: 8px 14px; padding: 12px 14px; border-radius: 6px; margin-bottom: 12px; }
.chip { display: flex; gap: 8px; align-items: center; font-size: 12px; line-height: 1.3; }
.chip span { width: 22px; height: 22px; border-radius: 4px; flex: none; }
.chip span.tint { border: 1px solid; }
.chip div { display: flex; flex-direction: column; }
.chip code { opacity: .85; }
.chip small { opacity: .6; font-size: 10px; }
.metrics { font-size: 12px; margin-bottom: 10px; }
footer { max-width: 760px; margin: 96px auto 72px; padding: 0 24px; color: var(--muted); font-size: 13px; }
</style>
</head>
<body>
"#;

const FOOT: &str = r#"<footer>
<p>Generated by <code>crates/omacharts-engine/examples/drawing_sheet.rs</code>, from the theme files in <code>crates/omacharts-engine/tests/fixtures/omarchy/</code> and the built-in themes in <code>crates/omacharts-engine/src/theme.rs</code>. Every colour comes from the engine's own calls: <code>theme_bars</code>, <code>theme_mono_bars</code>, <code>Theme::swatch</code> and <code>held_to</code>.</p>
</footer>
</body>
</html>
"#;
