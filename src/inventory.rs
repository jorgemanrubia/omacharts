//! The symbol inventory, and when it arrives.
//!
//! Two halves. The curated one is written by hand and compiled in: a few
//! hundred instruments carrying the tier weights that make `GC` mean gold
//! futures, the session origins that make a four-hour bar bucket correctly,
//! and every future, currency pair and crypto, none of which appear in any
//! listings feed. The generated one is every US-listed equity and ETF, built
//! from Nasdaq Trader's daily files by `tools/build_listings.py`, and every
//! listing on the two Taiwanese exchanges, built from the TWSE's registry by
//! `tools/build_taiwan_listings.py`.
//!
//! Only the curated half is ready when the window opens. The other eleven
//! thousand are parsed on a thread afterwards and swapped in, because five
//! milliseconds on the startup path is five milliseconds, and the names
//! somebody is most likely to type are in the half that is already there.
//!
//! A file in the data directory beats the compiled copy when it was built
//! later, which is how the inventory is refreshed without a new binary: see
//! [`crate::listings`], which fetches it once a day.
//!
//! And a third, small one: symbols nobody has listed yet that a data feed
//! said it knows, kept from the moment somebody asked ([`remember`]). A
//! company that listed this morning has prices before it has a row in any
//! file, and once looked up it stays findable — in a watchlist, a chart
//! restored at the next launch, a command — until the listings carry it.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::rc::Rc;

use omacharts_engine::{Instrument, SearchIndex};

/// Every US listing, built in CI rather than at runtime: a failed download is
/// then a red build rather than a broken app.
const LISTINGS: &str = include_str!("../crates/omacharts-engine/src/listings.tsv");

/// Every TWSE and TPEx listing, built the same way.
const TAIWAN_LISTINGS: &str = include_str!("../crates/omacharts-engine/src/listings_tw.tsv");

/// The US listings compiled into this binary.
pub fn compiled() -> &'static str {
    LISTINGS
}

/// Where a refreshed inventory is written: beside the store.
pub fn listings_path() -> PathBuf {
    crate::listings::path_in(&crate::listings::dir())
}

/// The symbols a feed confirmed that no listing had: see [`remember`].
pub fn found_path() -> PathBuf {
    crate::store::data_dir().join("found.tsv")
}

/// The US listings to search: downloaded or compiled in, whichever was built
/// later. A download that never happened, or went wrong, leaves the app
/// exactly as it shipped rather than without an inventory.
pub fn us_listings() -> String {
    let downloaded = std::fs::read_to_string(listings_path()).ok();
    crate::listings::choose(downloaded.as_deref(), LISTINGS).to_string()
}

