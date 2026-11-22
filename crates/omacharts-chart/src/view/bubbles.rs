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
    static SPRITES: std::cell::RefCell<Sprites> = std::cell::RefCell::new(Sprites::default());
    static VISIBLE: Cell<VisibleLargest> = const { Cell::new(VisibleLargest { from: 0, to: 0, book: 0, largest: 0 }) };
}

/// A bubble's radius for an order of `size` against `largest`.
#[inline]
pub(super) fn radius(size: u64, largest: u64, bar_px: f64) -> f64 {
    // Narrow bars, smaller bubbles: never wider than three bars.
    let most = MAX_RADIUS.min((bar_px * 1.5).max(MIN_RADIUS + 3.0));
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
    let (buy, sell) = (colors::rgba(&buy), colors::rgba(&sell));
    let (buy_alpha, sell_alpha) = inks.alphas(flow::LARGE_ORDERS_ALPHA);
    let scale = cr.target().device_scale().0.max(1.0);
    SPRITES.with(|sprites| {
        let mut sprites = sprites.borrow_mut();
        for placed in orders {
            let bought = placed.order.side != Aggressor::Sell;
            let (ink, alpha) = if bought { (buy, buy_alpha) } else { (sell, sell_alpha) };
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
            sprites.paint(cr, ink, alpha, r, x, y, scale);
        }
    });
}

/// How finely a bubble's radius is drawn: a quarter of a pixel, so that a
/// day of them is a few dozen sprites.
type Rgba = (f64, f64, f64, f64);

/// How much more solid a bubble's rim is than its fill.
const RIM: f64 = 0.5;

const RADIUS_STEP: f64 = 0.25;

/// Bubbles drawn once and stamped: a filled circle with its rim, for each
/// colour, opacity and radius a frame has asked for.
///
/// A thousand circles filled as one path is a thousand overlapping edges
/// for cairo to sort, tens of milliseconds a frame; a thousand filled one
/// by one is a thousand tessellations. A stamp is a copy of a few hundred
/// pixels, the same picture, each bubble over the ones before it.
#[derive(Default)]
struct Sprites {
    made: std::collections::HashMap<([u64; 4], u64, u32, u64), cairo::ImageSurface>,
}

impl Sprites {
    /// The most sprites kept: colours or zoom changing often start afresh.
    const MOST: usize = 512;

    #[allow(clippy::too_many_arguments)]
    fn paint(&mut self, cr: &cairo::Context, ink: Rgba, alpha: f64, r: f64, x: f64, y: f64, scale: f64) {
        let step = (r / RADIUS_STEP).round() as u32;
        let key = ([ink.0.to_bits(), ink.1.to_bits(), ink.2.to_bits(), ink.3.to_bits()], alpha.to_bits(), step, scale.to_bits());
        if !self.made.contains_key(&key) {
            if self.made.len() >= Self::MOST {
                self.made.clear();
            }
            let Some(sprite) = sprite(ink, alpha, f64::from(step) * RADIUS_STEP, scale) else { return };
            self.made.insert(key, sprite);
        }
        let sprite = &self.made[&key];
        // On whole device pixels, so the stamp is a copy and not a resample.
        let half = f64::from(sprite.width()) / scale / 2.0;
        let left = ((x - half) * scale).round() / scale;
        let top = ((y - half) * scale).round() / scale;
        let _ = cr.set_source_surface(sprite, left, top);
        let _ = cr.paint();
    }
}

fn sprite((red, green, blue, opaque): Rgba, alpha: f64, r: f64, scale: f64) -> Option<cairo::ImageSurface> {
    // The radius and the rim's half width, and a pixel to spare.
    let side = ((r + 1.5) * 2.0 * scale).ceil() as i32;
    let surface = cairo::ImageSurface::create(cairo::Format::ARgb32, side, side).ok()?;
    surface.set_device_scale(scale, scale);
    let cr = cairo::Context::new(&surface).ok()?;
    let centre = f64::from(side) / scale / 2.0;
    cr.arc(centre, centre, r, 0.0, std::f64::consts::TAU);
    cr.set_source_rgba(red, green, blue, opaque * alpha);
    cr.fill_preserve().ok()?;
    // A rim much stronger than the fill: the rim carries the shape, so the
    // fill can stay light enough not to stain the candles under it, and
    // overlapping bubbles stay apart.
    cr.set_line_width(1.0);
    cr.set_source_rgba(red, green, blue, opaque * (alpha + RIM).min(1.0));
    cr.stroke().ok()?;
    drop(cr);
    surface.flush();
    Some(surface)
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
        assert_eq!(radius(100, 100, 10.0), 15.0, "no wider than three bars");
    }

    #[test]
    fn a_bubble_is_stamped_round_its_order_at_any_scale() {
        for scale in [1.0, 2.0] {
            let side = (40.0 * scale) as i32;
            let mut surface = cairo::ImageSurface::create(cairo::Format::ARgb32, side, side).unwrap();
            surface.set_device_scale(scale, scale);
            {
                let cr = cairo::Context::new(&surface).unwrap();
                let mut sprites = Sprites::default();
                sprites.paint(&cr, (1.0, 0.0, 0.0, 1.0), 0.5, 8.0, 20.0, 20.0, scale);
                sprites.paint(&cr, (1.0, 0.0, 0.0, 1.0), 0.5, 8.1, 20.0, 20.0, scale);
                assert_eq!(sprites.made.len(), 1, "radii a hair apart are one sprite");
            }
            surface.flush();
            let stride = surface.stride() as usize;
            let data = surface.data().unwrap();
            let alpha_at = |x: f64, y: f64| data[((y * scale) as usize) * stride + ((x * scale) as usize) * 4 + 3];
            assert!(alpha_at(20.0, 20.0) > 100, "the order's centre is filled at scale {scale}");
            assert!(alpha_at(20.0, 13.5) > 0, "and out to its radius");
            assert_eq!(alpha_at(20.0, 10.0), 0, "and no further");
            assert_eq!(alpha_at(2.0, 2.0), 0);
        }
    }
}
