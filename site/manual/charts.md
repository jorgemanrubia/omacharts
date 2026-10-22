---
title: "Charts"
description: "Resolutions, sessions, bars, scales, moving around, the crosshair, the header and the chart menu."
section: charting
nav_order: 14
---

A chart shows one symbol at one resolution. A chart's settings, and the defaults new charts open with, are in [Chart settings](/manual/chart-settings/).

<!-- regenerate:
theme: tokyo-night
window: 2560x1440 @2, bin/headless-window (HOME sandboxed, theme seeded)
state:
  omacharts chartbook create Manual --symbol NVDA --switch
  omacharts chartbook delete "Chartbook 1"
  omacharts chart set --resolution 1D
  omacharts config set chart_header symbol,resolution,change,template
  key ctrl+b
capture: bin/headless-window cli chart screenshot --output /tmp/candles.png
crop: none (resized to 2000 wide)
-->
{% include figure.html src="/assets/manual/charts/daily-candles.jpg" alt="An NVDA daily candle chart with its header and a volume strip" caption="A daily chart. The header shows the symbol, the resolution, the day's move and the chart template." %}

## Resolutions

A resolution is the length of one bar, typed as a number and a unit.

| Typed | Means |
|---|---|
| `15`, `240` | A bare number is minutes. `240` is four hours |
| `5m`, `90m` | Minutes |
| `1h`, `4h` | Hours |
| `1D`, `D` | Days. A unit alone means one of it |
| `1W` | Weeks |
| `1M`, `3M`, `1mo` | Months. `1M` is a month and `1m` a minute; `mo` is a month in either case |
| `1Y` | Years |

Longer spellings (`min`, `hr`, `day`, `wk`, `month`, `year`) work too. A resolution is shown in its largest exact unit, so `60m` becomes `1h` and `7D` becomes `1W`. Monthly and yearly bars follow calendar months and years, so `3M` is calendar quarters.

### Choosing a resolution

The resolution strip is the row of resolution buttons across the top of the focused chart.

| Key or gesture | Does |
|---|---|
| Type a digit over the chart | Opens the resolution box with that digit in it |
| <kbd>Ctrl</kbd> + <kbd>Shift</kbd> + <kbd>←</kbd> <kbd>→</kbd> | The previous or next resolution on the strip. Stops at either end. From a resolution not on the strip, starts at the nearest one that is |
| A button on the resolution strip | That resolution, on that chart |
| The resolution in the header | Opens the list of the strip's resolutions, for a chart too narrow to show the strip |

<!-- regenerate:
theme: tokyo-night
window: 2560x1440 @2, bin/headless-window (HOME sandboxed, theme seeded)
state:
  (one NVDA 1D chart, sidebar hidden)
  key 4
  type h
capture: bin/headless-window shot /tmp/resbox.png
crop: 0,0,1640,320
-->
{% include figure.html src="/assets/manual/charts/resolution-box.png" alt="The resolution box under the strip of an NVDA daily chart, with 4h typed" caption="The resolution box with 4h typed." %}

As you type, the box shows what you have typed as a resolution (`240` shows **4 hours**) or **not a resolution**. <kbd>Enter</kbd> sets it; <kbd>Esc</kbd> or a click elsewhere cancels. A resolution not on the strip is added to it, in order of length.

### The preset resolutions

The resolution strip is the same for every chart. Out of the box it is `5m 15m 1h 4h 1D 1W`. To edit it, right-click the strip and choose **Edit resolutions…**, or open Preferences › Charts › Other › **Preset resolutions** › **Edit…**.

<!-- regenerate:
theme: tokyo-night
window: 2560x1440 @2, bin/headless-window (HOME sandboxed, theme seeded)
state:
  (one NVDA 1D chart, sidebar hidden)
  run bin/virtual-pointer --extent 1280x720 --button right click 600 24
  click 675 46            # Edit resolutions…
