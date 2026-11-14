//! Dictation: what is said with Omacharts in front goes to Omarchy's default
//! agent, which does it with the `omacharts` command.
//!
//! How to dictate is Omarchy's — Right Alt held, speak, release — and
//! nothing here changes it. [`superwhisper`] is how the window tells a
//! dictation that landed in it from a paste; [`agent`] is how the agent is
//! run. This is what sits between: the words that go to the agent, the one
//! conversation successive dictations share, and the queue that keeps them
//! in order.
//!
//! The first dictation of a conversation goes with a fixed line saying where
//! the person is and what to use, and the extra context from Preferences ›
//! Agent after it. Nothing about the charts: the agent asks `omacharts` for
//! what it needs. Every dictation after that continues the same session, so
//! "add an RSI" and then "now make it the 15 minute" build on each other.
//! A new conversation starts when Omacharts starts, after [`IDLE`] without
//! a dictation, when the agent or the extra context changes, after a turn
//! fails, and when somebody says "start over".

pub mod agent;
pub mod superwhisper;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

use agent::{Agent, Invocation, Output};

/// The extra context, written in Preferences › Agent, put after the
/// fixed line at the start of every conversation.
pub const CONTEXT: &str = "agent_context";

/// How long a conversation waits for its next dictation. Long enough for a
/// session at the desk; after it, the charts have moved on and so does the
/// agent.
pub const IDLE: Duration = Duration::from_secs(30 * 60);

/// The words that open a conversation.
pub fn opening(said: &str, context: &str) -> String {
    let mut prompt = format!("The user is in Omacharts and said: \"{}\". Use the `omacharts` CLI to do it.", said.trim());
    let context = context.trim();
    if !context.is_empty() {
        prompt.push_str("\n\n");
        prompt.push_str(context);
    }
    prompt
}

/// The words for every dictation after the first.
pub fn following(said: &str) -> String {
    format!("The user said: \"{}\"", said.trim())
}

/// Is `said` somebody asking for a new conversation, and nothing else?
pub fn is_start_over(said: &str) -> bool {
    let plain: String = said
        .chars()
        .filter(|c| c.is_alphanumeric() || c.is_whitespace())
        .collect::<String>()
        .to_lowercase();
    let plain = plain.split_whitespace().collect::<Vec<_>>().join(" ");
    matches!(
        plain.as_str(),
        "start over" | "start again" | "new conversation" | "start a new conversation" | "lets start over"
    )
}

/// The conversation dictations are continuing, if one is going.
#[derive(Debug, Default)]
pub struct Conversation {
    going: Option<Going>,
}

#[derive(Debug)]
struct Going {
    agent: Agent,
    context: String,
    session: String,
    last: Instant,
}

impl Conversation {
    /// The prompt for `said`, and the session it continues: the opening
    /// words and none when there is no conversation to continue.
    pub fn prompt(&self, agent: Agent, context: &str, said: &str, now: Instant) -> (String, Option<String>) {
        match &self.going {
            Some(going)
                if going.agent == agent
                    && going.context == context.trim()
                    && now.saturating_duration_since(going.last) < IDLE =>
            {
                (following(said), Some(going.session.clone()))
            }
            _ => (opening(said, context), None),
        }
    }

    /// A turn finished, in `session` if the agent named one.
    pub fn finished(&mut self, agent: Agent, context: &str, session: Option<String>, now: Instant) {
        let session = session.or_else(|| self.going.as_ref().map(|going| going.session.clone()));
        self.going = session.map(|session| Going { agent, context: context.trim().to_string(), session, last: now });
    }

    /// Start the next dictation afresh.
    pub fn forget(&mut self) {
        self.going = None;
    }

    pub fn is_going(&self) -> bool {
        self.going.is_some()
    }
}

