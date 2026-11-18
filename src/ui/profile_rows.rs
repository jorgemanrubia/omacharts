//! The rows of a volume profile's properties, for the study's settings and a
//! profile drawing's alike.
//!
//! One set of names, controls, ranges and one order, laid out here, so the
//! two dialogs cannot drift apart: a row the study has and the drawing means
//! the same thing by is built by the same function and put in the same place.
//! What each side builds on its own is only what its own storage makes
//! different — a colour, which is a preset on a drawing and a theme colour on
//! a study — and what only a study has: its period, and whether only the
//! newest is drawn, neither of which a span between two anchors has.

use std::cell::Cell;
use std::rc::Rc;

use adw::prelude::*;
use omacharts_engine::indicators::profile::{Method, DELTA_COLOURINGS};
use omacharts_engine::BarColouring;

pub use crate::ui::property_rows::{
    BACKGROUND, TRANSPARENCY, percent_row, switch_row, thickness_row, transparency_row,
};
#[cfg(test)]
pub use crate::ui::property_rows::titles;

pub const OUTLINE: &str = "Outline";
pub const OUTLINE_COLOUR: &str = "Outline colour";
pub const OUTLINE_STYLE: &str = "Outline style";
pub const OUTLINE_THICKNESS: &str = "Outline thickness";
pub const SHOW_POC: &str = "Show point of control";
pub const POC_COLOUR: &str = "Point of control colour";
pub const POC_STYLE: &str = "Point of control style";
pub const POC_THICKNESS: &str = "Point of control thickness";
pub const STYLE: &str = "Style";
pub const ALIGNMENT: &str = "Alignment";
pub const AUTOMATIC: &str = "Automatic";
pub const ROWS: &str = "Rows";
pub const VALUE_AREA: &str = "Value area %";
pub const HIGHLIGHT: &str = "Highlight value area";
pub const VALUE_AREA_COLOUR: &str = "Value area colour";
pub const METHOD: &str = "Calculation method";
pub const COLOURING: &str = "Colouring";
pub const WIDTH: &str = "Width %";
/// What the Colouring row offers first: the colouring chosen on the
/// Defaults page of Preferences, whatever that is.
pub const FOLLOWS_DEFAULTS: &str = "Default";

/// The fewest and most rows the Rows box offers: the engine's own floor and
/// ceiling, past which a count is clamped anyway.
const FEWEST_ROWS: f64 = 4.0;
const MOST_ROWS: f64 = 800.0;

/// What the Rows box shows when the count is automatic and nothing has been
/// drawn yet to say how many it came to.
const ROWS_UNKNOWN: usize = 48;

/// The rows of the Style group, built by each side, laid out here.
pub struct StyleRows {
    pub outline: adw::SwitchRow,
    pub outline_colour: gtk::Widget,
    pub outline_style: gtk::Widget,
    pub outline_thickness: gtk::Widget,
    pub background: gtk::Widget,
    pub transparency: gtk::Widget,
    pub show_poc: adw::SwitchRow,
    pub poc_colour: gtk::Widget,
    pub poc_style: gtk::Widget,
    pub poc_thickness: gtk::Widget,
    pub style: gtk::Widget,
    pub alignment: gtk::Widget,
    /// How much of its room the longest row reaches: of a study's period,
    /// of a drawing's span.
    pub width: gtk::Widget,
}

/// The Style group, in its one order: the outline, the rows, the point of
/// control, then how the rows are drawn; a stroke's colour, thickness and
/// style in that order, as on every line in every dialog. The outline's own rows are there
/// while there is an outline, and the point of control's while it is drawn.
pub fn style_group(rows: StyleRows) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::new();
    group.set_title("Style");
    group.add(&rows.outline);
    let outline_rows = [
        rows.outline_colour,
        rows.outline_thickness,
        rows.outline_style,
    ];
    for row in &outline_rows {
        group.add(row);
    }
    follow_switch(&rows.outline, outline_rows);
    group.add(&rows.background);
    group.add(&rows.transparency);
    group.add(&rows.show_poc);
    let poc_rows = [rows.poc_colour, rows.poc_thickness, rows.poc_style];
    for row in &poc_rows {
        group.add(row);
    }
    follow_switch(&rows.show_poc, poc_rows);
    group.add(&rows.style);
    group.add(&rows.alignment);
    group.add(&rows.width);
    group
}

