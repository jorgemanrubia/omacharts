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
//!   0.2.15/.trying-4242        a hand-over in flight, by its process id
//!   0.2.15/.bad                handed over to and never started: skipped
//!   .incoming-0.2.16-4300/     a download in progress, or one that died
//!   .checked                   its modification time is the last check
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
//! - **A version that does not start is never tried again.** The launcher
//!   leaves a marker named after its process id before the `exec`, and the
//!   version it starts removes it first thing. A marker whose process is gone
//!   is a version that died before reaching `main`, and it is marked bad.
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

pub use crate::launch::{alive, shelf_dir, Shelf, Version, SOURCE_ENV};
use crate::launch::{trying, BAD, BINARY, LAUNCHER_ENV, PROBE_ENV};

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
    /// Whether this process is a downloaded version, handed over to.
    pub handed_over: bool,
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
    let shelf_root = shelf_dir();
    if let Some(from) = std::env::var_os(LAUNCHER_ENV) {
        // SAFETY: first thing in `main`, before any other thread exists.
        unsafe { std::env::remove_var(LAUNCHER_ENV) };
        let from = from.to_string_lossy();
        let handed_over = exe.as_deref().is_some_and(|exe| exe.starts_with(&shelf_root));
        if let Some((version, path)) = from.split_once('\u{1f}')
            && let Some(installed_version) = Version::parse(version)
        {
            let _ = ORIGIN.set(Origin { installed: PathBuf::from(path), installed_version, handed_over });
        }
        if handed_over && let Some(dir) = exe.as_deref().and_then(Path::parent) {
            let _ = fs::remove_file(dir.join(trying(std::process::id())));
        }
        return;
    }
    if std::env::var_os(PROBE_ENV).is_some() {
        return;
    }
    let Some(exe) = exe else { return };
    let source_given = std::env::var_os(SOURCE_ENV).is_some_and(|v| !v.is_empty());
    if (exe != Path::new(INSTALLED_AT) && !source_given) || exe.starts_with(&shelf_root) {
        return;
    }
    let running = Version::running();
    crate::launch::hand_over_if_newer(&exe, running);
    // Nothing newer, or it would not start: this binary is the one.
    unsafe { std::env::remove_var(LAUNCHER_ENV) };
    let _ = ORIGIN.set(Origin { installed: exe, installed_version: running, handed_over: false });
}

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
                Some(Found { version, ready: dir.join(BINARY).is_file() && !dir.join(BAD).exists() })
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
    /// - every ready version older than the newest ready one;
    /// - every bad marker but the newest, and that one too unless it is newer
    ///   than the newest ready version, which is the only thing it guards;
    /// - every download in progress whose process is gone.
    pub fn tidy(&self, installed: Version, alive: impl Fn(u32) -> bool) -> usize {
        let mut gone = 0;
        let newest_ready = self.newest_ready();
        let mut kept_bad = false;
        for found in self.found() {
            let keep = found.version > installed
                && if found.ready {
                    Some(found.version) == newest_ready
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
            if target.join(BINARY).is_file() {
                // Another process put the same version in place first.
                return Ok(());
            }
            let _ = fs::remove_dir_all(&target);
            fs::rename(&incoming, &target).map_err(Failure::Disk)?;
            if let Ok(dir) = fs::File::open(&self.root) {
                let _ = dir.sync_all();
            }
            Ok(())
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
            Some(said) if said == version => Ok(()),
            Some(said) => Err(Failure::Broken(format!("{asset} says it is {said}, not {version}"))),
            None => Err(Failure::Broken(format!("{version} does not start here"))),
        }
    }

    /// Remove every download.
    pub fn clear(&self) -> io::Result<()> {
        match fs::remove_dir_all(&self.root) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
            _ => Ok(()),
        }
    }
}

