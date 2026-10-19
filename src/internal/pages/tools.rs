use gtk::prelude::*;
use gtk::{Box, Button, Label, Orientation, ScrolledWindow, Separator};
use std::rc::Rc;

use crate::browser::history::HistoryManager;

/// Page-scoped / system actions the Tools page can trigger. Each one maps
/// to a `win.*` action owned by the window, so the same code runs whether
/// it is invoked from here or from the hamburger menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolAction {
    ZoomIn,
    ZoomOut,
    ZoomReset,
    OpenBookmarks,
    OpenTasks,
    OpenDownloadsFolder,
}

pub struct ToolsDeps {
    pub on_downloads: Rc<dyn Fn()>,
    pub on_history: Rc<dyn Fn()>,
    pub on_clear_browsing_data: crate::app_state::ClearBrowsingData,
    pub history: HistoryManager,
    /// Runs a page/system action (maps to a `win.*` action).
    pub on_tool: Rc<dyn Fn(ToolAction)>,
    /// Current zoom level of the active web view, if any.
    pub zoom_level: Rc<dyn Fn() -> Option<f64>>,
}

fn section_title(key: &str) -> Label {
    let label = Label::new(Some(&rust_i18n::t!(key)));

    label.add_css_class("title-3");

    label.set_halign(gtk::Align::Start);

    label
}

/// A flat button that runs a [`ToolAction`] when clicked.
fn tool_button(label_key: &str, action: ToolAction, on_tool: &Rc<dyn Fn(ToolAction)>) -> Button {
    let button = Button::with_label(&rust_i18n::t!(label_key));

    button.set_halign(gtk::Align::Start);

    let on_tool = on_tool.clone();

    button.connect_clicked(move |_| {
        on_tool(action);
    });

    button
}

/// Escapes raw page source so it can be dropped into a `<pre>` without the
/// markup (or any `<script>` inside it) being interpreted.
pub fn escape_html(source: &str) -> String {
    let mut escaped = String::with_capacity(source.len());

    for ch in source.chars() {
        match ch {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            _ => escaped.push(ch),
        }
    }

    escaped
}

/// Full HTML document shown by "View page source".
pub fn source_document(source: &str) -> String {
    format!(
        "<!DOCTYPE html><html><head><meta charset=\"utf-8\">\
         <title>{}</title></head>\
         <body style=\"margin:0\"><pre style=\"white-space:pre-wrap;word-wrap:break-word;\
         font-family:monospace;font-size:13px;padding:12px;\">{}</pre></body></html>",
        escape_html(&rust_i18n::t!("tools.source_title")),
        escape_html(source)
    )
}

