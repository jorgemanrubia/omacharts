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

use std::cell::{Cell, RefCell};

use chrono::NaiveDate;

use crate::bars::{bucket_of, Grid, Timeframe};
use crate::exchanges::{registry, Schedule};
use crate::symbols::{Instrument, InstrumentKind};

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
pub fn to_tick(price: f64, tick_size: f64) -> i32 {
    (price / tick_size).round() as i32
}

/// The price `tick` whole ticks of `tick_size` stand for.
pub fn to_price(tick: i32, tick_size: f64) -> f64 {
    f64::from(tick) * tick_size
}

/// The unix second a minute opens at.
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
    pub fn volume(&self) -> u64 {
        self.at_bid + self.at_ask + self.unknown
    }

    fn is_empty(&self) -> bool {
        *self == Level::default()
    }

    fn add(&mut self, other: &Level) {
        self.at_bid += other.at_bid;
        self.at_ask += other.at_ask;
        self.unknown += other.unknown;
        self.bid_trades += other.bid_trades;
        self.ask_trades += other.ask_trades;
    }

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
    pub fn volume(&self) -> u64 {
        self.at_bid + self.at_ask + self.unknown
    }

    /// Buyers less sellers: ask volume less bid volume.
    pub fn delta(&self) -> i64 {
        self.at_ask as i64 - self.at_bid as i64
    }

    pub fn trades(&self) -> u64 {
        self.bid_trades + self.ask_trades + self.unknown_trades
    }

    pub fn add(&mut self, other: &Sides) {
        self.at_bid += other.at_bid;
        self.at_ask += other.at_ask;
        self.unknown += other.unknown;
        self.bid_trades += other.bid_trades;
        self.ask_trades += other.ask_trades;
        self.unknown_trades += other.unknown_trades;
    }

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

    pub fn is_empty(&self) -> bool {
        self.levels.is_empty()
    }

    /// The highest tick with a level, traded or not.
    pub fn high_tick(&self) -> Option<i32> {
        (!self.levels.is_empty()).then(|| self.low_tick + self.levels.len() as i32 - 1)
    }

    /// The level at `tick`, zero outside the range.
    pub fn at(&self, tick: i32) -> Level {
        let i = i64::from(tick) - i64::from(self.low_tick);
        match usize::try_from(i) {
            Ok(i) => self.levels.get(i).copied().unwrap_or_default(),
            Err(_) => Level::default(),
        }
    }

    /// Everything that traded, sides or not.
    pub fn total(&self) -> u64 {
        self.sides.volume()
    }

    /// Ask volume less bid volume.
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
    pub fn add(&mut self, bar: &LadderBar) {
        self.add_ladder(&bar.ladder);
    }

    /// Add another ladder in, growing the range as needed.
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
    pub fn opening(ts: i64, price: f64) -> LadderBar {
        LadderBar { ts, open: price, high: price, low: price, close: price, volume: 0, ladder: Ladder::with_capacity(32) }
    }

    /// One print: OHLC, volume and the ladder together. Every fold of
    /// trades — the forming minute, tick, volume and range bars — goes
    /// through here.
    pub fn add_trade(&mut self, tick: i32, price: f64, size: u64, side: Side, count: u32) {
        self.high = self.high.max(price);
        self.low = self.low.min(price);
        self.close = price;
        self.volume += size;
        self.ladder.add_trade(tick, size, side, count);
    }

    /// Fold a later bar into this one: the high, low, close, volume and ladder.
    pub fn merge(&mut self, later: &LadderBar) {
        self.high = self.high.max(later.high);
        self.low = self.low.min(later.low);
        self.close = later.close;
        self.volume += later.volume;
        self.ladder.add_ladder(&later.ladder);
    }

    pub fn sides(&self) -> &Sides {
        &self.ladder.sides
    }
}

/// Where trading days begin, the way the chart's bars already cut them.
///
/// The exchange's own calendar when its hours are known — a CME equity-index
/// day is named for the date it ends on and starts at 17:00 Chicago the
/// evening before — and otherwise midnight UTC moved by the instrument's
/// `session_origin`, as [`Grid`] does. FX and crypto are cut at the origin,
/// as there.
///
/// A day is numbered as its **trading date in days since 1970-01-01**: a
/// date rather than the instant the session starts, so a day keeps its name
/// across daylight saving and reads back as a date in the store.
#[derive(Clone, Debug)]
pub struct Days {
    schedule: Option<Schedule<'static>>,
    origin: i64,
    /// The last day asked about: where it starts and ends, and its number.
    last: Cell<Option<(i64, i64, i64)>>,
}