/// A release asset is a binary of some tens of megabytes; anything far
/// larger is not one, and is not going to fill the disk finding out.
const MAX_DOWNLOAD: u64 = 256 * 1024 * 1024;
const MAX_BINARY: u64 = 512 * 1024 * 1024;

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Ask `binary` its version, without letting it hand over or reach the
/// window, and without waiting on it for longer than `patience`.
fn probe(binary: &Path, patience: Duration) -> Option<Version> {
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
    let mut child = None;
    for _ in 0..10 {
        match spawn() {
            Ok(started) => {
                child = Some(started);
                break;
            }
            Err(error) if error.raw_os_error() == Some(26) => std::thread::sleep(Duration::from_millis(50)),
            Err(_) => return None,
        }
    }
    let mut child = child?;
    let deadline = std::time::Instant::now() + patience;
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => break,
            Ok(None) if std::time::Instant::now() < deadline => std::thread::sleep(Duration::from_millis(10)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
    let mut said = String::new();
    child.stdout.take()?.read_to_string(&mut said).ok()?;
    Version::parse(said.trim().strip_prefix("omacharts")?)
}

// -- where releases come from ---------------------------------------------

/// Something that publishes releases: GitHub, or a test's fake.
pub trait Source {
    /// The newest release's version.
    fn latest(&self) -> Result<Version, Failure>;
    /// One of `version`'s assets, to read.
    fn open(&self, version: Version, asset: &str) -> Result<Box<dyn Read>, Failure>;
}

/// Why a check or a download did not finish.
#[derive(Debug)]
pub enum Failure {
    /// Not reachable, or not answering as expected. Usually just offline.
    Network(String),
    /// The download is not what was published.
    Mismatch { asset: String },
    /// Verified, but does not unpack or does not start.
    Broken(String),
    /// Out of space, or not writable.
    Disk(io::Error),
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Failure::Network(why) => write!(f, "releases not reachable: {why}"),
            Failure::Mismatch { asset } => write!(f, "{asset} does not match its published checksum; discarded"),
            Failure::Broken(why) => write!(f, "{why}; discarded"),
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
}

// -- a check --------------------------------------------------------------

/// What a check found.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Checked {
    /// Nothing newer than what is installed, downloaded or running.
    UpToDate { latest: Version },
    /// `latest` is newer, and was not downloaded because nobody asked.
    Available { latest: Version },
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
    let latest = source.latest()?;
    shelf.note_checked();

    let ready = shelf.newest_ready();
    let floor = [Some(before), Some(running), shelf.newest_known()].into_iter().flatten().max().unwrap_or(before);
    if latest <= floor {
        return Ok(match ready {
            Some(version) if version > before => Checked::Ready { version, latest },
            _ => Checked::UpToDate { latest },
        });
    }
    if !download {
        return Ok(Checked::Available { latest });
    }

    match shelf.install(source, latest, &asset()) {
        Ok(()) => {}
        // Verified and still no good: never again, until a newer release.
        Err(failure @ Failure::Broken(_)) => {
            shelf.mark_bad(latest);
            return Err(failure);
        }
        Err(failure) => return Err(failure),
    }

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
        fs::write(dir.join(BINARY), script_saying(version)).unwrap();
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(dir.join(BINARY), fs::Permissions::from_mode(0o755)).unwrap();
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
    }

    struct Offline;
    impl Source for Offline {
        fn latest(&self) -> Result<Version, Failure> {
            Err(Failure::Network("offline".into()))
        }
        fn open(&self, _: Version, _: &str) -> Result<Box<dyn Read>, Failure> {
            Err(Failure::Network("offline".into()))
        }
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
        assert_eq!(probe(&shelf.root.join("0.2.15/omacharts"), Duration::from_secs(5)), Some(v("0.2.15")));
        assert_eq!(checked.restart_brings(v("0.2.14"), v("0.2.14")), Some(v("0.2.15")));
    }

    #[test]
    fn a_download_that_does_not_match_its_checksum_is_discarded() {
        let scratch = Scratch::new();
        let shelf = scratch.shelf();
        let mut fake = Fake::releasing("0.2.15");
        fake.assets.insert(asset(), gzip(b"#!/bin/sh\necho tampered\n"));
        let failed = check(&fake, &shelf, v("0.2.14"), &|| v("0.2.14"), true).unwrap_err();
        assert!(matches!(failed, Failure::Mismatch { .. }), "{failed}");
        assert!(on_shelf(&shelf).is_empty(), "nothing kept: {:?}", on_shelf(&shelf));
        assert_eq!(shelf.choose(v("0.2.14"), |_| true), None);
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
    fn a_verified_binary_that_says_another_version_or_does_not_start_is_never_tried_again() {
        let scratch = Scratch::new();
        let shelf = scratch.shelf();
        let fake = Fake::with_binary("0.2.15", &script_saying("0.2.13"));
        assert!(matches!(check(&fake, &shelf, v("0.2.14"), &|| v("0.2.14"), true), Err(Failure::Broken(_))));
        assert_eq!(shelf.choose(v("0.2.14"), |_| true), None);
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
        assert!(matches!(check(&fake, &shelf, v("0.2.14"), &|| v("0.2.14"), true), Err(Failure::Broken(_))));
        assert_eq!(shelf.choose(v("0.2.14"), |_| true), None);
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
        let (version, binary) = shelf.choose(v("0.2.14"), |_| true).unwrap();
        assert_eq!(version, v("0.2.16"));
        assert_eq!(binary, shelf.root.join("0.2.16/omacharts"));

        shelf.mark_bad(v("0.2.16"));
        assert_eq!(shelf.choose(v("0.2.14"), |_| true).map(|(v, _)| v), Some(v("0.2.15")));
        assert_eq!(shelf.choose(v("0.2.16"), |_| true), None);
    }

    #[test]
    fn a_hand_over_that_never_reached_main_marks_its_version_bad() {
        let scratch = Scratch::new();
        let shelf = scratch.shelf();
        shelve(&shelf, "0.2.15");
        shelve(&shelf, "0.2.16");
        fs::write(shelf.root.join("0.2.16").join(trying(4242)), b"").unwrap();

        // Still alive: a launch in flight, not a failure.
        assert_eq!(shelf.choose(v("0.2.14"), |_| true).map(|(v, _)| v), Some(v("0.2.16")));
        // Gone with its marker still there: it died before reaching `main`.
        assert_eq!(shelf.choose(v("0.2.14"), |pid| pid != 4242).map(|(v, _)| v), Some(v("0.2.15")));
        assert!(shelf.root.join("0.2.16").join(BAD).exists());
        // And it stays skipped once the marker is gone.
        let _ = fs::remove_file(shelf.root.join("0.2.16").join(trying(4242)));
        assert_eq!(shelf.choose(v("0.2.14"), |_| true).map(|(v, _)| v), Some(v("0.2.15")));
    }

    #[test]
    fn a_process_handed_over_to_never_hands_over_again() {
        // The guard is the variable: run the real launch path in a copy of
        // this test binary sitting in a version's folder, with the variable
        // set as the launcher sets it and a newer version beside it. The
        // copy must carry on as itself, know what is installed, clear the
        // variable, and remove the marker the launcher left for it.
        const CHILD: &str = "OMACHARTS_TEST_AT_LAUNCH";
        if std::env::var_os(CHILD).is_some() {
            let exe = std::env::current_exe().unwrap();
            let marker = exe.parent().unwrap().join(trying(std::process::id()));
            fs::write(&marker, b"").unwrap();
            at_launch();
            assert!(std::env::var_os(LAUNCHER_ENV).is_none());
            let origin = origin().unwrap();
            assert_eq!(origin.installed_version, v("0.2.14"));
            assert_eq!(origin.installed, Path::new("/usr/bin/omacharts"));
            assert!(origin.handed_over);
            assert!(!marker.exists());
            println!("carried on as itself");
            return;
        }
        let scratch = Scratch::new();
        let versions = scratch.0.join("omacharts/versions");
        let running = Version::running();
        let here = versions.join(running.to_string());
        fs::create_dir_all(&here).unwrap();
        let copy = here.join(BINARY);
        fs::copy(std::env::current_exe().unwrap(), &copy).unwrap();
        shelve(&Shelf::new(versions.clone()), &Version(running.0 + 1, 0, 0).to_string());
        let output = std::process::Command::new(&copy)
            .args(["--exact", "update::tests::a_process_handed_over_to_never_hands_over_again", "--nocapture"])
            .env(CHILD, "1")
            .env("XDG_DATA_HOME", &scratch.0)
            .env(SOURCE_ENV, "http://127.0.0.1:9")
            .env(LAUNCHER_ENV, "0.2.14\u{1f}/usr/bin/omacharts")
            .output()
            .unwrap();
        let said = String::from_utf8_lossy(&output.stdout);
        assert!(output.status.success() && said.contains("carried on as itself"), "{said}");
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
        fs::write(dir.join(BINARY), script).unwrap();
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(dir.join(BINARY), fs::Permissions::from_mode(0o755)).unwrap();

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
        let said = run();
        let running = Version::running();
        assert!(said.contains(&format!("handed over: {running}\u{1f}")), "{said}");
        assert!(said.contains("| --exact |"), "the arguments go along: {said}");
        assert!(said.contains(".trying-"), "the marker is left for the new process to clear: {said}");

        // The script never cleared its marker and has exited, so it is a
        // version that did not start: the next launch skips it.
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
        shelf.mark_bad(v("0.2.13"));
        shelf.mark_bad(v("0.2.17"));
        shelf.mark_bad(v("0.2.18"));
        fs::create_dir_all(shelf.root.join(".incoming-0.2.19-77")).unwrap();
        fs::create_dir_all(shelf.root.join(".incoming-0.2.19-78")).unwrap();
        shelf.tidy(v("0.2.14"), |pid| pid == 78);
        assert_eq!(on_shelf(&shelf), [".incoming-0.2.19-78", "0.2.16", "0.2.18"]);
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
