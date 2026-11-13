//! Feature flags: parts of the app that are switched on from the command
//! line, kept experimental until they are ready for everybody.
//!
//! One table, [`FLAGS`], names every flag the app knows and whether it is on
//! when nobody has said. Each is stored as a setting of its own,
//! `feature.<name>`, `Local` in `setting_keys`: an experiment is switched on
//! on a machine, not carried to the next one by a restore.
//!
//! Asking whether a flag is on costs a load of a plain bool. The settings are
//! read into it once, at start ([`adopt`]), and written through it when a
//! flag is switched ([`set`]); nothing asks the database while the app runs.
//!
//! `omacharts features` lists them and switches them. It is left out of
//! every listing on purpose — `--help`, `surface --json`, the completions,
//! the man page, the skill and the manual — so that an experiment is not
//! advertised: see `doc/prop-trading/spec.md`.
//!
//! A flag goes when its part ships. Its name stays in [`RETIRED`], so that
//! `features enable` says what became of it, and a store that still holds
//! its setting is not read for it. Trading was the first: it is on for
//! everybody, with no flag.

use crate::store::Store;

/// A flag the app knows.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Feature {
    /// Sierra Chart: its data feed, read from its files, on the Data page
    /// and in `provider` commands, with `provider setup`; and its trading
    /// service. Off, nothing looks for its folder, watches a
    /// file or connects to its DTC server.
    Sierra,
}

/// What a flag is called on the command line, whether it is on when
/// nobody has said, and what it switches.
pub struct Flag {
    pub feature: Feature,
    pub name: &'static str,
    pub default: bool,
    pub about: &'static str,
}

/// Every flag, in the order `omacharts features` lists them.
pub const FLAGS: &[Flag] = &[Flag {
    feature: Feature::Sierra,
    name: "sierra",
    default: false,
    about: "Sierra Chart as a data feed, from its own files, and as a trading service",
}];

/// Flags there were once, and what became of each: a name somebody may
/// still type, answered with what it is now rather than "no such flag".
/// Each one's setting, `feature.<name>`, may still be in a store from
/// before, and is read by nothing.
pub const RETIRED: &[(&str, &str)] = &[("trading", "trading is always on; it is no longer a feature flag")];

/// What became of the retired flag called `name`, as typed.
pub fn retired(name: &str) -> Option<&'static str> {
    RETIRED.iter().find(|(old, _)| old.eq_ignore_ascii_case(name.trim())).map(|(_, now)| *now)
}

/// What every flag's setting starts with.
pub const PREFIX: &str = "feature.";

impl Feature {
    pub fn flag(self) -> &'static Flag {
        FLAGS.iter().find(|flag| flag.feature == self).expect("every feature is in FLAGS")
    }

    fn index(self) -> usize {
        FLAGS.iter().position(|flag| flag.feature == self).expect("every feature is in FLAGS")
    }

    /// The setting it is stored under.
    pub fn setting(self) -> String {
        format!("{PREFIX}{}", self.flag().name)
    }
}

/// The flag called `name`, as typed.
pub fn named(name: &str) -> Option<&'static Flag> {
    FLAGS.iter().find(|flag| flag.name.eq_ignore_ascii_case(name.trim()))
}

/// Room for as many flags as the table can grow to; one bool each.
const ROOM: usize = 8;

const _: () = assert!(FLAGS.len() <= ROOM, "make ROOM larger");

// The process's answer, held where a check is a load. Under test each
// thread has its own, because libtest runs tests side by side and a window
// test switching a flag on must not reach a test running beside it.
#[cfg(not(test))]
mod current {
    use std::sync::atomic::{AtomicBool, Ordering};

    use super::{FLAGS, ROOM};

    static ON: [AtomicBool; ROOM] = [const { AtomicBool::new(false) }; ROOM];
    static READ: AtomicBool = AtomicBool::new(false);

    pub fn get(at: usize) -> bool {
        match READ.load(Ordering::Relaxed) {
            true => ON[at].load(Ordering::Relaxed),
            false => FLAGS[at].default,
        }
    }

