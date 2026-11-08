//! The launcher as the package installs it: `bin/omacharts` in a prefix with
//! the app at `lib/omacharts/omacharts`, run as a process. The apps here are
//! shell scripts that say who they are, so nothing starts GTK.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, MutexGuard};

const INSTALLED: &str = env!("CARGO_PKG_VERSION");

/// One test at a time. A file written on one thread while another forks to
/// run something is still open for writing in that child until it execs, and
/// running the file in that moment fails with "text file busy" — here, or in
/// the launcher, which would take the version for broken.
static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

struct Prefix {
    root: PathBuf,
    _turn: MutexGuard<'static, ()>,
}

impl Prefix {
    fn new(name: &str) -> Prefix {
        let turn = ONE_AT_A_TIME.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let root = std::env::temp_dir().join(format!("omacharts-launcher-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("bin")).unwrap();
        fs::copy(env!("CARGO_BIN_EXE_omacharts-launch"), root.join("bin/omacharts")).unwrap();
        let prefix = Prefix { root, _turn: turn };
        prefix.script(&prefix.root.join("lib/omacharts/omacharts"), "packaged");
        prefix
    }

    fn launcher(&self) -> PathBuf {
        self.root.join("bin/omacharts")
    }

    fn versions(&self) -> PathBuf {
        self.root.join("data/omacharts/versions")
    }

    /// An app that prints its name, what the launcher told it, and its
    /// arguments.
    fn script(&self, path: &Path, name: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, format!("#!/bin/sh\necho \"{name} | $OMACHARTS_LAUNCHER | $*\"\n")).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    }

    fn shelve(&self, version: &str) -> PathBuf {
        let binary = self.versions().join(version).join("omacharts");
        self.script(&binary, version);
        binary
    }

    fn run(&self, args: &[&str]) -> (String, u32) {
        self.run_with(args, None)
    }

    /// With `OMACHARTS_NO_UPDATE` set to `no_update`, or unset.
    fn run_with(&self, args: &[&str], no_update: Option<&str>) -> (String, u32) {
        let mut command = Command::new(self.launcher());
        command.env_remove("OMACHARTS_NO_UPDATE");
        if let Some(value) = no_update {
            command.env("OMACHARTS_NO_UPDATE", value);
        }
        let child = command
            .args(args)
            .env("XDG_DATA_HOME", self.root.join("data"))
            .env_remove("OMACHARTS_LAUNCHER")
            .env_remove("OMACHARTS_PROBE")
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let pid = child.id();
        let output = child.wait_with_output().unwrap();
        (String::from_utf8_lossy(&output.stdout).trim().to_string(), pid)
    }
}

impl Drop for Prefix {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn newer(by: u32) -> String {
    let mut parts = INSTALLED.split('.').map(|part| part.parse::<u32>().unwrap());
    let (major, minor, patch) = (parts.next().unwrap(), parts.next().unwrap(), parts.next().unwrap());
    format!("{major}.{minor}.{}", patch + by)
}

#[test]
fn with_nothing_downloaded_it_runs_the_packaged_app_with_every_argument() {
    let prefix = Prefix::new("plain");
    let (said, _) = prefix.run(&["chart", "open", "two words"]);
    // No folder: the packaged app is not a download.
    let told = format!("{INSTALLED}\u{1f}{}\u{1f}", prefix.launcher().display());
    assert_eq!(said, format!("packaged | {told} | chart open two words"));
}

#[test]
fn it_runs_a_newer_download_names_its_folder_and_leaves_it_a_marker() {
    let prefix = Prefix::new("newer");
    prefix.shelve(&newer(1));
    let (said, pid) = prefix.run(&["--version"]);
    let folder = prefix.versions().join(newer(1));
    let told = format!("{INSTALLED}\u{1f}{}\u{1f}{}", prefix.launcher().display(), folder.display());
    assert_eq!(said, format!("{} | {told} | --version", newer(1)));
    // The app clears it once it has shown it runs; a script does not, which
    // is how later launches know this version keeps ending first.
    assert!(folder.join(format!(".trying-{pid}")).exists());
}

#[test]
fn with_updates_switched_off_it_runs_the_packaged_app() {
    let prefix = Prefix::new("off");
    prefix.shelve(&newer(1));
    let (said, _) = prefix.run_with(&[], Some("1"));
    assert!(said.starts_with("packaged |"), "{said}");
    let (said, _) = prefix.run_with(&[], Some("0"));
    assert!(said.starts_with(&format!("{} |", newer(1))), "0 is not off: {said}");
}

#[test]
fn a_download_no_newer_than_the_package_is_never_run() {
    let prefix = Prefix::new("same");
    prefix.shelve(INSTALLED);
    let (said, _) = prefix.run(&[]);
    assert!(said.starts_with("packaged |"), "{said}");
}

/// Markers from launches long gone.
fn ended(folder: &Path, launches: u32) {
    for pid in 0..launches {
        fs::write(folder.join(format!(".trying-{}", 4_000_000_000 + pid)), b"").unwrap();
    }
}

#[test]
fn a_download_whose_launches_keep_ending_first_is_passed_over_for_good() {
    let prefix = Prefix::new("died");
    prefix.shelve(&newer(1));
    let folder = prefix.versions().join(newer(1));
    // Two may be anything.
    ended(&folder, 2);
    let (said, _) = prefix.run(&[]);
    assert!(said.starts_with(&format!("{} |", newer(1))), "{said}");
    // That one ended too, and made three.
    let (said, _) = prefix.run(&[]);
    assert!(said.starts_with("packaged |"), "{said}");
    assert!(folder.join(".bad").exists());
}

#[test]
fn a_download_that_has_run_is_never_passed_over() {
    let prefix = Prefix::new("healthy");
    prefix.shelve(&newer(1));
    let folder = prefix.versions().join(newer(1));
    fs::write(folder.join(".healthy"), b"").unwrap();
    ended(&folder, 10);
    let (said, _) = prefix.run(&[]);
    assert!(said.starts_with(&format!("{} |", newer(1))), "{said}");
    assert!(!folder.join(".bad").exists());
}

#[test]
fn a_download_this_machine_cannot_execute_is_marked_bad_and_the_next_one_down_runs() {
    let prefix = Prefix::new("broken");
    let binary = prefix.shelve(&newer(3));
    fs::write(&binary, b"\x7fELF but not really").unwrap();
    let missing = prefix.shelve(&newer(2));
    fs::write(&missing, "#!/nowhere/sh\necho never\n").unwrap();
    prefix.shelve(&newer(1));
    let (said, _) = prefix.run(&[]);
    assert!(said.starts_with(&format!("{} |", newer(1))), "{said}");
    let why = fs::read_to_string(prefix.versions().join(newer(3)).join(".bad")).unwrap();
    assert!(why.contains("not an executable"), "{why}");
    let why = fs::read_to_string(prefix.versions().join(newer(2)).join(".bad")).unwrap();
    assert!(why.contains("interpreter"), "{why}");
}

#[test]
fn a_download_that_cannot_be_started_this_time_is_not_given_up_on() {
    let prefix = Prefix::new("eacces");
    let binary = prefix.shelve(&newer(1));
    fs::set_permissions(&binary, fs::Permissions::from_mode(0o644)).unwrap();
    let (said, _) = prefix.run(&[]);
    assert!(said.starts_with("packaged |"), "{said}");
    let folder = prefix.versions().join(newer(1));
    assert!(!folder.join(".bad").exists());
    assert!(!fs::read_dir(&folder).unwrap().flatten().any(|entry| entry.file_name().to_string_lossy().starts_with(".trying-")));
    fs::set_permissions(&binary, fs::Permissions::from_mode(0o755)).unwrap();
    let (said, _) = prefix.run(&[]);
    assert!(said.starts_with(&format!("{} |", newer(1))), "{said}");
}

#[test]
fn asked_its_version_by_the_updater_it_answers_for_the_package() {
    let prefix = Prefix::new("probe");
    prefix.shelve(&newer(1));
    let output = Command::new(prefix.launcher())
        .arg("--version")
        .env("XDG_DATA_HOME", prefix.root.join("data"))
        .env("OMACHARTS_PROBE", "1")
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), format!("omacharts {INSTALLED}"));
}
