//! Thinkorswim charts.
//!
//! Candles come from [`tos_market`]. The session is the account's own feed,
//! so this claims no delay. A live gateway is never opened.

pub mod session;

use crate::bars::{Bar, Timeframe, Unit};
use crate::provider::{Capability, Provider, ProviderError};
use crate::symbols::{Instrument, InstrumentKind};

const CAPABILITIES: &[Capability] = &[
    Capability {
        timeframe: Timeframe::minutes(1),
        history_days: Some(20),
        max_request_days: Some(20),
    },
    Capability {
        timeframe: Timeframe::minutes(2),
        history_days: Some(20),
        max_request_days: Some(20),
    },
    Capability {
        timeframe: Timeframe::minutes(5),
        history_days: Some(20),
        max_request_days: Some(20),
    },
    Capability {
        timeframe: Timeframe::minutes(15),
        history_days: Some(20),
        max_request_days: Some(20),
    },
    Capability {
        timeframe: Timeframe::minutes(30),
        history_days: Some(20),
        max_request_days: Some(20),
    },
    Capability {
        timeframe: Timeframe::minutes(90),
        history_days: Some(20),
        max_request_days: Some(20),
    },
    Capability {
        timeframe: Timeframe::hours(1),
        history_days: Some(20),
        max_request_days: Some(20),
    },
    Capability {
        timeframe: Timeframe::days(1),
        history_days: None,
        max_request_days: None,
    },
];

/// What to ask the gateway, and how to fold the answer when it has no code
/// of its own for that resolution. Intraday history is about twenty sessions
/// (`DAY20`). Ninety-minute bars are folded from the half-hour series.
#[derive(Debug)]
struct Requested {
    aggregation: &'static str,
    daily: bool,
    /// Fold into buckets of this many seconds. `None` keeps the gateway's bars.
    bucket: Option<i64>,
}

/// How thinkorswim is offered.
///
/// "Your own" is the point of it: these are the account's own charts, which
/// is why there is a sign-in to do. What it does *not* say is how fresh they
/// are — that is [`crate::providers::freshness`]'s to answer, from what this
/// provider reports, because the answer changes the day a chart is fed by
/// the gateway's subscription instead of a snapshot on a timer.
pub const LISTED: crate::providers::Listed = crate::providers::Listed {
    id: "tos",
    label: "thinkorswim",
    serves: "Your own Schwab paperMoney account, US listings",
    setup: Some(&session::SETUP),
};

pub struct Tos;

impl Default for Tos {
    fn default() -> Tos {
        Tos
    }
}

impl Tos {
    pub fn new() -> Tos {
        Tos
    }
}

/// This instrument in thinkorswim's spelling, if the session can chart it.
pub fn tos_symbol(instrument: &Instrument) -> Option<String> {
    if let Some(explicit) = instrument.override_for("tos") {
        return Some(explicit.to_string());
    }
    Some(match instrument.kind {
        InstrumentKind::FutureRoot => format!("/{}", instrument.symbol),
        InstrumentKind::Index => index_symbol(&instrument.symbol)?.to_string(),
        InstrumentKind::Equity | InstrumentKind::Etf if instrument.suffix.is_none() => {
            instrument.symbol.replace('.', "/")
        }
        InstrumentKind::Equity
        | InstrumentKind::Etf
        | InstrumentKind::Fx
        | InstrumentKind::Crypto => {
            return None;
        }
    })
}

/// Names the session actually charts. The `$` is not consistent:
/// `$DJI` and `$DXY` need it, `SPX` and `VIX` reject it. Anything else is
/// left unmapped rather than guessed.
fn index_symbol(symbol: &str) -> Option<&'static str> {
    Some(match symbol {
        "GSPC" => "SPX",
        "NDX" => "NDX",
        "IXIC" => "COMP",
        "DJI" => "$DJI",
        "RUT" => "RUT",
        "VIX" => "VIX",
        "NYA" => "NYA",
        "SOX" => "SOX",
        "DXY" => "$DXY",
        _ => return None,
    })
}

fn requested(timeframe: Timeframe) -> Result<Requested, ProviderError> {
    let (aggregation, daily, bucket) = match (timeframe.unit, timeframe.count) {
        (Unit::Minute, 1) => ("MIN1", false, None),
        (Unit::Minute, 2) => ("MIN2", false, None),
        (Unit::Minute, 5) => ("MIN5", false, None),
        (Unit::Minute, 15) => ("MIN15", false, None),
        (Unit::Minute, 30) => ("MIN30", false, None),
        (Unit::Minute, 90) => ("MIN30", false, Some(90 * 60)),
        (Unit::Hour, 1) => ("HOUR1", false, None),
        (Unit::Day, 1) => ("DAY", true, None),
        _ => {
            return Err(ProviderError::Unsupported(format!(
                "{} is folded, not fetched",
                timeframe.label()
            )));
        }
    };
    Ok(Requested {
        aggregation,
        daily,
        bucket,
    })
}

