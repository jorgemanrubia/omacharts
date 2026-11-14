//! Dictation in the window: a dictation that lands here outside a text box
//! goes to Omarchy's default agent, and what became of it is said in a
//! toast. See [`crate::dictation`] for how it is told from a paste and how
//! the agent is run.

use std::cell::{Cell, OnceCell};
use std::rc::Rc;

use adw::prelude::*;
use gtk::{gio, glib};
use omacharts_engine::log::Entry;

use super::glow::Glow;
use super::Window;
use crate::dictation::{superwhisper, Dispatcher, Event, Settings, World};

/// What the window keeps of dictation.
#[derive(Default)]
pub(super) struct Dictation {
    /// The queue to the agent, started by the first dictation.
    dispatcher: OnceCell<Dispatcher>,
    /// Superwhisper's number for the last dictation sent, so that pasting
    /// the same words again is a paste.
    heard: Cell<Option<u64>>,
    /// Dictations sent and not yet done with, here on the main loop.
    outstanding: Cell<usize>,
    /// The glow along the bottom of the window while the agent works.
    glow: Glow,
}

/// The longest a toast quotes; the Log has the rest.
const QUOTE: usize = 80;

impl Window {
    /// Ctrl+V outside a text box. A dictation Superwhisper has just pasted
    /// here goes to the agent; anything else pastes what was copied in the
    /// window, as it always has.
    pub(super) fn paste_or_dictation(self: &Rc<Self>) {
        self.paste_or_dictation_from(superwhisper::socket());
    }

    /// The same, asking Superwhisper at `socket`.
    fn paste_or_dictation_from(self: &Rc<Self>, socket: Option<std::path::PathBuf>) {
        let Some(socket) = socket.filter(|socket| socket.exists()) else {
            self.paste();
            return;
        };
        let clipboard = self.window.clipboard();
        let this = self.clone();
        glib::spawn_future_local(async move {
            let status = gio::spawn_blocking(move || superwhisper::status(&socket)).await.ok().flatten();
            let heard = match status {
                Some(status) => {
                    let pasted = clipboard.read_text_future().await.ok().flatten().unwrap_or_default();
                    superwhisper::heard(&status, &pasted, now())
                }
                None => None,
            };
            match heard.filter(|heard| this.dictation.heard.get() != Some(heard.session)) {
                Some(heard) => {
                    this.dictation.heard.set(Some(heard.session));
                    this.dictated(heard.text);
                }
                None => {
                    this.paste();
                }
            }
        });
    }

    /// Send `said` to the agent, with the extra context and the models as
    /// they are now.
    pub(super) fn dictated(self: &Rc<Self>, said: String) {
        let settings = Settings::of(&self.store);
        let dispatcher = self.dictation.dispatcher.get_or_init(|| self.start_dictation(World::omarchy()));
        let ahead = dispatcher.send(said.clone(), settings);
        self.dictation.outstanding.set(self.dictation.outstanding.get() + 1);
        if ahead > 0 {
            let waiting = match ahead {
                1 => "after the one the agent is on".to_string(),
                n => format!("after {n} more"),
            };
            self.tell(Entry::info("Agent", format!("Queued “{}” {waiting}", quoted(&said))).confirming());
        }
    }

    /// The queue to the agent, saying here what becomes of each dictation.
    fn start_dictation(self: &Rc<Self>, world: World) -> Dispatcher {
        let (sender, receiver) = async_channel::unbounded::<Event>();
        let weak = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            while let Ok(event) = receiver.recv().await {
                let Some(this) = weak.upgrade() else { return };
                this.glow_for(&event);
                this.tell(said_of(event));
            }
        });
        Dispatcher::start(world, move |event| {
            let _ = sender.send_blocking(event);
        })
    }

    /// Lay the glow over the bottom of the window, in `overlay`.
    pub(super) fn lay_glow(&self, overlay: &gtk::Overlay) {
        overlay.add_overlay(&self.dictation.glow.widget);
    }

    /// The glow, as `event` leaves the agent's work: showing from the
    /// first turn started until the last dictation sent is done with.
    fn glow_for(&self, event: &Event) {
        let glow = &self.dictation.glow;
        if let Event::Started { .. } = event {
            return glow.working();
        }
        let left = self.dictation.outstanding.get().saturating_sub(1);
        self.dictation.outstanding.set(left);
        match event {
            Event::Failed { .. } => glow.failed(left > 0),
            _ if left == 0 => glow.done(),
            _ => {}
        }
    }

    /// Say `entry`, with somewhere for a toast to show.
    fn tell(&self, entry: Entry) {
        if entry.confirms {
            self.ensure_toasts();
        }
        self.say(entry);
    }

    #[cfg(test)]
    pub(super) fn start_dictation_in(self: &Rc<Self>, world: World) {
        let dispatcher = self.start_dictation(world);
        let _ = self.dictation.dispatcher.set(dispatcher);
    }
}

