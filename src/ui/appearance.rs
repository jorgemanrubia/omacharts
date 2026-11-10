//! How every chart looks, on one page: the theme, and what candles look
//! like.
//!
//! Built wherever it is wanted — in Preferences, and beside a chart's own
//! settings, where you are when you notice the candles are the wrong
//! colour. A theme is an Omarchy theme, and making one is how a
//! palette is made your own — Omarchy already has the tools for that, and a
//! theme made there dresses the whole desktop rather than one app.
//!
//! The charts behind the window are the preview: they change as a choice is
//! made.

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use adw::prelude::*;
use gtk::glib;
use omacharts_engine::BarStyle;
use omacharts_engine::theme::{
    bar_palette, BarLook, BarScheme, Body, ColorChoice, Direction, Theme, Thickness,
};

use crate::chart_look::{ChartLook, LookEdit};
use crate::theming::Theming;
use crate::ui::window::Window;
use crate::ui::{colors, palette, scope_rule, theme_picker};

struct Context {
    window: Rc<Window>,
    theming: Rc<RefCell<Theming>>,
    page: adw::PreferencesPage,
    /// The look the page shows, as the rule scopes it: read once per
    /// rebuild and per change, not per picture drawn.
    look: RefCell<ChartLook>,
    /// The groups [`rebuild`] makes anew: the theme's colours are in them.
    groups: RefCell<Vec<adw::PreferencesGroup>>,
}

thread_local! {
    /// Every page built and not yet thrown away, so a desktop theme change
    /// can redraw the card that follows it. More than one: Preferences and
    /// a chart's settings can both be open.
    static SHOWN: RefCell<Vec<Weak<Context>>> = const { RefCell::new(Vec::new()) };
}

/// The Appearance page. The theme and the bars go through
/// [`scope_rule::change`], which repaints what it changed.
pub fn page(window: Rc<Window>, theming: Rc<RefCell<Theming>>) -> adw::PreferencesPage {
    let context = Rc::new(Context {
        look: RefCell::new(scope_rule::look(&window)),
        window,
        theming,
        page: adw::PreferencesPage::new(),
        groups: RefCell::new(Vec::new()),
    });
    rebuild(&context);
    SHOWN.with(|shown| {
        let mut shown = shown.borrow_mut();
        shown.retain(|c| c.upgrade().is_some_and(|c| c.page.root().is_some()));
        shown.push(Rc::downgrade(&context));
    });
    context.page.clone()
}

/// The theme or the bars changed from somewhere other than one of these
/// pages — the desktop, a command — so every page on screen shows it.
pub fn restyled() {
    let shown: Vec<Rc<Context>> =
        SHOWN.with(|shown| shown.borrow().iter().filter_map(Weak::upgrade).collect());
    for context in shown.iter().filter(|c| c.page.root().is_some()) {
        rebuild(context);
    }
}

/// Clear and re-add the groups.
///
/// Called whenever the theme changes, because the bar colours on offer are
/// the theme's.
fn rebuild(context: &Rc<Context>) {
    *context.look.borrow_mut() = scope_rule::look(&context.window);
    let page = &context.page;
    for group in context.groups.borrow_mut().drain(..) {
        page.remove(&group);
    }
    let groups = vec![theme_group(context), bars_group(context)];
    for group in &groups {
        page.add(group);
    }
    *context.groups.borrow_mut() = groups;
}

/// The theme the look in scope is drawn on: the window's, or the focused
/// chart's own.
fn theme(context: &Context) -> Theme {
    context.window.theme_of(&context.look.borrow())
}

fn theme_group(context: &Rc<Context>) -> adw::PreferencesGroup {
    let look = context.look.borrow().bars.clone();
    let choices = theme_picker::Choices {
        desktop: context.theming.borrow().desktop().cloned(),
        named: context.theming.borrow().named().to_vec(),
        current: theme(context).id,
        bars: Rc::new(move |theme| look.scheme(theme)),
        pick: {
            let ctx = context.clone();
            Rc::new(move |id| choose_theme(&ctx, id))
        },
    };
    theme_picker::group(choices)
}

