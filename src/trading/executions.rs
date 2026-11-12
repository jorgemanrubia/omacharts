//! An account's fills in one contract, and the round trips they make.
//!
//! What a chart shows of past trading: a mark at each fill, and a line
//! from each entry to the exit that closed it. The fills come from two
//! places, the trading service's fill history, asked once a session, and
//! the session's own book, which has today's orders and every fill heard
//! since; [`fills_of`] makes one list of them, each fill once. [`pair`]
//! then matches exits to entries first in, first out, which is how a
//! position's round trips are counted when it was built and taken off in
//! parts.
//!
//! No store, no window and no trading service here: the window hands the orders in
//! and draws what comes back.

use std::collections::HashSet;

use super::{Order, Side};

/// One fill: when, at what price, which way and how many.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Execution {
    /// Unix seconds.
    pub at: i64,
    pub price: f64,
    pub side: Side,
    pub qty: u32,
}

/// A fill as the chart marks it: whether it opened or added to a position,
/// closed or took some off, or both, as a fill that turns a long into a
/// short does.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Marked {
    pub at: i64,
    pub price: f64,
    pub side: Side,
    pub opens: bool,
    pub closes: bool,
}

/// One round trip: an entry, the exit that closed it, and how many
/// contracts went through both.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Trip {
    /// When and where the entry filled.
    pub opened: (i64, f64),
    /// When and where the exit filled.
    pub closed: (i64, f64),
    /// The position's side: [`Side::Buy`] for a long.
    pub side: Side,
    pub qty: u32,
}

impl Trip {
    /// Won, lost, or `None` for one closed where it opened.
    pub fn won(&self) -> Option<bool> {
        let moved = self.closed.1 - self.opened.1;
        let gained = match self.side {
            Side::Buy => moved,
            Side::Sell => -moved,
        };
        (gained != 0.0).then_some(gained > 0.0)
    }
}

/// What [`pair`] made of the fills: a mark for each, oldest first, and
/// the round trips, in the order their exits filled.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Paired {
    pub marks: Vec<Marked>,
    pub trips: Vec<Trip>,
}

/// What is still open of an entry, waiting for an exit.
struct Lot {
    /// Where it filled, or `None` for what was held before the first
    /// fill there is: an exit taken off it closes it with no line.
    from: Option<(i64, f64)>,
    qty: u32,
}

/// Pair `fills`, oldest first, first in first out. `held_before` is the
/// position before the first of them, positive long: what the history
/// does not reach back to, so that the exits that close it are not taken
/// for entries.
///
/// A fill the same way as the position, or from flat, is an entry. One
/// the other way is an exit, taken off the oldest entries first, a round
/// trip for each it closes part of: an entry scaled out of in three parts
/// makes three, three entries closed by one exit make three. What an exit
/// has left over once the position is flat opens a position the other
/// way, so a fill that turns long into short is both.
pub fn pair(fills: &[Execution], held_before: i32) -> Paired {
    let mut lots: std::collections::VecDeque<Lot> = std::collections::VecDeque::new();
    // Which way the lots hold, while there are any.
    let mut long = held_before > 0;
    if held_before != 0 {
        lots.push_back(Lot { from: None, qty: held_before.unsigned_abs() });
    }
    let mut paired = Paired { marks: Vec::with_capacity(fills.len()), trips: Vec::new() };
    for fill in fills.iter().filter(|fill| fill.qty > 0) {
        let buys = fill.side == Side::Buy;
        let mut left = fill.qty;
        let mut closes = false;
        if !lots.is_empty() && buys != long {
            closes = true;
            while left > 0 {
                let Some(lot) = lots.front_mut() else { break };
                let taken = lot.qty.min(left);
                if let Some(from) = lot.from {
                    paired.trips.push(Trip {
                        opened: from,
                        closed: (fill.at, fill.price),
                        side: if long { Side::Buy } else { Side::Sell },
                        qty: taken,
                    });
                }
                lot.qty -= taken;
                left -= taken;
                if lot.qty == 0 {
                    lots.pop_front();
                }
            }
        }
        let opens = left > 0;
        if opens {
            long = buys;
            lots.push_back(Lot { from: Some((fill.at, fill.price)), qty: left });
        }
        paired.marks.push(Marked { at: fill.at, price: fill.price, side: fill.side, opens, closes });
    }
    paired
}

