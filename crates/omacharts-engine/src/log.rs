//! What the app has to say about its feeds, its brokers and itself, kept for
//! the Log window to show.
//!
//! An in-memory ring of the last [`CAPACITY`] entries: nothing is written to
//! disk, and the oldest falls off the front as the newest goes on the back,
//! so it never grows. The same thing said twice in a row by the same source
//! is one entry said ×N, with the time of the last.
//!
//! Appending is a lock, a push and a `try_send` on a channel of one, from any
//! thread. Whoever shows the log waits on [`Log::woken`] and asks
//! [`Log::since`] what changed: no timer, no polling, the way the desk's
//! sessions wake the window.
//!
//! The words are the app's to choose (`src/log_words.rs`); this only keeps
//! them. What it does do is hold back the words it was told to conceal, such
//! as a login's user ID, from every entry put in it.

use std::collections::VecDeque;
use std::sync::{Mutex, OnceLock};

use chrono::{DateTime, Local};

/// How many entries the ring keeps.
pub const CAPACITY: usize = 1000;

/// What a concealed word is replaced by.
const CONCEALED: &str = "…";

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Hash)]
pub enum Level {
    Info,
    Warning,
    Error,
}

impl Level {
    pub fn word(self) -> &'static str {
        match self {
            Level::Info => "Info",
            Level::Warning => "Warning",
            Level::Error => "Error",
        }
    }
}

/// One thing said.
#[derive(Clone, PartialEq, Debug)]
pub struct Entry {
    /// Its place in the log, counting from 1 since the process started;
    /// kept by a repeat.
    pub id: u64,
    /// When it was last said.
    pub at: DateTime<Local>,
    pub level: Level,
    /// Who says it: a feed's or a login's name, never a user ID.
    pub source: String,
    /// In a trader's words.
    pub text: String,
    /// What the feed or the system said, word for word, behind "Details".
    pub detail: Option<String>,
    /// How many times in a row it was said: 1 the first time.
    pub repeats: u32,
    /// Something somebody asked for that was done: said in a toast as well.
    pub confirms: bool,
    /// Bumped by every change to the log, so that [`Log::since`] can say
    /// what changed after a point.
    pub revision: u64,
}

impl Entry {
    pub fn new(level: Level, source: impl Into<String>, text: impl Into<String>) -> Entry {
        Entry {
            id: 0,
            at: Local::now(),
            level,
            source: source.into(),
            text: text.into(),
            detail: None,
            repeats: 1,
            confirms: false,
            revision: 0,
        }
    }

    pub fn info(source: impl Into<String>, text: impl Into<String>) -> Entry {
        Entry::new(Level::Info, source, text)
    }

    pub fn warning(source: impl Into<String>, text: impl Into<String>) -> Entry {
        Entry::new(Level::Warning, source, text)
    }

    pub fn error(source: impl Into<String>, text: impl Into<String>) -> Entry {
        Entry::new(Level::Error, source, text)
    }

    /// With what was said word for word, when it adds anything.
    pub fn detail(mut self, detail: impl Into<String>) -> Entry {
        let detail = detail.into();
        if !detail.is_empty() && detail != self.text {
            self.detail = Some(detail);
        }
        self
    }

    /// As something asked for and done.
    pub fn confirming(mut self) -> Entry {
        self.confirms = true;
        self
    }

    /// "Rithmic Test · Signed in · 2 accounts (×3)".
    pub fn line(&self) -> String {
        let mut line = format!("{} · {}", self.source, self.text);
        if self.repeats > 1 {
            line.push_str(&format!(" (×{})", self.repeats));
        }
        line
    }
}

struct Ring {
    entries: VecDeque<Entry>,
    next_id: u64,
    revision: u64,
    concealed: Vec<String>,
}

pub struct Log {
    capacity: usize,
    ring: Mutex<Ring>,
    wake: async_channel::Sender<()>,
    woken: async_channel::Receiver<()>,
}

impl Log {
    pub fn new(capacity: usize) -> Log {
        // One at most: a wake already waiting says everything a second
        // would.
        let (wake, woken) = async_channel::bounded(1);
        Log {
            capacity,
            ring: Mutex::new(Ring { entries: VecDeque::new(), next_id: 1, revision: 0, concealed: Vec::new() }),
            wake,
            woken,
        }
    }

    fn ring(&self) -> std::sync::MutexGuard<'_, Ring> {
        self.ring.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Never let `word` into an entry from now on. Words of fewer than
    /// three characters are left alone, or a user ID of "a" would take
    /// every "a" out of every sentence.
    pub fn conceal(&self, word: &str) {
        let word = word.trim();
        if word.chars().count() < 3 {
            return;
        }
        let mut ring = self.ring();
        if !ring.concealed.iter().any(|known| known == word) {
            ring.concealed.push(word.to_string());
            // The longest first, so that "trader-2" is not left as "…-2"
            // by "trader" going before it.
            ring.concealed.sort_by_key(|known| std::cmp::Reverse(known.len()));
        }
    }

