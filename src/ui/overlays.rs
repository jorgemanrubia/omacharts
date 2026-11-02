//! What every chart draws over its bars: the header across its top, and
//! the lines across it. One answer for all of them, in every chartbook,
//! rather than a switch per chart that only decides what the next chart
//! opens with.
//!
//! Built wherever it is wanted, as Appearance is: in Preferences, and
//! beside a chart's own settings.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use adw::prelude::*;
use gtk::glib;
use omacharts_engine::{Direction, Theme};

use crate::chart_look::{ChartLook, LookEdit};
use crate::header::{Layout, Part, Zone};
use crate::store::Store;
use crate::ui::window::{Window, SETTING_SYNC_CROSSHAIR};
use crate::ui::{colors, drop_line, scope_rule};

/// A Lines switch on screen, and how it reads whether its line is drawn.
type Switch = (glib::WeakRef<adw::SwitchRow>, Rc<dyn Fn() -> bool>);

thread_local! {
    /// Every Lines switch on screen, so a key or a command that changes a
    /// setting moves its switch with it.
    static SWITCHES: RefCell<Vec<Switch>> = const { RefCell::new(Vec::new()) };
    /// Every header card on screen, so a command, a theme change or a
    /// change of scope reaches it. More than one: Preferences and a chart's
    /// settings can both be open.
    static CARDS: RefCell<Vec<Weak<HeaderCard>>> = const { RefCell::new(Vec::new()) };
}

/// The Overlays page. The header and the lines are the look, changed where
/// the rule says; `on_crosshair` puts the crosshair switch, which is every
/// chart's, on the charts once it is stored.
pub fn page(window: Rc<Window>, store: Rc<Store>, on_crosshair: Rc<dyn Fn()>) -> adw::PreferencesPage {
    let page = adw::PreferencesPage::new();
    let header = adw::PreferencesGroup::new();
    header.set_title("Header");
    let card = HeaderCard::new(window.clone(), store.clone());
    let restore = gtk::Button::with_label("Restore defaults");
    restore.add_css_class("flat");
    let restoring = card.clone();
    restore.connect_clicked(move |_| restoring.set(Layout::standard()));
    header.set_header_suffix(Some(&restore));
    header.add(&card.card);
    page.add(&header);
    page.add(&lines_group(window, store, on_crosshair));
    page
}

/// The header, the look of the charts or the scope changed — a command, the
/// Appearance page, the desktop's theme, the rule over the sidebar, the
/// focused chart — so every header card on screen shows the header in scope
/// now, in the colours the charts use now.
pub fn header_changed() {
    let live: Vec<Rc<HeaderCard>> = CARDS.with(|cards| {
        let mut cards = cards.borrow_mut();
        cards.retain(|card| card.upgrade().is_some());
        cards.iter().filter_map(Weak::upgrade).collect()
    });
    for card in live {
        *card.layout.borrow_mut() = scope_rule::look(&card.window).header;
        card.rebuild();
    }
}

/// The header as a picture of it: its two rows of left, centre and right
/// laid over a faint chart the way a chart lays them out, each part drawn
/// as a chart draws it, and the parts it does not show waiting underneath,
/// drawn the same way. A part is dragged into a place, or back down among
/// the others to take it off, as a right-click or a double-click on it does.
///
/// A picture rather than a list of names, because what is being chosen is
/// how the top of every chart looks, and a name says nothing about that.
/// The places are outlined only while something is being dragged: at rest
/// the picture is what a chart's top looks like.
///
/// It changes the header in scope: every chart's, or under Local the
/// focused chart's. The account, Buy and Sell, the quantity and the day's
/// P&L are shown and offered while trading is switched on, with or without
/// a login, like every trading setting: they draw nothing on a chart until
/// there is an account to trade on. Switched off, they are neither shown nor
/// offered, and stay where they were.
struct HeaderCard {
    window: Rc<Window>,
    store: Rc<Store>,
    layout: RefCell<Layout>,
    card: gtk::Box,
    chart: gtk::DrawingArea,
    /// The six places, in the order of [`Zone::ALL`].
    zones: [gtk::Box; 6],
    second: gtk::CenterBox,
    /// The parts the header does not show, and where a part is dropped to
    /// take it off.
    spare: gtk::Box,
    spare_row: gtk::Box,
    all_shown: gtk::Label,
}

