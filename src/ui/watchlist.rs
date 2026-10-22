//! The watchlist rail.
//!
//! One `ListBox` holding every section's header and every symbol, rather than
//! a list per section. That is what lets the arrow keys run from the bottom of
//! one section into the top of the next without the user noticing there was a
//! boundary — which is the whole point of a watchlist you navigate rather than
//! click.
//!
//! Prices shown here come from the cache only. The rail never issues a
//! request; the window prefetches around the selection instead.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use adw::prelude::*;
use gtk::gio;
use gtk::glib;
use omacharts_engine::{grouped_decimals, FetchFailure, Instrument};

use crate::store::{Entry, Section, Store, DEFAULT_WATCHLIST};
use crate::ui::search::SymbolSearch;
use crate::ui::shortcuts;

/// What a new install starts with, so the rail is never an empty column.
///
/// Things somebody opening a charting app for the first time recognises.
/// Futures are not on the list for that reason — ES and CL are in the
/// inventory and one search away, but a front-month contract greeting a
/// first-time user says this app is not for them.
pub const DEFAULTS: &[(&str, &[&str])] = &[
    // The liquid ETFs rather than the indices themselves: they are what people
    // actually watch and trade, they carry real volume so the profile and the
    // volume pane have something to show, and an index has neither.
    ("Indexes", &["SPY", "QQQ", "DIA"]),
    ("US Stocks", &["NVDA", "AAPL", "MSFT", "AMZN", "META", "GOOGL", "TSLA", "AMD", "SHOP"]),
    ("Currencies", &["EURUSD", "GBPUSD", "AUDUSD"]),
    ("Crypto", &["BTC", "ETH", "SOL"]),
];

/// Move a section's symbols into a watchlist of their own, and return it.
///
/// Apart from the rail because it is all the part that can go wrong, and the
/// rail needs a window to exist. `None` when the watchlist could not be made,
/// which leaves the section exactly where it was.
///
/// The order is the point: the symbols move first and the section goes last,
/// so an interrupted promotion can leave them in both places and never in
/// neither.
fn promote_section_in(store: &Store, from: i64, section: i64, name: &str) -> Option<i64> {
    let entries: Vec<Entry> = store
        .watchlist_sections(from)
        .into_iter()
        .find(|candidate| candidate.id == section)
        .map(|candidate| candidate.entries)?;

    let watchlist = store.add_watchlist(name)?;
    let root = store.root_section(watchlist);
    for entry in &entries {
        store.move_entry_to_section(section, root, entry, None);
    }
    store.remove_section(section);
    Some(watchlist)
}

/// The watchlist `delta` steps along from `current`, wrapping at both ends.
///
/// `None` when there is nowhere to go: one watchlist has nothing to rotate
/// to, and a `current` that is not in the list at all is a rail pointed at
/// something that has been deleted, which a keystroke should not quietly
/// resolve by jumping somewhere arbitrary.
fn next_in_rotation(ids: &[i64], current: i64, delta: i32) -> Option<i64> {
    if ids.len() < 2 {
        return None;
    }
    let at = ids.iter().position(|id| *id == current)? as i32;
    let next = (at + delta).rem_euclid(ids.len() as i32) as usize;
    Some(ids[next])
}

/// The watchlist a number key names: 1 is the first the switcher lists.
///
/// `None` for zero, which names nothing, and past the last list, which a
/// keystroke should leave alone rather than clamp to the end.
fn numbered(ids: &[i64], n: usize) -> Option<i64> {
    ids.get(n.checked_sub(1)?).copied()
}

/// The last close and the move onto it, when the cache holds enough to say.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Quote {
    pub last: f64,
    pub change: f64,
    pub change_pct: f64,
}

pub type QuoteLookup = Rc<dyn Fn(&Instrument) -> Option<Quote>>;

/// A column of the rail.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Column {
    Symbol,
    Last,
    Change,
    ChangePct,
}

impl Column {
    pub const ALL: [Column; 4] = [Column::Symbol, Column::Last, Column::Change, Column::ChangePct];
    /// What a fresh install shows: what it costs now, and how far it has
    /// moved. The absolute change is the one of the three you can work out
    /// from the other two.
    pub const DEFAULT: [Column; 3] = [Column::Symbol, Column::Last, Column::ChangePct];

    /// The columns shipped before, kept so a rail still carrying them can be
    /// moved on without overriding a choice somebody actually made.
    const PREVIOUS_DEFAULT: [Column; 3] = [Column::Symbol, Column::Change, Column::ChangePct];

    pub fn key(self) -> &'static str {
        match self {
            Column::Symbol => "symbol",
            Column::Last => "last",
            Column::Change => "change",
            Column::ChangePct => "change_pct",
        }
    }

    pub fn from_key(key: &str) -> Option<Column> {
        Column::ALL.into_iter().find(|c| c.key() == key)
    }

    /// What the column's header says.
    fn title(self) -> &'static str {
        match self {
            Column::Symbol => "Symbol",
            Column::Last => "Last",
            Column::Change => "Chg",
            Column::ChangePct => "Chg %",
        }
    }

    /// What the column is called where there is room to say it in full.
    fn label(self) -> &'static str {
        match self {
            Column::Symbol => "Symbol",
            Column::Last => "Last price",
            Column::Change => "Change",
            Column::ChangePct => "Change %",
        }
    }

    fn width_chars(self) -> i32 {
        match self {
            Column::Symbol => 0,
            Column::Last => 10,
            Column::Change => 9,
            Column::ChangePct => 7,
        }
    }
}

impl crate::ui::part_list::Part for Column {
    fn every() -> Vec<Self> {
        Column::ALL.to_vec()
    }
    fn key(self) -> &'static str {
        Column::key(self)
    }
    fn label(self) -> &'static str {
        Column::label(self)
    }
    fn fixed(self) -> bool {
        self == Column::Symbol
    }
}

/// Parse the stored column list, falling back to the default.
///
/// The symbol column is always present and always first: a row without it is
/// not a row, and no amount of configuration should let you hide what you are
/// looking at.
pub fn parse_columns(stored: Option<&str>) -> Vec<Column> {
    let mut columns: Vec<Column> = stored
        .map(|s| s.split(',').filter_map(|k| Column::from_key(k.trim())).collect())
        .unwrap_or_default();
    if columns.is_empty() {
        columns = Column::DEFAULT.to_vec();
    }
    // A rail still showing the old default was never configured — it was
    // simply never touched. Move it on rather than leaving it behind.
    if columns == Column::PREVIOUS_DEFAULT {
        columns = Column::DEFAULT.to_vec();
    }
    columns.retain(|c| *c != Column::Symbol);
    let mut out = vec![Column::Symbol];
    out.extend(columns);
    out
}

pub fn columns_to_string(columns: &[Column]) -> String {
    columns.iter().map(|c| c.key()).collect::<Vec<_>>().join(",")
}

/// How a watchlist's rows are ordered within each section, when not in the
/// order they were put there.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Sort {
    pub column: Column,
    pub descending: bool,
}

impl Sort {
    /// Written as the column's key, with `:desc` when it runs high to low.
    pub fn key(self) -> String {
        match self.descending {
            true => format!("{}:desc", self.column.key()),
            false => self.column.key().to_string(),
        }
    }

    pub fn from_key(key: &str) -> Option<Sort> {
        let (column, descending) = match key.strip_suffix(":desc") {
            Some(column) => (column, true),
            None => (key, false),
        };
        Column::from_key(column).map(|column| Sort { column, descending })
    }

    /// The next state a click on `column`'s header moves to: up, then down,
    /// then back to the order the list was written in.
    fn after_click(current: Option<Sort>, column: Column) -> Option<Sort> {
        match current {
            Some(sort) if sort.column == column && !sort.descending => {
                Some(Sort { column, descending: true })
            }
            Some(sort) if sort.column == column => None,
            _ => Some(Sort { column, descending: false }),
        }
    }

    /// What a row sorts on, worked out once for each of them.
    ///
    /// A quote is a read of the bar cache — a query and a whole series
    /// decoded — so asking for one inside the comparator costs the 2·n·log n
    /// reads a sort makes rather than the n there are rows: 3.45ms against
    /// 1.44ms for a 200-row list, on the main loop, every time a quote
    /// arrives. It also keeps the comparison pure, and a comparator whose
    /// answers can change part-way through a sort is one the sort may panic
    /// on — which in here would take the window with it.
    fn rank(self, instrument: &Instrument, quote: &QuoteLookup) -> Rank {
        match self.column {
            Column::Symbol => Rank::Name(instrument.display_symbol()),
            Column::Last => Rank::Price(quote(instrument).map(|q| q.last)),
            Column::Change => Rank::Price(quote(instrument).map(|q| q.change)),
            Column::ChangePct => Rank::Price(quote(instrument).map(|q| q.change_pct)),
        }
    }

    /// Order two rows. A row with no price yet sorts last whichever way the
    /// column runs: a dash is not a small number.
    fn compare(self, a: &Rank, b: &Rank) -> std::cmp::Ordering {
        let ordering = match (a, b) {
            (Rank::Name(a), Rank::Name(b)) => a.cmp(b),
            (Rank::Price(Some(x)), Rank::Price(Some(y))) => x.total_cmp(y),
            (Rank::Price(Some(_)), Rank::Price(None)) => return std::cmp::Ordering::Less,
            (Rank::Price(None), Rank::Price(Some(_))) => return std::cmp::Ordering::Greater,
            _ => std::cmp::Ordering::Equal,
        };
        if self.descending { ordering.reverse() } else { ordering }
    }
}

/// What one row of a sorted list is held against the others by.
enum Rank {
    Name(String),
    Price(Option<f64>),
}

/// A column on its way to another place in the header.
///
/// A type of its own rather than the column's key as a string, because a
/// drop target matches on the type it is offered: the symbols below take a
/// string, and sharing one with them lit every row up as somewhere a column
/// could be dropped. Nothing but the header accepts this, and the header
/// accepts nothing else.
#[derive(Clone, Copy, glib::Boxed)]
#[boxed_type(name = "OmachartsColumnDrag")]
struct ColumnDrag(Column);

/// Which gap between the column titles `x` falls in, counted in columns.
fn gap_at(header: &gtk::Box, x: f64) -> usize {
    let mut at = 0;
    let mut child = header.first_child();
    while let Some(widget) = child {
        child = widget.next_sibling();
        let middle = widget.allocation().x() as f64 + widget.width() as f64 / 2.0;
        if x < middle {
            break;
        }
        at += 1;
    }
    at
}

/// Where the line stands for the `at`th gap: against the near edge of the
/// title that would follow it, or past the last one at the end of the row.
fn gap_x(header: &gtk::Box, at: usize) -> i32 {
    let mut seen = 0;
    let mut last = 0;
    let mut child = header.first_child();
    while let Some(widget) = child {
        child = widget.next_sibling();
        if seen == at {
            return (widget.allocation().x() - 3).max(0);
        }
        last = widget.allocation().x() + widget.width();
        seen += 1;
    }
    // Past the last title is the rail's own edge, so the line stands just
    // inside it rather than half off the end.
    (last - 2).max(0)
}

/// Halfway between two rows, measured from their middles rather than from
/// the edges between them.
///
/// The edges are the layout's business — padding, margins and a row's own
/// box all land on them — and a line halfway between two of those sat six
/// pixels above where the eye says the middle is. Halfway between the two
/// rows' centres is the middle of the two symbols, which is what the line
/// is claiming to be.
fn between(within: &gtk::Overlay, above: &gtk::ListBoxRow, below: &gtk::ListBoxRow) -> i32 {
    let middle = |row: &gtk::ListBoxRow| match row.compute_bounds(within) {
        Some(box_) => box_.y() + box_.height() / 2.0,
        None => (row.allocation().y() + row.height() / 2) as f32,
    };
    ((middle(above) + middle(below)) / 2.0).round() as i32
}

/// A row's top and bottom in the overlay the line is drawn on.
fn within(overlay: &gtk::Overlay, row: &gtk::ListBoxRow) -> (i32, i32) {
    match row.compute_bounds(overlay) {
        Some(box_) => (box_.y().round() as i32, (box_.y() + box_.height()).round() as i32),
        None => (row.allocation().y(), row.allocation().y() + row.height()),
    }
}

/// How far a press may travel and still be a click rather than a drag.
const CLICK_SLOP: f64 = 4.0;

/// The columns after `moving` is dropped in the `at`th gap, or nothing when
/// that is the gap it already sits in.
fn dropped(columns: &[Column], moving: Column, at: usize) -> Option<Vec<Column>> {
    // The symbol column does not move, and nothing lands in front of it:
    // a drop over on that side takes the nearest place that is really a
    // place, which is straight after it.
    if moving == Column::Symbol {
        return None;
    }
    let at = at.max(1);
    let from = columns.iter().position(|c| *c == moving)?;
    // The gaps are counted among the columns as they stand, so a gap after
    // the one being moved loses a place once it is taken out.
    let to = if at > from { at - 1 } else { at };
    if to == from {
        return None;
    }
    let mut columns = columns.to_vec();
    let column = columns.remove(from);
    columns.insert(to.min(columns.len()), column);
    Some(columns)
}

/// Where a watchlist's sort is kept. Per list, because a list of movers wants
/// sorting by change and a list of holdings wants the order it was built in.
fn sort_setting(watchlist: i64) -> String {
    format!("watchlist_sort_{watchlist}")
}

pub fn stored_sort(store: &Store, watchlist: i64) -> Option<Sort> {
    store.setting(&sort_setting(watchlist)).as_deref().and_then(Sort::from_key)
}

pub fn store_sort(store: &Store, watchlist: i64, sort: Option<Sort>) {
    store.set_setting(&sort_setting(watchlist), &sort.map(Sort::key).unwrap_or_default());
}

/// The section Enter or Space folds or unfolds from this row, and whether it
/// ends up folded; `None` leaves the key to the list.
fn fold_target(on: &RowKind) -> Option<(i64, bool)> {
    match on {
        RowKind::Header { section_id, collapsed } => Some((*section_id, !collapsed)),
        RowKind::Entry { .. } => None,
    }
}

/// What a row in the list is.
#[derive(Clone)]
enum RowKind {
    Header { section_id: i64, collapsed: bool },
    Entry {
        section_id: i64,
        entry: Entry,
        instrument: Instrument,
        /// The value labels, so a new quote can be written straight into them.
        cells: Vec<(Column, gtk::Label)>,
    },
}

impl RowKind {
    fn section_id(&self) -> i64 {
        match self {
            RowKind::Header { section_id, .. } | RowKind::Entry { section_id, .. } => *section_id,
        }
    }

    fn instrument(&self) -> Option<&Instrument> {
        match self {
            RowKind::Entry { instrument, .. } => Some(instrument),
            RowKind::Header { .. } => None,
        }
    }
}

/// Which section a new symbol belongs in.
///
/// Beside whatever is highlighted. The rail is one list of several sections,
/// so adding always to the root puts the symbol at the top of the rail, a long
/// way from the Energy symbol you were looking at when you asked for it.
/// Section titles are rows like any other and carry their own section, which
/// is what makes highlighting a title and adding put the symbol inside that
/// section rather than above it.
///
/// `sections` is the section each row belongs to, in list order. Nothing
/// highlighted leaves nothing to infer from, so it falls to the root.
fn section_for_new_symbol(sections: &[i64], selected: Option<usize>, root: i64) -> i64 {
    selected.and_then(|at| sections.get(at)).copied().unwrap_or(root)
}

pub struct Watchlist {
    pub widget: gtk::Box,
    list: gtk::ListBox,
    /// Why the prices are dashes, shown only while they are.
    trouble: gtk::Label,
    store: Rc<Store>,
    index: crate::inventory::Inventory,
    quote: QuoteLookup,
    search: Rc<SymbolSearch>,
    /// What a picked symbol is handed to. `false` when no chart took it.
    on_pick: Rc<dyn Fn(Instrument) -> bool>,
    /// What the last pick was answered, for [`pick`](Self::pick) to hand
    /// back: a selection reaches `on_pick` through the list's own signal,
    /// which has nowhere to return it.
    taken: Cell<bool>,
    /// Parallel to the list's rows.
    rows: RefCell<Vec<RowKind>>,
    columns: RefCell<Vec<Column>>,
    /// The column titles over the rows, which are also how they are sorted.
    column_header: gtk::Box,
    /// The line that says where a column being dragged would land.
    column_marker: gtk::Box,
    /// Where a symbol being dragged will be inserted.
    row_marker: gtk::Box,
    /// What that line is positioned inside. A row's allocation is in the
    /// list's coordinates and the line's margin is in the overlay's, and
    /// the list sits a few pixels into the overlay — which is exactly how
    /// far above the gap the line was landing.
    row_overlay: gtk::Overlay,
    /// Where that line is now, so motion over one row costs nothing after
    /// the first event.
    row_marked: Cell<i32>,
    /// How the list on screen is sorted, read from the store on every rebuild.
    sort: Cell<Option<Sort>>,
    /// Set while we are selecting a row ourselves, so rebuilding does not
    /// re-load the chart.
    quiet: Cell<bool>,
    /// Which watchlist the rail is showing. Not which one the bar widget
    /// shows — that is always the default one.
    active: Cell<i64>,
    /// Steps out to every watchlist, and back in.
    switcher: gtk::Button,
    /// The watchlist on screen by name, or "Watchlists" on the page of them.
    title: gtk::Label,
    /// Adds to the page on screen: a symbol or a section, or a list.
    add: gtk::Button,
    /// The rail's two pages: one list's symbols, and every list.
    pages: gtk::Stack,
    /// Every watchlist, one row each, when the rail has stepped out.
    lists: gtk::ListBox,
    /// The watchlist on each of those rows, parallel to them.
    list_ids: RefCell<Vec<i64>>,
    /// The empty state's first offer, when that is all the rail holds.
    /// Somewhere for the keyboard to land on a list with no rows in it.
    empty_focus: RefCell<Option<gtk::Button>>,
    /// Told whenever the rail starts showing a different watchlist, so the
    /// window can write the choice down against the chartbook it belongs to.
    /// One hook for every way of switching, because a second route to the
    /// same state is a second route that can forget to save.
    on_switch: RefCell<Option<Rc<dyn Fn()>>>,
    /// Asked which chartbook is using a watchlist, so the page of lists can
    /// refuse one that is spoken for. The chartbooks live in the window; the
    /// rail only knows how to ask about one list at a time.
    using: RefCell<Option<Rc<dyn Fn(i64) -> Option<String>>>>,
}

