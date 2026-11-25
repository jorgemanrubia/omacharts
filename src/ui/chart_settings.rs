//! Settings that belong to the chart rather than the app.
//!
//! Reached from the gear beside the legend, because that is where you are
//! looking when you want them. Three pages: how the bars are read and
//! scaled, what is drawn on top of them, and how the chart trades.

mod compare;

use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;
use gtk::glib;
#[cfg(test)]
use omacharts_engine::indicators::Kind;
use omacharts_engine::{BarColouring, BarStyle, Indicator, Session};

use crate::ui::controls;
#[cfg(test)]
use crate::ui::profile_rows;
use crate::ui::property_rows;
use crate::ui::shortcuts;
use crate::ui::window::Window;
pub(crate) use omacharts_widgets::stroke::{stroke_sample, Ink, LINE_SAMPLE_WIDTH};
/// The indicators' list, picker and settings are the widgets crate's, so a
/// program embedding the chart has them; here under the names the rest of
/// the app reaches them by.
pub use omacharts_widgets::indicator_settings::*;

/// Where the dialog should land when it opens.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Focus {
    Chart,
    Indicators,
    Indicator(u32),
}

pub struct ChartSettings;

impl ChartSettings {
    /// The chart's own settings, on the first page.
    pub fn present(window: &Rc<Window>) {
        ChartSettings::open(window, Focus::Chart);
    }

    /// Straight to the indicators.
    pub fn present_indicators(window: &Rc<Window>) {
        ChartSettings::open(window, Focus::Indicators);
    }

    /// Straight to one indicator's panel, for the gear beside it on the chart.
    pub fn present_indicator(window: &Rc<Window>, id: u32) {
        ChartSettings::open(window, Focus::Indicator(id));
    }

    fn open(window: &Rc<Window>, focus: Focus) {
        let dialog = adw::PreferencesDialog::new();
        dialog.set_title("Chart");
        dialog.set_content_width(DIALOG_WIDTH);
        // Most of the window's height, like the preferences: the indicator
        // list and an indicator's own page are long, and every row that fits
        // is a scroll saved. The width stays the column the rows sit in.
        dialog.set_content_height((window.window.height() * 85 / 100).max(620));

        let chart_page = adw::PreferencesPage::new();
        chart_page.set_title("Chart");
        chart_page.set_icon_name(Some("preferences-system-symbolic"));
        chart_page.add(&title_group(window));
        chart_page.add(&bars_group(window));
        chart_page.add(&scales_group(window));
        chart_page.add(&compare::group(window, &dialog));
        chart_page.add(&session_group(window));
        chart_page.add(&drawings_group(window));
        chart_page.add(&defaults_group(window, &dialog));
        dialog.add(&chart_page);

        let indicators_page = adw::PreferencesPage::new();
        indicators_page.set_title("Indicators");
        indicators_page.set_icon_name(Some("view-list-symbolic"));
        dialog.add(&indicators_page);

        let trading_page = trading_page(window);
        dialog.add(&trading_page);
        let pages = [chart_page.clone(), indicators_page.clone(), trading_page];

        {
            // Alt and an arrow between the pages, and the keyboard goes with
            // it. The pages are named here rather than asked of the dialog: a
            // preferences dialog keeps them in a view stack of its own, and
            // the ones it was given are the ones it has.
            let dialog_for_tabs = dialog.clone();
            crate::ui::dialogs::step_tabs_with_alt(&dialog, move |forward| {
                let shown = dialog_for_tabs.visible_page();
                let at = pages
                    .iter()
                    .position(|page| shown.as_ref() == Some(page.upcast_ref()))
                    .unwrap_or(0) as u32;
                let next = crate::ui::dialogs::step_round(at, pages.len() as u32, forward);
                let Some(page) = pages.get(next as usize) else { return };
                dialog_for_tabs.set_visible_page(page);
                // On the next turn of the loop: the page being moved to has
                // not been laid out and has nothing to give the focus to
                // until it has.
                let page = page.clone();
                glib::idle_add_local_once(move || {
                    page.child_focus(gtk::DirectionType::TabForward);
                });
            });
        }

        let owner = chart_owner(window);
        // Above the list, because it decides what the list is: the chart's
        // own, or a template's shown here and changed elsewhere.
        let templates = adw::PreferencesGroup::new();
        indicators_page.add(&templates);
        let list = fill_page(&owner, &dialog, &indicators_page, "On the chart");
        template_rows(window, &dialog, &templates, &Refresh::rebuilding(&owner, &dialog, &list));

        dialog.present(Some(&window.window));
        match focus {
            Focus::Chart => {}
            Focus::Indicators => dialog.set_visible_page(&indicators_page),
            // One indicator of a template is the template's to change, so the
            // gear on such a chart lands on the list that says so.
            Focus::Indicator(id) => {
                dialog.set_visible_page(&indicators_page);
                open_indicator_panel_for(
                    &owner,
                    &Refresh::rebuilding(&owner, &dialog, &list),
                    id,
                    Panel::Subpage(dialog.clone()),
                );
            }
        }
    }
}

