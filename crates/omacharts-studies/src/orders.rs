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

/// A large order where it is drawn: the order, whether it printed in
/// regular hours, and which of the book's sessions it is in.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Placed {
    pub order: Order,
    pub regular: bool,
    pub session: u32,
}

/// The largest orders of one session, all told and in each part of it.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct SessionLargest {
    /// What the hours said the session is: its trading date, say.
    pub key: i64,
    pub all: u64,
    pub regular: u64,
    pub extended: u64,
}

/// The large orders of a span, oldest first, filtered once: a chart finds
/// the ones it shows by binary search and sizes them against the largest
/// of their session, or of their hours, read here, or of those it shows.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct Book {
    pub orders: Vec<Placed>,
    pub sessions: Vec<SessionLargest>,
}

impl Book {
    /// The orders that began printing in `from..to`, unix nanoseconds.
    pub fn within(&self, from: i64, to: i64) -> &[Placed] {
        let start = self.orders.partition_point(|placed| placed.order.ts_nanos < from);
        let end = start + self.orders[start..].partition_point(|placed| placed.order.ts_nanos < to);
        &self.orders[start..end]
    }

    /// The largest order of `placed`'s session, or of its session's
    /// regular or extended hours when `by_hours`.
    pub fn largest_for(&self, placed: &Placed, by_hours: bool) -> u64 {
        let Some(session) = self.sessions.get(placed.session as usize) else { return placed.order.size };
        match (by_hours, placed.regular) {
            (false, _) => session.all,
            (true, true) => session.regular,
            (true, false) => session.extended,
        }
    }

    /// The largest of `orders`.
    pub fn largest_in(orders: &[Placed]) -> u64 {
        orders.iter().map(|placed| placed.order.size).max().unwrap_or(0)
    }
}

/// A [`Book`] built a print at a time: from the history, oldest first, then
/// from each trade as it prints.
#[derive(Clone, Debug)]
pub struct Builder {
    combiner: Combiner,
    thresholds: Thresholds,
    book: Book,
}

impl Builder {
    pub fn new(combine: bool, thresholds: Thresholds) -> Builder {
        Builder { combiner: Combiner::new(combine), thresholds, book: Book::default() }
    }

    /// Take the next print. `hours` says, of a moment in unix seconds, the
    /// session it is in and whether it is in regular hours; it is asked
    /// only of an order big enough to count in either.
    #[inline]
    pub fn push(&mut self, print: Print, hours: &mut impl FnMut(i64) -> (i64, bool)) {
        if let Some(order) = self.combiner.push(print) {
            self.place(order, hours);
        }
    }

    fn place(&mut self, order: Order, hours: &mut impl FnMut(i64) -> (i64, bool)) {
        if let Some(placed) = placed(&self.book, &self.thresholds, order, hours) {
            add(&mut self.book, placed.0, placed.1);
        }
    }

    /// The book as it stands, with the order still open — a sweep whose
    /// next fill may yet come — in it if it counts.
    pub fn book(&self, hours: &mut impl FnMut(i64) -> (i64, bool)) -> Book {
        let mut book = self.book.clone();
        if let Some(open) = self.combiner.peek()
            && let Some((placed, key)) = placed(&book, &self.thresholds, open, hours)
        {
            add(&mut book, placed, key);
        }
        book
    }

    /// The orders closed so far, read in place: [`Builder::book`] without
    /// the order still open, and without copying the book. Each order is in
    /// it once the print after its last fill has arrived, and the largest
    /// of each session are of those orders alone.
    #[inline]
    pub fn closed(&self) -> &Book {
        &self.book
    }

    /// The order still open, placed as [`Builder::book`] places it — in the
    /// last session of [`Builder::closed`] or the one after it — if it is
    /// big enough to count yet. Nothing is copied; `hours` is asked only of
    /// an open order that could count.
    #[inline]
    pub fn open(&self, hours: &mut impl FnMut(i64) -> (i64, bool)) -> Option<Placed> {
        let open = self.combiner.peek()?;
        placed(&self.book, &self.thresholds, open, hours).map(|(placed, _)| placed)
    }

    /// Every print in: the open order is closed, and the book is final.
    pub fn finish(mut self, hours: &mut impl FnMut(i64) -> (i64, bool)) -> Book {
        if let Some(order) = self.combiner.flush() {
            self.place(order, hours);
        }
        self.book
    }
}

fn placed(book: &Book, thresholds: &Thresholds, order: Order, hours: &mut impl FnMut(i64) -> (i64, bool)) -> Option<(Placed, i64)> {
    if order.size <= thresholds.floor() {
        return None;
    }
    let (key, regular) = hours(order.ts_nanos.div_euclid(1_000_000_000));
    if !thresholds.counts(order.size, regular) {
        return None;
    }
    let session = match book.sessions.last() {
        Some(last) if last.key == key => book.sessions.len() - 1,
        _ => book.sessions.len(),
    };
    Some((Placed { order, regular, session: session as u32 }, key))
}

