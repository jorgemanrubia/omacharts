//! Continuous futures: a root's chart drawn, at each time, from the contract
//! that was its front month then.
//!
//! A futures root, `ES`, names whichever contract trades now. A chart of it
//! going back a year is four contracts end to end, and each span of it has to
//! be read from the month that was front during that span: drawn from the
//! current month alone, a long chart shows that month's own history from when
//! it was a thinly traded back month, sparse bars at a premium.
//!
//! Who draws the series is a feed's to declare, in [`Continuity`]:
//!
//! - **Native.** The feed has its own continuous series of a root and serves
//!   it as it is: Yahoo's `ES=F`, an Interactive Brokers `CONTFUT`. Nothing
//!   here touches it. A feed that serves its series both back-adjusted and as
//!   traded lists both, and is asked for one with [`Provider::adjusted`].
//! - **Per contract.** The feed serves each dated contract's own bars, and
//!   ladders where it has them, and this module stitches them: Sierra Chart,
//!   from its file of each month.
//! - **Both.** Stitched when the feed also has ladders, so a volume profile
//!   and a cumulative delta are read from the same months as the bars under
//!   them; otherwise its own series. [`source`] is the whole rule.
//! - **Neither**, the default: a root is its front month alone, as a feed
//!   that can say nothing better has always charted it.
//!
//! **The rolls.** A span ends on the day the app's own front-month rule in
//! [`crate::contracts`] moves to the next contract, so the chart rolls when
//! everything else in the app does. Daily bars are cut at that date, and
//! intraday ones where its trading day starts, at the bar it starts in: no
//! bar is in two spans and none falls between them.
//!
//! **Back-adjusted or as traded**, per chart. Back-adjusted, the default,
//! moves every earlier month by the gap between the two months' closes on
//! the last day before each roll, summed from the newest roll back, so a roll
//! is no jump on the chart and an indicator reaching across one reads a
//! series that moves only as the market did. As traded draws each month at
//! its own prices. The current month is the same in both: nothing after the
//! last roll is moved, so the last price, a position's P&L and an order are
//! real prices. Volume is never adjusted. Ladders move by the same whole
//! number of ticks as the bars over them.
//!
//! **What a chart asks for.** A stitched root is spelled to the feed as
//! [`Spelled`], `ES~ESZ26~back-adjusted`: the root, the month that is front,
//! and the adjustment. That spelling is the cache key's, so the two
//! adjustments are two series, and a roll, which moves every earlier price,
//! is a new series rather than a cache drawn against the month before.
//!
//! **Drawings** are kept at the prices they were drawn at, one set per root
//! whichever way its months are joined, and are never moved. A line drawn at
//! a price on the current month stays at that price; switching a chart to
//! the other adjustment draws the same lines over bars that have moved, and
//! after a roll, back-adjusting moves the old bars a line was drawn against
//! by that roll's gap. That is what a drawing on a back-adjusted chart does
//! elsewhere too, and the alternative, moving every drawing at every roll,
//! would move the levels drawn at real prices for the month being traded.
//!
//! **A month that is missing** from the feed is stood in for by the month
//! after it, over the span it would have drawn, and the log says so. A run of
//! a year missing is where the feed's history of the root ends.
//!
//! Only the spans asked for are read: a request from `since` reads the months
//! from the one front at `since` on, and each from where its span starts.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use chrono::{Datelike, NaiveDate};

use crate::bars::{Bar, Timeframe, Unit};
use crate::contracts::{self, Month, Rule};
use crate::ladders::Minute;
use crate::provider::{
    Capability, Closes, LadderSink, Ladders, Pacing, Provider, ProviderError, SharedCooldown, Sink, Stream,
    Subscription, Update,
};
use crate::symbols::{Instrument, InstrumentKind};

/// How a continuous series treats the price gap at each roll.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Adjustment {
    /// Every earlier month moved by the gaps at the rolls after it, so the
    /// series has no jump at a roll.
    #[default]
    BackAdjusted,
    /// Each month at the prices it traded at.
    AsTraded,
}

impl Adjustment {
    pub const ALL: [Adjustment; 2] = [Adjustment::BackAdjusted, Adjustment::AsTraded];

    /// As it is stored, typed and spelled to a feed.
    pub fn key(self) -> &'static str {
        match self {
            Adjustment::BackAdjusted => "back-adjusted",
            Adjustment::AsTraded => "as-traded",
        }
    }

    pub fn from_key(key: &str) -> Option<Adjustment> {
        Adjustment::ALL.into_iter().find(|adjustment| adjustment.key().eq_ignore_ascii_case(key.trim()))
    }

    /// What a menu calls it.
    pub fn label(self) -> &'static str {
        match self {
            Adjustment::BackAdjusted => "Back-adjusted",
            Adjustment::AsTraded => "As traded",
        }
    }
}

/// How a feed serves a futures root over time: see the module docs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Continuity {
    /// The feed's own continuous series of a root, in the adjustments it
    /// serves it in. Empty for a feed with no such series.
    pub native: &'static [Adjustment],
    /// Does the feed serve each dated contract's bars, asked for as
    /// `ESZ26` — the root, the month's code and a two-digit year — and their
    /// ladders when it has ladders, from as far back as it has them?
    pub per_contract: bool,
}

impl Continuity {
    /// A root is its front month and nothing more: the default.
    pub const FRONT_MONTH: Continuity = Continuity { native: &[], per_contract: false };
    /// Each month's own bars, for the engine to stitch.
    pub const PER_CONTRACT: Continuity = Continuity { native: &[], per_contract: true };

