//! An indicator's settings, and the list and the picker they are reached
//! from: what a chart's settings dialog shows on its Indicators page, over
//! a [`Host`] — a chart, or anything standing for charts.
//!
//! [`present`] is the whole of it for a program embedding the chart: the
//! indicators on a chart, with a + to add one, each a page of its own.
//! [`add_indicator`] goes straight to the picker (Ctrl+Shift+I), and
//! [`present_indicator`] to one indicator's page (the gear in the legend).
//! Changes apply as they are made; the chart is right there.

mod time;

use std::cell::{Cell, OnceCell, RefCell};
use std::rc::Rc;

use adw::prelude::*;
use gtk::glib;
use omacharts_chart::colors;
use omacharts_engine::indicators::{
    self, Kind, LineStyle, Params, Reset, Stroke, MAX_FILL_ALPHA, MAX_PANE_SHARE, MIN_FILL_ALPHA,
    MIN_PANE_SHARE,
};
use omacharts_engine::indicators::cvd::Style;
use omacharts_engine::indicators::market;
use omacharts_engine::indicators::flow;
use omacharts_engine::indicators::profile::{Align, Style as ProfileStyle};
use omacharts_engine::theme::ColorChoice;
use omacharts_engine::{BarColouring, Indicator, Session};

use crate::colouring_rows;
use crate::controls;
use crate::dialogs;
use crate::drop_line;
use crate::profile_rows;
use crate::property_rows;
use crate::shortcuts;
use crate::stroke::{describe_stroke, stroke_sample, Ink, LINE_SAMPLE_WIDTH, WIDTHS};


/// What the indicator settings are opened over: a chart, or something
/// that stands for charts (an app's template of indicators). The settings
/// change the indicators through it and read what the chart in front of
/// whoever is editing draws.
pub trait Host {
    /// The indicators being edited.
    fn indicators(&self) -> Vec<Indicator>;
    /// Keep `indicators` and draw them.
    fn set_indicators(&self, indicators: Vec<Indicator>);
    fn theme(&self) -> omacharts_engine::Theme;
    /// The bar scheme the chart in front is painted in.
    fn bar_scheme(&self) -> omacharts_engine::BarScheme;
    /// The chart's colouring, and whether up and down are swapped.
    fn chart_look(&self) -> (BarColouring, bool);
    /// A template's id when these are a template's indicators, on charts of
    /// every resolution and feed, rather than one chart's.
    fn template(&self) -> Option<u32> {
        None
    }
    /// The template a chart's indicators are edited through, by name: said
    /// above every panel, since a change reaches other charts.
    fn through_template(&self) -> Option<String> {
        None
    }
    /// Whether a cumulative delta is estimated, its feed having no bid and
    /// ask volume.
    fn estimates_delta(&self) -> bool {
        false
    }
    /// How many rows profile `id` is drawing.
    fn profile_rows(&self, id: u32) -> Option<usize>;
    /// The chart's resolution.
    fn timeframe(&self) -> omacharts_engine::Timeframe;
    /// The chart's instrument and session, for session times' presets.
    fn market(&self) -> Option<(omacharts_engine::Instrument, Session)>;
    /// The time zone and the clock the chart's times are shown in.
    fn clock(&self) -> (chrono_tz::Tz, omacharts_engine::clock::TimeFormat);
    /// Call `watcher` whenever the chart changes, until it answers false.
    fn watch_chart(&self, watcher: Box<dyn Fn() -> bool>);
    /// Something about the chart changed that the settings did not set.
    fn chart_changed_elsewhere(&self) {}
    /// What the dialogs are presented over and sized against.
    fn parent(&self) -> gtk::Widget;
}

/// The indicator settings' hold on their [`Host`].
pub struct Owner {
    host: Rc<dyn Host>,
    /// What panels are sized against and shown over.
    pub window: gtk::Widget,
    /// A template's, rather than a chart's.
    template: Option<u32>,
}

impl Owner {
    pub fn new(host: Rc<dyn Host>) -> Rc<Owner> {
        Rc::new(Owner { window: host.parent(), template: host.template(), host })
    }

    fn through_template(&self) -> Option<String> {
        self.host.through_template()
    }

    pub fn indicators(&self) -> Vec<Indicator> {
        self.host.indicators()
    }

    pub fn set_indicators(&self, indicators: Vec<Indicator>) {
        self.host.set_indicators(indicators)
    }

    fn next_indicator_id(&self) -> u32 {
        self.indicators().iter().map(|i| i.id).max().unwrap_or(0) + 1
    }

    pub fn theme(&self) -> omacharts_engine::Theme {
        self.host.theme()
    }

    fn estimates_delta(&self) -> bool {
        self.template.is_none() && self.host.estimates_delta()
    }

    fn bar_scheme(&self) -> omacharts_engine::BarScheme {
        self.host.bar_scheme()
    }

    fn profile_rows(&self, id: u32) -> Option<usize> {
        match self.template {
            None => self.host.profile_rows(id),
            Some(_) => None,
        }
    }

