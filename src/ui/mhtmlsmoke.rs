//! Test-only MHTML export self-test (`AXYS_MHTML_SMOKE=1`, driven by
//! `.github/mhtml-smoke.sh`). Never active in normal use.
//!
//! The shell script serves a local page with a stylesheet, an image and a
//! marker. This task loads it through the normal tab pipeline and saves it
//! with the exact WebKit call the "Save page as…" menu uses (`save_to_file`
//! with `SaveMode::Mhtml`), then quits. The script, not this task, judges the
//! file: it must be a multipart MHTML document that embeds the stylesheet and
//! the image instead of linking to them.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use gtk::gio;
use gtk::glib;
use gtk::prelude::*;
use webkit6::prelude::*;
use webkit6::{SaveMode, WebView};

use crate::browser::tabs::Tab;

/// Title the smoke page sets once its own script has run, so saving waits for
/// a real load instead of a timer.
pub const READY_TITLE: &str = "axys-mhtml-ready";

const LOAD_TIMEOUT: Duration = Duration::from_secs(30);
const SAVE_TIMEOUT: Duration = Duration::from_secs(60);

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

fn loaded_view(tabs: &Rc<RefCell<Vec<Tab>>>) -> Option<WebView> {
    tabs.borrow()
        .iter()
        .map(|tab| tab.web_view.clone())
        .find(|view| !view.is_loading() && view.title().as_deref() == Some(READY_TITLE))
}

async fn run(ctx: SmokeContext, path: String) {
    wait_until(LOAD_TIMEOUT, || loaded_view(&ctx.tabs).is_some()).await;

    // `window.load` already waits for stylesheets and images, but WebKit's
    // save walks the document and its resources: give it a moment after the
    // marker instead of racing the last commit.
    glib::timeout_future(Duration::from_millis(500)).await;

    let Some(view) = loaded_view(&ctx.tabs) else {
        eprintln!("MHTML_SMOKE_ERROR page_not_loaded");
        eprintln!("MHTML_SMOKE_DONE");
        ctx.app.quit();
        return;
    };

    let outcome: Rc<RefCell<Option<Result<(), String>>>> = Rc::new(RefCell::new(None));
    let sink = outcome.clone();
    let file = gio::File::for_path(&path);

    view.save_to_file(
        &file,
        SaveMode::Mhtml,
        gio::Cancellable::NONE,
        move |result| {
            *sink.borrow_mut() = Some(result.map_err(|error| error.to_string()));
        },
    );

    wait_until(SAVE_TIMEOUT, || outcome.borrow().is_some()).await;

    match outcome.borrow().clone() {
        Some(Ok(())) => {
            let bytes = std::fs::metadata(&path)
                .map(|metadata| metadata.len())
                .unwrap_or(0);

            eprintln!("MHTML_SMOKE_SAVED path={path} bytes={bytes}");
        }
        Some(Err(error)) => eprintln!("MHTML_SMOKE_ERROR save_failed {error}"),
        None => eprintln!("MHTML_SMOKE_ERROR save_timed_out"),
    }

    eprintln!("MHTML_SMOKE_DONE");
    ctx.app.quit();
}

/// Starts the self-test when `AXYS_MHTML_SMOKE` is set.
pub fn start_if_requested(ctx: SmokeContext) {
    if std::env::var("AXYS_MHTML_SMOKE").is_err() {
        return;
    }

    let Ok(path) = std::env::var("AXYS_MHTML_SMOKE_OUT") else {
        eprintln!("MHTML_SMOKE_ERROR AXYS_MHTML_SMOKE_OUT is not set");
        ctx.app.quit();
        return;
    };

    glib::MainContext::default().spawn_local(run(ctx, path));
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    const SCRIPT: &str = include_str!("../../.github/mhtml-smoke.sh");

    #[test]
    fn the_shell_wait_exceeds_the_internal_budget_with_room_to_spare() {
        // The wait loop is 220 steps of 0.5 s (110 s) and the external
        // backstop is 140 s; the self-test's worst case is 30 + 0.5 + 60 s.
        let internal = super::LOAD_TIMEOUT + super::SAVE_TIMEOUT + Duration::from_millis(500);

        assert!(
            internal < Duration::from_secs(110),
            "internal budget {internal:?} must finish before the shell gives up"
        );
        assert!(
            SCRIPT.contains("timeout -s TERM 140"),
            "the external backstop must stay above the shell wait"
        );
        assert!(
            SCRIPT.contains("seq 1 220"),
            "the shell wait must cover the internal budget"
        );
    }
}
