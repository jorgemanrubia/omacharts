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
use crate::studies::orders::{Aggressor, Print};
use crate::provider::{LadderSink, LadderUpdate, Ladders, ProviderError, Subscription};

use super::{mapped, multiplier, not_set_up, opened, read_failed, stem_for, Sierra};

/// How far back a request may go, in days: as far as a file does, which
/// for a quarterly contract is about four months.
const HISTORY_DAYS: u32 = 400;

/// Records spanned before a fold is split across threads.
const PARALLEL_ABOVE: usize = 1 << 20;

/// One record's prints, as ladder trades: what traded at the bid as
/// sellers', at the ask as buyers', and the rest, if a record has any, on
/// no side. `None` for a record whose price is not a number; a negative or
/// zero price is a price, as the bars of the file take it.
type Prints = (i32, f64, [(u64, Side); 3]);

fn prints(record: &sierra::Record, multiplier: f64, tick_size: f64) -> Option<Prints> {
    let price = f64::from(record.close) * multiplier;
    if !price.is_finite() {
        return None;
    }
    let tick = to_tick(price, tick_size);
    let (bid, ask, all) = (u64::from(record.bid_volume), u64::from(record.ask_volume), u64::from(record.volume));
    let unknown = all.saturating_sub(bid + ask);
    Some((tick, price, [(bid, Side::Sell), (ask, Side::Buy), (unknown, Side::Unknown)]))
}

/// The single trades of `records`, oldest first. A record carries what
/// traded at the bid and at the ask; a record with both is two prints of
/// one moment, one a side. The pieces of a trade Sierra Chart split are
/// marked, the first as such and the rest as continuing it, up to the one
/// marked last, so combining folds them back into the order they were.
pub(super) fn trades_in(records: &[sierra::Record], multiplier: f64, tick_size: f64) -> Vec<Print> {
    let mut out = Vec::with_capacity(records.len());
    let mut unbundled = false;
    for record in records {
        let Some((tick, _, sides)) = prints(record, multiplier, tick_size) else { continue };
        let first = record.is_first_sub_trade();
        let continues = unbundled && !first;
        unbundled = first || (unbundled && !record.is_last_sub_trade());
        let ts = record.unix_micros().saturating_mul(1_000);
        for (size, side) in sides {
            if size == 0 {
                continue;
            }
            let side = match side {
                Side::Buy => Aggressor::Buy,
                Side::Sell => Aggressor::Sell,
                Side::Unknown => Aggressor::Unknown,
            };
            out.push(Print::new(ts, tick, size, side, continues));
        }
    }
    out
}

