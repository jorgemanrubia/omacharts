//! Settings.
//!
//! A theme is every colour the app wears, candles included; once it is yours
//! to edit, every one of them is a row on the Theme page. How candles are
//! painted from those colours — filled, hollow, red up, monochrome — is a
//! chart setting, and lives under Charts.
//!
//! Built-ins and the Omarchy theme are not editable in place. "Duplicate"
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

use crate::cache;
use crate::chart_defaults::ChartDefaults;
use crate::store::Store;
use crate::theming::Theming;
use crate::ui::colors;
use crate::ui::screenshot;

pub struct Preferences;

struct Context {
    store: Rc<Store>,
    theming: Rc<RefCell<Theming>>,
    on_change: Rc<dyn Fn()>,
    /// Opens the window's own resolution editor. The list belongs to the
    /// window — every chart offers the same resolutions — so it is edited
    /// there and merely reached from here.
    on_edit_resolutions: Rc<dyn Fn()>,
    /// Puts every chart to the defaults on the Charts page.
    on_apply_chart_defaults: Rc<dyn Fn()>,
    page: adw::PreferencesPage,
    general_page: adw::PreferencesPage,
    charts_page: adw::PreferencesPage,
    charts_other_page: adw::PreferencesPage,
    data_page: adw::PreferencesPage,
    groups: RefCell<Vec<adw::PreferencesGroup>>,
}

impl Preferences {
    pub fn present(
        parent: &impl IsA<gtk::Widget>,
        store: Rc<Store>,
        theming: Rc<RefCell<Theming>>,
        on_change: Rc<dyn Fn()>,
        on_edit_resolutions: Rc<dyn Fn()>,
        on_apply_chart_defaults: Rc<dyn Fn()>,
    ) {
        let dialog = adw::Dialog::new();
        dialog.set_title("Preferences");
        // The breakpoint below needs a floor to measure against.
        dialog.set_size_request(360, 360);
        // Most of the window rather than a fixed box: the appearance page is a
        // long list of colours, and every row that fits is a scroll saved.
        let parent = parent.as_ref();
        dialog.set_content_width((parent.width() * 85 / 100).max(560));
        dialog.set_content_height((parent.height() * 85 / 100).max(640));

        let page = adw::PreferencesPage::new();
        let general_page = adw::PreferencesPage::new();
        let charts_page = adw::PreferencesPage::new();
        let data_page = adw::PreferencesPage::new();

        // General first: what the app does, before what it looks like.
        // Charts is two things: what a new chart opens with, and the rest.
        let charts_other_page = adw::PreferencesPage::new();
        let charts = gtk::Stack::new();
        charts.add_titled(&charts_page, Some("defaults"), "Defaults");
        charts.add_titled(&charts_other_page, Some("other"), "Other");

        let sections: [(&str, &str, gtk::Widget); 4] = [
            ("General", "preferences-system-symbolic", general_page.clone().upcast()),
            ("Charts", "utilities-system-monitor-symbolic", charts.upcast()),
            ("Theme", "applications-graphics-symbolic", page.clone().upcast()),
            ("Data", "network-server-symbolic", data_page.clone().upcast()),
        ];
        dialog.set_child(Some(&split_view(&dialog, &sections)));

        let context = Rc::new(Context {
            store,
            theming,
            on_change,
            on_edit_resolutions,
            on_apply_chart_defaults,
            page,
            general_page,
            charts_page,
            charts_other_page,
            data_page,
            groups: RefCell::new(Vec::new()),
        });
        rebuild(&context);
        build_general_page(&context);

        dialog.present(Some(parent));
    }
}

/// The sections down a sidebar, the selected one beside it.
///
/// A sidebar rather than the preferences dialog's own view switcher, because
/// the dialog fills most of the window and a switcher across its top leaves
/// that width to margins.
fn split_view(
    dialog: &adw::Dialog,
    sections: &[(&str, &str, gtk::Widget)],
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
        content.replace(std::slice::from_ref(page));
        view.set_show_content(true);
    });
    list.select_row(list.row_at_index(0).as_ref());

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
/// right-clicking a strip, which you have to know about to find.
fn resolutions_group(context: &Rc<Context>) -> adw::PreferencesGroup {
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
    group
}

