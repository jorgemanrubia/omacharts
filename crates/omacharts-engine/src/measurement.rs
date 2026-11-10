//! Range measurements shared by the pointer tool and terminal commands.
use crate::{Anchor, Bar, InstrumentKind, grouped_decimals, price_decimals};
use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct Measurement {
    pub from: Anchor,
    pub to: Anchor,
    pub price_change: f64,
    pub percent_change: Option<f64>,
    /// Distance between columns, not an inclusive candle count.
    pub bars: Option<u64>,
    pub elapsed_seconds: u64,
    /// How many decimals the change is written to: the price axis's rule
    /// for the starting price, so a share moves in cents and a currency in
    /// fractions of a pip.
    #[serde(skip)]
    pub decimals: usize,
}

/// A measurement as words, in pieces, so the chart can set the percentage
/// apart and a terminal can run them together.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Parts {
    /// `+0.53%`, or nothing when the start is at zero and no percentage exists.
    pub percent: Option<String>,
    /// `+4.01`, at the price's own precision with its thousands marked.
    pub change: String,
    /// `71 bars, 5h 55m`, or only the time when there are too few candles to
    /// count columns.
    pub span: String,
}

impl Parts {
    /// The pieces on one line: `+0.53% +4.01; 71 bars, 5h 55m`.
    pub fn line(&self) -> String {
        match &self.percent {
            Some(percent) => format!("{percent} {}; {}", self.change, self.span),
            None => format!("{}; {}", self.change, self.span),
        }
    }
}

impl Measurement {
    pub fn between(from: Anchor, to: Anchor, bars: &[Bar], kind: Option<InstrumentKind>) -> Self {
        let price_change = to.price - from.price;
        Self {
            from,
            to,
            price_change,
            percent_change: (from.price != 0.0)
                .then(|| price_change / from.price.abs() * 100.0)
                .filter(|p| p.is_finite()),
            bars: (bars.len() >= 2)
                .then(|| (column(bars, to.ts) - column(bars, from.ts)).abs().round() as u64),
            elapsed_seconds: to.ts.abs_diff(from.ts),
            // From the end when the start is at zero, which has no precision
            // of its own to give.
            decimals: price_decimals(
                0.0,
                if from.price == 0.0 { to.price.abs() } else { from.price.abs() },
                kind,
            ),
        }
    }

    pub fn parts(&self) -> Parts {
        let duration = duration(self.elapsed_seconds);
        Parts {
            percent: self.percent_change.map(|p| format!("{}%", signed(p, 2))),
            change: signed(self.price_change, self.decimals),
            span: match self.bars {
                Some(1) => format!("1 bar, {duration}"),
                Some(n) => format!("{n} bars, {duration}"),
                None => duration,
            },
        }
    }
}

/// A change with its sign always shown, `+` on a gain and on nothing at all,
/// as the watchlist writes one. A loss too small to survive the rounding is
/// no change, not `-0.00`.
fn signed(value: f64, decimals: usize) -> String {
    let text = grouped_decimals(value.abs(), decimals);
    let nothing = !text.chars().any(|c| c.is_ascii_digit() && c != '0');
    match value < 0.0 && !nothing {
        true => format!("-{text}"),
        false => format!("+{text}"),
    }
}

/// Calendar time in its two largest units, the smaller left off when it is
/// zero: `7y 7mo`, `3mo 1w`, `5w 6d`, `12d 4h`, `5h 55m`, `5h`, `40s`.
///
/// A span of two weeks or more is written in weeks, two months or more in
/// months, and a year or more in years, since a long count of days is a
/// number to divide in your head. A year is 365 days and a month a twelfth
/// of one: a span in seconds has no calendar to count real months on.
fn duration(seconds: u64) -> String {
    const WEEK: u64 = 7 * 86400;
    const YEAR: u64 = 365 * 86400;
    const MONTH: u64 = YEAR / 12;
    let (big, small, units) = if seconds >= YEAR {
        (seconds / YEAR, seconds % YEAR / MONTH, ("y", "mo"))
    } else if seconds >= 2 * MONTH {
        (seconds / MONTH, seconds % MONTH / WEEK, ("mo", "w"))
    } else if seconds >= 2 * WEEK {
        (seconds / WEEK, seconds % WEEK / 86400, ("w", "d"))
    } else if seconds >= 86400 {
        (seconds / 86400, seconds % 86400 / 3600, ("d", "h"))
    } else if seconds >= 3600 {
        (seconds / 3600, seconds % 3600 / 60, ("h", "m"))
    } else if seconds >= 60 {
        (seconds / 60, seconds % 60, ("m", "s"))
    } else {
        return format!("{seconds}s");
    };
    match small {
        0 => format!("{big}{}", units.0),
        _ => format!("{big}{} {small}{}", units.0, units.1),
    }
}

