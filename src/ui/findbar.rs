//! Find-in-page bar (Ctrl+F), Chromium style.
//!
//! A small floating bar anchored at the bottom of the window. It asks the
//! active tab's page to search (`View::find`) and shows the match count the
//! page reports; Enter goes to the next match, Esc closes. Nothing is
//! persisted, and closing the bar stops the search.
//!
//! The page keeps at most one result in flight: a newer query supersedes a
//! pending one, so a late reply from an older query can never overwrite the
//! current count. That is why this file no longer tracks generations or hook
//! handles of its own.

use gtk::prelude::*;
use gtk::{Align, Box, Button, Entry, Label, Orientation};
use std::cell::RefCell;
use std::rc::Rc;

use crate::browser::engine::{FindOutcome, FindRequest, View};

/// Floating find bar. Create once per window and overlay it on the content.
#[derive(Clone)]
pub struct FindBar {
    /// Anchored bottom-center by the window; hidden by default.
    pub container: Box,
    entry: Entry,
    count: Label,
    find_prev: Button,
    find_next: Button,
    find_close: Button,
    current_web_view: Rc<RefCell<Option<View>>>,
}

/// How a result is shown: the count, and an error tint when nothing matched.
///
/// The widgets are held weakly: the page keeps the callback alive until it
/// answers, and a strong capture would make the bar (and its page) immortal.
fn reporter(count: &Label, entry: &Entry) -> impl FnOnce(FindOutcome) + 'static {
    let count = count.downgrade();
    let entry = entry.downgrade();

    move |outcome| {
        if let Some(count) = count.upgrade() {
            count.set_text(&outcome.matches.to_string());
        }

        if let Some(entry) = entry.upgrade() {
            if outcome.found {
                entry.remove_css_class("error");
            } else {
                entry.add_css_class("error");
            }
        }
    }
}

/// Runs a fresh query against `view`. An empty query stops the search and
/// clears the count, which is what the bar shows after the user clears it.
fn search_in(view: &View, count: &Label, entry: &Entry, text: &str) {
    if text.trim().is_empty() {
        view.find_stop();
        count.set_text("");
        entry.remove_css_class("error");

        return;
    }

    view.find(FindRequest::first(text), reporter(count, entry));
}

/// Moves inside the current query: Enter and the arrows.
fn step_in(view: &View, count: &Label, entry: &Entry, query: &str, previous: bool) {
    if query.trim().is_empty() {
        return;
    }

    let request = FindRequest::first(query);

    let request = if previous {
        request.previous()
    } else {
        request.next()
    };

    view.find(request, reporter(count, entry));
}

impl FindBar {
    /// Builds the (initially hidden) bar. `current_web_view` must be the
    /// window's shared active-view slot.
    pub fn new(current_web_view: Rc<RefCell<Option<View>>>) -> Self {
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
            find_prev: prev.clone(),
            find_next: next.clone(),
            find_close: close.clone(),
            current_web_view,
        };

        {
            // No `bar` capture: the entry would then hold itself through the
            // handler (a permanent leak). Keep the entry weak instead.
            let current_web_view = bar.current_web_view.clone();
            let count = bar.count.clone();
            let entry_weak = bar.entry.downgrade();
            let changed_entry = bar.entry.clone();
            let prev = bar.find_prev.clone();
            let next = bar.find_next.clone();

            changed_entry.connect_changed(move |_| {
                let Some(entry) = entry_weak.upgrade() else {
                    return;
                };

                let text = entry.text().to_string();
                let has_text = !text.trim().is_empty();

                // Nothing to step through while the query is empty.
                prev.set_sensitive(has_text);
                next.set_sensitive(has_text);

                let view = current_web_view.borrow().as_ref().cloned();

                if let Some(view) = view {
                    search_in(&view, &count, &entry, &text);
                }
            });
        }

        {
            let views = bar.current_web_view.clone();
            let count = bar.count.clone();
            let entry_weak = bar.entry.downgrade();
            let activate_entry = bar.entry.clone();

            activate_entry.connect_activate(move |_| {
                let Some(entry) = entry_weak.upgrade() else {
                    return;
                };

                let view = views.borrow().as_ref().cloned();

                if let Some(view) = view {
                    step_in(&view, &count, &entry, &entry.text(), false);
                }
            });
        }

