//! Trades, in memory only: the current session's prints for one contract.
//!
//! Nothing here is stored. A tick, volume or range chart is drawn from this
//! buffer while the app runs, and the minute ladders in [`crate::ladders`]
//! are what is kept. Every bar folded out of trades — the forming minute
//! included — is a [`LadderBar`] built through [`LadderBar::add_trade`], so
//! there is one way a print becomes OHLC, volume and a ladder.

use std::collections::VecDeque;

use crate::ladders::{to_price, Forming, LadderBar, Minute, Side};

/// One print.
///
/// 24 bytes: `ts_nanos` (8), `size` (8), `price_tick` (4), `side` (1) and
/// three of padding. The compiler orders the fields that way whatever order
/// they are written in; a test holds it to the size.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Trade {
    /// Unix nanoseconds.
    pub ts_nanos: i64,
    pub price_tick: i32,
    pub size: u64,
    pub side: Side,
}

const NANOS: i64 = 1_000_000_000;

impl Trade {
    /// The unix second of the minute this print falls in.
    pub fn minute(&self) -> i64 {
        self.ts_nanos.div_euclid(60 * NANOS) * 60
    }
}

/// How many trades [`Ticks`] keeps unless told otherwise: 120 MB, a busy
/// day of ES and then some.
pub const DEFAULT_CAPACITY: usize = 5_000_000;

/// A contract's trades, oldest first, up to a bound.
///
/// Append-only in time order. When full, the oldest whole minute goes, so
/// what is kept always starts at a minute boundary and a fold of it never
/// begins part way through one.
#[derive(Clone, Debug)]
pub struct Ticks {
    trades: VecDeque<Trade>,
    capacity: usize,
}

impl Default for Ticks {
    fn default() -> Ticks {
        Ticks::new(DEFAULT_CAPACITY)
    }
}

impl Ticks {
    pub fn new(capacity: usize) -> Ticks {
        let capacity = capacity.max(1);
        Ticks { trades: VecDeque::with_capacity(capacity.min(1 << 16)), capacity }
    }

    pub fn len(&self) -> usize {
        self.trades.len()
    }

    pub fn is_empty(&self) -> bool {
        self.trades.is_empty()
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    pub fn first(&self) -> Option<&Trade> {
        self.trades.front()
    }

    pub fn last(&self) -> Option<&Trade> {
        self.trades.back()
    }

    /// Add a print. One older than the last is a caller's mistake; it is
    /// kept, at the end, rather than lost.
    pub fn push(&mut self, trade: Trade) {
        if self.trades.len() >= self.capacity {
            self.drop_oldest_minute();
        }
        self.trades.push_back(trade);
    }

    fn drop_oldest_minute(&mut self) {
        let Some(minute) = self.trades.front().map(Trade::minute) else { return };
        let end = self.trades.partition_point(|t| t.minute() <= minute);
        self.trades.drain(..end.max(1));
    }

    /// Everything from `ts_nanos` on.
    pub fn since(&self, ts_nanos: i64) -> Span<'_> {
        self.range(ts_nanos, i64::MAX)
    }

    /// Everything in `from..to`, nanoseconds.
    pub fn range(&self, from: i64, to: i64) -> Span<'_> {
        let a = self.trades.partition_point(|t| t.ts_nanos < from);
        let b = self.trades.partition_point(|t| t.ts_nanos < to).max(a);
        let (head, tail) = self.trades.as_slices();
        let cut = |s: &'_ [Trade], lo: usize, hi: usize| -> (usize, usize) { (lo.min(s.len()), hi.min(s.len())) };
        let (h0, h1) = cut(head, a, b);
        let (t0, t1) = cut(tail, a.saturating_sub(head.len()), b.saturating_sub(head.len()));
        Span { head: &head[h0..h1], tail: &tail[t0..t1] }
    }

    pub fn all(&self) -> Span<'_> {
        let (head, tail) = self.trades.as_slices();
        Span { head, tail }
    }

    pub fn clear(&mut self) {
        self.trades.clear();
    }
}

