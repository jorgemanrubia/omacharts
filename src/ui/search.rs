//! Find a symbol by typing.
//!
//! The whole inventory is already in memory, so every keystroke re-runs the
//! search and rebuilds the list. No debounce, no spinner, no network — which
//! is the point: it should feel like the Omarchy launcher, not like a web
//! autocomplete.
//!
//! One instance serves every place a symbol is picked — charting it, adding it
//! to a watchlist section — by swapping the handler at presentation time.
//! Identical behaviour everywhere, and the index is only built once.
//!
//! On a chart it also takes a resolution. Typing on a chart opens this box
//! whatever the first key was, because a number is as likely to be `2330`,
//! TSMC, as it is `15` minutes, and only the whole query can tell them apart.

use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;
use gtk::glib;
use omacharts_engine::{Instrument, SearchIndex, Timeframe};

use crate::ui::dialogs;

/// Rows beyond this are noise — nobody scrolls a symbol picker.
const LIMIT: usize = 24;

type Handler = Rc<RefCell<Option<Box<dyn Fn(Instrument)>>>>;
type ResolutionHandler = Rc<RefCell<Option<Box<dyn Fn(Timeframe)>>>>;

/// One row's worth of answer.
#[derive(Clone, Debug, PartialEq)]
enum Pick {
    Symbol(Instrument),
    /// A ticker the inventory does not have, offered anyway.
    Unlisted(Instrument),
    Resolution(Timeframe),
}

pub struct SymbolSearch {
    dialog: adw::Dialog,
    list: gtk::ListBox,
    /// The list's window onto itself, so a selection moved by the
    /// keyboard can be brought into it.
    scroller: gtk::ScrolledWindow,
    entry: gtk::SearchEntry,
    index: crate::inventory::Inventory,
    /// What is on display, parallel to the list rows.
    shown: Rc<RefCell<Vec<Pick>>>,
    handler: Handler,
    /// Set only where a resolution means something — on a chart, not when
    /// adding to a watchlist — and its absence is what keeps the row out.
    on_resolution: ResolutionHandler,
    /// True from the moment the picker is asked to open until it has settled.
    ///
    /// An entry selects what it holds when it gains focus, and on the first
    /// open that focus is installed by the dialog being mapped — after
    /// anything this code can schedule. Rather than race it, the selection is
    /// undone whenever it appears during this window.
    opening: Rc<std::cell::Cell<bool>>,
}

impl SymbolSearch {
    pub fn new(index: crate::inventory::Inventory) -> Rc<SymbolSearch> {
        let entry = gtk::SearchEntry::new();
        entry.set_placeholder_text(Some("Symbol or name"));
        entry.set_hexpand(true);

        let list = gtk::ListBox::new();
        list.set_selection_mode(gtk::SelectionMode::Browse);
        list.add_css_class("navigation-sidebar");

        let scroller = gtk::ScrolledWindow::new();
        scroller.set_child(Some(&list));
        scroller.set_vexpand(true);
        scroller.set_hscrollbar_policy(gtk::PolicyType::Never);

        let header = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        header.set_margin_top(10);
        header.set_margin_bottom(10);
        header.set_margin_start(12);
        header.set_margin_end(12);
        header.append(&entry);

        let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
        content.append(&header);
        content.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
        content.append(&scroller);

        let dialog = adw::Dialog::new();
        dialog.set_content_width(520);
        dialog.set_content_height(430);
        dialog.set_child(Some(&content));

        let search = Rc::new(SymbolSearch {
            dialog,
            list,
            scroller,
            entry,
            index,
            shown: Rc::new(RefCell::new(Vec::new())),
            handler: Rc::new(RefCell::new(None)),
            on_resolution: Rc::new(RefCell::new(None)),
            opening: Rc::new(std::cell::Cell::new(false)),
        });
        search.wire();
        search.populate("");
        search
    }

