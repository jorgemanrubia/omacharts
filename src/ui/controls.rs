//! Controls the settings pages share, so a question asked on two pages is
//! answered with the same widget on both.

use adw::prelude::*;
use gtk::glib;

/// A number with two ends: a slider to aim with, and a box to say it in.
///
/// Every bounded figure in the app is chosen by feel more often than by
/// arithmetic — a pane is about a third of the chart, a shading is barely
/// there, overbought is somewhere up near the top — and a pair of − and +
/// buttons is the wrong shape for that question. The box beside the slider is
/// for the times you already know the figure and would rather type it than
/// aim at it. Having both on every such row, rather than a slider here and a
/// spinner there, is what lets a person stop thinking about which they are
/// looking at.
///
/// One `GtkAdjustment` drives both, so there is one value rather than two
/// controls kept in step: dragging moves the number as it goes, and typing
/// moves the handle. The range lives on that adjustment, which is where it is
/// enforced — a figure typed past either end is refused by the box itself and
/// never reaches the caller, so nothing downstream has to clamp what it is
/// handed.
pub struct Bounded {
    /// The row, ready to be added to a group.
    pub row: adw::ActionRow,
    adjustment: gtk::Adjustment,
}

/// Build a row for a figure between `min` and `max`.
///
/// `step` is what one press of an arrow key moves it, and `digits` how many
/// decimals are shown — none for a percent or a level, one or two for a
/// fraction. The slider snaps to the same steps, so it can only land on
/// figures the box could have been given; a slider that stops between figures
/// is a slider the box then disagrees with.
///
/// `changed` hears every figure the person settles on, already inside the
/// range. The initial `value` is placed before it is connected, so building
/// the row does not count as changing it.
#[allow(clippy::too_many_arguments)]
pub fn bounded_row(
    title: &str,
    subtitle: Option<&str>,
    value: f64,
    min: f64,
    max: f64,
    step: f64,
    digits: u32,
    changed: impl Fn(f64) + 'static,
) -> Bounded {
    let row = adw::ActionRow::new();
    row.set_title(title);
    if let Some(subtitle) = subtitle {
        row.set_subtitle(subtitle);
    }

    // A page of arrow presses: ten steps, which is a tenth of a percent
    // range and feels like a page on the shorter ranges too.
    let adjustment = gtk::Adjustment::new(value.clamp(min, max), min, max, step, step * 10.0, 0.0);

    let slider = gtk::Scale::new(gtk::Orientation::Horizontal, Some(&adjustment));
    slider.set_draw_value(false);
    slider.set_digits(digits as i32);
    slider.set_round_digits(digits as i32);
    slider.set_size_request(180, -1);
    slider.set_valign(gtk::Align::Center);

    row.add_suffix(&slider);
    row.add_suffix(&entry(&adjustment, step, digits));

    adjustment.connect_value_changed(move |adjustment| changed(adjustment.value()));

    Bounded { row, adjustment }
}

/// The box a figure can be typed into.
///
/// The figure is committed on Enter or on leaving the box, never per
/// keystroke: the 5 on the way to 50 would otherwise reach the chart and
/// redraw it on the way past. Anything outside the range is refused — the
/// text snaps back to the figure that was there — rather than pulled to the
/// nearest end: a box that quietly changes 150 into 100 has agreed to
/// something the person did not say. Anything that is not a number at all is
/// treated the same way.
fn entry(adjustment: &gtk::Adjustment, step: f64, digits: u32) -> gtk::SpinButton {
    let entry = gtk::SpinButton::new(Some(adjustment), step, digits);
    entry.set_valign(gtk::Align::Center);
    entry.set_numeric(true);
    entry.set_snap_to_ticks(true);
    entry.set_update_policy(gtk::SpinButtonUpdatePolicy::IfValid);
    // Wide enough for the longest figure the range allows, and no wider, so
    // a row of these lines up and the box reads as part of the slider rather
    // than a field of its own.
    let chars = width_in_chars(adjustment.lower(), adjustment.upper(), digits);
    entry.set_width_chars(chars);
    entry.set_max_width_chars(chars);
    entry
}

/// How many characters the widest figure in the range takes to print.
fn width_in_chars(min: f64, max: f64, digits: u32) -> i32 {
    let printed = |value: f64| format!("{value:.*}", digits as usize).len();
    printed(min).max(printed(max)) as i32
}

impl Bounded {
    /// Move both controls to `value`, which the range may pull in.
    ///
    /// This is the same as the person having moved it: `changed` hears it,
    /// so a row that sets a sibling — oversold pulled under an overbought that
    /// has dropped past it — stores the sibling's new figure as well.
    pub fn set(&self, value: f64) {
        self.adjustment.set_value(value);
    }

    /// The figure both controls are showing.
    pub fn value(&self) -> f64 {
        self.adjustment.value()
    }

    /// A handle a sibling's callback can hold.
    ///
    /// Weak, because that callback lives on the sibling's own adjustment, and
    /// a pair of rows each holding the other strongly is a pair of rows that
    /// outlive the dialog.
    pub fn downgrade(&self) -> WeakBounded {
        WeakBounded { adjustment: self.adjustment.downgrade() }
    }
}

/// A `Bounded` that does not keep its row alive.
pub struct WeakBounded {
    adjustment: glib::WeakRef<gtk::Adjustment>,
}

impl WeakBounded {
    /// As `Bounded::set`, if the row is still around.
    pub fn set(&self, value: f64) {
        if let Some(adjustment) = self.adjustment.upgrade() {
            adjustment.set_value(value);
        }
    }

    /// The figure showing, if the row is still around.
    pub fn value(&self) -> Option<f64> {
        self.adjustment.upgrade().map(|adjustment| adjustment.value())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_box_is_as_wide_as_the_widest_figure_it_can_hold() {
        assert_eq!(width_in_chars(0.0, 100.0, 0), 3, "a percent goes up to 100");
        assert_eq!(width_in_chars(0.1, 6.0, 1), 3, "a count of deviations reads as 6.0");
        assert_eq!(width_in_chars(0.0, 1.0, 2), 4, "a fraction reads as 1.00");
        assert_eq!(width_in_chars(-10.0, 5.0, 0), 3, "and a minus sign takes a place");
    }

    /// Everything below needs a display, and a second test starting GTK on
    /// another thread would take the run down with it, so it all goes through
    /// the one guard.
    #[test]
    fn the_slider_and_the_box_show_one_figure_and_refuse_what_is_past_the_ends() {
        if !crate::ui::gtk_ready() {
            return;
        }
        let heard = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let heard_by_row = heard.clone();
        let row = bounded_row("Level", None, 70.0, 0.0, 100.0, 1.0, 0, move |v| {
            heard_by_row.borrow_mut().push(v)
        });
        assert_eq!(row.value(), 70.0);
        assert!(heard.borrow().is_empty(), "placing the first figure is not a change");

        row.set(80.0);
        assert_eq!(heard.borrow().as_slice(), &[80.0]);

        // Past the top end it is held at the top.
        row.set(150.0);
        assert_eq!(row.value(), 100.0);

        let weak = row.downgrade();
        weak.set(40.0);
        assert_eq!(weak.value(), Some(40.0));
        assert_eq!(heard.borrow().last(), Some(&40.0));
        drop(row);
        assert_eq!(weak.value(), None, "a weak handle does not keep the row alive");
    }
}
