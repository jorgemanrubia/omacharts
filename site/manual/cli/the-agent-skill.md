---
title: "The agent skill"
description: "Installing the agent skill for Claude and Codex."
section: agents
parent: cli
nav_order: 21
---

`skill` installs the agent skill ([The agent skill](/manual/agent-skill/))
for Claude and Codex.

## `skill`

| Command | Does |
|---|---|
| `omacharts skill install` | Installs the skill for each agent, in its skills folder |
| `omacharts skill status` | Says, per agent, whether it is installed and where |
| `omacharts skill uninstall` | Removes it, and the agent's `skills` folder too if that leaves it empty |

All three take the same flags:

| Flag | Does |
|---|---|
| `--claude` | Claude only, whether or not it looks installed on this machine |
| `--codex` | Codex only, the same way |
| `--to DIR` | One skills folder instead of any agent's, such as a project's own `.claude/skills`. A relative path is from where you are |
| `--json` | One JSON document on stdout: each agent's `state` (`installed`, `absent`, `occupied`, `dangling`), its `path`, whether it was `refused`, and the skill's `source` |

| Case | What happens |
|---|---|
| No flag | Installs for every agent present, meaning its configuration folder exists or its command is on `PATH` |
| Where an agent's skills go | `$CLAUDE_CONFIG_DIR/skills`, else `~/.claude/skills`; `$CODEX_HOME/skills`, else `~/.codex/skills` |
| No agent found | Says where it looked, writes nothing, exits `0` |
| Already installed | `already installed`, exit `0`. Running it twice is not an error |
| An earlier install pointing at another copy of the skill | Pointed at this one, and says so |
| Something else at `skills/omacharts` | Left as it is, reported on stderr, exit `5`. Any other agent is still installed and reported |
| The package removed after installing | `skill status` says the skill is no longer where it pointed. `skill uninstall` clears it |
| The skill missing from this machine | `install` exits `1` and names where it expected it |

Upgrading the package upgrades the installed skill.

```
$ omacharts skill install
installed for Claude: /home/you/.claude/skills/omacharts -> /usr/share/omacharts/agents/skills/omacharts
installed for Codex: /home/you/.codex/skills/omacharts -> /usr/share/omacharts/agents/skills/omacharts
  [exit 0]

$ omacharts skill status
installed for Claude: /home/you/.claude/skills/omacharts -> /usr/share/omacharts/agents/skills/omacharts
not installed for Codex; `omacharts skill install` would write /home/you/.codex/skills/omacharts
  [exit 0]

$ omacharts skill install --to proj/skills
omacharts: not installed: /home/you/proj/skills/omacharts is already a directory, and not something Omacharts put there; move it aside and run this again
  [exit 5]
```
