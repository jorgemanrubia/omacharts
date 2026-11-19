---
title: "Market Profile"
description: "How much time the price spent at each price, period by period, in TPO letters"
section: charting
parent: indicators/volume
nav_order: 3
---

For each reset period, how long the price spent at each price. The period is cut into **TPO periods**, half an hour each unless you choose otherwise, lettered A to Z and then a to z. Every row the bars of a TPO period reach gets that period's letter once, however many of its bars touch the row, so a row's letters read left to right in the order the price visited it. The row with the most letters is the **point of control**, and the **value area** is the rows around it that hold the given share of every letter in the period.

| Drawn | Legend | Type to add | CLI |
|---|---|---|---|
| Over the price | **TPO 30m · Session** | `tpo` | `market_profile` |

<!-- regenerate:
theme: tokyo-night
window: 1600x1200 @2, scratchpad kit
state:
  key ctrl+shift+b
  omacharts chart set --symbol SPY --resolution 5m --session regular --template none
  omacharts chart indicator add market_profile --profile-width 0.6
  move 795 595
capture: bin/headless-window cli chart screenshot --output /tmp/tpo.png; magick /tmp/tpo.png -quality 88 site/assets/manual/indicators/market-profile.jpg
crop: none
-->
{% include figure.html src="/assets/manual/indicators/market-profile.jpg" alt="SPY 5m with a market profile of letters at the start of each session" caption="SPY 5m with a session market profile." %}

## Settings

| Setting | Default | Allowed |
|---|---|---|
| **Reset every** | Session | Session, Week, Month, Quarter, Year ([Reset periods](/manual/indicators/#reset-periods)) |
| **TPO period (minutes)** | 30 | 1 to 1440. Each TPO period's letters are counted from the period's first bar |
| **Automatic**, **Row height (ticks)** | Automatic | A row [so many ticks](#rows) tall, 1 to 10000 |
| **Value area %** | 70% | 10 to 100% |
| **Highlight value area** | On | Off draws every row alike |
| **Value area colour** | The profile's colour | A palette name, or a custom colour, for the value area while it is highlighted. The rows outside it are the profile's colour, fainter |
| **Style** | Letters | Letters, or Blocks: a square for each letter. Letters too small to read are drawn as blocks |
| **Colour** | The next free colour | A palette name, or a custom colour |
| **Show point of control** | On | Off draws the point of control's row as the value area's |
| **Point of control colour** | A contrasting colour from the theme | A palette name, or a custom colour |
| **Alignment** | Left | Left, or Right: the rows grow back from the end of the period, the letters too, and the session still forming is drawn past the last bar as a volume profile's is ([Aligned right](/manual/indicators/volume/volume-profile/#aligned-right)). A volume profile with the same period is drawn from this edge too ([Beside a volume profile](#beside-a-volume-profile)) |
| **Width %** | 40% | 10 to 100%: the furthest the row with the most letters may reach, of the period's width. Each letter or block is about as wide as its row is tall, so a profile with room to spare stays that compact |

## Rows

Rows are a whole number of the instrument's ticks tall, one height for every period on the chart, so a quiet session is drawn short beside a busy one. **Automatic** takes the smallest of 1, 2, 4, 5, 10, 20, 40, 50 ticks and on that keeps a typical period within 60 rows: an ES session of 60 points is a point a row. With **Automatic** off, **Row height (ticks)** is the height.

The point of control is the row with the most letters. Of two with as many, it is the one nearer the middle of the profile. The value area starts there and takes the next row up or down, whichever holds more letters, until it holds the **Value area %** of them.

## Beside a volume profile

A market profile and a [volume profile](/manual/indicators/volume/volume-profile/) with the same **Reset every** are drawn as a pair: each session's two sit side by side inside that session, touching, the volume profile against the edge they are drawn from and the market profile's letters starting where its longest rows end. Each is as wide as its **Width %**, and the two are squeezed alike when they come to more than the session. They are drawn from the market profile's **Alignment**, whatever the volume profile's says: from the session's start when it is Left, and back from the session's end, mirrored, when it is Right, the session still forming then drawn past the last bar with both in it. A volume profile's own **Alignment** is greyed while it is paired, and reads *Set by the market profile beside it*.

So a session's volume profile is never drawn against the next session's market profile. With each from an edge of its own, one's rows would grow back from the session's end and the other's on from the next session's start, and the two would read as one pair across the boundary.

With different periods, each is drawn from its own **Alignment**, as it is alone. Change either's **Reset every** to match the other's and they pair again. Profiles drawn from the same edge still share their room side by side, as several volume profiles do: volume profiles nearest the edge, then market profiles, each kind in the order the legend lists it.

## Composite profiles

Right-click a profile and choose **Merge with previous profile** to fold it into the one before it: one profile over both periods, with one point of control and one value area. Each period's TPO periods keep their letters, counted from that period's first bar. Merge again to take in another. **Split composite profile**, on a composite, puts its periods back as they were. A composite is kept as a [volume profile's](/manual/indicators/volume/volume-profile/#composite-profiles) is: with the chart, for the symbol and the period it was made under.

Paired with a volume profile, merging or splitting either merges or splits both, so a session's two always cover the same span. Merges made while they had different periods are joined when they pair again. With different periods, or with no volume profile, a market profile merges and splits on its own.
