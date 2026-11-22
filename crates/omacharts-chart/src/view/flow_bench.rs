//! What order flow costs a frame, measured off screen: a chart of minute
//! bars with their ladders held, a footprint, large orders, and a volume
//! delta strip, drawn by the real `draw`.
//!
//! Apart from [`super::bench`] because that file is grafted onto the
//! revision a branch came off by `bin/perf-compare`, and none of what this
//! sets up exists there. These rows are compared within one build instead:
//! a chart with the indicators on against the same chart with them off.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use gtk::cairo;
use omacharts_engine::indicators::flow::Compare;
use omacharts_engine::indicators::{self, Kind, Params};
use omacharts_engine::ladders::{Held, HeldDay, Minute, Side};
use omacharts_engine::studies::orders::{Aggressor, Book, Builder, Print, Thresholds};
use omacharts_engine::{theme, Bar, Indicator, Timeframe};

use super::{draw, draw_pointer, layout, Drawn, State};

/// The tick the synthetic contract trades in.
pub const TICK: f64 = 0.25;

/// A chart of minute bars with order flow on it, and no window.
pub struct FlowScene {
    state: State,
    held: Rc<RefCell<Held>>,
    book: Rc<RefCell<Option<Arc<Book>>>>,
    walk: u64,
}

impl FlowScene {
    /// `bars` minutes, `visible` of them on screen, with the indicators
    /// named by `kinds`; `trades` prints a minute, one in `large_every` of
    /// them a large order.
    pub fn new(bars: usize, visible: usize, kinds: &[&str], trades: usize, large_every: usize) -> FlowScene {
        FlowScene::ending(1_600_000_000, bars, visible, kinds, trades, large_every)
    }

    /// The same, its last minute ending at `end`, unix seconds.
    pub fn ending(end: i64, bars: usize, visible: usize, kinds: &[&str], trades: usize, large_every: usize) -> FlowScene {
        let mut walk = 0x2545_f491_4f6c_dd1du64;
        let start = end.div_euclid(60) * 60 - (bars as i64) * 60;
        let mut price_tick = 24_000i32;
        let mut minutes: Vec<Minute> = Vec::with_capacity(bars);
        let mut prints: Vec<Print> = Vec::with_capacity(bars * trades / large_every.max(1) + 1);
        for n in 0..bars {
            let ts = start + n as i64 * 60;
            let mut minute = Minute::opening(ts, f64::from(price_tick) * TICK);
            for t in 0..trades {
                let step = (next(&mut walk) % 3) as i32 - 1;
                price_tick += step;
                let side = if step >= 0 { Side::Buy } else { Side::Sell };
                let large = large_every > 0 && next(&mut walk).is_multiple_of(large_every as u64);
                let size = if large { 60 + next(&mut walk) % 300 } else { 1 + next(&mut walk) % 8 };
                minute.add_trade(price_tick, f64::from(price_tick) * TICK, size, side, 1);
                if large {
                    let at = ts * 1_000_000_000 + (t as i64) * (60_000_000_000 / trades.max(1) as i64);
                    let aggressor = if side == Side::Buy { Aggressor::Buy } else { Aggressor::Sell };
                    prints.push(Print::new(at, price_tick, size, aggressor, false));
                }
            }
            minutes.push(minute);
        }
        let series: Vec<Bar> = minutes
            .iter()
            .map(|m| Bar { ts: m.ts, open: m.open, high: m.high, low: m.low, close: m.close, volume: m.volume as f64 })
            .collect();
        let mut held = Held::new(TICK);
        let mut day = omacharts_engine::ladders::Day::new(start.div_euclid(86_400));
        for minute in minutes {
            day.insert(minute);
        }
        held.days.extend(HeldDay::whole(day));
        let held = Rc::new(RefCell::new(held));

        let mut builder = Builder::new(true, Thresholds { regular: 50, extended: 50 });
        let mut hours = |seconds: i64| (seconds.div_euclid(86_400), true);
        for print in prints {
            builder.push(print, &mut hours);
        }
        let book = Rc::new(RefCell::new(Some(Arc::new(builder.finish(&mut hours)))));

        let theme = theme::builtin_themes().into_iter().next().expect("a built-in theme");
        let scheme = theme::theme_bars(&theme);
        let indicators: Vec<Indicator> = kinds
            .iter()
            .enumerate()
            .filter_map(|(at, key)| Kind::ALL.into_iter().find(|kind| kind.key() == *key).map(|kind| Indicator::new(at as u32 + 1, kind)))
            .collect();
        let colors = indicators::palette_colors(&indicators, &theme);
        let timeframe = Timeframe::minutes(1);
        let mut state = State::blank(theme, scheme);
        state.timeframe = timeframe;
        state.visible = visible;
        state.overhang = 0;
        {
            let held = held.borrow();
            let context = indicators::Context {
                session_origin: 0,
                timeframe,
                kind: None,
                instrument: None,
                zone: omacharts_engine::clock::ChartZone::default().for_instrument(None),
                ladders: Some(&held),
                awaiting_ladders: false,
                memos: None,
                schedule: None,
            };
            state.indicators = indicators
                .iter()
                .zip(colors)
                .map(|(indicator, color)| Drawn {
                    color,
                    output: indicators::compute_in(indicator, &series, &context),
                    indicator: indicator.clone(),
                })
                .collect();
        }
        state.bars = series;
        let reading = held.clone();
        state.volume_at_price = Some(Rc::new(move |_first, read: &mut dyn FnMut(Option<&Held>, bool)| {
            read(Some(&reading.borrow()), false)
        }));
        let books = book.clone();
        state.large_orders = Some(Rc::new(move |_, _, _| books.borrow().clone().map(|book| (book, TICK))));
        FlowScene { state, held, book, walk }
    }

