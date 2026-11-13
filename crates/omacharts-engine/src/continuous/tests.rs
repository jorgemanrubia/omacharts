use super::*;
use crate::indicators::vwap;
use crate::indicators::Reset;
use crate::ladders::to_tick;
use super::fake::*;

/// The month the rule has front at `ts`, by the spans' own cut.
fn front_at(ts: i64, timeframe: Timeframe) -> usize {
    let mut front = MONTHS.len() - 1;
    for (index, (code, year)) in MONTHS.iter().enumerate().skip(1).rev() {
        let starts = contracts::rule_of("ES").roll(previous(&contracts::rule_of("ES"), Month::of(*code, *year).unwrap())).unwrap();
        if ts < cut("ES", starts, timeframe) {
            front = index - 1;
        }
    }
    front
}

fn es() -> Instrument {
    root_instrument("ES")
}

fn spelled(adjustment: Adjustment) -> String {
    Spelled { root: "ES".into(), front: month(4), adjustment }.to_string()
}

/// The fake stitched, and the fake, to ask what it was asked.
struct Feed {
    stitched: Continuous,
    months: Arc<Months>,
}

impl std::ops::Deref for Feed {
    type Target = Continuous;
    fn deref(&self) -> &Continuous {
        &self.stitched
    }
}

impl Feed {
    fn asked_for(&self, symbol: &str) -> bool {
        self.months.asked_for(symbol)
    }
    fn asked(&self) -> std::sync::MutexGuard<'_, Vec<(String, Timeframe, Option<i64>)>> {
        self.months.asked.lock().unwrap()
    }
}

fn continuous(months: Months) -> Feed {
    let months = Arc::new(months);
    Feed { stitched: Continuous::shared(months.clone()), months }
}

/// Every bar once, in order, and every one at the price of the month front
/// then: no bar is in two months and none falls between them.
fn assert_drawn_from_the_front(bars: &[Bar], timeframe: Timeframe, offset: impl Fn(usize) -> f64) {
    assert!(bars.windows(2).all(|pair| pair[0].ts < pair[1].ts), "a bar twice or out of order");
    for bar in bars {
        let front = front_at(bar.ts, timeframe);
        let expected = price(front, bar.ts) + offset(front);
        assert!((bar.close - expected).abs() < 1e-9, "{} drawn as {} not {expected} (front {front})", bar.ts, bar.close);
    }
}

#[test]
fn a_root_is_spelled_by_its_front_month_and_a_dated_contract_is_itself() {
    let feed = continuous(Months::all());
    assert_eq!(feed.symbol_for(&es()).as_deref(), Some("ES~ESZ26~back-adjusted"));
    let dated = es().in_month(month(3)).unwrap();
    assert_eq!(feed.symbol_for(&dated).as_deref(), Some("ESU6"));
    assert_eq!(feed.charted_contract(&es()).as_deref(), Some("ESZ6"), "the month whose prices are drawn now");
    assert_eq!(Spelled::parse("ES~ESZ26~as-traded").unwrap().adjustment, Adjustment::AsTraded);
    assert_eq!(Spelled::parse("ES~NQZ26~as-traded"), None, "another root's month");
    assert_eq!(Spelled::parse("ESZ6"), None);
    assert_eq!(shown("ES~ESZ26~as-traded"), "ES");
}

#[test]
fn a_dated_contract_is_charted_from_its_own_month_alone() {
    let feed = continuous(Months::all());
    let bars = feed.bars("ESU26", Timeframe::days(1), None).unwrap();
    assert_eq!(bars, series(3, Timeframe::days(1), None));
    assert_eq!(feed.asked().len(), 1, "one month asked, nothing stitched");
}

#[test]
fn daily_bars_change_month_at_each_roll_with_none_twice_and_none_missing() {
    let feed = continuous(Months::all());
    let daily = Timeframe::days(1);
    let bars = feed.bars(&spelled(Adjustment::AsTraded), daily, None).unwrap();
    assert_drawn_from_the_front(&bars, daily, |_| 0.0);
    // From the day the oldest month became front, every weekday.
    let starts = contracts::rule_of("ES").roll(previous(&contracts::rule_of("ES"), month(0))).unwrap();
    let weekdays = series(0, daily, Some(midnight(starts)));
    assert_eq!(bars.iter().map(|bar| bar.ts).collect::<Vec<_>>(), weekdays.iter().map(|bar| bar.ts).collect::<Vec<_>>());
}