/// `rows` shown while `switch` is on, from now and as it is turned.
fn follow_switch<const N: usize>(switch: &adw::SwitchRow, rows: [gtk::Widget; N]) {
    for row in &rows {
        row.set_visible(switch.is_active());
    }
    switch.connect_active_notify(move |switch| {
        for row in &rows {
            row.set_visible(switch.is_active());
        }
    });
}

/// The rows of the Parameters group, built by each side, laid out here.
pub struct ParameterRows {
    /// What only a study has, first: its period and whether only the newest
    /// is drawn.
    pub leading: Vec<gtk::Widget>,
    pub method: adw::ComboRow,
    pub colouring: adw::ComboRow,
    pub granularity: Granularity,
    pub value_area: gtk::Widget,
    pub highlight: adw::SwitchRow,
    pub value_area_colour: gtk::Widget,
}

/// The Parameters rows, in their one order, onto `group`.
pub fn add_parameters(group: &adw::PreferencesGroup, rows: &ParameterRows) {
    for row in &rows.leading {
        group.add(row);
    }
    group.add(&rows.method);
    group.add(&rows.colouring);
    group.add(&rows.granularity.automatic);
    group.add(&rows.granularity.rows);
    group.add(&rows.value_area);
    group.add(&rows.highlight);
    group.add(&rows.value_area_colour);
}

/// How finely a profile is divided: automatically, or into so many rows.
pub struct Granularity {
    pub automatic: adw::SwitchRow,
    pub rows: adw::SpinRow,
    /// Show another count, or automatic, without calling back.
    pub show: Rc<dyn Fn(Option<usize>)>,
}

/// The Automatic switch and the Rows box under it.
///
/// While the count is automatic the box is greyed and shows the count being
/// drawn, which `drawn` says, so the number on screen is the number on the
/// chart rather than a placeholder. Turning Automatic off hands over that
/// count, so taking control does not also change the chart.
pub fn granularity(
    rows: Option<usize>,
    drawn: Rc<dyn Fn() -> Option<usize>>,
    changed: impl Fn(Option<usize>) + 'static,
) -> Granularity {
    let changed: Rc<dyn Fn(Option<usize>)> = Rc::new(changed);
    let quiet = Rc::new(Cell::new(false));

    let automatic = adw::SwitchRow::new();
    automatic.set_title(AUTOMATIC);
    automatic.set_active(rows.is_none());

    let count = adw::SpinRow::with_range(FEWEST_ROWS, MOST_ROWS, 1.0);
    count.set_title(ROWS);
    count.set_value(rows.or_else(|| drawn()).unwrap_or(ROWS_UNKNOWN) as f64);
    count.set_sensitive(rows.is_some());

    {
        let changed = changed.clone();
        let quiet = quiet.clone();
        let drawn = drawn.clone();
        let count = count.downgrade();
        automatic.connect_active_notify(move |switch| {
            let on = switch.is_active();
            let Some(count) = count.upgrade() else { return };
            count.set_sensitive(!on);
            if quiet.get() {
                return;
            }
            let taken = count.value() as usize;
            changed(if on { None } else { Some(taken) });
            if on && let Some(drawn) = drawn() {
                quiet.set(true);
                count.set_value(drawn as f64);
                quiet.set(false);
            }
        });
    }
    {
        let changed = changed.clone();
        let quiet = quiet.clone();
        count.connect_value_notify(move |count| {
            if quiet.get() || !count.is_sensitive() {
                return;
            }
            changed(Some(count.value() as usize));
        });
    }

    let show: Rc<dyn Fn(Option<usize>)> = {
        let automatic = automatic.clone();
        let count = count.clone();
        Rc::new(move |rows: Option<usize>| {
            quiet.set(true);
            automatic.set_active(rows.is_none());
            count.set_sensitive(rows.is_some());
            count.set_value(rows.or_else(|| drawn()).unwrap_or(ROWS_UNKNOWN) as f64);
            quiet.set(false);
        })
    };

    Granularity {
        automatic,
        rows: count,
        show,
    }
}

