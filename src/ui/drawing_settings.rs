//! The properties of one drawing, and the nine configurations of each kind.
//!
//! Two dialogs that share one editor. A drawing's properties are reached by
//! right-clicking it or pressing Enter on it, and are about that drawing
//! alone; a kind's configurations are reached by right-clicking its tool,
//! and are about every drawing that follows them. Both apply as they are
//! chosen — the chart is right there behind the dialog, and seeing the line
//! turn amber is the whole of the feedback — so there is nothing to confirm.
//! Done and Ctrl+Enter close; Escape closes the way it closes every dialog.
//!
//! A drawing follows a configuration until a property of its own is set, at
//! which point it keeps its own look; the dialog then offers to save that
//! look as a configuration, which moves every drawing following that number
//! with it. Colours are the nine presets, roles the theme fills, with a hex
//! as the way out for somebody who wants exactly one colour and knows the
//! theme will not follow it.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use adw::prelude::*;
use gtk::{gio, glib};
use omacharts_engine::drawings::{
    self, Arrow, ArrowHead, Configurations, Kind, Paint, Place, Preset, ProfileAlign, ProfileShape, Style, Text, VwapSource,
    LineStyle, Stroke, CONFIGURATIONS,
};
use omacharts_engine::Theme;

use crate::store::Store;
use crate::ui::chart_settings::{self, Ink};
use crate::ui::controls;
use crate::ui::colors;
use crate::ui::dialogs;
use crate::ui::palette;
use crate::ui::pane::ChartPane;
use crate::ui::window::Window;
pub use omacharts_chart::shapes::{paint_head, rounded};

/// The size of a configuration's preview tile.
const PREVIEW_W: i32 = 84;
const PREVIEW_H: i32 = 44;
/// The swatch on a menu row.
const SWATCH_W: i32 = 30;
const SWATCH_H: i32 = 12;

/// Something that shows a row a value: the editor keeps one per row so a
/// style chosen elsewhere can be put in front of the hand.
type Shower<T> = Rc<dyn Fn(&T)>;

/// A row and the way to show it a style: what every builder in here hands
/// back, and what the editor collects.
type StyleRow = (adw::ActionRow, Shower<Style>);

/// The way to put a drawing, as it now is, in front of the Text tab.
type ShowText = Rc<dyn Fn(&omacharts_engine::Drawing, &Style)>;

/// The Text tab, and the way to show it a drawing as it now is.
type TextTab = (adw::PreferencesPage, ShowText);

// ---------------------------------------------------------------------------
// A drawing's properties
// ---------------------------------------------------------------------------

/// Open the properties of the drawing selected on `pane` — of all of them,
/// when several of one kind are: what is set here goes on each. Nothing
/// selected, or lines and rectangles together, and nothing opens.
pub fn present(window: &Rc<Window>, store: &Rc<Store>, pane: &Rc<ChartPane>) {
    let view = pane.view.clone();
    if view.selection_kind().is_none() {
        return;
    }
    let Some(drawing) = view.selected_drawing() else { return };
    // The chart's, which may wear a theme of its own.
    let theme = view.theme();
    let kind = drawing.kind;
    let configs_now = window.drawing_configurations();

    let page = adw::PreferencesPage::new();

    // What it looks like, across the top: a picture of the drawing in its
    // configuration — or in its own look — with the number, or "Custom",
    // as a tag in the middle of it. The whole picture is the button that
    // opens the choice, and it repaints as a property below changes.
    let following = adw::PreferencesGroup::new();
    let shown_preview = gtk::DrawingArea::new();
    shown_preview.set_hexpand(true);
    shown_preview.set_size_request(-1, 112);
    shown_preview.add_css_class("drawing-preview");
    paint_preview(&shown_preview, &theme, kind, drawing.style(&configs_now), &drawing.text);
    let shown_label = gtk::Label::new(None);
    shown_label.add_css_class("drawing-config-tag");
    shown_label.set_halign(gtk::Align::Start);
    shown_label.set_valign(gtk::Align::Start);
    shown_label.set_margin_start(8);
    shown_label.set_margin_top(8);
    // A sheet over the picture that is nothing until the pointer is on the
    // button, and then a tint and a ring: the picture paints its own ground
    // over the button's, so the button's own hover would never show.
    let hover = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    hover.add_css_class("drawing-preview-hover");
    hover.set_can_target(false);
    let shown = gtk::Overlay::new();
    shown.set_child(Some(&shown_preview));
    shown.add_overlay(&hover);
    shown.add_overlay(&shown_label);
    let picker = gtk::MenuButton::new();
    picker.set_child(Some(&shown));
    picker.set_hexpand(true);
    picker.add_css_class("flat");
    picker.add_css_class("drawing-config-picker");
    picker.set_cursor_from_name(Some("pointer"));
    picker.set_tooltip_text(Some("Choose a configuration, shown as it will look"));
    following.add(&picker);

    // The way back from a look of its own: write it over configuration N.
    // Under the picture, off to the right, and no louder than a link: it
    // is there while the look is the drawing's own and not otherwise.
    let save = gtk::MenuButton::new();
    save.set_label("Save as…");
    save.add_css_class("flat");
    save.add_css_class("drawing-save-as");
    save.set_halign(gtk::Align::End);
    let save_row = save.clone();
    following.add(&save);
    page.add(&following);

    // What the row above shows, from the drawing as it is now. Bound late,
    // since the editor below calls it and is built before it.
    type Refresh = Rc<RefCell<Option<Rc<dyn Fn()>>>>;
    let refresh: Refresh = Rc::new(RefCell::new(None));
    let call_refresh = {
        let refresh = refresh.clone();
        Rc::new(move || {
            if let Some(f) = refresh.borrow().as_ref() {
                f();
            }
        })
    };

    // The look itself. Edits here are the drawing's own from then on.
    let on_style: Rc<dyn Fn(Style)> = {
        let view = view.clone();
        let window = window.clone();
        let call_refresh = call_refresh.clone();
        Rc::new(move |style: Style| {
            let configs = window.drawing_configurations();
            view.edit_selected(move |d| {
                d.edit_style(&configs, |own| *own = style.clone());
            });
            call_refresh();
        })
    };
    let editor = style_editor(theme.clone(), kind, drawing.style(&configs_now).clone(), on_style.clone());
    page.add(&editor.group);
    for group in &editor.more {
        page.add(group);
    }

    // A figure's words are a subject of their own, behind a tab. A text
    // drawing *is* its words, and `style_editor` has already put the three
    // rows on this page: a tab there would be a tab over nothing.
    let text_tab = (!kind.is_text() && kind.takes_text()).then(|| {
        let on_text: Rc<dyn Fn(Text)> = {
            let view = view.clone();
            let call_refresh = call_refresh.clone();
            Rc::new(move |text: Text| {
                view.edit_selected(move |d| d.set_text(text.clone()));
                // The picture above shows this drawing's own words, so it
                // has to be redrawn as they are typed. Without this it
                // caught up on the dialog's quarter-second tick, which is
                // long enough to read as the preview lagging the field.
                call_refresh();
            })
        };
        text_page(theme.clone(), &drawing, drawing.style(&configs_now).clone(), on_style, on_text)
    });

    // No row for who else sees it. Where a drawing goes is the chart's
    // business now — two switches at the foot of its settings — and asking
    // the same question again of every drawing was most of what made the
    // old arrangement heavy.

    // Removing is the chart's business — Delete, or the drawing's menu —
    // not a button at the bottom of its properties.
    let dialog = adw::Dialog::new();
    dialog.set_title(&format!("{} properties", kind.label()));
    dialog.set_content_width(440);

    let show_text = text_tab.as_ref().map(|(_, show)| show.clone());
    *refresh.borrow_mut() = Some({
        let view = view.clone();
        let label = shown_label.clone();
        let theme = theme.clone();
        let preview = shown_preview.clone();
        let save_row = save_row.clone();
        let editor = editor.clone();
        let window = window.clone();
        Rc::new(move || {
            let Some(d) = view.selected_drawing() else { return };
            let configs = window.drawing_configurations();
            match d.config {
                Some(n) => {
                    label.set_text(&format!("{n}"));
                    save_row.set_visible(false);
                }
                None => {
                    label.set_text("Custom");
                    // The pill stays one word — it has to read at a glance
                    // over a picture — and says the rest when asked.
                    label.set_tooltip_text(Some(&match d.started_from {
                        Some(n) => format!("Its own look, from configuration {n}"),
                        None => "Its own look".to_string(),
                    }));
                    offer_save_as(&save_row, d.started_from);
                    save_row.set_visible(true);
                }
            }
            let style = d.style(&configs);
            paint_preview(&preview, &theme, kind, style, &d.text);
            editor.show(style);
            if let Some(show_text) = show_text.as_ref() {
                show_text(&d, style);
            }
        })
    });
    call_refresh();

    // The nine as pictures, and this drawing's own look beside them while it
    // has one. Built each time the menu opens, so it shows the
    // configurations as they are now and the custom tile only while there
    // is something custom to show.
    let popover = gtk::Popover::new();
    {
        let view = view.clone();
        let window = window.clone();
        let theme = theme.clone();
        let call_refresh = call_refresh.clone();
        popover.connect_show(move |popover| {
            let Some(d) = view.selected_drawing() else { return };
            let configs = window.drawing_configurations();
            let custom = d.style.clone().filter(|_| d.config.is_none());
            let grid = configuration_grid(&theme, kind, &configs, d.config, custom.as_ref(), &d.text, {
                let view = view.clone();
                let popover = popover.clone();
                let call_refresh = call_refresh.clone();
                move |n| {
                    view.apply_configuration(n);
                    popover.popdown();
                    call_refresh();
                }
            });
            popover.set_child(Some(&grid));
        });
    }
    picker.set_popover(Some(&popover));

    // Under the button, between the picture and the rest of the sheet,
    // where the eye already is; not up over the picture it was just given.
    save.set_direction(gtk::ArrowType::Down);
    offer_save_as(&save, drawing.started_from);
    let actions = gio::SimpleActionGroup::new();
    let save_as = gio::SimpleAction::new("save-as", Some(glib::VariantTy::INT32));
    {
        let window = window.clone();
        let store = store.clone();
        let view = view.clone();
        let call_refresh = call_refresh.clone();
        save_as.connect_activate(move |_, target| {
            let Some(n) = target.and_then(|t| t.get::<i32>()) else { return };
            let Some(d) = view.selected_drawing() else { return };
            let mut configs = window.drawing_configurations();
            let own = d.style(&configs).clone();
            configs.set(d.kind, n as u8, own);
            window.set_drawing_configurations(&store, configs);
            view.apply_configuration(n as u8);
            call_refresh();
        });
    }
    actions.add_action(&save_as);
    dialog.insert_action_group("drawing", Some(&actions));

    // Alt+N here is the same choice as a tile in the menu: the keyboard is
    // in the dialog, so the chart would never see it. Caught on the way
    // down, before a row that happens to have the focus can.
    {
        let keys = gtk::EventControllerKey::new();
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        let view = view.clone();
        let call_refresh = call_refresh.clone();
        keys.connect_key_pressed(move |_, key, _, modifiers| {
            if !modifiers.contains(gtk::gdk::ModifierType::ALT_MASK) {
                return glib::Propagation::Proceed;
            }
            let Some(n) = key.to_unicode().and_then(|c| c.to_digit(10)) else {
                return glib::Propagation::Proceed;
            };
            if view.apply_configuration(n as u8) {
                call_refresh();
            }
            glib::Propagation::Stop
        });
        dialog.add_controller(keys);
    }

    // A change made with the keys on the chart while this is open — Alt+N,
    // Ctrl+Z — is reflected here too.
    let tick = {
        let call_refresh = call_refresh.clone();
        let view = view.clone();
        let last = RefCell::new(view.selected_drawing());
        glib::timeout_add_local(std::time::Duration::from_millis(250), move || {
            let now = view.selected_drawing();
            if now != *last.borrow() {
                *last.borrow_mut() = now;
                call_refresh();
            }
            glib::ControlFlow::Continue
        })
    };

    match text_tab {
        // One subject, one page, no switcher to put over it.
        None => finish(&dialog, &page, None, &view.area, Some(tick)),
        Some((text_page, _)) => {
            // A plain stack and its switcher, not `AdwViewSwitcher`: that one
            // always draws an icon beside the label, and two tabs about a
            // drawing's shape and its words have no icons that would say
            // anything a word does not. Asked for none it drew the
            // "no symbol" placeholder beside both, which is how this was
            // found. A `GtkStackSwitcher` shows the titles and nothing else.
            let stack = gtk::Stack::new();
            stack.set_vexpand(true);
            // "General" rather than the kind's name: the dialog's title
            // already says which kind this is.
            stack.add_titled(&page, Some("general"), "General");
            stack.add_titled(&text_page, Some("text"), "Text");
            let switcher = gtk::StackSwitcher::new();
            switcher.set_stack(Some(&stack));
            {
                // Alt and an arrow between Look and Text, and the keyboard
                // goes with it.
                let stack = stack.clone();
                dialogs::step_tabs_with_alt(&dialog, move |forward| {
                    let pages = stack.pages();
                    let count = pages.n_items();
                    let at = (0..count)
                        .find(|n| {
                            pages
                                .item(*n)
                                .and_downcast::<gtk::StackPage>()
                                .zip(stack.visible_child())
                                .is_some_and(|(page, shown)| page.child() == shown)
                        })
                        .unwrap_or(0);
                    let next = dialogs::step_round(at, count.max(1), forward);
                    if let Some(page) = pages.item(next).and_downcast::<gtk::StackPage>() {
                        let child = page.child();
                        stack.set_visible_child(&child);
                        focus_into(child);
                    }
                });
            }
            finish(&dialog, &stack, Some(&switcher), &view.area, Some(tick));
        }
    }
    dialog.present(Some(&window.window));
}

