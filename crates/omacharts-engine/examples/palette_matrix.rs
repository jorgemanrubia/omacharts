//! Every palette colour on every shipped theme, drawn as the chart draws it,
//! and the numbers it is judged by.
//!
//! One row per theme, the built-ins first and then each Omarchy theme in the
//! fixtures; one column per colour a study can be given by name, then the
//! market profile's open and close arrows, which are the palette's quiet
//! defaults. Each cell is the theme's chart background with a line, a filled
//! profile at the strength a volume profile is drawn at, and a figure, all in
//! that colour, and the line's contrast in the corner.
//!
//!     cargo run -p omacharts-engine --example palette_matrix -- OUT_DIR
//!
//! writes `matrix.svg` and `contrast.md` there. The SVG is rasterised by
//! whoever needs a picture (`rsvg-convert -z 2`); the Markdown is the same
//! numbers as a table. Nothing here is a test: `tests/palette_contrast.rs`
//! holds the floors.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use omacharts_engine::indicators::profile::{DESIGNED_ALPHA, OUTSIDE_SHARE};
use omacharts_engine::theme::{builtin_themes, contrast_ratio, delta_e, mix, Theme, PALETTE_NAMES};
use omacharts_engine::{omarchy, ColorChoice};

const CELL: (f64, f64) = (118.0, 72.0);
const LABEL: f64 = 132.0;
const HEAD: f64 = 28.0;

fn main() {
    let out = std::env::args().nth(1).map(PathBuf::from).expect("an output directory");
    std::fs::create_dir_all(&out).expect("output directory");
    let themes = shipped();
    std::fs::write(out.join("matrix.svg"), svg(&themes)).expect("write svg");
    std::fs::write(out.join("contrast.md"), table(&themes)).expect("write table");
    eprintln!("wrote {}", out.display());
}

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
    themes
}

/// The colours of one row, by the name a column is headed with.
fn columns(theme: &Theme) -> Vec<(String, String)> {
    let mut all: Vec<(String, String)> = PALETTE_NAMES
        .iter()
        .map(|name| (name.to_string(), ColorChoice::swatch(name).resolve(theme)))
        .collect();
    all.push(("MP open".into(), theme.tpo_open()));
    all.push(("MP close".into(), theme.tpo_close()));
    all
}

fn svg(themes: &[(String, Theme)]) -> String {
    let count = columns(&themes[0].1).len() as f64;
    let width = LABEL + count * CELL.0;
    let height = HEAD + themes.len() as f64 * CELL.1;
    let mut s = String::new();
    let _ = write!(
        s,
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" font-family="Inter, sans-serif">
<rect width="{width}" height="{height}" fill="#f4f4f4"/>
"##
    );
    for (n, (name, _)) in columns(&themes[0].1).iter().enumerate() {
        let x = LABEL + n as f64 * CELL.0 + CELL.0 / 2.0;
        let _ = writeln!(s, r##"<text x="{x}" y="19" font-size="12" font-weight="600" text-anchor="middle" fill="#222">{name}</text>"##);
    }
    for (row, (name, theme)) in themes.iter().enumerate() {
        let y = HEAD + row as f64 * CELL.1;
        let mode = if theme.mode == omacharts_engine::Mode::Light { "light" } else { "dark" };
        let _ = writeln!(
            s,
            r##"<text x="10" y="{:.1}" font-size="12" font-weight="600" fill="#222">{name}</text><text x="10" y="{:.1}" font-size="10" fill="#666">{mode}</text>"##,
            y + CELL.1 / 2.0 - 2.0,
            y + CELL.1 / 2.0 + 12.0
        );
        for (col, (_, hex)) in columns(theme).iter().enumerate() {
            cell(&mut s, LABEL + col as f64 * CELL.0, y, hex, theme);
        }
    }
    s.push_str("</svg>\n");
    s
}

/// One colour on one theme: the background, a filled profile, a line over
/// it, a figure and the line's contrast.
fn cell(s: &mut String, x: f64, y: f64, hex: &str, theme: &Theme) {
    let bg = &theme.ui.background;
    let (w, h) = CELL;
    let _ = writeln!(s, r##"<rect x="{x}" y="{y}" width="{w}" height="{h}" fill="{bg}"/>"##);
    // A profile: rows down the left, the middle ones the value area.
    let rows = [10.0, 18.0, 30.0, 44.0, 52.0, 40.0, 26.0, 14.0, 8.0];
    for (i, length) in rows.iter().enumerate() {
        let alpha = if (2..=5).contains(&i) { DESIGNED_ALPHA } else { DESIGNED_ALPHA * OUTSIDE_SHARE };
        let _ = writeln!(
            s,
            r##"<rect x="{:.1}" y="{:.1}" width="{length}" height="5" fill="{hex}" fill-opacity="{alpha:.3}"/>"##,
            x + 6.0,
            y + 6.0 + i as f64 * 6.0
        );
    }
    // A line across it.
    let points: Vec<String> = [0.0, 0.18, 0.32, 0.5, 0.66, 0.82, 1.0]
        .iter()
        .zip([40.0, 22.0, 34.0, 14.0, 30.0, 18.0, 26.0])
        .map(|(t, dy)| format!("{:.1},{:.1}", x + 6.0 + t * (w - 12.0), y + dy))
        .collect();
    let _ = writeln!(
        s,
        r##"<polyline points="{}" fill="none" stroke="{hex}" stroke-width="1.5" stroke-linejoin="round"/>"##,
        points.join(" ")
    );
    let _ = writeln!(
        s,
        r##"<text x="{:.1}" y="{:.1}" font-size="11" font-weight="600" fill="{hex}">1,250.25</text>"##,
        x + 6.0,
        y + h - 7.0
    );
    let _ = writeln!(
        s,
        r##"<text x="{:.1}" y="{:.1}" font-size="9" text-anchor="end" fill="{}">{:.1}:1</text>"##,
        x + w - 5.0,
        y + h - 7.0,
        theme.ui.text_muted,
        contrast_ratio(hex, bg)
    );
}

fn table(themes: &[(String, Theme)]) -> String {
    let names: Vec<String> = columns(&themes[0].1).into_iter().map(|(n, _)| n).collect();
    let mut s = String::new();
    let _ = writeln!(s, "Each cell: the line's WCAG contrast against the chart background, then the OKLab distance of a value-area fill (the colour at {DESIGNED_ALPHA} over the background) from the background.\n");
    let _ = writeln!(s, "| Theme | {} |", names.join(" | "));
    let _ = writeln!(s, "|---|{}", "---|".repeat(names.len()));
    for (name, theme) in themes {
        let bg = &theme.ui.background;
        let cells: Vec<String> = columns(theme)
            .iter()
            .map(|(_, hex)| {
                let fill = mix(hex, bg, 1.0 - DESIGNED_ALPHA);
                format!("{:.2} / {:.3}", contrast_ratio(hex, bg), delta_e(&fill, bg))
            })
            .collect();
        let _ = writeln!(s, "| {name} | {} |", cells.join(" | "));
    }
    s
}
