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

use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;
use gtk::{gio, glib};
use omacharts_engine::drawings::{
    self, Arrow, Configurations, Kind, Paint, Preset, Scope, Style, CONFIGURATIONS,
};
use omacharts_engine::Theme;

use crate::store::Store;
use crate::ui::colors;
use crate::ui::dialogs;
use crate::ui::palette;
use crate::ui::pane::ChartPane;
use crate::ui::window::Window;

/// The size of a configuration's preview tile.
const PREVIEW_W: i32 = 84;
const PREVIEW_H: i32 = 44;
/// How strong the candles under a preview are: ground, not subject.
const CANDLE_ALPHA: f64 = 0.5;

/// Something that shows a row a value: the editor keeps one per row so a
/// style chosen elsewhere can be put in front of the hand.
type Shower<T> = Rc<dyn Fn(&T)>;

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
    let theme = window.theme();
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
    paint_preview(&shown_preview, &theme, kind, drawing.style(&configs_now));
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
    let editor = style_editor(window, kind, drawing.style(&configs_now).clone(), on_style);
    page.add(&editor.group);

    // Who else sees it: one row, a little apart from the look.
    let sharing = adw::PreferencesGroup::new();
    sharing.set_margin_top(12);
    let scope_row = adw::ComboRow::new();
    scope_row.set_title("Shown on");
    let scopes = Scope::all();
    let names: Vec<String> = scopes.iter().map(|s| s.label()).collect();
    let names: Vec<&str> = names.iter().map(String::as_str).collect();
    scope_row.set_model(Some(&gtk::StringList::new(&names)));
    scope_row.set_selected(scopes.iter().position(|s| *s == drawing.scope).unwrap_or(1) as u32);
    {
        let view = view.clone();
        let scopes = scopes.clone();
        scope_row.connect_selected_notify(move |row| {
            let Some(scope) = scopes.get(row.selected() as usize).copied() else { return };
            view.edit_selected(move |d| d.scope = scope);
        });
    }
    sharing.add(&scope_row);
    page.add(&sharing);

    // Removing is the chart's business — Delete, or the drawing's menu —
    // not a button at the bottom of its properties.
    let dialog = adw::Dialog::new();
    dialog.set_title(&format!("{} properties", kind.label()));
    dialog.set_content_width(440);

    *refresh.borrow_mut() = Some({
        let view = view.clone();
        let label = shown_label.clone();
        let preview = shown_preview.clone();
        let save_row = save_row.clone();
        let editor = editor.clone();
        let window = window.clone();
        let theme = theme.clone();
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
                    save_row.set_visible(true);
                }
            }
            let style = d.style(&configs);
            paint_preview(&preview, &theme, kind, style);
            editor.show(style);
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
            let grid = configuration_grid(&theme, kind, &configs, d.config, custom.as_ref(), {
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

    let save_menu = gio::Menu::new();
    for n in 1..=CONFIGURATIONS {
        let item = gio::MenuItem::new(Some(&format!("Configuration {n}")), None);
        item.set_action_and_target_value(Some("drawing.save-as"), Some(&(n as i32).to_variant()));
        save_menu.append_item(&item);
    }
    save.set_menu_model(Some(&save_menu));
    // Under the button, between the picture and the rest of the sheet,
    // where the eye already is; not up over the picture it was just given.
    save.set_direction(gtk::ArrowType::Down);
    if let Some(popover) = save.popover() {
        popover.set_position(gtk::PositionType::Bottom);
    }
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

    finish(&dialog, &page, &view.area, Some(tick));
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
        let preview = preview_tile(&theme, kind, &style);
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
                            paint_preview(preview, &theme, kind, &style);
                            let changed = !configs.is_default(kind, n);
                            edited.set_visible(changed);
                            mark_edited(row, changed);
                        }
                    })
                };
                let editor = style_editor(&window, kind, current, on_style);
                let sub = adw::PreferencesPage::new();
                sub.add(&editor.group);
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
                paint_preview(preview, &theme, kind, style);
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

