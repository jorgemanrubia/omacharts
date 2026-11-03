//! Minute ladders in the market data file.
//!
//! Three tables, all in `market`:
//!
//! - **`ladder_days`**, one row per contract per trading day: the day's
//!   minutes as one blob ([`omacharts_engine::ladders::encode_day`]) and
//!   their sum as another, so a range reads day totals and never sums a day
//!   again. A day is also the unit the cache evicts.
//! - **`ladder_tail`**, one row per minute sealed live, encoded on its own.
//!   Writing a minute as it seals costs one small insert — a few hundred
//!   bytes — where folding it into its day row would re-encode and rewrite
//!   some 100 KB every minute of the session, most of it unchanged. A day's
//!   tail is folded into its row by [`Store::compact_ladders`] once the day
//!   has closed (the loader does it as it opens a contract), and whenever
//!   provider history for that day arrives. Reading applies the tail on top
//!   of the row, so nothing waits on compaction to be seen.
//! - **`ladder_coverage`**, the spans of minute opens, `from_ts..to_ts`,
//!   that are *known*: every print in them was seen, live or from the
//!   provider, so a minute missing inside one traded nothing. A disconnect
//!   leaves a hole here, and [`Store::missing_ladders`] is the hole: exactly
//!   the span to ask the provider for, and nothing already held.
//!
//! The provider is authoritative. Its minutes for a span replace whatever is
//! stored there, live-built or not, minute for minute — including a live
//! minute it does not have, which goes — and each day's total is kept equal
//! to the sum of its minutes as they change.

use omacharts_engine::ladders::{
    decode_day_with_total, decode_ladder, decode_minute, encode_day, encode_ladder, encode_minute, Day, Days, Held,
    HeldDay, Minute, Series,
};
use rusqlite::{params, Connection, OptionalExtension};

use super::Store;