    /// The feed's own continuous series, in these adjustments.
    pub const fn native(adjustments: &'static [Adjustment]) -> Continuity {
        Continuity { native: adjustments, per_contract: false }
    }
}

/// Where a feed's continuous series of a root comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// Stitched here from the feed's months.
    Stitched,
    /// The feed's own.
    Native,
    /// The front month alone.
    FrontMonth,
}

/// Which series a feed's roots are drawn from. Stitched when the feed serves
/// its months and either has no series of its own or has ladders, which
/// only the months can keep in step with the bars; else its own series when
/// it has one; else the front month.
pub fn source(provider: &dyn Provider) -> Source {
    let continuity = provider.continuity();
    match continuity.per_contract && (continuity.native.is_empty() || provider.ladders().is_some()) {
        true => Source::Stitched,
        false if !continuity.native.is_empty() => Source::Native,
        false => Source::FrontMonth,
    }
}

/// Is there a choice of adjustment for a chart of `instrument` from
/// `provider`? A root the feed stitches, or one the feed has its own series
/// of in both adjustments. Never a dated contract, which is only itself.
pub fn offers_choice(provider: &dyn Provider, instrument: &Instrument) -> bool {
    if instrument.kind != InstrumentKind::FutureRoot || instrument.is_contract() {
        return false;
    }
    match source(provider) {
        Source::Stitched => provider.symbol_for(instrument).is_some_and(|symbol| Spelled::parse(&symbol).is_some()),
        Source::Native => Adjustment::ALL.iter().all(|adjustment| provider.continuity().native.contains(adjustment)),
        Source::FrontMonth => false,
    }
}

/// Does any feed this build offers have a choice of adjustment for its
/// roots? Where none does, there is no default to choose either. Builds each
/// feed, which connects to nothing.
pub fn any_offered() -> bool {
    crate::providers::offered().filter_map(|feed| crate::providers::built(feed.id)).any(|provider| {
        match source(provider.as_ref()) {
            Source::Stitched => true,
            Source::Native => Adjustment::ALL.iter().all(|adjustment| provider.continuity().native.contains(adjustment)),
            Source::FrontMonth => false,
        }
    })
}

/// What a chart of `instrument` in `adjustment` asks `provider` for, given
/// the symbol it routed to: the stitched spelling in that adjustment, the
/// feed's own series in it, or `symbol` as it is where there is no choice.
pub fn routed(provider: &dyn Provider, instrument: &Instrument, symbol: &str, adjustment: Adjustment) -> String {
    if let Some(spelled) = Spelled::parse(symbol) {
        return Spelled { adjustment, ..spelled }.to_string();
    }
    match offers_choice(provider, instrument) && source(provider) == Source::Native {
        true => provider.adjusted(symbol, adjustment),
        false => symbol.to_string(),
    }
}

/// A symbol as somebody reads it: a stitched root's spelling as its root.
pub fn shown(symbol: &str) -> &str {
    match symbol.split_once(SEPARATOR) {
        Some((root, _)) if Spelled::parse(symbol).is_some() => root,
        _ => symbol,
    }
}

const SEPARATOR: char = '~';

/// A stitched root as a feed is asked for it: `ES~ESZ26~back-adjusted`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Spelled {
    pub root: String,
    /// The month front now: the last span, which runs on.
    pub front: Month,
    pub adjustment: Adjustment,
}

impl Spelled {
    pub fn parse(symbol: &str) -> Option<Spelled> {
        let mut parts = symbol.split(SEPARATOR);
        let (root, front, adjustment) = (parts.next()?, parts.next()?, parts.next()?);
        if parts.next().is_some() || root.is_empty() {
            return None;
        }
        let (of, front) = contracts::parse(front, contracts::today())?;
        (of == root).then_some(())?;
        Some(Spelled { root: root.to_string(), front, adjustment: Adjustment::from_key(adjustment)? })
    }
}

impl std::fmt::Display for Spelled {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}{SEPARATOR}{}{SEPARATOR}{}", self.root, contract(&self.root, self.front), self.adjustment.key())
    }
}

/// A dated contract as a per-contract feed is asked for it: `ESZ26`, with a
/// two-digit year, which no decade can be mistaken for.
pub fn contract(root: &str, month: Month) -> String {
    format!("{root}{}{:02}", month.code(), month.year.rem_euclid(100))
}

/// The contract before `month` in the rule's cycle.
fn previous(rule: &Rule, month: Month) -> Month {
    (1..=24).map(|back| month.plus(-back)).find(|m| rule.months.contains(&m.code())).unwrap_or(month.plus(-1))
}

/// The contracts the root was drawn from, newest first, from `front` back:
/// each with the date it became the front month, the day the rule rolled the
/// one before it.
fn schedule(root: &str, front: Month) -> impl Iterator<Item = (Month, NaiveDate)> {
    let rule = contracts::rule_of(root);
    let mut month = Some(front);
    std::iter::from_fn(move || {
        let current = month?;
        let before = previous(&rule, current);
        let starts = rule.roll(before)?;
        month = (starts.year() > 1980).then_some(before);
        Some((current, starts))
    })
}

