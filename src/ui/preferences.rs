//! Settings.
//!
//! A theme is every colour the app wears, candles included; once it is yours
//! to edit, every one of them is a row on the Theme page. How candles are
//! painted from those colours — vivid, soft, quiet, monochrome — is a
//! chart setting, and lives under Charts.
//!
//! Built-ins and the Omarchy theme are not editable in place. "Customize"
//! makes a copy you own, which is both simpler to reason about and means a
//! fiddled palette can always be abandoned by switching back.

use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;
use gtk::{gio, glib};
use omacharts_engine::{BarColouring, BarStyle, Session};
use omacharts_engine::theme::{
    theme_bars, BarSlot, UiSlot, FALLBACK_THEME_ID, OMARCHY_ID, SWATCH_NAMES,
};

use crate::backup;
use crate::cache;
use crate::chart_defaults::ChartDefaults;
use crate::store::Store;
use crate::theming::Theming;
use crate::ui::colors;
use crate::ui::screenshot;
use crate::ui::window::Window;

pub struct Preferences;

/// What opens the dialog at the Trading section rather than at a tab of
/// Charts.
pub const TRADING: &str = "trading";

/// The sidebar plus the column a preferences page lays its rows in, with
/// room either side of it.
const MAX_WIDTH: i32 = 980;

/// Where the dialog was when it was last looked at, for this run of the app:
/// the section down the sidebar, by its place, and the tab shown in Charts.
/// Opening the dialog again goes back there, so somebody adjusting one row
/// over and over does not have to find it each time. Not kept across
/// restarts.
#[derive(Clone, Default)]
struct Seen {
    section: usize,
    charts_tab: Option<String>,
}

thread_local! {
    static LAST_SEEN: RefCell<Seen> = RefCell::new(Seen::default());
}

/// Have the dialog open next at the Theme section, for a test that reads
/// what is on it.
#[cfg(test)]
pub(crate) fn open_next_at_theme() {
    LAST_SEEN.with(|seen| seen.borrow_mut().section = 2);
}

struct Context {
    store: Rc<Store>,
    theming: Rc<RefCell<Theming>>,
    on_change: Rc<dyn Fn()>,
    /// Opens the window's own resolution editor. The list belongs to the
    /// window — every chart offers the same resolutions — so it is edited
    /// there and merely reached from here.
    on_edit_resolutions: Rc<dyn Fn()>,
    /// Has the window read back everything a restored backup changed.
    on_restored: Rc<dyn Fn()>,
    page: adw::PreferencesPage,
    general_page: adw::PreferencesPage,
    charts_page: adw::PreferencesPage,
    charts_other_page: adw::PreferencesPage,
    charts_templates_page: adw::PreferencesPage,
    data_page: adw::PreferencesPage,
    trading_page: adw::PreferencesPage,
    groups: RefCell<Vec<adw::PreferencesGroup>>,
    /// The window, for the chart templates: they are shown on its charts,
    /// and only it knows which of its charts are using one.
    window: Rc<Window>,
    /// The list on the Templates tab, rebuilt whenever one changes.
    templates_group: RefCell<Option<adw::PreferencesGroup>>,
    /// Puts the Defaults tab's template choice back in step with the list,
    /// after a template is made, renamed or deleted on the tab beside it.
    refill_default_template: RefCell<Option<Rc<dyn Fn()>>>,
}

impl Preferences {
    #[allow(clippy::too_many_arguments)]
    pub fn present(
        parent: &impl IsA<gtk::Widget>,
        store: Rc<Store>,
        theming: Rc<RefCell<Theming>>,
        on_change: Rc<dyn Fn()>,
        on_edit_resolutions: Rc<dyn Fn()>,
        on_restored: Rc<dyn Fn()>,
        window: Rc<Window>,
        charts_tab: Option<&str>,
    ) {
        // Trading is a section, not a tab of Charts, and is asked for by the
        // same word: from a chart's account list.
        let (charts_tab, at_trading) = match charts_tab {
            Some(TRADING) => (None, true),
            tab => (tab, false),
        };
        let dialog = adw::Dialog::new();
        dialog.set_title("Preferences");
        // The breakpoint below needs a floor to measure against.
        dialog.set_size_request(360, 360);
        // Most of the window's height: the theme page is a long list of
        // colours, and every row that fits is a scroll saved. Not its width:
        // the rows sit in a column of their own, and past the sidebar and that
        // column a wider dialog is only margin.
        let parent = parent.as_ref();
        dialog.set_content_width((parent.width() * 85 / 100).clamp(560, MAX_WIDTH));
        dialog.set_content_height((parent.height() * 85 / 100).max(640));

        let page = adw::PreferencesPage::new();
        let general_page = adw::PreferencesPage::new();
        let charts_page = adw::PreferencesPage::new();
        let data_page = adw::PreferencesPage::new();
        let trading_page = adw::PreferencesPage::new();

        // General first: what the app does, before what it looks like.
        // Charts is four things: what a new chart opens with, the sets of
        // indicators charts can share, the line every chart's header shows,
        // and the rest.
        let charts_other_page = adw::PreferencesPage::new();
        let charts_templates_page = adw::PreferencesPage::new();
        let charts = gtk::Stack::new();
        charts.add_titled(&charts_page, Some("defaults"), "Defaults");
        charts.add_titled(&charts_templates_page, Some("templates"), "Templates");
        // What every chart's header shows, changed on all of them as it is
        // changed here.
        let charts_header_page = adw::PreferencesPage::new();
        let store_for_header = store.clone();
        let window_for_header = window.clone();
        // The account, Buy and Sell, the quantity and the day's P&L are
        // listed and offered while trading is switched on, with or without
        // a login, like every trading setting: they draw nothing on a chart
        // until there is an account to trade on. Switched off, they are
        // neither listed nor offered, and stay where they were.
        let (sections, restore_header) = header_sections(crate::header::load(&store), move |header| {
            crate::header::save(&store_for_header, header);
            window_for_header.apply_header();
        });
        for section in &sections {
            charts_header_page.add(&section.group);
        }
        let restore_group = adw::PreferencesGroup::new();
        let restore = gtk::Button::with_label("Restore defaults");
        restore.set_halign(gtk::Align::Start);
        restore.connect_clicked(move |_| restore_header());
        restore_group.add(&restore);
        charts_header_page.add(&restore_group);
        charts.add_titled(&charts_header_page, Some("header"), "Header");
        charts.add_titled(&charts_other_page, Some("other"), "Other");
        // Opened from something on a chart, at the tab that is about it;
        // otherwise where it was left.
        let last = LAST_SEEN.with(|seen| seen.borrow().clone());
        if let Some(tab) = charts_tab.map(str::to_string).or(last.charts_tab) {
            charts.set_visible_child_name(&tab);
        }
        charts.connect_visible_child_name_notify(|charts| {
            let tab = charts.visible_child_name().map(|name| name.to_string());
            LAST_SEEN.with(|seen| seen.borrow_mut().charts_tab = tab);
        });

        let mut sections: Vec<(&str, &str, gtk::Widget)> = vec![
            ("General", "preferences-system-symbolic", general_page.clone().upcast()),
            ("Charts", "utilities-system-monitor-symbolic", charts.upcast()),
            ("Theme", "applications-graphics-symbolic", page.clone().upcast()),
            ("Data", "network-server-symbolic", data_page.clone().upcast()),
        ];
        // Trading's page only while trading is switched on.
        let trading = crate::features::trading();
        if trading {
            sections.push(("Trading", "view-list-bullet-symbolic", trading_page.clone().upcast()));
        }
        let start = match (charts_tab, at_trading && trading) {
            (_, true) => sections.len() - 1,
            (Some(_), _) => 1,
            (None, _) => last.section.min(sections.len() - 1),
        };
        dialog.set_child(Some(&split_view(&dialog, &sections, start)));

        let context = Rc::new(Context {
            store,
            theming,
            on_change,
            on_edit_resolutions,
            on_restored,
            page,
            general_page,
            charts_page,
            charts_other_page,
            charts_templates_page,
            data_page,
            trading_page,
            groups: RefCell::new(Vec::new()),
            window,
            templates_group: RefCell::new(None),
            refill_default_template: RefCell::new(None),
        });
        rebuild(&context);
        build_general_page(&context);

        dialog.present(Some(parent));
    }
}

/// The header's sections, a list each, handing every change to `changed` as
/// the whole header.
///
/// A part is in the header once, so a part in one section is not offered by
/// the other's +, and is offered again the moment it is taken out.
///
/// Comes with the way back to the standard header, every section at once.
#[allow(clippy::type_complexity)]
fn header_sections(
    current: crate::header::Header,
    changed: impl Fn(&crate::header::Header) + 'static,
) -> (Vec<Rc<crate::ui::part_list::PartList<crate::header::Part>>>, Rc<dyn Fn()>) {
    use crate::header::Section;
    use crate::ui::part_list::PartList;
    type Lists = Rc<RefCell<Vec<(Section, std::rc::Weak<PartList<crate::header::Part>>)>>>;
    let header = Rc::new(RefCell::new(current));
    let changed = Rc::new(changed);
    let lists: Lists = Rc::new(RefCell::new(Vec::new()));
    let elsewhere = Rc::new(move |header: &crate::header::Header, section: Section| -> Vec<crate::header::Part> {
        Section::ALL
            .into_iter()
            .filter(|other| *other != section)
            .flat_map(|other| header.section(other).to_vec())
            .collect()
    });
    let built: Vec<_> = Section::ALL
        .into_iter()
        .map(|section| {
            let (edited, changed, others) = (header.clone(), changed.clone(), lists.clone());
            let elsewhere_now = elsewhere.clone();
            // Trading's parts while it is switched off are neither listed
            // nor lost: they stay where they were for when it is on.
            let current = header.borrow().shown(section, crate::features::trading());
            let list = PartList::new(
                section.title(),
                &format!("Add to the {}", section.title().to_lowercase()),
                current,
                move |parts: Vec<crate::header::Part>| {
                    edited.borrow_mut().set_shown(section, parts, crate::features::trading());
                    let now = edited.borrow().clone();
                    for (other, list) in others.borrow().iter().filter(|(other, _)| *other != section) {
                        if let Some(list) = list.upgrade() {
                            list.set_elsewhere(elsewhere_now(&now, *other));
                        }
                    }
                    changed(&now);
                },
            );
            list.set_elsewhere(elsewhere(&header.borrow(), section));
            lists.borrow_mut().push((section, Rc::downgrade(&list)));
            list
        })
        .collect();
    // Kept by their groups, which let go when the dialog is disposed: a
    // section with no rows and nothing to offer holds nothing that would.
    for list in &built {
        let kept = built.clone();
        list.group.connect_destroy(move |_| {
            let _ = &kept;
        });
    }
    let shown: Vec<_> = built.iter().map(Rc::downgrade).collect();
    let restore = Rc::new(move || {
        let standard = crate::header::Header::standard();
        *header.borrow_mut() = standard.clone();
        for (section, list) in Section::ALL.into_iter().zip(&shown) {
            if let Some(list) = list.upgrade() {
                list.replace(standard.shown(section, crate::features::trading()));
                list.set_elsewhere(elsewhere(&standard, section));
            }
        }
        changed(&standard);
    });
    (built, restore)
}

