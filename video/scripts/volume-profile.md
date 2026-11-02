---
title: Volume profile
stage: script
length: 230s
theme: tokyo-night
voice: narrator
music: ambient electronic, warm analog pads and a soft plucked arpeggio over a slow, deliberate pulse; patient and a little curious through the theory, steadier once the study is on screen, and in the agent section the pulse firms up and a second arpeggio layer enters as the profiles fold together; it opens up for the theme changes near the end, with room to breathe, and resolves brighter for the close
---

## Brief

**Pitch.** A volume profile shows where a market actually traded, price
by price. It is the basic instrument of auction market theory, and in
Omacharts it is a drawing, a study, and composites you can build by hand
or hand to an agent.

**For.** Traders who have heard of volume profile, value area and point
of control but never used them, and Omacharts users who haven't turned the
study on yet. No prior auction theory assumed.

**Payoff.** Straight after the cover: SPY on four-hour bars with the bars
hidden, a monthly profile under every month, and in a few quick seconds
the months fold together into four wide composites, each with one point
of control and one value area. No terminal yet; that is saved for the
end. Then the first line says the idea in plain words: where the volume
sits tells you what the market agreed was fair.

**Beats.** Each section opens with a sentence or two over its chart, with
nothing moving, saying what it is about. The first one also carries the
idea that ties the video together.
1. **The idea: an auction.** One SPY session profile, held still. Kept
   short and plain: the volume profile comes from auction market theory;
   the market is an auction, and price moves up and down to find the
   other side of the trade. In balance it trades inside value and builds
   the profile; out of balance it leaves to find new value. One zoom,
   held while the value area (the band holding about seventy percent of
   the volume) and the point of control (the busiest price) are named.
   When price leaves value, either it's rejected and comes back, or it's
   accepted and new value builds there.
2. **The drawing.** The quickest profile: exactly the bars you choose.
   On QQQ, Alt and V, drag across a rally from its low to its high, and
   the profile of just those bars appears. Pull a corner grip to stretch
   it a few bars and it's worked out again.
3. **The study.** On a fresh SPY four-hour chart, add Volume Profile
   (Ctrl and Shift and I). One profile per session, under the candles.
   Right-click a profile and step Reset every to Week, then Month. Zoom
   out to a year and a half of months and hide the bars (Ctrl and Alt and
   Shift and H): with only the profiles left, where price found balance
   is plain to see.
4. **Composites.** On that same view: neighbouring months that share
   their value are really one balance. Right-click, Merge with previous
   profile, three times: December 2025 to March 2026 becomes one profile,
   one point of control, one value area.
5. **Ask an agent.** Doing that across the whole chart is a judgement
   applied many times, which is what an agent is for. A terminal opens
   on the right; the request is typed in plain words: merge
   neighbouring profiles with value areas overlapping at least 50%. The
   agent reads the profiles (`omacharts chart profile list --on-screen
   --json`) and merges right to left (`omacharts chart profile merge --at
   …`); each merge lands on the chart as it runs, the first one with a
   paired zoom. The terminal closes, the bars come back.
6. **Any theme.** Preferences › Theme, two themes, one of them light; the
   candles, profiles and composites recolour live. Mostly music.
7. **Close.** One line on what the profile gives you, and where to read
   more: the Volume page of the manual.

**What the source changes.**
- Order: the drawing now comes straight after the theory, so the study,
  zooming out, hiding the bars, composites by hand and the agent all
  happen on one SPY chart, in the sequence the author asked for.
- The drawing key, Alt and V, is missing from the `?` shortcuts list; that
  is being fixed separately, and the close points at the manual, not `?`.
