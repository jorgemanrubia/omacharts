//! Single trades, kept a trading day a row, for large orders.
//!
//! A day is one blob of sixteen bytes a trade, oldest first: the time in
//! unix nanoseconds (`i64`), the price in whole ticks (`i32`) and the size,
//! side and continuation flag packed as [`Print::packed`] has them (`u32`),
//! all little-endian. A busy day of ES is some three million trades, about
//! 48 MB; nothing is ever decoded into anything bigger than a [`Print`].
//!
//! `upto` is the moment the row is known to, exclusive: a day still
//! trading is asked of the provider again only from there, and the trades a
//! live feed delivered past it are never counted twice. `complete` is a day
//! that has closed and been read whole. `source` is where the provider read
//! the day from — of the two files Sierra Chart may keep of a contract, the
//! one it was writing — and a day kept from anywhere else is not this one's:
//! it is read again, so a file given up part way never stands for the day.

use rusqlite::{params, OptionalExtension};

use omacharts_engine::studies::orders::Print;

use super::Store;

/// Forget day `day` of `key`'s trades, on any connection: the cache sweep's
/// own, in its transaction.
pub(crate) fn forget_day(conn: &rusqlite::Connection, key: &str, day: i64) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM print_days WHERE key = ?1 AND day = ?2", params![key, day])?;
    Ok(())
}

/// Bytes a trade.
const PRINT_BYTES: usize = 16;

/// One kept day.
#[derive(Clone, Debug, PartialEq)]
pub struct PrintDay {
    pub tick_size: f64,
    /// Known to here, unix nanoseconds, exclusive.
    pub upto: i64,
    pub complete: bool,
    pub prints: Vec<Print>,
    /// Where the provider read it from; empty for a feed with one place.
    pub source: String,
}

pub fn encode_prints(prints: &[Print]) -> Vec<u8> {
    let mut out = Vec::with_capacity(prints.len() * PRINT_BYTES);
    for print in prints {
        out.extend_from_slice(&print.ts_nanos.to_le_bytes());
        out.extend_from_slice(&print.tick.to_le_bytes());
        out.extend_from_slice(&print.packed().to_le_bytes());
    }
    out
}

/// The trades of a blob; a ragged end, from a write cut short, is left off.
pub fn decode_prints(data: &[u8]) -> Vec<Print> {
    data.as_chunks::<PRINT_BYTES>()
        .0
        .iter()
        .map(|chunk| {
            let (ts, rest) = chunk.split_at(8);
            let (tick, packed) = rest.split_at(4);
            let ts = i64::from_le_bytes(ts.try_into().unwrap_or_default());
            let tick = i32::from_le_bytes(tick.try_into().unwrap_or_default());
            let packed = u32::from_le_bytes(packed.try_into().unwrap_or_default());
            Print::from_parts(ts, tick, packed)
        })
        .collect()
}

impl Store {
    /// Day `day` of `key`'s trades, if it is kept in `tick_size`'s ticks,
    /// read from `source`.
    pub fn load_print_day(&self, key: &str, day: i64, tick_size: f64, source: &str) -> Option<PrintDay> {
        let row = self
            .conn
            .prepare_cached("SELECT tick_size, upto, complete, data, source FROM print_days WHERE key = ?1 AND day = ?2")
            .and_then(|mut s| {
                s.query_row(params![key, day], |r| {
                    Ok((r.get::<_, f64>(0)?, r.get::<_, i64>(1)?, r.get::<_, bool>(2)?, r.get::<_, Vec<u8>>(3)?, r.get::<_, String>(4)?))
                })
                .optional()
            })
            .ok()??;
        let (kept_tick, upto, complete, data, kept_source) = row;
        (kept_tick == tick_size && kept_source == source)
            .then(|| PrintDay { tick_size, upto, complete, prints: decode_prints(&data), source: kept_source })
    }

    /// Is day `day` of `key`'s trades kept whole, in `tick_size`'s ticks,
    /// as read from `source`? Asked without reading the day.
    pub fn print_day_complete(&self, key: &str, day: i64, tick_size: f64, source: &str) -> bool {
        self.conn
            .prepare_cached("SELECT 1 FROM print_days WHERE key = ?1 AND day = ?2 AND tick_size = ?3 AND source = ?4 AND complete")
            .and_then(|mut s| s.exists(params![key, day, tick_size, source]))
            .unwrap_or(false)
    }

