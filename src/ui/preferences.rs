//! Settings.
//!
//! Six sections, each a question somebody arrives with. General: the app on
//! this desktop, its clock, its backups. Market data: where the bars come
//! from and how they are kept. Templates: the sets of indicators charts can
//! share, the same whichever chart is on one. Trading, while it is switched
//! on: the accounts, and how orders and positions are drawn. Then the
//! visual sections, under a rule that says which charts they change.
//! Appearance: how every chart looks, and Overlays: what every chart draws
//! over its bars, each answered once for all of them; they are modules of
//! their own, [`crate::ui::appearance`] and [`crate::ui::overlays`], because
//! a chart's settings show them too. Charts: what a new chart opens with.

use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;
use gtk::{gio, glib};

use crate::backup;
use crate::cache;
use crate::store::Store;
use crate::theming::Theming;
use crate::ui::form_window;
use crate::ui::scope_rule::Scope;
use crate::ui::window::Window;

pub struct Preferences;

/// The section of the account lists and the trading look, by its id.
pub const TRADING: &str = "trading";

/// The section that edits what every chart's header shows, by its id.
pub const OVERLAYS: &str = "overlays";

/// The tag of the page at the root of a section's page stack.
pub(crate) const ROOT: &str = "root";

/// A section that is every chart's look, which a chart's settings show too:
/// its id, title, icon, and the window's way to build its page for a scope.
type Visual = (&'static str, &'static str, &'static str, fn(&Rc<Scope>) -> adw::PreferencesPage);

const VISUAL: [Visual; 2] = [
    ("appearance", "Appearance", "preferences-desktop-appearance-symbolic", Window::appearance_page),
    (OVERLAYS, "Overlays", "view-grid-symbolic", Window::overlays_page),
];

/// Where the window was when it was last looked at, for this run of the app:
/// the section down the sidebar, by its id. Opening it again goes back
/// there, so somebody adjusting one row over and over does not have to find
/// it each time. Not kept across restarts.
#[derive(Clone, Default)]
struct Seen {
    section: Option<String>,
}

thread_local! {
    static LAST_SEEN: RefCell<Seen> = RefCell::new(Seen::default());
}

struct Context {
    store: Rc<Store>,
    /// For the trading look, which is worn over whichever theme is chosen.
    theming: Rc<RefCell<Theming>>,
    /// Repaints the charts once the feed or the look has been stored.
    on_change: Rc<dyn Fn()>,
    /// Opens the window's own resolution editor. The list belongs to the
    /// window — every chart offers the same resolutions — so it is edited
    /// there and merely reached from here.
    on_edit_resolutions: Rc<dyn Fn()>,
    /// Has the window read back everything a restored backup changed.
    on_restored: Rc<dyn Fn()>,
    general_page: adw::PreferencesPage,
    charts_page: adw::PreferencesPage,
    templates_page: adw::PreferencesPage,
    data_page: adw::PreferencesPage,
    trading_page: adw::PreferencesPage,
    /// The window, for the chart templates: they are shown on its charts,
    /// and only it knows which of its charts are using one.
    window: Rc<Window>,
    /// Which charts the visual sections and Charts change: this window's
    /// own, so a chart's settings picking Local leaves it as it is.
    scope: Rc<Scope>,
    /// The list on the Templates page, rebuilt whenever one changes.
    templates_group: RefCell<Option<adw::PreferencesGroup>>,
    /// Reads the Charts page's rows again, after a template is made,
    /// renamed or deleted on the Templates page.
    show_chart_settings: RefCell<Option<Rc<dyn Fn()>>>,
}

thread_local! {
    /// The window while it is open, so asking again raises it rather than
    /// opening a second one beside it.
    static OPEN: RefCell<glib::WeakRef<adw::Window>> = RefCell::new(glib::WeakRef::new());
}

impl Preferences {
    /// A window of its own rather than a sheet over the charts.
    ///
    /// A sheet is drawn inside the main window, so the compositor never sees
    /// it: it cannot be moved, resized, floated beside the chart it is
    /// changing or sent to another workspace. A toplevel can, and it leaves
    /// the charts usable while it is open — which matters here, because half
    /// of what it changes is how they look.
    ///
    /// Sections down a sidebar, each a navigation view of its own so a page
    /// can be pushed inside one without taking the others with it.
    #[allow(clippy::too_many_arguments)]
    pub fn present(
        parent: &impl IsA<gtk::Window>,
        store: Rc<Store>,
        theming: Rc<RefCell<Theming>>,
        on_change: Rc<dyn Fn()>,
        on_edit_resolutions: Rc<dyn Fn()>,
        on_restored: Rc<dyn Fn()>,
        window: Rc<Window>,
        at: Option<&str>,
    ) {
        if let Some(open) = OPEN.with(|open| open.borrow().upgrade()) {
            // Asked for a particular section, it opens again there; changes
            // are already applied, so closing loses nothing.
            if at.is_none() {
                open.present();
                return;
            }
            open.close();
        }

        let general_page = adw::PreferencesPage::new();
        let charts_page = adw::PreferencesPage::new();
        let templates_page = adw::PreferencesPage::new();
        let data_page = adw::PreferencesPage::new();
        let trading_page = adw::PreferencesPage::new();

        // What the app does first, then, under a rule that says which
        // charts they change, what the charts look like. A template is the
        // same for every chart on it, so it sits with the app, not under
        // the rule. Trading's page only while trading is switched on.
        let mut app: Vec<Section> = vec![
            ("general", "General", "preferences-system-symbolic", section_view(&general_page, "General")),
            ("data", "Market data", "network-transmit-receive-symbolic", section_view(&data_page, "Market data")),
            ("templates", "Templates", "edit-copy-symbolic", section_view(&templates_page, "Templates")),
        ];
        if crate::features::trading() {
            app.push((TRADING, "Trading", "view-list-bullet-symbolic", section_view(&trading_page, "Trading")));
        }
        // Global to begin with, whatever the arrangement: Preferences is
        // the app's, and opening it to change one chart is the exception.
        let (scope, rule) = Scope::new(&window, false);
        let mut visual = visual_sections(&scope);
        visual.push(("charts", "Charts", "utilities-system-monitor-symbolic", section_view(&charts_page, "Charts")));
        // Opened from a chart at the section it asked for, else where it was
        // left. The header is the first thing on Overlays, so opening there
        // is all it takes to show it.
        let last = LAST_SEEN.with(|seen| seen.borrow().clone());
        let start = at
            .map(str::to_string)
            .or(last.section)
            .unwrap_or_else(|| "general".to_string());

        let context = Rc::new(Context {
            store,
            theming,
            on_change,
            on_edit_resolutions,
            on_restored,
            general_page,
            charts_page,
            templates_page,
            data_page,
            trading_page,
            window,
            scope,
            templates_group: RefCell::new(None),
            show_chart_settings: RefCell::new(None),
        });
        build_general_page(&context);

        // A form window, but not one `form_window` keeps: Preferences
        // remembers its own.
        let frame = form_window::untracked(parent, "Preferences", (820, 640));
        frame.add_css_class("preferences-window");
        let sections = Sections::new("Preferences", [&app, &visual], &rule);
        sections.connect_selected(|id| LAST_SEEN.with(|seen| seen.borrow_mut().section = Some(id.to_string())));
        if let Some(at) = sections.rows().iter().position(|row| row.widget_name() == start) {
            sections.select(at);
        }
        // Narrow enough and the sidebar becomes a page of its own, the way
        // every split view on this platform folds.
        if let Ok(narrow) = adw::BreakpointCondition::parse("max-width: 560sp") {
            let breakpoint = adw::Breakpoint::new(narrow);
            breakpoint.add_setter(&sections.split, "collapsed", Some(&true.to_value()));
            frame.add_breakpoint(breakpoint);
        }
        frame.set_content(Some(&sections.split));

        OPEN.with(|open| open.borrow().set(Some(&frame)));
        frame.present();
    }
}

/// A section of a settings window: its id, title, icon, and its page stack,
/// so a page can be pushed inside one without taking the others with it.
pub(crate) type Section = (&'static str, &'static str, &'static str, adw::NavigationView);

/// `page` as the root of a section's page stack, under a header bar.
pub(crate) fn section_view(page: &impl IsA<gtk::Widget>, title: &str) -> adw::NavigationView {
    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&adw::HeaderBar::new());
    toolbar.set_content(Some(page));
    let view = adw::NavigationView::new();
    view.add(&adw::NavigationPage::with_tag(&toolbar, title, ROOT));
    view
}

/// Appearance and Overlays, for Preferences and for a chart's settings,
/// changing the charts `scope` says.
pub(crate) fn visual_sections(scope: &Rc<Scope>) -> Vec<Section> {
    VISUAL.iter().map(|(id, title, icon, page)| (*id, *title, *icon, section_view(&page(scope), title))).collect()
}

/// A settings window's sections down a sidebar, in two lists read as one,
/// and the selected one beside it.
#[derive(Clone)]
pub(crate) struct Sections {
    pub split: adw::NavigationSplitView,
    /// The sidebar's header bar, for a window that names something under
    /// its title.
    pub header: adw::HeaderBar,
    lists: [gtk::ListBox; 2],
}

impl Sections {
    /// `lists` down a sidebar titled `title`, `rule` between them, the first
    /// section chosen.
    ///
    /// Two lists rather than one with a header, so the rule's buttons sit in
    /// the tab order between them.
    pub(crate) fn new(title: &str, lists: [&[Section]; 2], rule: &impl IsA<gtk::Widget>) -> Sections {
        let stack = gtk::Stack::new();
        let mut titles: Vec<(String, String)> = Vec::new();
        let lists = lists.map(|sections| {
            let list = gtk::ListBox::new();
            list.add_css_class("navigation-sidebar");
            for (id, title, icon, view) in sections {
                stack.add_named(view, Some(id));
                titles.push((id.to_string(), title.to_string()));
                list.append(&sidebar_row(id, title, icon));
            }
            list
        });

        let column = gtk::Box::new(gtk::Orientation::Vertical, 0);
        column.append(&lists[0]);
        column.append(rule);
        column.append(&lists[1]);
        let header = adw::HeaderBar::new();
        let side = adw::ToolbarView::new();
        side.add_top_bar(&header);
        let scroller = gtk::ScrolledWindow::new();
        scroller.set_hscrollbar_policy(gtk::PolicyType::Never);
        scroller.set_child(Some(&column));
        side.set_content(Some(&scroller));

        let first = titles.first().map(|(_, title)| title.clone()).unwrap_or_default();
        let content = adw::NavigationPage::new(&stack, &first);
        let split = adw::NavigationSplitView::new();
        split.set_sidebar(Some(&adw::NavigationPage::new(&side, title)));
        split.set_content(Some(&content));

        for (at, list) in lists.iter().enumerate() {
            // A section chosen shows its page under its title, and goes to
            // it when the split is folded. One selection across both lists:
            // choosing in one lets go of the other.
            let (stack, content, shown, titles) = (stack.clone(), content.clone(), split.clone(), titles.clone());
            let other = lists[1 - at].clone();
            list.connect_row_selected(move |_, row| {
                let Some(row) = row else { return };
                other.unselect_all();
                let id = row.widget_name();
                let Some((_, title)) = titles.iter().find(|(known, _)| *known == id.as_str()) else { return };
                stack.set_visible_child_name(&id);
                content.set_title(title);
                shown.set_show_content(true);
            });
            // The arrows read the two lists as one, past the rule: left to
            // itself a list hands an arrow off its end to Tab, which stops on
            // the rule's buttons, where the next arrow goes nowhere.
            let other = lists[1 - at].clone();
            list.connect_keynav_failed(move |_, direction| {
                let row = match (at, direction) {
                    (0, gtk::DirectionType::Down) => other.row_at_index(0),
                    (1, gtk::DirectionType::Up) => other.last_child().and_downcast::<gtk::ListBoxRow>(),
                    _ => None,
                };
                let Some(row) = row else { return glib::Propagation::Proceed };
                other.select_row(Some(&row));
                row.grab_focus();
                glib::Propagation::Stop
            });
        }
        let sections = Sections { split, header, lists };
        sections.select(0);
        sections
    }

    /// Every section's row, down both lists.
    pub(crate) fn rows(&self) -> Vec<gtk::ListBoxRow> {
        self.lists.iter().flat_map(|list| (0..).map_while(move |at| list.row_at_index(at))).collect()
    }

    /// The chosen section, by its place down the sidebar.
    pub(crate) fn selected(&self) -> usize {
        self.rows().iter().position(|row| row.is_selected()).unwrap_or(0)
    }

    /// Choose the section at `at` down the sidebar.
    pub(crate) fn select(&self, at: usize) {
        let Some(row) = self.rows().into_iter().nth(at) else { return };
        if let Some(list) = row.parent().and_downcast::<gtk::ListBox>() {
            list.select_row(Some(&row));
        }
    }

    /// `chosen` hears the id of each section chosen.
    pub(crate) fn connect_selected(&self, chosen: impl Fn(&str) + Clone + 'static) {
        for list in &self.lists {
            let chosen = chosen.clone();
            list.connect_row_selected(move |_, row| {
                if let Some(row) = row {
                    chosen(&row.widget_name());
                }
            });
        }
    }
}

/// A section's row in a settings sidebar: its icon and its title, named by
/// its id.
fn sidebar_row(id: &str, title: &str, icon: &str) -> gtk::ListBoxRow {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    row.append(&gtk::Image::from_icon_name(icon));
    row.append(&gtk::Label::new(Some(title)));
    gtk::ListBoxRow::builder().child(&row).name(id).build()
}

/// A word in a sidebar rule's look: small, spaced capitals, faded.
pub(crate) fn rule_word(text: &str) -> gtk::Label {
    let label = gtk::Label::new(Some(text));
    label.add_css_class("caption-heading");
    label.add_css_class("dim-label");
    let spaced = gtk::pango::AttrList::new();
    spaced.insert(gtk::pango::AttrInt::new_letter_spacing(gtk::pango::SCALE));
    label.set_attributes(Some(&spaced));
    label
}

/// A way to the resolution strip's list from the settings, since the other
/// way in is right-clicking a strip, which you have to know about to find
/// (whether there is a strip at all is the header's to say); and how far the
/// charts zoom to the wheel and the touchpad.
fn chart_group(context: &Rc<Context>) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::new();
    group.set_title("Chart");

    let row = adw::ActionRow::new();
    row.set_title("Preset resolutions");
    row.set_activatable(true);

    let button = gtk::Button::with_label("Edit…");
    button.add_css_class("action-word");
    button.set_valign(gtk::Align::Center);
    let edit = context.on_edit_resolutions.clone();
    button.connect_clicked(move |_| edit());
    row.add_suffix(&button);

    let edit = context.on_edit_resolutions.clone();
    row.connect_activated(move |_| edit());

    group.add(&row);

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

/// A chart's settings, for every chart or for the focused one, as the rule
/// over the sidebar says.
///
/// Global, a row changes the defaults and every chart in every chartbook at
/// once: picking Global is the asking. Local, it changes the focused chart,
/// as its own settings would. Either way the rows show what they change, read
/// again after every change, so a template change declined shows the old one.
fn build_charts_page(context: &Rc<Context>) {
    let scope = context.scope.clone();
    let rows = crate::ui::chart_settings::ChartRows::new(&context.window, move || !scope.local(), || {});
    // Grouped as a chart's own settings are, so a row is found in the same
    // place in both.
    let group = |title: &str, rows: &[&adw::PreferencesRow]| {
        let group = adw::PreferencesGroup::new();
        group.set_title(title);
        for row in rows {
            group.add(*row);
        }
        context.charts_page.add(&group);
        group
    };
    group("Scales", &[rows.log.upcast_ref()]);
    let hours = group("Session", &[rows.session.upcast_ref()]);
    // Only where some feed has a choice to make: the default of nothing is
    // no row.
    if omacharts_engine::continuous::any_offered() {
        hours.add(&rows.continuous);
    }
    group("Indicators", &[rows.template.upcast_ref(), rows.legend.upcast_ref()]);
    group("Drawings", &[rows.shows.upcast_ref(), rows.sends.upcast_ref()]);
    // What `every()` names moves with the scope, and under Local with the
    // focus, and the window tells its watchers of both.
    let (show, shown) = (rows.show.clone(), rows.log.downgrade());
    context.window.watch_chart(move || {
        if shown.upgrade().is_none_or(|row| row.root().is_none()) {
            return false;
        }
        show();
        true
    });
    *context.show_chart_settings.borrow_mut() = Some(rows.show);
}

/// How orders and positions are drawn on a chart: `first`, the Trading
/// page's own rows (which side the labels sit at, and how P&L is shown),
/// then a colour for each of buy, sell, long, short, profit and loss, and a
/// pattern for each kind of line.
/// One look worn over every theme, and each row saves only itself: what
/// nobody set keeps following the theme. On the Trading page, there while
/// trading is switched on, with or without a login, as every trading
/// setting is: without one nothing draws in it.
fn trading_look_group(context: &Rc<Context>, first: Vec<adw::PreferencesRow>) -> adw::PreferencesGroup {
    use omacharts_engine::indicators::LineStyle;
    use omacharts_engine::{TradingColour, TradingLine, TradingLook};

    let group = adw::PreferencesGroup::new();
    group.set_title("Orders and positions");
    for row in &first {
        group.add(row);
    }
    let theming = context.theming.borrow();
    let theme = theming.theme();
    let shown = omacharts_engine::theme_trading(&theme, theming.trading_look(), theming.bar_look());
    let mut set = theming.trading_look().clone();
    drop(theming);
    // Change the look by `edit`, and nothing else in it.
    let save = |ctx: &Rc<Context>, edit: &dyn Fn(&mut TradingLook)| {
        let mut look = ctx.theming.borrow().trading_look().clone();
        edit(&mut look);
        let mut theming = ctx.theming.borrow_mut();
        theming.set_trading_look(look, &ctx.store);
        theming.apply();
        drop(theming);
        (ctx.on_change)();
    };

    // From the theme's palette, as every other colour is picked, so a slot
    // set to a swatch follows the theme from then on.
    for slot in TradingColour::ALL {
        let row = adw::ActionRow::new();
        row.set_title(slot.label());
        let ctx = context.clone();
        let picker = crate::ui::palette::picker(&theme, set.colour_mut(slot).clone(), shown.colour(slot), move |choice| {
            save(&ctx, &|look| *look.colour_mut(slot) = Some(choice.clone()));
        });
        row.add_suffix(&picker);
        row.set_activatable_widget(Some(&picker));
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
    context.general_page.add(&time_group(context));
    context.general_page.add(&chart_group(context));
    context.general_page.add(&backups_group(context));
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

/// The Templates page: every chart template, and a way to make, edit, rename
/// and delete them.
///
/// Rebuilt whole after any change, as the theme page is. There are a handful,
/// and a row that patched itself would be a second way to be wrong about what
/// a template is called or how many charts are using it.
fn rebuild_templates(context: &Rc<Context>) {
    if let Some(old) = context.templates_group.borrow_mut().take() {
        context.templates_page.remove(&old);
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

    context.templates_page.add(&group);
    *context.templates_group.borrow_mut() = Some(group);
}

/// A template's indicators, in their own editor over the preferences.
fn edit_template(context: &Rc<Context>, template: u32) {
    let ctx = context.clone();
    crate::ui::chart_settings::TemplateEditor::present(
        &context.window,
        template,
        &context.templates_page,
        move || rebuild_templates(&ctx),
    );
}

/// Everything showing the list of templates, read again.
fn templates_changed(context: &Rc<Context>) {
    rebuild_templates(context);
    let show = context.show_chart_settings.borrow().clone();
    if let Some(show) = show {
        show();
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
    let dialog = crate::ui::dialogs::card("Delete Chart Template?", Some(&body), None);
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
    clear.add_css_class("action-word");
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

    // Trading switched off has no Trading page, and the Market data page adds a
    // trading service's data feed through the trading service's own dialog itself.
    if !crate::features::trading() {
        crate::ui::feed_settings::build(
            &context.data_page,
            context.store.clone(),
            context.on_change.clone(),
            vec![group],
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
    let window = context.window.clone();
    trading.set_pnl_changed(Rc::new(move || window.adopt_pnl_display()));
    trading.set_last(&trading_look_group(context, crate::ui::trading_settings::chart_rows(&trading)));
    let adding = trading.clone();
    let redraw = crate::ui::feed_settings::build(
        &context.data_page,
        context.store.clone(),
        context.on_change.clone(),
        vec![group],
        Rc::new(move |service: &str, anchor: &gtk::Widget| crate::ui::trading_settings::add_service(&adding, service, anchor)),
    );
    trading.redraw_data_page(redraw);
}

/// When the settings were last backed up, and the way back to any backup.
///
/// On General rather than beside the cache: the cache is market data that
/// can be fetched again, and this is everything somebody made.
fn backups_group(context: &Rc<Context>) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::new();
    group.set_title("Preference backups");
    let settings = context.store.path().map(std::path::Path::to_path_buf);

    let row = adw::ActionRow::new();
    row.set_title("Last backup");
    let restore = gtk::Button::with_label("Restore…");
    restore.add_css_class("action-word");
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
    now.add_css_class("action-word");
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
    let preferences = anchor.ancestor(adw::Window::static_type()).and_downcast::<adw::Window>();
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
fn restore_from_file(context: &Rc<Context>, anchor: &gtk::Button, preferences: Option<adw::Window>) {
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
    preferences: Option<&adw::Window>,
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
            // A failure said before clears once the switch works.
            Ok(()) => {
                if let Some(row) = row_weak.upgrade() {
                    row.set_subtitle("");
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
    let blank = |text: &str| text.trim().is_empty().then(String::new);
    ask_for_text(anchor, heading, "Name", initial, verb, blank, done);
}

/// A chart's title, asked for the way a name is, under its own word: blank is
/// an answer, and leaves the chart untitled.
pub(crate) fn ask_title(anchor: &impl IsA<gtk::Widget>, initial: &str, done: impl Fn(String) + 'static) {
    ask_for_text(anchor, "Chart title", "Title", initial, "Save", |_| None, done);
}

/// A line of text asked for in a card, offering `initial`, and handed back
/// trimmed. `refused` says why a text is no answer: it holds `verb` back,
/// and marks the box with the reason, though not while the box is blank,
/// which is not a mistake yet.
pub(crate) fn ask_for_text(
    anchor: &impl IsA<gtk::Widget>,
    heading: &str,
    label: &str,
    initial: &str,
    verb: &str,
    refused: impl Fn(&str) -> Option<String> + 'static,
    done: impl Fn(String) + 'static,
) {
    let name = adw::EntryRow::new();
    name.set_title(label);
    name.set_text(initial);
    let rows = adw::PreferencesGroup::new();
    rows.add(&name);

    let dialog = crate::ui::dialogs::card(heading, None, Some(rows.upcast_ref()));
    dialog.add_response("cancel", "Cancel");
    dialog.add_response("ok", verb);
    dialog.set_response_appearance("ok", adw::ResponseAppearance::Suggested);
    dialog.set_default_response(Some("ok"));
    dialog.set_close_response("cancel");
    // Offered blank, for a new name, it cannot be answered until it is not.
    dialog.set_response_enabled("ok", refused(initial).is_none());

    let refused = Rc::new(refused);
    let gate = dialog.clone();
    let checks = refused.clone();
    name.connect_changed(move |field| {
        let text = field.text();
        let why = checks(&text);
        gate.set_response_enabled("ok", why.is_none());
        match why.filter(|_| !text.trim().is_empty()) {
            Some(why) => {
                field.add_css_class("error");
                field.set_tooltip_text(Some(&why));
            }
            None => {
                field.remove_css_class("error");
                field.set_tooltip_text(None);
            }
        }
    });
    // An entry row keeps Return for itself; typing a name and pressing it is
    // one gesture and should finish the one thing.
    let finish = dialog.clone();
    let checks = refused.clone();
    name.connect_entry_activated(move |field| {
        if checks(&field.text()).is_none() {
            finish.emit_by_name::<()>("response", &[&"ok"]);
            finish.close();
        }
    });

    let field = name.clone();
    let answered = std::cell::Cell::new(false);
    dialog.connect_response(None, move |_, response| {
        let typed = field.text();
        if response != "ok" || refused(&typed).is_some() || answered.replace(true) {
            return;
        }
        done(typed.trim().to_string());
    });

    dialog.present(Some(anchor));
    name.grab_focus();
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