/// The sections down a sidebar, the selected one beside it.
///
/// A sidebar rather than the preferences dialog's own view switcher, because
/// the dialog fills most of the window and a switcher across its top leaves
/// that width to margins.
fn split_view(
    dialog: &adw::Dialog,
    sections: &[(&str, &str, gtk::Widget)],
    start: usize,
) -> adw::NavigationSplitView {
    let content = adw::NavigationView::new();
    // A section with tabs of its own shows them in its header, where the
    // title would be.
    let section_page = |(title, _, page): &(&str, &str, gtk::Widget)| {
        let header = adw::HeaderBar::new();
        // Words alone: a switcher that insists on an icon per tab draws a
        // placeholder for every tab that has none.
        if let Some(stack) = page.downcast_ref::<gtk::Stack>() {
            let switcher = gtk::StackSwitcher::new();
            switcher.set_stack(Some(stack));
            header.set_title_widget(Some(&switcher));
        }
        let toolbar = adw::ToolbarView::new();
        toolbar.add_top_bar(&header);
        toolbar.set_content(Some(page));
        adw::NavigationPage::new(&toolbar, title)
    };

    let list = gtk::ListBox::new();
    list.add_css_class("navigation-sidebar");
    for (title, icon, _) in sections {
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        row.append(&gtk::Image::from_icon_name(icon));
        row.append(&gtk::Label::new(Some(title)));
        list.append(&row);
    }

    let sidebar = adw::ToolbarView::new();
    sidebar.add_top_bar(&adw::HeaderBar::new());
    let scroller = gtk::ScrolledWindow::new();
    scroller.set_hscrollbar_policy(gtk::PolicyType::Never);
    scroller.set_child(Some(&list));
    sidebar.set_content(Some(&scroller));

    let split = adw::NavigationSplitView::new();
    split.set_sidebar(Some(&adw::NavigationPage::new(&sidebar, "Preferences")));
    split.set_content(Some(&adw::NavigationPage::new(&content, "")));

    let pages: Vec<adw::NavigationPage> = sections.iter().map(section_page).collect();
    let view = split.clone();
    list.connect_row_selected(move |_, row| {
        let Some(page) = row.and_then(|row| pages.get(row.index() as usize)) else {
            return;
        };
        if let Some(row) = row {
            LAST_SEEN.with(|seen| seen.borrow_mut().section = row.index() as usize);
        }
        content.replace(std::slice::from_ref(page));
        view.set_show_content(true);
    });
    list.select_row(list.row_at_index(start as i32).as_ref());

    // Too narrow for both side by side, the sidebar becomes a page of its
    // own and a section is pushed over it.
    let narrow = adw::Breakpoint::new(adw::BreakpointCondition::new_length(
        adw::BreakpointConditionLengthType::MaxWidth,
        560.0,
        adw::LengthUnit::Sp,
    ));
    narrow.add_setter(&split, "collapsed", Some(&true.to_value()));
    dialog.add_breakpoint(narrow);

    split
}

/// Clear and re-add the appearance groups.
///
/// Called whenever the selection changes, because which editors belong on the
/// page depends on whether the selected theme is editable.
fn rebuild(context: &Rc<Context>) {
    for group in context.groups.borrow_mut().drain(..) {
        context.page.remove(&group);
    }
    let mut groups = Vec::new();

    groups.push(theme_group(context));
    let theme = context.theming.borrow().theme();
    if theme.source.is_editable() {
        let [backgrounds, text, other] = UiSlot::GROUPS;
        groups.push(bar_colors_group(context));
        if crate::features::trading() {
            groups.push(trading_look_group(context));
        }
        groups.push(theme_colors_group(context, backgrounds));
        groups.push(theme_colors_group(context, text));
        groups.push(palette_group(context));
        groups.push(theme_colors_group(context, other));
    }

    for group in &groups {
        context.page.add(group);
    }
    *context.groups.borrow_mut() = groups;
}

/// A way to the resolution list from the settings, since the other way in is
/// right-clicking a strip, which you have to know about to find; the one
/// switch about how the charts behave towards each other; and how far they
/// zoom to the wheel and the touchpad.
fn chart_group(context: &Rc<Context>) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::new();
    group.set_title("Chart");

    let row = adw::ActionRow::new();
    row.set_title("Preset resolutions");
    row.set_activatable(true);

    let button = gtk::Button::with_label("Edit…");
    button.set_valign(gtk::Align::Center);
    let edit = context.on_edit_resolutions.clone();
    button.connect_clicked(move |_| edit());
    row.add_suffix(&button);

    let edit = context.on_edit_resolutions.clone();
    row.connect_activated(move |_| edit());

    group.add(&row);

    // One switch for the app rather than one per chart, and nothing to do
    // with how a chart is linked to the watchlist: two charts of the same
    // symbol are two looks at the same moment, whichever list moves them.
    let sync = adw::ActionRow::new();
    sync.set_title("Sync crosshair across charts");
    let switch = gtk::Switch::new();
    switch.set_valign(gtk::Align::Center);
    switch.set_active(context.store.setting_bool(crate::ui::window::SETTING_SYNC_CROSSHAIR, true));
    let store = context.store.clone();
    let window = context.window.clone();
    switch.connect_state_set(move |_, on| {
        store.set_setting_bool(crate::ui::window::SETTING_SYNC_CROSSHAIR, on);
        if !on {
            window.clear_echoes();
        }
        glib::Propagation::Proceed
    });
    sync.add_suffix(&switch);
    sync.set_activatable_widget(Some(&switch));
    group.add(&sync);

    group.add(&zoom_row(context));

    group
}

/// How far the wheel and a two-finger swipe zoom, for every chart, as a
/// percentage of the base rate. Read by the charts on every scroll, so a
/// change is felt on the next one. The mark is where it started.
fn zoom_row(context: &Rc<Context>) -> adw::ActionRow {
    use crate::zoom::{DEFAULT, MAX, MIN};
    let row = adw::ActionRow::new();
    row.set_title("Zoom sensitivity");
    let scale = gtk::Scale::with_range(gtk::Orientation::Horizontal, MIN as f64, MAX as f64, 5.0);
    scale.set_size_request(180, -1);
    scale.set_valign(gtk::Align::Center);
    scale.set_draw_value(true);
    scale.set_value_pos(gtk::PositionType::Left);
    scale.set_digits(0);
    scale.set_round_digits(0);
    scale.set_format_value_func(|_, v| format!("{v:.0}%"));
    scale.add_mark(DEFAULT as f64, gtk::PositionType::Bottom, None);
    scale.set_value(crate::zoom::sensitivity(&context.store) as f64);
    let store = context.store.clone();
    scale.connect_value_changed(move |scale| {
        let percent = scale.value().round() as u32;
        if percent != crate::zoom::sensitivity(&store) {
            crate::zoom::save(&store, percent);
        }
    });
    row.add_suffix(&scale);
    row
}

fn theme_group(context: &Rc<Context>) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::new();
    let theming = context.theming.borrow();

    // System first, then the themes somebody made, then the ones that ship,
    // each run set apart from the next by a line.
    let system = omacharts_engine::theme::OMARCHY_ID;
    let themes = theming.themes();
    let mut ordered: Vec<&_> = themes.iter().filter(|t| t.id == system).collect();
    // Off Omarchy there is no System to set apart.
    let made_from = ordered.len();
    ordered.extend(themes.iter().filter(|t| t.id != system && t.source.is_editable()));
    let shipped_from = ordered.len();
    ordered.extend(themes.iter().filter(|t| t.id != system && !t.source.is_editable()));
    let ids: Vec<String> = ordered.iter().map(|t| t.id.clone()).collect();
    // The entry that tracks the desktop is "System", the way every other
    // setting on this platform spells it. Labelled with the desktop theme's
    // own name it was indistinguishable from a fixed preset, and the
    // difference between them is the whole point: one keeps changing.
    let names: Vec<String> = ordered
        .iter()
        .map(|t| if t.id == system { "System".to_string() } else { t.name.clone() })
        .collect();
    let splits = vec![made_from, shipped_from];
    let following = theming.theme().name.clone();
    let is_system = theming.theme_id() == omacharts_engine::theme::OMARCHY_ID;
    let selected = ids.iter().position(|id| id == theming.theme_id()).unwrap_or(0);
    let source = theming.theme().source;
    drop(theming);

    let row = adw::ComboRow::new();
    row.set_title("Theme");
    // System is the one choice whose colours are not its name, so it says
    // which desktop theme it is wearing.
    if is_system {
        row.set_subtitle(&format!("Following {following}"));
    }
    row.set_model(Some(&string_list(&names)));
    row.set_list_factory(Some(&separated_list(splits)));
    row.set_selected(selected as u32);

    let ctx = context.clone();
    let choices = ids.clone();
    row.connect_selected_notify(move |row| {
        let Some(id) = choices.get(row.selected() as usize) else { return };
        ctx.theming.borrow_mut().select_theme(id, &ctx.store);
        apply(&ctx);
        rebuild(&ctx);
    });
    group.add(&row);

    if source.is_editable() {
        group.add(&own_theme_actions(context));
    } else {
        group.add(&customize_row(context));
    }
    group
}

