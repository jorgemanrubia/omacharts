//! Keeping the bar cache from growing for ever.
//!
//! Bars are kept so that a chart opened a second time is drawn from disk
//! instead of fetched, and until now nothing ever threw any of them away: a
//! year of opening charts was a database that only grew. This drops the
//! oldest of it back out when the whole thing passes a limit the user sets.
//!
//! Three things keep it from being felt. The size is read from SQLite's own
//! page counters rather than measured, which is a header read and nothing
//! else. The work happens on its own connection on its own thread, which is
//! what WAL is for. And it runs at most once every six hours, because a cache
//! sitting well under its limit is a question not worth asking twice in a day.

use std::path::PathBuf;

use rusqlite::Connection;

use crate::store::Store;

/// What the cache may occupy before the oldest of it is dropped.
pub const DEFAULT_LIMIT: i64 = 1024 * 1024 * 1024;

/// No limit: what was fetched is kept for good, and a sweep drops nothing.
pub const UNLIMITED: i64 = i64::MAX;

/// The sizes offered in settings, largest last.
///
/// Unlimited last of all, for anybody with the disk to keep everything ever
/// fetched: a limit is only ever met by dropping data that may have to be
/// fetched again one day.
pub const LIMITS: [(i64, &str); 6] = [
    (256 * 1024 * 1024, "256 MB"),
    (512 * 1024 * 1024, "512 MB"),
    (1024 * 1024 * 1024, "1 GB"),
    (2 * 1024 * 1024 * 1024, "2 GB"),
    (5 * 1024 * 1024 * 1024, "5 GB"),
    (UNLIMITED, "Unlimited"),
];

/// How much of the limit a sweep leaves behind.
///
/// Pruning to exactly the limit would leave the cache one chart away from
/// being over it again, with the next sweep six hours out. A fifth of the
/// limit is hundreds of series of headroom at any size worth setting — far
/// more than a day of opening charts — and still not so much that a cache
/// which had only just crossed the line loses most of itself.
const KEEP: f64 = 0.8;

/// The most often a sweep runs.
pub const CADENCE: i64 = 6 * 60 * 60;

pub const SETTING_LIMIT: &str = "cache_limit";
pub const SETTING_SWEPT_AT: &str = "cache_swept_at";

/// What a sweep did, for the settings row that has to show it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Swept {
    pub dropped: usize,
    pub before: i64,
    pub after: i64,
}

/// What the market data file occupies, not counting pages it has already
/// freed. Settings are not the cache, and do not count against its limit.
///
/// Deliberately not the size of the file. Deleting rows hands their pages to
/// SQLite's freelist to be used again and leaves the file exactly as large as
/// it was, so a sweep measured against the file would free half the cache,
/// read the same number back, and sweep again every six hours for ever.
pub fn used_bytes(conn: &Connection) -> i64 {
    let read = |name: &str| -> i64 {
        conn.pragma_query_value(Some(crate::migrations::MARKET), name, |row| row.get(0))
            .unwrap_or(0)
    };
    let pages = read("page_count") - read("freelist_count");
    pages.max(0) * read("page_size")
}

/// What a limit is called in the menu that sets it.
///
/// The row showing how much of it is used says the same words, so that "1 GB"
/// there and "1 GB" in the menu cannot drift into being two renderings of one
/// number.
pub fn limit_label(bytes: i64) -> String {
    LIMITS
        .iter()
        .find(|(size, _)| *size == bytes)
        .map(|(_, label)| label.to_string())
        .unwrap_or_else(|| format!("{} MB", bytes / (1024 * 1024)))
}

/// How many of the oldest series and days to drop to get `before` down to
/// `target`.
///
/// `sizes` is every droppable one's payload, oldest first. The payload is a
/// little under what it costs in pages, so this errs towards dropping one
/// more rather than one fewer, which is the right way to be wrong when the
/// alternative is still being over the limit afterwards. What the open
/// charts draw is never among them (see [`InUse`]), so a cache that cannot
/// get under its limit without it stops short rather than dropping it.
fn drop_count(sizes: &[i64], before: i64, target: i64) -> usize {
    let mut freed = 0;
    for (dropped, size) in sizes.iter().enumerate() {
        if freed >= before - target {
            return dropped;
        }
        freed += size;
    }
    sizes.len()
}

