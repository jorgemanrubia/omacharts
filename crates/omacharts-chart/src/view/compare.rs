//! Other markets drawn over the chart's own, each rebased to 0% at one bar,
//! on a price axis labelled in percent.
//!
//! The chart keeps working in its own prices. A compared market is drawn
//! where the chart's price would be had it moved by the same percentage since
//! the base bar, and the axis, the crosshair and the end labels write those
//! prices as changes from the base. So the scale, its gestures, the drawings
//! and the marks all go on meaning what they meant, and none of them had to be
//! taught about percent.
//!
//! The base is the first bar on screen unless it is pinned, so scrolling and
//! zooming rebase every line: what the chart shows is performance over the
//! window in view. Nothing here runs for a chart that compares nothing — the
//! chart holds no overlay at all then.

use std::cell::RefCell;
use std::rc::Rc;

use gtk::cairo;
use gtk::glib;
use omacharts_engine::compare::{self as engine, Rebase};
use omacharts_engine::{Bar, Timeframe};

use super::{colors, Columns, Handler, Layout, PriceScale, State, AXIS_CHIP_H};

/// One compared market, as the window hands it over.
#[derive(Clone, Debug, PartialEq)]
pub struct CompareLine {
    /// The symbol, as the end label writes it.
    pub label: String,
    pub color: String,
    /// Its bars at the chart's resolution, folded the way the chart's own
    /// are. Empty while they are on their way.
    pub bars: Rc<Vec<Bar>>,
}

/// What the chart compares, and how.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Comparing {
    pub lines: Vec<CompareLine>,
    /// The moment the lines are rebased at, when one was pinned.
    pub from: Option<i64>,
    /// The chart's own bars drawn as a line like the others.
    pub lines_only: bool,
    /// The chart's own symbol as its end label writes it, and its colour as a
    /// line.
    pub label: String,
    pub color: String,
}

/// One market's change at the bar being read, for the legend.
#[derive(Clone, Debug, PartialEq)]
pub struct Reading {
    /// Which line: `None` is the chart's own symbol.
    pub line: Option<usize>,
    /// Nothing while its bars are on their way, or before it begins.
    pub change: Option<f64>,
}

/// Every market's change at one bar, highest first: the hovered bar, or the
/// last on screen with nothing hovered.
#[derive(Clone, Debug, PartialEq)]
pub struct Readout {
    pub readings: Vec<Reading>,
    /// The gap between the axis's gridlines, as a fraction, which says how
    /// many places a change is written to.
    pub step: f64,
}

impl Readout {
    pub fn text(&self, change: f64) -> String {
        engine::percent(change, self.step / 10.0)
    }
}

/// What the chart holds while it compares.
pub(super) struct Overlay {
    pub(super) spec: Comparing,
    /// Each line's closes laid onto the chart's bars, by bar.
    aligned: Vec<Vec<Option<f64>>>,
    /// The chart's resolution, which says whether a pinned base is a day.
    timeframe: Timeframe,
    /// What the legend was last told, so it is told again only when that
    /// changes.
    sent: RefCell<Option<Readout>>,
    on_readout: Handler<dyn Fn(Readout)>,
}

/// The base of one frame: the bar every line is 0% at, and each line's value
/// there.
pub(super) struct Based {
    pub(super) index: usize,
    pub(super) rebase: Rebase,
    bases: Vec<Option<f64>>,
}

impl Overlay {
    pub(super) fn new(spec: Comparing, bars: &[Bar], timeframe: Timeframe, on_readout: Handler<dyn Fn(Readout)>) -> Overlay {
        let mut overlay = Overlay { spec, aligned: Vec::new(), timeframe, sent: RefCell::new(None), on_readout };
        overlay.realign(bars, timeframe);
        overlay
    }

    /// Lay every line onto the chart's bars again, after they changed.
    pub(super) fn realign(&mut self, bars: &[Bar], timeframe: Timeframe) {
        self.timeframe = timeframe;
        self.aligned =
            self.spec.lines.iter().map(|line| engine::align(bars, &line.bars, timeframe)).collect();
    }