const SETTING_COLUMNS: &str = "watchlist_columns";
pub(crate) const SETTING_ACTIVE: &str = "active_watchlist";

/// The rail's pages: one list's symbols, and every list.
const PAGE_SYMBOLS: &str = "symbols";
const PAGE_LISTS: &str = "lists";

/// What the band calls the page of lists.
const LISTS_TITLE: &str = "Watchlists";

/// What the band's + makes on a list's symbols, and on the page of lists.
const ADD_TIP: &str = "Add a symbol or a section";
const NEW_LIST_TIP: &str = "New watchlist";

impl Watchlist {
    pub fn new(
        store: Rc<Store>,
        index: crate::inventory::Inventory,
        search: Rc<SymbolSearch>,
        quote: QuoteLookup,
        on_pick: impl Fn(Instrument) -> bool + 'static,
    ) -> Rc<Watchlist> {
        let list = gtk::ListBox::new();
        list.set_selection_mode(gtk::SelectionMode::Single);
        list.add_css_class("navigation-sidebar");

        // The same mark again, lying down: where a symbol being dragged will
        // be inserted. Over the list rather than between the rows, so the
        // rows do not open a gap and shuffle while one is being carried.
        let row_marker = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        row_marker.add_css_class("row-drop");
        row_marker.set_size_request(-1, 2);
        row_marker.set_margin_start(8);
        row_marker.set_margin_end(8);
        row_marker.set_halign(gtk::Align::Fill);
        row_marker.set_valign(gtk::Align::Start);
        row_marker.set_can_target(false);
        row_marker.set_visible(false);

        let rows_under_mark = gtk::Overlay::new();
        rows_under_mark.set_child(Some(&list));
        rows_under_mark.add_overlay(&row_marker);

        let scroller = gtk::ScrolledWindow::new();
        scroller.set_child(Some(&rows_under_mark));
        scroller.set_vexpand(true);
        scroller.set_hscrollbar_policy(gtk::PolicyType::Never);

        // The band across the top of the rail: nothing in it but its height,
        // which `.rail-header` sets. It used to hold the column headings —
        // "Symbol", "Last", "Chg%" — which said nothing a row did not already
        // say for itself, so now it only steps down from under the window's
        // corner controls and stops a few pixels later.
        let header = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        header.add_css_class("rail-header");

        // The step `.rail-header` takes is 37 pixels of nothing, and the
        // corner controls only cover its right-hand end. Naming the watchlist
        // in what is left costs no height at all, which is the only reason
        // the rail can be told what it is showing without giving up a row.
        //
        // The button before the name steps the rail out to every watchlist,
        // in place of the symbols, rather than opening a menu over them: a
        // menu of a dozen lists ran taller than the rail and carried a rename
        // and a delete on every row, and the rail already knows how to show a
        // list of rows and walk it with the arrows. A button that looks like
        // one, because a name that brightened under the pointer was not
        // something anybody guessed they could click.
        let switcher = gtk::Button::from_icon_name("view-list-symbolic");
        switcher.add_css_class("flat");
        switcher.add_css_class("rail-switcher");
        switcher.set_valign(gtk::Align::Center);
        switcher.set_tooltip_text(Some("All watchlists"));

        // Ellipsized at a width that clears the corner controls, instead of
        // growing under them.
        let title = gtk::Label::new(None);
        title.add_css_class("rail-title");
        title.set_valign(gtk::Align::Center);
        title.set_ellipsize(gtk::pango::EllipsizeMode::End);
        title.set_max_width_chars(14);
        title.set_xalign(0.0);

        // The button's edge on the rows' edge: a row sits 6px in, Adwaita's
        // sidebar row margin. Down the middle of the band below the 7px the
        // corner controls also step down (see `.window-corner`), which puts
        // the name on the line a chart's own symbol and resolutions are on.
        let named = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        named.set_halign(gtk::Align::Start);
        named.set_valign(gtk::Align::Center);
        named.set_margin_start(6);
        named.set_margin_top(7);
        named.append(&switcher);
        named.append(&title);

        // Adding, at the other end of the band from the name and clear of
        // the corner controls, where a sidebar keeps its "new". It used to be
        // a strip of its own at the foot of the rail, which cost a row's
        // height and sat where nobody was looking.
        let add = gtk::Button::from_icon_name("list-add-symbolic");
        add.add_css_class("flat");
        add.add_css_class("rail-add");
        add.set_tooltip_text(Some(ADD_TIP));
        add.set_halign(gtk::Align::End);
        add.set_valign(gtk::Align::Center);
        add.set_margin_top(7);
        add.set_margin_end(crate::ui::window::RAIL_ADD_END);

        let band = gtk::Overlay::new();
        band.set_child(Some(&header));
        band.add_overlay(&named);
        band.add_overlay(&add);

        // A column of dashes with no explanation is the same failure as an
        // empty chart, in a quieter voice — so the rail says why, directly
        // above the first price it is missing rather than at the bottom where
        // a short list would leave it far from them. Hidden entirely when
        // there is nothing wrong, so it costs no height at all.
        let trouble = gtk::Label::new(None);
        trouble.add_css_class("caption");
        trouble.add_css_class("dim-label");
        trouble.set_wrap(true);
        trouble.set_xalign(0.0);
        trouble.set_margin_start(12);
        trouble.set_margin_end(10);
        trouble.set_margin_top(6);
        trouble.set_margin_bottom(2);
        trouble.set_visible(false);

        // The column titles, lined up over the cells: a row's box starts 26px
        // from the rail's edge, past Adwaita's 6px row margin, 8 of padding
        // and its own 12, and ends 24px from the other, past the same 6 and 8
        // and its own 10.
        let column_header = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        // Named so the stylesheet can take GTK's drop outline off this one
        // row without touching the drop feedback on the symbols below.
        column_header.add_css_class("column-headings");

        // A tick between two titles rather than a rule down the row: it says
        // where the column lands without reading as a divider between them.
        // Over the titles rather than among them, so nothing shifts while a
        // column is being carried, and never a drop target of its own.
        let column_marker = gtk::Box::new(gtk::Orientation::Vertical, 0);
        column_marker.add_css_class("column-drop");
        column_marker.set_size_request(2, 11);
        column_marker.set_halign(gtk::Align::Start);
        column_marker.set_valign(gtk::Align::Center);
        column_marker.set_can_target(false);
        column_marker.set_visible(false);

        // The margins belong to the overlay, so a title's own x is the one
        // the line is placed at.
        let columns_over_rows = gtk::Overlay::new();
        columns_over_rows.set_child(Some(&column_header));
        columns_over_rows.add_overlay(&column_marker);
        columns_over_rows.set_margin_start(26);
        columns_over_rows.set_margin_end(24);
        columns_over_rows.set_margin_top(4);
        columns_over_rows.set_margin_bottom(2);

        let symbols = gtk::Box::new(gtk::Orientation::Vertical, 0);
        symbols.append(&columns_over_rows);
        symbols.append(&trouble);
        symbols.append(&scroller);

        // Every watchlist, in the same rows as the symbols, so stepping out
        // is the same rail one level up rather than a different thing.
        let lists = gtk::ListBox::new();
        lists.add_css_class("navigation-sidebar");
        let lists_scroller = gtk::ScrolledWindow::new();
        lists_scroller.set_child(Some(&lists));
        lists_scroller.set_vexpand(true);
        lists_scroller.set_hscrollbar_policy(gtk::PolicyType::Never);
        lists_scroller.set_margin_top(6);

        let pages = gtk::Stack::new();
        pages.add_named(&symbols, Some(PAGE_SYMBOLS));
        pages.add_named(&lists_scroller, Some(PAGE_LISTS));
        pages.set_transition_type(gtk::StackTransitionType::SlideLeftRight);
        pages.set_transition_duration(120);
        pages.set_vexpand(true);

        let widget = gtk::Box::new(gtk::Orientation::Vertical, 0);
        widget.set_size_request(248, -1);
        widget.append(&band);
        widget.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
        widget.append(&pages);

        let columns = parse_columns(store.setting(SETTING_COLUMNS).as_deref());
        // A watchlist deleted in another window, or a setting from a database
        // that has been rolled back, leaves an id pointing at nothing. The
        // default is the one that is always there to fall back to.
        let active = store
            .setting(SETTING_ACTIVE)
            .and_then(|id| id.parse().ok())
            .filter(|id| store.watchlist_exists(*id))
            .unwrap_or(DEFAULT_WATCHLIST);

        let watchlist = Rc::new(Watchlist {
            widget,
            list,
            trouble,
            store,
            index,
            quote,
            search,
            on_pick: Rc::new(on_pick),
            taken: Cell::new(true),
            rows: RefCell::new(Vec::new()),
            columns: RefCell::new(columns),
            column_header: column_header.clone(),
            column_marker: column_marker.clone(),
            row_marker: row_marker.clone(),
            row_overlay: rows_under_mark.clone(),
            row_marked: Cell::new(-1),
            sort: Cell::new(None),
            quiet: Cell::new(false),
            active: Cell::new(active),
            switcher: switcher.clone(),
            add: add.clone(),
            title,
            pages,
            lists,
            list_ids: RefCell::new(Vec::new()),
            empty_focus: RefCell::new(None),
            on_switch: RefCell::new(None),
            using: RefCell::new(None),
        });


        watchlist.wire_selection();
        watchlist.wire_keys();
        watchlist.wire_click_to_focus();
        watchlist.wire_switcher();
        watchlist.wire_add();
        watchlist.wire_column_drop();
        watchlist.wire_column_menu();
        watchlist.rebuild();
        watchlist
    }

    /// What a watchlist with nothing in it says for itself.
    ///
    /// Common rather than rare now that there can be several watchlists:
    /// making one lands you here, so this is the first thing somebody sees
    /// after creating one — and what they see should be an invitation with
    /// some presence to it, not a caption apologising for the space.
    ///
    /// The keys are set as keys rather than as words. A shortcut written in
    /// running text reads as a sentence somebody has to parse; in a pill it
    /// reads as something to press, which is the whole reason for saying it
    /// here at all.
    fn empty_state(self: &Rc<Self>) -> gtk::Box {
        let state = gtk::Box::new(gtk::Orientation::Vertical, 10);
        state.set_valign(gtk::Align::Center);
        state.set_margin_start(8);
        state.set_margin_end(8);

        // What the thing is for, rather than what state it is in. Somebody
        // who has just made their first watchlist learns nothing from being
        // told it is empty — they can see that — and this is the one moment
        // the app can say what to put in it and why.
        //
        // It names both halves of what is offered below it — symbols, and
        // sections — which is what makes the pair of actions read as
        // explained rather than arbitrary. It wraps rather than being cut
        // down, because a placeholder's minimum width becomes the sidebar's
        // minimum width and the sentence is not the part that gives way.
        let sentence =
            gtk::Label::new(Some("Keep the symbols you follow here, grouped into sections."));
        sentence.add_css_class("dim-label");
        // The block exists to carry this, so it is not the smallest thing in
        // it: quieter than the two offers in tone, but not in size.
        sentence.add_css_class("rail-empty-blurb");
        sentence.set_wrap(true);
        sentence.set_justify(gtk::Justification::Center);
        sentence.set_margin_bottom(6);
        state.append(&sentence);

        let symbol = self.empty_action("Add a symbol", shortcuts::ADD_SYMBOL);
        let this = self.clone();
        symbol.connect_clicked(move |_| this.add_symbol());
        state.append(&symbol);
        *self.empty_focus.borrow_mut() = Some(symbol.clone());

        let section = self.empty_action("Add a section", shortcuts::ADD_SECTION);
        let this = self.clone();
        section.connect_clicked(move |button| this.prompt_new_section(button));
        state.append(&section);

        state
    }

    /// One of the two offers: what it does, and the key that does it.
    ///
    /// The key sits inside the button, so it belongs to the thing you click
    /// and brightens with it. It wears the same pill the keyboard-shortcuts
    /// window uses, which is the app's one way of saying "this is a key".
    ///
    /// The verb gives way on a narrow rail and the key never does: half a
    /// shortcut is worse than no shortcut, and the rail can be dragged down
    /// to a hundred and sixty pixels.
    fn empty_action(&self, verb: &str, accel: &str) -> gtk::Button {
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        row.set_halign(gtk::Align::Center);

        let label = gtk::Label::new(Some(verb));
        label.set_ellipsize(gtk::pango::EllipsizeMode::End);
        row.append(&label);

        if let Some(key) = shortcuts::accel_label(accel) {
            let key = gtk::Label::new(Some(&key));
            key.add_css_class("keycap");
            row.append(&key);
        }

        let button = gtk::Button::new();
        button.set_child(Some(&row));
        button.add_css_class("flat");
        button.add_css_class("rail-empty-action");
        button.set_hexpand(true);
        button
    }

    /// Which watchlist the rail is showing.
    pub fn active_watchlist(&self) -> i64 {
        self.active.get()
    }

    /// Show a different watchlist. An id that names nothing is ignored rather
    /// than emptying the rail — a chartbook can outlive the watchlist it was
    /// pointed at, and the rail is not the place to find that out.
    pub fn set_active_watchlist(self: &Rc<Self>, id: i64) {
        if !self.store.watchlist_exists(id) {
            return;
        }
        // However it was asked for — a list picked, a number key, a command
        // — a list asked for is a list to look at.
        self.show_symbols();
        if id == self.active.get() {
            return;
        }
        self.active.set(id);
        self.store.set_setting(SETTING_ACTIVE, &id.to_string());
        self.rebuild();
        self.announce_switch();
    }

    /// Answer which chartbook is using a watchlist, for the page of lists.
    ///
    /// Set by the window, because that is where the chartbooks are. Asked
    /// each time the page is filled rather than remembered, so a chartbook
    /// renamed since the page last opened is named as it is called now.
    pub fn connect_chartbook_using(&self, using: impl Fn(i64) -> Option<String> + 'static) {
        *self.using.borrow_mut() = Some(Rc::new(using));
    }

    /// The chartbook using this watchlist, when it is not this rail's own.
    ///
    /// The list on screen belongs to the chartbook on screen by definition, so
    /// it is never reported as spoken for — not even where a book written
    /// before one list meant one book still claims it as well.
    fn chartbook_using(&self, id: i64) -> Option<String> {
        if id == self.active.get() {
            return None;
        }
        let ask = self.using.borrow().clone();
        ask.and_then(|ask| ask(id))
    }

    /// Select a symbol the way a click does, so whatever is listening for a
    /// pick hears about it.
    ///
    /// Not [`highlight`](Self::highlight), which is deliberately silent: that
    /// one exists for the other direction, a chart moving and the rail
    /// following, and using it here would light up a row and move nothing.
    ///
    /// `None` when the list does not hold it, otherwise whether a chart took
    /// it.
    pub fn pick(self: &Rc<Self>, instrument: &Instrument) -> Option<bool> {
        let at = self.row_of(instrument)?;
        let row = self.list.row_at_index(at as i32)?;
        // Selecting what is already selected emits nothing, so the pick has
        // to be made by hand — otherwise clicking the symbol you are already
        // on in the bar widget would do nothing at all.
        if self.list.selected_row().as_ref() == Some(&row) {
            return Some((self.on_pick)(instrument.clone()));
        }
        self.taken.set(true);
        self.list.select_row(Some(&row));
        Some(self.taken.get())
    }

    /// Put the highlight back on `instrument`, for a pick no chart took, so
    /// the list does not claim a symbol no chart is showing. On no row when
    /// the list does not hold it, or there is nothing to point at.
    pub fn put_back(&self, instrument: Option<&Instrument>) {
        let row = instrument
            .and_then(|instrument| self.row_of(instrument))
            .and_then(|at| self.list.row_at_index(at as i32));
        match row {
            Some(row) => self.select_quietly(&row),
            None => {
                self.quiet.set(true);
                self.list.unselect_all();
                self.quiet.set(false);
            }
        }
    }

    fn row_of(&self, instrument: &Instrument) -> Option<usize> {
        self.rows.borrow().iter().position(|kind| match kind {
            RowKind::Entry { instrument: i, .. } => {
                i.symbol == instrument.symbol && i.suffix == instrument.suffix
            }
            RowKind::Header { .. } => false,
        })
    }

    pub fn connect_switched(&self, on_switch: impl Fn() + 'static) {
        *self.on_switch.borrow_mut() = Some(Rc::new(on_switch));
    }

    /// Borrowed out before it is called: switching rebuilds the rail, and a
    /// handler that touched it while the borrow was held would panic.
    fn announce_switch(&self) {
        let handler = self.on_switch.borrow().clone();
        if let Some(handler) = handler {
            handler();
        }
    }

