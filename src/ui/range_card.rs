//! The two cards that change how much of the past a chart shows: Ctrl+G's
//! range and the resolution a digit on the chart starts.
//!
//! Both are the picker's frame with the list folded away: a centred title
//! whose ⓘ holds the syntax and a box. Nothing under the box comes or goes
//! with what is typed, so the card keeps one shape; the list drops down
//! over it when asked for. Which chart it acts on is said by the ring
//! around that chart, not in words.
//!
//! The range card has two more things. The resolution it will show at sits
//! under the box at its right, `1D auto`, and Tab reaches it; it offers only the
//! resolutions the range can be shown at. From and To, under the box, are
//! the first and last day the range lands on, and Tab goes into them to set
//! either by hand, which writes the range back into the box as a slice,
//! `2026/03/21:`.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use adw::prelude::*;
use chrono::{Datelike, Days, NaiveDate};
use gtk::gdk;
use gtk::glib;
use omacharts_engine::bars::Unit;
use omacharts_engine::range::End;
use omacharts_engine::{Range, Timeframe};

use crate::ui::pick_frame::{Over, PickFrame};

/// As wide as two dates side by side and no wider.
const WIDTH: i32 = 320;

/// How far below the card's content the month hangs: the card's shadow
/// margin, which the content stops short of, and a 6px gap.
const MONTH_DROP: i32 = 32;

/// What a range comes to at a resolution, for the chip and its list.
pub struct Plan {
    pub timeframe: Timeframe,
    /// Picked by the window rather than set by hand.
    pub auto: bool,
    /// The range can be shown at it: the feed has the symbol at it and
    /// reaches back far enough, and the bars are a chart rather than a
    /// candle or two, or more than a chart spreads across. One that does
    /// not is not offered, rather than offered with a warning: a year's
    /// candle for today is no choice anybody means.
    pub fits: bool,
}

type PlanFn = Rc<dyn Fn(Range, Option<Timeframe>) -> Plan>;
type OnDay = Rc<dyn Fn(NaiveDate)>;

/// What the window tells a card when it opens.
pub struct Context {
    pub today: NaiveDate,
    /// The first and last day on screen now.
    pub showing: (NaiveDate, NaiveDate),
    /// The first day the series holds, for a range that starts with it.
    pub first: NaiveDate,
    /// The resolution the chart has now.
    pub current: Timeframe,
    /// The resolutions the strip offers, which the arrows step through.
    pub ladder: Vec<Timeframe>,
    pub plan: PlanFn,
    /// Show a range, at a resolution set by hand or at the window's pick.
    pub apply: Rc<dyn Fn(Range, Option<Timeframe>)>,
    /// Change the resolution and nothing else.
    pub resolution: Rc<dyn Fn(Timeframe)>,
}

/// The resolution a range will show at, in a key-sized chip pinned in the
/// row under the box: `1D`, or `1W auto` when it is the window's pick.
struct Chip {
    chip: gtk::Box,
    key: gtk::Label,
    auto: gtk::Label,
    caret: gtk::Label,
}

impl Chip {
    fn new() -> Chip {
        let key = gtk::Label::new(None);
        let auto = gtk::Label::new(Some("auto"));
        auto.add_css_class("range-auto");
        let caret = gtk::Label::new(Some("▾"));
        caret.add_css_class("range-caret");
        let chip = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        chip.add_css_class("range-chip");
        chip.append(&key);
        chip.append(&auto);
        chip.append(&caret);
        Chip { chip, key, auto, caret }
    }

    fn show(&self, plan: &Plan) {
        self.key.set_text(&plan.timeframe.key());
        self.auto.set_visible(plan.auto);
        self.inert(false);
    }

    /// Nothing typed and nothing chosen: just `auto`, and nothing to open.
    /// The range picks the resolution once there is one, so there is
    /// nothing yet to choose between.
    fn blank(&self) {
        self.key.set_text("auto");
        self.auto.set_visible(false);
        self.inert(true);
    }

    fn inert(&self, inert: bool) {
        set_class(&self.chip, "inert", inert);
        self.caret.set_visible(!inert);
        self.chip.set_focusable(!inert);
        self.chip.set_cursor_from_name(Some(if inert { "default" } else { "pointer" }));
    }

    fn is_inert(&self) -> bool {
        self.chip.has_css_class("inert")
    }
}

/// The units Left and Right walk the chip through, finest first. A quarter
/// is no unit of `Timeframe`'s, it is three months; the dial keeps it apart
/// so `2Q` stays `2Q` while Up and Down count it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Dial {
    Minute,
    Hour,
    Day,
    Week,
    Month,
    Quarter,
    Year,
}

const DIALS: [Dial; 7] = [Dial::Minute, Dial::Hour, Dial::Day, Dial::Week, Dial::Month, Dial::Quarter, Dial::Year];

impl Dial {
    fn of(timeframe: Timeframe) -> (u32, Dial) {
        let dial = match timeframe.unit {
            Unit::Minute => Dial::Minute,
            Unit::Hour => Dial::Hour,
            Unit::Day => Dial::Day,
            Unit::Week => Dial::Week,
            Unit::Month => Dial::Month,
            Unit::Year => Dial::Year,
        };
        (timeframe.count, dial)
    }

    fn timeframe(self, count: u32) -> Timeframe {
        let (count, unit) = match self {
            Dial::Minute => (count, Unit::Minute),
            Dial::Hour => (count, Unit::Hour),
            Dial::Day => (count, Unit::Day),
            Dial::Week => (count, Unit::Week),
            Dial::Month => (count, Unit::Month),
            Dial::Quarter => (count * 3, Unit::Month),
            Dial::Year => (count, Unit::Year),
        };
        Timeframe { count, unit }.normalised()
    }

    fn key(self, count: u32) -> String {
        let unit = match self {
            Dial::Minute => "m",
            Dial::Hour => "h",
            Dial::Day => "D",
            Dial::Week => "W",
            Dial::Month => "M",
            Dial::Quarter => "Q",
            Dial::Year => "Y",
        };
        format!("{count}{unit}")
    }
}

/// The line under the resolution card's box: the resolution typed, in
/// words (`4 hours`), or why the text is not one.
struct Hint {
    words: gtk::Label,
}

impl Hint {
    fn new() -> Hint {
        let words = gtk::Label::new(None);
        words.add_css_class("range-hint");
        words.set_halign(gtk::Align::Center);
        Hint { words }
    }

    fn name(&self, words: &str) {
        self.words.set_text(words);
        set_class(&self.words, "error", false);
    }

    fn error(&self, what: &str) {
        self.words.set_text(what);
        set_class(&self.words, "error", true);
    }

    /// Nothing typed yet: the line keeps its height, empty.
    fn blank(&self) {
        self.name("");
    }
}