/// One edit to the defaults, as the rows on the Charts page make them.
type Change = dyn Fn(&dyn Fn(&mut ChartDefaults));

/// What a new chart opens with, and a way to give it to every chart there is.
///
/// Changing a row changes only what the next chart opens with: charts already
/// open have settings of their own, and a preference that reached into them
/// unasked would undo whatever was set on each. Apply to all is the asking.
fn build_charts_page(context: &Rc<Context>) {
    let defaults = Rc::new(std::cell::Cell::new(ChartDefaults::load(&context.store)));

    // Grouped as a chart's own settings are, so a row is found in the same
    // place in both.
    let bars = adw::PreferencesGroup::new();
    bars.set_title("Bars");
    let scales = adw::PreferencesGroup::new();
    scales.set_title("Scales");
    let hours = adw::PreferencesGroup::new();
    hours.set_title("Session");
    let drawings = adw::PreferencesGroup::new();
    drawings.set_title("Drawings");

    // Once something has changed: what that did, and the offer to take it
    // further than new charts, as one line under the last group. Faded rather
    // than removed while there is nothing to say, so nothing moves when it
    // appears.
    let footer = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    footer.set_halign(gtk::Align::End);
    footer.set_margin_top(36);
    let note = gtk::Label::new(Some("New charts will use these options."));
    note.add_css_class("dim-label");
    let apply = gtk::Button::with_label("Apply to all charts now");
    footer.append(&note);
    footer.append(&apply);
    let offer = |line: &gtk::Box, shown: bool| {
        line.set_opacity(if shown { 1.0 } else { 0.0 });
        line.set_sensitive(shown);
        line.set_can_target(shown);
    };
    offer(&footer, false);
    let window = context.window.clone();
    let line = footer.clone();
    apply.connect_clicked(move |_| {
        let line = line.clone();
        window.apply_chart_defaults(move || offer(&line, false));
    });

    // Every row writes through here: the defaults are saved as they change,
    // and the button to take them further appears.
    let changed: Rc<Change> = {
        let store = context.store.clone();
        let defaults = defaults.clone();
        let footer = footer.clone();
        Rc::new(move |change| {
            let mut next = defaults.get();
            change(&mut next);
            // Rows announce their value as they are drawn, too; only a real
            // change is something to apply.
            if next == defaults.get() {
                return;
            }
            next.save(&store);
            defaults.set(next);
            offer(&footer, true);
        })
    };

    let current = defaults.get();
    let style = adw::ComboRow::new();
    style.set_title("Style");
    let names: Vec<&str> = BarStyle::ALL.iter().map(|s| s.label()).collect();
    style.set_model(Some(&string_list(&names.iter().map(|n| n.to_string()).collect::<Vec<_>>())));
    style.set_selected(BarStyle::ALL.iter().position(|s| *s == current.bar_style).unwrap_or(0) as u32);

    let colouring = adw::ComboRow::new();
    colouring.set_title("Colouring");
    crate::ui::chart_settings::fill_colourings(&colouring, current.colouring);

    // The two switches worn over the colouring: the hollow rise greyed on
    // OHLC bars, which have no body to leave open, and the swap on a
    // colouring that paints the same either way.
    let hollow = adw::SwitchRow::new();
    hollow.set_title(crate::ui::chart_settings::HOLLOW_LABEL);
    hollow.set_active(current.hollow_up);
    hollow.set_sensitive(crate::ui::chart_settings::hollow_applies(current.bar_style));
    let swapped = adw::SwitchRow::new();
    swapped.set_title(crate::ui::chart_settings::SWAP_LABEL);
    swapped.set_active(current.swapped);
    let theme = context.theming.borrow().theme();
    swapped.set_sensitive(crate::ui::chart_settings::two_toned(&theme, current.colouring));

    // How thick OHLC bars are drawn, offered only while they are the style.
    let thickness = {
        let changed = changed.clone();
        crate::ui::controls::bounded_row(
            crate::ui::chart_settings::THICKNESS_LABEL,
            None,
            f64::from(current.ohlc_thickness),
            f64::from(crate::chart_defaults::MIN_OHLC_THICKNESS),
            f64::from(crate::chart_defaults::MAX_OHLC_THICKNESS),
            1.0,
            0,
            move |px| {
                let px = px.round() as u8;
                changed(&|d| d.ohlc_thickness = px);
            },
        )
        .row
    };
    thickness.set_visible(current.bar_style == BarStyle::Ohlc);
    {
        let changed = changed.clone();
        let hollow = hollow.clone();
        let thickness = thickness.clone();
        style.connect_selected_notify(move |row| {
            let Some(chosen) = BarStyle::ALL.get(row.selected() as usize).copied() else { return };
            changed(&|d| d.bar_style = chosen);
            hollow.set_sensitive(crate::ui::chart_settings::hollow_applies(chosen));
            thickness.set_visible(chosen == BarStyle::Ohlc);
        });
    }
    {
        let changed = changed.clone();
        let defaults = defaults.clone();
        let swapped = swapped.clone();
        colouring.connect_selected_notify(move |row| {
            if crate::ui::chart_settings::filling_colourings(row) {
                return;
            }
            let Some(chosen) = BarColouring::ALL.get(row.selected() as usize).copied() else {
                return;
            };
            if chosen != defaults.get().colouring {
                changed(&|d| d.colouring = chosen);
            }
            swapped.set_sensitive(crate::ui::chart_settings::two_toned(&theme, chosen));
        });
    }
    {
        let changed = changed.clone();
        hollow.connect_active_notify(move |row| {
            let on = row.is_active();
            changed(&|d| d.hollow_up = on);
        });
    }
    {
        let changed = changed.clone();
        swapped.connect_active_notify(move |row| {
            let on = row.is_active();
            changed(&|d| d.swapped = on);
        });
    }
    bars.add(&style);
    bars.add(&thickness);
    bars.add(&colouring);
    bars.add(&hollow);
    bars.add(&swapped);

    let session = adw::ComboRow::new();
    session.set_title("Hours");
    let sessions: Vec<String> = Session::ALL.iter().map(|s| s.label().to_string()).collect();
    session.set_model(Some(&string_list(&sessions)));
    session.set_selected(Session::ALL.iter().position(|s| *s == current.session).unwrap_or(0) as u32);
    {
        let changed = changed.clone();
        session.connect_selected_notify(move |row| {
            let Some(chosen) = Session::ALL.get(row.selected() as usize).copied() else { return };
            changed(&|d| d.session = chosen);
        });
    }
    hours.add(&session);

    let switch = |title: &str, on: bool, set: fn(&mut ChartDefaults, bool)| {
        let row = adw::SwitchRow::new();
        row.set_title(title);
        row.set_active(on);
        let changed = changed.clone();
        row.connect_active_notify(move |row| {
            let on = row.is_active();
            changed(&|d| set(d, on));
        });
        row
    };
    scales.add(&switch("Logarithmic scale", current.log_scale, |d, on| d.log_scale = on));
    scales.add(&switch("Gridlines", current.show_grid, |d, on| d.show_grid = on));
    drawings.add(&switch("Show drawings from other charts", current.shows_drawings, |d, on| {
        d.shows_drawings = on
    }));
    drawings.add(&switch("Send drawings to other charts", current.sends_drawings, |d, on| {
        d.sends_drawings = on
    }));

    // Which template a new chart shows, as a choice like the others: "None"
    // first, then every template. Refilled when the Templates tab changes
    // the list, which is why the ids are kept beside the row rather than
    // captured once.
    let indicators = adw::PreferencesGroup::new();
    indicators.set_title("Indicators");
    let template = adw::ComboRow::new();
    template.set_title("Template");
    let ids: Rc<RefCell<Vec<u32>>> = Rc::new(RefCell::new(Vec::new()));
    let filling = Rc::new(std::cell::Cell::new(false));
    let refill: Rc<dyn Fn()> = {
        let window = context.window.clone();
        let store = context.store.clone();
        let template = template.clone();
        let ids = ids.clone();
        let filling = filling.clone();
        let defaults = defaults.clone();
        Rc::new(move || {
            let templates = window.templates();
            // Deleting the default's template puts new charts back on None,
            // which this has to hear about before it shows a choice.
            let mut current = defaults.get();
            current.template = ChartDefaults::load(&store).template;
            defaults.set(current);
            let mut names = vec!["None".to_string()];
            names.extend(templates.iter().map(|t| t.name.clone()));
            *ids.borrow_mut() = templates.iter().map(|t| t.id).collect();
            filling.set(true);
            template.set_model(Some(&string_list(&names)));
            let at = current
                .template
                .and_then(|id| templates.iter().position(|t| t.id == id))
                .map(|at| at + 1)
                .unwrap_or(0);
            template.set_selected(at as u32);
            filling.set(false);
        })
    };
    refill();
    {
        let changed = changed.clone();
        template.connect_selected_notify(move |row| {
            if filling.get() {
                return;
            }
            let chosen = match row.selected() as usize {
                0 => None,
                at => ids.borrow().get(at - 1).copied(),
            };
            changed(&|d| d.template = chosen);
        });
    }
    *context.refill_default_template.borrow_mut() = Some(refill);
    indicators.add(&template);

    drawings.add(&footer);
    for group in [&bars, &scales, &hours, &indicators, &drawings] {
        context.charts_page.add(group);
    }
}