// ---------------------------------------------------------------------------
// A kind's configurations
// ---------------------------------------------------------------------------

/// The nine configurations of a kind: each shown as it looks, each editable,
/// and a way back to the defaults.
pub fn present_configurations(window: &Rc<Window>, store: &Rc<Store>, kind: Kind) {
    let theme = window.theme();
    let dialog = adw::Dialog::new();
    dialog.set_title(&format!("{} configurations", kind.label()));
    dialog.set_content_width(480);
    dialog.set_content_height(620);

    let navigation = adw::NavigationView::new();

    let page = adw::PreferencesPage::new();
    let group = adw::PreferencesGroup::new();
    group.set_description(Some(
        "Every drawing that follows a configuration looks like it, and changes with it. Alt+1 to Alt+9 on a selected drawing, or while a tool is in hand.",
    ));
    let rows: Rc<RefCell<Vec<(adw::ActionRow, gtk::DrawingArea, gtk::Label)>>> =
        Rc::new(RefCell::new(Vec::new()));
    for n in 1..=CONFIGURATIONS {
        let configs = window.drawing_configurations();
        let style = configs.of(kind, n).clone();
        let row = adw::ActionRow::new();
        row.set_title(&format!("Configuration {n}"));
        row.set_subtitle(&describe(kind, &style));
        row.set_activatable(true);
        let preview = preview_tile(&theme, kind, &style, &sample());
        row.add_prefix(&preview);
        // A tag rather than a word in the subtitle: the eye finds a tag
        // down a list of nine, and reads a word in nine subtitles.
        let edited = gtk::Label::new(Some("Edited"));
        edited.add_css_class("drawing-config-edited");
        edited.set_valign(gtk::Align::Center);
        edited.set_visible(!configs.is_default(kind, n));
        mark_edited(&row, !configs.is_default(kind, n));
        row.add_suffix(&edited);
        row.add_suffix(&gtk::Image::from_icon_name("go-next-symbolic"));
        {
            let window = window.clone();
            let store = store.clone();
            let navigation = navigation.clone();
            let rows = rows.clone();
            let theme = theme.clone();
            row.connect_activated(move |_| {
                let configs = window.drawing_configurations();
                let current = configs.of(kind, n).clone();
                let on_style: Rc<dyn Fn(Style)> = {
                    let window = window.clone();
                    let store = store.clone();
                    let rows = rows.clone();
                    let theme = theme.clone();
                    Rc::new(move |style: Style| {
                        let mut configs = window.drawing_configurations();
                        configs.set(kind, n, style.clone());
                        window.set_drawing_configurations(&store, configs.clone());
                        if let Some((row, preview, edited)) = rows.borrow().get(n as usize - 1) {
                            row.set_subtitle(&describe(kind, &style));
                            paint_preview(preview, &theme, kind, &style, &sample());
                            let changed = !configs.is_default(kind, n);
                            edited.set_visible(changed);
                            mark_edited(row, changed);
                        }
                    })
                };
                let editor = style_editor(window.theme(), kind, current, on_style);
                let sub = adw::PreferencesPage::new();
                sub.add(&editor.group);
                for group in &editor.more {
                    sub.add(group);
                }
                if let Some(text) = &editor.text {
                    sub.add(text);
                }
                let toolbar = adw::ToolbarView::new();
                toolbar.add_top_bar(&adw::HeaderBar::new());
                toolbar.set_content(Some(&sub));
                let page = adw::NavigationPage::new(&toolbar, &format!("Configuration {n}"));
                navigation.push(&page);
            });
        }
        rows.borrow_mut().push((row.clone(), preview, edited));
        group.add(&row);
    }
    page.add(&group);

    let restore_group = adw::PreferencesGroup::new();
    let restore = gtk::Button::with_label("Restore defaults");
    restore.set_halign(gtk::Align::Start);
    restore.set_tooltip_text(Some("The nine as they shipped: the presets, in order"));
    {
        let window = window.clone();
        let store = store.clone();
        let rows = rows.clone();
        let theme = theme.clone();
        restore.connect_clicked(move |_| {
            let mut configs = window.drawing_configurations();
            configs.reset(kind);
            window.set_drawing_configurations(&store, configs.clone());
            for (n, (row, preview, edited)) in rows.borrow().iter().enumerate() {
                let n = n as u8 + 1;
                let style = configs.of(kind, n);
                row.set_subtitle(&describe(kind, style));
                paint_preview(preview, &theme, kind, style, &sample());
                edited.set_visible(false);
                mark_edited(row, false);
            }
        });
    }
    restore_group.add(&restore);
    page.add(&restore_group);

    let header = adw::HeaderBar::new();
    header.set_show_start_title_buttons(false);
    header.set_show_end_title_buttons(false);
    let done = gtk::Button::with_label("Done");
    done.add_css_class("suggested-action");
    done.set_tooltip_text(Some(&format!("Done ({})", dialogs::commit_label())));
    let dialog_for_done = dialog.clone();
    done.connect_clicked(move |_| {
        let _ = dialog_for_done.close();
    });
    header.pack_end(&done);
    let root = adw::ToolbarView::new();
    root.add_top_bar(&header);
    root.set_content(Some(&page));
    navigation.add(&adw::NavigationPage::new(&root, &format!("{} configurations", kind.label())));
    dialog.set_child(Some(&navigation));

    let done_on_key = done.clone();
    dialogs::commit_on_ctrl_enter(&dialog, move || done_on_key.emit_clicked());
    dialog.present(Some(&window.window));
}

/// A row that is not as shipped wears a stripe of the accent down its
/// edge, as well as its tag, so the changed ones stand out at a glance.
fn mark_edited(row: &adw::ActionRow, edited: bool) {
    if edited {
        row.add_css_class("drawing-config-row-edited");
    } else {
        row.remove_css_class("drawing-config-row-edited");
    }
}

/// The nine, as somewhere to write this drawing's own look.
///
/// The one it was following when it stopped following anything wears a
/// star, because it is the answer somebody is most often looking for: a
/// look arrived at by nudging Configuration 4 usually wants to go back over
/// Configuration 4, and without the mark there is nothing on the screen
/// that says which 4 it was. A star rather than a word, since it is one row
/// of nine that reads differently and nothing about it needs explaining
/// twice.
fn offer_save_as(button: &gtk::MenuButton, started_from: Option<u8>) {
    crate::ui::menus::set_model(button, &save_menu(started_from));
}

fn save_menu(started_from: Option<u8>) -> gio::Menu {
    let menu = gio::Menu::new();
    for n in 1..=CONFIGURATIONS {
        let label = match started_from == Some(n) {
            true => format!("Configuration {n} *"),
            false => format!("Configuration {n}"),
        };
        let item = gio::MenuItem::new(Some(&label), None);
        item.set_action_and_target_value(Some("drawing.save-as"), Some(&(n as i32).to_variant()));
        menu.append_item(&item);
    }
    menu
}

/// Put the keyboard in a page just switched to.
///
/// On the next turn of the loop, because the page has not been laid out yet
/// and has nothing to give the focus to until it has. Leaving the focus on
/// a row of the page no longer showing means the next Tab walks a tab
/// nobody is looking at.
fn focus_into(page: impl IsA<gtk::Widget> + 'static) {
    glib::idle_add_local_once(move || {
        page.as_ref().child_focus(gtk::DirectionType::TabForward);
    });
}

/// A configuration in words, for the row under its number.
fn describe(kind: Kind, style: &Style) -> String {
    match kind {
        Kind::Line | Kind::Horizontal | Kind::Arrow | Kind::Zigzag => {
            let arrow = match (style.arrow, style.head) {
                (Arrow::None, _) => String::new(),
                (arrow, ArrowHead::Filled) => format!(", arrow {}", arrow.label().to_lowercase()),
                (arrow, head) => {
                    format!(", {} arrow {}", head.label().to_lowercase(), arrow.label().to_lowercase())
                }
            };
            format!("{}, {}px{arrow}", name_of(&style.colour), style.width)
        }
        Kind::Rect | Kind::Ellipse => {
            let edge = match style.border {
                true => format!(", edge {} {}px", name_of(&style.colour), style.width),
                false => ", no edge".to_string(),
            };
            format!("{} at {:.0}%{edge}", name_of(&style.fill), style.alpha * 100.0)
        }
        // A text configuration is its three text properties and nothing
        // else, so they are what the row says. The face is named only when
        // it is not the desktop's, which is the one nobody chose.
        Kind::Text => {
            let face = match style.text.family_name() {
                Some(family) => format!(", {family}"),
                None => String::new(),
            };
            format!("{}, {:.0}px{face}", name_of(&style.text.colour), style.text.clamped_size())
        }
        Kind::Profile => {
            let look = &style.profile;
            let background = match style.fill == style.colour {
                true => String::new(),
                false => format!(" on {}", name_of(&style.fill)),
            };
            format!(
                "{}{background} {}, from the {}, point of control {}",
                name_of(&style.colour),
                look.shape.label().to_lowercase(),
                look.align.label().to_lowercase(),
                name_of(&look.poc)
            )
        }
        Kind::Vwap => format!(
            "{}, {}px{}, {}{}",
            name_of(&style.colour),
            style.width,
            match style.vwap.line {
                LineStyle::Solid => String::new(),
                other => format!(" {}", other.label().to_lowercase()),
            },
            style.vwap.source.label().to_lowercase(),
            vwap_bands(style)
        ),
        Kind::Fib => fib_summary(style),
    }
}

/// A retracement in words: its colour, then how its levels are drawn when
/// that is not all of them as a solid hairline in it, and whether it is
/// labelled.
fn fib_summary(style: &Style) -> String {
    let look = &style.fib;
    let mut words = vec![name_of(&style.colour)];
    let shown: Vec<String> = look.enabled().map(|level| level.name()).collect();
    if shown.len() < look.levels.len() {
        words.push(format!("levels {}", shown.join(" ")));
    }
    if let Some(width) = look.width()
        && width != drawings::FIB_WIDTH
    {
        words.push(format!("{width}px"));
    }
    let patterns: Vec<LineStyle> = look.enabled().map(|level| level.stroke.style).collect();
    match patterns.first() {
        Some(first) if patterns.iter().all(|p| p == first) && *first != LineStyle::Solid => {
            words.push(first.label().to_lowercase())
        }
        Some(_) if patterns.iter().any(|p| *p != patterns[0]) => words.push("mixed lines".to_string()),
        _ => {}
    }
    if look.levels.iter().any(|level| level.colour.is_some()) {
        words.push("own colours".to_string());
    }
    match (look.fib_labels, look.price_labels) {
        (true, false) => {}
        (true, true) => words.push("prices".to_string()),
        (false, true) => words.push("prices, no ratios".to_string()),
        (false, false) => words.push("no labels".to_string()),
    }
    words.join(", ")
}

/// An anchored VWAP's bands in words: `±1σ, ±2σ`, or nothing for none.
fn vwap_bands(style: &Style) -> String {
    let bands: Vec<String> = style.vwap.shown().map(|band| format!("±{}σ", band.deviations)).collect();
    match bands.is_empty() {
        true => String::new(),
        false => format!(", bands {}", bands.join(", ")),
    }
}

fn name_of(paint: &Paint) -> String {
    match paint {
        Paint::Preset { preset } => preset.name().to_string(),
        Paint::Fixed { hex } => hex.clone(),
    }
}

// ---------------------------------------------------------------------------
// The editor the two dialogs share
// ---------------------------------------------------------------------------

/// The rows that edit a [`Style`], and a way to show a new one in them.
#[derive(Clone)]
struct Editor {
    group: adw::PreferencesGroup,
    /// The groups after it, for a kind whose properties are several
    /// subjects: an anchored VWAP's line, and each of its bands.
    more: Vec<adw::PreferencesGroup>,
    /// How the kind's words are set, for the kinds that are a shape and so
    /// keep that apart from their own rows.
    ///
    /// Handed back rather than added, because the two dialogs put it in
    /// different places: a drawing's properties give it a tab of its own,
    /// since a figure's label is a second subject; a configuration is one
    /// page and the rows simply follow the shape's. Either way they are the
    /// same rows, driven by the same editor, so `show` reaches them both.
    text: Option<adw::PreferencesGroup>,
    show: Rc<dyn Fn(&Style)>,
}

impl Editor {
    fn show(&self, style: &Style) {
        (self.show)(style);
    }
}

