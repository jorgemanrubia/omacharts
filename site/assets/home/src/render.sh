#!/usr/bin/env bash
# Render the home backgrounds from fresh app captures.
#
#   site/assets/home/src/render.sh CAPTURES_DIR [name...]
#
# CAPTURES_DIR holds <book>-<theme>.png as `omacharts chartbook screenshot`
# writes them, at 3200x1800 (a 1600x900 window at scale 2 with the rail
# hidden); the books are described in assets/examples/README.md. The pages
# load them as captures/<book>-<theme>.png, so they are rendered from a
# scratch directory where that link exists rather than from the repo.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
out="$(cd "$here/.." && pwd)"
captures="$(readlink -f "$1")"; shift
names=("$@"); [ ${#names[@]} -gt 0 ] || names=(mosaic wall stack ribbon)
work="$(mktemp -d)"; trap 'rm -rf "$work"' EXIT
cp "$here"/*.html "$here"/base.css "$work/"
ln -s "$captures" "$work/captures"
for name in "${names[@]}"; do
  chromium --headless=new --no-sandbox --hide-scrollbars --force-device-scale-factor=1 \
    --window-size=3840,2160 --virtual-time-budget=15000 --screenshot="$work/$name.png" \
    "file://$work/$name.html" 2>/dev/null
  magick "$work/$name.png" -strip -sampling-factor 4:2:0 -quality 80 "$out/bg-$name.jpg"
  magick "$work/$name.png" -resize 1920x1080 -strip -sampling-factor 4:2:0 -quality 82 "$out/bg-$name-1920.jpg"
  echo "bg-$name.jpg $(stat -c %s "$out/bg-$name.jpg") bytes, bg-$name-1920.jpg $(stat -c %s "$out/bg-$name-1920.jpg") bytes"
done
