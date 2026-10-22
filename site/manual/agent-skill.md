---
title: "The agent skill"
description: "What the agent skill is, what it teaches an agent, and installing it for Claude and Codex."
section: agents
nav_order: 31
---

The agent skill is a file that teaches an AI agent to drive Omacharts. The
same skill serves Claude Code and Codex. With it installed, you say "put a
200-day average on this" or "chart SPY and QQQ side by side" and the agent
runs the `omacharts` commands that do it, in the open window. The commands
are in [The CLI](/manual/cli/).

## What the skill teaches

It covers how charts are named, how chart templates, link
modes and drawings work, and that signing in to a feed needs you at a
browser, so the agent does that only when asked.

## Installing the skill

Install it from a terminal with `omacharts skill install`. The package and
the first launch do not install it.

```
$ omacharts skill install
installed for Claude: /home/you/.claude/skills/omacharts -> /usr/share/omacharts/agents/skills/omacharts
installed for Codex: /home/you/.codex/skills/omacharts -> /usr/share/omacharts/agents/skills/omacharts
  [exit 0]
```

It installs for every agent found on the machine. With no agent found it says where
it looked and installs nothing. If something else already sits where the
skill goes, that agent is skipped and reported, and any other agent is still
installed. Running the install twice is not an error. Upgrading the package
upgrades the skill.

## Claude Code plugin

Claude Code can also take the skill as a plugin. Then
`claude plugin update omacharts` brings a newer skill without a newer
package, and `/plugin` lists or disables it like any other:

```sh
claude plugin marketplace add /usr/share/omacharts/agents
claude plugin install omacharts@omacharts
```

Codex needs only `skill install`.

## An example session

> **You:** Give me NVDA at four resolutions, two by two.

Under the hood, the agent runs:

```
omacharts chartbook create Desk --symbol NVDA --switch
omacharts chart split vertical
omacharts chart split horizontal --chart pos:0
omacharts chart split horizontal --chart pos:2
omacharts chart set --chart pos:1 --resolution 1h
omacharts chart set --chart pos:2 --resolution 15m
omacharts chart set --chart pos:3 --resolution 5m
omacharts status show
```

It finishes with `status show` to check what is on screen.

> **You:** Put an RSI on all four.

```
omacharts template create Momentum
omacharts template indicator add Momentum rsi --period 14
omacharts chart set --chart pos:0 --template Momentum
omacharts chart set --chart pos:1 --template Momentum
omacharts chart set --chart pos:2 --template Momentum
omacharts chart set --chart pos:3 --template Momentum
```

## See also

- [The CLI](/manual/cli/)
- [Where your data lives](/manual/your-data/#where-everything-is)