/// The rows for a kind's properties, their colours picked on `theme`. Every
/// change calls `on_style` with the whole style as it now is.
fn style_editor(theme: Theme, kind: Kind, current: Style, on_style: Rc<dyn Fn(Style)>) -> Editor {
    let style: Rc<RefCell<Style>> = Rc::new(RefCell::new(current));
    // Set while the rows are being shown a style, so a row's own signal does
    // not call back as if the hand had changed it.
    let showing = Rc::new(std::cell::Cell::new(false));
    // No title: the dialog's own says which kind this is, and the rows say
    // the rest.
    let group = adw::PreferencesGroup::new();

    let emit = {
        let style = style.clone();
        let on_style = on_style.clone();
        let showing = showing.clone();
        let emit: Rc<dyn Fn()> = Rc::new(move || {
            if showing.get() {
                return;
            }
            // Cloned out before the call, not inside it: a temporary borrow
            // in an argument lives to the end of the statement, and the
            // handler repaints the preview, which shows the editor the style
            // again and needs the cell for itself.
            let current = style.borrow().clone();
            on_style(current);
        });
        emit
    };

    let mut shows: Vec<Shower<Style>> = Vec::new();
    let mut more = Vec::new();

    match kind {
        Kind::Line | Kind::Horizontal | Kind::Arrow | Kind::Zigzag => {
            // Colour.
            let (row, show) = paint_row(&theme, "Colour", &style.borrow().colour, {
                let style = style.clone();
                let emit = emit.clone();
                move |paint| {
                    style.borrow_mut().colour = paint;
                    emit();
                }
            });
            group.add(&row);
            shows.push(Rc::new(move |s: &Style| show(&s.colour)));

            // Thickness.
            let (row, show) = width_row("Thickness", style.borrow().width, {
                let style = style.clone();
                let emit = emit.clone();
                move |width| {
                    style.borrow_mut().width = width;
                    emit();
                }
            });
            group.add(&row);
            shows.push(Rc::new(move |s: &Style| show(s.width)));

            // Arrow. Each choice is drawn rather than named, in the line's
            // own colour at its own width: which end "the start" is, and
            // what a head looks like on a thick line, are things to see.
            let row = adw::ComboRow::new();
            row.set_title("Arrow");
            let names: Vec<&str> = Arrow::ALL.iter().map(|a| a.label()).collect();
            row.set_model(Some(&gtk::StringList::new(&names)));
            let (factory, samples) = arrow_factory(theme.clone(), style.clone());
            row.set_factory(Some(&factory));
            row.set_selected(Arrow::ALL.iter().position(|a| *a == style.borrow().arrow).unwrap_or(0) as u32);
            {
                let style = style.clone();
                let emit = emit.clone();
                row.connect_selected_notify(move |row| {
                    let Some(arrow) = Arrow::ALL.get(row.selected() as usize).copied() else { return };
                    style.borrow_mut().arrow = arrow;
                    emit();
                });
            }
            group.add(&row);
            let row_for_show = row.clone();
            shows.push(Rc::new(move |s: &Style| {
                row_for_show.set_selected(Arrow::ALL.iter().position(|a| *a == s.arrow).unwrap_or(0) as u32);
                // The samples are in the colour, width and head of the
                // moment, which the other rows may just have changed.
                samples.borrow_mut().retain(|weak| weak.upgrade().is_some());
                for sample in samples.borrow().iter().filter_map(|weak| weak.upgrade()) {
                    sample.queue_draw();
                }
            }));

            // The head's shape, drawn too. Greyed while there is no arrow
            // to put it on.
            let row = adw::ComboRow::new();
            row.set_title("Arrowhead");
            let names: Vec<&str> = ArrowHead::ALL.iter().map(|h| h.label()).collect();
            row.set_model(Some(&gtk::StringList::new(&names)));
            let (factory, samples) = head_factory(theme.clone(), style.clone());
            row.set_factory(Some(&factory));
            row.set_selected(ArrowHead::ALL.iter().position(|h| *h == style.borrow().head).unwrap_or(0) as u32);
            row.set_sensitive(style.borrow().arrow != Arrow::None);
            {
                let style = style.clone();
                let emit = emit.clone();
                row.connect_selected_notify(move |row| {
                    let Some(shape) = ArrowHead::ALL.get(row.selected() as usize).copied() else { return };
                    style.borrow_mut().head = shape;
                    emit();
                });
            }
            group.add(&row);
            let row_for_show = row.clone();
            shows.push(Rc::new(move |s: &Style| {
                row_for_show.set_selected(ArrowHead::ALL.iter().position(|h| *h == s.head).unwrap_or(0) as u32);
                row_for_show.set_sensitive(s.arrow != Arrow::None);
                samples.borrow_mut().retain(|weak| weak.upgrade().is_some());
                for sample in samples.borrow().iter().filter_map(|weak| weak.upgrade()) {
                    sample.queue_draw();
                }
            }));
        }
        // The study's properties, as one drawing carries them.
        Kind::Profile => {
            // Filled rows wear an outline only when asked; the outline style
            // is nothing but one, so the switch is for the filled style.
            let filled = |s: &Style| s.profile.shape == ProfileShape::Filled;
            let edge = adw::SwitchRow::new();
            edge.set_title("Outline");
            edge.set_active(style.borrow().border || !filled(&style.borrow()));
            edge.set_sensitive(filled(&style.borrow()));
            group.add(&edge);

            let (outline_row, show) = paint_row(&theme, "Outline colour", &style.borrow().colour, {
                let style = style.clone();
                let emit = emit.clone();
                move |paint| {
                    style.borrow_mut().colour = paint;
                    emit();
                }
            });
            group.add(&outline_row);
            shows.push(Rc::new(move |s: &Style| show(&s.colour)));

            let (thickness_row, show) = width_row("Outline thickness", style.borrow().width, {
                let style = style.clone();
                let emit = emit.clone();
                move |width| {
                    style.borrow_mut().width = width;
                    emit();
                }
            });
            group.add(&thickness_row);
            shows.push(Rc::new(move |s: &Style| show(s.width)));

            // The outline's own rows wait for there to be an outline.
            let outlined = move |s: &Style| s.border || !filled(s);
            for row in [outline_row.clone().upcast::<gtk::Widget>(), thickness_row.clone().upcast()] {
                row.set_sensitive(outlined(&style.borrow()));
            }
            {
                let style = style.clone();
                let emit = emit.clone();
                let showing = showing.clone();
                let rows = [outline_row.clone().upcast::<gtk::Widget>(), thickness_row.clone().upcast()];
                edge.connect_active_notify(move |row| {
                    if showing.get() || !filled(&style.borrow()) {
                        return;
                    }
                    for widget in &rows {
                        widget.set_sensitive(row.is_active());
                    }
                    style.borrow_mut().border = row.is_active();
                    emit();
                });
            }
            {
                let rows = [outline_row.upcast::<gtk::Widget>(), thickness_row.upcast()];
                shows.push(Rc::new(move |s: &Style| {
                    edge.set_active(outlined(s));
                    edge.set_sensitive(filled(s));
                    for widget in &rows {
                        widget.set_sensitive(outlined(s));
                    }
                }));
            }

            let (row, show) = paint_row(&theme, "Background", &style.borrow().fill, {
                let style = style.clone();
                let emit = emit.clone();
                move |paint| {
                    style.borrow_mut().fill = paint;
                    emit();
                }
            });
            group.add(&row);
            shows.push(Rc::new(move |s: &Style| show(&s.fill)));

            // Over the outline and the background together, from the look
            // as designed at 0%: their strengths are measured, and this
            // only fades them.
            let fade = adw::ActionRow::new();
            fade.set_title("Transparency");
            let scale = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, 100.0, 1.0);
            scale.set_size_request(160, -1);
            scale.set_valign(gtk::Align::Center);
            scale.set_draw_value(true);
            scale.set_value_pos(gtk::PositionType::Left);
            scale.set_format_value_func(|_, v| format!("{v:.0}%"));
            scale.set_value((1.0 - style.borrow().profile.opacity) * 100.0);
            {
                let style = style.clone();
                let emit = emit.clone();
                scale.connect_value_changed(move |scale| {
                    style.borrow_mut().profile.opacity = (1.0 - scale.value() / 100.0).clamp(0.0, 1.0);
                    emit();
                });
            }
            fade.add_suffix(&scale);
            group.add(&fade);
            shows.push(Rc::new(move |s: &Style| scale.set_value((1.0 - s.profile.opacity) * 100.0)));

            let (row, show) = paint_row(&theme, "Point of control", &style.borrow().profile.poc, {
                let style = style.clone();
                let emit = emit.clone();
                move |paint| {
                    style.borrow_mut().profile.poc = paint;
                    emit();
                }
            });
            group.add(&row);
            shows.push(Rc::new(move |s: &Style| show(&s.profile.poc)));

            let (row, show) = width_row("Point of control thickness", style.borrow().profile.poc_width, {
                let style = style.clone();
                let emit = emit.clone();
                move |width| {
                    style.borrow_mut().profile.poc_width = width;
                    emit();
                }
            });
            group.add(&row);
            shows.push(Rc::new(move |s: &Style| show(s.profile.poc_width)));

            let shapes = ProfileShape::ALL;
            let (row, show) = named_row(
                "Style",
                &shapes.map(ProfileShape::label),
                shapes.iter().position(|s| *s == style.borrow().profile.shape),
                {
                    let style = style.clone();
                    let emit = emit.clone();
                    move |at| {
                        style.borrow_mut().profile.shape = shapes[at];
                        emit();
                    }
                },
            );
            group.add(&row);
            shows.push(Rc::new(move |s: &Style| show(shapes.iter().position(|x| *x == s.profile.shape))));

            let aligns = ProfileAlign::ALL;
            let (row, show) = named_row(
                "Alignment",
                &aligns.map(ProfileAlign::label),
                aligns.iter().position(|a| *a == style.borrow().profile.align),
                {
                    let style = style.clone();
                    let emit = emit.clone();
                    move |at| {
                        style.borrow_mut().profile.align = aligns[at];
                        emit();
                    }
                },
            );
            group.add(&row);
            shows.push(Rc::new(move |s: &Style| show(aligns.iter().position(|x| *x == s.profile.align))));

            let area = adw::SpinRow::with_range(10.0, 100.0, 5.0);
            area.set_title("Value area %");
            area.set_value(style.borrow().profile.value_area * 100.0);
            {
                let style = style.clone();
                let emit = emit.clone();
                let showing = showing.clone();
                area.connect_value_notify(move |row| {
                    if showing.get() {
                        return;
                    }
                    style.borrow_mut().profile.value_area = row.value() / 100.0;
                    emit();
                });
            }
            group.add(&area);
            shows.push(Rc::new(move |s: &Style| area.set_value(s.profile.value_area * 100.0)));
        }
        // The study's rows, in the study's order and groups: what it is
        // worked out from, the line, then a group for each band.
        Kind::Vwap => {
            group.set_title("Parameters");
            let sources = VwapSource::ALL;
            let (row, show) = named_row(
                "Source",
                &sources.map(VwapSource::label),
                sources.iter().position(|s| *s == style.borrow().vwap.source),
                {
                    let style = style.clone();
                    let emit = emit.clone();
                    move |at| {
                        style.borrow_mut().vwap.source = sources[at];
                        emit();
                    }
                },
            );
            group.add(&row);
            shows.push(Rc::new(move |s: &Style| show(sources.iter().position(|x| *x == s.vwap.source))));

            let line = adw::PreferencesGroup::new();
            line.set_title("Line");
            let ink = {
                let style = style.clone();
                let theme = theme.clone();
                Ink::new(move || style.borrow().colour.hex(&theme))
            };
            // The bands' samples too: a band with no colour of its own is
            // drawn in the line's.
            let inks: Rc<RefCell<Vec<Ink>>> = Rc::new(RefCell::new(vec![ink.clone()]));
            let (row, show) = paint_row(&theme, "Colour", &style.borrow().colour, {
                let style = style.clone();
                let emit = emit.clone();
                let inks = inks.clone();
                move |paint| {
                    style.borrow_mut().colour = paint;
                    for ink in inks.borrow().iter() {
                        ink.repaint();
                    }
                    emit();
                }
            });
            line.add(&row);
            shows.push(Rc::new(move |s: &Style| show(&s.colour)));
            let current = Stroke::new(style.borrow().width, style.borrow().vwap.line);
            for (row, show) in stroke_rows(&ink, current, &showing, {
                let style = style.clone();
                let emit = emit.clone();
                move |stroke: Stroke| {
                    let mut style = style.borrow_mut();
                    style.width = stroke.width;
                    style.vwap.line = stroke.style;
                    drop(style);
                    emit();
                }
            }) {
                line.add(&row);
                shows.push(Rc::new(move |s: &Style| show(Stroke::new(s.width, s.vwap.line))));
            }
            {
                let ink = ink.clone();
                shows.push(Rc::new(move |_: &Style| ink.repaint()));
            }
            more.push(line);

            for index in 0..drawings::VWAP_BAND_COUNT {
                let (group, band_ink, band_shows) = vwap_band_group(&theme, &style, &showing, &emit, index);
                more.push(group);
                inks.borrow_mut().push(band_ink);
                shows.extend(band_shows);
            }
        }
        // One colour for every level that has none of its own, whether the
        // levels are labelled, and then the seven levels, each a row that
        // opens on its own colour, thickness and pattern.
        Kind::Fib => {
            let inks: Rc<RefCell<Vec<Ink>>> = Rc::new(RefCell::new(Vec::new()));
            let (row, show) = paint_row(&theme, "Colour", &style.borrow().colour, {
                let style = style.clone();
                let emit = emit.clone();
                let inks = inks.clone();
                move |paint| {
                    style.borrow_mut().colour = paint;
                    for ink in inks.borrow().iter() {
                        ink.repaint();
                    }
                    emit();
                }
            });
            group.add(&row);
            shows.push(Rc::new(move |s: &Style| show(&s.colour)));

            type Get = fn(&drawings::FibLook) -> bool;
            type Set = fn(&mut drawings::FibLook, bool);
            let switches: [(&str, Get, Set); 2] = [
                ("Fib labels", |look| look.fib_labels, |look, on| look.fib_labels = on),
                ("Price labels", |look| look.price_labels, |look, on| look.price_labels = on),
            ];
            for (title, get, set) in switches {
                let row = adw::SwitchRow::new();
                row.set_title(title);
                row.set_active(get(&style.borrow().fib));
                {
                    let style = style.clone();
                    let emit = emit.clone();
                    row.connect_active_notify(move |row| {
                        set(&mut style.borrow_mut().fib, row.is_active());
                        emit();
                    });
                }
                group.add(&row);
                shows.push(Rc::new(move |s: &Style| row.set_active(get(&s.fib))));
            }

            let levels = adw::PreferencesGroup::new();
            levels.set_title("Levels");
            for index in 0..style.borrow().fib.levels.len() {
                let (row, ink, level_shows) = fib_level_row(&theme, &style, &showing, &emit, index);
                levels.add(&row);
                inks.borrow_mut().push(ink);
                shows.extend(level_shows);
            }
            more.push(levels);
        }
        // A box and an ellipse have the same properties — a fill at an alpha
        // under an edge — and differ only in the outline drawn round them, so
        // they are edited by the same rows.
        Kind::Rect | Kind::Ellipse => {
            // Background and how much of it shows.
            let (row, show) = paint_row(&theme, "Background", &style.borrow().fill, {
                let style = style.clone();
                let emit = emit.clone();
                move |paint| {
                    style.borrow_mut().fill = paint;
                    emit();
                }
            });
            group.add(&row);
            shows.push(Rc::new(move |s: &Style| show(&s.fill)));

            let alpha_row = adw::ActionRow::new();
            alpha_row.set_title("Transparency");
            alpha_row.set_subtitle("How much of the candles shows through.");
            let scale = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, 100.0, 1.0);
            scale.set_size_request(160, -1);
            scale.set_valign(gtk::Align::Center);
            scale.set_draw_value(true);
            scale.set_value_pos(gtk::PositionType::Left);
            scale.set_format_value_func(|_, v| format!("{v:.0}%"));
            scale.set_value((1.0 - style.borrow().alpha) * 100.0);
            {
                let style = style.clone();
                let emit = emit.clone();
                scale.connect_value_changed(move |scale| {
                    style.borrow_mut().alpha = (1.0 - scale.value() / 100.0).clamp(0.0, 1.0);
                    emit();
                });
            }
            alpha_row.add_suffix(&scale);
            group.add(&alpha_row);
            let scale_for_show = scale.clone();
            shows.push(Rc::new(move |s: &Style| scale_for_show.set_value((1.0 - s.alpha) * 100.0)));

            // The edge: whether, how thick, what colour.
            let border_row = adw::SwitchRow::new();
            border_row.set_title("Border");
            border_row.set_active(style.borrow().border);
            {
                let style = style.clone();
                let emit = emit.clone();
                border_row.connect_active_notify(move |row| {
                    style.borrow_mut().border = row.is_active();
                    emit();
                });
            }
            group.add(&border_row);
            let (width_row, show_width) = width_row("Border thickness", style.borrow().width, {
                let style = style.clone();
                let emit = emit.clone();
                move |width| {
                    style.borrow_mut().width = width;
                    emit();
                }
            });
            group.add(&width_row);
            let (colour_row, show_colour) = paint_row(&theme, "Border colour", &style.borrow().colour, {
                let style = style.clone();
                let emit = emit.clone();
                move |paint| {
                    style.borrow_mut().colour = paint;
                    emit();
                }
            });
            group.add(&colour_row);
            // The edge's rows only mean something while there is an edge.
            let follow = {
                let width_row = width_row.clone();
                let colour_row = colour_row.clone();
                move |on: bool| {
                    width_row.set_sensitive(on);
                    colour_row.set_sensitive(on);
                }
            };
            follow(style.borrow().border);
            {
                let follow = follow.clone();
                border_row.connect_active_notify(move |row| follow(row.is_active()));
            }
            let border_for_show = border_row.clone();
            shows.push(Rc::new(move |s: &Style| {
                border_for_show.set_active(s.border);
                show_width(s.width);
                show_colour(&s.colour);
                follow(s.border);
            }));
        }
        // A text drawing has no shape to edit, so its look *is* its text
        // properties: the three rows that would be the Text tab on a figure
        // are the page here, with no tab to put them behind.
        Kind::Text => {
            for (row, show) in text_rows(&theme, &style, &emit) {
                group.add(&row);
                shows.push(show);
            }
        }
    }

    // A shape's words, in a group of their own. The text kind's three rows
    // are already its page, above.
    let text = (!kind.is_text() && kind.takes_text()).then(|| {
        let group = adw::PreferencesGroup::new();
        group.set_title("Text");
        // The ink, the face and the size, and not where it sits: a
        // configuration says how words are set, never where one drawing's
        // happen to sit. Placement belongs to the drawing, and is on its
        // Text tab.
        group.set_description(Some("How a label on this drawing is set."));
        for (row, show) in text_rows(&theme, &style, &emit) {
            group.add(&row);
            shows.push(show);
        }
        group
    });

    let show: Rc<dyn Fn(&Style)> = {
        let style = style.clone();
        let showing = showing.clone();
        Rc::new(move |next: &Style| {
            *style.borrow_mut() = next.clone();
            showing.set(true);
            for show in &shows {
                show(next);
            }
            showing.set(false);
        })
    };
    Editor { group, more, text, show }
}

