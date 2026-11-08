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

/// What the legend's row for the chart's own symbol is called among the
/// rows, and each compared market's.
const OWN_ROW: &str = "compare-own";
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
            let routed = instrument.as_ref().and_then(|instrument| self.route(instrument));
            let (Some(instrument), Some(routed)) = (instrument.clone(), routed) else {
                loaded.push(Loaded {
                    comparison: comparison.clone(),
                    instrument,
                    key: None,
                    timeframe,
                    bars: Rc::new(Vec::new()),
                    phase: Phase::Unserved,
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

    /// One row for the chart's own symbol and one for each market: a dot in
    /// its colour, its symbol, and its change, which the chart fills in.
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
        let theme = self.theming.borrow().theme();
        let own = pane.instrument.borrow().as_ref().map(|i| i.display_symbol()).unwrap_or_default();
        let mut all = vec![(OWN_ROW.to_string(), own, theme.ui.text.clone(), Phase::Shown, None)];
        for (line, (label, colour, phase)) in rows.iter().enumerate() {
            all.push((row_name(line), label.clone(), colour.clone(), *phase, Some(line)));
        }
        for (name, label, colour, phase, line) in all {
            let dot = gtk::DrawingArea::new();
            dot.set_size_request(crate::ui::pane::LEGEND_DOT, crate::ui::pane::LEGEND_DOT);
            dot.set_valign(gtk::Align::Center);
            let dot_colour = colour.clone();
            dot.set_draw_func(move |_, cr, width, height| {
                colors::set_source(cr, &dot_colour);
                cr.arc(width as f64 / 2.0, height as f64 / 2.0, 4.0, 0.0, std::f64::consts::TAU);
                let _ = cr.fill();
            });
            let symbol = gtk::Label::new(Some(&label));
            symbol.add_css_class("compare-symbol");
            symbol.set_xalign(0.0);
            names.add_widget(&symbol);
            let value = gtk::Label::new(trouble_words(phase));
            value.add_css_class("compare-value");
            value.set_xalign(1.0);
            value.set_width_chars(7);
            let row = crate::ui::pane::legend_row(&dot, &symbol);
            row.set_widget_name(&name);
            row.append(&value);
            if phase != Phase::Shown {
                row.add_css_class("compare-pending");
            }
            if let Some(line) = line {
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
            }
            legend.append(&row);
        }
        legend.set_visible(true);
    }

    /// The legend in the order and with the changes the chart reads.
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
        let mut before: Option<gtk::Widget> = None;
        for reading in &readout.readings {
            let name = match reading.line {
                None => OWN_ROW.to_string(),
                Some(line) => row_name(line),
            };
            let Some(row) = rows.iter().find(|row| row.widget_name() == name) else { continue };
            legend.reorder_child_after(row, before.as_ref());
            before = Some(row.clone());
            let Some(value) = row.last_child().and_then(|last| match reading.line {
                // The market's own rows end with their close button.
                Some(_) => last.prev_sibling(),
                None => Some(last),
            }) else {
                continue;
            };
            let Ok(value) = value.downcast::<gtk::Label>() else { continue };
            let phase = reading.line.and_then(|line| compared.get(line)).map_or(Phase::Shown, |l| l.phase);
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
        let pane = self.focused_pane();
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
            section.append(Some("Compare from here"), Some("chart.compare-from-here"));
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
            assert_eq!(legend_rows(&pane), [OWN_ROW, "compare-0", "compare-1"]);

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
