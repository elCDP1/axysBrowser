# Phase 4: the engine seam API

Status: **steps 1–3 landed** (over WebKit, no behaviour change):
`View`, `WeakView`, `Session` and `ViewConfig` exist, `Session` is built once
per window and is the only thing that creates pages, and `View` now derefs to
`gtk::Widget`, not to the engine's view.

The whole page surface moved with step 3, ahead of the plan's order, because
the flip is what makes the rest possible: navigation, zoom, source, print,
MHTML export, find (`find`/`find_stop`), the settings that change live, and
the signals as `on_load`, `on_uri_changed`, `on_title_changed`,
`on_loading_changed`, `on_context_menu` (handlers live with the page).

Steps 5–7 landed too: `Session` creates pages and owns cookies, ITP, site
data and its download stream; `Download` is the neutral transfer handle the
download manager tracks; `AuthPrompt`/`Credentials` carry an authentication
request and its single answer; `AuthenticateHandler` is
`Rc<dyn Fn(&View, AuthPrompt)>`. **No file outside `src/browser/engine/`
names the engine** — `grep -rn webkit6 src | grep -v engine/` is empty. What
remains is the Servo backend itself (§8), which now has a closed contract to
implement:

| Seam item | What the Servo backend must do |
| --- | --- |
| `Session` | `Servo` + preferences (UA, cache/data dirs), one session per window; cookies and site data from Servo's storage (or `Unsupported`) |
| `View` | `WebViewBuilder` + rendering context, frame → GTK texture, input events, load/zoom/title/uri |
| `on_*` | Servo's `WebViewDelegate` callbacks, stored on the page like the WebKit backend does |
| `find` | Servo has no find API: report `Unsupported` |
| `print`, `save_mhtml`, `source` | source via `evaluate_javascript`; print and MHTML `Unsupported` |
| `Download` | Servo has no download manager: no download hook is built on that backend |
| `AuthPrompt` | Servo has no auth signal: nothing to prompt |

## 0. Rules

1. Linux behaviour does not change. Every step below is a signature change
   plus a delegation, and the 286 existing tests must stay green.
2. No engine type appears in any signature outside `browser/engine`. Today
   16 files name `webkit6`; the target is `browser/engine` and nothing else,
   which a CI check enforces (step 8).
3. A capability a backend does not have is **reported, never faked**
   (`Capabilities`, `Unsupported`). The interface disables the action instead
   of offering a button that silently does nothing.
4. Each step is its own green commit: `fmt`, `build --locked`, `test`,
   `clippy -D warnings`, `doc`.

## 1. What the interface takes from WebKit today