/// The Text tab of a figure's properties: what it says, where that sits, and
/// how it is set.
///
/// A tab rather than more rows under the shape's, because a figure's text is
/// a second subject: a box has a fill, an edge and a thickness, and then,
/// separately, it may have something written in it. Mixing the two into one
/// list makes the common case — a box with no label at all — read as a page
/// half of which does not apply.
///
/// The content is not part of the configuration and is edited straight onto
/// the drawing; the three style rows are, and go through the same editor the
/// shape's rows do.
fn text_page(
    theme: Theme,
    drawing: &omacharts_engine::Drawing,
    style: Style,
    on_style: Rc<dyn Fn(Style)>,
    on_text: Rc<dyn Fn(Text)>,
) -> TextTab {
    let page = adw::PreferencesPage::new();
    let showing = Rc::new(std::cell::Cell::new(false));
    let text: Rc<RefCell<Text>> = Rc::new(RefCell::new(drawing.text.clone()));

    let emit_text: Rc<dyn Fn()> = {
        let text = text.clone();
        let on_text = on_text.clone();
        let showing = showing.clone();
        Rc::new(move || {
            if showing.get() {
                return;
            }
            let now = text.borrow().clone();
            on_text(now);
        })
    };

    // Where the words sit, first and above everything: the picture is what
    // the eye goes to, and it is the choice most often being made when this
    // tab is opened.
    let (place, show_place) = place_picker(text.borrow().at, {
        let text = text.clone();
        let emit_text = emit_text.clone();
        move |at| {
            text.borrow_mut().at = at;
            emit_text();
        }
    });
    page.add(&place);

    // What it says. A plain area: the weight and the slope of a run live in
    // the text and a box like this cannot show them, so it edits the
    // characters and says so — the chart itself is where a word is made
    // bold, with the caret in it.
    let content = adw::PreferencesGroup::new();
    content.set_title("Text");
    let area = gtk::TextView::new();
    area.set_wrap_mode(gtk::WrapMode::WordChar);
    area.set_top_margin(8);
    area.set_bottom_margin(8);
    area.set_left_margin(8);
    area.set_right_margin(8);
    area.buffer().set_text(&drawing.text.plain_text());
    let frame = gtk::ScrolledWindow::new();
    frame.set_child(Some(&area));
    frame.set_min_content_height(84);
    frame.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    frame.add_css_class("card");
    content.add(&frame);
    page.add(&content);
    {
        let text = text.clone();
        let emit_text = emit_text.clone();
        let showing = showing.clone();
        area.buffer().connect_changed(move |buffer| {
            if showing.get() {
                return;
            }
            let (start, end) = buffer.bounds();
            let typed = buffer.text(&start, &end, true).to_string();
            let mut text = text.borrow_mut();
            // The runs are rebuilt from plain characters, so editing here
            // flattens a label that was partly bold. The description above
            // points at the chart, which is where the formatting lives.
            text.spans = vec![omacharts_engine::Span::plain(&typed)];
            let at = text.at;
            *text = Text { spans: text.spans.clone(), at }.tidied();
            drop(text);
            emit_text();
        });
    }

    // Where it sits, and how it is set.
    let look = adw::PreferencesGroup::new();
    let style_cell: Rc<RefCell<Style>> = Rc::new(RefCell::new(style));
    let emit_style: Rc<dyn Fn()> = {
        let style_cell = style_cell.clone();
        let on_style = on_style.clone();
        let showing = showing.clone();
        Rc::new(move || {
            if showing.get() {
                return;
            }
            let now = style_cell.borrow().clone();
            on_style(now);
        })
    };


    let mut show_style: Vec<Shower<Style>> = Vec::new();
    for (row, show) in text_rows(&theme, &style_cell, &emit_style) {
        look.add(&row);
        show_style.push(show);
    }
    page.add(&look);

    let show: ShowText = {
        let text = text.clone();
        let style_cell = style_cell.clone();
        let area = area.clone();
        let showing = showing.clone();
        Rc::new(move |drawing: &omacharts_engine::Drawing, style: &Style| {
            showing.set(true);
            *text.borrow_mut() = drawing.text.clone();
            *style_cell.borrow_mut() = style.clone();
            let plain = drawing.text.plain_text();
            // Only when it differs, or setting it would move the caret to
            // the end on every keystroke.
            let (start, end) = area.buffer().bounds();
            if area.buffer().text(&start, &end, true) != plain {
                area.buffer().set_text(&plain);
            }
            show_place(&drawing.text.at);
            for show in &show_style {
                show(style);
            }
            showing.set(false);
        })
    };
    (page, show)
}

/// Where a figure's label sits, as a picture of the figure with the words in
/// it: nine cells, one lit.
///
/// A graphical control rather than a list of nine names, because "top left"
/// is a position and reading a position off a word is work the eye should
/// not have to do. Each cell is the figure in miniature with a bar of text
/// where the label would go, so the control is a row of nine small answers
/// to the question rather than a menu about it.
fn place_picker(
    current: Place,
    on_pick: impl Fn(Place) + Clone + 'static,
) -> (adw::PreferencesGroup, Shower<Place>) {
    // A heading over it and no row under it: the picker opens the Text tab
    // as a band of its own, centred, rather than sitting on the right-hand
    // end of a row the way a switch does. It is the one control here that is
    // looked at rather than read, and giving it the width lets the nine
    // places be nine places rather than a cluster in a corner.
    let group = adw::PreferencesGroup::new();
    group.set_title("Position");

    let grid = gtk::Grid::new();
    grid.set_row_spacing(3);
    grid.set_column_spacing(3);
    grid.set_halign(gtk::Align::Center);
    grid.set_valign(gtk::Align::Center);
    grid.set_margin_top(4);
    grid.set_margin_bottom(4);
    grid.add_css_class("drawing-place-grid");

    let mut buttons: Vec<(Place, gtk::ToggleButton)> = Vec::new();
    let mut cells: Vec<gtk::DrawingArea> = Vec::new();
    for (at, place) in Place::ALL.into_iter().enumerate() {
        let cell = gtk::DrawingArea::new();
        cell.set_size_request(PLACE_CELL, PLACE_CELL);
        let button = gtk::ToggleButton::new();
        {
            // Lit from the button, read at draw time, so the stylesheet and
            // the picture cannot disagree about which cell is chosen.
            let button = button.clone();
            cell.set_draw_func(move |area, cr, w, h| {
                draw_place(area, cr, w as f64, h as f64, button.is_active())
            });
        }
        button.add_css_class("flat");
        button.add_css_class("drawing-place-cell");
        button.set_child(Some(&cell));
        button.set_tooltip_text(Some(place.label()));
        if let Some((_, first)) = buttons.first() {
            button.set_group(Some(first));
        }
        grid.attach(&button, at as i32 % 3, at as i32 / 3, 1, 1);
        buttons.push((place, button));
        cells.push(cell);
    }

    // Set while the buttons are being shown a place, so lighting one does
    // not read as a hand choosing it.
    let showing = Rc::new(std::cell::Cell::new(false));
    for (place, button) in &buttons {
        let place = *place;
        let on_pick = on_pick.clone();
        let showing = showing.clone();
        let cells_for_press = cells.clone();
        button.connect_toggled(move |button| {
            for cell in &cells_for_press {
                cell.queue_draw();
            }
            if showing.get() || !button.is_active() {
                return;
            }
            on_pick(place);
        });
    }

    let cells: Vec<gtk::DrawingArea> = cells;
    let light = {
        let buttons = buttons.clone();
        let cells = cells.clone();
        let showing = showing.clone();
        move |at: Place| {
            showing.set(true);
            for (place, button) in &buttons {
                // Only when it is actually wrong. These are a radio group,
                // and this runs inside one of their own toggle handlers —
                // the click tells the drawing, the drawing tells the dialog,
                // the dialog shows the picker — so setting a button that is
                // already right sends GTK round the group again from inside
                // its own bookkeeping.
                let want = *place == at;
                if button.is_active() != want {
                    button.set_active(want);
                }
            }
            showing.set(false);
            for cell in &cells {
                cell.queue_draw();
            }
        }
    };
    light(current);
    group.add(&grid);
    (group, Rc::new(move |at: &Place| light(*at)))
}

