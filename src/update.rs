//! Updates the way a browser does them: downloaded behind the window, used
//! from the next launch, and never needing root.
//!
//! pacman owns `/usr/bin/omacharts` and `/usr/lib/omacharts/`, and this
//! never touches them. A newer release is downloaded into
//! `$XDG_DATA_HOME/omacharts/versions/<version>/` instead, and the command on
//! PATH — a launcher that links nothing, see [`crate::launch`] — execs it in
//! place of the installed app. Omarchy's repository runs a release or two
//! behind GitHub's, so that hand-over is the steady state rather than the
//! exception.
//!
//! ```text
//! versions/
//!   0.2.15/omacharts           complete and verified; renamed into place whole
//!   0.2.15/.trying-4242        a launch that has not yet shown it healthy
//!   0.2.15/.healthy            it has run properly here: never marked bad
//!   0.2.16/.bad                never run or downloaded again, and why
//!   .incoming-0.2.16-4300/     a download in progress, or one that died
//!   .checked                   its modification time is the last check
//!   .before-0.2.16             the backup taken before 0.2.16 first opened the settings
//! ```
//!
//! Five rules hold it together:
//!
//! - **Only a verified, complete version is ever a directory with a version
//!   for a name.** A download is written under `.incoming-…`, checked against
//!   the release's published sha256, decompressed, asked its `--version`, and
//!   only then renamed into place. A rename within one directory is atomic, so
//!   a launch sees the whole version or none of it — and a disk that fills up
//!   halfway leaves an `.incoming-…` the next tidy removes.
//! - **Hand over only to a version newer than the installed binary.** Never
//!   to an equal or older one, whatever is on disk.
//! - **pacman wins.** Once the installed binary is as new as a download, the
//!   download is deleted: at every check, after every download, and before
//!   one is ever started. A download that finishes after pacman has overtaken
//!   it is thrown away rather than offered.
//! - **One hand-over per launch.** The process handed to is told so in its
//!   environment and never hands over again, so two downloads can never
//!   bounce a launch between each other.
//! - **A version that does not run is given up on, and one that has run
//!   never is.** The launcher leaves a marker named after its process id
//!   before the `exec`, and the version takes it away as soon as `main` is
//!   reached, putting it back only when it is about to be the window. It
//!   clears its markers once that window has been up for a few seconds, and
//!   says so in `.healthy`; nothing else — a command, a `--version`, a launch
//!   handed to a window already open — counts either way. Three window
//!   launches in a row that ended first mark
//!   it bad, as does a definite reason it cannot run here: a checksum that
//!   does not match, a binary that does not unpack, says another version or
//!   is not something this machine can execute. Anything that may pass — a
//!   timeout, memory, a file busy for a moment — is this launch's problem
//!   only, and the next one tries again. A version that has been healthy
//!   stays runnable whatever happens after, since it may be the only one that
//!   can open a database it migrated; and the settings are backed up before
//!   a version that has never run here first opens them, and put back from
//!   that backup if it is given up on with nothing newer left to open them.
//!
//! The launch path is the part with a budget, and it all runs in the
//! launcher: one directory read when there is nothing newer, which is
//! microseconds, and a marker file when there is. Either way it is one `exec`
//! into the binary that keeps the window. Nothing is hashed at launch; that was done once,
//! before the rename.

use std::fmt;
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{Duration, SystemTime};

pub use crate::launch::{alive, as_root, shelf_dir, updates_off, Shelf, Version, NO_UPDATE_ENV, SOURCE_ENV};
use crate::launch::{trying, Told, BAD, BINARY, HEALTHY, LAUNCHER_ENV, PROBE_ENV};

/// Where releases are published.
pub const REPOSITORY: &str = "https://github.com/jorgemanrubia/omacharts";

/// Where the package puts the command: the launcher, or the app itself when
/// it was copied there by hand out of the release tarball.
pub const INSTALLED_AT: &str = "/usr/bin/omacharts";

/// Set on the process "Restart to update" starts: the id of the process it
/// replaces, which has to be gone, and its application id released, before
/// the new one can be the window.
const RESTARTING: &str = "OMACHARTS_RESTARTING";

/// The setting behind Preferences → Update automatically.
pub const SETTING: &str = "update_automatically";

/// How often a release is asked for, across launches as well as within one.
pub const CADENCE: Duration = Duration::from_secs(6 * 60 * 60);

/// The release asset this machine downloads: the binary, gzipped, under a
/// name with no version in it so the URL can be written from the tag alone.
pub fn asset() -> String {
    format!("omacharts-{}.gz", std::env::consts::ARCH)
}

// -- which binary this process is -----------------------------------------

/// What the process knows about the installed binary it came from. Absent
/// for anything that is not an installed binary — a build in `target/`, a
/// test — and absence is what keeps those from ever downloading or handing
/// over.
#[derive(Clone, Debug)]
pub struct Origin {
    /// The installed command: what "Restart to update" starts again, and
    /// what is asked its version.
    pub installed: PathBuf,
    /// Its version when this process was launched. pacman may replace it
    /// while this runs, which is what [`installed_version`] asks about.
    pub installed_version: Version,
    /// The downloaded version's folder, when this process is one, handed
    /// over to.
    pub folder: Option<PathBuf>,
}

impl Origin {
    /// Whether this process is a downloaded version, handed over to.
    pub fn handed_over(&self) -> bool {
        self.folder.is_some()
    }
}

static ORIGIN: OnceLock<Origin> = OnceLock::new();

/// See [`Origin`]. `None` means updates do not apply to this process.
pub fn origin() -> Option<&'static Origin> {
    ORIGIN.get()
}

/// The installed binary's version now, which pacman may have changed since
/// launch. Asked of the binary itself, off the main loop: a background check
/// is the only caller. The launcher answers a probe itself, in well under a
/// millisecond.
pub fn installed_version(origin: &Origin) -> Version {
    probe(&origin.installed, Duration::from_secs(10)).unwrap_or(origin.installed_version)
}

/// The very first thing the app's `main` does: learn what is installed from
/// the launcher that started it, or — when the app is itself the installed
/// command, copied there out of the release tarball — act as the launcher
/// would and hand over to a newer download.
///
/// Returns only if this process carries on as itself.
pub fn at_launch() {
    let exe = std::env::current_exe().ok();
    if let Some(from) = std::env::var_os(LAUNCHER_ENV) {
        // SAFETY: first thing in `main`, before any other thread exists.
        unsafe { std::env::remove_var(LAUNCHER_ENV) };
        if let Some(told) = Told::decode(&from.to_string_lossy()) {
            let folder = told.folder.filter(|folder| exe.as_deref().is_some_and(|exe| runs_from(exe, folder)));
            // The launcher's marker stays: until `main` knows this launch is
            // not a window — see [`not_a_window`] — a crash anywhere on the
            // way to one, GTK's start included, counts against the version.
            let _ = ORIGIN.set(Origin { installed: told.installed, installed_version: told.installed_version, folder });
        }
        return;
    }
    if std::env::var_os(PROBE_ENV).is_some() {
        return;
    }
    let Some(exe) = exe else { return };
    let source_given = std::env::var_os(SOURCE_ENV).is_some_and(|v| !v.is_empty());
    if (exe != Path::new(INSTALLED_AT) && !source_given) || under(&exe, &shelf_dir()) {
        return;
    }
    let running = Version::running();
    crate::launch::hand_over_if_newer(&exe, running);
    // Nothing newer, or it would not start: this binary is the one.
    unsafe { std::env::remove_var(LAUNCHER_ENV) };
    let _ = ORIGIN.set(Origin { installed: exe, installed_version: running, folder: None });
}

/// Is `exe` the binary in `folder`? Both as the file system resolves them:
/// the running binary's path is, and a data folder reached through a
/// symbolic link is not, until it is asked.
fn runs_from(exe: &Path, folder: &Path) -> bool {
    let resolved = |path: &Path| fs::canonicalize(path).ok();
    let exe = resolved(exe);
    exe.as_deref().and_then(Path::parent).is_some_and(|parent| Some(parent) == resolved(folder).as_deref())
}

/// Is `path` somewhere inside `dir`, resolved the same way?
fn under(path: &Path, dir: &Path) -> bool {
    let resolved = |path: &Path| fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    resolved(path).starts_with(resolved(dir))
}

/// This launch is not a window: a command, a `--version`, or arguments
/// handed to a window another process keeps. It proves nothing about the
/// version either way, so its marker goes now, and stopping it with Ctrl-C
/// or a timeout does not count as a launch that did not run.
pub fn not_a_window() {
    let Some(folder) = origin().and_then(|origin| origin.folder.as_deref()) else { return };
    let _ = fs::remove_file(folder.join(trying(std::process::id())));
}

/// This process is about to become the window: from here until the window
/// has been up for a few seconds, a launch that ends counts as one that did
/// not run. The launcher's marker says so already; this is for a process
/// started some other way, such as in place of an older version that found
/// its database too new.
pub fn window_starting() {
    let Some(folder) = origin().and_then(|origin| origin.folder.as_deref()) else { return };
    if !folder.join(HEALTHY).exists() {
        let _ = fs::write(folder.join(trying(std::process::id())), b"");
    }
}

/// This version has shown it runs: its window has been up for a few
/// seconds. Nothing else counts — not a `--version`, not a command, not a
/// launch handed to a window an older version keeps — because a version
/// called healthy is never given up on, and the backup and the older
/// download that would let it be are let go. Its folder says so for good,
/// and every launch still marked as trying it is cleared.
pub fn healthy() {
    if UNOPENED.load(std::sync::atomic::Ordering::Relaxed) != 0 {
        return;
    }
    if let Some(folder) = origin().and_then(|origin| origin.folder.as_deref()) {
        mark_healthy(folder);
    }
}

/// The window carries on with nothing saved: the settings could not be
/// opened, or not copied before this version first opened them. Up for
/// hours, that is still not a version shown to run here, so it is never
/// called healthy, and the copy that lets the settings be put back is kept.
///
/// `error` is why they could not be opened, when they were tried. Only a
/// failure this version's own code made — a migration step that went wrong
/// — also counts the launch as one that did not run, however it ends: three,
/// and it is given up on and the settings put back. A full disk, a file
/// another process holds, permissions or a damaged file are not the
/// version's doing; a good release given up on over one is never
/// downloaded again, so those clear the launch as an ordinary close does.
pub fn settings_unopened(error: Option<&crate::migrations::Error>) {
    let state = if error.is_some_and(its_own_doing) { COUNTED } else { NOT_HEALTHY };
    UNOPENED.store(state, std::sync::atomic::Ordering::Relaxed);
}

const NOT_HEALTHY: u8 = 1;
const COUNTED: u8 = 2;
static UNOPENED: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

/// Whether opening the settings failed in a way only this version's code
/// explains: SQL its migrations got wrong, a step of theirs that failed in
/// Rust. Anything the machine or another process could cause is not, and
/// when in doubt it is not.
fn its_own_doing(error: &crate::migrations::Error) -> bool {
    use rusqlite::ErrorCode::*;
    match error {
        crate::migrations::Error::FromTheFuture { .. } => false,
        crate::migrations::Error::Sqlite(
            rusqlite::Error::SqliteFailure(failure, _) | rusqlite::Error::SqlInputError { error: failure, .. },
        ) => matches!(failure.code, Unknown | ConstraintViolation | TypeMismatch | ApiMisuse | ParameterOutOfRange),
        crate::migrations::Error::Sqlite(
            rusqlite::Error::FromSqlConversionFailure(..)
            | rusqlite::Error::InvalidColumnType(..)
            | rusqlite::Error::InvalidColumnIndex(_)
            | rusqlite::Error::InvalidColumnName(_)
            | rusqlite::Error::QueryReturnedNoRows
            | rusqlite::Error::IntegralValueOutOfRange(..)
            | rusqlite::Error::InvalidParameterCount(..)
            | rusqlite::Error::MultipleStatement
            | rusqlite::Error::ExecuteReturnedResults,
        ) => true,
        crate::migrations::Error::Sqlite(_) => false,
    }
}

fn mark_healthy(folder: &Path) {
    if !folder.join(HEALTHY).exists() {
        let _ = fs::write(folder.join(HEALTHY), b"");
    }
    for entry in fs::read_dir(folder).into_iter().flatten().flatten() {
        if entry.file_name().to_str().is_some_and(|name| name.starts_with(".trying-")) {
            let _ = fs::remove_file(entry.path());
        }
    }
    // Healthy, so never rolled back, and neither is anything older: what
    // they wrote is somebody's work, and this version reads it.
    if let Some(version) = folder.file_name().and_then(|name| name.to_str()).and_then(Version::parse)
        && let Some(root) = folder.parent()
    {
        for entry in fs::read_dir(root).into_iter().flatten().flatten() {
            if let Some(name) = entry.file_name().to_str()
                && about(name, &[BEFORE, SAVED]).is_some_and(|before| before <= version)
            {
                let _ = fs::remove_file(entry.path());
            }
        }
    }
}

/// The last thing `main` does on its way out: this launch's marker goes. A
/// window closed before it was healthy ended on its own terms; a version
/// that crashes never gets here, and its marker stays to count. Never marks
/// the version healthy.
pub fn settle() {
    if UNOPENED.load(std::sync::atomic::Ordering::Relaxed) == COUNTED {
        return;
    }
    let Some(folder) = origin().and_then(|origin| origin.folder.as_deref()) else { return };
    let _ = fs::remove_file(folder.join(trying(std::process::id())));
}

/// Where the shelf remembers what the settings were before `folder`'s
/// version first opened them: `.before-<version>` beside the version
/// folders rather than in one, so tidying a version given up on does not
/// lose it. Two lines: the settings' schema version then, and the copy's
/// path — `none` and nothing when there were no settings.
fn before_record(folder: &Path) -> Option<PathBuf> {
    let version = folder.file_name()?.to_str()?;
    Some(folder.parent()?.join(format!("{BEFORE}{version}")))
}

const BEFORE: &str = ".before-";
/// The copy of the settings a `.before-` record points at: `.saved-<version>`,
/// on the shelf and not among the backups, whose pruning keeps one an hour
/// and would delete it the moment the window takes the next.
const SAVED: &str = ".saved-";

/// Forget `version`'s record and the copy it points at.
fn forget(root: &Path, version: Version) {
    let _ = fs::remove_file(root.join(format!("{BEFORE}{version}")));
    let _ = fs::remove_file(root.join(format!("{SAVED}{version}")));
}

