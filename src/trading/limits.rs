//! Each account's own limits, kept by the app rather than the trading
//! service: a day's profit target and its largest loss, the most contracts
//! held at once, and the most a single trade may risk to its stop.
//!
//! They are for an agent trading from the command line, so they are
//! written so that it cannot argue its way past them:
//!
//! - Once the day's P&L reaches the profit target or the loss, the account
//!   takes no new entries until the next trading day. Closing what is held
//!   is always allowed.
//! - A limit changed holds at once, tighter or looser. Raising or clearing
//!   the daily limit that was reached lifts the day's stop on entries,
//!   unless the day's P&L still reaches what it now is.
//!
//! A trading day is CME's: it starts at 5 p.m. in Chicago.
//!
//! The limits are a setting of this machine's, [`LIMITS`]; the day a limit
//! was reached is the app's note, [`HITS`]. Both are keyed by account, as
//! `login/number`.

use std::collections::BTreeMap;

use chrono::{DateTime, Duration, NaiveDate, Utc};
use serde::{Deserialize, Serialize};

use crate::store::Store;

/// The setting the limits are kept in.
pub const LIMITS: &str = "trading_limits";

/// The setting the days a daily limit was reached are noted in.
pub const HITS: &str = "trading_limit_hits";

/// One account's limits. A limit not set is no limit.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Caps {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub daily_profit: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub daily_loss: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_qty: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_risk: Option<f64>,
}

impl Caps {
    pub fn is_empty(&self) -> bool {
        *self == Caps::default()
    }
}

/// What is kept of one account: its limits. Kept as it was when a looser
/// limit waited for the next trading day, `later` being that limit, the
/// last one asked for and so the one that holds.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
struct Kept {
    #[serde(default)]
    now: Caps,
    #[serde(default, skip_serializing)]
    later: Option<Later>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct Later {
    caps: Caps,
}

/// The trading day `at` falls in: CME's, which starts at 5 p.m. Chicago
/// time, named by the date it ends on.
pub fn trading_day(at: DateTime<Utc>) -> NaiveDate {
    let chicago = at.with_timezone(&chrono_tz::America::Chicago);
    (chicago + Duration::hours(7)).date_naive()
}

/// Every account's limits.
pub fn load(store: &Store) -> BTreeMap<String, Caps> {
    let kept: BTreeMap<String, Kept> =
        store.setting(LIMITS).and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default();
    kept.into_iter().map(|(account, kept)| (account, kept.later.map_or(kept.now, |later| later.caps))).collect()
}

fn save(store: &Store, all: &BTreeMap<String, Caps>) {
    let kept: BTreeMap<&String, Kept> = all.iter().map(|(account, caps)| (account, Kept { now: *caps, later: None })).collect();
    match kept.is_empty() {
        true => store.remove_setting(LIMITS),
        false => {
            if let Ok(text) = serde_json::to_string(&kept) {
                store.set_setting(LIMITS, &text);
            }
        }
    }
}

/// The limits holding on `account`.
pub fn holding(store: &Store, account: &str) -> Caps {
    load(store).get(account).copied().unwrap_or_default()
}

/// `account`'s limits set to `asked`, every one of them, from now: a limit
/// left out is asked to be no limit. A daily limit reached on `day` that
/// `asked` raises or clears no longer stops entries; if the day's P&L
/// still reaches it, it is noted again as soon as the P&L is next heard.
pub fn set(store: &Store, account: &str, asked: Caps, day: NaiveDate) {
    let mut all = load(store);
    match asked.is_empty() {
        true => all.remove(account),
        false => all.insert(account.to_string(), asked),
    };
    save(store, &all);
    if let Some(hit) = hit_on(store, account, day)
        && reached(&asked, hit.pnl) != Some(hit.limit.as_str())
    {
        forget_hit(store, account);
    }
}

/// A daily limit reached: which, at what P&L.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Hit {
    pub day: NaiveDate,
    /// `daily-profit` or `daily-loss`.
    pub limit: String,
    pub pnl: f64,
}

/// The daily limit `account` reached on `day`, if it did.
pub fn hit_on(store: &Store, account: &str, day: NaiveDate) -> Option<Hit> {
    let all: BTreeMap<String, Hit> = store.setting(HITS).and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default();
    all.get(account).filter(|hit| hit.day == day).cloned()
}

/// Note that `account` reached `hit`. Notes from past days are dropped.
pub fn note_hit(store: &Store, account: &str, hit: &Hit) {
    let mut all: BTreeMap<String, Hit> = store.setting(HITS).and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default();
    all.retain(|_, kept| kept.day >= hit.day);
    all.insert(account.to_string(), hit.clone());
    if let Ok(text) = serde_json::to_string(&all) {
        store.set_setting(HITS, &text);
    }
}