/// What became of a dictation, for the window to say.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    /// The agent has started on it.
    Started { agent: &'static str, said: String },
    /// The agent finished, with its last words if it had any.
    Done { agent: &'static str, said: String, reply: Option<String> },
    /// It could not be done.
    Failed { said: String, why: String, detail: Option<String> },
    /// A new conversation was asked for.
    StartedOver,
}

/// Runs a command to its end, or says why it could not.
pub type Runner = Box<dyn Fn(&Invocation) -> Result<Output, String> + Send>;

/// What the dispatcher needs of the world, so that a test can be the agent.
pub struct World {
    /// Omarchy's name for its default agent.
    pub default_agent: Box<dyn Fn() -> String + Send>,
    /// The command for one turn of `agent`.
    pub invocation: Box<dyn Fn(Agent, Vec<String>) -> Invocation + Send>,
    /// Runs it.
    pub run: Runner,
}

impl World {
    /// The real one: Omarchy's default agent, run in its own folder under
    /// the app's data, as the account Omarchy has selected for it, finding
    /// this very `omacharts` first on its PATH.
    pub fn omarchy() -> World {
        World {
            default_agent: Box::new(agent::omarchy_default),
            invocation: Box::new(|agent, arguments| {
                let directory = crate::store::data_dir().join("agent");
                let _ = std::fs::create_dir_all(&directory);
                let mut environment = Vec::new();
                if let Some(home) = agent::omarchy_account_home(agent) {
                    environment.push((agent.home_variable().to_string(), home));
                }
                if let Some(path) = path_with_this_omacharts() {
                    environment.push(("PATH".to_string(), path));
                }
                Invocation { program: agent.program().to_string(), arguments, directory, environment }
            }),
            run: Box::new(|invocation| agent::run(invocation, agent::LONGEST)),
        }
    }
}

/// PATH with this executable's folder first, when this is `omacharts`: the
/// agent then drives the same build, and the same store, as the window it
/// was sent from.
fn path_with_this_omacharts() -> Option<String> {
    let exe = std::env::current_exe().ok()?;
    if exe.file_name()? != "omacharts" {
        return None;
    }
    let folder = exe.parent()?.to_path_buf();
    let mut paths = vec![folder];
    if let Some(path) = std::env::var_os("PATH") {
        paths.extend(std::env::split_paths(&path));
    }
    std::env::join_paths(paths).ok()?.into_string().ok()
}

enum Job {
    Say { said: String, context: String },
}

/// The queue: dictations, one turn at a time, in the order they were said.
pub struct Dispatcher {
    jobs: mpsc::Sender<Job>,
    /// Dictations sent and not yet finished.
    pending: Arc<AtomicUsize>,
}

impl Dispatcher {
    /// Start the thread that runs the turns, telling `events` what became
    /// of each. The conversation lives as long as this does.
    pub fn start(world: World, events: impl Fn(Event) + Send + 'static) -> Dispatcher {
        let (jobs, queue) = mpsc::channel::<Job>();
        let pending = Arc::new(AtomicUsize::new(0));
        let left = pending.clone();
        std::thread::Builder::new()
            .name("dictation".into())
            .spawn(move || {
                let mut conversation = Conversation::default();
                while let Ok(job) = queue.recv() {
                    match job {
                        Job::Say { said, context } => turn(&world, &mut conversation, &events, said, &context),
                    }
                    left.fetch_sub(1, Ordering::SeqCst);
                }
            })
            .expect("the dictation thread starts");
        Dispatcher { jobs, pending }
    }

    /// Queue `said`, with the extra context as it is now. Says how many
    /// dictations are ahead of it.
    pub fn send(&self, said: String, context: String) -> usize {
        let ahead = self.pending.fetch_add(1, Ordering::SeqCst);
        if self.jobs.send(Job::Say { said, context }).is_err() {
            self.pending.fetch_sub(1, Ordering::SeqCst);
        }
        ahead
    }
}

/// One dictation, start to finish.
fn turn(world: &World, conversation: &mut Conversation, events: &dyn Fn(Event), said: String, context: &str) {
    if is_start_over(&said) {
        conversation.forget();
        events(Event::StartedOver);
        return;
    }
    let agent = match Agent::named(&(world.default_agent)()) {
        Ok(agent) => agent,
        Err(why) => return events(Event::Failed { said, why, detail: None }),
    };
    let (prompt, session) = conversation.prompt(agent, context, &said, Instant::now());
    events(Event::Started { agent: agent.label(), said: said.clone() });
    let invocation = (world.invocation)(agent, agent.arguments(&prompt, session.as_deref()));
    let output = match (world.run)(&invocation) {
        Ok(output) => output,
        Err(why) => {
            conversation.forget();
            return events(Event::Failed { said, why: format!("{} could not run: {why}", agent.label()), detail: None });
        }
    };
    let read = agent.read(&output.stdout);
    let failure = read.failure.clone().or_else(|| {
        (!output.success).then(|| last_line(&output.stderr).unwrap_or_else(|| "it stopped with an error".into()))
    });
    match failure {
        Some(why) => {
            conversation.forget();
            let detail = Some(output.stderr.trim().to_string()).filter(|s| !s.is_empty());
            events(Event::Failed { said, why: format!("{} could not do it: {why}", agent.label()), detail });
        }
        None => {
            conversation.finished(agent, context, read.session, Instant::now());
            events(Event::Done { agent: agent.label(), said, reply: read.reply });
        }
    }
}

fn last_line(text: &str) -> Option<String> {
    text.lines().map(str::trim).rfind(|line| !line.is_empty()).map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[test]
    fn the_opening_is_the_fixed_line_and_then_the_context() {
        assert_eq!(
            opening(" add an RSI ", ""),
            "The user is in Omacharts and said: \"add an RSI\". Use the `omacharts` CLI to do it."
        );
        assert_eq!(
            opening("add an RSI", "  Use Sonnet\n"),
            "The user is in Omacharts and said: \"add an RSI\". Use the `omacharts` CLI to do it.\n\nUse Sonnet"
        );
    }

    #[test]
    fn start_over_is_the_whole_of_what_was_said() {
        assert!(is_start_over("Start over."));
        assert!(is_start_over("new conversation!"));
        assert!(is_start_over("Let's start over."));
        assert!(!is_start_over("Start over with the gold chart."));
    }

    #[test]
    fn a_conversation_continues_until_it_is_idle_or_changes() {
        let now = Instant::now();
        let mut conversation = Conversation::default();
        let (first, session) = conversation.prompt(Agent::Claude, "ctx", "a", now);
        assert!(first.starts_with("The user is in Omacharts"));
        assert_eq!(session, None);
        conversation.finished(Agent::Claude, "ctx", Some("s1".into()), now);

        let (next, session) = conversation.prompt(Agent::Claude, " ctx ", "b", now + Duration::from_secs(60));
        assert_eq!(next, "The user said: \"b\"");
        assert_eq!(session.as_deref(), Some("s1"));

        // Idle too long, another agent, other context: each starts afresh.
        assert_eq!(conversation.prompt(Agent::Claude, "ctx", "c", now + IDLE).1, None);
        assert_eq!(conversation.prompt(Agent::Codex, "ctx", "c", now).1, None);
        assert_eq!(conversation.prompt(Agent::Claude, "Use Sonnet", "c", now).1, None);

        conversation.forget();
        assert_eq!(conversation.prompt(Agent::Claude, "ctx", "c", now).1, None);
    }

    /// A world whose agent is a closure: what it was asked, in order.
    fn fake(agent: &'static str, answer: impl Fn(&Invocation) -> Result<Output, String> + Send + 'static) -> (World, Arc<Mutex<Vec<Invocation>>>) {
        let asked = Arc::new(Mutex::new(Vec::new()));
        let log = asked.clone();
        let world = World {
            default_agent: Box::new(move || agent.to_string()),
            invocation: Box::new(|agent, arguments| Invocation {
                program: agent.program().into(),
                arguments,
                directory: std::env::temp_dir(),
                environment: vec![],
            }),
            run: Box::new(move |invocation| {
                log.lock().unwrap().push(invocation.clone());
                answer(invocation)
            }),
        };
        (world, asked)
    }

    fn claude_says(session: &str, result: &str) -> Output {
        Output {
            success: true,
            stdout: serde_json::json!({"type": "result", "is_error": false, "result": result, "session_id": session}).to_string(),
            stderr: String::new(),
        }
    }

    fn collect() -> (Arc<Mutex<Vec<Event>>>, impl Fn(Event) + Send + 'static) {
        let events = Arc::new(Mutex::new(Vec::new()));
        let sink = events.clone();
        (events, move |event| sink.lock().unwrap().push(event))
    }

    fn wait_for(events: &Arc<Mutex<Vec<Event>>>, count: usize) -> Vec<Event> {
        let started = Instant::now();
        while events.lock().unwrap().len() < count {
            assert!(started.elapsed() < Duration::from_secs(10), "waited for {count} events: {:?}", events.lock().unwrap());
            std::thread::sleep(Duration::from_millis(5));
        }
        events.lock().unwrap().clone()
    }

    #[test]
    fn dictations_run_in_order_in_one_session() {
        // Slow enough that the second and third arrive while the first runs.
        let (world, asked) = fake("claude", |_| {
            std::thread::sleep(Duration::from_millis(100));
            Ok(claude_says("s-1", "done"))
        });
        let (events, sink) = collect();
        let dispatcher = Dispatcher::start(world, sink);
        assert_eq!(dispatcher.send("do a".into(), "Use Sonnet".into()), 0);
        assert_eq!(dispatcher.send("do b".into(), "Use Sonnet".into()), 1);
        assert_eq!(dispatcher.send("do c".into(), "Use Sonnet".into()), 2);
        let events = wait_for(&events, 6);

        let said: Vec<_> = events
            .iter()
            .filter_map(|event| match event {
                Event::Done { said, .. } => Some(said.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(said, ["do a", "do b", "do c"]);

        let asked = asked.lock().unwrap();
        let prompts: Vec<&str> = asked.iter().map(|i| i.arguments.last().unwrap().as_str()).collect();
        assert_eq!(
            prompts,
            [
                "The user is in Omacharts and said: \"do a\". Use the `omacharts` CLI to do it.\n\nUse Sonnet",
                "The user said: \"do b\"",
                "The user said: \"do c\"",
            ]
        );
        assert!(!asked[0].arguments.contains(&"--resume".to_string()));
        assert!(asked[1].arguments.windows(2).any(|w| w == ["--resume", "s-1"]));
        assert!(asked[2].arguments.windows(2).any(|w| w == ["--resume", "s-1"]));
    }

    #[test]
    fn start_over_opens_a_new_conversation() {
        let (world, asked) = fake("claude", |_| Ok(claude_says("s-1", "ok")));
        let (events, sink) = collect();
        let dispatcher = Dispatcher::start(world, sink);
        dispatcher.send("do a".into(), String::new());
        dispatcher.send("Start over.".into(), String::new());
        dispatcher.send("do b".into(), String::new());
        let events = wait_for(&events, 5);
        assert!(events.contains(&Event::StartedOver));
        let asked = asked.lock().unwrap();
        assert_eq!(asked.len(), 2);
        assert!(asked[1].arguments.last().unwrap().starts_with("The user is in Omacharts"));
        assert!(!asked[1].arguments.contains(&"--resume".to_string()));
    }

    #[test]
    fn a_failed_turn_is_said_and_the_next_starts_afresh() {
        let calls = Arc::new(AtomicUsize::new(0));
        let count = calls.clone();
        let (world, asked) = fake("claude", move |_| match count.fetch_add(1, Ordering::SeqCst) {
            0 => Ok(claude_says("s-1", "ok")),
            1 => Ok(Output { success: false, stdout: String::new(), stderr: "API Error: overloaded\n".into() }),
            _ => Ok(claude_says("s-2", "ok")),
        });
        let (events, sink) = collect();
        let dispatcher = Dispatcher::start(world, sink);
        for said in ["a", "b", "c"] {
            dispatcher.send(said.into(), String::new());
        }
        let events = wait_for(&events, 6);
        assert!(events.contains(&Event::Failed {
            said: "b".into(),
            why: "Claude Code could not do it: API Error: overloaded".into(),
            detail: Some("API Error: overloaded".into()),
        }));
        let asked = asked.lock().unwrap();
        assert!(asked[1].arguments.contains(&"--resume".to_string()));
        assert!(!asked[2].arguments.contains(&"--resume".to_string()));
    }

    #[test]
    fn no_agent_is_said_and_nothing_is_run() {
        let (world, asked) = fake("", |_| unreachable!("nothing to run"));
        let (events, sink) = collect();
        let dispatcher = Dispatcher::start(world, sink);
        dispatcher.send("do a".into(), String::new());
        assert_eq!(
            wait_for(&events, 1),
            [Event::Failed { said: "do a".into(), why: agent::NO_AGENT.into(), detail: None }]
        );
        assert!(asked.lock().unwrap().is_empty());
    }

    #[test]
    fn an_agent_that_will_not_start_is_said() {
        let (world, _) = fake("claude", |_| Err("claude is not installed".into()));
        let (events, sink) = collect();
        let dispatcher = Dispatcher::start(world, sink);
        dispatcher.send("do a".into(), String::new());
        let events = wait_for(&events, 2);
        assert_eq!(
            events[1],
            Event::Failed { said: "do a".into(), why: "Claude Code could not run: claude is not installed".into(), detail: None }
        );
    }

    #[test]
    fn a_fake_agent_on_disk_is_run_for_real() {
        // The whole path through a process: a script stands in for claude,
        // writes down what it was given, and answers as claude does.
        let folder = std::env::temp_dir().join(format!("omacharts-fake-agent-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let script = folder.join("claude");
        let record = folder.join("asked");
        std::fs::write(
            &script,
            format!(
                "#!/bin/sh\nprintf '%s\\n' \"$@\" >> '{}'\nprintf '{{\"is_error\":false,\"result\":\"ok\",\"session_id\":\"real-1\"}}\\n'\n",
                record.display()
            ),
        )
        .unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        let program = script.display().to_string();
        let world = World {
            default_agent: Box::new(|| "claude".into()),
            invocation: Box::new(move |_, arguments| Invocation {
                program: program.clone(),
                arguments,
                directory: std::env::temp_dir(),
                environment: vec![],
            }),
            run: Box::new(|invocation| agent::run(invocation, Duration::from_secs(10))),
        };
        let (events, sink) = collect();
        let dispatcher = Dispatcher::start(world, sink);
        dispatcher.send("do a".into(), String::new());
        dispatcher.send("do b".into(), String::new());
        let events = wait_for(&events, 4);
        assert_eq!(events[3], Event::Done { agent: "Claude Code", said: "do b".into(), reply: Some("ok".into()) });
        let asked = std::fs::read_to_string(&record).unwrap();
        assert!(asked.contains("--resume\nreal-1\n"));
        let _ = std::fs::remove_dir_all(&folder);
    }
}
