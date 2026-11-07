//! Find a symbol by typing.
//!
//! The whole inventory is already in memory, so every keystroke re-runs the
//! search and rebuilds the list. No debounce, no spinner, no network — which
//! is the point: it should feel like the Omarchy launcher, not like a web
//! autocomplete.
//!
//! One instance serves every place a symbol is picked — charting it, adding it
//! to a watchlist section — by swapping the handler at presentation time.
//! Identical behaviour everywhere, and the index is only built once. Where it
//! opens is `pick_frame`'s business: over the chart a pick will change, or
//! centred on the window for a watchlist, which no chart shows.
//!
//! A futures root is a row like any other, and also a fold: opened, the
//! contracts it lists sit under it, soonest first, the front month marked,
//! the way TradingView nests them. The rows come from the root's own rule
//! the moment it opens; the feed is asked at the same time, and what it says
//! replaces them when it answers. Typing a contract's own symbol, `ESH7`,
//! or a root and a month letter, `ESH`, puts the contract first without
//! opening anything.

use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::{Rc, Weak};

use adw::prelude::*;
use chrono::NaiveDate;
use gtk::glib;
use omacharts_engine::contracts::{self, Listing, Month};
use omacharts_engine::{Instrument, InstrumentKind, SearchIndex};

use crate::ui::pick_frame::{Over, PickFrame};

/// Rows beyond this are noise — nobody scrolls a symbol picker.
const LIMIT: usize = 24;

/// How far a contract sits in from its root.
const NESTED: i32 = 16;

type Handler = Rc<RefCell<Option<Box<dyn Fn(Instrument)>>>>;
/// Asks the feeds what contracts a root lists, and says when they have
/// answered.
type Lister = Rc<dyn Fn(&Instrument, Box<dyn Fn()>)>;

pub struct SymbolSearch {
    frame: Rc<PickFrame>,
    index: crate::inventory::Inventory,
    /// Instruments on display, parallel to the list rows.
    shown: Rc<RefCell<Vec<Instrument>>>,
    handler: Handler,
    /// True from the moment the picker is asked to open until it has settled.
    ///
    /// An entry selects what it holds when it gains focus, and that focus can
    /// be installed by the frame being mapped — after anything this code can
    /// schedule. Rather than race it, the selection is undone whenever it
    /// appears during this window.
    opening: Rc<std::cell::Cell<bool>>,
    /// The futures roots open to show their contracts, by symbol. Kept for
    /// as long as the picker is open, so retyping does not fold what was
    /// just opened; the next opening starts with every one folded.
    expanded: RefCell<HashSet<String>>,
    /// The roots whose feed has been asked for its contracts this opening.
    /// Once is enough: what it says is kept by the engine.
    asked: RefCell<HashSet<String>>,
    lister: RefCell<Option<Lister>>,
}

impl SymbolSearch {
    pub fn new(index: crate::inventory::Inventory) -> Rc<SymbolSearch> {
        let search = Rc::new(SymbolSearch {
            frame: PickFrame::new("Symbol or name"),
            index,
            shown: Rc::new(RefCell::new(Vec::new())),
            handler: Rc::new(RefCell::new(None)),
            opening: Rc::new(std::cell::Cell::new(false)),
            expanded: RefCell::new(HashSet::new()),
            asked: RefCell::new(HashSet::new()),
            lister: RefCell::new(None),
        });
        search.wire();
        search.populate("");
        search
    }

    /// How to ask the feeds for a root's contracts; without it, a root
    /// opens on its rule alone.
    pub fn list_contracts_through(&self, lister: impl Fn(&Instrument, Box<dyn Fn()>) + 'static) {
        *self.lister.borrow_mut() = Some(Rc::new(lister));
    }