impl Store {
    /// Write whole days, replacing what is stored for them, and fold away
    /// their tails. Coverage is the caller's to record with
    /// [`Self::cover_ladders`]; [`Self::save_ladder_history`] does both.
    pub fn save_ladder_days(&self, key: &str, tick_size: f64, days: &[Day]) -> rusqlite::Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        for day in days {
            write_day(&tx, key, tick_size, day)?;
            tx.execute("DELETE FROM ladder_tail WHERE key = ?1 AND day = ?2", params![key, day.day])?;
        }
        tx.commit()
    }

    /// The trading days `from_day..=to_day` of `key`, each with its stored
    /// total and any minutes sealed live since applied on top.
    ///
    /// A row that cannot be read is left out, and so is its coverage the
    /// next time anything asks what is missing — see [`Self::missing_ladders`].
    pub fn load_ladder_series(&self, key: &str, from_day: i64, to_day: i64) -> Series {
        let mut series = Series::new(0.0);
        let mut statement = match self.conn.prepare_cached(
            "SELECT tick_size, data, total FROM ladder_days
             WHERE key = ?1 AND day BETWEEN ?2 AND ?3 ORDER BY day",
        ) {
            Ok(statement) => statement,
            Err(_) => return series,
        };
        let rows = statement.query_map(params![key, from_day, to_day], |r| {
            Ok((r.get::<_, f64>(0)?, r.get::<_, Vec<u8>>(1)?, r.get::<_, Vec<u8>>(2)?))
        });
        if let Ok(rows) = rows {
            for (tick_size, data, total) in rows.flatten() {
                series.tick_size = tick_size;
                if let Ok(day) = decode_day_with_total(&data, &total, tick_size) {
                    series.days.push(day);
                }
            }
        }
        for (day, tick_size, minute) in tail(&self.conn, key, from_day, to_day) {
            series.tick_size = tick_size;
            series.insert_minute(day, minute);
        }
        series
    }

    /// Store a minute sealed live, and the span the feed now knows complete.
    ///
    /// One small row, not a rewrite of its day. A minute inside a span the
    /// store already knows, that is not itself complete, is not written: a
    /// minute the feed joined part way through must not overwrite the
    /// provider's whole one.
    pub fn append_ladder_minute(
        &self,
        key: &str,
        tick_size: f64,
        day: i64,
        minute: &Minute,
        covered: Option<(i64, i64)>,
    ) -> rusqlite::Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        let complete = covered.is_some_and(|(from, to)| (from..to).contains(&minute.ts));
        if complete || !is_covered(&tx, key, minute.ts)? {
            tx.execute(
                "INSERT OR REPLACE INTO ladder_tail (key, ts, day, tick_size, data) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![key, minute.ts, day, tick_size, encode_minute(minute, tick_size)],
            )?;
        }
        if let Some((from, to)) = covered {
            add_span(&tx, key, from, to)?;
        }
        tx.commit()
    }

    /// The provider's minutes for `from..to`, which replace everything
    /// stored there: a minute built live is overwritten by the provider's,
    /// and one the provider does not have is dropped, since the provider saw
    /// every print and the feed may not have. The span is known afterwards.
    pub fn save_ladder_history(
        &self,
        key: &str,
        tick_size: f64,
        days: &Days,
        from: i64,
        to: i64,
        minutes: &[Minute],
    ) -> rusqlite::Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        let mut touched: Vec<i64> = {
            let mut statement = tx.prepare_cached(
                "SELECT day FROM ladder_days WHERE key = ?1 AND last_ts >= ?2 AND first_ts < ?3
                 UNION SELECT day FROM ladder_tail WHERE key = ?1 AND ts >= ?2 AND ts < ?3",
            )?;
            statement.query_map(params![key, from, to], |r| r.get(0))?.collect::<Result<_, _>>()?
        };
        let fresh: Vec<(i64, &Minute)> = minutes
            .iter()
            .filter(|m| (from..to).contains(&m.ts))
            .map(|m| (days.of(m.ts), m))
            .collect();
        touched.extend(fresh.iter().map(|(day, _)| *day));
        touched.sort_unstable();
        touched.dedup();
        for number in touched {
            let mut day = read_day(&tx, key, number)?.unwrap_or_else(|| Day::new(number));
            day.remove(from, to);
            for (_, minute) in fresh.iter().filter(|(d, _)| *d == number) {
                day.insert((*minute).clone());
            }
            write_day(&tx, key, tick_size, &day)?;
            tx.execute("DELETE FROM ladder_tail WHERE key = ?1 AND day = ?2", params![key, number])?;
        }
        add_span(&tx, key, from, to)?;
        tx.commit()
    }

    /// Fold the live tails of days before `before_day` into their rows.
    /// Returns how many days it folded.
    pub fn compact_ladders(&self, key: &str, before_day: i64) -> rusqlite::Result<usize> {
        let tx = self.conn.unchecked_transaction()?;
        let numbers: Vec<i64> = {
            let mut statement =
                tx.prepare_cached("SELECT DISTINCT day FROM ladder_tail WHERE key = ?1 AND day < ?2")?;
            statement.query_map(params![key, before_day], |r| r.get(0))?.collect::<Result<_, _>>()?
        };
        for &number in &numbers {
            let tick_size = tail(&tx, key, number, number).first().map(|(_, t, _)| *t).unwrap_or(0.0);
            if let Some(day) = read_day(&tx, key, number)? {
                write_day(&tx, key, tick_size, &day)?;
            }
            tx.execute("DELETE FROM ladder_tail WHERE key = ?1 AND day = ?2", params![key, number])?;
        }
        tx.commit()?;
        Ok(numbers.len())
    }

    /// Record that every print in `from..to` is held.
    pub fn cover_ladders(&self, key: &str, from: i64, to: i64) -> rusqlite::Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        add_span(&tx, key, from, to)?;
        tx.commit()
    }

    /// The spans of `key` known complete, oldest first.
    pub fn ladder_coverage(&self, key: &str) -> Vec<(i64, i64)> {
        spans(&self.conn, key, i64::MIN, i64::MAX).unwrap_or_default()
    }

    /// The parts of `from..to` nothing is known about: what to ask the
    /// provider for. A disconnect is one of these; a quiet stretch the feed
    /// watched is not.
    pub fn missing_ladders(&self, key: &str, from: i64, to: i64) -> Vec<(i64, i64)> {
        let mut missing = Vec::new();
        let mut at = from;
        for (a, b) in spans(&self.conn, key, from, to).unwrap_or_default() {
            if a > at {
                missing.push((at, a.min(to)));
            }
            at = at.max(b);
            if at >= to {
                break;
            }
        }
        if at < to {
            missing.push((at, to));
        }
        missing
    }

    /// What a chart holds of `key` over the trading days
    /// `from_day..=to_day`: each day's stored total and nothing more, so a
    /// month is a few hundred kilobytes rather than a month of decoded
    /// minutes. A day with minutes sealed live and not yet folded into its
    /// row comes whole, its tail applied. The floor is the first minute
    /// stored for `key` at all: the provider's history goes no further.
    pub fn load_ladder_held(&self, key: &str, days: &Days, from_day: i64, to_day: i64) -> Held {
        let mut held = Held::new(0.0);
        let tailed: Vec<i64> = self
            .conn
            .prepare_cached("SELECT DISTINCT day FROM ladder_tail WHERE key = ?1 AND day BETWEEN ?2 AND ?3")
            .and_then(|mut s| s.query_map(params![key, from_day, to_day], |r| r.get(0))?.collect())
            .unwrap_or_default();
        if let Ok(mut statement) = self.conn.prepare_cached(
            "SELECT day, tick_size, first_ts, last_ts, total FROM ladder_days
             WHERE key = ?1 AND day BETWEEN ?2 AND ?3 ORDER BY day",
        ) {
            let rows = statement.query_map(params![key, from_day, to_day], |r| {
                Ok((r.get::<_, i64>(0)?, r.get::<_, f64>(1)?, r.get::<_, i64>(2)?, r.get::<_, i64>(3)?, r.get::<_, Vec<u8>>(4)?))
            });
            if let Ok(rows) = rows {
                for (day, tick_size, first, last, total) in rows.flatten() {
                    held.tick_size = tick_size;
                    if tailed.contains(&day) {
                        continue;
                    }
                    if let Ok(total) = decode_ladder(&total) {
                        held.days.push(HeldDay::total(day, first, last, total));
                    }
                }
            }
        }
        for number in tailed {
            if let Ok(Some(day)) = read_day(&self.conn, key, number) {
                if let Some((_, tick_size, _)) = tail(&self.conn, key, number, number).first() {
                    held.tick_size = *tick_size;
                }
                held.load(day);
            }
        }
        let floor: Option<i64> = self
            .conn
            .query_row(
                "SELECT MIN(ts) FROM (SELECT MIN(first_ts) AS ts FROM ladder_days WHERE key = ?1
                                      UNION ALL SELECT MIN(ts) FROM ladder_tail WHERE key = ?1)",
                params![key],
                |r| r.get(0),
            )
            .ok()
            .flatten();
        held.floor = floor.unwrap_or(i64::MAX);
        held.floor_day = match floor {
            Some(floor) => {
                let day = days.of(floor);
                if floor <= days.start(day) { day } else { day + 1 }
            }
            None => i64::MAX,
        };
        held
    }

    /// One trading day of `key`, minutes and all, its tail applied.
    pub fn load_ladder_day(&self, key: &str, day: i64) -> Option<Day> {
        read_day(&self.conn, key, day).ok().flatten()
    }

    /// Every day stored for `key`, as `(day, minutes)`, oldest first.
    pub fn ladder_days(&self, key: &str) -> Vec<(i64, i64)> {
        let Ok(mut statement) =
            self.conn.prepare_cached("SELECT day, minutes FROM ladder_days WHERE key = ?1 ORDER BY day")
        else {
            return Vec::new();
        };
        statement
            .query_map(params![key], |r| Ok((r.get(0)?, r.get(1)?)))
            .map(|rows| rows.flatten().collect())
            .unwrap_or_default()
    }
}

