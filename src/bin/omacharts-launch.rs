//! `/usr/bin/omacharts`, as the package installs it: the command everybody
//! runs, which runs the app.
//!
//! The app is `/usr/lib/omacharts/omacharts`, or a newer version the updater
//! downloaded, whichever is newer; this decides, then becomes that binary
//! with an `exec`, passing every argument and the whole environment along.
//! It links nothing but `std`, so it is running in a fraction of a
//! millisecond, and GTK is loaded once, by the binary that keeps the window
//! — see `src/launch.rs` for why that matters.
//!
//! Asked its version by the updater, it answers for itself: it is the
//! version pacman installed, and asking the app would start GTK for nothing.

use std::path::{Path, PathBuf};

#[allow(dead_code)]
#[path = "../launch.rs"]
mod launch;

fn main() {
    let installed = launch::Version::running();
    if std::env::var_os(launch::PROBE_ENV).is_some() {
        println!("omacharts {installed}");
        return;
    }
    let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("/usr/bin/omacharts"));
    launch::hand_over_if_newer(&exe, installed);

    let Some(app) = the_app(&exe) else {
        eprintln!("omacharts: the app is not installed beside {}", exe.display());
        std::process::exit(127);
    };
    let failed = launch::exec(&app);
    eprintln!("omacharts: {} could not be started: {failed}", app.display());
    std::process::exit(127);
}

/// The app this launcher was installed with: under `lib/omacharts/` in the
/// same prefix, or beside it in a cargo target directory. Never itself.
fn the_app(exe: &Path) -> Option<PathBuf> {
    let packaged = exe.parent().and_then(Path::parent).map(|prefix| prefix.join("lib/omacharts/omacharts"));
    let beside = exe.with_file_name("omacharts");
    [packaged, Some(beside)].into_iter().flatten().find(|app| app.is_file() && app != exe)
}
