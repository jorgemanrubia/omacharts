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
//! that has closed and been read whole.

use rusqlite::{params, OptionalExtension};

use omacharts_engine::studies::orders::Print;

use super::Store;

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
    /// Day `day` of `key`'s trades, if it is kept in `tick_size`'s ticks.
    pub fn load_print_day(&self, key: &str, day: i64, tick_size: f64) -> Option<PrintDay> {
        let row = self
            .conn
            .prepare_cached("SELECT tick_size, upto, complete, data FROM print_days WHERE key = ?1 AND day = ?2")
            .and_then(|mut s| {
                s.query_row(params![key, day], |r| {
                    Ok((r.get::<_, f64>(0)?, r.get::<_, i64>(1)?, r.get::<_, bool>(2)?, r.get::<_, Vec<u8>>(3)?))
                })
                .optional()
            })
            .ok()??;
        let (kept_tick, upto, complete, data) = row;
        (kept_tick == tick_size).then(|| PrintDay { tick_size, upto, complete, prints: decode_prints(&data) })
    }

    /// Keep day `day` of `key`'s trades, in place of what was kept.
    pub fn save_print_day(&self, key: &str, day: i64, held: &PrintDay) -> rusqlite::Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO print_days (key, day, tick_size, upto, complete, data)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![key, day, held.tick_size, held.upto, held.complete, encode_prints(&held.prints)],
        )?;
        Ok(())
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
        let day = PrintDay { tick_size: 0.25, upto: 99, complete: false, prints: vec![Print::new(5, 1, 2, Aggressor::Buy, false)] };
        store.save_print_day("ES", 3, &day).expect("kept");
        assert_eq!(store.load_print_day("ES", 3, 0.25), Some(day.clone()));
        assert_eq!(store.load_print_day("ES", 3, 0.5), None, "another tick is another ladder");
        assert_eq!(store.load_print_day("ES", 4, 0.25), None);
        assert_eq!(store.forget_print_days("ES", 4).expect("forgotten"), 1);
        assert_eq!(store.load_print_day("ES", 3, 0.25), None);
    }
}
