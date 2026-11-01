//! How every chart looks: the theme and what candles look like on one page,
//! and the lines every chart draws or does not on a second, Overlays.
//!
//! Both are built wherever they are wanted — in Preferences, and beside a
//! chart's own settings, where you are when you notice the candles are the
//! wrong colour. A theme is an Omarchy theme, and making one is how a
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
use omacharts_engine::theme::{bar_colour, foreground, BarLook, Body, ColorChoice, Direction, Swatch, FOREGROUND};

use crate::store::Store;
use crate::theming::Theming;
use crate::ui::{colors, palette, theme_picker};

struct Context {
    store: Rc<Store>,
    theming: Rc<RefCell<Theming>>,
    on_change: Rc<dyn Fn()>,
    /// Puts a Thickness chosen here on every chart, and on new ones.
    on_thickness: Rc<dyn Fn(u8)>,
    page: adw::PreferencesPage,
    /// The groups [`rebuild`] owns.
    groups: RefCell<Vec<adw::PreferencesGroup>>,
}

thread_local! {
    /// Every page built and not yet thrown away, so a desktop theme change
    /// can redraw the card that follows it. More than one: Preferences and
    /// a chart's settings can both be open.
    static SHOWN: RefCell<Vec<Weak<Context>>> = const { RefCell::new(Vec::new()) };
}

