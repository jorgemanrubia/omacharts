---
title: "Averages and bands"
description: "The flags of the moving averages, the VWAP and Bollinger Bands."
section: agents
parent: cli/indicators
nav_order: 1
---

The flags `chart indicator add` and `set` take for the [averages and bands](/manual/indicators/averages-and-bands/): `sma`, `ema`, `vwap` and `bollinger`, beside the ones [every indicator takes](/manual/cli/indicators/#flags).

| Flag | Takes | For |
|---|---|---|
| `--period N` | Bars | SMA, EMA, Bollinger, RSI, ATR; the stochastic's %K length |
| `--deviations F` | 0 to 50 | Bollinger |
| `--anchor P` | `session`, `week`, `month`, `quarter`, `year`; `never` for one running total | VWAP, volume profile, market profile, cumulative delta; `never` only for cumulative delta |
| `--bands LIST` | `1,2,3` or any of them; `none` | VWAP; `1` or `none` for Bollinger |
| `--band-alpha F` | 0 to 1; 0.16 when not given | VWAP, Bollinger |
| `--band-transparency F` | 0 to 1, or `default` for solid | VWAP, Bollinger: how much of the chart shows through every band's edges |

`--bands` chooses which bands are drawn, `--band-alpha` how solid every
band's shading is, `--band-transparency` how much shows through their edges,
and `--deviations` where a Bollinger's bands sit. A band's colour and edges,
and a VWAP band's multiple, are set only in the window.

```sh
omacharts chart indicator add sma --period 200 --color Amber
omacharts chart indicator add ema --period 9
omacharts chart indicator add vwap --bands 1,2 --band-alpha 0.3
omacharts chart indicator add bollinger --deviations 2.5
```

Refusals:

| Refused | Exit | Message |
|---|---|---|
| `--deviations` on a VWAP: `set vwap --deviations 2` | 2 | `--deviations sets the one band a Bollinger has; VWAP has 3 of them` |