/// A copy to make your own, for a theme you cannot edit in place.
fn customize_row(context: &Rc<Context>) -> adw::ActionRow {
    let row = adw::ActionRow::new();
    row.set_title("Make it yours");

    let button = gtk::Button::with_label("Customize");
    button.set_valign(gtk::Align::Center);
    let ctx = context.clone();
    button.connect_clicked(move |button| {
        let base = ctx.theming.borrow().theme();
        let suggested = format!("Copy of {}", base.name);
        let ctx = ctx.clone();
        ask_name(button, "Customize theme", &suggested, "Create", move |chosen| {
            let mut theming = ctx.theming.borrow_mut();
            let taken: Vec<String> = theming.themes().iter().map(|t| t.id.clone()).collect();
            let id = unique_id(&chosen, &taken);
            ctx.store.save_theme(&base.duplicate(id.clone(), chosen));
            theming.reload_custom(&ctx.store);
            theming.select_theme(&id, &ctx.store);
            drop(theming);
            apply(&ctx);
            rebuild(&ctx);
        });
    });
    row.add_suffix(&button);
    row
}

/// Rename and Delete for a theme of your own, quiet beneath the picker:
/// they are things done to a theme now and then, not every time it is open.
fn own_theme_actions(context: &Rc<Context>) -> gtk::Box {
    let actions = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    actions.set_halign(gtk::Align::End);
    actions.set_margin_top(6);

    let rename = gtk::Button::with_label("Rename");
    rename.add_css_class("flat");
    let ctx = context.clone();
    rename.connect_clicked(move |button| {
        let current = ctx.theming.borrow().theme();
        let initial = current.name.clone();
        let ctx = ctx.clone();
        ask_name(button, "Rename theme", &initial, "Rename", move |chosen| {
            // The id stays: it is what the setting and every saved reference
            // point at, and a name is only what the list shows.
            let mut theme = current.clone();
            theme.name = chosen;
            ctx.store.save_theme(&theme);
            ctx.theming.borrow_mut().reload_custom(&ctx.store);
            rebuild(&ctx);
        });
    });

    let delete = gtk::Button::with_label("Delete");
    delete.add_css_class("flat");
    delete.add_css_class("error");
    let ctx = context.clone();
    delete.connect_clicked(move |button| confirm_delete(&ctx, button));

    actions.append(&rename);
    actions.append(&delete);
    actions
}

/// Asked first, because the button is small and a theme is an afternoon of
/// picking colours that no undo brings back.
fn confirm_delete(context: &Rc<Context>, anchor: &gtk::Button) {
    let theme = context.theming.borrow().theme();
    let body = format!("Delete \u{201c}{}\u{201d}? Charts go back to System.", theme.name);
    let dialog = adw::AlertDialog::new(Some("Delete theme"), Some(&body));
    dialog.add_response("cancel", "Cancel");
    dialog.add_response("delete", "Delete");
    dialog.set_response_appearance("delete", adw::ResponseAppearance::Destructive);
    dialog.set_default_response(Some("cancel"));
    dialog.set_close_response("cancel");

    let ctx = context.clone();
    dialog.connect_response(None, move |_, response| {
        if response != "delete" {
            return;
        }
        let mut theming = ctx.theming.borrow_mut();
        ctx.store.delete_theme(&theme.id);
        theming.reload_custom(&ctx.store);
        // Back to System, which is where every copy came from. Handing over
        // to whichever preset happens to sort first would leave the app on a
        // theme nobody chose.
        let back = if theming.omarchy_available() { OMARCHY_ID } else { FALLBACK_THEME_ID };
        theming.select_theme(back, &ctx.store);
        drop(theming);
        apply(&ctx);
        rebuild(&ctx);
    });
    dialog.present(Some(anchor));
}

fn theme_colors_group(context: &Rc<Context>, group_name: &str) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::new();
    group.set_title(group_name);

    let theme = context.theming.borrow().theme();
    for slot in UiSlot::ALL.into_iter().filter(|s| s.group() == group_name) {
        let row = adw::ActionRow::new();
        row.set_title(slot.label());

        let button = color_button(UiSlot::get(slot, &theme.ui));
        let ctx = context.clone();
        button.connect_rgba_notify(move |button| {
            let hex = colors::to_hex(&button.rgba());
            let mut theme = ctx.theming.borrow().theme();
            if !theme.source.is_editable() {
                return;
            }
            UiSlot::set(slot, &mut theme.ui, hex);
            ctx.store.save_theme(&theme);
            ctx.theming.borrow_mut().reload_custom(&ctx.store);
            apply(&ctx);
        });
        row.add_suffix(&button);
        group.add(&row);
    }
    group
}

/// The theme's indicator palette. Editing these changes every overlay that
/// chose a colour by name rather than by hex.
fn palette_group(context: &Rc<Context>) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::new();
    group.set_title("Palette");
    group.set_description(Some(
        "Offered first when picking a colour for an indicator, so they stay consistent.",
    ));

    let theme = context.theming.borrow().theme();
    for name in SWATCH_NAMES {
        let Some(swatch) = theme.swatch(name) else { continue };
        let row = adw::ActionRow::new();
        row.set_title(name);

        let button = color_button(&swatch.hex);
        let ctx = context.clone();
        let name = name.to_string();
        button.connect_rgba_notify(move |button| {
            let hex = colors::to_hex(&button.rgba());
            let mut theme = ctx.theming.borrow().theme();
            if !theme.source.is_editable() {
                return;
            }
            if let Some(swatch) = theme.swatches.iter_mut().find(|s| s.name == name) {
                swatch.hex = hex;
            }
            ctx.store.save_theme(&theme);
            ctx.theming.borrow_mut().reload_custom(&ctx.store);
            apply(&ctx);
        });
        row.add_suffix(&button);
        group.add(&row);
    }
    group
}

fn bar_colors_group(context: &Rc<Context>) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::new();
    group.set_title("Bars");

    // The theme's candles as written, not as the chosen style paints them:
    // editing an up body under Hollow must not save a transparent one.
    let scheme = theme_bars(&context.theming.borrow().theme());
    for slot in BarSlot::ALL {
        let row = adw::ActionRow::new();
        row.set_title(slot.label());

        let button = color_button(BarSlot::get(slot, &scheme));
        button.set_dialog(&alpha_dialog());
        let ctx = context.clone();
        button.connect_rgba_notify(move |button| {
            let rgba = button.rgba();
            // Alpha is meaningful here: a transparent body is how hollow
            // candles are expressed, so keep it rather than flattening to
            // #rrggbb.
            let hex = format!(
                "#{:02x}{:02x}{:02x}{:02x}",
                (rgba.red() * 255.0).round() as u8,
                (rgba.green() * 255.0).round() as u8,
                (rgba.blue() * 255.0).round() as u8,
                (rgba.alpha() * 255.0).round() as u8,
            );
            let mut theme = ctx.theming.borrow().theme();
            if !theme.source.is_editable() {
                return;
            }
            // As written rather than as resolved, so a colour nobody set —
            // monochrome, until somebody does — keeps following the theme.
            let mut bars = omacharts_engine::editable_bars(&theme);
            BarSlot::set(slot, &mut bars, hex);
            theme.bars = Some(bars);
            ctx.store.save_theme(&theme);
            ctx.theming.borrow_mut().reload_custom(&ctx.store);
            apply(&ctx);
        });
        row.add_suffix(&button);
        group.add(&row);
    }
    group
}

/// How orders and positions are drawn on a chart: a colour for each of buy,
/// sell, long, short, profit and loss, and a pattern for each kind of line.
/// Only on a theme of somebody's own, like the bars, and each row saves
/// only itself: what nobody set keeps following the theme. There while
/// trading is switched on, with or without a login, as every trading
/// setting is: without one nothing draws in it.
fn trading_look_group(context: &Rc<Context>) -> adw::PreferencesGroup {
    use omacharts_engine::indicators::LineStyle;
    use omacharts_engine::{TradingColour, TradingLine, TradingLook};

    let group = adw::PreferencesGroup::new();
    group.set_title("Trading");
    let theme = context.theming.borrow().theme();
    let swapped = context.theming.borrow().swapped();
    let shown = omacharts_engine::theme_trading(&theme, swapped);
    // Change the theme's own look by `edit`, and nothing else in it.
    let save = |ctx: &Rc<Context>, edit: &dyn Fn(&mut TradingLook)| {
        let mut theme = ctx.theming.borrow().theme();
        if !theme.source.is_editable() {
            return;
        }
        let mut look = omacharts_engine::editable_trading(&theme);
        edit(&mut look);
        theme.trading = Some(Box::new(look));
        ctx.store.save_theme(&theme);
        ctx.theming.borrow_mut().reload_custom(&ctx.store);
        apply(ctx);
    };

    for slot in TradingColour::ALL {
        let row = adw::ActionRow::new();
        row.set_title(slot.label());
        let button = color_button(shown.colour(slot));
        let ctx = context.clone();
        button.connect_rgba_notify(move |button| {
            let rgba = button.rgba();
            let hex = format!(
                "#{:02x}{:02x}{:02x}",
                (rgba.red() * 255.0).round() as u8,
                (rgba.green() * 255.0).round() as u8,
                (rgba.blue() * 255.0).round() as u8,
            );
            save(&ctx, &|look| *look.colour_mut(slot) = Some(omacharts_engine::ColorChoice::Fixed { hex: hex.clone() }));
        });
        row.add_suffix(&button);
        group.add(&row);
    }

    let ink = {
        let text = theme.ui.text.clone();
        crate::ui::chart_settings::Ink::new(move || text.clone())
    };
    for slot in TradingLine::ALL {
        let row = adw::ActionRow::new();
        row.set_title(slot.label());
        let strip = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        strip.add_css_class("linked");
        strip.add_css_class("stroke-picker");
        strip.set_valign(gtk::Align::Center);
        let mut first: Option<gtk::ToggleButton> = None;
        for style in LineStyle::ALL {
            let button = gtk::ToggleButton::new();
            button.add_css_class("flat");
            button.set_tooltip_text(Some(style.label()));
            let sample =
                crate::ui::chart_settings::stroke_sample(crate::ui::chart_settings::LINE_SAMPLE_WIDTH, style, &ink);
            button.set_child(Some(&sample));
            match &first {
                Some(anchor) => button.set_group(Some(anchor)),
                None => first = Some(button.clone()),
            }
            button.set_active(style == shown.line(slot));
            let ctx = context.clone();
            button.connect_toggled(move |button| {
                if button.is_active() {
                    save(&ctx, &|look| *look.line_mut(slot) = Some(style));
                }
            });
            strip.append(&button);
        }
        row.add_suffix(&strip);
        group.add(&row);
    }
    group
}