/// How each thing that becomes of a dictation is said: a toast when it is
/// sent and when it is done, the corner notice when it could not be.
fn said_of(event: Event) -> Entry {
    match event {
        Event::Started { agent, said } => Entry::info(agent, format!("Sent “{}”", quoted(&said))).confirming(),
        Event::Done { agent, said, reply } => {
            let text = match reply.as_deref().and_then(first_line) {
                Some(reply) => format!("Done · {}", quoted(reply)),
                None => format!("Done “{}”", quoted(&said)),
            };
            let entry = Entry::info(agent, text).confirming();
            match reply {
                Some(reply) => entry.detail(format!("“{}”\n\n{reply}", said.trim())),
                None => entry,
            }
        }
        Event::Opened { agent, said } => {
            Entry::info(agent, format!("Opened in a terminal with “{}”", quoted(&said))).confirming()
        }
        Event::Failed { said, why, detail } => {
            let detail = match detail {
                Some(detail) => format!("“{}”\n\n{detail}", said.trim()),
                None => format!("“{}”", said.trim()),
            };
            Entry::error("Agent", why).detail(detail)
        }
        Event::StartedOver => Entry::info("Agent", "Starting a new conversation").confirming(),
    }
}

fn first_line(text: &str) -> Option<&str> {
    text.lines().map(str::trim).find(|line| !line.is_empty())
}

/// `text` short enough for a toast.
fn quoted(text: &str) -> String {
    let text = text.trim();
    match text.chars().count() > QUOTE {
        true => format!("{}…", text.chars().take(QUOTE - 1).collect::<String>().trim_end()),
        false => text.to_string(),
    }
}

