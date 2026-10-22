//! The frame around the charts: the gutter between two panes, and the ring on
//! the one that has focus.
//!
//! Several charts tile the window the way a tiling window manager tiles the
//! screen, and the frame borrows that idiom because it is the one an Omarchy
//! user already reads without thinking: windows sit on a gap, and the focused
//! one wears a thin border in the theme's accent. Nothing else is drawn. A
//! border around every pane would be furniture competing with the candles,
//! and the chart is the main character.
//!
//! Both colours are derived from the theme rather than taken from it, for
//! the same reason the indicator palette is. A theme's own colours were
//! chosen for a terminal, and taken as they are they fail in ways the shipped
//! themes demonstrate: Solitude's accent pulled toward its background lands
//! on the exact grey of its axis line, Lumon's accent is the blue its candles
//! wear, and Lupine's panel colour is its chart colour to within a pixel
//! value. So the gutter is the window surface, lifted only when it cannot be
//! seen, and the ring keeps the accent's hue but has its lightness chosen to
//! be seen the same amount on every theme and to stay clear of everything
//! the chart draws.

use crate::theme::{contrast_ratio, delta_e, mix, BarScheme, Direction, Oklch, Theme};

/// The gap between two panes, in CSS pixels. The whole band is the drag
/// handle, so it has to be wide enough to aim at with a pointer; it is also
/// the gap Omarchy leaves between two windows, so four charts on it look
/// like four windows on the desktop.
pub const GUTTER_WIDTH: u32 = 5;

/// The focus ring's width, in CSS pixels. Two, not one: a hairline at the
/// contrast the ring is held to is findable, and findable is not enough
/// when four panes are open and the question is which one the keyboard will
/// act on. Two pixels at that contrast are seen at a glance; the same thing
/// in a louder colour would compete with the candles.
pub const RING_WIDTH: u32 = 2;

/// How far off the chart the gutter must sit to read as a gap. Lumon's and
/// Miasma's surfaces, at about 0.04, are the quietest panels the app has
/// that still look like panels; a 4px band needs a little more than that.
pub const MIN_GUTTER: f64 = 0.05;

/// The ring's contrast against the chart background. WCAG's floor for
/// graphics that carry meaning, and what it carries is "keys go here". It is
/// the same number on every theme, which is what makes the ring equally
/// quiet on Rose Pine and on Hackerman, where the accent as-is would be 1.8:1
/// on one and 5:1 on the other.
pub const RING_CONTRAST: f64 = 3.0;

/// The most vivid the ring may be. Lupine's and Catppuccin Latte's accents
/// are well past this, and a hairline that vivid vibrates against the chart.
const MAX_RING_CHROMA: f64 = 0.14;

/// The ring must never be mistaken for the chart's own lines. The furniture
/// floor is the one the palette uses: the axis, grid and border are quiet
/// greys that a ring of similar lightness blends into, and the crosshair is
/// the accent itself, which on Rose Pine already sits at ring lightness.
/// Candles are louder and wear hues the ring can share, so they need the
/// larger distance.
pub const MIN_FROM_FURNITURE: f64 = 0.06;
pub const MIN_FROM_CANDLE: f64 = 0.08;

/// How much of the ring's colour the gutter takes on when the pointer is
/// over it. Half: enough to say "this moves", short of the band lighting up.
const HOVER_BLEND: f64 = 0.5;

/// The three colours the frame is made of, as `#rrggbb`.
///
/// All opaque. The ring could have been the accent at some alpha, but what
/// alpha composites to depends on what is behind it, and the chart's edge is
/// exactly where that is least certain. An opaque colour is what the
/// stylesheet gets and what the proof sheet draws, and they cannot disagree.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Frame {
    /// The band between two panes, which is also the drag handle.
    pub gutter: String,
    /// The same band while the pointer is over it.
    pub gutter_hover: String,
    /// The ring around the focused pane.
    pub focus: String,
}