impl HeaderCard {
    fn new(window: Rc<Window>, store: Rc<Store>) -> Rc<HeaderCard> {
        let card = gtk::Box::new(gtk::Orientation::Vertical, 0);
        card.add_css_class("header-card");
        card.set_overflow(gtk::Overflow::Hidden);

        let chart = gtk::DrawingArea::new();
        chart.set_content_height(208);
        chart.set_hexpand(true);
        let zones: [gtk::Box; 6] = std::array::from_fn(|_| {
            let zone = gtk::Box::new(gtk::Orientation::Horizontal, 2);
            zone.add_css_class("header-zone");
            zone.set_valign(gtk::Align::Center);
            zone
        });
        // A chart's two rows to scale, as the parts are: see `.header-card`.
        let row = |places: &[gtk::Box], height: i32| {
            let row = gtk::CenterBox::new();
            row.set_start_widget(Some(&places[0]));
            row.set_center_widget(Some(&places[1]));
            row.set_end_widget(Some(&places[2]));
            row.set_height_request(height);
            row
        };
        let first = row(&zones[..3], 26);
        let second = row(&zones[3..], 20);
        let rows = gtk::Box::new(gtk::Orientation::Vertical, 0);
        rows.append(&first);
        rows.append(&second);
        // However many parts a row holds, it is shown whole, smaller.
        let fitted = gtk::Box::new(gtk::Orientation::Vertical, 0);
        fitted.set_layout_manager(Some(crate::ui::layouts::Shrink::default()));
        fitted.append(&rows);
        fitted.set_valign(gtk::Align::Start);
        fitted.set_margin_top(6);
        fitted.set_margin_start(12);
        fitted.set_margin_end(12);
        let picture = gtk::Overlay::new();
        picture.set_child(Some(&chart));
        picture.add_overlay(&fitted);
        card.append(&picture);

        let spare_row = gtk::Box::new(gtk::Orientation::Vertical, 0);
        spare_row.add_css_class("header-spares");
        spare_row.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
        let spare = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        spare.set_layout_manager(Some(crate::ui::layouts::Wrap::default()));
        spare.set_margin_top(10);
        spare.set_margin_bottom(10);
        spare.set_margin_start(12);
        spare.set_margin_end(12);
        spare_row.append(&spare);
        let all_shown = gtk::Label::new(Some("Every part is in the header"));
        all_shown.add_css_class("dim-label");
        all_shown.set_margin_top(14);
        all_shown.set_margin_bottom(14);
        spare_row.append(&all_shown);
        card.append(&spare_row);

        let this = Rc::new(HeaderCard {
            layout: RefCell::new(scope_rule::look(&window).header),
            window,
            store,
            card,
            chart,
            zones,
            second,
            spare,
            spare_row,
            all_shown,
        });
        let window = this.window.clone();
        this.chart.set_draw_func(move |_, cr, width, height| {
            let look = scope_rule::look(&window);
            // The candles start just under the rows the header has.
            let top = f64::from(fitted.margin_top() + fitted.height()) + 14.0;
            draw_chart(cr, f64::from(width), f64::from(height), top, &look, &window.theme_of(&look));
        });
        for (zone, place) in Zone::ALL.into_iter().zip(&this.zones) {
            this.take_drops_in(zone, place);
        }
        this.take_drops_back();
        this.rebuild();
        CARDS.with(|cards| cards.borrow_mut().push(Rc::downgrade(&this)));
        this
    }

