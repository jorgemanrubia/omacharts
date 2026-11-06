//! A Sierra Chart installation: where it is, and where it keeps its data.
//!
//! On Linux Sierra Chart runs under Wine, so an install is a Windows
//! program's folder somewhere in the Linux file system, often inside a Wine
//! prefix and sometimes not (Wine runs a program from anywhere). The data
//! folder is `Data` inside it unless Sierra Chart was told otherwise, in
//! which case `DataFilesFolder.txt` says where, as a Windows path. Under
//! Wine `Z:` is the Linux root and `C:` the prefix's `drive_c`, so that path
//! is mapped back; one that cannot be mapped, or names nothing, is ignored
//! in favour of `Data`, which is what Sierra Chart itself falls back to.
//!
//! [`candidates`] looks where people put Sierra Chart: the downloads and
//! home folders, every Wine prefix including Bottles' and Lutris', and
//! `/opt`. It costs a handful of directory reads, so a settings page can
//! call it when it opens.

use std::fmt;
use std::path::{Path, PathBuf};

use crate::settings::{self, Scale, SymbolSettings};
use crate::symbols::Listing;

/// A Sierra Chart installation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Install {
    /// The folder Sierra Chart is installed in.
    pub root: PathBuf,
    /// Where it keeps its `.scid` and `.dly` files.
    pub data: PathBuf,
}

/// Why a folder is not a usable Sierra Chart install. Its `Display` is a
/// short sentence, fit for a settings row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Problem {
    /// Nothing is at the path, or it is not a folder.
    NoFolder,
    /// A folder, but not one Sierra Chart is installed in.
    NotSierraChart,
    /// Sierra Chart is there but its data folder is not.
    NoData,
    /// The folder could not be read.
    Unreadable(std::io::ErrorKind),
}

impl fmt::Display for Problem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Problem::NoFolder => "There is no folder here",
            Problem::NotSierraChart => "No Sierra Chart folder here",
            Problem::NoData => "Its Data folder is missing",
            Problem::Unreadable(std::io::ErrorKind::PermissionDenied) => "This folder can't be opened",
            Problem::Unreadable(_) => "This folder can't be read",
        })
    }
}

impl std::error::Error for Problem {}

/// Files only a Sierra Chart folder has.
const MARKERS: [&str; 3] = ["SierraChart_64.exe", "SierraChart.exe", "Sierra4.config"];

impl Install {
    /// Opens the Sierra Chart install at `root`.
    pub fn open(root: &Path) -> Result<Install, Problem> {
        match std::fs::metadata(root) {
            Ok(m) if m.is_dir() => {}
            Ok(_) => return Err(Problem::NoFolder),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(Problem::NoFolder),
            Err(e) => return Err(Problem::Unreadable(e.kind())),
        }
        if let Err(e) = std::fs::read_dir(root) {
            return Err(Problem::Unreadable(e.kind()));
        }
        if !MARKERS.iter().any(|m| root.join(m).is_file()) {
            return Err(Problem::NotSierraChart);
        }
        let data = data_folder(root).ok_or(Problem::NoData)?;
        Ok(Install { root: root.to_path_buf(), data })
    }

    /// Every symbol in the data folder, from one directory read.
    pub fn listing(&self) -> Listing {
        Listing::read(&self.data)
    }

    /// The `SymbolSettings` folder.
    pub fn settings_folder(&self) -> PathBuf {
        self.root.join("SymbolSettings")
    }

    /// How a symbol's stored prices scale, by its file stem
    /// (`ESZ26_FUT_CME`), or `None` if no service file knows it.
    pub fn scale(&self, stem: &str) -> Option<Scale> {
        self.symbol_settings(stem).map(|s| s.scale)
    }

    /// [`Install::scale`], with the file and pattern that said so.
    pub fn symbol_settings(&self, stem: &str) -> Option<SymbolSettings> {
        settings::lookup(&self.settings_folder(), stem)
    }
}

/// The data folder: `DataFilesFolder.txt`'s, when it names a folder that
/// exists, otherwise `Data`.
fn data_folder(root: &Path) -> Option<PathBuf> {
    let named = std::fs::read(root.join("DataFilesFolder.txt"))
        .ok()
        .and_then(|b| String::from_utf8(b).ok())
        .and_then(|s| windows_path(&s, root))
        .filter(|p| p.is_dir());
    named.or_else(|| Some(root.join("Data")).filter(|p| p.is_dir()))
}

/// The Wine prefix a path is inside, if it is: the folder holding the
/// `drive_c` it is under.
fn prefix_of(path: &Path) -> Option<&Path> {
    path.ancestors().find(|a| a.file_name().is_some_and(|n| n == "drive_c"))?.parent()
}

