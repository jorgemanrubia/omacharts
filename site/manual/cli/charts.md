---
title: "Charts"
description: "Changing a chart's symbol, resolution and look, the days it shows, and the bars every chart draws."
section: agents
parent: cli
nav_order: 2
---

`chart set` changes one chart, and `chart range` the days it shows.
`config bars` and `config set` change every
chart, or what new charts open with. What each setting looks like on screen
is in [Charts](/manual/charts/).

`chart bars [--last N]` reads a chart's last bars back as the window holds
them, oldest first: time, open, high, low, close and volume. `chart values`
reads its indicators back as drawn, the latest or bar by bar, and `chart
stream` keeps reading them as the market moves, as described below. All
three read the chart on screen, so they need a window open; `--json` for a
program.

## `chart set`

Changes what a chart shows and how. The reply names the chart and each
change.

```
$ omacharts chart set --symbol NVDA --resolution 240
chart 2: symbol NVDA, resolution 4h
  [exit 0]

$ omacharts chart set --style candles --colouring theme-soft --hollow-up on --session regular --grid off --auto-scale off --log-scale on
chart 4: style candles, colouring theme-soft, rises hollow, session regular, grid off, auto-scale off, log scale on
  [exit 0]

$ omacharts chart set --title "Semis desk"
chart 4: title "Semis desk"
  [exit 0]

$ omacharts chart set --title ""
chart 4: untitled
  [exit 0]

$ omacharts chart set --resolution 3x
omacharts: "3x" is not a resolution; try 5m, 1h, 1D, 1W, 1M or 1Y
  [exit 2]
```

