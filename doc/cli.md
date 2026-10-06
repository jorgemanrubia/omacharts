# Driving Omacharts from a terminal

Everything the window can do can be done from here. This page is written for
somebody — or something — with no other knowledge of the app.

If you are a program rather than a person, read this instead and skip the
prose:

```
omacharts surface --json
```

That emits every command, every argument and flag with its type, the values
the enumerated ones accept, the exit codes, and a worked example per command.
It is generated from the same table the parser is built from, so it cannot
describe a command that does not exist.

## Where commands run

- **With the app open**, a command is handed to the running window and takes
  effect immediately — a watchlist you create in a terminal appears in the
  rail without a restart. The output and the exit status come back to your
  terminal as if it had run locally.
- **With nothing running**, the same command runs against the stored database.
  No GTK, no display, no window. This works over ssh.

You do not choose between them and there is no flag for it.

`skill` is the exception. It runs in the process you typed it in whether or not
the app is open, because the answer is about your machine and not the window's:
the agent variables it reads are the ones you were given, and a relative
`skill install --to DIR` resolves from the directory you are standing in.
`skill status` and `skill uninstall` are the same.

## Exit codes

An exit code is the only thing a script can rely on without parsing text.

| Code | Meaning |
|------|---------|
| 0 | the command did what it says |
| 1 | something went wrong that none of the others describe |
| 2 | the command was not spelled in a way the parser accepts |
| 3 | what was named does not exist |
| 4 | the name fits more than one thing; say which with `id:N` |
| 5 | understood, and refused |
| 6 | the command meant the chart you are looking at, and no window is open |
| 7 | the command hit a bug in Omacharts and did not finish |

Data goes to stdout, errors to stderr, always.

Code 7 is the one that is not about the command. A command runs inside the
window when one is open, so a bug in it is caught and reported rather than
allowed to take the window down with it — nothing else was affected, and
running the same command again will do the same thing. It is worth reporting.

## Naming things

Watchlists, sections and chartbooks are named by name, case-insensitively. If
two share a name, the command fails with code 4 and tells you the ids; say
which you meant with `id:N`:

```
omacharts watchlist show id:3
```

## Worked example: fine-tuning the chart in front of you

This is what the whole thing is for. Somebody is looking at a chart and says
"put a 200-day average on it". No identifiers, no lookups:

```
$ omacharts chart indicator add sma --period 200 --color Amber --style dashed
added SMA(200) on pos:0 AAPL 1D in "Macro"
  [exit 0]
```

**Every chart and chartbook command defaults to the chart you are looking at**,
in the chartbook that is open. Naming one explicitly still works and overrides
the default. The output always names what it actually hit — the symbol, the
resolution and the chartbook — so you can confirm it reached the right chart
without asking a second question.

That default only exists while a window does. With nothing running there is no
"the one I am looking at", and the command says so rather than quietly acting
on whatever was stored last:

```
$ omacharts chart indicator add rsi
omacharts: no window is open, so there is no focused chart to act on; name one, or start Omacharts
  [exit 6]
```

To find out what is open before changing it:

```
$ omacharts status show
chartbook  Macro (id:0)
* pos:0  AAPL       1D    unlinked
```

Everything `status` prints is spelled the way the other commands accept it, so
it composes without translation. **Prefer `pos:N` over a raw chart id**: the
window hands out fresh ids whenever it rebuilds an arrangement, so an id read
a moment ago can name a different chart. A position names the same place on
the screen either way.

## Indicators and their parameters

Every indicator the app has, with every parameter it exposes:

| Indicator | Parameters |
|---|---|
| `sma`, `ema` | `--period` |
| `rsi` | `--period`, `--overbought`, `--oversold`, `--height` |
| `atr` | `--period`, `--height` |
| `volume` | `--height` |
| `vwap` | `--anchor`, `--bands`, `--band-alpha` |
| `volume_profile` | `--anchor`, `--rows` (a number or `auto`), `--value-area`, `--poc-color` |

Anything that draws a line also takes `--color`, `--width` and `--style`, and
everything takes `--visible`.

Colours come in two kinds, and the difference matters:

- **A palette name** — Blue, Amber, Violet, Teal, Rose, Green, Orange, Cyan —
  is re-resolved every time the desktop theme changes, so the indicator follows
  the theme.
- **`#rrggbb`** is the exact colour you picked, and is never touched again.

