//! Comparing markets on a chart: where their bars come from, how the chart
//! and its legend are told, and the window's ways in — the chart's menu, its
//! settings and the actions behind them.
//!
//! A compared market's bars come the way a chart's own do, through the feed
//! that would chart it, at the chart's resolution: what the cache holds is
//! drawn at once, and the gap is asked for behind it. Nothing here waits on
//! the network, and nothing here runs for a chart that compares nothing.
//!
//! What a market reads is shown the moment it is asked for, and stays dimmed
//! in the legend until its feed has answered: drawn from the cache while the
//! fetch is out, solid once it lands, and saying why when it fails.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use adw::prelude::*;
use gtk::gio;
use omacharts_engine::compare::{Compare, Comparison};
use omacharts_engine::theme::ColorChoice;
use omacharts_engine::{Bar, FetchFailure, Instrument, Timeframe};

use super::{fold_for_screen, Feed, Routed, Window};
use crate::live::Series;
use crate::loader::{Request, FOREGROUND, REFRESH};
use crate::ui::chart::{CompareLine, Comparing, Readout};
use crate::ui::colors;
use crate::ui::pane::ChartPane;

/// Where one compared market's bars stand.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// Asked for, and drawn from whatever the cache had meanwhile.
    Loading,
    Shown,
    /// The feed answered with a failure; whatever the cache had is drawn.
    Failed(FetchFailure),
    /// No feed charts it, or the listings do not know it.
    Unserved,
}

/// One compared market's bars, as far as they have come.
#[derive(Clone, Debug)]
pub struct Loaded {
    pub comparison: Comparison,
    pub instrument: Option<Instrument>,
    /// The cache key it was asked for under, `provider:symbol`.
    pub key: Option<String>,
    /// The resolution its bars are folded to: the chart's when they were
    /// asked for.
    pub timeframe: Timeframe,
    pub bars: Rc<Vec<Bar>>,
    pub phase: Phase,
}

thread_local! {
    /// The bar a right-click on a chart was over, for "Compare from here".
    static MENU_AT: Cell<Option<i64>> = const { Cell::new(None) };
    /// "Lines only", ticked to match the chart a menu is opened on.
    static LINES_ACTION: RefCell<Option<gio::SimpleAction>> = const { RefCell::new(None) };
}

/// What the legend's row saying where the lines are pinned is called, and
/// each compared market's row.
const PIN_ROW: &str = "compare-pin";
fn row_name(line: usize) -> String {
    format!("compare-{line}")
}

/// The words a row shows in place of a change it cannot give.
fn trouble_words(phase: Phase) -> Option<&'static str> {
    match phase {
        Phase::Failed(failure) => Some(failure.message()),
        Phase::Unserved => Some("Not on any data feed"),
        Phase::Loading | Phase::Shown => None,
    }
}

impl Window {
    /// Fetch every market `pane` compares at its resolution, drawing what
    /// the cache has now. Called wherever the chart's own bars are, so a
    /// change of symbol, resolution or session takes the comparison along.
    pub(super) fn load_comparisons(self: &Rc<Self>, pane: &Rc<ChartPane>) {
        let compare = pane.compare.borrow().clone();
        if !compare.active() && pane.compared.borrow().is_empty() {
            return;
        }
        let timeframe = pane.timeframe.get();
        let native = timeframe.native();
        let mut loaded = Vec::with_capacity(compare.symbols.len());
        for comparison in &compare.symbols {
            let instrument = self.index.find(&comparison.symbol, comparison.suffix.as_deref());
            let routed = instrument.as_ref().and_then(|instrument| self.route_compared(pane, instrument));
            let (Some(instrument), Some(routed)) = (instrument.clone(), routed) else {
                // Not named by the curated half the window opens on is not
                // yet "not on any feed": the whole catalogue looks again
                // when it lands.
                let phase = match instrument.is_none() && !self.index.complete() {
                    true => Phase::Loading,
                    false => Phase::Unserved,
                };
                loaded.push(Loaded {
                    comparison: comparison.clone(),
                    instrument,
                    key: None,
                    timeframe,
                    bars: Rc::new(Vec::new()),
                    phase,
                });
                continue;
            };
            let key = routed.key();
            let Routed { feed, symbol, .. } = routed;
            // Streamed already by a chart of its own, then the memo, then the
            // cache: on screen before any request leaves.
            let series = Series { key: key.clone(), native };
            let streamed = feed.live.as_ref().and_then(|live| live.with_bars(&series, |bars| bars.to_vec()));
            let memo = self.series.borrow().get(&(key.clone(), timeframe)).cloned();
            let bars = match (streamed, memo) {
                (Some(bars), _) => Rc::new(fold_for_screen(&instrument, timeframe, pane.session.get(), bars)),
                (None, Some(bars)) => bars,
                (None, None) => {
                    let cached = self.store.load_bars(&key, native);
                    Rc::new(fold_for_screen(&instrument, timeframe, pane.session.get(), cached))
                }
            };
            // A streamed feed is pushed what it charts, and has nothing to
            // ask for; the bars it holds now are the answer.
            let phase = match &feed.live {
                Some(_) => Phase::Shown,
                None => {
                    feed.loader.fetch(Request { key: key.clone(), symbol, timeframe, speculative: false }, FOREGROUND);
                    Phase::Loading
                }
            };
            loaded.push(Loaded {
                comparison: comparison.clone(),
                instrument: Some(instrument),
                key: Some(key),
                timeframe,
                bars,
                phase,
            });
        }
        *pane.compared.borrow_mut() = loaded;
        self.apply_comparing(pane);
    }