    /// The indicators worked out again over the bars, as the window does
    /// when a chart's bars arrive, with the ladders held.
    pub fn recompute(&mut self) {
        let held = self.held.borrow();
        let context = indicators::Context {
            session_origin: 0,
            timeframe: self.state.timeframe,
            kind: None,
            instrument: None,
            zone: omacharts_engine::clock::ChartZone::default().for_instrument(None),
            ladders: Some(&held),
            awaiting_ladders: false,
            memos: None,
            schedule: None,
        };
        let series = &self.state.bars;
        for drawn in &mut self.state.indicators {
            drawn.output = indicators::compute_in(&drawn.indicator, series, &context);
        }
    }

    /// Large orders sized against the bubbles on screen rather than their
    /// session: the comparison that is worked out again every frame.
    pub fn compare_against_visible(&mut self) {
        for drawn in &mut self.state.indicators {
            if let Params::LargeOrders { compare, .. } = &mut drawn.indicator.params {
                *compare = Compare::Visible;
            }
        }
        self.recompute();
    }

    /// No book yet: a chart whose trades are still being read.
    pub fn without_book(&self) {
        self.book.replace(None);
    }

    /// Put the pointer where a hand holds it.
    pub fn point_at(&mut self, width: f64, height: f64, across: f64, down: f64) {
        self.state.pointer = Some((width * across, height * down));
    }

    /// The book drawn, swapped for `book`, as the window does when one comes.
    pub fn swap_book(&self, book: Arc<Book>) {
        self.book.replace(Some(book));
    }

    /// One trade on the forming minute: the newest bar and its ladder move,
    /// as a tick on a live chart moves them.
    pub fn trade(&mut self) {
        let Some(last) = self.state.bars.last_mut() else { return };
        let step = (next(&mut self.walk) % 3) as i32 - 1;
        let tick = ((last.close / TICK).round() as i32 + step).clamp((last.low / TICK) as i32 - 4, (last.high / TICK) as i32 + 4);
        let price = f64::from(tick) * TICK;
        let size = 1 + next(&mut self.walk) % 8;
        last.close = price;
        last.high = last.high.max(price);
        last.low = last.low.min(price);
        last.volume += size as f64;
        let ts = last.ts;
        let mut held = self.held.borrow_mut();
        let forming = held.forming.get_or_insert_with(|| Minute::opening(ts, price));
        forming.add_trade(tick, price, size, if step >= 0 { Side::Buy } else { Side::Sell }, 1);
    }

    /// Every indicator left out of the legend, so two charts that differ
    /// only by one indicator draw the same words.
    pub fn without_legend(mut self) -> FlowScene {
        for drawn in &mut self.state.indicators {
            drawn.indicator.in_legend = false;
        }
        self
    }

    /// The footprint's figures written in its rows, or left out.
    pub fn footprint_numbers(mut self, on: bool) -> FlowScene {
        for drawn in &mut self.state.indicators {
            if let Params::Footprint { numbers, .. } = &mut drawn.indicator.params {
                *numbers = on;
            }
        }
        self
    }

    /// Each footprint's volume profile on, with its totals written or not.
    pub fn footprint_profile(mut self, numbers: bool) -> FlowScene {
        for drawn in &mut self.state.indicators {
            if let Params::Footprint { profile, .. } = &mut drawn.indicator.params {
                (profile.on, profile.numbers) = (true, numbers);
            }
        }
        self
    }

    /// The chart's bars drawn in `style`, rising in `up` and falling in
    /// `down`, filled.
    pub fn bars(mut self, style: omacharts_engine::bars::BarStyle, up: &str, down: &str) -> FlowScene {
        self.state.bar_style = style;
        let scheme = &mut self.state.scheme;
        (scheme.up, scheme.up_fill, scheme.down, scheme.down_fill) = (up.into(), up.into(), down.into(), down.into());
        self
    }

    /// The footprint painted in `colouring`.
    pub fn footprint_colouring(mut self, colouring: omacharts_engine::theme::BarColouring) -> FlowScene {
        for drawn in &mut self.state.indicators {
            if let Params::Footprint { inks, .. } = &mut drawn.indicator.params {
                inks.colouring = Some(colouring.into());
            }
        }
        self
    }

    /// What a pointer motion costs before anything is repainted.
    pub fn motion(&self, width: f64, height: f64) -> usize {
        let state = &self.state;
        let (x, y) = state.pointer.unwrap_or((width / 2.0, height / 2.0));
        let plan = layout(state, width, height);
        usize::from(plan.edge_at(y).is_some()) + usize::from(plan.control_at(x, y).is_some())
    }

    /// The chart as the scene holds it, for a test to look into.
    #[cfg(test)]
    pub(super) fn state(&self) -> &State {
        &self.state
    }

    pub fn view_frame(&self, cr: &cairo::Context, width: f64, height: f64) {
        draw(cr, width, height, &self.state);
    }

    pub fn pointer_frame(&self, cr: &cairo::Context, width: f64, height: f64) {
        draw_pointer(cr, width, height, &self.state);
    }

    /// How many large orders the book holds.
    pub fn large_orders(&self) -> usize {
        self.book.borrow().as_ref().map_or(0, |book| book.orders.len())
    }
}

fn next(seed: &mut u64) -> u64 {
    *seed ^= *seed << 13;
    *seed ^= *seed >> 7;
    *seed ^= *seed << 17;
    *seed
}
