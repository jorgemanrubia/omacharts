//! What the files in the data folder are for: symbols, and the contracts
//! of futures.
//!
//! Sierra Chart names a file after its symbol, so `ESZ26_FUT_CME.scid` is
//! the December 2026 E-mini S&P, and `EURUSD.dly` is EURUSD's daily bars.
//! The symbol is the code, the security type and the exchange, joined by
//! underscores; a future's code is its root, a month letter and a two-digit
//! year. CME Group futures all say `_CME` here, even ones that trade on
//! NYMEX or COMEX, because that is the symbol Sierra Chart's own data
//! service gives them.
//!
//! A [`Listing`] is what a data folder holds, from a single directory
//! read. The sizes the directory entries already carry say how many
//! records each `.scid` has, so no file is opened to list a thousand of
//! them.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::scid::{HEADER_SIZE, RECORD_SIZE};

/// The futures month codes, January first.
pub const MONTH_CODES: [char; 12] = ['F', 'G', 'H', 'J', 'K', 'M', 'N', 'Q', 'U', 'V', 'X', 'Z'];

/// How far ahead of this year a contract can be listed. A two-digit year
/// more than this far ahead is taken to be last century: `ESZ99` is 1999,
/// while `CLZ45` is 2045. CME lists crude about ten years out, nothing
/// further, so twenty leaves room without mistaking real history.
pub const YEARS_AHEAD: i32 = 20;

/// A futures contract: its month and year.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Contract {
    /// The full year, 2026 rather than 26. Ordered before the month, so
    /// contracts sort in expiry order.
    pub year: i32,
    /// The month code, `F` for January to `Z` for December.
    pub month: char,
}

impl Contract {
    /// The month, 1 for January.
    pub fn month_number(&self) -> u32 {
        MONTH_CODES.iter().position(|&c| c == self.month).map_or(0, |i| i as u32 + 1)
    }

    /// A number that grows by one each month, for ordering contracts and
    /// counting the months between them.
    pub fn months(&self) -> i32 {
        self.year * 12 + self.month_number() as i32 - 1
    }
}

/// A symbol, as a file in the data folder is named.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct FileSymbol {
    /// The whole name without its extension: `ESZ26_FUT_CME`.
    pub stem: String,
    /// What contracts of the same product share: `ES`. For anything that
    /// is not a dated future, the whole stem.
    pub root: String,
    /// The contract month, for a dated future.
    pub contract: Option<Contract>,
    /// The security type after the first underscore: `FUT`.
    pub kind: Option<String>,
    /// The exchange after the second: `CME`.
    pub exchange: Option<String>,
}

/// Reads a file name's stem as a symbol, taking a two-digit year to be
/// within [`YEARS_AHEAD`] years ahead of this one. `None` only for an empty
/// stem.
pub fn parse(stem: &str) -> Option<FileSymbol> {
    parse_in(stem, crate::time::current_year())
}

/// [`parse`], as if it were `current_year`.
pub fn parse_in(stem: &str, current_year: i32) -> Option<FileSymbol> {
    if stem.is_empty() {
        return None;
    }
    let mut parts = stem.splitn(3, '_');
    let code = parts.next().unwrap_or_default();
    let kind = parts.next().filter(|s| !s.is_empty()).map(str::to_owned);
    let exchange = parts.next().filter(|s| !s.is_empty()).map(str::to_owned);
    let future = kind.as_deref() == Some("FUT");
    let (root, contract) = match future.then(|| split_contract(code, current_year)).flatten() {
        Some((root, contract)) => (root.to_owned(), Some(contract)),
        None if future => (code.to_owned(), None),
        None => (stem.to_owned(), None),
    };
    Some(FileSymbol { stem: stem.to_owned(), root, contract, kind, exchange })
}

/// `ESZ26` as `ES` and December 2026.
fn split_contract(code: &str, current_year: i32) -> Option<(&str, Contract)> {
    let b = code.as_bytes();
    if b.len() < 4 || !b[b.len() - 2..].iter().all(u8::is_ascii_digit) {
        return None;
    }
    let month = b[b.len() - 3] as char;
    if !MONTH_CODES.contains(&month) {
        return None;
    }
    let yy = i32::from(b[b.len() - 2] - b'0') * 10 + i32::from(b[b.len() - 1] - b'0');
    let mut year = current_year - current_year.rem_euclid(100) + yy;
    if year > current_year + YEARS_AHEAD {
        year -= 100;
    } else if year <= current_year + YEARS_AHEAD - 100 {
        year += 100;
    }
    Some((&code[..b.len() - 3], Contract { year, month }))
}

