//! Updates, as the window sees them: a check once the first frame is on
//! screen and every few hours after, on a thread of its own, and the corner
//! notice that offers the restart once a newer version is ready.
//!
//! Everything about downloading and handing over is [`crate::update`]'s. A
//! window that is not the installed binary's — a build in `target/`, a test —
//! has no [`crate::update::origin`], and none of this runs.

use std::cell::Cell;
use std::rc::Rc;
use std::time::SystemTime;

use adw::prelude::*;
use gtk::glib;

use super::Window;
use crate::ui::notice::UpdateNotice;
use crate::update::{self, Checked, Shelf, Version};

/// Seconds a window is up before its version counts as one that runs.
const HEALTHY_AFTER: u32 = 3;

impl Window {
    /// Start keeping up to date, behind the first frame: an idle at low
    /// priority, registered when that frame has been painted, as signing in
    /// is. Nothing before it waits on any of this.
    pub(super) fn update_after_first_frame(self: &Rc<Self>) {
        if update::origin().is_none() {
            return;
        }
        let weak = Rc::downgrade(self);
        let mapped = Rc::new(Cell::new(false));
        self.window.connect_map(move |window| {
            if mapped.replace(true) {
                return;
            }
            let Some(clock) = window.frame_clock() else { return };
            let painted: Rc<std::cell::RefCell<Option<glib::SignalHandlerId>>> = Rc::default();
            let (weak, once) = (weak.clone(), painted.clone());
            let handler = clock.connect_after_paint(move |clock| {
                if let Some(handler) = once.borrow_mut().take() {
                    clock.disconnect(handler);
                }
                // A downloaded version that has drawn its window and kept it
                // up for a few seconds runs: it is never given up on now.
                glib::timeout_add_seconds_local_once(HEALTHY_AFTER, update::healthy);
                let weak = weak.clone();
                glib::idle_add_local_full(glib::Priority::LOW, move || {
                    if let Some(this) = weak.upgrade() {
                        this.keep_up_to_date();
                    }
                    glib::ControlFlow::Break
                });
            });
            *painted.borrow_mut() = Some(handler);
        });
    }

    /// Check now, and again every [`update::CADENCE`]. A timer, which the
    /// app otherwise avoids for noticing outside changes: nothing can tell a
    /// running window that a release was published, so it has to ask. Four
    /// times a day, and each time it is one request when nothing is new.
    fn keep_up_to_date(self: &Rc<Self>) {
        self.check_for_update(true);
        let weak = Rc::downgrade(self);
        glib::timeout_add_seconds_local(update::CADENCE.as_secs() as u32, move || {
            let Some(this) = weak.upgrade() else { return glib::ControlFlow::Break };
            this.check_for_update(false);
            glib::ControlFlow::Continue
        });
    }

