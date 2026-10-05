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
//! Public because the measurement itself lives in `examples/frame_bench.rs`,
//! and an example is an outside caller like any other.

use gtk::cairo;
use omacharts_engine::indicators::{self, Kind};
use omacharts_engine::{theme, Bar, Indicator, Timeframe};

use super::{draw, Drawn, State};

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

    /// Put the pointer where a hand holds it, as a fraction across and down
    /// the chart.
    pub fn point_at(&mut self, width: f64, height: f64, across: f64, down: f64) {
        self.state.pointer = Some((width * across, height * down));
    }

    /// What a pan or a zoom repaints: the chart body, every bar of it.
    pub fn view_frame(&self, cr: &cairo::Context, width: f64, height: f64) {
        draw(cr, width, height, &self.state);
    }

    /// What moving the pointer repaints.
    ///
    /// The whole body, because a motion event invalidates the whole widget and
    /// GTK4 has no way to invalidate less of one.
    pub fn pointer_frame(&self, cr: &cairo::Context, width: f64, height: f64) {
        draw(cr, width, height, &self.state);
    }
}

/// A series with a market's shape: a random walk with a day's range around
/// each close, and volume that varies the way volume does.
///
/// Deterministic, so two runs of the benchmark are drawing the same picture
/// and their numbers can be compared.
fn series(count: usize) -> Vec<Bar> {
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
