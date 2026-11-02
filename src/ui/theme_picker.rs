//! The theme, picked from two cards.
//!
//! One follows the desktop and shows what that looks like right now; the
//! other opens the catalogue of every Omarchy theme on the machine, and once
//! one is picked, shows that one. Two cards rather than a list because the
//! two are different kinds of answer: one keeps changing and the others stay
//! put, and a list puts them side by side as if they were the same.
//!
//! The previews are a miniature of the app — the strip, the chart and the
//! rail — drawn in the theme's colours with the bars as chosen, so a theme
//! is picked by what it will look like rather than by its name.

use std::rc::Rc;

use adw::prelude::*;
use omacharts_engine::theme::{mix, BarScheme, Direction, Theme, OMARCHY_ID};

use crate::ui::{appearance, colors};

/// The tag the catalogue is pushed under.
const CATALOGUE: &str = "themes";

/// A catalogue tile's preview, three to a row in the window's default size.
const TILE_W: i32 = 150;
const TILE_H: i32 = 84;

/// What the cards need from whoever shows them.
pub struct Choices {
    /// The theme following the desktop, if there is a desktop to follow.
    pub desktop: Option<Theme>,
    /// Every other theme on offer, each fixed at its palette.
    pub named: Vec<Theme>,
    /// The theme being worn.
    pub current: String,
    /// The bars as chosen, on a given theme.
    pub bars: Rc<dyn Fn(&Theme) -> BarScheme>,
    /// Wear this theme. Called after the catalogue has closed.
    pub pick: Rc<dyn Fn(&str)>,
}

/// The two cards, in a group of their own.
pub fn group(choices: Choices) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::new();
    group.set_title("Theme");

    let choices = Rc::new(choices);
    let following = choices.current == OMARCHY_ID && choices.desktop.is_some();

    let cards = gtk::Box::new(gtk::Orientation::Horizontal, 14);
    cards.set_homogeneous(true);

    if let Some(desktop) = &choices.desktop {
        let preview = framed(preview(desktop.clone(), (choices.bars)(desktop), false));
        let card = card(
            &preview,
            "Follow system",
            &format!("Now {}", desktop.name),
            false,
            following,
        );
        let pick = choices.pick.clone();
        card.connect_clicked(move |_| pick(OMARCHY_ID));
        cards.append(&card);
    } else {
        group.set_description(Some("One of Omarchy's palettes."));
        // The catalogue keeps half the row and its place on the right, so
        // its picture is the size it is beside Follow system rather than
        // the whole width.
        cards.append(&gtk::Box::new(gtk::Orientation::Horizontal, 0));
    }

    let named = &choices.named;
    let picked = named.iter().find(|t| !following && t.id == choices.current);
    let catalogue = match picked {
        Some(theme) => card(
            &framed(preview(theme.clone(), (choices.bars)(theme), false)),
            &theme.name,
            &format!("Catalogue · {} themes", named.len()),
            true,
            true,
        ),
        None => card(
            &mosaic(named, &choices.bars),
            "Catalogue",
            &format!("{} Omarchy themes", named.len()),
            true,
            false,
        ),
    };
    let opening = choices.clone();
    catalogue.connect_clicked(move |card| {
        let view = card
            .ancestor(adw::NavigationView::static_type())
            .and_downcast::<adw::NavigationView>();
        if let Some(view) = view {
            open_catalogue(&view, &opening, following);
        }
    });
    cards.append(&catalogue);

    group.add(&cards);
    group
}

