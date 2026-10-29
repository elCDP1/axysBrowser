//! Test-only memory self-test (`AXYS_MEM_SMOKE=1`, driven by
//! `.github/mem-smoke.sh`). Never active in normal use.
//!
//! The harness serves a local page (`AXYS_MEM_SMOKE_URL`) that allocates
//! memory and then sets its title to [`READY_TITLE`]. The test:
//!
//! 1. repeats several open-N-tabs / close-N-tabs cycles, waiting until the
//!    pages have REALLY loaded (title marker, not merely "a timer fired")
//!    before measuring, and until the closed tabs' `WebView`s are finalized
//!    (a GObject weak reference proves nothing in our code still holds
//!    them; WebKit may legitimately keep a cached web process alive, so
//!    process exit is reported but not what is judged);
//! 2. closes the LAST tab of the window after loading a page in it and
//!    reports whether its `WebView` was freed.
//!
//! It only reports raw facts as `MEM_SMOKE_*` lines on stderr; the pass /
//! fail decision lives in the shell script.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use gtk::glib;
use gtk::prelude::*;
use webkit6::WebView;
use webkit6::prelude::WebViewExt;

use super::memprobe;
use crate::browser::tabs::{NewTabCallback, Tab};

/// Title the smoke page sets once its memory is allocated.
pub const READY_TITLE: &str = "axys-smoke-ready";

const CYCLES: usize = 3;
const TABS_PER_CYCLE: usize = 4;
const LOAD_TIMEOUT: Duration = Duration::from_secs(40);
const EXIT_TIMEOUT: Duration = Duration::from_secs(15);
const SETTLE: Duration = Duration::from_secs(3);

pub struct SmokeContext {
    pub tabs: Rc<RefCell<Vec<Tab>>>,
    pub new_tab: NewTabCallback,
    pub close_tab: Rc<dyn Fn(usize)>,
    pub navigate_tab: Rc<dyn Fn(usize, String)>,
    pub close_window: Rc<dyn Fn()>,
    pub app: gtk::Application,
}

async fn wait_until(timeout: Duration, mut condition: impl FnMut() -> bool) -> bool {
    let start = Instant::now();

    loop {
        if condition() {
            return true;
        }

        if start.elapsed() >= timeout {
            return false;
        }

        glib::timeout_future(Duration::from_millis(250)).await;
    }
}

fn loaded_count(tabs: &Rc<RefCell<Vec<Tab>>>, ids: &[usize]) -> usize {
    tabs.borrow()
        .iter()
        .filter(|tab| ids.contains(&tab.id))
        .filter(|tab| {
            !tab.web_view.is_loading() && tab.web_view.title().as_deref() == Some(READY_TITLE)
        })
        .count()
}

fn view_of(tabs: &Rc<RefCell<Vec<Tab>>>, id: usize) -> Option<WebView> {
    tabs.borrow()
        .iter()
        .find(|tab| tab.id == id)
        .map(|tab| tab.web_view.clone())
}

fn freed_count(weaks: &[glib::WeakRef<WebView>]) -> usize {
    weaks.iter().filter(|weak| weak.upgrade().is_none()).count()
}