/// Smallest platform range that covers `since`. `None` asks for the longest
/// window this feed requests in one shot.
fn range_code(daily: bool, since: Option<i64>, now: i64) -> &'static str {
    let span_days = since
        .map(|since| (now.saturating_sub(since) / 86_400).saturating_add(1))
        .unwrap_or(i64::MAX);
    if daily {
        match span_days {
            d if d <= 180 => "MONTH6",
            d if d <= 365 => "YEAR1",
            d if d <= 365 * 2 => "YEAR2",
            d if d <= 365 * 5 => "YEAR5",
            d if d <= 365 * 10 => "YEAR10",
            _ => "YEAR20",
        }
    } else {
        match span_days {
            d if d <= 1 => "DAY1",
            d if d <= 5 => "DAY5",
            d if d <= 10 => "DAY10",
            _ => "DAY20",
        }
    }
}

fn fold(mut bars: Vec<Bar>, bucket: i64) -> Vec<Bar> {
    if bucket <= 0 || bars.is_empty() {
        return bars;
    }
    bars.sort_by_key(|bar| bar.ts);
    let mut out: Vec<Bar> = Vec::with_capacity(bars.len());
    for bar in bars {
        let start = bar.ts.div_euclid(bucket) * bucket;
        match out.last_mut() {
            Some(last) if last.ts == start => {
                last.high = last.high.max(bar.high);
                last.low = last.low.min(bar.low);
                last.close = bar.close;
                last.volume += bar.volume;
            }
            _ => out.push(Bar { ts: start, ..bar }),
        }
    }
    out
}

/// What a chart says when this feed has nobody signed in to it.
///
/// One sentence, and it is the same one the settings panel and the CLI use,
/// because somebody reading it on a chart and then going to look will
/// otherwise wonder whether they found the right place.
pub const NOT_SIGNED_IN: &str = "thinkorswim is not signed in";

fn map_err(error: tos_market::Error) -> ProviderError {
    use tos_market::Error;

    match error {
        // Nothing saved, or what is saved is a live-trading session this feed
        // will not connect to. Both are the same thing to a chart: there is
        // no usable session, and one place to go and get one.
        Error::NoSession(_) | Error::LiveTradingDisabled | Error::Config(_) => {
            ProviderError::NeedsSetup(NOT_SIGNED_IN.into())
        }
        // The gateway had its say and refused the token. Also setup, not
        // network: nothing retried on a timer will make an expired session
        // work, and the chart must say so rather than claim the provider is
        // having a bad minute.
        Error::Login(why) => ProviderError::NeedsSetup(format!(
            "the thinkorswim session has expired ({why}); sign in again"
        )),
        // A gateway complaining about the symbol is the one error worth
        // telling apart from a transport failure, because the answer is a
        // different ticker rather than waiting.
        Error::Gateway { ref id, ref message, .. }
            if names_a_symbol(id) || names_a_symbol(message) =>
        {
            ProviderError::NotFound
        }
        other => ProviderError::Network(other.to_string()),
    }
}

fn names_a_symbol(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    lower.contains("symbol") || lower.contains("not found") || lower.contains("unknown")
}

