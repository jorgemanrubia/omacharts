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
//! a dictation, when the agent, its model or the extra context changes,
//! after a turn fails, and when somebody says "start over". An agent that
//! dictation does not run itself is opened in a terminal by Omarchy instead,
//! and each dictation there is a conversation of its own.

pub mod agent;
pub mod superwhisper;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

use agent::{Agent, Detected, Invocation, Output};

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
    model: String,
    context: String,
    session: String,
    last: Instant,
}

impl Conversation {
    /// The prompt for `said`, and the session it continues: the opening
    /// words and none when there is no conversation to continue.
    pub fn prompt(&self, agent: Agent, model: &str, context: &str, said: &str, now: Instant) -> (String, Option<String>) {
        match &self.going {
            Some(going)
                if going.agent == agent
                    && going.model == model.trim()
                    && going.context == context.trim()
                    && now.saturating_duration_since(going.last) < IDLE =>
            {
                (following(said), Some(going.session.clone()))
            }
            _ => (opening(said, context), None),
        }
    }

    /// A turn finished, in `session` if the agent named one.
    pub fn finished(&mut self, agent: Agent, model: &str, context: &str, session: Option<String>, now: Instant) {
        let session = session.or_else(|| self.going.as_ref().map(|going| going.session.clone()));
        self.going = session.map(|session| Going {
            agent,
            model: model.trim().to_string(),
            context: context.trim().to_string(),
            session,
            last: now,
        });
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
    Started { agent: String, said: String },
    /// The agent finished, with its last words if it had any.
    Done { agent: String, said: String, reply: Option<String> },
    /// Omarchy opened an agent dictation does not run itself, in a
    /// terminal, with it.
    Opened { agent: String, said: String },
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
    /// The command that opens Omarchy's default agent in a terminal with a
    /// prompt, for an agent not run here.
    pub terminal: Box<dyn Fn(String) -> Invocation + Send>,
    /// Starts it, and leaves it running.
    pub launch: Launcher,
}

/// Starts a command and leaves it running, or says why it could not.
pub type Launcher = Box<dyn Fn(&Invocation) -> Result<(), String> + Send>;

/// What a dictation goes with, read from the settings when it is said.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Settings {
    /// The extra context from Preferences › Agent.
    pub context: String,
    /// The model chosen for each agent; one not here, or empty, is the
    /// agent's own default.
    pub models: Vec<(Agent, String)>,
}

impl Settings {
    /// What `store` says now.
    pub fn of(store: &crate::store::Store) -> Settings {
        Settings {
            context: store.setting(CONTEXT).unwrap_or_default(),
            models: Agent::ALL
                .into_iter()
                .filter_map(|agent| Some((agent, store.setting(&agent::model_setting(agent))?)))
                .collect(),
        }
    }

    /// The model chosen for `agent`, empty for its own.
    pub fn model(&self, agent: Agent) -> &str {
        self.models.iter().find(|(known, _)| *known == agent).map(|(_, model)| model.trim()).unwrap_or("")
    }
}

impl World {
    /// The real one: Omarchy's default agent, run in its own folder under
    /// the app's data, as the account Omarchy has selected for it, finding
    /// this very `omacharts` first on its PATH.
    pub fn omarchy() -> World {
        World {
            default_agent: Box::new(agent::omarchy_default),
            invocation: Box::new(|agent, arguments| {
                let directory = agent_folder();
                let mut environment = Vec::new();
                if let Some(variable) = agent.home_variable()
                    && let Some(home) = agent::omarchy_account_home(agent)
                {
                    environment.push((variable.to_string(), home));
                }
                if let Some(path) = path_with_this_omacharts() {
                    environment.push(("PATH".to_string(), path));
                }
                Invocation { program: agent.program().to_string(), arguments, directory, environment }
            }),
            run: Box::new(|invocation| agent::run(invocation, agent::LONGEST)),
            terminal: Box::new(|prompt| Invocation {
                program: "omarchy-agent-prompt".to_string(),
                arguments: vec![prompt],
                directory: agent_folder(),
                environment: path_with_this_omacharts().map(|path| ("PATH".to_string(), path)).into_iter().collect(),
            }),
            launch: Box::new(|invocation| agent::launch(invocation, &agent_folder().join("terminal.log"))),
        }
    }
}

/// The folder the agent is run in, under the app's data.
fn agent_folder() -> std::path::PathBuf {
    let directory = crate::store::data_dir().join("agent");
    let _ = std::fs::create_dir_all(&directory);
    directory
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
    Say { said: String, settings: Settings },
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
                        Job::Say { said, settings } => turn(&world, &mut conversation, &events, said, &settings),
                    }
                    left.fetch_sub(1, Ordering::SeqCst);
                }
            })
            .expect("the dictation thread starts");
        Dispatcher { jobs, pending }
    }

    /// Queue `said`, with the settings as they are now. Says how many
    /// dictations are ahead of it.
    pub fn send(&self, said: String, settings: Settings) -> usize {
        let ahead = self.pending.fetch_add(1, Ordering::SeqCst);
        if self.jobs.send(Job::Say { said, settings }).is_err() {
            self.pending.fetch_sub(1, Ordering::SeqCst);
        }
        ahead
    }
}

