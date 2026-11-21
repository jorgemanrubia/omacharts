---
title: "The CLI"
description: "Every omacharts command, with its flags, replies and exit codes."
section: agents
nav_order: 30
moved:
  symbols: /manual/cli/symbols/
  symbol-search: /manual/cli/symbols/#symbol-search
  symbol-show: /manual/cli/symbols/#symbol-show
  "--suffix-on-other-commands": /manual/cli/symbols/#--suffix-on-other-commands
  charts: /manual/cli/charts/
  chart-set: /manual/cli/charts/#chart-set
  chart-range: /manual/cli/charts/#chart-range
  config-bars: /manual/cli/charts/#config-bars
  indicators: /manual/cli/indicators/
  session-times: /manual/cli/indicators/session-times/
  chart-templates: /manual/cli/chart-templates/
  comparing-markets: /manual/cli/comparing-markets/
  measuring-a-move: /manual/cli/drawings/measure/
  volume-and-market-profiles: /manual/cli/indicators/volume/
  drawings: /manual/cli/drawings/
  arranging-charts: /manual/cli/arranging-charts/
  chart-split: /manual/cli/arranging-charts/#chart-split
  chart-close: /manual/cli/arranging-charts/#chart-close
  chart-focus: /manual/cli/arranging-charts/#chart-focus
  chart-list: /manual/cli/arranging-charts/#chart-list
  chartbooks: /manual/cli/chartbooks/
  chartbook-templates: /manual/cli/chartbook-templates/
  linking: /manual/cli/linking/
  sharing-the-crosshair: /manual/cli/sharing-the-crosshair/
  showing-and-hiding-the-sidebars: /manual/cli/showing-and-hiding-the-sidebars/
  watchlists-and-sections: /manual/cli/watchlists-and-sections/
  making-and-filling-a-list: /manual/cli/watchlists-and-sections/#making-and-filling-a-list
  putting-a-list-in-order: /manual/cli/watchlists-and-sections/#putting-a-list-in-order
  sorting: /manual/cli/watchlists-and-sections/#sorting
  turning-a-section-into-a-watchlist: /manual/cli/watchlists-and-sections/#turning-a-section-into-a-watchlist
  moving-watchlists-between-machines: /manual/cli/watchlists-and-sections/#moving-watchlists-between-machines
  screenshots: /manual/cli/screenshots/
  market-data: /manual/cli/market-data/
  provider-login-and-provider-logout: /manual/cli/market-data/#provider-login-and-provider-logout
  provider-list-and-provider-status: /manual/cli/market-data/#provider-list-and-provider-status
  trading: /manual/cli/trading/
  preferences: /manual/cli/preferences/
  settings: /manual/cli/preferences/#settings
  theme: /manual/cli/preferences/#theme
  when-a-setting-is-checked-and-takes-effect: /manual/cli/preferences/#when-a-setting-is-checked-and-takes-effect
  your-data: /manual/cli/your-data/
  backup: /manual/cli/your-data/#backup
  cache: /manual/cli/your-data/#cache
  the-agent-skill: /manual/cli/the-agent-skill/
  skill: /manual/cli/the-agent-skill/#skill
  the-omarchy-plugin: /manual/cli/the-omarchy-plugin/
  plugin: /manual/cli/the-omarchy-plugin/#plugin
  a-key-for-the-plugin: /manual/cli/the-omarchy-plugin/#a-key-for-the-plugin
  watchlist-feed: /manual/cli/the-omarchy-plugin/#watchlist-feed
---

`omacharts` is the command that drives Omacharts from a terminal. Every
action in the window has a command.

The commands are documented by topic, each on a page of its own:

{% include children.html head="Topic" what="What it covers" %}

## Launching the app

| Command | Does |
|---|---|
| `omacharts` | Opens the window, or brings the open one to the front |
| `omacharts SYMBOL [SUFFIX]` | The same, and selects the symbol in its watchlist, as a click there would. The second word is the venue: `omacharts SAP DE` |
| `omacharts --watchlist LIST [SYMBOL]` | Shows that watchlist, by switching to the chartbook that shows it or pointing the open one at it, and selects the symbol in it. `LIST` is a name or `id:N` |
| `omacharts --provider NAME [SYMBOL]` | Charts from that feed for this launch only, without changing the stored setting ([Market data](/manual/cli/market-data/)). Typed at an open window, switches it for as long as it stays open |
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
sidebar on, drawing tools off
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
sidebar on, drawing tools off
* pos:0  AMD        1h    follow
  [exit 0]
```

The second line says whether the sidebar and the drawing tools are out ([Showing and hiding the sidebars](/manual/cli/showing-and-hiding-the-sidebars/)).

`--json` adds every chart's settings (style, colouring, session, scales, link
mode, whether it shows and sends drawings, indicators and chart template),
`sidebar` and `drawingTools`, and `windowOpen`, which says whether you are
reading the window or the stored data.

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
  <SYMBOL>...  one or more tickers, such as NVDA, 2330.TW, the ratio GC/SI or the spread TNX-FVX

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
| `symbol` | Searching the instruments | [Symbols](/manual/cli/symbols/) |
| `watchlist` | Watchlists and the symbols in them | [Watchlists and sections](/manual/cli/watchlists-and-sections/); `watchlist feed` under [The Omarchy plugin](/manual/cli/the-omarchy-plugin/#watchlist-feed) |
| `section` | The sections inside a watchlist | [Watchlists and sections](/manual/cli/watchlists-and-sections/) |
| `chartbook` | Chartbooks | [Chartbooks](/manual/cli/chartbooks/); `chartbook template` under [Chartbook templates](/manual/cli/chartbook-templates/); `chartbook sidebar` under [Showing and hiding the sidebars](/manual/cli/showing-and-hiding-the-sidebars/); `chartbook screenshot` under [Screenshots](/manual/cli/screenshots/) |
| `chart` | The charts in a chartbook | [Charts](/manual/cli/charts/) (`set`, `range`), [Indicators](/manual/cli/indicators/), [Drawings](/manual/cli/drawings/), [Measure](/manual/cli/drawings/measure/), [Zoom](/manual/cli/drawings/zoom/), [Arranging charts](/manual/cli/arranging-charts/) (`list`, `split`, `close`, `focus`), [Linking](/manual/cli/linking/), [Sharing the crosshair](/manual/cli/sharing-the-crosshair/) (`crosshair`), [Showing and hiding the sidebars](/manual/cli/showing-and-hiding-the-sidebars/) (`drawing-tools`), [Screenshots](/manual/cli/screenshots/) |
| `template` | Chart templates | [Chart templates](/manual/cli/chart-templates/) |
| `provider` | The data feed, and signing in to one | [Market data](/manual/cli/market-data/) |
| `trading` | Trading logins, accounts, positions and orders | [Trading](/manual/cli/trading/) |
| `config` | Stored preferences | [Preferences](/manual/cli/preferences/); `config bars` under [Charts](/manual/cli/charts/#config-bars) |
| `cache` | The downloaded prices | [Your data](/manual/cli/your-data/#cache) |
| `backup` | Copies of your settings, and putting one back | [Your data](/manual/cli/your-data/#backup) |
| `skill` | The agent skill | [`skill`](/manual/cli/the-agent-skill/#skill) |
| `plugin` | The Omarchy plugin | [The Omarchy plugin](/manual/cli/the-omarchy-plugin/#plugin) |
