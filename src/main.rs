//! omacharts.

use std::rc::Rc;

use std::cell::RefCell;

use adw::prelude::*;
use gtk::gio;
use gtk::gio::prelude::ApplicationCommandLineExt;
use gtk::glib;
use omacharts::cli;
use omacharts::store::Store;
use omacharts::ui::Window;

const APP_ID: &str = "com.jorgemanrubia.Omacharts";

fn main() -> glib::ExitCode {
    let args: Vec<String> = std::env::args().collect();

    // A few commands answer for the process they were typed in, and the bus is
    // never asked about them: handing `skill` to the window would report on
    // the window's agent variables and resolve a relative `--to` against the
    // window's working directory, which is nobody's intention. The spec says
    // which, and they are the commands that read no database — so none is
    // opened, and `skill status` still answers on a machine whose database
    // cannot be written.
    if cli::runs_in_the_caller(&args) {
        let nothing = Store::memory().expect("an empty database");
        return report(cli::run(&args, &nothing, None, &cli::Here));
    }

    // Everything else goes where its result can be shown.
    //
    // With a window open, the command is handed to it, so a watchlist created
    // in a terminal appears in the rail at once rather than after a restart.
    // GTK's single-instance hand-off carries the output and the exit status
    // back to this process, so nothing is lost by running it somewhere else.
    //
    // With nothing open, the same command runs right here against the
    // database — no GTK, no display, no window. That is what makes this
    // usable over ssh and out of a cron line, and it is why the check below
    // asks the bus rather than starting an application to find out.
    if let Some(refused) = cli::stdin_is_not_piped(&args) {
        return report(refused);
    }
    if cli::is_command(&args) && !app_is_running() {
        return report(cli::run(&args, &open_store(), None, &cli::Here));
    }
    if let Some(code) = peel_off(&args).filter(|_| !cli::is_command(&args)) {
        return code;
    }

    // GTK4 picks the Vulkan renderer by default, and bringing up a Vulkan
    // context costs most of half a second before anything of ours runs —
    // measured here, window on screen: vulkan 575ms, gl 191ms, cairo 148ms,
    // against 8ms of our own work. The chart is drawn with cairo into a
    // DrawingArea either way, so the renderer only composites the result;
    // paying a third of a second for that is a bad trade on an app whose
    // whole point is feeling instant.
    //
    // "gl", not "ngl": the renderer was renamed and the old name now draws a
    // warning on every launch. It is still honoured — the timings above are
    // identical either way — but a warning nobody can act on is noise.
    //
    // Software compositing (cairo) is faster still to the first frame and
    // pays for it while panning, which is the one thing this app does
    // constantly. Anything already set is left alone.
    if std::env::var_os("GSK_RENDERER").is_none() {
        // SAFETY: single-threaded, before GTK or any other thread starts.
        unsafe { std::env::set_var("GSK_RENDERER", "gl") };
    }

    // The command line is handled rather than ignored, because a second
    // launch is how anything outside the app asks for a symbol: the bar widget
    // runs `omacharts NVDA`, and GTK hands that to the instance already
    // running instead of starting another.
    let app = adw::Application::builder()
        .application_id(APP_ID)
        .flags(gio::ApplicationFlags::HANDLES_COMMAND_LINE)
        .build();

    let window: RefCell<Option<Rc<Window>>> = RefCell::new(None);
    app.connect_command_line(move |app, command_line| {
        let args: Vec<String> = command_line
            .arguments()
            .into_iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();

        if window.borrow().is_none() {
            let store = match Store::open() {
                Ok(store) => store,
                // Written by a newer Omacharts than this one. The file is
                // intact and it is theirs, so this is the one failure that
                // must not be worked around: carrying on with an empty
                // database would put a pristine watchlist on screen over the
                // top of years of their own, and look exactly like having
                // lost it.
                Err(error @ omacharts::migrations::Error::FromTheFuture { .. }) => {
                    eprintln!("omacharts: {error}");
                    return glib::ExitCode::FAILURE;
                }
                // Anything else — unreadable, unwritable, corrupt — and an
                // in-memory database at least puts a usable window on screen
                // rather than nothing at all.
                Err(error) => {
                    eprintln!("omacharts: {error}; carrying on without saving anything");
                    Store::memory().expect("open database")
                }
            };
            // Trimming the cache is housekeeping, and housekeeping goes
            // behind the first frame with the rest of it: a cache that has
            // been over its limit for six hours can be over it a second
            // longer. Read before the store moves into the window.
            let sweep = store
                .path()
                .filter(|_| store.cache_sweep_due())
                .map(|path| (path.to_path_buf(), store.cache_limit()));

            *window.borrow_mut() = Some(Window::build(app, Rc::new(store)));

            if let Some((path, limit)) = sweep {
                glib::idle_add_local_once(move || {
                    omacharts::cache::sweep_in_background(path, limit, |swept| {
                        if swept.dropped > 0 {
                            eprintln!(
                                "omacharts: cache over its limit, dropped {} series",
                                swept.dropped
                            );
                        }
                    });
                });
            }
        }

        let Some(window) = window.borrow().clone() else {
            return glib::ExitCode::FAILURE;
        };

        // The window, to a command that wants what is on screen rather than
        // what was last written down. `None` until `Window` carries the three
        // methods `cli::Live` asks for — until then a command still runs and
        // still answers, but the rail is redrawn on the next thing that
        // touches it rather than at once.
        let live: Option<Box<dyn cli::Live>> = cli::live(&window);

        // A command rather than a symbol. Run here, in the process holding the
        // window, so the output goes back down the pipe the arguments came up
        // and the result is on screen before the terminal gets its prompt.
        //
        // On its own connection: SQLite in WAL mode takes a second writer
        // beside the window's own, and a command is rare enough that opening
        // one costs nothing anybody can measure.
        if cli::is_command(&args) {
            let Ok(store) = Store::open() else {
                return glib::ExitCode::FAILURE;
            };
            let outcome = cli::run(&args, &store, live.as_deref(), &Typed(command_line));
            if !outcome.out.is_empty() {
                command_line.print_literal(&outcome.out);
            }
            if !outcome.err.is_empty() {
                command_line.printerr_literal(&outcome.err);
            }
            return glib::ExitCode::from(outcome.code);
        }

        let symbols: Vec<String> =
            args.iter().skip(1).filter(|arg| !arg.starts_with('-')).cloned().collect();
        if let Some(symbol) = symbols.first() {
            window.show_named(&symbol.to_uppercase(), symbols.get(1).map(String::as_str));
        }

        window.window.present();

        // After the window is on screen, never before it: keeping an installed
        // bar widget current is housekeeping, and housekeeping does not get to
        // sit in front of the first frame.
        glib::idle_add_local_once(|| {
            if let Some(home) = glib::home_dir().to_str().map(std::path::PathBuf::from)
                && let Err(error) = omacharts::bar_plugin::refresh(&home)
            {
                eprintln!("omacharts: bar widget not updated: {error}");
            }
        });

        glib::ExitCode::SUCCESS
    });

    app.run_with_args(&args)
}