/// What a profile's rows measure: the volume at each price, or the ask
/// volume less the bid volume there.
pub fn method_row(
    method: Method,
    changed: impl Fn(Method) + 'static,
) -> (adw::ComboRow, Rc<dyn Fn(Method)>) {
    let at = |method: Method| Method::ALL.iter().position(|m| *m == method).unwrap_or(0) as u32;
    let (row, show) = combo_row(METHOD, &Method::ALL.map(Method::label), at(method), move |picked| {
        changed(Method::ALL.get(picked).copied().unwrap_or_default())
    });
    (row, Rc::new(move |method| show(at(method))))
}

/// The way to show a Colouring row another colouring without calling back.
pub type ShowColouring = Rc<dyn Fn(Option<BarColouring>)>;

/// How a delta's bought and sold rows are painted: in the colouring chosen in
/// Preferences, followed as it changes, or in one of the theme's own. A
/// colouring outside those that a profile already has is offered last, so
/// the row shows what is drawn.
pub fn colouring_row(
    current: Option<BarColouring>,
    changed: impl Fn(Option<BarColouring>) + 'static,
) -> (adw::ComboRow, ShowColouring) {
    let mut offered: Vec<BarColouring> = DELTA_COLOURINGS.to_vec();
    if let Some(kept) = current.filter(|c| !offered.contains(c)) {
        offered.push(kept);
    }
    let mut names = vec![FOLLOWS_DEFAULTS];
    names.extend(offered.iter().map(|c| c.label()));
    let at = {
        let offered = offered.clone();
        move |current: Option<BarColouring>| {
            current.and_then(|c| offered.iter().position(|o| *o == c)).map_or(0, |at| at + 1) as u32
        }
    };
    let (row, show) = combo_row(COLOURING, &names, at(current), move |picked| {
        changed(picked.checked_sub(1).and_then(|at| offered.get(at).copied()))
    });
    (row, Rc::new(move |current| show(at(current))))
}

fn combo_row(
    title: &str,
    names: &[&str],
    at: u32,
    picked: impl Fn(usize) + 'static,
) -> (adw::ComboRow, Rc<dyn Fn(u32)>) {
    let quiet = Rc::new(Cell::new(false));
    let row = adw::ComboRow::new();
    row.set_title(title);
    row.set_model(Some(&gtk::StringList::new(names)));
    row.set_selected(at);
    {
        let quiet = quiet.clone();
        row.connect_selected_notify(move |row| {
            if !quiet.get() {
                picked(row.selected() as usize);
            }
        });
    }
    let shown = row.clone();
    let show: Rc<dyn Fn(u32)> = Rc::new(move |at| {
        quiet.set(true);
        shown.set_selected(at);
        quiet.set(false);
    });
    (row, show)
}

/// Whether a profile measuring by `method` has a value area to set: a
/// delta has none, its rows cancelling each other out.
pub fn has_value_area(method: Method) -> bool {
    method == Method::Total
}

/// Whether a profile measuring by `method` is painted in a colouring: a
/// delta is, bought and sold as rising and falling bars; a total is in the
/// profile's own colour.
pub fn has_colouring(method: Method) -> bool {
    method == Method::Delta
}

/// The rows the calculation method decides about.
pub struct ByMethod {
    pub value_area: gtk::Widget,
    pub highlight: adw::SwitchRow,
    pub value_area_colour: gtk::Widget,
    pub colouring: adw::ComboRow,
    /// Rows a delta does not paint with, greyed while it is one.
    pub total_only: Vec<gtk::Widget>,
}

