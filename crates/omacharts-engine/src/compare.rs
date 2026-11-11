//! Comparing markets on one chart: other symbols drawn over the chart's own,
//! each rebased to 0% at one bar, so a line is how far that market has come
//! since then.
//!
//! What is kept is small — which symbols, a colour each if somebody chose one,
//! where the base is pinned if it is, and whether the chart's own bars give way
//! to a line. Everything else is worked out from the bars each symbol already
//! loads, at the chart's resolution, here, where it can be tested without a
//! window.
//!
//! Two pieces of arithmetic carry the feature, and both live here:
//!
//! - [`align`] lays another market's closes onto the chart's own bars. Markets
//!   keep different hours — bitcoin trades through the weekend, gold nearly
//!   round the clock, a stock only in its session — so a bar of the chart has
//!   no bar of the other market at exactly its time as often as it has one.
//!   Each bar takes the other market's last close at or before it, carried
//!   forward over the other's gaps. A weekend is then a flat stretch rather
//!   than a jump, and an overnight is nothing at all.
//! - [`Rebase`] turns a value into a change from the base bar, and a change
//!   into a price on the chart's own axis. The compared lines are drawn in
//!   the chart's prices, as if each market had started where the chart did,
//!   which is what lets the drawings, the marks and the scale's own gestures
//!   keep working unchanged while the axis is labelled in percent.

use serde::{Deserialize, Serialize};

use crate::bars::{Bar, Timeframe};
use crate::theme::{ColorChoice, Theme, SWATCH_SEQUENCE};

/// One market compared on a chart.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Comparison {
    pub symbol: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suffix: Option<String>,
    /// A colour somebody chose. Missing, it takes the theme's next one in the
    /// order indicators are handed theirs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<ColorChoice>,
}

impl Comparison {
    pub fn new(symbol: &str, suffix: Option<&str>) -> Comparison {
        Comparison { symbol: symbol.to_string(), suffix: suffix.map(str::to_string), color: None }
    }

    /// The symbol as it is written on the chart: `2330.TW`, `GC`.
    pub fn spelled(&self) -> String {
        match &self.suffix {
            Some(suffix) if !suffix.is_empty() => format!("{}.{suffix}", self.symbol),
            _ => self.symbol.clone(),
        }
    }

    /// Whether this is the market `symbol` and `suffix` name.
    pub fn is(&self, symbol: &str, suffix: Option<&str>) -> bool {
        self.symbol.eq_ignore_ascii_case(symbol)
            && self.suffix.as_deref().unwrap_or("").eq_ignore_ascii_case(suffix.unwrap_or(""))
    }
}

/// Everything a chart keeps about what it compares.
///
/// Empty is the chart as it always was, and is written down as nothing at
/// all, so a chart that never compared anything is stored exactly as before.
#[derive(Clone, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub struct Compare {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub symbols: Vec<Comparison>,
    /// The moment the lines are rebased at, when somebody pinned one. Missing,
    /// the base is the first bar on screen, so scrolling and zooming rebase
    /// everything: the lines are performance over the window in view.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<i64>,
    /// The chart's own bars drawn as a line like the others, for a race
    /// between lines and nothing else.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub lines_only: bool,
}

impl Compare {
    /// Nothing compared, and so nothing to keep: the base and the lines-only
    /// switch mean nothing without a market to compare.
    pub fn is_empty(&self) -> bool {
        self.symbols.is_empty()
    }

    /// Whether the chart is drawn in percent.
    pub fn active(&self) -> bool {
        !self.symbols.is_empty()
    }

    /// Add a market, unless it is already compared. True when it was added.
    pub fn add(&mut self, comparison: Comparison) -> bool {
        if self.position(&comparison.symbol, comparison.suffix.as_deref()).is_some() {
            return false;
        }
        self.symbols.push(comparison);
        true
    }

    /// Take a market off. True when it was there.
    pub fn remove(&mut self, symbol: &str, suffix: Option<&str>) -> bool {
        let Some(at) = self.position(symbol, suffix) else { return false };
        self.symbols.remove(at);
        if self.symbols.is_empty() {
            *self = Compare::default();
        }
        true
    }

