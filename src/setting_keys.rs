//! What each setting is, for anything that copies settings from one place to
//! another.
//!
//! Most settings are somebody's choices and go wherever their settings go.
//! A few belong to this machine — where its screenshots are saved, how much
//! disk its cache may take, how wide its sidebar is on its screen — and a
//! settings file brought from another machine must not overwrite them. And a
//! few are the app's notes to itself, written as it runs, which change
//! nothing anybody chose and must not make a backup look necessary.
//!
//! Every key the app writes is listed here once. A key that is not is shared,
//! which is the safe answer for a backup, and the test at the bottom fails the
//! moment a new one is written anywhere without being given a kind.

/// What a setting is to a backup and to a restore from another machine.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    /// Somebody's choice: backed up, compared, and carried between machines.
    Shared,
    /// This machine's: backed up, but kept as it is when settings arrive
    /// from elsewhere.
    Local,
    /// The app's own note, rewritten as it runs: not a change worth a backup,
    /// and this machine's to keep.
    Bookkeeping,
}

/// The keys that are not shared, with what they are. A key ending in `.` or
/// `_` names every key that starts with it.
const KEYS: &[(&str, Kind)] = &[
    ("screenshot_folder", Kind::Local),
    ("cache_limit", Kind::Local),
    ("sidebar_width", Kind::Local),
    ("cache_swept_at", Kind::Bookkeeping),
    ("cooldown.", Kind::Bookkeeping),
    ("last_symbol", Kind::Bookkeeping),
    ("last_suffix", Kind::Bookkeeping),
    ("last_timeframe", Kind::Bookkeeping),
    ("bar_widget_offered", Kind::Bookkeeping),
    ("active_watchlist", Kind::Bookkeeping),
    ("show_watchlist", Kind::Bookkeeping),
    ("show_drawing_tools", Kind::Bookkeeping),
];

/// Every key the app writes that is shared, named so the test below can tell
/// a key nobody classified from one that was meant to be shared.
#[cfg(test)]
const SHARED: &[&str] = &[
    "theme",
    "bar_scheme",
    "coloured_bar_scheme",
    "workspace",
    "timeframes",
    "indicators",
    "provider",
    "sync_crosshair",
    "drawing_configurations",
    "screenshot_save_file",
    "watchlist_columns",
    "watchlist_sort_",
    "watchlist_link_",
    "bar_style",
    "chart_session",
    "show_grid",
    "chart_log_scale",
    "chart_shows_drawings",
    "chart_sends_drawings",
    "chart_template",
    "chart_templates",
    "chart_header",
];

pub fn kind(key: &str) -> Kind {
    KEYS.iter()
        .find(|(name, _)| matches(name, key))
        .map(|(_, kind)| *kind)
        .unwrap_or(Kind::Shared)
}