    fn wire(self: &Rc<Self>) {
        // Undo the select-all an entry does when it gains focus, for as long
        // as the picker is still opening. Typing a letter on the chart opens
        // this with that letter in the box, and a selected letter is one the
        // next keystroke replaces — so "S" then "H" searched for "H".
        //
        // Watching the selection rather than the focus, because the focus that
        // does this is installed by the dialog's own mapping on the first open
        // and lands after everything this code can schedule. A selection that
        // nobody asked for cannot arrive too late to be undone.
        if let Some(text) = self.entry.delegate().and_then(|d| d.downcast::<gtk::Text>().ok()) {
            let opening = self.opening.clone();
            text.connect_notify_local(Some("selection-bound"), move |text, _| {
                if !opening.get() || text.selection_bounds().is_none() {
                    return;
                }
                // -1 is the end. Putting the caret there collapses the
                // selection, which fires this again — harmlessly, there being
                // nothing selected the second time.
                text.set_position(-1);
            });
        }

        // Typing. Every keystroke re-runs the search against memory, which is
        // cheap enough that debouncing would only add latency.
        let (list, index, shown) = (self.list.clone(), self.index.clone(), self.shown.clone());
        let on_resolution = self.on_resolution.clone();
        self.entry.connect_search_changed(move |entry| {
            let resolutions = on_resolution.borrow().is_some();
            repopulate(&list, &index.get(), &shown, &entry.text(), resolutions);
        });

        // Enter takes the highlighted row, so an exact ticker never needs the
        // arrow keys or the mouse.
        let list = self.list.clone();
        self.entry.connect_activate(move |_| {
            if let Some(row) = list.selected_row() {
                row.activate();
            }
        });

        // The same from anywhere in the dialog, for a hand that has left the
        // box for the list. Enter there is the row's own key and means this
        // too, so the two agree rather than compete.
        let list = self.list.clone();
        dialogs::commit_on_ctrl_enter(&self.dialog, move || {
            if let Some(row) = list.selected_row() {
                row.activate();
            }
        });

        // Only the box needs saying. Escape anywhere else in here already
        // closes the dialog, which is why there is no controller for it.
        dialogs::close_on_search_escape(&self.entry, &self.dialog);

        // Up and down move the selection while focus stays in the entry.
        let keys = gtk::EventControllerKey::new();
        let list = self.list.clone();
        let scroller = self.scroller.clone();
        keys.connect_key_pressed(move |_, key, _, _| {
            let rows = row_count(&list);
            if rows == 0 {
                return glib::Propagation::Proceed;
            }
            let current = list.selected_row().map(|r| r.index()).unwrap_or(0);
            let next = match key {
                gtk::gdk::Key::Down => (current + 1).min(rows - 1),
                gtk::gdk::Key::Up => (current - 1).max(0),
                _ => return glib::Propagation::Proceed,
            };
            if let Some(row) = list.row_at_index(next) {
                list.select_row(Some(&row));
                // Focus stays in the entry so typing carries on, and
                // the list does not scroll itself for a selection it
                // was not given focus for: arrowing past the bottom
                // moved a highlight nobody could see.
                reveal(&scroller, &list, &row);
            }
            glib::Propagation::Stop
        });
        self.entry.add_controller(keys);

        let shown = self.shown.clone();
        let dialog = self.dialog.clone();
        let handler = self.handler.clone();
        let on_resolution = self.on_resolution.clone();
        self.list.connect_row_activated(move |_, row| {
            let index = row.index().max(0) as usize;
            let Some(picked) = shown.borrow().get(index).cloned() else { return };
            dialog.close();
            match picked {
                Pick::Symbol(instrument) | Pick::Unlisted(instrument) => {
                    if let Some(handler) = handler.borrow().as_ref() {
                        handler(instrument);
                    }
                }
                Pick::Resolution(timeframe) => {
                    if let Some(handler) = on_resolution.borrow().as_ref() {
                        handler(timeframe);
                    }
                }
            }
        });
    }

    fn populate(&self, query: &str) {
        let resolutions = self.on_resolution.borrow().is_some();
        repopulate(&self.list, &self.index.get(), &self.shown, query, resolutions);
    }

    /// Open the picker. `title` says what picking will do.
    pub fn present(
        &self,
        parent: &impl IsA<gtk::Widget>,
        title: &str,
        on_pick: impl Fn(Instrument) + 'static,
    ) {
        *self.on_resolution.borrow_mut() = None;
        self.open(parent, title, "", on_pick);
    }