/// Wear a theme, where the rule says, and rebuild the page once the card
/// that chose it has finished: the cards and the bar colours on offer are
/// the theme's.
fn choose_theme(context: &Rc<Context>, id: &str) {
    if id == theme(context).id {
        return;
    }
    scope_rule::change(&context.window, LookEdit::Theme(Some(id.to_string())));
    // Every page, not only this one: the other one open shows the old
    // theme's cards and colours otherwise.
    glib::idle_add_local_once(restyled);
}

/// The bars as a card: how they are drawn at its head, then Up and Down,
/// each its colour and, beside it, what the style draws of that direction:
/// a candle's body, an OHLC bar's thickness. What a style does not draw is
/// not shown, rather than greyed: a body on OHLC bars, a thickness on
/// candles.
fn bars_group(context: &Rc<Context>) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::new();
    group.set_title("Bars");

    let offered = bar_palette(&theme(context));

    let card = Card::default();
    group.add(&style_head(context, &card));
    for up in [true, false] {
        group.add(&direction_row(context, &offered, up, &card));
    }
    card.show(context.look.borrow().style);
    group
}

/// What the Bars group redraws and hides as its answers change.
#[derive(Default, Clone)]
struct Card {
    /// Everything drawn in the bars' colours, redrawn after any change.
    pictures: Rc<RefCell<Vec<gtk::DrawingArea>>>,
    parts: Rc<RefCell<Vec<Part>>>,
}

/// A part only some styles draw, with the test for which. Weak: a part's
/// own controls hold the card.
type Part = (glib::WeakRef<gtk::Widget>, fn(BarStyle) -> bool);

impl Card {
    fn drawn(&self, picture: &gtk::DrawingArea) {
        self.pictures.borrow_mut().push(picture.clone());
    }

    fn shown_under(&self, part: &impl IsA<gtk::Widget>, under: fn(BarStyle) -> bool) {
        self.parts.borrow_mut().push((part.upcast_ref::<gtk::Widget>().downgrade(), under));
    }

    /// Show what `style` draws, and nothing it does not.
    fn show(&self, style: BarStyle) {
        for (part, under) in self.parts.borrow().iter() {
            if let Some(part) = part.upgrade() {
                part.set_visible(under(style));
            }
        }
    }
}

/// How the bars are drawn, as the card's head: three tabs, each a picture
/// and a word, the one in use underlined. A head rather than a row, and
/// bigger than the rows, because what it picks decides which rows there
/// are under it. Named at the left like a row, the tabs at the right,
/// where the rows' controls are. A style is told apart by its shape, so the
/// picture says it before the word does.
fn style_head(context: &Rc<Context>, card: &Card) -> adw::PreferencesRow {
    let head = adw::PreferencesRow::new();
    head.set_title("Style");
    head.set_activatable(false);
    head.add_css_class("bars-head");
    let tabs = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    // The last word's end over the colours' chevrons, which sit inside the
    // chip's padding and the row's suffix spacing: measured on screen.
    tabs.set_margin_end(EDGE + 7);
    let name = gtk::Label::new(Some("Style"));
    name.set_hexpand(true);
    name.set_xalign(0.0);
    tabs.append(&name);
    let mut first: Option<gtk::ToggleButton> = None;
    for each in BarStyle::ALL {
        let button = gtk::ToggleButton::new();
        let inside = gtk::Box::new(gtk::Orientation::Horizontal, 9);
        let glyph = style_glyph(context, each);
        card.drawn(&glyph);
        inside.append(&glyph);
        inside.append(&gtk::Label::new(Some(match each {
            BarStyle::Candles => "Candles",
            BarStyle::Ohlc => "OHLC",
        })));
        button.set_child(Some(&inside));
        button.set_group(first.as_ref());
        button.set_active(context.look.borrow().style == each);
        let (ctx, card) = (context.clone(), card.clone());
        button.connect_toggled(move |button| {
            if !button.is_active() || ctx.look.borrow().style == each {
                return;
            }
            change(&ctx, LookEdit::Style(each), &card);
            card.show(each);
        });
        first.get_or_insert(button.clone());
        tabs.append(&button);
    }
    head.set_child(Some(&tabs));
    head
}

