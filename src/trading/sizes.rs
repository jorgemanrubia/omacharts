//! How many an order from a chart is for: its quantity, the lists of
//! sizes a trader picks it from, and which list a chart picks from.
//!
//! Broker-neutral and pure, beside the accounts, orders and positions in
//! [`crate::trading`]: no widget, no broker and no store reads in here
//! beyond loading and saving the lists. The header's Trading size part,
//! the keys that set it and the orders that spend it only ask.
//!
//! A **size template** is a named list of sizes and what it applies to:
//! kinds of instrument (stocks, futures), optionally narrowed to some roots
//! (`ES`, `NQ`). A chart picks from the narrowest template that applies to
//! its instrument, so a template for `MES` alone wins over Futures for an
//! `MES` chart and leaves every other future on Futures.
//!
//! A chart's size is the first in its template until somebody picks
//! another, and sticks once picked. What is stored with the chart is the
//! size and the template it was picked from, so that a chart moved from
//! ES to AAPL does not go on buying two shares: a size picked from another
//! template than the one in force is not the chart's size.

use omacharts_engine::{Instrument, InstrumentKind};
use serde::{Deserialize, Serialize};

use crate::store::Store;

/// The size templates, as a JSON list in the order they are listed.
pub const SIZE_TEMPLATES: &str = "trading_size_templates";

/// How many contracts or shares: never none.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "u32", into = "u32")]
pub struct Quantity(u32);

impl Quantity {
    pub const ONE: Quantity = Quantity(1);

    /// `n`, unless it is zero.
    pub fn new(n: u32) -> Option<Quantity> {
        (n > 0).then_some(Quantity(n))
    }

    pub fn get(self) -> u32 {
        self.0
    }
}

impl TryFrom<u32> for Quantity {
    type Error = String;

    fn try_from(n: u32) -> Result<Quantity, String> {
        Quantity::new(n).ok_or_else(|| "a size is at least 1".to_string())
    }
}

impl From<Quantity> for u32 {
    fn from(quantity: Quantity) -> u32 {
        quantity.0
    }
}

impl std::fmt::Display for Quantity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// The groups of instruments a template can apply to, as a trader names
/// them: an ETF trades in shares like a stock does.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    Stocks,
    Futures,
}

impl Category {
    pub const ALL: [Category; 2] = [Category::Stocks, Category::Futures];

    pub fn label(self) -> &'static str {
        match self {
            Category::Stocks => "Stocks",
            Category::Futures => "Futures",
        }
    }

    /// The kinds of instrument it is.
    pub fn kinds(self) -> &'static [InstrumentKind] {
        match self {
            Category::Stocks => &[InstrumentKind::Equity, InstrumentKind::Etf],
            Category::Futures => &[InstrumentKind::FutureRoot],
        }
    }

    pub fn of(kind: InstrumentKind) -> Option<Category> {
        Category::ALL.into_iter().find(|category| category.kinds().contains(&kind))
    }

    /// What one of it is called, for saying a size: "2 contracts".
    pub fn unit(self, quantity: Quantity) -> &'static str {
        match (self, quantity.get()) {
            (Category::Stocks, 1) => "share",
            (Category::Stocks, _) => "shares",
            (Category::Futures, 1) => "contract",
            (Category::Futures, _) => "contracts",
        }
    }
}

/// What a template applies to: instruments of these categories, and when
/// `roots` names any, only those.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Applies {
    pub categories: Vec<Category>,
    /// Roots, or symbols for stocks, as the app spells them: `ES`, `AAPL`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub roots: Vec<String>,
}

impl Applies {
    /// Does it apply to `instrument`?
    pub fn covers(&self, instrument: &Instrument) -> bool {
        let Some(category) = Category::of(instrument.kind) else { return false };
        self.categories.contains(&category)
            && (self.roots.is_empty() || self.roots.iter().any(|root| root.eq_ignore_ascii_case(&instrument.symbol)))
    }