/// An instrument of `root`, for its exchange's calendar.
fn root_instrument(root: &str) -> Instrument {
    Instrument {
        symbol: root.to_string(),
        name: String::new(),
        kind: InstrumentKind::FutureRoot,
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

fn midnight(date: NaiveDate) -> i64 {
    date.and_hms_opt(0, 0, 0).map_or(0, |midnight| midnight.and_utc().timestamp())
}

/// Where a span starting on the trading day `date` starts, in bars of
/// `timeframe`: a daily bar's own stamp, midnight UTC of its date; an
/// intraday bar's, the start of the bar the trading day opens in.
fn cut(root: &str, date: NaiveDate, timeframe: Timeframe) -> i64 {
    match timeframe.unit {
        Unit::Minute | Unit::Hour => {
            let start = crate::exchanges::registry()
                .for_instrument(&root_instrument(root))
                .map_or_else(|| midnight(date), |schedule| schedule.day_start(date));
            let step = timeframe.seconds().max(60);
            start.div_euclid(step) * step
        }
        _ => midnight(date),
    }
}

/// One month's part of a stitched series.
#[derive(Clone, Debug)]
struct Span {
    month: Month,
    /// The trading date it took over from the month before it.
    starts: NaiveDate,
    /// Its first bar's stamp at the series' timeframe, and where the span
    /// after it starts: `None` for the front month, which runs on.
    from: i64,
    to: Option<i64>,
    /// Its own bars over the span, unadjusted.
    bars: Vec<Bar>,
    /// Months the feed has nothing of that this one is drawn over instead.
    stood_in: Vec<Month>,
}

/// A run of months missing this long is where the feed's history ends.
const MISSING_RUN_DAYS: i64 = 400;

/// A month's bars from `since`, as the feed has them: `None` when it has no
/// such contract.
type Fetch<'a> = dyn Fn(Month, i64) -> Result<Option<Vec<Bar>>, ProviderError> + 'a;

fn fetch_from<'a>(inner: &'a dyn Provider, root: &str, timeframe: Timeframe) -> impl Fn(Month, i64) -> Result<Option<Vec<Bar>>, ProviderError> + 'a {
    let root = root.to_string();
    move |month, since| match inner.bars(&contract(&root, month), timeframe, Some(since)) {
        Ok(bars) => Ok(Some(bars)),
        Err(ProviderError::NotFound) => Ok(None),
        Err(error) => Err(error),
    }
}

/// The spans of `root` from `front` back to the one front at `since`, newest
/// first, each with its own bars over it. A month the feed has nothing of
/// over its span is drawn from the month after it, and so is one it could
/// not read just now, which goes in `unread`.
fn spans(root: &str, front: Month, timeframe: Timeframe, since: Option<i64>, fetch: &Fetch, unread: &mut Vec<(Month, String)>) -> Result<Vec<Span>, ProviderError> {
    let since = since.unwrap_or(i64::MIN);
    let mut found: Vec<Span> = Vec::new();
    // Missing months since the last one found, newest first, with where
    // the oldest of them starts.
    let mut missing: Vec<Month> = Vec::new();
    let mut missing_from: Option<(NaiveDate, i64)> = None;
    let mut upper: Option<i64> = None;
    for (month, starts) in schedule(root, front).take(600) {
        let from = cut(root, starts, timeframe);
        let want = from.max(since);
        let within = |bars: Vec<Bar>, from: i64, to: Option<i64>| -> Vec<Bar> {
            bars.into_iter().filter(|bar| bar.ts >= from && to.is_none_or(|to| bar.ts < to)).collect()
        };
        let got = match fetch(month, want) {
            Ok(got) => got.map(|bars| within(bars, want, upper)),
            Err(error) if found.is_empty() => return Err(error),
            Err(error) => {
                unread.push((month, error.to_string()));
                None
            }
        };
        match got {
            // The front month is whatever it has, nothing new included.
            Some(bars) if found.is_empty() => {
                found.push(Span { month, starts, from, to: None, bars, stood_in: Vec::new() });
            }
            None if found.is_empty() => return Err(ProviderError::NotFound),
            Some(bars) if !bars.is_empty() => {
                if let (Some((missing_starts, missing_from)), Some(newer)) = (missing_from.take(), found.last_mut()) {
                    stand_in(newer, missing_starts, missing_from, std::mem::take(&mut missing), fetch)?;
                }
                found.push(Span { month, starts, from, to: upper, bars, stood_in: Vec::new() });
            }
            _ => {
                missing.push(month);
                missing_from = Some((starts, want));
                let newest_missing = found.last().map_or(starts, |newer| newer.starts);
                if (newest_missing - starts).num_days() > MISSING_RUN_DAYS {
                    break;
                }
            }
        }
        upper = Some(from);
        if from <= since {
            // The span `since` falls in is drawn, by a month that stands in
            // for it when its own is missing.
            if let (Some((missing_starts, missing_from)), Some(newer)) = (missing_from.take(), found.last_mut()) {
                stand_in(newer, missing_starts, missing_from, std::mem::take(&mut missing), fetch)?;
            }
            break;
        }
    }
    Ok(found)
}

/// Draw `newer` back over months the feed has nothing of, from `from`.
fn stand_in(newer: &mut Span, starts: NaiveDate, from: i64, months: Vec<Month>, fetch: &Fetch) -> Result<(), ProviderError> {
    let to = newer.to;
    if let Some(bars) = fetch(newer.month, from)? {
        newer.bars = bars.into_iter().filter(|bar| bar.ts >= from && to.is_none_or(|to| bar.ts < to)).collect();
    }
    newer.starts = starts;
    newer.from = from;
    newer.stood_in.extend(months);
    Ok(())
}

/// A roll, as a gap is kept under: the feed, the root, the older and newer
/// month, the roll date, and the bars it was measured on: `None` for daily
/// bars, else the timeframe, since bars of another one can say otherwise.
type Roll = (String, String, Month, Month, NaiveDate, Option<Timeframe>);

