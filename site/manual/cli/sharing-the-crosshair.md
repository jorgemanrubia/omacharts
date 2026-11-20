---
title: "Sharing the crosshair"
description: "Whether the pointer on one chart draws the same moment on the others."
section: agents
parent: cli
nav_order: 13
---

Whether the pointer on one chart draws the same moment on every other chart of the same symbol, whatever their link mode. One setting for every chart, Preferences › Charts › Other › **Sync crosshair across charts** ([Sharing the crosshair](/manual/charts/#sharing-the-crosshair)).

| Command | Does |
|---|---|
| `chart crosshair` | Whether the crosshair is shared; prints `on` or `off`. |
| `chart crosshair on` or `off` | Turns it on or off. |
| `config set sync_crosshair on` or `off` | The same setting, by its key; see [Preferences](/manual/cli/preferences/). |
