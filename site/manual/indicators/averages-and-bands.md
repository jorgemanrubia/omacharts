---
title: "Averages and bands"
description: "Simple and exponential moving averages, the VWAP and Bollinger Bands, with every setting."
section: charting
parent: indicators
nav_order: 1
moved:
  simple-moving-average: /manual/indicators/averages-and-bands/sma/
  exponential-moving-average: /manual/indicators/averages-and-bands/ema/
  volume-weighted-average-price: /manual/indicators/averages-and-bands/vwap/
  bollinger-bands: /manual/indicators/averages-and-bands/bollinger-bands/
---

These indicators are averages drawn over the price, and the VWAP and Bollinger Bands can draw [bands](#bands) either side of theirs. A length is a whole number of bars, one or more, and every line has the **Colour**, **Thickness** and **Line style** rows of [Colours and lines](/manual/indicators/#colours-and-lines).

{% include children.html head="Indicator" what="What it shows" %}

## Bands

A VWAP and Bollinger Bands draw the same kind of band, a line a multiple of a standard deviation either side of a centre line. Each band has its own group in the indicator's settings, **Band** on a Bollinger and **Band 1** to **Band 3** on a VWAP.

| Row | Does | Range |
|---|---|---|
| **Show** | Draws the band | |
| **Standard deviations** | How far out it sits | 0.1 to 6, in tenths |
| **Shaded** | Fills the area between its edges | |
| **Shading %** | How solid that fill is | 0 to 100%; 16% by default |
| **Colour** | The band's colour. Left alone it is the indicator's | |
| **Thickness**, **Line style** | Its edges. No line leaves a shaded band with no outline | |

## See also

- [Indicators](/manual/indicators/)
- [Indicators from the CLI](/manual/cli/#indicators)
