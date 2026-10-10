//! The drawing tools, on a bar that slides in from the left.
//!
//! The rail of watchlists lives on the right; the tools live on the left, the
//! same way: a key, or a handle in the bottom-left corner, brings them in,
//! and they take no room when they are out. The handle is the pen glyph,
//! faint until the pointer is near or the bar is out, with a tooltip that
//! says what it is and which key does the same. It sits at the left end of
//! the chartbook strip when there is one, and floats in the corner the
//! strip would occupy when there is not. Nothing in the window's top
//! corner, which belongs to the charts.
//!
//! One column of buttons, which is the shape a handful of tools wants and
//! stays the shape as they are added. Each button is a sign the app drew
//! itself — a pointer, a line, a box, the same box as a curve, a letter —
//! each the bare shape, in the palette's own ink and nothing else, dim
//! until it is hovered or lit, rather than a stock glyph that would look
//! like every other application's or a preview that would compete with the
//! chart.
//!
//! The first button is no tool at all: the pointer, which selects and moves
//! what is drawn, and is what Escape goes back to. One button is lit at a
//! time, the way radio buttons are, and clicking the lit one leaves it lit:
//! the way to put a tool down is to pick another, or the pointer. A
//! right-click on a tool opens that kind's configurations.

use std::cell::RefCell;
use std::rc::Rc;

use gtk::prelude::*;
use gtk::{gio, glib};
use omacharts_engine::drawings::Kind;

/// The width of a tool button, which is also the bar's.
const TOOL: i32 = 36;

/// Which band of the bar a tool belongs to.
///
/// Read off the kind rather than written out as a list, so a kind added to
/// the engine lands in the right band without anybody remembering to come
/// back here.
fn tool_group(kind: Option<Kind>) -> u8 {
    match kind {
        None => 0,
        Some(k) if k.is_line() => 1,
        Some(k) if k.is_text() => 3,
        Some(_) => 2,
    }
}

/// A rounded rectangle as a path, corner radius `r`.
fn rounded(cr: &gtk::cairo::Context, x: f64, y: f64, w: f64, h: f64, r: f64) {
    use std::f64::consts::{FRAC_PI_2, PI};
    cr.new_sub_path();
    cr.arc(x + w - r, y + r, r, -FRAC_PI_2, 0.0);
    cr.arc(x + w - r, y + h - r, r, 0.0, FRAC_PI_2);
    cr.arc(x + r, y + h - r, r, FRAC_PI_2, PI);
    cr.arc(x + r, y + r, r, PI, 3.0 * FRAC_PI_2);
    cr.close_path();
}

/// The tab on the edge: its width out from the edge, and its height.
const HANDLE_W: i32 = 22;
const HANDLE_H: i32 = 26;

