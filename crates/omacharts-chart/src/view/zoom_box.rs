//! The zoom tool: a box dragged across the plot, and the view laid exactly
//! over it.
//!
//! A box is a stretch of time and a band of prices at once, which is the
//! thing a wheel cannot ask for: the wheel zooms time about the pointer and
//! leaves the prices to fit whatever lands on screen. So the box's left and
//! right edges become the view's, and its top and bottom become a held price
//! scale — held, because a scale that went on fitting the bars would throw
//! the band away the moment it was drawn.
//!
//! Every zoom remembers where the chart was looking first, and stepping back
//! puts each back in turn. The memory is the series': a view kept on one
//! symbol or resolution is in bars and prices another does not have, so it
//! is forgotten when either changes, and when the chart is reset to how it
//! opens. It is not stored, as the view itself is not.

use super::{layout, price_range, State, MAX_VISIBLE};

/// How many views back the chart remembers. Far more than anybody steps
/// back through; there only so a session of zooming cannot grow it forever.
const MEMORY: usize = 64;

/// What a box asks the chart to show: the columns from `left` to `right`,
/// counted from the first bar's, fractions and all, and the prices from the
/// bottom to the top when it holds them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct ZoomWindow {
    pub left: f64,
    pub right: f64,
    pub prices: Option<(f64, f64)>,
}

impl State {
    /// The window a box dragged from `from` to `to`, in pixels, covers on a
    /// chart `width` by `height`: either corner first, and clamped to the
    /// price plot, so a box pulled out over an axis stops at its edge.
    ///
    /// `None` with nothing on the chart, and for a box too thin in either
    /// direction to be anything but a click: under `least` pixels.
    pub(super) fn zoom_box(
        &self,
        width: f64,
        height: f64,
        from: (f64, f64),
        to: (f64, f64),
        least: f64,
    ) -> Option<ZoomWindow> {
        let (first, visible) = self.slice();
        if visible == 0 {
            return None;
        }
        let plan = layout(self, width, height);
        let scale = price_range(self, &self.bars[first..first + visible], &plan)?;
        let clamp_x = |x: f64| x.clamp(plan.plot_x, plan.plot_x + plan.plot_w);
        let clamp_y = |y: f64| y.clamp(plan.price_y, plan.price_y + plan.price_h);
        let (x0, x1) = (clamp_x(from.0.min(to.0)), clamp_x(from.0.max(to.0)));
        let (y0, y1) = (clamp_y(from.1.min(to.1)), clamp_y(from.1.max(to.1)));
        if x1 - x0 < least || y1 - y0 < least {
            return None;
        }
        let bar_w = plan.plot_w / self.columns().max(1) as f64;
        let column = |x: f64| self.left_edge() as f64 + (x - plan.plot_x) / bar_w;
        // The top of the box is the higher price.
        let high = scale.price_at(y0, plan.price_y, plan.price_h);
        let low = scale.price_at(y1, plan.price_y, plan.price_h);
        let prices = (low.is_finite() && high.is_finite() && low < high).then_some((low, high));
        Some(ZoomWindow { left: column(x0), right: column(x1), prices })
    }

    /// The window from the bar at `from` to the bar at `to`, both of them
    /// whole, in either order, with the prices given: what a command asks
    /// for. Moments past either end of the bars are counted at the end's
    /// step, as a drawing's are. `None` with no bars.
    pub(super) fn zoom_span(&self, from: i64, to: i64, prices: Option<(f64, f64)>) -> Option<ZoomWindow> {
        if self.bars.is_empty() {
            return None;
        }
        let (a, b) = (super::index_of_ts(&self.bars, from), super::index_of_ts(&self.bars, to));
        let (left, right) = (a.min(b), a.max(b));
        // A bar's column runs from its index to the next.
        Some(ZoomWindow { left: left.floor(), right: right.floor() + 1.0, prices })
    }

    /// Show `window`, remembering where the chart was looking so
    /// [`zoom_back`](Self::zoom_back) can return there. Says whether it
    /// showed anything.
    ///
    /// The columns are the box's, rounded to whole ones and never fewer than
    /// the closest zoom allows, which then stands centred on the box. The
    /// view stops following the live edge, since what it shows is a stretch
    /// somebody chose; the price scale holds the box's prices when it has
    /// them and is left as it was when it has none.
    pub(super) fn zoom_to(&mut self, window: ZoomWindow) -> bool {
        let ZoomWindow { left, right, prices } = window;
        if self.bars.is_empty() || !left.is_finite() || !right.is_finite() || right <= left {
            return false;
        }
        let Some(before) = self.viewport() else { return false };
        let span = self.span().max(self.fewest());
        let columns = ((right - left).round() as usize).clamp(self.fewest(), MAX_VISIBLE).min(span);
        let middle = (left + right) / 2.0;
        let len = self.bars.len() as i64;
        // At least one bar stays on screen at either end.
        let edge = ((middle - columns as f64 / 2.0).round() as i64).clamp(1 - columns as i64, len - 1);
        self.range = None;
        self.visible = columns;
        self.anchored = false;
        self.overhang = 0;
        self.overhang_by_hand = true;
        self.lead = (-edge).max(0) as usize;
        self.first = edge.max(0) as usize;
        if let Some((low, high)) = prices {
            self.hold_window(low, high);
        }
        self.zoom_memory.push(before);
        if self.zoom_memory.len() > MEMORY {
            self.zoom_memory.remove(0);
        }
        true
    }

