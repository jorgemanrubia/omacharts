//! Volume at price: what traded at each price, on which side, minute by minute.
//!
//! One kind of data serves every volume indicator that has to know more than
//! a bar's range — the volume profile, bid and ask volume, delta, footprints,
//! a per-bar point of control. Its grain is the **minute ladder**: a minute's
//! open, high, low, close and volume, and for each price that traded the
//! volume that hit the bid and the volume that lifted the offer.
//!
//! Prices are whole ticks (`i32`) of the series' tick size, so a ladder is a
//! dense array indexed from its lowest tick, and summing two is adding
//! integers — exactly, in any order, which is what lets a day keep its total
//! and take a minute back out of it when the provider replaces one.
//!
//! A range is answered as whole days' totals plus the loose minutes at either
//! end, so a year is some 250 additions of a day ladder however long it is.
//!
//! Sides: volume **at the bid** (`at_bid`) is sell-aggressor volume, a seller
//! hitting the bid; volume **at the ask** (`at_ask`) is buy-aggressor volume.
//! Both are what traded, never what was resting in the book: the sizes on
//! offer at a price are depth, which is another thing. A print with no known aggressor counts at
//! its price as [`Level::unknown`] — in the price's volume, so a profile
//! has it — and on neither side, so delta does not. Volume a provider
//! reports with no price at all is in [`Sides::unknown`] alone.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

/// Which trading day a moment belongs to, numbered as its trading date in
/// days since 1970-01-01: a date rather than the instant the session
/// starts, so a day keeps its name across daylight saving and reads back
/// as a date in a store.
///
/// Where days begin is the caller's — an exchange's calendar, or a fixed
/// time of day — so this model needs no calendar of its own.
pub trait TradingDays {
    /// The trading day `ts` (unix seconds) belongs to.
    fn of(&self, ts: i64) -> i64;
}

/// Who took liquidity: a buyer lifting the offer, a seller hitting the bid,
/// or a print that does not say.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
#[repr(u8)]
pub enum Side {
    Buy,
    Sell,
    #[default]
    Unknown,
}

/// `price` as whole ticks of `tick_size`, rounded to the nearest: a price read
/// as 7625.2499999 is still 30501 at 0.25.
#[inline]
pub fn to_tick(price: f64, tick_size: f64) -> i32 {
    (price / tick_size).round() as i32
}

/// The price `tick` whole ticks of `tick_size` stand for.
#[inline]
pub fn to_price(tick: i32, tick_size: f64) -> f64 {
    f64::from(tick) * tick_size
}

/// The unix second a minute opens at.
#[inline]
pub fn minute_of(ts: i64) -> i64 {
    ts.div_euclid(60) * 60
}

/// One price of a ladder: volume and trade count on each side, and the
/// volume whose side nobody said.
///
/// One array of these rather than one array per column, so a minute is one
/// allocation and a price's numbers sit in one cache line.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Level {
    /// Sell-aggressor volume: traded at the bid, not the size bid.
    pub at_bid: u64,
    /// Buy-aggressor volume: traded at the ask, not the size offered.
    pub at_ask: u64,
    /// Volume traded here with no known aggressor: in the price's volume,
    /// on neither side. Its trades are counted only in the ladder's
    /// [`Sides::unknown_trades`].
    pub unknown: u64,
    pub bid_trades: u32,
    pub ask_trades: u32,
}

impl Level {
    /// Everything that traded at this price, sides or not.
    #[inline]
    pub fn volume(&self) -> u64 {
        self.at_bid + self.at_ask + self.unknown
    }

    #[inline]
    fn is_empty(&self) -> bool {
        *self == Level::default()
    }

    #[inline]
    fn add(&mut self, other: &Level) {
        self.at_bid += other.at_bid;
        self.at_ask += other.at_ask;
        self.unknown += other.unknown;
        self.bid_trades += other.bid_trades;
        self.ask_trades += other.ask_trades;
    }

    #[inline]
    fn sub(&mut self, other: &Level) {
        self.at_bid = self.at_bid.saturating_sub(other.at_bid);
        self.at_ask = self.at_ask.saturating_sub(other.at_ask);
        self.unknown = self.unknown.saturating_sub(other.unknown);
        self.bid_trades = self.bid_trades.saturating_sub(other.bid_trades);
        self.ask_trades = self.ask_trades.saturating_sub(other.ask_trades);
    }
}

/// A ladder's totals per side, kept as it is built so a bar's bid and ask
/// volume, delta and trade counts never sum its price array.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Sides {
    /// Sell-aggressor volume, as [`Level::at_bid`].
    pub at_bid: u64,
    /// Buy-aggressor volume, as [`Level::at_ask`].
    pub at_ask: u64,
    /// Volume printed with no aggressor: what the levels hold as
    /// [`Level::unknown`], and any a provider reported with no price.
    pub unknown: u64,
    pub bid_trades: u64,
    pub ask_trades: u64,
    pub unknown_trades: u64,
}

impl Sides {
    /// Everything that traded, sides or not.
    #[inline]
    pub fn volume(&self) -> u64 {
        self.at_bid + self.at_ask + self.unknown
    }

    /// Buyers less sellers: ask volume less bid volume.
    #[inline]
    pub fn delta(&self) -> i64 {
        self.at_ask as i64 - self.at_bid as i64
    }

    #[inline]
    pub fn trades(&self) -> u64 {
        self.bid_trades + self.ask_trades + self.unknown_trades
    }

    #[inline]
    pub fn add(&mut self, other: &Sides) {
        self.at_bid += other.at_bid;
        self.at_ask += other.at_ask;
        self.unknown += other.unknown;
        self.bid_trades += other.bid_trades;
        self.ask_trades += other.ask_trades;
        self.unknown_trades += other.unknown_trades;
    }

    #[inline]
    fn sub(&mut self, other: &Sides) {
        self.at_bid = self.at_bid.saturating_sub(other.at_bid);
        self.at_ask = self.at_ask.saturating_sub(other.at_ask);
        self.unknown = self.unknown.saturating_sub(other.unknown);
        self.bid_trades = self.bid_trades.saturating_sub(other.bid_trades);
        self.ask_trades = self.ask_trades.saturating_sub(other.ask_trades);
        self.unknown_trades = self.unknown_trades.saturating_sub(other.unknown_trades);
    }
}

/// Volume at each price over some stretch: a minute, a bar, a day, a range.
///
/// `levels[i]` is the price `low_tick + i`. Prices inside the range that did
/// not trade are zero; an empty ladder has no levels and its `low_tick`
/// means nothing.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Ladder {
    pub low_tick: i32,
    pub levels: Vec<Level>,
    pub sides: Sides,
}

impl Ladder {
    pub fn new() -> Ladder {
        Ladder::default()
    }

    pub fn with_capacity(levels: usize) -> Ladder {
        Ladder { low_tick: 0, levels: Vec::with_capacity(levels), sides: Sides::default() }
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.levels.is_empty()
    }

    /// The highest tick with a level, traded or not.
    #[inline]
    pub fn high_tick(&self) -> Option<i32> {
        (!self.levels.is_empty()).then(|| self.low_tick + self.levels.len() as i32 - 1)
    }

    /// The level at `tick`, zero outside the range.
    #[inline]
    pub fn at(&self, tick: i32) -> Level {
        let i = i64::from(tick) - i64::from(self.low_tick);
        match usize::try_from(i) {
            Ok(i) => self.levels.get(i).copied().unwrap_or_default(),
            Err(_) => Level::default(),
        }
    }

    /// Everything that traded, sides or not.
    #[inline]
    pub fn total(&self) -> u64 {
        self.sides.volume()
    }

    /// Ask volume less bid volume.
    #[inline]
    pub fn delta(&self) -> i64 {
        self.sides.delta()
    }