/// A card: a picture, and under it a mark saying whether it is the one in
/// use, a name and a line.
fn card(picture: &impl IsA<gtk::Widget>, title: &str, line: &str, opens: bool, selected: bool) -> gtk::Button {
    let caption = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    let dot = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    dot.add_css_class("card-dot");
    dot.set_valign(gtk::Align::Center);
    caption.append(&dot);

    let words = gtk::Box::new(gtk::Orientation::Vertical, 1);
    words.set_hexpand(true);
    let name = gtk::Label::new(Some(title));
    name.add_css_class("title");
    name.set_xalign(0.0);
    name.set_ellipsize(gtk::pango::EllipsizeMode::End);
    words.append(&name);
    let detail = gtk::Label::new(Some(line));
    detail.add_css_class("detail");
    detail.set_xalign(0.0);
    detail.set_ellipsize(gtk::pango::EllipsizeMode::End);
    words.append(&detail);
    caption.append(&words);

    if opens {
        let chevron = gtk::Image::from_icon_name("go-next-symbolic");
        chevron.add_css_class("detail");
        caption.append(&chevron);
    }

    let column = gtk::Box::new(gtk::Orientation::Vertical, 10);
    column.append(picture);
    column.append(&caption);

    let button = gtk::Button::new();
    button.add_css_class("theme-card");
    if selected {
        button.add_css_class("selected");
    }
    button.set_child(Some(&column));
    button
}

/// Six themes in a grid, for the catalogue card before one has been picked:
/// what is in there, without having to open it.
fn mosaic(themes: &[Theme], bars: &Rc<dyn Fn(&Theme) -> BarScheme>) -> gtk::AspectFrame {
    let grid = gtk::Grid::new();
    grid.set_row_spacing(4);
    grid.set_column_spacing(4);
    grid.set_row_homogeneous(true);
    grid.set_column_homogeneous(true);
    // Spread across the list, so the six are not six of a kind.
    let step = (themes.len() / 6).max(1);
    for (n, theme) in themes.iter().step_by(step).take(6).enumerate() {
        let area = preview(theme.clone(), bars(theme), true);
        area.set_hexpand(true);
        area.set_vexpand(true);
        grid.attach(&area, n as i32 % 3, n as i32 / 3, 1, 1);
    }
    let frame = gtk::AspectFrame::new(0.5, 0.5, 16.0 / 9.0, false);
    frame.set_child(Some(&grid));
    // The tiles expand to fill the frame. Said outright here, or GTK hands
    // that expansion up to the card and the page, and on a window taller
    // than the page both theme cards stretch to fill it.
    frame.set_vexpand(false);
    frame
}

/// Every theme as a tile; picking one closes the catalogue and wears it.
/// None is marked picked while the desktop is `following`.
fn open_catalogue(view: &adw::NavigationView, choices: &Rc<Choices>, following: bool) {
    if view.find_page(CATALOGUE).is_some() {
        view.pop_to_tag(CATALOGUE);
        return;
    }
    let desktop = choices.desktop.as_ref().map(|t| t.name.as_str());

    let flow = gtk::FlowBox::new();
    flow.set_selection_mode(gtk::SelectionMode::None);
    flow.set_homogeneous(true);
    flow.set_min_children_per_line(3);
    flow.set_max_children_per_line(5);
    flow.set_column_spacing(12);
    flow.set_row_spacing(12);
    for theme in &choices.named {
        // A fixed size rather than a frame: a flow box gives its children
        // their natural height, so a frame there could only ever shrink.
        let picture = preview(theme.clone(), (choices.bars)(theme), false);
        picture.set_content_width(TILE_W);
        picture.set_content_height(TILE_H);
        let label = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        let name = gtk::Label::new(Some(&theme.name));
        name.set_xalign(0.0);
        name.set_hexpand(true);
        name.set_ellipsize(gtk::pango::EllipsizeMode::End);
        label.append(&name);
        // The theme the desktop is wearing, so it can be found by name.
        if desktop == Some(theme.name.as_str()) {
            let tag = gtk::Label::new(Some("desktop"));
            tag.add_css_class("detail");
            label.append(&tag);
        }
        let column = gtk::Box::new(gtk::Orientation::Vertical, 6);
        column.append(&picture);
        column.append(&label);

        let tile = gtk::Button::new();
        tile.add_css_class("flat");
        tile.add_css_class("theme-tile");
        tile.set_halign(gtk::Align::Center);
        if !following && theme.id == choices.current {
            tile.add_css_class("selected");
        }
        tile.set_child(Some(&column));
        tile.set_tooltip_text(Some(&theme.name));
        let pick = choices.pick.clone();
        let id = theme.id.clone();
        let back = view.downgrade();
        tile.connect_clicked(move |_| {
            if let Some(view) = back.upgrade() {
                view.pop();
            }
            pick(&id);
        });
        flow.insert(&tile, -1);
    }

    let note = gtk::Label::new(Some("Your Omarchy themes. Each stays put whatever the desktop does."));
    note.add_css_class("detail");
    note.set_xalign(0.0);
    note.set_wrap(true);

    let content = gtk::Box::new(gtk::Orientation::Vertical, 14);
    content.set_margin_top(12);
    content.set_margin_bottom(24);
    content.set_margin_start(24);
    content.set_margin_end(24);
    content.append(&note);
    content.append(&flow);

    let clamp = adw::Clamp::new();
    clamp.set_maximum_size(860);
    clamp.set_child(Some(&content));
    let scroller = gtk::ScrolledWindow::new();
    scroller.set_hscrollbar_policy(gtk::PolicyType::Never);
    scroller.set_child(Some(&clamp));

    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&adw::HeaderBar::new());
    toolbar.set_content(Some(&scroller));
    let page = adw::NavigationPage::with_tag(&toolbar, "Themes", CATALOGUE);
    page.add_css_class("theme-catalogue");
    view.push(&page);
}