/// The minutes of `records` after records up to `latest`, oldest first, in
/// whole ticks of `tick_size`, and the latest time once they are in. Each
/// record is in the minute of the time the bars of the file take it at: a
/// record written a little before the one ahead of it is in the minute
/// already open.
fn minutes_of(records: &[sierra::Record], multiplier: f64, tick_size: f64, mut latest: i64) -> (Vec<Minute>, i64) {
    let mut minutes: Vec<Minute> = Vec::new();
    for record in records {
        let Some((tick, price, sides)) = prints(record, multiplier, tick_size) else { continue };
        latest = sierra::fold::in_order(record.datetime, latest);
        let ts = sierra::unix_micros_from_sc(latest).div_euclid(60_000_000) * 60;
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
    (minutes, latest)
}

/// [`minutes_of`], split across threads at minute boundaries for a span
/// long enough to be worth it.
fn minutes_in(records: &[sierra::Record], multiplier: f64, tick_size: f64) -> Vec<Minute> {
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get()).min(16);
    if records.len() < PARALLEL_ABOVE || threads < 2 {
        return minutes_of(records, multiplier, tick_size, i64::MIN).0;
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
    let folded: Vec<(Vec<Minute>, i64)> = std::thread::scope(|scope| {
        let folding: Vec<_> =
            pieces.iter().map(|piece| scope.spawn(move || minutes_of(piece, multiplier, tick_size, i64::MIN))).collect();
        folding.into_iter().map(|folded| folded.join().unwrap_or_default()).collect()
    });
    // Joined as one fold would have made them: a piece that opens on a
    // record written before the latest of the pieces ahead is folded again
    // from that latest, and its first minute, if it is the minute already
    // open, goes on in it.
    let mut minutes: Vec<Minute> = Vec::with_capacity(folded.iter().map(|(minutes, _)| minutes.len()).sum());
    let mut latest = i64::MIN;
    for (piece, (mut piece_minutes, mut last)) in pieces.iter().zip(folded) {
        let Some(first) = piece.iter().find(|record| f64::from(record.close).is_finite()) else { continue };
        if first.datetime < latest {
            (piece_minutes, last) = minutes_of(piece, multiplier, tick_size, latest);
        }
        latest = last;
        let mut rest = piece_minutes.into_iter();
        if let (Some(open), Some(next)) = (minutes.last_mut(), rest.as_slice().first())
            && next.ts == open.ts
        {
            open.merge(next);
            rest.next();
        }
        minutes.extend(rest);
    }
    minutes
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

    /// The file a root is read from: its front month, which rolls.
    fn contract(&self, symbol: &str) -> Option<String> {
        opened().and_then(|opened| stem_for(&opened, symbol))
    }

    fn minutes(&self, symbol: &str, tick_size: f64, from: i64, to: i64) -> Result<Vec<Minute>, ProviderError> {
        let opened = opened().ok_or_else(not_set_up)?;
        let stem = stem_for(&opened, symbol).ok_or(ProviderError::NotFound)?;
        let path = opened.scid_path(&stem).ok_or(ProviderError::NotFound)?;
        let scid = mapped(&path)?;
        let scid = scid.lock().unwrap_or_else(|e| e.into_inner());
        Ok(minutes_in(scid.between(from, to), multiplier(&opened, &stem), tick_size))
    }

    /// Every record of a tick file is one trade.
    fn has_prints(&self, symbol: &str) -> bool {
        self.serves(symbol)
    }

    fn prints(&self, symbol: &str, tick_size: f64, from: i64, to: i64) -> Result<Vec<Print>, ProviderError> {
        let opened = opened().ok_or_else(not_set_up)?;
        let stem = stem_for(&opened, symbol).ok_or(ProviderError::NotFound)?;
        let path = opened.scid_path(&stem).ok_or(ProviderError::NotFound)?;
        let scid = mapped(&path)?;
        let scid = scid.lock().unwrap_or_else(|e| e.into_inner());
        Ok(trades_in(scid.between(from, to), multiplier(&opened, &stem), tick_size))
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

    /// Each record is a print a side, at its own moment, and the pieces of
    /// a trade Sierra Chart split continue the first of them, up to the
    /// last; a trade after them starts afresh.
    #[test]
    fn single_trades_come_from_the_records_and_their_sub_trade_marks() {
        let mut split = [trade(1, 100.0, 5, true), trade(1, 100.25, 7, true), trade(1, 100.5, 3, true), trade(2, 100.5, 4, false)];
        split[0].open = sierra::FIRST_SUB_TRADE_OF_UNBUNDLED_TRADE;
        split[2].open = sierra::LAST_SUB_TRADE_OF_UNBUNDLED_TRADE;
        let prints = trades_in(&split, 1.0, 0.25);
        let seen: Vec<(i32, u64, Aggressor, bool)> = prints.iter().map(|p| (p.tick, p.size(), p.side(), p.continues())).collect();
        assert_eq!(
            seen,
            vec![(400, 5, Aggressor::Buy, false), (401, 7, Aggressor::Buy, true), (402, 3, Aggressor::Buy, true), (402, 4, Aggressor::Sell, false)]
        );
        let mut combiner = crate::studies::orders::Combiner::new(true);
        let mut orders: Vec<u64> = prints.iter().filter_map(|p| combiner.push(*p)).map(|o| o.size).collect();
        orders.extend(combiner.flush().map(|o| o.size));
        assert_eq!(orders, vec![15, 4], "the split trade is one order again");
    }

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
        let minutes = minutes_of(&records, 0.01, 0.25, i64::MIN).0;
        assert_eq!(minutes.len(), 2);
        let first = &minutes[0];
        assert_eq!((first.ts, first.open, first.high, first.close, first.volume), (600, 7647.0, 7647.25, 7647.25, 5));
        assert_eq!(first.ladder.at(to_tick(7647.0, 0.25)).at_ask, 2);
        assert_eq!(first.ladder.at(to_tick(7647.25, 0.25)).at_bid, 3);
        assert_eq!((first.ladder.sides.ask_trades, first.ladder.sides.bid_trades), (1, 1));
        assert_eq!(minutes[1].ts, 660);
    }

    /// A file of a month of trades, as Sierra Chart writes one: a header and
    /// fixed-size records, oldest first.
    fn month_of_trades(path: &std::path::Path, start: i64) -> Vec<sierra::Record> {
        let records: Vec<sierra::Record> = (0..30 * 24 * 120)
            .map(|i| trade(start + i * 30, 764_700.0 + ((i * 7) % 41) as f32 * 25.0, 1 + (i % 4) as u32, i % 3 == 0))
            .collect();
        let mut bytes = vec![0u8; sierra::scid::HEADER_SIZE];
        bytes[0..4].copy_from_slice(b"SCID");
        bytes[4..8].copy_from_slice(&(sierra::scid::HEADER_SIZE as u32).to_le_bytes());
        bytes[8..12].copy_from_slice(&(sierra::scid::RECORD_SIZE as u32).to_le_bytes());
        bytes[12..14].copy_from_slice(&1u16.to_le_bytes());
        for r in &records {
            bytes.extend_from_slice(&r.datetime.to_le_bytes());
            for price in [r.open, r.high, r.low, r.close] {
                bytes.extend_from_slice(&price.to_le_bytes());
            }
            for count in [r.trades, r.volume, r.bid_volume, r.ask_volume] {
                bytes.extend_from_slice(&count.to_le_bytes());
            }
        }
        std::fs::write(path, bytes).unwrap();
        records
    }

    /// A day weeks back in a file is found by its time, not read through to,
    /// and its minutes are exactly the minutes of its trades.
    #[test]
    fn a_day_deep_in_a_file_is_read_by_its_time_and_whole() {
        let dir = std::env::temp_dir().join(format!("omacharts-sierra-ladders-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("ESZ26_FUT_CME.scid");
        let start = 1_790_000_000 - 1_790_000_000 % 86_400;
        let records = month_of_trades(&path, start);
        let scid = sierra::Scid::open(&path).unwrap();
        assert_eq!(scid.len(), records.len());

        let (from, to) = (start + 4 * 86_400 + 14 * 3_600, start + 4 * 86_400 + 20 * 3_600);
        let read = scid.between(from, to);
        assert_eq!(read.len(), 6 * 120, "the six hours' trades and no others");
        let expected: Vec<sierra::Record> =
            records.iter().filter(|r| (from..to).contains(&r.unix_seconds())).copied().collect();
        assert_eq!(minutes_in(read, 0.01, 0.25), minutes_of(&expected, 0.01, 0.25, i64::MIN).0);
        assert_eq!(minutes_in(read, 0.01, 0.25).len(), 6 * 60);
        let _ = std::fs::remove_dir_all(dir);
    }

    /// Split across threads, the minutes are the same minutes: records
    /// written out of order, and one stamped far ahead, included.
    #[test]
    fn minutes_folded_in_pieces_are_the_minutes_folded_whole() {
        let mut records: Vec<sierra::Record> = (0..(PARALLEL_ABOVE as i64 + 5_000))
            .map(|i| trade(1_700_000_000 + i / 7, 764700.0 + (i % 13) as f32 * 25.0, 1 + (i % 3) as u32, i % 2 == 0))
            .collect();
        for (i, record) in records.iter_mut().enumerate() {
            match i % 997 {
                0 => record.datetime -= 30_000_000,
                1 if i % 3 == 0 => record.datetime += 86_400_000_000,
                _ => {}
            }
        }
        assert_eq!(minutes_in(&records, 0.01, 0.25), minutes_of(&records, 0.01, 0.25, i64::MIN).0);
    }

    /// A record written before the one ahead of it is in the minute already
    /// open, as the bars of the file take it, and a price below zero is a
    /// price.
    #[test]
    fn a_minute_takes_its_trades_as_the_bars_do() {
        let records = [
            trade(600, 100.0, 1, true),
            trade(665, 200.0, 1, true),
            trade(655, -350.0, 4, false),
            trade(720, 0.0, 2, true),
        ];
        let minutes = minutes_of(&records, 0.01, 0.25, i64::MIN).0;
        assert_eq!(minutes.iter().map(|m| (m.ts, m.volume)).collect::<Vec<_>>(), [(600, 1), (660, 5), (720, 2)]);
        assert_eq!(minutes[1].low, -3.5);
        assert_eq!(minutes[1].ladder.at(to_tick(-3.5, 0.25)).at_bid, 4);
        let bars = sierra::fold::bars(&records, 60, 0.01);
        assert_eq!(bars.iter().map(|b| (b.start, b.volume)).collect::<Vec<_>>(), [(600, 1), (660, 5), (720, 2)]);
    }
}

