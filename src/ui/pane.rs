//! One chart in the layout.
//!
//! The window used to be a chart with a legend painted over it. With several
//! charts side by side, everything that was "the chart's" moves in here: which
//! symbol it shows, at what resolution, with which indicators, in which
//! session, drawn how. A pane is the unit you focus, split, and close.
//!
//! What stays with the window is what is genuinely one per window — the
//! watchlist, the theme, the symbol index — and the question of which pane is
//! focused.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use gtk::prelude::*;
use omacharts_engine::{BarStyle, Indicator, Instrument, LinkGroup, Session, Theme, Timeframe};

use crate::ui::chart::ChartView;
use crate::ui::shortcuts;

/// How a pane follows the others.
///
/// Panes in the same group share one symbol: a watchlist in that group drives
/// them, and a symbol picked on any of them moves the rest. An unlinked pane
/// is parked — the reason to split a chart in the first place is usually to
/// leave one of them showing something while you go looking at something
/// else.
pub const LINK_OFF: &str = "Not linked — this chart stays where it is";

/// One chart filling the window, and the way back.
pub const MAXIMIZE: &str = "Maximize";
pub const RESTORE: &str = "Restore";

pub struct ChartPane {
    pub id: u32,
    pub view: Rc<ChartView>,
    /// What goes in the layout: the chart, its legend, and the focus ring.
    pub root: gtk::Box,
    /// The symbol, over the top left of the chart. A button: clicking the
    /// name of the thing you are looking at to change it is the shortest
    /// route there is.
    pub symbol_button: gtk::Button,
    /// This chart's resolutions, since the resolution is this chart's. One
    /// strip in the header could only ever describe one of them.
    ///
    /// Shown on the focused chart only: four strips on screen is the same row
    /// of buttons four times, and only one of them is the one you are about to
    /// press. The others say their resolution in a word instead.
    pub strip: gtk::Box,
    pub buttons: RefCell<Vec<(Timeframe, gtk::ToggleButton)>>,
    /// The resolution beside the symbol, which opens every resolution: the
    /// one way to change it that fits on any chart, however narrow.
    pub timeframe_menu: gtk::MenuButton,
    timeframe_label: gtk::Label,
    /// One row per indicator, under the readout.
    pub indicator_legend: gtk::Box,
    pub gear: gtk::Button,
    pub link: gtk::MenuButton,
    /// Fills the window with this chart, and puts it back. In the top right
    /// corner, out from under the legend, and only there while the pointer is
    /// on the chart: with four charts open, four of these drawn all the time
    /// is four more things between you and the prices.
    pub expand: gtk::Button,
    expand_icon: gtk::DrawingArea,
    /// Which way the expand glyph points, and which word its tooltip uses.
    expand_state: Rc<Cell<bool>>,
    pub instrument: RefCell<Option<Instrument>>,
    /// The symbol and venue this chart was asked for and the index could not
    /// name yet.
    ///
    /// A chart restored from a saved arrangement can name a listing the
    /// curated half of the inventory does not carry, and the long tail only
    /// arrives once the window is up. Remembering what was asked for is what
    /// lets the chart be written back down under that symbol rather than under
    /// nothing, and filled in once the catalogue lands. It is cleared the
    /// moment an instrument is set, so a chart the user has since changed is
    /// no longer waiting for anything.
    pub pending: RefCell<Option<(String, Option<String>)>>,
    pub timeframe: Cell<Timeframe>,
    pub indicators: RefCell<Vec<Indicator>>,
    pub bar_style: Cell<BarStyle>,
    pub session: Cell<Session>,
    pub show_grid: Cell<bool>,
    pub linked: Cell<LinkGroup>,
    /// The group and its colour, so it can be read off four charts at a
    /// glance rather than by opening four popovers. Held here because the
    /// drawing happens on every frame and the theme it comes from does not.
    link_mark: Rc<RefCell<LinkMark>>,
}