/// Everything that is not about how the app looks: what the charts offer, and
/// where their data comes from.
fn build_general_page(context: &Rc<Context>) {
    // First thing in the dialog: whether Omacharts is in the bar is the one
    // setting about the app rather than about a chart, and it is the one
    // people come looking for.
    if let Some(group) = desktop_group() {
        context.general_page.add(&group);
    }
    context.charts_other_page.add(&time_group(context));
    context.charts_other_page.add(&chart_group(context));
    context.general_page.add(&screenshots_group(context));
    context.general_page.add(&updates_group(context));
    build_charts_page(context);
    rebuild_templates(context);
    build_data_page(context);
}

/// The clock every chart reads, and how it writes the time.
///
/// Applied the moment it changes, to every chart on screen: it is not a default
/// for new charts but how the app tells time, and a chart left reading the old
/// clock would disagree with the one beside it about when a bar was.
fn time_group(context: &Rc<Context>) -> adw::PreferencesGroup {
    use omacharts_engine::clock::{ChartZone, TimeFormat, ZONES};

    let group = adw::PreferencesGroup::new();
    group.set_title("Time");
    group.set_description(Some(
        "How the time axis, the crosshair and every time you type into an indicator tell the time.",
    ));

    // The exchange's own first, then the curated zones in the order the
    // markets open across a day. A zone set by name from the command line
    // that is not among them is offered too, so the row can show it.
    let current = crate::clock::zone(&context.store);
    let mut zones: Vec<ChartZone> = std::iter::once(ChartZone::Exchange)
        .chain(ZONES.iter().filter_map(|name| ChartZone::from_key(name)))
        .collect();
    if !zones.contains(&current) {
        zones.push(current);
    }
    let now = chrono::Utc::now();
    let names: Vec<String> = zones
        .iter()
        .map(|zone| match zone {
            ChartZone::Exchange => "Each chart's exchange".to_string(),
            ChartZone::Fixed(tz) => {
                use chrono::Offset;
                let offset = now.with_timezone(tz).offset().fix().local_minus_utc();
                format!("{} ({})", zone.label(), utc_offset(offset))
            }
        })
        .collect();
    let zone_row = adw::ComboRow::new();
    zone_row.set_title("Time zone");
    zone_row.set_model(Some(&string_list(&names)));
    zone_row.set_enable_search(true);
    zone_row.set_selected(zones.iter().position(|z| *z == current).unwrap_or(0) as u32);
    {
        let store = context.store.clone();
        let window = context.window.clone();
        zone_row.connect_selected_notify(move |row| {
            let Some(chosen) = zones.get(row.selected() as usize).copied() else { return };
            if chosen == crate::clock::zone(&store) {
                return;
            }
            crate::clock::save_zone(&store, chosen);
            window.adopt_clock();
        });
    }
    group.add(&zone_row);

    let format_row = adw::ComboRow::new();
    format_row.set_title("Time format");
    let labels: Vec<String> = TimeFormat::ALL.iter().map(|f| f.label().to_string()).collect();
    format_row.set_model(Some(&string_list(&labels)));
    let format = crate::clock::format(&context.store);
    format_row.set_selected(TimeFormat::ALL.iter().position(|f| *f == format).unwrap_or(0) as u32);
    {
        let store = context.store.clone();
        let window = context.window.clone();
        format_row.connect_selected_notify(move |row| {
            let Some(chosen) = TimeFormat::ALL.get(row.selected() as usize).copied() else { return };
            if chosen == crate::clock::format(&store) {
                return;
            }
            crate::clock::save_format(&store, chosen);
            window.adopt_clock();
        });
    }
    group.add(&format_row);
    group
}

/// `UTC−4`, `UTC+5:30`, `UTC`.
fn utc_offset(seconds: i32) -> String {
    let minutes = seconds / 60;
    let sign = if minutes < 0 { "−" } else { "+" };
    let (hours, rest) = (minutes.abs() / 60, minutes.abs() % 60);
    match (hours, rest) {
        (0, 0) => "UTC".to_string(),
        (h, 0) => format!("UTC{sign}{h}"),
        (h, m) => format!("UTC{sign}{h}:{m:02}"),
    }
}

/// The Templates tab: every chart template, and a way to make, edit, rename
/// and delete them.
///
/// Rebuilt whole after any change, as the theme page is. There are a handful,
/// and a row that patched itself would be a second way to be wrong about what
/// a template is called or how many charts are using it.
fn rebuild_templates(context: &Rc<Context>) {
    if let Some(old) = context.templates_group.borrow_mut().take() {
        context.charts_templates_page.remove(&old);
    }
    let group = adw::PreferencesGroup::new();
    group.set_title("Chart templates");

    let add = gtk::Button::from_icon_name("list-add-symbolic");
    add.add_css_class("flat");
    add.set_valign(gtk::Align::Center);
    add.set_tooltip_text(Some("New chart template"));
    let ctx = context.clone();
    add.connect_clicked(move |button| {
        let ctx = ctx.clone();
        ask_name(button, "New Chart Template", "", "Create", move |name| {
            let id = ctx.window.create_template(&name);
            templates_changed(&ctx);
            // Straight into it: an empty template is only ever made to be
            // filled.
            edit_template(&ctx, id);
        });
    });
    group.set_header_suffix(Some(&add));

    let templates = context.window.templates();
    if templates.is_empty() {
        let row = adw::ActionRow::new();
        row.set_title("No chart templates");
        row.add_css_class("dim-label");
        group.add(&row);
    }
    for template in templates {
        let row = adw::ActionRow::new();
        row.set_title(&template.name);
        let holds = match template.indicators.is_empty() {
            true => "No indicators".to_string(),
            false => template.indicators.iter().map(|i| i.label()).collect::<Vec<_>>().join(", "),
        };
        let using = match context.window.charts_on_template(template.id) {
            0 => String::new(),
            1 => " · 1 chart".to_string(),
            n => format!(" · {n} charts"),
        };
        row.set_subtitle(&format!("{holds}{using}"));
        row.set_activatable(true);
        let ctx = context.clone();
        let id = template.id;
        row.connect_activated(move |_| edit_template(&ctx, id));

        let rename = gtk::Button::from_icon_name("document-edit-symbolic");
        rename.add_css_class("flat");
        rename.set_valign(gtk::Align::Center);
        rename.set_tooltip_text(Some("Rename"));
        let ctx = context.clone();
        let name = template.name.clone();
        rename.connect_clicked(move |button| {
            let ctx = ctx.clone();
            ask_name(button, "Rename Chart Template", &name, "Rename", move |chosen| {
                // The id stays, so every chart on it stays on it.
                ctx.window.rename_template(id, &chosen);
                templates_changed(&ctx);
            });
        });
        row.add_suffix(&rename);

        let delete = gtk::Button::from_icon_name("user-trash-symbolic");
        delete.add_css_class("flat");
        delete.set_valign(gtk::Align::Center);
        delete.set_tooltip_text(Some("Delete"));
        let ctx = context.clone();
        let name = template.name.clone();
        delete.connect_clicked(move |button| confirm_delete_template(&ctx, button, id, &name));
        row.add_suffix(&delete);

        row.add_suffix(&gtk::Image::from_icon_name("go-next-symbolic"));
        group.add(&row);
    }

    context.charts_templates_page.add(&group);
    *context.templates_group.borrow_mut() = Some(group);
}

/// A template's indicators, in their own editor over the preferences.
fn edit_template(context: &Rc<Context>, template: u32) {
    let ctx = context.clone();
    crate::ui::chart_settings::TemplateEditor::present(
        &context.window,
        template,
        &context.charts_templates_page,
        move || rebuild_templates(&ctx),
    );
}

/// Everything showing the list of templates, read again.
fn templates_changed(context: &Rc<Context>) {
    rebuild_templates(context);
    let refill = context.refill_default_template.borrow().clone();
    if let Some(refill) = refill {
        refill();
    }
}

/// Asked first when charts are using it, saying how many: deleting takes
/// nothing off them, but they stop following it, and that is not undone by
/// making another one with the same name.
fn confirm_delete_template(context: &Rc<Context>, anchor: &gtk::Button, id: u32, name: &str) {
    let body = match context.window.charts_on_template(id) {
        0 => format!("Delete \u{201c}{name}\u{201d}? No chart is using it."),
        1 => format!(
            "\u{201c}{name}\u{201d} is used by 1 chart. It keeps the template's indicators \
             as its own, and stops changing with it."
        ),
        n => format!(
            "\u{201c}{name}\u{201d} is used by {n} charts. They keep the template's \
             indicators as their own, and stop changing with it."
        ),
    };
    let dialog = adw::AlertDialog::new(Some("Delete Chart Template?"), Some(&body));
    dialog.add_response("cancel", "Cancel");
    dialog.add_response("delete", "Delete");
    dialog.set_response_appearance("delete", adw::ResponseAppearance::Destructive);
    dialog.set_default_response(Some("cancel"));
    dialog.set_close_response("cancel");
    let ctx = context.clone();
    dialog.connect_response(None, move |_, response| {
        if response != "delete" {
            return;
        }
        ctx.window.delete_template(id);
        templates_changed(&ctx);
    });
    dialog.present(Some(anchor));
}

