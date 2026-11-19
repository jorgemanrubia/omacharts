//! A bracket: an entry sent together with the target and the stop that
//! close it, so that from the moment it fills the position is never
//! without them.
//!
//! The trading service keeps the two exits and works them once the entry
//! fills, at their distance from the fill, in ticks: Rithmic's own bracket
//! order. What is typed is turned into those distances here.
//!
//! Each exit is typed either as a price, `7620.50`, or as a distance in
//! points from the fill with its sign, `+6` or `-4`: the sign says which
//! way the price moves, so a long's target is `+` and its stop `-`, and a
//! short's the other way round. A price is turned into a distance from
//! the price the entry is expected at — its own price for a limit or a
//! stop, the last price for a market order — so it moves with the fill
//! by however far the fill is from there.

use super::Side;

/// One exit as typed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Leg {
    /// A price.
    At(f64),
    /// Points from the fill, signed as the price moves.
    Points(f64),
}

/// One exit of a bracket: how many it closes, and how far from the fill
/// it rests, in ticks.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ExitLeg {
    pub qty: u32,
    pub ticks: u32,
}

/// An exit placed: what it closes, and at what price.
pub type Placed = (u32, f64);

/// A bracket's exits: its targets and its stops, each group closing no
/// more than the entry between them, or absent. At least one exit.
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Exits {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub targets: Vec<ExitLeg>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub stops: Vec<ExitLeg>,
}

impl Exits {
    /// One target and one stop, each for the whole of `qty`.
    pub fn single(qty: u32, target: Option<u32>, stop: Option<u32>) -> Exits {
        let leg = |ticks| ExitLeg { qty, ticks };
        Exits { targets: target.map(leg).into_iter().collect(), stops: stop.map(leg).into_iter().collect() }
    }

    pub fn is_empty(&self) -> bool {
        self.targets.is_empty() && self.stops.is_empty()
    }

    /// Where each exit stands once the entry fills at `fill`, with what it
    /// closes: the targets, then the stops.
    pub fn prices(&self, side: Side, fill: f64, tick: f64) -> (Vec<Placed>, Vec<Placed>) {
        let toward = match side {
            Side::Buy => 1.0,
            Side::Sell => -1.0,
        };
        let at = |leg: &ExitLeg, sign: f64| {
            (leg.qty, super::on_contract_tick(fill + sign * leg.ticks as f64 * tick, Some(tick)))
        };
        (
            self.targets.iter().map(|leg| at(leg, toward)).collect(),
            self.stops.iter().map(|leg| at(leg, -toward)).collect(),
        )
    }

    /// The exits in words for the Log: "target 8 ticks ×1, stop 16 ticks ×2".
    pub fn words(&self) -> String {
        let mut words = Vec::new();
        for (name, legs) in [("target", &self.targets), ("stop", &self.stops)] {
            for leg in legs {
                words.push(format!("{name} {} ticks ×{}", leg.ticks, leg.qty));
            }
        }
        words.join(", ")
    }
}

/// An exit as typed on the command line: a price, or points from the fill
/// with a `+` or a `-` in front.
pub fn leg(text: &str) -> Result<Leg, String> {
    let text = text.trim();
    let (signed, digits) = match text.strip_prefix('+') {
        Some(rest) => (Some(1.0), rest),
        None => match text.strip_prefix('-') {
            Some(rest) => (Some(-1.0), rest),
            None => (None, text),
        },
    };
    let number = digits
        .replace(',', "")
        .parse::<f64>()
        .ok()
        .filter(|number| number.is_finite() && *number > 0.0 && !digits.starts_with(['+', '-']))
        .ok_or_else(|| format!("{text:?} is not a price, or points from the fill such as +6 or -4"))?;
    Ok(match signed {
        Some(sign) => Leg::Points(sign * number),
        None => Leg::At(number),
    })
}

