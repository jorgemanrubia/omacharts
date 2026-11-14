---
title: "Bollinger Bands"
description: "A simple average with a standard-deviation band either side"
section: charting
parent: indicators/averages-and-bands
nav_order: 4
---

Drawn over the price. A simple average of closes with a band a multiple of their standard deviation either side, shaded and outlined.

| Setting | Default | Allowed |
|---|---|---|
| **Period** | 20 | 1 to 500 |
| **Standard deviations** | 2 | 0.1 to 6 |
| **Band** | On | See [Bands](/manual/indicators/averages-and-bands/#bands) |

The label includes the multiple, as in **BB 20 2** and **BB 20 2.5**.

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

## See also

- [Averages and bands](/manual/indicators/averages-and-bands/)
- [Indicators from the CLI](/manual/cli/#indicators)