/// A preview held to the screen's shape, whatever width it is given.
fn framed(area: gtk::DrawingArea) -> gtk::AspectFrame {
    let frame = gtk::AspectFrame::new(0.5, 0.5, 16.0 / 9.0, false);
    frame.set_child(Some(&area));
    frame
}

/// A miniature of the app in `theme`, with `bars` for candles. `compact`
/// leaves out the strip and the rail, for a preview too small to show them.
fn preview(theme: Theme, bars: BarScheme, compact: bool) -> gtk::DrawingArea {
    let area = gtk::DrawingArea::new();
    area.add_css_class("theme-preview");
    area.set_overflow(gtk::Overflow::Hidden);
    area.set_content_width(if compact { 40 } else { 160 });
    area.set_content_height(if compact { 22 } else { 90 });
    area.set_draw_func(move |_, cr, width, height| {
        paint(cr, f64::from(width), f64::from(height), &theme, &bars, compact);
    });
    area
}

/// The same few candles in every preview, as (open, close, high, low).
const CANDLES: [(f64, f64, f64, f64); 24] = [
    (0.42, 0.50, 0.55, 0.38), (0.50, 0.46, 0.53, 0.42), (0.46, 0.56, 0.60, 0.44),
    (0.56, 0.62, 0.66, 0.53), (0.62, 0.55, 0.64, 0.51), (0.55, 0.48, 0.58, 0.44),
    (0.48, 0.40, 0.50, 0.35), (0.40, 0.45, 0.48, 0.36), (0.45, 0.38, 0.47, 0.33),
    (0.38, 0.30, 0.41, 0.25), (0.30, 0.36, 0.39, 0.27), (0.36, 0.44, 0.47, 0.34),
    (0.44, 0.52, 0.56, 0.42), (0.52, 0.49, 0.57, 0.46), (0.49, 0.58, 0.61, 0.47),
    (0.58, 0.66, 0.70, 0.56), (0.66, 0.61, 0.69, 0.58), (0.61, 0.70, 0.74, 0.59),
    (0.70, 0.76, 0.80, 0.67), (0.76, 0.71, 0.79, 0.68), (0.71, 0.79, 0.83, 0.69),
    (0.79, 0.86, 0.90, 0.77), (0.86, 0.81, 0.88, 0.78), (0.81, 0.88, 0.93, 0.79),
];

