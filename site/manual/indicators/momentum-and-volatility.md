---
title: "Momentum and volatility"
description: "The RSI, MACD, ATR and stochastic, with every setting."
section: charting
parent: indicators
nav_order: 3
---

Momentum and volatility indicators each have a [strip](/manual/indicators/#overlays-and-strips) of their own under the price. A length is a whole number of bars, one or more, and every line has the **Colour**, **Thickness** and **Line style** rows of [Colours and lines](/manual/indicators/#colours-and-lines).

## Relative Strength Index

Its own strip. Wilder's RSI of closes, 0 to 100 on a fixed scale, with guides at the oversold level, 50 and the overbought level, and shading between the two levels.

| Setting | Default | Allowed |
|---|---|---|
| **Period** | 14 | 2 to 200 |
| **Overbought** | 70 | 0 to 100 |
| **Oversold** | 30 | 0 to 100, below overbought |
| **Pane height %** | 16% | 5 to 95% |

Moving one level past the other pushes the other along.

<!-- regenerate:
theme: tokyo-night
window: 1600x1200 @2, scratchpad kit
state:
  key ctrl+b
  omacharts chart set --symbol SPY --resolution 1D --template none
  omacharts chart indicator add rsi
  move 795 595
capture: bin/headless-window cli chart screenshot --output /tmp/rsi.png
crop: 0,812,1592,380 (then -quality 88 to site/assets/manual/indicators/rsi.jpg)
-->
{% include figure.html src="/assets/manual/indicators/rsi.jpg" alt="An RSI strip under SPY daily, the line between shaded guides at 30 and 70" caption="RSI 14 under SPY daily." %}

## Moving Average Convergence Divergence

Its own strip. The gap between a fast and a slow exponential average of closes, a signal line that averages the gap, and a histogram of the difference between the two, either side of a guide at zero. The histogram is in the theme's up volume colour above zero and the down colour below. The scale fits what is on screen.

| Setting | Default | Allowed |
|---|---|---|
| **Fast length** | 12 | 1 to 199, below the slow length |
| **Slow length** | 26 | 2 to 200, above the fast length |
| **Signal length** | 9 | 1 to 200 |
| **Signal colour** | A contrasting colour from the theme | A palette name, or a custom colour |
| **Pane height %** | 16% | 5 to 95% |

Moving the fast length up to the slow one pushes the slow one along, and the other way round.

<!-- regenerate:
theme: tokyo-night
window: 1600x1200 @2, scratchpad kit
state:
  key ctrl+b
  omacharts chart set --symbol SPY --resolution 1D --template none
  omacharts chart indicator add macd
  move 795 595
capture: bin/headless-window cli chart screenshot --output /tmp/macd.png
crop: 0,812,1592,380 (then -quality 88 to site/assets/manual/indicators/macd.jpg)
-->
{% include figure.html src="/assets/manual/indicators/macd.jpg" alt="A MACD strip under SPY daily, with the MACD and signal lines over a green and red histogram around zero" caption="MACD 12 26 9 under SPY daily." %}

## Average True Range

Its own strip. Wilder's average of the true range (the bar's range, stretched to the previous close when the bar gapped), in price units. The scale fits what is on screen.

| Setting | Default | Allowed |
|---|---|---|
| **Period** | 14 | 2 to 200 |
| **Pane height %** | 16% | 5 to 95% |

<!-- regenerate:
theme: tokyo-night
window: 1600x1200 @2, scratchpad kit
state:
  key ctrl+b
  omacharts chart set --symbol SPY --resolution 1D --template none
  omacharts chart indicator add atr
  move 795 595
capture: bin/headless-window cli chart screenshot --output /tmp/atr.png
crop: 0,812,1592,380 (then -quality 88 to site/assets/manual/indicators/atr.jpg)
-->
{% include figure.html src="/assets/manual/indicators/atr.jpg" alt="An ATR strip under SPY daily" caption="ATR 14 under SPY daily." %}

## Stochastic

Its own strip. The slow stochastic. Where each close sits in the range of the last *%K length* bars, smoothed into %K, and %K averaged again into %D. 0 to 100 on a fixed scale, with guides and shading like the RSI's.

| Setting | Default | Allowed |
|---|---|---|
| **%K length** | 14 | 1 to 200 |
| **%K smoothing** | 3 | 1 to 50 |
| **%D smoothing** | 3 | 1 to 50 |
| **Overbought** | 80 | 0 to 100 |
| **Oversold** | 20 | 0 to 100, below overbought |
| **%D colour** | A contrasting colour from the theme | A palette name, or a custom colour |
| **Pane height %** | 16% | 5 to 95% |

<!-- regenerate:
theme: tokyo-night
window: 1600x1200 @2, scratchpad kit
state:
  key ctrl+b
  omacharts chart set --symbol SPY --resolution 1D --template none
  omacharts chart indicator add stochastic
  move 795 595
capture: bin/headless-window cli chart screenshot --output /tmp/stoch.png
crop: 0,812,1592,380 (then -quality 88 to site/assets/manual/indicators/stochastic.jpg)
-->
{% include figure.html src="/assets/manual/indicators/stochastic.jpg" alt="A stochastic strip under SPY daily, with %K and %D between shaded guides at 20 and 80" caption="Stoch 14 3 3 under SPY daily." %}

## See also

- [Indicators](/manual/indicators/)
- [Indicators from the CLI](/manual/cli/#indicators)