/// A configuration in words, for the row under its number.
fn describe(kind: Kind, style: &Style) -> String {
    match kind {
        Kind::Line => {
            let arrow = match style.arrow {
                Arrow::None => String::new(),
                arrow => format!(", arrow {}", arrow.label().to_lowercase()),
            };
            format!("{}, {}px{arrow}", name_of(&style.colour), style.width)
        }
        Kind::Rect => {
            let edge = match style.border {
                true => format!(", edge {} {}px", name_of(&style.colour), style.width),
                false => ", no edge".to_string(),
            };
            format!("{} at {:.0}%{edge}", name_of(&style.fill), style.alpha * 100.0)
        }
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
    show: Rc<dyn Fn(&Style)>,
}

impl Editor {
    fn show(&self, style: &Style) {
        (self.show)(style);
    }
}

/// The rows for a kind's properties. Every change calls `on_style` with the
/// whole style as it now is.
fn style_editor(window: &Rc<Window>, kind: Kind, current: Style, on_style: Rc<dyn Fn(Style)>) -> Editor {
    let theme = window.theme();
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
        Rc::new(move || {
            if showing.get() {
                return;
            }
            // Cloned out before the call, not inside it: a temporary borrow
            // in an argument lives to the end of the statement, and the
            // handler repaints the preview, which shows the editor the style
            // again and needs the cell for itself.
            let current = style.borrow().clone();
            on_style(current);
        })
    };

    let mut shows: Vec<Shower<Style>> = Vec::new();

    match kind {
        Kind::Line => {
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
                // The samples are in the colour and width of the moment,
                // which the rows above may just have changed.
                samples.borrow_mut().retain(|weak| weak.upgrade().is_some());
                for sample in samples.borrow().iter().filter_map(|weak| weak.upgrade()) {
                    sample.queue_draw();
                }
            }));
        }
        Kind::Rect => {
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
    }

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
    Editor { group, show }
}

/// A row that picks a width, in pixels, by number.
fn width_row(title: &str, current: f64, on_pick: impl Fn(f64) + 'static) -> (adw::ActionRow, Rc<dyn Fn(f64)>) {
    let row = adw::ActionRow::new();
    row.set_title(title);
    let spin = gtk::SpinButton::with_range(0.5, 12.0, 0.5);
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
    let shown = palette::swatch_area(&current.hex(theme), false);
    let button = gtk::MenuButton::new();
    button.set_child(Some(&shown));
    button.set_valign(gtk::Align::Center);
    button.add_css_class("flat");
    let popover = gtk::Popover::new();

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
    row.add_suffix(&button);

    let theme = theme.clone();
    let shown_for_show = shown.clone();
    (row, Rc::new(move |paint: &Paint| palette::paint(&shown_for_show, &paint.hex(&theme), false)))
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
        let preview = preview_tile(theme, kind, configs.of(kind, n));
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
        let preview = preview_tile(theme, kind, custom);
        preview.add_css_class("drawing-preview-current");
        let cell = tile(preview, "Custom", "This drawing's own look, as it is now");
        cell.set_sensitive(false);
        grid.attach(&cell, 0, 3, 1, 1);
    }
    content.append(&grid);
    content
}

/// A small chart with the drawing on it: three candles in the theme's own
/// colours, and the drawing as the style says, so a picker shows the look
/// rather than naming it.
fn preview_tile(theme: &Theme, kind: Kind, style: &Style) -> gtk::DrawingArea {
    let area = gtk::DrawingArea::new();
    area.set_size_request(PREVIEW_W, PREVIEW_H);
    area.add_css_class("drawing-preview");
    paint_preview(&area, theme, kind, style);
    area
}

/// Paint a small chart with the drawing on it, at whatever size the area
/// has. The same picture serves a tile in the menu and the wide strip at
/// the top of the properties: the candles and the drawing scale with the
/// height, and more candles fill a wider strip, so neither looks like the
/// other blown up or shrunk down.
fn paint_preview(area: &gtk::DrawingArea, theme: &Theme, kind: Kind, style: &Style) {
    let theme = theme.clone();
    let style = style.clone();
    area.set_draw_func(move |_, cr, w, h| draw_preview(cr, w as f64, h as f64, &theme, kind, &style));
    area.queue_draw();
}