fn matches(name: &str, key: &str) -> bool {
    match name.ends_with('.') || name.ends_with('_') {
        true => key.starts_with(name),
        false => key == name,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_are_told_apart_by_name_and_by_prefix() {
        assert_eq!(kind("theme"), Kind::Shared);
        assert_eq!(kind("screenshot_folder"), Kind::Local);
        assert_eq!(kind("cooldown.yahoo"), Kind::Bookkeeping);
        assert_eq!(kind("watchlist_link_3"), Kind::Shared);
        assert_eq!(kind("something new"), Kind::Shared);
        // Somebody's own sets of indicators, and which one new charts use.
        assert_eq!(kind("chart_templates"), Kind::Shared);
        assert_eq!(kind("chart_template"), Kind::Shared);
        assert_eq!(kind("chart_header"), Kind::Shared);
    }

    /// Every key the app writes has been given a kind on purpose.
    ///
    /// Read off the source: every `set_setting` and `set_setting_bool` outside
    /// a test, its key resolved through the `const` it names. A key built by a
    /// function is checked by the prefix that function writes. Adding a
    /// setting without deciding whether it travels between machines fails
    /// here, which is the point.
    #[test]
    fn every_key_written_is_classified() {
        let sources = rust_sources(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src"));
        let consts: Vec<(String, String)> = sources.iter().flat_map(|text| string_consts(text)).collect();
        // Keys made by a function, by the prefix the function writes; and
        // the few places that pass along a key somebody else chose.
        let built = [
            ("link_setting(", "watchlist_link_"),
            ("sort_setting(", "watchlist_sort_"),
            ("cooldown_key(", "cooldown."),
        ];
        let passed_along = ["key", "&key", "&self", "KEY"];

        let mut unclassified = Vec::new();
        for text in &sources {
            // Up to the tests module only: a `#[cfg(test)]` helper earlier in
            // the file must not hide the code after it.
            let live = text.split("#[cfg(test)]\nmod tests").next().unwrap_or("");
            for argument in written_keys(live) {
                let argument = argument.trim();
                if passed_along.contains(&argument) {
                    continue;
                }
                let keys: Vec<String> = if let Some(literal) = argument.strip_prefix('"') {
                    vec![literal.trim_end_matches('"').to_string()]
                } else if let Some((_, prefix)) =
                    built.iter().find(|(call, _)| argument.trim_start_matches('&').starts_with(call))
                {
                    vec![prefix.to_string()]
                } else {
                    let name = argument.rsplit("::").next().unwrap_or(argument);
                    consts.iter().filter(|(c, _)| c == name).map(|(_, v)| v.clone()).collect()
                };
                if keys.is_empty() {
                    unclassified.push(format!("{argument} (not resolved to a key)"));
                }
                for key in keys {
                    let classified = KEYS.iter().any(|(name, _)| matches(name, &key))
                        || SHARED.iter().any(|name| matches(name, &key));
                    if !classified {
                        unclassified.push(key);
                    }
                }
            }
        }
        unclassified.sort();
        unclassified.dedup();
        assert!(
            unclassified.is_empty(),
            "classify these in src/setting_keys.rs: {unclassified:?}"
        );
    }

    fn rust_sources(dir: std::path::PathBuf) -> Vec<String> {
        let mut out = Vec::new();
        let Ok(entries) = std::fs::read_dir(&dir) else { return out };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                out.extend(rust_sources(path));
            } else if path.extension().is_some_and(|e| e == "rs")
                && path.file_name().is_some_and(|n| n != "setting_keys.rs")
            {
                out.push(std::fs::read_to_string(&path).unwrap_or_default());
            }
        }
        out
    }

    /// `const NAME: &str = "value";`, anywhere in a file.
    fn string_consts(text: &str) -> Vec<(String, String)> {
        text.lines()
            .filter_map(|line| {
                let rest = line.trim().trim_start_matches("pub(crate) ").trim_start_matches("pub ");
                let rest = rest.strip_prefix("const ")?;
                let (name, rest) = rest.split_once(": &str = \"")?;
                let value = rest.strip_suffix("\";")?;
                Some((name.to_string(), value.to_string()))
            })
            .collect()
    }

    /// The first argument of every settings write in `text`.
    fn written_keys(text: &str) -> Vec<String> {
        let mut out = Vec::new();
        for call in ["set_setting_bool(", "set_setting("] {
            let mut rest = text;
            while let Some(at) = rest.find(call) {
                rest = &rest[at + call.len()..];
                // `fn set_setting(&self, key: …` is the definition, not a write.
                let argument: String = rest
                    .chars()
                    .scan(0i32, |depth, c| {
                        match c {
                            '(' => *depth += 1,
                            ')' if *depth == 0 => return None,
                            ')' => *depth -= 1,
                            ',' if *depth == 0 => return None,
                            _ => {}
                        }
                        Some(c)
                    })
                    .collect();
                if !argument.trim().is_empty() {
                    out.push(argument);
                }
            }
        }
        out
    }
}
