//! Omarchy's default agent, run for a dictation.
//!
//! Omarchy keeps one coding agent as the default (`omarchy default agent`,
//! stored in `~/.config/omarchy/defaults/agent`), and `omarchy agent` opens it
//! in a terminal. A dictation would rather have a turn run in the
//! background, its answer read back, and the next turn continue the same
//! conversation. The agents in [`Agent`] are run that way, each in its own
//! headless mode, with the model chosen for it in Preferences › Agent:
//!
//! | Agent | One turn | Model | Next turn |
//! |---|---|---|---|
//! | Claude Code | `claude -p --output-format json` | `--model` | `--resume <session_id>` |
//! | Codex | `codex exec --json` | `-m` | `codex exec resume <thread_id>` |
//! | Cursor CLI | `cursor-agent -p --output-format json` | `--model` | `--resume <session_id>` |
//! | GitHub Copilot | `copilot -p --silent` | `--model` | `--session-id <uuid>`, chosen here |
//! | OpenCode | `opencode run` | `-m provider/model` | starts afresh |
//! | Antigravity | `agy --print` | `--model` | starts afresh |
//!
//! Every flag is the agent's own, from its `--help`. OpenCode and Antigravity
//! name their session only in output whose shape their help does not give,
//! so each of their turns is a new conversation rather than a guess.
//!
//! Only a dictation runs an agent's command: [`run`] for a turn, [`launch`]
//! for Omarchy's terminal. The models Preferences offers are kept here, and
//! the one an agent uses by default is read from its settings file.
//!
//! Any other agent Omarchy offers is not run here. Its dictations go to
//! `omarchy-agent-prompt`, which opens it in a terminal, with Omarchy's own
//! flags, its own default model and a new conversation each time.
//!
//! The account Omarchy has selected for the agent is used, as `omarchy agent`
//! uses it.

use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::Value;

/// The agents dictation runs a turn of itself.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Agent {
    Claude,
    Codex,
    Cursor,
    Copilot,
    OpenCode,
    Antigravity,
}

/// What Omarchy's default agent is, to dictation.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Detected {
    /// None chosen, or this is not Omarchy.
    None,
    /// One dictation runs itself.
    Known(Agent),
    /// One Omarchy opens in a terminal: its id and its name.
    Other { id: String, name: String },
}

impl Detected {
    /// Omarchy's id for its default agent, as `omarchy-default-agent`
    /// prints it.
    pub fn of(id: &str) -> Detected {
        let id = id.trim();
        if id.is_empty() {
            return Detected::None;
        }
        match Agent::named(id) {
            Some(agent) => Detected::Known(agent),
            None => Detected::Other { id: id.to_string(), name: omarchy_name(id).unwrap_or(id).to_string() },
        }
    }

    /// Its name, if there is one.
    pub fn name(&self) -> Option<String> {
        match self {
            Detected::None => None,
            Detected::Known(agent) => Some(agent.label().to_string()),
            Detected::Other { name, .. } => Some(name.clone()),
        }
    }
}

/// The names Omarchy gives the agents it offers, from
/// `omarchy-default-agent`.
fn omarchy_name(id: &str) -> Option<&'static str> {
    Some(match id {
        "pi" => "Pi",
        "omp" => "Oh My Pi",
        "ori" => "Ori",
        "crush" => "Crush",
        "grok" => "Grok",
        "openclaw" => "OpenClaw",
        "hermes" => "Hermes",
        "muse" => "Muse Code",
        "kimi" => "Kimi",
        "qwen" => "Qwen Code",
        "dsh" => "DeepSeek Harness",
        "mcode" => "MiniMax Code",
        "kiro-cli" => "Kiro CLI",
        "freebuff" => "Freebuff",
        "kilo" => "Kilo",
        "devin" => "Devin CLI",
        "amp" => "Amp",
        "commandcode" => "Command Code",
        _ => return None,
    })
}

impl Agent {
    pub const ALL: [Agent; 6] =
        [Agent::Claude, Agent::Codex, Agent::Cursor, Agent::Copilot, Agent::OpenCode, Agent::Antigravity];