/// The version a shelf entry named `<prefix><version>` is about, for the
/// prefixes of what the shelf keeps beside the version folders.
fn about(name: &str, prefixes: &[&str]) -> Option<Version> {
    prefixes.iter().find_map(|prefix| Version::parse(name.strip_prefix(prefix)?))
}
/// A shelf-wide lock, for the one change that must never run twice at once.
const LOCK: &str = ".lock";
/// What a rollback a command did has to say, for the next window to show.
const NOTICE: &str = ".notice";
/// A release whose download did not match its checksum once already.
const MISMATCHED: &str = ".mismatched-";

/// One `.before-` record, read.
#[derive(Debug)]
struct Before {
    version: Version,
    /// `None` when there were no settings yet.
    schema: Option<i32>,
    backup: Option<PathBuf>,
}

fn records(root: &Path) -> Vec<Before> {
    let mut found: Vec<Before> = fs::read_dir(root)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let version = Version::parse(entry.file_name().to_str()?.strip_prefix(BEFORE)?)?;
            let text = fs::read_to_string(entry.path()).ok()?;
            let mut lines = text.lines();
            let schema = match lines.next()?.trim() {
                "none" => None,
                number => Some(number.parse().ok()?),
            };
            let backup = lines.next().map(str::trim).filter(|path| !path.is_empty()).map(PathBuf::from);
            if schema.is_some() && backup.is_none() {
                return None;
            }
            Some(Before { version, schema, backup })
        })
        .collect();
    found.sort_unstable_by_key(|before| std::cmp::Reverse(before.version));
    found
}

/// The settings' schema version as SQLite has it, journal included; `None`
/// when there is no settings file or it cannot be read.
fn schema_of(settings: &Path) -> Option<i32> {
    if !settings.exists() {
        return None;
    }
    let conn = rusqlite::Connection::open_with_flags(settings, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).ok()?;
    crate::migrations::version(&conn).ok()
}

/// Hold the shelf's lock until the result is dropped.
fn lock(root: &Path) -> Option<fs::File> {
    fs::create_dir_all(root).ok()?;
    let file = fs::OpenOptions::new().create(true).truncate(false).write(true).open(root.join(LOCK)).ok()?;
    file.lock().ok()?;
    Some(file)
}

/// A name no other process, nor this one a moment ago, has used.
fn unique() -> String {
    let now = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap_or_default();
    format!("{}-{}-{}", now.as_secs(), std::process::id(), now.subsec_nanos())
}

/// Before the database is opened by a downloaded version that has never
/// run here: back the settings up, so that if it migrates them and then
/// turns out not to run, the copy from before it is there to go back to.
/// Two `stat`s for any version that has.
///
/// `Err` says why the copy could not be taken — a full disk, most likely —
/// and then the settings must not be opened by this version, whose
/// migration would change them with nothing to go back to: half migrated
/// and then given up on, they would open in no version at all. Settings
/// already at this version's schema have nothing to migrate, and are fine.
pub fn before_first_open(settings: &Path) -> Result<(), String> {
    let Some(folder) = origin().and_then(|origin| origin.folder.as_deref()) else { return Ok(()) };
    backup_before_first_open(folder, settings).or_else(|why| {
        if schema_of(settings).is_some_and(|schema| schema >= crate::migrations::LATEST) { Ok(()) } else { Err(why) }
    })
}

fn backup_before_first_open(folder: &Path, settings: &Path) -> Result<(), String> {
    let Some(record) = before_record(folder) else { return Ok(()) };
    if folder.join(HEALTHY).exists() || record.exists() {
        return Ok(());
    }
    let Some(root) = folder.parent() else { return Ok(()) };
    let Some(version) = folder.file_name().and_then(|name| name.to_str()).and_then(Version::parse) else {
        return Ok(());
    };
    // Under the shelf's lock, and asked again under it: a command and the
    // window starting this version together would otherwise both copy, and
    // the second copy could be of what the first one's migration wrote.
    // Whoever comes second finds the record, so no process of this version
    // migrates until the copy from before any of them did is written.
    let _held = lock(root).ok_or("the downloads folder cannot be locked")?;
    if record.exists() {
        return Ok(());
    }
    let text = if settings.exists() {
        let saved = root.join(format!("{SAVED}{version}"));
        let schema = save(settings, &saved)?;
        format!("{schema}\n{}\n", saved.display())
    } else {
        "none\n\n".to_string()
    };
    // Written whole under another name and renamed, so a record is never
    // read half written.
    let incoming = root.join(format!("{BEFORE}{version}.{}", unique()));
    if let Err(error) = fs::write(&incoming, text).and_then(|()| fs::rename(&incoming, &record)) {
        let _ = fs::remove_file(&incoming);
        return Err(error.to_string());
    }
    Ok(())
}

