//! Regular and extended trading hours.
//!
//! A free feed hands back every bar it has, including the thin overnight ones.
//! Those matter sometimes and ruin a chart the rest of the time: a handful of
//! trades at 3am stretch the price scale and leave the session everyone
//! actually traded squashed into a corner.
//!
//! So the chart can ask for regular hours only. What "regular" means is the
//! instrument's exchange's to say — see [`crate::exchanges`]: New York's cash
//! session for a US listing, the cash session too for the index futures that
//! track it, the old pit hours for crude or gold, Taipei's day for the TWSE.
//! Instruments that keep hours nobody wrote down, or no split at all, are
//! left alone, because filtering them would only throw data away.

use serde::{Deserialize, Serialize};

use crate::bars::Bar;
use crate::exchanges::registry;
use crate::symbols::Instrument;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Session {
    /// Everything the provider sends, overnight included.
    #[default]
    Extended,
    /// The cash session only.
    Regular,
}

impl Session {
    pub const ALL: [Session; 2] = [Session::Extended, Session::Regular];

    pub fn label(self) -> &'static str {
        match self {
            Session::Extended => "Extended hours",
            Session::Regular => "Regular hours only",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Session::Extended => "extended",
            Session::Regular => "regular",
        }
    }

    pub fn from_key(key: &str) -> Option<Session> {
        Session::ALL.into_iter().find(|s| s.key() == key)
    }
}

/// Does restricting to regular hours mean anything for this instrument?
pub fn has_regular_hours(instrument: &Instrument) -> bool {
    registry().for_instrument(instrument).is_some_and(|s| s.has_regular())
}

/// Keep only the bars inside the regular session.
///
/// A no-op for daily and coarser bars — one bar already is a session — and for
/// instruments with no regular session to speak of.
pub fn filter(bars: &[Bar], session: Session, instrument: &Instrument, intraday: bool) -> Vec<Bar> {
    if session == Session::Extended || !intraday {
        return bars.to_vec();
    }
    match registry().for_instrument(instrument) {
        Some(schedule) if schedule.has_regular() => {
            bars.iter().copied().filter(|bar| schedule.is_regular(bar.ts)).collect()
        }
        _ => bars.to_vec(),
    }
}

