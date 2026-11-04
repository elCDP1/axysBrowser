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

    // Match the mounts WebKit's own bwrap setup needs, not just the
    // namespaces: /dev, /proc and a private /tmp. A host where the namespaces
    // open but these mounts fail cannot run the sandbox either.
    Command::new("bwrap")
        .args([
            "--unshare-user",
            "--unshare-pid",
            "--unshare-net",
            "--ro-bind",
            "/",
            "/",
            "--dev",
            "/dev",
            "--proc",
            "/proc",
            "--tmpfs",
            "/tmp",
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

/// Whether a boolean-ish environment variable is switched on.
///
/// Mesa reads these with `debug_get_bool_option()`, where an empty value,
/// `0`, `false` and `no` mean off and anything else — junk included — means
/// on. Presence alone is not the test: `LIBGL_ALWAYS_SOFTWARE=0` on a GPU
/// host used to be read as "software GL" and threw away the GPU path.
fn env_flag_is_set(value: Option<&str>) -> bool {
    match value {
        None => false,
        Some(value) => !matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "" | "0" | "false" | "no" | "off"
        ),
    }
}

/// The same rule for `OsStr` environment values. A value that is not valid
/// UTF-8 cannot be proven falsey, so it counts as set (the safe direction for
/// a sandbox switch).
fn env_flag_is_set_os(value: Option<&std::ffi::OsStr>) -> bool {
    match value {
        None => false,
        Some(value) => value
            .to_str()
            .map(|value| env_flag_is_set(Some(value)))
            .unwrap_or(true),
    }
}

/// DRM drivers that expose a node without a usable 3D engine. WSLg's GPU
/// passthrough shows up as `faux_driver` next to a real `renderD*` node, so
/// the node's existence proves nothing; `vgem` and `vkms` are test and
/// virtual devices with no renderer at all.
const SOFTWARE_DRM_DRIVERS: [&str; 3] = ["faux_driver", "vgem", "vkms"];

/// Mesa drivers that rasterize on the CPU.
const SOFTWARE_MESA_DRIVERS: [&str; 4] = ["llvmpipe", "softpipe", "swrast", "swiftshader"];

/// What the environment says about GL, as plain data so the decision is
/// testable without a GPU, a `/dev/dri` tree or a display.
#[derive(Debug, Clone, Copy, Default)]
struct GlEvidence<'a> {
    /// e.g. `renderD128`; `None` when `/dev/dri` has no node.
    render_node: Option<&'a str>,
    /// The node's kernel driver (`amdgpu`, `i915`, `faux_driver`, …).
    drm_driver: Option<&'a str>,
    libgl_always_software: Option<&'a str>,
    /// `GALLIUM_DRIVER` / `MESA_LOADER_DRIVER_OVERRIDE`.
    mesa_driver: Option<&'a str>,
}

/// Whether GL here is Mesa's CPU rasterizer rather than a 3D engine.
fn gl_is_software(evidence: &GlEvidence<'_>) -> bool {
    if env_flag_is_set(evidence.libgl_always_software) {
        return true;
    }

    if evidence
        .mesa_driver
        .map(|driver| driver.trim().to_ascii_lowercase())
        .is_some_and(|driver| SOFTWARE_MESA_DRIVERS.contains(&driver.as_str()))
    {
        return true;
    }

    // Only positive evidence of a real 3D engine keeps GL: a missing node, an
    // unreadable driver or a known non-3D driver all mean software, where
    // cairo renders identically for a fraction of the memory.
    match (evidence.render_node, evidence.drm_driver) {
        (Some(_), Some(driver)) => {
            SOFTWARE_DRM_DRIVERS.contains(&driver.trim().to_ascii_lowercase().as_str())
        }
        _ => true,
    }
}

/// Whether the *interface* (GTK's own rendering) should use the software
/// renderer.
///
/// GTK's default GL renderer is right on a real GPU, but where GL is software
/// (llvmpipe/swrast: VMs, containers, Xvfb, WSLg) it costs far more than it
/// buys for a window this simple. Measured on WSLg with one internal page
/// open: the main process sat at 168 MiB PSS with GL and 58 MiB with cairo.
/// Nothing is dropped feature-wise — every widget still renders — and a real
/// GPU keeps GL. `AXYS_GSK=gl|cairo` overrides the choice; an explicit
/// `GSK_RENDERER` in the environment always wins.
fn should_use_software_renderer(gl_is_software: bool, override_value: Option<&str>) -> bool {
    match override_value {
        Some(value) => match value.trim().to_ascii_lowercase().as_str() {
            "cairo" | "software" | "0" | "off" | "false" | "never" => true,
            "gl" | "gpu" | "1" | "on" | "true" | "always" => false,
            _ => gl_is_software,
        },
        None => gl_is_software,
    }
}

