---
name: video
description: Make narrated screencast videos of Omacharts, stage by stage — brief, script, read-through, storyboard, render — from a Markdown script the author steers. `/video` shows where every video stands and what to do next; `/video new <idea>` starts one; notes in chat or `> note:` lines in the script steer it. Use whenever a video, screencast, demo, trailer, voice-over or YouTube clip of Omacharts is mentioned, when a file under video/scripts/ is touched, or when the author comments on a script, a read-through, a storyboard or a rendered video.
---

# Omacharts videos

Videos are rendered, not recorded. The author — whoever is making the
video — shapes a script and judges what comes out; nobody sits at a screen
recorder, and nothing touches the desktop they are using. Every video is one Markdown file, `video/scripts/<slug>.md`, and moves
through five stages, each cheaper to change than the next. Most of the shaping
happens in the first three, where it costs seconds.

## The stages

| | Stage | The author gets | Takes |
|---|---|---|---|
| 1 | **Brief** | pitch, audience, opening payoff, 4–7 beats, length | a minute to read |
| 2 | **Script** | every scene as Picture + Voice | a few minutes to read |
| 3 | **Read-through** | the narration, voiced, played on their machine | a minute to listen |
| 4 | **Storyboard** | one still per scene, with its narration under it | a minute to look |
| 5 | **Render** | the finished video, with music | the video's length |

### 1. Brief — is this the right video?

**What to look for.** Is it one idea? Does the payoff in the first five seconds
make someone want the rest? Are the beats in the order a viewer needs them? Is
the length honest for what it covers?

**Typical notes.** "Open on the theme switch instead." "Drop beat 4." "This is
two videos." "Aim it at people already using Omacharts."

### 2. Script — does it say it well?

**What to look for.** Read the Voice lines aloud in your head: does each
sentence sound like you explaining it, or like a brochure? Does the Picture
show what the Voice talks about at that moment? Is anything said that the
screen already shows?

**Typical notes.** "Too wordy." "Say why, not what." "Scene 3 needs a beat to
breathe." "Show the readout before naming it."

### 3. Read-through — does it sound right?

**What to look for.** Pace, stumbles, mispronounced words, sentences that run
out of breath, and the voice itself. This is where TTS shows its seams, and
fixing a line here costs a minute of audio, not a render.

**Typical notes.** "Faster in scene 2." "She says Omarchy wrong." "Pause after
the percentage."

### 4. Storyboard — does it look right?

**What to look for.** The data on screen (is it a chart worth looking at?),
the layout and theme, where the cursor and zooms land, whether each picture
matches the line under it.

**Typical notes.** "Pick a symbol with a cleaner trend." "Zoom closer on the
readout." "Too much on screen in scene 1."

### 5. Render — is it finished?

**What to look for.** Mostly polish: timing of actions against the voice, zoom
feel, music level and choice, the ending. A note that goes back to the story
means an earlier stage missed it — say which, so it can be caught there next
time.

## Commands

- `/video` — every script, its stage, and the one thing to do next on each.
- `/video new <idea>` — the scriptwriter drafts a brief. Say as much or as
  little as you like: a topic is enough, a length and an audience help.
- `/video <slug> <notes>`, or notes in plain chat about a video — apply them
  at its current stage.
- `/video <slug> ok` — approve the current stage and produce the next one.
- `/video <slug> read`, `board`, `render` — produce that stage now, without
  moving the stage forward.

Notes can also live in the script: a `> note: …` line under the scene it is
about. Write as many as you want while reading, then say "apply notes".

## Running it

**Writing and revising** goes to the `video-scriptwriter` subagent. Hand it
the script's path, the stage, and the author's notes verbatim — nothing from this
conversation it has not been told. It edits the file and reports what it
changed and any note it could not apply. It reads the script fresh every
time, so the file must hold everything that matters.

**After it returns,** read the diff yourself, then commit the script with a
message that says what the steer was ("Open the measure video on the readout"),
so `git log` on a script is the history of how it was shaped. Then tell the author,
in a few lines, what changed and what to look at.

**Approving a stage** moves `stage:` forward in the frontmatter and produces
the next stage's artifact.

**Read-through, storyboard and render** are `bin/video`, which runs the heavy
work on this machine and plays or opens the result where the author watches —
locally, or on another machine when `~/.config/omacharts-video/config` says
`viewer = ssh <host>`:

| Stage | Command | Makes, under `video/out/<slug>/` |
|---|---|---|
| read-through | `bin/video read <slug>` | `read.wav`: every scene voiced, in order |
| storyboard | `bin/video board <slug>` | `board.png`: a still per scene, clicks marked, narration under each |
| music | `bin/video music <slug> [n]` | `music/candidate-*.flac`, sized to the video |
| render | `bin/video render <slug>` | `<slug>.mp4` (4K), `<slug>-1080p.mp4`, `.srt`, `review/` |

`bin/video status` lists the scripts; `bin/video show <file>` plays any file
where the author watches; `bin/video setup` builds the speech and music
environments once. Voicing is cached per scene, so after the first read-through
only changed lines are spoken again — speech runs at several times real time on
the CPU, so say how long a command will take before starting a slow one.

**Before handing over a storyboard or a render, look at it.** Open `board.png`
or the frames in `review/` and check that each action did what the Picture says
— a click that missed is the most common fault, and the author should not be
the one to find it.

**A pinned choice goes in the frontmatter:** `voice:` names a file in
`video/voices/`, `music_file:` the track the author picked.

## This skill learns

**When the author gives a note, ask whether it is about this video or about every
video.** "Scene 3 is too fast" is about this one: fix it in the script. "You
always cram the ending" is about all of them: fix it, and write it under
[Taste](#taste) as a rule with its reason, in the same turn. If it contradicts
a rule already there, replace that rule. Say in the reply which rule was added
or changed, so they can veto it.

Things that broke in the tooling go in [studio.md](studio.md), with why.

## Taste

Rules the author has set. They override [craft.md](craft.md), the general
principles of a good screencast, which the scriptwriter also follows.

- **Tone: succinct, conversational and friendly, without overdoing it.**
  Talk like showing a peer something you built. No hype words ("blazing",
  "game-changer", "super easy"), no catchphrases ("let's dive in", "and
  that's it!"), no exclamation marks, no stacked rhetorical questions.
  Friendly comes from plain words and a relaxed pace, not from enthusiasm
  markers. Applies to the narration, the on-screen text and the music
  choice alike.
- **Narrator: a female voice, not deep.** Mid-pitched to slightly bright,
  clear and relaxed. A low, heavy voice reads as a trailer, not as someone
  showing you their tool.
  The voice is pinned: `video/voices/narrator.json`, a light, airy but
  clear woman's voice in her thirties, cloned from `narrator.wav`. Every
  video uses it unless the author asks otherwise.
- **Film the latest main.** Videos show omacharts as it is on main —
  `bin/video` fetches it and builds it in a worktree of its own — unless the
  author asks for something else, which goes in the script as `build:`.
- **Music: a theme composed for each video,** not a library track reused.
  The starting point is ambient electronic — gentle analog synth pads, a
  soft plucked arpeggio, a subtle pulse, calm and optimistic, spacious —
  shaped to the video's mood and length. The brief proposes it; the author
  picks from `bin/video music` candidates.

## Reference

- [script-format.md](script-format.md) — the Markdown format, cues and
  actions, and how notes work.
- [craft.md](craft.md) — what makes a screencast good.
- [studio.md](studio.md) — the pipeline, and how recording works without a
  screen.