/// A symbol's intraday file, as the directory listing saw it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Intraday {
    pub path: PathBuf,
    /// Whole records, from the file's size; 0 for a header-only file.
    pub records: u64,
    /// The file's size in bytes.
    pub bytes: u64,
    pub modified: Option<SystemTime>,
}

/// One symbol in a data folder, with whichever of its files are there.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub symbol: FileSymbol,
    /// Its `.scid`, if it has one.
    pub intraday: Option<Intraday>,
    /// Its `.dly`, if it has one.
    pub daily: Option<PathBuf>,
}

impl Entry {
    /// Whether the intraday file holds any records.
    pub fn has_intraday_records(&self) -> bool {
        self.intraday.as_ref().is_some_and(|i| i.records > 0)
    }
}

/// Every symbol in a data folder, sorted by root and then by contract,
/// oldest first.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Listing {
    pub data: PathBuf,
    entries: Vec<Entry>,
}

fn order(e: &Entry) -> (&str, Option<Contract>, &str) {
    (&e.symbol.root, e.symbol.contract, &e.symbol.stem)
}

impl Listing {
    /// Lists a data folder with one directory read. Files that are not a
    /// `.scid` or a `.dly` are left out, and so is anything unreadable: a
    /// folder that cannot be read lists as empty.
    pub fn read(data: &Path) -> Listing {
        Self::read_in(data, crate::time::current_year())
    }

    pub(crate) fn read_in(data: &Path, current_year: i32) -> Listing {
        let mut entries: Vec<Entry> = Vec::new();
        let mut by_stem: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
        for dirent in std::fs::read_dir(data).into_iter().flatten().flatten() {
            let name = dirent.file_name();
            let Some(name) = name.to_str() else { continue };
            let Some((stem, ext)) = name.rsplit_once('.') else { continue };
            let intraday = ext.eq_ignore_ascii_case("scid");
            if !intraday && !ext.eq_ignore_ascii_case("dly") {
                continue;
            }
            let index = match by_stem.get(stem) {
                Some(&i) => i,
                None => {
                    let Some(symbol) = parse_in(stem, current_year) else { continue };
                    entries.push(Entry { symbol, intraday: None, daily: None });
                    by_stem.insert(stem.to_owned(), entries.len() - 1);
                    entries.len() - 1
                }
            };
            let path = dirent.path();
            if intraday {
                let meta = dirent.metadata().ok();
                let bytes = meta.as_ref().map_or(0, |m| m.len());
                let records = bytes.saturating_sub(HEADER_SIZE as u64) / RECORD_SIZE as u64;
                let modified = meta.and_then(|m| m.modified().ok());
                entries[index].intraday = Some(Intraday { path, records, bytes, modified });
            } else {
                entries[index].daily = Some(path);
            }
        }
        entries.sort_by(|a, b| order(a).cmp(&order(b)));
        Listing { data: data.to_path_buf(), entries }
    }

    /// Every symbol.
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// Every root, each once, in order.
    pub fn roots(&self) -> impl Iterator<Item = &str> {
        let mut last: Option<&str> = None;
        self.entries.iter().filter_map(move |e| {
            let root = e.symbol.root.as_str();
            (last != Some(root)).then(|| {
                last = Some(root);
                root
            })
        })
    }

