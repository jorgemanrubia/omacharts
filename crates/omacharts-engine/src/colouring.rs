//! How a study with a colouring is painted, one model for every one of them:
//! a volume pane, a volume profile and its drawing, the cumulative delta,
//! the volume delta, the footprint and large orders.
//!
//! A study's **Colouring** is either unset, the house colouring (a volume
//! pane's, the chart's candles), one of the presets, or **Custom**. With a
//! preset its colours are the preset's, and nothing of the study's own
//! stands over them. With Custom each colour the study paints with — its
//! *roles*, a rise and a fall, a buy and a sell — is its own, picked one by
//! one. Choosing Custom starts each role at the colour it was drawn in just
//! before, so nothing moves until one is picked; choosing a preset again
//! lets the picked colours go, and the preset is what is drawn.
//!
//! A study saved before Custom was a choice, with a colour picked over its
//! colouring, reads as Custom: its picked roles as they were, the others as
//! its colouring drew them. Nothing is drawn differently after the upgrade.
//!
//! How solid each role is drawn is kept whichever colouring is chosen.

use std::borrow::Cow;

use serde::{Deserialize, Serialize};

use crate::theme::{BarColouring, ColorChoice, Theme};

/// What a study's Colouring row says: a preset, or its own colours.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Colouring {
    Scheme(BarColouring),
    Custom,
}

/// The key Custom is stored and typed as.
pub const CUSTOM_KEY: &str = "custom";
/// What Custom is called on the Colouring row.
pub const CUSTOM_LABEL: &str = "Custom";

impl Colouring {
    pub fn key(self) -> &'static str {
        match self {
            Colouring::Scheme(scheme) => scheme.key(),
            Colouring::Custom => CUSTOM_KEY,
        }
    }

    /// The colouring a stored or typed key names: a preset's, or Custom.
    pub fn from_key(key: &str) -> Option<Colouring> {
        match key {
            CUSTOM_KEY => Some(Colouring::Custom),
            key => BarColouring::from_key(key).map(Colouring::Scheme),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Colouring::Scheme(scheme) => scheme.label(),
            Colouring::Custom => CUSTOM_LABEL,
        }
    }

    /// The preset, if it is one.
    pub fn scheme(self) -> Option<BarColouring> {
        match self {
            Colouring::Scheme(scheme) => Some(scheme),
            Colouring::Custom => None,
        }
    }
}

impl From<BarColouring> for Colouring {
    fn from(scheme: BarColouring) -> Colouring {
        Colouring::Scheme(scheme)
    }
}

impl Serialize for Colouring {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.key())
    }
}

impl<'de> Deserialize<'de> for Colouring {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let key = String::deserialize(deserializer)?;
        Colouring::from_key(&key).ok_or_else(|| serde::de::Error::custom(format!("{key:?} is not a colouring")))
    }
}

/// Is a study of `colouring`, with `colours` picked for its roles, in its
/// own colours? Chosen as Custom, or saved with a colour picked before
/// Custom could be chosen.
pub fn is_custom(colouring: Option<Colouring>, colours: &[Option<&ColorChoice>]) -> bool {
    colouring == Some(Colouring::Custom) || colours.iter().any(Option::is_some)
}

/// The preset whose colours stand for a role with none of its own: the
/// study's preset, or `None` for the house colouring. A Custom study's
/// roles all have their own once Custom is chosen; one saved before Custom
/// was a choice draws the roles it never picked as its colouring did.
pub fn base(colouring: Option<Colouring>) -> Option<BarColouring> {
    colouring.and_then(Colouring::scheme)
}

/// The colour role `own` is drawn in: its own while the study is Custom,
/// else `base`, the colour its preset gives it. Borrowed unless a colour of
/// its own is resolved, so a frame of a preset allocates nothing.
pub fn drawn<'a>(custom: bool, own: Option<&ColorChoice>, base: &'a str, theme: &Theme) -> Cow<'a, str> {
    match own.filter(|_| custom) {
        Some(choice) => Cow::Owned(choice.resolve(theme)),
        None => Cow::Borrowed(base),
    }
}