    /// Walk to the watchlist before or after this one, round the ends.
    ///
    /// Through `set_active_watchlist` like every other way of switching, so
    /// the choice is written down against the chartbook rather than only
    /// being on screen.
    pub fn rotate_watchlist(self: &Rc<Self>, delta: i32) {
        let ids: Vec<i64> = self.watchlists().into_iter().map(|(id, _)| id).collect();
        if let Some(next) = next_in_rotation(&ids, self.active.get(), delta) {
            self.set_active_watchlist(next);
        }
    }

    /// Show the `n`th watchlist, counting from 1 in the switcher's order.
    ///
    /// False when there is no such list, so the key is left for somebody
    /// else. Through `set_active_watchlist`, like the rotation above.
    pub fn show_numbered(self: &Rc<Self>, n: usize) -> bool {
        let ids: Vec<i64> = self.watchlists().into_iter().map(|(id, _)| id).collect();
        let Some(id) = numbered(&ids, n) else { return false };
        self.set_active_watchlist(id);
        true
    }

    /// Every watchlist, in display order, as id and name.
    pub fn watchlists(&self) -> Vec<(i64, String)> {
        self.store.watchlists()
    }

    /// Put the keyboard on the rail, landing on something selected so the
    /// arrows have somewhere to go from.
    pub fn grab_focus(self: &Rc<Self>) {
        if self.showing_lists() {
            return self.focus_lists();
        }
        let row = self.list.selected_row().or_else(|| {
            (0..)
                .map_while(|i| self.list.row_at_index(i))
                .find(|row| row.is_selectable() && row.is_visible())
        });
        if let Some(row) = row {
            self.list.select_row(Some(&row));
            row.grab_focus();
        } else if let Some(offer) = self.empty_focus.borrow().as_ref() {
            // With no rows there is nothing to land on, and a rail the
            // keyboard cannot reach is a rail whose shortcuts do not work —
            // including the two the empty state has just advertised.
            offer.grab_focus();
        } else {
            self.list.grab_focus();
        }
    }

    /// Put the keyboard in the rail without disturbing what is highlighted.
    ///
    /// `grab_focus` lands on a row and highlights it, and a highlight is what
    /// drives the chart — so it is the wrong thing to do when all that
    /// happened is a click on the rail. The shortcuts only need the keyboard
    /// to be in here; they do not need anything selected.
    fn take_focus(self: &Rc<Self>) {
        if self.showing_lists() {
            self.focus_lists();
        } else if let Some(row) = self.list.selected_row() {
            row.grab_focus();
        } else if let Some(offer) = self.empty_focus.borrow().as_ref() {
            offer.grab_focus();
        } else {
            self.list.grab_focus();
        }
    }

    /// A click anywhere in the rail puts the keyboard in it.
    ///
    /// The rail owns shortcuts of its own — a new section, rotating through
    /// the watchlists — and they ask whether the keyboard is in here before
    /// they do anything. Clicking a symbol focused the rail as a side effect
    /// of selecting it, but clicking the empty space below the last row, or
    /// the header, left the keyboard wherever it was, so the shortcuts the
    /// empty state advertises did nothing.
    ///
    /// Caught on the way down, because the list claims a press for its own
    /// selection and a gesture waiting on the way up would never hear about
    /// it. Taking focus first is harmless: it changes no selection, and the
    /// press carries on to whatever it was going to do anyway.
    fn wire_click_to_focus(self: &Rc<Self>) {
        let click = gtk::GestureClick::new();
        click.set_propagation_phase(gtk::PropagationPhase::Capture);
        let this = self.clone();
        click.connect_pressed(move |_, _, _, _| {
            if !this.has_focus() {
                this.take_focus();
            }
        });
        self.widget.add_controller(click);
    }

    /// Does the keyboard currently live here?
    pub fn has_focus(&self) -> bool {
        // Whether the keyboard is in the rail now, which is not the same
        // question as `focus_child`, which answers what held it last.
        self.widget.state_flags().contains(gtk::StateFlags::FOCUS_WITHIN)
    }

    pub fn columns(&self) -> Vec<Column> {
        self.columns.borrow().clone()
    }

    pub fn set_columns(self: &Rc<Self>, columns: Vec<Column>) {
        let columns = parse_columns(Some(&columns_to_string(&columns)));
        self.store.set_setting(SETTING_COLUMNS, &columns_to_string(&columns));
        *self.columns.borrow_mut() = columns;
        self.rebuild();
    }

    /// Every symbol on the rail, in the order the arrow keys walk them.
    ///
    /// Collapsed sections are excluded: what you cannot see, you cannot arrow
    /// onto, and prefetching it would spend the request budget on rows that
    /// are not there.
    ///
    /// The row's own flag, not whether it is on screen: with the rail closed
    /// every row is off screen, and the chart still walks the list.
    pub fn flat_order(&self) -> Vec<Instrument> {
        self.walked_rows().into_iter().map(|(_, instrument)| instrument).collect()
    }

    /// `flat_order`, with the index of the row each symbol is on.
    fn walked_rows(&self) -> Vec<(usize, Instrument)> {
        self.rows
            .borrow()
            .iter()
            .enumerate()
            .filter_map(|(at, kind)| match kind {
                RowKind::Entry { instrument, .. } => {
                    let visible = self
                        .list
                        .row_at_index(at as i32)
                        .map(|row| row.get_visible())
                        .unwrap_or(false);
                    visible.then(|| (at, instrument.clone()))
                }
                RowKind::Header { .. } => None,
            })
            .collect()
    }

    /// `flat_order`, and which of its symbols a step from the chart starts
    /// at.
    ///
    /// The highlighted row when it holds what the chart shows, so a symbol
    /// listed twice steps on from the copy the chart reached rather than the
    /// first; then the first copy of it; then the highlighted row anyway,
    /// for a symbol picked from search or sitting in a folded section,
    /// which then steps on from where the list was left instead of from its
    /// top. `None` only with nothing to go on.
    pub fn step_order(
        &self,
        current: Option<&Instrument>,
    ) -> (Vec<Instrument>, Option<usize>) {
        let walked = self.walked_rows();
        let selected = self.list.selected_row().map(|row| row.index().max(0) as usize);
        let from = step_from(&walked, selected, current);
        (walked.into_iter().map(|(_, instrument)| instrument).collect(), from)
    }

    /// Highlight the `at`th symbol of `flat_order`: the row itself, which
    /// for a symbol listed twice `highlight` cannot tell apart.
    pub fn highlight_walked(&self, at: usize) {
        let Some(&(row, _)) = self.walked_rows().get(at) else { return };
        let Some(row) = self.list.row_at_index(row as i32) else { return };
        self.select_quietly(&row);
    }

    /// Highlight a symbol without loading it again.
    pub fn highlight(self: &Rc<Self>, instrument: &Instrument) {
        // Already on it, perhaps on its second copy: moving to the first
        // would send the next step back up the list.
        let on_it = self.list.selected_row().is_some_and(|row| {
            matches!(
                self.rows.borrow().get(row.index().max(0) as usize),
                Some(RowKind::Entry { instrument: i, .. })
                    if i.symbol == instrument.symbol && i.suffix == instrument.suffix
            )
        });
        if on_it {
            return;
        }
        let Some(at) = self.row_of(instrument) else { return };
        let Some(row) = self.list.row_at_index(at as i32) else { return };
        self.select_quietly(&row);
    }

    /// Select `row` without loading it again, and scroll it into view.
    fn select_quietly(&self, row: &gtk::ListBoxRow) {
        self.quiet.set(true);
        self.list.select_row(Some(row));
        self.quiet.set(false);
        self.reveal(row);
    }

    /// Scroll the rail until `row` is in view. A row the keyboard is on
    /// needs nothing, since the viewport follows the focus; a row only
    /// selected, as a chart moving leaves it, is followed by nothing.
    fn reveal(&self, row: &gtk::ListBoxRow) {
        let Some(scroller) =
            self.list.ancestor(gtk::ScrolledWindow::static_type()).and_downcast::<gtk::ScrolledWindow>()
        else {
            return;
        };
        // Measured against what the viewport scrolls rather than against the
        // list, which need not start at its top.
        let Some(content) = scroller.child().and_then(|viewport| viewport.first_child()) else { return };
        let Some(bounds) = row.compute_bounds(&content) else { return };
        let top = f64::from(bounds.y());
        scroller.vadjustment().clamp_page(top, top + f64::from(bounds.height()));
    }

    fn wire_selection(self: &Rc<Self>) {
        let this = self.clone();
        self.list.connect_row_selected(move |_, row| {
            if this.quiet.get() {
                return;
            }
            let Some(row) = row else { return };
            let at = row.index().max(0) as usize;
            let picked = match this.rows.borrow().get(at) {
                Some(RowKind::Entry { instrument, .. }) => Some(instrument.clone()),
                _ => None,
            };
            if let Some(instrument) = picked {
                this.taken.set((this.on_pick)(instrument));
            }
        });
    }

    /// Plain arrows move a symbol at a time, past open sections' headers —
    /// `step_row`, below. Ctrl with an arrow jumps a whole section, for a
    /// long rail.
    fn wire_keys(self: &Rc<Self>) {
        let keys = gtk::EventControllerKey::new();
        let this = self.clone();
        keys.connect_key_pressed(move |_, key, _, state| {
            let ctrl = state.contains(gtk::gdk::ModifierType::CONTROL_MASK);
            if !ctrl {
                return glib::Propagation::Proceed;
            }
            let forward = match key {
                gtk::gdk::Key::Down => true,
                gtk::gdk::Key::Up => false,
                _ => return glib::Propagation::Proceed,
            };
            this.jump_section(forward);
            glib::Propagation::Stop
        });
        self.list.add_controller(keys);

        // ↑ and ↓ step over the headers of open sections, and Enter or Space
        // flips the section header the keyboard is on. ← and → are left to
        // pass: they pan the chart wherever the keyboard is. Caught on the way down,
        // before the list's own Enter and Space, which would otherwise spend the
        // key activating a header that does nothing when activated.
        let keys = gtk::EventControllerKey::new();
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        let this = self.clone();
        keys.connect_key_pressed(move |_, key, _, state| {
            use gtk::gdk::{Key, ModifierType};
            let held = ModifierType::CONTROL_MASK
                | ModifierType::ALT_MASK
                | ModifierType::SHIFT_MASK
                | ModifierType::SUPER_MASK;
            if state.intersects(held) {
                return glib::Propagation::Proceed;
            }
            match key {
                Key::Up | Key::KP_Up => glib::Propagation::from(this.step_row(false)),
                Key::Down | Key::KP_Down => glib::Propagation::from(this.step_row(true)),
                Key::Return | Key::KP_Enter | Key::space => {
                    glib::Propagation::from(this.fold_from_keyboard())
                }
                _ => glib::Propagation::Proceed,
            }
        });
        self.list.add_controller(keys);

        // Delete takes the highlighted symbol off the rail.
        let keys = gtk::EventControllerKey::new();
        let this = self.clone();
        keys.connect_key_pressed(move |_, key, _, _| {
            if !matches!(key, gtk::gdk::Key::Delete | gtk::gdk::Key::KP_Delete) {
                return glib::Propagation::Proceed;
            }
            this.remove_selected();
            glib::Propagation::Stop
        });
        self.list.add_controller(keys);

        // New, for the two things a rail is made of: plain for a symbol,
        // Shift for the section that holds them.
        //
        // Only the section half is here. Ctrl+N is an application accelerator
        // and those are dispatched above the focused widget, so the key never
        // reaches the rail at all — the window asks `add_symbol_if_focused`
        // instead. Nothing claims Ctrl+Shift+N, so this one can simply be
        // listened for. On the rail's own widget rather than on the list,
        // because the keyboard being anywhere in the rail is the gate, and an
        // event arriving here is that gate rather than a second reading of it.
        let keys = gtk::EventControllerKey::new();
        let this = self.clone();
        keys.connect_key_pressed(move |_, key, _, state| {
            let wanted = gtk::gdk::ModifierType::CONTROL_MASK | gtk::gdk::ModifierType::SHIFT_MASK;
            if !matches!(key, gtk::gdk::Key::n | gtk::gdk::Key::N)
                || !state.contains(wanted)
            {
                return glib::Propagation::Proceed;
            }
            this.prompt_new_section(&this.list);
            glib::Propagation::Stop
        });
        self.widget.add_controller(keys);
    }

    /// Up or down a row, past the header of a section that is open: its
    /// symbols are what the arrows are for, and the header would be a stop
    /// that charts nothing. A folded section's header is still a stop,
    /// because it is the only row the section has, and the one Enter unfolds.
    fn step_row(self: &Rc<Self>, forward: bool) -> bool {
        // Only when a row itself has the keyboard, as with folding.
        let Some(row) = self.list.root().and_then(|root| root.focus()).and_downcast::<gtk::ListBoxRow>()
        else {
            return false;
        };
        let at = row.index().max(0) as usize;
        let rows = self.rows.borrow();
        let stop = |i: &usize| {
            let shown = self.list.row_at_index(*i as i32).is_some_and(|row| row.get_visible());
            let header = matches!(rows.get(*i), Some(RowKind::Header { collapsed: false, .. }));
            shown && !header
        };
        let target = if forward {
            (at + 1..rows.len()).find(stop)
        } else {
            (0..at).rev().find(stop)
        };
        drop(rows);
        // At either end the key is still the rail's: past the last symbol is
        // nowhere, not the next widget over.
        let Some(row) = target.and_then(|at| self.list.row_at_index(at as i32)) else { return true };
        if row.is_selectable() {
            self.list.select_row(Some(&row));
        }
        row.grab_focus();
        true
    }

    /// Fold or unfold the section where the keyboard is, and leave the
    /// keyboard on its header. A rebuild makes every row new, so without that
    /// the next arrow key would start from nowhere.
    fn fold_from_keyboard(self: &Rc<Self>) -> bool {
        // Only when a row itself has the keyboard: a key in the box renaming a
        // section, or on a button in a header, is that widget's.
        let Some(row) = self.list.root().and_then(|root| root.focus()).and_downcast::<gtk::ListBoxRow>()
        else {
            return false;
        };
        let on = self.rows.borrow().get(row.index().max(0) as usize).and_then(fold_target);
        let Some((id, folded)) = on else { return false };
        self.fold_section(id, folded);
        if let Some(header) = self.header_row(id) {
            header.grab_focus();
        }
        true
    }

    /// Fold or unfold a section. Nothing is rebuilt when it is already that way.
    fn fold_section(self: &Rc<Self>, section_id: i64, folded: bool) {
        let already = self.rows.borrow().iter().any(|kind| match kind {
            RowKind::Header { section_id: id, collapsed } => *id == section_id && *collapsed == folded,
            RowKind::Entry { .. } => false,
        });
        if !already {
            self.store.set_section_collapsed(section_id, folded);
            self.rebuild();
        }
    }

    fn header_row(&self, section_id: i64) -> Option<gtk::ListBoxRow> {
        let at = self.rows.borrow().iter().position(
            |kind| matches!(kind, RowKind::Header { section_id: id, .. } if *id == section_id),
        )?;
        self.list.row_at_index(at as i32)
    }

    /// Remove whatever is highlighted, and leave the highlight where it was so
    /// Delete can be pressed again.
    pub fn remove_selected(self: &Rc<Self>) {
        let at = self.list.selected_row().map(|r| r.index().max(0) as usize);
        let Some(at) = at else { return };
        let target = match self.rows.borrow().get(at) {
            Some(RowKind::Entry { section_id, entry, .. }) => Some((*section_id, entry.clone())),
            _ => None,
        };
        let Some((section_id, entry)) = target else { return };
        self.remove_entry(section_id, &entry);

        // Land on the row that took its place, or the one above if it was last.
        let next = self
            .list
            .row_at_index(at as i32)
            .or_else(|| self.list.row_at_index(at as i32 - 1));
        if let Some(row) = next {
            if row.is_selectable() {
                self.list.select_row(Some(&row));
            }
        }
    }

    fn remove_entry(self: &Rc<Self>, section_id: i64, entry: &Entry) {
        self.store.remove_from_section(section_id, &entry.symbol, entry.suffix.as_deref());
        self.changed();
    }

    /// Select the first symbol of the next or previous section.
    fn jump_section(self: &Rc<Self>, forward: bool) {
        let current = self.list.selected_row().map(|r| r.index()).unwrap_or(0).max(0) as usize;
        let rows = self.rows.borrow();

        // Which section are we in now?
        let current_section = rows[..=current.min(rows.len().saturating_sub(1))]
            .iter()
            .rev()
            .find_map(|kind| match kind {
                RowKind::Entry { section_id, .. } | RowKind::Header { section_id, .. } => {
                    Some(*section_id)
                }
            });

        let candidates: Vec<usize> = rows
            .iter()
            .enumerate()
            .filter_map(|(at, kind)| match kind {
                RowKind::Entry { section_id, .. } if Some(*section_id) != current_section => {
                    Some(at)
                }
                _ => None,
            })
            .collect();
        drop(rows);

        let target = if forward {
            candidates.into_iter().find(|at| *at > current)
        } else {
            candidates.into_iter().filter(|at| *at < current).next_back()
        };
        if let Some(at) = target {
            if let Some(row) = self.list.row_at_index(at as i32) {
                self.list.select_row(Some(&row));
                row.grab_focus();
            }
        }
    }

    /// The list itself changed: redraw it, and tell the bar widget so it does
    /// not sit a refresh interval behind.
    pub fn changed(self: &Rc<Self>) {
        self.rebuild();
        crate::bar_plugin::notify_changed();
    }

