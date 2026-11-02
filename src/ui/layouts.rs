//! Layouts GTK and libadwaita 1.5 lack, for `set_layout_manager`.

use gtk::prelude::*;
use gtk::subclass::prelude::*;
use gtk::{glib, gsk};

/// The space between two children, along a line and between lines.
const GAP: i32 = 8;

glib::wrapper! {
    /// Children left to right, each at its own width, starting a new line
    /// when the next one does not fit, the way words wrap. A flow box does
    /// not do this: it lines its children up in columns as wide as the
    /// widest in each, which leaves holes between narrow ones and pushes
    /// some to the far edge. Libadwaita's wrap box needs 1.7.
    pub struct Wrap(ObjectSubclass<imp::Wrap>) @extends gtk::LayoutManager;
}

impl Default for Wrap {
    fn default() -> Wrap {
        glib::Object::new()
    }
}

/// Where each child of `widget` that takes part goes in `width`, and how
/// tall the lines come to. A child wider than `width` gets a line to itself.
fn place(widget: &gtk::Widget, width: i32) -> (Vec<(gtk::Widget, gtk::Allocation)>, i32) {
    let mut placed = Vec::new();
    let (mut x, mut y, mut line) = (0, 0, 0);
    let mut next = widget.first_child();
    while let Some(child) = next {
        next = child.next_sibling();
        if !child.should_layout() {
            continue;
        }
        let w = child.measure(gtk::Orientation::Horizontal, -1).1;
        let h = child.measure(gtk::Orientation::Vertical, w).1;
        if x > 0 && x + w > width {
            (x, y, line) = (0, y + line + GAP, 0);
        }
        placed.push((child, gtk::Allocation::new(x, y, w, h)));
        x += w + GAP;
        line = line.max(h);
    }
    (placed, y + line)
}

glib::wrapper! {
    /// One child laid out across the room it is given, and when that is
    /// less than the child's minimum width, laid out at that minimum and
    /// drawn smaller, whole: a picture of something
    /// wide, which shrinks rather than spilling over its edge. Pointer and
    /// drop positions go through the same scale, so it stays usable.
    pub struct Shrink(ObjectSubclass<imp::Shrink>) @extends gtk::LayoutManager;
}

impl Default for Shrink {
    fn default() -> Shrink {
        glib::Object::new()
    }
}

/// The child of `widget`, the width it is laid out at in `width`, its
/// height there, and the scale that fits that width into `width`. The
/// minimum, not the natural width: a centre box's natural width keeps its
/// centre at the middle, twice its wider side, and would shrink a row that
/// fits.
fn shrunk(widget: &gtk::Widget, width: i32) -> Option<(gtk::Widget, i32, i32, f32)> {
    let child = widget.first_child()?;
    let least = child.measure(gtk::Orientation::Horizontal, -1).0;
    let laid = if width < 0 { least } else { least.max(width) };
    let height = child.measure(gtk::Orientation::Vertical, laid).1;
    let scale = if width < 0 || laid == 0 { 1.0 } else { width as f32 / laid as f32 };
    Some((child, laid, height, scale))
}

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct Shrink;

    #[glib::object_subclass]
    impl ObjectSubclass for Shrink {
        const NAME: &'static str = "OmachartsShrink";
        type Type = super::Shrink;
        type ParentType = gtk::LayoutManager;
    }

    impl ObjectImpl for Shrink {}

    impl LayoutManagerImpl for Shrink {
        fn request_mode(&self, _widget: &gtk::Widget) -> gtk::SizeRequestMode {
            gtk::SizeRequestMode::HeightForWidth
        }

        /// Any width at all, its child's minimum by choice; as tall as
        /// the child comes out at the width given.
        fn measure(
            &self,
            widget: &gtk::Widget,
            orientation: gtk::Orientation,
            for_size: i32,
        ) -> (i32, i32, i32, i32) {
            let Some((_, laid, height, scale)) = shrunk(widget, for_size) else { return (0, 0, -1, -1) };
            match orientation {
                gtk::Orientation::Horizontal => (0, laid, -1, -1),
                _ => {
                    let height = (height as f32 * scale).ceil() as i32;
                    (height, height, -1, -1)
                }
            }
        }

        fn allocate(&self, widget: &gtk::Widget, width: i32, _height: i32, _baseline: i32) {
            if let Some((child, laid, height, scale)) = shrunk(widget, width) {
                child.allocate(laid, height, -1, Some(gsk::Transform::new().scale(scale, scale)));
            }
        }
    }

    #[derive(Default)]
    pub struct Wrap;

    #[glib::object_subclass]
    impl ObjectSubclass for Wrap {
        const NAME: &'static str = "OmachartsWrap";
        type Type = super::Wrap;
        type ParentType = gtk::LayoutManager;
    }

    impl ObjectImpl for Wrap {}

    impl LayoutManagerImpl for Wrap {
        fn request_mode(&self, _widget: &gtk::Widget) -> gtk::SizeRequestMode {
            gtk::SizeRequestMode::HeightForWidth
        }

        /// As narrow as its widest child, as wide as all of them on one
        /// line; as tall as the lines they wrap into at the width given.
        fn measure(
            &self,
            widget: &gtk::Widget,
            orientation: gtk::Orientation,
            for_size: i32,
        ) -> (i32, i32, i32, i32) {
            if orientation == gtk::Orientation::Horizontal {
                let line = place(widget, i32::MAX).0;
                let widest = line.iter().map(|(_, at)| at.width()).max().unwrap_or(0);
                let all = line.last().map_or(0, |(_, at)| at.x() + at.width());
                return (widest, all, -1, -1);
            }
            let height = place(widget, if for_size < 0 { i32::MAX } else { for_size }).1;
            (height, height, -1, -1)
        }

        fn allocate(&self, widget: &gtk::Widget, width: i32, _height: i32, _baseline: i32) {
            for (child, at) in place(widget, width).0 {
                child.size_allocate(&at, -1);
            }
        }
    }
}