    /// The tick that traded most, every side together; the lowest of a tie.
    pub fn point_of_control(&self) -> Option<i32> {
        let mut best: Option<(usize, u64)> = None;
        for (i, level) in self.levels.iter().enumerate() {
            let volume = level.volume();
            if volume > 0 && best.is_none_or(|(_, most)| volume > most) {
                best = Some((i, volume));
            }
        }
        best.map(|(i, _)| self.low_tick + i as i32)
    }

    /// Every level as `(tick, level)`, lowest first.
    pub fn iter(&self) -> impl Iterator<Item = (i32, &Level)> + '_ {
        self.levels.iter().enumerate().map(|(i, level)| (self.low_tick + i as i32, level))
    }

    /// Every level as `(price, at_bid, at_ask)`, lowest first.
    pub fn prices(&self, tick_size: f64) -> impl Iterator<Item = (f64, u64, u64)> + '_ {
        self.iter().map(move |(tick, l)| (to_price(tick, tick_size), l.at_bid, l.at_ask))
    }

    /// Empty it, keeping the allocation.
    pub fn clear(&mut self) {
        self.levels.clear();
        self.sides = Sides::default();
    }

    /// Grow the range to hold `low..=high`, without moving anything already
    /// there off its price.
    #[inline]
    pub fn cover(&mut self, low: i32, high: i32) {
        if self.levels.is_empty() {
            self.low_tick = low;
            self.levels.resize((high - low + 1) as usize, Level::default());
            return;
        }
        if low < self.low_tick {
            let extra = (self.low_tick - low) as usize;
            let len = self.levels.len();
            self.levels.resize(len + extra, Level::default());
            self.levels.copy_within(0..len, extra);
            self.levels[..extra].fill(Level::default());
            self.low_tick = low;
        }
        let top = self.low_tick + self.levels.len() as i32 - 1;
        if high > top {
            let len = self.levels.len() + (high - top) as usize;
            self.levels.resize(len, Level::default());
        }
    }

    /// One print: `count` trades of `size` in all at `tick`.
    ///
    /// A print with no aggressor is counted at its price, on neither side.
    #[inline]
    pub fn add_trade(&mut self, tick: i32, size: u64, side: Side, count: u32) {
        self.cover(tick, tick);
        let level = &mut self.levels[(tick - self.low_tick) as usize];
        match side {
            Side::Buy => {
                level.at_ask += size;
                level.ask_trades += count;
                self.sides.at_ask += size;
                self.sides.ask_trades += u64::from(count);
            }
            Side::Sell => {
                level.at_bid += size;
                level.bid_trades += count;
                self.sides.at_bid += size;
                self.sides.bid_trades += u64::from(count);
            }
            Side::Unknown => {
                level.unknown += size;
                self.sides.unknown += size;
                self.sides.unknown_trades += u64::from(count);
            }
        }
    }

    /// Add a bar's ladder in.
    #[inline]
    pub fn add(&mut self, bar: &LadderBar) {
        self.add_ladder(&bar.ladder);
    }

    /// Add another ladder in, growing the range as needed.
    #[inline]
    pub fn add_ladder(&mut self, other: &Ladder) {
        self.sides.add(&other.sides);
        let Some(high) = other.high_tick() else { return };
        self.cover(other.low_tick, high);
        let offset = (other.low_tick - self.low_tick) as usize;
        for (mine, theirs) in self.levels[offset..offset + other.levels.len()].iter_mut().zip(&other.levels) {
            mine.add(theirs);
        }
    }

    /// Take a ladder that was added in back out, and drop the prices left
    /// empty at either end.
    #[inline]
    pub fn sub_ladder(&mut self, other: &Ladder) {
        self.sides.sub(&other.sides);
        for (tick, theirs) in other.iter() {
            let i = i64::from(tick) - i64::from(self.low_tick);
            if let Some(mine) = usize::try_from(i).ok().and_then(|i| self.levels.get_mut(i)) {
                mine.sub(theirs);
            }
        }
        self.trim();
    }

    /// Drop untraded prices at either end.
    pub fn trim(&mut self) {
        let Some(first) = self.levels.iter().position(|l| !l.is_empty()) else {
            self.levels.clear();
            return;
        };
        let last = self.levels.iter().rposition(|l| !l.is_empty()).unwrap_or(first);
        self.levels.truncate(last + 1);
        if first > 0 {
            self.levels.drain(..first);
            self.low_tick += first as i32;
        }
    }
}

/// A bar with its ladder: OHLC and volume, and what traded at each price.
///
/// Any length: a minute, a footprint bar folded from minutes, a tick,
/// volume or range bar folded from trades. `ts` is where it opens.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct LadderBar {
    /// Where the bar opens, unix seconds.
    pub ts: i64,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    /// Everything that traded, aggressor known or not, in contracts.
    ///
    /// Whole, where [`crate::Bar::volume`] is an `f64`: a ladder counts
    /// prints, which are whole contracts on every market that has ladders,
    /// and adding and taking minutes out of a day's total has to come back
    /// exactly. A bar's volume comes from feeds that report fractions — a
    /// crypto pair's, an adjusted series' — and is only ever drawn. A bar
    /// made from one of these takes its volume `as f64`, which is exact up
    /// to 2^53 contracts.
    pub volume: u64,
    pub ladder: Ladder,
}

/// One minute of a contract: the grain ladders are kept in — fetched,
/// stored, sealed live — and every longer [`LadderBar`] is folded from.
///
/// The same type, named for what it holds wherever it is always one
/// minute long; a bar of any other length is a [`LadderBar`].
pub type Minute = LadderBar;

impl LadderBar {
    /// A bar opened by a print at `price`, with nothing traded yet.
    #[inline]
    pub fn opening(ts: i64, price: f64) -> LadderBar {
        LadderBar { ts, open: price, high: price, low: price, close: price, volume: 0, ladder: Ladder::with_capacity(32) }
    }

    /// One print: OHLC, volume and the ladder together. Every fold of
    /// trades — the forming minute, tick, volume and range bars — goes
    /// through here.
    #[inline]
    pub fn add_trade(&mut self, tick: i32, price: f64, size: u64, side: Side, count: u32) {
        self.high = self.high.max(price);
        self.low = self.low.min(price);
        self.close = price;
        self.volume += size;
        self.ladder.add_trade(tick, size, side, count);
    }

    /// Fold a later bar into this one: the high, low, close, volume and ladder.
    #[inline]
    pub fn merge(&mut self, later: &LadderBar) {
        self.high = self.high.max(later.high);
        self.low = self.low.min(later.low);
        self.close = later.close;
        self.volume += later.volume;
        self.ladder.add_ladder(&later.ladder);
    }

    #[inline]
    pub fn sides(&self) -> &Sides {
        &self.ladder.sides
    }
}

/// A ladder bar read as a bar, for the studies.
impl omacharts_studies::Ohlcv for LadderBar {
    #[inline(always)]
    fn ts(&self) -> i64 {
        self.ts
    }
    #[inline(always)]
    fn open(&self) -> f64 {
        self.open
    }
    #[inline(always)]
    fn high(&self) -> f64 {
        self.high
    }
    #[inline(always)]
    fn low(&self) -> f64 {
        self.low
    }
    #[inline(always)]
    fn close(&self) -> f64 {
        self.close
    }
    #[inline(always)]
    fn volume(&self) -> f64 {
        self.volume as f64
    }
}

/// A ladder bar's volume at each price and on each side, for the studies
/// of volume at price.
impl omacharts_studies::AtPrice for LadderBar {
    #[inline]
    fn bought(&self) -> u64 {
        self.ladder.sides.at_ask
    }
    #[inline]
    fn sold(&self) -> u64 {
        self.ladder.sides.at_bid
    }
    #[inline]
    fn each_price(&self, mut each: impl FnMut(i32, u64)) {
        for (tick, level) in self.ladder.iter() {
            each(tick, level.volume());
        }
    }
}

