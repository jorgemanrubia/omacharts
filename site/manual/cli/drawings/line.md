---
title: "Line"
description: "A line between two anchors"
section: agents
parent: cli/drawings
nav_order: 2
---

`line` is a straight line between `--from` and `--to`. It takes the flags [every drawing takes](/manual/cli/drawings/#flags), and an arrowhead at either end with `--arrow` and `--head` ([Arrow](/manual/cli/drawings/arrow/)). What it draws, and its properties in the window, are in [Line](/manual/drawings/line/).

```sh
omacharts chart drawing add line --from 2026-06-26,190 --to 2026-09-22,209 --config 3 --text "rising lows" --text-at bottom
```
