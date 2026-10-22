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
  sidebar without a restart. The output and the exit status come back to your
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
* pos:0  AAPL       1D    focus
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
| `bollinger` | `--period`, `--deviations`, `--bands` (`1` or `none`), `--band-alpha` |
| `rsi` | `--period`, `--overbought`, `--oversold`, `--height` |
| `macd` | `--fast`, `--slow`, `--signal`, `--signal-color`, `--height` |
| `atr` | `--period`, `--height` |
| `stochastic` | `--period` (%K length), `--k-smooth`, `--d-period`, `--overbought`, `--oversold`, `--height`, `--d-color` |
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
omacharts chart indicator add stochastic --period 14 --k-smooth 3 --d-period 3
omacharts chart indicator add bollinger --period 20 --deviations 2
omacharts chart indicator add macd --fast 12 --slow 26 --signal 9 --signal-color Amber
```

A Bollinger and a VWAP carry the same kind of band, so they answer to the same
two flags: `--bands` says which of them are drawn and `--band-alpha` how solid
the shading is. A VWAP has three, at one, one and a half and two standard
deviations; a Bollinger has one, and `--deviations` is what moves it. Naming a
band an indicator has not got is an error rather than a quiet no-op, so
`--bands 2` on a Bollinger is refused instead of switching its only band off.

A MACD's quick average has to stay quicker than its slow one — equal lengths
measure nothing and the wrong way round is the line upside down — so
`--fast 26 --slow 12` is refused, and so is either one on its own that would
cross what the chart already holds.

`set` reconfigures one already on the chart. If the chart has two of a kind it
asks which, with `--id`.

A parameter an indicator has no use for is refused rather than ignored —
`--period` on a volume profile is an error, not a no-op, because silence would
read as the command having worked.

## Chart templates

A chart template is a named set of indicators that charts share. A chart on a
template shows the template's indicators, and changing the template changes
every chart on it at once, in every chartbook. A new profile has one called
**Default**, holding the volume study, and new charts open with it.

```
omacharts template list
omacharts template create Momentum
omacharts template create Swing --from-chart
omacharts template indicator add Momentum rsi --period 14
omacharts template indicator add Momentum macd --signal-color Amber
omacharts chart set --template Momentum
omacharts template default Momentum
```

`template indicator` takes the same actions and the same parameters as
`chart indicator` — the table above applies to both — with the template named
before the indicator. `template list` marks with `*` the one new charts open
with, and says how many charts use each.

`template create NAME --from-chart` makes the template out of what a chart
already shows: the focused chart's indicators, or the one `--book` and
`--chart` name, which is then put on the new template. A chart with no
indicators is refused, and with no window open and no `--book` the command
exits `6`, as a chart verb does.

Putting a chart on a template replaces its own indicators with the template's.
`chart set --template none` puts it on none, which is the empty set: the chart
is left with no indicators. To start from a template and make it the chart's
own, make a template from it — that is the next paragraph.

**Changing a chart's indicators changes its template.** `chart indicator
add`, `remove` and `set` on a chart that is on one change the template, and
every chart on it, the same as the window's Indicators page does — and say
so, naming the template and how many charts followed:

```
omacharts chart indicator add sma --period 200
# added SMA(200) on pos:0 SPY 1D in "Macro", in chart template "Default"
```

To give one chart a set of its own, make a template from it (right-click the
chart › Template › Create new template from chart…) and change that one.

Renaming a template keeps every chart on it, since charts name it by id.
Deleting one leaves its charts with a copy of its indicators, as their own.
The default for new charts is also `config set chart_template NAME|none`,
which is the same setting `template default` writes.

## The header

The line at a chart's top-left shows its symbol and resolution, and can also
show the day's move — as a percentage, in the chart's own colours for a rise
and a fall — and the name of its chart template. It is one line for the whole
app: every chart in every chartbook shows the same parts, in the order given,
and a window that is open changes them all at once:

```
omacharts config set chart_header symbol,resolution,change
omacharts config set chart_header template,symbol
```

`none` is a header with nothing in it. The day's move comes from the same
cached quotes the watchlist shows, so it is as fresh as the sidebar and fetches
nothing of its own.

## The clock

Charts read one time zone and write times one way, for the whole app: the time
axis, the crosshair, the times typed into an indicator, and the times
`chart drawing` reads and prints. A window that is open changes at once:

```
omacharts config set chart_time_zone Europe/London
omacharts config set chart_time_zone exchange
omacharts config set time_format 12h
```

The zone is an IANA name, New York until one is set; `exchange` puts each chart
in the clock of the exchange its instrument trades on. A name that is not a
zone is refused rather than read back as New York. The format is `24h` or
`12h`, and until one is set it follows the desktop's clock.

A Session Times range keeps the chart's clock unless it is given one of its
own, which is what a market named after a city wants — London opens at eight in
London:

```
omacharts chart indicator set session_times --add-range custom --name London --from 8am --to 4:30pm --zone Europe/London
```

## Link modes

Each chart has a link mode: what a click in the watchlist does to it.

* **`focus`** (the default) takes the click while it is the focused chart.
  Split a chart and click a symbol, and the chart in front of you takes it
  while the other keeps what it had.
* **`follow`** takes every click. Follow charts show the same instrument: a
  symbol typed on one moves the others, and a pointer on one draws a crosshair
  on the rest. Make a chart Follow, split it, change the new one's resolution,
  and both walk the watchlist together.
* **`pinned`** takes none, focused or not — for a context chart such as SPY
  beside a stock. A click that moves nothing, because the focused chart is
  pinned and nothing follows, lights up its pin.

```
omacharts chart set --link follow   # follow the watchlist, and lead the others
omacharts chart set --link pinned   # never moved by the watchlist
omacharts chart list                # the last column is the mode
```

The chain in a chart's top-right corner picks any of the three once there is a
second chart to tell apart, and the chart menu has them under Watchlist. A lone
chart shows its chain only while it is pinned. Ctrl+L moves the focused chart
on to its next mode, round Focus, Follow and Pinned, and Ctrl+Shift+L back the
other way; arriving at Follow, it takes what the other Follow charts show rather
than moving them to what it had. Following is within the open
chartbook: a chartbook you come back to is as you left it.

`omacharts SYMBOL`, which is what a click in the bar widget runs, picks the
symbol in a watchlist the way a click does. When nothing takes it, because the
focused chart is pinned and nothing follows, it says so and exits 5.

A chartbook saved when charts were in one of nine link groups keeps what a
click did to each chart: a chart in the group its chartbook's watchlist drove
becomes `follow` and any other chart `focus`.

### Sorting a watchlist

The sidebar's column headers sort it: click one for low to high, again for high to
low, and a third time to go back to the order the list was written in. The same
from a terminal, kept per watchlist:

```
$ omacharts watchlist sort Semis change_pct --descending
"Semis" sorted by change_pct, high to low
  [exit 0]

