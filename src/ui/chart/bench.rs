//! What a frame costs, measured off screen.
//!
//! The draw function is the thing being measured, so this drives the real one
//! rather than a copy of it: a harness that reimplements the drawing measures
//! the harness. It builds the state a live chart holds — a series, a volume
//! strip, two moving averages and an RSI — and hands it to the same `draw` the
//! widget's draw callback calls, onto an image surface instead of a window.
//!
//! What it cannot see is everything outside cairo: GSK turning the frame into
//! a render node, the texture upload, the compositor. Those are real costs and
//! they are not here. What is here is the part that grows with the number of
//! bars on screen, which is the part a chart gets slow by.
//!
//! It also answers the pointer's questions without a pointer — the hit tests a
//! motion event sets off, which is the other half of what moving a hand over a
//! chart costs.
//!
//! Public because the measurements themselves live in `examples/frame_bench.rs`
//! and `examples/perf.rs`, and an example is an outside caller like any other.

use gtk::cairo;
use omacharts_engine::drawings::{Anchor, Drawing, Kind as DrawingKind};
use omacharts_engine::indicators::{self, Kind};
use omacharts_engine::{theme, Bar, Indicator, Timeframe};

use super::{draw, draw_pointer, layout, Drawn, State};

/// A chart set up to be drawn, with no window under it.
pub struct Scene {
    state: State,
}

impl Scene {
    /// A chart holding `bars` bars with `visible` of them on screen, wearing
    /// the furniture a chart usually wears.
    pub fn new(bars: usize, visible: usize) -> Scene {
        let series = series(bars);
        let theme = theme::builtin_themes().into_iter().next().expect("a built-in theme");
        let scheme = theme::theme_bars(&theme);

        let indicators: Vec<Indicator> = [Kind::Volume, Kind::Sma, Kind::Ema, Kind::Rsi]
            .into_iter()
            .enumerate()
            .map(|(at, kind)| Indicator::new(at as u32 + 1, kind))
            .collect();
        let colors = indicators::palette_colors(&indicators, &theme);
        let timeframe = Timeframe::days(1);
        let drawn: Vec<Drawn> = indicators
            .iter()
            .zip(colors)
            .map(|(indicator, color)| Drawn {
                color,
                output: indicators::compute(indicator, &series, 0, timeframe, None),
                indicator: indicator.clone(),
            })
            .collect();

        let mut state = State::blank(theme, scheme);
        state.bars = series;
        state.indicators = drawn;
        state.timeframe = timeframe;
        state.visible = visible;
        Scene { state }
    }

    /// What a drained tick costs a chart: the tail put in place, and the
    /// indicators recomputed over the series — which is the whole of the
    /// per-frame work the window does for a chart whose bars moved, apart
    /// from the frame itself.
    pub fn tick(&mut self, tail: &[Bar]) {
        for bar in tail {
            omacharts_engine::stream::upsert(&mut self.state.bars, *bar);
        }
        let timeframe = self.state.timeframe;
        let series = &self.state.bars;
        for drawn in &mut self.state.indicators {
            drawn.output = indicators::compute(&drawn.indicator, series, 0, timeframe, None);
        }
    }

    /// The last bar as the chart holds it, for a benchmark to move.
    pub fn last_bar(&self) -> Option<Bar> {
        self.state.bars.last().copied()
    }

    /// Put the pointer where a hand holds it, as a fraction across and down
    /// the chart.
    pub fn point_at(&mut self, width: f64, height: f64, across: f64, down: f64) {
        self.state.pointer = Some((width * across, height * down));
    }

