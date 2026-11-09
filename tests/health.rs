//! The real app, handed over to by the real launcher, run the ways that
//! never put a window up: none of them may call the version healthy, and
//! none may leave a launch behind that counts as one that did not run.
//!
//! A version called healthy is never given up on, and the backup and the
//! older download that would let it be are let go, so this is the claim
//! that has to hold for everything but a window kept up for a few seconds.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const INSTALLED: &str = env!("CARGO_PKG_VERSION");

fn newer() -> String {
    let mut parts = INSTALLED.split('.').map(|part| part.parse::<u32>().unwrap());
    let (major, minor, patch) = (parts.next().unwrap(), parts.next().unwrap(), parts.next().unwrap());
    format!("{major}.{minor}.{}", patch + 1)
}

/// `from` copied to `to` as an executable, by `install` in a process of its
/// own, so no descriptor open for writing is ever inherited by a child.
fn place(from: &Path, to: &Path) {
    fs::create_dir_all(to.parent().unwrap()).unwrap();
    let status = Command::new("/usr/bin/install").arg("-m755").arg(from).arg(to).status().unwrap();
    assert!(status.success());
}

fn markers(folder: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(folder)
        .unwrap()
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.starts_with('.'))
        .collect();
    names.sort();
    names
}

#[test]
fn nothing_but_a_window_marks_a_downloaded_version_healthy_or_counts_against_it() {
    let root = std::env::temp_dir().join(format!("omacharts-health-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let launcher = root.join("bin/omacharts");
    place(Path::new(env!("CARGO_BIN_EXE_omacharts-launch")), &launcher);
    let packaged = root.join("packaged.sh");
    fs::write(&packaged, "#!/bin/sh\necho packaged\n").unwrap();
    place(&packaged, &root.join("lib/omacharts/omacharts"));
    let folder: PathBuf = root.join("home/.local/share/omacharts/versions").join(newer());
    place(Path::new(env!("CARGO_BIN_EXE_omacharts")), &folder.join("omacharts"));
    // A launch that ended before this test, which must still be there after.
    fs::write(folder.join(".trying-4000000000"), b"").unwrap();

    let runs: [&[&str]; 5] = [
        &["--version"],
        &["--help"],
        // In the caller, without the database.
        &["update", "check"],
        &["skill", "status"],
        // In this process, against the database, with no window open.
        &["watchlist", "list"],
    ];
    for args in runs {
        let output = Command::new(&launcher)
            .args(args)
            .env("HOME", root.join("home"))
            .env("XDG_DATA_HOME", root.join("home/.local/share"))
            .env("XDG_CONFIG_HOME", root.join("home/.config"))
            .env("XDG_CACHE_HOME", root.join("home/.cache"))
            // No bus, so no window is found and nothing is handed to one.
            .env("DBUS_SESSION_BUS_ADDRESS", "unix:path=/nonexistent")
            .env("OMACHARTS_UPDATE_SOURCE", "http://127.0.0.1:9")
            .env_remove("OMACHARTS_LAUNCHER")
            .env_remove("OMACHARTS_PROBE")
            .env_remove("OMACHARTS_NO_UPDATE")
            .env_remove("WAYLAND_DISPLAY")
            .env_remove("DISPLAY")
            .output()
            .unwrap();
        let said = String::from_utf8_lossy(&output.stdout);
        assert!(!said.contains("packaged"), "{args:?} ran the download: {said}");
        assert_eq!(markers(&folder), [".trying-4000000000"], "{args:?}");
    }
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn a_window_launch_with_no_display_does_not_count_against_a_download() {
    // A bus of its own, so the launch registers and GTK is started, and
    // ends the process for want of a display, as from ssh or a script.
    if !Path::new("/usr/bin/dbus-run-session").exists() {
        eprintln!("no dbus-run-session here; skipped");
        return;
    }
    let root = std::env::temp_dir().join(format!("omacharts-nodisplay-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let launcher = root.join("bin/omacharts");
    place(Path::new(env!("CARGO_BIN_EXE_omacharts-launch")), &launcher);
    let packaged = root.join("packaged.sh");
    fs::write(&packaged, "#!/bin/sh\necho packaged\n").unwrap();
    place(&packaged, &root.join("lib/omacharts/omacharts"));
    let folder: PathBuf = root.join("home/.local/share/omacharts/versions").join(newer());
    place(Path::new(env!("CARGO_BIN_EXE_omacharts")), &folder.join("omacharts"));
    let runtime = root.join("runtime");
    fs::create_dir_all(&runtime).unwrap();

    // Three, which would give it up if they counted; with nothing set, and
    // with an X display named that no server holds, as a stale `DISPLAY`
    // in a script is.
    let stale = !Path::new("/tmp/.X11-unix/X99").exists();
    for display in [None, None, None, Some(":99"), Some(":99.0"), Some(":99")].into_iter().filter(|d| d.is_none() || stale) {
        let mut command = Command::new("/usr/bin/dbus-run-session");
        if let Some(display) = display {
            command.env("DISPLAY", display);
        } else {
            command.env_remove("DISPLAY");
        }
        let output = command
            .arg("--")
            .arg(&launcher)
            .env("HOME", root.join("home"))
            .env("XDG_DATA_HOME", root.join("home/.local/share"))
            .env("XDG_CONFIG_HOME", root.join("home/.config"))
            .env("XDG_CACHE_HOME", root.join("home/.cache"))
            .env("XDG_RUNTIME_DIR", &runtime)
            .env("OMACHARTS_UPDATE_SOURCE", "http://127.0.0.1:9")
            .env_remove("DBUS_SESSION_BUS_ADDRESS")
            .env_remove("OMACHARTS_LAUNCHER")
            .env_remove("OMACHARTS_PROBE")
            .env_remove("OMACHARTS_NO_UPDATE")
            .env_remove("WAYLAND_DISPLAY")
            .output()
            .unwrap();
        let said = String::from_utf8_lossy(&output.stdout);
        assert!(!said.contains("packaged"), "{display:?}: the download was given up on: {said}");
        assert!(!output.status.success(), "{display:?}: a window came up with no display?");
    }
    assert!(markers(&folder).is_empty(), "{:?}", markers(&folder));
    let _ = fs::remove_dir_all(&root);
}
