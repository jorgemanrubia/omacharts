//! Settings that belong to the chart rather than the app.
//!
//! Reached from the gear beside the legend, because that is where you are
//! looking when you want them. Two pages: how the bars are read and scaled,
//! and what is drawn on top of them.

mod time;

use std::cell::{OnceCell, RefCell};
use std::rc::Rc;

use adw::prelude::*;
use omacharts_engine::indicators::{
    self, Kind, LineStyle, Params, Reset, Stroke, MAX_FILL_ALPHA, MAX_PANE_SHARE, MIN_FILL_ALPHA,
    MIN_PANE_SHARE,
};
use omacharts_engine::theme::ColorChoice;
use omacharts_engine::{BarColouring, BarStyle, Indicator, Session};

use crate::store::Store;
use crate::ui::colors;
use crate::ui::controls;
use crate::ui::dialogs;
use crate::ui::drop_line;
use crate::ui::shortcuts;
use crate::ui::window::Window;

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
    pub fn present(window: &Rc<Window>, store: Rc<Store>) {
        ChartSettings::open(window, store, Focus::Chart);
    }

    /// Straight to the indicators.
    pub fn present_indicators(window: &Rc<Window>, store: Rc<Store>) {
        ChartSettings::open(window, store, Focus::Indicators);
    }

    /// Straight to one indicator's panel, for the gear beside it on the chart.
    pub fn present_indicator(window: &Rc<Window>, store: Rc<Store>, id: u32) {
        ChartSettings::open(window, store, Focus::Indicator(id));
    }

    fn open(window: &Rc<Window>, store: Rc<Store>, focus: Focus) {
        let dialog = adw::PreferencesDialog::new();
        dialog.set_title("Chart");
        dialog.set_content_width(560);
        // Most of the window's height, like the preferences: the indicator
        // list and an indicator's own page are long, and every row that fits
        // is a scroll saved. The width stays the column the rows sit in.
        dialog.set_content_height((window.window.height() * 85 / 100).max(620));

        let chart_page = adw::PreferencesPage::new();
        chart_page.set_title("Chart");
        chart_page.set_icon_name(Some("preferences-system-symbolic"));
        chart_page.add(&bars_group(window, &store));
        chart_page.add(&scales_group(window));
        chart_page.add(&session_group(window));
        chart_page.add(&drawings_group(window));
        chart_page.add(&defaults_group(window, &dialog));
        dialog.add(&chart_page);

        let indicators_page = adw::PreferencesPage::new();
        indicators_page.set_title("Indicators");
        indicators_page.set_icon_name(Some("view-list-symbolic"));
        dialog.add(&indicators_page);

        {
            // Alt and an arrow between the pages, and the keyboard goes with
            // it. The pages are named here rather than asked of the dialog: a
            // preferences dialog keeps them in a view stack of its own, and
            // the ones it was given are the ones it has.
            let pages = [chart_page.clone(), indicators_page.clone()];
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

        let owner = Owner::chart(window);
        // Above the list, because it decides what the list is: the chart's
        // own, or a template's shown here and changed elsewhere.
        let templates = adw::PreferencesGroup::new();
        indicators_page.add(&templates);
        let list = IndicatorList::new(&indicators_page, "On the chart");
        // Wired once here rather than on every rebuild: the button outlives
        // the rows, and reconnecting it each time would stack up handlers.
        let owner_for_add = owner.clone();
        let refresh_for_add = Refresh::rebuilding(&owner, &dialog, &list);
        list.add.connect_clicked(move |_| {
            pick_indicator(&owner_for_add, &refresh_for_add);
        });
        rebuild_indicators(&owner, &dialog, &list);
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
        dialog.set_content_width(560);
        dialog.set_content_height((window.window.height() * 85 / 100).max(620));

        let page = adw::PreferencesPage::new();
        page.set_title("Indicators");
        page.set_icon_name(Some("view-list-symbolic"));
        dialog.add(&page);

        let owner = Owner::template(window, template);
        let list = IndicatorList::new(&page, "Indicators in the template");
        let owner_for_add = owner.clone();
        let refresh_for_add = Refresh::rebuilding(&owner, &dialog, &list);
        list.add.connect_clicked(move |_| {
            pick_indicator(&owner_for_add, &refresh_for_add);
        });
        rebuild_indicators(&owner, &dialog, &list);
        dialog.connect_closed(move |_| closed());
        dialog.present(Some(over));
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

fn bars_group(window: &Rc<Window>, store: &Rc<Store>) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::new();
    group.set_title("Bars");

    let names: Vec<&str> = BarStyle::ALL.iter().map(|s| s.label()).collect();
    let row = adw::ComboRow::new();
    row.set_title("Style");
    row.set_model(Some(&gtk::StringList::new(&names)));
    row.set_selected(
        BarStyle::ALL.iter().position(|s| *s == window.bar_style()).unwrap_or(0) as u32
    );

    let colouring = adw::ComboRow::new();
    colouring.set_title("Colouring");
    fill_colourings(&colouring, window.bar_style(), window.bar_colouring());

    let window_for_style = window.clone();
    let colouring_for_style = colouring.clone();
    row.connect_selected_notify(move |row| {
        if let Some(style) = BarStyle::ALL.get(row.selected() as usize).copied() {
            window_for_style.set_bar_style(style);
            fill_colourings(&colouring_for_style, style, window_for_style.bar_colouring());
        }
    });
    group.add(&row);

    let window_for_colouring = window.clone();
    colouring.connect_selected_notify(move |row| {
        if filling_colourings(row) {
            return;
        }
        let style = window_for_colouring.bar_style();
        let Some(chosen) = BarColouring::offered(style).get(row.selected() as usize).copied()
        else {
            return;
        };
        if chosen != window_for_colouring.bar_colouring().for_style(style) {
            window_for_colouring.set_bar_colouring(chosen);
        }
    });
    group.add(&colouring);

    // One switch for the window rather than one per chart: it is about how the
    // charts behave towards each other, which is not a property of any one of
    // them.
    let sync = adw::ActionRow::new();
    sync.set_title("Crosshair across Follow charts");
    sync.set_subtitle("The pointer on one draws the same moment on the others");
    let switch = gtk::Switch::new();
    switch.set_valign(gtk::Align::Center);
    switch.set_active(store.setting_bool(crate::ui::window::SETTING_SYNC_CROSSHAIR, true));
    let window_for_sync = window.clone();
    let store_for_sync = store.clone();
    switch.connect_state_set(move |_, on| {
        store_for_sync.set_setting_bool(crate::ui::window::SETTING_SYNC_CROSSHAIR, on);
        if !on {
            window_for_sync.clear_echoes();
        }
        glib::Propagation::Proceed
    });
    sync.add_suffix(&switch);
    group.add(&sync);

    group
}

/// Offer the colourings a bar of `style` can be painted in, with `current`
/// selected — as an OHLC bar draws it, so a hollow chart turned into OHLC
/// reads as coloured, which is what it shows.
///
/// A new model selects its first entry on the way in, which the row's own
/// handler would take for a choice; [`filling_colourings`] tells it not to.
pub fn fill_colourings(row: &adw::ComboRow, style: BarStyle, current: BarColouring) {
    let offered = BarColouring::offered(style);
    let names: Vec<&str> = offered.iter().map(|c| c.label(style)).collect();
    row.set_widget_name(FILLING);
    row.set_model(Some(&gtk::StringList::new(&names)));
    let shown = current.for_style(style);
    row.set_selected(offered.iter().position(|c| *c == shown).unwrap_or(0) as u32);
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
    row.set_subtitle("Bars, session, scale and drawings as a new chart opens, set in Preferences");
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
    // The gestures, which are otherwise something you either know or do not:
    // nothing on the chart says the prices down the side can be dragged.
    group.set_description(Some(
        "Drag the price scale to stretch it, the time scale to squeeze time. \
         Double-click either to put it back.",
    ));

    let auto = adw::ActionRow::new();
    auto.set_title("Auto scale price");
    auto.set_subtitle(
        "Fits the visible bars vertically. Turn it off to drag the chart up and down",
    );
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
    log.set_subtitle("Equal heights are equal percentage moves, for long histories");
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
    grid.set_subtitle("The scales and their labels stay either way");
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
    reset.set_subtitle("Both scales back to how the chart opened, at the latest bars");
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

    let window = window.clone();
    row.connect_selected_notify(move |row| {
        let Some(chosen) = Session::ALL.get(row.selected() as usize).copied() else { return };
        window.set_session(chosen);
        window.refresh();
    });

    group.add(&row);
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

    let shows = adw::SwitchRow::new();
    shows.set_title("Show drawings from other charts");
    shows.set_active(pane.shows_drawings.get());
    {
        let window = window.clone();
        shows.connect_active_notify(move |row| window.set_shows_drawings(row.is_active()));
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

/// Whose indicators an editor is changing: the focused chart's, or a chart
/// template's.
///
/// The list, an indicator's panel and the picker are one editor whichever it
/// is, so they are built against this rather than against the window. A
/// template is edited with exactly the rows a chart is, and the two cannot
/// drift apart into an editor that offers a parameter the other has lost.
pub struct Owner {
    app: Rc<Window>,
    /// The application window, which panels are sized against and shown over.
    pub window: adw::ApplicationWindow,
    /// `None` for the focused chart.
    template: Option<u32>,
}

impl Owner {
    pub fn chart(app: &Rc<Window>) -> Rc<Owner> {
        Rc::new(Owner { app: app.clone(), window: app.window.clone(), template: None })
    }

    pub fn template(app: &Rc<Window>, template: u32) -> Rc<Owner> {
        Rc::new(Owner { app: app.clone(), window: app.window.clone(), template: Some(template) })
    }

    /// The template whose indicators these are: the one being edited, or
    /// the one the focused chart is on. A chart on a template shows the
    /// template's indicators, so changing them from the chart changes the
    /// template — and every chart on it — rather than a copy nobody asked
    /// for.
    fn target(&self) -> Option<u32> {
        self.template.or_else(|| self.app.focused_template())
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

    fn indicators(&self) -> Vec<Indicator> {
        match self.target() {
            None => self.app.indicators(),
            Some(id) => self
                .app
                .templates()
                .into_iter()
                .find(|t| t.id == id)
                .map(|t| t.indicators)
                .unwrap_or_default(),
        }
    }

    /// Changing a template's redraws every chart on it, as it changes.
    fn set_indicators(&self, indicators: Vec<Indicator>) {
        match self.target() {
            None => self.app.set_indicators(indicators),
            Some(id) => self.app.set_template_indicators(id, indicators),
        }
    }

    fn next_indicator_id(&self) -> u32 {
        self.indicators().iter().map(|i| i.id).max().unwrap_or(0) + 1
    }

    fn theme(&self) -> omacharts_engine::Theme {
        self.app.theme()
    }

    /// The focused chart's, for a template too: it is the chart in front of
    /// whoever is editing, and a template has no bars of its own.
    fn bar_scheme(&self) -> omacharts_engine::BarScheme {
        self.app.bar_scheme()
    }

    /// How many rows a profile is drawing. A template draws nothing of its
    /// own, but the chart in front of whoever is editing it does.
    fn profile_rows(&self, id: u32) -> Option<usize> {
        match self.template {
            None => self.app.profile_rows(id),
            Some(_) => None,
        }
    }
}

/// The one group the indicator list lives in, and the rows currently in it.
///
/// Tracked explicitly because a preferences page does not hand back the groups
/// you added — its own children are a scroller and a clamp — so walking them
/// looking for things to remove finds nothing, and every rebuild quietly
/// appends another copy of the list.
#[derive(Clone)]
pub struct IndicatorList {
    group: adw::PreferencesGroup,
    rows: Rc<RefCell<Vec<gtk::Widget>>>,
    /// The + in the group's header. Kept so the hotkey can hang the picker off
    /// the same button the pointer would have used: a popover that appears
    /// somewhere else depending on how you asked for it is two features.
    add: gtk::Button,
}

impl IndicatorList {
    fn new(page: &adw::PreferencesPage, title: &str) -> IndicatorList {
        let group = adw::PreferencesGroup::new();
        group.set_title(title);
        page.add(&group);

        let add = gtk::Button::from_icon_name("list-add-symbolic");
        add.add_css_class("flat");
        let tip = shortcuts::tooltip_with_key("Add an indicator", shortcuts::ADD_INDICATOR);
        add.set_tooltip_text(Some(&tip));
        add.set_valign(gtk::Align::Center);
        group.set_header_suffix(Some(&add));

        IndicatorList { group, rows: Rc::new(RefCell::new(Vec::new())), add }
    }

    fn clear(&self) {
        for row in self.rows.borrow_mut().drain(..) {
            self.group.remove(&row);
        }
    }

    fn push(&self, row: &impl IsA<gtk::Widget>) {
        self.group.add(row);
        self.rows.borrow_mut().push(row.clone().upcast());
    }
}

/// Where the line can be seen: the colour it is drawn in right now, and the
/// samples showing it.
///
/// The samples used to be painted in whatever the colour was when the panel was
/// built, so changing the colour left a purple swatch above four blue lines.
/// They ask for the colour every time they are drawn instead, and the colour
/// row repaints them when it changes one.
#[derive(Clone)]
struct Ink {
    colour: Rc<dyn Fn() -> String>,
    samples: Rc<RefCell<Vec<glib::WeakRef<gtk::DrawingArea>>>>,
}

impl Ink {
    fn new(colour: impl Fn() -> String + 'static) -> Ink {
        Ink { colour: Rc::new(colour), samples: Rc::new(RefCell::new(Vec::new())) }
    }

    fn colour(&self) -> String {
        (self.colour)()
    }

    fn watch(&self, area: &gtk::DrawingArea) {
        self.samples.borrow_mut().push(area.downgrade());
    }

    fn repaint(&self) {
        for area in self.samples.borrow().iter() {
            if let Some(area) = area.upgrade() {
                area.queue_draw();
            }
        }
    }
}

/// Something to run when an indicator changes, so whatever is showing it
/// redraws.
///
/// A callback rather than the preferences dialog and its list, because adding
/// an indicator no longer needs either — the picker opens straight from the
/// chart — and a row that sets a line width should not have to know which of
/// the two it is serving.
#[derive(Clone)]
struct Refresh(Rc<dyn Fn()>);

impl Refresh {
    /// Nothing behind the panel to redraw.
    fn none() -> Refresh {
        Refresh(Rc::new(|| {}))
    }

    fn rebuilding(
        owner: &Rc<Owner>,
        dialog: &adw::PreferencesDialog,
        list: &IndicatorList,
    ) -> Refresh {
        let owner = owner.clone();
        let dialog = dialog.clone();
        let list = list.clone();
        Refresh(Rc::new(move || rebuild_indicators(&owner, &dialog, &list)))
    }

    fn run(&self) {
        (self.0)()
    }
}

/// Rebuild the list of indicators.
///
/// A list, and only a list: each row says what the indicator is and lets you
/// turn it off or take it away. Everything else lives on its own panel, because
/// a column of expanders holding every parameter of every indicator is a shape
/// you cannot read.
fn rebuild_indicators(
    owner: &Rc<Owner>,
    dialog: &adw::PreferencesDialog,
    list: &IndicatorList,
) {
    list.clear();
    let indicators = owner.indicators();
    let refresh = Refresh::rebuilding(owner, dialog, list);

    // On a template, what is being edited is the template, and the page says
    // so before anything is changed: a change here reaches every chart on it.
    let through = owner.through_template();
    let description = match (&through, owner.template.is_some(), indicators.is_empty()) {
        // The heading names the template; this says only what that means.
        (Some(_), _, _) => Some("A change here reaches every chart that uses it.".to_string()),
        (None, true, _) => None,
        (None, false, true) => Some("Nothing yet.".to_string()),
        (None, false, false) => Some(
            "Drag to reorder. The list is the chart, top to bottom; colours come from the theme."
                .to_string(),
        ),
    };
    list.group.set_description(description.as_deref());
    list.group.set_title(match (&through, owner.template) {
        (Some(name), _) => format!("On the {name} template"),
        (None, Some(_)) => "Indicators in the template".to_string(),
        (None, None) => "On the chart".to_string(),
    }
    .as_str());

    // The price plot is a row of the list because strips can be stacked above it
    // as well as below, and a list showing only the strips could not say which
    // side of the price each one is on. A chart with no strips has no stack to
    // describe, so it is left out there.
    let stacked = indicators.iter().any(|i| i.kind.in_own_pane());
    let price_at = if stacked { price_row_at(&indicators) } else { usize::MAX };
    for (at, indicator) in indicators.iter().enumerate() {
        if at == price_at {
            list.push(&price_row(owner, &refresh));
        }
        list.push(&indicator_row(owner, dialog, &refresh, &indicators, indicator));
    }
    if price_at == indicators.len() {
        list.push(&price_row(owner, &refresh));
    }

    if indicators.is_empty() {
        let empty = adw::ActionRow::new();
        empty.set_title("Add an indicator");
        empty.set_subtitle("Moving averages, VWAP, volume profile");
        empty.set_activatable(true);
        let owner_for_empty = owner.clone();
        let refresh_for_empty = refresh.clone();
        empty.connect_activated(move |_| {
            pick_indicator(&owner_for_empty, &refresh_for_empty);
        });
        list.push(&empty);
    }
}

/// Where the price plot sits in the list: above the first strip that is below
/// it, and at the end when every strip is above it.
///
/// The strips above the price are listed before the strips below it, which is
/// what every way of reordering them maintains — so one boundary is enough to
/// place the price among them.
fn price_row_at(indicators: &[Indicator]) -> usize {
    indicators
        .iter()
        .position(|i| i.kind.in_own_pane() && !i.above_price)
        .unwrap_or(indicators.len())
}

/// The price plot's own line in the list.
///
/// Not a thing you can configure or remove — it is the chart — so it carries
/// nothing but its name. It is a place to drop a strip on, which is how a strip
/// is moved across the price from here.
fn price_row(owner: &Rc<Owner>, refresh: &Refresh) -> adw::ActionRow {
    let row = adw::ActionRow::new();
    row.set_title("Price");
    row.set_subtitle("Candles, and everything drawn over them");
    // No handle and no dot. Every other row wears both, so their absence is
    // what says this row is the chart itself rather than another indicator —
    // and a mark here would read as the handle it is not.
    row.add_css_class("dim-label");

    let moving = |value: &glib::Value| value.get::<String>().ok()?.parse::<u32>().ok();
    let watched = owner.clone();
    let owner = owner.clone();
    let refresh = refresh.clone();
    let target = drop_line::target(
        &row,
        // A strip from above the price becomes the first below it, and one
        // from below the last above it: the line goes on the far side.
        move |value| {
            let id = moving(value)?;
            let all = watched.indicators();
            let strip = all.iter().find(|i| i.id == id && i.kind.in_own_pane())?;
            Some(if strip.above_price { drop_line::Edge::After } else { drop_line::Edge::Before })
        },
        move |value| {
            let Some(moving) = moving(value) else { return false };
            let mut all = owner.indicators();
            // Dropped on the price: the strip changes sides, which is the one
            // thing dropping it on another row cannot say.
            if !indicators::cross_price(&mut all, moving) {
                return false;
            }
            owner.set_indicators(all);
            refresh.run();
            true
        },
    );
    row.add_controller(target);
    row
}

/// One line in the list: what it is, whether it is drawn, and a way in.
fn indicator_row(
    owner: &Rc<Owner>,
    dialog: &adw::PreferencesDialog,
    refresh: &Refresh,
    all: &[Indicator],
    indicator: &Indicator,
) -> adw::ActionRow {
    let id = indicator.id;
    let colour = drawn_colour(owner, all, id);
    let row = adw::ActionRow::new();
    row.set_title(&indicator.label());
    row.set_subtitle(indicator.kind.name());
    row.set_activatable(true);

    // A dot in the indicator's own colour, so the list matches the chart.
    let swatch = gtk::DrawingArea::new();
    swatch.set_size_request(12, 12);
    swatch.set_valign(gtk::Align::Center);
    swatch.set_draw_func(move |_, cr, width, height| {
        let radius = (width.min(height) as f64) / 2.0;
        colors::set_source(cr, &colour);
        cr.arc(width as f64 / 2.0, height as f64 / 2.0, radius, 0.0, std::f64::consts::TAU);
        let _ = cr.fill();
    });
    row.add_prefix(&swatch);

    // Order is a property of the list, so its controls sit on the list side of
    // the row. It decides two things at once: which strip is stacked above
    // which under the chart, and which line is drawn over which on it.
    // The handle is prepended rather than appended, so it ends up left of the
    // swatch: order is a property of the list, and the list runs down the left
    // edge. It is there to say the row can be dragged — the drag itself works
    // anywhere on the row that a control is not already using.
    let handle = gtk::Image::from_icon_name("list-drag-handle-symbolic");
    handle.add_css_class("dim-label");
    handle.set_tooltip_text(Some("Drag to reorder"));
    row.add_prefix(&handle);
    wire_reorder(owner, refresh, &row, id);

    let visible = gtk::Switch::new();
    visible.set_active(indicator.visible);
    visible.set_valign(gtk::Align::Center);
    visible.set_tooltip_text(Some("Show on the chart"));
    let owner_for_visible = owner.clone();
    visible.connect_state_set(move |_, state| {
        update(&owner_for_visible, id, |i| i.visible = state);
        glib::Propagation::Proceed
    });
    row.add_suffix(&visible);

    let remove = gtk::Button::from_icon_name("user-trash-symbolic");
    remove.add_css_class("flat");
    remove.set_valign(gtk::Align::Center);
    remove.set_tooltip_text(Some("Remove"));
    let owner_for_remove = owner.clone();
    let refresh_for_remove = refresh.clone();
    remove.connect_clicked(move |_| {
        let kept: Vec<Indicator> =
            owner_for_remove.indicators().into_iter().filter(|i| i.id != id).collect();
        owner_for_remove.set_indicators(kept);
        refresh_for_remove.run();
    });
    row.add_suffix(&remove);

    let owner_for_open = owner.clone();
    let refresh_for_open = refresh.clone();
    let dialog_for_open = dialog.clone();
    row.connect_activated(move |_| {
        open_indicator_panel_for(
            &owner_for_open,
            &refresh_for_open,
            id,
            Panel::Subpage(dialog_for_open.clone()),
        );
    });
    row
}

/// Let a row be picked up and dropped on another to change the order.
///
/// Dragging rather than a pair of arrows, which is what the watchlist already
/// does and what a list of half a dozen things wants: moving the bottom one to
/// the top is one gesture rather than five clicks.
///
/// The id travels as the payload rather than the position, because the list is
/// rebuilt while the drag is in flight and a position would by then mean a
/// different row.
fn wire_reorder(owner: &Rc<Owner>, refresh: &Refresh, row: &adw::ActionRow, id: u32) {
    let source = gtk::DragSource::new();
    source.set_actions(gtk::gdk::DragAction::MOVE);
    let payload = id.to_string();
    source.connect_prepare(move |_, _, _| {
        Some(gtk::gdk::ContentProvider::for_value(&payload.to_value()))
    });
    row.add_controller(source);

    // Where the dragged indicator is now and where it is going, in the list
    // as it stands — which is the order the rows are shown in.
    let places = {
        let owner = owner.clone();
        move |value: &glib::Value| {
            let moving = value.get::<String>().ok()?.parse::<u32>().ok()?;
            let indicators = owner.indicators();
            Some((
                indicators.iter().position(|i| i.id == moving)?,
                indicators.iter().position(|i| i.id == id)?,
            ))
        }
    };
    let landing = places.clone();
    let owner = owner.clone();
    let refresh = refresh.clone();
    let target = drop_line::target(
        row,
        move |value| landing(value).and_then(|(from, to)| drop_line::edge(from, to)),
        move |value| {
            let Some((from, to)) = places(value) else { return false };
            if from == to {
                return false;
            }
            let mut indicators = owner.indicators();
            // Taken out before the destination is used, so dropping downwards
            // lands after the row you dropped on and upwards lands in its place —
            // which is what the pointer was over either way.
            // A strip dropped next to a strip on the other side of the price has
            // crossed it: the row it was dropped on says which side that is. Read
            // before the drag is lifted out, because that shifts everything after
            // it — the row dropped on included.
            let landing = indicators
                .get(to)
                .filter(|onto| onto.kind.in_own_pane())
                .map(|onto| onto.above_price);
            let mut dragged = indicators.remove(from);
            if let (true, Some(side)) = (dragged.kind.in_own_pane(), landing) {
                dragged.above_price = side;
            }
            indicators.insert(to, dragged);
            owner.set_indicators(indicators);
            refresh.run();
            true
        },
    );
    row.add_controller(target);
}

/// Where an indicator's settings are shown.
enum Panel {
    /// Pushed over the list it was opened from, with a Done button.
    Subpage(adw::PreferencesDialog),
    /// A dialog of its own, with one button that adds the indicator. Used by
    /// the add path, which can start from the chart with no settings window
    /// open at all, so there is nothing to push a page onto. With the
    /// template the chart was on before the indicator went onto it, to go
    /// back onto if it is never added.
    Add(Option<u32>),
}

/// One indicator's settings.
///
/// Changes apply as you make them, on the chart, whichever way this was
/// opened: you are looking at the thing you are configuring. For the add path
/// that means the indicator is already drawn while you set it up, and
/// dismissing the dialog takes it back off again — the Add button is how you
/// say you meant it.
fn open_indicator_panel_for(owner: &Rc<Owner>, refresh: &Refresh, id: u32, panel: Panel) {
    let indicators = owner.indicators();
    let Some(indicator) = indicators.iter().find(|i| i.id == id).cloned() else {
        return;
    };
    let colour = drawn_colour(owner, &indicators, id);

    let page = adw::PreferencesPage::new();
    // Said before anything can be changed, however the panel was reached —
    // the list, the gear on the chart, the add path — because a change here
    // reaches charts that are not on screen.
    if let Some(name) = owner.through_template() {
        let warning = adw::PreferencesGroup::new();
        let banner = adw::ActionRow::new();
        banner.set_title(&format!("Editing the {name} template"));
        banner.set_subtitle("A change here reaches every chart that uses it.");
        banner.add_prefix(&gtk::Image::from_icon_name("dialog-information-symbolic"));
        // A note, not a control: the keyboard goes past it to the first
        // setting, which is what a panel opens on.
        banner.set_focusable(false);
        banner.set_activatable(false);
        warning.add(&banner);
        page.add(&warning);
    }
    if indicator.kind == Kind::SessionTimes {
        time::fill(owner, refresh, &page, indicator.id);
    } else {
        page.add(&parameters_group(owner, refresh, &indicator));
        if let Some(group) = appearance_group(owner, refresh, &colour, &indicator) {
            page.add(&group);
        }
        for group in band_groups(owner, refresh, &colour, &indicator) {
            page.add(&group);
        }
        page.add(&legend_group(owner, refresh, &indicator));
    }

    let header = adw::HeaderBar::new();
    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&header);
    toolbar.set_content(Some(&page));

    match panel {
        // Nothing packed: an AdwHeaderBar inside a navigation page draws its
        // own back button, and settings here apply as you change them, so a
        // Done beside it would be the same action offered twice.
        Panel::Subpage(dialog) => {
            let subpage = adw::NavigationPage::new(&toolbar, &indicator.label());
            dialog.push_subpage(&subpage);
            focus_first_parameter(&page);
        }
        Panel::Add(was_on) => {
            let modal = adw::Dialog::new();
            modal.set_title(&indicator.label());
            modal.set_content_width(560);
            modal.set_content_height((owner.window.height() * 85 / 100).max(620));
            modal.set_child(Some(&toolbar));

            // Cancel on the left, the suggested action on the right, and the
            // dialog's own close button turned off — the arrangement GNOME
            // uses for anything that creates something. A bare close button
            // next to Add says nothing about what closing would do.
            header.set_show_start_title_buttons(false);
            header.set_show_end_title_buttons(false);

            let added = Rc::new(std::cell::Cell::new(false));

            let cancel = gtk::Button::with_label("Cancel");
            let modal_for_cancel = modal.clone();
            cancel.connect_clicked(move |_| {
                modal_for_cancel.close();
            });
            header.pack_start(&cancel);

            let add = gtk::Button::with_label("Add");
            add.add_css_class("suggested-action");
            let added_on_click = added.clone();
            let modal_for_add = modal.clone();
            add.connect_clicked(move |_| {
                added_on_click.set(true);
                modal_for_add.close();
            });
            add.set_tooltip_text(Some(&format!("Add ({})", dialogs::commit_label())));
            header.pack_end(&add);

            // The key goes through the button rather than around it, so there
            // is one path by which an indicator is kept and one place to look
            // when it is not.
            let add_on_key = add.clone();
            dialogs::commit_on_ctrl_enter(&modal, move || add_on_key.emit_clicked());

            // Closed any other way — Escape, the close button, clicking out —
            // means it was never added, so it comes back off the chart.
            let owner_for_close = owner.clone();
            let refresh_for_close = refresh.clone();
            modal.connect_closed(move |_| {
                if added.get() {
                    return;
                }
                let back = was_on.filter(|template| {
                    owner_for_close.app.templates().iter().any(|t| t.id == *template)
                });
                if back.is_some() {
                    owner_for_close.app.set_template(back);
                } else {
                    let kept: Vec<Indicator> =
                        owner_for_close.indicators().into_iter().filter(|i| i.id != id).collect();
                    owner_for_close.set_indicators(kept);
                }
                refresh_for_close.run();
            });

            modal.present(Some(&owner.window));
            focus_first_parameter(&page);
        }
    }
}

/// Land the keyboard on the first thing you came to change.
///
/// A panel opens on its first parameter — the period, usually — so the figure
/// can be typed straight away rather than reached for. Which control that is
/// differs by indicator, and GTK's own forward-tab rule finds it without this
/// having to know: a period for a moving average, an anchor for a VWAP, a
/// slider for a volume pane. Starting from the page rather than the dialog is
/// what skips the Cancel button in the header.
///
/// It waits for the panel to be on screen, because a row that has not been
/// mapped yet cannot take the keyboard — the same frame-late lesson as the
/// sidebar, which cannot be focused until the frame after it is revealed. Once
/// only: the panel is mapped again every time it is scrolled back into view.
///
/// Whatever it lands on that can be typed into has its value selected: the
/// number already there is a default, and replacing one is quicker than
/// editing it a character at a time.
fn focus_first_parameter(page: &adw::PreferencesPage) {
    let landed = std::cell::Cell::new(false);
    page.connect_map(move |page| {
        if landed.replace(true) {
            return;
        }
        if !page.child_focus(gtk::DirectionType::TabForward) {
            return;
        }
        let Some(window) = page.root().and_then(|root| root.downcast::<gtk::Window>().ok()) else {
            return;
        };
        let Some(focus) = gtk::prelude::GtkWindowExt::focus(&window) else {
            return;
        };
        if let Ok(text) = focus.downcast::<gtk::Text>() {
            text.select_region(0, -1);
        }
    });
}

/// The indicator's own line: colour, weight, pattern.
fn appearance_group(
    owner: &Rc<Owner>,
    refresh: &Refresh,
    colour: &str,
    indicator: &Indicator,
) -> Option<adw::PreferencesGroup> {
    let id = indicator.id;

    // Volume is bars, not a line: a colour for each direction, and no
    // thickness or pattern to ask about.
    if indicator.kind == Kind::Volume {
        return Some(volume_colours(owner, refresh, indicator));
    }

    // A volume profile is bars and a mark, not a line, so it is asked about
    // its two colours and nothing about thickness or pattern.
    if indicator.kind == Kind::VolumeProfile {
        return Some(profile_colours(owner, refresh, colour, indicator));
    }

    let group = adw::PreferencesGroup::new();
    group.set_title("Line");

    let ink = {
        let owner = owner.clone();
        Ink::new(move || drawn_colour(&owner, &owner.indicators(), id))
    };

    group.add(&colour_row(
        owner,
        refresh,
        &ink,
        "Colour",
        colour,
        indicator.color.clone(),
        move |indicator, choice| indicator.color = choice,
        id,
    ));
    for row in stroke_rows(owner, refresh, &ink, id, "", indicator.stroke, {
        move |indicator: &mut Indicator, stroke: Stroke| indicator.stroke = stroke
    }) {
        group.add(&row);
    }

    // A strip's second line follows the theme the way a point of control
    // does: unset is the next colour along from the first line's, so the two
    // never come out the same.
    let second = match &indicator.params {
        Params::Stochastic { d_color, .. } => Some(("%D colour", d_color.clone())),
        Params::Macd { signal_color, .. } => Some(("Signal colour", signal_color.clone())),
        _ => None,
    };
    if let Some((title, chosen)) = second {
        let shown = owner.theme().companion_or(chosen.as_ref(), colour);
        group.add(&colour_row(
            owner,
            refresh,
            &Ink::new(String::new),
            title,
            &shown,
            chosen,
            move |indicator, choice| match &mut indicator.params {
                Params::Stochastic { d_color, .. } => *d_color = choice,
                Params::Macd { signal_color, .. } => *signal_color = choice,
                _ => {}
            },
            id,
        ));
    }
    Some(group)
}

/// Width and style, as one expander-free pair of rows.
///
/// Returns the group they belong to so callers can drop it straight in.
fn stroke_rows(
    owner: &Rc<Owner>,
    refresh: &Refresh,
    ink: &Ink,
    id: u32,
    prefix: &str,
    stroke: Stroke,
    apply: impl Fn(&mut Indicator, Stroke) + Clone + 'static,
) -> Vec<adw::ActionRow> {
    // Rows rather than a group of their own. A PreferencesGroup added inside
    // another draws its own card, which is why thickness and style sat in a
    // second box under the same heading as the colour.
    let mut rows = Vec::new();

    let apply_width = apply.clone();
    rows.push(picker_row(
        owner,
        refresh,
        ink,
        id,
        &format!("{prefix}Thickness"),
        &WIDTHS.map(|width| (width, Stroke { width, style: stroke.style })),
        stroke,
        move |indicator, picked| apply_width(indicator, picked),
    ));

    rows.push(picker_row(
        owner,
        refresh,
        ink,
        id,
        &format!("{prefix}Line style"),
        &LineStyle::ALL.map(|style| {
            (LINE_SAMPLE_WIDTH, Stroke { width: stroke.width.max(LINE_SAMPLE_WIDTH), style })
        }),
        stroke,
        move |indicator, picked| {
            // Only the pattern: picking a style must not quietly change the
            // weight the row above it is showing.
            apply(indicator, Stroke { width: stroke.width, style: picked.style });
        },
    ));

    rows
}

/// The weights on offer. Four, plus none.
///
/// A spin button asking for a number between zero and five in halves is a
/// question nobody has an opinion about; what people want is a thicker line or
/// a thinner one, and they want to see which. Zero stays because a VWAP band
/// uses it to draw its fill with no outline.
const WIDTHS: [f64; 5] = [0.0, 1.0, 1.5, 2.5, 4.0];

/// What a style sample is drawn at, so dashes and dots are legible whatever
/// weight the line itself is set to.
const LINE_SAMPLE_WIDTH: f64 = 1.5;

/// A row of drawn samples, one of them pressed.
///
/// Grouped toggles rather than a dropdown: the whole point is seeing the
/// options beside each other.
#[allow(clippy::too_many_arguments)]
fn picker_row(
    owner: &Rc<Owner>,
    refresh: &Refresh,
    ink: &Ink,
    id: u32,
    title: &str,
    options: &[(f64, Stroke)],
    current: Stroke,
    apply: impl Fn(&mut Indicator, Stroke) + Clone + 'static,
) -> adw::ActionRow {
    let row = adw::ActionRow::new();
    row.set_title(title);

    let strip = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    strip.add_css_class("linked");
    strip.add_css_class("stroke-picker");
    strip.set_valign(gtk::Align::Center);

    let mut first: Option<gtk::ToggleButton> = None;
    for (sample, stroke) in options {
        let button = gtk::ToggleButton::new();
        button.add_css_class("flat");
        let area = stroke_sample(*sample, stroke.style, ink);
        ink.watch(&area);
        button.set_child(Some(&area));
        button.set_tooltip_text(Some(&describe_stroke(*sample, stroke.style)));
        match &first {
            Some(anchor) => button.set_group(Some(anchor)),
            None => first = Some(button.clone()),
        }
        // Whichever sample describes what is set now.
        button.set_active(if options.len() == WIDTHS.len() {
            (sample - current.width).abs() < 0.01
        } else {
            stroke.style == current.style
        });

        let owner_for_pick = owner.clone();
        let refresh_for_pick = refresh.clone();
        let apply_for_pick = apply.clone();
        let picked = *stroke;
        button.connect_toggled(move |button| {
            if !button.is_active() {
                return;
            }
            let apply = apply_for_pick.clone();
            update(&owner_for_pick, id, move |indicator| apply(indicator, picked));
            refresh_for_pick.run();
        });
        strip.append(&button);
    }

    row.add_suffix(&strip);
    row
}

fn describe_stroke(width: f64, style: LineStyle) -> String {
    if width <= 0.0 {
        return "No line".to_string();
    }
    format!("{} · {width}px", style.label())
}

/// One option, drawn as the line it would produce.
fn stroke_sample(width: f64, style: LineStyle, ink: &Ink) -> gtk::DrawingArea {
    let area = gtk::DrawingArea::new();
    area.set_content_width(34);
    area.set_content_height(18);
    let ink = ink.clone();
    area.set_draw_func(move |_, cr, w, h| {
        if width <= 0.0 {
            return;
        }
        let (w, h) = (w as f64, h as f64);
        colors::set_source(cr, &ink.colour());
        cr.set_line_width(width);
        cr.set_dash(&style.dashes(width), 0.0);
        cr.set_line_cap(gtk::cairo::LineCap::Round);
        let y = (h / 2.0).round() + if width % 2.0 == 1.0 { 0.5 } else { 0.0 };
        cr.move_to(5.0, y);
        cr.line_to(w - 5.0, y);
        let _ = cr.stroke();
    });
    area
}

/// A colour row: a quiet way back to the default, then the swatch.
///
/// Reset is enabled whenever a colour is pinned — not only when the hex
/// differs from the theme's. Pinning the same colour still stops the indicator
/// following the theme, so there is still something to undo.
#[allow(clippy::too_many_arguments)]
fn colour_row(
    owner: &Rc<Owner>,
    refresh: &Refresh,
    ink: &Ink,
    title: &str,
    current: &str,
    choice: Option<ColorChoice>,
    apply: impl Fn(&mut Indicator, Option<ColorChoice>) + Clone + 'static,
    id: u32,
) -> adw::ActionRow {
    let row = adw::ActionRow::new();
    row.set_title(title);

    let default = gtk::Button::with_label("Reset");
    default.add_css_class("flat");
    default.add_css_class("subtle-link");
    default.set_valign(gtk::Align::Center);
    default.set_tooltip_text(Some("Back to the colour the theme gives it"));
    default.set_sensitive(choice.is_some());

    // Only the list behind this panel is rebuilt when a colour changes, so
    // these two have to keep each other up to date — otherwise Reset stays
    // greyed out until the panel is reopened, right after the one action that
    // gives it something to undo.
    let owner_for_colour = owner.clone();
    let refresh_for_colour = refresh.clone();
    let ink_for_colour = ink.clone();
    let apply_for_colour = apply.clone();
    let default_weak = default.downgrade();
    let button = crate::ui::palette::picker(
        &owner.theme(),
        choice,
        current,
        move |picked| {
            let apply = apply_for_colour.clone();
            update(&owner_for_colour, id, move |indicator| {
                apply(indicator, Some(picked.clone()))
            });
            if let Some(default) = default_weak.upgrade() {
                default.set_sensitive(true);
            }
            ink_for_colour.repaint();
            refresh_for_colour.run();
        },
    );

    let owner_for_default = owner.clone();
    let refresh_for_default = refresh.clone();
    let ink_for_default = ink.clone();
    let button_weak = button.downgrade();
    default.connect_clicked(move |default| {
        let apply = apply.clone();
        update(&owner_for_default, id, move |indicator| apply(indicator, None));
        default.set_sensitive(false);
        // Show what it reverted to. The default is not a fixed colour — it is
        // whichever of the palette this indicator's siblings have left free —
        // so it has to be asked for after the change, not guessed before it.
        if let Some(button) = button_weak.upgrade() {
            let indicators = owner_for_default.indicators();
            crate::ui::palette::show(&button, &drawn_colour(&owner_for_default, &indicators, id));
        }
        ink_for_default.repaint();
        refresh_for_default.run();
    });

    row.add_suffix(&default);
    row.add_suffix(&button);
    row
}

fn parameters_group(
    owner: &Rc<Owner>,
    refresh: &Refresh,
    indicator: &Indicator,
) -> adw::PreferencesGroup {
    let id = indicator.id;
    let group = adw::PreferencesGroup::new();
    group.set_title("Parameters");

    match &indicator.params {
        Params::MovingAverage { period } => {
            group.add(&spin_row(
                owner,
                refresh,
                id,
                "Period",
                *period as f64,
                1.0,
                500.0,
                1.0,
                move |indicator, value| {
                    indicator.params = Params::MovingAverage { period: value as usize };
                },
            ));
        }
        Params::Vwap { reset, .. } => {
            group.add(&reset_row(owner, refresh, id, *reset));
        }
        // The multiple the bands sit at is edited with the band itself, below,
        // which is where a VWAP's is edited too.
        Params::Bollinger { period, .. } => {
            group.add(&spin_row(
                owner, refresh, id, "Period", *period as f64, 1.0, 500.0, 1.0,
                move |indicator, value| {
                    if let Params::Bollinger { period, .. } = &mut indicator.params {
                        *period = value as usize;
                    }
                },
            ));
        }
        Params::Volume { height, .. } => {
            group.add(&pane_height_row(owner, refresh, id, *height));
        }
        Params::Rsi { period, height, overbought, oversold } => {
            group.add(&spin_row(
                owner, refresh, id, "Period", *period as f64, 2.0, 200.0, 1.0,
                move |indicator, value| {
                    if let Params::Rsi { period, .. } = &mut indicator.params {
                        *period = value as usize;
                    }
                },
            ));
            for row in level_rows(owner, refresh, id, *overbought, *oversold) {
                group.add(&row);
            }
            group.add(&pane_height_row(owner, refresh, id, *height));
        }
        Params::Stochastic { period, k_smooth, d_period, height, overbought, oversold, .. } => {
            group.add(&spin_row(
                owner, refresh, id, "%K length", *period as f64, 1.0, 200.0, 1.0,
                move |indicator, value| {
                    if let Params::Stochastic { period, .. } = &mut indicator.params {
                        *period = value as usize;
                    }
                },
            ));
            group.add(&spin_row(
                owner, refresh, id, "%K smoothing", *k_smooth as f64, 1.0, 50.0, 1.0,
                move |indicator, value| {
                    if let Params::Stochastic { k_smooth, .. } = &mut indicator.params {
                        *k_smooth = value as usize;
                    }
                },
            ));
            group.add(&spin_row(
                owner, refresh, id, "%D smoothing", *d_period as f64, 1.0, 50.0, 1.0,
                move |indicator, value| {
                    if let Params::Stochastic { d_period, .. } = &mut indicator.params {
                        *d_period = value as usize;
                    }
                },
            ));
            for row in level_rows(owner, refresh, id, *overbought, *oversold) {
                group.add(&row);
            }
            group.add(&pane_height_row(owner, refresh, id, *height));
        }
        Params::Macd { fast, slow, signal, height, .. } => {
            for row in macd_length_rows(owner, refresh, id, *fast as f64, *slow as f64) {
                group.add(&row);
            }
            // The same kind of row as the two above it, rather than the
            // stepper the other strips use: three lengths that look like two
            // and a half read as a mistake.
            let owner_for_signal = owner.clone();
            let refresh_for_signal = refresh.clone();
            group.add(
                &controls::bounded_row(
                    "Signal length",
                    None,
                    *signal as f64,
                    1.0,
                    MAX_MACD_LENGTH,
                    1.0,
                    0,
                    move |length| {
                        update(&owner_for_signal, id, |indicator| {
                            if let Params::Macd { signal, .. } = &mut indicator.params {
                                *signal = length as usize;
                            }
                        });
                        refresh_for_signal.run();
                    },
                )
                .row,
            );
            group.add(&pane_height_row(owner, refresh, id, *height));
        }
        Params::Atr { period, height } => {
            group.add(&spin_row(
                owner, refresh, id, "Period", *period as f64, 2.0, 200.0, 1.0,
                move |indicator, value| {
                    if let Params::Atr { period, .. } = &mut indicator.params {
                        *period = value as usize;
                    }
                },
            ));
            group.add(&pane_height_row(owner, refresh, id, *height));
        }
        Params::VolumeProfile { reset, rows, value_area, .. } => {
            group.add(&reset_row(owner, refresh, id, *reset));
            for row in rows_rows(owner, refresh, id, *rows) {
                group.add(&row);
            }
            group.add(
                &percent_row(
                    owner,
                    refresh,
                    id,
                    "Value area %",
                    *value_area,
                    0.10,
                    1.0,
                    move |indicator, share| {
                        if let Params::VolumeProfile { value_area, .. } = &mut indicator.params {
                            *value_area = share;
                        }
                    },
                ),
            );
        }
        // Edited on a page of its own; see `time`.
        Params::SessionTimes { .. } => {}
    }
    group
}

/// Last on every indicator's page: whether the chart's legend names it.
///
/// Apart from whether it is drawn at all, which is the switch on its row in
/// the list: an indicator can be on the chart and not take a line at the top
/// of it, which is what session shading wants and an average might.
fn legend_group(owner: &Rc<Owner>, refresh: &Refresh, indicator: &Indicator) -> adw::PreferencesGroup {
    let id = indicator.id;
    let group = adw::PreferencesGroup::new();
    group.set_title("Legend");
    let row = adw::SwitchRow::new();
    row.set_title("Show in chart legend");
    row.set_active(indicator.in_legend);
    let owner = owner.clone();
    let refresh = refresh.clone();
    row.connect_active_notify(move |row| {
        let on = row.is_active();
        update(&owner, id, |indicator| indicator.in_legend = on);
        refresh.run();
    });
    group.add(&row);
    group
}

/// A rising and a falling session's volume bar. Unset, each is the candle's
/// own colour dimmed toward the chart.
fn volume_colours(
    owner: &Rc<Owner>,
    refresh: &Refresh,
    indicator: &Indicator,
) -> adw::PreferencesGroup {
    let id = indicator.id;
    let group = adw::PreferencesGroup::new();
    group.set_title("Colours");

    let (up, down) =
        crate::ui::chart::volume_colours(indicator, &owner.theme(), &owner.bar_scheme());
    let Params::Volume { up_color, down_color, .. } = &indicator.params else { return group };
    // Bars have no line to repaint as a sample.
    let unused = Ink::new(String::new);
    group.add(&colour_row(
        owner,
        refresh,
        &unused,
        "Up",
        &up,
        up_color.clone(),
        move |indicator, choice| {
            if let Params::Volume { up_color, .. } = &mut indicator.params {
                *up_color = choice;
            }
        },
        id,
    ));
    group.add(&colour_row(
        owner,
        refresh,
        &unused,
        "Down",
        &down,
        down_color.clone(),
        move |indicator, choice| {
            if let Params::Volume { down_color, .. } = &mut indicator.params {
                *down_color = choice;
            }
        },
        id,
    ));
    group
}

/// The two colours a volume profile actually has.
fn profile_colours(
    owner: &Rc<Owner>,
    refresh: &Refresh,
    colour: &str,
    indicator: &Indicator,
) -> adw::PreferencesGroup {
    let id = indicator.id;
    let group = adw::PreferencesGroup::new();
    group.set_title("Colours");

    // Neither row has samples to repaint: a profile has no line to show.
    let unused = Ink::new(String::new);
    group.add(&colour_row(
        owner,
        refresh,
        &unused,
        "Profile",
        colour,
        indicator.color.clone(),
        move |indicator, choice| indicator.color = choice,
        id,
    ));

    let Params::VolumeProfile { poc_color, .. } = &indicator.params else { return group };
    // Unset is not "the same as the profile": it is the next colour along the
    // theme's sequence, which is the pair that sequence keeps far apart in hue.
    let shown = owner.theme().companion_or(poc_color.as_ref(), colour);

    group.add(&colour_row(
        owner,
        refresh,
        &unused,
        "Point of control",
        &shown,
        poc_color.clone(),
        move |indicator, choice| {
            if let Params::VolumeProfile { poc_color, .. } = &mut indicator.params {
                *poc_color = choice;
            }
        },
        id,
    ));

    group
}

/// One group per band, because every band has the same half-dozen choices and
/// burying them in expanders only hides which are on.
fn band_groups(
    owner: &Rc<Owner>,
    refresh: &Refresh,
    line_colour: &str,
    indicator: &Indicator,
) -> Vec<adw::PreferencesGroup> {
    let Some(bands) = indicator.params.bands() else { return Vec::new() };
    let id = indicator.id;
    let line_colour = line_colour.to_string();
    let mut groups = Vec::new();

    for (index, band) in bands.iter().enumerate() {
        let group = adw::PreferencesGroup::new();
        // Numbered only where there is more than one to tell apart. "Band 1"
        // on a Bollinger reads as a promise of a Band 2 that is not coming.
        group.set_title(&match bands.len() {
            1 => "Band".to_string(),
            _ => format!("Band {}", index + 1),
        });

        let show = adw::ActionRow::new();
        show.set_title("Show");
        let switch = gtk::Switch::new();
        switch.set_active(band.enabled);
        switch.set_valign(gtk::Align::Center);
        let owner_for_show = owner.clone();
        let refresh_for_show = refresh.clone();
        switch.connect_state_set(move |_, on| {
            update(&owner_for_show, id, move |indicator| {
                if let Some(band) =
                    indicator.params.bands_mut().and_then(|bands| bands.get_mut(index))
                {
                    band.enabled = on;
                }
            });
            refresh_for_show.run();
            glib::Propagation::Proceed
        });
        show.add_suffix(&switch);
        group.add(&show);

        // Tenths, because one, one and a half and two are the figures people
        // reach for, and six is further out than any band is drawn.
        let owner_for_dev = owner.clone();
        let refresh_for_dev = refresh.clone();
        let deviations = controls::bounded_row(
            "Standard deviations",
            None,
            band.deviations,
            0.1,
            6.0,
            0.1,
            1,
            move |value| {
                update(&owner_for_dev, id, move |indicator| {
                    if let Some(band) =
                        indicator.params.bands_mut().and_then(|bands| bands.get_mut(index))
                    {
                        band.deviations = value;
                    }
                });
                refresh_for_dev.run();
            },
        );
        group.add(&deviations.row);

        let shaded = adw::ActionRow::new();
        shaded.set_title("Shaded");
        shaded.set_subtitle("Fill the area between this band's edges");
        let fill = gtk::Switch::new();
        fill.set_active(band.fill);
        fill.set_valign(gtk::Align::Center);
        let owner_for_fill = owner.clone();
        let refresh_for_fill = refresh.clone();
        fill.connect_state_set(move |_, on| {
            update(&owner_for_fill, id, move |indicator| {
                if let Some(band) =
                    indicator.params.bands_mut().and_then(|bands| bands.get_mut(index))
                {
                    band.fill = on;
                }
            });
            refresh_for_fill.run();
            glib::Propagation::Proceed
        });
        shaded.add_suffix(&fill);
        group.add(&shaded);

        // How much of the colour that shading gets, over the whole range the
        // figure has: clear at one end, solid at the other. How heavy a
        // backdrop wants to be is a matter of taste, and the shading is drawn
        // under the candles, so even a solid band has the bars on top of it.
        //
        // The ends come from the engine, as the pane height's do, so the
        // slider and the chart agree about where they are.
        group.add(
            &percent_row(
                owner,
                refresh,
                id,
                "Shading %",
                band.alpha(),
                MIN_FILL_ALPHA,
                MAX_FILL_ALPHA,
                move |indicator, share| {
                    if let Some(band) =
                        indicator.params.bands_mut().and_then(|bands| bands.get_mut(index))
                    {
                        band.fill_alpha = Some(share);
                    }
                },
            ),
        );

        let current = band
            .color
            .as_ref()
            .map(|c| c.resolve(&owner.theme()))
            .unwrap_or_else(|| line_colour.clone());

        // A band's samples follow the band's own colour, falling back to the
        // line's when it has none of its own — the same answer the chart draws.
        let ink = {
            let owner = owner.clone();
            Ink::new(move || {
                let indicators = owner.indicators();
                let band_colour = indicators
                    .iter()
                    .find(|i| i.id == id)
                    .and_then(|i| i.params.bands())
                    .and_then(|bands| bands.get(index))
                    .and_then(|band| band.color.clone())
                    .map(|choice| choice.resolve(&owner.theme()));
                band_colour.unwrap_or_else(|| drawn_colour(&owner, &indicators, id))
            })
        };

        group.add(&colour_row(
            owner,
            refresh,
            &ink,
            "Colour",
            &current,
            band.color.clone(),
            move |indicator, choice| {
                if let Some(band) =
                    indicator.params.bands_mut().and_then(|bands| bands.get_mut(index))
                {
                    band.color = choice;
                }
            },
            id,
        ));

        for row in stroke_rows(owner, refresh, &ink, id, "", band.stroke, {
            move |indicator: &mut Indicator, stroke: Stroke| {
                if let Some(band) =
                    indicator.params.bands_mut().and_then(|bands| bands.get_mut(index))
                {
                    band.stroke = stroke;
                }
            }
        }) {
            group.add(&row);
        }

        groups.push(group);
    }
    groups
}

fn reset_row(
    owner: &Rc<Owner>,
    refresh: &Refresh,
    id: u32,
    current: Reset,
) -> adw::ComboRow {
    let names: Vec<&str> = Reset::ALL.iter().map(|r| r.label()).collect();
    let row = adw::ComboRow::new();
    row.set_title("Reset every");
    row.set_subtitle("Promoted automatically when the chart is too coarse for it.");
    row.set_model(Some(&gtk::StringList::new(&names)));
    row.set_selected(Reset::ALL.iter().position(|r| *r == current).unwrap_or(0) as u32);

    let owner = owner.clone();
    let refresh = refresh.clone();
    row.connect_selected_notify(move |row| {
        let Some(chosen) = Reset::ALL.get(row.selected() as usize).copied() else { return };
        update(&owner, id, |indicator| match &mut indicator.params {
            Params::Vwap { reset, .. } => *reset = chosen,
            Params::VolumeProfile { reset, .. } => *reset = chosen,
            Params::MovingAverage { .. }
            | Params::Bollinger { .. }
            | Params::Volume { .. }
            | Params::Rsi { .. }
            | Params::Macd { .. }
            | Params::Atr { .. }
            | Params::Stochastic { .. }
            | Params::SessionTimes { .. } => {}
        });
        refresh.run();
    });
    row
}

#[allow(clippy::too_many_arguments)]
fn spin_row(
    owner: &Rc<Owner>,
    refresh: &Refresh,
    id: u32,
    title: &str,
    value: f64,
    min: f64,
    max: f64,
    step: f64,
    apply: impl Fn(&mut Indicator, f64) + 'static,
) -> adw::SpinRow {
    let row = adw::SpinRow::with_range(min, max, step);
    row.set_title(title);
    row.set_value(value);
    if step < 1.0 {
        row.set_digits(1);
    }

    let owner = owner.clone();
    let refresh = refresh.clone();
    row.connect_value_notify(move |row| {
        let value = row.value();
        update(&owner, id, |indicator| apply(indicator, value));
        // The list behind this panel shows the period in its title.
        refresh.run();
    });
    row
}

/// A proportion, shown as the whole percent a person reads.
///
/// The range arrives as the fractions the thing is really stored in, and
/// 0–100 is only how it is shown. Callers that did the arithmetic themselves
/// are how one of these ends up off by a hundred with nobody noticing.
///
/// Whole percent. Nothing on the chart shows a tenth of one, and a slider that
/// stops between figures is a slider the box then disagrees with.
#[allow(clippy::too_many_arguments)]
fn percent_row(
    owner: &Rc<Owner>,
    refresh: &Refresh,
    id: u32,
    title: &str,
    share: f64,
    least: f64,
    most: f64,
    apply: impl Fn(&mut Indicator, f64) + 'static,
) -> adw::ActionRow {
    let owner = owner.clone();
    let refresh = refresh.clone();
    controls::bounded_row(
        title,
        None,
        as_percent(share),
        as_percent(least),
        as_percent(most),
        1.0,
        0,
        move |percent| {
            let share = as_share(percent);
            update(&owner, id, |indicator| apply(indicator, share));
            refresh.run();
        },
    )
    .row
}

/// A stored fraction as the whole percent a person reads, and back.
///
/// One place does this arithmetic, in both directions. A caller doing its own
/// is how a row ends up off by a hundred with nobody noticing until a chart
/// looks wrong.
fn as_percent(share: f64) -> f64 {
    share * 100.0
}

/// The percent on the row as the fraction the indicator is stored in.
fn as_share(percent: f64) -> f64 {
    percent / 100.0
}

/// An oscillator's overbought and oversold levels, the pair RSI and the
/// stochastic both draw across their strips.
///
/// Each runs the whole 0–100 of the strip, because where the lines go is a
/// matter of taste — 80/20 on a stochastic, 70/30 on RSI, tighter on a quiet
/// instrument — and a row that stopped at 50 would be deciding that for you.
/// What the pair may not do is cross: a band whose floor is above its ceiling
/// is not a band. So moving one past the other pushes the other along, a step
/// ahead, rather than refusing the move — the figure you asked for is the one
/// you get, and the one you did not ask about is the one that gives way.
/// Overbought stops a step short of 0 and oversold a step short of 100, which
/// is what keeps there always being somewhere for the other to go.
fn level_rows(
    owner: &Rc<Owner>,
    refresh: &Refresh,
    id: u32,
    overbought: f64,
    oversold: f64,
) -> [adw::ActionRow; 2] {
    // Each row's callback needs the other row, which does not exist yet when
    // the first is built; the cells are filled once both do.
    let under: Rc<OnceCell<controls::WeakBounded>> = Rc::new(OnceCell::new());
    let over: Rc<OnceCell<controls::WeakBounded>> = Rc::new(OnceCell::new());

    let owner_for_over = owner.clone();
    let refresh_for_over = refresh.clone();
    let under_for_over = under.clone();
    let overbought = controls::bounded_row(
        "Overbought",
        None,
        overbought,
        LEVEL_STEP,
        100.0,
        LEVEL_STEP,
        0,
        move |level| {
            update(&owner_for_over, id, |indicator| {
                if let Params::Rsi { overbought, .. } | Params::Stochastic { overbought, .. } =
                    &mut indicator.params
                {
                    *overbought = level;
                }
            });
            if let Some(under) = under_for_over.get()
                && under.value().is_some_and(|floor| floor >= level)
            {
                under.set(level - LEVEL_STEP);
            }
            refresh_for_over.run();
        },
    );

    let owner_for_under = owner.clone();
    let refresh_for_under = refresh.clone();
    let over_for_under = over.clone();
    let oversold = controls::bounded_row(
        "Oversold",
        None,
        oversold,
        0.0,
        100.0 - LEVEL_STEP,
        LEVEL_STEP,
        0,
        move |level| {
            update(&owner_for_under, id, |indicator| {
                if let Params::Rsi { oversold, .. } | Params::Stochastic { oversold, .. } =
                    &mut indicator.params
                {
                    *oversold = level;
                }
            });
            if let Some(over) = over_for_under.get()
                && over.value().is_some_and(|ceiling| ceiling <= level)
            {
                over.set(level + LEVEL_STEP);
            }
            refresh_for_under.run();
        },
    );

    // Filled once both exist. Neither can already be set: this is the only
    // place that sets them.
    let _ = under.set(oversold.downgrade());
    let _ = over.set(overbought.downgrade());

    [overbought.row, oversold.row]
}

/// Whole levels, and the least a pair of them can be apart.
const LEVEL_STEP: f64 = 1.0;

/// A MACD's two averages, which have to stay in that order.
///
/// A fast average no faster than the slow one does not measure convergence at
/// all — at equal lengths there is nothing to measure, and past it the line is
/// the one you wanted upside down. So each row pushes the other out of its way
/// rather than refusing the drag, which is what the overbought and oversold
/// pair above do and for the same reason: a slider that stops dead reads as
/// broken, and one that quietly makes room says what the rule is.
fn macd_length_rows(
    owner: &Rc<Owner>,
    refresh: &Refresh,
    id: u32,
    fast: f64,
    slow: f64,
) -> [adw::ActionRow; 2] {
    // Each row's callback needs the other row, which does not exist yet when
    // the first is built; the cells are filled once both do.
    let quick: Rc<OnceCell<controls::WeakBounded>> = Rc::new(OnceCell::new());
    let slower: Rc<OnceCell<controls::WeakBounded>> = Rc::new(OnceCell::new());

    let owner_for_fast = owner.clone();
    let refresh_for_fast = refresh.clone();
    let slower_for_fast = slower.clone();
    let fast = controls::bounded_row(
        "Fast length",
        None,
        fast,
        1.0,
        MAX_MACD_LENGTH - 1.0,
        1.0,
        0,
        move |length| {
            update(&owner_for_fast, id, |indicator| {
                if let Params::Macd { fast, .. } = &mut indicator.params {
                    *fast = length as usize;
                }
            });
            if let Some(slower) = slower_for_fast.get()
                && slower.value().is_some_and(|slow| slow <= length)
            {
                slower.set(length + 1.0);
            }
            refresh_for_fast.run();
        },
    );

    let owner_for_slow = owner.clone();
    let refresh_for_slow = refresh.clone();
    let quick_for_slow = quick.clone();
    let slow = controls::bounded_row(
        "Slow length",
        None,
        slow,
        2.0,
        MAX_MACD_LENGTH,
        1.0,
        0,
        move |length| {
            update(&owner_for_slow, id, |indicator| {
                if let Params::Macd { slow, .. } = &mut indicator.params {
                    *slow = length as usize;
                }
            });
            if let Some(quick) = quick_for_slow.get()
                && quick.value().is_some_and(|fast| fast >= length)
            {
                quick.set(length - 1.0);
            }
            refresh_for_slow.run();
        },
    );

    // Filled once both exist. Neither can already be set: this is the only
    // place that sets them.
    let _ = slower.set(slow.downgrade());
    let _ = quick.set(fast.downgrade());

    [fast.row, slow.row]
}

/// The longest either of a MACD's averages may be set to from the window.
const MAX_MACD_LENGTH: f64 = 200.0;

/// How much of the chart an indicator's own strip takes.
///
/// One row for every indicator that has a strip, rather than one per kind: the
/// question is the same whichever of them is asking it.
///
/// The ends come from the engine's own clamp rather than from a pair of
/// numbers typed in here, which is the only way the slider and the chart agree
/// about what the extremes are.
fn pane_height_row(
    owner: &Rc<Owner>,
    refresh: &Refresh,
    id: u32,
    height: f64,
) -> adw::ActionRow {
    percent_row(
        owner,
        refresh,
        id,
        "Pane height %",
        height,
        MIN_PANE_SHARE,
        MAX_PANE_SHARE,
        move |indicator, share| match &mut indicator.params {
            Params::Volume { height, .. }
            | Params::Rsi { height, .. }
            | Params::Macd { height, .. }
            | Params::Atr { height, .. }
            | Params::Stochastic { height, .. } => *height = share,
            _ => {}
        },
    )
}

/// How a volume profile is divided up: automatically, or by a number.
///
/// Two rows rather than one control, because they answer different questions.
/// The switch decides whether the instrument's own price increment sets the
/// row height — a nickel on a share, half a pip on a currency major — and the
/// spin is for the rare chart where you want a specific count instead. While
/// the switch is on the spin still shows the count being drawn, so automatic
/// is something you can see rather than take on faith.
fn rows_rows(
    owner: &Rc<Owner>,
    refresh: &Refresh,
    id: u32,
    rows: Option<usize>,
) -> Vec<adw::PreferencesRow> {
    let drawn = owner.profile_rows(id);

    let spin = adw::SpinRow::with_range(4.0, 400.0, 1.0);
    spin.set_title("Rows");
    spin.set_value(rows.or(drawn).unwrap_or(48) as f64);
    spin.set_sensitive(rows.is_some());

    let automatic = adw::ActionRow::new();
    automatic.set_title("Automatic");
    automatic.set_subtitle("Rows as tall as the instrument's own price steps");
    let switch = gtk::Switch::new();
    switch.set_active(rows.is_none());
    switch.set_valign(gtk::Align::Center);
    automatic.add_suffix(&switch);
    automatic.set_activatable_widget(Some(&switch));

    let owner_for_switch = owner.clone();
    let refresh_for_switch = refresh.clone();
    let spin_weak = spin.downgrade();
    switch.connect_active_notify(move |switch| {
        let on = switch.is_active();
        // Turning it off hands over the count that was on screen a moment ago,
        // so taking control does not also change the chart.
        let taken = spin_weak.upgrade().map(|spin| spin.value() as usize).unwrap_or(48);
        update(&owner_for_switch, id, move |indicator| {
            if let Params::VolumeProfile { rows, .. } = &mut indicator.params {
                *rows = if on { None } else { Some(taken) };
            }
        });
        if let Some(spin) = spin_weak.upgrade() {
            spin.set_sensitive(!on);
            if on && let Some(drawn) = owner_for_switch.profile_rows(id) {
                spin.set_value(drawn as f64);
            }
        }
        refresh_for_switch.run();
    });

    let owner_for_spin = owner.clone();
    let refresh_for_spin = refresh.clone();
    spin.connect_value_notify(move |spin| {
        let value = spin.value() as usize;
        update(&owner_for_spin, id, move |indicator| {
            if let Params::VolumeProfile { rows, .. } = &mut indicator.params {
                *rows = Some(value);
            }
        });
        refresh_for_spin.run();
    });

    vec![automatic.upcast(), spin.upcast()]
}

/// The colour this indicator is actually drawn in.
///
/// Asked of the whole set rather than of the indicator, because that is where
/// the answer lives now: an automatic colour is the first one its siblings have
/// not taken, so a second moving average is a different colour from the first.
fn drawn_colour(owner: &Rc<Owner>, indicators: &[Indicator], id: u32) -> String {
    let colours = omacharts_engine::palette_colors(indicators, &owner.theme());
    indicators
        .iter()
        .position(|i| i.id == id)
        .and_then(|at| colours.get(at).cloned())
        .unwrap_or_else(|| owner.theme().ui.accent.clone())
}

/// Change one indicator in place and redraw.
fn update(owner: &Rc<Owner>, id: u32, change: impl Fn(&mut Indicator)) {
    let mut indicators = owner.indicators();
    let Some(indicator) = indicators.iter_mut().find(|i| i.id == id) else { return };
    change(indicator);
    owner.set_indicators(indicators);
}

/// Pick the kind of indicator to add.
///
/// A dialog rather than a popover hanging off whatever was clicked. It is
/// reached from a button in the settings and from a key on the chart, and the
/// key has nothing to point at — a picker that lands in the top-left corner
/// when summoned by Ctrl+Shift+I and under a button otherwise is two features
/// wearing one name. The symbol search, which has exactly the same two ways
/// in, has always been a dialog for the same reason.
pub fn add_indicator(window: &Rc<Window>) {
    pick_indicator(&Owner::chart(window), &Refresh::none());
}

fn pick_indicator(owner: &Rc<Owner>, refresh: &Refresh) {
    let entry = gtk::SearchEntry::new();
    entry.set_placeholder_text(Some("Indicator"));

    let rows = gtk::ListBox::new();
    rows.set_selection_mode(gtk::SelectionMode::Browse);
    rows.add_css_class("navigation-sidebar");

    let scroller = gtk::ScrolledWindow::new();
    scroller.set_child(Some(&rows));
    scroller.set_vexpand(true);
    scroller.set_hscrollbar_policy(gtk::PolicyType::Never);

    let header = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    header.set_margin_top(10);
    header.set_margin_bottom(10);
    header.set_margin_start(12);
    header.set_margin_end(12);
    header.append(&entry);

    let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
    content.append(&header);
    content.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
    content.append(&scroller);

    let dialog = adw::Dialog::new();
    dialog.set_title("Add an indicator");
    dialog.set_content_width(420);
    // Tall enough for every family under its heading without scrolling.
    dialog.set_content_height(620);
    dialog.set_child(Some(&content));

    let shown: Rc<RefCell<Vec<Kind>>> = Rc::new(RefCell::new(Vec::new()));
    // Grouped under their families while nothing is typed, which is when the
    // list is being browsed; once something is, it is a ranked answer, and
    // headings would scatter the best match away from the top.
    let grouped = Rc::new(std::cell::Cell::new(true));
    {
        let shown = shown.clone();
        let grouped = grouped.clone();
        rows.set_header_func(move |row, before| {
            if !grouped.get() {
                row.set_header(None::<&gtk::Widget>);
                return;
            }
            let kinds = shown.borrow();
            let category = |row: &gtk::ListBoxRow| {
                kinds.get(row.index().max(0) as usize).map(|kind| kind.category())
            };
            let this = category(row);
            if this.is_none() || before.and_then(category) == this {
                row.set_header(None::<&gtk::Widget>);
                return;
            }
            let heading = gtk::Label::new(this.map(|c| c.label()));
            heading.set_xalign(0.0);
            heading.add_css_class("heading");
            heading.add_css_class("dim-label");
            heading.set_margin_start(10);
            heading.set_margin_top(if before.is_some() { 14 } else { 6 });
            heading.set_margin_bottom(4);
            row.set_header(Some(&heading));
        });
    }

    let fill = {
        let rows = rows.clone();
        let shown = shown.clone();
        let grouped = grouped.clone();
        move |query: &str| {
            while let Some(child) = rows.first_child() {
                rows.remove(&child);
            }
            let browsing = query.trim().is_empty();
            grouped.set(browsing);
            let kinds = match browsing {
                true => indicators::Category::ALL.iter().flat_map(|c| c.kinds()).collect(),
                false => indicators::search(query),
            };
            // Filled before the rows, which the header function reads.
            *shown.borrow_mut() = kinds.clone();
            for kind in &kinds {
                let name = gtk::Label::new(Some(kind.name()));
                name.set_xalign(0.0);
                name.set_hexpand(true);

                let row_box = gtk::Box::new(gtk::Orientation::Horizontal, 10);
                row_box.set_margin_top(7);
                row_box.set_margin_bottom(7);
                row_box.set_margin_start(10);
                row_box.set_margin_end(10);
                row_box.append(&name);
                // The chip is the short name the chart will show; where that
                // is the name itself, it would be saying it twice.
                if !kind.short_name().eq_ignore_ascii_case(kind.name()) {
                    let short = gtk::Label::new(Some(kind.short_name()));
                    short.add_css_class("symbol-kind");
                    short.set_valign(gtk::Align::Center);
                    row_box.append(&short);
                }

                let row = gtk::ListBoxRow::new();
                row.set_child(Some(&row_box));
                rows.append(&row);
            }
            rows.invalidate_headers();
            if let Some(first) = rows.row_at_index(0) {
                rows.select_row(Some(&first));
            }
        }
    };
    fill("");

    let fill_on_type = fill.clone();
    entry.connect_search_changed(move |entry| fill_on_type(&entry.text()));

    let rows_for_enter = rows.clone();
    entry.connect_activate(move |_| {
        if let Some(row) = rows_for_enter.selected_row() {
            row.activate();
        }
    });

    // Up and down move the selection while focus stays in the entry, so one
    // hand never has to leave the keyboard.
    let keys = gtk::EventControllerKey::new();
    let rows_for_keys = rows.clone();
    keys.connect_key_pressed(move |_, key, _, _| {
        let delta = match key {
            gtk::gdk::Key::Down => 1,
            gtk::gdk::Key::Up => -1,
            _ => return glib::Propagation::Proceed,
        };
        let at = rows_for_keys.selected_row().map(|r| r.index()).unwrap_or(0);
        if let Some(next) = rows_for_keys.row_at_index(at + delta) {
            rows_for_keys.select_row(Some(&next));
        }
        glib::Propagation::Stop
    });
    entry.add_controller(keys);

    let owner_for_pick = owner.clone();
    let refresh_for_pick = refresh.clone();
    let dialog_for_pick = dialog.clone();
    rows.connect_row_activated(move |_, row| {
        let at = row.index().max(0) as usize;
        let Some(kind) = shown.borrow().get(at).copied() else { return };
        let id = owner_for_pick.next_indicator_id();
        // On a template it goes onto the template, like every other change
        // made here, so there is no template to go back onto if it is not
        // added after all: it simply comes off again.
        let was_on = None;
        let mut indicators = owner_for_pick.indicators();
        indicators.push(Indicator::new(id, kind));
        owner_for_pick.set_indicators(indicators);
        refresh_for_pick.run();
        dialog_for_pick.close();
        // Straight into its settings: picking the kind and setting it up are
        // two steps, and the second is where you say whether you meant it.
        open_indicator_panel_for(&owner_for_pick, &refresh_for_pick, id, Panel::Add(was_on));
    });

    // What Enter does in the box, from anywhere in the dialog — including the
    // list, where Enter is already the row's own key and means the same thing.
    let rows_for_commit = rows.clone();
    dialogs::commit_on_ctrl_enter(&dialog, move || {
        if let Some(row) = rows_for_commit.selected_row() {
            row.activate();
        }
    });

    // Only the box needs saying. Escape on anything else inside the dialog
    // already closes it, which is why there is no controller for it.
    dialogs::close_on_search_escape(&entry, &dialog);
    dialogs::close_on_click_outside(&dialog);

    dialog.present(Some(&owner.window));
    entry.grab_focus();
}

use gtk::glib;

#[cfg(test)]
mod tests {
    use super::*;

    // GTK objects are not built in here. Starting GTK is something one test in
    // the suite may do, from its own thread, and a second one asking would
    // take the whole run down with it — so these ask the arithmetic the rows
    // are built from instead.

    #[test]
    fn a_stored_fraction_is_shown_as_whole_percent() {
        assert_eq!(as_percent(0.16), 16.0, "a sixth of the chart reads as 16%");
        assert_eq!(as_percent(0.0), 0.0, "clear shading reads as 0%");
        assert_eq!(as_percent(1.0), 100.0, "and solid shading as 100%");
    }

    /// The row reads one way and writes the other, so a figure that survives
    /// the trip is the only thing standing between a pane height and a value
    /// a hundred times the one that was chosen.
    #[test]
    fn a_percentage_comes_back_as_the_fraction_it_was_shown_from() {
        for share in [0.0, MIN_PANE_SHARE, 0.16, 0.5, MAX_PANE_SHARE, 1.0] {
            assert_eq!(as_share(as_percent(share)), share);
        }
    }

    /// A slider that moves in whole percent can only land on the extremes if
    /// the extremes are whole percentages. Were the engine to clamp a pane to
    /// 0.055 of the chart, the least it allows would be a figure no slider in
    /// here could reach and no box could be typed.
    #[test]
    fn the_ends_of_a_pane_are_figures_a_whole_percent_row_can_reach() {
        for end in [MIN_PANE_SHARE, MAX_PANE_SHARE] {
            let shown = as_percent(end);
            assert_eq!(shown, shown.round(), "{end} is not a whole percentage");
        }
    }
}