/// The gaps between months at rolls: a roll in the past never changes.
/// `None` for a roll whose months have no price at the same time before it,
/// read whole: one that will never be measured.
static GAPS: Mutex<Option<HashMap<Roll, Option<f64>>>> = Mutex::new(None);

/// What is known of the gap at a roll.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum Gap {
    Measured(f64),
    /// Both months were read and never traded at the same time before the
    /// roll: there is nothing to measure, now or later.
    Unmeasurable,
    /// A month could not be read just now: asked again next time.
    Unknown,
}

impl Gap {
    fn of(kept: Option<f64>) -> Gap {
        kept.map_or(Gap::Unmeasurable, Gap::Measured)
    }
}

/// The bars a gap is measured on when there are no daily ones: hourly where
/// the feed has them, so a series and its ladders, asked at different
/// timeframes, are moved by the same gap.
fn fallback_basis(inner: &dyn Provider, timeframe: Timeframe) -> Timeframe {
    match inner.serves(Timeframe::hours(1)) {
        true => Timeframe::hours(1),
        false => timeframe,
    }
}

/// What `newer` traded above `older` at the roll on `roll`, in whole ticks
/// when the root's tick is known: the two months' closes on the last day
/// both have a daily bar before the roll, whatever the series' timeframe;
/// else, on hourly bars, or the series' own where the feed has no hourly
/// ones, the older month's last bar before the roll and the newer month's
/// at the same time.
pub(crate) fn gap(inner: &dyn Provider, root: &str, older: Month, newer: Month, roll: NaiveDate, timeframe: Timeframe) -> Gap {
    let basis = fallback_basis(inner, timeframe);
    let key = |on: Option<Timeframe>| (inner.id().to_string(), root.to_string(), older, newer, roll, on);
    let known = |on: Option<Timeframe>| GAPS.lock().unwrap_or_else(|e| e.into_inner()).as_ref().and_then(|gaps| gaps.get(&key(on)).copied());
    let keep = |on: Option<Timeframe>, gap: Option<f64>| {
        GAPS.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(HashMap::new).insert(key(on), gap);
    };
    if let Some(kept) = known(None) {
        return Gap::of(kept);
    }
    let timeframe = basis;
    // A month's bars, none when the feed has none of it; `Err` when it could
    // not be read just now.
    let read = |month: Month, on: Timeframe, since: i64| -> Result<Vec<Bar>, ()> {
        match inner.bars(&contract(root, month), on, Some(since)) {
            Ok(bars) => Ok(bars),
            Err(ProviderError::NotFound) => Ok(Vec::new()),
            Err(_) => Err(()),
        }
    };
    let close_before = |bars: &[Bar], before: i64| bars.iter().rev().find(|bar| bar.ts < before).copied();
    let daily = Timeframe::days(1);
    let by_day = || -> Result<Option<f64>, ()> {
        let since = midnight(roll - chrono::Duration::days(14));
        let (older, newer) = (read(older, daily, since)?, read(newer, daily, since)?);
        let day = older.iter().rev().filter(|bar| bar.ts < midnight(roll)).find(|bar| newer.iter().any(|n| n.ts == bar.ts));
        Ok(day.and_then(|day| newer.iter().find(|n| n.ts == day.ts).map(|then| then.close - day.close)))
    };
    let by_bar = || -> Result<Option<f64>, ()> {
        let at = cut(root, roll, timeframe);
        let since = at - 7 * 86_400;
        let (older, newer) = (read(older, timeframe, since)?, read(newer, timeframe, since)?);
        let Some(last) = close_before(&older, at) else { return Ok(None) };
        Ok(close_before(&newer, last.ts + 1).map(|then| then.close - last.close))
    };
    let rounded = |gap: f64| match crate::specs::tick(root) {
        Some(tick) if tick > 0.0 => (gap / tick).round() * tick,
        _ => gap,
    };
    // Daily first even when an hourly gap is known: one measured on hourly
    // bars while the daily ones could not be read gives way to the daily
    // one as soon as they can, for the series and its ladders alike.
    let by_day = inner.serves(daily).then(by_day);
    if let Some(Ok(Some(gap))) = by_day {
        let gap = rounded(gap);
        keep(None, Some(gap));
        return Gap::Measured(gap);
    }
    if let Some(kept) = known(Some(timeframe)) {
        return Gap::of(kept);
    }
    match (by_day, by_bar()) {
        (_, Ok(Some(gap))) => {
            let gap = rounded(gap);
            keep(Some(timeframe), Some(gap));
            Gap::Measured(gap)
        }
        // Every month read, and no time they both traded: never measured.
        (None | Some(Ok(None)), Ok(None)) => {
            keep(Some(timeframe), None);
            Gap::Unmeasurable
        }
        _ => Gap::Unknown,
    }
}

/// What each span is moved by, in its order: nothing for the front month,
/// and for each before it the gaps at every roll after it. As traded,
/// nothing for any. A roll with no gap to be had is joined as it is, and
/// one whose months could not be read to measure it goes in `unjoined`.
fn offsets(inner: &dyn Provider, root: &str, months: &[(Month, NaiveDate)], adjustment: Adjustment, timeframe: Timeframe, unjoined: &mut Vec<Month>) -> Vec<f64> {
    let mut offsets = Vec::with_capacity(months.len());
    let mut total = 0.0;
    for (at, (month, _)) in months.iter().enumerate() {
        if at > 0 && adjustment == Adjustment::BackAdjusted {
            let (newer, roll) = months[at - 1];
            match gap(inner, root, *month, newer, roll, timeframe) {
                Gap::Measured(gap) => total += gap,
                Gap::Unmeasurable => say_once(
                    format!("gap {root} {month:?}"),
                    inner.label(),
                    format!("{root}: no price of {} and {} at the same time before the roll, so they are joined without adjusting", contract(root, *month), contract(root, newer)),
                ),
                Gap::Unknown => {
                    unjoined.push(*month);
                    say_once(
                        format!("gap unknown {root} {month:?}"),
                        inner.label(),
                        format!("{root}: {} or {} could not be read to measure the roll, so they are joined without adjusting until they can be", contract(root, *month), contract(root, newer)),
                    )
                }
            }
        }
        offsets.push(total);
    }
    offsets
}