/// One direction's row: its body for candles, or its thickness for OHLC
/// bars, then its colour. The colour last, at the edge, so it does not move
/// with the style.
fn direction_row(context: &Rc<Context>, offered: &Theme, up: bool, card: &Card) -> adw::ActionRow {
    let row = adw::ActionRow::new();
    row.set_title(if up { "Up" } else { "Down" });
    let (colour, body, thickness) = {
        let look = context.look.borrow();
        let (colour, body, thickness) = side(&look.bars, up);
        (colour.clone(), body, thickness)
    };
    let (drawn_by, picked_by) = (context.clone(), context.clone());
    let repaint = card.clone();
    let bodies = choice(
        card,
        Body::ALL.to_vec(),
        body,
        |body| body_word(body).to_string(),
        move |cr, w, h, body| {
            let mut bars = drawn_by.look.borrow().bars.clone();
            match up {
                true => bars.up_body = body,
                false => bars.down_body = body,
            }
            draw_bar(cr, w, h, &bars.scheme(&theme(&drawn_by)), up, BarStyle::Candles);
        },
        move |body| change(&picked_by, if up { LookEdit::UpBody(body) } else { LookEdit::DownBody(body) }, &repaint),
    );
    card.shown_under(&bodies, |style| style == BarStyle::Candles);
    row.add_suffix(&bodies);

    let (drawn_by, picked_by) = (context.clone(), context.clone());
    let repaint = card.clone();
    let thicknesses = choice(
        card,
        Thickness::ALL.to_vec(),
        thickness,
        |px| px.key().to_string(),
        move |cr, w, h, px| {
            let mut bars = drawn_by.look.borrow().bars.clone();
            match up {
                true => bars.up_thickness = px,
                false => bars.down_thickness = px,
            }
            draw_bar(cr, w, h, &bars.scheme(&theme(&drawn_by)), up, BarStyle::Ohlc);
        },
        move |px| change(&picked_by, if up { LookEdit::UpThickness(px) } else { LookEdit::DownThickness(px) }, &repaint),
    );
    card.shown_under(&thicknesses, |style| style == BarStyle::Ohlc);
    row.add_suffix(&thicknesses);

    let (ctx, repaint) = (context.clone(), card.clone());
    let chip = palette::chip(offered, colour, move |choice| {
        change(&ctx, if up { LookEdit::Up(choice) } else { LookEdit::Down(choice) }, &repaint);
    });
    // Apart from the dropdown before it, so each chevron is read as its own
    // control's.
    chip.set_margin_start(12);
    chip.set_margin_end(EDGE);
    row.add_suffix(&chip);
    row
}

/// Room at the card's right, past the last control of a row.
const EDGE: i32 = 8;

/// The box a dropdown's picture is drawn in, a body's and a thickness's
/// alike.
const GLYPH: (i32, i32) = (14, 16);

fn body_word(body: Body) -> &'static str {
    match body {
        Body::Filled => "Filled",
        Body::Bordered => "Bordered",
        Body::Hollow => "Hollow",
    }
}

