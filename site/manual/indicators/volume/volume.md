---
title: "Volume"
description: "Each bar's volume, in a strip under the price"
section: charting
parent: indicators/volume
nav_order: 1
---

Each bar's volume, in the candles' up or down colour. The **Default** [chart template](/manual/chart-templates/) holds it, with [Session Times](/manual/indicators/session-times/).

| Drawn | Legend | Type to add | CLI |
|---|---|---|---|
| In a strip of its own | **Volume** | `vol` | `volume` |

<!-- regenerate:
theme: tokyo-night
window: 1600x1200 @2, scratchpad kit
state:
  key ctrl+shift+b
  omacharts chart set --symbol SPY --resolution 1D --template none
  omacharts chart indicator add volume
  move 795 595
capture: bin/headless-window cli chart screenshot --output /tmp/vol.png
crop: 0,812,1592,380 (then -quality 88 to site/assets/manual/indicators/volume.jpg)
-->
{% include figure.html src="/assets/manual/indicators/volume.jpg" alt="The volume strip under a SPY daily chart, bars in the candles' green and red" caption="The Volume strip under SPY daily." %}

## Settings

| Setting | Default | Allowed |
|---|---|---|
| **Pane height %** | 18% | 5 to 95% |
| **Opacity %** | 85% | 20 to 100%. At 100% the bars hide what is behind them |
| **Up** colour | The candles' up colour | A palette name, or a custom colour |
| **Down** colour | The candles' down colour | A palette name, or a custom colour |

A selected strip is drawn solid whatever its **Opacity %**.

## Colours

**Up** and **Down** follow the chart's candles, whatever their colours are changed to, and a colour set by hand wins. A volume bar is a solid bar with no body to leave open, so hollow candles do not reach the volume.

<!-- regenerate:
theme: tokyo-night
window: 2560x1440 @2, bin/headless-window (HOME sandboxed, theme seeded)
state:
  (one NVDA 1D chart on the Default template, sidebar hidden)
  key ctrl+shift+s; click 700 72 (Indicators); click 600 282 (Volume)
capture: bin/headless-window shot /tmp/volume-opacity.png
crop: 762,640,1036,518 (the Colours group)
-->
{% include figure.html src="/assets/manual/indicators/volume-opacity.png" alt="The Volume indicator's Colours group: the Opacity slider at 85%, then Up and Down" caption="Opacity, over the strip's Up and Down." %}

## See also

- [Volume](/manual/indicators/volume/)
- [Adding an indicator](/manual/indicators/#adding-an-indicator)
- [Indicators from the CLI](/manual/cli/#indicators)
