#!/usr/bin/env bash
# Build a self-contained AppDir for omacharts: the release binary plus every
# shared library it needs, GTK's runtime data (pixbuf loaders, GSettings
# schemas, icons, a fallback font) and glibc itself with its dynamic loader,
# so the result runs on distributions older and newer than the one that built
# it. Only the GPU stack (Mesa/NVIDIA) is left to the host.
#
# Usage: build-appdir.sh <AppDir>     (run from the repository root, after
#        `cargo build --release --locked --bin omacharts`)
set -euo pipefail

APPDIR="$(realpath -m "$1")"
HERE="$(dirname "$(realpath "$0")")"
ROOT="$(cd "$HERE/../.." && pwd)"
ID=com.jorgemanrubia.Omacharts
BIN="$ROOT/target/release/omacharts"
LIBDIR="$APPDIR/usr/lib"
MULTIARCH="$(gcc -print-multiarch)"
SYSLIB="/usr/lib/$MULTIARCH"

[ -x "$BIN" ] || { echo "::error::$BIN not built" >&2; exit 1; }

# Libraries that must come from the host because they are tied to its kernel
# driver or hardware: the OpenGL/EGL/Vulkan stack that GTK renders with. Every
# other library is bundled.
EXCLUDE='^(linux-vdso|libcuda|libnvidia|libGL\.so|libGLX|libEGL|libGLdispatch|libOpenGL|libgbm|libgallium|libvulkan_|libdrm)'

rm -rf "$APPDIR"
mkdir -p "$LIBDIR" "$APPDIR/lib64" "$APPDIR/usr/bin"

# --- the application, laid out like packaging/aur/PKGBUILD installs it --------
install -Dm755 "$BIN" "$APPDIR/usr/bin/omacharts"
install -Dm644 "$ROOT/packaging/$ID.desktop" "$APPDIR/usr/share/applications/$ID.desktop"
for size in 16 24 32 48 64 128 256 512; do
    install -Dm644 "$ROOT/assets/icons/${size}x${size}/$ID.png" \
        "$APPDIR/usr/share/icons/hicolor/${size}x${size}/apps/$ID.png"
done
install -Dm644 "$ROOT/assets/icons/$ID.svg" "$APPDIR/usr/share/icons/hicolor/scalable/apps/$ID.svg"
install -Dm644 "$ROOT/agents/.claude-plugin/plugin.json" "$APPDIR/usr/share/omacharts/agents/.claude-plugin/plugin.json"
install -Dm644 "$ROOT/agents/.claude-plugin/marketplace.json" "$APPDIR/usr/share/omacharts/agents/.claude-plugin/marketplace.json"
install -Dm644 "$ROOT/agents/skills/omacharts/SKILL.md" "$APPDIR/usr/share/omacharts/agents/skills/omacharts/SKILL.md"
install -Dm644 "$ROOT/LICENSE" "$APPDIR/usr/share/licenses/omacharts/LICENSE"
install -Dm644 "$ROOT/doc/cli.md" "$APPDIR/usr/share/doc/omacharts/cli.md"

