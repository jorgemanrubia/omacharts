//! The Colours rows every study painted in a colouring shows, built once
//! here for all of them: a volume pane, a volume profile and its drawing,
//! the cumulative delta, the volume delta, the footprint and large orders.
//!
//! **Colouring** first: **Default**, the presets, and **Custom** last. With
//! a preset, the preset's colours are drawn and no swatch is shown, only
//! one **Opacity** row for every colour the study draws, where it has a
//! strength to set. With Custom, one row a colour the study paints with —
//! its roles, as [`omacharts_engine::colouring`] names them — each its
//! swatch and how solid it is, on the one row every colour has
//! ([`property_rows::colour_row`]). Custom starts every role at the colour
//! it was drawn in a moment before, so nothing moves; a preset chosen again
//! is drawn as it is, and the colours picked under Custom come back if
//! Custom is chosen again while the dialog is open.

use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;
use omacharts_engine::colouring::{Colouring, ColoursMut};
use omacharts_engine::indicators::profile::DELTA_COLOURINGS;
use omacharts_engine::theme::{BarColouring, ColorChoice, Theme};

use super::property_rows::{self, Transparency};

pub use super::profile_rows::{COLOURING, FOLLOWS_DEFAULTS};
pub use omacharts_engine::colouring::CUSTOM_LABEL as CUSTOM;
pub use property_rows::OPACITY_LABEL as OPACITY;

/// A study's colours as they are now, for the rows to show.
pub struct Shown {
    /// The colouring the row shows: Custom for one saved with a colour
    /// picked before Custom was a choice, too.
    pub colouring: Option<Colouring>,
    pub labels: &'static [&'static str],
    /// What each role picked, while Custom.
    pub choices: Vec<Option<ColorChoice>>,
    /// The colour each role is drawn in now.
    pub drawn: Vec<String>,
    /// How solid each role is drawn, if the study sets that a role at a
    /// time; empty if not.
    pub alphas: Vec<f64>,
    /// How solid the study is drawn over all, for its one Opacity row, if
    /// it has a strength to set at all.
    pub opacity: Option<f64>,
}

impl Shown {
    /// For a study gone from under the dialog.
    pub fn empty() -> Shown {
        Shown { colouring: None, labels: &[], choices: Vec::new(), drawn: Vec::new(), alphas: Vec::new(), opacity: None }
    }

    /// `colours` as they are, each role drawn in `drawn`; a role without a
    /// strength of its own is as solid as the study, or `designed`.
    pub fn of(colours: &ColoursMut, drawn: Vec<String>, designed: f64) -> Shown {
        let over_all = colours.opacity.as_deref().copied();
        let alphas: Vec<f64> = colours.alphas.iter().map(|a| a.unwrap_or(over_all.unwrap_or(designed))).collect();
        let opacity = match (over_all, alphas.first()) {
            (Some(alpha), _) => Some(alpha),
            (None, Some(first)) => Some(*first),
            (None, None) => None,
        };
        Shown {
            colouring: colours.shown(),
            labels: colours.labels,
            choices: colours.colours.iter().map(|c| (**c).clone()).collect(),
            drawn,
            alphas,
            opacity,
        }
    }
}

/// What a dialog hands the rows: how to read its study's colours, and how
/// to change them and redraw.
pub struct Host {
    pub theme: Theme,
    /// The study's colours as they are now.
    pub read: Box<dyn Fn() -> Shown>,
    /// Change the study's colours, the colours each role is drawn in now
    /// handed along, then redraw.
    pub edit: Box<dyn Fn(&dyn Fn(&mut ColoursMut, &[String]))>,
}

/// The rows, and the way to show them the study as it is now without
/// calling back, for a change made elsewhere.
pub struct Rows {
    /// In order: Colouring, then Opacity, then a row a role. Each shows or
    /// hides itself by the colouring chosen, and all of them with the
    /// Colouring row, as a profile's delta-only rows are.
    pub widgets: Vec<gtk::Widget>,
    pub reshow: Rc<dyn Fn()>,
}

impl Rows {
    /// The Colouring row, for a dialog that shows and hides it.
    pub fn colouring(&self) -> adw::ComboRow {
        self.widgets[0].clone().downcast().expect("the Colouring row is first")
    }

    pub fn add_to(&self, group: &adw::PreferencesGroup) {
        for widget in &self.widgets {
            group.add(widget);
        }
    }
}