    /// Open it on a chart, already carrying a query: a symbol or a resolution.
    ///
    /// Typing on the chart opens this with that keystroke in the box, so the
    /// key that summoned the picker is not lost — which is what makes "just
    /// start typing" feel like one gesture instead of two.
    pub fn present_for_chart(
        &self,
        parent: &impl IsA<gtk::Widget>,
        query: &str,
        on_pick: impl Fn(Instrument) + 'static,
        on_resolution: impl Fn(Timeframe) + 'static,
    ) {
        *self.on_resolution.borrow_mut() = Some(Box::new(on_resolution));
        self.open(parent, "Symbol or resolution", query, on_pick);
    }

    fn open(
        &self,
        parent: &impl IsA<gtk::Widget>,
        title: &str,
        query: &str,
        on_pick: impl Fn(Instrument) + 'static,
    ) {
        *self.handler.borrow_mut() = Some(Box::new(on_pick));
        self.opening.set(true);
        self.dialog.set_title(title);
        self.entry.set_text(query);
        self.entry.set_position(-1);
        self.populate(query);
        self.dialog.present(Some(parent));
        self.entry.grab_focus();
        self.entry.set_position(-1);
        // Long enough for the dialog to finish opening and install its own
        // focus, short enough to be over before anybody reads the first
        // result. After this a selection in the box is one the user made.
        let opening = self.opening.clone();
        glib::timeout_add_local_once(std::time::Duration::from_millis(400), move || {
            opening.set(false);
        });
    }
}

/// Re-run the search and rebuild the rows, keeping `shown` parallel to them.
fn repopulate(
    list: &gtk::ListBox,
    index: &SearchIndex,
    shown: &RefCell<Vec<Pick>>,
    query: &str,
    resolutions: bool,
) {
    while let Some(child) = list.first_child() {
        list.remove(&child);
    }

    let rows = picks(index, query, resolutions);
    for pick in &rows {
        list.append(&match pick {
            Pick::Symbol(instrument) => row_for(instrument),
            Pick::Unlisted(instrument) => unlisted_row(instrument),
            Pick::Resolution(timeframe) => resolution_row(*timeframe),
        });
    }
    *shown.borrow_mut() = rows;

    // Always leave the best match highlighted, so Enter is enough.
    if let Some(first) = list.row_at_index(0) {
        list.select_row(Some(&first));
        // A new query is a new list: whatever was scrolled to belonged
        // to the old one.
        if let Some(scroller) =
            list.ancestor(gtk::ScrolledWindow::static_type()).and_downcast::<gtk::ScrolledWindow>()
        {
            scroller.vadjustment().set_value(0.0);
        }
    }
}

/// What a query offers, best first — the first is what Enter takes.
fn picks(index: &SearchIndex, query: &str, resolutions: bool) -> Vec<Pick> {
    let symbols: Vec<Instrument> = index
        .search(query, LIMIT)
        .iter()
        .filter_map(|hit| index.get(hit.index).cloned())
        .collect();

    // Nothing matched, but the query looks like a ticker: offer it anyway.
    //
    // The inventory is a list of symbols somebody wrote down, and the provider
    // knows more symbols than any list does — a company that listed this
    // morning has prices before it has an entry. Refusing to try is the app
    // asserting something it cannot know.
    let guess = symbols.is_empty().then(|| unlisted(query)).flatten();
    let mut rows: Vec<Pick> =
        symbols.into_iter().map(Pick::Symbol).chain(guess.map(Pick::Unlisted)).collect();

    // A resolution only where one means something, and only when the query
    // starts with a digit: `d`, `h` and `w` parse as resolutions on their
    // own, and would get in front of DIS, HD and WMT.
    let timeframe = (resolutions && query.trim().starts_with(|c: char| c.is_ascii_digit()))
        .then(|| Timeframe::parse(query))
        .flatten();
    if let Some(timeframe) = timeframe {
        // First, unless the query is exactly a ticker. Every all-digit ticker
        // has four digits or more, so `5`, `15` and `240` are resolutions; and
        // `2330` typed whole means TSMC rather than a day and a half of
        // minutes. Either way the other answer is one arrow away.
        let at = match rows.first() {
            Some(Pick::Symbol(first)) if is_exactly(first, query) => 1,
            _ => 0,
        };
        rows.insert(at, Pick::Resolution(timeframe));
    }
    rows
}

