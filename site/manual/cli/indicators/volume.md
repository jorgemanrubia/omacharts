---
title: "Volume"
description: "The flags of volume and the cumulative delta. The volume profile and the market profile have pages of their own."
section: agents
parent: cli/indicators
nav_order: 2
---

The flags `chart indicator add` and `set` take for the [volume](/manual/indicators/volume/) family: `volume` and `cumulative_delta`, beside the ones [every indicator takes](/manual/cli/indicators/#flags). The volume profile and the market profile have pages of their own:

{% include children.html head="Indicator" what="What it covers" %}

| Flag | Takes | For |
|---|---|---|
| `--anchor P` | `session`, `week`, `month`, `quarter`, `year`; `never` for one running total | VWAP, volume profile, market profile, cumulative delta; `never` only for cumulative delta |
| `--cvd-style S` | `candles`, `line` | Cumulative delta |
| `--up-color C`, `--down-color C` | A colour | Volume |
| `--up-transparency F`, `--down-transparency F` | 0 to 1, or `default` to follow `--opacity` | Volume: how much of the chart shows through its rising or its falling bars |
| `--colouring C` | `default`, or `theme` (Vivid), `theme-soft`, `theme-quiet` for one of its own. `default` is the chart's colouring for volume (`chart` too, as earlier versions spelled it) and the one chosen in Preferences for a volume profile and a cumulative delta | Volume, volume profile, cumulative delta ([Volume](/manual/indicators/volume/)) |
| `--opacity F` | `0` to `1`, where `1` is fully opaque; 0.85 when not given | Volume: rising and falling bars alike, letting go of either's own transparency |
| `--height F` | 0.05 to 0.95; Volume defaults to 0.18, the other strips to 0.16 | Volume, cumulative delta, RSI, MACD, ATR, stochastic |

Volume and a volume profile are bars, not lines, so `--width` and `--line-style`
change nothing on them. `--color` on volume colours only its dot in the
legend.

```sh
omacharts chart indicator add volume --height 0.25
```
