//! Order templates: named brackets an order is sent with.
//!
//! A template is a name, its targets and its stops, either group possibly
//! empty. A chart has one picked, or none, and every order from it — a
//! key, the menu, the order entry dialog, a command — goes as one bracket
//! the trading service holds, its exits worked once the entry fills.
//!
//! Each exit says how far from the fill it rests and how much it closes.
//!
//! **How far**, three ways:
//!
//! - **Ticks**, from the fill.
//! - **Money**, what this exit's own contracts make (a target) or lose (a
//!   stop) at its price.
//! - **Risk**, measured on the whole order: for a stop, the money the whole
//!   order loses if it is hit, which starts at what 1R is worth; for a
//!   target, a multiple of R — 0.5, 1.5, 2 — the whole order makes there,
//!   R being the "Value of 1R" setting the charts write P&L in, so a 2R
//!   target reads +2R on the chart once it is working.
//!
//! Money and risk become ticks with what a tick of the contract is worth,
//! rounded **toward the entry**, and never under one tick: an exit never
//! risks, nor asks for, more than it says.
//!
//! **How much**, a number of contracts or a share of the order. The exits
//! of a group are sized in the order listed: a number is taken as it is, a
//! share of the order's quantity is rounded down, each is held to what is
//! left, and whatever is left after the last goes to the last. So the
//! targets together close exactly what the entry opens, and so do the
//! stops, and an exit left with nothing is not sent. A share of a position
//! held beside the order is not offered: the trading service holds the
//! exits as one bracket with the entry, sized to it.

use serde::{Deserialize, Serialize};

use super::brackets::{ExitLeg, Exits};
use crate::store::Store;

/// The templates, as a JSON list in the order they are listed: the order
/// Ctrl+Shift and a digit counts them in.
pub const ORDER_TEMPLATES: &str = "trading_order_templates";

/// How far from the fill an exit rests.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "mode", content = "value")]
pub enum Distance {
    Ticks(u32),
    /// What the exit's own contracts make or lose there.
    Money(f64),
    /// A stop's money lost by the whole order; a target's multiple of R
    /// made by it.
    Risk(f64),
}

impl Distance {
    pub fn mode(self) -> Mode {
        match self {
            Distance::Ticks(_) => Mode::Ticks,
            Distance::Money(_) => Mode::Money,
            Distance::Risk(_) => Mode::Risk,
        }
    }

    pub fn value(self) -> f64 {
        match self {
            Distance::Ticks(ticks) => f64::from(ticks),
            Distance::Money(value) | Distance::Risk(value) => value,
        }
    }
}

/// The three ways of saying a distance, for the picker.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Ticks,
    Money,
    Risk,
}

impl Mode {
    pub const ALL: [Mode; 3] = [Mode::Ticks, Mode::Money, Mode::Risk];

    pub fn label(self) -> &'static str {
        match self {
            Mode::Ticks => "Ticks",
            Mode::Money => "Money",
            Mode::Risk => "Risk",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Mode::Ticks => "ticks",
            Mode::Money => "money",
            Mode::Risk => "risk",
        }
    }
}

/// How much an exit closes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Size {
    /// So many contracts or shares.
    Fixed(u32),
    /// This share of the order's quantity, `numerator/denominator`.
    Share(u32, u32),
}

impl Size {
    pub const ALL: Size = Size::Share(1, 1);

    /// The shares offered, largest first.
    pub const SHARES: [Size; 6] =
        [Size::Share(1, 1), Size::Share(3, 4), Size::Share(2, 3), Size::Share(1, 2), Size::Share(1, 3), Size::Share(1, 4)];

    /// "All", "1/2", "3".
    pub fn text(self) -> String {
        match self {
            Size::Share(n, d) if n == d => "All".to_string(),
            Size::Share(n, d) => format!("{n}/{d}"),
            Size::Fixed(n) => n.to_string(),
        }
    }

    /// What it asks for of an order of `qty`, before what is left holds it.
    fn wanted(self, qty: u32) -> u32 {
        match self {
            Size::Fixed(n) => n,
            Size::Share(_, 0) => 0,
            Size::Share(n, d) => (u64::from(qty) * u64::from(n) / u64::from(d)).min(u64::from(u32::MAX)) as u32,
        }
    }

