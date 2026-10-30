use gtk::prelude::*;
use gtk::{Box, Label, Orientation};

use crate::ui::logo::build_mark;

/// WebKit turns its web-content sandbox off whenever this variable exists,
/// whatever its value. The browser sets it at startup when the sandbox
/// cannot work (WSL, restricted user namespaces), so the About page reads
/// the same source of truth WebKit does.
fn sandbox_disabled(disable_var: Option<&std::ffi::OsStr>) -> bool {
    disable_var.is_some()
}

pub fn build_about() -> Box {
    let page = Box::new(Orientation::Vertical, 16);

    page.set_vexpand(true);
    page.set_hexpand(true);
    page.set_valign(gtk::Align::Center);
    page.set_halign(gtk::Align::Center);

    let mark = build_mark(72);

    mark.set_halign(gtk::Align::Center);

    let title = Label::new(Some("axysBrowser"));
    title.add_css_class("title-1");

    let version = Label::new(Some(&format!("axysBrowser {}", env!("CARGO_PKG_VERSION"))));
    version.add_css_class("dim-label");

    let description = Label::new(Some(&rust_i18n::t!("about.description")));

    description.set_wrap(true);
    description.set_max_width_chars(60);
    description.set_justify(gtk::Justification::Center);

    let author = Label::new(Some(&rust_i18n::t!("about.author")));

    author.add_css_class("title-3");

    let copyright = Label::new(Some(&rust_i18n::t!("about.copyright")));

    copyright.add_css_class("dim-label");

    let license = Label::new(Some("AXYS Attribution License 1.0"));

    license.add_css_class("dim-label");

    let sandbox_off =
        sandbox_disabled(std::env::var_os("WEBKIT_DISABLE_SANDBOX_THIS_IS_DANGEROUS").as_deref());

    let sandbox = Label::new(Some(&if sandbox_off {
        rust_i18n::t!("about.sandbox_off").to_string()
    } else {
        rust_i18n::t!("about.sandbox_on").to_string()
    }));

    sandbox.set_wrap(true);
    sandbox.set_max_width_chars(60);
    sandbox.set_justify(gtk::Justification::Center);
    sandbox.add_css_class(if sandbox_off { "error" } else { "dim-label" });

    let notice = Label::new(Some(&rust_i18n::t!("about.notice")));

    notice.set_wrap(true);
    notice.set_max_width_chars(60);
    notice.set_justify(gtk::Justification::Center);
    notice.add_css_class("dim-label");

    page.append(&mark);

    page.append(&title);
    page.append(&version);
    page.append(&description);
    page.append(&author);
    page.append(&copyright);
    page.append(&license);
    page.append(&sandbox);
    page.append(&notice);

    page
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;

    use super::sandbox_disabled;

    #[test]
    fn sandbox_is_reported_disabled_whenever_the_variable_exists() {
        assert!(!sandbox_disabled(None));
        assert!(sandbox_disabled(Some(OsStr::new("1"))));
        assert!(
            sandbox_disabled(Some(OsStr::new("0"))),
            "WebKit ignores the value"
        );
        assert!(sandbox_disabled(Some(OsStr::new(""))));
    }
}