    fn set(self: &Rc<Self>, layout: Layout) {
        scope_rule::change(&self.window, LookEdit::Header(layout));
        *self.layout.borrow_mut() = scope_rule::look(&self.window).header;
        self.rebuild();
    }

    // The parts hold the card they rebuild, and the card holds the parts:
    // GTK lets go of a part's handlers when the window is disposed, which is
    // what ends that.
    fn rebuild(self: &Rc<Self>) {
        self.card.remove_css_class("dragging");
        for place in &self.zones {
            place.remove_css_class("shut");
            while let Some(child) = place.first_child() {
                place.remove(&child);
            }
        }
        while let Some(child) = self.spare.first_child() {
            self.spare.remove(&child);
        }
        // The colours are read here, so a new theme needs only this.
        self.chart.queue_draw();
        let layout = self.layout.borrow().clone();
        let trading = crate::features::trading();
        for (zone, place) in Zone::ALL.into_iter().zip(&self.zones) {
            for part in layout.shown(zone, trading) {
                place.append(&self.placed(part));
            }
        }
        self.second.set_visible(layout.second_row());
        let spare: Vec<Part> = Part::every_shown(trading).into_iter().filter(|part| !layout.contains(*part)).collect();
        self.all_shown.set_visible(spare.is_empty());
        self.spare.set_visible(!spare.is_empty());
        for part in spare {
            self.spare.append(&self.spare_card(part));
        }
    }

    /// A part where the header has it: dragged to move it, right-clicked or
    /// double-clicked to take it off.
    fn placed(self: &Rc<Self>, part: Part) -> gtk::Widget {
        let chip = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        chip.add_css_class("header-chip");
        chip.add_css_class(drop_line::MARKED);
        chip.set_widget_name(part.key());
        chip.set_valign(gtk::Align::Center);
        chip.set_tooltip_text(Some(part.label()));
        chip.append(&self.sample(part));
        let click = gtk::GestureClick::new();
        click.set_button(0);
        let this = self.clone();
        click.connect_released(move |click, presses, _, _| {
            if click.current_button() == gtk::gdk::BUTTON_SECONDARY || presses == 2 {
                let mut layout = this.layout.borrow().clone();
                layout.remove(part);
                this.set(layout);
            }
        });
        chip.add_controller(click);
        self.lift(chip.upcast_ref(), part);
        chip.upcast()
    }

    /// A part the header does not show: what it looks like, and its name
    /// beside it, on one line so every card is the same height whatever
    /// the part draws. Clicked, it goes where it would be by default.
    fn spare_card(self: &Rc<Self>, part: Part) -> gtk::Widget {
        let card = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        card.add_css_class("header-spare");
        card.set_valign(gtk::Align::Start);
        let look = self.sample(part);
        look.set_valign(gtk::Align::Center);
        // Tighter than on the header, so three cards fit on a line.
        look.set_margin_start(6);
        look.set_margin_end(6);
        card.append(&look);
        let name = gtk::Label::new(Some(part.label()));
        name.set_valign(gtk::Align::Center);
        name.add_css_class("spare-name");
        card.append(&name);
        let click = gtk::GestureClick::new();
        let this = self.clone();
        click.connect_released(move |_, _, _, _| {
            let mut layout = this.layout.borrow().clone();
            layout.add(part);
            this.set(layout);
        });
        card.add_controller(click);
        self.lift(card.upcast_ref(), part);
        card.upcast()
    }