/// A Windows path as Wine maps it, for an install at `root`. `Z:` is the
/// Linux root; any other drive is the prefix's `dosdevices` link for it,
/// or, for `C:`, the prefix's `drive_c`.
pub(crate) fn windows_path(text: &str, root: &Path) -> Option<PathBuf> {
    let text = text.trim_matches(|c: char| c.is_whitespace() || c == '\u{feff}' || c == '\0');
    let b = text.as_bytes();
    if b.len() < 2 || !b[0].is_ascii_alphabetic() || b[1] != b':' {
        return None;
    }
    let letter = b[0].to_ascii_lowercase();
    let rest: Vec<&str> = text[2..].split(['\\', '/']).filter(|s| !s.is_empty()).collect();
    let base = if letter == b'z' {
        PathBuf::from("/")
    } else {
        let prefix = prefix_of(root)?;
        let device = prefix.join("dosdevices").join(format!("{}:", letter as char));
        if device.exists() {
            device
        } else if letter == b'c' {
            prefix.join("drive_c")
        } else {
            return None;
        }
    };
    Some(rest.iter().fold(base, |p, part| p.join(part)))
}

/// Folders that hold a Sierra Chart install, best guesses first, each
/// once. Only folders that are there, and look like an install, are
/// returned.
pub fn candidates() -> Vec<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    candidates_in(home.as_deref(), Path::new("/opt"))
}

/// Whether a folder's name is some spelling of Sierra Chart.
fn is_sierra_name(name: &std::ffi::OsStr) -> bool {
    let Some(name) = name.to_str() else { return false };
    let squashed: String = name.chars().filter(|c| !matches!(c, ' ' | '_' | '-')).collect();
    squashed.eq_ignore_ascii_case("sierrachart")
}

/// The Sierra Chart folders directly inside `parent`, in name order.
fn sierra_folders_in(parent: &Path, out: &mut Vec<PathBuf>) {
    let Ok(dir) = std::fs::read_dir(parent) else { return };
    let mut found: Vec<PathBuf> = dir.flatten().filter(|e| is_sierra_name(&e.file_name())).map(|e| e.path()).collect();
    found.sort();
    out.extend(found);
}

/// The folders directly inside `parent` that match, in name order.
fn subfolders(parent: &Path, keep: impl Fn(&str) -> bool) -> Vec<PathBuf> {
    let Ok(dir) = std::fs::read_dir(parent) else { return Vec::new() };
    let mut found: Vec<PathBuf> =
        dir.flatten().filter(|e| e.file_name().to_str().is_some_and(&keep) && e.path().is_dir()).map(|e| e.path()).collect();
    found.sort();
    found
}

