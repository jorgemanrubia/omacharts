---
title: Market profile
stage: script
length: 225s
theme: tokyo-night
voice: narrator
music: ambient electronic, soft analog pads and a gentle plucked arpeggio over a light, even pulse that steps like the half hours a profile counts, a note added at a time as the letters stack; curious and a little playful in the colour-cycling opening, unhurried and clear through the reading of real sessions, warmer and fuller once the market and volume profiles meet back to back, resolving bright and calm for the close
---

## Brief

**Pitch.** A market profile shows where the market spent its time, a
letter for every half hour at every price it visited. It's the simplest
way to watch value being transferred: where the market accepted a price,
and where it turned one away. In Omacharts it's a study, it reads as well
as it looks, and it pairs with a volume profile back to back.

**For.** Traders who know candles, maybe volume profile, but have never
read a market profile; and Omacharts users who haven't turned the new
study on. No auction theory assumed.

**Payoff.** Straight after the cover: one real ES session's market
profile, zoomed in until the letters fill the screen, bars hidden. While
the first lines play, the profile changes colour three or four times,
each colour held about four seconds, so the letters and their shape are
the first thing the viewer sees. The first sentence says the idea that
ties the video together: a market profile shows where the market spent
its time, and that's where it agreed on value.

**Beats.** Each section opens with a sentence or two over its chart,
with nothing moving, saying what it is about.
1. **What it's for.** Over the colour-cycling profile: a market profile
   shows how value is transferred, where the market accepted a price
   over time. It's close to a volume profile, but different: a volume
   profile shows the volume traded at each price, a market profile
   counts time, one letter for each half hour the price was there, A for
   the first, B for the next. Same shape idea, a different question.
   Time is simpler and less noisy, so it reads market structure clearly.
2. **Reading real sessions.** A few real ES (or SPY) sessions, letters
   readable, one zoom per feature, each zoom framed to fit the whole
   feature: the point of control (the price with the most letters) and
   the value area around it; excess, a tail of single letters at an
   extreme where the auction was rejected and left quickly; and a poor
   low, a flat, unfinished bottom with several letters at the same last
   price, a level the market tends to come back to. The sessions are
   picked from the data at the script stage; the narration names only
   what the chart shows.
3. **Add it.** On a fresh chart, the way a person does it: Ctrl and
   Shift and I, Market Profile. Profiles appear on every session. Zoom
   in until the letters show; hide the bars with Ctrl and Alt and Shift
   and H, so only the profiles are left; zoom back out.
4. **Make it yours.** A short loop through the profile's settings
   dialog, one change at a time with a beat to see each: Value area
   percent, the point of control's colour and line, Outline, Display as
   Letters or Blocks, and Open and close, the small arrows marking where
   each session opened and closed. Esc after, so nothing stays selected.
5. **Zoom out.** As the sessions shrink, letters too small to read turn
   into blocks, and the shapes still read. Zoom further, to a couple of
   weeks of sessions side by side: balance days, trend days, the profile
   telling them apart at a glance.
6. **With a volume profile.** Add a volume profile with the same Reset
   every, Session. Set its Alignment to Right, keep the market profile
   on Left: each session's two now grow back to back from a shared spine,
   volume to the left and time to the right, so where they agree and
   where they don't is plain. The open and close arrows still mark each
   session's start and end.
7. **Close.** One line on what the profile gives you, then where to
   read more: the Market Profile page of the manual.

**What the source changes.**
- The intro colour cycling is a highlight, not a lesson, so it's done
  off camera (`cli chart indicator set market_profile` with a new
  colour) with the chart untouched otherwise and nothing selected. If
  the author would rather see the settings dialog do it, beat 4 already
  shows that, so the intro stays clean.
- Look of the whole video: Quiet bar colouring, no grid, the watchlist
  sidebar (Ctrl+Shift+B) and the drawing tools (Ctrl+Shift+D) closed in
  setup.
- Zooms: every camera zoom is sized to hold the whole profile or
  feature it points at, with margin, not just its middle; the storyboard
  checks each one, since a zoom that looks good can still clip an edge.
  The chart's own zooms (the wheel) are the same: each stops with the
  sessions it is about fully on screen.
- On-screen text: anything shown as text, key badges included, holds at
  least four seconds, so nothing flashes past.
- Real data: ES sessions with a clear excess tail and a clear poor low
  are picked from the data at the script stage (SPY if ES has no clean
  example); the regular session only, five-minute bars, thirty-minute
  TPO periods. The narration quotes no prices or counts; the storyboard
  reads them off the studio.
