//! Large orders: single trades bigger than a threshold, from the prints.
//!
//! A print is one trade as the exchange reports it. An aggressive order
//! that takes more than the best price offers is reported as several prints,
//! one per resting order it filled, all at the same moment and on the same
//! side; CME reports them with one timestamp, and Sierra Chart marks the
//! pieces of a trade it split. [`Combiner`] folds such a run back into the
//! one order it was, when asked to; otherwise every print is an order of its
//! own. An order's price is the average of its fills, weighted by size.
//!
//! Prices are whole ticks (`i32`), as volume at price is counted. Nothing
//! here allocates.

/// Who took liquidity: a buyer lifting the offer, a seller hitting the bid,
/// or a print that does not say.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Hash)]
#[repr(u8)]
pub enum Aggressor {
    Buy = 0,
    Sell = 1,
    #[default]
    Unknown = 2,
}

/// One print, in sixteen bytes: when, where, how many, which side, and
/// whether it continues the trade before it.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Hash)]
pub struct Print {
    /// Unix nanoseconds.
    pub ts_nanos: i64,
    pub tick: i32,
    /// The size in the low 29 bits, the side in the next two, and the top
    /// bit for a print the source says continues the one before it.
    packed: u32,
}

const SIZE_BITS: u32 = 29;
const SIZE_MASK: u32 = (1 << SIZE_BITS) - 1;
const CONTINUES: u32 = 1 << 31;

impl Print {
    /// A print of `size` (held to 2^29 - 1, half a billion) at `tick`.
    /// `continues` says the source marked it a further fill of the trade
    /// before it.
    #[inline]
    pub fn new(ts_nanos: i64, tick: i32, size: u64, side: Aggressor, continues: bool) -> Print {
        let size = size.min(u64::from(SIZE_MASK)) as u32;
        let packed = size | ((side as u32) << SIZE_BITS) | if continues { CONTINUES } else { 0 };
        Print { ts_nanos, tick, packed }
    }

    #[inline]
    pub fn size(&self) -> u64 {
        u64::from(self.packed & SIZE_MASK)
    }

    #[inline]
    pub fn side(&self) -> Aggressor {
        match (self.packed >> SIZE_BITS) & 3 {
            0 => Aggressor::Buy,
            1 => Aggressor::Sell,
            _ => Aggressor::Unknown,
        }
    }

    #[inline]
    pub fn continues(&self) -> bool {
        self.packed & CONTINUES != 0
    }

    /// The packed size, side and flag, for a store that keeps prints as
    /// bytes; [`Print::from_parts`] reads it back.
    #[inline]
    pub fn packed(&self) -> u32 {
        self.packed
    }

    #[inline]
    pub fn from_parts(ts_nanos: i64, tick: i32, packed: u32) -> Print {
        Print { ts_nanos, tick, packed }
    }
}

/// One order: a print, or a run of them folded together.
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct Order {
    /// When its first fill printed, unix nanoseconds.
    pub ts_nanos: i64,
    pub size: u64,
    pub side: Aggressor,
    /// The average tick of its fills, weighted by their sizes.
    pub tick: f64,
    pub fills: u32,
}

/// An order being folded together.
#[derive(Clone, Copy, Debug)]
struct Open {
    first: i64,
    last: i64,
    size: u64,
    /// The sum of tick times size, for the weighted average.
    weighted: i128,
    side: Aggressor,
    fills: u32,
}

impl Open {
    fn of(print: &Print) -> Open {
        Open {
            first: print.ts_nanos,
            last: print.ts_nanos,
            size: print.size(),
            weighted: i128::from(print.tick) * i128::from(print.size()),
            side: print.side(),
            fills: 1,
        }
    }

    fn order(&self, at: i32) -> Order {
        let tick = match self.size {
            0 => f64::from(at),
            size => self.weighted as f64 / size as f64,
        };
        Order { ts_nanos: self.first, size: self.size, side: self.side, tick, fills: self.fills }
    }
}

/// Prints into orders, in the order they printed.
///
/// With `combine` on, a print continues the open order when it is on the
/// same known side and either has the same timestamp as the order's last
/// fill or is marked as continuing it. A print of unknown side is always an
/// order of its own.
#[derive(Clone, Debug)]
pub struct Combiner {
    combine: bool,
    open: Option<Open>,
    /// The tick of the open order's last fill, for an order of size 0.
    at: i32,
}

impl Combiner {
    pub fn new(combine: bool) -> Combiner {
        Combiner { combine, open: None, at: 0 }
    }

    pub fn combines(&self) -> bool {
        self.combine
    }

    /// Take the next print. Hands back the order it closed, if it closed one.
    #[inline]
    pub fn push(&mut self, print: Print) -> Option<Order> {
        if let Some(open) = self.open.as_mut()
            && self.combine
            && print.side() != Aggressor::Unknown
            && print.side() == open.side
            && (print.ts_nanos == open.last || print.continues())
        {
            open.last = print.ts_nanos;
            open.size += print.size();
            open.weighted += i128::from(print.tick) * i128::from(print.size());
            open.fills += 1;
            self.at = print.tick;
            return None;
        }
        let closed = self.open.map(|open| open.order(self.at));
        self.open = Some(Open::of(&print));
        self.at = print.tick;
        closed
    }