# --- GTK runtime data that lives outside the libraries -----------------------
# gdk-pixbuf loaders: GTK's icons are SVGs, read through librsvg's loader. The
# cache has to name the modules by absolute path, and the mount point of an
# AppImage is not known until it runs, so this is a template that AppRun fills
# in at start.
PIXBUF_LOADERS="$SYSLIB/gdk-pixbuf-2.0/2.10.0/loaders"
mkdir -p "$LIBDIR/gdk-pixbuf-2.0/2.10.0/loaders"
cp -aL "$PIXBUF_LOADERS"/libpixbufloader-svg.so "$LIBDIR/gdk-pixbuf-2.0/2.10.0/loaders/"
[ -e "$PIXBUF_LOADERS/libpixbufloader-png.so" ] && cp -aL "$PIXBUF_LOADERS"/libpixbufloader-png.so "$LIBDIR/gdk-pixbuf-2.0/2.10.0/loaders/"
"$SYSLIB/gdk-pixbuf-2.0/gdk-pixbuf-query-loaders" "$LIBDIR"/gdk-pixbuf-2.0/2.10.0/loaders/*.so |
    sed -E 's|^"/.*/(libpixbufloader-[^/"]+\.so)"|"@LOADERS@/\1"|' >"$LIBDIR/gdk-pixbuf-2.0/2.10.0/loaders.cache.in"
grep -c '^"@LOADERS@' "$LIBDIR/gdk-pixbuf-2.0/2.10.0/loaders.cache.in" | sed 's/^/pixbuf loaders: /'

# GSettings schemas: GTK aborts when it asks for a schema that is not installed
# (the file chooser, for one); libadwaita reads org.gnome.desktop.interface
# (package gsettings-desktop-schemas) when there is no settings portal. Found
# through XDG_DATA_DIRS, so no extra variable.
mkdir -p "$APPDIR/usr/share/glib-2.0/schemas"
for schema in org.gtk.gtk4.Settings.FileChooser.gschema.xml org.gnome.desktop.enums.xml \
    org.gnome.desktop.interface.gschema.xml; do
    [ -e "/usr/share/glib-2.0/schemas/$schema" ] || { echo "::error::$schema not found (gtk4, gsettings-desktop-schemas)" >&2; exit 1; }
done
cp /usr/share/glib-2.0/schemas/org.gtk.gtk4.*.xml /usr/share/glib-2.0/schemas/org.gnome.desktop.enums.xml \
    /usr/share/glib-2.0/schemas/org.gnome.desktop.interface.gschema.xml "$APPDIR/usr/share/glib-2.0/schemas/"
glib-compile-schemas "$APPDIR/usr/share/glib-2.0/schemas"
rm "$APPDIR/usr/share/glib-2.0/schemas"/*.xml

# Icons: libadwaita's own are compiled in; GTK's standard names come from Adwaita.
mkdir -p "$APPDIR/usr/share/icons/Adwaita"
cp -a /usr/share/icons/Adwaita/index.theme /usr/share/icons/Adwaita/symbolic /usr/share/icons/Adwaita/scalable "$APPDIR/usr/share/icons/Adwaita/"
[ -d /usr/share/icons/Adwaita/symbolic-up-to-32 ] && cp -a /usr/share/icons/Adwaita/symbolic-up-to-32 "$APPDIR/usr/share/icons/Adwaita/"
[ -d /usr/share/icons/Adwaita/16x16 ] && cp -a /usr/share/icons/Adwaita/16x16 "$APPDIR/usr/share/icons/Adwaita/"
cp /usr/share/icons/hicolor/index.theme "$APPDIR/usr/share/icons/hicolor/"
gtk4-update-icon-cache -qtf "$APPDIR/usr/share/icons/Adwaita" 2>/dev/null || gtk-update-icon-cache -qtf "$APPDIR/usr/share/icons/Adwaita" 2>/dev/null || true

# Fonts: the host's fonts through a bundled fontconfig, with DejaVu as the
# fallback for a host that has none (see AppRun for when this file is used).
mkdir -p "$APPDIR/usr/share/fonts/dejavu" "$APPDIR/etc/fonts"
cp /usr/share/fonts/truetype/dejavu/DejaVuSans.ttf /usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf \
    /usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf "$APPDIR/usr/share/fonts/dejavu/"
cat >"$APPDIR/etc/fonts/fonts.conf" <<'XML'
<?xml version="1.0"?>
<!DOCTYPE fontconfig SYSTEM "fonts.dtd">
<!-- Used only when the host has no /etc/fonts/fonts.conf (see AppRun). -->
<fontconfig>
  <dir>/usr/share/fonts</dir>
  <dir>/usr/local/share/fonts</dir>
  <dir prefix="xdg">fonts</dir>
  <dir>~/.fonts</dir>
  <dir prefix="relative">../../usr/share/fonts</dir>
  <cachedir prefix="xdg">fontconfig</cachedir>
  <cachedir>~/.fontconfig</cachedir>
  <config><rescan><int>30</int></rescan></config>
</fontconfig>
XML

# Empty on purpose: GIO modules on the host (dconf, gvfs, ...) are linked
# against the host's GLib, not the bundled one.
mkdir -p "$LIBDIR/gio/modules"

# --- dlopen()ed glibc pieces that ldd cannot see -------------------------------
mkdir -p "$LIBDIR/gconv"
cp -a "$SYSLIB"/gconv/. "$LIBDIR/gconv/"
for so in libnss_files.so.2 libnss_dns.so.2 libresolv.so.2; do
    [ -e "$SYSLIB/$so" ] && cp -aL "$SYSLIB/$so" "$LIBDIR/"
done

# --- the exec hook that keeps AppRun's environment out of host programs -------
gcc -shared -fPIC -O2 -Wall -Wextra -o "$LIBDIR/exec.so" "$HERE/exec.c" -ldl

# --- the shared-library closure of every ELF in the AppDir --------------------
export LD_LIBRARY_PATH="$LIBDIR"
is_elf() { [ "$(head -c4 "$1" 2>/dev/null | od -An -c | tr -d ' ')" = '177ELF' ]; }
bundle_deps() {
    local f="$1" out lib name
    out="$(ldd "$f" 2>/dev/null || true)"
    echo "$out" | awk '/=> \//{print $3}' | while read -r lib; do
        name="$(basename "$lib")"
        if [[ "$name" =~ $EXCLUDE ]]; then continue; fi
        [ -e "$LIBDIR/$name" ] || { cp -aL "$lib" "$LIBDIR/$name"; echo "$LIBDIR/$name"; }
    done
    if echo "$out" | grep -q 'not found'; then
        echo "::error::unresolved dependency for $f" >&2
        echo "$out" | grep 'not found' >&2
        return 1
    fi
}
queue="$(find "$APPDIR/usr" -type f \( -name '*.so' -o -name '*.so.*' -o -perm -u+x \) | while read -r f; do is_elf "$f" && echo "$f"; done)"
while [ -n "$queue" ]; do
    next=""
    while read -r f; do
        [ -n "$f" ] || continue
        new="$(bundle_deps "$f")"
        next="$next"$'\n'"$new"
    done <<<"$queue"
    queue="$(echo "$next" | sed '/^$/d')"
done

# --- glibc's dynamic loader (see AppRun) -------------------------------------
cp -aL "$SYSLIB/ld-linux-x86-64.so.2" "$APPDIR/lib64/ld-linux-x86-64.so.2"
rm -f "$LIBDIR/ld-linux-x86-64.so.2"
LOADER_HASH="$(sha256sum "$APPDIR/lib64/ld-linux-x86-64.so.2" | cut -c1-12)"
LOADER="/tmp/.omacharts-appimage-$LOADER_HASH/ld-linux-x86-64.so.2"
patchelf --set-interpreter "$LOADER" "$APPDIR/usr/bin/omacharts"

# --- relative RPATHs instead of an exported LD_LIBRARY_PATH -------------------
unset LD_LIBRARY_PATH # patchelf is a host program
"$HERE/set-runpaths.sh" "$APPDIR"

# --- AppImage entry point and metadata ----------------------------------------
sed "s|@LOADER@|$LOADER|" "$HERE/AppRun" >"$APPDIR/AppRun"
chmod +x "$APPDIR/AppRun"
cp "$APPDIR/usr/share/applications/$ID.desktop" "$APPDIR/"
cp "$APPDIR/usr/share/icons/hicolor/256x256/apps/$ID.png" "$APPDIR/"
ln -sf "$ID.png" "$APPDIR/.DirIcon"

echo "::group::Libraries still resolved from the host (the GPU stack is expected here)"
"$APPDIR/lib64/ld-linux-x86-64.so.2" --library-path "$LIBDIR" --list "$APPDIR/usr/bin/omacharts" |
    grep -v "$APPDIR" | grep '=> /' || true
echo "::endgroup::"
du -sh "$APPDIR"