async fn run(ctx: SmokeContext, url: String) {
    glib::timeout_future(SETTLE).await;

    let first_tab = ctx.tabs.borrow().first().map(|tab| tab.id);

    let Some(first_tab) = first_tab else {
        eprintln!("MEM_SMOKE_ERROR no initial tab");
        ctx.app.quit();
        return;
    };

    for cycle in 1..=CYCLES {
        let (base_kb, base_pids) = memprobe::tree_snapshot();
        let base_web = base_pids.len();

        let ids: Vec<usize> = (0..TABS_PER_CYCLE)
            .map(|_| (ctx.new_tab)(Some(&url)))
            .collect();

        wait_until(LOAD_TIMEOUT, || loaded_count(&ctx.tabs, &ids) == ids.len()).await;

        let loaded = loaded_count(&ctx.tabs, &ids);

        glib::timeout_future(SETTLE).await;

        let (peak_kb, peak_pids) = memprobe::tree_snapshot();
        let peak_web = peak_pids.len();

        let weaks: Vec<glib::WeakRef<WebView>> = ids
            .iter()
            .filter_map(|id| view_of(&ctx.tabs, *id))
            .map(|view| view.downgrade())
            .collect();

        for id in &ids {
            (ctx.close_tab)(*id);
        }

        wait_until(EXIT_TIMEOUT, || freed_count(&weaks) == weaks.len()).await;

        let views_freed = freed_count(&weaks);

        wait_until(Duration::from_secs(5), || {
            memprobe::web_process_pids().len() <= base_web
        })
        .await;

        glib::timeout_future(SETTLE).await;

        let (after_kb, after_pids) = memprobe::tree_snapshot();
        let after_web = after_pids.len();

        eprintln!(
            "MEM_SMOKE_CYCLE cycle={cycle} opened={} loaded={loaded} views_freed={views_freed} \
             base_kb={base_kb} peak_kb={peak_kb} after_kb={after_kb} web_base={base_web} \
             web_peak={peak_web} web_after={after_web}",
            ids.len()
        );
    }

    // Last tab: load a page in the only remaining tab, close it and check
    // that the web process which held the page is released.
    (ctx.navigate_tab)(first_tab, url.clone());

    wait_until(LOAD_TIMEOUT, || loaded_count(&ctx.tabs, &[first_tab]) == 1).await;

    let loaded = loaded_count(&ctx.tabs, &[first_tab]);

    glib::timeout_future(SETTLE).await;

    let (before_kb, before_pids) = memprobe::tree_snapshot();

    let last_weak: Vec<glib::WeakRef<WebView>> = view_of(&ctx.tabs, first_tab)
        .map(|view| vec![view.downgrade()])
        .unwrap_or_default();

    (ctx.close_tab)(first_tab);

    wait_until(EXIT_TIMEOUT, || freed_count(&last_weak) == last_weak.len()).await;

    let view_freed = freed_count(&last_weak);

    glib::timeout_future(SETTLE).await;

    let after_kb = memprobe::tree_memory_kb();
    let still_alive = before_pids
        .iter()
        .filter(|pid| memprobe::is_alive(**pid))
        .count();

    eprintln!(
        "MEM_SMOKE_LASTTAB loaded={loaded} tracked={} view_freed={view_freed} \
         before_kb={before_kb} after_kb={after_kb} old_web_processes={} \
         still_alive={still_alive} tabs_left={}",
        last_weak.len(),
        before_pids.len(),
        ctx.tabs.borrow().len()
    );

    // Window teardown: open two loaded tabs, then close the whole window.
    // Callbacks stored inside the internal pages used to hold the window /
    // `top_stack` / `tabs` strongly (GObject reference cycles), so a closed
    // window's WebViews survived; this checks they are really freed.
    let window_ids: Vec<usize> = (0..2).map(|_| (ctx.new_tab)(Some(&url))).collect();

    wait_until(LOAD_TIMEOUT, || {
        loaded_count(&ctx.tabs, &window_ids) == window_ids.len()
    })
    .await;

    let window_loaded = loaded_count(&ctx.tabs, &window_ids);

    let window_weaks: Vec<glib::WeakRef<WebView>> = window_ids
        .iter()
        .filter_map(|id| view_of(&ctx.tabs, *id))
        .map(|view| view.downgrade())
        .collect();

    // Keep the application (and this task) alive past the last window.
    let guard = ctx.app.hold();

    (ctx.close_window)();

    wait_until(EXIT_TIMEOUT, || {
        freed_count(&window_weaks) == window_weaks.len()
    })
    .await;

    let window_freed = freed_count(&window_weaks);

    eprintln!(
        "MEM_SMOKE_WINDOW loaded={window_loaded} tracked={} views_freed={window_freed}",
        window_weaks.len()
    );

    drop(guard);

    eprintln!("MEM_SMOKE_DONE");

    ctx.app.quit();
}

/// Starts the self-test when `AXYS_MEM_SMOKE` is set.
pub fn start_if_requested(ctx: SmokeContext) {
    if std::env::var("AXYS_MEM_SMOKE").is_err() {
        return;
    }

    let Ok(url) = std::env::var("AXYS_MEM_SMOKE_URL") else {
        eprintln!("MEM_SMOKE_ERROR AXYS_MEM_SMOKE_URL is not set");
        ctx.app.quit();
        return;
    };

    glib::MainContext::default().spawn_local(run(ctx, url));
}
