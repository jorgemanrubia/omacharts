//! Bars, timeframes, and turning one timeframe into another.

use chrono::{Datelike, TimeZone, Utc};
use serde::{Deserialize, Serialize};

use crate::symbols::InstrumentKind;

/// One candle. `ts` is the unix second the bar opens at.
#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
pub struct Bar {
    pub ts: i64,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub volume: f64,
}

/// How each bar is drawn.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BarStyle {
    #[default]
    Candles,
    /// Open and close as ticks either side of a high-low line.
    Ohlc,
}

impl BarStyle {
    pub const ALL: [BarStyle; 2] = [BarStyle::Candles, BarStyle::Ohlc];

    pub fn label(self) -> &'static str {
        match self {
            BarStyle::Candles => "Candles",
            BarStyle::Ohlc => "OHLC bars",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            BarStyle::Candles => "candles",
            BarStyle::Ohlc => "ohlc",
        }
    }

    pub fn from_key(key: &str) -> Option<BarStyle> {
        BarStyle::ALL.into_iter().find(|s| s.key() == key)
    }
}

/// The unit a resolution is counted in.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Unit {
    Minute,
    Hour,
    Day,
    Week,
    Month,
    Year,
}

/// A chart resolution: a count of a unit.
///
/// Not an enum of presets, because people type resolutions. "3m" is a perfectly
/// reasonable thing to want and no fixed list will ever contain everyone's.
/// The strip in the header is just [`Timeframe::PRESETS`]; anything parseable
/// works.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
pub struct Timeframe {
    pub count: u32,
    pub unit: Unit,
}

impl Default for Timeframe {
    fn default() -> Timeframe {
        Timeframe::days(1)
    }
}

impl Timeframe {
    pub const fn minutes(count: u32) -> Timeframe {
        Timeframe { count, unit: Unit::Minute }
    }
    pub const fn hours(count: u32) -> Timeframe {
        Timeframe { count, unit: Unit::Hour }
    }
    pub const fn days(count: u32) -> Timeframe {
        Timeframe { count, unit: Unit::Day }
    }
    pub const fn weeks(count: u32) -> Timeframe {
        Timeframe { count, unit: Unit::Week }
    }
    pub const fn months(count: u32) -> Timeframe {
        Timeframe { count, unit: Unit::Month }
    }
    pub const fn years(count: u32) -> Timeframe {
        Timeframe { count, unit: Unit::Year }
    }

    /// What the header strip offers. Everything else is typed.
    pub const PRESETS: [Timeframe; 6] = [
        Timeframe::minutes(5),
        Timeframe::minutes(15),
        Timeframe::hours(1),
        Timeframe::hours(4),
        Timeframe::days(1),
        Timeframe::weeks(1),
    ];

    /// The intervals a provider is expected to serve directly. Anything else
    /// is folded from one of these.
    const SERVED_MINUTES: [u32; 7] = [1, 2, 5, 15, 30, 60, 90];

    /// How long a bar lasts. Months and years are an average length, since
    /// no fixed number is right: fine for ordering and spacing, never for
    /// finding where a bar begins — [`bucket_of`] is for that.
    pub fn seconds(self) -> i64 {
        let unit = match self.unit {
            Unit::Minute => 60,
            Unit::Hour => 3_600,
            Unit::Day => 86_400,
            Unit::Week => 604_800,
            Unit::Month => 2_629_746,
            Unit::Year => 31_556_952,
        };
        unit * self.count.max(1) as i64
    }

    /// "5m", "4h", "1D", "1W", "1M", "1Y" — what you type and what is stored.
    pub fn key(self) -> String {
        let suffix = match self.unit {
            Unit::Minute => 'm',
            Unit::Hour => 'h',
            Unit::Day => 'D',
            Unit::Week => 'W',
            Unit::Month => 'M',
            Unit::Year => 'Y',
        };
        format!("{}{suffix}", self.count)
    }