    pub fn position(&self, symbol: &str, suffix: Option<&str>) -> Option<usize> {
        self.symbols.iter().position(|c| c.is(symbol, suffix))
    }

    /// The colour each compared market is drawn in, in order: its own choice,
    /// or the next of the theme's sequence that neither the chart's
    /// indicators nor an earlier market took.
    pub fn colors(&self, theme: &Theme, taken: &[String]) -> Vec<String> {
        let mut used: Vec<String> = taken.iter().map(|hex| hex.to_ascii_lowercase()).collect();
        let mut free = SWATCH_SEQUENCE
            .iter()
            .filter_map(|name| theme.swatch(name).map(|swatch| swatch.hex.clone()))
            .collect::<Vec<_>>();
        let mut next = 0usize;
        self.symbols
            .iter()
            .map(|comparison| {
                if let Some(choice) = &comparison.color {
                    let hex = choice.resolve(theme);
                    used.push(hex.to_ascii_lowercase());
                    return hex;
                }
                free.retain(|hex| !used.contains(&hex.to_ascii_lowercase()));
                let hex = match free.first() {
                    Some(hex) => hex.clone(),
                    // Six markets and six indicators: the sequence again, in
                    // order, as indicators past six get it.
                    None => {
                        let name = SWATCH_SEQUENCE[next % SWATCH_SEQUENCE.len()];
                        next += 1;
                        ColorChoice::swatch(name).resolve(theme)
                    }
                };
                used.push(hex.to_ascii_lowercase());
                hex
            })
            .collect()
    }
}

/// Six hours, in seconds: what a daily bar's time is moved by before its day
/// is read off it. See [`align`].
const DAY_LEAD: i64 = 6 * 3_600;

/// The moment a bar is matched by.
///
/// To the second, inside the day. A day or longer is matched by its day,
/// because feeds stamp a day differently: most at midnight, a future at the
/// evening its session opens, which is the day before by the calendar. Six
/// hours forward puts an evening open on the day it trades for and leaves a
/// midnight or a morning on its own.
fn moment(ts: i64, by_day: bool) -> i64 {
    match by_day {
        true => (ts + DAY_LEAD).div_euclid(86_400),
        false => ts,
    }
}

/// Another market's closes laid onto the chart's bars: for each of `main`,
/// the close of the last bar of `other` at or before it, or nothing before
/// `other` has begun.
///
/// Carried forward, so a market shut for the weekend or the night holds its
/// last close across the chart's bars until it trades again, rather than
/// dropping to nothing and leaving a break or, joined across, a slope through
/// prices that were never there.
///
/// One pass over each series: both are in time order.
pub fn align(main: &[Bar], other: &[Bar], timeframe: Timeframe) -> Vec<Option<f64>> {
    let by_day = timeframe.seconds() >= 86_400;
    let mut out = Vec::with_capacity(main.len());
    let mut at = 0usize;
    let mut last: Option<f64> = None;
    for bar in main {
        let when = moment(bar.ts, by_day);
        while let Some(next) = other.get(at) {
            if moment(next.ts, by_day) > when {
                break;
            }
            if next.close.is_finite() {
                last = Some(next.close);
            }
            at += 1;
        }
        out.push(last);
    }
    out
}

/// Where the lines are rebased: the bar the chart's own close is 0% at.
///
/// Pinned, the last bar at or before the pinned moment, or the first bar for
/// a moment before the series begins; on daily and longer bars, by the day,
/// so a date typed as midnight in one clock finds that day's bar however the
/// feed stamps it. Otherwise the first bar on screen.
pub fn base_index(main: &[Bar], first_visible: usize, from: Option<i64>, timeframe: Timeframe) -> usize {
    let last = main.len().saturating_sub(1);
    let by_day = timeframe.seconds() >= 86_400;
    match from {
        None => first_visible.min(last),
        Some(ts) => {
            let at = moment(ts, by_day);
            main.partition_point(|bar| moment(bar.ts, by_day) <= at).saturating_sub(1).min(last)
        }
    }
}