impl ChartPane {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: u32,
        theme: Theme,
        scheme: omacharts_engine::theme::BarScheme,
        timeframe: Timeframe,
        indicators: Vec<Indicator>,
        bar_style: BarStyle,
        session: Session,
        show_grid: bool,
        linked: LinkGroup,
    ) -> Rc<ChartPane> {
        let view = ChartView::new(theme, scheme);

        let symbol_button = gtk::Button::new();
        symbol_button.add_css_class("flat");
        symbol_button.add_css_class("legend-symbol");
        // The same weight the plain label had: it is still the title of the
        // chart, it just happens to be clickable.
        symbol_button.add_css_class("readout-symbol");
        symbol_button.set_valign(gtk::Align::Center);
        symbol_button.set_tooltip_text(Some(&shortcuts::tooltip("Find a symbol", "win.find")));

        let strip = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        strip.add_css_class("linked");
        strip.add_css_class("timeframe-strip");
        strip.set_valign(gtk::Align::Center);
        strip.set_visible(false);

        let timeframe_menu = gtk::MenuButton::new();
        timeframe_menu.add_css_class("flat");
        timeframe_menu.add_css_class("readout-symbol");
        timeframe_menu.set_valign(gtk::Align::Center);
        timeframe_menu.set_tooltip_text(Some("Resolution: click for the list"));
        // No chevron: a screenshot keeps this label because it is information,
        // and a chevron would put a control into every picture of a chart. The
        // flat button's hover is affordance enough, as it is for the symbol.
        // A label of our own rather than the button's, because a MenuButton
        // draws the arrow after a plain label whatever it is told, and only
        // listens when given a child.
        let timeframe_label = gtk::Label::new(None);
        timeframe_menu.set_child(Some(&timeframe_label));
        timeframe_menu.set_always_show_arrow(false);

        // Drawn rather than named. Adwaita's "insert-link" is a chain with a
        // downward arrow under it — it means *insert* a link, and the arrow
        // read as a dropdown nobody could open. There is no plain chain in the
        // theme, so here is one: two capsules and the bar that joins them,
        // in the legend's own colour while unlinked. Once linked it gives way
        // to the group's number on a key.
        let link_mark: Rc<RefCell<LinkMark>> = Rc::default();
        let link = gtk::MenuButton::new();
        link.set_child(Some(&link_icon(link_mark.clone())));
        link.add_css_class("flat");
        link.add_css_class("legend-link");
        link.set_valign(gtk::Align::Center);
        // A MenuButton draws a chevron beside whatever it is given unless it
        // is told not to, and a chain with an arrow under it was the exact
        // thing the drawn icon exists to avoid.
        link.set_always_show_arrow(false);
        set_link_look(&link, linked);

        let gear = gtk::Button::from_icon_name("emblem-system-symbolic");
        gear.add_css_class("flat");
        gear.add_css_class("legend-gear");
        gear.set_tooltip_text(Some(&shortcuts::tooltip("Chart settings", "chart.settings")));
        gear.set_valign(gtk::Align::Center);

        // The link sits with the symbol, because that is what it is about:
        // whether this chart follows the rail's symbol or keeps its own.
        let bar = gtk::Box::new(gtk::Orientation::Horizontal, 2);
        bar.append(&symbol_button);
        bar.append(&link);
        bar.append(&timeframe_menu);
        bar.append(&gear);

        let indicator_legend = gtk::Box::new(gtk::Orientation::Vertical, 0);
        indicator_legend.set_halign(gtk::Align::Start);

        let legend = gtk::Box::new(gtk::Orientation::Vertical, 0);
        legend.set_halign(gtk::Align::Start);
        legend.set_valign(gtk::Align::Start);
        legend.set_margin_start(12);
        legend.set_margin_top(6);
        legend.append(&bar);
        legend.append(&indicator_legend);

        // The strip sits across the top of its own chart rather than in the
        // legend: centred it reads as this chart's own toolbar, where in the
        // corner it was one more thing in a stack of names.
        let top = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        top.set_halign(gtk::Align::Center);
        top.set_valign(gtk::Align::Start);
        top.set_margin_top(6);
        top.append(&strip);

        // The opposite corner from the legend, and the only thing in it, so
        // that a chart which has grown to fill the window still shows the way
        // back without anything else being in the way.
        let expand_state = Rc::new(Cell::new(false));
        let expand_icon = expand_icon(expand_state.clone());
        let expand = gtk::Button::new();
        expand.set_child(Some(&expand_icon));
        expand.add_css_class("flat");
        expand.add_css_class("pane-expand");
        expand.set_valign(gtk::Align::Center);
        expand.set_tooltip_text(Some(&shortcuts::tooltip(MAXIMIZE, "chart.maximize")));
        // Nothing to maximize away from until there is a second chart.
        expand.set_visible(false);

        let corner = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        corner.set_halign(gtk::Align::End);
        corner.set_valign(gtk::Align::Start);
        corner.set_margin_top(6);
        corner.set_margin_end(CORNER_MARGIN);
        corner.append(&expand);

        let overlay = gtk::Overlay::new();
        // The chart's own two layers, stacked, rather than the drawing area
        // alone: the crosshair is a widget over the bars now.
        overlay.set_child(Some(&view.root));
        overlay.add_overlay(&legend);
        overlay.add_overlay(&top);
        overlay.add_overlay(&corner);

        // The three are laid over the chart independently, so on a narrow
        // chart the centred strip lands on the symbol. It steps aside rather
        // than overlap, and the resolution beside the symbol offers the same
        // list. Decided where the overlay places it, the one moment every
        // width involved is known.
        let legend_margin = legend.margin_start();
        let (legend_row, strip_row, corner_box) = (bar.clone(), top.clone(), corner.clone());
        overlay.connect_get_child_position(move |overlay, child| {
            if child == strip_row.upcast_ref::<gtk::Widget>() {
                let natural = |w: &gtk::Widget| w.preferred_size().1.width();
                let left = legend_margin + natural(legend_row.upcast_ref());
                let right = natural(corner_box.upcast_ref()) + corner_box.margin_end();
                strip_row.set_child_visible(strip_fits(
                    overlay.width(),
                    left,
                    natural(strip_row.upcast_ref()),
                    right,
                ));
            }
            None
        });

        // A box rather than the overlay itself, so the focus ring is drawn on
        // something that is not also the drawing surface.
        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        root.add_css_class("chart-pane");
        root.set_hexpand(true);
        root.set_vexpand(true);
        root.append(&overlay);

        Rc::new(ChartPane {
            id,
            view,
            root,
            symbol_button,
            strip,
            buttons: RefCell::new(Vec::new()),
            timeframe_menu,
            timeframe_label,
            indicator_legend,
            gear,
            link,
            expand,
            expand_icon,
            expand_state,
            instrument: RefCell::new(None),
            pending: RefCell::new(None),
            timeframe: Cell::new(timeframe),
            indicators: RefCell::new(indicators),
            bar_style: Cell::new(bar_style),
            session: Cell::new(session),
            show_grid: Cell::new(show_grid),
            linked: Cell::new(linked),
            link_mark,
        })
    }

    /// Point this chart's strip, and the resolution beside its symbol, at the
    /// resolution it is showing.
    pub fn sync_strip(&self) {
        let current = self.timeframe.get();
        // Keep the keyboard with the resolution it is choosing. Clicking the
        // strip leaves the focus ring on the button clicked, and stepping with
        // the keys from there moved the pressed state along while the ring
        // stayed put — two answers to "which one is this", a button apart.
        // Only when the strip already has the keyboard: taking it from the
        // chart would send the arrow keys somewhere nobody pointed them.
        let chosen = {
            let buttons = self.buttons.borrow();
            let holds_keyboard = buttons.iter().any(|(_, button)| button.has_focus());
            for (listed, button) in buttons.iter() {
                button.set_active(*listed == current);
            }
            holds_keyboard
                .then(|| buttons.iter().find(|(listed, _)| *listed == current))
                .flatten()
                .map(|(_, button)| button.clone())
        };
        if let Some(button) = chosen {
            button.grab_focus();
        }
        self.write_timeframe();
    }

    /// Mark this one as the pane that keys and menus act on.
    ///
    /// A ring rather than anything louder: with four charts on screen the
    /// focused one has to be obvious at a glance and invisible the moment you
    /// stop looking for it.
    ///
    /// The resolution beside the symbol stays put either way. The strip appears
    /// here and says the same thing, so the label used to stand down for it —
    /// but a legend that drops a line the moment you focus the chart reads
    /// differently depending on which pane you are on, and saying the
    /// resolution twice on one chart is cheaper than that.
    pub fn set_focused(&self, focused: bool) {
        if focused {
            self.root.add_css_class("focused");
        } else {
            self.root.remove_css_class("focused");
        }
        self.strip.set_visible(focused);
    }

    /// Join a group, or leave. The colour is resolved against the live theme
    /// by the window and handed in, because a pane has no theme of its own to
    /// ask once the user has changed it.
    pub fn set_link_group(&self, group: LinkGroup, colour: Option<String>) {
        self.linked.set(group);
        set_link_look(&self.link, group);
        mark_link(&self.link, &self.link_mark, group, colour);
    }

    /// Push the maximize corner in from the right, to leave room for
    /// something else that wants the same corner.
    pub fn set_corner_clearance(&self, clearance: i32) {
        if let Some(corner) = self.expand.parent() {
            corner.set_margin_end(CORNER_MARGIN + clearance);
        }
    }

    /// Offer the maximize corner at all, which is a question of whether
    /// there is anything else on screen to maximize away from.
    pub fn set_expandable(&self, expandable: bool) {
        self.expand.set_visible(expandable);
    }

    /// Point the glyph in or out, and say which it is.
    pub fn set_maximized(&self, maximized: bool) {
        if self.expand_state.replace(maximized) != maximized {
            self.expand_icon.queue_draw();
        }
        let what = if maximized { RESTORE } else { MAXIMIZE };
        self.expand.set_tooltip_text(Some(&shortcuts::tooltip(what, "chart.maximize")));
    }

    pub fn label(&self) -> String {
        match self.instrument.borrow().as_ref() {
            Some(instrument) => format!(
                "{}  ·  {}",
                instrument.display_symbol(),
                self.timeframe.get().label()
            ),
            None => String::new(),
        }
    }

    pub fn write_readout(&self) {
        let symbol = self
            .instrument
            .borrow()
            .as_ref()
            .map(|i| i.display_symbol())
            .unwrap_or_default();
        self.symbol_button.set_label(&symbol);
        self.write_timeframe();
    }

    /// The resolution, beside the symbol.
    ///
    /// Written from the strip as well as from the readout because the
    /// resolution changes on its own, with the symbol doing nothing. Reaching
    /// it only through the readout left the label right by luck — the legend is
    /// rebuilt when the resolution changes, for the sake of a promoted reset
    /// period, and that is what happened to rewrite it. The day that rebuild
    /// becomes conditional the label would go on naming whichever resolution
    /// the chart was on when its symbol last arrived.
    fn write_timeframe(&self) {
        self.timeframe_label.set_text(&self.timeframe.get().label());
    }
}