    /// "all", "1/2", "3": as typed on the command line, the other way.
    pub fn parse(text: &str) -> Result<Size, String> {
        let text = text.trim().to_lowercase();
        if text == "all" {
            return Ok(Size::ALL);
        }
        if let Some((n, d)) = text.split_once('/') {
            let (n, d) = (n.trim().parse::<u32>(), d.trim().parse::<u32>());
            return match (n, d) {
                (Ok(n), Ok(d)) if n > 0 && d > 0 && n <= d => Ok(Size::Share(n, d)),
                _ => Err(format!("{text:?} is not a share such as 1/2")),
            };
        }
        match text.parse::<u32>() {
            Ok(n) if n > 0 => Ok(Size::Fixed(n)),
            _ => Err(format!("{text:?} is not all, a share such as 1/2, or a number of contracts")),
        }
    }
}

/// One target or stop of a template.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Exit {
    pub distance: Distance,
    pub size: Size,
}

/// A named bracket.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct OrderTemplate {
    /// The app's own number for it, which a chart remembers it by, so that
    /// a rename keeps every chart on it.
    pub id: u32,
    pub name: String,
    #[serde(default)]
    pub targets: Vec<Exit>,
    #[serde(default)]
    pub stops: Vec<Exit>,
}

/// The two groups.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Group {
    Targets,
    Stops,
}

impl OrderTemplate {
    pub fn exits(&self, group: Group) -> &Vec<Exit> {
        match group {
            Group::Targets => &self.targets,
            Group::Stops => &self.stops,
        }
    }

    pub fn exits_mut(&mut self, group: Group) -> &mut Vec<Exit> {
        match group {
            Group::Targets => &mut self.targets,
            Group::Stops => &mut self.stops,
        }
    }

    /// In one line, for a menu or a tooltip: the targets, then the stops,
    /// "+1R 1/2 · +2R rest | −$500 order".
    pub fn summary(&self) -> String {
        let group = |exits: &[Exit], stop: bool| -> String {
            let sizes = exits.len() > 1;
            exits
                .iter()
                .enumerate()
                .map(|(at, exit)| {
                    let sign = if stop { "−" } else { "+" };
                    let distance = match exit.distance {
                        Distance::Ticks(ticks) => format!("{sign}{ticks}t"),
                        Distance::Money(money) => format!("{sign}${}", amount(money)),
                        Distance::Risk(risk) if stop => format!("{sign}${} order", amount(risk)),
                        Distance::Risk(r) => format!("{sign}{}R", amount(r)),
                    };
                    match (sizes, at + 1 == exits.len()) {
                        (false, _) => distance,
                        (true, true) => format!("{distance} rest"),
                        (true, false) => match exit.size {
                            Size::Fixed(n) => format!("{distance} ×{n}"),
                            Size::Share(..) => format!("{distance} {}", exit.size.text()),
                        },
                    }
                })
                .collect::<Vec<_>>()
                .join(" · ")
        };
        match (self.targets.is_empty(), self.stops.is_empty()) {
            (true, true) => "No exits · orders go plain".to_string(),
            (false, true) => group(&self.targets, false),
            (true, false) => group(&self.stops, true),
            (false, false) => format!("{} | {}", group(&self.targets, false), group(&self.stops, true)),
        }
    }
}

/// A number as a person writes money or R: "500", "1.5", "1,250".
pub fn amount(value: f64) -> String {
    let rounded = (value * 100.0).round() / 100.0;
    if rounded.fract() == 0.0 && rounded.abs() < 1e15 {
        super::domain::grouped(rounded.abs() as u64)
    } else {
        let text = format!("{rounded:.2}");
        text.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

/// The templates, as listed; none until somebody makes one, and none when
/// what is stored cannot be read.
pub fn load(store: &Store) -> Vec<OrderTemplate> {
    store.setting(ORDER_TEMPLATES).and_then(|json| serde_json::from_str(&json).ok()).unwrap_or_default()
}

pub fn save(store: &Store, templates: &[OrderTemplate]) {
    if let Ok(json) = serde_json::to_string(templates) {
        store.set_setting(ORDER_TEMPLATES, &json);
    }
}

/// A number no template has had in this list.
pub fn next_id(templates: &[OrderTemplate]) -> u32 {
    templates.iter().map(|template| template.id).max().map_or(1, |id| id + 1)
}

/// A name none of `templates` has: "New template", "New template 2"…
pub fn fresh_name(templates: &[OrderTemplate]) -> String {
    (1..)
        .map(|n| match n {
            1 => "New template".to_string(),
            n => format!("New template {n}"),
        })
        .find(|name| !templates.iter().any(|template| template.name.eq_ignore_ascii_case(name)))
        .unwrap_or_default()
}

/// A new template: a target at 2R and a stop risking what 1R is worth.
pub fn new_template(templates: &[OrderTemplate], r: f64) -> OrderTemplate {
    OrderTemplate {
        id: next_id(templates),
        name: fresh_name(templates),
        targets: vec![Exit { distance: Distance::Risk(2.0), size: Size::ALL }],
        stops: vec![Exit { distance: Distance::Risk(r), size: Size::ALL }],
    }
}

/// The template a chart remembers by `id`, while there is one.
pub fn by_id(templates: &[OrderTemplate], id: u32) -> Option<&OrderTemplate> {
    templates.iter().find(|template| template.id == id)
}

/// A template by its name, however it is cased, or by `id:N`.
pub fn find<'a>(templates: &'a [OrderTemplate], text: &str) -> Option<&'a OrderTemplate> {
    let text = text.trim();
    if let Some(id) = text.strip_prefix("id:").and_then(|id| id.trim().parse::<u32>().ok()) {
        return by_id(templates, id);
    }
    templates.iter().find(|template| template.name.eq_ignore_ascii_case(text))
}

