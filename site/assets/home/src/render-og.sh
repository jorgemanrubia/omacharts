#!/usr/bin/env bash
# Render the OpenGraph pictures (the card a link to the site unfolds into)
# from the same captures as the home backgrounds.
#
#   site/assets/home/src/render-og.sh CAPTURES_DIR [home|manual...]
#
# CAPTURES_DIR is as render.sh takes it. Each page is drawn at 2400x1260,
# twice the 1200x630 the card is shown at, and saved to assets/og/<name>.jpg
# at that size: a preview is scaled down on every platform, and the 2x
# source keeps the type crisp there. Archivo comes from Google Fonts, so
# the render needs the network; Cascadia and the mark come from the site.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
site="$(cd "$here/../../.." && pwd)"
out="$site/assets/og"; mkdir -p "$out"
captures="$(readlink -f "$1")"; shift
names=("$@"); [ ${#names[@]} -gt 0 ] || names=(home manual)
work="$(mktemp -d)"; trap 'rm -rf "$work"' EXIT
cp "$here"/og-*.html "$here"/og.css "$here"/og-ribbon.js "$site/icon.svg" "$work/"
ln -s "$captures" "$work/captures"
ln -s "$site/assets/fonts" "$work/fonts"
for name in "${names[@]}"; do
  chromium --headless=new --no-sandbox --hide-scrollbars --force-device-scale-factor=1 \
    --window-size=2400,1260 --virtual-time-budget=20000 --screenshot="$work/$name.png" \
    "file://$work/og-$name.html" 2>/dev/null
  magick "$work/$name.png" -strip -sampling-factor 4:2:0 -quality 72 "$out/$name.jpg"
  echo "$name.jpg $(magick identify -format '%wx%h' "$out/$name.jpg") $(stat -c %s "$out/$name.jpg") bytes"
done
