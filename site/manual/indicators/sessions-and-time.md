---
title: "Sessions and time"
description: "Sessions, Time Window and Session Levels: the trading day, a stretch of each day, and the levels a day leaves, with every setting."
section: charting
parent: indicators
nav_order: 4
---

These indicators draw the trading day rather than a study of prices. **Sessions** shades the extended hours and draws a line where each day begins, **Time Window** marks a stretch of each day, and **Session Levels** carries each day's high, low and close across the next. All three follow the trading day of the chart's exchange.

<!-- regenerate:
theme: tokyo-night
window: 1600x1200 @2, bin/headless-window (HOME sandboxed with ~/.local/share/omarchy/themes/tokyo-night copied in)
state:
  omacharts config set theme omarchy:tokyo-night
  omacharts config set time_format 24h
  key ctrl+b
  omacharts chartbook create Sessions --symbol ES --switch
  omacharts chart set --resolution 15m --template none
  omacharts chart indicator add sessions
  omacharts chart indicator add time_window --name London --from 8am --to 1630 --zone Europe/London --mark on --shade off
  omacharts chart indicator add session_levels
  move 700 300
capture: bin/headless-window cli chart screenshot --output /tmp/es.png; magick /tmp/es.png -quality 88 site/assets/manual/indicators/es-sessions-london-levels.jpg
crop: none
-->
{% include figure.html src="/assets/manual/indicators/es-sessions-london-levels.jpg" alt="ES 15m with the extended hours shaded, dashed lines between trading days, the London session's start and its high and low, and the prior regular session's high, low and close" caption="ES 15m with Sessions, a London window (unshaded) and Session Levels." %}

## The trading day

Every instrument trades on an exchange, and the exchange keeps its own clock and its own trading day. A day is split into parts:

| Part | Is |
|---|---|
| **Regular hours** | The main session, the one a chart on **Regular hours only** keeps. Labelled **RTH** on the chart |
| **Pre-market** | Before the regular session, on markets that have one |
| **Post-market** | After the regular session |
| **Overnight** | On futures, from the start of the trading day to the regular session |

Everything outside the regular session is extended hours. **ON** on the chart is the overnight and pre-market together, everything from the start of the trading day to the regular open.

Some examples, in New York time where the exchange is in the US:

| Market | Trading day | Regular hours |
|---|---|---|
| US stocks and ETFs | 04:00 to 20:00 | 09:30 to 16:00 |
| Equity index futures such as ES and NQ (CME Globex) | 18:00 the evening before to 17:00 (17:00 to 16:00 Chicago time) | 09:30 to 16:00, the cash session |
| Crude oil (NYMEX) | 18:00 the evening before to 17:00 | 09:00 to 14:30 |
| Gold (COMEX) | 18:00 the evening before to 17:00 | 08:20 to 13:30 |
| Madrid | | 09:00 to 17:30 Madrid time |
| Tokyo | | 09:00 to 11:30 and 12:30 to 15:30 Tokyo time, with the lunch break left out |
| FX | Round the clock from 17:00 New York time on Sunday to 17:00 on Friday. Each day starts at 17:00 | None |
| Crypto | Never stops. Each day starts at midnight UTC | None |

Listings abroad keep their own exchange's hours in its own clock: London, Xetra, Paris, Zurich, Taipei, Hong Kong and the rest.

There is no holiday calendar. A market holiday is treated as an ordinary weekday, and a day with no bars is not drawn at all.

An instrument whose exchange Omacharts has no hours for gets Sessions' lines at midnight UTC and no shading, no time window counted from a session, and no Session Levels.