/// A consistent copy of the settings at `to`, synced, and the schema it is
/// at. The settings' own connection is not this one, so a window writing
/// them meanwhile is fine: SQLite copies a single moment.
fn save(settings: &Path, to: &Path) -> Result<i32, String> {
    let partial = PathBuf::from(format!("{}.{}", to.display(), unique()));
    let copied = (|| {
        let conn = rusqlite::Connection::open_with_flags(settings, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(|error| error.to_string())?;
        conn.execute("VACUUM main INTO ?1", [partial.to_string_lossy()]).map_err(|error| error.to_string())?;
        drop(conn);
        fs::File::open(&partial).and_then(|file| file.sync_all()).map_err(|error| error.to_string())?;
        let schema = schema_of(&partial).ok_or("the copy cannot be read")?;
        fs::rename(&partial, to).map_err(|error| error.to_string())?;
        Ok(schema)
    })();
    if copied.is_err() {
        let _ = fs::remove_file(&partial);
    }
    copied
}

/// The settings are open, in a downloaded version that has not shown it
/// runs yet. If they are at the schema they were at before it first opened
/// them, it migrated nothing, and there is nothing of its to roll back: the
/// copy goes, and the record says so, so a rollback for a later version is
/// never sent back to before it — and the settings are not copied again at
/// every launch of a version only ever run from the bar. A small read once
/// it says so.
pub fn opened(settings: &Path) {
    let Some(folder) = origin().and_then(|origin| origin.folder.as_deref()) else { return };
    forget_if_unmigrated(folder, settings);
}

fn forget_if_unmigrated(folder: &Path, settings: &Path) {
    let Some(record) = before_record(folder) else { return };
    let Ok(text) = fs::read_to_string(&record) else { return };
    let Some(schema) = text.lines().next().and_then(|line| line.trim().parse::<i32>().ok()) else { return };
    if Some(schema) != schema_of(settings) {
        return;
    }
    let Some(root) = folder.parent() else { return };
    let Some(version) = folder.file_name().and_then(|name| name.to_str()).and_then(Version::parse) else { return };
    let _ = fs::remove_file(root.join(format!("{SAVED}{version}")));
    let _ = fs::write(&record, UNMIGRATED);
}

/// What a record says once its version has opened the settings without
/// migrating them: nothing to put back, and nothing to copy again.
const UNMIGRATED: &str = "unmigrated\n";

/// The database was written by a newer Omacharts that never ran properly
/// here and has been given up on, and nothing newer is left to open it:
/// put the settings back as they were before that version first opened
/// them, so this one can. The newer file is kept beside them, never
/// deleted. Once per version. Says what was done, and leaves it for the
/// next window to say too; `None` when there was nothing to do.
///
/// Only for an installed omacharts: a build in `target/` is somebody
/// working on it, and the settings are not its to roll back.
pub fn roll_back_for_a_newer_database(settings: &Path) -> Option<String> {
    if origin().is_none() || updates_off() || as_root() || newer_for_a_newer_database().is_some() {
        return None;
    }
    let shelf = Shelf::new(shelf_dir());
    let said = roll_back(&shelf, settings, Version::running(), crate::migrations::LATEST)?;
    let _ = fs::write(shelf.root.join(NOTICE), &said);
    Some(said)
}

/// What a rollback left to be said, said once: the window shows it, since a
/// command's terminal may be nobody's — the bar's, say.
pub fn take_notice() -> Option<String> {
    let notice = shelf_dir().join(NOTICE);
    let said = fs::read_to_string(&notice).ok()?;
    let _ = fs::remove_file(&notice);
    Some(said).filter(|said| !said.trim().is_empty())
}

fn roll_back(shelf: &Shelf, settings: &Path, running: Version, known: i32) -> Option<String> {
    // One at a time: the bar's command and the window can both find the
    // database too new at the same moment.
    let _held = lock(&shelf.root)?;
    // And perhaps the other did it first.
    if schema_of(settings).is_none_or(|schema| schema <= known) {
        return None;
    }
    // The newest version given up on whose backup this one can open: the
    // least that is lost. One that ran is not given up on, and one that can
    // still run is started instead of rolling anything back.
    let chosen = records(&shelf.root).into_iter().find(|before| {
        let folder = shelf.root.join(before.version.to_string());
        let runnable = folder.join(BINARY).is_file() && !folder.join(BAD).exists();
        before.version > running
            && !runnable
            && !folder.join(HEALTHY).exists()
            && before.schema.is_none_or(|schema| schema <= known)
            && before.backup.as_deref().is_none_or(Path::exists)
    })?;
    let version = chosen.version;

    let unique = unique();
    let aside = settings.with_file_name(format!("omacharts.db.from-{version}-{unique}"));
    let with = |path: &Path, suffix: &str| PathBuf::from(format!("{}{suffix}", path.display()));
    if let Some(backup) = &chosen.backup {
        // Copied in under another name first, so the settings file is never
        // half written.
        let incoming = with(settings, &format!(".restoring-{unique}"));
        if let Err(error) = fs::copy(backup, &incoming).and_then(|_| fs::File::open(&incoming)?.sync_all()) {
            eprintln!("omacharts: could not put back the settings from before {version}: {error}");
            let _ = fs::remove_file(&incoming);
            return None;
        }
        if put_aside(settings, &aside, &with).is_none() {
            let _ = fs::remove_file(&incoming);
            return None;
        }
        fs::rename(&incoming, settings).ok()?;
    } else {
        put_aside(settings, &aside, &with)?;
    }
    // Used, and every older one with it: the settings are now from before
    // them all.
    for before in records(&shelf.root).into_iter().filter(|before| before.version <= version) {
        forget(&shelf.root, before.version);
    }
    Some(format!(
        "Omacharts {version} did not run here and had updated your settings, so they are back as they \
         were before it first ran; what changed since is kept in {}",
        aside.display()
    ))
}

/// The settings file and its journal, renamed out of the way together.
fn put_aside(settings: &Path, aside: &Path, with: &dyn Fn(&Path, &str) -> PathBuf) -> Option<()> {
    if settings.exists()
        && let Err(error) = fs::rename(settings, aside)
    {
        eprintln!("omacharts: could not set the newer settings aside: {error}");
        return None;
    }
    for suffix in ["-wal", "-shm"] {
        let _ = fs::rename(with(settings, suffix), with(aside, suffix));
    }
    Some(())
}

/// The database was written by a newer Omacharts than this one. When this
/// is the installed version and a newer download is on the shelf, that is
/// the one to run — most likely it is the one that wrote it.
pub fn newer_for_a_newer_database() -> Option<(Told, PathBuf)> {
    let origin = origin()?;
    if origin.handed_over() || updates_off() || as_root() {
        return None;
    }
    let floor = Version::running().max(origin.installed_version);
    let chosen = Shelf::new(shelf_dir()).choose(floor, alive)?;
    let told = Told {
        installed_version: origin.installed_version,
        installed: origin.installed.clone(),
        folder: Some(chosen.folder),
    };
    Some((told, chosen.binary))
}

/// [`newer_for_a_newer_database`], for a command: become it. Returns only
/// when there is none, or it would not start.
pub fn become_newer_for_a_newer_database() {
    let Some((told, binary)) = newer_for_a_newer_database() else { return };
    let folder = told.folder.clone().unwrap_or_default();
    let marker = folder.join(trying(std::process::id()));
    let _ = fs::write(&marker, b"");
    told.tell();
    let error = crate::launch::exec(&binary);
    let _ = fs::remove_file(&marker);
    // SAFETY: nothing else reads the environment while a command opens its
    // database, and the exec that would have used it did not happen.
    unsafe { std::env::remove_var(LAUNCHER_ENV) };
    eprintln!("omacharts: {} could not be started: {error}", binary.display());
}

/// [`newer_for_a_newer_database`], for the window: start it, to come up once
/// this process is gone, with the arguments this one was given. Says whether
/// it did.
pub fn restart_as_newer_for_a_newer_database() -> bool {
    use std::os::unix::process::CommandExt;
    let Some((told, binary)) = newer_for_a_newer_database() else { return false };
    let started = std::process::Command::new(&binary)
        .args(std::env::args_os().skip(1))
        .env(LAUNCHER_ENV, told.encode())
        .env(RESTARTING, std::process::id().to_string())
        .stdin(std::process::Stdio::null())
        .process_group(0)
        .spawn();
    // No marker for it: it says for itself whether it is a window.
    match started {
        Ok(_) => true,
        Err(error) => {
            eprintln!("omacharts: {} could not be started: {error}", binary.display());
            false
        }
    }
}

/// What to say when the database is newer than this binary and nothing
/// newer is here to open it.
pub const NEWER_DATABASE_HELP: &str = "`omacharts update reset` and then `omacharts update download` \
     fetch the newest release again, which can open it, or the manual's Updates section says how to put \
     back the copy of the settings from before it";

/// The second thing `main` does: if this process was started by "Restart to
/// update", wait for the one it replaces to be gone and to have given up the
/// application id, so this one becomes the window rather than handing its
/// launch to the window that is closing.
///
/// `still_held` asks the session bus whether the id is owned. Bounded, so a
/// window that never closes cannot leave this waiting forever.
pub fn wait_for_restart(still_held: impl Fn() -> bool) {
    let Some(pid) = std::env::var_os(RESTARTING) else { return };
    // SAFETY: single-threaded, before GTK or any other thread starts.
    unsafe { std::env::remove_var(RESTARTING) };
    let Some(pid) = pid.to_str().and_then(|pid| pid.parse::<u32>().ok()) else { return };
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    while std::time::Instant::now() < deadline {
        if !alive(pid) && !still_held() {
            return;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}

/// Start the installed command again, to come up once this process is gone.
/// It hands over to the newest download as any launch does.
pub fn spawn_restart() -> io::Result<()> {
    use std::os::unix::process::CommandExt;
    let origin = origin().ok_or_else(|| io::Error::other("not an installed omacharts"))?;
    std::process::Command::new(&origin.installed)
        .env(RESTARTING, std::process::id().to_string())
        .stdin(std::process::Stdio::null())
        // Its own process group, so nothing aimed at this one reaches it.
        .process_group(0)
        .spawn()
        .map(|_| ())
}

// -- the shelf of downloaded versions -------------------------------------

/// One version directory, as found.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Found {
    version: Version,
    /// Has a binary and is not marked bad.
    ready: bool,
    /// Has run properly here.
    healthy: bool,
}

const CHECKED: &str = ".checked";
const INCOMING: &str = ".incoming-";

impl Shelf {
    fn found(&self) -> Vec<Found> {
        let Ok(entries) = fs::read_dir(&self.root) else { return Vec::new() };
        let mut found: Vec<Found> = entries
            .flatten()
            .filter_map(|entry| {
                let version = Version::parse(entry.file_name().to_str()?)?;
                let dir = entry.path();
                let ready = dir.join(BINARY).is_file() && !dir.join(BAD).exists();
                Some(Found { version, ready, healthy: ready && dir.join(HEALTHY).exists() })
            })
            .collect();
        found.sort_unstable_by_key(|found| std::cmp::Reverse(found.version));
        found
    }

    /// The newest version ready to be handed over to.
    pub fn newest_ready(&self) -> Option<Version> {
        self.found().into_iter().find(|found| found.ready).map(|found| found.version)
    }

    /// The newest version this has anything of, bad or not: one marked bad
    /// is not downloaded again.
    pub fn newest_known(&self) -> Option<Version> {
        self.found().first().map(|found| found.version)
    }

    /// Delete what is no longer needed, and say how many entries went:
    ///
    /// - every version as old as `installed` or older, ready or bad — pacman
    ///   has caught up with it;
    /// - every ready version older than the newest ready one, but for the
    ///   newest that has run here, until a newer one has: if the newest does
    ///   not run, that one does, and it can open what it last wrote;
    /// - every bad marker but the newest, and that one too unless it is newer
    ///   than the newest ready version, which is the only thing it guards;
    /// - every download in progress whose process is gone;
    /// - every record of a backup or a mismatch for a version as old as
    ///   `installed`: the package has it now, so nothing rolls back to before it.
    pub fn tidy(&self, installed: Version, alive: impl Fn(u32) -> bool) -> usize {
        let mut gone = 0;
        let newest_ready = self.newest_ready();
        let newest_healthy = self.found().into_iter().find(|found| found.healthy).map(|found| found.version);
        // A version that opened the settings and migrated them, and that no
        // version since has been healthy after: perhaps only ever run from
        // the bar, never a window, so never healthy itself, and perhaps the
        // only one that can open them now.
        let migrated: Vec<Version> = records(&self.root).into_iter().map(|before| before.version).collect();
        let mut kept_bad = false;
        for found in self.found() {
            let keep = found.version > installed
                && if found.ready {
                    Some(found.version) == newest_ready
                        || Some(found.version) == newest_healthy
                        || migrated.contains(&found.version)
                } else {
                    let guards = newest_ready.is_none_or(|ready| found.version > ready) && !kept_bad;
                    kept_bad |= guards;
                    guards
                };
            if !keep && fs::remove_dir_all(self.root.join(found.version.to_string())).is_ok() {
                gone += 1;
            }
        }
        for entry in fs::read_dir(&self.root).into_iter().flatten().flatten() {
            let name = entry.file_name();
            let Some(rest) = name.to_str().and_then(|name| name.strip_prefix(INCOMING)) else { continue };
            let pid = rest.rsplit_once('-').and_then(|(_, pid)| pid.parse::<u32>().ok());
            if pid.is_none_or(|pid| !alive(pid)) && fs::remove_dir_all(entry.path()).is_ok() {
                gone += 1;
            }
        }
        for entry in fs::read_dir(&self.root).into_iter().flatten().flatten() {
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            let Some(version) = about(name, &[BEFORE, SAVED, MISMATCHED]) else { continue };
            if version <= installed && fs::remove_file(entry.path()).is_ok() {
                gone += 1;
            }
        }
        gone
    }

    /// When a release was last asked for, by any process.
    pub fn last_checked(&self) -> Option<SystemTime> {
        fs::metadata(self.root.join(CHECKED)).and_then(|meta| meta.modified()).ok()
    }

    /// Is a check due, `CADENCE` after the last one?
    pub fn check_due(&self, now: SystemTime) -> bool {
        self.last_checked()
            .and_then(|at| now.duration_since(at).ok())
            .is_none_or(|since| since >= CADENCE)
    }

    /// Remove every download, every mark and the time of the last check, so
    /// the next check starts from nothing — and a version given up on can be
    /// downloaded again. All but a version newer than `installed` that has
    /// run here, unless `all`: the settings may have been migrated by it,
    /// and the installed version would not open them.
    pub fn reset(&self, installed: Version, all: bool) -> io::Result<usize> {
        let keep: Vec<Version> = if all {
            Vec::new()
        } else {
            self.found().into_iter().filter(|found| found.healthy && found.version > installed).map(|found| found.version).collect()
        };
        let entries = match fs::read_dir(&self.root) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(0),
            Err(error) => return Err(error),
        };
        let mut gone = 0;
        for entry in entries.flatten() {
            let name = entry.file_name();
            if name.to_str().and_then(Version::parse).is_some_and(|version| keep.contains(&version)) {
                continue;
            }
            // Which backup came before which version: what lets a database a
            // removed version migrated be put back, so it outlives a plain
            // reset. `--all` is starting over, and takes them too. The lock
            // and an unsaid notice stay either way.
            // The copies themselves stay even then, to be put back by hand,
            // until the package catches up with their version.
            if name.to_str().is_some_and(|name| {
                (!all && name.starts_with(BEFORE)) || name.starts_with(SAVED) || name == LOCK || name == NOTICE
            }) {
                continue;
            }
            let path = entry.path();
            let removed = if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                fs::remove_dir_all(&path)
            } else {
                fs::remove_file(&path)
            };
            removed?;
            gone += 1;
        }
        Ok(gone)
    }

    fn note_checked(&self) {
        if fs::create_dir_all(&self.root).is_ok() {
            let _ = fs::File::create(self.root.join(CHECKED));
        }
    }

    /// Download, verify and put `version` in place, or leave nothing behind.
    fn install(&self, source: &dyn Source, version: Version, asset: &str) -> Result<(), Failure> {
        let incoming = self.root.join(format!("{INCOMING}{version}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&incoming);
        fs::create_dir_all(&incoming).map_err(Failure::Disk)?;
        let installed = self.fill(source, version, asset, &incoming).and_then(|()| {
            let target = self.root.join(version.to_string());
            // Everything in the folder, and the folder's own entry, on disk
            // before the rename makes it a version: a crash just after
            // must not leave a version whose binary is empty.
            sync_dir(&incoming).map_err(Failure::Disk)?;
            sync_dir(&self.root).map_err(Failure::Disk)?;
            match fs::rename(&incoming, &target) {
                Ok(()) => {}
                // Another process put the same version in place first, and
                // a finished version is never taken apart: it may be running.
                Err(error) if matches!(error.raw_os_error(), Some(17) | Some(39)) => {
                    return if target.join(BINARY).is_file() { Ok(()) } else { Err(Failure::Disk(error)) };
                }
                Err(error) => return Err(Failure::Disk(error)),
            }
            sync_dir(&self.root).map_err(Failure::Disk)
        });
        let _ = fs::remove_dir_all(&incoming);
        installed
    }

    fn fill(&self, source: &dyn Source, version: Version, asset: &str, incoming: &Path) -> Result<(), Failure> {
        use sha2::{Digest, Sha256};

        let expected = {
            let mut text = String::new();
            source
                .open(version, &format!("{asset}.sha256"))?
                .take(4096)
                .read_to_string(&mut text)
                .map_err(|error| Failure::Network(error.to_string()))?;
            text.split_whitespace()
                .next()
                .filter(|sum| sum.len() == 64 && sum.bytes().all(|b| b.is_ascii_hexdigit()))
                .map(str::to_ascii_lowercase)
                .ok_or_else(|| Failure::Network(format!("{asset}.sha256 is not a checksum")))?
        };

        // The compressed file, hashed as it is written.
        let packed = incoming.join(format!("{BINARY}.gz"));
        let mut hasher = Sha256::new();
        {
            let mut reader = source.open(version, asset)?.take(MAX_DOWNLOAD);
            let mut file = fs::File::create(&packed).map_err(Failure::Disk)?;
            let mut buffer = vec![0u8; 64 * 1024];
            loop {
                let read = reader.read(&mut buffer).map_err(|error| Failure::Network(error.to_string()))?;
                if read == 0 {
                    break;
                }
                hasher.update(&buffer[..read]);
                file.write_all(&buffer[..read]).map_err(Failure::Disk)?;
            }
            file.sync_all().map_err(Failure::Disk)?;
        }
        let actual = hex(&hasher.finalize());
        if actual != expected {
            return Err(Failure::Mismatch { asset: asset.to_string() });
        }

        let binary = incoming.join(BINARY);
        {
            let packed = fs::File::open(&packed).map_err(Failure::Disk)?;
            let mut unpacked = flate2::read::GzDecoder::new(io::BufReader::new(packed)).take(MAX_BINARY);
            let mut file = fs::File::create(&binary).map_err(Failure::Disk)?;
            io::copy(&mut unpacked, &mut file).map_err(|error| match error.kind() {
                io::ErrorKind::InvalidData | io::ErrorKind::InvalidInput | io::ErrorKind::UnexpectedEof => {
                    Failure::Broken(format!("{asset} does not unpack: {error}"))
                }
                _ => Failure::Disk(error),
            })?;
            use std::os::unix::fs::PermissionsExt;
            file.set_permissions(fs::Permissions::from_mode(0o755)).map_err(Failure::Disk)?;
            file.sync_all().map_err(Failure::Disk)?;
        }
        let _ = fs::remove_file(&packed);

        // Verified, so it may be run: once, to prove it starts on this
        // machine and is the version it was published as.
        match probe(&binary, Duration::from_secs(20)) {
            Ok(said) if said == version => Ok(()),
            Ok(said) => Err(Failure::Broken(format!("{asset} says it is {said}, not {version}"))),
            Err(Unstarted::Definitely(why)) => Err(Failure::Broken(format!("{version} cannot run here: {why}"))),
            Err(Unstarted::ForNow(why)) => Err(Failure::NotNow(format!("{version} did not answer: {why}"))),
        }
    }
}

fn sync_dir(dir: &Path) -> io::Result<()> {
    fs::File::open(dir)?.sync_all()
}

/// A release asset is a binary of some tens of megabytes; anything far
/// larger is not one, and is not going to fill the disk finding out.
const MAX_DOWNLOAD: u64 = 256 * 1024 * 1024;
const MAX_BINARY: u64 = 512 * 1024 * 1024;

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Why a probe got no version.
#[derive(Debug, PartialEq, Eq)]
enum Unstarted {
    /// It is not something this machine can run, and will not be.
    Definitely(String),
    /// It did not answer this time: too slow, out of memory, an exit that
    /// may not happen again.
    ForNow(String),
}

/// Ask `binary` its version, without letting it hand over or reach the
/// window, and without waiting on it for longer than `patience`.
fn probe(binary: &Path, patience: Duration) -> Result<Version, Unstarted> {
    use crate::launch::{refused, Refused};
    use std::os::unix::process::ExitStatusExt;
    let spawn = || {
        std::process::Command::new(binary)
            .arg("--version")
            .env(PROBE_ENV, "1")
            .env_remove(LAUNCHER_ENV)
            .env_remove(SOURCE_ENV)
            .env("DBUS_SESSION_BUS_ADDRESS", "")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
    };
    // A file just written can be busy for a moment, while a fork elsewhere in
    // the process still holds the descriptor it was written through.
    let mut attempts = 0;
    let mut child = loop {
        attempts += 1;
        match spawn() {
            Ok(started) => break started,
            Err(error) if error.raw_os_error() == Some(26) && attempts < 10 => {
                std::thread::sleep(Duration::from_millis(50))
            }
            // A file just made executable and still refused is a folder
            // mounted noexec, which no later download changes.
            Err(error) if error.raw_os_error() == Some(13) => {
                return Err(Unstarted::Definitely(format!("not allowed to run in this folder ({error})")));
            }
            Err(error) => {
                return Err(match refused(&error, binary) {
                    Refused::Definitely(why) => Unstarted::Definitely(why),
                    Refused::Vanished | Refused::ForNow(_) => Unstarted::ForNow(error.to_string()),
                });
            }
        }
    };
    let deadline = std::time::Instant::now() + patience;
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => break,
            // The shell's and the dynamic loader's codes for a binary that
            // cannot be run: not executable, or a library it needs is
            // missing or too old. Every launch would end the same way.
            Ok(Some(status)) if matches!(status.code(), Some(126 | 127)) => {
                return Err(Unstarted::Definitely(format!("--version ended with {status}")));
            }
            // A Rust panic: answering `--version` touches nothing outside
            // the binary, so it would panic every time.
            Ok(Some(status)) if status.code() == Some(101) => {
                return Err(Unstarted::Definitely(format!("--version panicked ({status})")));
            }
            // A crash answering `--version` is the binary, every time. A
            // kill or a hang-up is something else's doing.
            Ok(Some(status)) if matches!(status.signal(), Some(4 | 5 | 6 | 7 | 8 | 11 | 31)) => {
                return Err(Unstarted::Definitely(format!("--version ended with {status}")));
            }
            Ok(Some(status)) => return Err(Unstarted::ForNow(format!("--version ended with {status}"))),
            Ok(None) if std::time::Instant::now() < deadline => std::thread::sleep(Duration::from_millis(10)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(Unstarted::ForNow(format!("no answer in {} seconds", patience.as_secs())));
            }
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(Unstarted::ForNow(error.to_string()));
            }
        }
    }
    let mut said = String::new();
    if let Some(mut out) = child.stdout.take() {
        let _ = out.read_to_string(&mut said);
    }
    said.trim()
        .strip_prefix("omacharts")
        .and_then(Version::parse)
        .ok_or_else(|| Unstarted::Definitely(format!("its --version said {:?}", said.trim())))
}

// -- where releases come from ---------------------------------------------

/// Something that publishes releases: GitHub, or a test's fake.
pub trait Source {
    /// The newest release's version.
    fn latest(&self) -> Result<Version, Failure>;
    /// One of `version`'s assets, to read.
    fn open(&self, version: Version, asset: &str) -> Result<Box<dyn Read>, Failure>;
    /// Whether `version` has `asset` yet. A release is announced a while
    /// before every architecture's binary is on it.
    fn has(&self, version: Version, asset: &str) -> Result<bool, Failure>;
}

/// Why a check or a download did not finish.
#[derive(Debug)]
pub enum Failure {
    /// Not reachable, or not answering as expected. Usually just offline.
    Network(String),
    /// The download is not what was published.
    Mismatch { asset: String },
    /// Verified, and cannot run here: does not unpack, says another version,
    /// or is not something this machine executes.
    Broken(String),
    /// Verified, and did not answer this time, for a reason that may pass.
    NotNow(String),
    /// Out of space, or not writable.
    Disk(io::Error),
}