capture: bin/headless-window shot /tmp/resdialog.png
crop: 490,190,1580,1060
-->
{% include figure.html src="/assets/manual/charts/resolutions-dialog.png" alt="The Resolutions dialog, with an Add row and the six preset resolutions, each with a bin" caption="The Resolutions dialog." %}

| Row | Does |
|---|---|
| **Add** | Type a resolution and press <kbd>Enter</kbd>. It joins the list in order of length. Something that is not a resolution is ignored |
| A resolution | Its bin removes it from the strip. All of them can be removed; a chart with no strip still takes a typed resolution |

## Sessions

A chart's session is which hours of bars it shows.

| Hours | Shows |
|---|---|
| **Extended hours** | Every bar, overnight included. The default |
| **Regular hours only** | Only the intraday bars inside regular trading hours |

**Regular hours only** applies to US stocks, ETFs, US indexes and dollar-priced futures (09:30 to 16:00 New York time) and to TWSE and TPEx listings and Taiwan's indexes (09:00 to 13:30 Taipei time). FX, crypto, listings outside the US and Taiwan, and indexes in other currencies show every bar. Daily and coarser bars always show every bar.

Set it from the chart menu's **Session** submenu or the **Hours** row of [Chart settings](/manual/chart-settings/).

## Bars

### Style

| Style | Draws |
|---|---|
| **Candles** | A body from open to close and a wick from high to low. The default |
| **OHLC bars** | A vertical bar from high to low, a tick to the left at the open and one to the right at the close |

<!-- regenerate:
theme: tokyo-night
window: 2560x1440 @2, bin/headless-window (HOME sandboxed, theme seeded)
state:
  (the daily-candles state above)
  omacharts chart set --style ohlc
capture: bin/headless-window cli chart screenshot --output /tmp/ohlc.png
crop: none (resized to 2000 wide)
-->
{% include figure.html src="/assets/manual/charts/daily-ohlc.jpg" alt="The same NVDA daily chart drawn as OHLC bars" caption="The same chart as OHLC bars." %}

### Colouring

The colouring is how a chart uses the theme's colours ([Themes and colours](/manual/themes/)).

| Colouring | Paints |
|---|---|
| **Filled** (**Coloured** for OHLC bars) | Rises in the theme's up colour, falls in its down colour, bodies filled. The default |
| **Hollow** | The same, with rising candles drawn as outlines. Candles only. An OHLC bar is drawn coloured |
| **Red up** | Red for a rise and green for a fall |
| **Monochrome** | Every bar in the theme's monochrome colour. An OHLC bar still shows its direction by its ticks; a candle does not |

<!-- regenerate:
theme: tokyo-night
window: 2560x1440 @2, bin/headless-window (HOME sandboxed, theme seeded)
state:
  (the daily-candles state above)
  omacharts config set chart_header symbol,resolution
  omacharts chart split horizontal
  omacharts chart split vertical --chart pos:0
  omacharts chart split vertical --chart pos:2
  omacharts chart set --chart pos:N --link pinned        # N = 0..3
  omacharts chart set --chart pos:0 --colouring theme
  omacharts chart set --chart pos:1 --colouring theme-hollow
  omacharts chart set --chart pos:2 --colouring theme-red-up
  omacharts chart set --chart pos:3 --colouring theme-mono
  then, with pos:2 focused: key alt+Left, key plus x5, key alt+Down, key plus x5, key alt+Right, key plus x5, key alt+Up, key plus x5
capture: bin/headless-window cli chartbook screenshot --output /tmp/colourings.png
crop: none (resized to 2000 wide)
-->
{% include figure.html src="/assets/manual/charts/colourings.jpg" alt="Four NVDA daily charts of the same two months, one in each colouring" caption="Filled, Red up (top); Hollow, Monochrome (bottom)." %}

Set them per chart from the chart menu's **Bar style** submenu or the **Style** and **Colouring** rows of Chart settings. To change every chart at once, use **Apply to all charts now** in Preferences ([Chart settings](/manual/chart-settings/#defaults-for-new-charts)).

