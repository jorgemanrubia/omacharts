//! Copies of the settings file to go back to.
//!
//! Everything somebody made — watchlists built up over months, chartbooks,
//! drawings, a theme picked colour by colour — lives in one small file, and
//! one bad command or one bad build is all it takes to lose it. So the app
//! keeps copies of that file on its own, in `backups/` beside it, and any of
//! them can be put back while the window is open.
//!
//! Only the settings file. The market data beside it is a cache that can be
//! fetched again, and is hundreds of times the size.
//!
//! A copy is taken with `VACUUM main INTO` a temporary name and then renamed:
//! SQLite knows about the WAL, so the copy has everything committed, and a
//! rename means nobody ever lists a half-written one. Taking one is a
//! function of a connection and a directory and nothing else, because a copy
//! that can be made anywhere is also the thing a later sync would send.
//!
//! Names are the moment in UTC, `settings-20261021-140307.db`: they sort the
//! way they were taken, and they do not change meaning when the clock moves
//! for summer time. They are shown in local time.

use std::path::{Path, PathBuf};

use chrono::{Datelike, DateTime, Local, NaiveDate, NaiveDateTime, TimeZone, Utc};
use rusqlite::Connection;

use crate::migrations;
use crate::setting_keys;
use crate::store::Store;

/// How far back the recent ones are kept closely: one in every
/// [`RECENT_EVERY_HOURS`] hours of this many, which is what undoing this
/// morning's mistake needs.
pub const RECENT_HOURS: i64 = 24;
pub const RECENT_EVERY_HOURS: i64 = 2;

/// How many days back one a day is kept, today included: a week behind the
/// last day, for somebody who noticed on Thursday what went wrong on Monday.
/// With the recent ones, never more than twenty files.
pub const KEEP_DAYS: i64 = 8;

/// The most that can be kept at once, which is what the preferences quote.
pub const MOST_KEPT: i64 = RECENT_HOURS / RECENT_EVERY_HOURS + KEEP_DAYS;

/// How often the window takes one, if anything changed.
///
/// An hour, so that every two-hour slot of the last day has one to keep if
/// anything changed in it. A timer, which this app otherwise does not
/// use to notice things: this one is not watching for anything, it is a
/// cadence, and an hour of edits is the most a crash can cost.
pub const CADENCE_SECONDS: u32 = 60 * 60;

const PREFIX: &str = "settings-";
const SUFFIX: &str = ".db";
const STAMP: &str = "%Y%m%d-%H%M%S";

/// One copy of the settings file.
#[derive(Clone, Debug, PartialEq)]
pub struct Backup {
    /// The file name, which is also what a command names it by.
    pub name: String,
    pub path: PathBuf,
    pub taken: DateTime<Utc>,
    pub bytes: u64,
    /// The schema version it was written at, read from the file's header.
    /// `None` for a file too short to have one.
    pub schema: Option<i32>,
}

/// What putting a backup back did.
#[derive(Debug)]
pub struct Restored {
    pub from: Backup,
    /// What was there before, kept as a backup like any other, so that a
    /// restore is undone by restoring this.
    pub kept: Backup,
}