    /// The part as a chart draws it, with a sample's values.
    fn sample(&self, part: Part) -> gtk::Widget {
        let text = |words: &str| {
            let label = gtk::Label::new(Some(words));
            label.add_css_class("readout-symbol");
            label.set_margin_start(5);
            label.set_margin_end(5);
            label
        };
        match part {
            // As an hour chart of crude writes them at the close of the
            // hour drawn behind: see `OIL`.
            Part::Symbol => text("CL=F").upcast(),
            Part::Resolution => text(&SAMPLE_TIMEFRAME.label()).upcast(),
            Part::Last => text("-37.63").upcast(),
            // In the colour a fall is painted in, as on a chart.
            Part::Change => {
                let label = text("");
                label.set_markup(&self.painted(&crate::header::change_text(-305.97), Direction::Down));
                label.upcast()
            }
            Part::Template => {
                let name = crate::templates::all(&self.store).into_iter().next().map(|t| t.name);
                text(name.as_deref().unwrap_or("Template")).upcast()
            }
            Part::Look => crate::ui::pane::look_icon(13).upcast(),
            Part::Name => {
                let label = text("Crude Oil WTI");
                label.remove_css_class("readout-symbol");
                label.add_css_class("legend-name");
                label.set_margin_start(0);
                label.upcast()
            }
            Part::Title => text("Title").upcast(),
            Part::Account => {
                let label = text("U1234567");
                label.add_css_class("dim-label");
                label.upcast()
            }
            // A long held, its open P&L in the colour of a rise.
            Part::Position => {
                let label = text("");
                label.set_markup(&format!("+2 {}", self.painted("(+$125.00)", Direction::Up)));
                label.upcast()
            }
            Part::Feed => {
                let dot = crate::ui::pane::feed_dot();
                dot.add_css_class("connected");
                icon(dot.upcast())
            }
            // The word quiet, the money in the colour of a rise.
            Part::DayPnl | Part::Size => {
                let (word, amount) = match part {
                    Part::DayPnl => ("P&L", self.painted("+$1,250.00", Direction::Up)),
                    _ => ("Qty", "2".to_string()),
                };
                let (line, label) = crate::ui::pane::worded(word);
                line.add_css_class("readout-symbol");
                line.set_margin_start(8);
                line.set_margin_end(8);
                label.set_markup(&amount);
                line.upcast()
            }
            Part::Trade => {
                let (pair, buttons) = crate::ui::pane::trade_pair();
                for button in buttons {
                    button.set_can_target(false);
                    button.set_focusable(false);
                }
                pair.upcast()
            }
            Part::Strip => {
                let strip = crate::ui::pane::strip();
                // The last few, round the day: the card is narrower than a
                // chart, and a strip as wide as a chart's pushed the right
                // of the picture off its edge.
                let listed = self.window.timeframes();
                for timeframe in listed.iter().skip(listed.len().saturating_sub(4)).copied() {
                    let button = crate::ui::pane::strip_button(timeframe);
                    button.set_active(timeframe == SAMPLE_TIMEFRAME);
                    button.set_can_target(false);
                    button.set_focusable(false);
                    strip.append(&button);
                }
                strip.upcast()
            }
            Part::Settings => icon(gtk::Image::from_icon_name(crate::ui::pane::GEAR_ICON).upcast()),
            Part::Maximize => icon(crate::ui::pane::expand_icon(Rc::new(Cell::new(false))).upcast()),
            Part::Link => icon(
                crate::ui::pane::mode_icon(
                    Rc::new(RefCell::new(None)),
                    Rc::new(Cell::new(omacharts_engine::LinkMode::default())),
                )
                .upcast(),
            ),
        }
    }

    /// `words` as markup in the colour a bar going `direction` is painted
    /// in, as on a chart.
    fn painted(&self, words: &str, direction: Direction) -> String {
        let theme = self.window.theme();
        let scheme = scope_rule::look(&self.window).bars.scheme(&theme);
        crate::header::change_markup(words, Some(scheme.outline(direction)))
    }

