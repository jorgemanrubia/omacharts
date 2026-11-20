---
title: "Indicators"
description: "Adding, changing, moving and removing a chart's indicators, Session Times among them."
section: agents
parent: cli
nav_order: 3
---

`omacharts chart indicator ACTION [KIND]` changes a chart's indicators.
`--book` and `--chart` pick the chart ([Which chart a command acts
on](/manual/cli/#which-chart-a-command-acts-on)). `KIND` is the indicator's name as the
command takes it, such as `sma`, `rsi`, `volume_profile` or `session_times`.
Each indicator's name, what it draws, and every parameter's default and range
are in [Indicators](/manual/indicators/) and the indicator's own page.

| Action | Does |
|---|---|
| `list` | One line per indicator, with its lengths: `SMA(50)`, `VWAP · week`, `BB(20,2.5)`, and `· selected` after the one selected in the window. With `--json`, each one's id, kind, parameters, colour, line, visibility, `in_legend`, whether the legend names it, and `selected` |
| `add KIND` | Adds one, at its defaults plus whatever flags are given |
| `remove KIND` | Removes every indicator of that kind, or with `--id`, that one |
| `set KIND` | Changes the one of that kind. With two of a kind, `--id N` says which |

| Flag | Takes | For |
|---|---|---|
| `--id N` | An id from `list --json`, or `selected` for the one selected in the window, whose kind can then be left out | `set` and `remove` |
| `--selected` | Nothing | `list`: only the one selected in the window |
| `--period N` | Bars | SMA, EMA, Bollinger, RSI, ATR; the stochastic's %K length |
| `--deviations F` | 0 to 50 | Bollinger |
| `--fast N`, `--slow N`, `--signal N` | Bars | MACD |
| `--signal-color C` | A colour | MACD |
| `--signal-style S` | `solid`, `dashed`, `dotted`; the MACD line's when not given | MACD |
| `--k-smooth N`, `--d-period N` | Bars | Stochastic |
| `--d-color C` | A colour | Stochastic |
| `--d-style S` | `solid`, `dashed`, `dotted`; %K's when not given | Stochastic |
| `--anchor P` | `session`, `week`, `month`, `quarter`, `year`; `never` for one running total | VWAP, volume profile, market profile, cumulative delta; `never` only for cumulative delta |
| `--cvd-style S` | `candles`, `line` | Cumulative delta |
| `--rows N` | `auto`, or a number of rows per period, held between 4 and 800 | Volume profile |
| `--tpo-minutes N` | 1 to 1440; 30 when not given | Market profile: how long each TPO period is |
| `--row-ticks N` | `auto`, or ticks to a row, 1 to 10000 | Market profile |
| `--tpo-style S` | `letters`, `blocks` | Market profile |
| `--open-close B` | `on`, `off`; on when not given | Market profile: whether it marks its open and its close with an arrow each, beside the TPO at its opening price and the one at its closing price |
| `--open-color C`, `--close-color C` | A colour, or `none` for the theme's | Market profile |
| `--expand-last B` | `on`, `off`; off when not given | Market profile: whether its newest profile, the one still forming, is drawn expanded, its TPOs laid out over its bars |
| `--open-transparency F`, `--close-transparency F` | 0 to 1, or `default` for solid | Market profile: how much of the chart shows through its open or close arrow |
| `--value-area F` | 0 to 1; 0.70 when not given | Volume profile, market profile |
| `--poc B` | `on`, `off` | Volume profile, market profile: whether it draws its point of control |
| `--last-only B` | `on`, `off` | Volume profile, market profile: whether it draws only its newest period |
| `--poc-color C` | A colour | Volume profile, market profile |
| `--profile-style S` | `filled`, `outline` | Volume profile |
| `--align A` | `left`, `right` | Volume profile, market profile |
| `--outline B` | `on`, `off` | Volume profile: whether filled rows wear an outline. Market profile: whether its TPOs do, off when not given |
| `--outline-color C` | A colour | Volume profile, market profile |
| `--outline-width F`, `--poc-width F` | 0.5 to 12 | Volume profile, market profile. A market profile's point of control is 2 when not given |
| `--outline-style S`, `--poc-style S` | `solid`, `dashed`, `dotted` | Volume profile, market profile |
| `--transparency F` | 0 to 1; 0.7 on a volume profile and 0.05 on a market profile when not given | Volume profile, market profile: how much of the chart shows through its rows. 0 is its colour solid, 1 invisible |
| `--value-area-transparency F`, `--outside-transparency F` | 0 to 1, or `default` | Volume profile, market profile: the value area's rows and the rows outside it, while it is highlighted. Unset, or `default`, they follow `--transparency` |
| `--outline-transparency F` | 0 to 1, or `default` | Volume profile: its outline. Unset, or `default`, it follows its rows. Market profile: its outline, solid unless set |
| `--poc-transparency F` | 0 to 1, or `default` | Volume profile, market profile: its point of control. `default` is its own, 0.1 on a volume profile and 0 on a market profile |
| `--highlight-value-area B` | `on`, `off` | Volume profile, market profile: whether its value area is set apart from its other rows; on when not given |
| `--value-area-color C` | A colour, or `none` | Volume profile, market profile: its value area's own colour while it is set apart; `none` gives it back the rows' colour |
| `--up-color C`, `--down-color C` | A colour | Volume |
| `--colouring C` | `default`, or `theme` (Vivid), `theme-soft`, `theme-quiet` for one of its own. `default` is the chart's colouring for volume (`chart` too, as earlier versions spelled it) and the one chosen in Preferences for a volume profile and a cumulative delta | Volume, volume profile, cumulative delta ([Volume](/manual/indicators/volume/)) |
| `--opacity F` | `0.2` to `1`, where `1` is fully opaque | Volume |
| `--overbought F`, `--oversold F` | 0 to 100 | RSI, stochastic |
| `--height F` | 0.05 to 0.95; Volume defaults to 0.18, the other strips to 0.16 | Volume, cumulative delta, RSI, MACD, ATR, stochastic |
| `--bands LIST` | `1,2,3` or any of them; `none` | VWAP; `1` or `none` for Bollinger |
| `--band-alpha F` | 0 to 1; 0.16 when not given | VWAP, Bollinger |
| `--add-range`, `--remove-range`, `--range`, `--name`, `--from`, `--to`, `--zone`, `--shade-alpha` | See [Session Times](#session-times) | Session Times |
| `--color C` | A colour | The line of any indicator; a profile's bars; a Session Times range's shading |
| `--width F` | Pixels; `0` draws no line | Any indicator with a line |
| `--line-style S` | `solid`, `dashed`, `dotted`; `--style S` is its older name | Any indicator with a line |
| `--visible B` | `on`, `off` | Any |
| `--legend B` | `on`, `off`; `on` for a new indicator, `off` for Session Times | Any. Whether the chart's legend names it. It is drawn either way |

A colour is a palette name (`Blue`, `Amber`, `Violet`, `Teal`, `Rose`,
`Green`, `Orange`, `Cyan`, `Neutral`, capitals ignored) or `#rrggbb`. A palette name follows
the theme; a hex does not.

Volume and a volume profile are bars, not lines, so `--width` and `--line-style`
change nothing on them. `--color` on volume colours only its dot in the
legend.

`--bands` chooses which bands are drawn, `--band-alpha` how solid every
band's shading is, and `--deviations` where a Bollinger's bands sit. A band's
colour and edges, and a VWAP band's multiple, are set only in the window.

The CLI accepts wider ranges than the dialog's sliders in places. Standard
deviations run 0.1 to 6 in the dialog and 0 to 50 in `--deviations`; the
value area 10 to 100% there and 0 to 1 in `--value-area`.

Examples:

```sh
omacharts chart indicator add sma --period 200 --color Amber
omacharts chart indicator add ema --period 9
omacharts chart indicator add vwap --bands 1,2 --band-alpha 0.3
omacharts chart indicator add bollinger --deviations 2.5
omacharts chart indicator add volume_profile --poc-color Amber
omacharts chart indicator add volume --height 0.25
omacharts chart indicator set rsi --period 21 --overbought 80
omacharts chart indicator add macd --signal-color Amber
omacharts chart indicator add atr --period 20
omacharts chart indicator add stochastic --k-smooth 5
```

```
$ omacharts chart indicator add sma --period 200
added SMA(200) on pos:0 SPY 1D in "Chartbook 1", in chart template "Default"
  [exit 0]
$ omacharts chart indicator add sma --period 50 --line-style dashed
added SMA(50) on pos:0 SPY 1D in "Chartbook 1", in chart template "Default"
  [exit 0]
$ omacharts chart indicator set sma --period 100
omacharts: this chart's template has 2 of them; say which with --id 3 or 4
  [exit 4]
$ omacharts chart indicator set sma --id 4 --period 100 --color Amber
set SMA(100) on pos:0 SPY 1D in "Chartbook 1", in chart template "Default"
  [exit 0]
```

Refusals:

| Refused | Exit | Message |
|---|---|---|
| A parameter the kind has no use for: `add volume_profile --period 5` | 2 | `VP has no period` |
| A band the kind has not got: `add bollinger --bands 2` | 2 | `BB has 1 band, so 2 is not one of them` |
| `--deviations` on a VWAP: `set vwap --deviations 2` | 2 | `--deviations sets the one band a Bollinger has; VWAP has 3 of them` |
| `--fast` not below `--slow`, in one command or against the length the chart already holds | 2 | `--fast 26 is not below --slow 12` |
| `--oversold` not below `--overbought` | 2 | `oversold 80 is not below overbought 70` |
| A length of 0, or not a whole number | 2 | `--period takes a whole number of bars, one or more` |
| A figure outside its range | 2 | `--height is 0.99, which is outside 0.05 to 0.95` |
| A colour that is not a palette name or a hex | 2 | The message lists the names |
| `set` with nothing to change | 2 | |
| `set` or `remove` of a kind the chart has not got | 3 | `this chart has no RSI` |
| `set` on two of a kind without `--id` | 4 | The message names the ids |

A chart template's indicators take the same actions and flags, with the
template named before the kind: `omacharts template indicator add Momentum
rsi --period 14`. See [Chart templates](/manual/cli/chart-templates/).

## Session Times

`session_times` is [Session Times](/manual/indicators/session-times/), a
list of ranges shaded behind the bars. Ranges are numbered from 1, in the
order `list --json` gives them. These flags change the list, and any other
indicator refuses them.

| Flag | Takes | Does |
|---|---|---|
| `--add-range R` | `eth`, `rth`, `asia`, `london`, `new_york`, `custom` | Adds a range at its preset. The flags that follow in the same command change it |
| `--remove-range N` | A range's number | Removes it, before anything else the command does |
| `--range N` | A range's number | Which range the other flags change |
| `--name TEXT` | Words | The range's name, which the legend lists |
| `--from TIME`, `--to TIME` | A time of day, written any way the editor takes: `9am`, `0930`, `21h30`, `1:30pm` | The range's hours, in the chart's time zone unless it has one of its own. `--to` before `--from` runs past midnight. ETH and RTH have none |
| `--zone ZONE` | An IANA name such as `Europe/London`, or `chart` for the chart's own time zone | The clock the range's hours keep |
| `--color C` | A colour | The range's shading. Every range's, when none is named |
| `--shade-alpha F` | 0.02 to 0.6 | How solid the shading is. Every range's, when none is named |

With one range, the flags change it without `--range`. With more, `--name`,
`--from`, `--to` and `--zone` need `--range`, or come straight after
`--add-range`.

```sh
omacharts chart indicator add session_times
omacharts template indicator set Default session_times --add-range london
omacharts template indicator set Default session_times --add-range custom --name Lunch --from 12pm --to 1:30pm
omacharts template indicator set Default session_times --range 2 --color Blue --shade-alpha 0.1
omacharts template indicator set Default session_times --remove-range 3
omacharts template indicator set Default session_times --legend on
```

```
$ omacharts template indicator list Default
Vol
Sessions · ETH
  [exit 0]
$ omacharts template indicator set Default session_times --add-range london
set Sessions · ETH, London in chart template "Default"
  [exit 0]
$ omacharts template indicator set Default session_times --add-range custom --name Lunch --from 12pm --to 1:30pm
set Sessions · ETH, London, Lunch in chart template "Default"
  [exit 0]
$ omacharts template indicator set Default session_times --range 2 --color Blue --shade-alpha 0.1
set Sessions · ETH, London, Lunch in chart template "Default"
  [exit 0]
```

Refusals:

| Refused | Exit | Message |
|---|---|---|
| These flags on another indicator: `add sma --add-range london` | 2 | `SMA has no ranges` |
| ETH or RTH a second time: `--add-range eth` | 2 | `ETH is already shaded; a second would shade the same bars twice` |
| A range that is not there: `--range 9` | 2 | `` there is no range 9; this has 2 (see `chart indicator list`) `` |
| Hours or a name with several ranges and none named: `--from 9am` | 2 | `this has 2 ranges; say which with --range N` |
| Hours on ETH or RTH: `--range 1 --from 9am --to 10am` | 2 | `ETH follows the instrument's exchange, so it has no hours of its own` |
| A time that is not one: `--from 25:00` | 2 | `--from takes a time of day such as 9am, 09:30 or 2130, not "25:00"` |
| A zone that is not one: `--zone Mars/X` | 2 | `` "Mars/X" is not a time zone; use an IANA name such as Europe/London, or `chart` `` |
