---
title: "Dictation"
description: "Saying what you want with Omacharts in front, and Omarchy's default agent doing it."
section: agents
nav_order: 31.5
---

Hold <kbd>Right Alt</kbd>, say what you want, and let go. With Omacharts in
front, what you said goes to Omarchy's default agent, and the agent does it
with the `omacharts` command, in the open window.

> "Put gold on the 15 minute with an RSI."

## Dictating is Omarchy's

The key, the recording, the blue animation at the bottom of the screen and
the transcription are Omarchy's dictation, set up in Omarchy and working as
they do everywhere else. Omacharts changes none of it. It needs Superwhisper
as Omarchy's dictation backend.

Dictate into a text box in Omacharts, such as the symbol search or a
drawing's text, and the words are typed there as usual. Only what you say
while no text box has the keyboard goes to the agent.

## What the agent is sent

The agent is the one Omarchy runs as its default (`omarchy default agent`),
with the account Omarchy has selected for it. Claude Code and Codex are
supported.

The first time, Omacharts sends one line and your words:

```
The user is in Omacharts and said: "Put gold on the 15 minute with an RSI.". Use the `omacharts` CLI to do it.
```

Nothing about your charts goes with it. The agent asks `omacharts` for what
it needs, and the [agent skill](/manual/agent-skill/) helps it if installed.
Claude Code may run `omacharts` and nothing else. Codex runs outside its
sandbox, which would keep it from reaching the window.

A toast says when your words are sent and when the agent is done, with the
first line of its answer. The whole answer is in the Log. If there is no
default agent, or it fails, the corner notice says why.

While the agent works, a soft amber glow breathes along the bottom of the
window, ending in red if it fails; with animations off it shows still.

## Your own context

Whatever you write in **Preferences › Agent** is sent after that line, at the
start of each conversation. It is empty until you write in it.

```
Use Sonnet. Keep answers to one line.
```

From a terminal, it is the `agent_context` setting
([Settings](/manual/cli/#settings)).

## One conversation

What you say next continues the same conversation, so each request builds on
the last:

> "Put gold on the 15 minute with an RSI."
>
> "Now the same for silver, beside it."
>
> "Link them."

Say something while the agent is still working and it waits its turn. A
toast says it was queued, and nothing is dropped.

A new conversation starts when:

- Omacharts starts.
- Nothing has been dictated for 30 minutes.
- You change the default agent or your context in Preferences.
- The agent could not do the last request.
- You say "start over" or "new conversation", and nothing else.

## See also

- [The agent skill](/manual/agent-skill/)
- [The CLI](/manual/cli/)
- [Preferences](/manual/preferences/#agent)