    /// Make `widget` the handle that drags `part`.
    fn lift(self: &Rc<Self>, widget: &gtk::Widget, part: Part) {
        let source = gtk::DragSource::new();
        source.set_actions(gtk::gdk::DragAction::MOVE);
        let payload = part.key().to_string();
        source.connect_prepare(move |_, _, _| Some(gtk::gdk::ContentProvider::for_value(&payload.to_value())));
        let (lifted, this) = (widget.clone(), self.clone());
        source.connect_drag_begin(move |source, _| {
            let ghost = gtk::WidgetPaintable::new(Some(&lifted));
            source.set_icon(Some(&ghost), lifted.width() / 2, lifted.height() / 2);
            lifted.add_css_class("lifted");
            this.show_places(part);
        });
        // Whether it landed or not: a drop has already rebuilt the card, and
        // one that missed leaves the places to put away.
        let this = self.clone();
        source.connect_drag_end(move |_, _, _| this.rebuild());
        widget.add_controller(source);
    }

    /// Outline the places while `part` is dragged, naming the empty ones
    /// that would take it, and open the second row when any of its places
    /// would. One that would not stays shut, unnamed and narrow.
    fn show_places(&self, part: Part) {
        self.card.add_css_class("dragging");
        let layout = self.layout.borrow();
        self.second.set_visible(layout.second_row() || Zone::ALL[3..].iter().any(|zone| layout.takes(*zone, part)));
        for (zone, place) in Zone::ALL.into_iter().zip(&self.zones) {
            if !layout.takes(zone, part) {
                place.add_css_class("shut");
            } else if place.first_child().is_none() {
                let name = gtk::Label::new(Some(match zone.first_row() {
                    Zone::Left => "left",
                    Zone::Center => "centre",
                    _ => "right",
                }));
                name.add_css_class("zone-name");
                name.set_hexpand(true);
                place.append(&name);
            }
        }
    }

    fn take_drops_in(self: &Rc<Self>, zone: Zone, place: &gtk::Box) {
        let target = gtk::DropTarget::new(glib::Type::STRING, gtk::gdk::DragAction::MOVE);
        target.set_preload(true);
        let (this, over) = (self.clone(), place.clone());
        target.connect_motion(move |target, x, _| {
            let part = target.value().as_ref().and_then(dragged);
            match part.filter(|part| this.layout.borrow().takes(zone, *part)) {
                Some(part) => {
                    over.add_css_class("drop-over");
                    mark_landing(&over, part, x);
                    gtk::gdk::DragAction::MOVE
                }
                None => gtk::gdk::DragAction::empty(),
            }
        });
        let over = place.clone();
        target.connect_leave(move |_| {
            over.remove_css_class("drop-over");
            unmark(&over);
        });
        let (this, over) = (self.clone(), place.clone());
        target.connect_drop(move |_, value, x, _| {
            let Some(part) = dragged(value).filter(|part| this.layout.borrow().takes(zone, *part)) else { return false };
            let mut layout = this.layout.borrow().clone();
            let at = among_all(&layout, zone, part, landing(&over, part, x).0);
            layout.place(part, zone, at);
            this.set(layout);
            true
        });
        place.add_controller(target);
    }

    /// The parts underneath take a part dropped back among them off the
    /// header.
    fn take_drops_back(self: &Rc<Self>) {
        let target = gtk::DropTarget::new(glib::Type::STRING, gtk::gdk::DragAction::MOVE);
        target.set_preload(true);
        let (this, over) = (self.clone(), self.spare_row.clone());
        target.connect_motion(move |target, _, _| {
            match target.value().as_ref().and_then(dragged).filter(|part| this.layout.borrow().contains(*part)) {
                Some(_) => {
                    over.add_css_class("drop-over");
                    gtk::gdk::DragAction::MOVE
                }
                None => gtk::gdk::DragAction::empty(),
            }
        });
        let over = self.spare_row.clone();
        target.connect_leave(move |_| over.remove_css_class("drop-over"));
        let this = self.clone();
        target.connect_drop(move |_, value, _, _| {
            let Some(part) = dragged(value).filter(|part| this.layout.borrow().contains(*part)) else { return false };
            let mut layout = this.layout.borrow().clone();
            layout.remove(part);
            this.set(layout);
            true
        });
        self.spare_row.add_controller(target);
    }
}

