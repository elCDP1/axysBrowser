# Windows + Servo: audit and port plan

Status: **phases 1–4 written; nothing has run on Windows yet.** The shared
interface no longer names an engine: everything outside
`src/browser/engine/` talks to the seam (`View`, `Session`, `Download`,
`AuthPrompt`, `BrowserEngine`). Both backends exist — `engine/webkit` for
Linux/WebKitGTK and `engine/servo` for Windows/Servo — and `Cargo.toml`
resolves each engine only for its own target, so Linux is unchanged and a
Windows build no longer pulls WebKitGTK.

The Servo backend has **never been compiled**. This machine has no MSVC, no
GTK4 for Windows and no CMake/NASM/libclang, and the only place a Servo build
can be proved is the `windows-2022` runner, which needs a push. Its code was
written against Servo 0.7's published sources — every signature in it was read
from the crate, not remembered — but *written* is not *works*; §8 lists what is
checked and §12 what is still open.

Two blockers remain:

* **Platform modules: done.** `paths.rs` no longer reaches for
  `std::os::unix::fs`: owner-only files go through `restrict`,
  `create_private_dir` and `create_private_file`, which set 0700/0600 on Unix
  and do nothing on Windows, where the profile's ACL already restricts them.
  `config_dir`/`data_dir`/`cache_dir` fall back to `%APPDATA%`/`%LOCALAPPDATA%`
  when neither `XDG_*` nor `HOME` exist, so the profile never lands in the
  working directory. `app::linux` takes `running_under_wsl` from
  `platform::linux`; the Windows half of `platform` went away with the Linux
  startup probes that asked it. `ui/memprobe.rs` compiles on
  Windows because it only *reads* `/proc` at runtime. The Windows check
  (`cargo check --locked --target x86_64-pc-windows-msvc`, also with
  `--all-targets`) now compiles the crate with no errors.
* **GTK4 for MSVC.** The interface is GTK4 on both platforms; a Windows build
  needs gvsbuild GTK4 import libraries and ~40 DLLs at runtime.
* **Verification.** The `windows` CI job now exists (§9) and has not run on a
  runner yet; locally the crate compiles for the Windows target. The rule
  "green on Linux *and* Windows before a commit" still keeps this work out of a
  commit.

This document is the report required before any engine work starts. It records
what the repository contains today, how the code splits between shared, Linux
and Windows code, what Servo 0.7 actually offers against every feature the
browser already ships, how the existing GTK4 interface can survive on Windows,
and what the port costs. Nothing here changes Linux behaviour.

## 1. Starting point

| Item | Value |
| --- | --- |
| Repository | `elCDP1/axysBrowser` |
| Branch created | `windows` |
| Base commit | `2e5a63c` — "Release 1.2.6: valid URL ports and reliable findbar callbacks" |
| Base tag | `v1.2.6` (last tag in the repository) |
| Tags present | `v1.1.7` … `v1.2.6` |
| `v1.3.0` tag | **does not exist** |
| Crate version at base | `1.2.6` |

There is no `v1.3.0` tag and no commit that claims to be 1.3.0 (`git tag -l`,
`git log --oneline` and the last eight release commits all top out at 1.2.6).
Because the request also asks to set 1.3.0 on both branches, **1.3.0 is defined
here as `2e5a63c` plus the version bump**, which is the only source that exists.
If a different 1.3.0 is intended, say so and the branch moves to it before
anything else lands.

The working tree was clean before the branch was created; no local change of
yours was overwritten. `main` is untouched apart from the 1.3.0 version edit,
and nothing has been committed.

## 2. The code today

22,388 lines of Rust across 46 files, one binary, two direct engine
dependencies: `gtk4 0.11` (feature `v4_10`) and `webkit6 0.6.1` (feature
`v2_52`). 505 lines mention GTK or WebKit.

WebKitGTK is reached through a **single module**, `src/browser/engine/`
(`mod.rs`, the seam, plus `webkit.rs`, 2,687 lines at the move):
`BrowserEngine::configure`, `load`, `reload`, `open_external`,
the timeout/error pages, the source-view marker, plus the signal handlers for
popups, permissions, authentication, downloads and script dialogs.