/// Move the template at `from` to `to`, the rest keeping their order.
pub fn move_to(templates: &mut Vec<OrderTemplate>, from: usize, to: usize) {
    if from >= templates.len() {
        return;
    }
    let template = templates.remove(from);
    let to = to.min(templates.len());
    templates.insert(to, template);
}

/// The quantities each exit of a group closes, for an order of `qty`:
/// sized in the order listed, a share rounded down, each held to what is
/// left, and whatever is left after the last given to the last. An exit
/// left with nothing closes nothing and is not sent.
pub fn split(sizes: &[Size], qty: u32) -> Vec<u32> {
    let mut left = qty;
    let mut split: Vec<u32> = sizes
        .iter()
        .map(|size| {
            let took = size.wanted(qty).min(left);
            left -= took;
            took
        })
        .collect();
    if let Some(last) = split.last_mut() {
        *last += left;
    }
    split
}

/// What an order on a contract is worth a tick, and what R is, to turn
/// money and risk into ticks.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Worth {
    /// The step its price moves in.
    pub tick: f64,
    /// What one whole point is worth on one contract, when it is known.
    pub point: Option<f64>,
    /// What 1R is worth: the "Value of 1R" setting.
    pub r: f64,
}

/// The bracket `template` makes of an order of `qty` on a contract worth
/// `worth`: each exit sized and as a distance in ticks. Refused in words
/// when a distance in money cannot be counted, or nothing is left to send.
pub fn bracket(template: &OrderTemplate, qty: u32, worth: Worth) -> Result<Exits, String> {
    if !(worth.tick.is_finite() && worth.tick > 0.0) {
        return Err("the contract's tick is not known, so the template's exits cannot be counted in ticks".into());
    }
    let group = |exits: &[Exit], stop: bool| -> Result<Vec<ExitLeg>, String> {
        let sizes: Vec<Size> = exits.iter().map(|exit| exit.size).collect();
        let mut legs = Vec::new();
        for (exit, leg_qty) in exits.iter().zip(split(&sizes, qty)) {
            if leg_qty == 0 {
                continue;
            }
            let ticks = match exit.distance {
                Distance::Ticks(ticks) => ticks.max(1),
                Distance::Money(money) => ticks_for(money, leg_qty, worth)?,
                Distance::Risk(risk) if stop => ticks_for(risk, qty, worth)?,
                Distance::Risk(r) => ticks_for(r * worth.r, qty, worth)?,
            };
            legs.push(ExitLeg { qty: leg_qty, ticks });
        }
        Ok(legs)
    };
    // A template with no exits sends the order plain: nothing to refuse.
    Ok(Exits { targets: group(&template.targets, false)?, stops: group(&template.stops, true)? })
}