/// An icon the size a chart's buttons draw theirs.
fn icon(image: gtk::Widget) -> gtk::Widget {
    let holder = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    holder.add_css_class("header-icon");
    holder.set_valign(gtk::Align::Center);
    // Homogeneous, so the icon is centred in the holder's width without
    // asking the row for more.
    holder.set_homogeneous(true);
    image.set_valign(gtk::Align::Center);
    image.set_halign(gtk::Align::Center);
    holder.append(&image);
    holder.upcast()
}

/// Where in `zone`, hidden parts and all, the `shown`-th part shown with
/// `part` lifted out sits: a drop lands among the parts it can see, and the
/// hidden ones keep their places around it.
fn among_all(layout: &Layout, zone: Zone, part: Part, shown: usize) -> usize {
    let trading = crate::features::trading();
    let others: Vec<Part> = layout.zone(zone).iter().copied().filter(|p| *p != part).collect();
    others
        .iter()
        .enumerate()
        .filter(|(_, p)| p.shown(trading))
        .nth(shown)
        .map_or(others.len(), |(at, _)| at)
}

/// The part a drag carries.
fn dragged(value: &glib::Value) -> Option<Part> {
    Part::from_key(&value.get::<String>().ok()?)
}

/// The parts in `place` a drop lands among: the part being dragged is not
/// one of them, since it leaves before it lands, and neither is the name an
/// empty place shows.
fn neighbours(place: &gtk::Box, part: Part) -> Vec<gtk::Widget> {
    crate::ui::children(place)
        .filter(|widget| widget.has_css_class("header-chip") && widget.widget_name() != part.key())
        .collect()
}

/// Where `part` dropped at `x` in `place` goes, as an index among the parts
/// it lands among, and which of them shows the line, on which edge.
fn landing(place: &gtk::Box, part: Part, x: f64) -> (usize, Option<(gtk::Widget, drop_line::Edge)>) {
    let parts = neighbours(place, part);
    let middle = |widget: &gtk::Widget| {
        widget.compute_bounds(place).map_or(0.0, |bounds| f64::from(bounds.x() + bounds.width() / 2.0))
    };
    let at = parts.iter().filter(|widget| middle(widget) < x).count();
    let line = match (parts.get(at), at.checked_sub(1).and_then(|before| parts.get(before))) {
        (Some(next), _) => Some((next.clone(), drop_line::Edge::Before)),
        (None, Some(last)) => Some((last.clone(), drop_line::Edge::After)),
        (None, None) => None,
    };
    (at, line)
}

fn mark_landing(place: &gtk::Box, part: Part, x: f64) {
    unmark(place);
    if let (_, Some((widget, edge))) = landing(place, part, x) {
        drop_line::mark(&widget, Some(edge));
    }
}

fn unmark(place: &gtk::Box) {
    for widget in crate::ui::children(place) {
        drop_line::mark(&widget, None);
    }
}

/// What the header card's picture is a chart of, so its parts and the
/// strip read the same resolution.
const SAMPLE_TIMEFRAME: omacharts_engine::Timeframe = omacharts_engine::Timeframe::hours(1);

/// Crude oil's May 2020 contract by the hour, from the Sunday evening open
/// to the hour it settled below zero on 20 April, the day before expiry:
/// open, high, low, close. The open, the settlement and the low are the
/// record's; the hours between are drawn, not a feed's prints: two bounces
/// that each give their gain back, so the slide reads as a market's rather
/// than a ramp, and the plunge as the break it was. A sample worth a second
/// look, where a random walk was one nobody looked at.
const OIL: [(f64, f64, f64, f64); 21] = [
    (17.73, 17.85, 17.01, 17.10),
    (17.10, 17.18, 16.20, 16.30),
    (16.30, 16.45, 15.34, 15.50),
    (15.50, 16.50, 15.38, 16.40),
    (16.40, 17.26, 16.33, 17.20),
    (17.20, 17.33, 16.16, 16.30),
    (16.30, 16.42, 15.21, 15.30),
    (15.30, 15.38, 14.20, 14.30),
    (14.30, 14.45, 13.14, 13.30),
    (13.30, 13.40, 12.28, 12.40),
    (12.40, 13.46, 12.33, 13.40),
    (13.40, 14.53, 13.26, 14.40),
    (14.40, 15.22, 14.31, 15.10),
    (15.10, 15.18, 13.90, 14.00),
    (14.00, 14.15, 12.64, 12.80),
    (12.80, 12.90, 11.38, 11.50),
    (11.50, 11.56, 10.13, 10.20),
    (10.20, 11.03, 10.06, 10.90),
    (10.90, 11.02, 7.51, 7.60),
    (7.60, 7.68, 3.88, 4.10),
    (4.10, 4.21, -40.32, -37.63),
];