```
omacharts chart indicator add vwap --anchor month --bands 1,2 --band-alpha 0.3
omacharts chart indicator add volume_profile --rows auto --color Violet --poc-color Amber
omacharts chart indicator set rsi --period 21 --overbought 80
```

`set` reconfigures one already on the chart. If the chart has two of a kind it
asks which, with `--id`.

A parameter an indicator has no use for is refused rather than ignored —
`--period` on a volume profile is an error, not a no-op, because silence would
read as the command having worked.

## Link groups

A chart belongs to one of nine groups, or to none. Charts in a group show the
same instrument: moving one moves the others, and a pointer on one draws a
crosshair on the rest.

```
omacharts chart set --link 3        # join group 3 and lead it
omacharts chart set --link none     # leave it, changing nobody
omacharts chart list                # the last column is the group
```

A watchlist drives a group too, and that is the other half of what groups are
for: picking a symbol in a list changes the charts in the group it drives
rather than the one chart you happen to be on. In the window, putting a list in
a group leads it the same way a chart does — with the row the list is on. From
a command there is no such row to lead with, because which row a list is on is
the rail's own state and is nowhere in the database, so this sets the group and
the list then follows it: the rail points at what its new group is showing the
moment that list is on screen.

```
$ omacharts watchlist link Semis 3
"Semis" now drives link group 3
  [exit 0]

$ omacharts watchlist link Semis
3
  [exit 0]

$ omacharts watchlist link Semis none
"Semis" drives no link group
  [exit 0]
```

**A group drives at most one watchlist.** One already taken is refused, and
the refusal names the list holding it, rather than quietly moving it:

```
$ omacharts watchlist link Energy 3
omacharts: link group 3 already drives "Semis"; take it off that list first
  [exit 5]
```

`watchlist list` shows who holds what in one read, which is the question worth
asking before changing any of it. Out of the box the default watchlist drives
group 1, and charts start in group 1, so a fresh install has the list driving
the charts.

**A chart put in a group leads it.** What that chart is showing becomes the
group's symbol, and everything else in the group follows: the other charts, the
charts of the chartbooks that are not open, and the watchlist driving the group.
You linked *this* chart, so this is the symbol you meant the group to be on:

```
$ omacharts chart set --chart pos:1 --link 2
chart 3: link group 2; 1 other chart in group 2 now shows AMD
  [exit 0]
```

`--symbol` and `--link` in one command lead with the new symbol, since that is
what the chart ends up showing.

Three changes write nothing, which is what makes the control safe to touch:

* **`--link none`** takes the chart out and changes nobody. The charts still in
  the group keep what they had.
* **Re-stating the group a chart is already in.** Only an actual change leads,
  so a script that re-applies a chart's whole state does not rewrite four others
  every time it runs.
* **Joining from a chart with no symbol.** The group keeps what it was showing
  rather than being blanked.

`chart set --symbol` on a chart already in a group changes that one chart.
Spreading a symbol over a group is the window's business — picking in the rail,
or typing in the search — and a command names the charts it means.

Whether a pointer on one chart draws a line on the ones linked to it:

```
omacharts chart crosshair off
```

## Worked example: the top US semiconductors in their own watchlist

This is a real session, run exactly as written.

```
$ omacharts watchlist create Semis
created watchlist "Semis" (id 3)
  [exit 0]

$ omacharts watchlist add Semis NVDA AMD AVGO TSM MU
added 5 to "Semis": NVDA AMD AVGO TSM MU
  [exit 0]

$ omacharts watchlist show Semis
Semis

  (no section)
    NVDA
    AMD
    AVGO
    TSM
    MU
  [exit 0]
```

A file of tickers goes in the same way. Leave `$(cat …)` unquoted, so the
shell splits it into one argument per ticker:

```
$ omacharts watchlist add Semis $(cat semis.txt)
added 4 to "Semis": QCOM INTC ARM MRVL
  [exit 0]
```

A watchlist exported from another charting tool needs tidying first —
exchange prefixes, quotes, `###` headings, Windows line endings — and the
shell can do it:

```
$ omacharts watchlist add Semis $(tr -d '\r"' < export.txt | tr ',;' '\n\n' | grep -v '^[[:space:]]*#' | sed 's/.*://')
added 4 to "Semis": AAPL MSFT JPM NVDA
skipped, not instruments: NOTATICKER
  [exit 0]
```