    /// The agent Omarchy calls `id`, when dictation runs it itself.
    pub fn named(id: &str) -> Option<Agent> {
        match id.trim() {
            "claude" => Some(Agent::Claude),
            "codex" => Some(Agent::Codex),
            "cursor-agent" => Some(Agent::Cursor),
            "copilot" => Some(Agent::Copilot),
            "opencode" => Some(Agent::OpenCode),
            "agy" | "gemini" | "antigravity" => Some(Agent::Antigravity),
            _ => None,
        }
    }

    /// Omarchy's id for it, which is also where its model is kept.
    pub fn id(self) -> &'static str {
        match self {
            Agent::Claude => "claude",
            Agent::Codex => "codex",
            Agent::Cursor => "cursor-agent",
            Agent::Copilot => "copilot",
            Agent::OpenCode => "opencode",
            Agent::Antigravity => "agy",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Agent::Claude => "Claude Code",
            Agent::Codex => "Codex",
            Agent::Cursor => "Cursor CLI",
            Agent::Copilot => "GitHub Copilot",
            Agent::OpenCode => "OpenCode",
            Agent::Antigravity => "Antigravity",
        }
    }

    /// Its command.
    pub fn program(self) -> &'static str {
        self.id()
    }

    /// Where it keeps an account other than its first, for the agents
    /// Omarchy keeps accounts of.
    pub fn home_variable(self) -> Option<&'static str> {
        match self {
            Agent::Claude => Some("CLAUDE_CONFIG_DIR"),
            Agent::Codex => Some("CODEX_HOME"),
            _ => None,
        }
    }

    /// Does a turn continue the one before it? The others start afresh.
    pub fn resumes(self) -> bool {
        !matches!(self, Agent::OpenCode | Agent::Antigravity)
    }

    /// The session to start a conversation with, for an agent that takes
    /// one from us rather than naming its own.
    pub fn new_session(self) -> Option<String> {
        (self == Agent::Copilot).then(uuid)
    }

    /// The models Preferences offers after the agent's own, by name and by
    /// what the agent takes. Kept here rather than asked of the agent: its
    /// command is run for a dictation and for nothing else. The ids are the
    /// ones each agent's own help, docs or model list give.
    pub fn models(self) -> &'static [(&'static str, &'static str)] {
        match self {
            // The aliases `claude --help` and its docs name, each the latest
            // of its kind.
            Agent::Claude => &[("Sonnet", "sonnet"), ("Opus", "opus"), ("Haiku", "haiku")],
            Agent::Codex => &[
                ("GPT-6 Astra", "gpt-6-astra"),
                ("GPT-5.6 Sol", "gpt-5.6-sol"),
                ("GPT-5.6 Terra", "gpt-5.6-terra"),
                ("GPT-5.6 Luna", "gpt-5.6-luna"),
                ("GPT-5.5", "gpt-5.5"),
            ],
            Agent::Cursor => &[
                ("GPT-5", "gpt-5"),
                ("Claude Sonnet 4 Thinking", "sonnet-4-thinking"),
                ("Claude Opus 4.8", "claude-opus-4-8"),
            ],
            // `auto` lets Copilot choose.
            Agent::Copilot => &[
                ("Auto", "auto"),
                ("GPT-6 Astra", "gpt-6-astra"),
                ("GPT-5.6 Sol", "gpt-5.6-sol"),
                ("GPT-5.3 Codex", "gpt-5.3-codex"),
                ("Claude Haiku 4.5", "claude-haiku-4.5"),
            ],
            // OpenCode's own models, `provider/model`, which need no account
            // of another provider's.
            Agent::OpenCode => &[
                ("Big Pickle", "opencode/big-pickle"),
                ("GLM 5.3 Flash", "opencode/glm-5.3-flash-free"),
                ("DeepSeek V4.1 Flash", "opencode/deepseek-v4.1-flash-free"),
                ("MiniMax M3.1 Flash", "opencode/minimax-m3.1-flash-free"),
            ],
            Agent::Antigravity => &[
                ("Gemini 3.8 Flash (high)", "gemini-3.8-flash-high"),
                ("Gemini 3.1 Pro (preview)", "gemini-3.1-pro-preview"),
                ("Gemini 3.5 Flash Lite", "gemini-3.5-flash-lite"),
            ],
        }
    }

    /// The arguments for one turn of `prompt`, continuing `session` if
    /// there is one, with `model` if one was chosen.
    pub fn arguments(self, prompt: &str, session: Option<&str>, model: Option<&str>) -> Vec<String> {
        let mut arguments: Vec<String> = Vec::new();
        let mut push = |items: &[&str]| arguments.extend(items.iter().map(|item| item.to_string()));
        let model = model.map(str::trim).filter(|model| !model.is_empty());
        match self {
            // Every agent runs with the permissions `omarchy-agent` gives it,
            // in the form its one headless turn takes them; nothing more.
            Agent::Claude => {
                push(&["-p", "--output-format", "json", "--permission-mode", "auto"]);
                if let Some(session) = session {
                    push(&["--resume", session]);
                }
                if let Some(model) = model {
                    push(&["--model", model]);
                }
                push(&["--", prompt]);
            }
            // `--approve-for-me` belongs to `exec`, before `resume`, which
            // does not take it.
            Agent::Codex => {
                push(&["exec", "--approve-for-me"]);
                if session.is_some() {
                    push(&["resume"]);
                }
                push(&["--json", "--skip-git-repo-check"]);
                if let Some(model) = model {
                    push(&["-m", model]);
                }
                if let Some(session) = session {
                    push(&[session]);
                }
                push(&["--", prompt]);
            }
            // The agent subcommand is named, as `omarchy-agent` names it.
            Agent::Cursor => {
                push(&["-p", "--output-format", "json", "--yolo", "--trust"]);
                if let Some(session) = session {
                    push(&["--resume", session]);
                }
                if let Some(model) = model {
                    push(&["--model", model]);
                }
                push(&["agent", "--", prompt]);
            }
            // Its answer alone, and no question form nobody would answer.
            Agent::Copilot => {
                push(&["--prompt", prompt, "--silent", "--allow-all", "--no-ask-user"]);
                if let Some(session) = session {
                    push(&["--session-id", session]);
                }
                if let Some(model) = model {
                    push(&["--model", model]);
                }
            }
            // The session kept out of the list of the person's own.
            Agent::OpenCode => {
                push(&["run", "--auto", "--archived"]);
                if let Some(model) = model {
                    push(&["-m", model]);
                }
                push(&[prompt]);
            }
            Agent::Antigravity => {
                push(&["--dangerously-skip-permissions"]);
                if let Some(model) = model {
                    push(&["--model", model]);
                }
                push(&["--print", prompt]);
            }
        }
        arguments
    }

    /// What a turn printed, read.
    pub fn read(self, stdout: &str) -> Turn {
        match self {
            // Cursor's `--output-format json` is Claude's one object.
            Agent::Claude | Agent::Cursor => read_claude(stdout),
            Agent::Codex => read_codex(stdout),
            // Their answer, as text.
            Agent::Copilot | Agent::OpenCode | Agent::Antigravity => Turn {
                session: None,
                reply: Some(stdout.trim().to_string()).filter(|said| !said.is_empty()),
                failure: None,
            },
        }
    }
}

