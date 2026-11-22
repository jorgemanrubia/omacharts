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

Each paints buying and selling in a **Colouring**: **Default**, the colouring chosen in Preferences, followed as it changes, or **Vivid**, **Soft**, **Quiet** or **Monochrome** of its own. **Monochrome** paints with one ink, the theme's neutral, solid for buying and faint for selling, so the sign still reads without colour. **Buying** and **Selling** set either side's colour apart from the colouring, each with its **Opacity**. **Swap up and down colours** in Preferences swaps them too.

## See also

- [Indicators](/manual/indicators/)
- [Volume](/manual/indicators/volume/)
- [Order flow from the CLI](/manual/cli/indicators/order-flow/)