    /// The base for a view whose first bar is `first`.
    pub(super) fn based(&self, bars: &[Bar], first: usize) -> Option<Based> {
        let index = engine::base_index(bars, first, self.spec.from, self.timeframe);
        let rebase = Rebase::new(bars.get(index)?.close)?;
        let bases = self.aligned.iter().map(|values| engine::base_value(values, index)).collect();
        Some(Based { index, rebase, bases })
    }

    /// Line `line`'s value at bar `at`, as the price on the chart's axis it
    /// is drawn at.
    fn placed(&self, based: &Based, line: usize, at: usize) -> Option<f64> {
        let value = (*self.aligned.get(line)?.get(at)?)?;
        let base = (*based.bases.get(line)?)?;
        Some(based.rebase.place(value, base))
    }

    /// The lowest and highest prices the frame has to show over the bars from
    /// `first`, `count` of them: `low` and `high`, the bars' own, widened to
    /// take in every compared line.
    pub(super) fn fit(&self, bars: &[Bar], first: usize, count: usize, low: f64, high: f64) -> (f64, f64) {
        let Some(based) = self.based(bars, first) else { return (low, high) };
        let (mut low, mut high) = (low, high);
        for line in 0..self.aligned.len() {
            for at in first..first + count {
                if let Some(price) = self.placed(&based, line, at) {
                    low = low.min(price);
                    high = high.max(price);
                }
            }
        }
        (low, high)
    }

    /// Each market's change at bar `at`, highest first, those with none
    /// last in the order they are compared in.
    fn readings(&self, bars: &[Bar], based: &Based, at: usize) -> Vec<Reading> {
        let own = bars.get(at).map(|bar| based.rebase.change_at(bar.close));
        let mut readings = vec![Reading { line: None, change: own }];
        for line in 0..self.aligned.len() {
            let change = self.placed(based, line, at).map(|price| based.rebase.change_at(price));
            readings.push(Reading { line: Some(line), change });
        }
        readings.sort_by(|a, b| match (a.change, b.change) {
            (Some(a), Some(b)) => b.total_cmp(&a),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => std::cmp::Ordering::Equal,
        });
        readings
    }

    /// Tell the legend what it reads now, if that is not what it was last
    /// told. After the frame rather than during it: the legend is widgets,
    /// and a widget relaid out from inside another's draw is a frame late
    /// and a warning.
    fn tell(&self, readout: Readout) {
        if self.sent.borrow().as_ref() == Some(&readout) {
            return;
        }
        *self.sent.borrow_mut() = Some(readout.clone());
        let handler = self.on_readout.clone();
        glib::idle_add_local_once(move || {
            if let Some(handler) = handler.borrow().as_ref() {
                handler(readout);
            }
        });
    }
}

/// The percent scale a frame's prices are labelled on: changes from the base
/// on a linear axis, ratios to it on a log one, so its gridlines land on round
/// percentages either way.
fn percent_scale(scale: &PriceScale, rebase: Rebase) -> PriceScale {
    let (low, high) = scale.prices();
    match scale.log {
        false => PriceScale { low: rebase.change_at(low), high: rebase.change_at(high), log: false },
        true => PriceScale {
            low: (low / rebase.anchor).max(f64::MIN_POSITIVE).ln(),
            high: (high / rebase.anchor).max(f64::MIN_POSITIVE).ln(),
            log: true,
        },
    }
}

/// The change a percent scale's tick stands for, and the price it is at.
fn tick_change(log: bool, tick: f64) -> f64 {
    match log {
        false => tick,
        true => tick - 1.0,
    }
}

/// The gap between the percent axis's gridlines, as a fraction.
pub(super) fn grid_step(scale: &PriceScale, rebase: Rebase, price_h: f64) -> f64 {
    percent_scale(scale, rebase).step(price_h)
}

