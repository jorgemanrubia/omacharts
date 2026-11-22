---
title: "Order flow"
description: "The flags of the cumulative delta, the volume delta, the footprint and large orders."
section: agents
parent: cli/indicators
nav_order: 3
---

The flags `chart indicator add` and `set` take for [order flow](/manual/indicators/order-flow/): `cvd`, `volume_delta`, `footprint` and `large_orders`, beside the ones [every indicator takes](/manual/cli/indicators/#flags). Each has a page with its own:

{% include children.html head="Indicator" what="What it covers" %}

## Colours

The volume delta, the footprint and large orders paint buying and selling, and take these. None of them has a line, so `--color`, `--transparency`, `--width` and `--line-style` are refused.

| Flag | Takes | For |
|---|---|---|
| `--colouring C` | `default` (or `chart`), as the chart's bars, or `theme` (Vivid), `theme-soft`, `theme-quiet`, `theme-mono` (or `monochrome`: one colour for both, the Monochrome candles' ink), or `custom` | The colouring buying and selling are taken from. `custom` keeps each colour its own, starting from the colours drawn a moment before; any other lets the custom colours go |
| `--buy-color C`, `--sell-color C` | A colour, or `none` to let it go | Buying's or selling's colour. Given on a colouring other than `custom`, it makes the colouring `custom`, the other side kept at the colour it was drawn in |
| `--buy-transparency F`, `--sell-transparency F` | 0 to 1, or `default` for the indicator's own | How much of the chart shows through buying or selling |

```
$ omacharts chart indicator set volume_delta --colouring theme-soft --sell-color Rose --sell-transparency 0.2
```

The colouring is `theme-soft` for a moment, then `--sell-color` makes it `custom`, buying kept in Soft's up colour.

`list --json` reads them back under `params.inks`, which is left out while every one is its default:

```json
"inks": { "colouring": "theme-soft", "sell": { "kind": "swatch", "name": "Rose" }, "sell_alpha": 0.8 }
```

A transparency comes back as its opacity, `sell_alpha`, 1 less the transparency given.

## See also

- [Order flow](/manual/indicators/order-flow/)
- [Indicators from the CLI](/manual/cli/indicators/)
