---
title: "Zig-zag"
description: "A line through several corners"
section: agents
parent: cli/drawings
nav_order: 5
---

`zigzag` is a line through several corners, each a `--point` in order. It takes the flags [every drawing takes](/manual/cli/drawings/#flags). What it draws, and its properties in the window, are in [Zig-zag](/manual/drawings/zig-zag/).

| Flag | Takes | Does |
|---|---|---|
| `--point ANCHOR` | An anchor, repeated | A zig-zag's corners in order, two at least. `--from` and `--to` alone give it one leg |

```sh
omacharts chart drawing add zigzag --point 2026-04-06,165 --point 2026-05-14,236 --point 2026-06-26,190 --point 2026-08-14,228 --point 2026-08-28,207 --config 5
```

Refusals:

| Refused | Exit | Message |
|---|---|---|
| `--point` on anything but a zig-zag | 2 | `--point is a zig-zag's corners; every other kind is --from and --to` |
| A zig-zag with one `--point` | 2 | `a zig-zag needs at least two --point corners` |
