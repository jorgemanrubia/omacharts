---
title: "Arrow"
description: "A line with a head at the end"
section: agents
parent: cli/drawings
nav_order: 4
---

`arrow` is a line with a head at the end you give last, `--to`. `--arrow` and `--head` set the heads of an arrow and of a line, beside the flags [every drawing takes](/manual/cli/drawings/#flags). What it draws, and its properties in the window, are in [Arrow](/manual/drawings/arrow/).

| Flag | Takes | Does |
|---|---|---|
| `--arrow WHERE` | `none`, `end`, `start`, `both` | Which ends have an arrowhead |
| `--head SHAPE` | `filled`, `open`, `barb` | The arrowhead |

```sh
omacharts chart drawing add arrow --from 2026-07-17,182 --to 2026-07-30,195 --config 9
omacharts chart drawing set --id 1 --color ink --width 2.5 --arrow end --head barb
```