    /// Narrowed to some roots, so it wins over one that is not.
    pub fn narrow(&self) -> bool {
        !self.roots.is_empty()
    }
}

/// A named list of sizes, and what it applies to.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SizeTemplate {
    pub name: String,
    pub sizes: Vec<Quantity>,
    pub applies_to: Applies,
}

impl SizeTemplate {
    /// The size a chart on it has until somebody picks another.
    pub fn first(&self) -> Option<Quantity> {
        self.sizes.first().copied()
    }
}

/// The templates there are until somebody edits them: stocks by the
/// hundred, futures one to nine.
pub fn standard() -> Vec<SizeTemplate> {
    let list = |sizes: std::ops::RangeInclusive<u32>, by: u32| {
        sizes.filter_map(|n| Quantity::new(n * by)).collect::<Vec<_>>()
    };
    vec![
        SizeTemplate {
            name: "Stocks".into(),
            sizes: list(1..=9, 100),
            applies_to: Applies { categories: vec![Category::Stocks], roots: Vec::new() },
        },
        SizeTemplate {
            name: "Futures".into(),
            sizes: list(1..=9, 1),
            applies_to: Applies { categories: vec![Category::Futures], roots: Vec::new() },
        },
    ]
}

/// The templates, as listed; the standard ones when nobody has edited
/// them, or when what is stored cannot be read.
pub fn load(store: &Store) -> Vec<SizeTemplate> {
    store
        .setting(SIZE_TEMPLATES)
        .and_then(|json| serde_json::from_str(&json).ok())
        .unwrap_or_else(standard)
}

pub fn save(store: &Store, templates: &[SizeTemplate]) {
    if let Ok(json) = serde_json::to_string(templates) {
        store.set_setting(SIZE_TEMPLATES, &json);
    }
}

/// The template a chart of `instrument` picks its size from: the
/// narrowest that applies, the first listed among as narrow.
pub fn template_for<'a>(templates: &'a [SizeTemplate], instrument: &Instrument) -> Option<&'a SizeTemplate> {
    let applying = || templates.iter().filter(|template| template.applies_to.covers(instrument));
    applying().find(|template| template.applies_to.narrow()).or_else(|| applying().next())
}

/// A size somebody picked for a chart, and the template they picked it
/// from: what is stored with the chart.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Picked {
    pub template: String,
    pub size: Quantity,
}

/// The size an order from a chart of `instrument` is for: the one picked
/// for it while its template is still the one in force, else the first in
/// that template. `None` when no template applies.
pub fn size_for(templates: &[SizeTemplate], instrument: &Instrument, picked: Option<&Picked>) -> Option<Quantity> {
    let template = template_for(templates, instrument)?;
    match picked {
        Some(picked) if picked.template == template.name => Some(picked.size),
        _ => template.first(),
    }
}

/// Entry `n`, counting from 1, of the template a chart of `instrument`
/// picks from, as a pick to store with the chart: what Ctrl and a digit
/// asks for. `None` when there is no such entry.
pub fn pick(templates: &[SizeTemplate], instrument: &Instrument, n: usize) -> Option<Picked> {
    let template = template_for(templates, instrument)?;
    let size = *template.sizes.get(n.checked_sub(1)?)?;
    Some(Picked { template: template.name.clone(), size })
}

/// A size in words, with its unit when the instrument has one: "2
/// contracts", "100 shares".
pub fn size_text(size: Quantity, kind: InstrumentKind) -> String {
    match Category::of(kind) {
        Some(category) => format!("{size} {}", category.unit(size)),
        None => size.to_string(),
    }
}

