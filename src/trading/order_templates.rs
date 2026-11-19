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
//! - **Money**, what the whole order makes (a target) or loses (a stop)
//!   if the price gets there, whatever the exit itself closes.
//! - **R**, the same in multiples of R — 0.5, 1, 1.5, 2 — R being the
//!   "Value of 1R" setting: with R at $500, a stop at 1R and one at $500
//!   rest at the same price, and one at 0.5R half as far; a target at 2R
//!   reads +2R on the chart once it is working.
//!
//! Money and R become ticks with what a tick of the contract is worth,
//! rounded **toward the entry**, and never under one tick: an exit never
//! risks, nor asks for, more than it says.
//!
//! **How much**, so many contracts, a share of the position, or the rest:
//! what the exits above leave, all of it for the first. The **position**
//! is what the entry opens, the position its exits protect: the entry's
//! quantity once it has filled. The exits are sized on it when the order
//! is sent, as one bracket the trading service holds, which works them
//! for what has filled; on Sierra Chart, where Omacharts works them
//! itself, a partial fill gets the first exits first, never more than has
//! filled. An entry that adds to a position already held protects only
//! what it adds, and one that closes some or all of a position goes
//! plain, without exits.
//!
//! The exits of a group are sized in the order listed: a number as it
//! is, the shares rounded down as they add up, so that 1/2 and 1/2 of a
//! 3-lot are 1 and 2, the rest what is left, each held to what the ones
//! above it leave. So a group never closes more than the position; it may
//! close less, a target of 1/2 alone leaving the other half to be closed
//! by hand, and an exit sized to nothing is not sent. [`fit`] says, in a
//! line or two, where a group's sizes do not match the position.

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
    /// What the whole order makes or loses there.
    Money(f64),
    /// A multiple of R lost (a stop) or made (a target) by the whole
    /// order: 1 is the "Value of 1R" setting.
    R(f64),
}

impl Distance {
    pub fn mode(self) -> Mode {
        match self {
            Distance::Ticks(_) => Mode::Ticks,
            Distance::Money(_) => Mode::Money,
            Distance::R(_) => Mode::R,
        }
    }

    pub fn value(self) -> f64 {
        match self {
            Distance::Ticks(ticks) => f64::from(ticks),
            Distance::Money(value) | Distance::R(value) => value,
        }
    }
}

/// The three ways of saying a distance, for the picker.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Ticks,
    Money,
    R,
}

impl Mode {
    pub const ALL: [Mode; 3] = [Mode::Ticks, Mode::Money, Mode::R];

    pub fn label(self) -> &'static str {
        match self {
            Mode::Ticks => "Ticks",
            Mode::Money => "Money",
            Mode::R => "R",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Mode::Ticks => "ticks",
            Mode::Money => "money",
            Mode::R => "r",
        }
    }
}

/// How much an exit closes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Size {
    /// So many contracts or shares.
    Fixed(u32),
    /// This share of the position, `numerator/denominator`; one typed as
    /// a percentage is so many hundredths.
    Share(u32, u32),
}

impl Size {
    pub const ALL: Size = Size::Share(1, 1);

    /// The shares offered, largest first.
    pub const SHARES: [Size; 5] = [Size::Share(3, 4), Size::Share(2, 3), Size::Share(1, 2), Size::Share(1, 3), Size::Share(1, 4)];

    /// "All", "1/2", "33%", "3".
    pub fn text(self) -> String {
        match self {
            Size::Share(n, d) if n == d => "All".to_string(),
            Size::Share(n, 100) => format!("{n}%"),
            Size::Share(n, d) => format!("{n}/{d}"),
            Size::Fixed(n) => n.to_string(),
        }
    }

