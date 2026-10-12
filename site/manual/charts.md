---
title: "Charts"
description: "Resolutions, sessions, bars, scales, moving around, Go to, the crosshair, the header and the chart menu."
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
  key ctrl+shift+b
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
| `1Q`, `Q` | Quarters, the same as `3M` |
| `1Y` | Years |

Longer spellings (`min`, `hr`, `day`, `wk`, `month`, `quarter`, `year`) work too. A resolution is shown in its largest exact unit, so `60m` becomes `1h` and `7D` becomes `1W`. Monthly and yearly bars follow calendar months and years, so `3M` is calendar quarters.

Intraday bars start with the trading day, in the exchange's own clock. On a chart of [regular hours](#sessions) they count from the open, so SPY's 4h bars start at 09:30 and 13:30 New York time, and the second one ends early at the 16:00 close. On extended hours they count from the start of the day's trading: 04:00, 08:00, 12:00 and 16:00 for SPY, and 17:00 Chicago time for ES. The times stay put when the clocks change for summer. FX and crypto count from midnight UTC.

### Choosing a resolution

The resolution strip is the row of resolution buttons across the top of the focused chart.

| Key or gesture | Does |
|---|---|
| Type a digit over the chart | Opens **Change resolution** with that digit in it |
| <kbd>Ctrl</kbd> + <kbd>Shift</kbd> + <kbd>←</kbd> <kbd>→</kbd> | The previous or next resolution on the strip. Stops at either end. From a resolution not on the strip, starts at the nearest one that is |
| A button on the resolution strip | That resolution, on that chart |
| The resolution in the header | Opens the list of the strip's resolutions, for a chart too narrow to show the strip |

<!-- regenerate:
theme: tokyo-night
window: 2560x1440 @2, bin/headless-window (HOME sandboxed, theme seeded)
state:
  omacharts chartbook create Manual --symbol NVDA --switch
  omacharts chartbook delete "Chartbook 1"
  omacharts chart set --resolution 1D
  click 400 400
  key ctrl+shift+b                # sidebar hidden
  move 640 600
  key 4
  type h
capture: bin/headless-window shot /tmp/resolution.png
crop: 898,193,763,399   (the whole card, 24px layout margin)
-->
{% include figure.html src="/assets/manual/charts/change-resolution.png" alt="The Change resolution card over an NVDA daily chart, with 4h typed and 4 hours under the box" caption="**Change resolution** with 4h typed." %}

**Change resolution** opens over the focused chart. Under the box it says what you typed as a resolution (`240` shows **4 hours**), or **not a resolution** in red. With more than one chart in the window, every chart fades and the one it will change wears a wider focus ring. The ⓘ beside the title has examples.

<kbd>Enter</kbd> sets the resolution; <kbd>Esc</kbd> or a click elsewhere closes it, changing nothing. A resolution not on the strip is added to it, in order of length.

### The preset resolutions