- Expanded profiles (TPOs laid out over their bars) and composites are
  left out to keep this one idea. They'd make a short second video:
  "Market profile over time", expanding a session and keeping the
  forming one expanded as it grows.
- No trading and no accounts on screen.
- Length: about three minutes (180s), roughly 380 words of narration at
  a relaxed pace, with holds for the colour cycling, the zooms and the
  settings loop.

## Setup

Three chartbooks, one per stretch of the video, in strip order: Profiles
(the opening: ES five-minute, regular session, a market profile, bars
hidden, October 12 and 13), Sessions (the reading: the same, October 9 to
13) and Study (ES five-minute, regular session, bars only, no study yet).
Every chart has Quiet colouring and no grid; every chartbook's sidebar is
off, and so are the drawing tools. New charts open on no chart template,
so nothing leaks into the Default template. A custom range set with
`chart range` survives a switch away and back (checked on the studio with
0.3.11), but one set before the chartbook's data has arrived is lost to the
latest bars; so every chartbook is visited again at the end, once its data
is in, and its range set there.

    cli template default none
    cli chart drawing-tools off
    cli chartbook rename "Chartbook 1" Profiles
    cli chart set --symbol ES --template none --session regular --colouring theme-quiet --grid off --bars off
    cli chart indicator add market_profile
    cli chartbook sidebar off
    cli chartbook create Sessions --symbol ES --switch
    cli chart set --session regular --colouring theme-quiet --grid off --bars off
    cli chart indicator add market_profile
    cli chartbook sidebar off
    cli chart range "oct 9:oct 13" --resolution 5m
    cli chartbook create Study --symbol ES --switch
    cli chart set --session regular --colouring theme-quiet --grid off
    cli chartbook sidebar off
    cli chart range "oct 16:oct 29" --resolution 5m
    wait 3.0
    cli chartbook switch Sessions
    wait 2.0
    cli chart range "oct 9:oct 13" --resolution 5m
    wait 1.0
    cli chartbook switch Study
    wait 2.0
    cli chart range "oct 16:oct 29" --resolution 5m
    wait 1.0
    cli chartbook switch Profiles
    wait 2.0
    cli chart range "oct 12:oct 13" --resolution 5m
    move 40% 99.3%
    wait 1.0

## Script

### 1. Where the market spent its time

**Picture.** ES on five-minute bars, the bars hidden: two big market
profiles of letters fill the screen, October 12 at the left edge (x ≈
2–18%) and October 13 in the middle (x ≈ 49–67%), each with its orange
point of control line and its brighter value area. Nothing is selected and
the cursor is off the chart. The profiles start Blue; on cue they turn
Violet, then Teal, then Rose, and end back on Blue, each colour held at
least four seconds. Nothing else moves.

**Voice.** A market profile shows where the market spent its time.
{violet}Read it by its shape: wide where the market traded for hours,
narrow where it passed straight through. {teal}The wide part is where it
accepted the price as value. {rose}Where it thins out, it turned the price
away and moved on.
{blue}It's a simple way to see value change hands.

    violet: cli chart indicator set market_profile --color Violet
    teal:   cli chart indicator set market_profile --color Teal
    rose:   cli chart indicator set market_profile --color Rose
    blue:   cli chart indicator set market_profile --color Blue
    after: 1.0

### 2. Time, not volume

**Picture.** The same two Blue profiles, held. Nothing moves.

**Voice.** It looks like a volume profile, but it asks a different
question. A volume profile shows the volume traded at each price. This one
counts time, in letters called TPOs. Time is simpler and less noisy than
volume. That makes the profile a clear way to read market structure.

    after: 0.6

### 3. Real sessions

**Picture.** Ctrl and Alt and right arrow: the Sessions chartbook, ES on
five-minute bars, bars hidden, three regular sessions: October 9's profile
at the left edge (x ≈ 2–12%, tall, y ≈ 8–92%), October 12's at x ≈ 33–45%
(y ≈ 8–55%) and October 13's at x ≈ 65–77% (y ≈ 22–78%). Letters readable.
Nothing else moves.

**Voice.** {next}Here are three real ES sessions, regular hours. Their
shapes say how each one traded.

    next: key ctrl+alt+Right
    after: 0.6

### 4. Point of control and value area

**Picture.** The same chart, held. The orange line across each profile is
its point of control; the letters around it are drawn brighter, the rows
outside the value area dimmer. Nothing moves.

**Voice.** The orange line is the point of control, where the market spent
longest. The brighter letters around it are the value area, about seventy
percent of the time.

    after: 0.6

