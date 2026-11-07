//! Daily files: `.dly`, a bar per day as text.
//!
//! A header line, then one row per day, oldest first:
//!
//! ```text
//! Date,  Open,  High,  Low,  Close,  Volume,  OpenInterest
//! 2026/09/11, 759950.00000000, 768350.00000000, 759425.00000000, 765950.00000000, 1904677, 0
//! ```
//!
//! Prices are stored scaled, like an intraday file's, and are returned as
//! stored. The rows are parsed straight off the bytes, without a `String`
//! per line or per field, at about a gigabyte a second: decades of a
//! currency pair parse in half a millisecond, and reading the file costs
//! more than parsing it.
//!
//! Sierra Chart ends every row with a line break, so a last line without
//! one is a row still being written, and is left out rather than read half
//! finished.

use std::io;
use std::path::Path;

/// One day, prices as stored.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Daily {
    pub year: i32,
    pub month: u32,
    pub day: u32,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub volume: u64,
    pub open_interest: u64,
}

impl Daily {
    /// The date as one number that sorts: `20260911`.
    pub fn ymd(&self) -> i64 {
        i64::from(self.year) * 10_000 + i64::from(self.month) * 100 + i64::from(self.day)
    }

    /// Midnight UTC at the start of the day, as unix seconds.
    pub fn unix_seconds(&self) -> i64 {
        crate::time::unix_seconds_from_civil(self.year, self.month, self.day)
    }
}

/// Every day in a `.dly`.
pub fn read(path: impl AsRef<Path>) -> io::Result<Vec<Daily>> {
    Ok(parse(&std::fs::read(path)?))
}

/// The days on or after a date.
pub fn read_since(path: impl AsRef<Path>, year: i32, month: u32, day: u32) -> io::Result<Vec<Daily>> {
    Ok(parse_since(&std::fs::read(path)?, i64::from(year) * 10_000 + i64::from(month) * 100 + i64::from(day)))
}

/// Every whole row in a `.dly`'s bytes; anything that is not a row (the
/// header, a blank line, a line half written) is skipped.
pub fn parse(bytes: &[u8]) -> Vec<Daily> {
    let whole = match bytes.iter().rposition(|&b| b == b'\n') {
        Some(i) => &bytes[..=i],
        None => &[],
    };
    let mut out = Vec::with_capacity(whole.len() / 90);
    for line in whole.split(|&b| b == b'\n') {
        if let Some(d) = row(line) {
            out.push(d);
        }
    }
    out
}

/// The rows on or after a date (`yyyymmdd`), found by bisecting the bytes
/// rather than parsing what comes before.
pub fn parse_since(bytes: &[u8], ymd: i64) -> Vec<Daily> {
    // The first byte offset whose line is on or after the date. Each probe
    // moves to the start of the next line and reads its date.
    let line_at = |at: usize| -> (usize, Option<i64>) {
        let start = if at == 0 { 0 } else { bytes[at - 1..].iter().position(|&b| b == b'\n').map_or(bytes.len(), |i| at + i) };
        let mut s = start;
        loop {
            if s >= bytes.len() {
                return (bytes.len(), None);
            }
            let end = bytes[s..].iter().position(|&b| b == b'\n').map_or(bytes.len(), |i| s + i);
            if let Some(d) = date(&bytes[s..end]) {
                return (s, Some(d));
            }
            s = end + 1;
        }
    };
    let (mut lo, mut hi) = (0usize, bytes.len());
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        match line_at(mid) {
            (_, Some(d)) if d < ymd => lo = mid + 1,
            _ => hi = mid,
        }
    }
    parse(&bytes[line_at(lo).0..])
}

fn date(line: &[u8]) -> Option<i64> {
    let line = trim(line);
    if line.len() < 10 || line[4] != b'/' || line[7] != b'/' {
        return None;
    }
    let y = digits(&line[0..4])?;
    let m = digits(&line[5..7])?;
    let d = digits(&line[8..10])?;
    Some(y as i64 * 10_000 + m as i64 * 100 + d as i64)
}

fn row(line: &[u8]) -> Option<Daily> {
    let line = trim(line);
    if line.first().is_none_or(|b| !b.is_ascii_digit()) {
        return None;
    }
    let mut fields = line.split(|&b| b == b',').map(trim);
    let when = fields.next()?;
    if when.len() != 10 || when[4] != b'/' || when[7] != b'/' {
        return None;
    }
    let mut d = Daily {
        year: digits(&when[0..4])? as i32,
        month: digits(&when[5..7])? as u32,
        day: digits(&when[8..10])? as u32,
        ..Daily::default()
    };
    d.open = number(fields.next()?)?;
    d.high = number(fields.next()?)?;
    d.low = number(fields.next()?)?;
    d.close = number(fields.next()?)?;
    d.volume = count(fields.next()?)?;
    d.open_interest = fields.next().map_or(Some(0), count)?;
    Some(d)
}

fn trim(mut s: &[u8]) -> &[u8] {
    while let [first, rest @ ..] = s {
        if first.is_ascii_whitespace() { s = rest } else { break }
    }
    while let [rest @ .., last] = s {
        if last.is_ascii_whitespace() { s = rest } else { break }
    }
    s
}

fn digits(s: &[u8]) -> Option<u64> {
    if s.is_empty() || s.len() > 19 {
        return None;
    }
    s.iter().try_fold(0u64, |n, &b| b.is_ascii_digit().then(|| n * 10 + u64::from(b - b'0')))
}