    pub fn label(self) -> String {
        self.key()
    }

    /// Spelled out, for confirming what was typed.
    ///
    /// Reads back the *normalised* resolution, so typing "240" says "4 hours"
    /// — which is both the honest answer and the thing worth knowing before
    /// pressing Enter.
    pub fn description(self) -> String {
        let count = self.count.max(1);
        let unit = match self.unit {
            Unit::Minute => "minute",
            Unit::Hour => "hour",
            Unit::Day => "day",
            Unit::Week => "week",
            Unit::Month => "month",
            Unit::Year => "year",
        };
        if count == 1 {
            format!("1 {unit}")
        } else {
            format!("{count} {unit}s")
        }
    }

    /// Parse what someone typed.
    ///
    /// A bare number means minutes, the way every charting package has worked
    /// since the 80s: "15" is fifteen minutes, "240" is four hours. A suffix
    /// is honoured when given, and case does not matter — with one exception:
    /// "M" alone is a month and "m" a minute, the way every other charting
    /// package spells them. "mo" is a month in any case, for anyone who would
    /// rather not lean on the shift key.
    pub fn parse(text: &str) -> Option<Timeframe> {
        let text = text.trim();
        if text.is_empty() {
            return None;
        }
        let (digits, suffix): (String, String) =
            text.chars().partition(|c| c.is_ascii_digit());
        let suffix = suffix.trim();
        let month = suffix == "M";
        let suffix = suffix.to_lowercase();

        // "D" and "W" on their own mean one of them.
        let count: u32 = if digits.is_empty() { 1 } else { digits.parse().ok()? };
        if count == 0 {
            return None;
        }

        let unit = match suffix.as_str() {
            _ if month => Unit::Month,
            "" => Unit::Minute,
            "m" | "min" | "mins" | "minute" | "minutes" => Unit::Minute,
            "h" | "hr" | "hrs" | "hour" | "hours" => Unit::Hour,
            "d" | "day" | "days" => Unit::Day,
            "w" | "wk" | "week" | "weeks" => Unit::Week,
            "mo" | "month" | "months" => Unit::Month,
            "y" | "yr" | "yrs" | "year" | "years" => Unit::Year,
            _ => return None,
        };
        Some(Timeframe { count, unit }.normalised())
    }

    /// Collapse a resolution onto the largest unit that expresses it exactly,
    /// so "60m" and "1h" are the same thing rather than two chart states.
    pub fn normalised(self) -> Timeframe {
        match self.unit {
            Unit::Minute if self.count.is_multiple_of(1440) => Timeframe::days(self.count / 1440),
            Unit::Minute if self.count.is_multiple_of(60) => Timeframe::hours(self.count / 60),
            Unit::Hour if self.count.is_multiple_of(24) => Timeframe::days(self.count / 24),
            Unit::Day if self.count.is_multiple_of(7) => Timeframe::weeks(self.count / 7),
            Unit::Month if self.count.is_multiple_of(12) => Timeframe::years(self.count / 12),
            _ => self,
        }
    }

    /// The resolution to actually fetch, which this one folds from.
    ///
    /// The largest served interval that divides this one evenly — folding
    /// needs whole bars, and asking for a finer interval than necessary burns
    /// history limits and rate limit for nothing.
    pub fn native(self) -> Timeframe {
        match self.unit {
            Unit::Minute => {
                if Timeframe::SERVED_MINUTES.contains(&self.count) {
                    return self;
                }
                let base = Timeframe::SERVED_MINUTES
                    .into_iter()
                    .rev()
                    .find(|m| self.count.is_multiple_of(*m))
                    .unwrap_or(1);
                Timeframe::minutes(base)
            }
            Unit::Hour => {
                if self.count == 1 {
                    self
                } else {
                    Timeframe::hours(1)
                }
            }
            Unit::Day | Unit::Week | Unit::Month | Unit::Year => {
                if self.unit == Unit::Day && self.count == 1 {
                    self
                } else {
                    Timeframe::days(1)
                }
            }
        }
    }

