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

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use adw::prelude::*;
use gtk::glib;
use omacharts_engine::BarStyle;
use omacharts_engine::theme::{bar_palette, BarScheme, Body, Direction, Theme, Thickness};

use crate::chart_look::LookEdit;
use crate::theming::Theming;
use crate::ui::scope_rule::{Scope, Worn};
use crate::ui::{colors, controls, palette, theme_picker};

/// What the page is built from, held by the page's own controls. It holds
/// the page and its groups weakly, so it goes when they do.
struct Context {
    /// The charts the page changes, and the window they are on.
    scope: Rc<Scope>,
    theming: Rc<RefCell<Theming>>,
    page: glib::WeakRef<adw::PreferencesPage>,
    /// What the page shows: read once per change, not per picture drawn.
    shown: RefCell<Shown>,
    /// Set while one of the page's own controls changes the look, so the
    /// page is not rebuilt from under the control's handler.
    changing: Cell<bool>,
    /// The groups [`rebuild`] makes anew: the theme's colours are in them.
    groups: RefCell<Vec<glib::WeakRef<adw::PreferencesGroup>>>,
}

/// The look in scope, as the rule scopes it, and the desktop's theme, which
/// the card that follows it previews whatever the look wears.
#[derive(Clone, PartialEq)]
struct Shown {
    worn: Worn,
    desktop: Option<Theme>,
}

impl Shown {
    fn now(scope: &Scope, theming: &RefCell<Theming>) -> Shown {
        let desktop = theming.borrow().desktop().cloned();
        Shown { worn: scope.worn(), desktop }
    }
}

/// The Appearance page, changing the charts `scope` says. The theme and
/// the bars go through [`Scope::change`], which repaints what it changed;
/// the page follows whatever else changes them through
/// [`crate::ui::window::Window::watch_chart`] — the desktop, a command, the
/// rule, another page — while it is on screen.
pub fn page(scope: Rc<Scope>, theming: Rc<RefCell<Theming>>) -> adw::PreferencesPage {
    let page = adw::PreferencesPage::new();
    let context = Rc::new(Context {
        shown: RefCell::new(Shown::now(&scope, &theming)),
        scope,
        theming,
        page: page.downgrade(),
        changing: Cell::new(false),
        groups: RefCell::new(Vec::new()),
    });
    rebuild(&context);
    let followed = Rc::downgrade(&context);
    context.scope.window().watch_chart(move || {
        let Some(context) = followed.upgrade().filter(|c| c.page.upgrade().is_some_and(|page| page.root().is_some()))
        else {
            return false;
        };
        if !context.changing.get() {
            follow(&context);
        }
        true
    });
    page
}

/// Show what the window wears now, if it is not what the page shows. The
/// whole page, since the cards and the bar colours on offer are the
/// theme's.
fn follow(context: &Rc<Context>) {
    let now = Shown::now(&context.scope, &context.theming);
    if *context.shown.borrow() == now {
        return;
    }
    *context.shown.borrow_mut() = now;
    rebuild(context);
}

/// Clear and re-add the groups.
fn rebuild(context: &Rc<Context>) {
    let Some(page) = context.page.upgrade() else { return };
    for group in context.groups.borrow_mut().drain(..).filter_map(|group| group.upgrade()) {
        page.remove(&group);
    }
    let groups = [theme_group(context), bars_group(context)];
    for group in &groups {
        page.add(group);
    }
    *context.groups.borrow_mut() = groups.iter().map(|group| group.downgrade()).collect();
}

/// Change the look where the rule says, from one of the page's own
/// controls, which shows the change itself.
fn edit(context: &Context, edit: LookEdit) {
    context.changing.set(true);
    context.scope.change(edit);
    context.changing.set(false);
}

