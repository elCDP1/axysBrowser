//! Windows startup environment.
//!
//! The Linux startup probes have no Windows counterpart: there is no
//! `/dev/dri` tree to read Mesa from and no bubblewrap sandbox to disable.
//! What is left is the interface renderer, and GTK does not reach the renderer
//! that works here on its own.
//!
//! gvsbuild builds GTK with DirectComposition patched out, so GSK's GL
//! renderer cannot realize a `GdkWin32Toplevel` on this platform and fails
//! with "OpenGL requires Direct Composition" for every surface. Measured with
//! `GSK_DEBUG=renderer` — GTK 4 rejects `GTK_DEBUG=renderer` as an unknown
//! value — GTK then settles on `GskCairoRenderer`, which is what this port has
//! always drawn with. Asking for it up front keeps that
//! renderer and skips a failed GL realization per surface. An explicit
//! `GSK_RENDERER` in the environment still wins.
pub(super) fn prepare() {
    let existing = std::env::var("GSK_RENDERER")
        .ok()
        .filter(|value| !value.trim().is_empty());

    if let Some(existing) = existing {
        eprintln!("Interface renderer: {existing} (GSK_RENDERER is set)");

        return;
    }

    unsafe {
        std::env::set_var("GSK_RENDERER", "cairo");
    }

    eprintln!(
        "Interface renderer: cairo (GSK's GL renderer needs Direct Composition, \
         which this GTK build does not have)"
    );
}
