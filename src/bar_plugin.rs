//! Putting omacharts in the Omarchy bar.
//!
//! The widget is a plugin folder plus an entry in the shell's layout. Both are
//! written here so the app can offer it as a switch rather than a page of
//! instructions.
//!
//! It deliberately outlives the app. The plugin asks the `omacharts watchlist`
//! command for its data, so the bar keeps working — and keeps its icon — after
//! the window is closed.

use std::path::{Path, PathBuf};

pub const PLUGIN_ID: &str = "omacharts.watchlist";

/// The id the widget was installed under before it had one of its own.
///
/// Kept only so that an install made by an older version can be found and
/// moved — see `migrate`. Nothing is ever written under it.
pub const OLD_PLUGIN_ID: &str = "jorgemanrubia.omacharts";

/// Where the shell reads its layout from.
fn shell_config(home: &Path) -> PathBuf {
    home.join(".config/omarchy/shell.json")
}

fn plugins_dir(home: &Path) -> PathBuf {
    home.join(".config/omarchy/plugins")
}

/// The folder the widget is written to.
///
/// Public so `plugin status` can say where it looked, which is most of what
/// makes that command worth typing when the answer is "not installed".
pub fn location(home: &Path) -> PathBuf {
    plugins_dir(home).join(PLUGIN_ID)
}

