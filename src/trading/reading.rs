//! What a chart says in numbers, for a program rather than an eye: its
//! bars, each indicator's latest values, and the levels a trader reads off
//! it — the session's VWAP and its bands, the volume profile's point of
//! control and value area, this session's and the last one's range.
//!
//! Written terse on purpose: a price carries the places its step has and
//! no more, so an agent reading it every half minute reads little.

use omacharts_chart::Drawn;
use omacharts_engine::Bar;
use omacharts_engine::indicators::{Kind, Output};
use serde_json::{Value, json};

/// `price` with as many places as `places` and none it does not need:
/// 7612.25, 7612.5, 7612.
pub fn num(price: f64, places: usize) -> String {
    let text = format!("{price:.places$}");
    match text.contains('.') {
        true => text.trim_end_matches('0').trim_end_matches('.').to_string(),
        false => text,
    }
}

/// How many places a price stepping `tick` has.
pub fn places(tick: Option<f64>) -> usize {
    tick.filter(|tick| tick.is_finite() && *tick > 0.0).map_or(2, |tick| omacharts_engine::on_tick(0.0, tick).1 as usize)
}

/// A price as a JSON number on its step, or null.
pub fn price(value: Option<f64>, places: usize) -> Value {
    match value.filter(|value| value.is_finite()) {
        Some(value) => {
            let scale = 10f64.powi(places as i32);
            json!((value * scale).round() / scale)
        }
        None => Value::Null,
    }
}

fn last(values: &[Option<f64>]) -> Option<f64> {
    values.last().copied().flatten()
}

/// A chart on screen in numbers, as the window read it.
#[derive(Clone, Debug)]
pub struct ChartRead {
    /// What the chart is called back: `pos:0 ES 15m in "Futures"`.
    pub label: String,
    pub symbol: String,
    /// Its resolution, `15m`.
    pub timeframe: String,
    /// A bar is shorter than a day.
    pub intraday: bool,
    /// How many places its prices have.
    pub places: usize,
    /// The clock it reads time on.
    pub zone: chrono_tz::Tz,
    /// Its last bars, oldest first; the last one may still be forming.
    pub bars: Vec<Bar>,
    /// Each indicator's latest values: see [`values`].
    pub values: Vec<Value>,
    pub levels: Levels,
}

/// The levels read off a chart. Each `None` where the chart has nothing
/// to read it from.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Levels {
    pub vwap: Option<f64>,
    /// Each enabled band of the VWAP, innermost first: deviations, upper,
    /// lower.
    pub bands: Vec<(f64, f64, f64)>,
    pub poc: Option<f64>,
    pub vah: Option<f64>,
    pub val: Option<f64>,
    /// This session's high and low.
    pub high: Option<f64>,
    pub low: Option<f64>,
    /// The session before's high, low and close.
    pub prior: Option<(f64, f64, f64)>,
}

/// The session a bar at `ts` belongs to: the trading day, CME's.
fn session_of(ts: i64) -> Option<chrono::NaiveDate> {
    chrono::DateTime::from_timestamp(ts, 0).map(super::limits::trading_day)
}

