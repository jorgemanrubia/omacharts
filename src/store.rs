//! The local database: settings, cached bars, and themes the user saved.
//!
//! One file, `$XDG_DATA_HOME/omacharts/omacharts.db`, and it is pure app
//! data — never in the repo, safe to delete, rebuilt on demand.
//!
//! Bars are stored columnar: one row per (symbol, timeframe) holding parallel
//! blobs of timestamps and prices. A row per bar would mean tens of thousands
//! of rows per series and a decode per row; this way a chart is one query and
//! one memcpy. Series are small enough — two years of hourly bars is about
//! 12,000 — that rewriting the whole row on a tail fetch costs less than
//! managing chunks would.

use std::path::{Path, PathBuf};

use omacharts_engine::{Bar, BarScheme, Drawing, Indicator, Theme, Timeframe};

use crate::migrations;
use rusqlite::{params, Connection, OptionalExtension};

pub struct Store {
    conn: Connection,
    /// The file behind it, when there is one. The background cache sweep
    /// opens its own connection to the same database, and this is how it
    /// knows which one — an in-memory store has nothing to sweep.
    path: Option<PathBuf>,
}

/// Symbols kept outside any section, in the watchlist that has always been
/// there. Every watchlist has a root of its own; this is the one whose row
/// predates there being more than one.
pub const ROOT_SECTION: i64 = 0;

/// What a root section's position is, in a column where every named section's
/// is zero or more. One number, below everything it is ordered against, is
/// what makes "the section that is not a section" a query rather than a rule
/// written down in two places.
const ROOT_POSITION: i64 = -1;

/// The watchlist every install has and nobody can delete.
///
/// A fixed row id rather than a name, because it can be renamed: resolving it
/// by the string "Default" would lose it the moment somebody called it
/// something else.
pub const DEFAULT_WATCHLIST: i64 = 1;

/// A named group of symbols in a watchlist. The root section has an empty
/// name and is drawn without a header.
#[derive(Clone, PartialEq, Debug)]
pub struct Section {
    pub id: i64,
    pub name: String,
    /// Sections fold away like they do in every other watchlist. The root
    /// never collapses — there is no header to click.
    pub collapsed: bool,
    /// The section holding whatever is not in a section. One per watchlist,
    /// so its id is only [`ROOT_SECTION`] in the default one.
    pub root: bool,
    pub entries: Vec<Entry>,
}

/// A watchlist entry, stored canonically so it survives a provider change.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Entry {
    pub symbol: String,
    pub suffix: Option<String>,
}

/// What we already have for one series, so a fetch can ask only for the gap.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Coverage {
    pub first_ts: i64,
    pub last_ts: i64,
    pub count: usize,
    pub fetched_at: i64,
}

impl Store {
    pub fn open() -> Result<Store, migrations::Error> {
        let dir = data_dir();
        let _ = std::fs::create_dir_all(&dir);
        Store::open_at(&dir.join("omacharts.db"))
    }

    pub fn open_at(path: &Path) -> Result<Store, migrations::Error> {
        let conn = Connection::open(path)?;
        // WAL so a background fetch writing never blocks the window reading.
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        let store = Store { conn, path: Some(path.to_path_buf()) };
        // Where a migration that could lose something leaves a copy first.
        // Beside the database rather than in a temp directory, because the
        // person who needs it has to be able to find it.
        store.migrate(Some(&path.with_extension("db.bak")))?;
        Ok(store)
    }

    /// A database with no file behind it: the tests, and the fallback when
    /// the real one cannot be opened. Nothing to snapshot.
    pub fn memory() -> Result<Store, migrations::Error> {
        let store = Store { conn: Connection::open_in_memory()?, path: None };
        store.migrate(None)?;
        Ok(store)
    }

    /// A store over a connection somebody else opened, with no migration run
    /// and no file recorded.
    ///
    /// For the background cache sweep, which opens its own connection to a
    /// database the window has already brought up to date, and wants the
    /// settings table without reimplementing it.
    pub(crate) fn adopt(conn: Connection) -> Store {
        Store { conn, path: None }
    }

    /// The database's file, for anything that needs a second connection to it.
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// Bring the database up to date, then decide whether what is cached in
    /// it is still worth believing.
    ///
    /// Two different questions, kept apart deliberately: one is the *shape* of
    /// the database, which only ever moves forward and is recorded in
    /// `user_version`; the other is whether the bars already in it were
    /// fetched correctly, which has its own counter because it moves on its
    /// own schedule. A release can change either without touching the other.
    fn migrate(&self, snapshot_to: Option<&Path>) -> Result<(), migrations::Error> {
        migrations::run(&self.conn, snapshot_to)?;
        self.invalidate_stale_cache()?;
        Ok(())
    }

