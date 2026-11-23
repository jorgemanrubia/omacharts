---
title: "Preferences"
description: "Reading and writing settings, the theme, and when a setting takes effect."
section: agents
parent: cli
nav_order: 19
---

`config list` prints every setting that has been written, `config get KEY`
one of them, and `config set KEY VALUE` writes one. A setting you have never
changed does not appear; asking for it exits 3, except `agent_context` and
`agent_model.<agent>`, which read as empty.

```
$ omacharts config get show_grid
omacharts: "show_grid" has never been set
  [exit 3]

$ omacharts config set show_grid on
set show_grid to on
  [exit 0]

$ omacharts config get cache_limit --json
{"key":"cache_limit","value":"1073741824"}
  [exit 0]
```

`config list` also lists `workspace` and other entries that are not settings;
`config set` on them is refused. `config set chart_templates` exits `2` and
points at `omacharts template`.

## Settings

Switches take `on` or `off`; `true`/`false`, `yes`/`no` and `1`/`0` mean the
same.

| Setting | Values | Default | Preferences row | Chapter |
|---|---|---|---|---|
| `theme` | `omarchy` (System), `midnight`, `carbon`, `paper`, `daylight`, `omarchy:SLUG` for one installed Omarchy theme, or the id of a theme of your own | `omarchy` on Omarchy, `midnight` elsewhere | Theme › Theme | [Themes](/manual/themes/) |
| `bar_style` | `candles`, `ohlc`, `line` | `candles` | Charts › Defaults › Style | [Chart settings](/manual/chart-settings/) |
| `ohlc_thickness` | `1` to `4` | `2` | Charts › Defaults › Thickness | [Chart settings](/manual/chart-settings/) |
| `bar_scheme` | `theme` (Vivid), `theme-soft`, `theme-quiet`, `theme-mono` | `theme` | Charts › Defaults › Colouring | [Chart settings](/manual/chart-settings/) |
| `hollow_up` | on, off | off | Charts › Defaults › Hollow up bars | [Chart settings](/manual/chart-settings/) |
| `swap_colours` | on, off | off | Charts › Defaults › Swap up and down colours | [Chart settings](/manual/chart-settings/) |
| `chart_session` | `extended`, `regular` | `extended` | Charts › Defaults › Hours | [Chart settings](/manual/chart-settings/) |
| `show_grid` | on, off | off | Charts › Defaults › Gridlines | [Chart settings](/manual/chart-settings/) |
| `chart_log_scale` | on, off | off | Charts › Defaults › Logarithmic scale | [Chart settings](/manual/chart-settings/) |
| `chart_show_drawings` | `none`, `chartbook`, `all` | `all` | Charts › Defaults › Drawings from other charts: Hide, This chartbook, All chartbooks | [Drawings](/manual/drawings/) |
| `chart_shows_drawings` | on, off | on | What versions before `chart_show_drawings` read, and still written beside it: read only where that is not set, on as `all` and off as `none` | [Drawings](/manual/drawings/) |
| `chart_sends_drawings` | on, off | on | Charts › Defaults › Send drawings to other charts | [Drawings](/manual/drawings/) |
| `chart_template` | a chart template's name or `id:N`, or `none` | `Default` | Charts › Defaults › Template | [Chart templates](/manual/chart-templates/) |
| `chart_header` | comma-separated `symbol`, `resolution`, `change`, `template`, `title`, `feed`, `stripe`; or `none` | `feed,symbol,resolution,change` | Charts › Header › Left | [Charts](/manual/charts/) |
| `chart_header_center` | the same | `title` | Charts › Header › Center | [Charts](/manual/charts/) |
| `chart_header_right` | the same, and `account`, `day_pnl`, `trade`, `size`, `order_template`, `position`, `total`, `settings`, `maximize`, `link` | `account,day_pnl,trade,size,order_template,position,total,settings,maximize,link` | Charts › Header › Right | [Charts](/manual/charts/) |
| `chart_time_zone` | an IANA name such as `Europe/London` or `America/Chicago`, or `exchange` for each chart's exchange's own | `America/New_York` | Charts › Other › Time › Time zone | [Preferences](/manual/preferences/#time) |
| `time_format` | `24h`, `12h` | the desktop's clock format | Charts › Other › Time › Time format | [Preferences](/manual/preferences/#time) |
| `timeframes` | comma-separated resolutions, such as `5m,15m,1h,4h,1D,1W`; an entry that is not a resolution is skipped | `5m,15m,1h,4h,1D,1W` | Charts › Other › Preset resolutions | [Charts](/manual/charts/) |
| `sync_crosshair` | on, off | on | Charts › Other › Chart › Sync crosshair across charts | [Charts](/manual/charts/#sharing-the-crosshair) |
| `zoom_sensitivity` | a percentage from `10` to `200`, with or without `%` | `100` | Charts › Other › Chart › Zoom sensitivity | [Charts](/manual/charts/#moving-around) |
| `provider` | `yahoo`, `tos` | `yahoo` | Data › Data feed | [Market data feeds](/manual/market-data/) |
| `screenshot_save_file` | on, off | on | General › Also save a PNG | [Screenshots](/manual/screenshots/) |
| `screenshot_folder` | a folder; `~/` is your home; empty for the default | `<Pictures>/Omacharts` | General › Save to | [Screenshots](/manual/screenshots/) |
| `cache_limit` | a size in bytes; `cache limit` takes `512MB` or `2GB` | 1 GB | Data › Keep at most | [Where your data lives](/manual/your-data/) |
| `agent_context` | any text; empty sends none | empty | Agent › Context | [Dictation](/manual/dictation/#what-the-agent-is-sent) |
| `agent_model.<agent>` | a model the agent takes, such as `opus`; empty for its default. `<agent>` is `claude`, `codex`, `cursor-agent`, `copilot`, `opencode` or `agy` | empty | Agent › Dictation › Model | [Dictation](/manual/dictation/#agents) |

## Theme

`config themes` lists every theme `config set theme` takes, as the Theme
page lists them: `omarchy`, System, which follows the desktop's theme, then
`midnight`, `carbon`, `paper` and `daylight`, every installed Omarchy theme
as `omarchy:` and its folder name, and your own. `*` marks the one in use;
`--json` gives each one's `id`, `name`, `mode`, `kind` (`system`, `shipped`,
`installed` or `custom`), `follows_desktop` and `active`. An open window
changes theme as soon as it is set.

```
$ omacharts config themes
* omarchy               Tokyo Night (system, dark, follows the desktop)
  midnight              Midnight (shipped, dark)
  paper                 Paper (shipped, light)
  omarchy:everforest    Everforest (installed, dark)
  [exit 0]

$ omacharts config set theme paper
set theme to paper
  [exit 0]
```

## When a setting is checked and takes effect

`provider`, `chart_header`, `chart_template`, `chart_time_zone`,
`time_format` and `zoom_sensitivity` are checked as they are written. A feed
that does not exist, a header part that is not one, a header part named
twice, a time zone that is not one, a time format that is not one or a zoom
sensitivity outside 10 to 200 is refused with exit 2; a template
that does not exist with exit 3. Every other
value is accepted as typed, and one the app cannot use counts as the
default. `config set theme nope` succeeds, and the window opens in the first
theme on offer, System on an Omarchy desktop.

```
$ omacharts config set provider thinkorswimm
omacharts: no such data feed: "thinkorswimm"; the feeds are yahoo, tos
  [exit 2]

$ omacharts config set chart_header symbol,banana
omacharts: "banana" is not part of a header; try symbol, resolution, change, template, title, account, feed, day_pnl, trade, size, order_template, position, total, stripe, settings, maximize, link
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

$ omacharts config set zoom_sensitivity 50%
charts zoom at 50% sensitivity
  [exit 0]
```

`agent_context` is said back by its length, and `""` clears it:

```
$ omacharts config set agent_context "Keep answers to one line."
dictation sends the agent 25 characters of extra context
  [exit 0]

$ omacharts config set agent_context ""
dictation sends the agent no extra context
  [exit 0]
```

`agent_model.<agent>` is the model dictation asks that agent for, and `""`
goes back to the agent's own. It is only for the agents Omacharts runs itself:

```
$ omacharts config set agent_model.claude opus
dictation asks Claude Code for opus
  [exit 0]

$ omacharts config set agent_model.claude ""
dictation lets Claude Code use its default model
  [exit 0]
```

With a window open, a new `provider` switches the charts' feed at once, a
new `chart_header` changes every header at once, and a new `chart_time_zone`
or `time_format` redraws every chart's times at once, and a new
`zoom_sensitivity` applies from the next scroll. The chart defaults apply to
the next chart made, by a split or a new chartbook. `timeframes`
written from a terminal is read when the window next opens.