/// The ⓘ's help: each example as a key cap, what it means on the right,
/// in titled sections, and under them the keys the card answers to.
fn help(sections: &[(&str, &[(&str, &str)])], keys: &[(&str, &str)]) -> gtk::Widget {
    let column = gtk::Box::new(gtk::Orientation::Vertical, 0);
    column.add_css_class("pick-help");
    for (at, (name, lines)) in sections.iter().enumerate() {
        let group = gtk::Box::new(gtk::Orientation::Vertical, 5);
        group.add_css_class("pick-help-group");
        set_class(&group, "later", at > 0);
        if !name.is_empty() {
            let head = gtk::Label::new(Some(&name.to_uppercase()));
            head.add_css_class("pick-help-section");
            head.set_xalign(0.0);
            group.append(&head);
        }
        for (examples, what) in lines.iter() {
            let caps = gtk::Box::new(gtk::Orientation::Horizontal, 4);
            for example in examples.split_whitespace() {
                let cap = gtk::Label::new(Some(example));
                cap.add_css_class("pick-help-key");
                caps.append(&cap);
            }
            let what = gtk::Label::new(Some(what));
            what.add_css_class("pick-help-what");
            what.set_xalign(1.0);
            what.set_hexpand(true);
            let row = gtk::Box::new(gtk::Orientation::Horizontal, 16);
            row.append(&caps);
            row.append(&what);
            group.append(&row);
        }
        column.append(&group);
    }
    if !keys.is_empty() {
        let foot = gtk::Box::new(gtk::Orientation::Horizontal, 14);
        foot.add_css_class("pick-help-foot");
        for (key, does) in keys.iter() {
            let cap = gtk::Label::new(Some(key));
            cap.add_css_class("pick-help-cap");
            let does = gtk::Label::new(Some(does));
            let pair = gtk::Box::new(gtk::Orientation::Horizontal, 5);
            pair.append(&cap);
            pair.append(&does);
            foot.append(&pair);
        }
        column.append(&foot);
    }
    column.upcast()
}

/// From or To: a box that takes a day however it is written, `2026/03/21`,
/// `23 nov`, `yesterday`, `mon`, `-6m`, and shows it back as a date when
/// the keyboard leaves it. Up and Down step it a day (Shift, ten), and the
/// calendar in its corner picks one with the pointer.
struct DateCell {
    root: gtk::Box,
    field: gtk::Entry,
    today: gtk::Label,
    /// From takes the first day of what is typed, To the last: `2025` from
    /// 1 January, to 31 December.
    is_from: bool,
    date: Cell<NaiveDate>,
    today_is: Cell<NaiveDate>,
    /// The card is writing the text, not the keyboard.
    quiet: Cell<bool>,
    changed: RefCell<Option<OnDay>>,
    entered: RefCell<Option<Rc<dyn Fn()>>>,
    /// What the month opens under, centred and as wide: the card.
    anchor: RefCell<Option<gtk::Widget>>,
    /// From and To as the card has them, for the month to shade.
    span: Cell<(NaiveDate, NaiveDate)>,
}

impl DateCell {
    fn new(name: &str, is_from: bool) -> Rc<DateCell> {
        let label = gtk::Label::new(Some(name));
        label.add_css_class("range-cell-name");
        label.set_xalign(0.0);
        label.set_hexpand(true);
        let today = gtk::Label::new(Some("today"));
        today.add_css_class("range-today");
        today.set_visible(false);
        let head = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        head.append(&label);
        head.append(&today);

        let field = gtk::Entry::new();
        field.add_css_class("range-date");
        field.set_width_chars(10);
        field.set_max_width_chars(10);
        field.set_secondary_icon_name(Some("x-office-calendar-symbolic"));
        field.set_secondary_icon_tooltip_text(Some("Pick from the month"));

        let root = gtk::Box::new(gtk::Orientation::Vertical, 3);
        root.set_hexpand(true);
        root.append(&head);
        root.append(&field);

        let cell = Rc::new(DateCell {
            root,
            field,
            today,
            is_from,
            date: Cell::new(NaiveDate::default()),
            today_is: Cell::new(NaiveDate::default()),
            quiet: Cell::new(false),
            changed: RefCell::new(None),
            entered: RefCell::new(None),
            anchor: RefCell::new(None),
            span: Cell::new((NaiveDate::default(), NaiveDate::default())),
        });

        // Typed: the range follows as soon as the text reads as a day.
        let weak = Rc::downgrade(&cell);
        cell.field.connect_changed(move |_| {
            if let Some(cell) = weak.upgrade()
                && !cell.quiet.get()
            {
                cell.typed();
            }
        });

        // Up and Down before the text sees them.
        let keys = gtk::EventControllerKey::new();
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        let weak = Rc::downgrade(&cell);
        keys.connect_key_pressed(move |_, key, _, state| {
            let Some(cell) = weak.upgrade() else { return glib::Propagation::Proceed };
            let step = if state.contains(gdk::ModifierType::SHIFT_MASK) { 10 } else { 1 };
            let date = cell.date.get();
            let moved = match key {
                gdk::Key::Up => date.checked_add_days(Days::new(step)),
                gdk::Key::Down => date.checked_sub_days(Days::new(step)),
                _ => return glib::Propagation::Proceed,
            };
            cell.change(moved);
            cell.show_date();
            glib::Propagation::Stop
        });
        cell.field.add_controller(keys);

        let weak = Rc::downgrade(&cell);
        cell.field.connect_icon_release(move |_, _| {
            if let Some(cell) = weak.upgrade() {
                cell.open_calendar();
            }
        });

        let focus = gtk::EventControllerFocus::new();
        let weak = Rc::downgrade(&cell);
        focus.connect_enter(move |_| {
            let Some(cell) = weak.upgrade() else { return };
            let entered = cell.entered.borrow().clone();
            if let Some(entered) = entered {
                entered();
            }
        });
        // Leaving, the text becomes the date it read as.
        let weak = Rc::downgrade(&cell);
        focus.connect_leave(move |_| {
            if let Some(cell) = weak.upgrade() {
                cell.show_date();
            }
        });
        cell.field.add_controller(focus);
        cell
    }

    /// The day the card has the cell on. The text is left alone while the
    /// keyboard is in it, so what is being typed is not written over.
    fn set(&self, date: NaiveDate, today: NaiveDate) {
        self.date.set(date);
        self.today_is.set(today);
        self.today.set_visible(date == today);
        if !self.focused() {
            self.show_date();
        }
    }

    fn show_date(&self) {
        self.quiet.set(true);
        self.field.set_text(&self.date.get().format("%Y/%m/%d").to_string());
        self.quiet.set(false);
        set_class(&self.field, "unread", false);
    }

    fn focused(&self) -> bool {
        let focus = self.field.root().and_then(|root| root.focus());
        focus.is_some_and(|focus| focus.is_ancestor(&self.field) || focus == *self.field.upcast_ref::<gtk::Widget>())
    }

    fn typed(&self) {
        let text = self.field.text();
        match day_of(&text, self.today_is.get(), self.is_from) {
            Some(date) => {
                set_class(&self.field, "unread", false);
                self.change(Some(date));
            }
            None => set_class(&self.field, "unread", !could_become(&text)),
        }
    }

    fn change(&self, date: Option<NaiveDate>) {
        let Some(date) = date.filter(|d| (1800..=2200).contains(&d.year())) else { return };
        let changed = self.changed.borrow().clone();
        if let Some(changed) = changed {
            changed(date);
        }
    }