impl Failure {
    /// Will downloading this version again end the same way? Then it is
    /// marked bad, and not downloaded again.
    pub fn definite(&self) -> bool {
        matches!(self, Failure::Mismatch { .. } | Failure::Broken(_))
    }
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Failure::Network(why) => write!(f, "releases not reachable: {why}"),
            Failure::Mismatch { asset } => write!(f, "{asset} does not match its published checksum; discarded"),
            Failure::Broken(why) => write!(f, "{why}; discarded"),
            Failure::NotNow(why) => write!(f, "{why}; discarded, to try again at the next check"),
            Failure::Disk(error) => write!(f, "could not be saved: {error}"),
        }
    }
}

/// Releases on GitHub, or on whatever [`SOURCE_ENV`] points at.
pub struct GitHub {
    base: String,
    agent: ureq::Agent,
}

impl GitHub {
    pub fn new() -> GitHub {
        let base = std::env::var(SOURCE_ENV)
            .ok()
            .filter(|base| !base.is_empty())
            .unwrap_or_else(|| REPOSITORY.to_string());
        let agent = ureq::Agent::config_builder()
            .timeout_connect(Some(Duration::from_secs(15)))
            .timeout_global(Some(Duration::from_secs(15 * 60)))
            .user_agent(concat!("omacharts/", env!("CARGO_PKG_VERSION")))
            .build()
            .into();
        GitHub { base: base.trim_end_matches('/').to_string(), agent }
    }
}

impl Default for GitHub {
    fn default() -> GitHub {
        GitHub::new()
    }
}

impl Source for GitHub {
    /// `releases/latest` redirects to the newest release's tag, which names
    /// its version. Not the API: that is rate limited, and the redirect is
    /// not.
    fn latest(&self) -> Result<Version, Failure> {
        let response = self
            .agent
            .get(&format!("{}/releases/latest", self.base))
            .config()
            .max_redirects(0)
            .build()
            .call()
            .map_err(|error| Failure::Network(error.to_string()))?;
        let location = response
            .headers()
            .get("location")
            .and_then(|value| value.to_str().ok())
            .ok_or_else(|| Failure::Network("releases/latest did not redirect to a release".into()))?;
        location
            .rsplit_once("/tag/")
            .and_then(|(_, tag)| Version::parse(tag))
            .ok_or_else(|| Failure::Network(format!("releases/latest went to {location}")))
    }

    fn open(&self, version: Version, asset: &str) -> Result<Box<dyn Read>, Failure> {
        let url = format!("{}/releases/download/v{version}/{asset}", self.base);
        let response = self.agent.get(&url).call().map_err(|error| Failure::Network(error.to_string()))?;
        Ok(Box::new(response.into_body().into_reader()))
    }

    /// Asked of the checksum, the last file a release's upload puts up.
    fn has(&self, version: Version, asset: &str) -> Result<bool, Failure> {
        let url = format!("{}/releases/download/v{version}/{asset}.sha256", self.base);
        match self.agent.head(&url).call() {
            Ok(_) => Ok(true),
            Err(ureq::Error::StatusCode(404)) => Ok(false),
            Err(error) => Err(Failure::Network(error.to_string())),
        }
    }
}

// -- a check --------------------------------------------------------------

/// What a check found.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Checked {
    /// Nothing newer than what is installed, downloaded or running.
    UpToDate { latest: Version },
    /// `latest` is newer, and was not downloaded because nobody asked.
    Available { latest: Version },
    /// `latest` is newer and announced, and this machine's binary is not
    /// on it yet.
    Publishing { latest: Version },
    /// A version is downloaded and starts at the next launch.
    Ready { version: Version, latest: Version },
    /// A download finished after pacman installed something as new, and was
    /// thrown away.
    Overtaken { version: Version },
}

impl Checked {
    /// The version "Restart to update" would bring, when there is one
    /// newer than both what is running and what is installed.
    pub fn restart_brings(&self, running: Version, installed: Version) -> Option<Version> {
        match *self {
            Checked::Ready { version, .. } if version > running && version > installed => Some(version),
            _ => None,
        }
    }
}

/// Ask `source` for its newest release and, with `download`, put it on the
/// shelf if it is newer than everything here.
///
/// `installed` is asked rather than passed: pacman can replace the installed
/// binary while a download runs, and the answer after the download is the
/// one that decides whether it is kept.
pub fn check(
    source: &dyn Source,
    shelf: &Shelf,
    running: Version,
    installed: &dyn Fn() -> Version,
    download: bool,
) -> Result<Checked, Failure> {
    let before = installed();
    shelf.tidy(before, alive);
    // Noted only once a check has its answer, so one that fails is asked
    // again at the next launch rather than six hours on.
    let latest = source.latest()?;

    let ready = shelf.newest_ready();
    let floor = [Some(before), Some(running), shelf.newest_known()].into_iter().flatten().max().unwrap_or(before);
    if latest <= floor {
        shelf.note_checked();
        return Ok(match ready {
            Some(version) if version > before => Checked::Ready { version, latest },
            _ => Checked::UpToDate { latest },
        });
    }
    let asset = asset();
    if !source.has(latest, &asset)? {
        return Ok(Checked::Publishing { latest });
    }
    if !download {
        shelf.note_checked();
        return Ok(Checked::Available { latest });
    }

    let mismatched = shelf.root.join(format!("{MISMATCHED}{latest}"));
    match shelf.install(source, latest, &asset) {
        Ok(()) => {
            let _ = fs::remove_file(&mismatched);
        }
        // A transfer can be corrupted on the way, so a first mismatch is
        // tried again at the next check; a second is what was published.
        Err(failure @ Failure::Mismatch { .. }) if !mismatched.exists() => {
            let _ = fs::write(&mismatched, b"");
            shelf.note_checked();
            return Err(failure);
        }
        // Not what was published, or cannot run here: never again, until a
        // newer release or `update reset`.
        Err(failure) if failure.definite() => {
            shelf.mark_bad(latest, &failure.to_string());
            return Err(failure);
        }
        // A binary that would not answer this time, a full disk, a link
        // that dropped: tried again at the next check, six hours on, not at
        // every launch — each try is the whole download again.
        Err(failure) => {
            shelf.note_checked();
            return Err(failure);
        }
    }
    shelf.note_checked();

    let after = installed();
    shelf.tidy(after, alive);
    if after >= latest {
        return Ok(Checked::Overtaken { version: latest });
    }
    Ok(Checked::Ready { version: latest, latest })
}

