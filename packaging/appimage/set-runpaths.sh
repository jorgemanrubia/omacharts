#!/usr/bin/env bash
# Give every bundled ELF an RPATH that points at <AppDir>/usr/lib relative to
# itself, so the bundle works WITHOUT exporting LD_LIBRARY_PATH. (DT_RPATH, not
# RUNPATH: it is searched before any LD_LIBRARY_PATH the user's session exports
# and also applies to the dependencies of what it loads.) An exported
# LD_LIBRARY_PATH leaks into every process omacharts spawns (the browser it
# opens for the thinkorswim sign-in, xdg-open, ...), and host programs crash
# when they pick up the bundled, newer glibc.
# Usage: set-runpaths.sh <AppDir>
set -euo pipefail
APPDIR="$(realpath "$1")"
LIBDIR="$APPDIR/usr/lib"
n=0
while IFS= read -r -d '' f; do
    [ "$(head -c4 "$f" | od -An -c | tr -d ' ')" = '177ELF' ] || continue
    # ld.so itself and glibc's loader-adjacent bits must stay untouched.
    case "$(basename "$f")" in ld-linux*) continue ;; esac
    rel="$(realpath --relative-to="$(dirname "$f")" "$LIBDIR")"
    if [ "$rel" = . ]; then rpath='$ORIGIN'; else rpath="\$ORIGIN/$rel"; fi
    patchelf --force-rpath --set-rpath "$rpath" "$f" 2>/dev/null || { echo "patchelf failed: $f" >&2; continue; }
    n=$((n + 1))
done < <(find "$APPDIR/usr" -type f \( -name '*.so' -o -name '*.so.*' -o -perm -u+x \) -print0)
echo "RPATH set on $n ELF files"