    /// The same, for a change made outside this window — a command typed in a
    /// terminal while the app is open.
    ///
    /// Two things `changed` does not have to worry about and this does. The
    /// list the rail is showing may have been deleted, which would leave an
    /// empty rail with a dead id behind it. And how far down the rows somebody
    /// had scrolled is lost by any rebuild, because emptying the list collapses
    /// the scrollbar's range to nothing on the way through — which is fine when
    /// the user asked for the change and wrong when it happened somewhere else.
    pub fn reload(self: &Rc<Self>) {
        let fell_back = !self.store.watchlist_exists(self.active.get());
        if fell_back {
            self.active.set(DEFAULT_WATCHLIST);
            self.store.set_setting(SETTING_ACTIVE, &DEFAULT_WATCHLIST.to_string());
        }

        let scroller = self
            .list
            .ancestor(gtk::ScrolledWindow::static_type())
            .and_downcast::<gtk::ScrolledWindow>();
        let offset = scroller.as_ref().map(|view| view.vadjustment().value());

        self.changed();

        // Only where the rail is still showing the same list: a fallback is a
        // different list, and the top of it is where to be.
        if let (Some(scroller), Some(offset), false) = (scroller, offset, fell_back) {
            // Once the new rows have been given a size, because an adjustment
            // clamps to a range that is still the old one.
            glib::idle_add_local_once(move || scroller.vadjustment().set_value(offset));
        }
        if fell_back {
            self.announce_switch();
        }
    }

    /// Rebuild the whole rail. A few dozen rows, so there is nothing to gain
    /// from patching it in place.
    pub fn rebuild(self: &Rc<Self>) {
        let selected = self.list.selected_row().map(|r| r.index());
        // The symbol, so that a sort, which moves it, keeps it selected; the
        // place, for when it has gone.
        let selected_entry = selected.and_then(|at| match self.rows.borrow().get(at as usize) {
            Some(RowKind::Entry { section_id, entry, .. }) => Some((*section_id, entry.clone())),
            _ => None,
        });

        self.quiet.set(true);
        clear_rows(&self.list);

        self.write_switcher();
        // A list made, renamed or deleted from a terminal shows on the page
        // of lists as well, if that is the page on screen.
        if self.showing_lists() {
            self.fill_lists();
        }
        self.sort.set(stored_sort(&self.store, self.active.get()));
        self.write_column_header();

        let mut kinds = Vec::new();
        for section in self.store.watchlist_sections(self.active.get()) {
            if !section.root {
                self.list.append(&self.section_header(section.id, &section.name, section.collapsed));
                kinds.push(RowKind::Header { section_id: section.id, collapsed: section.collapsed });
            }
            let mut entries: Vec<(&Entry, Instrument)> = section
                .entries
                .iter()
                .filter_map(|entry| {
                    let found = self.index.find(&entry.symbol, entry.suffix.as_deref());
                    found.map(|instrument| (entry, instrument))
                })
                .collect();
            if let Some(sort) = self.sort.get() {
                let mut ranked: Vec<(Rank, (&Entry, Instrument))> = entries
                    .into_iter()
                    .map(|row| (sort.rank(&row.1, &self.quote), row))
                    .collect();
                ranked.sort_by(|(a, _), (b, _)| sort.compare(a, b));
                entries = ranked.into_iter().map(|(_, row)| row).collect();
            }
            for (entry, instrument) in entries {
                let (row, cells) = self.entry_row(section.id, entry, &instrument);
                row.set_visible(!section.collapsed);
                self.list.append(&row);
                kinds.push(RowKind::Entry {
                    section_id: section.id,
                    entry: entry.clone(),
                    instrument: instrument.clone(),
                    cells,
                });
            }
        }
        let empty = kinds.is_empty();
        *self.rows.borrow_mut() = kinds;

        // The placeholder went with the rows above. Built here rather than kept
        // around because an empty rail is the only time anybody sees it.
        if empty {
            self.list.set_placeholder(Some(&self.empty_state()));
        } else {
            *self.empty_focus.borrow_mut() = None;
        }

        let moved_to = selected_entry.and_then(|(section, wanted)| {
            self.rows.borrow().iter().position(|kind| {
                matches!(kind, RowKind::Entry { section_id, entry, .. }
                    if *section_id == section && *entry == wanted)
            })
        });
        let reselect = moved_to.map(|at| at as i32).or(selected);
        if let Some(row) = reselect.and_then(|at| self.list.row_at_index(at)) {
            self.list.select_row(Some(&row));
        }
        self.quiet.set(false);
    }

    /// Section title, a disclosure arrow, and its controls. Double-clicking the
    /// title renames it in place.
    fn section_header(
        self: &Rc<Self>,
        id: i64,
        name: &str,
        collapsed: bool,
    ) -> gtk::ListBoxRow {
        let arrow = gtk::Button::from_icon_name(if collapsed {
            "pan-end-symbolic"
        } else {
            "pan-down-symbolic"
        });
        arrow.add_css_class("flat");
        arrow.add_css_class("section-fold");
        arrow.set_valign(gtk::Align::Center);
        let this = self.clone();
        arrow.connect_clicked(move |_| this.fold_section(id, !collapsed));

        let label = gtk::Label::new(Some(name));
        label.set_xalign(0.0);
        label.set_hexpand(true);
        label.add_css_class("dim-label");
        label.add_css_class("caption-heading");
        label.set_ellipsize(gtk::pango::EllipsizeMode::End);

        let stack = gtk::Stack::new();
        stack.set_hexpand(true);
        stack.add_named(&label, Some("label"));
        let rename = gtk::Entry::new();
        rename.set_text(name);
        stack.add_named(&rename, Some("entry"));
        stack.set_visible_child_name("label");

        let click = gtk::GestureClick::new();
        let stack_weak = stack.downgrade();
        let rename_weak = rename.downgrade();
        click.connect_pressed(move |_, presses, _, _| {
            if presses < 2 {
                return;
            }
            if let (Some(stack), Some(rename)) = (stack_weak.upgrade(), rename_weak.upgrade()) {
                stack.set_visible_child_name("entry");
                rename.grab_focus();
                rename.select_region(0, -1);
            }
        });
        label.add_controller(click);

        let this = self.clone();
        rename.connect_activate(move |entry| {
            let text = entry.text().trim().to_string();
            if !text.is_empty() {
                this.store.rename_section(id, &text);
            }
            this.changed();
        });
        let focus = gtk::EventControllerFocus::new();
        let stack_weak = stack.downgrade();
        focus.connect_leave(move |_| {
            if let Some(stack) = stack_weak.upgrade() {
                stack.set_visible_child_name("label");
            }
        });
        rename.add_controller(focus);

        let header = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        header.set_margin_start(4);
        header.set_margin_end(10);
        header.set_margin_top(6);
        header.append(&arrow);
        header.append(&stack);

        let row = gtk::ListBoxRow::new();
        row.set_child(Some(&header));
        // A header takes the keyboard but never the selection: an arrow key
        // stops on one without loading anything, and Enter folds it.
        row.set_selectable(false);
        row.set_activatable(false);
        // Named like the symbols for the same reason: the line says where
        // the drop lands, so the frame GTK would draw here is noise.
        row.add_css_class("section-row");

        // Dropping a symbol on a header puts it in that section, at the end.
        // Without this there is no way to move something into a collapsed or
        // empty section. Dropping a section on one puts it beside that section.
        let target = gtk::DropTarget::new(glib::Type::STRING, gtk::gdk::DragAction::MOVE);
        let this = self.clone();
        target.set_preload(true);
        let marking = self.clone();
        target.connect_motion(move |target, _, _| {
            let at = match target.value().as_ref().and_then(parse_drag) {
                // Beside this section, on the side it is coming from.
                Some(Dragged::Section(moving)) => {
                    let order: Vec<i64> = marking
                        .rows
                        .borrow()
                        .iter()
                        .filter_map(|kind| match kind {
                            RowKind::Header { section_id, .. } => Some(*section_id),
                            RowKind::Entry { .. } => None,
                        })
                        .collect();
                    match header_drop_side(&order, moving, id) {
                        Beside::Before => marking.gap_above_section(id),
                        Beside::After => marking.end_of_section(id),
                    }
                }
                // A symbol dropped on a header joins the section at its end.
                _ => marking.end_of_section(id),
            };
            marking.mark_insertion(at);
            gtk::gdk::DragAction::MOVE
        });
        let marking = self.clone();
        target.connect_leave(move |_| marking.mark_insertion(None));
        target.connect_drop(move |_, value, _, _| match parse_drag(value) {
            Some(Dragged::Entry { from, entry }) => {
                this.mark_insertion(None);
                this.store.move_entry_to_section(from, id, &entry, None);
                this.changed();
                true
            }
            Some(Dragged::Section(moving)) => this.drop_section_on_header(moving, id),
            None => false,
        });
        row.add_controller(target);

        self.wire_section_reorder(&row, id);

        // Everything you can do to a section lives behind a right-click. A
        // delete button sitting on every header all the time is both noise and
        // an invitation to lose a section by accident.
        let menu = gtk::GestureClick::new();
        menu.set_button(gtk::gdk::BUTTON_SECONDARY);
        let this = self.clone();
        let section_name = name.to_string();
        let row_weak = row.downgrade();
        menu.connect_pressed(move |_, _, x, y| {
            let Some(row) = row_weak.upgrade() else { return };
            this.section_menu(&row, id, &section_name, x, y);
        });
        row.add_controller(menu);
        row
    }

    /// The section context menu.
    fn section_menu(
        self: &Rc<Self>,
        anchor: &gtk::ListBoxRow,
        id: i64,
        name: &str,
        x: f64,
        y: f64,
    ) {
        let actions = gio::SimpleActionGroup::new();

        let add = gio::SimpleAction::new("add", None);
        let this = self.clone();
        add.connect_activate(move |_, _| this.add_symbol_to(id));
        actions.add_action(&add);

        let promote = gio::SimpleAction::new("promote", None);
        let this = self.clone();
        let promoted = name.to_string();
        promote.connect_activate(move |_, _| this.promote_section(id, &promoted));
        actions.add_action(&promote);

        let remove = gio::SimpleAction::new("remove", None);
        let this = self.clone();
        let name = name.to_string();
        let anchor_for_remove = anchor.clone();
        remove.connect_activate(move |_, _| {
            this.confirm_remove_section(&anchor_for_remove, id, &name);
        });
        actions.add_action(&remove);
        anchor.insert_action_group("section", Some(&actions));

        // Only named sections are given a header at all, so the root — which
        // has no name to lend a watchlist — never reaches this menu.
        let model = gio::Menu::new();
        // The key beside the row is the rail's own Ctrl+N, which adds beside
        // the highlight rather than under this header: the same command,
        // arrived at without the menu. Promoting and removing are left bare,
        // because they have no key — a row advertising a shortcut nothing
        // listens for is worse than a row that says nothing.
        shortcuts::append_with_key(&model, "Add symbol…", "section.add", shortcuts::ADD_SYMBOL);
        model.append(Some("Turn into a watchlist"), Some("section.promote"));
        let destructive = gio::Menu::new();
        destructive.append(Some("Remove section…"), Some("section.remove"));
        model.append_section(None, &destructive);

        popup_menu(&model, anchor, x, y);
    }

    fn entry_row(
        self: &Rc<Self>,
        section_id: i64,
        entry: &Entry,
        instrument: &Instrument,
    ) -> (gtk::ListBoxRow, Vec<(Column, gtk::Label)>) {
        let quote = (self.quote)(instrument);
        let row_box = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        row_box.set_margin_start(12);
        row_box.set_margin_end(10);

        let mut cells = Vec::new();
        for column in self.columns.borrow().iter() {
            let label = self.cell(*column, instrument, quote);
            row_box.append(&label);
            if *column != Column::Symbol {
                cells.push((*column, label));
            }
        }

        let row = gtk::ListBoxRow::new();
        row.set_child(Some(&row_box));
        row.set_activatable(true);
        row.set_tooltip_text(Some(&instrument.full_name()));
        // Named so the stylesheet can take GTK's drop frame off the symbols:
        // the line between them says where the drop goes, which is the thing
        // being asked.
        row.add_css_class("symbol-row");

        self.wire_row_removal(&row, section_id, entry);
        self.wire_row_reorder(&row, section_id, entry);
        (row, cells)
    }

    /// Say why the rail has no prices, or `None` once a fetch works.
    ///
    /// The rail draws nothing but the cache, so a provider it cannot reach
    /// reaches a person here as a column of dashes and no reason — which
    /// reads as a watchlist of symbols this app does not carry. One line,
    /// the same words the chart uses, and gone the moment prices arrive.
    pub fn set_trouble(&self, trouble: Option<FetchFailure>) {
        // Only while something is actually missing. A rail whose prices are
        // all cached has nothing unexplained on it, and a line sitting over a
        // full column of numbers saying they will fill in shortly is noise
        // about a refresh nobody asked to be told about. The chart's own
        // corner marker is what covers that case.
        let show = trouble.filter(|_| self.has_a_missing_price());
        match show {
            Some(trouble) => {
                self.trouble.set_text(trouble.message());
                self.trouble.set_visible(true);
            }
            None => self.trouble.set_visible(false),
        }
    }

    /// Is any symbol on screen still showing a dash?
    fn has_a_missing_price(&self) -> bool {
        self.rows.borrow().iter().any(|kind| match kind {
            RowKind::Entry { instrument, .. } => (self.quote)(instrument).is_none(),
            RowKind::Header { .. } => false,
        })
    }

    /// Write new numbers into the rows that are already there.
    ///
    /// Prefetching means quotes arrive constantly, and rebuilding the rail for
    /// each one tears down every widget — which loses the selection, steals
    /// focus mid-keypress, and flickers. Only the values change, so only the
    /// values are written.
    pub fn refresh_quotes(&self) {
        for kind in self.rows.borrow().iter() {
            let RowKind::Entry { instrument, cells, .. } = kind else { continue };
            let quote = (self.quote)(instrument);
            for (column, label) in cells {
                write_cell(label, *column, quote, instrument.kind);
            }
        }
        // New prices can reorder a list sorted by them.
        if self.sort.get().is_some_and(|sort| sort.column != Column::Symbol) {
            self.resort();
        }
    }

    /// Put the rows back in sorted order after their prices moved, by moving
    /// the rows that are out of place rather than building the rail again —
    /// which would lose the selection, the focus and the scroll every time a
    /// quote came in.
    fn resort(&self) {
        let Some(sort) = self.sort.get() else { return };
        let mut rows = self.rows.borrow_mut();
        // One section's run of symbols at a time, between its headers.
        let mut start = 0;
        for end in 0..=rows.len() {
            if end < rows.len() && rows[end].instrument().is_some() {
                continue;
            }
            let ranks: Vec<Rank> = rows[start..end]
                .iter()
                .map(|row| match row.instrument() {
                    Some(instrument) => sort.rank(instrument, &self.quote),
                    // Never: the run ends at the first row that is not a symbol.
                    None => Rank::Price(None),
                })
                .collect();
            let mut order: Vec<usize> = (start..end).collect();
            order.sort_by(|&a, &b| sort.compare(&ranks[a - start], &ranks[b - start]));
            if order.iter().copied().ne(start..end) {
                self.move_rows(&mut rows, start, &order);
            }
            start = end + 1;
        }
    }

    /// Lay `rows[start..]` out in `order`, widgets and all, keeping whatever
    /// was selected and focused where it was.
    fn move_rows(&self, rows: &mut [RowKind], start: usize, order: &[usize]) {
        let widgets: Vec<gtk::ListBoxRow> =
            order.iter().filter_map(|&at| self.list.row_at_index(at as i32)).collect();
        if widgets.len() != order.len() {
            return;
        }
        let selected = self.list.selected_row();
        let focused = widgets.iter().find(|row| row.has_focus()).cloned();
        self.quiet.set(true);
        for row in &widgets {
            self.list.remove(row);
        }
        for (offset, row) in widgets.iter().enumerate() {
            self.list.insert(row, (start + offset) as i32);
        }
        if let Some(row) = selected {
            self.list.select_row(Some(&row));
        }
        if let Some(row) = focused {
            row.grab_focus();
        }
        self.quiet.set(false);
        let reordered: Vec<RowKind> = order.iter().map(|&at| rows[at].clone()).collect();
        for (offset, kind) in reordered.into_iter().enumerate() {
            rows[start + offset] = kind;
        }
    }

    /// Where the `at`th divide between columns is, measured on the cells in
    /// the rows rather than on the titles over them.
    ///
    /// The headings are not the columns. A heading row and a row of symbols
    /// carry different margins and give their stretchy first column
    /// different amounts of room, so the gap between two titles and the gap
    /// between the numbers under them land in different places — and a
    /// handle belongs on the one the reader can see, which is where the data
    /// divides.
    fn column_edge(&self, at: usize) -> Option<i32> {
        let column = *self.columns.borrow().get(at)?;
        let cell = self.rows.borrow().iter().find_map(|kind| match kind {
            RowKind::Entry { cells, .. } => {
                cells.iter().find(|(c, _)| *c == column).map(|(_, label)| label.clone())
            }
            RowKind::Header { .. } => None,
        })?;
        let mine = cell.compute_bounds(&self.column_header)?;
        // Halfway across the gutter between this column and the one before
        // it, which is the line the eye draws between them.
        match cell.prev_sibling().and_then(|before| before.compute_bounds(&self.column_header)) {
            Some(before) => Some(((before.x() + before.width() + mine.x()) / 2.0).round() as i32),
            None => Some(mine.x().round() as i32),
        }
    }

    /// Where a mark stands for the `at`th divide: on the cells' own edge,
    /// and on the titles' only when there are no rows to measure.
    fn divide_x(&self, at: usize) -> i32 {
        self.column_edge(at).unwrap_or_else(|| gap_x(&self.column_header, at))
    }

