//! What a futures contract is, as far as a price on it goes: the step its
//! price moves in, what one point of that price is worth, and in which
//! currency. A chart puts prices on the step, an order is rounded to it,
//! and a position's profit and loss is the move times the point value.
//!
//! The exchanges' own specifications are a table, `contracts.tsv`, read with
//! the rest of [`crate::exchanges`]. A broker or feed that knows a contract
//! may say what it knows too ([`supply`]): it fills in a root the table does
//! not list, and where both say, the table is kept and a disagreement is
//! put in the log once, as it means one of the two is wrong.

use std::collections::HashMap;
use std::sync::Mutex;

use crate::exchanges::registry;
use crate::log;

/// A futures root's specification.
#[derive(Clone, Debug, PartialEq)]
pub struct Spec {
    /// The step every price it trades at is a whole number of.
    pub tick: f64,
    /// What one whole point of the price is worth on one contract, in
    /// [`Spec::currency`]; `None` when nobody has said.
    pub point: Option<f64>,
    /// The currency a point is worth in.
    pub currency: Option<String>,
    /// What the price is a number of: "cents per bushel".
    pub quoted: Option<String>,
    /// The exchange it trades on.
    pub exchange: Option<String>,
}

impl Spec {
    /// What one tick is worth on one contract: $12.50 for ES.
    pub fn tick_value(&self) -> Option<f64> {
        self.point.map(|point| point * self.tick)
    }

    /// What `qty` contracts made going from `from` to `to`: negative `qty`
    /// for a short.
    pub fn pnl(&self, from: f64, to: f64, qty: f64) -> Option<f64> {
        self.point.map(|point| (to - from) * qty * point)
    }
}

/// What a broker or feed says about a contract, each part when it says it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Heard {
    pub tick: Option<f64>,
    pub point: Option<f64>,
    pub currency: Option<String>,
}

static HEARD: Mutex<Option<HashMap<String, Heard>>> = Mutex::new(None);

fn heard(root: &str) -> Option<Heard> {
    HEARD.lock().unwrap_or_else(|e| e.into_inner()).as_ref()?.get(root).cloned()
}

/// The specification of the futures root `root`: the table's, with what a
/// broker said filling in what it lacks. `None` when no tick is known.
pub fn spec(root: &str) -> Option<Spec> {
    let table = registry();
    let said = heard(root).unwrap_or_default();
    Some(Spec {
        tick: table.contract_tick(root).or(said.tick)?,
        point: table.contract_point(root).or(said.point),
        currency: table.contract_currency(root).map(str::to_string).or(said.currency),
        quoted: table.contract_quoted(root).map(str::to_string),
        exchange: table.contract_exchange(root).map(str::to_string),
    })
}

/// The tick of `root`, without building the rest: for what asks on every
/// frame.
pub fn tick(root: &str) -> Option<f64> {
    registry().contract_tick(root).or_else(|| heard(root)?.tick)
}

/// What a point of `root` is worth, without building the rest.
pub fn point(root: &str) -> Option<f64> {
    registry().contract_point(root).or_else(|| heard(root)?.point)
}

/// `source` (a feed's or a broker's name) says this about `root`. Kept for
/// a root the table lacks; where the table says otherwise, the table is kept
/// and the difference logged, once for each thing said.
pub fn supply(source: &str, root: &str, said: Heard) {
    {
        let mut all = HEARD.lock().unwrap_or_else(|e| e.into_inner());
        let all = all.get_or_insert_with(HashMap::new);
        if all.get(root) == Some(&said) {
            return;
        }
        all.insert(root.to_string(), said.clone());
    }
    if let Some(text) = disagreement(source, root, &said) {
        log::global().add(log::Entry::warning(source, text));
    }
}

/// What `said` gets wrong against the exchange's specification, in a
/// trader's words, if anything.
fn disagreement(source: &str, root: &str, said: &Heard) -> Option<String> {
    let table = registry();
    let differs = |ours: Option<f64>, theirs: Option<f64>| match (ours, theirs) {
        (Some(ours), Some(theirs)) => (ours - theirs).abs() > ours.abs() * 1e-9,
        _ => false,
    };
    let (tick, point) = (table.contract_tick(root), table.contract_point(root));
    if !differs(tick, said.tick) && !differs(point, said.point) {
        return None;
    }
    let tick_value = |tick: Option<f64>, point: Option<f64>| match (tick, point) {
        (Some(tick), Some(point)) => format!("a tick of {tick} worth {}", money(tick * point)),
        (Some(tick), None) => format!("a tick of {tick}"),
        _ => "no tick".to_string(),
    };
    Some(format!(
        "{root}: {source} says {}, the exchange's specification {} · using the exchange's",
        tick_value(said.tick.or(tick), said.point.or(point)),
        tick_value(tick, point),
    ))
}

fn money(value: f64) -> String {
    let text = format!("{value:.4}");
    let text = text.trim_end_matches('0').trim_end_matches('.');
    match text.split_once('.') {
        Some((_, cents)) if cents.len() == 1 => format!("${text}0"),
        _ => format!("${text}"),
    }
}