/// How far the maximize corner sits in from the chart's right edge. The same
/// gap the legend keeps on the left, so the two corners are a pair.
const CORNER_MARGIN: i32 = 12;

/// Whether a strip `strip` wide, centred in `width`, clears what sits at
/// either edge — `left` and `right` of it — with a little air on each side.
fn strip_fits(width: i32, left: i32, strip: i32, right: i32) -> bool {
    const GAP: i32 = 8;
    let start = (width - strip) / 2;
    start >= left + GAP && start + strip <= width - right - GAP
}

/// Two arrows on a diagonal, pointing away from each other — and, once the
/// chart has the window to itself, back towards each other.
///
/// The same glyph every tiling window manager and every video player uses for
/// this, which is the point: it has to be readable at eighteen pixels without
/// a label, and the shape people already know is the one that is. Drawn rather
/// than named for the same reason the chain is: Adwaita's nearest icons are
/// "view-fullscreen", which is a frame with corner brackets and reads as a
/// crop tool at this size, and "zoom-fit-best", which reads as a magnifier.
fn expand_icon(maximized: Rc<Cell<bool>>) -> gtk::DrawingArea {
    let area = gtk::DrawingArea::new();
    area.set_content_width(18);
    area.set_content_height(18);
    area.set_draw_func(move |area, cr, width, height| {
        let colour = area.color();
        cr.set_source_rgba(
            colour.red() as f64,
            colour.green() as f64,
            colour.blue() as f64,
            colour.alpha() as f64,
        );
        let (w, h) = (width as f64, height as f64);
        cr.set_line_width(1.3);
        cr.set_line_cap(gtk::cairo::LineCap::Round);
        cr.set_line_join(gtk::cairo::LineJoin::Round);

        let (cx, cy) = (w / 2.0, h / 2.0);

        // An arrowhead on a diagonal: the two legs are the pointing direction
        // turned a quarter either way, which on a diagonal lands them square
        // on the axes and keeps them crisp at this size.
        let head_at = |tx: f64, ty: f64, dx: f64, dy: f64, head: f64| {
            cr.move_to(tx - dx * head, ty);
            cr.line_to(tx, ty);
            cr.line_to(tx, ty - dy * head);
        };

        if maximized.get() {
            // Coming back in: two arrows from the corners towards the middle,
            // stopping short of each other so the pair reads as two arrows
            // and not as one line with a knot in it. They start nearer the
            // corners than the other glyph's do, because an arrow needs a
            // shaft behind its head to be an arrow at eighteen pixels.
            let head = w * 0.22;
            let (near, far) = (w * 0.1, w - w * 0.1);
            let gap = w * 0.11;
            cr.move_to(far, near);
            cr.line_to(cx + gap, cy - gap);
            cr.move_to(near, far);
            cr.line_to(cx - gap, cy + gap);
            let _ = cr.stroke();
            head_at(cx + gap, cy - gap, -1.0, 1.0, head);
            head_at(cx - gap, cy + gap, 1.0, -1.0, head);
        } else {
            // Going out: one shaft corner to corner, pointed at both ends.
            let head = w * 0.26;
            let (near, far) = (w * 0.2, w - w * 0.2);
            cr.move_to(near, far);
            cr.line_to(far, near);
            let _ = cr.stroke();
            head_at(far, near, 1.0, -1.0, head);
            head_at(near, far, -1.0, 1.0, head);
        }
        let _ = cr.stroke();
    });
    area
}

/// Which group a link toggle stands for, and the colour it wears in this
/// theme.
#[derive(Default)]
pub(crate) struct LinkMark {
    group: LinkGroup,
    colour: Option<String>,
}

