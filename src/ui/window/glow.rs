//! The glow along the bottom of the window while the agent works on a
//! dictation.
//!
//! Omarchy shows a blue glow at the bottom of the screen while it listens;
//! this is the same kind of thing for the part after, in the window and in
//! the theme's amber, so that listening and thinking read as two things. It
//! fades in when a turn starts, breathes slowly while the agent works and
//! while any dictation queued behind it does, and fades out when the last is
//! done. A turn that failed ends in the theme's red instead.
//!
//! It is a gradient laid over the bottom of the window that takes no events
//! and fades to nothing well before any chart's content, so it never stands
//! between the hand and the window. Its clock is a timer, like the breath of
//! a position's line, waking at most twenty times a second and only while a
//! turn is under way; idle, it is hidden and there is no timer at all. With
//! the desktop's animations off it shows still, and comes and goes at once.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use adw::prelude::*;
use gtk::glib;

/// The class the glow wears; the stylesheet paints it.
pub const GLOW: &str = "agent-glow";
/// The class it wears for a turn that failed.
pub const FAILED: &str = "failed";

/// How tall the glow is, in pixels. The stylesheet fades it to nothing
/// over this height.
const HEIGHT: i32 = 44;
/// One breath, in seconds: slow, so it reads as working rather than as
/// calling for attention.
const PERIOD: f64 = 2.2;
/// How far a breath falls, as a share of the glow at its fullest.
const LOW: f64 = 0.45;
/// Fading in and out, in seconds.
const FADE_IN: f64 = 0.6;
const FADE_OUT: f64 = 0.9;
/// How long a failure's red holds before it fades, in seconds.
const TINT: f64 = 0.9;
/// How strong the glow is with the desktop's animations off.
const STILL: f64 = 0.8;
/// The fewest microseconds between two steps of the glow.
const FRAME_US: u64 = 50_000;

/// Where the glow is in its life.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Phase {
    Idle,
    Working,
    Ending { failed: bool },
}

/// The glow's state: its phase, since when in seconds, from what opacity,
/// and until when a failure's red shows over a turn that goes on.
#[derive(Clone, Copy, Debug)]
struct Motion {
    phase: Phase,
    since: f64,
    from: f64,
    tint_until: Option<f64>,
}

impl Default for Motion {
    fn default() -> Motion {
        Motion { phase: Phase::Idle, since: 0.0, from: 0.0, tint_until: None }
    }
}

pub(super) struct Glow {
    pub(super) widget: gtk::Box,
    motion: Rc<Cell<Motion>>,
    timer: Rc<RefCell<Option<glib::SourceId>>>,
}

impl Default for Glow {
    fn default() -> Glow {
        let widget = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        widget.add_css_class(GLOW);
        widget.set_valign(gtk::Align::End);
        widget.set_hexpand(true);
        widget.set_height_request(HEIGHT);
        widget.set_can_target(false);
        widget.set_can_focus(false);
        widget.set_visible(false);
        widget.set_opacity(0.0);
        Glow { widget, motion: Rc::default(), timer: Rc::default() }
    }
}

impl Glow {
    /// A turn started: fade in, or carry on breathing.
    pub(super) fn working(&self) {
        let motion = self.motion.get();
        if motion.phase == Phase::Working {
            return;
        }
        self.widget.remove_css_class(FAILED);
        self.widget.set_visible(true);
        if !animated() {
            self.stop();
            self.widget.set_opacity(STILL);
            self.motion.set(Motion { phase: Phase::Working, ..Motion::default() });
            return;
        }
        self.motion.set(Motion { phase: Phase::Working, since: now(), from: self.widget.opacity(), tint_until: None });
        self.keep_time();
    }

    /// The last turn there was is done: fade out.
    pub(super) fn done(&self) {
        if self.motion.get().phase == Phase::Idle {
            return;
        }
        self.end(false);
    }

    /// A turn failed. With more to come the red shows for a moment over the
    /// breath, which goes on; otherwise it is how the glow ends.
    pub(super) fn failed(&self, more: bool) {
        self.widget.add_css_class(FAILED);
        self.widget.set_visible(true);
        if more {
            if self.motion.get().phase != Phase::Working {
                self.working();
                self.widget.add_css_class(FAILED);
            }
            let motion = self.motion.get();
            self.motion.set(Motion { tint_until: Some(now() + TINT), ..motion });
            if !animated() {
                self.later(TINT, |glow| glow.widget.remove_css_class(FAILED));
            }
            return;
        }
        self.end(true);
    }

    fn end(&self, failed: bool) {
        if !animated() {
            self.stop();
            match failed {
                true => {
                    self.widget.set_opacity(STILL);
                    self.motion.set(Motion { phase: Phase::Ending { failed }, ..Motion::default() });
                    self.later(TINT, |glow| {
                        if matches!(glow.motion.get().phase, Phase::Ending { .. }) {
                            glow.rest();
                        }
                    });
                }
                false => self.rest(),
            }
            return;
        }
        self.motion.set(Motion { phase: Phase::Ending { failed }, since: now(), from: self.widget.opacity(), tint_until: None });
        self.keep_time();
    }

