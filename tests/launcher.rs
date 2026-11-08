//! The launcher as the package installs it: `bin/omacharts` in a prefix with
//! the app at `lib/omacharts/omacharts`, run as a process. The apps here are
//! shell scripts that say who they are, so nothing starts GTK.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

const INSTALLED: &str = env!("CARGO_PKG_VERSION");

struct Prefix {
    root: PathBuf,
}

impl Prefix {
    fn new(name: &str) -> Prefix {
        let root = std::env::temp_dir().join(format!("omacharts-launcher-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("bin")).unwrap();
        fs::copy(env!("CARGO_BIN_EXE_omacharts-launch"), root.join("bin/omacharts")).unwrap();
        let prefix = Prefix { root };
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
        let child = Command::new(self.launcher())
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
    let told = format!("{INSTALLED}\u{1f}{}", prefix.launcher().display());
    assert_eq!(said, format!("packaged | {told} | chart open two words"));
}

#[test]
fn it_runs_a_newer_download_and_leaves_it_a_marker() {
    let prefix = Prefix::new("newer");
    prefix.shelve(&newer(1));
    let (said, pid) = prefix.run(&["--version"]);
    assert!(said.starts_with(&format!("{} | {INSTALLED}\u{1f}", newer(1))), "{said}");
    assert!(said.ends_with("| --version"), "{said}");
    // The app removes it first thing; a script does not, which is how the
    // next launch would know this version never started.
    assert!(prefix.versions().join(newer(1)).join(format!(".trying-{pid}")).exists());
}

#[test]
fn a_download_no_newer_than_the_package_is_never_run() {
    let prefix = Prefix::new("same");
    prefix.shelve(INSTALLED);
    let (said, _) = prefix.run(&[]);
    assert!(said.starts_with("packaged |"), "{said}");
}

#[test]
fn a_download_that_died_before_starting_is_passed_over_for_good() {
    let prefix = Prefix::new("died");
    prefix.shelve(&newer(1));
    // A marker from a process that is long gone.
    fs::write(prefix.versions().join(newer(1)).join(".trying-4000000000"), b"").unwrap();
    let (said, _) = prefix.run(&[]);
    assert!(said.starts_with("packaged |"), "{said}");
    assert!(prefix.versions().join(newer(1)).join(".bad").exists());
}

#[test]
fn a_download_that_cannot_be_executed_is_marked_bad_and_the_package_runs() {
    let prefix = Prefix::new("broken");
    let binary = prefix.shelve(&newer(2));
    prefix.shelve(&newer(1));
    fs::set_permissions(&binary, fs::Permissions::from_mode(0o644)).unwrap();
    let (said, _) = prefix.run(&[]);
    assert!(said.starts_with("packaged |"), "{said}");
    assert!(prefix.versions().join(newer(2)).join(".bad").exists());
    // And next time, the next one down.
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
