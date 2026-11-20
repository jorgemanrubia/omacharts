---
title: "Volume and market profiles"
description: "Reading a chart's volume and market profiles, and merging, splitting and expanding them."
section: agents
parent: cli
nav_order: 7
---

`omacharts chart profile list` reads the focused chart's volume profiles, or
its market profiles when it has no volume profile: where
each begins and ends, its point of control and its value area, and whether it
is on screen (`●`) or not (`○`). `--on-screen` keeps only the ones on screen.
With `--json`, each profile has `start` and `end` (Unix seconds, with
`start_text` and `end_text` in the chart's clock), `periods` (the start of each
period in it), `composite`, `poc`, `value_area_low`, `value_area_high`,
`volume` and `visible`. `--last` keeps only the newest profile.

`--rows` adds each profile's rows, from its lowest price, as a `price volume`
table under it. A row is named by its low, which is the traded price when a
row is one tick. With `--json` they are `rows`, a list of `[price, volume]`
pairs, and `row_height`. There are as many rows as the study draws, set by
its `--rows` ([Indicators](/manual/cli/indicators/)).

`--nodes` adds each profile's high-volume nodes, the prices volume piled up
at, and its low-volume nodes, the thinnest price between two neighbouring
high-volume nodes. Each is given by the price at its centre, the prices it
spans in brackets, and its strength: how far it stands above or sinks below
the volume around it, as a share of the point of control's volume. A profile
with even volume throughout has none. With `--json` they are `hvn` and `lvn`,
lowest price first, each with `price`, `low`, `high`, `volume` and
`strength`.

The nodes are found the same way every time. Each row's volume is first
averaged with the rows within a fortieth of the profile's height either side
(at least one row), so a single busy row is not a node. A high-volume node is
a peak of those averages that stands at least a tenth of the point of
control's volume above the lowest volume between it and any higher peak, on
each side. A low-volume node is the lowest point between two neighbouring
high-volume nodes. A node spans the rows within half its strength of its
centre.

```
$ omacharts chart profile list --last --nodes
1 profiles on pos:0 ESZ6 5m in "Chartbook 1"
●  2026-11-15 18:10 → 2026-11-16 02:50  POC 7579.70  VA 7556.20–7614.40
   hvn 7550.6 (7548.2–7553.8) 0.38 · 7566 (7564–7568.4) 0.52 · 7578.8 (7573.4–7583) 0.78 · 7599.8 (7597.2–7603) 0.52 · 7610.8 (7609–7613) 0.21
   lvn 7560.2 (7555.6–7563.4) 0.78 · 7571 (7568.4–7573.8) 0.52 · 7591.6 (7583.8–7596.8) 0.57 · 7607 (7604.2–7609) 0.21
  [exit 0]
```

```
$ omacharts chart profile list --on-screen
21 profiles on pos:0 SPY 1h in "Chartbook 1"
●  2026-10-02 09:30 → 2026-10-02 15:30  POC 769.35  VA 768.30–770.40
●  2026-10-05 09:30 → 2026-10-05 15:30  POC 775.25  VA 772.80–776.60
…
●  2026-10-27 09:30 → 2026-10-27 15:30  POC 769.75  VA 768.10–771.20
●  2026-10-28 09:30 → 2026-10-29 15:30  POC 768.15  VA 767.60–771.20  composite of 2
●  2026-10-30 09:30 → 2026-10-30 15:30  POC 757.95  VA 753.10–760.40
●  2026-11-02 09:30 → 2026-11-02 16:00  POC 754.15  VA 751.60–759.90
  [exit 0]
```

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

`omacharts chart profile merge --at WHEN` folds the focused chart's volume
profile holding that moment into the one before it, and `omacharts chart
profile split --at WHEN` puts a composite back into its periods
([Composite profiles](/manual/indicators/volume/volume-profile/#composite-profiles)).
A market profile is merged and split the same way, on its own or, beside a
volume profile of the same period, together with it
([Composite profiles](/manual/indicators/volume/market-profile/#composite-profiles)). `WHEN`
is a day, a day and a time, read in the chart's time zone, or Unix seconds,
such as `start` from `list`. `--id N` says which profile when the chart has more
than one, a market profile's id included. A window must be open;
otherwise it exits 6.

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
$ omacharts chart profile merge --at 2026-10-28
merged the profile into the one before it on pos:0 SPY 1h in "Chartbook 1"
  [exit 0]
```

A market profile is a study of its own, usable alone, and merges and splits
as a volume profile does. With a volume profile study of the same
`--anchor` on the chart, each session's two are drawn inside it, each from
its own `--align` (side by side when alike, back to back when not), and merge
and split together. With different anchors, each keeps to itself
([Beside a volume profile](/manual/indicators/volume/market-profile/#beside-a-volume-profile)).

```
# A market profile on its own
omacharts chart indicator add market_profile --tpo-minutes 30 --row-ticks auto

# A market profile beside a volume profile, both per session: drawn side by side
omacharts chart indicator add volume_profile --anchor session --profile-width 0.3
omacharts chart indicator add market_profile --anchor session --profile-width 0.4

# Back to back instead: the letters grow left, the volume profile's rows right
omacharts chart indicator set market_profile --align right

# Merge a session into the one before it, and split it again; a pair does both
omacharts chart profile merge --at 2026-11-18
omacharts chart profile split --at 2026-11-18

# Lay a session's TPOs out over its bars, and stack them again
omacharts chart profile expand --at 2026-11-18
omacharts chart profile collapse --at 2026-11-18

# Keep the session still forming expanded
omacharts chart indicator set market_profile --expand-last on
```
