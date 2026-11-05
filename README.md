# axysBrowser

[![CI](https://github.com/elCDP1/axysBrowser/actions/workflows/ci.yml/badge.svg)](https://github.com/elCDP1/axysBrowser/actions/workflows/ci.yml)
![Rust](https://img.shields.io/badge/rust-1.92%2B-orange)
![Platform](https://img.shields.io/badge/platform-Linux-blue)
[![License](https://img.shields.io/badge/license-AXYS%20Attribution%201.0-lightgrey)](LICENSE)

A lightweight, privacy-focused web browser for Linux, written in Rust with
GTK4 and WebKitGTK 6.

> Status: `1.2.3`. Linux only (a Windows build is experimental and not working yet).

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
- Chromium-style three-dot menu: grouped entries with their shortcuts, a zoom
  row (− / 100% / + / full screen, stays open while you adjust), Print…,
  Find…, Save page as… (MHTML), Bookmark this page and Clear browsing data…;
  Settings stays pinned at the bottom and the rest scrolls on short windows
- Right-click on a tab: reload, duplicate, mute / unmute site, close, close
  other tabs, close tabs to the right
- Right-click menu on web content: WebKit's standard actions plus
  "View page source" (and "Inspect Element" when developer tools are on)
- Privacy window (ephemeral session, nothing is stored; toggled from the same menu item)
- Private downloads keep a separate list that dies with the window (files stay on disk, like other browsers)
- Popup blocking: `window.open()` and `target=_blank` open a tab only after a
  user gesture, at most 3 per 5 seconds per tab (a page that just loads cannot
  open tabs, and one click cannot open dozens)
- Links to other applications (`ssh:`, `smb:`, `vnc:`, custom handlers) ask
  before opening; `mailto:`, `tel:` and `sms:` open directly. Without a user
  gesture they never open.
- `mailto:`/`tel:`/`sms:` typed in the address bar reach the desktop app too,
  instead of becoming an invalid `https://mailto:…` URL
- A page gets at most 5 `alert()`/`confirm()` dialogs per 10 seconds; the
  rest are dismissed so a dialog loop cannot lock the browser window
- Per-site permissions (notifications, camera, mic, location) with
  allow/block prompts; clipboard reads are allowed after a user gesture by
  default, and Settings → Privacy can ask per site instead
- Script clipboard writes need a user gesture, so background tabs cannot
  replace what you copied
- Permission answers stay bound to the origin that asked: navigating while the
  prompt is open cancels the grant, and nothing is remembered for the new page
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

# install the icon so launchers and docks show it (PNG for docks that
# cannot use the scalable SVG, e.g. WSLg's X11 taskbar integration)
install -Dm644 assets/icons/hicolor/scalable/apps/axysbrowser.svg \
  "$HOME/.local/share/icons/hicolor/scalable/apps/axysbrowser.svg"
for size in 48 128 256; do
  install -Dm644 "assets/icons/hicolor/${size}x${size}/apps/axysbrowser.png" \
    "$HOME/.local/share/icons/hicolor/${size}x${size}/apps/axysbrowser.png"
done
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
| Ctrl+P | Print the page |
| Ctrl+S | Save the page as a file (MHTML) |
| Ctrl+U | View page source |
| Ctrl+Shift+O | Bookmarks manager |
| Ctrl+Shift+Delete | Clear browsing data (opens Settings) |
| Ctrl+Shift+W | Close the window |
| F11 | Toggle fullscreen |
| Esc | Close find bar, else stop loading |
| Ctrl+Tab, Ctrl+Shift+Tab, Ctrl+PgUp/PgDn, Ctrl+1…9 | Switch tabs |
| Alt+Left / Alt+Right | Back / forward |
| Ctrl++ / Ctrl+- / Ctrl+0 | Zoom |
| Ctrl+D | Bookmark page |
| Ctrl+H / Ctrl+J | History / downloads |

As in Chromium, a page that handles one of these keys itself (for example
Ctrl+S in an online editor, with `preventDefault`) gets it first, and the
browser only acts when the page did not.

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
seconds of history. If a write fails (full disk, read-only profile) it is
retried a few times with growing delays, then again on the next visit and on
exit.

## Testing

```sh
cargo test --locked                      # unit tests
cargo fmt --all --check                  # formatting
cargo clippy --locked --all-targets -- -D warnings
xvfb-run -a bash .github/gui-smoke.sh    # startup + a real page render under Xvfb
xvfb-run -a bash .github/mem-smoke.sh    # open/close tabs, assert WebViews free
xvfb-run -a bash .github/rest-smoke.sh   # idle memory with one internal page
xvfb-run -a bash .github/mhtml-smoke.sh  # export a page to MHTML and inspect it
xvfb-run -a bash .github/history-smoke.sh # a visit reaches history.toml on exit
```

CI runs formatting, build, tests and Clippy on stable and on the declared
Rust 1.92 MSRV, plus ShellCheck and actionlint. A tag release calls that same
workflow through `workflow_call`; publishing waits for it and for a check that
the tag, `Cargo.toml` and this README agree. The uploaded binary gets a
SHA-256 sidecar that is verified before it is attached to the release.

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

`rest-smoke.sh` opens nothing but `axys://resources` and checks that the
whole process tree stays under 160 MiB PSS while idle and that no web
process is left running for a native page. It also recomputes the GL
evidence itself (render node, kernel driver, `LIBGL_ALWAYS_SOFTWARE`,
Mesa driver) and fails unless the renderer the app picked matches it, so a
`/dev/dri` entry backed by a fake driver cannot pass as hardware. It is the
regression guard for the choice described under
[Rendering and GPU](#rendering-and-gpu): software GL costs about 100 MiB and
fails there.

`mhtml-smoke.sh` loads a local page with a stylesheet and an image and saves
it with the same ``save_to_file(…, SaveMode::Mhtml, …)` call the "Save page
as…" menu uses. It then checks the file rather than the app's word for it: a
`multipart/related` document whose `text/html` part carries the page's marker
and the stylesheet rule, with the image embedded as a non-empty `image/png`
part and `Content-Location:` entries, and no 404 while the page loaded. It
covers WebKit's MHTML generation and the export call; the GTK file dialog
itself is still a manual check.

`history-smoke.sh` closes the loop between a real page load and the file on
disk. History saves are deferred by two seconds and coalesced, so the smoke
loads a local page through the normal tab pipeline, quits the app normally
and then requires `history.toml` to hold exactly that visit, with the page's
own title and owner-only permissions (0600 file, 0700 directory). The unit
tests call the manager directly; only this proves the shutdown flush that
writes the last visits still runs.

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
instead of aborting. The state is also shown, in red when it is off, on the
About page (`axys://about`). Install `bubblewrap` and allow unprivileged user
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

Hardware acceleration in Settings is WebKit's GPU use for pages — a
different switch from the one below, which is GTK's renderer for the window.
The window picks its renderer from what GL actually is, not from whether
`/dev/dri` exists: the kernel driver of every `renderD*` node (`faux_driver`,
`vgem` and `vkms` mean no 3D engine — WSLg exposes `faux_driver` beside a real
`renderD128`, which is why the node alone proves nothing, and a virtual node
no longer hides a real GPU behind it), a
`LIBGL_ALWAYS_SOFTWARE` that is switched on (`=0` is off), and Mesa's driver
override naming the CPU rasterizer (`llvmpipe`, `softpipe`, `swrast`,
`swiftshader`). A real GPU keeps the default GL renderer; anything software
gets `cairo`, which draws every widget identically for a fraction of the
memory — with one internal page open, the main process measured 168 MiB PSS
with GL on WSLg and 58 MiB with cairo (under Xvfb the same comparison is
~240 vs ~105 MiB). Set `AXYS_GSK=gl` to force GPU rendering anyway,
`AXYS_GSK=cairo` to force software, or set `GSK_RENDERER` yourself, which
always wins. The chosen renderer and the evidence behind it are printed at
startup.

Web processes get a memory ceiling sized to the machine: 35 % of usable RAM,
clamped to 512 MiB–4 GiB (512 MiB when RAM cannot be read), and never above
half of usable RAM. The old 2 GiB floor let a single tab take a 1 GiB
container's whole quota. "RAM" is the smaller of `/proc/meminfo` and the
nearest cgroup v2/v1 memory quota. Mount points and their roots come from
`/proc/self/mountinfo`, then the code walks from the process's own nested
cgroup up to that mount's hierarchy root, so systemd slices, containers and
non-standard mounts are not missed. The figure is **per web process**. It is
printed at startup, and
`AXYS_WEB_MEMORY_LIMIT` (MiB) overrides it. A tab is still only killed when
it really exceeds the ceiling, which no realistic page does: Discord, Slack
and Figma sit well under 2 GB on machines that can hold them. WebKit's own
30 s memory poll is kept explicitly; 1.1.8 had lowered it to 5 s, six times
more often than the default.

## Contributing

Issues and pull requests are welcome. Before opening a PR, please make
sure it passes the checks in [Testing](#testing) (formatting, unit tests
and clippy with `-D warnings`). See [CONTRIBUTING.md](CONTRIBUTING.md) for
details.

Please report security problems privately, not in a public issue: see
[SECURITY.md](SECURITY.md).

## License

AXYS Attribution License 1.0 — see [LICENSE](LICENSE) and [NOTICE](NOTICE).
Derivative works must keep visible attribution to the original project and author.