/// The picture itself, on any surface: a strip of candles with the
/// drawing over them, the way it sits on a chart.
fn draw_preview(cr: &gtk::cairo::Context, w: f64, h: f64, theme: &Theme, kind: Kind, style: &Style) {
    {
        let scale = (h / PREVIEW_H as f64).clamp(1.0, 3.0);
        let bars = omacharts_engine::theme::theme_bars(theme);
        colors::set_source(cr, &theme.ui.background);
        rounded(cr, 0.5, 0.5, w - 1.0, h - 1.0, 5.0);
        let _ = cr.fill();
        // Candles across the middle, a run of them in a wide strip and four
        // in a tile, rising and falling the way a real stretch does.
        let pitch = 14.0 * scale;
        let count = ((w * 0.7) / pitch).floor().max(4.0) as usize;
        let left = (w - (count as f64 - 1.0) * pitch) / 2.0;
        let body_w = (5.0 * scale).round();
        let body_w = if body_w % 2.0 == 0.0 { body_w + 1.0 } else { body_w };
        let (wick_h, body_h) = ((20.0 * scale).round(), (10.0 * scale).round());
        for i in 0..count {
            let t = i as f64 / (count as f64 - 1.0).max(1.0);
            let wave = ((t * 6.0).sin() * 0.18) + ((t * 2.0).cos() * 0.1);
            let (x, y) = (left + i as f64 * pitch, (h * (0.5 - wave)).round());
            // Wick and body share one pixel column: a wick drawn a pixel to
            // the right of its body is the first thing the eye catches.
            let column = x.floor() + 0.5;
            let wick_w = scale.round().max(1.0);
            let up = i % 3 != 1;
            // At less than full strength: the candles are the ground the
            // drawing sits on, not the subject, and at full saturation a
            // body punches through a translucent fill as if it were on
            // top of it.
            colors::set_source_alpha(cr, if up { &bars.up } else { &bars.down }, CANDLE_ALPHA);
            cr.rectangle(column - wick_w / 2.0, y - wick_h / 2.0, wick_w, wick_h);
            let _ = cr.fill();
            cr.rectangle(column - body_w / 2.0, y - body_h / 2.0, body_w, body_h);
            let _ = cr.fill();
        }
        let colour = style.colour.hex(theme);
        let line_w = style.width.min(4.0) * scale.sqrt();
        match kind {
            Kind::Line => {
                let (a, b) = ((w * 0.15, h * 0.75), (w * 0.85, h * 0.25));
                colors::set_source(cr, &colour);
                cr.set_line_width(line_w);
                cr.set_line_cap(gtk::cairo::LineCap::Round);
                cr.move_to(a.0, a.1);
                cr.line_to(b.0, b.1);
                let _ = cr.stroke();
                if style.arrow.at_end() {
                    head(cr, a, b, line_w);
                }
                if style.arrow.at_start() {
                    head(cr, b, a, line_w);
                }
            }
            Kind::Rect => {
                let (x, y, rw, rh) = (w * 0.3, h * 0.2, w * 0.4, h * 0.6);
                colors::set_source_alpha(cr, &style.fill.hex(theme), style.alpha);
                cr.rectangle(x, y, rw, rh);
                let _ = cr.fill();
                if style.border {
                    colors::set_source_alpha(cr, &colour, drawings::BORDER_ALPHA);
                    cr.set_line_width(style.width.clamp(1.0, 3.0) * scale.sqrt());
                    cr.rectangle(x.round() + 0.5, y.round() + 0.5, rw.round(), rh.round());
                    let _ = cr.stroke();
                }
            }
        }
    }
}

/// Every sample the arrow factory has drawn, so a colour change can ask
/// them to draw again. Weak, since the rows of a popped-down list go away.
type Samples = Rc<RefCell<Vec<glib::WeakRef<gtk::DrawingArea>>>>;

