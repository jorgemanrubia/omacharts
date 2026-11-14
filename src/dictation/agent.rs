//! Omarchy's default agent, run without a terminal, one turn at a time.
//!
//! Omarchy keeps one coding agent as the default (`omarchy default agent`,
//! stored in `~/.config/omarchy/defaults/agent`), and `omarchy agent` opens it
//! in a terminal. A dictation wants something else of it: a turn run in the
//! background, its answer read back, and the next turn continuing the same
//! session. So the agent's own command is run here, in its headless mode,
//! with its session id kept between turns:
//!
//! - **Claude Code**: `claude -p --output-format json`, then `--resume <id>`.
//!   It may run `omacharts` and load skills, and nothing else: anything more
//!   is refused rather than asked about, since nobody is there to answer.
//! - **Codex**: `codex exec --json`, then `codex exec resume <id>`. It runs
//!   outside Codex's sandbox, which would keep `omacharts` from reaching the
//!   window.
//!
//! Any other default agent is said to be one dictation cannot drive yet.
//! The account Omarchy has selected for the agent is used, as `omarchy agent`
//! uses it.

use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::Value;

/// The agents dictation knows how to run a turn of.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Agent {
    Claude,
    Codex,
}

impl Agent {
    /// The agent Omarchy calls `name`, or why it cannot be driven.
    pub fn named(name: &str) -> Result<Agent, String> {
        match name.trim() {
            "claude" => Ok(Agent::Claude),
            "codex" => Ok(Agent::Codex),
            "" => Err(NO_AGENT.to_string()),
            other => Err(format!("Dictation works with Claude Code and Codex, and the default agent is {other}")),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Agent::Claude => "Claude Code",
            Agent::Codex => "Codex",
        }
    }

    /// Its command, and Omarchy's name for it.
    pub fn program(self) -> &'static str {
        match self {
            Agent::Claude => "claude",
            Agent::Codex => "codex",
        }
    }

    /// Where it keeps an account other than its first.
    pub fn home_variable(self) -> &'static str {
        match self {
            Agent::Claude => "CLAUDE_CONFIG_DIR",
            Agent::Codex => "CODEX_HOME",
        }
    }

    /// The arguments for one turn of `prompt`, continuing `session` if
    /// there is one.
    pub fn arguments(self, prompt: &str, session: Option<&str>) -> Vec<String> {
        let mut arguments: Vec<String> = Vec::new();
        let mut push = |items: &[&str]| arguments.extend(items.iter().map(|item| item.to_string()));
        match self {
            Agent::Claude => {
                push(&["-p", "--output-format", "json", "--permission-mode", "dontAsk"]);
                if let Some(session) = session {
                    push(&["--resume", session]);
                }
                push(&["--allowedTools", "Bash(omacharts:*)", "Skill", "--", prompt]);
            }
            Agent::Codex => {
                push(&["exec"]);
                if session.is_some() {
                    push(&["resume"]);
                }
                push(&["--json", "--skip-git-repo-check", "-c", "sandbox_mode=\"danger-full-access\""]);
                if let Some(session) = session {
                    push(&[session]);
                }
                push(&["--", prompt]);
            }
        }
        arguments
    }

    /// What a turn printed, read.
    pub fn read(self, stdout: &str) -> Turn {
        match self {
            Agent::Claude => read_claude(stdout),
            Agent::Codex => read_codex(stdout),
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn omarchy_names_the_agents_dictation_drives() {
        assert_eq!(Agent::named("claude\n"), Ok(Agent::Claude));
        assert_eq!(Agent::named("codex"), Ok(Agent::Codex));
        assert_eq!(Agent::named(""), Err(NO_AGENT.to_string()));
        assert!(Agent::named("pi").unwrap_err().contains("the default agent is pi"));
    }

    #[test]
    fn claude_runs_a_turn_headless_and_resumes_the_session() {
        let first = Agent::Claude.arguments("--add an RSI", None);
        assert_eq!(
            first,
            [
                "-p", "--output-format", "json", "--permission-mode", "dontAsk", "--allowedTools",
                "Bash(omacharts:*)", "Skill", "--", "--add an RSI"
            ]
        );
        let next = Agent::Claude.arguments("now MACD", Some("abc"));
        assert_eq!(&next[5..7], ["--resume", "abc"]);
        assert_eq!(next.last().unwrap(), "now MACD");
    }

    #[test]
    fn codex_runs_a_turn_headless_and_resumes_the_session() {
        let first = Agent::Codex.arguments("hi", None);
        assert_eq!(first[0], "exec");
        assert!(first.contains(&"--json".to_string()));
        assert_eq!(&first[first.len() - 2..], ["--", "hi"]);
        let next = Agent::Codex.arguments("again", Some("t-1"));
        assert_eq!(&next[..2], ["exec", "resume"]);
        assert_eq!(&next[next.len() - 3..], ["t-1", "--", "again"]);
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
