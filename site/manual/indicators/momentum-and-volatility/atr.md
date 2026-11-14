---
title: "Average True Range"
description: "The average true range of the bars, in price units"
section: charting
parent: indicators/momentum-and-volatility
nav_order: 3
---

Its own strip. Wilder's average of the true range (the bar's range, stretched to the previous close when the bar gapped), in price units. The scale fits what is on screen.

| Setting | Default | Allowed |
|---|---|---|
| **Period** | 14 | 2 to 200 |
| **Pane height %** | 16% | 5 to 95% |

<!-- regenerate:
theme: tokyo-night
window: 1600x1200 @2, scratchpad kit
state:
  key ctrl+shift+b
  omacharts chart set --symbol SPY --resolution 1D --template none
  omacharts chart indicator add atr
  move 795 595
capture: bin/headless-window cli chart screenshot --output /tmp/atr.png
crop: 0,812,1592,380 (then -quality 88 to site/assets/manual/indicators/atr.jpg)
-->
{% include figure.html src="/assets/manual/indicators/atr.jpg" alt="An ATR strip under SPY daily" caption="ATR 14 under SPY daily." %}

## See also

- [Momentum and volatility](/manual/indicators/momentum-and-volatility/)
- [Indicators from the CLI](/manual/cli/#indicators)
