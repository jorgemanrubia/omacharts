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
| **Colouring** | **Default** | **Vivid**, **Soft** or **Quiet**, so the volume can be painted in a colouring the price is not ([Colouring](/manual/charts/#colouring)) |
| **Opacity %** | 85% | 20 to 100%. At 100% the bars hide what is behind them |
| **Up** colour | The colouring's up colour | A palette name, or a custom colour |
| **Down** colour | The colouring's down colour | A palette name, or a custom colour |

A selected strip is drawn solid whatever its **Opacity %**.

## Colouring

**Default** is the chart's own colouring, and follows it whatever it is changed to, and its **Swap up and down colours** with it. **Hollow up bars** does not reach the volume: a volume bar is a solid bar with no body to leave open.

Choosing a colouring keeps the volume in it while the price changes: vivid candles over a soft volume, or the other way round. A colouring of its own is painted its own way round, whether or not the chart's up and down are swapped. **Up** and **Down** then take that colouring's colours, and a colour set by hand wins over both.

Monochrome is not offered, because it paints rising and falling bars alike. A strip already painted Monochrome keeps it, and it is listed last.

<!-- regenerate:
theme: tokyo-night
window: 2560x1440 @2, bin/headless-window (HOME sandboxed, theme seeded)
state:
  (one NVDA 1D chart on the Default template, sidebar hidden)
  key ctrl+shift+s; click 700 72 (Indicators); click 600 282 (Volume); click 800 383 (the Colouring row)
capture: bin/headless-window shot /tmp/volume-colouring.png
crop: 700,84,1160,1276
-->
{% include figure.html src="/assets/manual/indicators/volume-colouring.png" alt="The Volume indicator's settings with its Colouring list open: Same as the chart, Vivid, Soft, Quiet, Monochrome" caption="A volume strip's own Colouring." %}

<!-- regenerate:
as above, with the Colouring row closed
crop: 762,640,1036,518 (the Colours group)
-->
{% include figure.html src="/assets/manual/indicators/volume-opacity.png" alt="The Volume indicator's Colours group: Colouring, then the Opacity slider at 85%, then Up and Down" caption="Opacity, under the strip's Colouring." %}

## See also

- [Volume](/manual/indicators/volume/)
- [Adding an indicator](/manual/indicators/#adding-an-indicator)
- [Indicators from the CLI](/manual/cli/#indicators)