/// One trading day: its minutes in order, and their sum.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct Day {
    /// The trading date, days since 1970-01-01; see the caller's [`TradingDays`].
    pub day: i64,
    /// Strictly increasing by `ts`.
    pub minutes: Vec<Minute>,
    /// The sum of `minutes`, kept as they change.
    pub total: Ladder,
}

impl Day {
    pub fn new(day: i64) -> Day {
        Day { day, minutes: Vec::new(), total: Ladder::new() }
    }

    pub fn first_ts(&self) -> Option<i64> {
        self.minutes.first().map(|m| m.ts)
    }

    pub fn last_ts(&self) -> Option<i64> {
        self.minutes.last().map(|m| m.ts)
    }

    /// Put `minute` in its place, replacing one already at its `ts`, and keep
    /// the total right.
    pub fn insert(&mut self, minute: Minute) {
        match self.minutes.binary_search_by_key(&minute.ts, |m| m.ts) {
            Ok(at) => {
                self.total.sub_ladder(&self.minutes[at].ladder);
                self.total.add_ladder(&minute.ladder);
                self.minutes[at] = minute;
            }
            Err(at) => {
                self.total.add_ladder(&minute.ladder);
                self.minutes.insert(at, minute);
            }
        }
    }

    /// Take out every minute opening in `from..to`, and keep the total right.
    pub fn remove(&mut self, from: i64, to: i64) {
        let (a, b) = self.span(from, to);
        for minute in self.minutes.drain(a..b) {
            self.total.sub_ladder(&minute.ladder);
        }
    }

    /// The total recomputed from the minutes, for a day read without one.
    pub fn resum(&mut self) {
        let mut total = Ladder::new();
        for minute in &self.minutes {
            total.add(minute);
        }
        self.total = total;
    }

    /// What traded in the minutes of this day opening in `from..to`.
    pub fn piece(&self, from: i64, to: i64) -> Ladder {
        let (a, b) = self.span(from, to);
        let parts: Vec<&Ladder> = self.minutes[a..b].iter().map(|m| &m.ladder).collect();
        sum(&parts)
    }

    /// The indices of the minutes opening in `from..to`.
    fn span(&self, from: i64, to: i64) -> (usize, usize) {
        let a = self.minutes.partition_point(|m| m.ts < from);
        let b = self.minutes.partition_point(|m| m.ts < to).max(a);
        (a, b)
    }
}

/// A contract's minute ladders, by trading day.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct Series {
    pub tick_size: f64,
    /// Ordered by `day`, and so by time; none is empty.
    pub days: Vec<Day>,
}

impl Series {
    pub fn new(tick_size: f64) -> Series {
        Series { tick_size, days: Vec::new() }
    }

    /// Put `minute` into trading day `day`, replacing a minute already at its
    /// `ts`. The day is made if it is new.
    pub fn insert_minute(&mut self, day: i64, minute: Minute) {
        // Appending to the last day is the live path and the usual one.
        let at = match self.days.last() {
            Some(last) if last.day == day => self.days.len() - 1,
            _ => match self.days.binary_search_by_key(&day, |d| d.day) {
                Ok(at) => at,
                Err(at) => {
                    self.days.insert(at, Day::new(day));
                    at
                }
            },
        };
        self.days[at].insert(minute);
    }

    /// [`Self::insert_minute`], finding the day with `days`.
    pub fn insert(&mut self, days: &impl TradingDays, minute: Minute) {
        let day = days.of(minute.ts);
        self.insert_minute(day, minute);
    }

    /// Take out every minute opening in `from..to`.
    pub fn remove(&mut self, from: i64, to: i64) {
        let (a, b) = self.day_span(from, to);
        for day in &mut self.days[a..b] {
            day.remove(from, to);
        }
        self.days.retain(|d| !d.minutes.is_empty());
    }

    /// Every minute, oldest first.
    pub fn minutes(&self) -> impl Iterator<Item = &Minute> + '_ {
        self.days.iter().flat_map(|d| d.minutes.iter())
    }

    /// The days holding any minute in `from..to`.
    fn day_span(&self, from: i64, to: i64) -> (usize, usize) {
        let a = self.days.partition_point(|d| d.last_ts().is_some_and(|ts| ts < from));
        let b = self.days.partition_point(|d| d.first_ts().is_some_and(|ts| ts < to)).max(a);
        (a, b)
    }

    /// Visit `from..to` as whole days and loose minutes: the total of each
    /// day inside it, and the ladder of each minute of a day it only partly
    /// covers. Two binary searches find the days at the edges and two more
    /// the minutes inside each of those; everything between is a day total.
    pub fn visit(&self, from: i64, to: i64, mut each: impl FnMut(&Ladder)) {
        let (a, b) = self.day_span(from, to);
        for day in &self.days[a..b] {
            let inside = day.first_ts().is_some_and(|t| t >= from) && day.last_ts().is_some_and(|t| t < to);
            if inside {
                each(&day.total);
            } else {
                let (i, j) = day.span(from, to);
                day.minutes[i..j].iter().for_each(|m| each(&m.ladder));
            }
        }
    }

    /// What traded at each price, by side, in the minutes opening in
    /// `from..to`.
    pub fn volume_at_price(&self, from: i64, to: i64) -> Ladder {
        // Covering the whole span up front saves regrowing as days arrive.
        let (mut low, mut high) = (i32::MAX, i32::MIN);
        self.visit(from, to, |ladder| {
            if let Some(top) = ladder.high_tick() {
                low = low.min(ladder.low_tick);
                high = high.max(top);
            }
        });
        let mut sum = Ladder::new();
        if low <= high {
            sum.cover(low, high);
        }
        self.visit(from, to, |ladder| sum_into(&mut sum, ladder));
        sum
    }

    /// The per-side totals of `from..to`, the way [`Self::volume_at_price`]
    /// finds them but without building a ladder.
    pub fn side_totals(&self, from: i64, to: i64) -> Sides {
        let mut sides = Sides::default();
        self.visit(from, to, |ladder| sides.add(&ladder.sides));
        sides
    }

    /// The minutes opening in `from..to` folded into bars, each with its
    /// ladder — footprints — where `bucket` says where the bar a minute
    /// belongs to opens. One allocation per bar, none per minute.
    pub fn bars_by(&self, from: i64, to: i64, mut bucket: impl FnMut(i64) -> i64) -> Vec<LadderBar> {
        let mut out: Vec<LadderBar> = Vec::new();
        let (a, b) = self.day_span(from, to);
        for day in &self.days[a..b] {
            let (i, j) = day.span(from, to);
            for minute in &day.minutes[i..j] {
                let ts = bucket(minute.ts);
                match out.last_mut() {
                    Some(bar) if bar.ts == ts => bar.merge(minute),
                    _ => out.push(LadderBar { ts, ladder: minute.ladder.clone(), ..*minute }),
                }
            }
        }
        out
    }
}

/// Add `ladder` into `sum`, whose range already covers it.
fn sum_into(sum: &mut Ladder, ladder: &Ladder) {
    sum.sides.add(&ladder.sides);
    if ladder.levels.is_empty() {
        return;
    }
    let offset = (ladder.low_tick - sum.low_tick) as usize;
    for (mine, theirs) in sum.levels[offset..offset + ladder.levels.len()].iter_mut().zip(&ladder.levels) {
        mine.add(theirs);
    }
}

/// The minute being traded now, built from live prints.
///
/// Trades are added and forgotten: when one arrives for a later minute the
/// forming one is sealed and handed back, final, to be put in the
/// [`Series`] and stored. It also keeps track of which minutes are
/// *complete* — every print seen since the feed connected — so a disconnect
/// reads as a gap to backfill rather than as minutes in which nothing
/// traded. A minute the feed joined or left part way through is still handed
/// back, so a chart draws it, but [`Forming::covered`] leaves it out.
#[derive(Clone, Debug)]
pub struct Forming {
    tick_size: f64,
    minute: Option<Minute>,
    /// The first minute every print of which has been seen, while connected.
    since: Option<i64>,
}

