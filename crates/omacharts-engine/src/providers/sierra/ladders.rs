//! Sierra Chart's volume at price, from the trades in its intraday files.
//!
//! Every record of a tick file is one trade with the volume that traded at
//! the bid and the volume that traded at the ask, so a minute ladder here
//! is exact: built from the prints, never estimated from a bar's range.
//! History is the mapped file, a span of it found by binary search and
//! folded a minute at a time — across threads for a long span — and live is
//! the file growing, each flush folded into the forming minute.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::ladders::{to_tick, Forming, Minute, Side};
use crate::provider::{LadderSink, LadderUpdate, Ladders, ProviderError, Subscription};

use super::{mapped, multiplier, not_set_up, opened, read_failed, stem_for, Sierra};

/// How far back a request may go, in days: as far as a file does, which
/// for a quarterly contract is about four months.
const HISTORY_DAYS: u32 = 400;

/// Records spanned before a fold is split across threads.
const PARALLEL_ABOVE: usize = 1 << 20;

/// One record's prints, as ladder trades: what traded at the bid as
/// sellers', at the ask as buyers', and the rest, if a record has any, on
/// no side. `None` for a record with no price.
/// A record as a tick, a price and its volume on each side.
type Prints = (i32, f64, [(u64, Side); 3]);

fn prints(record: &sierra::Record, multiplier: f64, tick_size: f64) -> Option<Prints> {
    let price = f64::from(record.close) * multiplier;
    if !price.is_finite() || price <= 0.0 {
        return None;
    }
    let tick = to_tick(price, tick_size);
    let (bid, ask, all) = (u64::from(record.bid_volume), u64::from(record.ask_volume), u64::from(record.volume));
    let unknown = all.saturating_sub(bid + ask);
    Some((tick, price, [(bid, Side::Sell), (ask, Side::Buy), (unknown, Side::Unknown)]))
}

/// The minutes of `records`, oldest first, in whole ticks of `tick_size`.
fn minutes_of(records: &[sierra::Record], multiplier: f64, tick_size: f64) -> Vec<Minute> {
    let mut minutes: Vec<Minute> = Vec::new();
    for record in records {
        let Some((tick, price, sides)) = prints(record, multiplier, tick_size) else { continue };
        let ts = record.unix_seconds().div_euclid(60) * 60;
        let minute = match minutes.last_mut() {
            Some(minute) if minute.ts == ts => minute,
            _ => {
                minutes.push(Minute::opening(ts, price));
                minutes.last_mut().expect("just pushed")
            }
        };
        // One trade, counted once, on the side that took it.
        let mut counted = false;
        for (size, side) in sides {
            if size > 0 {
                minute.add_trade(tick, price, size, side, u32::from(!counted));
                counted = true;
            }
        }
    }
    minutes
}

/// [`minutes_of`], split across threads at minute boundaries for a span
/// long enough to be worth it.
fn minutes_in(records: &[sierra::Record], multiplier: f64, tick_size: f64) -> Vec<Minute> {
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get()).min(16);
    if records.len() < PARALLEL_ABOVE || threads < 2 {
        return minutes_of(records, multiplier, tick_size);
    }
    // Cut where a minute starts, so no minute is in two pieces.
    let mut cuts = vec![0];
    for piece in 1..threads {
        let at = records.len() * piece / threads;
        let minute = records[at].unix_seconds().div_euclid(60);
        let from = *cuts.last().unwrap_or(&0);
        let cut = from + records[from..].partition_point(|record| record.unix_seconds().div_euclid(60) < minute);
        if cut > from && cut < records.len() {
            cuts.push(cut);
        }
    }
    cuts.push(records.len());
    let pieces: Vec<&[sierra::Record]> = cuts.windows(2).map(|w| &records[w[0]..w[1]]).collect();
    std::thread::scope(|scope| {
        let folding: Vec<_> =
            pieces.iter().map(|piece| scope.spawn(move || minutes_of(piece, multiplier, tick_size))).collect();
        folding.into_iter().flat_map(|folded| folded.join().unwrap_or_default()).collect()
    })
}

struct Watching {
    stop: Arc<AtomicBool>,
    wake: Arc<Mutex<Option<sierra::StopHandle>>>,
}

impl Subscription for Watching {}

impl Drop for Watching {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(wake) = self.wake.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
            wake.stop();
        }
    }
}