    fn write_column_header(self: &Rc<Self>) {
        while let Some(child) = self.column_header.first_child() {
            self.column_header.remove(&child);
        }
        let sort = self.sort.get();
        for column in self.columns.borrow().iter().copied() {
            let arrow = match sort {
                Some(sort) if sort.column == column && sort.descending => Some("↓"),
                Some(sort) if sort.column == column => Some("↑"),
                _ => None,
            };
            // Numbers line up on the right, so their titles do too, with the
            // arrow on the side away from the edge they share.
            let title = column.title();
            let label = gtk::Label::new(Some(&match arrow {
                None => title.to_string(),
                Some(arrow) if column == Column::Symbol => format!("{title} {arrow}"),
                Some(arrow) => format!("{arrow} {title}"),
            }));
            label.add_css_class("caption");
            if arrow.is_none() {
                label.add_css_class("dim-label");
            }
            if column == Column::Symbol {
                label.set_xalign(0.0);
                label.set_hexpand(true);
            } else {
                label.set_xalign(1.0);
                label.set_width_chars(column.width_chars());
            }
            let tip = match Sort::after_click(sort, column) {
                Some(next) if next.descending => format!("Sort by {title}, high to low"),
                Some(_) => format!("Sort by {title}, low to high"),
                None => "Back to the list's own order".to_string(),
            };
            label.set_tooltip_text(Some(&tip));

            let click = gtk::GestureClick::new();
            let from = Rc::new(Cell::new(f64::MIN));
            let pressed = from.clone();
            click.connect_pressed(move |_, _, x, _| pressed.set(x));
            let this = self.clone();
            click.connect_released(move |_, _, x, _| {
                // A press that travelled was somebody moving the column, or
                // trying to; it is not a click, whatever it ends up doing.
                if (x - from.get()).abs() > CLICK_SLOP {
                    return;
                }
                let next = Sort::after_click(this.sort.get(), column);
                store_sort(&this.store, this.active.get(), next);
                this.rebuild();
            });
            label.add_controller(click);

            // Carried further than a click, the same press moves the column
            // instead of sorting by it: GTK hands the sequence to whichever
            // of the two the gesture turns out to be. Not the symbol, which
            // is always the first column — a drag of it could only ever have
            // been undone by the rule that puts it back.
            if column == Column::Symbol {
                self.column_header.append(&label);
                continue;
            }
            let source = gtk::DragSource::new();
            source.set_actions(gtk::gdk::DragAction::MOVE);
            source.connect_prepare(move |_, _, _| {
                Some(gtk::gdk::ContentProvider::for_value(&ColumnDrag(column).to_value()))
            });
            // What you are dragging: the title itself, under the pointer,
            // which is the widget we already have rather than a drawing of it.
            source.connect_drag_begin(move |source, _| {
                let Some(label) = source.widget().and_downcast::<gtk::Label>() else { return };
                let ghost = gtk::WidgetPaintable::new(Some(&label));
                source.set_icon(Some(&ghost), label.width() / 2, label.height() / 2);
            });
            label.add_controller(source);
            self.column_header.append(&label);
        }
        self.column_marker.set_visible(false);
    }

    /// Where a dragged column would land: a short line standing in the gap it
    /// would go into, which moves between the titles as the pointer does.
    ///
    /// Over the titles rather than among them, so the header does not open a
    /// gap and shuffle sideways while a column is being carried — the line
    /// says where it would land, and nothing else moves until it does.
    /// A motion works out which gap the pointer is in and moves the line only
    /// when that answer changes; that is the whole of the per-motion work.
    /// A right-click on the titles offers to edit them: a click of the left
    /// button already sorts by the one under it, and a drag moves it, so
    /// adding and taking away columns is the thing left to reach.
    fn wire_column_menu(self: &Rc<Self>) {
        let menu = gtk::GestureClick::new();
        menu.set_button(gtk::gdk::BUTTON_SECONDARY);
        let this = self.clone();
        menu.connect_pressed(move |gesture, _, x, y| {
            gesture.set_state(gtk::EventSequenceState::Claimed);
            let actions = gio::SimpleActionGroup::new();
            let edit = gio::SimpleAction::new("edit", None);
            let editor = this.clone();
            edit.connect_activate(move |_, _| editor.edit_columns());
            actions.add_action(&edit);
            this.column_header.insert_action_group("columns", Some(&actions));

            let model = gio::Menu::new();
            model.append(Some("Edit columns\u{2026}"), Some("columns.edit"));
            popup_menu(&model, &this.column_header, x, y);
        });
        self.column_header.add_controller(menu);
    }

    /// The columns, as a list to add to, take from and drag into order, in
    /// the shape the preferences edit the charts' header in.
    pub fn edit_columns(self: &Rc<Self>) {
        let dialog = adw::Dialog::new();
        dialog.set_title("Watchlist Columns");
        dialog.set_content_width(400);
        let page = adw::PreferencesPage::new();
        let this = self.clone();
        let list = crate::ui::part_list::PartList::new(
            "In the watchlist",
            "Add a column",
            self.columns(),
            move |columns| this.set_columns(columns),
        );
        page.add(&list.group);
        let root = adw::ToolbarView::new();
        root.add_top_bar(&adw::HeaderBar::new());
        root.set_content(Some(&page));
        dialog.set_child(Some(&root));
        dialog.present(Some(&self.widget));
    }

    fn wire_column_drop(self: &Rc<Self>) {
        let marker = self.column_marker.clone();
        let target = gtk::DropTarget::new(ColumnDrag::static_type(), gtk::gdk::DragAction::MOVE);
        // Which gap the line is standing in, so a motion inside the same gap
        // costs the lookup and nothing else.
        let showing: Rc<Cell<Option<usize>>> = Rc::new(Cell::new(None));

        let header = self.column_header.clone();
        let mark = marker.clone();
        let at_now = showing.clone();
        let this = self.clone();
        target.connect_motion(move |_, x, _| {
            let at = gap_at(&header, x).max(1);
            if at_now.get() != Some(at) {
                at_now.set(Some(at));
                mark.set_margin_start(this.divide_x(at));
                mark.set_visible(true);
            }
            gtk::gdk::DragAction::MOVE
        });

        let mark = marker.clone();
        let at_now = showing.clone();
        target.connect_leave(move |_| {
            at_now.set(None);
            mark.set_visible(false);
        });

        let this = self.clone();
        let header = self.column_header.clone();
        let mark = marker.clone();
        let at_now = showing.clone();
        target.connect_drop(move |_, value, x, _| {
            at_now.set(None);
            mark.set_visible(false);
            let Ok(ColumnDrag(moving)) = value.get::<ColumnDrag>() else { return false };
            let at = gap_at(&header, x).max(1);
            let Some(columns) = dropped(&this.columns.borrow(), moving, at) else { return false };
            this.set_columns(columns);
            true
        });
        self.column_header.add_controller(target);
    }

    /// The middle of the gap above the row at `at`, which is where a symbol
    /// dropped on that row goes in.
    ///
    /// Between the two rows rather than on one of their edges: the rows
    /// carry a margin, so their edges are a few pixels apart and a line on
    /// either of them reads as belonging to that row rather than to the
    /// space between.
    fn gap_above(&self, at: i32) -> Option<i32> {
        let row = self.list.row_at_index(at)?;
        match at.checked_sub(1).and_then(|above| self.list.row_at_index(above)) {
            Some(above) => Some(between(&self.row_overlay, &above, &row)),
            // Nothing above it: the top of the list is the gap.
            None => Some(within(&self.row_overlay, &row).0),
        }
    }

    /// The middle of the gap below the row at `at`.
    fn gap_below(&self, at: i32) -> Option<i32> {
        let row = self.list.row_at_index(at)?;
        match self.list.row_at_index(at + 1) {
            Some(below) => Some(between(&self.row_overlay, &row, &below)),
            None => Some(within(&self.row_overlay, &row).1),
        }
    }

    /// Put the insertion line at `y` in the list, or take it away.
    ///
    /// Per motion: the same answer moves nothing, which is most of them —
    /// a row is many pixels tall and the line sits on its edge.
    fn mark_insertion(&self, y: Option<i32>) {
        match y {
            Some(y) => {
                if self.row_marked.get() == y && self.row_marker.is_visible() {
                    return;
                }
                self.row_marked.set(y);
                self.row_marker.set_margin_top((y - 1).max(0));
                self.row_marker.set_visible(true);
            }
            None => {
                self.row_marked.set(-1);
                self.row_marker.set_visible(false);
            }
        }
    }

    /// Where a symbol dropped on this section's header would go in: after
    /// its last symbol, or against the header itself when it has none.
    fn end_of_section(&self, section_id: i64) -> Option<i32> {
        let rows = self.rows.borrow();
        let last = rows.iter().enumerate().filter(|(_, kind)| {
            matches!(kind, RowKind::Entry { section_id: id, .. } if *id == section_id)
        });
        let at = last.map(|(at, _)| at).next_back();
        let at = match at {
            Some(at) => at,
            None => rows.iter().position(
                |kind| matches!(kind, RowKind::Header { section_id: id, .. } if *id == section_id),
            )?,
        };
        drop(rows);
        self.gap_below(at as i32)
    }

    /// The middle of the gap above a section's header, where a section
    /// dropped in front of it would go.
    fn gap_above_section(&self, section_id: i64) -> Option<i32> {
        let at = self.rows.borrow().iter().position(
            |kind| matches!(kind, RowKind::Header { section_id: id, .. } if *id == section_id),
        )?;
        self.gap_above(at as i32)
    }

    /// Where a section dropped on this one's symbols would go: a section
    /// cannot sit among another's rows, so it goes to the end of it the
    /// pointer is nearer.
    fn nearer_end(&self, section_id: i64, row: i32) -> Option<i32> {
        let rows = self.rows.borrow();
        let mine: Vec<usize> = rows
            .iter()
            .enumerate()
            .filter(|(_, kind)| {
                matches!(kind, RowKind::Entry { section_id: id, .. } if *id == section_id)
            })
            .map(|(at, _)| at)
            .collect();
        let at = mine.iter().position(|at| *at == row as usize)?;
        let first = mine.len();
        drop(rows);
        match at * 2 < first {
            true => self.gap_above_section(section_id),
            false => self.end_of_section(section_id),
        }
    }

    fn cell(&self, column: Column, instrument: &Instrument, quote: Option<Quote>) -> gtk::Label {
        let label = gtk::Label::new(None);
        if column == Column::Symbol {
            label.set_text(&instrument.display_symbol());
            label.set_xalign(0.0);
            label.set_hexpand(true);
            label.add_css_class("symbol-row-ticker");
            label.set_ellipsize(gtk::pango::EllipsizeMode::End);
            return label;
        }
        label.add_css_class("numeric");
        label.add_css_class("caption");
        label.set_xalign(1.0);
        label.set_width_chars(column.width_chars());
        write_cell(&label, column, quote, instrument.kind);
        label
    }

    /// Right-click offers removal, rather than removing on the click itself.
    fn wire_row_removal(self: &Rc<Self>, row: &gtk::ListBoxRow, section_id: i64, entry: &Entry) {
        let click = gtk::GestureClick::new();
        click.set_button(gtk::gdk::BUTTON_SECONDARY);

        let this = self.clone();
        let entry = entry.clone();
        let row_weak = row.downgrade();
        click.connect_pressed(move |_, _, x, y| {
            let Some(row) = row_weak.upgrade() else { return };

            let actions = gio::SimpleActionGroup::new();
            let remove = gio::SimpleAction::new("remove", None);
            let this = this.clone();
            let entry = entry.clone();
            remove.connect_activate(move |_, _| this.remove_entry(section_id, &entry));
            actions.add_action(&remove);
            row.insert_action_group("symbol", Some(&actions));

            let model = gio::Menu::new();
            // Delete does the same to whatever the rail has highlighted.
            let accel = shortcuts::REMOVE_SYMBOL;
            shortcuts::append_with_key(&model, "Remove", "symbol.remove", accel);
            popup_menu(&model, &row, x, y);
        });
        row.add_controller(click);
    }

    /// Drag a row onto another to put it there, within its section.
    fn wire_row_reorder(self: &Rc<Self>, row: &gtk::ListBoxRow, section_id: i64, entry: &Entry) {
        let payload = format!(
            "{section_id}\t{}\t{}",
            entry.symbol,
            entry.suffix.clone().unwrap_or_default()
        );

        let source = gtk::DragSource::new();
        source.set_actions(gtk::gdk::DragAction::MOVE);
        let dragged = payload.clone();
        source.connect_prepare(move |_, _, _| {
            Some(gtk::gdk::ContentProvider::for_value(&dragged.to_value()))
        });
        // The row itself under the pointer. Without an icon of its own GTK
        // draws the payload, and the payload is a section id and a tab.
        let this = self.clone();
        source.connect_drag_begin(move |source, _| {
            this.list.add_css_class("dragging");
            let Some(row) = source.widget() else { return };
            let ghost = gtk::WidgetPaintable::new(Some(&row));
            source.set_icon(Some(&ghost), row.width() / 2, row.height() / 2);
        });
        let this = self.clone();
        source.connect_drag_end(move |_, _, _| {
            this.list.remove_css_class("dragging");
            this.mark_insertion(None);
        });
        row.add_controller(source);

        let target = gtk::DropTarget::new(glib::Type::STRING, gtk::gdk::DragAction::MOVE);
        // A symbol lands in front of the row it is dropped on, so the line
        // goes on that row's top edge — the place, not the row.
        // Read the payload while it is over us rather than only on the drop,
        // so the line can say where this particular thing would land.
        target.set_preload(true);
        let this = self.clone();
        let marked = row.clone();
        target.connect_motion(move |target, _, _| {
            let dragged = target.value().as_ref().and_then(parse_drag);
            let at = match dragged {
                // A section cannot sit among another's symbols.
                Some(Dragged::Section(_)) => this.nearer_end(section_id, marked.index()),
                // Where it lands when the list is in its own order is this
                // row's place. When the list is sorted the row is the sort's
                // to pick and all the drop decides is the section, so the
                // line goes at the end of it rather than promising a place
                // it will not take.
                _ => match this.sort.get() {
                    Some(_) => this.end_of_section(section_id),
                    None => this.gap_above(marked.index()),
                },
            };
            this.mark_insertion(at);
            gtk::gdk::DragAction::MOVE
        });
        let this = self.clone();
        target.connect_leave(move |_| this.mark_insertion(None));
        let this = self.clone();
        let onto = entry.clone();
        target.connect_drop(move |_, value, _, _| match parse_drag(value) {
            Some(Dragged::Entry { from, entry }) => {
                this.mark_insertion(None);
                // A place in a sorted section is the sort's to decide, so a
                // symbol can only be dropped into a different one.
                if from == section_id && (entry == onto || this.sort.get().is_some()) {
                    return false;
                }
                this.store.move_entry_to_section(from, section_id, &entry, Some(&onto));
                this.changed();
                true
            }
            Some(Dragged::Section(moving)) => this.drop_section_on_row(moving, section_id, &onto),
            None => false,
        });
        row.add_controller(target);
    }

    /// Drag a section's header to move the section, and its symbols with it.
    ///
    /// On the header rather than on a grip, because a collapsed section is a
    /// header and nothing else and has to drag like the rest of them. A symbol
    /// is picked up from its own row, so neither gesture can be mistaken for
    /// the other.
    fn wire_section_reorder(self: &Rc<Self>, row: &gtk::ListBoxRow, id: i64) {
        let source = gtk::DragSource::new();
        source.set_actions(gtk::gdk::DragAction::MOVE);
        let payload = format!("{SECTION_DRAG}\t{id}");
        source.connect_prepare(move |_, _, _| {
            Some(gtk::gdk::ContentProvider::for_value(&payload.to_value()))
        });
        // A section travels as itself, and the rows it passes over stay as
        // they are: the line says where it lands.
        let this = self.clone();
        source.connect_drag_begin(move |source, _| {
            this.list.add_css_class("dragging");
            let Some(row) = source.widget() else { return };
            let ghost = gtk::WidgetPaintable::new(Some(&row));
            source.set_icon(Some(&ghost), row.width() / 2, row.height() / 2);
        });
        let this = self.clone();
        source.connect_drag_end(move |_, _, _| {
            this.list.remove_css_class("dragging");
            this.mark_insertion(None);
        });
        row.add_controller(source);
    }

    /// A section dropped on another section's header.
    fn drop_section_on_header(self: &Rc<Self>, moving: i64, onto: i64) -> bool {
        let order = self.store.section_order(self.active.get());
        let side = header_drop_side(&order, moving, onto);
        self.reorder_sections(&order, moving, onto, side)
    }

    /// A section dropped on a symbol row, which in one list is a drop target
    /// like any other.
    ///
    /// Where the row is in its section is read back from the store rather than
    /// captured when this was wired up: the rows the rail is showing are torn
    /// down and rebuilt by every change, and a handler holding one is holding a
    /// widget that no longer exists.
    fn drop_section_on_row(self: &Rc<Self>, moving: i64, onto: i64, under: &Entry) -> bool {
        let sections = self.store.watchlist_sections(self.active.get());
        let order: Vec<i64> = sections.iter().filter(|s| !s.root).map(|s| s.id).collect();
        let Some(section) = sections.iter().find(|s| s.id == onto) else { return false };
        let Some(at) = section.entries.iter().position(|entry| entry == under) else {
            return false;
        };
        let Some((beside, side)) = row_drop_target(&order, section, at) else { return false };
        self.reorder_sections(&order, moving, beside, side)
    }