/// What a study's colours are, to be read or changed by the one set of
/// rows every study's settings show, and by the command line: its
/// colouring, and each role's name, colour picked and how solid it is.
pub struct ColoursMut<'a> {
    pub colouring: &'a mut Option<Colouring>,
    /// Each role's name, as its row is titled: "Up" and "Down", "Buying"
    /// and "Selling".
    pub labels: &'static [&'static str],
    pub colours: Vec<&'a mut Option<ColorChoice>>,
    /// How solid each role is drawn; empty for a study whose strength is
    /// set elsewhere, as a profile's is.
    pub alphas: Vec<&'a mut Option<f64>>,
    /// A strength over all the roles, for a study that has one, as volume
    /// does: the one Opacity row sets it.
    pub opacity: Option<&'a mut f64>,
}

impl ColoursMut<'_> {
    pub fn is_custom(&self) -> bool {
        let colours: Vec<Option<&ColorChoice>> = self.colours.iter().map(|c| c.as_ref()).collect();
        is_custom(*self.colouring, &colours)
    }

    /// What the Colouring row shows: Custom for one saved with a colour
    /// picked before Custom was a choice, too.
    pub fn shown(&self) -> Option<Colouring> {
        match self.is_custom() {
            true => Some(Colouring::Custom),
            false => *self.colouring,
        }
    }

    /// Choose `chosen`. Custom starts each role without a colour of its own
    /// at `drawn`, the colour it was drawn in a moment ago, so nothing
    /// moves; a preset, or the house colouring, lets every picked colour go
    /// and is drawn as it is. How solid each role is stays as it was.
    pub fn pick(&mut self, chosen: Option<Colouring>, drawn: &[String]) {
        match chosen {
            Some(Colouring::Custom) => self.go_custom(drawn),
            _ => {
                *self.colouring = chosen;
                for colour in &mut self.colours {
                    **colour = None;
                }
            }
        }
    }

    /// Custom, each role that has no colour of its own started at the one
    /// it is drawn in now.
    fn go_custom(&mut self, drawn: &[String]) {
        for (colour, now) in self.colours.iter_mut().zip(drawn) {
            if colour.is_none() {
                **colour = Some(ColorChoice::Fixed { hex: now.clone() });
            }
        }
        *self.colouring = Some(Colouring::Custom);
    }

    /// Give role `role` the colour `choice`, or the one its preset gives it
    /// when `None`. A study of a preset becomes Custom first, every other
    /// role kept at the colour `drawn` says it has, so a colour picked on
    /// the command line over a preset refines it rather than starting over.
    pub fn set_colour(&mut self, role: usize, choice: Option<ColorChoice>, drawn: &[String]) {
        self.go_custom(drawn);
        let fallback = drawn.get(role).map(|hex| ColorChoice::Fixed { hex: hex.clone() });
        if let Some(colour) = self.colours.get_mut(role) {
            **colour = choice.or(fallback);
        }
    }

    /// How solid role `role` is drawn, 0 to 1.
    pub fn set_alpha(&mut self, role: usize, alpha: f64) {
        if let Some(held) = self.alphas.get_mut(role) {
            **held = Some(alpha.clamp(0.0, 1.0));
        }
    }

    /// How solid every role is drawn: the one Opacity row a preset shows.
    /// A study with a strength over all of them takes it there, and its
    /// roles follow it again.
    pub fn set_opacity(&mut self, alpha: f64) {
        let alpha = alpha.clamp(0.0, 1.0);
        match &mut self.opacity {
            Some(opacity) => {
                **opacity = alpha;
                for held in &mut self.alphas {
                    **held = None;
                }
            }
            None => {
                for held in &mut self.alphas {
                    **held = Some(alpha);
                }
            }
        }
    }

    /// Whether the study has a strength to set at all.
    pub fn has_opacity(&self) -> bool {
        self.opacity.is_some() || !self.alphas.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Study {
        colouring: Option<Colouring>,
        up: Option<ColorChoice>,
        down: Option<ColorChoice>,
        up_alpha: Option<f64>,
        down_alpha: Option<f64>,
    }

    impl Study {
        fn new(colouring: Option<Colouring>) -> Study {
            Study { colouring, up: None, down: None, up_alpha: None, down_alpha: None }
        }

        fn colours(&mut self) -> ColoursMut<'_> {
            ColoursMut {
                colouring: &mut self.colouring,
                labels: &["Up", "Down"],
                colours: vec![&mut self.up, &mut self.down],
                alphas: vec![&mut self.up_alpha, &mut self.down_alpha],
                opacity: None,
            }
        }
    }

    fn fixed(hex: &str) -> Option<ColorChoice> {
        Some(ColorChoice::Fixed { hex: hex.into() })
    }

    #[test]
    fn custom_and_the_presets_are_stored_by_their_keys() {
        for colouring in BarColouring::ALL.map(Colouring::Scheme).into_iter().chain([Colouring::Custom]) {
            let json = serde_json::to_string(&colouring).unwrap();
            assert_eq!(serde_json::from_str::<Colouring>(&json).unwrap(), colouring, "{json}");
        }
        assert_eq!(serde_json::to_string(&Colouring::Custom).unwrap(), r#""custom""#);
        assert_eq!(Colouring::from_key("monochrome"), Some(Colouring::Scheme(BarColouring::Monochrome)));
        assert!(serde_json::from_str::<Colouring>(r#""plaid""#).is_err());
    }

    /// Custom starts every role at what it was drawn in; a pick then refines
    /// one role; the preset chosen again draws the preset, the picks let go
    /// and the strengths kept.
    #[test]
    fn custom_starts_from_what_was_drawn_and_a_preset_starts_over() {
        let mut study = Study::new(Some(Colouring::Scheme(BarColouring::Quiet)));
        study.up_alpha = Some(0.4);
        let quiet = ["#aaaaaa".to_string(), "#555555".to_string()];
        study.colours().pick(Some(Colouring::Custom), &quiet);
        assert_eq!((study.colouring, study.up.clone(), study.down.clone()), (Some(Colouring::Custom), fixed("#aaaaaa"), fixed("#555555")));

        study.colours().set_colour(0, fixed("#00ff00"), &quiet);
        assert_eq!((study.up.clone(), study.down.clone()), (fixed("#00ff00"), fixed("#555555")), "one role refined");

        study.colours().pick(Some(Colouring::Scheme(BarColouring::Quiet)), &quiet);
        assert_eq!((study.colouring, study.up.clone(), study.down.clone()), (Some(Colouring::Scheme(BarColouring::Quiet)), None, None));
        assert_eq!(study.up_alpha, Some(0.4), "how solid it is drawn is kept");
    }

    /// A colour picked over a preset, as the command line does, makes the
    /// study Custom with the other role kept at its preset's colour.
    #[test]
    fn a_colour_picked_over_a_preset_makes_it_custom() {
        let mut study = Study::new(Some(Colouring::Scheme(BarColouring::Soft)));
        let soft = ["#123456".to_string(), "#654321".to_string()];
        study.colours().set_colour(1, fixed("#ff0000"), &soft);
        assert_eq!((study.colouring, study.up.clone(), study.down.clone()), (Some(Colouring::Custom), fixed("#123456"), fixed("#ff0000")));
    }

    /// A study saved with a colour picked over its colouring, before Custom
    /// was a choice, is Custom, and its other role is drawn as its
    /// colouring drew it.
    #[test]
    fn a_colour_picked_before_custom_was_a_choice_reads_as_custom() {
        let mut study = Study::new(Some(Colouring::Scheme(BarColouring::Quiet)));
        study.up = fixed("#00ff00");
        assert!(study.colours().is_custom());
        assert_eq!(study.colours().shown(), Some(Colouring::Custom));
        assert_eq!(base(study.colouring), Some(BarColouring::Quiet), "the role it never picked is still Quiet's");
        let theme = crate::theme::builtin_themes().remove(0);
        assert_eq!(drawn(true, study.up.as_ref(), "#aaaaaa", &theme), "#00ff00");
        assert_eq!(drawn(true, study.down.as_ref(), "#555555", &theme), "#555555");
        assert_eq!(drawn(false, study.up.as_ref(), "#aaaaaa", &theme), "#aaaaaa", "a preset draws its own");
    }

    #[test]
    fn one_opacity_sets_every_role_or_the_study_s_own() {
        let mut study = Study::new(None);
        study.colours().set_opacity(0.6);
        assert_eq!((study.up_alpha, study.down_alpha), (Some(0.6), Some(0.6)));
        let (mut colouring, mut up, mut up_alpha, mut opacity) = (None, None, Some(0.3), 1.0);
        let mut volume = ColoursMut {
            colouring: &mut colouring,
            labels: &["Up"],
            colours: vec![&mut up],
            alphas: vec![&mut up_alpha],
            opacity: Some(&mut opacity),
        };
        volume.set_opacity(0.5);
        assert_eq!((opacity, up_alpha), (0.5, None), "the pane's strength, its roles following it");
    }
}