/// A chart template's indicators, in an editor of their own.
///
/// The same list and panels as a chart's Indicators page, over the template
/// rather than the chart: every change is made to the template and drawn at
/// once on every chart using it.
pub struct TemplateEditor;

impl TemplateEditor {
    /// `closed` runs when it goes, for whatever opened it to read the
    /// template again.
    pub fn present(
        window: &Rc<Window>,
        template: u32,
        over: &impl IsA<gtk::Widget>,
        closed: impl Fn() + 'static,
    ) {
        let name = window
            .templates()
            .into_iter()
            .find(|t| t.id == template)
            .map(|t| t.name)
            .unwrap_or_default();
        let dialog = adw::PreferencesDialog::new();
        dialog.set_title(&name);
        dialog.set_content_width(DIALOG_WIDTH);
        dialog.set_content_height((window.window.height() * 85 / 100).max(620));

        let page = adw::PreferencesPage::new();
        page.set_title("Indicators");
        page.set_icon_name(Some("view-list-symbolic"));
        dialog.add(&page);

        let owner = Owner::new(Rc::new(WindowHost { app: window.clone(), template: Some(template) }));
        fill_page(&owner, &dialog, &page, "Indicators in the template");
        dialog.connect_closed(move |_| closed());
        dialog.present(Some(over));
    }
}

/// The window as the indicator settings' host: its focused chart, or one
/// of its templates.
pub struct WindowHost {
    pub app: Rc<Window>,
    /// `None` for the focused chart.
    pub template: Option<u32>,
}

/// The focused chart's indicators, to be edited.
pub fn chart_owner(window: &Rc<Window>) -> Rc<Owner> {
    Owner::new(Rc::new(WindowHost { app: window.clone(), template: None }))
}

impl WindowHost {
    /// The template whose indicators these are: the one being edited, or
    /// the one the focused chart is on. A chart on a template shows the
    /// template's indicators, so changing them from the chart changes the
    /// template — and every chart on it — rather than a copy nobody asked
    /// for.
    fn target(&self) -> Option<u32> {
        self.template.or_else(|| self.app.focused_template())
    }
}

impl Host for WindowHost {
    fn indicators(&self) -> Vec<Indicator> {
        match self.target() {
            None => self.app.indicators(),
            Some(id) => self.app.templates().into_iter().find(|t| t.id == id).map(|t| t.indicators).unwrap_or_default(),
        }
    }

    /// Changing a template's redraws every chart on it, as it changes.
    fn set_indicators(&self, indicators: Vec<Indicator>) {
        match self.target() {
            None => self.app.set_indicators(indicators),
            Some(id) => self.app.set_template_indicators(id, indicators),
        }
    }

    fn theme(&self) -> omacharts_engine::Theme {
        self.app.theme()
    }

    /// The focused chart's, for a template too: it is the chart in front of
    /// whoever is editing, and a template has no bars of its own.
    fn bar_scheme(&self) -> omacharts_engine::BarScheme {
        self.app.bar_scheme()
    }

    fn chart_look(&self) -> (BarColouring, bool) {
        self.app.chart_look()
    }

    fn template(&self) -> Option<u32> {
        self.template
    }

    /// The name of the template a chart's settings are editing through, when
    /// they are: what the page says so the change is not a surprise.
    fn through_template(&self) -> Option<String> {
        if self.template.is_some() {
            return None;
        }
        let id = self.app.focused_template()?;
        self.app.templates().into_iter().find(|t| t.id == id).map(|t| t.name)
    }

    fn estimates_delta(&self) -> bool {
        !self.app.focused_pane_has_ladders()
    }

    fn profile_rows(&self, id: u32) -> Option<usize> {
        self.app.profile_rows(id)
    }

    fn timeframe(&self) -> omacharts_engine::Timeframe {
        self.app.focused_pane().timeframe.get()
    }

    fn market(&self) -> Option<(omacharts_engine::Instrument, Session)> {
        self.app.focused_market()
    }

    fn clock(&self) -> (chrono_tz::Tz, omacharts_engine::clock::TimeFormat) {
        let store = self.app.store();
        let instrument = match self.template {
            None => self.app.focused_market().map(|(instrument, _)| instrument),
            Some(_) => None,
        };
        (crate::clock::zone(store).for_instrument(instrument.as_ref()), crate::clock::format(store))
    }

    fn watch_chart(&self, watcher: Box<dyn Fn() -> bool>) {
        self.app.watch_chart(move || watcher());
    }

    fn chart_changed_elsewhere(&self) {
        self.app.chart_changed_elsewhere();
    }

    fn parent(&self) -> gtk::Widget {
        self.app.window.clone().upcast()
    }
}

/// A way to rebuild something, held by what it rebuilds and let go of later.
type Again = RefCell<Option<Rc<dyn Fn()>>>;

