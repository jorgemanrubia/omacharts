//! The listings, as the window sees them: the long tail parsed behind the
//! first frame, and a fresh copy fetched once a day and swapped in, so a
//! company that listed today is searchable without a restart.
//!
//! Fetching, checking and writing are [`crate::listings`]'s.

use std::rc::Rc;
use std::time::SystemTime;

use gtk::glib;

use super::Window;
use crate::listings;

/// Seconds after the window opens before the first check: well behind the
/// first frame, the charts' first fetches and signing in.
const SETTLE: u32 = 60;

impl Window {
    /// Parse the listings off the main loop and swap them in; then chart
    /// what the curated half could not name, and redraw the rail, which
    /// left out its symbols outside it.
    pub fn load_listings(self: &Rc<Self>) {
        let weak = Rc::downgrade(self);
        crate::inventory::load_in_background(self.index.clone(), move |_| {
            let Some(this) = weak.upgrade() else { return };
            this.resolve_pending();
            let rail = this.watchlist.borrow().as_ref().cloned();
            if let Some(rail) = rail {
                rail.rebuild();
            }
        });
    }

    /// A symbol a feed confirmed that no listing has, searchable from now on.
    pub fn learn_symbol(&self, instrument: &omacharts_engine::Instrument) {
        self.index.add_found(instrument.clone());
    }

    /// Check for newer listings once things have settled, and again whenever
    /// a day has passed. A timer, which the app otherwise avoids for noticing
    /// outside changes: nothing can tell a running window that the listings
    /// were published, so it has to ask. Every few hours it looks at a file's
    /// time, and once a day that is one small request.
    pub(super) fn keep_listings_current(self: &Rc<Self>) {
        // Never from a test: no window in the suite reaches the network.
        if cfg!(test) {
            return;
        }
        let weak = Rc::downgrade(self);
        glib::timeout_add_seconds_local_once(SETTLE, move || {
            if let Some(this) = weak.upgrade() {
                this.refresh_listings_if_due();
            }
        });
        let weak = Rc::downgrade(self);
        glib::timeout_add_seconds_local(listings::LOOK_EVERY.as_secs() as u32, move || {
            let Some(this) = weak.upgrade() else { return glib::ControlFlow::Break };
            this.refresh_listings_if_due();
            glib::ControlFlow::Continue
        });
    }

    /// One check, on a thread, when Preferences allows it and a day has
    /// passed since the last. Quiet whatever happens: a failure is a line on
    /// stderr, and search goes on with the listings it had.
    fn refresh_listings_if_due(self: &Rc<Self>) {
        if !self.store.setting_bool(listings::SETTING, true) {
            return;
        }
        // Everything from here, the file's time included, on a thread of its
        // own: the main loop never waits on the disk or the network. Nobody
        // joins it, so a slow line never holds up quitting either.
        let (send, receive) = async_channel::bounded(1);
        std::thread::Builder::new()
            .name("omacharts listings".into())
            .spawn(move || {
                let dir = listings::dir();
                let answer = match listings::due(&dir, SystemTime::now()) {
                    true => Some(listings::refresh(&listings::source(), &dir, crate::inventory::compiled())),
                    false => None,
                };
                let _ = send.send_blocking(answer);
            })
            .ok();
        let weak = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            match receive.recv().await.ok().flatten() {
                Some(Ok(fetched)) if fetched.updated => {
                    if let Some(this) = weak.upgrade() {
                        this.load_listings();
                    }
                }
                Some(Err(why)) => eprintln!("omacharts: {why}"),
                _ => {}
            }
        });
    }
}