    /// Write a section order down and redraw the rail.
    ///
    /// A drop that works out to the order already stored is refused rather
    /// than applied, so dropping a section back where it came from does not
    /// rebuild the rail to put it in the same place.
    fn reorder_sections(
        self: &Rc<Self>,
        order: &[i64],
        moving: i64,
        onto: i64,
        side: Beside,
    ) -> bool {
        let Some(ordered) = sections_beside(order, moving, onto, side) else { return false };
        self.store.reorder_sections(self.active.get(), &ordered);
        self.changed();
        true
    }

    /// Name the watchlist on screen in the band, unless the band is naming
    /// the page of lists.
    fn write_switcher(&self) {
        if self.showing_lists() {
            return;
        }
        let name = self
            .store
            .watchlists()
            .into_iter()
            .find(|(id, _)| *id == self.active.get())
            .map(|(_, name)| name)
            .unwrap_or_default();

        self.title.set_text(&name);
    }

    /// The way out, and the ways back in: a click or Enter on a list. Escape
    /// goes back to the list on screen without changing it.
    fn wire_switcher(self: &Rc<Self>) {
        let this = self.clone();
        // The same button back in, as Escape does: to the list on screen.
        self.switcher.connect_clicked(move |_| {
            if this.showing_lists() {
                this.show_symbols();
                this.take_focus();
            } else {
                this.show_lists();
            }
        });

        let this = self.clone();
        self.lists.connect_row_activated(move |_, row| {
            let id = this.list_ids.borrow().get(row.index().max(0) as usize).copied();
            if let Some(id) = id {
                this.open_list(id);
            }
        });

        // Only when a row itself has the keyboard: Escape in the box renaming
        // or naming a list is that box's.
        let keys = gtk::EventControllerKey::new();
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        let this = self.clone();
        keys.connect_key_pressed(move |_, key, _, state| {
            use gtk::gdk::ModifierType;
            let held = ModifierType::CONTROL_MASK
                | ModifierType::ALT_MASK
                | ModifierType::SHIFT_MASK
                | ModifierType::SUPER_MASK;
            let row = this.lists.root().and_then(|root| root.focus()).and_downcast::<gtk::ListBoxRow>();
            if key != gtk::gdk::Key::Escape || state.intersects(held) || row.is_none() {
                return glib::Propagation::Proceed;
            }
            this.show_symbols();
            this.take_focus();
            glib::Propagation::Stop
        });
        self.lists.add_controller(keys);
    }

    /// The + in the band. On a list's symbols it offers the two things a
    /// list holds, the one done all day first; on the page of lists there is
    /// only one thing to make, so it makes it.
    fn wire_add(self: &Rc<Self>) {
        let actions = gio::SimpleActionGroup::new();
        let symbol = gio::SimpleAction::new("symbol", None);
        let this = self.clone();
        symbol.connect_activate(move |_, _| this.add_symbol());
        actions.add_action(&symbol);
        let section = gio::SimpleAction::new("section", None);
        let this = self.clone();
        section.connect_activate(move |_, _| this.prompt_new_section(&this.add));
        actions.add_action(&section);
        self.add.insert_action_group("add", Some(&actions));

        let this = self.clone();
        self.add.connect_clicked(move |button| {
            if this.showing_lists() {
                this.prompt_new_watchlist();
                return;
            }
            let model = gio::Menu::new();
            for (label, action, accel) in [
                ("Add a symbol", "add.symbol", shortcuts::ADD_SYMBOL),
                ("Add a section", "add.section", shortcuts::ADD_SECTION),
            ] {
                let item = gio::MenuItem::new(Some(label), Some(action));
                item.set_attribute_value("accel", Some(&accel.to_variant()));
                model.append_item(&item);
            }
            popup_menu(&model, button, 0.0, button.height() as f64);
        });
    }

    /// Step out to every watchlist, with the one on screen highlighted and
    /// the keyboard on it.
    fn show_lists(self: &Rc<Self>) {
        self.fill_lists();
        self.pages.set_visible_child_name(PAGE_LISTS);
        self.switcher.add_css_class("open");
        self.title.set_text(LISTS_TITLE);
        self.add.set_tooltip_text(Some(NEW_LIST_TIP));
        self.focus_lists();
    }

    /// Back to the symbols of the list on screen.
    fn show_symbols(&self) {
        self.pages.set_visible_child_name(PAGE_SYMBOLS);
        self.switcher.remove_css_class("open");
        self.write_switcher();
        self.add.set_tooltip_text(Some(ADD_TIP));
    }

    fn showing_lists(&self) -> bool {
        self.pages.visible_child_name().as_deref() == Some(PAGE_LISTS)
    }

    fn focus_lists(&self) {
        match self.lists.selected_row() {
            Some(row) => row.grab_focus(),
            None => self.lists.grab_focus(),
        };
    }

    /// Show a list's symbols and put the keyboard in them, without picking
    /// one: a highlight drives the chart, and opening a list is for looking
    /// at it, not for moving the chart to whatever happens to be first.
    fn open_list(self: &Rc<Self>, id: i64) {
        self.set_active_watchlist(id);
        self.take_focus();
    }

    /// One row a watchlist, in the order the number keys count them.
    fn fill_lists(self: &Rc<Self>) {
        clear_rows(&self.lists);
        let mut ids = Vec::new();
        for (id, name) in self.watchlists() {
            let row = self.list_row(id, &name);
            self.lists.append(&row);
            if id == self.active.get() {
                self.lists.select_row(Some(&row));
            }
            ids.push(id);
        }
        *self.list_ids.borrow_mut() = ids;
    }

    /// One watchlist: its name, and how many symbols it holds.
    ///
    /// Renaming and deleting are behind a right-click, as a section's are,
    /// and behind a pencil and a bin that show before the count while the
    /// pointer is on the row. Only then: on every row at once they were two
    /// controls a list for things done once a month, beside the one done all
    /// day. Delete still asks first, so it is not one slip away from a list
    /// somebody meant to open.
    ///
    /// A list another chartbook is using cannot be opened here, and says
    /// whose it is rather than merely refusing: the same words the chain's
    /// popover uses for a group another list holds.
    fn list_row(self: &Rc<Self>, id: i64, name: &str) -> gtk::ListBoxRow {
        let label = gtk::Label::new(Some(name));
        label.set_xalign(0.0);
        label.add_css_class("symbol-row-ticker");
        label.set_ellipsize(gtk::pango::EllipsizeMode::End);

        let rename = gtk::Entry::new();
        rename.set_text(name);

        let stack = gtk::Stack::new();
        stack.set_hexpand(true);
        stack.set_hhomogeneous(false);
        stack.add_named(&label, Some("label"));
        stack.add_named(&rename, Some("entry"));

        let row_box = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        row_box.set_margin_start(12);
        row_box.set_margin_end(10);
        row_box.append(&stack);

        let row = gtk::ListBoxRow::new();
        row.add_css_class("list-row");

        if let Some(book) = self.chartbook_using(id) {
            let owner = gtk::Label::new(Some(&book));
            owner.add_css_class("dim-label");
            owner.add_css_class("caption");
            owner.set_ellipsize(gtk::pango::EllipsizeMode::End);
            row_box.append(&owner);
            label.add_css_class("dim-label");
            row.set_activatable(false);
        }

        let symbols: usize = self.store.watchlist_sections(id).iter().map(|s| s.entries.len()).sum();
        let count = gtk::Label::new(Some(&symbols.to_string()));
        count.add_css_class("dim-label");
        count.add_css_class("caption");
        count.add_css_class("numeric");
        count.set_xalign(1.0);
        // Three digits wide whatever it holds, so the buttons before it stand
        // in one column down the page rather than stepping with the count.
        count.set_width_chars(3);

        // Spaced as far apart as a row is tall, so a bin is not hit for a
        // pencil, and further from the count, which stays where it was.
        let actions = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        actions.set_margin_end(6);
        actions.set_visible(false);
        let edit = gtk::Button::from_icon_name("document-edit-symbolic");
        edit.add_css_class("flat");
        edit.add_css_class("list-row-action");
        edit.set_tooltip_text(Some("Rename"));
        let stack_weak = stack.downgrade();
        let rename_weak = rename.downgrade();
        edit.connect_clicked(move |_| {
            if let (Some(stack), Some(rename)) = (stack_weak.upgrade(), rename_weak.upgrade()) {
                start_rename(&stack, &rename);
            }
        });
        actions.append(&edit);
        // The default watchlist has no bin at all, here or in the menu. A
        // disabled one would be a control that exists to be refused, and the
        // rule it enforces — something has to be left — is not worth
        // explaining twice.
        if id != DEFAULT_WATCHLIST {
            let remove = gtk::Button::from_icon_name("user-trash-symbolic");
            remove.add_css_class("flat");
            remove.add_css_class("list-row-action");
            remove.set_tooltip_text(Some("Delete…"));
            let this = self.clone();
            let list_name = name.to_string();
            remove.connect_clicked(move |_| this.confirm_remove_watchlist(id, &list_name));
            actions.append(&remove);
        }
        row_box.append(&actions);
        row_box.append(&count);
        row.set_child(Some(&row_box));

        let hover = gtk::EventControllerMotion::new();
        let actions_weak = actions.downgrade();
        let show = move |on: bool| {
            if let Some(actions) = actions_weak.upgrade() {
                actions.set_visible(on);
            }
        };
        let enter = show.clone();
        hover.connect_enter(move |_, _, _| enter(true));
        hover.connect_leave(move |_| show(false));
        row.add_controller(hover);

        let this = self.clone();
        rename.connect_activate(move |entry| {
            let text = entry.text().trim().to_string();
            if !text.is_empty() {
                this.store.rename_watchlist(id, &text);
            }
            // Refilled once this signal is over: it is the box being typed
            // in that the refill takes away.
            let this = this.clone();
            glib::idle_add_local_once(move || {
                this.fill_lists();
                this.focus_lists();
            });
        });
        let focus = gtk::EventControllerFocus::new();
        let stack_weak = stack.downgrade();
        focus.connect_leave(move |_| {
            if let Some(stack) = stack_weak.upgrade() {
                stack.set_visible_child_name("label");
            }
        });
        rename.add_controller(focus);

        let menu = gtk::GestureClick::new();
        menu.set_button(gtk::gdk::BUTTON_SECONDARY);
        let this = self.clone();
        let list_name = name.to_string();
        let row_weak = row.downgrade();
        let stack_weak = stack.downgrade();
        let rename_weak = rename.downgrade();
        menu.connect_pressed(move |_, _, x, y| {
            let (Some(row), Some(stack), Some(rename)) =
                (row_weak.upgrade(), stack_weak.upgrade(), rename_weak.upgrade())
            else {
                return;
            };
            this.list_menu(&row, id, &list_name, (&stack, &rename), x, y);
        });
        row.add_controller(menu);
        row
    }

    /// A watchlist's context menu: the row's rename and delete, by the same
    /// rule as its bin.
    fn list_menu(
        self: &Rc<Self>,
        anchor: &gtk::ListBoxRow,
        id: i64,
        name: &str,
        (stack, rename): (&gtk::Stack, &gtk::Entry),
        x: f64,
        y: f64,
    ) {
        let actions = gio::SimpleActionGroup::new();

        let edit = gio::SimpleAction::new("rename", None);
        let stack_weak = stack.downgrade();
        let rename_weak = rename.downgrade();
        edit.connect_activate(move |_, _| {
            if let (Some(stack), Some(rename)) = (stack_weak.upgrade(), rename_weak.upgrade()) {
                start_rename(&stack, &rename);
            }
        });
        actions.add_action(&edit);

        let remove = gio::SimpleAction::new("remove", None);
        let this = self.clone();
        let name = name.to_string();
        remove.connect_activate(move |_, _| this.confirm_remove_watchlist(id, &name));
        actions.add_action(&remove);
        anchor.insert_action_group("list", Some(&actions));

        let model = gio::Menu::new();
        model.append(Some("Rename"), Some("list.rename"));
        if id != DEFAULT_WATCHLIST {
            let destructive = gio::Menu::new();
            destructive.append(Some("Delete…"), Some("list.remove"));
            model.append_section(None, &destructive);
        }
        popup_menu(&model, anchor, x, y);
    }

    /// Naming it is making it: a row at the foot of the lists becomes the
    /// box to type the name in, and Enter makes the list and opens it.
    /// Leaving the box, or Escape, leaves no list behind.
    fn prompt_new_watchlist(self: &Rc<Self>) {
        let entry = gtk::Entry::new();
        entry.set_placeholder_text(Some("Watchlist name"));
        let row = gtk::ListBoxRow::new();
        row.set_child(Some(&entry));
        row.set_selectable(false);
        row.set_activatable(false);
        self.lists.append(&row);
        entry.grab_focus();

        let this = self.clone();
        entry.connect_activate(move |entry| {
            let name = entry.text().trim().to_string();
            let made = if name.is_empty() { None } else { this.store.add_watchlist(&name) };
            let this = this.clone();
            glib::idle_add_local_once(move || match made {
                // A new watchlist is empty, so there is nothing to look at
                // until the rail is showing it.
                Some(id) => this.open_list(id),
                None => {
                    this.fill_lists();
                    this.focus_lists();
                }
            });
        });

        let gone = {
            let this = self.clone();
            let row = row.downgrade();
            move || {
                if let Some(row) = row.upgrade().filter(|row| row.parent().is_some()) {
                    this.lists.remove(&row);
                }
            }
        };
        // Escape hands the keyboard back; leaving the box takes the row away.
        let keys = gtk::EventControllerKey::new();
        let this = self.clone();
        keys.connect_key_pressed(move |_, key, _, _| {
            if key != gtk::gdk::Key::Escape {
                return glib::Propagation::Proceed;
            }
            this.focus_lists();
            glib::Propagation::Stop
        });
        entry.add_controller(keys);
        let focus = gtk::EventControllerFocus::new();
        focus.connect_leave(move |_| {
            let gone = gone.clone();
            glib::idle_add_local_once(gone);
        });
        entry.add_controller(focus);
    }

    /// Make a section into a watchlist of its own, and show it.
    ///
    /// Nothing is destroyed, so nothing is confirmed: the symbols move. They
    /// move *before* the section goes, so the worst an interrupted promotion
    /// can do is leave them in both places — losing them is not among the
    /// outcomes. A watchlist that cannot be made at all leaves the section
    /// exactly where it was.
    ///
    /// They land in the new watchlist's root rather than in a section of the
    /// same name. The name has become the watchlist's, and saying it twice
    /// would be a list called Energy whose only section is called Energy.
    fn promote_section(self: &Rc<Self>, id: i64, name: &str) {
        let Some(watchlist) = promote_section_in(&self.store, self.active.get(), id, name) else {
            return;
        };
        self.set_active_watchlist(watchlist);
        // The rail is showing the new list already; this is for the bar
        // widget, which may have been reading the one the section came from.
        self.changed();
    }

    /// Deleting a watchlist takes its symbols with it, so it asks first.
    fn confirm_remove_watchlist(self: &Rc<Self>, id: i64, name: &str) {
        let count: usize = self
            .store
            .watchlist_sections(id)
            .iter()
            .map(|section| section.entries.len())
            .sum();

        let body = match count {
            0 => format!("Delete \u{201c}{name}\u{201d}?"),
            1 => format!("Delete \u{201c}{name}\u{201d} and the symbol in it?"),
            n => format!("Delete \u{201c}{name}\u{201d} and the {n} symbols in it?"),
        };

        let dialog = adw::AlertDialog::new(Some("Delete watchlist"), Some(&body));
        dialog.add_response("cancel", "Cancel");
        dialog.add_response("delete", "Delete");
        dialog.set_response_appearance("delete", adw::ResponseAppearance::Destructive);
        dialog.set_default_response(Some("cancel"));
        dialog.set_close_response("cancel");

        let this = self.clone();
        dialog.connect_response(None, move |_, response| {
            if response != "delete" {
                return;
            }
            this.store.remove_watchlist(id);
            // Whatever was being looked at has gone, and the one that is
            // always there is where there is always something to see.
            let fell_back = this.active.get() == id;
            if fell_back {
                this.active.set(DEFAULT_WATCHLIST);
                this.store.set_setting(SETTING_ACTIVE, &DEFAULT_WATCHLIST.to_string());
            }
            // Not `changed()`: the bar widget shows the default watchlist, and
            // that is the one watchlist this cannot have deleted.
            this.rebuild();
            if fell_back {
                this.announce_switch();
            }
        });
        dialog.present(Some(&self.widget));
    }

    /// Add a symbol beside the highlight: the rail's own New.
    pub fn add_symbol(self: &Rc<Self>) {
        let root = self.store.root_section(self.active.get());
        let selected = self.list.selected_row().map(|row| row.index().max(0) as usize);
        let sections: Vec<i64> = self.rows.borrow().iter().map(RowKind::section_id).collect();
        self.add_symbol_to(section_for_new_symbol(&sections, selected, root));
    }

    /// The same, for a key the window owns.
    ///
    /// `false` when the keyboard is not in the rail, so Ctrl+N goes on to make
    /// a chartbook. The decision belongs here rather than in a key controller
    /// on the list: Ctrl+N is an application accelerator, and those are
    /// dispatched above the focused widget, so a controller on the rail would
    /// never be offered the key at all.
    pub fn add_symbol_if_focused(self: &Rc<Self>) -> bool {
        if !self.has_focus() {
            return false;
        }
        self.add_symbol();
        true
    }

