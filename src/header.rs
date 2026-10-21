//! What a chart's header shows: the line at its top-left, and in what order.
//!
//! Each chart has its own, written down with it; a new chart takes the one
//! the preferences name. Symbol and resolution are what every chart showed
//! before there was a choice, so they are what a chart written before then
//! comes back with.

use crate::store::Store;

/// The header new charts open with.
pub const SETTING: &str = "chart_header";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Part {
    Symbol,
    Resolution,
    /// The day's move, as a percentage, in the chart's own up and down colours.
    Change,
    /// The name of the chart template the chart is on; nothing when it is on
    /// none.
    Template,
}

impl Part {
    pub const ALL: [Part; 4] = [Part::Symbol, Part::Resolution, Part::Change, Part::Template];

    /// How it is written down and typed.
    pub fn key(self) -> &'static str {
        match self {
            Part::Symbol => "symbol",
            Part::Resolution => "resolution",
            Part::Change => "change",
            Part::Template => "template",
        }
    }

    pub fn from_key(key: &str) -> Option<Part> {
        Part::ALL.into_iter().find(|part| part.key() == key)
    }

    /// How a list of them names it.
    pub fn label(self) -> &'static str {
        match self {
            Part::Symbol => "Symbol",
            Part::Resolution => "Resolution",
            Part::Change => "Daily change",
            Part::Template => "Chart template",
        }
    }
}

/// What a chart shows when nobody has chosen.
pub fn standard() -> Vec<Part> {
    vec![Part::Symbol, Part::Resolution]
}

/// The parts named in `keys`, in order, skipping any this build does not
/// know — a header written by a newer build keeps what this one can show
/// rather than losing the whole arrangement it was stored in.
pub fn from_keys<S: AsRef<str>>(keys: &[S]) -> Vec<Part> {
    let mut parts: Vec<Part> = Vec::new();
    for part in keys.iter().filter_map(|key| Part::from_key(key.as_ref())) {
        if !parts.contains(&part) {
            parts.push(part);
        }
    }
    parts
}

pub fn keys(parts: &[Part]) -> Vec<String> {
    parts.iter().map(|part| part.key().to_string()).collect()
}

/// `symbol,resolution,change`, as typed; `none` for an empty header.
///
/// Refuses a name it does not know and a part named twice, rather than
/// dropping either: a header that came out different from the one typed would
/// look as though the command worked.
pub fn parse(text: &str) -> Result<Vec<Part>, String> {
    let text = text.trim();
    if text.is_empty() || text.eq_ignore_ascii_case("none") {
        return Ok(Vec::new());
    }
    let mut parts = Vec::new();
    for key in text.split(',').map(|key| key.trim().to_lowercase()) {
        let Some(part) = Part::from_key(&key) else {
            return Err(format!(
                "{key:?} is not part of a header; try {}",
                Part::ALL.map(Part::key).join(", ")
            ));
        };
        if parts.contains(&part) {
            return Err(format!("{key} is named twice"));
        }
        parts.push(part);
    }
    Ok(parts)
}

/// The way [`parse`] reads it.
pub fn spell(parts: &[Part]) -> String {
    match parts.is_empty() {
        true => "none".to_string(),
        false => keys(parts).join(","),
    }
}

/// The header new charts open with.
pub fn load_default(store: &Store) -> Vec<Part> {
    store.setting(SETTING).and_then(|text| parse(&text).ok()).unwrap_or_else(standard)
}

pub fn save_default(store: &Store, parts: &[Part]) {
    store.set_setting(SETTING, &spell(parts));
}

/// A day's move as the header writes it: `+1.25%`.
pub fn change_text(change_pct: f64) -> String {
    format!("{change_pct:+.2}%")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_header_is_typed_and_written_the_same_way() {
        let parts = parse("Symbol, change,template").unwrap();
        assert_eq!(parts, vec![Part::Symbol, Part::Change, Part::Template]);
        assert_eq!(spell(&parts), "symbol,change,template");
        assert_eq!(parse("none").unwrap(), Vec::new());
        assert_eq!(spell(&[]), "none");
        assert!(parse("symbol,price").is_err());
        assert!(parse("symbol,symbol").is_err());
    }

    #[test]
    fn what_is_saved_is_what_new_charts_open_with() {
        let store = Store::memory().unwrap();
        assert_eq!(load_default(&store), standard());
        save_default(&store, &[Part::Change, Part::Symbol]);
        assert_eq!(load_default(&store), vec![Part::Change, Part::Symbol]);
        save_default(&store, &[]);
        assert!(load_default(&store).is_empty());
    }

    /// A newer build's part is dropped, not the header: the rest still shows.
    #[test]
    fn a_part_this_build_does_not_know_is_left_out() {
        assert_eq!(from_keys(&["symbol", "volume", "symbol", "change"]), vec![Part::Symbol, Part::Change]);
    }

    #[test]
    fn a_change_reads_with_its_sign() {
        assert_eq!(change_text(-1.0), "-1.00%");
        assert_eq!(change_text(0.256), "+0.26%");
    }
}
