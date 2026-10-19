use gtk::Application;
use gtk::gio::ApplicationFlags;
use gtk::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;

use crate::app_state::AppState;
use crate::ui::window::{build_window, build_window_with_url};

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

pub fn run() -> gtk::glib::ExitCode {
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
        .flags(ApplicationFlags::HANDLES_OPEN)
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

    // URLs handed by the system (default-browser launches, `xdg-open
    // https://…`, terminal `axysbrowser <url>`): open each in its own
    // window instead of dropping them. GApplication turns bare words into
    // relative file URIs, so only existing paths open as files — the rest
    // resolves like typed address-bar input (domains, searches).
    {
        let state_slot = state_slot.clone();

        app.connect_open(move |app, files, _hint| {
            let Some(state) = state_slot.borrow().clone() else {
                return;
            };

            let mut opened = false;

            for file in files {
                let uri = file.uri().to_string();

                let is_web = uri.starts_with("http://") || uri.starts_with("https://");

                // Internal pages (`axysbrowser axys://tasks`, desktop
                // shortcuts) must be passed through verbatim: treating them
                // as a bare word resolved `axys://tasks` to a bogus URL.
                let is_internal = uri.to_ascii_lowercase().starts_with("axys://");

                let is_existing_file =
                    uri.starts_with("file://") && file.query_exists(None::<&gtk::gio::Cancellable>);

                let target = if is_web || is_internal || is_existing_file {
                    Some(uri)
                } else {
                    file.basename().map(|base| {
                        let input = base.to_string_lossy().to_string();

                        crate::browser::address::resolve_input(&input, &state.search.borrow())
                    })
                };

                if let Some(target) = target {
                    build_window_with_url(app, state.clone(), Some(target));
                    opened = true;
                }
            }

            if !opened {
                build_window(app, state);
            }
        });
    }

    // Forward the real argv so a **cold** launch (`axysbrowser <url>`, a
    // desktop `Exec=… %u`) delivers its URL through `connect_open` above.
    // Passing an empty argv made GApplication fire only `activate`, silently
    // dropping the URL on the first launch (it worked only when an instance
    // was already running, because D-Bus activation carries the files).
    let args: Vec<String> = std::env::args().collect();

    app.run_with_args(&args)
}