/// A DRM device is a 3D render node only when it is a `renderD*` node.
///
/// A `card*` node is the KMS side of a device and can exist with no 3D engine
/// at all: Azure's `hyperv_drm` exposes `card1` and nothing else, and Mesa
/// cannot even open an EGL device there, so counting it as a GPU made the app
/// pick GL on a machine where GL does not work. It also keeps out `renderD`
/// without a number, which no kernel creates.
fn is_render_node(name: &str) -> bool {
    name.strip_prefix("renderD")
        .is_some_and(|digits| !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit()))
}

/// Every render node under `/dev/dri` with its kernel driver, sorted by node
/// name.
///
/// All of them, not just the first one: a virtual device often gets the lower
/// number (`vgem` on `renderD128`, the real GPU on `renderD129`), and reading
/// only the first would throw away a usable GPU.
fn render_nodes() -> Vec<(String, Option<String>)> {
    let Ok(entries) = std::fs::read_dir("/dev/dri") else {
        return Vec::new();
    };

    let mut nodes: Vec<String> = entries
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| is_render_node(name))
        .collect();

    // Sorted and deduplicated, so a machine with two GPUs always makes the
    // same choice.
    nodes.sort();
    nodes.dedup();

    nodes
        .into_iter()
        .map(|node| {
            let driver = drm_driver(&node);

            (node, driver)
        })
        .collect()
}

/// The node the interface should use: the first one that is readable and is
/// not a known virtual device.
///
/// When nothing is usable, the first node is returned anyway: the decision is
/// still software (`gl_is_software` rejects virtual and unknown drivers), but
/// the log has to name the node it rejected. Otherwise a WSLg `faux_driver`
/// node, or a broken sysfs link, looks like a machine without a GPU.
fn pick_render_node(nodes: &[(String, Option<String>)]) -> Option<&str> {
    let usable = nodes.iter().find(|(_, driver)| {
        driver.as_deref().is_some_and(|driver| {
            !SOFTWARE_DRM_DRIVERS.contains(&driver.trim().to_ascii_lowercase().as_str())
        })
    });

    usable
        .or_else(|| nodes.first())
        .map(|(node, _)| node.as_str())
}

