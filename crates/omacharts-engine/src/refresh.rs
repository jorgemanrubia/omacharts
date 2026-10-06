//! When a chart left open is worth fetching again.
//!
//! A chart is a photograph until something refetches it. Open one at the bell
//! and look back at it after lunch and it is still showing the bell, with
//! nothing on screen to say so — which is the one way a charting application
//! can be actively misleading rather than merely incomplete.
//!
//! The provider is nobody's friend here. Yahoo's data is delayed and its rate
//! limiting is aggressive enough that a single unlucky burst can leave an
//! address refused for every subsequent call, so "refresh" cannot mean
//! "poll". It has to mean asking for the least that keeps a chart honest, and
//! not asking at all whenever the answer is knowable in advance. That second
//! half is most of this module: a chart whose market is shut, whose window is
//! not on screen, or that the user has panned back into history has nothing
//! to gain from a request, and a request with nothing to gain is pure cost.
//!
//! Everything here is a pure function of a chart's state and the clock, so
//! the decision can be tested without a window, a provider or a database —
//! which is the only way the awkward cases (Sunday evening, a future at three
//! in the morning, the minute after the close) ever get exercised at all.

use crate::bars::Timeframe;
use crate::provider::Delivery;
use crate::session;
use crate::symbols::Instrument;

/// The shortest period between two fetches of the same chart.
///
/// One minute, because that is the finest bar Yahoo serves — see the
/// provider's capability table. Below a minute there is no new data to
/// arrive, only the same bytes again, so a faster timer buys nothing and
/// spends the rate limit that the rest of the application is sharing.
pub const FLOOR_SECONDS: i64 = 60;

/// The longest.
///
/// Fifteen minutes, which is two independent arguments arriving at one
/// number. It is the worst delay the provider admits to — ten minutes for
/// futures, fifteen for indexes — so a chart fetched more often than this is
/// chasing data the feed does not have yet. And it is already what this
/// application calls stale elsewhere: the bar widget refetches a watchlist
/// quote once its daily bars are older than this, and a chart holding itself
/// to a different number would mean the window and the bar disagreeing about
/// the same question.
pub const CEILING_SECONDS: i64 = 15 * 60;

/// How long after the last print a chart goes on fetching itself.
///
/// The bar that was forming when the market shut is published in pieces like
/// any other, so stopping dead on the close would leave a part-built bar as
/// the last thing on the chart until somebody reopened it. One ceiling's
/// worth of grace is enough for every resolution to come back exactly once
/// more, which is all it takes to turn that bar into the real one.
const GRACE_SECONDS: i64 = CEILING_SECONDS;

/// How often a chart showing `timeframe` is worth fetching again.
///
/// One fetch per bar on screen, bounded at both ends. Tying the period to the
/// bar is the whole idea: the thing a refetch can actually deliver is the next
/// bar, so asking more often than one bar's worth returns a forming bar that
/// has barely moved, and asking less often means watching bars appear late.
/// A thirty-second timer on a daily chart is pointless and a fifteen-minute
/// one on a two-minute chart is useless, and both fall out of this rather
/// than having to be argued about separately.
///
/// The resolution on screen, not the one fetched. A four-hour chart is folded
/// from hourly bars, but what the person is watching is the four-hour bar, and
/// in any case everything from a quarter of an hour upwards lands on the
/// ceiling.
pub fn period(timeframe: Timeframe) -> i64 {
    timeframe.seconds().clamp(FLOOR_SECONDS, CEILING_SECONDS)
}

/// One chart on screen, and everything about it that decides whether fetching
/// it again right now would achieve anything.
///
/// Gathered into a struct rather than taken as six arguments because every
/// field is a reason to refuse, and a caller that forgets one does not get a
/// slightly worse cadence — it gets requests going out for a market that
/// closed on Friday.
pub struct Candidate<'a> {
    /// How the provider keeps a chart current. A provider that streams has
    /// nothing to be asked for, and nothing below is consulted. There is no
    /// setting beside it: whether a chart refreshes is the provider's to
    /// say, and nobody else's.
    pub delivery: Delivery,
    /// Whether the window is on screen at all. A minimised window, or one on
    /// a workspace nobody is looking at, is charting for an audience of
    /// nobody.
    pub visible: bool,
    /// False once the user has panned back into history. A chart showing last
    /// March does not change when today's bar does, and somebody reading
    /// history is the person least served by a fetch — the reply would arrive
    /// and repaint under them.
    pub at_latest: bool,
    /// The resolution on screen, which is what sets the period.
    pub timeframe: Timeframe,
    /// What is being charted, which is what says whether its market is open.
    pub instrument: &'a Instrument,
    /// When this series was last fetched, in unix seconds, as the cache
    /// recorded it. `None` when there is nothing cached at all.
    ///
    /// The cache's own timestamp rather than a note kept by the timer,
    /// because it is the truth about when data last arrived however it
    /// arrived — so opening a chart does not immediately refetch it, and two
    /// panes on the same series share one period instead of taking turns.
    pub fetched_at: Option<i64>,
}