$ omacharts watchlist sort Semis
change_pct:desc
  [exit 0]

$ omacharts watchlist sort Semis none
"Semis" is in its own order
  [exit 0]
```

A list is sorted **within each section**, so the sections stay where you put
them, and it keeps itself sorted as prices move. A symbol without a price yet
sorts last either way. While a list is sorted, dragging a symbol within its
section does nothing — the sort decides its place — but dragging it into
another section still moves it.

**A chart made `follow` leads.** What that chart is showing becomes what every
other Follow chart in the chartbook shows, and the watchlist points at it. You
made *this* chart follow, so this is the symbol you meant them to be on:

```
$ omacharts chart set --chart pos:1 --link follow
chart 3: link follow; 1 other Follow chart now shows AMD
  [exit 0]
```

`--symbol` and `--link` in one command lead with the new symbol, since that is
what the chart ends up showing.

Three changes write nothing, which is what makes the control safe to touch:

* **Leaving follow** (`--link focus` or `--link pinned`) changes nobody. The
  charts still following keep what they had.
* **Re-stating the mode a chart is already in.** Only an actual change leads,
  so a script that re-applies a chart's whole state does not rewrite four others
  every time it runs.
* **Following from a chart with no symbol.** The Follow charts keep what they
  were showing rather than being blanked.

`chart set --symbol` on a Follow chart changes that one chart. Spreading a
symbol over the Follow charts is the window's business — picking in the sidebar,
or typing in the search — and a command names the charts it means.

Whether a pointer on a Follow chart draws a line on the other Follow charts:

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

The sidebar is dragged with a mouse: a section header moves the section, a symbol
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
first thing in the sidebar. They cannot be ordered: name that section and the
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

## Moving watchlists between machines

`watchlist export` writes every watchlist — sections, their order and the
symbols in them — as one JSON file, and `watchlist import` brings it in on the
other machine. Carry the file however you like:
Syncthing, a dotfiles repository, `scp`.

```
$ omacharts watchlist export > watchlists.json
$ omacharts watchlist export Semis Macro > some.json

