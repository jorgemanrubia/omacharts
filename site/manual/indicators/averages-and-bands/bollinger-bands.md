---
title: "Bollinger Bands"
description: "A simple average with a standard-deviation band either side"
section: charting
parent: indicators/averages-and-bands
nav_order: 4
---

A simple average of closes with a band a multiple of their standard deviation either side, shaded and outlined.

| Drawn | Legend | Type to add | CLI |
|---|---|---|---|
| Over the price | **BB 20 2** | `bb`, or `bb 20 2.5` for the period and the multiple | `bollinger` |

<!-- regenerate:
theme: tokyo-night
window: 1600x1200 @2, scratchpad kit
state:
  key ctrl+shift+b
  omacharts chart set --symbol SPY --resolution 1D --template none
  omacharts chart indicator add bollinger
  move 795 595
capture: bin/headless-window cli chart screenshot --output /tmp/bb.png; magick /tmp/bb.png -quality 88 site/assets/manual/indicators/bollinger.jpg
crop: none
-->
{% include figure.html src="/assets/manual/indicators/bollinger.jpg" alt="SPY daily with shaded Bollinger Bands around the candles" caption="SPY daily with BB 20 2." %}

## Settings

| Setting | Default | Allowed |
|---|---|---|
| **Period** | 20 | 1 to 500 |
| **Standard deviations** | 2 | 0.1 to 6 |
| **Band** | On | See [Bands](/manual/indicators/averages-and-bands/#bands) |
| **Colour**, **Thickness**, **Line style** | The next free colour, 1.5 px, solid | See [Colours and lines](/manual/indicators/#colours-and-lines) |

The legend includes the multiple, as in **BB 20 2** and **BB 20 2.5**.

## See also

- [Averages and bands](/manual/indicators/averages-and-bands/)
- [Adding an indicator](/manual/indicators/#adding-an-indicator)
- [Indicators from the CLI](/manual/cli/#indicators)
