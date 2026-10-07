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
use omacharts_engine::{FetchFailure, Instrument, LinkGroup};

use crate::store::{Entry, Section, Store, DEFAULT_WATCHLIST};
use crate::ui::pane::{link_icon, mark_link, LinkMark};
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

    fn width_chars(self) -> i32 {
        match self {
            Column::Symbol => 0,
            Column::Last => 9,
            Column::Change => 8,
            Column::ChangePct => 7,
        }
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

/// A key that folds sections.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Fold {
    Close,
    Open,
    Toggle,
}

/// The section a folding key acts on from this row, and whether it ends up
/// folded; `None` leaves the key to the list.
fn fold_target(on: &RowKind, key: Fold) -> Option<(i64, bool)> {
    match (on, key) {
        (RowKind::Header { section_id, .. }, Fold::Close) => Some((*section_id, true)),
        (RowKind::Header { section_id, .. }, Fold::Open) => Some((*section_id, false)),
        (RowKind::Header { section_id, collapsed }, Fold::Toggle) => Some((*section_id, !collapsed)),
        (RowKind::Entry { section_id, .. }, Fold::Close) => Some((*section_id, true)),
        _ => None,
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
    on_pick: Rc<dyn Fn(Instrument)>,
    /// Parallel to the list's rows.
    rows: RefCell<Vec<RowKind>>,
    columns: RefCell<Vec<Column>>,
    /// Set while we are selecting a row ourselves, so rebuilding does not
    /// re-load the chart.
    quiet: Cell<bool>,
    /// The chain beside the name: which group of charts this list drives.
    link: gtk::MenuButton,
    link_mark: Rc<RefCell<LinkMark>>,
    link_group: Cell<LinkGroup>,
    /// Which watchlist the rail is showing. Not which one the bar widget
    /// shows — that is always the default one.
    active: Cell<i64>,
    /// Names the watchlist on screen and offers the rest.
    switcher: gtk::MenuButton,
    /// The empty state's first offer, when that is all the rail holds.
    /// Somewhere for the keyboard to land on a list with no rows in it.
    empty_focus: RefCell<Option<gtk::Button>>,
    /// Told whenever the rail starts showing a different watchlist, so the
    /// window can write the choice down against the chartbook it belongs to.
    /// One hook for every way of switching, because a second route to the
    /// same state is a second route that can forget to save.
    on_switch: RefCell<Option<Rc<dyn Fn()>>>,
    /// Asked which chartbook is using a watchlist, so the switcher can refuse
    /// one that is spoken for. The chartbooks live in the window; the rail
    /// only knows how to ask about one list at a time.
    using: RefCell<Option<Rc<dyn Fn(i64) -> Option<String>>>>,
}

const SETTING_COLUMNS: &str = "watchlist_columns";
const SETTING_ACTIVE: &str = "active_watchlist";

impl Watchlist {
    pub fn new(
        store: Rc<Store>,
        index: crate::inventory::Inventory,
        search: Rc<SymbolSearch>,
        quote: QuoteLookup,
        on_pick: impl Fn(Instrument) + 'static,
    ) -> Rc<Watchlist> {
        let list = gtk::ListBox::new();
        list.set_selection_mode(gtk::SelectionMode::Single);
        list.add_css_class("navigation-sidebar");

        let scroller = gtk::ScrolledWindow::new();
        scroller.set_child(Some(&list));
        scroller.set_vexpand(true);
        scroller.set_hscrollbar_policy(gtk::PolicyType::Never);

        // The band across the top of the rail: nothing in it but its height,
        // which `.rail-header` sets. It used to hold the column headings —
        // "Symbol", "Last", "Chg%" — which said nothing a row did not already
        // say for itself, so now it only steps down from under the window's
        // corner controls and stops a few pixels later.
        let header = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        header.add_css_class("rail-header");

        // The step `.rail-header` takes is thirty pixels of nothing, and the
        // corner controls only cover its right-hand end. Naming the watchlist
        // in what is left costs no height at all, which is the only reason
        // the rail can be told what it is showing without giving up a row.
        let switcher = gtk::MenuButton::new();
        switcher.add_css_class("flat");
        switcher.add_css_class("rail-switcher");
        switcher.set_valign(gtk::Align::Center);

        // The chain beside the name, because the question it answers is about
        // this list: which group of charts does picking a symbol here move.
        let link_mark: Rc<RefCell<LinkMark>> = Rc::default();
        let link = gtk::MenuButton::new();
        link.set_child(Some(&link_icon(link_mark.clone())));
        link.add_css_class("flat");
        link.add_css_class("legend-link");
        link.set_always_show_arrow(false);
        link.set_valign(gtk::Align::Center);

        // Over the tickers: a row sits 6px in (Adwaita's sidebar row
        // margin), pads 8px, and its box starts another 12px in, so a
        // symbol's first letter lands 26px from the rail's edge, and so
        // does the name's. The switcher wears no horizontal padding (see
        // `.rail-switcher`), so this margin alone decides it.
        let named = gtk::Box::new(gtk::Orientation::Horizontal, 2);
        named.set_halign(gtk::Align::Start);
        named.set_valign(gtk::Align::Start);
        named.set_margin_start(26);
        named.set_margin_top(1);
        named.append(&switcher);
        named.append(&link);

        let band = gtk::Overlay::new();
        band.set_child(Some(&header));
        band.add_overlay(&named);

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

        let widget = gtk::Box::new(gtk::Orientation::Vertical, 0);
        widget.set_size_request(248, -1);
        widget.append(&band);
        widget.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
        widget.append(&trouble);
        widget.append(&scroller);

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
            rows: RefCell::new(Vec::new()),
            columns: RefCell::new(columns),
            quiet: Cell::new(false),
            link: link.clone(),
            link_mark,
            link_group: Cell::new(LinkGroup::None),
            active: Cell::new(active),
            switcher: switcher.clone(),
            empty_focus: RefCell::new(None),
            on_switch: RefCell::new(None),
            using: RefCell::new(None),
        });

        let actions = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        actions.set_halign(gtk::Align::Center);
        actions.set_margin_top(4);
        actions.set_margin_bottom(4);

        let add_symbol = gtk::Button::from_icon_name("list-add-symbolic");
        let tip = shortcuts::tooltip_with_key("Add a symbol", shortcuts::ADD_SYMBOL);
        add_symbol.set_tooltip_text(Some(&tip));
        add_symbol.add_css_class("flat");
        let this = watchlist.clone();
        add_symbol.connect_clicked(move |_| this.add_symbol());
        actions.append(&add_symbol);

        let add_section = gtk::Button::from_icon_name("folder-new-symbolic");
        let tip = shortcuts::tooltip_with_key("Add a section", shortcuts::ADD_SECTION);
        add_section.set_tooltip_text(Some(&tip));
        add_section.add_css_class("flat");
        let this = watchlist.clone();
        add_section.connect_clicked(move |button| this.prompt_new_section(button));
        actions.append(&add_section);

        watchlist.widget.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
        watchlist.widget.append(&actions);

        watchlist.wire_selection();
        watchlist.wire_keys();
        watchlist.wire_click_to_focus();
        watchlist.wire_switcher();
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
        if id == self.active.get() || !self.store.watchlist_exists(id) {
            return;
        }
        self.active.set(id);
        self.store.set_setting(SETTING_ACTIVE, &id.to_string());
        self.rebuild();
        self.announce_switch();
    }

    /// Hear about the rail being pointed at a different watchlist.
    /// The chain, for the window to hang the group popover off — the list of
    /// groups is the same list a chart offers, and it is built where the
    /// theme that colours it lives.
    pub fn link_button(&self) -> gtk::MenuButton {
        self.link.clone()
    }

    /// Which group picking a symbol here moves.
    pub fn link_group(&self) -> LinkGroup {
        self.link_group.get()
    }

    /// The list already driving `group`, for a row that has to explain why it
    /// cannot be picked. `None` means the group is free, or is this list's.
    pub fn group_held_by(&self, group: LinkGroup) -> Option<(i64, String)> {
        group_held_by(&self.store, group, self.active.get())
    }

    /// Answer which chartbook is using a watchlist, for the switcher.
    ///
    /// Set by the window, because that is where the chartbooks are. Asked
    /// while the menu is being built rather than remembered, so a chartbook
    /// renamed since the last time it opened is named as it is called now.
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

    /// The symbol the rail is on, if it is on one.
    ///
    /// What a list leads its group with when it is put in one. A watchlist has
    /// no symbol of its own, and the row somebody left the selection on is the
    /// nearest thing to one — so a list with nothing selected, or whose
    /// selection is a section header, has nothing to lead with.
    pub fn selected_instrument(&self) -> Option<Instrument> {
        let at = self.list.selected_row()?.index().max(0) as usize;
        match self.rows.borrow().get(at) {
            Some(RowKind::Entry { instrument, .. }) => Some(instrument.clone()),
            _ => None,
        }
    }

    /// Join a group, or leave it. Remembered against the watchlist rather
    /// than the rail, so a list keeps driving the same charts when a
    /// chartbook brings it back.
    ///
    /// Only the group is written here. Making the group follow this list's
    /// symbol is the window's business, because it owns the charts and the
    /// books that are away — and it has to read the group back out of here
    /// afterwards, since a group another list holds is refused below.
    pub fn set_link_group(&self, group: LinkGroup, colour: Option<String>) {
        // A group drives one list. The popover disables a group another list
        // already holds, so reaching here with one is a bug rather than a
        // choice — refuse it instead of quietly stealing it.
        if group_held_by(&self.store, group, self.active.get()).is_some() {
            return;
        }
        self.link_group.set(group);
        self.store.set_setting(
            &link_setting(self.active.get()),
            &group.number().unwrap_or(0).to_string(),
        );
        self.paint_link(group, colour);
    }

    /// The same, for a list that has just been shown: read what it was left
    /// in rather than writing anything down.
    ///
    /// Never leads, whatever it finds. Being shown a list, or redrawing the
    /// rail after a symbol was added to one, is not somebody putting that list
    /// in a group — so this only ever repaints the chain.
    ///
    /// Answers whether the group changed, which is the one thing that has a
    /// consequence: the list then has to be pointed at what its new group is
    /// showing. Without the answer the caller would have to follow on every
    /// redraw, and adding one symbol to a list would jump the selection off
    /// whatever somebody was looking at.
    pub fn adopt_link_group(&self, colour: impl Fn(LinkGroup) -> Option<String>) -> bool {
        let mut stored = stored_group(&self.store, self.active.get());
        // A database written before a group could only be held once can have
        // two lists claiming one. The first keeps it and this one comes back
        // unlinked, written down so the same surprise does not happen twice.
        if group_held_by(&self.store, stored, self.active.get()).is_some() {
            stored = LinkGroup::None;
            self.store.set_setting(&link_setting(self.active.get()), "0");
        }
        let changed = self.link_group.get() != stored;
        self.link_group.set(stored);
        self.paint_link(stored, colour(stored));
        changed
    }

    fn paint_link(&self, group: LinkGroup, colour: Option<String>) {
        self.link.set_tooltip_text(Some(&match group {
            LinkGroup::None => "Not linked — this list drives the chart you are on".to_string(),
            other => format!("Linked · {}", other.label()),
        }));
        self.link.set_opacity(if group.is_linked() { 1.0 } else { 0.28 });
        mark_link(&self.link, &self.link_mark, group, colour);
    }

    /// Select a symbol the way a click does, so whatever is listening for a
    /// pick hears about it.
    ///
    /// Not [`highlight`](Self::highlight), which is deliberately silent: that
    /// one exists for the other direction, a chart moving and the rail
    /// following, and using it here would light up a row and move nothing.
    pub fn pick(self: &Rc<Self>, instrument: &Instrument) -> bool {
        let Some(at) = self.row_of(instrument) else { return false };
        let Some(row) = self.list.row_at_index(at as i32) else { return false };
        // Selecting what is already selected emits nothing, so the pick has
        // to be made by hand — otherwise clicking the symbol you are already
        // on in the bar widget would do nothing at all.
        if self.list.selected_row().as_ref() == Some(&row) {
            (self.on_pick)(instrument.clone());
        } else {
            self.list.select_row(Some(&row));
        }
        true
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

    /// Every watchlist, in display order, as id and name.
    pub fn watchlists(&self) -> Vec<(i64, String)> {
        self.store.watchlists()
    }

    /// Put the keyboard on the rail, landing on something selected so the
    /// arrows have somewhere to go from.
    pub fn grab_focus(self: &Rc<Self>) {
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
        if let Some(row) = self.list.selected_row() {
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
    pub fn flat_order(&self) -> Vec<Instrument> {
        self.rows
            .borrow()
            .iter()
            .enumerate()
            .filter_map(|(at, kind)| match kind {
                RowKind::Entry { instrument, .. } => {
                    let visible = self
                        .list
                        .row_at_index(at as i32)
                        .map(|row| row.is_visible())
                        .unwrap_or(false);
                    visible.then(|| instrument.clone())
                }
                RowKind::Header { .. } => None,
            })
            .collect()
    }

    /// Highlight a symbol without loading it again.
    pub fn highlight(self: &Rc<Self>, instrument: &Instrument) {
        let Some(at) = self.row_of(instrument) else { return };
        let Some(row) = self.list.row_at_index(at as i32) else { return };
        self.quiet.set(true);
        self.list.select_row(Some(&row));
        self.quiet.set(false);
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
                (this.on_pick)(instrument);
            }
        });
    }

    /// Plain arrows move a symbol at a time — the list does that itself.
    /// Ctrl with an arrow jumps a whole section, for a long rail.
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

        // ← folds and → unfolds the section header the keyboard is on, and
        // Enter or Space flips it; ← on a symbol folds the section it is in,
        // the way a tree folds the branch a leaf is on. Caught on the way down,
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
            let fold = match key {
                Key::Left | Key::KP_Left => Fold::Close,
                Key::Right | Key::KP_Right => Fold::Open,
                Key::Return | Key::KP_Enter | Key::space => Fold::Toggle,
                _ => return glib::Propagation::Proceed,
            };
            glib::Propagation::from(this.fold_from_keyboard(fold))
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

    /// Fold or unfold the section where the keyboard is, and leave the
    /// keyboard on its header. A rebuild makes every row new, so without that
    /// the next arrow key would start from nowhere.
    fn fold_from_keyboard(self: &Rc<Self>, key: Fold) -> bool {
        // Only when a row itself has the keyboard: a key in the box renaming a
        // section, or on a button in a header, is that widget's.
        let Some(row) = self.list.root().and_then(|root| root.focus()).and_downcast::<gtk::ListBoxRow>()
        else {
            return false;
        };
        let on = self.rows.borrow().get(row.index().max(0) as usize).cloned();
        let Some((id, folded)) = on.and_then(|on| fold_target(&on, key)) else { return false };
        // The symbols outside any section have no header to fold to.
        if self.header_row(id).is_none() {
            return false;
        }
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

        self.quiet.set(true);
        clear_rows(&self.list);

        self.write_switcher();

        let mut kinds = Vec::new();
        for section in self.store.watchlist_sections(self.active.get()) {
            if !section.root {
                self.list.append(&self.section_header(section.id, &section.name, section.collapsed));
                kinds.push(RowKind::Header { section_id: section.id, collapsed: section.collapsed });
            }
            for entry in &section.entries {
                let Some(instrument) = self.index.find(&entry.symbol, entry.suffix.as_deref()) else {
                    continue;
                };
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

        if let Some(index) = selected {
            if let Some(row) = self.list.row_at_index(index) {
                self.list.select_row(Some(&row));
            }
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
        // stops on one without loading anything, and ← → Enter fold it.
        row.set_selectable(false);
        row.set_activatable(false);

        // Dropping a symbol on a header puts it in that section, at the end.
        // Without this there is no way to move something into a collapsed or
        // empty section. Dropping a section on one puts it beside that section.
        let target = gtk::DropTarget::new(glib::Type::STRING, gtk::gdk::DragAction::MOVE);
        let this = self.clone();
        target.connect_drop(move |_, value, _, _| match parse_drag(value) {
            Some(Dragged::Entry { from, entry }) => {
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
        row_box.set_margin_top(4);
        row_box.set_margin_bottom(4);
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
        row.add_controller(source);

        let target = gtk::DropTarget::new(glib::Type::STRING, gtk::gdk::DragAction::MOVE);
        let this = self.clone();
        let onto = entry.clone();
        target.connect_drop(move |_, value, _, _| match parse_drag(value) {
            Some(Dragged::Entry { from, entry }) => {
                if from == section_id && entry == onto {
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

    /// The switcher names the watchlist on screen, in the strip the corner
    /// controls already reserved.
    ///
    /// The chevron is appended here rather than left to the MenuButton so the
    /// name can ellipsize at a width that clears those controls, instead of
    /// growing under them.
    fn write_switcher(&self) {
        let name = self
            .store
            .watchlists()
            .into_iter()
            .find(|(id, _)| *id == self.active.get())
            .map(|(_, name)| name)
            .unwrap_or_default();

        let content = gtk::Box::new(gtk::Orientation::Horizontal, 2);
        let label = gtk::Label::new(Some(&name));
        label.set_ellipsize(gtk::pango::EllipsizeMode::End);
        label.set_max_width_chars(14);
        label.set_xalign(0.0);
        let chevron = gtk::Image::from_icon_name("pan-down-symbolic");
        chevron.set_pixel_size(12);
        content.append(&label);
        content.append(&chevron);
        self.switcher.set_child(Some(&content));
    }

    /// Build the menu every time it opens, because what it lists is what it
    /// edits: renaming one and opening it again has to show the new name.
    fn wire_switcher(self: &Rc<Self>) {
        let this = self.clone();
        self.switcher.set_create_popup_func(move |button| {
            let popover = gtk::Popover::new();
            popover.add_css_class("menu");
            popover.set_child(Some(&this.switcher_menu(&popover)));
            button.set_popover(Some(&popover));
        });
    }

    fn switcher_menu(self: &Rc<Self>, popover: &gtk::Popover) -> gtk::Box {
        let menu = gtk::Box::new(gtk::Orientation::Vertical, 0);
        menu.set_size_request(200, -1);
        for (id, name) in self.store.watchlists() {
            menu.append(&self.switcher_row(id, &name, popover));
        }
        menu.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
        menu.append(&self.new_watchlist_row(popover));
        menu
    }

    /// One watchlist: its name switches to it, the pencil renames it in
    /// place, and the bin is simply absent on the one that cannot go.
    fn switcher_row(self: &Rc<Self>, id: i64, name: &str, popover: &gtk::Popover) -> gtk::Box {
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 0);

        let tick = gtk::Image::from_icon_name("object-select-symbolic");
        tick.set_pixel_size(12);
        // The space is kept on every row, so the names line up down the menu
        // however the current one moves.
        tick.set_opacity(if id == self.active.get() { 1.0 } else { 0.0 });

        let label = gtk::Label::new(Some(name));
        label.set_xalign(0.0);
        label.set_hexpand(true);
        label.set_ellipsize(gtk::pango::EllipsizeMode::End);
        label.set_max_width_chars(16);

        let content = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        content.append(&tick);
        content.append(&label);

        let pick = gtk::Button::new();
        pick.add_css_class("flat");
        pick.set_hexpand(true);

        // A watchlist is one chartbook's, so one another book is using is not
        // on offer — and the row names the book that has it rather than merely
        // refusing, which would leave nothing to understand. The same two
        // words the chain's popover uses for a group another list holds, so
        // the app says "taken, and here is by whom" one way.
        if let Some(book) = self.chartbook_using(id) {
            let owner = gtk::Label::new(Some(&book));
            owner.add_css_class("dim-label");
            owner.add_css_class("caption");
            owner.set_ellipsize(gtk::pango::EllipsizeMode::End);
            content.append(&owner);
            // The button only: the pencil and the bin act on the watchlist
            // itself, which is nobody's to begin with.
            pick.set_sensitive(false);
        }
        pick.set_child(Some(&content));

        let rename = gtk::Entry::new();
        rename.set_text(name);

        let stack = gtk::Stack::new();
        stack.set_hexpand(true);
        stack.add_named(&pick, Some("label"));
        stack.add_named(&rename, Some("entry"));
        row.append(&stack);

        let this = self.clone();
        let popover_weak = popover.downgrade();
        pick.connect_clicked(move |_| {
            this.set_active_watchlist(id);
            if let Some(popover) = popover_weak.upgrade() {
                popover.popdown();
            }
        });

        let edit = gtk::Button::from_icon_name("document-edit-symbolic");
        edit.add_css_class("flat");
        edit.set_tooltip_text(Some("Rename"));
        let stack_weak = stack.downgrade();
        let rename_weak = rename.downgrade();
        edit.connect_clicked(move |_| {
            if let (Some(stack), Some(rename)) = (stack_weak.upgrade(), rename_weak.upgrade()) {
                stack.set_visible_child_name("entry");
                rename.grab_focus();
                rename.select_region(0, -1);
            }
        });
        row.append(&edit);

        let this = self.clone();
        let stack_weak = stack.downgrade();
        let label_weak = label.downgrade();
        rename.connect_activate(move |entry| {
            let text = entry.text().trim().to_string();
            if !text.is_empty() {
                this.store.rename_watchlist(id, &text);
                // The menu is rebuilt the next time it opens, but this one is
                // still on screen and would otherwise go on showing the name
                // that has just been changed.
                if let Some(label) = label_weak.upgrade() {
                    label.set_text(&text);
                }
                this.rebuild();
            }
            if let Some(stack) = stack_weak.upgrade() {
                stack.set_visible_child_name("label");
            }
        });

        // The default watchlist has no bin at all. A disabled one would be a
        // control that exists to be refused, and the rule it enforces —
        // something has to be left — is not worth explaining twice.
        if id != DEFAULT_WATCHLIST {
            let remove = gtk::Button::from_icon_name("user-trash-symbolic");
            remove.add_css_class("flat");
            remove.set_tooltip_text(Some("Delete"));
            let this = self.clone();
            let popover_weak = popover.downgrade();
            let name = name.to_string();
            remove.connect_clicked(move |_| {
                if let Some(popover) = popover_weak.upgrade() {
                    popover.popdown();
                }
                this.confirm_remove_watchlist(id, &name);
            });
            row.append(&remove);
        }

        row
    }

    /// Naming it is making it, so the button becomes the field rather than
    /// opening a second popover on top of this one.
    fn new_watchlist_row(self: &Rc<Self>, popover: &gtk::Popover) -> gtk::Stack {
        let stack = gtk::Stack::new();

        let add = gtk::Button::new();
        add.add_css_class("flat");
        let content = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let icon = gtk::Image::from_icon_name("list-add-symbolic");
        icon.set_pixel_size(12);
        let label = gtk::Label::new(Some("New watchlist"));
        label.set_xalign(0.0);
        label.set_hexpand(true);
        content.append(&icon);
        content.append(&label);
        add.set_child(Some(&content));

        let entry = gtk::Entry::new();
        entry.set_placeholder_text(Some("Watchlist name"));

        stack.add_named(&add, Some("button"));
        stack.add_named(&entry, Some("entry"));

        let stack_weak = stack.downgrade();
        let entry_weak = entry.downgrade();
        add.connect_clicked(move |_| {
            if let (Some(stack), Some(entry)) = (stack_weak.upgrade(), entry_weak.upgrade()) {
                stack.set_visible_child_name("entry");
                entry.grab_focus();
            }
        });

        let this = self.clone();
        let popover_weak = popover.downgrade();
        entry.connect_activate(move |entry| {
            let name = entry.text().trim().to_string();
            if !name.is_empty() {
                // A new watchlist is empty, so there is nothing to look at
                // until the rail is showing it.
                if let Some(id) = this.store.add_watchlist(&name) {
                    this.set_active_watchlist(id);
                }
            }
            if let Some(popover) = popover_weak.upgrade() {
                popover.popdown();
            }
        });

        stack
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
            // Or the group leaks: every row in the popover eventually reads
            // as taken, by a list nobody can find.
            this.store.set_setting(&link_setting(id), "0");
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
            // Folded, the symbol arrives where nobody can watch it arrive,
            // which reads as the add having quietly failed.
            if folded {
                this.store.set_section_collapsed(section_id, false);
            }
            this.changed();
            // Land on what was just added. It goes to the end of its section,
            // which on a full rail is below the fold.
            if this.pick(&instrument)
                && let Some(row) = this.list.selected_row()
            {
                row.grab_focus();
            }
        });
    }

    /// Anchored to whatever asked for it: the control at the foot of the rail
    /// when it was clicked, and the list itself when it was a key, so the box
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

/// The group a watchlist is left in, read from the one place it is written.
///
/// A list with nothing written down is unlinked, except the default one,
/// which drives group 1. Charts start in group 1 too, so out of the box the
/// watchlist drives the charts — which is what linking did before there were
/// groups to choose between. A default that left the two in different groups
/// would make a fresh install less useful than the version before it.
fn stored_group(store: &Store, watchlist: i64) -> LinkGroup {
    match store.setting(&link_setting(watchlist)) {
        Some(value) => value.parse::<u8>().ok().map(LinkGroup::numbered).unwrap_or_default(),
        None if watchlist == DEFAULT_WATCHLIST => LinkGroup::numbered(1),
        None => LinkGroup::None,
    }
}

/// Which watchlist holds each group, as group, id and name.
///
/// A scan rather than an index. There are a handful of lists, and a second
/// copy of who holds what is a second copy that can come to disagree with the
/// first — the settings are the one source of truth and this reads them.
///
/// Many charts share a group, but only one list drives it: a group is the
/// thing a list drives. Where two claim one — possible in a database written
/// before that was enforced — the earlier in display order keeps it, so the
/// answer does not depend on which list happened to be opened first.
pub fn group_owners(store: &Store) -> Vec<(u8, i64, String)> {
    let mut held: Vec<(u8, i64, String)> = Vec::new();
    for (id, name) in store.watchlists() {
        let Some(group) = stored_group(store, id).number() else { continue };
        if held.iter().any(|(taken, ..)| *taken == group) {
            continue;
        }
        held.push((group, id, name));
    }
    held
}

/// The list already driving `group`, if it is not this one.
pub fn group_held_by(store: &Store, group: LinkGroup, besides: i64) -> Option<(i64, String)> {
    let number = group.number()?;
    group_owners(store)
        .into_iter()
        .find(|(taken, id, _)| *taken == number && *id != besides)
        .map(|(_, id, name)| (id, name))
}

/// Where a watchlist's group is written down.
///
/// Against the watchlist's id rather than the rail, because the rail shows a
/// different list from one chartbook to the next and the group belongs to the
/// list — an energy list that drives group 3 should still drive group 3 when
/// another book brings it back.
fn link_setting(id: i64) -> String {
    format!("watchlist_link_{id}")
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

/// Empty the rail, ready to be filled again.
///
/// Walks the siblings rather than asking for the first child over and over,
/// and takes off only what it put on. A list holds more than its rows: the
/// placeholder is a child, and so is any popover anchored on the list itself —
/// the box for naming a new section is, when a key rather than the button at
/// the foot of the rail opened it. `gtk_list_box_remove` refuses a child that
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
        Column::Last => label.set_text(&format!("{:.*}", decimals, q.last)),
        Column::Change => label.set_text(&format!("{:+.*}", decimals, q.change)),
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

    /// Out of the box the watchlist drives the charts, which is what linking
    /// did before there were groups to pick between. Charts start in group 1,
    /// so the default list has to as well or a fresh install links nothing to
    /// anything.
    #[test]
    fn the_default_watchlist_starts_driving_group_one() {
        let store = Store::memory().unwrap();
        assert_eq!(stored_group(&store, DEFAULT_WATCHLIST), LinkGroup::numbered(1));

        // Any other list starts out of the way.
        let scratch = store.add_watchlist("Scratch").expect("made");
        assert_eq!(stored_group(&store, scratch), LinkGroup::None);
    }

    /// Many charts share a group; one list drives it. So a group another list
    /// already holds is not available, and the row that says so needs to name
    /// the list holding it.
    #[test]
    fn a_group_drives_one_watchlist_and_says_who_holds_it() {
        let store = Store::memory().unwrap();
        let scratch = store.add_watchlist("Scratch").expect("made");

        // The default list holds group 1 without anything being written down.
        assert_eq!(
            group_held_by(&store, LinkGroup::numbered(1), scratch),
            Some((DEFAULT_WATCHLIST, "Default".to_string())),
        );
        // And nothing holds group 2.
        assert_eq!(group_held_by(&store, LinkGroup::numbered(2), scratch), None);
        // A list never collides with itself.
        assert_eq!(group_held_by(&store, LinkGroup::numbered(1), DEFAULT_WATCHLIST), None);
    }

    /// Deleting a list has to give its group back, or the popover fills up
    /// with groups held by lists nobody can find.
    #[test]
    fn deleting_a_watchlist_frees_the_group_it_held() {
        let store = Store::memory().unwrap();
        let energy = store.add_watchlist("Energy").expect("made");
        store.set_setting(&link_setting(energy), "3");
        assert!(group_held_by(&store, LinkGroup::numbered(3), DEFAULT_WATCHLIST).is_some());

        store.remove_watchlist(energy);
        assert_eq!(group_held_by(&store, LinkGroup::numbered(3), DEFAULT_WATCHLIST), None);

        // And the row itself is gone, not merely ignored. Left behind it
        // would be read again by anything that walks the settings, and would
        // come back to life under a list that reused the id.
        assert_eq!(store.setting(&link_setting(energy)), None, "the group outlived its list");
    }

    /// `remove_watchlist` deletes this row, and spells the key itself rather
    /// than reaching into this module for it. Two spellings of one key is a
    /// cleanup that stops happening the day either side is reworded, so the
    /// agreement is pinned here where the name is defined.
    #[test]
    fn the_key_the_store_clears_is_the_key_this_writes() {
        let store = Store::memory().unwrap();
        let list = store.add_watchlist("Metals").expect("made");
        store.set_setting(&link_setting(list), "5");
        assert_eq!(store.setting(&format!("watchlist_link_{list}")).as_deref(), Some("5"));
    }

    /// Nothing stopped two lists claiming one group until now, so a database
    /// can already hold that. Display order decides, rather than whichever
    /// list the window happened to open first.
    #[test]
    fn a_group_claimed_twice_stays_with_the_earlier_list() {
        let store = Store::memory().unwrap();
        let first = store.add_watchlist("Energy").expect("made");
        let second = store.add_watchlist("Metals").expect("made");
        store.set_setting(&link_setting(first), "4");
        store.set_setting(&link_setting(second), "4");

        let owners = group_owners(&store);
        let holders: Vec<i64> =
            owners.iter().filter(|(group, ..)| *group == 4).map(|(_, id, _)| *id).collect();
        assert_eq!(holders, vec![first], "only one list may hold a group");
        assert!(
            group_held_by(&store, LinkGroup::numbered(4), second).is_some(),
            "the later list is told the group is taken rather than sharing it",
        );
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
    fn folding_keys_act_on_the_header_or_the_section_a_symbol_is_in() {
        let open = RowKind::Header { section_id: 3, collapsed: false };
        let folded = RowKind::Header { section_id: 3, collapsed: true };
        assert_eq!(fold_target(&open, Fold::Close), Some((3, true)));
        assert_eq!(fold_target(&folded, Fold::Open), Some((3, false)));
        assert_eq!(fold_target(&folded, Fold::Toggle), Some((3, false)));
        assert_eq!(fold_target(&open, Fold::Toggle), Some((3, true)));

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
        assert_eq!(fold_target(&symbol, Fold::Close), Some((3, true)));
        assert_eq!(fold_target(&symbol, Fold::Open), None);
        assert_eq!(fold_target(&symbol, Fold::Toggle), None, "Enter stays the symbol's");
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