The resolution strip is the same for every chart. Out of the box it is `5m 15m 1h 4h 1D 1W`. To edit it, right-click the strip, where the [header](#the-header) shows it, and choose **Edit resolutions…**, or open Preferences › General › Chart › **Preset resolutions** › **Edit…**.

The strip is a part of [the header](#the-header). To hide it, take it off the header in Preferences › Overlays › **Header**, or run `omacharts chart strip off`; `chart strip on` puts it back in the centre. The resolutions stay: typing one, the resolution in the header and <kbd>Ctrl</kbd> + <kbd>Shift</kbd> + <kbd>←</kbd> <kbd>→</kbd> all still work.

<!-- regenerate:
theme: tokyo-night
window: 2560x1440 @2, bin/headless-window (HOME sandboxed, theme seeded)
state:
  (one NVDA 1D chart, sidebar hidden)
  omacharts chart strip on
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

Regular hours are the instrument's exchange's own, in its own clock: 09:30 to 16:00 New York time for US stocks and ETFs and for equity index futures such as ES, 09:00 to 14:30 New York time for crude oil, 09:00 to 17:30 Madrid time for a Madrid listing, and Tokyo's session without its lunch break. The [trading day](/manual/indicators/session-times/#the-trading-day) has more examples. FX and crypto trade round the clock and have no regular hours, so they show every bar either way. Daily and coarser bars always show every bar.

The price is the price whatever bars are shown: out of regular hours, a chart showing only them still puts its [last price](#the-crosshair-and-the-last-price) at the latest trade and follows it as it streams, and its price scale reaches it while the chart shows its latest bars.

Set it from the chart menu's **Session** submenu or the **Hours** row of [Chart settings](/manual/chart-settings/). [Session Times](/manual/indicators/session-times/) shades the extended hours on a chart that shows them.

## Bars

### Style

Charts draw their bars in the style picked at the head of Preferences › Appearance › **Bars**, which `omacharts config bars --style` sets too. A chart's **Bar style** submenu changes that chart alone, giving it [a look of its own](#a-charts-own-look), and shows its bars if they were hidden.

| Style | Draws |
|---|---|
| **Candles** | A body from open to close and a wick from high to low. The default |
| **OHLC bars** | A vertical bar from high to low, a tick to the left at the open and one to the right at the close |
| **Line** | One line through the closes, in the theme's accent. For a series that is only a close: a yield, a spread |

OHLC bars have a thickness, 1, 1.5, 2 or 3 pixels and 2 by default, for rising and falling bars apart. It is part of the bars' look, beside each direction's colour in Preferences › Appearance › [Bars](/manual/themes/#bars), there while the bars are OHLC bars, and `omacharts config bars --up-thickness`, `--down-thickness` sets it too. Where the bars are packed too close for it, they are drawn thinner so neighbours never touch.

<!-- regenerate:
theme, window: as Preferences › General ([Preferences](/manual/preferences/)): tokyo-night as an Omarchy desktop's,
  Yaru-purple icons, 2560x2400 @2 from bin/headless-window start --size 2560x2400, HOME sandboxed
state:
  omacharts config bars --style ohlc --down-thickness 1.5
  key ctrl+comma
  run swaymsg '[title="^Preferences$"] resize set 820 770, move position 230 100'
  click 313 351          # Appearance in the sidebar
  click 775 614          # Down's thickness
capture: bin/headless-window shot /tmp/thickness.png
crop: 892,898,1184,706   (the Bars group and the open list, 8px layout margin)
-->
{% include figure.html src="/assets/manual/charts/ohlc-thickness.png" alt="The Bars group of Preferences › Appearance on OHLC bars, Down's thickness list open on 1, 1.5, 2 and 3" caption="Each direction's thickness, beside its colour on OHLC bars." %}

<!-- regenerate:
theme: tokyo-night
window: 2560x1440 @2, bin/headless-window (HOME sandboxed, theme seeded)
state:
  (the daily-candles state above)
  omacharts config bars --style ohlc
capture: bin/headless-window cli chart screenshot --output /tmp/ohlc.png
crop: none (resized to 2000 wide)
-->
{% include figure.html src="/assets/manual/charts/daily-ohlc.jpg" alt="The same NVDA daily chart drawn as OHLC bars" caption="The same chart as OHLC bars." %}

### Colours

The colour of a rise and of a fall, and whether a candle's body is filled, bordered or hollow, are set beside the style ([Bars](/manual/themes/#bars)). They are every chart's, except on a chart with [a look of its own](#a-charts-own-look).

### Hiding the bars

<kbd>Ctrl</kbd> + <kbd>Alt</kbd> + <kbd>Shift</kbd> + <kbd>H</kbd> hides the focused chart's bars, and pressing it again shows them, drawn in the chart's style. Choosing **None** in the **Bar style** submenu does the same, and choosing a style shows them again. Hidden bars are always the chart's alone, for a chart that is all indicators, and are not part of its look.

Only the bars go. The price scale still fits them, and the indicators, drawings, last price and crosshair stay. Hidden bars are kept with the chart, and a chart made by splitting it starts with its bars hidden too.

While they are hidden, a struck-through candle after the chart's resolution says so. Click it to show them again.

The **Show bars** switch of [Chart settings](/manual/chart-settings/) and `omacharts chart set --bars off` hide them too.

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

The time axis is labelled in the time zone and the time format chosen in Preferences › General › Time ([Time](/manual/preferences/#time)), New York time unless you change it.

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
  key ctrl+shift+b                          # sidebar hidden
  omacharts chart split horizontal    # so the left chart's price axis is mid-window and the menu has room
  omacharts chart focus pos:0
  move 1250 710
  run bin/virtual-pointer --extent 1280x720 --button right click 600 300   # the left chart's price axis
capture: bin/headless-window shot /tmp/axismenu.png
crop: 490,300,1180,536   (the menu with 20px layout margin right and below)
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

The wheel zooms a step a notch. A trackpad's two-finger swipe up or down zooms by how far the fingers travel, so a slow swipe zooms a little. Preferences › General › Chart › **Zoom sensitivity** makes both gentler or faster, for every chart at once, and also sets how far the wheel stretches the price axis.

A chart can be panned a little past either end of its bars. One panned back into history stays there as new bars arrive. <kbd>↑</kbd> <kbd>↓</kbd> over a chart move through the watchlist, not the chart ([Watchlists](/manual/watchlists/)).

## Go to

<kbd>Ctrl</kbd> + <kbd>G</kbd> opens **Go to**. Type a date, such as `3 weeks ago`, and the chart jumps there and keeps the window you have. Or set the window yourself, as a span such as `6m` or a custom range such as `oct 1:oct 22`. Either way it is a few keystrokes and <kbd>Enter</kbd>.

<!-- regenerate:
theme: tokyo-night
window: 2560x1440 @2, bin/headless-window (HOME sandboxed, theme seeded)
state:
  (the Change resolution state above, without its keys)
  key ctrl+g
  type ytd
capture: bin/headless-window shot /tmp/goto.png   (icon theme seeded too: gtk-icon-theme-name=Yaru-magenta in the sandboxed HOME's gtk-4.0/settings.ini)
crop: 898,193,763,532   (the whole card, 24px layout margin)
-->
{% include figure.html src="/assets/manual/charts/go-to.png" alt="The Go to card with ytd typed, 1D auto on the row under the box, and From 2026/01/01 and To 2026/11/01, marked today" caption="**Go to** with ytd typed." %}

**A date (keeps the window)** starts the chart at the beginning of that day and shows as much time as it shows now, at the same resolution. With ten days on screen, `oct 12` shows 12 October to 22 October. When that would run past now, the window ends now instead and starts as much earlier, so the date lands inside it. With ten days on screen on 1 November, `oct 28` and `today` both show 22 October to 1 November. The day you typed is tinted and named on the time axis until you zoom or pan. Whatever of the window is past the latest bar is empty room, as when you scroll past the end, and the bars get wider or narrower to fit it. The card names the whole window, such as **22 Oct – 1 Nov**.

| Typed | Is |
|---|---|
| `3 weeks ago`, `3w ago`, `a month ago`, `1y ago` | That long before today, to the day. On 1 November, `3 weeks ago` is 11 October |
| `today`, `yesterday` | That day |
| `fri`, `2fri` | The latest Friday, today included, and the one before it. Any day of the week, `mon` to `sun`, and any count |
| `last fri` | The latest Friday before today. On a Friday, a week ago |
| `oct 22`, `22 oct`, `october 22`, `10/22` | The latest 22 October that has passed |
| `22 oct 2025`, `2025/10/22`, `2025-10-22` | That day in that year |
| `on oct 22`, `on the 22 oct` | The same day. `on` changes nothing |

Numbers run year, month, day, so `10/11` is 11 October. A day still to come, such as `tomorrow` or `next fri`, is refused: the box turns red and the card stays open.

**A span** is back from today. `3w` is the last three weeks, up to today; `3 weeks ago` is a date, the one day. A span counts back from the latest bar, so on a Sunday the week to date is the week that just traded.

| Typed | Shows |
|---|---|
| `6m`, `3y`, `2w`, `10d` | The last six months, three years, two weeks or ten days. A small letter counts back from today, and a letter alone is one: `m` is the last month, `q` the last three |
| `6 months`, `3 years`, `2 wks`, `10 days` | The same, with the unit spelled out or shortened, with or without a space, in any case: `d` `day`, `w` `wk` `week`, `m` `mo` `month`, `q` `qtr` `quarter`, `y` `yr` `year` |
| `Y`, `M`, `Q`, `W` | The year, month, quarter or week to date. A capital counts whole periods, so `2Y` is this year to date and all of last year |
| `ytd`, `qtd`, `mtd`, `wtd`, `2ytd` | The same, in any case |
| `2025` | All of 2025 |
| `2025/03`, `2025-03` | All of March 2025 |
| `june`, `jan`, `Sept` | All of the latest June, January or September. In June, `june` is this June, all of it, with the days after the latest bar as empty room |
| `june 2025`, `jan 2024` | All of that month |
| `-2w` | The same as `2w` |
| `all`, `:` | The whole history |

**A custom range** is two ends joined by `:` or `..`, both included, with or without spaces around it. Either end can be left out. An end is anything a date can be, a month such as `2025/03` or `jan`, a year, or a count back such as `-6m` or `-6 months`.

| Typed | Shows |
|---|---|
| `3 weeks ago:` | From that day to today. The card says **since 11 Oct** |
| `oct 1:oct 22` | 1 to 22 October |
| `jan:mar` | 1 January to 31 March, the latest of each |
| `2025:` | From 1 January 2025 to today |
| `2025/03:2025/06` | March to June 2025 |
| `:2020` | From the first bar to the end of 2020 |
| `mon:` | From Monday to today |
| `-6m:-1m` | From six months ago to one month ago |
| `oct 1..oct 22` | The same as `oct 1:oct 22` |

Left open at the end, it runs to today, so new bars stay in it.

**From** and **To**, under the box, are the first and last day the chart will show. **today** over **To** means it runs to the latest bar. With more than one chart in the window, every chart fades and the one it will change wears a wider focus ring. The ⓘ beside the title has a few examples of each kind. Text that cannot become one of them turns the box red.

| Key | In Go to |
|---|---|
| <kbd>Enter</kbd> | Goes to what you typed. <kbd>Ctrl</kbd> + <kbd>Enter</kbd> does it from anywhere in the card |
| <kbd>Space</kbd> or <kbd>?</kbd> in an empty box, or <kbd>↓</kbd> | Drops a list under the box: the common ranges when nothing is typed, or what the text could mean, each with the day it starts. <kbd>↑</kbd> <kbd>↓</kbd> move through it and <kbd>Enter</kbd> takes the highlighted row |
| <kbd>Tab</kbd> | Moves to the resolution under the box, then to **From** and **To** |
| <kbd>Esc</kbd>, or a click outside | Closes it, changing nothing |

<!-- regenerate:
theme: tokyo-night
window: 2560x1440 @2, bin/headless-window (HOME sandboxed, theme seeded)
state:
  (the state above)
  key ctrl+a BackSpace     # the box emptied
  key space
capture: bin/headless-window shot /tmp/golist.png
crop: 898,193,763,889   (the card and the list, 24px layout margin)
-->
{% include figure.html src="/assets/manual/charts/go-to-list.png" alt="The Go to card with the list of common ranges dropped under its empty box: ytd, 1y, 6m, 3m, mtd, 1m, 2026-10, 2026, 2025: and qtd, each with the day it starts" caption="The common ranges, with nothing typed." %}

### The resolution in Go to

Under the box, at its right, is the resolution the chart will show. **auto** beside it means Omacharts chose it. **auto** alone, before anything is typed, means it will choose once there is something to show.

The resolution stays the chart's own unless it cannot show the window: too few bars, too many, or further back than the feed keeps bars of that resolution. Then Omacharts takes the resolution on the strip that fits best, or one off the strip, such as `5m`, when none on it fits. A year of 5m bars is shown daily, and a month of daily bars hourly.

To choose one yourself, <kbd>Tab</kbd> to it. Only a resolution that can show the window is offered. The arrows, the wheel and typing skip the rest, and a resolution you chose goes back to **auto** when the window changes to one it cannot show.

| Key or gesture | On the resolution |
|---|---|
| <kbd>↑</kbd> <kbd>↓</kbd> | Counts it up or down: `1D`, `2D`, `3D` |
| <kbd>←</kbd> <kbd>→</kbd> | A shorter or longer unit with the same count: `2D` becomes `2h` or `2W`. When that count cannot show the window, the nearest one that can, from the strip first |
| Typing one, such as `4h` or `W` | Sets it |
| The wheel | Steps along the resolution strip |
| <kbd>Backspace</kbd> or <kbd>Delete</kbd> | The chart's own resolution. Pressed again, back to **auto** |
| A click, or <kbd>Space</kbd> | Lists **auto**, the chart's own resolution, then the strip's resolutions and the common ones off it that can show the window |

### From and To

<kbd>Tab</kbd> from the resolution goes into **From**, then **To**. Type a day the way the box takes one: `2026/03/21`, `23 nov`, `yesterday`, `mon`. A month or a year works too; **From** takes its first day and **To** its last. <kbd>↑</kbd> <kbd>↓</kbd> move the day by one, with <kbd>Shift</kbd> by ten. The calendar in the corner of each opens the month under the card, with **From** to **To** shaded, to pick a day with the pointer. Its title, such as **October 2026**, shows the months of the year instead, and clicked again a dozen years. The box above then shows the custom range the two days make, such as `03/21:`.

### How long it holds

The chart keeps the window until you zoom or pan. A new symbol, new bars and a resized window are laid across the same days, so the year to date stays the year to date on any symbol. While it holds, its name shows in the corner under the price axis, such as **YTD**. It is not saved with the chartbook.

`omacharts chart range ytd` does the same from a terminal ([`chart range`](/manual/cli/#chart-range)).

## Gridlines

Gridlines are the same on every chart, in every chartbook, and start off. <kbd>Ctrl</kbd> + <kbd>Shift</kbd> + <kbd>G</kbd> shows or hides them. The scales and their labels stay either way. The same switch is the chart menu's **Gridlines**, the **Gridlines** row of [Chart settings](/manual/chart-settings/), and Preferences › Overlays › **Gridlines**.

The switch changes the look the chart wears. On a chart with [a look of its own](#a-charts-own-look) that is the chart's alone. On any other it is every chart's, so the gridlines go on or off on every chart that has no look of its own. The [last price line](#the-last-price-line) works the same way.

## The last price line

A dashed line runs across every chart at the last price, in every chartbook. Preferences › Overlays › **Last price line**, the row of that name in [Chart settings](/manual/chart-settings/), and `omacharts config price-line off` take it off every chart. The price itself stays on the price axis either way.

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
{% include figure.html src="/assets/manual/charts/crosshair.jpg" alt="An NVDA hourly chart with the crosshair and its price and time tags, and the last-price tag on its dashed line" caption="The crosshair's tags in the accent colour, and the last price in the colour of the last bar's direction." %}

The crosshair is a pair of dotted lines that cross at the centre of the bar under the pointer. The price is tagged on the price axis and the bar's time on the time axis (the date alone on daily and coarser bars), in the chosen [time zone and format](/manual/preferences/#time). When each chart reads its exchange's clock, the time ends with the zone, such as **21 Oct 2026 07:30 CT**. Over an indicator strip the price tag shows that strip's value.

The last price is a tag on the price axis, with a dashed line across the chart at its level. Both are in Blue, the default line colour, when the last bar rose, and in the down colour when it fell ([Bars](/manual/themes/#bars)). Out of hours, on a chart of regular hours only, it is the latest trade, coloured by whether it is above or below the session's close.

### Sharing the crosshair

The pointer on one chart draws a dimmed crosshair at the same moment and price on every other chart on screen of the same symbol, each at its own resolution. It does not matter how the charts are linked to the watchlist. Charts of different symbols never share a crosshair.

It is one switch for every chart: Preferences › Overlays › **Crosshair across Follow charts** ([Preferences](/manual/preferences/#overlays)). It is on by default.

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
{% include figure.html src="/assets/manual/charts/feed-failure.png" alt="A EURUSD daily chart with no bars and the line: No data feed in Preferences → Market data charts this symbol." caption="EURUSD on a feed that cannot chart it." %}

| The chart says | What it means | What to do |
|---|---|---|
| *Press Ctrl+K to find a symbol* | The chart has no symbol yet. | Type a letter over it ([Symbols](/manual/symbols/)). |
| *Loading…* | The prices are loading. | Nothing. |
| *No data for this symbol* | The feed has no bars for it, such as a delisted ticker. | Try another symbol, or another feed. |
| *The data feed does not have this symbol.* | The feed has no such ticker. | Check the spelling and venue suffix ([Symbols](/manual/symbols/)). |
| *No data feed in Preferences → Market data charts this symbol.* | The feed cannot chart this kind of instrument. | Switch feed ([Market data feeds](/manual/market-data/)). |
| *The data feed does not serve this resolution.* | The feed has no bars at this resolution. | Try another resolution. |
| *This data feed is not signed in. Open Preferences → Market data.* | thinkorswim is chosen and is not signed in, or the sign-in has expired. | Preferences › Market data › **Sign in…**. |
| *Rate limited by the data feed. Prices fill in shortly.* | The feed has stopped sending prices for a while. | Wait. Prices fill in when it allows them again. |
| *No network connection. Prices fill in when there is one.* | This machine is offline. | Nothing. It fills in when the connection is back. |
| *The data feed is not answering. Prices fill in when it does.* | The feed is not answering. | Wait. If it persists, check the feed in Preferences › Market data. |
| *The data feed sent a reply this could not read.* | The feed sent something that was not prices. | Wait for the next refresh. |
| *Could not open the local price cache.* | The disk is full, or Omacharts cannot write to its data folder. | Check the data folder ([Where your data lives](/manual/your-data/)). |

A chart that has bars but could not get newer ones keeps them and shows a small rose dot in the top-right corner of the plot. Hover over it for the reason.

Prices refresh as bars close. A chart does not refresh while its market is shut or while it is panned back into history. On a streaming feed, prices arrive as they change ([Market data feeds](/manual/market-data/)).

## The header

The header runs across a chart's top. It has two rows, each with a left, a centre and a right, and every part of it goes in one of those six places, in any order. It is the same on every chart in every chartbook, except a chart with [a look of its own](#a-charts-own-look).

| Part | Shows |
|---|---|
| **Symbol** | The symbol, with its venue. Click it to [find another](/manual/symbols/) |
| **Resolution** | The resolution. Click it for the list of the strip's resolutions |
| **Daily change** | The day's move as a percentage, in the chart's up and down colours, and in the header's text when it has not moved. The same figure as the sidebar's |
| **Last price** | The last price, as the sidebar writes it |
| **Chart template** | The name of the [chart template](/manual/chart-templates/) the chart is on, dimmed. Nothing on a chart that is on none |
| **Local visuals** | A window pulled off another, dimmed, on a chart whose [look of its own](#a-charts-own-look) differs from every chart's. Nothing on any other |
| **Name** | The instrument's full name, faded. Nothing when the feed knows no name beyond the ticker |
| **Chart title** | The chart's own title. Click it to [edit it](#chart-titles) |
| **Resolution strip** | The [preset resolutions](#the-preset-resolutions) as buttons, on the focused chart only |
| **Chart settings** | The gear, while the pointer is on the chart ([The corners](/manual/window/#the-corners)) |
| **Maximize** | The two arrows, when the chartbook has two charts or more |
| **Link mode** | The link button ([Linking charts to the watchlist](/manual/watchlists/#linking-charts-to-the-watchlist)) |

Out of the box the first row has the data feed, the symbol, the resolution and the daily change on the left, the chart title in the centre, and the gear, **Maximize** and the link on the right, with the account, the P&L, **Buy**, **Sell** and the quantity ahead of them while trading is on. The second row is empty, and an empty second row takes no room.

Edit it in Preferences › Overlays › **Header**, or right-click the header and choose **Edit header…**, which opens Preferences there. A header with every part taken off has nothing to right-click, so open Preferences › Overlays to put parts back. The page draws the header as a chart shows it, with the parts it does not show underneath. Drag a part to any place in the header, or down among the others to take it off, or right-click or double-click it; click one underneath to put it back where it goes out of the box. The places are outlined while a part is being dragged. A place on the second row takes a part only under a place that has one, and when the last part leaves a place on the first row, the parts under it move up.

On a narrow chart, a row's centre is hidden when it would run into what is either side of it.

From a terminal, `omacharts config header` takes one place per argument, each `place=part,part`:

```
omacharts config header left=symbol,change center=strip right=settings,maximize,link left2=name
```

The places are `left`, `center` and `right`, and `left2`, `center2` and `right2` on the second row. `none` is a header with nothing in it, and `config header` alone prints the header as it is.

### Chart titles

A chart can have a chart title, shown by the header's **Chart title** part, centred across the top. A chart has none until it is given one, and then that place is empty.

There are five ways to edit it:

- **In the header.** Point anywhere across the top of the chart, between the two sides of the header, and an untitled chart shows **Add title** there. Click, and type where it stands. Click an existing chart title to change it the same way. <kbd>Enter</kbd> or clicking away keeps it, <kbd>Esc</kbd> puts the old one back, and blank removes it.
- **<kbd>F2</kbd>**, with no drawing selected. It edits the chart title in the header, or opens the **Chart title** dialog when the header leaves the chart title out or has no room for it. With a drawing selected, <kbd>F2</kbd> puts the caret in the drawing's text instead ([Drawings](/manual/drawings/)).
- **Edit chart title…** in the [chart menu](#the-chart-menu), which opens the **Chart title** dialog. **Save** keeps what is typed, blank included, and **Cancel** changes nothing.
- The **Title** row at the top of [Chart settings](/manual/chart-settings/), which changes it as it is typed.
- `chart set --title`, and `--title ""` to remove it ([Changing charts from the CLI](/manual/cli/#charts)).

<!-- regenerate:
theme: tokyo-night
window: 2560x1440 @2, bin/headless-window (HOME sandboxed, theme seeded)
state:
  omacharts chart set --symbol NVDA --resolution 1D
  key ctrl+shift+b
  click 300 22
  type Semis desk
capture: bin/headless-window shot /tmp/title-editing.png
crop: 0,0,2420,120   (the header, up to the price axis)
-->
{% include figure.html src="/assets/manual/charts/chart-title-editing.png" alt="The top of an NVDA daily chart with Semis desk being typed into the chart title's place in the header" caption="Typing a chart title where it stands." %}

<!-- regenerate:
as above, then
  key Return
  run bin/virtual-pointer --extent 1280x720 --button right click 300 150
  click 386 325           # Edit chart title…
capture: bin/headless-window shot /tmp/title-dialog.png
crop: 940,460,680,520   (the whole dialog)
-->
{% include figure.html src="/assets/manual/charts/chart-title-dialog.png" alt="The Chart title dialog, with a Title field holding Semis desk and Cancel and Save buttons" caption="The Chart title dialog, from Edit chart title… or F2." %}

A chart made by splitting another starts with no chart title.

## A chart's own look

A chart's look is its theme, how its bars are drawn and coloured, its header, its gridlines and its last-price line. Every chart wears the same look until you give one a look of its own.

The first change you make to one chart alone copies every chart's look onto it, then makes the change. From then on, a change to every chart's look leaves that chart as it was. Colours are kept as the names you picked, such as **Blue** or **Foreground**, so a new theme recolours the chart the way it recolours every other. A hex colour stays that colour.

A chart with a look of its own is on the window's theme until you pick one for it under Preferences › **LOCAL** › Appearance › Theme. The chart then draws in that theme: its background, scales and bars, and its header, legend and buttons too. The rest of the window keeps the window's theme. The reset arrow after **LOCAL** puts the chart back on the window's theme with the rest of every chart's look.

To give one chart a look of its own, focus it, open Preferences and pick **LOCAL** on the **VISUAL** rule, then change Appearance or Overlays. The reset arrow after **LOCAL** puts it back on every chart's look. Choosing a style from a chart's **Bar style** submenu also gives that chart a look of its own. A chart's own look comes with it when you split it or duplicate its chartbook.

`omacharts chart look` gives the focused chart a look of its own, reads the look it wears, and with `--follow` puts it back on every chart's ([`chart look`](/manual/cli/#chart-look)).

```sh
omacharts chart look --style ohlc --up blue --grid off
omacharts chart look --theme catppuccin-latte
omacharts chart look --follow
```

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
crop: 560,262,640,986   (the whole menu, 20px layout margin)
-->
{% include figure.html src="/assets/manual/charts/chart-menu.png" alt="The chart menu open over a chart, each item with its key" caption="The chart menu on a chartbook of two charts." %}

| Item | Does | Explained in |
|---|---|---|
| **Paste** | Puts down the drawings or the indicator last copied. Only there while something has been copied | [Drawings](/manual/drawings/), [Indicators](/manual/indicators/#selecting-an-indicator) |
| **Bar style** › **Candles**, **OHLC bars**, **Line**, **None** | How this chart draws its bars, in [a look of its own](#a-charts-own-look), or its bars hidden, with its key | [Bars](#bars) |
| **Session** › **Extended hours**, **Regular hours only** | The chart's hours | [Sessions](#sessions) |
| **Template** › **None**, then each chart template | Puts the chart on a template, with its key | [Chart templates](/manual/chart-templates/) |
| **Template** › **Create new template from chart…** | Saves the chart's indicators as a new template and puts the chart on it. Greyed out on a chart with no indicators | [Chart templates](/manual/chart-templates/#creating-a-template) |
| **Gridlines** | Ticked while the charts have them. The same on every chart | [Gridlines](#gridlines) |
| **Indicators** | Ticked while the chart shows its indicators, with its key | [Hiding every indicator](/manual/indicators/#hiding-every-indicator) |
| **Drawings** | Ticked while the chart shows its drawings, with its key | [Hiding the drawings](/manual/drawings/#hiding-the-drawings) |
| **Edit chart title…** | Asks for the chart title, with its key. Blank removes it | [Chart titles](#chart-titles) |
| **Split horizontally**, **Split vertically** | Divides the chart in two | [Splitting and arranging charts](/manual/layouts/) |
| **Sidebars** › **Drawing tools**, **Watchlist** | Shows or hides each sidebar, ticked while it is open, with its key | [The tools](/manual/drawings/#the-tools), [The sidebar](/manual/watchlists/#the-sidebar) |
| **Maximize** / **Restore** | Gives the chart the window, or puts it back. Only with two charts or more | [Splitting and arranging charts](/manual/layouts/#maximising-a-chart) |
| **Close chart** | Closes it. Only with two charts or more | [Splitting and arranging charts](/manual/layouts/#closing-a-chart) |
| **Watchlist** › **Focus**, **Follow**, **Pinned** | The chart's link mode | [Choosing a link mode](/manual/watchlists/#choosing-a-link-mode) |
| **Take screenshot** | A picture of the chart, to the clipboard | [Screenshots](/manual/screenshots/) |
| **Remove all drawings** | Deletes every drawing on the chart. Only there while it has some | [Drawings](/manual/drawings/) |
| **Indicators…** | The Indicators page of Chart settings | [Indicators](/manual/indicators/) |
| **Chart settings…** | The Chart page of Chart settings | [Chart settings](/manual/chart-settings/) |

## See also

- [Changing charts from the CLI](/manual/cli/#charts)