/// Which template the chart is on, and a way into the one it is on.
///
/// Rebuilt whole after every choice, because a choice can be refused — the
/// question about replacing the chart's own indicators can be answered with
/// Cancel — and the rows have to say what the chart is on, not what was
/// clicked.
fn template_rows(
    window: &Rc<Window>,
    dialog: &adw::PreferencesDialog,
    group: &adw::PreferencesGroup,
    list: &Refresh,
) {
    let rows: Rc<RefCell<Vec<gtk::Widget>>> = Rc::new(RefCell::new(Vec::new()));
    // Holds the function that rebuilds these rows, which the rows themselves
    // call; let go when the dialog closes, or the two would keep each other.
    let rebuild: Rc<Again> = Rc::new(RefCell::new(None));
    let fill = {
        let window = window.clone();
        let group = group.clone();
        let list = list.clone();
        let rebuild = rebuild.clone();
        move || {
            for row in rows.borrow_mut().drain(..) {
                group.remove(&row);
            }
            let templates = window.templates();
            let current = window.focused_template();

            let choice = adw::ComboRow::new();
            choice.set_title("Chart template");
            let mut names = vec!["None".to_string()];
            names.extend(templates.iter().map(|t| t.name.clone()));
            let model = gtk::StringList::new(&[]);
            for name in &names {
                model.append(name);
            }
            choice.set_model(Some(&model));
            let at = current
                .and_then(|id| templates.iter().position(|t| t.id == id))
                .map(|at| at + 1)
                .unwrap_or(0);
            choice.set_selected(at as u32);
            {
                let window = window.clone();
                let list = list.clone();
                let rebuild = rebuild.clone();
                let ids: Vec<u32> = templates.iter().map(|t| t.id).collect();
                choice.connect_selected_notify(move |row| {
                    let chosen = match row.selected() as usize {
                        0 => None,
                        at => ids.get(at - 1).copied(),
                    };
                    if chosen == window.focused_template() {
                        return;
                    }
                    let list = list.clone();
                    let rebuild = rebuild.clone();
                    // Off the row's own signal: the rows are rebuilt by what
                    // this does, and this row is one of them.
                    let window = window.clone();
                    glib::idle_add_local_once(move || {
                        window.choose_template(chosen, move || {
                            list.run();
                            let again = rebuild.borrow().clone();
                            if let Some(rebuild) = again {
                                rebuild();
                            }
                        });
                    });
                });
            }
            group.add(&choice);
            rows.borrow_mut().push(choice.upcast());

            let legend = adw::SwitchRow::new();
            legend.set_title(LEGEND_LABEL);
            legend.set_active(window.shows_legend());
            {
                let window = window.clone();
                legend.connect_active_notify(move |row| window.set_shows_legend(row.is_active()));
            }
            group.add(&legend);
            rows.borrow_mut().push(legend.upcast());
            // No way into a second editor for the template: the list below
            // is the template's while the chart is on one, and says so.
        }
    };
    let fill: Rc<dyn Fn()> = Rc::new(fill);
    *rebuild.borrow_mut() = Some(fill.clone());
    dialog.connect_closed(move |_| {
        rebuild.borrow_mut().take();
    });
    fill();
}

/// The row a chart's title is typed into.
pub const TITLE_LABEL: &str = "Title";

/// The switch for a chart's whole indicator legend, here and in the
/// preferences, where it is the default for new charts.
pub const LEGEND_LABEL: &str = "Show legend for indicators";

pub const TRADING_SYMBOL_LABEL: &str = "Trading symbol";

/// The switch for marking the account's fills on the chart.
pub const SHOW_FILLS_LABEL: &str = "Show fills";

/// The Trading tab's icon: orders going out and fills coming back, apart
/// from the lists the other tabs wear.
pub const TRADING_ICON: &str = "network-transmit-receive-symbolic";

/// What the chart trades: none, its own symbol, until
/// one is picked, MES for a chart of ES. Its data stays what it charts.
fn trading_page(window: &Rc<Window>) -> adw::PreferencesPage {
    let page = adw::PreferencesPage::new();
    page.set_title("Trading");
    page.set_icon_name(Some(TRADING_ICON));
    let group = adw::PreferencesGroup::new();
    let row = adw::ActionRow::new();
    row.set_title(TRADING_SYMBOL_LABEL);
    row.set_activatable(true);
    let value = gtk::Label::new(None);
    value.add_css_class("dim-label");
    let clear = gtk::Button::from_icon_name("edit-clear-symbolic");
    clear.add_css_class("flat");
    clear.set_valign(gtk::Align::Center);
    clear.set_tooltip_text(Some("Trade the chart's own symbol"));
    row.add_suffix(&value);
    row.add_suffix(&clear);
    group.add(&row);
    let fills = adw::SwitchRow::new();
    fills.set_title(SHOW_FILLS_LABEL);
    group.add(&fills);
    page.add(&group);

    let id = window.focused_pane().id;
    fills.set_active(window.shows_fills(id));
    {
        let window = window.clone();
        fills.connect_active_notify(move |row| window.set_shows_fills(id, row.is_active()));
    }
    let show = {
        let window = Rc::downgrade(window);
        let value = value.downgrade();
        let clear = clear.downgrade();
        move || {
            let (Some(window), Some(value), Some(clear)) = (window.upgrade(), value.upgrade(), clear.upgrade()) else {
                return false;
            };
            let traded = window.trading_symbol_named(id);
            value.set_text(traded.as_deref().unwrap_or("None"));
            clear.set_visible(traded.is_some());
            value.root().is_some()
        }
    };
    show();
    window.watch_chart(show);
    {
        let window = window.clone();
        row.connect_activated(move |_| window.find_trading_symbol(id));
    }
    {
        let window = window.clone();
        clear.connect_clicked(move |_| window.set_trading_symbol(id, None));
    }
    page
}