    pub fn put(values: &[(usize, bool)]) {
        for (at, on) in values {
            ON[*at].store(*on, Ordering::Relaxed);
        }
        READ.store(true, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod current {
    use std::cell::Cell;

    use super::{FLAGS, ROOM};

    thread_local! {
        static ON: [Cell<Option<bool>>; ROOM] = const { [const { Cell::new(None) }; ROOM] };
    }

    pub fn get(at: usize) -> bool {
        ON.with(|on| on[at].get()).unwrap_or(FLAGS[at].default)
    }

    pub fn put(values: &[(usize, bool)]) {
        ON.with(|on| {
            for (at, value) in values {
                on[*at].set(Some(*value));
            }
        });
    }
}

/// Is `feature` on? A load of a bool, for anywhere.
#[inline]
pub fn on(feature: Feature) -> bool {
    current::get(feature.index())
}

/// Is Sierra Chart on?
#[inline]
pub fn sierra() -> bool {
    on(Feature::Sierra)
}

/// Have the engine ask these flags whether a feed behind one is open: see
/// `omacharts_engine::providers::gate_with`. Every way of reading the
/// flags in calls it, so no feed is asked about before.
fn gate_feeds() {
    omacharts_engine::providers::gate_with(|name| named(name).is_some_and(|flag| on(flag.feature)));
}

/// Switch `feature` for this thread alone, as a test needs it, with nothing
/// written down.
#[cfg(test)]
pub fn force(feature: Feature, on: bool) {
    gate_feeds();
    current::put(&[(feature.index(), on)]);
}

/// Every flag back to its default, for this thread alone: what each test on
/// the GTK thread starts from.
#[cfg(test)]
pub fn reset() {
    current::put(&FLAGS.iter().enumerate().map(|(at, flag)| (at, flag.default)).collect::<Vec<_>>());
}

/// What the settings say of `feature`, read from the store.
pub fn stored(store: &Store, feature: Feature) -> bool {
    store.setting_bool(&feature.setting(), feature.flag().default)
}

/// Read every flag from the settings into what [`on`] answers: at start,
/// and before a command runs in a process with no window.
pub fn adopt(store: &Store) {
    gate_feeds();
    let values: Vec<(usize, bool)> =
        FLAGS.iter().enumerate().map(|(at, flag)| (at, stored(store, flag.feature))).collect();
    current::put(&values);
}

/// Switch `feature` on or off, in the settings and for this process. True
/// when that changed anything.
pub fn set(store: &Store, feature: Feature, on: bool) -> bool {
    let was = stored(store, feature);
    let key = feature.setting();
    match on == feature.flag().default {
        true => store.remove_setting(&key),
        false => store.set_setting_bool(&key, on),
    }
    gate_feeds();
    current::put(&[(feature.index(), on)]);
    was != on
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_flag_has_a_name_of_its_own_and_its_setting() {
        for (at, flag) in FLAGS.iter().enumerate() {
            assert!(!flag.name.is_empty() && !flag.about.is_empty());
            assert_eq!(flag.name, flag.name.to_lowercase(), "typed in lower case");
            assert!(!flag.name.contains(char::is_whitespace));
            assert!(FLAGS.iter().skip(at + 1).all(|other| other.name != flag.name), "{} twice", flag.name);
            assert!(retired(flag.name).is_none(), "{} is retired and a flag", flag.name);
            assert_eq!(flag.feature.flag().name, flag.name);
            assert_eq!(flag.feature.setting(), format!("feature.{}", flag.name));
        }
        assert_eq!(Feature::Sierra.setting(), "feature.sierra");
    }

    #[test]
    fn sierra_is_off_until_somebody_switches_it_on() {
        assert!(!Feature::Sierra.flag().default);
        let store = Store::memory().expect("a store");
        adopt(&store);
        assert!(!sierra());
        assert!(!stored(&store, Feature::Sierra));
        assert!(store.setting(&Feature::Sierra.setting()).is_none(), "nothing written by reading");
    }

    #[test]
    fn switching_a_flag_is_kept_and_read_back() {
        let store = Store::memory().expect("a store");
        assert!(set(&store, Feature::Sierra, true));
        assert!(sierra());
        assert!(!set(&store, Feature::Sierra, true), "already on");
        // Another process starting reads it from the settings.
        current::put(&[(Feature::Sierra.index(), false)]);
        adopt(&store);
        assert!(sierra());
        assert!(set(&store, Feature::Sierra, false));
        assert!(!sierra());
        assert!(store.setting(&Feature::Sierra.setting()).is_none(), "off is the default, so nothing is kept");
    }

    /// A store from when trading was behind a flag may still say so. It
    /// is read by nothing and stops nothing from starting.
    #[test]
    fn a_trading_flag_left_in_the_store_is_ignored() {
        let store = Store::memory().expect("a store");
        store.set_setting_bool("feature.trading", false);
        store.set_setting_bool(&Feature::Sierra.setting(), true);
        adopt(&store);
        assert!(sierra());
        assert!(named("trading").is_none());
        assert!(retired(" Trading ").is_some_and(|now| now.contains("always on")));
    }

    #[test]
    fn flags_are_found_by_name_as_typed() {
        assert_eq!(named("sierra").map(|flag| flag.feature), Some(Feature::Sierra));
        assert_eq!(named(" Sierra ").map(|flag| flag.feature), Some(Feature::Sierra));
        assert!(named("sierr").is_none());
        assert!(named("").is_none());
    }
}
