//! A line as a row of samples to choose from: each width or pattern drawn
//! in the colour the line is, so the choice is seen rather than read.

use std::cell::RefCell;
use std::rc::Rc;

use gtk::glib;
use gtk::prelude::*;
use omacharts_chart::colors;
use omacharts_engine::indicators::LineStyle;

/// Where the line can be seen: the colour it is drawn in right now, and the
/// samples showing it.
///
/// The samples used to be painted in whatever the colour was when the panel was
/// built, so changing the colour left a purple swatch above four blue lines.
/// They ask for the colour every time they are drawn instead, and the colour
/// row repaints them when it changes one.
#[derive(Clone)]
pub struct Ink {
    colour: Rc<dyn Fn() -> String>,
    samples: Rc<RefCell<Vec<glib::WeakRef<gtk::DrawingArea>>>>,
}

impl Ink {
    pub fn new(colour: impl Fn() -> String + 'static) -> Ink {
        Ink { colour: Rc::new(colour), samples: Rc::new(RefCell::new(Vec::new())) }
    }

    pub fn colour(&self) -> String {
        (self.colour)()
    }

    pub fn watch(&self, area: &gtk::DrawingArea) {
        self.samples.borrow_mut().push(area.downgrade());
    }

    pub fn repaint(&self) {
        for area in self.samples.borrow().iter() {
            if let Some(area) = area.upgrade() {
                area.queue_draw();
            }
        }
    }
}

/// A spin button asking for a number between zero and five in halves is a
/// question nobody has an opinion about; what people want is a thicker line or
/// a thinner one, and they want to see which. Zero stays because a VWAP band
/// uses it to draw its fill with no outline.
pub const WIDTHS: [f64; 5] = omacharts_engine::indicators::STROKE_WIDTHS;

/// What a style sample is drawn at, so dashes and dots are legible whatever
/// weight the line itself is set to.
pub const LINE_SAMPLE_WIDTH: f64 = 1.5;

pub fn describe_stroke(width: f64, style: LineStyle) -> String {
    if width <= 0.0 {
        return "No line".to_string();
    }
    format!("{} · {width}px", style.label())
}

/// One option, drawn as the line it would produce.
pub fn stroke_sample(width: f64, style: LineStyle, ink: &Ink) -> gtk::DrawingArea {
    let area = gtk::DrawingArea::new();
    area.set_content_width(34);
    area.set_content_height(18);
    let ink = ink.clone();
    area.set_draw_func(move |_, cr, w, h| {
        if width <= 0.0 {
            return;
        }
        let (w, h) = (w as f64, h as f64);
        colors::set_source(cr, &ink.colour());
        cr.set_line_width(width);
        cr.set_dash(&style.dashes(width), 0.0);
        cr.set_line_cap(gtk::cairo::LineCap::Round);
        let y = (h / 2.0).round() + if width % 2.0 == 1.0 { 0.5 } else { 0.0 };
        cr.move_to(5.0, y);
        cr.line_to(w - 5.0, y);
        let _ = cr.stroke();
    });
    area
}