    /// Do `then` to the glow `seconds` from now, as it is then.
    fn later(&self, seconds: f64, then: impl FnOnce(&Glow) + 'static) {
        let glow = Glow { widget: self.widget.clone(), motion: self.motion.clone(), timer: self.timer.clone() };
        glib::timeout_add_local_once(Duration::from_secs_f64(seconds), move || then(&glow));
    }

    /// Hidden, and nothing running.
    fn rest(&self) {
        self.stop();
        self.motion.set(Motion::default());
        self.widget.set_opacity(0.0);
        self.widget.remove_css_class(FAILED);
        self.widget.set_visible(false);
    }

    fn stop(&self) {
        if let Some(timer) = self.timer.take() {
            timer.remove();
        }
    }

    /// Put the glow on the clock, if it is not on it already.
    fn keep_time(&self) {
        if self.timer.borrow().is_some() {
            return;
        }
        if self.step() != Step::Going {
            return self.settle_step();
        }
        let glow = Glow { widget: self.widget.clone(), motion: self.motion.clone(), timer: self.timer.clone() };
        let timer = glib::timeout_add_local(Duration::from_micros(FRAME_US), move || match glow.step() {
            Step::Going => glib::ControlFlow::Continue,
            _ => {
                // Returning Break removes it: there is nothing to remove.
                glow.timer.take();
                glow.settle_step();
                glib::ControlFlow::Break
            }
        });
        *self.timer.borrow_mut() = Some(timer);
    }

    /// After a step that stopped the clock: hidden if the glow has gone,
    /// left as it is if it holds still.
    fn settle_step(&self) {
        if self.motion.get().phase != Phase::Working {
            self.rest();
        }
    }

    /// The glow as it is now.
    fn step(&self) -> Step {
        let now = now();
        let mut motion = self.motion.get();
        if motion.tint_until.is_some_and(|until| now >= until) {
            motion.tint_until = None;
            self.motion.set(motion);
            self.widget.remove_css_class(FAILED);
        }
        if !animated() {
            // Animations were turned off part way: still from here on, or
            // gone if it was going.
            if motion.phase != Phase::Working {
                return Step::Gone;
            }
            self.widget.set_opacity(STILL);
            self.motion.set(Motion { phase: Phase::Working, ..Motion::default() });
            if motion.tint_until.is_some() {
                self.later(TINT, |glow| glow.widget.remove_css_class(FAILED));
            }
            return Step::Still;
        }
        match level(motion.phase, motion.from, now - motion.since) {
            Some(level) => {
                if (self.widget.opacity() - level).abs() >= 0.01 {
                    self.widget.set_opacity(level);
                }
                Step::Going
            }
            None => Step::Gone,
        }
    }

    /// Whether the glow is shown, whatever the window around it is doing.
    #[cfg(test)]
    pub(super) fn showing(&self) -> bool {
        self.widget.property::<bool>("visible")
    }

    /// Whether the glow is on the clock.
    #[cfg(test)]
    pub(super) fn ticking(&self) -> bool {
        self.timer.borrow().is_some()
    }
}

/// What a step of the glow left it doing.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Step {
    Going,
    Still,
    Gone,
}

/// How strong the glow is `t` seconds into `phase`, having started it at
/// `from`; `None` once it has gone.
fn level(phase: Phase, from: f64, t: f64) -> Option<f64> {
    let t = t.max(0.0);
    match phase {
        Phase::Idle => None,
        Phase::Working if t < FADE_IN => Some(from + (1.0 - from) * ease(t / FADE_IN)),
        Phase::Working => Some(breath(t - FADE_IN)),
        Phase::Ending { failed: false } if t < FADE_OUT => Some(from * (1.0 - ease(t / FADE_OUT))),
        Phase::Ending { failed: true } if t < TINT => Some(from + (1.0 - from) * ease(t / 0.2)),
        Phase::Ending { failed: true } if t < TINT + FADE_OUT => Some(1.0 - ease((t - TINT) / FADE_OUT)),
        Phase::Ending { .. } => None,
    }
}

/// A breath `t` seconds after the fade in: full at the start, down to [`LOW`] and back
/// over [`PERIOD`].
fn breath(t: f64) -> f64 {
    LOW + (1.0 - LOW) * (0.5 + 0.5 * (std::f64::consts::TAU * t / PERIOD).cos())
}