/// The chart title, first, since it is what the chart is called. Kept
/// as it is typed, and blank for none.
fn title_group(window: &Rc<Window>) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::new();
    let row = adw::EntryRow::new();
    row.set_title(TITLE_LABEL);
    row.set_text(&window.chart_title());
    // Set while the row is brought up to date with a title given elsewhere,
    // so that reading it is not taken for typing it.
    let syncing = Rc::new(std::cell::Cell::new(false));
    let typed = window.clone();
    let quiet = syncing.clone();
    row.connect_changed(move |row| {
        if !quiet.get() {
            typed.set_chart_title(&row.text());
        }
    });
    group.add(&row);

    // Titled in the header or by a command while this is open, the row reads
    // the title again; not while it says the same thing, which is every time
    // it is the row's own typing that changed it, trailing space and all.
    let weak = Rc::downgrade(window);
    let shown = row.downgrade();
    window.watch_chart(move || {
        let (Some(window), Some(row)) = (weak.upgrade(), shown.upgrade()) else { return false };
        if row.root().is_none() {
            return false;
        }
        let title = window.chart_title();
        if crate::ui::window::clean_title(&row.text()) != title {
            syncing.set(true);
            row.set_text(&title);
            syncing.set(false);
        }
        true
    });
    group
}

fn bars_group(window: &Rc<Window>) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::new();
    group.set_title("Bars");

    // The styles, then None: hiding the bars keeps the style they had, and
    // the colouring row keeps describing it, greyed until they are back.
    let mut names: Vec<&str> = BarStyle::ALL.iter().map(|s| s.label()).collect();
    names.push(crate::ui::window::NO_BARS_LABEL);
    let hidden_at = BarStyle::ALL.len() as u32;
    let row = adw::ComboRow::new();
    row.set_title("Style");
    row.set_model(Some(&gtk::StringList::new(&names)));
    row.set_selected(match window.bars_hidden() {
        true => hidden_at,
        false => BarStyle::ALL.iter().position(|s| *s == window.bar_style()).unwrap_or(0) as u32,
    });

    let colouring = adw::ComboRow::new();
    colouring.set_title("Colouring");
    fill_colourings(&colouring, window.bar_colouring());
    colouring.set_sensitive(!window.bars_hidden());

    // The two switches worn over the colouring. Greyed with it while the
    // bars are hidden; the hollow rise also on OHLC bars, which have no
    // body to leave open, and the swap on a colouring that paints the same
    // either way.
    let hollow = adw::SwitchRow::new();
    hollow.set_title(HOLLOW_LABEL);
    hollow.set_active(window.bars_hollow_up());
    let swapped = adw::SwitchRow::new();
    swapped.set_title(SWAP_LABEL);
    swapped.set_active(window.bars_swapped());
    let theme = window.theme();
    let shown = !window.bars_hidden();
    hollow.set_sensitive(shown && hollow_applies(window.bar_style()));
    hollow.set_visible(hollow_offered(window.bar_style()));
    swapped.set_sensitive(shown && two_toned(&theme, window.bar_colouring()));

    // Set while the rows are being brought up to date with a chart changed
    // from elsewhere, so that reading the change is not taken for making it.
    let syncing = Rc::new(std::cell::Cell::new(false));

    // How thick OHLC bars are drawn, offered only while they are.
    let thickness = {
        let window = window.clone();
        let syncing = syncing.clone();
        controls::bounded_row(
            THICKNESS_LABEL,
            None,
            f64::from(window.ohlc_thickness()),
            f64::from(crate::chart_defaults::MIN_OHLC_THICKNESS),
            f64::from(crate::chart_defaults::MAX_OHLC_THICKNESS),
            1.0,
            0,
            move |px| {
                let px = px.round() as u8;
                if !syncing.get() && px != window.ohlc_thickness() {
                    window.set_ohlc_thickness(px);
                }
            },
        )
    };
    thickness.row.set_visible(!window.bars_hidden() && window.bar_style() == BarStyle::Ohlc);

    let window_for_style = window.clone();
    let colouring_for_style = colouring.clone();
    let hollow_for_style = hollow.clone();
    let swapped_for_style = swapped.clone();
    let syncing_for_style = syncing.clone();
    let thickness_for_style = thickness.row.clone();
    row.connect_selected_notify(move |row| {
        if syncing_for_style.get() {
            return;
        }
        let ohlc = BarStyle::ALL.get(row.selected() as usize) == Some(&BarStyle::Ohlc);
        thickness_for_style.set_visible(ohlc);
        if row.selected() == hidden_at {
            window_for_style.set_bars_hidden(true);
            colouring_for_style.set_sensitive(false);
            hollow_for_style.set_sensitive(false);
            swapped_for_style.set_sensitive(false);
        } else if let Some(style) = BarStyle::ALL.get(row.selected() as usize).copied() {
            window_for_style.set_bar_style(style);
            colouring_for_style.set_sensitive(true);
            hollow_for_style.set_sensitive(hollow_applies(style));
            hollow_for_style.set_visible(hollow_offered(style));
            let theme = window_for_style.theme();
            swapped_for_style.set_sensitive(two_toned(&theme, window_for_style.bar_colouring()));
        }
    });
    group.add(&row);
    group.add(&thickness.row);

    let window_for_colouring = window.clone();
    let swapped_for_colouring = swapped.clone();
    let syncing_for_colouring = syncing.clone();
    colouring.connect_selected_notify(move |row| {
        if filling_colourings(row) || syncing_for_colouring.get() {
            return;
        }
        let Some(chosen) = BarColouring::ALL.get(row.selected() as usize).copied() else {
            return;
        };
        if chosen != window_for_colouring.bar_colouring() {
            window_for_colouring.set_bar_colouring(chosen);
        }
        let theme = window_for_colouring.theme();
        swapped_for_colouring.set_sensitive(two_toned(&theme, chosen));
    });
    group.add(&colouring);

    let window_for_hollow = window.clone();
    let syncing_for_hollow = syncing.clone();
    hollow.connect_active_notify(move |row| {
        if !syncing_for_hollow.get() && row.is_active() != window_for_hollow.bars_hollow_up() {
            window_for_hollow.set_bars_hollow_up(row.is_active());
        }
    });
    group.add(&hollow);

    let window_for_swapped = window.clone();
    let syncing_for_swapped = syncing.clone();
    swapped.connect_active_notify(move |row| {
        if !syncing_for_swapped.get() && row.is_active() != window_for_swapped.bars_swapped() {
            window_for_swapped.set_bars_swapped(row.is_active());
        }
    });
    group.add(&swapped);

    // A command, or a theme, can change the chart while this is open; the
    // rows read it again rather than go on describing what it was. Weakly,
    // so the window does not keep a closed dialog's rows alive.
    let weak = Rc::downgrade(window);
    let rows = (row.downgrade(), colouring.downgrade(), hollow.downgrade(), swapped.downgrade());
    let thickness = (thickness.row.downgrade(), thickness.adjustment.clone());
    window.watch_chart(move || {
        let (Some(window), Some(row), Some(colouring), Some(hollow), Some(swapped)) =
            (weak.upgrade(), rows.0.upgrade(), rows.1.upgrade(), rows.2.upgrade(), rows.3.upgrade())
        else {
            return false;
        };
        if row.root().is_none() {
            return false;
        }
        syncing.set(true);
        sync_bars(&window, &row, &colouring, &hollow, &swapped, hidden_at);
        if let Some(shown) = thickness.0.upgrade() {
            shown.set_visible(!window.bars_hidden() && window.bar_style() == BarStyle::Ohlc);
            thickness.1.set_value(f64::from(window.ohlc_thickness()));
        }
        syncing.set(false);
        true
    });

    group
}

