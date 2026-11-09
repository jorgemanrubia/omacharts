//! omacharts.

use std::rc::Rc;

use std::cell::RefCell;

use adw::prelude::*;
use gtk::gio;
use gtk::gio::prelude::ApplicationCommandLineExt;
use gtk::glib;
use omacharts::cli;
use omacharts::log_words;
use omacharts::store::Store;
use omacharts::ui::window::NotShown;
use omacharts::ui::Window;

const APP_ID: &str = "com.jorgemanrubia.Omacharts";

fn main() -> glib::ExitCode {
    let code = run();
    // On the way out, however it went: see `omacharts::update::settle`.
    omacharts::update::settle();
    code
}

fn run() -> glib::ExitCode {
    // Before anything else: learn from the launcher what is installed, which
    // is what the updater compares against. An app that is itself the
    // installed command becomes a newer download here, with an exec, and
    // nothing done above this line would survive it. See `omacharts::launch`.
    omacharts::update::at_launch();
    // Started by "Restart to update": the window it replaces is closing, and
    // this has to wait for it to let go of the application id, or the launch
    // would be handed to the window that is on its way out.
    omacharts::update::wait_for_restart(app_is_running);

    omacharts::feeds::identify();
    let launch = cli::caller_paths(std::env::args().collect::<Vec<String>>());

    // The launch options come off first, so that everything below reads a
    // command line of commands and symbols: `--provider` says what this
    // process charts from, and nothing after this point has to know it
    // exists. `launch` keeps them, because a second invocation's arguments
    // are handed to the window and the flag has an answer to earn there too.
    let args = match cli::peel_launch(&launch) {
        Ok(peeled) => {
            if let Some(feed) = peeled.provider {
                omacharts::feeds::use_for_this_launch(feed);
            }
            peeled.args
        }
        Err(fault) => {
            omacharts::update::not_a_window();
            eprintln!("omacharts: {}\n\n{}", fault.message, cli::usage());
            return glib::ExitCode::from(fault.code);
        }
    };
    // A command, `--version`, `--help` or a mistyped option: never a window,
    // so this launch says nothing about whether the version runs. A symbol
    // or nothing at all might be, and is decided below.
    if cli::is_command(&args) || args.get(1).is_some_and(|arg| arg.starts_with('-')) {
        omacharts::update::not_a_window();
    }

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
        let store = open_store();
        // What the flags switch, as a window would read them: a feed behind
        // one is there for the command only while it is on.
        omacharts::features::adopt(&store);
        return report(cli::run(&args, &store, None, &cli::Here));
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
        let given: Vec<String> = command_line
            .arguments()
            .into_iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();

        // Peeled again, because these arguments may have come from another
        // process: `omacharts --provider tos NVDA` typed at a terminal with a
        // window already open arrives here, and `tos` must not be read as a
        // second symbol.
        let (args, asked_for, in_list) = match cli::peel_launch(&given) {
            Ok(peeled) => (peeled.args, peeled.provider, peeled.watchlist),
            Err(fault) => {
                command_line.printerr_literal(&format!("omacharts: {}\n", fault.message));
                return glib::ExitCode::from(fault.code);
            }
        };

        let starting = window.borrow().is_none();
        if starting {
            // This process is the window: a downloaded version that ends
            // from here until it has been up a few seconds did not run.
            omacharts::update::window_starting();
            // Not copied first, a version that never ran here leaves the
            // settings alone this time, and tries again next launch.
            let uncopied = omacharts::update::before_first_open(&settings_file()).err();
            let opened = match uncopied.as_ref().map_or_else(Store::open, |_| Ok(Store::memory().expect("open database"))) {
                // Written by a newer download that is gone or given up on,
                // and nothing newer to start instead: put back the settings
                // from before it, and say so once.
                Err(omacharts::migrations::Error::FromTheFuture { .. })
                    if !omacharts::update::restart_as_newer_for_a_newer_database() =>
                {
                    if let Some(said) = omacharts::update::roll_back_for_a_newer_database(&settings_file()) {
                        eprintln!("omacharts: {said}");
                    }
                    Store::open()
                }
                Err(omacharts::migrations::Error::FromTheFuture { .. }) => return glib::ExitCode::SUCCESS,
                opened => opened,
            };
            let store = match opened {
                Ok(store) if let Some(why) = &uncopied => {
                    omacharts::update::settings_unopened(None);
                    eprintln!("omacharts: settings not copied before this version first opened them: {why}; carrying on without saving anything");
                    log_words::say(log_words::trouble(
                        "Settings",
                        "Settings could not be copied before this version first opened them · nothing is saved this time",
                        why,
                    ));
                    store
                }
                Ok(store) => {
                    omacharts::update::opened(&settings_file());
                    // A rollback this window did, or a command did with no
                    // window to say it in: said once, here.
                    if let Some(said) = omacharts::update::take_notice() {
                        log_words::say(log_words::note("Settings", said));
                    }
                    store
                }
                // Written by a newer Omacharts than this one. The file is
                // intact and it is theirs, so this is the one failure that
                // must not be worked around: carrying on with an empty
                // database would put a pristine watchlist on screen over the
                // top of years of their own, and look exactly like having
                // lost it. A newer download on the shelf most likely wrote
                // it, and was started in this one's place above.
                Err(error @ omacharts::migrations::Error::FromTheFuture { .. }) => {
                    eprintln!("omacharts: {error}; {}", omacharts::update::NEWER_DATABASE_HELP);
                    return glib::ExitCode::FAILURE;
                }
                // Anything else — unreadable, unwritable, corrupt — and an
                // in-memory database at least puts a usable window on screen
                // rather than nothing at all.
                Err(error) => {
                    // Not shown to run here, however long it is up; and
                    // counted against it when its own migration failed.
                    omacharts::update::settings_unopened(Some(&error));
                    eprintln!("omacharts: {error}; carrying on without saving anything");
                    log_words::say(log_words::trouble(
                        "Settings",
                        "Settings could not be opened · nothing is saved this time",
                        &error,
                    ));
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

            let backups = store.path().map(std::path::Path::to_path_buf);

            *window.borrow_mut() = Some(Window::build(app, Rc::new(store)));

            // Behind the first frame too, and on a thread of its own: a
            // backup is for the next time something goes wrong, not for
            // this launch.
            if let Some(settings) = backups {
                glib::idle_add_local_once(move || omacharts::backup::keep_taking(settings));
            }

            if let Some((path, limit)) = sweep {
                glib::idle_add_local_once(move || {
                    omacharts::cache::sweep_in_background(path, limit, |swept| {
                        if swept.dropped > 0 {
                            eprintln!(
                                "omacharts: cache over its limit, dropped {} series",
                                swept.dropped
                            );
                            log_words::say(log_words::note(
                                "Price cache",
                                format!("Over its limit · dropped {} series", swept.dropped),
                            ));
                        }
                    });
                });
            }
        }

        let Some(window) = window.borrow().clone() else {
            return glib::ExitCode::FAILURE;
        };

        // A window already on screen switches to the feed it was asked for,
        // the way the settings panel switches it: for this window only, and
        // without touching what is stored. Said back, so the terminal that
        // asked knows it happened.
        if !starting
            && let Some(feed) = asked_for
            && window.provider().id() != feed.id
        {
            if feed.setup.is_some_and(|setup| setup.trades())
                && let Ok(store) = Store::open()
            {
                omacharts::feeds::remember_logins(&store);
            }
            window.switch_feeds(
                omacharts_engine::providers::ordered(&omacharts::feeds::launch_order(feed))
                    .into_iter()
                    .map(std::sync::Arc::from)
                    .collect(),
            );
            command_line.printerr_literal(&format!(
                "omacharts: charting from {} for this window; `omacharts config set \
                 provider {}` makes it the default\n",
                feed.label, feed.id,
            ));
        }

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
        // Resolved here rather than when it was peeled, because this is the
        // first point with a database to resolve it against — the window's
        // own, since the bar sends one on every click. A list that does not
        // exist is refused the way a misspelt feed is, with the window still
        // put up.
        let mut code = glib::ExitCode::SUCCESS;
        match in_list.as_deref().map(|list| cli::watchlist_named(window.store(), list)).transpose() {
            Ok(watchlist) => match symbols.first() {
                Some(symbol) => {
                    let shown = window.show_named(
                        &symbol.to_uppercase(),
                        symbols.get(1).map(String::as_str),
                        watchlist.as_ref().map(|(id, _)| *id),
                    );
                    let named = symbols.iter().map(|s| s.to_uppercase()).collect::<Vec<_>>().join(" ");
                    match shown {
                        // By its name, which is what the rail shows: the bar
                        // names the list as `id:1`.
                        Err(NotShown::Unlisted) => {
                            if let Some((_, name)) = &watchlist {
                                command_line.printerr_literal(&format!(
                                    "omacharts: {named} is not in watchlist {name:?}\n"
                                ));
                                code = glib::ExitCode::from(cli::EXIT_NOT_FOUND);
                            }
                        }
                        // Selected in the list and taken by nothing, which is
                        // not success: the chart in front kept its symbol.
                        Err(NotShown::Refused) => {
                            command_line.printerr_literal(&format!(
                                "omacharts: {named} not shown; the focused chart is pinned \
                                 and no chart follows the watchlist\n"
                            ));
                            code = glib::ExitCode::from(cli::EXIT_REFUSED);
                        }
                        Ok(()) => {}
                    }
                }
                None => {
                    if let Some((id, _)) = watchlist {
                        window.show_watchlist(id);
                    }
                }
            },
            Err(fault) => {
                command_line.printerr_literal(&format!("omacharts: {}\n", fault.message));
                code = glib::ExitCode::from(fault.code);
            }
        }

        window.window.present();

        // After the window is on screen, never before it: keeping an installed
        // bar widget current is housekeeping, and housekeeping does not get to
        // sit in front of the first frame.
        glib::idle_add_local_once(|| {
            if let Some(home) = glib::home_dir().to_str().map(std::path::PathBuf::from)
                && omacharts::bar_plugin::launch_owns_widget(&home)
                && let Err(error) = omacharts::bar_plugin::refresh(&home)
            {
                eprintln!("omacharts: bar widget not updated: {error}");
                log_words::say(log_words::trouble("Bar widget", "Bar widget not updated", &error));
            }
        });

        code
    });

    // With no display to open, GTK ends the process while registering,
    // before anything of ours could say so: from ssh or a script that is
    // not the version failing, so it must not count against it.
    if !display_reachable() {
        omacharts::update::not_a_window();
    }
    // Handed to a window another process keeps, this launch is not one, and
    // being killed while it waits for the answer says nothing about it.
    // Registering is the first thing running does anyway.
    if app.register(gio::Cancellable::NONE).is_ok() && app.is_remote() {
        omacharts::update::not_a_window();
    }

    // The arguments as typed, launch options included: these travel to
    // whichever process holds the window, and the flag is answered there.
    app.run_with_args(&launch)
}