#[test]
fn hourly_bars_change_month_where_the_roll_s_trading_day_starts() {
    let feed = continuous(Months::all());
    let hourly = Timeframe::hours(1);
    let bars = feed.bars(&spelled(Adjustment::AsTraded), hourly, None).unwrap();
    assert_drawn_from_the_front(&bars, hourly, |_| 0.0);
    // The first bar of September's span is at 17:00 Chicago the evening
    // before its roll date.
    let roll = contracts::rule_of("ES").roll(month(2)).unwrap();
    let at = cut("ES", roll, hourly);
    let wall = crate::exchanges::wall(chrono_tz::America::Chicago, at).unwrap();
    assert_eq!((wall.0, wall.1), (roll.pred_opt().unwrap(), 17 * 60));
    let before = bars.iter().rev().find(|bar| bar.ts < at).unwrap();
    let after = bars.iter().find(|bar| bar.ts >= at).unwrap();
    assert_eq!((at - before.ts, after.ts - at), (3_600, 0), "no hour lost or doubled at the roll");
    assert!((after.close - before.close - PREMIUM).abs() < 0.1, "as traded, the roll is the gap");
}

#[test]
fn back_adjusted_moves_each_month_by_the_gaps_after_it_and_leaves_the_front_alone() {
    let feed = continuous(Months::all());
    for timeframe in [Timeframe::days(1), Timeframe::hours(1)] {
        let adjusted = feed.bars(&spelled(Adjustment::BackAdjusted), timeframe, None).unwrap();
        let traded = feed.bars(&spelled(Adjustment::AsTraded), timeframe, None).unwrap();
        // Four rolls of ten points each: the oldest month moved by forty,
        // the next by thirty, the front by nothing.
        assert_drawn_from_the_front(&adjusted, timeframe, |front| PREMIUM * (4 - front) as f64);
        assert_eq!(adjusted.len(), traded.len());
        let front_starts = cut("ES", contracts::rule_of("ES").roll(month(3)).unwrap(), timeframe);
        for (a, t) in adjusted.iter().zip(&traded) {
            assert_eq!(a.volume, t.volume, "volume is never adjusted");
            if a.ts >= front_starts {
                assert_eq!(a, t, "the front month is its real prices in both");
            }
        }
        assert_eq!(adjusted.last().unwrap().close, series(4, timeframe, None).last().unwrap().close, "the last price is real");
    }
}

#[test]
fn the_choice_of_adjustment_is_the_symbol_asked_for() {
    let feed = continuous(Months::all());
    assert!(offers_choice(&*feed, &es()));
    assert!(!offers_choice(&*feed, &es().in_month(month(4)).unwrap()), "a dated chart has nothing to choose");
    let back = feed.symbol_for(&es()).unwrap();
    let traded = routed(&*feed, &es(), &back, Adjustment::AsTraded);
    assert_eq!(traded, "ES~ESZ26~as-traded");
    assert_eq!(routed(&*feed, &es(), &traded, Adjustment::BackAdjusted), back);
    assert_eq!(routed(&*feed, &es(), "ESZ6", Adjustment::AsTraded), "ESZ6");
    let daily = Timeframe::days(1);
    let (b, t) = (feed.bars(&back, daily, None).unwrap(), feed.bars(&traded, daily, None).unwrap());
    assert!((b[0].close - t[0].close - 4.0 * PREMIUM).abs() < 1e-9, "the oldest month, moved and not");
}

#[test]
fn a_month_the_feed_lacks_is_drawn_from_the_month_after_it_and_said() {
    let feed = continuous(Months::without(&[1]));
    let daily = Timeframe::days(1);
    let bars = feed.bars(&spelled(Adjustment::AsTraded), daily, None).unwrap();
    let missing = |front: usize| if front == 1 { 2 } else { front };
    assert!(bars.windows(2).all(|pair| pair[0].ts < pair[1].ts));
    for bar in &bars {
        assert!((bar.close - price(missing(front_at(bar.ts, daily)), bar.ts)).abs() < 1e-9, "{}", bar.ts);
    }
    assert!(bars.iter().any(|bar| front_at(bar.ts, daily) == 0), "the month before the missing one is still drawn");
    let said = crate::log::global().entries();
    assert!(said.iter().any(|entry| entry.line().contains("no ESH26, so ESM26 is drawn there")), "not said");

    // Back-adjusted, the month before the missing one is moved by the gap
    // to the month standing in, at the missing month's start.
    let adjusted = feed.bars(&spelled(Adjustment::BackAdjusted), daily, None).unwrap();
    let oldest = adjusted.first().unwrap();
    assert!((oldest.close - price(4, oldest.ts)).abs() < 1e-9, "every month in line with the front");
}