    /// Throw away cached bars written by a version that fetched them wrongly.
    ///
    /// Bumped when the *content* of the cache stops being trustworthy rather
    /// than when its shape changes — version 2 is the daily series fetched
    /// with `range=max`, which Yahoo silently coarsened into month-ends. Those
    /// bars look perfectly valid, so nothing else would ever notice them.
    fn invalidate_stale_cache(&self) -> rusqlite::Result<()> {
        const CACHE_VERSION: i64 = 2;
        let stored: i64 = self
            .conn
            .query_row("SELECT value FROM meta WHERE key = 'cache_version'", [], |r| {
                r.get::<_, String>(0)
            })
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);
        if stored == CACHE_VERSION {
            return Ok(());
        }
        self.conn.execute("DELETE FROM bar_series", [])?;
        self.conn.execute(
            "INSERT INTO meta (key, value) VALUES ('cache_version', ?1)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![CACHE_VERSION.to_string()],
        )?;
        Ok(())
    }

    // -- settings ----------------------------------------------------------

    pub fn setting(&self, key: &str) -> Option<String> {
        self.conn
            .query_row("SELECT value FROM settings WHERE key = ?1", params![key], |r| r.get(0))
            .optional()
            .ok()
            .flatten()
    }

    /// Every setting that has been written, in key order.
    ///
    /// For `omacharts config list`, which is the only way somebody outside
    /// the app can find out what a setting is called before asking for it.
    pub fn settings(&self) -> Vec<(String, String)> {
        let Ok(mut stmt) = self.conn.prepare("SELECT key, value FROM settings ORDER BY key") else {
            return Vec::new();
        };
        let Ok(rows) = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?))) else {
            return Vec::new();
        };
        rows.filter_map(Result::ok).collect()
    }

    pub fn set_setting(&self, key: &str, value: &str) {
        let _ = self.conn.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        );
    }

    /// A stored flag, read the way people write one.
    ///
    /// The app writes "1" and "0", but a setting can also be typed —
    /// `omacharts config set <key> false` — and only the two digits were
    /// understood, so every other spelling silently fell back to the default
    /// and the command looked as though it had done nothing. The words cost
    /// nothing to accept and are what somebody reaches for first.
    pub fn setting_bool(&self, key: &str, default: bool) -> bool {
        match self.setting(key) {
            Some(value) => match value.trim().to_ascii_lowercase().as_str() {
                "1" | "true" | "yes" | "on" => true,
                "0" | "false" | "no" | "off" => false,
                _ => default,
            },
            None => default,
        }
    }

    pub fn set_setting_bool(&self, key: &str, value: bool) {
        self.set_setting(key, if value { "1" } else { "0" });
    }

    // -- bars --------------------------------------------------------------

    pub fn coverage(&self, key: &str, timeframe: Timeframe) -> Option<Coverage> {
        self.conn
            .query_row(
                "SELECT first_ts, last_ts, count, fetched_at FROM bar_series
                 WHERE key = ?1 AND interval = ?2",
                params![key, timeframe.key()],
                |r| {
                    Ok(Coverage {
                        first_ts: r.get(0)?,
                        last_ts: r.get(1)?,
                        count: r.get::<_, i64>(2)? as usize,
                        fetched_at: r.get(3)?,
                    })
                },
            )
            .optional()
            .ok()
            .flatten()
    }

    /// Note that we asked and the provider had nothing new to add.
    ///
    /// `fetched_at` means what its name says — when this series was last
    /// fetched — and a reply with no new bars in it is still a reply. Without
    /// this a chart refreshing itself on a timer would ask again on every
    /// tick for as long as the provider had nothing to give it, which is
    /// precisely the position a public holiday or a thin overnight session
    /// puts it in.
    ///
    /// It also sharpens the proxy for use that [`crate::cache::evict`] orders
    /// by, in the one place that comment admits it is blunt: a series being
    /// polled is a series somebody has on screen.
    pub fn mark_fetched(&self, key: &str, timeframe: Timeframe) {
        let _ = self.conn.execute(
            "UPDATE bar_series SET fetched_at = ?3 WHERE key = ?1 AND interval = ?2",
            params![key, timeframe.key(), chrono::Utc::now().timestamp()],
        );
    }

    pub fn load_bars(&self, key: &str, timeframe: Timeframe) -> Vec<Bar> {
        let row: Option<(Vec<u8>, Vec<u8>, Vec<u8>, Vec<u8>, Vec<u8>, Vec<u8>)> = self
            .conn
            .query_row(
                "SELECT ts, open, high, low, close, volume FROM bar_series
                 WHERE key = ?1 AND interval = ?2",
                params![key, timeframe.key()],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)),
            )
            .optional()
            .ok()
            .flatten();

        let Some((ts, open, high, low, close, volume)) = row else {
            return Vec::new();
        };
        let stamps = decode_i64(&ts);
        let (o, h, l, c, v) = (
            decode_f64(&open),
            decode_f64(&high),
            decode_f64(&low),
            decode_f64(&close),
            decode_f64(&volume),
        );
        let n = stamps.len().min(o.len()).min(h.len()).min(l.len()).min(c.len());
        (0..n)
            .map(|i| Bar {
                ts: stamps[i],
                open: o[i],
                high: h[i],
                low: l[i],
                close: c[i],
                volume: v.get(i).copied().unwrap_or(0.0),
            })
            .collect()
    }

    /// Merge `fresh` into whatever is stored.
    ///
    /// Completed bars are immutable, but the last bar we stored may have been
    /// forming when we saw it, so a bar arriving with a timestamp we already
    /// hold always wins. Returns the merged series.
    pub fn merge_bars(&self, key: &str, timeframe: Timeframe, fresh: &[Bar]) -> Vec<Bar> {
        let mut bars = self.load_bars(key, timeframe);
        if fresh.is_empty() {
            return bars;
        }
        if bars.is_empty() {
            bars = fresh.to_vec();
        } else {
            for bar in fresh {
                match bars.binary_search_by_key(&bar.ts, |b| b.ts) {
                    Ok(at) => bars[at] = *bar,
                    Err(at) => bars.insert(at, *bar),
                }
            }
        }
        self.write_bars(key, timeframe, &bars);
        bars
    }

    /// Replace a series wholesale. Used by [`Self::merge_bars`], and when an
    /// overlap mismatch means the cached history can no longer be trusted.
    pub fn write_bars(&self, key: &str, timeframe: Timeframe, bars: &[Bar]) {
        if bars.is_empty() {
            let _ = self.conn.execute(
                "DELETE FROM bar_series WHERE key = ?1 AND interval = ?2",
                params![key, timeframe.key()],
            );
            return;
        }
        let ts: Vec<i64> = bars.iter().map(|b| b.ts).collect();
        let col = |f: fn(&Bar) -> f64| encode_f64(&bars.iter().map(f).collect::<Vec<_>>());
        let _ = self.conn.execute(
            "INSERT INTO bar_series
                 (key, interval, first_ts, last_ts, count, fetched_at,
                  ts, open, high, low, close, volume)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
             ON CONFLICT(key, interval) DO UPDATE SET
                 first_ts = excluded.first_ts, last_ts = excluded.last_ts,
                 count = excluded.count, fetched_at = excluded.fetched_at,
                 ts = excluded.ts, open = excluded.open, high = excluded.high,
                 low = excluded.low, close = excluded.close,
                 volume = excluded.volume",
            params![
                key,
                timeframe.key(),
                ts.first().copied().unwrap_or(0),
                ts.last().copied().unwrap_or(0),
                bars.len() as i64,
                chrono::Utc::now().timestamp(),
                encode_i64(&ts),
                col(|b| b.open),
                col(|b| b.high),
                col(|b| b.low),
                col(|b| b.close),
                col(|b| b.volume),
            ],
        );
    }

    pub fn drop_series(&self, key: &str, timeframe: Timeframe) {
        let _ = self.conn.execute(
            "DELETE FROM bar_series WHERE key = ?1 AND interval = ?2",
            params![key, timeframe.key()],
        );
    }

    /// Approximate bytes the cached bars occupy.
    pub fn cache_bytes(&self) -> i64 {
        self.conn
            .query_row(
                "SELECT COALESCE(SUM(LENGTH(ts) + LENGTH(open) + LENGTH(high)
                        + LENGTH(low) + LENGTH(close) + LENGTH(volume)), 0)
                 FROM bar_series",
                [],
                |r| r.get(0),
            )
            .unwrap_or(0)
    }

    pub fn cached_series(&self) -> i64 {
        self.conn
            .query_row("SELECT COUNT(*) FROM bar_series", [], |r| r.get(0))
            .unwrap_or(0)
    }

    /// Settings and saved themes survive; only market data goes.
    pub fn clear_market_data(&self) -> rusqlite::Result<()> {
        self.conn.execute("DELETE FROM bar_series", [])?;
        self.conn.execute_batch("VACUUM")?;
        Ok(())
    }

    /// What the database occupies now.
    ///
    /// The same number the sweep evicts against, and the same one settings
    /// shows. Three places quoting three measurements of "the cache" would
    /// mean watching a prune run and seeing the figure sit still.
    pub fn used_bytes(&self) -> i64 {
        crate::cache::used_bytes(&self.conn)
    }

    /// What the cache is allowed to occupy.
    pub fn cache_limit(&self) -> i64 {
        self.setting(crate::cache::SETTING_LIMIT)
            .and_then(|v| v.parse::<i64>().ok())
            .filter(|bytes| *bytes > 0)
            .unwrap_or(crate::cache::DEFAULT_LIMIT)
    }

    pub fn set_cache_limit(&self, bytes: i64) {
        self.set_setting(crate::cache::SETTING_LIMIT, &bytes.to_string());
    }

    /// Whether enough time has passed to be worth looking again.
    ///
    /// A cache that was under its limit six hours ago is almost certainly
    /// still under it, and the one case that cannot wait — the user choosing
    /// a smaller limit — sweeps on the spot rather than through this.
    pub fn cache_sweep_due(&self) -> bool {
        let last = self
            .setting(crate::cache::SETTING_SWEPT_AT)
            .and_then(|v| v.parse::<i64>().ok())
            .unwrap_or(0);
        chrono::Utc::now().timestamp() - last >= crate::cache::CADENCE
    }

    // -- watchlist ---------------------------------------------------------

    /// Every watchlist, in display order, as id and name.
    pub fn watchlists(&self) -> Vec<(i64, String)> {
        let Ok(mut stmt) =
            self.conn.prepare("SELECT id, name FROM watchlists ORDER BY position, id")
        else {
            return Vec::new();
        };
        let Ok(rows) = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?))) else {
            return Vec::new();
        };
        rows.filter_map(Result::ok).collect()
    }

    pub fn watchlist_exists(&self, id: i64) -> bool {
        self.conn
            .query_row("SELECT 1 FROM watchlists WHERE id = ?1", params![id], |_| Ok(()))
            .optional()
            .ok()
            .flatten()
            .is_some()
    }

    /// The default watchlist, in display order.
    ///
    /// The one the bar widget shows, which is why it is the one this returns:
    /// switching the rail to a scratch list is a thing you do while looking at
    /// the app, and it has no business rewriting what sits in the system bar.
    pub fn watchlist(&self) -> Vec<Section> {
        self.watchlist_sections(DEFAULT_WATCHLIST)
    }

    /// One watchlist's sections, in display order.
    ///
    /// Sections are optional: symbols can sit at the root. The root comes back
    /// as a nameless section, present only when it holds something, so callers
    /// render one uniform list either way.
    pub fn watchlist_sections(&self, watchlist: i64) -> Vec<Section> {
        let root_id = self.root_section(watchlist);
        let mut out = Vec::new();
        let root = self.section_entries(root_id);
        if !root.is_empty() {
            out.push(Section {
                id: root_id,
                name: String::new(),
                collapsed: false,
                root: true,
                entries: root,
            });
        }
        out.extend(self.named_sections(watchlist));
        out
    }

    /// Where a symbol lands when it is put in a watchlist rather than in one
    /// of its sections.
    ///
    /// Every watchlist has one, held apart from the named sections by a
    /// position no section it is ordered against can reach. The default
    /// watchlist's is [`ROOT_SECTION`], which is the row that was there before
    /// any of this.
    pub fn root_section(&self, watchlist: i64) -> i64 {
        self.conn
            .query_row(
                "SELECT id FROM watchlist_sections
                 WHERE watchlist_id = ?1 AND position = ?2 ORDER BY id LIMIT 1",
                params![watchlist, ROOT_POSITION],
                |r| r.get(0),
            )
            .unwrap_or(ROOT_SECTION)
    }

    fn named_sections(&self, watchlist: i64) -> Vec<Section> {
        let Ok(mut stmt) = self.conn.prepare(
            "SELECT id, name, collapsed FROM watchlist_sections
             WHERE watchlist_id = ?1 AND position <> ?2 ORDER BY position, id",
        ) else {
            return Vec::new();
        };
        let Ok(rows) = stmt.query_map(params![watchlist, ROOT_POSITION], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)? != 0))
        }) else {
            return Vec::new();
        };
        rows.filter_map(Result::ok)
            .map(|(id, name, collapsed)| Section {
                id,
                name,
                collapsed,
                root: false,
                entries: self.section_entries(id),
            })
            .collect()
    }

    /// Create an empty watchlist, with the root section it needs to hold a
    /// symbol that is not in a section.
    pub fn add_watchlist(&self, name: &str) -> Option<i64> {
        let position: i64 = self
            .conn
            .query_row("SELECT COALESCE(MAX(position), -1) + 1 FROM watchlists", [], |r| r.get(0))
            .unwrap_or(0);
        self.conn
            .execute(
                "INSERT INTO watchlists (name, position) VALUES (?1, ?2)",
                params![name, position],
            )
            .ok()?;
        let id = self.conn.last_insert_rowid();
        self.conn
            .execute(
                "INSERT INTO watchlist_sections (name, position, watchlist_id)
                 VALUES ('', ?1, ?2)",
                params![ROOT_POSITION, id],
            )
            .ok()?;
        Some(id)
    }

    pub fn rename_watchlist(&self, id: i64, name: &str) {
        let _ = self
            .conn
            .execute("UPDATE watchlists SET name = ?2 WHERE id = ?1", params![id, name]);
    }

    /// Remove a watchlist and everything in it. The default one stays: it is
    /// what the bar widget shows and what a deleted watchlist falls back to,
    /// so there has to be one that is always there.
    pub fn remove_watchlist(&self, id: i64) {
        if id == DEFAULT_WATCHLIST {
            return;
        }
        let _ = self.conn.execute(
            "DELETE FROM watchlist_entries WHERE section_id IN
             (SELECT id FROM watchlist_sections WHERE watchlist_id = ?1)",
            params![id],
        );
        let _ = self
            .conn
            .execute("DELETE FROM watchlist_sections WHERE watchlist_id = ?1", params![id]);
        let _ = self.conn.execute("DELETE FROM watchlists WHERE id = ?1", params![id]);
        // The group it drove goes with it. Left behind, the row still claims
        // the group — so the next list offering that group was told it was
        // taken, by a list that no longer exists and could not be named.
        let _ = self
            .conn
            .execute("DELETE FROM settings WHERE key = ?1", params![format!("watchlist_link_{id}")]);
    }

    pub fn set_section_collapsed(&self, id: i64, collapsed: bool) {
        let _ = self.conn.execute(
            "UPDATE watchlist_sections SET collapsed = ?2 WHERE id = ?1",
            params![id, collapsed as i64],
        );
    }

    fn section_entries(&self, section_id: i64) -> Vec<Entry> {
        let Ok(mut stmt) = self.conn.prepare(
            "SELECT symbol, suffix FROM watchlist_entries
             WHERE section_id = ?1 ORDER BY position, symbol",
        ) else {
            return Vec::new();
        };
        let Ok(rows) = stmt.query_map(params![section_id], |r| {
            Ok(Entry {
                symbol: r.get(0)?,
                suffix: {
                    let s: String = r.get(1)?;
                    if s.is_empty() { None } else { Some(s) }
                },
            })
        }) else {
            return Vec::new();
        };
        rows.filter_map(Result::ok).collect()
    }

    /// Put a symbol at the root of the default watchlist, outside any section.
    pub fn add_to_root(&self, symbol: &str, suffix: Option<&str>) {
        self.add_to_section(ROOT_SECTION, symbol, suffix);
    }

    pub fn add_section(&self, watchlist: i64, name: &str) -> Option<i64> {
        let position: i64 = self
            .conn
            .query_row(
                "SELECT COALESCE(MAX(position), -1) + 1 FROM watchlist_sections
                 WHERE watchlist_id = ?1",
                params![watchlist],
                |r| r.get(0),
            )
            .unwrap_or(0);
        self.conn
            .execute(
                "INSERT INTO watchlist_sections (name, position, watchlist_id)
                 VALUES (?1, ?2, ?3)",
                params![name, position, watchlist],
            )
            .ok()?;
        Some(self.conn.last_insert_rowid())
    }

    pub fn rename_section(&self, id: i64, name: &str) {
        let _ = self.conn.execute(
            "UPDATE watchlist_sections SET name = ?2 WHERE id = ?1",
            params![id, name],
        );
    }

    /// The named sections of a watchlist, in display order.
    ///
    /// The ids alone, because reordering has no use for the symbols and
    /// reading them costs a query per section.
    pub fn section_order(&self, watchlist: i64) -> Vec<i64> {
        let Ok(mut stmt) = self.conn.prepare(
            "SELECT id FROM watchlist_sections
             WHERE watchlist_id = ?1 AND position <> ?2 ORDER BY position, id",
        ) else {
            return Vec::new();
        };
        let Ok(rows) = stmt.query_map(params![watchlist, ROOT_POSITION], |r| r.get(0)) else {
            return Vec::new();
        };
        rows.filter_map(Result::ok).collect()
    }

    /// Persist a new order for a watchlist's named sections.
    ///
    /// A section's symbols are positioned within the section, so moving the
    /// section moves its symbols with it and no entry has to be touched. That
    /// is also what stops one section's symbols ever landing among another's.
    ///
    /// The root is not in `ordered` and cannot be put there: it is held at
    /// [`ROOT_POSITION`], below every position a named section can take, which
    /// is what keeps the nameless bucket first however the rest are dragged
    /// about.
    ///
    /// `ordered` is the whole order rather than a change to part of it.
    /// Positions are written straight from it, so a named section left out
    /// keeps an old position that one of these may now collide with.
    pub fn reorder_sections(&self, watchlist: i64, ordered: &[i64]) {
        for (position, id) in ordered.iter().enumerate() {
            let _ = self.conn.execute(
                "UPDATE watchlist_sections SET position = ?3
                 WHERE id = ?1 AND watchlist_id = ?2 AND position <> ?4",
                params![id, watchlist, position as i64, ROOT_POSITION],
            );
        }
    }

    /// Remove a section and everything in it. A root cannot be removed —
    /// there is no header to remove it from, and its watchlist would have
    /// nowhere to put a symbol that is not in a section.
    pub fn remove_section(&self, id: i64) {
        let position: i64 = self
            .conn
            .query_row("SELECT position FROM watchlist_sections WHERE id = ?1", params![id], |r| {
                r.get(0)
            })
            .unwrap_or(ROOT_POSITION);
        if id == ROOT_SECTION || position == ROOT_POSITION {
            return;
        }
        let _ = self
            .conn
            .execute("DELETE FROM watchlist_entries WHERE section_id = ?1", params![id]);
        let _ = self
            .conn
            .execute("DELETE FROM watchlist_sections WHERE id = ?1", params![id]);
    }

    pub fn add_to_section(&self, section_id: i64, symbol: &str, suffix: Option<&str>) {
        let position: i64 = self
            .conn
            .query_row(
                "SELECT COALESCE(MAX(position), -1) + 1 FROM watchlist_entries
                 WHERE section_id = ?1",
                params![section_id],
                |r| r.get(0),
            )
            .unwrap_or(0);
        let _ = self.conn.execute(
            "INSERT OR IGNORE INTO watchlist_entries (section_id, symbol, suffix, position)
             VALUES (?1, ?2, ?3, ?4)",
            params![section_id, symbol, suffix.unwrap_or(""), position],
        );
    }

    pub fn remove_from_section(&self, section_id: i64, symbol: &str, suffix: Option<&str>) {
        let _ = self.conn.execute(
            "DELETE FROM watchlist_entries
             WHERE section_id = ?1 AND symbol = ?2 AND suffix = ?3",
            params![section_id, symbol, suffix.unwrap_or("")],
        );
    }

    /// Persist a new order for one section's entries.
    ///
    /// Positions are rewritten from the list given, so the caller only has to
    /// know the order it wants, not what the old positions were.
    pub fn reorder_entries(&self, section_id: i64, ordered: &[Entry]) {
        for (position, entry) in ordered.iter().enumerate() {
            let _ = self.conn.execute(
                "UPDATE watchlist_entries SET position = ?4
                 WHERE section_id = ?1 AND symbol = ?2 AND suffix = ?3",
                params![
                    section_id,
                    entry.symbol,
                    entry.suffix.clone().unwrap_or_default(),
                    position as i64
                ],
            );
        }
    }

    /// Move one entry so it sits where `before` currently sits.
    pub fn move_entry(&self, section_id: i64, moving: &Entry, before: &Entry) {
        let mut entries = self.section_entries(section_id);
        let Some(from) = entries.iter().position(|e| e == moving) else { return };
        let entry = entries.remove(from);
        let to = entries.iter().position(|e| e == before).unwrap_or(entries.len());
        entries.insert(to, entry);
        self.reorder_entries(section_id, &entries);
    }

    /// Move an entry into another section, landing before `before` when given
    /// and at the end otherwise.
    pub fn move_entry_to_section(
        &self,
        from: i64,
        to: i64,
        moving: &Entry,
        before: Option<&Entry>,
    ) {
        if from == to {
            if let Some(before) = before {
                self.move_entry(from, moving, before);
            }
            return;
        }
        self.remove_from_section(from, &moving.symbol, moving.suffix.as_deref());
        self.add_to_section(to, &moving.symbol, moving.suffix.as_deref());
        if let Some(before) = before {
            self.move_entry(to, moving, before);
        }
    }

    /// Give a new install something to look at rather than an empty rail.
    pub fn seed_watchlist_if_empty(&self, defaults: &[(&str, &[&str])]) {
        let existing: i64 = self
            .conn
            .query_row(
                "SELECT (SELECT COUNT(*) FROM watchlist_sections WHERE id <> ?1)
                      + (SELECT COUNT(*) FROM watchlist_entries)",
                params![ROOT_SECTION],
                |r| r.get(0),
            )
            .unwrap_or(0);
        if existing > 0 {
            return;
        }
        for (name, symbols) in defaults {
            if let Some(id) = self.add_section(DEFAULT_WATCHLIST, name) {
                for symbol in *symbols {
                    self.add_to_section(id, symbol, None);
                }
            }
        }
    }

    // -- indicators --------------------------------------------------------

    /// The indicators on the chart. One set, shared by every symbol — arrowing
    /// down a watchlist to compare setups only works if the chart keeps its
    /// shape.
    pub fn indicators(&self) -> Vec<Indicator> {
        match self.setting("indicators") {
            // A chart nobody has configured carries nothing, the same as a
            // chart in a new chartbook. Volume used to be drawn here
            // unconditionally, and keeping it as the default meant a fresh
            // install opened with an indicator the user never asked for.
            None => Vec::new(),
            Some(json) => serde_json::from_str(&json).unwrap_or_default(),
        }
    }

    pub fn set_indicators(&self, indicators: &[Indicator]) {
        if let Ok(json) = serde_json::to_string(indicators) {
            self.set_setting("indicators", &json);
        }
    }

    // -- drawings ----------------------------------------------------------

    /// What has been drawn on a symbol, oldest first. The suffix is part of
    /// the key, as it is for a watchlist entry: BHP in Sydney and BHP in
    /// London are two charts.
    pub fn drawings(&self, symbol: &str, suffix: Option<&str>) -> Vec<Drawing> {
        let Ok(mut stmt) = self
            .conn
            .prepare("SELECT id, json FROM drawings WHERE symbol = ?1 AND suffix = ?2 ORDER BY id")
        else {
            return Vec::new();
        };
        let Ok(rows) = stmt.query_map(params![symbol, suffix.unwrap_or("")], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
        }) else {
            return Vec::new();
        };
        rows.filter_map(Result::ok)
            .filter_map(|(id, json)| {
                let mut drawing: Drawing = serde_json::from_str(&json).ok()?;
                // The row is the id. What is inside the JSON is whatever the
                // drawing had when it was written, which for a new one is
                // nothing.
                drawing.id = id;
                Some(drawing)
            })
            .collect()
    }

    /// Write a drawing down for the first time. Hands back its id, which the
    /// caller keeps on the drawing so later writes find the same row.
    pub fn add_drawing(&self, symbol: &str, suffix: Option<&str>, drawing: &Drawing) -> Option<i64> {
        let json = serde_json::to_string(drawing).ok()?;
        self.conn
            .execute(
                "INSERT INTO drawings (symbol, suffix, json) VALUES (?1, ?2, ?3)",
                params![symbol, suffix.unwrap_or(""), json],
            )
            .ok()?;
        Some(self.conn.last_insert_rowid())
    }

    /// A drawing that moved, or changed colour. One that was never added is
    /// not written: there is no row for it to go in.
    pub fn update_drawing(&self, drawing: &Drawing) {
        if drawing.id == 0 {
            return;
        }
        if let Ok(json) = serde_json::to_string(drawing) {
            let _ = self
                .conn
                .execute("UPDATE drawings SET json = ?1 WHERE id = ?2", params![json, drawing.id]);
        }
    }

    pub fn remove_drawing(&self, id: i64) {
        let _ = self.conn.execute("DELETE FROM drawings WHERE id = ?1", params![id]);
    }

    /// Everything drawn on a symbol, gone.
    pub fn clear_drawings(&self, symbol: &str, suffix: Option<&str>) {
        let _ = self.conn.execute(
            "DELETE FROM drawings WHERE symbol = ?1 AND suffix = ?2",
            params![symbol, suffix.unwrap_or("")],
        );
    }

    // -- saved themes ------------------------------------------------------

    pub fn custom_themes(&self) -> Vec<Theme> {
        self.json_rows("SELECT json FROM custom_themes ORDER BY id")
    }

    pub fn save_theme(&self, theme: &Theme) {
        if let Ok(json) = serde_json::to_string(theme) {
            let _ = self.conn.execute(
                "INSERT INTO custom_themes (id, json) VALUES (?1, ?2)
                 ON CONFLICT(id) DO UPDATE SET json = excluded.json",
                params![theme.id, json],
            );
        }
    }

    pub fn delete_theme(&self, id: &str) {
        let _ = self.conn.execute("DELETE FROM custom_themes WHERE id = ?1", params![id]);
    }

    pub fn custom_bar_schemes(&self) -> Vec<BarScheme> {
        self.json_rows("SELECT json FROM custom_bar_schemes ORDER BY id")
    }

    pub fn save_bar_scheme(&self, scheme: &BarScheme) {
        if let Ok(json) = serde_json::to_string(scheme) {
            let _ = self.conn.execute(
                "INSERT INTO custom_bar_schemes (id, json) VALUES (?1, ?2)
                 ON CONFLICT(id) DO UPDATE SET json = excluded.json",
                params![scheme.id, json],
            );
        }
    }

    pub fn delete_bar_scheme(&self, id: &str) {
        let _ = self
            .conn
            .execute("DELETE FROM custom_bar_schemes WHERE id = ?1", params![id]);
    }

    /// A row of JSON that fails to parse is skipped rather than fatal: a theme
    /// saved by a newer version must not stop the app opening.
    fn json_rows<T: serde::de::DeserializeOwned>(&self, sql: &str) -> Vec<T> {
        let Ok(mut stmt) = self.conn.prepare(sql) else {
            return Vec::new();
        };
        let Ok(rows) = stmt.query_map([], |r| r.get::<_, String>(0)) else {
            return Vec::new();
        };
        rows.filter_map(Result::ok)
            .filter_map(|json| serde_json::from_str(&json).ok())
            .collect()
    }
}

