---
title: "Momentum and volatility"
description: "The flags of the RSI, MACD, ATR and stochastic."
section: agents
parent: cli/indicators
nav_order: 4
---

The flags `chart indicator add` and `set` take for [momentum and volatility](/manual/indicators/momentum-and-volatility/): `rsi`, `macd`, `atr` and `stochastic`, beside the ones [every indicator takes](/manual/cli/indicators/#flags).

| Flag | Takes | For |
|---|---|---|
| `--period N` | Bars | SMA, EMA, Bollinger, RSI, ATR; the stochastic's %K length |
| `--fast N`, `--slow N`, `--signal N` | Bars | MACD |
| `--signal-color C` | A colour | MACD |
| `--signal-transparency F` | 0 to 1, or `default` to follow `--transparency` | MACD: how much of the chart shows through its signal line |
| `--signal-style S` | `solid`, `dashed`, `dotted`; the MACD line's when not given | MACD |
| `--k-smooth N`, `--d-period N` | Bars | Stochastic |
| `--d-color C` | A colour | Stochastic |
| `--d-transparency F` | 0 to 1, or `default` to follow `--transparency` | Stochastic: how much of the chart shows through its %D line |
| `--d-style S` | `solid`, `dashed`, `dotted`; %K's when not given | Stochastic |
| `--overbought F`, `--oversold F` | 0 to 100 | RSI, stochastic |
| `--height F` | 0.05 to 0.95; Volume defaults to 0.18, the other strips to 0.16 | Volume, cumulative delta, volume delta, RSI, MACD, ATR, stochastic |

```sh
omacharts chart indicator set rsi --period 21 --overbought 80
omacharts chart indicator add macd --signal-color Amber
omacharts chart indicator add atr --period 20
omacharts chart indicator add stochastic --k-smooth 5
```

Refusals:

| Refused | Exit | Message |
|---|---|---|
| `--fast` not below `--slow`, in one command or against the length the chart already holds | 2 | `--fast 26 is not below --slow 12` |
| `--oversold` not below `--overbought` | 2 | `oversold 80 is not below overbought 70` |