/// A new random UUID, version 4.
fn uuid() -> String {
    use std::io::Read;
    let mut bytes = [0u8; 16];
    let read = std::fs::File::open("/dev/urandom").and_then(|mut random| random.read_exact(&mut bytes));
    if read.is_err() {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_nanos())
            .unwrap_or_default();
        bytes = (nanos ^ ((std::process::id() as u128) << 64)).to_le_bytes();
    }
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let hex: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
    format!("{}-{}-{}-{}-{}", &hex[..8], &hex[8..12], &hex[12..16], &hex[16..20], &hex[20..])
}

/// Said when Omarchy has no default agent.
pub const NO_AGENT: &str = "No default agent: choose one with `omarchy default agent`";

/// What one turn came to.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Turn {
    /// The session to continue next time.
    pub session: Option<String>,
    /// The agent's last words.
    pub reply: Option<String>,
    /// Why it failed, when it says it did.
    pub failure: Option<String>,
}

/// One object: `session_id`, `result`, and `is_error` when it went wrong.
fn read_claude(stdout: &str) -> Turn {
    let Some(value) = stdout.lines().rev().find_map(|line| serde_json::from_str::<Value>(line.trim()).ok()) else {
        return Turn::default();
    };
    let text = |key: &str| value.get(key).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty());
    let session = text("session_id").map(str::to_string);
    let result = text("result").map(str::to_string);
    let failed = value.get("is_error").and_then(Value::as_bool).unwrap_or(false);
    match failed {
        true => Turn {
            session,
            reply: None,
            failure: Some(result.or_else(|| text("subtype").map(str::to_string)).unwrap_or_else(|| "it failed".into())),
        },
        false => Turn { session, reply: result, failure: None },
    }
}

