---
title: "Relative Strength Index"
description: "Wilder's RSI of closes, 0 to 100, with overbought and oversold levels"
section: charting
parent: indicators/momentum-and-volatility
nav_order: 1
---

Wilder's RSI of closes, 0 to 100 on a fixed scale, with guides at the oversold level, 50 and the overbought level, and shading between the two levels.

| Drawn | Legend | Type to add | CLI |
|---|---|---|---|
| In a strip of its own | **RSI 14** | `rsi`, or `rsi 21` for the period | `rsi` |

<!-- regenerate:
theme: tokyo-night
window: 1600x1200 @2, scratchpad kit
state:
  key ctrl+shift+b
  omacharts chart set --symbol SPY --resolution 1D --template none
  omacharts chart indicator add rsi
  move 795 595
capture: bin/headless-window cli chart screenshot --output /tmp/rsi.png
crop: 0,812,1592,380 (then -quality 88 to site/assets/manual/indicators/rsi.jpg)
-->
{% include figure.html src="/assets/manual/indicators/rsi.jpg" alt="An RSI strip under SPY daily, the line between shaded guides at 30 and 70" caption="RSI 14 under SPY daily." %}

## Settings

| Setting | Default | Allowed |
|---|---|---|
| **Period** | 14 | 2 to 200 |
| **Overbought** | 70 | 0 to 100 |
| **Oversold** | 30 | 0 to 100, below overbought |
| **Pane height %** | 16% | 5 to 95% |
| **Colour**, **Thickness**, **Line style** | The next free colour, 1.5 px, solid | See [Colours and lines](/manual/indicators/#colours-and-lines) |

Moving one level past the other pushes the other along.

## See also

- [Momentum and volatility](/manual/indicators/momentum-and-volatility/)
- [Adding an indicator](/manual/indicators/#adding-an-indicator)
- [Indicators from the CLI](/manual/cli/#indicators)