pub(crate) fn candidates_in(home: Option<&Path>, opt: &Path) -> Vec<PathBuf> {
    let mut places: Vec<PathBuf> = Vec::new();
    if let Some(home) = home {
        sierra_folders_in(&home.join("Downloads"), &mut places);
        sierra_folders_in(home, &mut places);
        let mut prefixes = subfolders(home, |n| n.starts_with(".wine"));
        prefixes.extend(subfolders(&home.join(".local/share/wineprefixes"), |_| true));
        prefixes.extend(subfolders(&home.join(".local/share/bottles/bottles"), |_| true));
        prefixes.extend(subfolders(&home.join("Games"), |_| true));
        for prefix in prefixes {
            let drive = prefix.join("drive_c");
            sierra_folders_in(&drive, &mut places);
            sierra_folders_in(&drive.join("Program Files"), &mut places);
        }
    }
    sierra_folders_in(opt, &mut places);

    let mut seen = std::collections::HashSet::new();
    places
        .into_iter()
        .filter(|p| MARKERS.iter().any(|m| p.join(m).is_file()))
        .filter(|p| seen.insert(std::fs::canonicalize(p).unwrap_or_else(|_| p.clone())))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::Scratch;

    fn fake_install(root: &Path) {
        std::fs::create_dir_all(root.join("Data")).unwrap();
        std::fs::write(root.join("SierraChart_64.exe"), "").unwrap();
    }

    #[test]
    fn an_install_is_a_folder_with_sierra_chart_and_its_data() {
        let dir = Scratch::new();
        let root = dir.join("SierraChart");
        assert_eq!(Install::open(&root), Err(Problem::NoFolder));
        std::fs::create_dir(&root).unwrap();
        assert_eq!(Install::open(&root), Err(Problem::NotSierraChart));
        std::fs::write(root.join("Sierra4.config"), "").unwrap();
        assert_eq!(Install::open(&root), Err(Problem::NoData));
        std::fs::create_dir(root.join("Data")).unwrap();
        assert_eq!(Install::open(&root), Ok(Install { root: root.clone(), data: root.join("Data") }));
        std::fs::write(dir.join("file"), "").unwrap();
        assert_eq!(Install::open(&dir.join("file")), Err(Problem::NoFolder));
        assert_eq!(Problem::NotSierraChart.to_string(), "No Sierra Chart folder here");
        assert_eq!(Problem::NoData.to_string(), "Its Data folder is missing");
    }

    #[test]
    fn the_data_folder_named_in_data_files_folder_is_used_when_it_is_there() {
        let dir = Scratch::new();
        let root = dir.join("SierraChart");
        fake_install(&root);
        let elsewhere = dir.join("Elsewhere").join("Data");
        std::fs::create_dir_all(&elsewhere).unwrap();
        let windows = format!("Z:{}\\\r\n", elsewhere.display().to_string().replace('/', "\\"));
        std::fs::write(root.join("DataFilesFolder.txt"), windows).unwrap();
        assert_eq!(Install::open(&root).unwrap().data, elsewhere);

        std::fs::write(root.join("DataFilesFolder.txt"), "Z:\\nowhere\\at\\all\\\r\n").unwrap();
        assert_eq!(Install::open(&root).unwrap().data, root.join("Data"));
        std::fs::write(root.join("DataFilesFolder.txt"), "").unwrap();
        assert_eq!(Install::open(&root).unwrap().data, root.join("Data"));
    }

    #[test]
    fn windows_paths_map_through_wines_drives() {
        let prefix = Path::new("/home/a/.wine_sc");
        let root = prefix.join("drive_c/SierraChart");
        assert_eq!(windows_path("Z:\\home\\a\\Downloads\\SierraChart\\Data\\", &root), Some(PathBuf::from("/home/a/Downloads/SierraChart/Data")));
        assert_eq!(windows_path("C:\\SierraChart\\Data", &root), Some(prefix.join("drive_c/SierraChart/Data")));
        assert_eq!(windows_path("c:/SierraChart/Data", &root), Some(prefix.join("drive_c/SierraChart/Data")));
        assert_eq!(windows_path("C:\\SierraChart\\Data", Path::new("/home/a/Downloads/SierraChart")), None);
        assert_eq!(windows_path("D:\\Data", &root), None);
        assert_eq!(windows_path("\\\\server\\share", &root), None);
        assert_eq!(windows_path("Data", &root), None);
    }

    #[test]
    fn a_prefixs_dosdevices_link_says_where_a_drive_is() {
        let dir = Scratch::new();
        let prefix = dir.join("prefix");
        let root = prefix.join("drive_c").join("SierraChart");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(prefix.join("dosdevices")).unwrap();
        let d = dir.join("disk");
        std::fs::create_dir_all(d.join("Data")).unwrap();
        std::os::unix::fs::symlink(&d, prefix.join("dosdevices").join("d:")).unwrap();
        assert_eq!(windows_path("D:\\Data", &root), Some(prefix.join("dosdevices/d:/Data")));
    }

    #[test]
    fn candidates_are_found_where_people_put_sierra_chart() {
        let dir = Scratch::new();
        let home = dir.join("home");
        let opt = dir.join("opt");
        fake_install(&home.join("Downloads/SierraChart"));
        fake_install(&home.join(".wine_sierrachart/drive_c/SierraChart"));
        fake_install(&home.join(".local/share/bottles/bottles/Trading/drive_c/Sierra Chart"));
        fake_install(&home.join(".local/share/wineprefixes/sc/drive_c/sierrachart"));
        fake_install(&home.join("Games/sierra-chart/drive_c/SierraChart"));
        fake_install(&opt.join("SierraChart"));
        std::fs::create_dir_all(home.join("SierraChart")).unwrap();
        std::os::unix::fs::symlink(home.join("Downloads/SierraChart"), home.join("Sierra Chart")).unwrap();

        let found = candidates_in(Some(&home), &opt);
        assert_eq!(
            found,
            [
                home.join("Downloads/SierraChart"),
                home.join(".wine_sierrachart/drive_c/SierraChart"),
                home.join(".local/share/wineprefixes/sc/drive_c/sierrachart"),
                home.join(".local/share/bottles/bottles/Trading/drive_c/Sierra Chart"),
                home.join("Games/sierra-chart/drive_c/SierraChart"),
                opt.join("SierraChart"),
            ]
        );
        assert!(candidates_in(None, &dir.join("nothing")).is_empty());
    }
}