#[test]
fn a_feed_with_only_its_front_month_charts_that() {
    let feed = continuous(Months::without(&[0, 1, 2, 3]));
    let bars = feed.bars(&spelled(Adjustment::BackAdjusted), Timeframe::days(1), None).unwrap();
    let starts = contracts::rule_of("ES").roll(month(3)).unwrap();
    assert_eq!(bars.first().unwrap().ts, midnight(starts), "its own span, not its thin history as a back month");
    assert!(feed.bars(&spelled(Adjustment::BackAdjusted), Timeframe::days(1), Some(last())).is_ok());
}

#[test]
fn only_the_months_a_request_reaches_are_read() {
    let feed = continuous(Months::all());
    let since = cut("ES", contracts::rule_of("ES").roll(month(2)).unwrap(), Timeframe::hours(1)) + 30 * 86_400;
    let bars = feed.bars(&spelled(Adjustment::BackAdjusted), Timeframe::hours(1), Some(since)).unwrap();
    assert_eq!(bars.first().unwrap().ts, since);
    for older in ["ESZ25", "ESH26", "ESM26"] {
        assert!(!feed.asked_for(older), "{older} read for a span it has none of");
    }
    let asked = feed.asked();
    let september = asked.iter().find(|(symbol, timeframe, _)| symbol == "ESU26" && *timeframe == Timeframe::hours(1)).unwrap();
    assert_eq!(september.2, Some(since), "read from where the request starts");
}

#[test]
fn a_stream_is_the_front_month_live_behind_the_months_before_it() {
    let feed = continuous(Months::all());
    let heard = Arc::new(Mutex::new(Vec::new()));
    let into = heard.clone();
    let symbol = spelled(Adjustment::BackAdjusted);
    let _live = feed
        .stream()
        .unwrap()
        .subscribe(&symbol, Timeframe::days(1), None, Box::new(move |update| into.lock().unwrap().push(update)))
        .unwrap();
    let heard = heard.lock().unwrap();
    let whole = feed.bars(&symbol, Timeframe::days(1), None).unwrap();
    let Update::Snapshot(first) = &heard[0] else { panic!("a snapshot first") };
    let roll = cut("ES", contracts::rule_of("ES").roll(month(3)).unwrap(), Timeframe::days(1));
    assert_eq!(first, &whole.iter().filter(|bar| bar.ts >= roll).copied().collect::<Vec<_>>(), "the front month at once");
    let Update::Snapshot(snapshot) = &heard[1] else { panic!("then the months before it") };
    assert_eq!(snapshot, &whole);
    let Update::Bar(bar) = &heard[2] else { panic!("then the live bar") };
    assert_eq!(bar.close, series(4, Timeframe::days(1), None).last().unwrap().close + 0.25, "live, unmoved");
    assert!(feed.asked_for("ESZ26"), "the front month is the one followed");
}

#[test]
fn ladders_are_read_from_each_span_s_month_and_moved_with_its_bars() {
    let feed = continuous(Months::all());
    let tick = 0.25;
    let roll = contracts::rule_of("ES").roll(month(2)).unwrap();
    let at = cut("ES", roll, Timeframe::minutes(1));
    let (from, to) = (at - 5 * 3_600, at + 5 * 3_600);
    for adjustment in Adjustment::ALL {
        let minutes = Ladders::minutes(&*feed, &spelled(adjustment), tick, from, to).unwrap();
        assert_eq!(minutes.len(), 10, "an hour's minute each, none twice");
        let bars = feed.bars(&spelled(adjustment), Timeframe::hours(1), Some(from)).unwrap();
        for minute in &minutes {
            let bar = bars.iter().find(|bar| bar.ts == minute.ts).unwrap();
            assert!((minute.close - bar.close).abs() < 1e-9, "{adjustment:?}: the ladder where its bar is");
            assert_eq!(minute.ladder.low_tick, to_tick(bar.close, tick), "{adjustment:?}: its prices moved by whole ticks");
            assert_eq!(minute.volume, 1, "volume is never adjusted");
        }
    }
    assert_eq!(Ladders::contract(&*feed, &spelled(Adjustment::AsTraded)), None, "kept under its own spelling");
    assert!(Ladders::serves(&*feed, &spelled(Adjustment::AsTraded)));
}