    /// The feed that would chart a market `pane` compares, and the symbol it
    /// is asked for there: a futures root joined at the rolls as the chart's
    /// own would be, so ES compared beside SPY is the same series ES charted
    /// on its own shows. See [`omacharts_engine::continuous::routed`].
    fn route_compared(&self, pane: &ChartPane, instrument: &Instrument) -> Option<Routed> {
        let routed = self.route(instrument)?;
        let symbol = omacharts_engine::continuous::routed(
            routed.feed.provider.as_ref(),
            instrument,
            &routed.symbol,
            pane.continuous.get(),
        );
        Some(Routed { symbol, ..routed })
    }

    /// Whether any market `pane` compares would now be asked for under
    /// another key than its bars were: a stitched root that rolled, or a
    /// feed that came or went. Such a chart fetches its comparisons again.
    pub(super) fn comparisons_rerouted(&self, pane: &ChartPane) -> bool {
        pane.compared.borrow().iter().any(|loaded| {
            let now = loaded.instrument.as_ref().and_then(|instrument| self.route_compared(pane, instrument));
            now.map(|routed| routed.key()) != loaded.key
        })
    }

    /// Bars that arrived for `key`, onto every chart comparing that market at
    /// that resolution. A failure keeps what the cache had and says so.
    pub(super) fn compare_arrived(
        self: &Rc<Self>,
        key: &str,
        native: Timeframe,
        bars: &[Bar],
        trouble: Option<FetchFailure>,
    ) {
        let panes: Vec<Rc<ChartPane>> = self
            .panes
            .borrow()
            .iter()
            .filter(|pane| {
                pane.compared
                    .borrow()
                    .iter()
                    .any(|loaded| loaded.key.as_deref() == Some(key) && loaded.timeframe.native() == native)
            })
            .cloned()
            .collect();
        for pane in panes {
            let session = pane.session.get();
            {
                let mut compared = pane.compared.borrow_mut();
                for loaded in compared.iter_mut() {
                    if loaded.key.as_deref() != Some(key) || loaded.timeframe.native() != native {
                        continue;
                    }
                    let Some(instrument) = &loaded.instrument else { continue };
                    let folded = Rc::new(fold_for_screen(instrument, loaded.timeframe, session, bars.to_vec()));
                    self.series.borrow_mut().insert((key.to_string(), loaded.timeframe), folded.clone());
                    loaded.bars = folded;
                    loaded.phase = match trouble {
                        Some(failure) => Phase::Failed(failure),
                        None => Phase::Shown,
                    };
                }
            }
            self.apply_comparing(&pane);
        }
    }

