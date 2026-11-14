---
title: "Dictation"
description: "Saying what you want with Omacharts in front, and Omarchy's default agent doing it with the omacharts command."
section: agents
nav_order: 31.5
---

Say what you want with Omacharts in front, and Omarchy's default agent does
it in the open window, with the `omacharts` command.

{: .warning }
The agent can run any `omacharts` command, the [trading commands](/manual/cli/#trading) included, and nothing asks before it does. With a broker signed in, "sell two at the market" places that order. Don't dictate an order unless you mean it.

## Dictating

1. Put Omacharts in front, with the keyboard on a chart and not in a text box.
2. Hold <kbd>Right Alt</kbd> and say what you want.
3. Let go. A glow along the bottom of the window shows the agent working.

The key, the recording and the transcription are Omarchy's dictation, with
Superwhisper behind it, and Omacharts changes none of them. In a text box,
such as the symbol search or a drawing's text, what you say is typed there
as usual and nothing goes to the agent.

## What the agent is sent

One fixed line with your words in it:

```
The user is in Omacharts and said: "add an RSI". Use the `omacharts` CLI to do it.
```

Nothing about your charts goes with it. The agent asks `omacharts` for what
it needs, and the [agent skill](/manual/agent-skill/) helps it if installed.
Whatever you write in Preferences › Agent › **Context** follows that line,
after a blank line.

Later requests in the same conversation go as `The user said: "…"` and
nothing else.

## Conversations

With an agent that keeps the conversation, each request builds on the last,
so "now the same for silver" means what you would expect. A new conversation
starts when:

- Omacharts starts.
- Nothing has been dictated for 30 minutes.
- The default agent, its model or your context changes.
- The agent failed at the last request.
- You say "start over", "start again" or "new conversation", and nothing else.

Dictate while the agent is still working and the request waits its turn.
None is dropped. A request the agent is still on after ten minutes is
stopped, so the ones behind it run.

## While the agent works

A soft amber glow breathes along the bottom of the window from the first
request until the last one waiting is done, then fades. A request that fails
turns it red for a moment. With the desktop's animations off the glow shows
still.

Nothing else is shown when a request works. The [Log](/manual/window/#the-log)
has every dictation, the agent's whole reply, each request that had to wait
and each new conversation. When something goes wrong, such as no default
agent or an agent that fails, the window says why.

## Agents

The agent is Omarchy's default (`omarchy default agent`), asked for again at
each dictation, with the account Omarchy has selected for it.

| Agent | Runs | Keeps the conversation | Model choice |
|---|---|---|---|
| Claude Code | In the background | Yes | Sonnet, Opus, Haiku |
| Codex | In the background | Yes | A short list of named models |
| Cursor CLI | In the background | Yes | A short list of named models |
| GitHub Copilot | In the background | Yes | Auto, and a short list of named models |
| OpenCode | In the background | No | A short list of named models |
| Antigravity | In the background | No | A short list of named models |
| Any other | In a terminal, a new one each time | No | None. It uses its own |

Every agent runs with the same permissions Omarchy gives it when it launches
it, such as Claude Code's auto mode, and nothing more. What you allow or deny
in the agent's own settings decides the rest. A command the agent may not run
in the background is refused, and its reply in the Log says so; allow it in
the agent's settings if you want dictation to do it.

## Preferences

Preferences › **Agent** holds what dictation goes with.

| Row | Does |
|---|---|
| **Agent** | Names Omarchy's default agent, or says how to set one. |
| **Model** | The model dictation asks the agent for, kept for each agent. **Agent's default**, first and chosen until you pick another, leaves it to the agent, and names the model when the agent's own settings do, as in **Agent's default (Opus)**. Not shown for an agent that opens in a terminal. |
| **Context** | Text sent after the fixed line at the start of each conversation, such as `Prefer 5 minute charts.` Empty by default, and saved as you type. |

From a terminal these are the `agent_model.<agent>` and `agent_context`
settings ([Settings](/manual/cli/#settings)).

## An example

> **You:** Put gold on the 15 minute with an RSI.

Under the hood, the agent runs something like:

```
omacharts status show
omacharts chart set --symbol GC --resolution 15m
omacharts chart indicator add rsi
```

> **You:** Now the same for silver, beside it.

```
omacharts chart split horizontal
omacharts chart set --chart pos:1 --symbol SI --resolution 15m
omacharts chart indicator add rsi --chart pos:1
```

With a line selected on the chart:

> **You:** Make that one rose.

```
omacharts chart drawing list --selected --json
omacharts chart drawing set --id selected --color Rose
```

What is selected in the window is what the agent reads first for "this" or
"that"; with nothing selected it reads the chart's drawings and indicators
and picks the one you meant.

> **You:** Start over.

The next request opens a new conversation.

## See also

- [The agent skill](/manual/agent-skill/)
- [Settings in the CLI](/manual/cli/#settings)
- [Trading with the CLI](/manual/cli/#trading)
- [Preferences](/manual/preferences/#agent)