/// Whether a screenshot is kept as a file, and where.
///
/// The group says what always happens, because the clipboard is the one part
/// of this with no control beside it: nothing else here would ever tell you a
/// screenshot is already waiting to be pasted, and somebody who turns the
/// switch below off needs to know they are declining the file rather than the
/// screenshot.
///
/// Both rows derive their state rather than reading something written down at
/// install time: nothing is stored until somebody changes it, so the day the
/// default folder moves, everyone who never chose moves with it.
/// Whether newer releases are downloaded behind the window. Off stops the
/// asking and leaves what is already downloaded in place: it is newer than
/// what pacman installed, and settings it has migrated would not open in an
/// older version.
fn updates_group(context: &Rc<Context>) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::new();
    group.set_title("Updates");
    let row = adw::SwitchRow::new();
    row.set_title("Update automatically");
    row.set_active(context.store.setting_bool(crate::update::SETTING, true));
    let store = context.store.clone();
    row.connect_active_notify(move |row| store.set_setting_bool(crate::update::SETTING, row.is_active()));
    group.add(&row);
    group
}

fn screenshots_group(context: &Rc<Context>) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::new();
    group.set_title("Screenshots");
    group.set_description(Some("Screenshots are always copied to the clipboard."));

    let folder = adw::ActionRow::new();
    folder.set_title("Save to");
    // The path is the whole point of the row, so it goes in the subtitle where
    // there is room for it — on one line, because a deep path is wider than
    // this dialog and a row that stretches it is worse than one that trails
    // off.
    folder.set_subtitle(&home_relative(&screenshot::folder(&context.store)));
    folder.set_subtitle_lines(1);
    folder.set_activatable(true);
    folder.set_sensitive(screenshot::save_file(&context.store));

    // "Also", because the line above says what already happened. No subtitle:
    // the row is one word away from the sentence it continues, and repeating
    // it underneath would be the third way of saying one thing.
    let keep = adw::ActionRow::new();
    keep.set_title("Also save a PNG");

    let switch = gtk::Switch::new();
    switch.set_valign(gtk::Align::Center);
    switch.set_active(screenshot::save_file(&context.store));
    let store = context.store.clone();
    let folder_row = folder.clone();
    switch.connect_state_set(move |_, on| {
        store.set_setting_bool(screenshot::SETTING_SAVE_FILE, on);
        // Greyed out rather than taken away: a folder nothing is being saved
        // to is still the folder it would be saved to, and hiding the row
        // would leave no way to see what turning this back on would do.
        folder_row.set_sensitive(on);
        glib::Propagation::Proceed
    });
    keep.add_suffix(&switch);
    group.add(&keep);

    let choose: Rc<dyn Fn(&adw::ActionRow)> = {
        let store = context.store.clone();
        Rc::new(move |row: &adw::ActionRow| {
            let dialog = gtk::FileDialog::new();
            dialog.set_title("Where screenshots are saved");
            let current = screenshot::folder(&store);
            if current.is_dir() {
                dialog.set_initial_folder(Some(&gio::File::for_path(&current)));
            }
            let parent = row.root().and_downcast::<gtk::Window>();
            let store = store.clone();
            let row = row.clone();
            dialog.select_folder(parent.as_ref(), gio::Cancellable::NONE, move |answer| {
                let Some(path) = answer.ok().and_then(|file| file.path()) else { return };
                // Choosing the default folder is choosing the default, not
                // pinning today's answer: stored empty, it keeps following.
                let chosen = if path == screenshot::default_folder() {
                    String::new()
                } else {
                    path.to_string_lossy().to_string()
                };
                store.set_setting(screenshot::SETTING_FOLDER, &chosen);
                row.set_subtitle(&home_relative(&screenshot::folder(&store)));
            });
        })
    };

    let button = gtk::Button::with_label("Change…");
    button.set_valign(gtk::Align::Center);
    let open = choose.clone();
    let row = folder.clone();
    button.connect_clicked(move |_| open(&row));
    folder.add_suffix(&button);

    let open = choose.clone();
    folder.connect_activated(move |row| open(row));

    group.add(&folder);
    group
}

/// A path as somebody would say it, so a row showing one is readable.
fn home_relative(path: &std::path::Path) -> String {
    let home = crate::store::home();
    match path.strip_prefix(&home) {
        Ok(rest) if !home.as_os_str().is_empty() => format!("~/{}", rest.display()),
        _ => path.display().to_string(),
    }
}

/// Where prices come from, and how much of them is kept.
fn build_data_page(context: &Rc<Context>) {
    let group = adw::PreferencesGroup::new();
    group.set_title("Storage");

    let cache = adw::ActionRow::new();
    cache.set_title("Cached data");

    // One closure, because three things move this number — clearing, choosing
    // a different limit, and a sweep finishing — and a row that only some of
    // them updated would be showing a figure the app knows is wrong.
    let refresh: Rc<dyn Fn()> = {
        let store = context.store.clone();
        let row = cache.clone();
        Rc::new(move || {
            let series = store.cached_series();
            let used = store.used_bytes();
            let limit = store.cache_limit();
            row.set_subtitle(&if series == 0 {
                describe_count(series)
            } else {
                format!(
                    "{} · {} of {} · {}",
                    describe_count(series),
                    describe_bytes(used),
                    cache::limit_label(limit),
                    describe_share(used, limit),
                )
            });
        })
    };
    refresh();

    let clear = gtk::Button::with_label("Clear");
    clear.add_css_class("destructive-action");
    clear.set_valign(gtk::Align::Center);
    let ctx = context.clone();
    let refreshed = refresh.clone();
    clear.connect_clicked(move |_| {
        let _ = ctx.store.clear_market_data();
        refreshed();
        (ctx.on_change)();
    });
    cache.add_suffix(&clear);
    group.add(&cache);

    let limit = adw::ComboRow::new();
    limit.set_title("Keep at most");
    limit.set_subtitle("Past this, the oldest data is dropped automatically");
    let labels: Vec<String> = cache::LIMITS.iter().map(|(_, label)| label.to_string()).collect();
    limit.set_model(Some(&string_list(&labels)));
    // A stored limit that is not one of the offered sizes falls back to the
    // one the app would have used anyway, rather than to whichever happens to
    // be first — a row showing a size that is not the size in force is worse
    // than no row at all.
    let chosen = context.store.cache_limit();
    let at = |bytes: i64| cache::LIMITS.iter().position(|(size, _)| *size == bytes);
    limit.set_selected(at(chosen).or_else(|| at(cache::DEFAULT_LIMIT)).unwrap_or(0) as u32);

    let ctx = context.clone();
    let refreshed = refresh.clone();
    limit.connect_selected_notify(move |row| {
        let Some((bytes, _)) = cache::LIMITS.get(row.selected() as usize) else { return };
        ctx.store.set_cache_limit(*bytes);
        refreshed();
        // A limit somebody has just made smaller has to bite now. Leaving it
        // to the six-hourly sweep would look exactly like a setting that does
        // nothing, and they are standing here watching the number.
        let Some(path) = ctx.store.path().map(std::path::Path::to_path_buf) else { return };
        let swept = refreshed.clone();
        cache::sweep_in_background(path, *bytes, move |_| swept());
    });
    group.add(&limit);

    // Trading switched off has no Trading page, and the Data page adds a
    // broker's data feed through the broker's own dialog itself.
    if !crate::features::trading() {
        crate::ui::feed_settings::build(
            &context.data_page,
            context.store.clone(),
            context.on_change.clone(),
            vec![group, backups_group(context)],
            Rc::new(|_: &str, _: &gtk::Widget| {}),
        );
        return;
    }
    let trading = crate::ui::trading_settings::build(
        &context.trading_page,
        context.store.clone(),
        context.window.desk(),
        context.on_change.clone(),
    );
    let window = context.window.clone();
    trading.set_edit_sizes(Rc::new(move |anchor: &gtk::Widget| window.edit_trading_sizes(anchor)));
    let window = context.window.clone();
    trading.set_labels_moved(Rc::new(move || window.adopt_order_labels()));
    let adding = trading.clone();
    let redraw = crate::ui::feed_settings::build(
        &context.data_page,
        context.store.clone(),
        context.on_change.clone(),
        vec![group, backups_group(context)],
        Rc::new(move |broker: &str, anchor: &gtk::Widget| crate::ui::trading_settings::add_broker(&adding, broker, anchor)),
    );
    trading.redraw_data_page(redraw);
}

/// When the settings were last backed up, and the way back to any backup.
///
/// Below the cache rather than beside it: the cache is what can be thrown
/// away, and this is what cannot.
fn backups_group(context: &Rc<Context>) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::new();
    group.set_title("Backups");
    let settings = context.store.path().map(std::path::Path::to_path_buf);
    if let Some(path) = &settings {
        group.set_description(Some(&format!(
            "Taken when your settings change: up to {} from the past week, in {}.",
            backup::MOST_KEPT,
            home_relative(&backup::dir(path)),
        )));
    }

    let row = adw::ActionRow::new();
    row.set_title("Last backup");
    let restore = gtk::Button::with_label("Restore…");
    restore.set_valign(gtk::Align::Center);

    // One closure, because taking a backup and restoring one both change the
    // answer, and the button has nothing to open while there is nothing in it.
    let refresh: Rc<dyn Fn()> = {
        let (row, restore, settings) = (row.clone(), restore.clone(), settings.clone());
        Rc::new(move || {
            let backups = settings.as_deref().map(backup::list).unwrap_or_default();
            row.set_subtitle(&match backups.first() {
                Some(newest) => format!(
                    "{} · {} kept",
                    capitalised(&backup::when(newest.taken, chrono::Local::now())),
                    backups.len()
                ),
                None => "None yet".to_string(),
            });
            restore.set_sensitive(!backups.is_empty());
        })
    };
    refresh();

    let now = gtk::Button::with_label("Back Up Now");
    now.set_valign(gtk::Align::Center);
    now.set_sensitive(settings.is_some());
    let store = context.store.clone();
    let refreshed = refresh.clone();
    let shown = row.clone();
    now.connect_clicked(move |_| {
        match backup::take(&store) {
            Ok((_, true)) => refreshed(),
            // Said, or the button looks like it did nothing.
            Ok((_, false)) => shown.set_subtitle("Nothing changed since the last backup"),
            Err(error) => shown.set_subtitle(&error.to_string()),
        }
    });

    let ctx = context.clone();
    restore.connect_clicked(move |button| choose_backup(&ctx, button));

    row.add_suffix(&now);
    row.add_suffix(&restore);
    group.add(&row);
    group
}

