---
title: "Zoom"
description: "A box dragged over the chart, and the chart zoomed to it"
section: charting
parent: drawings
nav_order: 2
hotkey: "Alt + X"
---

Zoom shows the stretch of time and the band of prices you drag a box over.

| Key | Using it | CLI |
|---|---|---|
| <kbd>Alt</kbd> + <kbd>X</kbd>, or **Zoom** just below **Measure** at the top of the tools | Drag a box over the bars you want to see | [`chart zoom`](/manual/cli/drawings/zoom/) |
| <kbd>Alt</kbd> + <kbd>Shift</kbd> + <kbd>Z</kbd> | Back to the view before the last zoom | [`chart zoom back`](/manual/cli/drawings/zoom/) |

<!-- regenerate:
theme: tokyo-night
window: 2560x1600 @2, bin/headless-window (HOME sandboxed and the theme seeded, as for the tools above)
data: the fake feed, built with --features omacharts-engine/fakes,fakes and launched with --provider fake
state:
  omacharts config set theme omarchy:tokyo-night   (before the window opens)
  omacharts chart set --symbol ES --resolution 1h --template none
  key ctrl+shift+d
  key alt+x
  press at 420 140, move to 700 330 and hold (the drag still in progress)
capture: bin/headless-window shot /tmp/zoom.png; magick /tmp/zoom.png -crop 1995x1600+0+0 +repage -resize 1600x site/assets/manual/drawings/zoom.png
crop: 0,0,1995,1600
-->
{% include figure.html src="/assets/manual/drawings/zoom.png" alt="A light box being dragged over a stretch of an hourly chart, with Zoom lit in the tools" caption="A box being dragged with Zoom. Letting go shows just what is inside it." %}

When you let go, the box's left and right edges become the chart's, and its top and bottom become the top and bottom of the price scale. The price scale stops fitting the bars, so the prices you chose stay put as you look around. Double-click the price axis to have it fit the bars again ([Scales](/manual/charts/#scales)).

The box is not kept as a drawing, and the tool goes back to the pointer after one zoom. <kbd>Esc</kbd> while dragging drops the box and puts the tool down, leaving the chart as it was. A click, or a box only a few pixels wide or tall, does nothing and keeps the tool selected. A box over fewer than twelve bars shows twelve, centred on it.

## Going back

Each zoom remembers the view before it. <kbd>Alt</kbd> + <kbd>Shift</kbd> + <kbd>Z</kbd> goes back one zoom at a time, as far as the view before the first, and then does nothing. The price scale goes back with it, fitting the bars again if it was before.

Every chart remembers its own zooms. Changing the chart's symbol or resolution, resetting it with <kbd>Ctrl</kbd> + <kbd>Esc</kbd>, switching chartbooks or closing Omacharts forgets them.

## See also

- [Drawings](/manual/drawings/)
- [Zooming from the CLI](/manual/cli/drawings/zoom/)
