---
title: "The Omarchy plugin"
description: "Putting the Omarchy plugin in the bar, a key for it, and the feed it reads."
section: agents
parent: cli
nav_order: 22
---

`plugin` puts the Omarchy plugin ([Omarchy plugin](/manual/bar-widget/)) in
the bar and takes it out.

## `plugin`

| Command | Does |
|---|---|
| `omacharts plugin install` | Puts the plugin at the right-hand end of the bar. If it is already there, brings it up to date. |
| `omacharts plugin uninstall` | Takes it out of the bar, wherever it was dragged to, and removes its folder. |
| `omacharts plugin status` | Whether it is in the bar, and the folder it is in or would go in. |

```
$ omacharts plugin status
not in the bar; it would go in /home/you/.config/omarchy/plugins/omacharts.watchlist
  [exit 0]

$ omacharts plugin install
installed in the bar, in /home/you/.config/omarchy/plugins/omacharts.watchlist
  [exit 0]

$ omacharts plugin install
already in the bar; brought it up to date in /home/you/.config/omarchy/plugins/omacharts.watchlist
  [exit 0]

$ omacharts plugin uninstall
taken out of the bar
  [exit 0]

$ omacharts plugin uninstall
not in the bar, so there was nothing to remove
  [exit 0]
```

`uninstall` can be run without checking first. A desktop with no Omarchy bar
is reported, not refused, and all three commands exit `0`:

```
$ omacharts plugin install
no Omarchy bar on this desktop, so there is nowhere to put the widget
  [exit 0]
```

`plugin status --json` gives `id`, `omarchy` (whether there is a bar),
`installed` and `path`.

## A key for the plugin

To open and close the plugin with a key, bind `omarchy-shell
omacharts.watchlist toggle` in Hyprland. `open`, `close` and `refresh`
work in place of `toggle`. The refresh interval is set with `omarchy bar
set`; see [Refreshing](/manual/bar-widget/#refreshing).

## `watchlist feed`

`omacharts watchlist feed` prints the default watchlist with its quotes, as
JSON. It is the only command that prints prices.
`--refresh` fetches fresh quotes first; without it you get the quotes as last
fetched.

```
$ omacharts watchlist feed --refresh
{"sections":[{"name":"Indexes","root":false,"entries":[{"symbol":"SPY","suffix":null,"display":"SPY","name":"SPDR S&P 500 ETF Trust","kind":"ETF","spark":[757.830017, …, 773.169983],"last":773.169983,"change":4.119995,"changePct":0.5357,"asOf":1792589400}, …
  [exit 0]
```

Each entry carries `symbol`, `suffix`, `display`, `name`, `kind`, `spark`
(the last 30 daily closes), `last`, `change`, `changePct` and `asOf`
(seconds since the epoch). A symbol with no quote yet has `null` for the
numbers. Beside the sections come `colors`, the theme's colours (`text`,
`soft`, `up`, `down`, `flat`, `hover` and the `tags` pills), and
`updatedAt`.