/// The levels of a chart with `bars`, drawing `drawn`; `intraday` when a
/// bar is shorter than a day, where sessions are told apart.
pub fn levels(bars: &[Bar], drawn: &[Drawn], intraday: bool) -> Levels {
    let mut levels = Levels::default();
    for one in drawn.iter().filter(|one| one.indicator.visible) {
        match (&one.indicator.kind, &one.output) {
            (Kind::Vwap, Output::Bands(bands)) if levels.vwap.is_none() => {
                levels.vwap = last(&bands.center);
                levels.bands = bands
                    .bands
                    .iter()
                    .filter(|band| band.enabled)
                    .filter_map(|band| Some((band.deviations, last(&band.upper)?, last(&band.lower)?)))
                    .collect();
            }
            (Kind::Vwap, Output::Line(line)) if levels.vwap.is_none() => levels.vwap = last(line),
            (Kind::VolumeProfile, Output::Profiles(profiles)) if levels.poc.is_none() => {
                if let Some(profile) = profiles.last().filter(|profile| !profile.rows.is_empty()) {
                    let (val, vah) = profile.value_area_bounds();
                    levels.poc = Some(profile.poc_price());
                    levels.vah = Some(vah);
                    levels.val = Some(val);
                }
            }
            _ => {}
        }
    }
    if intraday && let Some(now) = bars.last().and_then(|bar| session_of(bar.ts)) {
        let today: Vec<&Bar> = bars.iter().rev().take_while(|bar| session_of(bar.ts) == Some(now)).collect();
        levels.high = today.iter().map(|bar| bar.high).reduce(f64::max);
        levels.low = today.iter().map(|bar| bar.low).reduce(f64::min);
        let before = &bars[..bars.len() - today.len()];
        if let Some(then) = before.last().and_then(|bar| session_of(bar.ts)) {
            let prior: Vec<&Bar> = before.iter().rev().take_while(|bar| session_of(bar.ts) == Some(then)).collect();
            let high = prior.iter().map(|bar| bar.high).reduce(f64::max);
            let low = prior.iter().map(|bar| bar.low).reduce(f64::min);
            if let (Some(high), Some(low), Some(close)) = (high, low, before.last().map(|bar| bar.close)) {
                levels.prior = Some((high, low, close));
            }
        }
    }
    levels
}

impl Levels {
    /// One line: `vwap 7605.5 b1 7600.25/7610.75 poc 7608 vah 7612 val 7601 hi 7615 lo 7598 prev 7620/7590/7601`.
    pub fn text(&self, places: usize) -> String {
        let n = |price: f64| num(price, places);
        let mut parts = Vec::new();
        if let Some(vwap) = self.vwap {
            parts.push(format!("vwap {}", n(vwap)));
        }
        for (deviations, upper, lower) in &self.bands {
            parts.push(format!("{}sd {}/{}", num(*deviations, 2), n(*upper), n(*lower)));
        }
        for (name, value) in [("poc", self.poc), ("vah", self.vah), ("val", self.val), ("hi", self.high), ("lo", self.low)] {
            if let Some(value) = value {
                parts.push(format!("{name} {}", n(value)));
            }
        }
        if let Some((high, low, close)) = self.prior {
            parts.push(format!("prev h{} l{} c{}", n(high), n(low), n(close)));
        }
        parts.join(" ")
    }

    pub fn json(&self, places: usize) -> Value {
        let p = |value: Option<f64>| price(value, places);
        let bands: Vec<Value> = self
            .bands
            .iter()
            .map(|(deviations, upper, lower)| json!({"sd": deviations, "up": p(Some(*upper)), "dn": p(Some(*lower))}))
            .collect();
        json!({
            "vwap": p(self.vwap),
            "bands": bands,
            "poc": p(self.poc),
            "vah": p(self.vah),
            "val": p(self.val),
            "hi": p(self.high),
            "lo": p(self.low),
            "prev": self.prior.map(|(high, low, close)| json!({"h": p(Some(high)), "l": p(Some(low)), "c": p(Some(close))})),
        })
    }
}

/// Each indicator on a chart with its latest values, as `chart values`
/// gives them.
pub fn values(drawn: &[Drawn], places: usize, label: impl Fn(&Drawn) -> String) -> Vec<Value> {
    let p = |value: Option<f64>| price(value, places);
    drawn
        .iter()
        .filter_map(|one| {
            let mut row = json!({
                "id": one.indicator.id,
                "kind": one.indicator.kind.key(),
                "label": label(one),
                "visible": one.indicator.visible,
            });
            let fields = match &one.output {
                Output::Line(line) => json!({"value": p(last(line))}),
                Output::Bands(bands) => json!({
                    "value": p(last(&bands.center)),
                    "bands": bands.bands.iter().filter(|band| band.enabled).map(|band| json!({
                        "deviations": band.deviations,
                        "upper": p(last(&band.upper)),
                        "lower": p(last(&band.lower)),
                    })).collect::<Vec<_>>(),
                }),
                Output::Profiles(profiles) => match profiles.last().filter(|profile| !profile.rows.is_empty()) {
                    Some(profile) => {
                        let (val, vah) = profile.value_area_bounds();
                        json!({
                            "from": profile.start_ts,
                            "to": profile.end_ts,
                            "poc": p(Some(profile.poc_price())),
                            "vah": p(Some(vah)),
                            "val": p(Some(val)),
                            "volume": profile.total_volume.round(),
                        })
                    }
                    None => json!({"poc": null, "vah": null, "val": null}),
                },
                Output::Volume { values, .. } => json!({"value": values.last()}),
                Output::Pane(pane) => json!({
                    "value": last(&pane.values),
                    "signal": pane.signal.as_deref().and_then(last),
                    "histogram": pane.histogram.as_deref().and_then(last),
                }),
                Output::Time(_) => return None,
                Output::Unmet(why) => json!({"unmet": why}),
            };
            if let (Some(row), Value::Object(fields)) = (row.as_object_mut(), fields) {
                row.extend(fields);
            }
            Some(row)
        })
        .collect()
}

