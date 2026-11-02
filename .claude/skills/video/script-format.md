# The script format

One Markdown file per video, `video/scripts/<slug>.md`. It is the only state
a video has: whatever stage it is in, the file says so, and anyone — the author,
the scriptwriter, a session that starts after `/clear` — picks it up from
there. It has to read well as prose, because the author reads it to judge the
video, and parse cleanly, because the pipeline renders from it.

```markdown
---
title: Measure a move
stage: brief
length: 60s
theme: tokyo-night
music: ambient synth pads, soft plucked arpeggio, calm and optimistic
---

## Brief

**Pitch.** Alt+M answers "how far did that go?" in two clicks.

**For.** Traders new to Omacharts who chart on another app today.

**Payoff.** Opens on a measurement already drawn across NVDA's March run:
+38.2%, 41 bars, 58 days.

**Beats.**
1. The question every chart raises.
2. Measure it: Alt+M, two clicks.
3. What the readout tells you.
4. It works from the terminal too.
5. Next step: press `?`.

## Setup

    cli chartbook create Measure --symbol NVDA
    cli chartbook switch Measure
    cli chart set --resolution 1D

## Script

### 1. The question

**Picture.** NVDA daily, full window. The cursor rests near the March low.

**Voice.** How far did that move actually go? {ruler}Alt and M,
{start}click where it starts, {end}click where it ends.

    ruler: key alt+m
    start: click 18% 72%
    end:   click 64% 31%
    after: 1.0

## Pronunciation

- Omarchy: oh-MAR-kee
- NVDA: Nvidia
```

## The parts

- **Frontmatter.** `stage` is one of `brief`, `script`, `board`, `render`, `done`, and is moved forward only when the author approves a stage.
  `build` says which omacharts to film: `main` when absent (the latest
  main, fetched and built in a worktree of its own), or a branch, tag or
  commit, or `here` for this checkout's own build.
  `polish: off` renders without the zooms and key badges.
  `voice` names a narrator in `video/voices/<name>.json` (`draft` until one
  is chosen). `music` describes the music in words; `music_file` pins the
  track picked from `bin/video music`.
- **Brief.** Exists from the first draft on; stays as the record of intent
  once the script is written.
- **Setup.** Off-camera commands run before recording starts, one per line.
- **Script.** Written at the `script` stage. One `###` section per scene,
  numbered, each with **Picture** (what the viewer sees, in prose), **Voice**
  (the exact narration) and an indented action block.
- **Storyboard.** Added by `bin/video board`: a link to
  `video/storyboards/<slug>.jpg`, the latest storyboard, kept in the repo.
- **Pronunciation.** How a word should sound, applied to every scene's
  narration before it goes to TTS.

## Cues and actions

A `{name}` in the narration marks the word an action fires on. The action
block names each cue once:

| Action | Means |
|---|---|
| `key <chord>` | press a chord: `alt+m`, `ctrl+alt+h`, `Escape`, `question` |
| `type <text>` | type literal text, at human speed |
| `cli <args>` | run `omacharts <args>` against the studio window |
| `move <x%> <y%>` | glide the cursor there |
| `click <x%> <y%>` | glide there and click |
| `rclick <x%> <y%>` | glide there and right-click, for a context menu |
| `drag <x%> <y%> <x%> <y%>` | press, move and release as one gesture |
| `scroll <x%> <y%> <steps>` | wheel notches; negative scrolls up |
| `wait <seconds>` | pause the timeline |
| `zoom <x%> <y%> <scale>` | a zoom there, held until the next action, the next zoom or 8 s |
| `terminal open [command]` / `terminal close` | a real terminal, themed like the app, tiled on the right at 38% of the width; the chart reflows beside it. Its shell runs this studio's `omacharts` and `vp-agent`; a command given starts in it at once, so nothing is typed just to begin the scene. The composer covers the snap with a light sweep and labels the two panes while it is open |
| `focus app` / `focus terminal` | which window the keys go to |

Several actions on one cue go on one line, separated by `;`. Positions are
percentages of the window, so they survive a change of resolution; at the
`script` stage they are estimates, and the storyboard pins them. `after: <s>`
holds the picture once the scene's narration ends.

Use `cli` in setup, and keys and the pointer for anything the viewer should
see a person do: a chart that silently changes symbol teaches nothing.

## Notes

The author steers by writing in the file as well as in chat. A line starting
`> note:` belongs to the section it sits in:

```markdown
### 2. Measure it

> note: too wordy, and say what the bars count means
```

Applying notes means making the change and deleting the note line. A note
that cannot be applied as written stays, with a `> reply:` line under it
saying why, so the question goes back to them in the file they are reading.
