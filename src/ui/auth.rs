//! HTTP/proxy authentication prompt.
//!
//! The engine cancels an authentication request unless the embedder claims
//! it and answers later, so the dialog keeps a cloned [`AuthPrompt`] and
//! either sends credentials or cancels it. The prompt answers exactly once,
//! which is what makes "sign in, then close the dialog" safe.

use gtk::prelude::*;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

use crate::browser::engine::{AuthPrompt, Credentials, View};

pub fn prompt(view: &View, prompt: AuthPrompt) {
    let Some(parent) = view
        .root()
        .and_then(|root| root.downcast::<gtk::Window>().ok())
    else {
        prompt.answer(None);
        return;
    };

    let host = prompt.host().to_string();
    let realm = prompt.realm().to_string();

    let dialog = gtk::Window::builder()
        .title(rust_i18n::t!("auth.title").as_ref())
        .transient_for(&parent)
        .modal(true)
        .resizable(false)
        .build();

    let content = gtk::Box::new(gtk::Orientation::Vertical, 12);

    content.set_margin_top(18);
    content.set_margin_bottom(18);
    content.set_margin_start(18);
    content.set_margin_end(18);

    let message = gtk::Label::new(Some(&rust_i18n::t!("auth.message", host = host.as_str())));

    message.set_wrap(true);
    message.set_xalign(0.0);

    content.append(&message);

    if !realm.is_empty() {
        let label = gtk::Label::new(Some(&rust_i18n::t!("auth.realm", realm = realm.as_str())));

        label.set_wrap(true);
        label.set_xalign(0.0);
        label.add_css_class("dim-label");

        content.append(&label);
    }

    let username = gtk::Entry::builder()
        .placeholder_text(rust_i18n::t!("auth.username"))
        .build();

    if let Some(proposed) = prompt.proposed_username() {
        username.set_text(proposed);
    }

    let password = gtk::Entry::builder()
        .placeholder_text(rust_i18n::t!("auth.password"))
        .visibility(false)
        .build();

    content.append(&username);
    content.append(&password);

    if prompt.is_retry() {
        let retry = gtk::Label::new(Some(&rust_i18n::t!("auth.retry")));

        retry.set_wrap(true);
        retry.set_xalign(0.0);
        retry.add_css_class("error");

        content.append(&retry);
    }

    let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 8);

    buttons.set_halign(gtk::Align::End);

    let cancel = gtk::Button::with_label(&rust_i18n::t!("auth.cancel"));
    let sign_in = gtk::Button::with_label(&rust_i18n::t!("auth.sign_in"));

    sign_in.add_css_class("suggested-action");

    buttons.append(&cancel);
    buttons.append(&sign_in);

    content.append(&buttons);
    dialog.set_child(Some(&content));

    // The close button (and Escape) must not cancel a request that was just
    // authenticated, so both paths record that a choice was already made.
    let decided = Rc::new(Cell::new(false));

    {
        let dialog = dialog.downgrade();
        let prompt = prompt.clone();
        let decided = decided.clone();
        let username = username.clone();
        let password = password.clone();

        sign_in.connect_clicked(move |_| {
            decided.set(true);

            prompt.answer(Some(Credentials {
                username: username.text().to_string(),
                password: password.text().to_string(),
            }));

            if let Some(dialog) = dialog.upgrade() {
                dialog.close();
            }
        });
    }

    {
        let dialog = dialog.downgrade();
        let prompt = prompt.clone();
        let decided = decided.clone();

        cancel.connect_clicked(move |_| {
            decided.set(true);
            prompt.answer(None);

            if let Some(dialog) = dialog.upgrade() {
                dialog.close();
            }
        });
    }

    // A tab closed while its sign-in dialog is open must not leave a modal
    // dialog over the window for a request nobody is waiting for any more:
    // closing the dialog cancels the request.
    let destroy_handler: Rc<RefCell<Option<gtk::glib::SignalHandlerId>>> =
        Rc::new(RefCell::new(None));

    {
        let dialog = dialog.downgrade();

        let id = view.connect_destroy(move |_| {
            if let Some(dialog) = dialog.upgrade() {
                dialog.close();
            }
        });

        *destroy_handler.borrow_mut() = Some(id);
    }

    {
        let prompt = prompt.clone();
        let decided = decided.clone();
        let view = view.clone();
        let destroy_handler = destroy_handler.clone();

        dialog.connect_close_request(move |_| {
            if !decided.get() {
                prompt.answer(None);
            }

            if let Some(id) = destroy_handler.borrow_mut().take() {
                view.disconnect(id);
            }

            gtk::glib::Propagation::Proceed
        });
    }

    {
        let dialog_for_key = dialog.downgrade();

        let key = gtk::EventControllerKey::new();

        key.connect_key_pressed(move |_, keyval, _, _| {
            if keyval == gtk::gdk::Key::Escape {
                if let Some(dialog) = dialog_for_key.upgrade() {
                    dialog.close();
                }

                gtk::glib::Propagation::Stop
            } else {
                gtk::glib::Propagation::Proceed
            }
        });

        dialog.add_controller(key);
    }

    {
        let sign_in = sign_in.clone();

        password.connect_activate(move |_| sign_in.emit_clicked());
    }

    {
        let password = password.clone();

        username.connect_activate(move |_| {
            password.grab_focus();
        });
    }

    dialog.present();

    if username.text().is_empty() {
        username.grab_focus();
    } else {
        password.grab_focus();
    }
}
