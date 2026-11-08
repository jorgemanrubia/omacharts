//! Small shapes the chart draws and the windows around it draw the same
//! way: an arrowhead and a rounded box.

use omacharts_engine::ArrowHead;

/// An arrowhead at `tip`, pointing away from `tail`, `size` long, in the
/// shape asked for: a filled triangle, an open chevron stroked at the
/// line's width, or a swept barb with a notch where the line meets it.
/// The source colour is already set.
pub fn paint_head(
    cr: &gtk::cairo::Context,
    tail: (f64, f64),
    tip: (f64, f64),
    width: f64,
    size: f64,
    shape: ArrowHead,
) {
    let (dx, dy) = (tip.0 - tail.0, tip.1 - tail.1);
    let length = dx.hypot(dy);
    if length < 1.0 {
        return;
    }
    let (ux, uy) = (dx / length, dy / length);
    let (bx, by) = (tip.0 - ux * size, tip.1 - uy * size);
    match shape {
        ArrowHead::Filled => {
            let (px, py) = (-uy * size * 0.45, ux * size * 0.45);
            cr.move_to(tip.0, tip.1);
            cr.line_to(bx + px, by + py);
            cr.line_to(bx - px, by - py);
            cr.close_path();
            let _ = cr.fill();
        }
        ArrowHead::Open => {
            let (px, py) = (-uy * size * 0.5, ux * size * 0.5);
            cr.save().ok();
            cr.set_line_width(width.max(1.0));
            cr.set_line_cap(gtk::cairo::LineCap::Round);
            cr.set_line_join(gtk::cairo::LineJoin::Round);
            cr.move_to(bx + px, by + py);
            cr.line_to(tip.0, tip.1);
            cr.line_to(bx - px, by - py);
            let _ = cr.stroke();
            cr.restore().ok();
        }
        ArrowHead::Barb => {
            let (px, py) = (-uy * size * 0.55, ux * size * 0.55);
            let (nx, ny) = (tip.0 - ux * size * 0.6, tip.1 - uy * size * 0.6);
            cr.move_to(tip.0, tip.1);
            cr.line_to(bx + px, by + py);
            cr.line_to(nx, ny);
            cr.line_to(bx - px, by - py);
            cr.close_path();
            let _ = cr.fill();
        }
    }
}

/// A box with its corners rounded to `r`, as a path, at most half its
/// shorter side.
pub fn rounded(cr: &gtk::cairo::Context, x: f64, y: f64, w: f64, h: f64, r: f64) {
    let r = r.min(w / 2.0).min(h / 2.0);
    cr.new_sub_path();
    cr.arc(x + w - r, y + r, r, -std::f64::consts::FRAC_PI_2, 0.0);
    cr.arc(x + w - r, y + h - r, r, 0.0, std::f64::consts::FRAC_PI_2);
    cr.arc(x + r, y + h - r, r, std::f64::consts::FRAC_PI_2, std::f64::consts::PI);
    cr.arc(x + r, y + r, r, std::f64::consts::PI, 1.5 * std::f64::consts::PI);
    cr.close_path();
}
