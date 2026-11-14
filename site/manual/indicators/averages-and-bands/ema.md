---
title: "Exponential Moving Average"
description: "An average of closes that weights the latest bars most"
section: charting
parent: indicators/averages-and-bands
nav_order: 2
---

An average of closes that weights the latest bars most, so it turns sooner than a simple average of the same length.

| Drawn | Legend | Type to add | CLI |
|---|---|---|---|
| Over the price | **EMA 21** | `ema`, or `ema 9` for the period | `ema` |

<!-- regenerate:
theme: tokyo-night
window: 1600x1200 @2, scratchpad kit
state:
  key ctrl+shift+b
  omacharts chart set --symbol SPY --resolution 1D --template none
  omacharts chart indicator add ema
  move 795 595
capture: bin/headless-window cli chart screenshot --output /tmp/ema.png; magick /tmp/ema.png -quality 88 site/assets/manual/indicators/ema.jpg
crop: none
-->
{% include figure.html src="/assets/manual/indicators/ema.jpg" alt="SPY daily with a 21-day exponential average" caption="SPY daily with EMA 21." %}

## Settings

| Setting | Default | Allowed |
|---|---|---|
| **Period** | 21 | 1 to 500 |
| **Colour**, **Thickness**, **Line style** | The next free colour, 1.5 px, solid | See [Colours and lines](/manual/indicators/#colours-and-lines) |

## See also

- [Averages and bands](/manual/indicators/averages-and-bands/)
- [Adding an indicator](/manual/indicators/#adding-an-indicator)
- [Indicators from the CLI](/manual/cli/#indicators)