/// What has been said, so a chart drawn again does not say it again.
static SAID: Mutex<Option<HashSet<String>>> = Mutex::new(None);

fn say_once(key: String, source: &str, text: String) {
    let fresh = SAID.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(HashSet::new).insert(key);
    if fresh {
        crate::log::add(crate::log::Entry::warning(source, text));
    }
}

/// Say which months the feed had nothing of, and what was drawn instead.
fn say_stood_in(inner: &dyn Provider, root: &str, spans: &[Span]) {
    for span in spans.iter().filter(|span| !span.stood_in.is_empty()) {
        let missing: Vec<String> = span.stood_in.iter().rev().map(|month| contract(root, *month)).collect();
        say_once(
            format!("missing {root} {}", missing.join(",")),
            inner.label(),
            format!("{root}: {} has no {}, so {} is drawn there", inner.label(), missing.join(", "), contract(root, span.month)),
        );
    }
}

/// Every span's bars, oldest first, each moved by its offset.
fn assembled(spans: &[Span], offsets: &[f64]) -> Vec<Bar> {
    let mut bars = Vec::with_capacity(spans.iter().map(|span| span.bars.len()).sum());
    for (span, offset) in spans.iter().zip(offsets).rev() {
        bars.extend(span.bars.iter().map(|bar| Bar {
            open: bar.open + offset,
            high: bar.high + offset,
            low: bar.low + offset,
            close: bar.close + offset,
            ..*bar
        }));
    }
    bars
}

/// The stitched series `spelled` names, from `since`, with `fetch` reading
/// each month, and whether it is short of what it should be: a month that
/// could not be read, or a roll joined unadjusted that should have been.
fn stitched(inner: &dyn Provider, spelled: &Spelled, timeframe: Timeframe, since: Option<i64>, fetch: &Fetch) -> Result<(Vec<Bar>, bool), ProviderError> {
    let root = spelled.root.as_str();
    let mut unread = Vec::new();
    let spans = spans(root, spelled.front, timeframe, since, fetch, &mut unread)?;
    for (month, why) in &unread {
        // Said every time: it is drawn again until it can be read.
        crate::log::add(crate::log::Entry::warning(
            inner.label(),
            format!("{root}: {} could not be read ({why}), so the month after it is drawn there until it can be", contract(root, *month)),
        ));
    }
    let missing: Vec<Span> = spans
        .iter()
        .map(|span| Span { stood_in: span.stood_in.iter().copied().filter(|month| !unread.iter().any(|(m, _)| m == month)).collect(), ..span.clone() })
        .collect();
    say_stood_in(inner, root, &missing);
    let months: Vec<(Month, NaiveDate)> = spans.iter().map(|span| (span.month, span.starts)).collect();
    let mut unjoined = Vec::new();
    let offsets = offsets(inner, root, &months, spelled.adjustment, timeframe, &mut unjoined);
    Ok((assembled(&spans, &offsets), !unread.is_empty() || !unjoined.is_empty()))
}

/// A stitched series, as [`DEGRADED`] knows it: the feed, the spelling and
/// the timeframe.
type Series = (String, String, Timeframe);

/// The series last stitched short of what they should be. Drawn again
/// whole the next time they are asked for, as though nothing were cached:
/// a series topped up from where its cache ends would keep the months
/// before that short until the next roll.
static DEGRADED: Mutex<Option<HashSet<Series>>> = Mutex::new(None);

fn degraded(series: &Series) -> bool {
    DEGRADED.lock().unwrap_or_else(|e| e.into_inner()).as_ref().is_some_and(|degraded| degraded.contains(series))
}

/// Whether feed `provider`'s `symbol` at `timeframe` was last stitched short
/// of what it should be: what was drawn is shown, and not to be kept, so a
/// restart reads it whole too.
pub fn drawn_short(provider: &str, symbol: &str, timeframe: Timeframe) -> bool {
    degraded(&(provider.to_string(), symbol.to_string(), timeframe))
}

fn note_degraded(series: Series, short: bool) {
    let mut degraded = DEGRADED.lock().unwrap_or_else(|e| e.into_inner());
    let degraded = degraded.get_or_insert_with(HashSet::new);
    match short {
        true => degraded.insert(series),
        false => degraded.remove(&series),
    };
}

/// How often a stitched root being followed live asks its feed which month
/// is front, as Sierra Chart's feed does for a root of its own.
const ROLL_CHECK: std::time::Duration = std::time::Duration::from_secs(600);

/// A stitched root followed live: the feed, the root and the month it is
/// followed in, for as long as `alive` is.
struct Followed {
    inner: Arc<dyn Provider>,
    root: String,
    front: Month,
    alive: std::sync::Weak<()>,
}

static FOLLOWED: Mutex<Vec<Followed>> = Mutex::new(Vec::new());
type Listener = Box<dyn Fn() -> bool + Send + Sync>;
static ROLLED: Mutex<Vec<Listener>> = Mutex::new(Vec::new());
static CHECKING: std::sync::Once = std::sync::Once::new();

