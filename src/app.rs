use gtk::Application;
use gtk::gio::ApplicationFlags;
use gtk::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;

use crate::app_state::AppState;
use crate::ui::window::{build_window, build_window_with_url};

#[cfg(target_os = "linux")]
mod linux;
#[cfg(windows)]
mod windows;

#[cfg(target_os = "linux")]
use linux as startup;
#[cfg(windows)]
use windows as startup;

/// Turns off GTK's own event and input-feedback sounds. Website audio is
/// WebKit/WebAudio and is deliberately untouched; this only removes feedback
/// such as a beep when a button or destructive action is activated.
fn disable_interface_sounds() {
    let Some(settings) = gtk::Settings::default() else {
        return;
    };

    for key in [
        "gtk-enable-event-sounds",
        "gtk-enable-input-feedback-sounds",
    ] {
        if settings.find_property(key).is_some() {
            settings.set_property(key, false);
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
    startup::prepare();

    let app = Application::builder()
        .application_id("com.axys.axysbrowser")
        .flags(ApplicationFlags::HANDLES_OPEN)
        .build();

    let state_slot = Rc::new(RefCell::new(None::<Rc<AppState>>));

    {
        let state_slot = state_slot.clone();

        app.connect_startup(move |app| {
            register_icon_theme();
            disable_interface_sounds();

            let state = AppState::load(app);

            *state_slot.borrow_mut() = Some(state);
        });
    }

    // History writes are deferred and coalesced (see `HistoryManager`);
    // flush whatever is still pending so a normal exit loses nothing.
    {
        let state_slot = state_slot.clone();

        app.connect_shutdown(move |_| {
            if let Some(state) = state_slot.borrow().as_ref() {
                state.history.flush();
            }
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
                    file.path()
                        .and_then(|path| {
                            bare_input_from_path(&path, std::env::current_dir().ok().as_deref())
                        })
                        .map(|input| {
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

/// Recovers what the user typed from the file GApplication made of it.
///
/// A bare word on the command line (`axysbrowser example.com/page`) reaches
/// `open` as a path under the launch directory. Keeping only the last path
/// segment turned that into a search for `page`; stripping the launch
/// directory gives back `example.com/page`. When the path is not below the
/// current directory (a remote activation from another directory) the last
/// segment is all that is left.
fn bare_input_from_path(path: &std::path::Path, cwd: Option<&std::path::Path>) -> Option<String> {
    let relative = cwd
        .and_then(|cwd| path.strip_prefix(cwd).ok())
        .filter(|relative| !relative.as_os_str().is_empty());

    let text = match relative {
        Some(relative) => relative.as_os_str(),
        None => path.file_name()?,
    };

    Some(text.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::bare_input_from_path;
    use std::path::Path;

    #[test]
    fn a_bare_address_keeps_every_segment_below_the_launch_directory() {
        let cwd = Path::new("/home/user");

        assert_eq!(
            bare_input_from_path(Path::new("/home/user/example.com/page"), Some(cwd)).as_deref(),
            Some("example.com/page")
        );
        assert_eq!(
            bare_input_from_path(Path::new("/home/user/example.com"), Some(cwd)).as_deref(),
            Some("example.com")
        );
        // Another launch directory: only the last segment is recoverable.
        assert_eq!(
            bare_input_from_path(Path::new("/srv/example.com/page"), Some(cwd)).as_deref(),
            Some("page")
        );
        assert_eq!(bare_input_from_path(Path::new("/"), None), None);
    }
}