Everything else that mentions WebKit does so for one of three reasons:

* it owns a `WebView` widget in the GTK tree (`ui/window.rs`, `ui/tabs.rs`),
* it touches a `NetworkSession` (`app_state.rs`, `browser/permissions.rs`,
  `browser/downloads.rs`),
* it is a diagnostic tool that inspects engine processes
  (`ui/memprobe.rs`, `ui/memsmoke.rs`, `ui/mhtmlsmoke.rs`, `ui/historysmoke.rs`).

That is a good starting position: the engine boundary is already narrow, and
`browser/address.rs` (717 lines), `browser/history.rs` (1,161),
`browser/bookmarks.rs` (839), `paths.rs` (421), `theme.rs` (822) and `i18n.rs`
(250) contain **no** engine code and are portable as they are.

### Module classification

| Module | Lines | Class | Notes |
| --- | --- | --- | --- |
| `browser/engine/webkit.rs` | 2,687 | **engine adapter** | Moved behind the `browser/engine/mod.rs` facade in phase 3 |
| `ui/window.rs` | 2,976 | mostly shared, engine point | Owns the `WebView` child in the GTK tree |
| `ui/menu.rs` | 1,517 | shared (GTK only) | No engine references at all |
| `ui/toolbar.rs` | 844 | shared (GTK only) | Two engine calls |
| `app_state.rs` | 966 | shared, session point | Builds the `NetworkSession` |
| `browser/history.rs` | 1,161 | **shared** | File-only |
| `browser/permissions.rs` | 1,144 | split | Policy is shared; the prompt type is WebKit's |
| `app.rs` | 922 | split | GTK app wiring + XDG/CLI |
| `theme.rs` | 822 | **shared** | CSS only |
| `browser/bookmarks.rs` | 839 | **shared** | Two incidental engine refs |
| `browser/downloads.rs` | 662 | adapter | WebKit `Download` type |
| `browser/address.rs` | 717 | **shared** | URL/search parsing |
| `ui/shortcuts.rs` | 522 | shared (GTK only) | |
| `ui/findbar.rs` | 471 | adapter | WebKit `FindController` |
| `internal/**` | 2,608 | **shared (GTK only)** | `axys://` pages are GTK widgets, no web content |
| `ui/{auth,bookmarks,logo,spinner,tabs,window_controls,navigation}.rs` | 1,680 | shared (GTK only) | |
| `ui/{memprobe,memsmoke,mhtmlsmoke,historysmoke}.rs` | 738 | **Linux-specific** | Reads `/proc`, WebKit processes |
| `paths.rs` | 421 | split | XDG → Windows known folders |
| `main.rs` | 55 | shared | |
| `browser/tabs.rs`, `browser/mod.rs`, `ui/actions.rs` | 386 | **shared** | |

Two things follow:

1. `axys://` pages are GTK widgets, not HTML. They survive a Servo port
   untouched **only if the window stays GTK**. That is the strongest argument
   for the GTK4-on-Windows option below.
2. The engine adapter surface is roughly the 20 operations in
   `BrowserEngine` plus the session, permission, download and find paths.

## 3. What Servo 0.7 provides

Servo became a crates.io library in April 2026; the current release is
`servo 0.7.0` (MSRV 1.88, this machine has 1.98). The API is
`ServoBuilder` → `Servo` → `WebViewBuilder` → `WebView`, with a
`RenderingContext` and a `WebViewDelegate`. The crate ships a working
`examples/winit_minimal.rs`, which was the starting point for the POC.

Three rendering contexts exist:

* `WindowRenderingContext` — surfman/OpenGL on a raw window handle; paints the
  whole window surface.
* `OffscreenRenderingContext` — a child surface of a `WindowRenderingContext`;
  this is how you composite a page into another scene.
* `SoftwareRenderingContext` — renders to an image with no window at all.

The rendering model is: Servo calls `notify_new_frame_ready`, the embedder
calls `WebView::paint()`, then `RenderingContext::present()`. Embedders must
also pump `Servo::spin_event_loop()` and provide an `EventLoopWaker` so Servo
can wake the UI thread from its own threads.

Two upstream facts shape the design:

