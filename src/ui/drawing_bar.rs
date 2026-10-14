//! The drawing tools, on a bar down the left edge.
//!
//! The rail of watchlists lives on the right; the tools live on the left, the
//! same way: a key, or a toggle in the window's top-left corner, brings them
//! in, and they take no room when they are out. The toggle is the mirror of
//! the rail's in the top-right — the same button, the left-panel glyph, at
//! the same height — so the two edges of the window open the same way. It
//! stands over the bar while the bar is out, and over the top-left chart
//! when it is not.
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

use std::rc::Rc;

use gtk::prelude::*;
use gtk::{gio, glib};
use omacharts_engine::drawings::Kind;

/// The width of a tool button, which is also the bar's.
const TOOL: i32 = 36;
/// The bar's width inside its padding: one tool, and the column's four
/// pixels either side.
pub const WIDTH: i32 = TOOL + 8;

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

pub struct DrawingBar {
    /// What goes in the layout: the bar with its ground, shown and hidden.
    pub root: gtk::Box,
    /// The button that opens and shuts this bar, lit while it is out. The
    /// window puts it in its top-left corner.
    pub toggle: gtk::ToggleButton,
    /// The pointer first, as `None`, then a button a tool.
    buttons: Vec<(Option<Kind>, gtk::ToggleButton, gtk::DrawingArea)>,
    /// The configuration number worn by the tool in hand, so Alt+R, Alt+3
    /// shows a 3 on the rectangle before anything is drawn. One per button,
    /// the pointer's never shown.
    badges: Vec<gtk::Label>,
    /// Set while the buttons are being shown a state, so their own signal
    /// does not read as a click.
    showing: std::cell::Cell<bool>,
    /// Whether Ctrl was down on the press that is about to become a toggle.
    ///
    /// A toggle says which button was lit and nothing about the keyboard,
    /// so the press is watched on the way down and what it saw is read a
    /// moment later by the toggle it causes.
    sticky_press: std::cell::Cell<bool>,
}

