//! Test-only history persistence self-test (`AXYS_HISTORY_SMOKE=1`, driven
//! by `.github/history-smoke.sh`). Never active in normal use.
//!
//! History writes are deferred and coalesced, so a visit can pass every unit
//! test and still be lost if the shutdown flush stops running. This task
//! loads a local page through the normal tab pipeline, waits until its own
//! script has marked the load, and quits the application the same way the
//! user does. The script, not this task, judges `history.toml` afterwards:
//! it must hold exactly that visit, with the page's title.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use gtk::glib;
use gtk::prelude::*;
use webkit6::WebView;
use webkit6::prelude::*;

use crate::browser::tabs::Tab;

/// Title the smoke page sets once its own script has run, so quitting waits
/// for a real load instead of a timer.
pub const READY_TITLE: &str = "axys-history-ready";

const LOAD_TIMEOUT: Duration = Duration::from_secs(40);

/// Time for the `Finished` load handler to record the visit after the page's
/// own script runs. Shorter than the two-second save delay on purpose: the
/// write must come from the shutdown flush, not from the deferred timer.
const RECORD_GRACE: Duration = Duration::from_millis(300);

pub struct SmokeContext {
    pub tabs: Rc<RefCell<Vec<Tab>>>,
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

        glib::timeout_future(Duration::from_millis(200)).await;
    }
}

fn ready_view(tabs: &Rc<RefCell<Vec<Tab>>>) -> Option<WebView> {
    tabs.borrow()
        .iter()
        .map(|tab| tab.web_view.clone())
        .find(|view| !view.is_loading() && view.title().as_deref() == Some(READY_TITLE))
}

async fn run(ctx: SmokeContext) {
    if !wait_until(LOAD_TIMEOUT, || ready_view(&ctx.tabs).is_some()).await {
        eprintln!("HISTORY_SMOKE_ERROR page_not_loaded");
        eprintln!("HISTORY_SMOKE_DONE");
        ctx.app.quit();
        return;
    }

    glib::timeout_future(RECORD_GRACE).await;

    eprintln!("HISTORY_SMOKE_VISITED");
    eprintln!("HISTORY_SMOKE_DONE");

    // Quitting here is the point: the visit is dirty and only the shutdown
    // hook in `app::run` writes it.
    ctx.app.quit();
}

/// Starts the self-test when `AXYS_HISTORY_SMOKE` is set.
pub fn start_if_requested(ctx: SmokeContext) {
    if std::env::var("AXYS_HISTORY_SMOKE").is_err() {
        return;
    }

    glib::MainContext::default().spawn_local(run(ctx));
}
