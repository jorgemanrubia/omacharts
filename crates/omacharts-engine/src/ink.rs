//! The colours words and numbers are written in.
//!
//! A theme's colours were chosen to be looked at, and text has to be read,
//! which is a stricter thing. Omarchy's palettes show the gap: Kanagawa's red
//! is a fine candle at 3.2:1 on its chart and a percentage nobody can read at
//! eleven pixels, Tokyo Night's muted grey is 3.6:1, and Catppuccin Latte's
//! green is 3.7:1 on its own background. A candle is a mark, and WCAG asks
//! 3:1 of a mark; a figure is text, and it asks 4.5:1.
//!
//! So the palette has text roles as well as marks, derived from it the way
//! [`crate::Frame`] is: from the theme and the bar scheme, once, whenever
//! either changes, with no colour of their own. Each is checked against every
//! surface it can be written on — the chart, the panels and a hovered row.
//! The theme's own colour is kept whenever it already reads;
//! otherwise only its lightness moves, in OKLCh, and only as far as it must,
//! so Kanagawa's red is still Kanagawa's red, a little lighter. Candles,
//! sparklines and other marks keep the scheme's own colours, which the
//! palette already holds to the 3:1 a mark needs.
//!
//! The bar widget writes in these, read out of `omacharts watchlist`, so the
//! panel in the bar takes every colour it draws from this one palette rather
//! than keeping a second idea of what up looks like.

use crate::theme::{held_to, mix, BarScheme, ContrastBand, Direction, Theme};

/// What text is held to against every surface it sits on: WCAG AA for body
/// text. The ceiling is the scale's own end, because text cannot be too
/// legible.
pub const TEXT_CONTRAST: ContrastBand = ContrastBand::new(4.5, 21.0);

/// What a mark that carries meaning is held to: a candle, a sparkline, a
/// control drawn as a glyph. WCAG's floor for non-text contrast.
pub const MARK_CONTRAST: f64 = 3.0;

/// How much of the text colour a hovered or selected row mixes into its
/// ground. Omarchy's own controls use the same eight per cent, so a row in the
/// bar's panel lights up the way every other row in the shell does.
pub const HOVER_TINT: f64 = 0.08;

/// The text colours for one theme and bar scheme, as `#rrggbb`.
///
/// Every one of them clears [`TEXT_CONTRAST`] against the chart, the panels
/// and [`Ink::hover`]. Derived when the theme or the scheme
/// changes and read from then on, never worked out while drawing.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Ink {
    /// Primary text: symbols, prices, names of things.
    pub text: String,
    /// Secondary text: axis labels, a symbol's long name, section headers.
    pub muted: String,
    /// A rise, written as text — a change column, a percentage.
    pub up: String,
    /// A fall, written as text.
    pub down: String,
    /// No change, written as text.
    pub flat: String,
    /// The ground of a hovered or selected row, opaque.
    pub hover: String,
}

impl Ink {
    /// The colour a move in `direction` is written in.
    pub fn direction(&self, direction: Direction) -> &str {
        match direction {
            Direction::Up => &self.up,
            Direction::Down => &self.down,
            Direction::Flat => &self.flat,
        }
    }
}

impl Theme {
    /// The ink for this theme, with direction colours from `scheme`.
    pub fn ink(&self, scheme: &BarScheme) -> Ink {
        let ui = &self.ui;
        let hover = mix(&ui.background, &ui.text, HOVER_TINT);
        let grounds = [ui.background.as_str(), ui.surface.as_str(), hover.as_str()];
        let legible = |colour: &str| legible(colour, &grounds);
        Ink {
            text: legible(&ui.text),
            muted: legible(&ui.text_muted),
            up: legible(scheme.outline(Direction::Up)),
            down: legible(scheme.outline(Direction::Down)),
            flat: legible(scheme.outline(Direction::Flat)),
            hover,
        }
    }
}