    pub fn is_derived(self) -> bool {
        self.native() != self
    }

    pub fn is_intraday(self) -> bool {
        matches!(self.unit, Unit::Minute | Unit::Hour)
    }

    /// Does each bar open on the first of a calendar month, 00:00 UTC? Read
    /// in local time west of Greenwich, that is the evening before — the
    /// wrong month, and on January the wrong year.
    pub fn is_calendar(self) -> bool {
        matches!(self.unit, Unit::Month | Unit::Year)
    }
}

/// How many decimals a price deserves, given how finely the chart is ruled and
/// what is being priced.
///
/// Two answers have to agree. The gridline spacing says what the *chart*
/// needs: lines 50 apart do not want three decimals. The instrument says what
/// the *market* quotes: a currency major is quoted to five places and moves in
/// the fourth, so rounding it to the step makes a day's range look like
/// nothing happened. The finer of the two wins.
///
/// Kind matters and magnitude alone cannot stand in for it. EURUSD at 1.12
/// and USDJPY at 157 are both currency majors quoted to a fractional pip, but
/// that is five decimals for one and three for the other — and a share price
/// of 1.12 wants neither.
pub fn price_decimals(step: f64, price: f64, kind: Option<InstrumentKind>) -> usize {
    let from_step = if step > 0.0 {
        (-step.log10().floor()).clamp(0.0, 8.0) as usize
    } else {
        0
    };

    let size = price.abs();
    let from_instrument = match kind {
        // Quoted to a fractional pip: five places, or three where the quote
        // runs in the hundreds, as the yen pairs do.
        Some(InstrumentKind::Fx) if size >= 20.0 => 3,
        Some(InstrumentKind::Fx) => 5,
        // Anything priced like a share, an index or a contract is quoted in
        // cents, and that holds at $15 as much as at $150. Giving the teens
        // four places was over-correction: it is where VIX at 15.31 picked up
        // two trailing zeros nobody asked for. Below a dollar the cent stops
        // being a useful unit, so precision comes back.
        _ => match size {
            p if p >= 1.0 => 2,
            p if p >= 0.01 => 4,
            _ => 6,
        },
    };
    from_step.max(from_instrument)
}

/// A bar whose body is this small a fraction of its range is a doji.
const DOJI: f64 = 0.05;
/// Above this share of doji-like bars, the opens are not real opens.
const DEGENERATE: f64 = 0.6;

/// Give a continuously-traded series the opens it should have had.
///
/// A market that never closes has no auction open: today's open is yesterday's
/// close, and that is how every charting platform draws it. Yahoo follows that
/// for crypto — BTC's open matches the previous close to a thousandth of a
/// percent — and does not for its FX symbols, where the daily open and close
/// are the same quote taken at the same moment. The result is a chart of
/// nothing but dojis with long wicks: the highs and lows are real, the bodies
/// are an artefact.
///
/// Only applied when the series is actually degenerate, so a provider that
/// gives real opens keeps them, and only the open moves — highs, lows and
/// closes are left exactly as they came. An open outside its own bar's range
/// is clamped into it, because a weekend gap must not produce a candle whose
/// body escapes its wick.
pub fn repair_continuous_opens(bars: &[Bar]) -> Vec<Bar> {
    if !opens_are_degenerate(bars) {
        return bars.to_vec();
    }
    let mut out = bars.to_vec();
    for i in 1..out.len() {
        let previous_close = out[i - 1].close;
        out[i].open = previous_close.clamp(out[i].low, out[i].high);
    }
    out
}

/// Does this series have bodies, or only wicks?
pub fn opens_are_degenerate(bars: &[Bar]) -> bool {
    let measurable: Vec<&Bar> = bars.iter().filter(|b| b.high > b.low).collect();
    // Too few bars to tell; leave them alone.
    if measurable.len() < 20 {
        return false;
    }
    let doji = measurable
        .iter()
        .filter(|b| (b.close - b.open).abs() / (b.high - b.low) < DOJI)
        .count();
    doji as f64 / measurable.len() as f64 > DEGENERATE
}