/// Is `query` this instrument's ticker, written in full?
fn is_exactly(instrument: &Instrument, query: &str) -> bool {
    let query = query.trim();
    instrument.symbol.eq_ignore_ascii_case(query)
        || instrument.display_symbol().eq_ignore_ascii_case(query)
}

/// Bring a row into the window, moving as little as will do it.
///
/// The row's own bounds within the list, which is the coordinate space
/// the scroll offset is in. Nothing happens when it is already visible,
/// so holding Down scrolls one row at a time from the bottom edge
/// rather than centring the selection and throwing the list about.
fn reveal(scroller: &gtk::ScrolledWindow, list: &gtk::ListBox, row: &gtk::ListBoxRow) {
    let Some(bounds) = row.compute_bounds(list) else { return };
    let adjustment = scroller.vadjustment();
    let top = bounds.y() as f64;
    let bottom = top + bounds.height() as f64;
    let seen = adjustment.value();
    let page = adjustment.page_size();

    if top < seen {
        adjustment.set_value(top);
    } else if bottom > seen + page {
        adjustment.set_value(bottom - page);
    }
}

/// Ticker, name, and what kind of thing it is. Nothing else — a picker is for
/// picking.
/// A symbol the inventory does not have, if the query could be one.
///
/// Deliberately strict about shape rather than about existence: the provider
/// decides whether it exists, and a chart that says "no data for this symbol"
/// is a better answer than a picker that says nothing at all.
fn unlisted(query: &str) -> Option<Instrument> {
    let typed = query.trim().to_uppercase();
    // `2330.TW` is a ticker on a venue, and a ticker abroad may be all
    // digits — every Taiwanese and Japanese one is. Without a venue a ticker
    // has to start with a letter, which is what keeps a stray number typed
    // into the field from being offered as a US listing.
    let (symbol, suffix) = omacharts_engine::symbols::split_suffix(&typed);
    let plausible = (1..=6).contains(&symbol.chars().count())
        && symbol
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || (suffix.is_some() && c.is_ascii_digit()))
        && symbol.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-');
    plausible.then(|| Instrument {
        symbol: symbol.to_string(),
        name: "Not in the list — look it up anyway".to_string(),
        kind: omacharts_engine::InstrumentKind::Equity,
        suffix: suffix.map(str::to_string),
        currency: None,
        tier: 2,
        session_origin: 0,
        overrides: Vec::new(),
        exchange: None,
        popularity: 0,
        local_name: None,
    })
}

fn unlisted_row(instrument: &Instrument) -> gtk::ListBoxRow {
    let row = row_for(instrument);
    row.add_css_class("symbol-row-unlisted");
    row
}

/// The badge's box, in logical pixels.
const BADGE: i32 = 18;

/// A quiet mark for what kind of thing this is.
///
/// Drawn rather than lettered, in the row's own colour at half strength: the
/// kind is already named in words further along the row, so this is there to
/// be recognised at a glance down the list, not read.
fn kind_badge(kind: omacharts_engine::InstrumentKind) -> gtk::DrawingArea {
    badge(move |cr, w, h| draw_kind(cr, kind, w, h))
}

/// A badge drawn by `draw`, in the row's own colour at half strength.
fn badge(draw: impl Fn(&gtk::cairo::Context, f64, f64) + 'static) -> gtk::DrawingArea {
    let area = gtk::DrawingArea::new();
    area.set_content_width(BADGE);
    area.set_content_height(BADGE);
    area.set_valign(gtk::Align::Center);
    area.set_draw_func(move |area, cr, w, h| {
        let c = area.color();
        let (r, g, b) = (c.red() as f64, c.green() as f64, c.blue() as f64);
        // Half strength reads as half strength on a dark theme, where the mark
        // is lighter than its ground. Ink on paper needs a little more to look
        // equally quiet.
        let alpha = if 0.3 * r + 0.6 * g + 0.1 * b > 0.5 { 0.5 } else { 0.62 };
        cr.set_source_rgba(r, g, b, alpha);
        draw(cr, w as f64, h as f64);
    });
    area
}