fn ease(x: f64) -> f64 {
    let x = x.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

fn now() -> f64 {
    glib::monotonic_time() as f64 / 1e6
}

/// Whether the desktop allows animation.
fn animated() -> bool {
    gtk::Settings::default().is_none_or(|settings| settings.is_gtk_enable_animations())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_turn_fades_in_breathes_slowly_and_fades_out() {
        assert_eq!(level(Phase::Working, 0.0, 0.0), Some(0.0), "from nothing");
        assert_eq!(level(Phase::Working, 0.0, FADE_IN), Some(1.0), "up to full, where the breath starts");
        let breaths: Vec<f64> = (0..200).map(|i| level(Phase::Working, 0.0, FADE_IN + i as f64 * 0.05).unwrap()).collect();
        let (low, high) = breaths.iter().fold((1.0f64, 0.0f64), |(low, high), &b| (low.min(b), high.max(b)));
        assert!((low - LOW).abs() < 0.01 && (high - 1.0).abs() < 0.01, "{low} to {high}");
        assert!((1.5..=3.0).contains(&PERIOD), "calm, about two seconds a breath");
        assert!((breath(0.3) - breath(0.3 + PERIOD)).abs() < 1e-9);

        assert_eq!(level(Phase::Ending { failed: false }, 0.8, 0.0), Some(0.8), "out from where it was");
        assert!(level(Phase::Ending { failed: false }, 0.8, FADE_OUT / 2.0).unwrap() < 0.8);
        assert_eq!(level(Phase::Ending { failed: false }, 0.8, FADE_OUT), None, "gone");
        assert_eq!(level(Phase::Idle, 0.0, 0.0), None);
    }

    #[test]
    fn a_failure_holds_its_red_a_moment_then_fades() {
        let failed = Phase::Ending { failed: true };
        assert_eq!(level(failed, 0.5, 0.0), Some(0.5));
        assert_eq!(level(failed, 0.5, TINT - 0.01), Some(1.0), "held full");
        assert!(level(failed, 0.5, TINT + FADE_OUT / 2.0).unwrap() < 1.0);
        assert_eq!(level(failed, 0.5, TINT + FADE_OUT), None);
    }

    /// Run the main loop until `done`, for at most `secs`.
    fn settle(secs: f64, done: impl Fn() -> bool) -> bool {
        let context = glib::MainContext::default();
        let deadline = std::time::Instant::now() + Duration::from_secs_f64(secs);
        while std::time::Instant::now() < deadline {
            if done() {
                return true;
            }
            context.iteration(false);
            std::thread::sleep(Duration::from_millis(5));
        }
        done()
    }

    /// Idle, the glow is hidden and nothing runs; through a turn it shows
    /// and breathes on the clock, and once done it goes and the clock stops.
    /// It never takes the pointer.
    #[test]
    fn the_glow_runs_only_while_a_turn_does() {
        crate::ui::on_gtk(|| {
            let glow = Glow::default();
            assert!(!glow.widget.can_target(), "never in the way of the hand");
            assert!(!glow.showing() && !glow.ticking(), "idle costs nothing");

            glow.working();
            assert!(glow.showing() && glow.ticking());
            assert!(settle(1.5, || glow.widget.opacity() > 0.9), "faded in");
            assert!(settle(1.5, || glow.widget.opacity() < 0.6), "and breathing: {}", glow.widget.opacity());

            glow.done();
            assert!(settle(2.0, || !glow.showing()), "faded out");
            assert!(!glow.ticking(), "and nothing left running");

            glow.failed(false);
            assert!(glow.widget.has_css_class(FAILED) && glow.showing(), "a failure shows red");
            assert!(settle(3.0, || !glow.showing()), "then goes");
            assert!(!glow.widget.has_css_class(FAILED) && !glow.ticking());
        });
    }

    /// A failure with more queued behind it is a moment of red over a glow
    /// that goes on.
    #[test]
    fn a_failure_with_more_to_come_tints_and_carries_on() {
        crate::ui::on_gtk(|| {
            let glow = Glow::default();
            glow.working();
            glow.failed(true);
            assert!(glow.widget.has_css_class(FAILED));
            assert!(settle(2.0, || !glow.widget.has_css_class(FAILED)), "the red passes");
            assert!(glow.showing() && glow.ticking(), "still working");
            glow.done();
            assert!(settle(2.0, || !glow.showing() && !glow.ticking()));
        });
    }

    /// With the desktop's animations off the glow shows still, with no
    /// clock, and goes at once.
    #[test]
    fn without_animations_the_glow_is_still() {
        crate::ui::on_gtk(|| {
            let settings = gtk::Settings::default().expect("settings");
            settings.set_gtk_enable_animations(false);
            let glow = Glow::default();
            glow.working();
            let still = glow.showing() && !glow.ticking() && glow.widget.opacity() == STILL;
            glow.done();
            let gone = !glow.showing() && !glow.ticking();
            glow.failed(false);
            let red = glow.widget.has_css_class(FAILED) && glow.showing() && !glow.ticking();
            let passed = settle(2.0, || !glow.showing());
            settings.set_gtk_enable_animations(true);
            assert!(still, "shown still, with no clock");
            assert!(gone, "and gone at once");
            assert!(red && passed, "a failure's red still shows a moment");
        });
    }
}
