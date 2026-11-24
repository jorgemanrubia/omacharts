//! An indicator written as one line of text: what the picker reads — a name
//! and the lengths after it, "ema 20", "bb 20 2", "macd 12 26 9" — then the
//! settings that change what it computes, named as `chart indicator`'s
//! flags name them: "vwap --anchor week --bands 1,3",
//! "tpo --tpo-minutes 30 --row-ticks 4".
//!
//! Only settings that change the numbers are read here; how an indicator is
//! painted is not written this way. A name that could be more than one
//! indicator ("delta", "poc") is refused rather than read as the first.

use super::market::{MAX_MINUTES, MAX_ROW_TICKS, MIN_MINUTES};
use super::{bollinger, flow, profile, split_query, Indicator, Kind, Params, Reset};

/// The settings [`parse`] reads, by flag.
pub const SETTINGS: &[&str] = &[
    "period",
    "deviations",
    "fast",
    "slow",
    "signal",
    "k-smooth",
    "d-period",
    "anchor",
    "bands",
    "rows",
    "value-area",
    "tpo-minutes",
    "row-ticks",
    "profile-method",
    "rth-threshold",
    "eth-threshold",
    "combine",
];

/// The indicator `text` names, with the lengths and settings it gives.
///
/// The name is matched whole, ignoring case, against each indicator's key,
/// short name, full name and then its aliases; one that matches more than
/// one indicator at the same level is an error. Each `--flag value` is
/// checked as the `chart indicator` command checks it.
pub fn parse(text: &str) -> Result<Indicator, String> {
    let text = text.trim();
    let (query, flags) = match text.find("--") {
        Some(at) => (text[..at].trim(), &text[at..]),
        None => (text, ""),
    };
    let (name, numbers) = split_query(query);
    let kind = kind_named(name)?;
    let mut indicator = Indicator::numbered(1, kind, &numbers).ok_or_else(|| {
        format!(
            "{} does not take {}",
            kind.short_name(),
            numbers.iter().map(f64::to_string).collect::<Vec<_>>().join(" ")
        )
    })?;

    let mut tokens = flags.split_whitespace();
    while let Some(token) = tokens.next() {
        let flag = token
            .strip_prefix("--")
            .ok_or_else(|| format!("{token:?} is not a setting; settings are written --name value"))?;
        if !SETTINGS.contains(&flag) {
            return Err(format!("--{flag} is not a setting that changes what an indicator computes; try {}", SETTINGS.join(", ")));
        }
        let value = tokens.next().ok_or_else(|| format!("--{flag} needs a value"))?;
        set(&mut indicator, flag, value)?;
    }
    // The quick average has to stay quicker than the slow one, whichever
    // of the two was given.
    if let Params::Macd { fast, slow, .. } = indicator.params
        && fast >= slow
    {
        return Err(format!("--fast {fast} is not below --slow {slow}"));
    }
    Ok(indicator)
}

/// The one indicator `name` names: see [`parse`].
pub fn kind_named(name: &str) -> Result<Kind, String> {
    let wanted = name.trim().to_lowercase();
    if wanted.is_empty() {
        return Err("no indicator named".into());
    }
    let own = |kind: &Kind| {
        [kind.key().to_string(), kind.key().replace('_', " "), kind.short_name().to_lowercase(), kind.name().to_lowercase()]
            .contains(&wanted)
    };
    let alias = |kind: &Kind| kind.aliases().contains(&wanted.as_str());
    for level in [&own as &dyn Fn(&Kind) -> bool, &alias] {
        let found: Vec<Kind> = Kind::ALL.into_iter().filter(|kind| level(kind)).collect();
        match found.as_slice() {
            [one] => return Ok(*one),
            [] => continue,
            many => {
                return Err(format!(
                    "{name:?} could be {}; name one",
                    many.iter().map(|kind| kind.key()).collect::<Vec<_>>().join(" or ")
                ))
            }
        }
    }
    Err(format!("{name:?} is not an indicator; try {}", Kind::ALL.map(Kind::key).join(", ")))
}

