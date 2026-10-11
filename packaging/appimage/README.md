# AppImage

`.github/workflows/appimage.yml` builds omacharts as an
[AppImage](https://appimage.org) and starts it in stock Debian, Ubuntu and
Fedora containers to prove that it runs on other distributions. Every push to
`main` leaves one behind as a CI artifact; a tagged release gets it attached.

| File | Purpose |
| --- | --- |
| `build-appdir.sh` | Turns the release build into an AppDir: the binary laid out as the PKGBUILD installs it, GTK's runtime data, every shared library (glibc and its loader included), then `set-runpaths.sh`. |
| `set-runpaths.sh` | Gives every bundled ELF file a relative `DT_RPATH` pointing at `usr/lib`. |
| `exec.c` | Preloaded into omacharts: programs it starts get the environment the AppImage was started with, not the one set up for the bundled GTK. |
| `AppRun` | Entry point. Installs the bundled loader, sets up the environment, starts omacharts. |
| `test-appimage.sh` | The distribution test, run inside a stock container. |

To build one locally, on Ubuntu 24.04 with the packages the workflow installs:

```
cargo build --release --locked --bin omacharts
packaging/appimage/build-appdir.sh AppDir
appimagetool --no-appstream AppDir
```

## How it works

- **Everything is bundled** except the GPU stack (Mesa, NVIDIA, Vulkan ICDs),
  which GTK renders with and which has to match the host's kernel driver. That
  includes glibc, so the AppImage also runs on distributions older than the
  build host (Ubuntu 22.04 is in the test matrix).
- **The bundled loader lives at a fixed path.** The binary's ELF interpreter is
  `/tmp/.omacharts-appimage-<hash>/ld-linux-x86-64.so.2`; the kernel cannot
  expand variables there, so `AppRun` copies the bundled loader to that path
  on start. Every bundled ELF file carries a relative RPATH instead of an
  exported `LD_LIBRARY_PATH`, which would leak into every program omacharts
  starts.
- **GTK comes only from the bundle.** `GTK_EXE_PREFIX`, `GDK_PIXBUF_MODULE_FILE`
  and an empty `GIO_MODULE_DIR` keep GTK, gdk-pixbuf and GIO from loading the
  host's modules (media backends, pixbuf loaders, dconf, gvfs, input methods),
  which are built against the host's GLib and GTK. Icons come from a bundled
  Adwaita, GSettings schemas from a bundled, compiled copy of GTK's, fonts
  from the host through the bundled fontconfig (DejaVu is included for a host
  that has none).
- **Host programs get the host's environment.** The browser opened for the
  thinkorswim sign-in, `xdg-open`, `ps` and `kill` are host programs. `exec.so`,
  preloaded by `AppRun`, undoes every variable `AppRun` set (they are listed in
  `APPIMAGE_RESTORE_VARS`) on each `exec` and `posix_spawn`, so none of them
  sees the bundled GTK's settings or the preload itself.

## Limitations

- **One user per machine for a given build.** The loader directory is created by
  the first user who starts the AppImage. `AppRun` refuses to use a directory
  that belongs to another user (other than root) or that anyone else can write
  to, because it is executed as the interpreter of the program. A second user
  gets an error until the directory is gone (it is removed when `/tmp` is
  cleaned, for example on reboot).
- **`/tmp` must allow executing files.** On a system where `/tmp` is mounted
  `noexec` the AppImage cannot start.
- **x86_64 only.**
- **No Omarchy bar plugin.** It shells out to `omarchy-shell` and expects
  `omacharts` on `PATH`; it is not part of the AppImage.

## Troubleshooting

`OMACHARTS_APPIMAGE_DEBUG=1 ./Omacharts-*.AppImage` prints the environment and
GTK's OpenGL debug output and, with `gdb` installed, a backtrace of a crash.
If a window appears but stays empty, try `GSK_RENDERER=cairo` to rule out the
host's OpenGL driver.

## Updating the pinned `appimagetool`

The workflow downloads a tagged `appimagetool` release and verifies it with
`sha256sum -c`. To update, change `APPIMAGETOOL_VERSION` and
`APPIMAGETOOL_SHA256` in the workflow together; the checksum is the SHA-256 of
`https://github.com/AppImage/appimagetool/releases/download/<tag>/appimagetool-x86_64.AppImage`.