#[test]
fn a_year_s_vwap_reads_across_the_rolls_as_one_series() {
    let feed = continuous(Months::all());
    let daily = Timeframe::days(1);
    let since = midnight(day("2026-01-01"));
    let adjusted = feed.bars(&spelled(Adjustment::BackAdjusted), daily, Some(since)).unwrap();
    let out = vwap::compute(&adjusted, Reset::Year, 0, &vwap::default_bands());
    // The adjusted series is the front month's own line through the year,
    // so its VWAP is the mean of that line's typical prices.
    let front: Vec<Bar> = series(4, daily, Some(since));
    let expected = vwap::compute(&front, Reset::Year, 0, &vwap::default_bands());
    assert_eq!(adjusted.len(), front.len(), "every trading day of the year");
    for (at, (got, want)) in out.center.iter().zip(&expected.center).enumerate() {
        assert!((got.unwrap() - want.unwrap()).abs() < 1e-9, "bar {at}");
    }
}

#[test]
fn which_series_a_feed_draws_is_its_own_declaration() {
    let mut months = Months::all();
    assert_eq!(source(&months), Source::Stitched);
    months.native = &[Adjustment::AsTraded];
    assert_eq!(source(&months), Source::Stitched, "both, with ladders: stitched, so profiles match the bars");
    months.ladders = false;
    assert_eq!(source(&months), Source::Native, "both, without ladders: its own");
    months.per_contract = false;
    assert_eq!(source(&months), Source::Native);
    months.native = &[];
    assert_eq!(source(&months), Source::FrontMonth);
    assert_eq!(source(&crate::providers::Yahoo::new()), Source::Native);
}

#[test]
fn a_feed_s_own_continuous_series_is_used_as_it_is() {
    let mut months = Months::all();
    months.per_contract = false;
    months.native = &[Adjustment::AsTraded];
    let feed = wrapped(Box::new(months));
    assert_eq!(feed.symbol_for(&es()).as_deref(), Some("ES"), "not stitched");
    assert!(!offers_choice(feed.as_ref(), &es()), "one series, nothing to choose");
    assert_eq!(routed(feed.as_ref(), &es(), "ES", Adjustment::AsTraded), "ES");
    assert_eq!(feed.charted_contract(&es()).as_deref(), Some("ESZ6"), "the front month, as the feed says");

    // A feed with its own series both ways is asked for the one chosen.
    let mut both = Months::all();
    both.per_contract = false;
    both.native = &[Adjustment::BackAdjusted, Adjustment::AsTraded];
    let feed = wrapped(Box::new(both));
    assert!(offers_choice(feed.as_ref(), &es()));
    assert_eq!(routed(feed.as_ref(), &es(), "ES", Adjustment::AsTraded), "ES@as-traded");
    assert_eq!(routed(feed.as_ref(), &es(), "ES", Adjustment::BackAdjusted), "ES@back-adjusted");
}

#[test]
fn the_adjustments_read_back_as_they_are_written() {
    for adjustment in Adjustment::ALL {
        assert_eq!(Adjustment::from_key(adjustment.key()), Some(adjustment));
    }
    assert_eq!(Adjustment::default(), Adjustment::BackAdjusted);
    assert_eq!(Adjustment::from_key("sideways"), None);
}

#[test]
fn a_root_followed_live_is_heard_to_roll_once_its_feed_charts_the_next_month() {
    let feed = continuous(Months::all());
    feed.months.roll_to("ESU6");
    let symbol = feed.symbol_for(&es()).unwrap();
    assert_eq!(symbol, "ES~ESU26~back-adjusted");
    let _live = feed.stream().unwrap().subscribe(&symbol, Timeframe::days(1), None, Box::new(|_| {})).unwrap();
    let _ladders = Ladders::watch(&*feed, &symbol, 0.25, Box::new(|_| {})).unwrap();
    assert_eq!(*feed.months.followed.lock().unwrap(), ["ESU26", "ladders ESU26"], "the front month followed, bars and ladders");
    let heard = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let into = heard.clone();
    on_rolled(move || {
        into.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        true
    });
    let heard = || heard.load(std::sync::atomic::Ordering::SeqCst);
    // One that stops listening, as a closed window does, hears no more.
    let closed = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let into = closed.clone();
    on_rolled(move || {
        into.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        false
    });

    assert!(!check_rolls(), "nothing rolled");
    assert_eq!(heard(), 0);
    feed.months.roll_to("ESZ6");
    assert!(check_rolls(), "the feed charts the root as December now");
    assert_eq!(heard(), 1);
    assert!(!check_rolls(), "heard once");
    let december = feed.symbol_for(&es()).unwrap();
    assert_eq!(december, "ES~ESZ26~back-adjusted", "routed again, it is December's");
    assert_eq!(drawn_front(&symbol).as_deref(), Some("ESU26"), "what the old chart's last price is a price of");

    let _again = feed.stream().unwrap().subscribe(&december, Timeframe::days(1), None, Box::new(|_| {})).unwrap();
    feed.months.roll_to("ESH7");
    assert!(check_rolls());
    assert_eq!(heard(), 2);
    assert_eq!(closed.load(std::sync::atomic::Ordering::SeqCst), 1, "let go once it stopped listening");
}

