//! Rithmic's volume at price: minute ladders from the history plant, and
//! live ones folded from the trades the session already hears.
//!
//! History is the plant's volume-profile minute bars as they come: bid and
//! ask volume and trade counts at each price, minute by minute, nothing
//! rebuilt from ticks. Live, each trade on the front month goes into a
//! [`Tape`] — the session's trade buffer and the forming minute — through a
//! watch on the one subscription the session holds for the contract: a
//! chart's bars and its ladders are two listeners on one feed, and Rithmic
//! is asked for the contract's trades once.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};

use crate::exchanges::registry;
use crate::ladders::{to_tick, Ladder, Minute, Side};
use crate::provider::{LadderSink, LadderUpdate, Ladders, ProviderError, Subscription};
use crate::ticks::{Tape, Trade, DEFAULT_CAPACITY};

use super::{exchange_of, map_err, session, Rithmic, CME_GROUP};

/// How far back the history plant's minute ladders go, in days. On Rithmic
/// Test a month asked came back as about fifteen days.
const HISTORY_DAYS: u32 = 30;

/// A minute from the plant, as a ladder of whole ticks.
pub fn minute_of(found: &rithmic::LadderMinute, tick_size: f64) -> Minute {
    let mut ladder = Ladder::with_capacity(found.prices.len());
    for (i, price) in found.prices.iter().enumerate() {
        let tick = to_tick(*price, tick_size);
        let at = |values: &[u64]| values.get(i).copied().unwrap_or(0);
        let count = |values: &[u32]| values.get(i).copied().unwrap_or(0);
        for (size, side, trades) in [
            (at(&found.bid), Side::Sell, count(&found.bid_trades)),
            (at(&found.ask), Side::Buy, count(&found.ask_trades)),
            (at(&found.unknown), Side::Unknown, count(&found.unknown_trades)),
        ] {
            if size > 0 || trades > 0 {
                ladder.add_trade(tick, size, side, trades);
            }
        }
    }
    // The bar's volume is the plant's; anything it counts that no price
    // carries is volume with no known side.
    let priced = ladder.sides.volume();
    ladder.sides.unknown += found.volume.saturating_sub(priced);
    Minute {
        ts: found.start,
        open: found.open,
        high: found.high,
        low: found.low,
        close: found.close,
        volume: found.volume.max(priced),
        ladder,
    }
}

fn side_of(trade: &rithmic::Trade) -> Side {
    match trade.aggressor {
        Some(rithmic::Side::Buy) => Side::Buy,
        Some(rithmic::Side::Sell) => Side::Sell,
        None => Side::Unknown,
    }
}

/// What the watch's thread hears.
enum Event {
    Live(rithmic::Live),
    Stop,
}

struct Watching {
    stop: Arc<AtomicBool>,
    events: mpsc::Sender<Event>,
}

impl Subscription for Watching {}

impl Drop for Watching {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        let _ = self.events.send(Event::Stop);
    }
}

/// Fold what the session hears into `tape`, and say so: the forming minute
/// after each print, a minute as it seals, and a drop — or a start over
/// after prints were missed — as a drop, followed by the feed resumed once
/// the session is watching again.
fn fold(events: &mpsc::Receiver<Event>, mut tape: Tape, say: &dyn Fn(LadderUpdate), stop: &AtomicBool) {
    let tick_size = tape.forming.tick_size();
    // Heard from the session at all: a Watching after this is a return.
    let mut started = false;
    for event in events {
        if stop.load(Ordering::Acquire) {
            return;
        }
        match event {
            Event::Stop => return,
            Event::Live(rithmic::Live::Trade(trade)) => {
                let print = Trade {
                    ts_nanos: trade.at_nanos,
                    price_tick: to_tick(trade.price, tick_size),
                    size: trade.size,
                    side: side_of(&trade),
                };
                if let Some(sealed) = tape.push(print) {
                    say(LadderUpdate::Sealed { minute: sealed, covered: tape.forming.covered() });
                }
                if let Some(forming) = tape.forming.current() {
                    say(LadderUpdate::Forming(forming.clone()));
                }
            }
            // The first Watching starts the feed; a later one, or one after
            // a loss, is the session starting over, after a reconnect or
            // missed prints, and what it missed can be asked for now.
            Event::Live(rithmic::Live::Watching { .. }) => {
                if tape.forming.is_connected() || tape.forming.current().is_some() {
                    say(LadderUpdate::Dropped(tape.forming.disconnect()));
                }
                if std::mem::replace(&mut started, true) {
                    say(LadderUpdate::Resumed);
                }
            }
            // Volume at a price is counted from the prints heard: the one a
            // subscription opens with may be in a minute the plant has.
            Event::Live(rithmic::Live::Last(_)) => {}
            Event::Live(rithmic::Live::Lost(_)) => {
                started = true;
                if tape.forming.is_connected() || tape.forming.current().is_some() {
                    say(LadderUpdate::Dropped(tape.forming.disconnect()));
                }
            }
        }
    }
}