/// The glyph itself, in whatever source the caller has already set.
///
/// Every mark lives in the same 11-pixel box so none of them out-weighs the
/// others down a list, and every axis-aligned stroke sits on a half pixel so a
/// one-pixel line covers one pixel instead of smearing across two.
fn draw_kind(cr: &gtk::cairo::Context, kind: omacharts_engine::InstrumentKind, w: f64, h: f64) {
    use omacharts_engine::InstrumentKind as K;
    // The box: snapped edges, and a centre line that is itself snapped.
    let snap = |v: f64| v.floor() + 0.5;
    let (left, top) = (snap(w / 2.0 - 5.5), snap(h / 2.0 - 5.5));
    let (right, bottom) = (left + 11.0, top + 11.0);
    let (cx, cy) = (left + 5.5, top + 5.5);

    cr.set_line_width(1.0);
    cr.set_line_cap(gtk::cairo::LineCap::Round);
    cr.set_line_join(gtk::cairo::LineJoin::Round);
    match kind {
        // A candle: the thing a stock chart is made of.
        K::Equity => {
            cr.move_to(cx, top);
            cr.line_to(cx, bottom);
            let _ = cr.stroke();
            cr.rectangle(cx - 3.0, top + 2.0, 6.0, 8.0);
            let _ = cr.stroke();
        }
        // A basket: a ragged list of holdings rather than one price.
        K::Etf => {
            for (i, y) in [top + 1.0, cy, bottom - 1.0].into_iter().enumerate() {
                cr.move_to(left, y);
                cr.line_to(right - [0.0, 3.0, 5.0][i], y);
            }
            let _ = cr.stroke();
        }
        // A line going up, which is what an index is a picture of.
        K::Index => {
            cr.move_to(left, bottom - 1.0);
            cr.line_to(left + 4.0, top + 5.0);
            cr.line_to(left + 7.0, top + 7.0);
            cr.line_to(right, top + 1.0);
            let _ = cr.stroke();
        }
        // A contract: a ruled sheet with a date on it.
        K::FutureRoot => {
            cr.rectangle(left, top + 1.0, 11.0, 9.0);
            cr.move_to(left, top + 4.0);
            cr.line_to(right, top + 4.0);
            let _ = cr.stroke();
        }
        // A pair, exchanged.
        K::Fx => {
            cr.move_to(left, cy - 2.5);
            cr.line_to(right, cy - 2.5);
            cr.move_to(right - 2.5, cy - 5.0);
            cr.line_to(right, cy - 2.5);
            cr.line_to(right - 2.5, cy);
            cr.move_to(right, cy + 2.5);
            cr.line_to(left, cy + 2.5);
            cr.move_to(left + 2.5, cy);
            cr.line_to(left, cy + 2.5);
            cr.line_to(left + 2.5, cy + 5.0);
            let _ = cr.stroke();
        }
        // A block, the way every chain draws itself.
        K::Crypto => {
            let r = 5.5 / (std::f64::consts::PI / 6.0).cos();
            for i in 0..6 {
                let a = std::f64::consts::PI / 6.0 + i as f64 * std::f64::consts::TAU / 6.0;
                let (x, y) = (cx + r * a.cos(), cy + r * a.sin() * 0.88);
                if i == 0 {
                    cr.move_to(x, y);
                } else {
                    cr.line_to(x, y);
                }
            }
            cr.close_path();
            let _ = cr.stroke();
        }
    }
}

fn row_for(instrument: &Instrument) -> gtk::ListBoxRow {
    let ticker = gtk::Label::new(Some(&instrument.display_symbol()));
    ticker.add_css_class("symbol-row-ticker");
    ticker.set_xalign(0.0);
    ticker.set_width_chars(9);

    let name = gtk::Label::new(Some(&instrument.full_name()));
    name.add_css_class("symbol-row-name");
    name.set_xalign(0.0);
    name.set_hexpand(true);
    name.set_ellipsize(gtk::pango::EllipsizeMode::End);

    // Where it trades, the way TradingView says it: the name alone does not
    // tell you whether BBVA means Madrid or the New York receipt.
    let venue = gtk::Label::new(instrument.exchange_label());
    venue.add_css_class("symbol-venue");
    venue.set_valign(gtk::Align::Center);
    venue.set_visible(instrument.exchange_label().is_some());

    let kind = gtk::Label::new(Some(instrument.kind.label()));
    kind.add_css_class("symbol-kind");
    kind.set_valign(gtk::Align::Center);

    row_of(&[
        kind_badge(instrument.kind).upcast(),
        ticker.upcast(),
        name.upcast(),
        venue.upcast(),
        kind.upcast(),
    ])
}