fn paint(cr: &gtk::cairo::Context, w: f64, h: f64, theme: &Theme, bars: &BarScheme, compact: bool) {
    let ui = &theme.ui;
    colors::set_source(cr, &ui.background);
    let _ = cr.paint();

    let rail = if compact { 0.0 } else { (w * 0.27).round() };
    let head = if compact { 0.0 } else { (h * 0.11).round() };
    cr.set_line_width(1.0);

    // The strip along the top: a symbol and a few resolutions.
    if !compact {
        colors::set_source(cr, &mix(&ui.text, &ui.background, 0.55));
        let bar_h = (head * 0.28).max(2.0);
        cr.rectangle(8.0, head * 0.38, 22.0, bar_h);
        for n in 0..4 {
            cr.rectangle((w * 0.22).round() + f64::from(n) * 16.0, head * 0.4, 10.0, (head * 0.22).max(2.0));
        }
        let _ = cr.fill();
    }

    // The plot: grid, then candles.
    let (left, right) = (6.0, w - rail - if compact { 6.0 } else { 22.0 });
    let (top, bottom) = (head + if compact { 3.0 } else { 6.0 }, h - if compact { 3.0 } else { 12.0 });
    colors::set_source(cr, &ui.grid);
    for n in 1..4 {
        let y = (top + (bottom - top) * f64::from(n) / 4.0).round() + 0.5;
        cr.move_to(left, y);
        cr.line_to(w - rail - 4.0, y);
    }
    let _ = cr.stroke();

    let y = |v: f64| bottom - v * (bottom - top);
    let step = (right - left) / CANDLES.len() as f64;
    let body = (step * 0.6).round().max(1.0);
    for rising in [true, false] {
        // In the direction's own colour, never a border's, as the chart
        // draws a candle as narrow as these.
        let direction = if rising { Direction::Up } else { Direction::Down };
        let (edge, fill) = (bars.outline(direction), bars.body(direction));
        for (n, (open, close, high, low)) in CANDLES.into_iter().enumerate() {
            if (close >= open) != rising {
                continue;
            }
            let x = (left + step * n as f64 + step / 2.0).round() + 0.5;
            let (upper, lower) = (y(open.max(close)).round(), y(open.min(close)).round().max(y(open.max(close)).round() + 1.0));
            let wick = (x, y(high).round(), y(low).round());
            appearance::candle(cr, edge, fill, wick, ((x - body / 2.0).round(), upper, body, lower - upper));
        }
    }
    if compact {
        return;
    }

    // The last price on the axis, and the rail with a few rows.
    let (_, last, _, _) = CANDLES[CANDLES.len() - 1];
    colors::set_source(cr, &bars.up);
    cr.rectangle(right + 3.0, y(last) - 3.0, 16.0, 6.0);
    let _ = cr.fill();

    let panel = &ui.surface;
    colors::set_source(cr, panel);
    cr.rectangle(w - rail, 0.0, rail, h);
    let _ = cr.fill();
    colors::set_source(cr, &mix(&ui.accent, panel, 0.7));
    cr.rectangle(w - rail + 4.0, head + 5.0, rail - 8.0, 7.0);
    let _ = cr.fill();
    let rows = 9;
    let pitch = (h - head - 12.0) / f64::from(rows);
    for n in 0..rows {
        let row = head + 7.0 + f64::from(n) * pitch;
        colors::set_source(cr, &mix(&ui.text, panel, 0.35));
        cr.rectangle(w - rail + 8.0, row, rail * 0.32, 3.0);
        let _ = cr.fill();
        colors::set_source(cr, if n % 3 == 1 { &bars.down } else { &bars.up });
        cr.rectangle(w - 8.0 - rail * 0.22, row, rail * 0.22, 3.0);
        let _ = cr.fill();
    }
}