/// The backups, newest first, to pick one to put back.
///
/// One dialog that is both the list and the question, with Cancel the
/// default: a restore replaces everything on screen, and the one thing that
/// makes that safe — what it replaces is kept — is said where the choice is
/// made.
fn choose_backup(context: &Rc<Context>, anchor: &gtk::Button) {
    let Some(settings) = context.store.path() else { return };
    let backups = backup::list(settings);

    let list = gtk::ListBox::new();
    list.add_css_class("boxed-list");
    list.set_selection_mode(gtk::SelectionMode::None);
    let chosen: Rc<RefCell<Option<String>>> = Rc::default();
    let mut first: Option<gtk::CheckButton> = None;
    let now = chrono::Local::now();
    let mut ticks = Vec::new();
    for item in &backups {
        let row = adw::ActionRow::new();
        row.set_title(&capitalised(&backup::when(item.taken, now)));
        let tick = gtk::CheckButton::new();
        // Grouped even when it is the only one, so it is drawn as the choice
        // it is rather than as a box to tick.
        let leader = first.get_or_insert_with(gtk::CheckButton::new);
        tick.set_group(Some(&*leader));
        row.add_prefix(&tick);
        row.set_activatable_widget(Some(&tick));
        list.append(&row);
        ticks.push((tick, item.name.clone()));
    }

    let scroller = gtk::ScrolledWindow::new();
    scroller.set_hscrollbar_policy(gtk::PolicyType::Never);
    scroller.set_propagate_natural_height(true);
    scroller.set_max_content_height(360);
    scroller.set_child(Some(&list));

    // Restore belongs to the list it acts on, so it sits right under it; the
    // dialog's own buttons, below a gap, are the other ways out of it — a
    // file from another machine, or nothing at all.
    let restore = gtk::Button::with_label("Restore");
    restore.add_css_class("destructive-action");
    restore.set_margin_top(12);
    restore.set_margin_bottom(24);
    restore.set_sensitive(false);
    let body = gtk::Box::new(gtk::Orientation::Vertical, 0);
    body.append(&scroller);
    body.append(&restore);

    let dialog = adw::AlertDialog::new(Some("Restore Settings"), None);
    dialog.set_extra_child(Some(&body));
    // Stacked, the last response added is drawn first: Cancel last.
    dialog.add_response("cancel", "Cancel");
    dialog.add_response("file", "Choose a File…");
    dialog.set_default_response(Some("cancel"));
    dialog.set_close_response("cancel");

    for (tick, name) in ticks {
        let (chosen, gate) = (chosen.clone(), restore.clone());
        tick.connect_toggled(move |tick| {
            if tick.is_active() {
                *chosen.borrow_mut() = Some(name.clone());
                gate.set_sensitive(true);
            }
        });
    }

    let ctx = context.clone();
    let preferences = anchor.ancestor(adw::Dialog::static_type()).and_downcast::<adw::Dialog>();
    let anchor_kept = anchor.clone();
    {
        let (ctx, anchor, preferences, dialog) =
            (ctx.clone(), anchor_kept.clone(), preferences.clone(), dialog.clone());
        restore.connect_clicked(move |_| {
            let Some(name) = chosen.borrow().clone() else { return };
            dialog.close();
            finish_restore(&ctx, &anchor, preferences.as_ref(), backup::restore(&ctx.store, &name));
        });
    }
    dialog.connect_response(None, move |_, response| {
        if response == "file" {
            restore_from_file(&ctx, &anchor_kept, preferences.clone());
        }
    });
    dialog.present(Some(anchor));
}

/// Pick a settings file — a backup copied from another machine — and put it
/// back.
fn restore_from_file(context: &Rc<Context>, anchor: &gtk::Button, preferences: Option<adw::Dialog>) {
    let picker = gtk::FileDialog::new();
    picker.set_title("Restore Settings From a File");
    let filter = gtk::FileFilter::new();
    filter.set_name(Some("Omacharts settings"));
    filter.add_suffix("db");
    let filters = gio::ListStore::new::<gtk::FileFilter>();
    filters.append(&filter);
    picker.set_filters(Some(&filters));
    let parent = anchor.root().and_downcast::<gtk::Window>();
    let (ctx, anchor) = (context.clone(), anchor.clone());
    picker.open(parent.as_ref(), gio::Cancellable::NONE, move |answer| {
        let Some(path) = answer.ok().and_then(|file| file.path()) else { return };
        finish_restore(&ctx, &anchor, preferences.as_ref(), backup::restore_file(&ctx.store, &path));
    });
}

/// After a restore: the dialog goes, since every page of it was built from
/// the settings just replaced, and the window reads them back. A refusal
/// says why and changes nothing.
fn finish_restore(
    context: &Rc<Context>,
    anchor: &gtk::Button,
    preferences: Option<&adw::Dialog>,
    result: Result<backup::Restored, backup::Error>,
) {
    match result {
        Ok(_) => {
            if let Some(preferences) = preferences {
                preferences.close();
            }
            (context.on_restored)();
        }
        Err(error) => {
            let failed = adw::AlertDialog::new(Some("Not Restored"), Some(&error.to_string()));
            failed.add_response("close", "Close");
            failed.present(Some(anchor));
        }
    }
}

/// "today at 14:03" as the first words of a row.
fn capitalised(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// The bar widget: one switch, and the truth about what it did.
fn desktop_group() -> Option<adw::PreferencesGroup> {
    let home = crate::store::home();
    if !crate::bar_plugin::available(&home) {
        return None;
    }

    let group = adw::PreferencesGroup::new();
    group.set_title("Desktop");

    let row = adw::ActionRow::new();
    row.set_title("Show in the Omarchy bar");
    row.set_subtitle("Your watchlist, a click away from anywhere");

    let switch = gtk::Switch::new();
    switch.set_valign(gtk::Align::Center);
    switch.set_active(crate::bar_plugin::installed(&home));
    let row_weak = row.downgrade();
    switch.connect_state_set(move |switch, on| {
        let home = crate::store::home();
        let result = if on {
            crate::bar_plugin::install(&home)
        } else {
            crate::bar_plugin::remove(&home)
        };
        match result {
            Ok(()) => {
                if let Some(row) = row_weak.upgrade() {
                    row.set_subtitle(if on {
                        "Added to the bar"
                    } else {
                        "Your watchlist, a click away from anywhere"
                    });
                }
            }
            // Say what went wrong and put the switch back, rather than
            // leaving it looking like something happened.
            Err(error) => {
                if let Some(row) = row_weak.upgrade() {
                    row.set_subtitle(&error);
                }
                switch.set_active(!on);
            }
        }
        glib::Propagation::Proceed
    });
    row.add_suffix(&switch);
    group.add(&row);

    Some(group)
}

fn apply(context: &Rc<Context>) {
    context.theming.borrow_mut().apply();
    (context.on_change)();
}

fn color_button(hex: &str) -> gtk::ColorDialogButton {
    let button = gtk::ColorDialogButton::new(Some(gtk::ColorDialog::new()));
    button.set_rgba(&colors::parse(hex));
    button.set_valign(gtk::Align::Center);
    button.add_css_class("swatch-button");
    button
}

fn alpha_dialog() -> gtk::ColorDialog {
    let dialog = gtk::ColorDialog::new();
    dialog.set_with_alpha(true);
    dialog
}

/// A drop-down list of names with a line above each one at `splits`, for a
/// list that is several lists one after the other. An empty run between two
/// splits makes them the same place, and that place gets one line.
pub(crate) fn separated_list(splits: Vec<usize>) -> gtk::SignalListItemFactory {
    let factory = gtk::SignalListItemFactory::new();
    factory.connect_setup(|_, item| {
        let Some(item) = item.downcast_ref::<gtk::ListItem>() else { return };
        let column = gtk::Box::new(gtk::Orientation::Vertical, 6);
        let line = gtk::Separator::new(gtk::Orientation::Horizontal);
        let label = gtk::Label::new(None);
        label.set_xalign(0.0);
        column.append(&line);
        column.append(&label);
        item.set_child(Some(&column));
    });
    factory.connect_bind(move |_, item| {
        let Some(item) = item.downcast_ref::<gtk::ListItem>() else { return };
        let Some(column) = item.child().and_downcast::<gtk::Box>() else { return };
        let (Some(line), Some(label)) =
            (column.first_child(), column.last_child().and_downcast::<gtk::Label>())
        else {
            return;
        };
        let name = item.item().and_downcast::<gtk::StringObject>().map(|s| s.string());
        label.set_label(name.as_deref().unwrap_or(""));
        let at = item.position() as usize;
        line.set_visible(at > 0 && splits.contains(&at));
    });
    factory
}

fn string_list(items: &[String]) -> gtk::StringList {
    let list = gtk::StringList::new(&[]);
    for item in items {
        list.append(item);
    }
    list
}

/// Asks for a theme's or a chart template's name, offering `initial`, and
/// hands back what was typed. Blank is not an answer: a theme with no name is
/// one nobody can pick out of the list again.
pub(crate) fn ask_name(
    anchor: &impl IsA<gtk::Widget>,
    heading: &str,
    initial: &str,
    verb: &str,
    done: impl Fn(String) + 'static,
) {
    ask_for_text(anchor, heading, "Name", initial, verb, false, done);
}

/// A chart's title, asked for the way a name is, under its own word: blank is
/// an answer, and leaves the chart untitled.
pub(crate) fn ask_title(anchor: &impl IsA<gtk::Widget>, initial: &str, done: impl Fn(String) + 'static) {
    ask_for_text(anchor, "Chart title", "Title", initial, "Save", true, done);
}

fn ask_for_text(
    anchor: &impl IsA<gtk::Widget>,
    heading: &str,
    label: &str,
    initial: &str,
    verb: &str,
    blank_ok: bool,
    done: impl Fn(String) + 'static,
) {
    let answers = move |text: &str| blank_ok || !text.trim().is_empty();
    let name = adw::EntryRow::new();
    name.set_title(label);
    name.set_text(initial);
    let rows = adw::PreferencesGroup::new();
    rows.add(&name);

    let dialog = adw::AlertDialog::new(Some(heading), None);
    dialog.set_extra_child(Some(&rows));
    dialog.add_response("cancel", "Cancel");
    dialog.add_response("ok", verb);
    dialog.set_response_appearance("ok", adw::ResponseAppearance::Suggested);
    dialog.set_default_response(Some("ok"));
    dialog.set_close_response("cancel");
    // Offered blank, for a new name, it cannot be answered until it is not.
    dialog.set_response_enabled("ok", answers(initial));

    let gate = dialog.clone();
    name.connect_changed(move |field| {
        gate.set_response_enabled("ok", answers(&field.text()));
    });
    // An entry row keeps Return for itself; typing a name and pressing it is
    // one gesture and should finish the one thing.
    let finish = dialog.clone();
    name.connect_entry_activated(move |field| {
        if answers(&field.text()) {
            finish.emit_by_name::<()>("response", &[&"ok"]);
            finish.close();
        }
    });

    let field = name.clone();
    let answered = std::cell::Cell::new(false);
    dialog.connect_response(None, move |_, response| {
        if response != "ok" || answered.replace(true) {
            return;
        }
        done(field.text().trim().to_string());
    });

    dialog.present(Some(anchor));
    name.grab_focus();
}

/// An id for a theme called `name`, clear of every id in `taken`. Two themes
/// may share a name; they may not share an id.
fn unique_id(name: &str, taken: &[String]) -> String {
    let slug: String = name
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect();
    let slug = format!("custom-{slug}");
    (1..)
        .map(|n| if n == 1 { slug.clone() } else { format!("{slug}-{n}") })
        .find(|id| !taken.contains(id))
        .expect("an unbounded range runs out of nothing")
}

pub(crate) fn describe_bytes(bytes: i64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;
    let bytes = bytes as f64;
    if bytes >= GB {
        format!("{:.1} GB", bytes / GB)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes / MB)
    } else if bytes >= KB {
        format!("{:.0} kB", bytes / KB)
    } else {
        format!("{bytes:.0} bytes")
    }
}