const NANOS: i64 = 1_000_000_000;

impl Forming {
    pub fn new(tick_size: f64) -> Forming {
        Forming { tick_size, minute: None, since: None }
    }

    #[inline]
    pub fn tick_size(&self) -> f64 {
        self.tick_size
    }

    /// The feed is live as of `at_nanos`: every minute from the next whole
    /// one on is complete until [`Self::disconnect`].
    pub fn connect(&mut self, at_nanos: i64) {
        let minute = at_nanos.div_euclid(60 * NANOS) * 60;
        let joined_late = at_nanos.rem_euclid(60 * NANOS) != 0;
        self.since = Some(if joined_late { minute + 60 } else { minute });
    }

    /// The feed dropped. Hands back the minute that was forming, which is
    /// partial and not covered; nothing after it is complete until the feed
    /// connects again.
    pub fn disconnect(&mut self) -> Option<Minute> {
        self.since = None;
        self.minute.take()
    }

    #[inline]
    pub fn is_connected(&self) -> bool {
        self.since.is_some()
    }

    /// The minute forming now, if anything has traded in it.
    #[inline]
    pub fn current(&self) -> Option<&Minute> {
        self.minute.as_ref()
    }

    /// The span of minute opens, `from..to`, known complete: every print
    /// in it was seen, and a minute missing from it traded nothing. Runs up
    /// to the forming minute, which is not complete yet.
    #[inline]
    pub fn covered(&self) -> Option<(i64, i64)> {
        let since = self.since?;
        let to = self.minute.as_ref().map(|m| m.ts)?;
        (to > since).then_some((since, to))
    }

    /// A print at `ts` (unix seconds), `price`, `size` contracts.
    #[inline]
    pub fn add_trade(&mut self, ts: i64, price: f64, size: u64, side: Side) -> Option<Minute> {
        self.push(ts * NANOS, to_tick(price, self.tick_size), size, side)
    }

    /// A print at `ts_nanos`, `tick`. Returns the minute it sealed, if it
    /// opened a new one.
    ///
    /// A print for a minute earlier than the forming one — a late report —
    /// is counted in the forming minute rather than lost.
    #[inline]
    pub fn push(&mut self, ts_nanos: i64, tick: i32, size: u64, side: Side) -> Option<Minute> {
        if self.since.is_none() {
            self.connect(ts_nanos);
        }
        let ts = ts_nanos.div_euclid(60 * NANOS) * 60;
        let price = to_price(tick, self.tick_size);
        let sealed = match &self.minute {
            Some(forming) if ts > forming.ts => self.minute.take(),
            _ => None,
        };
        let minute = self.minute.get_or_insert_with(|| Minute::opening(ts, price));
        minute.add_trade(tick, price, size, side, 1);
        sealed
    }

    /// Seal the forming minute if the clock, at `now` unix seconds, has left
    /// it: a quiet market sends no print to do it.
    #[inline]
    pub fn seal_by(&mut self, now: i64) -> Option<Minute> {
        match &self.minute {
            Some(forming) if now >= forming.ts + 60 => self.minute.take(),
            _ => None,
        }
    }
}

// -- what a chart holds ------------------------------------------------------

/// One trading day as a chart holds it: always its total, and its minutes
/// only when a span cuts through it.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct HeldDay {
    /// The day, its total, and its minutes when `whole` — none otherwise.
    pub day: Day,
    /// The first and last minute it holds, read beside the total so a day
    /// can be placed without its minutes.
    pub first_ts: i64,
    pub last_ts: i64,
    /// Are `day.minutes` loaded?
    pub whole: bool,
}

impl HeldDay {
    /// A day with its minutes.
    pub fn whole(day: Day) -> Option<HeldDay> {
        let (first_ts, last_ts) = (day.first_ts()?, day.last_ts()?);
        Some(HeldDay { day, first_ts, last_ts, whole: true })
    }

    /// A day known only by its total.
    #[inline]
    pub fn total(number: i64, first_ts: i64, last_ts: i64, total: Ladder) -> HeldDay {
        HeldDay { day: Day { day: number, minutes: Vec::new(), total }, first_ts, last_ts, whole: false }
    }

    fn inside(&self, from: i64, to: i64) -> bool {
        self.first_ts >= from && self.last_ts < to
    }

    fn touches(&self, from: i64, to: i64) -> bool {
        self.last_ts >= from && self.first_ts < to
    }
}

/// The part of trading day `day` a span `from..to` cuts: its minutes
/// opening in `from..to`, summed. What a period that starts or ends inside
/// a day needs of it, held in place of the day's minutes.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Piece {
    pub day: i64,
    pub from: i64,
    pub to: i64,
}

impl Piece {
    /// The piece of `held` that `from..to` cuts, an end past the day's own
    /// kept open — `i64::MIN` or `i64::MAX` — so a period that runs on past
    /// the day's last minute asks for one piece however far it reaches, and
    /// keeps it as minutes seal onto the day: see [`Held::seal`].
    pub fn of(held: &HeldDay, from: i64, to: i64) -> Piece {
        Piece {
            day: held.day.day,
            from: if from <= held.first_ts { i64::MIN } else { from },
            to: if to > held.last_ts { i64::MAX } else { to },
        }
    }

    /// Does a minute opening at `ts` fall in it?
    pub fn holds(&self, ts: i64) -> bool {
        (self.from..self.to).contains(&ts)
    }
}

/// What a chart holds of a contract's ladders: the total of every day its
/// bars reach, the pieces of the days a period cuts, the minutes of the days
/// sealed live, and the minute forming now.
///
/// A year of minutes decoded is some 280 MB, and a session profile is
/// answered by day totals alone. A period that cuts a day — a regular
/// session cuts every one — needs only that part of it, so the part is read
/// and summed elsewhere and held as one ladder, a few kilobytes, rather than
/// the day's minutes, a megabyte: see [`Held::take_wanted`]. A span this
/// cannot answer exactly is answered with `None`, never with part of it.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct Held {
    pub tick_size: f64,
    /// Ordered by day.
    pub days: Vec<HeldDay>,
    /// The minute trading now, which no day holds yet.
    pub forming: Option<Minute>,
    /// Where the ladders begin: the provider's history goes no further
    /// back, so a span that starts before this is not known whole.
    pub floor: i64,
    /// The first trading day held from its start, for spans counted in days.
    pub floor_day: i64,
    /// The parts of days without their minutes that spans have cut, each
    /// summed.
    pub pieces: HashMap<Piece, Ladder>,
    /// Pieces a span was asked of and could not be answered without, for the
    /// caller to read: see [`Held::take_wanted`].
    pub wanted: RefCell<Vec<Piece>>,
    /// Pieces that answered a span since [`Held::take_used`] last took them:
    /// what is not to be let go to make room.
    pub used: RefCell<HashSet<Piece>>,
    /// Pieces that are not on their way: the store did not have them when
    /// asked, or there was no room to hold them. A span that needs one keeps
    /// the estimate; one that needs a piece still coming waits for it. See
    /// [`Held::coming`].
    pub missing: HashSet<Piece>,
    /// Each minute's ask volume less its bid volume, oldest first, for days
    /// held without their minutes: what a cumulative delta walks minute by
    /// minute, some twenty kilobytes a day where the minutes are a megabyte.
    /// Read only for the days a cumulative delta reached; see
    /// [`Held::take_wanted_flows`].
    pub flows: HashMap<i64, Vec<(i64, i64)>>,
    /// Days whose flow a computation reached for and did not find.
    pub wanted_flows: RefCell<Vec<i64>>,
    /// Changes whenever what is held of a minute already past changes —
    /// days loaded, flows put in — so a computation kept from before can
    /// tell it has to start again. A minute sealed live does not change it:
    /// that is the newest, and only the bars it falls in need doing again.
    pub generation: u64,
}