/// The Appearance page. `on_change` repaints the charts after the theme or
/// the bars have been stored.
pub fn page(
    store: Rc<Store>,
    theming: Rc<RefCell<Theming>>,
    on_change: Rc<dyn Fn()>,
    on_thickness: Rc<dyn Fn(u8)>,
) -> adw::PreferencesPage {
    let context = Rc::new(Context {
        store,
        theming,
        on_change,
        on_thickness,
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

/// Store a change, then repaint the charts.
fn apply(context: &Rc<Context>) {
    context.theming.borrow_mut().apply();
    (context.on_change)();
}

fn theme_group(context: &Rc<Context>) -> adw::PreferencesGroup {
    let theming = context.theming.borrow();
    let look = theming.bar_look().clone();
    let choices = theme_picker::Choices {
        themes: theming.themes().into_iter().cloned().collect(),
        current: theming.theme_id().to_string(),
        bars: Rc::new(move |theme| look.scheme(theme)),
        pick: {
            let ctx = context.clone();
            Rc::new(move |id| choose_theme(&ctx, id))
        },
    };
    drop(theming);
    let group = theme_picker::group(choices);
    group.set_widget_name(crate::ui::scope_rule::GLOBAL_ONLY);
    group
}

/// Wear a theme, and rebuild the page once the card that chose it has
/// finished: the cards and the bar colours on offer are the theme's.
fn choose_theme(context: &Rc<Context>, id: &str) {
    if id == context.theming.borrow().theme_id() {
        return;
    }
    context.theming.borrow_mut().select_theme(id, &context.store);
    apply(context);
    // Every page, not only this one: the other one open shows the old
    // theme's cards and colours otherwise.
    glib::idle_add_local_once(restyled);
}

/// A row for each direction: its colour, and its body as one of three
/// candles drawn in that colour. The body is picked as a picture because
/// "filled, bordered or hollow" is what the eye compares, where a hollow
/// switch and a border switch left the combination to be imagined.
fn bars_group(context: &Rc<Context>) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::new();
    group.set_title("Bars");

    let theming = context.theming.borrow();
    let theme = theming.theme();
    let mut look = theming.bar_look().clone();
    drop(theming);

    let thickness = thickness_row(context);
    group.add(&style_row(context, &thickness));
    group.add(&thickness);

    // The theme's swatches and Foreground: what a bar can be drawn in.
    let mut offered = theme.clone();
    offered.swatches.push(Swatch::new(FOREGROUND, &foreground(&theme)));

    for up in [true, false] {
        let (choice, current) = side(&mut look, up);
        let (choice, current) = (choice.clone(), *current);
        let row = adw::ActionRow::new();
        row.set_title(if up { "Up" } else { "Down" });

        let strip = gtk::Box::new(gtk::Orientation::Horizontal, 2);
        strip.add_css_class("segmented");
        let mut first: Option<gtk::ToggleButton> = None;
        let mut glyphs = Vec::new();
        for body in Body::ALL {
            let glyph = candle_glyph(context, up, body);
            let tile = gtk::ToggleButton::new();
            tile.add_css_class("body-tile");
            tile.set_tooltip_text(Some(match body {
                Body::Filled => "Filled",
                Body::Bordered => "Bordered in the text colour",
                Body::Hollow => "Hollow",
            }));
            tile.set_child(Some(&glyph));
            tile.set_group(first.as_ref());
            tile.set_active(current == body);
            let ctx = context.clone();
            tile.connect_toggled(move |tile| {
                if tile.is_active() {
                    change_bars(&ctx, |look| *side(look, up).1 = body);
                }
            });
            first.get_or_insert(tile.clone());
            strip.append(&tile);
            glyphs.push(glyph);
        }

        let ctx = context.clone();
        let picker = palette::picker(&offered, Some(choice.clone()), &bar_colour(&offered, &choice), move |choice| {
            change_bars(&ctx, |look| *side(look, up).0 = choice);
            // The candles are drawn in the colour just picked.
            glyphs.iter().for_each(|glyph| glyph.queue_draw());
        });
        row.add_suffix(&picker);
        row.add_suffix(&strip);
        group.add(&row);
    }
    group
}

/// How every chart draws its bars, as three pictures of them: a style is
/// told apart by its shape, which a word only describes.
/// How many pixels wide OHLC bars are drawn, offered only while they are
/// what the bars are drawn as. Every chart's, like the rest of the page.
fn thickness_row(context: &Rc<Context>) -> adw::ActionRow {
    let current = crate::chart_defaults::ChartDefaults::load(&context.store).ohlc_thickness;
    let on_thickness = context.on_thickness.clone();
    let row = crate::ui::controls::bounded_row(
        crate::ui::chart_settings::THICKNESS_LABEL,
        None,
        f64::from(current),
        f64::from(crate::chart_defaults::MIN_OHLC_THICKNESS),
        f64::from(crate::chart_defaults::MAX_OHLC_THICKNESS),
        1.0,
        0,
        move |px| on_thickness(px.round() as u8),
    )
    .row;
    row.set_visible(crate::chart_defaults::bar_style(&context.store) == BarStyle::Ohlc);
    row
}

fn style_row(context: &Rc<Context>, thickness: &adw::ActionRow) -> adw::ActionRow {
    let row = adw::ActionRow::new();
    row.set_title("Drawn as");
    let strip = gtk::Box::new(gtk::Orientation::Horizontal, 2);
    strip.add_css_class("segmented");
    let current = crate::chart_defaults::bar_style(&context.store);
    let mut first: Option<gtk::ToggleButton> = None;
    for style in BarStyle::ALL {
        let tile = gtk::ToggleButton::new();
        tile.add_css_class("body-tile");
        let inside = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        inside.append(&style_glyph(context, style));
        inside.append(&gtk::Label::new(Some(match style {
            BarStyle::Candles => "Candles",
            BarStyle::Ohlc => "OHLC",
        })));
        tile.set_child(Some(&inside));
        tile.set_group(first.as_ref());
        tile.set_active(current == style);
        let (ctx, thickness) = (context.clone(), thickness.clone());
        tile.connect_toggled(move |tile| {
            if tile.is_active() {
                thickness.set_visible(style == BarStyle::Ohlc);
            }
            if tile.is_active() && crate::chart_defaults::bar_style(&ctx.store) != style {
                ctx.store.set_setting(crate::chart_defaults::SETTING_BAR_STYLE, style.key());
                (ctx.on_change)();
            }
        });
        first.get_or_insert(tile.clone());
        strip.append(&tile);
    }
    row.add_suffix(&strip);
    row
}

/// A rise and a fall drawn in `style`, in the colours the charts use.
fn style_glyph(context: &Rc<Context>, style: BarStyle) -> gtk::DrawingArea {
    const W: i32 = 18;
    const H: i32 = 22;
    let area = gtk::DrawingArea::new();
    area.set_content_width(W);
    area.set_content_height(H);
    area.set_valign(gtk::Align::Center);
    let ctx = context.clone();
    area.set_draw_func(move |_, cr, _, _| {
        let theming = ctx.theming.borrow();
        let scheme = theming.bar_scheme();
        drop(theming);
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

/// One direction's half of the look: its colour and its body.
fn side(look: &mut BarLook, up: bool) -> (&mut ColorChoice, &mut Body) {
    match up {
        true => (&mut look.up, &mut look.up_body),
        false => (&mut look.down, &mut look.down_body),
    }
}

/// A candle as `body` would draw this direction's, in the colours the charts
/// would use, read when it is drawn so a new colour needs only a redraw.
fn candle_glyph(context: &Rc<Context>, up: bool, body: Body) -> gtk::DrawingArea {
    const W: i32 = 14;
    const H: i32 = 26;
    let area = gtk::DrawingArea::new();
    area.set_content_width(W);
    area.set_content_height(H);
    let ctx = context.clone();
    area.set_draw_func(move |_, cr, _, _| {
        let theming = ctx.theming.borrow();
        let mut look = theming.bar_look().clone();
        *side(&mut look, up).1 = body;
        let scheme = look.scheme(&theming.theme());
        let direction = if up { Direction::Up } else { Direction::Down };
        let (edge, fill) = (scheme.edge(direction), scheme.body(direction));

        // A nine-pixel body round a wick down its middle pixel.
        let (x, left, width, top, bottom) = (7.5, 3.0, 9.0, 8.0, 18.0);
        cr.set_line_width(1.0);
        colors::set_source(cr, edge);
        if colors::is_transparent(fill) {
            cr.move_to(x, 2.0);
            cr.line_to(x, top);
            cr.move_to(x, bottom);
            cr.line_to(x, f64::from(H) - 2.0);
            cr.rectangle(left + 0.5, top + 0.5, width - 1.0, bottom - top - 1.0);
            let _ = cr.stroke();
            return;
        }
        cr.move_to(x, 2.0);
        cr.line_to(x, f64::from(H) - 2.0);
        let _ = cr.stroke();
        cr.rectangle(left, top, width, bottom - top);
        let _ = cr.fill();
        if edge != fill {
            colors::set_source(cr, fill);
            cr.rectangle(left + 1.0, top + 1.0, width - 2.0, bottom - top - 2.0);
            let _ = cr.fill();
        }
    });
    area
}

/// Change one of the bars' answers, store it, and repaint everything.
fn change_bars(context: &Rc<Context>, edit: impl FnOnce(&mut BarLook)) {
    let mut look = context.theming.borrow().bar_look().clone();
    edit(&mut look);
    context.theming.borrow_mut().set_bar_look(look, &context.store);
    apply(context);
}
