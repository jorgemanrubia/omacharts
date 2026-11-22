---
title: "Session times"
description: "Shading the extended hours, the regular session and hours of your own, from the CLI."
section: agents
parent: cli/indicators
nav_order: 5
---

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
| `--shade-alpha F` | 0 to 1 | How solid the shading is. Every range's, when none is named |

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