impl Theme {
    /// The frame for this theme, with candles from `scheme`, which is what
    /// the ring has to stay clear of.
    pub fn frame(&self, scheme: &BarScheme) -> Frame {
        let gutter = gutter(self);
        let focus = focus_ring(self, scheme);
        let gutter_hover = mix(&gutter, &focus, HOVER_BLEND);
        Frame { gutter, gutter_hover, focus }
    }
}

/// The window surface, so the gap between charts is the same thing the
/// sidebar and the header sit on, the way gaps between windows show the
/// desktop. Lifted off the chart only when the theme put its surface on top
/// of it.
fn gutter(theme: &Theme) -> String {
    let ui = &theme.ui;
    if delta_e(&ui.surface, &ui.background) >= MIN_GUTTER {
        return ui.surface.clone();
    }
    let (Some(background), Some(mut gutter)) = (Oklch::of(&ui.background), Oklch::of(&ui.surface)) else {
        return ui.surface.clone();
    };
    let step = if background.l < 0.5 { 0.01 } else { -0.01 };
    for _ in 0..100 {
        if delta_e(&gutter.hex(), &ui.background) >= MIN_GUTTER {
            break;
        }
        gutter = gutter.with_lightness(gutter.l + step);
    }
    gutter.hex()
}

/// The accent's hue, at the lightness that makes a hairline of it exactly as
/// visible as it should be, and no closer to anything the chart draws than
/// an overlay is allowed to be.
///
/// Lightness is found by walking away from the background until the ring
/// clears its contrast, then walking on only as far as it takes to clear the
/// furniture and the candles. Always away: a step toward the background would
/// buy distance from a candle by giving up the visibility just established.
/// If no lightness in range is clear of everything, the ring stays at the
/// contrast it reached; being seen matters more than being unique.
fn focus_ring(theme: &Theme, scheme: &BarScheme) -> String {
    let ui = &theme.ui;
    let (Some(background), Some(accent)) = (Oklch::of(&ui.background), Oklch::of(&ui.accent)) else {
        return ui.accent.clone();
    };
    let step = if background.l < 0.5 { 0.01 } else { -0.01 };
    let mut ring = Oklch { l: background.l, c: accent.c.min(MAX_RING_CHROMA), h: accent.h };
    for _ in 0..100 {
        if contrast_ratio(&ring.hex(), &ui.background) >= RING_CONTRAST {
            break;
        }
        ring = ring.with_lightness(ring.l + step);
    }

    let clear = |candidate: Oklch| {
        let hex = candidate.hex();
        let furniture = [&ui.axis, &ui.grid, &ui.border, &ui.crosshair];
        let candles = [scheme.outline(Direction::Up), scheme.outline(Direction::Down)];
        furniture.iter().all(|f| delta_e(&hex, f) >= MIN_FROM_FURNITURE)
            && candles.iter().all(|c| delta_e(&hex, c) >= MIN_FROM_CANDLE)
    };
    let mut candidate = ring;
    while (0.0..=1.0).contains(&(candidate.l + step)) {
        if clear(candidate) {
            return candidate.hex();
        }
        candidate = candidate.with_lightness(candidate.l + step);
    }
    ring.hex()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::{builtin_themes, theme_bars, Mode, UiColors};

    fn with_ui(ui: UiColors) -> Theme {
        Theme { ui, ..builtin_themes()[0].clone() }
    }

    #[test]
    fn the_gutter_is_the_window_surface_when_that_can_be_seen() {
        // Catppuccin's surface sits well off its chart, so the gap is the
        // surface, untouched.
        let mut theme = builtin_themes()[0].clone();
        theme.ui.background = "#1e1e2e".into();
        theme.ui.surface = "#313244".into();
        assert_eq!(theme.frame(&theme_bars(&theme)).gutter, "#313244");
    }

    #[test]
    fn the_ring_is_never_the_crosshairs_colour() {
        // Rose Pine: the accent, which is also the crosshair, is already at
        // ring lightness on its cream background.
        let theme = with_ui(UiColors {
            background: "#faf4ed".into(),
            surface: "#f2e9e1".into(),
            surface_variant: "#dfdad9".into(),
            border: "#cecacd".into(),
            text: "#575279".into(),
            text_muted: "#6e6a86".into(),
            grid: "#f2e9e1".into(),
            axis: "#cecacd".into(),
            crosshair: "#56949f".into(),
            accent: "#56949f".into(),
        });
        let ring = theme.frame(&theme_bars(&theme)).focus;
        assert!(delta_e(&ring, &theme.ui.crosshair) >= MIN_FROM_FURNITURE, "{ring}");
    }

    #[test]
    fn a_surface_on_top_of_the_chart_is_lifted_off_it() {
        // Lupine's light surface is its chart to within a pixel value.
        let mut theme = builtin_themes().into_iter().find(|t| t.mode == Mode::Light).unwrap();
        theme.ui.background = "#fcfcfc".into();
        theme.ui.surface = "#fafafa".into();
        let gutter = theme.frame(&theme_bars(&theme)).gutter;
        assert_ne!(gutter, theme.ui.surface);
        assert!(delta_e(&gutter, &theme.ui.background) >= MIN_GUTTER, "{gutter}");
        // Lifted the way the theme's own surfaces go: darker on a light theme.
        assert!(Oklch::of(&gutter).unwrap().l < Oklch::of(&theme.ui.background).unwrap().l);
    }

    #[test]
    fn the_ring_keeps_the_accents_hue() {
        let theme = &builtin_themes()[0];
        let ring = Oklch::of(&theme.frame(&theme_bars(theme)).focus).unwrap();
        let accent = Oklch::of(&theme.ui.accent).unwrap();
        assert!((ring.h - accent.h).abs() < 2.0, "{} vs {}", ring.h, accent.h);
    }

    #[test]
    fn a_ring_that_lands_on_the_axis_moves_off_it() {
        // Solitude: a grey accent and a grey axis, and the accent at ring
        // lightness is the axis to the pixel.
        let theme = with_ui(UiColors {
            background: "#101315".into(),
            surface: "#1a1e21".into(),
            surface_variant: "#343d41".into(),
            border: "#4b4e55".into(),
            text: "#cacccc".into(),
            text_muted: "#8a8c8e".into(),
            grid: "#1c2022".into(),
            axis: "#4b4e55".into(),
            crosshair: "#798186".into(),
            accent: "#798186".into(),
        });
        let ring = theme.frame(&theme_bars(&theme)).focus;
        assert!(delta_e(&ring, &theme.ui.axis) >= MIN_FROM_FURNITURE, "{ring}");
        assert!(contrast_ratio(&ring, &theme.ui.background) >= RING_CONTRAST, "{ring}");
    }

    #[test]
    fn a_ring_in_the_candles_colour_moves_away_from_them() {
        // Lumon: the accent is the blue the candles wear.
        let theme = builtin_themes()[0].clone();
        let mut scheme = theme_bars(&theme);
        scheme.up = "#3a6499".into();
        scheme.down = "#2f5480".into();
        let ring = theme.frame(&scheme).focus;
        assert!(delta_e(&ring, &scheme.up) >= MIN_FROM_CANDLE, "{ring}");
        assert!(delta_e(&ring, &scheme.down) >= MIN_FROM_CANDLE, "{ring}");
    }

    #[test]
    fn the_hover_colour_sits_between_the_gutter_and_the_ring() {
        let theme = &builtin_themes()[0];
        let frame = theme.frame(&theme_bars(theme));
        assert_ne!(frame.gutter_hover, frame.gutter);
        assert!(delta_e(&frame.gutter_hover, &frame.gutter) < delta_e(&frame.focus, &frame.gutter));
    }

    #[test]
    fn a_theme_with_unparseable_colours_still_yields_a_frame() {
        let mut theme = builtin_themes()[0].clone();
        theme.ui.accent = "nonsense".into();
        let frame = theme.frame(&theme_bars(&theme));
        assert!(!frame.focus.is_empty() && !frame.gutter.is_empty());
    }
}