| File | Engine surface used | Seam replacement |
| --- | --- | --- |
| `browser/tabs.rs` | `WebView` field, clone | `engine::View` |
| `ui/window.rs` | `WebView::builder().network_session()`, GTK placement, `connect_context_menu`, `LoadEvent` state machine, `uri/title/is_loading`, `zoom_level/set_zoom_level`, `evaluate_javascript`, `main_resource().data()`, `PrintOperation`, `save_to_file(SaveMode::Mhtml)`, `stop_loading/try_close/terminate_web_process`, `WebViewExt::settings()` | `Session::view()`, `View: Deref<Target = gtk::Widget>`, `on_*` subscriptions, `LoadState`, `eval`, `source`, `print`, `save_mhtml`, `close` |
| `ui/toolbar.rs` | `WebView` type, `uri/title/is_loading/zoom_level` | `View`, `WeakView` |
| `ui/navigation.rs` | `WebView`, `is_loading` (watchdog), `engine::log_safe_uri` | `View`, `browser::util::log_safe_uri` |
| `ui/shortcuts.rs` | `WebView` in the shortcut context | `View` |
| `ui/findbar.rs` | `FindController`, `FindOptions`, `connect_found_text`, `connect_failed_to_find_text`, `search/search_next/search_previous`, handler ids kept for disconnecting | `View::find(request, done)` (one call for search, next and previous), `View::find_stop`, `FindOutcome`, `Subscription` |
| `ui/auth.rs` | `AuthenticationRequest`, `Credential`, `CredentialPersistence` | `AuthChallenge`, `AuthAnswer` |
| `browser/downloads.rs` | `NetworkSession::connect_download_started`, `Download` (`connect_decide_destination`, `connect_estimated_progress_notify`, `connect_finished`), `received_data_length`, `DownloadError` | `Session::on_download`, `engine::Download`, `DownloadError` |
| `app_state.rs` | `NetworkSession`, `cookie_manager().set_persistent_storage(Sqlite)`, `set_itp_enabled`, `website_data_manager().clear(WebsiteDataTypes::ALL)` | `Session::save_cookies_to`, `set_tracking_prevention`, `clear_data` |
| `ui/{memsmoke,historysmoke,mhtmlsmoke}.rs` | `WebView`, `glib::WeakRef<WebView>` | `View`, `WeakView` (these stay Linux-only) |
| `browser/bookmarks.rs`, `ui/bookmarks.rs`, `internal/pages/bookmarks.rs` | `webkit6::gdk` — that is GDK, re-exported by WebKitGTK | `gtk::gdk` (step 0) |
| `app.rs` | `engine::running_under_wsl` | `platform::linux::running_under_wsl` (step 0) |

Two exports in the current facade are not engine facts and move out in step 0:
`log_safe_uri` is string redaction and `running_under_wsl` is a platform probe.

## 2. The seam types

Signature level only; no bodies. `gtk` here means `gtk4`, which both platforms
already link.

