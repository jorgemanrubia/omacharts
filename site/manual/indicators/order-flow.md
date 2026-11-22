---
title: "Order flow"
description: "The cumulative delta, the volume delta, the footprint and large orders, with every setting."
section: charting
parent: indicators
nav_order: 3
---

The order flow indicators show who took liquidity: buyers lifting the offer or sellers hitting the bid. They run from the widest view to the narrowest. The cumulative delta adds buying less selling up over a session, the volume delta shows it bar by bar, the footprint price by price inside each bar, and large orders single trades.

{% include children.html head="Indicator" what="What it shows" %}

## Which feeds they need

What was bought and what was sold is known only on a feed with volume at price, such as Rithmic or Sierra Chart ([Market data](/manual/market-data/)). On any other feed, Yahoo for one:

| Indicator | Without volume at price |
|---|---|
| Cumulative Delta, Volume Delta | Estimated from each bar's direction. The legend reads **CVD ≈** or **VD ≈** |
| Footprint, Large Orders | Draw nothing but the line **Needs a data feed with volume at price** |

## Colours

Every study whose colours say up or down, bought or sold, offers the same **Colouring** list, worked out the same way: the order flow indicators here, and [Volume](/manual/indicators/volume/volume/), a [volume profile](/manual/indicators/volume/volume-profile/)'s Ask volume - bid volume and its [drawing](/manual/drawings/volume-profile/). Each preset paints a study exactly as it paints the chart's bars on the same theme:

| Colouring | Paints |
|---|---|
| **Default** | As the chart's own bars, in the colouring set in its settings, followed as it changes |
| **Vivid**, **Soft**, **Quiet** | That colouring's up colour for buying, or a rise, and its down colour for selling, or a fall |
| **Monochrome** | One colour for buying and selling alike, the ink the chart's Monochrome candles are drawn in. Where a thing sits and what it says carry the sign. It is not **Quiet**, which keeps two muted colours |
| **Custom** | A colour of your own for each side |

With any colouring but **Custom**, the dialog shows one **Opacity** row under **Colouring**, how solid both colours are drawn. The cumulative delta and the volume profiles have no opacity there, so they show none. With **Custom** it shows a row for each colour the indicator draws instead: **Buying** and **Selling**, each a swatch and its own **Opacity**, or the cumulative delta's **Up** and **Down**, a swatch each. They start at the colours the indicator was drawn in a moment before, so nothing changes until you pick one. Choosing a colouring other than **Custom** again draws that colouring as it is, and lets the custom colours go once the dialog is closed. A chart whose bars are swapped (**Swap up and down colours** in its settings) wears every colouring but **Custom** swapped, on its studies too.

## See also

- [Indicators](/manual/indicators/)
- [Volume](/manual/indicators/volume/)
- [Order flow from the CLI](/manual/cli/indicators/order-flow/)