    /// Hand the chart what it compares, coloured from the theme, and lay the
    /// legend out for it. With nothing compared, the chart is handed nothing
    /// and the legend goes.
    pub(super) fn apply_comparing(self: &Rc<Self>, pane: &Rc<ChartPane>) {
        let compare = pane.compare.borrow().clone();
        if !compare.active() {
            pane.compared.borrow_mut().clear();
            pane.view.set_comparing(None);
            self.rebuild_compare_legend(pane, &[]);
            return;
        }
        let theme = self.theming.borrow().theme();
        let indicators = pane.indicators.borrow().clone();
        let taken = omacharts_engine::palette_colors(&indicators, &theme);
        let colours = compare.colors(&theme, &taken);
        let compared = pane.compared.borrow().clone();
        let lines: Vec<CompareLine> = compared
            .iter()
            .zip(&colours)
            .map(|(loaded, colour)| CompareLine {
                label: loaded.comparison.spelled(),
                color: colour.clone(),
                bars: loaded.bars.clone(),
            })
            .collect();
        let label = pane.instrument.borrow().as_ref().map(|i| i.display_symbol()).unwrap_or_default();
        pane.view.set_comparing(Some(Comparing {
            lines,
            from: compare.from,
            lines_only: compare.lines_only,
            label,
            color: theme.ui.text.clone(),
        }));
        let weak = Rc::downgrade(self);
        let id = pane.id;
        pane.view.set_compare_readout_handler(move |readout| {
            if let Some(this) = weak.upgrade() {
                this.show_compare_readout(id, &readout);
            }
        });
        let rows: Vec<(String, String, Phase)> = compared
            .iter()
            .zip(colours)
            .map(|(loaded, colour)| (loaded.comparison.spelled(), colour, loaded.phase))
            .collect();
        self.rebuild_compare_legend(pane, &rows);
    }

    /// One row for each market compared, in the order they were added: a
    /// dot in its colour, its symbol, and its change, which the chart fills
    /// in. The chart's own symbol is in the header and has no row. With the
    /// base pinned, a last row says from when, with a way back to the left
    /// edge.
    fn rebuild_compare_legend(self: &Rc<Self>, pane: &Rc<ChartPane>, rows: &[(String, String, Phase)]) {
        let legend = &pane.compare_legend;
        while let Some(child) = legend.first_child() {
            legend.remove(&child);
        }
        if rows.is_empty() {
            legend.set_visible(false);
            return;
        }
        let names = gtk::SizeGroup::new(gtk::SizeGroupMode::Horizontal);
        for (line, (label, colour, phase)) in rows.iter().enumerate() {
            let dot = gtk::DrawingArea::new();
            dot.set_size_request(crate::ui::pane::LEGEND_DOT, crate::ui::pane::LEGEND_DOT);
            dot.set_valign(gtk::Align::Center);
            let dot_colour = colour.clone();
            dot.set_draw_func(move |_, cr, width, height| {
                colors::set_source(cr, &dot_colour);
                cr.arc(width as f64 / 2.0, height as f64 / 2.0, 4.0, 0.0, std::f64::consts::TAU);
                let _ = cr.fill();
            });
            let symbol = gtk::Label::new(Some(label));
            symbol.add_css_class("compare-symbol");
            symbol.set_xalign(0.0);
            names.add_widget(&symbol);
            let value = gtk::Label::new(trouble_words(*phase));
            value.add_css_class("compare-value");
            value.set_xalign(1.0);
            value.set_width_chars(7);
            let row = crate::ui::pane::legend_row(&dot, &symbol);
            row.set_widget_name(&row_name(line));
            row.append(&value);
            if *phase != Phase::Shown {
                row.add_css_class("compare-pending");
            }
            let remove = gtk::Button::from_icon_name("window-close-symbolic");
            remove.add_css_class("flat");
            remove.add_css_class("legend-button");
            remove.set_tooltip_text(Some("Stop comparing"));
            let this = self.clone();
            let owner = pane.clone();
            remove.connect_clicked(move |_| {
                this.focus(owner.id);
                this.remove_comparison(line);
            });
            row.append(&remove);
            legend.append(&row);
        }
        let pinned = pane.compare.borrow().from;
        if let Some(from) = pinned {
            legend.append(&self.pin_row(pane, from));
        }
        legend.set_visible(true);
    }

    /// The legend's last row while the base is pinned: from when, and a
    /// button that measures from the left edge again.
    fn pin_row(self: &Rc<Self>, pane: &Rc<ChartPane>, from: i64) -> gtk::Box {
        let row = gtk::Box::new(gtk::Orientation::Horizontal, crate::ui::pane::LEGEND_GAP);
        row.add_css_class("legend-row");
        row.set_widget_name(PIN_ROW);
        let words = gtk::Label::new(Some(&format!("From {}", self.day_on(pane, from))));
        words.add_css_class("compare-pin");
        words.set_xalign(0.0);
        row.append(&words);
        let unpin = gtk::Button::from_icon_name("window-close-symbolic");
        unpin.add_css_class("flat");
        unpin.add_css_class("legend-button");
        unpin.add_css_class("compare-unpin");
        unpin.set_tooltip_text(Some("Compare from the left edge"));
        let this = self.clone();
        let owner = pane.clone();
        unpin.connect_clicked(move |_| {
            this.focus(owner.id);
            this.set_compare_from(None);
        });
        row.append(&unpin);
        row
    }

