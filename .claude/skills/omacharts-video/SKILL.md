---
name: omacharts-video
description: Make narrated screencast videos of Omacharts — write or change a video script, render it (voice, music, recording, composition), review the result, and fold Jorge's feedback back into this skill. Use whenever a video, screencast, demo, trailer, voice-over or YouTube clip of Omacharts is mentioned, when a script under video/scripts/ is touched, or when Jorge comments on a rendered video ("the voice is too fast", "zoom in on the ruler", "less music").
---

# Making Omacharts videos

Videos are rendered, not recorded. Jorge iterates on a script and watches the
result; nobody sits in front of a screen recorder, and nobody touches the
desktop he is using. Everything below exists to keep that true.

## This skill learns

**When Jorge gives feedback on a video, update this file in the same turn as
the fix.** Fix the script or the pipeline for the video in hand, then ask what
the note says about *every* video and write that down under
[Taste](#taste) or [Pitfalls](#pitfalls) — as a rule, with the reason, not as
a changelog entry. "Scene 3 was too fast" is a fix; "leave a beat of at least
0.6s after an action lands before the voice moves on, because the eye arrives
after the ear" is the lesson. If a note contradicts something already here,
replace the old rule rather than adding a second one. If a note is only about
that one video, it goes in the script, not here.

Say in your reply which rule you added or changed, so he can veto it.

## The pipeline

```
script.yml ──► voice ──► record ──► compose ──► review ──► final.mp4 + .srt
               (TTS +     (studio:    (Remotion:   (frames at
               word        nested      zooms, key   every cue,
               timings)    Hyprland,   badges,      checked
                           real time)  music, VO)   before Jorge)
```

1. **Voice.** Each scene's narration goes to TTS, which returns audio and word
   timings. Cached by a hash of text + voice + settings, so editing scene 4
   only re-voices scene 4.
2. **Record.** The studio comes up, runs `setup` off camera, starts the
   recorder, and plays the timeline: every action fires at the moment the
   narration reaches its cue word. The narration clock drives the actions,
   so picture and voice are in sync by construction, and `events.json`
   records when each action actually landed and where on screen.
3. **Compose.** The raw recording, the voice, the music and `events.json`
   go into a Remotion composition: zoom to where the action is, key badges
   for every chord pressed, title and end cards, captions, music ducked
   under the voice and the whole mix normalised to −14 LUFS for YouTube.
4. **Review.** Before handing anything over, pull a frame at every cue and
   at every zoom's peak, look at them, and fix what is wrong. Jorge's time
   goes on taste, not on spotting a key that did not land.

## The studio

A Hyprland of its own, nested in Jorge's but drawing to a virtual 4K output
nothing displays. Proven on 2026-11-01 with Hyprland 0.56.2:

- `Hyprland -c studio.lua` with `WAYLAND_DISPLAY` pointing at the parent's
  socket by absolute path and `XDG_RUNTIME_DIR` set to a private directory.
  That directory must be **short** — under `/run/user/$UID/` — because a
  Wayland socket path is capped at 108 bytes and the scratchpad path is
  longer than that; Hyprland aborts with "m_szWLDisplaySocket was null".
- `hyprctl output create headless REC`, with a monitor rule for `REC` at
  `3840x2160@60`, scale 2, already in the config so it comes up right.
- Then disable the nested window's own output
  (`hyprctl eval 'hl.monitor({ output = "WAYLAND-1", disabled = true })'`)
  so every window lands on `REC`. Hyprland 0.56 takes Lua through
  `hyprctl eval`; the old `dispatch`/`keyword` string syntax is gone.
- Its own D-Bus session. omacharts hands a launch to whatever already owns
  its app id on the bus, so on Jorge's bus a studio launch would just raise
  his real window.
- Its own `HOME` and `XDG_DATA_HOME`. The theme is read from
  `$HOME/.config/omarchy/current/theme`, so a studio home with its own copy
  lets a video change theme without changing his desktop. Data comes from
  a snapshot of `$XDG_DATA_HOME/omacharts`, so charts look the same render
  after render (the last bar still moves with the live feed).
- Input: `omacharts` CLI commands for anything the app can do (the surface
  is `omacharts surface --json`), `wtype` for keys, `bin/virtual-pointer`
  for the mouse — all three confirmed reaching the studio window, with the
  cursor drawn on the output.
- Capture: wlr-screencopy works on `REC` (`grim -o REC` gives a 3840x2160
  frame). `gpu-screen-recorder` does **not** — it captures through KMS and a
  virtual output has no connector. Video comes from `wf-recorder`:
  `wf-recorder -o REC -r 60 -c hevc_vaapi -d /dev/dri/renderD128 -p qp=18`
  held 60fps at 3840x2160 with no gaps through drags and crosshair
  movement (593 frames in 9.87s). `libx264 -p preset=ultrafast` did not
  keep up — it fell behind and stretched 10s of action into 14.5s — so
  encode on the GPU while recording and do any re-encoding afterwards.
- Shutting down: find the studio's processes by
  `XDG_RUNTIME_DIR=<studio dir>` in `/proc/<pid>/environ` and kill exactly
  those. Never by name — Jorge's own Hyprland and omacharts are running.
  Not `pkill -f` either: its pattern is in the shell's own command line,
  so it kills the shell running the cleanup halfway through.

## Scripts

One file per video, `video/scripts/<name>.yml`. Narration is the spine; cues
in braces mark where an action fires.

```yaml
title: Split, link, measure
theme: tokyo-night
voice: default
music: { track: video/music/<file>, level: -22 }
setup:                      # off camera, before recording starts
  - cli: chartbook create Demo --symbol NVDA
  - cli: chartbook switch Demo
  - cli: chart set --resolution 1D
scenes:
  - say: |
      Omacharts is keyboard first. {split}Split the chart with one key,
      {sym}and give the new one a symbol of its own.
    at:
      split: { key: ctrl+alt+h }
      sym: [ { type: AAPL }, { key: Return } ]
    after: 1.0               # seconds held once the voice ends
```

Actions: `cli`, `key`, `type`, `move` (eased, with a duration), `click`,
`drag`, `scroll`, `wait`, `zoom` (a hint to the composer when the automatic
zoom picks the wrong spot), `theme`.

Prefer `cli` for setup and `key`/pointer for what the viewer should see a
person doing: a chart that silently changes symbol teaches nothing.

## Taste

Rules Jorge has set, each from a note on a real video. None yet.

## Pitfalls

Things that broke, and why.

- A fresh window ignores its first chord (see `bin/headless-window`): send a
  harmless key during `setup`, never as the first thing on camera.