## Scales

The price axis fits the bars on screen until you drag it. A chart's scales come back as they were when the app reopens.

| Gesture or key | Does |
|---|---|
| Drag the price axis | Up stretches the prices apart, down squeezes them together. The scale stops fitting itself |
| Wheel over the price axis, or <kbd>Ctrl</kbd> + wheel over the chart | The same, a notch at a time |
| Drag the chart up or down | Moves along the price axis, once the scale is no longer fitting itself |
| Double-click the price axis | Back to fitting the bars |
| Drag the time axis | Left squeezes more time on screen, right spreads it out |
| Double-click the time axis | Back to the resting zoom, at the latest bar |
| <kbd>Ctrl</kbd> + <kbd>Esc</kbd> | **Reset chart**. Both scales back to how the chart opens, at the latest bars |

### Logarithmic scale

On a logarithmic price axis equal heights are equal percentage moves. Switch it from the price axis menu or the **Logarithmic scale** row of [Chart settings](/manual/chart-settings/). Indicator strips stay linear. While a price at or below zero is on screen (a spread, or a contract that traded negative) the axis is drawn linear, and turns logarithmic again once that price is off screen.

<!-- regenerate:
theme: tokyo-night
window: 2560x1440 @2, bin/headless-window (HOME sandboxed, theme seeded)
state:
  (one NVDA chart, sidebar hidden)
  omacharts chart set --symbol NVDA --resolution 1W --log-scale on
  move 640 400
  key minus x10
  move 1279 700
capture: bin/headless-window cli chart screenshot --output /tmp/log.png
crop: none (resized to 2000 wide)
-->
{% include figure.html src="/assets/manual/charts/weekly-log.jpg" alt="NVDA weekly bars from 2007 to 2026 on a logarithmic price axis" caption="Twenty years of weekly bars on a logarithmic scale." %}

### The price axis menu

Right-click the price axis for its menu.

<!-- regenerate:
theme: tokyo-night
window: 2560x1440 @2, bin/headless-window (HOME sandboxed, theme seeded)
state:
  omacharts chart set --symbol NVDA --resolution 1D
  key ctrl+b                          # sidebar hidden
  omacharts chart split horizontal    # so the left chart's price axis is mid-window and the menu has room
  omacharts chart set --chart pos:0 --style candles
  omacharts chart set --chart pos:1 --style candles
  omacharts chart focus pos:0
  move 1250 710
  run bin/virtual-pointer --extent 1280x720 --button right click 600 300   # the left chart's price axis
capture: bin/headless-window shot /tmp/axismenu.png
crop: 490,300,1220,582   (the menu with 20px layout margin right and below)
-->
![The price axis menu, with Auto scale price ticked](/assets/manual/charts/price-axis-menu.png)

| Item | Does |
|---|---|
| **Auto scale price** | Ticked while the scale fits the visible bars. Choosing it toggles. Off, the chart can be dragged up and down |
| **Logarithmic scale** | Ticked while the axis is logarithmic. Choosing it toggles |
| **Reset chart** | As <kbd>Ctrl</kbd> + <kbd>Esc</kbd> |

## Moving around

| Gesture or key | Does |
|---|---|
| Drag the chart | Pans through time. Dragging right reveals older bars |
| Wheel | Zooms time about the pointer |
| <kbd>Shift</kbd> + wheel | Pans through time |
| A trackpad's sideways swipe | Pans, whatever is held |
| <kbd>←</kbd> <kbd>→</kbd> | Pans five bars. Over a selected drawing they nudge it instead ([Drawings](/manual/drawings/)) |
| <kbd>+</kbd> <kbd>−</kbd> | Zooms in and out about the middle |
| <kbd>End</kbd> | Back to the latest bar, following new bars again |

A chart can be panned a little past either end of its bars. One panned back into history stays there as new bars arrive. <kbd>↑</kbd> <kbd>↓</kbd> over a chart move through the watchlist, not the chart ([Watchlists](/manual/watchlists/)).