    /// The changes the chart reads, each into its market's row. The rows
    /// stay in the order they were laid out in, the chart's own first and
    /// the markets as they are compared, however the changes rank.
    fn show_compare_readout(&self, id: u32, readout: &Readout) {
        let Some(pane) = self.pane(id) else { return };
        let legend = &pane.compare_legend;
        let mut rows: Vec<gtk::Widget> = Vec::new();
        let mut child = legend.first_child();
        while let Some(row) = child {
            child = row.next_sibling();
            rows.push(row);
        }
        let compared = pane.compared.borrow();
        for reading in &readout.readings {
            // The chart's own reading has no row: the header names it.
            let Some(line) = reading.line else { continue };
            let name = row_name(line);
            let Some(row) = rows.iter().find(|row| row.widget_name() == name) else { continue };
            // A market's row ends with its close button, after the value.
            let Some(value) = row.last_child().and_then(|last| last.prev_sibling()) else { continue };
            let Ok(value) = value.downcast::<gtk::Label>() else { continue };
            let phase = compared.get(line).map_or(Phase::Shown, |l| l.phase);
            let text = match (reading.change, trouble_words(phase)) {
                (_, Some(words)) => words.to_string(),
                (Some(change), None) => readout.text(change),
                (None, None) => String::new(),
            };
            if value.text() != text {
                value.set_text(&text);
            }
        }
    }

    /// Change what the focused chart compares, and redraw and save it.
    fn edit_compare(self: &Rc<Self>, reload: bool, edit: impl FnOnce(&mut Compare)) {
        let pane = self.focused_pane();
        edit(&mut pane.compare.borrow_mut());
        match reload {
            true => self.load_comparisons(&pane),
            false => self.apply_comparing(&pane),
        }
        self.save_workspace();
        self.chart_changed_elsewhere();
    }

    /// Compare `instrument` on the focused chart. Its line is drawn from the
    /// cache straight away, and its row stays dimmed until its feed answers.
    pub fn add_comparison(self: &Rc<Self>, instrument: Instrument) {
        let pane = self.focused_pane();
        let own = pane.instrument.borrow().as_ref().is_some_and(|showing| {
            showing.symbol == instrument.symbol && showing.suffix == instrument.suffix
        });
        if own {
            return;
        }
        self.edit_compare(true, |compare| {
            compare.add(Comparison::new(&instrument.symbol, instrument.suffix.as_deref()));
        });
    }

    /// Stop comparing the focused chart's `line`th market.
    pub fn remove_comparison(self: &Rc<Self>, line: usize) {
        self.edit_compare(true, |compare| {
            let Some(comparison) = compare.symbols.get(line).cloned() else { return };
            compare.remove(&comparison.symbol, comparison.suffix.as_deref());
        });
    }

    /// Stop comparing anything on the focused chart.
    pub fn clear_comparisons(self: &Rc<Self>) {
        self.edit_compare(true, |compare| *compare = Compare::default());
    }

    pub fn set_comparison_color(self: &Rc<Self>, line: usize, color: Option<ColorChoice>) {
        self.edit_compare(false, |compare| {
            if let Some(comparison) = compare.symbols.get_mut(line) {
                comparison.color = color;
            }
        });
    }

    pub fn set_compare_lines_only(self: &Rc<Self>, on: bool) {
        self.edit_compare(false, |compare| compare.lines_only = on);
    }

    /// Pin the focused chart's base to the bar at `ts`, or let it follow the
    /// left edge again with `None`.
    pub fn set_compare_from(self: &Rc<Self>, ts: Option<i64>) {
        self.edit_compare(false, |compare| compare.from = ts);
    }

    /// What the focused chart compares, as kept.
    pub fn comparison(&self) -> Compare {
        self.focused_pane().compare.borrow().clone()
    }

    /// The colours the focused chart's markets are drawn in, in order.
    pub fn comparison_colors(&self) -> Vec<String> {
        let pane = self.focused_pane();
        let theme = self.theming.borrow().theme();
        let taken = omacharts_engine::palette_colors(&pane.indicators.borrow(), &theme);
        pane.compare.borrow().colors(&theme, &taken)
    }

    /// What the listings call a market, `Gold` for GC.
    pub fn instrument_name(&self, symbol: &str, suffix: Option<&str>) -> Option<String> {
        self.index.find(symbol, suffix).map(|instrument| instrument.name).filter(|name| !name.is_empty())
    }