    /// Adding uses the same picker as everywhere else.
    pub fn add_symbol_to(self: &Rc<Self>, section_id: i64) {
        let section = self
            .store
            .watchlist_sections(self.active.get())
            .into_iter()
            .find(|s| s.id == section_id);
        let title = match &section {
            Some(section) if !section.root => format!("Add to {}", section.name),
            _ => "Add to watchlist".to_string(),
        };
        let folded = section.map(|section| section.collapsed).unwrap_or(false);
        let this = self.clone();
        self.search.present(&self.widget, &title, move |instrument| {
            this.store.add_to_section(section_id, &instrument.symbol, instrument.suffix.as_deref());
            // Folded, or behind the page of lists, the symbol arrives where
            // nobody can watch it arrive, which reads as the add having
            // quietly failed.
            if folded {
                this.store.set_section_collapsed(section_id, false);
            }
            this.show_symbols();
            this.changed();
            // Land on what was just added. It goes to the end of its section,
            // which on a full rail is below the fold.
            if this.pick(&instrument).is_some()
                && let Some(row) = this.list.selected_row()
            {
                row.grab_focus();
            }
        });
    }

    /// Anchored to whatever asked for it: the + in the band when it was
    /// clicked, and the list itself when it was a key, so the box
    /// opens where the person pressing it is already looking.
    fn prompt_new_section(self: &Rc<Self>, anchor: &impl IsA<gtk::Widget>) {
        let entry = gtk::Entry::new();
        entry.set_placeholder_text(Some("Section name"));

        let popover = gtk::Popover::new();
        popover.set_child(Some(&entry));
        popover.set_parent(anchor);

        // Unparented once it closes, so a box opened on the list itself does
        // not sit among the rows as a child the list cannot take off. From an
        // idle rather than here, because closing happens inside the popover.
        popover.connect_closed(|popover| {
            let popover = popover.clone();
            glib::idle_add_local_once(move || popover.unparent());
        });

        let this = self.clone();
        let popover_weak = popover.downgrade();
        entry.connect_activate(move |entry| {
            let name = entry.text().trim().to_string();
            // Down before the rail is rebuilt under it: the anchor may be one
            // of the rows about to go.
            if let Some(popover) = popover_weak.upgrade() {
                popover.popdown();
            }
            if !name.is_empty() {
                this.store.add_section(this.active.get(), &name);
                this.changed();
            }
        });

        popover.popup();
        entry.grab_focus();
    }

    /// Removing a section takes its symbols with it, so it asks first.
    fn confirm_remove_section(
        self: &Rc<Self>,
        anchor: &impl IsA<gtk::Widget>,
        id: i64,
        name: &str,
    ) {
        let count = self
            .store
            .watchlist_sections(self.active.get())
            .into_iter()
            .find(|s| s.id == id)
            .map(|s| s.entries.len())
            .unwrap_or(0);

        let body = match count {
            0 => format!("Remove “{name}”?"),
            1 => format!("Remove “{name}” and the symbol in it?"),
            n => format!("Remove “{name}” and the {n} symbols in it?"),
        };

        let dialog = adw::AlertDialog::new(Some("Remove section"), Some(&body));
        dialog.add_response("cancel", "Cancel");
        dialog.add_response("remove", "Remove");
        dialog.set_response_appearance("remove", adw::ResponseAppearance::Destructive);
        dialog.set_default_response(Some("cancel"));
        dialog.set_close_response("cancel");

        let this = self.clone();
        dialog.connect_response(None, move |_, response| {
            if response == "remove" {
                this.store.remove_section(id);
                this.changed();
            }
        });
        dialog.present(Some(anchor));
    }
}

/// What a drag in the rail is carrying.
///
/// Headers and symbols sit in one list, so every drop target can be handed
/// either kind. One parse of the payload decides which it got, rather than
/// each target inferring it from the shape of the text and the one that
/// forgets silently doing nothing.
enum Dragged {
    /// A symbol, and the section it is being taken out of.
    Entry { from: i64, entry: Entry },
    /// A section, by id — not by its place in the order, which means a
    /// different section once the rail has been rebuilt mid-drag.
    Section(i64),
}

/// Marks a section's payload. A symbol's first field is a section id, so a
/// word there cannot be mistaken for one.
const SECTION_DRAG: &str = "section";

/// Unpack a dragged row.
fn parse_drag(value: &glib::Value) -> Option<Dragged> {
    let text = value.get::<String>().ok()?;
    let parts: Vec<&str> = text.split('\t').collect();
    match parts.as_slice() {
        [kind, id] if *kind == SECTION_DRAG => Some(Dragged::Section(id.parse().ok()?)),
        [from, symbol, suffix] => Some(Dragged::Entry {
            from: from.parse().ok()?,
            entry: Entry {
                symbol: symbol.to_string(),
                suffix: (!suffix.is_empty()).then(|| suffix.to_string()),
            },
        }),
        _ => None,
    }
}

/// Which side of a section another one lands on.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Beside {
    Before,
    After,
}

/// The section order after `moving` is dropped beside `onto`, or `None` when
/// that works out to the order already stored.
///
/// `order` is the named sections, which is every section a drop can land
/// beside: the nameless root is not in it and never moves.
fn sections_beside(order: &[i64], moving: i64, onto: i64, side: Beside) -> Option<Vec<i64>> {
    let mut out = order.to_vec();
    let from = out.iter().position(|id| *id == moving)?;
    out.remove(from);
    let at = out.iter().position(|id| *id == onto)?;
    out.insert(at + usize::from(side == Beside::After), moving);
    (out != order).then_some(out)
}

/// Which side of the section a header was dropped on the dragged one takes.
///
/// Dropping downwards lands after the header the pointer was over and
/// upwards lands in its place, which is where the pointer was either way.
/// The same rule the indicator list reorders by, so the two gestures do not
/// have to be learned separately.
fn header_drop_side(order: &[i64], moving: i64, onto: i64) -> Beside {
    let at = |wanted: i64| order.iter().position(|id| *id == wanted);
    match (at(moving), at(onto)) {
        (Some(from), Some(to)) if from < to => Beside::After,
        _ => Beside::Before,
    }
}

/// Which section a header dropped on a symbol row lands beside, and on which
/// side.
///
/// A section cannot sit between another's symbols, so a drop in the middle of
/// one goes to whichever of its ends the pointer was nearer — `at` is the
/// dropped-on symbol's place among that section's own rows. Dropping into the
/// loose symbols at the top means first of the named sections: the root has no
/// legal place above it, so both of its ends say the same thing. `None` when
/// there is no named section to land beside at all.
fn row_drop_target(order: &[i64], under: &Section, at: usize) -> Option<(i64, Beside)> {
    if under.root {
        return Some((*order.first()?, Beside::Before));
    }
    let side = match at * 2 < under.entries.len() {
        true => Beside::Before,
        false => Beside::After,
    };
    Some((under.id, side))
}

/// Turn a list's row into the box its new name is typed in, with the old
/// one selected to be typed over.
fn start_rename(stack: &gtk::Stack, rename: &gtk::Entry) {
    stack.set_visible_child_name("entry");
    rename.grab_focus();
    rename.select_region(0, -1);
}

/// Empty the rail, ready to be filled again.
///
/// Walks the siblings rather than asking for the first child over and over,
/// and takes off only what it put on. A list holds more than its rows: the
/// placeholder is a child, and so is any popover anchored on the list itself —
/// the box for naming a new section is, when a key rather than the + in the
/// band opened it. `gtk_list_box_remove` refuses a child that
/// is not a row with a warning and takes nothing off, so a loop that re-reads
/// the first child spins on that popover for ever, which is a frozen app for
/// anyone who names a section from the keyboard.
fn clear_rows(list: &gtk::ListBox) {
    list.set_placeholder(gtk::Widget::NONE);
    let mut child = list.first_child();
    while let Some(widget) = child {
        child = widget.next_sibling();
        if let Some(row) = widget.downcast_ref::<gtk::ListBoxRow>() {
            list.remove(row);
        }
    }
}

/// Which of `walked` — the symbols a step from the chart walks, each with
/// the row it is on — a step starts at, given the highlighted row and what
/// the chart shows. `Watchlist::step_order` has the reasoning.
fn step_from(
    walked: &[(usize, Instrument)],
    selected: Option<usize>,
    current: Option<&Instrument>,
) -> Option<usize> {
    let highlighted = selected.and_then(|row| walked.iter().position(|(at, _)| *at == row));
    let is_current =
        |i: &Instrument| current.is_some_and(|c| c.symbol == i.symbol && c.suffix == i.suffix);
    highlighted
        .filter(|&at| walked.get(at).is_some_and(|(_, i)| is_current(i)))
        .or_else(|| walked.iter().position(|(_, i)| is_current(i)))
        .or(highlighted)
}

/// Put a quote into one value label, direction colouring included.
fn write_cell(
    label: &gtk::Label,
    column: Column,
    quote: Option<Quote>,
    kind: omacharts_engine::InstrumentKind,
) {
    let decimals = quote.map(|q| decimals_for(q.last, kind)).unwrap_or(2);
    for class in ["change-up", "change-down", "change-flat", "dim-label"] {
        label.remove_css_class(class);
    }
    let Some(q) = quote else {
        label.set_text(if column == Column::ChangePct { "" } else { "–" });
        label.add_css_class("dim-label");
        return;
    };
    match column {
        Column::Last => label.set_text(&grouped_decimals(q.last, decimals)),
        Column::Change => {
            // The plus `{:+}` would write: on anything not negative or NaN.
            let plus = if q.change.is_sign_positive() && !q.change.is_nan() { "+" } else { "" };
            label.set_text(&format!("{plus}{}", grouped_decimals(q.change, decimals)))
        }
        Column::ChangePct => label.set_text(&format!("{:+.2}%", q.change_pct)),
        Column::Symbol => {}
    }
    if column != Column::Last {
        // Direction is defined once, in the engine; this just wears it.
        label.add_css_class(omacharts_engine::Direction::of_change(q.change).css_class());
    }
}

/// How many decimals a price of this size deserves.
///
/// The engine owns the rule so the rail and the chart cannot give the same
/// price two ways. There is no gridline here, so only the instrument has an
/// opinion.
fn decimals_for(price: f64, kind: omacharts_engine::InstrumentKind) -> usize {
    omacharts_engine::price_decimals(0.0, price, Some(kind))
}

