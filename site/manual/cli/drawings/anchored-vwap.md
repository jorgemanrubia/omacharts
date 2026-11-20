---
title: "Anchored VWAP"
description: "The volume-weighted average price from the bar you click"
section: agents
parent: cli/drawings
nav_order: 10
---

`avwap`, or `vwap`, is an anchored VWAP from the bar at `--from` to the last bar, whose price is ignored. It takes no `--text`. Its line, source and bands are set with the flags below, beside the ones [every drawing takes](/manual/cli/drawings/#flags). What it draws, and its properties in the window, are in [Anchored VWAP](/manual/drawings/anchored-vwap/).

| Flag | Takes | Does |
|---|---|---|
| `--source S` | `hlc3`, `hl2`, `ohlc4`, `close` | The price each bar adds to an anchored VWAP; `hlc3` by default |
| `--style S` | `solid`, `dashed`, `dotted` | The older name of `--line-style` on an anchored VWAP |
| `--bands LIST` | `1,2,3` or any of them; `none` | Which of an anchored VWAP's three bands are drawn, by place from the line out; none by default |
| `--band N` | 1 to 3 | The band the `--band-*` flags change. Without it they change all three |
| `--band-deviations F` | 0.1 to 6 | How far out the band sits, in standard deviations. Needs `--band` |
| `--band-shaded B` | `on`, `off` | Whether the band shades the area between its edges |
| `--band-alpha F` | 0 to 1 | How solid that shading is |
| `--band-color C` | A colour; `line` | The band's colour; `line` follows the line's |
| `--band-width F` | 0 to 12 | The thickness of the band's edges; 0 draws none |
| `--band-style S` | `solid`, `dashed`, `dotted` | How the band's edges are drawn |

```sh
omacharts chart drawing add avwap --from 2026-09-08,0 --bands 1,3
```
