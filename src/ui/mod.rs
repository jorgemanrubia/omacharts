//! The widgets.

pub mod chart;
pub mod chart_settings;
pub mod colors;
pub mod dialogs;
pub mod drawing_settings;
pub mod pane;
pub mod palette;
pub mod preferences;
pub mod screenshot;
pub mod search;
pub mod shortcuts;
pub mod watchlist;
pub mod window;

pub use chart::ChartView;
pub use window::Window;

/// Whether widgets can be built on this thread.
///
/// Two bargains in one. There may be no display, in which case there is
/// nothing to check — that is the usual answer on CI. And libtest gives every
/// test its own thread even at `--test-threads=1`, so the first test to reach
/// GTK claims it and `gtk::init` from the next one panics with "attempted to
/// initialize GTK from two different threads". Asking first turns that into a
/// skip, which is what the display-less case already does.
///
/// And GTK is asked exactly once. On a runner with no display `gtk::init`
/// fails and leaves GTK uninitialised, so a second test asking again would
/// call it again — and the second attempt segfaults inside GTK, which ends
/// the whole test binary rather than failing an assertion. One GTK test
/// never hit it; the second did, on CI and anywhere else without a display.
/// So the first attempt's answer is kept, and every later test on any other
/// thread gets a plain no.
#[cfg(test)]
pub fn gtk_ready() -> bool {
    if gtk::is_initialized_main_thread() {
        return true;
    }
    static ASKED: std::sync::Mutex<Option<bool>> = std::sync::Mutex::new(None);
    let mut asked = ASKED.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    match *asked {
        // Either it failed, or it succeeded on a thread that is not this one.
        Some(_) => false,
        None => {
            let ok = gtk::init().is_ok();
            *asked = Some(ok);
            ok
        }
    }
}
