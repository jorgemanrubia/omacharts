//! A chart's row of resolutions: one toggle a resolution, the one showing
//! pressed, in the order they are handed.

use gtk::prelude::*;
use omacharts_engine::Timeframe;

/// The buttons of a strip, each with the resolution it picks.
pub type Buttons = Vec<(Timeframe, gtk::ToggleButton)>;

/// An empty strip, hidden until it is filled and put where it fits.
pub fn new() -> gtk::Box {
    let strip = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    strip.add_css_class("linked");
    strip.add_css_class("timeframe-strip");
    strip.set_valign(gtk::Align::Center);
    strip.set_visible(false);
    strip
}

/// Put a button on `strip` for every one of `ladder`, `current` pressed,
/// in place of what it held, and answer them. `picked` hears a click.
pub fn fill(strip: &gtk::Box, ladder: &[Timeframe], current: Timeframe, picked: impl Fn(Timeframe) + 'static) -> Buttons {
    while let Some(child) = strip.first_child() {
        strip.remove(&child);
    }
    let picked = std::rc::Rc::new(picked);
    let mut first: Option<gtk::ToggleButton> = None;
    let mut buttons = Vec::new();
    for timeframe in ladder.iter().copied() {
        let button = gtk::ToggleButton::with_label(&timeframe.label());
        button.add_css_class("flat");
        button.set_tooltip_text(Some(&timeframe.description()));
        // A click picks a resolution; it does not move the keyboard. Taking
        // the focus left a ring on the button that outlived the click, and
        // the keys meant for the chart went to the strip instead.
        button.set_focus_on_click(false);
        match &first {
            Some(anchor) => button.set_group(Some(anchor)),
            None => first = Some(button.clone()),
        }
        button.set_active(timeframe == current);
        let picked = picked.clone();
        button.connect_toggled(move |button| {
            if button.is_active() {
                picked(timeframe);
            }
        });
        strip.append(&button);
        buttons.push((timeframe, button));
    }
    buttons
}

/// Press the button of `current`, and answer the button to hand the
/// keyboard to, for the caller to give it once it has let go of the list.
///
/// Keeps the keyboard with the resolution it is choosing. Clicking the
/// strip leaves the focus ring on the button clicked, and stepping with the
/// keys from there moved the pressed state along while the ring stayed put
/// — two answers to "which one is this", a button apart. Only when the
/// strip already has the keyboard: taking it from the chart would send the
/// arrow keys somewhere nobody pointed them.
pub fn mark(buttons: &[(Timeframe, gtk::ToggleButton)], current: Timeframe) -> Option<gtk::ToggleButton> {
    let holds_keyboard = buttons.iter().any(|(_, button)| button.has_focus());
    for (listed, button) in buttons {
        button.set_active(*listed == current);
    }
    holds_keyboard.then(|| buttons.iter().find(|(listed, _)| *listed == current)).flatten().map(|(_, button)| button.clone())
}