### 5. Excess

**Picture.** A camera frame on the bottom of October 9's profile: the three
rows of single A prints at its very bottom (x ≈ 2–4%, y ≈ 85–92%), a
buying tail, with the AB rows above them, all inside the frame with margin.
Held through the scene; the cursor stays off the chart, so nothing lets
the frame go early.

**Voice.** {tail}At the bottom of the first session, the profile ends in a
thin tail. Price touched those lows early and never came back.
That's excess: buyers turned those prices away fast.

    tail: frame 0% 55% 30% 98%
    after: 0.8

### 6. A poor low

**Picture.** The camera comes off October 9 and frames the bottom of
October 12's profile: its lowest row has only K and L, the row above J, K
and L, a flat, blunt bottom with no tail (x ≈ 33–37%, y ≈ 50–55%), framed
with some of the profile above it for context.

**Voice.** {low}The next session ends flat instead: a blunt bottom, and no
tail. That's a poor low. It looks unfinished,
and markets tend to come back to one.

    low: frame 27% 22% 55% 62%
    after: 0.6

### 7. Coming back

**Picture.** The frame widens to hold both October 12's flat low and
October 13's profile to its right, whose D and E letters reach below the
level of October 12's low (October 13's lower letters reach y ≈ 77%). Held
until it eases back to the full chart.

**Voice.** {next}The next day did, trading right back through that low.

    next: frame 29% 36% 80% 84%
    after: 3.0

### 8. On a chart of your own

**Picture.** Ctrl and Alt and right arrow: the Study chartbook, ES on
five-minute bars, regular sessions from October 16 to 29, candles only in
the Quiet colouring, nothing on the chart. Nothing else moves.

**Voice.** {next}Adding one is quick. This is ES on five-minute bars,
regular sessions only.

    next: key ctrl+alt+Right
    after: 0.6

### 9. Add it

**Picture.** Ctrl and Shift and I opens **Add an indicator**; "market" is
typed and the list shows **Market Profile** with its TPO badge; Enter opens
its settings page, Cancel and Add at the bottom, the chart previewing
behind it, dimmed. Ctrl and Enter adds it, Esc deselects it: a profile at
the start of every session, drawn as blocks at this size.

**Voice.** {add}Control, Shift and I, {pick}Market Profile, {done}and every
session gets one, drawn as blocks at this size.

    add:  key ctrl+shift+i
    pick: type market; wait 0.4; key Return
    done: key ctrl+Return; wait 0.6; key Escape
    after: 1.0

### 10. Zoom in, hide the bars

**Picture.** The cursor rests near the right of the plot, clear of the
price axis (the wheel over the axis scales price instead), and the wheel
zooms time in about it, twelve notches, until two sessions fill the
screen, October 28 and 29, each profile fully on screen top to bottom and
the blocks turned into letters. (October 27's profile, its session
starting off the left edge, is squeezed to a sliver against it.) Ctrl and Alt and Shift and H, its badge on
screen: the candles disappear and only the letter profiles are left. The
cursor goes off the chart.

**Voice.** {zoom}Zoom in, and the letters show. {hide}Control, Alt, Shift
and H hides the bars, leaving only the profiles.

    zoom: scroll 94% 50% -12
    hide: key ctrl+alt+shift+h; wait 0.4; move 40% 99.3%
    after: 1.2

### 11. Make it yours

**Picture.** The three letter profiles, bars hidden, held. Nothing moves.

**Voice.** The look is yours to set, and the chart follows as you change
it.

    after: 0.5

### 12. Outline

**Picture.** The cursor right-clicks October 28's profile, on the left, on its letters; its
menu opens, the Reset every buttons along the top and **Properties…** at
the bottom, which it clicks. The settings dialog opens in the middle of the
window, the chart dimmed behind it. A click on the **Outline** switch in
the Style group: a stepped edge appears round every profile's letters,
behind the dialog. Esc closes the dialog, and Esc again deselects the
study (it takes two more, the first lands while the dialog is still going), and the chart holds with the outlines on. The cursor goes off the
chart.

**Voice.** {menu}Its properties are a right-click away. An outline gives
each profile a stepped edge, {close}so the shapes stand out.

    menu:  keep 9% 28% 31% 55%; rclick 10% 30%; wait 0.6; click 13% 50.7%; wait 0.8; keep 35% 8% 65% 92%; click 61.5% 83%
    close: key Escape; wait 0.6; key Escape; wait 0.3; key Escape; move 40% 99.3%
    after: 1.5

### 13. A heavier point of control

