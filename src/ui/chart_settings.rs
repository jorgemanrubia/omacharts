//! Settings that belong to the chart rather than the app.
//!
//! Reached from the gear beside the legend, because that is where you are
//! looking when you want them. Two pages: how the bars are read and scaled,
//! and what is drawn on top of them.

use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;
use omacharts_engine::indicators::{
    self, Kind, LineStyle, Params, Reset, Stroke, MAX_PANE_SHARE, MIN_PANE_SHARE,
};
use omacharts_engine::theme::ColorChoice;
use omacharts_engine::{BarStyle, Indicator, Session};

use crate::store::Store;
use crate::ui::colors;
use crate::ui::dialogs;
use crate::ui::shortcuts;
use crate::ui::window::Window;

pub const SETTING_SESSION: &str = "chart_session";

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
        dialog.set_content_height(620);

        let chart_page = adw::PreferencesPage::new();
        chart_page.set_title("Chart");
        chart_page.set_icon_name(Some("preferences-system-symbolic"));
        chart_page.add(&bars_group(window, &store));
        chart_page.add(&scales_group(window));
        chart_page.add(&session_group(window, &store));
        dialog.add(&chart_page);

        let indicators_page = adw::PreferencesPage::new();
        indicators_page.set_title("Indicators");
        indicators_page.set_icon_name(Some("view-list-symbolic"));
        dialog.add(&indicators_page);
        let list = IndicatorList::new(&indicators_page);
        // Wired once here rather than on every rebuild: the button outlives
        // the rows, and reconnecting it each time would stack up handlers.
        let window_for_add = window.clone();
        let refresh_for_add = Refresh::rebuilding(window, &dialog, &list);
        list.add.connect_clicked(move |_| {
            pick_indicator(&window_for_add, &refresh_for_add);
        });
        rebuild_indicators(window, &dialog, &list);

        dialog.present(Some(&window.window));
        match focus {
            Focus::Chart => {}
            Focus::Indicators => dialog.set_visible_page(&indicators_page),
            Focus::Indicator(id) => {
                dialog.set_visible_page(&indicators_page);
                open_indicator_panel_for(
                    window,
                    &Refresh::rebuilding(window, &dialog, &list),
                    id,
                    Panel::Subpage(dialog.clone()),
                );
            }
        }
    }


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

    let window_for_style = window.clone();
    row.connect_selected_notify(move |row| {
        if let Some(style) = BarStyle::ALL.get(row.selected() as usize).copied() {
            window_for_style.set_bar_style(style);
        }
    });
    group.add(&row);

    // One switch for the window rather than one per chart: it is about how the
    // charts behave towards each other, which is not a property of any one of
    // them.
    let sync = adw::ActionRow::new();
    sync.set_title("Crosshair across linked charts");
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