/// Bring the Bars rows to what the focused chart is wearing now.
fn sync_bars(
    window: &Rc<Window>,
    row: &adw::ComboRow,
    colouring: &adw::ComboRow,
    hollow: &adw::SwitchRow,
    swapped: &adw::SwitchRow,
    hidden_at: u32,
) {
    let hidden = window.bars_hidden();
    row.set_selected(match hidden {
        true => hidden_at,
        false => BarStyle::ALL.iter().position(|s| *s == window.bar_style()).unwrap_or(0) as u32,
    });
    let current = window.bar_colouring();
    colouring.set_selected(BarColouring::ALL.iter().position(|c| *c == current).unwrap_or(0) as u32);
    colouring.set_sensitive(!hidden);
    hollow.set_active(window.bars_hollow_up());
    swapped.set_active(window.bars_swapped());
    hollow.set_sensitive(!hidden && hollow_applies(window.bar_style()));
    hollow.set_visible(hollow_offered(window.bar_style()));
    swapped.set_sensitive(!hidden && two_toned(&window.theme(), current));
}

/// What the two switches over the colouring are called, in a chart's
/// settings and in the preferences alike.
pub const HOLLOW_LABEL: &str = "Hollow up bars";
pub const SWAP_LABEL: &str = "Swap up and down colours";
/// How wide a settings dialog is, a chart's, a study's and a drawing's.
pub const DIALOG_WIDTH: i32 = omacharts_widgets::dialogs::SETTINGS_WIDTH;

