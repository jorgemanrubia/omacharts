//! Past fills on the chart: a small triangle at each fill's bar and price,
//! and a dotted line from each entry to the exit that closed it.
//!
//! The chart only draws them. Which fills there are, which of them opened
//! a position and which closed one, and which entry each exit closed is the
//! embedder's to say; it hands them over sorted by time, and they are drawn
//! as given until it hands over others.
//!
//! The look is Sierra Chart's. An entry stands to the left of its bar and
//! points right, at it; an exit stands to the right and points left, so
//! the two ends of a trip face each other across the bars they held, and a
//! fill that closed one way and opened the other wears both. A buy is the
//! theme's colour for a rise and a sell its colour for a fall, as the
//! candles have them. The tip is at the fill's price and sits off the
//! bar's body, so neither hides the other.
//!
//! Cheap in the frame: two searches find the fills in view and only those
//! are drawn, each placed with a search of the bars. Nothing is worked out
//! per bar, and a chart with none draws nothing at all.

use gtk::cairo;

use super::{colors, index_of_ts, Layout, PriceScale, State};

/// One fill, as the chart marks it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ExecutionMark {
    /// Unix seconds.
    pub ts: i64,
    pub price: f64,
    pub buy: bool,
    /// It opened a position or added to one: a triangle pointing right.
    pub opens: bool,
    /// It closed a position or took some off: a triangle pointing left.
    pub closes: bool,
}

/// A round trip, entry to exit, as the dotted line joining them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TripLine {
    /// When and where the entry filled.
    pub from: (i64, f64),
    /// When and where the exit filled.
    pub to: (i64, f64),
    /// Won, lost, or `None` for one closed where it opened.
    pub won: Option<bool>,
}

/// The fills a chart marks and the trips that join them, with their
/// colours. Empty, the default, draws nothing.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Executions {
    /// Oldest first.
    pub marks: Vec<ExecutionMark>,
    /// By when the entry filled, oldest first.
    pub trips: Vec<TripLine>,
    pub buy: String,
    pub sell: String,
    /// A trip's line: won, lost, and closed where it opened.
    pub won: String,
    pub lost: String,
    pub even: String,
}

impl Executions {
    pub fn is_empty(&self) -> bool {
        self.marks.is_empty() && self.trips.is_empty()
    }
}

/// How long a triangle is, tip to base, and how far it stands each side of
/// its middle, in pixels.
const LENGTH: f64 = 6.0;
const HALF: f64 = 3.5;

/// The gap between a triangle's tip and its bar's body, and how far out
/// from its middle the body reaches, as a share of the column.
const STAND_OFF: f64 = 1.0;
const BODY_HALF: f64 = 0.35;

/// The trip line's weight and its dots.
const TRIP_WEIGHT: f64 = 1.0;
const TRIP_DASH: [f64; 2] = [1.5, 2.5];

/// How strongly the trip line is drawn: under the triangles, quieter than
/// they are, so a busy day reads as its fills first.
const TRIP_ALPHA: f64 = 0.85;

/// Where a fill falls among the bars: the index of the bar it is part of,
/// or `None` when it falls in a gap between two, out of the hours the
/// chart shows, or before the first. One past the last bar is the last:
/// a fill heard before the bar it is part of.
fn bar_of(state: &State, ts: i64) -> Option<usize> {
    let after = state.bars.partition_point(|bar| bar.ts <= ts);
    let at = after.checked_sub(1)?;
    let last = after == state.bars.len();
    (last || ts < state.timeframe.shift(state.bars[at].ts, 1)).then_some(at)
}

/// Where a trip's end goes across the plot: the middle of its fill's bar,
/// or, for one off the bars, where its time would be.
fn x_of(state: &State, plan: &Layout, ts: i64) -> f64 {
    let index = match bar_of(state, ts) {
        Some(at) => at as f64,
        None => index_of_ts(&state.bars, ts),
    };
    state.column_edge(plan, index + 0.5)
}