/// A bar as `[time, open, high, low, close, volume]`, prices on their step.
pub fn bar_json(bar: &Bar, places: usize) -> Value {
    let p = |value: f64| price(Some(value), places);
    json!([bar.ts, p(bar.open), p(bar.high), p(bar.low), p(bar.close), bar.volume.round()])
}

/// Bars as a terse table, one a line: time, open, high, low, close,
/// volume. The time is `HH:MM` in `zone` for bars shorter than a day, the
/// date for longer.
pub fn bars_text(bars: &[Bar], places: usize, intraday: bool, zone: chrono_tz::Tz) -> String {
    let mut out = String::new();
    for bar in bars {
        let at = chrono::DateTime::from_timestamp(bar.ts, 0).map(|at| at.with_timezone(&zone));
        let time = match (at, intraday) {
            (Some(at), true) => at.format("%H:%M").to_string(),
            (Some(at), false) => at.format("%m-%d").to_string(),
            (None, _) => bar.ts.to_string(),
        };
        let n = |price: f64| num(price, places);
        out.push_str(&format!("{time} {} {} {} {} {}\n", n(bar.open), n(bar.high), n(bar.low), n(bar.close), bar.volume.round()));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bar(ts: i64, open: f64, high: f64, low: f64, close: f64) -> Bar {
        Bar { ts, open, high, low, close, volume: 10.0 }
    }

    #[test]
    fn a_price_has_no_places_it_does_not_need() {
        assert_eq!(num(7612.25, 2), "7612.25");
        assert_eq!(num(7612.5, 2), "7612.5");
        assert_eq!(num(7612.0, 2), "7612");
        assert_eq!(num(1.08455, 5), "1.08455");
        assert_eq!(num(120.0, 0), "120");
        assert_eq!(places(Some(0.25)), 2);
    }

    #[test]
    fn sessions_are_told_apart_at_five_in_chicago() {
        // 2026-11-16 16:00 CST and 16:30, then 17:00 and 17:30, in UTC.
        let bars = [
            bar(1_794_866_400, 10.0, 12.0, 9.0, 11.0),
            bar(1_794_868_200, 11.0, 13.0, 10.0, 12.0),
            bar(1_794_870_000, 12.0, 14.0, 11.5, 13.0),
            bar(1_794_871_800, 13.0, 15.0, 12.5, 14.0),
        ];
        let levels = levels(&bars, &[], true);
        assert_eq!((levels.high, levels.low), (Some(15.0), Some(11.5)));
        assert_eq!(levels.prior, Some((13.0, 9.0, 12.0)));
        assert_eq!(levels.text(2), "hi 15 lo 11.5 prev h13 l9 c12");
        let daily = super::levels(&bars, &[], false);
        assert_eq!(daily.high, None, "no sessions on a daily chart");
    }

    #[test]
    fn bars_are_a_line_each() {
        let text = bars_text(&[bar(1_794_866_400, 10.0, 12.25, 9.5, 11.0)], 2, true, chrono_tz::America::Chicago);
        assert_eq!(text, "16:00 10 12.25 9.5 11 10\n");
        assert_eq!(bar_json(&bar(5, 1.0, 2.0, 0.5, 1.5), 2), json!([5, 1.0, 2.0, 0.5, 1.5, 10.0]));
    }
}
