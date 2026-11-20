---
title: "Showing and hiding the sidebars"
description: "Putting the sidebar and the drawing tools out or away."
section: agents
parent: cli
nav_order: 14
---

The sidebar on the right and the drawing tools on the left, put out or away as their corner buttons and keys do ([The window](/manual/window/)). Without a state each prints `on` or `off`.

| Command | Does |
|---|---|
| `chartbook sidebar [on\|off]` | Shows or hides the open chartbook's sidebar, like <kbd>Ctrl</kbd> + <kbd>Shift</kbd> + <kbd>B</kbd> but leaving the keyboard where it is. Kept with the chartbook. `--book BOOK` for another, applied when it is next opened. With no window open, `--book` is needed (exit `6`). |
| `chart drawing-tools [on\|off]` | Puts the drawing tools out or away, like <kbd>Ctrl</kbd> + <kbd>Shift</kbd> + <kbd>D</kbd>. One setting for the window. |

```
$ omacharts chartbook sidebar off
chartbook "Desk" hides the sidebar
  [exit 0]

$ omacharts chart drawing-tools
off
  [exit 0]
```
