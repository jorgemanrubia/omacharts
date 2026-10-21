//! A line where a dragged thing will land, for the short lists put in order
//! by dragging one entry onto another.
//!
//! GTK frames whichever widget the pointer is over, which says "onto this"
//! when what the drop does is "next to this". The frame is taken off these
//! widgets, and a line is drawn on the edge the dragged one will end up
//! against instead — in the accent the frame was drawn in, so it is the same
//! mark saying the right thing.

use gtk::glib;
use gtk::prelude::*;

/// Which edge of the widget under the pointer the dragged one lands against.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Edge {
    /// Above a row, or left of a tab.
    Before,
    /// Below a row, or right of a tab.
    After,
}

const MARKED: &str = "drop-marked";
const BEFORE: &str = "drop-before";
const AFTER: &str = "drop-after";

/// Where something moved from `from` onto the entry at `onto` lands, when
/// the drop takes it out of the list and puts it back at `onto`'s index:
/// after that entry going down the list, in its place going up, and nowhere
/// dropped on itself.
pub fn edge(from: usize, onto: usize) -> Option<Edge> {
    match from.cmp(&onto) {
        std::cmp::Ordering::Less => Some(Edge::After),
        std::cmp::Ordering::Greater => Some(Edge::Before),
        std::cmp::Ordering::Equal => None,
    }
}

/// A drop target on `widget` that marks where the payload would land.
///
/// `landing` reads the payload and says which edge, or nothing when this is
/// not somewhere it can go; `drop` is the drop itself, as `connect_drop`
/// would take it. The line goes on leave and on drop, so a drop the widget
/// survives does not leave it marked.
pub fn target(
    widget: &impl IsA<gtk::Widget>,
    landing: impl Fn(&glib::Value) -> Option<Edge> + 'static,
    drop: impl Fn(&glib::Value) -> bool + 'static,
) -> gtk::DropTarget {
    let widget = widget.as_ref().clone();
    widget.add_css_class(MARKED);
    let target = gtk::DropTarget::new(glib::Type::STRING, gtk::gdk::DragAction::MOVE);
    // Read the payload while it is over us rather than only on the drop, so
    // the line can say where this particular thing would land.
    target.set_preload(true);
    // Where it lands depends on what is dragged and onto what, never on where
    // over the widget the pointer is, so it is worked out once per hover
    // rather than on every motion: `landing` may read the store.
    let landed: std::rc::Rc<std::cell::Cell<Option<Option<Edge>>>> = Default::default();
    let marked = widget.clone();
    let worked_out = landed.clone();
    target.connect_motion(move |target, _, _| {
        if worked_out.get().is_none()
            && let Some(value) = target.value()
        {
            let at = landing(&value);
            worked_out.set(Some(at));
            mark(&marked, at);
        }
        gtk::gdk::DragAction::MOVE
    });
    let marked = widget.clone();
    let forgotten = landed.clone();
    target.connect_leave(move |_| {
        forgotten.set(None);
        mark(&marked, None);
    });
    let marked = widget;
    target.connect_drop(move |_, value, _, _| {
        landed.set(None);
        mark(&marked, None);
        drop(value)
    });
    target
}

fn mark(widget: &gtk::Widget, at: Option<Edge>) {
    let (on, off) = match at {
        Some(Edge::Before) => (Some(BEFORE), AFTER),
        Some(Edge::After) => (Some(AFTER), BEFORE),
        None => (None, BEFORE),
    };
    widget.remove_css_class(off);
    match on {
        Some(class) => widget.add_css_class(class),
        None => widget.remove_css_class(AFTER),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_line_goes_where_the_drop_puts_it() {
        assert_eq!(edge(0, 2), Some(Edge::After));
        assert_eq!(edge(3, 1), Some(Edge::Before));
        assert_eq!(edge(2, 2), None);
    }
}