/// A dated contract's root: "ES" for "ESZ6", "6E" for "6EZ26". Anything
/// not shaped like a root, a month code and a year is its own root.
pub fn root_of(contract: &str) -> &str {
    let digits = contract.trim_end_matches(|c: char| c.is_ascii_digit());
    match digits.len() < contract.len() && digits.len() > 1 {
        true => &digits[..digits.len() - 1],
        false => contract,
    }
}

/// The micro contracts and the full-size contract each tracks: the same
/// index, metal, oil or currency at a fraction of the size, and so the
/// same price, which is what lets a chart of one trade the other. CME
/// Group's micro futures, by the root each is quoted against.
const MICROS: &[(&str, &str)] = &[
    ("MES", "ES"),
    ("MNQ", "NQ"),
    ("MYM", "YM"),
    ("M2K", "RTY"),
    ("MCL", "CL"),
    ("MGC", "GC"),
    ("SIL", "SI"),
    ("M6E", "6E"),
    ("M6A", "6A"),
    ("M6B", "6B"),
];

/// The full-size root `root` tracks: "ES" for "MES", and itself for a root
/// that is no micro.
pub fn underlying(root: &str) -> &str {
    MICROS.iter().find(|(micro, _)| *micro == root).map_or(root, |(_, full)| full)
}

/// Do the futures roots `a` and `b` track the same thing at the same
/// price: the same root, or a full-size contract and its micro, either way?
pub fn track_together(a: &str, b: &str) -> bool {
    underlying(a) == underlying(b)
}

/// Are the dated contracts `a` and `b` one another's micro or full-size
/// contract in the same month, "ESZ6" and "MESZ6": two sizes of one thing
/// expiring together, which trade at the same price. Another month of
/// either is not: a calendar spread apart. A year written in one digit
/// and in two, "Z6" and "Z26", is the same year.
pub fn same_month_sizes(a: &str, b: &str) -> bool {
    let (root_a, root_b) = (root_of(a), root_of(b));
    if root_a == a || root_b == b || root_a == root_b || !track_together(root_a, root_b) {
        return false;
    }
    let month = |contract: &str, root: &str| {
        let rest = &contract[root.len()..];
        let (code, year) = rest.split_at(rest.char_indices().nth(1).map_or(rest.len(), |(at, _)| at));
        (code.to_string(), year.to_string())
    };
    let ((code_a, year_a), (code_b, year_b)) = (month(a, root_a), month(b, root_b));
    let years = if year_a.len() == year_b.len() { year_a == year_b } else { year_a.ends_with(&year_b) || year_b.ends_with(&year_a) };
    code_a == code_b && years
}

