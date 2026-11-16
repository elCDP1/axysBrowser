//! Find-in-page bar (Ctrl+F), Chromium style.
//!
//! A small floating bar anchored at the bottom of the window. It drives
//! WebKit's [`webkit6::FindController`] on the active tab: typing searches
//! incrementally (wrap-around), Enter goes to the next match, Esc closes.
//! Nothing is persisted; closing the bar stops the search.

use gtk::prelude::*;
use gtk::{Align, Box, Button, Entry, Label, Orientation};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use webkit6::{FindController, FindOptions, WebView, prelude::WebViewExt};

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
    current_web_view: Rc<RefCell<Option<WebView>>>,
    hooked: Rc<RefCell<Vec<FindHook>>>,
    /// Incremented for every search and when the bar stops. Async signals
    /// carry the value they started with, so a late reply from an older query
    /// or tab can never overwrite the current match count.
    generation: Rc<Cell<u64>>,
}

/// A controller whose match-count handlers belong to the current search.
struct FindHook {
    controller: FindController,
    found: gtk::glib::SignalHandlerId,
    failed: gtk::glib::SignalHandlerId,
}

/// Removes `find`'s handlers and forgets it. Keeping the old handlers would
/// both leak them and let a queued result update the next query.
fn disconnect_hook(hooked: &Rc<RefCell<Vec<FindHook>>>, find: &FindController) {
    let removed = {
        let mut hooks = hooked.borrow_mut();
        let index = hooks.iter().position(|hook| &hook.controller == find);
        index.map(|index| hooks.remove(index))
    };

    if let Some(hook) = removed {
        let FindHook {
            controller,
            found,
            failed,
        } = hook;

        controller.disconnect(found);
        controller.disconnect(failed);
    }
}

/// Connects one pair of handlers for `search_id`. The weak view/entry/count
/// references avoid the controller -> handler -> widget cycle that used to
/// keep closed tabs and bars alive.
fn hook_search(
    current_web_view: &Rc<RefCell<Option<WebView>>>,
    hooked: &Rc<RefCell<Vec<FindHook>>>,
    generation: &Rc<Cell<u64>>,
    count: &Label,
    entry: &Entry,
    find: &FindController,
    search_id: u64,
) {
    let count_ok = count.downgrade();
    let entry_ok = entry.downgrade();
    let view_ok = Rc::downgrade(current_web_view);
    let generation_ok = generation.clone();

    let found = find.connect_found_text(move |controller, matches| {
        if generation_ok.get() != search_id {
            return;
        }

        let Some(view_slot) = view_ok.upgrade() else {
            return;
        };

        let Some(active) = view_slot.borrow().as_ref().cloned() else {
            return;
        };

        let Some(active_find) = active.find_controller() else {
            return;
        };

        if &active_find != controller {
            return;
        }

        if let Some(count) = count_ok.upgrade() {
            count.set_text(&matches.to_string());
        }

        if let Some(entry) = entry_ok.upgrade() {
            entry.remove_css_class("error");
        }
    });

    let count_fail = count.downgrade();
    let entry_fail = entry.downgrade();
    let view_fail = Rc::downgrade(current_web_view);
    let generation_fail = generation.clone();

    let failed = find.connect_failed_to_find_text(move |controller| {
        if generation_fail.get() != search_id {
            return;
        }

        let Some(view_slot) = view_fail.upgrade() else {
            return;
        };

        let Some(active) = view_slot.borrow().as_ref().cloned() else {
            return;
        };

        let Some(active_find) = active.find_controller() else {
            return;
        };

        if &active_find != controller {
            return;
        }

        if let Some(count) = count_fail.upgrade() {
            count.set_text("0");
        }

        if let Some(entry) = entry_fail.upgrade() {
            entry.add_css_class("error");
        }
    });

    hooked.borrow_mut().push(FindHook {
        controller: find.clone(),
        found,
        failed,
    });
}