/// A line of JSON per event: `thread.started` names the session, each
/// `item.completed` of an `agent_message` is something it said, and
/// `turn.failed` or `error` is the turn going wrong.
fn read_codex(stdout: &str) -> Turn {
    let mut turn = Turn::default();
    for line in stdout.lines() {
        let Ok(event) = serde_json::from_str::<Value>(line.trim()) else { continue };
        let text = |value: Option<&Value>| value.and_then(Value::as_str).map(str::trim).map(str::to_string);
        match event.get("type").and_then(Value::as_str) {
            Some("thread.started") => turn.session = text(event.get("thread_id")).or(turn.session),
            Some("item.completed") => {
                let item = event.get("item");
                if item.and_then(|item| item.get("type")).and_then(Value::as_str) == Some("agent_message")
                    && let Some(said) = text(item.and_then(|item| item.get("text"))).filter(|s| !s.is_empty())
                {
                    turn.reply = Some(said);
                }
            }
            Some("turn.failed") => {
                turn.failure = text(event.get("error").and_then(|error| error.get("message")))
                    .or(Some("the turn failed".into()));
            }
            Some("error") => turn.failure = text(event.get("message")).or(Some("it failed".into())),
            _ => {}
        }
    }
    turn
}

/// A command to run, whole: what, with what, where, and with which
/// variables set besides the window's own.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Invocation {
    pub program: String,
    pub arguments: Vec<String>,
    pub directory: PathBuf,
    pub environment: Vec<(String, String)>,
}

/// What a finished command left.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Output {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
}

/// The longest a turn may take. An agent that is still going after this is
/// stuck, and the dictations behind it should not wait for ever.
pub const LONGEST: Duration = Duration::from_secs(10 * 60);

/// Run `invocation` to the end, or until it has taken `longest`.
pub fn run(invocation: &Invocation, longest: Duration) -> Result<Output, String> {
    spawned(invocation);
    let mut command = Command::new(&invocation.program);
    command
        .args(&invocation.arguments)
        .current_dir(&invocation.directory)
        .envs(invocation.environment.iter().map(|(k, v)| (k, v)))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().map_err(|error| match error.kind() {
        std::io::ErrorKind::NotFound => format!("{} is not installed", invocation.program),
        _ => format!("could not start {}: {error}", invocation.program),
    })?;
    // Read both pipes while it runs, or a chatty agent fills one and waits
    // on it for ever.
    let drain = |pipe: Option<Box<dyn std::io::Read + Send>>| {
        std::thread::spawn(move || {
            let mut text = String::new();
            if let Some(mut pipe) = pipe {
                let _ = pipe.read_to_string(&mut text);
            }
            text
        })
    };
    let stdout = drain(child.stdout.take().map(|pipe| Box::new(pipe) as Box<dyn std::io::Read + Send>));
    let stderr = drain(child.stderr.take().map(|pipe| Box::new(pipe) as Box<dyn std::io::Read + Send>));
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() >= longest => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("it was still going after {} minutes, and was stopped", longest.as_secs() / 60));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(100)),
            Err(error) => return Err(error.to_string()),
        }
    };
    Ok(Output {
        success: status.success(),
        stdout: stdout.join().unwrap_or_default(),
        stderr: stderr.join().unwrap_or_default(),
    })
}

/// Omarchy's default agent, by its name: `claude`, `codex`, … Empty when
/// none has been chosen, or this is not Omarchy.
pub fn omarchy_default() -> String {
    Command::new("omarchy-default-agent")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .unwrap_or_default()
}

/// The config home of the account Omarchy has selected for `agent`, or
/// nothing for its first account, as `omarchy agent` asks for it.
pub fn omarchy_account_home(agent: Agent) -> Option<String> {
    Command::new("omarchy-agent-account-home")
        .arg(agent.program())
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .filter(|home| !home.is_empty())
}