pub struct DrawingBar {
    /// What goes in the layout: the revealer, so the bar can slide.
    pub root: gtk::Revealer,
    /// The handles made for this bar, drawn again when it opens or shuts.
    handles: RefCell<Vec<gtk::DrawingArea>>,
    on_toggle: Rc<dyn Fn()>,
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
    /// `on_configure` which kind's configurations to open; `on_toggle` that
    /// the handle was clicked.
    pub fn new(
        on_arm: impl Fn(Option<Kind>) + Clone + 'static,
        on_configure: impl Fn(Kind) + Clone + 'static,
        on_toggle: impl Fn() + 'static,
    ) -> Rc<DrawingBar> {
        let column = gtk::Box::new(gtk::Orientation::Vertical, 4);
        column.add_css_class("drawing-bar");
        column.set_valign(gtk::Align::Start);
        column.set_margin_top(10);
        column.set_margin_start(6);
        column.set_margin_end(2);

        let mut buttons: Vec<(Option<Kind>, gtk::ToggleButton, gtk::DrawingArea)> = Vec::new();
        let mut badges = Vec::new();
        let mut group_so_far: Option<u8> = None;
        for kind in std::iter::once(None).chain(Kind::ALL.into_iter().map(Some)) {
            // A rule between the groups: the pointer, which selects rather
            // than draws; the strokes; the shapes; the words. A
            // `GtkSeparator` rather than a line drawn here, because that is
            // what a toolbar's dividers are made of everywhere else and it
            // takes the theme's own colour for the job.
            let group = tool_group(kind);
            if group_so_far.is_some_and(|last| last != group) {
                let rule = gtk::Separator::new(gtk::Orientation::Horizontal);
                rule.add_css_class("drawing-tool-rule");
                column.append(&rule);
            }
            group_so_far = Some(group);
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
                Some(Kind::Horizontal) => {
                    "Horizontal line (Alt+H): a level, held flat however you drag it"
                }
                Some(Kind::Arrow) => "Arrow (Alt+A): a line that points where it ends",
                Some(Kind::Rect) => "Rectangle (Alt+R): press at one corner, release at the other",
                Some(Kind::Ellipse) => {
                    "Circle (Alt+C): press and drag to any shape, round or wide"
                }
                Some(Kind::Text) => "Text (Alt+T): click where the words go, then type",
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
            handles: RefCell::new(Vec::new()),
            on_toggle: Rc::new(on_toggle),
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
        for handle in self.handles.borrow().iter() {
            if shown {
                handle.add_css_class("open");
            } else {
                handle.remove_css_class("open");
            }
        }
    }

    pub fn is_shown(&self) -> bool {
        self.root.reveals_child()
    }

    /// A handle that opens and shuts this bar: a small tab stuck to the
    /// window's left edge, rounded on the side that faces in, with the
    /// left-panel glyph on it — the mirror of the rail's own toggle on
    /// the right. Dim until the pointer is near or the bar is out. Made as
    /// often as the window has a corner to put one in.
    pub fn handle(&self) -> gtk::DrawingArea {
        let handle = gtk::DrawingArea::new();
        handle.set_size_request(HANDLE_W, HANDLE_H);
        handle.set_halign(gtk::Align::Start);
        handle.set_valign(gtk::Align::End);
        handle.add_css_class("drawing-handle");
        if self.is_shown() {
            handle.add_css_class("open");
        }
        handle.set_cursor_from_name(Some("pointer"));
        handle.set_tooltip_text(Some(&crate::ui::shortcuts::tooltip("Drawing tools", "win.drawing-tools")));
        let on_toggle = self.on_toggle.clone();
        let click = gtk::GestureClick::new();
        click.connect_released(move |_, _, _, _| on_toggle());
        handle.add_controller(click);
        handle.set_draw_func(|area, cr, w, h| {
            // Everything in the handle's own colour, so the stylesheet
            // decides how loud it is: the tab is that colour at a whisper,
            // its edge a little more, the glyph at full strength.
            let (w, h) = (w as f64, h as f64);
            let fg = area.color();
            let (r, g, b, a) = (fg.red() as f64, fg.green() as f64, fg.blue() as f64, fg.alpha() as f64);
            // The tab: flat against the left edge, rounded on the right.
            let radius = 7.0;
            cr.new_sub_path();
            cr.move_to(0.0, 0.5);
            cr.line_to(w - radius - 0.5, 0.5);
            cr.arc(w - radius - 0.5, radius + 0.5, radius, -std::f64::consts::FRAC_PI_2, 0.0);
            cr.line_to(w - 0.5, h - radius - 0.5);
            cr.arc(w - radius - 0.5, h - radius - 0.5, radius, 0.0, std::f64::consts::FRAC_PI_2);
            cr.line_to(0.0, h - 0.5);
            cr.close_path();
            // Louder when it floats over the chart, where a whisper of
            // the ink blends into whatever indicator sits beneath it.
            let floating = area.has_css_class("floating");
            cr.set_source_rgba(r, g, b, a * if floating { 0.14 } else { 0.10 });
            let _ = cr.fill_preserve();
            cr.set_source_rgba(r, g, b, a * if floating { 0.5 } else { 0.28 });
            cr.set_line_width(1.0);
            let _ = cr.stroke();
            // The glyph: a panel with its left third filled, the mirror of
            // the rail's toggle on the other side of the window.
            let (gw, gh) = (13.0, 10.0);
            let (gx, gy) = (((w - gw) / 2.0).round() + 0.5, ((h - gh) / 2.0).round() + 0.5);
            cr.set_source_rgba(r, g, b, a);
            cr.set_line_width(1.2);
            cr.set_line_join(gtk::cairo::LineJoin::Round);
            rounded(cr, gx, gy, gw, gh, 2.0);
            let _ = cr.stroke();
            cr.rectangle(gx + 0.5, gy + 0.5, 4.0, gh - 1.0);
            let _ = cr.fill();
        });
        self.handles.borrow_mut().push(handle.clone());
        handle
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
                        let (a, b) = ((4.5, h - 4.5), (w - 4.5, 4.5));
                        cr.move_to(a.0, a.1);
                        cr.line_to(b.0, b.1);
                        let _ = cr.stroke();
                    }
                    // Flat, which is the whole of what it is: the sign is
                    // the one thing in the column that cannot be mistaken
                    // for the line above it.
                    Some(Kind::Horizontal) => {
                        let y = (h / 2.0).round() + 0.5;
                        cr.move_to(4.5, y);
                        cr.line_to(w - 4.5, y);
                        let _ = cr.stroke();
                    }
                    // The line, with the open head it is drawn with.
                    Some(Kind::Arrow) => {
                        let (a, b) = ((4.5, h - 4.5), (w - 5.0, 5.0));
                        cr.move_to(a.0, a.1);
                        cr.line_to(b.0, b.1);
                        let _ = cr.stroke();
                        let angle = (b.1 - a.1).atan2(b.0 - a.0);
                        let (spread, length) = (0.42, 6.5);
                        for side in [-1.0, 1.0] {
                            let away = angle + std::f64::consts::PI + side * spread;
                            cr.move_to(b.0, b.1);
                            cr.line_to(b.0 + length * away.cos(), b.1 + length * away.sin());
                        }
                        let _ = cr.stroke();
                    }
                    Some(Kind::Rect) => {
                        // A box, faintly filled the way the drawn one is.
                        let (x, y, rw, rh) = (4.5, 5.5, w - 9.0, h - 11.0);
                        cr.rectangle(x, y, rw, rh);
                        cr.set_source_rgba(r, g, b, a * 0.18);
                        let _ = cr.fill_preserve();
                        cr.set_source_rgba(r, g, b, a);
                        cr.set_line_width(1.0);
                        let _ = cr.stroke();
                    }
                    Some(Kind::Ellipse) => {
                        // The box's sign with the outline swapped for the
                        // curve. Drawn a little wider than tall, because the
                        // tool is not held to a circle and the sign should
                        // not promise one.
                        let (x, y, rw, rh) = (4.5, 5.5, w - 9.0, h - 11.0);
                        let curve = |cr: &gtk::cairo::Context| {
                            cr.save().ok();
                            cr.translate(x + rw / 2.0, y + rh / 2.0);
                            cr.scale(rw / 2.0, rh / 2.0);
                            cr.arc(0.0, 0.0, 1.0, 0.0, std::f64::consts::TAU);
                            cr.restore().ok();
                        };
                        curve(cr);
                        cr.set_source_rgba(r, g, b, a * 0.18);
                        let _ = cr.fill();
                        cr.set_source_rgba(r, g, b, a);
                        cr.set_line_width(1.0);
                        curve(cr);
                        let _ = cr.stroke();
                    }
                    Some(Kind::Text) => {
                        // A capital I with its serifs: the mark every
                        // application puts on its text tool, and the one
                        // thing in this column that is a letter rather than
                        // a shape — which is the point, since the tool makes
                        // letters.
                        let (cx, top, bottom) = ((w / 2.0).round() + 0.5, 5.5, h - 5.5);
                        let arm = 4.0;
                        cr.set_line_width(1.5);
                        cr.move_to(cx, top);
                        cr.line_to(cx, bottom);
                        cr.move_to(cx - arm, top);
                        cr.line_to(cx + arm, top);
                        cr.move_to(cx - arm, bottom);
                        cr.line_to(cx + arm, bottom);
                        let _ = cr.stroke();
                    }
                }
            });
            icon.queue_draw();
        }
    }
}