```rust
/// A live page. Cloneable handle; the widget the interface puts in its stack.
#[derive(Clone)]
pub struct View(/* backend handle */);
pub struct WeakView(/* backend weak handle */);

impl std::ops::Deref for View {
    type Target = gtk::Widget; // set_hexpand, is_mapped, grab_focus, add_controller…
}

impl View {
    // Navigation
    pub fn load(&self, uri: &str);
    pub fn load_html(&self, html: &str, base: Option<&str>);
    pub fn reload(&self);
    pub fn stop(&self);
    /// Leaves the page: stops loading, closes the view, ends the web process.
    pub fn close(&self);
    pub fn uri(&self) -> Option<String>;
    pub fn title(&self) -> Option<String>;
    pub fn is_loading(&self) -> bool;
    pub fn can_go_back(&self) -> bool;
    pub fn go_back(&self);
    pub fn can_go_forward(&self) -> bool;
    pub fn go_forward(&self);

    // Zoom
    pub fn zoom(&self) -> f64;
    pub fn set_zoom(&self, level: f64);

    // Script and content
    pub fn eval(&self, script: &str, done: impl FnOnce(Result<JsValue, EvalError>) + 'static);
    pub fn source(&self, done: impl FnOnce(String) + 'static);

    // Find: one entry point for search, next and previous, always answered
    // through the callback (capability-gated by `Capabilities::find`).
    pub fn find(&self, request: FindRequest, done: impl FnOnce(FindOutcome) + 'static);
    pub fn find_stop(&self);

    // Page-scoped tools; `Err(Unsupported)` when the backend lacks them
    pub fn print(&self, parent: Option<&gtk::Window>) -> Result<(), Unsupported>;
    pub fn save_mhtml(
        &self,
        file: &gtk::gio::File,
        done: impl FnOnce(Result<(), SaveError>) + 'static,
    ) -> Result<(), Unsupported>;

    // Signals, as disposable subscriptions
    pub fn on_load(&self, f: impl Fn(LoadState) + 'static) -> Subscription;
    pub fn on_uri_changed(&self, f: impl Fn(&str) + 'static) -> Subscription;
    pub fn on_title_changed(&self, f: impl Fn(Option<&str>) + 'static) -> Subscription;
    pub fn on_loading_changed(&self, f: impl Fn(bool) + 'static) -> Subscription;
    pub fn on_destroy(&self, f: impl Fn() + 'static) -> Subscription;
    pub fn on_context_menu(
        &self,
        f: impl Fn(&gtk::Widget) -> bool + 'static, // append entries, return handled?
    ) -> Subscription;

    pub fn downgrade(&self) -> WeakView;
}

impl WeakView {
    pub fn upgrade(&self) -> Option<View>;
}

/// A persistent or ephemeral browsing session (cookies, cache, site data).
#[derive(Clone)]
pub struct Session(/* backend handle */);

impl Session {
    pub fn persistent(itp: bool) -> Session;
    pub fn ephemeral() -> Session;
    pub fn view(&self, config: &ViewConfig) -> View;
    pub fn set_cookie_policy(&self, policy: CookiePolicy);
    pub fn save_cookies_to(&self, path: &std::path::Path); // Sqlite, Linux today
    pub fn set_tracking_prevention(&self, enabled: bool);
    pub fn clear_data(&self, kinds: DataKinds, done: impl FnOnce() + 'static);
    pub fn on_download(&self, f: impl Fn(Download) + 'static) -> Subscription;
}

pub struct ViewConfig {
    pub developer_tools: bool,
    pub hardware_acceleration: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoadState {
    Started,
    // WebKit only: Servo reports started and finished, so this variant is not
    // compiled on that backend.
    Redirected,
    Committed,
    Finished,
}

/// One find action. `First` starts a search, `Next`/`Previous` move inside
/// it, so the find bar calls the same function for typing and for the arrows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FindRequest {
    pub query: String,
    pub step: FindStep,
    pub wrap: bool,
    pub case_sensitive: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FindStep {
    First,
    Next,
    Previous,
}

/// There is deliberately no "current index": WebKit's `found-text` signal
/// reports the match count only, so a seam field for it could not be honoured
/// by the Linux backend.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FindOutcome {
    pub matches: u32,
    pub found: bool,
}

pub enum JsValue {
    String(String),
    Number(f64),
    Bool(bool),
    Null,
    Undefined,
    Array(Vec<JsValue>),
    Other,
}
impl JsValue {
    pub fn as_str(&self) -> &str; // today's `to_str()`
}

pub enum EvalError {
    ScriptException(String),
    NoResult,
}

pub struct AuthChallenge(/* backend handle */);
impl AuthChallenge {
    pub fn host(&self) -> &str;
    pub fn realm(&self) -> &str;
    pub fn is_proxy(&self) -> bool;
    pub fn is_retry(&self) -> bool;
    pub fn answer(self, user: &str, password: &str, persist: CredentialLifetime);
    pub fn cancel(self);
}

pub enum CredentialLifetime {
    None,
    Session,
    Permanent,
}

pub struct Download(/* backend handle */);
impl Download {
    pub fn suggested_filename(&self) -> String;
    pub fn received_bytes(&self) -> u64;
    pub fn estimated_progress(&self) -> f64;
    pub fn set_destination(&self, path: &std::path::Path);
    pub fn cancel(&self);
    pub fn on_progress(&self, f: impl Fn() + 'static) -> Subscription;
    pub fn on_finished(&self, f: impl Fn(Result<(), DownloadError>) + 'static) -> Subscription;
}

pub enum DownloadError {
    Cancelled,
    Failed(String),
}

pub enum CookiePolicy {
    Always,
    Never,
    NoThirdParty,
}

pub struct DataKinds(/* bitflags: cookies, cache, storage, all */);

/// Dropping it disconnects; `disconnect()` does it eagerly, which is what the
/// find bar needs when a tab closes.
pub struct Subscription(/* handler id or registry key */);
impl Subscription {
    pub fn disconnect(self);
}

/// Features the backend does not implement, reported instead of faked.
#[derive(Clone, Copy, Debug)]
pub struct Capabilities {
    pub print: bool,
    pub save_mhtml: bool,
    pub devtools: bool,
    pub tracking_prevention: bool,
    pub find: bool,
}
pub struct Unsupported; // Display: "not supported by this engine"

pub enum SaveError {
    Unsupported,
    Io(std::io::Error),
}
```

