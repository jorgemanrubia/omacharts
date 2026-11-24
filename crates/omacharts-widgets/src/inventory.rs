//! The instruments a symbol picker searches: one index, read without a
//! borrow held, swapped whole when a longer list arrives, and the symbols a
//! feed confirmed kept apart so a swap cannot lose them.
//!
//! Where the instruments come from is the embedder's business: hand
//! [`Inventory::of`] an index, or start from the curated list and
//! [`Inventory::replace`] it.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use omacharts_engine::{Instrument, SearchIndex};

/// The index everything searches, replaced once the long tail has loaded.
///
/// An `Rc` inside the cell rather than the index itself, so a reader takes a
/// cheap clone and holds no borrow while it searches — a borrow held across a
/// GTK callback is a panic waiting for the moment the index is swapped.
#[derive(Clone)]
pub struct Inventory {
    index: Rc<RefCell<Rc<SearchIndex>>>,
    /// Whether the index is the whole catalogue yet or still the curated half.
    ///
    /// Nobody outside can tell the two apart by looking, and the difference
    /// matters to anything that has to resolve a symbol somebody stored
    /// earlier: the curated half cannot name most of what they could have
    /// stored.
    complete: Rc<Cell<bool>>,
    /// Symbols a feed confirmed while this window was open, kept apart so an
    /// index swapped in afterwards — the long tail landing, a refresh —
    /// cannot lose one that was read from disk a moment before it was
    /// written there.
    found: Rc<RefCell<Vec<Instrument>>>,
}

impl Inventory {
    /// `index`, whole: nothing longer is on its way.
    pub fn of(index: SearchIndex) -> Inventory {
        Inventory { index: Rc::new(RefCell::new(Rc::new(index))), complete: Rc::new(Cell::new(true)), found: Rc::default() }
    }

    /// The curated half, ready immediately.
    pub fn curated() -> Inventory {
        Inventory {
            index: Rc::new(RefCell::new(Rc::new(SearchIndex::new(
                omacharts_engine::symbols::seed(),
            )))),
            complete: Rc::new(Cell::new(false)),
            found: Rc::default(),
        }
    }

    pub fn get(&self) -> Rc<SearchIndex> {
        self.index.borrow().clone()
    }

    /// The long tail has landed, so this index can name any listing.
    pub fn complete(&self) -> bool {
        self.complete.get()
    }

    pub fn len(&self) -> usize {
        self.get().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// One instrument by its neutral symbol and optional exchange suffix: a
    /// dated contract, `ESH7`, by the root it is a contract of.
    pub fn find(&self, symbol: &str, suffix: Option<&str>) -> Option<Instrument> {
        self.get().resolve(symbol, suffix)
    }

    pub fn replace(&self, index: SearchIndex) {
        *self.index.borrow_mut() = Rc::new(with_found(index, &self.found.borrow()));
        self.complete.set(true);
    }

    /// The symbols a feed confirmed while this window was open.
    pub fn found(&self) -> Vec<Instrument> {
        self.found.borrow().clone()
    }

    /// Add one symbol a feed confirmed, searchable at once.
    pub fn add_found(&self, instrument: Instrument) {
        self.found.borrow_mut().push(instrument);
        let index = SearchIndex::new(self.get().items().to_vec());
        *self.index.borrow_mut() = Rc::new(with_found(index, &self.found.borrow()));
    }
}

/// `index` with every one of `found` it does not already name.
fn with_found(index: SearchIndex, found: &[Instrument]) -> SearchIndex {
    let missing: Vec<&Instrument> =
        found.iter().filter(|f| index.find(&f.symbol, f.suffix.as_deref()).is_none()).collect();
    if missing.is_empty() {
        return index;
    }
    let mut items = index.items().to_vec();
    items.extend(missing.into_iter().cloned());
    SearchIndex::new(items)
}


/// Whether `query` is a whole word of `name`, whatever the case: `nvidia`
/// of "NVIDIA Corporation", and not `ant` of "Antero Resources". A query a
/// listed name answers to is somebody typing a name, so neither the search
/// nor a command offers it to the feed as a ticker.
pub fn names_it(name: &str, query: &str) -> bool {
    let query = query.trim();
    name.split(|c: char| !c.is_alphanumeric()).any(|word| word.eq_ignore_ascii_case(query))
}

/// A symbol the inventory does not have, if `query` could be one: what the
/// search offers to look up on the feed, and what a command naming a symbol
/// asks the feed about before refusing it.
///
/// Strict about shape rather than about existence: the feed decides whether
/// it exists. `2330.TW` is a ticker on a venue, and a ticker abroad may be
/// all digits — every Taiwanese and Japanese one is. Without a venue a ticker
/// has to start with a letter, which keeps a stray number from being offered
/// as a US listing.
pub fn guess(query: &str) -> Option<Instrument> {
    let typed = query.trim().to_uppercase();
    let (symbol, suffix) = omacharts_engine::symbols::split_suffix(&typed);
    let plausible = (1..=6).contains(&symbol.chars().count())
        && symbol
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || (suffix.is_some() && c.is_ascii_digit()))
        && symbol.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-');
    plausible.then(|| Instrument {
        symbol: symbol.to_string(),
        name: format!("{symbol}: look it up on the feed"),
        kind: omacharts_engine::InstrumentKind::Equity,
        suffix: suffix.map(str::to_string),
        currency: None,
        tier: 2,
        session_origin: 0,
        overrides: Vec::new(),
        exchange: None,
        popularity: 0,
        local_name: None,
        formula: None,
    })
}