        {
            let views = bar.current_web_view.clone();
            let count = bar.count.clone();
            let entry = bar.entry.clone();
            let prev_button = prev.clone();

            prev_button.connect_clicked(move |_| {
                let view = views.borrow().as_ref().cloned();

                if let Some(view) = view {
                    step_in(&view, &count, &entry, &entry.text(), true);
                }
            });
        }

        {
            let views = bar.current_web_view.clone();
            let count = bar.count.clone();
            let entry = bar.entry.clone();
            let next_button = next.clone();

            next_button.connect_clicked(move |_| {
                let view = views.borrow().as_ref().cloned();

                if let Some(view) = view {
                    step_in(&view, &count, &entry, &entry.text(), false);
                }
            });
        }

        {
            // Weak container: `close` is a child of `bar.container`, so a
            // strong capture would be `container -> close -> handler ->
            // container`, leaking the bar forever.
            let container = bar.container.downgrade();
            let current_web_view = bar.current_web_view.clone();

            close.connect_clicked(move |_| {
                if let Some(container) = container.upgrade() {
                    container.set_visible(false);
                }

                let view = current_web_view.borrow().as_ref().cloned();

                if let Some(view) = view {
                    view.find_stop();
                }
            });
        }

        bar.container.append(&bar.entry);
        bar.container.append(&bar.count);
        bar.container.append(&prev);
        bar.container.append(&next);
        bar.container.append(&close);

        bar.container.set_visible(false);

        bar.find_prev.set_sensitive(false);
        bar.find_next.set_sensitive(false);

        bar
    }

    /// Shows the bar and focuses its entry with the text selected. Does
    /// nothing on internal pages or tabs without a page: there is nothing
    /// to search there, and the bar used to open with a stale count.
    pub fn show(&self) {
        let view = self.current_web_view.borrow().as_ref().cloned();

        let Some(view) = view else {
            self.container.set_visible(false);

            return;
        };

        self.container.set_visible(true);
        self.entry.grab_focus();
        self.entry.select_region(0, -1);

        // Re-run the query in the current tab: reopening after a tab switch
        // used to show the previous tab's match count with no highlights.
        self.research_in(&view);
    }

    /// Hides the bar and stops highlighting.
    pub fn hide(&self) {
        self.container.set_visible(false);

        let view = self.current_web_view.borrow().as_ref().cloned();

        if let Some(view) = view {
            view.find_stop();
        }
    }

    /// Clears the highlights left in `view` (the tab being switched away).
    pub fn stop_in(&self, view: &View) {
        if self.is_visible() {
            view.find_stop();
        }
    }

    /// Whether the bar is currently open.
    pub fn is_visible(&self) -> bool {
        self.container.is_visible()
    }

    /// Re-translates placeholder and tooltips (language change; the bar
    /// itself is built once per window).
    pub fn refresh_labels(&self) {
        self.entry
            .set_placeholder_text(Some(&rust_i18n::t!("find.title")));
        self.find_prev
            .set_tooltip_text(Some(&rust_i18n::t!("find.previous")));
        self.find_next
            .set_tooltip_text(Some(&rust_i18n::t!("find.next")));
        self.find_close
            .set_tooltip_text(Some(&rust_i18n::t!("find.close")));
    }

    /// Clears what `view` has highlighted. Call when *that* tab closes: the
    /// bar's handlers live on the page, and a page that is going away must
    /// not keep a search running. The bar in another tab is untouched.
    pub fn forget_hook(&self, view: &View) {
        view.find_stop();
    }

    /// Re-runs the current query in `view` (call on tab switch while open).
    pub fn research_in(&self, view: &View) {
        if !self.is_visible() {
            return;
        }

        let text = self.entry.text().to_string();

        search_in(view, &self.count, &self.entry, &text);

        let has_text = !text.trim().is_empty();

        self.find_prev.set_sensitive(has_text);
        self.find_next.set_sensitive(has_text);
    }
}