/// Could another bar still arrive for this instrument at `ts`?
///
/// The question a chart left open has to answer before fetching itself again.
/// Asking a provider for bars that cannot exist spends a rate limit on a
/// guaranteed empty reply, and a watchlist of forty symbols asking it all
/// weekend is how an address gets throttled for the Monday open.
///
/// Generous at every edge on purpose. Being wrong in this direction costs one
/// request that comes back with nothing; being wrong in the other leaves a
/// chart quietly stale while the thing it charts is moving, which is the only
/// failure here anybody would notice. There is no holiday calendar, so a
/// public holiday reads as an ordinary weekday and costs a handful of empty
/// replies — the same trade, taken knowingly.
pub fn is_trading(instrument: &Instrument, ts: i64) -> bool {
    // FX and crypto are asked about whenever: a free feed's currency pairs
    // print odd bars through the weekend, and a quiet market costs one empty
    // reply. And an instrument whose exchange is not known keeps hours
    // nothing here can rule on — guessing would mean refusing to refresh a
    // listing right through its own session, which is worse than not asking.
    if instrument.kind.is_continuous() {
        return true;
    }
    match registry().for_instrument(instrument) {
        Some(schedule) => schedule.is_open(ts),
        None => true,
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;
    use chrono_tz::America::New_York;
    use chrono_tz::Asia::{Taipei, Tokyo};

    use super::*;
    use crate::symbols::InstrumentKind;

    /// Is this instant inside the New York cash session on a weekday?
    fn in_regular_hours(ts: i64) -> bool {
        registry().for_instrument(&instrument(InstrumentKind::Equity, None)).unwrap().is_regular(ts)
    }

    fn instrument(kind: InstrumentKind, suffix: Option<&str>) -> Instrument {
        Instrument {
            symbol: "X".into(),
            name: "X".into(),
            kind,
            suffix: suffix.map(str::to_string),
            currency: None,
            tier: 0,
            session_origin: 0,
            overrides: Vec::new(),
            exchange: None,
            popularity: 0,
            local_name: None,
        }
    }

    /// The same instrument, quoted somewhere in particular.
    fn priced_in(kind: InstrumentKind, currency: &str) -> Instrument {
        Instrument { currency: Some(currency.into()), ..instrument(kind, None) }
    }

    fn bar(ts: i64) -> Bar {
        Bar { ts, open: 1.0, high: 1.0, low: 1.0, close: 1.0, volume: 1.0 }
    }

    /// 2024-03-13 was a Wednesday, in US daylight time (UTC-4).
    fn wednesday_at(hour: u32, minute: u32) -> i64 {
        New_York
            .with_ymd_and_hms(2024, 3, 13, hour, minute, 0)
            .single()
            .unwrap()
            .timestamp()
    }

    /// 2024-01-10, a Wednesday in standard time (UTC-5).
    fn winter_wednesday_at(hour: u32, minute: u32) -> i64 {
        New_York
            .with_ymd_and_hms(2024, 1, 10, hour, minute, 0)
            .single()
            .unwrap()
            .timestamp()
    }

    #[test]
    fn the_cash_session_runs_from_the_open_to_the_close() {
        assert!(!in_regular_hours(wednesday_at(9, 29)), "before the bell");
        assert!(in_regular_hours(wednesday_at(9, 30)), "the open");
        assert!(in_regular_hours(wednesday_at(12, 0)));
        assert!(in_regular_hours(wednesday_at(15, 59)));
        assert!(!in_regular_hours(wednesday_at(16, 0)), "the close belongs to the next session");
        assert!(!in_regular_hours(wednesday_at(3, 0)), "overnight");
    }

    #[test]
    fn the_session_keeps_its_local_hours_across_daylight_saving() {
        // Same wall-clock times, different UTC offsets.
        assert!(in_regular_hours(winter_wednesday_at(9, 30)));
        assert!(!in_regular_hours(winter_wednesday_at(9, 29)));
        assert!(in_regular_hours(winter_wednesday_at(15, 59)));
        // And the two dates really are on different offsets.
        assert_ne!(
            wednesday_at(9, 30) % 86_400,
            winter_wednesday_at(9, 30) % 86_400,
            "the fixture dates should straddle the change"
        );
    }

    #[test]
    fn weekends_are_not_the_cash_session() {
        let saturday = New_York.with_ymd_and_hms(2024, 3, 16, 12, 0, 0).single().unwrap();
        assert!(!in_regular_hours(saturday.timestamp()));
    }

    #[test]
    fn filtering_keeps_only_the_session() {
        let bars = vec![
            bar(wednesday_at(4, 0)),
            bar(wednesday_at(9, 30)),
            bar(wednesday_at(12, 0)),
            bar(wednesday_at(18, 0)),
        ];
        let stock = instrument(InstrumentKind::Equity, None);
        let kept = filter(&bars, Session::Regular, &stock, true);
        assert_eq!(kept.len(), 2);
        assert_eq!(kept[0].ts, wednesday_at(9, 30));
    }

    #[test]
    fn extended_hours_keeps_everything() {
        let bars: Vec<Bar> = (0..24).map(|h| bar(wednesday_at(h, 0))).collect();
        let stock = instrument(InstrumentKind::Equity, None);
        assert_eq!(filter(&bars, Session::Extended, &stock, true).len(), bars.len());
    }

    #[test]
    fn daily_bars_are_never_filtered() {
        // One daily bar already is a session; dropping it for being stamped
        // outside 09:30 would empty the chart.
        let bars = vec![bar(wednesday_at(0, 0)), bar(wednesday_at(0, 0) + 86_400)];
        let stock = instrument(InstrumentKind::Equity, None);
        assert_eq!(filter(&bars, Session::Regular, &stock, false).len(), 2);
    }

    #[test]
    fn instruments_without_a_cash_session_are_left_alone() {
        let bars: Vec<Bar> = (0..24).map(|h| bar(wednesday_at(h, 0))).collect();
        for kind in [InstrumentKind::Fx, InstrumentKind::Crypto] {
            let it = instrument(kind, None);
            assert!(!has_regular_hours(&it));
            assert_eq!(filter(&bars, Session::Regular, &it, true).len(), bars.len());
        }
        // Nor does a listing on an exchange nobody wrote the hours of.
        let unknown = instrument(InstrumentKind::Equity, Some("ZZ"));
        assert!(!has_regular_hours(&unknown));
        assert_eq!(filter(&bars, Session::Regular, &unknown, true).len(), bars.len());
    }

    #[test]
    fn an_index_quoted_abroad_keeps_its_own_hours() {
        // The Nikkei's whole session is overnight in New York, so holding it
        // to the cash session there would leave nothing on the chart at all.
        // Its currency alone does not say where it is priced; its exchange
        // does.
        let nikkei = priced_in(InstrumentKind::Index, "JPY");
        assert!(!has_regular_hours(&nikkei));
        let bars: Vec<Bar> = (9..15)
            .map(|h| bar(Tokyo.with_ymd_and_hms(2024, 3, 13, h, 0, 0).single().unwrap().timestamp()))
            .collect();
        assert!(!bars.iter().any(|b| in_regular_hours(b.ts)), "the fixture should be overnight");
        assert_eq!(filter(&bars, Session::Regular, &nikkei, true).len(), bars.len());

        let nikkei = Instrument { exchange: Some("TSE".into()), ..nikkei };
        assert!(has_regular_hours(&nikkei));
        let kept: Vec<i64> = filter(&bars, Session::Regular, &nikkei, true).iter().map(|b| b.ts).collect();
        assert_eq!(kept.len(), 5, "09:00 to 15:00 on the hour, less the lunch break: {kept:?}");
    }

    /// A listing abroad keeps its own exchange's hours, in its own clock.
    #[test]
    fn a_madrid_listing_keeps_madrid_hours() {
        use chrono_tz::Europe::Madrid;
        let santander = instrument(InstrumentKind::Equity, Some("MC"));
        assert!(has_regular_hours(&santander));
        let at = |h| Madrid.with_ymd_and_hms(2024, 3, 13, h, 0, 0).single().unwrap().timestamp();
        let bars: Vec<Bar> = (7..20).map(|h| bar(at(h))).collect();
        let kept: Vec<i64> = filter(&bars, Session::Regular, &santander, true).iter().map(|b| b.ts).collect();
        assert_eq!(kept.first(), Some(&at(9)));
        assert_eq!(kept.last(), Some(&at(17)));
        assert!(is_trading(&santander, at(10)));
        assert!(!is_trading(&santander, at(20)));
    }

    #[test]
    fn a_dollar_index_still_has_a_cash_session() {
        // The S&P really is on New York hours, and so is a ticker typed into
        // the search field with no listing behind it to say otherwise.
        assert!(has_regular_hours(&priced_in(InstrumentKind::Index, "USD")));
        assert!(has_regular_hours(&instrument(InstrumentKind::Equity, None)));

        let bars = vec![bar(wednesday_at(4, 0)), bar(wednesday_at(12, 0))];
        let spx = priced_in(InstrumentKind::Index, "USD");
        assert_eq!(filter(&bars, Session::Regular, &spx, true).len(), 1);
    }

    /// A moment in New York, for the days of the week the futures session
    /// cares about. March 2024 runs Friday the 15th, Saturday the 16th,
    /// Sunday the 17th, Monday the 18th.
    fn ny(day: u32, hour: u32, minute: u32) -> i64 {
        New_York
            .with_ymd_and_hms(2024, 3, day, hour, minute, 0)
            .single()
            .unwrap()
            .timestamp()
    }

    /// The window a chart refetches inside is the extended one, not the cash
    /// session: somebody with a chart open before the bell is watching the
    /// pre-market, and telling them nothing is coming would be wrong.
    #[test]
    fn a_share_is_trading_through_the_pre_and_post_market() {
        let stock = instrument(InstrumentKind::Equity, None);
        assert!(!is_trading(&stock, ny(13, 3, 59)), "before the pre-market");
        assert!(is_trading(&stock, ny(13, 4, 0)), "the pre-market opens");
        assert!(is_trading(&stock, ny(13, 9, 0)), "an hour before the bell");
        assert!(is_trading(&stock, ny(13, 12, 0)), "the cash session");
        assert!(is_trading(&stock, ny(13, 18, 0)), "after the bell, still printing");
        assert!(!is_trading(&stock, ny(13, 20, 0)), "the post-market closes");
        assert!(!is_trading(&stock, ny(13, 2, 0)), "the middle of the night");
    }

    /// The case that saves the most requests by far: a chart left open over a
    /// weekend has two days in which nothing it could ask for exists.
    #[test]
    fn nothing_listed_is_trading_at_the_weekend() {
        let stock = instrument(InstrumentKind::Equity, None);
        assert!(!is_trading(&stock, ny(16, 12, 0)), "Saturday");
        assert!(!is_trading(&stock, ny(17, 12, 0)), "Sunday");
    }

    /// Futures keep their own week, and it is most of one. Holding them to the
    /// cash market's hours would switch refreshing off exactly when somebody
    /// watches an index future: overnight.
    #[test]
    fn a_future_trades_overnight_and_almost_all_week() {
        let future = instrument(InstrumentKind::FutureRoot, None);
        assert!(is_trading(&future, ny(13, 3, 0)), "three in the morning, and printing");
        assert!(is_trading(&future, ny(13, 12, 0)), "the middle of the day");
        assert!(is_trading(&future, ny(13, 23, 0)), "and late at night");

        // The hour each session breaks for.
        assert!(!is_trading(&future, ny(13, 17, 30)), "the daily break");
        assert!(is_trading(&future, ny(13, 18, 0)), "the next session opens");

        // The two ends of the week.
        assert!(is_trading(&future, ny(15, 16, 0)), "Friday afternoon");
        assert!(!is_trading(&future, ny(15, 17, 30)), "Friday's close is the week's");
        assert!(!is_trading(&future, ny(16, 12, 0)), "Saturday, like everything else");
        assert!(!is_trading(&future, ny(17, 12, 0)), "Sunday lunchtime, still shut");
        assert!(is_trading(&future, ny(17, 18, 0)), "Sunday evening, and the week restarts");
    }

    /// A market with no close has no moment at which asking is pointless.
    #[test]
    fn what_never_closes_is_always_trading() {
        for kind in [InstrumentKind::Crypto, InstrumentKind::Fx] {
            let it = instrument(kind, None);
            assert!(is_trading(&it, ny(17, 3, 0)), "{kind:?} at 3am on a Sunday");
            assert!(is_trading(&it, ny(13, 12, 0)), "{kind:?} midweek");
        }
    }

    /// Hours nobody wrote down are hours this must not rule on. Reading
    /// "closed" off New York's clock would leave a listing abroad stale all
    /// through its own day.
    #[test]
    fn a_listing_whose_hours_we_do_not_know_is_treated_as_trading() {
        let unknown = instrument(InstrumentKind::Equity, Some("ZZ"));
        assert!(is_trading(&unknown, ny(13, 4, 0)));
        assert!(is_trading(&unknown, ny(13, 22, 0)), "and at a time we cannot rule out");

        let nikkei = priced_in(InstrumentKind::Index, "JPY");
        assert!(is_trading(&nikkei, ny(13, 2, 0)));
    }

    /// The cash session and the window a chart refetches in are not the same
    /// window, and nothing should quietly collapse them into one.
    #[test]
    fn trading_is_a_wider_window_than_the_cash_session() {
        let stock = instrument(InstrumentKind::Equity, None);
        let before_the_bell = ny(13, 8, 0);
        assert!(!in_regular_hours(before_the_bell), "not a regular-hours bar");
        assert!(is_trading(&stock, before_the_bell), "but another one is coming");
    }

    /// A moment in Taipei. October 2026 runs Friday the 2nd, Saturday the
    /// 3rd, Sunday the 4th, Monday the 5th.
    fn taipei(day: u32, hour: u32, minute: u32) -> i64 {
        Taipei.with_ymd_and_hms(2026, 10, day, hour, minute, 0).single().unwrap().timestamp()
    }

    #[test]
    fn a_taiwan_listing_keeps_taipei_hours() {
        for suffix in ["TW", "TWO"] {
            let tsmc = Instrument {
                currency: Some("TWD".into()),
                ..instrument(InstrumentKind::Equity, Some(suffix))
            };
            assert!(has_regular_hours(&tsmc), ".{suffix} has a cash session");
            let bars: Vec<Bar> = [(8, 59), (9, 0), (11, 0), (13, 25), (13, 30), (13, 31), (14, 0)]
                .iter()
                .map(|(h, m)| bar(taipei(5, *h, *m)))
                .collect();
            let kept: Vec<i64> =
                filter(&bars, Session::Regular, &tsmc, true).iter().map(|b| b.ts).collect();
            assert_eq!(
                kept,
                vec![taipei(5, 9, 0), taipei(5, 11, 0), taipei(5, 13, 25), taipei(5, 13, 30)],
                "09:00 through the 13:30 closing auction, and nothing either side"
            );
        }
    }

    /// The Taipei session is the middle of New York's night. Reading it off
    /// New York's clock, the way every suffixed listing used to be treated,
    /// would either call it shut all day or never call it shut at all.
    #[test]
    fn a_taiwan_listing_is_trading_only_through_its_own_day() {
        let tsmc = instrument(InstrumentKind::Equity, Some("TW"));
        assert!(!is_trading(&tsmc, taipei(5, 8, 29)), "before orders are taken");
        assert!(is_trading(&tsmc, taipei(5, 8, 30)), "the pre-open");
        assert!(is_trading(&tsmc, taipei(5, 10, 0)), "the session");
        assert!(is_trading(&tsmc, taipei(5, 14, 15)), "the after-hours fixed-price session");
        assert!(!is_trading(&tsmc, taipei(5, 14, 30)), "everything has closed");
        assert!(!is_trading(&tsmc, taipei(5, 22, 30)), "the US open is the Taipei night");
        assert!(!is_trading(&tsmc, taipei(3, 10, 0)), "Saturday");
        assert!(!is_trading(&tsmc, taipei(4, 10, 0)), "Sunday");
        assert!(is_trading(&tsmc, taipei(2, 10, 0)), "Friday");
    }

    #[test]
    fn the_taiex_keeps_taipei_hours_too() {
        // An index never carries a suffix, so its currency is what says where
        // it is priced.
        let taiex = priced_in(InstrumentKind::Index, "TWD");
        assert!(has_regular_hours(&taiex));
        assert!(is_trading(&taiex, taipei(5, 10, 0)));
        assert!(!is_trading(&taiex, taipei(5, 22, 0)));
        let tokyo = instrument(InstrumentKind::Equity, Some("T"));
        assert!(!is_trading(&tokyo, taipei(5, 22, 0)), "Tokyo has closed by its own clock");
        assert!(is_trading(&tokyo, taipei(5, 10, 0)), "and is open at eleven in Tokyo");
    }

    #[test]
    fn keys_round_trip() {
        for session in Session::ALL {
            assert_eq!(Session::from_key(session.key()), Some(session));
        }
        assert_eq!(Session::from_key("nonsense"), None);
    }
}
