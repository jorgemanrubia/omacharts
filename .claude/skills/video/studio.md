# The studio

How a video is recorded without anyone in front of a screen, and without
touching the desktop the author is using: a Hyprland of its own, nested in
theirs but
drawing to a virtual 4K output nothing displays. Proven on 2026-11-01 with
Hyprland 0.56.2.

## Pipeline

```
script.md ──► voice ──► record ──► compose ──► review ──► final.mp4 + .srt
              (TTS +     (studio,    (Remotion:   (frames at
              word        real       zooms, key   every cue,
              timings)    time)      badges,      checked
                                     music, VO)   before anyone)
```

1. **Voice.** Each scene's narration goes to TTS, which returns audio and
   word timings. Cached by a hash of text + voice + settings, so editing
   scene 4 only re-voices scene 4.
2. **Record.** The studio comes up, runs the setup off camera, starts the
   recorder, and plays the timeline: every action fires at the moment the
   narration reaches its cue word. The narration clock drives the actions,
   so picture and voice are in sync by construction, and `events.json`
   records when each action actually landed and where on screen.
3. **Compose.** The recording, the voice, the music and `events.json` go
   into a Remotion composition: zooms to where the action is, key badges,
   title and end cards, music ducked under the voice, the mix normalised to
   −14 LUFS. Remotion renders through headless Chrome, so convert the HEVC
   recording to an intermediate it decodes reliably first.
4. **Review.** Pull a frame at every cue and at every zoom's peak, look at
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
`omacharts surface --json`), `wtype` for keys, `bin/virtual-pointer` for the
mouse — all three confirmed reaching the studio window, with the cursor
drawn on the output. A fresh window ignores its first chord (see
`bin/headless-window`): send a harmless key during setup, never as the first
thing on camera.

## Capturing it

wlr-screencopy works on `REC` (`grim -o REC` gives a 3840x2160 frame).
`gpu-screen-recorder` does **not** — it captures through KMS and a virtual
output has no connector. Video comes from `wf-recorder`:

    wf-recorder -o REC -r 60 -c hevc_vaapi -d /dev/dri/renderD128 -p qp=18

That held 60fps at 3840x2160 with no gaps through drags and crosshair
movement (593 frames in 9.87s). `libx264 -p preset=ultrafast` did not keep
up — it fell behind and stretched 10s of action into 14.5s — so encode on the
GPU while recording and re-encode afterwards.

## Shutting it down

Find the studio's processes by `XDG_RUNTIME_DIR=<studio dir>` in
`/proc/<pid>/environ` and kill exactly those. Never by name — the desktop's own
Hyprland and omacharts are running. Not `pkill -f` either: its pattern is in
the shell's own command line, so it kills the shell running the cleanup
halfway through.
