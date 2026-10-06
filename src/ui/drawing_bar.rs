//! The drawing tools, on a bar that slides in from the left.
//!
//! The rail of watchlists lives on the right; the tools live on the left, the
//! same way: a key, or a subtle handle on the edge, brings them in, and they
//! take no room when they are out. One column of buttons, since there are
//! two tools and a column of three is already more than a toolbar needs.
//! Each button is a sign the app drew itself — a pointer, a line with its
//! grips, a box with two — in the palette's own ink and nothing else, dim
//! until it is hovered or lit, rather than a stock glyph that would look
//! like every other application's or a preview that would compete with the
//! chart.
//!
//! The first button is no tool at all: the pointer, which selects and moves
//! what is drawn, and is what Escape goes back to. One button is lit at a
//! time, the way radio buttons are, and clicking the lit one leaves it lit:
//! the way to put a tool down is to pick another, or the pointer. A
//! right-click on a tool opens that kind's configurations.

use std::rc::Rc;

use gtk::prelude::*;
use gtk::{gio, glib};
use omacharts_engine::drawings::Kind;

/// The width of a tool button, which is also the bar's.
const TOOL: i32 = 36;

pub struct DrawingBar {
    /// What goes in the layout: the revealer, so the bar can slide.
    pub root: gtk::Revealer,
    /// The pointer first, as `None`, then a button a tool.
    buttons: Vec<(Option<Kind>, gtk::ToggleButton, gtk::DrawingArea)>,
    /// The configuration number worn by the tool in hand, so Alt+R, Alt+3
    /// shows a 3 on the rectangle before anything is drawn. One per button,
    /// the pointer's never shown.
    badges: Vec<gtk::Label>,
    /// Set while the buttons are being shown a state, so their own signal
    /// does not read as a click.
    showing: std::cell::Cell<bool>,
}

