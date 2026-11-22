//! A second rich feed that exists only to prove the boundary holds.
//!
//! It has what Rithmic has — bars, a stream, minute ladders and a sign-in
//! typed into a dialog — and none of it is named anywhere above this folder.
//! It is found by [`crate::providers::listed`] and never offered, and only
//! in a build with the `fakes` feature, which is a test build: the tests
//! that route a chart to it and draw its row and open its dialog on the
//! Data page do so through the same code every real feed goes through, and the
//! day one of them needs a line naming this feed, the boundary has a hole.
//!
//! Its bars and its stream are [`Synthetic`]'s, and its ladders are made
//! from them: each minute's trades are invented, the same every time, to run
//! from the minute's open to its close through its high and low, with now
//! and then a sweep of several prints at one moment and an order far bigger
//! than the rest — what the order flow indicators are drawn from, for the
//! manual's pictures and the tests. What is signed in is held in memory, and
//! so is its one secret.

use std::sync::{Mutex, RwLock};

use crate::bars::{Bar, Timeframe};
use crate::ladders::{to_price, to_tick, Minute, Side};
use crate::studies::orders::{Aggressor, Print};
use crate::provider::{
    Capability, LadderSink, Ladders, Provider, ProviderError, Stream, Subscription,
};
use crate::providers::{Access, Field, How, Listed, Saved, Secrets, SignIn, Synthetic};
use crate::symbols::{Instrument, InstrumentKind};

/// How the fake is listed.
pub const LISTED: Listed = Listed {
    id: "fake",
    label: "Fake Rich Feed",
    serves: "Futures, invented, from a login typed into a dialog",
    covers: "Futures, invented",
    build: |_| Box::new(Fake::new()),
    setup: Some(&Signing),
    experimental: false,
    gate: None,
    login: None,
};

pub struct Fake {
    bars: Synthetic,
}

/// Milliseconds between the forming bar's moves, when set: a market that is
/// open, for measuring what follows each tick. Unset, the bars stand still.
pub const TICK_ENV: &str = "OMACHARTS_FAKE_TICK_MS";

impl Fake {
    pub fn new() -> Fake {
        let tick = std::env::var(TICK_ENV)
            .ok()
            .and_then(|ms| ms.trim().parse::<u64>().ok())
            .filter(|ms| *ms > 0)
            .map(std::time::Duration::from_millis);
        Fake { bars: Synthetic::new(tick) }
    }
}

impl Default for Fake {
    fn default() -> Fake {
        Fake::new()
    }
}

impl Provider for Fake {
    fn id(&self) -> &'static str {
        LISTED.id
    }

    fn label(&self) -> &'static str {
        LISTED.label
    }

    fn delay_minutes(&self, _: InstrumentKind) -> u32 {
        0
    }

    /// Futures roots only, so that a stock still goes to the next feed.
    fn native_symbol(&self, instrument: &Instrument) -> Option<String> {
        (instrument.kind == InstrumentKind::FutureRoot).then(|| instrument.symbol.clone())
    }

    fn capabilities(&self) -> &'static [Capability] {
        self.bars.capabilities()
    }

    fn bars(&self, symbol: &str, timeframe: Timeframe, since: Option<i64>) -> Result<Vec<Bar>, ProviderError> {
        self.bars.bars(symbol, timeframe, since)
    }

    fn stream(&self) -> Option<&dyn Stream> {
        self.bars.stream()
    }

    fn ladders(&self) -> Option<&dyn Ladders> {
        Some(self)
    }

    /// The rule's next three contracts, each expiring a day later than the
    /// rule says: enough for a test to tell a feed's listing from the rule's.
    fn contracts(&self, root: &str) -> Result<Vec<(String, chrono::NaiveDate)>, ProviderError> {
        let today = crate::contracts::today();
        Ok(crate::contracts::listing(root, today)
            .into_iter()
            .take(3)
            .map(|listed| (listed.symbol, listed.expires + chrono::Days::new(1)))
            .collect())
    }
}

impl Ladders for Fake {
    fn serves(&self, _: &str) -> bool {
        true
    }

    fn history_days(&self) -> u32 {
        5
    }

    /// The synthetic minutes of the span, each with the trades
    /// [`trades_of`] invents for it.
    fn minutes(&self, symbol: &str, tick_size: f64, from: i64, to: i64) -> Result<Vec<Minute>, ProviderError> {
        let bars = self.bars.bars(symbol, Timeframe::minutes(1), Some(from))?;
        Ok(bars
            .iter()
            .filter(|bar| (from..to).contains(&bar.ts))
            .filter_map(|bar| minute_of(symbol, bar, tick_size))
            .collect())
    }