/// Pop a real menu up where the pointer is.
///
/// A GtkPopoverMenu from a model, not a box of buttons: it is what the
/// platform draws for a context menu, and it behaves like one.
fn popup_menu(model: &gio::Menu, over: &impl IsA<gtk::Widget>, x: f64, y: f64) {
    let popover = gtk::PopoverMenu::from_model(Some(model));
    popover.set_parent(over);
    popover.set_has_arrow(false);
    popover.set_halign(gtk::Align::Start);
    popover.set_pointing_to(Some(&gtk::gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
    // Clicking an item closes the popover and *then* activates its action, so
    // unparenting on close would pull the action context out from under the
    // click. Let the activation happen first.
    popover.connect_closed(|popover| {
        let popover = popover.clone();
        glib::idle_add_local_once(move || popover.unparent());
    });
    // Popped from an idle, not here. GtkPopoverMenu inserts its section
    // separators from an idle after this callback returns, and a popup surface
    // is sized once, when it is first shown — GTK never re-presents a mapped
    // popover for a resize that started inside it. Showing it now measures a
    // menu two separators short of itself, and the scroller inside every
    // GtkPopoverMenu absorbs the difference by scrolling, which is why the last
    // item sat under the bottom corner. The separator sync runs at a higher
    // idle priority than this one, so by the time it pops it is whole.
    glib::idle_add_local_once(move || popover.popup());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::DEFAULT_WATCHLIST;

    /// Naming a section from the keyboard anchors the box on the list itself,
    /// which makes it a child of the list — and then writing the name rebuilds
    /// the rail with it still there. GTK will not take a child that is not a
    /// row off a list, so a loop that empties the list by re-reading its first
    /// child never gets past that popover: one core, for ever, with the whole
    /// app frozen around it. Rebuilding has to finish whatever else is
    /// anchored on the rail.
    ///
    /// Written as the clearing alone because a rail needs a window; a
    /// regression here hangs this test rather than failing it, which is the
    /// one way this can go wrong.
    ///
    /// Ignored by default, so run it deliberately:
    ///
    /// ```sh
    /// cargo test clearing_the_rail -- --ignored
    /// ```
    ///
    /// It needs a real display and GTK all to itself. A build machine has no
    /// display, and libtest hands every test its own thread, so whichever GTK
    /// test gets there first owns GTK and this one dies on the first widget it
    /// touches — a segfault that says nothing about the rail. The bounded walk
    /// itself is six lines; what this pins down is that GTK really does refuse
    /// to take the popover off, which is the part no amount of reasoning can
    /// settle.
    #[test]
    #[ignore = "needs a display and GTK to itself; see the note above"]
    fn clearing_the_rail_finishes_with_a_popover_anchored_on_the_list() {
        if !crate::ui::gtk_ready() {
            return;
        }

        let list = gtk::ListBox::new();
        for symbol in ["ES", "GC", "CL"] {
            list.append(&gtk::Label::new(Some(symbol)));
        }
        list.set_placeholder(Some(&gtk::Label::new(Some("Nothing here yet"))));
        let popover = gtk::Popover::new();
        popover.set_parent(&list);

        clear_rows(&list);

        assert!(list.row_at_index(0).is_none(), "a row was left on the rail");
        assert_eq!(
            popover.parent().as_ref(),
            Some(list.upcast_ref::<gtk::Widget>()),
            "the box being typed into is not the rail's to take away"
        );
        popover.unparent();
    }

    /// The section each row belongs to, laid out the way `rebuild` lays the
    /// rail out: every named section's title, then its symbols, with the
    /// root's symbols carrying no title of their own.
    fn row_sections(store: &Store, watchlist: i64) -> Vec<i64> {
        let mut rows = Vec::new();
        for section in store.watchlist_sections(watchlist) {
            if !section.root {
                rows.push(section.id);
            }
            rows.extend(section.entries.iter().map(|_| section.id));
        }
        rows
    }

    /// Adding while something is highlighted means adding beside it. Going to
    /// the root instead put the symbol at the top of the rail, sections away
    /// from the one it was asked for.
    #[test]
    fn a_new_symbol_joins_the_section_the_highlight_is_in() {
        let store = Store::memory().unwrap();
        let root = store.root_section(DEFAULT_WATCHLIST);
        store.add_to_section(root, "SPY", None);
        let energy = store.add_section(DEFAULT_WATCHLIST, "Energy").unwrap();
        store.add_to_section(energy, "CL", None);
        store.add_to_section(energy, "NG", None);

        let rows = row_sections(&store, DEFAULT_WATCHLIST);
        assert_eq!(rows, vec![root, energy, energy, energy], "SPY, the title, CL, NG");

        assert_eq!(section_for_new_symbol(&rows, Some(3), root), energy, "on a symbol");
        assert_eq!(section_for_new_symbol(&rows, Some(1), root), energy, "on the title");
        assert_eq!(section_for_new_symbol(&rows, Some(0), root), root, "on a loose symbol");
    }

    /// Nothing highlighted names no section, and neither does a highlight left
    /// pointing past the end of a rail that has since grown shorter.
    #[test]
    fn with_nothing_highlighted_a_new_symbol_falls_to_the_root() {
        let store = Store::memory().unwrap();
        let root = store.root_section(DEFAULT_WATCHLIST);
        let energy = store.add_section(DEFAULT_WATCHLIST, "Energy").unwrap();
        store.add_to_section(energy, "CL", None);

        let rows = row_sections(&store, DEFAULT_WATCHLIST);
        assert_eq!(section_for_new_symbol(&rows, None, root), root);
        assert_eq!(section_for_new_symbol(&rows, Some(99), root), root);
    }

    /// The other half of "to the end": the store appends within a section, so
    /// the rail only ever has to choose which section.
    #[test]
    fn a_new_symbol_lands_after_the_ones_already_there() {
        let store = Store::memory().unwrap();
        let energy = store.add_section(DEFAULT_WATCHLIST, "Energy").unwrap();
        for symbol in ["CL", "NG", "BZ"] {
            store.add_to_section(energy, symbol, None);
        }

        let section = store
            .watchlist_sections(DEFAULT_WATCHLIST)
            .into_iter()
            .find(|s| s.id == energy)
            .expect("the section should be there");
        let symbols: Vec<&str> = section.entries.iter().map(|e| e.symbol.as_str()).collect();
        assert_eq!(symbols, vec!["CL", "NG", "BZ"]);
    }

    /// The symbols move and the name goes with them, which is the whole of
    /// what the menu item promises. They land in the new list's root rather
    /// than in a section of the same name: a watchlist called Energy whose
    /// only section is called Energy says it twice.
    #[test]
    fn a_section_turned_into_a_watchlist_takes_its_symbols_and_its_name() {
        let store = Store::memory().unwrap();
        let section = store.add_section(DEFAULT_WATCHLIST, "Energy").unwrap();
        store.add_to_section(section, "CL", None);
        store.add_to_section(section, "NG", None);

        let made = promote_section_in(&store, DEFAULT_WATCHLIST, section, "Energy")
            .expect("the watchlist should be made");

        assert!(store.watchlists().iter().any(|(id, name)| *id == made && name == "Energy"));
        let sections = store.watchlist_sections(made);
        assert_eq!(sections.len(), 1, "the symbols go in the root, not in a section");
        assert!(sections[0].root);
        let symbols: Vec<&str> = sections[0].entries.iter().map(|e| e.symbol.as_str()).collect();
        assert_eq!(symbols, vec!["CL", "NG"]);

        // And it is gone from where it was, rather than left behind empty.
        assert!(
            !store.watchlist_sections(DEFAULT_WATCHLIST).iter().any(|s| s.id == section),
            "the section should not still be in the list it came from",
        );
    }

    /// Two lists called Energy are allowed — nothing resolves a watchlist by
    /// its name — and a section that is not there cannot be promoted at all.
    #[test]
    fn promoting_tolerates_a_name_already_taken_and_refuses_a_section_that_is_not_there() {
        let store = Store::memory().unwrap();
        let first = store.add_section(DEFAULT_WATCHLIST, "Energy").unwrap();
        store.add_to_section(first, "CL", None);
        let second = store.add_section(DEFAULT_WATCHLIST, "Energy").unwrap();
        store.add_to_section(second, "NG", None);

        let one = promote_section_in(&store, DEFAULT_WATCHLIST, first, "Energy").unwrap();
        let two = promote_section_in(&store, DEFAULT_WATCHLIST, second, "Energy").unwrap();
        assert_ne!(one, two, "held by id, so the name may repeat");

        assert_eq!(promote_section_in(&store, DEFAULT_WATCHLIST, 9_999, "Nowhere"), None);
    }

    /// Everything but the folding keys is the list's, so arrows still walk
    /// the rail and Enter on a symbol is still the symbol's.
    #[test]
    fn folding_keys_act_on_the_header_only() {
        let open = RowKind::Header { section_id: 3, collapsed: false };
        let folded = RowKind::Header { section_id: 3, collapsed: true };
        assert_eq!(fold_target(&folded), Some((3, false)));
        assert_eq!(fold_target(&open), Some((3, true)));

        let symbol = RowKind::Entry {
            section_id: 3,
            entry: Entry { symbol: "CL".into(), suffix: None },
            instrument: Instrument {
                symbol: "CL".into(),
                name: "Crude oil".into(),
                kind: omacharts_engine::InstrumentKind::FutureRoot,
                suffix: None,
                currency: None,
                tier: 1,
                session_origin: 0,
                overrides: Vec::new(),
                exchange: None,
                popularity: 0,
                local_name: None,
            },
            cells: Vec::new(),
        };
        assert_eq!(fold_target(&symbol), None, "Enter stays the symbol's");
    }

    /// A handful of lists, so running off one end should land on the other
    /// rather than stopping — the user asked for rotation, and a key that
    /// stops working at the edge is a key you have to think about.
    #[test]
    fn rotating_past_either_end_comes_round_again() {
        let ids = [1, 4, 7];
        assert_eq!(next_in_rotation(&ids, 1, 1), Some(4));
        assert_eq!(next_in_rotation(&ids, 7, 1), Some(1), "off the end is the start");
        assert_eq!(next_in_rotation(&ids, 1, -1), Some(7), "and back the other way");
        assert_eq!(next_in_rotation(&ids, 4, -1), Some(1));
    }

    /// One watchlist has nowhere to rotate to, and a rail pointed at one that
    /// has been deleted should not be resolved by jumping somewhere arbitrary.
    /// Both have to say so rather than pick, because the caller turns `None`
    /// into leaving the key for somebody else.
    #[test]
    fn there_is_nowhere_to_rotate_from_one_watchlist_or_from_none_of_them() {
        assert_eq!(next_in_rotation(&[1], 1, 1), None);
        assert_eq!(next_in_rotation(&[], 1, 1), None);
        assert_eq!(next_in_rotation(&[1, 2], 9, 1), None, "showing something deleted");
    }

    /// The keys have to walk the order the switcher lists, or the two
    /// disagree about what "next" means.
    #[test]
    fn rotation_follows_the_order_the_switcher_shows() {
        let ids = [3, 1, 2];
        let mut seen = vec![3];
        let mut at = 3;
        for _ in 0..2 {
            at = next_in_rotation(&ids, at, 1).unwrap();
            seen.push(at);
        }
        assert_eq!(seen, ids.to_vec(), "display order, not sorted order");
    }

    /// Ctrl+1 is the first list the switcher shows, not the lowest id, and a
    /// number with no list behind it is nobody's.
    #[test]
    fn a_number_key_counts_down_the_switcher_from_one() {
        let ids = [3, 1, 2];
        assert_eq!(numbered(&ids, 1), Some(3), "display order, not sorted order");
        assert_eq!(numbered(&ids, 3), Some(2));
        assert_eq!(numbered(&ids, 4), None, "past the last list");
        assert_eq!(numbered(&ids, 0), None, "zero names nothing");
        assert_eq!(numbered(&[], 1), None);
    }

    /// A section as the store hands one over, for the drop rules that only
    /// care about which section it is and how many rows it has.
    fn section(id: i64, root: bool, symbols: &[&str]) -> Section {
        Section {
            id,
            name: format!("Section {id}"),
            collapsed: false,
            root,
            entries: symbols
                .iter()
                .map(|symbol| Entry { symbol: (*symbol).to_string(), suffix: None })
                .collect(),
        }
    }

    /// The two drags share every drop target in the rail, so a payload one of
    /// them produced must never read as the other's — a header picked up and
    /// read as its first symbol would move the wrong thing.
    #[test]
    fn a_dragged_section_and_a_dragged_symbol_are_told_apart() {
        let header = format!("{SECTION_DRAG}\t7").to_value();
        assert!(matches!(parse_drag(&header), Some(Dragged::Section(7))));

        let Some(Dragged::Entry { from, entry }) = parse_drag(&"3\tNVDA\t".to_value()) else {
            panic!("a symbol's payload should come back as a symbol");
        };
        assert_eq!((from, entry.symbol.as_str(), entry.suffix), (3, "NVDA", None));

        assert!(parse_drag(&"nonsense".to_value()).is_none());
    }

    /// Dropping downwards lands after the header the pointer was over and
    /// upwards lands in its place, which is where the pointer was either way.
    #[test]
    fn a_section_dropped_on_a_header_lands_where_the_pointer_was() {
        let order = [10, 20, 30];

        let side = header_drop_side(&order, 10, 30);
        assert_eq!(side, Beside::After, "dragged down the rail");
        assert_eq!(sections_beside(&order, 10, 30, side), Some(vec![20, 30, 10]));

        let side = header_drop_side(&order, 30, 10);
        assert_eq!(side, Beside::Before, "dragged up the rail");
        assert_eq!(sections_beside(&order, 30, 10, side), Some(vec![30, 10, 20]));
    }

    /// A section cannot sit between another's symbols, so a drop in the middle
    /// of one goes to whichever of its ends the pointer was nearer. Anything
    /// else would be a section's symbols interleaved with another's.
    #[test]
    fn a_section_dropped_among_another_sections_symbols_goes_to_the_nearer_end() {
        let order = [10, 20, 30];
        let onto = section(20, false, &["A", "B", "C", "D"]);

        assert_eq!(row_drop_target(&order, &onto, 0), Some((20, Beside::Before)));
        assert_eq!(row_drop_target(&order, &onto, 1), Some((20, Beside::Before)));
        assert_eq!(row_drop_target(&order, &onto, 2), Some((20, Beside::After)));
        assert_eq!(row_drop_target(&order, &onto, 3), Some((20, Beside::After)));

        assert_eq!(sections_beside(&order, 30, 20, Beside::Before), Some(vec![10, 30, 20]));
        assert_eq!(sections_beside(&order, 10, 20, Beside::After), Some(vec![20, 10, 30]));
    }

    /// The loose symbols at the top are the nameless root: nothing may be
    /// ordered above it, so both of its ends mean first of the named sections.
    #[test]
    fn a_section_dropped_in_the_loose_symbols_at_the_top_becomes_the_first() {
        let order = [10, 20, 30];
        let root = section(1, true, &["SPY", "QQQ", "DIA"]);

        for at in 0..root.entries.len() {
            assert_eq!(row_drop_target(&order, &root, at), Some((10, Beside::Before)));
        }
        assert_eq!(sections_beside(&order, 30, 10, Beside::Before), Some(vec![30, 10, 20]));

        // And a watchlist with no named section has nowhere to put one.
        assert_eq!(row_drop_target(&[], &root, 0), None);
    }

    /// Dropping a section back where it came from — on its own header, or
    /// among its own symbols — is not a move, and rebuilding the rail to put
    /// the rows back exactly as they were would only lose the selection.
    #[test]
    fn a_section_dropped_on_itself_changes_nothing() {
        let order = [10, 20, 30];
        assert_eq!(sections_beside(&order, 20, 20, Beside::Before), None, "its own header");
        assert_eq!(sections_beside(&order, 20, 20, Beside::After), None, "its own symbols");
        assert_eq!(sections_beside(&order, 10, 20, Beside::Before), None, "already in that place");
        assert_eq!(sections_beside(&order, 99, 10, Beside::Before), None, "a section that is gone");
    }

    #[test]
    fn the_symbol_column_is_always_first_and_never_hidden() {
        assert_eq!(parse_columns(Some("change,change_pct"))[0], Column::Symbol);
        assert_eq!(parse_columns(Some("last"))[0], Column::Symbol);
        // Even if someone stores a list without it.
        let columns = parse_columns(Some("change_pct,change"));
        assert_eq!(columns, vec![Column::Symbol, Column::ChangePct, Column::Change]);
    }

    #[test]
    fn column_order_is_respected() {
        let columns = parse_columns(Some("symbol,change_pct,last,change"));
        assert_eq!(
            columns,
            vec![Column::Symbol, Column::ChangePct, Column::Last, Column::Change]
        );
    }

    #[test]
    fn an_empty_or_unreadable_setting_falls_back_to_the_default() {
        assert_eq!(parse_columns(None), Column::DEFAULT.to_vec());
        assert_eq!(parse_columns(Some("")), Column::DEFAULT.to_vec());
        assert_eq!(parse_columns(Some("nonsense,rubbish")), Column::DEFAULT.to_vec());
    }

    #[test]
    fn a_column_dropped_in_a_gap_lands_there() {
        use Column::{Change, ChangePct, Last, Symbol};
        let shown = [Symbol, Last, ChangePct];
        // Carried right, past the column after it: the gap beyond Chg % is
        // the third, and Last ends up after it.
        assert_eq!(dropped(&shown, Last, 3), Some(vec![Symbol, ChangePct, Last]));
        // And back again.
        assert_eq!(dropped(&[Symbol, ChangePct, Last], Last, 1), Some(vec![Symbol, Last, ChangePct]));
        // Dropped in the gap it came from, either side of itself: nothing
        // moves, so the drop is refused rather than rebuilding the rail.
        assert_eq!(dropped(&shown, Last, 1), None);
        assert_eq!(dropped(&shown, Last, 2), None);
        // A column the rail is not showing is not a column to move.
        assert_eq!(dropped(&shown, Change, 1), None);
        // The far end, which is where a pointer that overshoots lands.
        assert_eq!(dropped(&shown, Last, 3), Some(vec![Symbol, ChangePct, Last]));
        // Symbol is first whatever a drop says, which is the rule the rail
        // has always had and the reason the drop goes through `parse_columns`.
        assert_eq!(parse_columns(Some(&columns_to_string(&[Last, ChangePct, Symbol])))[0], Symbol);
        // It does not move, and nothing lands in front of it: a drop over
        // on that side takes the first place that is really a place.
        assert_eq!(dropped(&shown, Symbol, 2), None, "the symbol column stays put");
        assert_eq!(dropped(&shown, Symbol, 0), None);
        assert_eq!(
            dropped(&shown, ChangePct, 0),
            Some(vec![Symbol, ChangePct, Last]),
            "dropped to the left of the symbol, it lands just after it"
        );
        assert_eq!(dropped(&shown, Last, 0), None, "which is where it already was");
    }

    #[test]
    fn a_sort_round_trips_through_its_setting() {
        for sort in [
            Sort { column: Column::ChangePct, descending: true },
            Sort { column: Column::Symbol, descending: false },
        ] {
            assert_eq!(Sort::from_key(&sort.key()), Some(sort));
        }
        assert_eq!(Sort::from_key(""), None, "an empty setting is the list's own order");
        assert_eq!(Sort::from_key("volume"), None);
    }

    #[test]
    fn a_header_click_goes_up_then_down_then_off() {
        let up = Some(Sort { column: Column::Last, descending: false });
        let down = Some(Sort { column: Column::Last, descending: true });
        assert_eq!(Sort::after_click(None, Column::Last), up);
        assert_eq!(Sort::after_click(up, Column::Last), down);
        assert_eq!(Sort::after_click(down, Column::Last), None);
        assert_eq!(
            Sort::after_click(down, Column::Symbol),
            Some(Sort { column: Column::Symbol, descending: false }),
            "another column starts from the bottom"
        );
    }

    #[test]
    fn a_symbol_without_a_price_sorts_last_either_way() {
        let named = |symbol: &str| Instrument {
            symbol: symbol.to_string(),
            name: symbol.to_string(),
            kind: omacharts_engine::InstrumentKind::Equity,
            suffix: None,
            currency: None,
            tier: 1,
            session_origin: 0,
            overrides: Vec::new(),
            exchange: None,
            popularity: 0,
            local_name: None,
        };
        let quote: QuoteLookup = Rc::new(|instrument: &Instrument| {
            match instrument.symbol.as_str() {
                "UP" => Some(Quote { last: 10.0, change: 1.0, change_pct: 5.0 }),
                "DOWN" => Some(Quote { last: 20.0, change: -1.0, change_pct: -2.0 }),
                _ => None,
            }
        });
        let mut rows = [named("NONE"), named("UP"), named("DOWN")];
        for descending in [false, true] {
            let sort = Sort { column: Column::ChangePct, descending };
            rows.sort_by(|a, b| sort.compare(&sort.rank(a, &quote), &sort.rank(b, &quote)));
            let order: Vec<&str> = rows.iter().map(|i| i.symbol.as_str()).collect();
            let expected = if descending { ["UP", "DOWN", "NONE"] } else { ["DOWN", "UP", "NONE"] };
            assert_eq!(order, expected);
        }
    }

    /// A step from the chart starts where the chart is in the list, and
    /// when it is not in the list, where the list was left: never back at
    /// the top, which is what the end of a list used to wrap to.
    #[test]
    fn a_step_starts_where_the_chart_is_or_where_the_list_was_left() {
        let named = |symbol: &str| Instrument {
            symbol: symbol.to_string(),
            name: symbol.to_string(),
            kind: omacharts_engine::InstrumentKind::Equity,
            suffix: None,
            currency: None,
            tier: 1,
            session_origin: 0,
            overrides: Vec::new(),
            exchange: None,
            popularity: 0,
            local_name: None,
        };
        // Row 1 is a section header, so the rows and the walk disagree.
        let rows = [(0, "SPY"), (2, "EURUSD"), (3, "CCJ"), (4, "EURUSD")];
        let walked: Vec<(usize, Instrument)> = rows
            .into_iter()
            .map(|(row, symbol)| (row, named(symbol)))
            .collect();

        assert_eq!(step_from(&walked, Some(3), Some(&named("CCJ"))), Some(2));
        assert_eq!(
            step_from(&walked, None, Some(&named("CCJ"))),
            Some(2),
            "nothing highlighted: found by symbol"
        );
        assert_eq!(
            step_from(&walked, Some(4), Some(&named("EURUSD"))),
            Some(3),
            "listed twice: the copy that is highlighted, not the first"
        );
        assert_eq!(
            step_from(&walked, Some(3), Some(&named("URA"))),
            Some(2),
            "not in the list: from where the list was left"
        );
        assert_eq!(step_from(&walked, None, Some(&named("URA"))), None);
        assert_eq!(step_from(&walked, Some(1), None), None, "a header is not a place in the walk");
    }

    #[test]
    fn columns_round_trip_through_settings() {
        let columns = vec![Column::Symbol, Column::Last, Column::ChangePct];
        let stored = columns_to_string(&columns);
        assert_eq!(parse_columns(Some(&stored)), columns);
    }

    #[test]
    fn prices_get_the_decimals_their_market_quotes() {
        use omacharts_engine::InstrumentKind::*;
        assert_eq!(decimals_for(7722.72, Index), 2, "an index");
        assert_eq!(decimals_for(233.95, Equity), 2, "a stock");
        assert_eq!(decimals_for(84908.12, Crypto), 2, "bitcoin");
        assert_eq!(decimals_for(1.1248, Fx), 5, "a currency major");
        assert_eq!(decimals_for(157.83, Fx), 3, "a yen pair");
        assert_eq!(decimals_for(0.00042, Crypto), 6, "a small coin");
    }

    #[test]
    fn a_currency_move_does_not_round_away_to_nothing() {
        let change = 0.0008_f64;
        let formatted =
            format!("{:+.*}", decimals_for(1.1734, omacharts_engine::InstrumentKind::Fx), change);
        assert_ne!(formatted, "+0.00");
        assert_eq!(formatted, "+0.00080");
    }

    #[test]
    fn the_default_is_symbol_price_and_percent() {
        assert_eq!(
            Column::DEFAULT.to_vec(),
            vec![Column::Symbol, Column::Last, Column::ChangePct]
        );
    }

    #[test]
    fn a_rail_still_on_the_old_default_moves_on() {
        let old = columns_to_string(&Column::PREVIOUS_DEFAULT);
        assert_eq!(parse_columns(Some(&old)), Column::DEFAULT.to_vec());
    }

    #[test]
    fn a_choice_that_merely_includes_the_change_is_left_alone() {
        // Only the exact old default is migrated; anything else was chosen.
        let chosen = columns_to_string(&[Column::Symbol, Column::Change]);
        assert_eq!(
            parse_columns(Some(&chosen)),
            vec![Column::Symbol, Column::Change]
        );
    }
}