    /// A share as typed: "1/3", "33%" or "0.33", under the whole position.
    pub fn parse_share(text: &str) -> Result<Size, String> {
        let text = text.trim().to_lowercase();
        let refused = || format!("{text:?} is not a share such as 1/3, 33% or 0.33");
        if let Some((n, d)) = text.split_once('/') {
            return match (n.trim().parse::<u32>(), d.trim().parse::<u32>()) {
                (Ok(n), Ok(d)) if n > 0 && n < d => Ok(Size::Share(n, d)),
                _ => Err(refused()),
            };
        }
        // 33% and 33 are percentages, 0.33 a decimal.
        let percent = match text.strip_suffix('%') {
            Some(percent) => percent.trim().parse::<f64>().map_err(|_| refused())?,
            None => match text.parse::<f64>().map_err(|_| refused())? {
                decimal if decimal < 1.0 => decimal * 100.0,
                percent => percent,
            },
        };
        let percent = percent.round();
        if !(1.0..100.0).contains(&percent) {
            return Err(refused());
        }
        // A share the editor offers is said as it says it: 50% is 1/2.
        Ok(match percent as u32 {
            25 => Size::Share(1, 4),
            50 => Size::Share(1, 2),
            75 => Size::Share(3, 4),
            percent => Size::Share(percent, 100),
        })
    }

    /// The rest: what the exits above leave, all of it for the first.
    pub fn is_all(self) -> bool {
        matches!(self, Size::Share(n, d) if n == d)
    }