/// A chart behind the header's parts, drawn the way the charts are: the
/// gridlines and the last price line when those are on, and [`OIL`]'s
/// candles in the charts' own colours and bodies, from `top`, under the
/// header so the parts read clear of them.
fn draw_chart(cr: &gtk::cairo::Context, width: f64, height: f64, top: f64, look: &ChartLook, theme: &Theme) {
    if look.grid {
        cr.set_line_width(1.0);
        colors::set_source(cr, &theme.ui.grid);
        let mut y = 44.5;
        while y < height {
            cr.move_to(0.0, y);
            cr.line_to(width, y);
            y += 44.0;
        }
        let mut x = 96.5;
        while x < width {
            cr.move_to(x, 0.0);
            cr.line_to(x, height);
            x += 96.0;
        }
        let _ = cr.stroke();
    }

    // Spread across the card, its bodies most of each step wide as a
    // chart's are when zoomed in, about two steps clear of the right edge:
    // the room a chart leaves there for the bars to come.
    let (left, right) = (12.0, 56.0);
    let step = ((width - left - right) / OIL.len() as f64).floor();
    let half = (step * 0.36).floor();
    // The hours before the plunge take the top half, and the plunge the
    // rest: to scale, the plunge is three times their fall, and they were
    // a slope of crumbs. There is no axis to give the game away.
    let range = |bars: &[(f64, f64, f64, f64)]| bars.iter().fold((f64::MAX, f64::MIN), |(lo, hi), b| (lo.min(b.2), hi.max(b.1)));
    let ((before, hi), (lo, _)) = (range(&OIL[..OIL.len() - 1]), range(&OIL));
    let bottom = height - 8.0;
    let middle = (top + bottom) / 2.0;
    let y = |price: f64| match price >= before {
        true => top + (hi - price) / (hi - before) * (middle - top),
        false => middle + (before - price) / (before - lo) * (bottom - middle),
    };

    let scheme = look.bars.scheme(theme);
    cr.set_line_width(1.0);
    for (i, (open, high, low, close)) in OIL.iter().enumerate() {
        let x = (width - right - half - step * (OIL.len() - 1 - i) as f64).round();
        let direction = if close >= open { Direction::Up } else { Direction::Down };
        colors::set_source(cr, scheme.edge(direction));
        cr.move_to(x + 0.5, y(*high).round());
        cr.line_to(x + 0.5, y(*low).round());
        let _ = cr.stroke();
        let (body_top, body_bottom) = (y(open.max(*close)).round(), y(open.min(*close)).round());
        let body_h = (body_bottom - body_top).max(1.0);
        let fill = scheme.body(direction);
        // Filled on whole pixels, the edge on half ones so it stays crisp;
        // a hollow body is its edge alone.
        if colors::rgba(fill).3 > 0.0 {
            cr.rectangle(x - half, body_top, 2.0 * half + 1.0, body_h);
            colors::set_source(cr, fill);
            let _ = cr.fill();
        }
        if body_h > 2.0 {
            cr.rectangle(x - half + 0.5, body_top + 0.5, 2.0 * half, body_h - 1.0);
            colors::set_source(cr, scheme.edge(direction));
            let _ = cr.stroke();
        }
    }

    if look.price_line
        && let Some((open, .., close)) = OIL.last()
    {
        let direction = if close >= open { Direction::Up } else { Direction::Down };
        cr.save().ok();
        cr.set_dash(&[3.0, 3.0], 0.0);
        colors::set_source_alpha(cr, scheme.outline(direction), 0.7);
        let at = y(*close).round() + 0.5;
        cr.move_to(0.0, at);
        cr.line_to(width, at);
        let _ = cr.stroke();
        cr.restore().ok();
    }
}

