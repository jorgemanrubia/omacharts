---
title: "Volume profile"
description: "The volume at each price over a span you choose"
section: agents
parent: cli/drawings
nav_order: 9
---

`profile`, or `vp`, is a volume profile of the span between `--from` and `--to`, whose prices are ignored. `--color` is its outline and `--fill` its background, and it takes no `--text`. Its rows are set with the flags below, beside the ones [every drawing takes](/manual/cli/drawings/#flags). A profile of every period is the [Volume Profile](/manual/cli/indicators/volume/volume-profile/) indicator. What it draws, and its properties in the window, are in [Volume profile](/manual/drawings/volume-profile/).

| Flag | Takes | Does |
|---|---|---|
| `--poc-color C` | A colour | A volume profile's point of control |
| `--profile-style S` | `filled`, `outline` | A volume profile's rows, or only their edge |
| `--align A` | `left`, `right` | The edge of its span a volume profile grows from |
| `--value-area F` | 0.1 to 1 | The share of a volume profile's volume its value area holds |
| `--transparency F` | 0 to 1 | How much of the chart shows through a volume profile's rows and outline: 0 solid, 1 invisible; 0.7 by default |
| `--value-area-transparency F`, `--outside-transparency F` | 0 to 1, or `default` | The same for a volume profile's value area, and the rows outside it, while it is highlighted; `default` follows `--transparency` |
| `--outline-transparency F` | 0 to 1, or `default` | The same for a volume profile's outline; `default` follows `--transparency` |
| `--poc-transparency F` | 0 to 1, or `default` | The same for a volume profile's point of control; `default` is its own, 0.1 |
| `--highlight-value-area B` | `on`, `off` | Whether a volume profile's value area is set apart from its other rows; on by default |
| `--value-area-color C` | A colour, or `none` | A volume profile's value area while it is set apart; `none` for its rows' colour |
| `--poc-width F` | 0.5 to 12 | A volume profile's point of control thickness; `--width` is its outline's |
| `--poc-style S` | `solid`, `dashed`, `dotted` | How a volume profile's point of control is drawn; `--line-style` is its outline's |
| `--poc B` | `on`, `off` | Whether a volume profile draws its point of control; on by default |
| `--rows N` | `auto`, or a number of rows, held between 4 and 800 | How finely a volume profile divides its span; `auto` by default, as the study |
| `--profile-method M` | `total`, `delta` | What a volume profile's rows measure: all the volume, or the ask volume less the bid volume, which needs a feed with volume at price; `total` by default, as the study |
| `--colouring C` | `default`, `theme`, `theme-soft`, `theme-quiet`, `theme-mono` (or `monochrome`) | How a delta volume profile's bought and sold rows are painted; `default` follows Preferences |
| `--profile-width F` | 0.1 to 1 | How much of its span a volume profile's longest row reaches; 1, the whole span, by default |
| `--outline B`, `--outline-color C`, `--outline-width F`, `--outline-style S` | As `--border`, `--color`, `--width`, `--line-style` | The study's names for a volume profile's outline; give one name or the other |

```sh
omacharts chart drawing add profile --from 2026-09-08,0 --to 2026-09-12,0 --profile-style outline --align right
```