    /// "all", "3", or a share, "1/2", "33%": as typed on the command line,
    /// the other way.
    pub fn parse(text: &str) -> Result<Size, String> {
        let text = text.trim().to_lowercase();
        if text == "all" || text == "rest" {
            return Ok(Size::ALL);
        }
        if let Ok(n) = text.parse::<u32>() {
            return match n {
                0 => Err(format!("{text:?}: a number of contracts is at least 1")),
                n => Ok(Size::Fixed(n)),
            };
        }
        Size::parse_share(&text).map_err(|_| format!("{text:?} is not all, a number of contracts, or a share such as 1/2 or 33%"))
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
    /// "+1R 1/2 · +2R rest | −1R".
    pub fn summary(&self) -> String {
        let group = |exits: &[Exit], stop: bool| -> String {
            exits
                .iter()
                .enumerate()
                .map(|(at, exit)| {
                    let sign = if stop { "−" } else { "+" };
                    let distance = match exit.distance {
                        Distance::Ticks(ticks) => format!("{sign}{ticks}t"),
                        Distance::Money(money) => format!("{sign}${}", amount(money)),
                        Distance::R(r) => format!("{sign}{}R", amount(r)),
                    };
                    match exit.size {
                        Size::Fixed(n) => format!("{distance} ×{n}"),
                        size if size.is_all() && exits.len() == 1 => distance,
                        size if size.is_all() && at > 0 => format!("{distance} rest"),
                        size if size.is_all() => format!("{distance} all"),
                        size => format!("{distance} {}", size.text()),
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
    named(templates, "New template")
}

/// `name`, or "`name` 2"… when a template has it.
fn named(templates: &[OrderTemplate], name: &str) -> String {
    (1..)
        .map(|n| match n {
            1 => name.to_string(),
            n => format!("{name} {n}"),
        })
        .find(|name| !templates.iter().any(|template| template.name.eq_ignore_ascii_case(name)))
        .unwrap_or_default()
}

/// A new template: a target at 2R and a stop at 1R.
pub fn new_template(templates: &[OrderTemplate]) -> OrderTemplate {
    OrderTemplate { name: fresh_name(templates), ..Preset::TargetAndStop.template(templates) }
}

/// What a template can start as, named by its shape.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Preset {
    /// A target at 2R and a stop at 1R, each for all of it.
    TargetAndStop,
    /// Half the position off at 1.5R, the other half left without a
    /// target, and a stop at 1R for all of it.
    HalfOff,
    /// 1 contract off at 1R, 1 at 2R, the rest at 3R, and a stop at 1R.
    Runner,
    /// A stop at 1R, and no target.
    StopOnly,
}

impl Preset {
    pub const ALL: [Preset; 4] = [Preset::TargetAndStop, Preset::HalfOff, Preset::Runner, Preset::StopOnly];

    pub fn label(self) -> &'static str {
        match self {
            Preset::TargetAndStop => "Target and stop",
            Preset::HalfOff => "Half off",
            Preset::Runner => "Runner",
            Preset::StopOnly => "Stop only",
        }
    }

    /// The template, called after the preset, "Half off 2" when the name
    /// is taken.
    pub fn template(self, templates: &[OrderTemplate]) -> OrderTemplate {
        let r = |multiple: f64, size: Size| Exit { distance: Distance::R(multiple), size };
        let targets = match self {
            Preset::TargetAndStop => vec![r(2.0, Size::ALL)],
            Preset::HalfOff => vec![r(1.5, Size::Share(1, 2))],
            Preset::Runner => vec![r(1.0, Size::Fixed(1)), r(2.0, Size::Fixed(1)), r(3.0, Size::ALL)],
            Preset::StopOnly => vec![],
        };
        OrderTemplate { id: next_id(templates), name: named(templates, self.label()), targets, stops: vec![r(1.0, Size::ALL)] }
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
/// sized in the order listed, each held to what the ones above it leave.
/// An exit left with nothing closes nothing and is not sent.
pub fn split(sizes: &[Size], qty: u32) -> Vec<u32> {
    sizing(sizes, qty).into_iter().map(|(_, took)| took).collect()
}

/// What each exit asks for of an order of `qty`, and what it takes: a
/// number as it is; the shares rounded down as they add up, so that 1/2
/// and 1/2 of 3 are 1 and 2; the rest what the ones above leave.
fn sizing(sizes: &[Size], qty: u32) -> Vec<(u64, u32)> {
    let mut left = qty;
    let mut shared = 0.0_f64;
    let upto = |share: f64| (f64::from(qty) * share + 1e-9).floor() as u64;
    sizes
        .iter()
        .map(|size| {
            let wanted = match *size {
                size if size.is_all() => u64::from(left),
                Size::Fixed(n) => u64::from(n),
                Size::Share(_, 0) => 0,
                Size::Share(n, d) => {
                    let before = upto(shared);
                    shared += f64::from(n) / f64::from(d);
                    upto(shared) - before
                }
            };
            let took = wanted.min(u64::from(left)) as u32;
            left -= took;
            (wanted, took)
        })
        .collect()
}

/// What the editor says under a group whose sizes do not fit every
/// order: a fact, or a warning.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Notice {
    pub text: String,
    pub warning: bool,
}

/// Where a group's sizes do not match the position, one short line for
/// each, warnings first, at most two:
///
/// - More asked for than the position, shares only, a warning: "Targets
///   close more than the position"; or exits after a Rest, which get
///   nothing: "Stops after Rest are never sent".
/// - Part of the position left without the group, a fact for targets
///   and a warning for stops: "1/2 of position has no target", "Contracts
///   past 2 have no stop", "Part of the position has no target".
/// - On small orders, an exit sized to nothing, a fact for targets and a
///   warning for stops: "Needs 2
///   contracts to send the target", "… to send every target".
pub fn fit(exits: &[Exit], group: Group) -> Vec<Notice> {
    let (noun, one) = match group {
        Group::Targets => ("Targets", "target"),
        Group::Stops => ("Stops", "stop"),
    };
    let stop = group == Group::Stops;
    let sizes: Vec<Size> = exits.iter().map(|exit| exit.size).collect();
    if sizes.is_empty() {
        return Vec::new();
    }
    let mut lines = Vec::new();
    let rest = sizes.iter().position(|size| size.is_all());
    let fixed: u64 = sizes.iter().map(|size| if let Size::Fixed(n) = size { u64::from(*n) } else { 0 }).sum();
    let shared: f64 = sizes
        .iter()
        .map(|size| match *size {
            Size::Share(n, d) if n < d => f64::from(n) / f64::from(d),
            _ => 0.0,
        })
        .sum();
    if rest.is_some_and(|at| at + 1 < sizes.len()) {
        lines.push(Notice { text: format!("{noun} after Rest are never sent"), warning: true });
    }
    if fixed == 0 && shared > 1.0 + 1e-9 {
        lines.push(Notice { text: format!("{noun} close more than the position"), warning: true });
    }
    let uncovered = match () {
        _ if rest.is_some() => None,
        _ if fixed == 0 && shared < 1.0 - 1e-9 => Some(format!("{} of position has no {one}", share_text(1.0 - shared))),
        _ if shared == 0.0 => Some(format!("Contracts past {fixed} have no {one}")),
        _ if shared < 1.0 - 1e-9 => Some(format!("Part of the position has no {one}")),
        _ => None,
    };
    if let Some(text) = uncovered {
        lines.push(Notice { text, warning: stop });
    }
    // The smallest order that sends every exit, found by sizing them.
    let from = (1..=10_000u32).find(|qty| split(&sizes, *qty).iter().all(|took| *took > 0));
    if let Some(from) = from.filter(|from| *from > 1) {
        let text = match sizes.len() {
            1 => format!("Needs {from} contracts to send the {one}"),
            _ => format!("Needs {from} contracts to send every {one}"),
        };
        // A stop not sent leaves that order without one.
        lines.push(Notice { text, warning: stop });
    }
    lines.sort_by_key(|line| !line.warning);
    lines.truncate(2);
    lines
}

/// A share as a person says it: "1/2", "2/3", "1/4", else "35%".
fn share_text(share: f64) -> String {
    for d in [2u32, 3, 4] {
        for n in 1..d {
            if (f64::from(n) / f64::from(d) - share).abs() < 1e-6 {
                return format!("{n}/{d}");
            }
        }
    }
    format!("{}%", (share * 100.0).round())
}

/// What an order on a contract is worth a tick, and what R is, to turn
/// money and R into ticks.
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
    let group = |exits: &[Exit]| -> Result<Vec<ExitLeg>, String> {
        let sizes: Vec<Size> = exits.iter().map(|exit| exit.size).collect();
        let mut legs = Vec::new();
        for (exit, leg_qty) in exits.iter().zip(split(&sizes, qty)) {
            if leg_qty == 0 {
                continue;
            }
            let ticks = match exit.distance {
                Distance::Ticks(ticks) => ticks.max(1),
                Distance::Money(money) => ticks_for(money, qty, worth)?,
                Distance::R(r) => ticks_for(r * worth.r, qty, worth)?,
            };
            legs.push(ExitLeg { qty: leg_qty, ticks });
        }
        Ok(legs)
    };
    // A template with no exits sends the order plain: nothing to refuse.
    Ok(Exits { targets: group(&template.targets)?, stops: group(&template.stops)? })
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
        let mut templates = vec![new_template(&[])];
        templates.push(new_template(&templates));
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
        let mut templates = vec![new_template(&[])];
        templates.push(new_template(&templates));
        templates.push(new_template(&templates));
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
        assert_eq!(split(&[half, half], 3), [1, 2], "shares rounded as they add up");
        assert_eq!(split(&[half], 3), [1], "the other half without one");
        assert_eq!(split(&[half, half], 1), [0, 1], "an exit with nothing is not sent");
        assert_eq!(split(&[Size::Share(1, 3), Size::Share(1, 3), Size::Share(1, 3)], 5), [1, 2, 2], "shares rounded as they add up");
        assert_eq!(split(&[Size::Fixed(1), Size::Fixed(1), Size::Fixed(9)], 3), [1, 1, 1], "held to what is left");
        assert_eq!(split(&[Size::Fixed(1), Size::Fixed(1), Size::ALL], 5), [1, 1, 3]);
        assert_eq!(split(&[Size::Share(33, 100), Size::ALL], 3), [0, 3], "33% of 3 is under one");
        assert_eq!(split(&[Size::Share(33, 100), Size::ALL], 10), [3, 7]);
        assert_eq!(split(&[Size::Fixed(1), Size::Fixed(5)], 3), [1, 2], "held to what is left");
        assert_eq!(split(&[Size::Fixed(2), Size::Fixed(2)], 1), [1, 0]);
        assert_eq!(split(&[Size::ALL], 7), [7]);
        assert_eq!(split(&[Size::ALL, half], 2), [2, 0]);
        assert_eq!(split(&[], 3), Vec::<u32>::new());
    }

    /// Ticks are ticks; money and R are what the whole order makes or
    /// loses there, whatever the exit closes, a stop's and a target's
    /// alike.
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
            vec![exit(Distance::R(1.0), Size::Share(1, 2)), exit(Distance::R(2.0), Size::Share(1, 2))],
            vec![exit(Distance::R(1.0), Size::ALL)],
        );
        let exits = bracket(&risk, 4, ES).unwrap();
        // The whole order: 4 contracts at $12.50 a tick is $50 a tick.
        assert_eq!(exits.stops, [ExitLeg { qty: 4, ticks: 10 }], "$500 risked on the order");
        assert_eq!(exits.targets, [ExitLeg { qty: 2, ticks: 10 }, ExitLeg { qty: 2, ticks: 20 }], "1R and 2R on the order");

        // Half of R risked: $250 on 4 contracts, 5 ticks; one and a half,
        // $750, 15.
        for (r, ticks) in [(0.5, 5), (1.5, 15)] {
            let stop = template(vec![], vec![exit(Distance::R(r), Size::ALL)]);
            assert_eq!(bracket(&stop, 4, ES).unwrap().stops, [ExitLeg { qty: 4, ticks }], "{r}R");
        }

        // Money on the whole order too: 3 contracts at $37.50 a tick, so
        // $150 is 4 ticks for the 1 contract and for the 2 alike.
        let money = template(vec![exit(Distance::Money(150.0), Size::Fixed(1)), exit(Distance::Money(150.0), Size::ALL)], vec![]);
        let exits = bracket(&money, 3, ES).unwrap();
        assert_eq!(exits.targets, [ExitLeg { qty: 1, ticks: 4 }, ExitLeg { qty: 2, ticks: 4 }]);
        assert!(exits.stops.is_empty());
        // $500 and 1R, R at $500, rest at the same price.
        let same = template(vec![exit(Distance::Money(500.0), Size::Fixed(1)), exit(Distance::R(1.0), Size::ALL)], vec![]);
        let exits = bracket(&same, 3, ES).unwrap();
        assert_eq!(exits.targets[0].ticks, exits.targets[1].ticks);
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
        // R is what 1R is worth now, for a target and a stop alike.
        let target = template(vec![exit(Distance::R(1.5), Size::ALL)], vec![]);
        let worth = Worth { r: 250.0, ..ES };
        assert_eq!(bracket(&target, 1, worth).map(|exits| exits.targets[0].ticks), Ok(30));
        let stop = template(vec![], vec![exit(Distance::R(1.0), Size::ALL)]);
        assert_eq!(bracket(&stop, 1, worth).map(|exits| exits.stops[0].ticks), Ok(20));
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
            vec![exit(Distance::R(1.0), Size::Share(1, 2)), exit(Distance::R(2.0), Size::Share(1, 2))],
            vec![exit(Distance::R(1.0), Size::ALL)],
        );
        assert_eq!(risk.summary(), "+1R 1/2 · +2R 1/2 | −1R");
        assert_eq!(template(vec![], vec![exit(Distance::R(0.5), Size::ALL)]).summary(), "−0.5R");
        assert_eq!(
            serde_json::to_value(exit(Distance::R(0.5), Size::ALL)).unwrap()["distance"],
            serde_json::json!({"mode": "r", "value": 0.5}),
            "kept as a multiple of R"
        );
        let ticks = template(vec![exit(Distance::Ticks(16), Size::ALL)], vec![]);
        assert_eq!(ticks.summary(), "+16t");
        let stop = template(vec![], vec![exit(Distance::Money(1250.0), Size::ALL)]);
        assert_eq!(stop.summary(), "−$1,250");
        assert_eq!(template(vec![], vec![]).summary(), "No exits · orders go plain");
        assert_eq!(template(vec![exit(Distance::R(1.5), Size::ALL)], vec![]).summary(), "+1.5R");
        let contracts = template(
            vec![exit(Distance::Ticks(8), Size::Fixed(1)), exit(Distance::Ticks(16), Size::Fixed(1)), exit(Distance::Ticks(40), Size::ALL)],
            vec![],
        );
        assert_eq!(contracts.summary(), "+8t ×1 · +16t ×1 · +40t rest");
    }

