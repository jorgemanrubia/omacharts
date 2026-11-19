---
title: "Market Profile"
description: "How much time the price spent at each price, period by period, in TPO letters"
section: charting
parent: indicators/volume
nav_order: 3
---

A market profile, devised by J. Peter Steidlmayer at the Chicago Board of Trade in the 1980s, shows where the market spent its time rather than how it moved. Prices it kept returning to are where buyers and sellers agreed on value; prices it passed through quickly are where one side was in control. The shape of a session's profile tells a balanced day, trading around one price, from one that moved away to find another.

For each reset period, how long the price spent at each price. The period is cut into **TPO periods**, half an hour each unless you choose otherwise, lettered A to Z and then a to z. Every row the bars of a TPO period reach gets that period's letter once, however many of its bars touch the row, so a row's letters read left to right in the order the price visited it. The row with the most letters is the **point of control**, and the **value area** is the rows around it that hold the given share of every letter in the period.

| Drawn | Legend | Type to add | CLI |
|---|---|---|---|
| Over the price | **TPO 30m · Session** | `tpo` | `market_profile` |

<!-- regenerate:
theme: tokyo-night
window: 2000x1440, bin/headless-window, scratchpad kit (seven invented SPY sessions)
state:
  key ctrl+shift+b
  omacharts chart set --symbol SPY --resolution 5m --session regular --template none
  omacharts chart indicator add market_profile --profile-width 0.6
  move 995 715
capture: bin/headless-window cli chart screenshot --output /tmp/tpo.png; magick /tmp/tpo.png -quality 88 site/assets/manual/indicators/market-profile.jpg
crop: none
-->
{% include figure.html src="/assets/manual/indicators/market-profile.jpg" alt="SPY 5m with a market profile of letters at the start of each session" caption="SPY 5m with a session market profile." %}

## Alone or beside a volume profile