fn set(indicator: &mut Indicator, flag: &str, value: &str) -> Result<(), String> {
    let kind = indicator.kind;
    let refuse = |what: &str| format!("{} has no {what}", kind.short_name());
    let bars = || match value.parse::<usize>() {
        Ok(n) if n >= 1 => Ok(n),
        _ => Err(format!("--{flag} takes a whole number of bars, one or more, not {value:?}")),
    };
    let share = || match value.parse::<f64>() {
        Ok(n) if (0.0..=1.0).contains(&n) => Ok(n),
        _ => Err(format!("--{flag} takes a share from 0 to 1, not {value:?}")),
    };
    let threshold = || match value.parse::<u64>() {
        Ok(n) if n <= flow::MAX_THRESHOLD => Ok(n),
        _ => Err(format!("--{flag} takes a whole number of contracts from 0 to {}, not {value:?}", flow::MAX_THRESHOLD)),
    };
    let switch = || match value {
        "on" => Ok(true),
        "off" => Ok(false),
        _ => Err(format!("--{flag} is on or off, not {value:?}")),
    };

    match (flag, &mut indicator.params) {
        (
            "period",
            Params::MovingAverage { period }
            | Params::Rsi { period, .. }
            | Params::Atr { period, .. }
            | Params::Bollinger { period, .. }
            | Params::Stochastic { period, .. },
        ) => *period = bars()?,
        ("period", _) => return Err(refuse("period")),
        ("deviations", Params::Bollinger { bands, .. }) => {
            let deviations = value
                .parse::<f64>()
                .ok()
                .filter(|n| (0.0..=bollinger::MAX_DEVIATIONS).contains(n))
                .ok_or_else(|| format!("--deviations takes 0 to {}, not {value:?}", bollinger::MAX_DEVIATIONS))?;
            if let Some(band) = bands.first_mut() {
                // Adding nought makes a negative nought the positive one.
                band.deviations = deviations + 0.0;
            }
        }
        ("deviations", Params::Vwap { bands, .. }) => {
            return Err(format!(
                "--deviations sets the one band a Bollinger has; {} has {} of them",
                kind.short_name(),
                bands.len()
            ))
        }
        ("deviations", _) => return Err(refuse("deviations")),
        ("fast", Params::Macd { fast, .. }) => *fast = bars()?,
        ("slow", Params::Macd { slow, .. }) => *slow = bars()?,
        ("fast", _) => return Err(refuse("fast average")),
        ("slow", _) => return Err(refuse("slow average")),
        ("signal", Params::Macd { signal, .. }) => *signal = bars()?,
        ("signal", _) => return Err(refuse("signal line")),
        ("k-smooth", Params::Stochastic { k_smooth, .. }) => *k_smooth = bars()?,
        ("k-smooth", _) => return Err(refuse("%K smoothing")),
        ("d-period", Params::Stochastic { d_period, .. }) => *d_period = bars()?,
        ("d-period", _) => return Err(refuse("%D smoothing")),
        ("anchor", params) => {
            let anchor = match value {
                "never" => None,
                key => Some(Reset::from_key(key).ok_or_else(|| format!("{key:?} is not an anchor; try session or week"))?),
            };
            match (params, anchor) {
                (Params::Cvd { reset, .. }, anchor) => *reset = anchor,
                (
                    Params::Vwap { reset, .. } | Params::VolumeProfile { reset, .. } | Params::MarketProfile { reset, .. },
                    Some(anchor),
                ) => *reset = anchor,
                (Params::Vwap { .. } | Params::VolumeProfile { .. } | Params::MarketProfile { .. }, None) => {
                    return Err(format!("{} always resets; never is a cumulative delta's", kind.short_name()))
                }
                _ => return Err(refuse("anchor")),
            }
        }
        ("bands", Params::Vwap { bands, .. } | Params::Bollinger { bands, .. }) => {
            let wanted: Vec<usize> = match value {
                "none" => Vec::new(),
                list => list
                    .split(',')
                    .map(|part| part.trim().parse::<usize>().ok().filter(|n| (1..=3).contains(n)).map(|n| n - 1))
                    .collect::<Option<Vec<usize>>>()
                    .ok_or_else(|| format!("{list:?} is not a band list; try 1,2 or none"))?,
            };
            if let Some(past) = wanted.iter().find(|at| **at >= bands.len()) {
                return Err(format!(
                    "{} has {} band{}, so {} is not one of them",
                    kind.short_name(),
                    bands.len(),
                    if bands.len() == 1 { "" } else { "s" },
                    past + 1
                ));
            }
            for (at, band) in bands.iter_mut().enumerate() {
                band.enabled = wanted.contains(&at);
            }
        }
        ("bands", _) => return Err(refuse("bands")),
        ("rows", Params::VolumeProfile { rows, .. }) => {
            *rows = match value {
                "auto" => None,
                n => Some(n.parse::<usize>().map_err(|_| format!("{n:?} is not a row count; use a number or `auto`"))?),
            }
        }
        ("rows", _) => return Err(refuse("rows")),
        ("value-area", Params::VolumeProfile { value_area, .. } | Params::MarketProfile { value_area, .. }) => {
            *value_area = share()?
        }
        ("value-area", _) => return Err(refuse("value area")),
        ("tpo-minutes", Params::MarketProfile { minutes, .. }) => {
            *minutes = value.parse::<u32>().ok().filter(|n| (MIN_MINUTES..=MAX_MINUTES).contains(n)).ok_or_else(|| {
                format!("--tpo-minutes takes minutes from {MIN_MINUTES} to {MAX_MINUTES}, not {value:?}")
            })?
        }
        ("tpo-minutes", _) => return Err(refuse("TPO period")),
        ("row-ticks", Params::MarketProfile { row_ticks, .. } | Params::Footprint { row_ticks, .. }) => {
            *row_ticks = match value {
                "auto" => None,
                n => Some(n.parse::<u32>().ok().filter(|n| (1..=MAX_ROW_TICKS).contains(n)).ok_or_else(|| {
                    format!("--row-ticks takes ticks from 1 to {MAX_ROW_TICKS}, or `auto`, not {n:?}")
                })?),
            }
        }
        ("row-ticks", _) => return Err(refuse("row height in ticks")),
        ("profile-method", Params::VolumeProfile { method, .. }) => {
            *method = profile::Method::from_key(value)
                .ok_or_else(|| format!("{value:?} is not a calculation method; use total or delta"))?
        }
        ("profile-method", _) => return Err(refuse("calculation method")),
        ("rth-threshold", Params::LargeOrders { regular, .. }) => *regular = threshold()?,
        ("rth-threshold", _) => return Err(refuse("RTH threshold")),
        ("eth-threshold", Params::LargeOrders { extended, .. }) => *extended = threshold()?,
        ("eth-threshold", _) => return Err(refuse("ETH threshold")),
        ("combine", Params::LargeOrders { combine, .. }) => *combine = switch()?,
        ("combine", _) => return Err(refuse("combining")),
        (other, _) => return Err(format!("--{other} is not a setting this reads")),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_line_reads_as_the_picker_reads_its_name_and_lengths() {
        assert_eq!(parse("ema 20").unwrap().label(), "EMA 20");
        assert_eq!(parse("EMA(50)").unwrap().label(), "EMA 50");
        assert_eq!(parse("bb 20 2.5").unwrap().label(), "BB 20 2.5");
        assert_eq!(parse("macd 12 26 9").unwrap().label(), "MACD 12 26 9");
        assert_eq!(parse("tpo").unwrap().kind, Kind::MarketProfile);
        assert_eq!(parse("market profile").unwrap().kind, Kind::MarketProfile);
        assert_eq!(parse("cumulative delta").unwrap().kind, Kind::Cvd);
        assert_eq!(parse("volume delta").unwrap().kind, Kind::VolumeDelta);
        assert_eq!(parse("volume_profile").unwrap().kind, Kind::VolumeProfile);
        assert_eq!(parse("vwap").unwrap(), Indicator::new(1, Kind::Vwap));
    }

    #[test]
    fn a_name_that_could_be_several_indicators_is_refused() {
        for name in ["delta", "poc", "ma", "bands", "order flow"] {
            let refused = parse(name).unwrap_err();
            assert!(refused.contains("could be"), "{name}: {refused}");
        }
        assert!(parse("zigzag").unwrap_err().contains("not an indicator"));
    }

    #[test]
    fn settings_change_what_it_computes() {
        let Params::Vwap { reset, bands } = parse("vwap --anchor week --bands 1,3").unwrap().params else { panic!() };
        assert_eq!(reset, Reset::Week);
        assert_eq!(bands.iter().map(|band| (band.deviations, band.enabled)).collect::<Vec<_>>(), [(1.0, true), (1.5, false), (2.0, true)]);

        let Params::MarketProfile { minutes, row_ticks, value_area, reset, .. } =
            parse("tpo --tpo-minutes 15 --row-ticks 4 --value-area 0.8").unwrap().params
        else {
            panic!()
        };
        assert_eq!((minutes, row_ticks, value_area, reset), (15, Some(4), 0.8, Reset::Session));

        let Params::Cvd { reset, .. } = parse("cvd --anchor never").unwrap().params else { panic!() };
        assert_eq!(reset, None);

        let Params::LargeOrders { regular, extended, combine, .. } =
            parse("large orders --rth-threshold 100 --eth-threshold 30 --combine off").unwrap().params
        else {
            panic!()
        };
        assert_eq!((regular, extended, combine), (100, 30, false));
        assert_eq!(parse("sma --period 200").unwrap().label(), "SMA 200");
        assert_eq!(parse("macd --fast 30 --slow 40").unwrap().label(), "MACD 30 40 9");
    }

    #[test]
    fn a_setting_it_cannot_take_is_refused() {
        for (text, said) in [
            ("ema 20 --anchor week", "no anchor"),
            ("vwap --anchor never", "always resets"),
            ("vwap --deviations 2", "one band a Bollinger has"),
            ("bb 20 --bands 2", "1 band"),
            ("macd --fast 30", "not below"),
            ("macd --fast 30 --slow 30", "not below"),
            ("tpo --tpo-minutes 0", "from 1"),
            ("vwap --color Blue", "not a setting"),
            ("vwap --anchor", "needs a value"),
            ("vwap anchor week", "not an indicator"),
            ("sma 2.5", "does not take"),
        ] {
            let refused = parse(text).unwrap_err();
            assert!(refused.contains(said), "{text}: {refused}");
        }
    }
}