/// The row for how many pixels wide OHLC bars are drawn.
pub const THICKNESS_LABEL: &str = property_rows::THICKNESS;

/// Whether turning `colouring` round would change what it paints, which is
/// when the switch for it is offered. Monochrome paints a rise and a fall
/// alike, so there is nothing to turn round.
pub fn two_toned(theme: &omacharts_engine::Theme, colouring: BarColouring) -> bool {
    colouring.scheme(theme).is_two_toned()
}

/// Whether a hollow rise would show on a bar of `style`: a candle has a
/// body to leave open, an OHLC bar does not.
pub fn hollow_applies(style: BarStyle) -> bool {
    style == BarStyle::Candles
}

/// Whether the hollow rise is offered at all: greyed on OHLC bars, which
/// are still bars, and gone on a line, which has no up bars to speak of.
pub fn hollow_offered(style: BarStyle) -> bool {
    style != BarStyle::Line
}

/// Offer the four palettes, with `current` selected.
///
/// A new model selects its first entry on the way in, which the row's own
/// handler would take for a choice; [`filling_colourings`] tells it not to.
pub fn fill_colourings(row: &adw::ComboRow, current: BarColouring) {
    let names: Vec<&str> = BarColouring::ALL.iter().map(|c| c.label()).collect();
    row.set_widget_name(FILLING);
    row.set_model(Some(&gtk::StringList::new(&names)));
    row.set_selected(BarColouring::ALL.iter().position(|c| *c == current).unwrap_or(0) as u32);
    row.set_widget_name("");
}

const FILLING: &str = "filling-colourings";

/// Whether the row is being refilled rather than chosen from.
pub fn filling_colourings(row: &adw::ComboRow) -> bool {
    row.widget_name() == FILLING
}

/// The way back to what a new chart opens with, for a chart that has wandered.
fn defaults_group(window: &Rc<Window>, dialog: &adw::PreferencesDialog) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::new();
    let row = adw::ActionRow::new();
    row.set_title("Reset to defaults");
    let button = gtk::Button::with_label("Reset");
    button.set_valign(gtk::Align::Center);
    let window = window.clone();
    let dialog = dialog.clone();
    button.connect_clicked(move |_| {
        // Every row above was read off the chart as it was; reopening is how
        // they come to read it as it is, once it is settled.
        dialog.close();
        let again = window.clone();
        window.reset_chart_to_defaults(move || again.open_chart_settings());
    });
    row.add_suffix(&button);
    row.set_activatable_widget(Some(&button));
    group.add(&row);
    group
}

/// What the two scales do: whether price fits the bars by itself, the lines the
/// scales draw across the chart, and the way back out of a scale you have
/// stretched into uselessness.
///
/// Here because the price scale's own options were only ever reachable by
/// right-clicking the strip of prices down the right-hand edge, which is a menu
/// nobody opens unless they already suspect it is there. That menu stays as it
/// was: these rows drive the same window actions, so the tick in the menu and
/// the switch here cannot come to different conclusions.
fn scales_group(window: &Rc<Window>) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::new();
    group.set_title("Scales");

    let auto = adw::ActionRow::new();
    auto.set_title("Auto scale price");
    let switch = gtk::Switch::new();
    switch.set_valign(gtk::Align::Center);
    switch.set_active(window.focused_pane().view.price_auto());
    let window_for_auto = window.clone();
    switch.connect_state_set(move |_, on| {
        // The action toggles rather than taking a value, so it is activated
        // only when the chart is not already the way the switch now reads.
        //
        // Spelled out through WidgetExt because an application window is an
        // action group in its own right, and that one holds the window's own
        // actions — it knows nothing of the "chart." group inserted on it.
        if window_for_auto.focused_pane().view.price_auto() != on {
            let _ = gtk::prelude::WidgetExt::activate_action(
                &window_for_auto.window,
                "chart.auto-scale",
                None,
            );
        }
        glib::Propagation::Proceed
    });
    auto.add_suffix(&switch);
    auto.set_activatable_widget(Some(&switch));
    group.add(&auto);

    let log = adw::ActionRow::new();
    log.set_title("Logarithmic scale");
    let switch = gtk::Switch::new();
    switch.set_valign(gtk::Align::Center);
    switch.set_active(window.log_scale());
    let window_for_log = window.clone();
    switch.connect_state_set(move |_, on| {
        if window_for_log.log_scale() != on {
            window_for_log.set_log_scale(on);
        }
        glib::Propagation::Proceed
    });
    log.add_suffix(&switch);
    log.set_activatable_widget(Some(&switch));
    group.add(&log);

    let grid = adw::ActionRow::new();
    grid.set_title("Gridlines");
    let switch = gtk::Switch::new();
    switch.set_valign(gtk::Align::Center);
    switch.set_active(window.show_grid());
    let window_for_grid = window.clone();
    switch.connect_state_set(move |_, on| {
        window_for_grid.set_show_grid(on);
        glib::Propagation::Proceed
    });
    grid.add_suffix(&switch);
    grid.set_activatable_widget(Some(&switch));
    group.add(&grid);

    let reset = adw::ActionRow::new();
    reset.set_title("Reset chart");
    let button = gtk::Button::with_label("Reset");
    button.set_valign(gtk::Align::Center);
    button.set_tooltip_text(Some(&shortcuts::tooltip("Reset chart", "chart.reset-view")));
    let window_for_reset = window.clone();
    button.connect_clicked(move |_| {
        let _ = gtk::prelude::WidgetExt::activate_action(
            &window_for_reset.window,
            "chart.reset-view",
            None,
        );
    });
    reset.add_suffix(&button);
    reset.set_activatable_widget(Some(&button));
    group.add(&reset);

    group
}