    fn wire(self: &Rc<Self>) {
        let entry = &self.frame.entry;
        // Undo the select-all an entry does when it gains focus, for as long
        // as the picker is still opening. Typing a letter on the chart opens
        // this with that letter in the box, and a selected letter is one the
        // next keystroke replaces — so "S" then "H" searched for "H".
        //
        // Watching the selection rather than the focus, because the focus that
        // does this can be installed by the frame's own mapping and land after
        // everything this code can schedule. A selection that nobody asked
        // for cannot arrive too late to be undone.
        if let Some(text) = entry.delegate().and_then(|d| d.downcast::<gtk::Text>().ok()) {
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
        let weak = Rc::downgrade(self);
        entry.connect_search_changed(move |entry| {
            if let Some(search) = weak.upgrade() {
                search.populate(&entry.text());
            }
        });

        // Right opens the root the highlight is on and Left folds it. The
        // keyboard stays in the box, so typing carries on, and the box's own
        // text widget takes Right and Left for the caret before anything
        // above it hears them: so this listens on the way down, and lets
        // the caret have Right unless it is already at the end of the text,
        // where Right has nothing else to do. Left folds whenever a fold is
        // under the highlight, which is the one thing it can visibly do
        // there.
        let keys = gtk::EventControllerKey::new();
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        let weak = Rc::downgrade(self);
        keys.connect_key_pressed(move |_, key, _, _| match weak.upgrade() {
            Some(search) => search.arrow(key),
            None => glib::Propagation::Proceed,
        });
        entry.add_controller(keys);
    }

    fn populate(self: &Rc<Self>, query: &str) {
        self.rebuild(query, None);
    }

    /// Rebuild the rows for `query`, leaving the highlight on `keep` when it
    /// is still among them and on the best match otherwise.
    fn rebuild(self: &Rc<Self>, query: &str, keep: Option<Instrument>) {
        let list = &self.frame.list;
        while let Some(child) = list.first_child() {
            list.remove(&child);
        }
        let rows = rows_for(&self.index.get(), &self.expanded.borrow(), query, contracts::today());
        let weak = Rc::downgrade(self);
        for row in &rows {
            list.append(&widget_for(row, &weak));
        }
        let shown: Vec<Instrument> = rows.into_iter().map(Row::into_instrument).collect();
        let kept = keep.and_then(|keep| shown.iter().position(|row| same(row, &keep)));
        *self.shown.borrow_mut() = shown;

        // Always leave the best match highlighted, so Enter is enough.
        if let Some(row) = list.row_at_index(kept.unwrap_or(0) as i32) {
            list.select_row(Some(&row));
            // A new query is a new list: whatever was scrolled to belonged
            // to the old one.
            if kept.is_none() {
                self.frame.scroller.vadjustment().set_value(0.0);
            }
        }
    }

    /// What the highlight is on.
    fn selected(&self) -> Option<Instrument> {
        let at = self.frame.list.selected_row()?.index().max(0) as usize;
        self.shown.borrow().get(at).cloned()
    }

    fn is_expanded(&self, root: &Instrument) -> bool {
        self.expanded.borrow().contains(&root.symbol)
    }

    fn toggle(self: &Rc<Self>, root: &Instrument) {
        match self.is_expanded(root) {
            true => self.collapse(root),
            false => self.expand(root),
        }
    }

    /// Open `root`: its contracts by its rule now, and the feed's once it
    /// has been asked, the first time this opening.
    fn expand(self: &Rc<Self>, root: &Instrument) {
        self.expanded.borrow_mut().insert(root.symbol.clone());
        self.set_open(root, true);
        if !self.asked.borrow_mut().insert(root.symbol.clone()) {
            return;
        }
        let Some(lister) = self.lister.borrow().clone() else { return };
        let (weak, opened) = (Rc::downgrade(self), root.clone());
        lister(
            root,
            Box::new(move || {
                let Some(search) = weak.upgrade() else { return };
                // What the feed says may be more months, or another front
                // month, than the rule drew; a fold closed since has nothing
                // to redraw.
                if search.is_expanded(&opened) {
                    search.set_open(&opened, true);
                }
            }),
        );
    }

    fn collapse(self: &Rc<Self>, root: &Instrument) {
        self.expanded.borrow_mut().remove(&root.symbol);
        self.set_open(root, false);
    }

    /// Open or fold `root` where it stands: its contracts go in or out
    /// under its row, its chevron turns, and every other row stays the
    /// widget it was. Not a rebuild of the list, because the chevron is
    /// under the pointer when it is clicked, and a widget taken away under
    /// the pointer leaves the pointer on nothing until it moves: the next
    /// click on the new chevron in its place went nowhere, so every other
    /// click was lost. A root not on the list, which a typed contract's
    /// list can be, is rebuilt in.
    fn set_open(self: &Rc<Self>, root: &Instrument, open: bool) {
        let list = &self.frame.list;
        let at = self.shown.borrow().iter().position(|row| is_root(row) && same(row, root));
        let Some(at) = at else {
            return self.rebuild(&self.frame.entry.text(), Some(root.clone()));
        };
        let was = self.selected();
        let under = |search: &SymbolSearch| {
            search.shown.borrow().get(at + 1).is_some_and(|row| row.is_contract() && row.root() == root.symbol)
        };
        while under(self) {
            if let Some(row) = list.row_at_index(at as i32 + 1) {
                list.remove(&row);
            }
            self.shown.borrow_mut().remove(at + 1);
        }
        if open {
            let mut index = at + 1;
            for listing in &contracts::listing(root.root(), contracts::today()) {
                let Some(instrument) = root.in_month(listing.month) else { continue };
                list.insert(&contract_row(&instrument, listing, true), index as i32);
                self.shown.borrow_mut().insert(index, instrument);
                index += 1;
            }
        }
        let Some(row) = list.row_at_index(at as i32) else { return };
        if let Some(fold) = chevron_of(&row) {
            fold.set_icon_name(match open {
                true => "pan-down-symbolic",
                false => "pan-end-symbolic",
            });
        }
        // The highlight stays on a contract under the root when it was on
        // one and the fold is still open; on the root otherwise.
        let kept = was
            .filter(|_| open)
            .and_then(|was| self.shown.borrow().iter().position(|row| same(row, &was)))
            .and_then(|index| list.row_at_index(index as i32));
        list.select_row(Some(&kept.unwrap_or(row)));
    }

    /// Right and Left on the highlight, when it is a root or one of its
    /// contracts.
    fn arrow(self: &Rc<Self>, key: gtk::gdk::Key) -> glib::Propagation {
        let Some(selected) = self.selected() else { return glib::Propagation::Proceed };
        match key {
            gtk::gdk::Key::Right if is_root(&selected) && !self.is_expanded(&selected) => {
                let entry = &self.frame.entry;
                if (entry.position().max(0) as usize) < entry.text().chars().count() {
                    return glib::Propagation::Proceed;
                }
                self.expand(&selected);
                glib::Propagation::Stop
            }
            gtk::gdk::Key::Left if selected.kind == InstrumentKind::FutureRoot && self.is_expanded(&selected.as_root()) => {
                self.collapse(&selected.as_root());
                glib::Propagation::Stop
            }
            _ => glib::Propagation::Proceed,
        }
    }

    /// Hand the row at `at` to whoever opened the picker.
    fn pick(&self, at: usize) {
        let picked = self.shown.borrow().get(at).cloned();
        if let (Some(instrument), Some(handler)) = (picked, self.handler.borrow().as_ref()) {
            handler(instrument);
        }
    }

    /// Open the picker over what it will change.
    pub fn present(self: &Rc<Self>, over: Over, on_pick: impl Fn(Instrument) + 'static) {
        self.present_with(over, "", on_pick);
    }

    /// Open it already carrying a query.
    ///
    /// Typing a letter on the chart opens this with that letter in the box, so
    /// the keystroke that summoned the picker is not lost — which is what
    /// makes "just start typing" feel like one gesture instead of two.
    pub fn present_with(self: &Rc<Self>, over: Over, query: &str, on_pick: impl Fn(Instrument) + 'static) {
        *self.handler.borrow_mut() = Some(Box::new(on_pick));
        self.opening.set(true);
        self.expanded.borrow_mut().clear();
        self.asked.borrow_mut().clear();
        let entry = &self.frame.entry;
        entry.set_text(query);
        entry.set_position(-1);
        self.populate(query);
        let weak = Rc::downgrade(self);
        self.frame.open(&over, move |at| {
            if let Some(search) = weak.upgrade() {
                search.pick(at);
            }
        });
        // Long enough for the frame to finish opening and install its own
        // focus, short enough to be over before anybody reads the first
        // result. After this a selection in the box is one the user made.
        let opening = self.opening.clone();
        glib::timeout_add_local_once(std::time::Duration::from_millis(400), move || {
            opening.set(false);
        });
    }
}

/// The same instrument on two rows.
fn same(a: &Instrument, b: &Instrument) -> bool {
    a.symbol == b.symbol && a.suffix == b.suffix
}

/// A root row's chevron, when it has one.
fn chevron_of(row: &gtk::ListBoxRow) -> Option<gtk::Button> {
    let mut child = row.child()?.first_child();
    while let Some(widget) = child {
        if let Some(button) = widget.downcast_ref::<gtk::Button>()
            && button.has_css_class("pick-expander")
        {
            return Some(button.clone());
        }
        child = widget.next_sibling();
    }
    None
}

/// A continuous futures root: the one kind of row that opens.
fn is_root(instrument: &Instrument) -> bool {
    instrument.kind == InstrumentKind::FutureRoot && !instrument.is_contract()
}

/// One row of the list.
enum Row {
    /// An instrument as the inventory lists it. A futures root is `open` or
    /// not; anything else has no fold.
    Listed { instrument: Instrument, open: Option<bool> },
    /// A dated contract from its root's listing: `nested` under the root,
    /// or at the top for having been typed.
    Contract { instrument: Instrument, listing: Listing, nested: bool },
    /// A symbol nobody listed, offered as the query was typed.
    Unlisted(Instrument),
}

impl Row {
    fn into_instrument(self) -> Instrument {
        match self {
            Row::Listed { instrument, .. } | Row::Contract { instrument, .. } | Row::Unlisted(instrument) => instrument,
        }
    }
}

/// What a query spells besides a search: a contract, or a root and a month
/// with no year.
#[derive(Debug, PartialEq, Eq)]
enum Typed {
    /// `ESH7`, `ES H27`: one contract.
    Contract(String, Month),
    /// `ESH`, `es h`: every listed contract of that month.
    Month(String, char),
}

/// A query read as a contract rather than searched for. Only the shape:
/// whether the root exists is the index's to say.
fn typed(query: &str, today: NaiveDate) -> Option<Typed> {
    if let Some((root, month)) = contracts::parse(query, today) {
        return Some(Typed::Contract(root, month));
    }
    let text = query.trim().to_ascii_uppercase();
    let (root, rest) = match text.split_once(char::is_whitespace) {
        Some((root, rest)) => (root.trim(), rest.trim()),
        None => {
            let last = text.chars().next_back()?;
            (&text[..text.len() - last.len_utf8()], &text[text.len() - last.len_utf8()..])
        }
    };
    let mut rest = rest.chars();
    let code = rest.next()?;
    if rest.next().is_some() || root.is_empty() || !root.chars().all(|c| c.is_ascii_alphanumeric()) {
        return None;
    }
    contracts::CODES.contains(&code).then(|| Typed::Month(root.to_string(), code))
}

/// The continuous root `symbol` names, when the index lists one.
fn futures_root(index: &SearchIndex, symbol: &str) -> Option<Instrument> {
    index.find(symbol, None).filter(|found| is_root(found)).cloned()
}

/// The rows for `query`: what it spells as a contract first, then the
/// search's hits, each open root followed by its contracts.
fn rows_for(index: &SearchIndex, expanded: &HashSet<String>, query: &str, today: NaiveDate) -> Vec<Row> {
    let mut rows = Vec::new();
    let contract_row = |root: &Instrument, listing: &Listing, nested: bool| {
        root.in_month(listing.month).map(|instrument| Row::Contract { instrument, listing: listing.clone(), nested })
    };
    match typed(query, today) {
        Some(Typed::Contract(root, month)) => {
            if let Some(root) = futures_root(index, &root) {
                let listed = contracts::listing(root.root(), today);
                rows.extend(match listed.iter().find(|listing| listing.month == month) {
                    Some(listing) => contract_row(&root, listing, false),
                    None => root.in_month(month).map(|instrument| Row::Listed { instrument, open: None }),
                });
            }
        }
        Some(Typed::Month(root, code)) => {
            if let Some(root) = futures_root(index, &root) {
                let listed = contracts::listing(root.root(), today);
                rows.extend(listed.iter().filter(|listing| listing.month.code() == code).filter_map(|listing| {
                    contract_row(&root, listing, false)
                }));
            }
        }
        None => {}
    }

    for hit in index.search(query, LIMIT) {
        let Some(instrument) = index.get(hit.index) else { continue };
        if !is_root(instrument) {
            rows.push(Row::Listed { instrument: instrument.clone(), open: None });
            continue;
        }
        let open = expanded.contains(&instrument.symbol);
        rows.push(Row::Listed { instrument: instrument.clone(), open: Some(open) });
        if open {
            let listed = contracts::listing(instrument.root(), today);
            rows.extend(listed.iter().filter_map(|listing| contract_row(instrument, listing, true)));
        }
    }

    // Nothing matched, but the query looks like a ticker: offer it anyway.
    //
    // The inventory is a list of symbols somebody wrote down, and the provider
    // knows more symbols than any list does — a company that listed this
    // morning has prices before it has an entry. Refusing to try is the app
    // asserting something it cannot know.
    if let Some(guess) = rows.is_empty().then(|| unlisted(query)).flatten() {
        rows.push(Row::Unlisted(guess));
    }
    rows
}

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

fn widget_for(row: &Row, search: &Weak<SymbolSearch>) -> gtk::ListBoxRow {
    match row {
        Row::Unlisted(instrument) => {
            let row = row_for(instrument);
            row.add_css_class("symbol-row-unlisted");
            row
        }
        Row::Listed { instrument, open: None } => row_for(instrument),
        Row::Listed { instrument, open: Some(open) } => {
            let row = row_for(instrument);
            let chevron = gtk::Button::from_icon_name(match open {
                true => "pan-down-symbolic",
                false => "pan-end-symbolic",
            });
            chevron.add_css_class("flat");
            chevron.add_css_class("pick-expander");
            chevron.set_valign(gtk::Align::Center);
            // The keyboard stays in the box whatever is clicked.
            chevron.set_can_focus(false);
            chevron.set_focus_on_click(false);
            let (search, root) = (search.clone(), instrument.clone());
            chevron.connect_clicked(move |_| {
                if let Some(search) = search.upgrade() {
                    search.toggle(&root);
                }
            });
            if let Some(line) = row.child().and_downcast::<gtk::Box>() {
                line.append(&chevron);
            }
            row
        }
        Row::Contract { instrument, listing, nested } => contract_row(instrument, listing, *nested),
    }
}

/// A row the way the rail draws one: ticker, name, where it trades, what
/// kind of thing it is. Nothing else — a picker is for picking.
fn row_for(instrument: &Instrument) -> gtk::ListBoxRow {
    let ticker = gtk::Label::new(Some(&instrument.display_symbol()));
    ticker.add_css_class("pick-ticker");
    ticker.set_xalign(0.0);
    ticker.set_width_chars(8);

    let name = gtk::Label::new(Some(&instrument.full_name()));
    name.add_css_class("pick-name");
    name.set_xalign(0.0);
    name.set_hexpand(true);
    name.set_ellipsize(gtk::pango::EllipsizeMode::End);

    // Where it trades, the way TradingView says it: the name alone does not
    // tell you whether BBVA means Madrid or the New York receipt.
    let venue = gtk::Label::new(instrument.exchange_label());
    venue.add_css_class("pick-venue");
    venue.set_valign(gtk::Align::Center);
    venue.set_visible(instrument.exchange_label().is_some());

    let kind = gtk::Label::new(Some(instrument.kind.label()));
    kind.add_css_class("symbol-kind");
    kind.set_valign(gtk::Align::Center);

    let row_box = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    row_box.append(&ticker);
    row_box.append(&name);
    row_box.append(&venue);
    row_box.append(&kind);

    let row = gtk::ListBoxRow::new();
    row.set_child(Some(&row_box));
    row
}

/// A contract's row in its root's slots: the symbol where the ticker goes,
/// the month for a name, the last day of trading for a venue, and "Front"
/// where the kind goes, on the one that is.
fn contract_row(instrument: &Instrument, listing: &Listing, nested: bool) -> gtk::ListBoxRow {
    let ticker = gtk::Label::new(Some(&instrument.display_symbol()));
    ticker.add_css_class("pick-ticker");
    ticker.set_xalign(0.0);
    ticker.set_width_chars(8);

    let month = gtk::Label::new(Some(&listing.month.label()));
    month.add_css_class("pick-name");
    month.set_xalign(0.0);
    month.set_hexpand(true);
    month.set_ellipsize(gtk::pango::EllipsizeMode::End);

    let expires = gtk::Label::new(Some(&listing.expires.format("Expires %-d %b %Y").to_string()));
    expires.add_css_class("pick-venue");
    expires.set_valign(gtk::Align::Center);

    let front = gtk::Label::new(Some("Front"));
    front.add_css_class("pick-front");
    front.set_valign(gtk::Align::Center);
    front.set_visible(listing.front);

    let row_box = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    if nested {
        row_box.set_margin_start(NESTED);
    }
    row_box.append(&ticker);
    row_box.append(&month);
    row_box.append(&expires);
    row_box.append(&front);

    let row = gtk::ListBoxRow::new();
    row.add_css_class("pick-contract");
    row.set_child(Some(&row_box));
    row
}

#[cfg(test)]
mod tests {
    use super::*;

