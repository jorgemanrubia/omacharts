# Omacharts

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
  watchlists as you need to. It all comes back the way you left it.
- **Keyboard first.** An intuitive, discoverable user interface, prepared for
  power users. Hotkeys for the whole app: split and close charts, resize them,
  walk the chartbooks, step the resolution, rotate the watchlists. Type a
  letter to find a symbol, a number to set a resolution. Press `?` to learn it all.
- **Indicators.** Moving averages, VWAP with bands, volume, volume profile,
  RSI and ATR, each in its own resizable strip. More coming.
- **Omarchy plugin.** Your watchlist in the bar, with sparklines, live, still
  there after the window closes. It installs itself the first time you run
  Omacharts on an Omarchy desktop, and `omacharts plugin` puts it back, brings
  it up to date or takes it out again.
- **Agent ready.** A rich CLI covers everything the window does, and says what
  it can do in a form a script or an agent can read.

## Installing

On Arch, install the package attached to the latest
[release](https://github.com/jorgemanrubia/omacharts/releases/latest):

```sh
curl -LO https://github.com/jorgemanrubia/omacharts/releases/latest/download/omacharts-0.1.7-1-x86_64.pkg.tar.zst
sudo pacman -U omacharts-0.1.7-1-x86_64.pkg.tar.zst
```

Or build that same package yourself from a clone:

```sh
git clone https://github.com/jorgemanrubia/omacharts
cd omacharts/packaging/aur
makepkg -si
```

Either one gets you the command on your path, the man page, shell
completions and the agent skill. To run it from a working tree instead — it
rebuilds on every launch — use `./bin/install`.

### Through Omarchy

Omacharts is for Omarchy, so Omarchy's own package repository is where it
belongs, and it is
[waiting to be merged there](https://github.com/omacom/omarchy-pkgs/pull/802).
Once it lands, this is the whole of it:

```sh
omarchy pkg add omacharts
```

Nothing to configure, since that repository is already enabled on an Omarchy
machine, and `pacman -Syu` will carry Omacharts along with the rest of the
system.

## Data

Omacharts is prepared to work with multiple data providers. Yahoo Finance is
the default. The other is Thinkorswim, Charles Schwab's trading platform,
from a major US brokerage:

```
omacharts config set provider tos
OMACHARTS_PROVIDER=tos omacharts
```

`OMACHARTS_PROVIDER` wins over the stored setting. The choice is read when
the process starts.

The first chart opens a browser at thinkorswim. Sign in there, and leave the
window on paperMoney: a live gateway is refused. The connector reads the
session from that browser and saves it. Later runs reuse the file.
`TOS_ENV_FILE` chooses the file; otherwise it is `~/.config/omacharts/tos.env`.
The feed reads charts only.

The symbol search covers every US-listed stock and ETF, and every listing on
the two Taiwanese exchanges — the TWSE (`2330.TW`) and the TPEx (`6488.TWO`) —
searchable by ticker, English name or Chinese name (`台積電`), and charted on
Taipei's own trading hours.

We are interested in adding more feeds, both free and paid. If you want to see
yours supported, please create a Pull Request.

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

## Roadmap

Rough order, and nothing here is a promise.

- [ ] **Beautiful annotations.** Trendlines, levels and notes that stay where
      you put them, and look like they belong on the chart rather than on top
      of it.
- [ ] **More data feeds.** Yahoo is one provider behind one interface. Others
      can sit behind the same one, including the paid ones with real-time
      prices.
- [ ] **More indicators.** MACD and Bollinger bands are the obvious gaps.
- [ ] ...

Pull requests are welcome.

## Licence

MIT. See [LICENSE](LICENSE).