/// Every program [`run`] and [`launch`] started, for the tests to check
/// that only a dictation starts an agent.
#[cfg(test)]
pub static SPAWNED: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

/// Note that `invocation` is about to start. These two are the only places
/// an agent's command is started, and only a dictation calls them.
fn spawned(_invocation: &Invocation) {
    #[cfg(test)]
    SPAWNED.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).push(_invocation.program.clone());
}

/// Open the agent in a terminal with `invocation`, as Omarchy does, and
/// leave it there: an error only when the launch itself fails at once.
/// What it says goes to `log`, rather than a pipe the terminal it opens
/// could hold open.
pub fn launch(invocation: &Invocation, log: &std::path::Path) -> Result<(), String> {
    spawned(invocation);
    let stderr = std::fs::File::create(log).map(Stdio::from).unwrap_or_else(|_| Stdio::null());
    let mut child = Command::new(&invocation.program)
        .args(&invocation.arguments)
        .current_dir(&invocation.directory)
        .envs(invocation.environment.iter().map(|(k, v)| (k, v)))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(stderr)
        .spawn()
        .map_err(|error| match error.kind() {
            std::io::ErrorKind::NotFound => format!("{} is not installed", invocation.program),
            _ => format!("could not start {}: {error}", invocation.program),
        })?;
    // Omarchy refuses at once when the agent is missing; a terminal that
    // opened is left to run.
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(5) {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(()),
            Ok(Some(_)) => {
                let said = std::fs::read_to_string(log).unwrap_or_default();
                let why = said.lines().map(str::trim).rfind(|line| !line.is_empty());
                return Err(why.unwrap_or("it stopped with an error").to_string());
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(error) => return Err(error.to_string()),
        }
    }
    // Reap it whenever it ends, so it does not linger as a zombie.
    std::thread::spawn(move || child.wait());
    Ok(())
}

/// The setting that keeps the model chosen for `agent`; empty or unset is
/// the agent's own default.
pub fn model_setting(agent: Agent) -> String {
    format!("{MODEL_PREFIX}{}", agent.id())
}

/// What every agent's model setting starts with.
pub const MODEL_PREFIX: &str = "agent_model.";

/// What Preferences offers for an agent: the model its own settings name,
/// when they say, and the models to choose instead.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Offered {
    /// The model the agent uses when it is given none, as its own settings
    /// file names it; none when it names none or cannot be read.
    pub default: Option<String>,
    /// By name and by what the agent takes.
    pub models: Vec<(String, String)>,
}

/// What Preferences offers for `agent`, found out without running it: its
/// settings file read, and the list kept here.
pub fn offered(agent: Agent) -> Offered {
    let models = agent.models().iter().map(|(name, id)| (name.to_string(), id.to_string())).collect();
    Offered { default: default_model(agent), models }
}

/// The folder an agent keeps the account Omarchy has selected in: the
/// account home Omarchy gives, the agent's own variable, or its usual one.
fn account_folder(agent: Agent, usual: &str) -> PathBuf {
    agent
        .home_variable()
        .and_then(|variable| omarchy_account_home(agent).or_else(|| std::env::var(variable).ok()))
        .filter(|home| !home.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(usual))
}

/// The model `agent` uses when given none, read from its own settings file
/// where that is one small file with the model in a key of its own.
pub fn default_model(agent: Agent) -> Option<String> {
    let read = |path: PathBuf| std::fs::read_to_string(path).ok();
    match agent {
        Agent::Claude => json_model(&read(account_folder(agent, ".claude").join("settings.json"))?),
        Agent::Codex => toml_model(&read(account_folder(agent, ".codex").join("config.toml"))?),
        Agent::OpenCode => {
            let config = std::env::var("XDG_CONFIG_HOME")
                .ok()
                .filter(|config| !config.is_empty())
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".config"));
            json_model(&read(config.join("opencode").join("opencode.json"))?)
        }
        Agent::Cursor | Agent::Copilot | Agent::Antigravity => None,
    }
}

/// `"model"` at the top of a JSON settings file.
pub fn json_model(text: &str) -> Option<String> {
    let value = serde_json::from_str::<Value>(text).ok()?;
    let model = value.get("model")?.as_str()?.trim();
    (!model.is_empty()).then(|| model.to_string())
}