/// Point a link toggle at a group.
pub(crate) fn mark_link(
    link: &gtk::MenuButton,
    mark: &RefCell<LinkMark>,
    group: LinkGroup,
    colour: Option<String>,
) {
    *mark.borrow_mut() = LinkMark { group, colour };
    if let Some(icon) = link.child() {
        icon.queue_draw();
    }
}

/// The link toggle: a faint chain while unlinked, and the group's number on a
/// key in its colour once linked.
///
/// The number because it is the group — the colour is only this theme's
/// answer about it, and the first group has none on purpose. A tinted chain
/// was all this used to be, and across four charts nobody could tell group 1
/// from unlinked, or which group the watchlist drove, without a popover.
pub(crate) fn link_icon(mark: Rc<RefCell<LinkMark>>) -> gtk::DrawingArea {
    let area = gtk::DrawingArea::new();
    area.set_content_width(19);
    area.set_content_height(18);
    area.set_draw_func(move |area, cr, width, height| {
        let mark = mark.borrow();
        let (w, h) = (width as f64, height as f64);
        let (Some(number), Some(colour)) = (mark.group.number(), mark.colour.as_deref()) else {
            // Unlinked is furniture, and dims with the rest of the legend
            // rather than insisting on a hue of its own.
            let ink = area.color();
            let (r, g, b, a) = (ink.red(), ink.green(), ink.blue(), ink.alpha());
            cr.set_source_rgba(r as f64, g as f64, b as f64, a as f64);
            draw_chain(cr, w / 2.0, h / 2.0, h);
            return;
        };
        draw_key(area, cr, w, h, colour, &number.to_string());
    });
    area
}

/// A key cap: the group's colour as a wash and a hairline, and its number on
/// top in the colour itself.
///
/// A wash rather than a fill, so the key sits in the legend like the text
/// beside it instead of shouting over the chart, and works on a light theme
/// and a dark one alike — the colour arrives already held legible against
/// the background, which is the only thing a theme can get wrong here.
fn draw_key(
    area: &gtk::DrawingArea,
    cr: &gtk::cairo::Context,
    w: f64,
    h: f64,
    colour: &str,
    label: &str,
) {
    use crate::ui::colors::{set_source, set_source_alpha};
    // The interface's own face, so the number matches the symbol and the
    // resolution beside it rather than whatever cairo calls sans.
    let family = area.pango_context().font_description().and_then(|f| f.family());
    let family = family.as_deref().unwrap_or("sans-serif").to_string();
    let scale = area.scale_factor().max(1);
    let options = cr.font_options().ok();
    let Some(ink) = ink_box(&family, scale, options.as_ref(), label) else { return };

    // In device pixels, because that is where centring is won or lost. The
    // digit's box is measured as drawn rather than predicted from the font's
    // metrics, which hinting and antialiasing both overrule; and a digit an
    // odd number of pixels wide cannot sit in the middle of a key an even
    // number wide, so the key gives up a pixel to match it rather than the
    // digit being drawn on a half pixel and blurred.
    let s = f64::from(scale);
    let mut key_w = (w * s).round() as i32;
    if (key_w - ink.width) % 2 != 0 {
        key_w -= 1;
    }
    // A little shorter than the box, so it sits on the text's height rather
    // than towering over it. Its height is even in device pixels, as every
    // digit's is not, for the same reason.
    let mut key_h = (16.0_f64.min(h) * s).round() as i32;
    if (key_h - ink.height) % 2 != 0 {
        key_h -= 1;
    }
    let key_top = ((h * s).round() as i32 - key_h) / 2;

    // The hairline half a line in from the key's edge, so it covers whole
    // pixels rather than smearing across two.
    let line = 1.0;
    let (kw, kh) = (f64::from(key_w) / s, f64::from(key_h) / s);
    let top = f64::from(key_top) / s;
    rounded(cr, line / 2.0, top + line / 2.0, kw - line, kh - line, 4.0);
    set_source_alpha(cr, colour, 0.16);
    let _ = cr.fill_preserve();
    set_source_alpha(cr, colour, 0.75);
    cr.set_line_width(line);
    let _ = cr.stroke();

    set_source(cr, colour);
    cr.select_font_face(&family, gtk::cairo::FontSlant::Normal, gtk::cairo::FontWeight::Bold);
    cr.set_font_size(KEY_FONT);
    let left = (key_w - ink.width) / 2 - ink.left;
    let baseline = key_top + (key_h - ink.height) / 2 - ink.top;
    cr.move_to(f64::from(left) / s, f64::from(baseline) / s);
    let _ = cr.show_text(label);
}

const KEY_FONT: f64 = 10.5;

/// Where a digit's ink falls, in device pixels, relative to where it is drawn
/// from.
#[derive(Clone, Copy)]
struct Ink {
    left: i32,
    top: i32,
    width: i32,
    height: i32,
}

/// A digit's ink, measured by drawing it once off screen at this scale with
/// these font options, and remembered: there are nine digits and the answer
/// only changes with the font and the screen.
fn ink_box(
    family: &str,
    scale: i32,
    options: Option<&gtk::cairo::FontOptions>,
    label: &str,
) -> Option<Ink> {
    thread_local! {
        static SEEN: RefCell<HashMap<(String, i32, String), Option<Ink>>> =
            RefCell::new(HashMap::new());
    }
    let key = (family.to_string(), scale, label.to_string());
    if let Some(ink) = SEEN.with(|seen| seen.borrow().get(&key).copied()) {
        return ink;
    }
    let ink = measure_ink(family, scale, options, label);
    SEEN.with(|seen| seen.borrow_mut().insert(key, ink));
    ink
}

