//! The real window on a feed that streams invented prices.
//!
//!     dbus-run-session -- env XDG_DATA_HOME=$(mktemp -d) \
//!         cargo run --release --example live_window -- [tick-ms] [charts]
//!
//! Opens Omacharts on the synthetic provider: every symbol charts, every
//! chart is a subscription, and the forming bar moves every `tick-ms`
//! milliseconds (default 250; `0` streams the snapshot and then nothing,
//! which is what a market that is shut looks like). `charts` splits the
//! window that many ways (default 1), each chart on the same symbol — one
//! subscription, however many. It is how the streaming path is seen working,
//! and measured at rest, without an account at a brokerage — and it is the
//! same window, the same registry and the same drain a real feed goes
//! through.
//!
//! A throwaway data home and a private bus, always: the store follows
//! `XDG_DATA_HOME`, and a command typed at a terminal is handed to whatever
//! owns the application id on the session bus.

use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use adw::prelude::*;
use gtk::gio;
use gtk::glib;
use omacharts::store::Store;
use omacharts::ui::Window;
use omacharts_engine::providers::Synthetic;
use omacharts_engine::Provider;

fn main() -> glib::ExitCode {
    let tick: u64 = std::env::args().nth(1).and_then(|arg| arg.parse().ok()).unwrap_or(250);
    let tick = (tick > 0).then(|| Duration::from_millis(tick));
    let charts: usize = std::env::args().nth(2).and_then(|arg| arg.parse().ok()).unwrap_or(1);

    if std::env::var_os("GSK_RENDERER").is_none() {
        // SAFETY: single-threaded, before GTK or any other thread starts.
        unsafe { std::env::set_var("GSK_RENDERER", "gl") };
    }
    let app = adw::Application::builder()
        .application_id("com.jorgemanrubia.Omacharts.LiveWindow")
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();
    app.connect_activate(move |app| {
        let store = Store::open().expect("a store in the throwaway data home");
        let provider: Arc<dyn Provider> = Arc::new(Synthetic::new(tick));
        let window = Window::build_with(app, Rc::new(store), provider);
        // Splitting copies the chart being split, so every chart shows the
        // same symbol and they all watch the one subscription.
        for n in 1..charts {
            window.split_focused(n % 2 == 1);
        }
        // Kept for the life of the application, like the real main does.
        std::mem::forget(window);
    });
    app.run_with_args::<&str>(&[])
}
