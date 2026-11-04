//! The instrument inventory and the search over it.
//!
//! Search never touches a database. The whole inventory is loaded once into a
//! flat vector with lowercased keys and first-character buckets; a query scans
//! a few hundred candidates, not the whole universe. The budget is under a
//! millisecond so that search-as-you-type is always safe.
//!
//! Ranking matters more than the index. Typing `GC` must surface gold futures,
//! not some microcap that happens to share the letters — which is what `tier`
//! is for.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InstrumentKind {
    Index,
    FutureRoot,
    Equity,
    Etf,
    Fx,
    Crypto,
}

impl InstrumentKind {
    pub fn label(self) -> &'static str {
        match self {
            InstrumentKind::Index => "Index",
            InstrumentKind::FutureRoot => "Futures",
            InstrumentKind::Equity => "Stock",
            InstrumentKind::Etf => "ETF",
            InstrumentKind::Fx => "FX",
            InstrumentKind::Crypto => "Crypto",
        }
    }

    /// Does this trade around the clock?
    ///
    /// A market that never closes has no auction open — today's open is
    /// yesterday's close — which matters anywhere a bar's open is treated as
    /// a separate price rather than a continuation.
    pub fn is_continuous(self) -> bool {
        matches!(self, InstrumentKind::Fx | InstrumentKind::Crypto)
    }

    /// Whether a day nobody traded still gets a price of its own: a future's
    /// settlement, set by the exchange whether or not a contract changed
    /// hands. Such a bar is a real price however far it moved, so it is
    /// never taken for a print that never traded.
    pub fn settles_untraded(self) -> bool {
        matches!(self, InstrumentKind::FutureRoot)
    }

    pub fn from_key(key: &str) -> Option<InstrumentKind> {
        Some(match key {
            "index" => InstrumentKind::Index,
            "future_root" => InstrumentKind::FutureRoot,
            "equity" => InstrumentKind::Equity,
            "etf" => InstrumentKind::Etf,
            "fx" => InstrumentKind::Fx,
            "crypto" => InstrumentKind::Crypto,
            _ => return None,
        })
    }
}

/// One tradeable thing, in nobody's symbology in particular.
///
/// `symbol` is canonical and neutral: `GSPC`, not `^GSPC`; `ES`, not `ES=F`.
/// Turning it into a provider's spelling is the provider's job.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Instrument {
    pub symbol: String,
    pub name: String,
    pub kind: InstrumentKind,
    /// Yahoo-style exchange suffix for non-US listings: `L`, `DE`, `MC`…
    pub suffix: Option<String>,
    pub currency: Option<String>,
    /// 0 is a curated major, 3 is excluded from search.
    pub tier: u8,
    /// Seconds past midnight UTC that this instrument's session opens. Drives
    /// session resets, and folding for a market whose hours are not known
    /// (see [`crate::bars::Grid`]); 0 for anything that trades a normal day.
    pub session_origin: i64,
    /// Per-provider spellings that no template produces.
    pub overrides: Vec<(String, String)>,
    /// Where it trades, when the source knew. Unset for anything whose venue
    /// can be worked out from its suffix or its kind.
    pub exchange: Option<String>,
    /// How well known this is among things of its own sort, 1 (obscure) to 9
    /// (household name); 0 when nothing ranked it. Curated rows leave it at 0
    /// and win on `tier` instead.
    pub popularity: u8,
    /// The name it goes by where it trades, when that is not the English one:
    /// 台積電 for TSMC. Searched alongside `name` and shown beside it, so the
    /// same row answers whichever language somebody types in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local_name: Option<String>,
}

/// Yahoo-style venue suffixes this inventory knows, and the venue each names.
///
/// Also what decides whether `2330.TW` is a ticker and a suffix or one ticker
/// with a dot in it: `BRK.B` is Berkshire's B shares, not a listing on an
/// exchange called B.
const VENUES: &[(&str, &str)] = &[
    ("MC", "BME"),
    ("L", "LSE"),
    ("DE", "XETRA"),
    ("PA", "Euronext"),
    ("SW", "SIX"),
    ("T", "TSE"),
    ("TO", "TSX"),
    ("AS", "Euronext"),
    ("MI", "Borsa Italiana"),
    ("HK", "HKEX"),
    ("TW", "TWSE"),
    ("TWO", "TPEx"),
];

/// Split `2330.TW` into its ticker and its venue suffix.
///
/// Only when what follows the last dot is a suffix some venue uses; anything
/// else comes back whole, so `BRK.B` stays one US ticker. Case is kept as
/// given, and the caller upper-cases if it wants to.
pub fn split_suffix(text: &str) -> (&str, Option<&str>) {
    match text.rsplit_once('.') {
        Some((symbol, suffix))
            if !symbol.is_empty() && VENUES.iter().any(|(s, _)| s.eq_ignore_ascii_case(suffix)) =>
        {
            (symbol, Some(suffix))
        }
        _ => (text, None),
    }
}