    const TODAY: &str = "2026-11-04";

    fn day(text: &str) -> NaiveDate {
        NaiveDate::parse_from_str(text, "%Y-%m-%d").unwrap()
    }

    /// A contract is read however it is spelled, a root and a month letter
    /// with no year is a month, and anything else is a search.
    #[test]
    fn a_query_spells_a_contract_or_a_month_or_nothing() {
        let today = day(TODAY);
        let march = Month::new(2027, 3).unwrap();
        for text in ["ESH7", "esh7", "ES H27", "ESH2027"] {
            assert_eq!(typed(text, today), Some(Typed::Contract("ES".into(), march)), "{text:?}");
        }
        for text in ["ESH", "esh", "ES H", "es h ", " ES  H"] {
            assert_eq!(typed(text, today), Some(Typed::Month("ES".into(), 'H')), "{text:?}");
        }
        assert_eq!(typed("6EH", today), Some(Typed::Month("6E".into(), 'H')));
        for text in ["ES", "H", "", "ES HH", "ES H X", "ESA", "ES-H", "AAPL"] {
            assert_eq!(typed(text, today), None, "{text:?}");
        }
    }

    /// Picked rows are told apart by what they are, not where they sit.
    fn symbols(search: &SymbolSearch) -> Vec<String> {
        search.shown.borrow().iter().map(|i| i.symbol.clone()).collect()
    }

