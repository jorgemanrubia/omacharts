---
title: "Volume Profile"
description: "Adding a volume profile, its flags, and reading, merging and splitting its profiles."
section: agents
parent: cli/indicators/volume
nav_order: 2
---

How much volume traded at each price, period by period ([Volume Profile](/manual/indicators/volume/volume-profile/)), as `volume_profile`.

## Adding it

`omacharts chart indicator add volume_profile` adds one and `set volume_profile` changes it, with the flags below and the ones [every indicator takes](/manual/cli/indicators/#flags).

```sh
omacharts chart indicator add volume_profile --anchor session --profile-width 0.3
omacharts chart indicator add volume_profile --poc-color Amber
```

## Flags

| Flag | Takes | For |
|---|---|---|
| `--anchor P` | `session`, `week`, `month`, `quarter`, `year`; `never` for one running total | VWAP, volume profile, market profile, cumulative delta; `never` only for cumulative delta |
| `--rows N` | `auto`, or a number of rows per period, held between 4 and 800 | Volume profile |
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
| `--colouring C` | `default`, or `theme` (Vivid), `theme-soft`, `theme-quiet` for one of its own. `default` is the chart's colouring for volume (`chart` too, as earlier versions spelled it) and the one chosen in Preferences for a volume profile and a cumulative delta | Volume, volume profile, cumulative delta ([Volume](/manual/indicators/volume/)) |

## Reading its profiles

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
its `--rows` ([Flags](#flags)).

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

## Merging and splitting

`omacharts chart profile merge --at WHEN` folds the focused chart's profile holding that moment into the one before it, and `omacharts chart profile split --at WHEN` puts a composite back into its periods. `WHEN`
is a day, a day and a time, read in the chart's time zone, or Unix seconds,
such as `start` from `list`. `--id N` says which profile when the chart has more
than one, a volume profile's or a market profile's. A window must be open;
otherwise it exits 6. Beside a [market profile](/manual/cli/indicators/volume/market-profile/#beside-a-volume-profile) of the same period, the two merge and split together ([Composite profiles](/manual/indicators/volume/volume-profile/#composite-profiles)).

```
$ omacharts chart profile merge --at 2026-10-28
merged the profile into the one before it on pos:0 SPY 1h in "Chartbook 1"
  [exit 0]
```

## Beside a market profile

With a market profile study of the same `--anchor` on the chart, each session's two are drawn as a pair, each from its own `--align` ([Beside a volume profile](/manual/cli/indicators/volume/market-profile/#beside-a-volume-profile)).

## A profile of a span

A volume profile of a span you choose, rather than of each period, is a drawing: [Volume profile](/manual/cli/drawings/volume-profile/).
