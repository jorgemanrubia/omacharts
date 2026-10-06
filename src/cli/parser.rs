//! The argument parser, built from [`super::spec`].
//!
//! Clap is given the table rather than a hand-written tree, so `--help`, the
//! "did you mean" on a typo, the shell completions and the JSON surface all
//! describe the same commands by construction. The alternative — a parser
//! here and a description of it somewhere else — is two things that agree
//! until the first hurried afternoon.

use clap::{Arg, ArgAction, Command};

use super::spec::{self, SURFACE};

/// What `omacharts --help` opens with.
///
/// The front door, so it names the nouns, shows one command that works as
/// written, and says where the rest is — including the machine-readable
/// surface, because something driving this from a script should not have to
/// discover that by reading prose meant for a person.
const ABOUT: &str = "Fast, beautiful charting software for Omarchy.";

const AFTER: &str = "\
Examples:
  omacharts                            open the app
  omacharts NVDA                       open it on a symbol
  omacharts watchlist create Semis     make a watchlist
  omacharts watchlist add Semis NVDA AMD AVGO

A command that changes something takes effect in a window that is already
open, straight away — no restart, nothing to reload.

Full documentation:  doc/cli.md
Every command as JSON: omacharts surface --json";

pub fn command() -> Command {
    let mut app = Command::new("omacharts")
        .about(ABOUT)
        .version(env!("CARGO_PKG_VERSION"))
        .after_help(AFTER)
        .subcommand_required(false)
        .arg_required_else_help(false)
        .allow_external_subcommands(true)
        .disable_help_subcommand(true)
        .subcommand(
            Command::new("help")
                .about("Show help for a command")
                .arg(Arg::new("COMMAND").num_args(0..).help("the command to explain")),
        )
        .subcommand(
            Command::new("surface")
                .about("Every command, as JSON, for anything driving this from a script")
                .arg(json_flag())
                // Generated for the package rather than typed by anyone, so
                // they are hidden: a person reading --help has no use for
                // them, and leaving them in the listing invites the question.
                .arg(
                    Arg::new("completions")
                        .long("completions")
                        .value_name("SHELL")
                        .hide(true)
                        .value_parser(["bash", "zsh", "fish"])
                        .help("write a shell completion script to stdout"),
                )
                .arg(
                    Arg::new("man")
                        .long("man")
                        .action(ArgAction::SetTrue)
                        .hide(true)
                        .help("write the man page to stdout"),
                ),
        );

    // The launch options. Declared to clap so that they appear in `--help`
    // like everything else — they are taken off the line before any command
    // is parsed, so clap never actually sees one, but help somebody cannot
    // find is help that does not exist.
    for option in spec::LAUNCH {
        let mut arg = Arg::new(option.long)
            .long(option.long)
            .value_name(option.value)
            .num_args(1)
            .help(option.help);
        let values = spec::launch_values(option.long);
        if !values.is_empty() {
            arg = arg.value_parser(values);
        }
        app = app.arg(arg);
    }

    for noun in SURFACE {
        let mut group = Command::new(noun.name)
            .about(noun.about)
            .subcommand_required(true)
            .arg_required_else_help(true);
        for verb in noun.verbs {
            group = group.subcommand(build_verb(verb));
        }
        app = app.subcommand(group);
    }
    app
}

fn build_verb(verb: &'static spec::Verb) -> Command {
    let mut cmd = Command::new(verb.name)
        .about(verb.about)
        .after_help(format!("Example:\n  {}", verb.example));

    for arg in verb.args {
        let mut a = Arg::new(arg.name).help(arg.help).required(arg.required);
        if arg.many {
            a = a.num_args(1..).action(ArgAction::Append);
        }
        if !arg.values.is_empty() {
            a = a.value_parser(arg.values.to_vec());
        }
        cmd = cmd.arg(a);
    }
    for flag in verb.flags {
        let mut a = Arg::new(flag.long).long(flag.long).help(flag.help);
        match flag.value {
            None => a = a.action(ArgAction::SetTrue),
            Some(name) => a = a.value_name(name).num_args(1),
        }
        if !flag.values.is_empty() {
            a = a.value_parser(flag.values.to_vec());
        }
        cmd = cmd.arg(a);
    }
    if verb.json {
        cmd = cmd.arg(json_flag());
    }
    cmd
}

fn json_flag() -> Arg {
    Arg::new("json")
        .long("json")
        .action(ArgAction::SetTrue)
        .help("print the result as JSON, for scripts and agents")
}

/// The whole command tree, as JSON.
///
/// Written from the same table the parser is built from. An agent reading
/// this never has to guess a flag's type from the way its help reads, which
/// is the failure the human text cannot avoid.
pub fn surface_json() -> String {
    let mut out = String::from("{\"version\":");
    out.push_str(&json_str(env!("CARGO_PKG_VERSION")));
    out.push_str(",\"selector\":");
    out.push_str(&json_str(spec::SELECTOR));
    out.push_str(",\"launch\":[");
    for (i, option) in spec::LAUNCH.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str(&format!(
            "{{\"name\":{},\"description\":{},\"type\":\"value\",\"value\":{},\"values\":{}}}",
            json_str(option.long),
            json_str(option.help),
            json_str(option.value),
            json_list(&spec::launch_values(option.long)),
        ));
    }
    out.push_str("],\"exitCodes\":[");
    for (i, (code, meaning)) in super::EXIT_CODES.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str(&format!("{{\"code\":{code},\"meaning\":{}}}", json_str(meaning)));
    }
    out.push_str("],\"commands\":[");
    let mut first = true;
    for noun in SURFACE {
        for verb in noun.verbs {
            if !first {
                out.push(',');
            }
            first = false;
            out.push_str(&verb_json(noun, verb));
        }
    }
    out.push_str("]}");
    out
}

