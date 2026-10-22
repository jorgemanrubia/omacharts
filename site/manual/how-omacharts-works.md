---
title: "How Omacharts works"
description: "The window, charts, chartbooks, watchlists, templates, drawings, the theme, the CLI, the Omarchy plugin and market data."
section: introduction
nav_order: 10
---

## The window

The chart fills the window. There is no header bar and no toolbar. The
sidebar, the drawing tools and the chartbook strip can each be hidden.

<!-- regenerate:
theme: tokyo-night
window: 2560x1440 @2, bin/headless-window
state:
  omacharts chartbook create Desk --symbol NVDA --switch
  omacharts chart split vertical
  omacharts chartbook create Macro --symbol SPY --switch
  omacharts chart indicator add sma --period 50 --color Blue
  omacharts chart indicator add sma --period 200 --color Amber
  key ctrl+d
  move 1270 710
  (the strip also held "Chartbook 1" and "Semis" from the quick start run)
capture: bin/headless-window shot window.png
crop: none, resized to 2000 wide
-->
{% include figure.html src="/assets/manual/how-omacharts-works/window.jpg" alt="The window with the drawing tools on the left, a daily SPY chart with two moving averages, the Default watchlist in the sidebar and four chartbook tabs along the bottom" caption="The window with the drawing tools, the sidebar and the chartbook strip shown." wide="true" %}

Around the chart:

- **The sidebar**, on the right, holds the watchlist.
  <kbd>Ctrl</kbd> + <kbd>B</kbd> shows and hides it.
- **The drawing tools**, on the left, one per kind of drawing.
  <kbd>Ctrl</kbd> + <kbd>D</kbd> shows and hides them.
- **The chartbook strip**, along the bottom, with a tab per chartbook. It
  appears when you make a second chartbook (<kbd>Ctrl</kbd> + <kbd>N</kbd>).
- **The window's corner**, top right, holds the watchlist toggle and the
  main menu (**New chartbook**, **Preferences** and **Keyboard Shortcuts**).
  Drag the blank space beside them to move the window.

Each chart shows its symbol and resolution at its top-left. Click either to
change it.

Type a letter over a chart to search for a symbol, or a digit to set the
resolution. <kbd>?</kbd> lists every key. See [The window](/manual/window/)
and [Hotkeys](/manual/hotkeys/).

## Charts

A chart shows one instrument at one resolution, such as NVDA on the daily or
SPY on the five-minute. Each chart has its own indicators, trading hours, bar
style, colouring, gridlines and price scale. New charts start from Preferences ›
Charts › Defaults ([Chart settings](/manual/chart-settings/)).

See [Symbols and search](/manual/symbols/), [Charts](/manual/charts/) and
[Indicators](/manual/indicators/).

## Splitting a chart

<kbd>Ctrl</kbd> + <kbd>Alt</kbd> + <kbd>H</kbd> and
<kbd>Ctrl</kbd> + <kbd>Alt</kbd> + <kbd>V</kbd> split the focused chart in
two. The new half is a copy with the same symbol, resolution and indicators.
Drag a divider, or hold <kbd>Ctrl</kbd> + <kbd>Alt</kbd> + <kbd>Shift</kbd>
and press an arrow, to give a chart more room.

One chart is always **the focused chart**, drawn with a ring. Keys and menus
act on it. <kbd>Ctrl</kbd> + <kbd>M</kbd> gives it the whole window, or puts
it back.

Right-click a chart to open its menu.

See [Splitting and arranging charts](/manual/layouts/).

## Chartbooks

A chartbook is a saved screen: the charts, how they are split, which one is
maximised, and the watchlist beside them. Keep as many as you like.

<kbd>Ctrl</kbd> + <kbd>N</kbd> makes a new chartbook.
<kbd>Ctrl</kbd> + <kbd>Alt</kbd> + <kbd>←</kbd> and <kbd>→</kbd> move
between them, as does clicking a tab in the strip. Leave a chartbook, or
quit, and it comes back as you left it.

Each chartbook shows one watchlist, and a watchlist belongs to one
chartbook, except the default watchlist, which any chartbook can show.

See [Chartbooks](/manual/chartbooks/).

## Watchlists

A watchlist is a list of symbols in sections, with the last price and the
day's change beside each. It lives in the sidebar. Click a symbol, or move
through the list with <kbd>↑</kbd> and <kbd>↓</kbd>, and the charts change to
it. On a chart, <kbd>↑</kbd> and <kbd>↓</kbd> move to the previous or next
symbol of the watchlist without opening the sidebar.

Each chart has a link mode that says whether it changes:

- **Follow** changes with every click in the watchlist. New charts are Follow.
- **Focus** changes only while the chart is focused.
- **Pinned** never changes from the watchlist.

<!-- regenerate:
theme: tokyo-night
window: 2560x1440 @2, bin/headless-window
state:
  omacharts chartbook create Desk --symbol NVDA --switch
  omacharts chart split vertical
  omacharts chart split horizontal --chart pos:0
  omacharts chart split horizontal --chart pos:2
  omacharts chart set --chart pos:1 --resolution 1h
  omacharts chart set --chart pos:2 --resolution 15m
  omacharts chart set --chart pos:3 --resolution 5m
  move 1270 710
capture: bin/headless-window cli chartbook screenshot --output desk.png
crop: none
-->
{% include figure.html src="/assets/manual/how-omacharts-works/desk.jpg" alt="Four NVDA charts in a two-by-two grid, daily, hourly, fifteen-minute and five-minute" caption="Four Follow charts showing NVDA at four resolutions." %}

