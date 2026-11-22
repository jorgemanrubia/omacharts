---
title: "Large Orders"
description: "A bubble at each trade bigger than a size you choose"
section: charting
parent: indicators/order-flow
nav_order: 4
---

A bubble at the price and bar of every trade bigger than a size you choose, in the buying colour for a buyer who lifted the offer and the selling colour for a seller who hit the bid. A bubble's area is in proportion to the trade's size, against the other large orders it is compared with.

| Drawn | Legend | Type to add | CLI |
|---|---|---|---|
| Over the candles | **Large > 50 · 20** | `large` | `large_orders` |

## Settings

| Setting | Default | Allowed |
|---|---|---|
| **RTH threshold** | 50 | A trade in the regular session counts when it is bigger than this, in contracts or shares. 0 counts every trade |
| **ETH threshold** | 20 | The same outside the regular session |
| **Size against** | Session | **Visible bars**: the large orders on screen, so bubbles change size as the chart scrolls. **Session**: the session's. **RTH or ETH**: the session's regular or extended hours', each against its own |
| **Combine same-time prints** | On | Off makes every print its own order |
| **Colouring** | **Default** | **Vivid**, **Soft** or **Quiet** ([Colours](/manual/indicators/order-flow/#colours)) |
| **Buying** | The colouring's up colour, at 55% opacity | A palette name, or a custom colour, with its **Opacity** |
| **Selling** | The colouring's down colour, at 55% opacity | The same |

## Combining prints

An order that takes more than the best price offers fills against several resting orders, and the exchange reports each fill as a print of its own, at the same moment and on the same side. With **Combine same-time prints** on, a run of such prints is one order, its size the sum of theirs and its price their average weighted by size. A print whose side is not known is always an order of its own.

## How far back

Large orders start where the feed's trades start. On Rithmic, the trades of earlier days are fetched once from its history and kept on disk by symbol and day, newest first, until a day comes back empty. Rithmic publishes no limit on how far back that goes. On Sierra Chart they are read from its own data files. Nothing is shown for days before the trades begin, and the chart is drawn first: the bubbles appear once their trades are read.

## See also

- [Order flow](/manual/indicators/order-flow/)
- [Footprint](/manual/indicators/order-flow/footprint/)
- [Large Orders from the CLI](/manual/cli/indicators/order-flow/large-orders/)
