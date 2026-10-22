---
title: "Indicators"
description: "How indicators are added, arranged and changed, and the families they come in."
section: charting
nav_order: 15
---

An indicator is something drawn from the chart's bars, such as a moving average, an RSI or the shading of the extended hours, over the candles or in a strip of its own under the price. Each chart has its own indicators, or shows those of the [chart template](/manual/chart-templates/) it is on.

The indicators come in families, each with a page that has every indicator in it and every setting:

| Family | Indicators |
|---|---|
| [Averages and bands](/manual/indicators/averages-and-bands/) | Simple Moving Average, Exponential Moving Average, Volume Weighted Average Price, Bollinger Bands |
| [Volume](/manual/indicators/volume/) | Volume, Volume Profile |
| [Momentum and volatility](/manual/indicators/momentum-and-volatility/) | Relative Strength Index, MACD, Average True Range, Stochastic |
| [Sessions and time](/manual/indicators/session-times/) | Session Times |

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

**Add an indicator** is a search box. Open it with <kbd>Ctrl</kbd> + <kbd>Shift</kbd> + <kbd>I</kbd> over a chart, the **+** at the top of the [Indicators page](#the-indicators-page), or the **Add an indicator** row that page shows when the chart has none.

Before anything is typed, it lists every indicator under its family's heading. Typing gives one list, best match first. A name, a short name or a related word finds an indicator (`poc` finds the volume profile, `london` Session Times), and a family's name finds its members (`volume`, `time`, `session`).

<!-- regenerate:
theme: tokyo-night
window: 1600x1900 @2, bin/headless-window (HOME sandboxed with ~/.local/share/omarchy/themes/tokyo-night copied in; tall enough for the whole list)
state:
  omacharts config set theme omarchy:tokyo-night
  key ctrl+b
  omacharts chart set --template none
  omacharts chart indicator add sma --period 50
  omacharts chart indicator add sma --period 200
  omacharts chart indicator add volume
  omacharts chart indicator add rsi
  move 795 900
  key ctrl+shift+i
capture: bin/headless-window shot /tmp/picker.png
crop: 364,314,872,1272   (the whole search, 16px margin)
-->
{% include figure.html src="/assets/manual/indicators/add-an-indicator.png" alt="The Add an indicator search with its list of indicators" caption="The **Add an indicator** search, before anything is typed." %}

| Key | Does |
|---|---|
| Type | Narrows the list |
| <kbd>↑</kbd> / <kbd>↓</kbd> | Moves the selection without leaving the box |
| <kbd>Enter</kbd>, <kbd>Ctrl</kbd> + <kbd>Enter</kbd>, a click | Picks the selected indicator |
| <kbd>Esc</kbd>, a click outside | Closes the search, adding nothing |

Picking one puts it on the chart at its defaults and opens its settings in a dialog named after it, with **Cancel** and **Add** in the header. The chart changes as you change a setting. **Add** or <kbd>Ctrl</kbd> + <kbd>Enter</kbd> keeps the indicator. **Cancel**, <kbd>Esc</kbd> or a click outside removes it. On a chart on a template, the indicator is added to the template, and **Cancel** takes it off the template again.

<!-- regenerate:
theme: tokyo-night
window: 1600x2200 @2, bin/headless-window (HOME sandboxed with ~/.local/share/omarchy/themes/tokyo-night copied in; tall enough that the whole dialog fits, Legend included)
state:
  omacharts config set theme omarchy:tokyo-night
  move 795 1095
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
crop: 223,150,1152,1902   (the whole dialog, 16px layout margin)
-->
{% include figure.html src="/assets/manual/indicators/macd-parameters.png" alt="The settings dialog of a MACD being added" caption="The settings of a MACD being added." %}

Every indicator's settings end with a **Legend** group, whose **Show in chart legend** switch says whether the [legend](#the-legend) names it.

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

Above the list, **Chart template** names the template the chart is on, or **None**. On a chart on a template, the list is the template's own: it is titled after it, such as **On the Momentum template**, with **A change here reaches every chart that uses it.** under the title. Its rows open, reorder, show, hide and remove the template's indicators, and **+** adds to the template. Every chart on the template changes with it ([Chart templates](/manual/chart-templates/)).

An indicator's settings opened on such a chart, from the list, the legend's gear or after adding one, start with an **Editing the Momentum template** row that says the same.

<!-- regenerate:
theme: tokyo-night
window: 2560x1440 @2, bin/headless-window (HOME sandboxed with ~/.local/share/omarchy/themes/tokyo-night copied in)
state:
  omacharts config set theme omarchy:tokyo-night
  key ctrl+b
  omacharts template create Momentum
  omacharts template indicator add Momentum rsi
  omacharts template indicator add Momentum macd
  omacharts chart set --template Momentum
  key ctrl+i
  click 500 333          # the RSI 14 row
  move 1270 710
capture: bin/headless-window shot /tmp/panel.png
crop: 688,68,1184,1304   (the whole dialog, 16px layout margin)
-->
{% include figure.html src="/assets/manual/indicators/panel-on-a-template.png" alt="The settings of an RSI on a chart on the Momentum template, opening with a row that says it is editing the template" caption="An RSI's settings on a chart on the **Momentum** template." %}

## The legend

The legend lists the chart's indicators under the symbol, faint until the pointer comes near. Each line is a dot in the indicator's colour, its label and three buttons.

| Control | Does |
|---|---|
| Eye | Hides it, or shows it again. A hidden indicator's label stays, dimmed |
| Gear | Opens its settings in Chart settings |
| Bin | Removes it |
| Double-click the label | Opens its settings, as the gear does |

An indicator with **Show in chart legend** off in its settings has no line in the legend, and is still drawn. Hiding it is the switch on its row in the [Indicators page](#the-indicators-page). [Session Times](/manual/indicators/session-times/) starts out of the legend, and every other indicator in it.

The label is the indicator's short name and its lengths, in the order its settings list them, such as **SMA 50** or **MACD 12 26 9**. A VWAP or a volume profile shows its period instead, such as **VWAP · Session**. When the period in use differs from the one chosen, the label names the one in use ([Reset periods](#reset-periods)). [Session Times](/manual/indicators/session-times/) names its ranges: **Sessions · ETH, London**.

## Colours and lines

An indicator with no chosen colour takes the first of the theme's sequence (Blue, Amber, Violet, Teal, Orange, Cyan) that no other indicator on the chart is using. When all are in use, they repeat. Adding an indicator never changes the colours already on the chart. [Session Times](/manual/indicators/session-times/) is the exception: it shades without a colour of its own unless one is chosen.

A **palette name** (Blue, Amber, Violet, Teal, Rose, Green, Orange, Cyan) is the theme's colour of that name and follows the theme when it changes. A **custom** colour, picked exactly or typed as a hex such as `#e0af68`, stays the same in every theme.

Each **Colour** row opens the theme's palette ([Themes and colours](/manual/themes/)). **Custom…** under it picks an exact colour. **Reset** beside the swatch goes back to the automatic colour.

| Row | Choices | Default |
|---|---|---|
| **Colour** | A palette name, or a custom colour | The next free colour of the sequence |
| **Thickness** | No line, 1, 1.5, 2.5, 4 px | 1.5 px |
| **Line style** | Solid, Dashed, Dotted | Solid |

A MACD's signal line and a stochastic's %D have a second colour row. Left alone, it is a contrasting colour from the theme.

## Indicators on a chart on a template

On a chart that is on a template, every change to its indicators changes the template, and every chart on it: the [Indicators page](#the-indicators-page), every indicator's settings, the legend's eye and bin, a strip's corner controls and the CLI alike ([How a chart on a template behaves](/manual/chart-templates/#how-a-chart-on-a-template-behaves)). To give one chart a set of its own, make a template from it: right-click the chart › **Template** › **Create new template from chart…**.

## Reset periods

The [VWAP](/manual/indicators/averages-and-bands/#volume-weighted-average-price) and the [volume profile](/manual/indicators/volume/#volume-profile) start over at the beginning of each period, chosen with **Reset every** in their settings.

| Period | Starts over |
|---|---|
| **Session** | At each session's open. A future's session opens the evening before and counts as one period |
| **Week** | On Monday |
| **Month** | On the 1st |
| **Quarter** | On 1 January, April, July and October |
| **Year** | On 1 January |

A period needs at least ten bars at the chart's resolution. One with fewer uses the finest period that has ten, and the legend names it. A session VWAP becomes weekly on a 4h chart, monthly on a daily chart and quarterly on a weekly chart. The chosen period is kept, and comes back on an intraday resolution.

## See also

- [Averages and bands](/manual/indicators/averages-and-bands/)
- [Volume](/manual/indicators/volume/)
- [Momentum and volatility](/manual/indicators/momentum-and-volatility/)
- [Session times](/manual/indicators/session-times/)
- [Indicators from the CLI](/manual/cli/#indicators)