impl Provider for Tos {
    fn id(&self) -> &'static str {
        "tos"
    }

    fn label(&self) -> &'static str {
        "thinkorswim"
    }

    /// The session's chart feed is the account's own. Claim no delay.
    fn delay_minutes(&self, _kind: InstrumentKind) -> u32 {
        0
    }

    fn symbol_for(&self, instrument: &Instrument) -> Option<String> {
        tos_symbol(instrument)
    }

    fn capabilities(&self) -> &'static [Capability] {
        CAPABILITIES
    }

    fn bars(
        &self,
        symbol: &str,
        timeframe: Timeframe,
        since: Option<i64>,
    ) -> Result<Vec<Bar>, ProviderError> {
        let requested = requested(timeframe)?;
        let range = range_code(requested.daily, since, chrono::Utc::now().timestamp());
        let candles = tos_market::candles(symbol, requested.aggregation, range).map_err(map_err)?;
        let mut bars: Vec<Bar> = candles
            .into_iter()
            .map(|c| Bar {
                ts: c.ts,
                open: c.open,
                high: c.high,
                low: c.low,
                close: c.close,
                volume: c.volume,
            })
            .collect();
        if let Some(bucket) = requested.bucket {
            bars = fold(bars, bucket);
        }
        if let Some(since) = since {
            bars.retain(|bar| bar.ts >= since);
        }
        Ok(bars)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn instrument(kind: InstrumentKind, symbol: &str) -> Instrument {
        Instrument {
            symbol: symbol.into(),
            name: String::new(),
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

    #[test]
    fn futures_indexes_and_class_shares_use_thinkorswim_names() {
        let tos = Tos::new();
        assert_eq!(
            tos.symbol_for(&instrument(InstrumentKind::FutureRoot, "ES"))
                .as_deref(),
            Some("/ES")
        );
        assert_eq!(
            tos.symbol_for(&instrument(InstrumentKind::FutureRoot, "6E"))
                .as_deref(),
            Some("/6E")
        );
        assert_eq!(
            tos.symbol_for(&instrument(InstrumentKind::Index, "GSPC"))
                .as_deref(),
            Some("SPX")
        );
        assert_eq!(
            tos.symbol_for(&instrument(InstrumentKind::Index, "DJI"))
                .as_deref(),
            Some("$DJI")
        );
        assert_eq!(
            tos.symbol_for(&instrument(InstrumentKind::Index, "VIX"))
                .as_deref(),
            Some("VIX")
        );
        assert_eq!(
            tos.symbol_for(&instrument(InstrumentKind::Equity, "BRK.B"))
                .as_deref(),
            Some("BRK/B")
        );
        assert_eq!(
            tos.symbol_for(&instrument(InstrumentKind::Equity, "AAPL"))
                .as_deref(),
            Some("AAPL")
        );
    }

    #[test]
    fn listings_abroad_and_spot_fx_are_not_guessed() {
        let mut sap = instrument(InstrumentKind::Equity, "SAP");
        sap.suffix = Some("DE".into());
        assert!(Tos::new().symbol_for(&sap).is_none());
        assert!(
            Tos::new()
                .symbol_for(&instrument(InstrumentKind::Fx, "EURUSD"))
                .is_none()
        );
        assert!(
            Tos::new()
                .symbol_for(&instrument(InstrumentKind::Index, "N225"))
                .is_none()
        );
    }

    #[test]
    fn an_explicit_override_wins() {
        let mut es = instrument(InstrumentKind::FutureRoot, "ES");
        es.overrides.push(("tos".into(), "/MES".into()));
        assert_eq!(Tos::new().symbol_for(&es).as_deref(), Some("/MES"));
    }

    #[test]
    fn a_tail_uses_a_short_range_and_a_first_fetch_uses_the_long_one() {
        let now = 1_700_000_000;
        assert_eq!(range_code(false, Some(now - 3_600), now), "DAY1");
        assert_eq!(range_code(false, None, now), "DAY20");
        assert_eq!(range_code(true, Some(now - 86_400 * 40), now), "MONTH6");
        assert_eq!(range_code(true, None, now), "YEAR20");
    }

    #[test]
    fn half_hours_fold_into_the_hour_they_belong_to() {
        let bar = |ts, close| Bar {
            ts,
            open: close,
            high: close,
            low: close,
            close,
            volume: 1.0,
        };
        let folded = fold(
            vec![bar(3_600, 10.0), bar(5_400, 12.0), bar(7_200, 8.0)],
            3_600,
        );
        assert_eq!(folded.len(), 2);
        assert_eq!(folded[0].ts, 3_600);
        assert_eq!(folded[0].close, 12.0);
        assert_eq!(folded[0].volume, 2.0);
        assert_eq!(folded[1].ts, 7_200);
    }

    /// Nothing here calls [`Provider::bars`]. A test that did would reach a
    /// brokerage from whatever machine ran it — and on a machine with a real
    /// session, succeed at it. The mapping is what has to be right, and the
    /// mapping can be asked directly.
    #[test]
    fn nothing_signed_in_is_a_failure_with_something_to_do_about_it() {
        for error in [
            tos_market::Error::NoSession("/x/tos.env".into()),
            // A saved live-trading session is refused, and refusing it is not
            // a network problem to wait out.
            tos_market::Error::LiveTradingDisabled,
        ] {
            let mapped = map_err(error);
            assert!(
                matches!(&mapped, ProviderError::NeedsSetup(what) if what == NOT_SIGNED_IN),
                "{mapped}"
            );
            assert_eq!(
                crate::provider::FetchFailure::from(&mapped),
                crate::provider::FetchFailure::NeedsSignIn
            );
        }
    }

    /// An expired session is the same kind of thing: no amount of retrying
    /// makes a refused token work, so the chart must not say "not answering".
    #[test]
    fn an_expired_session_says_to_sign_in_again() {
        let mapped = map_err(tos_market::Error::Login("session expired".into()));
        let ProviderError::NeedsSetup(what) = &mapped else {
            panic!("{mapped}");
        };
        assert!(what.contains("sign in again"), "{what}");
    }

    #[test]
    fn a_gateway_that_does_not_know_the_ticker_is_a_missing_symbol() {
        let mapped = map_err(tos_market::Error::Gateway {
            service: "chart".into(),
            id: "bad_symbol".into(),
            message: "unknown symbol ZZZZ".into(),
        });
        assert!(matches!(mapped, ProviderError::NotFound), "{mapped}");
    }

    /// Everything else is transport, and transport is worth waiting out.
    #[test]
    fn a_dropped_connection_is_still_a_network_failure() {
        let mapped = map_err(tos_market::Error::ConnectionLost { sent: true });
        assert!(matches!(mapped, ProviderError::Network(_)), "{mapped}");
    }

    #[test]
    fn four_hours_is_not_fetched_as_itself() {
        let error = requested(Timeframe::hours(4)).unwrap_err();
        assert!(matches!(error, ProviderError::Unsupported(_)), "{error}");
    }
}