    /// Call `watcher` whenever the chart changes, until it answers false.
    pub fn watch_chart(&self, watcher: impl Fn() -> bool + 'static) {
        self.host.watch_chart(Box::new(watcher));
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
    pub fn new(page: &adw::PreferencesPage, title: &str) -> IndicatorList {
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


/// Something to run when an indicator changes, so whatever is showing it
/// redraws.
///
/// A callback rather than the preferences dialog and its list, because adding
/// an indicator no longer needs either — the picker opens straight from the
/// chart — and a row that sets a line width should not have to know which of
/// the two it is serving.
#[derive(Clone)]
pub struct Refresh(Rc<dyn Fn()>);

impl Refresh {
    /// Nothing behind the panel to redraw.
    pub fn none() -> Refresh {
        Refresh(Rc::new(|| {}))
    }

    pub fn rebuilding(
        owner: &Rc<Owner>,
        dialog: &adw::PreferencesDialog,
        list: &IndicatorList,
    ) -> Refresh {
        let owner = owner.clone();
        let dialog = dialog.clone();
        let list = list.clone();
        Refresh(Rc::new(move || rebuild_indicators(&owner, &dialog, &list)))
    }

    pub fn run(&self) {
        (self.0)()
    }
}

/// Rebuild the list of indicators.
///
/// A list, and only a list: each row says what the indicator is and lets you
/// turn it off or take it away. Everything else lives on its own panel, because
/// a column of expanders holding every parameter of every indicator is a shape
/// you cannot read.
pub fn rebuild_indicators(
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
            "Drag to reorder. Colours come from the theme."
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

    // The price is not a row: every strip sits below it, so the list shows
    // only what can be changed. If where a strip sits becomes a setting, the
    // price comes back as a row strips are dropped across (see
    // `indicators::cross_price`).
    for indicator in &indicators {
        list.push(&indicator_row(owner, dialog, &refresh, &indicators, indicator));
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
            let dragged = indicators.remove(from);
            indicators.insert(to, dragged);
            owner.set_indicators(indicators);
            refresh.run();
            true
        },
    );
    row.add_controller(target);
}

/// Where an indicator's settings are shown.
pub enum Panel {
    /// Pushed over the list it was opened from, with a Done button.
    Subpage(adw::PreferencesDialog),
    /// A dialog of its own, with one button that adds the indicator. Used by
    /// the add path, which can start from the chart with no settings window
    /// open at all, so there is nothing to push a page onto. Never added,
    /// it comes back off.
    Add,
    /// A dialog of its own over one already on the chart, closed by its
    /// close button: the picker asked for another of a kind a chart holds
    /// one of, and this is the one it holds.
    Alone,
}

/// Retitles an indicator's open panel.
type Titler = Box<dyn Fn(&str)>;

/// One indicator's settings.
///
/// Changes apply as you make them, on the chart, whichever way this was
/// opened: you are looking at the thing you are configuring. For the add path
/// that means the indicator is already drawn while you set it up, and
/// dismissing the dialog takes it back off again — the Add button is how you
/// say you meant it.
pub fn open_indicator_panel_for(owner: &Rc<Owner>, refresh: &Refresh, id: u32, panel: Panel) {
    let indicators = owner.indicators();
    let Some(indicator) = indicators.iter().find(|i| i.id == id).cloned() else {
        return;
    };
    let colour = drawn_colour(owner, &indicators, id);

    // The panel's title is the indicator's label, which its settings write
    // — "Large orders > 50 · 20" — so it follows every change made here, as
    // the legend does, rather than saying what was set when it opened.
    let titled: Rc<RefCell<Option<Titler>>> = Rc::default();
    let refresh = &{
        let outer = refresh.clone();
        let owner = owner.clone();
        let titled = titled.clone();
        Refresh(Rc::new(move || {
            outer.run();
            if let (Some(title), Some(now)) = (titled.borrow().as_ref(), owner.indicators().iter().find(|i| i.id == id)) {
                title(&now.label());
            }
        }))
    };

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
        let parameters = parameters_group(owner, refresh, &indicator);
        page.add(&parameters);
        let look = appearance_groups(owner, refresh, &colour, &indicator);
        // A delta is painted in its colouring, as a drawing's is, and has
        // no value area.
        if indicator.kind == Kind::VolumeProfile {
            profile_rows::look_follows_method(&parameters, &look);
        }
        for group in &look {
            page.add(group);
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
            let titling = subpage.downgrade();
            *titled.borrow_mut() = Some(Box::new(move |title| {
                if let Some(subpage) = titling.upgrade() {
                    subpage.set_title(title);
                }
            }));
            dialog.push_subpage(&subpage);
            focus_first_parameter(&page);
        }
        Panel::Add => {
            let modal = adw::Dialog::new();
            modal.set_title(&indicator.label());
            let titling = modal.downgrade();
            *titled.borrow_mut() = Some(Box::new(move |title| {
                if let Some(modal) = titling.upgrade() {
                    modal.set_title(title);
                }
            }));
            modal.set_content_width(crate::dialogs::SETTINGS_WIDTH);
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
                let kept: Vec<Indicator> =
                    owner_for_close.indicators().into_iter().filter(|i| i.id != id).collect();
                owner_for_close.set_indicators(kept);
                refresh_for_close.run();
            });

            modal.present(Some(&owner.window));
            focus_first_parameter(&page);
        }
        Panel::Alone => {
            let modal = adw::Dialog::new();
            modal.set_title(&indicator.label());
            modal.set_content_width(crate::dialogs::SETTINGS_WIDTH);
            modal.set_content_height((owner.window.height() * 85 / 100).max(620));
            modal.set_child(Some(&toolbar));
            let titling = modal.downgrade();
            *titled.borrow_mut() = Some(Box::new(move |title| {
                if let Some(modal) = titling.upgrade() {
                    modal.set_title(title);
                }
            }));
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
/// How an indicator is drawn: a profile's Style, Value area and Point of
/// control groups, laid out by `profile_rows` alike for both kinds, or the
/// one group anything else has, if any.
pub fn appearance_groups(
    owner: &Rc<Owner>,
    refresh: &Refresh,
    colour: &str,
    indicator: &Indicator,
) -> Vec<adw::PreferencesGroup> {
    match indicator.kind {
        Kind::VolumeProfile => profile_style(owner, refresh, colour, indicator),
        Kind::MarketProfile => market_style(owner, refresh, colour, indicator),
        Kind::VolumeDelta | Kind::Footprint | Kind::LargeOrders => vec![flow_colours(owner, refresh, indicator)],
        _ => appearance_group(owner, refresh, colour, indicator).into_iter().collect(),
    }
}

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


    let group = adw::PreferencesGroup::new();
    group.set_title("Line");

    let ink = {
        let owner = owner.clone();
        Ink::new(move || drawn_colour(&owner, &owner.indicators(), id))
    };

    let line_strength = indicator_strength(owner, refresh, id, indicator.line_alpha(), |indicator, alpha| {
        indicator.alpha = Some(alpha)
    });
    group.add(&colour_row_with(
        owner,
        refresh,
        &ink,
        property_rows::COLOUR,
        colour,
        indicator.color.clone(),
        move |indicator, choice| indicator.color = choice,
        id,
        Some(line_strength),
    ));
    for row in stroke_rows(owner, refresh, &ink, id, "", indicator.stroke, {
        move |indicator: &mut Indicator, stroke: Stroke| indicator.stroke = stroke
    }) {
        group.add(&row);
    }

    // A strip's second line follows the theme the way a point of control
    // does: unset is the next colour along from the first line's, so the two
    // never come out the same.
    // A cumulative delta's line is drawn only as a line: as candles it is
    // in its colouring, and these rows would change nothing on screen.
    if let Params::Cvd { style, .. } = &indicator.params {
        let is_line = |params: &Params| matches!(params, Params::Cvd { style: Style::Line, .. });
        group.set_visible(*style == Style::Line);
        let weak = group.downgrade();
        let owner = owner.clone();
        owner.clone().watch_chart(move || {
            let Some(group) = weak.upgrade().filter(|group| group.root().is_some()) else { return false };
            if let Some(indicator) = owner.indicators().iter().find(|i| i.id == id) {
                group.set_visible(is_line(&indicator.params));
            }
            true
        });
    }

    // And its pattern beside its colour. Unset, it is drawn in the first
    // line's, which is what it always was.
    let second = match &indicator.params {
        Params::Stochastic { d_color, d_style, .. } => Some(("%D colour", "%D line style", d_color.clone(), *d_style)),
        Params::Macd { signal_color, signal_style, .. } => {
            Some(("Signal colour", "Signal line style", signal_color.clone(), *signal_style))
        }
        _ => None,
    };
    if let Some((title, pattern_title, chosen, pattern)) = second {
        let shown = owner.theme().companion_or(chosen.as_ref(), colour);
        let second_ink = {
            let owner = owner.clone();
            Ink::new(move || {
                let indicators = owner.indicators();
                let drawn = drawn_colour(&owner, &indicators, id);
                let chosen = indicators.iter().find(|i| i.id == id).and_then(|i| match &i.params {
                    Params::Stochastic { d_color, .. } => d_color.clone(),
                    Params::Macd { signal_color, .. } => signal_color.clone(),
                    _ => None,
                });
                owner.theme().companion_or(chosen.as_ref(), &drawn)
            })
        };
        let second_strength = indicator_strength(owner, refresh, id, indicator.second_alpha(), |indicator, alpha| {
            match &mut indicator.params {
                Params::Stochastic { d_alpha, .. } => *d_alpha = Some(alpha),
                Params::Macd { signal_alpha, .. } => *signal_alpha = Some(alpha),
                _ => {}
            }
        });
        group.add(&colour_row_with(
            owner,
            refresh,
            &second_ink,
            title,
            &shown,
            chosen,
            move |indicator, choice| match &mut indicator.params {
                Params::Stochastic { d_color, .. } => *d_color = choice,
                Params::Macd { signal_color, .. } => *signal_color = choice,
                _ => {}
            },
            id,
            Some(second_strength),
        ));
        let current = pattern.unwrap_or(indicator.stroke.style);
        group.add(&pattern_row(owner, refresh, &second_ink, id, pattern_title, current, |indicator, picked| {
            match &mut indicator.params {
                Params::Stochastic { d_style, .. } => *d_style = Some(picked),
                Params::Macd { signal_style, .. } => *signal_style = Some(picked),
                _ => {}
            }
        }));
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

/// The pattern alone, for a line whose thickness is a row of its own or is
/// not the indicator's: a point of control's, an outline's, a second line's.
fn pattern_row(
    owner: &Rc<Owner>,
    refresh: &Refresh,
    ink: &Ink,
    id: u32,
    title: &str,
    current: LineStyle,
    apply: impl Fn(&mut Indicator, LineStyle) + Clone + 'static,
) -> adw::ActionRow {
    let options = LineStyle::ALL.map(|style| (LINE_SAMPLE_WIDTH, Stroke { width: LINE_SAMPLE_WIDTH, style }));
    let row = picker_row(
        owner,
        refresh,
        ink,
        id,
        title,
        &options,
        Stroke { width: LINE_SAMPLE_WIDTH, style: current },
        move |indicator, picked| apply(indicator, picked.style),
    );
    row.add_css_class("line-style-row");
    row
}

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


/// A colour row: the swatch, whose palette offers Default first, the way
/// back to the colour the theme gives it, and how much of the chart shows
/// through the colour, on the app's standard colour row
/// ([`property_rows::colour_row`]).
///
/// Default is offered whenever the colour has one to go back to, even when a
/// pinned colour has the same hex as the theme's: pinning it still stops the
/// indicator following the theme.
#[allow(clippy::too_many_arguments)]
fn colour_row_with(
    owner: &Rc<Owner>,
    refresh: &Refresh,
    ink: &Ink,
    title: &str,
    current: &str,
    choice: Option<ColorChoice>,
    apply: impl Fn(&mut Indicator, Option<ColorChoice>) + Clone + 'static,
    id: u32,
    transparency: Option<property_rows::Transparency>,
) -> adw::ActionRow {
    let owner_for_colour = owner.clone();
    let refresh_for_colour = refresh.clone();
    let ink_for_colour = ink.clone();
    let apply_for_colour = apply.clone();
    // Back to the default, and what it came to: not a fixed colour but
    // whichever of the palette this indicator's siblings have left free, so
    // it is asked for after the change, not guessed before it.
    let default: Rc<dyn Fn() -> String> = {
        let owner = owner.clone();
        let refresh = refresh.clone();
        let ink = ink.clone();
        Rc::new(move || {
            let apply = apply.clone();
            update(&owner, id, move |indicator| apply(indicator, None));
            ink.repaint();
            refresh.run();
            drawn_colour(&owner, &owner.indicators(), id)
        })
    };
    let button = crate::palette::picker(
        &owner.theme(),
        choice,
        current,
        move |picked| {
            let apply = apply_for_colour.clone();
            update(&owner_for_colour, id, move |indicator| {
                apply(indicator, Some(picked.clone()))
            });
            ink_for_colour.repaint();
            refresh_for_colour.run();
        },
        Some(default),
    );
    property_rows::colour_row(title, &button, transparency)
}

/// How solid one of an indicator's colours is drawn, `alpha` now, for its
/// colour row: set by `write` as it is moved, as a strength.
fn indicator_strength(
    owner: &Rc<Owner>,
    refresh: &Refresh,
    id: u32,
    alpha: f64,
    write: impl Fn(&mut Indicator, f64) + 'static,
) -> property_rows::Transparency {
    let owner = owner.clone();
    let refresh = refresh.clone();
    profile_rows::strength(alpha, move |alpha| {
        update(&owner, id, |indicator| write(indicator, alpha));
        refresh.run();
    })
    .0
}

pub fn parameters_group(
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
        Params::Cvd { reset, style, height, .. } => {
            // Said once, here, rather than on the chart: the legend's ≈ is
            // the mark there.
            if owner.estimates_delta() {
                group.set_description(Some(CVD_ESTIMATED));
            }
            group.add(&reset_row_of(owner, refresh, id, *reset, true));
            let style_row = choice_row(
                owner,
                refresh,
                id,
                CVD_STYLE_LABEL,
                &Style::ALL.map(Style::label),
                Style::ALL.iter().position(|s| s == style),
                |indicator, at| {
                    if let Params::Cvd { style, .. } = &mut indicator.params {
                        *style = Style::ALL[at];
                    }
                },
            );
            // After the change is made: the line's own rows come and go with it.
            let host = owner.host.clone();
            style_row.connect_selected_notify(move |_| host.chart_changed_elsewhere());
            group.add(&style_row);
            study_colours(owner, refresh, indicator).add_to(&group);
            group.add(&pane_height_row(owner, refresh, id, *height));
        }
        Params::VolumeDelta { height, .. } => {
            if owner.estimates_delta() {
                group.set_description(Some(CVD_ESTIMATED));
            }
            group.add(&pane_height_row(owner, refresh, id, *height));
        }
        Params::Footprint { intensity, numbers, row_ticks, profile, compare, .. } => {
            // Automatic, or so many ticks a row, as a market profile's:
            // greyed while automatic, showing the ticks being drawn.
            let granularity = {
                let owner = owner.clone();
                let refresh = refresh.clone();
                let drawn: Rc<dyn Fn() -> Option<usize>> = {
                    let owner = owner.clone();
                    Rc::new(move || owner.profile_rows(id))
                };
                profile_rows::granularity(row_ticks.map(|t| t as usize), drawn, move |ticks| {
                    update(&owner, id, move |indicator| {
                        if let Params::Footprint { row_ticks, .. } = &mut indicator.params {
                            *row_ticks = ticks.map(|t| t as u32);
                        }
                    });
                    refresh.run();
                })
            };
            granularity.rows.set_title(MARKET_ROW_TICKS_LABEL);
            granularity.automatic.set_title(profile_rows::AUTOMATIC);
            granularity.rows.set_range(1.0, f64::from(omacharts_engine::studies::footprint::MAX_ROW_TICKS));
            (granularity.show)(row_ticks.map(|t| t as usize));
            group.add(&granularity.automatic);
            group.add(&granularity.rows);
            group.add(&choice_row(
                owner,
                refresh,
                id,
                LARGE_COMPARE_LABEL,
                &flow::Compare::ALL.map(flow::Compare::label),
                flow::Compare::ALL.iter().position(|c| c == compare),
                |indicator, at| {
                    if let Params::Footprint { compare, .. } = &mut indicator.params {
                        *compare = flow::Compare::ALL.get(at).copied().unwrap_or_else(flow::footprint_compare);
                    }
                },
            ));
            group.add(&flow_switch(owner, refresh, id, FOOTPRINT_INTENSITY_LABEL, *intensity, |params, on| {
                if let Params::Footprint { intensity, .. } = params {
                    *intensity = on;
                }
            }));
            group.add(&flow_switch(owner, refresh, id, FOOTPRINT_NUMBERS_LABEL, *numbers, |params, on| {
                if let Params::Footprint { numbers, .. } = params {
                    *numbers = on;
                }
            }));
            for row in footprint_profile_rows(owner, refresh, id, profile) {
                group.add(&row);
            }
        }
        Params::LargeOrders { regular, extended, compare, combine, .. } => {
            for (title, value, regular_hours) in
                [(LARGE_RTH_LABEL, *regular, true), (LARGE_ETH_LABEL, *extended, false)]
            {
                group.add(&spin_row(
                    owner,
                    refresh,
                    id,
                    title,
                    value as f64,
                    0.0,
                    LARGE_MAX_THRESHOLD as f64,
                    1.0,
                    move |indicator, value| {
                        if let Params::LargeOrders { regular, extended, .. } = &mut indicator.params {
                            *(if regular_hours { regular } else { extended }) = value.max(0.0) as u64;
                        }
                    },
                ));
            }
            group.add(&choice_row(
                owner,
                refresh,
                id,
                LARGE_COMPARE_LABEL,
                &flow::Compare::ALL.map(flow::Compare::label),
                flow::Compare::ALL.iter().position(|c| c == compare),
                |indicator, at| {
                    if let Params::LargeOrders { compare, .. } = &mut indicator.params {
                        *compare = flow::Compare::ALL.get(at).copied().unwrap_or_default();
                    }
                },
            ));
            group.add(&flow_switch(owner, refresh, id, LARGE_COMBINE_LABEL, *combine, |params, on| {
                if let Params::LargeOrders { combine, .. } = params {
                    *combine = on;
                }
            }));
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
        Params::VolumeProfile {
            reset, rows, value_area, method, last_only, ..
        } => {
            let reset_row = reset_row(owner, refresh, id, *reset);
            // The newest period alone, on either method and either edge.
            let last_row = last_only_row(owner, refresh, id, *last_only);
            let (method_row, _) = {
                let owner = owner.clone();
                let refresh = refresh.clone();
                profile_rows::method_row(*method, move |picked| {
                    update(&owner, id, |indicator| {
                        if let Params::VolumeProfile { method, .. } = &mut indicator.params {
                            *method = picked;
                        }
                    });
                    refresh.run();
                })
            };
            let colours = study_colours(owner, refresh, indicator);
            let colouring_row = colours.colouring();
            let granularity = {
                let owner = owner.clone();
                let refresh = refresh.clone();
                let drawn: Rc<dyn Fn() -> Option<usize>> = {
                    let owner = owner.clone();
                    Rc::new(move || owner.profile_rows(id))
                };
                profile_rows::granularity(*rows, drawn, move |count| {
                    update(&owner, id, move |indicator| {
                        if let Params::VolumeProfile { rows, .. } = &mut indicator.params {
                            *rows = count;
                        }
                    });
                    refresh.run();
                })
            };
            let (area_row, _) = {
                let owner = owner.clone();
                let refresh = refresh.clone();
                profile_rows::value_area_row(*value_area, move |share| {
                    update(&owner, id, |indicator| {
                        if let Params::VolumeProfile { value_area, .. } = &mut indicator.params {
                            *value_area = share;
                        }
                    });
                    refresh.run();
                })
            };
            profile_rows::follow_method(
                &method_row,
                profile_rows::ByMethod {
                    value_area: area_row.clone().upcast(),
                    colouring: colouring_row.clone(),
                    total_only: Vec::new(),
                },
            );
            profile_rows::add_parameters(
                &group,
                &profile_rows::ParameterRows {
                    leading: vec![reset_row.upcast(), last_row.upcast()],
                    own: std::iter::once(method_row.upcast()).chain(colours.widgets.iter().cloned()).collect(),
                    granularity,
                    value_area: area_row.upcast(),
                },
            );
        }
        Params::MarketProfile { reset, minutes, row_ticks, value_area, last_only, .. } => {
            let reset_row = reset_row(owner, refresh, id, *reset);
            let last_row = last_only_row(owner, refresh, id, *last_only);
            let minutes_row = spin_row(
                owner,
                refresh,
                id,
                MARKET_MINUTES_LABEL,
                f64::from(*minutes),
                f64::from(market::MIN_MINUTES),
                f64::from(market::MAX_MINUTES),
                1.0,
                move |indicator, value| {
                    if let Params::MarketProfile { minutes, .. } = &mut indicator.params {
                        *minutes = value as u32;
                    }
                },
            );
            // Automatic, or so many ticks a row: greyed while automatic,
            // showing the height being drawn.
            let granularity = {
                let owner = owner.clone();
                let refresh = refresh.clone();
                let drawn: Rc<dyn Fn() -> Option<usize>> = {
                    let owner = owner.clone();
                    Rc::new(move || owner.profile_rows(id))
                };
                profile_rows::granularity(row_ticks.map(|t| t as usize), drawn, move |ticks| {
                    update(&owner, id, move |indicator| {
                        if let Params::MarketProfile { row_ticks, .. } = &mut indicator.params {
                            *row_ticks = ticks.map(|t| t as u32);
                        }
                    });
                    refresh.run();
                })
            };
            granularity.rows.set_title(MARKET_ROW_TICKS_LABEL);
            granularity.automatic.set_title(profile_rows::AUTOMATIC);
            granularity.rows.set_range(1.0, f64::from(market::MAX_ROW_TICKS));
            (granularity.show)(row_ticks.map(|t| t as usize));
            let (area_row, _) = {
                let owner = owner.clone();
                let refresh = refresh.clone();
                profile_rows::value_area_row(*value_area, move |share| {
                    update(&owner, id, |indicator| {
                        if let Params::MarketProfile { value_area, .. } = &mut indicator.params {
                            *value_area = share;
                        }
                    });
                    refresh.run();
                })
            };
            // The volume profile's rows in its order, the TPO period where
            // its method is.
            profile_rows::add_parameters(
                &group,
                &profile_rows::ParameterRows {
                    leading: vec![reset_row.upcast(), last_row.upcast()],
                    own: vec![minutes_row.upcast()],
                    granularity,
                    value_area: area_row.upcast(),
                },
            );
        }
        // Edited on a page of its own; see `time`.
        Params::SessionTimes { .. } => {}
    }
    group
}

/// The market profile row that says how long a TPO period is.
pub const MARKET_MINUTES_LABEL: &str = "TPO period (minutes)";

/// Whether a profile study draws only its newest period, on either kind,
/// either method and either edge.
fn last_only_row(owner: &Rc<Owner>, refresh: &Refresh, id: u32, on: bool) -> adw::SwitchRow {
    let owner = owner.clone();
    let refresh = refresh.clone();
    profile_rows::switch_row(PROFILE_LAST_ONLY_LABEL, on, move |on| {
        update(&owner, id, |indicator| {
            if let Params::VolumeProfile { last_only, .. } | Params::MarketProfile { last_only, .. } = &mut indicator.params {
                *last_only = on;
            }
        });
        refresh.run();
    })
    .0
}

/// The market profile box that sets how many ticks tall a row is.
pub const MARKET_ROW_TICKS_LABEL: &str = "Row height (ticks)";

/// How a market profile is drawn: letters or blocks, its colour, its first
/// and last TPO, its side and width, which it shares a period's room with a
/// volume profile by; its outline; its value area and its point of control.
/// The volume profile's groups, rows and order, with its first and last TPO
/// after the outline.
fn market_style(owner: &Rc<Owner>, refresh: &Refresh, colour: &str, indicator: &Indicator) -> Vec<adw::PreferencesGroup> {
    let id = indicator.id;
    let Params::MarketProfile {
        style,
        align,
        width,
        poc,
        poc_color,
        highlight,
        value_area_color,
        open_close,
        open_color,
        close_color,
        poc_width,
        poc_style,
        outline,
        outline_color,
        outline_width,
        outline_style,
        expand_last,
        ..
    } = &indicator.params
    else {
        return Vec::new();
    };
    // The outline's and the point of control's patterns are drawn in their
    // colours, as a volume profile's are.
    let params_of = {
        let owner = owner.clone();
        move || owner.indicators().into_iter().find(|i| i.id == id).map(|i| i.params)
    };
    let outline_ink = {
        let owner = owner.clone();
        let params_of = params_of.clone();
        Ink::new(move || {
            let drawn = drawn_colour(&owner, &owner.indicators(), id);
            match params_of() {
                Some(Params::MarketProfile { outline_color: Some(choice), .. }) => choice.resolve(&owner.theme()),
                _ => drawn,
            }
        })
    };
    let poc_ink = {
        let owner = owner.clone();
        Ink::new(move || {
            let drawn = drawn_colour(&owner, &owner.indicators(), id);
            let chosen = match params_of() {
                Some(Params::MarketProfile { poc_color, .. }) => poc_color,
                _ => None,
            };
            owner.theme().companion_or(chosen.as_ref(), &drawn)
        })
    };
    let shape = choice_row(
        owner,
        refresh,
        id,
        profile_rows::DISPLAY_AS,
        &market::Style::ALL.map(market::Style::label),
        market::Style::ALL.iter().position(|s| s == style),
        |indicator, at| {
            if let Params::MarketProfile { style, .. } = &mut indicator.params {
                *style = market::Style::ALL[at];
            }
        },
    );
    // The value area's and the rest's strengths follow the rows' until set
    // apart, and are shown again as the rows' is moved.
    let followers: Followers = Rc::default();
    let strength = |read: fn(&Params) -> Option<f64>, write: fn(&mut Params, f64), moves: Option<&Followers>| {
        strength(owner, refresh, indicator, read, write, moves)
    };
    let (transparency, _) = strength(
        |params| market_strengths(params).map(|s| s.rows),
        |params, a| {
            if let Params::MarketProfile { alpha, .. } = params {
                *alpha = Some(a);
            }
        },
        Some(&followers),
    );
    let follower = |read: fn(&Params) -> Option<f64>, write: fn(&mut Params, f64)| {
        let (transparency, show) = strength(read, write, None);
        followers.borrow_mut().push(show);
        transparency
    };
    let area_strength = follower(
        |params| market_strengths(params).map(|s| s.value_area),
        |params, a| {
            if let Params::MarketProfile { value_area_alpha, .. } = params {
                *value_area_alpha = Some(a);
            }
        },
    );
    let outside_strength = follower(
        |params| market_strengths(params).map(|s| s.outside),
        |params, a| {
            if let Params::MarketProfile { outside_alpha, .. } = params {
                *outside_alpha = Some(a);
            }
        },
    )
    .alone(profile_rows::OUTSIDE_TRANSPARENCY, profile_rows::OUTSIDE_TIP);
    let (poc_strength, _) = strength(
        |params| market_strengths(params).map(|s| s.poc),
        |params, a| {
            if let Params::MarketProfile { poc_alpha, .. } = params {
                *poc_alpha = Some(a);
            }
        },
        None,
    );
    let (outline_strength, _) = strength(
        |params| match params {
            Params::MarketProfile { outline_alpha, .. } => Some(market::outline_strength(*outline_alpha)),
            _ => None,
        },
        |params, a| {
            if let Params::MarketProfile { outline_alpha, .. } = params {
                *outline_alpha = Some(a);
            }
        },
        None,
    );
    let (open_strength, _) = strength(
        |params| match params {
            Params::MarketProfile { open_alpha, .. } => Some(market::end_strength(*open_alpha)),
            _ => None,
        },
        |params, a| {
            if let Params::MarketProfile { open_alpha, .. } = params {
                *open_alpha = Some(a);
            }
        },
        None,
    );
    let (close_strength, _) = strength(
        |params| match params {
            Params::MarketProfile { close_alpha, .. } => Some(market::end_strength(*close_alpha)),
            _ => None,
        },
        |params, a| {
            if let Params::MarketProfile { close_alpha, .. } = params {
                *close_alpha = Some(a);
            }
        },
        None,
    );
    let (outline_switch, _) = {
        let owner = owner.clone();
        let refresh = refresh.clone();
        profile_rows::switch_row(profile_rows::OUTLINE, *outline, move |on| {
            update(&owner, id, |indicator| {
                if let Params::MarketProfile { outline, .. } = &mut indicator.params {
                    *outline = on;
                }
            });
            refresh.run();
        })
    };
    let outline_row = colour_row_with(
        owner,
        refresh,
        &outline_ink,
        profile_rows::OUTLINE_COLOUR,
        &outline_color.as_ref().map(|c| c.resolve(&owner.theme())).unwrap_or_else(|| colour.to_string()),
        outline_color.clone(),
        move |indicator, choice| {
            if let Params::MarketProfile { outline_color, .. } = &mut indicator.params {
                *outline_color = choice;
            }
        },
        id,
        Some(outline_strength),
    );
    let outline_thickness = thickness_row(
        owner,
        refresh,
        id,
        profile_rows::OUTLINE_THICKNESS,
        *outline_width,
        |indicator, width| {
            if let Params::MarketProfile { outline_width, .. } = &mut indicator.params {
                *outline_width = width;
            }
        },
    );
    let outline_pattern = pattern_row(
        owner,
        refresh,
        &outline_ink,
        id,
        profile_rows::OUTLINE_STYLE,
        *outline_style,
        |indicator, picked| {
            if let Params::MarketProfile { outline_style, .. } = &mut indicator.params {
                *outline_style = picked;
            }
        },
    );
    let poc_width_row = thickness_row(
        owner,
        refresh,
        id,
        profile_rows::THICKNESS,
        *poc_width,
        |indicator, width| {
            if let Params::MarketProfile { poc_width, .. } = &mut indicator.params {
                *poc_width = width;
            }
        },
    );
    let poc_pattern = pattern_row(
        owner,
        refresh,
        &poc_ink,
        id,
        profile_rows::LINE_STYLE,
        *poc_style,
        |indicator, picked| {
            if let Params::MarketProfile { poc_style, .. } = &mut indicator.params {
                *poc_style = picked;
            }
        },
    );
    let background = colour_row_with(
        owner,
        refresh,
        &Ink::new(String::new),
        property_rows::COLOUR,
        colour,
        indicator.color.clone(),
        move |indicator, choice| indicator.color = choice,
        id,
        Some(transparency),
    );
    let (poc_switch, _) = {
        let owner = owner.clone();
        let refresh = refresh.clone();
        profile_rows::switch_row(profile_rows::SHOW, *poc, move |on| {
            update(&owner, id, |indicator| {
                if let Params::MarketProfile { poc, .. } = &mut indicator.params {
                    *poc = on;
                }
            });
            refresh.run();
        })
    };
    let shown = owner.theme().companion_or(poc_color.as_ref(), colour);
    let poc_colour_row = colour_row_with(
        owner,
        refresh,
        &poc_ink,
        profile_rows::COLOUR,
        &shown,
        poc_color.clone(),
        move |indicator, choice| {
            if let Params::MarketProfile { poc_color, .. } = &mut indicator.params {
                *poc_color = choice;
            }
        },
        id,
        Some(poc_strength),
    );
    // The open's colour and the close's, offered while they are marked.
    let theme = owner.theme();
    let end_row = |title: &str, shown: String, choice: &Option<ColorChoice>, first: bool, strength| {
        colour_row_with(
            owner,
            refresh,
            &Ink::new(String::new),
            title,
            &shown,
            choice.clone(),
            move |indicator, choice| {
                if let Params::MarketProfile { open_color, close_color, .. } = &mut indicator.params {
                    *(if first { open_color } else { close_color }) = choice;
                }
            },
            id,
            Some(strength),
        )
    };
    let first_row = end_row(
        profile_rows::OPEN_COLOUR,
        open_color.as_ref().map_or_else(|| theme.tpo_open(), |c| c.resolve(&theme)),
        open_color,
        true,
        open_strength,
    );
    let last_row = end_row(
        profile_rows::CLOSE_COLOUR,
        close_color.as_ref().map_or_else(|| theme.tpo_close(), |c| c.resolve(&theme)),
        close_color,
        false,
        close_strength,
    );
    let (ends_switch, _) = {
        let owner = owner.clone();
        let refresh = refresh.clone();
        profile_rows::switch_row(profile_rows::OPEN_CLOSE, *open_close, move |on| {
            update(&owner, id, |indicator| {
                if let Params::MarketProfile { open_close, .. } = &mut indicator.params {
                    *open_close = on;
                }
            });
            refresh.run();
        })
    };
    let alignment = choice_row(
        owner,
        refresh,
        id,
        profile_rows::ALIGNMENT,
        &Align::ALL.map(Align::label),
        Align::ALL.iter().position(|a| a == align),
        |indicator, at| {
            if let Params::MarketProfile { align, .. } = &mut indicator.params {
                *align = Align::ALL[at];
            }
        },
    );
    let (width_row, _) = {
        let owner = owner.clone();
        let refresh = refresh.clone();
        profile_rows::width_row(*width, move |share| {
            update(&owner, id, |indicator| {
                if let Params::MarketProfile { width, .. } = &mut indicator.params {
                    *width = share;
                }
            });
            refresh.run();
        })
    };
    let (expand_row, _) = {
        let owner = owner.clone();
        let refresh = refresh.clone();
        profile_rows::switch_row(profile_rows::EXPAND_LAST, *expand_last, move |on| {
            update(&owner, id, |indicator| {
                if let Params::MarketProfile { expand_last, .. } = &mut indicator.params {
                    *expand_last = on;
                }
            });
            refresh.run();
        })
    };
    expand_row.set_tooltip_text(Some(profile_rows::EXPAND_LAST_TIP));
    let (highlight_row, area_colour_row) = value_area_rows(
        owner,
        refresh,
        id,
        *highlight,
        value_area_color.clone(),
        area_strength,
    );
    profile_rows::look_groups(profile_rows::LookRows {
        style: profile_rows::StyleRows {
            display: shape.upcast(),
            colour: background.upcast(),
            outline: Some(profile_rows::OutlineRows {
                switch: outline_switch,
                colour: outline_row.upcast(),
                thickness: outline_thickness.upcast(),
                style: outline_pattern.upcast(),
            }),
            ends: Some(profile_rows::EndRows {
                switch: ends_switch,
                open: first_row.upcast(),
                close: last_row.upcast(),
            }),
            alignment: alignment.upcast(),
            width: width_row.upcast(),
            expand_last: Some(expand_row.upcast()),
        },
        value_area: profile_rows::ValueAreaRows {
            highlight: highlight_row,
            colour: area_colour_row.upcast(),
            outside: outside_strength.upcast(),
        },
        poc: profile_rows::PocRows {
            show: poc_switch,
            colour: poc_colour_row.upcast(),
            thickness: Some(poc_width_row.upcast()),
            style: Some(poc_pattern.upcast()),
        },
    })
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

/// A rising and a falling session's volume bar, and how solid both are.
/// Unset, each is the candle's own colour.
fn volume_colours(
    owner: &Rc<Owner>,
    refresh: &Refresh,
    indicator: &Indicator,
) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::new();
    group.set_title("Colours");
    let rows = Rc::new(RefCell::new(volume_rows(owner, refresh, indicator)));
    for row in rows.borrow().iter() {
        group.add(row);
    }
    // The palette, and the colours following the chart's, can be changed by
    // a command while this is open. The rows are built again rather than set,
    // because building one sets nothing: no choice is made by reading one.
    let id = indicator.id;
    let weak = group.downgrade();
    let owner_for_watch = owner.clone();
    let refresh = refresh.clone();
    owner.watch_chart(move || {
        let Some(group) = weak.upgrade().filter(|group| group.root().is_some()) else {
            return false;
        };
        let indicators = owner_for_watch.indicators();
        let Some(indicator) = indicators.iter().find(|i| i.id == id) else { return true };
        for row in rows.borrow_mut().drain(..) {
            group.remove(&row);
        }
        let fresh = volume_rows(&owner_for_watch, &refresh, indicator);
        for row in &fresh {
            group.add(row);
        }
        *rows.borrow_mut() = fresh;
        true
    });
    group
}

/// The volume pane's palette, its opacity and its two colours, as it is
/// painted now.
fn volume_rows(owner: &Rc<Owner>, refresh: &Refresh, indicator: &Indicator) -> Vec<gtk::Widget> {
    study_colours(owner, refresh, indicator).widgets
}

/// The volume pane's colour rows, each with how solid its bars are.
pub const VOLUME_UP: &str = "Up";
pub const VOLUME_DOWN: &str = "Down";

/// The colour each role of `indicator` is drawn in now: a volume pane's
/// Default is the chart's own bars, as every study's is.
fn study_drawn(owner: &Owner, indicator: &Indicator) -> Vec<String> {
    let theme = owner.theme();
    if indicator.kind == Kind::Volume {
        let (up, down) = omacharts_chart::volume_colours(indicator, &theme, &owner.bar_scheme(), owner.host.chart_look().1);
        return vec![up, down];
    }
    let Some((colouring, own)) = indicator.params.colours() else { return Vec::new() };
    let (house, swapped) = owner.host.chart_look();
    let base = omacharts_chart::delta_colours(omacharts_engine::colouring::base(colouring), &theme, house, swapped);
    colouring_rows::drawn_roles(colouring, own, base, &theme)
}

/// How solid a study's roles are drawn unless told.
fn designed_alpha(indicator: &Indicator) -> f64 {
    match indicator.kind {
        Kind::Volume => omacharts_chart::volume_alphas(indicator).0,
        Kind::Footprint => flow::FOOTPRINT_ALPHA,
        Kind::LargeOrders => flow::LARGE_ORDERS_ALPHA,
        _ => flow::VOLUME_DELTA_ALPHA,
    }
}

/// The Colouring rows every study painted in one shows, from
/// [`colouring_rows`]: a volume pane, a volume profile, the cumulative
/// delta, the volume delta, the footprint and large orders.
fn study_colours(owner: &Rc<Owner>, refresh: &Refresh, indicator: &Indicator) -> colouring_rows::Rows {
    let id = indicator.id;
    let read = {
        let owner = owner.clone();
        // What the dialog opened on, for one built before it is on a chart.
        let opened = indicator.clone();
        move || {
            let indicators = owner.indicators();
            let indicator = indicators.iter().find(|i| i.id == id).unwrap_or(&opened);
            let drawn = study_drawn(&owner, indicator);
            let designed = designed_alpha(indicator);
            let mut params = indicator.params.clone();
            match params.colours_mut() {
                Some(colours) => colouring_rows::Shown::of(&colours, drawn, designed),
                None => colouring_rows::Shown::empty(),
            }
        }
    };
    let edit = {
        let owner = owner.clone();
        let refresh = refresh.clone();
        move |change: &dyn Fn(&mut omacharts_engine::colouring::ColoursMut, &[String])| {
            let indicators = owner.indicators();
            let Some(drawn) = indicators.iter().find(|i| i.id == id).map(|i| study_drawn(&owner, i)) else { return };
            update(&owner, id, |indicator| {
                if let Some(mut colours) = indicator.params.colours_mut() {
                    change(&mut colours, &drawn);
                }
            });
            refresh.run();
        }
    };
    colouring_rows::rows(colouring_rows::Host { theme: owner.theme(), read: Box::new(read), edit: Box::new(edit) })
}

/// The rows of the order flow indicators' own.
pub const FOOTPRINT_INTENSITY_LABEL: &str = "Shade by delta";
pub const FOOTPRINT_NUMBERS_LABEL: &str = "Numbers";
pub const FOOTPRINT_PROFILE_LABEL: &str = "Volume profile";
pub const FOOTPRINT_PROFILE_COLOUR_LABEL: &str = "Profile colour";
pub const FOOTPRINT_PROFILE_NUMBERS_LABEL: &str = "Profile numbers";
pub const LARGE_RTH_LABEL: &str = "RTH threshold";
pub const LARGE_ETH_LABEL: &str = "ETH threshold";
pub const LARGE_COMPARE_LABEL: &str = "Size against";
/// Which other charts' drawings a chart shows: see
/// [`omacharts_engine::drawings::ShowDrawings`].
pub const SHOW_DRAWINGS_LABEL: &str = "Drawings from other charts";
pub const LARGE_COMBINE_LABEL: &str = "Combine same-time prints";
pub const FLOW_BUY_LABEL: &str = "Buying";
pub const FLOW_SELL_LABEL: &str = "Selling";
/// The biggest threshold the box takes: past any single order's size.
pub const LARGE_MAX_THRESHOLD: u64 = omacharts_engine::indicators::flow::MAX_THRESHOLD;

/// A switch of an order flow indicator's.
fn flow_switch(
    owner: &Rc<Owner>,
    refresh: &Refresh,
    id: u32,
    title: &str,
    on: bool,
    write: impl Fn(&mut Params, bool) + 'static,
) -> adw::SwitchRow {
    let owner = owner.clone();
    let refresh = refresh.clone();
    property_rows::switch_row(title, on, move |on| {
        update(&owner, id, |indicator| write(&mut indicator.params, on));
        refresh.run();
    })
    .0
}

/// A footprint's volume profile: its switch, and under it while it is on,
/// its colour beside how solid it is, and whether its totals are written.
fn footprint_profile_rows(owner: &Rc<Owner>, refresh: &Refresh, id: u32, profile: &flow::BarProfile) -> Vec<gtk::Widget> {
    let edit = {
        let owner = owner.clone();
        let refresh = refresh.clone();
        Rc::new(move |change: &dyn Fn(&mut flow::BarProfile)| {
            update(&owner, id, |indicator| {
                if let Params::Footprint { profile, .. } = &mut indicator.params {
                    change(profile);
                }
            });
            refresh.run();
        })
    };
    let theme = owner.theme();
    let drawn = profile.chosen().resolve(&theme);
    let button = {
        let picked = {
            let edit = edit.clone();
            move |choice: ColorChoice| edit(&|profile| profile.colour = Some(choice.clone()))
        };
        let default: Rc<dyn Fn() -> String> = {
            let edit = edit.clone();
            let theme = theme.clone();
            Rc::new(move || {
                edit(&|profile| profile.colour = None);
                flow::BarProfile::default().chosen().resolve(&theme)
            })
        };
        crate::palette::picker(&theme, profile.colour.clone(), &drawn, picked, Some(default))
    };
    let (transparency, _) = {
        let edit = edit.clone();
        property_rows::Transparency::new(1.0 - profile.drawn_alpha(), move |share| {
            edit(&|profile| profile.alpha = Some(1.0 - share))
        })
    };
    let colour = property_rows::colour_row(FOOTPRINT_PROFILE_COLOUR_LABEL, &button, Some(transparency));
    let numbers = {
        let edit = edit.clone();
        property_rows::switch_row(FOOTPRINT_PROFILE_NUMBERS_LABEL, profile.numbers, move |on| edit(&|profile| profile.numbers = on)).0
    };
    let shown = |on: bool| {
        colour.set_visible(on);
        numbers.set_visible(on);
    };
    shown(profile.on);
    let switch = {
        let (colour, numbers) = (colour.clone(), numbers.clone());
        property_rows::switch_row(FOOTPRINT_PROFILE_LABEL, profile.on, move |on| {
            edit(&|profile| profile.on = on);
            colour.set_visible(on);
            numbers.set_visible(on);
        })
        .0
    };
    vec![switch.upcast(), colour.upcast(), numbers.upcast()]
}

/// How an order flow indicator paints buying and selling: its colouring,
/// and each side's colour beside how solid it is, as a volume pane's are.
fn flow_colours(owner: &Rc<Owner>, refresh: &Refresh, indicator: &Indicator) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::new();
    group.set_title("Colours");
    study_colours(owner, refresh, indicator).add_to(&group);
    group
}

/// The cumulative delta row that picks candles or a line.
const CVD_STYLE_LABEL: &str = "Style";

/// What a cumulative delta's settings say, once, on a chart whose feed
/// cannot tell bought from sold.
pub const CVD_ESTIMATED: &str = "Estimated from bar direction: this feed has no bid and ask volume";

/// The volume profile switch that sets its value area apart from its other
/// rows.
pub const PROFILE_HIGHLIGHT_LABEL: &str = profile_rows::HIGHLIGHT;

/// The volume profile row that paints its value area a colour of its own.
pub const PROFILE_AREA_COLOUR_LABEL: &str = profile_rows::COLOUR;

/// Whether a volume profile sets its value area apart, and in what colour:
/// unset, the rows' own, only stronger, as it always was.
fn value_area_rows(
    owner: &Rc<Owner>,
    refresh: &Refresh,
    id: u32,
    highlight: bool,
    choice: Option<ColorChoice>,
    transparency: property_rows::Transparency,
) -> (adw::SwitchRow, adw::ActionRow) {
    let switch = adw::SwitchRow::new();
    switch.set_title(PROFILE_HIGHLIGHT_LABEL);
    switch.set_active(highlight);
    {
        let owner = owner.clone();
        let refresh = refresh.clone();
        switch.connect_active_notify(move |row| {
            let on = row.is_active();
            update(&owner, id, |indicator| {
                if let Params::VolumeProfile { highlight, .. } | Params::MarketProfile { highlight, .. } =
                    &mut indicator.params
                {
                    *highlight = on;
                }
            });
            refresh.run();
        });
    }
    let drawn = drawn_colour(owner, &owner.indicators(), id);
    let shown = choice.as_ref().map(|c| c.resolve(&owner.theme())).unwrap_or(drawn);
    let colour = colour_row_with(
        owner,
        refresh,
        &Ink::new(String::new),
        PROFILE_AREA_COLOUR_LABEL,
        &shown,
        choice,
        |indicator, choice| {
            if let Params::VolumeProfile { value_area_color, .. } | Params::MarketProfile { value_area_color, .. } =
                &mut indicator.params
            {
                *value_area_color = choice;
            }
        },
        id,
        Some(transparency),
    );
    (switch, colour)
}

/// The volume profile switch that draws the newest period's profile alone.
pub const PROFILE_LAST_ONLY_LABEL: &str = profile_rows::LAST_ONLY;

/// The volume profile switch that draws its point of control or leaves it
/// out.
pub const PROFILE_POC_LABEL: &str = profile_rows::SHOW;

/// The volume profile row that fades its rows, or its outline's fill: the
/// rows' Colour row, which carries their transparency.
pub const PROFILE_TRANSPARENCY_LABEL: &str = profile_rows::COLOUR;

/// The volume profile row that says whether its rows are filled or drawn
/// round.
pub const PROFILE_STYLE_LABEL: &str = profile_rows::DISPLAY_AS;

/// How a volume profile is drawn: its outline, its rows, its point of
/// control, and its shape, side and width.
fn profile_style(
    owner: &Rc<Owner>,
    refresh: &Refresh,
    colour: &str,
    indicator: &Indicator,
) -> Vec<adw::PreferencesGroup> {
    let id = indicator.id;
    let Params::VolumeProfile {
        style,
        align,
        width,
        poc_color,
        highlight,
        value_area_color,
        outline,
        outline_color,
        outline_width,
        outline_style,
        poc_width,
        poc_style,
        poc,
        alpha,
        opacity,
        ..
    } = &indicator.params
    else {
        return Vec::new();
    };

    // Laid out by `profile_rows`, in the order a profile drawing's are. The
    // colour has no samples to repaint; the outline's and the point of
    // control's patterns are drawn in their colours.
    let unused = Ink::new(String::new);
    let params_of = {
        let owner = owner.clone();
        move || owner.indicators().into_iter().find(|i| i.id == id).map(|i| i.params)
    };
    let outline_ink = {
        let owner = owner.clone();
        let params_of = params_of.clone();
        Ink::new(move || {
            let drawn = drawn_colour(&owner, &owner.indicators(), id);
            match params_of() {
                Some(Params::VolumeProfile { outline_color: Some(choice), .. }) => choice.resolve(&owner.theme()),
                _ => drawn,
            }
        })
    };
    let poc_ink = {
        let owner = owner.clone();
        let params_of = params_of.clone();
        Ink::new(move || {
            let drawn = drawn_colour(&owner, &owner.indicators(), id);
            let chosen = match params_of() {
                Some(Params::VolumeProfile { poc_color, .. }) => poc_color,
                _ => None,
            };
            owner.theme().companion_or(chosen.as_ref(), &drawn)
        })
    };

    // Filled rows wear an outline only when asked; the outline style is
    // nothing but one, so the switch is for the filled style.
    let filled = *style == ProfileStyle::Filled;
    let switch = adw::SwitchRow::new();
    switch.set_title(profile_rows::OUTLINE);
    switch.set_active(*outline || !filled);
    switch.set_sensitive(filled);
    let shown_outline = outline_color
        .as_ref()
        .map(|c| c.resolve(&owner.theme()))
        .unwrap_or_else(|| colour.to_string());
    let outline_pattern = pattern_row(
        owner,
        refresh,
        &outline_ink,
        id,
        profile_rows::OUTLINE_STYLE,
        *outline_style,
        |indicator, picked| {
            if let Params::VolumeProfile { outline_style, .. } = &mut indicator.params {
                *outline_style = picked;
            }
        },
    );
    let width_row = thickness_row(
        owner,
        refresh,
        id,
        profile_rows::OUTLINE_THICKNESS,
        *outline_width,
        |indicator, width| {
            if let Params::VolumeProfile { outline_width, .. } = &mut indicator.params {
                *outline_width = width;
            }
        },
    );
    // Set while the Style row puts the switch right, so the switch does not
    // take that for a hand turning the outline on or off.
    let following = Rc::new(Cell::new(false));
    {
        let owner = owner.clone();
        let refresh = refresh.clone();
        let following = following.clone();
        switch.connect_active_notify(move |row| {
            if following.get() {
                return;
            }
            let on = row.is_active();
            update(&owner, id, |indicator| {
                if let Params::VolumeProfile { outline, .. } = &mut indicator.params {
                    *outline = on;
                }
            });
            refresh.run();
        });
    }

    // What follows the rows' strength until set apart, shown again as it
    // is moved.
    let followers: Followers = Rc::default();
    let follow = {
        let followers = followers.clone();
        move || {
            for show in followers.borrow().iter() {
                show();
            }
        }
    };
    // A profile saved with an older fade reads here as what it draws.
    let (transparency, _) = {
        let owner = owner.clone();
        let refresh = refresh.clone();
        let follow = follow.clone();
        property_rows::Transparency::new(
            1.0 - omacharts_engine::indicators::profile::drawn_alpha(*alpha, *opacity),
            move |share| {
                update(&owner, id, |indicator| {
                    if let Params::VolumeProfile { alpha, opacity, .. } = &mut indicator.params {
                        *alpha = (1.0 - share).clamp(0.0, 1.0);
                        *opacity = omacharts_engine::indicators::profile::opaque();
                    }
                });
                follow();
                refresh.run();
            },
        )
    };
    let strength = |read: fn(&Params) -> Option<f64>, write: fn(&mut Params, f64)| {
        let (transparency, show) = strength(owner, refresh, indicator, read, write, None);
        followers.borrow_mut().push(show);
        transparency
    };
    let outline_strength = strength(
        |params| volume_strengths(params).map(|(s, style)| s.outline_in(style)),
        |params, a| {
            if let Params::VolumeProfile { outline_alpha, .. } = params {
                *outline_alpha = Some(a);
            }
        },
    );
    let area_strength = strength(
        |params| volume_strengths(params).map(|(s, _)| s.value_area),
        |params, a| {
            if let Params::VolumeProfile { value_area_alpha, .. } = params {
                *value_area_alpha = Some(a);
            }
        },
    );
    let outside_strength = strength(
        |params| volume_strengths(params).map(|(s, _)| s.outside),
        |params, a| {
            if let Params::VolumeProfile { outside_alpha, .. } = params {
                *outside_alpha = Some(a);
            }
        },
    )
    .alone(profile_rows::OUTSIDE_TRANSPARENCY, profile_rows::OUTSIDE_TIP);
    let poc_strength = strength(
        |params| volume_strengths(params).map(|(s, _)| s.poc),
        |params, a| {
            if let Params::VolumeProfile { poc_alpha, .. } = params {
                *poc_alpha = Some(a);
            }
        },
    );
    let background = colour_row_with(
        owner,
        refresh,
        &unused,
        profile_rows::COLOUR,
        colour,
        indicator.color.clone(),
        move |indicator, choice| indicator.color = choice,
        id,
        Some(transparency),
    );
    let outline_row = colour_row_with(
        owner,
        refresh,
        &outline_ink,
        profile_rows::OUTLINE_COLOUR,
        &shown_outline,
        outline_color.clone(),
        move |indicator, choice| {
            if let Params::VolumeProfile { outline_color, .. } = &mut indicator.params {
                *outline_color = choice;
            }
        },
        id,
        Some(outline_strength),
    );

    // Drawn unless turned off, on either method.
    let (poc_switch, _) = {
        let owner = owner.clone();
        let refresh = refresh.clone();
        profile_rows::switch_row(profile_rows::SHOW, *poc, move |on| {
            update(&owner, id, |indicator| {
                if let Params::VolumeProfile { poc, .. } = &mut indicator.params {
                    *poc = on;
                }
            });
            refresh.run();
        })
    };
    // Unset is not "the same as the profile": it is the next colour along the
    // theme's sequence, which is the pair that sequence keeps far apart in hue.
    let shown = owner.theme().companion_or(poc_color.as_ref(), colour);
    let poc_colour_row = colour_row_with(
        owner,
        refresh,
        &poc_ink,
        profile_rows::COLOUR,
        &shown,
        poc_color.clone(),
        move |indicator, choice| {
            if let Params::VolumeProfile { poc_color, .. } = &mut indicator.params {
                *poc_color = choice;
            }
        },
        id,
        Some(poc_strength),
    );
    let poc_pattern = pattern_row(
        owner,
        refresh,
        &poc_ink,
        id,
        profile_rows::LINE_STYLE,
        *poc_style,
        |indicator, picked| {
            if let Params::VolumeProfile { poc_style, .. } = &mut indicator.params {
                *poc_style = picked;
            }
        },
    );
    let poc_width_row = thickness_row(
        owner,
        refresh,
        id,
        profile_rows::THICKNESS,
        *poc_width,
        |indicator, width| {
            if let Params::VolumeProfile { poc_width, .. } = &mut indicator.params {
                *poc_width = width;
            }
        },
    );

    let shape = choice_row(
        owner,
        refresh,
        id,
        profile_rows::DISPLAY_AS,
        &ProfileStyle::ALL.map(ProfileStyle::label),
        ProfileStyle::ALL.iter().position(|s| s == style),
        |indicator, at| {
            if let Params::VolumeProfile { style, .. } = &mut indicator.params {
                *style = ProfileStyle::ALL[at];
            }
        },
    );
    // The switch follows the style at once, both ways: on and greyed while
    // the outline is all there is, and back to what it was set to, and
    // free, the moment the rows are filled again. It used to wait for the
    // panel to be opened again, which left it greyed over a filled profile.
    {
        let switch = switch.clone();
        shape.connect_selected_notify(move |row| {
            let filled = ProfileStyle::ALL.get(row.selected() as usize) == Some(&ProfileStyle::Filled);
            let chosen = matches!(params_of(), Some(Params::VolumeProfile { outline: true, .. }));
            following.set(true);
            switch.set_active(chosen || !filled);
            following.set(false);
            switch.set_sensitive(filled);
            // The outline's strength is the edge's or the outline style's.
            follow();
        });
    }
    let alignment = choice_row(
        owner,
        refresh,
        id,
        profile_rows::ALIGNMENT,
        &Align::ALL.map(Align::label),
        Align::ALL.iter().position(|a| a == align),
        |indicator, at| {
            if let Params::VolumeProfile { align, .. } = &mut indicator.params {
                *align = Align::ALL[at];
            }
        },
    );
    // A share of the room the profile has: its period, or the empty space
    // past the last bar for the session drawn there.
    let (width, _) = {
        let owner = owner.clone();
        let refresh = refresh.clone();
        profile_rows::width_row(*width, move |share| {
            update(&owner, id, |indicator| {
                if let Params::VolumeProfile { width, .. } = &mut indicator.params {
                    *width = share;
                }
            });
            refresh.run();
        })
    };

    let (highlight_row, area_colour_row) =
        value_area_rows(owner, refresh, id, *highlight, value_area_color.clone(), area_strength);
    profile_rows::look_groups(profile_rows::LookRows {
        style: profile_rows::StyleRows {
            display: shape.upcast(),
            colour: background.upcast(),
            outline: Some(profile_rows::OutlineRows {
                switch,
                colour: outline_row.upcast(),
                thickness: width_row.upcast(),
                style: outline_pattern.upcast(),
            }),
            ends: None,
            alignment: alignment.upcast(),
            width: width.upcast(),
            expand_last: None,
        },
        value_area: profile_rows::ValueAreaRows {
            highlight: highlight_row,
            colour: area_colour_row.upcast(),
            outside: outside_strength.upcast(),
        },
        poc: profile_rows::PocRows {
            show: poc_switch,
            colour: poc_colour_row.upcast(),
            thickness: Some(poc_width_row.upcast()),
            style: Some(poc_pattern.upcast()),
        },
    })
}

/// How solid each part of a volume profile study is drawn, and its
/// display, which says which of its outlines is drawn.
fn volume_strengths(params: &Params) -> Option<(omacharts_engine::indicators::profile::Strengths, ProfileStyle)> {
    use omacharts_engine::indicators::profile::{drawn_alpha, Alphas, Strengths};
    let Params::VolumeProfile {
        alpha, opacity, value_area_alpha, outside_alpha, outline_alpha, poc_alpha, style, ..
    } = params
    else {
        return None;
    };
    let set = Alphas { value_area: *value_area_alpha, outside: *outside_alpha, outline: *outline_alpha, poc: *poc_alpha };
    Some((Strengths::study(drawn_alpha(*alpha, *opacity), &set), *style))
}

/// How solid each sort of a market profile's rows is drawn, as letters.
fn market_strengths(params: &Params) -> Option<market::Strengths> {
    let Params::MarketProfile { alpha, value_area_alpha, outside_alpha, poc_alpha, .. } = params else {
        return None;
    };
    Some(market::Strengths::of(*alpha, *value_area_alpha, *outside_alpha, *poc_alpha))
}

/// Rows shown again once what they follow has moved.
type Followers = Rc<RefCell<Vec<Rc<dyn Fn()>>>>;

/// The transparency of one of a profile study's parts, as `read` says how
/// solid it is drawn now, set apart by `write`, showing `moves` again as it
/// is moved; and the way to show it again once something it follows has
/// moved. It goes on the part's colour row, or on one of its own.
fn strength(
    owner: &Rc<Owner>,
    refresh: &Refresh,
    indicator: &Indicator,
    read: fn(&Params) -> Option<f64>,
    write: fn(&mut Params, f64),
    moves: Option<&Followers>,
) -> (property_rows::Transparency, Rc<dyn Fn()>) {
    let id = indicator.id;
    let (row, show) = {
        let owner = owner.clone();
        let refresh = refresh.clone();
        let moves = moves.cloned();
        profile_rows::strength(read(&indicator.params).unwrap_or(1.0), move |alpha| {
            update(&owner, id, |indicator| write(&mut indicator.params, alpha));
            for show in moves.iter().flat_map(|moves| moves.borrow().clone()) {
                show();
            }
            refresh.run();
        })
    };
    let owner = owner.clone();
    let again: Rc<dyn Fn()> = Rc::new(move || {
        let now = owner.indicators().into_iter().find(|i| i.id == id).and_then(|i| read(&i.params));
        if let Some(alpha) = now {
            show(alpha);
        }
    });
    (row, again)
}

/// A line's thickness in pixels, applied as it is spun.
fn thickness_row(
    owner: &Rc<Owner>,
    refresh: &Refresh,
    id: u32,
    title: &str,
    current: f64,
    apply: impl Fn(&mut Indicator, f64) + 'static,
) -> adw::SpinRow {
    let owner = owner.clone();
    let refresh = refresh.clone();
    profile_rows::thickness_row(title, current, move |width| {
        update(&owner, id, |indicator| apply(indicator, width));
        refresh.run();
    })
    .0
}

/// A row choosing one of a few named ways, applied as soon as it is picked.
fn choice_row(
    owner: &Rc<Owner>,
    refresh: &Refresh,
    id: u32,
    title: &str,
    names: &[&str],
    current: Option<usize>,
    apply: impl Fn(&mut Indicator, usize) + 'static,
) -> adw::ComboRow {
    let row = adw::ComboRow::new();
    row.set_title(title);
    row.set_model(Some(&gtk::StringList::new(names)));
    row.set_selected(current.unwrap_or(0) as u32);
    let owner = owner.clone();
    let refresh = refresh.clone();
    row.connect_selected_notify(move |row| {
        let at = row.selected() as usize;
        update(&owner, id, |indicator| apply(indicator, at));
        refresh.run();
    });
    row
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

        // Switch rows, as a drawn VWAP's bands have.
        let owner_for_show = owner.clone();
        let refresh_for_show = refresh.clone();
        let (show, _) = property_rows::switch_row(property_rows::SHOW, band.enabled, move |on| {
            update(&owner_for_show, id, move |indicator| {
                if let Some(band) =
                    indicator.params.bands_mut().and_then(|bands| bands.get_mut(index))
                {
                    band.enabled = on;
                }
            });
            refresh_for_show.run();
        });
        group.add(&show);

        // Tenths, because one, one and a half and two are the figures people
        // reach for, and six is further out than any band is drawn.
        let owner_for_dev = owner.clone();
        let refresh_for_dev = refresh.clone();
        let deviations = controls::bounded_row(
            "Standard deviations",
            None,
            band.deviations,
            omacharts_engine::drawings::MIN_VWAP_DEVIATIONS,
            omacharts_engine::drawings::MAX_VWAP_DEVIATIONS,
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

        let owner_for_fill = owner.clone();
        let refresh_for_fill = refresh.clone();
        let (shaded, _) = property_rows::switch_row(property_rows::SHADED, band.fill, move |on| {
            update(&owner_for_fill, id, move |indicator| {
                if let Some(band) =
                    indicator.params.bands_mut().and_then(|bands| bands.get_mut(index))
                {
                    band.fill = on;
                }
            });
            refresh_for_fill.run();
        });
        group.add(&shaded);

        // How much of the colour that shading gets, over the whole range the
        // figure has: clear at one end, solid at the other. How heavy a
        // backdrop wants to be is a matter of taste, and the shading is drawn
        // under the candles, so even a solid band has the bars on top of it.
        //
        // The ends come from the engine, as the pane height's do, so the
        // slider and the chart agree about where they are.
        let shading = indicator_strength(owner, refresh, id, band.alpha(), move |indicator, alpha| {
            if let Some(band) = indicator.params.bands_mut().and_then(|bands| bands.get_mut(index)) {
                band.fill_alpha = Some(alpha.clamp(MIN_FILL_ALPHA, MAX_FILL_ALPHA));
            }
        })
        .alone(property_rows::SHADING, property_rows::SHADING_TIP);
        group.add(&shading);

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

        let edge_strength = indicator_strength(owner, refresh, id, band.edge_alpha(), move |indicator, alpha| {
            if let Some(band) = indicator.params.bands_mut().and_then(|bands| bands.get_mut(index)) {
                band.line_alpha = Some(alpha);
            }
        });
        group.add(&colour_row_with(
            owner,
            refresh,
            &ink,
            property_rows::COLOUR,
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
            Some(edge_strength),
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
    reset_row_of(owner, refresh, id, Some(current), false)
}

/// The Reset every row, and with `never` a last choice of never starting
/// again, which `None` is: a cumulative delta's.
fn reset_row_of(
    owner: &Rc<Owner>,
    refresh: &Refresh,
    id: u32,
    current: Option<Reset>,
    never: bool,
) -> adw::ComboRow {
    let mut names: Vec<&str> = Reset::ALL.iter().map(|r| r.label()).collect();
    if never {
        names.push(omacharts_engine::indicators::cvd::NEVER);
    }
    let at = |reset: Option<Reset>| match reset {
        Some(reset) => Reset::ALL.iter().position(|r| *r == reset).unwrap_or(0),
        None => Reset::ALL.len(),
    };
    let row = adw::ComboRow::new();
    row.set_title("Reset every");
    row.set_model(Some(&gtk::StringList::new(&names)));
    row.set_selected(at(current) as u32);
    // A period the chart is too coarse for cannot be picked: it would be
    // drawn as another. Only for a chart's own indicators — a template's
    // are on charts of every resolution.
    if owner.template.is_none() {
        let timeframe = owner.host.timeframe();
        // The period being drawn, which a chosen one the chart is too coarse
        // for is not.
        let in_use = current.map(|current| current.effective_for(timeframe));
        row.set_selected(at(in_use) as u32);
        let factory = gtk::SignalListItemFactory::new();
        factory.connect_setup(|_, item| {
            let label = gtk::Label::new(None);
            label.set_xalign(0.0);
            if let Some(item) = item.downcast_ref::<gtk::ListItem>() {
                item.set_child(Some(&label));
            }
        });
        factory.connect_bind(move |_, item| {
            let Some(item) = item.downcast_ref::<gtk::ListItem>() else { return };
            let Some(label) = item.child().and_downcast::<gtk::Label>() else { return };
            let Some(period) = Reset::ALL.get(item.position() as usize).copied() else {
                label.set_text(omacharts_engine::indicators::cvd::NEVER);
                label.set_sensitive(true);
                item.set_selectable(true);
                item.set_activatable(true);
                return;
            };
            label.set_text(period.label());
            let possible = period.is_meaningful_for(timeframe);
            label.set_sensitive(possible);
            label.set_tooltip_text((!possible).then(|| {
                format!("Too few {} bars in a {} to draw it", timeframe.label(), period.label().to_lowercase())
            }).as_deref());
            item.set_selectable(possible);
            item.set_activatable(possible);
        });
        row.set_list_factory(Some(&factory));
    }

    let owner = owner.clone();
    let refresh = refresh.clone();
    row.connect_selected_notify(move |row| {
        let chosen = Reset::ALL.get(row.selected() as usize).copied();
        if chosen.is_none() && !never {
            return;
        }
        update(&owner, id, |indicator| match &mut indicator.params {
            Params::Vwap { reset, .. } => *reset = chosen.unwrap_or_default(),
            Params::VolumeProfile { reset, .. } | Params::MarketProfile { reset, .. } => {
                *reset = chosen.unwrap_or_default()
            }
            Params::Cvd { reset, .. } => *reset = chosen,
            Params::MovingAverage { .. }
            | Params::Bollinger { .. }
            | Params::Volume { .. }
            | Params::VolumeDelta { .. }
            | Params::Footprint { .. }
            | Params::LargeOrders { .. }
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
        move |indicator, share| {
            if let Some(height) = indicator.params.height_mut() {
                *height = share;
            }
        },
    )
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
pub fn add_indicator(host: Rc<dyn Host>) {
    pick_indicator(&Owner::new(host), &Refresh::none());
}

/// The indicators on `host`'s chart, in a dialog of their own: the list,
/// a + that adds one, and each one's settings a page pushed over it.
pub fn present(host: Rc<dyn Host>) {
    open(host, None);
}

/// Straight to indicator `id`'s settings, over the list: the gear beside it
/// in the legend.
pub fn present_indicator(host: Rc<dyn Host>, id: u32) {
    open(host, Some(id));
}

fn open(host: Rc<dyn Host>, focus: Option<u32>) {
    let owner = Owner::new(host);
    let dialog = adw::PreferencesDialog::new();
    dialog.set_title("Indicators");
    dialog.set_content_width(crate::dialogs::SETTINGS_WIDTH);
    dialog.set_content_height((owner.window.height() * 85 / 100).max(620));
    let page = adw::PreferencesPage::new();
    page.set_title("Indicators");
    page.set_icon_name(Some("view-list-symbolic"));
    dialog.add(&page);
    let list = fill_page(&owner, &dialog, &page, "On the chart");
    dialog.present(Some(&owner.window));
    if let Some(id) = focus {
        open_indicator_panel_for(&owner, &Refresh::rebuilding(&owner, &dialog, &list), id, Panel::Subpage(dialog.clone()));
    }
}

/// The indicator list on `page`, titled `title`, with its + wired to the
/// picker, read again whenever the chart changes while `dialog` is open.
pub fn fill_page(owner: &Rc<Owner>, dialog: &adw::PreferencesDialog, page: &adw::PreferencesPage, title: &str) -> IndicatorList {
    let list = IndicatorList::new(page, title);
    // Wired once here rather than on every rebuild: the button outlives
    // the rows, and reconnecting it each time would stack up handlers.
    let owner_for_add = owner.clone();
    let refresh_for_add = Refresh::rebuilding(owner, dialog, &list);
    list.add.connect_clicked(move |_| {
        pick_indicator(&owner_for_add, &refresh_for_add);
    });
    rebuild_indicators(owner, dialog, &list);
    // Something else can add, change or take away an indicator while the
    // list is open, and the list reads it again.
    let weak = Rc::downgrade(owner);
    let dialog_weak = dialog.downgrade();
    let watched = list.clone();
    owner.watch_chart(move || {
        let (Some(owner), Some(dialog)) = (weak.upgrade(), dialog_weak.upgrade()) else {
            return false;
        };
        if dialog.root().is_none() {
            return false;
        }
        rebuild_indicators(&owner, &dialog, &watched);
        true
    });
    list
}

pub fn pick_indicator(owner: &Rc<Owner>, refresh: &Refresh) {
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

    // What each row adds, in order.
    let shown: Rc<RefCell<Vec<Indicator>>> = Rc::default();
    // Whether the query gave them their numbers ("sma 50"), which makes them
    // set up already.
    let numbered = Rc::new(std::cell::Cell::new(false));
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
            let picks = shown.borrow();
            let category = |row: &gtk::ListBoxRow| {
                picks.get(row.index().max(0) as usize).map(|pick| pick.kind.category())
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
        let numbered = numbered.clone();
        move |query: &str| {
            while let Some(child) = rows.first_child() {
                rows.remove(&child);
            }
            // Numbers after the name are its lengths: the rows are then the
            // kinds that take them.
            let (name, numbers) = indicators::split_query(query);
            let browsing = name.is_empty() && numbers.is_empty();
            grouped.set(browsing);
            let kinds: Vec<Kind> = match browsing {
                true => indicators::Category::ALL.iter().flat_map(|c| c.kinds()).collect(),
                false => indicators::search(name),
            };
            numbered.set(!numbers.is_empty());
            // Filled before the rows, which the header function reads.
            *shown.borrow_mut() =
                kinds.into_iter().filter_map(|kind| Indicator::numbered(0, kind, &numbers)).collect();
            for pick in shown.borrow().iter() {
                let kind = pick.kind;
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
                // is the name itself, it would be saying it twice. With
                // numbers, it is the label they make, which is the only place
                // they show before Enter.
                let chip = match numbered.get() {
                    true => Some(pick.label()),
                    false => (!kind.short_name().eq_ignore_ascii_case(kind.name()))
                        .then(|| kind.short_name().to_string()),
                };
                if let Some(chip) = chip {
                    let short = gtk::Label::new(Some(&chip));
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
        let Some(picked) = shown.borrow().get(at).cloned() else { return };
        // A kind a chart holds one of, asked for again: the one there is
        // opened, rather than a second put down that would never be drawn.
        if let Some(there) = there_already(&owner_for_pick.indicators(), picked.kind) {
            dialog_for_pick.close();
            open_indicator_panel_for(&owner_for_pick, &refresh_for_pick, there, Panel::Alone);
            return;
        }
        let id = owner_for_pick.next_indicator_id();
        let mut indicators = owner_for_pick.indicators();
        indicators.push(Indicator { id, ..picked });
        owner_for_pick.set_indicators(indicators);
        refresh_for_pick.run();
        dialog_for_pick.close();
        // Typed with its numbers, it is set up already: they said what was
        // meant.
        if numbered.get() {
            return;
        }
        // Straight into its settings: picking the kind and setting it up are
        // two steps, and the second is where you say whether you meant it.
        open_indicator_panel_for(&owner_for_pick, &refresh_for_pick, id, Panel::Add);
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


/// The one indicator of `kind` among `indicators`, for a kind a chart holds
/// at most one of.
pub fn there_already(indicators: &[Indicator], kind: Kind) -> Option<u32> {
    kind.one_per_chart().then(|| indicators.iter().find(|i| i.kind == kind).map(|i| i.id)).flatten()
}

#[cfg(test)]
mod tests {
    use super::*;
    use omacharts_engine::indicators::profile::{Method, DELTA_COLOURINGS};

    // GTK objects are not built in here. Starting GTK is something one test in
    // the suite may do, from its own thread, and a second one asking would
    // take the whole run down with it — so these ask the arithmetic the rows
    // are built from instead.

    /// The method row offers both, the total first as the default, and
    /// only the total has a value area to set.
    #[test]
    fn a_profile_offers_its_methods_and_a_value_area_only_for_the_total() {
        assert_eq!(Method::ALL.map(Method::label), ["Total volume", "Ask volume - bid volume"]);
        assert_eq!(Method::ALL[0], Method::default());
        assert!(profile_rows::has_value_area(Method::Total));
        assert!(!profile_rows::has_value_area(Method::Delta));
    }

    /// A delta offers the colourings that tell a rise from a fall, by the
    /// names a chart's own colouring row gives them, after the default;
    /// a total offers none.
    #[test]
    fn a_delta_profile_offers_the_two_toned_colourings_after_the_default() {
        let names: Vec<&str> = DELTA_COLOURINGS.iter().map(|c| c.label()).collect();
        assert_eq!(names, ["Vivid", "Soft", "Quiet", "Monochrome"]);
        assert!(names.iter().all(|name| BarColouring::ALL.iter().any(|c| c.label() == *name)));
        assert_eq!(profile_rows::FOLLOWS_DEFAULTS, "Default");
        assert!(profile_rows::has_colouring(Method::Delta));
        assert!(!profile_rows::has_colouring(Method::Total));
    }

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

    /// The kinds a chart draws one of are found to be changed rather than
    /// added again; anything else is added beside what is there.
    #[test]
    fn only_one_footprint_or_session_times_is_there_already() {
        use omacharts_engine::indicators::Kind;
        let held = [Indicator::new(3, Kind::Sma), Indicator::new(4, Kind::Footprint), Indicator::new(5, Kind::SessionTimes)];
        assert_eq!(super::there_already(&held, Kind::Footprint), Some(4));
        assert_eq!(super::there_already(&held, Kind::SessionTimes), Some(5));
        assert_eq!(super::there_already(&held, Kind::Sma), None, "as many as wanted");
        assert_eq!(super::there_already(&held[..1], Kind::Footprint), None);
    }
}
