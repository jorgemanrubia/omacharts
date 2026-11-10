//! The corner notice: an error, said in the window's bottom-left corner
//! until somebody dismisses it or opens the Log.
//!
//! One notice at a time. A second error replaces the words of the first and
//! counts it ("· 1 more") rather than stacking another card, and the notice
//! never takes the keyboard from the chart: its buttons are not focused by a
//! click and not reached by Tab. It is built the first time there is an
//! error and taken out of the window when dismissed, so a window with
//! nothing wrong has none.
//!
//! Which entries surface at all, and how, is [`surface`]'s to say, apart
//! from any widget.
//!
//! The same corner says when an update is ready, in [`UpdateNotice`].

use std::rc::Rc;

use adw::prelude::*;
use omacharts_engine::log::{Entry, Level};

/// How an entry reaches somebody who is not reading the Log.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Surface {
    /// In the Log only.
    Quiet,
    /// In the corner notice, until dismissed.
    Notice,
    /// In a toast, for something asked for and done.
    Toast,
}

/// What [`surface`] needs to know of the window.
#[derive(Clone, Copy, Default, Debug)]
pub struct State {
    /// The Log is open, and shows everything as it arrives.
    pub log_open: bool,
}

/// An error surfaces in the notice, unless the Log is open to show it;
/// something asked for and done, in a toast; anything else stays in the Log.
pub fn surface(entry: &Entry, state: &State) -> Surface {
    if entry.confirms {
        return Surface::Toast;
    }
    match entry.level {
        Level::Error if !state.log_open => Surface::Notice,
        _ => Surface::Quiet,
    }
}

/// The card itself, in a revealer the window lays over its bottom-left
/// corner.
pub struct Notice {
    pub revealer: gtk::Revealer,
    text: gtk::Label,
    more: gtk::Label,
}

impl Notice {
    /// `show_log` opens the Log, and `dismiss` takes the notice away.
    pub fn new(show_log: impl Fn() + 'static, dismiss: impl Fn() + 'static) -> Rc<Notice> {
        let icon = gtk::Image::from_icon_name("dialog-error-symbolic");
        icon.add_css_class("error");
        icon.set_valign(gtk::Align::Start);

        let text = gtk::Label::new(None);
        text.set_wrap(true);
        text.set_wrap_mode(gtk::pango::WrapMode::WordChar);
        text.set_lines(2);
        text.set_ellipsize(gtk::pango::EllipsizeMode::End);
        text.set_max_width_chars(44);
        text.set_xalign(0.0);
        let more = gtk::Label::new(None);
        more.add_css_class("dim-label");
        more.set_xalign(0.0);
        more.set_visible(false);
        let words = gtk::Box::new(gtk::Orientation::Vertical, 2);
        words.set_valign(gtk::Align::Center);
        words.append(&text);
        words.append(&more);

        let show = gtk::Button::with_label("Show log");
        show.connect_clicked(move |_| show_log());
        let close = gtk::Button::from_icon_name("window-close-symbolic");
        close.add_css_class("flat");
        close.add_css_class("circular");
        close.set_tooltip_text(Some("Dismiss"));
        close.connect_clicked(move |_| dismiss());
        for button in [&show, &close] {
            button.set_focus_on_click(false);
            button.set_can_focus(false);
            button.set_valign(gtk::Align::Center);
        }

        let card = gtk::Box::new(gtk::Orientation::Horizontal, 10);
        card.add_css_class("card");
        card.add_css_class("log-notice");
        card.append(&icon);
        card.append(&words);
        card.append(&show);
        card.append(&close);

        let revealer = gtk::Revealer::new();
        revealer.set_transition_type(gtk::RevealerTransitionType::SlideUp);
        revealer.set_halign(gtk::Align::Start);
        revealer.set_valign(gtk::Align::End);
        revealer.set_can_focus(false);
        revealer.set_child(Some(&card));
        Rc::new(Notice { revealer, text, more })
    }

