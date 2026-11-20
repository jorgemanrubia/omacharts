---
title: "Chart templates"
description: "Making chart templates, filling them with indicators, and putting charts on them."
section: agents
parent: cli
nav_order: 4
---

A chart template is a named set of indicators that charts share. Change the
template and every chart on it changes. How a chart follows a template is in
[Chart templates](/manual/chart-templates/).

| Command | Does |
|---|---|
| `omacharts template list` | Every template: its id, name, how many charts use it, and what it holds. `*` marks the one new charts open with |
| `omacharts template create NAME` | Makes an empty template |
| `omacharts template create NAME --from-chart` | Makes a template holding the focused chart's indicators, and puts the chart on it. `--book` and `--chart` name another chart. Refused on a chart with no indicators |
| `omacharts template rename TEMPLATE NAME` | Renames one; its charts stay on it |
| `omacharts template delete TEMPLATE` | Deletes one; its charts keep its indicators as their own |
| `omacharts template indicator ACTION TEMPLATE [KIND]` | `list`, `add`, `remove` or `set` the template's indicators, with every flag `chart indicator` takes ([Indicators](/manual/cli/indicators/)). Every chart on it follows |
| `omacharts template default [TEMPLATE]` | Reads, or sets, the template new charts open with; `none` for none |
| `omacharts chart set --template TEMPLATE` | Puts a chart on a template, or on `none` |
| `omacharts config set chart_template TEMPLATE` | The same setting `template default` writes: a name, `id:N`, or `none` |

`TEMPLATE` is a name or `id:N` ([Naming things](/manual/cli/#naming-things)); two
templates may share a name. `chart set --template` replaces the chart's
indicators without asking.

```
$ omacharts template create Swing
created chart template "Swing" (id:3)
  [exit 0]
$ omacharts template indicator add Swing rsi --period 14
added RSI(14) in chart template "Swing"
  [exit 0]
$ omacharts template indicator add Swing macd --signal-color Amber
added MACD(12,26,9) in chart template "Swing"
  [exit 0]
$ omacharts chart set --template Swing
chart 33: chart template "Swing"
  [exit 0]
$ omacharts template list
* id:1  Default              0 charts  Volume
  id:2  Momentum             0 charts  RSI 14, MACD 12 26 9
  id:3  Swing                1 chart   RSI 14, MACD 12 26 9
  [exit 0]
$ omacharts template indicator set Swing rsi --overbought 80
set RSI(14) in chart template "Swing", and on the chart using it
  [exit 0]
$ omacharts template default Swing
new charts open with chart template "Swing"
  [exit 0]
```

On a chart that is on a template, adding, removing, hiding, reordering or
reconfiguring an indicator changes the template, and every chart on it, the
same as the window does. The reply names the template and how many other
charts followed:

```
$ omacharts chart indicator add sma --period 200
added SMA(200) on pos:0 SPY 1D in "Chartbook 1", in chart template "Default", and on the other chart using it
  [exit 0]
```

`template delete` on a name two templates share exits `4`. Deleting the
default template leaves its charts their indicators, and new charts open with
no template:

```
$ omacharts template create Swing
created chart template "Swing" (id:4)
  [exit 0]
$ omacharts template delete Swing
omacharts: 2 chart templates are called "Swing"; use id:3 or id:4
  [exit 4]
$ omacharts template delete id:4
deleted chart template "Swing"
  [exit 0]
$ omacharts template delete Swing
deleted chart template "Swing"; the chart using it keeps its indicators as its own
  [exit 0]
$ omacharts template default
new charts open with no chart template
  [exit 0]
$ omacharts chart set --template none
chart 37: no chart template, and no indicators
  [exit 0]
$ omacharts chart set --template Nope
omacharts: no chart template called "Nope"; see `omacharts template list`
  [exit 3]
```

`config get chart_template` prints the template's id, or `none`. `template
default` with no argument says it by name.

`template create --from-chart` keeps what a chart shows as a new template and
puts the chart on it. A chart with no indicators is refused:

```
$ omacharts template create Trend --from-chart
omacharts: pos:0 SPY 1D has no indicators to make a template from; `omacharts template create Trend` makes an empty one
  [exit 5]
$ omacharts chart indicator add rsi
added RSI(14) on pos:0 SPY 1D in "Chartbook 1"
  [exit 0]
$ omacharts template create Trend --from-chart
created chart template "Trend" (id:5) from the indicator on pos:0 SPY 1D in "Chartbook 1", and put the chart on it
  [exit 0]
```