#[test]
fn a_month_that_could_not_be_read_is_read_again_the_next_time_the_series_is_asked_for() {
    // Four-hour bars, which no other test asks for: what is stitched short
    // is known by the feed, the spelling and the timeframe.
    let feed = continuous(Months::all());
    let timeframe = Timeframe::hours(4);
    let symbol = spelled(Adjustment::AsTraded);
    *feed.months.unreadable.lock().unwrap() = vec![1];
    let short = feed.bars(&symbol, timeframe, None).unwrap();
    let said = crate::log::global().entries();
    assert!(said.iter().any(|entry| entry.line().contains("ESH26 could not be read")), "not said");

    *feed.months.unreadable.lock().unwrap() = Vec::new();
    let since = last() - 10 * 86_400;
    let again = feed.bars(&symbol, timeframe, Some(since)).unwrap();
    let whole = feed.bars(&symbol, timeframe, None).unwrap();
    assert_ne!(short, whole, "March was stood in for");
    assert_eq!(again, whole, "asked from where its cache ends, the whole series, March read this time");
    assert_drawn_from_the_front(&again, timeframe, |_| 0.0);
    let topped = feed.bars(&symbol, timeframe, Some(since)).unwrap();
    assert_eq!(topped.first().unwrap().ts, since, "once whole, topped up as before");
}

#[test]
fn a_gap_is_measured_on_daily_bars_whatever_the_timeframe_asked() {
    let feed = continuous(Months::all());
    let roll = contracts::rule_of("ES").roll(month(2)).unwrap();
    let hourly = gap(&*feed.months, "ES", month(2), month(3), roll, Timeframe::hours(1));
    let minutes = gap(&*feed.months, "ES", month(2), month(3), roll, Timeframe::minutes(1));
    assert_eq!(hourly, Gap::Measured(PREMIUM));
    assert_eq!(hourly, minutes, "a series and its ladders moved alike");
}

/// A feed of its own name, so what is known of its rolls is its own.
fn named(id: &'static str, months: Months) -> Feed {
    continuous(Months { id, ..months })
}

#[test]
fn a_roll_with_nothing_to_measure_is_joined_as_it_is_once_and_for_all() {
    // September's history starts the day it became front: June and
    // September never traded at the same time.
    let feed = named("months-late", Months { late: vec![3], ..Months::all() });
    let roll = contracts::rule_of("ES").roll(month(2)).unwrap();
    assert_eq!(gap(&*feed.months, "ES", month(2), month(3), roll, Timeframe::hours(1)), Gap::Unmeasurable);
    let daily = Timeframe::days(1);
    let symbol = spelled(Adjustment::BackAdjusted);
    let bars = feed.bars(&symbol, daily, None).unwrap();
    assert!(!drawn_short("months-late", &symbol, daily), "a stitch that is all it can be is not short");
    // Each month moved by the gaps after it but the one there is none of.
    assert_drawn_from_the_front(&bars, daily, |front| PREMIUM * ((4 - front) - usize::from(front <= 2)) as f64);
    let said = crate::log::global().entries();
    assert!(said.iter().any(|entry| entry.line().contains("no price of ESM26 and ESU26 at the same time before the roll")), "not said");

    // Its ladders are moved alike, across that roll too.
    let at = cut("ES", roll, Timeframe::minutes(1));
    let minutes = Ladders::minutes(&*feed, &symbol, 0.25, at - 5 * 3_600, at + 5 * 3_600).unwrap();
    assert_eq!(minutes.len(), 10);
}

#[test]
fn a_roll_that_could_not_be_measured_just_now_holds_back_only_the_ladders_before_it() {
    let feed = named("months-busy", Months::all());
    let roll = contracts::rule_of("ES").roll(month(3)).unwrap();
    let at = cut("ES", roll, Timeframe::minutes(1));
    let symbol = spelled(Adjustment::BackAdjusted);
    let (from, to) = (at - 5 * 3_600, at + 5 * 3_600);
    assert_eq!(roll_cuts(&symbol, from, to), [at], "split where the front month starts");
    assert_eq!(roll_cuts("ESZ26", from, to), Vec::<i64>::new());
    *feed.months.unreadable.lock().unwrap() = vec![3];
    assert!(Ladders::minutes(&*feed, &symbol, 0.25, from, at).is_err(), "September moved by a guess");
    let front = Ladders::minutes(&*feed, &symbol, 0.25, at, to).unwrap();
    assert_eq!(front.len(), 5, "December's own, whatever September's");
    *feed.months.unreadable.lock().unwrap() = Vec::new();
    assert_eq!(Ladders::minutes(&*feed, &symbol, 0.25, from, at).unwrap().len(), 5, "asked again, measured");
}