/// A count, which Sierra Chart sometimes writes as `12.0`.
fn count(s: &[u8]) -> Option<u64> {
    digits(s).or_else(|| number(s).filter(|v| *v >= 0.0).map(|v| v as u64))
}

const POW10: [f64; 23] = [
    1e0, 1e1, 1e2, 1e3, 1e4, 1e5, 1e6, 1e7, 1e8, 1e9, 1e10, 1e11, 1e12, 1e13, 1e14, 1e15, 1e16, 1e17, 1e18, 1e19, 1e20, 1e21, 1e22,
];

/// A decimal number. Up to 15 significant digits, which is every price
/// Sierra Chart writes, is one exact integer divided by an exact power of
/// ten, so the result is the correctly rounded double, the same as
/// `str::parse` gives; anything else goes to `str::parse`.
fn number(s: &[u8]) -> Option<f64> {
    let (neg, body) = match s.first() {
        Some(b'-') => (true, &s[1..]),
        _ => (false, s),
    };
    let mut mantissa = 0u64;
    let mut significant = 0;
    let mut scale = 0usize;
    let mut dot = false;
    let mut any = false;
    for &b in body {
        match b {
            b'0'..=b'9' => {
                any = true;
                if mantissa == 0 && b == b'0' {
                    if dot {
                        scale += 1;
                    }
                    continue;
                }
                significant += 1;
                mantissa = mantissa.wrapping_mul(10) + u64::from(b - b'0');
                if dot {
                    scale += 1;
                }
            }
            b'.' if !dot => dot = true,
            _ => return std::str::from_utf8(s).ok()?.parse().ok(),
        }
    }
    if !any {
        return None;
    }
    if significant > 15 || scale >= POW10.len() {
        return std::str::from_utf8(s).ok()?.parse().ok();
    }
    let v = mantissa as f64 / POW10[scale];
    Some(if neg { -v } else { v })
}

#[cfg(test)]
mod tests {
    use super::*;

    const FILE: &str = "Date,  Open,  High,  Low,  Close,  Volume,  OpenInterest\r\n\
        2026/09/10, 759000.00000000, 760000.00000000, 758000.00000000, 759950.00000000, 1500000, 0\r\n\
        2026/09/11, 759950.00000000, 768350.00000000, 759425.00000000, 765950.00000000, 1904677, 0\r\n\
        2026/09/14, 765950.00000000, 766000.00000000, 761000.00000000, 762025.00000000, 1200000, 12\r\n";

    #[test]
    fn rows_read_as_they_are_written() {
        let days = parse(FILE.as_bytes());
        assert_eq!(days.len(), 3);
        assert_eq!(
            days[1],
            Daily {
                year: 2026,
                month: 9,
                day: 11,
                open: 759_950.0,
                high: 768_350.0,
                low: 759_425.0,
                close: 765_950.0,
                volume: 1_904_677,
                open_interest: 0
            }
        );
        assert_eq!(days[2].open_interest, 12);
        assert_eq!(days[1].unix_seconds(), 1_789_084_800);
    }

    #[test]
    fn a_last_line_still_being_written_is_left_out() {
        let partial = format!("{FILE}2026/09/15, 762025.00000000, 7640");
        assert_eq!(parse(partial.as_bytes()).len(), 3);
        assert!(parse(b"Date,  Open").is_empty());
        assert!(parse(b"").is_empty());
    }

    #[test]
    fn a_header_only_file_has_no_days() {
        assert!(parse(b"Date,  Open,  High,  Low,  Close,  Volume,  OpenInterest\r\n").is_empty());
    }

    #[test]
    fn decimals_parse_exactly_as_the_standard_parser_would() {
        for s in ["1.2414", "0.00001", "765950.00000000", "4118.4", "78.36", "0", "-12.5", "1e3", "123456789012345678.5", ".5"] {
            assert_eq!(number(s.as_bytes()), s.parse::<f64>().ok(), "{s}");
        }
        assert_eq!(number(b""), None);
        assert_eq!(number(b"."), None);
        assert_eq!(number(b"abc"), None);
    }

    #[test]
    fn the_days_since_a_date_are_found_without_reading_the_rest() {
        let since = |ymd| parse_since(FILE.as_bytes(), ymd).iter().map(|d| d.day).collect::<Vec<_>>();
        assert_eq!(since(20_260_101), [10, 11, 14]);
        assert_eq!(since(20_260_911), [11, 14]);
        assert_eq!(since(20_260_912), [14]);
        assert!(since(20_260_915).is_empty());

        let mut big = String::from("Date,  Open,  High,  Low,  Close,  Volume,  OpenInterest\r\n");
        let mut days = Vec::new();
        for i in 0..3000 {
            let (y, m, d) = crate::time::civil_from_days(10_000 + i);
            days.push(i64::from(y) * 10_000 + i64::from(m) * 100 + i64::from(d));
            big.push_str(&format!("{y:04}/{m:02}/{d:02}, 1.5, 2.5, 1.0, 2.0, {i}, 0\r\n"));
        }
        for (i, &ymd) in days.iter().enumerate().step_by(37) {
            let got = parse_since(big.as_bytes(), ymd);
            assert_eq!(got.len(), 3000 - i);
            assert_eq!(got[0].volume, i as u64);
        }
    }
}