fn theme_group(context: &Rc<Context>) -> adw::PreferencesGroup {
    let (look, current) = {
        let worn = &context.shown.borrow().worn;
        (worn.look.bars.clone(), worn.theme.id.clone())
    };
    let choices = theme_picker::Choices {
        desktop: context.theming.borrow().desktop().cloned(),
        named: context.theming.borrow().named(),
        current,
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
    if id == context.shown.borrow().worn.theme.id {
        return;
    }
    edit(context, LookEdit::Theme(Some(id.to_string())));
    let context = context.clone();
    glib::idle_add_local_once(move || follow(&context));
}

/// The bars as a card: how they are drawn at its head, then Up and Down,
/// each its colour and, beside it, what the style draws of that direction:
/// a candle's body, an OHLC bar's thickness. What a style does not draw is
/// not shown, rather than greyed: a body on OHLC bars, a thickness on
/// candles, and Up and Down on a line, which has no rise or fall to colour
/// and shows its two shades in their place.
fn bars_group(context: &Rc<Context>) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::new();
    group.set_title("Bars");

    let offered = bar_palette(&context.shown.borrow().worn.theme);

    let card = Card::default();
    group.add(&style_head(context, &card));
    for direction in [Direction::Up, Direction::Down] {
        let row = direction_row(context, &offered, direction, &card);
        card.shown_under(&row, |style| style != BarStyle::Line);
        group.add(&row);
    }
    for row in line_rows(context, &offered, &card) {
        group.add(&row);
    }
    card.show(context.shown.borrow().worn.look.style);
    group
}