#[test]
fn what_was_drawn_short_is_known_until_it_is_drawn_whole() {
    let feed = named("months-short", Months::all());
    let daily = Timeframe::days(1);
    let symbol = spelled(Adjustment::AsTraded);
    *feed.months.unreadable.lock().unwrap() = vec![1];
    feed.bars(&symbol, daily, None).unwrap();
    assert!(drawn_short("months-short", &symbol, daily));
    assert!(!drawn_short("months-short", &symbol, Timeframe::hours(1)), "by timeframe");
    *feed.months.unreadable.lock().unwrap() = Vec::new();
    feed.bars(&symbol, daily, Some(last())).unwrap();
    assert!(!drawn_short("months-short", &symbol, daily));
}

/// A read: the contract, the timeframe, from, to when it was asked by time,
/// and how many bars it held.
type Asked = (String, Timeframe, i64, Option<i64>, usize);

/// Years of ES, every quarter's month, as a folder of a hundred contract
/// files has them: each from half a year before it is front to a week after
/// its roll, the front month from `front_from`, and each quarter ten points
/// above the one before it. What it was asked, by month and by time.
struct Years {
    front: Month,
    front_from: i64,
    read: Mutex<Vec<Asked>>,
    /// How far an hourly chart's load had got at each read, halfway
    /// through it, as a feed that measures what it reads says.
    progress: Mutex<Vec<f64>>,
}

impl Years {
    fn new(front_from: i64) -> Arc<Years> {
        Arc::new(Years { front: Month::of('Z', 2026).unwrap(), front_from, read: Mutex::new(Vec::new()), progress: Mutex::new(Vec::new()) })
    }

    fn spelled(&self) -> String {
        Spelled { root: "ES".into(), front: self.front, adjustment: Adjustment::BackAdjusted }.to_string()
    }

    fn quarter(month: Month) -> f64 {
        f64::from(month.year * 4 + month.month as i32 / 3)
    }

    /// The bars `month` holds at `timeframe`.
    fn held(&self, month: Month, timeframe: Timeframe) -> Vec<Bar> {
        let rule = contracts::rule_of("ES");
        let (Some(into), Some(out)) = (rule.roll(previous(&rule, month)), rule.roll(month)) else { return Vec::new() };
        let from = match month == self.front {
            true => self.front_from,
            false => midnight(into) - 180 * 86_400,
        };
        let to = (midnight(out) + 7 * 86_400).min(now());
        let step = match timeframe.unit {
            Unit::Day => 86_400,
            _ => timeframe.seconds(),
        };
        let first = from.div_euclid(step) * step;
        (first..to)
            .step_by(step as usize)
            .map(|ts| {
                let close = 3_000.0 + (ts / 3_600) as f64 / 24.0 / 10.0 + 10.0 * (Years::quarter(month) - 8_000.0);
                Bar { ts, open: close, high: close + 1.0, low: close - 1.0, close, volume: 1.0 }
            })
            .collect()
    }

    fn month_of(symbol: &str) -> Option<Month> {
        let (root, month) = contracts::parse(symbol, contracts::today())?;
        (root == "ES" && month <= Month::of('Z', 2026).unwrap() && month.year >= 1990).then_some(month)
    }

    /// A read says it is half done, notes how far an hourly chart's load
    /// has got, and is done.
    fn report(&self) {
        crate::progress::advance(0.5);
        if let Some(now) = crate::progress::tests::reported(&crate::progress::name(&self.spelled(), Timeframe::hours(1))) {
            self.progress.lock().unwrap().push(now);
        }
        crate::progress::advance(1.0);
    }

    /// The bars read of `symbol` at `timeframe`, however they were asked for.
    fn bars_read(&self, symbol: &str, timeframe: Timeframe) -> usize {
        self.read.lock().unwrap().iter().filter(|read| read.0 == symbol && read.1 == timeframe).map(|read| read.4).sum()
    }