impl Ladders for Rithmic {
    /// Every contract on a CME group exchange: the history plant has
    /// volume-profile minute bars for any of them.
    fn serves(&self, symbol: &str) -> bool {
        registry().contract_exchange(crate::specs::root_of(symbol)).is_some_and(|exchange| CME_GROUP.contains(&exchange))
    }

    fn history_days(&self) -> u32 {
        HISTORY_DAYS
    }

    fn minutes(&self, symbol: &str, tick_size: f64, from: i64, to: i64) -> Result<Vec<Minute>, ProviderError> {
        if !session::here() {
            return Err(ProviderError::NeedsSetup(
                "Rithmic ladders are served by the open window, which holds the one session".into(),
            ));
        }
        let client = session::client().map_err(map_err)?;
        let exchange = exchange_of(symbol);
        // A dated contract is its own; a root is its front month.
        let contract = match crate::specs::root_of(symbol) == symbol {
            true => client.front_month(symbol, exchange).map_err(map_err)?,
            false => symbol.to_string(),
        };
        let found = client.ladders(&contract, exchange, from, to).map_err(map_err)?;
        Ok(found.iter().map(|m| minute_of(m, tick_size)).collect())
    }

    fn watch(&self, symbol: &str, tick_size: f64, sink: LadderSink) -> Result<Box<dyn Subscription>, ProviderError> {
        let stop = Arc::new(AtomicBool::new(false));
        let (events, inbox) = mpsc::channel();
        let sender = events.clone();
        let stopping = stop.clone();
        let root = symbol.to_string();
        std::thread::Builder::new()
            .name("omacharts rithmic ladders".into())
            .spawn(move || {
                let say = |update: LadderUpdate| {
                    if !stopping.load(Ordering::Acquire) {
                        sink(update);
                    }
                };
                let Ok(client) = session::client() else { return };
                let hear = move |live| {
                    let _ = sender.send(Event::Live(live));
                };
                let _watch = match crate::specs::root_of(&root) == root {
                    true => client.watch(&root, exchange_of(&root), hear),
                    false => client.watch_contract(&root, exchange_of(&root), hear),
                };
                fold(&inbox, Tape::new(tick_size, DEFAULT_CAPACITY), &say, &stopping);
            })
            .map_err(|error| ProviderError::Network(error.to_string()))?;
        Ok(Box::new(Watching { stop, events }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn trade(seconds: i64, price: f64, size: u64, aggressor: Option<rithmic::Side>) -> Event {
        Event::Live(rithmic::Live::Trade(rithmic::Trade { price, size, aggressor, at_nanos: seconds * 1_000_000_000 }))
    }

    /// A plant minute becomes a ladder of whole ticks, each side where the
    /// plant put it, and volume no price carries is volume with no side.
    #[test]
    fn a_plant_minute_is_a_ladder_of_whole_ticks() {
        let found = rithmic::LadderMinute {
            start: 600,
            open: 7625.0,
            high: 7625.5,
            low: 7625.0,
            close: 7625.25,
            volume: 40,
            prices: vec![7625.0, 7625.25, 7625.5],
            bid: vec![10, 0, 3],
            ask: vec![2, 15, 0],
            unknown: vec![0, 0, 0],
            bid_trades: vec![4, 0, 1],
            ask_trades: vec![1, 6, 0],
            unknown_trades: vec![0, 0, 0],
        };
        let minute = minute_of(&found, 0.25);
        assert_eq!(minute.ts, 600);
        assert_eq!(minute.ladder.low_tick, 30_500);
        assert_eq!(minute.ladder.at(30_500).at_bid, 10);
        assert_eq!(minute.ladder.at(30_501).at_ask, 15);
        assert_eq!(minute.ladder.at(30_502).bid_trades, 1);
        assert_eq!((minute.ladder.sides.at_bid, minute.ladder.sides.at_ask, minute.ladder.sides.unknown), (13, 17, 10));
        assert_eq!(minute.volume, 40);
    }

    /// Trades go into the forming minute, which seals when one arrives for
    /// the next; the feed starting over is a drop, so the minute it was in
    /// is never taken as complete.
    #[test]
    fn trades_build_minutes_and_a_start_over_is_a_drop() {
        let (send, events) = mpsc::channel();
        let watching = || Event::Live(rithmic::Live::Watching { contract: "ESZ6".into() });
        send.send(watching()).unwrap();
        send.send(trade(600, 7625.0, 2, Some(rithmic::Side::Buy))).unwrap();
        send.send(trade(660, 7625.0, 1, Some(rithmic::Side::Sell))).unwrap();
        send.send(trade(725, 7625.25, 3, Some(rithmic::Side::Buy))).unwrap();
        send.send(trade(726, 7625.25, 1, None)).unwrap();
        send.send(watching()).unwrap();
        send.send(Event::Stop).unwrap();
        let heard = std::cell::RefCell::new(Vec::new());
        fold(&events, Tape::new(0.25, 1_000), &|u| heard.borrow_mut().push(u), &AtomicBool::new(false));
        let heard = heard.into_inner();
        let sealed: Vec<(i64, Option<(i64, i64)>)> = heard
            .iter()
            .filter_map(|u| match u {
                LadderUpdate::Sealed { minute, covered } => Some((minute.ts, *covered)),
                _ => None,
            })
            .collect();
        // Joined on the dot at 600, so 600 on is complete.
        assert_eq!(sealed, vec![(600, Some((600, 660))), (660, Some((600, 720)))]);
        let [.., LadderUpdate::Dropped(Some(partial)), LadderUpdate::Resumed] = heard.as_slice() else {
            panic!("{heard:?}")
        };
        assert_eq!((partial.ts, partial.volume, partial.ladder.sides.unknown), (720, 4, 1));
        assert!(heard.iter().any(|u| matches!(u, LadderUpdate::Forming(m) if m.ts == 720)));
        assert_eq!(heard.iter().filter(|u| **u == LadderUpdate::Resumed).count(), 1, "the first Watching is no return");
    }

    /// A drop is a drop at once, and the session watching again is the feed
    /// resumed: the cue to ask for what it missed, said once.
    #[test]
    fn coming_back_after_a_drop_is_the_feed_resumed() {
        let (send, events) = mpsc::channel();
        let watching = || Event::Live(rithmic::Live::Watching { contract: "ESZ6".into() });
        send.send(watching()).unwrap();
        send.send(trade(600, 7625.0, 2, Some(rithmic::Side::Buy))).unwrap();
        send.send(Event::Live(rithmic::Live::Lost(rithmic::Error::Connection("connection closed".into())))).unwrap();
        send.send(watching()).unwrap();
        send.send(Event::Stop).unwrap();
        let heard = std::cell::RefCell::new(Vec::new());
        fold(&events, Tape::new(0.25, 1_000), &|u| heard.borrow_mut().push(u), &AtomicBool::new(false));
        let heard = heard.into_inner();
        let [LadderUpdate::Forming(_), LadderUpdate::Dropped(Some(partial)), LadderUpdate::Resumed] = heard.as_slice()
        else {
            panic!("{heard:?}")
        };
        assert_eq!(partial.ts, 600);
    }
}