impl DrawingBar {
    /// `on_arm` is told which tool was picked, or `None` for the pointer;
    /// `on_configure` which kind's configurations to open.
    pub fn new(
        on_arm: impl Fn(Option<Kind>) + Clone + 'static,
        on_configure: impl Fn(Kind) + Clone + 'static,
    ) -> Rc<DrawingBar> {
        let column = gtk::Box::new(gtk::Orientation::Vertical, 4);
        column.add_css_class("drawing-bar");
        column.set_valign(gtk::Align::Start);
        column.set_margin_top(10);
        column.set_margin_start(6);
        column.set_margin_end(2);

        let mut buttons: Vec<(Option<Kind>, gtk::ToggleButton, gtk::DrawingArea)> = Vec::new();
        let mut badges = Vec::new();
        for kind in std::iter::once(None).chain(Kind::ALL.into_iter().map(Some)) {
            let icon = gtk::DrawingArea::new();
            icon.set_size_request(TOOL - 10, TOOL - 10);
            let badge = gtk::Label::new(None);
            badge.add_css_class("drawing-config-badge");
            badge.set_halign(gtk::Align::End);
            badge.set_valign(gtk::Align::End);
            badge.set_visible(false);
            let stack = gtk::Overlay::new();
            stack.set_child(Some(&icon));
            stack.add_overlay(&badge);
            badges.push(badge);
            let button = gtk::ToggleButton::new();
            button.add_css_class("drawing-tool");
            button.add_css_class("flat");
            button.set_child(Some(&stack));
            button.set_size_request(TOOL, TOOL);
            button.set_tooltip_text(Some(match kind {
                None => "Pointer (Esc): select, move and resize drawings",
                Some(Kind::Line) => "Line (Alt+L): click where it starts, then where it ends",
                Some(Kind::Rect) => "Rectangle (Alt+R): press at one corner, release at the other",
            }));
            // One lit at a time, and a click on the lit one changes nothing:
            // a group of toggles is a set of radio buttons.
            if let Some((_, first, _)) = buttons.first() {
                button.set_group(Some(first));
            }
            column.append(&button);

            let Some(kind) = kind else {
                buttons.push((None, button, icon));
                continue;
            };

            // Right-click: the configurations of this kind.
            let menu = gio::Menu::new();
            let item = gio::MenuItem::new(Some(&format!("{} configurations…", kind.label())), None);
            item.set_action_and_target_value(Some("tools.configure"), Some(&kind.key().to_variant()));
            menu.append_item(&item);
            let popover = gtk::PopoverMenu::from_model(Some(&menu));
            popover.set_parent(&button);
            popover.set_has_arrow(false);
            let right = gtk::GestureClick::new();
            right.set_button(gtk::gdk::BUTTON_SECONDARY);
            let popover_for_click = popover.clone();
            right.connect_pressed(move |_, _, x, y| {
                popover_for_click.set_pointing_to(Some(&gtk::gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
                popover_for_click.popup();
            });
            button.add_controller(right);
            buttons.push((Some(kind), button, icon));
        }

        let bar = Rc::new(DrawingBar {
            root: gtk::Revealer::new(),
            buttons,
            badges,
            showing: std::cell::Cell::new(false),
        });
        bar.root.set_transition_type(gtk::RevealerTransitionType::SlideRight);
        bar.root.set_transition_duration(160);
        bar.root.set_child(Some(&column));
        bar.root.set_reveal_child(false);

        for (kind, button, _) in &bar.buttons {
            let kind = *kind;
            let on_arm = on_arm.clone();
            let weak = Rc::downgrade(&bar);
            button.connect_toggled(move |button| {
                let Some(bar) = weak.upgrade() else { return };
                // The group lights one and dims the rest; only the one
                // lit is a choice.
                if bar.showing.get() || !button.is_active() {
                    return;
                }
                on_arm(kind);
            });
        }

        let actions = gio::SimpleActionGroup::new();
        let configure = gio::SimpleAction::new("configure", Some(glib::VariantTy::STRING));
        configure.connect_activate(move |_, target| {
            let Some(kind) = target.and_then(|t| t.str()).and_then(Kind::from_key) else { return };
            on_configure(kind);
        });
        actions.add_action(&configure);
        column.insert_action_group("tools", Some(&actions));

        bar.paint();
        // The pointer, until a tool is picked.
        bar.show_armed(None, 1);
        bar
    }

    /// Light the button of the tool in hand — the pointer, when there is
    /// none — with the configuration it will draw in on it, when that is
    /// not the first: the usual one needs no saying, and a badge that is
    /// always there is one nobody reads.
    pub fn show_armed(&self, armed: Option<Kind>, config: u8) {
        self.showing.set(true);
        for ((kind, button, _), badge) in self.buttons.iter().zip(&self.badges) {
            let lit = armed == *kind;
            button.set_active(lit);
            badge.set_text(&format!("{config}"));
            badge.set_visible(lit && kind.is_some() && config != 1);
        }
        self.showing.set(false);
    }

    pub fn set_shown(&self, shown: bool) {
        self.root.set_reveal_child(shown);
    }

    pub fn is_shown(&self) -> bool {
        self.root.reveals_child()
    }

    /// After a theme change: the ink is the button's own colour, read at
    /// draw time, so there is nothing to do but draw.
    pub fn restyle(&self) {
        for (_, _, icon) in &self.buttons {
            icon.queue_draw();
        }
    }

    /// The signs, in the button's ink: the palette's foreground, which the
    /// button dims until it is hovered or lit. No colour of their own and
    /// no candle under the box — a toolbar is a row of signs, not of
    /// previews, and the chart beside it is the only thing that should
    /// have colour.
    fn paint(&self) {
        for (kind, _, icon) in &self.buttons {
            let kind = *kind;
            icon.set_draw_func(move |area, cr, w, h| {
                let (w, h) = (w as f64, h as f64);
                let fg = area.color();
                let (r, g, b, a) = (fg.red() as f64, fg.green() as f64, fg.blue() as f64, fg.alpha() as f64);
                cr.set_source_rgba(r, g, b, a);
                cr.set_line_width(1.5);
                cr.set_line_cap(gtk::cairo::LineCap::Round);
                cr.set_line_join(gtk::cairo::LineJoin::Round);
                match kind {
                    None => {
                        // The arrow every pointer is.
                        let (x, y) = ((w / 2.0 - 5.0).round() + 0.5, (h / 2.0 - 7.5).round() + 0.5);
                        cr.move_to(x, y);
                        cr.line_to(x, y + 14.0);
                        cr.line_to(x + 3.5, y + 10.5);
                        cr.line_to(x + 6.0, y + 16.0);
                        cr.line_to(x + 8.5, y + 15.0);
                        cr.line_to(x + 6.0, y + 9.5);
                        cr.line_to(x + 11.0, y + 9.5);
                        cr.close_path();
                        let _ = cr.fill();
                    }
                    Some(Kind::Line) => {
                        // A stroke with a grip at each end, which is what
                        // says "a drawing" rather than "a slash".
                        let (a, b) = ((4.5, h - 4.5), (w - 4.5, 4.5));
                        cr.move_to(a.0, a.1);
                        cr.line_to(b.0, b.1);
                        let _ = cr.stroke();
                        for (x, y) in [a, b] {
                            cr.rectangle(x - 2.5, y - 2.5, 5.0, 5.0);
                            let _ = cr.fill();
                        }
                    }
                    Some(Kind::Rect) => {
                        // A box, faintly filled the way the drawn one is,
                        // with a grip at the two corners a hand places.
                        let (x, y, rw, rh) = (4.5, 5.5, w - 9.0, h - 11.0);
                        cr.rectangle(x, y, rw, rh);
                        cr.set_source_rgba(r, g, b, a * 0.18);
                        let _ = cr.fill_preserve();
                        cr.set_source_rgba(r, g, b, a);
                        cr.set_line_width(1.0);
                        let _ = cr.stroke();
                        for (gx, gy) in [(x, y), (x + rw, y + rh)] {
                            cr.rectangle(gx - 2.5, gy - 2.5, 5.0, 5.0);
                            let _ = cr.fill();
                        }
                    }
                }
            });
            icon.queue_draw();
        }
    }
}

/// The corner button's glyph: a pen stroke with a grip at its end, in the
/// button's own colour, so it dims with the rest of the corner.
pub fn tools_icon() -> gtk::DrawingArea {
    let area = gtk::DrawingArea::new();
    area.set_size_request(16, 16);
    area.set_draw_func(|area, cr, w, h| {
        let (w, h) = (w as f64, h as f64);
        let fg = area.color();
        cr.set_source_rgba(fg.red() as f64, fg.green() as f64, fg.blue() as f64, fg.alpha() as f64);
        cr.set_line_width(1.8);
        cr.set_line_cap(gtk::cairo::LineCap::Round);
        cr.move_to(3.0, h - 3.0);
        cr.line_to(w - 4.0, 4.0);
        let _ = cr.stroke();
        cr.rectangle(w - 6.5, 1.5, 5.0, 5.0);
        let _ = cr.fill();
        cr.rectangle(0.5, h - 5.5, 5.0, 5.0);
        let _ = cr.fill();
    });
    area
}