/// Sizes as typed into a template: `1, 2, 5, 10`, in any order, each
/// once. Refuses anything that is not a whole number above zero rather
/// than dropping it, so what is saved is what was typed.
pub fn parse_sizes(text: &str) -> Result<Vec<Quantity>, String> {
    let mut sizes = Vec::new();
    for word in text.split([',', ' ']).map(str::trim).filter(|word| !word.is_empty()) {
        let size = word
            .parse::<u32>()
            .ok()
            .and_then(Quantity::new)
            .ok_or_else(|| format!("{word} is not a quantity"))?;
        if !sizes.contains(&size) {
            sizes.push(size);
        }
    }
    match sizes.is_empty() {
        true => Err("No quantities".to_string()),
        false => Ok(sizes),
    }
}

/// The other way: what [`parse_sizes`] reads.
pub fn spell_sizes(sizes: &[Quantity]) -> String {
    sizes.iter().map(Quantity::to_string).collect::<Vec<_>>().join(", ")
}

/// Roots as typed: `ES, NQ`, upper-cased, each once.
pub fn parse_roots(text: &str) -> Vec<String> {
    let mut roots: Vec<String> = Vec::new();
    for root in text.split([',', ' ']).map(str::trim).filter(|root| !root.is_empty()) {
        let root = root.to_uppercase();
        if !roots.contains(&root) {
            roots.push(root);
        }
    }
    roots
}

#[cfg(test)]
mod tests {
    use super::*;

    fn instrument(symbol: &str, kind: InstrumentKind) -> Instrument {
        Instrument {
            symbol: symbol.into(),
            name: symbol.into(),
            kind,
            suffix: None,
            currency: None,
            tier: 0,
            session_origin: 0,
            overrides: Vec::new(),
            exchange: None,
            popularity: 0,
            local_name: None,
        }
    }

    fn sizes(list: &[u32]) -> Vec<Quantity> {
        list.iter().filter_map(|n| Quantity::new(*n)).collect()
    }

    #[test]
    fn a_quantity_is_never_none() {
        assert_eq!(Quantity::new(0), None);
        assert_eq!(Quantity::new(3).map(Quantity::get), Some(3));
        assert!(serde_json::from_str::<Quantity>("0").is_err());
        assert_eq!(serde_json::from_str::<Quantity>("5").unwrap(), Quantity::new(5).unwrap());
    }

    /// Out of the box stocks go by the hundred and futures one at a time,
    /// nine of each; an ETF is a stock, and FX has no template.
    #[test]
    fn the_standard_templates_are_stocks_by_the_hundred_and_futures_one_to_nine() {
        let templates = standard();
        let es = instrument("ES", InstrumentKind::FutureRoot);
        let spy = instrument("SPY", InstrumentKind::Etf);
        let aapl = instrument("AAPL", InstrumentKind::Equity);
        assert_eq!(template_for(&templates, &es).unwrap().sizes, sizes(&[1, 2, 3, 4, 5, 6, 7, 8, 9]));
        assert_eq!(
            template_for(&templates, &aapl).unwrap().sizes,
            sizes(&[100, 200, 300, 400, 500, 600, 700, 800, 900])
        );
        assert_eq!(template_for(&templates, &spy).unwrap().name, "Stocks");
        assert_eq!(size_for(&templates, &es, None), Quantity::new(1));
        assert_eq!(size_for(&templates, &aapl, None), Quantity::new(100));
        assert_eq!(template_for(&templates, &instrument("EURUSD", InstrumentKind::Fx)), None);
        assert_eq!(size_for(&templates, &instrument("EURUSD", InstrumentKind::Fx), None), None);
    }