/// A subscription to a stitched root's front month, followed for a roll
/// while it is held.
struct Following {
    _subscription: Box<dyn Subscription>,
    _alive: Arc<()>,
}

impl Subscription for Following {}

/// `subscription`, of `spelled`'s front month, noted as followed: when the
/// feed's front month of the root is another, [`on_rolled`] hears it.
fn follow(inner: Arc<dyn Provider>, spelled: Spelled, subscription: Box<dyn Subscription>) -> Following {
    let alive = Arc::new(());
    let mut followed = FOLLOWED.lock().unwrap_or_else(|e| e.into_inner());
    followed.retain(|followed| followed.alive.strong_count() > 0);
    followed.push(Followed {
        inner,
        root: spelled.root,
        front: spelled.front,
        alive: Arc::downgrade(&alive),
    });
    drop(followed);
    CHECKING.call_once(|| {
        let _ = std::thread::Builder::new().name("omacharts rolls".into()).spawn(|| loop {
            std::thread::sleep(ROLL_CHECK);
            check_rolls();
        });
    });
    Following { _subscription: subscription, _alive: alive }
}

/// Be told when a stitched root followed live has rolled: its feed's front
/// month is no longer the one it is drawn up to. The chart's symbol names
/// that month, so whoever hears this routes it again, to a series, a stream
/// and ladders of the month that is front now. Called from any thread, for
/// as long as it says it is still listening.
pub fn on_rolled(listener: impl Fn() -> bool + Send + Sync + 'static) {
    ROLLED.lock().unwrap_or_else(|e| e.into_inner()).push(Box::new(listener));
}

/// Ask each feed whether a root followed live has rolled, now rather than
/// at the next check, and tell [`on_rolled`] if one has. Whether one had.
pub fn check_rolls() -> bool {
    let followed: Vec<(Arc<dyn Provider>, String, Month)> = {
        let mut followed = FOLLOWED.lock().unwrap_or_else(|e| e.into_inner());
        followed.retain(|followed| followed.alive.strong_count() > 0);
        followed.iter().map(|followed| (followed.inner.clone(), followed.root.clone(), followed.front)).collect()
    };
    let rolled: Vec<(String, Month)> = followed
        .into_iter()
        .filter(|(inner, root, front)| front_now(inner.as_ref(), root).is_some_and(|now| now != *front))
        .map(|(_, root, front)| (root, front))
        .collect();
    if rolled.is_empty() {
        return false;
    }
    // Heard once: what is routed again follows the month front now.
    FOLLOWED
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .retain(|followed| !rolled.iter().any(|(root, front)| *root == followed.root && *front == followed.front));
    ROLLED.lock().unwrap_or_else(|e| e.into_inner()).retain(|listener| listener());
    true
}

/// The month `inner` charts `root` as now.
fn front_now(inner: &dyn Provider, root: &str) -> Option<Month> {
    let front = inner.charted_contract(&root_instrument(root))?;
    contracts::parse(&front, contracts::today()).filter(|(of, _)| of == root).map(|(_, month)| month)
}

/// Where a stitched symbol's months change between `from` and `to`, oldest
/// first, on minute bars: where to split a request for its ladders so that
/// each is of one month and moved by one offset. None for any other symbol.
pub fn roll_cuts(symbol: &str, from: i64, to: i64) -> Vec<i64> {
    let Some(spelled) = Spelled::parse(symbol) else { return Vec::new() };
    let mut cuts = Vec::new();
    for (_, starts) in schedule(&spelled.root, spelled.front).take(600) {
        let at = cut(&spelled.root, starts, Timeframe::minutes(1));
        if at <= from {
            break;
        }
        if at < to {
            cuts.push(at);
        }
    }
    cuts.reverse();
    cuts
}

/// The contract a stitched symbol is drawn up to, `ESZ26`, as the feed was
/// asked for it when the chart was shown: what its last price is a price
/// of, whatever month is front by now. `None` for any other symbol.
pub fn drawn_front(symbol: &str) -> Option<String> {
    Continuous::front_of(symbol).map(|(_, front)| front)
}

/// A feed whose roots are stitched from its months: everything it does,
/// with a root spelled as [`Spelled`] drawn from the months and everything
/// else handed through as it was.
pub struct Continuous {
    inner: Arc<dyn Provider>,
}

impl Continuous {
    pub fn new(inner: Box<dyn Provider>) -> Continuous {
        Continuous { inner: Arc::from(inner) }
    }

    /// The same, of a feed held elsewhere too.
    pub fn shared(inner: Arc<dyn Provider>) -> Continuous {
        Continuous { inner }
    }

    /// The front month of a stitched symbol, as the feed is asked for it.
    fn front_of(symbol: &str) -> Option<(Spelled, String)> {
        let spelled = Spelled::parse(symbol)?;
        let front = contract(&spelled.root, spelled.front);
        Some((spelled, front))
    }
}

/// `provider`, stitched when [`source`] says its roots are.
pub fn wrapped(provider: Box<dyn Provider>) -> Box<dyn Provider> {
    match source(provider.as_ref()) {
        Source::Stitched => Box::new(Continuous::new(provider)),
        _ => provider,
    }
}