/// The side of one cell of the position grid.
///
/// Big enough to aim at and no bigger: there is one bar in each now, so the
/// cell only has to be a comfortable target.
const PLACE_CELL: i32 = 28;

/// One cell: a bar standing for the words, and nothing else.
///
/// The first try drew the figure in every cell with the bar placed inside
/// it, which is more faithful and much worse to look at: nine outlines and
/// nine dashes, at this size, read as a field of noise rather than as nine
/// positions. The grid's own border is the figure — one outline, round the
/// lot — and each cell is simply a place in it, which is how an anchor
/// picker works everywhere else. What is lit says where the words go.
fn draw_place(area: &gtk::DrawingArea, cr: &gtk::cairo::Context, w: f64, h: f64, lit: bool) {
    let fg = area.color();
    let (r, g, b, a) = (fg.red() as f64, fg.green() as f64, fg.blue() as f64, fg.alpha() as f64);
    // A dot, not a bar. A bar is a picture of a line of text, and nine of
    // them is a page of text where what is wanted is nine places — the
    // smallest mark that can sit somewhere says "somewhere" and nothing
    // else. Quiet until it is the one chosen, and a little larger then, so
    // the chosen place reads at a glance rather than only by its cell.
    let radius = if lit { 3.0 } else { 2.0 };
    cr.set_source_rgba(r, g, b, a * if lit { 1.0 } else { 0.35 });
    cr.arc((w / 2.0).round(), (h / 2.0).round(), radius, 0.0, std::f64::consts::TAU);
    let _ = cr.fill();
}

/// The three rows a text configuration is: ink, face, size.
///
/// Shared by the text drawing's own page and by the Text tab of every figure,
/// because they are the same three properties and a label written in a box
/// should be set from the same rows as a label written on its own.
fn text_rows(
    theme: &Theme,
    style: &Rc<RefCell<Style>>,
    emit: &Rc<dyn Fn()>,
) -> Vec<StyleRow> {
    let mut rows: Vec<StyleRow> = Vec::new();

    let (row, show) = paint_row(theme, "Colour", &style.borrow().text.colour, {
        let style = style.clone();
        let emit = emit.clone();
        move |paint| {
            style.borrow_mut().text.colour = paint;
            emit();
        }
    });
    rows.push((row, Rc::new(move |s: &Style| show(&s.text.colour))));

    let (row, show) = font_row(&style.borrow().text, {
        let style = style.clone();
        let emit = emit.clone();
        move |family| {
            style.borrow_mut().text.family = family;
            emit();
        }
    });
    rows.push((row, Rc::new(move |s: &Style| show(&s.text))));

    let (row, show) = size_row(style.borrow().text.clamped_size(), {
        let style = style.clone();
        let emit = emit.clone();
        move |size| {
            style.borrow_mut().text.size = size;
            emit();
        }
    });
    rows.push((row, Rc::new(move |s: &Style| show(s.text.clamped_size()))));

    rows
}

/// A row that picks the face: the desktop's own font, or any family the font
/// system has.
///
/// The system's font is the first choice and the default, named as what it
/// is rather than by the family it happens to be on this desktop, so a
/// configuration saved here follows the desktop the way a preset colour
/// follows the theme. Everything after it is a family by name.
fn font_row(
    current: &drawings::TextStyle,
    on_pick: impl Fn(Option<String>) + 'static,
) -> (adw::ActionRow, Shower<drawings::TextStyle>) {
    let row = adw::ComboRow::new();
    row.set_title("Font");
    let families = font_families();
    let mut names: Vec<String> = vec![system_font_label()];
    names.extend(families.iter().cloned());
    let refs: Vec<&str> = names.iter().map(String::as_str).collect();
    row.set_model(Some(&gtk::StringList::new(&refs)));

    let index_of = {
        let families = families.clone();
        move |style: &drawings::TextStyle| match style.family_name() {
            None => 0,
            Some(family) => families
                .iter()
                .position(|f| f.eq_ignore_ascii_case(family))
                .map(|at| at as u32 + 1)
                // A face the configuration names and this machine does not
                // have: shown as the system's, because that is what will be
                // drawn, and left alone unless the hand picks another.
                .unwrap_or(0),
        }
    };
    row.set_selected(index_of(current));
    {
        let families = families.clone();
        row.connect_selected_notify(move |row| {
            let picked = match row.selected() {
                0 => None,
                n => families.get(n as usize - 1).cloned(),
            };
            on_pick(picked);
        });
    }
    let row_for_show = row.clone();
    let shower: Shower<drawings::TextStyle> =
        Rc::new(move |style: &drawings::TextStyle| row_for_show.set_selected(index_of(style)));
    (row.upcast(), shower)
}

/// The families the font system has, sorted, deduplicated.
fn font_families() -> Vec<String> {
    use gtk::prelude::FontMapExt;
    let mut names: Vec<String> = pangocairo::FontMap::default()
        .list_families()
        .iter()
        .map(|family| family.name().to_string())
        .collect();
    names.sort_by_key(|n| n.to_lowercase());
    names.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
    names
}

/// How the system's own font is offered: as itself, with the family it is on
/// this desktop in brackets, so the choice is both the role and the fact.
fn system_font_label() -> String {
    match crate::ui::text::system_family() {
        Some(family) => format!("System font ({family})"),
        None => "System font".to_string(),
    }
}

/// A row that picks the text size, in pixels, with a slider and a box that
/// agree — the control every other bounded figure in the window uses.
fn size_row(current: f64, on_pick: impl Fn(f64) + 'static) -> (adw::ActionRow, Rc<dyn Fn(f64)>) {
    let bounded = crate::ui::controls::bounded_row(
        "Font size",
        None,
        current,
        drawings::MIN_TEXT_SIZE,
        drawings::MAX_TEXT_SIZE,
        drawings::TEXT_SIZE_STEP,
        0,
        on_pick,
    );
    let row = bounded.row.clone();
    let weak = bounded.downgrade();
    // The control is owned by the row it was put in; only a handle to it is
    // kept here, so showing a style a dialog has since closed is a no-op
    // rather than a dangling call.
    let shower: Rc<dyn Fn(f64)> = Rc::new(move |size: f64| weak.set(size));
    (row, shower)
}

/// A row that picks a width, in pixels, by number.
/// A small profile in a style's look, for a swatch or a preview: rows off
/// the edge it grows from, and the busiest marked across.
fn profile_sample(cr: &gtk::cairo::Context, theme: &Theme, style: &Style, x: f64, y: f64, w: f64, h: f64) {
    let rows = [0.3, 0.55, 0.85, 1.0, 0.7, 0.45, 0.25];
    let row_h = h / rows.len() as f64;
    let colour = style.colour.hex(theme);
    let fill = style.fill.hex(theme);
    let opacity = style.profile.opacity.clamp(0.0, 1.0);
    let right = style.profile.align == ProfileAlign::Right;
    let span = |share: f64| match right {
        true => (x + w - w * share, w * share),
        false => (x, w * share),
    };
    let path = |cr: &gtk::cairo::Context| {
        let edge = if right { x + w } else { x };
        cr.move_to(edge, y);
        for (n, share) in rows.iter().enumerate() {
            let end = if right { x + w - w * share } else { x + w * share };
            cr.line_to(end, y + n as f64 * row_h);
            cr.line_to(end, y + (n + 1) as f64 * row_h);
        }
        cr.line_to(edge, y + h);
        cr.close_path();
    };
    match style.profile.shape {
        ProfileShape::Filled => {
            for (n, share) in rows.iter().enumerate() {
                let inside = (2..=4).contains(&n);
                let alpha = if inside { style.alpha } else { style.alpha * drawings::PROFILE_ROW_SHARE };
                colors::set_source_alpha(cr, &fill, alpha.max(0.12) * opacity);
                let (left, width) = span(*share);
                cr.rectangle(left, y + n as f64 * row_h, width, row_h);
                let _ = cr.fill();
            }
            let edge = if style.border { drawings::PROFILE_EDGE_ALPHA } else { 0.0 };
            colors::set_source_alpha(cr, &colour, edge * opacity);
            cr.set_line_width(1.0);
        }
        ProfileShape::Outline => {
            colors::set_source_alpha(cr, &colour, opacity);
            cr.set_line_width(style.width.clamp(1.0, 2.0));
        }
    }
    path(cr);
    let _ = cr.stroke();
    let poc = (y + 3.5 * row_h).round() + 0.5;
    colors::set_source_alpha(cr, &style.profile.poc.hex(theme), drawings::PROFILE_POC_ALPHA);
    cr.set_line_width(style.profile.poc_width.clamp(1.0, 2.0));
    let (left, width) = span(1.0);
    cr.move_to(left, poc);
    cr.line_to(left + width, poc);
    let _ = cr.stroke();
}

/// A small anchored VWAP in a style's look: a dot on the bar it starts from
/// and the line settling away from it, between the bands it shows, each
/// shaded and edged as the chart draws it.
fn vwap_sample(cr: &gtk::cairo::Context, theme: &Theme, style: &Style, (x, y, w, h): (f64, f64, f64, f64), line_w: f64) {
    let colour = style.colour.hex(theme);
    let (x0, y0) = (x + line_w, y + h * 0.85);
    // The line, lifted by `lift`; and back along it lowered by as much,
    // which closes a band's shading.
    let curve = |cr: &gtk::cairo::Context, lift: f64| {
        cr.move_to(x0, y0 - lift * 0.3);
        cr.curve_to(x + w * 0.2, y + h * 0.15 - lift, x + w * 0.45, y + h * 0.5 - lift, x + w, y + h * 0.4 - lift);
    };
    let back = |cr: &gtk::cairo::Context, lift: f64| {
        cr.line_to(x + w, y + h * 0.4 + lift);
        cr.curve_to(x + w * 0.45, y + h * 0.5 + lift, x + w * 0.2, y + h * 0.15 + lift, x0, y0 + lift * 0.3);
        cr.close_path();
    };
    // Spread by place rather than by multiple, so three bands at nearly the
    // same one still read as three.
    let lift = |n: usize| h * 0.12 * (n + 1) as f64;
    for (n, band) in style.vwap.bands.iter().enumerate().filter(|(_, b)| b.enabled && b.fill) {
        colors::set_source_alpha(cr, &band.paint(&style.colour).hex(theme), band.fill_alpha);
        curve(cr, lift(n));
        back(cr, lift(n));
        let _ = cr.fill();
    }
    for (n, band) in style.vwap.bands.iter().enumerate().filter(|(_, b)| b.enabled && !b.stroke.is_hidden()) {
        colors::set_source(cr, &band.paint(&style.colour).hex(theme));
        cr.set_line_width(band.stroke.width);
        cr.set_dash(&band.stroke.style.dashes(band.stroke.width), 0.0);
        for side in [lift(n), -lift(n)] {
            curve(cr, side);
            let _ = cr.stroke();
        }
    }
    cr.set_dash(&[], 0.0);
    if style.width <= 0.0 {
        return;
    }
    colors::set_source(cr, &colour);
    cr.set_line_width(line_w);
    cr.set_line_cap(gtk::cairo::LineCap::Round);
    cr.set_dash(&style.vwap.line.dashes(line_w), 0.0);
    curve(cr, 0.0);
    let _ = cr.stroke();
    cr.set_dash(&[], 0.0);
    cr.arc(x0, y0, line_w.max(1.5), 0.0, std::f64::consts::TAU);
    let _ = cr.fill();
}

