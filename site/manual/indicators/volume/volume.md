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
| **Colouring** | **Default** | **Vivid**, **Soft**, **Quiet** or **Monochrome**, so the volume can be painted in a colouring the price is not ([Colouring](/manual/charts/#colouring)), or **Custom** for colours of your own |
| **Opacity** | 85% | How solid rising and falling bars are both drawn, 0 to 100%. At 100% they hide what is behind them. Shown for every colouring but **Custom** |
| **Up** | The colour rising bars were drawn in before **Custom** was chosen | A palette name, or a custom colour, with its own **Opacity**. Shown only for **Custom** |
| **Down** | The same for falling bars | The same |

A selected strip is drawn solid whatever its opacity. The CLI's `--opacity` sets both bars' at once, and `--up-transparency` and `--down-transparency` each on its own.

## Colouring

**Default** is the chart's own colouring, and follows it whatever it is changed to, and its **Swap up and down colours** with it. **Hollow up bars** does not reach the volume: a volume bar is a solid bar with no body to leave open.

Choosing a colouring keeps the volume in it while the price changes: vivid candles over a soft volume, or the other way round. A colouring of its own is painted its own way round, whether or not the chart's up and down are swapped.

**Custom** shows an **Up** and a **Down** row in place of **Opacity**, each a swatch and how solid its bars are. They start at the colours and the opacity the strip had a moment before, so nothing moves until you pick one. Choosing another colouring draws that colouring as it is; the custom colours come back if you choose **Custom** again before closing the dialog.

Chosen here, **Monochrome** paints every bar in one colour, the ink of the chart's Monochrome candles, rising and falling alike; a strip on **Default** under Monochrome candles is that same colour. **Quiet** is not the same: it keeps two muted colours, one for rising and one for falling.

<!-- regenerate:
theme: tokyo-night
window: 2560x1440 @2, bin/headless-window (HOME sandboxed, theme seeded)
state:
  (one ES 1m chart on the synthetic feed, window 1400x900 @2, a Volume indicator and nothing else)
  key ctrl+shift+s; click 700 90 (Indicators); click 600 355 (Volume); click 900 323 (the Colouring row)
capture: bin/headless-window shot /tmp/volume-colouring.png
crop: 1258x1588+808+104, the dialog, 16px round it and the open list
-->
{% include figure.html src="/assets/manual/indicators/volume-colouring.png" alt="The Volume indicator's settings with its Colouring list open on Default, Vivid, Soft, Quiet, Monochrome and Custom" caption="A volume strip's own Colouring." %}

<!-- regenerate:
as above, with the Colouring row closed on Default
crop: the Colours group, from its title to the Opacity row
-->
{% include figure.html src="/assets/manual/indicators/volume-opacity.png" alt="The Volume indicator's Colours group: Colouring on Default, then one Opacity slider at 85%" caption="A colouring, with one opacity for both." %}

<!-- regenerate:
as above, then choose Custom in the Colouring row
crop: the Colours group, from its title to the Down row
-->
{% include figure.html src="/assets/manual/indicators/volume-custom.png" alt="The Volume indicator's Colours group on Custom: Up and Down, each a swatch and its Opacity slider" caption="Custom, a colour and an opacity for each." %}

## See also

- [Volume](/manual/indicators/volume/)
- [Adding an indicator](/manual/indicators/#adding-an-indicator)
- [Volume from the CLI](/manual/cli/indicators/volume/)