impl Provider for Continuous {
    fn id(&self) -> &'static str {
        self.inner.id()
    }

    fn label(&self) -> &'static str {
        self.inner.label()
    }

    fn delay_minutes(&self, kind: InstrumentKind) -> u32 {
        self.inner.delay_minutes(kind)
    }

    /// A root the feed has a front month of, spelled for stitching,
    /// back-adjusted: a chart set to as traded asks [`routed`] for that.
    fn symbol_for(&self, instrument: &Instrument) -> Option<String> {
        let symbol = self.inner.symbol_for(instrument)?;
        if instrument.kind != InstrumentKind::FutureRoot || instrument.is_contract() || symbol != instrument.symbol {
            return Some(symbol);
        }
        let front = self.inner.charted_contract(instrument).and_then(|front| contracts::parse(&front, contracts::today()));
        match front {
            Some((root, front)) if root == symbol => {
                Some(Spelled { root, front, adjustment: Adjustment::BackAdjusted }.to_string())
            }
            _ => Some(symbol),
        }
    }

    fn could_serve(&self, instrument: &Instrument) -> bool {
        self.inner.could_serve(instrument)
    }

    /// The front month: the last span, which is never moved, so the prices
    /// a chart shows now are that month's real ones in either adjustment.
    fn charted_contract(&self, instrument: &Instrument) -> Option<String> {
        self.inner.charted_contract(instrument)
    }

    fn capabilities(&self) -> &'static [Capability] {
        self.inner.capabilities()
    }

    fn continuity(&self) -> Continuity {
        self.inner.continuity()
    }

    /// A stitched root's series from `since`; all of it, whatever `since`
    /// says, when it was last drawn short, so that what replaces it in the
    /// cache is the whole series: the loader starts a cache over when the
    /// bars it gets disagree with it.
    fn bars(&self, symbol: &str, timeframe: Timeframe, since: Option<i64>) -> Result<Vec<Bar>, ProviderError> {
        let Some(spelled) = Spelled::parse(symbol) else { return self.inner.bars(symbol, timeframe, since) };
        let series = (self.inner.id().to_string(), symbol.to_string(), timeframe);
        let since = since.filter(|_| !degraded(&series));
        let fetch = fetch_from(self.inner.as_ref(), &spelled.root, timeframe);
        let (bars, short) = stitched(self.inner.as_ref(), &spelled, timeframe, since, &fetch)?;
        note_degraded(series, short);
        Ok(bars)
    }

    fn contracts(&self, root: &str) -> Result<Vec<(String, NaiveDate)>, ProviderError> {
        self.inner.contracts(root)
    }

    fn pacing(&self) -> Pacing {
        self.inner.pacing()
    }

    fn cooldown_until(&self) -> Option<std::time::Instant> {
        self.inner.cooldown_until()
    }

    fn max_batch(&self) -> usize {
        self.inner.max_batch()
    }

    /// A stitched root's closes are its front month's: the newest two are
    /// after the last roll, or straddle it by a day.
    fn closes(&self, symbols: &[&str]) -> Result<Vec<Closes>, ProviderError> {
        let asked: Vec<String> =
            symbols.iter().map(|symbol| Continuous::front_of(symbol).map_or_else(|| symbol.to_string(), |(_, front)| front)).collect();
        let asked: Vec<&str> = asked.iter().map(String::as_str).collect();
        let mut closes = self.inner.closes(&asked)?;
        for close in &mut closes {
            if let Some(at) = asked.iter().position(|asked| *asked == close.symbol) {
                close.symbol = symbols[at].to_string();
            }
        }
        Ok(closes)
    }

    fn shared_cooldown(&self) -> Option<SharedCooldown> {
        self.inner.shared_cooldown()
    }

    fn resume_cooldown(&self, cooldown: SharedCooldown) {
        self.inner.resume_cooldown(cooldown)
    }

    fn retry_after(&self, error: &ProviderError, attempt: u32) -> Option<std::time::Duration> {
        self.inner.retry_after(error, attempt)
    }

    fn stream(&self) -> Option<&dyn Stream> {
        self.inner.stream().map(|_| self as &dyn Stream)
    }

    fn ladders(&self) -> Option<&dyn Ladders> {
        self.inner.ladders().map(|_| self as &dyn Ladders)
    }

    fn retire(&self) {
        self.inner.retire()
    }
}

impl Stream for Continuous {
    /// The front month's own stream, live as the feed has it, with the
    /// months before it put in front of each snapshot it sends.
    fn subscribe(&self, symbol: &str, timeframe: Timeframe, since: Option<i64>, sink: Sink) -> Result<Box<dyn Subscription>, ProviderError> {
        let stream = self.inner.stream().ok_or_else(|| ProviderError::Unsupported("no stream".into()))?;
        let Some((spelled, front)) = Continuous::front_of(symbol) else { return stream.subscribe(symbol, timeframe, since, sink) };
        let inner = self.inner.clone();
        let series = (self.inner.id().to_string(), symbol.to_string(), timeframe);
        let following = spelled.clone();
        let stitching: Sink = Box::new(move |update| match update {
            Update::Snapshot(bars) => {
                // The snapshot is the front month's; the rest are read here,
                // on the feed's thread.
                let given = Mutex::new(Some(bars));
                let root = spelled.root.clone();
                let read = fetch_from(inner.as_ref(), &root, timeframe);
                let fetch = |month: Month, from: i64| -> Result<Option<Vec<Bar>>, ProviderError> {
                    if month == spelled.front
                        && let Some(bars) = given.lock().unwrap_or_else(|e| e.into_inner()).take()
                    {
                        return Ok(Some(bars.into_iter().filter(|bar| bar.ts >= from).collect()));
                    }
                    read(month, from)
                };
                // The months before the front month in full when they were
                // drawn short: what the chart holds of them is replaced.
                let since = since.filter(|_| !degraded(&series));
                match stitched(inner.as_ref(), &spelled, timeframe, since, &fetch) {
                    Ok((bars, short)) => {
                        note_degraded(series.clone(), short);
                        sink(Update::Snapshot(bars))
                    }
                    Err(error) => sink(Update::Lost((&error).into())),
                }
            }
            other => sink(other),
        });
        let subscription = stream.subscribe(&front, timeframe, since, stitching)?;
        Ok(Box::new(follow(self.inner.clone(), following, subscription)))
    }
}