#[cfg(test)]
pub(crate) fn forget(root: &str) {
    if let Some(all) = HEARD.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
        all.remove(root);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    /// A micro tracks its full-size contract and nothing else: ES and MES
    /// either way, NQ never with MES, a root with itself.
    #[test]
    fn the_same_month_in_the_other_size_trades_at_the_same_price() {
        assert!(same_month_sizes("ESZ6", "MESZ6") && same_month_sizes("MESZ6", "ESZ6"));
        assert!(same_month_sizes("NQZ6", "MNQZ26"), "a year in one digit and in two");
        assert!(!same_month_sizes("ESZ6", "MESH7"), "a calendar spread");
        assert!(!same_month_sizes("ESZ6", "ESH7"), "another month of one size");
        assert!(!same_month_sizes("ESZ6", "ESZ6"), "the contract itself is no stand-in");
        assert!(!same_month_sizes("ESZ6", "MNQZ6"));
        assert!(!same_month_sizes("ES", "MESZ6"), "a root names no month");
    }

    #[test]
    fn a_micro_tracks_its_full_size_contract_alone() {
        for (micro, full) in [("MES", "ES"), ("MNQ", "NQ"), ("MYM", "YM"), ("M2K", "RTY"), ("MCL", "CL"), ("MGC", "GC"), ("SIL", "SI"), ("M6E", "6E")] {
            assert!(track_together(micro, full) && track_together(full, micro), "{micro} {full}");
            assert!(track_together(micro, micro));
            assert_eq!(underlying(micro), full);
        }
        assert!(!track_together("NQ", "MES"));
        assert!(!track_together("ES", "NQ"));
        assert!(!track_together("MES", "MNQ"));
        assert!(track_together("ZN", "ZN"));
        // Every micro and what it tracks are roots the contracts table knows,
        // each with a tick of its own: an order on a micro is never rounded
        // to its full-size contract's.
        for (micro, full) in MICROS {
            assert!(tick(micro).is_some(), "{micro}");
            assert!(tick(full).is_some(), "{full}");
        }
        assert_eq!((tick("M6E"), tick("6E")), (Some(0.0001), Some(0.00005)));
    }

    /// A one-tick move on one contract, in dollars, as the exchanges give it.
    #[test]
    fn a_tick_is_worth_what_the_exchange_says() {
        for (root, value) in [
            ("ES", 12.5),
            ("MES", 1.25),
            ("NQ", 5.0),
            ("MNQ", 0.5),
            ("RTY", 5.0),
            ("M2K", 0.5),
            ("YM", 5.0),
            ("MYM", 0.5),
            ("6E", 6.25),
            ("6J", 6.25),
            ("6B", 6.25),
            ("6A", 5.0),
            ("M6E", 1.25),
            ("M6A", 1.0),
            ("M6B", 0.625),
            ("6C", 5.0),
            ("6S", 6.25),
            ("ZB", 31.25),
            ("ZN", 15.625),
            ("ZF", 7.8125),
            ("ZT", 7.8125),
            ("ZC", 12.5),
            ("ZS", 12.5),
            ("ZW", 12.5),
            ("ZL", 6.0),
            ("ZM", 10.0),
            ("LE", 10.0),
            ("HE", 10.0),
            ("CL", 10.0),
            ("MCL", 1.0),
            ("BZ", 10.0),
            ("NG", 10.0),
            ("RB", 4.2),
            ("HO", 4.2),
            ("PL", 5.0),
            ("PA", 50.0),
            ("GC", 10.0),
            ("MGC", 1.0),
            ("SI", 25.0),
            ("SIL", 5.0),
            ("HG", 12.5),
            ("KC", 18.75),
            ("SB", 11.2),
            ("CC", 10.0),
            ("CT", 5.0),
        ] {
            let spec = spec(root).unwrap_or_else(|| panic!("{root} has a spec"));
            let worth = spec.tick_value().unwrap();
            assert!(close(worth, value), "{root}: a tick is {worth}, not {value}");
            assert_eq!(spec.currency.as_deref(), Some("USD"), "{root}");
            assert!(spec.quoted.is_some() && spec.exchange.is_some(), "{root}");
        }
    }

    #[test]
    fn every_listed_root_has_a_whole_spec() {
        for root in registry().contract_roots() {
            let spec = spec(root).unwrap();
            assert!(spec.point.is_some() && spec.currency.is_some() && spec.quoted.is_some(), "{root}");
        }
    }

    /// The P&L of a one-tick move, long and short.
    #[test]
    fn profit_and_loss_is_the_move_times_the_point() {
        for (root, up, value) in [
            ("ES", 7_653.25, 12.5),
            ("NQ", 21_000.25, 5.0),
            ("CL", 61.01, 10.0),
            ("ZN", 112.0 + 1.0 / 64.0, 15.625),
            ("6E", 1.15005, 6.25),
            ("GC", 2_400.1, 10.0),
            ("ZC", 425.25, 12.5),
        ] {
            let spec = spec(root).unwrap();
            let from = up - spec.tick;
            assert!((spec.pnl(from, up, 1.0).unwrap() - value).abs() < 1e-6, "{root} long");
            assert!((spec.pnl(from, up, -2.0).unwrap() + 2.0 * value).abs() < 1e-6, "{root} short");
        }
    }

    #[test]
    fn a_broker_fills_in_a_root_the_table_lacks() {
        assert_eq!(spec("ZZQ"), None);
        supply("Feed", "ZZQ", Heard { tick: Some(0.5), point: Some(20.0), currency: Some("EUR".into()) });
        let spec = spec("ZZQ").unwrap();
        assert_eq!((spec.tick, spec.point, spec.currency.as_deref()), (0.5, Some(20.0), Some("EUR")));
        assert_eq!(spec.tick_value(), Some(10.0));
        assert_eq!((tick("ZZQ"), point("ZZQ")), (Some(0.5), Some(20.0)));
        forget("ZZQ");
        assert_eq!(tick("ZZQ"), None);
    }

    #[test]
    fn the_exchange_is_kept_over_a_broker_that_disagrees() {
        let agreeing = Heard { tick: Some(0.25), point: Some(50.0), currency: Some("USD".into()) };
        assert_eq!(disagreement("Rithmic", "ES", &agreeing), None);
        let wrong = Heard { tick: Some(0.25), point: Some(5.0), currency: None };
        assert_eq!(
            disagreement("Rithmic", "ES", &wrong).as_deref(),
            Some("ES: Rithmic says a tick of 0.25 worth $1.25, the exchange's specification a tick of 0.25 worth $12.50 · using the exchange's")
        );
        assert_eq!(spec("ES").unwrap().point, Some(50.0));
        assert_eq!(disagreement("Rithmic", "ZN", &Heard { tick: Some(0.015625), point: Some(1000.0), currency: None }), None);
    }

    #[test]
    fn a_contract_is_its_root_a_month_and_a_year() {
        assert_eq!(root_of("ESZ6"), "ES");
        assert_eq!(root_of("6EZ26"), "6E");
        assert_eq!(root_of("MNQH7"), "MNQ");
        assert_eq!(root_of("ES"), "ES");
    }

    #[test]
    fn money_is_written_as_dollars_and_cents() {
        assert_eq!(money(12.5), "$12.50");
        assert_eq!(money(15.625), "$15.625");
        assert_eq!(money(10.0), "$10");
    }
}