/// Fold `src` into `target`.
///
/// `origin` shifts the bucket boundaries, in seconds. Futures sessions open at
/// 18:00 ET, so 4h bars bucketed from midnight disagree with every other tool
/// on the planet; passing the session offset here is what makes them line up.
///
/// Weekly folds to ISO weeks (Monday), monthly and yearly to calendar months
/// and years in UTC, and all three ignore `origin`.
pub fn resample(src: &[Bar], target: Timeframe, origin: i64) -> Vec<Bar> {
    if src.is_empty() {
        return Vec::new();
    }
    fold(src, move |ts| bucket_of(*ts, target, origin))
}

/// The bucket a bar at `ts` folds into when the chart shows `target`: the
/// unix second its folded bar opens at.
///
/// Public because a live tail has to find the start of the bucket a changed
/// native bar belongs to, and asking the same function the whole-series fold
/// asks is the only way the two can agree about where a four-hour bar
/// begins.
pub fn bucket_of(ts: i64, target: Timeframe, origin: i64) -> i64 {
    if matches!(target.unit, Unit::Month | Unit::Year) {
        return calendar_start(ts, target);
    }
    if target.unit == Unit::Week {
        let weeks = target.count.max(1) as i64;
        let start = week_start(ts);
        // Multi-week buckets count from the epoch's first Monday so they
        // are stable rather than depending on where the data begins.
        const FIRST_MONDAY: i64 = 4 * 86_400;
        return (start - FIRST_MONDAY).div_euclid(weeks * 604_800) * (weeks * 604_800)
            + FIRST_MONDAY;
    }
    let step = target.seconds();
    (ts - origin).div_euclid(step) * step + origin
}

fn fold(src: &[Bar], bucket_of: impl Fn(&i64) -> i64) -> Vec<Bar> {
    let mut out: Vec<Bar> = Vec::with_capacity(src.len() / 4 + 1);
    for bar in src {
        let bucket = bucket_of(&bar.ts);
        match out.last_mut() {
            Some(last) if last.ts == bucket => {
                last.high = last.high.max(bar.high);
                last.low = last.low.min(bar.low);
                last.close = bar.close;
                last.volume += bar.volume;
            }
            _ => out.push(Bar { ts: bucket, ..*bar }),
        }
    }
    out
}

/// Unix second of the first of the month, 00:00 UTC, that opens the monthly
/// or yearly bucket `ts` falls in.
///
/// Multi-month and multi-year buckets count from 1970 so they are stable
/// rather than depending on where the data begins: 3M is the calendar
/// quarters, 6M the halves.
fn calendar_start(ts: i64, target: Timeframe) -> i64 {
    let Some(at) = Utc.timestamp_opt(ts, 0).single() else {
        return ts;
    };
    let count = i64::from(target.count.max(1));
    let months = if target.unit == Unit::Year { 12 * count } else { count };
    let since_1970 = i64::from(at.year() - 1970) * 12 + i64::from(at.month0());
    let start = since_1970.div_euclid(months) * months;
    let (year, month0) = (1970 + start.div_euclid(12), start.rem_euclid(12));
    i32::try_from(year)
        .ok()
        .and_then(|year| Utc.with_ymd_and_hms(year, month0 as u32 + 1, 1, 0, 0, 0).single())
        .map(|start| start.timestamp())
        .unwrap_or(ts)
}