/// How much of the limit is spoken for, as a whole percent.
///
/// Rounded away from both ends rather than to nearest. A cache with something
/// in it must not read 0% and one with room left must not read 100%, because
/// those two are the only readings anybody acts on and both would be lies.
fn describe_share(used: i64, limit: i64) -> String {
    if used <= 0 || limit <= 0 {
        return "0%".to_string();
    }
    if used >= limit {
        return "100%".to_string();
    }
    let share = (used as f64 / limit as f64 * 100.0).round() as i64;
    format!("{}%", share.clamp(1, 99))
}

fn describe_count(series: i64) -> String {
    match series {
        0 => "nothing cached yet".to_string(),
        1 => "1 series".to_string(),
        n => format!("{n} series"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A part in one section is not offered by the others, and is offered
    /// again once it is taken out; every change is the whole header.
    #[test]
    fn a_part_in_one_section_is_not_offered_in_the_others() {
        use crate::header::{Header, Part};
        crate::ui::on_gtk(|| {
            crate::features::force(crate::features::Feature::Trading, true);
            let saved: Rc<RefCell<Vec<Header>>> = Rc::default();
            let seen = saved.clone();
            let (sections, _) =
                header_sections(Header::standard(), move |header| seen.borrow_mut().push(header.clone()));
            let [left, center, right] = sections.as_slice() else { panic!("three sections") };
            assert_eq!(left.group.title(), "Left");
            assert_eq!(center.group.title(), "Center");
            assert_eq!(right.group.title(), "Right");
            assert_eq!(left.parts(), vec![Part::Feed, Part::Symbol, Part::Resolution, Part::Account]);
            assert_eq!(center.parts(), vec![Part::Title]);
            assert_eq!(right.parts(), vec![Part::DayPnl, Part::Trade, Part::Size, Part::Settings, Part::Maximize, Part::Link]);
            assert_eq!(left.offered(), vec![Part::Change, Part::Template, Part::Stripe]);
            assert_eq!(center.offered(), vec![Part::Change, Part::Template, Part::Stripe]);
            assert_eq!(right.offered(), vec![Part::Change, Part::Template, Part::Stripe]);

            // Out of the center, the title is offered by all three.
            center.set(Vec::new());
            assert_eq!(left.offered(), vec![Part::Change, Part::Template, Part::Title, Part::Stripe]);
            assert_eq!(center.offered(), vec![Part::Change, Part::Template, Part::Title, Part::Stripe]);
            assert_eq!(right.offered(), vec![Part::Change, Part::Template, Part::Title, Part::Stripe]);
            // The stripe into the left is offered by none.
            left.set(vec![Part::Symbol, Part::Stripe]);
            assert_eq!(
                center.offered(),
                vec![Part::Resolution, Part::Change, Part::Template, Part::Title, Part::Account, Part::Feed]
            );
            assert!(!left.offered().contains(&Part::Stripe));
            assert!(!right.offered().contains(&Part::Stripe));
            assert_eq!(
                saved.borrow().last(),
                Some(&Header {
                    left: vec![Part::Symbol, Part::Stripe],
                    center: Vec::new(),
                    right: vec![Part::DayPnl, Part::Trade, Part::Size, Part::Settings, Part::Maximize, Part::Link]
                })
            );
            // The corner's controls move like any other part: the gear out
            // of the right is offered by the left, and put there is offered
            // by neither until it is taken out again.
            right.set(vec![Part::Maximize, Part::Link]);
            assert!(left.offered().contains(&Part::Settings));
            left.set(vec![Part::Symbol, Part::Settings]);
            assert!(!right.offered().contains(&Part::Settings));
            assert_eq!(saved.borrow().last().map(|header| header.right.clone()), Some(vec![Part::Maximize, Part::Link]));
            left.set(vec![Part::Symbol]);
            assert!(right.offered().contains(&Part::Settings));
        });
    }

    /// Restoring puts every section back as it ships, at once, lists and
    /// offers included, and hands the whole header on once.
    #[test]
    fn restoring_the_header_puts_every_section_back() {
        use crate::header::{Header, Part};
        crate::ui::on_gtk(|| {
            crate::features::force(crate::features::Feature::Trading, true);
            let saved: Rc<RefCell<Vec<Header>>> = Rc::default();
            let seen = saved.clone();
            let moved = Header {
                left: vec![Part::Change, Part::Settings, Part::Size],
                center: Vec::new(),
                right: vec![Part::Symbol, Part::Stripe],
            };
            let (sections, restore) =
                header_sections(moved, move |header| seen.borrow_mut().push(header.clone()));
            let [left, center, right] = sections.as_slice() else { panic!("three sections") };
            restore();
            assert_eq!(left.parts(), vec![Part::Feed, Part::Symbol, Part::Resolution, Part::Account]);
            assert_eq!(center.parts(), vec![Part::Title]);
            assert_eq!(right.parts(), vec![Part::DayPnl, Part::Trade, Part::Size, Part::Settings, Part::Maximize, Part::Link]);
            assert_eq!(left.offered(), vec![Part::Change, Part::Template, Part::Stripe]);
            assert_eq!(right.offered(), vec![Part::Change, Part::Template, Part::Stripe]);
            assert_eq!(saved.borrow().as_slice(), &[Header::standard()]);
            // And the lists go on from there.
            center.set(Vec::new());
            assert_eq!(saved.borrow().last().map(|header| header.left.clone()), Some(Header::standard().left));
        });
    }

    #[test]
    fn copies_get_ids_that_are_not_taken() {
        assert_eq!(unique_id("My Night", &[]), "custom-my-night");
        assert_eq!(unique_id("My Night", &["custom-my-night".into()]), "custom-my-night-2");
    }

    /// Prefixed, so a copy named after a theme that ships — "Midnight" — is
    /// not given the id of the one it was copied from and silently hidden by
    /// it.
    #[test]
    fn a_copy_named_like_a_shipped_theme_does_not_take_its_id() {
        assert_ne!(unique_id("Midnight", &[]), "midnight");
    }

    #[test]
    fn names_with_punctuation_still_make_valid_ids() {
        let id = unique_id("Blue / Orange", &[]);
        assert!(id.chars().all(|c| c.is_alphanumeric() || c == '-'), "{id}");
    }

    #[test]
    fn sizes_read_sensibly() {
        assert_eq!(describe_bytes(0), "0 bytes");
        assert_eq!(describe_bytes(2048), "2 kB");
        assert_eq!(describe_bytes(5 * 1024 * 1024), "5.0 MB");
        assert_eq!(describe_bytes(3 * 1024 * 1024 * 1024), "3.0 GB");
    }

    /// The two readings somebody acts on are "empty" and "full", so neither
    /// may appear unless it is true.
    #[test]
    fn a_share_of_the_limit_never_rounds_to_a_lie() {
        let gb = 1024 * 1024 * 1024;
        assert_eq!(describe_share(0, gb), "0%");
        assert_eq!(describe_share(1024, gb), "1%", "something is not nothing");
        assert_eq!(describe_share(gb - 1, gb), "99%", "room left is not full");
        assert_eq!(describe_share(gb, gb), "100%");
        assert_eq!(describe_share(gb * 2, gb), "100%", "over is still full");
        assert_eq!(describe_share(gb / 2, gb), "50%");
    }

    #[test]
    fn counts_read_sensibly() {
        assert_eq!(describe_count(0), "nothing cached yet");
        assert_eq!(describe_count(1), "1 series");
        assert_eq!(describe_count(7), "7 series");
    }
}