    fn settle() {
        let context = glib::MainContext::default();
        for _ in 0..20 {
            while context.iteration(false) {}
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
    }

    fn labels(row: &gtk::ListBoxRow) -> Vec<String> {
        let mut out = Vec::new();
        let mut child = row.child().and_then(|line| line.first_child());
        while let Some(widget) = child {
            // Its own flag: the frame is not on screen here.
            if let Some(label) = widget.downcast_ref::<gtk::Label>()
                && label.get_visible()
            {
                out.push(label.text().to_string());
            }
            child = widget.next_sibling();
        }
        out
    }

    fn chevron(row: &gtk::ListBoxRow) -> Option<gtk::Button> {
        chevron_of(row)
    }

    /// A root opens to its contracts and folds them away again, from the
    /// chevron and from the arrow keys; a contract picked is handed over
    /// as itself; and a contract typed comes first.
    #[test]
    fn a_futures_root_unfolds_its_contracts() {
        crate::ui::on_gtk(|| {
            let today = contracts::today();
            contracts::forget("ES");
            let index = crate::inventory::Inventory::curated();
            let search = SymbolSearch::new(index.clone());
            let asked: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
            let noted = asked.clone();
            search.list_contracts_through(move |root, done| {
                noted.borrow_mut().push(root.symbol.clone());
                done();
            });
            let window = gtk::Window::new();
            let inside = gtk::Box::new(gtk::Orientation::Vertical, 0);
            window.set_child(Some(&inside));
            let picked: Rc<RefCell<Option<Instrument>>> = Rc::new(RefCell::new(None));
            let handed = picked.clone();
            search.present_with(Over::window(&inside, "a test"), "es", move |instrument| {
                *handed.borrow_mut() = Some(instrument);
            });
            settle();

            let list = &search.frame.list;
            let shown = symbols(&search);
            assert_eq!(shown.first().map(String::as_str), Some("ES"), "{shown:?}");
            let root = list.row_at_index(0).unwrap();
            let fold = chevron(&root).expect("a chevron on the root");
            assert!(chevron(&list.row_at_index(1).unwrap()).is_none(), "only a root has one");
            assert_eq!(fold.icon_name().as_deref(), Some("pan-end-symbolic"));

            let listed = contracts::listing("ES", today);
            let expected: Vec<String> = listed.iter().map(|l| l.symbol.clone()).collect();
            fold.emit_clicked();
            let shown = symbols(&search);
            assert_eq!(&shown[1..=expected.len()], &expected[..], "{shown:?}");
            assert_eq!(*asked.borrow(), vec!["ES".to_string()], "the feed asked once");
            assert_eq!(chevron(&list.row_at_index(0).unwrap()).unwrap().icon_name().as_deref(), Some("pan-down-symbolic"));
            assert_eq!(list.selected_row().map(|row| row.index()), Some(0), "the root stays highlighted");
            let first = list.row_at_index(1).unwrap();
            assert!(first.has_css_class("pick-contract"));
            let front = listed.iter().position(|l| l.front).expect("a front month");
            let words = labels(&list.row_at_index(1 + front as i32).unwrap());
            assert_eq!(words[0], expected[front]);
            assert_eq!(words[1], listed[front].month.label());
            assert!(words[2].starts_with("Expires "), "{words:?}");
            assert_eq!(words.last().map(String::as_str), Some("Front"));
            let other = (front + 1) % expected.len();
            assert!(!labels(&list.row_at_index(1 + other as i32).unwrap()).contains(&"Front".to_string()));

            // Retyping keeps the fold open; a contract picked is itself.
            search.frame.entry.set_text("e");
            settle();
            let shown = symbols(&search);
            let at = shown.iter().position(|s| s == &expected[1]).expect("still open");
            list.row_at_index(at as i32).unwrap().activate();
            settle();
            assert_eq!(picked.borrow().as_ref().map(|i| i.symbol.clone()), Some(expected[1].clone()));
            assert!(picked.borrow().as_ref().is_some_and(Instrument::is_contract));

            // Left on a contract folds the root and lands on it; Right opens
            // it again, and the feed is not asked twice.
            // As typed: the caret at the end, where Right is the fold's.
            search.frame.entry.set_text("es");
            search.frame.entry.set_position(-1);
            settle();
            let folded = |search: &SymbolSearch| !symbols(search).iter().any(|s| expected.contains(s));
            let shown = symbols(&search);
            let at = shown.iter().position(|s| s == &expected[0]).unwrap();
            list.select_row(list.row_at_index(at as i32).as_ref());
            assert_eq!(search.arrow(gtk::gdk::Key::Left), glib::Propagation::Stop);
            assert!(folded(&search), "{:?}", symbols(&search));
            assert_eq!(search.selected().map(|i| i.symbol), Some("ES".into()));
            assert_eq!(search.arrow(gtk::gdk::Key::Right), glib::Propagation::Stop);
            assert_eq!(symbols(&search)[1], expected[0]);
            assert_eq!(asked.borrow().len(), 1);
            assert_eq!(search.arrow(gtk::gdk::Key::Left), glib::Propagation::Stop);
            assert!(folded(&search));

            // A contract typed comes first, whether or not the root is open.
            search.frame.entry.set_text("ESH7");
            settle();
            let shown = symbols(&search);
            assert_eq!(shown.first().map(String::as_str), Some("ESH7"), "{shown:?}");
            search.frame.entry.set_text("es h");
            settle();
            let shown = symbols(&search);
            let months: Vec<&String> = expected.iter().filter(|s| s.starts_with("ESH")).collect();
            assert!(!months.is_empty());
            assert_eq!(shown.iter().take(months.len()).collect::<Vec<_>>(), months, "{shown:?}");

            // Opened again, every fold is closed.
            search.frame.entry.set_text("es");
            search.frame.entry.set_position(-1);
            settle();
            search.arrow(gtk::gdk::Key::Right);
            assert!(search.is_expanded(&search.selected().unwrap()));
            search.present_with(Over::window(&inside, "a test"), "es", |_| {});
            settle();
            assert!(folded(&search));
            search.frame.close();
        });
    }

    /// Let the real events of the last little while arrive and be acted on.
    fn arrive() {
        let context = glib::MainContext::default();
        let until = std::time::Instant::now() + std::time::Duration::from_millis(600);
        while std::time::Instant::now() < until {
            while context.iteration(false) {}
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }

    /// A gesture or a key sent by the compositor the tests run under, as a
    /// hand would send it: `bin/headless-window click X Y`, `key Right`.
    fn send(args: &[&str]) {
        let tool = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("bin/headless-window");
        let done = std::process::Command::new(tool).args(args).output().expect("bin/headless-window runs");
        assert!(done.status.success(), "headless-window {args:?}: {}", String::from_utf8_lossy(&done.stderr));
        arrive();
    }

    /// The fold, clicked and keyed for real: the pointer and keyboard of the
    /// headless compositor the suite runs under, through the window, the
    /// popover and the row, not the handlers called by name. This is what
    /// found a list rebuilt under the pointer losing every other click, and
    /// Right taken by the box's caret before the fold heard it. Only under
    /// `bin/headless-window`, where there is a screen to click on.
    #[test]
    fn the_fold_answers_a_real_click_and_real_arrow_keys() {
        if std::env::var_os("OMACHARTS_WINDOW").is_none() {
            eprintln!("not under bin/headless-window: nothing to click");
            return;
        }
        crate::ui::on_gtk(|| {
            let today = contracts::today();
            contracts::forget("ES");
            let index = crate::inventory::Inventory::curated();
            let search = SymbolSearch::new(index.clone());
            search.list_contracts_through(|_, done| done());
            let window = gtk::Window::new();
            window.set_title(Some("omacharts search test"));
            let inside = gtk::Box::new(gtk::Orientation::Vertical, 0);
            window.set_child(Some(&inside));
            // The whole screen, so that the screen's coordinates are the
            // window's.
            window.fullscreen();
            window.present();
            for _ in 0..300 {
                if window.is_mapped() && window.is_active() {
                    break;
                }
                arrive();
            }
            assert!(window.is_active(), "the test window never took the screen");
            let picked: Rc<RefCell<Option<Instrument>>> = Rc::new(RefCell::new(None));
            let handed = picked.clone();
            search.present_with(Over::window(&inside, "a test"), "es", move |instrument| {
                *handed.borrow_mut() = Some(instrument);
            });
            arrive();
            let list = &search.frame.list;
            let expected: Vec<String> = contracts::listing("ES", today).into_iter().map(|l| l.symbol).collect();
            let open = |search: &SymbolSearch| symbols(search).get(1) == expected.first();
            let icon = |list: &gtk::ListBox| chevron(&list.row_at_index(0).unwrap()).unwrap().icon_name();
            assert_eq!(symbols(&search)[0], "ES");
            assert!(!open(&search));

            // Where the chevron is on the screen: the popover's place on the
            // window, its own frame within that, and the button within it.
            let popover = &search.frame.popover;
            let surface = popover.native().and_then(|native| native.surface()).expect("the popover is on screen");
            let popup = surface.downcast::<gtk::gdk::Popup>().expect("a popover is a popup");
            let (frame_x, frame_y) = popover.native().map(|native| native.surface_transform()).unwrap_or((0.0, 0.0));
            let at = |row: i32, widget: &gtk::Widget| {
                let bounds = widget.compute_bounds(popover).expect("in the popover");
                let _ = row;
                (
                    popup.position_x() as f64 + frame_x + f64::from(bounds.x() + bounds.width() / 2.0),
                    popup.position_y() as f64 + frame_y + f64::from(bounds.y() + bounds.height() / 2.0),
                )
            };
            let fold = chevron(&list.row_at_index(0).unwrap()).expect("a chevron");
            let (x, y) = at(0, fold.upcast_ref());
            let (x, y) = (x.round().to_string(), y.round().to_string());

            // Click: open, then shut, then open again: a click is never lost.
            send(&["click", &x, &y]);
            assert!(open(&search), "the first click did not open it: {:?}", symbols(&search));
            assert_eq!(icon(list).as_deref(), Some("pan-down-symbolic"));
            send(&["click", &x, &y]);
            assert!(!open(&search), "the second click did not fold it: {:?}", symbols(&search));
            assert_eq!(icon(list).as_deref(), Some("pan-end-symbolic"));
            send(&["click", &x, &y]);
            assert!(open(&search), "the third click did not open it");
            send(&["click", &x, &y]);
            assert!(!open(&search), "the fourth click did not fold it");

            // Keys, with the keyboard in the box and the caret at the end:
            // Right opens, Down lands on a contract, Left folds and lands on
            // the root, and another Left is the caret's.
            send(&["key", "End"]);
            assert_eq!(search.frame.entry.position(), 2);
            send(&["key", "Right"]);
            assert!(open(&search), "Right did not open it: {:?}", symbols(&search));
            assert_eq!(search.frame.entry.position(), 2, "Right was the fold's, not the caret's");
            send(&["key", "Down"]);
            assert_eq!(search.selected().map(|i| i.symbol), expected.first().cloned());
            send(&["key", "Left"]);
            assert!(!open(&search), "Left on a contract did not fold its root");
            assert_eq!(search.selected().map(|i| i.symbol), Some("ES".into()), "and land on it");
            assert_eq!(search.frame.entry.position(), 2);
            send(&["key", "Left"]);
            assert!(!open(&search));
            assert_eq!(search.frame.entry.position(), 1, "with nothing to fold, Left is the caret's");

            // The row itself, clicked, is still a pick of the root.
            let name = list.row_at_index(0).unwrap().child().and_then(|line| line.first_child()).expect("the ticker");
            let (x, y) = at(0, &name);
            send(&["click", &x.round().to_string(), &y.round().to_string()]);
            assert_eq!(picked.borrow().as_ref().map(|i| i.symbol.clone()), Some("ES".into()));
            window.close();
            arrive();
        });
    }
}
