//! How far a load has got, for a chart waiting on it to say so.
//!
//! A load is the work behind one chart's first bars: a feed's request, or a
//! stream's first snapshot, on whatever thread does it. Whoever starts one
//! [`begin`]s it under a name and runs the work [`within`] it; the work says
//! how far it is with [`advance`], and splits itself into [`part`]s, each a
//! share of its own part, so a stitch of many months and the file each month
//! is read from report into the one number. Whoever waits reads it with
//! [`of`], which never waits on the work.
//!
//! Only a feed that can measure what it reads says anything: Sierra Chart's,
//! from the records it folds. A load nothing has reported on reads as
//! `None`, and is shown as it always was. What is reported only grows.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use crate::bars::Timeframe;

/// What is reported, in parts of this, kept one more than it is so that 0
/// is nothing reported yet.
const WHOLE: u32 = 1_000_000;

/// One load's progress.
#[derive(Debug)]
pub struct Load(AtomicU32);

impl Load {
    /// How far it is, from 0 to 1, once anything has said.
    pub fn fraction(&self) -> Option<f64> {
        match self.0.load(Ordering::Relaxed) {
            0 => None,
            done => Some(f64::from(done - 1) / f64::from(WHOLE)),
        }
    }

    fn reach(&self, fraction: f64) {
        // Only ever forward.
        self.0.fetch_max((fraction.clamp(0.0, 1.0) * f64::from(WHOLE)) as u32 + 1, Ordering::Relaxed);
    }
}

/// The share of a load a piece of work stands for: from `base`, `share` of
/// the whole.
#[derive(Clone, Debug)]
pub struct Part {
    load: Arc<Load>,
    base: f64,
    share: f64,
}

static LOADS: Mutex<Option<HashMap<String, Arc<Load>>>> = Mutex::new(None);

thread_local! {
    static CURRENT: RefCell<Option<Part>> = const { RefCell::new(None) };
}

/// What a load of `symbol` at `timeframe` is known by.
pub fn name(symbol: &str, timeframe: Timeframe) -> String {
    format!("{symbol}|{}", timeframe.label())
}

/// Start a load under `name`, over whatever was there: the whole of it, for
/// [`within`].
pub fn begin(name: &str) -> Part {
    let load = Arc::new(Load(AtomicU32::new(0)));
    LOADS.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(HashMap::new).insert(name.to_string(), load.clone());
    Part { load, base: 0.0, share: 1.0 }
}

/// The load is over: nobody waits on it now.
pub fn end(name: &str) {
    if let Some(loads) = LOADS.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
        loads.remove(name);
    }
}

/// How far the load under `name` has got, if it says. Never waits: while
/// another thread is starting or ending a load this is `None` for a moment.
pub fn of(name: &str) -> Option<f64> {
    let loads = LOADS.try_lock().ok()?;
    loads.as_ref()?.get(name)?.fraction()
}

/// The part of a load this thread is working on, to carry to another.
pub fn current() -> Option<Part> {
    CURRENT.with(|current| current.borrow().clone())
}

/// Run `work` as `part` of its load: what it reports is of that part.
pub fn within<R>(part: Option<Part>, work: impl FnOnce() -> R) -> R {
    let before = CURRENT.with(|current| current.replace(part));
    let result = work();
    CURRENT.with(|current| *current.borrow_mut() = before);
    result
}

/// Run `work` as the share from `base` to `base + share` of the part this
/// thread works on; as it is where there is none.
pub fn part<R>(base: f64, share: f64, work: impl FnOnce() -> R) -> R {
    let Some(now) = current() else { return work() };
    let inner = Part { load: now.load.clone(), base: now.base + now.share * base, share: now.share * share };
    within(Some(inner), work)
}

/// The part this thread works on is `fraction` done.
pub fn advance(fraction: f64) {
    CURRENT.with(|current| {
        if let Some(part) = current.borrow().as_ref() {
            part.load.reach(part.base + part.share * fraction.clamp(0.0, 1.0));
        }
    });
}

/// Is a load being measured on this thread? A feed with nothing to report
/// to does its work as it would anyway.
pub fn measuring() -> bool {
    CURRENT.with(|current| current.borrow().is_some())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// What the load under `name` says, waiting out whoever holds the
    /// loads: [`of`] never waits, so while any other test starts, ends or
    /// reads a load it says nothing, which a test can't tell from a load
    /// that says nothing.
    pub(crate) fn reported(name: &str) -> Option<f64> {
        LOADS.lock().unwrap_or_else(|e| e.into_inner()).as_ref()?.get(name)?.fraction()
    }

    /// What the load under `name` says, which it must.
    pub(crate) fn settled(name: &str) -> f64 {
        reported(name).unwrap_or_else(|| panic!("{name} says nothing"))
    }

    #[test]
    fn a_load_nothing_reports_on_is_unmeasured() {
        let part = begin("quiet|1h");
        within(Some(part), || {});
        assert_eq!(of("quiet|1h"), None);
        end("quiet|1h");
        assert_eq!(of("quiet|1h"), None);
    }

    #[test]
    fn parts_report_into_the_whole_and_it_never_goes_back() {
        let whole = begin("parts|1h");
        let mut seen = Vec::new();
        within(Some(whole), || {
            for month in 0..4 {
                part(f64::from(month) / 4.0, 0.25, || {
                    for step in 1..=4 {
                        advance(f64::from(step) / 4.0);
                        seen.push(settled("parts|1h"));
                    }
                    // A piece read again says less, and is not heard.
                    advance(0.1);
                    seen.push(settled("parts|1h"));
                });
            }
        });
        assert!(seen.windows(2).all(|pair| pair[0] <= pair[1]), "{seen:?}");
        assert!((seen.last().unwrap() - 1.0).abs() < 1e-6);
        assert!((seen[0] - 1.0 / 16.0).abs() < 1e-6);
        end("parts|1h");
    }

    #[test]
    fn it_is_read_from_another_thread_as_it_is_written() {
        let part = begin("threads|1h");
        let worker = std::thread::spawn(move || {
            within(Some(part), || {
                for step in 1..=1000 {
                    advance(f64::from(step) / 1000.0);
                }
            })
        });
        let mut last = 0.0;
        while !worker.is_finished() {
            if let Some(now) = of("threads|1h") {
                assert!(now >= last);
                last = now;
            }
        }
        worker.join().unwrap();
        assert_eq!(settled("threads|1h"), 1.0);
        end("threads|1h");
    }
}