    /// Keep day `day` of `key`'s trades, in place of what was kept, stamped
    /// now: what the cache sweep orders days by.
    pub fn save_print_day(&self, key: &str, day: i64, held: &PrintDay) -> rusqlite::Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO print_days (key, day, tick_size, upto, complete, data, source, fetched_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                key,
                day,
                held.tick_size,
                held.upto,
                held.complete,
                encode_prints(&held.prints),
                held.source,
                chrono::Utc::now().timestamp()
            ],
        )?;
        Ok(())
    }

    /// Is any day of `key`'s trades from before day `day` kept whole with
    /// trades in it, in `tick_size`'s ticks, as read from `source`? What
    /// tells a gap — days the cache let go, or a feed's holidays — from the
    /// start of what there is.
    pub fn print_days_kept_before(&self, key: &str, day: i64, tick_size: f64, source: &str) -> bool {
        self.conn
            .prepare_cached(
                "SELECT 1 FROM print_days WHERE key = ?1 AND day < ?2 AND tick_size = ?3 AND source = ?4
                 AND complete AND LENGTH(data) > 0",
            )
            .and_then(|mut s| s.exists(params![key, day, tick_size, source]))
            .unwrap_or(false)
    }

    /// Forget day `day` of `key`'s trades, as the cache sweep does.
    pub fn forget_print_day(&self, key: &str, day: i64) -> rusqlite::Result<()> {
        forget_day(&self.conn, key, day)
    }

    /// Forget `key`'s trades from before day `before`: a month that rolled
    /// off, or days past what is wanted.
    pub fn forget_print_days(&self, key: &str, before: i64) -> rusqlite::Result<usize> {
        self.conn.execute("DELETE FROM print_days WHERE key = ?1 AND day < ?2", params![key, before])
    }
}

#[cfg(test)]
mod tests {
    use omacharts_engine::studies::orders::Aggressor;

    use super::*;

    #[test]
    fn a_day_of_trades_is_sixteen_bytes_a_trade_and_reads_back() {
        let prints = vec![
            Print::new(1_700_000_000_123_456_789, -4, 7, Aggressor::Buy, false),
            Print::new(1_700_000_000_123_456_789, 5, 400, Aggressor::Sell, true),
            Print::new(1_700_000_001_000_000_000, 6, 1, Aggressor::Unknown, false),
        ];
        let data = encode_prints(&prints);
        assert_eq!(data.len(), 48);
        assert_eq!(decode_prints(&data), prints);
        assert_eq!(decode_prints(&data[..40]), prints[..2], "a ragged end is left off");
    }

    #[test]
    fn a_kept_day_reads_back_only_in_its_own_ticks() {
        let store = Store::memory().expect("a store");
        let day = PrintDay {
            tick_size: 0.25,
            upto: 99,
            complete: false,
            prints: vec![Print::new(5, 1, 2, Aggressor::Buy, false)],
            source: String::new(),
        };
        store.save_print_day("ES", 3, &day).expect("kept");
        assert_eq!(store.load_print_day("ES", 3, 0.25, ""), Some(day.clone()));
        assert!(!store.print_day_complete("ES", 3, 0.25, ""), "still trading");
        store.save_print_day("ES", 3, &PrintDay { complete: true, ..day.clone() }).expect("kept");
        assert!(store.print_day_complete("ES", 3, 0.25, ""));
        assert!(!store.print_day_complete("ES", 3, 0.5, ""));
        assert_eq!(store.load_print_day("ES", 3, 0.5, ""), None, "another tick is another ladder");
        assert_eq!(store.load_print_day("ES", 4, 0.25, ""), None);
        assert_eq!(store.forget_print_days("ES", 4).expect("forgotten"), 1);
        assert_eq!(store.load_print_day("ES", 3, 0.25, ""), None);
    }

    /// A day read from a file Sierra Chart has since given up for another
    /// of the same contract is not the day as the other file has it.
    #[test]
    fn a_day_kept_from_another_file_is_not_this_one_s() {
        let store = Store::memory().expect("a store");
        let old = PrintDay {
            tick_size: 0.25,
            upto: 99,
            complete: true,
            prints: vec![Print::new(5, 1, 2, Aggressor::Buy, false)],
            source: "ESZ6.CME".into(),
        };
        store.save_print_day("ES", 3, &old).expect("kept");
        assert!(store.print_day_complete("ES", 3, 0.25, "ESZ6.CME"));
        assert!(!store.print_day_complete("ES", 3, 0.25, "ESZ26_FUT_CME"), "the old file's day stood for the new one's");
        assert_eq!(store.load_print_day("ES", 3, 0.25, "ESZ26_FUT_CME"), None);
    }
}
