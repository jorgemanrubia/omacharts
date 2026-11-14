---
title: "Volume Weighted Average Price"
description: "The volume-weighted average price since the period began"
section: charting
parent: indicators/averages-and-bands
nav_order: 3
---

Drawn over the price. The average price paid since the period began, each bar's typical price (the mean of high, low and close) weighted by its volume. Three bands at 1, 1.5 and 2 standard deviations. When on, the first is shaded with no outline, the second dashed, the third a plain line. An instrument with no volume still gets a line.

| Setting | Default | Allowed |
|---|---|---|
| **Reset every** | Session | Session, Week, Month, Quarter, Year ([Reset periods](/manual/indicators/#reset-periods)) |
| **Band 1**, **Band 2**, **Band 3** | Off | See [Bands](/manual/indicators/averages-and-bands/#bands) |

<!-- regenerate:
theme: tokyo-night
window: 1600x1200 @2, scratchpad kit
state:
  key ctrl+shift+b
  omacharts chart set --symbol SPY --resolution 5m --session regular --template none
  omacharts chart indicator add vwap --bands 1,2,3
  move 795 595
capture: bin/headless-window cli chart screenshot --output /tmp/vwap.png; magick /tmp/vwap.png -quality 88 site/assets/manual/indicators/vwap.jpg
crop: none
-->
{% include figure.html src="/assets/manual/indicators/vwap.jpg" alt="SPY 5m over two sessions with a VWAP and its three bands" caption="SPY 5m with a session VWAP and its three bands." %}

## See also

- [Averages and bands](/manual/indicators/averages-and-bands/)
- [Indicators from the CLI](/manual/cli/#indicators)
