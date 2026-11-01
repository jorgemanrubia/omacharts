# The studio

How a video is recorded without anyone in front of a screen, and without
touching the desktop the author is using: a Hyprland of its own, nested in
theirs but
drawing to a virtual 4K output nothing displays. Proven on 2026-11-01 with
Hyprland 0.56.2.

## Pipeline

```
script.md ──► voice ──► record ──► compose ──► review ──► final.mp4 + .srt
              (TTS +     (studio,    (ffmpeg:     (a frame at
              word        real       mix, duck,   every cue,
              timings)    time)      master)      checked
                                                  before anyone)
```

1. **Voice.** Each scene's narration goes to TTS, which returns audio and
   word timings. Cached by a hash of text + voice + settings, so editing
   scene 4 only re-voices scene 4.
2. **Record.** The studio comes up, runs the setup off camera, starts the
   recorder, and plays the timeline: every action fires at the moment the
   narration reaches its cue word. The narration clock drives the actions,
   so picture and voice are in sync by construction, and `events.json`
   records when each action actually landed and where on screen.
3. **Compose.** ffmpeg today: the recording cut at the sync point, the
   voice placed scene by scene, music ducked under it, the mix mastered,
   a 4K H.264 file and a 1080p one, and subtitles from the aligned words.
   **Not built yet:** the zooms, key badges and title cards in
   [craft.md](craft.md). `events.json` records every action's time and
   position for them; the plan is a Remotion composition that reads it,
   with the 4K recording converted first to an intermediate headless Chrome
   decodes reliably.
4. **Review.** `review/` holds a frame just after every cue; look at
   them, and fix what is wrong before the author sees anything.

## Bringing it up

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
  its app id on the bus, so on the desktop's own bus a studio launch would
  just raise the real window.
- Its own `HOME` and `XDG_DATA_HOME`. The theme is read from
  `$HOME/.config/omarchy/current/theme`, so a studio home with its own copy
  lets a video change theme without changing the desktop's. Data comes from a
  snapshot of `$XDG_DATA_HOME/omacharts`, so charts look the same render
  after render (the last bar still moves with the live feed).

## Driving it

`omacharts` CLI commands for anything the app can do (the surface is
`omacharts surface --json`), `wtype` for keys, `bin/virtual-pointer`'s
protocol for the mouse. Lessons, each paid for:

- **A pointer device per gesture.** One device held for the whole run worked
  until the window rebuilt its panes; after a split its clicks reached
  nothing, while a fresh device's landed every time. `Hand` makes one for
  each glide, click or drag, and a separate `virtual-pointer hold` keeps the
  seat from ever having none.
- **Chords need room.** Every wtype run brings a keyboard with its own
  keymap, and a chord sent before the far end has read it arrives as the bare
  letter (which opens the symbol search). 300ms before the first key and
  120ms around modifiers are what held up while the machine was busy.
- A fresh window ignores its first chord: the studio sends two Escapes on
  start, never anything on camera.
- No Hyprland animations, and `disable_watchdog_warning`: a nested Hyprland
  started without `start-hyprland` draws a warning banner across the top of
  every frame otherwise.

## Market data

`~/.local/share/omacharts-video/studio-data/` holds `market.db` from the
first run, copied into every later one so the charts are the same each time
(the latest bar still moves with the live feed). Never `omacharts.db`: that
is the layout, and keeping it would start the next run on whatever the last
one left on screen. Delete the folder to take a fresh snapshot.

## Capturing it

wlr-screencopy works on `REC` (`grim -o REC` gives a 3840x2160 frame).
`gpu-screen-recorder` does **not** — it captures through KMS and a virtual
output has no connector. Video comes from `wf-recorder`:

    wf-recorder -o REC -r 60 -c hevc_vaapi -d /dev/dri/renderD128 -p qp=16

That held 60fps at 3840x2160 with no gaps through drags and crosshair
movement. `libx264 -p preset=ultrafast` did not keep up — it fell behind and
stretched 10s of action into 14.5s — so encode on the GPU while recording
and re-encode afterwards.

**Sync** is found, not assumed: in the pre-roll the window border turns
magenta for 0.3s, and the frame where it appears says exactly where the
timeline's zero falls in the file. Cutting at that point puts every action
on its word.

## Sound

Each scene's voice is normalised to −16 LUFS and placed at its start. Music
is looped or cut to length at −27 LUFS, faded at both ends, and ducked under
the voice by a sidechain compressor. The mix is mastered in two passes to
−14 LUFS integrated, −1 dBTP — measured at −14.1 on the first render.

## Speech

Chatterbox (MIT) is the engine for drafts; it clones a voice from a 10-second
reference when the voice file names one. Qwen3-TTS (Apache-2.0) clones too,
from a reference and its transcript. Both are 5–15× slower than real time on
this CPU and slower still when other heavy jobs share it — that is what the
per-scene cache is for. Every line is force-aligned (torchaudio's MMS_FA) to
time its words, which is how cues find their moment; it reads letters only,
so numbers belong in the narration spelled out.

## Shutting it down

Find the studio's processes by `XDG_RUNTIME_DIR=<studio dir>` in
`/proc/<pid>/environ` and kill exactly those. Never by name — the author's own
Hyprland and omacharts are running alongside. Not `pkill -f` either: its
pattern is in the shell's own command line, so it kills the shell running
the cleanup halfway through. `bin/video` turns SIGINT, SIGTERM and SIGHUP
into a clean shutdown; a background job started from a non-interactive shell
has SIGINT ignored, and Python inherits that unless it sets a handler.