/// `minute` moved by `ticks` of `tick_size`: its prices and its ladder.
fn shifted(mut minute: Minute, ticks: i32, tick_size: f64) -> Minute {
    if ticks != 0 {
        let by = f64::from(ticks) * tick_size;
        minute.open += by;
        minute.high += by;
        minute.low += by;
        minute.close += by;
        minute.ladder.low_tick += ticks;
    }
    minute
}

impl Ladders for Continuous {
    fn serves(&self, symbol: &str) -> bool {
        let Some(ladders) = self.inner.ladders() else { return false };
        match Continuous::front_of(symbol) {
            Some((_, front)) => ladders.serves(&front),
            None => ladders.serves(symbol),
        }
    }

    fn history_days(&self) -> u32 {
        self.inner.ladders().map_or(0, |ladders| ladders.history_days())
    }

    /// A stitched root's own spelling names its front month already, and so
    /// what it is kept under.
    fn contract(&self, symbol: &str) -> Option<String> {
        match Spelled::parse(symbol) {
            Some(_) => None,
            None => self.inner.ladders()?.contract(symbol),
        }
    }

    /// Each span's minutes from its own month, moved by the same ticks as
    /// the bars over them; a month the feed has no ladders of is drawn from
    /// the month after it, as its bars are.
    fn minutes(&self, symbol: &str, tick_size: f64, from: i64, to: i64) -> Result<Vec<Minute>, ProviderError> {
        let ladders = self.inner.ladders().ok_or(ProviderError::NotFound)?;
        let Some(spelled) = Spelled::parse(symbol) else { return ladders.minutes(symbol, tick_size, from, to) };
        let root = spelled.root.as_str();
        let minute = Timeframe::minutes(1);
        // Newest first: the month each span is read from, where it starts,
        // and the months it was taken from in turn, for the offsets.
        let mut reads: Vec<(Month, i64, i64)> = Vec::new();
        let mut drawn: Vec<(Month, NaiveDate)> = Vec::new();
        let mut upper = to;
        let mut standing: Option<Month> = None;
        let mut missing_since: Option<NaiveDate> = None;
        for (month, starts) in schedule(root, spelled.front).take(600) {
            let span_from = cut(root, starts, minute);
            let has = ladders.serves(&contract(root, month));
            match has {
                true => missing_since = None,
                false => {
                    let since = *missing_since.get_or_insert(drawn.last().map_or(starts, |last| last.1));
                    if (since - starts).num_days() > MISSING_RUN_DAYS {
                        break;
                    }
                }
            }
            let reader = match (has, standing) {
                (true, _) => {
                    match drawn.last_mut() {
                        Some(last) if last.0 == month => last.1 = starts,
                        _ => drawn.push((month, starts)),
                    }
                    Some(month)
                }
                (false, Some(newer)) => {
                    if let Some(last) = drawn.last_mut() {
                        last.1 = starts;
                    }
                    Some(newer)
                }
                (false, None) => None,
            };
            if let Some(reader) = reader {
                standing = Some(reader);
                let (a, b) = (span_from.max(from), upper);
                if a < b {
                    reads.push((reader, a, b));
                }
            }
            upper = span_from.min(upper);
            if span_from <= from {
                break;
            }
        }
        // Ladders are kept once read, so a span is not moved by a guess: with
        // a gap that could not be measured just now nothing is kept, and the
        // next chart asks again. Asked a span at a time, as [`roll_cuts`]
        // splits a request, only the spans before that roll wait.
        let mut unjoined = Vec::new();
        let offsets = offsets(self.inner.as_ref(), root, &drawn, spelled.adjustment, Timeframe::days(1), &mut unjoined);
        if let Some(month) = unjoined.first() {
            return Err(ProviderError::Malformed(format!("no gap at the roll from {} to know its ladders by", contract(root, *month))));
        }
        let offset_of = |month: Month| drawn.iter().position(|(m, _)| *m == month).map_or(0.0, |at| offsets[at]);
        let mut minutes = Vec::new();
        for (month, a, b) in reads.into_iter().rev() {
            let ticks = (offset_of(month) / tick_size).round() as i32;
            let read = ladders.minutes(&contract(root, month), tick_size, a, b)?;
            minutes.extend(read.into_iter().filter(|m| m.ts >= a && m.ts < b).map(|m| shifted(m, ticks, tick_size)));
        }
        Ok(minutes)
    }

    /// The front month's live ladders: never moved.
    fn watch(&self, symbol: &str, tick_size: f64, sink: LadderSink) -> Result<Box<dyn Subscription>, ProviderError> {
        let ladders = self.inner.ladders().ok_or(ProviderError::NotFound)?;
        match Continuous::front_of(symbol) {
            Some((spelled, front)) => Ok(Box::new(follow(self.inner.clone(), spelled, ladders.watch(&front, tick_size, sink)?))),
            None => ladders.watch(symbol, tick_size, sink),
        }
    }
}

#[cfg(any(test, feature = "fakes"))]
pub mod fake;
#[cfg(test)]
mod tests;