- Drawings show on every chart of their symbol, and composites belong to
  their chart, symbol and period. So the teaser has a chartbook of its own
  (SPY, the same view, folded by the CLI off the timeline's main thread),
  the theory its own SPY five-minute chart, the drawing QQQ, and the study,
  composites and agent share one SPY four-hour chart. The agent starts on
  the composite made by hand and does the rest.
- Composites by hand happen on screen through the profile's own menu, the
  way a person does it. The agent section is the one place the terminal
  is the subject, as asked.
- The terminal is real, tiled to the right of the app at about 38% of the
  width, and runs `vp-agent`, which plays an agent's turn with the real
  `omacharts` commands against the studio window, so the data is the same
  every render.
- The craft guide allows only cuts and short crossfades. The sweep as the
  terminal opens and closes, and the paired zoom on the first merge, are an
  exception for the agent section only, because the author asked for them.
  Everything else cuts.
- The composite ranges come from the author's own run on SPY four-hour
  with monthly profiles: September to November 2025, December 2025 to
  March 2026, May to July 2026 and August to October 2026. The narration
  quotes no prices, percentages or counts the app shows; the storyboard
  reads them off the studio.
- The agent itself is shown, not named, unless the author wants it named.
- Length: the author is fine with two minutes or more, so each section
  takes the time it needs: about three minutes fifty all told, most of it
  narration at a relaxed pace, with real holds where the app is working.

## Setup

Four chartbooks, one per stretch of the video, in strip order: SPY (the
teaser: four-hour, monthly profiles, eighteen months, bars hidden), Auction
(SPY five-minute, regular session, session profiles), QQQ (daily, six
months, nothing on it) and Study (SPY four-hour, three months, nothing on
it yet). New charts open on no chart template, so nothing leaks into the
Default template. The sidebar is hidden for the whole video.

    cli template default none
    cli chartbook rename "Chartbook 1" SPY
    cli chart set --symbol SPY --resolution 4h --template none --bars off
    cli chart indicator add volume_profile --anchor month
    cli chart range 18m
    cli chartbook create Auction --symbol SPY --switch
    cli chart set --resolution 5m --session regular
    cli chart indicator add volume_profile
    cli chart range "oct 27:oct 30"
    cli chartbook create QQQ --symbol QQQ --switch
    cli chart set --resolution 1D
    cli chart range 6m
    cli chartbook create Study --symbol SPY --switch
    cli chart set --resolution 4h
    cli chart range 3m
    cli chartbook switch SPY
    key ctrl+b

## Script

### 1. Months into balance

**Picture.** SPY on four-hour bars over the last eighteen months, the bars
hidden, so the chart is only a row of blue monthly profiles, each with its
stronger value area and its point of control. Held still for about two
seconds, long enough to take in the months. Then, right to left, a third of
a second apart, neighbouring months fold together: October 2026 into
September and that into August; July into June and that into May; March
2026 into February, January and December; November 2025 into October and
September. Four wide composites stand among the single months, each with
one point of control and one value area. Held a beat. No voice in this
scene. (The merges are `cli`, as a preview of the agent's result; the
storyboard checks they match what the agent leaves in scene 14.)

**Voice.** {fold}

    fold: wait 1.0; cli chart profile merge --at 2026-10-15; wait 0.3; cli chart profile merge --at 2026-09-15; wait 0.3; cli chart profile merge --at 2026-07-15; wait 0.3; cli chart profile merge --at 2026-06-15; wait 0.3; cli chart profile merge --at 2026-03-16; wait 0.3; cli chart profile merge --at 2026-02-16; wait 0.3; cli chart profile merge --at 2026-01-15; wait 0.3; cli chart profile merge --at 2025-11-14; wait 0.3; cli chart profile merge --at 2025-10-15
    after: 5.0

### 2. The idea

**Picture.** The folded chart from scene 1, held. Nothing moves.

**Voice.** Where the volume sits tells you what the market agreed was
fair. This is a year and a half of SPY, one volume profile a month, with
the months that traded at the same prices folded into one.

    after: 0.6

### 3. An auction

**Picture.** Ctrl and Alt and right arrow: the Auction chartbook, SPY on
five-minute bars over four regular sessions, a session profile at the
start of each, the value area stronger and the point of control a line
across it. Nothing else moves.

**Voice.** {next}It comes from auction market theory, and the basics are
short. The market is an auction. Price moves up and down to find the other
side of each trade. In balance, it trades inside value and builds the
profile; out of balance, it leaves to find new value.

    next: key ctrl+alt+Right
    after: 0.6

### 4. Value

**Picture.** The same chart. On "value area", one zoom of about two times
on the third session's profile, framing its stronger band and the point
of control line together. It is held through both definitions and eases
back to the full chart as "When price leaves value" begins. The cursor
stays off the chart.

**Voice.** Here's one session. {zoom}The value area is the band holding
about seventy percent of the volume. The point of control is the busiest
price. When price leaves value, the market tests the move. Either it's
rejected and price comes back, or it's accepted and new value builds
there.

    zoom: zoom 58% 50% 2.0
    after: 0.8

### 5. A profile you draw

**Picture.** Ctrl and Alt and right arrow: the QQQ chartbook, QQQ daily
over six months, candles only. Nothing else moves.

**Voice.** {next}The quickest way to get a profile is to draw one, over
exactly the bars you choose. A rally, say, from its low to its high.

    next: key ctrl+alt+Right
    after: 0.5

### 6. Draw it

**Picture.** Alt and V, its badge on screen. The cursor presses at the
low of a clean rally in the middle of the chart and drags to its high; on
release, a profile of just those bars appears behind them, as tall as
their range, its value area stronger and its point of control a line. It
is left selected, a grip at each corner. The cursor takes the top-right
grip and pulls it a few bars to the right; the profile widens and is
worked out again.

**Voice.** {tool}Alt and V, {drag}and drag across the move. That's where
its volume traded. {grip}Stretch it a few bars, and it's worked out again.

    tool: key alt+v
    drag: drag 34% 72% 62% 30%
    grip: drag 62% 30% 67% 30%
    after: 1.0

### 7. One for every period

**Picture.** Ctrl and Alt and right arrow: the Study chartbook, SPY on
four-hour bars over the last three months, candles only. Nothing else
moves.

**Voice.** {next}For a profile of every period, there's a study. This is
SPY on four-hour bars.

    next: key ctrl+alt+Right
    after: 0.5

### 8. Add the study

**Picture.** Ctrl and Shift and I opens **Add an indicator**; "profile"
is typed and Enter picks Volume Profile, whose settings open; Ctrl and
Enter adds it. A narrow profile sits at the start of every session, under
the candles. The cursor right-clicks a profile near the middle of the
chart and its menu opens, the **Reset every** buttons along the top with
Session ringed; a click on **Week** redraws one profile a week. Another
right-click, a click on **Month**: four monthly profiles, wide and easy
to read.

**Voice.** {add}Add Volume Profile {pick}from the indicators, {done}and
every session gets its own. For a wider view, {menu1}reset it {week}every
week, {menu2}or {month}every month.

    add:   key ctrl+shift+i
    pick:  type profile; key Return
    done:  key ctrl+Return
    menu1: rclick 50% 45%
    week:  click 55% 48%
    menu2: rclick 50% 45%
    month: click 58% 48%
    after: 1.0

### 9. Zoom out, hide the bars

**Picture.** The cursor rests near the right of the plot and the wheel
zooms time out about it: the months shrink until about a year and a half
is on screen, May 2025 to now, a monthly profile under every month. Ctrl
and Alt and Shift and H, its badge on screen: the candles disappear and
only the profiles are left. Runs of months stacked at the same prices
stand out from the ones that moved. Held, the cursor off the chart.

**Voice.** Now {out}zoom out to a year and a half. {hide}Control, Alt,
Shift and H hides the bars. With only the profiles left, you can see where
price found balance at a glance: months stacked at the same prices,
building the same value.

    out:  scroll 88% 50% 8
    hide: key ctrl+alt+shift+h
    after: 1.2

### 10. Composites

**Picture.** The same view, bars hidden. Nothing moves for the first two
sentences. Then the cursor right-clicks January 2026's profile and its
menu opens; a click on **Merge with previous profile** folds it into
December: one wider profile, one point of control, one value area over
both. A right-click on February, Merge with previous profile, and it joins;
the same on March. December to March is one composite, its value area
across all four months. Held, the cursor off the chart.

**Voice.** Neighbouring months that share their value are really one
balance, and a composite treats them as one. {menu1}December and January
traded at the same prices, {merge1}so merge them: one profile, one point of
control, one value area. {menu2}February overlaps too, {merge2}and
{menu3}so does {merge3}March. That's the whole balance, December to March,
in one profile.

    menu1:  rclick 41% 42%
    merge1: click 46% 51%
    menu2:  rclick 46% 42%
    merge2: click 51% 51%
    menu3:  rclick 51% 42%
    merge3: click 56% 51%
    after: 1.2

### 11. Ask an agent

**Picture.** The same view, the December to March composite among the
single months. Nothing moves.

**Voice.** Doing that across the whole chart is the same judgement, made
again and again. That's work for an agent.

    after: 0.5

### 12. The request

**Picture.** A terminal opens on the right, taking about 38% of the
width, themed like the app, the composer covering the snap with a light
sweep and labelling the two panes; the chart reflows into the rest, every
profile still on screen. `vp-agent` is typed at the shell
prompt and Enter starts it; it shows a bare `❯` prompt. The request is
typed into it at human speed: "Merge neighbouring profiles with value
areas overlapping at least 50%". Enter sends it.

**Voice.** {open}An agent drives Omacharts from the terminal, {start}with
the same commands you'd type. {ask}The rule goes in plain words: merge
neighbouring profiles with value areas overlapping at least half. It works
out the rest. {go}

    open:  terminal open
    start: type vp-agent; key Return
    ask:   type Merge neighbouring profiles with value areas overlapping at least 50%
    go:    key Return
    after: 0.3

### 13. The run

**Picture.** The agent's one sentence of plan, then the real `omacharts
chart profile list --on-screen --json` and how many profiles it found.
Then it walks them right to left, a line per pair: a merge prints its
overlap and the real `omacharts chart profile merge --at …`, and the
profile on the left folds into its neighbour as it lands; a pair that
doesn't overlap enough says "kept apart" in grey. On the first merge, a
paired zoom frames the merge command in the terminal and the profile it
changes on the chart together, then pulls back to the full split frame
for the rest, which run at their own pace. It ends on its summary line,
how many merges and how many profiles there are now.

**Voice.** It reads the profiles on screen and walks them right to left.
Each merge is a real Omacharts command, and the chart changes as it lands.
Months that don't overlap enough stay apart.

    after: 8.0

### 14. The result

**Picture.** The terminal closes, the same light sweep covering it, and
the chart widens back to the full frame: the same eighteen months, bars hidden, now four
wide composites among the single months. Held through the first two
sentences. Then Ctrl and Alt and Shift and H, its badge on screen, and the
candles come back over the profiles. Held, the cursor off the chart.

**Voice.** {close}Each composite is a range where the market accepted one
value. The gaps between them are where it went looking for the next.
{bars}The same keys bring the bars back.

    close: terminal close; focus app
    bars:  key ctrl+alt+shift+h
    after: 1.5

### 15. Any theme

**Picture.** Ctrl and comma opens Preferences over the middle of the
window, the chart still showing on either side. A click on **Theme** in
its sidebar, a click on the **Theme** row's drop-down, and a click on
**Carbon**: the window, the candles, the profiles and the composites turn
to Carbon's neutral greys at once. A breath, then the drop-down again and
**Paper**: everything goes light, warm off-white with ink text, the
profiles recoloured to match. Escape closes Preferences and the full chart
holds in Paper, the music up in the gap. (If the dialog hides too much of
the chart, the storyboard can close it between the two themes.)

**Voice.** {prefs}And it all follows the theme, composites included.

    prefs: key ctrl+comma; wait 0.8; click 33% 30%; wait 0.6; click 62% 22%; wait 0.5; click 62% 36%; wait 2.5; click 62% 22%; wait 0.5; click 62% 39%; wait 1.5; key Escape
    after: 7.5

### 16. Close

**Picture.** SPY four-hour in Paper, bars and composites showing, held.
The cursor rests off the chart.

**Voice.** A profile shows you where the market agreed, and where it
didn't. The Volume page of the manual has the rest.

    after: 2.5

## Pronunciation

- SPY: S P Y
- QQQ: triple Q
- Omacharts: Oma-charts
