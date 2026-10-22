---
title: "Averages and bands"
description: "Simple and exponential moving averages, the VWAP and Bollinger Bands, with every setting."
section: charting
parent: indicators
nav_order: 1
---

These indicators are averages drawn over the price, and the VWAP and Bollinger Bands can draw [bands](#bands) either side of theirs. A length is a whole number of bars, one or more, and every line has the **Colour**, **Thickness** and **Line style** rows of [Colours and lines](/manual/indicators/#colours-and-lines).

## Simple Moving Average

Drawn over the price. The average close of the last *period* bars. It has no value until there are that many.

| Setting | Default | Allowed |
|---|---|---|
| **Period** | 50 | 1 to 500 |

<!-- regenerate:
theme: tokyo-night
window: 1600x1200 @2, scratchpad kit
state:
  key ctrl+b
  omacharts chart set --symbol SPY --resolution 1D --template none
  omacharts chart indicator add sma --period 50
  omacharts chart indicator add sma --period 200
  move 795 595
capture: bin/headless-window cli chart screenshot --output /tmp/sma.png; magick /tmp/sma.png -quality 88 site/assets/manual/indicators/sma.jpg
crop: none
-->
{% include figure.html src="/assets/manual/indicators/sma.jpg" alt="SPY daily with a 50-day average in blue and a 200-day average in amber" caption="SPY daily with SMA 50 and SMA 200." %}

## Exponential Moving Average

Drawn over the price. An average of closes that weights the latest bars most, so it turns sooner than a simple average of the same length.

| Setting | Default | Allowed |
|---|---|---|
| **Period** | 21 | 1 to 500 |

<!-- regenerate:
theme: tokyo-night
window: 1600x1200 @2, scratchpad kit
state:
  key ctrl+b
  omacharts chart set --symbol SPY --resolution 1D --template none
  omacharts chart indicator add ema
  move 795 595
capture: bin/headless-window cli chart screenshot --output /tmp/ema.png; magick /tmp/ema.png -quality 88 site/assets/manual/indicators/ema.jpg
crop: none
-->
{% include figure.html src="/assets/manual/indicators/ema.jpg" alt="SPY daily with a 21-day exponential average" caption="SPY daily with EMA 21." %}

## Volume Weighted Average Price

Drawn over the price. The average price paid since the period began, each bar's typical price (the mean of high, low and close) weighted by its volume. Three bands at 1, 1.5 and 2 standard deviations. When on, the first is shaded with no outline, the second dashed, the third a plain line. An instrument with no volume still gets a line.

| Setting | Default | Allowed |
|---|---|---|
| **Reset every** | Session | Session, Week, Month, Quarter, Year ([Reset periods](/manual/indicators/#reset-periods)) |
| **Band 1**, **Band 2**, **Band 3** | Off | See [Bands](#bands) |

<!-- regenerate:
theme: tokyo-night
window: 1600x1200 @2, scratchpad kit
state:
  key ctrl+b
  omacharts chart set --symbol SPY --resolution 5m --session regular --template none
  omacharts chart indicator add vwap --bands 1,2,3
  move 795 595
capture: bin/headless-window cli chart screenshot --output /tmp/vwap.png; magick /tmp/vwap.png -quality 88 site/assets/manual/indicators/vwap.jpg
crop: none
-->
{% include figure.html src="/assets/manual/indicators/vwap.jpg" alt="SPY 5m over two sessions with a VWAP and its three bands" caption="SPY 5m with a session VWAP and its three bands." %}

## Bollinger Bands

Drawn over the price. A simple average of closes with a band a multiple of their standard deviation either side, shaded and outlined.

| Setting | Default | Allowed |
|---|---|---|
| **Period** | 20 | 1 to 500 |
| **Standard deviations** | 2 | 0.1 to 6 |
| **Band** | On | See [Bands](#bands) |

The label includes the multiple, as in **BB 20 2** and **BB 20 2.5**.

<!-- regenerate:
theme: tokyo-night
window: 1600x1200 @2, scratchpad kit
state:
  key ctrl+b
  omacharts chart set --symbol SPY --resolution 1D --template none
  omacharts chart indicator add bollinger
  move 795 595
capture: bin/headless-window cli chart screenshot --output /tmp/bb.png; magick /tmp/bb.png -quality 88 site/assets/manual/indicators/bollinger.jpg
crop: none
-->
{% include figure.html src="/assets/manual/indicators/bollinger.jpg" alt="SPY daily with shaded Bollinger Bands around the candles" caption="SPY daily with BB 20 2." %}

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
