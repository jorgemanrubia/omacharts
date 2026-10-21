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
use crate::store::Store;

/// How many of the newest are always kept, whatever day they are from.
pub const KEEP_NEWEST: usize = 10;

/// How many days back one a day is kept, today included. The newest ten
/// alone can all be from one busy afternoon, which is no help to somebody
/// who noticed on Thursday what went wrong on Monday.
pub const KEEP_DAYS: i64 = 14;

/// How often the window takes one, if anything changed.
///
/// An hour, because the newest ten are then most of a working day and the
/// one-a-day rule covers the rest. A timer, which this app otherwise does not
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
/// The second half of the answer is whether it is new. A copy identical to
/// the newest one is not kept: it would push a real one out of the ten
/// without adding anything, and `VACUUM INTO` writes the same bytes for the
/// same contents, so comparing them is exact.
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
        && same_contents(&partial, &newest.path)
    {
        std::fs::remove_file(&partial)?;
        return Ok((newest, false));
    }
    // Two in one second — a command run twice — are the same moment, and the
    // later copy replaces the earlier one.
    let path = folder.join(&name);
    std::fs::rename(&partial, &path)?;
    let backup = describe(&path).ok_or_else(|| std::io::Error::other("the backup vanished"))?;
    Ok((backup, true))
}

fn same_contents(a: &Path, b: &Path) -> bool {
    match (std::fs::metadata(a), std::fs::metadata(b)) {
        (Ok(x), Ok(y)) if x.len() == y.len() => {
            matches!((std::fs::read(a), std::fs::read(b)), (Ok(x), Ok(y)) if x == y)
        }
        _ => false,
    }
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

/// Which of `taken`, newest first, go: everything but the newest
/// [`KEEP_NEWEST`] and the newest of each of the last [`KEEP_DAYS`] calendar
/// days, in the time zone they are given in.
///
/// Calendar days in somebody's own zone rather than UTC's, because "the
/// backup from Tuesday" means their Tuesday.
fn doomed<Tz: TimeZone>(taken: &[DateTime<Tz>], now: &DateTime<Tz>) -> Vec<usize> {
    let today = now.date_naive();
    let oldest_day = today - chrono::Duration::days(KEEP_DAYS - 1);
    let mut days_kept: Vec<NaiveDate> = Vec::new();
    let mut doomed = Vec::new();
    for (at, when) in taken.iter().enumerate() {
        let day = when.date_naive();
        let a_day_kept = day >= oldest_day && day <= today && !days_kept.contains(&day);
        if a_day_kept {
            days_kept.push(day);
        }
        if at >= KEEP_NEWEST && !a_day_kept {
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
    let settings = store.path().ok_or(Error::NoFile)?.to_path_buf();
    let from = find(&settings, name)?;
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
        let (kept, _) = take(store)?;
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

    #[test]
    fn ten_or_fewer_are_all_kept() {
        let taken = newest_first((0..10).map(|h| at(21, h)).collect());
        assert!(doomed(&taken, &at(21, 12)).is_empty());
    }

    /// A busy afternoon makes more than ten in a day; past the newest ten,
    /// only one a day survives, and that one is the day's newest.
    #[test]
    fn past_the_newest_ten_one_a_day_is_kept() {
        let mut taken: Vec<_> = (0..12).map(|h| at(21, h + 10)).collect();
        taken.extend((0..3).map(|h| at(20, h + 10)));
        taken.extend((0..3).map(|h| at(19, h + 10)));
        let taken = newest_first(taken);
        let gone: Vec<_> = doomed(&taken, &at(21, 23)).into_iter().map(|i| taken[i]).collect();

        // The 21st has its newest among the ten, so its two oldest go. The
        // 20th and 19th keep their newest only.
        assert_eq!(
            gone,
            vec![at(21, 11), at(21, 10), at(20, 11), at(20, 10), at(19, 11), at(19, 10)]
        );
    }

    #[test]
    fn a_day_older_than_two_weeks_keeps_nothing_past_the_newest_ten() {
        let mut taken: Vec<_> = (0..10).map(|h| at(21, h)).collect();
        taken.push(at(8, 12)); // 13 days back: the last day still covered
        taken.push(at(7, 12)); // 14 days back: not
        let taken = newest_first(taken);
        let gone: Vec<_> = doomed(&taken, &at(21, 23)).into_iter().map(|i| taken[i]).collect();
        assert_eq!(gone, vec![at(7, 12)]);
    }

    /// Days are the calendar's in the zone given, not UTC's: 23:30 in Madrid
    /// on the 20th is the 20th there, though it is the 21st nowhere else
    /// that matters to the person asking.
    #[test]
    fn days_are_counted_in_the_zone_given() {
        let zone = chrono::FixedOffset::east_opt(-5 * 3600).unwrap();
        let mut taken: Vec<_> = (0..10).map(|h| at(21, h + 12).with_timezone(&zone)).collect();
        // 02:00 UTC on the 21st is 21:00 on the 20th at UTC-5, so these two
        // are the same local day, and only the newer is kept.
        taken.push(at(21, 2).with_timezone(&zone));
        taken.push(at(21, 1).with_timezone(&zone));
        taken.sort_by(|a, b| b.cmp(a));
        let now = at(21, 23).with_timezone(&zone);
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
        let first = backdate(&first, 5);

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

    #[test]
    fn an_unknown_name_is_said_so() {
        let settings = scratch("unknown");
        let store = Store::open_at(&settings).unwrap();
        assert!(matches!(restore(&store, "settings-nope"), Err(Error::NotFound(_))));
        assert!(matches!(restore(&Store::memory().unwrap(), "x"), Err(Error::NoFile)));
        cleanup(&settings);
    }
}
