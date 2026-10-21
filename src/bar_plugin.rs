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

pub const PLUGIN_ID: &str = "jorgemanrubia.omacharts";

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

/// Write the plugin folder and add it to the bar.
pub fn install(home: &Path) -> Result<(), String> {
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
    edit_shell(home, add_to_layout)
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

/// Ask the shell to recompile its plugins. Best effort, like `notify_changed`.
fn reload_plugins() {
    let _ = std::process::Command::new("omarchy-shell")
        .args(["-q", "shell", "rescanPlugins"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}

/// Take it out of the bar and remove the folder.
pub fn remove(home: &Path) -> Result<(), String> {
    edit_shell(home, remove_from_layout)?;
    let target = location(home);
    if target.exists() {
        std::fs::remove_dir_all(&target).map_err(|e| format!("could not remove it: {e}"))?;
    }
    Ok(())
}

/// Read, change, write — atomically, because a half-written shell.json is a
/// desktop with no bar.
fn edit_shell(home: &Path, change: impl Fn(&mut serde_json::Value) -> bool) -> Result<(), String> {
    let path = shell_config(home);
    let text = std::fs::read_to_string(&path).map_err(|e| format!("shell.json: {e}"))?;
    let mut config: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("shell.json is not readable: {e}"))?;

    if !change(&mut config) {
        return Ok(());
    }

    let rendered = serde_json::to_string_pretty(&config)
        .map_err(|e| format!("could not write shell.json: {e}"))?;
    let temp = path.with_extension("json.omacharts-tmp");
    std::fs::write(&temp, rendered + "\n").map_err(|e| format!("shell.json: {e}"))?;
    std::fs::rename(&temp, &path).map_err(|e| format!("shell.json: {e}"))
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
}