/// Every generated file as one text, the symbols feeds found last. They are
/// separate files because they come from unrelated sources that fail
/// independently; a found symbol a listing has since taken in is dropped by
/// [`merge`], which keeps the first row of any symbol.
fn generated(us: &str) -> String {
    let found = std::fs::read_to_string(found_path()).unwrap_or_default();
    format!("{us}\n{TAIWAN_LISTINGS}\n{found}")
}

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

    pub(crate) fn replace(&self, index: SearchIndex) {
        *self.index.borrow_mut() = Rc::new(with_found(index, &self.found.borrow()));
        self.complete.set(true);
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

/// Every instrument, built now rather than on a thread.
///
/// The window never calls this — it shows the curated half immediately and
/// swaps the rest in behind the first frame, which is what keeps startup
/// fast. A command-line process has no frame to be behind and exits in
/// milliseconds, so it pays the few milliseconds once and searches
/// everything.
pub fn everything() -> SearchIndex {
    SearchIndex::new(merge(&generated(&us_listings()), omacharts_engine::symbols::seed()))
}

/// The generated listings and the curated rows, as one list.
///
/// A curated row wins any collision, because it is the one carrying a tier, a
/// session origin and a provider override — judgements no feed can make. AAPL
/// is in both files and must come out tier 0.
///
/// But winning is not the same as arriving alone. The feed knows two things
/// about AAPL that the hand-written row does not: which venue lists it, and
/// that it is the most searched ticker there is. So a curated row inherits
/// whatever it left blank from the listing it displaces, and keeps everything
/// it filled in. Without this the two hundred symbols people actually type
/// would be the only ones with no venue and no ranking — the exact inverse of
/// what either column is for.
///
/// Only from a listing of its own kind, though. `ES` is the E-mini and also
/// Eversource Energy; `MGC` is micro gold and also a Vanguard fund. Sharing
/// four letters with a stock is not a reason to claim its exchange, and the
/// futures root would have gone out labelled NYSE.
pub fn merge(listings: &str, mut curated: Vec<Instrument>) -> Vec<Instrument> {
    let mut items = omacharts_engine::symbols::parse_seed(listings);
    // The first row of a symbol is the one kept: a listing over a symbol a
    // feed found before the listings had it.
    let mut seen: HashSet<(String, Option<String>)> = HashSet::new();
    items.retain(|i| seen.insert((i.symbol.clone(), i.suffix.clone())));

    let listed: HashMap<(&str, Option<&str>), &Instrument> =
        items.iter().map(|i| ((i.symbol.as_str(), i.suffix.as_deref()), i)).collect();
    for row in &mut curated {
        let inherited = listed
            .get(&(row.symbol.as_str(), row.suffix.as_deref()))
            .filter(|l| l.kind == row.kind)
            .map(|l| (l.exchange.clone(), l.popularity, l.local_name.clone()));
        let Some((exchange, popularity, local_name)) = inherited else {
            continue;
        };
        if row.exchange.is_none() {
            row.exchange = exchange;
        }
        if row.local_name.is_none() {
            row.local_name = local_name;
        }
        if row.popularity == 0 {
            row.popularity = popularity;
        }
    }

    let won: HashSet<(&str, Option<&str>)> =
        curated.iter().map(|c| (c.symbol.as_str(), c.suffix.as_deref())).collect();
    items.retain(|l| !won.contains(&(l.symbol.as_str(), l.suffix.as_deref())));
    items.extend(curated);
    items
}

/// Parse the listings off the main thread and hand back one merged index.
pub fn load_in_background(inventory: Inventory, done: impl Fn(usize) + 'static) {
    let (sender, receiver) = async_channel::bounded::<Vec<Instrument>>(1);

    std::thread::spawn(move || {
        let _ = sender.send_blocking(merge(&generated(&us_listings()), omacharts_engine::symbols::seed()));
    });

    glib::spawn_future_local(async move {
        if let Ok(items) = receiver.recv().await {
            let count = items.len();
            inventory.replace(SearchIndex::new(items));
            done(count);
        }
    });
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

/// Why a symbol could not be looked up, in a sentence for the person who
/// asked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NotLookedUp {
    /// The feed answered, and has no such symbol.
    Unknown(String),
    /// No answer worth believing either way: offline, refused, no feed.
    Unanswered(String),
}

impl NotLookedUp {
    pub fn message(&self) -> &str {
        match self {
            NotLookedUp::Unknown(said) | NotLookedUp::Unanswered(said) => said,
        }
    }
}

/// Ask the feed that would chart `guess` whether it knows it, and the
/// instrument it is if so: named, and a stock or a fund, as far as the feed
/// can say. Blocks on the network.
pub fn look_up(feeds: &crate::feeds::Feeds, guess: &Instrument) -> Result<Instrument, NotLookedUp> {
    let shown = guess.display_symbol();
    let Some((provider, native)) = feeds.route(guess) else {
        return Err(NotLookedUp::Unanswered(format!("no data feed here charts {shown}")));
    };
    let described = provider.describe(&native).map_err(|error| match error {
        omacharts_engine::ProviderError::NotFound => {
            NotLookedUp::Unknown(format!("{} does not know {shown}", provider.label()))
        }
        other => NotLookedUp::Unanswered(format!("{} could not look up {shown}: {other}", provider.label())),
    })?;
    Ok(Instrument {
        name: described.name.unwrap_or_else(|| guess.symbol.clone()),
        kind: described.kind.unwrap_or(guess.kind),
        exchange: described.exchange,
        currency: described.currency,
        ..guess.clone()
    })
}

/// The feeds a command asks about a symbol nobody listed: the order the
/// window charts from. A test build asks [`testing::Newly`] instead, which
/// knows two invented symbols and nothing else, so no test reaches the
/// network.
#[cfg(not(test))]
pub fn look_up_feeds(store: &crate::store::Store) -> crate::feeds::Feeds {
    crate::feeds::Feeds::load(store)
}

#[cfg(test)]
pub fn look_up_feeds(_store: &crate::store::Store) -> crate::feeds::Feeds {
    crate::feeds::Feeds::of(vec![Box::new(testing::Newly)])
}

/// A feed for tests that knows two symbols no listing has, and nothing else.
#[cfg(test)]
pub mod testing {
    use omacharts_engine::{Bar, Capability, Described, Instrument, InstrumentKind, Provider, ProviderError, Timeframe};

    /// Knows `ZQNEW`, a stock it can name, and `ZQFND`, a fund it can only
    /// say has bars — the default answer, through the default `describe`.
    pub struct Newly;

    impl Provider for Newly {
        fn id(&self) -> &'static str {
            "newly"
        }
        fn label(&self) -> &'static str {
            "Test Feed"
        }
        fn delay_minutes(&self, _: InstrumentKind) -> u32 {
            0
        }
        fn native_symbol(&self, instrument: &Instrument) -> Option<String> {
            matches!(instrument.kind, InstrumentKind::Equity | InstrumentKind::Etf).then(|| instrument.display_symbol())
        }
        fn capabilities(&self) -> &'static [Capability] {
            &[]
        }
        fn bars(&self, symbol: &str, _: Timeframe, _: Option<i64>) -> Result<Vec<Bar>, ProviderError> {
            match symbol {
                "ZQNEW" | "ZQFND" => Ok(vec![Bar { ts: 1_795_000_000, open: 1.0, high: 2.0, low: 0.5, close: 1.5, volume: 10.0 }]),
                _ => Err(ProviderError::NotFound),
            }
        }
        fn describe(&self, symbol: &str) -> Result<Described, ProviderError> {
            match symbol {
                "ZQNEW" => Ok(Described {
                    name: Some("Zq Newly Listed, Inc.".into()),
                    kind: Some(InstrumentKind::Equity),
                    exchange: Some("NasdaqGS".into()),
                    currency: Some("USD".into()),
                }),
                "ZQOFF" => Err(ProviderError::Offline("no route to the feed".into())),
                // The trait's own answer: a daily fetch.
                _ => match self.bars(symbol, Timeframe::days(1), None)?.is_empty() {
                    true => Err(ProviderError::NotFound),
                    false => Ok(Described::default()),
                },
            }
        }
    }
}