/// A number no [`Held`] has had before.
fn fresh_generation() -> u64 {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

impl Day {
    /// Each minute's ask volume less its bid volume, by when it opens.
    pub fn flow(&self) -> Vec<(i64, i64)> {
        self.minutes.iter().map(|m| (m.ts, m.ladder.sides.delta())).collect()
    }
}

impl Held {
    pub fn new(tick_size: f64) -> Held {
        Held { tick_size, generation: fresh_generation(), ..Held::default() }
    }

    /// Everything in `series`, minutes and all, trusted from its first
    /// minute on; whole days from the one after it.
    pub fn of(series: &Series) -> Held {
        let days: Vec<HeldDay> = series.days.iter().cloned().filter_map(HeldDay::whole).collect();
        let floor = days.first().map_or(i64::MAX, |d| d.first_ts);
        let floor_day = days.first().map_or(i64::MAX, |d| d.day.day + 1);
        Held { tick_size: series.tick_size, days, floor, floor_day, generation: fresh_generation(), ..Held::default() }
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.days.is_empty() && self.forming.is_none()
    }

    /// The pieces [`Self::between`] was asked about and could not answer
    /// without, newest day first, taken.
    pub fn take_wanted(&self) -> Vec<Piece> {
        let mut wanted = std::mem::take(&mut *self.wanted.borrow_mut());
        wanted.sort_unstable_by(|a, b| b.day.cmp(&a.day).then(b.from.cmp(&a.from)));
        wanted
    }

    /// The pieces that answered a span since this was last called, taken.
    pub fn take_used(&self) -> HashSet<Piece> {
        std::mem::take(&mut *self.used.borrow_mut())
    }

    /// Put in a piece read for a span, unless its day has since come whole
    /// or gone. Whether it went in.
    pub fn put_piece(&mut self, piece: Piece, ladder: Ladder) -> bool {
        let Ok(at) = self.days.binary_search_by_key(&piece.day, |d| d.day.day) else { return false };
        if self.days[at].whole {
            return false;
        }
        self.missing.remove(&piece);
        self.pieces.insert(piece, ladder);
        true
    }

    /// Roughly what the pieces hold, in bytes.
    pub fn piece_bytes(&self) -> usize {
        self.pieces.values().map(|l| l.levels.capacity() * std::mem::size_of::<Level>()).sum::<usize>()
            + self.pieces.len() * std::mem::size_of::<(Piece, Ladder)>()
    }

    /// Roughly what it holds, in bytes.
    pub fn bytes(&self) -> usize {
        let ladder = |l: &Ladder| l.levels.capacity() * std::mem::size_of::<Level>();
        let minute = |m: &Minute| ladder(&m.ladder) + std::mem::size_of::<Minute>();
        self.days
            .iter()
            .map(|d| {
                std::mem::size_of::<HeldDay>()
                    + ladder(&d.day.total)
                    + d.day.minutes.iter().map(minute).sum::<usize>()
                    + (d.day.minutes.capacity() - d.day.minutes.len()) * std::mem::size_of::<Minute>()
            })
            .sum::<usize>()
            + self.piece_bytes()
            + self.forming.as_ref().map_or(0, minute)
    }

    fn span(&self, from: i64, to: i64) -> impl Iterator<Item = &HeldDay> + '_ {
        let a = self.days.partition_point(|d| d.last_ts < from);
        let b = self.days.partition_point(|d| d.first_ts < to).max(a);
        self.days[a..b].iter().filter(move |d| d.touches(from, to))
    }

    /// The days `from..to` cuts through whose minutes are not loaded: what
    /// to read before [`Self::between`] can answer it.
    pub fn cut(&self, from: i64, to: i64) -> Vec<i64> {
        self.span(from, to).filter(|d| !d.whole && !d.inside(from, to)).map(|d| d.day.day).collect()
    }

    /// The days a cumulative delta reached for and found neither minutes
    /// nor a flow for, newest first, taken.
    pub fn take_wanted_flows(&self) -> Vec<i64> {
        let mut wanted = std::mem::take(&mut *self.wanted_flows.borrow_mut());
        wanted.sort_unstable_by(|a, b| b.cmp(a));
        wanted.dedup();
        wanted
    }

    /// Put in a day's flow, unless the day has since come whole or gone.
    /// Whether it went in.
    pub fn put_flow(&mut self, day: i64, flow: Vec<(i64, i64)>) -> bool {
        let Ok(at) = self.days.binary_search_by_key(&day, |d| d.day.day) else { return false };
        if self.days[at].whole {
            return false;
        }
        self.flows.insert(day, flow);
        self.generation = fresh_generation();
        true
    }

    /// Hand each minute opening in `from..to` to `each`, oldest first, as
    /// its open and its ask volume less its bid volume, the forming one
    /// included. `false`, with whatever was handed already to be thrown
    /// away, when that cannot be told exactly: the span starts before
    /// [`Self::floor`], or reaches a day held by its total alone whose flow
    /// is not read yet, which is then asked for.
    pub fn minute_deltas(&self, from: i64, to: i64, mut each: impl FnMut(i64, i64)) -> bool {
        if from < self.floor {
            return false;
        }
        let mut short = false;
        for held in self.span(from, to) {
            if held.whole {
                let (i, j) = held.day.span(from, to);
                if !short {
                    held.day.minutes[i..j].iter().for_each(|m| each(m.ts, m.ladder.sides.delta()));
                }
            } else if let Some(flow) = self.flows.get(&held.day.day) {
                if !short {
                    let a = flow.partition_point(|(ts, _)| *ts < from);
                    let b = flow.partition_point(|(ts, _)| *ts < to).max(a);
                    flow[a..b].iter().for_each(|(ts, delta)| each(*ts, *delta));
                }
            } else {
                let mut wanted = self.wanted_flows.borrow_mut();
                if !wanted.contains(&held.day.day) {
                    wanted.push(held.day.day);
                }
                short = true;
            }
        }
        if short {
            return false;
        }
        if let Some(forming) = self.forming.as_ref().filter(|m| (from..to).contains(&m.ts)) {
            each(forming.ts, forming.ladder.sides.delta());
        }
        true
    }

    /// The ask volume less the bid volume of the trading days numbered
    /// `from_day..to_day`, by their totals, and the forming minute if it is
    /// in one of them: [`Self::days_between`] without building a ladder.
    pub fn days_delta(&self, from_day: i64, to_day: i64, days: &impl TradingDays) -> Option<i64> {
        if from_day < self.floor_day {
            return None;
        }
        let a = self.days.partition_point(|d| d.day.day < from_day);
        let b = self.days.partition_point(|d| d.day.day < to_day).max(a);
        let mut delta: i64 = self.days[a..b].iter().map(|d| d.day.total.sides.delta()).sum();
        if let Some(forming) = self.forming.as_ref().filter(|m| (from_day..to_day).contains(&days.of(m.ts))) {
            delta += forming.ladder.sides.delta();
        }
        Some(delta)
    }

    /// Put a day's minutes in, in place of its total alone and any pieces.
    pub fn load(&mut self, day: Day) {
        let Some(held) = HeldDay::whole(day) else { return };
        let number = held.day.day;
        self.pieces.retain(|piece, _| piece.day != number);
        self.flows.remove(&number);
        self.generation = fresh_generation();
        match self.days.binary_search_by_key(&held.day.day, |d| d.day.day) {
            Ok(at) => self.days[at] = held,
            Err(at) => self.days.insert(at, held),
        }
    }

    /// What traded at each price in the minutes opening in `from..to`, the
    /// forming one included: whole days by their totals, the loose minutes
    /// at either end one by one. `None` when that cannot be told exactly:
    /// the span starts before [`Self::floor`], or cuts a day whose minutes
    /// are not loaded and whose piece is not read yet.
    pub fn between(&self, from: i64, to: i64) -> Option<Ladder> {
        if from < self.floor {
            return None;
        }
        let mut parts: Vec<&Ladder> = Vec::new();
        let mut short = false;
        for held in self.span(from, to) {
            if held.inside(from, to) {
                parts.push(&held.day.total);
            } else if held.whole {
                let (i, j) = held.day.span(from, to);
                parts.extend(held.day.minutes[i..j].iter().map(|m| &m.ladder));
            } else {
                let piece = Piece::of(held, from, to);
                match self.pieces.get(&piece) {
                    Some(ladder) => {
                        parts.push(ladder);
                        self.used.borrow_mut().insert(piece);
                    }
                    None => {
                        let mut wanted = self.wanted.borrow_mut();
                        if !wanted.contains(&piece) {
                            wanted.push(piece);
                        }
                        short = true;
                    }
                }
            }
        }
        if short {
            return None;
        }
        if let Some(forming) = self.forming.as_ref().filter(|m| (from..to).contains(&m.ts)) {
            parts.push(&forming.ladder);
        }
        Some(sum(&parts))
    }

    /// Whether [`Self::between`] cannot answer `from..to` only because a
    /// piece of it is still to be read: the span is not before
    /// [`Self::floor`], and some day it cuts has neither its minutes nor
    /// that piece, which is not among the [`Self::missing`]. Such a span is
    /// answered exactly in a moment, so a profile of it is better drawn
    /// nothing than estimated now and redrawn then.
    pub fn coming(&self, from: i64, to: i64) -> bool {
        from >= self.floor
            && self.span(from, to).any(|held| {
                if held.whole || held.inside(from, to) {
                    return false;
                }
                let piece = Piece::of(held, from, to);
                !self.pieces.contains_key(&piece) && !self.missing.contains(&piece)
            })
    }

    /// The trading days numbered `from_day..to_day` by their totals, and the
    /// forming minute if it is in one of them: for bars a day or longer.
    /// `None` when the first of them is before [`Self::floor_day`].
    pub fn days_between(&self, from_day: i64, to_day: i64, days: &impl TradingDays) -> Option<Ladder> {
        if from_day < self.floor_day {
            return None;
        }
        let a = self.days.partition_point(|d| d.day.day < from_day);
        let b = self.days.partition_point(|d| d.day.day < to_day).max(a);
        let mut parts: Vec<&Ladder> = self.days[a..b].iter().map(|d| &d.day.total).collect();
        if let Some(forming) = self.forming.as_ref().filter(|m| (from_day..to_day).contains(&days.of(m.ts))) {
            parts.push(&forming.ladder);
        }
        Some(sum(&parts))
    }

    /// A minute that closed, into trading day `number`: in place of one at
    /// its `ts` in a day whose minutes are loaded, onto the total of one
    /// whose are not — which live only ever hands minutes newer than it
    /// holds.
    pub fn seal(&mut self, number: i64, minute: Minute) {
        if self.forming.as_ref().is_some_and(|f| f.ts <= minute.ts) {
            self.forming = None;
        }
        let at = match self.days.binary_search_by_key(&number, |d| d.day.day) {
            Ok(at) => at,
            Err(at) => {
                let fresh = HeldDay { day: Day::new(number), first_ts: minute.ts, last_ts: minute.ts, whole: true };
                self.days.insert(at, fresh);
                at
            }
        };
        let held = &mut self.days[at];
        held.first_ts = held.first_ts.min(minute.ts);
        held.last_ts = held.last_ts.max(minute.ts);
        if held.whole {
            held.day.insert(minute);
        } else {
            if let Some(flow) = self.flows.get_mut(&number) {
                let entry = (minute.ts, minute.ladder.sides.delta());
                match flow.binary_search_by_key(&minute.ts, |(ts, _)| *ts) {
                    Ok(at) => flow[at] = entry,
                    Err(at) => flow.insert(at, entry),
                }
            }
            held.day.total.add(&minute);
            // The pieces of it whose span the minute opens in take it too,
            // so a period running on past the day's last minute keeps its
            // piece rather than reading the day again.
            for (piece, ladder) in self.pieces.iter_mut() {
                if piece.day == number && piece.holds(minute.ts) {
                    ladder.add_ladder(&minute.ladder);
                }
            }
        }
    }
}