Did not know the tickers? Search for them first:

```
$ omacharts symbol search semiconductor --limit 3
SMH        VanEck Semiconductor ETF           ETF      NASDAQ
SOXX       iShares Semiconductor ETF          ETF      NASDAQ
TSM        Taiwan Semiconductor ADR           Stock    NYSE
  [exit 0]
```

A section can become a watchlist of its own, carrying its symbols with it:

```
$ omacharts section promote Default Energy
turned section "Energy" into a watchlist with its 2 symbols
  [exit 0]
```

Every command takes `--json` when you would rather parse it:

```
$ omacharts watchlist show Semis --json
{"id":3,"name":"Semis","sections":[{"id":7,"name":"","root":true,"collapsed":false,
"symbols":[{"symbol":"NVDA","suffix":null,"display":"NVDA"}, ...]}]}
```

## Putting a watchlist in order

The rail is dragged with a mouse: a section header moves the section, a symbol
row moves the symbol. Both gestures have a command, and the commands are what
a script or an agent has instead of a pointer.

`section order` names the sections first to last. Any you leave out keep the
order they had, behind the ones you named — so "put Energy at the top" is one
section long rather than the whole list spelled out:

```
$ omacharts section order Macro Energy
sections of "Macro": Energy, Indexes
  [exit 0]

$ omacharts watchlist show Macro
Macro

  Energy
    CL
    NG

  Indexes
    SPY
    QQQ
  [exit 0]
```

A section's symbols are positioned inside it, so moving the section moves them
with it and two sections' symbols can never end up interleaved.

The symbols that are in no section have no header to drag and are always the
first thing in the rail. They cannot be ordered: name that section and the
command refuses rather than quietly leaving them somewhere no gesture can put
them. `section list` prints it as `(no section)`, with the id the commands
below need to move symbols in and out of it.

`watchlist move` moves one symbol — into another section, to another place in
the one it is in, or both at once:

```
$ omacharts watchlist move Macro QQQ --section Energy --before CL
moved QQQ from Indexes to Energy in "Macro", in front of CL
  [exit 0]

$ omacharts watchlist move Macro NG --before CL
moved NG in Energy of "Macro", in front of CL
  [exit 0]
```

Without `--before` it lands at the end of the section it is moving into, and
without `--section` it stays in the section it is already in. One of the two is
required: with neither, there is nowhere to move it to, and that is a usage
error rather than a success that changed nothing.

The symbol does not have to say where it is now — that is worked out — with one
exception. A watchlist may hold the same symbol in two sections, and nothing on
the command line says which copy you meant, so that is refused with exit 4
until you name the section it is coming out of:

```
$ omacharts watchlist move Default CL --section Majors
omacharts: CL is in 2 sections of "Default"; say which with --from id:1 or --from id:2
  [exit 4]

$ omacharts watchlist move Default CL --from Energy --section Majors
moved CL from Energy to Majors in "Default", at the end
  [exit 0]
```

## Failure is unambiguous

A name that does not exist and a command that was misspelled fail differently,
so a caller can tell them apart without reading English:

```
$ omacharts watchlist show Nope
omacharts: no watchlist called "Nope"; try `omacharts watchlist list`
  [exit 3]

$ omacharts watchlist addd Semis NVDA
error: unrecognized subcommand 'addd'

  tip: a similar subcommand exists: 'add'
  [exit 2]
```

Mutations say what they changed rather than succeeding silently, so you can
confirm your own work without a second query.

Adding a symbol twice is not an error. Deleting something that is not there
is reported as not found rather than passing quietly.

## Setting up an arrangement of charts

```
omacharts chartbook create Semis --watchlist Semis --symbol NVDA --switch
omacharts chart split horizontal
omacharts chart set --symbol AMD --resolution 1h
omacharts chart focus pos:0
omacharts chart list
```

Each chartbook shows one watchlist beside its charts, and which one is part of
the book rather than of the window — an arrangement of energy charts keeps the
energy list when you come back to it. **A watchlist belongs to one chartbook**,
so handing it to a second is refused and the refusal names the book that has
it. The default watchlist is the exception: it is where every fallback lands,
so any number of books may show it.

```
$ omacharts chartbook watchlist Semis Semis
chartbook "Semis" now shows watchlist "Semis"
  [exit 0]

$ omacharts chartbook list
* id:0 Macro                     2 charts  watchlist "Energy"
  id:1 Semis                     1 charts  watchlist "Semis"
  [exit 0]
```