/// Grey what a delta has no use for and show its Colouring, for `method`
/// now and, through what is handed back, for any method shown later; and
/// as the Calculation method row and the highlight switch are turned. The
/// value area's colour is there to set while the value area is set apart.
pub fn follow_method(method_row: &adw::ComboRow, rows: ByMethod) -> Rc<dyn Fn(Method)> {
    let rows = Rc::new(rows);
    let apply: Rc<dyn Fn(Method)> = {
        let rows = rows.clone();
        Rc::new(move |method| {
            let total = has_value_area(method);
            rows.value_area.set_sensitive(total);
            rows.highlight.set_sensitive(total);
            rows.value_area_colour.set_sensitive(total && rows.highlight.is_active());
            rows.colouring.set_visible(has_colouring(method));
            for row in &rows.total_only {
                row.set_sensitive(total);
            }
        })
    };
    {
        let colour = rows.value_area_colour.clone();
        rows.highlight.connect_active_notify(move |row| colour.set_sensitive(row.is_sensitive() && row.is_active()));
    }
    {
        let apply = apply.clone();
        method_row.connect_selected_notify(move |row| {
            apply(Method::ALL.get(row.selected() as usize).copied().unwrap_or_default())
        });
    }
    apply(Method::ALL.get(method_row.selected() as usize).copied().unwrap_or_default());
    apply
}

/// Grey `style`'s Background while `parameters`' Calculation method is a
/// delta, whose rows are painted in its colouring: for a dialog whose two
/// groups are built apart.
pub fn background_follows_method(parameters: &adw::PreferencesGroup, style: &adw::PreferencesGroup) {
    let (Some(method), Some(background)) = (row_titled(parameters.upcast_ref(), METHOD), row_titled(style.upcast_ref(), BACKGROUND))
    else {
        return;
    };
    let Ok(method) = method.downcast::<adw::ComboRow>() else { return };
    let follow = move |row: &adw::ComboRow| {
        background.set_sensitive(has_value_area(Method::ALL.get(row.selected() as usize).copied().unwrap_or_default()))
    };
    follow(&method);
    method.connect_selected_notify(follow);
}

/// The row titled `title` anywhere under `widget`.
fn row_titled(widget: &gtk::Widget, title: &str) -> Option<gtk::Widget> {
    if widget.downcast_ref::<adw::PreferencesRow>().is_some_and(|row| row.title() == title) {
        return Some(widget.clone());
    }
    let mut child = widget.first_child();
    while let Some(widget) = child {
        if let Some(found) = row_titled(&widget, title) {
            return Some(found);
        }
        child = widget.next_sibling();
    }
    None
}

/// How much of its room the longest row reaches, 10 to 100%.
pub fn width_row(share: f64, changed: impl Fn(f64) + 'static) -> (adw::ActionRow, Rc<dyn Fn(f64)>) {
    use omacharts_engine::indicators::profile::{drawn_width, MAX_WIDTH, MIN_WIDTH};
    let (row, show) = percent_row(WIDTH, drawn_width(share), MIN_WIDTH, MAX_WIDTH, changed);
    (row, Rc::new(move |share| show(drawn_width(share))))
}

/// The share of the volume the value area holds, 10 to 100%.
pub fn value_area_row(
    share: f64,
    changed: impl Fn(f64) + 'static,
) -> (adw::ActionRow, Rc<dyn Fn(f64)>) {
    percent_row(VALUE_AREA, share, 0.10, 1.0, changed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_rows_box_follows_automatic_and_hands_over_the_count_drawn() {
        crate::ui::on_gtk(|| {
            let seen: Rc<std::cell::RefCell<Vec<Option<usize>>>> = Rc::default();
            let rows = {
                let seen = seen.clone();
                granularity(None, Rc::new(|| Some(236)), move |rows| {
                    seen.borrow_mut().push(rows)
                })
            };
            assert!(rows.automatic.is_active());
            assert!(!rows.rows.is_sensitive());
            assert_eq!(
                rows.rows.value(),
                236.0,
                "the count being drawn, not a placeholder"
            );

            rows.automatic.set_active(false);
            assert_eq!(
                seen.borrow().last(),
                Some(&Some(236)),
                "taking control keeps the chart as it was"
            );
            assert!(rows.rows.is_sensitive());
            rows.rows.set_value(120.0);
            assert_eq!(seen.borrow().last(), Some(&Some(120)));

            let before = seen.borrow().len();
            (rows.show)(None);
            assert!(rows.automatic.is_active());
            assert_eq!(
                seen.borrow().len(),
                before,
                "showing a style is not changing it"
            );
        });
    }
}