/// The sum of `parts`, sized once.
fn sum(parts: &[&Ladder]) -> Ladder {
    let (mut low, mut high) = (i32::MAX, i32::MIN);
    for part in parts {
        if let Some(top) = part.high_tick() {
            low = low.min(part.low_tick);
            high = high.max(top);
        }
    }
    let mut total = Ladder::new();
    if low <= high {
        total.cover(low, high);
    }
    for part in parts {
        sum_into(&mut total, part);
    }
    total
}

// -- encoding ---------------------------------------------------------------

/// The encoding's version, the first byte of every blob. Version 1, from
/// before a price kept its volume with no known side, is still read: its
/// levels read back with none, and the ladder's total with all of it.
pub const ENCODING_VERSION: u8 = 2;

/// Flag: per-price trade counts follow each level.
#[doc(hidden)]
pub const FLAG_COUNTS: u8 = 1;

/// Flag: each level carries its volume with no known side. Never set in
/// version 1.
#[doc(hidden)]
pub const FLAG_UNKNOWN: u8 = 2;

/// Why a blob could not be read.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DecodeError {
    /// It ended part way through.
    Truncated,
    /// Written by an encoding this build does not know.
    Version(u8),
    /// It reads, but not as anything this could have written.
    Corrupt,
}

impl std::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DecodeError::Truncated => write!(f, "ladder data ends early"),
            DecodeError::Version(v) => write!(f, "ladder data is in encoding {v}, unknown here"),
            DecodeError::Corrupt => write!(f, "ladder data is corrupt"),
        }
    }
}

impl std::error::Error for DecodeError {}

fn put_varint(out: &mut Vec<u8>, mut value: u64) {
    while value >= 0x80 {
        out.push((value as u8) | 0x80);
        value >>= 7;
    }
    out.push(value as u8);
}

fn put_zigzag(out: &mut Vec<u8>, value: i64) {
    put_varint(out, ((value << 1) ^ (value >> 63)) as u64);
}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn byte(&mut self) -> Result<u8, DecodeError> {
        let b = *self.bytes.get(self.at).ok_or(DecodeError::Truncated)?;
        self.at += 1;
        Ok(b)
    }

    fn varint(&mut self) -> Result<u64, DecodeError> {
        let mut value = 0u64;
        for shift in (0..64).step_by(7) {
            let b = self.byte()?;
            value |= u64::from(b & 0x7f) << shift;
            if b < 0x80 {
                return Ok(value);
            }
        }
        Err(DecodeError::Corrupt)
    }

    fn zigzag(&mut self) -> Result<i64, DecodeError> {
        let v = self.varint()?;
        Ok((v >> 1) as i64 ^ -((v & 1) as i64))
    }

    fn u32(&mut self) -> Result<u32, DecodeError> {
        u32::try_from(self.varint()?).map_err(|_| DecodeError::Corrupt)
    }

    fn tick(&mut self, base: i64) -> Result<i32, DecodeError> {
        i32::try_from(base + self.zigzag()?).map_err(|_| DecodeError::Corrupt)
    }

    /// The version and the flags.
    fn header(&mut self) -> Result<u8, DecodeError> {
        match self.byte()? {
            ENCODING_VERSION => self.byte(),
            1 => Ok(self.byte()? & !FLAG_UNKNOWN),
            other => Err(DecodeError::Version(other)),
        }
    }
}

