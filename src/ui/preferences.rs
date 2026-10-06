//! Settings.
//!
//! Two choices sit at the top because they are the two people actually make:
//! the overall theme, and what candles look like. Everything below is the
//! same two things taken apart — every colour a theme defines is editable,
//! once the theme is yours to edit.
//!
//! Built-ins and the Omarchy theme are not editable in place. "Duplicate"
//! makes a copy you own, which is both simpler to reason about and means a
//! fiddled palette can always be abandoned by switching back.

use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;
use gtk::{gio, glib};
use omacharts_engine::theme::{
    BarScheme, BarSlot, Source, Theme, UiSlot, FALLBACK_THEME_ID, OMARCHY_ID, SWATCH_NAMES,
    THEME_BARS_ID, THEME_MONO_ID, THEME_RED_UP_ID,
};

use crate::cache;
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
    page: adw::PreferencesPage,
    general_page: adw::PreferencesPage,
    groups: RefCell<Vec<adw::PreferencesGroup>>,
}

impl Preferences {
    pub fn present(
        parent: &impl IsA<gtk::Widget>,
        store: Rc<Store>,
        theming: Rc<RefCell<Theming>>,
        on_change: Rc<dyn Fn()>,
        on_edit_resolutions: Rc<dyn Fn()>,
    ) {
        let dialog = adw::PreferencesDialog::new();
        dialog.set_title("Preferences");
        dialog.set_content_width(560);
        dialog.set_content_height(640);

        let page = adw::PreferencesPage::new();
        page.set_title("Appearance");
        page.set_icon_name(Some("applications-graphics-symbolic"));

        let general_page = adw::PreferencesPage::new();
        general_page.set_title("General");
        general_page.set_icon_name(Some("preferences-system-symbolic"));

        // General first: what the app does, before what it looks like.
        dialog.add(&general_page);
        dialog.add(&page);

        let context = Rc::new(Context {
            store,
            theming,
            on_change,
            on_edit_resolutions,
            page,
            general_page,
            groups: RefCell::new(Vec::new()),
        });
        rebuild(&context);
        build_general_page(&context);

        dialog.present(Some(parent));
    }
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
        for name in UiSlot::GROUPS {
            groups.push(theme_colors_group(context, name));
        }
        groups.push(palette_group(context));
    }

    groups.push(bars_group(context));
    let scheme = context.theming.borrow().bar_scheme();
    if scheme.source.is_editable() {
        for name in BarSlot::GROUPS {
            groups.push(bar_colors_group(context, name));
        }
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
    group.set_description(Some(if theming.omarchy_available() {
        "“System” tracks your Omarchy theme as you change it. \
         The others are fixed, and stay put whatever the desktop does."
    } else {
        "How the whole app looks."
    }));

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
    // Both states say what they are, not just the tracking one. A fixed theme
    // looks exactly like a tracking one until the desktop changes and nothing
    // happens — which is a confusing way to find out you picked a copy.
    row.set_subtitle(&if is_system {
        format!("Following {following}")
    } else {
        format!("{following} · fixed, the desktop will not change it")
    });
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

    group.add(&duplicate_row(context, source, true));
    group
}

fn bars_group(context: &Rc<Context>) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::new();
    group.set_title("Bars");
    group.set_description(Some("Candle colours, chosen separately from the theme."));

    let theming = context.theming.borrow();
    let schemes = theming.bar_schemes();
    let ids: Vec<String> = schemes.iter().map(|s| s.id.clone()).collect();
    let names: Vec<String> = schemes
        .iter()
        .map(|s| {
            if s.id == THEME_BARS_ID {
                "Match theme".to_string()
            } else {
                s.name.clone()
            }
        })
        .collect();
    let selected = ids.iter().position(|id| id == theming.scheme_id()).unwrap_or(0);
    let source = theming.bar_scheme().source;
    let scheme_id = theming.scheme_id().to_string();
    drop(theming);

    group.add(&colouring_row(context, &scheme_id));

    let row = adw::ComboRow::new();
    row.set_title("Bar scheme");
    row.set_model(Some(&string_list(&names)));
    row.set_selected(selected as u32);

    let ctx = context.clone();
    let choices = ids.clone();
    row.connect_selected_notify(move |row| {
        let Some(id) = choices.get(row.selected() as usize) else { return };
        ctx.theming.borrow_mut().select_bar_scheme(id, &ctx.store);
        apply(&ctx);
        rebuild(&ctx);
    });
    group.add(&row);

    group.add(&duplicate_row(context, source, false));
    group
}

