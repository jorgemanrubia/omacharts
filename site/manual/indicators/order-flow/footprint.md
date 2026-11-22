---
title: "Footprint"
description: "Each bar's delta at every price, drawn beside a narrow candle once the bars are wide enough"
section: charting
parent: indicators/order-flow
nav_order: 3
---

Each bar's volume at the ask less its volume at the bid, at every price it traded, drawn inside the bar. The candle narrows to the bar's left edge and is drawn hollow. Beside it each price is a row, its bar as long as its delta and its number at its left, in the buying colour when more was bought there and the selling colour when more was sold. Zoomed out, the chart shows its ordinary candles.

| Drawn | Legend | Type to add | CLI |
|---|---|---|---|
| Over the candles, from a bar width | **FP** | `fp` | `footprint` |

## Settings

| Setting | Default | Allowed |
|---|---|---|
| **Show from bar width** | 64 px | 24 to 400 px. Narrower bars are drawn as candles |
| **Intensity** | On | Off fills every row alike, at the side's **Opacity** |
| **Colouring** | **Default** | **Vivid**, **Soft**, **Quiet** or **Monochrome** ([Colours](/manual/indicators/order-flow/#colours)) |
| **Buying** | The colouring's up colour, at 85% opacity | A palette name, or a custom colour, with its **Opacity** |
| **Selling** | The colouring's down colour, at 85% opacity | The same |

## Rows

A row is one tick while the ticks are at least 3 px tall. When they are thinner, a row holds 2, 4, 5, 10, 20, 25, 50 or more ticks together, so the rows stay readable, and every bar's rows cover the same prices. A row's number is left out once the rows are under 10 px tall, and written short past a thousand: **1.2K**, **-35K**, **2.5M**.

## Intensity

With **Intensity** on, a row is filled more solidly the bigger its delta, in five steps from faint up to the side's **Opacity**. The strongest is measured against the biggest delta of any row on screen, so a row's length and its strength say the same thing, and scrolling to a busier stretch makes the rest fainter. Each row keeps a thin outline in its own colour, so a faint row is still seen.

## See also

- [Order flow](/manual/indicators/order-flow/)
- [Volume profile](/manual/indicators/volume/volume-profile/), the same measure over a session
- [Footprint from the CLI](/manual/cli/indicators/order-flow/footprint/)
