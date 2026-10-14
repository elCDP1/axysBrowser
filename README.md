# axysBrowser

A lightweight, privacy-focused web browser for Linux, written in Rust with
GTK4 and WebKitGTK 6.

> Status: `1.0.0-beta.1`. Linux only (a Windows build is experimental and not working yet).

## Features

- Tabs, bookmarks bar, history (with age-based cleanup), downloads manager
- Privacy window (ephemeral session, nothing is stored; toggled from the same menu item)
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

## License

AXYS Attribution License 1.0 — see [LICENSE](LICENSE) and [NOTICE](NOTICE).
Derivative works must keep visible attribution to the original project and author.