/// Could GDK open a display? What its backends try, asked without starting
/// GTK: an X display named in `DISPLAY`, or a Wayland socket —
/// `WAYLAND_DISPLAY`, or `wayland-0` when unset, in `XDG_RUNTIME_DIR` unless
/// it is a path. An environment variable and a `stat` or two. Wrong the
/// other way, it only means a launch that is not counted.
fn display_reachable() -> bool {
    reachable_with(|name| std::env::var_os(name), std::path::Path::new("/tmp/.X11-unix"))
}

/// [`display_reachable`], given the environment and where X keeps its
/// sockets.
fn reachable_with(var: impl Fn(&str) -> Option<std::ffi::OsString>, x11: &std::path::Path) -> bool {
    let set = |name: &str| var(name).filter(|value| !value.is_empty());
    if let Some(display) = set("DISPLAY") {
        // A local display, `:0` or `:0.0`, has its socket in /tmp/.X11-unix;
        // one on another host, as ssh forwards it, is taken at its word. A
        // local one whose socket is gone — a `DISPLAY` left over with
        // Xwayland off — leaves Wayland to be asked, as GDK asks it.
        let display = display.to_string_lossy();
        let Some(number) = display.strip_prefix(':').or_else(|| display.strip_prefix("unix:")) else { return true };
        let number = number.split('.').next().unwrap_or_default();
        if x11.join(format!("X{number}")).exists() {
            return true;
        }
    }
    let socket = set("WAYLAND_DISPLAY").unwrap_or_else(|| "wayland-0".into());
    let socket = std::path::PathBuf::from(socket);
    if socket.is_absolute() {
        return socket.exists();
    }
    set("XDG_RUNTIME_DIR").is_some_and(|dir| std::path::Path::new(&dir).join(socket).exists())
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
    if let Err(why) = omacharts::update::before_first_open(&settings_file()) {
        // Never migrated without the copy to go back to; a command has no
        // empty window to carry on in, so it stops, and nothing is changed.
        eprintln!("omacharts: settings not copied before this version first opened them, so left alone: {why}");
        omacharts::update::settle();
        std::process::exit(cli::EXIT_ERROR as i32);
    }
    let opened = match Store::open() {
        Err(omacharts::migrations::Error::FromTheFuture { .. }) => {
            // A newer download most likely wrote it: become that, or if it
            // is gone or given up on, put back the settings from before it.
            omacharts::update::become_newer_for_a_newer_database();
            if let Some(said) = omacharts::update::roll_back_for_a_newer_database(&settings_file()) {
                eprintln!("omacharts: {said}");
            }
            Store::open()
        }
        opened => opened,
    };
    match opened {
        Ok(store) => {
            omacharts::update::opened(&settings_file());
            store
        }
        Err(error) => {
            if matches!(error, omacharts::migrations::Error::FromTheFuture { .. }) {
                eprintln!("omacharts: {error}; {}", omacharts::update::NEWER_DATABASE_HELP);
            } else {
                eprintln!("omacharts: {error}");
            }
            omacharts::update::settle();
            std::process::exit(cli::EXIT_ERROR as i32);
        }
    }
}