const SECONDS_PER_DAY: i64 = 86_400;

fn epoch() -> NaiveDate {
    NaiveDate::from_ymd_opt(1970, 1, 1).unwrap_or_default()
}

impl Days {
    pub fn new(instrument: &Instrument) -> Days {
        let schedule = match instrument.kind {
            InstrumentKind::Fx | InstrumentKind::Crypto => None,
            _ => registry().for_instrument(instrument),
        };
        Days { schedule, origin: instrument.session_origin, last: Cell::new(None) }
    }

    /// Days cut every 24 hours at `origin` seconds past midnight UTC.
    pub fn fixed(origin: i64) -> Days {
        Days { schedule: None, origin, last: Cell::new(None) }
    }

    /// The trading day `ts` belongs to.
    pub fn of(&self, ts: i64) -> i64 {
        if let Some((start, end, day)) = self.last.get()
            && (start..end).contains(&ts)
        {
            return day;
        }
        let found = match self.schedule.and_then(|s| s.trading_date(ts).map(|date| (s, date))) {
            Some((schedule, date)) => {
                let end = date.succ_opt().map_or(i64::MAX, |next| schedule.day_start(next));
                (schedule.day_start(date), end, (date - epoch()).num_days())
            }
            None => {
                let start = (ts - self.origin).div_euclid(SECONDS_PER_DAY) * SECONDS_PER_DAY + self.origin;
                // Named, like an exchange's, for the date its middle falls on:
                // a session opening at 22:00 UTC is the next day's.
                let day = (start + SECONDS_PER_DAY / 2).div_euclid(SECONDS_PER_DAY);
                (start, start + SECONDS_PER_DAY, day)
            }
        };
        self.last.set(Some(found));
        found.2
    }

    /// Where trading day `day` begins, unix seconds.
    pub fn start(&self, day: i64) -> i64 {
        match self.schedule {
            Some(schedule) => schedule.day_start(epoch() + chrono::Duration::days(day)),
            None => {
                // The one start of the day whose middle falls on `day`.
                let noon = day * SECONDS_PER_DAY + SECONDS_PER_DAY / 2;
                (noon - 1 - self.origin).div_euclid(SECONDS_PER_DAY) * SECONDS_PER_DAY + self.origin
            }
        }
    }
}

/// One trading day: its minutes in order, and their sum.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct Day {
    /// The trading date, days since 1970-01-01; see [`Days`].
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
    pub fn insert(&mut self, days: &Days, minute: Minute) {
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

    /// The minutes opening in `from..to` folded into bars of `timeframe`
    /// along `grid`, each with its ladder: footprints. One allocation per
    /// bar, none per minute.
    pub fn bars(&self, from: i64, to: i64, timeframe: Timeframe, grid: &Grid) -> Vec<LadderBar> {
        self.bars_by(from, to, |ts| bucket_of(ts, timeframe, grid))
    }

    /// [`Self::bars`] with any rule for where a bar opens.
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

    pub fn is_connected(&self) -> bool {
        self.since.is_some()
    }

    /// The minute forming now, if anything has traded in it.
    pub fn current(&self) -> Option<&Minute> {
        self.minute.as_ref()
    }

    /// The span of minute opens, `from..to`, known complete: every print
    /// in it was seen, and a minute missing from it traded nothing. Runs up
    /// to the forming minute, which is not complete yet.
    pub fn covered(&self) -> Option<(i64, i64)> {
        let since = self.since?;
        let to = self.minute.as_ref().map(|m| m.ts)?;
        (to > since).then_some((since, to))
    }

    /// A print at `ts` (unix seconds), `price`, `size` contracts.
    pub fn add_trade(&mut self, ts: i64, price: f64, size: u64, side: Side) -> Option<Minute> {
        self.push(ts * NANOS, to_tick(price, self.tick_size), size, side)
    }

    /// A print at `ts_nanos`, `tick`. Returns the minute it sealed, if it
    /// opened a new one.
    ///
    /// A print for a minute earlier than the forming one — a late report —
    /// is counted in the forming minute rather than lost.
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

/// What a chart holds of a contract's ladders: the total of every day its
/// bars reach, the minutes of only the days a period cuts, and the minute
/// forming now.
///
/// A year of minutes decoded is some 280 MB, and a session profile is
/// answered by day totals alone, so minutes are loaded a day at a time as a
/// span needs them — see [`Held::cut`] — and a span this cannot answer
/// exactly is answered with `None`, never with part of it.
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
    /// Days a span was asked of and could not be answered for want of their
    /// minutes, for the caller to load: see [`Held::take_wanted`].
    pub wanted: RefCell<Vec<i64>>,
}