/// `model = "…"` at the top of a TOML file, before its first table.
pub fn toml_model(text: &str) -> Option<String> {
    for line in text.lines().map(str::trim) {
        if line.starts_with('[') {
            return None;
        }
        let Some((key, value)) = line.split_once('=') else { continue };
        if key.trim() != "model" {
            continue;
        }
        let value = value.split(" #").next().unwrap_or("").trim();
        let model = value.strip_prefix('"').and_then(|v| v.strip_suffix('"'))
            .or_else(|| value.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')))?
            .trim();
        return (!model.is_empty()).then(|| model.to_string());
    }
    None
}

/// What the first entry of the Model list says: the agent's own model, by
/// the name the list gives it when it is one of them.
pub fn default_label(offered: &Offered) -> String {
    match &offered.default {
        Some(model) => {
            let name = offered.models.iter().find(|(_, id)| id == model).map(|(name, _)| name.as_str());
            format!("Agent's default ({})", name.unwrap_or(model))
        }
        None => "Agent's default".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn omarchy_names_the_agents_dictation_runs_and_the_ones_it_opens() {
        assert_eq!(Detected::of("claude\n"), Detected::Known(Agent::Claude));
        assert_eq!(Detected::of("codex"), Detected::Known(Agent::Codex));
        assert_eq!(Detected::of("cursor-agent"), Detected::Known(Agent::Cursor));
        assert_eq!(Detected::of("copilot"), Detected::Known(Agent::Copilot));
        assert_eq!(Detected::of("opencode"), Detected::Known(Agent::OpenCode));
        assert_eq!(Detected::of("agy"), Detected::Known(Agent::Antigravity));
        assert_eq!(Detected::of(""), Detected::None);
        assert_eq!(Detected::of("pi"), Detected::Other { id: "pi".into(), name: "Pi".into() });
        assert_eq!(Detected::of("newcomer").name().as_deref(), Some("newcomer"));
        for agent in Agent::ALL {
            assert_eq!(Agent::named(agent.id()), Some(agent));
        }
    }

    #[test]
    fn claude_runs_a_turn_headless_and_resumes_the_session() {
        let first = Agent::Claude.arguments("--add an RSI", None, None);
        assert_eq!(
            first,
            ["-p", "--output-format", "json", "--permission-mode", "auto", "--", "--add an RSI"]
        );
        let next = Agent::Claude.arguments("now MACD", Some("abc"), Some("opus"));
        assert_eq!(&next[5..9], ["--resume", "abc", "--model", "opus"]);
        assert_eq!(next.last().unwrap(), "now MACD");
    }

    #[test]
    fn codex_runs_a_turn_headless_and_resumes_the_session() {
        let first = Agent::Codex.arguments("hi", None, None);
        assert_eq!(&first[..2], ["exec", "--approve-for-me"]);
        assert!(first.contains(&"--json".to_string()));
        assert!(!first.contains(&"-m".to_string()));
        assert_eq!(&first[first.len() - 2..], ["--", "hi"]);
        let next = Agent::Codex.arguments("again", Some("t-1"), Some("gpt-5.5"));
        assert_eq!(&next[..3], ["exec", "--approve-for-me", "resume"]);
        assert_eq!(&next[next.len() - 5..], ["-m", "gpt-5.5", "t-1", "--", "again"]);
    }

    #[test]
    fn cursor_runs_a_turn_headless_and_resumes_the_session() {
        let first = Agent::Cursor.arguments("hi", None, None);
        assert_eq!(
            first,
            ["-p", "--output-format", "json", "--yolo", "--trust", "agent", "--", "hi"]
        );
        let next = Agent::Cursor.arguments("again", Some("c-1"), Some("sonnet-4.5"));
        assert_eq!(&next[5..], ["--resume", "c-1", "--model", "sonnet-4.5", "agent", "--", "again"]);
        let out = r#"{"type":"result","subtype":"success","is_error":false,"result":"Done.","session_id":"c-1","request_id":"r"}"#;
        assert_eq!(Agent::Cursor.read(out), Turn { session: Some("c-1".into()), reply: Some("Done.".into()), failure: None });
    }

    #[test]
    fn copilot_is_given_a_session_and_keeps_it() {
        let session = Agent::Copilot.new_session().unwrap();
        assert_eq!(session.len(), 36);
        assert_eq!(&session[14..15], "4");
        assert_ne!(Agent::Copilot.new_session(), Some(session.clone()));
        assert_eq!(Agent::Claude.new_session(), None);
        let first = Agent::Copilot.arguments("hi", Some(&session), None);
        assert_eq!(first, ["--prompt", "hi", "--silent", "--allow-all", "--no-ask-user", "--session-id", &session]);
        let modelled = Agent::Copilot.arguments("hi", Some(&session), Some("gpt-6-astra"));
        assert_eq!(&modelled[modelled.len() - 2..], ["--model", "gpt-6-astra"]);
        assert_eq!(Agent::Copilot.read("  Added RSI.\n").reply.as_deref(), Some("Added RSI."));
    }

    #[test]
    fn opencode_and_antigravity_start_afresh_each_turn() {
        assert_eq!(Agent::OpenCode.arguments("hi", None, None), ["run", "--auto", "--archived", "hi"]);
        assert_eq!(
            Agent::OpenCode.arguments("hi", None, Some("opencode/big-pickle")),
            ["run", "--auto", "--archived", "-m", "opencode/big-pickle", "hi"]
        );
        assert_eq!(Agent::Antigravity.arguments("hi", None, None), ["--dangerously-skip-permissions", "--print", "hi"]);
        assert_eq!(
            Agent::Antigravity.arguments("hi", None, Some("gemini-3-pro")),
            ["--dangerously-skip-permissions", "--model", "gemini-3-pro", "--print", "hi"]
        );
        assert!(!Agent::OpenCode.resumes() && !Agent::Antigravity.resumes());
        assert_eq!(Agent::OpenCode.read("ok\n"), Turn { session: None, reply: Some("ok".into()), failure: None });
    }

    /// Each agent is let do what `omarchy-agent` lets it do when it opens
    /// it, and nothing more: whatever else the person allowed or denied in
    /// the agent's own settings decides the rest.
    #[test]
    fn every_agent_runs_with_omarchys_permissions_and_no_others() {
        let omarchy: [(Agent, &[&str]); 6] = [
            (Agent::Claude, &["--permission-mode", "auto"]),
            (Agent::Codex, &["--approve-for-me"]),
            (Agent::Cursor, &["--yolo", "--trust"]),
            (Agent::Copilot, &["--allow-all"]),
            (Agent::OpenCode, &["--auto"]),
            (Agent::Antigravity, &["--dangerously-skip-permissions"]),
        ];
        let others = [
            "dontAsk",
            "bypassPermissions",
            "--allowedTools",
            "--dangerously-bypass-approvals-and-sandbox",
            "--sandbox",
            "-s",
            "--force",
            "--allow-all-tools",
            "--allow-all-paths",
            "--yolo-mode",
            "--full-auto",
        ];
        for (agent, flags) in omarchy {
            for session in [None, agent.resumes().then_some("s")] {
                let arguments = agent.arguments("hi", session, Some("m"));
                assert!(arguments.windows(flags.len()).any(|run| run == flags), "{agent:?}: {arguments:?}");
                for other in others {
                    assert!(!arguments.iter().any(|a| a == other), "{agent:?} has {other}: {arguments:?}");
                }
                assert!(!arguments.iter().any(|a| a.contains("sandbox")), "{agent:?}: {arguments:?}");
            }
        }
    }

    #[test]
    fn default_passes_no_model() {
        for agent in Agent::ALL {
            for model in [None, Some(""), Some("  ")] {
                let arguments = agent.arguments("hi", None, model);
                assert!(
                    !arguments.iter().any(|a| a == "--model" || a == "-m"),
                    "{agent:?} {model:?}: {arguments:?}"
                );
            }
        }
    }

    #[test]
    fn the_agents_own_model_is_read_from_its_settings() {
        assert_eq!(json_model(r#"{"model":"opus","permissions":{}}"#).as_deref(), Some("opus"));
        assert_eq!(json_model(r#"{"permissions":{}}"#), None);
        assert_eq!(json_model("not json"), None);
        let toml = "# mine\nmodel = \"gpt-5.5\" # the fast one\napproval = 'never'\n[profiles.x]\nmodel = \"o3\"\n";
        assert_eq!(toml_model(toml).as_deref(), Some("gpt-5.5"));
        assert_eq!(toml_model("[profiles.x]\nmodel = \"o3\"\n"), None, "a profile's is not the default");
        assert_eq!(toml_model("model_reasoning_effort = \"high\""), None);
        let claude = Offered { default: Some("opus".into()), models: vec![("Opus".into(), "opus".into())] };
        assert_eq!(default_label(&claude), "Agent's default (Opus)");
        let codex = Offered { default: Some("gpt-5.5".into()), models: vec![] };
        assert_eq!(default_label(&codex), "Agent's default (gpt-5.5)");
        assert_eq!(default_label(&Offered::default()), "Agent's default");
    }

    #[test]
    fn every_agent_offers_models_of_its_own_kept_here() {
        for agent in Agent::ALL {
            let offered = offered(agent);
            assert!(!offered.models.is_empty(), "{agent:?}");
            assert!(offered.models.iter().all(|(name, id)| !name.is_empty() && !id.contains(' ')), "{agent:?}");
        }
        assert!(Agent::OpenCode.models().iter().all(|(_, id)| id.contains('/')), "provider/model");
        assert_eq!(model_setting(Agent::Cursor), "agent_model.cursor-agent");
    }

    #[test]
    fn claude_says_its_session_and_its_answer() {
        let out = r#"{"type":"result","subtype":"success","is_error":false,"result":"Added RSI(14) on pos:0.","session_id":"s-1"}"#;
        assert_eq!(
            Agent::Claude.read(out),
            Turn { session: Some("s-1".into()), reply: Some("Added RSI(14) on pos:0.".into()), failure: None }
        );
        let failed = r#"{"type":"result","subtype":"error_max_turns","is_error":true,"session_id":"s-2"}"#;
        assert_eq!(Agent::Claude.read(failed).failure.as_deref(), Some("error_max_turns"));
        assert_eq!(Agent::Claude.read("not json"), Turn::default());
    }

    #[test]
    fn codex_says_its_session_and_its_last_words() {
        let out = [
            r#"{"type":"thread.started","thread_id":"t-9"}"#,
            r#"{"type":"turn.started"}"#,
            r#"{"type":"item.completed","item":{"type":"agent_message","text":"Looking."}}"#,
            r#"{"type":"item.completed","item":{"type":"command_execution","command":"omacharts status show"}}"#,
            r#"{"type":"item.completed","item":{"type":"agent_message","text":"Done: gold on 15m."}}"#,
            r#"{"type":"turn.completed"}"#,
        ]
        .join("\n");
        assert_eq!(
            Agent::Codex.read(&out),
            Turn { session: Some("t-9".into()), reply: Some("Done: gold on 15m.".into()), failure: None }
        );
        let failed = r#"{"type":"turn.failed","error":{"message":"rate limited"}}"#;
        assert_eq!(Agent::Codex.read(failed).failure.as_deref(), Some("rate limited"));
    }

    #[test]
    fn a_command_is_run_to_the_end_and_read() {
        let invocation = Invocation {
            program: "sh".into(),
            arguments: vec!["-c".into(), "echo \"$WORDS\"; echo oops >&2; exit 3".into()],
            directory: std::env::temp_dir(),
            environment: vec![("WORDS".into(), "said".into())],
        };
        let output = run(&invocation, LONGEST).unwrap();
        assert_eq!(output, Output { success: false, stdout: "said\n".into(), stderr: "oops\n".into() });
    }

    #[test]
    fn a_command_that_does_not_end_is_stopped() {
        let invocation = Invocation {
            program: "sleep".into(),
            arguments: vec!["30".into()],
            directory: std::env::temp_dir(),
            environment: vec![],
        };
        let started = Instant::now();
        assert!(run(&invocation, Duration::from_millis(300)).is_err());
        assert!(started.elapsed() < Duration::from_secs(10));
    }

    #[test]
    fn a_missing_agent_is_said_to_be_missing() {
        let invocation = Invocation {
            program: "omacharts-no-such-agent".into(),
            arguments: vec![],
            directory: std::env::temp_dir(),
            environment: vec![],
        };
        assert_eq!(run(&invocation, LONGEST), Err("omacharts-no-such-agent is not installed".into()));
    }
}
