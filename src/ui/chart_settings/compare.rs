//! The markets a chart is compared with: each in its colour, a way to add
//! one, where they are measured from, and whether the chart's own bars give
//! way to a line.

use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;

use crate::ui::window::Window;

/// The group, filled again whenever the chart's comparison changes.
pub(super) fn group(window: &Rc<Window>, dialog: &adw::PreferencesDialog) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::new();
    group.set_title("Compare");
    let rows: Rc<RefCell<Vec<gtk::Widget>>> = Rc::new(RefCell::new(Vec::new()));
    fill(window, dialog, &group, &rows);
    // A command, the chart's menu or its legend can change the list while
    // the dialog is open.
    let weak_group = group.downgrade();
    let weak_dialog = dialog.downgrade();
    let weak_window = Rc::downgrade(window);
    window.watch_chart(move || {
        let (Some(group), Some(dialog), Some(window)) =
            (weak_group.upgrade(), weak_dialog.upgrade(), weak_window.upgrade())
        else {
            return false;
        };
        // After whatever changed it has finished with the row it came from.
        let rows = rows.clone();
        gtk::glib::idle_add_local_once(move || fill(&window, &dialog, &group, &rows));
        true
    });
    group
}

fn fill(
    window: &Rc<Window>,
    dialog: &adw::PreferencesDialog,
    group: &adw::PreferencesGroup,
    rows: &Rc<RefCell<Vec<gtk::Widget>>>,
) {
    for row in rows.borrow_mut().drain(..) {
        group.remove(&row);
    }
    let compare = window.comparison();
    let colours = window.comparison_colors();
    let theme = window.theme();
    let mut added: Vec<gtk::Widget> = Vec::new();

    for (line, (comparison, colour)) in compare.symbols.iter().zip(&colours).enumerate() {
        let row = adw::ActionRow::new();
        row.set_title(&comparison.spelled());
        if let Some(name) = window.instrument_name(&comparison.symbol, comparison.suffix.as_deref()) {
            row.set_subtitle(&name);
        }
        let picker = {
            let window = window.clone();
            crate::ui::palette::picker(&theme, comparison.color.clone(), colour, move |picked| {
                window.set_comparison_color(line, Some(picked));
            })
        };
        row.add_suffix(&picker);
        let remove = gtk::Button::from_icon_name("user-trash-symbolic");
        remove.set_valign(gtk::Align::Center);
        remove.add_css_class("flat");
        remove.set_tooltip_text(Some("Stop comparing"));
        {
            let window = window.clone();
            remove.connect_clicked(move |_| window.remove_comparison(line));
        }
        row.add_suffix(&remove);
        added.push(row.upcast());
    }

    let add = adw::ActionRow::new();
    add.set_title("Compare with\u{2026}");
    add.set_activatable(true);
    add.add_prefix(&gtk::Image::from_icon_name("list-add-symbolic"));
    {
        let window = window.clone();
        let dialog = dialog.downgrade();
        // The search opens over the chart, which the dialog would cover.
        add.connect_activated(move |_| {
            if let Some(dialog) = dialog.upgrade() {
                dialog.close();
            }
            window.prompt_comparison();
        });
    }
    added.push(add.upcast());

    if compare.active() {
        let lines = adw::SwitchRow::new();
        lines.set_title("Lines only");
        lines.set_active(compare.lines_only);
        {
            let window = window.clone();
            lines.connect_active_notify(move |row| window.set_compare_lines_only(row.is_active()));
        }
        added.push(lines.upcast());

        let from = adw::ActionRow::new();
        from.set_title("Measured from");
        let at = gtk::Label::new(Some(&match compare.from {
            None => "Left edge".to_string(),
            Some(ts) => window.day_of(ts),
        }));
        at.add_css_class("dim-label");
        from.add_suffix(&at);
        if compare.from.is_some() {
            let unpin = gtk::Button::with_label("Left edge");
            unpin.set_valign(gtk::Align::Center);
            {
                let window = window.clone();
                unpin.connect_clicked(move |_| window.set_compare_from(None));
            }
            from.add_suffix(&unpin);
        }
        added.push(from.upcast());
    }

    for row in &added {
        group.add(row);
    }
    *rows.borrow_mut() = added;
}