fn measure_ink(
    family: &str,
    scale: i32,
    options: Option<&gtk::cairo::FontOptions>,
    label: &str,
) -> Option<Ink> {
    const SIDE: i32 = 96;
    const ORIGIN: i32 = 32;
    let mut surface = gtk::cairo::ImageSurface::create(gtk::cairo::Format::A8, SIDE, SIDE).ok()?;
    {
        let cr = gtk::cairo::Context::new(&surface).ok()?;
        if let Some(options) = options {
            cr.set_font_options(options);
        }
        cr.scale(f64::from(scale), f64::from(scale));
        cr.select_font_face(family, gtk::cairo::FontSlant::Normal, gtk::cairo::FontWeight::Bold);
        cr.set_font_size(KEY_FONT);
        let origin = f64::from(ORIGIN) / f64::from(scale);
        cr.move_to(origin, origin);
        cr.show_text(label).ok()?;
    }
    surface.flush();
    let stride = surface.stride() as usize;
    let data = surface.data().ok()?;
    // Half covered or more is ink: the faint fringe of antialiasing is there
    // on both sides alike and only blurs where the edge is.
    let (mut x0, mut y0, mut x1, mut y1) = (SIDE, SIDE, -1, -1);
    for y in 0..SIDE {
        for x in 0..SIDE {
            if data[y as usize * stride + x as usize] >= 128 {
                (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x), y1.max(y));
            }
        }
    }
    (x1 >= x0).then(|| Ink {
        left: x0 - ORIGIN,
        top: y0 - ORIGIN,
        width: x1 - x0 + 1,
        height: y1 - y0 + 1,
    })
}

/// A chain: two rounded links, overlapping, on the diagonal, centred on
/// `(cx, cy)` in a box `size` across.
///
/// Flat and side by side they read as a Venn diagram — two ovals that happen
/// to overlap. On the diagonal the same two shapes read as a chain, which is
/// why every chain icon is drawn that way, and it survives being shrunk to
/// sixteen pixels where the flat version does not. Checked by rendering all
/// three at true size and looking at them.
fn draw_chain(cr: &gtk::cairo::Context, cx: f64, cy: f64, size: f64) {
    let link_w = size * 0.56;
    let link_h = size * 0.36;
    let overlap = link_h * 0.34;
    cr.set_line_width(1.3);

    cr.save().ok();
    cr.translate(cx, cy);
    cr.rotate(-std::f64::consts::FRAC_PI_4);
    let total = link_w * 2.0 - overlap;
    for x in [-total / 2.0, -total / 2.0 + link_w - overlap] {
        rounded(cr, x, -link_h / 2.0, link_w, link_h, link_h / 2.0);
        let _ = cr.stroke();
    }
    cr.restore().ok();
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

/// The legend stays out of the way, so an unlinked chart's toggle is nearly
/// invisible. A linked one is not: following the rail is the state worth being
/// able to see across four charts without looking for it.
fn set_link_look(link: &gtk::MenuButton, group: LinkGroup) {
    let linked = group.is_linked();
    link.set_tooltip_text(Some(&match group {
        LinkGroup::None => LINK_OFF.to_string(),
        other => format!("Linked · {}", other.label()),
    }));
    // Two states that cannot be confused, from one icon: Adwaita has a chain
    // and no broken chain, so the difference has to be carried by weight
    // rather than by a second glyph. A linked chart says so plainly; an
    // unlinked one keeps a faint handle you can find when you want it.
    link.set_opacity(if linked { 1.0 } else { 0.28 });
}

/// How the panes are arranged.
///
/// A binary tree, the way a tiling window manager keeps one: splitting divides
/// the focused pane in two, and closing a pane hands its space back to its
/// sibling by collapsing the split above them. Nothing else moves.
#[derive(Clone, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Node {
    Leaf(u32),
    Split {
        horizontal: bool,
        /// Where the divider sits, as a share of the split's length. Stored
        /// with the shape because it is part of the arrangement: a layout that
        /// comes back with every divider centred is not the layout you left.
        #[serde(default = "half")]
        ratio: f64,
        first: Box<Node>,
        second: Box<Node>,
    },
}

fn half() -> f64 {
    0.5
}

/// How close to an edge a divider may be pushed.
///
/// The same bounds a dragged handle is held to, because a key that can park a
/// divider somewhere the mouse cannot would make the two ways of moving it
/// disagree about where the ends are. Past this a pane has no chart left in
/// it, only its own chrome.
const DIVIDER_MIN: f64 = 0.05;
const DIVIDER_MAX: f64 = 0.95;

/// How much of a split one press of a resize key moves its divider.
///
/// A fiftieth, which is some thirty pixels of a full-width window: large
/// enough to see a single press land, small enough to stop on the ratio you
/// meant. Held down at the usual repeat rate it crosses the whole range in
/// well over a second, so the divider travels rather than jumping to the end.
const RESIZE_STEP: f64 = 0.02;

/// What a resize key asks for: this chart wider, narrower, taller or shorter.
///
/// Named by what happens to the chart rather than by the arrow that was
/// pressed, because the two only agree half the time. Right makes the focused
/// chart wider whichever side of its divider it sits on, so the divider goes
/// right for a chart in the first half and *left* for one in the second — a
/// `Right` variant would be naming the key in one case and the divider in the
/// other.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Resize {
    Left,
    Right,
    Up,
    Down,
}

impl Resize {
    /// Whether it is a left-right divider that has to move.
    fn horizontal(self) -> bool {
        matches!(self, Resize::Left | Resize::Right)
    }

    /// How far the divider goes, and which way.
    fn delta(self) -> f64 {
        match self {
            Resize::Right | Resize::Down => RESIZE_STEP,
            Resize::Left | Resize::Up => -RESIZE_STEP,
        }
    }
}

impl Node {
    pub fn leaf(id: u32) -> Node {
        Node::Leaf(id)
    }

    /// Replace the leaf `id` with a split holding it and `added`.
    pub fn split(&self, id: u32, added: u32, horizontal: bool) -> Node {
        match self {
            Node::Leaf(leaf) if *leaf == id => Node::Split {
                horizontal,
                ratio: 0.5,
                first: Box::new(Node::Leaf(id)),
                second: Box::new(Node::Leaf(added)),
            },
            Node::Leaf(leaf) => Node::Leaf(*leaf),
            Node::Split { horizontal: h, ratio, first, second } => Node::Split {
                horizontal: *h,
                ratio: *ratio,
                first: Box::new(first.split(id, added, horizontal)),
                second: Box::new(second.split(id, added, horizontal)),
            },
        }
    }

    /// Take `id` out, collapsing the split that held it into its sibling.
    /// `None` when `id` was the only leaf, which the caller refuses to do.
    pub fn remove(&self, id: u32) -> Option<Node> {
        match self {
            Node::Leaf(leaf) => (*leaf != id).then_some(Node::Leaf(*leaf)),
            Node::Split { horizontal, ratio, first, second } => {
                match (first.remove(id), second.remove(id)) {
                    (Some(a), Some(b)) => Some(Node::Split {
                        horizontal: *horizontal,
                        ratio: *ratio,
                        first: Box::new(a),
                        second: Box::new(b),
                    }),
                    (Some(only), None) | (None, Some(only)) => Some(only),
                    (None, None) => None,
                }
            }
        }
    }

