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
//!
//! A launcher installed today hands over to versions released long after
//! it, so what it tells them, and the files it leaves in a version's folder,
//! are a contract every later version keeps:
//!
//! - [`LAUNCHER_ENV`], `<installed version>␟<installed command>␟<folder>`,
//!   the last empty unless this launch was handed to that version's folder.
//! - `.trying-<pid>` in the folder, for a launch that has not yet shown the
//!   version healthy. The version removes every one of them once it is
//!   ([`HEALTHY`]); one whose process is gone is a launch that died first.
//! - [`HEALTHY`]: this version has run properly here once. It is never
//!   marked bad after that, whatever happens.
//! - [`BAD`]: never hand over to it and never download it again. Only for
//!   a definite reason, written inside it.

use std::ffi::OsString;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Set on the app by whatever started it as the installed binary; see the
/// module's documentation. Also what makes one hand-over the last: a process
/// that finds it set never hands over again.
pub const LAUNCHER_ENV: &str = "OMACHARTS_LAUNCHER";

/// Set to ask a binary its `--version` without it handing over to anything.
pub const PROBE_ENV: &str = "OMACHARTS_PROBE";

/// Points the updater at another server, for a demo or a manual test: a
/// base URL laid out like a GitHub repository's releases. It also lets a
/// binary that is not the installed one take part, which is the point —
/// nothing else makes a build from a working tree download or hand over.
pub const SOURCE_ENV: &str = "OMACHARTS_UPDATE_SOURCE";

/// Set to anything but `0` to run the installed version and nothing else:
/// no hand-over, no download. The way out when a download misbehaves.
pub const NO_UPDATE_ENV: &str = "OMACHARTS_NO_UPDATE";

/// The binary inside a version's folder.
pub const BINARY: &str = "omacharts";
/// A version never to be run or downloaded again, and why.
pub const BAD: &str = ".bad";
/// A version that has run properly here at least once.
pub const HEALTHY: &str = ".healthy";

/// Launches that end before the version shows itself healthy, in a row,
/// before it is given up on.
pub const UNHEALTHY_LIMIT: usize = 3;

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

/// Is [`NO_UPDATE_ENV`] set?
pub fn updates_off() -> bool {
    std::env::var_os(NO_UPDATE_ENV).is_some_and(|value| !value.is_empty() && value != "0")
}

/// Running as root, as `sudo -E` would: nothing a user downloaded is run or
/// written then. Asked of `/proc/self`, which belongs to the effective user.
pub fn as_root() -> bool {
    use std::os::unix::fs::MetadataExt;
    fs::metadata("/proc/self").is_ok_and(|meta| meta.uid() == 0)
}

/// What [`LAUNCHER_ENV`] says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Told {
    pub installed_version: Version,
    pub installed: PathBuf,
    /// The version folder this launch was handed to, if any.
    pub folder: Option<PathBuf>,
}

impl Told {
    pub fn encode(&self) -> String {
        let folder = self.folder.as_deref().map(|folder| folder.display().to_string()).unwrap_or_default();
        format!("{}\u{1f}{}\u{1f}{folder}", self.installed_version, self.installed.display())
    }

    pub fn decode(text: &str) -> Option<Told> {
        let mut fields = text.split('\u{1f}');
        let installed_version = Version::parse(fields.next()?)?;
        let installed = PathBuf::from(fields.next()?);
        let folder = fields.next().filter(|folder| !folder.is_empty()).map(PathBuf::from);
        Some(Told { installed_version, installed, folder })
    }

    /// Put it in this process's environment, for the process it becomes.
    pub fn tell(&self) {
        // SAFETY: called before any other thread exists, or just before an
        // exec that replaces them all.
        unsafe { std::env::set_var(LAUNCHER_ENV, self.encode()) };
    }
}

/// The `versions` directory.
pub struct Shelf {
    pub root: PathBuf,
}

/// A version [`Shelf::choose`] picked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Chosen {
    pub version: Version,
    pub folder: PathBuf,
    pub binary: PathBuf,
    pub healthy: bool,
}

impl Shelf {
    pub fn new(root: PathBuf) -> Shelf {
        Shelf { root }
    }

    /// The newest version above `floor` that can be handed over to. One that
    /// has never been healthy and has had [`UNHEALTHY_LIMIT`] launches end
    /// before it was is marked bad here, and the next one down is tried.
    ///
    /// The launch path: one `read_dir`, and only when something newer is
    /// there, a look inside it.
    pub fn choose(&self, floor: Version, alive: impl Fn(u32) -> bool) -> Option<Chosen> {
        let mut above: Vec<Version> = fs::read_dir(&self.root)
            .ok()?
            .filter_map(|entry| Version::parse(entry.ok()?.file_name().to_str()?))
            .filter(|version| *version > floor)
            .collect();
        above.sort_unstable_by_key(|version| std::cmp::Reverse(*version));
        for version in above {
            let folder = self.root.join(version.to_string());
            let binary = folder.join(BINARY);
            if folder.join(BAD).exists() || !binary.is_file() {
                continue;
            }
            let healthy = folder.join(HEALTHY).exists();
            if !healthy {
                let ended = ended_launches(&folder, &alive);
                if ended >= UNHEALTHY_LIMIT {
                    self.mark_bad(version, &format!("{ended} launches in a row ended before it ran properly"));
                    continue;
                }
            }
            return Some(Chosen { version, folder, binary, healthy });
        }
        None
    }