/// The row that sets the resolution, saying what it will be: `240` reads
/// back as "4 hours" before anybody commits to it.
fn resolution_row(timeframe: Timeframe) -> gtk::ListBoxRow {
    let what = gtk::Label::new(Some(&timeframe.description()));
    what.add_css_class("symbol-row-ticker");
    what.set_xalign(0.0);
    what.set_hexpand(true);

    let kind = gtk::Label::new(Some("Resolution"));
    kind.add_css_class("symbol-kind");
    kind.set_valign(gtk::Align::Center);

    row_of(&[badge(draw_clock).upcast(), what.upcast(), kind.upcast()])
}

/// One row, its parts laid out left to right.
fn row_of(parts: &[gtk::Widget]) -> gtk::ListBoxRow {
    let row_box = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    row_box.set_margin_top(7);
    row_box.set_margin_bottom(7);
    row_box.set_margin_start(10);
    row_box.set_margin_end(10);
    for part in parts {
        row_box.append(part);
    }
    let row = gtk::ListBoxRow::new();
    row.set_child(Some(&row_box));
    row
}

/// A clock face, in the same 11-pixel box as the kind marks.
fn draw_clock(cr: &gtk::cairo::Context, w: f64, h: f64) {
    let (cx, cy) = ((w / 2.0).floor() + 0.5, (h / 2.0).floor() + 0.5);
    cr.set_line_width(1.0);
    cr.set_line_cap(gtk::cairo::LineCap::Round);
    cr.arc(cx, cy, 5.5, 0.0, std::f64::consts::TAU);
    cr.move_to(cx, cy - 3.0);
    cr.line_to(cx, cy);
    cr.line_to(cx + 2.5, cy);
    let _ = cr.stroke();
}

fn row_count(list: &gtk::ListBox) -> i32 {
    let mut n = 0;
    let mut child = list.first_child();
    while let Some(widget) = child {
        n += 1;
        child = widget.next_sibling();
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all(query: &str, resolutions: bool) -> Vec<Pick> {
        picks(&crate::inventory::everything(), query, resolutions)
    }

    fn resolution(text: &str) -> Pick {
        Pick::Resolution(Timeframe::parse(text).unwrap())
    }

    fn ticker(pick: Option<&Pick>) -> Option<String> {
        match pick {
            Some(Pick::Symbol(instrument)) => Some(instrument.display_symbol()),
            _ => None,
        }
    }

    fn offers_a_resolution(picks: &[Pick]) -> bool {
        picks.iter().any(|p| matches!(p, Pick::Resolution(_)))
    }

    #[test]
    fn a_short_number_or_a_unit_is_a_resolution() {
        for (query, expected) in [("5", "5"), ("15", "15"), ("240", "4h"), ("4h", "4h"), ("1D", "1D")] {
            assert_eq!(all(query, true).first(), Some(&resolution(expected)), "{query:?}");
        }
    }

    #[test]
    fn a_number_that_is_a_ticker_is_the_ticker_with_the_resolution_next() {
        let picks = all("2330", true);
        assert_eq!(ticker(picks.first()).as_deref(), Some("2330.TW"));
        assert_eq!(picks.get(1), Some(&resolution("2330")));
        assert_eq!(ticker(all("2330.tw", true).first()).as_deref(), Some("2330.TW"));
    }

    #[test]
    fn letters_never_offer_a_resolution() {
        // `d`, `h` and `w` parse as resolutions on their own.
        for query in ["msft", "d", "h", "w"] {
            assert!(!offers_a_resolution(&all(query, true)), "{query:?}");
        }
        assert_eq!(ticker(all("msft", true).first()).as_deref(), Some("MSFT"));
    }

    #[test]
    fn picking_for_a_watchlist_never_offers_a_resolution() {
        for query in ["5", "240", "2330"] {
            assert!(!offers_a_resolution(&all(query, false)), "{query:?}");
        }
    }
}