/// The percent axis: its gridlines and labels, with 0% — the base — drawn a
/// little firmer than the rest, as the line everything is measured from.
#[allow(clippy::too_many_arguments)]
pub(super) fn draw_grid(
    cr: &cairo::Context,
    state: &State,
    based: &Based,
    plot_x: f64,
    plot_w: f64,
    plot_y: f64,
    price_h: f64,
    scale: &PriceScale,
    to_y: &impl Fn(f64) -> f64,
) {
    let percents = percent_scale(scale, based.rebase);
    let step = percents.step(price_h);
    cr.set_line_width(1.0);
    cr.select_font_face("sans-serif", cairo::FontSlant::Normal, cairo::FontWeight::Normal);
    cr.set_font_size(11.0);
    let ui = &state.theme.ui;
    percents.ticks(price_h, |tick| {
        let change = tick_change(scale.log, tick);
        let y = to_y(based.rebase.price_of(change)).round() + 0.5;
        if y <= plot_y || y >= plot_y + price_h {
            return;
        }
        let zero = change.abs() < step * 1e-6;
        if zero {
            // The base, dashed across the whole plot whether or not the grid
            // is drawn: without it nothing says where 0% is.
            cr.save().ok();
            cr.set_dash(&[4.0, 3.0], 0.0);
            colors::set_source_alpha(cr, &ui.text_muted, 0.45);
            cr.move_to(plot_x, y);
            cr.line_to(plot_x + plot_w, y);
            let _ = cr.stroke();
            cr.restore().ok();
        } else if state.show_grid {
            colors::set_source(cr, &ui.grid);
            cr.move_to(plot_x, y);
            cr.line_to(plot_x + plot_w, y);
            let _ = cr.stroke();
        }
        match zero {
            true => colors::set_source(cr, &ui.text),
            false => colors::set_source_alpha(cr, &ui.text_muted, 0.9),
        }
        cr.move_to(plot_x + plot_w + 6.0, y + 3.5);
        let _ = cr.show_text(&engine::percent(change, step));
    });

    colors::set_source(cr, &ui.axis);
    let x = (plot_x + plot_w).round() + 0.5;
    cr.move_to(x, plot_y);
    cr.line_to(x, plot_y + price_h);
    let _ = cr.stroke();
}

/// The weight of a compared line, and of the chart's own when it is one.
const LINE_W: f64 = 1.6;
const OWN_LINE_W: f64 = 2.0;

/// Every compared line, and the chart's own when it is drawn as one, inside
/// the price box. A pinned base is marked down the plot, faintly: the moment
/// every line is measured from.
pub(super) fn draw_lines(
    cr: &cairo::Context,
    state: &State,
    overlay: &Overlay,
    based: &Based,
    columns: &Columns,
    plan: &Layout,
    to_y: &impl Fn(f64) -> f64,
) {
    if overlay.spec.from.is_some()
        && (columns.first..columns.first + columns.visible).contains(&based.index)
    {
        let x = (columns.left_of(based.index) + columns.bar_px / 2.0).round() + 0.5;
        cr.save().ok();
        cr.set_dash(&[2.0, 4.0], 0.0);
        cr.set_line_width(1.0);
        colors::set_source_alpha(cr, &state.theme.ui.text_muted, 0.5);
        cr.move_to(x, plan.price_y);
        cr.line_to(x, plan.price_y + plan.price_h);
        let _ = cr.stroke();
        cr.restore().ok();
    }
    cr.save().ok();
    cr.set_line_join(cairo::LineJoin::Round);
    cr.set_line_cap(cairo::LineCap::Round);
    // Later lines over earlier ones, and the chart's own over them all.
    for (line, spec) in overlay.spec.lines.iter().enumerate() {
        cr.set_line_width(LINE_W);
        colors::set_source(cr, &spec.color);
        stroke(cr, columns, to_y, |at| overlay.placed(based, line, at));
    }
    cr.restore().ok();
    if state.drawn_as_line() {
        draw_own_line(cr, state, columns, to_y);
    }
}

