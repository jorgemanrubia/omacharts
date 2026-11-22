---
title: "Rectangle"
description: "A box between two corners"
section: agents
parent: cli/drawings
nav_order: 7
---

`rect` is a box with `--from` and `--to` at opposite corners. Its edge and fill are set with the flags below, beside the ones [every drawing takes](/manual/cli/drawings/#flags). What it draws, and its properties in the window, are in [Rectangle](/manual/drawings/rectangle/).

| Flag | Takes | Does |
|---|---|---|
| `--border B` | `on`, `off` | Whether a box or circle has an edge |
| `--select-by-border B` | `on`, `off` | Whether a box or circle is selected by its edge alone, leaving a click inside it to the chart; off by default |
| `--fill C` | A colour | A box's or circle's fill |
| `--alpha F` | 0 to 1 | How much of the fill shows: 0 none, 1 solid |

```sh
omacharts chart drawing add rect --from 2026-09-01,207 --to 2026-09-24,232 --config 6 --text range --text-at top
omacharts chart drawing add rect --from 2026-09-08,178 --to 2026-09-12,186 --fill down --alpha 0.25
```
