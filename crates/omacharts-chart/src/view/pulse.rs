//! The breath of a mark that asks for one: the open position's line.
//!
//! A faint halo either side of the line, in the line's colour, that swells
//! and fades on a slow period. The line itself, its label and its tabs are
//! never touched: they are drawn by the chart, and the halo is drawn beside
//! them on a layer of its own.
//!
//! A layer of its own because GTK4 invalidates a whole widget or none of it,
//! and a breath redrawn on the chart would re-rasterise every candle on
//! screen a dozen times a second to change one band of pixels. Here a frame
//! of the breath is a handful of gradient nodes and nothing else: no cairo
//! surface, no layout, the chart's own frame composited from where it was.
//! It takes no events and asks for no size, so it changes nothing about how
//! the chart is driven or laid out.
//!
//! Its clock is a timer, not the frame clock's tick: a tick would hold the
//! window's frame clock running at the display's rate for as long as the
//! breath lasts, where the timer wakes at most twenty times a second, asks
//! for a frame only when the breath has moved by a step the eye could see,
//! and sleeps through the rest between breaths. One timer for every chart,
//! so several breathing are drawn in the same frames. There is no timer at
//! all unless some line is breathing: on screen, and the desktop allowing
//! animation; and when the frames it asks for are not being drawn, as for a
//! window the compositor is not showing, it stops asking and only looks in
//! once a second. While the hand is on a chart that breathes — a pan, a
//! zoom on an axis, a mark or a drawing dragged — every breath holds where
//! it is rather than asking for frames of its own beside the chart's.

use std::cell::{Cell, RefCell};

use gtk::glib;
use gtk::gdk;
use gtk::graphene;
use gtk::gsk;
use gtk::prelude::*;
use gtk::subclass::prelude::*;

/// The CSS class the layer carries, so the screenshot can leave it out by
/// name: a picture is of one moment, and which moment of a breath it caught
/// is not what it is of.
pub const PULSE_LAYER: &str = "chart-pulse-layer";

/// One breath, in seconds: a rise, a slower fall, and a rest at nothing.
/// Slower than a breath at rest, so it never reads as a thing breathing.
const RISE: f64 = 1.5;
const FALL: f64 = 2.7;
const REST: f64 = 1.0;
const PERIOD: f64 = RISE + FALL + REST;
/// How strong the halo is against the line's edge at the top of a breath,
/// as an alpha of the line's colour, on a dark ground and on a light one:
/// light on dark carries further than a tint on white does.
pub(super) const PEAK_DARK: f64 = 0.11;
pub(super) const PEAK_LIGHT: f64 = 0.07;
/// How far the halo reaches from the line's edge, in pixels.
pub(super) const REACH: f64 = 4.0;
/// How far the halo takes to come up from nothing beside a label, and at
/// any other end of it, in pixels: it never starts or stops on an edge.
pub(super) const LABEL_FADE: f64 = 24.0;
pub(super) const END_FADE: f64 = 16.0;
/// The fewest microseconds between two frames of the breath, which is how
/// often its timer wakes while it is moving.
const MIN_FRAME_US: i64 = 50_000;
/// How long a frame asked for may go undrawn before the window is taken as
/// not shown, and how often it is looked in on then.
const UNSEEN_US: i64 = 500_000;
const LOOK_IN_US: i64 = 1_000_000;

/// A stretch of the line the halo runs along, from `from` to `to`, faded
/// in over `fade_in` pixels and out over `fade_out`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Span {
    pub from: f64,
    pub to: f64,
    pub fade_in: f64,
    pub fade_out: f64,
}

/// Where the halo goes: either side of a line at `y`, `half` its weight,
/// along `spans` of it, in `colour` up to `peak`, kept between `top` and
/// `bottom`.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Band {
    pub y: f64,
    pub half: f64,
    pub spans: Vec<Span>,
    pub colour: gdk::RGBA,
    pub peak: f64,
    pub top: f64,
    pub bottom: f64,
}

