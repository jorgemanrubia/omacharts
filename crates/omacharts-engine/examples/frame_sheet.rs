//! Render the frame around tiled charts, and the chart's own furniture, as
//! every shipped theme draws them, onto one self-contained HTML page:
//! `doc/themes/frames.html`.
//!
//! The frame is two things: the gutter between two panes and the ring on the
//! focused one. The furniture is the grid, the axis lines and the crosshair.
//! The tests say whether each is *acceptable*. This page is for the question
//! the thresholds cannot answer: is it quiet enough, and is it still
//! unmistakable? Each theme is drawn as a tiled layout at one CSS pixel per
//! pixel, with the chart's own grid, axes, labels, candles, an overlay and
//! the crosshair, so a one-pixel ring is judged at the size it will have on
//! screen. Beside each picture are the numbers it is judged by. Under it the
//! furniture is drawn twice, as the derivation used to take it from the
//! theme and as it now holds it to a band, so the change can be checked
//! without running the app.
//!
//!     cargo run -p omacharts-engine --example frame_sheet
//!
//! writes the page in place. Pass a path to write it elsewhere.
//!
//! The colours come from `Theme::frame`, which is also what the app's
//! stylesheet is built from, and the widths are the engine's own constants.
//! Nothing here is a copy of what the app does; it is the same code.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use omacharts_engine::frame::{
    Frame, GUTTER_WIDTH, MIN_FROM_CANDLE, MIN_FROM_FURNITURE, MIN_GUTTER, RING_CONTRAST, RING_WIDTH,
};
use omacharts_engine::omarchy::{self, AXIS_CONTRAST, CROSSHAIR_CONTRAST, GRID_CONTRAST};
use omacharts_engine::theme::{
    builtin_themes, contrast_ratio, delta_e, ensure_distinct, mix, theme_bars, BarScheme,
    ContrastBand, Direction, Oklch, Theme,
};

fn main() {
    let out = std::env::args().nth(1).map(PathBuf::from).unwrap_or_else(|| {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../doc/themes/frames.html")
    });
    let themes = themes();
    let rows: Vec<Row> =
        themes.iter().map(|(name, theme, before)| Row::of(name, theme, before.as_ref())).collect();

    let mut html = String::new();
    html.push_str(HEAD);
    intro(&mut html, rows.len());
    summary(&mut html, &rows);
    furniture_summary(&mut html, &rows);
    for row in &rows {
        section(&mut html, row);
    }
    html.push_str(FOOT);
    std::fs::write(&out, html).expect("write sheet");
    eprintln!("wrote {}", out.display());
}

/// Every Omarchy theme from the fixtures, each with its furniture as the
/// derivation used to produce it, then the four we ship ourselves, which
/// are hand-tuned and have no "before".
fn themes() -> Vec<(String, Theme, Option<Theme>)> {
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
        let theme = omarchy::derive(&keys, &name);
        let before = before(&keys, &theme);
        out.push((name, theme, Some(before)));
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    for theme in builtin_themes() {
        out.push((format!("{} (built in)", theme.name), theme, None));
    }
    out
}

/// The furniture as the derivation took it from the theme before it was
/// held to a band: the lighter background nudged off the chart until it was
/// a different pixel value (RGB 0.015), and the muted and accent colours
/// exactly as they came. Reproduced here rather than kept in the engine, so
/// the comparison stays honest after the engine has moved on.
fn before(keys: &HashMap<String, String>, theme: &Theme) -> Theme {
    let pick = |names: &[&str], fallback: &str| -> String {
        names.iter().find_map(|n| keys.get(*n).cloned()).unwrap_or_else(|| fallback.to_string())
    };
    let mut before = theme.clone();
    let ui = &mut before.ui;
    ui.grid = ensure_distinct(
        &pick(&["lighter_background", "selection"], &ui.background),
        &ui.background,
        0.015,
        &ui.text,
    );
    ui.axis = pick(&["muted", "selection"], &ui.text);
    ui.border = ui.axis.clone();
    ui.crosshair = pick(&["accent", "blue", "cyan"], &ui.text);
    before
}

// ---------------------------------------------------------------------------
// Measurement
// ---------------------------------------------------------------------------

