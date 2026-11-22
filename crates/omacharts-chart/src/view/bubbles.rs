//! Large orders, drawn: a bubble at each one's price on the bar it printed
//! in, its area in proportion to the order against the largest it is
//! compared with.
//!
//! The book is the window's, built off this thread and handed over whole
//! ([`super::LargeOrderBooks`]); drawing finds the orders on screen by
//! binary search and reads the largest of them once each time the bars on
//! screen change, never per frame.

use std::cell::Cell;

use gtk::cairo;
use omacharts_engine::indicators::flow::{self, Compare};
use omacharts_engine::studies::orders::{bubble_share, Aggressor, Book};
use omacharts_engine::Params;

use super::{Columns, Drawn, State};
use crate::colors;

/// The smallest bubble's radius, in pixels, and the largest's: never so
/// big that one hides a stretch of candles.
const MIN_RADIUS: f64 = 3.0;
const MAX_RADIUS: f64 = 22.0;

/// The largest order on screen, for the span and book it was read for.
#[derive(Clone, Copy, Default, PartialEq, Debug)]
pub(super) struct VisibleLargest {
    from: i64,
    to: i64,
    book: usize,
    largest: u64,
}

thread_local! {
    static VISIBLE: Cell<VisibleLargest> = const { Cell::new(VisibleLargest { from: 0, to: 0, book: 0, largest: 0 }) };
}

/// A bubble's radius for an order of `size` against `largest`.
#[inline]
pub(super) fn radius(size: u64, largest: u64, bar_px: f64) -> f64 {
    // Narrow bars, smaller bubbles: never wider than a few bars.
    let most = MAX_RADIUS.min((bar_px * 3.0).max(MIN_RADIUS + 3.0));
    MIN_RADIUS + (most - MIN_RADIUS) * bubble_share(size, largest)
}

/// Draw `drawn`'s bubbles over the bars on screen.
pub(super) fn draw(cr: &cairo::Context, state: &State, drawn: &Drawn, columns: &Columns, to_y: &impl Fn(f64) -> f64) {
    let Params::LargeOrders { regular, extended, compare, combine, inks } = &drawn.indicator.params else { return };
    let Some(books) = state.large_orders.as_ref() else { return };
    let Some((book, tick_size)) = books(*regular, *extended, *combine) else { return };
    let (first, end) = (columns.first, (columns.first + columns.visible).min(state.bars.len()));
    if first >= end {
        return;
    }
    let bars = &state.bars[..end];
    let from = bars[first].ts;
    let to = match bars.get(end) {
        Some(next) => next.ts,
        None => state.timeframe.shift(bars[end - 1].ts, 1),
    };
    let orders = book.within(from.saturating_mul(1_000_000_000), to.saturating_mul(1_000_000_000));
    if orders.is_empty() {
        return;
    }
    let largest_visible = || {
        let key = (from, to, std::sync::Arc::as_ptr(&book) as usize);
        VISIBLE.with(|cell| {
            let was = cell.get();
            if (was.from, was.to, was.book) == key {
                return was.largest;
            }
            let largest = Book::largest_in(orders);
            cell.set(VisibleLargest { from, to, book: key.2, largest });
            largest
        })
    };
    let visible = match compare {
        Compare::Visible => largest_visible(),
        _ => 0,
    };
    let (buy, sell) = super::footprint::inks(state, inks);
    let (buy_alpha, sell_alpha) = inks.alphas(flow::LARGE_ORDERS_ALPHA);
    cr.save().ok();
    cr.set_line_width(1.0);
    // Two passes, a side each, so each colour is set once.
    for (side, ink, alpha) in [(true, &buy, buy_alpha), (false, &sell, sell_alpha)] {
        for placed in orders {
            let bought = placed.order.side != Aggressor::Sell;
            if bought != side {
                continue;
            }
            let seconds = placed.order.ts_nanos.div_euclid(1_000_000_000);
            let bar = bars.partition_point(|b| b.ts <= seconds).saturating_sub(1).max(first);
            let x = super::bar_left(columns, bar) + columns.bar_px / 2.0;
            let y = to_y(placed.order.tick * tick_size);
            let largest = match compare {
                Compare::Visible => visible,
                Compare::Session => book.largest_for(placed, false),
                Compare::Hours => book.largest_for(placed, true),
            };
            let r = radius(placed.order.size, largest, columns.bar_px);
            cr.new_sub_path();
            cr.arc(x, y, r, 0.0, std::f64::consts::TAU);
        }
        colors::set_source_alpha(cr, ink, alpha);
        let _ = cr.fill_preserve();
        // A rim a little stronger than the fill, so overlapping bubbles stay
        // apart.
        colors::set_source_alpha(cr, ink, (alpha + 0.3).min(1.0));
        let _ = cr.stroke();
    }
    cr.restore().ok();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bubble_s_area_follows_its_order_and_stays_within_a_few_bars() {
        assert_eq!(radius(100, 100, 20.0), MAX_RADIUS);
        assert_eq!(radius(0, 100, 20.0), MIN_RADIUS);
        let quarter = radius(25, 100, 20.0) - MIN_RADIUS;
        assert!((quarter - (MAX_RADIUS - MIN_RADIUS) / 2.0).abs() < 1e-9, "a quarter of the order is half the reach");
        assert_eq!(radius(100, 100, 2.0), MIN_RADIUS + 3.0, "narrow bars, small bubbles");
    }
}