pub fn rows(host: Host) -> Rows {
    let host = Rc::new(host);
    let shown = (host.read)();
    let offered: Vec<BarColouring> = DELTA_COLOURINGS.to_vec();
    let mut names = vec![FOLLOWS_DEFAULTS];
    names.extend(offered.iter().map(|c| c.label()));
    names.push(CUSTOM);
    let at = {
        let offered = offered.clone();
        move |colouring: Option<Colouring>| -> u32 {
            match colouring {
                None => 0,
                Some(Colouring::Custom) => offered.len() as u32 + 1,
                Some(Colouring::Scheme(scheme)) => offered.iter().position(|o| *o == scheme).map_or(0, |at| at as u32 + 1),
            }
        }
    };
    let combo = adw::ComboRow::new();
    combo.set_title(COLOURING);
    combo.set_model(Some(&gtk::StringList::new(&names)));
    combo.set_selected(at(shown.colouring));

    // How solid every colour is, while a preset is drawn.
    let opacity = shown.opacity.map(|alpha| {
        let host = host.clone();
        let (transparency, show) = Transparency::new(1.0 - alpha, move |share| {
            (host.edit)(&|colours, _| colours.set_opacity(1.0 - share));
        });
        (transparency.bare(OPACITY), show)
    });

    // A row a role, each its swatch and how solid it is.
    let mut swatches: Vec<gtk::MenuButton> = Vec::new();
    let mut strengths: Vec<Rc<dyn Fn(f64)>> = Vec::new();
    let mut role_rows: Vec<adw::ActionRow> = Vec::new();
    for (role, title) in shown.labels.iter().enumerate() {
        let picked = {
            let host = host.clone();
            move |choice: ColorChoice| (host.edit)(&|colours, drawn| colours.set_colour(role, Some(choice.clone()), drawn))
        };
        let current = shown.drawn.get(role).cloned().unwrap_or_default();
        let button = crate::ui::palette::picker(&host.theme, shown.choices.get(role).cloned().flatten(), &current, picked, None);
        let transparency = shown.alphas.get(role).map(|alpha| {
            let host = host.clone();
            let (transparency, show) = Transparency::new(1.0 - alpha, move |share| {
                (host.edit)(&|colours, _| colours.set_alpha(role, 1.0 - share));
            });
            strengths.push(show);
            transparency
        });
        role_rows.push(property_rows::colour_row(title, &button, transparency));
        swatches.push(button);
    }

    // Which rows show: the Opacity row with a preset, the roles' with
    // Custom, and none while the Colouring row is hidden.
    let follow: Rc<dyn Fn()> = {
        let combo = combo.clone();
        let opacity = opacity.as_ref().map(|(row, _)| row.clone());
        let role_rows = role_rows.clone();
        let custom_at = offered.len() as u32 + 1;
        Rc::new(move || {
            let custom = combo.selected() == custom_at;
            let visible = combo.property::<bool>("visible");
            if let Some(row) = &opacity {
                row.set_visible(visible && !custom);
            }
            for row in &role_rows {
                row.set_visible(visible && custom);
            }
        })
    };
    follow();
    {
        let follow = follow.clone();
        combo.connect_visible_notify(move |_| follow());
    }

    // The swatches and the strengths show what is drawn now, and the
    // Colouring row what is chosen, without calling back.
    let quiet = Rc::new(std::cell::Cell::new(false));
    let reshow: Rc<dyn Fn()> = {
        let host = host.clone();
        let combo = combo.clone();
        let quiet = quiet.clone();
        let follow = follow.clone();
        let opacity_show = opacity.as_ref().map(|(_, show)| show.clone());
        Rc::new(move || {
            let now = (host.read)();
            for (button, hex) in swatches.iter().zip(&now.drawn) {
                crate::ui::palette::show(button, hex);
            }
            for (show, alpha) in strengths.iter().zip(&now.alphas) {
                show(1.0 - alpha);
            }
            if let (Some(show), Some(alpha)) = (&opacity_show, now.opacity) {
                show(1.0 - alpha);
            }
            quiet.set(true);
            combo.set_selected(at(now.colouring));
            quiet.set(false);
            follow();
        })
    };

    // The colours picked under Custom, while a preset is drawn, to come
    // back if Custom is chosen again.
    let kept: Rc<RefCell<Option<Vec<Option<ColorChoice>>>>> = Rc::new(RefCell::new(None));
    {
        let host = host.clone();
        let offered = offered.clone();
        let reshow = reshow.clone();
        combo.connect_selected_notify(move |combo| {
            if quiet.get() {
                return;
            }
            let at = combo.selected() as usize;
            let chosen = match at {
                0 => None,
                at if at <= offered.len() => Some(Colouring::Scheme(offered[at - 1])),
                _ => Some(Colouring::Custom),
            };
            let before = (host.read)();
            match chosen {
                Some(Colouring::Custom) => {
                    let back = kept.borrow_mut().take();
                    (host.edit)(&|colours, drawn| match &back {
                        Some(back) => {
                            for (role, choice) in back.iter().enumerate() {
                                colours.set_colour(role, choice.clone(), drawn);
                            }
                        }
                        None => colours.pick(Some(Colouring::Custom), drawn),
                    });
                }
                _ => {
                    if before.colouring == Some(Colouring::Custom) {
                        *kept.borrow_mut() = Some(before.choices.clone());
                    }
                    (host.edit)(&|colours, drawn| colours.pick(chosen, drawn));
                }
            }
            reshow();
        });
    }

    let mut widgets: Vec<gtk::Widget> = vec![combo.upcast()];
    widgets.extend(opacity.map(|(row, _)| row.upcast()));
    widgets.extend(role_rows.into_iter().map(|row| row.upcast()));
    Rows { widgets, reshow }
}

/// The colour each of a study's roles is drawn in: `base`, its preset's
/// pair or the house colouring's, or while it is Custom, each role's own.
pub fn drawn_roles(colouring: Option<Colouring>, own: [Option<&ColorChoice>; 2], base: (String, String), theme: &Theme) -> Vec<String> {
    use omacharts_engine::colouring;
    let custom = colouring::is_custom(colouring, &own);
    vec![
        colouring::drawn(custom, own[0], &base.0, theme).into_owned(),
        colouring::drawn(custom, own[1], &base.1, theme).into_owned(),
    ]
}