## Gridlines

<kbd>Ctrl</kbd> + <kbd>Shift</kbd> + <kbd>G</kbd> shows or hides the focused chart's gridlines. The scales and their labels stay either way. The same switch is the chart menu's **Gridlines** and the **Gridlines** row of [Chart settings](/manual/chart-settings/).

## The crosshair and the last price

<!-- regenerate:
theme: tokyo-night
window: 2560x1440 @2, bin/headless-window (HOME sandboxed, theme seeded)
state:
  omacharts chartbook create Desk --symbol NVDA --switch
  omacharts chart split horizontal
  omacharts chart split vertical --chart pos:1
  omacharts chart set --chart pos:1 --resolution 1h
  omacharts chart set --chart pos:2 --resolution 15m
  key alt+Right
  key ctrl+m
  move 700 300; move 702 302
capture: bin/headless-window shot /tmp/max.png
crop: 1100,500,1460,940
-->
{% include figure.html src="/assets/manual/charts/crosshair.jpg" alt="An NVDA hourly chart with the crosshair and its price and time tags, and the last-price tag on its dashed line" caption="The crosshair's tags in the accent colour, and the last price in the colour of the last bar." %}

The crosshair is a pair of dotted lines that cross at the centre of the bar under the pointer. The price is tagged on the price axis and the bar's time on the time axis (the date alone on daily and coarser bars). Over an indicator strip the price tag shows that strip's value. With [Crosshair across Follow charts](/manual/watchlists/#the-crosshair-across-follow-charts) on, the other Follow charts show the same moment and price, dimmed.

The last price is a tag on the price axis, filled in the colour of the last bar's direction, with a dashed line across the chart at its level.

## When a chart is empty or out of date

A chart with no bars says why in the middle of the plot.

<!-- regenerate:
theme: tokyo-night
window: 2560x1600 @2, $SP/kit/hw (no thinkorswim session)
state:
  omacharts config set provider tos
  omacharts chart set --symbol EURUSD --resolution 1D
capture: bin/headless-window shot /tmp/unserved.png
crop: 0,0,1998,880, resized to 1400 wide
-->
{% include figure.html src="/assets/manual/charts/feed-failure.png" alt="A EURUSD daily chart with no bars and the line: This data provider cannot chart this symbol. Another one may be able to." caption="EURUSD on a feed that cannot chart it." %}

| The chart says | What it means | What to do |
|---|---|---|
| *Press Ctrl+K to find a symbol* | The chart has no symbol yet. | Type a letter over it ([Symbols](/manual/symbols/)). |
| *Loading…* | The prices are loading. | Nothing. |
| *No data for this symbol* | The feed has no bars for it, such as a delisted ticker. | Try another symbol, or another feed. |
| *The data provider does not have this symbol.* | The feed has no such ticker. | Check the spelling and venue suffix ([Symbols](/manual/symbols/)). |
| *This data provider cannot chart this symbol. Another one may be able to.* | The feed cannot chart this kind of instrument. | Switch feed ([Market data feeds](/manual/market-data/)). |
| *The data provider does not serve this resolution.* | The feed has no bars at this resolution. | Try another resolution. |
| *This data provider is not signed in. Open Preferences → Market data → Provider.* | thinkorswim is chosen and is not signed in, or the sign-in has expired. | Preferences › Data › **Sign in…**. |
| *Rate limited by the data provider. Prices fill in shortly.* | The feed has stopped sending prices for a while. | Wait. Prices fill in when it allows them again. |
| *No network connection. Prices fill in when there is one.* | This machine is offline. | Nothing. It fills in when the connection is back. |
| *The data provider is not answering. Prices fill in when it does.* | The feed is not answering. | Wait. If it persists, check the feed in Preferences › Data. |
| *The data provider sent a reply this could not read.* | The feed sent something that was not prices. | Wait for the next refresh. |
| *Could not open the local price cache.* | The disk is full, or Omacharts cannot write to its data folder. | Check the data folder ([Where your data lives](/manual/your-data/)). |

A chart that has bars but could not get newer ones keeps them and shows a small rose dot in the top-right corner of the plot. Hover over it for the reason.

Prices refresh as bars close. A chart does not refresh while its market is shut or while it is panned back into history. On a streaming feed, prices arrive as they change ([Market data feeds](/manual/market-data/)).

## The header

The header is the line at a chart's top-left. It is the same on every chart in every chartbook, and can show these parts in any order.

| Part | Shows |
|---|---|
| **Symbol** | The symbol, with its venue. Click it to [find another](/manual/symbols/) |
| **Resolution** | The resolution. Click it for the list of the strip's resolutions |
| **Daily change** | The day's move as a percentage, in the chart's up and down colours. The same figure as the sidebar's |
| **Chart template** | The name of the [chart template](/manual/chart-templates/) the chart is on, dimmed. Nothing on a chart that is on none |

Out of the box it shows the symbol and the resolution.

Edit it in Preferences › Charts › **Header**, under **In the header**, or right-click the header and choose **Edit header…**, which opens the same tab. The **+** beside the title adds a part not yet there, a part's bin removes it, and dragging a part by its handle reorders it.

## The chart menu

Right-click a chart for its menu. The chart under the pointer is focused first. A right-click on a drawing opens the drawing's own menu instead ([Drawings](/manual/drawings/)), and one on the price axis the [price axis menu](#the-price-axis-menu).

<!-- regenerate:
theme: tokyo-night
window: 2560x1440 @2, bin/headless-window (HOME sandboxed, theme seeded)
state:
  (one NVDA 1D chart, sidebar hidden)
  key ctrl+alt+h
  run bin/virtual-pointer --extent 1280x720 --button right click 300 150
capture: bin/headless-window shot /tmp/chartmenu.png
crop: 70,290,1580,900
-->
{% include figure.html src="/assets/manual/charts/chart-menu.png" alt="The chart menu open over a chart, each item with its key" caption="The chart menu on a chartbook of two charts." %}

| Item | Does | Explained in |
|---|---|---|
| **Paste drawing** | Puts down the drawings last copied. Only there while something has been copied | [Drawings](/manual/drawings/) |
| **Bar style** › **Candles**, **OHLC bars** | The chart's bar style | [Bars](#bars) |
| **Session** › **Extended hours**, **Regular hours only** | The chart's hours | [Sessions](#sessions) |
| **Template** › **None**, then each chart template | Puts the chart on a template, with its key | [Chart templates](/manual/chart-templates/) |
| **Template** › **Create new template from chart…** | Saves the chart's indicators as a new template and puts the chart on it. Greyed out on a chart with no indicators | [Chart templates](/manual/chart-templates/#creating-a-template) |
| **Gridlines** | Ticked while the chart has them | [Gridlines](#gridlines) |
| **Split horizontally**, **Split vertically** | Divides the chart in two | [Splitting and arranging charts](/manual/layouts/) |
| **Maximize** / **Restore** | Gives the chart the window, or puts it back. Only with two charts or more | [Splitting and arranging charts](/manual/layouts/#maximising-a-chart) |
| **Close chart** | Closes it. Only with two charts or more | [Splitting and arranging charts](/manual/layouts/#closing-a-chart) |
| **Watchlist** › **Follow**, **Focus**, **Pinned** | The chart's link mode | [Choosing a link mode](/manual/watchlists/#choosing-a-link-mode) |
| **Take screenshot** | A picture of the chart, to the clipboard | [Screenshots](/manual/screenshots/) |
| **Remove all drawings** | Deletes every drawing on the chart. Only there while it has some | [Drawings](/manual/drawings/) |
| **Indicators…** | The Indicators page of Chart settings | [Indicators](/manual/indicators/) |
| **Chart settings…** | The Chart page of Chart settings | [Chart settings](/manual/chart-settings/) |

## See also

- [Changing charts from the CLI](/manual/cli/#charts)