    /// Move the divider of the split at `path`, where each step says which
    /// half to descend into.
    pub fn with_ratio(&self, path: &[bool], ratio: f64) -> Node {
        match self {
            Node::Leaf(id) => Node::Leaf(*id),
            Node::Split { horizontal, ratio: current, first, second } => {
                let (ratio, first, second) = match path.split_first() {
                    None => (ratio.clamp(DIVIDER_MIN, DIVIDER_MAX), first.clone(), second.clone()),
                    Some((true, rest)) => (
                        *current,
                        Box::new(first.with_ratio(rest, ratio)),
                        second.clone(),
                    ),
                    Some((false, rest)) => (
                        *current,
                        first.clone(),
                        Box::new(second.with_ratio(rest, ratio)),
                    ),
                };
                Node::Split { horizontal: *horizontal, ratio, first, second }
            }
        }
    }

    /// Which halves to descend into to reach the pane `id`, which is also the
    /// path of every divider standing over it.
    fn path_to(&self, id: u32) -> Option<Vec<bool>> {
        match self {
            Node::Leaf(leaf) => (*leaf == id).then(Vec::new),
            Node::Split { first, second, .. } => {
                let (half, mut rest) = match (first.path_to(id), second.path_to(id)) {
                    (Some(rest), _) => (true, rest),
                    (None, Some(rest)) => (false, rest),
                    (None, None) => return None,
                };
                rest.insert(0, half);
                Some(rest)
            }
        }
    }

    /// The divider that resizes the pane `id`: the path to it, and where it
    /// has to go. Feed both back to `with_ratio`, or to the handle on screen.
    ///
    /// The one that moves is the nearest divider *of the matching
    /// orientation*, not the nearest divider. In a two-by-two every pane has
    /// both a row divider and a column divider standing over it and the closer
    /// of the two is the wrong one for two of the four keys — which looks like
    /// the right pane resizing along the wrong axis.
    ///
    /// `None` when there is nothing to move: one chart on its own, or a chart
    /// in a column asked to be wider. That is a key that does nothing, not an
    /// error and not a wrap onto some other divider, because no other divider
    /// is the answer to the question that was asked.
    pub fn resize(&self, id: u32, how: Resize) -> Option<(Vec<bool>, f64)> {
        let path = self.path_to(id)?;
        let mut node = self;
        let mut found: Option<(Vec<bool>, f64, bool)> = None;
        for (depth, half) in path.iter().enumerate() {
            let Node::Split { horizontal, ratio, first, second } = node else { break };
            if *horizontal == how.horizontal() {
                found = Some((path[..depth].to_vec(), *ratio, *half));
            }
            node = if *half { first } else { second };
        }
        let (divider, ratio, _) = found?;
        // The arrow moves the boundary, not the chart. A chart in the second
        // half therefore shrinks when the key points away from it, which is
        // what every tiling window manager does and what the key looks like it
        // should do — growing whichever chart has the focus means the same
        // arrow moves the divider opposite ways depending on which side you
        // happen to be sitting on, and in the bottom-right pane of a four-way
        // split all four keys then feel backwards.
        Some((divider, (ratio + how.delta()).clamp(DIVIDER_MIN, DIVIDER_MAX)))
    }

    /// Point every leaf at its new id, in one pass.
    ///
    /// One pass is the whole point. Renaming one id at a time looks equivalent
    /// and is not: once the ids being handed out overlap the ids already in the
    /// tree, each rename can catch a leaf an earlier rename just wrote, and the
    /// renames chase each other down the tree. 1→2 then 2→3 then 3→4 leaves
    /// three different panes all called 4, and an arrangement whose every
    /// quadrant names one chart is an arrangement that cannot be drawn.
    pub fn relabel(&self, ids: &HashMap<u32, u32>) -> Node {
        match self {
            Node::Leaf(id) => Node::Leaf(ids.get(id).copied().unwrap_or(*id)),
            Node::Split { horizontal, ratio, first, second } => Node::Split {
                horizontal: *horizontal,
                ratio: *ratio,
                first: Box::new(first.relabel(ids)),
                second: Box::new(second.relabel(ids)),
            },
        }
    }

    /// The same arrangement with any leaf that appears twice taken out, the
    /// first of each kept where it is.
    ///
    /// `None` when nothing is left, which cannot happen for a tree that has a
    /// leaf at all. A pane can only be in one place, so a repeated leaf is not
    /// a layout with an odd shape — it is a layout that was written down
    /// wrongly, and mounting it would put one chart's widget into two parents
    /// and silently lose whichever came second.
    pub fn deduped(&self) -> Option<Node> {
        self.pruned(&mut HashSet::new())
    }

    fn pruned(&self, seen: &mut HashSet<u32>) -> Option<Node> {
        match self {
            Node::Leaf(id) => seen.insert(*id).then_some(Node::Leaf(*id)),
            Node::Split { horizontal, ratio, first, second } => {
                match (first.pruned(seen), second.pruned(seen)) {
                    (Some(a), Some(b)) => Some(Node::Split {
                        horizontal: *horizontal,
                        ratio: *ratio,
                        first: Box::new(a),
                        second: Box::new(b),
                    }),
                    (Some(only), None) | (None, Some(only)) => Some(only),
                    (None, None) => None,
                }
            }
        }
    }

