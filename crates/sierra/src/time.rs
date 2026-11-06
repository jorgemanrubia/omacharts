//! Calendar dates from unix time and back, in UTC.
//!
//! Small enough to keep here rather than take a date crate for: a `.dly`
//! row is a date, a two-digit contract year needs this year, and that is
//! all the calendar this crate needs.

/// The UTC date of a unix time, as (year, month 1–12, day 1–31).
pub fn civil_from_unix_seconds(unix_seconds: i64) -> (i32, u32, u32) {
    civil_from_days(unix_seconds.div_euclid(86_400))
}

/// The unix time of midnight UTC at the start of a date.
pub fn unix_seconds_from_civil(year: i32, month: u32, day: u32) -> i64 {
    days_from_civil(year, month, day) * 86_400
}

/// Days since 1970-01-01 to a date, in the proleptic Gregorian calendar
/// (Howard Hinnant's `days_from_civil`).
pub(crate) fn days_from_civil(year: i32, month: u32, day: u32) -> i64 {
    let y = i64::from(year) - i64::from(month <= 2);
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let m = i64::from(month);
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + i64::from(day) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The date that many days after 1970-01-01.
pub(crate) fn civil_from_days(days: i64) -> (i32, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year as i32, month, day)
}

/// This year, by the system clock.
pub(crate) fn current_year() -> i32 {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    civil_from_unix_seconds(now).0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_round_trip_through_days() {
        for days in -800_000..800_000 {
            let (y, m, d) = civil_from_days(days);
            assert_eq!(days_from_civil(y, m, d), days);
        }
    }

    #[test]
    fn known_dates_land_where_they_should() {
        assert_eq!(civil_from_unix_seconds(0), (1970, 1, 1));
        assert_eq!(unix_seconds_from_civil(2026, 9, 11), 1_789_084_800);
        assert_eq!(civil_from_unix_seconds(1_789_084_800 + 86_399), (2026, 9, 11));
        assert_eq!(days_from_civil(1899, 12, 30), -25_569);
        assert_eq!(civil_from_days(days_from_civil(2024, 2, 29)), (2024, 2, 29));
    }
}
