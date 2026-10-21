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

/// The sizes offered in settings, largest last.
///
/// No unlimited: the whole point of the setting is that the cache stops
/// growing, and an option that turns that off is the behaviour being fixed.
/// Anybody who wants more room picks more room.
pub const LIMITS: [(i64, &str); 5] = [
    (256 * 1024 * 1024, "256 MB"),
    (512 * 1024 * 1024, "512 MB"),
    (1024 * 1024 * 1024, "1 GB"),
    (2 * 1024 * 1024 * 1024, "2 GB"),
    (5 * 1024 * 1024 * 1024, "5 GB"),
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

/// How many of the oldest series to drop to get `before` down to `target`.
///
/// `sizes` is every series' payload, oldest first. The payload is a little
/// under what the series costs in pages, so this errs towards dropping one
/// more rather than one fewer, which is the right way to be wrong when the
/// alternative is still being over the limit afterwards.
fn drop_count(sizes: &[i64], before: i64, target: i64) -> usize {
    // Always leave one. A single series larger than the whole limit would
    // otherwise be dropped on every sweep and fetched straight back by the
    // chart still showing it.
    let droppable = sizes.len().saturating_sub(1);
    let mut freed = 0;
    for (dropped, size) in sizes.iter().take(droppable).enumerate() {
        if freed >= before - target {
            return dropped;
        }
        freed += size;
    }
    droppable
}

/// Drop the oldest series until the database is back under its limit.
///
/// Oldest by `fetched_at`, which is when a series was last *written* rather
/// than last read. That is a closer proxy for use than it sounds: the app
/// refetches a series whenever a chart that needs new bars is opened, so
/// anything looked at regularly keeps a recent stamp. What it mistakes is a
/// chart opened often that never needs new bars — and those are the cheapest
/// possible things to fetch again. A true last-read column would cost a write
/// on every chart opened, every day, to sharpen an eviction that happens
/// twice a day at most and only once a cache has grown past a gigabyte.
pub fn evict(conn: &Connection, limit: i64) -> rusqlite::Result<Swept> {
    let before = used_bytes(conn);
    if before <= limit {
        return Ok(Swept { dropped: 0, before, after: before });
    }

    // The order here and the order in the delete below have to be the same
    // one, or the rows that were measured are not the rows that go. Neither
    // is `fetched_at` alone, which ties.
    const ORDER: &str = "ORDER BY fetched_at, key, interval";
    let mut stmt = conn.prepare(&format!(
        "SELECT LENGTH(ts) + LENGTH(open) + LENGTH(high) + LENGTH(low)
              + LENGTH(close) + LENGTH(volume)
         FROM bar_series {ORDER}"
    ))?;
    let sizes: Vec<i64> = stmt.query_map([], |row| row.get(0))?.flatten().collect();
    drop(stmt);

    let dropped = drop_count(&sizes, before, (limit as f64 * KEEP) as i64);
    if dropped == 0 {
        return Ok(Swept { dropped: 0, before, after: before });
    }
    conn.execute(
        &format!(
            "DELETE FROM bar_series WHERE (key, interval) IN
                 (SELECT key, interval FROM bar_series {ORDER} LIMIT ?1)"
        ),
        [dropped as i64],
    )?;

    // Freeing a few hundred megabytes writes a few hundred megabytes of
    // journal, and nothing else is going to come along and fold it back in
    // for a while. Passive never waits on a reader, so the window carries on
    // regardless of whether this gets to finish.
    let _ = conn.pragma_update(None, "wal_checkpoint", "PASSIVE");

    Ok(Swept { dropped, before, after: used_bytes(conn) })
}

/// Sweep on a thread of its own, and say what happened once it is done.
///
/// Its own connection to the same file rather than the window's: WAL lets a
/// writer work alongside readers, and the alternative is holding the one
/// connection the UI draws from while several hundred megabytes go.
pub fn sweep_in_background(path: PathBuf, limit: i64, done: impl Fn(Swept) + 'static) {
    let (sender, receiver) = async_channel::bounded::<Swept>(1);

    std::thread::spawn(move || {
        // The window has already migrated these files; this connection only
        // has to agree with it about how they are written.
        let Ok(conn) = crate::store::connect(&path) else { return };

        let Ok(swept) = evict(&conn, limit) else { return };
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

    /// Dropping the lot would take the chart on screen with it, and the next
    /// fetch would put it straight back — twice a day, for ever.
    #[test]
    fn one_series_always_survives_however_big_it_is() {
        assert_eq!(drop_count(&[5_000], 5_000, 800), 0);
        assert_eq!(drop_count(&[5_000, 5_000], 10_000, 800), 1);
    }
}