pub fn build_tools(deps: ToolsDeps) -> ScrolledWindow {
    let ToolsDeps {
        on_downloads,
        on_history,
        on_clear_browsing_data,
        history,
        on_tool,
        zoom_level,
    } = deps;

    let page = Box::new(Orientation::Vertical, 14);

    page.set_margin_top(28);
    page.set_margin_bottom(28);
    page.set_margin_start(28);
    page.set_margin_end(28);

    page.set_vexpand(true);
    page.set_hexpand(true);

    let title = Label::new(Some(&rust_i18n::t!("tools.title")));

    title.add_css_class("title-1");

    title.set_halign(gtk::Align::Start);

    page.append(&title);

    let status = Label::new(None);

    status.add_css_class("dim-label");

    status.set_halign(gtk::Align::Start);

    /*
     * Browser pages (Chromium's "More tools" grouping).
     */
    page.append(&section_title("tools.browser"));

    let browser_row = Box::new(Orientation::Horizontal, 8);

    let downloads = Button::with_label(&rust_i18n::t!("tools.open_downloads"));

    downloads.set_halign(gtk::Align::Start);

    {
        let callback = on_downloads.clone();

        downloads.connect_clicked(move |_| {
            callback();
        });
    }

    let history_button = Button::with_label(&rust_i18n::t!("history.title"));

    history_button.set_halign(gtk::Align::Start);

    {
        let callback = on_history.clone();

        history_button.connect_clicked(move |_| {
            callback();
        });
    }

    browser_row.append(&downloads);
    browser_row.append(&history_button);
    browser_row.append(&tool_button(
        "tools.bookmarks",
        ToolAction::OpenBookmarks,
        &on_tool,
    ));
    browser_row.append(&tool_button("tools.tasks", ToolAction::OpenTasks, &on_tool));

    page.append(&browser_row);

    /*
     * Zoom for the active tab's page. View source and the inspector are not
     * listed here on purpose: this is a local `axys://` page, so those
     * actions belong to the web content itself (hamburger menu and the
     * WebView's right-click menu), not to a page about the browser.
     */
    page.append(&section_title("tools.page"));

    let zoom_row = Box::new(Orientation::Horizontal, 8);

    let zoom_label = Label::new(None);

    zoom_label.set_halign(gtk::Align::Start);

    let update_zoom = {
        let zoom_label = zoom_label.clone();
        let zoom_level = zoom_level.clone();

        Rc::new(move || {
            let percent = zoom_level()
                .map(|level| (level * 100.0).round() as i64)
                .unwrap_or(100);

            zoom_label.set_text(&rust_i18n::t!("tools.zoom_level", percent = percent));
        })
    };

    zoom_row.append(&zoom_label);

    // Show the current level immediately, not only after the first click.
    update_zoom();

    {
        let update_zoom = update_zoom.clone();

        // The page is built once and reused for the life of the tab; re-read
        // the zoom every time it becomes visible so a level changed by
        // Ctrl+scroll or the menu is never shown stale on return.
        page.connect_map(move |_| update_zoom());
    }

    for (key, action) in [
        ("tools.zoom_out", ToolAction::ZoomOut),
        ("tools.zoom_reset", ToolAction::ZoomReset),
        ("tools.zoom_in", ToolAction::ZoomIn),
    ] {
        let button = Button::with_label(&rust_i18n::t!(key));

        {
            let on_tool = on_tool.clone();
            let update_zoom = update_zoom.clone();

            button.connect_clicked(move |_| {
                on_tool(action);

                // Re-read the level right after the action ran.
                update_zoom();
            });
        }

        zoom_row.append(&button);
    }

    page.append(&zoom_row);

    /*
     * System.
     */
    page.append(&section_title("tools.system"));

    page.append(&tool_button(
        "tools.open_downloads_folder",
        ToolAction::OpenDownloadsFolder,
        &on_tool,
    ));

    page.append(&Separator::new(Orientation::Horizontal));

    /*
     * Browsing data.
     */
    page.append(&section_title("tools.browsing_data"));

    let browsing_data = Button::with_label(&rust_i18n::t!("tools.clear_browsing_data"));

    browsing_data.set_halign(gtk::Align::Start);

    browsing_data.add_css_class("destructive-action");

    {
        let callback = on_clear_browsing_data.clone();
        let status = status.clone();

        browsing_data.connect_clicked(move |_| {
            status.set_text(&rust_i18n::t!("tools.clearing"));

            let status = status.clone();

            callback(Rc::new(move |ok| {
                status.set_text(&rust_i18n::t!(if ok {
                    "tools.cleared"
                } else {
                    "tools.clear_failed"
                }));
            }));
        });
    }

    page.append(&browsing_data);

    // Manual age-based history cleanup (the manager also caps the total at
    // MAX_HISTORY_ENTRIES, oldest first).
    let purge_row = Box::new(Orientation::Horizontal, 8);

    for (label_key, max_age_secs) in [
        ("history.older_day", 86_400u64),
        ("history.older_week", 604_800u64),
        ("history.older_month", 2_592_000u64),
    ] {
        let purge = Button::with_label(&rust_i18n::t!(label_key));

        purge.set_halign(gtk::Align::Start);

        {
            let history = history.clone();
            let status = status.clone();

            purge.connect_clicked(move |_| {
                let removed = history.remove_older_than(max_age_secs);

                status.set_text(&rust_i18n::t!("tools.purged", count = removed));
            });
        }

        purge_row.append(&purge);
    }

    let note = Label::new(Some(&rust_i18n::t!("tools.note")));

    note.add_css_class("dim-label");

    note.set_halign(gtk::Align::Start);

    note.set_wrap(true);

    page.append(&purge_row);
    page.append(&status);
    page.append(&note);

    // The page has several sections, so it must scroll on small windows.
    let scroller = ScrolledWindow::new();

    scroller.set_child(Some(&page));
    scroller.set_hexpand(true);
    scroller.set_vexpand(true);

    scroller
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escape_html_neutralizes_markup() {
        assert_eq!(
            escape_html("<script>alert(1)</script> & <b>"),
            "&lt;script&gt;alert(1)&lt;/script&gt; &amp; &lt;b&gt;"
        );
    }

    #[test]
    fn source_document_wraps_escaped_source() {
        let document = source_document("<p>hi</p>");

        assert!(document.contains("&lt;p&gt;hi&lt;/p&gt;"));
        assert!(!document.contains("<p>hi</p>"));
    }
}