    /// The month, under the card and as wide, on the cell's day.
    fn open_calendar(self: &Rc<Self>) {
        let anchor = self.anchor.borrow().clone().unwrap_or_else(|| self.field.clone().upcast());
        let popover = gtk::Popover::new();
        popover.add_css_class("pick-drop");
        popover.add_css_class("range-month-pop");
        popover.set_has_arrow(false);
        popover.set_position(gtk::PositionType::Bottom);
        popover.set_parent(&anchor);
        popover.set_pointing_to(Some(&gdk::Rectangle::new(0, anchor.height(), anchor.width(), 1)));
        // Clear of the card's own shadow margin, then a 6px gap.
        popover.set_offset(0, MONTH_DROP);
        let weak = Rc::downgrade(self);
        let closing = popover.downgrade();
        let pick: OnDay = Rc::new(move |day| {
            let Some(cell) = weak.upgrade() else { return };
            cell.change(Some(day));
            cell.show_date();
            if let Some(popover) = closing.upgrade() {
                popover.popdown();
            }
        });
        let month = MonthView::new(self.date.get(), self.span.get(), self.is_from, self.today_is.get(), pick);
        // As wide as the card: its content is the card less the popover's
        // padding, which this one has too.
        month.root.set_size_request(anchor.width(), -1);
        popover.set_child(Some(&month.root));
        let field = self.field.downgrade();
        // The handler holds the view, so it lives as long as the popover: its
        // arrows and title reach it only weakly.
        let view = month.clone();
        popover.connect_closed(move |popover| {
            let _ = &view;
            let (popover, field) = (popover.clone(), field.clone());
            glib::idle_add_local_once(move || {
                popover.unparent();
                if let Some(field) = field.upgrade() {
                    field.grab_focus_without_selecting();
                }
            });
        });
        popover.popup();
        month.focus_day();
    }
}

/// What the month view shows: the days of a month, the months of a year,
/// or a dozen years.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Zoom {
    Days,
    Months,
    Years,
}

/// A month of days for From or To: `October 2026` between two arrows, and
/// the stretch between From and To shaded, the end being set filled and the
/// other ringed. The title zooms out the way calendars do: on the days it
/// shows the months of the year, on the months a dozen years; picking one
/// zooms back in. Our own rather than GTK's calendar, which has a header
/// for the month and another for the year, and no way to shade a range.
struct MonthView {
    root: gtk::Box,
    title: gtk::Button,
    grid: gtk::Grid,
    /// The first of the month on show; its year is the one the months and
    /// the years are drawn around.
    month: Cell<NaiveDate>,
    zoom: Cell<Zoom>,
    span: (NaiveDate, NaiveDate),
    is_from: bool,
    today: NaiveDate,
    pick: OnDay,
}

impl MonthView {
    fn new(day: NaiveDate, span: (NaiveDate, NaiveDate), is_from: bool, today: NaiveDate, pick: OnDay) -> Rc<MonthView> {
        let back = gtk::Button::from_icon_name("pan-start-symbolic");
        back.set_tooltip_text(Some("Back"));
        let on = gtk::Button::from_icon_name("pan-end-symbolic");
        on.set_tooltip_text(Some("On"));
        for button in [&back, &on] {
            button.add_css_class("flat");
            button.add_css_class("range-month-step");
            button.set_valign(gtk::Align::Center);
        }
        let title = gtk::Button::new();
        title.add_css_class("flat");
        title.add_css_class("range-month-title");
        title.set_hexpand(true);
        title.set_halign(gtk::Align::Center);
        let head = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        head.append(&back);
        head.append(&title);
        head.append(&on);
        let rule = gtk::Separator::new(gtk::Orientation::Horizontal);
        let grid = gtk::Grid::new();
        grid.set_column_homogeneous(true);
        grid.set_row_spacing(2);
        let root = gtk::Box::new(gtk::Orientation::Vertical, 8);
        root.add_css_class("range-month");
        root.append(&head);
        root.append(&rule);
        root.append(&grid);
        let view = Rc::new(MonthView {
            root,
            title,
            grid,
            month: Cell::new(day.with_day(1).unwrap_or(day)),
            zoom: Cell::new(Zoom::Days),
            span,
            is_from,
            today,
            pick,
        });
        for (button, ahead) in [(&back, false), (&on, true)] {
            let weak = Rc::downgrade(&view);
            button.connect_clicked(move |_| {
                if let Some(view) = weak.upgrade() {
                    view.step(ahead);
                }
            });
        }
        let weak = Rc::downgrade(&view);
        view.title.connect_clicked(move |_| {
            let Some(view) = weak.upgrade() else { return };
            let out = match view.zoom.get() {
                Zoom::Days => Zoom::Months,
                Zoom::Months | Zoom::Years => Zoom::Years,
            };
            view.zoom.set(out);
            view.draw();
        });
        view.draw();
        view
    }

    /// A month, a year or a dozen years on, as zoomed.
    fn step(self: &Rc<Self>, ahead: bool) {
        let months = match self.zoom.get() {
            Zoom::Days => 1,
            Zoom::Months => 12,
            Zoom::Years => 144,
        };
        let month = self.month.get();
        let moved = match ahead {
            true => month.checked_add_months(chrono::Months::new(months)),
            false => month.checked_sub_months(chrono::Months::new(months)),
        };
        if let Some(moved) = moved.filter(|d| (1800..=2200).contains(&d.year())) {
            self.month.set(moved);
            self.draw();
        }
    }

    /// The keyboard on the day being set, so arrows walk the days and
    /// Enter picks one.
    fn focus_day(&self) {
        let day = if self.is_from { self.span.0 } else { self.span.1 };
        let mut child = self.grid.first_child();
        while let Some(widget) = child {
            if widget.widget_name() == day.to_string() {
                if let Some(button) = widget.first_child() {
                    button.grab_focus();
                }
                return;
            }
            child = widget.next_sibling();
        }
    }

    fn draw(self: &Rc<Self>) {
        while let Some(child) = self.grid.first_child() {
            self.grid.remove(&child);
        }
        match self.zoom.get() {
            Zoom::Days => self.draw_days(),
            Zoom::Months => self.draw_months(),
            Zoom::Years => self.draw_years(),
        }
    }