    /// Put `entry` on the end, or count it again when it is what was said
    /// last by the same source.
    pub fn add(&self, mut entry: Entry) {
        {
            let mut ring = self.ring();
            for word in &ring.concealed {
                hide(&mut entry.source, word);
                hide(&mut entry.text, word);
                if let Some(detail) = entry.detail.as_mut() {
                    hide(detail, word);
                }
            }
            ring.revision += 1;
            let revision = ring.revision;
            if let Some(last) = ring.entries.back_mut()
                && last.source == entry.source
                && last.text == entry.text
                && last.level == entry.level
            {
                last.repeats += 1;
                last.at = entry.at;
                last.detail = entry.detail.or(last.detail.take());
                last.revision = revision;
            } else {
                entry.id = ring.next_id;
                ring.next_id += 1;
                entry.repeats = 1;
                entry.revision = revision;
                ring.entries.push_back(entry);
                while ring.entries.len() > self.capacity {
                    ring.entries.pop_front();
                }
            }
        }
        let _ = self.wake.try_send(());
    }

    /// Every entry, oldest first.
    pub fn entries(&self) -> Vec<Entry> {
        self.ring().entries.iter().cloned().collect()
    }

    /// The entries added or counted again after `revision`, oldest first,
    /// and the revision to ask from next time. Only the newest entry is ever
    /// counted again, so they are always the end of the log.
    pub fn since(&self, revision: u64) -> (Vec<Entry>, u64) {
        let ring = self.ring();
        let fresh = ring.entries.iter().rev().take_while(|entry| entry.revision > revision).count();
        let entries = ring.entries.iter().skip(ring.entries.len() - fresh).cloned().collect();
        (entries, ring.revision)
    }

    /// The revision now, without copying anything.
    pub fn revision(&self) -> u64 {
        self.ring().revision
    }

    /// Forget every entry. Ids go on counting.
    pub fn clear(&self) {
        let mut ring = self.ring();
        ring.entries.clear();
        ring.revision += 1;
    }

    /// What wakes whoever shows the log: one message whenever anything was
    /// added since it last looked.
    pub fn woken(&self) -> async_channel::Receiver<()> {
        self.woken.clone()
    }
}

fn hide(text: &mut String, word: &str) {
    if text.contains(word) {
        *text = text.replace(word, CONCEALED);
    }
}

/// The app's one log.
pub fn global() -> &'static Log {
    static LOG: OnceLock<Log> = OnceLock::new();
    LOG.get_or_init(|| Log::new(CAPACITY))
}

/// Add `entry` to the app's log.
pub fn add(entry: Entry) {
    global().add(entry);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_ring_keeps_the_newest_thousand_in_order() {
        let log = Log::new(CAPACITY);
        for n in 0..CAPACITY + 25 {
            log.add(Entry::info("Yahoo", format!("line {n}")));
        }
        let entries = log.entries();
        assert_eq!(entries.len(), CAPACITY);
        assert_eq!(entries.first().unwrap().text, "line 25", "the oldest went first");
        assert_eq!(entries.last().unwrap().text, format!("line {}", CAPACITY + 24));
        assert!(entries.windows(2).all(|pair| pair[0].id < pair[1].id));
    }

    #[test]
    fn the_same_thing_said_again_is_counted_with_the_later_time() {
        let log = Log::new(CAPACITY);
        log.add(Entry::warning("Yahoo", "The data feed is not answering."));
        let first = log.entries()[0].clone();
        std::thread::sleep(std::time::Duration::from_millis(2));
        log.add(Entry::warning("Yahoo", "The data feed is not answering."));
        log.add(Entry::warning("Yahoo", "The data feed is not answering."));
        let entries = log.entries();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].repeats, 3);
        assert_eq!(entries[0].id, first.id);
        assert!(entries[0].at > first.at, "the time is the last one's");
        assert_eq!(entries[0].line(), "Yahoo · The data feed is not answering. (×3)");

        // Another source, or something else said, is a new entry; and the
        // same again after it is new too, since it was not said last.
        log.add(Entry::warning("Rithmic Test", "The data feed is not answering."));
        log.add(Entry::warning("Yahoo", "The data feed is not answering."));
        assert_eq!(log.entries().len(), 3);
    }

    #[test]
    fn since_says_what_was_added_or_counted_again() {
        let log = Log::new(CAPACITY);
        log.add(Entry::info("Yahoo", "one"));
        let (all, at) = log.since(0);
        assert_eq!(all.len(), 1);
        log.add(Entry::info("Yahoo", "two"));
        log.add(Entry::info("Yahoo", "two"));
        let (fresh, later) = log.since(at);
        assert_eq!(fresh.iter().map(|e| (e.text.as_str(), e.repeats)).collect::<Vec<_>>(), [("two", 2)]);
        assert!(log.since(later).0.is_empty());
        log.clear();
        assert!(log.entries().is_empty());
    }

    #[test]
    fn a_concealed_word_never_reaches_an_entry() {
        let log = Log::new(CAPACITY);
        log.conceal("trader-4417");
        log.conceal("ab");
        log.add(Entry::error("trader-4417 on Test", "Refused trader-4417").detail("user trader-4417 is locked"));
        let entry = &log.entries()[0];
        assert!(!format!("{entry:?}").contains("trader-4417"));
        assert_eq!(entry.text, "Refused …");
        log.add(Entry::info("Yahoo", "about"));
        assert_eq!(log.entries()[1].text, "about", "too short to conceal");
        log.conceal("trader");
        log.conceal("trader-2");
        log.add(Entry::info("trader-2", "x"));
        assert_eq!(log.entries()[2].source, "…", "a longer word left half shown");
    }

    #[test]
    fn adding_wakes_once_however_much_is_added() {
        let log = Log::new(CAPACITY);
        let woken = log.woken();
        for _ in 0..5 {
            log.add(Entry::info("Yahoo", "x"));
        }
        assert!(woken.try_recv().is_ok());
        assert!(woken.try_recv().is_err());
    }
}