/// `account`'s note of a daily limit reached, gone.
fn forget_hit(store: &Store, account: &str) {
    let mut all: BTreeMap<String, Hit> = store.setting(HITS).and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default();
    if all.remove(account).is_none() {
        return;
    }
    match all.is_empty() {
        true => store.remove_setting(HITS),
        false => {
            if let Ok(text) = serde_json::to_string(&all) {
                store.set_setting(HITS, &text);
            }
        }
    }
}

/// Which daily limit a day's P&L of `pnl` has reached, if any.
pub fn reached(caps: &Caps, pnl: f64) -> Option<&'static str> {
    if caps.daily_loss.is_some_and(|loss| pnl <= -loss.abs()) {
        return Some("daily-loss");
    }
    if caps.daily_profit.is_some_and(|profit| pnl >= profit) {
        return Some("daily-profit");
    }
    None
}

/// An order as the limits see it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Entry {
    pub qty: u32,
    /// Contracts held on the contract before it, signed: long is positive.
    pub held: i32,
    /// Contracts it buys, signed: a sell is negative.
    pub signed: i32,
    /// What it risks to its stop, in money, if it has a stop.
    pub risk: Option<f64>,
    /// Sent with exits: once it fills they work, and either one filling on
    /// its own opens a position, so it is never only a close.
    pub bracket: bool,
}

impl Entry {
    /// Only takes off what is held: allowed whatever the limits.
    pub fn closes(&self) -> bool {
        !self.bracket && self.held != 0 && self.signed.signum() == -self.held.signum() && self.signed.abs() <= self.held.abs()
    }
}

/// Why `entry` may not go, or nothing when it may. `hit` is a daily limit
/// already reached today, `pnl` the day's P&L as last heard.
pub fn refuse(caps: &Caps, hit: Option<&Hit>, pnl: Option<f64>, entry: &Entry) -> Option<String> {
    if entry.closes() {
        return None;
    }
    if let Some(hit) = hit {
        return Some(format!(
            "the {} limit was reached today at a P&L of {}: no new entries until the next trading day",
            hit.limit.replace('-', " "),
            money(hit.pnl)
        ));
    }
    if let Some(limit) = pnl.and_then(|pnl| reached(caps, pnl)) {
        return Some(format!(
            "the {} limit is reached at a P&L of {}: no new entries until the next trading day",
            limit.replace('-', " "),
            money(pnl.unwrap_or_default())
        ));
    }
    if let Some(max) = caps.max_qty {
        if entry.qty > max {
            return Some(format!("{} contracts is more than the {max} the account's limit allows", entry.qty));
        }
        let after = (entry.held + entry.signed).unsigned_abs();
        if after > max {
            return Some(format!("that would hold {after} contracts, more than the {max} the account's limit allows"));
        }
    }
    if let Some(max) = caps.max_risk {
        match entry.risk {
            None => {
                return Some(format!("the account risks at most {} a trade: an entry needs a stop-loss to count its risk", money(max)));
            }
            Some(risk) if risk > max + 1e-9 => {
                return Some(format!("the stop risks {}, more than the {} a trade the account's limit allows", money(risk), money(max)));
            }
            Some(_) => {}
        }
    }
    None
}

