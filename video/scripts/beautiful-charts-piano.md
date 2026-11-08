---
title: Beautiful charts
stage: render
timing: stills
studio_scale: 8
build: 0db337c
theme: ethereal
cover: 3
cover_fade: 1.5
end_card: 4
end_fade: 1.0
music: tender, slightly playful piano with celesta sparkle and soft pizzicato, warm strings swelling gently to a heartfelt peak, then settling; original, generated for this video
music_file: video/out/beautiful-charts-piano/music/3-playful-piano-celesta-strings-ten-chords.flac
music_source: generated for this video with ACE-Step 1.5 (bin/video's music engine), seed 2202, 92 BPM; its ending repainted (seed 3137) into ten struck chords, D7 G Am F to C, the original kept to 63.3 s and crossfaded into the repaint over half a second, the last chord lifted 4 dB; prompts in video/out/beautiful-charts-piano/sources.txt and outro/job.json
music_licence: original music made for this project; no third-party rights
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

**Beats.** Times are seconds from the end of the cover; the music is a
tender piano theme with celesta and pizzicato that grows, with warm
strings, to one heartfelt peak and settles.
1. **One candle** (0–2). Still, monochrome, under the piano's first notes.
2. **The candle changes** (3–26). Colour arrives, the body empties to an
   outline, the light turns to Kanagawa's warm ink, the candles become OHLC
   bars, while the chart grows around them.
3. **The workspace gathers** (26–39). SPY, then Bitcoin slide in from the
   right through Everforest; gold, Nasdaq futures and the Aussie dollar
   rise from below.
4. **Dawn** (38.5–42). As the strings come in, daylight (Rosé Pine)
   sweeps across, and the pull-back arrives on the whole desk at the
   music's peak (55).
5. **Night returns** (59–79). Nord falls from the opposite corner as the
   music drops, then Tokyo Night, the camera still easing back.
6. **Ten chords** (82–97). The music closes on ten struck chords, D7, G,
   A minor and F resolving to C. On each one the whole desk turns to
   another Omarchy theme, light and dark in turn: Catppuccin Latte,
   Gruvbox, Flexoki Light, Retro 82, Lupine, Ristretto, White, Matte Black,
   Rosé Pine's daylight on the loudest chord, and home to Tokyo Night on
   the last, which rings out into the end card. The desk comes to rest on
   the first.

**Music.** Original, generated for this video: no rights to clear.

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

### chord-catppuccin

**Picture.** The whole desk in Catppuccin, the hero Vivid and filled.

    theme catppuccin

### chord-catppuccin-latte

**Picture.** The whole desk in Catppuccin Latte, light, the hero Vivid and filled.

    theme catppuccin-latte

### chord-gruvbox

**Picture.** The whole desk in Gruvbox, the hero Vivid and filled.

    theme gruvbox

### chord-flexoki-light

**Picture.** The whole desk in Flexoki Light, the hero Vivid and filled.

    theme flexoki-light

### chord-retro-82

**Picture.** The whole desk in Retro 82, the hero Vivid and filled.

    theme retro-82

### chord-lupine

**Picture.** The whole desk in Lupine, light, the hero Vivid and filled.

    theme lupine

### chord-ristretto

**Picture.** The whole desk in Ristretto, the hero Vivid and filled.

    theme ristretto

### chord-white

**Picture.** The whole desk in White, the hero Vivid and filled.

    theme white

### chord-matte-black

**Picture.** The whole desk in Matte Black, the hero Vivid and filled.

    theme matte-black

## Shot

The camera opens at 7.6× on the hero candle, holds a breath, and pulls
back without stopping, evenly in its logarithm, to the whole desk on the
music's peak, and keeps easing back while the night returns. Candle and
theme changes are crossfades on the phrases; the two changes between
dark and light sweep across from opposite corners. Then, held on the
whole desk, a crisp change of theme on each of the ten closing chords,
every one landing on its chord's onset (measured from the track).

    0: look ink
    0: camera 44.39% 21.84% 7.6
    2: camera 44.39% 21.84% 7.6
    3-6.5: fade colour
    10-13: fade hollow
    15-20: fade kanagawa
    22-25.5: fade bars
    26.4: camera 44.39% 21.84% 3.2
    31-36: fade forest
    38.5-42: sweep dawn
    55: camera 49.2% 50.8% 1.1
    59.5-63: sweep dusk from bottom-right
    75.5-79.5: fade night
    81.8: camera 50% 50% 1
    81.82-81.94: fade chord-catppuccin-latte
    83.12-83.24: fade chord-gruvbox
    84.42-84.54: fade chord-flexoki-light
    85.72-85.84: fade chord-retro-82
    87.02-87.14: fade chord-lupine
    88.32-88.44: fade chord-ristretto
    89.64-89.76: fade chord-white
    90.94-91.06: fade chord-matte-black
    93.38-93.50: fade dawn
    96.40-96.52: fade night
    97.6: end

## Pronunciation

None. The video has no narration.