/// Do the per-price trade counts add up to the per-side totals? Only then
/// can they stand in for the totals; a ladder whose provider sent totals
/// alone is written with the totals and no per-price counts.
fn counts_complete(ladder: &Ladder) -> bool {
    let (bid, ask) = ladder
        .levels
        .iter()
        .fold((0u64, 0u64), |(b, a), l| (b + u64::from(l.bid_trades), a + u64::from(l.ask_trades)));
    bid == ladder.sides.bid_trades && ask == ladder.sides.ask_trades
}

/// Does any price carry volume with no known side? Only then is a level's
/// written; a provider that always says the side pays nothing for it.
fn has_unknown(ladder: &Ladder) -> bool {
    ladder.levels.iter().any(|l| l.unknown > 0)
}

/// The flags byte for ladders written with these.
fn flags(counts: bool, unknown: bool) -> u8 {
    (if counts { FLAG_COUNTS } else { 0 }) | (if unknown { FLAG_UNKNOWN } else { 0 })
}

/// A ladder's body: width, the no-aggressor totals, the side trade counts
/// unless the levels carry them, and the levels. The low tick is the
/// caller's to write, relative to whatever suits it.
fn put_ladder(out: &mut Vec<u8>, ladder: &Ladder, flags: u8) {
    let (counts, unknown) = (flags & FLAG_COUNTS != 0, flags & FLAG_UNKNOWN != 0);
    put_varint(out, ladder.levels.len() as u64);
    put_varint(out, ladder.sides.unknown);
    put_varint(out, ladder.sides.unknown_trades);
    if !counts {
        put_varint(out, ladder.sides.bid_trades);
        put_varint(out, ladder.sides.ask_trades);
    }
    for level in &ladder.levels {
        put_varint(out, level.at_bid);
        put_varint(out, level.at_ask);
        if unknown {
            put_varint(out, level.unknown);
        }
        if counts {
            put_varint(out, u64::from(level.bid_trades));
            put_varint(out, u64::from(level.ask_trades));
        }
    }
}

fn read_ladder(r: &mut Reader<'_>, low_tick: i32, flags: u8) -> Result<Ladder, DecodeError> {
    let (counts, unknown) = (flags & FLAG_COUNTS != 0, flags & FLAG_UNKNOWN != 0);
    let width = usize::try_from(r.varint()?).map_err(|_| DecodeError::Corrupt)?;
    // Every level is at least two bytes; a width past what is left cannot
    // be read, and must not get to ask for gigabytes first.
    if width > r.bytes.len().saturating_sub(r.at) / 2 + 1 {
        return Err(DecodeError::Truncated);
    }
    let mut sides = Sides { unknown: r.varint()?, unknown_trades: r.varint()?, ..Sides::default() };
    if !counts {
        sides.bid_trades = r.varint()?;
        sides.ask_trades = r.varint()?;
    }
    let mut levels = Vec::with_capacity(width);
    for _ in 0..width {
        let mut level = Level { at_bid: r.varint()?, at_ask: r.varint()?, ..Level::default() };
        if unknown {
            level.unknown = r.varint()?;
        }
        if counts {
            level.bid_trades = r.u32()?;
            level.ask_trades = r.u32()?;
            sides.bid_trades += u64::from(level.bid_trades);
            sides.ask_trades += u64::from(level.ask_trades);
        }
        sides.at_bid += level.at_bid;
        sides.at_ask += level.at_ask;
        levels.push(level);
    }
    if width > 0 && i64::from(low_tick) + width as i64 - 1 > i64::from(i32::MAX) {
        return Err(DecodeError::Corrupt);
    }
    Ok(Ladder { low_tick, levels, sides })
}

/// One minute, against the minute before it: the open and the low tick as
/// deltas, and OHLC as ticks off the low tick.
fn put_minute(out: &mut Vec<u8>, minute: &Minute, prev: (i64, i32), tick_size: f64, flags: u8) {
    let low = minute.ladder.low_tick;
    put_zigzag(out, minute.ts - prev.0);
    put_zigzag(out, i64::from(low) - i64::from(prev.1));
    for price in [minute.open, minute.high, minute.low, minute.close] {
        put_zigzag(out, i64::from(to_tick(price, tick_size)) - i64::from(low));
    }
    put_ladder(out, &minute.ladder, flags);
}

fn read_minute(r: &mut Reader<'_>, prev: (i64, i32), tick_size: f64, flags: u8) -> Result<Minute, DecodeError> {
    let ts = prev.0.checked_add(r.zigzag()?).ok_or(DecodeError::Corrupt)?;
    let low_tick = r.tick(i64::from(prev.1))?;
    let mut ohlc = [0.0; 4];
    for price in &mut ohlc {
        *price = to_price(r.tick(i64::from(low_tick))?, tick_size);
    }
    let ladder = read_ladder(r, low_tick, flags)?;
    let [open, high, low, close] = ohlc;
    Ok(Minute { ts, open, high, low, close, volume: ladder.sides.volume(), ladder })
}

/// A day's minutes as one blob.
///
/// ```text
/// u8      version (2; 1 is read, and has no bit 1)
/// u8      flags: bit 0, per-price trade counts present;
///                bit 1, per-price volume with no known side present
/// zigzag  day
/// varint  minute count
/// minute* each against the one before (the first against ts 0, tick 0):
///   zigzag  ts delta, seconds
///   zigzag  low tick delta
///   zigzag  open, high, low, close as ticks off this minute's low tick
///   varint  width (levels)
///   varint  no-aggressor volume, priced or not, then its trade count
///   varint  bid trades, ask trades           (only without per-price counts)
///   level*  varint bid, varint ask [, varint unknown]
///           [, varint bid trades, varint ask trades]
/// ```
///
/// Volume and the per-side volumes are not written: they are the levels'
/// sums plus the no-aggressor volume, which is written whole because a
/// provider can report some with no price. The low tick is against the previous
/// minute's rather than the day's first, which keeps it to a byte. OHLC is
/// written as ticks, so a price off the tick grid reads back on it.
pub fn encode_day(day: &Day, tick_size: f64) -> Vec<u8> {
    let counts = day.minutes.iter().all(|m| counts_complete(&m.ladder));
    encode_day_as(day, tick_size, counts)
}

/// [`encode_day`], choosing whether to keep per-price trade counts.
///
/// [`encode_day`] keeps them when every minute's add up to its totals, so a
/// day served with totals alone round-trips exactly and pays nothing for
/// counts it lacks. Written without, the side totals stay and the per-price
/// counts read back as zero.
pub fn encode_day_as(day: &Day, tick_size: f64, counts: bool) -> Vec<u8> {
    let flags = flags(counts, day.minutes.iter().any(|m| has_unknown(&m.ladder)));
    let mut out = Vec::with_capacity(64 + day.minutes.len() * 80);
    out.push(ENCODING_VERSION);
    out.push(flags);
    put_zigzag(&mut out, day.day);
    put_varint(&mut out, day.minutes.len() as u64);
    let mut prev = (0, 0);
    for minute in &day.minutes {
        put_minute(&mut out, minute, prev, tick_size, flags);
        prev = (minute.ts, minute.ladder.low_tick);
    }
    out
}

/// A day read back, its total summed from its minutes.
pub fn decode_day(bytes: &[u8], tick_size: f64) -> Result<Day, DecodeError> {
    let mut day = decode_minutes(bytes, tick_size)?;
    day.resum();
    Ok(day)
}

/// A day read back with the total stored beside it, which is not summed
/// again.
pub fn decode_day_with_total(bytes: &[u8], total: &[u8], tick_size: f64) -> Result<Day, DecodeError> {
    let mut day = decode_minutes(bytes, tick_size)?;
    day.total = decode_ladder(total)?;
    Ok(day)
}

/// A day's minutes read one at a time into a minute the caller keeps:
/// [`decode_day`] for a reader that walks a day once and must not allocate
/// a ladder per minute to do it. Once the caller's minute has grown to the
/// widest of the day, reading the rest allocates nothing.
pub struct DayMinutes<'a> {
    reader: Reader<'a>,
    tick_size: f64,
    flags: u8,
    day: i64,
    left: usize,
    prev: (i64, i32),
}

