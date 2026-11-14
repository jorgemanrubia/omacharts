---
title: "Simple Moving Average"
description: "The average close of the last N bars"
section: charting
parent: indicators/averages-and-bands
nav_order: 1
---

The average close of the last *period* bars. It has no value until there are that many.

| Drawn | Legend | Type to add | CLI |
|---|---|---|---|
| Over the price | **SMA 50** | `sma`, or `sma 200` for the period | `sma` |

<!-- regenerate:
theme: tokyo-night
window: 1600x1200 @2, scratchpad kit
state:
  key ctrl+shift+b
  omacharts chart set --symbol SPY --resolution 1D --template none
  omacharts chart indicator add sma --period 50
  omacharts chart indicator add sma --period 200
  move 795 595
capture: bin/headless-window cli chart screenshot --output /tmp/sma.png; magick /tmp/sma.png -quality 88 site/assets/manual/indicators/sma.jpg
crop: none
-->
{% include figure.html src="/assets/manual/indicators/sma.jpg" alt="SPY daily with a 50-day average in blue and a 200-day average in amber" caption="SPY daily with SMA 50 and SMA 200." %}

## Settings

| Setting | Default | Allowed |
|---|---|---|
| **Period** | 50 | 1 to 500 |
| **Colour**, **Thickness**, **Line style** | The next free colour, 1.5 px, solid | See [Colours and lines](/manual/indicators/#colours-and-lines) |

## See also

- [Averages and bands](/manual/indicators/averages-and-bands/)
- [Adding an indicator](/manual/indicators/#adding-an-indicator)
- [Indicators from the CLI](/manual/cli/#indicators)
