---
name: omacharts
description: >-
  Drive Omacharts, the market-charting app for Omarchy, from the `omacharts`
  command — put symbols on screen, build and rearrange chart layouts, and
  read back what the user is looking at. Use it whenever somebody asks about
  a market or about their own charts: "what's semis doing", "pull up gold on
  the 15 minute", "chart SPY and QQQ side by side", "set me up for the
  open", "a 2x2 of the majors with RSI on each, linked", "put a 200-day
  average on this", "add AMD to my semis list", "what have I got open". Also
  for indicators (moving average, VWAP, volume profile, RSI, ATR),
  resolutions and sessions, bar styles and colours, watchlists and their
  sections, chartbooks, link groups, the instrument search, and the cached
  price data. Also for marking a chart up — "draw the trendline", "box the
  range", "circle that gap", "mark the breakout", "label this high" — which
  is lines, horizontal levels, arrows, zig-zags, boxes, circles and words.
  Use it for any mention of omacharts, and whenever the user speaks about
  charts, watchlists, chartbooks or link groups as things already on their
  screen.
---

# Omacharts

Everything the window can do, `omacharts` can do from a terminal, and a command
takes effect in a window that is already open, straight away.

**The command surface is generated. Read it rather than guessing:**

```
omacharts surface --json
```

Every command, every flag with its type, the values the enumerated ones accept,
the exit codes, and a worked example each — built from the same table as the
parser, so it cannot describe a command that does not exist. Nothing here
repeats it; this is only what it structurally cannot say. (Working inside the
Omacharts repo itself? Read its `AGENTS.md` instead.)

## What the surface cannot tell you

**Commands act on what the user is looking at.** Every `chart` and `chartbook`
verb defaults to the focused chart in the open chartbook, so "put a 200-day
average on this" is one command and no lookup. Name a chart only when the user
means a different one.

**Charts are addressed by position.** `pos:0` is the first in the arrangement.
The window hands out fresh ids whenever it rebuilds one, so an id read a moment
ago can name a different chart. Prefer `pos:N`, which is also how every command
names back what it hit.

**Read that line back.** A mutation reports the chart, symbol, resolution and
chartbook it actually touched — with an implicit target, the only way to catch
it reaching the wrong one. These are not fire-and-forget.

**On exit 6, start the app or name a chart.** Never retry, and never fall back
to the stored arrangement — you cannot tell last week's from what is on screen.

**Prefer a theme swatch to a hex**, so the indicator goes on following the
desktop theme. A hex is for when one exact colour was asked for.

**A feed change is immediate, and a sign-in needs the user.** `config set
provider tos` switches a window that is open to the other feed on the spot
and stores it; `--provider tos` on a running window switches that window
without storing. A feed that charts somebody's brokerage account has to be signed in to
in a browser, by them: `provider login` opens one and waits, so run it only
when the user has asked for it and never as a step inside something else.
`provider status` says whether it is signed in, expired or missing, how the
feed delivers bars — polled on a timer, or streamed as each print arrives —
and, for a feed that streams, what the open window holds a subscription to and
whether anything has ticked. A chart on a streamed feed is never refetched on
a timer; if it looks still, `provider status` is where to look first.

**`watchlist feed --json` is the only command that prints a price**, and only
for the default watchlist. Otherwise "what's X doing" means putting it on screen.

## Workflows

The orders commands go in, which is the part a surface cannot express.

**Watchlists onto another machine.** `watchlist export > file` there, then
`watchlist import file` (or `import - < file` over ssh) here. Import only adds
what is missing, so it is safe to repeat; `--replace` makes each list match the
file exactly.

**A 2×2 of the majors at 15m, RSI on each.** Three splits make four charts out
of one; splitting copies what the chart showed, so each position is set
afterwards:

```
omacharts chart split vertical
omacharts chart split horizontal --chart pos:0
omacharts chart split horizontal --chart pos:2
omacharts chart set --chart pos:0 --symbol SPY --resolution 15m
omacharts chart set --chart pos:1 --symbol QQQ --resolution 15m
omacharts chart set --chart pos:2 --symbol DIA --resolution 15m
omacharts chart set --chart pos:3 --symbol IWM --resolution 15m
omacharts chart indicator add rsi --chart pos:0 --period 14
omacharts chart indicator add rsi --chart pos:1 --period 14
omacharts chart indicator add rsi --chart pos:2 --period 14
omacharts chart indicator add rsi --chart pos:3 --period 14
```

**Do not link four charts showing four different symbols.** A group is charts
that show the *same* instrument, so the four above belong in no group at all.
Link groups are for one symbol across several resolutions — a daily, an hourly
and a 5-minute that move together as you walk a watchlist:

```
omacharts chart split vertical
omacharts chart set --chart pos:0 --symbol SPY --resolution 1D --link 2
omacharts chart set --chart pos:1 --resolution 1h --link 2
```