impl<'a> DayMinutes<'a> {
    /// The day encoded in `bytes` by [`encode_day`], ready to read.
    pub fn new(bytes: &'a [u8], tick_size: f64) -> Result<DayMinutes<'a>, DecodeError> {
        let mut reader = Reader { bytes, at: 0 };
        let flags = reader.header()?;
        let day = reader.zigzag()?;
        let left = usize::try_from(reader.varint()?).map_err(|_| DecodeError::Corrupt)?;
        if left > bytes.len() {
            return Err(DecodeError::Corrupt);
        }
        Ok(DayMinutes { reader, tick_size, flags, day, left, prev: (0, 0) })
    }

    /// The trading day, numbered as [`Day::day`].
    pub fn day(&self) -> i64 {
        self.day
    }

    /// Minutes not read yet.
    pub fn left(&self) -> usize {
        self.left
    }

    /// The next minute, into `minute`, replacing what it held. `None` once
    /// the day is read; an error if the bytes are damaged.
    pub fn read_into(&mut self, minute: &mut Minute) -> Option<Result<(), DecodeError>> {
        if self.left == 0 {
            return None;
        }
        self.left -= 1;
        Some(self.read(minute))
    }

    fn read(&mut self, minute: &mut Minute) -> Result<(), DecodeError> {
        let r = &mut self.reader;
        let ts = self.prev.0.checked_add(r.zigzag()?).ok_or(DecodeError::Corrupt)?;
        if ts <= self.prev.0 && self.prev != (0, 0) {
            return Err(DecodeError::Corrupt);
        }
        let low_tick = r.tick(i64::from(self.prev.1))?;
        let mut ohlc = [0.0; 4];
        for price in &mut ohlc {
            *price = to_price(r.tick(i64::from(low_tick))?, self.tick_size);
        }
        read_ladder_into(r, low_tick, self.flags, &mut minute.ladder)?;
        let [open, high, low, close] = ohlc;
        (minute.ts, minute.open, minute.high, minute.low, minute.close) = (ts, open, high, low, close);
        minute.volume = minute.ladder.sides.volume();
        self.prev = (ts, low_tick);
        Ok(())
    }
}

/// [`read_ladder`] into a ladder the caller keeps, its levels' storage
/// reused.
fn read_ladder_into(r: &mut Reader<'_>, low_tick: i32, flags: u8, ladder: &mut Ladder) -> Result<(), DecodeError> {
    let (counts, unknown) = (flags & FLAG_COUNTS != 0, flags & FLAG_UNKNOWN != 0);
    let width = usize::try_from(r.varint()?).map_err(|_| DecodeError::Corrupt)?;
    if width > r.bytes.len().saturating_sub(r.at) / 2 + 1 {
        return Err(DecodeError::Truncated);
    }
    let mut sides = Sides { unknown: r.varint()?, unknown_trades: r.varint()?, ..Sides::default() };
    if !counts {
        sides.bid_trades = r.varint()?;
        sides.ask_trades = r.varint()?;
    }
    ladder.levels.clear();
    for _ in 0..width {
        let mut level = Level { at_bid: r.varint()?, at_ask: r.varint()?, ..Level::default() };
        if unknown {
            level.unknown = r.varint()?;
        }
        if counts {
            level.bid_trades = r.u32()?;
            level.ask_trades = r.u32()?;
            sides.bid_trades += u64::from(level.bid_trades);
            sides.ask_trades += u64::from(level.ask_trades);
        }
        sides.at_bid += level.at_bid;
        sides.at_ask += level.at_ask;
        ladder.levels.push(level);
    }
    if width > 0 && i64::from(low_tick) + width as i64 - 1 > i64::from(i32::MAX) {
        return Err(DecodeError::Corrupt);
    }
    ladder.low_tick = low_tick;
    ladder.sides = sides;
    Ok(())
}

fn decode_minutes(bytes: &[u8], tick_size: f64) -> Result<Day, DecodeError> {
    let mut r = Reader { bytes, at: 0 };
    let flags = r.header()?;
    let day = r.zigzag()?;
    let n = usize::try_from(r.varint()?).map_err(|_| DecodeError::Corrupt)?;
    if n > bytes.len() {
        return Err(DecodeError::Corrupt);
    }
    let mut minutes: Vec<Minute> = Vec::with_capacity(n);
    let mut prev = (0, 0);
    for _ in 0..n {
        let minute = read_minute(&mut r, prev, tick_size, flags)?;
        if minutes.last().is_some_and(|last| last.ts >= minute.ts) {
            return Err(DecodeError::Corrupt);
        }
        prev = (minute.ts, minute.ladder.low_tick);
        minutes.push(minute);
    }
    Ok(Day { day, minutes, total: Ladder::new() })
}

/// One ladder on its own — a day's total. Version, flags, the low tick, then
/// the body as a minute's.
pub fn encode_ladder(ladder: &Ladder) -> Vec<u8> {
    let flags = flags(counts_complete(ladder), has_unknown(ladder));
    let mut out = Vec::with_capacity(16 + ladder.levels.len() * 6);
    out.push(ENCODING_VERSION);
    out.push(flags);
    put_zigzag(&mut out, i64::from(ladder.low_tick));
    put_ladder(&mut out, ladder, flags);
    out
}

pub fn decode_ladder(bytes: &[u8]) -> Result<Ladder, DecodeError> {
    let mut r = Reader { bytes, at: 0 };
    let flags = r.header()?;
    let low = r.tick(0)?;
    read_ladder(&mut r, low, flags)
}

/// One minute on its own, for a minute stored as it seals: version, flags,
/// then the minute against ts 0 and tick 0.
pub fn encode_minute(minute: &Minute, tick_size: f64) -> Vec<u8> {
    let flags = flags(counts_complete(&minute.ladder), has_unknown(&minute.ladder));
    let mut out = Vec::with_capacity(16 + minute.ladder.levels.len() * 6);
    out.push(ENCODING_VERSION);
    out.push(flags);
    put_minute(&mut out, minute, (0, 0), tick_size, flags);
    out
}

pub fn decode_minute(bytes: &[u8], tick_size: f64) -> Result<Minute, DecodeError> {
    let mut r = Reader { bytes, at: 0 };
    let flags = r.header()?;
    read_minute(&mut r, (0, 0), tick_size, flags)
}

#[cfg(test)]
mod day_minutes_tests {
    use super::*;

    #[test]
    fn a_day_read_a_minute_at_a_time_is_the_day_decoded() {
        let mut minutes = Vec::new();
        for i in 0..300i64 {
            let ts = 1_700_000_040 + i * 60;
            let tick = 20_000 + (i % 17) as i32 - 8;
            let mut minute = Minute::opening(ts, to_price(tick, 0.25));
            for k in 0..(1 + i % 5) as i32 {
                let side = [Side::Buy, Side::Sell, Side::Unknown][(i as usize + k as usize) % 3];
                minute.add_trade(tick + k, to_price(tick + k, 0.25), 1 + (i as u64 % 7), side, 1);
            }
            minutes.push(minute);
        }
        let day = Day { day: 19_700, minutes, total: Ladder::new() };
        for counts in [true, false] {
            let bytes = encode_day_as(&day, 0.25, counts);
            let whole = decode_day(&bytes, 0.25).unwrap();
            let mut reader = DayMinutes::new(&bytes, 0.25).unwrap();
            assert_eq!((reader.day(), reader.left()), (whole.day, whole.minutes.len()));
            let mut minute = Minute::default();
            for expected in &whole.minutes {
                reader.read_into(&mut minute).unwrap().unwrap();
                assert_eq!(&minute, expected);
            }
            assert!(reader.read_into(&mut minute).is_none());
        }
    }
}