/// `colour`, made readable as text on every one of `grounds`.
///
/// The one rule for legible text. A colour that already reads on all of them
/// comes back exactly as it was. One that does not keeps its hue and chroma
/// and moves in lightness, away from the grounds, until it clears the floor
/// on each — the grounds of one theme all sit on the same side of its text,
/// so clearing the nearest clears the rest, and a second pass only confirms
/// it.
pub fn legible(colour: &str, grounds: &[&str]) -> String {
    let mut out = colour.to_string();
    for _ in 0..4 {
        let before = out.clone();
        for ground in grounds {
            out = held_to(&out, ground, TEXT_CONTRAST, None);
        }
        if out == before {
            break;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::omarchy::{derive, parse};
    use crate::theme::{builtin_themes, contrast_ratio, theme_bars, Oklch};

    fn holds(colour: &str, grounds: &[&str]) -> bool {
        grounds.iter().all(|g| contrast_ratio(colour, g) >= TEXT_CONTRAST.floor)
    }

    #[test]
    fn a_colour_that_already_reads_is_kept_to_the_byte() {
        assert_eq!(legible("#e6edf3", &["#0d1117", "#151b23"]), "#e6edf3");
    }

    #[test]
    fn a_colour_that_does_not_read_keeps_its_hue() {
        // Kanagawa's red: a good candle, a poor percentage.
        let red = "#c34043";
        let ground = "#1f1f28";
        assert!(contrast_ratio(red, ground) < TEXT_CONTRAST.floor);
        let fixed = legible(red, &[ground]);
        assert!(holds(&fixed, &[ground]), "{fixed}");
        let (was, now) = (Oklch::of(red).unwrap(), Oklch::of(&fixed).unwrap());
        assert!((was.h - now.h).abs() < 3.0, "hue moved from {} to {}", was.h, now.h);
        assert!(now.l > was.l, "on a dark ground it should lighten");
    }

    #[test]
    fn on_a_light_ground_it_darkens() {
        let green = "#40a02b"; // Catppuccin Latte's
        let fixed = legible(green, &["#eff1f5"]);
        assert!(holds(&fixed, &["#eff1f5"]), "{fixed}");
        assert!(Oklch::of(&fixed).unwrap().l < Oklch::of(green).unwrap().l);
    }

    #[test]
    fn every_ground_is_cleared_not_only_the_first() {
        let grounds = ["#1a1b26", "#24283b", "#2f3549"];
        let fixed = legible("#565f89", &grounds);
        assert!(holds(&fixed, &grounds), "{fixed}");
    }

    #[test]
    fn a_user_colour_too_dark_for_a_dark_theme_is_brought_up() {
        // A custom scheme whose up is near the background it is drawn on.
        let fixed = legible("#0a3a0a", &["#101010"]);
        assert!(holds(&fixed, &["#101010"]), "{fixed}");
    }

    #[test]
    fn a_user_colour_too_light_for_a_light_theme_is_brought_down() {
        let fixed = legible("#d0ffd0", &["#ffffff", "#f4f4f4"]);
        assert!(holds(&fixed, &["#ffffff", "#f4f4f4"]), "{fixed}");
    }

    #[test]
    fn identical_up_and_down_stay_identical() {
        // The monochrome schemes paint both directions one colour; the ink
        // must not invent a difference the chart does not show.
        let theme = builtin_themes()[0].clone();
        let mut scheme = theme_bars(&theme);
        scheme.down = scheme.up.clone();
        let ink = theme.ink(&scheme);
        assert_eq!(ink.up, ink.down);
    }

    #[test]
    fn a_theme_missing_most_of_its_keys_still_writes_legibly() {
        for text in ["mode = \"light\"\n", "mode = \"dark\"\nbackground = \"#000000\"\n", ""] {
            let theme = derive(&parse(text), "Bare");
            let ink = theme.ink(&theme_bars(&theme));
            let ui = &theme.ui;
            let grounds = [ui.background.as_str(), ui.surface.as_str(), ink.hover.as_str()];
            for (role, colour) in [("text", &ink.text), ("muted", &ink.muted), ("up", &ink.up), ("down", &ink.down), ("flat", &ink.flat)] {
                assert!(holds(colour, &grounds), "{text:?}: {role} {colour}");
            }
        }
    }

    #[test]
    fn the_hover_ground_sits_between_the_background_and_the_text() {
        let theme = builtin_themes()[0].clone();
        let ink = theme.ink(&theme_bars(&theme));
        let (bg, text) = (&theme.ui.background, &theme.ui.text);
        assert!(contrast_ratio(&ink.hover, bg) > 1.0);
        assert!(contrast_ratio(&ink.hover, text) < contrast_ratio(bg, text));
    }
}