    /// Mark the chart up with `count` drawings, spread over what is on screen.
    ///
    /// A pointer motion hit-tests every drawing on the chart, so the number of
    /// them is what makes moving the hand expensive. An empty chart is the
    /// cheap case and not the one worth watching; a chart somebody has been
    /// marking up for a month is.
    ///
    /// A mix of kinds, because they are not hit-tested the same way — a box
    /// tests its four edges, a level tests one line across the plot — and the
    /// last one is selected, which is the state that also tests grips.
    pub fn mark_up(&mut self, count: usize) {
        const KINDS: [DrawingKind; 5] = [
            DrawingKind::Line,
            DrawingKind::Horizontal,
            DrawingKind::Rect,
            DrawingKind::Ellipse,
            DrawingKind::Arrow,
        ];
        let (first, visible) = (self.state.first, self.state.visible.max(1));
        let mut drawings = Vec::with_capacity(count);
        for n in 0..count {
            // Walked by a stride that is prime to the span, so they land all
            // over the visible bars rather than piling up in one place.
            let from = self.state.bars.get(first + (n * 37) % visible);
            let to = self.state.bars.get(first + (n * 37 + 11) % visible);
            let (Some(from), Some(to)) = (from, to) else {
                break;
            };
            let mut drawing = Drawing::new(
                KINDS[n % KINDS.len()],
                Anchor::new(from.ts, from.low),
                Anchor::new(to.ts, to.high),
            );
            drawing.order = n as i64;
            drawings.push(drawing);
        }
        self.state.selected = drawings.len().checked_sub(1).into_iter().collect();
        self.state.drawings = drawings;
    }

    /// What a pointer motion costs before anything is repainted.
    ///
    /// The three questions the cursor asks of where the hand is: is it on a
    /// pane divider, on one of a pane's corner boxes, on a drawing. That is
    /// the work the motion handler does before it asks for the crosshair
    /// layer, which is [`Scene::pointer_frame`].
    ///
    /// Not the crosshair readout, which is the one thing a motion does that is
    /// not here. It costs a layout and a price range, and both of those are
    /// paid by the hit test on the line above — so leaving it out keeps this
    /// whole harness inside two files that can be grafted onto an older
    /// revision, which is what `bin/perf-compare` needs of it, at the price of
    /// a little precision in one row.
    ///
    /// Answers with how many of the three found something, so that nothing
    /// here can be optimised away as a result nobody reads.
    pub fn motion(&self, width: f64, height: f64) -> usize {
        let state = &self.state;
        let (x, y) = state.pointer.unwrap_or((width / 2.0, height / 2.0));
        let plan = layout(state, width, height);
        usize::from(plan.edge_at(y).is_some())
            + usize::from(plan.control_at(x, y).is_some())
            + usize::from(state.drawing_at(width, height, x, y).is_some())
    }

    /// What a pan or a zoom repaints: the chart body, every bar of it.
    pub fn view_frame(&self, cr: &cairo::Context, width: f64, height: f64) {
        draw(cr, width, height, &self.state);
    }

    /// What moving the pointer repaints: the layer the crosshair is on, and
    /// nothing under it.
    pub fn pointer_frame(&self, cr: &cairo::Context, width: f64, height: f64) {
        draw_pointer(cr, width, height, &self.state);
    }
}

/// A series with a market's shape: a random walk with a day's range around
/// each close, and volume that varies the way volume does.
///
/// Deterministic, so two runs of the benchmark are drawing the same picture
/// and their numbers can be compared. Public so that the one generator serves
/// every benchmark: bars that differ between two measurements are two
/// measurements that cannot be put beside each other.
pub fn series(count: usize) -> Vec<Bar> {
    let mut walk = Walk { seed: 0x2545_f491_4f6c_dd1d };
    let mut bars = Vec::with_capacity(count);
    let mut price = 100.0f64;
    // Daily bars ending at a fixed instant, so the axis labels come out as the
    // same strings every run.
    let mut ts = 1_600_000_000i64 - count as i64 * 86_400;
    for _ in 0..count {
        let open = price;
        let close = (open * (1.0 + (walk.next() - 0.5) * 0.04)).max(1.0);
        let reach = open.max(close) * (1.0 + walk.next() * 0.012);
        let floor = open.min(close) * (1.0 - walk.next() * 0.012);
        bars.push(Bar {
            ts,
            open,
            high: reach,
            low: floor,
            close,
            volume: 1_000_000.0 * (0.3 + walk.next()),
        });
        price = close;
        ts += 86_400;
    }
    bars
}

/// xorshift64, which is three lines and needs no dependency.
struct Walk {
    seed: u64,
}

impl Walk {
    /// The next number in 0..1.
    fn next(&mut self) -> f64 {
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 7;
        self.seed ^= self.seed << 17;
        (self.seed >> 11) as f64 / (1u64 << 53) as f64
    }
}