/// What the shelf already holds, without asking anybody: tidied against
/// `installed`, and the version a restart would bring, if one is newer than
/// both what is running and what is installed. For a launch between checks.
pub fn on_the_shelf(shelf: &Shelf, running: Version, installed: Version) -> Option<Version> {
    shelf.tidy(installed, alive);
    shelf.newest_ready().filter(|ready| *ready > running && *ready > installed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};
    use std::collections::HashMap;

    fn v(text: &str) -> Version {
        Version::parse(text).unwrap()
    }

    struct Scratch(PathBuf);

    impl Scratch {
        fn new() -> Scratch {
            static COUNTER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
            let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let dir = std::env::temp_dir().join(format!("omacharts-update-{}-{n}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap();
            Scratch(dir)
        }
        fn shelf(&self) -> Shelf {
            Shelf::new(self.0.join("versions"))
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    /// A binary that answers `--version` the way the real one does.
    fn script_saying(version: &str) -> Vec<u8> {
        format!("#!/bin/sh\necho omacharts {version}\n").into_bytes()
    }

    fn gzip(bytes: &[u8]) -> Vec<u8> {
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        encoder.write_all(bytes).unwrap();
        encoder.finish().unwrap()
    }

    fn sha256(bytes: &[u8]) -> String {
        use sha2::Digest;
        hex(&sha2::Sha256::digest(bytes))
    }

    /// Put a ready version on the shelf by hand.
    fn shelve(shelf: &Shelf, version: &str) {
        let dir = shelf.root.join(version);
        fs::create_dir_all(&dir).unwrap();
        place_executable(&dir.join(BINARY), &script_saying(version));
    }

    /// Make `path` an executable holding `contents`, never opening it for
    /// writing in this process.
    ///
    /// Tests run on many threads, and some of them start processes. A child
    /// forked while this process holds a file open for writing holds it too,
    /// until it execs, and running the file in that moment fails with "text
    /// file busy" — which the launch path takes for a version that cannot
    /// start, and marks bad. So the file is written by `install`, in a process
    /// of its own, from a copy nothing ever runs.
    fn place_executable(path: &Path, contents: &[u8]) {
        let source = path.with_file_name(format!(".source-{}", std::process::id()));
        fs::write(&source, contents).unwrap();
        place_copy(&source, path);
        fs::remove_file(&source).unwrap();
    }

    /// `source` copied to `path` as an executable, as [`place_executable`].
    fn place_copy(source: &Path, path: &Path) {
        let status = std::process::Command::new("/usr/bin/install")
            .arg("-m755")
            .arg(source)
            .arg(path)
            .status()
            .unwrap();
        assert!(status.success(), "install {} {}", source.display(), path.display());
    }

    fn on_shelf(shelf: &Shelf) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(&shelf.root)
            .into_iter()
            .flatten()
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| name != CHECKED)
            .collect();
        names.sort();
        names
    }

    /// A release server in memory. Never the network.
    struct Fake {
        latest: Version,
        assets: HashMap<String, Vec<u8>>,
        opened: RefCell<Vec<String>>,
        /// Called once the binary has been handed over, to change the world
        /// mid-download.
        during: Option<Box<dyn Fn()>>,
    }

    impl Fake {
        /// A release whose asset is a binary that says it is `version`.
        fn releasing(version: &str) -> Fake {
            Fake::with_binary(version, &script_saying(version))
        }

        fn with_binary(version: &str, binary: &[u8]) -> Fake {
            let packed = gzip(binary);
            let mut assets = HashMap::new();
            assets.insert(format!("{}.sha256", asset()), format!("{}  {}\n", sha256(&packed), asset()).into_bytes());
            assets.insert(asset(), packed);
            Fake { latest: v(version), assets, opened: RefCell::new(Vec::new()), during: None }
        }
    }

    impl Source for Fake {
        fn latest(&self) -> Result<Version, Failure> {
            Ok(self.latest)
        }
        fn open(&self, version: Version, asset: &str) -> Result<Box<dyn Read>, Failure> {
            assert_eq!(version, self.latest, "only the latest is ever downloaded");
            self.opened.borrow_mut().push(asset.to_string());
            if asset == super::asset()
                && let Some(during) = &self.during
            {
                during();
            }
            let bytes = self.assets.get(asset).cloned().ok_or_else(|| Failure::Network("404".into()))?;
            Ok(Box::new(io::Cursor::new(bytes)))
        }
        fn has(&self, version: Version, asset: &str) -> Result<bool, Failure> {
            assert_eq!(version, self.latest);
            Ok(self.assets.contains_key(&format!("{asset}.sha256")))
        }
    }

    struct Offline;
    impl Source for Offline {
        fn latest(&self) -> Result<Version, Failure> {
            Err(Failure::Network("offline".into()))
        }
        fn open(&self, _: Version, _: &str) -> Result<Box<dyn Read>, Failure> {
            Err(Failure::Network("offline".into()))
        }
        fn has(&self, _: Version, _: &str) -> Result<bool, Failure> {
            Err(Failure::Network("offline".into()))
        }
    }

    fn version_of(chosen: Option<crate::launch::Chosen>) -> Option<Version> {
        chosen.map(|chosen| chosen.version)
    }

    fn why_bad(shelf: &Shelf, version: &str) -> String {
        fs::read_to_string(shelf.root.join(version).join(BAD)).unwrap_or_default()
    }

    #[test]
    fn versions_compare_as_numbers_not_text() {
        assert!(v("0.2.10") > v("0.2.9"));
        assert!(v("1.0.0") > v("0.99.99"));
        assert_eq!(v("v0.2.15"), v("0.2.15"));
        for wrong in ["", "0.2", "0.2.15.1", "0.2.x", "0.2.-1", " v", "0..1", "0.2.15-rc1", ".incoming-0.2.1-4"] {
            assert_eq!(Version::parse(wrong), None, "{wrong:?}");
        }
        assert_eq!(Version::running().to_string(), env!("CARGO_PKG_VERSION"));
    }

    #[test]
    fn a_newer_release_is_downloaded_verified_and_put_in_place_whole() {
        let scratch = Scratch::new();
        let shelf = scratch.shelf();
        let fake = Fake::releasing("0.2.15");
        let checked = check(&fake, &shelf, v("0.2.14"), &|| v("0.2.14"), true).unwrap();
        assert_eq!(checked, Checked::Ready { version: v("0.2.15"), latest: v("0.2.15") });
        assert_eq!(on_shelf(&shelf), ["0.2.15"], "nothing but the version, no temp left behind");
        assert!(shelf.root.join("0.2.15/omacharts").is_file());
        assert_eq!(probe(&shelf.root.join("0.2.15/omacharts"), Duration::from_secs(5)), Ok(v("0.2.15")));
        assert_eq!(checked.restart_brings(v("0.2.14"), v("0.2.14")), Some(v("0.2.15")));
    }

    #[test]
    fn a_download_that_does_not_match_its_checksum_twice_is_discarded_and_not_fetched_again_until_a_reset() {
        let scratch = Scratch::new();
        let shelf = scratch.shelf();
        let mut fake = Fake::releasing("0.2.15");
        fake.assets.insert(asset(), gzip(b"#!/bin/sh\necho tampered\n"));
        // Once could be the transfer: tried again at the next check.
        let failed = check(&fake, &shelf, v("0.2.14"), &|| v("0.2.14"), true).unwrap_err();
        assert!(matches!(failed, Failure::Mismatch { .. }), "{failed}");
        assert!(!shelf.root.join("0.2.15").exists(), "nothing kept: {:?}", on_shelf(&shelf));
        assert!(shelf.root.join(".mismatched-0.2.15").exists());
        assert!(!shelf.check_due(SystemTime::now()), "the next try waits for the next check");
        // Twice is what was published.
        let failed = check(&fake, &shelf, v("0.2.14"), &|| v("0.2.14"), true).unwrap_err();
        assert!(matches!(failed, Failure::Mismatch { .. }), "{failed}");
        assert!(!shelf.root.join("0.2.15").join(BINARY).exists(), "nothing kept: {:?}", on_shelf(&shelf));
        assert!(why_bad(&shelf, "0.2.15").contains("checksum"), "{}", why_bad(&shelf, "0.2.15"));
        assert_eq!(shelf.choose(v("0.2.14"), |_| true), None);

        let again = Fake::releasing("0.2.15");
        assert_eq!(check(&again, &shelf, v("0.2.14"), &|| v("0.2.14"), true).unwrap(), Checked::UpToDate { latest: v("0.2.15") });
        assert!(again.opened.borrow().is_empty());

        // `update reset` forgets it, and the next check fetches it whole.
        shelf.reset(v("0.2.14"), false).unwrap();
        let checked = check(&again, &shelf, v("0.2.14"), &|| v("0.2.14"), true).unwrap();
        assert_eq!(checked, Checked::Ready { version: v("0.2.15"), latest: v("0.2.15") });
    }

    #[test]
    fn a_checksum_file_that_is_not_a_checksum_is_refused() {
        let scratch = Scratch::new();
        let shelf = scratch.shelf();
        let mut fake = Fake::releasing("0.2.15");
        fake.assets.insert(format!("{}.sha256", asset()), b"<html>Not Found</html>".to_vec());
        assert!(matches!(check(&fake, &shelf, v("0.2.14"), &|| v("0.2.14"), true), Err(Failure::Network(_))));
        assert!(on_shelf(&shelf).is_empty());
    }

    #[test]
    fn a_verified_binary_that_says_another_version_or_cannot_run_here_is_never_tried_again() {
        let scratch = Scratch::new();
        let shelf = scratch.shelf();
        let fake = Fake::with_binary("0.2.15", &script_saying("0.2.13"));
        assert!(matches!(check(&fake, &shelf, v("0.2.14"), &|| v("0.2.14"), true), Err(Failure::Broken(_))));
        assert_eq!(shelf.choose(v("0.2.14"), |_| true), None);
        assert!(why_bad(&shelf, "0.2.15").contains("says it is 0.2.13"), "{}", why_bad(&shelf, "0.2.15"));
        // And the next check does not fetch it again.
        let again = Fake::with_binary("0.2.15", &script_saying("0.2.13"));
        assert_eq!(
            check(&again, &shelf, v("0.2.14"), &|| v("0.2.14"), true).unwrap(),
            Checked::UpToDate { latest: v("0.2.15") }
        );
        assert!(again.opened.borrow().is_empty());

        let scratch = Scratch::new();
        let shelf = scratch.shelf();
        let fake = Fake::with_binary("0.2.15", b"\x7fELF not really");
        let failed = check(&fake, &shelf, v("0.2.14"), &|| v("0.2.14"), true).unwrap_err();
        assert!(matches!(failed, Failure::Broken(_)), "{failed}");
        assert_eq!(shelf.choose(v("0.2.14"), |_| true), None);
        assert!(why_bad(&shelf, "0.2.15").contains("cannot run here"), "{}", why_bad(&shelf, "0.2.15"));
    }

    #[test]
    fn a_verified_binary_that_does_not_answer_this_time_is_discarded_but_not_given_up_on() {
        let scratch = Scratch::new();
        let shelf = scratch.shelf();
        // Ends with a failure, which an out-of-memory or a busy machine can
        // make of any binary: nothing says it would the next time.
        let fake = Fake::with_binary("0.2.15", b"#!/bin/sh\nexit 3\n");
        let failed = check(&fake, &shelf, v("0.2.14"), &|| v("0.2.14"), true).unwrap_err();
        assert!(matches!(failed, Failure::NotNow(_)), "{failed}");
        assert!(on_shelf(&shelf).is_empty(), "nothing kept, nothing marked: {:?}", on_shelf(&shelf));
        assert!(!shelf.check_due(SystemTime::now()), "tried again at the next check, not at every launch");

        let fine = Fake::releasing("0.2.15");
        let checked = check(&fine, &shelf, v("0.2.14"), &|| v("0.2.14"), true).unwrap();
        assert_eq!(checked, Checked::Ready { version: v("0.2.15"), latest: v("0.2.15") });
        assert!(!shelf.check_due(SystemTime::now()));
    }

    #[test]
    fn a_binary_whose_libraries_are_missing_or_that_panics_cannot_run_here() {
        // The dynamic loader's exit when a library is missing or too old,
        // the shell's for a file it cannot execute, and a Rust panic's.
        for code in [127, 126, 101] {
            let scratch = Scratch::new();
            let shelf = scratch.shelf();
            let fake = Fake::with_binary("0.2.15", format!("#!/bin/sh\nexit {code}\n").as_bytes());
            let failed = check(&fake, &shelf, v("0.2.14"), &|| v("0.2.14"), true).unwrap_err();
            assert!(matches!(failed, Failure::Broken(_)), "{code}: {failed}");
            assert!(why_bad(&shelf, "0.2.15").contains("cannot run here"), "{}", why_bad(&shelf, "0.2.15"));
            let again = Fake::releasing("0.2.15");
            check(&again, &shelf, v("0.2.14"), &|| v("0.2.14"), true).unwrap();
            assert!(again.opened.borrow().is_empty(), "never downloaded again");
        }
    }

    #[test]
    fn a_download_that_fails_on_the_way_waits_for_the_next_check() {
        let scratch = Scratch::new();
        let shelf = scratch.shelf();
        let mut fake = Fake::releasing("0.2.15");
        fake.assets.remove(&asset());
        let failed = check(&fake, &shelf, v("0.2.14"), &|| v("0.2.14"), true).unwrap_err();
        assert!(matches!(failed, Failure::Network(_)), "{failed}");
        assert!(!shelf.check_due(SystemTime::now()), "not the whole download again at the next launch");
        assert!(!shelf.root.join("0.2.15").join(BAD).exists(), "a failed download is not a bad version");
    }

    #[test]
    fn a_mismatch_once_then_a_good_download_is_ready() {
        let scratch = Scratch::new();
        let shelf = scratch.shelf();
        let mut fake = Fake::releasing("0.2.15");
        fake.assets.insert(asset(), gzip(b"#!/bin/sh\necho tampered\n"));
        check(&fake, &shelf, v("0.2.14"), &|| v("0.2.14"), true).unwrap_err();
        let good = Fake::releasing("0.2.15");
        let checked = check(&good, &shelf, v("0.2.14"), &|| v("0.2.14"), true).unwrap();
        assert_eq!(checked, Checked::Ready { version: v("0.2.15"), latest: v("0.2.15") });
        assert_eq!(on_shelf(&shelf), ["0.2.15"]);
    }

    #[test]
    fn a_probe_that_crashes_is_definite_and_one_killed_is_for_now() {
        let scratch = Scratch::new();
        for (signal, definite) in [("SEGV", true), ("ABRT", true), ("TERM", false), ("KILL", false)] {
            let binary = scratch.0.join(format!("dies-{signal}"));
            place_executable(&binary, format!("#!/bin/sh\nkill -{signal} $$\n").as_bytes());
            let probed = probe(&binary, Duration::from_secs(5));
            assert_eq!(matches!(probed, Err(Unstarted::Definitely(_))), definite, "{signal}: {probed:?}");
        }
    }

    #[test]
    fn a_probe_that_takes_too_long_is_for_now() {
        let scratch = Scratch::new();
        let binary = scratch.0.join("slow");
        place_executable(&binary, b"#!/bin/sh\nsleep 5\necho omacharts 0.2.15\n");
        assert!(matches!(probe(&binary, Duration::from_millis(200)), Err(Unstarted::ForNow(_))));
    }

    #[test]
    fn only_a_failure_that_would_happen_again_is_definite() {
        use crate::launch::{refused, Refused};
        assert!(Failure::Mismatch { asset: asset() }.definite());
        assert!(Failure::Broken("no".into()).definite());
        for passing in [
            Failure::NotNow("slow".into()),
            Failure::Network("offline".into()),
            Failure::Disk(io::Error::from_raw_os_error(28)),
        ] {
            assert!(!passing.definite(), "{passing}");
        }

        let scratch = Scratch::new();
        let there = scratch.0.join("there");
        fs::write(&there, b"").unwrap();
        let gone = scratch.0.join("gone");
        let errno = io::Error::from_raw_os_error;
        assert!(matches!(refused(&errno(8), &there), Refused::Definitely(_)), "ENOEXEC");
        assert!(matches!(refused(&errno(80), &there), Refused::Definitely(_)), "ELIBBAD");
        assert!(matches!(refused(&errno(2), &there), Refused::Definitely(_)), "no interpreter");
        assert_eq!(refused(&errno(2), &gone), Refused::Vanished, "tidied away meanwhile");
        for passing in [12, 11, 13, 26, 24] {
            assert!(matches!(refused(&errno(passing), &there), Refused::ForNow(_)), "errno {passing}");
        }
    }

    #[test]
    fn a_release_whose_binary_is_not_up_yet_is_being_published_and_asked_again() {
        let scratch = Scratch::new();
        let shelf = scratch.shelf();
        let mut fake = Fake::releasing("0.2.15");
        fake.assets.clear();
        for download in [false, true] {
            let checked = check(&fake, &shelf, v("0.2.14"), &|| v("0.2.14"), download).unwrap();
            assert_eq!(checked, Checked::Publishing { latest: v("0.2.15") });
            assert_eq!(checked.restart_brings(v("0.2.14"), v("0.2.14")), None);
        }
        assert!(fake.opened.borrow().is_empty());
        assert!(shelf.check_due(SystemTime::now()), "not counted as a check");
        assert!(on_shelf(&shelf).is_empty());
    }

    #[test]
    fn a_check_is_noted_only_once_it_has_its_answer() {
        let scratch = Scratch::new();
        let shelf = scratch.shelf();
        let _ = check(&Offline, &shelf, v("0.2.14"), &|| v("0.2.14"), true);
        assert!(shelf.check_due(SystemTime::now()), "offline");
        fs::write(scratch.0.join("versions"), b"").unwrap();
        let _ = check(&Fake::releasing("0.2.15"), &shelf, v("0.2.14"), &|| v("0.2.14"), true);
        fs::remove_file(scratch.0.join("versions")).unwrap();
        assert!(shelf.check_due(SystemTime::now()), "a download that could not be saved");
        check(&Fake::releasing("0.2.15"), &shelf, v("0.2.14"), &|| v("0.2.14"), false).unwrap();
        assert!(!shelf.check_due(SystemTime::now()), "an answer");
    }

    #[test]
    fn a_truncated_download_leaves_nothing_behind() {
        let scratch = Scratch::new();
        let shelf = scratch.shelf();
        let mut fake = Fake::releasing("0.2.15");
        let whole = fake.assets[&asset()].clone();
        let cut = whole[..whole.len() / 2].to_vec();
        // Published for the cut file, so the checksum passes and the
        // decompression is what has to notice.
        fake.assets.insert(format!("{}.sha256", asset()), format!("{}\n", sha256(&cut)).into_bytes());
        fake.assets.insert(asset(), cut);
        assert!(check(&fake, &shelf, v("0.2.14"), &|| v("0.2.14"), true).is_err());
        assert!(on_shelf(&shelf).iter().all(|name| !name.starts_with(INCOMING)), "{:?}", on_shelf(&shelf));
        assert_eq!(shelf.choose(v("0.2.14"), |_| true), None);
    }

    #[test]
    fn an_unwritable_shelf_fails_the_download_and_puts_nothing_in_place() {
        let scratch = Scratch::new();
        // A file where the directory should be: every write fails, as a full
        // disk would.
        fs::write(scratch.0.join("versions"), b"").unwrap();
        let shelf = scratch.shelf();
        let failed = check(&Fake::releasing("0.2.15"), &shelf, v("0.2.14"), &|| v("0.2.14"), true).unwrap_err();
        assert!(matches!(failed, Failure::Disk(_)), "{failed}");
    }

    #[test]
    fn offline_is_a_failure_and_changes_nothing() {
        let scratch = Scratch::new();
        let shelf = scratch.shelf();
        shelve(&shelf, "0.2.15");
        assert!(matches!(check(&Offline, &shelf, v("0.2.14"), &|| v("0.2.14"), true), Err(Failure::Network(_))));
        assert_eq!(on_shelf(&shelf), ["0.2.15"]);
    }

    #[test]
    fn nothing_is_downloaded_unless_newer_than_installed_downloaded_and_running() {
        // (a) Installed is newer than the one running (a download, before
        // pacman caught up and overtook it), and the release is between them.
        let scratch = Scratch::new();
        let shelf = scratch.shelf();
        let fake = Fake::releasing("0.2.15");
        let checked = check(&fake, &shelf, v("0.2.14"), &|| v("0.2.16"), true).unwrap();
        assert_eq!(checked, Checked::UpToDate { latest: v("0.2.15") });
        assert!(fake.opened.borrow().is_empty());

        // Already downloaded: not again.
        let scratch = Scratch::new();
        let shelf = scratch.shelf();
        shelve(&shelf, "0.2.15");
        let fake = Fake::releasing("0.2.15");
        let checked = check(&fake, &shelf, v("0.2.14"), &|| v("0.2.14"), true).unwrap();
        assert_eq!(checked, Checked::Ready { version: v("0.2.15"), latest: v("0.2.15") });
        assert!(fake.opened.borrow().is_empty());

        // Running it already, handed over to.
        let checked = check(&fake, &shelf, v("0.2.15"), &|| v("0.2.14"), true).unwrap();
        assert_eq!(checked.restart_brings(v("0.2.15"), v("0.2.14")), None);
        assert!(fake.opened.borrow().is_empty());

        // Without `download`, only said.
        let scratch = Scratch::new();
        let shelf = scratch.shelf();
        let fake = Fake::releasing("0.2.15");
        let checked = check(&fake, &shelf, v("0.2.14"), &|| v("0.2.14"), false).unwrap();
        assert_eq!(checked, Checked::Available { latest: v("0.2.15") });
        assert!(fake.opened.borrow().is_empty());
        assert!(on_shelf(&shelf).is_empty());
    }

    #[test]
    fn a_launch_hands_over_only_to_a_ready_version_strictly_newer_than_installed() {
        // (b)
        let scratch = Scratch::new();
        let shelf = scratch.shelf();
        assert_eq!(shelf.choose(v("0.2.14"), |_| true), None, "no folder at all");
        fs::create_dir_all(&shelf.root).unwrap();
        assert_eq!(shelf.choose(v("0.2.14"), |_| true), None, "an empty folder");

        shelve(&shelf, "0.2.13");
        shelve(&shelf, "0.2.14");
        assert_eq!(shelf.choose(v("0.2.14"), |_| true), None, "older and equal are never chosen");

        shelve(&shelf, "0.2.16");
        shelve(&shelf, "0.2.15");
        fs::create_dir_all(shelf.root.join(".incoming-0.2.17-1")).unwrap();
        fs::create_dir_all(shelf.root.join("0.2.18")).unwrap(); // no binary: not complete
        let chosen = shelf.choose(v("0.2.14"), |_| true).unwrap();
        assert_eq!(chosen.version, v("0.2.16"));
        assert_eq!(chosen.binary, shelf.root.join("0.2.16/omacharts"));
        assert_eq!(chosen.folder, shelf.root.join("0.2.16"));

        shelf.mark_bad(v("0.2.16"), "a test says so");
        assert_eq!(version_of(shelf.choose(v("0.2.14"), |_| true)), Some(v("0.2.15")));
        assert_eq!(shelf.choose(v("0.2.16"), |_| true), None);
        assert_eq!(why_bad(&shelf, "0.2.16"), "a test says so\n");
    }

    #[test]
    fn three_launches_in_a_row_that_end_before_the_version_is_healthy_give_it_up() {
        let scratch = Scratch::new();
        let shelf = scratch.shelf();
        shelve(&shelf, "0.2.15");
        shelve(&shelf, "0.2.16");
        let folder = shelf.root.join("0.2.16");
        let alive = |pid: u32| pid >= 1000;
        // Launches in flight are not failures, however many.
        for pid in 2000..2005 {
            fs::write(folder.join(trying(pid)), b"").unwrap();
        }
        assert_eq!(version_of(shelf.choose(v("0.2.14"), alive)), Some(v("0.2.16")));
        // Ended before it was healthy: two may be bad luck.
        for pid in [1, 2] {
            fs::write(folder.join(trying(pid)), b"").unwrap();
        }
        assert_eq!(version_of(shelf.choose(v("0.2.14"), alive)), Some(v("0.2.16")));
        // Three is the version.
        fs::write(folder.join(trying(3)), b"").unwrap();
        assert_eq!(version_of(shelf.choose(v("0.2.14"), alive)), Some(v("0.2.15")));
        assert!(why_bad(&shelf, "0.2.16").contains("3 launches"), "{}", why_bad(&shelf, "0.2.16"));
    }

    #[test]
    fn a_version_that_has_been_healthy_is_never_given_up_on() {
        let scratch = Scratch::new();
        let shelf = scratch.shelf();
        shelve(&shelf, "0.2.15");
        shelve(&shelf, "0.2.16");
        let folder = shelf.root.join("0.2.16");
        for pid in 1..10 {
            fs::write(folder.join(trying(pid)), b"").unwrap();
        }
        mark_healthy(&folder);
        assert!(folder.join(HEALTHY).exists());
        assert!(!fs::read_dir(&folder).unwrap().flatten().any(|entry| entry.file_name().to_string_lossy().starts_with(".trying-")), "every marker cleared");

        for pid in 1..10 {
            fs::write(folder.join(trying(pid)), b"").unwrap();
        }
        let chosen = shelf.choose(v("0.2.14"), |_| false).unwrap();
        assert_eq!(chosen.version, v("0.2.16"));
        assert!(chosen.healthy);
        shelf.mark_bad(v("0.2.16"), "nope");
        assert!(!folder.join(BAD).exists());
    }

    #[test]
    fn the_launcher_says_which_folder_it_handed_over_to() {
        let told = Told { installed_version: v("0.2.14"), installed: PathBuf::from("/usr/bin/omacharts"), folder: Some(PathBuf::from("/home/a/.local/share/omacharts/versions/0.2.16")) };
        assert_eq!(Told::decode(&told.encode()), Some(told.clone()));
        let none = Told { folder: None, ..told };
        assert_eq!(Told::decode(&none.encode()), Some(none.clone()));
        assert_eq!(Told::decode("0.2.14\u{1f}/usr/bin/omacharts"), Some(none), "the two-field form still reads");
        assert_eq!(Told::decode("nonsense"), None);
    }

    #[test]
    fn a_folder_reached_through_a_symbolic_link_is_the_same_folder() {
        let scratch = Scratch::new();
        let real = scratch.0.join("real/versions/0.2.16");
        fs::create_dir_all(&real).unwrap();
        shelve(&Shelf::new(scratch.0.join("real/versions")), "0.2.16");
        std::os::unix::fs::symlink(scratch.0.join("real"), scratch.0.join("linked")).unwrap();
        let exe = fs::canonicalize(real.join(BINARY)).unwrap();
        assert!(runs_from(&exe, &scratch.0.join("linked/versions/0.2.16")));
        assert!(runs_from(&exe, &real));
        assert!(!runs_from(&exe, &scratch.0.join("linked/versions")));
        assert!(under(&exe, &scratch.0.join("linked/versions")));
    }

    #[test]
    fn the_settings_are_backed_up_before_a_version_that_never_ran_here_opens_them() {
        let scratch = Scratch::new();
        let data = scratch.0.join("data");
        fs::create_dir_all(&data).unwrap();
        let settings = data.join("omacharts.db");
        crate::store::Store::open_at(&settings).unwrap().set_setting("a", "b");
        let shelf = Shelf::new(data.join("versions"));
        shelve(&shelf, "0.2.16");
        let folder = shelf.root.join("0.2.16");

        backup_before_first_open(&folder, &settings).unwrap();
        let record = shelf.root.join(".before-0.2.16");
        let saved = shelf.root.join(".saved-0.2.16");
        assert_eq!(fs::read_to_string(&record).unwrap(), format!("{}\n{}\n", crate::migrations::LATEST, saved.display()));
        assert_eq!(setting_a(&saved).as_deref(), Some("b"));
        assert!(crate::backup::list(&settings).is_empty(), "not among the backups, which are pruned");

        // Once is enough, and a version that has run needs none — and is
        // never rolled back, so the record and the copy go.
        crate::store::Store::open_at(&settings).unwrap().set_setting("a", "c");
        backup_before_first_open(&folder, &settings).unwrap();
        assert_eq!(setting_a(&saved).as_deref(), Some("b"));
        mark_healthy(&folder);
        assert!(!record.exists() && !saved.exists());
        backup_before_first_open(&folder, &settings).unwrap();
        assert!(!record.exists() && !saved.exists());

        // Nothing to back up on a first launch.
        let fresh = Scratch::new();
        let folder = fresh.0.join("0.2.16");
        fs::create_dir_all(&folder).unwrap();
        backup_before_first_open(&folder, &fresh.0.join("omacharts.db")).unwrap();
        assert_eq!(fs::read_to_string(fresh.0.join(".before-0.2.16")).unwrap(), "none\n\n");
    }

    /// A settings file with `a` set to `value`, read back.
    fn setting_a(settings: &Path) -> Option<String> {
        let conn = rusqlite::Connection::open(settings).ok()?;
        conn.query_row("SELECT value FROM settings WHERE key = 'a'", [], |r| r.get(0)).ok()
    }

    /// What a version newer than this build does to the settings: change
    /// one, and migrate them past what this build can open.
    fn migrate_past(settings: &Path, value: &str, schema: i32) {
        let conn = rusqlite::Connection::open(settings).unwrap();
        conn.execute("UPDATE settings SET value = ?1 WHERE key = 'a'", [value]).unwrap();
        conn.pragma_update(None, "user_version", schema).unwrap();
    }

    const KNOWN: i32 = crate::migrations::LATEST;

    /// A shelf in `data/versions` and settings in `data`, `a` set to `before`.
    fn settings_and_shelf(scratch: &Scratch) -> (PathBuf, PathBuf, Shelf) {
        let data = scratch.0.join("data");
        fs::create_dir_all(&data).unwrap();
        let settings = data.join("omacharts.db");
        crate::store::Store::open_at(&settings).unwrap().set_setting("a", "before");
        (data.clone(), settings, Shelf::new(data.join("versions")))
    }

    fn set_aside(data: &Path, version: &str) -> Vec<PathBuf> {
        let prefix = format!("omacharts.db.from-{version}-");
        fs::read_dir(data)
            .unwrap()
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.file_name().unwrap().to_string_lossy().starts_with(&prefix))
            .filter(|path| !path.to_string_lossy().ends_with("-wal") && !path.to_string_lossy().ends_with("-shm"))
            .collect()
    }

    #[test]
    fn settings_a_version_given_up_on_migrated_are_put_back_and_the_newer_ones_kept() {
        let scratch = Scratch::new();
        let (data, settings, shelf) = settings_and_shelf(&scratch);
        shelve(&shelf, "0.2.16");
        backup_before_first_open(&shelf.root.join("0.2.16"), &settings).unwrap();
        // It ran, migrated the settings, and was given up on.
        migrate_past(&settings, "after", KNOWN + 1);
        assert_eq!(roll_back(&shelf, &settings, v("0.2.14"), KNOWN), None, "still runnable: it is started instead");
        shelf.mark_bad(v("0.2.16"), "3 launches in a row ended before it ran properly");

        assert_eq!(roll_back(&shelf, &settings, v("0.2.16"), KNOWN), None, "not newer than this one");
        let said = roll_back(&shelf, &settings, v("0.2.14"), KNOWN).expect("rolled back");
        assert!(said.contains("0.2.16") && said.contains("omacharts.db.from-0.2.16-"), "{said}");
        assert_eq!(setting_a(&settings).as_deref(), Some("before"));
        assert_eq!(schema_of(&settings), Some(KNOWN));
        let aside = set_aside(&data, "0.2.16");
        assert_eq!(aside.len(), 1, "{aside:?}");
        assert_eq!(setting_a(&aside[0]).as_deref(), Some("after"), "the newer settings kept, never deleted");
        assert!(!data.read_dir().unwrap().flatten().any(|e| e.file_name().to_string_lossy().contains("restoring")));

        // Once.
        assert!(!shelf.root.join(".before-0.2.16").exists());
        migrate_past(&settings, "again", KNOWN + 1);
        assert_eq!(roll_back(&shelf, &settings, v("0.2.14"), KNOWN), None);
        assert_eq!(setting_a(&settings).as_deref(), Some("again"));
    }

    #[test]
    fn settings_this_version_can_open_are_left_alone() {
        let scratch = Scratch::new();
        let (_, settings, shelf) = settings_and_shelf(&scratch);
        shelve(&shelf, "0.2.16");
        backup_before_first_open(&shelf.root.join("0.2.16"), &settings).unwrap();
        shelf.mark_bad(v("0.2.16"), "no");
        crate::store::Store::open_at(&settings).unwrap().set_setting("a", "after");
        assert_eq!(roll_back(&shelf, &settings, v("0.2.14"), KNOWN), None);
        assert_eq!(setting_a(&settings).as_deref(), Some("after"));
    }

    #[test]
    fn the_newest_backup_this_version_can_open_is_the_one_put_back() {
        let scratch = Scratch::new();
        let (data, settings, shelf) = settings_and_shelf(&scratch);
        // 0.2.15 opened them as they were; 0.2.16 after 0.2.15 had changed
        // them; 0.2.17 after 0.2.16 had migrated them past this build.
        shelve(&shelf, "0.2.15");
        backup_before_first_open(&shelf.root.join("0.2.15"), &settings).unwrap();
        crate::store::Store::open_at(&settings).unwrap().set_setting("a", "by 0.2.15");
        shelve(&shelf, "0.2.16");
        backup_before_first_open(&shelf.root.join("0.2.16"), &settings).unwrap();
        migrate_past(&settings, "by 0.2.16", KNOWN + 1);
        shelve(&shelf, "0.2.17");
        backup_before_first_open(&shelf.root.join("0.2.17"), &settings).unwrap();
        migrate_past(&settings, "by 0.2.17", KNOWN + 2);
        for version in ["0.2.15", "0.2.16", "0.2.17"] {
            shelf.mark_bad(v(version), "no");
        }

        // 0.2.17's backup is from 0.2.16's schema, which this build cannot
        // open; 0.2.16's is the newest it can, and keeps 0.2.15's change.
        roll_back(&shelf, &settings, v("0.2.14"), KNOWN).expect("rolled back");
        assert_eq!(setting_a(&settings).as_deref(), Some("by 0.2.15"));
        assert_eq!(set_aside(&data, "0.2.16").len(), 1);
        assert_eq!(on_shelf(&shelf).iter().filter(|name| name.starts_with(BEFORE)).count(), 1, "{:?}", on_shelf(&shelf));
        assert!(shelf.root.join(".before-0.2.17").exists(), "for a build that can open its backup");
    }

    #[test]
    fn a_version_that_opened_the_settings_without_migrating_them_is_never_rolled_back_to() {
        let scratch = Scratch::new();
        let (_, settings, shelf) = settings_and_shelf(&scratch);
        shelve(&shelf, "0.2.15");
        let folder = shelf.root.join("0.2.15");
        backup_before_first_open(&folder, &settings).unwrap();
        // It opened them at the same schema and changed a setting: not a
        // migration, and nothing to roll back to.
        crate::store::Store::open_at(&settings).unwrap().set_setting("a", "by 0.2.15");
        forget_if_unmigrated(&folder, &settings);
        assert!(!shelf.root.join(".saved-0.2.15").exists());
        assert_eq!(fs::read_to_string(shelf.root.join(".before-0.2.15")).unwrap(), UNMIGRATED);
        // And its next launch copies nothing again.
        backup_before_first_open(&folder, &settings).unwrap();
        assert!(!shelf.root.join(".saved-0.2.15").exists());

        // One that did migrate keeps it.
        shelve(&shelf, "0.2.16");
        let folder = shelf.root.join("0.2.16");
        backup_before_first_open(&folder, &settings).unwrap();
        migrate_past(&settings, "by 0.2.16", KNOWN + 1);
        forget_if_unmigrated(&folder, &settings);
        assert!(shelf.root.join(".before-0.2.16").exists());
        shelf.mark_bad(v("0.2.15"), "no");
        shelf.mark_bad(v("0.2.16"), "no");
        roll_back(&shelf, &settings, v("0.2.14"), KNOWN).expect("rolled back");
        assert_eq!(setting_a(&settings).as_deref(), Some("by 0.2.15"), "the change 0.2.15 made is kept");
    }

    #[test]
    fn the_copy_from_before_a_version_outlives_the_backups_taken_and_pruned_after_it() {
        let scratch = Scratch::new();
        let (_, settings, shelf) = settings_and_shelf(&scratch);
        shelve(&shelf, "0.2.16");
        // A backup from before, as the window's hourly one leaves.
        crate::backup::take_if_changed(&settings).unwrap();
        backup_before_first_open(&shelf.root.join("0.2.16"), &settings).unwrap();
        // It migrates, and the window's backup runs before it has been up
        // three seconds: a newer backup in the same slot, and a prune.
        migrate_past(&settings, "after", KNOWN + 1);
        std::thread::sleep(Duration::from_millis(1100));
        crate::backup::take_if_changed(&settings).unwrap();
        assert_eq!(crate::backup::list(&settings).len(), 1, "the backup from before is pruned");
        shelf.mark_bad(v("0.2.16"), "no");
        roll_back(&shelf, &settings, v("0.2.14"), KNOWN).expect("rolled back");
        assert_eq!(setting_a(&settings).as_deref(), Some("before"));
        assert!(!shelf.root.join(".saved-0.2.16").exists(), "used, and gone with its record");
    }

    #[test]
    fn a_command_and_a_window_starting_a_version_together_copy_the_settings_once() {
        let scratch = Scratch::new();
        let (_, settings, shelf) = settings_and_shelf(&scratch);
        shelve(&shelf, "0.2.16");
        let folder = shelf.root.join("0.2.16");
        let barrier = std::sync::Barrier::new(4);
        std::thread::scope(|scope| {
            for _ in 0..4 {
                scope.spawn(|| {
                    barrier.wait();
                    backup_before_first_open(&folder, &settings).unwrap();
                    // What each process does next: migrate.
                    migrate_past(&settings, "after", KNOWN + 1);
                });
            }
        });
        let saved = shelf.root.join(".saved-0.2.16");
        assert_eq!(setting_a(&saved).as_deref(), Some("before"), "copied before any of them migrated");
        assert_eq!(schema_of(&saved), Some(KNOWN));
        assert!(on_shelf(&shelf).iter().all(|name| !name.contains(".before-0.2.16.")), "{:?}", on_shelf(&shelf));
    }

    #[test]
    fn a_version_that_migrated_the_settings_is_kept_until_a_newer_one_has_run() {
        let scratch = Scratch::new();
        let (_, settings, shelf) = settings_and_shelf(&scratch);
        // 0.2.15 is only ever run from the bar: it migrates, and is never
        // a window, so never healthy. Then 0.2.16 is downloaded.
        shelve(&shelf, "0.2.15");
        backup_before_first_open(&shelf.root.join("0.2.15"), &settings).unwrap();
        migrate_past(&settings, "by 0.2.15", KNOWN + 1);
        forget_if_unmigrated(&shelf.root.join("0.2.15"), &settings);
        shelve(&shelf, "0.2.16");
        shelf.tidy(v("0.2.14"), |_| true);
        assert!(shelf.root.join("0.2.15").join(BINARY).exists(), "{:?}", on_shelf(&shelf));
        // If 0.2.16 is given up on, 0.2.15 runs and opens them.
        shelf.mark_bad(v("0.2.16"), "no");
        assert_eq!(version_of(shelf.choose(v("0.2.14"), |_| true)), Some(v("0.2.15")));

        // Once a newer one has run, it reads them, and 0.2.15 can go.
        shelve(&shelf, "0.2.17");
        mark_healthy(&shelf.root.join("0.2.17"));
        shelf.tidy(v("0.2.14"), |_| true);
        assert!(!shelf.root.join("0.2.15").exists(), "{:?}", on_shelf(&shelf));
    }

    #[test]
    fn two_rollbacks_at_once_put_the_settings_back_once() {
        let scratch = Scratch::new();
        let (data, settings, shelf) = settings_and_shelf(&scratch);
        shelve(&shelf, "0.2.16");
        backup_before_first_open(&shelf.root.join("0.2.16"), &settings).unwrap();
        migrate_past(&settings, "after", KNOWN + 1);
        shelf.mark_bad(v("0.2.16"), "no");

        let barrier = std::sync::Barrier::new(4);
        let done: Vec<Option<String>> = std::thread::scope(|scope| {
            let runs: Vec<_> = (0..4)
                .map(|_| {
                    scope.spawn(|| {
                        barrier.wait();
                        roll_back(&shelf, &settings, v("0.2.14"), KNOWN)
                    })
                })
                .collect();
            runs.into_iter().map(|run| run.join().unwrap()).collect()
        });
        assert_eq!(done.iter().flatten().count(), 1, "{done:?}");
        assert_eq!(setting_a(&settings).as_deref(), Some("before"));
        let aside = set_aside(&data, "0.2.16");
        assert_eq!(aside.len(), 1, "{aside:?}");
        assert_eq!(setting_a(&aside[0]).as_deref(), Some("after"));
    }

    #[test]
    fn a_version_that_has_run_or_was_tidied_after_running_is_never_rolled_back() {
        let scratch = Scratch::new();
        let (_, settings, shelf) = settings_and_shelf(&scratch);
        shelve(&shelf, "0.2.15");
        shelve(&shelf, "0.2.16");
        backup_before_first_open(&shelf.root.join("0.2.15"), &settings).unwrap();
        backup_before_first_open(&shelf.root.join("0.2.16"), &settings).unwrap();
        migrate_past(&settings, "after", KNOWN + 1);
        // A newer one ran: what every older one did is behind it.
        mark_healthy(&shelf.root.join("0.2.16"));
        assert!(!on_shelf(&shelf).iter().any(|name| name.starts_with(BEFORE)), "{:?}", on_shelf(&shelf));
        fs::remove_dir_all(shelf.root.join("0.2.16")).unwrap();
        shelf.mark_bad(v("0.2.15"), "no");
        assert_eq!(roll_back(&shelf, &settings, v("0.2.14"), KNOWN), None);
        assert_eq!(setting_a(&settings).as_deref(), Some("after"));
    }

    #[test]
    fn a_rollback_from_a_command_is_left_for_the_next_window_to_say() {
        let scratch = Scratch::new();
        let shelf = scratch.shelf();
        fs::create_dir_all(&shelf.root).unwrap();
        fs::write(shelf.root.join(NOTICE), "said").unwrap();
        shelf.reset(v("0.2.14"), true).unwrap();
        assert!(shelf.root.join(NOTICE).exists(), "a reset does not swallow it");
    }

    #[test]
    fn a_plain_reset_keeps_which_backup_came_before_which_version_and_all_does_not() {
        let scratch = Scratch::new();
        let shelf = scratch.shelf();
        shelve(&shelf, "0.2.16");
        fs::write(shelf.root.join(".before-0.2.16"), b"none\n\n").unwrap();
        fs::write(shelf.root.join(".saved-0.2.16"), b"").unwrap();
        shelf.reset(v("0.2.14"), false).unwrap();
        assert_eq!(on_shelf(&shelf), [".before-0.2.16", ".saved-0.2.16"]);
        shelf.reset(v("0.2.14"), true).unwrap();
        assert_eq!(on_shelf(&shelf), [".saved-0.2.16"], "kept to put back by hand");
        shelf.tidy(v("0.2.16"), |_| true);
        assert!(on_shelf(&shelf).is_empty(), "{:?}", on_shelf(&shelf));
    }

    #[test]
    fn tidying_forgets_the_records_of_versions_the_package_has_caught_up_with() {
        let scratch = Scratch::new();
        let shelf = scratch.shelf();
        fs::create_dir_all(&shelf.root).unwrap();
        for name in [".before-0.2.15", ".before-0.2.17", ".mismatched-0.2.15", ".mismatched-0.2.17"] {
            fs::write(shelf.root.join(name), b"none\n\n").unwrap();
        }
        shelf.tidy(v("0.2.16"), |_| true);
        assert_eq!(on_shelf(&shelf), [".before-0.2.17", ".mismatched-0.2.17"]);
    }

    #[test]
    fn a_reset_clears_everything_but_a_newer_version_that_has_run() {
        let scratch = Scratch::new();
        let shelf = scratch.shelf();
        shelve(&shelf, "0.2.15");
        shelve(&shelf, "0.2.16");
        mark_healthy(&shelf.root.join("0.2.15"));
        shelf.mark_bad(v("0.2.17"), "no");
        fs::create_dir_all(shelf.root.join(".incoming-0.2.18-1")).unwrap();
        shelf.note_checked();
        shelf.reset(v("0.2.14"), false).unwrap();
        assert_eq!(on_shelf(&shelf), ["0.2.15"]);
        assert!(shelf.check_due(SystemTime::now()));
        // Not newer than installed: nothing needs it.
        shelf.reset(v("0.2.15"), false).unwrap();
        assert!(on_shelf(&shelf).is_empty());

        shelve(&shelf, "0.2.16");
        mark_healthy(&shelf.root.join("0.2.16"));
        shelf.reset(v("0.2.14"), true).unwrap();
        assert!(on_shelf(&shelf).is_empty());
        assert_eq!(Shelf::new(scratch.0.join("nowhere")).reset(v("0.2.14"), true).unwrap(), 0);
    }

    #[test]
    fn a_version_another_process_finished_first_is_kept_as_it_is() {
        let scratch = Scratch::new();
        let shelf = scratch.shelf();
        shelve(&shelf, "0.2.15");
        let binary = shelf.root.join("0.2.15").join(BINARY);
        let before = fs::metadata(&binary).unwrap().ino();
        fs::write(shelf.root.join("0.2.15").join(trying(1)), b"").unwrap();
        shelf.install(&Fake::releasing("0.2.15"), v("0.2.15"), &asset()).unwrap();
        assert_eq!(fs::metadata(&binary).unwrap().ino(), before, "never taken apart: it may be running");
        assert!(shelf.root.join("0.2.15").join(trying(1)).exists());
        assert_eq!(on_shelf(&shelf), ["0.2.15"], "and its download is gone");

        // Something that is not a finished version in the way: refused, and
        // left alone.
        let scratch = Scratch::new();
        let shelf = scratch.shelf();
        fs::create_dir_all(shelf.root.join("0.2.15")).unwrap();
        fs::write(shelf.root.join("0.2.15/note"), b"mine").unwrap();
        let failed = shelf.install(&Fake::releasing("0.2.15"), v("0.2.15"), &asset()).unwrap_err();
        assert!(matches!(failed, Failure::Disk(_)), "{failed}");
        assert_eq!(fs::read(shelf.root.join("0.2.15/note")).unwrap(), b"mine");
    }

    use std::os::unix::fs::MetadataExt;

    #[test]
    fn a_process_handed_over_to_never_hands_over_again() {
        // The guard is the variable: run the real launch path in a copy of
        // this test binary sitting in a version's folder, with the variable
        // set as the launcher sets it and a newer version beside it. The
        // copy must carry on as itself, know what is installed and which
        // folder it runs from — named through a symbolic link, as a data
        // folder moved to another disk would be — and clear the variable.
        // The marker the launcher left stays until `main` knows the launch
        // is not a window, comes back for a window, and leaves with the
        // window on the way out; only a window up long enough marks the
        // version healthy.
        const CHILD: &str = "OMACHARTS_TEST_AT_LAUNCH";
        if std::env::var_os(CHILD).is_some() {
            let exe = std::env::current_exe().unwrap();
            let folder = exe.parent().unwrap().to_path_buf();
            let marker = folder.join(trying(std::process::id()));
            fs::write(&marker, b"").unwrap();
            at_launch();
            assert!(std::env::var_os(LAUNCHER_ENV).is_none());
            let origin = origin().unwrap();
            assert_eq!(origin.installed_version, v("0.2.14"));
            assert_eq!(origin.installed, Path::new("/usr/bin/omacharts"));
            assert!(origin.handed_over());
            assert!(marker.exists(), "it may yet be a window, and crash on the way to one");
            not_a_window();
            assert!(!marker.exists(), "not a window, so not counted");
            settle();
            assert!(!folder.join(HEALTHY).exists(), "an exit proves nothing");
            window_starting();
            assert!(marker.exists(), "a window counts until it is healthy");
            settle();
            assert!(!marker.exists(), "closed before it was healthy: not a crash");
            assert!(!folder.join(HEALTHY).exists());
            window_starting();
            healthy();
            assert!(!marker.exists());
            assert!(folder.join(HEALTHY).exists());
            window_starting();
            assert!(!marker.exists(), "a healthy version is not counted");
            println!("carried on as itself");
            return;
        }
        let scratch = Scratch::new();
        let versions = scratch.0.join("real/omacharts/versions");
        let running = Version::running();
        let here = versions.join(running.to_string());
        fs::create_dir_all(&here).unwrap();
        let copy = here.join(BINARY);
        place_copy(&std::env::current_exe().unwrap(), &copy);
        shelve(&Shelf::new(versions.clone()), &Version(running.0 + 1, 0, 0).to_string());
        std::os::unix::fs::symlink(scratch.0.join("real"), scratch.0.join("linked")).unwrap();
        let told = Told {
            installed_version: v("0.2.14"),
            installed: PathBuf::from("/usr/bin/omacharts"),
            folder: Some(scratch.0.join("linked/omacharts/versions").join(running.to_string())),
        };
        let output = std::process::Command::new(scratch.0.join("linked/omacharts/versions").join(running.to_string()).join(BINARY))
            .args(["--exact", "update::tests::a_process_handed_over_to_never_hands_over_again", "--nocapture"])
            .env(CHILD, "1")
            .env("XDG_DATA_HOME", scratch.0.join("linked"))
            .env(SOURCE_ENV, "http://127.0.0.1:9")
            .env(LAUNCHER_ENV, told.encode())
            .output()
            .unwrap();
        let said = String::from_utf8_lossy(&output.stdout);
        assert!(output.status.success() && said.contains("carried on as itself"), "{said}");
    }

    /// What a migration this version got wrong fails with.
    fn its_own_failure() -> crate::migrations::Error {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        crate::migrations::Error::Sqlite(conn.execute_batch("CREATE TABL watchlists (id)").unwrap_err())
    }

    /// What the machine fails with, for `code`.
    fn the_machines_failure(code: std::ffi::c_int) -> crate::migrations::Error {
        crate::migrations::Error::Sqlite(rusqlite::Error::SqliteFailure(rusqlite::ffi::Error::new(code), None))
    }

    #[test]
    fn only_a_failure_of_the_versions_own_making_counts_against_it() {
        use rusqlite::ffi::*;
        assert!(its_own_doing(&its_own_failure()));
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE t (x NOT NULL)").unwrap();
        let constraint = conn.execute("INSERT INTO t VALUES (NULL)", []).unwrap_err();
        assert!(its_own_doing(&crate::migrations::Error::Sqlite(constraint)));
        for code in [SQLITE_FULL, SQLITE_BUSY, SQLITE_LOCKED, SQLITE_READONLY, SQLITE_CANTOPEN, SQLITE_PERM, SQLITE_IOERR, SQLITE_NOMEM, SQLITE_CORRUPT, SQLITE_NOTADB, SQLITE_AUTH] {
            assert!(!its_own_doing(&the_machines_failure(code)), "{code}");
        }
        assert!(!its_own_doing(&crate::migrations::Error::FromTheFuture { found: 11, known: 10 }));
    }

    #[test]
    fn a_window_that_could_not_open_the_settings_is_never_healthy_and_counts_only_its_own_failure() {
        // In a process of its own for each: what it records is the
        // process's, as the window's is.
        const CHILD: &str = "OMACHARTS_TEST_UNOPENED";
        if let Some(how) = std::env::var_os(CHILD) {
            let folder = std::env::current_exe().unwrap().parent().unwrap().to_path_buf();
            let marker = folder.join(trying(std::process::id()));
            fs::write(&marker, b"").unwrap();
            at_launch();
            window_starting();
            match how.to_str().unwrap() {
                "its own" => settings_unopened(Some(&its_own_failure())),
                "disk full" => settings_unopened(Some(&the_machines_failure(rusqlite::ffi::SQLITE_FULL))),
                _ => settings_unopened(None),
            }
            healthy();
            assert!(!folder.join(HEALTHY).exists(), "up, but with nothing of theirs");
            settle();
            println!("counted: {}", marker.exists());
            return;
        }
        for (how, counted) in [("its own", true), ("disk full", false), ("not copied", false)] {
            let scratch = Scratch::new();
            let versions = scratch.0.join("omacharts/versions");
            let running = Version::running();
            let here = versions.join(running.to_string());
            fs::create_dir_all(&here).unwrap();
            place_copy(&std::env::current_exe().unwrap(), &here.join(BINARY));
            let told = Told {
                installed_version: v("0.2.14"),
                installed: PathBuf::from("/usr/bin/omacharts"),
                folder: Some(here.clone()),
            };
            let output = std::process::Command::new(here.join(BINARY))
                .args([
                    "--exact",
                    "update::tests::a_window_that_could_not_open_the_settings_is_never_healthy_and_counts_only_its_own_failure",
                    "--nocapture",
                ])
                .env(CHILD, how)
                .env("XDG_DATA_HOME", &scratch.0)
                .env(SOURCE_ENV, "http://127.0.0.1:9")
                .env(LAUNCHER_ENV, told.encode())
                .output()
                .unwrap();
            let said = String::from_utf8_lossy(&output.stdout);
            assert!(output.status.success() && said.contains(&format!("counted: {counted}")), "{how}: {said}{}", String::from_utf8_lossy(&output.stderr));
            assert!(!here.join(HEALTHY).exists(), "{how}");
        }
    }

    #[test]
    fn a_version_whose_own_migration_failed_three_times_is_given_up_on_and_the_settings_put_back() {
        let scratch = Scratch::new();
        let (_, settings, shelf) = settings_and_shelf(&scratch);
        shelve(&shelf, "0.2.16");
        backup_before_first_open(&shelf.root.join("0.2.16"), &settings).unwrap();
        // Its migration got one step in, then failed: at a schema newer
        // than the package's, never opened, three launches counted.
        migrate_past(&settings, "half", KNOWN + 1);
        for pid in [4_000_000_001u32, 4_000_000_002, 4_000_000_003] {
            fs::write(shelf.root.join("0.2.16").join(trying(pid)), b"").unwrap();
        }
        assert_eq!(shelf.choose(v("0.2.14"), |_| false), None, "given up on");
        assert!(shelf.root.join("0.2.16").join(BAD).exists());
        roll_back(&shelf, &settings, v("0.2.14"), KNOWN).expect("rolled back");
        assert_eq!(setting_a(&settings).as_deref(), Some("before"));
    }

    #[test]
    fn settings_that_cannot_be_copied_first_are_not_opened_by_a_version_that_never_ran() {
        use std::os::unix::fs::PermissionsExt;
        let scratch = Scratch::new();
        let (_, settings, shelf) = settings_and_shelf(&scratch);
        // Older than this build, so opening them would migrate them.
        rusqlite::Connection::open(&settings).unwrap().pragma_update(None, "user_version", KNOWN - 1).unwrap();
        shelve(&shelf, "0.2.16");
        let folder = shelf.root.join("0.2.16");
        // Room for the lock, none for the copy: a full disk, as near as a
        // test gets without filling one.
        fs::write(shelf.root.join(LOCK), b"").unwrap();
        fs::set_permissions(&shelf.root, fs::Permissions::from_mode(0o555)).unwrap();
        let copied = backup_before_first_open(&folder, &settings);
        fs::set_permissions(&shelf.root, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(copied.is_err(), "{copied:?}");
        assert!(!shelf.root.join(".before-0.2.16").exists());
        assert_eq!(schema_of(&settings), Some(KNOWN - 1), "left as they were");
        // Next launch, with room, it is copied.
        backup_before_first_open(&folder, &settings).unwrap();
        assert!(shelf.root.join(".before-0.2.16").exists());
    }

    #[test]
    fn a_process_told_of_a_folder_it_does_not_run_from_is_not_a_download() {
        const CHILD: &str = "OMACHARTS_TEST_NOT_FROM";
        if std::env::var_os(CHILD).is_some() {
            at_launch();
            let origin = origin().unwrap();
            assert!(!origin.handed_over());
            settle();
            println!("not a download");
            return;
        }
        let scratch = Scratch::new();
        let folder = scratch.0.join("0.2.16");
        fs::create_dir_all(&folder).unwrap();
        let told = Told { installed_version: v("0.2.14"), installed: PathBuf::from("/usr/bin/omacharts"), folder: Some(folder.clone()) };
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "update::tests::a_process_told_of_a_folder_it_does_not_run_from_is_not_a_download", "--nocapture"])
            .env(CHILD, "1")
            .env(LAUNCHER_ENV, told.encode())
            .output()
            .unwrap();
        assert!(String::from_utf8_lossy(&output.stdout).contains("not a download"));
        assert!(!folder.join(HEALTHY).exists(), "nothing written to a folder it is not");
    }

    #[test]
    fn a_probed_binary_never_hands_over() {
        if std::env::var_os("OMACHARTS_TEST_PROBED").is_some() {
            at_launch();
            println!("carried on as itself");
            return;
        }
        let scratch = Scratch::new();
        shelve(&Shelf::new(scratch.0.join("omacharts/versions")), &Version(Version::running().0 + 1, 0, 0).to_string());
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "update::tests::a_probed_binary_never_hands_over", "--nocapture"])
            .env("OMACHARTS_TEST_PROBED", "1")
            .env("XDG_DATA_HOME", &scratch.0)
            .env(SOURCE_ENV, "http://127.0.0.1:9")
            .env(PROBE_ENV, "1")
            .env_remove(LAUNCHER_ENV)
            .output()
            .unwrap();
        assert!(String::from_utf8_lossy(&output.stdout).contains("carried on as itself"));
    }

    #[test]
    fn a_launch_execs_the_newer_version_with_its_arguments_and_the_guard_set() {
        const CHILD: &str = "OMACHARTS_TEST_EXEC";
        if std::env::var_os(CHILD).is_some() {
            at_launch();
            // Only reached if no hand-over happened.
            println!("carried on as itself");
            return;
        }
        let scratch = Scratch::new();
        let shelf = Shelf::new(scratch.0.join("omacharts/versions"));
        let newer = Version(Version::running().0 + 1, 0, 0);
        let dir = shelf.root.join(newer.to_string());
        fs::create_dir_all(&dir).unwrap();
        // Says what it was given, and whether the marker is there.
        let script = "#!/bin/sh\necho \"handed over: $OMACHARTS_LAUNCHER | $1 | $(ls -A \"$(dirname \"$0\")\" | tr '\\n' ' ')\"\n";
        place_executable(&dir.join(BINARY), script.as_bytes());

        let test = "update::tests::a_launch_execs_the_newer_version_with_its_arguments_and_the_guard_set";
        let run = || {
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", test, "--nocapture"])
                .env(CHILD, "1")
                .env(SOURCE_ENV, "http://127.0.0.1:9")
                .env("XDG_DATA_HOME", &scratch.0)
                .env_remove(LAUNCHER_ENV)
                .env_remove(PROBE_ENV)
                .output()
                .unwrap();
            String::from_utf8_lossy(&output.stdout).into_owned()
        };
        let running = Version::running();
        // The script never shows itself healthy and exits, three times: a
        // version that does not run. The fourth launch carries on as itself.
        for _ in 0..crate::launch::UNHEALTHY_LIMIT {
            let said = run();
            assert!(said.contains(&format!("handed over: {running}\u{1f}")), "{said}");
            assert!(said.contains(&format!("\u{1f}{}", dir.display())), "the folder is named: {said}");
            assert!(said.contains("| --exact |"), "the arguments go along: {said}");
            assert!(said.contains(".trying-"), "the marker is left for the new process to clear: {said}");
            assert!(!dir.join(BAD).exists());
        }
        let said = run();
        assert!(said.contains("carried on as itself"), "{said}");
        assert!(dir.join(BAD).exists());
    }

    #[test]
    fn a_build_that_is_not_installed_never_hands_over() {
        // The test binary is in target/, which is not /usr/bin/omacharts.
        if std::env::var_os(SOURCE_ENV).is_none() && std::env::var_os(LAUNCHER_ENV).is_none() {
            at_launch();
            assert!(origin().is_none());
        }
    }

    #[test]
    fn tidying_keeps_only_the_newest_ready_version_above_installed() {
        // (c)
        let scratch = Scratch::new();
        let shelf = scratch.shelf();
        for version in ["0.2.12", "0.2.14", "0.2.15", "0.2.16"] {
            shelve(&shelf, version);
        }
        shelf.mark_bad(v("0.2.13"), "no");
        shelf.mark_bad(v("0.2.17"), "no");
        shelf.mark_bad(v("0.2.18"), "no");
        fs::create_dir_all(shelf.root.join(".incoming-0.2.19-77")).unwrap();
        fs::create_dir_all(shelf.root.join(".incoming-0.2.19-78")).unwrap();
        shelf.tidy(v("0.2.14"), |pid| pid == 78);
        assert_eq!(on_shelf(&shelf), [".incoming-0.2.19-78", "0.2.16", "0.2.18"]);
    }

    #[test]
    fn tidying_keeps_the_newest_version_that_has_run_until_a_newer_one_has() {
        let scratch = Scratch::new();
        let shelf = scratch.shelf();
        for version in ["0.2.15", "0.2.16", "0.2.17"] {
            shelve(&shelf, version);
        }
        mark_healthy(&shelf.root.join("0.2.15"));
        mark_healthy(&shelf.root.join("0.2.16"));
        shelf.tidy(v("0.2.14"), |_| true);
        assert_eq!(on_shelf(&shelf), ["0.2.16", "0.2.17"]);
        // If 0.2.17 is given up on, 0.2.16 is still there to run.
        shelf.mark_bad(v("0.2.17"), "no");
        assert_eq!(version_of(shelf.choose(v("0.2.14"), |_| true)), Some(v("0.2.16")));
        mark_healthy(&shelf.root.join("0.2.17"));
        let _ = fs::remove_file(shelf.root.join("0.2.17").join(BAD));
        shelf.tidy(v("0.2.14"), |_| true);
        assert_eq!(on_shelf(&shelf), ["0.2.17"]);
    }

    #[test]
    fn once_pacman_is_as_new_every_download_goes() {
        // (c) at a check, offline or not.
        let scratch = Scratch::new();
        let shelf = scratch.shelf();
        shelve(&shelf, "0.2.15");
        shelve(&shelf, "0.2.16");
        let _ = check(&Offline, &shelf, v("0.2.16"), &|| v("0.2.16"), true);
        assert!(on_shelf(&shelf).is_empty(), "{:?}", on_shelf(&shelf));

        let scratch = Scratch::new();
        let shelf = scratch.shelf();
        shelve(&shelf, "0.2.15");
        let fake = Fake::releasing("0.2.15");
        assert_eq!(
            check(&fake, &shelf, v("0.2.15"), &|| v("0.2.17"), true).unwrap(),
            Checked::UpToDate { latest: v("0.2.15") }
        );
        assert!(on_shelf(&shelf).is_empty());
    }

    #[test]
    fn a_download_pacman_overtakes_while_it_runs_is_thrown_away_and_not_offered() {
        // (d)
        let scratch = Scratch::new();
        let shelf = scratch.shelf();
        let installed = Rc::new(Cell::new(v("0.2.14")));
        let mut fake = Fake::releasing("0.2.15");
        let upgraded = installed.clone();
        fake.during = Some(Box::new(move || upgraded.set(v("0.2.15"))));
        let checked = check(&fake, &shelf, v("0.2.14"), &|| installed.get(), true).unwrap();
        assert_eq!(checked, Checked::Overtaken { version: v("0.2.15") });
        assert_eq!(checked.restart_brings(v("0.2.14"), installed.get()), None);
        assert!(on_shelf(&shelf).is_empty(), "{:?}", on_shelf(&shelf));
    }

    use std::rc::Rc;

    #[test]
    fn restart_is_offered_only_for_a_version_newer_than_running_and_installed() {
        // (e)
        let ready = Checked::Ready { version: v("0.2.15"), latest: v("0.2.15") };
        assert_eq!(ready.restart_brings(v("0.2.14"), v("0.2.14")), Some(v("0.2.15")));
        assert_eq!(ready.restart_brings(v("0.2.15"), v("0.2.14")), None, "already running it");
        assert_eq!(ready.restart_brings(v("0.2.14"), v("0.2.15")), None, "pacman has it");
        assert_eq!(ready.restart_brings(v("0.2.16"), v("0.2.14")), None);
        assert_eq!(ready.restart_brings(v("0.2.14"), v("0.2.16")), None);
        for other in [
            Checked::UpToDate { latest: v("0.2.15") },
            Checked::Available { latest: v("0.2.15") },
            Checked::Publishing { latest: v("0.2.15") },
            Checked::Overtaken { version: v("0.2.15") },
        ] {
            assert_eq!(other.restart_brings(v("0.2.14"), v("0.2.14")), None);
        }
    }

    #[test]
    fn between_checks_the_shelf_is_tidied_and_offers_only_what_is_newer_than_both() {
        // (c) and (e) at a launch that asks nobody.
        let scratch = Scratch::new();
        let shelf = scratch.shelf();
        shelve(&shelf, "0.2.15");
        assert_eq!(on_the_shelf(&shelf, v("0.2.14"), v("0.2.14")), Some(v("0.2.15")));
        assert_eq!(on_the_shelf(&shelf, v("0.2.15"), v("0.2.14")), None, "running it");
        assert_eq!(on_the_shelf(&shelf, v("0.2.15"), v("0.2.15")), None, "pacman caught up");
        assert!(on_shelf(&shelf).is_empty(), "and it was deleted");
    }

    #[test]
    fn a_check_is_due_six_hours_after_the_last() {
        let scratch = Scratch::new();
        let shelf = scratch.shelf();
        let now = SystemTime::now();
        assert!(shelf.check_due(now), "never checked");
        shelf.note_checked();
        assert!(!shelf.check_due(now + Duration::from_secs(60)));
        assert!(shelf.check_due(now + CADENCE + Duration::from_secs(60)));
    }

    #[test]
    fn the_asset_is_named_for_the_architecture_alone() {
        let name = asset();
        assert!(name == "omacharts-x86_64.gz" || name == "omacharts-aarch64.gz", "{name}");
    }
}
