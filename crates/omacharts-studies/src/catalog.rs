//! Every indicator there is, and the form it can be computed in.
//!
//! A list rather than something worked out, so that adding an indicator
//! somewhere is a change here too: a test in the engine holds every
//! indicator the chart offers to an entry, and anything built on these
//! studies can walk this list to make sure it offers all of them.

/// How an indicator can be computed one bar at a time.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Form {
    /// A [`crate::Study`] in this crate, under the same key.
    Stepped,
    /// Its value at a bar depends on bars after it — a whole period's
    /// profile is only whole once the period is over — so it is drawn over
    /// a span and has no study. What can be known so far is a different
    /// indicator, and gets its own entry when it exists.
    WholePeriod,
    /// Not arithmetic on bars at all: which stretches of the day are which,
    /// which whoever supplies the bars already knows.
    Calendar,
}

/// Every indicator, by key, and its form.
pub const INDICATORS: &[(&str, Form)] = &[
    ("sma", Form::Stepped),
    ("ema", Form::Stepped),
    ("vwap", Form::Stepped),
    ("bollinger", Form::Stepped),
    ("macd", Form::Stepped),
    ("volume_profile", Form::WholePeriod),
    // What it is so far, a bar at a time, is `developing_market_profile`,
    // in [`DEVELOPING`].
    ("market_profile", Form::WholePeriod),
    ("volume", Form::Stepped),
    // Estimated from each bar's direction. Read from volume at price where
    // there is some: that reading is `volume_delta`, in [`AT_PRICE`].
    ("cvd", Form::Stepped),
    ("rsi", Form::Stepped),
    ("atr", Form::Stepped),
    ("stochastic", Form::Stepped),
    ("session_times", Form::Calendar),
];

/// Studies of volume at price, by key: [`crate::AtPriceStudy`]s, for bars
/// that know what traded at each price and on which side. Each reads what
/// one of the indicators above draws from ladders — the cumulative delta,
/// the volume profile so far — and none of them estimates.
pub const AT_PRICE: &[&str] = &["volume_delta", "developing_profile"];

/// Studies of what a whole-period indicator above could be known to be at
/// each bar, a bar at a time, from bars alone: `(study, indicator)`, each a
/// [`crate::Study`] under its own key. At the last bar of a period each
/// reads what the indicator draws for the whole of it.
pub const DEVELOPING: &[(&str, &str)] = &[("developing_market_profile", "market_profile")];

/// The form of the indicator `key`, if there is one by that name.
pub fn form(key: &str) -> Option<Form> {
    INDICATORS.iter().find(|(name, _)| *name == key).map(|(_, form)| *form)
}
