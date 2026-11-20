---
title: "Text"
description: "Words on the chart"
section: agents
parent: cli/drawings
nav_order: 8
---

`text` is words at `--from`. `--text` is the whole of it, with `**bold**`, `*italic*` and `***both***` read as emphasis, and `--text-color`, `--font` and `--font-size` set how it looks ([Flags](/manual/cli/drawings/#flags)). What it draws, and its properties in the window, are in [Text](/manual/drawings/text/).

```sh
omacharts chart drawing add text --from 2026-07-08,180 --text "**the** turn" --config 9
```

Refusals:

| Refused | Exit | Message |
|---|---|---|
| A text drawing with no words, on `add` or `set` | 2 | `` `drawing add text` needs --text: a text drawing with nothing to say draws nothing `` |
