//! The drawing tools, on a bar that slides in from the left.
//!
//! The rail of watchlists lives on the right; the tools live on the left, the
//! same way: a key, or a subtle handle on the edge, brings them in, and they
//! take no room when they are out. One column of buttons, since there are
//! two tools and a column of two is already more than a toolbar needs. Each
//! button is an icon the app drew itself in the theme's own colours — a line
//! with its grips, a box over a candle — rather than a stock glyph that
//! would look like every other application's.
//!
//! A button is lit while its tool is in hand, and clicking it again puts the
//! tool down. Right-clicking one opens that kind's configurations.

use std::cell::RefCell;
use std::rc::Rc;

use gtk::prelude::*;
use gtk::{gio, glib};
use omacharts_engine::drawings::{self, Kind, Paint};
use omacharts_engine::Theme;

use crate::ui::colors;

/// The width of a tool button, which is also the bar's.
const TOOL: i32 = 36;

pub struct DrawingBar {
    /// What goes in the layout: the revealer, so the bar can slide.
    pub root: gtk::Revealer,
    buttons: Vec<(Kind, gtk::ToggleButton, gtk::DrawingArea)>,
    /// The configuration number worn by the tool in hand, so Alt+R, Alt+3
    /// shows a 3 on the rectangle before anything is drawn.
    badges: Vec<gtk::Label>,
    theme: RefCell<Theme>,
    /// Set while the buttons are being shown a state, so their own signal
    /// does not read as a click.
    showing: std::cell::Cell<bool>,
}

impl DrawingBar {
    /// `on_arm` is told which tool was picked, or `None` for the one in hand
    /// being put down; `on_configure` which kind's configurations to open.
    pub fn new(
        theme: Theme,
        on_arm: impl Fn(Option<Kind>) + Clone + 'static,
        on_configure: impl Fn(Kind) + Clone + 'static,
    ) -> Rc<DrawingBar> {
        let column = gtk::Box::new(gtk::Orientation::Vertical, 4);
        column.add_css_class("drawing-bar");
        column.set_valign(gtk::Align::Start);
        column.set_margin_top(10);
        column.set_margin_start(6);
        column.set_margin_end(2);

        let mut buttons = Vec::new();
        let mut badges = Vec::new();
        for kind in Kind::ALL {
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
                Kind::Line => "Line (Alt+L): click where it starts, then where it ends",
                Kind::Rect => "Rectangle (Alt+R): press at one corner, release at the other",
            }));
            column.append(&button);

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
            buttons.push((kind, button, icon));
        }

        let bar = Rc::new(DrawingBar {
            root: gtk::Revealer::new(),
            buttons,
            badges,
            theme: RefCell::new(theme),
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
                if bar.showing.get() {
                    return;
                }
                on_arm(if button.is_active() { Some(kind) } else { None });
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
        bar
    }

    /// Light the button of the tool in hand, and only that one, with the
    /// configuration it will draw in on it.
    pub fn show_armed(&self, armed: Option<Kind>, config: u8) {
        self.showing.set(true);
        for ((kind, button, _), badge) in self.buttons.iter().zip(&self.badges) {
            let lit = armed == Some(*kind);
            button.set_active(lit);
            badge.set_text(&format!("{config}"));
            badge.set_visible(lit);
        }
        self.showing.set(false);
    }

    pub fn set_shown(&self, shown: bool) {
        self.root.set_reveal_child(shown);
    }

    pub fn is_shown(&self) -> bool {
        self.root.reveals_child()
    }

    pub fn restyle(&self, theme: Theme) {
        *self.theme.borrow_mut() = theme;
        self.paint();
    }

    /// The icons, in the theme's colours: the line is the first
    /// configuration's and the box the second's, the way the first two
    /// configurations are the candles' colours.
    fn paint(&self) {
        let theme = self.theme.borrow().clone();
        for (kind, _, icon) in &self.buttons {
            let kind = *kind;
            let theme = theme.clone();
            icon.set_draw_func(move |area, cr, w, h| {
                let (w, h) = (w as f64, h as f64);
                let fg = area.color();
                let ink = format!(
                    "#{:02x}{:02x}{:02x}",
                    (fg.red() * 255.0) as u8,
                    (fg.green() * 255.0) as u8,
                    (fg.blue() * 255.0) as u8
                );
                match kind {
                    Kind::Line => {
                        let colour = Paint::preset(drawings::Preset::Blue).hex(&theme);
                        let (a, b) = ((4.0, h - 5.0), (w - 4.0, 5.0));
                        colors::set_source(cr, &colour);
                        cr.set_line_width(2.0);
                        cr.set_line_cap(gtk::cairo::LineCap::Round);
                        cr.move_to(a.0, a.1);
                        cr.line_to(b.0, b.1);
                        let _ = cr.stroke();
                        // The grips, which are what say "a drawing" rather
                        // than "a slash".
                        for (x, y) in [a, b] {
                            colors::set_source(cr, &theme.ui.background);
                            cr.rectangle(x - 3.5, y - 3.5, 7.0, 7.0);
                            let _ = cr.fill();
                            colors::set_source(cr, &ink);
                            cr.rectangle(x - 2.5, y - 2.5, 5.0, 5.0);
                            let _ = cr.fill();
                        }
                    }
                    Kind::Rect => {
                        let colour = Paint::preset(drawings::Preset::Amber).hex(&theme);
                        // A candle under the box, so the box reads as a box
                        // over price.
                        let bars = omacharts_engine::theme::theme_bars(&theme);
                        colors::set_source(cr, &bars.up);
                        cr.rectangle((w / 2.0).round() + 0.5, 2.0, 1.0, h - 4.0);
                        let _ = cr.fill();
                        cr.rectangle((w / 2.0).round() - 2.0, h * 0.3, 5.0, h * 0.4);
                        let _ = cr.fill();
                        colors::set_source_alpha(cr, &colour, 0.3);
                        cr.rectangle(3.0, 5.0, w - 6.0, h - 10.0);
                        let _ = cr.fill();
                        colors::set_source(cr, &colour);
                        cr.set_line_width(1.0);
                        cr.rectangle(3.5, 5.5, w - 7.0, h - 11.0);
                        let _ = cr.stroke();
                        for (x, y) in [(3.5, 5.5), (w - 3.5, h - 5.5)] {
                            colors::set_source(cr, &ink);
                            cr.rectangle(x - 2.0, y - 2.0, 4.0, 4.0);
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
