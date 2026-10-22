---
title: "Indicators"
description: "Every indicator and its settings, and how indicators are added, arranged and changed."
section: charting
nav_order: 15
---

An indicator is a study of the chart's bars, such as a moving average or an RSI, drawn over the candles or in a strip of its own under the price. Each chart has its own indicators, or shows those of the [chart template](/manual/chart-templates/) it is on.

<!-- regenerate:
theme: tokyo-night
window: 1600x1200 @2, scratchpad kit (bin/headless-window, sandboxed HOME)
state:
  key ctrl+b
  omacharts chart set --template none
  omacharts chart indicator add sma --period 50
  omacharts chart indicator add sma --period 200
  omacharts chart indicator add volume
  omacharts chart indicator add rsi
  key ctrl+i
capture: bin/headless-window shot /tmp/dlg.png
crop: 240,40,1120,1120
-->
{% include figure.html src="/assets/manual/indicators/indicators-dialog.png" alt="The Indicators page of Chart settings" caption="The Indicators page of a chart with two averages, Volume and an RSI." %}

## Adding an indicator

**Add an indicator** is a search box. Open it with <kbd>Ctrl</kbd> + <kbd>Shift</kbd> + <kbd>I</kbd> over a chart, the **+** at the top of the [Indicators page](#the-indicators-page), or the **Add an indicator** row that page shows when the chart has none. Type a name, a short name or a related word (`poc` finds the volume profile).

<!-- regenerate:
theme: tokyo-night
window: 1600x1200 @2, scratchpad kit
state:
  key ctrl+b
  omacharts chart set --template none
  omacharts chart indicator add sma --period 50
  omacharts chart indicator add sma --period 200
  omacharts chart indicator add volume
  omacharts chart indicator add rsi
  key ctrl+shift+i
capture: bin/headless-window shot /tmp/picker.png
crop: 380,220,840,760
-->
{% include figure.html src="/assets/manual/indicators/add-an-indicator.png" alt="The Add an indicator search with its list of indicators" caption="The **Add an indicator** search, before anything is typed." %}

| Key | Does |
|---|---|
| Type | Narrows the list |
| <kbd>↑</kbd> / <kbd>↓</kbd> | Moves the selection without leaving the box |
| <kbd>Enter</kbd>, <kbd>Ctrl</kbd> + <kbd>Enter</kbd>, a click | Picks the selected indicator |
| <kbd>Esc</kbd>, a click outside | Closes the search, adding nothing |

Picking one puts it on the chart at its defaults and opens its settings in a dialog named after it, with **Cancel** and **Add** in the header. The chart changes as you change a setting. **Add** or <kbd>Ctrl</kbd> + <kbd>Enter</kbd> keeps the indicator. **Cancel**, <kbd>Esc</kbd> or a click outside removes it, and a chart that was on a template goes back on it.

<!-- regenerate:
theme: tokyo-night
window: 1600x1800 @2, scratchpad kit (tall enough that the whole dialog fits, Signal colour included)
state:
  move 795 895
  key ctrl+b
  omacharts chart set --template none
  omacharts chart indicator add sma --period 50
  omacharts chart indicator add sma --period 200
  omacharts chart indicator add volume
  omacharts chart indicator add rsi
  key ctrl+shift+i
  type macd
  key Return
capture: bin/headless-window shot /tmp/macd.png
crop: 208,104,1184,1592   (the whole dialog, 16px layout margin)
-->
{% include figure.html src="/assets/manual/indicators/macd-parameters.png" alt="The settings dialog of a MACD being added" caption="The settings of a MACD being added." %}

## Overlays and strips

An **overlay** is drawn over the price. A **strip** is a pane of its own under the price, with its own scale and its name in the top-left corner.

A strip takes a share of the chart's height, set with **Pane height %** in its settings.

| Gesture | Does |
|---|---|
| Drag the line between two rows | Resizes the strip the line belongs to. The new height is saved |
| Pointer in a strip | Shows its corner controls, top right |
| The up or down chevron in the corner | Moves the strip one row up or down, past the price plot when that is next. A chevron with nowhere to go is not shown |
| The × in the corner | Removes the indicator |

<!-- regenerate:
theme: tokyo-night
window: 1600x1200 @2, scratchpad kit
state:
  key ctrl+b
  omacharts chart set --template none
  omacharts chart indicator add sma --period 50
  omacharts chart indicator add sma --period 200
  omacharts chart indicator add volume
  omacharts chart indicator add rsi
  move 600 520
  move 602 522
capture: bin/headless-window shot /tmp/corner.png
crop: 0,950,1600,250
-->
{% include figure.html src="/assets/manual/indicators/strip-corner.png" alt="The corner controls of an RSI strip at the foot of a chart" caption="The corner controls of the bottom strip." %}

## The Indicators page

The Indicators page lists the focused chart's indicators under **On the chart**. Open it with <kbd>Ctrl</kbd> + <kbd>I</kbd> or the chart menu's **Indicators…**. It is also a page of [Chart settings](/manual/chart-settings/); <kbd>Alt</kbd> + <kbd>←</kbd> / <kbd>→</kbd> moves between the **Chart** and **Indicators** pages.

| Row part | Does |
|---|---|
| Handle | Drag the row to reorder. The drag works anywhere on the row that is not a control |
| Dot | The colour it is drawn in now |
| Title and subtitle | Its legend label, such as **RSI 14**, and its full name |
| Switch | Shows or hides it. A hidden indicator keeps its settings |
| Bin | Removes it |
| Click the row | Opens its settings over the list. Changes apply as they are made. The back arrow returns to the list |

The list is in the order of the chart, top to bottom. Overlays are listed in the order they are drawn, strips in the order they stack. When the chart has a strip, **Price** (**Candles, and everything drawn over them**) is a row too, in the price plot's place. Dragging a strip onto **Price** moves it to the other side of the price.

Above the list, **Chart template** names the template the chart is on, or **None**. A chart on a template lists the template's indicators without handles, switches or bins. **Edit Chart Template…** below the row changes them ([Chart templates](/manual/chart-templates/)).

## The legend

The legend lists the chart's indicators under the symbol, faint until the pointer comes near. Each line is a dot in the indicator's colour, its label and three buttons.

| Control | Does |
|---|---|
| Eye | Hides it, or shows it again. A hidden indicator's label stays, dimmed |
| Gear | Opens its settings in Chart settings |
| Bin | Removes it |
| Double-click the label | Opens its settings, as the gear does |

The label is the indicator's short name and its lengths, in the order its settings list them, such as **SMA 50** or **MACD 12 26 9**. A VWAP or a volume profile shows its period instead, such as **VWAP · Session**. When the period in use differs from the one chosen, the label names the one in use ([Reset periods](#reset-periods)).

## Colours and lines

An indicator with no chosen colour takes the first of the theme's sequence (Blue, Amber, Violet, Teal, Orange, Cyan) that no other indicator on the chart is using. When all are in use, they repeat. Adding an indicator never changes the colours already on the chart.

A **palette name** (Blue, Amber, Violet, Teal, Rose, Green, Orange, Cyan) is the theme's colour of that name and follows the theme when it changes. A **custom** colour, picked exactly or typed as a hex such as `#e0af68`, stays the same in every theme.

Each **Colour** row opens the theme's palette ([Themes and colours](/manual/themes/)). **Custom…** under it picks an exact colour. **Reset** beside the swatch goes back to the automatic colour.

| Row | Choices | Default |
|---|---|---|
| **Colour** | A palette name, or a custom colour | The next free colour of the sequence |
| **Thickness** | No line, 1, 1.5, 2.5, 4 px | 1.5 px |
| **Line style** | Solid, Dashed, Dotted | Solid |

A MACD's signal line and a stochastic's %D have a second colour row. Left alone, it is a contrasting colour from the theme.

## Indicators on a chart on a template

Changing an indicator on a chart that is on a template takes the chart off the template, keeping the indicators it showed ([How a chart on a template behaves](/manual/chart-templates/#how-a-chart-on-a-template-behaves)).

## Reset periods

The VWAP and the volume profile start over at the beginning of each period, chosen with **Reset every** in their settings.

| Period | Starts over |
|---|---|
| **Session** | At each session's open. A future's session opens the evening before and counts as one period |
| **Week** | On Monday |
| **Month** | On the 1st |
| **Quarter** | On 1 January, April, July and October |
| **Year** | On 1 January |

A period needs at least ten bars at the chart's resolution. One with fewer uses the finest period that has ten, and the legend names it. A session VWAP becomes weekly on a 4h chart, monthly on a daily chart and quarterly on a weekly chart. The chosen period is kept, and comes back on an intraday resolution.

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

## The indicators

A length is a whole number of bars, one or more. Every indicator drawn as a line has the **Colour**, **Thickness** and **Line style** rows of [Colours and lines](#colours-and-lines). Volume and the volume profile are bars and have no thickness or style.

### Simple Moving Average

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

### Exponential Moving Average

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

### Volume Weighted Average Price

Drawn over the price. The average price paid since the period began, each bar's typical price (the mean of high, low and close) weighted by its volume. Three bands at 1, 1.5 and 2 standard deviations. When on, the first is shaded with no outline, the second dashed, the third a plain line. An instrument with no volume still gets a line.

| Setting | Default | Allowed |
|---|---|---|
| **Reset every** | Session | Session, Week, Month, Quarter, Year ([Reset periods](#reset-periods)) |
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

### Bollinger Bands

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

### Volume Profile

Drawn over the price. For each reset period, how much volume traded at each price, as a histogram against the left edge of the period, at most 40% of its width. The rows in the **value area**, the band around the busiest price that holds the given share of the period's volume, are drawn stronger. The **point of control**, the busiest row, is a line across the profile.

| Setting | Default | Allowed |
|---|---|---|
| **Reset every** | Session | Session, Week, Month, Quarter, Year ([Reset periods](#reset-periods)) |
| **Automatic**, **Rows** | Automatic | A number of rows per period, 4 to 400 |
| **Value area %** | 70% | 10 to 100% |
| **Profile** colour | The next free colour | A palette name, or a custom colour |
| **Point of control** colour | A contrasting colour from the theme | A palette name, or a custom colour |

**Automatic** gives every period rows of the same height, a round multiple of the instrument's price step. With **Automatic** off, **Rows** is the number of rows each period is divided into.

<!-- regenerate:
theme: tokyo-night
window: 1600x1200 @2, scratchpad kit
state:
  key ctrl+b
  omacharts chart set --symbol SPY --resolution 5m --session regular --template none
  omacharts chart indicator add volume_profile
  move 795 595
capture: bin/headless-window cli chart screenshot --output /tmp/vp.png; magick /tmp/vp.png -quality 88 site/assets/manual/indicators/volume-profile.jpg
crop: none
-->
{% include figure.html src="/assets/manual/indicators/volume-profile.jpg" alt="SPY 5m with a volume profile at the start of each session" caption="SPY 5m with a session volume profile." %}

### Volume

Its own strip. Each bar's volume, in the candles' up or down colour, dimmed.

| Setting | Default | Allowed |
|---|---|---|
| **Pane height %** | 18% | 5 to 95% |
| **Up** colour | The candles' up colour, dimmed | A palette name, or a custom colour |
| **Down** colour | The candles' down colour, dimmed | A palette name, or a custom colour |

The **Default** [chart template](/manual/chart-templates/) holds Volume.

<!-- regenerate:
theme: tokyo-night
window: 1600x1200 @2, scratchpad kit
state:
  key ctrl+b
  omacharts chart set --symbol SPY --resolution 1D --template none
  omacharts chart indicator add volume
  move 795 595
capture: bin/headless-window cli chart screenshot --output /tmp/vol.png
crop: 0,812,1592,380 (then -quality 88 to site/assets/manual/indicators/volume.jpg)
-->
{% include figure.html src="/assets/manual/indicators/volume.jpg" alt="The volume strip under a SPY daily chart, bars in dimmed green and red" caption="The Volume strip under SPY daily." %}

### Relative Strength Index

Its own strip. Wilder's RSI of closes, 0 to 100 on a fixed scale, with guides at the oversold level, 50 and the overbought level, and shading between the two levels.

| Setting | Default | Allowed |
|---|---|---|
| **Period** | 14 | 2 to 200 |
| **Overbought** | 70 | 0 to 100 |
| **Oversold** | 30 | 0 to 100, below overbought |
| **Pane height %** | 16% | 5 to 95% |

Moving one level past the other pushes the other along.

<!-- regenerate:
theme: tokyo-night
window: 1600x1200 @2, scratchpad kit
state:
  key ctrl+b
  omacharts chart set --symbol SPY --resolution 1D --template none
  omacharts chart indicator add rsi
  move 795 595
capture: bin/headless-window cli chart screenshot --output /tmp/rsi.png
crop: 0,812,1592,380 (then -quality 88 to site/assets/manual/indicators/rsi.jpg)
-->
{% include figure.html src="/assets/manual/indicators/rsi.jpg" alt="An RSI strip under SPY daily, the line between shaded guides at 30 and 70" caption="RSI 14 under SPY daily." %}

### Moving Average Convergence Divergence

Its own strip. The gap between a fast and a slow exponential average of closes, a signal line that averages the gap, and a histogram of the difference between the two, either side of a guide at zero. The histogram is in the theme's up volume colour above zero and the down colour below. The scale fits what is on screen.

| Setting | Default | Allowed |
|---|---|---|
| **Fast length** | 12 | 1 to 199, below the slow length |
| **Slow length** | 26 | 2 to 200, above the fast length |
| **Signal length** | 9 | 1 to 200 |
| **Signal colour** | A contrasting colour from the theme | A palette name, or a custom colour |
| **Pane height %** | 16% | 5 to 95% |

Moving the fast length up to the slow one pushes the slow one along, and the other way round.

<!-- regenerate:
theme: tokyo-night
window: 1600x1200 @2, scratchpad kit
state:
  key ctrl+b
  omacharts chart set --symbol SPY --resolution 1D --template none
  omacharts chart indicator add macd
  move 795 595
capture: bin/headless-window cli chart screenshot --output /tmp/macd.png
crop: 0,812,1592,380 (then -quality 88 to site/assets/manual/indicators/macd.jpg)
-->
{% include figure.html src="/assets/manual/indicators/macd.jpg" alt="A MACD strip under SPY daily, with the MACD and signal lines over a green and red histogram around zero" caption="MACD 12 26 9 under SPY daily." %}

### Average True Range

Its own strip. Wilder's average of the true range (the bar's range, stretched to the previous close when the bar gapped), in price units. The scale fits what is on screen.

| Setting | Default | Allowed |
|---|---|---|
| **Period** | 14 | 2 to 200 |
| **Pane height %** | 16% | 5 to 95% |

<!-- regenerate:
theme: tokyo-night
window: 1600x1200 @2, scratchpad kit
state:
  key ctrl+b
  omacharts chart set --symbol SPY --resolution 1D --template none
  omacharts chart indicator add atr
  move 795 595
capture: bin/headless-window cli chart screenshot --output /tmp/atr.png
crop: 0,812,1592,380 (then -quality 88 to site/assets/manual/indicators/atr.jpg)
-->
{% include figure.html src="/assets/manual/indicators/atr.jpg" alt="An ATR strip under SPY daily" caption="ATR 14 under SPY daily." %}

### Stochastic

Its own strip. The slow stochastic. Where each close sits in the range of the last *%K length* bars, smoothed into %K, and %K averaged again into %D. 0 to 100 on a fixed scale, with guides and shading like the RSI's.

| Setting | Default | Allowed |
|---|---|---|
| **%K length** | 14 | 1 to 200 |
| **%K smoothing** | 3 | 1 to 50 |
| **%D smoothing** | 3 | 1 to 50 |
| **Overbought** | 80 | 0 to 100 |
| **Oversold** | 20 | 0 to 100, below overbought |
| **%D colour** | A contrasting colour from the theme | A palette name, or a custom colour |
| **Pane height %** | 16% | 5 to 95% |

<!-- regenerate:
theme: tokyo-night
window: 1600x1200 @2, scratchpad kit
state:
  key ctrl+b
  omacharts chart set --symbol SPY --resolution 1D --template none
  omacharts chart indicator add stochastic
  move 795 595
capture: bin/headless-window cli chart screenshot --output /tmp/stoch.png
crop: 0,812,1592,380 (then -quality 88 to site/assets/manual/indicators/stochastic.jpg)
-->
{% include figure.html src="/assets/manual/indicators/stochastic.jpg" alt="A stochastic strip under SPY daily, with %K and %D between shaded guides at 20 and 80" caption="Stoch 14 3 3 under SPY daily." %}

## See also

- [Indicators from the CLI](/manual/cli/#indicators)