fn now() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs_f64())
        .unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dictation::agent::{Invocation, Output};
    use crate::store::Store;
    use omacharts_engine::log::Level;
    use std::sync::{Arc, Mutex};

    /// A dictation in the window: sent with the context as Preferences has
    /// it, queued behind the one running, said in toasts, and the agent's
    /// answer kept in the Log. The agent is a closure; nothing is launched.
    #[test]
    fn a_dictation_goes_to_the_agent_and_is_said_in_toasts() {
        crate::ui::on_gtk(|| {
            let store = Rc::new(Store::memory().expect("a store"));
            store.set_setting(crate::dictation::CONTEXT, "Use Sonnet.");
            store.set_setting("agent_model.claude", "haiku");
            let window = crate::ui::window::trade::tests::window(&store);
            let log = crate::ui::window::log::tests::private(&window);
            let asked = Arc::new(Mutex::new(Vec::<Invocation>::new()));
            let seen = asked.clone();
            window.start_dictation_in(World {
                default_agent: Box::new(|| "claude".into()),
                invocation: Box::new(|agent, arguments| Invocation {
                    program: agent.program().into(),
                    arguments,
                    directory: std::env::temp_dir(),
                    environment: vec![],
                }),
                run: Box::new(move |invocation| {
                    seen.lock().unwrap().push(invocation.clone());
                    std::thread::sleep(std::time::Duration::from_millis(50));
                    Ok(Output {
                        success: true,
                        stdout: r#"{"is_error":false,"result":"Added RSI(14) on pos:0.\nAnything else?","session_id":"s"}"#.into(),
                        stderr: String::new(),
                    })
                }),
                terminal: Box::new(|_| unreachable!()),
                launch: Box::new(|_| unreachable!()),
            });
            let glow = &window.dictation.glow;
            assert_eq!(glow.widget.parent(), window.trade.overlay.borrow().clone().map(|overlay| overlay.upcast()));
            window.dictated("add an RSI".into());
            window.dictated("now a MACD".into());
            let started = || log.entries().iter().any(|entry| entry.text.starts_with("Sent"));
            assert!(crate::ui::window::trade::tests::settle(&window, started));
            assert!(glow.showing() && glow.ticking(), "glowing while the agent works");
            let first = || log.entries().iter().any(|entry| entry.text.starts_with("Done"));
            assert!(crate::ui::window::trade::tests::settle(&window, first));
            let done = || log.entries().iter().filter(|entry| entry.text.starts_with("Done")).count() == 2;
            let through = Cell::new(true);
            let lasted = crate::ui::window::trade::tests::settle(&window, || {
                through.set(through.get() && (done() || glow.showing()));
                done()
            });
            assert!(lasted, "{:?}", log.entries());
            assert!(through.get(), "the glow went out between two queued turns");
            let out = || !glow.showing();
            let settle = crate::ui::window::trade::tests::settle;
            assert!(settle(&window, out) || settle(&window, out), "out once done");
            assert!(!glow.ticking());
            let lines: Vec<String> = log.entries().iter().map(|entry| entry.line()).collect();
            assert_eq!(
                lines,
                [
                    "Agent · Queued “now a MACD” after the one the agent is on",
                    "Claude Code · Sent “add an RSI”",
                    "Claude Code · Done · Added RSI(14) on pos:0.",
                    "Claude Code · Sent “now a MACD”",
                    "Claude Code · Done · Added RSI(14) on pos:0.",
                ]
            );
            assert!(window.trade.toasts.borrow().is_some(), "somewhere for the toasts, with no login");
            let asked = asked.lock().unwrap();
            assert!(asked[0].arguments.last().unwrap().ends_with("\n\nUse Sonnet."));
            assert!(asked[0].arguments.windows(2).any(|pair| pair == ["--model", "haiku"]));
            assert!(asked[1].arguments.windows(2).any(|pair| pair == ["--resume", "s"]));
        });
    }

    /// No Superwhisper listening: Ctrl+V outside a text box pastes what was
    /// copied in the window, at once, as it always has.
    #[test]
    fn without_superwhisper_a_paste_is_a_paste() {
        crate::ui::on_gtk(|| {
            let store = Rc::new(Store::memory().expect("a store"));
            let window = crate::ui::window::trade::tests::window(&store);
            let log = crate::ui::window::log::tests::private(&window);
            window.paste_or_dictation_from(Some(std::env::temp_dir().join("omacharts-no-superwhisper.sock")));
            assert!(log.entries().is_empty());
            assert!(window.dictation.dispatcher.get().is_none(), "nothing sent to an agent");
        });
    }

    #[test]
    fn what_becomes_of_a_dictation_is_said() {
        let sent = said_of(Event::Started { agent: "Claude Code".into(), said: "add an RSI".into() });
        assert_eq!(sent.line(), "Claude Code · Sent “add an RSI”");
        assert!(sent.confirms);

        let done = said_of(Event::Done {
            agent: "Claude Code".into(),
            said: "add an RSI".into(),
            reply: Some("Added RSI(14) on pos:0 ES 5m.\n\nAnything else?".into()),
        });
        assert_eq!(done.line(), "Claude Code · Done · Added RSI(14) on pos:0 ES 5m.");
        assert!(done.detail.unwrap().contains("Anything else?"));

        let opened = said_of(Event::Opened { agent: "Pi".into(), said: "add an RSI".into() });
        assert_eq!(opened.line(), "Pi · Opened in a terminal with “add an RSI”");
        assert!(opened.confirms);

        let failed = said_of(Event::Failed { said: "add an RSI".into(), why: "No default agent".into(), detail: None });
        assert_eq!(failed.level, Level::Error);
        assert!(!failed.confirms);
        assert_eq!(failed.detail.as_deref(), Some("“add an RSI”"));
    }

    #[test]
    fn a_long_dictation_is_cut_short_in_a_toast() {
        let long = "word ".repeat(40);
        let quote = quoted(&long);
        assert!(quote.chars().count() <= QUOTE);
        assert!(quote.ends_with('…'));
    }
}