fn session_group(window: &Rc<Window>) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::new();
    group.set_title("Session");

    let names: Vec<&str> = Session::ALL.iter().map(|s| s.label()).collect();
    let session = window.session();

    let row = adw::ComboRow::new();
    row.set_title("Hours");
    row.set_model(Some(&gtk::StringList::new(&names)));
    row.set_selected(Session::ALL.iter().position(|s| *s == session).unwrap_or(0) as u32);

    let for_hours = window.clone();
    row.connect_selected_notify(move |row| {
        let Some(chosen) = Session::ALL.get(row.selected() as usize).copied() else { return };
        for_hours.set_session(chosen);
        for_hours.refresh();
    });
    group.add(&row);

    // Only on a root whose feed can join its months either way: anything
    // else has nothing to choose, and no row.
    if window.offers_continuous() {
        use omacharts_engine::continuous::Adjustment;
        let names: Vec<&str> = Adjustment::ALL.iter().map(|a| a.label()).collect();
        let joined = adw::ComboRow::new();
        joined.set_title("Continuous futures");
        joined.set_model(Some(&gtk::StringList::new(&names)));
        let now = window.continuous();
        joined.set_selected(Adjustment::ALL.iter().position(|a| *a == now).unwrap_or(0) as u32);
        let window = window.clone();
        joined.connect_selected_notify(move |row| {
            let Some(chosen) = Adjustment::ALL.get(row.selected() as usize).copied() else { return };
            window.set_continuous(chosen);
        });
        group.add(&joined);
    }
    group
}

/// Whether this chart shows what other charts of its symbol draw, and
/// whether it sends them what is drawn here.
///
/// Two switches, where there used to be a choice of eleven. A chart could
/// belong to one of nine numbered drawing groups, or to the global one, or
/// to none — which meant every drawing, every chart and every command
/// carried a question nobody asked twice: *which group?* Both switches are
/// on, so a drawing made on a symbol is a drawing about the symbol, and the
/// two that are worth having are reachable one at a time:
///
/// - turn **sending** off to sketch on one chart without it appearing on
///   every other view of the same instrument;
/// - turn **showing** off to keep one chart clean of what is already drawn.
///
/// They are independent on purpose. A chart that shows but does not send is
/// a reference view; one that sends but does not show is rarer and still
/// coherent, and neither needs explaining in terms of the other.
fn drawings_group(window: &Rc<Window>) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::new();
    group.set_title("Drawings");

    let pane = window.focused_pane();

    let shows = adw::ComboRow::new();
    shows.set_title(SHOW_DRAWINGS_LABEL);
    {
        use omacharts_engine::drawings::ShowDrawings;
        let choices: Vec<&str> = ShowDrawings::ALL.iter().map(|s| s.label()).collect();
        shows.set_model(Some(&gtk::StringList::new(&choices)));
        shows.set_selected(ShowDrawings::ALL.iter().position(|s| *s == pane.shows_drawings.get()).unwrap_or(0) as u32);
        let window = window.clone();
        shows.connect_selected_notify(move |row| {
            let Some(chosen) = ShowDrawings::ALL.get(row.selected() as usize).copied() else { return };
            window.set_shows_drawings(chosen);
        });
    }
    group.add(&shows);

    let sends = adw::SwitchRow::new();
    sends.set_title("Send drawings to other charts");
    sends.set_active(pane.sends_drawings.get());
    {
        let window = window.clone();
        sends.connect_active_notify(move |row| window.set_sends_drawings(row.is_active()));
    }
    group.add(&sends);

    group
}

#[cfg(test)]
pub(crate) fn profile_titles(window: &Rc<Window>, kind: Kind) -> Vec<(String, Vec<String>)> {
    let owner = chart_owner(window);
    let indicator = Indicator::new(1, kind);
    let colour = owner.theme().ui.accent.clone();
    let mut groups = vec![parameters_group(&owner, &Refresh::none(), &indicator)];
    groups.extend(appearance_groups(&owner, &Refresh::none(), &colour, &indicator));
    groups
        .iter()
        .map(|group| (group.title().to_string(), profile_rows::titles(group)))
        .collect()
}