/// The chart's evenly spaced columns interpolate gaps and extrapolate at
/// the end's cadence, just as drawing anchors do. Call with at least two bars.
fn column(bars: &[Bar], ts: i64) -> f64 {
    let at = bars.partition_point(|b| b.ts < ts);
    let right = at.clamp(1, bars.len() - 1);
    let left = right - 1;
    left as f64
        + (ts as f64 - bars[left].ts as f64)
            / (bars[right].ts as f64 - bars[left].ts as f64).max(1.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn series() -> Vec<Bar> {
        [0, 86400, 4 * 86400, 5 * 86400]
            .into_iter()
            .map(|ts| Bar {
                ts,
                open: 100.0,
                high: 110.0,
                low: 90.0,
                close: 105.0,
                volume: 1.0,
            })
            .collect()
    }
    #[test]
    fn market_gaps_count_columns_but_elapsed_time_counts_the_weekend() {
        let m = Measurement::between(
            Anchor::new(86400, 100.0),
            Anchor::new(4 * 86400, 110.0),
            &series(),
            None,
        );
        assert_eq!(m.bars, Some(1));
        assert_eq!(m.elapsed_seconds, 3 * 86400);
        assert_eq!(m.percent_change, Some(10.0));
        let reversed = Measurement::between(m.to, m.from, &series(), None);
        assert_eq!(reversed.bars, m.bars);
        assert_eq!(reversed.price_change, -10.0);
    }
    #[test]
    fn zero_prices_missing_data_and_future_columns_are_explicit() {
        let a = Anchor::new(5 * 86400, 0.0);
        let b = Anchor::new(7 * 86400, 10.0);
        assert_eq!(Measurement::between(a, b, &series(), None).bars, Some(2));
        let m = Measurement::between(a, b, &[], None);
        assert_eq!(m.bars, None);
        assert_eq!(m.percent_change, None);
        let same = Measurement::between(a, a, &series(), None);
        assert_eq!(
            (same.bars, same.elapsed_seconds, same.price_change),
            (Some(0), 0, 0.0)
        );
    }

    fn parts(from: f64, to: f64, kind: InstrumentKind) -> Parts {
        Measurement::between(
            Anchor::new(86400, from),
            Anchor::new(4 * 86400, to),
            &series(),
            Some(kind),
        )
        .parts()
    }

    #[test]
    fn the_change_is_written_as_the_price_axis_writes_the_price() {
        let share = parts(750.0, 754.01, InstrumentKind::Equity);
        assert_eq!(share.percent.as_deref(), Some("+0.53%"));
        assert_eq!(share.change, "+4.01");
        assert_eq!(share.span, "1 bar, 3d");
        assert_eq!(share.line(), "+0.53% +4.01; 1 bar, 3d");

        let penny = parts(0.0042, 0.0040, InstrumentKind::Equity);
        assert_eq!((penny.percent.as_deref(), penny.change.as_str()), (Some("-4.76%"), "-0.000200"));

        let fx = parts(1.11828, 1.11700, InstrumentKind::Fx);
        assert_eq!((fx.percent.as_deref(), fx.change.as_str()), (Some("-0.11%"), "-0.00128"));

        let btc = parts(77861.97, 79123.45, InstrumentKind::Crypto);
        assert_eq!((btc.percent.as_deref(), btc.change.as_str()), (Some("+1.62%"), "+1,261.48"));

        // A loss that rounds away is no change, signed as one.
        let flat = parts(750.0, 749.999, InstrumentKind::Equity);
        assert_eq!((flat.percent.as_deref(), flat.change.as_str()), (Some("+0.00%"), "+0.00"));
    }

    #[test]
    fn what_cannot_be_measured_is_left_out_rather_than_written_as_missing() {
        // No percentage from zero; the change still takes the end's precision.
        let from_zero = Measurement::between(Anchor::new(0, 0.0), Anchor::new(86400, 10.0), &series(), None);
        assert_eq!(from_zero.parts().line(), "+10.00; 1 bar, 1d");
        // Too few candles to count columns: the time alone.
        let bare = Measurement::between(Anchor::new(0, 100.0), Anchor::new(32 * 86400, 101.0), &[], None);
        assert_eq!(bare.parts().line(), "+1.00% +1.00; 4w 4d");
    }

    #[test]
    fn elapsed_time_leaves_off_a_smaller_unit_that_is_zero() {
        assert_eq!(duration(13 * 86400 + 3 * 3600), "13d 3h");
        assert_eq!(duration(4 * 86400), "4d");
        assert_eq!(duration(5 * 3600 + 55 * 60), "5h 55m");
        assert_eq!(duration(5 * 3600), "5h");
        assert_eq!(duration(90), "1m 30s");
        assert_eq!(duration(40), "40s");
        assert_eq!(duration(0), "0s");
    }

    #[test]
    fn a_long_span_is_written_in_weeks_months_and_years() {
        assert_eq!(duration(14 * 86400), "2w");
        assert_eq!(duration(41 * 86400), "5w 6d");
        assert_eq!(duration(60 * 86400), "8w 4d");
        assert_eq!(duration(100 * 86400), "3mo 1w");
        assert_eq!(duration(364 * 86400), "11mo 4w");
        assert_eq!(duration(365 * 86400), "1y");
        assert_eq!(duration(2771 * 86400), "7y 7mo");
    }
}