    fn draw_days(&self) {
        let month = self.month.get();
        self.title.set_label(&month.format("%B %Y").to_string());
        for (col, name) in ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"].iter().enumerate() {
            let label = gtk::Label::new(Some(name));
            label.add_css_class("range-month-name");
            self.grid.attach(&label, col as i32, 0, 1, 1);
        }
        let (low, high) = match self.span.0 <= self.span.1 {
            true => self.span,
            false => (self.span.1, self.span.0),
        };
        let setting = if self.is_from { self.span.0 } else { self.span.1 };
        let lead = u64::from(month.weekday().num_days_from_sunday());
        let Some(first) = month.checked_sub_days(Days::new(lead)) else { return };
        for at in 0..42u64 {
            let Some(day) = first.checked_add_days(Days::new(at)) else { continue };
            let cell = gtk::Box::new(gtk::Orientation::Horizontal, 0);
            cell.add_css_class("range-month-cell");
            cell.set_widget_name(&day.to_string());
            // Only this month's days: the next month's, greyed, carry none.
            if low < high && day.month() == month.month() {
                set_class(&cell, "between", day > low && day < high);
                set_class(&cell, "start", day == low);
                set_class(&cell, "end", day == high);
            }
            let button = gtk::Button::with_label(&day.day().to_string());
            button.add_css_class("flat");
            button.add_css_class("range-month-day");
            button.set_hexpand(true);
            button.set_halign(gtk::Align::Center);
            set_class(&button, "out", day.month() != month.month());
            set_class(&button, "setting", day == setting);
            set_class(&button, "other", day != setting && (day == low || day == high));
            set_class(&button, "today", day == self.today);
            let pick = self.pick.clone();
            button.connect_clicked(move |_| pick(day));
            cell.append(&button);
            self.grid.attach(&cell, (at % 7) as i32, 1 + (at / 7) as i32, 1, 1);
        }
    }

    /// The months of the year on show, three rows of four; the one the day
    /// being set falls in filled.
    fn draw_months(self: &Rc<Self>) {
        let shown = self.month.get();
        self.title.set_label(&shown.year().to_string());
        let setting = if self.is_from { self.span.0 } else { self.span.1 };
        for month in 1..=12u32 {
            let Some(first) = NaiveDate::from_ymd_opt(shown.year(), month, 1) else { continue };
            let button = self.zoom_button(&first.format("%b").to_string(), first, Zoom::Days);
            set_class(&button, "setting", setting.year() == shown.year() && setting.month() == month);
            set_class(&button, "today", self.today.year() == shown.year() && self.today.month() == month);
            let at = (month - 1) as i32;
            self.grid.attach(&button, at % 4, at / 4, 1, 1);
        }
    }

    /// A dozen years around the one on show, three rows of four.
    fn draw_years(self: &Rc<Self>) {
        let shown = self.month.get();
        let first = shown.year() - shown.year().rem_euclid(12);
        self.title.set_label(&format!("{first} – {}", first + 11));
        let setting = if self.is_from { self.span.0 } else { self.span.1 };
        for at in 0..12 {
            let year = first + at;
            let Some(day) = NaiveDate::from_ymd_opt(year, shown.month(), 1) else { continue };
            let button = self.zoom_button(&year.to_string(), day, Zoom::Months);
            set_class(&button, "setting", setting.year() == year);
            set_class(&button, "today", self.today.year() == year);
            self.grid.attach(&button, at % 4, at / 4, 1, 1);
        }
    }

    /// A month or a year to pick, which zooms in on it.
    fn zoom_button(self: &Rc<Self>, label: &str, to: NaiveDate, zoom: Zoom) -> gtk::Button {
        let button = gtk::Button::with_label(label);
        button.add_css_class("flat");
        button.add_css_class("range-month-pick");
        button.set_hexpand(true);
        let weak = Rc::downgrade(self);
        button.connect_clicked(move |_| {
            if let Some(view) = weak.upgrade() {
                view.month.set(to);
                view.zoom.set(zoom);
                view.draw();
            }
        });
        button
    }
}

/// The day From (`is_from`) or To takes from what is typed in it: any one
/// end a slice takes, `23 nov`, `2025/03`, `-6m`, and From its first day,
/// To its last.
fn day_of(text: &str, today: NaiveDate, is_from: bool) -> Option<NaiveDate> {
    let end = End::parse(text)?;
    match is_from {
        true => end.first(today),
        false => end.last(today),
    }
}

/// Ctrl+G.
pub struct RangeCard {
    frame: Rc<PickFrame>,
    /// The ranges the list holds, in its order.
    shown: RefCell<Vec<Range>>,
    from: Rc<DateCell>,
    to: Rc<DateCell>,
    chip: Chip,
    context: RefCell<Option<Context>>,
    /// The resolution set by hand on the chip, if it has been.
    forced: Cell<Option<Timeframe>>,
    /// The resolution the chip shows, for the wheel to step from.
    showing_tf: Cell<Option<Timeframe>>,
    /// The count and unit the arrows last set, while they are what
    /// `forced` holds; see [`Dial`].
    dial: Cell<Option<(u32, Dial)>>,
    /// The range the chip counts bars over.
    counting: Cell<Option<Range>>,
    typed_tf: RefCell<String>,
    /// Tab has been into the dates, so they are the answer even before
    /// anything is typed.
    touched: Cell<bool>,
}

impl RangeCard {
    pub fn new() -> Rc<RangeCard> {
        let frame = PickFrame::new("");
        frame.set_bare(
            "Go to",
            help(
                &[
                    (
                        "Up to today",
                        &[
                            ("6m 3y", "the last 6 months, 3 years"),
                            ("ytd qtd mtd", "this year, quarter, month"),
                            ("2ytd", "this year and last"),
                            ("mon: 10/12:", "since Monday, 12 Oct"),
                            ("all", "the whole history"),
                        ],
                    ),
                    (
                        "A set period",
                        &[
                            ("2025", "all of 2025"),
                            ("2025/03", "March 2025"),
                            ("2025/03/12 12 mar", "one day; no year is this year"),
                            ("mon 2mon", "this Monday, the one before"),
                            ("2025/03:2025/06", "March to June 2025"),
                            ("-6m:-1m", "6 months to 1 month ago"),
                        ],
                    ),
                ],
                &[("Space", "common ranges")],
            ),
        );
        frame.popover.add_css_class("range-goto");
        frame.drop_list();
        frame.set_width(WIDTH);

        let chip = Chip::new();
        chip.chip.set_focusable(true);
        chip.chip.set_focus_on_click(true);
        chip.chip.set_cursor_from_name(Some("pointer"));
        // The resolution on a row of its own under the box, at its right:
        // in the box's corner, a long slice ran under it.
        chip.chip.set_halign(gtk::Align::End);
        let resolution = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        resolution.add_css_class("range-chip-row");
        resolution.append(&chip.chip);
        chip.chip.set_hexpand(true);
        frame.append(&resolution);

        let from = DateCell::new("From", true);
        let to = DateCell::new("To", false);
        let arrow = gtk::Label::new(Some("→"));
        arrow.add_css_class("range-arrow");
        arrow.set_valign(gtk::Align::End);
        let dates = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        dates.add_css_class("range-dates");
        dates.append(&from.root);
        dates.append(&arrow);
        dates.append(&to.root);
        frame.append(&dates);

        let card = Rc::new(RangeCard {
            frame,
            shown: RefCell::new(Vec::new()),
            from,
            to,
            chip,
            context: RefCell::new(None),
            forced: Cell::new(None),
            showing_tf: Cell::new(None),
            dial: Cell::new(None),
            counting: Cell::new(None),
            typed_tf: RefCell::new(String::new()),
            touched: Cell::new(false),
        });
        for cell in [&card.from, &card.to] {
            *cell.anchor.borrow_mut() = card.frame.popover.child();
        }
        card.wire();
        card
    }