    /// A bar's day in the focused chart's clock, `3 Nov 2025`, with its time
    /// for an intraday chart.
    pub fn day_of(&self, ts: i64) -> String {
        self.day_on(&self.focused_pane(), ts)
    }

    /// [`Window::day_of`] in `pane`'s clock and resolution.
    fn day_on(&self, pane: &ChartPane, ts: i64) -> String {
        let zone = pane
            .instrument
            .borrow()
            .as_ref()
            .map_or(chrono_tz::UTC, |instrument| self.clock_for(instrument).zone);
        let format = match pane.timeframe.get().is_intraday() {
            true => "%-d %b %Y %H:%M",
            false => "%-d %b %Y",
        };
        chrono::DateTime::from_timestamp(ts, 0)
            .map(|utc| utc.with_timezone(&zone).format(format).to_string())
            .unwrap_or_default()
    }

    /// Open the symbol search to pick a market to compare the focused chart
    /// with.
    pub fn prompt_comparison(self: &Rc<Self>) {
        let this = self.clone();
        self.search.present(self.over_focused(), move |instrument| this.add_comparison(instrument));
    }

    /// Ask again for compared markets gone stale on a chart on screen, as the
    /// chart's own are asked for: speculatively, behind anything somebody is
    /// waiting on.
    pub(super) fn refresh_comparisons(self: &Rc<Self>, pane: &Rc<ChartPane>, visible: bool, now: i64) {
        let compared = pane.compared.borrow().clone();
        for loaded in compared {
            let (Some(instrument), Some(key)) = (&loaded.instrument, &loaded.key) else { continue };
            let Some(Routed { feed, symbol, .. }) = self.route(instrument) else { continue };
            if feed.live.is_some() || Feed::quiet(&feed.refresh_quiet_until) {
                continue;
            }
            let candidate = omacharts_engine::refresh::Candidate {
                delivery: feed.provider.delivery(),
                visible,
                at_latest: pane.view.at_latest(),
                timeframe: loaded.timeframe,
                instrument,
                fetched_at: self.store.coverage(key, loaded.timeframe.native()).map(|c| c.fetched_at),
            };
            if omacharts_engine::refresh::due(&candidate, now).is_ok() {
                let request = Request { key: key.clone(), symbol, timeframe: loaded.timeframe, speculative: true };
                feed.loader.fetch(request, REFRESH);
            }
        }
    }

    /// The chart menu's comparison rows: a way to add one, and with one
    /// there, where the lines are measured from and whether the chart's own
    /// bars give way to a line.
    pub(super) fn compare_menu_section(&self) -> gio::Menu {
        let section = gio::Menu::new();
        section.append(Some("Compare with\u{2026}"), Some("chart.compare-with"));
        let compare = self.focused_pane().compare.borrow().clone();
        if compare.active() {
            // Only over a bar: the room past the last one is no moment to
            // measure from, and pinned there every line would read from the
            // latest bar, off to the right.
            if MENU_AT.with(Cell::get).is_some() {
                section.append(Some("Compare from here"), Some("chart.compare-from-here"));
            }
            if compare.from.is_some() {
                section.append(Some("Compare from left edge"), Some("chart.compare-from-left"));
            }
            section.append(Some("Lines only"), Some("chart.compare-lines"));
            LINES_ACTION.with(|action| {
                if let Some(action) = action.borrow().as_ref() {
                    action.set_state(&compare.lines_only.to_variant());
                }
            });
        }
        section
    }

    /// Remember the bar under a right-click at `x` of the focused chart, for
    /// "Compare from here".
    pub(super) fn note_menu_at(&self, x: f64) {
        MENU_AT.with(|at| at.set(self.focused_pane().view.bar_time_at(x)));
    }

