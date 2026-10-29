//! Bars, timeframes, and turning one timeframe into another.

use std::cell::Cell;

use chrono::{Datelike, TimeZone, Utc};
use serde::{Deserialize, Serialize};

use crate::exchanges::{registry, Schedule};
use crate::session::Session;
use crate::symbols::{Instrument, InstrumentKind};

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
    /// rather not lean on the shift key. "Q" is a quarter, three months.
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
            // A quarter is three months, so 1Q is 3M and keeps that key.
            "q" | "qtr" | "quarter" | "quarters" => {
                return Some(Timeframe { count: count.checked_mul(3)?, unit: Unit::Month }.normalised());
            }
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

    /// The moment `bars` bars on from one opening at `ts`, or back for a
    /// negative count. Months and years step by the calendar: an average
    /// month on from the first lands on the last day of the month before
    /// as often as not.
    pub fn shift(self, ts: i64, bars: i64) -> i64 {
        if self.is_calendar() {
            let months = self.calendar_months();
            if let Some(start) = month_of(ts).and_then(|month| month_start(month + bars * months)) {
                return start;
            }
        }
        ts + bars * self.seconds()
    }

    /// How many calendar months one bar spans, for a monthly or yearly chart.
    fn calendar_months(self) -> i64 {
        let count = i64::from(self.count.max(1));
        if self.unit == Unit::Year { 12 * count } else { count }
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

/// A value to so many decimals with its thousands marked, the way a
/// spreadsheet's `#,##0.00` shows it: `2,210.40`, `-1,234.5`. Four digits
/// run together are a misread waiting to happen in a column of prices, and
/// on an axis. One string, for the labels a frame writes by the dozen.
///
/// For what is shown, never for what is stored or printed for a script:
/// nothing parses a number back with the commas in it.
pub fn grouped_decimals(value: f64, decimals: usize) -> String {
    use std::fmt::Write as _;
    let mut out = String::with_capacity(24 + decimals);
    let _ = write!(out, "{value:.decimals$}");
    // Commas into the first run of digits: after the sign, before the point.
    let Some(from) = out.find(|c: char| c.is_ascii_digit()) else { return out };
    let to = out[from..].find(|c: char| !c.is_ascii_digit()).map_or(out.len(), |at| from + at);
    let mut at = to;
    while at > from + 3 {
        at -= 3;
        out.insert(at, ',');
    }
    out
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

/// How far outside its neighbours a print that never traded may sit and still
/// be believed. Thin futures settle 1–3% outside the days around them with
/// no volume, and those are real; the phantoms seen were 7–15% off.
const PHANTOM_GAP: f64 = 0.05;

/// And how many steps of the grain its prices are quoted in. On a penny
/// stock 5% is a tick or two, which is an ordinary day's move, not a print
/// nobody made.
const PHANTOM_TICKS: f64 = 3.0;

/// Drop prints that never traded and sit well outside the market around them.
///
/// Yahoo now and then serves a bar with no volume and one price for all four,
/// far from the days either side: EXUS.DE on 2025-10-24 is 39.63 flat between
/// two days that traded around 34.1. Drawn, it is a spike on the daily chart
/// and the high and the close of the week folded from it.
///
/// Applied as a series is read, never as it is stored: the cache keeps what
/// the provider said, so a bar is judged once both its neighbours are there,
/// and a change to the rule reaches every series already cached.
///
/// A flat bar with no volume is also what a quiet day looks like, every day
/// of a fund priced once at its NAV, a thin future's settlement, and the one
/// print a thin listing gets on a crash day. Those stay: a quiet day carries
/// the last close, a fund's neighbours have no volume either, a settlement
/// lands within a few percent of the days around it, and a crash is still
/// down the next day. Only a new price more than [`PHANTOM_GAP`] and
/// [`PHANTOM_TICKS`] outside both traded neighbours goes, and only when the
/// next day is back: nearer the day before than half the way to the print.
///
/// What is judged follows from that and nothing else: any provider and any
/// resolution, but only a bar between two that traded. A series that never
/// reports volume — an index, a currency pair, a published series —
/// is never judged. The last bar is not either, having nothing after it, and
/// two such bars in a row are both kept, since neither has traded neighbours,
/// so dropping one never changes whether another is a phantom.
pub fn drop_phantoms(bars: &mut Vec<Bar>) {
    let phantoms: Vec<usize> = bars
        .windows(3)
        .enumerate()
        .filter(|(_, w)| is_phantom(&w[0], &w[1], &w[2]))
        .map(|(i, _)| i + 1)
        .collect();
    for i in phantoms.into_iter().rev() {
        bars.remove(i);
    }
}

/// The grain `bars` are quoted in: the coarsest power of ten every price of
/// theirs is a whole number of, to within the rounding a price stored as a
/// 32-bit float carries. 0.01 for a penny stock quoted in cents; much finer
/// for a split-adjusted price, which is a quotient and has no grain at all.
fn grain(bars: [&Bar; 3]) -> f64 {
    let prices = || bars.into_iter().flat_map(|b| [b.open, b.high, b.low, b.close]);
    let Some(lowest) = prices().filter(|p| *p > 0.0).min_by(f64::total_cmp) else {
        return 0.0;
    };
    let top = lowest.log10().floor() as i32;
    (top - 8..=top)
        .rev()
        .map(|power| 10f64.powi(power))
        .find(|step| prices().all(|p| ((p / step) - (p / step).round()).abs() < 1e-3))
        .unwrap_or(0.0)
}

fn is_phantom(prev: &Bar, bar: &Bar, next: &Bar) -> bool {
    let flat = bar.open == bar.close && bar.high == bar.close && bar.low == bar.close;
    let traded = prev.volume > 0.0 && next.volume > 0.0;
    let carried = bar.close == prev.close;
    if bar.volume != 0.0 || !flat || !traded || carried || bar.close <= 0.0 {
        return false;
    }
    let (high, low) = (prev.high.max(next.high), prev.low.min(next.low));
    let (gap, edge) = match bar.close {
        c if c > high => (c - high, high),
        c if c < low => (low - c, low),
        _ => return false,
    };
    let far = gap > PHANTOM_GAP * edge && (gap / grain([prev, bar, next])).round() >= PHANTOM_TICKS;
    let back = (next.close - prev.close).abs() < 0.5 * (bar.close - prev.close).abs();
    far && back
}

/// How far past its own body and both neighbours a wick on a bar that never
/// traded may reach and still be believed, in widths of the widest range
/// around it. The strays seen reach 33–59 of them. Nothing real comes near:
/// the most in 36 years of ^VIX, ^IRX and ^TNX is under 5, and ^VIX on
/// 2024-08-05, from 23 to 65.73 and back to 38.57, is under 2.
const STRAY_WICK: f64 = 10.0;

/// How many bars either side that range is taken from. One is too few: a
/// yield pinned near zero has days with no range at all, and against them a
/// move of a basis point is a hundred widths.
const STRAY_WICK_AROUND: usize = 5;

/// Cut back wicks that reach far past everything around them on a bar that
/// never traded.
///
/// Yahoo served EURUSD and GBPUSD on 2012-01-27 with a low of 0.76 and 0.64,
/// a day that otherwise traded around 1.31 and 1.57: the other side of the
/// pair, put where the low goes. Drawn, the whole chart is squeezed to fit a
/// spike nobody could have traded, and the week folded from it carries it.
///
/// Never stored: the cache keeps what the provider said. The bar stays,
/// since its open and close are good; only the wick goes, back to the body,
/// rather than to a price nobody printed. A wick is cut only on a bar with no
/// volume — a currency pair, an index — when it reaches past the bar's body
/// and both neighbours' same extremes by more than [`STRAY_WICK`] times the
/// widest of: the ranges of the [`STRAY_WICK_AROUND`] bars either side, its
/// own body, and a hundredth of its price. What is measured is how far the
/// wick is from anything the market did that week, not how far it is from
/// the price, which near zero is no distance at all. A bar whose body is at
/// or under zero is not judged, nor are the first and the last.
pub fn clip_stray_wicks(bars: &mut [Bar]) {
    let clipped: Vec<(usize, f64, f64)> = (1..bars.len().saturating_sub(1))
        .filter_map(|i| clip(bars, i).map(|(high, low)| (i, high, low)))
        .collect();
    for (i, high, low) in clipped {
        if let Some(bar) = bars.get_mut(i) {
            (bar.high, bar.low) = (high, low);
        }
    }
}

/// The high and low `bars[i]` should be drawn with, when either is a stray.
fn clip(bars: &[Bar], i: usize) -> Option<(f64, f64)> {
    let (prev, bar, next) = (bars.get(i.checked_sub(1)?)?, bars.get(i)?, bars.get(i + 1)?);
    let (top, bottom) = (bar.open.max(bar.close), bar.open.min(bar.close));
    if bar.volume != 0.0 || bottom <= 0.0 {
        return None;
    }
    let above = bar.high - prev.high.max(next.high).max(top);
    let below = prev.low.min(next.low).min(bottom) - bar.low;
    // Nearly every bar of a pair or an index has no volume, so the ranges
    // around it are only read for a wick already past the least room it
    // could be given.
    let least = STRAY_WICK * (top - bottom).max(top / 100.0);
    if above.max(below) <= least {
        return None;
    }
    let around = i.saturating_sub(STRAY_WICK_AROUND)..(i + 1 + STRAY_WICK_AROUND).min(bars.len());
    let widest = around
        .filter(|&j| j != i)
        .filter_map(|j| bars.get(j))
        .map(|b| b.high - b.low)
        .fold(0.0, f64::max);
    let room = STRAY_WICK * widest.max(top - bottom).max(top / 100.0);
    let high = if above > room { top } else { bar.high };
    let low = if below > room { bottom } else { bar.low };
    (high != bar.high || low != bar.low).then_some((high, low))
}

/// A stored series as a chart at `timeframe` draws it: the
/// prints that never traded left out ([`drop_phantoms`]) and, on bars of a
/// day or longer, the wicks nobody could have traded cut back
/// ([`clip_stray_wicks`]).
///
/// On the way to the screen only, never on the way to the cache. A series in
/// memory is written back as it stands — a live one every few seconds — so a
/// cut wick that reached memory would replace the one the provider sent.
///
/// The same for every provider: both rules read nothing but the bars. Wicks
/// are not judged within the day, where an index really does spike and come
/// back inside an hour.
///
/// A future's prints are all kept: one with no volume is its settlement,
/// which the exchange sets every day whether or not anybody traded, and on a
/// fast day it can sit far from both neighbours. Palladium on 2026-01-27
/// settled 14% down with no trades, 8% under the next day, while platinum did
/// the same 3.6% under its own.
pub fn drawable(bars: &mut Vec<Bar>, timeframe: Timeframe, kind: InstrumentKind) {
    if !kind.settles_untraded() {
        drop_phantoms(bars);
    }
    if !timeframe.is_intraday() {
        clip_stray_wicks(bars);
    }
}

/// [`drawable`] of `native` from `start` on, as the whole series would draw
/// it: what a live tick refolds into the chart's last bucket, which on a
/// weekly, monthly or yearly chart reaches back over days that have already
/// been judged. Each bar is judged with the bars before it in view, which is
/// all either rule reads behind it.
pub fn drawable_from(native: &[Bar], start: usize, timeframe: Timeframe, kind: InstrumentKind) -> Vec<Bar> {
    let Some(first) = native.get(start).map(|bar| bar.ts) else {
        return Vec::new();
    };
    let mut bars = native[start.saturating_sub(2 * (STRAY_WICK_AROUND + 1))..].to_vec();
    drawable(&mut bars, timeframe, kind);
    bars.retain(|bar| bar.ts >= first);
    bars
}

/// The lines a chart's bars are cut along when they are folded to a coarser
/// resolution within the day: where a 4h or a 45m bar begins.
///
/// A market whose hours are known counts each trading day afresh, from where
/// its bars start on the exchange's own clock: the regular open on a chart of
/// regular hours, so SPY's 4h bars are 09:30 and 13:30 in New York, and the
/// start of the trading day otherwise, 04:00 there and 17:00 in Chicago for
/// Globex. That is where every other charting tool puts them, and it follows
/// the exchange through daylight saving, which no fixed offset from UTC can.
/// A day's last bar runs short when the session ends inside it.
///
/// Anything else, a market that never shuts or a venue nothing here knows,
/// counts from midnight UTC, moved by the instrument's `session_origin`.
#[derive(Clone, Debug)]
pub struct Grid {
    origin: i64,
    hours: Option<(Schedule<'static>, bool)>,
    /// The last trading day asked about: where it starts, where the next one
    /// does, and where its bars start. Bars are folded in order, so a bar is
    /// usually a comparison against this and the time zone is only read when
    /// the day changes; read for every bar, it made a fold six times slower.
    day: Cell<Option<(i64, i64, i64)>>,
}

impl Grid {
    /// The grid `instrument`'s bars fold along on a chart keeping `session`.
    pub fn new(instrument: &Instrument, session: Session) -> Grid {
        // FX and crypto trade all day: midnight is as good a line as any,
        // and the one they have always been cut along.
        let hours = match instrument.kind {
            InstrumentKind::Fx | InstrumentKind::Crypto => None,
            _ => registry().for_instrument(instrument),
        };
        Grid {
            origin: instrument.session_origin,
            hours: hours.map(|schedule| (schedule, session == Session::Regular && schedule.has_regular())),
            day: Cell::new(None),
        }
    }

    /// Midnight UTC moved by `origin` seconds, every day alike.
    #[cfg(test)]
    pub fn fixed(origin: i64) -> Grid {
        Grid { origin, hours: None, day: Cell::new(None) }
    }

    /// Where the steps of the day `ts` falls in count from.
    ///
    /// A bar before its day's bars start, which a filtered chart should never
    /// hold, is counted from the start of the trading day instead: it keeps a
    /// step of its own ahead of the day's first, and can never land behind
    /// the day before.
    fn counts_from(&self, ts: i64) -> i64 {
        let Some((schedule, regular)) = self.hours else { return self.origin };
        let (start, _, bars) = match self.day.get() {
            Some(day @ (start, end, _)) if (start..end).contains(&ts) => day,
            _ => {
                let Some(date) = schedule.trading_date(ts) else { return self.origin };
                let end = date.succ_opt().map_or(i64::MAX, |next| schedule.day_start(next));
                let day = (schedule.day_start(date), end, schedule.bars_start(date, regular));
                self.day.set(Some(day));
                day
            }
        };
        if ts >= bars { bars } else { start }
    }
}

/// Fold `src` into `target`, cutting intraday bars along `grid`.
///
/// Weekly folds to ISO weeks (Monday), monthly and yearly to calendar months
/// and years in UTC, and all three ignore the grid. Days count from the
/// grid's fixed origin.
pub fn resample(src: &[Bar], target: Timeframe, grid: &Grid) -> Vec<Bar> {
    if src.is_empty() {
        return Vec::new();
    }
    fold(src, move |ts| bucket_of(*ts, target, grid))
}

/// The bucket a bar at `ts` folds into when the chart shows `target`: the
/// unix second its folded bar opens at.
///
/// Public because a live tail has to find the start of the bucket a changed
/// native bar belongs to, and asking the same function the whole-series fold
/// asks is the only way the two can agree about where a four-hour bar
/// begins.
pub fn bucket_of(ts: i64, target: Timeframe, grid: &Grid) -> i64 {
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
    let origin = match target.is_intraday() {
        true => grid.counts_from(ts),
        false => grid.origin,
    };
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
    let months = target.calendar_months();
    month_of(ts)
        .and_then(|month| month_start(month.div_euclid(months) * months))
        .unwrap_or(ts)
}

/// Months since January 1970 at `ts`, in UTC.
fn month_of(ts: i64) -> Option<i64> {
    let at = Utc.timestamp_opt(ts, 0).single()?;
    Some(i64::from(at.year() - 1970) * 12 + i64::from(at.month0()))
}

/// Unix second of 00:00 UTC on the first of the month `month` months after
/// January 1970.
fn month_start(month: i64) -> Option<i64> {
    let year = i32::try_from(1970 + month.div_euclid(12)).ok()?;
    let month0 = month.rem_euclid(12) as u32;
    Some(Utc.with_ymd_and_hms(year, month0 + 1, 1, 0, 0, 0).single()?.timestamp())
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

    #[test]
    fn a_crash_print_and_a_tick_or_two_on_a_penny_are_kept() {
        let at = |ts, o, h, l, c, v| Bar { ts, open: o, high: h, low: l, close: c, volume: v };
        let kept = |mut bars: Vec<Bar>| {
            let n = bars.len();
            drop_phantoms(&mut bars);
            bars.len() == n
        };
        // 4BRZ.SW on 2020-03-12, as Yahoo serves it: the day's one print was
        // a quarter down, and the next day was still most of the way there.
        assert!(kept(vec![
            at(1, 29.3, 29.3, 29.3, 29.3, 65.0),
            at(2, 22.135, 22.135, 22.135, 22.135, 0.0),
            at(3, 24.89, 24.89, 24.89, 24.89, 58.0),
        ]));
        // HME.V on 2015-08-24: 0.14 under a low of 0.15 is 7%, and one tick.
        assert!(kept(vec![
            at(1, 0.17, 0.17, 0.15, 0.17, 171_700.0),
            at(2, 0.14, 0.14, 0.14, 0.14, 0.0),
            at(3, 0.16, 0.16, 0.16, 0.16, 149_500.0),
        ]));

        // 1127.HK on 2021-07-19: five ticks over two days that traded at
        // 0.85, and back the next. Nobody paid 0.91.
        let mut bars = vec![
            at(1, 0.85, 0.86, 0.85, 0.86, 36_000.0),
            at(2, 0.91, 0.91, 0.91, 0.91, 0.0),
            at(3, 0.85, 0.85, 0.85, 0.85, 16_000.0),
        ];
        drop_phantoms(&mut bars);
        assert_eq!(bars.len(), 2);
    }

    #[test]
    fn a_flat_print_that_never_traded_is_dropped() {
        let bar = |ts, low: f64, high: f64, volume| Bar { ts, open: low, high, low, close: high, volume };
        // EXUS.DE around 2025-10-24, as Yahoo serves it.
        let mut bars = vec![
            bar(1, 34.035, 34.155, 79_416.0),
            bar(2, 39.625, 39.625, 0.0),
            bar(3, 34.29, 34.385, 124_400.0),
        ];
        drop_phantoms(&mut bars);
        assert_eq!(bars.iter().map(|b| b.ts).collect::<Vec<_>>(), [1, 3]);
    }

    #[test]
    fn quiet_days_funds_settlements_and_the_last_bar_are_kept() {
        let bar = |ts, price, volume| Bar { ts, open: price, high: price + 1.0, low: price - 1.0, close: price, volume };
        let flat = |ts, price| Bar { ts, open: price, high: price, low: price, close: price, volume: 0.0 };
        let kept = |mut bars: Vec<Bar>| {
            let n = bars.len();
            drop_phantoms(&mut bars);
            bars.len() == n
        };

        // Nothing traded, so the last price is carried: inside the range.
        assert!(kept(vec![bar(1, 100.0, 10.0), flat(2, 100.0), bar(3, 100.5, 10.0)]));
        // A fund priced once a day at its NAV: no neighbour traded either.
        assert!(kept(vec![flat(1, 100.0), flat(2, 103.0), flat(3, 100.0)]));
        // A thin future settling a little outside the days around it.
        assert!(kept(vec![bar(1, 100.0, 10.0), flat(2, 103.0), bar(3, 100.0, 10.0)]));
        // The last close carried, however far the next day gaps from it.
        assert!(kept(vec![bar(1, 100.0, 10.0), flat(2, 100.0), bar(3, 80.0, 10.0)]));
        // The newest bar has nothing after it to disagree with.
        assert!(kept(vec![bar(1, 100.0, 10.0), bar(2, 100.0, 10.0), flat(3, 120.0)]));
    }

    #[test]
    fn a_wick_far_past_everything_on_a_bar_nobody_traded_is_cut_to_the_body() {
        let fx = |ts, o, h, l, c| Bar { ts, open: o, high: h, low: l, close: c, volume: 0.0 };
        // EURUSD around 2012-01-27, as Yahoo serves it.
        let mut bars = vec![
            fx(1, 1.3107, 1.3180, 1.3095, 1.3107),
            fx(2, 1.3089, 1.3160, 0.7606, 1.3093),
            fx(3, 1.3207, 1.3208, 1.3084, 1.3208),
        ];
        clip_stray_wicks(&mut bars);
        assert_eq!((bars[1].high, bars[1].low), (1.3160, 1.3089), "the low goes back to the body");
        assert_eq!(bars[1].close, 1.3093, "the rest of the bar is believed");

        // The same, turned over: the stray is the high.
        let mut turned = vec![fx(1, 0.763, 0.764, 0.759, 0.763), fx(2, 0.764, 1.3148, 0.760, 0.764), fx(3, 0.757, 0.764, 0.757, 0.757)];
        clip_stray_wicks(&mut turned);
        assert_eq!(turned[1].high, 0.764);
    }

    #[test]
    fn crash_days_traded_wicks_and_the_ends_are_kept() {
        let at = |ts, o, h, l, c, v| Bar { ts, open: o, high: h, low: l, close: c, volume: v };
        let kept = |mut bars: Vec<Bar>| {
            let before = bars.clone();
            clip_stray_wicks(&mut bars);
            bars == before
        };
        // ^TNX on 2020-03-09: a real low 18% under the body.
        assert!(kept(vec![at(1, 0.70, 0.76, 0.66, 0.74, 0.0), at(2, 0.49, 0.58, 0.398, 0.54, 0.0), at(3, 0.56, 0.78, 0.55, 0.76, 0.0)]));
        // GME on 2021-01-28: a low half the body, but it traded.
        assert!(kept(vec![at(1, 249.0, 380.0, 249.0, 347.0, 9e7), at(2, 265.0, 483.0, 112.0, 193.0, 5e7), at(3, 414.0, 414.0, 250.0, 325.0, 5e7)]));
        // The first and the newest bar have no neighbour on one side.
        assert!(kept(vec![at(1, 1.3, 1.3, 0.7, 1.3, 0.0), at(2, 1.3, 1.31, 1.29, 1.3, 0.0), at(3, 1.3, 2.0, 1.29, 1.3, 0.0)]));
    }

    #[test]
    fn a_panic_in_an_index_and_a_basis_point_off_zero_are_kept() {
        let index = |rows: &[(f64, f64, f64, f64)]| -> Vec<Bar> {
            let at = |(i, &(open, high, low, close)): (usize, &(f64, f64, f64, f64))| {
                Bar { ts: i as i64, open, high, low, close, volume: 0.0 }
            };
            rows.iter().enumerate().map(at).collect()
        };
        let kept = |mut bars: Vec<Bar>| {
            let before = bars.clone();
            clip_stray_wicks(&mut bars);
            bars == before
        };
        // ^VIX on 2024-08-05, as Yahoo serves it: a high 70% over the body
        // and twice the day before's, but reaching past the days around it
        // by under three of their ranges.
        assert!(kept(index(&[(20.52, 29.66, 20.01, 23.39), (23.39, 65.73, 23.39, 38.57), (33.71, 34.77, 24.02, 27.71)])));
        // ^IRX around 2011-11-02: a high of 0.02 over days pinned at 0.005
        // is four times the price, and one basis point and a half.
        let mut irx = vec![(0.01, 0.015, 0.01, 0.015), (0.015, 0.015, 0.01, 0.01), (0.01, 0.01, 0.005, 0.01)];
        irx.extend([(0.005, 0.005, 0.005, 0.005); 2]);
        irx.push((0.005, 0.02, 0.005, 0.005));
        irx.extend([(0.005, 0.005, 0.005, 0.005); 3]);
        irx.push((0.005, 0.01, 0.005, 0.005));
        assert!(kept(index(&irx)));
    }

    #[test]
    fn a_bar_within_the_day_keeps_its_wick() {
        let fx = |ts, o, h, l, c| Bar { ts, open: o, high: h, low: l, close: c, volume: 0.0 };
        let served = vec![
            fx(1, 1.3107, 1.3180, 1.3095, 1.3107),
            fx(2, 1.3089, 1.3160, 0.7606, 1.3093),
            fx(3, 1.3207, 1.3208, 1.3084, 1.3208),
        ];
        let drawn = |timeframe: Timeframe| {
            let mut bars = served.clone();
            drawable(&mut bars, timeframe, InstrumentKind::Equity);
            bars[1].low
        };
        assert_eq!(drawn(Timeframe::days(1)), 1.3089, "a day");
        assert_eq!(drawn(Timeframe::weeks(1)), 1.3089, "folded to weeks");
        assert_eq!(drawn(Timeframe::hours(1)), 0.7606, "within the day");
    }

    /// Palladium's settlement on 2026-01-27: no trades, one price, 8% under
    /// the next day. A stock's print like that goes; a future's stays.
    #[test]
    fn a_futures_settlement_with_no_trades_is_kept() {
        let day = |ts, price: f64, volume| Bar { ts, open: price, high: price, low: price, close: price, volume };
        let served = vec![day(1, 2169.9, 24.0), day(2, 1861.8, 0.0), day(3, 2025.4, 34.0)];
        let drawn = |kind| {
            let mut bars = served.clone();
            drawable(&mut bars, Timeframe::days(1), kind);
            bars.len()
        };
        assert_eq!(drawn(InstrumentKind::FutureRoot), 3, "the settlement is a price");
        assert_eq!(drawn(InstrumentKind::Equity), 2, "a stock's lone print is not");
    }

    #[test]
    fn a_tail_is_drawn_as_the_whole_series_draws_it() {
        let mut native: Vec<Bar> = (0..40)
            .map(|i| {
                let price = 34.0 + (i % 7) as f64 * 0.1;
                Bar { ts: i, open: price, high: price + 0.1, low: price - 0.1, close: price, volume: 1000.0 }
            })
            .collect();
        native[20] = Bar { ts: 20, open: 39.625, high: 39.625, low: 39.625, close: 39.625, volume: 0.0 };
        native[30] = Bar { ts: 30, open: 34.2, high: 34.3, low: 20.0, close: 34.2, volume: 0.0 };
        let mut whole = native.clone();
        drawable(&mut whole, Timeframe::days(1), InstrumentKind::Etf);
        assert_eq!(whole.len(), 39, "the print is left out");
        assert_eq!(whole.iter().find(|b| b.ts == 30).map(|b| b.low), Some(34.2), "and the wick cut");

        for start in 0..=native.len() {
            let from = native.get(start).map_or(i64::MAX, |b| b.ts);
            let expected: Vec<Bar> = whole.iter().copied().filter(|b| b.ts >= from).collect();
            let tail = drawable_from(&native, start, Timeframe::days(1), InstrumentKind::Etf);
            assert_eq!(tail, expected, "from {start}");
        }
    }

    #[test]
    fn prices_mark_their_thousands() {
        assert_eq!(grouped_decimals(2210.4, 2), "2,210.40");
        assert_eq!(grouped_decimals(112345.67, 2), "112,345.67");
        assert_eq!(grouped_decimals(-1234.5, 1), "-1,234.5");
        assert_eq!(grouped_decimals(999.99, 2), "999.99");
        assert_eq!(grouped_decimals(0.00012, 5), "0.00012");
        assert_eq!(grouped_decimals(123456.0, 0), "123,456");
        assert_eq!(grouped_decimals(87435.4, 2), "87,435.40");
        assert_eq!(grouped_decimals(-1234567.0, 0), "-1,234,567");
        assert_eq!(grouped_decimals(f64::NAN, 2), "NaN");
    }

    fn bar(ts: i64, o: f64, h: f64, l: f64, c: f64, v: f64) -> Bar {
        Bar { ts, open: o, high: h, low: l, close: c, volume: v }
    }

    #[test]
    fn four_hour_folds_from_hourly() {
        let src: Vec<Bar> = (0..8)
            .map(|i| bar(i * 3600, 10.0 + i as f64, 12.0 + i as f64, 9.0 + i as f64, 11.0 + i as f64, 1.0))
            .collect();
        let out = resample(&src, Timeframe::hours(4), &Grid::fixed(0));
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
        assert_eq!(resample(&src, Timeframe::hours(4), &Grid::fixed(0)).len(), 1);
        assert_eq!(resample(&src, Timeframe::hours(4), &Grid::fixed(3600)).len(), 2);
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
        for (quarter, months) in [("Q", 3), ("1Q", 3), ("1q", 3), ("2Q", 6), ("4q", 12), ("1 quarter", 3)] {
            assert_eq!(Timeframe::parse(quarter), Some(Timeframe::months(months).normalised()), "{quarter}");
        }
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
        assert_eq!(bucket_of(leap_day, Timeframe::months(1), &Grid::fixed(0)), february);
        assert_eq!(bucket_of(march, Timeframe::months(1), &Grid::fixed(0)), march);
        assert_eq!(bucket_of(march - 1, Timeframe::months(1), &Grid::fixed(0)), february);
        // Quarters and years count from January, whatever the origin.
        assert_eq!(bucket_of(leap_day, Timeframe::months(3), &Grid::fixed(3_600)), january);
        assert_eq!(bucket_of(leap_day, Timeframe::years(1), &Grid::fixed(0)), january);
        assert_eq!(bucket_of(leap_day, Timeframe::years(5), &Grid::fixed(0)), 1_577_836_800); // 2020-01-01
        // Before the epoch too.
        assert_eq!(bucket_of(-1, Timeframe::years(1), &Grid::fixed(0)), -31_536_000); // 1969-01-01
    }

    /// Past the last bar, the columns of a monthly chart are the firsts of
    /// the months after it, not an average month on — which from 1 October
    /// is 31 October.
    #[test]
    fn calendar_bars_step_by_the_calendar() {
        let october = 1_727_740_800; // 2024-10-01
        assert_eq!(Timeframe::months(1).shift(october, 1), 1_730_419_200); // 2024-11-01
        assert_eq!(Timeframe::months(1).shift(october, -10), 1_701_388_800); // 2023-12-01
        assert_eq!(Timeframe::months(3).shift(october, 1), 1_735_689_600); // 2025-01-01
        assert_eq!(Timeframe::years(1).shift(1_704_067_200, 2), 1_767_225_600); // 2026-01-01
        assert_eq!(Timeframe::days(1).shift(october, 3), october + 3 * 86_400);
    }

    #[test]
    fn daily_bars_fold_into_months() {
        let day = 86_400;
        let january = 1_704_067_200;
        let src: Vec<Bar> = (0..60)
            .map(|i| bar(january + i * day, i as f64, i as f64 + 1.0, i as f64 - 1.0, i as f64, 1.0))
            .collect();
        let out = resample(&src, Timeframe::months(1), &Grid::fixed(0));
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
        assert!(resample(&[], Timeframe::hours(4), &Grid::fixed(0)).is_empty());
    }

    /// A listing of `kind`, from JSON, so a field added to an instrument
    /// needs nothing here.
    fn listing(kind: &str, symbol: &str, suffix: Option<&str>, session_origin: i64) -> Instrument {
        serde_json::from_value(serde_json::json!({
            "symbol": symbol, "name": symbol, "kind": kind, "suffix": suffix, "tier": 0,
            "session_origin": session_origin, "overrides": [], "popularity": 0, "formula": null,
        }))
        .expect("an instrument")
    }

    fn spy() -> Instrument {
        listing("etf", "SPY", None, 0)
    }

    fn es() -> Instrument {
        // The fixed origin it carried before it had hours: 18:00 New York in
        // summer, which a fold that still read it would show in winter.
        listing("future_root", "ES", None, 79_200)
    }

    fn at(zone: chrono_tz::Tz, (y, m, d): (i32, u32, u32), (h, min): (u32, u32)) -> i64 {
        zone.with_ymd_and_hms(y, m, d, h, min, 0).single().expect("a moment").timestamp()
    }

    /// Where each bar of `bars` opens, as a wall clock in `zone`.
    fn clocks(bars: &[Bar], zone: chrono_tz::Tz) -> Vec<String> {
        bars.iter()
            .map(|bar| zone.timestamp_opt(bar.ts, 0).single().expect("a moment").format("%m-%d %H:%M").to_string())
            .collect()
    }

    fn flat(ts: i64) -> Bar {
        bar(ts, 1.0, 1.0, 1.0, 1.0, 1.0)
    }

    /// Yahoo's hourly bars for a US listing on regular hours: 09:30 to 15:30.
    fn regular_hours(days: &[(i32, u32, u32)]) -> Vec<Bar> {
        let ny = chrono_tz::America::New_York;
        days.iter().flat_map(|day| (9..=15).map(move |h| flat(at(ny, *day, (h, 30))))).collect()
    }

    /// SPY's 4h bars on regular hours open at 09:30 and 13:30 in New York,
    /// summer and winter alike, and the day's second one runs short: three
    /// hours to the close.
    #[test]
    fn regular_hours_fold_from_the_open() {
        let ny = chrono_tz::America::New_York;
        let grid = Grid::new(&spy(), Session::Regular);
        for (summer, winter) in [((2024, 7, 15), (2024, 1, 16)), ((2024, 7, 16), (2024, 1, 17))] {
            for day in [summer, winter] {
                let out = resample(&regular_hours(&[day]), Timeframe::hours(4), &grid);
                assert_eq!(out.iter().map(|b| b.ts).collect::<Vec<_>>(), [at(ny, day, (9, 30)), at(ny, day, (13, 30))]);
                assert_eq!(out.iter().map(|b| b.volume).collect::<Vec<_>>(), [4.0, 3.0]);
            }
        }
        // A week in one fold: each day starts again at its own open.
        let week: Vec<(i32, u32, u32)> = (15..=19).map(|d| (2024, 7, d)).collect();
        let out = resample(&regular_hours(&week), Timeframe::hours(4), &grid);
        assert_eq!(out.len(), 10);
        assert!(clocks(&out, ny).iter().all(|c| c.ends_with("09:30") || c.ends_with("13:30")), "{:?}", clocks(&out, ny));
    }

    /// On extended hours the day counts from the start of the pre-market:
    /// 04:00, 08:00, 12:00 and 16:00, on both sides of daylight saving.
    #[test]
    fn extended_hours_fold_from_the_pre_market() {
        let ny = chrono_tz::America::New_York;
        let grid = Grid::new(&spy(), Session::Extended);
        for day in [(2024, 7, 15), (2024, 1, 16)] {
            let src: Vec<Bar> = (8..40).map(|half| flat(at(ny, day, (half / 2, half % 2 * 30)))).collect();
            let out = resample(&src, Timeframe::hours(4), &grid);
            let hours: Vec<i64> = [4, 8, 12, 16].map(|h| at(ny, day, (h, 0))).to_vec();
            assert_eq!(out.iter().map(|b| b.ts).collect::<Vec<_>>(), hours);
        }
    }

    /// Globex's day starts at 17:00 in Chicago, and so does its first 4h
    /// bar, either side of the change to summer time on 2024-03-10. The
    /// fixed 22:00 UTC the inventory carried for it was 16:00 in Chicago in
    /// winter.
    #[test]
    fn futures_fold_from_the_globex_day_across_daylight_saving() {
        let chicago = chrono_tz::America::Chicago;
        let schedule = registry().for_instrument(&es()).expect("CME hours");
        let first = at(chicago, (2024, 3, 6), (17, 0));
        let src: Vec<Bar> = (0..24 * 7)
            .map(|h| first + h * 3_600)
            .filter(|ts| schedule.is_open(*ts))
            .map(flat)
            .collect();
        let out = resample(&src, Timeframe::hours(4), &Grid::new(&es(), Session::Extended));
        let clocks = clocks(&out, chicago);
        assert!(
            clocks.iter().all(|c| ["17:00", "21:00", "01:00", "05:00", "09:00", "13:00"].iter().any(|h| c.ends_with(h))),
            "{clocks:?}"
        );
        assert!(clocks.contains(&"03-06 17:00".to_string()), "winter: {clocks:?}");
        assert!(clocks.contains(&"03-10 17:00".to_string()), "summer: {clocks:?}");
        // On regular hours, from the cash open at 08:30.
        let regular: Vec<Bar> = src.iter().copied().filter(|b| schedule.is_regular(b.ts)).collect();
        let out = resample(&regular, Timeframe::hours(4), &Grid::new(&es(), Session::Regular));
        let clocks = self::clocks(&out, chicago);
        assert!(clocks.iter().all(|c| c.ends_with("08:30") || c.ends_with("12:30")), "{clocks:?}");
    }

    /// Taipei's half hour before the open is an auction that prints no
    /// bars, so extended hours count from the 09:00 open as regular hours
    /// do, where the inventory's fixed origin already had them.
    #[test]
    fn an_opening_auction_is_not_where_the_day_starts() {
        let taipei = chrono_tz::Asia::Taipei;
        let tsmc = listing("equity", "2330", Some("TW"), 3_600);
        let src: Vec<Bar> = (9..=13).map(|h| flat(at(taipei, (2024, 7, 15), (h, 0)))).collect();
        for session in [Session::Extended, Session::Regular] {
            let out = resample(&src, Timeframe::hours(4), &Grid::new(&tsmc, session));
            assert_eq!(clocks(&out, taipei), ["07-15 09:00", "07-15 13:00"]);
            assert_eq!(out, resample(&src, Timeframe::hours(4), &Grid::fixed(3_600)));
        }
    }

    /// A market that never shuts, and a venue nothing here knows, fold the
    /// way they always have: from midnight UTC, moved by the session origin.
    #[test]
    fn markets_without_hours_keep_the_fixed_origin() {
        let anywhere = listing("equity", "XYZ", Some("ZZ"), 3_600);
        assert!(registry().for_instrument(&anywhere).is_none());
        let cases = [
            (listing("crypto", "BTC", None, 0), 0),
            (listing("fx", "EURUSD", None, 0), 0),
            (anywhere, 3_600),
        ];
        let src: Vec<Bar> = (0..200).map(|i| flat(1_720_000_000 + i * 1_800)).collect();
        for (instrument, origin) in cases {
            for session in [Session::Extended, Session::Regular] {
                for target in [Timeframe::hours(4), Timeframe::hours(3), Timeframe::minutes(45)] {
                    let grid = Grid::new(&instrument, session);
                    assert_eq!(resample(&src, target, &grid), resample(&src, target, &Grid::fixed(origin)));
                    let step = target.seconds();
                    for bar in &src {
                        let before = (bar.ts - origin).div_euclid(step) * step + origin;
                        assert_eq!(bucket_of(bar.ts, target, &grid), before, "{}", instrument.symbol);
                    }
                }
            }
        }
    }

    /// Weeks, months, years and days are not cut by the hour, so a market's
    /// hours change nothing about them.
    #[test]
    fn longer_resolutions_ignore_the_hours() {
        let grid = Grid::new(&spy(), Session::Regular);
        for ts in [1_720_000_000, 1_705_400_000, 1_709_208_000] {
            for target in [Timeframe::days(2), Timeframe::weeks(1), Timeframe::months(1), Timeframe::years(1)] {
                assert_eq!(bucket_of(ts, target, &grid), bucket_of(ts, target, &Grid::fixed(0)));
            }
        }
    }

    /// Steps under an hour count from the open too. Those that divide the
    /// half hour land where they always did; those that do not now start
    /// with the session, as 45m bars at 09:30 and 10:15.
    #[test]
    fn steps_within_the_hour_count_from_the_open() {
        let ny = chrono_tz::America::New_York;
        let day = (2024, 1, 16);
        let src: Vec<Bar> = (0..26).map(|q| flat(at(ny, day, (9, 30)) + q * 900)).collect();
        let grid = Grid::new(&spy(), Session::Regular);
        for minutes in [30, 10, 5] {
            let target = Timeframe::minutes(minutes);
            assert_eq!(resample(&src, target, &grid), resample(&src, target, &Grid::fixed(0)), "{minutes}m");
        }
        let out = resample(&src, Timeframe::minutes(45), &grid);
        assert_eq!(clocks(&out[..3], ny), ["01-16 09:30", "01-16 10:15", "01-16 11:00"]);
        let out = resample(&regular_hours(&[day]), Timeframe::hours(2), &grid);
        assert_eq!(clocks(&out, ny), ["01-16 09:30", "01-16 11:30", "01-16 13:30", "01-16 15:30"]);
    }

    /// A bar before its day's bars start, which a chart of regular hours
    /// should never hold, is counted from the start of the trading day: a
    /// bucket of its own, after the day before and ahead of the open.
    #[test]
    fn a_bar_before_the_open_keeps_its_place() {
        let ny = chrono_tz::America::New_York;
        let src = vec![
            flat(at(ny, (2024, 7, 15), (15, 30))),
            flat(at(ny, (2024, 7, 16), (8, 0))),
            flat(at(ny, (2024, 7, 16), (9, 0))),
            flat(at(ny, (2024, 7, 16), (9, 30))),
        ];
        let out = resample(&src, Timeframe::hours(4), &Grid::new(&spy(), Session::Regular));
        assert_eq!(clocks(&out, ny), ["07-15 13:30", "07-16 08:00", "07-16 09:30"]);
        assert_eq!(out[1].volume, 2.0);
    }

    /// A live tick refolds from the bucket it landed in, and what that gives
    /// has to be the tail of folding the whole series again, wherever the
    /// tick lands: on regular hours across days, and on Globex across the
    /// change to summer time.
    #[test]
    fn a_live_tail_folds_as_the_whole_series_does() {
        let days: Vec<(i32, u32, u32)> = (11..=15).map(|d| (2024, 3, d)).collect();
        let cme = registry().for_instrument(&es()).expect("CME hours");
        let first = at(chrono_tz::America::Chicago, (2024, 3, 7), (17, 0));
        let globex: Vec<Bar> = (0..24 * 6).map(|h| first + h * 3_600).filter(|ts| cme.is_open(*ts)).map(flat).collect();
        let cases = [(spy(), Session::Regular, regular_hours(&days)), (es(), Session::Extended, globex)];
        for (instrument, session, native) in cases {
            for target in [Timeframe::hours(4), Timeframe::hours(3), Timeframe::hours(2)] {
                let whole = resample(&native, target, &Grid::new(&instrument, session));
                for from in 0..native.len() {
                    let grid = Grid::new(&instrument, session);
                    let start = crate::stream::tail_start(&native, from, target, &grid);
                    let tail = resample(&native[start..], target, &grid);
                    assert_eq!(tail[..], whole[whole.len() - tail.len()..], "{} {target:?} from {from}", instrument.symbol);
                }
            }
        }
    }
}