/// `$XDG_DATA_HOME/omacharts`, falling back to `~/.local/share/omacharts`.
pub fn data_dir() -> PathBuf {
    if let Some(base) = std::env::var_os("XDG_DATA_HOME").filter(|v| !v.is_empty()) {
        return PathBuf::from(base).join("omacharts");
    }
    home().join(".local/share/omacharts")
}

pub fn home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default()
}

fn encode_i64(values: &[i64]) -> Vec<u8> {
    let mut out = Vec::with_capacity(values.len() * 8);
    for v in values {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out
}

fn encode_f64(values: &[f64]) -> Vec<u8> {
    let mut out = Vec::with_capacity(values.len() * 8);
    for v in values {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out
}

fn decode_i64(bytes: &[u8]) -> Vec<i64> {
    bytes
        .chunks_exact(8)
        .map(|c| i64::from_le_bytes(c.try_into().unwrap()))
        .collect()
}

fn decode_f64(bytes: &[u8]) -> Vec<f64> {
    bytes
        .chunks_exact(8)
        .map(|c| f64::from_le_bytes(c.try_into().unwrap()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every symbol in a watchlist, sections and all, for the tests that only
    /// care about which watchlist a symbol ended up in.
    fn symbols(sections: &[Section]) -> Vec<String> {
        sections.iter().flat_map(|s| &s.entries).map(|e| e.symbol.clone()).collect()
    }

    fn bar(ts: i64, close: f64) -> Bar {
        Bar { ts, open: close - 1.0, high: close + 1.0, low: close - 2.0, close, volume: 100.0 }
    }

    #[test]
    fn bars_round_trip() {
        let store = Store::memory().unwrap();
        let bars = vec![bar(100, 10.0), bar(200, 11.0), bar(300, 12.0)];
        store.write_bars("yahoo:ES=F", Timeframe::days(1), &bars);

        let back = store.load_bars("yahoo:ES=F", Timeframe::days(1));
        assert_eq!(back, bars);

        let coverage = store.coverage("yahoo:ES=F", Timeframe::days(1)).unwrap();
        assert_eq!((coverage.first_ts, coverage.last_ts, coverage.count), (100, 300, 3));
    }

    #[test]
    fn a_tail_fetch_overwrites_the_provisional_last_bar() {
        let store = Store::memory().unwrap();
        store.write_bars("k", Timeframe::hours(1), &[bar(100, 10.0), bar(200, 11.0)]);

        // 200 was still forming when we stored it; 300 is new.
        let merged = store.merge_bars("k", Timeframe::hours(1), &[bar(200, 99.0), bar(300, 12.0)]);
        assert_eq!(merged.len(), 3);
        assert_eq!(merged[1].close, 99.0);
        assert_eq!(merged[2].ts, 300);
    }

    #[test]
    fn backfill_extends_without_duplicating() {
        let store = Store::memory().unwrap();
        store.write_bars("k", Timeframe::days(1), &[bar(300, 12.0)]);
        let merged = store.merge_bars("k", Timeframe::days(1), &[bar(100, 10.0), bar(200, 11.0)]);
        assert_eq!(merged.iter().map(|b| b.ts).collect::<Vec<_>>(), vec![100, 200, 300]);
    }

    #[test]
    fn timeframes_do_not_collide() {
        let store = Store::memory().unwrap();
        store.write_bars("k", Timeframe::days(1), &[bar(100, 1.0)]);
        store.write_bars("k", Timeframe::hours(1), &[bar(100, 2.0), bar(200, 3.0)]);
        assert_eq!(store.load_bars("k", Timeframe::days(1)).len(), 1);
        assert_eq!(store.load_bars("k", Timeframe::hours(1)).len(), 2);
    }

    #[test]
    fn clearing_market_data_keeps_settings_and_themes() {
        let store = Store::memory().unwrap();
        store.set_setting("theme", "midnight");
        store.save_theme(&omacharts_engine::theme::builtin_themes()[0].duplicate("mine", "Mine"));
        store.write_bars("k", Timeframe::days(1), &[bar(100, 1.0)]);
        assert!(store.cache_bytes() > 0);

        store.clear_market_data().unwrap();

        assert_eq!(store.cache_bytes(), 0);
        assert_eq!(store.cached_series(), 0);
        assert_eq!(store.setting("theme").as_deref(), Some("midnight"));
        assert_eq!(store.custom_themes().len(), 1);
    }

    #[test]
    fn the_watchlist_round_trips() {
        let store = Store::memory().unwrap();
        let indexes = store.add_section(DEFAULT_WATCHLIST, "Indexes").unwrap();
        let crypto = store.add_section(DEFAULT_WATCHLIST, "Crypto").unwrap();
        store.add_to_section(indexes, "GSPC", None);
        store.add_to_section(indexes, "NDX", None);
        store.add_to_section(crypto, "BTC", None);
        store.add_to_section(indexes, "SAN", Some("MC"));

        let list = store.watchlist();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].name, "Indexes");
        assert_eq!(list[0].entries.len(), 3);
        assert_eq!(list[1].name, "Crypto");
        assert_eq!(list[1].entries[0].symbol, "BTC");
        assert!(list[0].entries.iter().any(|e| e.suffix.as_deref() == Some("MC")));
    }

    #[test]
    fn symbols_can_live_at_the_root_without_a_section() {
        let store = Store::memory().unwrap();
        store.add_to_root("ES", None);
        store.add_to_root("GC", None);

        let list = store.watchlist();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, ROOT_SECTION);
        assert!(list[0].name.is_empty(), "the root has no name");
        assert_eq!(list[0].entries.len(), 2);
    }

    #[test]
    fn the_root_comes_before_named_sections_and_only_when_used() {
        let store = Store::memory().unwrap();
        store.add_section(DEFAULT_WATCHLIST, "Crypto");
        assert_eq!(store.watchlist().len(), 1, "an empty root is not shown");

        store.add_to_root("ES", None);
        let list = store.watchlist();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].id, ROOT_SECTION);
        assert_eq!(list[1].name, "Crypto");
    }

    #[test]
    fn the_root_reorders_like_any_section() {
        let store = Store::memory().unwrap();
        for symbol in ["A", "B", "C"] {
            store.add_to_root(symbol, None);
        }
        let entry = |s: &str| Entry { symbol: s.into(), suffix: None };
        store.move_entry(ROOT_SECTION, &entry("C"), &entry("A"));
        let order: Vec<String> =
            store.watchlist()[0].entries.iter().map(|e| e.symbol.clone()).collect();
        assert_eq!(order, vec!["C", "A", "B"]);
    }

    /// A watchlist somebody has just made and dropped a symbol or two into
    /// has exactly one section, and it is the root — the one `section_order`
    /// leaves out. So the order is empty while the table is not, which is the
    /// state the first named section arrives into, and the one nobody sets up
    /// on purpose. It has to come out as a second section after the loose
    /// symbols, and it has to be the only thing in the order.
    #[test]
    fn the_first_named_section_lands_after_the_loose_symbols_it_was_added_beside() {
        let store = Store::memory().unwrap();
        let scratch = store.add_watchlist("Scratch").expect("made");
        let root = store.root_section(scratch);
        store.add_to_section(root, "ES", None);
        store.add_to_section(root, "GC", None);
        assert!(store.section_order(scratch).is_empty(), "a root is not in the order");

        let energy = store.add_section(scratch, "Energy").expect("made");

        assert_eq!(store.section_order(scratch), vec![energy]);
        let list = store.watchlist_sections(scratch);
        assert_eq!(list.iter().map(|s| s.id).collect::<Vec<_>>(), vec![root, energy]);
        assert!(list[0].root, "the loose symbols stay first");
        assert_eq!(symbols(&list), vec!["ES", "GC"]);

        // And the operations that read that order cope with one named section
        // beside the root: moving a symbol down into it, ordering the one
        // section there is, and taking it away again.
        let entry = Entry { symbol: "GC".into(), suffix: None };
        store.move_entry_to_section(root, energy, &entry, None);
        store.reorder_sections(scratch, &[energy]);
        let list = store.watchlist_sections(scratch);
        assert_eq!(list.iter().map(|s| s.id).collect::<Vec<_>>(), vec![root, energy]);
        assert_eq!(symbols(&list), vec!["ES", "GC"]);

        store.remove_section(energy);
        let list = store.watchlist_sections(scratch);
        assert_eq!(list.len(), 1, "back to loose symbols and nothing else");
        assert!(list[0].root);
        assert_eq!(symbols(&list), vec!["ES"], "GC went with the section it was in");
        assert!(store.section_order(scratch).is_empty());
    }

    /// Dragging a section header reorders the sections and nothing else: the
    /// symbols are positioned inside their own section, so they come along
    /// without a single entry being rewritten.
    #[test]
    fn reordering_sections_carries_their_symbols_and_leaves_the_order_inside_them() {
        let store = Store::memory().unwrap();
        let energy = store.add_section(DEFAULT_WATCHLIST, "Energy").unwrap();
        let metals = store.add_section(DEFAULT_WATCHLIST, "Metals").unwrap();
        store.add_to_section(energy, "CL", None);
        store.add_to_section(energy, "NG", None);
        store.add_to_section(metals, "GC", None);
        assert_eq!(store.section_order(DEFAULT_WATCHLIST), vec![energy, metals]);

        store.reorder_sections(DEFAULT_WATCHLIST, &[metals, energy]);

        let list = store.watchlist();
        assert_eq!(list.iter().map(|s| s.id).collect::<Vec<_>>(), vec![metals, energy]);
        assert_eq!(symbols(&list), vec!["GC", "CL", "NG"], "Energy's two stay together, in order");
    }

    /// The nameless bucket is the one section that cannot move. It is held
    /// below everything the named ones are ordered against, so asking for it
    /// anywhere in the order changes nothing.
    #[test]
    fn the_root_stays_first_however_the_named_sections_are_ordered() {
        let store = Store::memory().unwrap();
        let root = store.root_section(DEFAULT_WATCHLIST);
        store.add_to_root("SPY", None);
        let energy = store.add_section(DEFAULT_WATCHLIST, "Energy").unwrap();
        store.add_to_section(energy, "CL", None);

        assert!(!store.section_order(DEFAULT_WATCHLIST).contains(&root), "it is not orderable");
        store.reorder_sections(DEFAULT_WATCHLIST, &[energy, root]);

        let list = store.watchlist();
        assert_eq!(list[0].id, root, "still the first row in the rail");
        assert_eq!(list[1].id, energy);
    }

    /// Sections of other watchlists are ordered against their own, so an id
    /// from somewhere else is ignored rather than given a position in this one.
    #[test]
    fn reordering_only_touches_the_watchlist_asked_about() {
        let store = Store::memory().unwrap();
        let scratch = store.add_watchlist("Scratch").unwrap();
        let mine = store.add_section(DEFAULT_WATCHLIST, "Mine").unwrap();
        let theirs = store.add_section(scratch, "Theirs").unwrap();

        store.reorder_sections(DEFAULT_WATCHLIST, &[theirs, mine]);

        assert_eq!(store.section_order(DEFAULT_WATCHLIST), vec![mine]);
        assert_eq!(store.section_order(scratch), vec![theirs]);
    }

    /// The database on a machine that has been running this app has sections
    /// A build older than the one that last wrote the file has to stop here,
    /// where the caller can still tell this apart from a database it simply
    /// could not read. Falling back to an empty one would put a pristine
    /// watchlist on screen over the top of somebody's real one.
    #[test]
    fn a_database_from_a_newer_omacharts_is_refused_rather_than_opened() {
        let path =
            std::env::temp_dir().join(format!("omacharts-future-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);

        let store = Store::open_at(&path).unwrap();
        store.add_section(DEFAULT_WATCHLIST, "Mine").unwrap();
        drop(store);

        let ahead = Connection::open(&path).unwrap();
        ahead.pragma_update(None, "user_version", migrations::LATEST + 1).unwrap();
        drop(ahead);

        match Store::open_at(&path) {
            Err(migrations::Error::FromTheFuture { found, known }) => {
                assert_eq!((found, known), (migrations::LATEST + 1, migrations::LATEST));
            }
            other => panic!("expected a refusal, got {:?}", other.map(|_| "a store")),
        }
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_symbol_put_in_one_watchlist_stays_out_of_the_others() {
        let store = Store::memory().unwrap();
        let scratch = store.add_watchlist("Scratch").unwrap();

        store.add_to_root("SPY", None);
        store.add_to_section(store.root_section(scratch), "BTC", None);

        assert_eq!(symbols(&store.watchlist()), vec!["SPY"]);
        assert_eq!(symbols(&store.watchlist_sections(scratch)), vec!["BTC"]);
    }

    /// Every watchlist needs somewhere to put a symbol that is not in a
    /// section, and sharing one would be the same bug as sharing the symbols.
    #[test]
    fn each_watchlist_gets_a_root_of_its_own() {
        let store = Store::memory().unwrap();
        let scratch = store.add_watchlist("Scratch").unwrap();
        assert_eq!(store.root_section(DEFAULT_WATCHLIST), ROOT_SECTION);
        assert_ne!(store.root_section(scratch), ROOT_SECTION);
    }

    #[test]
    fn the_default_watchlist_can_be_renamed_but_not_deleted() {
        let store = Store::memory().unwrap();
        store.rename_watchlist(DEFAULT_WATCHLIST, "Majors");
        store.remove_watchlist(DEFAULT_WATCHLIST);
        assert_eq!(store.watchlists(), vec![(DEFAULT_WATCHLIST, "Majors".to_string())]);
    }

    #[test]
    fn deleting_a_watchlist_takes_its_sections_and_symbols() {
        let store = Store::memory().unwrap();
        let scratch = store.add_watchlist("Scratch").unwrap();
        let section = store.add_section(scratch, "Metals").unwrap();
        store.add_to_section(section, "GC", None);

        store.remove_watchlist(scratch);
        assert!(!store.watchlist_exists(scratch));
        assert!(store.watchlist_sections(scratch).is_empty());
        assert!(store.watchlist().is_empty(), "the default one is untouched and still empty");
    }

    /// What the bar widget reads. Switching the rail to a scratch list is
    /// something you do while looking at the app; the system bar has no
    /// business changing because of it.
    #[test]
    fn the_bar_widget_reads_the_default_watchlist_whatever_else_exists() {
        let store = Store::memory().unwrap();
        store.add_to_root("SPY", None);
        let scratch = store.add_watchlist("Scratch").unwrap();
        store.add_to_section(store.root_section(scratch), "BTC", None);

        assert_eq!(symbols(&store.watchlist()), vec!["SPY"]);
    }

    #[test]
    fn sections_keep_the_order_they_were_made_in() {
        let store = Store::memory().unwrap();
        for name in ["First", "Second", "Third"] {
            store.add_section(DEFAULT_WATCHLIST, name);
        }
        let names: Vec<String> = store.watchlist().into_iter().map(|s| s.name).collect();
        assert_eq!(names, vec!["First", "Second", "Third"]);
    }

    #[test]
    fn adding_the_same_symbol_twice_is_harmless() {
        let store = Store::memory().unwrap();
        let id = store.add_section(DEFAULT_WATCHLIST, "Watchlist").unwrap();
        store.add_to_section(id, "ES", None);
        store.add_to_section(id, "ES", None);
        assert_eq!(store.watchlist()[0].entries.len(), 1);
    }

    #[test]
    fn the_root_cannot_be_removed() {
        let store = Store::memory().unwrap();
        store.add_to_root("ES", None);
        store.remove_section(ROOT_SECTION);
        assert_eq!(store.watchlist()[0].entries.len(), 1, "the root survives");
    }

    #[test]
    fn removing_a_section_takes_its_entries() {
        let store = Store::memory().unwrap();
        let id = store.add_section(DEFAULT_WATCHLIST, "Temp").unwrap();
        store.add_to_section(id, "ES", None);
        store.remove_section(id);
        assert!(store.watchlist().is_empty());
        let count: i64 = store
            .conn
            .query_row("SELECT COUNT(*) FROM watchlist_entries", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn sections_remember_being_collapsed() {
        let store = Store::memory().unwrap();
        let id = store.add_section(DEFAULT_WATCHLIST, "Futures").unwrap();
        assert!(!store.watchlist()[0].collapsed, "new sections start open");

        store.set_section_collapsed(id, true);
        assert!(store.watchlist()[0].collapsed);

        store.set_section_collapsed(id, false);
        assert!(!store.watchlist()[0].collapsed);
    }

    #[test]
    fn sections_can_be_renamed_and_entries_removed() {
        let store = Store::memory().unwrap();
        let id = store.add_section(DEFAULT_WATCHLIST, "Old").unwrap();
        store.add_to_section(id, "ES", None);
        store.add_to_section(id, "NQ", None);
        store.rename_section(id, "New");
        store.remove_from_section(id, "ES", None);

        let list = store.watchlist();
        assert_eq!(list[0].name, "New");
        assert_eq!(list[0].entries.len(), 1);
        assert_eq!(list[0].entries[0].symbol, "NQ");
    }

    #[test]
    fn seeding_only_happens_once() {
        let store = Store::memory().unwrap();
        let defaults: &[(&str, &[&str])] = &[("Indexes", &["GSPC"]), ("Crypto", &["BTC"])];
        store.seed_watchlist_if_empty(defaults);
        store.seed_watchlist_if_empty(defaults);
        assert_eq!(store.watchlist().len(), 2);
    }

    #[test]
    fn entries_can_be_reordered() {
        let store = Store::memory().unwrap();
        let id = store.add_section(DEFAULT_WATCHLIST, "W").unwrap();
        for symbol in ["A", "B", "C"] {
            store.add_to_section(id, symbol, None);
        }
        let entry = |s: &str| Entry { symbol: s.into(), suffix: None };

        // Drag C above A.
        store.move_entry(id, &entry("C"), &entry("A"));
        let order: Vec<String> =
            store.watchlist()[0].entries.iter().map(|e| e.symbol.clone()).collect();
        assert_eq!(order, vec!["C", "A", "B"]);

        // And back to the end.
        store.reorder_entries(id, &[entry("A"), entry("B"), entry("C")]);
        let order: Vec<String> =
            store.watchlist()[0].entries.iter().map(|e| e.symbol.clone()).collect();
        assert_eq!(order, vec!["A", "B", "C"]);
    }

    #[test]
    fn an_entry_can_move_between_sections() {
        let store = Store::memory().unwrap();
        let a = store.add_section(DEFAULT_WATCHLIST, "A").unwrap();
        let b = store.add_section(DEFAULT_WATCHLIST, "B").unwrap();
        store.add_to_section(a, "ES", None);
        store.add_to_section(a, "NQ", None);
        store.add_to_section(b, "BTC", None);
        let entry = |s: &str| Entry { symbol: s.into(), suffix: None };

        // Dropped onto BTC, so it lands above it.
        store.move_entry_to_section(a, b, &entry("ES"), Some(&entry("BTC")));

        let list = store.watchlist();
        let in_a: Vec<String> = list[0].entries.iter().map(|e| e.symbol.clone()).collect();
        let in_b: Vec<String> = list[1].entries.iter().map(|e| e.symbol.clone()).collect();
        assert_eq!(in_a, vec!["NQ"]);
        assert_eq!(in_b, vec!["ES", "BTC"]);
    }

    #[test]
    fn dropping_on_a_section_rather_than_a_row_appends() {
        let store = Store::memory().unwrap();
        let a = store.add_section(DEFAULT_WATCHLIST, "A").unwrap();
        let b = store.add_section(DEFAULT_WATCHLIST, "B").unwrap();
        store.add_to_section(a, "ES", None);
        store.add_to_section(b, "BTC", None);
        store.move_entry_to_section(a, b, &Entry { symbol: "ES".into(), suffix: None }, None);

        let in_b: Vec<String> =
            store.watchlist()[1].entries.iter().map(|e| e.symbol.clone()).collect();
        assert_eq!(in_b, vec!["BTC", "ES"]);
    }

    #[test]
    fn moving_within_a_section_still_reorders() {
        let store = Store::memory().unwrap();
        let a = store.add_section(DEFAULT_WATCHLIST, "A").unwrap();
        for symbol in ["X", "Y", "Z"] {
            store.add_to_section(a, symbol, None);
        }
        let entry = |s: &str| Entry { symbol: s.into(), suffix: None };
        store.move_entry_to_section(a, a, &entry("Z"), Some(&entry("X")));
        let order: Vec<String> =
            store.watchlist()[0].entries.iter().map(|e| e.symbol.clone()).collect();
        assert_eq!(order, vec!["Z", "X", "Y"]);
    }

    #[test]
    fn moving_onto_an_unknown_target_appends() {
        let store = Store::memory().unwrap();
        let id = store.add_section(DEFAULT_WATCHLIST, "W").unwrap();
        for symbol in ["A", "B"] {
            store.add_to_section(id, symbol, None);
        }
        let entry = |s: &str| Entry { symbol: s.into(), suffix: None };
        store.move_entry(id, &entry("A"), &entry("GONE"));
        let order: Vec<String> =
            store.watchlist()[0].entries.iter().map(|e| e.symbol.clone()).collect();
        assert_eq!(order, vec!["B", "A"]);
    }

    #[test]
    fn moving_something_that_is_not_there_is_a_no_op() {
        let store = Store::memory().unwrap();
        let id = store.add_section(DEFAULT_WATCHLIST, "W").unwrap();
        store.add_to_section(id, "A", None);
        let entry = |s: &str| Entry { symbol: s.into(), suffix: None };
        store.move_entry(id, &entry("ZZ"), &entry("A"));
        assert_eq!(store.watchlist()[0].entries.len(), 1);
    }

    /// Ten series of a thousand bars, a limit set halfway through them, and
    /// the ones fetched longest ago are what goes.
    #[test]
    fn a_cache_over_its_limit_loses_its_oldest_series_first() {
        let store = Store::memory().unwrap();
        let bars: Vec<Bar> = (0..1000).map(|i| bar(i, i as f64)).collect();
        for series in 0..10 {
            store.write_bars(&format!("yahoo:S{series}"), Timeframe::days(1), &bars);
            // write_bars stamps them all in the same second, so the order has
            // to be made explicit or there is nothing to call oldest.
            store
                .conn
                .execute(
                    "UPDATE bar_series SET fetched_at = ?2 WHERE key = ?1",
                    params![format!("yahoo:S{series}"), 1_700_000_000i64 + series],
                )
                .unwrap();
        }

        let full = store.used_bytes();
        let limit = full / 2;
        let swept = crate::cache::evict(&store.conn, limit).unwrap();

        assert!(swept.dropped > 0, "something had to go to get under {limit}");
        assert!(swept.after <= limit, "{} is still over {limit}", swept.after);
        assert!(
            store.load_bars("yahoo:S0", Timeframe::days(1)).is_empty(),
            "the oldest should have gone first"
        );
        assert_eq!(
            store.load_bars("yahoo:S9", Timeframe::days(1)).len(),
            1000,
            "the newest should still be there"
        );
    }

    /// The usual case: a cache nowhere near its limit is left alone, and
    /// nothing is read or rewritten to find that out.
    #[test]
    fn a_cache_under_its_limit_is_left_alone() {
        let store = Store::memory().unwrap();
        store.write_bars("yahoo:ES=F", Timeframe::days(1), &[bar(100, 1.0)]);

        let swept = crate::cache::evict(&store.conn, 1024 * 1024 * 1024).unwrap();
        assert_eq!(swept.dropped, 0);
        assert_eq!(swept.before, swept.after);
        assert_eq!(store.cached_series(), 1);
    }

    /// Six hours between sweeps, so a launch is never held up by work that
    /// was done this morning.
    #[test]
    fn a_sweep_waits_out_its_cadence_but_a_fresh_database_does_not() {
        let store = Store::memory().unwrap();
        assert!(store.cache_sweep_due(), "nothing has ever swept this one");

        let now = chrono::Utc::now().timestamp();
        store.set_setting(crate::cache::SETTING_SWEPT_AT, &now.to_string());
        assert!(!store.cache_sweep_due(), "swept just now");

        store.set_setting(
            crate::cache::SETTING_SWEPT_AT,
            &(now - crate::cache::CADENCE - 1).to_string(),
        );
        assert!(store.cache_sweep_due(), "swept longer ago than the cadence");
    }

    /// The limit is a number people choose from a short list, and a database
    /// that has never been asked still has to answer.
    #[test]
    fn the_cache_limit_falls_back_to_a_gigabyte() {
        let store = Store::memory().unwrap();
        assert_eq!(store.cache_limit(), crate::cache::DEFAULT_LIMIT);

        store.set_cache_limit(512 * 1024 * 1024);
        assert_eq!(store.cache_limit(), 512 * 1024 * 1024);

        // Nonsense in the settings table is not a reason to have no limit.
        store.set_setting(crate::cache::SETTING_LIMIT, "nonsense");
        assert_eq!(store.cache_limit(), crate::cache::DEFAULT_LIMIT);
    }

    #[test]
    fn a_cache_written_by_an_older_version_is_discarded_once() {
        let file = std::env::temp_dir().join(format!("omacharts-cache-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&file);

        {
            let store = Store::open_at(&file).unwrap();
            store.write_bars("k", Timeframe::days(1), &[bar(100, 1.0)]);
            store.set_setting("keep", "me");
            // Pretend it was written before the fix.
            store
                .conn
                .execute("UPDATE meta SET value = '1' WHERE key = 'cache_version'", [])
                .unwrap();
        }

        let store = Store::open_at(&file).unwrap();
        assert_eq!(store.cached_series(), 0, "stale bars are dropped");
        assert_eq!(store.setting("keep").as_deref(), Some("me"), "settings survive");

        // And a second open does not drop anything again.
        store.write_bars("k", Timeframe::days(1), &[bar(100, 1.0)]);
        drop(store);
        let store = Store::open_at(&file).unwrap();
        assert_eq!(store.cached_series(), 1);

        let _ = std::fs::remove_file(&file);
    }


    /// The app writes "1" and "0", but `omacharts config set <key> false` puts
    /// a word there, and a reader that understood only the digits treated it
    /// as unset — so the command appeared to do nothing at all.
    #[test]
    fn a_flag_is_read_the_way_people_write_one() {
        let store = Store::memory().unwrap();
        for yes in ["1", "true", "TRUE", "yes", "on", " true "] {
            store.set_setting("flag", yes);
            assert!(store.setting_bool("flag", false), "{yes:?} should read as true");
        }
        for no in ["0", "false", "False", "no", "off"] {
            store.set_setting("flag", no);
            assert!(!store.setting_bool("flag", true), "{no:?} should read as false");
        }

        // Anything else is not an answer, so the default stands.
        store.set_setting("flag", "maybe");
        assert!(store.setting_bool("flag", true));
        assert!(!store.setting_bool("flag", false));
        assert!(store.setting_bool("never set", true));
    }

    /// A fresh install opens on an empty chart. Volume was the default here
    /// when it was drawn unconditionally, which made the very first chart
    /// arrive carrying an indicator nobody had added.
    #[test]
    fn a_chart_nobody_has_configured_carries_nothing() {
        use omacharts_engine::IndicatorKind;
        let store = Store::memory().unwrap();
        assert!(store.indicators().is_empty(), "a fresh chart came with indicators on it");

        // And what somebody did add comes back.
        store.set_indicators(&[Indicator::new(1, IndicatorKind::Volume)]);
        assert_eq!(store.indicators().len(), 1);
    }

    #[test]
    fn indicators_round_trip() {
        use omacharts_engine::IndicatorKind;
        let store = Store::memory().unwrap();

        let set = vec![Indicator::new(1, IndicatorKind::Sma), Indicator::new(2, IndicatorKind::Vwap)];
        store.set_indicators(&set);
        assert_eq!(store.indicators(), set);

        store.set_indicators(&[]);
        assert!(store.indicators().is_empty());
    }

    #[test]
    fn drawings_belong_to_a_symbol_and_survive_a_round_trip() {
        use omacharts_engine::{Anchor, DrawingKind, Preset};
        let store = Store::memory().unwrap();
        assert!(store.drawings("AAPL", None).is_empty(), "a fresh symbol came with drawings");

        let mut line = Drawing::new(DrawingKind::Line, Anchor::new(100, 1.0), Anchor::new(200, 2.0));
        line.id = store.add_drawing("AAPL", None, &line).unwrap();
        let mut rect = Drawing::new(DrawingKind::Rect, Anchor::new(300, 3.0), Anchor::new(400, 4.0));
        rect.preset = Preset::Amber;
        rect.id = store.add_drawing("AAPL", None, &rect).unwrap();
        assert_ne!(line.id, rect.id);
        assert_eq!(store.drawings("AAPL", None), vec![line.clone(), rect.clone()]);

        // Another symbol, and the same symbol on another exchange, are other
        // charts.
        assert!(store.drawings("NVDA", None).is_empty());
        assert!(store.drawings("AAPL", Some("L")).is_empty());

        line.to = Anchor::new(250, 2.5);
        store.update_drawing(&line);
        assert_eq!(store.drawings("AAPL", None)[0].to, Anchor::new(250, 2.5));

        store.remove_drawing(line.id);
        assert_eq!(store.drawings("AAPL", None), vec![rect]);
        store.clear_drawings("AAPL", None);
        assert!(store.drawings("AAPL", None).is_empty());
    }

    #[test]
    fn a_drawing_that_was_never_added_is_not_written_by_an_update() {
        use omacharts_engine::{Anchor, DrawingKind};
        let store = Store::memory().unwrap();
        let unsaved = Drawing::new(DrawingKind::Line, Anchor::new(1, 1.0), Anchor::new(2, 2.0));
        store.update_drawing(&unsaved);
        assert!(store.drawings("AAPL", None).is_empty());
    }

    #[test]
    fn an_unreadable_indicator_list_is_ignored_rather_than_fatal() {
        let store = Store::memory().unwrap();
        store.set_setting("indicators", "{ this is not json");
        assert!(store.indicators().is_empty());
    }

    #[test]
    fn settings_round_trip() {
        let store = Store::memory().unwrap();
        assert_eq!(store.setting("missing"), None);
        assert!(store.setting_bool("auto", true));
        store.set_setting_bool("auto", false);
        assert!(!store.setting_bool("auto", true));
    }

    #[test]
    fn saved_themes_round_trip_and_delete() {
        let store = Store::memory().unwrap();
        let mine = omacharts_engine::theme::builtin_themes()[0].duplicate("mine", "Mine");
        store.save_theme(&mine);
        assert_eq!(store.custom_themes(), vec![mine.clone()]);
        store.delete_theme("mine");
        assert!(store.custom_themes().is_empty());
    }

    #[test]
    fn an_empty_write_removes_the_series() {
        let store = Store::memory().unwrap();
        store.write_bars("k", Timeframe::days(1), &[bar(100, 1.0)]);
        store.write_bars("k", Timeframe::days(1), &[]);
        assert!(store.coverage("k", Timeframe::days(1)).is_none());
    }
}