/// One of an anchored VWAP's bands, as the study's settings show one of
/// its own: whether it is drawn, how far out, its shading, its colour, and
/// its edges.
fn vwap_band_group(
    theme: &Theme,
    style: &Rc<RefCell<Style>>,
    showing: &Rc<Cell<bool>>,
    emit: &Rc<dyn Fn()>,
    index: usize,
) -> (adw::PreferencesGroup, Ink, Vec<Shower<Style>>) {
    let group = adw::PreferencesGroup::new();
    group.set_title(&format!("Band {}", index + 1));
    let mut shows: Vec<Shower<Style>> = Vec::new();
    let band = style.borrow().vwap.bands[index].clone();
    // Every change to a band goes through here, and none while a style is
    // being shown.
    let edit = {
        let style = style.clone();
        let emit = emit.clone();
        let showing = showing.clone();
        Rc::new(move |change: &dyn Fn(&mut drawings::VwapBand)| {
            if showing.get() {
                return;
            }
            if let Some(band) = style.borrow_mut().vwap.bands.get_mut(index) {
                change(band);
            }
            emit();
        })
    };

    let switch_row = |title: &str, subtitle: Option<&str>, on: bool, apply: fn(&mut drawings::VwapBand, bool)| {
        let row = adw::SwitchRow::new();
        row.set_title(title);
        if let Some(subtitle) = subtitle {
            row.set_subtitle(subtitle);
        }
        row.set_active(on);
        let edit = edit.clone();
        row.connect_active_notify(move |row| {
            let on = row.is_active();
            edit(&|band| apply(band, on));
        });
        row
    };

    let show = switch_row("Show", None, band.enabled, |band, on| band.enabled = on);
    group.add(&show);
    shows.push(Rc::new(move |s: &Style| show.set_active(s.vwap.bands[index].enabled)));

    // Tenths, as the study's: one, one and a half and two are the figures
    // people reach for.
    let deviations = controls::bounded_row(
        "Standard deviations",
        None,
        band.deviations,
        drawings::MIN_VWAP_DEVIATIONS,
        drawings::MAX_VWAP_DEVIATIONS,
        0.1,
        1,
        {
            let edit = edit.clone();
            move |value| edit(&|band| band.deviations = value)
        },
    );
    group.add(&deviations.row);
    {
        let adjustment = deviations.adjustment.clone();
        shows.push(Rc::new(move |s: &Style| adjustment.set_value(s.vwap.bands[index].deviations)));
    }

    let shaded = switch_row("Shaded", Some("Fill the area between this band's edges"), band.fill, |band, on| {
        band.fill = on
    });
    group.add(&shaded);
    shows.push(Rc::new(move |s: &Style| shaded.set_active(s.vwap.bands[index].fill)));

    let shading = controls::bounded_row(
        "Shading %",
        None,
        band.fill_alpha * 100.0,
        omacharts_engine::indicators::bands::MIN_FILL_ALPHA * 100.0,
        omacharts_engine::indicators::bands::MAX_FILL_ALPHA * 100.0,
        1.0,
        0,
        {
            let edit = edit.clone();
            move |percent| edit(&|band| band.fill_alpha = percent / 100.0)
        },
    );
    group.add(&shading.row);
    {
        let adjustment = shading.adjustment.clone();
        shows.push(Rc::new(move |s: &Style| adjustment.set_value(s.vwap.bands[index].fill_alpha * 100.0)));
    }

    // The band's own colour, or the line's while it has none: the same
    // answer the chart draws.
    let ink = {
        let style = style.clone();
        let theme = theme.clone();
        Ink::new(move || {
            let style = style.borrow();
            style.vwap.bands[index].paint(&style.colour).hex(&theme)
        })
    };
    let reset = gtk::Button::with_label("Reset");
    reset.add_css_class("flat");
    reset.add_css_class("subtle-link");
    reset.set_valign(gtk::Align::Center);
    reset.set_tooltip_text(Some("Back to the line's colour"));
    reset.set_sensitive(band.colour.is_some());
    let (button, show_paint) = paint_button(theme, band.paint(&style.borrow().colour), {
        let edit = edit.clone();
        let ink = ink.clone();
        let reset = reset.downgrade();
        move |paint| {
            edit(&|band| band.colour = Some(paint.clone()));
            if let Some(reset) = reset.upgrade() {
                reset.set_sensitive(true);
            }
            ink.repaint();
        }
    });
    {
        let edit = edit.clone();
        let ink = ink.clone();
        let style = style.clone();
        let show_paint = show_paint.clone();
        reset.connect_clicked(move |reset| {
            edit(&|band| band.colour = None);
            reset.set_sensitive(false);
            let line = style.borrow().colour.clone();
            show_paint(&line);
            ink.repaint();
        });
    }
    let colour = adw::ActionRow::new();
    colour.set_title("Colour");
    colour.add_suffix(&reset);
    colour.add_suffix(&button);
    group.add(&colour);
    {
        let ink = ink.clone();
        shows.push(Rc::new(move |s: &Style| {
            let band = &s.vwap.bands[index];
            show_paint(band.paint(&s.colour));
            reset.set_sensitive(band.colour.is_some());
            ink.repaint();
        }));
    }
    for (row, show) in stroke_rows(&ink, band.stroke, showing, {
        let edit = edit.clone();
        move |stroke: Stroke| edit(&|band| band.stroke = stroke)
    }) {
        group.add(&row);
        shows.push(Rc::new(move |s: &Style| show(s.vwap.bands[index].stroke)));
    }
    (group, ink, shows)
}

/// One of a retracement's levels: a row named by its ratio, with a switch
/// that draws it or not and a sample of how it is drawn, that opens on its
/// colour, its thickness and its pattern.
///
/// A row that opens rather than a group each, because seven groups of four
/// rows is a page nobody finds the eighth thing on, and most levels are
/// never opened: they follow the drawing's colour as a solid hairline.
fn fib_level_row(
    theme: &Theme,
    style: &Rc<RefCell<Style>>,
    showing: &Rc<Cell<bool>>,
    emit: &Rc<dyn Fn()>,
    index: usize,
) -> (adw::ExpanderRow, Ink, Vec<Shower<Style>>) {
    let mut shows: Vec<Shower<Style>> = Vec::new();
    let level = style.borrow().fib.levels[index].clone();
    let edit = {
        let style = style.clone();
        let emit = emit.clone();
        let showing = showing.clone();
        Rc::new(move |change: &dyn Fn(&mut drawings::FibLevel)| {
            if showing.get() {
                return;
            }
            if let Some(level) = style.borrow_mut().fib.levels.get_mut(index) {
                change(level);
            }
            emit();
        })
    };
    let row = adw::ExpanderRow::new();
    row.set_title(&level.name());

    // The level's colour as it is drawn, for the samples.
    let ink = {
        let style = style.clone();
        let theme = theme.clone();
        Ink::new(move || {
            let style = style.borrow();
            style.fib.levels[index].paint(&style.colour).hex(&theme)
        })
    };

    // How it is drawn, on the row itself, so the seven read as a picture of
    // the drawing before any is opened.
    let sample = gtk::DrawingArea::new();
    sample.set_content_width(56);
    sample.set_content_height(16);
    sample.set_valign(gtk::Align::Center);
    {
        let style = style.clone();
        let ink = ink.clone();
        sample.set_draw_func(move |_, cr, w, h| {
            let style = style.borrow();
            let Some(level) = style.fib.levels.get(index) else { return };
            if !level.enabled || level.stroke.is_hidden() {
                return;
            }
            let width = level.stroke.width;
            let y = (h as f64 / 2.0).round() + if (width.round() as i64) % 2 == 1 { 0.5 } else { 0.0 };
            colors::set_source_alpha(cr, &ink.colour(), drawings::FIB_ALPHA);
            cr.set_line_width(width);
            cr.set_dash(&level.stroke.style.dashes(width), 0.0);
            cr.move_to(2.0, y);
            cr.line_to(w as f64 - 2.0, y);
            let _ = cr.stroke();
        });
    }
    ink.watch(&sample);
    row.add_suffix(&sample);

    let switch = gtk::Switch::new();
    switch.set_valign(gtk::Align::Center);
    switch.set_active(level.enabled);
    switch.set_tooltip_text(Some("Show"));
    {
        let edit = edit.clone();
        let ink = ink.clone();
        switch.connect_active_notify(move |switch| {
            let on = switch.is_active();
            edit(&|level| level.enabled = on);
            ink.repaint();
        });
    }
    row.add_suffix(&switch);
    {
        let switch = switch.clone();
        shows.push(Rc::new(move |s: &Style| switch.set_active(s.fib.levels[index].enabled)));
    }

    // Its own colour, or the drawing's while it has none: the same answer
    // the chart draws.
    let reset = gtk::Button::with_label("Reset");
    reset.add_css_class("flat");
    reset.add_css_class("subtle-link");
    reset.set_valign(gtk::Align::Center);
    reset.set_tooltip_text(Some("Back to the drawing's colour"));
    reset.set_sensitive(level.colour.is_some());
    let (button, show_paint) = paint_button(theme, level.paint(&style.borrow().colour), {
        let edit = edit.clone();
        let ink = ink.clone();
        let reset = reset.downgrade();
        move |paint| {
            edit(&|level| level.colour = Some(paint.clone()));
            if let Some(reset) = reset.upgrade() {
                reset.set_sensitive(true);
            }
            ink.repaint();
        }
    });
    {
        let edit = edit.clone();
        let ink = ink.clone();
        let style = style.clone();
        let show_paint = show_paint.clone();
        reset.connect_clicked(move |reset| {
            edit(&|level| level.colour = None);
            reset.set_sensitive(false);
            let colour = style.borrow().colour.clone();
            show_paint(&colour);
            ink.repaint();
        });
    }
    let colour = adw::ActionRow::new();
    colour.set_title("Colour");
    colour.add_suffix(&reset);
    colour.add_suffix(&button);
    row.add_row(&colour);
    {
        let ink = ink.clone();
        shows.push(Rc::new(move |s: &Style| {
            let level = &s.fib.levels[index];
            show_paint(level.paint(&s.colour));
            reset.set_sensitive(level.colour.is_some());
            ink.repaint();
        }));
    }
    for (stroke_row, show) in stroke_rows(&ink, level.stroke, showing, {
        let edit = edit.clone();
        let ink = ink.clone();
        move |stroke: Stroke| {
            edit(&|level| level.stroke = stroke);
            ink.repaint();
        }
    }) {
        row.add_row(&stroke_row);
        shows.push(Rc::new(move |s: &Style| show(s.fib.levels[index].stroke)));
    }
    (row, ink, shows)
}

/// A small retracement in a style's look: its levels across the box, 0 at
/// the top as a swing dragged upward has it, each in its colour, thickness
/// and pattern at the strength the chart draws a level.
fn fib_sample(cr: &gtk::cairo::Context, theme: &Theme, style: &Style, (x, y, w, h): (f64, f64, f64, f64)) {
    cr.save().ok();
    for level in style.fib.enabled() {
        if level.stroke.is_hidden() {
            continue;
        }
        let width = level.stroke.width.min(h / 4.0).max(0.75);
        let at = (y + h * level.ratio).round() + if (width.round() as i64) % 2 == 1 { 0.5 } else { 0.0 };
        colors::set_source_alpha(cr, &level.paint(&style.colour).hex(theme), drawings::FIB_ALPHA);
        cr.set_line_width(width);
        cr.set_dash(&level.stroke.style.dashes(width), 0.0);
        cr.move_to(x, at);
        cr.line_to(x + w, at);
        let _ = cr.stroke();
    }
    cr.restore().ok();
}

/// A row of stroke samples, and a way to show it another stroke.
type StrokeRow = (adw::ActionRow, Rc<dyn Fn(Stroke)>);

/// A thickness and a pattern, picked from drawn samples: the rows a study's
/// line is set with, with the same choices, so an anchored VWAP and the
/// study are set the same way.
fn stroke_rows(
    ink: &Ink,
    current: Stroke,
    showing: &Rc<Cell<bool>>,
    on_pick: impl Fn(Stroke) + Clone + 'static,
) -> Vec<StrokeRow> {
    // What is set now, which a pick changes half of.
    let now = Rc::new(Cell::new(current));
    let widths: Vec<(f64, Stroke)> = chart_settings::WIDTHS
        .iter()
        .map(|width| (*width, Stroke::new(*width, current.style)))
        .collect();
    let styles: Vec<(f64, Stroke)> = LineStyle::ALL
        .iter()
        .map(|style| (chart_settings::LINE_SAMPLE_WIDTH, Stroke::new(chart_settings::LINE_SAMPLE_WIDTH, *style)))
        .collect();
    let mut rows = Vec::new();
    for (title, options, by_width) in [("Thickness", widths, true), ("Line style", styles, false)] {
        let row = adw::ActionRow::new();
        row.set_title(title);
        let strip = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        strip.add_css_class("linked");
        strip.add_css_class("stroke-picker");
        strip.set_valign(gtk::Align::Center);
        let matches = move |option: &Stroke, stroke: Stroke| match by_width {
            true => (option.width - stroke.width).abs() < 0.01,
            false => option.style == stroke.style,
        };
        let mut first: Option<gtk::ToggleButton> = None;
        let mut buttons = Vec::new();
        for (sample, option) in options {
            let button = gtk::ToggleButton::new();
            button.add_css_class("flat");
            let style = match by_width {
                true => now.get().style,
                false => option.style,
            };
            let area = chart_settings::stroke_sample(sample, style, ink);
            ink.watch(&area);
            button.set_child(Some(&area));
            button.set_tooltip_text(Some(&chart_settings::describe_stroke(sample, style)));
            match &first {
                Some(anchor) => button.set_group(Some(anchor)),
                None => first = Some(button.clone()),
            }
            button.set_active(matches(&option, current));
            let now = now.clone();
            let showing = showing.clone();
            let on_pick = on_pick.clone();
            button.connect_toggled(move |button| {
                if !button.is_active() || showing.get() {
                    return;
                }
                let picked = match by_width {
                    true => Stroke::new(option.width, now.get().style),
                    false => Stroke::new(now.get().width, option.style),
                };
                now.set(picked);
                on_pick(picked);
            });
            strip.append(&button);
            buttons.push((button, option));
        }
        row.add_suffix(&strip);
        let now = now.clone();
        let show: Rc<dyn Fn(Stroke)> = Rc::new(move |stroke: Stroke| {
            now.set(stroke);
            for (button, option) in &buttons {
                button.set_active(matches(option, stroke));
            }
            // A thickness nobody offered, from the keyboard, presses none.
            if !buttons.iter().any(|(_, option)| matches(option, stroke)) {
                for (button, _) in &buttons {
                    button.set_active(false);
                }
            }
        });
        rows.push((row, show));
    }
    rows
}