struct Row {
    name: String,
    theme: Theme,
    bars: BarScheme,
    frame: Frame,
    gutter_from_chart: f64,
    gutter_lifted: bool,
    ring: Oklch,
    ring_contrast: f64,
    /// The closest of the axis, grid, border and crosshair, and how close.
    nearest_furniture: (&'static str, f64),
    /// The closer candle colour, and how close.
    nearest_candle: (&'static str, f64),
    /// The grid, axis and crosshair against the chart, as WCAG counts it.
    grid_contrast: f64,
    axis_contrast: f64,
    crosshair_contrast: f64,
    /// The same theme with its furniture as the old derivation produced it.
    before: Option<Box<Row>>,
}

impl Row {
    fn of(name: &str, theme: &Theme, before: Option<&Theme>) -> Row {
        let bars = theme_bars(theme);
        let frame = theme.frame(&bars);
        let ui = &theme.ui;
        let furniture = [
            ("axis", &ui.axis),
            ("grid", &ui.grid),
            ("border", &ui.border),
            ("crosshair", &ui.crosshair),
        ];
        let nearest_furniture = furniture
            .iter()
            .map(|(n, hex)| (*n, delta_e(&frame.focus, hex)))
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .unwrap();
        let candles = [("up", bars.outline(Direction::Up)), ("down", bars.outline(Direction::Down))];
        let nearest_candle = candles
            .iter()
            .map(|(n, hex)| (*n, delta_e(&frame.focus, hex)))
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .unwrap();
        Row {
            name: name.to_string(),
            theme: theme.clone(),
            gutter_from_chart: delta_e(&frame.gutter, &ui.background),
            gutter_lifted: frame.gutter != ui.surface,
            ring: Oklch::of(&frame.focus).unwrap(),
            ring_contrast: contrast_ratio(&frame.focus, &ui.background),
            nearest_furniture,
            nearest_candle,
            grid_contrast: contrast_ratio(&ui.grid, &ui.background),
            axis_contrast: contrast_ratio(&ui.axis, &ui.background),
            crosshair_contrast: contrast_ratio(&ui.crosshair, &ui.background),
            before: before.map(|b| Box::new(Row::of(name, b, None))),
            frame,
            bars,
        }
    }

    fn furniture_failures(&self) -> usize {
        usize::from(!GRID_CONTRAST.holds(self.grid_contrast))
            + usize::from(!AXIS_CONTRAST.holds(self.axis_contrast))
            + usize::from(!CROSSHAIR_CONTRAST.holds(self.crosshair_contrast))
    }

    fn failures(&self) -> usize {
        usize::from(self.gutter_from_chart < MIN_GUTTER)
            + usize::from(self.ring_contrast < RING_CONTRAST)
            + usize::from(self.nearest_furniture.1 < MIN_FROM_FURNITURE)
            + usize::from(self.nearest_candle.1 < MIN_FROM_CANDLE)
    }
}

// ---------------------------------------------------------------------------
// The page
// ---------------------------------------------------------------------------

fn intro(html: &mut String, count: usize) {
    let _ = write!(
        html,
        r#"<header>
<h1>The frame around tiled charts</h1>
<p class="lede">How omacharts separates one chart from the next and marks the one that has focus, derived from the desktop theme, shown for all {count} themes it can wear.</p>
</header>
<section class="prose">
<h2>The problem</h2>
<p>One chart became several. Right-click a chart and it splits in two; right-click again and the new one closes, its space going back to its neighbour. The panes tile the window the way a tiling window manager tiles the screen, and exactly one of them has focus: it is the one the keyboard, the menus and the symbol search act on, so it has to be obvious at a glance which one that is.</p>
<p>Two things need drawing, then. Something that says where one chart stops and the next starts, which is also the handle you drag to resize them. And something that says which pane is focused. Both have to be quiet, because the chart is the main character and a border around every pane would be furniture competing with the candles. Both have to come from the theme, so they look as much a part of Nord or Rose Pine as the window chrome does. And both have to survive the themes that make everything hard: the ones that are nearly black, the ones that are nearly white, the ones with a grey accent and the ones whose accent is the colour their candles already wear.</p>
<h2>What is drawn</h2>
<p>The idiom is the window manager's, because an Omarchy user already reads it without thinking: windows sit on a gap, and the focused one wears a thin border in the theme's accent.</p>
<p>The <strong>gutter</strong> between two panes is {gutter}px of the window surface, the same colour the sidebar sits on, so the gap between charts is the window showing through, as gaps between windows show the desktop. A theme that put its surface on top of its chart (Lupine's are a pixel value apart) has the gutter lifted off the chart until it reads as a gap. The gutter is the drag handle; under the pointer it takes on half the ring's colour, enough to say it moves.</p>
<p>The <strong>focus ring</strong> is a {ring}px border inside the edge of the focused pane, between the chart and the gutter. It keeps the accent's hue, but its lightness is chosen rather than taken: walked away from the chart background until the ring has <b>{contrast}:1</b> contrast on every theme, where the accent as-is would be 1.8:1 on Rose Pine and 5:1 on Hackerman. It then walks on, only as far as it must, to be at least <b>{furniture}</b> ΔE from the axis, the grid, the border and the crosshair, and at least <b>{candle}</b> from either candle colour, so that it can never be read as a line the chart drew. Solitude's accent at ring lightness is its axis line to the pixel; Lumon's is the blue its candles wear; the ring moves off both. Chroma is capped at 0.14, which is what keeps Lupine's and Catppuccin Latte's vivid accents from vibrating as a hairline.</p>
<p>Unfocused panes get nothing at all. A lone chart, in a window that has not been split, gets nothing either.</p>
<h2>The grid, the axis and the crosshair</h2>
<p>The chart's own furniture has the same problem as the frame, and used to have only half the answer. The grid was the theme's <code>lighter_background</code>, nudged off the chart only if it was the same pixel value; the axis was the theme's <code>muted</code> colour and the crosshair its <code>accent</code>, both exactly as they came. Those colours were chosen for a terminal's selection and comments, not for hairlines across a chart, and on a pale chart a terminal's selection grey is a lattice: White's grid was a mid grey at 1.8:1 on white, Flexoki Light's 1.24:1 on cream, and Lupine's, at 1.04:1, was not there at all. The test that guarded this enforced a floor — the grid must be a different pixel value from the chart and quieter than the text — and on a light theme the text is near-black on near-white, so "quieter than the text" let a lattice through.</p>
<p>Each of the three is now held to a band of WCAG contrast against the chart, the same band on every theme because the ratio already accounts for the ground. The <strong>grid</strong> is felt rather than read: <b>{grid_lo}–{grid_hi}:1</b>, which is where the four hand-tuned built-in themes put theirs. The <strong>axis</strong> lines are louder, because they mark where the plot ends, and still a line rather than a bar: <b>{axis_lo}–{axis_hi}:1</b>, where Ethereal's and Vantablack's greys taken as they were made an axis at nearly 5:1. The <strong>crosshair</strong> is a pointer, so its floor is WCAG's <b>{xhair_lo}:1</b> for graphics that carry meaning, the same as the ring's, and its ceiling is wide, <b>{xhair_hi}:1</b>, which is where it stops being a pointer and becomes the text colour, as Kanagawa's accent literally is. The chart's border is the axis colour, so the rule above an indicator strip has the weight of the axis beside it.</p>
<p>A theme's own colour is kept to the byte whenever it is already inside its band; most are. One that is not moves in lightness only, keeping its hue, and only as far as it must: a loud colour walks toward the chart until it is just inside the ceiling, a quiet one is rebuilt from the chart's own lightness, walking away until it reaches the floor. The grid may also be no more vivid than the chart it sits on, by more than a whisper. Contrast is used rather than ΔE for the same reason the overlays use it — the question is whether a line can be seen on its ground, which is a question about light — and because OKLab has no toe at black, so it calls a near-black grid on a black chart a tenth of the way to white, which is not what the eye sees.</p>
<h2>How to read the pictures</h2>
<p>Each theme is drawn as a tiled layout at one CSS pixel per pixel: one tall pane and two stacked beside it, so both a vertical and a horizontal gutter appear, with the window's own controls — the watchlist toggle and the main menu — in the top-right corner, where they sit over the chart when the watchlist is closed; there is no header bar. The left pane has focus and shows the crosshair; the gutter between the two right-hand panes is shown as it looks under the pointer. Candles, the grid, the axes, the labels, the overlay and the volume strip are the theme's own, drawn the way the chart draws them, including the hairline the chart rules above every indicator strip, which is the line a gutter must not be confused with. The <button type="button" onclick="document.body.classList.toggle('zoom')">2× button</button> doubles everything, which is roughly what a HiDPI screen does.</p>
<p>Under each layout the same chart is drawn twice more, smaller and without the frame: with the grid, axis and crosshair as the derivation used to take them from the theme, and as it holds them now. The built-in themes are hand-tuned and have no before. The crosshair is drawn opaque here; the app draws it at about half strength, so on screen it is quieter than on this page by the same amount on every theme.</p>
<p><strong>ΔE</strong> is the distance between two colours in OKLab, where black to white is 1.0 and about 0.02 is the least an eye can see. <strong>Contrast</strong> is the WCAG ratio against the chart background. Numbers below a guarantee, or outside a band, are marked <span class="bad">like this</span>.</p>
</section>
"#,
        gutter = GUTTER_WIDTH,
        ring = RING_WIDTH,
        contrast = RING_CONTRAST,
        furniture = MIN_FROM_FURNITURE,
        candle = MIN_FROM_CANDLE,
        grid_lo = GRID_CONTRAST.floor,
        grid_hi = GRID_CONTRAST.ceiling,
        axis_lo = AXIS_CONTRAST.floor,
        axis_hi = AXIS_CONTRAST.ceiling,
        xhair_lo = CROSSHAIR_CONTRAST.floor,
        xhair_hi = CROSSHAIR_CONTRAST.ceiling,
    );
}

fn summary(html: &mut String, rows: &[Row]) {
    let _ = write!(
        html,
        r#"<section class="prose wide"><h2>Summary</h2>
<p>For each theme: how far the gutter sits off the chart (at least {MIN_GUTTER}, and whether it had to be lifted off the window surface to get there), the ring's contrast, the closest the ring comes to any of the chart's own lines and to either candle colour, and how many guarantees were broken.</p>
<div class="scroll"><table class="summary">
<tr><th>Theme</th><th>Mode</th><th>Gutter</th><th>ΔE from chart</th><th>Lifted</th><th>Ring</th><th>Contrast</th><th>Nearest furniture</th><th>ΔE</th><th>Nearer candle</th><th>ΔE</th><th>Broken</th></tr>
"#
    );
    for row in rows {
        let _ = writeln!(
            html,
            "<tr><td><a href=\"#{id}\">{name}</a></td><td>{mode}</td><td>{gutter}</td>{}<td>{}</td><td>{ring}</td>{}<td>{}</td>{}<td>{}</td>{}<td>{}</td></tr>",
            mark(row.gutter_from_chart, MIN_GUTTER, 3),
            if row.gutter_lifted { "yes" } else { "" },
            mark(row.ring_contrast, RING_CONTRAST, 2),
            row.nearest_furniture.0,
            mark(row.nearest_furniture.1, MIN_FROM_FURNITURE, 3),
            row.nearest_candle.0,
            mark(row.nearest_candle.1, MIN_FROM_CANDLE, 3),
            row.failures(),
            id = anchor(&row.name),
            name = row.name,
            mode = row.theme.mode.label(),
            gutter = chip(&row.frame.gutter),
            ring = chip(&row.frame.focus),
        );
    }
    html.push_str("</table></div></section>\n");
}

/// Before and after, for the grid, the axis and the crosshair.
fn furniture_summary(html: &mut String, rows: &[Row]) {
    let _ = write!(
        html,
        r#"<section class="prose wide"><h2>The furniture, before and after</h2>
<p>For each theme: the contrast of the grid, the axis and the crosshair against the chart, as the derivation used to take them from the theme and as it holds them now, and how many bands the current colours break. The bands are {}–{}:1 for the grid, {}–{}:1 for the axis and {}–{}:1 for the crosshair.</p>
<div class="scroll"><table class="summary">
<tr><th>Theme</th><th>Mode</th><th>Grid before</th><th>after</th><th>Axis before</th><th>after</th><th>Crosshair before</th><th>after</th><th>Broken</th></tr>
"#,
        GRID_CONTRAST.floor,
        GRID_CONTRAST.ceiling,
        AXIS_CONTRAST.floor,
        AXIS_CONTRAST.ceiling,
        CROSSHAIR_CONTRAST.floor,
        CROSSHAIR_CONTRAST.ceiling,
    );
    for row in rows {
        let was = |pick: fn(&Row) -> f64, band: ContrastBand| match &row.before {
            Some(before) => band_mark(pick(before), band, 2),
            None => "<td></td>".to_string(),
        };
        let _ = writeln!(
            html,
            "<tr><td><a href=\"#{id}\">{name}</a></td><td>{mode}</td>{}{}{}{}{}{}<td>{}</td></tr>",
            was(|r| r.grid_contrast, GRID_CONTRAST),
            band_mark(row.grid_contrast, GRID_CONTRAST, 2),
            was(|r| r.axis_contrast, AXIS_CONTRAST),
            band_mark(row.axis_contrast, AXIS_CONTRAST, 2),
            was(|r| r.crosshair_contrast, CROSSHAIR_CONTRAST),
            band_mark(row.crosshair_contrast, CROSSHAIR_CONTRAST, 2),
            row.furniture_failures(),
            id = anchor(&row.name),
            name = row.name,
            mode = row.theme.mode.label(),
        );
    }
    html.push_str("</table></div></section>\n");
}

fn anchor(name: &str) -> String {
    name.chars().map(|c| if c.is_alphanumeric() { c } else { '-' }).collect()
}

fn chip(hex: &str) -> String {
    format!("<i style=\"background:{hex}\"></i><code>{hex}</code>")
}

fn mark(value: f64, floor: f64, decimals: usize) -> String {
    let class = if value < floor { " class=\"bad\"" } else { "" };
    format!("<td{class}>{value:.decimals$}</td>")
}

fn band_mark(value: f64, band: ContrastBand, decimals: usize) -> String {
    let class = if band.holds(value) { "" } else { " class=\"bad\"" };
    format!("<td{class}>{value:.decimals$}</td>")
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
    let _ = write!(
        html,
        r#"</div>
<div class="scroll"><table class="metrics">
<tr><th>chart</th><th>surface</th><th>gutter</th><th>ΔE from chart</th><th>ΔE from strip rule</th><th>hover</th><th>accent</th><th>ring</th><th>L</th><th>C</th><th>H</th><th>contrast</th><th>ΔE axis</th><th>ΔE grid</th><th>ΔE border</th><th>ΔE crosshair</th><th>ΔE up</th><th>ΔE down</th></tr>
<tr><td>{}</td><td>{}</td><td>{}</td>{}<td>{:.3}</td><td>{}</td><td>{}</td><td>{}</td><td>{:.2}</td><td>{:.3}</td><td>{:.0}</td>{}{}{}{}{}{}{}</tr>
</table></div>
"#,
        chip(&ui.background),
        chip(&ui.surface),
        chip(&row.frame.gutter),
        mark(row.gutter_from_chart, MIN_GUTTER, 3),
        delta_e(&row.frame.gutter, &mix(&ui.border, &ui.background, 0.1)),
        chip(&row.frame.gutter_hover),
        chip(&ui.accent),
        chip(&row.frame.focus),
        row.ring.l,
        row.ring.c,
        row.ring.h,
        mark(row.ring_contrast, RING_CONTRAST, 2),
        mark(delta_e(&row.frame.focus, &ui.axis), MIN_FROM_FURNITURE, 3),
        mark(delta_e(&row.frame.focus, &ui.grid), MIN_FROM_FURNITURE, 3),
        mark(delta_e(&row.frame.focus, &ui.border), MIN_FROM_FURNITURE, 3),
        mark(delta_e(&row.frame.focus, &ui.crosshair), MIN_FROM_FURNITURE, 3),
        mark(delta_e(&row.frame.focus, row.bars.outline(Direction::Up)), MIN_FROM_CANDLE, 3),
        mark(delta_e(&row.frame.focus, row.bars.outline(Direction::Down)), MIN_FROM_CANDLE, 3),
    );
    furniture(html, row);
    html.push_str("</section>\n");
}

/// The chart's own furniture, before and after, at true size.
fn furniture(html: &mut String, row: &Row) {
    html.push_str(r#"<h3>Grid, axis and crosshair</h3><div class="pair">"#);
    if let Some(before) = &row.before {
        strip(html, before, "Before");
    }
    strip(html, row, if row.before.is_some() { "After" } else { "As shipped" });
    html.push_str("</div>");

    let line = |what: &str, pick: fn(&Row) -> &str, ratio: fn(&Row) -> f64, band: ContrastBand| {
        let ui = &row.theme.ui;
        let was = match &row.before {
            Some(before) => format!(
                "<td>{}</td>{}<td>{:.3}</td>",
                chip(pick(before)),
                band_mark(ratio(before), band, 2),
                delta_e(pick(before), &ui.background)
            ),
            None => "<td></td><td></td><td></td>".to_string(),
        };
        format!(
            "<tr><td>{what}</td>{was}<td>{}</td>{}<td>{:.3}</td><td>{}–{}</td></tr>\n",
            chip(pick(row)),
            band_mark(ratio(row), band, 2),
            delta_e(pick(row), &ui.background),
            band.floor,
            band.ceiling
        )
    };
    let _ = write!(
        html,
        r#"<div class="scroll"><table class="metrics">
<tr><th></th><th>before</th><th>contrast</th><th>ΔE</th><th>after</th><th>contrast</th><th>ΔE</th><th>band</th></tr>
{}{}{}</table></div>
"#,
        line("grid", |r| &r.theme.ui.grid, |r| r.grid_contrast, GRID_CONTRAST),
        line("axis", |r| &r.theme.ui.axis, |r| r.axis_contrast, AXIS_CONTRAST),
        line("crosshair", |r| &r.theme.ui.crosshair, |r| r.crosshair_contrast, CROSSHAIR_CONTRAST),
    );
}

const STRIP_W: f64 = 372.0;
const STRIP_H: f64 = 220.0;

/// One unframed pane with the crosshair, captioned.
fn strip(html: &mut String, row: &Row, caption: &str) {
    let _ = write!(
        html,
        r#"<figure><svg width="{STRIP_W}" height="{STRIP_H}" viewBox="0 0 {STRIP_W} {STRIP_H}" shape-rendering="crispEdges" role="img" aria-label="{caption}: the chart furniture for {name}">"#,
        name = row.name,
    );
    pane(html, row, 0.0, 0.0, STRIP_W, STRIP_H, "AAPL  ·  1D", 1, false, true);
    let _ = write!(html, "</svg><figcaption>{caption}</figcaption></figure>");
}

// ---------------------------------------------------------------------------
// The picture
// ---------------------------------------------------------------------------

const WIDTH: f64 = 760.0;
const HEIGHT: f64 = 400.0;
const PRICE_AXIS_W: f64 = 64.0;
const TIME_AXIS_H: f64 = 24.0;

/// One tall pane on the left, two stacked on the right, the gutters between
/// them, the ring on the left one, and the window's two corner controls over
/// the top-right pane. Sized in whole pixels
/// and drawn with crisp edges, so a one-pixel line is one pixel.
fn mock(html: &mut String, row: &Row) {
    let ui = &row.theme.ui;
    let frame = &row.frame;
    let gutter = GUTTER_WIDTH as f64;
    let total_h = HEIGHT;
    let _ = write!(
        html,
        r#"<svg width="{WIDTH}" height="{total_h}" viewBox="0 0 {WIDTH} {total_h}" shape-rendering="crispEdges" role="img" aria-label="Tiled charts for {name}">"#,
        name = row.name,
    );
    // The window: everything the panes do not cover is the surface.
    let _ = write!(html, r#"<rect x="0" y="0" width="{WIDTH}" height="{total_h}" fill="{}"/>"#, ui.surface);

    let left_w = ((WIDTH - gutter) / 2.0).floor();
    let right_x = left_w + gutter;
    let right_w = WIDTH - right_x;
    let top_h = ((HEIGHT - gutter) / 2.0).floor();
    let bottom_y = top_h + gutter;
    let bottom_h = HEIGHT - top_h - gutter;

    // Gutters. The band between the right-hand panes is shown hovered.
    let _ = write!(
        html,
        r#"<rect x="{left_w}" y="0" width="{gutter}" height="{HEIGHT}" fill="{}"/>"#,
        frame.gutter
    );
    let _ = write!(
        html,
        r#"<rect x="{right_x}" y="{top_h}" width="{right_w}" height="{gutter}" fill="{}"/>"#,
        frame.gutter_hover
    );

    pane(html, row, 0.0, 0.0, left_w, HEIGHT, "AAPL  ·  1D", 1, true, true);
    pane(html, row, right_x, 0.0, right_w, top_h, "NVDA  ·  1H", 2, false, false);
    pane(html, row, right_x, bottom_y, right_w, bottom_h, "BTC-USD  ·  15m", 3, false, false);
    corner(html, row);
    html.push_str("</svg>\n");
}

/// The window's controls, as `.window-corner` in `src/theming.rs` lays them
/// out: two 24px buttons with 3px around them, at the window's top-right, in
/// the text colour at three quarters strength. With the watchlist closed they
/// sit over the top-right pane, on top of the price axis, which is the
/// overlap this page exists to show.
fn corner(html: &mut String, row: &Row) {
    let ui = &row.theme.ui;
    let cy = 15.0;
    let menu_x = WIDTH - 6.0 - 12.0;
    let toggle_x = menu_x - 24.0;
    let _ = write!(html, r#"<g fill="{0}" stroke="{0}" opacity="0.75">"#, ui.text);
    // The watchlist toggle: a frame with its right third filled.
    let _ = write!(
        html,
        r#"<rect x="{}" y="{}" width="14" height="11" rx="1.5" fill="none" stroke-width="1.5"/><rect x="{}" y="{}" width="5" height="11" stroke="none"/>"#,
        toggle_x - 7.0,
        cy - 5.5,
        toggle_x + 2.0,
        cy - 5.5
    );
    // The main menu.
    for dy in [-4.0, 0.0, 4.0] {
        let _ = write!(
            html,
            r#"<rect x="{}" y="{}" width="14" height="1.5" stroke="none"/>"#,
            menu_x - 7.0,
            cy + dy - 0.75
        );
    }
    html.push_str("</g>");
}

/// One pane: the ring if focused, then the chart inside it, drawn the way
/// `crates/omacharts-chart/src/view.rs` draws it, with the crosshair if `pointer`.
#[allow(clippy::too_many_arguments)]
fn pane(
    html: &mut String,
    row: &Row,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    label: &str,
    seed: u64,
    focused: bool,
    pointer: bool,
) {
    let ui = &row.theme.ui;
    let ring = RING_WIDTH as f64;
    let _ = write!(html, r#"<rect x="{x}" y="{y}" width="{w}" height="{h}" fill="{}"/>"#, ui.background);
    if focused {
        let _ = write!(
            html,
            r#"<rect x="{}" y="{}" width="{}" height="{}" fill="none" stroke="{}" stroke-width="{ring}"/>"#,
            x + ring / 2.0,
            y + ring / 2.0,
            w - ring,
            h - ring,
            row.frame.focus
        );
    }
    // The chart's own area, inside the ring.
    let (cx, cy, cw, ch) = (x + ring, y + ring, w - 2.0 * ring, h - 2.0 * ring);
    let plot_w = cw - PRICE_AXIS_W;
    let plot_h = ch - TIME_AXIS_H;
    let muted = mix(&ui.text_muted, &ui.background, 0.1);

    // Grid, then axes, then labels: the order the chart uses.
    let mut gy = cy + 28.0;
    while gy < cy + plot_h - 8.0 {
        let _ = write!(html, r#"<rect x="{cx}" y="{gy}" width="{plot_w}" height="1" fill="{}"/>"#, ui.grid);
        let _ = write!(
            html,
            r#"<text x="{}" y="{}" fill="{muted}" font-size="11">{:.2}</text>"#,
            cx + plot_w + 6.0,
            gy + 4.0,
            190.0 - (gy - cy) / 10.0
        );
        gy += 40.0;
    }
    let mut gx = cx + 60.0;
    while gx < cx + plot_w - 20.0 {
        let _ = write!(html, r#"<rect x="{gx}" y="{cy}" width="1" height="{plot_h}" fill="{}"/>"#, ui.grid);
        let _ = write!(
            html,
            r#"<text x="{gx}" y="{}" fill="{muted}" font-size="11" text-anchor="middle">{:02} Sep</text>"#,
            cy + ch - 7.0,
            ((gx - cx) / 10.0) as u32 % 28 + 1
        );
        gx += 90.0;
    }
    let _ = write!(html, r#"<rect x="{}" y="{cy}" width="1" height="{plot_h}" fill="{}"/>"#, cx + plot_w, ui.axis);
    let _ = write!(html, r#"<rect x="{cx}" y="{}" width="{plot_w}" height="1" fill="{}"/>"#, cy + plot_h, ui.axis);

    // The volume strip, with the rule the chart draws above every indicator
    // strip: the border colour at 0.9. A gutter has to look like something
    // else than this, or a split reads as one more indicator.
    let strip_h = 52.0;
    let strip_y = cy + plot_h - strip_h;
    let _ = write!(
        html,
        r#"<rect x="{cx}" y="{strip_y}" width="{plot_w}" height="1" fill="{}"/>"#,
        mix(&ui.border, &ui.background, 0.1)
    );
    let price_h = plot_h - strip_h - 4.0;

    // Candles: a deterministic random walk, so the page is stable between runs.
    let mut state = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
    let mut next = || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        (state % 1000) as f64 / 1000.0
    };
    let bar_w = 9.0;
    let count = ((plot_w - 20.0) / bar_w) as usize;
    let mut price = price_h * 0.55;
    let mut closes = Vec::with_capacity(count);
    for i in 0..count {
        let open = price;
        let close = (open + (next() - 0.5) * price_h * 0.12).clamp(price_h * 0.15, price_h * 0.9);
        let hi = close.max(open) + next() * price_h * 0.04;
        let lo = close.min(open) - next() * price_h * 0.04;
        let dir = Direction::of_bar(open, close);
        let bx = cx + 10.0 + i as f64 * bar_w;
        let _ = write!(
            html,
            r#"<rect x="{}" y="{}" width="1" height="{}" fill="{}"/>"#,
            bx + 4.0,
            cy + price_h - hi,
            hi - lo,
            row.bars.outline(dir)
        );
        let body = row.bars.body(dir);
        let (top, height) = (close.max(open), (close - open).abs().max(1.0));
        if body.len() < 9 {
            let _ = write!(
                html,
                r#"<rect x="{}" y="{}" width="7" height="{height}" fill="{body}"/>"#,
                bx + 1.0,
                cy + price_h - top
            );
        }
        let volume = 6.0 + next() * (strip_h - 12.0);
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

    // One overlay, a smoothed line through the closes, in the first series colour.
    let overlay = row.theme.series(0);
    let mut points = String::new();
    for i in 0..closes.len() {
        let lo = i.saturating_sub(4);
        let window = &closes[lo..=i];
        let avg = window.iter().map(|p| p.1).sum::<f64>() / window.len() as f64;
        let _ = write!(points, "{:.1},{:.1} ", closes[i].0, avg);
    }
    let _ = write!(
        html,
        r#"<polyline points="{points}" fill="none" stroke="{overlay}" stroke-width="1.5" shape-rendering="geometricPrecision"/>"#
    );

    // The crosshair, where the pointer is.
    if pointer {
        let (hx, hy) = (cx + plot_w * 0.62, cy + price_h * 0.42);
        let _ = write!(
            html,
            r#"<line x1="{hx}" y1="{cy}" x2="{hx}" y2="{}" stroke="{}" stroke-dasharray="3 3"/><line x1="{cx}" y1="{hy}" x2="{}" y2="{hy}" stroke="{}" stroke-dasharray="3 3"/>"#,
            cy + plot_h,
            ui.crosshair,
            cx + plot_w,
            ui.crosshair
        );
    }

    // The readout, over the top left.
    let _ = write!(
        html,
        r#"<text x="{}" y="{}" fill="{}" font-size="12" font-weight="700">{label}</text>"#,
        cx + 12.0,
        cy + 19.0,
        ui.text
    );
}

const HEAD: &str = r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>The frame around tiled charts</title>
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
.prose a { color: var(--text); text-decoration: underline; text-decoration-color: var(--muted); }
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
.summary a { color: var(--text); text-decoration: none; }
.theme { max-width: 1180px; margin: 72px auto 0; padding: 0 24px; }
.theme h2 { font-size: 24px; font-weight: 600; margin: 0 0 20px; padding-top: 24px; border-top: 1px solid var(--rule); }
.mode { color: var(--muted); font-weight: 400; font-size: 16px; margin-left: 8px; }
.mock { overflow-x: auto; margin-bottom: 14px; }
.mock svg { display: block; font-family: -apple-system, "Inter", "Segoe UI", system-ui, sans-serif; }
.zoom .mock svg { zoom: 2; }
.metrics { font-size: 12px; }
.theme h3 { font-size: 15px; font-weight: 600; margin: 28px 0 10px; color: var(--muted); }
.pair { display: flex; flex-wrap: wrap; gap: 16px; margin-bottom: 14px; }
.pair figure { margin: 0; }
.pair svg { display: block; font-family: -apple-system, "Inter", "Segoe UI", system-ui, sans-serif; }
.zoom .pair svg { zoom: 2; }
.pair figcaption { font-size: 12px; color: var(--muted); margin-top: 6px; }
footer { max-width: 760px; margin: 96px auto 72px; padding: 0 24px; color: var(--muted); font-size: 13px; }
@media (max-width: 640px) { header { padding-top: 40px; } h1 { font-size: 28px; } }
</style>
</head>
<body>
"#;

const FOOT: &str = r#"<footer>
<p>Generated by <code>crates/omacharts-engine/examples/frame_sheet.rs</code> from the theme files in <code>crates/omacharts-engine/tests/fixtures/omarchy/</code> and the built-in themes in <code>crates/omacharts-engine/src/theme.rs</code>. The frame colours are <code>Theme::frame</code>, the same call the app's stylesheet is built from, and the widths are the engine's own constants; the furniture is <code>omarchy::derive</code>, the same call the app makes, and its "before" is a reproduction of the derivation this replaced. To regenerate after a change to the derivation or a new theme: <code>cargo run -p omacharts-engine --example frame_sheet</code>.</p>
</footer>
</body>
</html>
"#;