$ omacharts watchlist import watchlists.json
Default: added 2 symbols
Semis: created, 12 symbols in 3 sections
Macro: already up to date
  [exit 0]
```

**Importing adds and never takes away.** A watchlist is matched by name, and
the default one to the default one whatever either is called. A match gains
the sections and symbols it lacks, after what it already has; nothing it holds
is moved or removed, and a symbol it already has is not added again in another
section. A watchlist with no match is created. So the same file can be imported
on every machine, as often as you like, and a second run changes nothing. A
file exported when lists drove link groups still imports; the group is read
past.

`--replace` makes each watchlist in the file exactly what it was where it was
exported — sections and order — for when this machine should match
rather than combine. Watchlists the file does not name are never touched, by
either form.

An import lands whole or not at all: a file that is not an export, one made by
a newer Omacharts, or one naming a list that is ambiguous here changes nothing.

`-` reads the file from stdin, which is how to import over ssh:

```
$ ssh desktop omacharts watchlist import - < watchlists.json
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

`--resolution` takes what you would type on the chart: a bare number is
minutes (`240` is four hours), and a suffix names the unit — `m`, `h`, `D`,
`W`, `M` for months and `Y` for years. Case does not matter except for the
month: `1M` is a month and `1m` a minute, and `1mo` is a month in any case.
Monthly and yearly bars are built from daily ones, on calendar months and
years in UTC, so `3M` is the calendar quarters.

A `chart set` with one bad value changes nothing at all — everything is
checked before anything is written, so you never get a half-applied chart.

Besides what a chart shows, `chart set` carries how it shows it — `--style`,
`--session`, `--grid`, `--auto-scale` and `--log-scale` — and those are stored
with the chart, so a price axis held still with `--auto-scale off` is still
held the next time the app opens.

`--log-scale on` makes the price axis logarithmic: equal heights are equal
percentage moves, which is what a chart of twenty years wants. Indicators in
their own strips stay linear. While the bars on screen go to zero or below —
a spread, a contract that traded negative — there is no logarithm to take, so
the axis draws linear and turns logarithmic again once they are off screen.

```sh
omacharts chart set --resolution 1W --log-scale on
```

## Chartbook templates

A chartbook template is a chartbook's arrangement under a name: the splits,
every divider as a share of its split rather than in pixels so it fits any
window, and for each chart its resolution and its chart template, or its own
indicators when it has none. The symbols are not in it. A chartbook keeps its
own symbols and link modes, and taking a template gives them to the charts by
where they sit; a chart the template adds is a copy of the focused one. A
template saved before resolutions were part of one leaves each chart's own.

```
omacharts chartbook template save Corners
omacharts chartbook template apply Corners --book Energy
omacharts chartbook create Metals --symbol GLD --template Corners --switch
omacharts chartbook template list
omacharts chartbook template rename Corners Quarters
omacharts chartbook template apply none --book Energy
omacharts chartbook template delete Quarters
```

**A chartbook on a template stays on it, and every change to its arrangement
goes to the template and to every other chartbook on it** — a split, a closed
chart, a dragged divider, a resolution, an indicator, a chart template — the
way a change to a chart template reaches every chart on it. A command that changes one is no
different from the window doing it. A chart that is added in one book is added
beside the same chart in the others, and one closed in one is closed in the
others, so each book keeps its symbols where they were.

`save NAME` makes a new template from the chartbook and puts the book on it.
It only ever makes a new one: a name a template already has, in any case, is
refused, and nothing changes. A book on a template needs no saving, since its
changes are already in it.
`chartbook create NAME --template T` makes a book on a template from the
start, every chart on `--symbol`. `apply none` takes a book off its template
and keeps everything it has.
Deleting a template does the same to every book on it. `list` marks with `*`
the template the open chartbook is on, and says how many chartbooks use each.

In the window these are the **Template** submenu on a chartbook tab's
right-click menu: None, the templates and Save as New Template…, then Manage
Templates…, and the New chartbook dialog's **Template** row, which is there
once a template exists.

`chartbook duplicate` makes a copy beside a chartbook — the charts, their
settings and drawings, and its chartbook template — and `--switch` opens it.
A watchlist belongs to one chartbook, so the copy shows the default one.