    /// What a group says where its sizes do not match the position: the
    /// part left without it, more than the position, and from what order
    /// every exit is sent; a stop's gap is a warning, a target's a fact.
    #[test]
    fn a_group_says_where_its_sizes_do_not_match_the_position() {
        let group = |sizes: &[Size]| sizes.iter().map(|size| exit(Distance::Ticks(8), *size)).collect::<Vec<_>>();
        let says = |sizes: &[Size], which| {
            fit(&group(sizes), which).into_iter().map(|notice| (notice.text, notice.warning)).collect::<Vec<_>>()
        };
        let fact = |text: &str| (text.to_string(), false);
        let warning = |text: &str| (text.to_string(), true);
        assert_eq!(says(&[Size::ALL], Group::Stops), []);
        assert_eq!(says(&[], Group::Stops), [], "no stops is a choice");
        assert_eq!(says(&[Size::Fixed(1), Size::ALL], Group::Targets), [fact("Needs 2 contracts to send every target")]);
        assert_eq!(says(&[Size::Share(1, 2)], Group::Targets), [fact("1/2 of position has no target"), fact("Needs 2 contracts to send the target")]);
        assert_eq!(says(&[Size::Share(1, 2)], Group::Stops), [warning("1/2 of position has no stop"), warning("Needs 2 contracts to send the stop")]);
        assert_eq!(says(&[Size::Share(1, 3), Size::Share(1, 3)], Group::Targets)[0], fact("1/3 of position has no target"));
        assert_eq!(says(&[Size::Share(65, 100)], Group::Targets)[0], fact("35% of position has no target"));
        assert_eq!(says(&[Size::Fixed(1), Size::Fixed(1)], Group::Targets)[0], fact("Contracts past 2 have no target"));
        assert_eq!(says(&[Size::Fixed(1)], Group::Stops), [warning("Contracts past 1 have no stop")]);
        assert_eq!(says(&[Size::Fixed(1), Size::Share(1, 2)], Group::Targets)[0], fact("Part of the position has no target"));
        assert_eq!(says(&[Size::Share(3, 4), Size::Share(1, 2)], Group::Stops)[0], warning("Stops close more than the position"));
        assert_eq!(says(&[Size::ALL, Size::Fixed(1)], Group::Targets)[0], warning("Targets after Rest are never sent"));
    }