#[derive(Debug)]
pub enum Error {
    /// An in-memory store, which has no file to copy.
    NoFile,
    NotFound(String),
    /// A file chosen to restore from that is not an Omacharts settings file.
    NotSettings(String),
    /// A backup from a newer Omacharts than this one. Refused before
    /// anything is touched: this build cannot know what the newer columns
    /// meant, and half-applying them is worse than not starting.
    FromTheFuture { name: String, found: i32, known: i32 },
    Io(std::io::Error),
    Database(migrations::Error),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::NoFile => write!(f, "these settings are not saved to a file, so there is nothing to back up"),
            Error::NotFound(name) => {
                write!(f, "no backup called {name:?}; `omacharts backup list` shows them")
            }
            Error::NotSettings(name) => {
                write!(f, "{name} is not an Omacharts settings file; nothing was changed")
            }
            Error::FromTheFuture { name, found, known } => write!(
                f,
                "{name} was written by a newer Omacharts (schema {found}, this build knows \
                 {known}); nothing was changed. Upgrade Omacharts to restore it"
            ),
            Error::Io(error) => write!(f, "{error}"),
            Error::Database(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(error: std::io::Error) -> Error {
        Error::Io(error)
    }
}

impl From<rusqlite::Error> for Error {
    fn from(error: rusqlite::Error) -> Error {
        Error::Database(migrations::Error::Sqlite(error))
    }
}

impl From<migrations::Error> for Error {
    fn from(error: migrations::Error) -> Error {
        Error::Database(error)
    }
}

/// Where the backups of a settings file live: beside it, where somebody
/// looking for them would look.
pub fn dir(settings: &Path) -> PathBuf {
    settings.with_file_name("backups")
}

/// Every backup of `settings`, newest first.
///
/// Anything in the folder that is not named like one is not one — a copy
/// being written, or a file somebody put there — and is left out rather than
/// shown or pruned.
pub fn list(settings: &Path) -> Vec<Backup> {
    let Ok(entries) = std::fs::read_dir(dir(settings)) else { return Vec::new() };
    let mut found: Vec<Backup> = entries
        .filter_map(Result::ok)
        .filter_map(|entry| describe(&entry.path()))
        .collect();
    found.sort_by(|a, b| b.taken.cmp(&a.taken).then_with(|| b.name.cmp(&a.name)));
    found
}

fn describe(path: &Path) -> Option<Backup> {
    let name = path.file_name()?.to_str()?.to_string();
    let taken = taken_from(&name)?;
    let metadata = std::fs::metadata(path).ok().filter(|m| m.is_file())?;
    Some(Backup {
        schema: schema_of(path),
        path: path.to_path_buf(),
        bytes: metadata.len(),
        taken,
        name,
    })
}

fn taken_from(name: &str) -> Option<DateTime<Utc>> {
    let stamp = name.strip_prefix(PREFIX)?.strip_suffix(SUFFIX)?;
    NaiveDateTime::parse_from_str(stamp, STAMP).ok().map(|naive| naive.and_utc())
}

/// `user_version`, out of the header rather than through SQLite: it is four
/// bytes at a fixed offset, and listing a folder of backups should not mean
/// opening a database per row.
fn schema_of(path: &Path) -> Option<i32> {
    use std::io::Read;
    let mut header = [0u8; 64];
    std::fs::File::open(path).ok()?.read_exact(&mut header).ok()?;
    if !header.starts_with(b"SQLite format 3\0") {
        return None;
    }
    Some(i32::from_be_bytes([header[60], header[61], header[62], header[63]]))
}

/// Take a backup through `store`'s connection, then prune.
///
/// The second half of the answer is whether it is new. A copy whose settings
/// are the newest one's is not kept: it would push a real one out without
/// adding anything. "The same" leaves out what the app writes as it runs —
/// [`setting_keys::Kind::Bookkeeping`] — so a cache sweep or a remembered
/// symbol is not a reason for a backup.
pub fn take(store: &Store) -> Result<(Backup, bool), Error> {
    let settings = store.path().ok_or(Error::NoFile)?.to_path_buf();
    let taken = write(&settings, |to| store.snapshot_settings(to))?;
    prune(&settings, Local::now());
    Ok(taken)
}

/// Take one only if the settings file has been written since the newest
/// backup. For the window's own cadence, on a thread of its own.
///
/// The modification times of the file and its WAL against the newest
/// backup's are the cheap question; they can say yes when nothing that
/// matters changed — a checkpoint rewrites the file — and the comparison in
/// [`take`] catches that.
pub fn take_if_changed(settings: &Path) -> Result<Option<Backup>, Error> {
    let modified = |path: &Path| std::fs::metadata(path).and_then(|m| m.modified()).ok();
    let written = [settings.to_path_buf(), wal_of(settings)]
        .iter()
        .filter_map(|path| modified(path))
        .max();
    let Some(written) = written else { return Ok(None) };
    if let Some(newest) = list(settings).first()
        && modified(&newest.path).is_some_and(|at| at >= written)
    {
        return Ok(None);
    }
    // Its own connection, to the settings file alone: the window's is on
    // another thread, and the market data is not being copied.
    let conn = Connection::open(settings)?;
    let store = Store::adopt(conn);
    let (backup, fresh) = write(settings, |to| store.snapshot_settings(to))?;
    prune(settings, Local::now());
    Ok(fresh.then_some(backup))
}

fn wal_of(settings: &Path) -> PathBuf {
    PathBuf::from(format!("{}-wal", settings.display()))
}

/// The copy itself: `snapshot` writes a database to the path it is given,
/// which is a temporary one until it is complete.
fn write(
    settings: &Path,
    snapshot: impl FnOnce(&Path) -> rusqlite::Result<()>,
) -> Result<(Backup, bool), Error> {
    let folder = dir(settings);
    std::fs::create_dir_all(&folder)?;
    let name = format!("{PREFIX}{}{SUFFIX}", Utc::now().format(STAMP));
    // A leading dot and no `.db`, so that [`list`] never sees it.
    let partial = folder.join(format!(".{name}.partial-{}", std::process::id()));
    let _ = std::fs::remove_file(&partial);
    if let Err(error) = snapshot(&partial) {
        let _ = std::fs::remove_file(&partial);
        return Err(error.into());
    }

    if let Some(newest) = list(settings).into_iter().next()
        && same_settings(&partial, &newest.path)
    {
        std::fs::remove_file(&partial)?;
        return Ok((newest, false));
    }
    // Two in one second — commands chained by an agent — must not overwrite
    // each other: the earlier one is a state that existed. The later is
    // named a second on, and on until a name is free.
    let mut at = Utc::now();
    let mut path = folder.join(&name);
    while path.exists() {
        at += chrono::Duration::seconds(1);
        path = folder.join(format!("{PREFIX}{}{SUFFIX}", at.format(STAMP)));
    }
    std::fs::rename(&partial, &path)?;
    let backup = describe(&path).ok_or_else(|| std::io::Error::other("the backup vanished"))?;
    Ok((backup, true))
}

fn same_settings(a: &Path, b: &Path) -> bool {
    matches!((fingerprint(a), fingerprint(b)), (Some(x), Some(y)) if x == y)
}

/// Everything somebody made in a settings file, as one number: every table
/// but `meta`, every row in a fixed order, with the bookkeeping settings left
/// out. Read only, and small — the settings file is tens of kilobytes.
fn fingerprint(path: &Path) -> Option<u64> {
    use std::hash::Hasher;
    let conn = Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).ok()?;
    let mut tables: Vec<String> = {
        let mut stmt = conn
            .prepare(
                "SELECT name FROM sqlite_master
                 WHERE type = 'table' AND name NOT LIKE 'sqlite_%' AND name != 'meta'",
            )
            .ok()?;
        stmt.query_map([], |row| row.get(0)).ok()?.filter_map(Result::ok).collect()
    };
    tables.sort();
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    for table in tables {
        hash.write(table.as_bytes());
        let mut stmt = conn.prepare(&format!("SELECT * FROM \"{table}\" ORDER BY 1, 2")).ok()?;
        let width = stmt.column_count();
        let mut rows = stmt.query([]).ok()?;
        while let Some(row) = rows.next().ok()? {
            if table == "settings"
                && row.get::<_, String>(0).is_ok_and(|key| {
                    setting_keys::kind(&key) == setting_keys::Kind::Bookkeeping
                })
            {
                continue;
            }
            for column in 0..width {
                hash.write(format!("{:?}", row.get_ref(column).ok()?).as_bytes());
                hash.write_u8(0);
            }
        }
    }
    Some(hash.finish())
}