/// Keep a symbol a feed confirmed, so that it is in the inventory of every
/// process from now on. One row per symbol, the newest kept; written whole.
pub fn remember(found: &Instrument) -> std::io::Result<()> {
    remember_in(&found_path(), found)
}

fn remember_in(path: &std::path::Path, found: &Instrument) -> std::io::Result<()> {
    let clean = |text: &str| text.replace(['\t', '\n', '\r'], " ");
    let row = format!(
        "{}\t{}\t{}\t{}\t{}\t2\t0\t-\t{}\t-",
        if found.kind == omacharts_engine::InstrumentKind::Etf { "etf" } else { "equity" },
        clean(&found.symbol),
        clean(&found.name),
        found.suffix.as_deref().map(clean).unwrap_or_else(|| "-".into()),
        found.currency.as_deref().map(clean).unwrap_or_else(|| "-".into()),
        found.exchange.as_deref().map(clean).unwrap_or_else(|| "-".into()),
    );
    let before = std::fs::read_to_string(path).unwrap_or_default();
    let same = |line: &str| {
        let fields: Vec<&str> = line.split('\t').collect();
        fields.get(1) == Some(&found.symbol.as_str())
            && fields.get(3).copied().filter(|s| *s != "-") == found.suffix.as_deref()
    };
    let mut text: String = before
        .lines()
        .filter(|line| line.starts_with('#') || !same(line))
        .map(|line| format!("{line}\n"))
        .collect();
    if text.is_empty() {
        text.push_str("# Symbols a data feed confirmed before any listing had them. Written by the app.\n");
    }
    text.push_str(&row);
    text.push('\n');
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let incoming = path.with_extension(format!("incoming-{}", std::process::id()));
    std::fs::write(&incoming, text)?;
    std::fs::rename(&incoming, path)
}