/// A drawing's rows held to a study's: the two dialogs are built apart, and
/// laid out alike.
#[cfg(test)]
mod drawing_tests {
    use super::*;
    use crate::store::Store;
    use omacharts_engine::drawings::{Kind as DrawingKind, Preset, Style as DrawingStyle};
    use omacharts_widgets::drawing_settings;

    /// A profile drawing's properties are the study's, in the same groups
    /// and the same order, less what only a study has: its period, and
    /// whether only the newest is drawn.
    #[test]
    fn a_profile_drawing_has_the_studys_rows_in_the_studys_order() {
        crate::ui::on_gtk(|| {
            let store = Rc::new(Store::memory().expect("a store"));
            let window = crate::ui::window::test_window(&store);
            let only_the_study = ["Reset every", PROFILE_LAST_ONLY_LABEL];
            let study: Vec<(String, Vec<String>)> =
                profile_titles(&window, omacharts_engine::indicators::Kind::VolumeProfile)
                .into_iter()
                .map(|(title, rows)| {
                    (
                        title,
                        rows.into_iter()
                            .filter(|row| !only_the_study.contains(&row.as_str()))
                            .collect(),
                    )
                })
                .collect();

            let drawing: Vec<(String, Vec<String>)> = drawing_settings::editor_groups(
                &window.theme(),
                DrawingKind::Profile,
                DrawingStyle::profile(Preset::Violet),
            )
            .iter()
            .map(|group| (group.title().to_string(), profile_rows::titles(group)))
            .collect();

            assert_eq!(drawing, study);
            // Every profile dialog is laid out alike: the same groups in the
            // same order, the same rows in each, what only one kind has where
            // the other's would be.
            use profile_rows as r;
            let names = |rows: &[&str]| rows.iter().map(|row| row.to_string()).collect::<Vec<_>>();
            let volume = |parameters: &[&str]| {
                vec![
                    ("Parameters".to_string(), names(parameters)),
                    (r::STYLE_GROUP.to_string(), names(&[
                        r::DISPLAY_AS, r::ALIGNMENT, r::WIDTH, r::COLOUR, r::OUTLINE,
                        r::OUTLINE_COLOUR, r::OUTLINE_THICKNESS, r::OUTLINE_STYLE,
                    ])),
                    (r::VALUE_AREA_GROUP.to_string(), names(&[r::HIGHLIGHT, r::COLOUR, r::OUTSIDE_TRANSPARENCY])),
                    (r::POC_GROUP.to_string(), names(&[r::SHOW, r::COLOUR, r::THICKNESS, r::LINE_STYLE])),
                ]
            };
            assert_eq!(
                drawing,
                volume(&[r::METHOD, r::COLOURING, FLOW_BUY_LABEL, FLOW_SELL_LABEL, r::AUTOMATIC, r::ROWS, r::VALUE_AREA]),
                "a drawing has no period"
            );
            let study = profile_titles(&window, omacharts_engine::indicators::Kind::VolumeProfile);
            assert_eq!(
                study,
                volume(&["Reset every", r::LAST_ONLY, r::METHOD, r::COLOURING, FLOW_BUY_LABEL, FLOW_SELL_LABEL, r::AUTOMATIC, r::ROWS, r::VALUE_AREA])
            );
            let market = profile_titles(&window, omacharts_engine::indicators::Kind::MarketProfile);
            assert_eq!(
                market,
                vec![
                    ("Parameters".to_string(), names(&[
                        "Reset every", r::LAST_ONLY, MARKET_MINUTES_LABEL, r::AUTOMATIC,
                        MARKET_ROW_TICKS_LABEL, r::VALUE_AREA,
                    ])),
                    (r::STYLE_GROUP.to_string(), names(&[
                        r::DISPLAY_AS, r::ALIGNMENT, r::EXPAND_LAST, r::WIDTH, r::COLOUR, r::OUTLINE,
                        r::OUTLINE_COLOUR, r::OUTLINE_THICKNESS, r::OUTLINE_STYLE, r::OPEN_CLOSE,
                        r::OPEN_COLOUR, r::CLOSE_COLOUR,
                    ])),
                    (r::VALUE_AREA_GROUP.to_string(), names(&[r::HIGHLIGHT, r::COLOUR, r::OUTSIDE_TRANSPARENCY])),
                    (r::POC_GROUP.to_string(), names(&[r::SHOW, r::COLOUR, r::THICKNESS, r::LINE_STYLE])),
                ],
                "the volume profile's, less its method, with its open and close after the outline and its last profile expanded under where it is placed"
            );
            assert!(
                drawing
                    .iter()
                    .any(|(_, rows)| rows.iter().any(|row| row == profile_rows::AUTOMATIC)),
                "{drawing:?}"
            );
        });
    }

}
