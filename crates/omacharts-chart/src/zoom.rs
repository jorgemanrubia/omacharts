//! How far a turn of the wheel or a swipe of two fingers zooms a chart.
//!
//! One setting for the whole process: a percentage of the base rate, read by
//! every chart the moment it changes. Whoever embeds the chart sets it with
//! [`set_sensitivity`]; it is 100% until then.
//!
//! The base rate is a notch of a mouse wheel, which zooms the time axis by
//! [`NOTCH`]. A touchpad does not send notches: it sends a stream of small
//! deltas measured in pixels, a few dozen of them for one swipe. Each of those
//! used to count as a whole notch, which is why a swipe threw the chart from
//! a week to a decade. They are now converted to notches first, at
//! [`PIXELS_PER_NOTCH`], so a swipe zooms by how far the fingers travel —
//! and the sensitivity then scales wheel and touchpad alike.

use std::sync::atomic::{AtomicU32, Ordering};

/// The percentage of the base rate, from [`MIN`] to [`MAX`].
pub const DEFAULT: u32 = 100;
pub const MIN: u32 = 10;
pub const MAX: u32 = 200;

/// The time axis zooms by this much for one notch of a mouse wheel, at 100%.
pub const NOTCH: f64 = 1.15;

/// How many pixels of touchpad travel count as one notch of a wheel. A firm
/// two-finger swipe is a couple of hundred pixels, which zooms by about one
/// doubling at 100%.
pub const PIXELS_PER_NOTCH: f64 = 40.0;

/// What a scroll delta is measured in, as GTK says.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Delta {
    /// Wheel notches: 1.0 a click, fractions of one from a high-resolution
    /// wheel.
    Notches,
    /// Surface pixels, from a touchpad or anything else that scrolls smoothly.
    Pixels,
}

/// The sensitivity charts read, as a fraction: 1.0 at 100%. Held for the
/// process rather than asked of the store on every scroll event, and set
/// again whenever the setting is written.
static CURRENT: AtomicU32 = AtomicU32::new(DEFAULT);

/// Set what charts scale their zoom by, as a percentage of the base rate,
/// held to [`MIN`]..=[`MAX`].
pub fn set_sensitivity(percent: u32) {
    CURRENT.store(clamp(percent), Ordering::Relaxed);
}

/// What charts scale their zoom by now.
pub fn current() -> f64 {
    CURRENT.load(Ordering::Relaxed) as f64 / 100.0
}

/// A percentage held to the range the setting allows.
pub fn clamp(percent: u32) -> u32 {
    percent.clamp(MIN, MAX)
}

/// A scroll delta as wheel notches, scaled by the sensitivity. Positive is
/// down, which zooms out.
pub fn notches(delta: f64, unit: Delta, sensitivity: f64) -> f64 {
    let notches = match unit {
        Delta::Notches => delta,
        Delta::Pixels => delta / PIXELS_PER_NOTCH,
    };
    notches * sensitivity
}

/// The time axis's zoom factor for so many notches: above 1 zooms out.
pub fn time_factor(notches: f64) -> f64 {
    NOTCH.powf(notches)
}

/// Gather small zoom steps until they move the view by a whole column.
///
/// The time axis counts whole columns, so a step of a fraction of one —
/// every event from a gentle swipe, near the closest zoom — would round back
/// to where it was and the chart would not zoom at all. The steps are carried
/// until together they are worth a column. Turning back drops what was
/// carried in the other direction. Answers the factor to zoom by now, if
/// any, and what to carry to the next event.
pub fn carried(carry: f64, factor: f64, columns: usize) -> (Option<f64>, f64) {
    let turned = (carry > 1.0 && factor < 1.0) || (carry < 1.0 && factor > 1.0);
    // At a limit the steps would pile up for ever; no more than a doubling
    // either way waits to be spent.
    let total = (if turned { 1.0 } else { carry } * factor).clamp(0.5, 2.0);
    match (columns as f64 * total).round() as usize == columns {
        true => (None, total),
        false => (Some(total), 1.0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_wheel_notch_zooms_as_it_always_has() {
        assert_eq!(time_factor(notches(1.0, Delta::Notches, 1.0)), NOTCH);
        assert_eq!(time_factor(notches(-1.0, Delta::Notches, 1.0)), 1.0 / NOTCH);
    }

    #[test]
    fn touchpad_pixels_are_worth_a_fraction_of_a_notch() {
        let one = notches(PIXELS_PER_NOTCH, Delta::Pixels, 1.0);
        assert!((one - 1.0).abs() < 1e-9);
        // A typical event of a few pixels is a small step, not a whole notch.
        let step = time_factor(notches(4.0, Delta::Pixels, 1.0));
        assert!(step > 1.0 && step < 1.02, "{step}");
    }

    #[test]
    fn the_sensitivity_scales_both() {
        let half = notches(1.0, Delta::Notches, 0.5);
        assert!((half - 0.5).abs() < 1e-9);
        let double = notches(PIXELS_PER_NOTCH, Delta::Pixels, 2.0);
        assert!((double - 2.0).abs() < 1e-9);
    }

    #[test]
    fn small_steps_add_up_to_a_column() {
        let factor = time_factor(notches(3.0, Delta::Pixels, 1.0));
        let (now, mut carry) = carried(1.0, factor, 20);
        assert_eq!(now, None);
        // On its own each event rounds back to 20 columns; a handful of
        // them together reach 21.
        let mut events = 1;
        let spent = loop {
            let (now, next) = carried(carry, factor, 20);
            events += 1;
            carry = next;
            if let Some(spent) = now {
                break spent;
            }
            assert!(events < 20, "never zoomed");
        };
        assert!((20.0 * spent).round() as usize == 21);
        assert_eq!(carry, 1.0);
    }

    #[test]
    fn a_whole_step_is_spent_at_once() {
        assert_eq!(carried(1.0, 1.15, 100), (Some(1.15), 1.0));
    }

    #[test]
    fn turning_back_drops_the_carry() {
        let (_, carry) = carried(1.0, 1.01, 20);
        assert!(carry > 1.0);
        let (now, carry) = carried(carry, 0.99, 20);
        assert_eq!(now, None);
        assert!(carry < 1.0);
    }

    #[test]
    fn the_carry_cannot_pile_up_at_a_limit() {
        let mut carry = 1.0;
        for _ in 0..100 {
            carry = carried(carry, 1.1, 0).1;
        }
        assert_eq!(carry, 2.0);
    }
}
