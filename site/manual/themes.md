---
title: "Themes and colours"
description: "The desktop theme, the Omarchy themes, the colours of the bars, and the palette that indicators and drawings use."
section: configuration
nav_order: 25
---

A theme is the set of colours the window is drawn in. It includes the palette, the named colours that indicators and drawings use. On an Omarchy desktop the window follows the desktop theme and changes with it.

<!-- regenerate:
theme: tokyo-night, everforest, catppuccin-latte, gruvbox (one capture each, the desktop theme switched between them)
window: 2560x1600 @2, $SP/kit/hw
state:
  omacharts chart set --symbol NVDA --resolution 1D
  omacharts chart indicator add sma --period 50 --color Amber
  omacharts chart indicator add ema --period 21 --color Blue
  omacharts chart indicator add rsi
  move 1270 790
  for each theme: $SP/kit/theme THEME; sleep 4
capture: bin/headless-window cli chart screenshot --output /tmp/theme-THEME.png   (four, joined 2x2 with magick +append/-append, resized to 1600 wide)
crop: none
-->
{% include figure.html src="/assets/manual/themes/four-themes.jpg" alt="The same NVDA daily chart under four desktop themes" caption="The same chart under Tokyo Night, Everforest, Catppuccin Latte and Gruvbox." %}

## Choosing a theme

Preferences › Appearance › **Theme** has two cards, each a small picture of the window in its theme. The same page is under **Global** in [Chart settings](/manual/chart-settings/).

| Card | What it is |
|---|---|
| **Follow system** | The desktop's Omarchy theme, followed as it changes. The card says which one, *Now Tokyo Night*. Only on Omarchy, where it is the default |
| **Catalogue** | Every Omarchy theme on the machine, as a page of pictures. Pick one and the window wears it, whatever the desktop does; the card then shows it. The desktop's own is marked **desktop** |

Without Omarchy there is no **Follow system**, and the catalogue holds a set of Omarchy's themes built into the app, Tokyo Night the default. A theme installed on the desktop while the window is open appears next time it opens. A light theme makes the whole window light, dialogs and menus included.

A theme of your own is made with Omarchy's tools, and then appears in the catalogue like any other.

## Bars

Preferences › Appearance › **Bars** sets how bars look on every chart, or under **LOCAL** on the focused chart alone ([A chart's own look](/manual/charts/#a-charts-own-look)). Tabs at its head pick **Candles** or **OHLC**, and on OHLC bars the **Thickness** row under them sets how wide they are drawn ([Style](/manual/charts/#style)). Under them, the **Up** and **Down** rows each hold the direction's colour and, for candles, its body, each picked from a list showing a bar drawn as the charts draw it. OHLC bars have no body, so the bodies go with them.

| Control | Does |
|---|---|
| Colour | A colour from the theme's palette, or **Foreground**, the theme's text colour (Omarchy's `foreground`), for bars with no colour. It is light on a dark theme and dark on a light one |
| Filled | The whole candle in its colour |
| Bordered | The wick and the body's edge in the theme's text colour, as TradingView draws one. It follows the theme, so it is light on a dark theme and dark on a light one |
| Hollow | The body as an outline in the candle's colour. Foreground and Hollow make a hollow candle in the text colour |

A border needs room: a candle whose body is under five pixels wide keeps its own colour throughout, and one too narrow for a body is drawn as a line in its colour. `omacharts config bars` sets the same answers ([`config bars`](/manual/cli/#config-bars)).

The colours are the theme's, so they change with it. Red for a rise is the two colours swapped; monochrome is Foreground both ways. Beside the palette and Foreground, the pickers offer **Slate** and **Sand**, a slate against a sand from the theme's palette that sit back behind what is drawn over the bars, and **Silver** and **Graphite**, two greys made from its text. They are every chart's, except on a chart with [a look of its own](/manual/charts/#a-charts-own-look). Volume bars take the candle colours unless the volume indicator was given colours of its own ([Volume](/manual/indicators/volume/)).

## The palette

The palette is the theme's named colours for indicators and drawings. An indicator or drawing on a palette colour changes with the theme.

An indicator added without a colour gets the next of Blue, Amber, Violet, Teal, Orange and Cyan not already on the chart. After six they repeat. Green and Rose are never given automatically, but you can pick them.

**Neutral** is a very light tone of the theme's own hue on a dark theme and a very dark one on a light theme: close to the theme's text, never quite it, and tinted rather than white or black, a pale lavender on Tokyo Night and a cream on Gruvbox. It suits outlines, profiles and lines that should read plainly without taking a colour. It comes from the theme's text, or its background or accent when the text is a plain grey, so it has no row in the theme editor, and like the rest of the palette it changes with the theme. It is never given automatically.

### The palette picker

A colour button in the indicator and drawing dialogs opens the theme's palette first.

<!-- regenerate:
theme: tokyo-night
window: 2560x1600 @2, bin/headless-window start --size 2560x1600 (fresh session, HOME sandboxed)
state:
  move 1270 790
  omacharts chart set --symbol NVDA --resolution 1D
  omacharts chart indicator add sma --period 50 --color Amber
  key ctrl+i              # twice: a fresh window swallows its first chord
  click 503 291          # the SMA 50 row
  click 849 315          # its Colour swatch
  move 1270 790
capture: bin/headless-window shot /tmp/picker.png
crop: 751,500,1206,600   (the Line group and the palette, 16px layout margin round both)
-->
{% include figure.html src="/assets/manual/themes/palette-picker.png" alt="The palette picker open under an indicator's Colour row: nine colours in three rows, Neutral last, the colour in use ringed and Custom… below" caption="The palette picker." %}

| Choice | When the theme changes |
|---|---|
| A swatch | The colour changes with the theme. |
| **Custom…** | Opens the colour chooser. The colour picked there does not change with the theme. |

## The Omarchy plugin's colours

The Omarchy plugin takes its colours from the desktop theme, even when the window is on another theme ([Omarchy plugin](/manual/bar-widget/#what-it-shows)).

## See also

- [The `theme` setting in the CLI](/manual/cli/#settings)