    /// One check, off the main loop. With the switch off nothing is asked
    /// for, and what is already downloaded stays: it is newer than what
    /// pacman installed, and the settings it may already have migrated would
    /// not open in the older one.
    fn check_for_update(self: &Rc<Self>, at_launch: bool) {
        if !self.store.setting_bool(update::SETTING, true) || update::updates_off() || update::as_root() {
            return;
        }
        let Some(origin) = update::origin() else { return };
        let (send, receive) = async_channel::bounded::<Option<Version>>(1);
        std::thread::spawn(move || {
            let shelf = Shelf::new(update::shelf_dir());
            let running = Version::running();
            // At launch the installed version is the one the launch just
            // started from. After that, and after any download, pacman may
            // have replaced it, so the binary itself is asked.
            let asked = Cell::new(0);
            let installed = || {
                let first = asked.replace(asked.get() + 1) == 0;
                if at_launch && first { origin.installed_version } else { update::installed_version(origin) }
            };
            let ready = if shelf.check_due(SystemTime::now()) {
                match update::check(&update::GitHub::new(), &shelf, running, &installed, true) {
                    Ok(checked @ Checked::Ready { .. }) => checked.restart_brings(running, installed()),
                    Ok(_) => None,
                    Err(failure) => {
                        eprintln!("omacharts: update: {failure}");
                        update::on_the_shelf(&shelf, running, origin.installed_version)
                    }
                }
            } else {
                update::on_the_shelf(&shelf, running, installed())
            };
            let _ = send.send_blocking(ready);
        });
        let weak = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            if let Ok(Some(version)) = receive.recv().await
                && let Some(this) = weak.upgrade()
            {
                this.offer_restart(version);
            }
        });
    }

    /// The corner notice: `version` is ready, and restarting brings it in.
    pub(super) fn offer_restart(self: &Rc<Self>, version: Version) {
        if self.messages.update_dismissed.get() == Some(version) {
            return;
        }
        let built = self.messages.update.borrow().clone();
        let notice = match built {
            Some(notice) => notice,
            None => {
                let Some(overlay) = self.trade.overlay.borrow().clone() else { return };
                let restart = Rc::downgrade(self);
                let dismiss = Rc::downgrade(self);
                let notice = UpdateNotice::new(
                    move || {
                        if let Some(this) = restart.upgrade() {
                            this.restart_to_update();
                        }
                    },
                    move || {
                        if let Some(this) = dismiss.upgrade() {
                            this.dismiss_update();
                        }
                    },
                );
                notice.revealer.set_margin_start(12);
                overlay.add_overlay(&notice.revealer);
                *self.messages.update.borrow_mut() = Some(notice.clone());
                notice
            }
        };
        self.messages.update_shown.set(Some(version));
        self.place_update_notice();
        notice.say(&version.to_string());
    }

    /// Above the chartbook strip and the trading panel, like the error
    /// notice, and above that notice too when both are showing.
    pub(super) fn place_update_notice(&self) {
        let Some(notice) = self.messages.update.borrow().clone() else { return };
        let strip = if self.book_strip.is_visible() { self.book_strip.height() } else { 0 };
        let error = self.notice_height().map_or(0, |height| height + 8);
        notice.revealer.set_margin_bottom(12 + strip + self.trading_clearance() + error);
    }

    fn dismiss_update(&self) {
        self.messages.update_dismissed.set(self.messages.update_shown.take());
        let notice = self.messages.update.borrow_mut().take();
        if let (Some(notice), Some(overlay)) = (notice, self.trade.overlay.borrow().as_ref()) {
            overlay.remove_overlay(&notice.revealer);
        }
    }

    /// Start the installed binary again, to come up once this one is gone,
    /// and close every window the way closing it by hand would: what a quit
    /// saves is saved, and the new process restores it as any launch does.
    fn restart_to_update(&self) {
        if let Err(error) = update::spawn_restart() {
            self.say(crate::log_words::trouble("Updates", "Could not restart", &error));
            return;
        }
        let Some(app) = self.window.application() else { return };
        let main: gtk::Window = self.window.clone().upcast();
        for window in app.windows().into_iter().filter(|window| *window != main) {
            window.close();
        }
        self.window.close();
    }

    #[cfg(test)]
    pub(super) fn update_notice(&self) -> Option<Rc<UpdateNotice>> {
        self.messages.update.borrow().clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_notice_says_which_version_is_ready_and_offers_the_restart() {
        crate::ui::on_gtk(|| {
            let store = Rc::new(crate::store::Store::memory().expect("a store"));
            let window = crate::ui::window::trade::tests::window(&store);
            window.offer_restart(Version(0, 2, 15));
            let notice = window.update_notice().expect("a notice");
            assert_eq!(notice.text(), "Omacharts 0.2.15 is ready");
            assert_eq!(notice.restart.label().as_deref(), Some("Restart to update"));

            window.dismiss_update();
            assert!(window.update_notice().is_none());
            window.offer_restart(Version(0, 2, 15));
            assert!(window.update_notice().is_none(), "dismissed stays dismissed");
            window.offer_restart(Version(0, 2, 16));
            assert_eq!(window.update_notice().unwrap().text(), "Omacharts 0.2.16 is ready");
        });
    }
}