/// A dropdown of a few answers, each told by a picture and a word, in the
/// shape of the colour beside it: its face the answer picked, and its list
/// the app's menu, the answer picked ticked. Every face, a body's or a
/// thickness's, is held in a [`palette::slot`] as wide as the longest
/// body, so the row does not move as a word or the style changes; the
/// picture at its left and the chevron after the word. `draw` paints an
/// answer's picture when it is drawn, so a change of colour elsewhere
/// needs only the card's redraw.
fn choice<T: Copy + PartialEq + 'static>(
    card: &Card,
    answers: Vec<T>,
    current: T,
    word: fn(T) -> String,
    draw: impl Fn(&gtk::cairo::Context, f64, f64, T) + 'static,
    picked: impl Fn(T) + 'static,
) -> adw::Bin {
    let words: Vec<String> = answers.iter().map(|answer| word(*answer)).collect();
    let model = gtk::StringList::new(&words.iter().map(String::as_str).collect::<Vec<_>>());
    let answers = Rc::new(answers);
    let draw = Rc::new(draw);
    let factory = |listed: bool| {
        let factory = gtk::SignalListItemFactory::new();
        let card = card.clone();
        factory.connect_setup(move |_, item| {
            let Some(item) = item.downcast_ref::<gtk::ListItem>() else { return };
            let glyph = gtk::DrawingArea::new();
            glyph.set_content_width(GLYPH.0);
            glyph.set_content_height(GLYPH.1);
            glyph.set_valign(gtk::Align::Center);
            card.drawn(&glyph);
            let label = gtk::Label::new(None);
            label.set_xalign(0.0);
            let inside = gtk::Box::new(gtk::Orientation::Horizontal, 7);
            inside.append(&glyph);
            inside.append(&label);
            match listed {
                true => {
                    label.set_hexpand(true);
                    let tick = gtk::Image::from_icon_name("object-select-symbolic");
                    tick.add_css_class("tick");
                    inside.append(&tick);
                }
                false => inside.append(&gtk::Image::from_icon_name("pan-down-symbolic")),
            }
            item.set_child(Some(&inside));
        });
        let (answers, draw) = (answers.clone(), draw.clone());
        factory.connect_bind(move |_, item| {
            let Some(item) = item.downcast_ref::<gtk::ListItem>() else { return };
            let Some(&answer) = answers.get(item.position() as usize) else { return };
            let Some(inside) = item.child() else { return };
            if let Some(glyph) = inside.first_child().and_downcast::<gtk::DrawingArea>() {
                let draw = draw.clone();
                glyph.set_draw_func(move |_, cr, w, h| draw(cr, f64::from(w), f64::from(h), answer));
                if let Some(label) = glyph.next_sibling().and_downcast::<gtk::Label>() {
                    label.set_text(&word(answer));
                }
            }
        });
        factory
    };
    let dropdown = gtk::DropDown::new(Some(model), None::<gtk::Expression>);
    dropdown.add_css_class("bar-choice");
    dropdown.set_valign(gtk::Align::Center);
    // The face draws its own chevron beside its word; GTK's sits out at the
    // edge, by the next control.
    dropdown.set_show_arrow(false);
    dropdown.set_factory(Some(&factory(false)));
    dropdown.set_list_factory(Some(&factory(true)));
    dropdown.set_selected(answers.iter().position(|answer| *answer == current).unwrap_or(0) as u32);
    let picks = answers.clone();
    dropdown.connect_selected_notify(move |dropdown| {
        if let Some(&answer) = picks.get(dropdown.selected() as usize) {
            picked(answer);
        }
    });
    let bodies: Vec<&str> = Body::ALL.iter().map(|body| body_word(*body)).collect();
    let face = dropdown.downgrade();
    palette::slot(&dropdown, &bodies, move || {
        let selected = face.upgrade().map_or(0, |dropdown| dropdown.selected() as usize);
        answers.get(selected).map(|answer| word(*answer)).unwrap_or_default()
    })
}

/// A rise and a fall drawn in `style`, in the colours the charts use.
fn style_glyph(context: &Rc<Context>, style: BarStyle) -> gtk::DrawingArea {
    const W: i32 = 24;
    const H: i32 = 22;
    let area = gtk::DrawingArea::new();
    area.set_content_width(W);
    area.set_content_height(H);
    area.set_valign(gtk::Align::Center);
    let ctx = context.clone();
    area.set_draw_func(move |_, cr, _, _| {
        let theme = theme(&ctx);
        let scheme = ctx.look.borrow().bars.scheme(&theme);
        // Drawn on an 18 by 22 grid, centred.
        cr.translate(f64::from(W - 18) / 2.0, f64::from(H - 22) / 2.0);
        cr.set_line_width(1.5);
        // (wick x, high, low, body top, body bottom, rising)
        let bars = [(5.0, 2.0, 18.0, 6.0, 14.0, true), (13.0, 4.0, 16.0, 8.0, 13.0, false)];
        match style {
            BarStyle::Candles | BarStyle::Ohlc => {
                for (x, high, low, top, bottom, up) in bars {
                    // The direction's colour, whatever its body: a hollow
                    // candle here would read as a fourth style.
                    colors::set_source(cr, if up { &scheme.up } else { &scheme.down });
                    cr.move_to(x, high);
                    cr.line_to(x, low);
                    let _ = cr.stroke();
                    if style == BarStyle::Candles {
                        cr.rectangle(x - 2.5, top, 5.0, bottom - top);
                        let _ = cr.fill();
                    } else {
                        // The open to the left, the close to the right.
                        let (open, close) = if up { (bottom, top) } else { (top, bottom) };
                        cr.move_to(x - 3.0, open);
                        cr.line_to(x, open);
                        cr.move_to(x, close);
                        cr.line_to(x + 3.0, close);
                        let _ = cr.stroke();
                    }
                }
            }
        }
    });
    area
}