**A chart put in a group leads it.** What that chart shows becomes the group's
symbol, and the rest of the group follows — including charts in chartbooks that
are not open. So `--link` goes on the chart that is already showing what you
want the group on, which is why `--symbol` and `--link` go together on the first
line above. `--link none` takes a chart out and changes nobody.

**A watchlist drives a group too**, and that is the other half of what groups
are for: picking a symbol in the list moves every chart in the group rather than
the one chart that happens to be focused. From a command this only sets the
group — which row a list is on is the rail's own state — so the list follows
whatever its new group is showing. A group drives at most one watchlist, and a
second is refused rather than moved:

```
omacharts watchlist link Semis 3
omacharts chart set --chart pos:0 --link 3
```

**A chartbook is the whole screen, saved** — an arrangement with its own
watchlist beside it. Build a setup once and switch back to it later:

```
omacharts chartbook create Chips --watchlist Semis --symbol NVDA --switch
omacharts status show --json
```

**Draw on the symbol, not the chart.** "Mark the breakout" is a line or a box
on what the user is looking at; the chart named only says which symbol, and
the drawing is then on every chart of that symbol. Two switches per chart say
how it takes part — `chart set --send-drawings off` keeps what is drawn there
to that chart, `--show-drawings off` keeps it clear of what others drew — and
both are on unless somebody turned one off. Anchors are a moment and a price. A drawing follows one of nine
configurations — `--config N`, shipped as the nine theme presets in order, 1
being the up colour and 2 the down colour — or gets a look of its own from
`--color`, `--width`, `--arrow`, `--head`, `--fill`, `--alpha`, `--border`. Prefer a
configuration or a preset name to a hex, so the drawing keeps following the
desktop theme:

```
omacharts chart drawing add line --from 2026-09-01,180.5 --to 2026-09-19,192 --config 4
omacharts chart drawing add rect --from 2026-09-08,178 --to 2026-09-12,186 --config 2
omacharts chart drawing add ellipse --from 2026-09-08,178 --to 2026-09-12,186 --config 5
omacharts chart drawing list --json
omacharts chart drawing set --id 1 --color ink --arrow end
omacharts chart drawing configs line --json
omacharts chart drawing remove --id 2
```

**Reach for the kind that says the thing.** A support or resistance level is
an `hline`, which is held level by the model rather than by the anchors, so a
price read off it is the price: give it two anchors and whichever carries the
price sets the level for both. An `arrow` is a line that points, and is its
own kind rather than a line with a property — reaching for an arrow and then
setting a line's `--arrow` is two steps to say one thing. A `zigzag` is a run
of swings, and the only kind that is not two anchors: its corners are
`--point`, once each and in order, and its ends are the first and the last of
them.

```
omacharts chart drawing add hline --from 2026-09-01,178.4 --to 2026-09-30,178.4 --config 2
omacharts chart drawing add arrow --from 2026-09-03,172 --to 2026-09-09,184 --config 1
omacharts chart drawing add zigzag --point 2026-09-01,170 --point 2026-09-08,186 --point 2026-09-15,176
omacharts chart drawing set --id 1 --order front
```

**A circle is dragged to any shape.** `ellipse` — `circle` is taken as the
same word — is a box with its outline swapped for a curve: the same two
anchors at opposite corners of the box it is drawn in, the same fill and
edge, never held round. Reach for it over a box when the empty corners
matter.

**Words go on the chart two ways.** `text` is a drawing that is nothing but
what it says, at one anchor — `--from` alone, and `--to` is not read. Any
other kind takes `--text` as a *label on itself*, placed by `--text-at`,
which is `center` by default and takes the edges and the corners:

```
omacharts chart drawing add text --from 2026-09-10,184 --text "gap fills here"
omacharts chart drawing add rect --from 2026-09-08,178 --to 2026-09-12,186 --config 2
omacharts chart drawing set --id 2 --text "supply" --text-at top
omacharts chart drawing set --id 2 --text ""
```

What a drawing says is never part of its configuration — a configuration says
how words are set, never which words — so `--text` leaves a drawing following
the configuration it was on, and the empty string takes the label off again.
How they are set *is*: `--text-color`, `--font` and `--font-size` are part of
the nine, and setting one gives that drawing a look of its own the way
`--color` does. `--font system` is the desktop's own face, which is the
default and what the JSON reports as a null font.

**Emphasis is written the way everybody writes it.** `**bold**`, `*italic*`,
`***both***`, and `\*` for an asterisk that is only an asterisk. It comes back
the same way: a drawing's `text.markup` in the JSON is what `--text` would
take to say it again, where `text.words` is the characters alone and
`text.spans` is where the emphasis falls.

```
omacharts chart drawing add text --from 2026-09-10,184 --text "**gap** fills here"
```
