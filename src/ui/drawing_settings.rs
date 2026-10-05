//! The settings of one drawing: its colour, and for a line its thickness.
//!
//! One small dialog rather than a page in the chart's settings, because a
//! drawing is a thing on the chart and not a setting of it: it is reached by
//! right-clicking the drawing, and the dialog is about that drawing alone.
//! Everything in it applies as it is chosen — the chart is right there behind
//! the dialog, and seeing the line turn amber is the whole of the feedback —
//! so there is nothing to confirm. Done, and Ctrl+Enter, just close it;
//! Escape closes it the way it closes every `AdwDialog`.
//!
//! The colours are the nine presets, roles the theme fills, and never a hex:
//! a drawing that followed the theme into every other colour and then wore
//! one fixed red would be the one thing on the chart that did not belong.

use std::rc::Rc;

use adw::prelude::*;
use gtk::glib;
use omacharts_engine::drawings::{self, Kind as DrawingKind, Preset};

use crate::ui::dialogs;
use crate::ui::palette;
use crate::ui::pane::ChartPane;
use crate::ui::window::Window;

/// The widths a line can have. The chart's default is in the middle.
const WIDTHS: [f64; 4] = [1.0, 1.5, 2.5, 4.0];

/// Open the dialog for the drawing selected on `pane`. Nothing selected,
/// nothing opens.
pub fn present(window: &Rc<Window>, pane: &Rc<ChartPane>) {
    let view = pane.view.clone();
    let Some(drawing) = view.selected_drawing() else { return };
    let theme = window.theme();

    let page = adw::PreferencesPage::new();

    // Colour: the nine presets as tiles, the one in use ringed. Repainted
    // on every pick rather than rebuilt, so the grid never moves under the
    // hand.
    let colour = adw::PreferencesGroup::new();
    colour.set_title("Colour");
    let grid = gtk::Grid::new();
    grid.set_row_spacing(6);
    grid.set_column_spacing(6);
    grid.set_halign(gtk::Align::Start);
    let tiles: Rc<Vec<(Preset, String, gtk::DrawingArea)>> = Rc::new(
        drawings::palette(&theme)
            .into_iter()
            .map(|(preset, hex)| {
                let area = palette::swatch_area(&hex, preset == drawing.preset);
                (preset, hex, area)
            })
            .collect(),
    );
    for (n, (preset, _, area)) in tiles.iter().enumerate() {
        let cell = gtk::Button::new();
        cell.add_css_class("flat");
        cell.set_tooltip_text(Some(preset.name()));
        cell.set_child(Some(area));
        let view = view.clone();
        let tiles = tiles.clone();
        let chosen = *preset;
        cell.connect_clicked(move |_| {
            view.edit_selected(|d| d.preset = chosen);
            for (preset, hex, area) in tiles.iter() {
                palette::paint(area, hex, *preset == chosen);
            }
        });
        grid.attach(&cell, n as i32 % 5, n as i32 / 5, 1, 1);
    }
    let row = adw::ActionRow::new();
    row.set_activatable(false);
    row.set_child(Some(&grid));
    colour.add(&row);
    page.add(&colour);

    // Thickness, for a line. A box has a hairline edge by design, so it is
    // not asked.
    if drawing.kind == DrawingKind::Line {
        let line = adw::PreferencesGroup::new();
        line.set_title("Line");
        let row = adw::ActionRow::new();
        row.set_title("Thickness");
        let choices = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        choices.add_css_class("linked");
        choices.set_valign(gtk::Align::Center);
        let mut first: Option<gtk::ToggleButton> = None;
        for width in WIDTHS {
            let button = gtk::ToggleButton::with_label(&format!("{width}"));
            button.set_tooltip_text(Some(&format!("{width} pixels")));
            if let Some(first) = &first {
                button.set_group(Some(first));
            }
            button.set_active((drawing.width - width).abs() < 0.01);
            let view = view.clone();
            button.connect_toggled(move |button| {
                if button.is_active() {
                    view.edit_selected(|d| d.width = width);
                }
            });
            choices.append(&button);
            first.get_or_insert(button);
        }
        row.add_suffix(&choices);
        line.add(&row);
        page.add(&line);
    }

    let dialog = adw::Dialog::new();
    dialog.set_title(drawing.kind.label());
    dialog.set_content_width(380);

    // Deleting is the one thing here that cannot be undone by choosing
    // again, so it sits apart and looks like what it is.
    let remove = adw::PreferencesGroup::new();
    let delete = gtk::Button::with_label("Delete drawing");
    delete.add_css_class("destructive-action");
    delete.set_halign(gtk::Align::Start);
    let view_for_delete = view.clone();
    let dialog_for_delete = dialog.clone();
    delete.connect_clicked(move |_| {
        view_for_delete.delete_selected();
        let _ = dialog_for_delete.close();
    });
    remove.add(&delete);
    page.add(&remove);

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
    content.set_content(Some(&page));
    dialog.set_child(Some(&content));

    // The key goes through the button rather than around it, so there is
    // one way to finish and it is the one the tooltip names.
    let done_on_key = done.clone();
    dialogs::commit_on_ctrl_enter(&dialog, move || done_on_key.emit_clicked());

    // The chart keeps the keyboard afterwards: Delete and Escape on it mean
    // the drawing that is still selected.
    let area = view.area.clone();
    dialog.connect_closed(move |_| {
        let area = area.clone();
        glib::idle_add_local_once(move || {
            area.grab_focus();
        });
    });

    dialog.present(Some(&window.window));
}