    /// Never hand over to `version` again, and never download it again,
    /// for the reason given. Not for a version that has been healthy here.
    pub fn mark_bad(&self, version: Version, why: &str) {
        let folder = self.root.join(version.to_string());
        if folder.join(HEALTHY).exists() {
            return;
        }
        if fs::create_dir_all(&folder).is_ok() {
            let _ = fs::write(folder.join(BAD), format!("{why}\n"));
        }
    }
}

/// How many `.trying-` markers in `folder` belong to processes that are gone.
pub fn ended_launches(folder: &Path, alive: impl Fn(u32) -> bool) -> usize {
    fs::read_dir(folder)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| {
            entry
                .file_name()
                .to_str()
                .and_then(|name| name.strip_prefix(".trying-"))
                .and_then(|pid| pid.parse::<u32>().ok())
                .is_some_and(|pid| !alive(pid))
        })
        .count()
}

/// The marker a launch leaves while it has not shown its version healthy.
pub fn trying(pid: u32) -> String {
    format!(".trying-{pid}")
}

/// Is process `pid` still running?
pub fn alive(pid: u32) -> bool {
    Path::new("/proc").join(pid.to_string()).exists()
}

/// What an exec that came back means for the version it was meant to start.
#[derive(Debug, PartialEq, Eq)]
pub enum Refused {
    /// It cannot start on this machine, and will not next time either.
    Definitely(String),
    /// The binary is gone: tidied away between choosing it and starting it.
    Vanished,
    /// Something about this moment — memory, a busy file, permissions.
    ForNow(String),
}

pub fn refused(error: &io::Error, binary: &Path) -> Refused {
    match error.raw_os_error() {
        // ENOEXEC, ELIBBAD: not an executable this kernel can run.
        Some(8) | Some(80) => Refused::Definitely(format!("not an executable this machine runs ({error})")),
        // ENOENT for a file that is there is its interpreter that is not.
        Some(2) if binary.is_file() => Refused::Definitely(format!("its program interpreter is missing ({error})")),
        Some(2) => Refused::Vanished,
        _ => Refused::ForNow(error.to_string()),
    }
}

/// Launch as the installed binary at `exe`, version `installed`: tell what
/// is started so, and if the shelf has a newer version, become it.
///
/// Returns only when nothing newer was there, when updates are switched off
/// or this runs as root, or when the exec failed. A definite failure marks
/// the version bad, unless it has been healthy here; a vanished binary means
/// choosing again; anything else is this launch's problem only. Whichever,
/// the caller carries on with its own binary, and [`LAUNCHER_ENV`] says so.
pub fn hand_over_if_newer(exe: &Path, installed: Version) {
    let mut told = Told { installed_version: installed, installed: exe.to_path_buf(), folder: None };
    told.tell();
    if updates_off() || as_root() {
        return;
    }
    let shelf = Shelf::new(shelf_dir());
    for _ in 0..4 {
        let Some(chosen) = shelf.choose(installed, alive) else { return };
        let marker = chosen.folder.join(trying(std::process::id()));
        let _ = fs::write(&marker, b"");
        told.folder = Some(chosen.folder.clone());
        told.tell();
        let mut failed = exec(&chosen.binary);
        // A binary just put in place can be busy for a moment.
        for _ in 0..5 {
            if failed.raw_os_error() != Some(26) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
            failed = exec(&chosen.binary);
        }
        // Still here: the exec failed.
        let _ = fs::remove_file(&marker);
        told.folder = None;
        told.tell();
        match refused(&failed, &chosen.binary) {
            Refused::Vanished => continue,
            Refused::Definitely(why) => {
                eprintln!("omacharts: {} could not be started: {why}", chosen.version);
                if chosen.healthy {
                    return;
                }
                shelf.mark_bad(chosen.version, &why);
            }
            Refused::ForNow(why) => {
                eprintln!("omacharts: {} could not be started this time ({why}); carrying on with {installed}", chosen.version);
                return;
            }
        }
    }
}

/// Become `binary`, with this process's arguments and environment.
///
/// With `execv` itself rather than `Command::exec`, which goes through
/// `execvp`: given a file the kernel will not run, that hands it to
/// `/bin/sh` as a script instead of saying so, and a download that is not
/// an executable would be run as one, line by line, rather than marked bad.
pub fn exec(binary: &Path) -> io::Error {
    use std::ffi::{c_char, c_int, CString};
    use std::os::unix::ffi::OsStrExt;

    unsafe extern "C" {
        fn execv(path: *const c_char, argv: *const *const c_char) -> c_int;
        fn signal(signum: c_int, handler: usize) -> usize;
    }
    const SIGPIPE: c_int = 13;
    const SIG_DFL: usize = 0;

    let invalid = || io::Error::from(io::ErrorKind::InvalidInput);
    let Ok(path) = CString::new(binary.as_os_str().as_bytes()) else { return invalid() };
    let mut args: Vec<OsString> = std::env::args_os().collect();
    if args.is_empty() {
        args.push(OsString::from(BINARY));
    }
    let Ok(args) = args.iter().map(|arg| CString::new(arg.as_bytes())).collect::<Result<Vec<_>, _>>() else {
        return invalid();
    };
    let mut argv: Vec<*const c_char> = args.iter().map(|arg| arg.as_ptr()).collect();
    argv.push(std::ptr::null());
    // SAFETY: both are NUL-terminated and outlive the call, which returns
    // only on failure. SIGPIPE goes back to its default first, as
    // `Command` does: the Rust runtime ignores it, and an ignored signal
    // would stay ignored in the binary this becomes.
    unsafe {
        let previous = signal(SIGPIPE, SIG_DFL);
        execv(path.as_ptr(), argv.as_ptr());
        let error = io::Error::last_os_error();
        signal(SIGPIPE, previous);
        error
    }
}