    /// Everything under a root: its contracts oldest first, and anything
    /// that is not a dated contract (a continuous symbol) last.
    pub fn of_root<'a>(&'a self, root: &str) -> &'a [Entry] {
        let lo = self.entries.partition_point(|e| e.symbol.root.as_str() < root);
        let hi = self.entries.partition_point(|e| e.symbol.root.as_str() <= root);
        &self.entries[lo..hi]
    }

    /// A root's dated contracts, oldest first.
    pub fn contracts_of<'a>(&'a self, root: &str) -> impl Iterator<Item = &'a Entry> + 'a {
        self.of_root(root).iter().filter(|e| e.symbol.contract.is_some())
    }

    /// The symbol with this stem, if the folder has it.
    pub fn get(&self, stem: &str) -> Option<&Entry> {
        let root = parse(stem)?.root;
        self.of_root(&root).iter().find(|e| e.symbol.stem == stem)
    }

    /// How many symbols there are.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether there are none.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::*;

    #[test]
    fn a_future_is_its_root_month_and_year() {
        let s = parse_in("ESZ26_FUT_CME", 2026).unwrap();
        assert_eq!(s.root, "ES");
        assert_eq!(s.contract, Some(Contract { year: 2026, month: 'Z' }));
        assert_eq!(s.kind.as_deref(), Some("FUT"));
        assert_eq!(s.exchange.as_deref(), Some("CME"));
        assert_eq!(s.contract.unwrap().month_number(), 12);
        let cl = parse_in("CLF12_FUT_CME", 2026).unwrap();
        assert_eq!((cl.root.as_str(), cl.contract.unwrap().year, cl.contract.unwrap().month_number()), ("CL", 2012, 1));
        assert_eq!(parse_in("MCLZ26_FUT_CME", 2026).unwrap().root, "MCL");
    }

    #[test]
    fn a_two_digit_year_is_the_nearest_one_that_could_be_listed() {
        let year = |stem| parse_in(stem, 2026).unwrap().contract.unwrap().year;
        assert_eq!(year("ESZ99_FUT_CME"), 1999);
        assert_eq!(year("ESH90_FUT_CME"), 1990);
        assert_eq!(year("ESH67_FUT_CME"), 1967);
        assert_eq!(year("ESH00_FUT_CME"), 2000);
        assert_eq!(year("CLZ35_FUT_CME"), 2035);
        assert_eq!(year("CLZ46_FUT_CME"), 2046);
        assert_eq!(year("CLZ47_FUT_CME"), 1947);
        assert_eq!(parse_in("ESZ99_FUT_CME", 2085).unwrap().contract.unwrap().year, 2099);
        assert_eq!(parse_in("ESZ05_FUT_CME", 2099).unwrap().contract.unwrap().year, 2105);
    }

    #[test]
    fn anything_that_is_not_a_dated_future_is_its_whole_name() {
        let fx = parse_in("EURUSD", 2026).unwrap();
        assert_eq!((fx.root.as_str(), fx.contract, fx.kind), ("EURUSD", None, None));
        let stock = parse_in("AAPL_STK_NASDAQ", 2026).unwrap();
        assert_eq!((stock.root.as_str(), stock.kind.as_deref()), ("AAPL_STK_NASDAQ", Some("STK")));
        let continuous = parse_in("ES_FUT_CME", 2026).unwrap();
        assert_eq!((continuous.root.as_str(), continuous.contract), ("ES", None));
        assert_eq!(parse_in("ESA26_FUT_CME", 2026).unwrap().contract, None);
        assert!(parse_in("", 2026).is_none());
    }

    #[test]
    fn a_listing_pairs_each_symbols_files_and_counts_records_from_sizes() {
        let dir = Scratch::new();
        scid_file(&dir.join("ESZ26_FUT_CME.scid"), &[trade(1, 1.0, 1, true), trade(2, 1.0, 1, true)]);
        std::fs::write(dir.join("ESZ26_FUT_CME.dly"), "").unwrap();
        scid_file(&dir.join("ESH99_FUT_CME.scid"), &[]);
        std::fs::write(dir.join("ESU26_FUT_CME.dly"), "").unwrap();
        std::fs::write(dir.join("CLF12_FUT_CME.dly"), "").unwrap();
        std::fs::write(dir.join("EURUSD.dly"), "").unwrap();
        std::fs::write(dir.join("ES.cht"), "").unwrap();
        std::fs::create_dir(dir.join("MarketDepthData")).unwrap();

        let listing = Listing::read_in(dir.path(), 2026);
        assert_eq!(listing.len(), 5);
        assert_eq!(listing.roots().collect::<Vec<_>>(), ["CL", "ES", "EURUSD"]);
        let es: Vec<&str> = listing.contracts_of("ES").map(|e| e.symbol.stem.as_str()).collect();
        assert_eq!(es, ["ESH99_FUT_CME", "ESU26_FUT_CME", "ESZ26_FUT_CME"]);
        let z = listing.get("ESZ26_FUT_CME").unwrap();
        assert_eq!(z.intraday.as_ref().unwrap().records, 2);
        assert!(z.daily.is_some() && z.has_intraday_records());
        assert_eq!(listing.get("ESH99_FUT_CME").unwrap().intraday.as_ref().unwrap().records, 0);
        assert!(listing.get("ESU26_FUT_CME").unwrap().intraday.is_none());
        assert!(listing.get("NQZ26_FUT_CME").is_none());
    }
}