/// A stretch of [`Ticks`], borrowed: at most two slices, because the buffer
/// is a ring.
#[derive(Clone, Copy, Debug)]
pub struct Span<'a> {
    pub head: &'a [Trade],
    pub tail: &'a [Trade],
}

impl<'a> Span<'a> {
    pub fn len(&self) -> usize {
        self.head.len() + self.tail.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn iter(&self) -> impl Iterator<Item = &'a Trade> + 'a {
        self.head.iter().chain(self.tail.iter())
    }
}

/// When a bar folded from trades closes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Rule {
    /// Every `n` trades.
    Trades(u32),
    /// Every `n` contracts. A print that runs past the end of a bar is split,
    /// the rest of it opening the next; it counts as one trade, in the bar
    /// it started in.
    Volume(u64),
    /// When the range would pass `n` ticks: the print that would take it
    /// there opens the next bar instead.
    Range(u32),
}

/// A bar folded from trades: where it opened and closed, in nanoseconds,
/// and its bar and ladder — `bar.ts` is the second it opened in.
#[derive(Clone, PartialEq, Debug)]
pub struct TradeBar {
    pub start_nanos: i64,
    pub end_nanos: i64,
    /// Prints in it, a split print counted where it started.
    pub trades: u64,
    pub bar: LadderBar,
}

/// Bars of a [`Rule`], built a print at a time.
#[derive(Clone, Debug)]
pub struct Fold {
    rule: Rule,
    tick_size: f64,
    forming: Option<TradeBar>,
    /// Ticks of the forming bar's range, for range bars.
    low: i32,
    high: i32,
}

impl Fold {
    pub fn new(rule: Rule, tick_size: f64) -> Fold {
        Fold { rule, tick_size, forming: None, low: 0, high: 0 }
    }

    pub fn forming(&self) -> Option<&TradeBar> {
        self.forming.as_ref()
    }

    /// The bar still open, taken out.
    pub fn finish(&mut self) -> Option<TradeBar> {
        self.forming.take()
    }

    /// Add a print; `sealed` is handed each bar it closes, in order. Most
    /// prints close none, and a block trade on volume bars can close several.
    pub fn push(&mut self, trade: &Trade, mut sealed: impl FnMut(TradeBar)) {
        let price = to_price(trade.price_tick, self.tick_size);
        match self.rule {
            Rule::Trades(n) => {
                let bar = self.open(trade, price);
                bar.bar.add_trade(trade.price_tick, price, trade.size, trade.side, 1);
                bar.trades += 1;
                if bar.trades >= u64::from(n.max(1)) {
                    sealed_one(&mut self.forming, &mut sealed);
                }
            }
            Rule::Volume(n) => {
                let n = n.max(1);
                let (mut left, mut count) = (trade.size, 1);
                loop {
                    let bar = self.open(trade, price);
                    let room = n - bar.bar.volume.min(n);
                    let part = left.min(room);
                    bar.bar.add_trade(trade.price_tick, price, part, trade.side, count);
                    bar.trades += u64::from(count);
                    left -= part;
                    count = 0;
                    if bar.bar.volume >= n {
                        sealed_one(&mut self.forming, &mut sealed);
                    }
                    if left == 0 {
                        break;
                    }
                }
            }
            Rule::Range(n) => {
                let tick = trade.price_tick;
                let breaks = self.forming.is_some()
                    && i64::from(self.high.max(tick)) - i64::from(self.low.min(tick)) > i64::from(n);
                if breaks && let Some(done) = self.forming.take() {
                    sealed(done);
                }
                if self.forming.is_none() {
                    (self.low, self.high) = (tick, tick);
                }
                (self.low, self.high) = (self.low.min(tick), self.high.max(tick));
                let bar = self.open(trade, price);
                bar.bar.add_trade(tick, price, trade.size, trade.side, 1);
                bar.trades += 1;
            }
        }
    }

