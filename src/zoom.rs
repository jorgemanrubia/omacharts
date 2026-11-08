//! How far a turn of the wheel or a swipe of two fingers zooms a chart.
//!
//! One setting for the whole app, as the clock is: a percentage of the base
//! rate, read by every chart the moment it changes.
//!
//! How a wheel's turn becomes a zoom is the chart's, in
//! [`omacharts_chart::zoom`]; what is kept here is the setting, stored and
//! handed to it.

use crate::store::Store;

pub use omacharts_chart::zoom::{clamp, DEFAULT, MAX, MIN};

/// The percentage of the base rate, from [`MIN`] to [`MAX`].
pub const SETTING: &str = "zoom_sensitivity";

pub fn sensitivity(store: &Store) -> u32 {
    store.setting(SETTING).and_then(|value| value.trim().parse::<u32>().ok()).map_or(DEFAULT, clamp)
}

pub fn save(store: &Store, percent: u32) {
    let percent = clamp(percent);
    store.set_setting(SETTING, &percent.to_string());
    omacharts_chart::zoom::set_sensitivity(percent);
}

/// Read the stored setting into what charts read: at start, and after a
/// command or a restore wrote it.
pub fn adopt(store: &Store) {
    omacharts_chart::zoom::set_sensitivity(sensitivity(store));
}

/// Read a percentage as `config set` is given one: `50` or `50%`, within
/// range. Out of range is refused rather than pulled to the nearest end.
pub fn parse(value: &str) -> Result<u32, String> {
    let figure = value.trim().trim_end_matches('%').trim();
    match figure.parse::<u32>() {
        Ok(percent) if (MIN..=MAX).contains(&percent) => Ok(percent),
        _ => Err(format!("{value:?} is not a zoom sensitivity; use a percentage from {MIN} to {MAX}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentages_are_read_with_or_without_the_sign() {
        assert_eq!(parse("50"), Ok(50));
        assert_eq!(parse("50%"), Ok(50));
        assert_eq!(parse(" 200 % "), Ok(200));
        assert!(parse("5").is_err());
        assert!(parse("250").is_err());
        assert!(parse("fast").is_err());
        assert!(parse("0.5").is_err());
    }
}