`BrowserEngine` keeps its role and loses its `WebView` parameter: `configure`
becomes `ViewConfig`, `wire_view(&View, …)` keeps its five callbacks (popups,
notifications, authentication, permissions, clipboard), and
`capabilities()` is added.

### 2.1 Find: checked against today's find bar

`ui/findbar.rs` was read line by line before fixing the shape. It does six
things, and `find(FindRequest, done)` covers all of them:

| What the bar does today | How the seam expresses it |
| --- | --- |
| `search(text, WRAP_AROUND, 500)` while typing | `FindRequest { step: First, wrap: true, max_matches: 500, .. }` |
| Enter / ▸ / ▾ → `search_next` / `search_previous` | `FindRequest { step: Next / Previous, .. }` |
| Match count from `found-text` | `FindOutcome { matches, .. }` in the callback |
| `failed-to-find-text` → count `0` + error style | `FindOutcome { matches: 0, found: false }` |
| Empty query / close / hide / tab switch away → `search_finish` | `View::find_stop()` (clears highlights and any pending result) |
| Generation counter + "is this the active view's controller" guard | the backend's pending-result slot; see below |

Three things the first draft missed and now belong to the contract:

1. **`max_matches`.** The bar asks WebKit for at most 500 matches. Without the
   field the seam would change the displayed count, so `FindRequest` carries it.
2. **Correlating a result with its query.** WebKit's `found-text` reports a
   count, not a query id, so a late result for an older query could land on the
   newer callback. The backend keeps `(query, callback)` and only delivers when
   `FindController::search_text()` still equals that query; a new `find` call
   replaces the pending pair, which is what the bar's generation counter did.
3. **`forget_hook` / `disconnect_hook` disappear.** They exist only because one
   `FindController` per view needs its handlers unhooked when the bar moves to
   another tab. With the hook living inside the view, closing the tab drops the
   view, the controller and the handlers together — fewer moving parts and no
   cross-tab leak. `window.rs` keeps calling `find_stop` through the existing
   close path.

### 2.2 Transitional `Deref`

The `Deref` below is the destination. While the migration walks the interface
file by file, the backend's `View` derefs to `webkit6::WebView` instead, which
is why `Tab`, the toolbar, the navigation watchdog, the shortcut context and
the window's active-view slots could switch type with no behaviour change and
no rewritten logic. Four call sites still need the engine view explicitly
(`settings()` twice, `add_named`, `PrintOperation`) and go through a
`pub(crate) View::webkit()` that exists only for this: each of them is step 3
work. When the last one is gone, the `Deref` flips to `gtk::Widget` and the CI
guard in step 8 can pass.

### What stays out of the seam

