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

Each paints buying and selling in a **Colouring**:

| Colouring | Paints |
|---|---|
| **Default** | The colouring chosen in Preferences, followed as it changes |
| **Vivid**, **Soft**, **Quiet** | That colouring's up colour for buying and its down colour for selling |
| **Monochrome** | One colour for buying and selling alike, the ink the chart's Monochrome candles are drawn in. Where a thing sits and what it says carry the sign. It is not **Quiet**, which keeps two muted colours |
| **Custom** | A colour of your own for each side |

With any colouring but **Custom**, the dialog shows one **Opacity** row under **Colouring**, how solid buying and selling are both drawn. The cumulative delta has no opacity, so it shows none. With **Custom** it shows a row for each colour the indicator draws instead: **Buying** and **Selling**, each a swatch and its own **Opacity**, or the cumulative delta's **Up** and **Down**, a swatch each. They start at the colours the indicator was drawn in a moment before, so nothing changes until you pick one. Choosing a colouring other than **Custom** again draws that colouring as it is, and lets the custom colours go once the dialog is closed. **Swap up and down colours** in Preferences swaps every colouring but **Custom**.

## See also

- [Indicators](/manual/indicators/)
- [Volume](/manual/indicators/volume/)
- [Order flow from the CLI](/manual/cli/indicators/order-flow/)