/// What the charts open now draw, which a sweep never drops.
///
/// Dropping it is what made a sweep pointless: the chart showing it fetched
/// it straight back, every series and every day of it, and the next sweep
/// found the cache as full as before and dropped it again. Anything else
/// is dropped oldest first, and fetched again only if a chart comes to need
/// it. A cache whose open charts alone are over the limit is left over it.
#[derive(Clone, Default, Debug, PartialEq)]
pub struct InUse {
    /// Each bar series drawn, `(key, interval)`.
    pub series: Vec<(String, String)>,
    /// Each contract whose ladders or single trades are drawn, `(stored key,
    /// from)`: every day of it ending after `from`, unix seconds.
    pub days: Vec<(String, i64)>,
}

impl InUse {
    fn holds(&self, what: &Evictable, reach: i64) -> bool {
        match what {
            // The charts name a feed's key, the store its kind's.
            Evictable::Series { key, interval } => {
                self.series.iter().any(|(k, i)| crate::store::market_key(k) == key.as_str() && i == interval)
            }
            Evictable::LadderDay { key, .. } | Evictable::PrintDay { key, .. } => {
                self.days.iter().any(|(k, from)| crate::store::market_key(k) == key.as_str() && reach >= *from)
            }
        }
    }
}

/// One thing the sweep can drop: a whole bar series, or one trading day of
/// a contract's minute ladders or of its single trades.
#[derive(Clone, PartialEq, Debug)]
enum Evictable {
    Series { key: String, interval: String },
    LadderDay { key: String, day: i64 },
    PrintDay { key: String, day: i64 },
}

/// Drop the oldest series, ladder days and trade days until the database is
/// back under its limit, leaving alone whatever `in_use` names.
///
/// Oldest by `fetched_at`, which is when a series or a day was last
/// *written* rather than last read. That is a closer proxy for use than it
/// sounds: the app refetches a series whenever a chart that needs new bars is
/// opened, so anything looked at regularly keeps a recent stamp. What it
/// mistakes is a chart opened often that never needs new bars — and those are
/// the cheapest possible things to fetch again. A true last-read column would
/// cost a write on every chart opened, every day, to sharpen an eviction that
/// happens twice a day at most and only once a cache has grown past a
/// gigabyte.
///
/// A contract's ladder and trade days are usually written together, so they
/// tie on `fetched_at` and go oldest trading day first: a sweep trims the
/// far end of a contract's history rather than punching holes in it. A
/// ladder day dropped takes its coverage with it, so the loader knows to
/// fetch it again; a trade day dropped is simply not kept, and read again
/// only by a chart reaching back to it.
pub fn evict(conn: &Connection, limit: i64, in_use: &InUse) -> rusqlite::Result<Swept> {
    let before = used_bytes(conn);
    if before <= limit {
        return Ok(Swept { dropped: 0, before, after: before });
    }

    let rows = evictable(conn, in_use)?;
    let sizes: Vec<i64> = rows.iter().map(|r| r.1).collect();
    let dropped = drop_count(&sizes, before, (limit as f64 * KEEP) as i64);
    if dropped == 0 {
        return Ok(Swept { dropped: 0, before, after: before });
    }
    let tx = conn.unchecked_transaction()?;
    for (_, _, what) in rows.iter().take(dropped) {
        match what {
            Evictable::Series { key, interval } => {
                tx.execute("DELETE FROM bar_series WHERE key = ?1 AND interval = ?2", [key, interval])?;
            }
            Evictable::LadderDay { key, day } => crate::store::forget_ladder_day(&tx, key, *day)?,
            Evictable::PrintDay { key, day } => crate::store::forget_print_day(&tx, key, *day)?,
        }
    }
    tx.commit()?;

    // Freeing a few hundred megabytes writes a few hundred megabytes of
    // journal, and nothing else is going to come along and fold it back in
    // for a while. Passive never waits on a reader, so the window carries on
    // regardless of whether this gets to finish.
    let _ = conn.pragma_update(None, "wal_checkpoint", "PASSIVE");

    Ok(Swept { dropped, before, after: used_bytes(conn) })
}