/// Follow `symbol`'s file, folding each flush into the forming minute:
/// the minute forming after each, and a minute as it seals. The file
/// started over is a drop, and following it again the feed resumed.
fn watch(symbol: String, tick_size: f64, say: &dyn Fn(LadderUpdate), stop: &AtomicBool, wake: &Mutex<Option<sierra::StopHandle>>) {
    let mut started = false;
    let mut forming = Forming::new(tick_size);
    while !stop.load(Ordering::Acquire) {
        let Some(opened) = opened() else { return };
        let Some(stem) = stem_for(&opened, &symbol) else { return };
        let Some(path) = opened.scid_path(&stem) else { return };
        let multiplier = multiplier(&opened, &stem);
        let Ok(scid) = mapped(&path) else { return };
        let position = scid.lock().unwrap_or_else(|e| e.into_inner()).len();
        let Ok(mut tail) = sierra::Tail::open(&path, position) else { return };
        *wake.lock().unwrap_or_else(|e| e.into_inner()) = Some(tail.stop_handle());
        if std::mem::replace(&mut started, true) {
            say(LadderUpdate::Resumed);
        }
        loop {
            if stop.load(Ordering::Acquire) {
                return;
            }
            match tail.wait(Duration::from_secs(5)) {
                Ok(sierra::Appended::Records(records)) => {
                    for record in records {
                        let Some((tick, _, sides)) = prints(record, multiplier, tick_size) else { continue };
                        let nanos = record.unix_micros() * 1_000;
                        for (size, side) in sides.into_iter().filter(|(size, _)| *size > 0) {
                            if let Some(sealed) = forming.push(nanos, tick, size, side) {
                                say(LadderUpdate::Sealed { minute: sealed, covered: forming.covered() });
                            }
                        }
                    }
                    if let Some(current) = forming.current() {
                        say(LadderUpdate::Forming(current.clone()));
                    }
                }
                Ok(sierra::Appended::Idle) => {
                    let now = chrono::Utc::now().timestamp();
                    if let Some(sealed) = forming.seal_by(now) {
                        say(LadderUpdate::Sealed { minute: sealed, covered: forming.covered() });
                    }
                }
                Ok(sierra::Appended::Stopped) => return,
                Ok(sierra::Appended::Truncated | sierra::Appended::Replaced | sierra::Appended::Removed) | Err(_) => {
                    say(LadderUpdate::Dropped(forming.disconnect()));
                    break;
                }
            }
        }
    }
}

impl Ladders for Sierra {
    /// A contract Sierra Chart has trades of, or a root whose front month
    /// it has.
    fn serves(&self, symbol: &str) -> bool {
        opened().is_some_and(|opened| stem_for(&opened, symbol).is_some_and(|stem| opened.records(&stem) > 0))
    }

    fn history_days(&self) -> u32 {
        HISTORY_DAYS
    }

    fn minutes(&self, symbol: &str, tick_size: f64, from: i64, to: i64) -> Result<Vec<Minute>, ProviderError> {
        let opened = opened().ok_or_else(not_set_up)?;
        let stem = stem_for(&opened, symbol).ok_or(ProviderError::NotFound)?;
        let path = opened.scid_path(&stem).ok_or(ProviderError::NotFound)?;
        let scid = mapped(&path)?;
        let scid = scid.lock().unwrap_or_else(|e| e.into_inner());
        Ok(minutes_in(scid.between(from, to), multiplier(&opened, &stem), tick_size))
    }

    fn watch(&self, symbol: &str, tick_size: f64, sink: LadderSink) -> Result<Box<dyn Subscription>, ProviderError> {
        let stop = Arc::new(AtomicBool::new(false));
        let wake = Arc::new(Mutex::new(None));
        let (stopping, waking) = (stop.clone(), wake.clone());
        let symbol = symbol.to_string();
        std::thread::Builder::new()
            .name("omacharts sierra ladders".into())
            .spawn(move || {
                let say = |update: LadderUpdate| {
                    if !stopping.load(Ordering::Acquire) {
                        sink(update);
                    }
                };
                watch(symbol, tick_size, &say, &stopping, &waking);
            })
            .map_err(|error| read_failed(std::io::Error::other(error)))?;
        Ok(Box::new(Watching { stop, wake }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn trade(seconds: i64, price: f32, size: u32, at_ask: bool) -> sierra::Record {
        let micros = seconds * 1_000_000;
        sierra::Record {
            datetime: sierra::sc_from_unix_micros(micros),
            open: 0.0,
            high: price + 25.0,
            low: price,
            close: price,
            trades: 1,
            volume: size,
            bid_volume: if at_ask { 0 } else { size },
            ask_volume: if at_ask { size } else { 0 },
        }
    }

    /// A minute's ladder is the trades in it, each at its price and on the
    /// side that took it, priced at the file's scale.
    #[test]
    fn a_minute_is_the_trades_in_it_on_the_side_that_took_them() {
        let records = [trade(600, 764700.0, 2, true), trade(630, 764725.0, 3, false), trade(661, 764700.0, 1, true)];
        let minutes = minutes_of(&records, 0.01, 0.25);
        assert_eq!(minutes.len(), 2);
        let first = &minutes[0];
        assert_eq!((first.ts, first.open, first.high, first.close, first.volume), (600, 7647.0, 7647.25, 7647.25, 5));
        assert_eq!(first.ladder.at(to_tick(7647.0, 0.25)).at_ask, 2);
        assert_eq!(first.ladder.at(to_tick(7647.25, 0.25)).at_bid, 3);
        assert_eq!((first.ladder.sides.ask_trades, first.ladder.sides.bid_trades), (1, 1));
        assert_eq!(minutes[1].ts, 660);
    }

    /// Split across threads, the minutes are the same minutes.
    #[test]
    fn minutes_folded_in_pieces_are_the_minutes_folded_whole() {
        let records: Vec<sierra::Record> = (0..(PARALLEL_ABOVE as i64 + 5_000))
            .map(|i| trade(1_700_000_000 + i / 7, 764700.0 + (i % 13) as f32 * 25.0, 1 + (i % 3) as u32, i % 2 == 0))
            .collect();
        assert_eq!(minutes_in(&records, 0.01, 0.25), minutes_of(&records, 0.01, 0.25));
    }
}