`chart list` prints the id of each chart, which is what `--chart` takes.
Commands act on the focused chart of the open chartbook unless you say
otherwise.

A `chart set` with one bad value changes nothing at all — everything is
checked before anything is written, so you never get a half-applied chart.

Besides what a chart shows, `chart set` carries how it shows it — `--style`,
`--session`, `--grid` and `--auto-scale` — and those are stored with the
chart, so a price axis held still with `--auto-scale off` is still held the
next time the app opens.

## Taking a picture of a chart

```
omacharts chart screenshot
omacharts chartbook screenshot --output /tmp/book.png
```

`chart screenshot` photographs the focused chart; `chartbook screenshot`
photographs the whole arrangement, every chart as laid out with the dividers
between them. The image is the chart and what names it — the candles, the
scales, the symbol, the resolution and the indicator legend — and none of the
controls drawn over it, nor the sidebar, nor the tab strip. Taking one changes
nothing on screen.

These are the only two commands that cannot be answered from the stored
arrangement, because a screenshot is of pixels and a saved layout has none.
With nothing running they exit `6`.

```
$ omacharts chart screenshot
saved pos:0 NVDA 1h in "Macro" to ~/Pictures/Omacharts/NVDA-1h-20261004-143012.png
  [exit 0]
```

Without `--output` the file goes to the folder in the settings, named after the
symbol, the resolution and the time. `--output` naming a directory rather than
a file puts it there under that same name.

The window's own key copies the image to the clipboard every time. A command
does not, unless you ask for it:

```
omacharts chart screenshot --clipboard
```

That split is deliberate: a loop taking fifty screenshots would otherwise stamp
fifty times on whatever you had copied. Note that a clipboard offer belongs to
the process that made it — unless a clipboard manager is running to take a copy,
what Omacharts puts there is gone when Omacharts quits.

Two settings govern the file. `screenshot_folder` is where it goes; unset means
`<Pictures>/Omacharts`, which is made the first time a screenshot needs it.
`screenshot_save_file` is whether the window keeps one at all — turning it off
leaves the keyboard copying to the clipboard and writing nothing. Neither
setting touches a command: `chart screenshot` always writes a file.

## Preferences

Settings are read and written by name — `config list` shows every one that has
been written, and `config set` writes any of them.

Three of them have a command of their own, because what the app does with them
is more than storing a value. Bar colours is one:

```
$ omacharts config bars monochrome
bars are monochrome; the colours are remembered
  [exit 0]

$ omacharts config bars coloured
bars carry their direction again, in the "hollow" scheme
  [exit 0]
```

`config bars red-up` is the third answer: the theme's colours the other way
round, red for a rise and green for a fall, the way charts are read in Taiwan,
mainland China, Japan and Korea. It remembers the palette in use as well.

`config set bar_scheme theme-mono` reaches the same scheme and is not the same
command: it does not remember the scheme that was in use, so putting the colour
back lands on the default rather than on the palette you had picked.

## Which data feed the charts come from

Yahoo Finance is the default and needs nothing set up. The stored choice is a
setting like any other:

```
$ omacharts config set provider tos
provider is tos
  [exit 0]
```

For one run, without changing what is stored, there is a launch option:

```
omacharts --provider tos
omacharts --provider tos NVDA
```

The order is: `--provider` for this launch, otherwise the stored setting,
otherwise Yahoo. A name that is not a feed stops the launch with code 2 rather
than quietly charting from the wrong source.

`--provider` is a launch option, not a command, and the feed is read once when
the process starts — the request queue is paced to that feed's rules and the
price cache is keyed by its name. So passing it while Omacharts is already
running changes nothing, and says so instead of being ignored. Use
`config set provider` for that, and restart.

The feeds are listed in `omacharts surface --json` under `launch`, so nothing
has to guess the names.

`provider list` shows them with what is stored and what is running, which can
differ when a launch was given the flag:

```
$ omacharts provider list
yahoo    Yahoo Finance · Every listing the symbol search covers · delayed 15 min for indexes, 10 for futures   (stored, from the next launch)
tos      thinkorswim · Your own Schwab paperMoney account, US listings · refetched on a timer, not a live stream   (in use for this launch · not signed in)
  [exit 0]
```

## Signing in to a feed that charts your own account

