---
title: "The CLI"
description: "Every omacharts command, with its flags, replies and exit codes."
section: agents
nav_order: 30
---

`omacharts` is the command that drives Omacharts from a terminal. Every
action in the window has a command.

## Launching the app

| Command | Does |
|---|---|
| `omacharts` | Opens the window, or brings the open one to the front |
| `omacharts SYMBOL [SUFFIX]` | The same, and selects the symbol in its watchlist, as a click there would. The second word is the venue: `omacharts SAP DE` |
| `omacharts --watchlist LIST [SYMBOL]` | Shows that watchlist, by switching to the chartbook that shows it or pointing the open one at it, and selects the symbol in it. `LIST` is a name or `id:N` |
| `omacharts --provider NAME [SYMBOL]` | Charts from that feed for this launch only, without changing the stored setting ([Market data](#market-data)). Typed at an open window, switches it for as long as it stays open |
| `omacharts --help`, `-h` | The groups, the launch options and a few examples |
| `omacharts --version`, `-V` | `omacharts 0.1.22` |

`omacharts NVDA` selects NVDA in the open chartbook's watchlist when that
list has it, otherwise in the first chartbook whose list has it, which then
opens. A symbol no list holds brings the window up and changes nothing.

A second `omacharts` acts on the open window; it never opens another.

```
$ omacharts NVDA
  [exit 0]

$ omacharts --watchlist Nope
omacharts: no watchlist called "Nope"; try `omacharts watchlist list`
  [exit 3]

$ omacharts --provider bogus
omacharts: no such data feed: "bogus"; the feeds are yahoo, tos
  [exit 2]
```

An unknown option prints the usage and exits `1`. When no chart takes the
symbol, because the focused chart is pinned and no chart follows the
watchlist, the launch exits `5` and says so; see [Linking charts to the
watchlist](/manual/watchlists/#linking-charts-to-the-watchlist).

## Where a command runs

With a window open, a command changes what is on screen. With no window
open, it changes what is stored, and the window opens on that next time.
Commands work over ssh and from cron.

A path is read from the folder you typed the command in, and `-` reads
what you piped in. `-` with nothing piped is refused with exit `2`.

## Which chart a command acts on

Every `chart` and `chartbook` command acts on the focused chart in the open
chartbook unless told otherwise:

| Flag | Takes | Default |
|---|---|---|
| `--book` | A chartbook's name, or `id:N` | The open chartbook |
| `--chart` | `pos:N`, or a raw id from `chart list` | The focused chart |

A position names a chart by its place in the arrangement: `pos:0`, `pos:1`
and so on, counting from 0. In a two-by-two made by splitting across and
then each half down, `pos:0` is the top left, `pos:1` the bottom left,
`pos:2` the top right and `pos:3` the bottom right.
[`status show`](#status-show) lists the open chartbook's charts by position.

Prefer `pos:N`. Ids change when charts are split or closed; a position does
not.

A command that changes something names the chart it changed:

```
$ omacharts chart indicator add rsi
added RSI(14) on pos:0 SPY 1D in "Macro"
  [exit 0]
```

With no window open there is no focused chart, so name one:

```
$ omacharts chart set --resolution 1W
omacharts: no window is open, so there is no focused chart to act on; name one, or start Omacharts
  [exit 6]

$ omacharts chart set --book Desk --chart pos:0 --resolution 1W
chart 55: resolution 1W
  [exit 0]
```

## `status show`

`status show` prints the open chartbook and its charts, with `*` on the
focused one. Each line gives the position, symbol, resolution and link mode,
spelled the way the other commands take them.

```
$ omacharts status show
chartbook  Desk (id:2)
  pos:0  NVDA       1D    follow
  pos:1  NVDA       1h    follow
  pos:2  NVDA       15m   follow
* pos:3  NVDA       5m    follow
  [exit 0]
```

With no window open it says so, then prints what was stored:

```
$ omacharts status show
nothing is open; this is what was stored when the window last closed

chartbook  Semis (id:1)
* pos:0  AMD        1h    follow
  [exit 0]
```

`--json` adds every chart's settings (style, colouring, session, scales, link
mode, whether it shows and sends drawings, indicators and chart template) and
`windowOpen`, which says whether you are reading the window or the stored
data.

## Naming things

Watchlists, sections, chartbooks and templates are picked by name; capitals
do not matter, so `semis` finds *Semis*. A name that matches nothing exits
`3`. A name that matches two exits `4` and lists the ids; `id:N` then says
which you meant.

```
$ omacharts --watchlist Semis AMD
omacharts: 2 watchlists are called "Semis"; use id:2 or id:3
  [exit 4]

$ omacharts --watchlist id:2 AMD
  [exit 0]
```

Symbols are tickers, `NVDA` or `2330.TW`. A listing abroad takes a venue
suffix; see [Symbols and search](/manual/symbols/).

## Exit codes

| Code | Means |
|---|---|
| `0` | The command did what it says |
| `1` | Something went wrong that none of the others describe |
| `2` | The command was mistyped: an unknown flag, a missing argument or a value of the wrong kind |
| `3` | What was named does not exist |
| `4` | The name fits more than one thing; say which with `id:N` |
| `5` | The command was understood but cannot be done, such as deleting the only chartbook |
| `6` | The command meant the chart you are looking at, and no window is open |
| `7` | A bug in Omacharts stopped the command; please [report it](https://github.com/jorgemanrubia/omacharts/issues) |

```
$ omacharts watchlist addd Semis NVDA
error: unrecognized subcommand 'addd'

  tip: a similar subcommand exists: 'add'

Usage: omacharts watchlist <COMMAND>

For more information, try '--help'.
  [exit 2]

$ omacharts watchlist show Nope
omacharts: no watchlist called "Nope"; try `omacharts watchlist list`
  [exit 3]
```

A command with one bad value changes nothing; adding what is already there
exits `0`, removing what is not there exits `3`.

## Output

Data goes to stdout and errors to stderr. Every command takes `--json` and
then prints one JSON document on stdout, except `watchlist export`, which
prints JSON already.

```
$ omacharts chartbook list --json
{"chartbooks":[{"id":0,"name":"Chartbook 1","charts":2,"watchlist":1,"active":false},{"id":1,"name":"Semis","charts":1,"watchlist":2,"active":true},{"id":2,"name":"Desk","charts":4,"watchlist":1,"active":false},{"id":3,"name":"Macro","charts":1,"watchlist":1,"active":false}]}
  [exit 0]
```

## Help

| Command | Shows |
|---|---|
| `omacharts --help` | The groups and the launch options |
| `omacharts GROUP --help` | A group's commands: `omacharts chart --help` |
| `omacharts GROUP COMMAND --help` | One command's arguments, flags and an example |
| `omacharts help GROUP COMMAND` | The same as `omacharts GROUP COMMAND --help` |
| `man omacharts` | The manual page, with every command |

```
$ omacharts help watchlist add
Put symbols in a watchlist

Usage: add [OPTIONS] <LIST> <SYMBOL>...

Arguments:
  <LIST>       a name, case-insensitive, or `id:N` when two of them share one
  <SYMBOL>...  one or more tickers, such as NVDA or 2330.TW

Options:
      --section <NAME>  put them in this section rather than at the top
      --suffix <S>      venue suffix, applied to every symbol given
      --json            print the result as JSON, for scripts and agents
  -h, --help            Print help

Example:
  omacharts watchlist add Semis NVDA AMD AVGO TSM MU
  [exit 0]
```

The package installs completions for bash, zsh and fish. <kbd>Tab</kbd>
completes groups, commands and flags, and the feed names after `--provider`.

## `surface`

`omacharts surface --json` prints every command, its flags and the exit codes
as one JSON document.

| Key | Holds |
|---|---|
| `version` | The version of the binary that printed it |
| `selector` | How names are given: a name, capitals ignored, or `id:N` |
| `launch` | The launch options and the values each takes, feed names included |
| `exitCodes` | Every exit code and what it means |
| `commands` | Every command: group, description, arguments and flags with their allowed values, an example, and three facts about it: whether it changes anything (`writes`), whether it reads or changes what is on screen (`touchesWorkspace`), and whether it runs without the window (`runsInTheCaller`) |

`surface --man` writes the manual page, and `surface --completions bash`,
`zsh` or `fish` writes a completion script. They regenerate what the package
installs.

## Scripting

A file of tickers goes into a watchlist as arguments. Leave `$(cat …)`
unquoted so the shell splits it into one argument per ticker:

```sh
omacharts watchlist add Semis $(cat semis.txt)
```

A list exported from another charting tool may need tidying first (exchange
prefixes, quotes, `#` headings, Windows line endings):

```sh
omacharts watchlist add Semis $(tr -d '\r"' < export.txt \
  | tr ',;' '\n\n' | grep -v '^[[:space:]]*#' | sed 's/.*://')
```

A ticker that is not an instrument is skipped and named; the others are
added.

Over ssh, a command acts on the machine you reached, and on the window open
there:

```sh
omacharts watchlist export > watchlists.json
ssh desktop omacharts watchlist import - < watchlists.json
```

In a script, read the exit code rather than the text; see [Exit
codes](#exit-codes).

## The command groups

| Group | Covers | Commands |
|---|---|---|
| `status` | What the app has open right now | [`status show`](#status-show) |
| `symbol` | Searching the instruments | [Symbols](#symbols) |
| `watchlist` | Watchlists and the symbols in them | [Watchlists and sections](#watchlists-and-sections); `watchlist feed` under [The Omarchy plugin](#watchlist-feed) |
| `section` | The sections inside a watchlist | [Watchlists and sections](#watchlists-and-sections) |
| `chartbook` | Chartbooks | [Chartbooks](#chartbooks); `chartbook template` under [Chartbook templates](#chartbook-templates); `chartbook screenshot` under [Screenshots](#screenshots) |
| `chart` | The charts in a chartbook | [Charts](#charts) (`set`), [Indicators](#indicators), [Drawings](#drawings), [Arranging charts](#arranging-charts) (`list`, `split`, `close`, `focus`), [Linking](#linking) (`crosshair`), [Screenshots](#screenshots) |
| `template` | Chart templates | [Chart templates](#chart-templates) |
| `provider` | The data feed, and signing in to one | [Market data](#market-data) |
| `config` | Stored preferences | [Preferences](#preferences); `config bars` under [Charts](#config-bars) |
| `cache` | The downloaded prices | [Your data](#cache) |
| `backup` | Copies of your settings, and putting one back | [Your data](#backup) |
| `skill` | The agent skill | [`skill`](#skill) |
| `plugin` | The Omarchy plugin | [The Omarchy plugin](#plugin) |

## Symbols

`symbol` searches the same instruments the window searches. Which
instruments there are, and how a listing abroad is named, is in [Symbols and
search](/manual/symbols/).

### `symbol search`

Finds instruments matching a query, best first.

```
$ omacharts symbol search semi --limit 5
SEMI       Columbia Select Technology ETF     ETF      NYSE Arca
SMH        VanEck Semiconductor ETF           ETF      NASDAQ
SOXX       iShares Semiconductor ETF          ETF      NASDAQ
TSM        Taiwan Semiconductor ADR           Stock    NYSE
2330.TW    Taiwan Semiconductor Manufacturin… Stock    TWSE
  [exit 0]

$ omacharts symbol search GC --limit 3
GC         Gold                               Futures
GCOW       Pacer Global Cash Cows Dividend E… ETF      Cboe BZX
GCAL       Goldman Sachs Dynamic California … ETF      NYSE Arca
  [exit 0]
```

| Argument or flag | Takes |
|---|---|
| `QUERY` | A ticker or part of a name |
| `--limit N` | How many to show; 10 when not given |
| `--json` | The result as JSON |

Nothing matching exits 3: `omacharts: nothing matches "ZZZQX"`.

### `symbol show`

Everything known about one instrument: its ticker as charted, name, kind,
venue and currency.

```
$ omacharts symbol show SAP DE
SAP.DE
SAP
kind      Stock
exchange  XETRA
currency  EUR
  [exit 0]
```

| Argument or flag | Takes |
|---|---|
| `SYMBOL` | The ticker, or the ticker and its venue in one: `SAP.DE` |
| `SUFFIX` | The venue suffix, when not part of `SYMBOL` |
| `--json` | The result as JSON, with the symbol, suffix and local name as separate fields |

An instrument the search does not know exits 3 and says what to try.

### `--suffix` on other commands

`--suffix` names a symbol's venue on `chart set`, `watchlist add`, `watchlist
remove` and `watchlist move`. On `add` and `remove` it applies to every
symbol given. `--symbol SAP --suffix DE` and `--symbol SAP.DE` are the same.
A symbol typed in lower case is charted in capitals. One the search does not
know is refused with exit 3:

```
$ omacharts chart set --symbol zzzqx
omacharts: no instrument called ZZZQX; try `omacharts symbol search ZZZQX`
  [exit 3]
```

## Charts

`chart set` changes one chart. `config bars` and `config set` change every
chart, or what new charts open with. What each setting looks like on screen
is in [Charts](/manual/charts/).

### `chart set`

Changes what a chart shows and how. The reply names the chart and each
change.

```
$ omacharts chart set --symbol NVDA --resolution 240
chart 2: symbol NVDA, resolution 4h
  [exit 0]

$ omacharts chart set --style candles --colouring theme-hollow --session regular --grid off --auto-scale off --log-scale on
chart 4: style candles, colouring theme-hollow, session regular, grid off, auto-scale off, log scale on
  [exit 0]

$ omacharts chart set --resolution 3x
omacharts: "3x" is not a resolution; try 5m, 1h, 1D, 1W, 1M or 1Y
  [exit 2]
```

| Flag | Takes | Explained in |
|---|---|---|
| `--symbol SYMBOL` | The instrument, such as `AAPL` or `2330.TW` | [Symbols and search](/manual/symbols/) |
| `--suffix S` | Its venue suffix, for a listing abroad | [Symbols and search](/manual/symbols/#listings-abroad) |
| `--resolution TF` | A resolution as typed on the chart | [Resolutions](/manual/charts/#resolutions) |
| `--style STYLE` | `candles`, `ohlc`, or `none` to hide the bars | [Bars](/manual/charts/#bars) |
| `--bars on` or `off` | Shows the bars in the style they had, or hides them | [Hiding the bars](/manual/charts/#hiding-the-bars) |
| `--colouring C` | `theme`, `theme-hollow`, `theme-red-up`, `theme-mono` | [Colouring](/manual/charts/#colouring) |
| `--session S` | `regular`, `extended` | [Sessions](/manual/charts/#sessions) |
| `--grid on` or `off` | Draws the grid | [Gridlines](/manual/charts/#gridlines) |
| `--auto-scale on` or `off` | Fits the price axis to the visible bars, or holds it still | [Scales](/manual/charts/#scales) |
| `--log-scale on` or `off` | Logarithmic or linear price axis | [Logarithmic scale](/manual/charts/#logarithmic-scale) |
| `--link MODE` | `focus`, `follow`, `pinned` | [Linking charts to the watchlist](/manual/watchlists/#linking-charts-to-the-watchlist) |
| `--template T` | A chart template, or `none` | [Chart templates](/manual/chart-templates/) |
| `--show-drawings on` or `off`, `--send-drawings on` or `off` | Whether the chart shows other charts' drawings, and whether it sends its own | [Drawings](/manual/drawings/) |
| `--book BOOK` | Which chartbook, by name or `id:N`; the open one when not given | [Which chart a command acts on](#which-chart-a-command-acts-on) |
| `--chart CHART` | Which chart: `pos:N`, or a raw id from `chart list`; the focused one when not given | [Which chart a command acts on](#which-chart-a-command-acts-on) |
| `--json` | The result as JSON | |

`--colouring theme-hollow` on an OHLC chart is accepted and drawn coloured,
since OHLC bars have no body.

`--style none` and `--bars off` both hide the bars and keep their style.
`--bars on` brings them back in it, and `--style candles` or `--style ohlc`
shows them in that one. A style and `--bars` that disagree, such as
`--style candles --bars off`, are refused. In JSON, `style` is `none` while
the bars are hidden, `bar_style` is the style they come back in, and `bars`
says whether they are shown.

### `config bars`

Sets the colouring of every chart at once, in every chartbook and for the
charts made next, as **Apply to all charts now** does.

```
$ omacharts config bars
coloured
  [exit 0]

$ omacharts config bars monochrome
bars are monochrome; the colours are remembered
  [exit 0]

$ omacharts config bars red-up
rising bars are red and falling bars green
  [exit 0]

$ omacharts config bars coloured
bars carry their direction again, in the "theme" scheme
  [exit 0]
```

| `STATE` | Does |
|---|---|
| (none) | Prints the current state: `coloured`, `red-up` or `monochrome` |
| `monochrome` | Every chart monochrome, remembering the colouring in use |
| `red-up` | Every chart red up |
| `coloured` | Every chart back to the remembered colouring, or to `theme` when nothing is remembered |

`config set bar_scheme theme-mono` is different. It changes only what new
charts open with, and remembers nothing to go back to.

The chart defaults (`bar_style`, `bar_scheme`, `chart_session`, `show_grid`,
`chart_log_scale`, `chart_header`, `timeframes`) are set with `config set`;
see [Settings](#settings).

## Indicators

`omacharts chart indicator ACTION [KIND]` changes a chart's indicators.
`--book` and `--chart` pick the chart ([Which chart a command acts
on](#which-chart-a-command-acts-on)). `KIND` is the indicator's name as the
command takes it, such as `sma`, `rsi`, `volume_profile` or `session_times`.
Each indicator's name, what it draws, and every parameter's default and range
are in [Indicators](/manual/indicators/) and the page of its family.

| Action | Does |
|---|---|
| `list` | One line per indicator, with its lengths: `SMA(50)`, `VWAP · week`, `BB(20,2.5)`. With `--json`, each one's id, kind, parameters, colour, line, visibility, and `in_legend`, whether the legend names it |
| `add KIND` | Adds one, at its defaults plus whatever flags are given |
| `remove KIND` | Removes every indicator of that kind |
| `set KIND` | Changes the one of that kind. With two of a kind, `--id N` says which |

| Flag | Takes | For |
|---|---|---|
| `--id N` | An id from `list --json` | `set`, on a chart with two of a kind |
| `--period N` | Bars | SMA, EMA, Bollinger, RSI, ATR; the stochastic's %K length |
| `--deviations F` | 0 to 50 | Bollinger |
| `--fast N`, `--slow N`, `--signal N` | Bars | MACD |
| `--signal-color C` | A colour | MACD |
| `--k-smooth N`, `--d-period N` | Bars | Stochastic |
| `--d-color C` | A colour | Stochastic |
| `--anchor P` | `session`, `week`, `month`, `quarter`, `year` | VWAP, volume profile |
| `--rows N` | `auto`, or a number of rows per period, held between 4 and 400 | Volume profile |
| `--value-area F` | 0 to 1; 0.70 when not given | Volume profile |
| `--poc-color C` | A colour | Volume profile |
| `--up-color C`, `--down-color C` | A colour | Volume |
| `--overbought F`, `--oversold F` | 0 to 100 | RSI, stochastic |
| `--height F` | 0.05 to 0.95; Volume defaults to 0.18, the other strips to 0.16 | Volume, RSI, MACD, ATR, stochastic |
| `--bands LIST` | `1,2,3` or any of them; `none` | VWAP; `1` or `none` for Bollinger |
| `--band-alpha F` | 0 to 1; 0.16 when not given | VWAP, Bollinger |
| `--add-range`, `--remove-range`, `--range`, `--name`, `--from`, `--to`, `--zone`, `--shade-alpha` | See [Session Times](#session-times) | Session Times |
| `--color C` | A colour | The line of any indicator; a profile's bars; a Session Times range's shading |
| `--width F` | Pixels; `0` draws no line | Any indicator with a line |
| `--style S` | `solid`, `dashed`, `dotted` | Any indicator with a line |
| `--visible B` | `on`, `off` | Any |
| `--legend B` | `on`, `off`; `on` for a new indicator, `off` for Session Times | Any. Whether the chart's legend names it. It is drawn either way |

A colour is a palette name (`Blue`, `Amber`, `Violet`, `Teal`, `Rose`,
`Green`, `Orange`, `Cyan`, capitals ignored) or `#rrggbb`. A palette name follows
the theme; a hex does not.

Volume and a volume profile are bars, not lines, so `--width` and `--style`
change nothing on them. `--color` on volume colours only its dot in the
legend.

`--bands` chooses which bands are drawn, `--band-alpha` how solid every
band's shading is, and `--deviations` where a Bollinger's bands sit. A band's
colour and edges, and a VWAP band's multiple, are set only in the window.

The CLI accepts wider ranges than the dialog's sliders in places. Standard
deviations run 0.1 to 6 in the dialog and 0 to 50 in `--deviations`; the
value area 10 to 100% there and 0 to 1 in `--value-area`.

Examples:

```sh
omacharts chart indicator add sma --period 200 --color Amber
omacharts chart indicator add ema --period 9
omacharts chart indicator add vwap --bands 1,2 --band-alpha 0.3
omacharts chart indicator add bollinger --deviations 2.5
omacharts chart indicator add volume_profile --poc-color Amber
omacharts chart indicator add volume --height 0.25
omacharts chart indicator set rsi --period 21 --overbought 80
omacharts chart indicator add macd --signal-color Amber
omacharts chart indicator add atr --period 20
omacharts chart indicator add stochastic --k-smooth 5
```

```
$ omacharts chart indicator add sma --period 200
added SMA(200) on pos:0 SPY 1D in "Chartbook 1", in chart template "Default"
  [exit 0]
$ omacharts chart indicator add sma --period 50 --style dashed
added SMA(50) on pos:0 SPY 1D in "Chartbook 1", in chart template "Default"
  [exit 0]
$ omacharts chart indicator set sma --period 100
omacharts: this chart's template has 2 of them; say which with --id 3 or 4
  [exit 4]
$ omacharts chart indicator set sma --id 4 --period 100 --color Amber
set SMA(100) on pos:0 SPY 1D in "Chartbook 1", in chart template "Default"
  [exit 0]
```

Refusals:

| Refused | Exit | Message |
|---|---|---|
| A parameter the kind has no use for: `add volume_profile --period 5` | 2 | `VP has no period` |
| A band the kind has not got: `add bollinger --bands 2` | 2 | `BB has 1 band, so 2 is not one of them` |
| `--deviations` on a VWAP: `set vwap --deviations 2` | 2 | `--deviations sets the one band a Bollinger has; VWAP has 3 of them` |
| `--fast` not below `--slow`, in one command or against the length the chart already holds | 2 | `--fast 26 is not below --slow 12` |
| `--oversold` not below `--overbought` | 2 | `oversold 80 is not below overbought 70` |
| A length of 0, or not a whole number | 2 | `--period takes a whole number of bars, one or more` |
| A figure outside its range | 2 | `--height is 0.99, which is outside 0.05 to 0.95` |
| A colour that is not a palette name or a hex | 2 | The message lists the names |
| `set` with nothing to change | 2 | |
| `set` or `remove` of a kind the chart has not got | 3 | `this chart has no RSI` |
| `set` on two of a kind without `--id` | 4 | The message names the ids |

A chart template's indicators take the same actions and flags, with the
template named before the kind: `omacharts template indicator add Momentum
rsi --period 14`. See [Chart templates](#chart-templates).

### Session Times

`session_times` is [Session Times](/manual/indicators/session-times/), a
list of ranges shaded behind the bars. Ranges are numbered from 1, in the
order `list --json` gives them. These flags change the list, and any other
indicator refuses them.

| Flag | Takes | Does |
|---|---|---|
| `--add-range R` | `eth`, `rth`, `asia`, `london`, `new_york`, `custom` | Adds a range at its preset. The flags that follow in the same command change it |
| `--remove-range N` | A range's number | Removes it, before anything else the command does |
| `--range N` | A range's number | Which range the other flags change |
| `--name TEXT` | Words | The range's name, which the legend lists |
| `--from TIME`, `--to TIME` | A time of day, written any way the editor takes: `9am`, `0930`, `21h30`, `1:30pm` | The range's hours, in the chart's time zone unless it has one of its own. `--to` before `--from` runs past midnight. ETH and RTH have none |
| `--zone ZONE` | An IANA name such as `Europe/London`, or `chart` for the chart's own time zone | The clock the range's hours keep |
| `--color C` | A colour | The range's shading. Every range's, when none is named |
| `--shade-alpha F` | 0.02 to 0.6 | How solid the shading is. Every range's, when none is named |

With one range, the flags change it without `--range`. With more, `--name`,
`--from`, `--to` and `--zone` need `--range`, or come straight after
`--add-range`.

```sh
omacharts chart indicator add session_times
omacharts template indicator set Default session_times --add-range london
omacharts template indicator set Default session_times --add-range custom --name Lunch --from 12pm --to 1:30pm
omacharts template indicator set Default session_times --range 2 --color Blue --shade-alpha 0.1
omacharts template indicator set Default session_times --remove-range 3
omacharts template indicator set Default session_times --legend on
```

```
$ omacharts template indicator list Default
Vol
Sessions · ETH
  [exit 0]
$ omacharts template indicator set Default session_times --add-range london
set Sessions · ETH, London in chart template "Default"
  [exit 0]
$ omacharts template indicator set Default session_times --add-range custom --name Lunch --from 12pm --to 1:30pm
set Sessions · ETH, London, Lunch in chart template "Default"
  [exit 0]
$ omacharts template indicator set Default session_times --range 2 --color Blue --shade-alpha 0.1
set Sessions · ETH, London, Lunch in chart template "Default"
  [exit 0]
```

Refusals:

| Refused | Exit | Message |
|---|---|---|
| These flags on another indicator: `add sma --add-range london` | 2 | `SMA has no ranges` |
| ETH or RTH a second time: `--add-range eth` | 2 | `ETH is already shaded; a second would shade the same bars twice` |
| A range that is not there: `--range 9` | 2 | `` there is no range 9; this has 2 (see `chart indicator list`) `` |
| Hours or a name with several ranges and none named: `--from 9am` | 2 | `this has 2 ranges; say which with --range N` |
| Hours on ETH or RTH: `--range 1 --from 9am --to 10am` | 2 | `ETH follows the instrument's exchange, so it has no hours of its own` |
| A time that is not one: `--from 25:00` | 2 | `--from takes a time of day such as 9am, 09:30 or 2130, not "25:00"` |
| A zone that is not one: `--zone Mars/X` | 2 | `` "Mars/X" is not a time zone; use an IANA name such as Europe/London, or `chart` `` |

## Chart templates

A chart template is a named set of indicators that charts share. Change the
template and every chart on it changes. How a chart follows a template is in
[Chart templates](/manual/chart-templates/).

| Command | Does |
|---|---|
| `omacharts template list` | Every template: its id, name, how many charts use it, and what it holds. `*` marks the one new charts open with |
| `omacharts template create NAME` | Makes an empty template |
| `omacharts template create NAME --from-chart` | Makes a template holding the focused chart's indicators, and puts the chart on it. `--book` and `--chart` name another chart. Refused on a chart with no indicators |
| `omacharts template rename TEMPLATE NAME` | Renames one; its charts stay on it |
| `omacharts template delete TEMPLATE` | Deletes one; its charts keep its indicators as their own |
| `omacharts template indicator ACTION TEMPLATE [KIND]` | `list`, `add`, `remove` or `set` the template's indicators, with every flag `chart indicator` takes ([Indicators](#indicators)). Every chart on it follows |
| `omacharts template default [TEMPLATE]` | Reads, or sets, the template new charts open with; `none` for none |
| `omacharts chart set --template TEMPLATE` | Puts a chart on a template, or on `none` |
| `omacharts config set chart_template TEMPLATE` | The same setting `template default` writes: a name, `id:N`, or `none` |

`TEMPLATE` is a name or `id:N` ([Naming things](#naming-things)); two
templates may share a name. `chart set --template` replaces the chart's
indicators without asking.

```
$ omacharts template create Swing
created chart template "Swing" (id:3)
  [exit 0]
$ omacharts template indicator add Swing rsi --period 14
added RSI(14) in chart template "Swing"
  [exit 0]
$ omacharts template indicator add Swing macd --signal-color Amber
added MACD(12,26,9) in chart template "Swing"
  [exit 0]
$ omacharts chart set --template Swing
chart 33: chart template "Swing"
  [exit 0]
$ omacharts template list
* id:1  Default              0 charts  Volume
  id:2  Momentum             0 charts  RSI 14, MACD 12 26 9
  id:3  Swing                1 chart   RSI 14, MACD 12 26 9
  [exit 0]
$ omacharts template indicator set Swing rsi --overbought 80
set RSI(14) in chart template "Swing", and on the chart using it
  [exit 0]
$ omacharts template default Swing
new charts open with chart template "Swing"
  [exit 0]
```

On a chart that is on a template, adding, removing, hiding, reordering or
reconfiguring an indicator changes the template, and every chart on it, the
same as the window does. The reply names the template and how many other
charts followed:

```
$ omacharts chart indicator add sma --period 200
added SMA(200) on pos:0 SPY 1D in "Chartbook 1", in chart template "Default", and on the other chart using it
  [exit 0]
```

`template delete` on a name two templates share exits `4`. Deleting the
default template leaves its charts their indicators, and new charts open with
no template:

```
$ omacharts template create Swing
created chart template "Swing" (id:4)
  [exit 0]
$ omacharts template delete Swing
omacharts: 2 chart templates are called "Swing"; use id:3 or id:4
  [exit 4]
$ omacharts template delete id:4
deleted chart template "Swing"
  [exit 0]
$ omacharts template delete Swing
deleted chart template "Swing"; the chart using it keeps its indicators as its own
  [exit 0]
$ omacharts template default
new charts open with no chart template
  [exit 0]
$ omacharts chart set --template none
chart 37: no chart template, and no indicators
  [exit 0]
$ omacharts chart set --template Nope
omacharts: no chart template called "Nope"; see `omacharts template list`
  [exit 3]
```

`config get chart_template` prints the template's id, or `none`. `template
default` with no argument says it by name.

`template create --from-chart` keeps what a chart shows as a new template and
puts the chart on it. A chart with no indicators is refused:

```
$ omacharts template create Trend --from-chart
omacharts: pos:0 SPY 1D has no indicators to make a template from; `omacharts template create Trend` makes an empty one
  [exit 5]
$ omacharts chart indicator add rsi
added RSI(14) on pos:0 SPY 1D in "Chartbook 1"
  [exit 0]
$ omacharts template create Trend --from-chart
created chart template "Trend" (id:5) from the indicator on pos:0 SPY 1D in "Chartbook 1", and put the chart on it
  [exit 0]
```

## Drawings

`omacharts chart drawing ACTION [KIND]` changes the drawings on a chart's
symbol. `--book` and `--chart` pick the chart ([Which chart a command acts
on](#which-chart-a-command-acts-on)). `KIND` is `line`, `hline`, `arrow`,
`zigzag`, `rect`, `ellipse` (or `circle`), or `text`. The tools and their
configurations are in [Drawings](/manual/drawings/). Whether a chart shows
other charts' drawings and sends its own is `chart set --show-drawings` and
`--send-drawings`, each `on` or `off`:

```
$ omacharts chart set --send-drawings off
chart 46: sends drawings to other charts off
  [exit 0]
```

| Action | Does |
|---|---|
| `list` | Every drawing the chart shows: its id, kind, anchors, words and look. With `--json`, the symbol, whether the chart shows and sends drawings, and each drawing's anchors, corners, configuration, style, scope, order and words with their emphasis |
| `add KIND` | Draws one |
| `set --id N` | Changes one: anchors, look, words, scope or order |
| `remove --id N` | Removes one |
| `clear` | Removes every drawing of the symbol, and the chart's own |
| `configs [KIND]` | The nine configurations of a kind, or of every kind; `(edited)` marks one changed |
| `configure KIND --config N` | Changes configuration N of a kind, with the look flags |
| `reset-configs [KIND]` | Puts a kind's nine back as shipped, or every kind's |

An anchor is `WHEN,PRICE`: a date (`2026-09-01`), a date and time
(`2026-09-01T14:30`) or unix seconds, then the price. A date or a time is
read in the chart's time zone, the one its time axis is labelled in
([Time](/manual/preferences/#time)), and `list` prints them the same way.

| Flag | Takes | Does |
|---|---|---|
| `--id N` | An id from `list` | Which drawing, for `set` and `remove` |
| `--from ANCHOR`, `--to ANCHOR` | Anchors | The two ends, or opposite corners. A text drawing takes `--from` only |
| `--point ANCHOR` | An anchor, repeated | A zig-zag's corners in order, two at least. `--from` and `--to` alone give it one leg |
| `--config N` | 1 to 9 | The configuration it follows; for `configure`, the one to change |
| `--color C` | A colour | A line's colour; a box's or circle's edge |
| `--width F` | 0.5 to 12 | A line's thickness; an edge's |
| `--arrow WHERE` | `none`, `end`, `start`, `both` | Which ends have an arrowhead |
| `--head SHAPE` | `filled`, `open`, `barb` | The arrowhead |
| `--border B` | `on`, `off` | Whether a box or circle has an edge |
| `--fill C` | A colour | A box's or circle's fill |
| `--alpha F` | 0 to 1 | How much of the fill shows: 0 none, 1 solid |
| `--text WORDS` | Words; `""` | The whole of a text drawing; a label on any other. The empty string removes a label. `**bold**`, `*italic*` and `***both***` are read as emphasis, `\*` as an asterisk |
| `--text-at WHERE` | `center`, `top`, `bottom`, `left`, `right`, `top-left`, `top-right`, `bottom-left`, `bottom-right` | Where a figure's label sits; `center` by default |
| `--text-color C` | A colour | The words' colour |
| `--font FAMILY` | A family; `system` | The words' font. A family this machine lacks falls back to the system's |
| `--font-size F` | 6 to 96 | The words' size in pixels |
| `--scope S` | `shared`, `local` | Whether the symbol's other charts see it. Defaults to what the chart sends |
| `--order WHERE` | `front`, `back` | Brings it to the front or sends it to the back |

A colour is one of the presets `up`, `down`, `blue`, `amber`, `violet`,
`teal`, `orange`, `cyan` and `ink`, which follow the theme, or `#rrggbb`,
which does not. Any of `--color`, `--width`, `--arrow`, `--head`,
`--border`, `--fill`, `--alpha`, `--text-color`, `--font` and `--font-size`
gives the drawing a look of its own, off its configuration; `--text` and
`--text-at` do not. A `local` drawing has a negative id. Changing a drawing's
scope gives it a new id.

```
$ omacharts chart drawing add line --from 2026-08-01,170 --to 2026-10-01,190 --config 4
drew #1 line  2026-08-01 170.00 → 2026-10-01 190.00  configuration 4 · shared on pos:0 NVDA 1D in "Chartbook 1"
  [exit 0]
$ omacharts chart drawing add rect --from 2026-09-08,178 --to 2026-09-12,186 --fill down --alpha 0.25
drew #4 rect  2026-09-08 178.00 → 2026-09-12 186.00  fill down at 0.25, edge up 1px · shared on pos:0 NVDA 1D in "Chartbook 1"
  [exit 0]
$ omacharts chart drawing set --id 1 --text supply --text-at top
set #1 line  2026-08-01 170.00 → 2026-10-01 190.00  "supply"  configuration 4 · shared on pos:0 NVDA 1D in "Chartbook 1"
  [exit 0]
$ omacharts chart drawing set --id 1 --color ink --width 2.5 --arrow end --head barb
set #1 line  2026-08-01 170.00 → 2026-10-01 190.00  "supply"  ink, 2.5px, barb arrow at the end · shared on pos:0 NVDA 1D in "Chartbook 1"
  [exit 0]
$ omacharts chart drawing add line --from 2026-08-01,170 --to 2026-10-01,190 --scope local
drew #-1 line  2026-08-01 170.00 → 2026-10-01 190.00  configuration 1 · local on pos:0 NVDA 1D in "Chartbook 1"
  [exit 0]
```

```
$ omacharts chart drawing configure line --config 3 --color teal --arrow end
Line configuration 3 is now teal, 1.5px, arrow at the end
  [exit 0]
$ omacharts chart drawing reset-configs line
restored the default line configurations
  [exit 0]
```

Refusals:

| Refused | Exit | Message |
|---|---|---|
| A text drawing with no words, on `add` or `set` | 2 | `` `drawing add text` needs --text: a text drawing with nothing to say draws nothing `` |
| `--point` on anything but a zig-zag | 2 | `--point is a zig-zag's corners; every other kind is --from and --to` |
| A zig-zag with one `--point` | 2 | `a zig-zag needs at least two --point corners` |
| An anchor that is not `WHEN,PRICE` | 2 | `--from takes WHEN,PRICE — like 2026-09-01,180.5 or 2026-09-01T14:30,180.5` |
| A colour that is not a preset or a hex | 2 | The message lists the presets |
| A figure outside its range | 2 | `--width is 20, which is outside 0.5 to 12` |
| `set` with nothing to set | 2 | `nothing to set: give --from, --to, --point, --config, --scope, --order, --text, or a property` |
| An id that is not there | 3 | `no drawing 99 on NVDA` |
| A chart with no symbol | 5 | `that chart has no symbol to draw on` |

## Arranging charts

`chart split`, `chart close`, `chart focus` and `chart list` arrange the
charts of a chartbook ([Splitting and arranging charts](/manual/layouts/)).
`--book` and `--chart` pick the chartbook and chart ([Which chart a command
acts on](#which-chart-a-command-acts-on)). A chartbook that is not open can
be changed too; the change is there when you open it.

Divider positions, and the heights of indicator strips dragged within a
chart, are saved with the chartbook. Neither has a command.

### `chart split`

```
$ omacharts chart split horizontal
split chart 6 horizontally; the new chart is 7
  [exit 0]
```

| Argument or flag | Takes |
|---|---|
| `DIRECTION` | `horizontal` (side by side) or `vertical` (stacked) |
| `--chart CHART` | The chart to split; the focused one when not given |
| `--book BOOK` | The chartbook; the open one when not given |
| `--json` | The result as JSON |

### `chart close`

```
$ omacharts chart close --chart pos:1
closed chart 13
  [exit 0]

$ omacharts chart close
omacharts: this is the only chart in the chartbook; there has to be one
  [exit 5]
```

Takes `--chart`, `--book` and `--json`.

### `chart focus`

Makes a chart the one the keyboard, the menus and later commands act on.

```
$ omacharts chart focus pos:0
focused pos:0 NVDA 10m
  [exit 0]

$ omacharts chart focus pos:7
omacharts: this chartbook has 3 charts, so there is no pos:7
  [exit 3]
```

| Argument or flag | Takes |
|---|---|
| `CHART` | `pos:N`, or a raw id from `chart list` |
| `--book BOOK` | The chartbook; the open one when not given |
| `--json` | The result as JSON |

### `chart list`

Lists the charts in a chartbook: raw id, symbol, resolution, bar style
(`none` while the bars are hidden) and link mode, with `*` on the focused one. `--chart` takes these raw ids. The
list is by id, not layout order; `status show` gives positions.

```
$ omacharts chart list
Desk — watchlist "Default"
* 159  NVDA         1D     candles  follow
  160  AMD          1h     candles  follow
  161  AMD          15m    candles  follow
  [exit 0]
```

`--book` lists another chartbook. `--json` gives each chart in full:
colouring, session, gridlines, both scales, whether it shows and sends
drawings, indicators and template.

## Chartbooks

A chartbook is a saved screen: its charts, how they are split, and the
watchlist beside them. The strip and what a chartbook keeps are in
[Chartbooks](/manual/chartbooks/).

Names and `id:N` work as under [Naming things](#naming-things); a chartbook's
`N` is its place in the strip, counting from 0. A place changes when a
chartbook before it is removed or a tab is dragged, so a script should use
names.

| Command | Does |
|---|---|
| `chartbook list` | Every chartbook, its number of charts and its watchlist; `*` marks the open one. |
| `chartbook show [BOOK]` | One chartbook's charts: id, symbol, resolution, bar style and link mode; `*` marks the focused chart. Default: the open one. |
| `chartbook create NAME` | A chartbook with one chart, showing the default watchlist. `--symbol SYMBOL` (default `SPY`), `--template TEMPLATE` ([Chartbook templates](#chartbook-templates)), `--switch` to open it. Give it another list with `chartbook watchlist`. |
| `chartbook rename BOOK NAME` | Renames it. |
| `chartbook delete BOOK` | Removes it and its charts. The only chartbook is refused, exit `5`. |
| `chartbook switch BOOK` | Opens it. |
| `chartbook watchlist BOOK LIST` | Gives it a watchlist. A list another chartbook shows is refused, exit `5`, naming the book. |
| `chartbook duplicate [BOOK]` | Copies it beside itself. `--name NAME` (default: the name and *copy*), `--switch` to open the copy. Default: the open one. |

<!-- TODO(manual): `chartbook create … --watchlist LIST` is in the surface
but refused with exit 2 at v0.1.22, so the table above leaves it out: the launch option `--watchlist` is peeled off the
command line before the command sees it ("--watchlist says which list to
open a symbol in; it does not apply to commands"). The examples below use
`chartbook watchlist` instead. doc/cli.md's example has the same problem. -->

```
$ omacharts chartbook create Semis --symbol NVDA --switch
created chartbook "Semis" (id:1) showing NVDA
  [exit 0]

$ omacharts chartbook watchlist Semis Semis
chartbook "Semis" now shows watchlist "Semis"
  [exit 0]

$ omacharts chartbook list
  id:0 Overview                  1 charts  watchlist "Default"
* id:1 Semis                     1 charts  watchlist "Semis"
  [exit 0]

$ omacharts chartbook create Macro
created chartbook "Macro" (id:2) showing SPY
  [exit 0]

$ omacharts chartbook watchlist Macro Semis
omacharts: chartbook "Semis" already shows "Semis"; a watchlist belongs to one
  [exit 5]

$ omacharts chartbook duplicate Semis --name "Semis weekly"
duplicated chartbook "Semis" as "Semis weekly" (id:2)
  [exit 0]
```

## Chartbook templates

A chartbook template is a chartbook's arrangement saved under a name: the
splits, and each chart's resolution and indicators, without the symbols.
Chartbooks on the same template change together. What following one means
is in [Chartbook templates](/manual/chartbook-templates/).

Names and `id:N` work as under [Naming things](#naming-things); the ids are in
`chartbook template list` and do not change. `--book BOOK` picks the
chartbook for `save` and `apply`; the default is the open one.

| Command | Does |
|---|---|
| `chartbook template list` | Every template, how many charts it lays out and how many chartbooks use it; `*` marks the open chartbook's. |
| `chartbook template save NAME` | Saves the chartbook as a new template and puts it on it. A name that is blank, already a template's (capitals ignored), `None`, or starts with `id:` is refused, exit `2`. |
| `chartbook template apply TEMPLATE` | Puts the chartbook on the template. `none` takes it off and keeps everything. |
| `chartbook template rename TEMPLATE NAME` | Renames it. |
| `chartbook template delete TEMPLATE` | Deletes it; the chartbooks on it keep what they have. |
| `chartbook create NAME --template TEMPLATE` | A new chartbook on the template from the start, every chart on `--symbol`. |

A change made by a command counts the same as one made in the window. Where
the window asks before replacing an arrangement no template holds, a command
does not ask.

```
$ omacharts chartbook template save Desk
saved chartbook "Semis" as new chartbook template "Desk" (3 charts)
  [exit 0]

$ omacharts chartbook template save desk
omacharts: there is already a chartbook template called "Desk"
  [exit 2]

$ omacharts chartbook create Metals --symbol GLD --template Desk --switch
created chartbook "Metals" (id:2) showing GLD, on chartbook template "Desk": 3 charts
  [exit 0]

$ omacharts chartbook template list
* id:1  Desk                 3 charts  2 chartbooks
  [exit 0]

$ omacharts chartbook template apply none --book Metals
chartbook "Metals" is on no chartbook template; it keeps its layout and indicators
  [exit 0]
```

`chartbook duplicate` keeps the original's template, so the copy is on it
too; see [Chartbooks](/manual/chartbooks/#duplicating-a-chartbook).

## Linking

A chart's link mode says whether it follows the symbol picked in the
watchlist. What each mode does is in [Linking charts to the
watchlist](/manual/watchlists/#linking-charts-to-the-watchlist).

`chart set --link MODE` takes `focus`, `follow` or `pinned`. New charts are `focus`. `chart list`
prints each chart's mode in its last column.

```
$ omacharts chart set --chart pos:1 --link pinned --symbol SPY
chart 68: symbol SPY, link pinned
  [exit 0]

$ omacharts chart set --chart pos:2 --link focus
chart 71: link focus
  [exit 0]

$ omacharts chart set --chart pos:2 --link follow
chart 79: link follow; 2 other Follow charts now show AMD
  [exit 0]
```

`chart set --symbol` on a Follow chart changes that one chart only. To put a
symbol on every Follow chart, click it in the sidebar, type it in the search,
or run `omacharts SYMBOL`.

`omacharts SYMBOL` exits `5` when the focused chart is pinned and no chart
follows ([Launching the app](#launching-the-app)):

```
$ omacharts NVDA
omacharts: NVDA not shown; the focused chart is pinned and no chart follows the watchlist
  [exit 5]
```

### `chart crosshair`

| Command | Does |
|---|---|
| `chart crosshair` | Whether the crosshair is shared across Follow charts; prints `on` or `off`. |
| `chart crosshair on` or `off` | Turns it on or off. |
| `config set sync_crosshair on` or `off` | The same setting, by its key; see [Preferences](#preferences). |

## Watchlists and sections

Names and `id:N` work as under [Naming things](#naming-things). A command
with no list given acts on the default watchlist.

| Command | Does |
|---|---|
| `watchlist list` | Every watchlist, with its id and how many symbols it holds. |
| `watchlist show [LIST]` | Its sections and symbols. |
| `watchlist create NAME` | An empty watchlist. |
| `watchlist rename LIST NAME` | Renames it. |
| `watchlist delete LIST` | Deletes it and everything in it. The default is refused, exit `5`. |
| `watchlist add LIST SYMBOL…` | Puts symbols in it, at the end. `--section NAME` puts them in a section rather than at the top; `--suffix S` applies a venue suffix to every symbol given ([Symbols](#symbols)). A ticker that is not an instrument is skipped and named; when none is, exit `3`. |
| `watchlist remove LIST SYMBOL…` | Takes symbols out of the top of the list, or with `--section NAME` out of that section. `--suffix S` as for `add`. |
| `watchlist move LIST SYMBOL` | Moves one symbol. `--section SECTION` into another section, `--before SYMBOL` in front of another, or both; `--from SECTION` when it is in two; `--suffix S`. |
| `watchlist sort LIST [COLUMN]` | Sorts it within each section by `symbol`, `last` or `change_pct`, low to high, or high to low with `--descending`; `none` for its own order. With no column, prints the sort. |
| `watchlist export [LIST…]` | Writes watchlists as JSON to stdout. Default: every one. |
| `watchlist import FILE` | Brings in an export, adding what is missing. `-` reads stdin; `--replace` makes each list exactly as exported. |
| `section list [LIST]` | Its sections, with their ids and counts. The symbols in no section are `(no section)`. |
| `section create LIST NAME` | Adds a section, last. |
| `section rename LIST SECTION NAME` | Renames it. |
| `section delete LIST SECTION` | Removes it and the symbols in it. |
| `section promote LIST SECTION` | Turns it into a watchlist of its own, carrying its symbols. |
| `section order LIST SECTION…` | Puts the sections in this order, first to last; any left out keep their order behind them. |

A chartbook's watchlist is set with `chartbook watchlist`
([Chartbooks](#chartbooks)).

### Making and filling a list

```
$ omacharts watchlist create Semis
created watchlist "Semis" (id 2)
  [exit 0]

$ omacharts watchlist add Semis NVDA AMD AVGO
added 3 to "Semis": NVDA AMD AVGO
  [exit 0]

$ omacharts section create Semis Memory
created section "Memory" in "Semis"
  [exit 0]

$ omacharts watchlist add Semis MU WDC --section Memory
added 2 to "Semis": MU WDC
  [exit 0]

$ omacharts watchlist show Semis
Semis

  (no section)
    NVDA
    AMD
    AVGO

  Memory
    MU
    WDC
  [exit 0]

$ omacharts watchlist add Semis XYZNOPE
omacharts: none of those are instruments this knows: XYZNOPE
  [exit 3]
```

A file of tickers goes in the same way; see [Scripting](#scripting).

### Putting a list in order

The symbols in no section have no header and cannot be ordered; naming them
is refused.

```
$ omacharts section order Semis Equipment
sections of "Semis": Equipment, Memory
  [exit 0]

$ omacharts watchlist add Semis TSM --section Memory
added 1 to "Semis": TSM
  [exit 0]

$ omacharts watchlist move Semis TSM --section Equipment --before AMAT
moved TSM from Memory to Equipment in "Semis", in front of AMAT
  [exit 0]

$ omacharts watchlist move Semis NVDA
omacharts: nowhere to move it to: name a --section, a --before, or both
  [exit 2]

$ omacharts watchlist add Semis NVDA --section Memory
added 1 to "Semis": NVDA
  [exit 0]

$ omacharts watchlist move Semis NVDA --section Equipment
omacharts: NVDA is in 2 sections of "Semis"; say which with --from id:5 or --from id:6
  [exit 4]
```

Without `--before`, a moved symbol lands at the end of its section. Without
`--section`, it stays in the section it is in.

### Sorting

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

### Turning a section into a watchlist

```
$ omacharts section promote Default Energy
turned section "Energy" into a watchlist with its 2 symbols
  [exit 0]
```

The default watchlist cannot be deleted:

```
$ omacharts watchlist delete Default
omacharts: "Default" is the watchlist the bar widget shows and cannot be deleted; rename it instead
  [exit 5]
```

### Moving watchlists between machines

`watchlist export` writes watchlists, with their sections and symbols, as one
JSON file. Copy it to the other machine and read it there with `watchlist
import`.

```
$ omacharts watchlist export > watchlists.json
$ omacharts watchlist export Semis Metals > some.json

$ omacharts watchlist import some.json
Semis: already up to date
Metals: added 1 symbol
  [exit 0]
```

Importing adds and never removes. Lists are matched by name (the default
list to the default list, whatever it is called); a matched list gains the
sections and symbols it lacks, after what it has, and a list with no match is
created. Importing the same file again changes nothing.

`--replace` makes each watchlist in the file exactly what it was where it
was exported, sections and order included:

```
$ omacharts watchlist import --replace some.json
Semis: replaced, 6 symbols in 1 section
Metals: replaced, 3 symbols in 0 sections
  [exit 0]
```

Neither form touches a watchlist the file does not name. A file that is not
an export (exit `2`), one made by a newer Omacharts, or one naming a list
that is ambiguous here changes nothing.

`-` reads the file from stdin; see [Scripting](#scripting).

An export is plain JSON:

```json
{
  "omacharts": "watchlists",
  "version": 1,
  "watchlists": [
    {
      "name": "Semis",
      "sections": [
        { "name": "", "symbols": ["NVDA", "AMD", "AVGO"] },
        { "name": "Memory", "symbols": ["MU", "WDC"] }
      ]
    }
  ]
}
```

The section with an empty name holds the symbols in no section.

## Screenshots

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
[Settings](#settings).

## Market data

`config set provider NAME` stores the feed ([Market data
feeds](/manual/market-data/)); the names are in `provider list`.
A name that is not a feed is refused with exit 2. `omacharts --provider NAME`
uses a feed for one launch without changing the stored setting ([Launching
the app](#launching-the-app)). The feed in use is `--provider` when it was
given, otherwise the stored setting, otherwise Yahoo.

thinkorswim signs in through Chromium, Chrome, Brave or Edge. If the app does
not find your browser on its own, set `TOS_BROWSER` to its path.

### `provider login` and `provider logout`

`provider login` signs in to a feed, as **Sign in…** in Preferences does. It
waits in the terminal while you sign in and prints the browser's progress on
stderr. `provider logout` signs out. A session lasts until thinkorswim ends
it; `provider status` says when that happened. Signing in again asks for no
new code while thinkorswim still trusts the browser you signed in with.

```
$ omacharts provider login
a browser is opening at thinkorswim; sign in there
…
signed in to thinkorswim
  [exit 0]

$ omacharts provider logout tos
no saved thinkorswim session to forget
  [exit 0]
```

Both take a feed's name and default to the feed in use. `provider logout
yahoo` is refused with exit 5; Yahoo has no sign-in.

### `provider list` and `provider status`

`provider list` shows every feed with what is stored and what is running.
The two differ only when the launch was given `--provider`.

```
$ omacharts provider list
yahoo    Yahoo Finance · Every listing the symbol search covers · delayed 15 min for indexes, 10 for futures   (in use)
tos      thinkorswim · US listings, from your thinkorswim account · live, as each print arrives · experimental   (not signed in)
  [exit 0]
```

`provider status` describes the feed in use: whether you are signed in (and
when a session expired) and, for a streamed feed with a window open, what is
streaming. While the market is shut, the line says a series has nothing new.

```
$ omacharts provider status
Yahoo Finance · Every listing the symbol search covers · delayed 15 min for indexes, 10 for futures
nothing to sign in to; it works as it is
delivery: polled — charts are refetched on a timer
  [exit 0]

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

With `--json` the same facts are fields: `delivery`, `subscriptions`, and
for `provider list` `stored`, `inUse`, `needsSignIn` and `session` per feed.

## Preferences

`config list` prints every setting that has been written, `config get KEY`
one of them, and `config set KEY VALUE` writes one. A setting you have never
changed does not appear; asking for it exits 3.

```
$ omacharts config get show_grid
omacharts: "show_grid" has never been set
  [exit 3]

$ omacharts config set show_grid off
set show_grid to off
  [exit 0]

$ omacharts config get cache_limit --json
{"key":"cache_limit","value":"1073741824"}
  [exit 0]
```

`config list` also lists `workspace` and other entries that are not settings;
`config set` on them is refused. `config set chart_templates` exits `2` and
points at `omacharts template`.

### Settings

Switches take `on` or `off`; `true`/`false`, `yes`/`no` and `1`/`0` mean the
same.

| Setting | Values | Default | Preferences row | Chapter |
|---|---|---|---|---|
| `theme` | `omarchy` (System), `midnight`, `carbon`, `paper`, `daylight`, `omarchy:SLUG` for one installed Omarchy theme, or the id of a theme of your own | `omarchy` on Omarchy, `midnight` elsewhere | Theme › Theme | [Themes](/manual/themes/) |
| `bar_style` | `candles`, `ohlc` | `candles` | Charts › Defaults › Style | [Chart settings](/manual/chart-settings/) |
| `bar_scheme` | `theme` (filled), `theme-hollow`, `theme-red-up`, `theme-mono` | `theme` | Charts › Defaults › Colouring | [Chart settings](/manual/chart-settings/) |
| `chart_session` | `extended`, `regular` | `extended` | Charts › Defaults › Hours | [Chart settings](/manual/chart-settings/) |
| `show_grid` | on, off | on | Charts › Defaults › Gridlines | [Chart settings](/manual/chart-settings/) |
| `chart_log_scale` | on, off | off | Charts › Defaults › Logarithmic scale | [Chart settings](/manual/chart-settings/) |
| `chart_shows_drawings` | on, off | on | Charts › Defaults › Show drawings from other charts | [Drawings](/manual/drawings/) |
| `chart_sends_drawings` | on, off | on | Charts › Defaults › Send drawings to other charts | [Drawings](/manual/drawings/) |
| `chart_template` | a chart template's name or `id:N`, or `none` | `Default` | Charts › Defaults › Template | [Chart templates](/manual/chart-templates/) |
| `chart_header` | comma-separated `symbol`, `resolution`, `change`, `template`; or `none` | `symbol,resolution` | Charts › Header | [Charts](/manual/charts/) |
| `chart_time_zone` | an IANA name such as `Europe/London` or `America/Chicago`, or `exchange` for each chart's exchange's own | `America/New_York` | Charts › Other › Time › Time zone | [Preferences](/manual/preferences/#time) |
| `time_format` | `24h`, `12h` | the desktop's clock format | Charts › Other › Time › Time format | [Preferences](/manual/preferences/#time) |
| `timeframes` | comma-separated resolutions, such as `5m,15m,1h,4h,1D,1W`; an entry that is not a resolution is skipped | `5m,15m,1h,4h,1D,1W` | Charts › Other › Preset resolutions | [Charts](/manual/charts/) |
| `sync_crosshair` | on, off | on | Chart settings › Crosshair across Follow charts | [Chart settings](/manual/chart-settings/) |
| `provider` | `yahoo`, `tos` | `yahoo` | Data › Data feed | [Market data feeds](/manual/market-data/) |
| `screenshot_save_file` | on, off | on | General › Also save a PNG | [Screenshots](/manual/screenshots/) |
| `screenshot_folder` | a folder; `~/` is your home; empty for the default | `<Pictures>/Omacharts` | General › Save to | [Screenshots](/manual/screenshots/) |
| `cache_limit` | a size in bytes; `cache limit` takes `512MB` or `2GB` | 1 GB | Data › Keep at most | [Where your data lives](/manual/your-data/) |

### Theme

`config set theme` takes `omarchy` for System, `midnight`, `carbon`, `paper`,
`daylight`, `omarchy:everforest` for an installed Omarchy theme (its folder
name after the colon), or the id of one of your own (`config get theme`
prints it while that theme is chosen). The window picks the theme up when it
next opens; Preferences changes the window in front of you.

```
$ omacharts config set theme midnight
set theme to midnight
  [exit 0]
```

### When a setting is checked and takes effect

`provider`, `chart_header`, `chart_template`, `chart_time_zone` and
`time_format` are checked as they are written. A feed that does not exist, a
header part that is not one, a header part named twice, a time zone that is
not one or a time format that is not one is refused with exit 2; a template
that does not exist with exit 3. Every other
value is accepted as typed, and one the app cannot use counts as the
default. `config set theme nope` succeeds, and the window opens in the first
theme on offer, System on an Omarchy desktop.

```
$ omacharts config set provider thinkorswimm
omacharts: no such data feed: "thinkorswimm"; the feeds are yahoo, tos
  [exit 2]

$ omacharts config set chart_header symbol,banana
omacharts: "banana" is not part of a header; try symbol, resolution, change, template
  [exit 2]

$ omacharts config set chart_time_zone Europe/London
charts read the time in London
  [exit 0]

$ omacharts config set chart_time_zone London
omacharts: "London" is not a time zone; use an IANA name such as Europe/London, or `exchange`
  [exit 2]

$ omacharts config set time_format 12h
charts write times as 12-hour (3:30 PM)
  [exit 0]
```

With a window open, a new `provider` switches the charts' feed at once, a
new `chart_header` changes every header at once, and a new `chart_time_zone`
or `time_format` redraws every chart's times at once. The chart defaults apply to
the next chart made, by a split or a new chartbook. `timeframes`
written from a terminal is read when the window next opens.

## Your data

### `backup`

`backup` keeps and restores copies of your settings ([Where your data
lives](/manual/your-data/)).

```
$ omacharts backup list
settings-20261021-200603.db  2026-10-21 22:06:03     64 kB  schema 4
settings-20261021-180012.db  2026-10-21 20:00:12     64 kB  schema 4
settings-20261020-191533.db  2026-10-20 21:15:33     64 kB  schema 4
  [exit 0]

$ omacharts backup create
backed up the settings to settings-20261021-200135.db (64 kB)
  [exit 0]

$ omacharts backup create
nothing changed since settings-20261021-200135.db, so nothing new was kept
  [exit 0]
```

`backup list` shows each copy's name, when it was taken (local time) and its
size. With `--json` it also names the folder.

`backup restore` takes a name from `backup list`, with or without `.db`, or
the path to a file:

```
$ omacharts backup restore settings-20261021-180012
restored the settings from settings-20261021-180012.db (2026-10-21 20:00:12); what they replaced is kept as settings-20261021-201029.db
  [exit 0]

$ omacharts backup restore ~/Downloads/notes.db
omacharts: notes.db is not an Omacharts settings file; nothing was changed
  [exit 1]
```

A restore does the same as from Preferences; see
[Restoring](/manual/your-data/#restoring). A name that is not a backup fails
with exit 1 and points at `backup list`.

### `cache`

`cache` shows, limits and clears the downloaded prices.

```
$ omacharts cache status
21 series · 4 MB of 1.0 GB · 0%
  [exit 0]

$ omacharts cache limit 2GB
the cache will be pruned back under 2.0 GB
  [exit 0]

$ omacharts cache limit
1.0 GB
  [exit 0]

$ omacharts cache clear
cleared 21 cached series
  [exit 0]
```

`cache limit` without a size reads it. A size is a number with `MB` or `GB`
(`512MB`, `2GB`, `1.5GB`). Any size is accepted, not only the sizes
Preferences offers; something that is not a size is refused with exit 2.
`config set cache_limit` takes the same limit, in bytes. `cache clear`
removes downloaded prices only. Settings, watchlists and chartbooks stay.

## The agent skill

`skill` installs the agent skill ([The agent skill](/manual/agent-skill/))
for Claude and Codex.

### `skill`

| Command | Does |
|---|---|
| `omacharts skill install` | Installs the skill for each agent, in its skills folder |
| `omacharts skill status` | Says, per agent, whether it is installed and where |
| `omacharts skill uninstall` | Removes it, and the agent's `skills` folder too if that leaves it empty |

All three take the same flags:

| Flag | Does |
|---|---|
| `--claude` | Claude only, whether or not it looks installed on this machine |
| `--codex` | Codex only, the same way |
| `--to DIR` | One skills folder instead of any agent's, such as a project's own `.claude/skills`. A relative path is from where you are |
| `--json` | One JSON document on stdout: each agent's `state` (`installed`, `absent`, `occupied`, `dangling`), its `path`, whether it was `refused`, and the skill's `source` |

| Case | What happens |
|---|---|
| No flag | Installs for every agent present, meaning its configuration folder exists or its command is on `PATH` |
| Where an agent's skills go | `$CLAUDE_CONFIG_DIR/skills`, else `~/.claude/skills`; `$CODEX_HOME/skills`, else `~/.codex/skills` |
| No agent found | Says where it looked, writes nothing, exits `0` |
| Already installed | `already installed`, exit `0`. Running it twice is not an error |
| An earlier install pointing at another copy of the skill | Pointed at this one, and says so |
| Something else at `skills/omacharts` | Left as it is, reported on stderr, exit `5`. Any other agent is still installed and reported |
| The package removed after installing | `skill status` says the skill is no longer where it pointed. `skill uninstall` clears it |
| The skill missing from this machine | `install` exits `1` and names where it expected it |

Upgrading the package upgrades the installed skill.

```
$ omacharts skill install
installed for Claude: /home/you/.claude/skills/omacharts -> /usr/share/omacharts/agents/skills/omacharts
installed for Codex: /home/you/.codex/skills/omacharts -> /usr/share/omacharts/agents/skills/omacharts
  [exit 0]

$ omacharts skill status
installed for Claude: /home/you/.claude/skills/omacharts -> /usr/share/omacharts/agents/skills/omacharts
not installed for Codex; `omacharts skill install` would write /home/you/.codex/skills/omacharts
  [exit 0]

$ omacharts skill install --to proj/skills
omacharts: not installed: /home/you/proj/skills/omacharts is already a directory, and not something Omacharts put there; move it aside and run this again
  [exit 5]
```

## The Omarchy plugin

`plugin` puts the Omarchy plugin ([Omarchy plugin](/manual/bar-widget/)) in
the bar and takes it out.

### `plugin`

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

### A key for the plugin

To open and close the plugin with a key, bind `omarchy-shell
omacharts.watchlist toggle` in Hyprland. `open`, `close` and `refresh`
work in place of `toggle`. The refresh interval is set with `omarchy bar
set`; see [Refreshing](/manual/bar-widget/#refreshing).

### `watchlist feed`

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
`muted`, `up`, `down`, `flat`, `hover` and the `tags` pills), and
`updatedAt`.