/// The settings file [`Store::open`] opens.
fn settings_file() -> std::path::PathBuf {
    omacharts::store::data_dir().join("omacharts.db")
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

#[cfg(test)]
mod tests {
    use super::reachable_with;
    use std::collections::HashMap;
    use std::ffi::OsString;
    use std::fs;
    use std::path::PathBuf;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("omacharts-display-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("x11")).unwrap();
        fs::create_dir_all(dir.join("run")).unwrap();
        dir
    }

    fn reachable(dir: &std::path::Path, vars: &[(&str, &str)]) -> bool {
        let vars: HashMap<String, OsString> = vars
            .iter()
            .map(|(name, value)| (name.to_string(), OsString::from(value.replace("{run}", &dir.join("run").to_string_lossy()))))
            .collect();
        reachable_with(|name| vars.get(name).cloned(), &dir.join("x11"))
    }

    #[test]
    fn a_display_is_reachable_by_its_socket_and_a_stale_x_one_leaves_wayland_to_be_asked() {
        let dir = scratch("reach");
        fs::write(dir.join("run/wayland-1"), b"").unwrap();
        // A stale `DISPLAY` beside a live Wayland socket: Wayland is reached.
        assert!(reachable(&dir, &[("DISPLAY", ":0"), ("WAYLAND_DISPLAY", "wayland-1"), ("XDG_RUNTIME_DIR", "{run}")]));
        // Stale, and nothing else: none.
        assert!(!reachable(&dir, &[("DISPLAY", ":0"), ("XDG_RUNTIME_DIR", "{run}")]));
        assert!(!reachable(&dir, &[("DISPLAY", ":0.0")]));
        // A live X socket, a remote display, and a Wayland one alone.
        fs::write(dir.join("x11/X0"), b"").unwrap();
        assert!(reachable(&dir, &[("DISPLAY", ":0.0")]));
        assert!(reachable(&dir, &[("DISPLAY", "localhost:10.0")]));
        assert!(reachable(&dir, &[("WAYLAND_DISPLAY", "wayland-1"), ("XDG_RUNTIME_DIR", "{run}")]));
        assert!(!reachable(&dir, &[("XDG_RUNTIME_DIR", "{run}")]), "no wayland-0 there");
        let _ = fs::remove_dir_all(&dir);
    }
}
