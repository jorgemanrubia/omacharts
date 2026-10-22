//! Chart templates: named sets of indicators that charts share.
//!
//! A chart using a template does not own its indicators. It shows the
//! template's, and changing the template changes every chart using it — which
//! is the point: one "Default" with a volume pane, and every chart that opens
//! with it follows when somebody decides they want an RSI under it as well.
//!
//! Kept as one JSON setting rather than a table of their own. They are a
//! handful of small values that are always read whole, and a table would be a
//! schema migration — a version an older build refuses to open — for nothing
//! a setting cannot already hold. Being a setting also means a backup carries
//! them and a restore puts them back, with no list of tables to keep in step.
//!
//! Each chart writes down its template's id in the arrangement. The id is
//! handed out once and never again, so renaming a template keeps every chart
//! on it, and a chart left naming a template that has since gone cannot
//! quietly start following a new one that happened to get the same number.

use omacharts_engine::{Indicator, IndicatorKind};
use serde::{Deserialize, Serialize};

use crate::chart_defaults::SETTING_TEMPLATE;
use crate::store::Store;

/// Where every template is kept.
pub const SETTING: &str = "chart_templates";

/// What a new install calls the template its charts open with.
pub const DEFAULT_NAME: &str = "Default";

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Template {
    pub id: u32,
    pub name: String,
    #[serde(default)]
    pub indicators: Vec<Indicator>,
}

/// Everything stored under [`SETTING`].
///
/// The next id travels with the list rather than being worked out from it,
/// because the highest id is exactly the one a deletion frees.
#[derive(Default, Serialize, Deserialize)]
struct Stored {
    #[serde(default)]
    next_id: u32,
    #[serde(default)]
    templates: Vec<Template>,
}

fn read(store: &Store) -> Stored {
    let mut stored: Stored = store
        .setting(SETTING)
        .and_then(|json| serde_json::from_str(&json).ok())
        .unwrap_or_default();
    // A value written by hand, or restored from a backup, may not carry a counter
    // that clears what it holds.
    let highest = stored.templates.iter().map(|t| t.id).max().unwrap_or(0);
    stored.next_id = stored.next_id.max(highest.saturating_add(1));
    stored
}

fn write(store: &Store, stored: &Stored) {
    if let Ok(json) = serde_json::to_string(stored) {
        store.set_setting(SETTING, &json);
    }
}

/// Every template, in the order they were made.
pub fn all(store: &Store) -> Vec<Template> {
    read(store).templates
}

pub fn get(store: &Store, id: u32) -> Option<Template> {
    all(store).into_iter().find(|t| t.id == id)
}

/// A new template with nothing in it yet.
pub fn create(store: &Store, name: &str) -> Template {
    create_with(store, name, Vec::new())
}

/// A new template holding `indicators`: what a chart shows, kept under a name.
pub fn create_from(store: &Store, name: &str, indicators: Vec<Indicator>) -> Template {
    create_with(store, name, indicators)
}

fn create_with(store: &Store, name: &str, indicators: Vec<Indicator>) -> Template {
    let mut stored = read(store);
    let template = Template { id: stored.next_id, name: name.to_string(), indicators };
    stored.next_id = stored.next_id.saturating_add(1);
    stored.templates.push(template.clone());
    write(store, &stored);
    template
}

/// `false` when there is no template with that id.
pub fn rename(store: &Store, id: u32, name: &str) -> bool {
    change(store, id, |t| t.name = name.to_string())
}

/// Replace what a template holds. `false` when there is no such template.
pub fn set_indicators(store: &Store, id: u32, indicators: Vec<Indicator>) -> bool {
    change(store, id, move |t| t.indicators = indicators.clone())
}

fn change(store: &Store, id: u32, edit: impl Fn(&mut Template)) -> bool {
    let mut stored = read(store);
    let Some(found) = stored.templates.iter_mut().find(|t| t.id == id) else { return false };
    edit(found);
    write(store, &stored);
    true
}

/// Take a template out, handing back what it held.
///
/// The charts using it are the caller's to let go of — they live in the
/// arrangement, which a window and a command each keep in their own way. New
/// charts stop opening with it here, because that is a setting like this one.
pub fn delete(store: &Store, id: u32) -> Option<Template> {
    let mut stored = read(store);
    let at = stored.templates.iter().position(|t| t.id == id)?;
    let gone = stored.templates.remove(at);
    write(store, &stored);
    if store.setting(SETTING_TEMPLATE).as_deref() == Some(id.to_string().as_str()) {
        store.set_setting(SETTING_TEMPLATE, crate::chart_defaults::NO_TEMPLATE);
    }
    Some(gone)
}

/// Give a profile its first template, once.
///
/// A new install gets "Default", holding the volume study, and its charts
/// open with it. So does a profile from before there were templates — but its
/// charts are written down without one and keep the indicators they have, so
/// nothing on screen changes; only the next new chart is different.
///
/// Once means once: the setting existing at all is the mark, so somebody who
/// deletes every template does not find "Default" back at the next launch.
/// Asked whether the row exists rather than read, because this runs on every
/// open of the database and the answer is nearly always yes.
pub fn seed(store: &Store) {
    if store.has_setting(SETTING) {
        return;
    }
    let default = create_with(store, DEFAULT_NAME, vec![Indicator::new(1, IndicatorKind::Volume)]);
    // Only when nobody has chosen: an explicit "none" is a choice.
    if !store.has_setting(SETTING_TEMPLATE) {
        store.set_setting(SETTING_TEMPLATE, &default.id.to_string());
    }
}