```
omacharts chartbook duplicate Macro --name "Macro weekly" --switch
```

## Drawing on a chart

A drawing is a line between two anchors, a box or an ellipse with them at
opposite corners, or words at one. An anchor is a moment and a price —
`2026-09-01,180.5`, or with the minute, `2026-09-01T14:30,180.5`, or unix
seconds — which is how the chart keeps them too, so a line through two daily
closes still passes through the same two moments on the hourly chart.

```
omacharts chart drawing add line --from 2026-09-01,180.5 --to 2026-09-19,192 --config 4
omacharts chart drawing add rect --from 2026-09-08,178 --to 2026-09-12,186 --fill down --alpha 0.25
omacharts chart drawing add ellipse --from 2026-09-08,178 --to 2026-09-12,186 --config 5
omacharts chart drawing add text --from 2026-09-10,184 --text "gap fills here"
omacharts chart drawing list
omacharts chart drawing set --id 1 --color ink --width 2.5 --arrow end
omacharts chart drawing set --id 1 --order front
omacharts chart drawing remove --id 2
omacharts chart drawing clear
```

**An ellipse is a box with a curve for an outline.** `ellipse` — `circle` is
read as the same word — takes the same two anchors at opposite corners of the
box it is drawn in, and the same fill and edge a box has. It is never held
round: dragged wide it is wide. What it has that a box has not is empty
corners, which belong to whatever is under them.

**A text drawing is at one point**, so `add text` takes `--from` and does not
read `--to`, and needs `--text`: words that say nothing draw nothing, and are
refused rather than written down invisible.

**A drawing follows a configuration, or has a look of its own.** Each kind
has nine configurations, 1 to 9; a drawing that follows one looks like it and
changes with it. `--config N` puts a drawing on configuration N. Any property
given by hand — `--color`, `--width`, `--arrow` and `--head` (filled, open
or barb) for a line; `--fill`,
`--alpha`, `--border`, `--width`, `--color` for a box's edge — takes the
drawing off its configuration and gives it that look, which no later change
to the configuration touches. The configurations themselves:

```
omacharts chart drawing configs
omacharts chart drawing configure line --config 3 --color teal --arrow end
omacharts chart drawing reset-configs rect
```

**Any drawing can carry words.** `--text` on a line, a box or an ellipse is a
label on it, placed by `--text-at` — `center` by default, or an edge or a
corner: `top`, `bottom`, `left`, `right`, `top-left` and the rest. The empty
string takes a label off again.

```
omacharts chart drawing set --id 1 --text "supply" --text-at top
omacharts chart drawing set --id 1 --text-color ink --font-size 16
omacharts chart drawing set --id 1 --text ""
```

What a drawing says is never part of a configuration — a configuration says
how words are set, never which words — so `--text` leaves a drawing on the
configuration it follows. How they are set is: `--text-color`, `--font` and
`--font-size` are among the nine, and giving one by hand takes the drawing
off its configuration the way `--color` does. `--font system` is the
desktop's own face, which is the default and what `--json` reports as a null
font; anything else is a family by name, and a family this machine does not
have falls back to the system's.

Bold and italic are runs inside the text rather than properties of it, and
have no flag: they are set on the chart, with the caret in the word. `--text`
writes plain characters, so using it on a label that was partly bold flattens
it. `--json` reports the runs, so a reader can see where the bold was.

**Text is held to a contrast floor against what it lands on**, which is the
composited fill inside a filled box or ellipse and the chart's background
anywhere else. A colour already clear of the floor is drawn exactly as the
preset resolves it, so an amber label on an amber box is the box's amber; one
that is not is lifted until it reads.

**Colours are the nine presets, or a hex.** `up` and `down` are the theme's
candle colours; `blue`, `amber`, `violet`, `teal`, `orange` and `cyan` are the
theme's own swatches; `ink` is the text colour. A preset is resolved against
whatever desktop theme is active, so a drawing goes on looking like it belongs
when the theme changes; `#rrggbb` is exactly that colour, and the theme leaves
it alone. The shipped configurations are the nine presets in order, so
configuration 1 is the up colour and 2 the down colour.

**Drawings belong to the symbol, and two switches say how a chart takes part.**
Both are on by default, so a drawing made on a symbol is a drawing about the
symbol: every chart of it sees what every other draws.