Yahoo needs nothing. thinkorswim charts your Schwab account, so it has to be
signed in to once, and `provider status` is how you find out where you stand:

```
$ omacharts provider status
thinkorswim · Your own Schwab paperMoney account, US listings · refetched on a timer, not a live stream
Not signed in
charts will be empty until you sign in: omacharts provider login
session: /home/you/.config/omacharts/tos.env
browser profile: /home/you/.config/omacharts/tos-browser
  [exit 0]
```

`provider login` opens a real browser window at thinkorswim and waits while
you sign in — your password and your one-time code are typed by you, into the
browser, and Omacharts neither sees them nor types anything into the page. It
needs a Chromium-family browser on the machine; with none, it says which to
install rather than failing in the browser's words. Progress goes to stderr
while it waits, so `--json` is still one object on stdout.

```
$ omacharts provider login
a browser is opening at thinkorswim; sign in there
…
signed in to thinkorswim
  [exit 0]
```

Unlike every other command, `login` and `logout` run in the terminal you typed
them in even when a window is open. A sign-in waits for a person, and a command
handed to the window runs inside its main loop — which would be ten minutes of
frozen application with the browser it is waiting for sitting on top of it.
`surface --json` marks both with `runsInTheCaller`.

`provider logout` forgets the saved session. The browser profile stays, so
signing in again is usually a click rather than another one-time code.

Sessions expire. When one does, charts say so rather than claiming the
provider is unreachable, `provider status` says `Session expired`, and signing
in again is the fix. All of this is also in Preferences → Market data →
Provider, which is the same four facts with a button.

## The widget in the Omarchy bar

The bar widget is a plugin folder plus an entry in the shell's layout. The
switch in Preferences writes both, and so does this:

```
$ omacharts plugin status
not in the bar; it would go in /home/you/.config/omarchy/plugins/jorgemanrubia.omacharts
  [exit 0]

$ omacharts plugin install
installed in the bar, in /home/you/.config/omarchy/plugins/jorgemanrubia.omacharts
  [exit 0]
```

`install` on a widget that is already there brings it up to date rather than
writing it again, which matters because the widget is installed once and then
never touched: without that, the bar keeps running whichever version shipped
the day the switch was flicked.

`uninstall` is idempotent, and a desktop with no Omarchy shell is reported
rather than refused — having no bar is a fact about the machine, not a command
that went wrong. Both exit 0:

```
$ omacharts plugin uninstall
taken out of the bar
  [exit 0]

$ omacharts plugin uninstall
not in the bar, so there was nothing to remove
  [exit 0]
```

## The cached market data

Bars are cached on disk and pruned in the background back under a limit:

```
$ omacharts cache status
1041 series · 212 MB of 1.0 GB · 21%
  [exit 0]

$ omacharts cache limit 2GB
the cache will be pruned back under 2.0 GB
  [exit 0]
```

`cache clear` throws every cached series away. Settings, watchlists and
chartbooks are not cache and are left alone.

## Teaching an agent about this app

Everything above is enough for an agent that has already been pointed here.
The gap is before that: `--help` only answers once something has thought to
ask, and `AGENTS.md` is only read by an agent already inside this repo. So
Omacharts ships a skill — `agents/skills/omacharts/SKILL.md` — whose whole job
is to be found, anywhere on the machine, when somebody says "what's semis
doing" or "set me up for the open".

It does not describe the command surface. `omacharts surface --json` already
does that perfectly and regenerates itself; a hand-written copy would be wrong
by the second release, and more confidently wrong than no copy at all. The
skill carries the two things the surface structurally cannot: that this app is
worth reaching for, and the orders commands go in — a 2×2 of linked charts is
several commands and a handful of facts about `pos:N` and link groups, none of
which is a word in any one command's help.

**One skill, not one per agent.** Claude reads `~/.claude/skills/<name>/`, Codex
reads `$CODEX_HOME/skills/<name>/`, and both want the same thing: a directory
with a `SKILL.md` whose frontmatter has a `name` and a `description`. So there
is one file, and installing points every agent at it. A copy each would be a
second thing to drift, which is the one failure worth designing against.