/// How strong the breath is `t` seconds into it, 0 to 1: up over
/// [`RISE`], down over the longer [`FALL`], each eased at both ends, then
/// nothing for [`REST`] — when no frame of it is drawn at all.
pub(super) fn breath(t: f64) -> f64 {
    let ease = |x: f64| {
        let x = x.clamp(0.0, 1.0);
        x * x * (3.0 - 2.0 * x)
    };
    match t.rem_euclid(PERIOD) {
        t if t < RISE => ease(t / RISE),
        t if t < RISE + FALL => 1.0 - ease((t - RISE) / FALL),
        _ => 0.0,
    }
}

/// How many steps a breath topping out at `peak` is drawn in: one for every
/// two levels of alpha out of 255 at the line's edge. Laid over the ground
/// the line is drawn on, that is about one level of the brightest channel —
/// the least a screen shows — so a frame is only asked for when what it
/// draws would look different, and never for one that would not.
fn steps(peak: f64) -> f64 {
    (peak * 255.0 / 2.0).round().max(1.0)
}

type Source = Box<dyn Fn(f64, f64) -> Option<Band>>;
type Held = Box<dyn Fn() -> bool>;

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct PulseLayer {
        /// Where the halo goes on a layer `width` by `height`, worked out
        /// again only after the chart was drawn.
        pub(super) source: RefCell<Option<Source>>,
        /// Whether the hand is on the chart, which holds the breath.
        pub(super) held: RefCell<Option<Held>>,
        pub(super) band: RefCell<Option<Band>>,
        /// The chart has been drawn since the band was worked out.
        pub(super) stale: Cell<bool>,
        /// The step of the breath last drawn.
        pub(super) step: Cell<i32>,
        pub(super) drawn_at: Cell<i64>,
        /// A frame has been asked for and not drawn yet.
        pub(super) waiting: Cell<bool>,
        /// On the clock: on screen, with a line to breathe, and the desktop
        /// allowing animation.
        pub(super) breathing: Cell<bool>,
        pub(super) animations: RefCell<Option<glib::SignalHandlerId>>,
        /// How many frames of the halo were drawn, for the tests.
        pub(super) frames: Cell<u64>,
        /// The halo at the top of a breath as a share of the band's own
        /// peak: one, unless a test asks for another strength to compare.
        pub(super) strength: Cell<f64>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for PulseLayer {
        const NAME: &'static str = "OmachartsPulseLayer";
        type Type = super::PulseLayer;
        type ParentType = gtk::Widget;
    }

    impl ObjectImpl for PulseLayer {
        fn dispose(&self) {
            if self.breathing.replace(false) {
                let _ = CLOCK.try_with(|clock| clock.leave(&self.obj()));
            }
            if let (Some(id), Some(settings)) = (self.animations.take(), gtk::Settings::default()) {
                settings.disconnect(id);
            }
        }
    }

    impl WidgetImpl for PulseLayer {
        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            self.obj().paint(snapshot);
        }

        fn map(&self) {
            self.parent_map();
            self.obj().keep_time();
        }

        fn unmap(&self) {
            self.parent_unmap();
            self.obj().keep_time();
        }
    }
}