    /// The months anything was read of at `timeframe`.
    fn months_read(&self, timeframe: Timeframe) -> Vec<String> {
        let mut months: Vec<String> =
            self.read.lock().unwrap().iter().filter(|read| read.1 == timeframe && read.4 > 0).map(|read| read.0.clone()).collect();
        months.sort();
        months.dedup();
        months
    }
}

impl Provider for Years {
    fn id(&self) -> &'static str {
        "years"
    }
    fn label(&self) -> &'static str {
        "Years"
    }
    fn delay_minutes(&self, _: InstrumentKind) -> u32 {
        0
    }
    fn native_symbol(&self, instrument: &Instrument) -> Option<String> {
        Some(instrument.symbol.clone())
    }
    fn charted_contract(&self, _: &Instrument) -> Option<String> {
        Some(contract("ES", self.front))
    }
    fn capabilities(&self) -> &'static [Capability] {
        &[]
    }
    fn continuity(&self) -> Continuity {
        Continuity::PER_CONTRACT
    }
    fn bars(&self, symbol: &str, timeframe: Timeframe, since: Option<i64>) -> Result<Vec<Bar>, ProviderError> {
        let month = Years::month_of(symbol).ok_or(ProviderError::NotFound)?;
        let bars: Vec<Bar> = self.held(month, timeframe).into_iter().filter(|bar| since.is_none_or(|since| bar.ts >= since)).collect();
        self.read.lock().unwrap().push((symbol.to_string(), timeframe, since.unwrap_or(i64::MIN), None, bars.len()));
        self.report();
        Ok(bars)
    }
    fn bars_between(&self, symbol: &str, timeframe: Timeframe, since: i64, until: i64) -> Result<Vec<Bar>, ProviderError> {
        let month = Years::month_of(symbol).ok_or(ProviderError::NotFound)?;
        let bars: Vec<Bar> = self.held(month, timeframe).into_iter().filter(|bar| bar.ts >= since && bar.ts < until).collect();
        self.read.lock().unwrap().push((symbol.to_string(), timeframe, since, Some(until), bars.len()));
        self.report();
        Ok(bars)
    }
    fn earliest(&self, symbol: &str) -> Option<i64> {
        let month = Years::month_of(symbol)?;
        self.held(month, Timeframe::hours(1)).first().map(|bar| bar.ts)
    }
    fn serves(&self, _: Timeframe) -> bool {
        true
    }
}

/// The months whose spans reach into the time from `since` on.
fn spans_since(front: Month, since: i64, timeframe: Timeframe) -> Vec<String> {
    let mut months = Vec::new();
    for (month, starts) in schedule("ES", front) {
        months.push(contract("ES", month));
        if cut("ES", starts, timeframe) <= since {
            break;
        }
    }
    months.sort();
    months
}

/// An hourly chart of a root with years of months on disk reads as far back
/// as its front month's own history, the months over that and no others,
/// and of each only its span: what made a hundred files of trades a wait of
/// minutes and tens of gigabytes.
#[test]
fn an_hourly_chart_reads_only_the_months_its_front_month_s_history_spans() {
    let hourly = Timeframe::hours(1);
    let front_from = now() - 200 * 86_400;
    let years = Years::new(front_from);
    let feed = Continuous::shared(years.clone());
    let started = std::time::Instant::now();
    let bars = feed.bars(&years.spelled(), hourly, None).unwrap();
    assert!(started.elapsed() < std::time::Duration::from_secs(2), "{:?}", started.elapsed());

    assert_eq!(bars.first().unwrap().ts, front_from.div_euclid(3_600) * 3_600, "as far back as the front month's history");
    assert_eq!(years.months_read(hourly), spans_since(years.front, front_from, hourly), "a month outside the window read");
    let read: usize = years.months_read(hourly).iter().map(|month| years.bars_read(month, hourly)).sum();
    assert!(read <= bars.len() + 24 * 8, "{read} bars read for {} drawn", bars.len());
    assert!(years.months_read(Timeframe::days(1)).len() <= 6, "the gaps are measured on the days around the rolls alone");
}