/// Why a chart is not being fetched again.
///
/// A reason rather than a bare `false`, following the provider boundary's
/// lead: the cases are genuinely different, a test that can only assert "no"
/// cannot tell a closed market from a chart that was fetched a moment ago,
/// and the two have very different consequences if the logic is wrong.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Held {
    /// The provider delivers new bars itself, so there is nothing to fetch.
    Streamed,
    /// The window is not on screen.
    Hidden,
    /// The user has panned back into history.
    Panned,
    /// Nothing is cached, so the chart's own first load is still what it is
    /// waiting for.
    Unloaded,
    /// Nothing is trading, so there is no bar to go and get.
    Closed,
    /// Fetched recently enough that another one would return the same bars.
    Fresh,
}

/// Is this chart worth fetching again at `now`?
///
/// The order of the refusals is the order of how decisive they are rather
/// than how cheap they are to check: a chart nobody can see is not worth
/// reasoning about further, and the common steady state — a chart fetched
/// four minutes ago — is the last thing asked, so that every more
/// interesting reason gets reported in preference to it.
pub fn due(candidate: &Candidate, now: i64) -> Result<(), Held> {
    if candidate.delivery == Delivery::Streamed {
        return Err(Held::Streamed);
    }
    if !candidate.visible {
        return Err(Held::Hidden);
    }
    if !candidate.at_latest {
        return Err(Held::Panned);
    }
    // Nothing cached means either that the chart's own foreground load is
    // still in flight, or that it failed. Neither is a timer's business: the
    // first is already coming and the second would be a retry loop against a
    // provider that has just said no.
    let Some(fetched_at) = candidate.fetched_at else {
        return Err(Held::Unloaded);
    };
    if !trading_lately(candidate.instrument, now) {
        return Err(Held::Closed);
    }
    if now - fetched_at < period(candidate.timeframe) {
        return Err(Held::Fresh);
    }
    Ok(())
}