/// Lower-cased, and with the two spellings of the same character folded into
/// one, so a query matches however either side happened to write it.
///
/// 臺 and 台 are the one pair that matters in practice: official names write
/// 臺灣 and everybody types 台灣, and a search for 台 that missed 臺灣水泥
/// would be wrong in a way nobody could see the reason for.
fn fold(text: &str) -> String {
    let lower = text.to_lowercase();
    match lower.contains('臺') {
        true => lower.replace('臺', "台"),
        false => lower,
    }
}

/// Is this a character from a script written without spaces between words?
///
/// Those names cannot be split into words to index, so every character is
/// indexed instead — which is what lets 積電 find 台積電.
fn unspaced(c: char) -> bool {
    matches!(c as u32,
        0x3040..=0x30FF     // kana
        | 0x3400..=0x4DBF   // CJK extension A
        | 0x4E00..=0x9FFF   // CJK unified ideographs
        | 0xF900..=0xFAFF   // CJK compatibility ideographs
        | 0xAC00..=0xD7AF)  // hangul
}

impl Instrument {
    pub fn override_for(&self, adapter: &str) -> Option<&str> {
        self.overrides
            .iter()
            .find(|(a, _)| a == adapter)
            .map(|(_, s)| s.as_str())
    }

    /// The size of one tick of its price, which every price it trades at is
    /// a whole number of: the contract's (see [`crate::specs`]), for a
    /// futures root. `None` for anything else, and for a root whose tick is
    /// not known — a stock's tick depends on its price and its venue, and is
    /// no one number.
    pub fn tick_size(&self) -> Option<f64> {
        match self.kind {
            InstrumentKind::FutureRoot => crate::specs::tick(self.root()),
            _ => None,
        }
    }

    /// Its contract specification, for a futures root whose tick is known.
    pub fn spec(&self) -> Option<crate::specs::Spec> {
        match self.kind {
            InstrumentKind::FutureRoot => crate::specs::spec(self.root()),
            _ => None,
        }
    }

    /// The futures root this is, or is a contract of: `ES` for `ES` and for
    /// `ESH7`. The symbol itself for anything that is not a future.
    pub fn root(&self) -> &str {
        match self.kind {
            InstrumentKind::FutureRoot => crate::specs::root_of(&self.symbol),
            _ => &self.symbol,
        }
    }

    /// The month this is a contract of, for a dated future: `ESH7` is March
    /// 2027. `None` for a root, which is the contract trading now, whichever
    /// that is.
    pub fn contract(&self) -> Option<crate::contracts::Month> {
        match self.kind {
            InstrumentKind::FutureRoot => crate::contracts::parse(&self.symbol, crate::contracts::today()).map(|(_, month)| month),
            _ => None,
        }
    }

    /// Is this a dated contract rather than a continuous root?
    pub fn is_contract(&self) -> bool {
        self.kind == InstrumentKind::FutureRoot && crate::specs::root_of(&self.symbol) != self.symbol
    }

    /// This root's contract of `month`: the same instrument, named `ESH7`
    /// rather than `ES`. Only a futures root has one.
    pub fn in_month(&self, month: crate::contracts::Month) -> Option<Instrument> {
        (self.kind == InstrumentKind::FutureRoot)
            .then(|| Instrument { symbol: crate::contracts::symbol(self.root(), month), ..self.clone() })
    }

    /// The continuous root of a dated contract: `ES` for `ESH7`. Itself for
    /// anything else.
    pub fn as_root(&self) -> Instrument {
        Instrument { symbol: self.root().to_string(), ..self.clone() }
    }

    /// What the UI shows as the instrument's ticker.
    /// Where it trades, in the form people say out loud.
    ///
    /// Taken from the listing when the source gave one, and otherwise worked
    /// out: a Yahoo suffix names a venue, and everything with no suffix and no
    /// listing behind it is one of the handful of kinds that has an obvious
    /// home.
    pub fn exchange_label(&self) -> Option<&str> {
        if let Some(named) = self.exchange.as_deref() {
            return Some(named);
        }
        if let Some(suffix) = self.suffix.as_deref() {
            return Some(
                VENUES.iter().find(|(s, _)| *s == suffix).map(|(_, venue)| *venue).unwrap_or(suffix),
            );
        }
        match self.kind {
            InstrumentKind::Fx => Some("FX"),
            InstrumentKind::Crypto => Some("Crypto"),
            _ => None,
        }
    }

    /// The name, and the local one beside it when there is one: `TSMC · 台積電`.
    /// A dated contract's month goes beside its root's name: `E-mini S&P 500
    /// · Mar 2027`.
    pub fn full_name(&self) -> String {
        match (self.local_name.as_deref(), self.contract()) {
            (Some(local), _) => format!("{} · {local}", self.name),
            (None, Some(month)) => format!("{} · {}", self.name, month.label()),
            (None, None) => self.name.clone(),
        }
    }

