//! An intraday file's records as trades and single prints.
//!
//! A record of a tick file is one trade: the volume that traded at the bid,
//! the volume that traded at the ask, and, where its volume says more than
//! both, the rest on no side. A record with volume on both sides is two
//! prints of one moment. Sierra Chart splits a trade that filled at several
//! prices into records, marking the first and the last piece; the prints of
//! the pieces after the first continue it, so combining prints gives back
//! the order they were.

use omacharts_ticks::ladders::{Side, to_tick};
use omacharts_studies::orders::{Aggressor, Print};

use crate::scid::Record;

/// One record's trades at `price`: its tick of `tick_size`, and what traded
/// at the bid as sellers', at the ask as buyers' and the rest on no side,
/// in that order, any of them nought. `None` for a price that is not a
/// number; a negative or zero price is a price, as the file's bars take it.
#[inline]
pub fn trades(record: &Record, price: f64, tick_size: f64) -> Option<(i32, [(u64, Side); 3])> {
    if !price.is_finite() {
        return None;
    }
    let (bid, ask, all) = (u64::from(record.bid_volume), u64::from(record.ask_volume), u64::from(record.volume));
    let unknown = all.saturating_sub(bid + ask);
    Some((to_tick(price, tick_size), [(bid, Side::Sell), (ask, Side::Buy), (unknown, Side::Unknown)]))
}

/// Records into single prints, a record at a time in the file's order:
/// what is carried from one record to the next is whether a split trade is
/// still being read.
#[derive(Clone, Copy, Debug, Default)]
pub struct Prints {
    unbundled: bool,
}

impl Prints {
    pub fn new() -> Prints {
        Prints::default()
    }

    /// The prints of the next record, priced at `price`: one for each side
    /// that traded, sold first, then bought, then neither, each at the
    /// record's time and marked as continuing a split trade where it does.
    /// A record whose price is not a number prints nothing and leaves a
    /// split trade where it was.
    #[inline]
    pub fn push(&mut self, record: &Record, price: f64, tick_size: f64, mut each: impl FnMut(Print)) {
        let Some((tick, sides)) = trades(record, price, tick_size) else { return };
        let first = record.is_first_sub_trade();
        let continues = self.unbundled && !first;
        self.unbundled = first || (self.unbundled && !record.is_last_sub_trade());
        let ts = record.unix_micros().saturating_mul(1_000);
        for (size, side) in sides {
            if size == 0 {
                continue;
            }
            let side = match side {
                Side::Buy => Aggressor::Buy,
                Side::Sell => Aggressor::Sell,
                Side::Unknown => Aggressor::Unknown,
            };
            each(Print::new(ts, tick, size, side, continues));
        }
    }
}

/// The single prints of `records`, oldest first, each priced at its stored
/// close times `multiplier`.
pub fn prints_in(records: &[Record], multiplier: f64, tick_size: f64) -> Vec<Print> {
    let mut out = Vec::with_capacity(records.len());
    let mut prints = Prints::new();
    for record in records {
        prints.push(record, f64::from(record.close) * multiplier, tick_size, |print| out.push(print));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use omacharts_studies::orders::Combiner;

    fn trade(seconds: i64, price: f32, size: u32, at_ask: bool) -> Record {
        Record {
            datetime: crate::sc_from_unix_micros(seconds * 1_000_000),
            open: 0.0,
            high: price + 25.0,
            low: price,
            close: price,
            trades: 1,
            volume: size,
            bid_volume: if at_ask { 0 } else { size },
            ask_volume: if at_ask { size } else { 0 },
        }
    }

    /// Each record is a print a side, at its own moment, and the pieces of
    /// a trade Sierra Chart split continue the first of them, up to the
    /// last; a trade after them starts afresh.
    #[test]
    fn single_trades_come_from_the_records_and_their_sub_trade_marks() {
        let mut split = [trade(1, 100.0, 5, true), trade(1, 100.25, 7, true), trade(1, 100.5, 3, true), trade(2, 100.5, 4, false)];
        split[0].open = crate::FIRST_SUB_TRADE_OF_UNBUNDLED_TRADE;
        split[2].open = crate::LAST_SUB_TRADE_OF_UNBUNDLED_TRADE;
        let prints = prints_in(&split, 1.0, 0.25);
        let seen: Vec<(i32, u64, Aggressor, bool)> = prints.iter().map(|p| (p.tick, p.size(), p.side(), p.continues())).collect();
        assert_eq!(
            seen,
            vec![(400, 5, Aggressor::Buy, false), (401, 7, Aggressor::Buy, true), (402, 3, Aggressor::Buy, true), (402, 4, Aggressor::Sell, false)]
        );
        let mut combiner = Combiner::new(true);
        let mut orders: Vec<u64> = prints.iter().filter_map(|p| combiner.push(*p)).map(|o| o.size).collect();
        orders.extend(combiner.flush().map(|o| o.size));
        assert_eq!(orders, vec![15, 4], "the split trade is one order again");
    }

    /// A record of both sides is two prints of one moment, sold first, and
    /// one with volume beyond both has the rest on no side; a record with no
    /// price prints nothing and leaves a split trade open.
    #[test]
    fn a_record_prints_each_side_that_traded_and_a_priceless_one_nothing() {
        let mut both = trade(3, 100.0, 9, true);
        (both.bid_volume, both.ask_volume) = (2, 4);
        let seen: Vec<(u64, Aggressor)> = prints_in(&[both], 1.0, 0.25).iter().map(|p| (p.size(), p.side())).collect();
        assert_eq!(seen, vec![(2, Aggressor::Sell), (4, Aggressor::Buy), (3, Aggressor::Unknown)]);
        assert_eq!(prints_in(&[both], 1.0, 0.25)[0].ts_nanos, 3_000_000_000);

        let mut first = trade(1, 100.0, 1, true);
        first.open = crate::FIRST_SUB_TRADE_OF_UNBUNDLED_TRADE;
        let mut nan = trade(1, f32::NAN, 1, true);
        nan.open = crate::LAST_SUB_TRADE_OF_UNBUNDLED_TRADE;
        let prints = prints_in(&[first, nan, trade(1, 100.25, 1, true)], 1.0, 0.25);
        assert_eq!(prints.iter().map(|p| p.continues()).collect::<Vec<_>>(), vec![false, true]);
        assert_eq!(trades(&nan, f64::NAN, 0.25), None);
    }
}