/// Is this instrument trading, or has it only just stopped?
///
/// The grace is what collects the bar that was forming at the close. Asking
/// the same question twice, once about the recent past, is cheaper than
/// teaching every market's calendar about its own closing edge.
fn trading_lately(instrument: &Instrument, now: i64) -> bool {
    session::is_trading(instrument, now) || session::is_trading(instrument, now - GRACE_SECONDS)
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;
    use chrono_tz::America::New_York;

    use super::*;
    use crate::symbols::InstrumentKind;

    fn instrument(kind: InstrumentKind) -> Instrument {
        Instrument {
            symbol: "X".into(),
            name: "X".into(),
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

    /// A moment in New York. March 2024 runs Wednesday the 13th, Friday the
    /// 15th, Saturday the 16th, Sunday the 17th.
    fn ny(day: u32, hour: u32, minute: u32) -> i64 {
        New_York
            .with_ymd_and_hms(2024, 3, day, hour, minute, 0)
            .single()
            .unwrap()
            .timestamp()
    }

    /// Midday on a Wednesday: the cash session, every market open.
    fn midweek() -> i64 {
        ny(13, 12, 0)
    }

    fn tf(key: &str) -> Timeframe {
        Timeframe::parse(key).expect(key)
    }

    fn share() -> Instrument {
        instrument(InstrumentKind::Equity)
    }

    /// A chart fetched exactly `ago` seconds before `now`, with every other
    /// reason to refuse removed, so each test puts back only the one it is
    /// about.
    fn charted<'a>(
        instrument: &'a Instrument,
        timeframe: Timeframe,
        ago: i64,
        now: i64,
    ) -> Candidate<'a> {
        Candidate {
            delivery: Delivery::Polled,
            visible: true,
            at_latest: true,
            timeframe,
            instrument,
            fetched_at: Some(now - ago),
        }
    }

    // -- the cadence -------------------------------------------------------

    /// The whole point of tying the period to the bar: the two ends of the
    /// strip get very different timers, and neither is a number somebody
    /// picked.
    #[test]
    fn the_period_follows_the_bar_on_screen() {
        assert_eq!(period(tf("2m")), 120, "a two-minute chart, every two minutes");
        assert_eq!(period(tf("5m")), 300);
        assert_eq!(period(tf("15m")), 900);
    }

    /// Below a minute there is no finer bar to be had, so a faster timer
    /// fetches the same bytes again and spends the rate limit doing it.
    #[test]
    fn nothing_is_fetched_more_often_than_the_finest_bar_that_exists() {
        assert_eq!(period(tf("1m")), FLOOR_SECONDS);
        assert_eq!(period(tf("1m")), 60);
    }

    /// And the top of the strip is held to the ceiling rather than to its own
    /// bar, because a daily chart refetched once a day would show today's
    /// price as of this morning.
    #[test]
    fn the_long_resolutions_all_land_on_the_ceiling() {
        for key in ["30m", "1h", "4h", "1D", "1W"] {
            assert_eq!(period(tf(key)), CEILING_SECONDS, "{key}");
        }
        assert_eq!(CEILING_SECONDS, 15 * 60);
    }

    /// Stated as a test because it is the complaint this feature exists to
    /// answer, and because the two halves of it are easy to get wrong in
    /// opposite directions.
    #[test]
    fn a_daily_chart_is_not_polled_every_thirty_seconds() {
        let now = midweek();
        let share = share();

        let daily = charted(&share, tf("1D"), 30, now);
        assert_eq!(due(&daily, now), Err(Held::Fresh), "thirty seconds is nothing on a daily");

        let fast = charted(&share, tf("2m"), 3 * 60, now);
        assert_eq!(due(&fast, now), Ok(()), "but three minutes is a missed bar on a 2m");
    }

    #[test]
    fn a_chart_comes_due_once_its_period_has_passed() {
        let now = midweek();
        let share = share();
        assert_eq!(due(&charted(&share, tf("5m"), 299, now), now), Err(Held::Fresh));
        assert_eq!(due(&charted(&share, tf("5m"), 300, now), now), Ok(()));
    }

    /// Opening a chart fetches it; the timer must not immediately fetch it
    /// again because its own clock has just started.
    #[test]
    fn a_chart_just_opened_is_not_due() {
        let now = midweek();
        let share = share();
        assert_eq!(due(&charted(&share, tf("1D"), 0, now), now), Err(Held::Fresh));
    }

    // -- the refusals ------------------------------------------------------

    /// Nothing a person can set holds a chart back. A chart on a polled
    /// provider that is on screen, at its live edge, trading and overdue is
    /// fetched, and there is no field on the candidate through which a
    /// preference could say otherwise.
    #[test]
    fn a_polled_provider_is_refreshed_without_anybody_asking_for_it() {
        let now = midweek();
        let share = share();
        let chart =
            Candidate { delivery: Delivery::Polled, ..charted(&share, tf("1D"), 86_400, now) };
        assert_eq!(due(&chart, now), Ok(()));
    }

    /// A provider that pushes bars is the one case with nothing to go and
    /// get, and it is the provider's word that says so.
    #[test]
    fn a_streamed_provider_is_never_polled() {
        let now = midweek();
        let share = share();
        let pushed =
            Candidate { delivery: Delivery::Streamed, ..charted(&share, tf("1D"), 86_400, now) };
        assert_eq!(due(&pushed, now), Err(Held::Streamed), "and nothing else is consulted");
    }

    #[test]
    fn a_window_nobody_can_see_fetches_nothing() {
        let now = midweek();
        let share = share();
        let hidden = Candidate { visible: false, ..charted(&share, tf("1D"), 86_400, now) };
        assert_eq!(due(&hidden, now), Err(Held::Hidden));
    }

    /// Somebody reading history is the person least served by a fetch: the
    /// reply would arrive and repaint the chart under them.
    #[test]
    fn a_chart_panned_back_into_history_fetches_nothing() {
        let now = midweek();
        let share = share();
        let panned = Candidate { at_latest: false, ..charted(&share, tf("1D"), 86_400, now) };
        assert_eq!(due(&panned, now), Err(Held::Panned));
    }

    /// A timer is not the place to retry a load that failed, nor the place to
    /// race one that has not landed yet.
    #[test]
    fn a_chart_with_nothing_cached_waits_for_its_own_load() {
        let now = midweek();
        let share = share();
        let loading = Candidate { fetched_at: None, ..charted(&share, tf("1D"), 0, now) };
        assert_eq!(due(&loading, now), Err(Held::Unloaded));
    }

    // -- the market being shut ---------------------------------------------

    /// The case that saves the most requests: a chart left open on Friday
    /// evening has two whole days in which nothing it could ask for exists.
    #[test]
    fn a_chart_whose_market_is_shut_for_the_weekend_asks_for_nothing() {
        let share = share();
        for (day, what) in [(16, "Saturday"), (17, "Sunday")] {
            let now = ny(day, 12, 0);
            let chart = charted(&share, tf("1D"), 86_400, now);
            assert_eq!(due(&chart, now), Err(Held::Closed), "{what}");
        }
    }

    #[test]
    fn a_share_overnight_asks_for_nothing() {
        let now = ny(13, 2, 0);
        let share = share();
        let chart = charted(&share, tf("5m"), 3_600, now);
        assert_eq!(due(&chart, now), Err(Held::Closed));
    }

    /// The pre-market is the reason the window is the extended one. Somebody
    /// with a chart open at eight in the morning is watching it move.
    #[test]
    fn a_share_in_the_pre_market_is_fetched() {
        let now = ny(13, 8, 0);
        let share = share();
        let chart = charted(&share, tf("5m"), 3_600, now);
        assert_eq!(due(&chart, now), Ok(()));
    }

    /// Stopping dead on the bell would leave the bar that was forming at the
    /// close as a part-built bar until somebody reopened the chart.
    #[test]
    fn a_market_that_has_just_shut_is_collected_once_more() {
        let future = instrument(InstrumentKind::FutureRoot);

        // Friday's futures close is 17:00; ten minutes past it the last bar
        // is still being published.
        let just_after = ny(15, 17, 10);
        let chart = charted(&future, tf("15m"), CEILING_SECONDS, just_after);
        assert_eq!(due(&chart, just_after), Ok(()), "one last fetch, to complete the bar");

        // An hour and a half later there is nothing left to collect.
        let long_after = ny(15, 18, 30);
        let chart = charted(&future, tf("15m"), CEILING_SECONDS, long_after);
        assert_eq!(due(&chart, long_after), Err(Held::Closed));
    }

    /// Overnight is exactly when somebody has an index future open, so the
    /// one instrument that trades while everybody is asleep must not be held
    /// to the cash market's day.
    #[test]
    fn a_future_is_fetched_through_the_night() {
        let now = ny(13, 3, 0);
        let future = instrument(InstrumentKind::FutureRoot);
        assert_eq!(due(&charted(&future, tf("15m"), 3_600, now), now), Ok(()));

        // And a share at the same moment is not.
        let share = share();
        assert_eq!(
            due(&charted(&share, tf("15m"), 3_600, now), now),
            Err(Held::Closed),
            "the same hour, a market that is shut"
        );
    }

    #[test]
    fn crypto_is_fetched_at_three_on_a_sunday_morning() {
        let now = ny(17, 3, 0);
        let coin = instrument(InstrumentKind::Crypto);
        assert_eq!(due(&charted(&coin, tf("15m"), 3_600, now), now), Ok(()));
    }

    /// Even a market that never shuts is held to the period. "Always open" is
    /// not "always fetch".
    #[test]
    fn a_market_that_never_closes_still_waits_its_turn() {
        let now = ny(17, 3, 0);
        let coin = instrument(InstrumentKind::Crypto);
        assert_eq!(due(&charted(&coin, tf("1D"), 60, now), now), Err(Held::Fresh));
    }

    /// The cadence and the market calendar are separate refusals, and a chart
    /// can be long overdue and still have nothing to ask for.
    #[test]
    fn being_overdue_is_not_a_reason_to_ask_a_shut_market() {
        let now = ny(16, 12, 0);
        let share = share();
        let chart = charted(&share, tf("1m"), 7 * 86_400, now);
        assert_eq!(due(&chart, now), Err(Held::Closed), "a week stale, and still no");
    }
}