fn theme_group(context: &Rc<Context>) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::new();
    group.set_title("Theme");
    let theming = context.theming.borrow();

    let ids: Vec<String> = theming.themes().iter().map(|t| t.id.clone()).collect();
    // The entry that tracks the desktop is "System", the way every other
    // setting on this platform spells it. Labelled with the desktop theme's
    // own name it was indistinguishable from a fixed preset, and the
    // difference between them is the whole point: one keeps changing.
    let names: Vec<String> = theming
        .themes()
        .iter()
        .map(|t| {
            if t.id == omacharts_engine::theme::OMARCHY_ID {
                "System".to_string()
            } else {
                t.name.clone()
            }
        })
        .collect();
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
    let on_apply = context.on_apply_chart_defaults.clone();
    let line = footer.clone();
    apply.connect_clicked(move |_| {
        on_apply();
        offer(&line, false);
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
    crate::ui::chart_settings::fill_colourings(&colouring, current.bar_style, current.colouring);

    {
        let changed = changed.clone();
        let colouring = colouring.clone();
        let defaults = defaults.clone();
        style.connect_selected_notify(move |row| {
            let Some(chosen) = BarStyle::ALL.get(row.selected() as usize).copied() else { return };
            changed(&|d| d.bar_style = chosen);
            crate::ui::chart_settings::fill_colourings(&colouring, chosen, defaults.get().colouring);
        });
    }
    {
        let changed = changed.clone();
        let defaults = defaults.clone();
        colouring.connect_selected_notify(move |row| {
            if crate::ui::chart_settings::filling_colourings(row) {
                return;
            }
            let style = defaults.get().bar_style;
            let Some(chosen) = BarColouring::offered(style).get(row.selected() as usize).copied()
            else {
                return;
            };
            if chosen != defaults.get().colouring.for_style(style) {
                changed(&|d| d.colouring = chosen);
            }
        });
    }
    bars.add(&style);
    bars.add(&colouring);

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

    drawings.add(&footer);
    for group in [&bars, &scales, &hours, &drawings] {
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

/// Everything that is not about how the app looks: what the charts offer, and
/// where their data comes from.
fn build_general_page(context: &Rc<Context>) {
    // First thing in the dialog: whether Omacharts is in the bar is the one
    // setting about the app rather than about a chart, and it is the one
    // people come looking for.
    if let Some(group) = desktop_group() {
        context.general_page.add(&group);
    }
    context.charts_other_page.add(&resolutions_group(context));
    context.general_page.add(&screenshots_group(context));
    build_charts_page(context);
    build_data_page(context);
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

    crate::ui::feed_settings::build(
        &context.data_page,
        context.store.clone(),
        context.on_change.clone(),
        group,
    );
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

fn string_list(items: &[String]) -> gtk::StringList {
    let list = gtk::StringList::new(&[]);
    for item in items {
        list.append(item);
    }
    list
}

/// "Midnight" -> ("midnight-copy", "Midnight copy"), avoiding ids in use.
/// Asks for a theme's name, offering `initial`, and hands back what was
/// typed. Blank is not an answer: a theme with no name is one nobody can pick
/// out of the list again.
fn ask_name(
    anchor: &gtk::Button,
    heading: &str,
    initial: &str,
    verb: &str,
    done: impl Fn(String) + 'static,
) {
    let name = adw::EntryRow::new();
    name.set_title("Name");
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

    let gate = dialog.clone();
    name.connect_changed(move |field| {
        gate.set_response_enabled("ok", !field.text().trim().is_empty());
    });
    // An entry row keeps Return for itself; typing a name and pressing it is
    // one gesture and should finish the one thing.
    let finish = dialog.clone();
    name.connect_entry_activated(move |field| {
        if !field.text().trim().is_empty() {
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

fn describe_bytes(bytes: i64) -> String {
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