/// One series' base: its first value at or after the base bar. A market that
/// had not begun by then starts at 0% where it begins.
pub fn base_value(values: &[Option<f64>], base: usize) -> Option<f64> {
    values.get(base..)?.iter().flatten().copied().find(|value| *value != 0.0 && value.is_finite())
}

/// Turning values into changes from a base, and changes into prices on the
/// chart's own axis.
///
/// The chart's axis stays in the chart's own prices, and the percent is how
/// they are labelled. A compared market is drawn where the chart's price
/// would be had it moved by the same percentage, so the base value of every
/// line sits at the chart's own base close.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Rebase {
    /// The chart's own close at the base bar.
    pub anchor: f64,
}

impl Rebase {
    pub fn new(anchor: f64) -> Option<Rebase> {
        (anchor.is_finite() && anchor != 0.0).then_some(Rebase { anchor })
    }

    /// A value's change from `base`, as a fraction: 0.1 is ten percent up.
    pub fn change(value: f64, base: f64) -> f64 {
        value / base - 1.0
    }

    /// A price on the chart's axis, as a change from the anchor.
    pub fn change_at(self, price: f64) -> f64 {
        Rebase::change(price, self.anchor)
    }

    /// The price on the chart's axis a change from the anchor is drawn at.
    pub fn price_of(self, change: f64) -> f64 {
        self.anchor * (1.0 + change)
    }

    /// A compared value, as the price on the chart's axis it is drawn at.
    pub fn place(self, value: f64, base: f64) -> f64 {
        self.price_of(Rebase::change(value, base))
    }
}

/// A change written as the axis and the labels write it: a sign, and as
/// many places as `step` — the gap between gridlines, as a fraction — calls
/// for, at most two.
pub fn percent(change: f64, step: f64) -> String {
    let places = percent_places(step);
    let value = change * 100.0;
    // A change that rounds to nothing is written without a sign: "-0.0%"
    // reads as a fall.
    let rounded = (value * 10f64.powi(places as i32)).round();
    if rounded == 0.0 {
        return format!("{:.*}%", places, 0.0);
    }
    let sign = if value > 0.0 { "+" } else { "\u{2212}" };
    format!("{sign}{}%", crate::bars::grouped_decimals(value.abs(), places))
}