/// Is a window already open?
///
/// Asked of the session bus rather than by starting an application and
/// looking, because registering one would make *this* process the instance
/// everything else hands its commands to — for the few milliseconds before it
/// exits, which is long enough to swallow somebody's launch.
///
/// No bus at all is an answer too: over ssh there is no session to have a
/// window in, so the command belongs here.
fn app_is_running() -> bool {
    let Ok(bus) = gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE) else {
        return false;
    };
    bus.call_sync(
        Some("org.freedesktop.DBus"),
        "/org/freedesktop/DBus",
        "org.freedesktop.DBus",
        "NameHasOwner",
        Some(&(APP_ID,).into()),
        Some(glib::VariantTy::new("(b)").expect("a known type")),
        gio::DBusCallFlags::NONE,
        200,
        gio::Cancellable::NONE,
    )
    .ok()
    .and_then(|reply| reply.child_value(0).get::<bool>())
    .unwrap_or(false)
}

/// The terminal a command came from, when the command runs in the window.
///
/// GTK hands the window the caller's working directory and stdin along with
/// the arguments, so a path resolves where it was typed and `-` reads the
/// pipe that was there.
struct Typed<'a>(&'a gio::ApplicationCommandLine);

impl cli::Caller for Typed<'_> {
    fn read(&self, path: &str) -> Result<String, cli::Fault> {
        use gio::prelude::*;
        use std::io::Read;
        if path == "-" {
            let stdin = self.0.stdin().ok_or_else(|| cli::unreadable(path, "none was passed"))?;
            let mut text = String::new();
            stdin.into_read().read_to_string(&mut text).map_err(|e| cli::unreadable(path, e))?;
            return Ok(text);
        }
        let (bytes, _) = self
            .0
            .create_file_for_arg(path)
            .load_contents(gio::Cancellable::NONE)
            .map_err(|error| cli::unreadable(path, error))?;
        String::from_utf8(bytes.to_vec()).map_err(|error| cli::unreadable(path, error))
    }
}

fn open_store() -> Store {
    match Store::open() {
        Ok(store) => store,
        Err(error) => {
            eprintln!("omacharts: {error}");
            std::process::exit(cli::EXIT_ERROR as i32);
        }
    }
}

fn report(outcome: cli::Outcome) -> glib::ExitCode {
    print!("{}", outcome.out);
    eprint!("{}", outcome.err);
    glib::ExitCode::from(outcome.code)
}

/// The arguments answered without a database, a bus or a window.
fn peel_off(args: &[String]) -> Option<glib::ExitCode> {
    match args.get(1).map(String::as_str) {
        Some("--help" | "-h") => {
            print!("{}", cli::usage());
            Some(glib::ExitCode::SUCCESS)
        }
        Some("--version" | "-V") => {
            println!("omacharts {}", env!("CARGO_PKG_VERSION"));
            Some(glib::ExitCode::SUCCESS)
        }
        // An unknown option is a mistake worth reporting. Anything else is a
        // symbol, and goes to the app.
        Some(other) if other.starts_with('-') => {
            eprintln!("omacharts: unknown option {other:?}\n\n{}", cli::usage());
            Some(glib::ExitCode::FAILURE)
        }
        _ => None,
    }
}