/// Unix second of the Monday 00:00 UTC that `ts` falls in.
fn week_start(ts: i64) -> i64 {
    // The epoch was a Thursday, so the first Monday is four days in. Shift
    // onto that grid, bucket, shift back.
    const FIRST_MONDAY: i64 = 4 * 86_400;
    (ts - FIRST_MONDAY).div_euclid(604_800) * 604_800 + FIRST_MONDAY
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bar(ts: i64, o: f64, h: f64, l: f64, c: f64, v: f64) -> Bar {
        Bar { ts, open: o, high: h, low: l, close: c, volume: v }
    }

    #[test]
    fn four_hour_folds_from_hourly() {
        let src: Vec<Bar> = (0..8)
            .map(|i| bar(i * 3600, 10.0 + i as f64, 12.0 + i as f64, 9.0 + i as f64, 11.0 + i as f64, 1.0))
            .collect();
        let out = resample(&src, Timeframe::hours(4), 0);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].open, 10.0);
        assert_eq!(out[0].close, 14.0);
        assert_eq!(out[0].high, 15.0);
        assert_eq!(out[0].low, 9.0);
        assert_eq!(out[0].volume, 4.0);
    }

    #[test]
    fn the_session_origin_moves_the_boundaries() {
        // Two hourly bars either side of a boundary that only exists when the
        // session origin is applied.
        let src = vec![bar(0, 1.0, 1.0, 1.0, 1.0, 1.0), bar(3600, 2.0, 2.0, 2.0, 2.0, 1.0)];
        assert_eq!(resample(&src, Timeframe::hours(4), 0).len(), 1);
        assert_eq!(resample(&src, Timeframe::hours(4), 3600).len(), 2);
    }

    #[test]
    fn weekly_buckets_start_on_monday() {
        // 2024-01-04 was a Thursday; its week starts Monday 2024-01-01.
        let thursday = 1_704_326_400;
        let monday = week_start(thursday);
        assert_eq!(monday, 1_704_067_200);
        // Same bucket for every day of that week.
        assert_eq!(week_start(monday + 6 * 86_400), monday);
        assert_eq!(week_start(monday + 7 * 86_400), monday + 604_800);
    }

    #[test]
    fn resolutions_parse_the_way_people_type_them() {
        assert_eq!(Timeframe::parse("3"), Some(Timeframe::minutes(3)));
        assert_eq!(Timeframe::parse("3m"), Some(Timeframe::minutes(3)));
        assert_eq!(Timeframe::parse("15"), Some(Timeframe::minutes(15)));
        assert_eq!(Timeframe::parse("4h"), Some(Timeframe::hours(4)));
        assert_eq!(Timeframe::parse("1D"), Some(Timeframe::days(1)));
        assert_eq!(Timeframe::parse("d"), Some(Timeframe::days(1)));
        assert_eq!(Timeframe::parse("W"), Some(Timeframe::weeks(1)));
        assert_eq!(Timeframe::parse(" 30 min "), Some(Timeframe::minutes(30)));
        assert_eq!(Timeframe::parse("3y"), Some(Timeframe::years(3)));
        assert_eq!(Timeframe::parse("Y"), Some(Timeframe::years(1)));

        assert_eq!(Timeframe::parse(""), None);
        assert_eq!(Timeframe::parse("0"), None);
        assert_eq!(Timeframe::parse("banana"), None);
    }

    /// "M" is the one suffix whose case matters: a capital is a month, as in
    /// every other charting package, and anything lower is a minute.
    #[test]
    fn a_capital_m_is_a_month_and_mo_is_one_in_any_case() {
        assert_eq!(Timeframe::parse("M"), Some(Timeframe::months(1)));
        assert_eq!(Timeframe::parse("3M"), Some(Timeframe::months(3)));
        assert_eq!(Timeframe::parse("3m"), Some(Timeframe::minutes(3)));
        for month in ["1mo", "1MO", "1Mo", "mo", "1 month", "6 Months"] {
            assert_eq!(Timeframe::parse(month).map(|t| t.unit), Some(Unit::Month), "{month}");
        }
        assert_eq!(Timeframe::parse("12M"), Some(Timeframe::years(1)));
        assert_eq!(Timeframe::parse("Min"), Some(Timeframe::minutes(1)));
    }

    #[test]
    fn monthly_and_yearly_buckets_follow_the_calendar() {
        // 2024-02-29 12:00 UTC, a leap day.
        let leap_day = 1_709_208_000;
        let february = 1_706_745_600; // 2024-02-01
        let march = 1_709_251_200; // 2024-03-01
        let january = 1_704_067_200; // 2024-01-01
        assert_eq!(bucket_of(leap_day, Timeframe::months(1), 0), february);
        assert_eq!(bucket_of(march, Timeframe::months(1), 0), march);
        assert_eq!(bucket_of(march - 1, Timeframe::months(1), 0), february);
        // Quarters and years count from January, whatever the origin.
        assert_eq!(bucket_of(leap_day, Timeframe::months(3), 3_600), january);
        assert_eq!(bucket_of(leap_day, Timeframe::years(1), 0), january);
        assert_eq!(bucket_of(leap_day, Timeframe::years(5), 0), 1_577_836_800); // 2020-01-01
        // Before the epoch too.
        assert_eq!(bucket_of(-1, Timeframe::years(1), 0), -31_536_000); // 1969-01-01
    }

    #[test]
    fn daily_bars_fold_into_months() {
        let day = 86_400;
        let january = 1_704_067_200;
        let src: Vec<Bar> = (0..60)
            .map(|i| bar(january + i * day, i as f64, i as f64 + 1.0, i as f64 - 1.0, i as f64, 1.0))
            .collect();
        let out = resample(&src, Timeframe::months(1), 0);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].ts, january);
        assert_eq!(out[0].open, 0.0);
        assert_eq!(out[0].close, 30.0);
        assert_eq!(out[0].volume, 31.0);
        assert_eq!(out[1].volume, 29.0);
    }

    #[test]
    fn equivalent_resolutions_collapse_to_one() {
        // "60" and "1h" must not be two different chart states.
        assert_eq!(Timeframe::parse("60").unwrap(), Timeframe::hours(1));
        assert_eq!(Timeframe::parse("240").unwrap(), Timeframe::hours(4));
        assert_eq!(Timeframe::parse("1440").unwrap(), Timeframe::days(1));
        assert_eq!(Timeframe::parse("24h").unwrap(), Timeframe::days(1));
        assert_eq!(Timeframe::parse("7d").unwrap(), Timeframe::weeks(1));
        // But 90 minutes has no larger exact unit.
        assert_eq!(Timeframe::parse("90").unwrap(), Timeframe::minutes(90));
    }

    #[test]
    fn keys_round_trip_through_parse() {
        for timeframe in Timeframe::PRESETS {
            assert_eq!(Timeframe::parse(&timeframe.key()), Some(timeframe));
        }
        for odd in [
            Timeframe::minutes(3),
            Timeframe::minutes(7),
            Timeframe::days(3),
            Timeframe::months(1),
            Timeframe::months(3),
            Timeframe::years(1),
        ] {
            assert_eq!(Timeframe::parse(&odd.key()), Some(odd));
        }
    }

    #[test]
    fn a_custom_resolution_folds_from_something_the_provider_serves() {
        // 3m divides by 1 only, so it folds from one-minute bars.
        assert_eq!(Timeframe::minutes(3).native(), Timeframe::minutes(1));
        // 10m folds from 5m, which is cheaper than asking for 1m.
        assert_eq!(Timeframe::minutes(10).native(), Timeframe::minutes(5));
        assert_eq!(Timeframe::minutes(45).native(), Timeframe::minutes(15));
        // Served intervals are fetched directly.
        assert_eq!(Timeframe::minutes(5).native(), Timeframe::minutes(5));
        assert_eq!(Timeframe::hours(1).native(), Timeframe::hours(1));
        assert_eq!(Timeframe::days(1).native(), Timeframe::days(1));
        // And everything coarse folds from daily.
        assert_eq!(Timeframe::hours(4).native(), Timeframe::hours(1));
        assert_eq!(Timeframe::weeks(1).native(), Timeframe::days(1));
        assert_eq!(Timeframe::days(3).native(), Timeframe::days(1));
        assert_eq!(Timeframe::months(1).native(), Timeframe::days(1));
        assert_eq!(Timeframe::years(1).native(), Timeframe::days(1));
    }

    #[test]
    fn the_native_resolution_always_divides_the_one_asked_for() {
        for count in 1..=240u32 {
            let timeframe = Timeframe::minutes(count).normalised();
            let native = timeframe.native();
            assert_eq!(
                timeframe.seconds() % native.seconds(),
                0,
                "{} does not fold from {}",
                timeframe.key(),
                native.key()
            );
        }
    }

    #[test]
    fn bar_styles_round_trip() {
        for style in BarStyle::ALL {
            assert_eq!(BarStyle::from_key(style.key()), Some(style));
            assert!(!style.label().is_empty());
        }
        assert_eq!(BarStyle::from_key("sticks"), None);
        assert_eq!(BarStyle::default(), BarStyle::Candles);
    }

    #[test]
    fn resolutions_read_back_in_words() {
        assert_eq!(Timeframe::minutes(1).description(), "1 minute");
        assert_eq!(Timeframe::minutes(3).description(), "3 minutes");
        assert_eq!(Timeframe::hours(1).description(), "1 hour");
        assert_eq!(Timeframe::hours(4).description(), "4 hours");
        assert_eq!(Timeframe::days(1).description(), "1 day");
        assert_eq!(Timeframe::weeks(2).description(), "2 weeks");
        assert_eq!(Timeframe::months(1).description(), "1 month");
        assert_eq!(Timeframe::years(2).description(), "2 years");
    }

    #[test]
    fn what_you_type_reads_back_normalised() {
        // Typing 240 is typing four hours, and should say so.
        assert_eq!(Timeframe::parse("240").unwrap().description(), "4 hours");
        assert_eq!(Timeframe::parse("60").unwrap().description(), "1 hour");
        assert_eq!(Timeframe::parse("3").unwrap().description(), "3 minutes");
        assert_eq!(Timeframe::parse("7d").unwrap().description(), "1 week");
    }

    #[test]
    fn intraday_is_about_the_unit_not_the_length() {
        assert!(Timeframe::minutes(3).is_intraday());
        assert!(Timeframe::hours(12).is_intraday());
        assert!(!Timeframe::days(1).is_intraday());
        assert!(!Timeframe::weeks(1).is_intraday());
        assert!(!Timeframe::months(1).is_intraday());
        assert!(!Timeframe::years(1).is_intraday());
        assert!(Timeframe::months(1).is_calendar());
        assert!(!Timeframe::weeks(1).is_calendar());
    }

    #[test]
    fn currency_majors_are_quoted_to_a_fractional_pip() {
        let fx = Some(InstrumentKind::Fx);
        // EURUSD ruled every 0.005: the step says three, the market says five.
        // This is the case that showed 1.126 for a pair trading at 1.12481.
        assert_eq!(price_decimals(0.005, 1.1248, fx), 5);
        assert_eq!(price_decimals(0.0005, 1.3240, fx), 5);
        // The yen pairs run in the hundreds and are quoted to three.
        assert_eq!(price_decimals(0.5, 157.83, fx), 3);
    }

    #[test]
    fn everything_else_is_quoted_by_its_size() {
        // An index ruled every 50 points: two decimals is already generous.
        assert_eq!(price_decimals(50.0, 7722.72, Some(InstrumentKind::Index)), 2);
        assert_eq!(price_decimals(1.0, 233.95, Some(InstrumentKind::Equity)), 2);
        // A share priced like a currency pair is still a share.
        assert_eq!(price_decimals(0.005, 1.12, Some(InstrumentKind::Equity)), 3);
        assert_eq!(price_decimals(0.0001, 0.00042, Some(InstrumentKind::Crypto)), 6);
        // The teens are not a special case. VIX at 15.31 is 15.31, not
        // 15.3100, and a share at €11.81 does not want four places either.
        assert_eq!(price_decimals(1.0, 15.31, Some(InstrumentKind::Index)), 2);
        assert_eq!(price_decimals(0.5, 11.808, Some(InstrumentKind::Equity)), 2);
    }

    #[test]
    fn a_chart_ruled_finer_than_the_market_still_gets_the_finer() {
        let fx = Some(InstrumentKind::Fx);
        assert_eq!(price_decimals(0.000001, 1.1248, fx), 6);
        assert_eq!(price_decimals(0.001, 7722.72, Some(InstrumentKind::Index)), 3);
    }

    #[test]
    fn a_degenerate_step_or_unknown_instrument_still_gives_an_answer() {
        assert_eq!(price_decimals(0.0, 100.0, None), 2);
        assert_eq!(price_decimals(-1.0, 100.0, None), 2);
        assert_eq!(price_decimals(0.0, 1.5, None), 2);
    }

    #[test]
    fn a_series_of_dojis_gets_its_opens_back() {
        // What Yahoo returns for a currency pair: real highs and lows, and an
        // open that is the close.
        let series: Vec<Bar> = (0..60)
            .map(|i| {
                let close = 1.10 + i as f64 * 0.001;
                Bar {
                    ts: i as i64 * 86_400,
                    open: close,
                    high: close + 0.004,
                    low: close - 0.004,
                    close,
                    volume: 0.0,
                }
            })
            .collect();
        assert!(opens_are_degenerate(&series));

        let fixed = repair_continuous_opens(&series);
        assert_eq!(fixed[0].open, series[0].open, "the first bar has no yesterday");
        for i in 1..fixed.len() {
            assert_eq!(fixed[i].open, series[i - 1].close, "today opens where yesterday left off");
            // Nothing else moved.
            assert_eq!(fixed[i].high, series[i].high);
            assert_eq!(fixed[i].low, series[i].low);
            assert_eq!(fixed[i].close, series[i].close);
        }
    }

    #[test]
    fn a_series_with_real_bodies_is_left_alone() {
        let series: Vec<Bar> = (0..60)
            .map(|i| {
                let open = 100.0 + i as f64;
                Bar {
                    ts: i as i64 * 86_400,
                    open,
                    high: open + 3.0,
                    low: open - 1.0,
                    close: open + 2.0,
                    volume: 1.0,
                }
            })
            .collect();
        assert!(!opens_are_degenerate(&series));
        assert_eq!(repair_continuous_opens(&series), series);
    }

    #[test]
    fn a_repaired_open_never_escapes_its_own_bar() {
        // A weekend gap: yesterday closed well above today's whole range.
        let series = [
            Bar { ts: 0, open: 2.0, high: 2.0, low: 2.0, close: 2.0, volume: 0.0 },
            Bar { ts: 86_400, open: 1.0, high: 1.05, low: 0.95, close: 1.0, volume: 0.0 },
        ];
        let mut long = vec![series[0]];
        long.extend(std::iter::repeat_n(series[1], 40));
        let fixed = repair_continuous_opens(&long);
        for bar in &fixed {
            assert!(bar.open >= bar.low && bar.open <= bar.high, "{bar:?}");
        }
    }

    #[test]
    fn too_short_a_series_is_not_judged() {
        let series: Vec<Bar> = (0..5)
            .map(|i| Bar { ts: i, open: 1.0, high: 1.1, low: 0.9, close: 1.0, volume: 0.0 })
            .collect();
        assert!(!opens_are_degenerate(&series));
    }

    #[test]
    fn empty_in_empty_out() {
        assert!(resample(&[], Timeframe::hours(4), 0).is_empty());
    }
}