/// The exits of an entry of `qty` on `side` expected at `reference`, on a
/// contract stepping `tick`, each for the whole of it: each leg as a
/// distance in ticks, refused in words when it is on the wrong side of the
/// entry or no distance at all.
pub fn exits(side: Side, reference: f64, tick: f64, qty: u32, target: Option<Leg>, stop: Option<Leg>) -> Result<Exits, String> {
    if target.is_none() && stop.is_none() {
        return Err("a bracket needs a target, a stop-loss or both".to_string());
    }
    if !(tick.is_finite() && tick > 0.0) {
        return Err("the contract's tick is not known, so its exits cannot be counted in ticks".to_string());
    }
    if !reference.is_finite() {
        return Err("there is no price to count the exits from".to_string());
    }
    // Which way the price goes for the target: up for a long.
    let up = match side {
        Side::Buy => 1.0,
        Side::Sell => -1.0,
    };
    let ticks = |leg: Leg, toward: f64, name: &str| -> Result<u32, String> {
        let moved = match leg {
            Leg::Points(points) => points,
            Leg::At(price) => price - reference,
        };
        if moved * toward <= 0.0 {
            let way = if toward > 0.0 { "above" } else { "below" };
            let sign = if toward > 0.0 { "+" } else { "-" };
            return Err(match leg {
                Leg::Points(_) => format!("a {} {name} is {sign} points from the fill", side.word().to_lowercase()),
                Leg::At(price) => {
                    format!("a {} {name} goes {way} the entry, {}: {} is not", side.word().to_lowercase(), trim(reference), trim(price))
                }
            });
        }
        let ticks = (moved.abs() / tick).round();
        Ok(ticks.max(1.0).min(u32::MAX as f64) as u32)
    };
    Ok(Exits::single(
        qty,
        target.map(|leg| ticks(leg, up, "target")).transpose()?,
        stop.map(|leg| ticks(leg, -up, "stop-loss")).transpose()?,
    ))
}

fn trim(price: f64) -> String {
    let text = format!("{price:.6}");
    text.trim_end_matches('0').trim_end_matches('.').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_leg_is_a_price_or_signed_points() {
        assert_eq!(leg("7620.50"), Ok(Leg::At(7620.5)));
        assert_eq!(leg("7,620.50"), Ok(Leg::At(7620.5)));
        assert_eq!(leg("+6"), Ok(Leg::Points(6.0)));
        assert_eq!(leg("-4"), Ok(Leg::Points(-4.0)));
        assert_eq!(leg(" -2.5 "), Ok(Leg::Points(-2.5)));
        for bad in ["", "+", "-", "0", "+0", "abc", "--4", "+-4", "NaN", "inf"] {
            assert!(leg(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn points_become_ticks_from_the_fill() {
        let exits = exits(Side::Buy, 7612.25, 0.25, 1, Some(Leg::Points(6.0)), Some(Leg::Points(-4.0)));
        assert_eq!(exits, Ok(Exits::single(1, Some(24), Some(16))));
        let short = super::exits(Side::Sell, 7612.25, 0.25, 1, Some(Leg::Points(-6.0)), Some(Leg::Points(4.0)));
        assert_eq!(short, Ok(Exits::single(1, Some(24), Some(16))));
    }

    #[test]
    fn prices_become_ticks_from_where_the_entry_is_expected() {
        let exits = exits(Side::Buy, 7612.25, 0.25, 2, Some(Leg::At(7618.25)), Some(Leg::At(7610.0)));
        assert_eq!(exits, Ok(Exits::single(2, Some(24), Some(9))));
        let (targets, stops) = exits.unwrap().prices(Side::Buy, 7613.0, 0.25);
        assert_eq!((targets, stops), (vec![(2, 7619.0)], vec![(2, 7610.75)]), "they move with the fill");
    }

    #[test]
    fn an_exit_on_the_wrong_side_is_refused() {
        assert!(exits(Side::Buy, 7612.25, 0.25, 1, Some(Leg::Points(-6.0)), None).is_err());
        assert!(exits(Side::Buy, 7612.25, 0.25, 1, None, Some(Leg::Points(4.0))).is_err());
        assert!(exits(Side::Sell, 7612.25, 0.25, 1, Some(Leg::At(7620.0)), None).is_err());
        assert!(exits(Side::Buy, 7612.25, 0.25, 1, Some(Leg::At(7612.25)), None).is_err(), "no distance");
        assert!(exits(Side::Buy, 7612.25, 0.25, 1, None, None).is_err());
        assert!(exits(Side::Buy, 7612.25, 0.0, 1, Some(Leg::Points(6.0)), None).is_err());
    }

    #[test]
    fn a_distance_under_a_tick_is_one_tick() {
        assert_eq!(exits(Side::Buy, 100.0, 0.25, 1, Some(Leg::Points(0.1)), None).map(|e| e.targets[0].ticks), Ok(1));
    }

}