/// Which scheme to go back to when the colour is put back.
///
/// Red up and Monochrome are schemes like any other, so choosing one would
/// otherwise throw away whatever was selected before — and somebody trying it
/// out and changing their mind would land on the default rather than on the
/// palette they had spent time picking.
const SETTING_COLOURED_BARS: &str = "coloured_bar_scheme";

/// Colour or no colour, and which way round, said in those terms.
///
/// The scheme list below can already express this — red-up and monochrome are
/// two of its entries — but only if you know that is what you are looking
/// for. This is the question people actually arrive with, so it is asked
/// plainly and the list is left to the people who want to choose a palette.
///
/// Red-up is for Taiwan, mainland China, Japan and Korea, where a rise is red
/// and a fall green.
fn colouring_row(context: &Rc<Context>, scheme_id: &str) -> adw::ComboRow {
    // The scheme behind every answer but the first, in the order they are
    // listed. The first answer is whichever coloured scheme was in use
    // before, which has to be looked up rather than named here.
    const SPECIAL: [&str; 2] = [THEME_RED_UP_ID, THEME_MONO_ID];
    let row = adw::ComboRow::new();
    row.set_title("Bar colours");
    // No subtitle: the answers say what they do, and a sentence beside them
    // squeezed the list down to an ellipsis — the one part of the row that
    // had to be readable.
    row.set_model(Some(&string_list(&[
        "Up and down".to_string(),
        "Red up".to_string(),
        "Monochrome".to_string(),
    ])));
    row.set_selected(SPECIAL.iter().position(|id| *id == scheme_id).map_or(0, |i| i as u32 + 1));

    let ctx = context.clone();
    row.connect_selected_notify(move |row| {
        {
            let mut theming = ctx.theming.borrow_mut();
            let current = theming.scheme_id().to_string();
            let chosen = row.selected().checked_sub(1).and_then(|i| SPECIAL.get(i as usize));
            let wanted = match chosen {
                Some(id) => id.to_string(),
                None => ctx
                    .store
                    .setting(SETTING_COLOURED_BARS)
                    .unwrap_or_else(|| THEME_BARS_ID.to_string()),
            };
            if wanted == current {
                return;
            }
            // Leaving a palette somebody picked: remember it to come back to.
            if !SPECIAL.contains(&current.as_str()) {
                ctx.store.set_setting(SETTING_COLOURED_BARS, &current);
            }
            theming.select_bar_scheme(&wanted, &ctx.store);
        }
        apply(&ctx);
        rebuild(&ctx);
    });
    row
}