* Servo is **moving away from multiple WebViews per rendering context**
  (servo/servo#35536). Each tab should own its context; do not build tab
  switching on swapping pages inside one surface.
* The best public precedent for embedding Servo in an existing widget tree is
  `iced_servo`, which renders into an offscreen context, calls
  `read_to_image`, and draws the result as an ordinary widget texture — no
  child windows, no per-platform window-procedure code. On Windows it notes
  `SoftwareRenderingContext` runs on WARP and that the GPU-to-CPU-to-GPU round
  trip is the cost to beat.

### Feature-by-feature mapping

| axysBrowser feature | WebKitGTK today | Servo 0.7 | Verdict |
| --- | --- | --- | --- |
| Navigation, reload, back/forward | `WebView::load`, `go_back` | `load`, `reload`, `go_back`/`go_forward` | direct |
| Tabs | one `WebView` widget per tab | one `WebView` + context per tab | direct |
| Zoom | `set_zoom_level` | `set_page_zoom` / `page_zoom` | direct |
| Popups (`window.open`, `target=_blank`) | `create` signal | `request_create_new` | direct |
| Permissions (camera, mic, location, notifications) | `permission-request` | `request_permission` | direct |
| Auth prompts | `authenticate` | `request_authentication` | direct |
| `alert`/`confirm`/`prompt` | `script-dialog` | `AlertDialog`/`ConfirmDialog`/`PromptDialog` via `show_embedder_control` | direct |
| Context menu | `context-menu` | `ContextMenu` via `show_embedder_control` | direct |
| File upload / `<select>` / colour pickers | GTK chooser | `FilePicker`/`SelectElement`/`ColorPicker` via `show_embedder_control` | direct |
| Notifications | `show-notification` | `show_notification` | direct |
| User agent, cache and profile dirs | `Settings`, `NetworkSession` | `Opts`, `Preferences`, `NetworkManager` | direct |
| Clear browsing data | `WebsiteDataManager::clear` | `SiteDataManager` | direct |
| Cookies policy | `CookieManager` policy | `Preferences` cookie options | direct |
| Download interception | `NetworkSession` `decide-policy` | `load_web_resource` / navigation delegate | **partial** — must be assembled |
| Find in page | `FindController` | *no public API in 0.7* | **gap** |
| Print | `PrintOperation` | *no print API* | **gap** |
| MHTML export | `save_to_file(SaveMode::Mhtml)` | *no equivalent* | **gap** |
| DevTools | WebKit inspector | WebDriver commands only | **gap** |
| Tracking prevention (ITP) | WebKit ITP | *none* | **gap**, needs an own blocker |
| Sandboxed web process | `bwrap` + WebKit sandbox | `multiprocess` feature, own sandboxing | **partial** |
| Process memory ceiling | `MemoryPressureSettings` | not exposed | **gap** |
| Session restore | shared code (URL list) | shared code | unaffected |

The five gaps are the real cost of the port and each needs a decision before
migration starts:

* **Find**: implement with an injected script (`window.find` +
  highlight spans) driven by the existing find bar. Behaviour will differ from
  WebKit's native highlight; acceptable, but it is a visible change.
* **Print**: no engine support. Options are (a) render pages to a bitmap and
  print that, (b) wait for a Servo print API, (c) ship without print on
  Windows until then. This is the one feature that may have to be deferred —
  it needs your approval, not mine.
* **MHTML**: axysBrowser already validates MHTML in `mhtml-smoke.sh`, so the
  serialiser can be built in shared code from the network layer; a real
  implementation, not a stub.
* **DevTools**: hard to replace. Realistic scope is "view page source plus a
  JS console", not an inspector.
* **ITP / tracker blocking**: Servo has no equivalent; blocking becomes an
  in-house list applied at `request_navigation`/`load_web_resource`. This is
  a privacy feature the project cannot quietly lose.

## 4. Can the current interface survive on Windows?

The interface is 12,000 lines of GTK4 widget code, hand-built to match the
current design (toolbar, tab strip, three-dot menu, floating pickers, CSS in
`theme.rs`). Rewriting it is a redesign, so the question is whether GTK4 can
host it on Windows.

**Yes, with caveats that must be proven.**

* GTK4 officially supports Windows; gvsbuild ships current builds
  (GTK 4.22.4 in its 2026.6.0 release), and the project's
  gvsbuild is the supported route (the old experimental
  `.github/workflows/windows.yml` did it that way before it was removed, see
  §9).
* `gtk4 0.11` targets GTK ≥ 4.10, so the same crate version builds against it.
* A known caveat: gvsbuild patches out DirectComposition by default, so GSK
  silently falls back to the Cairo renderer unless `GDK_DEBUG=dcomp` is set.
  For a browser that is not necessarily bad — the app already chooses
  `cairo` when GL is software — but it must be measured, not assumed.
* Distribution becomes "binary + ~40 GTK DLLs". That is how GTK apps ship on
  Windows, but it is a packaging decision for you.

### Packaging

`.github/package-windows.sh` builds the release zip from the GTK 4 runtime
the binary links against (DLLs, gdk-pixbuf loaders, compiled GLib schemas and
the Adwaita icon theme), the release executable and Servo's ANGLE libraries,
then starts the staged `axysbrowser.exe` and requires a responsive window
before it keeps the artifact:

```bash
cargo build --release --locked
.github/package-windows.sh --gtk /c/gtk
```

Both omissions that broke the hand-built zips are caught here: the ANGLE DLLs
(without them the first page panicked in surfman's EGL bindings) and
`share/icons` (without it a bookmark with no favicon resolved
`web-browser-symbolic` to nothing and drew a missing-image icon).

**How Servo's surface gets inside that window.** GTK4 has no child-window
embedding (no `GtkSocket`), and `GtkGLArea` only exposes GTK's own GL context,
so a `WindowRenderingContext` on a foreign HWND is not available. The
supported shape is the one `iced_servo` uses:

```
Servo WebView  →  per-tab RenderingContext (offscreen or software)
               →  read_to_image() / shared texture
               →  GdkMemoryTexture  →  GtkPicture inside the tab's container
```

This keeps every existing widget, CSS rule and layout in place: the page simply
becomes a texture inside the content area. Input is translated from GTK
`GestureClick`/`EventControllerKey`/`EventControllerScroll` into Servo's
`InputEvent`s — the reverse of what the POC does with winit, same event types.

Cost: one copy per frame. At 1024×768 that is ~3 MB per frame; the first
measurement in phase 2 decides whether the GPU-texture path
(`gleam_gl_api()` sharing instead of `read_to_image`) is needed.

**Rejected alternatives.** A winit + native-widget window would force a
rewrite of the entire interface (design risk, months of work) and a
WebView2/Chromium path is explicitly forbidden. GTK4 hosting is the only option
that keeps the design *and* the technology rules.

## 5. Proposed layout

Keep one repository, one binary, conditional dependencies:

```toml
[target.'cfg(target_os = "linux")'.dependencies]
gtk = { package = "gtk4", version = "0.11", features = ["v4_10"] }
webkit6 = { version = "0.6.1", features = ["v2_52"] }
webkit6-sys = "0.6.0"

[target.'cfg(windows)'.dependencies]
gtk = { package = "gtk4", version = "0.11", features = ["v4_10"] }
servo = "0.7"
```

```text
src/
  core/          shared and engine-agnostic: address, history, bookmarks,
                 settings, paths, i18n, theme
  engine/
    mod.rs       the seam: platform selection and the backend contract
    webkit.rs    cfg(target_os = "linux") — today's engine.rs, unchanged in behaviour
    servo.rs     cfg(windows)
  ui/            GTK4 widgets, shared by both platforms
  internal/      axys:// pages, shared
  platform/
    linux/       XDG, bwrap probe, /proc memory probe, WebKit process checks
    windows/     known folders, Win32 memory probe, process enumeration
```

The types the seam has to expose, the migration order and the tests it makes
possible are in [`phase4-engine-api.md`](phase4-engine-api.md).

`engine/mod.rs` is deliberately a facade, not an abstraction: it picks the
backend for the platform and re-exports its surface, and it documents what a
backend has to provide (`BrowserEngine` and its methods, the session and view
types, `AuthenticateHandler`, the cookie/UA/cache helpers). Phase 3 stopped
here on purpose. A trait or a set of type aliases written against a single
backend would be guesswork about Servo's shape, so the real abstraction is
drawn in phase 4, with two implementations to check it against — and the
interface files then only change if a second engine proves they must.
What phase 3 does establish: `browser::engine` is the only module that
*selects* the engine, so adding a platform is one `cfg` and a backend file in
one place. What it does not establish yet is type isolation — WebKit types
still reach the interface in 16 files (`ui/window.rs`, `ui/findbar.rs`,
`browser/downloads.rs`, `ui/auth.rs`, the session in `app_state.rs`, the
`/proc` diagnostics), because those files hold a `WebView`, a `NetworkSession`
or a `Download` directly. Narrowing the seam to the engine's own types is
part of phase 4, where a second implementation says what the shared shape
actually has to be.

Linux must keep building exactly as it does today: `cargo build` on `main`
touches none of the new modules.

## 6. Memory work (the defining feature)

Linux measures PSS from `/proc`; Windows has no PSS. The equivalents:

* **Private commit** — `PROCESS_MEMORY_COUNTERS_EX::PrivateUsage`. This is the
  number that matters: memory the process holds and no one else can reclaim.
  Working set alone overstates retention because it includes shared pages and
  pages the OS can trim.
* **Working set** — `WorkingSetSize`, reported but never used as the gate.
* **Process and thread counts**, and GDI/USER handle counts
  (`GetGuiResources`) as the graphics-resource proxy, since there is no
  DMA-BUF accounting.

The existing self-test hooks (`AXYS_MEM_SMOKE`) and the tab lifecycle checks
are kept; the Windows variants get their own probe module so the Linux ones
stay byte-for-byte as they are. The schedule from `rest-smoke.sh`
(idle / 1 / 5 / 10 / 20 tabs, repeated open-close, long navigation, window
close) is reused with Windows thresholds, which have to be measured before
they are written down.

The one design decision that decides whether memory works: **the delegate must
not own the window state**. `AppState` owns the `WebView`, the `WebView` owns
its delegate, so a strong reference from delegate to state is a cycle that
keeps every closed tab's engine alive forever. The POC therefore holds a
`Weak<AppState>`; this is also why the POC prints `[closed] webview released`
when a page is torn down.

## 7. Proof of concept

`poc/windows-servo/` is a standalone crate (own `[workspace]`, never built by
the Linux package). It proves, on a real Windows desktop:

* Servo initialises in-process and serves frames through
  `WindowRenderingContext` on a native HWND;
* navigation, reload and zoom;
* JavaScript evaluation, with the result crossing back into Rust;
* mouse move/click, wheel, keyboard and resize routing;
* several windows, therefore several concurrent `WebView`s against one `Servo`;
* teardown that actually releases the `WebView`.

```powershell
cd poc\windows-servo
cargo run                                  # Servo demo page
cargo run -- https://example.com
cargo run -- --webviews 3
cargo run -- --self-test                   # exit 0 = pass
```

`--self-test` loads a `data:` page whose script sets `document.title`, then
asserts the value read back from Rust equals `js-ok:2`. That is the
machine-checkable gate for CI.

### Build and result

`.github/workflows/windows-servo.yml` builds it on `windows-2022` and runs the
headless self-test. Run **44566879766** (commit `75302f5`) is green:
`cargo fmt --check`, `cargo build --locked`, and `--self-test --headless`
passing.

What that run actually proves, in order of the log:

* Servo initialises inside our process; no external browser is involved.
* A `SoftwareRenderingContext` is created through ANGLE — EGL over D3D11 with
  WARP, so a machine with no graphics driver still renders. Plain WGL was the
  first attempt and fails there (`RequiredExtensionUnavailable`), which is why
  the crate enables Servo's `no-wgl` feature; this is also what a packaged
  Windows build will need on driver-less machines.
* A `data:` page loads to `LoadStatus::Complete`.
* `document.title` read back through `evaluate_javascript` equals `js-ok:2`,
  so script execution and the value bridge into Rust both work.
* Frames are painted and read back as pixels.

Two things the run taught that belong in the port notes:

* `surfman::Error` implements neither `Display` nor `std::error::Error`, so
  rendering-context errors cannot go through `?` into `Box<dyn Error>`.
* Read the frame **before** `present()`. A double-buffered context swaps on
  present, so reading afterwards returns the fresh, blank back buffer; the
  first green-attempt failed its pixel assertion for exactly this reason.

What it does **not** prove: the windowed path (`WindowRenderingContext` on a
real HWND), human-driven mouse/keyboard/scroll, several windows at once, and
memory behaviour. Those need a desktop session, so they stay manual for now;
CI covers the engine, not the input plumbing.

## 8. The Servo backend

`src/browser/engine/servo.rs` plus `servo/{view,session,download}.rs` implement
the same surface `engine/webkit` exposes, so `ui/` compiles against either.
Everything engine-specific lives in `engine/`: no `webkit6` name remains outside
it, and the two backends share `engine/shared.rs` (external-scheme policy, the
confirmation rule, the failure page, the `data:`/JS-string encoders) so that
policy cannot drift between platforms.

| Seam operation | Servo 0.7 call | State |
| --- | --- | --- |
| create a page | `WebViewBuilder::new(servo, SoftwareRenderingContext)` | written |
| frames | `notify_new_frame_ready` → `paint()` → `read_to_image()` → `gdk::MemoryTexture` → `gtk::Picture` | written |
| resize | `WebView::resize` when the picture's allocation changes | written |
| input | `GestureClick` / `EventControllerMotion` / `EventControllerScroll` / `EventControllerKey` → `InputEvent` | written; `Code::Unidentified` |
| load, reload, back/forward | `load`, `reload`, `can_go_back`/`go_back(1)`, `can_go_forward`/`go_forward(1)` | direct |
| URI, title, loading | delegate → `on_uri_changed`, `on_title_changed`, `on_load`, `on_loading_changed` | direct; two load stages |
| zoom | `set_page_zoom` / `page_zoom` | direct |
| close | drop the `WebView` (the delegate holds `Weak<ViewInner>`) | direct |
| authentication | `request_authentication` → `AuthPrompt` | written |
| script dialogs | `EmbedderControl::SimpleDialog` → `gtk::AlertDialog` | written; `prompt()` dismissed |
| context menu | `EmbedderControl::ContextMenu` → `gtk::Popover` (Servo's entries + the interface's) | written |
| notifications | `show_notification` | written |
| external schemes | `request_navigation` + shared policy → `gtk::UriLauncher` | written |
| failure page | `document.write` of the shared page, keeping the URL | written |
| clear cookies / site data | `SiteDataManager`, `NetworkManager::clear_cache` | written |
| find | — | **`FindOutcome { 0, false }`**: the seam has no error channel |
| print, MHTML | — | **`Err(Unsupported)`** |
| downloads | — | **uninhabited `Download`**: the session hook, the save dialog and the manager's interception are not compiled on this backend |
| permissions | `request_permission` | **denies every request**, and logs which; the permission store is not part of this build |
| popups | `request_create_new` | **closed**: the request carries no URL to open |
| private windows, ITP, cookie policy | — | **not isolated, not enforced**: one profile, no policy API |

Windows has no dead code standing in for these features either: what the engine
cannot produce is not compiled. `LoadState::Redirected` exists only where a
backend reports redirects, `browser::permissions` and its settings row only
where a backend can ask for a decision, and `DownloadManager::watch` with the
save dialog only where a transfer can start. `cargo clippy --all-targets -- -D
warnings` is clean on both lines.

Details worth keeping:

* **The pump.** `EventLoopWaker` turns Servo's wake into
  `glib::MainContext::wakeup()`, and one `glib::timeout_add_local(16 ms)` spins
  `Servo::spin_event_loop()` and paints the pages whose delegate saw a frame. A
  wake-driven pump is the optimisation; this timer is what keeps a page
  animating while nobody touches the browser, and it costs one loop spin per
  tick even when nothing is dirty.
* **One engine per process.** `ENGINE` is a thread-local `OnceCell`-style slot
  holding the single `Servo`; the rustls provider is installed once, before it.
* **`Code::Unidentified`.** GDK keyvals do not carry the W3C `event.code`;
  deriving it from the keyval would be wrong on non-US layouts, so keyboard
  events carry `Key::Character`/`Key::Named` only. Sites that read `event.code`
  (games, mostly) see `Unidentified`.
* **Software rendering only.** `set_hardware_acceleration` records the setting
  and changes nothing: the picture bridge is a CPU copy by design.
* **Load stages.** Servo reports started/complete, not WebKit's
  redirected/committed, so `LoadState` carries two of its four values.
* **`Deref<gtk::Widget>`.** `View` derefs to its `gtk::Picture`, which is what
  lets `ui/window.rs` keep its `add_named`, `add_controller` and
  `connect_destroy` calls unchanged.

## 9. CI plan

Kept on `main` unchanged: format, build, test, clippy, MSRV, the Xvfb smokes and
the release build. Added on `windows`:

1. `windows` job: **landed.** `.github/workflows/ci.yml` gained a `windows`
   job — `windows-2022`, native MSVC, NASM, libclang, gvsbuild GTK4 discovered
   from its latest release — that runs `cargo fmt --all --check` and
   `cargo check --locked --target x86_64-pc-windows-msvc`, re-emitting compiler
   errors as annotations. It is gated to pushes on `windows` and PRs into
   `windows`, so `main` never waits on it. The crate compiles under the same
   command on a local Windows machine with that toolchain installed; the job
   still has to prove it on the runner.
2. `windows-servo-poc` job: **done**, `.github/workflows/windows-servo.yml`
   builds the POC and runs its headless self-test. It needs libclang for the
   ANGLE bindings, NASM for `aws-lc-sys`, the ANGLE DLLs beside the binary,
   a dependency cache (key `servo-0.7-angle`) and a long timeout — a cold build
   is ~25 minutes, a cached one ~4.
3. Windows memory smoke: the ported `mem-smoke` schedule with private-commit
   thresholds.
4. A sync check: shared modules must hash identically on `main` and `windows`;
   a divergence fails the job instead of being discovered at port time.

Nothing here exists yet; it lands with the phase that needs it.

## 10. Versioning and synchronisation

* Linux 1.3.0 and Windows 1.3.0 are the same functional generation.
* Minor Linux releases (1.3.1, 1.3.2 …) are cherry-picked into `windows` when
  they touch shared code, and batched otherwise.
* A major Linux release (1.4.0) becomes the new base: `windows` is rebased onto
  that tag, shared fixes come with it, Servo adaptations are reapplied on top,
  the full Windows test set runs, then Windows 1.4.0 is published.
* Windows never publishes a version that has not passed its tests, and Linux
  never receives Servo code.
* No existing branch or tag is rewritten; `windows` is an ordinary branch.

## 11. Phases and the approval gate

| Phase | Content | State |
| --- | --- | --- |
| 0 | audit, branch `windows`, 1.3.0 version bump | done (uncommitted) |
| 1 | this report, dependency proposal, POC source | done (uncommitted) |
| 2 | POC compiled and run on Windows CI; GTK4-on-Windows + Servo texture proven; memory baseline measured | **needs your approval** |
| 3 | engine facade + `webkit` move, no behaviour change, Linux green | **done, uncommitted** (see §5) |
| 4 | seam types, then the `servo` backend: navigation, tabs, permissions, popups, zoom, downloads | **written, uncommitted, never compiled** — see §8; find/print/MHTML/downloads are reported as unsupported instead of faked |
| 5 | the five gaps (find, print, MHTML, DevTools, tracker blocking) | needs a decision each |
| 6 | Windows CI: smokes, memory, packaging | needs approval |

## 12. Open questions

1. **1.3.0 definition** — is `2e5a63c` + version bump the intended 1.3.0?
2. **Print** — defer on Windows, or accept a bitmap-based print?
3. **GTK4 DLL bundling** — is shipping ~40 GTK DLLs next to `axysbrowser.exe`
   acceptable, or is a single-file installer required?
4. **GPU path** — start with CPU texture upload (simple, matches the existing
   cairo decision) and optimise later, or build the GL-sharing path first?
5. **DevTools** — is "page source + JS console" an acceptable Windows scope?