/// The chart's own bars as one line through their closes, in the theme's
/// text colour: what the Line style draws, and Lines only while comparing.
pub(super) fn draw_own_line(cr: &cairo::Context, state: &State, columns: &Columns, to_y: &impl Fn(f64) -> f64) {
    cr.save().ok();
    cr.set_line_join(cairo::LineJoin::Round);
    cr.set_line_cap(cairo::LineCap::Round);
    cr.set_line_width(OWN_LINE_W);
    colors::set_source(cr, &state.theme.ui.text);
    for (at, bar) in columns.bars.iter().enumerate() {
        let (x, y) = (columns.x(at), to_y(bar.close));
        match at {
            0 => cr.move_to(x, y),
            _ => cr.line_to(x, y),
        }
    }
    // A lone bar is a dot: the round cap of a line no longer than itself.
    if columns.bars.len() == 1 {
        cr.rel_line_to(0.0, 0.0);
    }
    let _ = cr.stroke();
    cr.restore().ok();
}

/// A line through one value a column, read from the last bar the column
/// stands for, broken where there is none.
fn stroke(cr: &cairo::Context, columns: &Columns, to_y: &impl Fn(f64) -> f64, value: impl Fn(usize) -> Option<f64>) {
    let mut drawing = false;
    for at in 0..columns.len() {
        match value(columns.source(at)) {
            Some(price) => {
                let (x, y) = (columns.x(at), to_y(price));
                if drawing {
                    cr.line_to(x, y);
                } else {
                    cr.move_to(x, y);
                    drawing = true;
                }
            }
            None => drawing = false,
        }
    }
    let _ = cr.stroke();
}

/// Where each line ends on screen — the last bar in view — as a chip on the
/// axis with its change, in its colour, and its symbol beside it inside the
/// plot. In place of the last price's tag, which is a price and not a change.
#[allow(clippy::too_many_arguments)]
pub(super) fn draw_end_labels(
    cr: &cairo::Context,
    state: &State,
    overlay: &Overlay,
    based: &Based,
    columns: &Columns,
    plan: &Layout,
    width: f64,
    step: f64,
    to_y: &impl Fn(f64) -> f64,
) {
    let Some(end) = (columns.first + columns.visible).checked_sub(1) else { return };
    let (top, bottom) = (plan.price_y, plan.price_y + plan.price_h);
    // The chart's own first, as it was drawn last.
    let mut ends: Vec<(f64, f64, &str, &str)> = Vec::new();
    if let Some(bar) = state.bars.get(end) {
        ends.push((bar.close, based.rebase.change_at(bar.close), &overlay.spec.label, &overlay.spec.color));
    }
    for (line, spec) in overlay.spec.lines.iter().enumerate() {
        if let Some(price) = (0..=end).rev().take(columns.visible).find_map(|at| overlay.placed(based, line, at)) {
            ends.push((price, based.rebase.change_at(price), &spec.label, &spec.color));
        }
    }
    // Off the plot is left out rather than pinned to its edge, where it
    // would name a change the axis beside it does not show.
    ends.retain(|(price, ..)| {
        let y = to_y(*price);
        y >= top && y <= bottom
    });
    ends.sort_by(|a, b| to_y(a.0).total_cmp(&to_y(b.0)));
    let mut ys: Vec<f64> = ends.iter().map(|(price, ..)| super::below_cover(state, to_y(*price))).collect();
    super::spread_chips(&mut ys, AXIS_CHIP_H + 1.0, bottom);

    let axis_x = plan.plot_x + plan.plot_w;
    cr.select_font_face("sans-serif", cairo::FontSlant::Normal, cairo::FontWeight::Bold);
    cr.set_font_size(10.5);
    let background = &state.theme.ui.background;
    for ((price, change, label, colour), y) in ends.iter().zip(ys) {
        let y = y.round() + 0.5;
        super::label_on_axis(cr, state, &engine::percent(*change, step / 10.0), axis_x, y, width, colour);
        // The symbol just inside the plot, on a plate of the chart's own
        // ground so a line passing under it does not cut through the letters.
        cr.select_font_face("sans-serif", cairo::FontSlant::Normal, cairo::FontWeight::Bold);
        cr.set_font_size(10.5);
        let Ok(extents) = cr.text_extents(label) else { continue };
        let (w, h) = (extents.x_advance() + 8.0, AXIS_CHIP_H - 2.0);
        let x = axis_x - w - 4.0;
        colors::set_source_alpha(cr, background, 0.82);
        super::pill_path(cr, x, y - h / 2.0, w, h);
        let _ = cr.fill();
        colors::set_source(cr, colour);
        cr.move_to(x + 4.0, y + 3.5);
        let _ = cr.show_text(label);
        // A short tick from the line's end to its label, when the spread
        // moved the label off it.
        let at = to_y(*price).round() + 0.5;
        if (at - y).abs() > 2.0 {
            cr.set_line_width(1.0);
            colors::set_source_alpha(cr, colour, 0.6);
            cr.move_to(axis_x - 3.0, at);
            cr.line_to(axis_x, y);
            let _ = cr.stroke();
        }
    }
}

