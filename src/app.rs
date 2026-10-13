use gtk::Application;
use gtk::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;

use crate::app_state::AppState;
use crate::ui::window::build_window;

fn running_under_wsl() -> bool {
    if std::env::var("WSL_DISTRO_NAME").is_ok() || std::env::var("WSL_INTEROP").is_ok() {
        return true;
    }

    if let Ok(version) = std::fs::read_to_string("/proc/version") {
        let lower = version.to_ascii_lowercase();

        if lower.contains("microsoft") || lower.contains("wsl") {
            return true;
        }
    }

    false
}

pub fn run() {
    // Known WebKitGTK6 + Mesa DMA-BUF renderer bug: on some Linux setups
    // (notably Mesa/Intel/AMD combos also affecting XFCE/X11 sessions), the
    // browser freezes entirely — not just the tab — on pages that re-render
    // very frequently while interacting with them, like a chat app's text
    // editor. Disabling the DMA-BUF renderer trades a bit of GPU-accelerated
    // compositing for stability, and is the standard fix distros/WebKit
    // itself recommend for this. Must be set before WebKit initializes,
    // so this has to happen before the GTK Application is even built.
    unsafe {
        std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
    }

    // WebKit's bubblewrap sandbox frequently fails inside WSL (restricted
    // user namespaces), which kills the web process exactly on pages that
    // spawn workers — Discord being the prime example. Disable only there;
    // real Linux keeps the sandbox.
    if running_under_wsl() {
        unsafe {
            std::env::set_var("WEBKIT_DISABLE_SANDBOX_THIS_IS_DANGEROUS", "1");
        }

        eprintln!("WSL detected: WebKit sandbox disabled for compatibility");
    }

    let app = Application::builder()
        .application_id("com.axys.axysbrowser")
        .build();

    let state_slot = Rc::new(RefCell::new(None::<Rc<AppState>>));

    {
        let state_slot = state_slot.clone();

        app.connect_startup(move |app| {
            let state = AppState::load(app);

            *state_slot.borrow_mut() = Some(state);
        });
    }

    {
        let state_slot = state_slot.clone();

        app.connect_activate(move |app| {
            let Some(state) = state_slot.borrow().clone() else {
                return;
            };

            build_window(app, state);
        });
    }

    // Never forward process argv to GApplication: with default flags it would
    // treat any argument (e.g. a URL from a .desktop launch) as a file to
    // open and refuse to activate, so no window would appear at all.
    app.run_with_args::<&str>(&[]);
}
