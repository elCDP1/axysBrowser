use gtk::prelude::*;
use gtk::{
    Align, Box as GtkBox, Button, EventControllerFocus, Label, Orientation, ProgressBar,
    ScrolledWindow,
};
use std::rc::Rc;

use crate::browser::downloads::{DownloadEntry, DownloadManager, DownloadStatus, format_speed};

/// A download row. When shown inside the toolbar popup, `on_interact` closes
/// the popup after a row button is used (clicking a popup button always
/// dismisses it); the full page passes `None`.
pub fn build_row(
    entry: &DownloadEntry,
    manager: &DownloadManager,
    on_interact: Option<Rc<dyn Fn()>>,
) -> GtkBox {
    let row = GtkBox::new(Orientation::Horizontal, 12);

    row.set_hexpand(true);

    let info = GtkBox::new(Orientation::Vertical, 3);

    info.set_hexpand(true);

    let name = Label::new(Some(&entry.filename));

    name.set_halign(Align::Start);

    name.set_ellipsize(gtk::pango::EllipsizeMode::Middle);

    name.set_max_width_chars(32);

    info.append(&name);

    let status_text = match &entry.status {
        DownloadStatus::InProgress => {
            let percent = format!("{:.0}%", entry.progress * 100.0);
            let speed = format_speed(entry.speed_bps());

            if speed.is_empty() {
                percent
            } else {
                format!("{percent} · {speed}")
            }
        }

        DownloadStatus::Completed => rust_i18n::t!("downloads.completed").to_string(),

        DownloadStatus::Cancelled => rust_i18n::t!("downloads.cancelled").to_string(),

        DownloadStatus::Failed(message) => {
            rust_i18n::t!("downloads.failed", message = message).to_string()
        }
    };

    let status = Label::new(Some(&status_text));

    status.add_css_class("dim-label");

    status.set_halign(Align::Start);

    status.set_ellipsize(gtk::pango::EllipsizeMode::End);

    status.set_max_width_chars(32);

    info.append(&status);

    if entry.status == DownloadStatus::InProgress {
        let bar = ProgressBar::new();

        bar.set_fraction(entry.progress);

        bar.set_hexpand(true);

        info.append(&bar);
    }

    row.append(&info);

    match &entry.status {
        DownloadStatus::InProgress => {
            let cancel = Button::with_label(&rust_i18n::t!("downloads.cancel"));

            cancel.add_css_class("flat");

            let manager = manager.clone();

            let id = entry.id;

            let on_interact = on_interact.clone();

            cancel.connect_clicked(move |_| {
                manager.cancel(id);

                if let Some(close) = on_interact.as_ref() {
                    close();
                }
            });

            row.append(&cancel);
        }

        DownloadStatus::Completed => {
            let show = Button::from_icon_name("folder-symbolic");

            show.add_css_class("flat");

            show.set_tooltip_text(Some(&rust_i18n::t!("downloads.show_in_folder")));

            let path = entry.path.clone();

            let on_interact = on_interact.clone();

            show.connect_clicked(move |_| {
                DownloadManager::open_containing_folder(&path);

                if let Some(close) = on_interact.as_ref() {
                    close();
                }
            });

            row.append(&show);
        }

        _ => {}
    }

    row
}

pub fn build_downloads(manager: DownloadManager) -> GtkBox {
    let page = GtkBox::new(Orientation::Vertical, 14);

    page.set_margin_top(28);
    page.set_margin_bottom(28);
    page.set_margin_start(28);
    page.set_margin_end(28);

    page.set_vexpand(true);
    page.set_hexpand(true);

    let title = Label::new(Some(&rust_i18n::t!("downloads.title")));

    title.add_css_class("title-1");

    title.set_halign(Align::Start);

    let list = GtkBox::new(Orientation::Vertical, 10);

    let scroller = ScrolledWindow::new();

    scroller.set_child(Some(&list));

    scroller.set_vexpand(true);

    scroller.set_hexpand(true);

    page.append(&title);

    page.append(&scroller);

    // Privacy honesty: files downloaded in a private window stay on disk
    // like everywhere else — closing it only clears this list, and each
    // private window keeps a separate list that dies with it.
    let note = Label::new(Some(&rust_i18n::t!("downloads.private_note")));

    note.add_css_class("dim-label");

    note.set_halign(Align::Start);

    note.set_wrap(true);

    page.append(&note);

    let refresh: Rc<dyn Fn()> = {
        let list = list.clone();

        let manager = manager.clone();

        Rc::new(move || {
            while let Some(child) = list.first_child() {
                list.remove(&child);
            }

            let entries = manager.entries();

            if entries.is_empty() {
                let empty = Label::new(Some(&rust_i18n::t!("downloads.empty")));

                empty.add_css_class("dim-label");

                empty.set_halign(Align::Start);

                list.append(&empty);

                return;
            }

            for entry in entries.iter().rev() {
                list.append(&build_row(entry, &manager, None));
            }
        })
    };

    refresh();

    manager.subscribe(&refresh);

    let refresh_keepalive = refresh.clone();

    let keepalive = EventControllerFocus::new();

    keepalive.connect_enter(move |_| {
        let _ = refresh_keepalive.clone();
    });

    page.add_controller(keepalive);

    page
}

#[cfg(test)]
mod tests {
    #[test]
    fn failed_status_interpolates_message() {
        // Guards the only interpolated string in the app: if the syntax ever
        // stops matching rust-i18n, the UI would show a raw "%{message}".
        let text = rust_i18n::t!("downloads.failed", message = "boom").to_string();

        assert!(text.contains("boom"), "unexpected: {text}");
        assert!(!text.contains("%{"), "uninterpolated: {text}");
    }
}