/// Every fill of `contract` in `orders`, each once, oldest first.
///
/// The same order can come from the history and from the book, and the
/// same fill twice in one of them; a fill is the same one when its order,
/// its time, its size and its price are. A fill with no time, which some
/// trading services leave out of what they push, takes `heard`'s answer
/// for its order and its place among the order's fills: when this window
/// first saw it.
pub fn fills_of<'a>(
    orders: impl IntoIterator<Item = &'a Order>,
    contract: &str,
    heard: &mut dyn FnMut(&str, usize) -> i64,
) -> Vec<Execution> {
    let mut seen: HashSet<(String, i64, u32, u64)> = HashSet::new();
    let mut fills = Vec::new();
    for order in orders.into_iter().filter(|order| order.contract == contract) {
        // A trading service that tells only how much filled and at what
        // average gives one fill of all of it.
        let whole = [super::Fill {
            basket: order.basket_id.clone(),
            at: order.at,
            qty: order.filled,
            price: order.average_fill().unwrap_or(0.0),
        }];
        let told = match order.fills.is_empty() {
            true if order.filled > 0 && order.average_fill().is_some() => &whole[..],
            true => &[],
            false => &order.fills[..],
        };
        for (index, fill) in told.iter().enumerate() {
            if fill.qty == 0 || !fill.price.is_finite() || fill.price <= 0.0 {
                continue;
            }
            let at = if fill.at > 0 { fill.at } else { heard(&order.basket_id, index) };
            if !seen.insert((order.basket_id.clone(), at, fill.qty, fill.price.to_bits())) {
                continue;
            }
            fills.push(Execution { at, price: fill.price, side: order.side, qty: fill.qty });
        }
    }
    // Stable, so fills of one moment keep the order they were told in.
    fills.sort_by_key(|fill| fill.at);
    fills
}

