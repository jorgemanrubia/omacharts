---
title: "Market Profile"
description: "Adding a market profile, its flags, and reading, merging, splitting and expanding its profiles."
section: agents
parent: cli/indicators/volume
nav_order: 3
---

How much time the price spent at each price, period by period, in TPO letters ([Market Profile](/manual/indicators/volume/market-profile/)), as `market_profile`.

## Adding it

`omacharts chart indicator add market_profile` adds one and `set market_profile` changes it, with the flags below and the ones [every indicator takes](/manual/cli/indicators/#flags).

```
# A market profile on its own
omacharts chart indicator add market_profile --tpo-minutes 30 --row-ticks auto
```

## Flags

| Flag | Takes | For |
|---|---|---|
| `--anchor P` | `session`, `week`, `month`, `quarter`, `year`; `never` for one running total | VWAP, volume profile, market profile, cumulative delta; `never` only for cumulative delta |
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

## Reading its profiles

`omacharts chart profile list` reads the focused chart's market profiles when it has no volume profile, and `--id N` names a market profile study when it has. `--on-screen`, `--last`, `--rows` and `--json` work as for a [volume profile](/manual/cli/indicators/volume/volume-profile/#reading-its-profiles).

A market profile also reads as the chart draws it. Each profile names its
open, its first bar's opening price, and its close, its last bar's closing
price (the latest one while the session forms), and whether it is drawn
expanded. With `--json` it has `open` and `close`, each a `price` and the
`row` it is in; `expanded`; `tick_size`, `row_ticks` and `row_height`, how
tall a row is; and `tpo_count`, every TPO in it. A row is named by its lowest
tick, and so are its levels: `poc` is the point of control's row,
`value_area_low` the lowest tick of the value area and `value_area_high` its
highest. `--tpos` adds each TPO period, its letter, the times of its first and
last bars, and its high and low, and each row's letters from its lowest
price. With `--json` the periods are `tpo_periods`, each with `letter`,
`start`, `end`, `high` and `low`, and the rows are `rows`, a list of
`[price, count, letters]`.

The listing as a whole says which study it is: `indicator`, its id, `kind`,
`volume_profile` or `market_profile`, and a market profile's `tpo_minutes`.
`paired_with` lists the ids of the profile studies drawn as one pair with it,
and each profile's `paired_with` names the profile of that study it is drawn
beside, by `indicator` and `start`.

```
$ omacharts chart profile list --id 2 --last --tpos
1 profiles on pos:0 SPY 5m in "Chartbook 1"
●  2026-11-18 09:30 → 2026-11-18 15:55  POC 676.40  VA 675.10–677.90  open 676.12 close 677.35  expanded
   A 2026-11-18 09:30 → 2026-11-18 09:55  675.60–677.20
   B 2026-11-18 10:00 → 2026-11-18 10:25  675.05–676.70
   …
   675.00 B
   675.10 AB
   …
  [exit 0]
```

## Merging and splitting

`omacharts chart profile merge --at WHEN` folds the focused chart's profile holding that moment into the one before it, and `omacharts chart profile split --at WHEN` puts a composite back into its periods. `WHEN`
is a day, a day and a time, read in the chart's time zone, or Unix seconds,
such as `start` from `list`. `--id N` says which profile when the chart has more
than one, a volume profile's or a market profile's. A window must be open;
otherwise it exits 6. Beside a [volume profile](/manual/cli/indicators/volume/volume-profile/) of the same period, the two merge and split together ([Composite profiles](/manual/indicators/volume/market-profile/#composite-profiles)).

```
# Merge a session into the one before it, and split it again; a pair does both
omacharts chart profile merge --at 2026-11-18
omacharts chart profile split --at 2026-11-18
```

## Expanded profiles

`omacharts chart profile expand --at WHEN` draws the focused chart's market
profile holding that moment expanded, its TPOs laid out over its bars, and
`omacharts chart profile collapse --at WHEN` stacks it again
([Expanded profiles](/manual/indicators/volume/market-profile/#expanded-profiles)).
Kept with the chart, as a merge is.

```
$ omacharts chart profile expand --at 2026-10-28
expanded the market profile on pos:0 SPY 30m in "Chartbook 1"
  [exit 0]
```

```
# Lay a session's TPOs out over its bars, and stack them again
omacharts chart profile expand --at 2026-11-18
omacharts chart profile collapse --at 2026-11-18

# Keep the session still forming expanded
omacharts chart indicator set market_profile --expand-last on
```

## Beside a volume profile

A market profile is a study of its own, usable alone, and merges and splits
as a volume profile does. With a volume profile study of the same
`--anchor` on the chart, each session's two are drawn inside it, each from
its own `--align` (side by side when alike, back to back when not), and merge
and split together. With different anchors, each keeps to itself
([Beside a volume profile](/manual/indicators/volume/market-profile/#beside-a-volume-profile)).

```
# A market profile beside a volume profile, both per session: drawn side by side
omacharts chart indicator add volume_profile --anchor session --profile-width 0.3
omacharts chart indicator add market_profile --anchor session --profile-width 0.4

# Back to back instead: the letters grow left, the volume profile's rows right
omacharts chart indicator set market_profile --align right
```