/// Delete the backups the retention rule does not keep.
fn prune(settings: &Path, now: DateTime<Local>) {
    let backups = list(settings);
    let taken: Vec<DateTime<Local>> =
        backups.iter().map(|backup| backup.taken.with_timezone(&Local)).collect();
    for at in doomed(&taken, &now) {
        if let Some(backup) = backups.get(at) {
            let _ = std::fs::remove_file(&backup.path);
        }
    }
}

/// Which of `taken`, newest first, go: everything but the newest in each
/// [`RECENT_EVERY_HOURS`]-hour slot of the last [`RECENT_HOURS`], and the
/// newest of each of the last [`KEEP_DAYS`] calendar days, in the time zone
/// they are given in.
///
/// Calendar days in somebody's own zone rather than UTC's, because "the
/// backup from Tuesday" means their Tuesday.
fn doomed<Tz: TimeZone>(taken: &[DateTime<Tz>], now: &DateTime<Tz>) -> Vec<usize> {
    let today = now.date_naive();
    let oldest_day = today - chrono::Duration::days(KEEP_DAYS - 1);
    let mut slots_kept: Vec<i64> = Vec::new();
    let mut days_kept: Vec<NaiveDate> = Vec::new();
    let mut doomed = Vec::new();
    for (at, when) in taken.iter().enumerate() {
        // From a clock that was ahead and has since been put right: kept, or
        // every backup taken meanwhile would go at once.
        if when > now {
            continue;
        }
        let hours = (now.clone() - when.clone()).num_hours();
        // Slots fixed to the clock, not counted back from now: counted back,
        // each hourly backup shared the newest's slot and was pruned before
        // it ever reached a slot of its own.
        let slot = when.timestamp().div_euclid(RECENT_EVERY_HOURS * 3600);
        let a_slot_kept = (0..RECENT_HOURS).contains(&hours) && !slots_kept.contains(&slot);
        if a_slot_kept {
            slots_kept.push(slot);
        }
        let day = when.date_naive();
        let a_day_kept = day >= oldest_day && day <= today && !days_kept.contains(&day);
        if a_day_kept {
            days_kept.push(day);
        }
        if !a_slot_kept && !a_day_kept {
            doomed.push(at);
        }
    }
    doomed
}