/// One direction's half of the look: its colour, its body and its thickness.
fn side(look: &BarLook, up: bool) -> (&ColorChoice, Body, Thickness) {
    match up {
        true => (&look.up, look.up_body, look.up_thickness),
        false => (&look.down, look.down_body, look.down_thickness),
    }
}

/// One bar of this direction as `style` draws it, in `scheme`, `w` by `h`.
fn draw_bar(cr: &gtk::cairo::Context, w: f64, h: f64, scheme: &BarScheme, up: bool, style: BarStyle) {
    let direction = if up { Direction::Up } else { Direction::Down };
    let x = (w / 2.0).floor() + 0.5;
    // A body of an odd number of pixels round a wick down its middle, and
    // as wide for any box, so it stays a candle's shape.
    let width = ((h * 0.45).round() as i32 | 1) as f64;
    let (left, top, bottom) = (x - width / 2.0, (h * 0.25).round(), (h * 0.75).round());
    cr.set_line_width(1.0);
    match style {
        BarStyle::Ohlc => {
            // As thick as the chart draws it, its edge on a pixel's as the
            // chart's is; the open to the left, the close to the right.
            let px = if up { scheme.up_thickness } else { scheme.down_thickness };
            let centre = (px / 2.0).fract();
            let x = (w / 2.0).floor() + centre;
            let (open, close) = if up { (bottom + centre, top + centre) } else { (top + centre, bottom + centre) };
            cr.set_line_width(px);
            colors::set_source(cr, scheme.outline(direction));
            cr.move_to(x, 1.0);
            cr.line_to(x, h - 1.0);
            let reach = (h * 0.3).round();
            cr.move_to(x - reach, open);
            cr.line_to(x, open);
            cr.move_to(x, close);
            cr.line_to(x + reach, close);
            let _ = cr.stroke();
        }
        BarStyle::Candles => {
            let (edge, fill) = (scheme.edge(direction), scheme.body(direction));
            colors::set_source(cr, edge);
            if colors::is_transparent(fill) {
                cr.move_to(x, 1.0);
                cr.line_to(x, top);
                cr.move_to(x, bottom);
                cr.line_to(x, h - 1.0);
                cr.rectangle(left + 0.5, top + 0.5, width - 1.0, bottom - top - 1.0);
                let _ = cr.stroke();
                return;
            }
            cr.move_to(x, 1.0);
            cr.line_to(x, h - 1.0);
            let _ = cr.stroke();
            cr.rectangle(left, top, width, bottom - top);
            let _ = cr.fill();
            if edge != fill {
                colors::set_source(cr, fill);
                cr.rectangle(left + 1.0, top + 1.0, width - 2.0, bottom - top - 2.0);
                let _ = cr.fill();
            }
        }
    }
}

/// Change one of the bars' answers where the rule says, then redraw the
/// card's pictures in the look that leaves.
fn change(context: &Rc<Context>, edit: LookEdit, card: &Card) {
    scope_rule::change(&context.window, edit);
    *context.look.borrow_mut() = scope_rule::look(&context.window);
    card.pictures.borrow().iter().for_each(|p| p.queue_draw());
}