/// A row choosing one of a few named ways, and a way to show another.
fn named_row(
    title: &str,
    names: &[&str],
    current: Option<usize>,
    on_pick: impl Fn(usize) + 'static,
) -> (adw::ComboRow, Rc<dyn Fn(Option<usize>)>) {
    let row = adw::ComboRow::new();
    row.set_title(title);
    row.set_model(Some(&gtk::StringList::new(names)));
    row.set_selected(current.unwrap_or(0) as u32);
    let showing = Rc::new(Cell::new(false));
    {
        let showing = showing.clone();
        row.connect_selected_notify(move |row| {
            if !showing.get() {
                on_pick(row.selected() as usize);
            }
        });
    }
    let shown = row.clone();
    let show: Rc<dyn Fn(Option<usize>)> = Rc::new(move |at| {
        showing.set(true);
        shown.set_selected(at.unwrap_or(0) as u32);
        showing.set(false);
    });
    (row, show)
}

fn width_row(title: &str, current: f64, on_pick: impl Fn(f64) + 'static) -> (adw::ActionRow, Rc<dyn Fn(f64)>) {
    let row = adw::ActionRow::new();
    row.set_title(title);
    let spin = gtk::SpinButton::with_range(
        drawings::MIN_WIDTH,
        drawings::MAX_WIDTH,
        drawings::WIDTH_STEP,
    );
    spin.set_digits(1);
    spin.set_valign(gtk::Align::Center);
    spin.set_value(current);
    spin.connect_value_changed(move |spin| on_pick(spin.value()));
    row.add_suffix(&spin);
    let spin_for_show = spin.clone();
    (row, Rc::new(move |width| spin_for_show.set_value(width)))
}

/// A row that picks a colour: the nine presets as tiles, and a wheel for a
/// colour of one's own.
fn paint_row(
    theme: &Theme,
    title: &str,
    current: &Paint,
    on_pick: impl Fn(Paint) + Clone + 'static,
) -> (adw::ActionRow, Shower<Paint>) {
    let row = adw::ActionRow::new();
    row.set_title(title);
    let (button, show) = paint_button(theme, current, on_pick);
    row.add_suffix(&button);
    (row, show)
}

/// The swatch that opens the presets and the wheel, for a row of its own.
fn paint_button(
    theme: &Theme,
    current: &Paint,
    on_pick: impl Fn(Paint) + Clone + 'static,
) -> (gtk::MenuButton, Shower<Paint>) {
    let shown = palette::swatch_area(&current.hex(theme), false);
    let button = gtk::MenuButton::new();
    button.set_child(Some(&shown));
    button.set_valign(gtk::Align::Center);
    button.add_css_class("flat");
    let popover = gtk::Popover::new();
    popover.set_has_arrow(false);

    let content = gtk::Box::new(gtk::Orientation::Vertical, 8);
    content.set_margin_top(8);
    content.set_margin_bottom(8);
    content.set_margin_start(8);
    content.set_margin_end(8);
    let grid = gtk::Grid::new();
    grid.set_row_spacing(4);
    grid.set_column_spacing(4);
    let palette = drawings::palette(theme);
    let current_preset = match current {
        Paint::Preset { preset } => Some(*preset),
        _ => None,
    };
    for (n, (preset, hex)) in palette.iter().enumerate() {
        let cell = gtk::Button::new();
        cell.add_css_class("flat");
        cell.set_tooltip_text(Some(preset.name()));
        cell.set_child(Some(&palette::swatch_area(hex, current_preset == Some(*preset))));
        let on_pick = on_pick.clone();
        let popover = popover.downgrade();
        let shown = shown.downgrade();
        let (preset, hex) = (*preset, hex.clone());
        cell.connect_clicked(move |_| {
            if let Some(shown) = shown.upgrade() {
                palette::paint(&shown, &hex, false);
            }
            on_pick(Paint::preset(preset));
            if let Some(popover) = popover.upgrade() {
                popover.popdown();
            }
        });
        grid.attach(&cell, n as i32 % 5, n as i32 / 5, 1, 1);
    }
    content.append(&grid);
    content.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
    let custom = gtk::Button::with_label("Custom…");
    custom.add_css_class("flat");
    custom.set_tooltip_text(Some("Pick an exact colour, which the theme will not change"));
    let start = colors::parse(&current.hex(theme));
    {
        let on_pick = on_pick.clone();
        let popover = popover.downgrade();
        let shown = shown.downgrade();
        custom.connect_clicked(move |button| {
            if let Some(popover) = popover.upgrade() {
                popover.popdown();
            }
            let dialog = gtk::ColorDialog::new();
            let window = button.root().and_downcast::<gtk::Window>();
            let on_pick = on_pick.clone();
            let shown = shown.clone();
            dialog.choose_rgba(window.as_ref(), Some(&start), None::<&gio::Cancellable>, move |answer| {
                let Ok(rgba) = answer else { return };
                let hex = colors::to_hex(&rgba);
                if let Some(shown) = shown.upgrade() {
                    palette::paint(&shown, &hex, false);
                }
                on_pick(Paint::fixed(&hex));
            });
        });
    }
    content.append(&custom);
    popover.set_child(Some(&content));
    button.set_popover(Some(&popover));

    let theme = theme.clone();
    let shown_for_show = shown.clone();
    (button, Rc::new(move |paint: &Paint| palette::paint(&shown_for_show, &paint.hex(&theme), false)))
}

// ---------------------------------------------------------------------------
// Previews
// ---------------------------------------------------------------------------

/// The nine configurations as tiles, each drawn as it will look, the one in
/// use ringed. For the picker in a drawing's properties.
fn configuration_grid(
    theme: &Theme,
    kind: Kind,
    configs: &Configurations,
    current: Option<u8>,
    custom: Option<&Style>,
    // The drawing's own words, so each tile shows what *this* label would
    // look like in that configuration rather than a stand-in.
    text: &Text,
    on_pick: impl Fn(u8) + Clone + 'static,
) -> gtk::Box {
    let content = gtk::Box::new(gtk::Orientation::Vertical, 6);
    content.set_margin_top(8);
    content.set_margin_bottom(8);
    content.set_margin_start(8);
    content.set_margin_end(8);
    let grid = gtk::Grid::new();
    grid.set_row_spacing(6);
    grid.set_column_spacing(6);
    let tile = |preview: gtk::DrawingArea, name: &str, tip: &str| {
        let stack = gtk::Box::new(gtk::Orientation::Vertical, 2);
        let label = gtk::Label::new(Some(name));
        label.add_css_class("dim-label");
        label.add_css_class("caption");
        stack.append(&preview);
        stack.append(&label);
        let cell = gtk::Button::new();
        cell.add_css_class("flat");
        cell.set_tooltip_text(Some(tip));
        cell.set_child(Some(&stack));
        cell
    };
    for n in 1..=CONFIGURATIONS {
        let preview = preview_tile(theme, kind, configs.of(kind, n), text);
        if current == Some(n) {
            preview.add_css_class("drawing-preview-current");
        }
        let cell = tile(preview, &format!("{n}"), &format!("Configuration {n} (Alt+{n})"));
        let on_pick = on_pick.clone();
        cell.connect_clicked(move |_| on_pick(n));
        grid.attach(&cell, (n as i32 - 1) % 3, (n as i32 - 1) / 3, 1, 1);
    }
    // This drawing's own look, while it has one: the one in use, and not a
    // choice so much as a reminder of what choosing a number gives up.
    if let Some(custom) = custom {
        let preview = preview_tile(theme, kind, custom, text);
        preview.add_css_class("drawing-preview-current");
        let cell = tile(preview, "Custom", "This drawing's own look, as it is now");
        // Picking it changes nothing, but it is not greyed: greyed reads as
        // unavailable, and this is the one that is in use.
        cell.add_css_class("drawing-preview-chosen");
        grid.attach(&cell, 0, 3, 1, 1);
    }
    content.append(&grid);
    content
}

/// The look alone, for a menu row: a stroke of the line, or the box with
/// its fill and edge, at a size that keeps the row a row. No candles — a
/// menu is read, not studied.
pub fn swatch(theme: &Theme, kind: Kind, style: &Style) -> gtk::DrawingArea {
    let area = gtk::DrawingArea::new();
    area.set_size_request(SWATCH_W, SWATCH_H);
    area.set_valign(gtk::Align::Center);
    let theme = theme.clone();
    let style = style.clone();
    area.set_draw_func(move |_, cr, w, h| {
        let (w, h) = (w as f64, h as f64);
        let colour = style.colour.hex(&theme);
        match kind {
            Kind::Line | Kind::Horizontal | Kind::Arrow | Kind::Zigzag => {
                colors::set_source(cr, &colour);
                cr.set_line_width(style.width.clamp(1.0, 4.0));
                cr.set_line_cap(gtk::cairo::LineCap::Round);
                let y = (h / 2.0).round() + 0.5;
                cr.move_to(2.0, y);
                cr.line_to(w - 2.0, y);
                let _ = cr.stroke();
            }
            Kind::Rect => {
                colors::set_source_alpha(cr, &style.fill.hex(&theme), style.alpha.max(0.25));
                cr.rectangle(1.0, 1.0, w - 2.0, h - 2.0);
                let _ = cr.fill();
                if style.border {
                    colors::set_source_alpha(cr, &colour, 0.8);
                    cr.set_line_width(1.0);
                    cr.rectangle(1.5, 1.5, w - 3.0, h - 3.0);
                    let _ = cr.stroke();
                }
            }
            Kind::Ellipse => {
                let curve = |cr: &gtk::cairo::Context, inset: f64| {
                    cr.save().ok();
                    cr.translate(w / 2.0, h / 2.0);
                    cr.scale(w / 2.0 - inset, h / 2.0 - inset);
                    cr.arc(0.0, 0.0, 1.0, 0.0, std::f64::consts::TAU);
                    cr.restore().ok();
                };
                colors::set_source_alpha(cr, &style.fill.hex(&theme), style.alpha.max(0.25));
                curve(cr, 1.0);
                let _ = cr.fill();
                if style.border {
                    colors::set_source_alpha(cr, &colour, 0.8);
                    cr.set_line_width(1.0);
                    curve(cr, 1.5);
                    let _ = cr.stroke();
                }
            }
            Kind::Profile => profile_sample(cr, &theme, &style, 1.0, 1.0, w - 2.0, h - 2.0),
            Kind::Vwap => vwap_sample(cr, &theme, &style, (2.0, 2.0, w - 4.0, h - 4.0), style.width.clamp(1.0, 3.0)),
            Kind::Fib => fib_sample(cr, &theme, &style, (2.0, 1.0, w - 4.0, h - 2.0)),
            // A swatch is thirty pixels by twelve: too small for a word, so
            // the text configuration shows the ink as a bar of it, the way
            // every other colour in the window is shown at this size.
            Kind::Text => {
                let ground = theme.ui.background.clone();
                let ink = drawings::text_colour(&style.text.colour, &theme, &ground);
                colors::set_source(cr, &ink);
                cr.rectangle(2.0, (h / 2.0).round() - 2.0, w - 4.0, 4.0);
                let _ = cr.fill();
            }
        }
    });
    area
}

/// A tile showing the drawing as the style says, so a picker shows the look
/// rather than naming it.
fn preview_tile(theme: &Theme, kind: Kind, style: &Style, text: &Text) -> gtk::DrawingArea {
    let area = gtk::DrawingArea::new();
    area.set_size_request(PREVIEW_W, PREVIEW_H);
    area.add_css_class("drawing-preview");
    paint_preview(&area, theme, kind, style, text);
    area
}

/// Paint the drawing on its own ground, at whatever size the area has.
///
/// No candles, anywhere. They were here so that a preview showed how a
/// drawing sits on a chart — but what a preview is for is telling one
/// drawing from another, and a run of market behind every one of them is
/// the same run every time: something the eye has to look past to reach the
/// thing that differs. They also cost a scheme, a bar style and a page of
/// arithmetic about how big a candle should be, all of it in aid of a
/// backdrop.
fn paint_preview(area: &gtk::DrawingArea, theme: &Theme, kind: Kind, style: &Style, text: &Text) {
    let theme = theme.clone();
    let style = style.clone();
    let text = text.clone();
    area.set_draw_func(move |_, cr, w, h| {
        let (w, h) = (w as f64, h as f64);
        colors::set_source(cr, &theme.ui.background);
        rounded(cr, 0.5, 0.5, w - 1.0, h - 1.0, 5.0);
        let _ = cr.fill();
        draw_preview_figure(cr, w, h, &theme, kind, &style, &text);
    });
    area.queue_draw();
}

