---
title: "Screenshots"
description: "A picture of a chart or of the whole chartbook, from a key or a menu, what it leaves out, where it goes, and the settings for the file."
section: charting
nav_order: 23
---

A screenshot is a picture of a chart or chartbook, without the buttons. It goes to the clipboard, and a PNG is saved as well unless you turn that off.

## Taking one

| Key or menu | Takes |
|---|---|
| <kbd>Ctrl</kbd> + <kbd>O</kbd> | The focused chart |
| <kbd>Ctrl</kbd> + <kbd>Shift</kbd> + <kbd>O</kbd> | The open chartbook: every chart, as laid out, with the dividers between them |
| The chart menu's **Take screenshot** | The chart right-clicked |
| A chartbook tab's menu, **Take screenshot** | That chartbook, which the right-click has already opened |

Taking one changes nothing on screen. A desktop notification says **Chart screenshot copied** (or **Chartbook screenshot copied**) with **Also saved to** and the file, or **Ready to paste** when no file is kept. When the file cannot be written, the picture is still on the clipboard and the notification says why it was not saved.

## What the picture shows

<!-- regenerate:
theme: tokyo-night
window: 2560x1440 @2, bin/headless-window (HOME sandboxed, theme seeded)
state:
  omacharts chartbook create Desk --symbol NVDA --switch
  omacharts chart split horizontal
  omacharts chart split vertical --chart pos:1
  omacharts chart set --chart pos:1 --resolution 1h
  omacharts chart set --chart pos:2 --resolution 15m
  omacharts chart focus pos:0
  key ctrl+b            # sidebar open
  move 300 300; move 302 301
capture: bin/headless-window shot /tmp/whole.png (cropped 0,0,1000,1440) and bin/headless-window cli chart screenshot --output /tmp/onechart.png, each resized to 1200 high and joined side by side with a 40-pixel gap
crop: as described
-->
{% include figure.html src="/assets/manual/screenshots/on-screen-and-saved.jpg" alt="The NVDA daily as it is on screen, with its buttons, crosshair and focus ring, beside the screenshot of the same chart without them" caption="The NVDA daily on screen (left) and its screenshot (right)." %}

| Kept | Left out |
|---|---|
| The bars, the price and time axes, the gridlines | The resolution strip (the resolution stays, in the header) |
| The header: symbol, resolution and whatever else it shows | The gear, the link button and the **Maximize** button |
| The indicator legend and every indicator | The buttons on an indicator's legend row and strip |
| The last-price tag | The crosshair and its tags |
| Drawings | The focus ring, the sidebar, the drawing tools, the chartbook strip and the window's own corners |

The picture has the size of the chart on screen, in screen pixels. A maximised chart fills the whole picture.

## Where it goes

The PNG goes to the folder under **Save to** in Preferences. The default is `<Pictures>/Omacharts`, that is `~/Pictures/Omacharts` unless your pictures folder is elsewhere. The folder is created with the first screenshot.

Files are named for what they show and when, in local time:

| Of | Named |
|---|---|
| A chart | `SYMBOL-RESOLUTION-YYYYMMDD-HHMMSS.png`, as `NVDA-1h-20261004-143012.png` |
| A chartbook | `NAME-YYYYMMDD-HHMMSS.png`, as `Macro-20261004-143012.png` |

Characters that cannot be in a file name become a dash, so `^GSPC` gives `GSPC-…`. A name already taken in the folder gets `-2`, `-3` and so on rather than being overwritten.

The copy on the clipboard is gone when Omacharts quits, unless a clipboard manager keeps it.

## Preferences

<!-- regenerate:
theme: tokyo-night
window: 2560x1440 @2, bin/headless-window (HOME sandboxed, theme seeded)
state:
  key ctrl+comma
  move 1270 710
capture: bin/headless-window shot /tmp/prefs.png
crop: 930,200,1188,388   (the Screenshots group alone, 18px layout margin, nothing of the sidebar)
-->
![The Screenshots group in Preferences › General, with Also save a PNG switched on and Save to set to ~/Pictures/Omacharts](/assets/manual/screenshots/preferences-screenshots.png)

Preferences › General › **Screenshots**:

| Row | Does |
|---|---|
| **Also save a PNG** | On by default. Off, screenshots are only copied, and **Save to** is greyed out but still shows where a file would go |
| **Save to** › **Change…** | Opens a folder chooser. Clicking the row does the same. Choosing `<Pictures>/Omacharts` restores the default, which then follows the pictures folder if it moves |

## See also

- [Taking screenshots from the CLI](/manual/cli/#screenshots)
- [Settings](/manual/cli/#settings)