use gtk::glib;

#[cfg(test)]
mod tests {
    use super::*;

    /// The generated file is written from a listings feed that knows nothing
    /// about how well known a symbol is, so a name in both files has to come
    /// back as the curated one or search stops ranking.
    #[test]
    fn a_curated_row_beats_a_generated_one() {
        let curated = omacharts_engine::symbols::seed();
        let merged = merge(LISTINGS, curated.clone());

        let aapl: Vec<&Instrument> = merged.iter().filter(|i| i.symbol == "AAPL").collect();
        assert_eq!(aapl.len(), 1, "AAPL is in both files and must appear once");
        assert_eq!(aapl[0].tier, 0, "and must keep the curated tier");

        for one in &curated {
            assert!(
                merged.iter().any(|m| m.symbol == one.symbol && m.tier == one.tier),
                "{} lost its curated row",
                one.symbol
            );
        }
    }

    /// The curated half is written by hand and carries no venue and no
    /// ranking, so without inheritance the two hundred symbols people actually
    /// type would be the only ones missing both — AAPL showing no exchange
    /// while every AAPL-derived ETF showed NASDAQ, and the most searched
    /// ticker there is scoring below a microcap.
    #[test]
    fn a_curated_row_inherits_what_it_left_blank() {
        let merged = merge(LISTINGS, omacharts_engine::symbols::seed());
        let aapl = merged.iter().find(|i| i.symbol == "AAPL").expect("no AAPL");

        assert_eq!(aapl.tier, 0, "the curated tier is the whole point and must survive");
        assert_eq!(aapl.name, "Apple", "as is the curated name");
        assert_eq!(aapl.exchange.as_deref(), Some("NASDAQ"), "venue comes from the listing");
        assert_eq!(aapl.popularity, 9, "and so does the ranking");
    }

    /// `ES` is the E-mini S&P and also Eversource Energy. One ticker, two
    /// unrelated things, and the curated row must not come out wearing the
    /// stock's exchange.
    #[test]
    fn a_curated_row_inherits_nothing_from_another_kind() {
        let merged = merge(LISTINGS, omacharts_engine::symbols::seed());
        let es = merged.iter().find(|i| i.symbol == "ES").expect("no ES");
        assert_eq!(es.kind, omacharts_engine::InstrumentKind::FutureRoot);
        assert_eq!(es.exchange, None, "a futures root does not list on NYSE");
        assert_eq!(es.popularity, 0, "nor does it borrow a utility's market cap");
    }

    #[test]
    fn the_generated_half_is_there_and_ranked_below() {
        let merged = merge(LISTINGS, omacharts_engine::symbols::seed());
        assert!(merged.len() > 10_000, "only {} instruments", merged.len());
        let generated = merged.iter().find(|i| i.symbol == "FOXF").expect("a long-tail listing");
        assert_eq!(generated.tier, 2, "the long tail must not outrank a major");
    }

