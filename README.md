# axysBrowser

A lightweight, privacy-focused web browser for Linux, written in Rust with
GTK4 and WebKitGTK 6.

> Status: `1.0.4`. Linux only (a Windows build is experimental and not working yet).

## Features

- Tabs, bookmarks bar, history (with age-based cleanup), downloads manager
- Tools page: bookmarks manager (`axys://bookmarks`), task manager
  (open tabs + browser memory), view page source, Web Inspector, zoom
  controls and "open downloads folder"
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

## Data locations

Settings, bookmarks and history live in `$XDG_CONFIG_HOME/axysbrowser`
(`~/.config/axysbrowser`); website data in `~/.local/share/axysbrowser`;
caches in `~/.cache/axysbrowser`.

## Testing

```sh
cargo test --locked                      # unit tests
cargo clippy --locked --all-targets -- -D warnings
xvfb-run -a bash .github/gui-smoke.sh    # startup under Xvfb
xvfb-run -a bash .github/mem-smoke.sh    # open/close tabs, assert WebViews free
```

`mem-smoke.sh` (plus `strict` variants with the WebKit sandbox on) drives
the built-in `AXYS_MEM_SMOKE` self-test against a local page: it checks
every page really loaded, every closed tab's `WebView` was finalized and
its web process exited, that closing a whole window frees its tabs'
`WebView`s too, and that leftover memory stays below half the growth the
pages caused.

## Troubleshooting (WSL)
Fractional Windows display scaling (125 %, 150 %) can clip 1–2 px rows of
text and misplace popup menus through the WSLg link. If that happens,
either switch Windows to 100 % scaling or live with it — the browser
cannot work around the compositor from inside.

## License

AXYS Attribution License 1.0 — see [LICENSE](LICENSE) and [NOTICE](NOTICE).
Derivative works must keep visible attribution to the original project and author.