/// `vgem on renderD128, amdgpu on renderD129`, or `no /dev/dri render node`.
fn describe_render_nodes(nodes: &[(String, Option<String>)]) -> String {
    if nodes.is_empty() {
        return "no /dev/dri render node".to_string();
    }

    nodes
        .iter()
        .map(|(node, driver)| match driver {
            Some(driver) => format!("{driver} on {node}"),
            None => format!("unknown driver on {node}"),
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// The kernel driver behind a DRM node, from sysfs.
fn drm_driver(node: &str) -> Option<String> {
    std::fs::read_link(format!("/sys/class/drm/{node}/device/driver"))
        .ok()?
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
}

/// First non-empty Mesa driver override. An empty `GALLIUM_DRIVER` must not
/// hide `MESA_LOADER_DRIVER_OVERRIDE`, which Mesa itself would still honour.
fn mesa_driver_override<'a>(gallium: Option<&'a str>, loader: Option<&'a str>) -> Option<&'a str> {
    [gallium, loader]
        .into_iter()
        .flatten()
        .map(str::trim)
        .find(|value| !value.is_empty())
}

/// Human-readable evidence, so startup logs (and the rest-memory smoke) can
/// tell "no GPU" from "GPU" without repeating the detection rules.
fn gl_description(evidence: &GlEvidence<'_>) -> String {
    if env_flag_is_set(evidence.libgl_always_software) {
        return "LIBGL_ALWAYS_SOFTWARE".to_string();
    }

    if let Some(mesa) = evidence.mesa_driver
        && SOFTWARE_MESA_DRIVERS.contains(&mesa.trim().to_ascii_lowercase().as_str())
    {
        return format!("GALLIUM_DRIVER={mesa}");
    }

    match (evidence.render_node, evidence.drm_driver) {
        (Some(node), Some(driver)) => format!("{driver} on {node}"),
        (Some(node), None) => format!("unknown driver on {node}"),
        _ => "no /dev/dri render node".to_string(),
    }
}

/// Chooses the GTK renderer before GTK initializes (the variable is read once).
fn configure_interface_renderer() {
    let override_value = std::env::var("AXYS_GSK").ok();

    // An explicit `GSK_RENDERER` is the user's own decision: never touch it.
    if let Some(existing) = std::env::var("GSK_RENDERER")
        .ok()
        .filter(|value| !value.is_empty())
    {
        eprintln!("Interface renderer: {existing} (GSK_RENDERER is set; AXYS_GSK is ignored)");

        return;
    }

    let nodes = render_nodes();
    let node = pick_render_node(&nodes);
    let driver = node.and_then(|chosen| {
        nodes
            .iter()
            .find(|(name, _)| name == chosen)
            .and_then(|(_, driver)| driver.clone())
    });
    let libgl = std::env::var("LIBGL_ALWAYS_SOFTWARE").ok();
    let gallium = std::env::var("GALLIUM_DRIVER").ok();
    let loader = std::env::var("MESA_LOADER_DRIVER_OVERRIDE").ok();
    let mesa = mesa_driver_override(gallium.as_deref(), loader.as_deref());

    let evidence = GlEvidence {
        render_node: node,
        drm_driver: driver.as_deref(),
        libgl_always_software: libgl.as_deref(),
        mesa_driver: mesa,
    };

    // With several devices, the log has to say which one was chosen and which
    // ones were ignored; with one it is already fully described.
    let description = if nodes.len() > 1 {
        format!(
            "{}; nodes: {}",
            gl_description(&evidence),
            describe_render_nodes(&nodes)
        )
    } else {
        gl_description(&evidence)
    };

    let software = gl_is_software(&evidence);

    if should_use_software_renderer(software, override_value.as_deref()) {
        unsafe {
            std::env::set_var("GSK_RENDERER", "cairo");
        }

        eprintln!(
            "Interface renderer: cairo (software GL: {description}). Use AXYS_GSK=gl to force GPU rendering."
        );
    } else {
        eprintln!("Interface renderer: gl (hardware GL: {description}).");
    }
}

#[cfg(test)]
mod renderer_tests {
    use super::{
        GlEvidence, describe_render_nodes, env_flag_is_set, gl_is_software, is_render_node,
        mesa_driver_override, pick_render_node, should_use_software_renderer,
    };

    fn nodes(entries: &[(&str, Option<&str>)]) -> Vec<(String, Option<String>)> {
        entries
            .iter()
            .map(|(node, driver)| ((*node).to_string(), driver.map(str::to_string)))
            .collect()
    }

    fn evidence<'a>(node: Option<&'a str>, driver: Option<&'a str>) -> GlEvidence<'a> {
        GlEvidence {
            render_node: node,
            drm_driver: driver,
            ..GlEvidence::default()
        }
    }

    #[test]
    fn a_real_gpu_keeps_the_default_renderer() {
        let gpu = evidence(Some("renderD128"), Some("amdgpu"));

        assert!(!gl_is_software(&gpu));
        assert!(!should_use_software_renderer(false, None));
    }

    #[test]
    fn a_dri_node_alone_does_not_prove_hardware_gl() {
        // WSLg: `/dev/dri/renderD128` exists but its driver is a fake one.
        let wsl = evidence(Some("renderD128"), Some("faux_driver"));

        assert!(gl_is_software(&wsl));
        assert!(should_use_software_renderer(gl_is_software(&wsl), None));

        // Virtual and test devices have no 3D engine either.
        for driver in ["vgem", "VKMS", "faux_driver"] {
            assert!(
                gl_is_software(&evidence(Some("renderD128"), Some(driver))),
                "{driver} must count as software"
            );
        }

        // `card*` nodes never reach that decision: CI has `card1` with
        // hyperv_drm and no render node, and picking GL there failed to open
        // an EGL device at all.
        for node in ["card0", "card1", "hyperv_drm", "renderD", "renderDx"] {
            assert!(!is_render_node(node), "{node} is not a render node");
        }

        for node in ["renderD0", "renderD128", "renderD255"] {
            assert!(is_render_node(node), "{node} is a render node");
        }

        // A node whose driver cannot be read is not evidence of hardware.
        assert!(gl_is_software(&evidence(Some("renderD128"), None)));
    }

    #[test]
    fn an_empty_gallium_driver_does_not_hide_the_loader_override() {
        assert_eq!(
            mesa_driver_override(Some(""), Some("llvmpipe")),
            Some("llvmpipe")
        );
        assert_eq!(
            mesa_driver_override(Some("   "), Some(" llvmpipe ")),
            Some("llvmpipe")
        );
        // A real first value still wins, as before.
        assert_eq!(
            mesa_driver_override(Some("radeonsi"), Some("llvmpipe")),
            Some("radeonsi")
        );
        assert_eq!(mesa_driver_override(None, None), None);

        let evidence = GlEvidence {
            render_node: Some("renderD128"),
            drm_driver: Some("amdgpu"),
            libgl_always_software: None,
            mesa_driver: mesa_driver_override(Some(""), Some("llvmpipe")),
        };

        assert!(
            gl_is_software(&evidence),
            "the loader override must still be used when GALLIUM_DRIVER is empty"
        );
    }

    #[test]
    fn software_gl_falls_back_to_cairo() {
        // No GPU node at all (VMs, containers, Xvfb).
        assert!(gl_is_software(&evidence(None, None)));

        // Mesa told to rasterize on the CPU, by either variable.
        let forced = GlEvidence {
            render_node: Some("renderD128"),
            drm_driver: Some("amdgpu"),
            libgl_always_software: Some("1"),
            mesa_driver: None,
        };

        assert!(gl_is_software(&forced));

        for driver in ["llvmpipe", "softpipe", "swrast", "swiftshader"] {
            let override_evidence = GlEvidence {
                render_node: Some("renderD128"),
                drm_driver: Some("amdgpu"),
                libgl_always_software: None,
                mesa_driver: Some(driver),
            };

            assert!(
                gl_is_software(&override_evidence),
                "{driver} must count as software"
            );
        }

        // The same variable naming a real driver is hardware.
        let radeonsi = GlEvidence {
            render_node: Some("renderD128"),
            drm_driver: Some("amdgpu"),
            libgl_always_software: None,
            mesa_driver: Some("radeonsi"),
        };

        assert!(!gl_is_software(&radeonsi));
    }

    #[test]
    fn libgl_always_software_is_read_by_value_not_by_presence() {
        for value in ["0", "false", "FALSE", "no", "off", ""] {
            assert!(
                !env_flag_is_set(Some(value)),
                "LIBGL_ALWAYS_SOFTWARE={value:?} must not force software"
            );
        }

        for value in ["1", "true", "yes", "on", "anything"] {
            assert!(
                env_flag_is_set(Some(value)),
                "LIBGL_ALWAYS_SOFTWARE={value:?} must force software"
            );
        }

        assert!(!env_flag_is_set(None), "an unset variable is off");

        // End to end: `=0` on a GPU host keeps GL.
        let gpu_with_flag_off = GlEvidence {
            render_node: Some("renderD128"),
            drm_driver: Some("i915"),
            libgl_always_software: Some("0"),
            mesa_driver: None,
        };

        assert!(!gl_is_software(&gpu_with_flag_off));
    }

    #[test]
    fn the_gsk_override_wins_over_the_detected_platform() {
        for value in ["cairo", "SOFTWARE", "0", "off", "never"] {
            assert!(
                should_use_software_renderer(false, Some(value)),
                "{value} must force the software renderer"
            );
        }

        for value in ["gl", "GPU", "1", "on", "always"] {
            assert!(
                !should_use_software_renderer(true, Some(value)),
                "{value} must force GPU rendering"
            );
        }
    }

    #[test]
    fn a_virtual_node_does_not_hide_a_real_gpu() {
        // vgem usually takes the lower render number; reading only the first
        // node used to throw the real GPU away and fall back to cairo.
        let mixed = nodes(&[("renderD128", Some("vgem")), ("renderD129", Some("amdgpu"))]);

        assert_eq!(pick_render_node(&mixed), Some("renderD129"));
    }

    #[test]
    fn an_unreadable_node_does_not_hide_a_real_gpu() {
        let mixed = nodes(&[("renderD128", None), ("renderD129", Some("i915"))]);

        assert_eq!(pick_render_node(&mixed), Some("renderD129"));
    }

    #[test]
    fn the_render_node_summary_names_every_node_and_driver() {
        let mixed = nodes(&[("renderD128", Some("vgem")), ("renderD129", None)]);

        assert_eq!(
            describe_render_nodes(&mixed),
            "vgem on renderD128, unknown driver on renderD129"
        );
        assert_eq!(describe_render_nodes(&[]), "no /dev/dri render node");
    }

    #[test]
    fn an_unreadable_driver_is_reported_instead_of_looking_like_no_gpu() {
        let unknown = nodes(&[("renderD128", None)]);

        // Software either way, but the log must name the node.
        assert_eq!(pick_render_node(&unknown), Some("renderD128"));

        let evidence = super::GlEvidence {
            render_node: Some("renderD128"),
            drm_driver: None,
            ..super::GlEvidence::default()
        };

        assert!(gl_is_software(&evidence));
        assert_eq!(
            super::gl_description(&evidence),
            "unknown driver on renderD128"
        );
    }

    #[test]
    fn virtual_nodes_are_software_but_still_named() {
        let virtual_only = nodes(&[
            ("renderD128", Some("faux_driver")),
            ("renderD129", Some("vkms")),
        ]);

        // Software either way, but the log has to name what it rejected
        // instead of pretending the machine has no render node at all.
        assert_eq!(pick_render_node(&virtual_only), Some("renderD128"));
        assert!(gl_is_software(&evidence(
            Some("renderD128"),
            Some("faux_driver")
        )));

        assert_eq!(pick_render_node(&[]), None);
    }

    #[test]
    fn the_first_real_gpu_wins_so_the_choice_is_stable() {
        let two = nodes(&[("renderD128", Some("amdgpu")), ("renderD129", Some("i915"))]);

        assert_eq!(pick_render_node(&two), Some("renderD128"));
    }

    #[test]
    fn an_unknown_override_falls_back_to_the_detected_platform() {
        assert!(should_use_software_renderer(true, Some("maybe")));
        assert!(!should_use_software_renderer(false, Some("maybe")));
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
    configure_interface_renderer();

    // WebKit's bubblewrap sandbox frequently fails inside WSL (restricted
    // user namespaces), which kills the web process exactly on pages that
    // spawn workers — Discord being the prime example. The same restriction
    // is on by default on Ubuntu 24.04 (AppArmor) and in many containers,
    // where WebKit aborts the UI process on the first web load. Fall back to
    // no sandbox with a warning instead of crashing; the README explains how
    // to enable it on Ubuntu. `AXYS_FORCE_SANDBOX=1` keeps it on for testing;
    // a falsey value no longer forces it, and a falsey
    // `WEBKIT_DISABLE_SANDBOX_THIS_IS_DANGEROUS` is removed because WebKit
    // treats mere presence as "disable".
    let disable_var = std::env::var_os("WEBKIT_DISABLE_SANDBOX_THIS_IS_DANGEROUS");
    let force_sandbox = env_flag_is_set_os(std::env::var_os("AXYS_FORCE_SANDBOX").as_deref());
    let disable_requested = env_flag_is_set_os(disable_var.as_deref());

    if disable_var.is_some() && !disable_requested {
        unsafe {
            std::env::remove_var("WEBKIT_DISABLE_SANDBOX_THIS_IS_DANGEROUS");
        }

        eprintln!(
            "Ignoring falsey WEBKIT_DISABLE_SANDBOX_THIS_IS_DANGEROUS; WebKit would treat \
             mere presence as disabled"
        );
    }

    let disable_var = std::env::var_os("WEBKIT_DISABLE_SANDBOX_THIS_IS_DANGEROUS");

    if disable_var.is_some() && !force_sandbox {
        eprintln!("WebKit sandbox disabled by WEBKIT_DISABLE_SANDBOX_THIS_IS_DANGEROUS");
    } else {
        if disable_var.is_some() && force_sandbox {
            unsafe {
                std::env::remove_var("WEBKIT_DISABLE_SANDBOX_THIS_IS_DANGEROUS");
            }

            eprintln!("AXYS_FORCE_SANDBOX overrides WEBKIT_DISABLE_SANDBOX_THIS_IS_DANGEROUS");
        }

        if !force_sandbox {
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
    use super::{env_flag_is_set_os, should_disable_dmabuf};
    use std::ffi::OsStr;

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

    #[test]
    fn sandbox_switches_ignore_falsey_values() {
        assert!(!env_flag_is_set_os(None));

        for value in ["", "0", "false", "FALSE", "no", "off", " off "] {
            assert!(
                !env_flag_is_set_os(Some(OsStr::new(value))),
                "{value:?} must be falsey"
            );
        }

        for value in ["1", "true", "yes", "on", "junk"] {
            assert!(
                env_flag_is_set_os(Some(OsStr::new(value))),
                "{value:?} must count as set"
            );
        }
    }
}
