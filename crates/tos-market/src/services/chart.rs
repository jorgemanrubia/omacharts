//! `chart`: price history (candles) for one symbol, streamed — a snapshot
//! with the candles arrays, then patches as bars form.

use serde::Deserialize;
use serde_json::json;

use crate::protocol::Request;

/// What to ask the chart service for, always with extended hours.
/// `time_aggregation` and `range` are the platform's own codes (`MIN1`,
/// `MIN5`, `DAY`, … / `DAY1`, `DAY5`, `MONTH1`, `YEAR2`, …).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChartParams {
    pub symbol: String,
    pub time_aggregation: String,
    pub range: String,
}

impl ChartParams {
    pub fn new(symbol: &str, time_aggregation: &str, range: &str) -> Self {
        ChartParams {
            symbol: symbol.into(),
            time_aggregation: time_aggregation.into(),
            range: range.into(),
        }
    }
}

/// One request id per (symbol, aggregation), so `/ES MIN1` and `/ES MIN30`
/// can stream together. Slash in the symbol is kept: ids already look like
/// `chart-/ES`.
pub fn chart_request(p: &ChartParams) -> Request {
    Request::new(
        "chart",
        format!("chart-{}-{}", p.symbol, p.time_aggregation),
        1,
        json!({
            "symbol": p.symbol,
            "timeAggregation": p.time_aggregation,
            "studies": [],
            "range": p.range,
            "extendedHours": true,
        }),
    )
}

/// Parallel arrays, one entry per bar; `timestamps` are ms since the epoch.
#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct ChartCandles {
    pub opens: Vec<f64>,
    pub highs: Vec<f64>,
    pub lows: Vec<f64>,
    pub closes: Vec<f64>,
    /// `null` per bar on an index (`$ADSPD` has no volume): read as 0.
    #[serde(deserialize_with = "nullable_zero")]
    pub volumes: Vec<f64>,
    pub timestamps: Vec<f64>,
}

fn nullable_zero<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Vec<f64>, D::Error> {
    Ok(Vec::<Option<f64>>::deserialize(d)?
        .into_iter()
        .map(|v| v.unwrap_or(0.0))
        .collect())
}

impl ChartCandles {
    pub fn len(&self) -> usize {
        self.timestamps
            .len()
            .min(self.opens.len())
            .min(self.highs.len())
            .min(self.lows.len())
            .min(self.closes.len())
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct ChartBody {
    pub symbol: String,
    pub candles: ChartCandles,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn candles_decode_and_count_the_shortest_array() {
        let body: ChartBody = serde_json::from_value(json!({
            "symbol": "VIX",
            "candles": {"opens": [1.0, 2.0], "highs": [1.5, 2.5], "lows": [0.5, 1.5],
                        "closes": [1.2, 2.2], "volumes": [0, 0], "timestamps": [1000, 2000]}
        }))
        .unwrap();
        assert_eq!(body.symbol, "VIX");
        assert_eq!(body.candles.len(), 2);
        let req = chart_request(&ChartParams::new("VIX", "MIN5", "DAY1"));
        assert_eq!(req.id(), "chart-VIX-MIN5");
    }

    #[test]
    fn two_aggregations_of_the_same_symbol_get_distinct_request_ids() {
        let min1 = chart_request(&ChartParams::new("/ES", "MIN1", "DAY1"));
        let min30 = chart_request(&ChartParams::new("/ES", "MIN30", "DAY10"));
        assert_eq!(min1.id(), "chart-/ES-MIN1");
        assert_eq!(min30.id(), "chart-/ES-MIN30");
    }

    #[test]
    fn an_index_chart_with_null_volumes_still_decodes() {
        // Recorded shape: `$ADSPD` MIN5 DAY1 sends `volumes: [null, …]`.
        let body: ChartBody = serde_json::from_value(json!({
            "symbol": "$ADSPD",
            "candles": {"opens": [120.0, -30.0], "highs": [140.0, 10.0], "lows": [100.0, -60.0],
                        "closes": [-30.0, -55.0], "volumes": [null, null], "timestamps": [1000, 2000]}
        }))
        .unwrap();
        assert_eq!(body.candles.len(), 2);
        assert_eq!(body.candles.volumes, vec![0.0, 0.0]);
    }
}