impl DrawingBar {
    /// `on_arm` is told which tool was picked — `None` for the pointer —
    /// and whether it should stay in hand after each drawing, which is what
    /// Ctrl on the click asks for;
    /// `on_configure` which kind's configurations to open; `on_toggle` that
    /// the toggle was pressed.
    pub fn new(
        on_arm: impl Fn(Option<Kind>, bool) + Clone + 'static,
        on_configure: impl Fn(Kind) + Clone + 'static,
        on_toggle: impl Fn() + 'static,
    ) -> Rc<DrawingBar> {
        let column = gtk::Box::new(gtk::Orientation::Vertical, 4);
        column.add_css_class("drawing-bar");
        column.set_valign(gtk::Align::Start);
        // Under the window's top-left corner, which holds this bar's toggle
        // while the bar is out: the same step the rail's header takes down
        // out from under the corner on the right, and eight pixels more.
        column.set_margin_top(45);
        // The same either side. It was six and two, which put the column a
        // visible two pixels left of centre in its own bar — the kind of
        // thing you cannot name but can see.
        column.set_margin_start(4);
        column.set_margin_end(4);

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
            // Every tool says the one thing that is true of all of them, at
            // the end of what is true of it alone.
            let held = match kind {
                None => "",
                Some(_) => "\nCtrl+click to keep it in hand",
            };
            let about = match kind {
                None => "Pointer (Esc): select, move and resize drawings",
                Some(Kind::Line) => "Line (Alt+L): click where it starts, then where it ends",
                Some(Kind::Horizontal) => {
                    "Horizontal line (Alt+H): a level, held flat however you drag it"
                }
                Some(Kind::Arrow) => "Arrow (Alt+A): a line that points where it ends",
                Some(Kind::Zigzag) => {
                    "Zig-zag (Alt+Z): click each corner; double-click or Enter ends it, Esc drops it"
                }
                Some(Kind::Rect) => "Rectangle (Alt+R): press at one corner, release at the other",
                Some(Kind::Ellipse) => {
                    "Circle (Alt+C): press and drag to any shape, round or wide"
                }
                Some(Kind::Text) => "Text (Alt+T): click where the words go, then type",
            };
            button.set_tooltip_text(Some(&format!("{about}{held}")));
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
            root: gtk::Box::new(gtk::Orientation::Vertical, 0),
            toggle: gtk::ToggleButton::new(),
            buttons,
            badges,
            showing: std::cell::Cell::new(false),
            sticky_press: std::cell::Cell::new(false),
        });
        // There at once, the way the rail is. The rail cannot slide without
        // every chart redrawing at every step, and two edges that open
        // differently read as two apps.
        bar.root.add_css_class("drawing-tools");
        bar.root.append(&column);
        bar.root.set_visible(false);

        bar.toggle.set_icon_name("sidebar-show-symbolic");
        bar.toggle.set_tooltip_text(Some(&crate::ui::shortcuts::tooltip("Drawing tools", "win.drawing-tools")));
        bar.toggle.add_css_class("flat");
        // `set_shown` lights the button too, which toggles it again; only a
        // press that disagrees with the bar is somebody asking for a change.
        let root = bar.root.clone();
        bar.toggle.connect_toggled(move |toggle| {
            if toggle.is_active() != root.get_visible() {
                on_toggle();
            }
        });

        for (kind, button, _) in &bar.buttons {
            let kind = *kind;
            let on_arm = on_arm.clone();
            let weak = Rc::downgrade(&bar);
            {
                // On the way down, before the toggle it causes.
                let press = gtk::GestureClick::new();
                press.set_propagation_phase(gtk::PropagationPhase::Capture);
                let weak = Rc::downgrade(&bar);
                press.connect_pressed(move |gesture, _, _, _| {
                    let Some(bar) = weak.upgrade() else { return };
                    let ctrl = gesture
                        .current_event_state()
                        .contains(gtk::gdk::ModifierType::CONTROL_MASK);
                    bar.sticky_press.set(ctrl);
                });
                button.add_controller(press);
            }
            button.connect_toggled(move |button| {
                let Some(bar) = weak.upgrade() else { return };
                // The group lights one and dims the rest; only the one
                // lit is a choice.
                if bar.showing.get() || !button.is_active() {
                    return;
                }
                on_arm(kind, bar.sticky_press.take());
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
        bar.show_armed(None, 1, false);
        bar
    }

    /// Light the button of the tool in hand — the pointer, when there is
    /// none — with the configuration it will draw in on it, when that is
    /// not the first: the usual one needs no saying, and a badge that is
    /// always there is one nobody reads.
    pub fn show_armed(&self, armed: Option<Kind>, config: u8, sticky: bool) {
        self.showing.set(true);
        for ((kind, button, _), badge) in self.buttons.iter().zip(&self.badges) {
            let lit = armed == *kind;
            button.set_active(lit);
            badge.set_text(&format!("{config}"));
            badge.set_visible(lit && kind.is_some() && config != 1);
            // A tool that stays in hand says so, or the chart simply goes
            // on drawing and nobody knows why.
            match lit && sticky {
                true => button.add_css_class("drawing-tool-stuck"),
                false => button.remove_css_class("drawing-tool-stuck"),
            }
        }
        self.showing.set(false);
    }

    pub fn set_shown(&self, shown: bool) {
        self.root.set_visible(shown);
        self.toggle.set_active(shown);
    }

    pub fn is_shown(&self) -> bool {
        self.root.get_visible()
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
                    // Three legs climbing, and a head on the last. Four
                    // made an M — too many turns at this size to read as
                    // anything but a letter — and swinging about one level
                    // made it a symbol rather than a picture of what the
                    // tool draws. A run of swings on a chart goes
                    // somewhere; this one goes up, and says so.
                    Some(Kind::Zigzag) => {
                        let (x0, x1) = (4.5, w - 5.5);
                        let step = (x1 - x0) / 3.0;
                        // Up, back a quarter of the way, up the same again:
                        // the two rises are equal and the pull-back sits
                        // between them, so the shape is the same read from
                        // either end. The first try had three legs of three
                        // different lengths and leaned.
                        let (lo, hi) = (h - 5.0, 5.0);
                        let dip = (lo - hi) * 0.25;
                        let rise = (lo - hi + dip) / 2.0;
                        let corners = [
                            (x0, lo),
                            (x0 + step, lo - rise),
                            (x0 + step * 2.0, lo - rise + dip),
                            (x1, hi),
                        ];
                        cr.move_to(corners[0].0, corners[0].1);
                        for (x, y) in &corners[1..] {
                            cr.line_to(*x, *y);
                        }
                        let _ = cr.stroke();
                        let (tail, tip) = (corners[2], corners[3]);
                        let angle = (tip.1 - tail.1).atan2(tip.0 - tail.0);
                        for side in [-1.0, 1.0] {
                            let away = angle + std::f64::consts::PI + side * 0.45;
                            cr.move_to(tip.0, tip.1);
                            cr.line_to(tip.0 + 6.0 * away.cos(), tip.1 + 6.0 * away.sin());
                        }
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
