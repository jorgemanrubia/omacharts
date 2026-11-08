---
title: Beautiful charts (Clair de lune)
cover_title: Beautiful charts
stage: render
timing: stills
studio_scale: 8
theme: ethereal
cover: 3
end_card: 6.5
end_fade: 1.2
cover_fade: 1.5
music: Debussy, Clair de lune, the recapitulation to the final cadence, in the Wright Brass quintet's arrangement
music_file: video/out/beautiful-charts-clair-de-lune/music/clair_de_lune_usaf_wright_brass.mp3
music_source: https://commons.wikimedia.org/wiki/File:Clair_de_Lune_-_Wright_Brass_-_United_States_Air_Force_Band_of_Flight.mp3 (The Wright Brass, United States Air Force Band of Flight, "Blue Skies", 1993; https://www.music.af.mil/Multimedia/Music/Public-Domain-Music/)
music_licence: public domain — a work of the US federal government (17 U.S.C. 105); the composition is public domain too (Debussy, d. 1918, published 1905)
music_from: 158.7
music_fade_in: 0.5
music_fade: 1.5
master: linear
---

## Brief

**Pitch.** Omacharts charts are beautiful in any light. One unbroken
camera move, no voice: it opens on a single candle filling the frame and
pulls back, slowly, until a whole six-chart workspace is in view. The
charts never change. Only their look does: the Omarchy theme, and the
hero chart's candles — monochrome, coloured, hollow, OHLC bars, soft.

**For.** Anyone seeing Omacharts for the first time. A showreel to share.

**Payoff.** The cover dissolves into one Tesla candle, the earnings gap of
24 October 2024, standing alone in the dark in Ethereal's ink.

**Beats.** Times are seconds from the end of the cover; the music is the
return of Clair de lune's theme, its swell, and the quiet chorale to its
final chord.
1. **One candle** (0–3). Still, monochrome.
2. **The candle changes** (5–28). Colour arrives, the body empties to an
   outline, the light turns to Kanagawa's warm ink, the candles become OHLC
   bars, while the chart grows around them.
3. **The workspace gathers** (24–44). SPY, then Bitcoin slide in from the
   right, then gold, Nasdaq futures and the Aussie dollar rise from below,
   through Everforest.
4. **Dawn** (42–45). On the climax the whole desk turns to Rosé Pine's
   daylight as the pull-back arrives.
5. **Night returns** (52–67). Nord sweeps in from the opposite corner,
   then Tokyo Night, the camera still easing back, to the final chord and
   the end card.

