//! Range measurements shared by the pointer tool and terminal commands.
use crate::{Anchor, Bar};
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
}

impl Measurement {
    pub fn between(from: Anchor, to: Anchor, bars: &[Bar]) -> Self {
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
        }
    }

    pub fn lines(&self) -> [String; 2] {
        let percent = self
            .percent_change
            .map(|p| format!("{p:+.2}%"))
            .unwrap_or_else(|| "n/a".into());
        let bars = self
            .bars
            .map(|n| n.to_string())
            .unwrap_or_else(|| "n/a".into());
        let seconds = self.elapsed_seconds;
        let duration = if seconds >= 86400 {
            format!("{}d {}h", seconds / 86400, seconds % 86400 / 3600)
        } else if seconds >= 3600 {
            format!("{}h {}m", seconds / 3600, seconds % 3600 / 60)
        } else if seconds >= 60 {
            format!("{}m {}s", seconds / 60, seconds % 60)
        } else {
            format!("{seconds}s")
        };
        [
            format!("{:+.4} ({percent})", self.price_change),
            format!("{bars} bars, {duration}"),
        ]
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
        );
        assert_eq!(m.bars, Some(1));
        assert_eq!(m.elapsed_seconds, 3 * 86400);
        assert_eq!(m.percent_change, Some(10.0));
        let reversed = Measurement::between(m.to, m.from, &series());
        assert_eq!(reversed.bars, m.bars);
        assert_eq!(reversed.price_change, -10.0);
    }
    #[test]
    fn zero_prices_missing_data_and_future_columns_are_explicit() {
        let a = Anchor::new(5 * 86400, 0.0);
        let b = Anchor::new(7 * 86400, 10.0);
        assert_eq!(Measurement::between(a, b, &series()).bars, Some(2));
        let m = Measurement::between(a, b, &[]);
        assert_eq!(m.bars, None);
        assert_eq!(m.percent_change, None);
        let same = Measurement::between(a, a, &series());
        assert_eq!(
            (same.bars, same.elapsed_seconds, same.price_change),
            (Some(0), 0, 0.0)
        );
    }
}