    pub(super) fn install_compare_actions(self: &Rc<Self>, actions: &gio::SimpleActionGroup) {
        let with = gio::SimpleAction::new("compare-with", None);
        let this = self.clone();
        with.connect_activate(move |_, _| this.prompt_comparison());
        actions.add_action(&with);

        let here = gio::SimpleAction::new("compare-from-here", None);
        let this = self.clone();
        here.connect_activate(move |_, _| {
            if let Some(ts) = MENU_AT.with(Cell::get) {
                this.set_compare_from(Some(ts));
            }
        });
        actions.add_action(&here);

        let left = gio::SimpleAction::new("compare-from-left", None);
        let this = self.clone();
        left.connect_activate(move |_, _| this.set_compare_from(None));
        actions.add_action(&left);

        let lines = gio::SimpleAction::new_stateful("compare-lines", None, &false.to_variant());
        let this = self.clone();
        lines.connect_activate(move |action, _| {
            let on = !this.comparison().lines_only;
            action.set_state(&on.to_variant());
            this.set_compare_lines_only(on);
        });
        actions.add_action(&lines);
        LINES_ACTION.with(|action| *action.borrow_mut() = Some(lines));
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Store;
    use crate::ui::window::trade::tests::bare_window;

    fn command(window: &Rc<Window>, words: &[&str]) -> String {
        let args: Vec<String> = std::iter::once("omacharts").chain(words.iter().copied()).map(String::from).collect();
        let live: &dyn crate::cli::Live = window;
        let outcome = crate::cli::run(&args, &window.store, Some(live), &crate::cli::Here);
        assert_eq!(outcome.code, 0, "{words:?}: {}", outcome.err);
        outcome.out
    }

    fn legend_rows(pane: &ChartPane) -> Vec<String> {
        let mut names = Vec::new();
        let mut child = pane.compare_legend.first_child();
        while let Some(row) = child {
            names.push(row.widget_name().to_string());
            child = row.next_sibling();
        }
        names
    }

    /// Markets compared from a command are on the chart and in its legend,
    /// are written down with it, and come back with the window.
    #[test]
    fn a_comparison_reaches_the_chart_and_survives_a_restart() {
        crate::ui::on_gtk(|| {
            let store = Rc::new(Store::memory().expect("a store"));
            let window = bare_window(&store);
            command(&window, &["chart", "compare", "add", "QQQ", "GC"]);
            let pane = window.focused_pane();
            let symbols: Vec<String> = pane.compare.borrow().symbols.iter().map(|c| c.spelled()).collect();
            assert_eq!(symbols, ["QQQ", "GC"]);
            assert!(pane.view.comparing(), "the chart is not drawing them");
            assert_eq!(pane.compared.borrow().len(), 2);
            assert!(pane.compare_legend.get_visible());
            assert_eq!(legend_rows(&pane), ["compare-0", "compare-1"], "the chart's own symbol is in the header");

            window.set_compare_lines_only(true);
            window.set_compare_from(Some(1_700_000_000));
            window.window.close();

            let again = bare_window(&store);
            let pane = again.focused_pane();
            let compare = pane.compare.borrow().clone();
            assert_eq!(compare.symbols.len(), 2, "the comparison did not come back");
            assert!(compare.lines_only);
            assert_eq!(compare.from, Some(1_700_000_000));
            assert!(pane.view.comparing());
            again.window.close();
        });
    }

    /// Taking the last market off draws the chart as it was, with no legend
    /// for comparisons and nothing written down.
    #[test]
    fn the_last_market_taken_off_leaves_the_chart_as_it_was() {
        crate::ui::on_gtk(|| {
            let store = Rc::new(Store::memory().expect("a store"));
            let window = bare_window(&store);
            let pane = window.focused_pane();
            assert!(!pane.view.comparing(), "a chart compares nothing to begin with");
            window.add_comparison(window.index.find("GC", None).expect("gold"));
            assert!(pane.view.comparing());
            window.set_compare_lines_only(true);
            window.remove_comparison(0);
            assert!(!pane.view.comparing());
            assert!(!pane.compare_legend.get_visible());
            assert_eq!(*pane.compare.borrow(), Compare::default(), "lines only went with the last market");
            let book = window.capture_book("Main".into());
            let json = serde_json::to_value(&book.panes[0]).expect("a chart as JSON");
            assert!(json.get("compare").is_none(), "an empty comparison is written down: {json}");
            window.window.close();
        });
    }

    /// A futures root compared is asked for joined at the rolls as the
    /// chart says, the series ES charted on its own is, and follows the
    /// chart when it is joined the other way.
    #[test]
    fn a_compared_root_is_the_series_its_own_chart_would_be() {
        use omacharts_engine::continuous::{fake::Months, Adjustment, Continuous};
        use std::sync::Arc;
        crate::ui::on_gtk(|| {
            let _ = adw::init();
            let store = Rc::new(Store::memory().expect("a store"));
            let app = adw::Application::new(None::<&str>, gio::ApplicationFlags::NON_UNIQUE);
            let feed: Arc<dyn omacharts_engine::Provider> = Arc::new(Continuous::new(Box::new(Months::all())));
            let window = Window::build_with(&app, store.clone(), vec![feed]);
            let pane = window.focused_pane();
            let es = window.index.find("ES", None).expect("ES");
            let september = es.in_month(omacharts_engine::contracts::Month::new(2026, 9).unwrap()).unwrap();
            window.show_in(&pane, september);
            window.add_comparison(es.clone());
            let key = || pane.compared.borrow()[0].key.clone();
            assert_eq!(key().as_deref(), Some("months:ES~ESZ26~back-adjusted"));
            assert_eq!(pane.compare.borrow().symbols[0].spelled(), "ES", "written down as the root");
            assert!(!window.comparisons_rerouted(&pane));
            window.set_continuous(Adjustment::AsTraded);
            assert_eq!(key().as_deref(), Some("months:ES~ESZ26~as-traded"), "joined as the chart now is");
            window.window.close();
        });
    }

    /// The legend's rows stay put as the pointer moves: the chart's own
    /// first, then the markets in the order they were compared in, however
    /// their changes rank at the bar read. Only the values change.
    #[test]
    fn the_legend_keeps_its_order_wherever_the_markets_rank() {
        use crate::ui::chart::Reading;
        crate::ui::on_gtk(|| {
            let store = Rc::new(Store::memory().expect("a store"));
            let window = bare_window(&store);
            command(&window, &["chart", "compare", "add", "QQQ", "GLD", "BTC-USD"]);
            let pane = window.focused_pane();
            let laid_out = legend_rows(&pane);
            assert_eq!(laid_out, ["compare-0", "compare-1", "compare-2"], "the markets alone, as added");
            // As if every feed had answered, so the rows show their changes.
            for loaded in pane.compared.borrow_mut().iter_mut() {
                loaded.phase = Phase::Shown;
            }
            let value = |name: &str| -> String {
                let mut child = pane.compare_legend.first_child();
                while let Some(row) = child {
                    if row.widget_name() == name {
                        let label = row.last_child().unwrap().prev_sibling().unwrap();
                        return label.downcast::<gtk::Label>().unwrap().text().to_string();
                    }
                    child = row.next_sibling();
                }
                panic!("no row {name}")
            };
            // Two bars where every market ranks differently.
            for changes in [[0.088, 0.176, -0.007, -0.052], [-0.03, 0.01, 0.12, 0.05]] {
                // Handed over ranked, as a chart once did: the rows must not
                // follow.
                let mut readings: Vec<Reading> = std::iter::once(None)
                    .chain((0..3).map(Some))
                    .zip(changes)
                    .map(|(line, change)| Reading { line, change: Some(change) })
                    .collect();
                readings.sort_by(|a, b| b.change.unwrap().total_cmp(&a.change.unwrap()));
                let readout = Readout { readings, step: 0.01 };
                window.show_compare_readout(pane.id, &readout);
                assert_eq!(legend_rows(&pane), laid_out, "the rows moved for {changes:?}");
                for line in 0..3 {
                    assert_eq!(value(&row_name(line)), readout.text(changes[line + 1]), "market {line}");
                }
            }
            window.window.close();
        });
    }

    /// A pinned base is said in the legend, under the markets, as the day it
    /// is pinned to, and its button measures from the left edge again. A
    /// base at the left edge says nothing. A pin kept from before is shown,
    /// not cleared.
    #[test]
    fn a_pinned_base_is_shown_in_the_legend_and_cleared_from_it() {
        crate::ui::on_gtk(|| {
            let store = Rc::new(Store::memory().expect("a store"));
            let window = bare_window(&store);
            command(&window, &["chart", "compare", "add", "QQQ", "GLD"]);
            let pane = window.focused_pane();
            assert_eq!(legend_rows(&pane), ["compare-0", "compare-1"], "nothing pinned, nothing said");

            // 8 Jan 2026, as the session opened in New York.
            window.set_compare_from(Some(1_767_882_600));
            window.window.close();
            let window = bare_window(&store);
            let pane = window.focused_pane();
            assert_eq!(pane.compare.borrow().from, Some(1_767_882_600), "a stored pin is kept");
            assert_eq!(legend_rows(&pane), ["compare-0", "compare-1", PIN_ROW]);
            let pin = pane.compare_legend.last_child().expect("the pin's row");
            let words = pin.first_child().unwrap().downcast::<gtk::Label>().unwrap().text().to_string();
            assert!(words.starts_with("From ") && words.contains("Jan 2026"), "{words}");

            let unpin = pin.last_child().unwrap().downcast::<gtk::Button>().unwrap();
            assert!(unpin.has_css_class("compare-unpin"), "seen without a hover, unlike a market's close button");
            unpin.emit_clicked();
            assert_eq!(pane.compare.borrow().from, None, "measured from the left edge again");
            assert_eq!(legend_rows(&pane), ["compare-0", "compare-1"]);
            window.window.close();
        });
    }

    /// A market compared that the curated half cannot name, as when the
    /// window has just opened, waits for the whole catalogue rather than
    /// saying it is on no feed, and is charted once the catalogue lands,
    /// through the feed a chart of it would use.
    #[test]
    fn a_market_the_curated_half_cannot_name_is_compared_once_the_catalogue_lands() {
        crate::ui::on_gtk(|| {
            let store = Rc::new(Store::memory().expect("a store"));
            let window = bare_window(&store);
            assert!(!window.index.complete(), "the window opens on the curated half");
            assert!(window.index.find("BIT", None).is_none(), "BIT is in the listings, not the curated half");
            let pane = window.focused_pane();
            window.edit_compare(true, |compare| {
                compare.add(Comparison::new("QQQ", None));
                compare.add(Comparison::new("BIT", None));
            });
            let phases = |pane: &ChartPane| pane.compared.borrow().iter().map(|l| l.phase).collect::<Vec<_>>();
            assert_eq!(phases(&pane)[1], Phase::Loading, "waiting on the catalogue, not on no feed");
            assert!(pane.compared.borrow()[1].instrument.is_none());

            window.index.replace(crate::inventory::everything());
            window.resolve_pending();
            let compared = pane.compared.borrow();
            let bit = &compared[1];
            assert_eq!(bit.instrument.as_ref().map(|i| i.symbol.as_str()), Some("BIT"));
            let own = window.route(bit.instrument.as_ref().unwrap()).map(|routed| routed.key());
            assert_eq!(bit.key, own, "the feed a chart of BIT would use");
            assert!(bit.key.is_some());
            assert_ne!(bit.phase, Phase::Unserved);
            drop(compared);
            window.window.close();
        });
    }

    /// The chart's own symbol is not something to compare it with.
    #[test]
    fn a_chart_is_not_compared_with_itself() {
        crate::ui::on_gtk(|| {
            let store = Rc::new(Store::memory().expect("a store"));
            let window = bare_window(&store);
            let own = window.focused_pane().instrument.borrow().clone().expect("a symbol");
            window.add_comparison(own);
            assert!(window.comparison().is_empty());
            window.window.close();
        });
    }

    /// The chart's menu offers a comparison, and with one, where it is
    /// measured from and lines only.
    #[test]
    fn the_chart_menu_offers_comparing() {
        crate::ui::on_gtk(|| {
            let store = Rc::new(Store::memory().expect("a store"));
            let window = bare_window(&store);
            let actions = |window: &Rc<Window>| -> Vec<String> {
                let menu = window.chart_menu_model(100.0);
                let mut found = Vec::new();
                let mut walk = vec![menu.upcast::<gio::MenuModel>()];
                while let Some(model) = walk.pop() {
                    for at in 0..model.n_items() {
                        if let Some(action) =
                            model.item_attribute_value(at, "action", None).and_then(|v| v.str().map(str::to_string))
                        {
                            found.push(action);
                        }
                        for link in ["section", "submenu"] {
                            if let Some(inner) = model.item_link(at, link) {
                                walk.push(inner);
                            }
                        }
                    }
                }
                found
            };
            let before = actions(&window);
            assert!(before.iter().any(|a| a == "chart.compare-with"));
            assert!(!before.iter().any(|a| a == "chart.compare-lines"), "nothing to draw as lines yet");
            window.add_comparison(window.index.find("GC", None).expect("gold"));
            // Opened beside the last bar, there is no bar to measure from.
            MENU_AT.with(|at| at.set(None));
            assert!(!actions(&window).iter().any(|a| a == "chart.compare-from-here"), "nothing here to pin");
            // Opened over a bar.
            MENU_AT.with(|at| at.set(Some(1_700_000_000)));
            let after = actions(&window);
            assert!(after.iter().any(|a| a == "chart.compare-from-here"));
            assert!(after.iter().any(|a| a == "chart.compare-lines"));
            assert!(!after.iter().any(|a| a == "chart.compare-from-left"), "the base is not pinned");
            window.set_compare_from(Some(1_700_000_000));
            assert!(actions(&window).iter().any(|a| a == "chart.compare-from-left"));
            window.window.close();
        });
    }
}