| Flag | Takes | Explained in |
|---|---|---|
| `--title TITLE` | The chart title, shown in its header; `""` for none | [Chart titles](/manual/charts/#chart-titles) |
| `--symbol SYMBOL` | The instrument, such as `AAPL` or `2330.TW`, or [arithmetic on symbols](/manual/symbols/#arithmetic-on-symbols), such as `GC/SI` | [Symbols and search](/manual/symbols/) |
| `--suffix S` | Its venue suffix, for a listing abroad | [Symbols and search](/manual/symbols/#listings-abroad) |
| `--resolution TF` | A resolution as typed on the chart | [Resolutions](/manual/charts/#resolutions) |
| `--style STYLE` | `candles`, `ohlc`, `line`, or `none` to hide the bars | [Bars](/manual/charts/#bars) |
| `--thickness PX` | How many pixels wide OHLC bars are drawn, `1` to `4` | [Style](/manual/charts/#style) |
| `--bars on` or `off` | Shows the bars in the style they had, or hides them | [Hiding the bars](/manual/charts/#hiding-the-bars) |
| `--colouring C` | `theme` (Vivid), `theme-soft`, `theme-quiet`, `theme-mono`; `theme-hollow` and `theme-red-up`, from earlier versions, are `theme` with the matching switch on | [Colouring](/manual/charts/#colouring) |
| `--hollow-up on` or `off` | Draws a rising candle as an outline | [Colouring](/manual/charts/#colouring) |
| `--swap-colours on` or `off` | Paints the colouring the other way round, red for a rise | [Colouring](/manual/charts/#colouring) |
| `--session S` | `regular`, `extended` | [Sessions](/manual/charts/#sessions) |
| `--grid on` or `off` | Draws the grid | [Gridlines](/manual/charts/#gridlines) |
| `--auto-scale on` or `off` | Fits the price axis to the visible bars, or holds it still | [Scales](/manual/charts/#scales) |
| `--log-scale on` or `off` | Logarithmic or linear price axis | [Logarithmic scale](/manual/charts/#logarithmic-scale) |
| `--indicators on` or `off` | Draws the chart's indicators, or hides them all | [Hiding every indicator](/manual/indicators/#hiding-every-indicator) |
| `--legend on` or `off` | Shows the indicator legend, or none of it | [The legend](/manual/indicators/#the-legend) |
| `--link MODE` | `focus`, `follow`, `pinned` | [Linking charts to the watchlist](/manual/watchlists/#linking-charts-to-the-watchlist) |
| `--template T` | A chart template, or `none` | [Chart templates](/manual/chart-templates/) |
| `--drawings on` or `off` | Draws the chart's drawings, its own and the shared, or hides them all | [Hiding the drawings](/manual/drawings/#hiding-the-drawings) |
| `--show-drawings on` or `off`, `--send-drawings on` or `off` | Whether the chart shows other charts' drawings, and whether it sends its own | [Drawings](/manual/drawings/) |
| `--feed FEED` | The data feed the chart is drawn from, by its id or its name as `provider list` gives them, or `default` to follow the order | [The header](/manual/charts/#the-header) |
| `--account ACCOUNT` | The account the chart trades on: its name, its number, its login and number such as `Acme Futures/ACME-0001`, or `default` to follow the default account | [The Account part](/manual/trading-on-a-chart/#the-account-part) |
| `--quantity N` | The quantity its Qty is set to, one its quantities offer | [The Account part](/manual/trading-on-a-chart/#the-account-part) |
| `--trading-symbol SYMBOL` | Trade the other size of what it charts, such as `MES` on an ES chart; `none` to trade what it charts | [Trading another size](/manual/trading-on-a-chart/#trading-another-size) |
| `--show-fills on` or `off` | Marks the account's executions on the chart | [Executions on the chart](/manual/trading-on-a-chart/#executions-on-the-chart) |
| `--order-template T` | The order template its orders are bracketed with, by name or `id:N`, or `none` (Beta) | [Order templates](/manual/trading-on-a-chart/#order-templates) |
| `--book BOOK` | Which chartbook, by name or `id:N`; the open one when not given | [Which chart a command acts on](/manual/cli/#which-chart-a-command-acts-on) |
| `--chart CHART` | Which chart: `pos:N`, or a raw id from `chart list`; the focused one when not given | [Which chart a command acts on](/manual/cli/#which-chart-a-command-acts-on) |
| `--json` | The result as JSON | |

`--hollow-up on` on an OHLC chart is accepted and changes nothing you can
see until the chart is candles again, since OHLC bars have no body.

`--style none` and `--bars off` both hide the bars and keep their style.
`--bars on` brings them back in it, and `--style candles` or `--style ohlc`
shows them in that one. A style and `--bars` that disagree, such as
`--style candles --bars off`, are refused. In JSON, `style` is `none` while
the bars are hidden, `bar_style` is the style they come back in, and `bars`
says whether they are shown.

## `chart range`

Does on the focused chart what **Go to** (<kbd>Ctrl</kbd> + <kbd>G</kbd>)
does. `RANGE` is a date, a span or a custom range, written the way **Go to**
takes it ([Go to](/manual/charts/#go-to)). The reply says which of the three
it was, the days the chart shows, the chart it acted on and, when the
resolution moved, the new one. Put quotes around a range with a space in it.

```
$ omacharts chart range ytd
span: year to date on pos:0 NVDA 1D in "Manual"
  [exit 0]

$ omacharts chart range "3 weeks ago:"
custom range: since 11 Oct on pos:0 NVDA 1D in "Manual", now 1h
  [exit 0]

$ omacharts chart range "1 week ago"
date: 3 Oct – 1 Nov (window kept) on pos:0 NVDA 1h in "Manual"
  [exit 0]

$ omacharts chart range 2025: --resolution 1W
custom range: since 1 Jan 2025 on pos:0 NVDA 1h in "Manual", now 1W
  [exit 0]

$ omacharts chart range 6x
omacharts: 6x is not a date, span or custom range; try oct 22, ytd or 2025:
  [exit 2]

$ omacharts chart range tomorrow
omacharts: tomorrow is in the future, with no days up to today
  [exit 2]
```

A custom range that starts with a minus, such as `-6m:-1m`, goes after `--`,
or it reads as a flag: `omacharts chart range -- -6m:-1m`.

| Argument or flag | Takes |
|---|---|
| `RANGE` | A date, a span or a custom range |
| `--resolution TF` | Shows it at this resolution, whether or not it fits. Without it, the resolution is chosen as in **Go to** |

It acts on the chart on screen, so with no window open it exits `6`. The
window holds until the chart is zoomed or panned and is not stored, so a
command that changes the arrangement, such as `chart set`, starts the view
over. Run `chart range` last.

## `chart values`

Reads a chart's indicators back in numbers, exactly as they are drawn. With
no bars named it gives each one's latest reading and the levels read off the
chart: the session's VWAP and bands, the volume profile's point of control
and value area, and the session's range. Name bars and it gives each of
those bars' readings instead, without the levels.

| Flag | Takes | For |
|---|---|---|
| `--indicator KIND` | an indicator kind, such as `sma` or `footprint`; once per kind | Only indicators of that kind. `footprint` reads the chart's volume at price whether a footprint is drawn or not |
| `--id N` | an id from `chart indicator list`; once per indicator | Only that indicator |
| `--last N` | 1 to 10000 | Every reading of the last N bars, the forming one among them |
| `--from WHEN`, `--to WHEN` | `2026-09-01`, `2026-09-01T14:30` in the chart's clock, or unix seconds | Every reading of the bars opening from `--from` up to, not including, `--to`; without `--to`, up to the newest |
| `--visible` | | Every reading of the bars on screen |
| `--row-ticks N` | `auto`, or 1 to 10000 | Ticks a footprint's rows hold. `auto`, the default, is the step it draws |
| `--footprint` | | The same as `--indicator footprint` |

With neither `--indicator` nor `--id` it reads every indicator but Session
times, which has no values. An id or a kind the chart does not have is
refused with exit `3`, and a kind that does not exist with exit `2`, naming
the kinds there are.

```
$ omacharts chart indicator add sma --period 3
added SMA(3) on pos:0 ES 1D in "Chartbook 1"
  [exit 0]

$ omacharts chart values --indicator sma --last 2 --json
{"chart":"pos:0 ES 1D in \"Chartbook 1\"","symbol":"ES","resolution":"1D","indicators":[{"id":1,"kind":"sma","label":"SMA 3","visible":true,"bars":[{"value":328.39,"time":1794268800},{"value":319.28,"time":1794355200}]}]}
  [exit 0]
```

Every entry in `indicators` has the indicator's `id`, `kind`, `label` and
`visible`, then what it reads. A bar's reading has its `time`, when the bar
opens in unix seconds, and fields named as in the `omacharts-studies` type the
indicator reads as, which is the arithmetic the chart draws from:

| Indicator | The latest | A bar's reading in `bars` |
|---|---|---|
| SMA, EMA | `value` | `value` |
| RSI, ATR, Volume | `value` | `value` |
| MACD | `value`, `signal`, `histogram` | `line`, `signal`, `histogram` |
| Stochastic | `value` (%K), `signal` (%D) | `k`, `d` |
| VWAP, Bollinger | `value`, and `bands`: `deviations`, `upper`, `lower` for each band shown | `center`, and `bands` as for the latest |
| Cumulative Delta, Volume Delta | `value` (the close) | `open`, `high`, `low`, `close`, `estimated` (read from the bar's direction, not volume at price), `starts` (the first bar of its period) |
| Volume profile | `from`, `to`, `poc`, `vah`, `val`, `volume` of the newest period | `profiles`: the same for every period that reaches into the bars |
| Market profile | `from`, `to`, `poc`, `vah`, `val`, `tpos`, `row_ticks` of the newest period | `profiles`, as for the volume profile |
| Footprint | `tick_size`, `row_ticks` and the newest bar in `bars` | `tick_size`, `row_ticks` and `bars`, row by row: see [Footprint](/manual/cli/indicators/order-flow/footprint/#reading-it-out) |
| Large orders | `orders` in the newest bar | `orders` in the bars |

A large order is its `time` (unix seconds, and `time_ns` in nanoseconds), its
`price` (the average of its fills, weighted by size), `size`, `side` (`buy`,
`sell` or `unknown`), `fills` and `regular` (whether it printed in regular
hours). `orders` is `null` until the chart has read its trades. An
indicator the chart's feed cannot compute, such as a footprint without
volume at price, has `unmet` with the line it draws instead.

Without `--json` it writes each indicator's label and latest reading on a
line, or its label and then a line a bar, a profile or an order.

## `chart stream`

Keeps reading a chart's indicators as the market moves, one JSON object a
line, each written out as soon as it is ready, so a program can act on each
change as it comes:

- a `snapshot` first, with the chart, its `symbol` and `resolution`, the
  newest bar and its readings
- an `update` whenever the newest bar changes, at most every `--every`
  milliseconds; changes in between are not lost, as the next update says
  the bar as it is then
- a `close` as each bar ends, with its final readings, never held back
- `closed` if the window closes, and `gone` if the chart is taken off the
  screen

A new symbol or resolution on the chart, or its bars read in again, starts
over with a snapshot. The readings are the ones `chart values --last 1`
gives a bar, in the same field names (see the table above), and `bar` is
`[time, open, high, low, close, volume]`.

It reads what the chart has already worked out when its bars moved and draws
nothing, so it costs the window a few microseconds an update. It is not
behind any feature flag.

| Flag | Takes | For |
|---|---|---|
| `--indicator KIND`, `--id N`, `--footprint`, `--row-ticks N` | as for `chart values` | Which indicators, and a footprint's rows |
| `--every MS` | milliseconds; `0` for every change | An update at most this often. The default is `250` |
| `--count N` | 1 or more | Stop after this many updates and closes |
| `--for SECONDS` | 1 or more | Stop after this long |

Otherwise it runs until the window closes, or until whatever reads it stops:
piped into `head`, it ends when `head` does. It needs a terminal or a pipe to
write to.

```
$ omacharts chart stream --indicator sma --every 0 --count 2
{"ev":"snapshot","chart":"pos:0 ES 1D in \"Chartbook 1\"","symbol":"ES","resolution":"1D","bar":[1794355200,314.37,315.48,310.94,312.04,335005.0],"indicators":[{"id":1,"kind":"sma","label":"SMA 3","visible":true,"value":319.28}]}
{"ev":"update","bar":[1794355200,314.37,315.48,310.94,313.04,335010.0],"indicators":[{"id":1,"kind":"sma","label":"SMA 3","visible":true,"value":319.62}]}
{"ev":"close","bar":[1794355200,314.37,315.48,310.94,313.04,335010.0],"indicators":[{"id":1,"kind":"sma","label":"SMA 3","visible":true,"value":319.62}]}
  [exit 0]
```

## `config bars`

Sets the colouring of every chart at once, in every chartbook and for the
charts made next, as **Apply to all charts now** does.

```
$ omacharts config bars
vivid
  [exit 0]

$ omacharts config bars monochrome
bars are monochrome; the colours are remembered
  [exit 0]

$ omacharts config bars red-up
rising bars are red and falling bars green, in the "theme" scheme
  [exit 0]

$ omacharts config bars
vivid, swapped
  [exit 0]

$ omacharts config bars vivid
bars carry their direction in colour again, in the "theme" scheme
  [exit 0]
```

| `STATE` | Does |
|---|---|
| (none) | Prints the current state: `vivid`, `soft`, `quiet` or `monochrome`, followed by `, hollow` when rises are hollow and `, swapped` when up and down are swapped. With `--json`, the `hollow_up` and `swapped` fields |
| `vivid` | Every chart back to the remembered colouring, or to `theme` when nothing is remembered |
| `soft` | Every chart soft: a slate and a sand from the theme's palette instead of its green and red ([Colouring](/manual/charts/#colouring)) |
| `quiet` | Every chart quiet: two greys made from the theme's text, a rise the lighter and a fall the darker ([Colouring](/manual/charts/#colouring)) |
| `monochrome` | Every chart monochrome, remembering the colouring in use |
| `coloured` | The same as `vivid`, from earlier versions |
| `hollow` | `vivid`, with rises hollow on every chart and for new ones. From earlier versions, when Hollow was a colouring; `chart set --hollow-up` and `config set hollow_up` are the switch itself |
| `red-up` | `vivid`, with up and down swapped on every chart and for new ones. From earlier versions, when Red up was a colouring; `chart set --swap-colours` and `config set swap_colours` are the switch itself |

The two switches are worn over the palette, not palettes themselves, so the
other states leave them as they are. `config set hollow_up off` and
`config set swap_colours off` turn one off for new charts, and
`chart set --hollow-up off` or `--swap-colours off` for one chart.

`config set bar_scheme theme-mono` is different. It changes only what new
charts open with, and remembers nothing to go back to.

The chart defaults (`bar_style`, `ohlc_thickness`, `bar_scheme`, `hollow_up`, `swap_colours`, `chart_session`, `show_grid`,
`chart_log_scale`, `chart_shows_legend`, `chart_header`, `timeframes`) are set with `config set`;
see [Settings](/manual/cli/preferences/#settings).