    fn watch(&self, _: &str, _: f64, _: LadderSink) -> Result<Box<dyn Subscription>, ProviderError> {
        Ok(Box::new(Watching))
    }

    fn has_prints(&self, _: &str) -> bool {
        true
    }

    /// The trades [`trades_of`] invents for each synthetic minute of the
    /// span, the same every time.
    fn prints(&self, symbol: &str, tick_size: f64, from: i64, to: i64) -> Result<Vec<Print>, ProviderError> {
        let bars = self.bars.bars(symbol, Timeframe::minutes(1), Some(from))?;
        let mut out = Vec::new();
        for bar in bars.iter().filter(|bar| (from..to).contains(&bar.ts)) {
            trades_of(symbol, bar, tick_size, |ts, tick, size, side| {
                let side = match side {
                    Side::Buy => Aggressor::Buy,
                    Side::Sell => Aggressor::Sell,
                    Side::Unknown => Aggressor::Unknown,
                };
                out.push(Print::new(ts, tick, size, side, false));
            });
        }
        Ok(out)
    }
}

/// A synthetic minute bar as a minute with its ladder: the trades
/// [`trades_of`] invents for it, folded together.
fn minute_of(symbol: &str, bar: &Bar, tick_size: f64) -> Option<Minute> {
    let mut minute: Option<Minute> = None;
    trades_of(symbol, bar, tick_size, |_, tick, size, side| {
        let price = to_price(tick, tick_size);
        minute.get_or_insert_with(|| Minute::opening(bar.ts, price)).add_trade(tick, price, size, side, 1);
    });
    minute
}

/// The trades of one synthetic bar, oldest first, as unix nanoseconds, a
/// whole tick of `tick_size`, a size and a side: the same every time for
/// the same bar. They run from the open to the nearer extreme, to the other,
/// and on to the close, a tick or so of noise either way; a print a tick
/// above the one before was bought, a tick below sold, and one at the same
/// price went the way the bar did, mostly. About one trade in thirty is a
/// sweep of two to four prints at one moment on one side, and one in a
/// thousand five hundred an order many times the usual size.
pub fn trades_of(symbol: &str, bar: &Bar, tick_size: f64, mut each: impl FnMut(i64, i32, u64, Side)) {
    if tick_size.is_nan() || tick_size <= 0.0 || !bar.open.is_finite() || !bar.close.is_finite() {
        return;
    }
    let mut state = symbol.bytes().fold(bar.ts as u64 ^ 0x9e37_79b9_7f4a_7c15, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3)
    });
    let mut next = move |below: u64| {
        state = state.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        (state >> 33) % below.max(1)
    };
    let (open, close) = (to_tick(bar.open, tick_size), to_tick(bar.close, tick_size));
    let (high, low) = (to_tick(bar.high, tick_size), to_tick(bar.low, tick_size));
    let rising = close >= open;
    let (first, second) = if rising { (low, high) } else { (high, low) };
    let legs = [(open, first), (first, second), (second, close)];
    let count = ((bar.volume / 20.0) as u64).clamp(12, 900);
    let per_leg = (count / 3).max(1);
    let step = 60_000_000_000 / (per_leg * 3 + 1) as i64;
    let mut ts = bar.ts * 1_000_000_000;
    let mut last = open;
    for (from, to) in legs {
        for i in 0..per_leg {
            ts += step;
            let along = from as f64 + (to - from) as f64 * (i + 1) as f64 / per_leg as f64;
            let tick = (along.round() as i32 + next(3) as i32 - 1).clamp(low, high);
            let side = match tick.cmp(&last) {
                std::cmp::Ordering::Greater => Side::Buy,
                std::cmp::Ordering::Less => Side::Sell,
                std::cmp::Ordering::Equal if (next(10) < 6) == rising => Side::Buy,
                std::cmp::Ordering::Equal => Side::Sell,
            };
            last = tick;
            let size = if next(1_500) == 0 { 60 + next(340) } else { 1 + next(6) + next(6) * next(2) };
            if next(30) == 0 {
                // A sweep: one order through several prices at one moment.
                let fills = 2 + next(3) as i32;
                let way = if side == Side::Buy { 1 } else { -1 };
                for fill in 0..fills {
                    each(ts, (tick + fill * way).clamp(low, high), size / fills as u64 + 1, side);
                }
            } else {
                each(ts, tick, size, side);
            }
        }
    }
}