/// Draw the fills and the trips in view, inside the price plot.
pub(super) fn draw(cr: &cairo::Context, state: &State, plan: &Layout, scale: &PriceScale) {
    let executions = &state.executions;
    if executions.is_empty() {
        return;
    }
    let (first, visible, _) = state.view();
    let (Some(first_bar), Some(last_bar)) = (state.bars.get(first), state.bars.get(first + visible.max(1) - 1)) else {
        return;
    };
    // The moments in view, from the first bar's start to the last's end;
    // the last bar on the chart reaches as far as anything heard.
    let from = first_bar.ts;
    let to = match first + visible >= state.bars.len() {
        true => i64::MAX,
        false => state.timeframe.shift(last_bar.ts, 1),
    };
    let bar_w = plan.plot_w / state.columns().max(1) as f64;
    let to_y = |price: f64| scale.to_y(price, plan.price_y, plan.price_h);

    cr.save().ok();
    cr.rectangle(plan.plot_x, plan.price_y, plan.plot_w, plan.price_h);
    cr.clip();

    // The trips first, under the triangles: any with an end in view or
    // spanning it. They are sorted by entry, so those that opened after the
    // view are never looked at.
    let opened_by = executions.trips.partition_point(|trip| trip.from.0 < to);
    cr.set_line_width(TRIP_WEIGHT);
    cr.set_dash(&TRIP_DASH, 0.0);
    for trip in executions.trips[..opened_by].iter().filter(|trip| trip.to.0 >= from) {
        let colour = match trip.won {
            Some(true) => &executions.won,
            Some(false) => &executions.lost,
            None => &executions.even,
        };
        colors::set_source_alpha(cr, colour, TRIP_ALPHA);
        cr.move_to(x_of(state, plan, trip.from.0), to_y(trip.from.1));
        cr.line_to(x_of(state, plan, trip.to.0), to_y(trip.to.1));
        let _ = cr.stroke();
    }
    cr.set_dash(&[], 0.0);

    // Then the fills in view, each beside its bar.
    let start = executions.marks.partition_point(|mark| mark.ts < from);
    let end = executions.marks.partition_point(|mark| mark.ts < to);
    // Just past the body, which is most of the column.
    let off = bar_w * BODY_HALF + STAND_OFF;
    cr.set_line_width(1.0);
    for mark in &executions.marks[start..end] {
        let Some(at) = bar_of(state, mark.ts) else { continue };
        let x = state.column_edge(plan, at as f64 + 0.5);
        let y = to_y(mark.price);
        let colour = if mark.buy { &executions.buy } else { &executions.sell };
        // An entry left of the bar pointing right; an exit right of it
        // pointing left. A fill that was both wears both.
        for (wanted, side) in [(mark.opens, -1.0), (mark.closes, 1.0)] {
            if !wanted {
                continue;
            }
            let tip = x + side * off;
            cr.move_to(tip, y);
            cr.line_to(tip + side * LENGTH, y - HALF);
            cr.line_to(tip + side * LENGTH, y + HALF);
            cr.close_path();
            colors::set_source(cr, colour);
            let _ = cr.fill_preserve();
            // A hairline of the ground round it, so it reads over a candle
            // of its own colour.
            colors::set_source_alpha(cr, &state.theme.ui.background, 0.9);
            cr.set_line_width(0.75);
            let _ = cr.stroke();
        }
    }
    cr.restore().ok();
}

#[cfg(test)]
mod tests {
    use omacharts_engine::{Bar, Timeframe};

    use super::*;

    /// Hourly bars at 09:00, 10:00 and 11:00 of a day, then 09:00 the next:
    /// the night between is a gap no bar covers.
    fn state() -> State {
        let theme = omacharts_engine::theme::builtin_themes().into_iter().next().expect("a built-in theme");
        let scheme = omacharts_engine::theme::theme_bars(&theme);
        let mut state = State::blank(theme, scheme);
        state.timeframe = Timeframe::hours(1);
        let day = 1_600_000_000 - 1_600_000_000 % 86_400 + 9 * 3_600;
        state.bars = [day, day + 3_600, day + 7_200, day + 86_400]
            .into_iter()
            .map(|ts| Bar { ts, open: 100.0, high: 101.0, low: 99.0, close: 100.5, volume: 1.0 })
            .collect();
        state
    }

    #[test]
    fn a_fill_is_on_the_bar_it_falls_in_and_on_none_in_a_gap() {
        let state = state();
        let day = state.bars[0].ts;
        assert_eq!(bar_of(&state, day - 1), None, "before the first bar");
        assert_eq!(bar_of(&state, day), Some(0));
        assert_eq!(bar_of(&state, day + 3_599), Some(0));
        assert_eq!(bar_of(&state, day + 7_200 + 1_800), Some(2));
        assert_eq!(bar_of(&state, day + 7_200 + 3_600 + 60), None, "overnight, which no bar shows");
        assert_eq!(bar_of(&state, day + 86_400 + 99_999), Some(3), "heard before its bar came: the last");
    }

    /// Fills out of view cost nothing and leave the frame as it was; one in
    /// view is drawn.
    #[test]
    fn only_fills_in_view_are_drawn() {
        let mut state = state();
        let (width, height) = (400.0, 300.0);
        let picture = |state: &State| {
            let mut surface = cairo::ImageSurface::create(cairo::Format::ARgb32, width as i32, height as i32).unwrap();
            super::super::draw(&cairo::Context::new(&surface).unwrap(), width, height, state);
            surface.data().unwrap().to_vec()
        };
        let bare = picture(&state);
        let colours = Executions {
            buy: "#00ff00".into(),
            sell: "#ff0000".into(),
            won: "#00ff00".into(),
            lost: "#ff0000".into(),
            even: "#888888".into(),
            ..Executions::default()
        };
        let long_before = state.bars[0].ts - 30 * 86_400;
        state.executions = Executions {
            marks: vec![ExecutionMark { ts: long_before, price: 100.0, buy: true, opens: true, closes: false }],
            trips: vec![TripLine { from: (long_before, 100.0), to: (long_before + 60, 100.5), won: Some(true) }],
            ..colours.clone()
        };
        assert!(picture(&state) == bare, "a fill out of view was drawn");
        let at = state.bars[1].ts + 60;
        state.executions = Executions {
            marks: vec![ExecutionMark { ts: at, price: 100.0, buy: true, opens: true, closes: false }],
            ..colours
        };
        assert!(picture(&state) != bare, "a fill in view was not drawn");
    }
}
