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

Omacharts works with more than one data feed. Yahoo Finance is the default
and needs nothing set up. The other is thinkorswim, the trading platform of
Charles Schwab, which charts your own account's data and streams it: every
chart on screen is a subscription to the gateway, bars arrive as they print,
and nothing on that path asks for data on a timer. Several charts showing the
same symbol at the same resolution share one subscription, and the last of
them to close is what ends it. `omacharts provider status` says what is being
streamed right now, and whether anything has ticked.

Pick one in Preferences → Market data → Provider, or from a terminal:

```
omacharts config set provider tos
omacharts --provider tos          # this launch only
```

A window that is open switches at once: the charts on screen are simply from
the other feed from then on, painted from its cache where it has one and
fetched where it does not. Nothing is cleared — each feed's bars are kept
apart, so switching back is instant.

thinkorswim needs signing in to, once. The settings panel has a button for
it — Sign in, or Sign out once there is a session to forget, which is also
how it says whether there is one — and so does the command line:

```
omacharts provider login
```

That opens a real Chrome window at thinkorswim, where you sign in yourself —
Omacharts never sees your password or your one-time code, and types nothing
into the page. It needs a Chromium-family browser on the machine (Chromium,
Chrome, Brave or Edge) and a Schwab account with thinkorswim. What it keeps
is the session the browser ended up with, in `~/.config/omacharts/tos.env`
(`TOS_ENV_FILE` moves it), and a browser profile beside it in
`~/.config/omacharts/tos-browser` so that the next sign-in is a trusted
device rather than another round of codes.

It asks for charts and nothing else: the client sends two kinds of request,
chart and login, refuses to send any other, and has no order-entry code at
all — a test fails if one is ever routed. Whichever account you sign in with
is the one it charts; the gateway it connects to has to be one of
thinkorswim's own, checked by address. Sessions expire after a while; when
one does, charts say so and signing in again is the fix, from the settings
panel or from `omacharts provider login`. The session itself is
spelled out by `omacharts provider status`: whether one is saved, which
account and when, the browser a sign-in would open, and the two files above.

It charts US stocks and ETFs, futures (`/ES`), class shares (`BRK.B`) and the
main US indexes. A Taipei or Madrid listing it has no name for at all, and a
chart says so rather than sitting empty — those need Yahoo.

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