    /// The template the author trades: a stop at 1R for the whole
    /// position and a target at 1.5R for half of it, the other half left
    /// without one; and the same said in money. R at $500, a buy filled at
    /// 6,000.00, on ES ($12.50 a tick) and MES ($1.25), 1 to 4 contracts:
    /// where each exit rests and what the target closes. Distances round
    /// toward the fill; half of 1 is no target, half of 3 is 1.
    #[test]
    fn half_off_at_one_and_a_half_r_lands_where_it_should() {
        use crate::trading::Side;
        let in_r = template(vec![exit(Distance::R(1.5), Size::Share(1, 2))], vec![exit(Distance::R(1.0), Size::ALL)]);
        let in_money = template(vec![exit(Distance::Money(750.0), Size::Share(1, 2))], vec![exit(Distance::Money(500.0), Size::ALL)]);
        let mes = Worth { tick: 0.25, point: Some(5.0), r: 500.0 };
        // (worth, qty, stop, target as (qty, price), if any)
        type Case = (Worth, u32, f64, Option<(u32, f64)>);
        let cases: [Case; 8] = [
            (ES, 1, 5990.00, None),
            (ES, 2, 5995.00, Some((1, 6007.50))),
            (ES, 3, 5996.75, Some((1, 6005.00))),
            (ES, 4, 5997.50, Some((2, 6003.75))),
            (mes, 1, 5900.00, None),
            (mes, 2, 5950.00, Some((1, 6075.00))),
            (mes, 3, 5966.75, Some((1, 6050.00))),
            (mes, 4, 5975.00, Some((2, 6037.50))),
        ];
        for (worth, qty, stop, target) in cases {
            let exits = bracket(&in_r, qty, worth).unwrap();
            assert_eq!(bracket(&in_money, qty, worth).unwrap(), exits, "$500 and $750 are 1R and 1.5R: {qty} at {worth:?}");
            let (targets, stops) = exits.prices(Side::Buy, 6000.0, worth.tick);
            assert_eq!(stops, [(qty, stop)], "the stop on {qty} at {worth:?}");
            assert_eq!(targets.first().copied(), target, "the target on {qty} at {worth:?}");
            assert!(targets.len() <= 1);
        }
        // A sell the other way about: 3 ES, 13 ticks up and 20 down.
        let (targets, stops) = bracket(&in_r, 3, ES).unwrap().prices(Side::Sell, 6000.0, 0.25);
        assert_eq!((targets, stops), (vec![(1, 5995.0)], vec![(3, 6003.25)]));
        // The editor says the half without a target, and from when the
        // target is sent.
        let notices: Vec<String> = fit(&in_r.targets, Group::Targets).into_iter().map(|notice| notice.text).collect();
        assert_eq!(notices, ["1/2 of position has no target", "Needs 2 contracts to send the target"]);
        assert!(fit(&in_r.stops, Group::Stops).is_empty());
        assert_eq!(in_r.summary(), "+1.5R 1/2 | −1R");
    }