/// One dictation, start to finish. Which agent is Omarchy's default is
/// asked afresh each time, so a change there needs no restart here.
fn turn(world: &World, conversation: &mut Conversation, events: &dyn Fn(Event), said: String, settings: &Settings) {
    if is_start_over(&said) {
        conversation.forget();
        events(Event::StartedOver);
        return;
    }
    let context = settings.context.as_str();
    let agent = match Detected::of(&(world.default_agent)()) {
        Detected::Known(agent) => agent,
        Detected::None => return events(Event::Failed { said, why: agent::NO_AGENT.into(), detail: None }),
        Detected::Other { name, .. } => {
            conversation.forget();
            let invocation = (world.terminal)(opening(&said, context));
            return match (world.launch)(&invocation) {
                Ok(()) => events(Event::Opened { agent: name, said }),
                Err(why) => events(Event::Failed { said, why: format!("Omarchy could not open {name}: {why}"), detail: None }),
            };
        }
    };
    let model = settings.model(agent);
    let (prompt, session) = conversation.prompt(agent, model, context, &said, Instant::now());
    let session = session.or_else(|| agent.new_session());
    events(Event::Started { agent: agent.label().into(), said: said.clone() });
    let arguments = agent.arguments(&prompt, session.as_deref(), Some(model));
    let invocation = (world.invocation)(agent, arguments);
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
            // The session the agent named, or the one it was given; none
            // for an agent that starts afresh each turn.
            let session = read.session.or(session).filter(|_| agent.resumes());
            match session {
                Some(session) => conversation.finished(agent, model, context, Some(session), Instant::now()),
                None => conversation.forget(),
            }
            events(Event::Done { agent: agent.label().into(), said, reply: read.reply });
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
        let (first, session) = conversation.prompt(Agent::Claude, "", "ctx", "a", now);
        assert!(first.starts_with("The user is in Omacharts"));
        assert_eq!(session, None);
        conversation.finished(Agent::Claude, "", "ctx", Some("s1".into()), now);

        let (next, session) = conversation.prompt(Agent::Claude, " ", " ctx ", "b", now + Duration::from_secs(60));
        assert_eq!(next, "The user said: \"b\"");
        assert_eq!(session.as_deref(), Some("s1"));

        // Idle too long, another agent, another model, other context: each
        // starts afresh.
        assert_eq!(conversation.prompt(Agent::Claude, "", "ctx", "c", now + IDLE).1, None);
        assert_eq!(conversation.prompt(Agent::Codex, "", "ctx", "c", now).1, None);
        assert_eq!(conversation.prompt(Agent::Claude, "opus", "ctx", "c", now).1, None);
        assert_eq!(conversation.prompt(Agent::Claude, "", "Use Sonnet", "c", now).1, None);

        conversation.forget();
        assert_eq!(conversation.prompt(Agent::Claude, "", "ctx", "c", now).1, None);
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
            terminal: Box::new(|_| unreachable!("a known agent is run, not opened")),
            launch: Box::new(|_| unreachable!("a known agent is run, not opened")),
        };
        (world, asked)
    }

    fn with(context: &str) -> Settings {
        Settings { context: context.into(), models: vec![] }
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
        assert_eq!(dispatcher.send("do a".into(), with("Use Sonnet")), 0);
        assert_eq!(dispatcher.send("do b".into(), with("Use Sonnet")), 1);
        assert_eq!(dispatcher.send("do c".into(), with("Use Sonnet")), 2);
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
        dispatcher.send("do a".into(), Settings::default());
        dispatcher.send("Start over.".into(), Settings::default());
        dispatcher.send("do b".into(), Settings::default());
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
            dispatcher.send(said.into(), Settings::default());
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
        dispatcher.send("do a".into(), Settings::default());
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
        dispatcher.send("do a".into(), Settings::default());
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
            terminal: Box::new(|_| unreachable!()),
            launch: Box::new(|_| unreachable!()),
        };
        let (events, sink) = collect();
        let dispatcher = Dispatcher::start(world, sink);
        dispatcher.send("do a".into(), Settings::default());
        dispatcher.send("do b".into(), Settings::default());
        let events = wait_for(&events, 4);
        assert_eq!(events[3], Event::Done { agent: "Claude Code".into(), said: "do b".into(), reply: Some("ok".into()) });
        let asked = std::fs::read_to_string(&record).unwrap();
        assert!(asked.contains("--resume\nreal-1\n"));
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn the_model_chosen_goes_with_each_turn_and_a_new_one_starts_afresh() {
        let (world, asked) = fake("claude", |_| Ok(claude_says("s-1", "ok")));
        let (events, sink) = collect();
        let dispatcher = Dispatcher::start(world, sink);
        let opus = Settings { context: String::new(), models: vec![(Agent::Claude, "opus".into()), (Agent::Codex, "gpt-5.5".into())] };
        dispatcher.send("do a".into(), opus.clone());
        dispatcher.send("do b".into(), opus);
        dispatcher.send("do c".into(), Settings::default());
        wait_for(&events, 6);
        let asked = asked.lock().unwrap();
        assert!(asked[0].arguments.windows(2).any(|w| w == ["--model", "opus"]));
        assert!(!asked[0].arguments.contains(&"--resume".to_string()));
        assert!(asked[1].arguments.windows(2).any(|w| w == ["--model", "opus"]));
        assert!(asked[1].arguments.windows(2).any(|w| w == ["--resume", "s-1"]));
        // Back to Default: no model, and a new conversation.
        assert!(!asked[2].arguments.contains(&"--model".to_string()));
        assert!(!asked[2].arguments.contains(&"--resume".to_string()));
    }

    #[test]
    fn copilot_keeps_the_session_it_was_given() {
        let (world, asked) = fake("copilot", |_| Ok(Output { success: true, stdout: "Done.\n".into(), stderr: String::new() }));
        let (events, sink) = collect();
        let dispatcher = Dispatcher::start(world, sink);
        dispatcher.send("do a".into(), Settings::default());
        dispatcher.send("do b".into(), Settings::default());
        let events = wait_for(&events, 4);
        assert_eq!(events[1], Event::Done { agent: "GitHub Copilot".into(), said: "do a".into(), reply: Some("Done.".into()) });
        let asked = asked.lock().unwrap();
        let session = |i: usize| {
            let arguments = &asked[i].arguments;
            arguments[arguments.iter().position(|a| a == "--session-id").unwrap() + 1].clone()
        };
        assert_eq!(session(0), session(1));
        assert_eq!(asked[1].arguments[1], "The user said: \"do b\"");
    }

    #[test]
    fn opencode_starts_every_dictation_afresh() {
        let (world, asked) = fake("opencode", |_| Ok(Output { success: true, stdout: "ok".into(), stderr: String::new() }));
        let (events, sink) = collect();
        let dispatcher = Dispatcher::start(world, sink);
        dispatcher.send("do a".into(), Settings::default());
        dispatcher.send("do b".into(), Settings::default());
        wait_for(&events, 4);
        let asked = asked.lock().unwrap();
        assert_eq!(asked[0].program, "opencode");
        assert!(asked[1].arguments.last().unwrap().starts_with("The user is in Omacharts and said: \"do b\""));
    }

    /// A world whose default agent is one dictation does not run itself:
    /// what Omarchy was asked to open.
    fn other(agent: &'static str, launched: Result<(), String>) -> (World, Arc<Mutex<Vec<Invocation>>>) {
        let opened = Arc::new(Mutex::new(Vec::new()));
        let log = opened.clone();
        let world = World {
            default_agent: Box::new(move || agent.to_string()),
            invocation: Box::new(|_, _| unreachable!("nothing is run headless")),
            run: Box::new(|_| unreachable!("nothing is run headless")),
            terminal: World::omarchy().terminal,
            launch: Box::new(move |invocation| {
                log.lock().unwrap().push(invocation.clone());
                launched.clone()
            }),
        };
        (world, opened)
    }

    #[test]
    fn another_agent_is_opened_by_omarchy_with_each_dictation() {
        let (world, opened) = other("pi", Ok(()));
        let (events, sink) = collect();
        let dispatcher = Dispatcher::start(world, sink);
        let settings = Settings { context: "Keep it brief.".into(), models: vec![(Agent::Claude, "opus".into())] };
        dispatcher.send("add an RSI".into(), settings.clone());
        dispatcher.send("now MACD".into(), settings);
        let events = wait_for(&events, 2);
        assert_eq!(events[0], Event::Opened { agent: "Pi".into(), said: "add an RSI".into() });
        let opened = opened.lock().unwrap();
        assert_eq!(opened[0].program, "omarchy-agent-prompt");
        assert_eq!(
            opened[0].arguments,
            ["The user is in Omacharts and said: \"add an RSI\". Use the `omacharts` CLI to do it.\n\nKeep it brief."]
        );
        // A new prompt each time, with no model: the agent's own.
        assert!(opened[1].arguments[0].starts_with("The user is in Omacharts and said: \"now MACD\""));
        assert!(!opened[1].arguments[0].contains("opus"));
    }

    #[test]
    fn another_agent_omarchy_cannot_open_is_said() {
        let (world, _) = other("crush", Err("crush is not installed".into()));
        let (events, sink) = collect();
        let dispatcher = Dispatcher::start(world, sink);
        dispatcher.send("do a".into(), Settings::default());
        assert_eq!(
            wait_for(&events, 1),
            [Event::Failed { said: "do a".into(), why: "Omarchy could not open Crush: crush is not installed".into(), detail: None }]
        );
    }
}
