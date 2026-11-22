---
title: "Volume"
description: "The flags of volume. The volume profile and the market profile have pages of their own."
section: agents
parent: cli/indicators
nav_order: 2
---

The flags `chart indicator add` and `set` take for the [volume](/manual/indicators/volume/) family: `volume`, beside the ones [every indicator takes](/manual/cli/indicators/#flags). The volume profile and the market profile have pages of their own:

{% include children.html head="Indicator" what="What it covers" %}

| Flag | Takes | For |
|---|---|---|
| `--up-color C`, `--down-color C` | A colour. Given on a colouring other than `custom`, it makes the colouring `custom`, the other colour kept as drawn | Volume, cumulative delta |
| `--up-transparency F`, `--down-transparency F` | 0 to 1, or `default` to follow `--opacity` | Volume: how much of the chart shows through its rising or its falling bars |
| `--colouring C` | `default`, or `theme` (Vivid), `theme-soft`, `theme-quiet`, `theme-mono` (or `monochrome`) for one of its own, or `custom`. `default` (or `chart`) paints it as the chart's bars, and every preset as the chart's bars in it, swapped when they are: the one value set every study takes ([Colours](/manual/indicators/order-flow/#colours)). `custom` keeps each colour its own, starting from the colours drawn a moment before; any other lets the custom colours go | Volume, volume profile ([Volume](/manual/indicators/volume/)) |
| `--opacity F` | `0` to `1`, where `1` is fully opaque; 0.85 when not given | Volume: rising and falling bars alike, letting go of either's own transparency |
| `--height F` | 0.05 to 0.95; Volume defaults to 0.18, the other strips to 0.16 | Volume, cumulative delta, volume delta, RSI, MACD, ATR, stochastic |

Volume and a volume profile are bars, not lines, so `--width` and `--line-style`
change nothing on them. `--color` on volume colours only its dot in the
legend.

```sh
omacharts chart indicator add volume --height 0.25
```
