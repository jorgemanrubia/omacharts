---
title: "Drawings"
description: "Adding, listing, changing and removing the drawings on a chart's symbol, and the flags every drawing takes."
section: agents
parent: cli
nav_order: 7
---

`omacharts chart drawing ACTION [KIND]` changes the drawings on a chart's
symbol. `--book` and `--chart` pick the chart ([Which chart a command acts
on](/manual/cli/#which-chart-a-command-acts-on)). `KIND` is `line`, `hline`, `arrow`,
`zigzag`, `rect`, `ellipse` (or `circle`), `text`, or `profile` (or `vp`: a
volume profile of the span between `--from` and `--to`, whose prices are
ignored; `--color` is its outline and `--fill` its background, and it takes no
`--text`), or `avwap` (or `vwap`: an anchored VWAP from the bar at `--from` to
the last bar, whose price is ignored; it takes no `--text`), or `fib` (or
`fibonacci`, `retracement`: a Fib retracement of the swing from `--from` to
`--to`, with 0 at `--to`; it takes no `--text`). The tools and their
configurations are in [Drawings](/manual/drawings/). Whether a chart shows
other charts' drawings and sends its own is `chart set --show-drawings` and
`--send-drawings`, each `on` or `off`:

```
$ omacharts chart set --send-drawings off
chart 46: sends drawings to other charts off
  [exit 0]
```

Each kind's own flags and examples are on its page:

{% include children.html head="Tool" what="What it draws" %}

| Action | Does |
|---|---|
| `list` | Every drawing the chart shows: its id, kind, anchors, words and look, and `· selected` after those selected in the window. With `--json`, the symbol, whether the chart shows and sends drawings, and each drawing's anchors, corners, configuration, style, scope, order, words with their emphasis, and `selected` |
| `add KIND` | Draws one |
| `set --id N` | Changes one: anchors, look, words, scope or order |
| `remove --id N` | Removes one |
| `clear` | Removes every drawing of the symbol, and the chart's own |
| `configs [KIND]` | The nine configurations of a kind, or of every kind; `(edited)` marks one changed |
| `configure KIND --config N` | Changes configuration N of a kind, with the look flags |
| `reset-configs [KIND]` | Puts a kind's nine back as shipped, or every kind's |

An anchor is `WHEN,PRICE`: a date (`2026-09-01`), a date and time
(`2026-09-01T14:30`) or unix seconds, then the price. A date or a time is
read in the chart's time zone, the one its time axis is labelled in
([Time](/manual/preferences/#time)), and `list` prints them the same way.

## Flags

These work on every kind. Each kind's own are on its page.

| Flag | Takes | Does |
|---|---|---|
| `--id N` | An id from `list`, or `selected` for the one selected in the window | Which drawing, for `set` and `remove`. With more than one selected, `selected` is refused with exit 4 and the ids |
| `--selected` | Nothing | `list`: only what is selected in the window |
| `--from ANCHOR`, `--to ANCHOR` | Anchors | The two ends, or opposite corners. A text drawing and an anchored VWAP take `--from` only |
| `--config N` | 1 to 9 | The configuration it follows; for `configure`, the one to change |
| `--color C` | A colour | A line's colour; a box's or circle's edge |
| `--color-transparency F` | 0 to 1, or `default` | How much of the chart shows through that colour. `default` is the kind's own: solid for a line, 0.65 for an edge, 0.3 for a Fib retracement's levels. Not a volume profile's or a text drawing's |
| `--width F` | 0.5 to 12; 0 on an anchored VWAP | A line's thickness; an edge's; every level of a Fib retracement. 0 leaves an anchored VWAP only its bands |
| `--line-style S` | `solid`, `dashed`, `dotted` | How a line, an arrow, a zig-zag, a box's or circle's edge, a volume profile's outline, or an anchored VWAP's line is drawn |
| `--text WORDS` | Words; `""` | The whole of a text drawing; a label on any other. The empty string removes a label. `**bold**`, `*italic*` and `***both***` are read as emphasis, `\*` as an asterisk |
| `--text-at WHERE` | `center`, `top`, `bottom`, `left`, `right`, `top-left`, `top-right`, `bottom-left`, `bottom-right` | Where a figure's label sits; `center` by default |
| `--text-color C` | A colour | The words' colour |
| `--text-transparency F` | 0 to 1, or `default` for solid | How much of the chart shows through the words |
| `--font FAMILY` | A family; `system` | The words' font. A family this machine lacks falls back to the system's |
| `--font-size F` | 6 to 96 | The words' size in pixels |
| `--scope S` | `shared`, `local` | Whether the symbol's other charts see it. Defaults to what the chart sends |
| `--order WHERE` | `front`, `back` | Brings it to the front or sends it to the back |

A colour is one of the presets `up`, `down`, `blue`, `amber`, `violet`,
`teal`, `orange`, `cyan` and `ink`, which follow the theme, or `#rrggbb`,
which does not. Any of `--color`, `--color-transparency`, `--width`, `--arrow`, `--head`,
`--border`, `--fill`, `--alpha`, `--text-color`, `--text-transparency`, `--font`, `--font-size`,
`--poc-color`, `--profile-style`, `--align`, `--value-area`, `--transparency` and the other `--*-transparency` flags, `--highlight-value-area`, `--value-area-color`, `--poc-width`, `--poc-style`, `--poc`, `--rows`, `--profile-method`, `--colouring`, `--profile-width`, the `--outline*` flags, `--source`, `--style`, `--bands`, the `--band-*` flags, `--levels`, `--fib-labels`, `--price-labels` and the `--level-*` flags gives the drawing a look of its own, off its configuration; `--text` and
`--text-at` do not. A `local` drawing has a negative id. Changing a drawing's
scope gives it a new id.

```
$ omacharts chart drawing add line --from 2026-08-01,170 --to 2026-10-01,190 --config 4
drew #1 line  2026-08-01 170.00 → 2026-10-01 190.00  configuration 4 · shared on pos:0 NVDA 1D in "Chartbook 1"
  [exit 0]
$ omacharts chart drawing add rect --from 2026-09-08,178 --to 2026-09-12,186 --fill down --alpha 0.25
drew #4 rect  2026-09-08 178.00 → 2026-09-12 186.00  fill down at 0.25, edge up 1px · shared on pos:0 NVDA 1D in "Chartbook 1"
  [exit 0]
$ omacharts chart drawing set --id 1 --text supply --text-at top
set #1 line  2026-08-01 170.00 → 2026-10-01 190.00  "supply"  configuration 4 · shared on pos:0 NVDA 1D in "Chartbook 1"
  [exit 0]
$ omacharts chart drawing set --id 1 --color ink --width 2.5 --arrow end --head barb
set #1 line  2026-08-01 170.00 → 2026-10-01 190.00  "supply"  ink, 2.5px, barb arrow at the end · shared on pos:0 NVDA 1D in "Chartbook 1"
  [exit 0]
$ omacharts chart drawing add line --from 2026-08-01,170 --to 2026-10-01,190 --scope local
drew #-1 line  2026-08-01 170.00 → 2026-10-01 190.00  configuration 1 · local on pos:0 NVDA 1D in "Chartbook 1"
  [exit 0]
```

```
$ omacharts chart drawing configure line --config 3 --color teal --arrow end
Line configuration 3 is now teal, 1.5px, arrow at the end
  [exit 0]
$ omacharts chart drawing reset-configs line
restored the default line configurations
  [exit 0]
```

Refusals:

| Refused | Exit | Message |
|---|---|---|
| An anchor that is not `WHEN,PRICE` | 2 | `--from takes WHEN,PRICE — like 2026-09-01,180.5 or 2026-09-01T14:30,180.5` |
| A colour that is not a preset or a hex | 2 | The message lists the presets |
| A figure outside its range | 2 | `--width is 20, which is outside 0.5 to 12` |
| `set` with nothing to set | 2 | `nothing to set: give --from, --to, --point, --config, --scope, --order, --text, or a property` |
| An id that is not there | 3 | `no drawing 99 on NVDA` |
| A chart with no symbol | 5 | `that chart has no symbol to draw on` |
