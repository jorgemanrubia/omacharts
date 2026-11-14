---
title: "Simple Moving Average"
description: "The average close of the last N bars"
section: charting
parent: indicators/averages-and-bands
nav_order: 1
---

Drawn over the price. The average close of the last *period* bars. It has no value until there are that many.

| Setting | Default | Allowed |
|---|---|---|
| **Period** | 50 | 1 to 500 |

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

## See also

- [Averages and bands](/manual/indicators/averages-and-bands/)
- [Indicators from the CLI](/manual/cli/#indicators)
