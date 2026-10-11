#!/usr/bin/env bash
# Portability test for the omacharts AppImage. Run inside a stock distribution
# container that has neither GTK nor anything else omacharts needs:
#   test-appimage.sh <path-to.AppImage> <output-dir>
# Only generic test infrastructure (X server, window manager, screenshot tools,
# a font) and a stand-in for the host GPU driver (Mesa) are installed here;
# every library omacharts needs must come out of the AppImage.
# HOSTILE_GTK=1 installs the host's own GTK 4 and libadwaita as well, to prove
# the bundle never loads them.
set -euo pipefail

APPIMAGE="$(realpath "$1")"
OUT="$(realpath -m "${2:-.}")"
mkdir -p "$OUT"
cd "$(mktemp -d)"

. /etc/os-release
echo "::group::Container: $PRETTY_NAME, $(ldd --version 2>&1 | head -1)"
echo "::endgroup::"

echo "::group::Install test infrastructure"
case "${ID_LIKE:-} ${ID}" in
*debian* | *ubuntu*)
    export DEBIAN_FRONTEND=noninteractive
    # Keep downloaded .debs (the images delete them) so the CI package cache works.
    rm -f /etc/apt/apt.conf.d/docker-clean
    echo 'Binary::apt::APT::Keep-Downloaded-Packages "true"; APT::Keep-Downloaded-Packages "true";' \
        >/etc/apt/apt.conf.d/99keep
    apt-get update -qq
    apt-get install -y -qq --no-install-recommends \
        ca-certificates xvfb icewm x11-utils imagemagick fonts-dejavu-core gdb procps dbus \
        libgl1-mesa-dri libglx-mesa0 libegl-mesa0 libgl1 libegl1 >/dev/null
    if [ -n "${HOSTILE_GTK:-}" ]; then
        apt-get install -y -qq --no-install-recommends libgtk-4-1 libadwaita-1-0 librsvg2-common >/dev/null
    fi
    ;;
*fedora* | *rhel*)
    dnf install -y -q --setopt=install_weak_deps=False --setopt=keepcache=1 \
        xorg-x11-server-Xvfb icewm xwininfo ImageMagick dbus-daemon \
        dejavu-sans-fonts gdb procps-ng mesa-dri-drivers mesa-libGL mesa-libEGL >/dev/null
    ;;
*)
    echo "::error::unsupported test distribution: $PRETTY_NAME"
    exit 1
    ;;
esac
echo "::endgroup::"

# Make sure the container really does lack what the AppImage must bring along
# (unless it is meant to have it, to prove the bundle ignores it).
absent="libgtk-4.so.1 libadwaita-1.so.0"
[ -n "${HOSTILE_GTK:-}" ] && absent=""
for lib in $absent; do
    if ldconfig -p | grep -q "$lib"; then
        echo "::error::test container unexpectedly provides $lib"
        exit 1
    fi
done

chmod +x "$APPIMAGE"

echo "::group::The command line works through the AppImage runtime (no FUSE in containers)"
# No bus here: `--version` is answered locally, not by a running instance.
VERSION_OUT="$(APPIMAGE_EXTRACT_AND_RUN=1 DBUS_SESSION_BUS_ADDRESS= "$APPIMAGE" --version 2>&1)" || {
    echo "$VERSION_OUT"
    echo "::error::omacharts --version failed"
    exit 1
}
echo "$VERSION_OUT"
[[ "$VERSION_OUT" == omacharts\ [0-9]* ]] || { echo "::error::unexpected --version output"; exit 1; }
APPIMAGE_EXTRACT_AND_RUN=1 DBUS_SESSION_BUS_ADDRESS= "$APPIMAGE" surface --json >"$OUT/surface.json"
python3 -c 'import json,sys; json.load(open(sys.argv[1]))' "$OUT/surface.json" 2>/dev/null ||
    grep -q '"version"' "$OUT/surface.json" || { echo "::error::surface --json produced no JSON"; exit 1; }
echo "::endgroup::"

"$APPIMAGE" --appimage-extract >/dev/null
APPDIR="$PWD/squashfs-root"

Xvfb "${XVFB_DISPLAY:-:99}" -screen 0 1280x800x24 &
XVFB_PID=$!
sleep 2
export DISPLAY="${XVFB_DISPLAY:-:99}"
icewm >/dev/null 2>&1 &
WM_PID=$!
sleep 2