struct Watching;

impl Subscription for Watching {}

static SAVED: RwLock<Option<Saved>> = RwLock::new(None);

/// Its secrets, in memory: the fake's keyring.
pub static KEPT: Memory = Memory(Mutex::new(Vec::new()));

/// Passwords by login, for nothing but this.
pub struct Memory(Mutex<Vec<(String, String)>>);

impl Memory {
    /// Is a secret kept for `login`?
    pub fn holds(&self, login: &str) -> bool {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).iter().any(|(known, _)| known == login)
    }
}

impl Secrets for Memory {
    fn save(&self, login: &str, _: &str, _: &str, password: &str) -> Result<(), String> {
        let mut kept = self.0.lock().unwrap_or_else(|e| e.into_inner());
        kept.retain(|(known, _)| known != login);
        kept.push((login.to_string(), password.to_string()));
        Ok(())
    }

    fn forget(&self, login: &str, _: &str, _: &str) -> Result<(), String> {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).retain(|(known, _)| known != login);
        Ok(())
    }
}

fn saved() -> Saved {
    SAVED.read().unwrap_or_else(|e| e.into_inner()).clone().unwrap_or_default()
}

#[derive(Debug)]
pub struct Signing;

impl SignIn for Signing {
    fn how(&self) -> How {
        How::Dialog
    }

    fn access(&self) -> Access {
        match saved().user {
            Some(_) => Access::Signed(saved().value("desk").unwrap_or("no desk").to_string()),
            None => Access::Missing,
        }
    }

    fn can_sign_in(&self) -> Result<String, String> {
        Ok("the token in memory".into())
    }

    fn sign_in(&self, log: Box<dyn Fn(&str) + Send + Sync>) -> Result<(), String> {
        let saved = saved();
        match (saved.user, saved.login) {
            (Some(_), Some(login)) if KEPT.holds(&login) => {
                log("found the token");
                Ok(())
            }
            _ => Err("no token is kept".into()),
        }
    }

    fn sign_out(&self) -> Result<(), String> {
        if let Some(login) = saved().login {
            KEPT.forget(&login, "", "")?;
        }
        Ok(())
    }

    fn connected(&self) -> bool {
        false
    }

    fn places(&self) -> Vec<(&'static str, String)> {
        vec![("Memory", "this process".into())]
    }

    fn remember(&self, saved: Saved) {
        *SAVED.write().unwrap_or_else(|e| e.into_inner()) = Some(saved);
    }

    fn keyring(&self) -> Option<&'static dyn Secrets> {
        Some(&KEPT)
    }

    /// Kept by the app, as a feed set up in a dialog of its own is.
    fn fields(&self) -> &'static [Field] {
        &[Field { key: "desk", label: "Desk" }]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bar() -> Bar {
        Bar { ts: 1_793_700_000, open: 100.0, high: 101.5, low: 99.25, close: 101.0, volume: 4_000.0 }
    }

    fn all(symbol: &str) -> Vec<(i64, i32, u64, Side)> {
        let mut out = Vec::new();
        trades_of(symbol, &bar(), 0.25, |ts, tick, size, side| out.push((ts, tick, size, side)));
        out
    }

    #[test]
    fn a_bars_trades_are_the_same_every_time_and_inside_it() {
        let trades = all("ES");
        assert_eq!(trades, all("ES"));
        assert_ne!(trades, all("NQ"));
        assert!(trades.len() >= 12);
        assert!(trades.iter().all(|(_, tick, _, _)| (397..=406).contains(tick)));
        assert!(trades.windows(2).all(|w| w[0].0 <= w[1].0), "oldest first");
        assert!(trades.iter().all(|(ts, ..)| (bar().ts * 1_000_000_000..(bar().ts + 60) * 1_000_000_000).contains(ts)));
        assert!(trades.iter().any(|(_, _, _, side)| *side == Side::Buy));
        assert!(trades.iter().any(|(_, _, _, side)| *side == Side::Sell));
    }

    #[test]
    fn its_minutes_hold_what_traded_in_them() {
        let minute = minute_of("ES", &bar(), 0.25).unwrap();
        let volume: u64 = all("ES").iter().map(|t| t.2).sum();
        assert_eq!(minute.volume, volume);
        assert_eq!(minute.ladder.total(), volume);
        assert_eq!(minute.ts, bar().ts);
    }
}
