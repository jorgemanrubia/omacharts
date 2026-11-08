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

## Timed to music

A video with no voice, cut to its music, says `timing: music` in its
frontmatter. Its scenes have a **Picture** and no **Voice**, and every cue
is a place in the music: `@bar.beat`, counted from the track's first
downbeat (`@9` or `@9.1` is bar 9's downbeat, `@9.3` its third beat,
`@9.2.5` the off-beat after the second). Several actions on one cue go on
one line, as elsewhere.

    @15.1: key ctrl+alt+Right; theme hackerman; pulse
    @19.1: zoom 25% 30% 1.9

| Frontmatter | Means |
|---|---|
| `music_tempo` | the BPM `bin/video music` asks the composer for |
| `music_bpm`, `music_downbeat` | the pinned track's tempo and first downbeat, written by `bin/video beats` |
| `music_meter` | beats in a bar, 4 unless set |
| `music_end` | the `bar.beat` the picture ends on |
| `music_fade` | seconds the music fades over at the end, 1.5 unless set |
| `cover` | seconds the cover is held; the music plays under it from 0 |
| `badges` | `on` to show key badges, off unless set |
| `end_card` | seconds an end card is held after the picture (`music_end`), the music finishing under it; none unless set |
| `end_title` | the end card's line under the wordmark, "Beautiful charting software" unless set |

And four actions for the camera and the beat, drawn afterwards:

| Action | Means |
|---|---|
| `theme <name>` | the desktop switches to that Omarchy theme, and the window with it. At most one a bar |
| `zoom <x%> <y%> <scale> [beats]` | the camera snaps there, settling within a beat, and holds: for `beats`, then back, or until the next move. `zoom 50% 50% 1` is the whole frame |
| `drift <x%> <y%> <scale> <beats>` | the camera glides there over that many beats |
| `pulse [strength]` | the picture kicks closer for an instant on a drum (0.035 unless set) |
| `flash [strength]` | a brief lift toward white, under a cut (0.35 unless set) |
| `early <seconds>` | fire the cue's steps that much before its beat, for a slow cue; it is shown finished on its beat |
| `cursor show` / `cursor hide` | the pointer drawn or not; hidden from the start in a video timed to music, and the border round the window goes |

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
