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

/// Whether the unprivileged user and network namespaces WebKit's bubblewrap
/// sandbox needs actually work here. Ubuntu 24.04 restricts them with
/// AppArmor and containers often do too; WebKit then aborts the UI process as
/// soon as a page spawns a web process. Probe once before WebKit starts.
fn sandbox_available() -> bool {
    use std::process::{Command, Stdio};

    Command::new("bwrap")
        .args([
            "--unshare-user",
            "--unshare-pid",
            "--unshare-net",
            "--ro-bind",
            "/",
            "/",
            "true",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

/// WebKit's DMA-BUF renderer is the fast GPU path on real Linux, but it can
/// freeze the whole browser on WSLg and on a few Mesa/X11 setups (frequent
/// re-renders, e.g. a chat editor). Keep WebKit's default (enabled) on real
/// Linux and disable it only under WSL. `AXYS_DMABUF=0|1` forces either
/// side; WebKit's own `WEBKIT_DISABLE_DMABUF_RENDERER`, when already set,
/// always wins. Must run before WebKit/GTK initialize.
fn should_disable_dmabuf(under_wsl: bool, override_value: Option<&str>) -> bool {
    match override_value {
        Some(value) => match value.trim().to_ascii_lowercase().as_str() {
            "0" | "off" | "false" | "never" => true,
            "1" | "on" | "true" | "always" => false,
            _ => under_wsl,
        },
        None => under_wsl,
    }
}

fn configure_rendering(under_wsl: bool) {
    if std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_some() {
        return;
    }

    let override_value = std::env::var("AXYS_DMABUF").ok();

    if should_disable_dmabuf(under_wsl, override_value.as_deref()) {
        unsafe {
            std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
        }
    }
}

/// Makes `icon_name("axysbrowser")` resolve when running from the source
/// tree. Installed builds get the icon from the system hicolor theme (see
/// the install steps in README).
fn register_icon_theme() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/icons");

    if dir.is_dir()
        && let Some(display) = gtk::gdk::Display::default()
    {
        gtk::IconTheme::for_display(&display).add_search_path(&dir);
    }
}

pub fn run() -> gtk::glib::ExitCode {
    let under_wsl = running_under_wsl();

    configure_rendering(under_wsl);

    // WebKit's bubblewrap sandbox frequently fails inside WSL (restricted
    // user namespaces), which kills the web process exactly on pages that
    // spawn workers — Discord being the prime example. The same restriction
    // is on by default on Ubuntu 24.04 (AppArmor) and in many containers,
    // where WebKit aborts the UI process on the first web load. Fall back to
    // no sandbox with a warning instead of crashing; the README explains how
    // to enable it on Ubuntu. `AXYS_FORCE_SANDBOX=1` keeps it on for testing.
    let force_sandbox = std::env::var_os("AXYS_FORCE_SANDBOX").is_some();

    if std::env::var_os("WEBKIT_DISABLE_SANDBOX_THIS_IS_DANGEROUS").is_none() && !force_sandbox {
        if under_wsl {
            unsafe {
                std::env::set_var("WEBKIT_DISABLE_SANDBOX_THIS_IS_DANGEROUS", "1");
            }

            eprintln!("WSL detected: WebKit sandbox disabled for compatibility");
        } else if !sandbox_available() {
            unsafe {
                std::env::set_var("WEBKIT_DISABLE_SANDBOX_THIS_IS_DANGEROUS", "1");
            }

            eprintln!(
                "WebKit sandbox unavailable (bubblewrap cannot create user/network \
                 namespaces); running without it. See the Ubuntu 24.04 notes in README."
            );
        }
    }

    let app = Application::builder()
        .application_id("com.axys.axysbrowser")
        .flags(ApplicationFlags::HANDLES_OPEN)
        .build();

    let state_slot = Rc::new(RefCell::new(None::<Rc<AppState>>));

    {
        let state_slot = state_slot.clone();

        app.connect_startup(move |app| {
            register_icon_theme();

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

                // Internal pages (`axysbrowser axys://resources`, desktop
                // shortcuts) must be passed through verbatim: treating them
                // as a bare word resolved `axys://resources` to a bogus URL.
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

#[cfg(test)]
mod tests {
    use super::should_disable_dmabuf;

    #[test]
    fn dmabuf_follows_the_platform_by_default() {
        assert!(should_disable_dmabuf(true, None));
        assert!(!should_disable_dmabuf(false, None));
    }

    #[test]
    fn dmabuf_override_accepts_common_spellings() {
        for value in ["0", "off", "false", "never", " OFF "] {
            assert!(should_disable_dmabuf(false, Some(value)), "{value}");
        }

        for value in ["1", "on", "true", "always", " ON "] {
            assert!(!should_disable_dmabuf(true, Some(value)), "{value}");
        }
    }

    #[test]
    fn dmabuf_unknown_override_falls_back_to_the_platform() {
        assert!(should_disable_dmabuf(true, Some("maybe")));
        assert!(!should_disable_dmabuf(false, Some("maybe")));
    }
}