    fn wire(self: &Rc<Self>) {
        let weak = Rc::downgrade(self);
        self.frame.entry.connect_search_changed(move |_| {
            if let Some(card) = weak.upgrade() {
                card.refresh();
            }
        });

        // Space or ? on an empty box, or Down, drops the list down.
        let keys = gtk::EventControllerKey::new();
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        let weak = Rc::downgrade(self);
        keys.connect_key_pressed(move |_, key, _, state| {
            let Some(card) = weak.upgrade() else { return glib::Propagation::Proceed };
            if state.intersects(gdk::ModifierType::CONTROL_MASK | gdk::ModifierType::ALT_MASK) {
                return glib::Propagation::Proceed;
            }
            let empty = card.frame.entry.text().is_empty();
            let shown = card.frame.list_shown();
            match key {
                gdk::Key::space | gdk::Key::question if empty => {
                    card.frame.show_list(!shown);
                    glib::Propagation::Stop
                }
                gdk::Key::Down if !shown => {
                    card.frame.show_list(true);
                    glib::Propagation::Stop
                }
                _ => glib::Propagation::Proceed,
            }
        });
        self.frame.entry.add_controller(keys);

        // Tab walks box → resolution → From → To and round again. Said here
        // because the window's own keys see Tab before GTK's focus chain does.
        // The same for Enter from a date or the chip, which shows the range;
        // in the chip's list it is the row's.
        let keys = gtk::EventControllerKey::new();
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        let weak = Rc::downgrade(self);
        keys.connect_key_pressed(move |_, key, _, state| {
            let Some(card) = weak.upgrade() else { return glib::Propagation::Proceed };
            let ctrl = state.contains(gdk::ModifierType::CONTROL_MASK);
            if crate::ui::dialogs::is_enter(key) && !ctrl && !card.frame.entry.has_focus() && !card.in_box() {
                let focus = card.chip.chip.root().and_then(|root| root.focus());
                let chip: &gtk::Widget = card.chip.chip.upcast_ref();
                match focus {
                    Some(focus) if &focus != chip && focus.is_ancestor(chip) => return glib::Propagation::Proceed,
                    _ => card.commit(),
                }
                return glib::Propagation::Stop;
            }
            let back = match key {
                gdk::Key::Tab if !state.contains(gdk::ModifierType::SHIFT_MASK) => false,
                gdk::Key::Tab | gdk::Key::ISO_Left_Tab => true,
                _ => return glib::Propagation::Proceed,
            };
            card.tab(back);
            glib::Propagation::Stop
        });
        self.frame.popover.add_controller(keys);

        let weak = Rc::downgrade(self);
        self.frame.on_enter(move || {
            if let Some(card) = weak.upgrade() {
                card.commit();
            }
        });

        for cell in [&self.from, &self.to] {
            let weak = Rc::downgrade(self);
            let is_from = Rc::ptr_eq(cell, &self.from);
            *cell.changed.borrow_mut() = Some(Rc::new(move |date| {
                if let Some(card) = weak.upgrade() {
                    card.set_day(is_from, date);
                }
            }));
            let weak = Rc::downgrade(self);
            *cell.entered.borrow_mut() = Some(Rc::new(move || {
                if let Some(card) = weak.upgrade() {
                    card.frame.show_list(false);
                    if !card.touched.replace(true) {
                        card.refresh();
                    }
                }
            }));
        }

        let keys = gtk::EventControllerKey::new();
        let weak = Rc::downgrade(self);
        keys.connect_key_pressed(move |_, key, _, state| match weak.upgrade() {
            Some(card) => card.chip_key(key, state),
            None => glib::Propagation::Proceed,
        });
        self.chip.chip.add_controller(keys);
        let focus = gtk::EventControllerFocus::new();
        let weak = Rc::downgrade(self);
        focus.connect_enter(move |_| {
            if let Some(card) = weak.upgrade() {
                card.typed_tf.borrow_mut().clear();
                card.frame.show_list(false);
            }
        });
        self.chip.chip.add_controller(focus);
        // A click drops the strip's resolutions down, each with what it
        // makes; the wheel steps along them.
        let click = gtk::GestureClick::new();
        let weak = Rc::downgrade(self);
        click.connect_released(move |_, _, _, _| {
            if let Some(card) = weak.upgrade() {
                card.chip.chip.grab_focus();
                card.open_resolutions();
            }
        });
        self.chip.chip.add_controller(click);
        let scroll = gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::VERTICAL | gtk::EventControllerScrollFlags::DISCRETE);
        let weak = Rc::downgrade(self);
        scroll.connect_scroll(move |_, _, dy| {
            if let Some(card) = weak.upgrade() {
                card.step_resolution(dy < 0.0);
            }
            glib::Propagation::Stop
        });
        self.chip.chip.add_controller(scroll);
    }

    /// The resolutions on the strip, and auto, under the chip.
    fn open_resolutions(self: &Rc<Self>) {
        if self.chip.is_inert() {
            return;
        }
        let Some(range) = self.counting.get() else { return };
        let (plan, ladder, current) = match self.context.borrow().as_ref() {
            Some(context) => (context.plan.clone(), context.ladder.clone(), context.current),
            None => return,
        };
        // Auto, then the chart's own resolution, then the rest of the strip
        // and the common ones off it, shortest first: those the range can be
        // shown at. A day on a strip that starts at an hour still offers 5m.
        let mut rest: Vec<Timeframe> = ladder.into_iter().chain(omacharts_engine::range::COMMON).filter(|tf| *tf != current).collect();
        rest.sort_by_key(|tf| tf.seconds());
        rest.dedup();
        let rest = rest.into_iter().map(Some);
        let picks: Vec<Option<Timeframe>> = [None, Some(current)]
            .into_iter()
            .chain(rest)
            .filter(|pick| pick.is_none_or(|tf| plan(range, Some(tf)).fits))
            .collect();
        let list = gtk::ListBox::new();
        list.set_selection_mode(gtk::SelectionMode::Browse);
        list.add_css_class("navigation-sidebar");
        list.add_css_class("pick-list");
        for pick in &picks {
            let name = match pick {
                Some(tf) => tf.key(),
                None => "auto".to_string(),
            };
            let key = gtk::Label::new(Some(&name));
            key.add_css_class("pick-ticker");
            key.set_xalign(0.0);
            key.set_hexpand(true);
            let row = gtk::ListBoxRow::new();
            row.set_child(Some(&key));
            list.append(&row);
        }
        let named = self.forced.get();
        let at = picks.iter().position(|pick| *pick == named).unwrap_or(0);
        if let Some(row) = list.row_at_index(at as i32) {
            list.select_row(Some(&row));
        }

        let popover = gtk::Popover::new();
        popover.add_css_class("pick-drop");
        popover.set_has_arrow(false);
        popover.set_position(gtk::PositionType::Bottom);
        popover.set_child(Some(&list));
        popover.set_parent(&self.chip.chip);
        let weak = Rc::downgrade(self);
        let closing = popover.downgrade();
        list.connect_row_activated(move |_, row| {
            let Some(card) = weak.upgrade() else { return };
            if let Some(pick) = picks.get(row.index().max(0) as usize) {
                card.forced.set(*pick);
                card.refresh();
            }
            if let Some(popover) = closing.upgrade() {
                popover.popdown();
            }
        });
        // One per click, so each lets go of the chip once it has closed,
        // and hands the keyboard back to it.
        let chip = self.chip.chip.downgrade();
        popover.connect_closed(move |popover| {
            let (popover, chip) = (popover.clone(), chip.clone());
            glib::idle_add_local_once(move || {
                popover.unparent();
                if let Some(chip) = chip.upgrade() {
                    chip.grab_focus();
                }
            });
        });
        popover.popup();
        if let Some(row) = list.row_at_index(at as i32) {
            row.grab_focus();
        }
    }

    /// One along the strip, coarser or finer, from what the chip shows.
    fn step_resolution(&self, coarser: bool) {
        if self.chip.is_inert() {
            return;
        }
        let ladder = match self.context.borrow().as_ref() {
            Some(context) => context.ladder.clone(),
            None => return,
        };
        self.typed_tf.borrow_mut().clear();
        let Some(now) = self.showing_tf.get() else { return };
        let next = match coarser {
            true => ladder.iter().copied().find(|tf| tf.seconds() > now.seconds() && self.fits(*tf)),
            false => ladder.iter().rev().copied().find(|tf| tf.seconds() < now.seconds() && self.fits(*tf)),
        };
        if let Some(next) = next {
            self.forced.set(Some(next));
            self.refresh();
        }
    }

    pub fn open(self: &Rc<Self>, over: &Over, context: Context) {
        *self.context.borrow_mut() = Some(context);
        self.forced.set(None);
        self.touched.set(false);
        self.typed_tf.borrow_mut().clear();
        self.frame.entry.set_text("");
        self.refresh();
        let weak = Rc::downgrade(self);
        self.frame.open(over, move |at| {
            let Some(card) = weak.upgrade() else { return };
            let range = card.shown.borrow().get(at).copied();
            let apply = card.context.borrow().as_ref().map(|c| c.apply.clone());
            if let (Some(range), Some(apply)) = (range, apply) {
                apply(range, card.forced.get());
            }
        });
    }

    /// Whether the keyboard is in the box.
    fn in_box(&self) -> bool {
        let focus = self.frame.entry.root().and_then(|root| root.focus());
        focus.is_some_and(|focus| focus.is_ancestor(&self.frame.entry))
    }

    fn tab(&self, back: bool) {
        // The chip is passed by while it has nothing to choose.
        let ring: Vec<gtk::Widget> = [
            self.frame.entry.clone().upcast(),
            self.chip.chip.clone().upcast(),
            self.from.field.clone().upcast(),
            self.to.field.clone().upcast(),
        ]
        .into_iter()
        .filter(|stop| !(self.chip.is_inert() && stop == self.chip.chip.upcast_ref::<gtk::Widget>()))
        .collect();
        let focus = self.frame.entry.root().and_then(|root| root.focus());
        let at = ring
            .iter()
            .position(|stop| focus.as_ref().is_some_and(|focus| focus == stop || focus.is_ancestor(stop)))
            .unwrap_or(0);
        let next = if back { (at + ring.len() - 1) % ring.len() } else { (at + 1) % ring.len() };
        self.frame.show_list(false);
        ring[next].grab_focus();
        if next == 0 {
            self.frame.entry.set_position(-1);
        }
    }

    /// The days a range lands on: from its first, or the series' first, to
    /// its last, or today.
    fn days(range: Range, context: &Context) -> (NaiveDate, NaiveDate) {
        let today = context.today;
        let from = range.start(today).unwrap_or(context.first);
        let to = range.end(today).map_or(today, |end| end.min(today));
        (from, to)
    }

    /// Bring everything under the box into line with what it holds.
    fn refresh(&self) {
        let text = self.frame.entry.text().to_string();
        crate::ui::range_card::fill(&self.frame.list, &self.shown, &text);

        let context = self.context.borrow();
        let Some(context) = context.as_ref() else { return };
        let today = context.today;
        let parsed = Range::parse(&text).filter(|_| !text.trim().is_empty());
        let touched = self.touched.get();

        let wrong = !text.trim().is_empty() && parsed.is_none() && !could_become(&text);
        let empty = parsed.is_some_and(|range| !range.holds_days(today));
        set_class(&self.frame.entry, "error", wrong || empty);

        let (from, to) = match parsed {
            Some(range) => Self::days(range, context),
            None if touched => (self.from.date.get(), self.to.date.get()),
            None => context.showing,
        };
        self.from.set(from, today);
        self.to.set(to, today);
        for cell in [&self.from, &self.to] {
            cell.span.set((from, to));
        }
        let idle = parsed.is_none() && !touched;
        for cell in [&self.from, &self.to] {
            set_class(&cell.field, "idle", idle);
        }
        set_class(&self.to.field, "error", from > to);

        // What the chip counts: what is typed, or else the days on show.
        let range = parsed.unwrap_or_else(|| slice(from, to, today));
        // A resolution set by hand that this range cannot be shown at goes
        // back to the window's pick.
        if let Some(forced) = self.forced.get()
            && !(context.plan)(range, Some(forced)).fits
        {
            self.forced.set(None);
        }
        let forced = self.forced.get();
        let plan = (context.plan)(range, forced);
        self.counting.set(Some(range));
        // Nothing asked yet: the arrows and the wheel start from the
        // chart's own resolution, not the window's pick for the days on
        // screen.
        match idle && forced.is_none() {
            true => {
                self.showing_tf.set(Some(context.current));
                self.chip.blank();
            }
            false => {
                self.showing_tf.set(Some(plan.timeframe));
                self.chip.show(&plan);
            }
        }
        if let Some((count, dial)) = self.dialed() {
            self.chip.key.set_text(&dial.key(count));
        }
    }

    /// Whether the range the chip counts can be shown at `timeframe`.
    fn fits(&self, timeframe: Timeframe) -> bool {
        let Some(range) = self.counting.get() else { return true };
        match self.context.borrow().as_ref() {
            Some(context) => (context.plan)(range, Some(timeframe)).fits,
            None => true,
        }
    }

    /// A day set by hand in From or To: the box says the slice it makes.
    fn set_day(&self, is_from: bool, date: NaiveDate) {
        let today = match self.context.borrow().as_ref() {
            Some(context) => context.today,
            None => return,
        };
        let date = date.min(today);
        let (from, to) = match is_from {
            true => (date, self.to.date.get()),
            false => (self.from.date.get(), date),
        };
        self.touched.set(true);
        // Keep the text and the cells in step without the cells losing the
        // part they are on: the refresh below reads the new slice back.
        self.frame.entry.set_text(&spelled(from, to, today));
        self.refresh();
    }

    fn chip_key(self: &Rc<Self>, key: gdk::Key, state: gdk::ModifierType) -> glib::Propagation {
        match key {
            gdk::Key::Up | gdk::Key::Down => {
                self.turn(if key == gdk::Key::Up { 1 } else { -1 }, 0);
                glib::Propagation::Stop
            }
            gdk::Key::Left | gdk::Key::Right => {
                self.turn(0, if key == gdk::Key::Right { 1 } else { -1 });
                glib::Propagation::Stop
            }
            gdk::Key::space => {
                self.open_resolutions();
                glib::Propagation::Stop
            }
            // The chart's own resolution, and from there back to auto.
            gdk::Key::BackSpace | gdk::Key::Delete => {
                self.typed_tf.borrow_mut().clear();
                let current = self.context.borrow().as_ref().map(|context| context.current);
                let next = match self.forced.get() {
                    Some(forced) if Some(forced) == current => None,
                    _ => current.filter(|current| self.fits(*current)),
                };
                self.forced.set(next);
                self.refresh();
                glib::Propagation::Stop
            }
            _ if state.intersects(gdk::ModifierType::CONTROL_MASK | gdk::ModifierType::ALT_MASK) => {
                glib::Propagation::Proceed
            }
            _ => match key.to_unicode().filter(|c| c.is_ascii_alphanumeric()) {
                Some(c) => {
                    let mut typed = self.typed_tf.borrow_mut();
                    typed.push(c);
                    // Digits wait for their unit; a unit settles it.
                    if !c.is_ascii_digit() {
                        let found = Timeframe::parse(&typed);
                        typed.clear();
                        drop(typed);
                        if let Some(found) = found.filter(|found| self.fits(*found)) {
                            self.forced.set(Some(found));
                            self.refresh();
                        }
                    }
                    glib::Propagation::Stop
                }
                None => glib::Propagation::Proceed,
            },
        }
    }

    /// The dial, while the resolution set by hand is still the one it made.
    fn dialed(&self) -> Option<(u32, Dial)> {
        self.dial.get().filter(|(count, dial)| self.forced.get() == Some(dial.timeframe(*count)))
    }

    /// Up and Down count the chip's resolution up or down; Left and Right
    /// move it along [`DIALS`], keeping the count where it fits. Anything,
    /// on the strip or not, that the range can be shown at: one it cannot is
    /// stepped over.
    fn turn(&self, count_by: i32, unit_by: i32) {
        let Some((count, dial)) = self.dialed().or_else(|| self.showing_tf.get().map(Dial::of)) else { return };
        self.typed_tf.borrow_mut().clear();
        let mut at = DIALS.iter().position(|d| *d == dial).unwrap_or(0) as i32;
        let mut count = count as i32;
        // Far enough to step over the minutes a year cannot be shown at.
        for _ in 0..120 {
            let next = ((count + count_by).clamp(1, 999), (at + unit_by).clamp(0, DIALS.len() as i32 - 1));
            if next == (count, at) {
                return;
            }
            (count, at) = next;
            let dial = DIALS[at as usize];
            let found = match unit_by {
                0 => Some(count).filter(|count| self.fits(dial.timeframe(*count as u32))),
                _ => self.count_in(dial, count),
            };
            if let Some(count) = found {
                self.dial.set(Some((count as u32, dial)));
                self.forced.set(Some(dial.timeframe(count as u32)));
                self.refresh();
                return;
            }
        }
    }

    /// A count of `dial` the range fits, for Left and Right landing on a
    /// unit: `count` itself, else the nearest the strip has in that unit,
    /// else the nearest at all. From `1D`, a year of bitcoin is too many
    /// `1h` bars, so Left lands on the strip's `4h` rather than passing the
    /// hours by.
    fn count_in(&self, dial: Dial, count: i32) -> Option<i32> {
        let fits = |count: i32| self.fits(dial.timeframe(count as u32));
        if fits(count) {
            return Some(count);
        }
        let ladder = self.context.borrow().as_ref().map(|context| context.ladder.clone()).unwrap_or_default();
        let nearest = |counts: &mut dyn Iterator<Item = i32>| counts.filter(|c| fits(*c)).min_by_key(|c| (c - count).abs());
        let strip = &mut ladder.into_iter().map(Dial::of).filter(|(_, d)| *d == dial).map(|(c, _)| c as i32);
        nearest(strip).or_else(|| nearest(&mut (1..=60)))
    }

    /// Enter with the list folded away.
    fn commit(&self) {
        let text = self.frame.entry.text().trim().to_string();
        let forced = self.forced.get();
        let (apply, resolution, today) = match self.context.borrow().as_ref() {
            Some(context) => (context.apply.clone(), context.resolution.clone(), context.today),
            None => return,
        };
        if text.is_empty() {
            self.frame.close();
            if let Some(forced) = forced {
                glib::idle_add_local_once(move || resolution(forced));
            }
            return;
        }
        // Not a range, or one with no days to show: the box says so in red
        // and the card stays.
        let Some(range) = Range::parse(&text).filter(|range| range.holds_days(today)) else { return };
        self.frame.close();
        glib::idle_add_local_once(move || apply(range, forced));
    }
}

