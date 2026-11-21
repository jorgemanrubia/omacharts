---
title: "Circle"
description: "An ellipse inside a box between two corners"
section: agents
parent: cli/drawings
nav_order: 8
---

`ellipse`, or `circle`, fills the box between `--from` and `--to`. It takes the edge and fill flags of a [rectangle](/manual/cli/drawings/rectangle/), `--border`, `--select-by-border`, `--fill` and `--alpha`, beside the ones [every drawing takes](/manual/cli/drawings/#flags). What it draws, and its properties in the window, are in [Circle](/manual/drawings/circle/).

```sh
omacharts chart drawing add ellipse --from 2026-09-29,233 --to 2026-10-12,247 --config 2
```
