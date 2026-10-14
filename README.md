# axysBrowser

A lightweight, privacy-focused web browser for Linux, written in Rust with
GTK4 and WebKitGTK 6.

> Status: `1.0.0-beta.1`. Linux only (a Windows build is experimental and not working yet).

## Features

- Tabs, bookmarks bar, history, downloads manager
- Privacy window (ephemeral session, nothing is stored)
- Tracking prevention (WebKit ITP) and cookie policy settings
- Search engines: Brave (default), DuckDuckGo, Google, Bing
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
| Ctrl+N / Ctrl+Shift+N | New window / privacy window |
| Ctrl+L | Focus address bar |
| Ctrl+R, Ctrl+Shift+R, F5 | Reload (Ctrl+Shift+R bypasses cache) |
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
