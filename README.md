# Omacharts

**[omacharts.org](https://omacharts.org)**

[![CI](https://github.com/jorgemanrubia/omacharts/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/jorgemanrubia/omacharts/actions/workflows/ci.yml?query=branch%3Amain)

Fast, beautiful charting software for [Omarchy](https://omarchy.org).

<p align="center">
  <a href="https://www.youtube.com/watch?v=uetKLwfoUrM">
    <img src="assets/examples/video-poster.jpg" width="100%" alt="Watch Omacharts on YouTube">
  </a>
</p>

<p align="center">
  <img src="assets/examples/banner.jpg" width="100%" alt="Four chart layouts, each under a different Omarchy theme">
</p>

## Features

- **Fast.** Instant load, rendering and interactions. The pillar for everything
  else.
- **Beautiful.** The charts are the protagonists and the user interface is at
  their service. It follows your Omarchy theme as you change it, and generates
  an indicator palette for whichever theme is active, so things look great
  without you having to be an artist.
- **Configurable layout.** Split a chart horizontally or vertically, as deep as
  you like, and resize the panes with the mouse or the keyboard. Each chart
  keeps its own symbol, resolution, indicators and settings. Keep as many
  arrangements as you want as chartbooks, each with its own watchlist, and
  switch between them from the strip along the bottom. Link charts and
  watchlists as you need to. It all comes back the way you left it, and
  `omacharts watchlist export` and `import` carry your watchlists to your
  other machines.
- **Keyboard first.** An intuitive, discoverable user interface, prepared for
  power users. Hotkeys for the whole app: split and close charts, resize them,
  walk the chartbooks, step the resolution, rotate the watchlists. Type a
  letter to find a symbol, a number to set a resolution. Press `?` to learn it all.
- **Indicators.** Moving averages, VWAP with bands, Bollinger bands, volume,
  volume profile, RSI, MACD, ATR and stochastic, each in its own resizable
  strip. Save a set of them as a chart template and put charts on it: change
  the template and every chart on it follows. More coming.
- **Drawings.** Trendlines, levels, arrows, zig-zags, boxes, circles and text,
  a keystroke each. Their colours are roles your theme fills, held clear of
  the candles, so a drawing reads on any theme. Figures take labels, and what
  you draw is on every chart of that symbol.
  [See how it works](https://www.youtube.com/watch?v=wwMnxI0watk).
- **Omarchy plugin.** Your watchlist in the bar, with sparklines, live, still
  there after the window closes. It installs itself the first time you run
  Omacharts on an Omarchy desktop, and `omacharts plugin` puts it back, brings
  it up to date or takes it out again.
- **Agent ready.** A rich CLI covers everything the window does, and says what
  it can do in a form a script or an agent can read.

## Installing

Omacharts lives in Omarchy's own package repository, which is already enabled
on an Omarchy machine. Open the Omarchy menu with <kbd>Super</kbd> +
<kbd>Space</kbd>, choose **Install → Package**, and pick **omacharts**.

Or from a terminal:

```sh
omarchy pkg add omacharts
```

Either way, that puts the command on your path with the man page, shell completions and
the agent skill, and `pacman -Syu` carries Omacharts along with the rest of
the system.

### The latest release

Omarchy's repository can be a release or two behind. To install the newest
release straight from GitHub, on x86_64 or aarch64, run the same line every
time — it always fetches the latest:

```sh
curl -fLo /tmp/omacharts.pkg.tar.zst \
  https://github.com/jorgemanrubia/omacharts/releases/latest/download/omacharts-$(uname -m).pkg.tar.zst \
  && sudo pacman -U /tmp/omacharts.pkg.tar.zst
```

### From source

On Arch, x86_64 or aarch64, you can build the same package from a clone:

```sh
git clone https://github.com/jorgemanrubia/omacharts
cd omacharts/packaging/aur
makepkg -si
```

To run it from a working tree instead — it rebuilds on every launch — use
`./bin/install`.

## Uninstalling

Open the Omarchy menu with <kbd>Super</kbd> + <kbd>Space</kbd>, choose
**Remove → Package**, and pick **omacharts**.

Or from a terminal:

```sh
omarchy pkg drop omacharts
```

Removing the package hides the bar widget. To uninstall the widget too, run
this before removing the package:

```sh
omacharts plugin uninstall
```

Your data stays in `~/.local/share/omacharts`; delete it for a clean slate. Everything you set up — settings, chartbooks, watchlists, drawings, themes — is in `omacharts.db` there, the file to copy to take it to another machine. Downloaded prices are in `market.db` beside it. Copies of `omacharts.db` are kept in `backups/` there — taken automatically, one every two hours for the last day and one a day for the week before, twenty at most — and any of them can be put back from Preferences › Data or with `omacharts backup restore`.

## Data

Omacharts uses Yahoo Finance by default. It needs no setup.

### thinkorswim

If you have a Charles Schwab account with thinkorswim, Omacharts can stream
live charts from it instead. Choose it in Preferences → Market data → Provider
and sign in from there, or from a terminal:

```
omacharts config set provider tos
omacharts provider login
```

Signing in opens Chrome, Chromium, Brave or Edge at thinkorswim, and you sign
in there yourself. Omacharts only asks for chart data and never places orders.

## From a terminal

Everything the window can do, `omacharts` can do from a command line, and a
command takes effect in a window that is already open, straight away.

```
omacharts watchlist create Semis
omacharts watchlist add Semis NVDA AMD AVGO TSM MU
```

See [doc/cli.md](doc/cli.md), or `omacharts surface --json` for the whole
command surface in a form a script can read.

## From an agent

Any agent can drive this app as well as a person can: `omacharts surface --json`
describes every command and argument in a form meant to be parsed.

What is left is discovery, so Omacharts ships a skill that makes an agent reach
for it when you say "what's semis doing" or "set me up for the open". It
installs for whichever agents you have:

```sh
omacharts skill install
```

See `omacharts skill --help`, or
[doc/cli.md](doc/cli.md#teaching-an-agent-about-this-app).

## Contributing

Pull requests are welcome.

## Licence

MIT. See [LICENSE](LICENSE).
