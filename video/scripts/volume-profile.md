---
title: Volume profile
stage: brief
length: 155s
theme: tokyo-night
voice: narrator
music: ambient electronic, warm analog pads and a soft plucked arpeggio over a slow, deliberate pulse; patient and a little curious through the theory, steadier once the study is on screen, and in the agent section the pulse firms up and a second arpeggio layer enters as the profiles fold together, resolving brighter for the close
---

## Brief

**Pitch.** A volume profile shows where a market actually traded, price
by price. It is the basic instrument of auction market theory, and in
Omacharts it is a study, a drawing, and composites you can build by hand
or hand to an agent.

**For.** Traders who have heard of volume profile, value area and point
of control but never used them, and Omacharts users who haven't turned the
study on yet. No prior auction theory assumed.

**Payoff.** Straight after the cover: SPY on four-hour bars, a monthly
profile under every month, and in a few quick seconds the months fold
together into four wide composites, each with one point of control and
one value area. No terminal yet; that is saved for the end. Then the
first line says the idea in plain words: where the volume sits tells you
what the market agreed was fair.

**Beats.** Each section opens with a sentence or two over its chart, with
nothing moving, saying what it is about. The first one also carries the
idea that ties the video together.
1. **The idea: an auction.** One SPY session profile, held still, with a
   zoom on each part as it is named. Kept short and plain: the market is
   an auction; price moves up and down to find the other side of the
   trade. Where most volume trades is where buyers and sellers agreed:
   the value area, the band holding about seventy percent of it. The
   point of control is the single busiest price. When price leaves value,
   the market tests the move: either it's rejected and price comes back,
   or it's accepted and new value builds there.
2. **The study.** On a fresh SPY chart, add Volume Profile (Ctrl and
   Shift and I). It draws one profile per period, under the candles, the
   value area stronger and the point of control as a line. Right-click a
   profile and step Reset every from Session to Week to Month with the
   buttons in its menu. One look change in its properties (Alt and
   Enter): Outline style, so the candles stay clear, or Right alignment.
3. **The drawing.** A profile of exactly the stretch you choose, not a
   calendar period. Alt and V, drag across a move, say a rally from its
   low to its high, and the profile of just those bars appears. Pull a
   corner grip to stretch it a few bars and it's worked out again. Alt and
   four puts it on one of the nine presets.
4. **Composites.** Two neighbouring periods that traded around the same
   prices are really one area of value. Right-click a profile, Merge with
   previous profile: one profile, one point of control, one value area
   over both. Merge once more to take in a third. Split composite profile
   puts them back.
5. **Ask an agent.** Doing that across a year of months is a judgement
   applied many times, which is what an agent is for. A transition opens
   the frame: the chart slides right and a real terminal slides in on the
   left, side by side. The author's prompt is typed in plain words: walk
   the profiles on screen right to left, merge each into its left
   neighbour when their value areas overlap by at least half of the
   narrower one, and read them again after each merge. The terminal shows
   the agent reading the profiles (`omacharts chart profile list
   --on-screen --json`) and then merging (`omacharts chart profile merge
   --at …`); each merge lands on the chart on the right as it runs. The
   first merge is shown at full speed with a zoom pairing the command and
   the profile it changes; the rest run faster. The frame pulls back to
   the full chart: nineteen monthly profiles are now ten, composites from
   September to November 2025, December 2025 to March 2026, May to July
   2026 and August to October 2026.
6. **Close.** One line on what the profile gives you, and where to read
   more: the Volume page of the manual.

**What the source changes.**
- Order: the drawing comes before composites, so composites by hand lead
  straight into the agent doing the same thing at scale.
- The drawing key, Alt and V, is bound in the app but missing from the
  `?` shortcuts list (`src/ui/window.rs`); worth adding before the video,
  since the close may point at `?`.
- Drawings show on every chart of their symbol, and composites belong to
  their chart, symbol and period. So each section gets its own chart: the
  theory on a SPY intraday session profile, the study and composites by
  hand on a SPY hourly chart with session or weekly profiles, the drawing
  on another symbol (QQQ, say) so it doesn't turn up later, and the agent
  on its own SPY four-hour chartbook with monthly profiles and no
  composites. The opening teaser is a fast preview of that last chart.
- Composites by hand happen on screen through the profile's own menu, the
  way a person does it. The agent section is the one place the terminal
  is the subject, as asked.
- The agent section needs the studio extended: a real terminal recorded
  next to the app window on one timeline, and a Remotion layout for the
  side-by-side with the slide-in and slide-out. An agent's run is not the
  same twice, so it is recorded once against the studio window and that
  recording is reused in every render, to keep the same data each time.
  Gaps where the agent is thinking are sped up.
- The craft guide allows only cuts and short crossfades. The slide into
  the split frame and back out, and the paired zoom on the first merge,
  are an exception for this section only, because the author asked for
  them. Everything else cuts.
- The nineteen-into-ten counts and the composite ranges come from the
  author's own run on SPY four-hour with monthly profiles. The narration
  won't quote prices or percentages the app shows; the storyboard reads
  them off the studio.
- The agent itself (Claude Code, or whichever one the author uses) is
  shown, not named, unless the author wants it named.
- Length: the theory takes about thirty seconds and the agent section
  about forty. With five section introductions that comes to about two
  and a half minutes, at the long end of a walkthrough but honest for
  five sections. If it has to be shorter, the style change in beat 2 and
  the preset in beat 3 are the first things to drop.