fn verb_json(noun: &spec::Noun, verb: &spec::Verb) -> String {
    let mut out = format!(
        "{{\"command\":{},\"noun\":{},\"verb\":{},\"description\":{},\"group\":{}",
        json_str(&format!("{} {}", noun.name, verb.name)),
        json_str(noun.name),
        json_str(verb.name),
        json_str(verb.about),
        json_str(noun.about),
    );
    out.push_str(",\"arguments\":[");
    for (i, arg) in verb.args.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str(&format!(
            "{{\"name\":{},\"description\":{},\"required\":{},\"variadic\":{},\"values\":{}}}",
            json_str(arg.name),
            json_str(arg.help),
            arg.required,
            arg.many,
            json_list(arg.values),
        ));
    }
    out.push_str("],\"flags\":[");
    let mut flags: Vec<String> = verb
        .flags
        .iter()
        .map(|flag| {
            format!(
                "{{\"name\":{},\"description\":{},\"type\":{},\"value\":{},\"values\":{}}}",
                json_str(flag.long),
                json_str(flag.help),
                json_str(if flag.value.is_some() { "value" } else { "switch" }),
                match flag.value {
                    Some(name) => json_str(name),
                    None => "null".to_string(),
                },
                json_list(flag.values),
            )
        })
        .collect();
    if verb.json {
        flags.push(format!(
            "{{\"name\":\"json\",\"description\":{},\"type\":\"switch\",\"value\":null,\"values\":[]}}",
            json_str("print the result as JSON, for scripts and agents"),
        ));
    }
    out.push_str(&flags.join(","));
    out.push_str(&format!(
        "],\"example\":{},\"writes\":{},\"touchesWorkspace\":{}}}",
        json_str(verb.example),
        verb.writes,
        verb.workspace,
    ));
    out
}

fn json_list(values: &[&str]) -> String {
    format!("[{}]", values.iter().map(|v| json_str(v)).collect::<Vec<_>>().join(","))
}

fn json_str(value: &str) -> String {
    super::json_string(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_parser_builds_and_agrees_with_itself() {
        command().debug_assert();
    }

    #[test]
    fn every_verb_carries_an_example_that_names_its_own_command() {
        for noun in SURFACE {
            for verb in noun.verbs {
                let head = format!("omacharts {} {}", noun.name, verb.name);
                assert!(
                    verb.example.starts_with(&head),
                    "{} {}: example is {:?}",
                    noun.name,
                    verb.name,
                    verb.example
                );
            }
        }
    }

    /// The surface is what something that cannot read prose relies on, so a
    /// field that stops being valid JSON is the whole contract gone.
    #[test]
    fn the_surface_is_json_and_describes_every_command() {
        let parsed: serde_json::Value = serde_json::from_str(&surface_json()).unwrap();
        let listed = parsed["commands"].as_array().unwrap();
        let expected: usize = SURFACE.iter().map(|n| n.verbs.len()).sum();
        assert_eq!(listed.len(), expected);
        assert!(!parsed["exitCodes"].as_array().unwrap().is_empty());
        for command in listed {
            assert!(command["example"].as_str().unwrap().starts_with("omacharts "));
            assert!(!command["description"].as_str().unwrap().is_empty());
        }
    }

    /// A launch option is part of the surface, and the surface is the only
    /// thing something driving this from a script can read. An option
    /// described nowhere is the environment variable this replaced.
    #[test]
    fn the_surface_describes_the_launch_options_and_the_feeds_they_accept() {
        let parsed: serde_json::Value = serde_json::from_str(&surface_json()).unwrap();
        let launch = parsed["launch"].as_array().expect("launch options");
        assert_eq!(launch.len(), spec::LAUNCH.len());
        let provider = launch
            .iter()
            .find(|option| option["name"] == "provider")
            .expect("--provider");
        let values: Vec<&str> =
            provider["values"].as_array().unwrap().iter().map(|v| v.as_str().unwrap()).collect();
        assert!(values.contains(&"yahoo") && values.contains(&"tos"), "{values:?}");
        assert_eq!(provider["value"], "NAME");
    }

    /// `--help` is where a person looks, and it is built from the same table.
    #[test]
    fn the_help_names_every_launch_option() {
        let help = command().render_help().to_string();
        for option in spec::LAUNCH {
            assert!(help.contains(&format!("--{}", option.long)), "{help}");
        }
    }

    /// Everything the JSON surface claims exists has to be a command the
    /// parser will actually accept. These are built from one table, so this
    /// passes by construction — which is the point: it fails the moment
    /// somebody describes a command somewhere else.
    #[test]
    fn the_surface_and_the_parser_describe_the_same_commands() {
        let app = command();
        for noun in SURFACE {
            let group = app
                .get_subcommands()
                .find(|c| c.get_name() == noun.name)
                .unwrap_or_else(|| panic!("the parser has no {:?}", noun.name));
            for verb in noun.verbs {
                assert!(
                    group.get_subcommands().any(|c| c.get_name() == verb.name),
                    "{} has no {:?}",
                    noun.name,
                    verb.name
                );
            }
        }
    }
}