/// The template a selector names: a name, case-insensitive, or `id:N`.
pub fn find<'a>(templates: &'a [Template], selector: &str) -> Lookup<'a> {
    if let Some(rest) = selector.strip_prefix("id:") {
        return match rest.parse::<u32>().ok().and_then(|id| templates.iter().find(|t| t.id == id)) {
            Some(found) => Lookup::Found(found),
            None => Lookup::Missing,
        };
    }
    let wanted = selector.to_lowercase();
    let hits: Vec<&Template> = templates.iter().filter(|t| t.name.to_lowercase() == wanted).collect();
    match hits.as_slice() {
        [one] => Lookup::Found(one),
        [] => Lookup::Missing,
        many => Lookup::Ambiguous(many.iter().map(|t| t.id).collect()),
    }
}

pub enum Lookup<'a> {
    Found(&'a Template),
    Missing,
    /// Two or more share the name; these are their ids.
    Ambiguous(Vec<u32>),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chart_defaults::ChartDefaults;

    #[test]
    fn templates_are_made_renamed_filled_and_deleted() {
        let store = Store::memory().unwrap();
        let swing = create(&store, "Swing");
        assert!(swing.indicators.is_empty());
        assert!(rename(&store, swing.id, "Swing trading"));
        assert!(set_indicators(&store, swing.id, vec![Indicator::new(1, IndicatorKind::Rsi)]));

        let back = get(&store, swing.id).unwrap();
        assert_eq!(back.name, "Swing trading");
        assert_eq!(back.indicators[0].kind, IndicatorKind::Rsi);

        assert_eq!(delete(&store, swing.id).map(|t| t.name), Some("Swing trading".into()));
        assert!(get(&store, swing.id).is_none());
        assert!(!rename(&store, swing.id, "Gone"), "renamed a template that is not there");
    }

    /// A deleted template's id is never handed out again, so a chart still
    /// naming it cannot start following somebody else's template.
    #[test]
    fn an_id_is_never_handed_out_twice() {
        let store = Store::memory().unwrap();
        let first = create(&store, "One");
        delete(&store, first.id);
        let second = create(&store, "Two");
        assert_ne!(first.id, second.id);
    }

    #[test]
    fn a_new_profile_opens_its_charts_with_a_default_holding_volume() {
        let store = Store::memory().unwrap();
        let templates = all(&store);
        assert_eq!(templates.len(), 1);
        assert_eq!(templates[0].name, DEFAULT_NAME);
        assert_eq!(templates[0].indicators.len(), 1);
        assert_eq!(templates[0].indicators[0].kind, IndicatorKind::Volume);
        assert_eq!(ChartDefaults::load(&store).template, Some(templates[0].id));
    }

    #[test]
    fn seeding_happens_once_however_often_it_is_asked() {
        let store = Store::memory().unwrap();
        seed(&store);
        seed(&store);
        assert_eq!(all(&store).len(), 1);

        // Deleting every template is a choice the next launch keeps.
        for template in all(&store) {
            delete(&store, template.id);
        }
        seed(&store);
        assert!(all(&store).is_empty());
        assert_eq!(ChartDefaults::load(&store).template, None);
    }

    /// A profile from before templates gets "Default" too, and a choice it
    /// had already made about what new charts open with is left alone.
    #[test]
    fn an_existing_profile_keeps_a_default_it_had_chosen() {
        let store = Store::memory().unwrap();
        store.remove_setting(SETTING);
        store.set_setting(SETTING_TEMPLATE, crate::chart_defaults::NO_TEMPLATE);
        seed(&store);
        assert_eq!(all(&store).len(), 1, "an existing profile gets Default as well");
        assert_eq!(ChartDefaults::load(&store).template, None, "the chosen None was overwritten");

        let store = Store::memory().unwrap();
        store.remove_setting(SETTING);
        store.remove_setting(SETTING_TEMPLATE);
        seed(&store);
        let default = all(&store)[0].id;
        assert_eq!(ChartDefaults::load(&store).template, Some(default));
    }

    #[test]
    fn deleting_the_default_template_puts_new_charts_back_on_none() {
        let store = Store::memory().unwrap();
        let default = all(&store)[0].id;
        delete(&store, default);
        assert_eq!(ChartDefaults::load(&store).template, None);
    }

    #[test]
    fn a_selector_is_a_name_or_an_id() {
        let store = Store::memory().unwrap();
        let swing = create(&store, "Swing");
        create(&store, "Twin");
        create(&store, "twin");
        let templates = all(&store);
        assert!(matches!(find(&templates, "swing"), Lookup::Found(t) if t.id == swing.id));
        assert!(matches!(find(&templates, &format!("id:{}", swing.id)), Lookup::Found(_)));
        assert!(matches!(find(&templates, "Twin"), Lookup::Ambiguous(ids) if ids.len() == 2));
        assert!(matches!(find(&templates, "nope"), Lookup::Missing));
        assert!(matches!(find(&templates, "id:99"), Lookup::Missing));
    }

    #[test]
    fn a_broken_value_reads_as_no_templates() {
        let store = Store::memory().unwrap();
        store.set_setting(SETTING, "{ not json");
        assert!(all(&store).is_empty());
        // And making one does not trip over it.
        assert_eq!(create(&store, "Fresh").name, "Fresh");
    }
}