    /// The forming bar, opened by `trade` if there is none.
    fn open(&mut self, trade: &Trade, price: f64) -> &mut TradeBar {
        let bar = self.forming.get_or_insert_with(|| TradeBar {
            start_nanos: trade.ts_nanos,
            end_nanos: trade.ts_nanos,
            trades: 0,
            bar: LadderBar::opening(trade.ts_nanos.div_euclid(NANOS), price),
        });
        bar.end_nanos = trade.ts_nanos;
        bar
    }
}

fn sealed_one(forming: &mut Option<TradeBar>, sealed: &mut impl FnMut(TradeBar)) {
    if let Some(done) = forming.take() {
        sealed(done);
    }
}

/// `trades` folded into bars of `rule`, the last one still open included.
pub fn fold<'a>(trades: impl IntoIterator<Item = &'a Trade>, rule: Rule, tick_size: f64) -> Vec<TradeBar> {
    let mut out = Vec::new();
    let mut fold = Fold::new(rule, tick_size);
    for trade in trades {
        fold.push(trade, |bar| out.push(bar));
    }
    out.extend(fold.finish());
    out
}

/// `trades` folded into minutes: what a [`Forming`] would have sealed, the
/// last minute included.
pub fn minutes<'a>(trades: impl IntoIterator<Item = &'a Trade>, tick_size: f64) -> Vec<Minute> {
    let mut out = Vec::new();
    let mut forming = Forming::new(tick_size);
    for t in trades {
        out.extend(forming.push(t.ts_nanos, t.price_tick, t.size, t.side));
    }
    out.extend(forming.disconnect());
    out
}

/// The live path for one contract: every print goes into the buffer and the
/// forming minute together.
#[derive(Clone, Debug)]
pub struct Tape {
    pub ticks: Ticks,
    pub forming: Forming,
}

impl Tape {
    pub fn new(tick_size: f64, capacity: usize) -> Tape {
        Tape { ticks: Ticks::new(capacity), forming: Forming::new(tick_size) }
    }