/// A load stitched of many months says how far it has got as it reads
/// them, never less than it said before, and all of it once it is read. A
/// load nobody measures says nothing.
#[test]
fn a_stitched_load_s_progress_only_grows_across_its_months() {
    let hourly = Timeframe::hours(1);
    let years = Years::new(now() - 200 * 86_400);
    let feed = Continuous::shared(years.clone());
    let name = crate::progress::name(&years.spelled(), hourly);
    let load = crate::progress::begin(&name);
    crate::progress::within(Some(load), || feed.bars(&years.spelled(), hourly, None).unwrap());

    let seen = years.progress.lock().unwrap().clone();
    assert!(seen.len() >= 3, "{seen:?}");
    assert!(seen.windows(2).all(|pair| pair[0] <= pair[1]), "went back: {seen:?}");
    assert!(seen[0] < 0.3, "the front month is read first, and says so: {seen:?}");
    assert_eq!(crate::progress::tests::settled(&name), 1.0);
    crate::progress::end(&name);

    let quiet = Years::new(now() - 200 * 86_400);
    Continuous::shared(quiet.clone()).bars(&quiet.spelled(), hourly, None).unwrap();
    assert!(quiet.progress.lock().unwrap().is_empty());
}

/// A daily chart goes back twenty years, not to the oldest month on disk,
/// and draws each month from its own daily bars.
#[test]
fn a_daily_chart_reaches_back_twenty_years() {
    let daily = Timeframe::days(1);
    let years = Years::new(now() - 200 * 86_400);
    let feed = Continuous::shared(years.clone());
    let started = std::time::Instant::now();
    let bars = feed.bars(&years.spelled(), daily, None).unwrap();
    assert!(started.elapsed() < std::time::Duration::from_secs(2), "{:?}", started.elapsed());
    let reach = midnight(contracts::today() - chrono::Duration::days(DAILY_YEARS * 365 + DAILY_YEARS / 4));
    assert!(bars.first().unwrap().ts >= reach);
    assert!(bars.first().unwrap().ts < reach + 120 * 86_400, "short of twenty years");
    assert!(years.months_read(daily).len() <= (DAILY_YEARS as usize + 1) * 4, "{} months read", years.months_read(daily).len());
    assert!(years.months_read(Timeframe::hours(1)).is_empty(), "a daily chart read intraday bars");
}

/// A second chart of the root, and the other adjustment, read nothing of
/// the expired months again: only the front month, which trades on.
#[test]
fn a_second_load_reads_no_expired_month_again() {
    let hourly = Timeframe::hours(1);
    let years = Years::new(now() - 200 * 86_400);
    let feed = Continuous::shared(years.clone());
    let first = feed.bars(&years.spelled(), hourly, None).unwrap();
    years.read.lock().unwrap().clear();

    let again = feed.bars(&years.spelled(), hourly, None).unwrap();
    assert_eq!(again, first);
    let traded = Spelled { adjustment: Adjustment::AsTraded, ..Spelled::parse(&years.spelled()).unwrap() }.to_string();
    feed.bars(&traded, hourly, None).unwrap();
    assert_eq!(years.months_read(hourly), vec![contract("ES", years.front)], "an expired month read again");
}

/// Scrolled back across rolls, the months before the chart's first bar are
/// read, only them and only over their spans, and moved as a chart drawn
/// that far back to begin with moves them.
#[test]
fn scrolling_back_reads_the_older_months_once_moved_by_every_roll_after_them() {
    let hourly = Timeframe::hours(1);
    let years = Years::new(now() - 120 * 86_400);
    let feed = Continuous::shared(years.clone());
    let shown = feed.bars(&years.spelled(), hourly, None).unwrap();
    let mut chart = shown.clone();
    let shown_months = years.months_read(hourly);

    // Three steps back, of four months each: across three rolls and more.
    for _ in 0..3 {
        let first = chart.first().unwrap().ts;
        let older = feed.bars_between(&years.spelled(), hourly, first - 120 * 86_400, first).unwrap();
        assert!(older.last().unwrap().ts < first && !older.is_empty());
        chart.splice(0..0, older);
    }
    let reached = chart.first().unwrap().ts;
    let fresh = Continuous::shared(Years::new(now() - 120 * 86_400));
    let whole = fresh.bars(&years.spelled(), hourly, Some(reached)).unwrap();
    assert_eq!(chart, whole, "scrolled back is what a chart drawn from there is");

    // Of every expired month, no time read twice.
    for month in years.months_read(hourly).iter().filter(|month| **month != contract("ES", years.front)) {
        let mut spans: Vec<(i64, i64)> = years
            .read
            .lock()
            .unwrap()
            .iter()
            .filter(|read| read.0 == *month && read.1 == hourly && read.4 > 0)
            .map(|read| (read.2, read.3.unwrap()))
            .collect();
        spans.sort();
        assert!(spans.windows(2).all(|pair| pair[0].1 <= pair[1].0), "{month} read twice: {spans:?}");
    }
    assert!(spans_since(years.front, reached, hourly).len() >= shown_months.len() + 3, "not across three rolls");
}