Pick AMD in the sidebar and all four charts change to AMD. The crosshair on
one is drawn on the others. The link button in a chart's corner picks its mode, and
<kbd>Ctrl</kbd> + <kbd>L</kbd> moves the focused chart to the next mode.

See [Watchlists](/manual/watchlists/) and
[Linking charts to the watchlist](/manual/watchlists/#linking-charts-to-the-watchlist).

## Chart templates

A chart template is a saved set of indicators that several charts share.
Add or remove an indicator on the template and every chart on it changes, in
every chartbook. New charts open on the **Default** template, which holds
volume.

<!-- regenerate:
theme: tokyo-night
window: 2560x1440 @2, bin/headless-window
state:
  (the Desk chartbook from the figure above)
  omacharts template create Momentum
  omacharts template indicator add Momentum rsi --period 14
  omacharts template indicator add Momentum macd
  omacharts chart set --chart pos:0 --template Momentum
  omacharts chart set --chart pos:1 --template Momentum
  omacharts chart set --chart pos:2 --template Momentum
  omacharts chart set --chart pos:3 --template Momentum
  move 1270 710
capture: bin/headless-window cli chartbook screenshot --output desk-template.png
crop: none
-->
{% include figure.html src="/assets/manual/how-omacharts-works/desk-template.jpg" alt="The same four NVDA charts, each with an RSI and a MACD under its candles" caption="The four charts on the *Momentum* chart template." %}

<!-- regenerate:
theme: tokyo-night
window: 2560x1440 @2, bin/headless-window
state:
  (as the figure above)
  key ctrl+comma
  click 215 147     (Charts)
  click 695 63      (Templates)
capture: bin/headless-window shot prefs.png
crop: 940,85,1180,480
-->
{% include figure.html src="/assets/manual/how-omacharts-works/templates.png" alt="The Templates tab listing the Default and Momentum chart templates with their indicators" caption="The Templates tab of Preferences › Charts." %}

Change the indicators of one chart by hand and it leaves its template,
keeping the indicators it had. Its header no longer names a template.

## Chartbook templates

A chartbook template is a saved arrangement that several chartbooks share.
It holds the splits, the divider positions and each chart's resolution and
indicators, but not the symbols. Split, close a chart or change a resolution
in one chartbook on the template and the others change too, each keeping its
own symbols.

See [Chart templates](/manual/chart-templates/) and
[Chartbook templates](/manual/chartbook-templates/).

## Drawings

A drawing belongs to the symbol, not to the chart. A trendline drawn on the
NVDA daily appears on the NVDA hourly and on every other chart of NVDA. Each
chart has two switches to change this. One keeps what you draw on it to
itself, the other hides what other charts drew.

<kbd>Alt</kbd> and a letter picks a tool (<kbd>Alt</kbd> + <kbd>L</kbd> a
line, <kbd>Alt</kbd> + <kbd>H</kbd> a level, <kbd>Alt</kbd> + <kbd>R</kbd> a
rectangle, <kbd>Alt</kbd> + <kbd>T</kbd> text). Each kind of drawing has nine
configurations, on <kbd>Alt</kbd> + <kbd>1</kbd> to <kbd>9</kbd>. See
[Drawings](/manual/drawings/).

## The theme

Omacharts uses the Omarchy desktop's theme. Change the theme and the window
changes with it. Candles take the theme's up and down colours, and indicators take their
colours from its palette.

A colour is a name from the theme's palette or a hex value. An indicator
coloured **Amber** takes the theme's amber and changes with the theme. A hex
value stays as you set it. Preferences › Theme also has built-in themes, and
lets you copy one and edit it. See [Themes and colours](/manual/themes/).

## The CLI

The `omacharts` command does everything the window does. With a window
open, the command acts in it. A watchlist created in a terminal appears in
the sidebar at once.

```
$ omacharts chart indicator add rsi
added RSI(14) on pos:0 SPY 1D in "Macro"
  [exit 0]
```

<!-- regenerate:
theme: tokyo-night
window: 2560x1440 @2, bin/headless-window
state:
  omacharts chartbook create Macro --symbol SPY --switch
  omacharts chart indicator add sma --period 50 --color Blue
  omacharts chart indicator add sma --period 200 --color Amber
  omacharts chart indicator add rsi
  move 1270 710
capture: bin/headless-window cli chart screenshot --output rsi-from-terminal.png
crop: none
-->
{% include figure.html src="/assets/manual/how-omacharts-works/rsi-from-terminal.jpg" alt="A daily SPY chart with two moving averages, volume and an RSI at the bottom" caption="The SPY chart with the RSI added." %}

A command that names no chart acts on the focused chart. Commands that
name what they act on work without a window, over ssh too.

Scripts and agents use the same command, and a skill teaches an agent when
to use it. See [The CLI](/manual/cli/) and
[The agent skill](/manual/agent-skill/).

## The Omarchy plugin

The Omarchy plugin shows the default watchlist in the Omarchy bar, with the
last price, the day's change and a sparkline per symbol. It stays there after
the window is closed. Click a symbol and the window opens on it. The plugin
is added on the first launch. Remove it and it stays removed. See [Omarchy plugin](/manual/bar-widget/).

## Market data

Prices come from a market data feed. The default feed needs no account.
Choose another and sign in to it in Preferences › Data. See
[Market data feeds](/manual/market-data/).

## Your data

Your watchlists, chartbooks, templates, drawings, settings and themes are
kept in one folder. Copy it to another machine to take them with you. A
backup can be restored from Preferences. See
[Where your data lives](/manual/your-data/).
