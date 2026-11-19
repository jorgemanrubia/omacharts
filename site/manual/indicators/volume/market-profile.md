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
| **Alignment** | Left | Left, or Right: the rows grow back from the end of the period, the letters too, and the session still forming is drawn past the last bar as a volume profile's is ([Aligned right](/manual/indicators/volume/volume-profile/#aligned-right)) |
| **Width %** | 40% | 10 to 100%: the furthest the row with the most letters may reach, of the period's width. Each letter or block is about as wide as its row is tall, so a profile with room to spare stays that compact |

## Rows

Rows are a whole number of the instrument's ticks tall, one height for every period on the chart, so a quiet session is drawn short beside a busy one. **Automatic** takes the smallest of 1, 2, 4, 5, 10, 20, 40, 50 ticks and on that keeps a typical period within 60 rows: an ES session of 60 points is a point a row. With **Automatic** off, **Row height (ticks)** is the height.

The point of control is the row with the most letters. Of two with as many, it is the one nearer the middle of the profile. The value area starts there and takes the next row up or down, whichever holds more letters, until it holds the **Value area %** of them.

## Beside a volume profile

A market profile and a [volume profile](/manual/indicators/volume/volume-profile/) aligned to the same edge share each period side by side, in the order the legend lists them, each as wide as its **Width %**, and never on top of each other. Aligned right, both are mirrored and run back from the end of the period.