```
$ omacharts skill install
installed for Claude: /home/you/.claude/skills/omacharts -> /usr/share/omacharts/agents/skills/omacharts
installed for Codex: /home/you/.codex/skills/omacharts -> /usr/share/omacharts/agents/skills/omacharts
  [exit 0]

$ omacharts skill status
installed for Claude: /home/you/.claude/skills/omacharts -> /usr/share/omacharts/agents/skills/omacharts
not installed for Codex; `omacharts skill install` would write /home/you/.codex/skills/omacharts
  [exit 0]

$ omacharts skill uninstall
removed for Claude: /home/you/.claude/skills/omacharts
  [exit 0]
```

Four things about that command are deliberate, and the first is the reason the
rest exist.

**It is never a side effect.** Not the package's post-install, not
`bin/install`, not the first launch. Somebody installing a charting app has not
agreed to have their agent's configuration written into, and there is no
version of "it is only a small file" that makes that assumption all right. One
explicit command, or nothing.

**It installs for whichever agents you have.** An agent counts as present if
its configuration directory is there — meaning it has run here and has state —
**or** its command is on `PATH`, which catches one installed but not yet
started. Either signal is enough on purpose: a skill installed for an agent you
do not use is a directory you never look in, while one missing for an agent you
do use is a failure you would have to notice and diagnose. `--claude` and
`--codex` name one outright, present or not, because somebody who typed the flag
has said what they mean. Finding nothing at all is not an error — it says what
it looked for and writes nothing.

**It is a symlink, not a copy.** The risk worth designing against is a skill
that drifts from the CLI, and the skill is kept beside the surface it describes
with a test that runs its examples — a link keeps that true through every
package upgrade, where a copy goes stale the first time a command changes.
Removing the package then leaves a dangling link, which is the right way round
to fail: a dangling link loads nothing and `skill status` says `points at ...
which is not there any more`, while a stale copy would go on answering,
wrongly. Codex evidently agrees — the skills directory it ships with is itself
full of links into `/usr/share`.

**It refuses rather than clobbers, per agent.** Anything already at an agent's
`skills/omacharts` that Omacharts did not put there is left exactly as it is,
and `uninstall` is held to the same rule, so it can only ever remove the link
it made. One agent refused does not hide another working: both are reported,
and the exit status carries the refusal.

```
$ omacharts skill install
installed for Claude: /home/you/.claude/skills/omacharts -> /usr/share/omacharts/agents/skills/omacharts
omacharts: not installed for Codex: /home/you/.codex/skills/omacharts is already
a directory, and not something Omacharts put there; move it aside and run this again
  [exit 5]
```

So a script that reads only the exit code learns that something it asked for
did not happen, and one that reads the lines learns which. `--json` puts the
whole report on stdout as one document either way.

Running `install` twice is not an error. `--to DIR` ignores the agents and uses
one directory — a project's own `.claude/skills`, say.

### The Claude Code plugin

Claude Code can also take the skill as a plugin, which is worth it for one
reason: `claude plugin update omacharts` picks up a new skill without a new
package, and `/plugin` lists and disables it like anything else.

```
claude plugin marketplace add /usr/share/omacharts/agents
claude plugin install omacharts@omacharts
```

From a clone rather than the package, `./agents` is the same thing.

Its `marketplace.json` also declares a `relevance` signal on the `omacharts`
command, which is meant to suggest the plugin the first time somebody runs
`omacharts` in a session. Be aware that only fires when the marketplace is
allowlisted in `pluginSuggestionMarketplaces` in **managed** settings, so on an
ordinary machine it does nothing yet. The skill's own description is what
actually does the discovering.

**Codex needs no equivalent.** It has plugins and a marketplace of its own, but
nothing in them corresponds to a `relevance` signal — a Codex plugin manifest
carries display metadata, not a discovery trigger — and Codex reads
`$CODEX_HOME/skills/` directly. So for Codex the skill directory is the whole
story, and adding a manifest would be a second thing to maintain for nothing.

## The command groups

| Group | What it covers |
|-------|----------------|
| `status` | what the app has open right now |
| `symbol` | searching the instrument inventory |
| `watchlist` | watchlists and the symbols in them |
| `section` | the named groups inside a watchlist |
| `chartbook` | saved arrangements of charts |
| `chart` | the charts inside a chartbook |
| `provider` | the data feed, and signing in to one that needs it |
| `config` | stored preferences |
| `cache` | the cached market data |
| `plugin` | the widget in the Omarchy bar |
| `skill` | the agent skill, installed only when asked |

`omacharts <group> --help` and `omacharts <group> <command> --help` both work,
as does `omacharts help <group> <command>`.