/// What the crosshair writes on the axis: the change at its height.
pub(super) fn crosshair_label(based: &Based, scale: &PriceScale, price_h: f64, price: f64) -> String {
    engine::percent(based.rebase.change_at(price), grid_step(scale, based.rebase, price_h) / 10.0)
}

/// On the layer over the chart: a dot where each line crosses the hovered
/// bar, and the legend told what every market reads there — or at the last
/// bar on screen with nothing hovered.
#[allow(clippy::too_many_arguments)]
pub(super) fn draw_pointer(
    cr: &cairo::Context,
    state: &State,
    overlay: &Overlay,
    plan: &Layout,
    scale: &PriceScale,
    first: usize,
    visible: usize,
    hovered: Option<usize>,
    bar_w: f64,
    bars_x: f64,
) {
    let Some(based) = overlay.based(&state.bars, first) else { return };
    let to_y = |price: f64| scale.to_y(price, plan.price_y, plan.price_h);
    let end = (first + visible).saturating_sub(1);
    let at = hovered.filter(|at| (first..first + visible).contains(at)).unwrap_or(end);
    let step = grid_step(scale, based.rebase, plan.price_h);
    overlay.tell(Readout { readings: overlay.readings(&state.bars, &based, at), step });

    let Some(hovered) = hovered.filter(|at| (first..first + visible).contains(at)) else { return };
    let x = bars_x + ((hovered - first) as f64 + 0.5) * bar_w;
    let mut dots: Vec<(f64, &str)> = Vec::new();
    if state.drawn_as_line()
        && let Some(bar) = state.bars.get(hovered)
    {
        dots.push((bar.close, &overlay.spec.color));
    }
    for (line, spec) in overlay.spec.lines.iter().enumerate() {
        if let Some(price) = overlay.placed(&based, line, hovered) {
            dots.push((price, &spec.color));
        }
    }
    let background = &state.theme.ui.background;
    for (price, colour) in dots {
        let y = to_y(price);
        if y < plan.price_y || y > plan.price_y + plan.price_h {
            continue;
        }
        colors::set_source(cr, background);
        cr.arc(x, y, 4.5, 0.0, std::f64::consts::TAU);
        let _ = cr.fill();
        colors::set_source(cr, colour);
        cr.arc(x, y, 3.0, 0.0, std::f64::consts::TAU);
        let _ = cr.fill();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bar(ts: i64, close: f64) -> Bar {
        Bar { ts, open: close, high: close + 1.0, low: close - 1.0, close, volume: 0.0 }
    }

    fn overlay(main: &[Bar], other: Vec<Bar>, lines_only: bool, from: Option<i64>) -> Overlay {
        let spec = Comparing {
            lines: vec![CompareLine { label: "GC".into(), color: "#ff0000".into(), bars: Rc::new(other) }],
            from,
            lines_only,
            label: "SPY".into(),
            color: "#ffffff".into(),
        };
        Overlay::new(spec, main, Timeframe::days(1), Rc::new(RefCell::new(None)))
    }

    const DAY: i64 = 86_400;

    #[test]
    fn the_fit_takes_in_a_line_that_ran_further_than_the_bars() {
        // The chart goes 100 → 110, the other market 50 → 100: doubled, so
        // drawn up to 200 on the chart's axis.
        let main: Vec<Bar> = (0..11).map(|d| bar(d * DAY, 100.0 + d as f64)).collect();
        let other: Vec<Bar> = (0..11).map(|d| bar(d * DAY, 50.0 + 5.0 * d as f64)).collect();
        let overlay = overlay(&main, other, false, None);
        let (low, high) = overlay.fit(&main, 0, 11, 99.0, 111.0);
        assert_eq!(low, 99.0);
        assert!((high - 200.0).abs() < 1e-9, "{high}");
    }

    #[test]
    fn scrolling_rebases_every_line_at_the_first_bar_on_screen() {
        let main: Vec<Bar> = (0..10).map(|d| bar(d * DAY, 100.0)).collect();
        let other: Vec<Bar> = (0..10).map(|d| bar(d * DAY, 10.0 * (d + 1) as f64)).collect();
        let overlay = overlay(&main, other, false, None);
        let based = overlay.based(&main, 4).unwrap();
        assert_eq!(based.index, 4);
        // 50 at bar 4 is 0%; 100 at bar 9 is +100%, drawn at 200.
        assert_eq!(overlay.placed(&based, 0, 4), Some(100.0));
        assert_eq!(overlay.placed(&based, 0, 9), Some(200.0));
    }

    #[test]
    fn a_pinned_base_holds_however_the_view_moves() {
        let main: Vec<Bar> = (0..10).map(|d| bar(d * DAY, 100.0 + d as f64)).collect();
        let other: Vec<Bar> = (0..10).map(|d| bar(d * DAY, 10.0)).collect();
        let overlay = overlay(&main, other, false, Some(2 * DAY));
        for first in [0, 5, 8] {
            assert_eq!(overlay.based(&main, first).unwrap().index, 2);
        }
        let based = overlay.based(&main, 7).unwrap();
        // The flat market sits at the chart's base close all along.
        assert_eq!(overlay.placed(&based, 0, 9), Some(102.0));
    }

    #[test]
    fn the_legend_ranks_the_markets_by_their_change_and_puts_the_missing_last() {
        let main: Vec<Bar> = (0..3).map(|d| bar(d * DAY, 100.0 + d as f64)).collect();
        let other: Vec<Bar> = vec![bar(0, 10.0), bar(2 * DAY, 20.0)];
        let mut overlay = overlay(&main, other, false, None);
        overlay.spec.lines.push(CompareLine { label: "NQ".into(), color: "#00ff00".into(), bars: Rc::new(Vec::new()) });
        overlay.realign(&main, Timeframe::days(1));
        let based = overlay.based(&main, 0).unwrap();
        let readings = overlay.readings(&main, &based, 2);
        let order: Vec<Option<usize>> = readings.iter().map(|r| r.line).collect();
        assert_eq!(order, vec![Some(0), None, Some(1)]);
        assert!((readings[0].change.unwrap() - 1.0).abs() < 1e-9);
        assert!((readings[1].change.unwrap() - 0.02).abs() < 1e-9);
        assert_eq!(readings[2].change, None, "still loading");
    }

    #[test]
    fn the_percent_axis_lands_on_round_changes_on_a_log_scale_too() {
        let rebase = Rebase::new(200.0).unwrap();
        let linear = PriceScale { low: 180.0, high: 260.0, log: false };
        let percents = percent_scale(&linear, rebase);
        assert!((percents.low + 0.1).abs() < 1e-12 && (percents.high - 0.3).abs() < 1e-12);
        let log = PriceScale { low: 180f64.ln(), high: 260f64.ln(), log: true };
        let mut ticks = Vec::new();
        percent_scale(&log, rebase).ticks(400.0, |tick| ticks.push(tick_change(true, tick)));
        assert!(ticks.iter().any(|change| change.abs() < 1e-9), "0% is a tick: {ticks:?}");
    }
}