* No GObject properties, `gio::Cancellable` or `webkit6::*` in any signature.
* No `String`-typed state that the interface has to parse (no "load-status
  strings").
* No engine handle escapes behind `View`/`Session`: the backend module is the
  only place that can unwrap it.

## 3. Ownership and lifetimes

* `View` is a cloneable handle, not a `glib::Object`. Every
  `glib::WeakRef<WebView>` in the interface (`last_web_view` in `ui/window.rs`,
  `memsmoke`) becomes `WeakView`. `Deref<Target = gtk::Widget>` keeps GTK calls
  (`.is_mapped()`, `.set_hexpand()`, `.add_controller()`) reading exactly as
  they do today.
* The widget identity has to be preserved: for WebKit the `View` *is* the
  `WebView`; for Servo the view word wraps the page container. The stack, the
  tab container and the find bar must receive the same widget either way.
* Callbacks registered on a view currently capture `glib::WeakRef`s of the view
  or the window. The seam must keep that: `on_*` closures capture `WeakView`,
  never `View`, or closed tabs stay alive (that is the memory feature).
* No delegate may own window state — the POC's `Weak<AppState>` rule.
* WebKit can emit signals during `load()`; the existing `idle_add_local_once`
  defers (popups) stay where they are.

## 4. Capabilities: the five gaps, honestly

| Feature | WebKit | Servo 0.7 | Seam |
| --- | --- | --- | --- |
| Print | `PrintOperation` | none | `capabilities.print`, `View::print -> Unsupported` |
| MHTML | `save_to_file(Mhtml)` | none | `capabilities.save_mhtml`; phase 5 builds a serialiser in shared code |
| DevTools | inspector | none | `capabilities.devtools`; phase 5 scope = source + JS console |
| Tracking prevention | ITP | none | `capabilities.tracking_prevention`; phase 5 = own request blocker |
| Find | `FindController` | no API | `capabilities.find`; phase 5 = injected script |

The menu already has Print…, Save page as…, Developer tools and the tracker
toggle; with this shape each entry is disabled when the running backend reports
`false`, instead of opening a dialog that cannot work. Linux keeps all five.

## 5. Migration order (one green commit each)

Steps 1–3 are a single commit with three reviewable slices: rustc will not let
an unused seam type exist alongside `-D warnings`, so the types and their first
call sites have to arrive together.

0. **Non-seam cleanups.** `log_safe_uri` → `browser/util.rs`;
   `running_under_wsl` → `platform::linux`; `webkit6::gdk` → `gtk::gdk` in the
   three files that only want GDK. Removes four files from the WebKit list with
   no seam types at all.
1. **Seam primitives, `View`/`WeakView` and the view-creating half of
   `Session`, each landing with its first consumer.** Checked against rustc
   before writing it: a `pub` item inside a private module chain (which
   `browser::engine::webkit` is) *is* reported as dead code, and a `pub use`
   with no user trips `unused_imports` — so a batch of types nobody calls
   cannot pass `clippy -D warnings`, and `#[allow(dead_code)]` is not an
   acceptable way out. Each type therefore lands with the step that uses it:
   `View`/`WeakView` and `ViewConfig` with step 2, the rest of `Session` with
   step 7, `Download` with step 6, `AuthChallenge` with step 5, `LoadState`
   and `FindOutcome` with steps 3 and 4.
2. **Type-only migration** of `browser/tabs.rs`, `ui/toolbar.rs`,
   `ui/navigation.rs`, `ui/shortcuts.rs`, plus tab creation in `ui/window.rs`
   through `Session::view`. Mechanical; no logic touched.
3. **`ui/window.rs`**: `LoadEvent` → `LoadState`, zoom, context-menu
   subscription, teardown (`close()`), view-source (`View::source`), and
   capability-gated Print/Save entries.
4. **`ui/findbar.rs`**: `FindController` → `View::find(request, done)` +
   `View::find_stop` + `Subscription::disconnect` when a tab closes. The
   find bar keeps its debounce and its "no matches" UI; only the plumbing
   changes, from a controller object to one call with a result callback.
5. **`ui/auth.rs`**: `AuthenticationRequest` → `AuthChallenge`.
6. **`browser/downloads.rs`**: `Session::on_download` + `engine::Download`.
7. **`app_state.rs`**: session creation, cookie storage, ITP toggle, clear-data.
   Plus `ui/{memsmoke,historysmoke,mhtmlsmoke}` swapped to `View`/`WeakView`.
8. **Close the door.** `webkit6` must not appear outside `browser/engine`; a CI
   step fails the build if it does:
   `! grep -rn --include=*.rs webkit6 src | grep -v '^src/browser/engine/'`.

At every step: the 286 tests are the Linux regression net, and the smokes in
CI (`gui-smoke`, `mem-smoke`, `mhtml-smoke`, `history-smoke`, `rest-smoke`)
cover the behaviour unit tests cannot.

## 6. Tests the seam makes possible

* **A fake backend** (`#[cfg(test)] engine::testing::Fake`) implementing `View`
  and `Session` with no engine at all: the tab/state logic in `ui/` becomes
  testable headlessly, on both platforms, without a display or a GPU. That is
  the only way Windows UI logic gets tested in CI.
* **Pure mapping tests**: `LoadState` → toolbar flags, capability gating
  (Print/Save must be disabled *before* a dialog opens), `FindOutcome`
  formatting for the find bar, `WeakView::upgrade()` after `close()`.
* **Windows seam smoke** (`--self-test`): the proven POC path — software
  rendering context, load a `data:` page, read a value back through `eval`,
  assert pixels. Runs on `windows-2022` with no display.

## 7. Risks

* `Deref<Target = gtk::Widget>` means a `View` is not a GObject: every
  `glib::WeakRef<WebView>` must be replaced rather than adapted, and any
  `glib` property access on the widget has to go through the widget.
* The context menu is WebKit's own today; the seam takes a menu widget to
  append to, which Servo fills with its own entries — behaviour will differ.
* `save_mhtml` and `print` are dialogs owned by the engine; gating has to
  happen before the dialog opens, not when it returns.
* Servo needs one rendering context per view (servo/servo#35536), so tab
  switching cannot swap pages inside one surface; that is what makes the
  per-tab container in §5 step 3 matter.

## 8. Phase 4b: what the Servo backend has to build

In order, each behind the seam so the interface never changes again:

1. `Session`: `ServoBuilder` + preferences (UA, cache/data dirs), one `Servo`
   per process.
2. `View`: one `RenderingContext` per tab (software context first), a
   `WebViewBuilder` with a delegate holding `Weak<ViewInner>`.
3. Frames: `notify_new_frame_ready` → `paint()` → `read_to_image()` →
   `GdkMemoryTexture` → `GtkPicture` inside the tab container (CPU copy first;
   the GPU-shared path is an optimisation).
4. Input: GTK controllers → Servo `InputEvent` (click, motion, wheel, key),
   focus and resize forwarding.
5. Load state, URI, title and JS results: `WebViewDelegate` → `LoadState`,
   `on_uri_changed`, `on_title_changed`, `JsValue`.
6. Zoom (`set_page_zoom`), `close()` (`set_focused(false)` + drop), and the
   capabilities table above.
7. Memory: `PrivateUsage` from `PROCESS_MEMORY_COUNTERS_EX` as the gate,
   working set reported only, plus `GetGuiResources` for handles.

### 8.1 What was built

Steps 1–6 are written (`engine/servo.rs`, `servo/{view,session,download}.rs`),
against the published 0.7 sources rather than from memory. Step 7 is not: it
needs the Windows platform module, and `ui/memprobe.rs` still reads `/proc`.

Four places where the plan and the real API disagreed, and what was done:

* **`Capabilities` was never added.** The draft above has a table of "what this
  backend cannot do"; the seam that shipped reports that per call instead
  (`Err(Unsupported)` for print/MHTML, an empty `FindOutcome` for find), so no
  file has to consult a capability table before offering an action. Adding one
  belongs with the UI work that would ask it.
* **`wire_view` takes five parameters on WebKit and four on Servo.** Clipboard
  reads are accepted and unused on Servo. Permissions are not accepted at all:
  the shared permission dialog is built on WebKit's request kinds, Servo 0.7
  reports its own `PermissionFeature` set with no common shape, and a store no
  code can consult would be dead code, so it is not compiled there. The backend
  denies every request and says which.
* **`Deref` flipped to `gtk::Widget` as planned**, over a `gtk::Picture`, so the
  interface's `add_named`, `add_controller` and `connect_destroy` calls work
  unchanged.
* **The error page is not a navigation.** Servo has no `load_alternate_html`,
  so the failure page is written into the current document with
  `document.open()/write()/close()`, which keeps the failing URL in the address
  bar and in `view.uri()` — which is also what the history check expects.
