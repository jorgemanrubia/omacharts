//! The rows of a volume profile's properties, for the study's settings and a
//! profile drawing's alike.
//!
//! One set of names, controls, ranges and one order, laid out here, so the
//! two dialogs cannot drift apart: a row the study has and the drawing means
//! the same thing by is built by the same function and put in the same place.
//! What each side builds on its own is only what its own storage makes
//! different — a colour, which is a preset on a drawing and a theme colour on
//! a study — and what only one of them has: a study's period, what it
//! measures and how wide it is drawn, none of which a span between two
//! anchors has.

use std::cell::Cell;
use std::rc::Rc;

use adw::prelude::*;

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
    /// How much of its room a study's rows reach. A drawing's room is the
    /// span it covers, edge to edge, so it has none.
    pub width: Option<gtk::Widget>,
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
    if let Some(width) = &rows.width {
        group.add(width);
    }
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
    /// What only a study has, first: its period, whether only the newest is
    /// drawn, what its rows measure and how a delta is painted.
    pub leading: Vec<gtk::Widget>,
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