    /// Back to where the chart was looking before the last zoom. Says
    /// whether there was one to go back to.
    pub(super) fn zoom_back(&mut self) -> bool {
        let Some(kept) = self.zoom_memory.pop() else { return false };
        self.take_viewport(kept);
        true
    }

    /// How many zooms there are to step back through.
    pub(super) fn zooms_back(&self) -> usize {
        self.zoom_memory.len()
    }

    /// Whatever box is in hand goes, and the memory of earlier views with it:
    /// they were views of a series the chart no longer shows.
    pub(super) fn forget_zooms(&mut self) {
        self.zoom_memory.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use omacharts_engine::Bar;
    use super::super::MIN_VISIBLE;

    /// Where the view's left edge is, how many columns it spans, and the
    /// prices it holds.
    fn shown(state: &State) -> (i64, usize, Option<(f64, f64)>) {
        (state.left_edge(), state.columns(), state.held_window())
    }

    const W: f64 = 900.0;
    const H: f64 = 500.0;

    fn bars(count: usize) -> Vec<Bar> {
        (0..count)
            .map(|i| {
                let base = 100.0 + (i as f64 * 0.3).sin() * 10.0;
                Bar { ts: 1_800_000_000 + i as i64 * 3600, open: base, high: base + 2.0, low: base - 2.0, close: base + 0.5, volume: 10.0 }
            })
            .collect()
    }

    fn charted(count: usize) -> State {
        let theme = omacharts_engine::theme::builtin_themes().into_iter().next().expect("a built-in theme");
        let scheme = omacharts_engine::theme::theme_bars(&theme);
        let mut state = State::blank(theme, scheme);
        state.bars = bars(count);
        state.visible = 100;
        state.instrument = Some(omacharts_engine::Instrument {
            symbol: "FAKE".into(),
            name: "FAKE".into(),
            kind: omacharts_engine::symbols::InstrumentKind::Equity,
            suffix: None,
            currency: None,
            tier: 0,
            session_origin: 0,
            overrides: Vec::new(),
            exchange: None,
            popularity: 0,
            local_name: None,
            formula: None,
        });
        state
    }

    /// The pixel a column's left edge and a price stand at, on the chart as
    /// it is now.
    fn pixel(state: &State, column: f64, price: f64) -> (f64, f64) {
        let plan = layout(state, W, H);
        let (first, visible) = state.slice();
        let scale = price_range(state, &state.bars[first..first + visible], &plan).unwrap();
        let bar_w = plan.plot_w / state.columns() as f64;
        (plan.plot_x + (column - state.left_edge() as f64) * bar_w, scale.to_y(price, plan.price_y, plan.price_h))
    }

    #[test]
    fn a_box_becomes_its_columns_and_its_prices() {
        let state = charted(300);
        // The view is the last 100 columns, with the resting margin past
        // the last bar, a sixteenth of them: 206 to 306.
        assert_eq!(state.left_edge(), 206);
        let (a, b) = (pixel(&state, 220.0, 104.0), pixel(&state, 250.0, 96.0));
        let window = state.zoom_box(W, H, a, b, 4.0).unwrap();
        assert!((window.left - 220.0).abs() < 1e-9, "{window:?}");
        assert!((window.right - 250.0).abs() < 1e-9, "{window:?}");
        let (low, high) = window.prices.unwrap();
        assert!((low - 96.0).abs() < 1e-9 && (high - 104.0).abs() < 1e-9, "{low} {high}");
        // Drawn from any corner, it is the same box.
        assert_eq!(state.zoom_box(W, H, b, a, 4.0), Some(window));
        let (c, d) = ((a.0, b.1), (b.0, a.1));
        assert_eq!(state.zoom_box(W, H, c, d, 4.0), Some(window));
    }

    #[test]
    fn a_box_over_an_axis_stops_at_the_plot() {
        let state = charted(300);
        let plan = layout(&state, W, H);
        let inside = pixel(&state, 280.0, 100.0);
        let window = state.zoom_box(W, H, inside, (W - 1.0, H - 1.0), 4.0).unwrap();
        assert!((window.right - 306.0).abs() < 1e-9, "the plot's right edge: {window:?}");
        let bottom = {
            let (first, visible) = state.slice();
            let scale = price_range(&state, &state.bars[first..first + visible], &plan).unwrap();
            scale.price_at(plan.price_y + plan.price_h, plan.price_y, plan.price_h)
        };
        assert!((window.prices.unwrap().0 - bottom).abs() < 1e-9);
    }

    #[test]
    fn a_click_or_a_sliver_is_no_box() {
        let state = charted(300);
        let a = pixel(&state, 220.0, 100.0);
        assert_eq!(state.zoom_box(W, H, a, (a.0 + 2.0, a.1 + 2.0), 4.0), None);
        assert_eq!(state.zoom_box(W, H, a, (a.0 + 200.0, a.1 + 2.0), 4.0), None, "wide but flat");
        assert_eq!(state.zoom_box(W, H, a, (a.0 + 2.0, a.1 + 200.0), 4.0), None, "tall but thin");
        assert_eq!(charted(0).zoom_box(W, H, a, (a.0 + 200.0, a.1 + 200.0), 4.0), None, "nothing charted");
    }

    #[test]
    fn zooming_shows_exactly_the_box_and_holds_its_prices() {
        let mut state = charted(300);
        assert!(state.price_auto);
        let window = ZoomWindow { left: 220.0, right: 250.0, prices: Some((96.0, 104.0)) };
        assert!(state.zoom_to(window));
        let (left, columns, held) = shown(&state);
        assert_eq!((left, columns), (220, 30));
        assert!(!state.price_auto, "autoscale is off while zoomed");
        let (low, high) = held.unwrap();
        assert!((low - 96.0).abs() < 1e-9 && (high - 104.0).abs() < 1e-9);
        assert!(!state.anchored, "a chosen stretch does not follow the live edge");
        // And the pixels at the box's corners are its corners now.
        let (x, y) = pixel(&state, 220.0, 104.0);
        let plan = layout(&state, W, H);
        assert!((x - plan.plot_x).abs() < 1e-9 && (y - plan.price_y).abs() < 1e-6, "{x} {y}");
    }

    #[test]
    fn a_box_narrower_than_the_closest_zoom_is_centred() {
        let mut state = charted(300);
        state.zoom_to(ZoomWindow { left: 240.0, right: 244.0, prices: None });
        let (left, columns, held) = shown(&state);
        assert_eq!(columns, MIN_VISIBLE);
        assert_eq!(left, 242 - MIN_VISIBLE as i64 / 2);
        assert_eq!(held, None, "no prices asked for: the scale is left fitting");
    }

    #[test]
    fn a_box_into_the_future_keeps_a_bar_on_screen() {
        let mut state = charted(300);
        state.zoom_to(ZoomWindow { left: 290.0, right: 330.0, prices: None });
        assert_eq!(shown(&state).0, 290);
        state.zoom_to(ZoomWindow { left: 400.0, right: 440.0, prices: None });
        let (first, visible) = state.slice();
        assert!(visible >= 1 && first < 300, "{first} {visible}");
    }

    #[test]
    fn each_step_back_undoes_one_zoom_until_there_are_none() {
        let mut state = charted(300);
        let opened = state.viewport();
        state.zoom_to(ZoomWindow { left: 200.0, right: 280.0, prices: Some((90.0, 110.0)) });
        let once = state.viewport();
        state.zoom_to(ZoomWindow { left: 220.0, right: 250.0, prices: Some((96.0, 104.0)) });
        assert_eq!(state.zooms_back(), 2);

        assert!(state.zoom_back());
        assert_eq!(state.viewport(), once);
        assert!(state.zoom_back());
        assert_eq!(state.viewport(), opened);
        assert!(state.price_auto, "back to fitting, as it opened");
        assert!(state.anchored, "and following the live edge again");

        assert!(!state.zoom_back(), "nothing further back");
        assert_eq!(state.viewport(), opened);
    }

    #[test]
    fn a_command_window_takes_its_first_and_last_bars_whole() {
        let state = charted(300);
        let ts = |i: usize| state.bars[i].ts;
        let window = state.zoom_span(ts(250), ts(220), Some((1.0, 2.0))).unwrap();
        assert_eq!((window.left, window.right), (220.0, 251.0));
        assert_eq!(window.prices, Some((1.0, 2.0)));
        assert_eq!(charted(0).zoom_span(1, 2, None), None);
    }

    #[test]
    fn the_memory_is_bounded() {
        let mut state = charted(300);
        for n in 0..(MEMORY + 10) {
            state.zoom_to(ZoomWindow { left: 100.0 + n as f64, right: 150.0 + n as f64, prices: None });
        }
        assert_eq!(state.zooms_back(), MEMORY);
        state.forget_zooms();
        assert_eq!(state.zooms_back(), 0);
    }
}
