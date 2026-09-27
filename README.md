# Pelta CBR for Linux

A fast, offline comic reader for the GNOME desktop. Open a `.cbr`, `.cbz` or `.cbt` and start
reading: no library to manage, no account, no sync, no telemetry.

- Native GTK 4 + libadwaita app, Wayland only
- Reads archives with libarchive (the same approach as GNOME Papers), pages decoded on demand
- Single-page or two-page spreads, fullscreen reading
- Sharp scaling (linear-light Lanczos3 or Mitchell–Netravali) and optional auto contrast
- Ships as a Flatpak for x86_64 and ARM64

Website: [peltacbr.vercel.app](https://peltacbr.vercel.app)

## Install

One line, on any distro with (or without) Flatpak:

```bash
curl -fsSL https://peltacbr.vercel.app/install.sh | bash
```

The installer checks Flatpak and Flathub, picks the build for your processor and installs it for
your user. You can also download the bundle for your processor from
[Releases](https://github.com/leonardobetti/pelta-cbr-linux/releases/latest) and open it with
GNOME Software or KDE Discover, or run:

```bash
flatpak install --user pelta-comic-reader-x86_64.flatpak
flatpak run com.pelta.ComicReader
```

## Build

The app follows the [gtk-rust-template](https://gitlab.gnome.org/World/Rust/gtk-rust-template)
layout: Meson drives Cargo and installs the desktop file, AppStream metadata and icons.

### Flatpak (recommended)

```bash
flatpak install flathub org.flatpak.Builder
flatpak run org.flatpak.Builder --user --install-deps-from=flathub --force-clean --install \
  build-dir packaging/com.pelta.ComicReader.yml
flatpak run com.pelta.ComicReader
```

### On the host

Needs Rust 1.92+, Meson, GTK ≥ 4.22, libadwaita ≥ 1.9 and libarchive development files.

```bash
cargo run
# or through Meson
meson setup _build -Dprofile=development && meson compile -C _build
```

## Layout

```
src/            Rust sources (window, archive reader, image processing, reading modes, settings)
data/           Desktop entry, AppStream metainfo, hicolor icons
packaging/      Flatpak manifest (org.gnome.Platform 50)
build-aux/      Meson Cargo helper, Wayland-only manifest check
```

## Releases

Publishing a GitHub release runs `.github/workflows/flatpak.yml`, which builds
`pelta-comic-reader-x86_64.flatpak` and `pelta-comic-reader-aarch64.flatpak` and attaches them to
the release. Keep the `-<arch>.flatpak` suffix: the website and `install.sh` look for it. The
bundles embed Flathub as their runtime source, so they install on machines that have never used
Flathub.

When the GNOME runtime moves on, bump `runtime-version` in the manifest and the CI image tag
together.

## Sandbox

The running app has no network access and no X11. It only asks for Wayland, IPC and GPU access,
and opens files through the file chooser portal. Network is used during the build, while Cargo
fetches crates.

## License

[GNU AGPL v3 or later](LICENSE). Copyright © 2026 Leonardo Betti.

Pelta is also available for macOS, Windows and the browser from the
[website](https://peltacbr.vercel.app). If it's useful to you,
[buy me a coffee on Ko-fi](https://ko-fi.com/leonardobetti).
