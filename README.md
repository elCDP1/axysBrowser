# axysBrowser

[![CI](https://github.com/elCDP1/axysBrowser/actions/workflows/ci.yml/badge.svg)](https://github.com/elCDP1/axysBrowser/actions/workflows/ci.yml)
![Rust](https://img.shields.io/badge/rust-1.92%2B-orange)
![Platform](https://img.shields.io/badge/platform-Linux-blue)
[![License](https://img.shields.io/badge/license-AXYS%20Attribution%201.0-lightgrey)](LICENSE)

A lightweight, privacy-focused web browser for Linux, written in Rust with
GTK4 and WebKitGTK 6.

> Status: `1.1.2`. Linux only (a Windows build is experimental and not working yet).

## Contents

- [Features](#features)
- [Build](#build)
- [Keyboard shortcuts](#keyboard-shortcuts)
- [Project structure](#project-structure)
- [Data locations](#data-locations)
- [Testing](#testing)
- [Troubleshooting](#troubleshooting)
- [Contributing](#contributing)
- [License](#license)

## Features

- Tabs, bookmarks bar, history (with age-based cleanup), downloads manager
- Tools page: bookmarks manager (`axys://bookmarks`), resources overview
  (`axys://resources`: open tabs + browser memory), zoom controls and
  "open downloads folder"
- Right-click menu on web content: WebKit's standard actions plus
  "View page source" (and "Inspect Element" when developer tools are on)
- Privacy window (ephemeral session, nothing is stored; toggled from the same menu item)
- Private downloads keep a separate list that dies with the window (files stay on disk, like other browsers)
- Per-site permissions (notifications, camera, mic, location) with allow/block prompts
- Tracking prevention (WebKit ITP) and cookie policy settings
- Search engines: Brave (default), DuckDuckGo, Google, Bing, Yahoo, Startpage, Ecosia, Qwant
- Find in page (Ctrl+F), reopen closed tab (Ctrl+Shift+T)
- Dark / light theme, English and Spanish UI

## Build

Requirements: Rust **1.92+**, GTK 4.10+ and WebKitGTK 6.0 (2.52+) development files.

```sh
# Debian / Ubuntu 24.04
sudo apt install libgtk-4-dev libwebkitgtk-6.0-dev libjavascriptcoregtk-6.0-dev pkg-config

# Arch Linux
sudo pacman -S --needed gtk4 webkitgtk-6.0 pkgconf

# Fedora
sudo dnf install gtk4-devel webkitgtk6.0-devel javascriptcoregtk6.0-devel pkgconf-pkg-config

cargo run --release
```

For daily use (so `axysbrowser <url>` resolves), install the binary into
`~/.cargo/bin` (already on `PATH` with rustup):

```sh
cargo install --path .
```

Links from other apps open in axysBrowser once it is set as the default
browser (URLs arrive through `HANDLES_OPEN`, including `xdg-open` and
`axysbrowser https://example.com`):

```sh
# install the .desktop entry locally
desktop-file-install --dir="$HOME/.local/share/applications" assets/axysbrowser.desktop
update-desktop-database "$HOME/.local/share/applications"

# install the icon so launchers and docks show it
install -Dm644 assets/icons/hicolor/scalable/apps/axysbrowser.svg \
  "$HOME/.local/share/icons/hicolor/scalable/apps/axysbrowser.svg"
gtk-update-icon-cache -f -t "$HOME/.local/share/icons/hicolor" 2>/dev/null || true

xdg-settings set default-web-browser axysbrowser.desktop
```

## Keyboard shortcuts

| Shortcut | Action |
| --- | --- |
| Ctrl+T / Ctrl+W | New / close tab |
| Ctrl+Shift+T | Reopen last closed tab |
| Ctrl+N / Ctrl+Shift+N | New window / privacy window |
| Ctrl+L, F6 | Focus address bar (selects everything) |
| Ctrl+R, Ctrl+Shift+R, F5 | Reload (Ctrl+Shift+R bypasses cache) |
| Ctrl+F, Enter / Esc | Find in page (next match / close) |
| Ctrl+O | Open a local file |
| F11 | Toggle fullscreen |
| Esc | Close find bar, else stop loading |
| Ctrl+Tab, Ctrl+Shift+Tab, Ctrl+PgUp/PgDn, Ctrl+1…9 | Switch tabs |
| Alt+Left / Alt+Right | Back / forward |
| Ctrl++ / Ctrl+- / Ctrl+0 | Zoom |
| Ctrl+D | Bookmark page |
| Ctrl+H / Ctrl+J | History / downloads |

## Project structure

```
src/
  main.rs            entry point (exit code from the GTK application)
  app.rs             GtkApplication wiring, CLI / HANDLES_OPEN
  app_state.rs       shared application state, settings load/save
  i18n.rs            locale loading and fallback
  paths.rs           XDG directories, atomic file writes
  theme.rs           CSS
  browser/           engine, tabs, history, bookmarks, downloads, permissions
  internal/
    pages/           built-in pages (newtab, history, downloads, bookmarks,
                     settings, resources, tools, welcome)
    router.rs        axys:// scheme routing
  ui/                window, toolbar, menus, shortcuts, find bar, smokes
locales/             en / es translations
config/              default settings
assets/              desktop entry and icon
.github/             CI workflows and Xvfb smoke scripts
```

## Data locations

Settings, bookmarks and history live in `$XDG_CONFIG_HOME/axysbrowser`
(`~/.config/axysbrowser`); website data in `~/.local/share/axysbrowser`;
caches in `~/.cache/axysbrowser`.

History bursts are coalesced and written a couple of seconds after a visit,
then flushed when the app exits normally, so a crash can lose the last two
seconds of history.

## Testing

```sh
cargo test --locked                      # unit tests
cargo fmt --all --check                  # formatting
cargo clippy --locked --all-targets -- -D warnings
xvfb-run -a bash .github/gui-smoke.sh    # startup + a real page render under Xvfb
xvfb-run -a bash .github/mem-smoke.sh    # open/close tabs, assert WebViews free
```

`gui-smoke.sh` opens a local page and passes only when a WebKit web process
exists and the page's own JavaScript reached the local server, so a broken
web-process launch (sandbox, namespaces) cannot slip through. Its `strict`
mode forces the sandbox on (`AXYS_FORCE_SANDBOX=1`) instead of letting the
browser fall back to no sandbox silently.

`mem-smoke.sh` (plus `strict` variants with the WebKit sandbox on) drives
the built-in `AXYS_MEM_SMOKE` self-test against a local page: it checks
every page really loaded, every closed tab's `WebView` was finalized and
its web process exited, that closing a whole window frees its tabs'
`WebView`s too, and that leftover memory stays below half the growth the
pages caused.

The strict variants need a host where `bwrap` can create and configure
user and network namespaces. Ubuntu 24.04 gates that behind AppArmor;
CI loads Ubuntu's `bwrap-userns-restrict` profile (falling back to the
`kernel.apparmor_restrict_unprivileged_userns` sysctl on the disposable
runner) so the sandboxed smoke runs for real. On a desktop, see
[Ubuntu 24.04 sandbox](#ubuntu-2404-sandbox) if the browser warns that
the sandbox is unavailable.

## Troubleshooting

### Linux sandbox
The browser probes `bubblewrap` before WebKit starts. If unprivileged user
and network namespaces are unavailable (hardened kernels, containers, or a
missing `bwrap`), it prints a warning and keeps running without the sandbox
instead of aborting. Install `bubblewrap` and allow unprivileged user
namespaces to keep the sandbox; check the host with:

```sh
bwrap --unshare-user --unshare-pid --unshare-net --ro-bind / / true
```

### Ubuntu 24.04 sandbox
Ubuntu 24.04 restricts unprivileged user namespaces with AppArmor, which
also blocks WebKit's `bwrap` sandbox (it configures loopback inside the
new network namespace). If the browser prints `WebKit sandbox
unavailable`, it keeps working without the sandbox. To restore it
without weakening the host globally, load Ubuntu's targeted profile:

```sh
sudo apt install apparmor-profiles
sudo install -m 0644 /usr/share/apparmor/extra-profiles/bwrap-userns-restrict /etc/apparmor.d/
sudo apparmor_parser -r /etc/apparmor.d/bwrap-userns-restrict
```

`sudo sysctl -w kernel.apparmor_restrict_unprivileged_userns=0` also works
but relaxes the restriction for every application.

### WSL
Fractional Windows display scaling (125 %, 150 %) can clip 1–2 px rows of
text and misplace popup menus through the WSLg link. If that happens,
either switch Windows to 100 % scaling or live with it — the browser
cannot work around the compositor from inside.

### Rendering and GPU
DMA-BUF (WebKit's GPU compositing path) is enabled by default on Linux and
disabled automatically under WSL, where it can freeze the browser. If you
hit freezes on a Mesa/X11 setup, run with `AXYS_DMABUF=0` (or WebKit's own
`WEBKIT_DISABLE_DMABUF_RENDERER=1`); `AXYS_DMABUF=1` forces it back on.

Hardware acceleration can be toggled in Settings and applies to open tabs
immediately. If the window chrome itself is slow or glitchy, try GTK's
software renderer with `GSK_RENDERER=cairo`. On machines without a working
GPU (VMs, Xvfb, some WSLg setups) GTK falls back to Mesa's software GL, which
is memory hungry: measured under Xvfb, the main process used ~240 MiB PSS
idle with the default renderer and ~105 MiB with `GSK_RENDERER=cairo`. On a
real GPU this does not apply, so the browser does not change the renderer
by itself.

## Contributing

Issues and pull requests are welcome. Before opening a PR, please make
sure it passes the checks in [Testing](#testing) (formatting, unit tests
and clippy with `-D warnings`). See [CONTRIBUTING.md](CONTRIBUTING.md) for
details.

## License

AXYS Attribution License 1.0 — see [LICENSE](LICENSE) and [NOTICE](NOTICE).
Derivative works must keep visible attribution to the original project and author.