    pub fn display_symbol(&self) -> String {
        match &self.suffix {
            Some(s) => format!("{}.{}", self.symbol, s),
            None => self.symbol.clone(),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SearchHit {
    pub index: usize,
    pub score: i32,
}

/// Lowercased keys and first-character buckets over an instrument list.
pub struct SearchIndex {
    items: Vec<Instrument>,
    symbol_lc: Vec<String>,
    name_lc: Vec<String>,
    /// The local name, folded; empty for the rows that have none.
    local_lc: Vec<String>,
    /// First character of the symbol, and of every word of the name, to the
    /// items it could match.
    buckets: HashMap<char, Vec<u32>>,
}

impl SearchIndex {
    pub fn new(items: Vec<Instrument>) -> SearchIndex {
        let mut symbol_lc = Vec::with_capacity(items.len());
        let mut name_lc = Vec::with_capacity(items.len());
        let mut local_lc = Vec::with_capacity(items.len());
        let mut buckets: HashMap<char, Vec<u32>> = HashMap::new();

        for (i, item) in items.iter().enumerate() {
            let sym = item.symbol.to_lowercase();
            let name = fold(&item.name);
            let local = item.local_name.as_deref().map(fold).unwrap_or_default();

            let mut firsts: Vec<char> = Vec::new();
            if let Some(c) = sym.chars().next() {
                firsts.push(c);
            }
            for word in name.split(|c: char| !c.is_alphanumeric()) {
                if let Some(c) = word.chars().next() {
                    firsts.push(c);
                }
            }
            firsts.extend(local.chars().next());
            firsts.extend(local.chars().filter(|c| unspaced(*c)));
            firsts.sort_unstable();
            firsts.dedup();
            for c in firsts {
                buckets.entry(c).or_default().push(i as u32);
            }

            symbol_lc.push(sym);
            name_lc.push(name);
            local_lc.push(local);
        }

        SearchIndex { items, symbol_lc, name_lc, local_lc, buckets }
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn get(&self, index: usize) -> Option<&Instrument> {
        self.items.get(index)
    }

    pub fn items(&self) -> &[Instrument] {
        &self.items
    }

    /// Exact lookup by canonical symbol and suffix.
    ///
    /// The watchlist stores what the user picked, not an index position, so it
    /// survives the inventory growing or being regenerated.
    pub fn find(&self, symbol: &str, suffix: Option<&str>) -> Option<&Instrument> {
        self.items
            .iter()
            .find(|i| i.symbol == symbol && i.suffix.as_deref() == suffix)
    }

    /// [`SearchIndex::find`], and failing that a dated contract of a futures
    /// root it has: `ESH7` is March 2027 of `ES`, whether or not anybody
    /// wrote it down. Only a contract spelled the exchange's way, which is
    /// how it was stored; what somebody types goes through [`crate::contracts::parse`]
    /// first.
    pub fn resolve(&self, symbol: &str, suffix: Option<&str>) -> Option<Instrument> {
        if let Some(found) = self.find(symbol, suffix) {
            return Some(found.clone());
        }
        if suffix.is_some() {
            return None;
        }
        let (root, month) = crate::contracts::parse(symbol, crate::contracts::today())?;
        if crate::contracts::symbol(&root, month) != symbol {
            return None;
        }
        self.find(&root, None).filter(|i| i.kind == InstrumentKind::FutureRoot)?.in_month(month)
    }

    /// The curated majors, for an empty search field.
    pub fn featured(&self, limit: usize) -> Vec<SearchHit> {
        let mut hits: Vec<SearchHit> = self
            .items
            .iter()
            .enumerate()
            .filter(|(_, i)| i.tier == 0)
            .map(|(index, _)| SearchHit { index, score: 0 })
            .collect();
        hits.truncate(limit);
        hits
    }

    /// Best matches for `query`, best first.
    ///
    /// A query spelled with its venue, `2330.tw`, also searches for the
    /// ticker alone among the listings on that venue. Also, not instead: a
    /// few canonical symbols have a dot of their own — `FTSEMIB.MI` is an
    /// index, not a Milan listing of something called FTSEMIB — and they
    /// still have to find themselves.
    pub fn search(&self, query: &str, limit: usize) -> Vec<SearchHit> {
        let q = fold(query.trim());
        if q.is_empty() {
            return self.featured(limit);
        }
        let mut hits = self.matching(&q, None);
        if let (symbol, Some(suffix)) = split_suffix(&q) {
            for hit in self.matching(symbol, Some(&suffix.to_uppercase())) {
                if !hits.iter().any(|h| h.index == hit.index) {
                    hits.push(hit);
                }
            }
        }
        hits.sort_by(|a, b| {
            b.score
                .cmp(&a.score)
                .then_with(|| self.symbol_lc[a.index].len().cmp(&self.symbol_lc[b.index].len()))
                .then_with(|| self.symbol_lc[a.index].cmp(&self.symbol_lc[b.index]))
        });
        hits.truncate(limit);
        hits
    }

    /// Every row `q` matches, unsorted, optionally only those on one venue.
    fn matching(&self, q: &str, venue: Option<&str>) -> Vec<SearchHit> {
        let Some(first) = q.chars().next() else {
            return Vec::new();
        };
        let Some(candidates) = self.buckets.get(&first) else {
            return Vec::new();
        };
        let mut hits: Vec<SearchHit> = Vec::new();
        for &i in candidates {
            let i = i as usize;
            if self.items[i].tier >= 3 {
                continue;
            }
            if venue.is_some() && self.items[i].suffix.as_deref() != venue {
                continue;
            }
            if let Some(score) = self.score(i, q) {
                hits.push(SearchHit { index: i, score });
            }
        }
        hits
    }

    /// `None` when the query does not match at all.
    ///
    /// The tiers are what make this feel right: an exact ticker always wins,
    /// but between two equally good textual matches the one people actually
    /// meant — an index, a front-month future, a mega-cap — comes first.
    ///
    /// A name that *is* the query counts as exact too. People type `DAX`
    /// meaning the index, whose canonical symbol is `GDAXI`; without this the
    /// NASDAQ ETF that happens to own the ticker would win on spelling alone.
    fn score(&self, i: usize, q: &str) -> Option<i32> {
        let symbol = &self.symbol_lc[i];
        let name = &self.name_lc[i];
        let item = &self.items[i];

        // Two ladders rather than one, because a ticker and a name are not
        // the same kind of evidence and the weights below treat them
        // differently. The rungs are the same numbers either way, so taking
        // the better of the two is the chain of `else if` this used to be.
        let symbolic = if symbol == q {
            Some(EXACT)
        } else if symbol.starts_with(q) {
            // A longer ticker is a worse answer: `aa` means Alcoa before it
            // means AAON.
            Some(700 - (symbol.len() - q.len()).min(20) as i32 * 4)
        } else if symbol.contains(q) {
            Some(320)
        } else {
            None
        };

        // The local name is the same kind of evidence as the English one, so
        // it climbs the same ladder and the better of the two counts.
        let nominal = name_band(name, q).max(name_band(&self.local_lc[i], q));

        let tier_weight = match item.tier {
            0 => 120,
            1 => 60,
            _ => 0,
        };
        // Indexes and futures are what this app is for; nudge them up when the
        // textual match is otherwise a tie.
        let kind_weight = match item.kind {
            InstrumentKind::Index | InstrumentKind::FutureRoot => 30,
            InstrumentKind::Etf => 12,
            _ => 0,
        };
        // Six a band, so fame tops out at 54, and the ceiling is what matters:
        // it sits just under the 60 a tier-1 row carries, so the most famous
        // listing a feed can name still loses to the least of the instruments
        // someone chose to curate — searching `bank` finds Bank of America,
        // not whichever bank ETF traded most yesterday. Well clear of the
        // 4-a-character penalty a longer ticker pays, though, which is the
        // whole point: `aa` should find Alcoa before AAON. It reorders a band
        // from within; it never promotes one match over a better one.
        let fame = item.popularity as i32 * 6;
        let prior = tier_weight + kind_weight + fame;

        // A ticker is a label anyone can collide with: eleven thousand of them
        // are four letters or fewer, so `tes` being the start of one is weak
        // evidence and gets the prior once. A name is something you have to
        // know before you can type it, so matching one is evidence twice over
        // — that this is the row you meant, and that you knew its name at all
        // — and the prior counts twice. Without this a test issue called TEST
        // outranked Tesla, and four leveraged ETFs outranked crude oil.
        //
        // It cannot promote anything over an exact match. The best a partial
        // can reach is 500 + 2 × 204 = 908, against the 1000 an exact scores
        // before a single weight is added, and
        // `an_exact_ticker_outscores_the_best_possible_partial` holds that
        // moat open.
        let by_symbol = symbolic.map(|textual| textual + prior);
        let by_name = nominal.map(|textual| textual + prior * 2);
        by_symbol.max(by_name)
    }
}

/// How well `q` matches one name, on the rungs [`SearchIndex::score`] uses.
fn name_band(name: &str, q: &str) -> Option<i32> {
    if name.is_empty() {
        None
    } else if name == q {
        Some(EXACT)
    } else if name.split(|c: char| !c.is_alphanumeric()).any(|w| w == q) {
        Some(500)
    } else if name.starts_with(q) {
        Some(450)
    } else if name
        .split(|c: char| !c.is_alphanumeric())
        .any(|w| w.starts_with(q))
    {
        Some(300)
    } else if name.contains(q) {
        Some(180)
    } else {
        None
    }
}

/// What a query that *is* the ticker, or *is* the name, scores before weights.
///
/// Far enough above every partial band that no combination of tier, kind and
/// fame can reach it from below: typing GC gives gold and typing TSLA gives
/// Tesla, whatever else happens to share the letters.
const EXACT: i32 = 1000;

/// The most any row can add to its textual band: tier 0, an index or a future,
/// and a household name. Only a bound, and only the test that pins it uses it.
#[cfg(test)]
const MAX_PRIOR: i32 = 120 + 30 + 9 * 6;

/// The curated inventory shipped in the binary.
///
/// Enough to use the app the moment it is installed, and the thing search is
/// tuned against. The generated database widens this to every US listing
/// without changing anything here.
pub fn seed() -> Vec<Instrument> {
    parse_seed(include_str!("seed.tsv"))
}

/// `kind<TAB>symbol<TAB>name<TAB>suffix<TAB>currency<TAB>tier<TAB>session_origin<TAB>yahoo_override<TAB>exchange<TAB>popularity<TAB>local_name`
///
/// Everything past `tier` is optional, so a file written before a column
/// existed still parses — which is what lets the generated half gain columns
/// without the curated half being rewritten to match.
pub fn parse_seed(text: &str) -> Vec<Instrument> {
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim_end();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let f: Vec<&str> = line.split('\t').collect();
        if f.len() < 6 {
            continue;
        }
        let Some(kind) = InstrumentKind::from_key(f[0]) else {
            continue;
        };
        let blank = |s: &str| if s.is_empty() || s == "-" { None } else { Some(s.to_string()) };
        out.push(Instrument {
            kind,
            symbol: f[1].to_string(),
            name: f[2].to_string(),
            suffix: blank(f[3]),
            currency: blank(f[4]),
            tier: f[5].parse().unwrap_or(2),
            session_origin: f.get(6).and_then(|v| v.parse().ok()).unwrap_or(0),
            overrides: match f.get(7).copied().and_then(blank) {
                Some(sym) => vec![("yahoo".to_string(), sym)],
                None => Vec::new(),
            },
            exchange: f.get(8).copied().and_then(blank),
            popularity: f.get(9).and_then(|v| v.parse().ok()).unwrap_or(0),
            local_name: f.get(10).copied().and_then(blank),
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn index() -> SearchIndex {
        SearchIndex::new(seed())
    }

    /// A generated-half row: tier 2, no overrides, as popular as you say.
    fn listed(symbol: &str, name: &str, popularity: u8) -> Instrument {
        Instrument {
            symbol: symbol.into(),
            name: name.into(),
            kind: InstrumentKind::Equity,
            suffix: None,
            currency: Some("USD".into()),
            tier: 2,
            session_origin: 0,
            overrides: Vec::new(),
            exchange: Some("NASDAQ".into()),
            popularity,
            local_name: None,
        }
    }

    /// The same, as a fund — which carries a kind weight a share does not, so
    /// it is the harder thing to outrank.
    fn fund(row: Instrument) -> Instrument {
        Instrument { kind: InstrumentKind::Etf, ..row }
    }

    #[test]
    fn the_seed_parses() {
        let items = seed();
        assert!(items.len() > 150, "seed looks short: {}", items.len());
        assert!(items.iter().any(|i| i.symbol == "GSPC"));
        assert!(items.iter().any(|i| i.symbol == "ES"));
    }

    #[test]
    fn gc_finds_gold_futures_first() {
        let idx = index();
        let hits = idx.search("gc", 5);
        let first = idx.get(hits[0].index).unwrap();
        assert_eq!(first.symbol, "GC", "got {first:?}");
        assert_eq!(first.kind, InstrumentKind::FutureRoot);
    }

    #[test]
    fn an_exact_ticker_wins() {
        let idx = index();
        for ticker in ["aapl", "es", "spy", "vix"] {
            let hits = idx.search(ticker, 5);
            assert!(!hits.is_empty(), "no hits for {ticker}");
            assert_eq!(
                idx.get(hits[0].index).unwrap().symbol.to_lowercase(),
                ticker,
                "wrong winner for {ticker}"
            );
        }
    }

    #[test]
    fn a_famous_name_beats_a_ticker_squatter() {
        // `DAX` is the index's name, not its symbol — and a NASDAQ ETF owns
        // the literal ticker. The index is what people mean.
        let mut items = seed();
        items.push(Instrument {
            symbol: "DAX".into(),
            name: "Global X DAX Germany ETF".into(),
            kind: InstrumentKind::Etf,
            suffix: None,
            currency: Some("USD".into()),
            tier: 2,
            session_origin: 0,
            overrides: Vec::new(),
            exchange: Some("NASDAQ".into()),
            popularity: 0,
            local_name: None,
        });
        let idx = SearchIndex::new(items);
        assert_eq!(idx.get(idx.search("dax", 5)[0].index).unwrap().symbol, "GDAXI");
    }

    /// A listing whose ticker happens to start with your letters, against the
    /// company whose *name* does. Nobody typing `tes` wants a structured
    /// product called TEST; they want Tesla, and the only thing separating the
    /// two is that one of them is a household name.
    #[test]
    fn a_famous_name_beats_a_ticker_that_merely_shares_the_letters() {
        let mut items = seed();
        // The curated file carries no ranking; a curated row picks one up from
        // the listing it displaces when the two halves are merged, which is
        // the state search actually runs against.
        for row in &mut items {
            if row.symbol == "TSLA" {
                row.popularity = 9;
            }
        }
        items.push(fund(listed("TEST", "YieldMax TSLA Performance & Distribution ETF", 3)));
        items.push(fund(listed("TESL", "Simplify Volt TSLA Revolution ETF", 1)));
        let idx = SearchIndex::new(items);
        assert_eq!(idx.get(idx.search("tes", 5)[0].index).unwrap().symbol, "TSLA");
    }

    /// The same thing one rung further down: `oil` is a whole word of the
    /// futures root's name, and four leveraged ETFs own tickers beginning
    /// OIL. A curated front-month contract is what the app is for.
    #[test]
    fn a_curated_name_beats_the_tickers_that_squat_on_the_word() {
        let mut items = seed();
        for (symbol, name) in [
            ("OILK", "ProShares K-1 Free Crude Oil ETF"),
            ("OILU", "MicroSectors Oil & Gas Exp. & Prod. 3x ETN"),
            ("OILD", "MicroSectors Oil & Gas Exp. & Prod. -3x ETN"),
            ("OILT", "Texas Capital Texas Oil Index ETF"),
        ] {
            items.push(fund(listed(symbol, name, 4)));
        }
        let idx = SearchIndex::new(items);
        assert_eq!(idx.get(idx.search("oil", 5)[0].index).unwrap().symbol, "CL");
    }

    /// The floor the whole search stands on: typing a ticker in full gives you
    /// that ticker. Promoting name matches is only safe while no stack of
    /// weights can climb from a partial band to the exact one, so this builds
    /// the worst case on purpose — the dullest possible exact match against a
    /// rival that is curated, an index, a household name, and matches both as
    /// a ticker prefix and as a whole word of its name.
    #[test]
    fn an_exact_ticker_outscores_the_best_possible_partial() {
        let exact = Instrument {
            symbol: "ZZZ".into(),
            name: "Nothing In Particular".into(),
            kind: InstrumentKind::Equity,
            suffix: None,
            currency: None,
            tier: 2,
            session_origin: 0,
            overrides: Vec::new(),
            exchange: None,
            popularity: 0,
            local_name: None,
        };
        let rival = Instrument {
            symbol: "ZZZA".into(),
            name: "Zzz Everything Index".into(),
            kind: InstrumentKind::Index,
            tier: 0,
            popularity: 9,
            ..exact.clone()
        };
        let idx = SearchIndex::new(vec![rival, exact]);
        let hits = idx.search("zzz", 2);
        assert_eq!(idx.get(hits[0].index).unwrap().symbol, "ZZZ");
        assert!(
            hits[0].score - hits[1].score >= EXACT - (500 + 2 * MAX_PRIOR),
            "the moat has narrowed: {} vs {}",
            hits[0].score,
            hits[1].score
        );
    }

    /// Cheap and exhaustive: every instrument anyone curated answers to its own
    /// ticker. One curated row shadowing another is the kind of thing a weight
    /// change causes and nobody notices until they type it.
    #[test]
    fn every_curated_ticker_finds_itself() {
        let idx = index();
        for item in seed() {
            let hits = idx.search(&item.symbol, 1);
            let first = idx.get(hits[0].index).unwrap();
            assert_eq!(first.symbol, item.symbol, "{} found {} first", item.symbol, first.symbol);
        }
    }

    #[test]
    fn fame_settles_a_tie_the_spelling_cannot() {
        // Both are prefix matches of the same length, so every other term in
        // the score is identical and the old ranking fell back to alphabetical
        // order — which put a microcap above the aluminium company.
        let idx = SearchIndex::new(vec![
            listed("AAON", "AAON Inc.", 3),
            listed("AAPX", "Some Obscure Thing", 1),
            listed("AACQ", "Alcoa-ish Corporation", 9),
        ]);
        let hits = idx.search("aa", 5);
        assert_eq!(idx.get(hits[0].index).unwrap().symbol, "AACQ");
        assert_eq!(idx.get(hits[2].index).unwrap().symbol, "AAPX");
    }

    #[test]
    fn fame_never_promotes_a_worse_match() {
        // The whole point of capping fame below the gaps between the textual
        // bands: someone typing a ticker in full gets that ticker, however
        // famous the thing that merely starts with it.
        let idx = SearchIndex::new(vec![
            listed("NVDA", "NVIDIA Corporation", 9),
            listed("NVD", "Nothing Very Dramatic", 1),
        ]);
        assert_eq!(idx.get(idx.search("nvd", 5)[0].index).unwrap().symbol, "NVD");
    }

    #[test]
    fn a_curated_row_outranks_a_famous_generated_one() {
        // 120 for a hand-picked row against 72 for the most famous listing
        // there is. Curation is a stronger signal than any feed, because it is
        // the one signal that knows what this app is for.
        let mut items = seed();
        items.push(listed("SPYY", "Something Else Entirely", 9));
        let idx = SearchIndex::new(items);
        assert_eq!(idx.get(idx.search("spy", 5)[0].index).unwrap().symbol, "SPY");
    }

    #[test]
    fn a_row_from_before_the_column_existed_still_parses() {
        // The curated half has eight columns and is not regenerated when the
        // generated half gains a ninth or a tenth. Both must keep loading.
        let eight = "equity\tOLD\tOld Eight Column\t-\tUSD\t2\t0\t-";
        let ten = "equity\tNEW\tNew Ten Column\t-\tUSD\t2\t0\t-\tNASDAQ\t7";
        let items = parse_seed(&format!("{eight}\n{ten}\n"));
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].popularity, 0, "a missing column is simply unranked");
        assert_eq!(items[0].exchange, None);
        assert_eq!(items[1].popularity, 7);
    }

    #[test]
    fn the_generated_listings_carry_their_ranking() {
        // Guards the seam between tools/build_listings.py and this parser: a
        // column written in the wrong place would read as zero everywhere and
        // quietly cost search its ordering, with nothing else going wrong.
        let items = parse_seed(include_str!("listings.tsv"));
        let ranked = items.iter().filter(|i| i.popularity > 0).count();
        assert!(
            ranked > items.len() / 2,
            "only {ranked} of {} listings are ranked",
            items.len()
        );
        let apple = items.iter().find(|i| i.symbol == "AAPL").expect("no AAPL");
        assert_eq!(apple.popularity, 9, "the largest company should top the scale");
    }

    #[test]
    fn names_are_searchable() {
        let idx = index();
        let hits = idx.search("apple", 5);
        assert_eq!(idx.get(hits[0].index).unwrap().symbol, "AAPL");

        let hits = idx.search("gold", 5);
        assert!(!hits.is_empty());
    }

    #[test]
    fn an_empty_query_shows_the_majors() {
        let idx = index();
        let hits = idx.search("", 10);
        assert_eq!(hits.len(), 10);
        assert!(hits.iter().all(|h| idx.get(h.index).unwrap().tier == 0));
    }

    #[test]
    fn well_known_tickers_beat_obscure_ones() {
        let idx = index();
        // Each of these is typed constantly and must win its prefix outright.
        for (query, expected) in [
            ("gc", "GC"),     // gold futures, not a microcap sharing the letters
            ("es", "ES"),     // E-mini S&P
            ("nq", "NQ"),
            ("cl", "CL"),
            ("sp", "SPY"),    // the ETF people mean when they type "sp"
            ("vi", "VIX"),
            ("bt", "BTC"),
            ("eur", "EURUSD"),
        ] {
            let hits = idx.search(query, 5);
            assert!(!hits.is_empty(), "no hits for {query}");
            assert_eq!(
                idx.get(hits[0].index).unwrap().symbol,
                expected,
                "{query} should surface {expected}, got {:?}",
                hits.iter().map(|h| &idx.get(h.index).unwrap().symbol).collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn full_names_are_searchable_too() {
        let idx = index();
        for (query, expected) in [
            ("gold", "GC"),
            ("nvidia", "NVDA"),
            ("bitcoin", "BTC"),
            ("crude", "CL"),
            ("santander", "SAN"),
            ("nasdaq 100", "NDX"),
            ("volatility", "VIX"),
        ] {
            let hits = idx.search(query, 8);
            assert!(!hits.is_empty(), "no hits for {query}");
            let symbols: Vec<&str> =
                hits.iter().map(|h| idx.get(h.index).unwrap().symbol.as_str()).collect();
            assert!(symbols.contains(&expected), "{query} -> {symbols:?}, wanted {expected}");
        }
    }

    #[test]
    fn a_tier_zero_match_outranks_a_better_textual_tier_two_match() {
        // "micro" prefixes several tier-2 futures; the point is only that
        // ranking never puts an obscure instrument above a curated major when
        // the textual quality is comparable.
        let idx = index();
        let hits = idx.search("s", 10);
        let top: Vec<u8> = hits.iter().take(3).map(|h| idx.get(h.index).unwrap().tier).collect();
        assert!(top.iter().all(|&t| t <= 1), "top of 's' was tiers {top:?}");
    }

    #[test]
    fn search_is_fast_enough_to_run_on_every_keystroke() {
        let idx = index();
        let queries = ["a", "ap", "app", "appl", "g", "gc", "gol", "e", "es", "spy"];
        let start = std::time::Instant::now();
        let rounds = 200;
        for _ in 0..rounds {
            for q in queries {
                let _ = idx.search(q, 20);
            }
        }
        let per_query = start.elapsed() / (rounds * queries.len() as u32);
        // The budget is a millisecond; anything near it means the index
        // regressed into a full scan.
        assert!(per_query < std::time::Duration::from_micros(500), "{per_query:?} per query");
    }

    #[test]
    fn only_the_round_the_clock_markets_are_continuous() {
        assert!(InstrumentKind::Fx.is_continuous());
        assert!(InstrumentKind::Crypto.is_continuous());
        for kind in [
            InstrumentKind::Equity,
            InstrumentKind::Etf,
            InstrumentKind::Index,
            InstrumentKind::FutureRoot,
        ] {
            assert!(!kind.is_continuous(), "{kind:?}");
        }
    }

    #[test]
    fn exact_lookup_distinguishes_listings() {
        let idx = index();
        assert_eq!(idx.find("AAPL", None).unwrap().name, "Apple");
        assert_eq!(idx.find("SAN", Some("MC")).unwrap().name, "Banco Santander");
        // The Madrid listing must not answer a lookup for a US one.
        assert!(idx.find("SAN", None).is_none());
        assert!(idx.find("NOPE", None).is_none());
    }

    #[test]
    fn a_venue_suffix_splits_off_and_a_share_class_does_not() {
        assert_eq!(split_suffix("2330.TW"), ("2330", Some("TW")));
        assert_eq!(split_suffix("6488.two"), ("6488", Some("two")), "case is the caller's");
        assert_eq!(split_suffix("SAP.DE"), ("SAP", Some("DE")));
        assert_eq!(split_suffix("BRK.B"), ("BRK.B", None), "a share class is part of the ticker");
        assert_eq!(split_suffix("2330"), ("2330", None));
        assert_eq!(split_suffix(".TW"), (".TW", None), "a venue with nothing listed on it");
    }

    #[test]
    fn the_local_name_column_parses_and_is_searched() {
        let row = "equity\t2330\tTSMC\tTW\tTWD\t2\t3600\t-\tTWSE\t9\t台積電";
        let items = parse_seed(row);
        assert_eq!(items[0].local_name.as_deref(), Some("台積電"));
        assert_eq!(items[0].full_name(), "TSMC · 台積電");
        assert_eq!(items[0].exchange_label(), Some("TWSE"));

        let idx = SearchIndex::new(items);
        for query in ["台積電", "台積", "積電", "臺積電", "tsmc", "2330", "2330.tw"] {
            assert_eq!(idx.search(query, 1).len(), 1, "{query:?} found nothing");
        }
        assert!(idx.search("2330.de", 1).is_empty(), "the venue has to match");
    }

    #[test]
    fn a_row_without_a_local_name_shows_its_name_alone() {
        let apple = index().find("AAPL", None).unwrap().clone();
        assert_eq!(apple.local_name, None);
        assert_eq!(apple.full_name(), "Apple");
    }

    #[test]
    fn nonsense_matches_nothing() {
        assert!(index().search("zzzzqq", 5).is_empty());
    }

    /// A dated contract is its root's instrument under the contract's name:
    /// the same specification, the same hours, named with its month.
    #[test]
    fn a_dated_contract_resolves_to_its_root_in_that_month() {
        let idx = index();
        let es = idx.find("ES", None).unwrap();
        assert!(!es.is_contract());
        assert_eq!(es.contract(), None);
        assert_eq!(es.root(), "ES");
        let march = idx.resolve("ESH7", None).expect("ESH7");
        assert!(march.is_contract());
        assert_eq!(march.root(), "ES");
        assert_eq!(march.kind, InstrumentKind::FutureRoot);
        assert_eq!(march.name, es.name);
        assert_eq!(march.display_symbol(), "ESH7");
        assert_eq!(march.contract().map(|m| m.label()).as_deref(), Some("Mar 2027"));
        assert_eq!(march.full_name(), "E-mini S&P 500 · Mar 2027");
        assert_eq!(march.tick_size(), es.tick_size());
        assert_eq!(march.as_root(), es.clone());
        assert_eq!(es.in_month(march.contract().unwrap()), Some(march.clone()));
        assert_eq!(idx.resolve("ES", None).as_ref(), Some(es));
        assert!(idx.resolve("ESH27", None).is_none(), "stored the exchange's way, or not at all");
        assert!(idx.resolve("ZZQH7", None).is_none(), "no such root");
        assert!(idx.resolve("AAPLH7", None).is_none(), "a share has no months");
        assert!(idx.resolve("ESH7", Some("MC")).is_none());
        let apple = idx.find("AAPL", None).unwrap();
        assert_eq!(apple.in_month(march.contract().unwrap()), None);
        assert_eq!(apple.root(), "AAPL");
    }
}