glib::wrapper! {
    /// The layer the breath is drawn on, over the chart and under the
    /// pointer's layer.
    pub struct PulseLayer(ObjectSubclass<imp::PulseLayer>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl PulseLayer {
    /// A layer whose halo goes where `source` says, held while `held`.
    pub(super) fn new(source: impl Fn(f64, f64) -> Option<Band> + 'static, held: impl Fn() -> bool + 'static) -> PulseLayer {
        let layer: PulseLayer = glib::Object::new();
        layer.add_css_class(PULSE_LAYER);
        layer.set_can_target(false);
        layer.set_can_focus(false);
        layer.set_hexpand(true);
        layer.set_vexpand(true);
        let imp = layer.imp();
        *imp.source.borrow_mut() = Some(Box::new(source));
        *imp.held.borrow_mut() = Some(Box::new(held));
        imp.stale.set(true);
        imp.strength.set(1.0);
        if let Some(settings) = gtk::Settings::default() {
            let weak = layer.downgrade();
            let id = settings.connect_gtk_enable_animations_notify(move |_| {
                if let Some(layer) = weak.upgrade() {
                    layer.keep_time();
                    layer.queue_draw();
                }
            });
            *imp.animations.borrow_mut() = Some(id);
        }
        layer
    }

    /// The chart under it was just drawn, `wanted` saying whether any of
    /// its marks breathes: the band is worked out again with the frame the
    /// chart is in. Nothing at all happens on a chart with no such mark and
    /// no halo showing.
    pub(super) fn chart_drawn(&self, wanted: bool) {
        let imp = self.imp();
        if !wanted && imp.band.borrow().is_none() {
            return;
        }
        imp.stale.set(true);
        self.queue_draw();
    }

    /// Whether the breath is on the clock.
    pub(super) fn ticking(&self) -> bool {
        self.imp().breathing.get()
    }

    /// How strong the halo is drawn now, 0 to 1 of its peak.
    pub(super) fn level(&self) -> f64 {
        match animated() {
            true => self.imp().step.get() as f64 / self.steps(),
            false => 0.0,
        }
    }

    /// How many steps the breath is drawn in, for the band as it is.
    fn steps(&self) -> f64 {
        let imp = self.imp();
        let peak = imp.band.borrow().as_ref().map_or(PEAK_DARK, |band| band.peak);
        steps(peak * imp.strength.get())
    }

    /// How many frames of the halo have been drawn.
    #[cfg(test)]
    pub(super) fn frames(&self) -> u64 {
        self.imp().frames.get()
    }

    /// Where the halo is, as last worked out.
    #[cfg(test)]
    pub(super) fn band(&self) -> Option<Band> {
        self.imp().band.borrow().clone()
    }

    /// Draw the halo at `level` of a breath, at `strength` times its
    /// peak, as a picture of one moment: the clock let go of, until it is
    /// asked for again.
    #[cfg(test)]
    pub(super) fn show(&self, level: f64, strength: f64) {
        let imp = self.imp();
        if imp.breathing.replace(false) {
            CLOCK.with(|clock| clock.leave(self));
        }
        imp.strength.set(strength);
        imp.step.set((level * self.steps()).round() as i32);
        self.queue_draw();
    }

    /// Take the clock up again from the start of a breath.
    #[cfg(test)]
    pub(super) fn restart(&self) {
        let imp = self.imp();
        CLOCK.with(|clock| clock.phase.set(0.0));
        imp.stale.set(true);
        self.queue_draw();
    }

    fn paint(&self, snapshot: &gtk::Snapshot) {
        let imp = self.imp();
        imp.waiting.set(false);
        if imp.stale.replace(false) {
            let band = imp.source.borrow().as_ref().and_then(|source| source(self.width() as f64, self.height() as f64));
            *imp.band.borrow_mut() = band;
            self.keep_time();
        }
        let level = self.level();
        let band = imp.band.borrow();
        let Some(band) = band.as_ref().filter(|_| level > 0.0) else { return };
        imp.frames.set(imp.frames.get() + 1);
        halo(snapshot, band, band.peak * imp.strength.get() * level, self.width() as f32);
    }

    /// Put the breath on the clock while there is a line to breathe, the
    /// layer is on screen and animation is allowed; take it off otherwise.
    fn keep_time(&self) {
        let imp = self.imp();
        let wanted = self.is_mapped() && animated() && imp.band.borrow().is_some();
        if wanted != imp.breathing.replace(wanted) {
            imp.waiting.set(false);
            CLOCK.with(|clock| match wanted {
                true => clock.join(self),
                false => clock.leave(self),
            });
        }
    }

    /// The breath at `phase`, `now` in microseconds: drawn again only when
    /// the step it is at changed, and the last frame of it is far enough
    /// behind and was drawn. Whether the window it is in is being drawn at
    /// all, as far as it can tell.
    fn breathe(&self, phase: f64, now: i64) -> bool {
        let imp = self.imp();
        let since = now - imp.drawn_at.get();
        if imp.waiting.get() {
            return since < UNSEEN_US;
        }
        let step = (breath(phase) * self.steps()).round() as i32;
        if step != imp.step.get() && since >= MIN_FRAME_US {
            imp.step.set(step);
            imp.drawn_at.set(now);
            imp.waiting.set(true);
            self.queue_draw();
        }
        true
    }
}

thread_local! {
    /// The one clock every breath keeps time by, so that four charts
    /// breathing are four layers drawn in the same frame, at the same moment
    /// of the same breath, rather than four timers and four frames.
    static CLOCK: Clock = Clock::default();
}

#[derive(Default)]
struct Clock {
    layers: RefCell<Vec<glib::WeakRef<PulseLayer>>>,
    /// How far through a breath it is, in seconds.
    phase: Cell<f64>,
    last_tick: Cell<Option<i64>>,
    /// How long it last asked to sleep, in seconds.
    asked: Cell<f64>,
    timer: RefCell<Option<glib::SourceId>>,
}

impl Clock {
    fn join(&self, layer: &PulseLayer) {
        self.layers.borrow_mut().push(layer.downgrade());
        if self.timer.borrow().is_none() {
            self.last_tick.set(None);
            self.wake_in(0);
        }
    }

    fn leave(&self, layer: &PulseLayer) {
        let mut layers = self.layers.borrow_mut();
        layers.retain(|kept| kept.upgrade().is_some_and(|kept| &kept != layer));
        if layers.is_empty()
            && let Some(timer) = self.timer.take()
        {
            timer.remove();
        }
    }

    /// Wake `micros` from now.
    fn wake_in(&self, micros: i64) {
        self.asked.set(micros as f64 / 1e6);
        let timer = glib::timeout_add_local_once(std::time::Duration::from_micros(micros.max(0) as u64), || {
            CLOCK.with(|clock| {
                // Fired, and so gone: there is nothing to remove.
                clock.timer.take();
                if let Some(next) = clock.tick(glib::monotonic_time()) {
                    clock.wake_in(next);
                }
            })
        });
        *self.timer.borrow_mut() = Some(timer);
    }

    /// The breath at `now`, in microseconds: moved on unless a hand is on
    /// any chart that breathes, and each layer drawn again as its step
    /// changes. How long until it next wakes, or never again with nothing
    /// left to breathe.
    fn tick(&self, now: i64) -> Option<i64> {
        let layers: Vec<PulseLayer> = self.layers.borrow().iter().filter_map(|layer| layer.upgrade()).collect();
        if layers.is_empty() {
            self.layers.borrow_mut().clear();
            return None;
        }
        let since = self.last_tick.replace(Some(now)).map_or(0.0, |then| (now - then) as f64 / 1e6);
        if layers.iter().any(|layer| layer.imp().held.borrow().as_ref().is_some_and(|held| held())) {
            return Some(MIN_FRAME_US);
        }
        // A loop held up longer than it asked to sleep moves the breath on
        // by no more than a quarter of a second for it, rather than jumping.
        let phase = (self.phase.get() + since.clamp(0.0, self.asked.get() + 0.25)) % PERIOD;
        self.phase.set(phase);
        // Not short-circuited: every layer has its step brought up to date.
        let seen = layers.iter().fold(false, |seen, layer| layer.breathe(phase, now) | seen);
        if !seen {
            return Some(LOOK_IN_US);
        }
        // At rest between breaths, sleep until the next one.
        let resting = phase >= RISE + FALL && layers.iter().all(|layer| layer.imp().step.get() == 0);
        Some(match resting {
            true => (((PERIOD - phase) * 1e6) as i64).max(1_000),
            false => MIN_FRAME_US,
        })
    }
}

/// Whether the desktop allows animation.
fn animated() -> bool {
    gtk::Settings::default().is_none_or(|settings| settings.is_gtk_enable_animations())
}

/// The halo of `band` at `alpha` beside the line: a gradient either side,
/// strongest against the line's edge and gone at [`REACH`], falling off as
/// a cube so it hugs the line and has no edge of its own; each stretch of it
/// faded in and out along the line through a mask.
fn halo(snapshot: &gtk::Snapshot, band: &Band, alpha: f64, width: f32) {
    const FALL: [f32; 5] = [0.0, 0.25, 0.5, 0.75, 1.0];
    let stops: Vec<gsk::ColorStop> = FALL
        .iter()
        .map(|t| {
            let mut colour = band.colour;
            colour.set_alpha((alpha * f64::from((1.0 - t).powi(3))) as f32);
            gsk::ColorStop::new(*t, colour)
        })
        .collect();
    let (above, below) = ((band.y - band.half) as f32, (band.y + band.half) as f32);
    let reach = REACH as f32;
    snapshot.push_clip(&graphene::Rect::new(0.0, band.top as f32, width, (band.bottom - band.top) as f32));
    for span in &band.spans {
        let (x, w) = (span.from as f32, (span.to - span.from) as f32);
        if w <= 0.0 {
            continue;
        }
        // The two fades share a short stretch between them rather than
        // overlapping.
        let fade_in = (span.fade_in as f32).min(w / 2.0) / w;
        let fade_out = (span.fade_out as f32).min(w / 2.0) / w;
        let (clear, solid) = (gdk::RGBA::new(0.0, 0.0, 0.0, 0.0), gdk::RGBA::new(0.0, 0.0, 0.0, 1.0));
        let along = [
            gsk::ColorStop::new(0.0, clear),
            gsk::ColorStop::new(fade_in, solid),
            gsk::ColorStop::new(1.0 - fade_out, solid),
            gsk::ColorStop::new(1.0, clear),
        ];
        let bounds = graphene::Rect::new(x, above - reach, w, below - above + 2.0 * reach);
        snapshot.push_mask(gsk::MaskMode::Alpha);
        snapshot.append_linear_gradient(&bounds, &graphene::Point::new(x, 0.0), &graphene::Point::new(x + w, 0.0), &along);
        snapshot.pop();
        snapshot.append_linear_gradient(
            &graphene::Rect::new(x, above - reach, w, reach),
            &graphene::Point::new(x, above),
            &graphene::Point::new(x, above - reach),
            &stops,
        );
        snapshot.append_linear_gradient(
            &graphene::Rect::new(x, below, w, reach),
            &graphene::Point::new(x, below),
            &graphene::Point::new(x, below + reach),
            &stops,
        );
        snapshot.pop();
    }
    snapshot.pop();
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A breath comes up from nothing, tops out, falls back more slowly
    /// than it rose, and rests at nothing before the next.
    #[test]
    fn a_breath_rises_falls_more_slowly_and_rests() {
        assert_eq!(breath(0.0), 0.0);
        assert!((breath(RISE) - 1.0).abs() < 1e-12);
        assert!(breath(RISE + FALL).abs() < 1e-12);
        for n in 0..10 {
            assert_eq!(breath(RISE + FALL + REST * n as f64 / 10.0), 0.0, "not resting");
        }
        for n in 1..100 {
            let t = RISE * n as f64 / 100.0;
            assert!(breath(t) > breath(t - 0.01), "not rising at {t}");
            let u = RISE + FALL * n as f64 / 100.0;
            assert!(breath(u) < breath(u - 0.01), "not falling at {u}");
        }
        const { assert!(FALL > RISE) };
        assert!((breath(0.7) - breath(0.7 + PERIOD)).abs() < 1e-12, "not the same breath again");
    }
}
