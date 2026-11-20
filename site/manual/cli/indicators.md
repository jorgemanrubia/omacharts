---
title: "Indicators"
description: "Adding, changing and removing a chart's indicators, and the flags every indicator takes."
section: agents
parent: cli
nav_order: 3
moved:
  session-times: /manual/cli/indicators/session-times/
---

`omacharts chart indicator ACTION [KIND]` changes a chart's indicators.
`--book` and `--chart` pick the chart ([Which chart a command acts
on](/manual/cli/#which-chart-a-command-acts-on)). `KIND` is the indicator's name as the
command takes it, such as `sma`, `rsi`, `volume_profile` or `session_times`.
Each indicator's name, what it draws, and every parameter's default and range
are in [Indicators](/manual/indicators/) and the indicator's own page.

The flags of each family, and of the volume profile and the market profile, are on their own pages:

{% include children.html head="Family" what="What it covers" %}

| Action | Does |
|---|---|
| `list` | One line per indicator, with its lengths: `SMA(50)`, `VWAP · week`, `BB(20,2.5)`, and `· selected` after the one selected in the window. With `--json`, each one's id, kind, parameters, colour, line, visibility, `in_legend`, whether the legend names it, and `selected` |
| `add KIND` | Adds one, at its defaults plus whatever flags are given |
| `remove KIND` | Removes every indicator of that kind, or with `--id`, that one |
| `set KIND` | Changes the one of that kind. With two of a kind, `--id N` says which |

## Flags

These work on every indicator. Each family's own are on its page.

| Flag | Takes | For |
|---|---|---|
| `--id N` | An id from `list --json`, or `selected` for the one selected in the window, whose kind can then be left out | `set` and `remove` |
| `--selected` | Nothing | `list`: only the one selected in the window |
| `--color C` | A colour | The line of any indicator; a profile's bars; a Session Times range's shading |
| `--width F` | Pixels; `0` draws no line | Any indicator with a line |
| `--line-style S` | `solid`, `dashed`, `dotted`; `--style S` is its older name | Any indicator with a line |
| `--visible B` | `on`, `off` | Any |
| `--legend B` | `on`, `off`; `on` for a new indicator, `off` for Session Times | Any. Whether the chart's legend names it. It is drawn either way |

A colour is a palette name (`Blue`, `Amber`, `Violet`, `Teal`, `Rose`,
`Green`, `Orange`, `Cyan`, `Neutral`, capitals ignored) or `#rrggbb`. A palette name follows
the theme; a hex does not.

The CLI accepts wider ranges than the dialog's sliders in places. Standard
deviations run 0.1 to 6 in the dialog and 0 to 50 in `--deviations`; the
value area 10 to 100% there and 0 to 1 in `--value-area`.

Examples:

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
| A length of 0, or not a whole number | 2 | `--period takes a whole number of bars, one or more` |
| A figure outside its range | 2 | `--height is 0.99, which is outside 0.05 to 0.95` |
| A colour that is not a palette name or a hex | 2 | The message lists the names |
| `set` with nothing to change | 2 | |
| `set` or `remove` of a kind the chart has not got | 3 | `this chart has no RSI` |
| `set` on two of a kind without `--id` | 4 | The message names the ids |

A chart template's indicators take the same actions and flags, with the
template named before the kind: `omacharts template indicator add Momentum
rsi --period 14`. See [Chart templates](/manual/cli/chart-templates/).