fn add(book: &mut Book, placed: Placed, key: i64) {
    if placed.session as usize == book.sessions.len() {
        book.sessions.push(SessionLargest { key, ..SessionLargest::default() });
    }
    let session = &mut book.sessions[placed.session as usize];
    let size = placed.order.size;
    session.all = session.all.max(size);
    match placed.regular {
        true => session.regular = session.regular.max(size),
        false => session.extended = session.extended.max(size),
    }
    book.orders.push(placed);
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

    /// Regular hours are 10 to 20 seconds into each 100-second "session".
    fn hours(seconds: i64) -> (i64, bool) {
        (seconds.div_euclid(100), (10..20).contains(&seconds.rem_euclid(100)))
    }

    const S: i64 = 1_000_000_000;

    #[test]
    fn a_book_keeps_only_the_orders_bigger_than_their_hours_threshold() {
        use Aggressor::*;
        let thresholds = Thresholds { regular: 50, extended: 20 };
        let prints = [
            p(5 * S, 100, 30, Buy),   // extended, counts
            p(12 * S, 100, 30, Buy),  // regular, too small
            p(13 * S, 101, 40, Sell), // regular: 40 + 20 combined = 60
            p(13 * S, 100, 20, Sell),
            p(105 * S, 99, 25, Buy), // the next session, extended
        ];
        let mut builder = Builder::new(true, thresholds);
        for print in prints {
            builder.push(print, &mut hours);
        }
        let book = builder.finish(&mut hours);
        let sizes: Vec<(u64, bool, u32)> = book.orders.iter().map(|o| (o.order.size, o.regular, o.session)).collect();
        assert_eq!(sizes, vec![(30, false, 0), (60, true, 0), (25, false, 1)]);
        assert_eq!(book.sessions[0], SessionLargest { key: 0, all: 60, regular: 60, extended: 30 });
        assert_eq!(book.largest_for(&book.orders[0], true), 30);
        assert_eq!(book.largest_for(&book.orders[0], false), 60);
        assert_eq!(book.within(10 * S, 106 * S).len(), 2);
        assert_eq!(book.within(0, 5 * S).len(), 0, "to is not included");
        assert_eq!(Book::largest_in(book.within(0, 200 * S)), 60);
    }

    #[test]
    fn a_book_as_it_stands_shows_the_open_order_without_closing_it() {
        let thresholds = Thresholds { regular: 10, extended: 10 };
        let mut builder = Builder::new(true, thresholds);
        builder.push(p(S, 100, 8, Aggressor::Buy), &mut hours);
        assert!(builder.book(&mut hours).orders.is_empty());
        builder.push(p(S, 101, 8, Aggressor::Buy), &mut hours);
        assert_eq!(builder.book(&mut hours).orders.len(), 1, "the sweep so far counts");
        builder.push(p(S, 102, 8, Aggressor::Buy), &mut hours);
        let book = builder.finish(&mut hours);
        assert_eq!(book.orders.iter().map(|o| o.order.size).collect::<Vec<_>>(), vec![24]);
    }

    /// Read in place, a book is its closed orders and the open one beside
    /// them: together, after every print, what the copy as it stands holds.
    #[test]
    fn the_closed_orders_and_the_open_one_are_the_book_as_it_stands() {
        use Aggressor::*;
        let thresholds = Thresholds { regular: 10, extended: 5 };
        let prints = [
            p(5 * S, 100, 4, Buy),
            p(5 * S, 101, 4, Buy),
            p(12 * S, 100, 30, Sell),
            p(13 * S, 99, 3, Sell),
            p(13 * S, 98, 9, Sell),
            p(105 * S, 99, 25, Buy),
            p(106 * S, 99, 2, Unknown),
        ];
        let mut builder = Builder::new(true, thresholds);
        assert_eq!(builder.open(&mut hours), None);
        for print in prints {
            builder.push(print, &mut hours);
            let mut together = builder.closed().clone();
            if let Some(open) = builder.open(&mut hours) {
                assert!(open.session as usize <= together.sessions.len());
                together.orders.push(open);
            }
            let copied = builder.book(&mut hours);
            assert_eq!(together.orders, copied.orders, "after the print at {}", print.ts_nanos);
            assert!(builder.closed().orders.len() <= copied.orders.len());
        }
        let closed: Vec<u64> = builder.closed().orders.iter().map(|o| o.order.size).collect();
        assert_eq!(closed, vec![8, 30, 12, 25], "each closed by the print after its last fill");
        assert_eq!(builder.open(&mut hours), None, "an unknown side of two is too small to count");
    }
}
