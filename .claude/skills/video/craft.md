# What makes a screencast good

General craft, distilled from people who make these well. The Taste rules in
`SKILL.md` override any of it.

## Story

- Open on the payoff. The first five seconds after the cover show something
  working — no logo sting beyond the cover, no "in this video I'll show
  you".
- One idea per video, shown as a task with an outcome, not a tour of
  features. A feature video runs 60–90 seconds; a walkthrough 2–4 minutes.
- End on one next step (the install command, the `?` key), then stop.

## Narration

- Say what the picture cannot: why and what for. Never narrate the clicks
  ("now I click the button") — the screen already shows that.
- Write for the ear: short sentences, contractions, one idea each. If a
  sentence needs a breath in the middle, split it.
- TTS mispronounces product words. Spell them in the narration the way they
  should sound, and keep the script's pronunciation list for words that come
  up again (Omarchy, VWAP, NVDA).

## Timing

- An action lands with its word or just after it, never before. Once a
  result appears, hold it for half a second to a second before the voice
  moves on; the eye arrives after the ear.
- Leave a short breath between ideas; cut every wait. Omacharts is fast, so
  let the video be fast where the app is — that is the point.

## Camera

- Zoom only to save the viewer searching: 1.5–2.2× for an area, more only
  for small text, held 2–4 seconds, eased in and out, never overlapping.
  Pull back to the full frame when moving to a different area, so the viewer
  keeps their bearings. Zooming on every click is worse than none.
- The cursor moves slowly on gently curved, eased paths and rests when it is
  not needed, clear of what is being read. In keyboard sections, hide it.
  Default size; no trails, halos or click sparkles.
- Show a key badge for every chord, long enough to read (a second at least).
  Omacharts is keyboard first, and the badge is how the viewer learns the
  key.
- No transitions beyond a cut or a short crossfade.

## The screen

- A believable, tidy workspace: realistic watchlists, nothing personal, no
  notifications, no debug output. The same data every render.
- Check readability at 1080p, not only 4K — most people watch smaller.
- Theme changes are moments in the story, not decoration.

## Sound

- Voice first. Music sits 18–24 dB under the voice while it speaks, rises in
  the gaps and at the ends, and fades rather than cuts. Master to −14 LUFS
  integrated with true peak under −1 dBTP, which is what YouTube normalises
  to.
- Instrumental only, steady, nothing busy in the voice's range.

## Avoid

- Feature dumps; long intros; recaps of what was just shown.
- Motion for its own sake: spins, whooshes, constant zoom, cursor effects.
- Tells of a robot: a cursor that teleports or moves in straight lines at
  constant speed, typing with every key exactly evenly spaced, actions at
  metronomic intervals. Type at human speed with a little jitter.
- Burned-in captions that cover the UI; ship an `.srt` instead.