impl Held {
    pub fn new(tick_size: f64) -> Held {
        Held { tick_size, ..Held::default() }
    }

    /// Everything in `series`, minutes and all, trusted from its first
    /// minute on; whole days from the one after it.
    pub fn of(series: &Series) -> Held {
        let days: Vec<HeldDay> = series.days.iter().cloned().filter_map(HeldDay::whole).collect();
        let floor = days.first().map_or(i64::MAX, |d| d.first_ts);
        let floor_day = days.first().map_or(i64::MAX, |d| d.day.day + 1);
        Held { tick_size: series.tick_size, days, forming: None, floor, floor_day, wanted: RefCell::default() }
    }

    pub fn is_empty(&self) -> bool {
        self.days.is_empty() && self.forming.is_none()
    }

    /// The days [`Self::between`] was asked about and could not answer for
    /// want of their minutes, newest first, taken.
    pub fn take_wanted(&self) -> Vec<i64> {
        let mut wanted = std::mem::take(&mut *self.wanted.borrow_mut());
        wanted.sort_unstable_by(|a, b| b.cmp(a));
        wanted
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

    /// Put a day's minutes in, in place of its total alone.
    pub fn load(&mut self, day: Day) {
        let Some(held) = HeldDay::whole(day) else { return };
        match self.days.binary_search_by_key(&held.day.day, |d| d.day.day) {
            Ok(at) => self.days[at] = held,
            Err(at) => self.days.insert(at, held),
        }
    }

    /// What traded at each price in the minutes opening in `from..to`, the
    /// forming one included: whole days by their totals, the loose minutes
    /// at either end one by one. `None` when that cannot be told exactly:
    /// the span starts before [`Self::floor`], or cuts a day whose minutes
    /// are not loaded.
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
                let mut wanted = self.wanted.borrow_mut();
                if !wanted.contains(&held.day.day) {
                    wanted.push(held.day.day);
                }
                short = true;
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

    /// The trading days numbered `from_day..to_day` by their totals, and the
    /// forming minute if it is in one of them: for bars a day or longer.
    /// `None` when the first of them is before [`Self::floor_day`].
    pub fn days_between(&self, from_day: i64, to_day: i64, days: &Days) -> Option<Ladder> {
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
            held.day.total.add(&minute);
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
const FLAG_COUNTS: u8 = 1;

/// Flag: each level carries its volume with no known side. Never set in
/// version 1.
const FLAG_UNKNOWN: u8 = 2;

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
mod tests {
    use chrono::{TimeZone, Utc};

    use super::*;

    /// A small deterministic generator, so a failing case can be run again.
    struct Rng(u64);

    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }

        fn below(&mut self, n: u64) -> u64 {
            self.next() % n.max(1)
        }
    }

    const TICK: f64 = 0.25;

    /// A minute of random prints around `center`.
    fn minute(rng: &mut Rng, ts: i64, center: i32) -> Minute {
        let mut m = Minute::opening(ts, to_price(center, TICK));
        for _ in 0..1 + rng.below(40) {
            let tick = center + rng.below(21) as i32 - 10;
            let side = match rng.below(10) {
                0 => Side::Unknown,
                1..=4 => Side::Sell,
                _ => Side::Buy,
            };
            m.add_trade(tick, to_price(tick, TICK), 1 + rng.below(20), side, 1);
        }
        m
    }

    /// `n` days of minutes, with holes, cut by `days`.
    fn series(rng: &mut Rng, days: &Days, start: i64, n: i64) -> Series {
        let mut series = Series::new(TICK);
        let mut center = 30_000;
        for i in 0..n * 1440 {
            if rng.below(5) == 0 {
                continue;
            }
            center += rng.below(5) as i32 - 2;
            series.insert(days, minute(rng, start + i * 60, center));
        }
        series
    }

    fn trimmed(ladder: &Ladder) -> Ladder {
        let mut ladder = ladder.clone();
        ladder.trim();
        ladder
    }

    fn brute(series: &Series, from: i64, to: i64) -> Ladder {
        let mut sum = Ladder::new();
        for m in series.minutes().filter(|m| (from..to).contains(&m.ts)) {
            sum.add(m);
        }
        sum
    }

    fn at(y: i32, mo: u32, d: u32, h: u32, mi: u32) -> i64 {
        Utc.with_ymd_and_hms(y, mo, d, h, mi, 0).single().unwrap().timestamp()
    }

    fn day_number(y: i32, mo: u32, d: u32) -> i64 {
        (NaiveDate::from_ymd_opt(y, mo, d).unwrap() - epoch()).num_days()
    }

    fn es() -> Instrument {
        Instrument {
            symbol: "ES".into(),
            name: "E-mini S&P 500".into(),
            kind: InstrumentKind::FutureRoot,
            suffix: None,
            currency: Some("USD".into()),
            tier: 0,
            session_origin: 79_200,
            overrides: Vec::new(),
            exchange: None,
            popularity: 0,
            local_name: None,
        }
    }

    #[test]
    fn prices_round_to_the_nearest_tick() {
        assert_eq!(to_tick(7625.25, 0.25), 30501);
        assert_eq!(to_tick(7625.2499999, 0.25), 30501);
        assert_eq!(to_tick(7625.2500001, 0.25), 30501);
        assert_eq!(to_tick(1.3, 0.1), 13, "1.3 / 0.1 is 12.999…");
        assert_eq!(to_tick(-0.25, 0.25), -1);
        assert_eq!(to_price(30501, 0.25), 7625.25);
    }

    #[test]
    fn a_cme_day_starts_the_evening_before_and_is_named_for_the_next() {
        let days = Days::new(&es());
        // 2024-03-12 is in US daylight time: 17:00 in Chicago is 22:00 UTC.
        assert_eq!(days.of(at(2024, 3, 12, 22, 0)), day_number(2024, 3, 13));
        assert_eq!(days.of(at(2024, 3, 12, 21, 59)), day_number(2024, 3, 12));
        assert_eq!(days.of(at(2024, 3, 13, 14, 30)), day_number(2024, 3, 13));
        assert_eq!(days.start(day_number(2024, 3, 13)), at(2024, 3, 12, 22, 0));
        // And in winter, an hour later in UTC.
        assert_eq!(days.start(day_number(2024, 1, 10)), at(2024, 1, 9, 23, 0));
    }

    #[test]
    fn a_fixed_origin_names_days_the_same_way() {
        let days = Days::fixed(79_200);
        assert_eq!(days.of(at(2024, 3, 12, 22, 0)), day_number(2024, 3, 13));
        assert_eq!(days.of(at(2024, 3, 12, 21, 59)), day_number(2024, 3, 12));
        assert_eq!(days.start(day_number(2024, 3, 13)), at(2024, 3, 12, 22, 0));
        let midnight = Days::fixed(0);
        assert_eq!(midnight.of(at(2024, 3, 12, 23, 59)), day_number(2024, 3, 12));
        assert_eq!(midnight.start(day_number(2024, 3, 12)), at(2024, 3, 12, 0, 0));
    }

    #[test]
    fn prints_land_on_their_side_and_unknown_ones_at_their_price_on_neither() {
        let mut m = Minute::opening(0, 100.0);
        m.add_trade(400, 100.0, 5, Side::Buy, 1);
        m.add_trade(398, 99.5, 3, Side::Sell, 1);
        m.add_trade(402, 100.5, 2, Side::Unknown, 1);
        assert_eq!(m.volume, 10);
        assert_eq!((m.high, m.low, m.close), (100.5, 99.5, 100.5));
        assert_eq!(m.ladder.low_tick, 398);
        assert_eq!(m.ladder.levels.len(), 5, "the unknown print takes its price");
        assert_eq!(m.ladder.at(400).at_ask, 5);
        assert_eq!(m.ladder.at(398).at_bid, 3);
        let unknown = m.ladder.at(402);
        assert_eq!((unknown.at_bid, unknown.at_ask, unknown.unknown, unknown.volume()), (0, 0, 2, 2));
        let s = m.ladder.sides;
        assert_eq!((s.at_bid, s.at_ask, s.unknown, s.volume(), s.delta()), (3, 5, 2, 10, 2));
        assert_eq!((s.bid_trades, s.ask_trades, s.unknown_trades), (1, 1, 1));
        assert_eq!(m.ladder.point_of_control(), Some(400));
        let prices: Vec<_> = m.ladder.prices(0.25).collect();
        assert_eq!(prices, vec![(99.5, 3, 0), (99.75, 0, 0), (100.0, 0, 5), (100.25, 0, 0), (100.5, 0, 0)]);
    }

    /// A feed that never says who took liquidity still has a volume at
    /// every price it traded, and no delta at all.
    #[test]
    fn trades_with_no_aggressor_have_volume_at_price_and_no_delta() {
        let days = Days::fixed(0);
        let mut s = Series::new(TICK);
        for i in 0..10 {
            let mut m = Minute::opening(i * 60, to_price(400, TICK));
            m.add_trade(400, to_price(400, TICK), 4, Side::Unknown, 1);
            m.add_trade(401 + i as i32 % 2, to_price(401, TICK), 1, Side::Unknown, 1);
            s.insert(&days, m);
        }
        let ladder = s.volume_at_price(0, 600);
        assert_eq!((ladder.low_tick, ladder.levels.len()), (400, 3));
        let volumes: Vec<u64> = ladder.levels.iter().map(Level::volume).collect();
        assert_eq!(volumes, vec![40, 5, 5]);
        assert_eq!(ladder.point_of_control(), Some(400));
        assert_eq!((ladder.total(), ladder.delta()), (50, 0));
        assert_eq!((ladder.sides.at_bid, ladder.sides.at_ask, ladder.sides.unknown), (0, 0, 50));
        for bar in s.bars_by(0, 600, |ts| ts.div_euclid(300) * 300) {
            assert_eq!((bar.volume, bar.ladder.delta()), (25, 0));
            assert_eq!(bar.ladder.levels.iter().map(Level::volume).sum::<u64>(), 25);
        }
        // Taken back out, a day's total drops the prices it leaves empty.
        let mut day = s.days[0].clone();
        day.remove(0, 600);
        assert!(day.total.levels.is_empty());
    }

    #[test]
    fn inserting_and_replacing_keeps_the_day_total_right() {
        let mut rng = Rng(7);
        let mut day = Day::new(1);
        for i in 0..200 {
            day.insert(minute(&mut rng, i * 60, 1000 + (i as i32 % 30)));
        }
        // Replace every third, insert some out of order, and take a stretch out.
        for i in (0..200).step_by(3) {
            day.insert(minute(&mut rng, i * 60, 900));
        }
        day.insert(minute(&mut rng, 30, 1200));
        day.remove(50 * 60, 60 * 60);
        assert!(day.minutes.windows(2).all(|w| w[0].ts < w[1].ts));
        let mut summed = day.clone();
        summed.resum();
        assert_eq!(trimmed(&day.total), trimmed(&summed.total));
        assert_eq!(day.total.sides, summed.total.sides);
    }

    #[test]
    fn a_range_is_the_brute_force_sum_of_its_minutes() {
        let mut rng = Rng(42);
        let days = Days::new(&es());
        let start = at(2024, 3, 10, 20, 0);
        let series = series(&mut rng, &days, start, 6);
        assert!(series.days.len() >= 6);
        let end = start + 6 * 86_400;
        let mut cases = vec![(start - 999, end + 999), (start, start), (end, start)];
        for _ in 0..200 {
            let a = start + rng.below((end - start) as u64) as i64;
            let b = a + rng.below(3 * 86_400) as i64;
            cases.push((a, b));
        }
        for (from, to) in cases {
            let fast = series.volume_at_price(from, to);
            let slow = brute(&series, from, to);
            assert_eq!(trimmed(&fast), trimmed(&slow), "{from}..{to}");
            assert_eq!(fast.sides, slow.sides);
            assert_eq!(series.side_totals(from, to), slow.sides);
        }
    }

    #[test]
    fn replacing_minutes_across_a_series_keeps_ranges_exact() {
        let mut rng = Rng(9);
        let days = Days::fixed(0);
        let mut s = series(&mut rng, &days, 0, 3);
        for _ in 0..500 {
            let ts = rng.below(3 * 1440) as i64 * 60;
            s.insert(&days, minute(&mut rng, ts, 29_000));
        }
        s.remove(86_000, 90_000);
        for (from, to) in [(0, 3 * 86_400), (40_000, 200_000)] {
            assert_eq!(trimmed(&s.volume_at_price(from, to)), trimmed(&brute(&s, from, to)));
        }
    }

    #[test]
    fn footprints_are_the_sum_of_their_minutes() {
        let mut rng = Rng(3);
        let days = Days::fixed(0);
        let s = series(&mut rng, &days, 0, 2);
        let bars = s.bars_by(3_600, 100_000, |ts| ts.div_euclid(300) * 300);
        assert!(!bars.is_empty());
        for bar in &bars {
            let (from, to) = (bar.ts.max(3_600), (bar.ts + 300).min(100_000));
            let slow = brute(&s, from, to);
            assert_eq!(trimmed(&bar.ladder), trimmed(&slow));
            assert_eq!(bar.volume, slow.sides.volume());
            let mins: Vec<_> = s.minutes().filter(|m| (from..to).contains(&m.ts)).collect();
            assert_eq!(bar.open, mins[0].open);
            assert_eq!(bar.close, mins[mins.len() - 1].close);
        }
    }

    #[test]
    fn the_forming_minute_seals_when_the_next_one_trades() {
        const N: i64 = 1_000_000_000;
        let mut f = Forming::new(TICK);
        f.connect(10 * N); // part way through minute 0
        assert!(f.add_trade(15, 100.0, 2, Side::Buy).is_none());
        assert!(f.add_trade(20, 99.75, 1, Side::Sell).is_none());
        assert!(f.add_trade(59, 100.25, 3, Side::Buy).is_none());
        assert_eq!(f.covered(), None, "minute 0 was joined late");
        let sealed = f.add_trade(61, 100.0, 4, Side::Unknown).unwrap();
        assert_eq!((sealed.ts, sealed.open, sealed.high, sealed.low, sealed.close), (0, 100.0, 100.25, 99.75, 100.25));
        assert_eq!(sealed.volume, 6);
        assert_eq!(sealed.ladder.sides.at_ask, 5);
        assert_eq!(f.current().map(|m| m.ts), Some(60));
        // Quiet minutes 2 and 3 are complete; the print in minute 4 seals 1.
        assert!(f.add_trade(250, 100.0, 1, Side::Buy).is_some());
        assert_eq!(f.covered(), Some((60, 240)));
        assert_eq!(f.seal_by(299), None);
        assert_eq!(f.seal_by(300).map(|m| m.ts), Some(240));
        // A disconnect hands back what was forming and covers nothing more.
        f.add_trade(301, 100.0, 1, Side::Buy);
        assert_eq!(f.disconnect().map(|m| m.ts), Some(300));
        assert!(!f.is_connected());
        assert_eq!(f.covered(), None);
    }

    #[test]
    fn a_day_round_trips_through_its_encoding() {
        let mut rng = Rng(11);
        let days = Days::fixed(0);
        let s = series(&mut rng, &days, 0, 1);
        let day = &s.days[0];
        let bytes = encode_day(day, TICK);
        assert_eq!(bytes[0], ENCODING_VERSION);
        assert_eq!(&decode_day(&bytes, TICK).unwrap(), day);
        let total = encode_ladder(&day.total);
        assert_eq!(decode_day_with_total(&bytes, &total, TICK).unwrap(), *day);
        let m = &day.minutes[3];
        assert_eq!(&decode_minute(&encode_minute(m, TICK), TICK).unwrap(), m);
    }

    /// Volume with no side at a price round-trips, and costs a day with
    /// none of it nothing: its blob is laid out as version 1's was.
    #[test]
    fn volume_with_no_side_round_trips_and_is_written_only_when_there_is_some() {
        let mut m = Minute::opening(60, 100.0);
        m.add_trade(400, 100.0, 3, Side::Buy, 1);
        m.add_trade(401, 100.25, 2, Side::Unknown, 1);
        m.ladder.sides.unknown += 4; // reported with no price
        m.volume += 4;
        let day = Day { day: 0, total: m.ladder.clone(), minutes: vec![m.clone()] };
        let bytes = encode_day(&day, TICK);
        assert_eq!(bytes[1] & FLAG_UNKNOWN, FLAG_UNKNOWN);
        assert_eq!(decode_day(&bytes, TICK).unwrap(), day);
        assert_eq!(decode_ladder(&encode_ladder(&day.total)).unwrap(), day.total);
        assert_eq!(decode_minute(&encode_minute(&m, TICK), TICK).unwrap(), m);

        let mut sided = Minute::opening(60, 100.0);
        sided.add_trade(400, 100.0, 3, Side::Buy, 1);
        assert_eq!(encode_minute(&sided, TICK)[1] & FLAG_UNKNOWN, 0);
    }

    /// Ladders stored before a price kept its volume with no side are read
    /// as they were written: that volume in the totals, at no price.
    #[test]
    fn version_1_blobs_still_decode() {
        // One minute at 60 s: low tick 400, OHLC all on it, one level of
        // 3 bid and 5 ask with a trade count each side, and 2 contracts
        // in one print with no side.
        let v1 = [1, FLAG_COUNTS, 120, 0xa0, 0x06, 0, 0, 0, 0, 1, 2, 1, 3, 5, 1, 2];
        let m = decode_minute(&v1, TICK).unwrap();
        assert_eq!((m.ts, m.open, m.close, m.volume), (60, 100.0, 100.0, 10));
        assert_eq!(m.ladder.levels, vec![Level { at_bid: 3, at_ask: 5, unknown: 0, bid_trades: 1, ask_trades: 2 }]);
        let s = m.ladder.sides;
        assert_eq!((s.at_bid, s.at_ask, s.unknown, s.unknown_trades, s.delta()), (3, 5, 2, 1, 2));

        // A whole day and a total in version 1 are version 2's bytes with
        // no per-price unknown, under the old version byte.
        let mut rng = Rng(5);
        let mut day = series(&mut rng, &Days::fixed(0), 0, 1).days.remove(0);
        for minute in &mut day.minutes {
            for level in &mut minute.ladder.levels {
                level.unknown = 0;
            }
        }
        day.resum();
        let mut bytes = encode_day(&day, TICK);
        let mut total = encode_ladder(&day.total);
        assert_eq!((bytes[1] & FLAG_UNKNOWN, total[1] & FLAG_UNKNOWN), (0, 0));
        bytes[0] = 1;
        total[0] = 1;
        assert_eq!(decode_day_with_total(&bytes, &total, TICK).unwrap(), day);
        assert_eq!(decode_day(&[9, 0], TICK), Err(DecodeError::Version(9)));
    }

    #[test]
    fn side_trade_counts_survive_without_per_price_counts() {
        let mut m = Minute::opening(60, 10.0);
        m.add_trade(40, 10.0, 7, Side::Buy, 0);
        m.ladder.sides.ask_trades = 3; // a provider that sent totals alone
        let day = Day { day: 0, total: m.ladder.clone(), minutes: vec![m] };
        let back = decode_day(&encode_day(&day, TICK), TICK).unwrap();
        assert_eq!(back, day);
        assert_eq!(back.minutes[0].ladder.sides.ask_trades, 3);

        // And dropped on request, the side totals still stand.
        let mut rng = Rng(5);
        let s = series(&mut rng, &Days::fixed(0), 0, 1);
        let with = encode_day(&s.days[0], TICK);
        let without = encode_day_as(&s.days[0], TICK, false);
        assert!(without.len() < with.len());
        let back = decode_day(&without, TICK).unwrap();
        assert_eq!(back.total.sides, s.days[0].total.sides);
    }

    #[test]
    fn a_damaged_blob_is_refused_not_misread() {
        let mut rng = Rng(13);
        let s = series(&mut rng, &Days::fixed(0), 0, 1);
        let bytes = encode_day(&s.days[0], TICK);
        assert_eq!(decode_day(&bytes[..bytes.len() / 2], TICK), Err(DecodeError::Truncated));
        let mut future = bytes.clone();
        future[0] = 99;
        assert_eq!(decode_day(&future, TICK), Err(DecodeError::Version(99)));
        assert!(decode_day(&[], TICK).is_err());
        assert!(decode_ladder(&[1, 0, 0, 0xff, 0xff, 0xff, 0xff, 0x0f]).is_err());
    }

    /// The days of `series` by their totals alone, as a chart first holds
    /// them, trusted from its first minute.
    fn by_totals(series: &Series) -> Held {
        let days = series
            .days
            .iter()
            .map(|d| HeldDay::total(d.day, d.first_ts().unwrap(), d.last_ts().unwrap(), d.total.clone()))
            .collect::<Vec<_>>();
        let floor = days.first().map_or(i64::MAX, |d| d.first_ts);
        Held { tick_size: series.tick_size, days, forming: None, floor, floor_day: i64::MIN, wanted: RefCell::default() }
    }

    /// A chart holds day totals and loads a day's minutes only when a span
    /// cuts it; what it answers is what summing every minute answers, and a
    /// span it cannot answer whole it does not answer at all.
    #[test]
    fn a_chart_answers_from_day_totals_and_loads_only_the_days_it_cuts() {
        let instrument = es();
        let days = Days::new(&instrument);
        let mut rng = Rng(0x5eed_1ad);
        let start = at(2026, 10, 12, 0, 0);
        let series = series(&mut rng, &days, start, 5);
        let mut held = by_totals(&series);
        let floor = held.floor;

        // Whole days: their totals, no minutes needed.
        let (whole_from, whole_to) = (days.start(series.days[1].day), days.start(series.days[3].day));
        assert!(held.cut(whole_from, whole_to).is_empty());
        assert_eq!(trimmed(&held.between(whole_from, whole_to).unwrap()), trimmed(&brute(&series, whole_from, whole_to)));

        // A span that cuts two days asks for exactly those two, and is
        // answered once they are in.
        let (from, to) = (whole_from + 3 * 3_600 + 60 * 7, whole_to + 5 * 3_600);
        assert_eq!(held.between(from, to), None);
        let cut = held.cut(from, to);
        assert_eq!(cut, vec![series.days[1].day, series.days[3].day]);
        for number in cut {
            held.load(series.days.iter().find(|d| d.day == number).unwrap().clone());
        }
        assert_eq!(trimmed(&held.between(from, to).unwrap()), trimmed(&brute(&series, from, to)));
        assert_eq!(held.days.iter().filter(|d| d.whole).count(), 2, "only the days it cut");

        // Before the floor nothing is known whole.
        assert_eq!(held.between(floor - 60, floor + 600), None);

        // Live: a sealed minute joins its day, and the forming one counts
        // until it is sealed, once.
        let last = series.days.last().unwrap();
        let next = last.last_ts().unwrap() + 60;
        let mut forming = Minute::opening(next, 7_500.0);
        forming.add_trade(30_000, 7_500.0, 5, Side::Buy, 1);
        held.forming = Some(forming.clone());
        let everything = |held: &Held| held.between(floor, next + 60).unwrap().sides.volume();
        let before = brute(&series, floor, next).sides.volume();
        assert_eq!(everything(&held), before + 5);
        held.seal(days.of(next), forming);
        assert_eq!(held.forming, None);
        assert_eq!(everything(&held), before + 5);
    }

    /// Spans counted in days, for daily bars: the totals of the days named,
    /// and nothing before the first day held from its start.
    #[test]
    fn whole_days_are_their_totals() {
        let instrument = es();
        let days = Days::new(&instrument);
        let mut rng = Rng(77);
        let series = series(&mut rng, &days, at(2026, 10, 12, 0, 0), 4);
        let mut held = by_totals(&series);
        let (a, b) = (series.days[1].day, series.days[3].day);
        let expected = brute(&series, days.start(a), days.start(b));
        assert_eq!(trimmed(&held.days_between(a, b, &days).unwrap()), trimmed(&expected));
        held.floor_day = a + 1;
        assert_eq!(held.days_between(a, b, &days), None);
    }
}