/// How many ticks from the fill `money` is made or lost on `qty`
/// contracts: rounded toward the fill, and at least one.
fn ticks_for(money: f64, qty: u32, worth: Worth) -> Result<u32, String> {
    let Some(point) = worth.point.filter(|point| point.is_finite() && *point > 0.0) else {
        return Err("what a point of the contract is worth is not known, so money cannot be counted in ticks".into());
    };
    if !(money.is_finite() && money > 0.0) {
        return Err("an exit's amount must be more than 0".into());
    }
    let per_tick = worth.tick * point * f64::from(qty.max(1));
    // A hair over, so that $500 at $12.50 a tick is 40 ticks, not 39.
    let ticks = (money / per_tick + 1e-9).floor();
    Ok(ticks.clamp(1.0, f64::from(u32::MAX)) as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ES: Worth = Worth { tick: 0.25, point: Some(50.0), r: 500.0 };

    fn exit(distance: Distance, size: Size) -> Exit {
        Exit { distance, size }
    }

    fn template(targets: Vec<Exit>, stops: Vec<Exit>) -> OrderTemplate {
        OrderTemplate { id: 1, name: "Scalp".into(), targets, stops }
    }

    #[test]
    fn templates_are_saved_read_back_and_none_to_begin_with() {
        let store = Store::memory().unwrap();
        assert!(load(&store).is_empty());
        let mut templates = vec![new_template(&[], 500.0)];
        templates.push(new_template(&templates, 250.0));
        templates[1].targets.push(exit(Distance::Ticks(16), Size::Share(1, 2)));
        templates[1].stops = vec![exit(Distance::Money(120.5), Size::Fixed(2))];
        save(&store, &templates);
        assert_eq!(load(&store), templates);
        assert_eq!((templates[0].name.as_str(), templates[1].name.as_str()), ("New template", "New template 2"));
        assert_eq!((templates[0].id, templates[1].id), (1, 2));
        store.set_setting(ORDER_TEMPLATES, "not json");
        assert!(load(&store).is_empty());
    }

    #[test]
    fn templates_are_found_by_name_or_id_and_moved() {
        let mut templates = vec![new_template(&[], 500.0)];
        templates.push(new_template(&templates, 500.0));
        templates.push(new_template(&templates, 500.0));
        assert_eq!(find(&templates, "new TEMPLATE 2").map(|t| t.id), Some(2));
        assert_eq!(find(&templates, "id:3").map(|t| t.id), Some(3));
        assert_eq!(find(&templates, "nothing"), None);
        move_to(&mut templates, 2, 0);
        assert_eq!(templates.iter().map(|t| t.id).collect::<Vec<_>>(), [3, 1, 2]);
        move_to(&mut templates, 0, 9);
        assert_eq!(templates.iter().map(|t| t.id).collect::<Vec<_>>(), [1, 2, 3]);
        templates.remove(2);
        assert_eq!(next_id(&templates), 3, "a removed template's number is taken again only at the end");
    }

    /// Shares round down, a number is taken as it is, each is held to what
    /// is left, and the last takes whatever is over.
    #[test]
    fn an_order_is_split_across_its_exits() {
        let half = Size::Share(1, 2);
        assert_eq!(split(&[half, half], 4), [2, 2]);
        assert_eq!(split(&[half, half], 3), [1, 2], "the rest goes to the last");
        assert_eq!(split(&[half, half], 1), [0, 1], "an exit with nothing is not sent");
        assert_eq!(split(&[Size::Share(1, 3), Size::Share(1, 3), Size::Share(1, 3)], 5), [1, 1, 3]);
        assert_eq!(split(&[Size::Fixed(1), Size::Fixed(5)], 3), [1, 2], "held to what is left");
        assert_eq!(split(&[Size::Fixed(2), Size::Fixed(2)], 1), [1, 0]);
        assert_eq!(split(&[Size::ALL], 7), [7]);
        assert_eq!(split(&[Size::ALL, half], 2), [2, 0]);
        assert_eq!(split(&[], 3), Vec::<u32>::new());
    }

    /// Ticks are ticks; money is what the exit's own contracts make or
    /// lose; risk is the whole order's, a stop's in money, a target's in R.
    #[test]
    fn each_mode_becomes_ticks_with_what_a_tick_is_worth() {
        // ES: $12.50 a tick a contract.
        let one = template(
            vec![exit(Distance::Ticks(8), Size::ALL)],
            vec![exit(Distance::Money(250.0), Size::ALL)],
        );
        let exits = bracket(&one, 2, ES).unwrap();
        assert_eq!(exits.targets, [ExitLeg { qty: 2, ticks: 8 }]);
        assert_eq!(exits.stops, [ExitLeg { qty: 2, ticks: 10 }], "$250 on two contracts is 10 ticks");

        let risk = template(
            vec![exit(Distance::Risk(1.0), Size::Share(1, 2)), exit(Distance::Risk(2.0), Size::Share(1, 2))],
            vec![exit(Distance::Risk(500.0), Size::ALL)],
        );
        let exits = bracket(&risk, 4, ES).unwrap();
        // The whole order: 4 contracts at $12.50 a tick is $50 a tick.
        assert_eq!(exits.stops, [ExitLeg { qty: 4, ticks: 10 }], "$500 risked on the order");
        assert_eq!(exits.targets, [ExitLeg { qty: 2, ticks: 10 }, ExitLeg { qty: 2, ticks: 20 }], "1R and 2R on the order");

        // Money on each target's own contracts.
        let money = template(vec![exit(Distance::Money(100.0), Size::Fixed(1)), exit(Distance::Money(100.0), Size::ALL)], vec![]);
        let exits = bracket(&money, 3, ES).unwrap();
        assert_eq!(exits.targets, [ExitLeg { qty: 1, ticks: 8 }, ExitLeg { qty: 2, ticks: 4 }]);
        assert!(exits.stops.is_empty());
    }

    /// Rounded toward the fill so that nothing risks or asks for more than
    /// it says, and never under a tick.
    #[test]
    fn money_rounds_toward_the_fill_and_is_at_least_a_tick() {
        let stop = |money: f64| template(vec![], vec![exit(Distance::Money(money), Size::ALL)]);
        let ticks = |money: f64, worth: Worth| bracket(&stop(money), 1, worth).map(|exits| exits.stops[0].ticks);
        assert_eq!(ticks(30.0, ES), Ok(2), "$30 is 2.4 ticks of ES: 2");
        assert_eq!(ticks(37.49, ES), Ok(2));
        assert_eq!(ticks(37.5, ES), Ok(3));
        assert_eq!(ticks(5.0, ES), Ok(1), "never under a tick");
        // MES: $1.25 a tick; NQ: $5.
        let mes = Worth { tick: 0.25, point: Some(5.0), r: 500.0 };
        assert_eq!(ticks(100.0, mes), Ok(80));
        let nq = Worth { tick: 0.25, point: Some(20.0), r: 500.0 };
        assert_eq!(ticks(100.0, nq), Ok(20));
        // A target in R uses what 1R is worth now.
        let target = template(vec![exit(Distance::Risk(1.5), Size::ALL)], vec![]);
        let worth = Worth { r: 250.0, ..ES };
        assert_eq!(bracket(&target, 1, worth).map(|exits| exits.targets[0].ticks), Ok(30));
    }

    #[test]
    fn what_cannot_be_counted_is_refused() {
        let money = template(vec![exit(Distance::Money(100.0), Size::ALL)], vec![]);
        assert!(bracket(&money, 1, Worth { point: None, ..ES }).is_err());
        assert!(bracket(&money, 1, Worth { tick: 0.0, ..ES }).is_err());
        let ticks = template(vec![exit(Distance::Ticks(4), Size::ALL)], vec![]);
        assert!(bracket(&ticks, 1, Worth { point: None, ..ES }).is_ok(), "ticks need no worth");
        assert!(bracket(&template(vec![], vec![]), 1, ES).is_ok_and(|exits| exits.is_empty()), "no exits: the order goes plain");
    }

    #[test]
    fn a_template_says_itself_in_one_line() {
        let risk = template(
            vec![exit(Distance::Risk(1.0), Size::Share(1, 2)), exit(Distance::Risk(2.0), Size::Share(1, 2))],
            vec![exit(Distance::Risk(500.0), Size::ALL)],
        );
        assert_eq!(risk.summary(), "+1R 1/2 · +2R rest | −$500 order");
        let ticks = template(vec![exit(Distance::Ticks(16), Size::ALL)], vec![]);
        assert_eq!(ticks.summary(), "+16t");
        let stop = template(vec![], vec![exit(Distance::Money(1250.0), Size::ALL)]);
        assert_eq!(stop.summary(), "−$1,250");
        assert_eq!(template(vec![], vec![]).summary(), "No exits · orders go plain");
        assert_eq!(template(vec![exit(Distance::Risk(1.5), Size::ALL)], vec![]).summary(), "+1.5R");
    }

    #[test]
    fn sizes_are_typed_as_all_a_share_or_a_number() {
        assert_eq!(Size::parse("all"), Ok(Size::ALL));
        assert_eq!(Size::parse(" 1/2 "), Ok(Size::Share(1, 2)));
        assert_eq!(Size::parse("3"), Ok(Size::Fixed(3)));
        for bad in ["0", "3/2", "0/2", "x", "1/0", ""] {
            assert!(Size::parse(bad).is_err(), "{bad:?}");
        }
        assert_eq!(Size::Share(2, 3).text(), "2/3");
        assert_eq!(Size::ALL.text(), "All");
    }
}