    /// A template narrowed to some roots wins for those, wherever it is
    /// listed, and leaves the rest to the broad one.
    #[test]
    fn the_narrower_template_wins() {
        let mut templates = standard();
        templates.push(SizeTemplate {
            name: "Micros".into(),
            sizes: sizes(&[5, 10, 20]),
            applies_to: Applies { categories: vec![Category::Futures], roots: vec!["MES".into(), "MNQ".into()] },
        });
        let mes = instrument("MES", InstrumentKind::FutureRoot);
        let es = instrument("ES", InstrumentKind::FutureRoot);
        assert_eq!(template_for(&templates, &mes).unwrap().name, "Micros");
        assert_eq!(template_for(&templates, &es).unwrap().name, "Futures");
        assert_eq!(size_for(&templates, &mes, None), Quantity::new(5));
        // A root is matched however it was typed.
        templates[2].applies_to.roots = vec!["mes".into()];
        assert_eq!(template_for(&templates, &mes).unwrap().name, "Micros");
        // Narrowed to a root of another category, it applies to nothing.
        templates[2].applies_to.categories = vec![Category::Stocks];
        assert_eq!(template_for(&templates, &mes).unwrap().name, "Futures");
    }

    /// Two that are as narrow as each other: the first listed.
    #[test]
    fn among_as_narrow_the_first_listed_wins() {
        let mut templates = standard();
        let mut again = templates[1].clone();
        again.name = "More futures".into();
        templates.push(again);
        assert_eq!(template_for(&templates, &instrument("NQ", InstrumentKind::FutureRoot)).unwrap().name, "Futures");
    }

    /// A pick sticks while its template is the one in force: on ES or NQ,
    /// both on Futures, and not once the chart is on a stock.
    #[test]
    fn a_picked_size_sticks_to_its_template() {
        let templates = standard();
        let es = instrument("ES", InstrumentKind::FutureRoot);
        let nq = instrument("NQ", InstrumentKind::FutureRoot);
        let aapl = instrument("AAPL", InstrumentKind::Equity);
        let picked = pick(&templates, &es, 2).unwrap();
        assert_eq!(picked, Picked { template: "Futures".into(), size: Quantity::new(2).unwrap() });
        assert_eq!(size_for(&templates, &es, Some(&picked)), Quantity::new(2));
        assert_eq!(size_for(&templates, &nq, Some(&picked)), Quantity::new(2));
        assert_eq!(size_for(&templates, &aapl, Some(&picked)), Quantity::new(100));
        assert_eq!(pick(&templates, &aapl, 9).map(|p| p.size), Quantity::new(900));
        assert_eq!(pick(&templates, &es, 10), None);
        assert_eq!(pick(&templates, &es, 0), None);
    }

    #[test]
    fn templates_are_saved_and_read_back() {
        let store = Store::memory().unwrap();
        assert_eq!(load(&store), standard());
        let mut templates = standard();
        templates[1].sizes = sizes(&[1, 3, 5]);
        templates[1].applies_to.roots = vec!["ES".into()];
        save(&store, &templates);
        assert_eq!(load(&store), templates);
        store.set_setting(SIZE_TEMPLATES, "not json");
        assert_eq!(load(&store), standard());
    }

    #[test]
    fn sizes_are_typed_and_written_the_same_way() {
        assert_eq!(parse_sizes("1, 2,5 10").unwrap(), sizes(&[1, 2, 5, 10]));
        assert_eq!(parse_sizes("3, 3, 1").unwrap(), sizes(&[3, 1]));
        assert!(parse_sizes("1, 0").is_err());
        assert!(parse_sizes("1, two").is_err());
        assert!(parse_sizes(" ").is_err());
        assert_eq!(spell_sizes(&sizes(&[1, 2, 5])), "1, 2, 5");
        assert_eq!(parse_roots("es, NQ es"), vec!["ES".to_string(), "NQ".to_string()]);
    }

    #[test]
    fn a_size_says_its_unit() {
        let one = Quantity::ONE;
        let two = Quantity::new(2).unwrap();
        assert_eq!(size_text(one, InstrumentKind::FutureRoot), "1 contract");
        assert_eq!(size_text(two, InstrumentKind::FutureRoot), "2 contracts");
        assert_eq!(size_text(Quantity::new(100).unwrap(), InstrumentKind::Etf), "100 shares");
        assert_eq!(size_text(two, InstrumentKind::Fx), "2");
    }
}
