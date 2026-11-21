---
title: "Horizontal line"
description: "A level across the chart at one price"
section: agents
parent: cli/drawings
nav_order: 4
---

`hline` is a level at one price, placed with `--from` and `--to` as a line is. It takes the flags [every drawing takes](/manual/cli/drawings/#flags). What it draws, and its properties in the window, are in [Horizontal line](/manual/drawings/horizontal-line/).

```sh
omacharts chart drawing add hline --from 2026-05-14,236 --to 2026-10-20,236 --config 4 --text "236 resistance" --text-at top-left
```