/// A digit on the chart: the resolution, the same card with nothing but the
/// box and what the resolution comes to over what is on screen.
pub struct ResolutionCard {
    frame: Rc<PickFrame>,
    hint: Hint,
    context: RefCell<Option<Context>>,
}

impl ResolutionCard {
    pub fn new() -> Rc<ResolutionCard> {
        let frame = PickFrame::new("");
        frame.set_bare(
            "Change resolution",
            help(
                &[(
                    "",
                    &[
                        ("15 90", "minutes"),
                        ("1h 4h", "hours"),
                        ("1D 1W", "days, weeks"),
                        ("1M 1Q 1Y", "months, quarters, years"),
                    ],
                )],
                &[],
            ),
        );
        frame.drop_list();
        frame.set_width(WIDTH);
        let hint = Hint::new();
        frame.append(&hint.words);
        let card = Rc::new(ResolutionCard { frame, hint, context: RefCell::new(None) });

        let weak = Rc::downgrade(&card);
        card.frame.entry.connect_search_changed(move |_| {
            if let Some(card) = weak.upgrade() {
                card.refresh();
            }
        });
        let weak = Rc::downgrade(&card);
        card.frame.on_enter(move || {
            let Some(card) = weak.upgrade() else { return };
            let Some(timeframe) = Timeframe::parse(&card.frame.entry.text()) else { return };
            let Some(resolution) = card.context.borrow().as_ref().map(|c| c.resolution.clone()) else { return };
            card.frame.close();
            glib::idle_add_local_once(move || resolution(timeframe));
        });
        card
    }

