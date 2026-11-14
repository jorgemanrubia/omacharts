---
title: "Stochastic"
description: "Where each close sits in the recent range, 0 to 100"
section: charting
parent: indicators/momentum-and-volatility
nav_order: 4
---

Its own strip. The slow stochastic. Where each close sits in the range of the last *%K length* bars, smoothed into %K, and %K averaged again into %D. 0 to 100 on a fixed scale, with guides and shading like the RSI's.

| Setting | Default | Allowed |
|---|---|---|
| **%K length** | 14 | 1 to 200 |
| **%K smoothing** | 3 | 1 to 50 |
| **%D smoothing** | 3 | 1 to 50 |
| **Overbought** | 80 | 0 to 100 |
| **Oversold** | 20 | 0 to 100, below overbought |
| **%D colour** | A contrasting colour from the theme | A palette name, or a custom colour |
| **%D line style** | %K's | Solid, Dashed, Dotted |
| **Pane height %** | 16% | 5 to 95% |

<!-- regenerate:
theme: tokyo-night
window: 1600x1200 @2, scratchpad kit
state:
  key ctrl+shift+b
  omacharts chart set --symbol SPY --resolution 1D --template none
  omacharts chart indicator add stochastic
  move 795 595
capture: bin/headless-window cli chart screenshot --output /tmp/stoch.png
crop: 0,812,1592,380 (then -quality 88 to site/assets/manual/indicators/stochastic.jpg)
-->
{% include figure.html src="/assets/manual/indicators/stochastic.jpg" alt="A stochastic strip under SPY daily, with %K and %D between shaded guides at 20 and 80" caption="Stoch 14 3 3 under SPY daily." %}

## See also

- [Momentum and volatility](/manual/indicators/momentum-and-volatility/)
- [Indicators from the CLI](/manual/cli/#indicators)