```
omacharts chart set --send-drawings off
omacharts chart set --show-drawings off
```

`--send-drawings off` keeps what is drawn on that chart to that chart —
useful for sketching on one view without it appearing on every other.
`--show-drawings off` keeps the chart clear of what is already drawn on the
symbol; its own are still there either way. The two are independent: a chart
that shows but does not send is a reference view.

Both switches are live, not only about what comes next. Turning sending off
takes back the drawings that chart sent — they stay on it and come off the
others — and turning it on sends back what it has been keeping. Only its own
move: each drawing remembers which chart made it, so a switch on one chart
never withdraws another's work, and a drawing made before charts recorded
that belongs to none of them and stays where it is.

A drawing's own `--scope` is `shared` or `local`, and defaults to what the
chart sends. A local drawing lives with its chart rather than with the symbol
and is numbered below zero, so one id names a drawing wherever it lives.
`list` names the symbol and both switches; `clear` removes everything the
chart can see.

By hand: Ctrl+D shows the tools, Alt+L, Alt+H, Alt+A, Alt+Z, Alt+R, Alt+C and
Alt+T arm them
(Escape is the pointer again; a right-click drops a drawing half laid down
and keeps the tool), a press and a second press (or one
press-and-drag) place a drawing — except the text tool, where one press puts
the caret down and you type on the chart; hold Ctrl while placing one or
dragging a corner and the anchor snaps to the nearest open, high, low or
close of the candle under it, and with a
tool in hand the crosshair snaps with it to show where — a click selects one (Shift+click or Ctrl+click adds or removes
another, Shift+drag on empty chart takes everything the box touches, and
Ctrl+A takes every drawing on the plot; what
is done to
the selection is done to all of them, properties included when they are all
one kind), a drag moves a corner or the whole thing, the arrow keys
nudge it (Ctrl+Shift+↑/↓ bring it to the front or send it to the back),
Delete removes it,
Enter or a right-click opens its properties, Alt+1 to Alt+9 is configuration
N, and Ctrl+Z and Ctrl+Y undo and redo on that chart.

Words are typed where they will be drawn: double-click any drawing, or take
"Edit text" from its menu, and the caret lands on the chart in the drawing's
own font and ink. Enter commits and Shift+Enter is a newline; Escape leaves
it as it was; Ctrl+B and Ctrl+I set the selection — or the word the caret is
in — bold or italic. Ctrl+= and Ctrl+− grow and shrink the selected
drawing's text, which on a figure is its label rather than the figure.

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
bars carry their direction again, in the "theme-hollow" scheme
  [exit 0]
```

`config bars red-up` is the third answer: the theme's colours the other way
round, red for a rise and green for a fall, the way charts are read in Taiwan,
mainland China, Japan and Korea. It remembers the palette in use as well.

Bar colours belong to each chart, so `config bars` answers for every chart at
once — the charts in every chartbook and the ones made next — the way Apply to
all charts does in the preferences. One chart on its own is
`chart set --colouring theme|theme-hollow|theme-red-up|theme-mono`.

`config set bar_scheme theme-mono` changes only what new charts open with, and
does not remember the scheme that was in use.

What a new chart opens with is a handful of settings, the ones the preferences
show under Charts: `bar_style`, `bar_scheme`, `chart_session`, `show_grid`,
`chart_log_scale`, `chart_shows_drawings` and `chart_sends_drawings`. Changing
one chart never changes them.

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

A change takes effect at once. `config set provider tos` with a window open
switches its charts to the other feed on the spot — each chart paints from
that feed's cache where it has one and keeps what it shows until the new bars
land where it does not — and `--provider tos` typed at a running window
switches that window the same way without storing anything, and says so.
Nothing is cleared: each feed's bars are kept apart in the cache, so switching
back is instant. A symbol the new feed cannot carry, or a feed nobody has
signed in to, is reported on the chart the way any other failure is.

The feeds are listed in `omacharts surface --json` under `launch`, so nothing
has to guess the names.

`provider list` shows them with what is stored and what is running, which can
differ when a launch was given the flag:

```
$ omacharts provider list
yahoo    Yahoo Finance · Every listing the symbol search covers · delayed 15 min for indexes, 10 for futures   (stored, not in use)
tos      thinkorswim · US listings, from your thinkorswim account · live, as each print arrives · experimental   (in use, not stored · not signed in)
  [exit 0]