/// Put the backup called `name` back, through `store`'s connection, while
/// whatever else has the file open carries on.
///
/// In order, so that a refusal changes nothing:
///
/// 1. The backup is copied aside and that copy opened the normal way, which
///    runs the migrations over it. One from an older build arrives in this
///    build's shape; one from a newer build is refused here.
/// 2. What is there now is backed up, so the restore can be undone.
/// 3. Every table of the settings file is emptied and filled from the copy
///    in one transaction. Rows rather than the file: the window has the file
///    open, and a file replaced under a live connection is one the
///    connection goes on reading the old inode of, or worse.
///
/// The market data is not touched. Whoever holds a window has to read the
/// settings back afterwards — see [`crate::cli::Live`].
pub fn restore(store: &Store, name: &str) -> Result<Restored, Error> {
    let settings = store.path().ok_or(Error::NoFile)?;
    let from = find(settings, name)?;
    restore_backup(store, from)
}

/// Put back the settings in any file — a backup brought over from another
/// machine, or that machine's `omacharts.db` itself — the same way.
///
/// Checked first for being settings at all: opening any other database the
/// normal way would migrate it into an empty set of settings, and restoring
/// that would wipe these.
pub fn restore_file(store: &Store, path: &Path) -> Result<Restored, Error> {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    std::fs::metadata(path)
        .map_err(|error| std::io::Error::new(error.kind(), format!("{}: {error}", path.display())))?;
    if !is_settings(path) {
        return Err(Error::NotSettings(name));
    }
    let metadata = std::fs::metadata(path)?;
    let taken = metadata.modified().map(DateTime::<Utc>::from).unwrap_or_else(|_| Utc::now());
    let from = Backup { schema: schema_of(path), path: path.to_path_buf(), bytes: metadata.len(), taken, name };
    // From another machine: what is this one's stays this one's.
    let kept = keep_this_machine(store);
    let restored = restore_backup(store, from)?;
    put_back(store, kept);
    Ok(restored)
}

/// Whether `path` is a settings file this app wrote: a database at a schema
/// it has a number for, holding the settings table. Read only.
fn is_settings(path: &Path) -> bool {
    if !schema_of(path).is_some_and(|version| version >= 1) {
        return false;
    }
    Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
        .and_then(|conn| {
            conn.query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name IN ('settings', 'watchlists')",
                [],
                |r| r.get::<_, i64>(0),
            )
        })
        .is_ok_and(|tables| tables == 2)
}

/// What this machine keeps when settings arrive from another: its local and
/// bookkeeping settings.
fn keep_this_machine(store: &Store) -> Vec<(String, String)> {
    store
        .settings()
        .into_iter()
        .filter(|(key, _)| setting_keys::kind(key) != setting_keys::Kind::Shared)
        .collect()
}

fn put_back(store: &Store, kept: Vec<(String, String)>) {
    for (key, _) in store.settings() {
        if setting_keys::kind(&key) != setting_keys::Kind::Shared {
            store.remove_setting(&key);
        }
    }
    for (key, value) in kept {
        store.set_setting(&key, &value);
    }
    // Each chartbook remembers how wide its sidebar was, on the screen it was
    // last on. Forgotten, a book takes this machine's own width, which is
    // the local setting just put back.
    let Some(mut workspace) =
        store.setting(WORKSPACE).and_then(|json| serde_json::from_str::<serde_json::Value>(&json).ok())
    else {
        return;
    };
    if let Some(books) = workspace["books"].as_array_mut() {
        for book in books {
            book["sidebar_width"] = serde_json::Value::Null;
        }
    }
    store.set_setting(WORKSPACE, &workspace.to_string());
}

/// The arrangement's setting, which is where each chartbook keeps its
/// sidebar width.
const WORKSPACE: &str = "workspace";

