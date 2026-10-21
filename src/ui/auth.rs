//! HTTP/proxy authentication prompt.
//!
//! WebKit cancels an authentication request unless the `authenticate` signal
//! is handled and a credential is delivered later, so the dialog keeps a
//! cloned `AuthenticationRequest` and either authenticates or cancels it.

use gtk::prelude::*;
use std::cell::Cell;
use std::rc::Rc;
use webkit6::{AuthenticationRequest, Credential, CredentialPersistence};

pub fn prompt(view: &webkit6::WebView, request: &AuthenticationRequest) {
    let Some(parent) = view
        .root()
        .and_then(|root| root.downcast::<gtk::Window>().ok())
    else {
        request.cancel();
        return;
    };

    let host = request
        .host()
        .map(|host| host.to_string())
        .unwrap_or_default();
    let realm = request
        .realm()
        .map(|realm| realm.to_string())
        .unwrap_or_default();

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

    if let Some(mut credential) = request.proposed_credential()
        && let Some(proposed) = credential.username()
    {
        username.set_text(&proposed);
    }

    let password = gtk::Entry::builder()
        .placeholder_text(rust_i18n::t!("auth.password"))
        .visibility(false)
        .build();

    content.append(&username);
    content.append(&password);

    if request.is_retry() {
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
        let request = request.clone();
        let decided = decided.clone();
        let username = username.clone();
        let password = password.clone();

        sign_in.connect_clicked(move |_| {
            let credential = Credential::new(
                &username.text(),
                &password.text(),
                CredentialPersistence::ForSession,
            );

            decided.set(true);
            request.authenticate(Some(&credential));

            if let Some(dialog) = dialog.upgrade() {
                dialog.close();
            }
        });
    }

    {
        let dialog = dialog.downgrade();
        let request = request.clone();
        let decided = decided.clone();

        cancel.connect_clicked(move |_| {
            decided.set(true);
            request.cancel();

            if let Some(dialog) = dialog.upgrade() {
                dialog.close();
            }
        });
    }

    {
        let request = request.clone();
        let decided = decided.clone();

        dialog.connect_close_request(move |_| {
            if !decided.get() {
                request.cancel();
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
