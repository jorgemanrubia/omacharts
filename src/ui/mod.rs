//! The widgets.

/// The chart itself is a crate of its own; it is here under the name the
/// rest of the app has always reached it by.
pub use omacharts_chart as chart;
pub use omacharts_chart::{colors, text};
/// The pieces around the chart are a crate of their own too, so that a
/// program embedding the chart can have them; here under their old names.
pub use omacharts_widgets::{dialogs, drawing_bar, menus, pick_frame, range_card, search, shortcuts, strip};
pub mod book_templates;
pub mod chart_settings;
pub mod colouring_rows;
pub mod controls;
pub mod drop_line;
pub mod notice;
pub mod order_entry;
pub mod order_templates;
pub mod drawing_settings;
pub mod feed_settings;
pub mod pane;
pub mod palette;
pub mod part_list;
pub mod preferences;
pub mod profile_rows;
pub mod property_rows;
pub mod sign_in_dialogs;
pub mod size_templates;
pub mod trading_settings;
pub mod screenshot;
pub mod trading_panel;
pub mod watchlist;
pub mod window;

pub use chart::ChartView;
pub use window::Window;

/// Run a test that builds widgets on the one thread GTK is ever
/// initialised on, and say whether it ran.
///
/// libtest gives every test its own thread, even at `--test-threads=1`, and
/// GTK belongs to the first thread that initialises it: a widget touched from
/// any other panics with "attempted to initialize GTK from two different
/// threads". So asking on the test's own thread meant the first GTK test
/// claimed GTK and every other one quietly returned — a whole suite of
/// window tests that passed by never running, one of them broken without
/// anybody knowing. Instead every one is handed to a single thread that owns
/// GTK for the life of the binary, one at a time, and its panic is carried
/// back to the test that sent it.
///
/// With no display, `gtk::init` fails, nothing runs and the answer is false,
/// which is the usual case on CI. GTK is asked exactly once: a second attempt
/// after a failed one segfaults inside GTK and ends the whole binary. That
/// is said once, straight to stderr past the harness's capture, so a run
/// whose window tests all passed by not running says so.
#[cfg(test)]
pub fn on_gtk(test: impl FnOnce() + Send + 'static) -> bool {
    use std::sync::{OnceLock, mpsc};
    type Job = (Box<dyn FnOnce() + Send>, mpsc::Sender<std::thread::Result<()>>);
    static GTK: OnceLock<Option<mpsc::Sender<Job>>> = OnceLock::new();
    let gtk = GTK.get_or_init(|| {
        let (jobs, queue) = mpsc::channel::<Job>();
        let (ready, answer) = mpsc::channel();
        std::thread::Builder::new()
            .name("gtk".into())
            .spawn(move || {
                let ok = gtk::init().is_ok();
                let _ = ready.send(ok);
                if !ok {
                    use std::io::Write;
                    let _ = writeln!(std::io::stderr(), "no display: the GTK tests are skipped, and pass without running");
                    return;
                }
                for (test, done) in queue {
                    // Each test starts with the flags as they ship, whatever
                    // the one before switched on.
                    crate::features::reset();
                    // And with no logins' feeds told, whatever the one before
                    // had: every test has a store of its own.
                    omacharts_engine::providers::forget_login_feeds();
                    let _ = done.send(std::panic::catch_unwind(std::panic::AssertUnwindSafe(test)));
                }
            })
            .expect("a thread for GTK");
        answer.recv().unwrap_or(false).then_some(jobs)
    });
    let Some(gtk) = gtk else { return false };
    let (done, finished) = mpsc::channel();
    gtk.send((Box::new(test), done)).expect("the GTK thread");
    // The GTK thread prints its panics into the output of whichever test
    // started it, so the message is raised again here, in this test's own.
    if let Err(panic) = finished.recv().expect("the GTK thread") {
        let message = panic
            .downcast_ref::<String>()
            .map(String::as_str)
            .or_else(|| panic.downcast_ref::<&str>().copied())
            .unwrap_or("a panic with no message");
        panic!("on the GTK thread: {message}");
    }
    true
}

/// A symbolic icon of the app's own, which no icon theme has: put where the
/// display's icon theme finds it, once, and named `name`.
pub fn symbolic_icon(name: &'static str, svg: &str) -> &'static str {
    let Some(display) = gtk::gdk::Display::default() else { return name };
    let theme = gtk::IconTheme::for_display(&display);
    if theme.has_icon(name) {
        return name;
    }
    let folder = crate::store::data_dir().join("icons");
    let file = folder.join(format!("{name}.svg"));
    if std::fs::read_to_string(&file).ok().as_deref() != Some(svg) {
        let _ = std::fs::create_dir_all(&folder);
        let _ = std::fs::write(&file, svg);
    }
    theme.add_search_path(&folder);
    name
}

/// The bracket an order template stands for: an entry with a target above
/// and a stop below.
pub fn bracket_icon() -> &'static str {
    symbolic_icon("omacharts-bracket-symbolic", include_str!("../../assets/symbolic/omacharts-bracket-symbolic.svg"))
}