fn restore_backup(store: &Store, from: Backup) -> Result<Restored, Error> {
    let settings = store.path().ok_or(Error::NoFile)?.to_path_buf();
    if let Some(found) = from.schema.filter(|found| *found > migrations::LATEST) {
        return Err(Error::FromTheFuture {
            name: from.name.clone(),
            found,
            known: migrations::LATEST,
        });
    }

    // A folder of its own: opening the copy puts an empty market file beside
    // it, and that must not be anybody's real one.
    let aside = dir(&settings).join(format!(".restoring-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&aside);
    std::fs::create_dir_all(&aside)?;
    let result = (|| {
        let copy = aside.join("omacharts.db");
        std::fs::copy(&from.path, &copy)?;
        match Store::open_at(&copy) {
            Ok(opened) => drop(opened),
            Err(migrations::Error::FromTheFuture { found, known }) => {
                return Err(Error::FromTheFuture { name: from.name.clone(), found, known });
            }
            Err(error) => return Err(error.into()),
        }
        // Not `take`, which prunes: a prune here could remove the very backup
        // being restored, before it is restored. The next backup prunes.
        let (kept, _) = write(&settings, |to| store.snapshot_settings(to))?;
        store.replace_settings(&copy)?;
        Ok(Restored { from: from.clone(), kept })
    })();
    let _ = std::fs::remove_dir_all(&aside);
    result
}

/// A backup by name, with or without its `.db`.
pub fn find(settings: &Path, name: &str) -> Result<Backup, Error> {
    let wanted = if name.ends_with(SUFFIX) { name.to_string() } else { format!("{name}{SUFFIX}") };
    list(settings)
        .into_iter()
        .find(|backup| backup.name == wanted)
        .ok_or_else(|| Error::NotFound(name.to_string()))
}

/// When a backup was taken, as somebody would say it: "today at 14:03".
pub fn when(taken: DateTime<Utc>, now: DateTime<Local>) -> String {
    let local = taken.with_timezone(&Local);
    let days = (now.date_naive() - local.date_naive()).num_days();
    match days {
        0 => format!("today at {}", local.format("%H:%M")),
        1 => format!("yesterday at {}", local.format("%H:%M")),
        _ if local.year() == now.year() => local.format("%-d %b at %H:%M").to_string(),
        _ => local.format("%-d %b %Y at %H:%M").to_string(),
    }
}

/// Keep taking backups for as long as the window is open: one when it opens,
/// then one an hour, each only if something changed.
///
/// Every one on a thread of its own. The copy is a few milliseconds for a
/// settings file of any size anybody has, but a few milliseconds on the main
/// loop is a dropped frame while somebody is dragging a chart, and nothing
/// here needs an answer.
pub fn keep_taking(settings: PathBuf) {
    let once = move |settings: PathBuf| {
        std::thread::spawn(move || {
            if let Err(error) = take_if_changed(&settings) {
                eprintln!("omacharts: settings not backed up: {error}");
            }
        });
    };
    once(settings.clone());
    gtk::glib::timeout_add_seconds_local(CADENCE_SECONDS, move || {
        once(settings.clone());
        gtk::glib::ControlFlow::Continue
    });
}

/// A local time as `2026-10-21 14:03:07`, for a listing.
pub fn local_stamp(taken: DateTime<Utc>) -> String {
    taken.with_timezone(&Local).format("%Y-%m-%d %H:%M:%S").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(day: u32, hour: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, day, hour, 0, 0).unwrap()
    }

    /// Newest first, the way [`list`] hands them over.
    fn newest_first(mut taken: Vec<DateTime<Utc>>) -> Vec<DateTime<Utc>> {
        taken.sort_by(|a, b| b.cmp(a));
        taken
    }

    /// A day of hourly backups keeps one in every two hours: twelve.
    #[test]
    fn the_last_day_keeps_one_every_two_hours() {
        let now = at(21, 23);
        let taken = newest_first((0..24).map(|h| now - chrono::Duration::hours(h)).collect());
        let kept = taken.len() - doomed(&taken, &now).len();
        assert_eq!(kept, 12);
    }

    /// Past the last day, one a day survives — the day's newest — for a week
    /// behind today, and nothing older.
    #[test]
    fn older_days_keep_their_newest_for_a_week() {
        let now = at(21, 23);
        let mut taken = Vec::new();
        for back in 2..=9 {
            taken.extend((0..3).map(|h| at(21 - back, 10 + h)));
        }
        let taken = newest_first(taken);
        let gone: Vec<_> = doomed(&taken, &now).into_iter().map(|i| taken[i]).collect();
        // The 14th (7 days back) is the last day kept; the 12th is past it.
        assert!(!gone.contains(&at(14, 12)));
        assert!(gone.contains(&at(12, 12)) && gone.contains(&at(13, 12)));
        assert!(gone.contains(&at(19, 11)) && !gone.contains(&at(19, 12)));
    }

    /// However busy, never more than a slot's worth for the last day plus a
    /// day's for the week: twenty.
    #[test]
    fn a_month_of_hourly_backups_keeps_twenty() {
        let now = at(30, 23);
        let taken = newest_first((0..24 * 30).map(|h| now - chrono::Duration::hours(h)).collect());
        let kept = taken.len() - doomed(&taken, &now).len();
        assert!(kept <= 20, "{kept}");
    }

    /// Pruned after every hourly backup, the way the window does, a week
    /// of changes still leaves one in every two hours of the last day.
    #[test]
    fn pruning_after_every_backup_keeps_the_last_day_in_two_hour_steps() {
        let start = at(1, 0);
        let mut kept: Vec<DateTime<Utc>> = Vec::new();
        for hour in 0..24 * 7 {
            let now = start + chrono::Duration::hours(hour);
            kept.insert(0, now);
            let gone = doomed(&kept, &now);
            kept = kept.iter().enumerate().filter(|(i, _)| !gone.contains(i)).map(|(_, w)| *w).collect();
        }
        let now = start + chrono::Duration::hours(24 * 7 - 1);
        let last_day = kept.iter().filter(|w| (now - **w).num_hours() < 24).count();
        assert_eq!(last_day, 12, "{kept:?}");
        assert!(kept.len() <= 20, "{}", kept.len());
    }

    /// One from a clock that was ahead is not thrown away when the clock is
    /// put right.
    #[test]
    fn a_backup_from_the_future_is_kept() {
        let now = at(21, 12);
        let taken = vec![at(28, 12), at(21, 11)];
        assert!(doomed(&taken, &now).is_empty());
    }

    /// Days are the calendar's in the zone given, not UTC's: 21:00 at UTC-5
    /// on the 20th is the 20th there, though it is the 21st in UTC.
    #[test]
    fn days_are_counted_in_the_zone_given() {
        let zone = chrono::FixedOffset::east_opt(-5 * 3600).unwrap();
        let now = at(23, 23).with_timezone(&zone);
        // 02:00 and 01:00 UTC on the 21st are both the evening of the 20th at
        // UTC-5, and well past the last day, so only the newer is kept.
        let taken = vec![at(21, 2).with_timezone(&zone), at(21, 1).with_timezone(&zone)];
        let gone: Vec<_> = doomed(&taken, &now).into_iter().map(|i| taken[i]).collect();
        assert_eq!(gone, vec![at(21, 1).with_timezone(&zone)]);
    }

    #[test]
    fn names_carry_the_moment_and_nothing_else_is_a_backup() {
        assert_eq!(taken_from("settings-20261021-140307.db"), Some(Utc.with_ymd_and_hms(2026, 10, 21, 14, 3, 7).unwrap()));
        assert_eq!(taken_from(".settings-20261021-140307.db.partial-1"), None);
        assert_eq!(taken_from("settings-yesterday.db"), None);
        assert_eq!(taken_from("omacharts.db"), None);
    }

    /// A directory of its own per test: the market file sits beside the
    /// settings one, and backups beside both.
    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir()
            .join(format!("omacharts-backup-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("omacharts.db")
    }

    fn cleanup(settings: &Path) {
        let _ = std::fs::remove_dir_all(settings.parent().unwrap());
    }

    /// Rename a backup to an earlier second, so a test can take two in a row
    /// without waiting a second between them.
    fn backdate(backup: &Backup, seconds: i64) -> Backup {
        let earlier = backup.taken - chrono::Duration::seconds(seconds);
        let path = backup.path.with_file_name(format!("{PREFIX}{}{SUFFIX}", earlier.format(STAMP)));
        std::fs::rename(&backup.path, &path).unwrap();
        describe(&path).unwrap()
    }

    #[test]
    fn a_backup_is_the_settings_and_not_the_market_data() {
        let settings = scratch("contents");
        let store = Store::open_at(&settings).unwrap();
        store.set_setting("keep", "me");
        store.write_bars("k", omacharts_engine::Timeframe::days(1), &[omacharts_engine::Bar {
            ts: 0, open: 1.0, high: 1.0, low: 1.0, close: 1.0, volume: 1.0,
        }]);

        let (backup, fresh) = take(&store).unwrap();
        assert!(fresh);
        assert_eq!(backup.schema, Some(migrations::LATEST));
        let copy = Connection::open(&backup.path).unwrap();
        let kept: String = copy
            .query_row("SELECT value FROM settings WHERE key = 'keep'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(kept, "me");
        let bars: i64 = copy
            .query_row("SELECT COUNT(*) FROM sqlite_master WHERE name = 'bar_series'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(bars, 0, "the market data came along");
        cleanup(&settings);
    }

    #[test]
    fn nothing_new_is_kept_when_nothing_changed() {
        let settings = scratch("unchanged");
        let store = Store::open_at(&settings).unwrap();
        let (first, _) = take(&store).unwrap();
        // Hours back, into a slot of its own: two in one slot keep only the
        // newer.
        let first = backdate(&first, 3 * 3600);

        let (again, fresh) = take(&store).unwrap();
        assert!(!fresh);
        assert_eq!(again, first);
        assert_eq!(list(&settings).len(), 1);

        assert_eq!(take_if_changed(&settings).unwrap(), None, "unchanged since the newest");
        store.set_setting("moved", "on");
        assert!(take_if_changed(&settings).unwrap().is_some());
        assert_eq!(list(&settings).len(), 2);
        cleanup(&settings);
    }

    #[test]
    fn restoring_puts_the_old_settings_back_and_keeps_what_it_replaced() {
        let settings = scratch("restore");
        let store = Store::open_at(&settings).unwrap();
        store.set_setting("theme", "then");
        let mine = store.add_watchlist("Mine").unwrap();
        store.add_to_section(store.root_section(mine), "AAPL", None);
        let (then, _) = take(&store).unwrap();
        let then = backdate(&then, 60);

        store.set_setting("theme", "now");
        store.remove_watchlist(mine);
        store.add_watchlist("Later").unwrap();
        // A second connection, as the window has while a command restores.
        let window = Store::open_at(&settings).unwrap();

        let restored = restore(&store, &then.name).unwrap();
        assert_eq!(restored.from.name, then.name);
        assert_eq!(window.setting("theme").as_deref(), Some("then"));
        let names: Vec<String> = window.watchlists().into_iter().map(|(_, n)| n).collect();
        assert_eq!(names, vec!["Default", "Mine"]);
        let entries = window.watchlist_sections(mine);
        assert_eq!(entries[0].entries[0].symbol, "AAPL");

        // And it can be undone: what was replaced is a backup of its own.
        let kept = Connection::open(&restored.kept.path).unwrap();
        let theme: String = kept
            .query_row("SELECT value FROM settings WHERE key = 'theme'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(theme, "now");
        let kept = backdate(&restored.kept, 30);
        restore(&store, &kept.name).unwrap();
        assert_eq!(window.setting("theme").as_deref(), Some("now"));
        cleanup(&settings);
    }

    #[test]
    fn the_market_data_survives_a_restore() {
        let settings = scratch("market");
        let store = Store::open_at(&settings).unwrap();
        let (then, _) = take(&store).unwrap();
        let then = backdate(&then, 60);
        store.write_bars("k", omacharts_engine::Timeframe::days(1), &[omacharts_engine::Bar {
            ts: 0, open: 1.0, high: 1.0, low: 1.0, close: 1.0, volume: 1.0,
        }]);
        restore(&store, &then.name).unwrap();
        assert_eq!(store.cached_series(), 1);
        cleanup(&settings);
    }

    #[test]
    fn a_backup_from_a_newer_omacharts_is_refused_and_nothing_changes() {
        let settings = scratch("newer");
        let store = Store::open_at(&settings).unwrap();
        store.set_setting("theme", "then");
        let (then, _) = take(&store).unwrap();
        let then = backdate(&then, 60);
        Connection::open(&then.path)
            .unwrap()
            .pragma_update(None, "user_version", migrations::LATEST + 1)
            .unwrap();
        store.set_setting("theme", "now");

        match restore(&store, &then.name) {
            Err(Error::FromTheFuture { found, .. }) => assert_eq!(found, migrations::LATEST + 1),
            other => panic!("expected a refusal, got {other:?}"),
        }
        assert_eq!(store.setting("theme").as_deref(), Some("now"));
        assert_eq!(list(&settings).len(), 1, "a refusal took a backup anyway");
        cleanup(&settings);
    }

    /// An older backup comes back in this build's shape, and its rows are
    /// carried across by column name rather than position.
    #[test]
    fn a_backup_from_an_older_schema_is_brought_up_to_date() {
        let settings = scratch("older");
        let store = Store::open_at(&settings).unwrap();
        let folder = dir(&settings);
        std::fs::create_dir_all(&folder).unwrap();
        let old = folder.join("settings-20260101-000000.db");
        {
            let conn = Connection::open(&old).unwrap();
            conn.execute("ATTACH ':memory:' AS market", []).unwrap();
            // The first schema alone, as a build from before drawings left it.
            match &migrations::MIGRATIONS[0].step {
                migrations::Step::Sql(sql) => conn.execute_batch(sql).unwrap(),
                migrations::Step::Rust(_) => unreachable!(),
            }
            conn.pragma_update(None, "user_version", 1).unwrap();
            conn.execute("INSERT INTO settings VALUES ('theme', 'ancient')", []).unwrap();
        }
        store.set_setting("theme", "now");

        restore(&store, "settings-20260101-000000").unwrap();
        assert_eq!(store.setting("theme").as_deref(), Some("ancient"));
        assert!(store.drawings("AAPL", None).is_empty(), "the drawings table is still there");
        cleanup(&settings);
    }

    /// What the app writes as it runs is not a change somebody made, so it
    /// does not make a backup; a choice does.
    #[test]
    fn bookkeeping_is_not_a_reason_for_a_backup() {
        let settings = scratch("bookkeeping");
        let store = Store::open_at(&settings).unwrap();
        let (first, _) = take(&store).unwrap();
        let _ = backdate(&first, 3 * 3600);

        store.set_setting("cache_swept_at", "1792500000");
        store.set_setting("cooldown.yahoo", "1792500000 120");
        store.set_setting("last_symbol", "NVDA");
        let (_, fresh) = take(&store).unwrap();
        assert!(!fresh, "bookkeeping made a backup");

        store.set_setting("theme", "paper");
        let (_, fresh) = take(&store).unwrap();
        assert!(fresh, "a choice did not");
        cleanup(&settings);
    }

    /// Settings from another machine bring its choices and leave this one's
    /// own: where screenshots go, how big the cache may grow, how wide the
    /// sidebar is on this screen.
    #[test]
    fn a_file_from_elsewhere_leaves_this_machines_own_settings() {
        let book = |width: i64| {
            format!(r#"{{"books":[{{"name":"A","sidebar_width":{width}}},{{"name":"B","sidebar_width":{width}}}],"active":0}}"#)
        };
        let elsewhere = scratch("their-machine");
        let theirs = Store::open_at(&elsewhere).unwrap();
        theirs.set_setting("theme", "paper");
        theirs.set_setting("screenshot_folder", "/their/pictures");
        theirs.set_setting("workspace", &book(500));
        drop(theirs);

        let settings = scratch("my-machine");
        let store = Store::open_at(&settings).unwrap();
        store.set_setting("theme", "midnight");
        store.set_setting("screenshot_folder", "/my/pictures");
        store.set_setting("cache_limit", "2000000000");
        store.set_setting("workspace", &book(280));
        restore_file(&store, &elsewhere).unwrap();

        assert_eq!(store.setting("theme").as_deref(), Some("paper"));
        assert_eq!(store.setting("screenshot_folder").as_deref(), Some("/my/pictures"));
        assert_eq!(store.setting("cache_limit").as_deref(), Some("2000000000"));
        let workspace: serde_json::Value =
            serde_json::from_str(&store.setting("workspace").unwrap()).unwrap();
        // Forgotten, so each book opens at this machine's own width.
        assert!(workspace["books"][1]["sidebar_width"].is_null());
        cleanup(&elsewhere);
        cleanup(&settings);
    }

    /// Two backups in one second are two states, and both are kept.
    #[test]
    fn two_backups_in_one_second_do_not_overwrite_each_other() {
        let settings = scratch("same-second");
        let store = Store::open_at(&settings).unwrap();
        store.set_setting("theme", "paper");
        let (first, _) = take(&store).unwrap();
        store.set_setting("theme", "midnight");
        let (second, fresh) = take(&store).unwrap();
        assert!(fresh);
        assert_ne!(first.name, second.name);
        assert!(first.path.exists() && second.path.exists());
        cleanup(&settings);
    }

    /// The backup being restored is still there afterwards, so restoring the
    /// safety copy and then this one again is always possible.
    #[test]
    fn restoring_keeps_the_backup_it_restored() {
        let settings = scratch("keeps-source");
        let store = Store::open_at(&settings).unwrap();
        store.set_setting("theme", "paper");
        let (then, _) = take(&store).unwrap();
        store.set_setting("theme", "midnight");
        restore(&store, &then.name).unwrap();
        assert!(then.path.exists(), "the restored backup was pruned");
        cleanup(&settings);
    }

    /// A backup brought from somewhere else restores like one from here.
    #[test]
    fn a_file_from_elsewhere_restores_and_anything_else_is_refused() {
        let elsewhere = scratch("elsewhere");
        let theirs = Store::open_at(&elsewhere).unwrap();
        theirs.set_setting("theme", "paper");
        drop(theirs);

        let settings = scratch("restore-file");
        let store = Store::open_at(&settings).unwrap();
        store.set_setting("theme", "midnight");
        restore_file(&store, &elsewhere).unwrap();
        assert_eq!(store.setting("theme").as_deref(), Some("paper"));

        // Not settings: a market file, an empty database, not a database.
        let market = crate::store::market_path(&elsewhere);
        let empty = settings.with_file_name("empty.db");
        Connection::open(&empty).unwrap().execute_batch("CREATE TABLE t (x);").unwrap();
        let text = settings.with_file_name("notes.txt");
        std::fs::write(&text, "hello").unwrap();
        for path in [&market, &empty, &text] {
            assert!(
                matches!(restore_file(&store, path), Err(Error::NotSettings(_))),
                "{} was taken for settings",
                path.display()
            );
        }
        assert_eq!(store.setting("theme").as_deref(), Some("paper"), "a refusal changed things");
        cleanup(&elsewhere);
        cleanup(&settings);
    }

    #[test]
    fn an_unknown_name_is_said_so() {
        let settings = scratch("unknown");
        let store = Store::open_at(&settings).unwrap();
        assert!(matches!(restore(&store, "settings-nope"), Err(Error::NotFound(_))));
        assert!(matches!(restore(&Store::memory().unwrap(), "x"), Err(Error::NoFile)));
        cleanup(&settings);
    }
}
