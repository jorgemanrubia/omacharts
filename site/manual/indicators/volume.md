---
title: "Volume"
description: "Volume and the volume profile, with every setting."
section: charting
parent: indicators
nav_order: 2
---

The volume indicators show how much traded: Volume bar by bar in a strip under the price, and the volume profile at each price, over the price. Both are drawn as bars and have no thickness or line style.

## Volume

Its own strip. Each bar's volume, in the candles' up or down colour, dimmed.

| Setting | Default | Allowed |
|---|---|---|
| **Pane height %** | 18% | 5 to 95% |
| **Up** colour | The candles' up colour, dimmed | A palette name, or a custom colour |
| **Down** colour | The candles' down colour, dimmed | A palette name, or a custom colour |

The **Default** [chart template](/manual/chart-templates/) holds Volume, with [Session Times](/manual/indicators/session-times/).

<!-- regenerate:
theme: tokyo-night
window: 1600x1200 @2, scratchpad kit
state:
  key ctrl+b
  omacharts chart set --symbol SPY --resolution 1D --template none
  omacharts chart indicator add volume
  move 795 595
capture: bin/headless-window cli chart screenshot --output /tmp/vol.png
crop: 0,812,1592,380 (then -quality 88 to site/assets/manual/indicators/volume.jpg)
-->
{% include figure.html src="/assets/manual/indicators/volume.jpg" alt="The volume strip under a SPY daily chart, bars in dimmed green and red" caption="The Volume strip under SPY daily." %}

## Volume Profile

Drawn over the price. For each reset period, how much volume traded at each price, as a histogram against the left edge of the period, at most 40% of its width. The rows in the **value area**, the band around the busiest price that holds the given share of the period's volume, are drawn stronger. The **point of control**, the busiest row, is a line across the profile.

| Setting | Default | Allowed |
|---|---|---|
| **Reset every** | Session | Session, Week, Month, Quarter, Year ([Reset periods](/manual/indicators/#reset-periods)) |
| **Automatic**, **Rows** | Automatic | A number of rows per period, 4 to 400 |
| **Value area %** | 70% | 10 to 100% |
| **Profile** colour | The next free colour | A palette name, or a custom colour |
| **Point of control** colour | A contrasting colour from the theme | A palette name, or a custom colour |

**Automatic** gives every period rows of the same height, a round multiple of the instrument's price step. With **Automatic** off, **Rows** is the number of rows each period is divided into.

<!-- regenerate:
theme: tokyo-night
window: 1600x1200 @2, scratchpad kit
state:
  key ctrl+b
  omacharts chart set --symbol SPY --resolution 5m --session regular --template none
  omacharts chart indicator add volume_profile
  move 795 595
capture: bin/headless-window cli chart screenshot --output /tmp/vp.png; magick /tmp/vp.png -quality 88 site/assets/manual/indicators/volume-profile.jpg
crop: none
-->
{% include figure.html src="/assets/manual/indicators/volume-profile.jpg" alt="SPY 5m with a volume profile at the start of each session" caption="SPY 5m with a session volume profile." %}

## See also

- [Indicators](/manual/indicators/)
- [Indicators from the CLI](/manual/cli/#indicators)
