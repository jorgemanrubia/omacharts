//! Which binary a launch runs: the one pacman installed, or a newer one the
//! updater downloaded. The part of [`crate::update`] that runs before
//! anything else does, written against `std` alone.
//!
//! `std` alone because it is compiled twice. Into the app, and into
//! `omacharts-launch`, the small binary the package installs as
//! `/usr/bin/omacharts` with the app itself beside it in
//! `/usr/lib/omacharts/`. The app links GTK, and on this kind of machine
//! loading GTK's hundred-odd libraries costs some ten milliseconds before
//! `main` is reached — measured with an empty C program linked to libgtk-4.
//! An app that handed itself over with an `exec` would pay that twice on
//! every launch that has a download, and with Omarchy's repository a release
//! or two behind GitHub's, that is most of them. The launcher links nothing,
//! starts in a fraction of a millisecond, and execs whichever binary is
//! newest; GTK is loaded once, by the binary that keeps the window.

use std::ffi::OsString;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Set on the app by whatever started it as the installed binary — the
/// launcher, or an app that is itself at `/usr/bin/omacharts` — as
/// `<version>\x1f<path>`: what is installed, which the process it started
/// has no other way to know. Also what makes one hand-over the last: a
/// process that finds it set never hands over again.
pub const LAUNCHER_ENV: &str = "OMACHARTS_LAUNCHER";

/// Set to ask a binary its `--version` without it handing over to anything.
pub const PROBE_ENV: &str = "OMACHARTS_PROBE";

/// Points the updater at another server, for a demo or a manual test: a
/// base URL laid out like a GitHub repository's releases. It also lets a
/// binary that is not the installed one take part, which is the point —
/// nothing else makes a build from a working tree download or hand over.
pub const SOURCE_ENV: &str = "OMACHARTS_UPDATE_SOURCE";

/// The binary inside a version's folder.
pub const BINARY: &str = "omacharts";
/// A version that was handed over to and never started.
pub const BAD: &str = ".bad";

/// A release's version, `major.minor.patch`.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Hash)]
pub struct Version(pub u32, pub u32, pub u32);

impl Version {
    /// `0.2.15` or `v0.2.15`; anything else is not a version.
    pub fn parse(text: &str) -> Option<Version> {
        let text = text.trim();
        let text = text.strip_prefix('v').unwrap_or(text);
        let mut parts = text.split('.');
        let mut next = || -> Option<u32> {
            let part = parts.next()?;
            if part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            part.parse().ok()
        };
        let version = Version(next()?, next()?, next()?);
        parts.next().is_none().then_some(version)
    }

    /// The version this was built as. The launcher and the app come out of
    /// one package, so it is the same number in both.
    pub fn running() -> Version {
        Version::parse(env!("CARGO_PKG_VERSION")).unwrap_or(Version(0, 0, 0))
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.0, self.1, self.2)
    }
}

/// Where downloaded versions live: `versions/` in the data folder, which is
/// `$XDG_DATA_HOME/omacharts` or `~/.local/share/omacharts` as for the store.
pub fn shelf_dir() -> PathBuf {
    let data = match std::env::var_os("XDG_DATA_HOME").filter(|v| !v.is_empty()) {
        Some(base) => PathBuf::from(base).join("omacharts"),
        None => std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default().join(".local/share/omacharts"),
    };
    data.join("versions")
}

/// The `versions` directory.
pub struct Shelf {
    pub root: PathBuf,
}

impl Shelf {
    pub fn new(root: PathBuf) -> Shelf {
        Shelf { root }
    }

    /// The newest version above `floor` that can be handed over to, and its
    /// binary. A version whose last hand-over died before reaching `main` is
    /// marked bad here, and the next one down is tried.
    ///
    /// The launch path: one `read_dir`, and only when something newer is
    /// there, a look inside it.
    pub fn choose(&self, floor: Version, alive: impl Fn(u32) -> bool) -> Option<(Version, PathBuf)> {
        let mut above: Vec<Version> = fs::read_dir(&self.root)
            .ok()?
            .filter_map(|entry| Version::parse(entry.ok()?.file_name().to_str()?))
            .filter(|version| *version > floor)
            .collect();
        above.sort_unstable_by_key(|version| std::cmp::Reverse(*version));
        for version in above {
            let dir = self.root.join(version.to_string());
            let binary = dir.join(BINARY);
            if dir.join(BAD).exists() || !binary.is_file() {
                continue;
            }
            let died = fs::read_dir(&dir).into_iter().flatten().flatten().any(|entry| {
                entry
                    .file_name()
                    .to_str()
                    .and_then(|name| name.strip_prefix(".trying-"))
                    .and_then(|pid| pid.parse::<u32>().ok())
                    .is_some_and(|pid| !alive(pid))
            });
            if died {
                self.mark_bad(version);
                continue;
            }
            return Some((version, binary));
        }
        None
    }

    /// Never hand over to `version` again, and never download it again.
    pub fn mark_bad(&self, version: Version) {
        let dir = self.root.join(version.to_string());
        if fs::create_dir_all(&dir).is_ok() {
            let _ = fs::write(dir.join(BAD), b"");
        }
    }
}

/// The marker a hand-over leaves while it is in flight, by process id.
pub fn trying(pid: u32) -> String {
    format!(".trying-{pid}")
}

/// Is process `pid` still running?
pub fn alive(pid: u32) -> bool {
    Path::new("/proc").join(pid.to_string()).exists()
}

/// Launch as the installed binary at `exe`, version `installed`: tell what
/// is started so, and if the shelf has a newer version, become it.
///
/// Returns only when nothing newer was there, or when the exec failed — in
/// which case that version is marked bad and never tried again. Either way
/// the caller carries on with its own binary, and [`LAUNCHER_ENV`] is set
/// for it.
pub fn hand_over_if_newer(exe: &Path, installed: Version) {
    // SAFETY: called first thing in `main`, before any other thread exists.
    unsafe { std::env::set_var(LAUNCHER_ENV, format!("{installed}\u{1f}{}", exe.display())) };
    let shelf = Shelf::new(shelf_dir());
    let Some((version, binary)) = shelf.choose(installed, alive) else { return };
    let marker = shelf.root.join(version.to_string()).join(trying(std::process::id()));
    let _ = fs::write(&marker, b"");
    let failed = exec(&binary);
    // Still here: the exec itself failed, so this version cannot start.
    let _ = fs::remove_file(&marker);
    shelf.mark_bad(version);
    eprintln!("omacharts: {version} could not be started ({failed}); carrying on with {installed}");
}

/// Become `binary`, with this process's arguments and environment.
pub fn exec(binary: &Path) -> io::Error {
    use std::os::unix::process::CommandExt;
    let mut args = std::env::args_os();
    let zeroth = args.next().unwrap_or_else(|| OsString::from("omacharts"));
    std::process::Command::new(binary).arg0(zeroth).args(args).exec()
}