    /// Feed a print. Returns the minute it sealed, if it opened a new one.
    pub fn push(&mut self, trade: Trade) -> Option<Minute> {
        self.ticks.push(trade);
        self.forming.push(trade.ts_nanos, trade.price_tick, trade.size, trade.side)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ladders::to_tick;

    const TICK: f64 = 0.25;
    const S: i64 = NANOS;

    fn trade(secs: f64, tick: i32, size: u64, side: Side) -> Trade {
        Trade { ts_nanos: (secs * S as f64) as i64, price_tick: tick, size, side }
    }

    #[test]
    fn a_trade_is_24_bytes() {
        assert_eq!(std::mem::size_of::<Trade>(), 24);
    }

    #[test]
    fn a_full_buffer_drops_its_oldest_whole_minute() {
        let mut ticks = Ticks::new(10);
        for i in 0..4 {
            ticks.push(trade(f64::from(i), 100, 1, Side::Buy)); // minute 0
        }
        for i in 0..6 {
            ticks.push(trade(60.0 + f64::from(i), 100, 1, Side::Buy)); // minute 1
        }
        assert_eq!(ticks.len(), 10);
        ticks.push(trade(120.0, 100, 1, Side::Buy));
        assert_eq!(ticks.len(), 7, "all four of minute 0 went, and nothing of minute 1");
        assert_eq!(ticks.first().map(Trade::minute), Some(60));
        // Many times over, it stays bounded and in order.
        for i in 0..1_000 {
            ticks.push(trade(180.0 + f64::from(i) * 7.0, 100, 1, Side::Buy));
        }
        assert!(ticks.len() <= 10);
        let all: Vec<_> = ticks.all().iter().map(|t| t.ts_nanos).collect();
        assert!(all.windows(2).all(|w| w[0] <= w[1]));
    }

    #[test]
    fn slices_borrow_the_stretch_asked_for() {
        let mut ticks = Ticks::new(8);
        // Wrap the ring so a span can straddle its two halves.
        for i in 0..20 {
            ticks.push(trade(f64::from(i) * 20.0, i, 1, Side::Sell));
        }
        let held: Vec<i32> = ticks.all().iter().map(|t| t.price_tick).collect();
        let first = ticks.first().unwrap().ts_nanos;
        let span = ticks.range(first + 20 * S, first + 80 * S);
        let got: Vec<i32> = span.iter().map(|t| t.price_tick).collect();
        assert_eq!(got, held[1..4].to_vec());
        assert_eq!(ticks.since(first + 21 * S).len(), held.len() - 2);
        assert!(ticks.range(first + 80 * S, first).is_empty());
        assert_eq!(ticks.since(i64::MIN).len(), ticks.len());
    }

    #[test]
    fn tick_bars_close_every_n_trades() {
        let trades: Vec<_> = (0..10).map(|i| trade(f64::from(i), 100 + i, 2, Side::Buy)).collect();
        let bars = fold(&trades, Rule::Trades(4), TICK);
        assert_eq!(bars.iter().map(|b| b.trades).collect::<Vec<_>>(), vec![4, 4, 2]);
        assert_eq!(bars[0].bar.volume, 8);
        assert_eq!((bars[1].bar.open, bars[1].bar.close), (to_price(104, TICK), to_price(107, TICK)));
        assert_eq!(bars[1].start_nanos, 4 * S);
        assert_eq!(bars[1].end_nanos, 7 * S);
        assert_eq!(bars[2].bar.ladder.sides.ask_trades, 2);
    }

    #[test]
    fn volume_bars_split_a_print_that_runs_past_the_end() {
        let trades =
            vec![trade(0.0, 100, 3, Side::Buy), trade(1.0, 101, 9, Side::Sell), trade(2.0, 102, 1, Side::Buy)];
        let bars = fold(&trades, Rule::Volume(5), TICK);
        assert_eq!(bars.iter().map(|b| b.bar.volume).collect::<Vec<_>>(), vec![5, 5, 3]);
        assert_eq!(bars[0].bar.ladder.at(101).at_bid, 2);
        assert_eq!(bars[1].bar.ladder.at(101).at_bid, 5);
        assert_eq!(bars[2].bar.ladder.at(101).at_bid, 2);
        // The split print counts once, where it started.
        assert_eq!(bars.iter().map(|b| b.trades).sum::<u64>(), 3);
        assert_eq!(bars[0].trades, 2);
    }

    #[test]
    fn range_bars_close_before_the_print_that_would_break_the_range() {
        let ticks = [100, 101, 102, 99, 98, 97, 101];
        let trades: Vec<_> = ticks.iter().enumerate().map(|(i, &t)| trade(i as f64, t, 1, Side::Buy)).collect();
        let bars = fold(&trades, Rule::Range(3), TICK);
        let ranges: Vec<(i32, i32)> = bars
            .iter()
            .map(|b| (to_tick(b.bar.low, TICK), to_tick(b.bar.high, TICK)))
            .collect();
        assert_eq!(ranges, vec![(99, 102), (97, 98), (101, 101)]);
        assert!(bars.iter().all(|b| b.bar.ladder.levels.len() <= 4));
    }

    #[test]
    fn minutes_from_ticks_match_what_the_forming_minute_seals() {
        let trades: Vec<_> = (0..300).map(|i| trade(f64::from(i) * 0.7, 100 + i % 5, 1, Side::Buy)).collect();
        let minutes = minutes(&trades, TICK);
        assert_eq!(minutes.iter().map(|m| m.ts).collect::<Vec<_>>(), vec![0, 60, 120, 180]);
        assert_eq!(minutes.iter().map(|m| m.volume).sum::<u64>(), 300);
        let mut tape = Tape::new(TICK, 1_000);
        let sealed: Vec<Minute> = trades.iter().filter_map(|t| tape.push(*t)).collect();
        assert_eq!(sealed[..], minutes[..3]);
        assert_eq!(tape.ticks.len(), 300);
    }
}