    /// Every pane, left to right and top to bottom — which is the order the
    /// keyboard walks them in.
    pub fn leaves(&self) -> Vec<u32> {
        match self {
            Node::Leaf(id) => vec![*id],
            Node::Split { first, second, .. } => {
                let mut out = first.leaves();
                out.extend(second.leaves());
                out
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_strip_shows_only_where_it_clears_both_corners() {
        // A wide chart: room on either side.
        assert!(strip_fits(1200, 160, 300, 40));
        // Narrow: centred, it would start on top of the symbol.
        assert!(!strip_fits(500, 160, 300, 40));
        // Or end under the maximize corner, when that is the wider of the two.
        assert!(!strip_fits(600, 40, 300, 160));
    }

    #[test]
    fn splitting_replaces_the_leaf_with_a_pair() {
        let layout = Node::leaf(1).split(1, 2, true);
        assert_eq!(layout.leaves(), vec![1, 2]);
        assert!(matches!(layout, Node::Split { horizontal: true, .. }));
    }

    #[test]
    fn splitting_only_touches_the_pane_asked_for() {
        let layout = Node::leaf(1).split(1, 2, true).split(2, 3, false);
        assert_eq!(layout.leaves(), vec![1, 2, 3]);
    }

    #[test]
    fn removing_gives_the_space_to_the_sibling() {
        // Two panes side by side: closing one leaves the other alone at the
        // top of the tree, holding everything.
        let layout = Node::leaf(1).split(1, 2, true);
        assert_eq!(layout.remove(2), Some(Node::Leaf(1)));
        assert_eq!(layout.remove(1), Some(Node::Leaf(2)));
    }

    #[test]
    fn removing_collapses_only_the_split_that_held_it() {
        let layout = Node::leaf(1).split(1, 2, true).split(2, 3, false);
        let left = layout.remove(3).unwrap();
        assert_eq!(left.leaves(), vec![1, 2]);
        assert!(matches!(left, Node::Split { horizontal: true, .. }));
    }

    #[test]
    fn the_last_pane_cannot_be_removed() {
        assert_eq!(Node::leaf(1).remove(1), None);
    }

    /// Four panes from two splits of a split, the way tmux does it: every
    /// leaf is splittable, including ones that came from a split.
    #[test]
    fn splitting_a_split_pane_gives_four() {
        let layout = Node::leaf(1)
            .split(1, 2, true)
            .split(1, 3, false)
            .split(2, 4, false);
        assert_eq!(layout.leaves().len(), 4);
        // And the shape is a pair of columns, each divided in two.
        let Node::Split { horizontal, first, second, .. } = &layout else { panic!("{layout:?}") };
        assert!(*horizontal);
        assert!(matches!(**first, Node::Split { horizontal: false, .. }));
        assert!(matches!(**second, Node::Split { horizontal: false, .. }));
    }

    /// Closing one of four leaves three, and only the split that held it
    /// collapses — the other column keeps its own division.
    #[test]
    fn closing_one_of_four_leaves_the_rest_alone() {
        let layout = Node::leaf(1).split(1, 2, true).split(1, 3, false).split(2, 4, false);
        let left = layout.remove(3).unwrap();
        assert_eq!(left.leaves(), vec![1, 2, 4]);
        let Node::Split { first, second, .. } = &left else { panic!("{left:?}") };
        assert_eq!(**first, Node::Leaf(1));
        assert!(matches!(**second, Node::Split { .. }));
    }

    /// The arrangement this bug destroyed: four panes in two columns, restored
    /// into ids that overlap the ones it was saved with.
    ///
    /// Renaming one id at a time turned all four leaves into the same pane —
    /// 1→2 caught the leaf already called 2, which 2→3 then caught again — and
    /// a window cannot draw one chart in four places, so three quarters of it
    /// came back empty.
    #[test]
    fn relabelling_into_ids_that_overlap_the_old_ones_keeps_the_panes_apart() {
        let layout = Node::leaf(1).split(1, 2, true).split(1, 3, false).split(2, 4, false);
        let ids = HashMap::from([(1, 2), (2, 3), (3, 4), (4, 5)]);

        let moved = layout.relabel(&ids);
        assert_eq!(moved.leaves(), vec![2, 4, 3, 5], "every pane keeps its own id");

        // And the shape is the one that was saved: two columns, each divided.
        let Node::Split { horizontal, first, second, .. } = &moved else { panic!("{moved:?}") };
        assert!(*horizontal);
        assert!(matches!(**first, Node::Split { horizontal: false, .. }));
        assert!(matches!(**second, Node::Split { horizontal: false, .. }));
    }

    #[test]
    fn relabelling_leaves_an_id_nobody_renamed_where_it_was() {
        let layout = Node::leaf(1).split(1, 2, true);
        let moved = layout.relabel(&HashMap::from([(2, 9)]));
        assert_eq!(moved.leaves(), vec![1, 9]);
    }

    /// A pane can only be in one place, so a tree naming one twice was written
    /// down wrongly. Mounting it would put a single chart's widget into two
    /// parents, and GTK would refuse the second and leave a hole.
    #[test]
    fn a_pane_named_twice_is_kept_only_where_it_first_appears() {
        let layout = Node::leaf(1).split(1, 2, true).split(1, 3, false).split(2, 4, false);
        let collapsed = layout.relabel(&HashMap::from([(1, 7), (2, 7), (3, 7), (4, 7)]));
        assert_eq!(collapsed.leaves(), vec![7, 7, 7, 7], "the shape this used to produce");
        assert_eq!(collapsed.deduped(), Some(Node::Leaf(7)));
    }

    #[test]
    fn deduping_a_tree_that_names_each_pane_once_changes_nothing() {
        let layout = Node::leaf(1).split(1, 2, true).split(1, 3, false).split(2, 4, false);
        assert_eq!(layout.deduped(), Some(layout.clone()));
    }

    /// A dragged divider is part of the arrangement, and has to survive both
    /// a restart and anything else happening to the tree.
    #[test]
    fn a_dragged_divider_is_remembered() {
        let layout = Node::leaf(1).split(1, 2, true).with_ratio(&[], 0.7);
        let Node::Split { ratio, .. } = &layout else { panic!("{layout:?}") };
        assert!((ratio - 0.7).abs() < 1e-9, "{ratio}");

        // Splitting one half leaves the outer divider where it was.
        let deeper = layout.split(2, 3, false);
        let Node::Split { ratio, .. } = &deeper else { panic!("{deeper:?}") };
        assert!((ratio - 0.7).abs() < 1e-9, "outer divider moved: {ratio}");

        // And so does closing a pane in the other half.
        let closed = deeper.remove(3).unwrap();
        let Node::Split { ratio, .. } = &closed else { panic!("{closed:?}") };
        assert!((ratio - 0.7).abs() < 1e-9, "outer divider moved on close: {ratio}");
    }

    #[test]
    fn an_inner_divider_moves_without_disturbing_the_outer_one() {
        let layout = Node::leaf(1)
            .split(1, 2, true)
            .with_ratio(&[], 0.3)
            .split(2, 3, false)
            .with_ratio(&[false], 0.8);
        let Node::Split { ratio, second, .. } = &layout else { panic!() };
        assert!((ratio - 0.3).abs() < 1e-9, "outer: {ratio}");
        let Node::Split { ratio, .. } = &**second else { panic!() };
        assert!((ratio - 0.8).abs() < 1e-9, "inner: {ratio}");
    }

    #[test]
    fn a_divider_cannot_be_pushed_off_the_end() {
        let layout = Node::leaf(1).split(1, 2, true).with_ratio(&[], 9.0);
        let Node::Split { ratio, .. } = &layout else { panic!() };
        assert!(*ratio <= 0.95 && *ratio >= 0.05, "{ratio}");
    }

    #[test]
    fn leaves_come_back_in_layout_order() {
        let layout = Node::leaf(1).split(1, 2, true).split(1, 3, false).split(2, 4, false);
        assert_eq!(layout.leaves(), vec![1, 3, 2, 4]);
    }

    /// The key moves the boundary, so it moves the same way whichever side of
    /// it asked. Which chart grows is then a consequence of where you were
    /// sitting, and never a surprise about which way the key points.
    #[test]
    fn a_divider_goes_the_way_the_key_points_from_either_side_of_it() {
        let layout = Node::leaf(1).split(1, 2, true);

        let (divider, ratio) = layout.resize(1, Resize::Right).unwrap();
        assert_eq!(divider, Vec::<bool>::new());
        assert!((ratio - 0.52).abs() < 1e-9, "the left pane gains: {ratio}");

        let (divider, ratio) = layout.resize(2, Resize::Right).unwrap();
        assert_eq!(divider, Vec::<bool>::new());
        assert!((ratio - 0.52).abs() < 1e-9, "same divider, same way: {ratio}");

        let (_, ratio) = layout.resize(1, Resize::Left).unwrap();
        assert!((ratio - 0.48).abs() < 1e-9, "{ratio}");
        let (_, ratio) = layout.resize(2, Resize::Left).unwrap();
        assert!((ratio - 0.48).abs() < 1e-9, "{ratio}");
    }

    /// Two columns, each divided: every pane has a column divider and a row
    /// divider over it, and the nearer of the two is the row. Pressing
    /// sideways has to reach past it to the column divider at the root.
    #[test]
    fn a_two_by_two_resizes_on_the_divider_of_the_orientation_asked_for() {
        let layout = Node::leaf(1).split(1, 2, true).split(1, 3, false).split(2, 4, false);

        // Top left: the root divider across, its own column's divider down.
        assert_eq!(layout.resize(1, Resize::Right), Some((vec![], 0.52)));
        assert_eq!(layout.resize(1, Resize::Down), Some((vec![true], 0.52)));

        // Bottom right reaches the same two orientations on different
        // dividers — the root across, its own column's down.
        assert_eq!(layout.resize(4, Resize::Right), Some((vec![], 0.52)));
        assert_eq!(layout.resize(4, Resize::Down), Some((vec![false], 0.52)));

        // And every one of them answers the key, not the pane's side of it.
        assert_eq!(layout.resize(3, Resize::Right), Some((vec![], 0.52)));
        assert_eq!(layout.resize(3, Resize::Up), Some((vec![true], 0.48)));
    }

    /// Nothing to move is a key that does nothing. There is no divider that
    /// way, and no other divider that would be the right answer instead.
    #[test]
    fn a_chart_with_no_divider_that_way_does_not_resize() {
        assert_eq!(Node::leaf(1).resize(1, Resize::Right), None);
        assert_eq!(Node::leaf(1).resize(1, Resize::Down), None);

        let column = Node::leaf(1).split(1, 2, false);
        assert_eq!(column.resize(1, Resize::Right), None, "a column has no width to give");
        assert!(column.resize(1, Resize::Up).is_some());

        let row = Node::leaf(1).split(1, 2, true);
        assert_eq!(row.resize(2, Resize::Down), None, "a row has no height to give");

        // And a pane that is not in this arrangement at all.
        assert_eq!(row.resize(9, Resize::Right), None);
    }

    /// Held down, a resize key arrives at the end of its travel and stops
    /// there — at the same place a dragged handle stops, not past it.
    #[test]
    fn a_nudged_divider_stops_where_a_dragged_one_does() {
        let layout = Node::leaf(1).split(1, 2, true).with_ratio(&[], 0.94);
        let (divider, ratio) = layout.resize(1, Resize::Right).unwrap();
        assert!((ratio - DIVIDER_MAX).abs() < 1e-9, "{ratio}");

        // Twice more and it is still there rather than off the end.
        let layout = layout.with_ratio(&divider, ratio);
        let (_, ratio) = layout.resize(1, Resize::Right).unwrap();
        assert!((ratio - DIVIDER_MAX).abs() < 1e-9, "{ratio}");

        let layout = Node::leaf(1).split(1, 2, true).with_ratio(&[], 0.06);
        let (_, ratio) = layout.resize(1, Resize::Left).unwrap();
        assert!((ratio - DIVIDER_MIN).abs() < 1e-9, "{ratio}");
    }

    /// What the key press actually does: the divider named is the one that
    /// moves, and every other divider is left where it was.
    #[test]
    fn resizing_moves_one_divider_and_leaves_the_others_alone() {
        let layout = Node::leaf(1)
            .split(1, 2, true)
            .split(1, 3, false)
            .split(2, 4, false)
            .with_ratio(&[true], 0.3)
            .with_ratio(&[false], 0.7);

        let (divider, ratio) = layout.resize(3, Resize::Down).unwrap();
        let resized = layout.with_ratio(&divider, ratio);

        let Node::Split { ratio: across, first, second, .. } = &resized else { panic!() };
        assert!((across - 0.5).abs() < 1e-9, "the columns did not move: {across}");
        let Node::Split { ratio: left, .. } = &**first else { panic!() };
        assert!((left - 0.32).abs() < 1e-9, "the left column's divider went down: {left}");
        let Node::Split { ratio: right, .. } = &**second else { panic!() };
        assert!((right - 0.7).abs() < 1e-9, "the right column did not move: {right}");
    }
}