/// How many places a percentage needs on a scale whose gridlines are `step`
/// apart, as a fraction.
pub fn percent_places(step: f64) -> usize {
    let step = step * 100.0;
    match step {
        s if s >= 1.0 || s <= 0.0 || !s.is_finite() => 0,
        s if s >= 0.1 => 1,
        _ => 2,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: i64 = 86_400;

    fn bar(ts: i64, close: f64) -> Bar {
        Bar { ts, open: close, high: close, low: close, close, volume: 0.0 }
    }

    /// Monday the first of a week, at midnight UTC.
    const MONDAY: i64 = 1_767_571_200; // 2026-01-05

    #[test]
    fn a_market_shut_for_the_weekend_holds_fridays_close_across_it() {
        // Bitcoin every day, a stock on weekdays: the stock's Friday close
        // stands on Saturday and Sunday rather than jumping or vanishing.
        let btc: Vec<Bar> = (0..7).map(|d| bar(MONDAY + d * DAY, 100.0 + d as f64)).collect();
        let stock: Vec<Bar> = (0..5).map(|d| bar(MONDAY + d * DAY, 10.0 + d as f64)).collect();
        let aligned = align(&btc, &stock, Timeframe::days(1));
        assert_eq!(
            aligned,
            vec![Some(10.0), Some(11.0), Some(12.0), Some(13.0), Some(14.0), Some(14.0), Some(14.0)]
        );
    }

    #[test]
    fn the_weekend_a_stock_chart_never_shows_is_left_out_of_bitcoin() {
        // The other way round: a stock's chart has no weekend bars, so
        // bitcoin is read on its trading days and Monday takes Monday's.
        let stock: Vec<Bar> = [0, 1, 2, 3, 4, 7].iter().map(|d| bar(MONDAY + d * DAY, 1.0)).collect();
        let btc: Vec<Bar> = (0..8).map(|d| bar(MONDAY + d * DAY, 100.0 + d as f64)).collect();
        let aligned = align(&stock, &btc, Timeframe::days(1));
        assert_eq!(aligned.last(), Some(&Some(107.0)));
        assert_eq!(aligned[4], Some(104.0));
    }

    #[test]
    fn nothing_is_drawn_before_the_compared_market_begins() {
        let main: Vec<Bar> = (0..4).map(|d| bar(MONDAY + d * DAY, 1.0)).collect();
        let late = vec![bar(MONDAY + 2 * DAY, 50.0)];
        assert_eq!(align(&main, &late, Timeframe::days(1)), vec![None, None, Some(50.0), Some(50.0)]);
    }

    #[test]
    fn a_future_stamped_the_evening_before_is_that_days_bar() {
        // Gold's Tuesday session opens on Monday evening, 23:00 UTC; a stock's
        // Tuesday is stamped at midnight. They are the same day.
        let stock = vec![bar(MONDAY, 1.0), bar(MONDAY + DAY, 1.0)];
        let gold = vec![bar(MONDAY - 3_600, 2000.0), bar(MONDAY + DAY - 3_600, 2010.0)];
        assert_eq!(align(&stock, &gold, Timeframe::days(1)), vec![Some(2000.0), Some(2010.0)]);
    }

    #[test]
    fn inside_the_day_a_bar_is_matched_to_the_second() {
        let hour = 3_600;
        let main: Vec<Bar> = (0..4).map(|h| bar(MONDAY + h * hour, 1.0)).collect();
        // Shut for the second hour, as a session break is.
        let other = vec![bar(MONDAY, 5.0), bar(MONDAY + 2 * hour, 7.0), bar(MONDAY + 3 * hour, 8.0)];
        assert_eq!(
            align(&main, &other, Timeframe::hours(1)),
            vec![Some(5.0), Some(5.0), Some(7.0), Some(8.0)]
        );
    }

    #[test]
    fn a_close_that_is_not_a_number_is_carried_over() {
        let main: Vec<Bar> = (0..3).map(|d| bar(MONDAY + d * DAY, 1.0)).collect();
        let other = vec![bar(MONDAY, 5.0), bar(MONDAY + DAY, f64::NAN), bar(MONDAY + 2 * DAY, 6.0)];
        assert_eq!(align(&main, &other, Timeframe::days(1)), vec![Some(5.0), Some(5.0), Some(6.0)]);
    }

    #[test]
    fn the_base_is_the_first_bar_on_screen_unless_pinned() {
        let main: Vec<Bar> = (0..10).map(|d| bar(MONDAY + d * DAY, 1.0)).collect();
        let hourly: Vec<Bar> = (0..10).map(|h| bar(MONDAY + h * 3_600, 1.0)).collect();
        let (daily, hour) = (Timeframe::days(1), Timeframe::parse("1h").unwrap());
        assert_eq!(base_index(&main, 4, None, daily), 4);
        assert_eq!(base_index(&main, 40, None, daily), 9, "never past the last bar");
        assert_eq!(base_index(&main, 4, Some(MONDAY + 2 * DAY), daily), 2);
        assert_eq!(base_index(&main, 4, Some(MONDAY - DAY), daily), 0, "before the series, its first bar");
        assert_eq!(base_index(&[], 0, None, daily), 0);
        // Between two hourly bars, the one before.
        assert_eq!(base_index(&hourly, 0, Some(MONDAY + 2 * 3_600 + 5), hour), 2);
        // A date typed as midnight in New York, five hours into the UTC day,
        // finds the bar a feed stamped at that day's open, later still.
        let stamped: Vec<Bar> = (0..5).map(|d| bar(MONDAY + d * DAY + 14 * 3_600 + 1_800, 1.0)).collect();
        assert_eq!(base_index(&stamped, 0, Some(MONDAY + 2 * DAY + 5 * 3_600), daily), 2);
    }

    #[test]
    fn a_market_that_begins_after_the_base_is_based_where_it_begins() {
        let values = [None, None, Some(4.0), Some(5.0)];
        assert_eq!(base_value(&values, 0), Some(4.0));
        assert_eq!(base_value(&values, 3), Some(5.0));
        assert_eq!(base_value(&values, 9), None);
    }

    #[test]
    fn a_compared_market_is_drawn_where_the_chart_would_be_at_the_same_change() {
        // The chart starts at 200, gold at 2000. Gold up 10% is drawn at 220.
        let rebase = Rebase::new(200.0).unwrap();
        assert!((rebase.place(2200.0, 2000.0) - 220.0).abs() < 1e-9);
        assert!((rebase.change_at(220.0) - 0.1).abs() < 1e-12);
        assert!((rebase.price_of(-0.5) - 100.0).abs() < 1e-12);
        // At its base every line is at the chart's own base close.
        assert_eq!(rebase.place(2000.0, 2000.0), 200.0);
        assert!(Rebase::new(0.0).is_none());
        assert!(Rebase::new(f64::NAN).is_none());
    }

    #[test]
    fn percentages_are_written_with_a_sign_and_the_places_the_scale_needs() {
        assert_eq!(percent(0.1234, 0.05), "+12%");
        assert_eq!(percent(0.1234, 0.005), "+12.3%");
        assert_eq!(percent(-0.01234, 0.0005), "\u{2212}1.23%");
        assert_eq!(percent(0.0, 0.05), "0%");
        assert_eq!(percent(-0.00001, 0.005), "0.0%", "a change that rounds to nothing has no sign");
        assert_eq!(percent(12.5, 1.0), "+1,250%");
    }

    #[test]
    fn a_market_is_compared_once() {
        let mut compare = Compare::default();
        assert!(compare.add(Comparison::new("GC", None)));
        assert!(!compare.add(Comparison::new("gc", None)), "the same symbol, whatever its case");
        assert!(compare.add(Comparison::new("2330", Some("TW"))));
        assert_eq!(compare.position("2330", Some("tw")), Some(1));
        assert!(compare.remove("GC", None));
        assert!(!compare.remove("GC", None));
        assert_eq!(compare.symbols[0].spelled(), "2330.TW");
    }

    #[test]
    fn taking_the_last_market_off_forgets_the_rest() {
        let mut compare = Compare { from: Some(5), lines_only: true, ..Compare::default() };
        compare.add(Comparison::new("GC", None));
        compare.remove("GC", None);
        assert_eq!(compare, Compare::default());
    }

    #[test]
    fn an_empty_compare_is_written_as_nothing() {
        assert_eq!(serde_json::to_string(&Compare::default()).unwrap(), "{}");
        let read: Compare = serde_json::from_str("{}").unwrap();
        assert_eq!(read, Compare::default());
        let mut compare = Compare { from: Some(1_700_000_000), lines_only: true, ..Compare::default() };
        compare.add(Comparison { color: Some(ColorChoice::swatch("Teal")), ..Comparison::new("BTC-USD", None) });
        let json = serde_json::to_string(&compare).unwrap();
        assert_eq!(serde_json::from_str::<Compare>(&json).unwrap(), compare);
    }

    #[test]
    fn each_market_gets_a_colour_nothing_else_on_the_chart_wears() {
        let theme = crate::theme::builtin_themes().into_iter().next().expect("a theme");
        let blue = theme.swatch("Blue").unwrap().hex.clone();
        let amber = theme.swatch("Amber").unwrap().hex.clone();
        let mut compare = Compare::default();
        compare.add(Comparison::new("GC", None));
        compare.add(Comparison { color: Some(ColorChoice::swatch("Violet")), ..Comparison::new("NQ", None) });
        compare.add(Comparison::new("BTC-USD", None));
        let colors = compare.colors(&theme, std::slice::from_ref(&blue));
        assert_eq!(colors[0], amber, "Blue is the SMA's");
        assert_eq!(colors[1], theme.swatch("Violet").unwrap().hex);
        assert_eq!(colors[2], theme.swatch("Teal").unwrap().hex, "past Violet, which NQ chose");
    }
}