    /// Open with `start`, the digit that was typed, already in the box.
    pub fn open(self: &Rc<Self>, over: &Over, context: Context, start: &str) {
        *self.context.borrow_mut() = Some(context);
        self.frame.entry.set_text(start);
        self.refresh();
        self.frame.open(over, |_| {});
    }

    fn refresh(&self) {
        let text = self.frame.entry.text().to_string();
        let parsed = Timeframe::parse(&text);
        let wrong = !text.trim().is_empty() && parsed.is_none();
        set_class(&self.frame.entry, "error", wrong);
        // The resolution in words and no count: counted over the days on
        // screen, `1q` read "1 bar", which looked like the whole history.
        match parsed {
            Some(timeframe) => self.hint.name(&timeframe.description()),
            None if wrong => self.hint.error("not a resolution"),
            None => self.hint.blank(),
        }
    }
}

/// From `from` to `to` as typed: open at the end when `to` is today, so it
/// stays live.
fn slice(from: NaiveDate, to: NaiveDate, today: NaiveDate) -> Range {
    Range::Slice(Some(End::Day(from)), (to < today).then_some(End::Day(to)))
}

/// The slice From and To make, as the card writes it into the box: the
/// days the way the cells show them, less the year when it is this one,
/// `03/21:`, which reads as the same day and keeps the text short of the
/// chip. Open at the end while To is today, so it stays live.
fn spelled(from: NaiveDate, to: NaiveDate, today: NaiveDate) -> String {
    let day = |day: NaiveDate| match day.year() == today.year() {
        true => day.format("%m/%d").to_string(),
        false => day.format("%Y/%m/%d").to_string(),
    };
    let end = if to < today { day(to) } else { String::new() };
    format!("{}:{end}", day(from))
}