fn session_group(window: &Rc<Window>, store: &Rc<Store>) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::new();
    group.set_title("Session");
    group.set_description(Some(
        "Overnight trade is thin, and a few prints at 3am stretch the price scale \
         enough to squash the session everyone actually traded.",
    ));

    let names: Vec<&str> = Session::ALL.iter().map(|s| s.label()).collect();
    let session = window.session();

    let row = adw::ComboRow::new();
    row.set_title("Hours");
    row.set_subtitle("Ignored for FX, crypto and foreign listings, which have no cash session.");
    row.set_model(Some(&gtk::StringList::new(&names)));
    row.set_selected(Session::ALL.iter().position(|s| *s == session).unwrap_or(0) as u32);

    let window = window.clone();
    let store = store.clone();
    row.connect_selected_notify(move |row| {
        let Some(chosen) = Session::ALL.get(row.selected() as usize).copied() else { return };
        window.set_session(chosen);
        store.set_setting(SETTING_SESSION, chosen.key());
        window.refresh();
    });

    group.add(&row);
    group
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
    fn new(page: &adw::PreferencesPage) -> IndicatorList {
        let group = adw::PreferencesGroup::new();
        group.set_title("On the chart");
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
        window: &Rc<Window>,
        dialog: &adw::PreferencesDialog,
        list: &IndicatorList,
    ) -> Refresh {
        let window = window.clone();
        let dialog = dialog.clone();
        let list = list.clone();
        Refresh(Rc::new(move || rebuild_indicators(&window, &dialog, &list)))
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
    window: &Rc<Window>,
    dialog: &adw::PreferencesDialog,
    list: &IndicatorList,
) {
    list.clear();
    let indicators = window.indicators();
    let refresh = Refresh::rebuilding(window, dialog, list);

    list.group.set_description(Some(if indicators.is_empty() {
        "Nothing yet."
    } else {
        "Drag to reorder. The list is the chart, top to bottom; colours come from the theme."
    }));

    // The price plot is a row of the list because strips can be stacked above it
    // as well as below, and a list showing only the strips could not say which
    // side of the price each one is on. A chart with no strips has no stack to
    // describe, so it is left out there.
    let stacked = indicators.iter().any(|i| i.kind.in_own_pane());
    let price_at = if stacked { price_row_at(&indicators) } else { usize::MAX };
    for (at, indicator) in indicators.iter().enumerate() {
        if at == price_at {
            list.push(&price_row(window, &refresh));
        }
        list.push(&indicator_row(window, dialog, &refresh, &indicators, indicator));
    }
    if price_at == indicators.len() {
        list.push(&price_row(window, &refresh));
    }

    if indicators.is_empty() {
        let empty = adw::ActionRow::new();
        empty.set_title("Add an indicator");
        empty.set_subtitle("Moving averages, VWAP, volume profile");
        empty.set_activatable(true);
        let window_for_empty = window.clone();
        let refresh_for_empty = refresh.clone();
        empty.connect_activated(move |_| {
            pick_indicator(&window_for_empty, &refresh_for_empty);
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
fn price_row(window: &Rc<Window>, refresh: &Refresh) -> adw::ActionRow {
    let row = adw::ActionRow::new();
    row.set_title("Price");
    row.set_subtitle("Candles, and everything drawn over them");
    // No handle and no dot. Every other row wears both, so their absence is
    // what says this row is the chart itself rather than another indicator —
    // and a mark here would read as the handle it is not.
    row.add_css_class("dim-label");

    let target = gtk::DropTarget::new(glib::Type::STRING, gtk::gdk::DragAction::MOVE);
    let window = window.clone();
    let refresh = refresh.clone();
    target.connect_drop(move |_, value, _, _| {
        let Ok(moving) = value.get::<String>().unwrap_or_default().parse::<u32>() else {
            return false;
        };
        let mut all = window.indicators();
        // Dropped on the price: the strip changes sides, which is the one thing
        // dropping it on another row cannot say.
        if !indicators::cross_price(&mut all, moving) {
            return false;
        }
        window.set_indicators(all);
        refresh.run();
        true
    });
    row.add_controller(target);
    row
}

/// One line in the list: what it is, whether it is drawn, and a way in.
fn indicator_row(
    window: &Rc<Window>,
    dialog: &adw::PreferencesDialog,
    refresh: &Refresh,
    all: &[Indicator],
    indicator: &Indicator,
) -> adw::ActionRow {
    let id = indicator.id;
    let colour = drawn_colour(window, all, id);
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
    wire_reorder(window, refresh, &row, id);

    let visible = gtk::Switch::new();
    visible.set_active(indicator.visible);
    visible.set_valign(gtk::Align::Center);
    visible.set_tooltip_text(Some("Show on the chart"));
    let window_for_visible = window.clone();
    visible.connect_state_set(move |_, state| {
        update(&window_for_visible, id, |i| i.visible = state);
        glib::Propagation::Proceed
    });
    row.add_suffix(&visible);

    let remove = gtk::Button::from_icon_name("user-trash-symbolic");
    remove.add_css_class("flat");
    remove.set_valign(gtk::Align::Center);
    remove.set_tooltip_text(Some("Remove"));
    let window_for_remove = window.clone();
    let refresh_for_remove = refresh.clone();
    remove.connect_clicked(move |_| {
        let kept: Vec<Indicator> =
            window_for_remove.indicators().into_iter().filter(|i| i.id != id).collect();
        window_for_remove.set_indicators(kept);
        refresh_for_remove.run();
    });
    row.add_suffix(&remove);

    let window_for_open = window.clone();
    let refresh_for_open = refresh.clone();
    let dialog_for_open = dialog.clone();
    row.connect_activated(move |_| {
        open_indicator_panel_for(
            &window_for_open,
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
fn wire_reorder(window: &Rc<Window>, refresh: &Refresh, row: &adw::ActionRow, id: u32) {
    let source = gtk::DragSource::new();
    source.set_actions(gtk::gdk::DragAction::MOVE);
    let payload = id.to_string();
    source.connect_prepare(move |_, _, _| {
        Some(gtk::gdk::ContentProvider::for_value(&payload.to_value()))
    });
    row.add_controller(source);

    let target = gtk::DropTarget::new(glib::Type::STRING, gtk::gdk::DragAction::MOVE);
    let window = window.clone();
    let refresh = refresh.clone();
    target.connect_drop(move |_, value, _, _| {
        let Ok(moving) = value.get::<String>().unwrap_or_default().parse::<u32>() else {
            return false;
        };
        if moving == id {
            return false;
        }
        let mut indicators = window.indicators();
        let (Some(from), Some(to)) = (
            indicators.iter().position(|i| i.id == moving),
            indicators.iter().position(|i| i.id == id),
        ) else {
            return false;
        };
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
        window.set_indicators(indicators);
        refresh.run();
        true
    });
    row.add_controller(target);
}

/// Where an indicator's settings are shown.
enum Panel {
    /// Pushed over the list it was opened from, with a Done button.
    Subpage(adw::PreferencesDialog),
    /// A dialog of its own, with one button that adds the indicator. Used by
    /// the add path, which can start from the chart with no settings window
    /// open at all, so there is nothing to push a page onto.
    Add,
}

/// One indicator's settings.
///
/// Changes apply as you make them, on the chart, whichever way this was
/// opened: you are looking at the thing you are configuring. For the add path
/// that means the indicator is already drawn while you set it up, and
/// dismissing the dialog takes it back off again — the Add button is how you
/// say you meant it.
fn open_indicator_panel_for(window: &Rc<Window>, refresh: &Refresh, id: u32, panel: Panel) {
    let indicators = window.indicators();
    let Some(indicator) = indicators.iter().find(|i| i.id == id).cloned() else {
        return;
    };
    let colour = drawn_colour(window, &indicators, id);

    let page = adw::PreferencesPage::new();
    page.add(&parameters_group(window, refresh, &indicator));
    if let Some(group) = appearance_group(window, refresh, &colour, &indicator) {
        page.add(&group);
    }
    for group in band_groups(window, refresh, &colour, &indicator) {
        page.add(&group);
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
        Panel::Add => {
            let modal = adw::Dialog::new();
            modal.set_title(&indicator.label());
            modal.set_content_width(560);
            modal.set_content_height(620);
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
            let window_for_close = window.clone();
            let refresh_for_close = refresh.clone();
            modal.connect_closed(move |_| {
                if added.get() {
                    return;
                }
                let kept: Vec<Indicator> =
                    window_for_close.indicators().into_iter().filter(|i| i.id != id).collect();
                window_for_close.set_indicators(kept);
                refresh_for_close.run();
            });

            modal.present(Some(&window.window));
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
    window: &Rc<Window>,
    refresh: &Refresh,
    colour: &str,
    indicator: &Indicator,
) -> Option<adw::PreferencesGroup> {
    let id = indicator.id;

    // Volume is drawn in the bar scheme's own up and down colours, the same
    // ones the candles use, so there is nothing here to ask about: a colour,
    // a thickness and a pattern that changed nothing would be three lies.
    if indicator.kind == Kind::Volume {
        return None;
    }

    // A volume profile is bars and a mark, not a line, so it is asked about
    // its two colours and nothing about thickness or pattern.
    if indicator.kind == Kind::VolumeProfile {
        return Some(profile_colours(window, refresh, colour, indicator));
    }

    let group = adw::PreferencesGroup::new();
    group.set_title("Line");

    let ink = {
        let window = window.clone();
        Ink::new(move || drawn_colour(&window, &window.indicators(), id))
    };

    group.add(&colour_row(
        window,
        refresh,
        &ink,
        "Colour",
        colour,
        indicator.color.clone(),
        move |indicator, choice| indicator.color = choice,
        id,
    ));
    for row in stroke_rows(window, refresh, &ink, id, "", indicator.stroke, {
        move |indicator: &mut Indicator, stroke: Stroke| indicator.stroke = stroke
    }) {
        group.add(&row);
    }
    Some(group)
}

/// Width and style, as one expander-free pair of rows.
///
/// Returns the group they belong to so callers can drop it straight in.
fn stroke_rows(
    window: &Rc<Window>,
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
        window,
        refresh,
        ink,
        id,
        &format!("{prefix}Thickness"),
        &WIDTHS.map(|width| (width, Stroke { width, style: stroke.style })),
        stroke,
        move |indicator, picked| apply_width(indicator, picked),
    ));

    rows.push(picker_row(
        window,
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
    window: &Rc<Window>,
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

        let window_for_pick = window.clone();
        let refresh_for_pick = refresh.clone();
        let apply_for_pick = apply.clone();
        let picked = *stroke;
        button.connect_toggled(move |button| {
            if !button.is_active() {
                return;
            }
            let apply = apply_for_pick.clone();
            update(&window_for_pick, id, move |indicator| apply(indicator, picked));
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
    window: &Rc<Window>,
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
    let window_for_colour = window.clone();
    let refresh_for_colour = refresh.clone();
    let ink_for_colour = ink.clone();
    let apply_for_colour = apply.clone();
    let default_weak = default.downgrade();
    let button = crate::ui::palette::picker(
        &window.theme(),
        choice,
        current,
        move |picked| {
            let apply = apply_for_colour.clone();
            update(&window_for_colour, id, move |indicator| {
                apply(indicator, Some(picked.clone()))
            });
            if let Some(default) = default_weak.upgrade() {
                default.set_sensitive(true);
            }
            ink_for_colour.repaint();
            refresh_for_colour.run();
        },
    );

    let window_for_default = window.clone();
    let refresh_for_default = refresh.clone();
    let ink_for_default = ink.clone();
    let button_weak = button.downgrade();
    default.connect_clicked(move |default| {
        let apply = apply.clone();
        update(&window_for_default, id, move |indicator| apply(indicator, None));
        default.set_sensitive(false);
        // Show what it reverted to. The default is not a fixed colour — it is
        // whichever of the palette this indicator's siblings have left free —
        // so it has to be asked for after the change, not guessed before it.
        if let Some(button) = button_weak.upgrade() {
            let indicators = window_for_default.indicators();
            crate::ui::palette::show(&button, &drawn_colour(&window_for_default, &indicators, id));
        }
        ink_for_default.repaint();
        refresh_for_default.run();
    });

    row.add_suffix(&default);
    row.add_suffix(&button);
    row
}

fn parameters_group(
    window: &Rc<Window>,
    refresh: &Refresh,
    indicator: &Indicator,
) -> adw::PreferencesGroup {
    let id = indicator.id;
    let group = adw::PreferencesGroup::new();
    group.set_title("Parameters");

    match &indicator.params {
        Params::MovingAverage { period } => {
            group.add(&spin_row(
                window,
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
            group.add(&reset_row(window, refresh, id, *reset));
        }
        Params::Volume { height } => {
            group.add(&pane_height_row(window, refresh, id, *height));
        }
        Params::Rsi { period, height, overbought, oversold } => {
            group.add(&spin_row(
                window, refresh, id, "Period", *period as f64, 2.0, 200.0, 1.0,
                move |indicator, value| {
                    if let Params::Rsi { period, .. } = &mut indicator.params {
                        *period = value as usize;
                    }
                },
            ));
            group.add(&spin_row(
                window, refresh, id, "Overbought", *overbought, 50.0, 100.0, 1.0,
                move |indicator, value| {
                    if let Params::Rsi { overbought, .. } = &mut indicator.params {
                        *overbought = value;
                    }
                },
            ));
            group.add(&spin_row(
                window, refresh, id, "Oversold", *oversold, 0.0, 50.0, 1.0,
                move |indicator, value| {
                    if let Params::Rsi { oversold, .. } = &mut indicator.params {
                        *oversold = value;
                    }
                },
            ));
            group.add(&pane_height_row(window, refresh, id, *height));
        }
        Params::Atr { period, height } => {
            group.add(&spin_row(
                window, refresh, id, "Period", *period as f64, 2.0, 200.0, 1.0,
                move |indicator, value| {
                    if let Params::Atr { period, .. } = &mut indicator.params {
                        *period = value as usize;
                    }
                },
            ));
            group.add(&pane_height_row(window, refresh, id, *height));
        }
        Params::VolumeProfile { reset, rows, value_area, .. } => {
            group.add(&reset_row(window, refresh, id, *reset));
            for row in rows_rows(window, refresh, id, *rows) {
                group.add(&row);
            }
            group.add(
                &percent_row(
                    window,
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
    }
    group
}

/// The two colours a volume profile actually has.
fn profile_colours(
    window: &Rc<Window>,
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
        window,
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
    let shown = poc_color
        .as_ref()
        .map(|choice| choice.resolve(&window.theme()))
        .unwrap_or_else(|| window.theme().companion(colour));

    group.add(&colour_row(
        window,
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
    window: &Rc<Window>,
    refresh: &Refresh,
    line_colour: &str,
    indicator: &Indicator,
) -> Vec<adw::PreferencesGroup> {
    let Params::Vwap { bands, .. } = &indicator.params else { return Vec::new() };
    let id = indicator.id;
    let line_colour = line_colour.to_string();
    let mut groups = Vec::new();

    for (index, band) in bands.iter().enumerate() {
        let group = adw::PreferencesGroup::new();
        group.set_title(&format!("Band {}", index + 1));

        let show = adw::ActionRow::new();
        show.set_title("Show");
        let switch = gtk::Switch::new();
        switch.set_active(band.enabled);
        switch.set_valign(gtk::Align::Center);
        let window_for_show = window.clone();
        let refresh_for_show = refresh.clone();
        switch.connect_state_set(move |_, on| {
            update(&window_for_show, id, move |indicator| {
                if let Params::Vwap { bands, .. } = &mut indicator.params
                    && let Some(band) = bands.get_mut(index)
                {
                    band.enabled = on;
                }
            });
            refresh_for_show.run();
            glib::Propagation::Proceed
        });
        show.add_suffix(&switch);
        group.add(&show);

        let deviations = adw::SpinRow::with_range(0.1, 6.0, 0.1);
        deviations.set_title("Standard deviations");
        deviations.set_digits(1);
        deviations.set_value(band.deviations);
        let window_for_dev = window.clone();
        let refresh_for_dev = refresh.clone();
        deviations.connect_value_notify(move |row| {
            let value = row.value();
            update(&window_for_dev, id, move |indicator| {
                if let Params::Vwap { bands, .. } = &mut indicator.params
                    && let Some(band) = bands.get_mut(index)
                {
                    band.deviations = value;
                }
            });
            refresh_for_dev.run();
        });
        group.add(&deviations);

        let shaded = adw::ActionRow::new();
        shaded.set_title("Shaded");
        shaded.set_subtitle("Fill the area between this band's edges");
        let fill = gtk::Switch::new();
        fill.set_active(band.fill);
        fill.set_valign(gtk::Align::Center);
        let window_for_fill = window.clone();
        let refresh_for_fill = refresh.clone();
        fill.connect_state_set(move |_, on| {
            update(&window_for_fill, id, move |indicator| {
                if let Params::Vwap { bands, .. } = &mut indicator.params
                    && let Some(band) = bands.get_mut(index)
                {
                    band.fill = on;
                }
            });
            refresh_for_fill.run();
            glib::Propagation::Proceed
        });
        shaded.add_suffix(&fill);
        group.add(&shaded);

        // How much of the colour that shading gets, between the ends the
        // engine's own clamp allows a band: never quite invisible, and
        // stopping well short of opaque, because it is a backdrop and the bars
        // have to stay readable through it.
        group.add(
            &percent_row(
                window,
                refresh,
                id,
                "Shading %",
                band.alpha(),
                0.02,
                0.6,
                move |indicator, share| {
                    if let Params::Vwap { bands, .. } = &mut indicator.params
                        && let Some(band) = bands.get_mut(index)
                    {
                        band.fill_alpha = Some(share);
                    }
                },
            ),
        );

        let current = band
            .color
            .as_ref()
            .map(|c| c.resolve(&window.theme()))
            .unwrap_or_else(|| line_colour.clone());

        // A band's samples follow the band's own colour, falling back to the
        // line's when it has none of its own — the same answer the chart draws.
        let ink = {
            let window = window.clone();
            Ink::new(move || {
                let indicators = window.indicators();
                let band_colour = indicators
                    .iter()
                    .find(|i| i.id == id)
                    .and_then(|i| match &i.params {
                        Params::Vwap { bands, .. } => bands.get(index).and_then(|b| b.color.clone()),
                        _ => None,
                    })
                    .map(|choice| choice.resolve(&window.theme()));
                band_colour.unwrap_or_else(|| drawn_colour(&window, &indicators, id))
            })
        };

        group.add(&colour_row(
            window,
            refresh,
            &ink,
            "Colour",
            &current,
            band.color.clone(),
            move |indicator, choice| {
                if let Params::Vwap { bands, .. } = &mut indicator.params
                    && let Some(band) = bands.get_mut(index)
                {
                    band.color = choice;
                }
            },
            id,
        ));

        for row in stroke_rows(window, refresh, &ink, id, "", band.stroke, {
            move |indicator: &mut Indicator, stroke: Stroke| {
                if let Params::Vwap { bands, .. } = &mut indicator.params
                    && let Some(band) = bands.get_mut(index)
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
    window: &Rc<Window>,
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

    let window = window.clone();
    let refresh = refresh.clone();
    row.connect_selected_notify(move |row| {
        let Some(chosen) = Reset::ALL.get(row.selected() as usize).copied() else { return };
        update(&window, id, |indicator| match &mut indicator.params {
            Params::Vwap { reset, .. } => *reset = chosen,
            Params::VolumeProfile { reset, .. } => *reset = chosen,
            Params::MovingAverage { .. }
            | Params::Volume { .. }
            | Params::Rsi { .. }
            | Params::Atr { .. } => {}
        });
        refresh.run();
    });
    row
}

#[allow(clippy::too_many_arguments)]
fn spin_row(
    window: &Rc<Window>,
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

    let window = window.clone();
    let refresh = refresh.clone();
    row.connect_value_notify(move |row| {
        let value = row.value();
        update(&window, id, |indicator| apply(indicator, value));
        // The list behind this panel shows the period in its title.
        refresh.run();
    });
    row
}

/// A proportion: a slider to aim with, and a box to say it in.
///
/// Every percentage in here is chosen by feel — a pane is about a third, a
/// shading is barely there — and a pair of − and + buttons is the wrong shape
/// for that question. The box beside the slider is for the times you already
/// know the figure and would rather type it than aim at it.
///
/// One `GtkAdjustment` drives both, so there is one value rather than two
/// controls that have to be kept in step: dragging moves the number as it
/// goes, and typing moves the handle.
///
/// The range arrives as the fractions the thing is really stored in, and
/// 0–100 is only how it is shown. Callers that did the arithmetic themselves
/// are how one of these ends up off by a hundred with nobody noticing.
#[allow(clippy::too_many_arguments)]
fn percent_row(
    window: &Rc<Window>,
    refresh: &Refresh,
    id: u32,
    title: &str,
    share: f64,
    least: f64,
    most: f64,
    apply: impl Fn(&mut Indicator, f64) + 'static,
) -> adw::ActionRow {
    let row = adw::ActionRow::new();
    row.set_title(title);

    let adjustment = percent_adjustment(share, least, most);

    let slider = gtk::Scale::new(gtk::Orientation::Horizontal, Some(&adjustment));
    slider.set_draw_value(false);
    slider.set_round_digits(0);
    slider.set_size_request(180, -1);
    slider.set_valign(gtk::Align::Center);

    let entry = percent_entry(&adjustment);

    row.add_suffix(&slider);
    row.add_suffix(&entry);

    let window = window.clone();
    let refresh = refresh.clone();
    adjustment.connect_value_changed(move |adjustment| {
        let share = as_share(adjustment.value());
        update(&window, id, |indicator| apply(indicator, share));
        refresh.run();
    });
    row
}

/// The box a percentage can be typed into.
///
/// The figure is committed on Enter or on leaving the box, never per
/// keystroke: the 5 on the way to 50 would otherwise reach the chart and
/// redraw it on the way past. Anything outside the range is pulled back to the
/// nearest end it allows rather than refused — the figure you can have is more
/// use than a complaint about the one you cannot — and anything that is not a
/// number at all leaves the value where it was.
fn percent_entry(adjustment: &gtk::Adjustment) -> gtk::SpinButton {
    let entry = gtk::SpinButton::new(Some(adjustment), 1.0, 0);
    entry.set_valign(gtk::Align::Center);
    entry.set_numeric(true);
    entry.set_snap_to_ticks(true);
    entry.set_update_policy(gtk::SpinButtonUpdatePolicy::IfValid);
    entry.set_width_chars(3);
    entry.set_max_width_chars(3);
    entry
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

/// The one value a percentage row runs on, in whole percent.
///
/// Both controls share it, which is what keeps them from drifting apart, and
/// it is where the range is enforced: a figure past either end is pulled back
/// to the end rather than written through to be clamped later somewhere the
/// person who typed it cannot see.
///
/// Whole percent. Nothing on the chart shows a tenth of one, and a slider that
/// stops between figures is a slider the box then disagrees with.
fn percent_adjustment(share: f64, least: f64, most: f64) -> gtk::Adjustment {
    gtk::Adjustment::new(
        as_percent(share),
        as_percent(least),
        as_percent(most),
        1.0,
        10.0,
        0.0,
    )
}

/// How much of the chart an indicator's own strip takes.
///
/// One row for every indicator that has a strip, rather than one per kind: the
/// question is the same whichever of them is asking it.
///
/// The ends come from the engine's own clamp rather than from a pair of
/// numbers typed in here, which is the only way the slider and the chart agree
/// about what the extremes are.
fn pane_height_row(
    window: &Rc<Window>,
    refresh: &Refresh,
    id: u32,
    height: f64,
) -> adw::ActionRow {
    percent_row(
        window,
        refresh,
        id,
        "Pane height %",
        height,
        MIN_PANE_SHARE,
        MAX_PANE_SHARE,
        move |indicator, share| match &mut indicator.params {
            Params::Volume { height }
            | Params::Rsi { height, .. }
            | Params::Atr { height, .. } => *height = share,
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
    window: &Rc<Window>,
    refresh: &Refresh,
    id: u32,
    rows: Option<usize>,
) -> Vec<adw::PreferencesRow> {
    let drawn = window.profile_rows(id);

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

    let window_for_switch = window.clone();
    let refresh_for_switch = refresh.clone();
    let spin_weak = spin.downgrade();
    switch.connect_active_notify(move |switch| {
        let on = switch.is_active();
        // Turning it off hands over the count that was on screen a moment ago,
        // so taking control does not also change the chart.
        let taken = spin_weak.upgrade().map(|spin| spin.value() as usize).unwrap_or(48);
        update(&window_for_switch, id, move |indicator| {
            if let Params::VolumeProfile { rows, .. } = &mut indicator.params {
                *rows = if on { None } else { Some(taken) };
            }
        });
        if let Some(spin) = spin_weak.upgrade() {
            spin.set_sensitive(!on);
            if on && let Some(drawn) = window_for_switch.profile_rows(id) {
                spin.set_value(drawn as f64);
            }
        }
        refresh_for_switch.run();
    });

    let window_for_spin = window.clone();
    let refresh_for_spin = refresh.clone();
    spin.connect_value_notify(move |spin| {
        let value = spin.value() as usize;
        update(&window_for_spin, id, move |indicator| {
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
fn drawn_colour(window: &Rc<Window>, indicators: &[Indicator], id: u32) -> String {
    let colours = omacharts_engine::palette_colors(indicators, &window.theme());
    indicators
        .iter()
        .position(|i| i.id == id)
        .and_then(|at| colours.get(at).cloned())
        .unwrap_or_else(|| window.theme().ui.accent.clone())
}

/// Change one indicator in place and redraw.
fn update(window: &Rc<Window>, id: u32, change: impl Fn(&mut Indicator)) {
    let mut indicators = window.indicators();
    let Some(indicator) = indicators.iter_mut().find(|i| i.id == id) else { return };
    change(indicator);
    window.set_indicators(indicators);
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
    pick_indicator(window, &Refresh::none());
}

fn pick_indicator(window: &Rc<Window>, refresh: &Refresh) {
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
    dialog.set_content_height(380);
    dialog.set_child(Some(&content));

    let shown: Rc<RefCell<Vec<Kind>>> = Rc::new(RefCell::new(Vec::new()));

    let fill = {
        let rows = rows.clone();
        let shown = shown.clone();
        move |query: &str| {
            while let Some(child) = rows.first_child() {
                rows.remove(&child);
            }
            let kinds = indicators::search(query);
            for kind in &kinds {
                let name = gtk::Label::new(Some(kind.name()));
                name.set_xalign(0.0);
                name.set_hexpand(true);
                let short = gtk::Label::new(Some(kind.short_name()));
                short.add_css_class("symbol-kind");
                short.set_valign(gtk::Align::Center);

                let row_box = gtk::Box::new(gtk::Orientation::Horizontal, 10);
                row_box.set_margin_top(7);
                row_box.set_margin_bottom(7);
                row_box.set_margin_start(10);
                row_box.set_margin_end(10);
                row_box.append(&name);
                row_box.append(&short);

                let row = gtk::ListBoxRow::new();
                row.set_child(Some(&row_box));
                rows.append(&row);
            }
            *shown.borrow_mut() = kinds;
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

    let window_for_pick = window.clone();
    let refresh_for_pick = refresh.clone();
    let dialog_for_pick = dialog.clone();
    rows.connect_row_activated(move |_, row| {
        let at = row.index().max(0) as usize;
        let Some(kind) = shown.borrow().get(at).copied() else { return };
        let id = window_for_pick.next_indicator_id();
        let mut indicators = window_for_pick.indicators();
        indicators.push(Indicator::new(id, kind));
        window_for_pick.set_indicators(indicators);
        refresh_for_pick.run();
        dialog_for_pick.close();
        // Straight into its settings: picking the kind and setting it up are
        // two steps, and the second is where you say whether you meant it.
        open_indicator_panel_for(&window_for_pick, &refresh_for_pick, id, Panel::Add);
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

    dialog.present(Some(&window.window));
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
        assert_eq!(as_percent(0.02), 2.0, "and the faintest shading as 2%");
    }

    /// The row reads one way and writes the other, so a figure that survives
    /// the trip is the only thing standing between a pane height and a value
    /// a hundred times the one that was chosen.
    #[test]
    fn a_percentage_comes_back_as_the_fraction_it_was_shown_from() {
        for share in [MIN_PANE_SHARE, 0.16, 0.5, MAX_PANE_SHARE] {
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
