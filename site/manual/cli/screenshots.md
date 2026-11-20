---
title: "Screenshots"
description: "Capturing the focused chart, or the open chartbook, to a file."
section: agents
parent: cli
nav_order: 16
---

`chart screenshot` captures the focused chart of the open chartbook, and
`chartbook screenshot` the open chartbook. What a picture holds, the key and
the Preferences rows are in [Screenshots](/manual/screenshots/). Unlike the
key, the commands always write a file, whatever **Also save a PNG** says, and
copy to the clipboard only with `--clipboard`.

```
$ omacharts chart screenshot
saved pos:0 NVDA 1D in "Desk" to /home/you/Pictures/Omacharts/NVDA-1D-20261021-221626.png
  [exit 0]

$ omacharts chartbook screenshot --clipboard --output /tmp/desk.png
saved chartbook "Desk", 3 charts to /tmp/desk.png, and copied it
  [exit 0]
```

| Flag | Takes |
|---|---|
| `--output PATH` | The file to write. A folder means the usual name, in that folder. Without it, the file goes to the folder in **Save to** |
| `--clipboard` | Also copies the picture, as the window's own key does |
| `--json` | The result as JSON: what was captured, the path, and whether it was copied |

With no window open both exit 6. Neither takes `--book` or `--chart`; a
chart that is not on screen cannot be captured. Focus the chart first with
`chart focus`, or open the chartbook with `chartbook switch`.

`screenshot_save_file` does not affect the commands; `screenshot_folder` is
where they write when `--output` is not given. Both are in
[Settings](/manual/cli/preferences/#settings).