/// Tell the bar widget the watchlist changed.
///
/// Fire and forget: the widget may not be installed, the shell may not be
/// running, and neither is a problem worth reporting. Without this the bar
/// sits a refresh interval behind every add and remove, which looks like it
/// is not listening.
pub fn notify_changed() {
    let _ = std::process::Command::new("omarchy-shell")
        .args([PLUGIN_ID, "refresh"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}

/// Is this an Omarchy desktop at all?
pub fn available(home: &Path) -> bool {
    shell_config(home).is_file() && plugins_dir(home).is_dir()
}

/// Is the widget installed and in the bar?
pub fn installed(home: &Path) -> bool {
    if !location(home).join("manifest.json").is_file() {
        return false;
    }
    std::fs::read_to_string(shell_config(home))
        .ok()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .map(|config| in_layout(&config))
        .unwrap_or(false)
}

/// The plugin, carried in the binary.
///
/// Embedded rather than copied from a folder beside the executable: the
/// installed widget then cannot drift from the app that installed it, and
/// there is no path to guess at runtime.
const FILES: &[(&str, &str)] = &[
    ("manifest.json", include_str!("../plugin/manifest.json")),
    ("Model.js", include_str!("../plugin/Model.js")),
    ("ServiceHost.js", include_str!("../plugin/ServiceHost.js")),
    ("Service.qml", include_str!("../plugin/Service.qml")),
    ("Panel.qml", include_str!("../plugin/Panel.qml")),
    ("OmachartsIcon.qml", include_str!("../plugin/OmachartsIcon.qml")),
    ("Sparkline.qml", include_str!("../plugin/Sparkline.qml")),
];

/// Where an older version put the widget.
fn old_location(home: &Path) -> PathBuf {
    plugins_dir(home).join(OLD_PLUGIN_ID)
}

/// Write the plugin folder and add it to the bar.
pub fn install(home: &Path) -> Result<(), String> {
    migrate_in_full(home)?;
    let target = location(home);
    // Replaced rather than merged: a file we stopped shipping left behind is
    // a file the shell will still load.
    if target.exists() {
        std::fs::remove_dir_all(&target).map_err(|e| format!("could not replace it: {e}"))?;
    }
    std::fs::create_dir_all(&target).map_err(|e| format!("could not create it: {e}"))?;
    for (name, contents) in FILES {
        std::fs::write(target.join(name), contents)
            .map_err(|e| format!("could not write {name}: {e}"))?;
    }
    edit_shell(home, add_to_layout).map(|_| ())
}

/// Bring an already-installed widget up to date with this binary.
///
/// The plugin is installed once, from a switch in the settings, and then never
/// touched again — which left the bar running whatever version of the widget
/// happened to be current the day it was switched on. Every upgrade after that
/// shipped a new Panel.qml nobody ever saw.
///
/// Only files whose contents actually differ are written. The shell watches
/// the folder and reloads on any change, so rewriting seven identical files
/// every launch would restart the bar for nothing.
pub fn refresh(home: &Path) -> Result<(), String> {
    migrate_in_full(home)?;
    if !installed(home) {
        return Ok(());
    }
    let target = location(home);
    let mut changed = false;
    for (name, contents) in FILES {
        let path = target.join(name);
        if std::fs::read_to_string(&path).is_ok_and(|current| current == *contents) {
            continue;
        }
        std::fs::write(&path, contents).map_err(|e| format!("could not update {name}: {e}"))?;
        changed = true;
    }
    // Writing the files is not enough: the shell compiled the old QML when it
    // started and will go on drawing it until something tells it otherwise.
    // rescanPlugins clears the component cache and reloads the plugins in
    // place, which is the small version of restarting the shell — nobody
    // should have to restart their desktop to get a bug fix in a bar widget.
    if changed {
        reload_plugins();
    }
    Ok(())
}

/// Move a widget installed under the old id to the new one.
///
/// The widget used to be `jorgemanrubia.omacharts`, and every desktop that
/// switched it on has a folder and a bar entry by that name. Nobody upgrading
/// should have to know that, or do anything about it, so this runs on its own
/// from every process that starts: the window, every command, and the widget's
/// own polls — which run `omacharts watchlist`, so a bar whose window is never
/// opened is moved the first time it asks for quotes.
///
/// Which is why it costs one stat when there is nothing to do, and that is
/// the usual case: the old folder is looked for, and nothing else is opened
/// until it is found. shell.json is not read. The one state this cannot see
/// — an entry in the bar whose folder has already gone — is left to
/// `migrate_in_full`, which the window and the `plugin` commands run and
/// which reads shell.json anyway.
///
/// Nothing is reported here either way; the caller in `main` is every
/// process, and a move that cannot be made — a read-only `~/.config/omarchy`,
/// say — would otherwise be a line on stderr from every poll of the bar
/// forever. The window and the `plugin` commands report it, once, to somebody
/// who is looking. And it fails early: the first step is writing the folder,
/// so a tree that cannot be written costs a stat and one refused mkdir, not
/// a parse of shell.json every time.
pub fn migrate(home: &Path) -> Result<(), String> {
    if !old_location(home).exists() {
        return Ok(());
    }
    migrate_in_full(home)
}

/// `migrate`, and also the bar entry whose folder has gone.
///
/// After it there is exactly one folder, under the new id, and shell.json
/// names only the new id — whether it found the old folder, both (a downgrade
/// that reinstalled the old one beside the new), or only a bar entry whose
/// folder has gone. The entry keeps its place and its settings; they live on
/// the entry itself, so renaming its id carries them over. Where the new id is
/// already in the bar too, that one is kept and the old one dropped, because
/// the widget does not allow two of itself.
///
/// The shell is reloaded afterwards, so the bar draws the widget under its new
/// name straight away rather than at the next login. Idempotent, because it
/// runs from every path in, and nothing is reloaded when nothing moved.
pub fn migrate_in_full(home: &Path) -> Result<(), String> {
    if migrate_files(home)? {
        reload_plugins();
    }
    Ok(())
}

/// The part of `migrate_in_full` that touches files. Says whether it changed
/// any, which is what decides whether the shell has to be told.
///
/// Several processes run this at once on the day of an upgrade — the bar's
/// polls, the window, a command typed in a terminal — and none of them knows
/// about the others. So every step is one that two can take together: the
/// folder is created with `create_dir_all`, every file lands by rename, and
/// an old folder another process has already removed counts as removed. The
/// entry is renamed by whichever gets there first; the others re-read
/// shell.json inside `edit_shell`, find nothing left to change, and write
/// nothing.
fn migrate_files(home: &Path) -> Result<bool, String> {
    let old = old_location(home);
    let old_dir = old.exists();
    // Parsed only when the id is in the text at all, and then asked whether a
    // rename would change anything: the id quoted in some other plugin's
    // setting is not an entry, and must not have this run on every launch.
    let old_entry = std::fs::read_to_string(shell_config(home))
        .ok()
        .filter(|text| text.contains(OLD_PLUGIN_ID))
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .is_some_and(|mut config| rename_in_config(&mut config));
    if !old_dir && !old_entry {
        return Ok(false);
    }

    // The new folder first, then the bar entry, then the old folder gone: at
    // no point does shell.json name a widget that has no folder. Written in
    // full even when only the entry was left, because an entry in the bar is
    // somebody asking for the widget.
    let mut changed = false;
    let target = location(home);
    std::fs::create_dir_all(&target).map_err(|e| format!("could not create it: {e}"))?;
    for (name, contents) in FILES {
        let path = target.join(name);
        if std::fs::read_to_string(&path).is_ok_and(|current| current == *contents) {
            continue;
        }
        replace_file(&path, contents).map_err(|e| format!("could not write {name}: {e}"))?;
        changed = true;
    }
    if old_entry {
        changed |= edit_shell(home, rename_in_config)?;
    }
    if old_dir {
        match std::fs::remove_dir_all(&old) {
            Ok(()) => changed = true,
            // Gone, or going, under another process — the folder itself or
            // a file in it that was there a moment ago: the outcome asked for.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(format!("could not remove the old widget: {e}")),
        }
    }
    Ok(changed)
}

/// Rewrite the old id to the new one wherever shell.json names it: the bar's
/// sections, its centre anchor, `plugins[]` and `disabledPlugins[]`.
pub fn rename_in_config(config: &mut serde_json::Value) -> bool {
    let mut changed = false;

    // Already in the bar under the new id means an old entry is a duplicate,
    // not something to rename.
    let mut have_new = in_layout(config);
    for section in ["left", "center", "right"] {
        let Some(items) =
            config.pointer_mut(&format!("/bar/layout/{section}")).and_then(|v| v.as_array_mut())
        else {
            continue;
        };
        changed |= rename_in_list(items, &mut have_new);
    }

    // Shell-wide lists: objects with an id in `plugins[]`, bare ids in
    // `disabledPlugins[]`. Neither should name a bar widget, but a hand-edited
    // file may, and the old id left there would be the one trace remaining.
    for list in ["plugins", "disabledPlugins"] {
        let Some(items) = config.get_mut(list).and_then(|v| v.as_array_mut()) else {
            continue;
        };
        let mut have_new = items.iter().any(|item| names(item, PLUGIN_ID));
        changed |= rename_in_list(items, &mut have_new);
    }

    if config["bar"]["centerAnchor"] == OLD_PLUGIN_ID {
        config["bar"]["centerAnchor"] = serde_json::json!(PLUGIN_ID);
        changed = true;
    }
    changed
}

/// Does this entry name this id — as an object's `id`, or as a bare string?
fn names(item: &serde_json::Value, id: &str) -> bool {
    item == id || item["id"] == id
}

/// Rename the first old entry in place and drop any after it, or drop them
/// all when the new id is already there. In place is what keeps its position,
/// and everything on the entry other than its id.
fn rename_in_list(items: &mut Vec<serde_json::Value>, have_new: &mut bool) -> bool {
    let before = items.clone();
    items.retain_mut(|item| {
        if !names(item, OLD_PLUGIN_ID) {
            return true;
        }
        if *have_new {
            return false;
        }
        *have_new = true;
        match item.is_string() {
            true => *item = serde_json::json!(PLUGIN_ID),
            false => item["id"] = serde_json::json!(PLUGIN_ID),
        }
        true
    });
    *items != before
}

/// Ask the shell to recompile its plugins. Best effort, like `notify_changed`.
fn reload_plugins() {
    // A test has no business restarting the bar of whoever is running it.
    if cfg!(test) {
        return;
    }
    let _ = std::process::Command::new("omarchy-shell")
        .args(["-q", "shell", "rescanPlugins"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}

/// Take it out of the bar and remove the folder, under either id. Says whether
/// there was anything to take out, so `plugin uninstall` can say so too.
pub fn remove(home: &Path) -> Result<bool, String> {
    migrate_in_full(home)?;
    let mut removed = edit_shell(home, remove_from_layout)?;
    let target = location(home);
    if target.exists() {
        std::fs::remove_dir_all(&target).map_err(|e| format!("could not remove it: {e}"))?;
        removed = true;
    }
    Ok(removed)
}

/// Read, change, write — atomically, because a half-written shell.json is a
/// desktop with no bar. Says whether anything was written.
fn edit_shell(home: &Path, change: impl Fn(&mut serde_json::Value) -> bool) -> Result<bool, String> {
    let path = shell_config(home);
    let text = std::fs::read_to_string(&path).map_err(|e| format!("shell.json: {e}"))?;
    let mut config: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("shell.json is not readable: {e}"))?;

    if !change(&mut config) {
        return Ok(false);
    }

    let rendered = serde_json::to_string_pretty(&config)
        .map_err(|e| format!("could not write shell.json: {e}"))?;
    replace_file(&path, &(rendered + "\n")).map_err(|e| format!("shell.json: {e}"))?;
    Ok(true)
}

/// Write a file so that anything reading it sees the old contents or the new,
/// never a partial write and never nothing.
///
/// Written beside it and renamed over it, under a name no other writer shares:
/// with one shared temporary name, two processes writing the same file at once
/// would truncate each other's half-written copy, and the second to rename
/// would find it already gone. The shell reads shell.json the moment it
/// changes, and an empty one puts the default layout in the bar.
fn replace_file(path: &Path, contents: &str) -> std::io::Result<()> {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(format!(
        ".omacharts-tmp.{}.{}",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    let temp = path.with_file_name(name);
    std::fs::write(&temp, contents)?;
    std::fs::rename(&temp, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&temp);
    })
}

/// Is our id anywhere in the bar?
pub fn in_layout(config: &serde_json::Value) -> bool {
    ["left", "center", "right"].iter().any(|section| {
        config["bar"]["layout"][section]
            .as_array()
            .map(|items| items.iter().any(|item| item["id"] == PLUGIN_ID))
            .unwrap_or(false)
    })
}

/// Add the widget to the right-hand side, where status things live.
pub fn add_to_layout(config: &mut serde_json::Value) -> bool {
    if in_layout(config) {
        return false;
    }
    let right = config
        .pointer_mut("/bar/layout/right")
        .and_then(|value| value.as_array_mut());
    match right {
        Some(items) => {
            items.push(serde_json::json!({ "id": PLUGIN_ID }));
            true
        }
        // A shell.json with no right-hand side is unusual but not broken;
        // make one rather than refusing.
        None => {
            let Some(root) = config.as_object_mut() else { return false };
            root.entry("bar").or_insert_with(|| serde_json::json!({}));
            let layout = root["bar"]
                .as_object_mut()
                .map(|b| b.entry("layout").or_insert_with(|| serde_json::json!({})));
            let Some(layout) = layout else { return false };
            let Some(layout) = layout.as_object_mut() else { return false };
            layout.insert(
                "right".to_string(),
                serde_json::json!([{ "id": PLUGIN_ID }]),
            );
            true
        }
    }
}

/// Take the widget out of every section it appears in.
pub fn remove_from_layout(config: &mut serde_json::Value) -> bool {
    let mut changed = false;
    for section in ["left", "center", "right"] {
        if let Some(items) =
            config.pointer_mut(&format!("/bar/layout/{section}")).and_then(|v| v.as_array_mut())
        {
            let before = items.len();
            items.retain(|item| item["id"] != PLUGIN_ID);
            changed |= items.len() != before;
        }
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn shell() -> serde_json::Value {
        json!({
            "bar": {
                "layout": {
                    "left": [{ "id": "omarchy.menu" }],
                    "center": [{ "id": "omarchy.clock" }],
                    "right": [{ "id": "omarchy.tray" }]
                }
            }
        })
    }

    #[test]
    fn adding_puts_it_on_the_right_and_leaves_everything_else_alone() {
        let mut config = shell();
        assert!(add_to_layout(&mut config));
        assert!(in_layout(&config));

        let right = config["bar"]["layout"]["right"].as_array().unwrap();
        assert_eq!(right.len(), 2);
        assert_eq!(right[0]["id"], "omarchy.tray", "the tray stays put");
        assert_eq!(right[1]["id"], PLUGIN_ID);
        assert_eq!(config["bar"]["layout"]["left"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn adding_twice_changes_nothing() {
        let mut config = shell();
        assert!(add_to_layout(&mut config));
        assert!(!add_to_layout(&mut config), "already there");
        assert_eq!(config["bar"]["layout"]["right"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn removing_takes_it_out_of_wherever_it_is() {
        // Someone may have dragged it to the centre.
        let mut config = shell();
        config["bar"]["layout"]["center"]
            .as_array_mut()
            .unwrap()
            .push(json!({ "id": PLUGIN_ID }));
        assert!(in_layout(&config));

        assert!(remove_from_layout(&mut config));
        assert!(!in_layout(&config));
        assert_eq!(config["bar"]["layout"]["center"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn removing_when_it_is_not_there_changes_nothing() {
        let mut config = shell();
        assert!(!remove_from_layout(&mut config));
    }

    #[test]
    fn a_shell_without_a_right_hand_side_gets_one() {
        let mut config = json!({ "bar": { "layout": { "left": [] } } });
        assert!(add_to_layout(&mut config));
        assert!(in_layout(&config));
        assert_eq!(config["bar"]["layout"]["right"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn a_round_trip_leaves_the_file_as_it_was() {
        let original = shell();
        let mut config = original.clone();
        add_to_layout(&mut config);
        remove_from_layout(&mut config);
        assert_eq!(config, original);
    }

    #[test]
    fn the_embedded_plugin_is_complete_and_declares_itself() {
        let names: Vec<&str> = FILES.iter().map(|(name, _)| *name).collect();
        for required in ["manifest.json", "Service.qml", "Panel.qml"] {
            assert!(names.contains(&required), "missing {required}");
        }
        assert!(FILES.iter().all(|(_, body)| !body.trim().is_empty()));

        // Every entry point the manifest names has to be one of the files we
        // actually write, or the shell loads a plugin with a hole in it.
        let manifest: serde_json::Value =
            serde_json::from_str(FILES[0].1).expect("manifest is valid json");
        assert_eq!(manifest["id"], PLUGIN_ID);
        for (_, entry) in manifest["entryPoints"].as_object().unwrap() {
            let entry = entry.as_str().unwrap();
            assert!(names.contains(&entry), "entry point {entry} is not shipped");
        }
    }

    /// A home of its own, so nothing here can reach the bar of whoever runs
    /// the suite.
    struct Home(PathBuf);

    impl Home {
        /// An Omarchy desktop with the widget in the bar the way an older
        /// version left it: under the old id, with a setting changed, between
        /// two other widgets.
        fn upgraded_from_old() -> Home {
            let home = Home::empty();
            let old = home.plugins().join(OLD_PLUGIN_ID);
            std::fs::create_dir_all(&old).unwrap();
            std::fs::write(old.join("manifest.json"), r#"{"id": "jorgemanrubia.omacharts"}"#)
                .unwrap();
            std::fs::write(old.join("Retired.qml"), "Item {}").unwrap();
            home.write_shell(json!({
                "bar": {
                    "layout": {
                        "left": [{ "id": "omarchy.menu" }],
                        "right": [
                            { "id": "omarchy.tray" },
                            { "id": OLD_PLUGIN_ID, "refreshIntervalSec": 300, "showTicker": false },
                            { "id": "omarchy.power" }
                        ]
                    }
                },
                "plugins": [],
                "version": 1
            }));
            home
        }

        fn empty() -> Home {
            static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
            let at = std::env::temp_dir().join(format!(
                "omacharts-bar-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            ));
            let _ = std::fs::remove_dir_all(&at);
            std::fs::create_dir_all(plugins_dir(&at)).unwrap();
            Home(at)
        }

        fn plugins(&self) -> PathBuf {
            plugins_dir(&self.0)
        }

        fn write_shell(&self, config: serde_json::Value) {
            std::fs::write(shell_config(&self.0), config.to_string()).unwrap();
        }

        fn shell(&self) -> serde_json::Value {
            serde_json::from_str(&std::fs::read_to_string(shell_config(&self.0)).unwrap()).unwrap()
        }

        /// Every plugin folder there is, by name.
        fn folders(&self) -> Vec<String> {
            let mut names: Vec<String> = std::fs::read_dir(self.plugins())
                .unwrap()
                .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
                .collect();
            names.sort();
            names
        }
    }

    impl Drop for Home {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn an_old_install_moves_to_the_new_id_and_keeps_its_place_and_settings() {
        let home = Home::upgraded_from_old();
        assert!(migrate_files(&home.0).unwrap());

        assert_eq!(home.folders(), [PLUGIN_ID], "one folder, under the new id");
        let manifest = std::fs::read_to_string(location(&home.0).join("manifest.json")).unwrap();
        assert_eq!(manifest, FILES[0].1, "the widget this binary carries");
        assert!(!location(&home.0).join("Retired.qml").exists(), "nothing of the old one");

        let shell = home.shell();
        let right = shell["bar"]["layout"]["right"].as_array().unwrap();
        assert_eq!(
            right,
            &vec![
                json!({ "id": "omarchy.tray" }),
                json!({ "id": PLUGIN_ID, "refreshIntervalSec": 300, "showTicker": false }),
                json!({ "id": "omarchy.power" }),
            ]
        );
        assert!(!shell.to_string().contains(OLD_PLUGIN_ID), "{shell}");
        assert!(installed(&home.0));
    }

    #[test]
    fn migrating_twice_does_nothing_the_second_time() {
        let home = Home::upgraded_from_old();
        assert!(migrate_files(&home.0).unwrap());
        let (shell, folders) = (home.shell(), home.folders());

        assert!(!migrate_files(&home.0).unwrap(), "nothing left to move, so no reload");
        assert_eq!(home.shell(), shell);
        assert_eq!(home.folders(), folders);
    }

    #[test]
    fn with_no_old_install_nothing_is_touched() {
        let home = Home::empty();
        let config = json!({ "bar": { "layout": { "right": [{ "id": "omarchy.tray" }] } } });
        home.write_shell(config.clone());

        assert!(!migrate_files(&home.0).unwrap());
        assert!(home.folders().is_empty(), "the widget is not installed for anybody");
        assert_eq!(home.shell(), config);
    }

    #[test]
    fn the_old_id_quoted_somewhere_else_is_not_an_install() {
        // A substring is how the check stays cheap, not what decides.
        let home = Home::empty();
        let config = json!({
            "bar": { "layout": { "right": [{ "id": "omarchy.notes", "text": OLD_PLUGIN_ID }] } }
        });
        home.write_shell(config.clone());

        assert!(!migrate_files(&home.0).unwrap());
        assert!(home.folders().is_empty());
        assert_eq!(home.shell(), config);
    }

    #[test]
    fn with_both_ids_installed_only_the_new_one_is_left() {
        // An upgrade, a downgrade that put the old one back, and an upgrade
        // again: two folders, and the widget in the bar twice.
        let home = Home::upgraded_from_old();
        std::fs::create_dir_all(location(&home.0)).unwrap();
        let mut config = home.shell();
        config["bar"]["layout"]["left"]
            .as_array_mut()
            .unwrap()
            .push(json!({ "id": PLUGIN_ID, "showTicker": true }));
        home.write_shell(config);

        assert!(migrate_files(&home.0).unwrap());
        assert_eq!(home.folders(), [PLUGIN_ID]);
        let shell = home.shell();
        assert!(!shell.to_string().contains(OLD_PLUGIN_ID), "{shell}");
        assert_eq!(shell["bar"]["layout"]["right"].as_array().unwrap().len(), 2);
        assert_eq!(
            shell["bar"]["layout"]["left"][1],
            json!({ "id": PLUGIN_ID, "showTicker": true }),
            "the one already under the new id is the one kept"
        );
    }

    #[test]
    fn an_old_entry_whose_folder_has_gone_gets_the_widget_back_under_the_new_id() {
        let home = Home::upgraded_from_old();
        std::fs::remove_dir_all(home.plugins().join(OLD_PLUGIN_ID)).unwrap();

        assert!(migrate_files(&home.0).unwrap());
        assert_eq!(home.folders(), [PLUGIN_ID]);
        assert_eq!(home.shell()["bar"]["layout"]["right"][1]["id"], PLUGIN_ID);
        assert!(installed(&home.0));
    }

    #[test]
    fn an_old_folder_with_no_entry_is_moved_too() {
        let home = Home::upgraded_from_old();
        home.write_shell(json!({ "bar": { "layout": { "right": [] } } }));

        assert!(migrate_files(&home.0).unwrap());
        assert_eq!(home.folders(), [PLUGIN_ID]);
        assert!(!installed(&home.0), "folder moved, but not put in a bar it was not in");
    }

    #[test]
    fn the_shell_wide_lists_are_renamed_as_well() {
        let mut config = json!({
            "bar": { "centerAnchor": OLD_PLUGIN_ID, "layout": {} },
            "plugins": [{ "id": OLD_PLUGIN_ID, "enabled": true }, { "id": "other.thing" }],
            "disabledPlugins": [OLD_PLUGIN_ID, PLUGIN_ID]
        });
        assert!(rename_in_config(&mut config));
        assert_eq!(
            config,
            json!({
                "bar": { "centerAnchor": PLUGIN_ID, "layout": {} },
                "plugins": [{ "id": PLUGIN_ID, "enabled": true }, { "id": "other.thing" }],
                "disabledPlugins": [PLUGIN_ID]
            })
        );
        assert!(!rename_in_config(&mut config), "and once is enough");
    }

    #[test]
    fn every_way_in_moves_an_old_install_first() {
        // Refreshing is what the window does at start, and what `plugin
        // install` does when the widget is already there.
        let home = Home::upgraded_from_old();
        refresh(&home.0).unwrap();
        assert_eq!(home.folders(), [PLUGIN_ID]);
        assert!(installed(&home.0));

        let home = Home::upgraded_from_old();
        install(&home.0).unwrap();
        assert_eq!(home.folders(), [PLUGIN_ID]);
        let right = home.shell()["bar"]["layout"]["right"].clone();
        assert_eq!(right.as_array().unwrap().len(), 3, "moved, not added a second time");
        assert_eq!(right[1]["refreshIntervalSec"], 300);

        let home = Home::upgraded_from_old();
        assert!(remove(&home.0).unwrap(), "there was a widget to take out");
        assert!(home.folders().is_empty(), "uninstalling takes the old one too");
        assert!(!home.shell().to_string().contains(OLD_PLUGIN_ID));
        assert!(!remove(&home.0).unwrap(), "and the second time there was not");
    }

    #[test]
    fn the_check_every_process_makes_looks_only_for_the_old_folder() {
        // Nothing to move: no folder, and shell.json is not even opened — it
        // is unreadable here, and that is not an error.
        let home = Home::empty();
        std::fs::create_dir(shell_config(&home.0)).unwrap();
        migrate(&home.0).unwrap();

        // An entry whose folder has gone is the one state the folder check
        // cannot see. It is left for the paths that read shell.json anyway.
        let home = Home::upgraded_from_old();
        std::fs::remove_dir_all(old_location(&home.0)).unwrap();
        let before = home.shell();
        migrate(&home.0).unwrap();
        assert_eq!(home.shell(), before, "not touched by the cheap check");
        assert!(home.folders().is_empty());

        migrate_in_full(&home.0).unwrap();
        assert_eq!(home.folders(), [PLUGIN_ID]);
        assert!(installed(&home.0));

        // And the folder alone is enough to set the whole move going.
        let home = Home::upgraded_from_old();
        migrate(&home.0).unwrap();
        assert_eq!(home.folders(), [PLUGIN_ID]);
        assert!(installed(&home.0));
    }

    #[test]
    fn many_moving_at_once_leave_one_widget_and_never_an_empty_shell() {
        // On the day of an upgrade the bar's polls, the window and a command
        // typed in a terminal can all find the old folder at the same moment.
        // Every one of them has to come out clean, and the shell — which
        // re-reads shell.json the moment it changes — must never find it
        // empty, half-written or missing.
        let home = Home::upgraded_from_old();
        let stop = std::sync::atomic::AtomicBool::new(false);
        let reads = std::sync::atomic::AtomicUsize::new(0);
        std::thread::scope(|scope| {
            let watcher = scope.spawn(|| {
                while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                    let text = std::fs::read_to_string(shell_config(&home.0))
                        .expect("shell.json is always there");
                    let shell: serde_json::Value =
                        serde_json::from_str(&text).expect("and always whole");
                    let right = shell["bar"]["layout"]["right"].as_array().unwrap();
                    assert_eq!(right.len(), 3, "{shell}");
                    assert!(
                        right[1]["id"] == OLD_PLUGIN_ID || right[1]["id"] == PLUGIN_ID,
                        "{shell}"
                    );
                    assert_eq!(right[1]["refreshIntervalSec"], 300, "{shell}");
                    reads.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                }
            });
            let movers: Vec<_> = (0..24).map(|_| scope.spawn(|| migrate(&home.0))).collect();
            // The watcher is stopped before any outcome is judged, or a mover
            // that failed would leave it spinning and the test hanging rather
            // than failing.
            let outcomes: Vec<_> = movers.into_iter().map(|mover| mover.join()).collect();
            stop.store(true, std::sync::atomic::Ordering::Relaxed);
            watcher.join().unwrap();
            for outcome in outcomes {
                outcome.unwrap().unwrap();
            }
        });
        assert!(reads.load(std::sync::atomic::Ordering::Relaxed) > 0, "the watcher looked");

        assert_eq!(home.folders(), [PLUGIN_ID], "one folder, and nothing left over");
        assert!(installed(&home.0));
        let shell = home.shell();
        assert!(!shell.to_string().contains(OLD_PLUGIN_ID), "{shell}");
        assert_eq!(shell["bar"]["layout"]["right"][1]["refreshIntervalSec"], 300);
        let leftovers: Vec<_> = std::fs::read_dir(home.0.join(".config/omarchy"))
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|name| name.contains("omacharts-tmp"))
            .collect();
        assert!(leftovers.is_empty(), "{leftovers:?}");
        for (name, contents) in FILES {
            assert_eq!(
                std::fs::read_to_string(location(&home.0).join(name)).unwrap(),
                *contents,
                "{name}"
            );
        }
    }

    #[test]
    fn a_file_is_replaced_whole_whoever_else_is_writing_it() {
        let home = Home::empty();
        let path = home.0.join("shared.txt");
        let bodies = ["a".repeat(50_000), "b".repeat(50_000)];
        let (path, bodies) = (&path, &bodies);
        std::thread::scope(|scope| {
            let writers: Vec<_> = (0..16)
                .map(|i| {
                    scope.spawn(move || {
                        for _ in 0..20 {
                            replace_file(path, &bodies[i % 2]).unwrap();
                            let seen = std::fs::read_to_string(path).unwrap();
                            assert!(bodies.contains(&seen), "a partial write was seen");
                        }
                    })
                })
                .collect();
            for writer in writers {
                writer.join().unwrap();
            }
        });
        let left: Vec<_> = std::fs::read_dir(&home.0)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|name| name.contains("omacharts-tmp"))
            .collect();
        assert!(left.is_empty(), "temporaries left behind: {left:?}");
    }
}