**Picture.** Right-click October 28's profile again, **Properties…**; the
wheel runs the dialog down to its **Point of control** group, and the
Thickness **+** is clicked twice: the orange lines thicken across every
profile behind the dialog. Esc, Esc, and the chart holds with the heavier
lines. The cursor goes off the chart.

**Voice.** {menu}The point of control can be thicker too, which makes the
busiest price easy to spot, {close}even zoomed out.

    menu:  keep 9% 28% 31% 55%; rclick 10% 30%; wait 0.6; click 13% 50.7%; wait 0.8; keep 35% 8% 65% 92%; scroll 50% 60% 15; wait 0.6; click 61.9% 70.8%; wait 0.4; click 61.9% 70.8%
    close: key Escape; wait 0.6; key Escape; wait 0.3; key Escape; move 40% 99.3%
    after: 1.5

### 14. Open and close

**Picture.** A camera frame on the top of October 28's profile, so its
two small arrows read: the open's beside the first letter in the row of
the opening price, pointing in at it, and the close's just past the last
letter in the row of the closing price, pointing back at it. (The
storyboard may tighten the frame to the rows the two arrows are on.) The
cursor stays off the chart.

**Voice.** {arrows}The two small arrows mark each session's open and close.
The open points in at the first letter, and the close back at the last.

    arrows: frame 2% 10% 30% 45%
    after: 1.0

### 15. Zoom out

**Picture.** The three outlined letter profiles, bars hidden, held. Nothing
moves.

**Voice.** Zoomed out, a profile still shows how its day went.

    after: 0.5

### 16. Blocks, then weeks

**Picture.** The cursor rests near the right of the plot and the wheel
zooms time out, nine notches: as the sessions shrink to about seven on
screen, the letters turn into
blocks, and the outlined shapes still read. Held a few seconds. Then six
more notches, to three weeks of sessions side by side, October 9 to 29, each a
small block profile with its point of control line. Every profile on
screen fits top to bottom. Held, the cursor off the chart.

**Voice.** {out1}Smaller, the letters turn into blocks, and the shapes
still read. {out2}Further out, weeks of them side by side: balanced days
short and wide, trending days tall and thin.

    out1: scroll 94% 50% 9
    out2: scroll 94% 50% 6; wait 0.4; move 40% 99.3%
    after: 1.5

### 17. With a volume profile

**Picture.** The zoomed-out chart from scene 16, held. Nothing moves.

**Voice.** It pairs well with a volume profile: time on one side, volume
on the other.

    after: 0.5

### 18. Add the volume profile

**Picture.** Ctrl and Shift and I, "volume profile" typed, Enter: the
Volume Profile settings page, the chart previewing behind it. A click
in the Style group opens the **Alignment**
drop-down and a click picks **Right**. Ctrl and Enter adds it, Esc
deselects it: every session now has a volume profile and its market profile
back to back. Its Reset every is Session, the market profile's too, so they
pair.

**Voice.** {add}Add a Volume Profile {pick}the same way, and in its
settings, {align}set the alignment to the right, so it grows the other
way. {done}The market profile stays on the left.

    add:   key ctrl+shift+i
    pick:  type volume profile; wait 0.4; key Return
    align: keep 35% 8% 65% 92%; click 61% 62.7%; wait 0.6; click 61% 70.2%
    done:  key ctrl+Return; wait 0.6; key Escape; move 40% 99.3%
    after: 0.8

### 19. Back to back

**Picture.** The wheel zooms time in about late October fifteen notches,
until October 28 and 29 fill the screen again, each fully on screen top to bottom: in
each, the volume profile's rows grow left and the market profile's letters
grow right from a spine they share, the pair starting at the session's
open. The cursor goes off the chart. On "arrows", a camera frame on
October 29's pair, top to bottom, with its open and close arrows beside the
letters.

**Voice.** {zoom}Zoom in, and each session's two sit back to back: volume
to the left, time to the right. Where they agree, the market traded
heavily and stayed. {arrows}The arrows still mark each session's open and
close.

    zoom:   scroll 94% 50% -15; wait 0.4; move 40% 99.3%
    arrows: frame 55% 36% 84% 92%
    after: 1.5

### 20. Close

**Picture.** The back to back pairs on the full chart, held. The cursor
rests off the chart.

**Voice.** Where the market spends its time is where it finds value. The
Market Profile page of the manual has the rest.

    after: 2.5

## Pronunciation

- ES: E S
- TPOs: T P Os
- TPO: T P O
- Omacharts: Oma-charts

## Storyboard

![Storyboard](../storyboards/market-profile.jpg)