    /// Say the newest error, and how many others are waiting behind it.
    pub fn say(&self, entry: &Entry, others: usize) {
        self.text.set_label(&entry.line());
        self.more.set_visible(others > 0);
        if others > 0 {
            self.more.set_label(&format!("· {others} more"));
        }
        self.revealer.set_reveal_child(true);
    }

    pub fn text(&self) -> String {
        self.text.label().to_string()
    }

    pub fn more(&self) -> Option<String> {
        self.more.property::<bool>("visible").then(|| self.more.label().to_string())
    }
}

/// The corner notice's other use: a downloaded update, ready, and the
/// restart that brings it in. Built the same way and laid over the same
/// corner, so it reads as the app speaking in its usual voice rather than
/// as a dialog.
pub struct UpdateNotice {
    pub revealer: gtk::Revealer,
    text: gtk::Label,
    pub restart: gtk::Button,
}

impl UpdateNotice {
    /// `restart` quits into the new version, and `dismiss` takes the notice
    /// away until a newer one is ready.
    pub fn new(restart: impl Fn() + 'static, dismiss: impl Fn() + 'static) -> Rc<UpdateNotice> {
        let icon = gtk::Image::from_icon_name("software-update-available-symbolic");
        icon.add_css_class("accent");
        icon.set_valign(gtk::Align::Center);

        let text = gtk::Label::new(None);
        text.set_xalign(0.0);
        text.set_valign(gtk::Align::Center);

        let button = gtk::Button::with_label("Restart to update");
        button.add_css_class("suggested-action");
        button.connect_clicked(move |_| restart());
        let close = gtk::Button::from_icon_name("window-close-symbolic");
        close.add_css_class("flat");
        close.add_css_class("circular");
        close.set_tooltip_text(Some("Dismiss"));
        close.connect_clicked(move |_| dismiss());
        for button in [&button, &close] {
            button.set_focus_on_click(false);
            button.set_can_focus(false);
            button.set_valign(gtk::Align::Center);
        }

        let card = gtk::Box::new(gtk::Orientation::Horizontal, 10);
        card.add_css_class("card");
        card.add_css_class("log-notice");
        card.append(&icon);
        card.append(&text);
        card.append(&button);
        card.append(&close);

        let revealer = gtk::Revealer::new();
        revealer.set_transition_type(gtk::RevealerTransitionType::SlideUp);
        revealer.set_halign(gtk::Align::Start);
        revealer.set_valign(gtk::Align::End);
        revealer.set_can_focus(false);
        revealer.set_child(Some(&card));
        Rc::new(UpdateNotice { revealer, text, restart: button })
    }

    /// Say that `version` is ready.
    pub fn say(&self, version: &str) {
        self.text.set_label(&format!("Omacharts {version} is ready"));
        self.revealer.set_reveal_child(true);
    }

    pub fn text(&self) -> String {
        self.text.label().to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_errors_surface_and_confirmations_are_toasts() {
        let closed = State { log_open: false };
        let open = State { log_open: true };
        let error = Entry::error("Lucid", "Not flattened · nothing held");
        let warning = Entry::warning("Yahoo Finance", "AAPL 1D · Rate limited");
        let info = Entry::info("Lucid", "Buy Market 2 ESZ6");
        let filled = Entry::info("Lucid", "Filled 2 ESZ6 at 7,612.25").confirming();
        let table = [
            (&error, closed, Surface::Notice),
            (&error, open, Surface::Quiet),
            (&warning, closed, Surface::Quiet),
            (&warning, open, Surface::Quiet),
            (&info, closed, Surface::Quiet),
            (&filled, closed, Surface::Toast),
            (&filled, open, Surface::Toast),
        ];
        for (entry, state, wanted) in table {
            assert_eq!(surface(entry, &state), wanted, "{} with {state:?}", entry.text);
        }
    }
}