/// Take one stored day out, its tail and what it covered with it: what the
/// cache sweep does to the oldest.
///
/// The coverage it held goes too, so the loader knows to ask for it again.
/// From its first minute to its last: the quiet stretches either side of a
/// session stay known, which they are.
pub(crate) fn forget_day(conn: &Connection, key: &str, day: i64) -> rusqlite::Result<()> {
    let bounds: Option<(i64, i64)> = conn
        .query_row(
            "SELECT first_ts, last_ts FROM ladder_days WHERE key = ?1 AND day = ?2",
            params![key, day],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    conn.execute("DELETE FROM ladder_days WHERE key = ?1 AND day = ?2", params![key, day])?;
    conn.execute("DELETE FROM ladder_tail WHERE key = ?1 AND day = ?2", params![key, day])?;
    if let Some((first, last)) = bounds {
        sub_span(conn, key, first, last + 60)?;
    }
    Ok(())
}

/// A day as it stands: its row, with its tail on top.
fn read_day(conn: &Connection, key: &str, number: i64) -> rusqlite::Result<Option<Day>> {
    let row: Option<(f64, Vec<u8>, Vec<u8>)> = conn
        .query_row(
            "SELECT tick_size, data, total FROM ladder_days WHERE key = ?1 AND day = ?2",
            params![key, number],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?;
    let mut day = row.and_then(|(tick_size, data, total)| decode_day_with_total(&data, &total, tick_size).ok());
    for (_, _, minute) in tail(conn, key, number, number) {
        day.get_or_insert_with(|| Day::new(number)).insert(minute);
    }
    Ok(day)
}

fn write_day(conn: &Connection, key: &str, tick_size: f64, day: &Day) -> rusqlite::Result<()> {
    let (Some(first), Some(last)) = (day.first_ts(), day.last_ts()) else {
        conn.execute("DELETE FROM ladder_days WHERE key = ?1 AND day = ?2", params![key, day.day])?;
        return Ok(());
    };
    conn.execute(
        "INSERT OR REPLACE INTO ladder_days
             (key, day, tick_size, minutes, first_ts, last_ts, fetched_at, data, total)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            key,
            day.day,
            tick_size,
            day.minutes.len() as i64,
            first,
            last,
            chrono::Utc::now().timestamp(),
            encode_day(day, tick_size),
            encode_ladder(&day.total),
        ],
    )?;
    Ok(())
}

/// The live minutes of days `from_day..=to_day`, oldest first.
fn tail(conn: &Connection, key: &str, from_day: i64, to_day: i64) -> Vec<(i64, f64, Minute)> {
    let Ok(mut statement) = conn.prepare_cached(
        "SELECT day, tick_size, data FROM ladder_tail WHERE key = ?1 AND day BETWEEN ?2 AND ?3 ORDER BY ts",
    ) else {
        return Vec::new();
    };
    let rows = statement.query_map(params![key, from_day, to_day], |r| {
        Ok((r.get::<_, i64>(0)?, r.get::<_, f64>(1)?, r.get::<_, Vec<u8>>(2)?))
    });
    let Ok(rows) = rows else { return Vec::new() };
    rows.flatten()
        .filter_map(|(day, tick_size, data)| decode_minute(&data, tick_size).ok().map(|m| (day, tick_size, m)))
        .collect()
}

/// The coverage spans of `key` that touch `from..=to`, oldest first.
fn spans(conn: &Connection, key: &str, from: i64, to: i64) -> rusqlite::Result<Vec<(i64, i64)>> {
    let mut statement = conn.prepare_cached(
        "SELECT from_ts, to_ts FROM ladder_coverage
         WHERE key = ?1 AND from_ts <= ?3 AND to_ts >= ?2 ORDER BY from_ts",
    )?;
    statement.query_map(params![key, from, to], |r| Ok((r.get(0)?, r.get(1)?)))?.collect()
}

fn is_covered(conn: &Connection, key: &str, ts: i64) -> rusqlite::Result<bool> {
    Ok(spans(conn, key, ts, ts)?.iter().any(|(a, b)| (*a..*b).contains(&ts)))
}

/// Join `from..to` to the coverage, merging what it touches.
fn add_span(conn: &Connection, key: &str, from: i64, to: i64) -> rusqlite::Result<()> {
    if from >= to {
        return Ok(());
    }
    let touching = spans(conn, key, from, to)?;
    let a = touching.iter().map(|s| s.0).fold(from, i64::min);
    let b = touching.iter().map(|s| s.1).fold(to, i64::max);
    for (start, _) in &touching {
        conn.execute("DELETE FROM ladder_coverage WHERE key = ?1 AND from_ts = ?2", params![key, start])?;
    }
    conn.execute("INSERT INTO ladder_coverage (key, from_ts, to_ts) VALUES (?1, ?2, ?3)", params![key, a, b])?;
    Ok(())
}

/// Take `from..to` out of the coverage.
fn sub_span(conn: &Connection, key: &str, from: i64, to: i64) -> rusqlite::Result<()> {
    for (a, b) in spans(conn, key, from, to)? {
        if b <= from || a >= to {
            continue;
        }
        conn.execute("DELETE FROM ladder_coverage WHERE key = ?1 AND from_ts = ?2", params![key, a])?;
        for (x, y) in [(a, from), (to, b)] {
            if x < y {
                conn.execute(
                    "INSERT INTO ladder_coverage (key, from_ts, to_ts) VALUES (?1, ?2, ?3)",
                    params![key, x, y],
                )?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use omacharts_engine::ladders::{Forming, Ladder, Side};

    use super::*;

    const TICK: f64 = 0.25;
    const KEY: &str = "ES";
    const NANOS: i64 = 1_000_000_000;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("omacharts-ladders-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("omacharts.db")
    }

    fn minute(ts: i64, tick: i32, buy: u64, sell: u64) -> Minute {
        let price = tick as f64 * TICK;
        let mut m = Minute::opening(ts, price);
        m.add_trade(tick, price, buy, Side::Buy, 1);
        m.add_trade(tick - 1, price - TICK, sell, Side::Sell, 1);
        m
    }

    fn trimmed(ladder: &Ladder) -> Ladder {
        let mut ladder = ladder.clone();
        ladder.trim();
        ladder
    }

    /// Every day's total against the sum of its minutes, and the whole
    /// range against the minutes one by one.
    fn assert_totals_exact(series: &Series) {
        for day in &series.days {
            let mut summed = day.clone();
            summed.resum();
            assert_eq!(trimmed(&day.total), trimmed(&summed.total), "day {}", day.day);
            assert_eq!(day.total.sides, summed.total.sides);
        }
        let mut brute = Ladder::new();
        series.minutes().for_each(|m| brute.add(m));
        assert_eq!(trimmed(&series.volume_at_price(i64::MIN, i64::MAX)), trimmed(&brute));
    }

    #[test]
    fn days_saved_read_back_the_same_from_a_reopened_file() {
        let path = scratch("days");
        let days = Days::fixed(0);
        let mut series = Series::new(TICK);
        for i in 0..3 * 1440 {
            series.insert(&days, minute(i * 60, 30_000 + (i % 40) as i32, 1 + i as u64 % 7, 2));
        }
        Store::open_at(&path).unwrap().save_ladder_days(KEY, TICK, &series.days).unwrap();

        let store = Store::open_at(&path).unwrap();
        let back = store.load_ladder_series(KEY, 0, 10);
        assert_eq!(back, series);
        assert_eq!(store.load_ladder_series(KEY, 1, 1).days, series.days[1..2].to_vec());
        assert!(store.load_ladder_series("NQ", 0, 10).days.is_empty());
        assert_eq!(store.ladder_days(KEY), vec![(0, 1440), (1, 1440), (2, 1440)]);
    }

    #[test]
    fn minutes_sealed_live_survive_a_restart() {
        let path = scratch("live");
        let days = Days::fixed(0);
        let mut forming = Forming::new(TICK);
        forming.connect(0);
        let mut sealed = Vec::new();
        {
            let store = Store::open_at(&path).unwrap();
            for s in 0..600 {
                let tick = 30_000 + (s % 9) as i32;
                let side = if s % 3 == 0 { Side::Sell } else { Side::Buy };
                if let Some(m) = forming.push(s * NANOS, tick, 1 + (s % 4) as u64, side) {
                    store.append_ladder_minute(KEY, TICK, days.of(m.ts), &m, forming.covered()).unwrap();
                    sealed.push(m);
                }
            }
        }
        assert_eq!(sealed.len(), 9);

        let store = Store::open_at(&path).unwrap();
        let back = store.load_ladder_series(KEY, 0, 0);
        assert_eq!(back.minutes().cloned().collect::<Vec<_>>(), sealed);
        assert_eq!(store.ladder_coverage(KEY), vec![(0, 540)]);
        assert_eq!(store.missing_ladders(KEY, 0, 600), vec![(540, 600)], "the forming minute is not held yet");
        assert_totals_exact(&back);
    }

    /// A disconnect is a hole the loader fills, and the provider's minutes
    /// for it replace the partial ones the feed built either side.
    #[test]
    fn a_disconnect_is_backfilled_and_the_provider_wins() {
        let store = Store::memory().unwrap();
        let days = Days::fixed(0);
        let mut forming = Forming::new(TICK);
        let save = |m: Option<Minute>, covered: Option<(i64, i64)>| {
            if let Some(m) = m {
                store.append_ladder_minute(KEY, TICK, days.of(m.ts), &m, covered).unwrap();
            }
        };
        forming.connect(0);
        for minute in 0..6 {
            let m = forming.push((minute * 60 + 10) * NANOS, 30_000, 5, Side::Buy);
            save(m, forming.covered());
        }
        // The feed drops at 5:30, and comes back at 8:30.
        save(forming.disconnect(), None);
        for minute in 8..12 {
            let m = forming.push((minute * 60 + 30) * NANOS, 30_010, 1, Side::Sell);
            save(m, forming.covered());
        }
        assert_eq!(store.ladder_coverage(KEY), vec![(0, 300), (540, 660)]);
        assert_eq!(store.missing_ladders(KEY, 0, 660), vec![(300, 540)]);
        assert_eq!(store.missing_ladders(KEY, -60, 720), vec![(-60, 0), (300, 540), (660, 720)]);
        let before = store.load_ladder_series(KEY, 0, 0);
        assert_eq!(before.minutes().map(|m| m.ts).collect::<Vec<_>>(), vec![0, 60, 120, 180, 240, 300, 480, 540, 600]);

        // The provider's history for the gap, from 5:00 to 9:00. It has
        // minute 5 whole, minute 6, nothing at 7, and minute 8 whole.
        let provider = [minute(300, 30_004, 50, 40), minute(360, 30_005, 9, 9), minute(480, 30_009, 30, 31)];
        store.save_ladder_history(KEY, TICK, &days, 300, 540, &provider).unwrap();

        assert_eq!(store.missing_ladders(KEY, 0, 660), vec![]);
        assert_eq!(store.ladder_coverage(KEY), vec![(0, 660)]);
        let after = store.load_ladder_series(KEY, 0, 0);
        let got: Vec<&Minute> = after.minutes().collect();
        assert_eq!(got.iter().map(|m| m.ts).collect::<Vec<_>>(), vec![0, 60, 120, 180, 240, 300, 360, 480, 540, 600]);
        assert_eq!(*got[5], provider[0], "the provider's minute 5 replaced the partial live one");
        assert_eq!(*got[7], provider[2], "and its minute 8");
        assert_eq!(got[8].volume, 1, "a complete live minute outside the span is left alone");
        assert_totals_exact(&after);

        // A partial live minute arriving for a span already known does not
        // overwrite it.
        let late = minute(480, 1, 1, 1);
        store.append_ladder_minute(KEY, TICK, 0, &late, None).unwrap();
        assert_eq!(store.load_ladder_series(KEY, 0, 0).minutes().nth(7), Some(&provider[2]));
    }

    #[test]
    fn a_live_minute_the_provider_does_not_have_is_dropped() {
        let store = Store::memory().unwrap();
        let days = Days::fixed(0);
        for ts in [0, 60, 120] {
            store.append_ladder_minute(KEY, TICK, 0, &minute(ts, 100, 1, 1), Some((0, ts + 60))).unwrap();
        }
        store.save_ladder_history(KEY, TICK, &days, 0, 180, &[minute(0, 100, 2, 2), minute(120, 100, 3, 3)]).unwrap();
        let series = store.load_ladder_series(KEY, 0, 0);
        assert_eq!(series.minutes().map(|m| (m.ts, m.volume)).collect::<Vec<_>>(), vec![(0, 4), (120, 6)]);
        assert_totals_exact(&series);
    }

    #[test]
    fn compaction_folds_closed_days_and_keeps_their_totals() {
        let store = Store::memory().unwrap();
        let days = Days::fixed(0);
        // Day 0 has a row already; more of it, and day 1, arrive live.
        let mut first = Series::new(TICK);
        for i in 0..100 {
            first.insert(&days, minute(i * 60, 200 + (i % 13) as i32, 3, 4));
        }
        store.save_ladder_days(KEY, TICK, &first.days).unwrap();
        for i in (50..1440).chain(1440..1500) {
            let m = minute(i * 60, 180 + (i % 29) as i32, 1 + i as u64 % 5, 2);
            store.append_ladder_minute(KEY, TICK, days.of(m.ts), &m, Some((0, i * 60 + 60))).unwrap();
        }
        let live = store.load_ladder_series(KEY, 0, 1);
        assert_eq!(store.compact_ladders(KEY, 1).unwrap(), 1, "day 1 is still open");
        let tail: i64 =
            store.conn.query_row("SELECT COUNT(*) FROM ladder_tail WHERE day = 0", [], |r| r.get(0)).unwrap();
        assert_eq!(tail, 0);
        let compacted = store.load_ladder_series(KEY, 0, 1);
        assert_eq!(compacted, live);
        assert_eq!(compacted.days[0].minutes.len(), 1440);
        assert_totals_exact(&compacted);
    }

    #[test]
    fn the_cache_sweep_drops_the_oldest_ladder_days_and_forgets_their_coverage() {
        let store = Store::memory().unwrap();
        let days = Days::fixed(0);
        let mut series = Series::new(TICK);
        for i in 0..4 * 1440 {
            series.insert(&days, minute(i * 60, 30_000 + (i % 50) as i32, 9, 9));
        }
        store.save_ladder_days(KEY, TICK, &series.days).unwrap();
        store.cover_ladders(KEY, 0, 4 * 86_400).unwrap();

        let swept = crate::cache::evict(&store.conn, 1).unwrap();
        assert_eq!(swept.dropped, 3, "everything but one");
        assert_eq!(store.ladder_days(KEY), vec![(3, 1440)], "the oldest days went first");
        assert_eq!(store.missing_ladders(KEY, 0, 4 * 86_400), vec![(0, 3 * 86_400)]);
    }
}