export HOME="$PWD/home"
mkdir -p "$HOME"
# Behave like a real desktop session: a session bus, and an LD_LIBRARY_PATH full of host
# libraries (GNUstep/Nix/vendor-driver sessions export one). The AppImage must not let
# it change which libraries it uses.
HOSTILE_LD=/usr/lib:/usr/lib64:/lib/x86_64-linux-gnu:/usr/lib/x86_64-linux-gnu
export LIBGL_ALWAYS_SOFTWARE=1 # no GPU in CI; Mesa llvmpipe from the host stands in for the driver
export GDK_BACKEND=x11

LD_LIBRARY_PATH="$HOSTILE_LD" dbus-run-session -- "$APPDIR/AppRun" >"$OUT/omacharts-stdout.log" 2>&1 &
APP_PID=$!

WINDOW_FOUND=0
RENDERED=0
for _ in $(seq 1 15); do
    sleep 2
    kill -0 "$APP_PID" 2>/dev/null || break
    # (capture first: `grep -q` closing the pipe early fails the pipeline under pipefail)
    WINDOWS="$(xwininfo -tree -root 2>/dev/null || true)"
    if grep -qiE '"omacharts"|com.jorgemanrubia.Omacharts' <<<"$WINDOWS"; then
        WINDOW_FOUND=1
        import -window root "$OUT/screenshot.png"
        STDDEV="$(identify -format '%[fx:standard_deviation]' "$OUT/screenshot.png")"
        echo "omacharts window found, frame stddev: $STDDEV"
        if awk -v v="$STDDEV" 'BEGIN{exit !(v > 0.02)}'; then
            RENDERED=1
            break
        fi
    fi
done

echo "::group::Diagnostics"
xwininfo -tree -root 2>&1 | head -40 || true
PID="$(pgrep -x omacharts | head -1 || true)" # APP_PID is only the dbus-run-session wrapper
MISSING=0
if [ -n "$PID" ]; then
    echo "Libraries mapped from outside the AppImage (only the GPU stack is allowed):"
    grep -o '/[^ ]*\.so[^ ]*' "/proc/$PID/maps" | sort -u | grep -v "$APPDIR" | grep -v '/tmp/.omacharts-appimage-' || true
    for lib in libc.so.6 libgtk-4.so.1 libadwaita-1.so.0 libglib-2.0.so.0 libcairo.so.2 libpango-1.0.so.0 \
        libgdk_pixbuf-2.0.so.0 libpixbufloader-svg.so librsvg-2.so.2 libX11.so.6; do
        if ! grep -o '/[^ ]*' "/proc/$PID/maps" | grep "/$lib" | grep -q "$APPDIR"; then
            echo "::error::$lib was not loaded from inside the AppImage"
            MISSING=1
        fi
    done
    # The host may only contribute libraries the bundle does not have (its GPU
    # stack and what that pulls in); a host copy of a bundled one is the failure.
    SHADOWED=0
    for lib in $(grep -o '/[^ ]*\.so[^ ]*' "/proc/$PID/maps" | sort -u | grep -v "$APPDIR" | grep -v '/tmp/.omacharts-appimage-'); do
        base="$(basename "$lib")"
        if ls "$APPDIR/usr/lib/${base%%.so*}".so* >/dev/null 2>&1; then
            echo "::error::$lib was loaded from the host although the AppImage bundles it"
            SHADOWED=1
        fi
    done
    [ "$SHADOWED" -eq 0 ] || MISSING=1
    echo "AppRun's environment (host programs it starts get this undone by exec.so):"
    tr '\0' '\n' <"/proc/$PID/environ" | grep -E '^(GDK_PIXBUF_MODULE_FILE|APPIMAGE_RESTORE_VARS|LD_PRELOAD)=' || true
else
    echo "omacharts has already exited"
    MISSING=1
fi
echo "::endgroup::"

echo "::group::omacharts stdout/stderr"
cat "$OUT/omacharts-stdout.log"
echo "::endgroup::"

if kill -0 "$APP_PID" 2>/dev/null; then
    kill "$APP_PID" 2>/dev/null || true
fi
kill "$WM_PID" "$XVFB_PID" 2>/dev/null || true

[ "$WINDOW_FOUND" -eq 1 ] || { echo "::error::no omacharts window ever appeared"; exit 1; }
[ "$RENDERED" -eq 1 ] || { echo "::error::omacharts window exists but the screen stayed blank"; exit 1; }
[ "$MISSING" -eq 0 ] || exit 1
if grep -qiE 'Settings schema .* is not installed|Failed to load module|symbol lookup error|GLIBC_' "$OUT/omacharts-stdout.log"; then
    echo "::error::omacharts reported a loading problem (see its output above)"
    exit 1
fi
echo "omacharts AppImage passed on $PRETTY_NAME"