/// The arrow choices as pictures: a short line in the style's colour at
/// its width, with a head at the ends the choice puts one on, and the
/// choice's name beside it. One factory serves the row and its list.
fn arrow_factory(theme: Theme, style: Rc<RefCell<Style>>) -> (gtk::SignalListItemFactory, Samples) {
    let factory = gtk::SignalListItemFactory::new();
    let samples: Samples = Rc::new(RefCell::new(Vec::new()));
    factory.connect_setup(move |_, item| {
        let Some(item) = item.downcast_ref::<gtk::ListItem>() else { return };
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
        let sample = gtk::DrawingArea::new();
        sample.set_size_request(56, 18);
        sample.set_valign(gtk::Align::Center);
        let label = gtk::Label::new(None);
        label.set_xalign(0.0);
        row.append(&sample);
        row.append(&label);
        item.set_child(Some(&row));
    });
    {
        let samples = samples.clone();
        factory.connect_bind(move |_, item| {
            let Some(item) = item.downcast_ref::<gtk::ListItem>() else { return };
            let Some(row) = item.child().and_downcast::<gtk::Box>() else { return };
            let Some(sample) = row.first_child().and_downcast::<gtk::DrawingArea>() else { return };
            let Some(label) = row.last_child().and_downcast::<gtk::Label>() else { return };
            let arrow = Arrow::ALL.get(item.position() as usize).copied().unwrap_or_default();
            label.set_text(arrow.label());
            let theme = theme.clone();
            let style = style.clone();
            sample.set_draw_func(move |_, cr, w, h| {
                let (w, h) = (w as f64, h as f64);
                let style = style.borrow();
                let line_w = style.width.min(4.0);
                let (a, b) = ((4.0, (h / 2.0).round()), (w - 4.0, (h / 2.0).round()));
                colors::set_source(cr, &style.colour.hex(&theme));
                cr.set_line_width(line_w);
                cr.set_line_cap(gtk::cairo::LineCap::Round);
                cr.move_to(a.0, a.1);
                cr.line_to(b.0, b.1);
                let _ = cr.stroke();
                if arrow.at_end() {
                    head(cr, a, b, line_w);
                }
                if arrow.at_start() {
                    head(cr, b, a, line_w);
                }
            });
            samples.borrow_mut().push(sample.downgrade());
        });
    }
    (factory, samples)
}

fn head(cr: &gtk::cairo::Context, tail: (f64, f64), tip: (f64, f64), width: f64) {
    let (dx, dy) = (tip.0 - tail.0, tip.1 - tail.1);
    let length = dx.hypot(dy).max(1.0);
    let (ux, uy) = (dx / length, dy / length);
    let size = 5.0 + 2.0 * width;
    let (bx, by) = (tip.0 - ux * size, tip.1 - uy * size);
    let (px, py) = (-uy * size * 0.45, ux * size * 0.45);
    cr.move_to(tip.0, tip.1);
    cr.line_to(bx + px, by + py);
    cr.line_to(bx - px, by - py);
    cr.close_path();
    let _ = cr.fill();
}

fn rounded(cr: &gtk::cairo::Context, x: f64, y: f64, w: f64, h: f64, r: f64) {
    let r = r.min(w / 2.0).min(h / 2.0);
    cr.new_sub_path();
    cr.arc(x + w - r, y + r, r, -std::f64::consts::FRAC_PI_2, 0.0);
    cr.arc(x + w - r, y + h - r, r, 0.0, std::f64::consts::FRAC_PI_2);
    cr.arc(x + r, y + h - r, r, std::f64::consts::FRAC_PI_2, std::f64::consts::PI);
    cr.arc(x + r, y + r, r, std::f64::consts::PI, 1.5 * std::f64::consts::PI);
    cr.close_path();
}

/// The header with its Done button, Ctrl+Enter through it, and the keyboard
/// back on the chart afterwards: Delete and Escape on it mean the drawing
/// that is still selected.
fn finish(dialog: &adw::Dialog, page: &adw::PreferencesPage, area: &gtk::DrawingArea, tick: Option<glib::SourceId>) {
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

    let content = adw::ToolbarView::new();
    content.add_top_bar(&header);
    content.set_content(Some(page));
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