/// Every series, ladder day and trade day no open chart draws, as
/// `(fetched_at, payload bytes, what)`, in the order they go.
///
/// The order measured is the order dropped, so it has to be total:
/// `fetched_at` alone ties. Among rows written in the same second, days go
/// first and by trading day, oldest first, ladders before trades, then
/// series by key.
fn evictable(conn: &Connection, in_use: &InUse) -> rusqlite::Result<Vec<(i64, i64, Evictable)>> {
    // Each with how far it reaches, unix seconds, for what the charts draw.
    let mut rows: Vec<(i64, i64, Evictable, i64)> = Vec::new();
    let mut stmt = conn.prepare(
        "SELECT fetched_at, LENGTH(ts) + LENGTH(open) + LENGTH(high) + LENGTH(low)
              + LENGTH(close) + LENGTH(volume), key, interval, last_ts
         FROM bar_series",
    )?;
    let series = stmt.query_map([], |row| {
        Ok((row.get(0)?, row.get(1)?, Evictable::Series { key: row.get(2)?, interval: row.get(3)? }, row.get(4)?))
    })?;
    rows.extend(series.flatten());
    let mut stmt = conn.prepare("SELECT fetched_at, LENGTH(data) + LENGTH(total), key, day, last_ts FROM ladder_days")?;
    let days = stmt.query_map([], |row| {
        Ok((row.get(0)?, row.get(1)?, Evictable::LadderDay { key: row.get(2)?, day: row.get(3)? }, row.get(4)?))
    })?;
    rows.extend(days.flatten());
    // `upto` is nanoseconds, and a day known to nothing reaches nowhere.
    let mut stmt = conn.prepare("SELECT fetched_at, LENGTH(data), key, day, upto FROM print_days")?;
    let days = stmt.query_map([], |row| {
        let upto: i64 = row.get(4)?;
        Ok((row.get(0)?, row.get(1)?, Evictable::PrintDay { key: row.get(2)?, day: row.get(3)? }, upto.div_euclid(1_000_000_000)))
    })?;
    rows.extend(days.flatten());
    let mut rows: Vec<(i64, i64, Evictable)> = rows
        .into_iter()
        .filter(|(_, _, what, reach)| !in_use.holds(what, *reach))
        .map(|(at, size, what, _)| (at, size, what))
        .collect();
    rows.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| order(&a.2).cmp(&order(&b.2))));
    Ok(rows)
}

fn order(what: &Evictable) -> (u8, i64, u8, &str, &str) {
    match what {
        Evictable::LadderDay { key, day } => (0, *day, 0, key, ""),
        Evictable::PrintDay { key, day } => (0, *day, 1, key, ""),
        Evictable::Series { key, interval } => (1, 0, 0, key, interval),
    }
}

/// Sweep on a thread of its own, and say what happened once it is done.
///
/// Its own connection to the same file rather than the window's: WAL lets a
/// writer work alongside readers, and the alternative is holding the one
/// connection the UI draws from while several hundred megabytes go.
pub fn sweep_in_background(path: PathBuf, limit: i64, in_use: InUse, done: impl Fn(Swept) + 'static) {
    let (sender, receiver) = async_channel::bounded::<Swept>(1);

    std::thread::spawn(move || {
        // The window has already migrated these files; this connection only
        // has to agree with it about how they are written.
        let Ok(conn) = crate::store::connect(&path) else { return };

        let Ok(swept) = evict(&conn, limit, &in_use) else { return };
        let store = Store::adopt(conn);
        store.set_setting(SETTING_SWEPT_AT, &chrono::Utc::now().timestamp().to_string());
        let _ = sender.send_blocking(swept);
    });

    glib::spawn_future_local(async move {
        if let Ok(swept) = receiver.recv().await {
            done(swept);
        }
    });
}

use gtk::glib;

#[cfg(test)]
mod tests {
    use super::*;

    /// Already under the target is the usual answer, and the arithmetic has
    /// to reach it without dropping anything.
    #[test]
    fn a_cache_already_under_the_target_keeps_everything() {
        assert_eq!(drop_count(&[10, 10, 10], 80, 80), 0);
        assert_eq!(drop_count(&[10, 10, 10], 50, 80), 0);
    }

    /// Only as much as the overshoot needs: being over by a little must not
    /// cost the whole cache.
    #[test]
    fn only_enough_to_cover_the_overshoot_is_dropped() {
        assert_eq!(drop_count(&[10, 10, 10, 10], 100, 80), 2);
    }

    #[test]
    fn enough_of_the_oldest_goes_to_get_back_under_the_target() {
        // 300 over a target of 100 means 200 to free, and the three oldest
        // series are what covers it.
        let sizes = [80, 80, 80, 80, 80];
        assert_eq!(drop_count(&sizes, 300, 100), 3);
    }

    /// Short of the target with everything droppable gone: all of it, and
    /// no more — what the charts draw was never offered.
    #[test]
    fn a_cache_that_cannot_reach_its_target_drops_what_it_may_and_stops() {
        assert_eq!(drop_count(&[10, 10], 5_000, 800), 2);
        assert_eq!(drop_count(&[], 5_000, 800), 0);
    }
}