/// "Duplicate" for things you cannot edit, "Delete" for the ones you can.
fn duplicate_row(context: &Rc<Context>, source: Source, is_theme: bool) -> adw::ActionRow {
    let row = adw::ActionRow::new();
    if source.is_editable() {
        row.set_title("This is yours to edit");
        row.set_subtitle(if is_theme {
            "Colours below are saved as you change them. Deleting goes back to System."
        } else {
            "Candle colours below are saved as you change them."
        });

        let delete = gtk::Button::with_label("Delete");
        delete.add_css_class("destructive-action");
        delete.set_valign(gtk::Align::Center);
        let ctx = context.clone();
        delete.connect_clicked(move |_| {
            let mut theming = ctx.theming.borrow_mut();
            if is_theme {
                let id = theming.theme().id;
                ctx.store.delete_theme(&id);
                theming.reload_custom(&ctx.store);
                // Back to System, which is where every copy came from. Handing
                // over to whichever preset happens to sort first would leave
                // the app on a theme nobody chose.
                let back = if theming.omarchy_available() {
                    OMARCHY_ID
                } else {
                    FALLBACK_THEME_ID
                };
                theming.select_theme(back, &ctx.store);
            } else {
                let id = theming.bar_scheme().id;
                ctx.store.delete_bar_scheme(&id);
                theming.reload_custom(&ctx.store);
                theming.select_bar_scheme(THEME_BARS_ID, &ctx.store);
            }
            drop(theming);
            apply(&ctx);
            rebuild(&ctx);
        });
        row.add_suffix(&delete);
    } else {
        row.set_title("Make it yours");
        // Said in terms of what this group is about. Both rows claiming to edit
        // "the theme colours" is wrong for the one that edits candles.
        row.set_subtitle(if is_theme {
            "Copy the theme and edit its colours. The copy stays put when the desktop changes."
        } else {
            "Copy this scheme and edit its candle colours."
        });

        let button = gtk::Button::with_label("Customize");
        button.set_valign(gtk::Align::Center);
        let ctx = context.clone();
        button.connect_clicked(move |_| {
            let mut theming = ctx.theming.borrow_mut();
            if is_theme {
                let base = theming.theme();
                let (id, name) = unique_name(
                    &base.name,
                    &theming.themes().iter().map(|t| t.id.clone()).collect::<Vec<_>>(),
                );
                let copy: Theme = base.duplicate(id.clone(), name);
                ctx.store.save_theme(&copy);
                theming.reload_custom(&ctx.store);
                theming.select_theme(&id, &ctx.store);
            } else {
                let base = theming.bar_scheme();
                let (id, name) = unique_name(
                    &base.name,
                    &theming.bar_schemes().iter().map(|s| s.id.clone()).collect::<Vec<_>>(),
                );
                let copy: BarScheme = base.duplicate(id.clone(), name);
                ctx.store.save_bar_scheme(&copy);
                theming.reload_custom(&ctx.store);
                theming.select_bar_scheme(&id, &ctx.store);
            }
            drop(theming);
            apply(&ctx);
            rebuild(&ctx);
        });
        row.add_suffix(&button);
    }
    row
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

fn bar_colors_group(context: &Rc<Context>, group_name: &str) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::new();
    group.set_title(group_name);

    let scheme = context.theming.borrow().bar_scheme();
    for slot in BarSlot::ALL.into_iter().filter(|s| s.group() == group_name) {
        let row = adw::ActionRow::new();
        row.set_title(slot.label());
        if matches!(slot, BarSlot::UpFill | BarSlot::DownFill) {
            row.set_subtitle("Fully transparent draws a hollow candle.");
        }

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
            let mut scheme = ctx.theming.borrow().bar_scheme();
            if !scheme.source.is_editable() {
                return;
            }
            BarSlot::set(slot, &mut scheme, hex);
            ctx.store.save_bar_scheme(&scheme);
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
    context.general_page.add(&resolutions_group(context));
    context.general_page.add(&screenshots_group(context));
    build_market_data(context);
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

fn build_market_data(context: &Rc<Context>) {
    let group = adw::PreferencesGroup::new();
    group.set_title("Market data");

    let provider = adw::ActionRow::new();
    provider.set_title("Provider");
    let feed = crate::feeds::stored(&context.store);
    provider.set_subtitle(&format!("{} · {}", feed.label, feed.summary));
    group.add(&provider);

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

    context.general_page.add(&group);
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
fn unique_name(base: &str, taken: &[String]) -> (String, String) {
    let slug: String = base
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect();
    for n in 1.. {
        let (id, name) = if n == 1 {
            (format!("{slug}-copy"), format!("{base} copy"))
        } else {
            (format!("{slug}-copy-{n}"), format!("{base} copy {n}"))
        };
        if !taken.contains(&id) {
            return (id, name);
        }
    }
    unreachable!()
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
    fn copies_get_names_that_are_not_taken() {
        let (id, name) = unique_name("Midnight", &[]);
        assert_eq!((id.as_str(), name.as_str()), ("midnight-copy", "Midnight copy"));

        let (id, name) = unique_name("Midnight", &["midnight-copy".into()]);
        assert_eq!((id.as_str(), name.as_str()), ("midnight-copy-2", "Midnight copy 2"));
    }

    #[test]
    fn names_with_punctuation_still_make_valid_ids() {
        let (id, _) = unique_name("Blue / Orange", &[]);
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