A market profile is a study of its own. It needs no volume profile, and on its own it is drawn, merged and split as a volume profile is ([Composite profiles](#composite-profiles)).

It can also be shown next to volume profiles. When a volume profile study with the same **Reset every** is on the chart, each session's two are drawn inside it, each from its own **Alignment**, and merge and split together ([Beside a volume profile](#beside-a-volume-profile)). With different periods, each one keeps to itself.

From the [CLI](/manual/cli/#volume-and-market-profiles):

```
# On its own
omacharts chart indicator add market_profile --tpo-minutes 30 --row-ticks auto

# Beside a volume profile, both per session: each session's two side by side
omacharts chart indicator add volume_profile --anchor session --profile-width 0.3
omacharts chart indicator add market_profile --anchor session --profile-width 0.4

# Back to back instead: the letters grow left, the volume profile's rows right
omacharts chart indicator set market_profile --align right

# Merge a session into the one before it, and split it again; a pair does both
omacharts chart profile merge --at 2026-11-18
omacharts chart profile split --at 2026-11-18
```

## Settings

The settings are in a [volume profile's](/manual/indicators/volume/volume-profile/#settings) four groups, with the same names in the same places. A switch's rows show while it is on.

### Parameters

| Setting | Default | Allowed |
|---|---|---|
| **Reset every** | Session | Session, Week, Month, Quarter, Year ([Reset periods](/manual/indicators/#reset-periods)) |
| **Last period only** | Off | On draws the newest period's profile alone, as it is drawn among the others |
| **TPO period (minutes)** | 30 | 1 to 1440. Each TPO period's letters are counted from the period's first bar |
| **Automatic rows**, **Row height (ticks)** | On | A row [so many ticks](#rows) tall, 1 to 10000, with **Automatic rows** off |
| **Value area %** | 70% | 10 to 100% |

### Style

A colour and its opacity share one row: the colour's name, the swatch, then **Opacity**, a slider and a figure for how solid the colour is drawn, 100% solid and 0% invisible. **Default**, first in the swatch's palette, goes back to the colour it has unless one is picked. The command line's `--*-transparency` flags set the same thing the other way up, as how much of the chart shows through.

| Setting | Default | Allowed |
|---|---|---|
| **Display** | Letters | Letters, or Blocks: a square for each letter. Letters too small to read are drawn as blocks |
| **Alignment** | Left | Left, or Right: the rows grow back from the end of the period, the letters too, and the session still forming is drawn past the last bar as a volume profile's is ([Aligned right](/manual/indicators/volume/volume-profile/#aligned-right)). Beside a volume profile with the same period, each keeps its own ([Beside a volume profile](#beside-a-volume-profile)) |
| **Width %** | 40% | 10 to 100%: the furthest the row with the most letters may reach, of the period's width. Each letter or block is about as wide as its row is tall, so a profile with room to spare stays that compact |
| **Colour** | The next free colour, at 95% opacity | A palette name, or a custom colour. Its **Opacity** is how solid the letters are drawn, 0 to 100%. Blocks are drawn fainter than letters at the same setting, so they do not cover the candles. The value area's two opacities follow it until they are set |
| **Outline** | Off | On draws the stepped edge round the letters' cells, as a volume profile's round its rows: the same for letters and blocks, a gap in the profile left open |
| **Outline colour** | The profile's **Colour**, solid | A palette name, or a custom colour. Its **Opacity** is how solid the outline is drawn, 0 to 100% |
| **Outline thickness** | 1 px | 0.5 to 12 px |
| **Outline style** | Solid | Solid, Dashed, Dotted |
| **Open and close** | Off | On marks where each profile starts and ends with a small arrow each (see below) |
| **Open** | A muted, greyed relative of the theme's up colour, solid | A palette name, or a custom colour. Its **Opacity** is how solid the open's arrow is drawn, 0 to 100% |
| **Close** | A muted, greyed relative of the theme's down colour, darker than the open's on a dark theme and lighter on a light one, so the two tell apart by lightness as well as hue; solid | A palette name, or a custom colour. Its **Opacity** is how solid the close's arrow is drawn, 0 to 100% |

The **open** is the period's opening price, its first bar's open, and the **close** its closing price, its last bar's close; while a session is still forming, the close is its latest price. Each is marked with a small arrow, the same as the chart's trade markers, beside a single TPO. The open's arrow stands just outside the profile's edge, pointing in at the first TPO period's letter or block in the row of the opening price. The close's arrow stands just past the last letter or block in the row of the closing price, pointing back at it. Aligned right, the profile is mirrored and so are the arrows. The open need not be on the first letter's lowest or highest row: it is wherever the session opened in that first TPO period. While the marks are on, the profile keeps a little room at each side for them, so they never cover a letter, the point of control or a volume profile beside it. A composite profile marks the first part's open and the last part's close.

### Value area

| Setting | Default | Allowed |
|---|---|---|
| **Highlight** | On | Off draws every row alike |
| **Colour** | The profile's **Colour**, its opacity following the Style group's | A palette name, or a custom colour, for the value area. Its **Opacity** is how solid the value area's letters are drawn, 0 to 100% |
| **Outside rows opacity %** | 55% | How solid the letters above and below the value area are drawn, 0 to 100% |

### Point of control

| Setting | Default | Allowed |
|---|---|---|
| **Show** | On | Off draws no point of control |
| **Colour** | A contrasting colour from the theme, solid | A palette name, or a custom colour. Its **Opacity** is how solid the line is drawn, 0 to 100% |
| **Thickness** | 2 px | 0.5 to 12 px |
| **Line style** | Solid | Solid, Dashed, Dotted |

The point of control is a line across the profile through the middle of its row, over the letters, as a volume profile's is. It is thicker than a volume profile's unless set, so through a row of letters it reads as a mark rather than a strikethrough.

## Rows

Rows are a whole number of the instrument's ticks tall, one height for every period on the chart, so a quiet session is drawn short beside a busy one. **Automatic rows** takes the smallest of 1, 2, 4, 5, 10, 20, 40, 50 ticks and on that keeps a typical period within 60 rows: an ES session of 60 points is a point a row. With **Automatic rows** off, **Row height (ticks)** is the height.

The point of control is the row with the most letters. Of two with as many, it is the one nearer the middle of the profile. The value area starts there and takes the next row up or down, whichever holds more letters, until it holds the **Value area %** of them.

## Beside a volume profile

<!-- regenerate:
theme: tokyo-night
window: 2560x1440 @2, bin/headless-window, scratchpad kit (seven invented SPY sessions)
state:
  omacharts chart set --symbol SPY --resolution 5m --session regular --template none
  omacharts chart indicator add market_profile --profile-width 1
  omacharts chart indicator add volume_profile --profile-width 0.4 --value-area-transparency 0.45 --outside-transparency 0.8 --poc-width 3 --poc-transparency 0
  key ctrl+shift+b
  scroll 640 360 7
  drag 700 500 612 500
  move 1270 700
capture: bin/headless-window cli chart screenshot --output /tmp/paired.png; magick /tmp/paired.png -quality 88 site/assets/manual/indicators/market-profile-paired.jpg
crop: none
-->
{% include figure.html src="/assets/manual/indicators/market-profile-paired.jpg" alt="SPY 5m over five sessions, each with a volume profile and a market profile side by side from the session's start" caption="Five sessions, each with its volume profile and market profile side by side." %}

A market profile and a [volume profile](/manual/indicators/volume/volume-profile/) with the same **Reset every** are drawn as a pair: each session's two inside that session, never over each other. Each is as wide as its **Width %**, and each grows from its own **Alignment**, so every combination of the two is a layout of its own:

| Market profile | Volume profile | Each session |
|---|---|---|
| Left | Left | Side by side from the session's start: the volume profile against it, the letters starting where its longest rows end |
| Right | Right | Side by side back from the session's end, mirrored: the volume profile against it, the letters before it. The session still forming is drawn past the last bar, both in it |
| Right | Left | Back to back: the letters grow left and the volume profile's rows right, from a spine they share, centred in the session |
| Left | Right | Back to back the other way: the volume profile's rows grow left and the letters right, from a shared spine, centred in the session |

Back to back, the two are centred with the room left over split either side, so neither has its base against the session's edge, where it would sit beside the next session's profile or the last one's and read as their pair. The session still forming is drawn on its bars, both of it. Side by side or back to back, the two are squeezed alike when their widths come to more than the session.

With different periods, each is drawn from its own **Alignment**, as it is alone. Change either's **Reset every** to match the other's and they pair again. Profiles of different periods drawn from the same edge still share their room side by side, as several volume profiles do.

## Composite profiles

Right-click a profile and choose **Merge with previous profile** to fold it into the one before it: one profile over both periods, with one point of control and one value area. Each period's TPO periods keep their letters, counted from that period's first bar. Merge again to take in another. **Split composite profile**, on a composite, puts its periods back as they were. A composite is kept as a [volume profile's](/manual/indicators/volume/volume-profile/#composite-profiles) is: with the chart, for the symbol and the period it was made under.

Paired with a volume profile, merging or splitting either merges or splits both, so a session's two always cover the same span. Merges made while they had different periods are joined when they pair again. With different periods, or with no volume profile, a market profile merges and splits on its own.
