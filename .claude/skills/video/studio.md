# The studio

How a video is recorded without anyone in front of a screen, and without
touching the desktop the author is using: a Hyprland of its own, nested in
theirs but
drawing to a virtual 4K output nothing displays. Proven on 2026-11-01 with
Hyprland 0.56.2.

## Pipeline

```
script.md ──► voice ──► record ──► polish ──► compose ──► review
              (TTS +     (studio,    (Remotion:  (ffmpeg:     (a frame at
              word        real       zooms, key  cover, mix,  every cue,
              timings)    time)      badges)     master)      checked)
```

1. **Voice.** Each scene's narration goes to TTS, which returns audio and
   word timings. Cached by a hash of text + voice + settings, so editing
   scene 4 only re-voices scene 4.
2. **Record.** The studio comes up, runs the setup off camera, starts the
   recorder, and plays the timeline: every action fires at the moment the
   narration reaches its cue word. The narration clock drives the actions,
   so picture and voice are in sync by construction, and `events.json`
   records when each action actually landed and where on screen.
3. **Polish.** The recording, cut at the sync point and re-encoded to H.264
   (headless Chrome decodes it reliably; the recorder's HEVC less so), goes
   to a Remotion composition in `video/compose/` that draws the zooms and key
   badges. `bin/video` makes the plan from `events.json` (`polish_plan`):
   zoom only on a run of two or more pointer actions close together, scaled to
   fit them, eased in and out, released when the hands go to the keyboard or
   1.6s after the last click; a badge for every chord with a modifier.
   Remotion only draws what the plan says. `polish: off` in a script skips it.
4. **Compose.** ffmpeg: the cover held and faded into the polished recording,
   the voice placed scene by scene, music ducked under it, the mix mastered,
   a 4K H.264 file and a 1080p one, and subtitles from the aligned words.
5. **Review.** `review/` holds a frame just after every cue; look at
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

## The terminal

For a scene about the command line — an agent using it, say — `terminal open`
tiles a real `foot` beside the app, themed from the same colours, with a shell
whose PATH holds this studio's `omacharts` and `video/agent/vp-agent`. Typed
commands run against the app on camera, so the chart changes as they do.
Lessons:

- Hyprland 0.56's Lua dispatchers: `hl.dispatch(hl.dsp.focus({ window =
  "class:…" }))` to move focus, `hl.dispatch(hl.dsp.window.resize({ x = N,
  y = 0, relative = true }))` to move the split. Which way a relative resize
  moves the split depends on the window's side, so the studio measures and
  corrects.
- `hl.dsp.window.close()` closes whatever has the focus. A `terminal close`
  with no terminal once closed the app: close only after checking the
  terminal is there and has the focus.
- An agent's own run is not the same twice and cannot be timed to a voice,
  so `vp-agent` plays one: its sentences are written, every command it shows
  is the real one, run live.

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
- **Shift over a letter goes through `bin/virtual-keyboard`.** wtype's
  keymap holds only the keys it types, one level each, and GTK then
  matches no accelerator with Shift and a letter in it: Ctrl+Shift+H and
  Ctrl+Alt+Shift+H did nothing in the studio (Ctrl+Shift+I only worked
  because the app catches it by hand). `virtual-keyboard` uploads the US
  keymap a real keyboard has and presses the modifiers as keys; `key` uses
  it for any chord with Shift and a letter, wtype for the rest.
- A chartbook's sidebar is its own (`sidebar_shown`, not scriptable), and a
  new one opens with it shown: hide it with Ctrl+Shift+B once on each, after the
  switch has settled. A chartbook switched to comes back at the zoom it had
  but on its latest bar, and very wide views are cut down, so a span that
  must be on screen is set after the last switch to that chartbook.
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

## Cover

`bin/video cover` renders `video/cover/cover.html` in headless Chromium at
3840x2160 over the site's own `bg-ribbon.jpg`, mark and Cascadia Mono, with
Archivo from Google Fonts (so it needs the network, like the OG cards).
`compose` holds it for `cover:` seconds (2.5 by default) and crossfades
into the recording over 0.6s — the recording's lead-in, where nothing
happens yet — and shifts the voice, subtitles and review frames by the
same amount. The first word comes `SETTLE` (1s) after the fade ends.

## Sound

Each scene's voice is normalised to −16 LUFS into a file of its own, then
placed at its start. In one ffmpeg graph, `loudnorm` ahead of `adelay`
silently cancels the delay and every line starts at zero — renders shipped
like that until the author heard the voice under the cover. Check the voice
track is silent where it should be before handing a render over. The
voices are each padded to the picture with `apad=whole_dur=` before
`amix`: padding after `amix` stops at the last word, cutting the closing
music, and a bare `apad` there never ends — one render spun in it for six
hours.
Music is cut to length at −27 LUFS, faded at both ends, and ducked under
the voice by a sidechain compressor. A track that dies away a little before
the picture ends (up to a tenth) is slowed with `atempo` so its last note
lands on the end; looping it, or letting it run out, left the closing lines
over silence in the Volume profile video. Only a much shorter one loops. The mix is mastered in two passes to
−14 LUFS integrated, −1 dBTP — measured at −14.1 on the first render.

## Speech

The narrator is Qwen3-TTS (Apache-2.0) cloning `video/voices/narrator.wav`
from that clip and its transcript — the same model family that designed the
voice, which keeps its character best. Chatterbox (MIT) is the fallback, and
`draft` its built-in voice. Newer torchaudio needs torchcodec to read audio,
so the aligner reads with soundfile. Both are 5–15× slower than real time on
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