/// The position before `fills`, given what is held after them: what the
/// fills do not account for was held before the first.
pub fn held_before(fills: &[Execution], held_now: i32) -> i32 {
    let moved: i64 = fills
        .iter()
        .map(|fill| match fill.side {
            Side::Buy => i64::from(fill.qty),
            Side::Sell => -i64::from(fill.qty),
        })
        .sum();
    i32::try_from(i64::from(held_now) - moved).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trading::{Fill, OrderKind, Stage};

    fn buy(at: i64, price: f64, qty: u32) -> Execution {
        Execution { at, price, side: Side::Buy, qty }
    }

    fn sell(at: i64, price: f64, qty: u32) -> Execution {
        Execution { at, price, side: Side::Sell, qty }
    }

    fn trip(opened: (i64, f64), closed: (i64, f64), side: Side, qty: u32) -> Trip {
        Trip { opened, closed, side, qty }
    }

    fn ends(paired: &Paired) -> Vec<(bool, bool)> {
        paired.marks.iter().map(|mark| (mark.opens, mark.closes)).collect()
    }

    #[test]
    fn a_long_and_a_short_each_make_one_round_trip() {
        let paired = pair(&[buy(1, 100.0, 1), sell(2, 101.0, 1), sell(3, 105.0, 2), buy(4, 106.0, 2)], 0);
        assert_eq!(
            paired.trips,
            [trip((1, 100.0), (2, 101.0), Side::Buy, 1), trip((3, 105.0), (4, 106.0), Side::Sell, 2)]
        );
        assert_eq!(ends(&paired), [(true, false), (false, true), (true, false), (false, true)]);
        assert_eq!(paired.trips[0].won(), Some(true));
        assert_eq!(paired.trips[1].won(), Some(false), "a short closed higher lost");
    }

    #[test]
    fn scaling_out_closes_the_entry_in_parts() {
        let paired = pair(&[buy(1, 100.0, 3), sell(2, 102.0, 1), sell(3, 104.0, 2)], 0);
        assert_eq!(
            paired.trips,
            [trip((1, 100.0), (2, 102.0), Side::Buy, 1), trip((1, 100.0), (3, 104.0), Side::Buy, 2)]
        );
    }

    #[test]
    fn scaling_in_is_closed_oldest_first() {
        let paired = pair(&[buy(1, 100.0, 1), buy(2, 98.0, 2), sell(3, 101.0, 2), sell(4, 99.0, 1)], 0);
        assert_eq!(
            paired.trips,
            [
                trip((1, 100.0), (3, 101.0), Side::Buy, 1),
                trip((2, 98.0), (3, 101.0), Side::Buy, 1),
                trip((2, 98.0), (4, 99.0), Side::Buy, 1),
            ]
        );
        assert_eq!(ends(&paired), [(true, false), (true, false), (false, true), (false, true)]);
    }

    #[test]
    fn a_fill_through_flat_closes_one_way_and_opens_the_other() {
        let paired = pair(&[buy(1, 100.0, 1), sell(2, 103.0, 3), buy(3, 101.0, 2)], 0);
        assert_eq!(
            paired.trips,
            [trip((1, 100.0), (2, 103.0), Side::Buy, 1), trip((2, 103.0), (3, 101.0), Side::Sell, 2)]
        );
        assert_eq!(ends(&paired), [(true, false), (true, true), (false, true)]);
        assert_eq!(paired.trips[1].won(), Some(true));
    }

    #[test]
    fn what_was_held_before_the_history_is_closed_without_a_line() {
        // Long two from before the first fill: the sell closes them, and
        // is no short.
        let fills = [sell(1, 100.0, 2), buy(2, 99.0, 1)];
        assert_eq!(held_before(&fills, 1), 2);
        let paired = pair(&fills, 2);
        assert!(paired.trips.is_empty(), "{:?}", paired.trips);
        assert_eq!(ends(&paired), [(false, true), (true, false)]);
    }

    #[test]
    fn a_trip_closed_where_it_opened_neither_won_nor_lost() {
        let paired = pair(&[sell(1, 100.0, 1), buy(2, 100.0, 1)], 0);
        assert_eq!(paired.trips[0].won(), None);
    }

    fn filled(basket: &str, side: Side, fills: Vec<Fill>) -> Order {
        Order {
            account: "A".into(),
            basket_id: basket.into(),
            contract: "ESZ6".into(),
            exchange: "CME".into(),
            side,
            qty: fills.iter().map(|fill| fill.qty).sum(),
            filled: fills.iter().map(|fill| fill.qty).sum(),
            kind: OrderKind::Market,
            price: None,
            status: Stage::Filled.into(),
            fill_price: None,
            fills,
            at: 50,
            tag: None,
            tick: Some(0.25),
        }
    }

    #[test]
    fn an_order_in_the_history_and_the_book_is_counted_once() {
        let fill = |at: i64, qty: u32, price: f64| Fill { basket: "B1".into(), at, qty, price };
        let history = filled("B1", Side::Buy, vec![fill(10, 1, 100.0), fill(11, 1, 100.25)]);
        let book = filled("B1", Side::Buy, vec![fill(10, 1, 100.0), fill(11, 1, 100.25)]);
        let other = Order { contract: "NQZ6".into(), ..filled("B2", Side::Sell, vec![fill(12, 1, 200.0)]) };
        let fills = fills_of([&book, &history, &other], "ESZ6", &mut |_, _| 0);
        assert_eq!(fills, [buy(10, 100.0, 1), buy(11, 100.25, 1)]);
    }

    #[test]
    fn a_fill_with_no_time_takes_the_moment_it_was_heard() {
        let order = filled("B7", Side::Sell, vec![Fill { basket: "B7".into(), at: 0, qty: 2, price: 101.0 }]);
        let mut asked = Vec::new();
        let fills = fills_of([&order], "ESZ6", &mut |basket, index| {
            asked.push((basket.to_string(), index));
            1_000
        });
        assert_eq!(fills, [sell(1_000, 101.0, 2)]);
        assert_eq!(asked, [("B7".to_string(), 0)]);
    }

    #[test]
    fn an_order_told_only_as_filled_at_an_average_is_one_fill() {
        let order = Order { fill_price: Some(99.5), ..filled("B3", Side::Buy, Vec::new()) };
        let order = Order { filled: 2, qty: 2, ..order };
        assert_eq!(fills_of([&order], "ESZ6", &mut |_, _| 0), [buy(50, 99.5, 2)]);
    }
}