    /// Futures, currencies and crypto are in no listings feed; they exist only
    /// because somebody wrote them down.
    #[test]
    fn the_curated_kinds_survive_the_merge() {
        let merged = merge(LISTINGS, omacharts_engine::symbols::seed());
        for symbol in ["GC", "EURUSD", "BTC", "GSPC"] {
            assert!(merged.iter().any(|i| i.symbol == symbol), "{symbol} went missing");
        }
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("omacharts-found-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    /// The shapes worth asking a feed about, and the ones not worth it.
    #[test]
    fn only_a_ticker_shape_is_a_guess() {
        let ant = guess("ant").expect("a ticker");
        assert_eq!((ant.symbol.as_str(), ant.suffix.as_deref()), ("ANT", None));
        let tw = guess("2330.tw").expect("a ticker abroad may be digits");
        assert_eq!((tw.symbol.as_str(), tw.suffix.as_deref()), ("2330", Some("TW")));
        for not in ["", "2330", "semiconductor", "GC/SI", "a b", "$$"] {
            assert!(guess(not).is_none(), "{not:?}");
        }
    }

    /// The feed is asked, and what it says is the instrument: named, with
    /// its kind and venue, when it can say them; the symbol alone when all
    /// it can say is that it has bars.
    #[test]
    fn a_feed_names_what_no_listing_has() {
        let feeds = crate::feeds::Feeds::of(vec![Box::new(testing::Newly)]);
        let new = look_up(&feeds, &guess("zqnew").unwrap()).unwrap();
        assert_eq!(new.symbol, "ZQNEW");
        assert_eq!(new.name, "Zq Newly Listed, Inc.");
        assert_eq!(new.kind, omacharts_engine::InstrumentKind::Equity);
        assert_eq!(new.exchange.as_deref(), Some("NasdaqGS"));
        assert_eq!(new.currency.as_deref(), Some("USD"));
        assert_eq!(new.tier, 2, "below anything curated, like any listing");

        let bare = look_up(&feeds, &guess("ZQFND").unwrap()).unwrap();
        assert_eq!(bare.name, "ZQFND", "nothing more to say than that it exists");

        let unknown = look_up(&feeds, &guess("ZZZQX").unwrap()).unwrap_err();
        assert_eq!(unknown, NotLookedUp::Unknown("Test Feed does not know ZZZQX".into()));
        let offline = look_up(&feeds, &guess("ZQOFF").unwrap()).unwrap_err();
        assert!(matches!(offline, NotLookedUp::Unanswered(_)), "{offline:?}");

        let none = crate::feeds::Feeds::of(Vec::new());
        assert!(matches!(look_up(&none, &guess("ZQNEW").unwrap()), Err(NotLookedUp::Unanswered(_))));
    }

    /// Once found, kept: one row per symbol, the newest; and a listing that
    /// has since taken the symbol in is the row searched.
    #[test]
    fn a_found_symbol_is_kept_until_a_listing_has_it() {
        let dir = scratch("kept");
        let path = dir.join("found.tsv");
        let feeds = crate::feeds::Feeds::of(vec![Box::new(testing::Newly)]);
        let new = look_up(&feeds, &guess("ZQNEW").unwrap()).unwrap();
        remember_in(&path, &new).unwrap();
        remember_in(&path, &look_up(&feeds, &guess("ZQFND").unwrap()).unwrap()).unwrap();
        remember_in(&path, &new).unwrap();
        let found = std::fs::read_to_string(&path).unwrap();
        assert_eq!(found.lines().filter(|l| l.contains("\tZQNEW\t")).count(), 1, "{found}");
        assert!(!dir.read_dir().unwrap().any(|e| e.unwrap().file_name().to_string_lossy().contains("incoming")));

        let merged = merge(&format!("{LISTINGS}\n{found}"), omacharts_engine::symbols::seed());
        let kept = merged.iter().find(|i| i.symbol == "ZQNEW").expect("found, so searchable");
        assert_eq!(kept.name, "Zq Newly Listed, Inc.");
        assert_eq!(kept.exchange.as_deref(), Some("NasdaqGS"));

        // The day the listings carry it, theirs is the row.
        let listed = LISTINGS.replacen(
            "\nequity\tA\t",
            "\nequity\tZQNEW\tZq Newly Listed\t-\tUSD\t2\t0\t-\tNASDAQ\t7\nequity\tA\t",
            1,
        );
        let merged = merge(&format!("{listed}\n{found}"), omacharts_engine::symbols::seed());
        let rows: Vec<&Instrument> = merged.iter().filter(|i| i.symbol == "ZQNEW").collect();
        assert_eq!(rows.len(), 1, "one row, not two");
        assert_eq!(rows[0].popularity, 7, "the listing's");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A refreshed file swapped in names what the old one did not, and a
    /// symbol found in the meantime survives the swap.
    #[test]
    fn a_swapped_index_keeps_what_was_found() {
        let inventory = Inventory::curated();
        let feeds = crate::feeds::Feeds::of(vec![Box::new(testing::Newly)]);
        inventory.add_found(look_up(&feeds, &guess("ZQNEW").unwrap()).unwrap());
        assert!(inventory.find("ZQNEW", None).is_some(), "searchable at once");
        assert!(!inventory.complete(), "and the curated half is still only that");

        let refreshed = LISTINGS.replacen(
            "\nequity\tA\t",
            "\nequity\tZQSWP\tZq Swapped In\t-\tUSD\t2\t0\t-\tNASDAQ\t5\nequity\tA\t",
            1,
        );
        inventory.replace(SearchIndex::new(merge(&refreshed, omacharts_engine::symbols::seed())));
        assert!(inventory.complete());
        assert_eq!(inventory.find("ZQSWP", None).map(|i| i.name), Some("Zq Swapped In".into()));
        assert!(inventory.find("ZQNEW", None).is_some(), "found before the swap, still there after it");
        assert!(inventory.find("AAPL", None).is_some());
    }

    /// A Taiwan listing is a ticker that is only a number, so it is the
    /// suffix that tells it apart from anything else — and a curated row
    /// that names it in English alone must still answer to its Chinese name.
    #[test]
    fn the_taiwan_half_is_there_in_both_languages() {
        let merged = merge(&generated(LISTINGS), omacharts_engine::symbols::seed());
        let on = |suffix: &str| merged.iter().filter(|i| i.suffix.as_deref() == Some(suffix)).count();
        assert!(on("TW") > 1000, "only {} TWSE listings", on("TW"));
        assert!(on("TWO") > 800, "only {} TPEx listings", on("TWO"));

        let tsmc: Vec<&Instrument> =
            merged.iter().filter(|i| i.symbol == "2330" && i.suffix.as_deref() == Some("TW")).collect();
        assert_eq!(tsmc.len(), 1, "2330.TW is in both files and must appear once");
        assert_eq!(tsmc[0].tier, 1, "with the curated tier");
        assert_eq!(tsmc[0].local_name.as_deref(), Some("台積電"));
        assert_eq!(tsmc[0].exchange.as_deref(), Some("TWSE"), "and the venue from the listing");
        assert_eq!(tsmc[0].popularity, 9);

        let index = SearchIndex::new(merged);
        for query in ["2330", "2330.TW", "tsmc", "台積電", "台積", "積電", "臺積電"] {
            let first = index.search(query, 1).first().and_then(|h| index.get(h.index)).cloned();
            assert_eq!(
                first.map(|i| i.display_symbol()).as_deref(),
                Some("2330.TW"),
                "{query:?} should find TSMC first"
            );
        }
        let gwc = index.search("環球晶", 1).first().and_then(|h| index.get(h.index)).cloned();
        assert_eq!(gwc.map(|i| i.display_symbol()).as_deref(), Some("6488.TWO"), "a TPEx listing");
    }
}