**Music.** The one public-domain recording of Clair de lune there is:
the Wright Brass of the US Air Force Band of Flight, 1993. No public-domain
or CC0 piano recording exists (Musopen's two are non-commercial).

## Setup

The workspace: six charts, the hero (TSLA daily) at the top left, 62% wide
and 64% tall; SPY and Bitcoin stacked to its right; gold, Nasdaq futures
(five-minute) and AUD/USD in thirds below. Every range ends in the past
and lies under today's price, so no chart shows a live last-price line.

    cli config set show_grid off
    cli config set chart_header symbol,resolution
    key ctrl+shift+b
    cli chart set --template none --grid off
    cli chart split vertical
    cli chart split horizontal --chart pos:0
    cli chart split vertical --chart pos:1
    cli chart split horizontal --chart pos:3
    cli chart split horizontal --chart pos:4
    drag 30% 50.1% 30% 64%
    drag 50.1% 30% 62% 30%
    drag 50.1% 85% 33.4% 85%
    cli chart set --chart pos:0 --template none --symbol TSLA --resolution 1D --style candles --colouring theme-mono --hollow-up off --log-scale off --grid off
    cli chart indicator add ema --chart pos:0 --period 21 --color Amber --width 1.5 --legend off
    cli chart indicator add sma --chart pos:0 --period 50 --color Blue --width 1.5 --style dashed --legend off
    cli chart indicator add volume --chart pos:0 --height 0.16 --colouring chart --legend off
    cli chart drawing add rect --chart pos:0 --from 2024-10-11,224.26 --to 2024-10-23,212.11 --color Teal --width 1 --fill Teal --alpha 0.08
    cli chart drawing add fib --chart pos:0 --from 2024-09-30,264.86 --to 2024-10-23,212.11 --color Violet --width 1
    cli chart drawing add text --chart pos:0 --from 2024-10-02,216.5 --text "Q3 earnings gap" --font-size 13 --text-color Teal
    cli chart set --chart pos:1 --template none --symbol SPY --resolution 1D --style candles --colouring theme-soft --hollow-up off --log-scale off --grid off
    cli chart indicator add ema --chart pos:1 --period 21 --color Amber --width 1.5 --legend off
    cli chart indicator add sma --chart pos:1 --period 200 --color Blue --width 1.5 --legend off
    cli chart indicator add volume --chart pos:1 --height 0.18 --colouring chart --legend off
    cli chart drawing add line --chart pos:1 --from 2025-02-19,613.23 --to 2025-04-25,542.7 --color down --width 1.5
    cli chart drawing add fib --chart pos:1 --from 2025-02-19,613.23 --to 2025-04-07,481.80 --color Violet --width 1 --levels 0,0.382,0.5,0.618,1
    cli chart drawing add profile --chart pos:1 --from 2025-04-03,600 --to 2025-04-25,480 --align right --poc-color Amber --poc-width 2 --transparency 0.6
    cli chart drawing add text --chart pos:1 --from 2025-04-02,590 --text "Liberation Day" --font-size 12 --text-color down
    cli chart set --chart pos:2 --template none --symbol BTC --resolution 1D --style candles --colouring theme --hollow-up on --log-scale off --grid off
    cli chart indicator add ema --chart pos:2 --period 20 --color Amber --width 1.5 --legend off
    cli chart indicator add ema --chart pos:2 --period 50 --color Violet --width 1.5 --legend off
    cli chart indicator add rsi --chart pos:2 --period 14 --height 0.2 --color Violet --overbought 70 --oversold 30 --legend off
    cli chart drawing add avwap --chart pos:2 --from 2023-10-11,27000 --source hlc3 --bands 1 --color Cyan --width 1.5 --band-shaded on --band-alpha 0.05
    cli chart drawing add fib --chart pos:2 --from 2024-01-11,48969 --to 2023-10-11,26561 --color Violet --width 1 --levels 0,0.236,0.382,0.5,0.618,1
    cli chart drawing add line --chart pos:2 --from 2023-10-11,26561 --to 2023-12-04,39880 --color Amber --width 1.5
    cli chart drawing add rect --chart pos:2 --from 2023-12-05,44706 --to 2024-01-09,40530 --color Teal --width 1 --fill Teal --alpha 0.07
    cli chart drawing add text --chart pos:2 --from 2023-12-12,47600 --text "Spot ETF approved" --font-size 12 --text-color Amber
    cli chart set --chart pos:3 --template none --symbol GC --resolution 1D --style ohlc --thickness 2 --colouring theme --log-scale off --grid off
    cli chart indicator add bollinger --chart pos:3 --period 20 --deviations 2 --color Blue --legend off
    cli chart indicator add sma --chart pos:3 --period 50 --color Amber --width 1.5 --legend off
    cli chart indicator add volume --chart pos:3 --height 0.16 --colouring chart --legend off
    cli chart drawing add line --chart pos:3 --from 2025-01-06,2625 --to 2025-04-25,3046.7 --color Teal --width 1.5
    cli chart drawing add line --chart pos:3 --from 2025-01-06,2863 --to 2025-04-25,3284.7 --color Teal --width 1.5
    cli chart drawing add hline --chart pos:3 --from 2025-01-02,3000 --to 2025-04-25,3000 --color Amber --width 1 --text "3000" --font-size 12
    cli chart drawing add text --chart pos:3 --from 2025-04-08,3470 --text "ATH 3,509.9" --font-size 12 --text-color Amber
    cli chart set --chart pos:4 --template none --symbol NQ --resolution 5m --style candles --colouring theme-quiet --hollow-up off --session regular --log-scale off --grid off
    cli chart indicator add vwap --chart pos:4 --anchor session --bands 1,2 --color Cyan --width 1.5 --legend off
    cli chart indicator add volume_profile --chart pos:4 --anchor session --align right --color Blue --poc-color Amber --poc-width 2 --legend off
    cli chart indicator add volume --chart pos:4 --height 0.15 --colouring chart --legend off
    cli chart drawing add rect --chart pos:4 --from 2026-09-21T09:30,30437.75 --to 2026-09-21T10:00,30216.5 --color Teal --width 1 --fill Teal --alpha 0.08 --text "Opening range" --text-at top-left --font-size 12
    cli chart drawing add line --chart pos:4 --from 2026-09-21T10:40,30500 --to 2026-09-21T15:55,30831 --color Amber --width 1.5
    cli chart drawing add fib --chart pos:4 --from 2026-09-21T15:35,30862.75 --to 2026-09-21T09:30,30216.5 --color Violet --width 1 --levels 0,0.236,0.382,0.5,0.618,1 --fib-labels off
    cli chart set --chart pos:5 --template none --symbol AUDUSD --resolution 1D --style candles --colouring theme-mono --hollow-up on --log-scale off --grid off
    cli chart indicator add bollinger --chart pos:5 --period 20 --deviations 2 --color Blue --band-alpha 0.06 --legend off
    cli chart indicator add rsi --chart pos:5 --period 14 --height 0.22 --color Violet --legend off
    cli chart drawing add line --chart pos:5 --from 2025-04-09,0.5923 --to 2025-07-11,0.6538 --color Amber --width 1.5
    cli chart drawing add fib --chart pos:5 --from 2025-07-24,0.6625 --to 2025-04-09,0.5923 --color Violet --width 1 --levels 0,0.382,0.5,0.618,1
    cli chart drawing add rect --chart pos:5 --from 2025-05-01,0.6511 --to 2025-06-10,0.6357 --color Teal --width 1 --fill Teal --alpha 0.08
    cli chart drawing add text --chart pos:5 --from 2025-04-22,0.6005 --text "Tariff low" --font-size 12 --text-color Amber

## Frame

    cli chart focus pos:1
    cli chart range "2025/02/19..2025/04/25" --resolution 1D
    cli chart focus pos:2
    cli chart range "2023/10/08..2024/01/20" --resolution 1D
    cli chart focus pos:3
    cli chart range "2025/01/02..2025/04/25" --resolution 1D
    cli chart focus pos:4
    cli chart range "2026/09/21..2026/09/21" --resolution 5m
    cli chart focus pos:5
    cli chart range "2025/02/24..2025/07/25" --resolution 1D
    cli chart focus pos:0
    cli chart range "2024/09/18..2024/11/05" --resolution 1D
    scroll 30% 30% -1
    scroll 30% 30% 1
    scroll 80% 15% -1
    scroll 80% 15% 1
    scroll 80% 47% -1
    scroll 80% 47% 1
    scroll 16% 80% -1
    scroll 16% 80% 1
    scroll 50% 80% -1
    scroll 50% 80% 1
    scroll 84% 80% -1
    scroll 84% 80% 1
    move -0.4% 50%

## Looks

### ink

**Picture.** Ethereal, the hero's candles Monochrome and filled.

    theme ethereal

### colour

**Picture.** Ethereal; the hero's candles take their colour, Vivid.

    cli chart set --chart pos:0 --colouring theme

### hollow

**Picture.** Ethereal; the hero's rising bodies empty to outlines.

    cli chart set --chart pos:0 --hollow-up on

### kanagawa

**Picture.** Kanagawa's warm ink; the hero still hollow.

    theme kanagawa

### bars

**Picture.** Kanagawa; the hero's candles become 3 px OHLC bars.

    cli chart set --chart pos:0 --style ohlc --thickness 3

### forest

**Picture.** Everforest; the hero back to candles, filled, in Soft.

    theme everforest
    cli chart set --chart pos:0 --style candles --hollow-up off --colouring theme-soft

### dawn

**Picture.** Rosé Pine, the light theme, over the whole desk.

    theme rose-pine

### dusk

**Picture.** Nord; the hero Vivid and filled again.

    theme nord
    cli chart set --chart pos:0 --colouring theme

### night

**Picture.** Tokyo Night, the last look.

    theme tokyo-night

## Shot

The camera opens at 7.6× on the hero candle, holds a breath, and pulls back without stopping, evenly in its logarithm, to
the whole desk on the climax, and keeps easing back while the night
returns, falling from the opposite corner to the dawn. Every change is a crossfade under the moving camera.

    0: look ink
    0: camera 44.39% 21.84% 7.6
    2: camera 44.39% 21.84% 7.6
    4-8: fade colour
    13-16: fade hollow
    17.5-22: fade kanagawa
    21.4: camera 44.39% 21.84% 3.2
    24-27.5: fade bars
    30-35: fade forest
    42-44.6: sweep dawn
    44.4: camera 49.2% 50.8% 1.1
    52-57: sweep dusk from bottom-right
    60-65: fade night
    67.2: camera 50% 50% 1
    67.2: end

## Pronunciation

None. The video has no narration.