/// How much bigger than the smallest tile a picture is being drawn.
///
/// The strokes and the sample text follow it, so the wide strip at the top
/// of a drawing's properties is the same picture as a tile in the picker
/// rather than a different one at a different size.
fn preview_scale(h: f64) -> f64 {
    (h / PREVIEW_H as f64).clamp(1.0, 2.0)
}

/// The drawing itself, over whatever ground the preview has laid down.
fn draw_preview_figure(
    cr: &gtk::cairo::Context,
    w: f64,
    h: f64,
    theme: &Theme,
    kind: Kind,
    style: &Style,
    text: &Text,
) {
    let scale = preview_scale(h);
    let colour = style.colour.hex(theme);
    let line_w = style.width.min(4.0) * scale.sqrt();
    match kind {
        Kind::Line | Kind::Horizontal | Kind::Arrow | Kind::Zigzag => {
            // A level is drawn level, and a zig-zag zig-zags, or the
            // picture would promise something the tool will not give.
            if kind == Kind::Zigzag {
                let (x0, x1) = (w * 0.12, w * 0.88);
                let (lo, hi) = (h * 0.76, h * 0.24);
                let step = (x1 - x0) / 3.0;
                let corners = [
                    (x0, lo),
                    (x0 + step, hi),
                    (x0 + step * 2.0, (lo + hi) / 2.0),
                    (x1, hi),
                ];
                colors::set_source(cr, &colour);
                cr.set_line_width(line_w);
                cr.set_line_cap(gtk::cairo::LineCap::Round);
                cr.set_line_join(gtk::cairo::LineJoin::Round);
                cr.move_to(corners[0].0, corners[0].1);
                for (cx, cy) in &corners[1..] {
                    cr.line_to(*cx, *cy);
                }
                let _ = cr.stroke();
                if style.arrow.at_end() {
                    head(cr, corners[2], corners[3], line_w, style.head);
                }
                if style.arrow.at_start() {
                    head(cr, corners[1], corners[0], line_w, style.head);
                }
            } else {
            let (a, b) = match kind {
                Kind::Horizontal => ((w * 0.12, h * 0.5), (w * 0.88, h * 0.5)),
                _ => ((w * 0.15, h * 0.75), (w * 0.85, h * 0.25)),
            };
            colors::set_source(cr, &colour);
            cr.set_line_width(line_w);
            cr.set_line_cap(gtk::cairo::LineCap::Round);
            cr.move_to(a.0, a.1);
            cr.line_to(b.0, b.1);
            let _ = cr.stroke();
            if style.arrow.at_end() {
                head(cr, a, b, line_w, style.head);
            }
            if style.arrow.at_start() {
                head(cr, b, a, line_w, style.head);
            }
            }
        }
        Kind::Rect | Kind::Ellipse => {
            let (x, y, rw, rh) = (w * 0.3, h * 0.2, w * 0.4, h * 0.6);
            let edge_w = style.width.clamp(1.0, 3.0) * scale.sqrt();
            colors::set_source_alpha(cr, &style.fill.hex(theme), style.alpha);
            if kind == Kind::Rect {
                cr.rectangle(x, y, rw, rh);
                let _ = cr.fill();
                if style.border {
                    colors::set_source_alpha(cr, &colour, drawings::BORDER_ALPHA);
                    cr.set_line_width(edge_w);
                    cr.rectangle(x.round() + 0.5, y.round() + 0.5, rw.round(), rh.round());
                    let _ = cr.stroke();
                }
            } else {
                let curve = |cr: &gtk::cairo::Context| {
                    cr.save().ok();
                    cr.translate(x + rw / 2.0, y + rh / 2.0);
                    cr.scale(rw / 2.0, rh / 2.0);
                    cr.arc(0.0, 0.0, 1.0, 0.0, std::f64::consts::TAU);
                    cr.restore().ok();
                };
                curve(cr);
                let _ = cr.fill();
                if style.border {
                    colors::set_source_alpha(cr, &colour, drawings::BORDER_ALPHA);
                    cr.set_line_width(edge_w);
                    curve(cr);
                    let _ = cr.stroke();
                }
            }
            preview_text(cr, (x, y, rw, rh), kind, theme, style, text, scale);
        }
        // The word itself, over the candles, which is the whole of what
        // this configuration decides. Centred, because there is no
        // figure for it to sit in the corner of.
        Kind::Text => {
            let box_h = h * 0.6;
            let box_at = (0.0, (h - box_h) / 2.0, w, box_h);
            preview_text(cr, box_at, Kind::Rect, theme, style, text, scale);
        }
        Kind::Profile => profile_sample(cr, theme, style, w * 0.25, h * 0.18, w * 0.5, h * 0.64),
        Kind::Vwap => vwap_sample(cr, theme, style, (w * 0.12, h * 0.2, w * 0.76, h * 0.6), line_w),
        Kind::Fib => fib_sample(cr, theme, style, (w * 0.12, h * 0.18, w * 0.76, h * 0.64)),
    }
}

/// The words a preview shows, in the style it is drawn in, placed in the
/// figure the way a real label would be.
///
/// A drawing's preview shows *that drawing's* words, so a label being typed
/// into the Text tab appears in the picture above it as it is typed and the
/// picture of a drawing that says nothing says nothing. Only a
/// configuration, which has no words of its own, gets [`SAMPLE`] — there a
/// stand-in is the only way the three text properties the configuration
/// does own are visible at all.
///
/// Real text through the same layout the chart uses, so what the tile shows
/// is what the chart will draw. Scaled with the tile, and skipped outright
/// when the tile is too small for the result to be anything but a smudge.
fn preview_text(
    cr: &gtk::cairo::Context,
    bounds: (f64, f64, f64, f64),
    kind: Kind,
    theme: &Theme,
    style: &Style,
    text: &Text,
    scale: f64,
) {
    if text.is_empty() {
        return;
    }
    let mut style = style.clone();
    style.text.size = style.text.clamped_size() * scale.sqrt();
    // Centred whatever the drawing's own placement is: a preview is a
    // picture of the look, and a label pinned to the bottom-left of a tile
    // this size is a label half off it.
    let text = drawings::Text { spans: text.spans.clone(), at: drawings::Place::Center };
    let Some((_, _, tw, th)) = crate::ui::text::block(kind, bounds, &text, &style) else { return };
    // Brought down to fit rather than dropped. A tile is a fraction of a
    // chart, so a label that is comfortable at full size is wider than the
    // figure here more often than not — and a preview that answers "what
    // does this say" with nothing, for most labels, is not a preview of
    // anything. It is shown smaller, which is what a picture of a thing at
    // a quarter of the size is anyway.
    if tw > 0.0 && th > 0.0 {
        // A hair under what would exactly fit. Scaling to the bound itself
        // asks for a block the same width as the box it goes in, and type
        // does not shrink linearly — it lands a pixel or two over as often
        // as under, and a pixel over was enough for the guard below to drop
        // the label entirely. A label that was *nearly* too wide drew
        // nothing at all, which is what this looked like.
        let fit = (bounds.2 / tw).min(bounds.3 / th) * 0.94;
        if fit < 1.0 {
            style.text.size = (style.text.size * fit).max(drawings::MIN_TEXT_SIZE);
        }
    }
    let Some((x, y, tw, th)) = crate::ui::text::block(kind, bounds, &text, &style) else { return };
    // Still too big, at the smallest the engine will set type: the tile has
    // no room for words at all, and a word spilling out of the box is a
    // worse picture than no word. A pixel of slack, because this is a
    // measurement of glyphs against a box and neither is exact.
    if tw > bounds.2 + 1.0 || th > bounds.3 + 1.0 {
        return;
    }
    let ground = drawings::text_ground(kind, &style, theme);
    let ink = drawings::text_colour(&style.text.colour, theme, &ground);
    crate::ui::text::draw(cr, (x, y), &text, &style.text, drawings::Align::Center, &ink, &ground);
}

/// What a configuration's preview says, having no words of its own.
///
/// Short enough for a tile, and a real word rather than lorem: the question
/// it is there to answer is whether a label in this configuration can be
/// read over its own fill, and that needs glyphs with stems and counters.
fn sample() -> Text {
    Text::plain("Note")
}

/// Every sample the arrow factory has drawn, so a colour change can ask
/// them to draw again. Weak, since the rows of a popped-down list go away.
type Samples = Rc<RefCell<Vec<glib::WeakRef<gtk::DrawingArea>>>>;

/// The arrow choices as pictures and nothing else: a short line in the
/// style's colour at its width, with a head at the ends the choice puts
/// one on. One factory serves the row and its list.
fn arrow_factory(theme: Theme, style: Rc<RefCell<Style>>) -> (gtk::SignalListItemFactory, Samples) {
    sample_factory(theme, style, |cr, style, arrow, w, h| {
        let line_w = style.width.min(4.0);
        let (a, b) = ((6.0, (h / 2.0).round()), (w - 6.0, (h / 2.0).round()));
        cr.set_line_width(line_w);
        cr.set_line_cap(gtk::cairo::LineCap::Round);
        cr.move_to(a.0, a.1);
        cr.line_to(b.0, b.1);
        let _ = cr.stroke();
        let arrow = Arrow::ALL.get(arrow).copied().unwrap_or_default();
        if arrow.at_end() {
            head(cr, a, b, line_w, style.head);
        }
        if arrow.at_start() {
            head(cr, b, a, line_w, style.head);
        }
    })
}

/// The arrowhead shapes as pictures: the same short line, with each shape
/// of head at its end.
fn head_factory(theme: Theme, style: Rc<RefCell<Style>>) -> (gtk::SignalListItemFactory, Samples) {
    sample_factory(theme, style, |cr, style, shape, w, h| {
        let line_w = style.width.min(4.0);
        let (a, b) = ((6.0, (h / 2.0).round()), (w - 6.0, (h / 2.0).round()));
        cr.set_line_width(line_w);
        cr.set_line_cap(gtk::cairo::LineCap::Round);
        cr.move_to(a.0, a.1);
        cr.line_to(b.0, b.1);
        let _ = cr.stroke();
        let shape = ArrowHead::ALL.get(shape).copied().unwrap_or_default();
        head(cr, a, b, line_w, shape);
    })
}

/// A list factory whose every row is a picture drawn by `paint`, given the
/// style of the moment and the row's position, with the line's colour
/// already set as the source. The pictures are collected so a change to
/// the colour or width can ask them to draw again.
fn sample_factory(
    theme: Theme,
    style: Rc<RefCell<Style>>,
    paint: impl Fn(&gtk::cairo::Context, &Style, usize, f64, f64) + Clone + 'static,
) -> (gtk::SignalListItemFactory, Samples) {
    let samples: Samples = Rc::new(RefCell::new(Vec::new()));
    let collected = samples.clone();
    let factory = controls::picture_factory(
        (84, 20),
        controls::Pictured::Alone,
        move |cr, position, w, h| {
            let style = style.borrow();
            colors::set_source(cr, &style.colour.hex(&theme));
            paint(cr, &style, position, w, h);
        },
        move |sample| collected.borrow_mut().push(sample.downgrade()),
    );
    (factory, samples)
}

fn head(cr: &gtk::cairo::Context, tail: (f64, f64), tip: (f64, f64), width: f64, shape: ArrowHead) {
    paint_head(cr, tail, tip, width, 5.0 + 2.0 * width, shape);
}

/// The header with its Done button, Ctrl+Enter through it, and the keyboard
/// back on the chart afterwards: Delete and Escape on it mean the drawing
/// that is still selected.
fn finish(
    dialog: &adw::Dialog,
    body: &impl IsA<gtk::Widget>,
    switcher: Option<&gtk::StackSwitcher>,
    area: &gtk::DrawingArea,
    tick: Option<glib::SourceId>,
) {
    let header = adw::HeaderBar::new();
    header.set_show_start_title_buttons(false);
    header.set_show_end_title_buttons(false);
    if let Some(switcher) = switcher {
        header.set_title_widget(Some(switcher));
    }
    let done = gtk::Button::with_label("Done");
    done.add_css_class("suggested-action");
    done.set_tooltip_text(Some(&format!("Done ({})", dialogs::commit_label())));
    let dialog_for_done = dialog.clone();
    done.connect_clicked(move |_| {
        let _ = dialog_for_done.close();
    });
    header.pack_end(&done);

    let content = adw::ToolbarView::new();
    content.add_top_bar(&header);
    content.set_content(Some(body));
    dialog.set_child(Some(&content));

    let done_on_key = done.clone();
    dialogs::commit_on_ctrl_enter(dialog, move || done_on_key.emit_clicked());

    let area = area.clone();
    let tick = RefCell::new(tick);
    dialog.connect_closed(move |_| {
        if let Some(tick) = tick.borrow_mut().take() {
            tick.remove();
        }
        let area = area.clone();
        glib::idle_add_local_once(move || {
            area.grab_focus();
        });
    });
}

/// Which preset a configuration starts from, for anything that wants to name
/// it: the nth, so configuration 1 is Up and 2 is Down.
pub fn preset_of(n: u8) -> Preset {
    Preset::ALL[(n.clamp(1, CONFIGURATIONS) - 1) as usize]
}