The times on the chart are in the time zone chosen in Preferences ([Time](/manual/preferences/#time)), so the same day reads differently on a chart in New York time and one in Chicago time.

## Sessions

Sessions shades the extended hours (pre-market, post-market and overnight) beneath everything, across the price plot and every strip, and draws a line where each trading day, week or month begins. The line falls where the exchange's day begins, not at midnight: on ES, Tuesday's line is at 17:00 Chicago time on Monday.

Its settings open with a row naming the chart's exchange and its day in the chart's time zone, such as **Chicago Mercantile Exchange** over **Trading 18:00–17:00, regular 09:30–16:00, New York time**. The row is not shown while editing a chart template, which has no exchange.

| Group › setting | Default | Allowed |
|---|---|---|
| Extended hours › **Shaded** | On | |
| Extended hours › **Shading %** | 7% | 2 to 60% |
| Extended hours › **Colour** | The theme's muted text colour | A palette name, or a custom colour. **Reset** goes back to the default |
| Separators › **Every** | Day | None, Day, Week, Month |
| Separators › **Colour** | The theme's muted text colour | A palette name, or a custom colour |
| Separators › **Thickness** | 1 px | No line, 1, 1.5, 2.5, 4 px |
| Separators › **Line style** | Dashed | Solid, Dashed, Dotted |

The legend label names the separators drawn: **Sessions · day**, **Sessions · week**, or **Sessions** with none.

A separator needs at least four bars between lines. On a chart too coarse for that, the next longer one is drawn and the legend names it: day lines stay daily on a 4h chart, become weekly on a daily chart and monthly on a weekly chart, and a monthly chart has none. The chosen setting is kept, and comes back on a finer resolution.

Shading is drawn on intraday charts only. It has nothing to shade, and its **Shaded** row says why, in two cases:

- The chart shows **Regular hours only** ("This chart shows regular hours only").
- The market trades round the clock, as FX and crypto do ("Trades around the clock, so there is nothing to shade"). Their day lines are still drawn, at 17:00 New York time for FX and midnight UTC for crypto.

## Time Window

A time window is one named stretch of each day, such as the London session or the first half hour of the regular session. It can shade the stretch, draw a line where it starts, and draw its high and low across the rest of the day.

<!-- regenerate:
theme: tokyo-night
window: 1600x2800 @2, bin/headless-window (HOME sandboxed with ~/.local/share/omarchy/themes/tokyo-night copied in; tall, so the whole dialog fits)
state:
  omacharts config set theme omarchy:tokyo-night
  omacharts config set time_format 24h
  key ctrl+b
  omacharts chartbook create Sessions --symbol ES --switch
  omacharts chart set --resolution 15m --template none
  omacharts chart indicator add time_window --name London --from 8am --to 1630 --zone Europe/London --mark on
  move 80 51               # the legend
  click 142 51             # London's gear
  click 400 150
  move 795 1300
capture: bin/headless-window shot /tmp/window.png
crop: 223,212,1152,2376   (the whole dialog, 16px margin)
-->
{% include figure.html src="/assets/manual/indicators/time-window-settings.png" alt="The settings of a time window named London, with its preset, clock times, time zone and what it shows" caption="The settings of a London window." %}

A window added from the picker is the opening range, shaded, with its high and low.

| Group › setting | Default | Allowed |
|---|---|---|
| Window › **Name** | Opening range | Any text. The legend, the line at the start and the high and low are labelled with it |
| Window › **Preset** | Opening range | See [Presets](#presets). Shows **Custom** once the hours are changed |
| Window › **Times** | After a session opens | **Clock times**, **After a session opens** |
| Window › **Weekdays only** | On | Skips Saturday and Sunday |
| Show › **Shaded** | On | |
| Show › **Shading %** | 7% | 2 to 60% |
| Show › **Line at the start** | Off | A line where each window starts, labelled with the name at the foot of the price plot |
| Show › **High and low** | On | The window's high and low, held until the window comes round again |
| Lines › **Colour** | The next free colour | A palette name, or a custom colour |
| Lines › **Thickness** | 1 px | No line, 1, 1.5, 2.5, 4 px |
| Lines › **Line style** | Dashed | Solid, Dashed, Dotted |

The **Lines** group is for all three: the shading, the line at the start, and the high and low. The high and low are labelled with the name, such as **London H** and **London L**.

With **Times** on **Clock times**, the window is the same hours every day:

| Setting | Does |
|---|---|
| **Starts**, **Ends** | The hours, typed as you like ([Typing a time](#typing-a-time)). A window that ends before it starts runs past midnight |
| **Time zone** | **Chart's time zone**, or a named zone. The subtitle says what the hours come to on the chart today, such as **03:00–11:30 New York time today** for London's 08:00 to 16:30 |

A window in a named zone keeps that zone's hours through its clock changes, so London opens at 08:00 London time in the weeks when its clocks and New York's disagree.

With **Times** on **After a session opens**, the window is counted from where a part of the exchange's day begins, wherever that is:

| Setting | Default | Allowed |
|---|---|---|
| **Session** | Regular hours | Regular hours, Overnight, Pre-market, Post-market |
| **Starts after (min)** | 0 | 0 to 1440 |
| **Lasts (min)** | 30 | 1 to 1440. The window ends with its session at the latest |

A market with no such part has no window: there is no regular-hours window on crypto, and no overnight one on a chart showing regular hours only.

**Weekdays only** judges a window by the day it ends on, in its own time zone. Asia, which opens on Sunday evening in New York, counts as Monday's.

A time window draws nothing on daily and longer charts.

### Presets

**Preset** fills in the window's hours. Choosing one also renames the window, unless you have given it a name of your own.

| Preset | Window |
|---|---|
| **Opening range** | The first 30 minutes of the regular session |
| **Initial balance** | The first 60 minutes of the regular session |
| **Asia** | 09:00 to 15:00 Tokyo time |
| **London** | 08:00 to 16:30 London time |
| **New York** | 09:30 to 16:00 New York time |

### Typing a time

**Starts** and **Ends** take a time however it is typed: `9`, `930`, `0930`, `093000`, `9am`, `9:30pm`, `9:30 p.m.`, `21h30` and `15:30` all work. The chart changes as you type. <kbd>Enter</kbd>, or leaving the field, writes the time back in the chosen [time format](/manual/preferences/#time). Text that is not a time is marked, and the chart keeps the last time that was.

## Session Levels

Session Levels draws each day's levels across the day after it: by default the regular session's high, low and close. Each level starts where its part of the day ends and runs until the same part ends the next day. The regular session's levels start at the close, and the overnight's at the regular open.

| Group › setting | Default | Allowed |
|---|---|---|
| Levels › **From** | Regular session | **Regular session**, **Overnight and pre-market**, **Whole day** |
| Levels › **High and low** | On | |
| Levels › **Close** | On | |
| Levels › **Open** | Off | |
| Levels › **Midpoint** | Off | Halfway between the high and the low |
| Line › **Colour** | The next free colour | A palette name, or a custom colour |
| Line › **Thickness** | 1 px | No line, 1, 1.5, 2.5, 4 px |
| Line › **Line style** | Solid | Solid, Dashed, Dotted |

Each line is labelled once, just above it at its latest stretch on screen, with where it comes from and which level it is:

| From | Legend | Lines |
|---|---|---|
| **Regular session** | **Levels (RTH)** | **RTH H**, **RTH L**, **RTH C**, **RTH O**, **RTH Mid** |
| **Overnight and pre-market** | **Levels (ON)** | **ON H**, **ON L**, and so on |
| **Whole day** | **Levels (Day)** | **Day H**, **Day L**, and so on |

A market that trades round the clock, such as FX or crypto, has no regular session, so its levels are always the whole day's, and **From** says so. On a chart showing **Regular hours only**, **Overnight and pre-market** has no bars to take levels from. Session Levels draws nothing on daily and longer charts.

## See also

- [Indicators](/manual/indicators/)
- [Sessions](/manual/charts/#sessions), for **Regular hours only**
- [Time](/manual/preferences/#time), for the charts' time zone and time format
- [Indicators from the CLI](/manual/cli/#sessions-and-time)