/// A line's two shades, in the places Up and Down hold for the other
/// styles: High, its colour at the highest close on screen, over Low, at
/// the lowest, the way it reads up the chart.
fn line_rows(context: &Rc<Context>, offered: &Theme, card: &Card) -> [adw::ActionRow; 2] {
    let (low, high) = {
        let bars = &context.shown.borrow().worn.look.bars;
        (bars.line_low.clone(), bars.line_high.clone())
    };
    [(false, high), (true, low)].map(|(is_low, choice)| {
        let row = adw::ActionRow::new();
        row.set_title(if is_low { "Low" } else { "High" });
        let (ctx, repaint) = (context.clone(), card.clone());
        let chip = palette::chip(offered, choice, move |choice| {
            change(&ctx, if is_low { LookEdit::LineLow(choice) } else { LookEdit::LineHigh(choice) }, &repaint);
        });
        // At the edge, as Up's and Down's colours are, so it stays put as
        // the style changes.
        chip.set_margin_end(EDGE);
        row.add_suffix(&chip);
        card.shown_under(&row, |style| style == BarStyle::Line);
        row
    })
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
        inside.append(&gtk::Label::new(Some(each.label())));
        button.set_child(Some(&inside));
        button.set_group(first.as_ref());
        button.set_active(context.shown.borrow().worn.look.style == each);
        let (ctx, card) = (context.clone(), card.clone());
        button.connect_toggled(move |button| {
            if !button.is_active() || ctx.shown.borrow().worn.look.style == each {
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
fn direction_row(context: &Rc<Context>, offered: &Theme, direction: Direction, card: &Card) -> adw::ActionRow {
    let row = adw::ActionRow::new();
    row.set_title(if direction == Direction::Up { "Up" } else { "Down" });
    let (colour, body, thickness) = {
        let mut bars = context.shown.borrow().worn.look.bars.clone();
        let side = bars.side_mut(direction);
        (side.colour.clone(), *side.body, *side.thickness)
    };
    for answers in [choice(context, card, direction, body), choice(context, card, direction, thickness)] {
        row.add_suffix(&answers);
    }

    let (ctx, repaint) = (context.clone(), card.clone());
    let chip = palette::chip(offered, colour, move |choice| {
        change(&ctx, LookEdit::Colour(direction, choice), &repaint);
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

/// What a bar choice offers: one direction's body, or its thickness.
trait Answer: Copy + PartialEq + 'static {
    const ALL: &'static [Self];
    /// The style that draws it, and the only one it is shown under.
    const STYLE: BarStyle;
    fn word(self) -> String;
    fn edit(direction: Direction, answer: Self) -> LookEdit;
}

impl Answer for Body {
    const ALL: &'static [Body] = &Body::ALL;
    const STYLE: BarStyle = BarStyle::Candles;
    fn word(self) -> String {
        body_word(self).to_string()
    }
    fn edit(direction: Direction, body: Body) -> LookEdit {
        LookEdit::Body(direction, body)
    }
}

impl Answer for Thickness {
    const ALL: &'static [Thickness] = &Thickness::ALL;
    const STYLE: BarStyle = BarStyle::Ohlc;
    fn word(self) -> String {
        self.key().to_string()
    }
    fn edit(direction: Direction, px: Thickness) -> LookEdit {
        LookEdit::Thickness(direction, px)
    }
}

/// A dropdown of `direction`'s answers of one kind, each told by a picture
/// and a word, in the shape of the colour beside it: its face the answer
/// picked, and its list the app's menu, the answer picked ticked. Every
/// face, a body's or a thickness's, is held in a [`palette::slot`] as wide
/// as the longest body, so the row does not move as a word or the style
/// changes; the picture at its left and the chevron after the word. An
/// answer's picture is the look in scope with that answer, painted when it
/// is drawn, so a change of colour elsewhere needs only the card's redraw.
/// Shown only under the style that draws it.
fn choice<T: Answer>(context: &Rc<Context>, card: &Card, direction: Direction, current: T) -> adw::Bin {
    let answers = T::ALL;
    let words: Rc<[String]> = answers.iter().map(|answer| answer.word()).collect();
    let model = gtk::StringList::new(&words.iter().map(String::as_str).collect::<Vec<_>>());
    let factory = |pictured| {
        let (drawn_by, card) = (context.clone(), card.clone());
        controls::picture_factory(
            GLYPH,
            pictured,
            move |cr, position, w, h| {
                let Some(&answer) = answers.get(position) else { return };
                let worn = &drawn_by.shown.borrow().worn;
                let mut look = worn.look.clone();
                look.set(T::edit(direction, answer));
                draw_bar(cr, w, h, &look.bars.scheme(&worn.theme), direction, T::STYLE);
            },
            move |glyph| card.drawn(glyph),
        )
    };
    let dropdown = gtk::DropDown::new(Some(model), None::<gtk::Expression>);
    dropdown.add_css_class("bar-choice");
    dropdown.set_valign(gtk::Align::Center);
    // The face draws its own chevron beside its word; GTK's sits out at the
    // edge, by the next control.
    dropdown.set_show_arrow(false);
    dropdown.set_factory(Some(&factory(controls::Pictured::Face(words.clone()))));
    dropdown.set_list_factory(Some(&factory(controls::Pictured::Listed(words))));
    dropdown.set_selected(answers.iter().position(|answer| *answer == current).unwrap_or(0) as u32);
    let (picked_by, repaint) = (context.clone(), card.clone());
    dropdown.connect_selected_notify(move |dropdown| {
        if let Some(&answer) = answers.get(dropdown.selected() as usize) {
            change(&picked_by, T::edit(direction, answer), &repaint);
        }
    });
    let bodies: Vec<&str> = Body::ALL.iter().map(|body| body_word(*body)).collect();
    let face = dropdown.downgrade();
    let slot = palette::slot(&dropdown, &bodies, move || {
        let selected = face.upgrade().map_or(0, |dropdown| dropdown.selected() as usize);
        answers.get(selected).map(|answer| answer.word()).unwrap_or_default()
    });
    card.shown_under(&slot, |style| style == T::STYLE);
    slot
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
        let scheme = &ctx.shown.borrow().worn.scheme;
        // Drawn on an 18 by 22 grid, centred.
        cr.translate(f64::from(W - 18) / 2.0, f64::from(H - 22) / 2.0);
        cr.set_line_width(1.5);
        // (wick x, high, low, body top, body bottom, rising)
        let bars = [(5.0, 2.0, 18.0, 6.0, 14.0, true), (13.0, 4.0, 16.0, 8.0, 13.0, false)];
        match style {
            BarStyle::Line => {
                // Shaded as the line chart shades it: it has no up or down.
                cr.move_to(1.0, 15.0);
                cr.line_to(6.0, 9.0);
                cr.line_to(10.0, 12.0);
                cr.line_to(17.0, 3.0);
                let fade = gtk::cairo::LinearGradient::new(0.0, 15.0, 0.0, 3.0);
                for (at, colour) in [(0.0, &scheme.line_low), (1.0, &scheme.line_high)] {
                    let (r, g, b, a) = colors::rgba(colour);
                    fade.add_color_stop_rgba(at, r, g, b, a);
                }
                let _ = cr.set_source(&fade);
                let _ = cr.stroke();
            }
            BarStyle::Candles | BarStyle::Ohlc => {
                for (x, high, low, top, bottom, up) in bars {
                    // The direction's colour, whatever its body: a hollow
                    // candle here would read as a fourth style.
                    let colour = if up { &scheme.up } else { &scheme.down };
                    if style == BarStyle::Candles {
                        candle(cr, colour, colour, (x, high, low), (x - 2.5, top, 5.0, bottom - top));
                    } else {
                        colors::set_source(cr, colour);
                        cr.move_to(x, high);
                        cr.line_to(x, low);
                        let _ = cr.stroke();
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

/// One bar of `direction` as `style` draws it, in `scheme`, `w` by `h`.
fn draw_bar(cr: &gtk::cairo::Context, w: f64, h: f64, scheme: &BarScheme, direction: Direction, style: BarStyle) {
    let up = direction == Direction::Up;
    let x = (w / 2.0).floor() + 0.5;
    // A body of an odd number of pixels round a wick down its middle, and
    // as wide for any box, so it stays a candle's shape.
    let width = ((h * 0.45).round() as i32 | 1) as f64;
    let (left, top, bottom) = (x - width / 2.0, (h * 0.25).round(), (h * 0.75).round());
    cr.set_line_width(1.0);
    match style {
        // A line has no direction, so is never drawn as one; an OHLC bar is
        // the nearest thing.
        BarStyle::Ohlc | BarStyle::Line => {
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
            candle(cr, scheme.edge(direction), scheme.body(direction), (x, 1.0, h - 1.0), (left, top, width, bottom - top));
        }
    }
}

/// One candle, as every picture of one draws it: a wick at `x`, a pixel's
/// centre, from `high` to `low`, in `edge` at the pen's width, and a body
/// over `(left, top, width, height)`, whole pixels. A body with an inside,
/// more than two pixels each way, shows a `fill` unlike its edge a pixel
/// in, and a hollow one shows what is behind it, the wick stopping at its
/// edge; a body with no inside is all edge.
pub(crate) fn candle(cr: &gtk::cairo::Context, edge: &str, fill: &str, (x, high, low): (f64, f64, f64), body: (f64, f64, f64, f64)) {
    let (left, top, width, height) = body;
    let inside = width > 2.0 && height > 2.0;
    let hollow = inside && colors::is_transparent(fill);
    colors::set_source(cr, edge);
    if hollow {
        cr.move_to(x, high);
        cr.line_to(x, top);
        cr.move_to(x, top + height);
        cr.line_to(x, low);
        let _ = cr.stroke();
        let pen = cr.line_width();
        cr.rectangle(left + pen / 2.0, top + pen / 2.0, width - pen, height - pen);
        let _ = cr.stroke();
        return;
    }
    cr.move_to(x, high);
    cr.line_to(x, low);
    let _ = cr.stroke();
    cr.rectangle(left, top, width, height);
    let _ = cr.fill();
    if inside && edge != fill {
        colors::set_source(cr, fill);
        cr.rectangle(left + 1.0, top + 1.0, width - 2.0, height - 2.0);
        let _ = cr.fill();
    }
}

/// Change one of the bars' answers where the rule says, then redraw the
/// card's pictures in the look that leaves.
fn change(context: &Rc<Context>, change: LookEdit, card: &Card) {
    edit(context, change);
    *context.shown.borrow_mut() = Shown::now(&context.scope, &context.theming);
    card.pictures.borrow().iter().for_each(|p| p.queue_draw());
}