/// Text that is not a range yet but could be with more keys: `20` on the
/// way to `2025`, `2026-` to a month, `y` to `ytd`, `12 n` to `12 nov`.
/// Only text no key can save turns the box red.
fn could_become(text: &str) -> bool {
    const MONTHS: [&str; 12] = [
        "january", "february", "march", "april", "may", "june", "july", "august", "september", "october", "november",
        "december",
    ];
    const WORDS: [&str; 19] = [
        "ytd", "mtd", "qtd", "wtd", "year", "month", "week", "day", "all", "max", "today", "yesterday", "monday",
        "tuesday", "wednesday", "thursday", "friday", "saturday", "sunday",
    ];
    let text = text.trim().to_lowercase();
    // Each run of letters is on its way to a word the grammar knows; one
    // with digits after it can only be a month's name, `mar 12`.
    let mut rest = text.as_str();
    while let Some(start) = rest.find(|c: char| c.is_ascii_alphabetic()) {
        let after = &rest[start..];
        let len = after.find(|c: char| !c.is_ascii_alphabetic()).unwrap_or(after.len());
        let (word, tail) = after.split_at(len);
        let scope = tail.split([':', '.']).next().unwrap_or_default();
        let known = match scope.contains(|c: char| c.is_ascii_digit()) {
            true => MONTHS.iter().any(|month| month.starts_with(word)),
            false => MONTHS.iter().chain(WORDS.iter()).any(|known| known.starts_with(word)),
        };
        if !known {
            return false;
        }
        rest = tail;
    }
    // And the rest is numbers and what goes between them.
    text.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '/' | ':' | '.' | ' ' | ','))
}

fn set_class(widget: &impl IsA<gtk::Widget>, class: &str, on: bool) {
    match on {
        true => widget.add_css_class(class),
        false => widget.remove_css_class(class),
    }
}

/// What to type for `range`, in lower case where that means the same thing:
/// `ytd`, `all`. Not `3M`, whose capital is whole months rather than three
/// months back.
fn typed(range: Range) -> String {
    let key = range.key();
    let lower = key.to_lowercase();
    match Range::parse(&lower) == Some(range) {
        true => lower,
        false => key,
    }
}

/// The ranges `query` could mean, one row each: what to type for it, what
/// it is, and the day it starts.
pub fn fill(rows: &gtk::ListBox, shown: &RefCell<Vec<Range>>, query: &str) {
    while let Some(child) = rows.first_child() {
        rows.remove(&child);
    }
    let today = chrono::Local::now().date_naive();
    let ranges = Range::suggest(query, today);
    for range in &ranges {
        let key = gtk::Label::new(Some(&typed(*range)));
        key.add_css_class("pick-ticker");
        key.set_xalign(0.0);
        key.set_width_chars(8);

        let what = gtk::Label::new(Some(&range.description()));
        what.add_css_class("pick-name");
        what.set_xalign(0.0);
        what.set_hexpand(true);
        what.set_ellipsize(gtk::pango::EllipsizeMode::End);

        let from = range.start(today).map(|start| start.format("%-d %b %Y").to_string()).unwrap_or_default();
        let from = gtk::Label::new(Some(&from));
        from.add_css_class("pick-venue");
        from.set_valign(gtk::Align::Center);

        let row_box = gtk::Box::new(gtk::Orientation::Horizontal, 10);
        row_box.append(&key);
        row_box.append(&what);
        row_box.append(&from);
        let row = gtk::ListBoxRow::new();
        row.set_child(Some(&row_box));
        rows.append(&row);
    }
    *shown.borrow_mut() = ranges;
    if let Some(first) = rows.row_at_index(0) {
        rows.select_row(Some(&first));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suggestions_are_spelled_in_lower_case_where_it_means_the_same() {
        let today = NaiveDate::from_ymd_opt(2026, 10, 27).unwrap();
        let ranges = Range::suggest("", today);
        let keys: Vec<String> = ranges.iter().copied().map(typed).collect();
        assert!(keys.iter().any(|k| k == "ytd"), "{keys:?}");
        assert!(keys.iter().all(|k| *k == k.to_lowercase()), "{keys:?}");
        // Each one, typed as shown, is the range it stands for.
        for (key, range) in keys.iter().zip(&ranges) {
            assert_eq!(Range::parse(key), Some(*range), "{key}");
        }
    }

    #[test]
    fn half_typed_text_is_not_an_error() {
        for text in ["2", "20", "2026-", "2026-0", "2026/0", "2026/03/1", "2025:", "2025.", "2025..20", "-6m.", "-2", "y", "2yt", "a", "m", "2mo", "th", "12 n", "mar 1", "10/", "yest", "mon:1"] {
            assert!(could_become(text), "{text}");
        }
        for text in ["6x", "2025q4", "zz", "mon2", "12 x"] {
            assert!(!could_become(text), "{text}");
        }
    }

    #[test]
    fn the_dial_counts_quarters_as_three_months() {
        assert_eq!(Dial::Quarter.timeframe(2), Timeframe::months(6));
        assert_eq!(Dial::Quarter.key(2), "2Q");
        assert_eq!(Dial::Minute.timeframe(60), Timeframe::hours(1), "normalised for the chart");
        assert_eq!(Dial::Minute.key(60), "60m", "but shown as dialled");
        assert_eq!(Dial::of(Timeframe::days(3)), (3, Dial::Day));
    }

    #[test]
    fn from_takes_the_first_day_of_what_is_typed_and_to_the_last() {
        let today = NaiveDate::from_ymd_opt(2026, 10, 29).unwrap_or_default();
        let on = |y, m, d| NaiveDate::from_ymd_opt(y, m, d);
        assert_eq!(day_of("23 nov", today, true), on(2025, 11, 23));
        assert_eq!(day_of("yesterday", today, false), on(2026, 10, 28));
        assert_eq!(day_of("2026/03/21", today, true), on(2026, 3, 21));
        assert_eq!(day_of("2025", today, true), on(2025, 1, 1));
        assert_eq!(day_of("2025", today, false), on(2025, 12, 31));
        assert_eq!(day_of("2025/03", today, false), on(2025, 3, 31));
        assert_eq!(day_of("mon", today, true), on(2026, 10, 26));
        assert_eq!(day_of("23 n", today, true), None);
    }

    #[test]
    fn a_slice_set_by_hand_stays_live_to_today() {
        let today = NaiveDate::from_ymd_opt(2026, 10, 20).unwrap_or_default();
        let from = NaiveDate::from_ymd_opt(2026, 3, 21).unwrap_or_default();
        assert_eq!(slice(from, today, today).key(), "2026-03-21:");
        let to = NaiveDate::from_ymd_opt(2026, 9, 30).unwrap_or_default();
        assert_eq!(slice(from, to, today).key(), "2026-03-21:2026-09-30");
    }

    #[test]
    fn the_box_spells_this_year_without_it() {
        let today = NaiveDate::from_ymd_opt(2026, 10, 20).unwrap_or_default();
        let on = |y, m, d| NaiveDate::from_ymd_opt(y, m, d).unwrap_or_default();
        assert_eq!(spelled(on(2026, 3, 21), today, today), "03/21:");
        assert_eq!(spelled(on(2026, 1, 1), on(2026, 9, 30), today), "01/01:09/30");
        assert_eq!(spelled(on(2025, 11, 3), on(2026, 2, 1), today), "2025/11/03:02/01");
        // Each reads back as the days it was written from.
        for (from, to) in [(on(2026, 3, 21), today), (on(2026, 1, 1), on(2026, 9, 30)), (on(2025, 11, 3), on(2026, 2, 1))] {
            let range = Range::parse(&spelled(from, to, today)).unwrap_or(Range::All);
            assert_eq!(range.start(today), Some(from));
            assert_eq!(range.end(today).unwrap_or(today), to);
        }
    }
}