/// Runs a query against the active view. Takes the pieces instead of the
/// whole bar so signal handlers can avoid capturing it and forming cycles.
fn search_current(
    current_web_view: &Rc<RefCell<Option<WebView>>>,
    hooked: &Rc<RefCell<Vec<FindHook>>>,
    generation: &Rc<Cell<u64>>,
    count: &Label,
    entry: &Entry,
    text: &str,
) {
    generation.set(generation.get().wrapping_add(1));

    let search_id = generation.get();

    let Some(view) = current_web_view.borrow().as_ref().cloned() else {
        return;
    };

    let Some(find) = view.find_controller() else {
        return;
    };

    disconnect_hook(hooked, &find);

    if text.is_empty() {
        find.search_finish();
        count.set_text("");
        entry.remove_css_class("error");
        return;
    }

    hook_search(
        current_web_view,
        hooked,
        generation,
        count,
        entry,
        &find,
        search_id,
    );

    find.search(text, FindOptions::WRAP_AROUND.bits(), 500);

    // The match count arrives async via found-text.
    entry.remove_css_class("error");
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
            find_prev: prev.clone(),
            find_next: next.clone(),
            find_close: close.clone(),
            current_web_view,
            hooked: Rc::new(RefCell::new(Vec::new())),
            generation: Rc::new(Cell::new(0)),
        };

        {
            // No `bar` capture: the entry would then hold itself through the
            // handler (a permanent leak). Keep the entry weak instead.
            let current_web_view = bar.current_web_view.clone();
            let hooked = bar.hooked.clone();
            let generation = bar.generation.clone();
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

                search_current(
                    &current_web_view,
                    &hooked,
                    &generation,
                    &count,
                    &entry,
                    &text,
                );
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
            // Weak container: `close` is a child of `bar.container`, so a
            // strong capture would be `container -> close -> handler ->
            // container`, leaking the bar (and its FindControllers) forever.
            let container = bar.container.downgrade();
            let current_web_view = bar.current_web_view.clone();
            let hooked = bar.hooked.clone();
            let generation = bar.generation.clone();

            close.connect_clicked(move |_| {
                if let Some(container) = container.upgrade() {
                    container.set_visible(false);
                }

                generation.set(generation.get().wrapping_add(1));

                let view = current_web_view.borrow().as_ref().cloned();

                if let Some(view) = view
                    && let Some(find) = view.find_controller()
                {
                    disconnect_hook(&hooked, &find);
                    find.search_finish();
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
    /// nothing on internal pages or tabs without a WebView: there is nothing
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
        self.generation.set(self.generation.get().wrapping_add(1));

        let view = self.current_web_view.borrow().as_ref().cloned();

        if let Some(view) = view
            && let Some(find) = view.find_controller()
        {
            disconnect_hook(&self.hooked, &find);
            find.search_finish();
        }
    }

    /// Clears the highlights left in `view` (the tab being switched away).
    pub fn stop_in(&self, view: &WebView) {
        if self.is_visible()
            && let Some(find) = view.find_controller()
        {
            self.generation.set(self.generation.get().wrapping_add(1));
            disconnect_hook(&self.hooked, &find);
            find.search_finish();
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

    /// Forgets the controller hooked for `view`. Call when *that* tab
    /// closes: controllers hold their views alive, so keeping dead ones
    /// would leak whole tabs. Only this view's controller is dropped — an
    /// open find bar in another tab must keep updating. It is re-hooked
    /// lazily on the next search.
    pub fn forget_hook(&self, view: &WebView) {
        if let Some(find) = view.find_controller() {
            disconnect_hook(&self.hooked, &find);
        }
    }

    /// Re-runs the current query in `view` (call on tab switch while open).
    pub fn research_in(&self, view: &WebView) {
        if !self.is_visible() {
            return;
        }

        let text = self.entry.text().to_string();

        if text.is_empty() {
            self.generation.set(self.generation.get().wrapping_add(1));
            self.count.set_text("");
            self.find_prev.set_sensitive(false);
            self.find_next.set_sensitive(false);
            self.entry.remove_css_class("error");

            if let Some(find) = view.find_controller() {
                disconnect_hook(&self.hooked, &find);
                find.search_finish();
            }

            return;
        }

        self.find_prev.set_sensitive(true);
        self.find_next.set_sensitive(true);

        search_current(
            &self.current_web_view,
            &self.hooked,
            &self.generation,
            &self.count,
            &self.entry,
            &text,
        );
    }
}
