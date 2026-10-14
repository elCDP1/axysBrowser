//! Find-in-page bar (Ctrl+F), Chromium style.
//!
//! A small floating bar anchored at the bottom of the window. It drives
//! WebKit's [`webkit6::FindController`] on the active tab: typing searches
//! incrementally (wrap-around), Enter goes to the next match, Esc closes.
//! Nothing is persisted; closing the bar stops the search.

use gtk::prelude::*;
use gtk::{Align, Box, Button, Entry, Label, Orientation};
use std::cell::RefCell;
use std::rc::Rc;
use webkit6::{FindController, FindOptions, WebView, prelude::WebViewExt};

/// Floating find bar. Create once per window and overlay it on the content.
#[derive(Clone)]
pub struct FindBar {
    /// Anchored bottom-center by the window; hidden by default.
    pub container: Box,
    entry: Entry,
    count: Label,
    current_web_view: Rc<RefCell<Option<WebView>>>,
    hooked: Rc<RefCell<Vec<FindController>>>,
}

/// Hooks match-count feedback for `view`'s controller, at most once per
/// controller (each tab owns its own).
fn ensure_hooked(
    hooked: &Rc<RefCell<Vec<FindController>>>,
    view: &WebView,
    count: &Label,
    entry: &Entry,
) -> Option<FindController> {
    let find = view.find_controller()?;

    // Each tab owns its controller; remember every one already wired so
    // switching tabs back and forth never stacks duplicate handlers.
    let known = hooked.borrow().iter().any(|known| known == &find);

    if !known {
        let count_ok = count.clone();
        let entry_ok = entry.clone();

        find.connect_found_text(move |_, matches| {
            count_ok.set_text(&matches.to_string());
            entry_ok.remove_css_class("error");
        });

        let count_fail = count.clone();
        let entry_fail = entry.clone();

        find.connect_failed_to_find_text(move |_| {
            count_fail.set_text("0");
            entry_fail.add_css_class("error");
        });

        hooked.borrow_mut().push(find.clone());
    }

    Some(find)
}

fn search_current(bar: &FindBar, text: &str) {
    let Some(view) = bar.current_web_view.borrow().as_ref().cloned() else {
        return;
    };

    let Some(find) = ensure_hooked(&bar.hooked, &view, &bar.count, &bar.entry) else {
        return;
    };

    if text.is_empty() {
        find.search_finish();
        bar.count.set_text("");
        bar.entry.remove_css_class("error");
        return;
    }

    find.search(text, FindOptions::WRAP_AROUND.bits(), 500);

    // The match count arrives async via found-text.
    bar.entry.remove_css_class("error");
}

impl FindBar {
    /// Builds the (initially hidden) bar. `current_web_view` must be the
    /// window's shared active-view slot.
    pub fn new(current_web_view: Rc<RefCell<Option<WebView>>>) -> Self {
        let container = Box::new(Orientation::Horizontal, 6);

        container.add_css_class("findbar");

        container.set_halign(Align::Center);
        container.set_valign(Align::End);
        container.set_margin_bottom(12);

        let entry = Entry::builder()
            .placeholder_text(rust_i18n::t!("find.title").as_ref())
            .width_chars(28)
            .build();

        let count = Label::new(None);
        count.add_css_class("dim-label");
        count.set_width_chars(6);

        let prev = Button::builder()
            .icon_name("go-up-symbolic")
            .tooltip_text(rust_i18n::t!("find.previous").as_ref())
            .build();
        prev.add_css_class("flat");

        let next = Button::builder()
            .icon_name("go-down-symbolic")
            .tooltip_text(rust_i18n::t!("find.next").as_ref())
            .build();
        next.add_css_class("flat");

        let close = Button::builder()
            .icon_name("window-close-symbolic")
            .tooltip_text(rust_i18n::t!("find.close").as_ref())
            .build();
        close.add_css_class("flat");

        let bar = Self {
            container,
            entry,
            count,
            current_web_view,
            hooked: Rc::new(RefCell::new(Vec::new())),
        };

        {
            let bar = bar.clone();
            let changed_entry = bar.entry.clone();

            changed_entry.connect_changed(move |entry| {
                search_current(&bar, entry.text().as_str());
            });
        }

        {
            let views = bar.current_web_view.clone();
            let activate_entry = bar.entry.clone();

            activate_entry.connect_activate(move |_| {
                if let Some(view) = views.borrow().as_ref()
                    && let Some(find) = view.find_controller()
                {
                    find.search_next();
                }
            });
        }

        {
            let views = bar.current_web_view.clone();

            prev.connect_clicked(move |_| {
                if let Some(view) = views.borrow().as_ref()
                    && let Some(find) = view.find_controller()
                {
                    find.search_previous();
                }
            });
        }

        {
            let views = bar.current_web_view.clone();

            next.connect_clicked(move |_| {
                if let Some(view) = views.borrow().as_ref()
                    && let Some(find) = view.find_controller()
                {
                    find.search_next();
                }
            });
        }

        {
            let bar = bar.clone();

            close.connect_clicked(move |_| {
                bar.hide();
            });
        }

        bar.container.append(&bar.entry);
        bar.container.append(&bar.count);
        bar.container.append(&prev);
        bar.container.append(&next);
        bar.container.append(&close);

        bar.container.set_visible(false);

        bar
    }

    /// Shows the bar and focuses its entry with the text selected.
    pub fn show(&self) {
        self.container.set_visible(true);
        self.entry.grab_focus();
        self.entry.select_region(0, -1);
    }

    /// Hides the bar and stops highlighting.
    pub fn hide(&self) {
        self.container.set_visible(false);

        if let Some(view) = self.current_web_view.borrow().as_ref()
            && let Some(find) = view.find_controller()
        {
            find.search_finish();
        }
    }

    /// Clears the highlights left in `view` (the tab being switched away).
    pub fn stop_in(&self, view: &WebView) {
        if self.is_visible()
            && let Some(find) = view.find_controller()
        {
            find.search_finish();
        }
    }

    /// Whether the bar is currently open.
    pub fn is_visible(&self) -> bool {
        self.container.is_visible()
    }

    /// Re-runs the current query in `view` (call on tab switch while open).
    pub fn research_in(&self, view: &WebView) {
        if !self.is_visible() {
            return;
        }

        let text = self.entry.text().to_string();

        if text.is_empty() {
            self.count.set_text("");
            return;
        }

        if ensure_hooked(&self.hooked, view, &self.count, &self.entry).is_some() {
            search_current(self, &text);
        }
    }
}