/// Every header card's picture again, after a line was switched: the
/// picture draws the lines as the charts do.
fn repaint_cards() {
    CARDS.with(|cards| {
        for card in cards.borrow().iter().filter_map(Weak::upgrade) {
            card.chart.queue_draw();
        }
    });
}

/// A line changed from somewhere else — the key, the chart menu, a
/// command — so every switch on screen reads what is stored now.
pub fn charts_changed() {
    let live: Vec<(adw::SwitchRow, Rc<dyn Fn() -> bool>)> = SWITCHES.with(|switches| {
        let mut switches = switches.borrow_mut();
        switches.retain(|(row, _)| row.upgrade().is_some());
        switches.iter().filter_map(|(row, read)| Some((row.upgrade()?, read.clone()))).collect()
    });
    for (row, read) in live {
        let on = read();
        // Only when it differs: setting it runs the handler, which stores it
        // and repaints the charts again.
        if row.is_active() != on {
            row.set_active(on);
        }
    }
    repaint_cards();
}

/// A line a look draws or not: its row's title, whether the look draws
/// it, and the edit that switches it.
pub(crate) type Line = (&'static str, fn(&ChartLook) -> bool, fn(bool) -> LookEdit);

pub(crate) const LINES: [Line; 2] = [
    ("Gridlines", |look| look.grid, LookEdit::Grid),
    ("Last price line", |look| look.price_line, LookEdit::PriceLine),
];

/// The look's lines, switched where the rule says, then the crosshair
/// across Follow charts, which is every chart's.
fn lines_group(window: Rc<Window>, store: Rc<Store>, on_crosshair: Rc<dyn Fn()>) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::new();
    group.set_title("Lines");
    for (title, drawn, edit) in LINES {
        let looked_at = window.clone();
        let read: Rc<dyn Fn() -> bool> = Rc::new(move || drawn(&scope_rule::look(&looked_at)));
        let row = switch_row(&group, title, read.clone());
        let window = window.clone();
        row.connect_active_notify(move |row| {
            // Moved to what is already there, by a change of scope: not an
            // edit, and under Local one would hand the chart a copy.
            if row.is_active() != read() {
                scope_rule::change(&window, edit(row.is_active()));
                repaint_cards();
            }
        });
    }
    let stored = store.clone();
    let read: Rc<dyn Fn() -> bool> = Rc::new(move || stored.setting_bool(SETTING_SYNC_CROSSHAIR, true));
    let row = switch_row(&group, "Crosshair across Follow charts", read.clone());
    // Between charts by nature, so never one chart's own.
    row.set_widget_name(scope_rule::GLOBAL_ONLY);
    row.connect_active_notify(move |row| {
        if row.is_active() != read() {
            store.set_setting_bool(SETTING_SYNC_CROSSHAIR, row.is_active());
            on_crosshair();
        }
    });
    group
}

/// A switch in `group` showing what `read` says, kept to it by
/// [`charts_changed`].
fn switch_row(group: &adw::PreferencesGroup, title: &str, read: Rc<dyn Fn() -> bool>) -> adw::SwitchRow {
    let row = adw::SwitchRow::new();
    row.set_title(title);
    row.set_active(read());
    group.add(&row);
    SWITCHES.with(|switches| switches.borrow_mut().push((row.downgrade(), read)));
    row
}
