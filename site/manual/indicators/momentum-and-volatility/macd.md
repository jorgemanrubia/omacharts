---
title: "Moving Average Convergence Divergence"
description: "The gap between a fast and a slow average, with a signal line"
section: charting
parent: indicators/momentum-and-volatility
nav_order: 2
---

Its own strip. The gap between a fast and a slow exponential average of closes, a signal line that averages the gap, and a histogram of the difference between the two, either side of a guide at zero. The histogram is in the theme's up volume colour above zero and the down colour below. The scale fits what is on screen.

| Setting | Default | Allowed |
|---|---|---|
| **Fast length** | 12 | 1 to 199, below the slow length |
| **Slow length** | 26 | 2 to 200, above the fast length |
| **Signal length** | 9 | 1 to 200 |
| **Signal colour** | A contrasting colour from the theme | A palette name, or a custom colour |
| **Signal line style** | The MACD line's | Solid, Dashed, Dotted |
| **Pane height %** | 16% | 5 to 95% |

Moving the fast length up to the slow one pushes the slow one along, and the other way round.

<!-- regenerate:
theme: tokyo-night
window: 1600x1200 @2, scratchpad kit
state:
  key ctrl+shift+b
  omacharts chart set --symbol SPY --resolution 1D --template none
  omacharts chart indicator add macd
  move 795 595
capture: bin/headless-window cli chart screenshot --output /tmp/macd.png
crop: 0,812,1592,380 (then -quality 88 to site/assets/manual/indicators/macd.jpg)
-->
{% include figure.html src="/assets/manual/indicators/macd.jpg" alt="A MACD strip under SPY daily, with the MACD and signal lines over a green and red histogram around zero" caption="MACD 12 26 9 under SPY daily." %}

## See also

- [Momentum and volatility](/manual/indicators/momentum-and-volatility/)
- [Indicators from the CLI](/manual/cli/#indicators)