```

## Signing in to a feed that charts your own account

Yahoo needs nothing. thinkorswim charts your Schwab account, so it has to be
signed in to once, and `provider status` is how you find out where you stand:

```
$ omacharts provider status
thinkorswim · US listings, from your thinkorswim account · live, as each print arrives
Not signed in
charts will be empty until you sign in: omacharts provider login
session: /home/you/.config/omacharts/tos.env
browser profile: /home/you/.config/omacharts/tos-browser
  [exit 0]
```

The last lines of `provider status` say how the feed delivers bars, and — for
one that streams — what the window holds a subscription to. A streamed feed
is never refetched on a timer: each chart on screen is one subscription, shared
by every chart showing the same symbol at the same resolution, and the line
per subscription says how many charts it serves, how many bars it holds and
when the last one arrived. It says plainly when a subscription is open and
nothing has ticked, which with the market shut is the normal state of a live
chart, rather than implying data is flowing. With no window open nothing is
subscribed, and it says that too.

```
$ omacharts provider status
thinkorswim · US listings, from your thinkorswim account · live, as each print arrives
Signed in · account D-1 · signed in 6 Oct 2026
session: /home/you/.config/omacharts/tos.env
browser profile: /home/you/.config/omacharts/tos-browser
delivery: streamed — bars arrive as they print, and nothing fetches on a timer
streaming 2 series:
  /ES 1m · 2 charts · 2602 bars · last bar 14:32 · updated 3 s ago
  AAPL 1D · 1 chart · waiting for the first snapshot
  [exit 0]
```

A polled feed says `delivery: polled — charts are refetched on a timer` and
nothing more. With `--json` the same facts are `delivery` and `subscriptions`.

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

A click on a ticker in the widget runs `omacharts --watchlist id:1 NVDA`. The
widget shows the default watchlist, so that is the list the window moves to:
the chartbook showing it, or — when none does — the book on screen, pointed at
it. Without `--watchlist`, a bare symbol stays in the list you are in when that
list holds it. Without a symbol, `--watchlist` just brings that list on
screen the same way. The option takes a name or `id:N`, and a list that does
not exist is refused with the window still put up:

```
omacharts --watchlist Default NVDA
omacharts --watchlist id:3 SAP DE
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

## Backups of your settings

Everything you made — settings, watchlists, chartbooks, drawings, themes — is
copied to `backups/` beside the settings file when the window opens and every
hour after, only when something changed: a copy identical to the newest one is
never kept. One every two hours is kept for the last day, and the newest of each
day for the week before, so never more than twenty. Cached prices are not part
of it.

```
$ omacharts backup list
settings-20261021-140307.db  2026-10-21 16:03:07     68 kB  schema 4
settings-20261021-130212.db  2026-10-21 15:02:12     68 kB  schema 4
  [exit 0]

$ omacharts backup create
backed up the settings to settings-20261021-141522.db (68 kB)
  [exit 0]

$ omacharts backup restore settings-20261021-130212
restored the settings from settings-20261021-130212.db (2026-10-21 15:02:12); what they replaced is kept as settings-20261021-141530.db
  [exit 0]
```

Settings from another machine — one of its backups, or its `omacharts.db` —
restore the same way, given the file's path. This
machine keeps its own screenshot folder, cache limit and sidebar width, and a
file that is not Omacharts settings is refused with nothing changed:

```
$ omacharts backup restore omacharts.db
restored the settings from omacharts.db (2026-10-21 12:10:21); what they replaced is kept as settings-20261021-101021.db
```

Names are the moment in UTC; the listing shows local time. A restore applies at
once, in a window that is open too: the charts, sidebar and theme on screen become
the restored ones. What it replaced is backed up first, so restoring that undoes
it. A backup written by a newer Omacharts is refused with nothing changed, and
one from an older one is brought up to date as it is restored. Preferences ›
Data › Backups does the same from the window.

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
several commands and a handful of facts about `pos:N` and link modes, none of
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
| `template` | chart templates: named sets of indicators that charts share |
| `provider` | the data feed, and signing in to one that needs it |
| `config` | stored preferences |
| `cache` | the cached market data |
| `backup` | copies of your settings, and putting one back |
| `plugin` | the widget in the Omarchy bar |
| `skill` | the agent skill, installed only when asked |

`omacharts <group> --help` and `omacharts <group> <command> --help` both work,
as does `omacharts help <group> <command>`.