    /// A template starts as one of four shapes, named after it.
    #[test]
    fn a_template_starts_from_a_preset() {
        let mut made = Vec::new();
        for preset in Preset::ALL {
            let template = preset.template(&made);
            made.push(template);
        }
        let summaries: Vec<String> = made.iter().map(|t| format!("{}: {}", t.name, t.summary())).collect();
        assert_eq!(
            summaries,
            [
                "Target and stop: +2R | −1R",
                "Half off: +1.5R 1/2 | −1R",
                "Runner: +1R ×1 · +2R ×1 · +3R rest | −1R",
                "Stop only: −1R"
            ]
        );
        assert_eq!(made.iter().map(|t| t.id).collect::<Vec<_>>(), [1, 2, 3, 4]);
        assert_eq!(Preset::HalfOff.template(&made).name, "Half off 2");
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
        assert_eq!(Size::parse("33%"), Ok(Size::Share(33, 100)));
    }

    #[test]
    fn a_share_is_typed_as_a_fraction_a_percentage_or_a_decimal() {
        assert_eq!(Size::parse_share("1/3"), Ok(Size::Share(1, 3)));
        assert_eq!(Size::parse_share(" 33 % "), Ok(Size::Share(33, 100)));
        assert_eq!(Size::parse_share("0.33"), Ok(Size::Share(33, 100)));
        assert_eq!(Size::parse_share(".5"), Ok(Size::Share(1, 2)), "a share offered, said as offered");
        assert_eq!(Size::parse_share("40"), Ok(Size::Share(40, 100)), "a bare number is a percentage");
        assert_eq!(Size::Share(33, 100).text(), "33%");
        for bad in ["0", "100%", "100", "1/1", "0.001", "x", "-0.5", "2/3/4", ""] {
            assert!(Size::parse_share(bad).is_err(), "{bad:?}");
        }
    }
}