fn money(amount: f64) -> String {
    let sign = if amount < 0.0 { "-" } else { "" };
    format!("{sign}${:.2}", amount.abs())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day(text: &str) -> NaiveDate {
        NaiveDate::parse_from_str(text, "%Y-%m-%d").unwrap()
    }

    fn entry(qty: u32, held: i32, signed: i32, risk: Option<f64>) -> Entry {
        Entry { qty, held, signed, risk, bracket: false }
    }

    #[test]
    fn the_trading_day_starts_at_five_in_chicago() {
        let at = |text: &str| DateTime::parse_from_rfc3339(text).unwrap().with_timezone(&Utc);
        // 4:59 p.m. CST on the 16th is still the 16th's day; 5 p.m. is the 17th's.
        assert_eq!(trading_day(at("2026-11-16T22:59:00Z")), day("2026-11-16"));
        assert_eq!(trading_day(at("2026-11-16T23:00:00Z")), day("2026-11-17"));
        assert_eq!(trading_day(at("2026-11-17T03:00:00Z")), day("2026-11-17"));
    }

    #[test]
    fn a_limit_changed_holds_at_once_tighter_or_looser() {
        let store = Store::memory().expect("a store");
        let today = day("2026-11-16");
        set(&store, "L/1", Caps { max_qty: Some(2), daily_loss: Some(1200.0), ..Caps::default() }, today);
        assert_eq!(holding(&store, "L/1").max_qty, Some(2));
        set(&store, "L/1", Caps { max_qty: Some(1), daily_loss: Some(1200.0), ..Caps::default() }, today);
        assert_eq!(holding(&store, "L/1").max_qty, Some(1), "tighter, at once");
        set(&store, "L/1", Caps { max_qty: Some(5), ..Caps::default() }, today);
        assert_eq!(holding(&store, "L/1"), Caps { max_qty: Some(5), ..Caps::default() }, "looser and cleared, at once too");
    }

    #[test]
    fn limits_kept_waiting_for_tomorrow_hold_now() {
        let store = Store::memory().expect("a store");
        store.set_setting(LIMITS, r#"{"L/1":{"now":{"max_qty":1},"later":{"from":"2026-11-17","caps":{"max_qty":5}}}}"#);
        assert_eq!(holding(&store, "L/1").max_qty, Some(5));
    }

    #[test]
    fn raising_or_clearing_the_limit_reached_lifts_the_day_s_stop() {
        let store = Store::memory().expect("a store");
        let today = day("2026-11-16");
        set(&store, "L/1", Caps { daily_loss: Some(1200.0), ..Caps::default() }, today);
        let hit = Hit { day: today, limit: "daily-loss".into(), pnl: -1250.0 };
        note_hit(&store, "L/1", &hit);
        set(&store, "L/1", Caps { daily_loss: Some(1000.0), max_qty: Some(1), ..Caps::default() }, today);
        assert_eq!(hit_on(&store, "L/1", today), Some(hit.clone()), "still reached: tighter");
        set(&store, "L/1", Caps { daily_loss: Some(2000.0), ..Caps::default() }, today);
        assert_eq!(hit_on(&store, "L/1", today), None, "raised past the P&L");
        note_hit(&store, "L/1", &hit);
        set(&store, "L/1", Caps::default(), today);
        assert_eq!(hit_on(&store, "L/1", today), None, "cleared");
    }

    #[test]
    fn closing_is_always_allowed() {
        let caps = Caps { max_qty: Some(1), max_risk: Some(100.0), ..Caps::default() };
        let hit = Hit { day: day("2026-11-16"), limit: "daily-loss".into(), pnl: -1300.0 };
        assert!(refuse(&caps, Some(&hit), Some(-1300.0), &entry(2, 2, -2, None)).is_none());
        assert!(refuse(&caps, Some(&hit), Some(-1300.0), &entry(1, -1, 1, None)).is_none());
        assert!(refuse(&caps, Some(&hit), None, &entry(1, 1, 1, Some(10.0))).is_some(), "adding is an entry");
        assert!(refuse(&caps, None, None, &entry(3, 1, -3, Some(10.0))).is_some(), "reversing is an entry");
        let bracketed = Entry { bracket: true, ..entry(2, 2, -2, Some(10.0)) };
        assert!(refuse(&caps, Some(&hit), Some(-1300.0), &bracketed).is_some(), "its exits would open one");
    }

    #[test]
    fn a_daily_limit_refuses_entries_for_the_day() {
        let caps = Caps { daily_profit: Some(1500.0), daily_loss: Some(1200.0), ..Caps::default() };
        assert!(refuse(&caps, None, Some(-1199.0), &entry(1, 0, 1, None)).is_none());
        assert!(refuse(&caps, None, Some(-1200.0), &entry(1, 0, 1, None)).unwrap().contains("daily loss"));
        assert!(refuse(&caps, None, Some(1500.0), &entry(1, 0, 1, None)).unwrap().contains("daily profit"));
        assert_eq!(reached(&caps, -1250.0), Some("daily-loss"));
        assert_eq!(reached(&caps, 0.0), None);
    }

    #[test]
    fn quantity_and_risk_are_counted() {
        let caps = Caps { max_qty: Some(2), max_risk: Some(200.0), ..Caps::default() };
        assert!(refuse(&caps, None, None, &entry(2, 0, 2, Some(200.0))).is_none());
        assert!(refuse(&caps, None, None, &entry(3, 0, 3, Some(10.0))).unwrap().contains("3 contracts"));
        assert!(refuse(&caps, None, None, &entry(1, 2, 1, Some(10.0))).unwrap().contains("hold 3"));
        assert!(refuse(&caps, None, None, &entry(1, 0, 1, None)).unwrap().contains("needs a stop-loss"));
        assert!(refuse(&caps, None, None, &entry(1, 0, 1, Some(250.0))).unwrap().contains("$250.00"));
    }

    #[test]
    fn limits_and_hits_are_kept() {
        let store = Store::memory().expect("a store");
        let today = day("2026-11-16");
        set(&store, "L/1", Caps { max_qty: Some(1), ..Caps::default() }, today);
        assert_eq!(holding(&store, "L/1").max_qty, Some(1));
        assert!(holding(&store, "L/2").is_empty());
        let hit = Hit { day: today, limit: "daily-loss".into(), pnl: -1201.0 };
        note_hit(&store, "L/1", &hit);
        assert_eq!(hit_on(&store, "L/1", today), Some(hit));
        assert_eq!(hit_on(&store, "L/1", day("2026-11-17")), None);
        // Cleared, it is gone at once.
        set(&store, "L/1", Caps::default(), today);
        assert!(holding(&store, "L/1").is_empty());
        assert!(store.setting(LIMITS).is_none());
    }
}