    /// The order still open, as it stands, without closing it: on a live
    /// feed, a sweep whose next fill may still come.
    #[inline]
    pub fn peek(&self) -> Option<Order> {
        self.open.map(|open| open.order(self.at))
    }

    /// Close the open order, if there is one: the prints have ended.
    #[inline]
    pub fn flush(&mut self) -> Option<Order> {
        self.open.take().map(|open| open.order(self.at))
    }
}

/// The smallest sizes that count as large, in and out of regular hours: an
/// order counts when it is bigger than the threshold of the hours it printed
/// in.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Thresholds {
    pub regular: u64,
    pub extended: u64,
}

impl Thresholds {
    #[inline]
    pub fn counts(&self, size: u64, regular: bool) -> bool {
        size > if regular { self.regular } else { self.extended }
    }

    /// The lower of the two: nothing smaller can count in either.
    #[inline]
    pub fn floor(&self) -> u64 {
        self.regular.min(self.extended)
    }
}

/// How big a bubble is against the largest it is compared with, `0..=1`:
/// the square root of the share of the size, so a bubble's area, not its
/// width, is in proportion to the order.
#[inline]
pub fn bubble_share(size: u64, largest: u64) -> f64 {
    if largest == 0 {
        return 1.0;
    }
    (size as f64 / largest as f64).clamp(0.0, 1.0).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(ts: i64, tick: i32, size: u64, side: Aggressor) -> Print {
        Print::new(ts, tick, size, side, false)
    }

    fn all(combine: bool, prints: &[Print]) -> Vec<Order> {
        let mut combiner = Combiner::new(combine);
        let mut out: Vec<Order> = prints.iter().filter_map(|p| combiner.push(*p)).collect();
        out.extend(combiner.flush());
        out
    }

    #[test]
    fn a_print_packs_into_sixteen_bytes_and_reads_back() {
        assert_eq!(std::mem::size_of::<Print>(), 16);
        for side in [Aggressor::Buy, Aggressor::Sell, Aggressor::Unknown] {
            for continues in [false, true] {
                let print = Print::new(-5, -3, 1234, side, continues);
                assert_eq!((print.size(), print.side(), print.continues()), (1234, side, continues));
                assert_eq!(Print::from_parts(-5, -3, print.packed()), print);
            }
        }
        assert_eq!(Print::new(0, 0, u64::MAX, Aggressor::Buy, false).size(), u64::from(SIZE_MASK));
    }

    #[test]
    fn same_time_same_side_prints_are_one_order_at_their_average_price() {
        use Aggressor::*;
        let prints = [p(1, 100, 10, Buy), p(1, 101, 30, Buy), p(1, 101, 5, Sell), p(2, 101, 7, Sell), p(2, 102, 1, Sell)];
        let combined = all(true, &prints);
        assert_eq!(combined.len(), 3);
        assert_eq!((combined[0].size, combined[0].fills, combined[0].side), (40, 2, Buy));
        assert_eq!(combined[0].tick, 100.75);
        assert_eq!((combined[1].size, combined[1].ts_nanos), (5, 1), "a new side starts a new order");
        assert_eq!((combined[2].size, combined[2].ts_nanos, combined[2].fills), (8, 2, 2));
        assert_eq!(all(false, &prints).len(), prints.len(), "off, each print is an order");
    }

    #[test]
    fn a_print_marked_as_continuing_joins_the_order_before_it() {
        use Aggressor::*;
        let prints = [p(1, 100, 10, Sell), Print::new(2, 99, 10, Sell, true), Print::new(3, 98, 10, Buy, true)];
        let combined = all(true, &prints);
        assert_eq!(combined.iter().map(|o| o.size).collect::<Vec<_>>(), vec![20, 10], "not across sides");
    }

    #[test]
    fn prints_of_no_known_side_never_combine() {
        use Aggressor::*;
        let prints = [p(1, 100, 10, Unknown), p(1, 100, 10, Unknown)];
        assert_eq!(all(true, &prints).len(), 2);
    }

    #[test]
    fn peeking_shows_the_open_order_without_closing_it() {
        let mut combiner = Combiner::new(true);
        assert_eq!(combiner.peek(), None);
        combiner.push(p(1, 10, 3, Aggressor::Buy));
        combiner.push(p(1, 12, 1, Aggressor::Buy));
        assert_eq!(combiner.peek().map(|o| (o.size, o.tick)), Some((4, 10.5)));
        assert_eq!(combiner.flush().map(|o| o.size), Some(4));
        assert_eq!(combiner.flush(), None);
    }

    #[test]
    fn an_order_counts_when_it_is_bigger_than_its_hours_threshold() {
        let thresholds = Thresholds { regular: 50, extended: 20 };
        assert!(!thresholds.counts(50, true));
        assert!(thresholds.counts(51, true));
        assert!(thresholds.counts(21, false));
        assert!(!thresholds.counts(20, false));
        assert_eq!(thresholds.floor(), 20);
    }

    #[test]
    fn a_bubbles_area_is_in_proportion_to_its_size() {
        assert_eq!(bubble_share(100, 100), 1.0);
        assert_eq!(bubble_share(25, 100), 0.5);
        assert_eq!(bubble_share(200, 100), 1.0, "held to the largest");
        assert_eq!(bubble_share(5, 0), 1.0);
    }
}
