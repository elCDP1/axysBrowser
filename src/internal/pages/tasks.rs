//! `axys://tasks` — live task manager.
//!
//! Chromium-inspired: a running list of the open tabs plus the browser's
//! memory, updated once a second, with a 60-second memory sparkline and the
//! current footprint shown live.
//!
//! WebKitGTK exposes no per-tab memory figure, so the graph and the big
//! number are the browser's *total* memory (main process + every WebKit
//! descendant, PSS when available), which is honest and still useful.

use gtk::glib::{self, ControlFlow};
use gtk::prelude::*;
use gtk::{Box, Button, DrawingArea, Label, Orientation, ScrolledWindow};
use std::cell::RefCell;
use std::rc::Rc;

use crate::ui::memprobe;

/// One row of the task manager: an open tab and whether it is the active one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TabRow {
    pub id: usize,
    pub title: String,
    pub url: String,
    pub active: bool,
}

pub struct TaskDeps {
    /// Snapshot of the window's open tabs, newest ordering preserved.
    pub tab_rows: Rc<dyn Fn() -> Vec<TabRow>>,
    pub on_close_tab: Rc<dyn Fn(usize)>,
}

/// Samples kept for the sparkline (one per second).
const WINDOW_SECONDS: usize = 60;

fn stat(label: &str) -> (Box, Label) {
    let column = Box::new(Orientation::Vertical, 2);

    let caption = Label::new(Some(label));

    caption.add_css_class("dim-label");

    caption.set_halign(gtk::Align::Start);

    let value = Label::new(None);

    value.add_css_class("title-2");

    value.set_halign(gtk::Align::Start);

    column.append(&caption);
    column.append(&value);

    (column, value)
}

fn build_tab_list(rows: &[TabRow], on_close_tab: &Rc<dyn Fn(usize)>) -> Box {
    let list = Box::new(Orientation::Vertical, 6);

    list.set_hexpand(true);

    for row in rows {
        let line = Box::new(Orientation::Horizontal, 10);

        line.set_hexpand(true);

        let info = Box::new(Orientation::Vertical, 2);

        info.set_hexpand(true);

        let title = Label::new(Some(&row.title));

        title.set_halign(gtk::Align::Start);
        title.set_ellipsize(gtk::pango::EllipsizeMode::End);

        let url = Label::new(Some(&row.url));

        url.add_css_class("dim-label");

        url.set_halign(gtk::Align::Start);
        url.set_ellipsize(gtk::pango::EllipsizeMode::Middle);

        info.append(&title);
        info.append(&url);

        line.append(&info);

        if row.active {
            let active = Label::new(Some(&rust_i18n::t!("tools.tasks_active")));

            active.add_css_class("dim-label");

            line.append(&active);
        }

        let close = Button::with_label(&rust_i18n::t!("tools.tasks_close"));

        close.add_css_class("flat");

        {
            let on_close_tab = on_close_tab.clone();
            let id = row.id;

            close.connect_clicked(move |_| {
                on_close_tab(id);
            });
        }

        line.append(&close);

        list.append(&line);
    }

    list
}

pub fn build_tasks(deps: TaskDeps) -> ScrolledWindow {
    let TaskDeps {
        tab_rows,
        on_close_tab,
    } = deps;

    let page = Box::new(Orientation::Vertical, 16);

    page.set_margin_top(28);
    page.set_margin_bottom(28);
    page.set_margin_start(28);
    page.set_margin_end(28);

    page.set_hexpand(true);

    let title = Label::new(Some(&rust_i18n::t!("tools.tasks")));

    title.add_css_class("title-1");

    title.set_halign(gtk::Align::Start);

    page.append(&title);

    /*
     * Live figures: current memory and web-process count.
     */
    let stats = Box::new(Orientation::Horizontal, 40);

    stats.set_halign(gtk::Align::Start);

    let (memory_column, memory_value) = stat(&rust_i18n::t!("tools.tasks_memory"));

    let (process_column, process_value) = stat(&rust_i18n::t!("tools.tasks_processes"));

    stats.append(&memory_column);
    stats.append(&process_column);

    page.append(&stats);

    /*
     * 60-second memory graph.
     */
    let graph_caption = Label::new(Some(&rust_i18n::t!("tools.tasks_graph")));

    graph_caption.add_css_class("dim-label");

    graph_caption.set_halign(gtk::Align::Start);

    page.append(&graph_caption);

    let samples: Rc<RefCell<Vec<f64>>> = Rc::new(RefCell::new(Vec::new()));

    let area = DrawingArea::new();

    area.set_hexpand(true);
    area.set_content_height(150);

    area.set_draw_func({
        let samples = samples.clone();

        move |_, cr, width, height| {
            let samples = samples.borrow();

            let w = width as f64;
            let h = height as f64;

            let top = h * 0.1;
            let bottom = h * 0.9;

            // Horizontal grid.
            cr.set_line_width(1.0);
            cr.set_source_rgba(0.5, 0.5, 0.5, 0.20);

            for step in 0..=3 {
                let y = top + (bottom - top) * step as f64 / 3.0;

                cr.move_to(0.0, y);
                cr.line_to(w, y);
            }

            let _ = cr.stroke();

            if samples.len() < 2 {
                return;
            }

            let min = samples.iter().copied().fold(f64::INFINITY, f64::min);
            let max = samples.iter().copied().fold(f64::NEG_INFINITY, f64::max);
            let range = (max - min).max(1.0);

            let step = w / (WINDOW_SECONDS as f64 - 1.0);

            // Filled area under the line.
            cr.set_source_rgba(0.23, 0.51, 0.96, 0.18);

            cr.move_to(0.0, bottom);

            for (index, value) in samples.iter().enumerate() {
                let x = index as f64 * step;
                let y = bottom - ((value - min) / range) * (bottom - top);

                cr.line_to(x, y);
            }

            let last_x = (samples.len() - 1) as f64 * step;

            cr.line_to(last_x, bottom);
            cr.close_path();
            let _ = cr.fill();

            // The line itself.
            cr.set_source_rgb(0.23, 0.51, 0.96);
            cr.set_line_width(2.0);

            for (index, value) in samples.iter().enumerate() {
                let x = index as f64 * step;
                let y = bottom - ((value - min) / range) * (bottom - top);

                if index == 0 {
                    cr.move_to(x, y);
                } else {
                    cr.line_to(x, y);
                }
            }

            let _ = cr.stroke();
        }
    });

    page.append(&area);

    /*
     * Open tabs.
     */
    let tabs_caption = Label::new(Some(&rust_i18n::t!("tools.tasks_tabs")));

    tabs_caption.add_css_class("title-3");

    tabs_caption.set_halign(gtk::Align::Start);

    let list = Box::new(Orientation::Vertical, 6);

    list.set_hexpand(true);

    page.append(&tabs_caption);
    page.append(&list);

    let note = Label::new(Some(&rust_i18n::t!("tools.tasks_note")));

    note.add_css_class("dim-label");

    note.set_halign(gtk::Align::Start);

    note.set_wrap(true);

    page.append(&note);

    let scroller = ScrolledWindow::new();

    scroller.set_child(Some(&page));
    scroller.set_hexpand(true);
    scroller.set_vexpand(true);

    /*
     * One-second tick: sample memory, redraw, refresh the tab list only when
     * its content actually changed (no per-second widget churn).
     */
    let update: Rc<dyn Fn()> = {
        let samples = samples.clone();
        let area = area.clone();
        let memory_value = memory_value.clone();
        let process_value = process_value.clone();
        let list = list.clone();
        let tab_rows = tab_rows.clone();
        let on_close_tab = on_close_tab.clone();
        let last_rows: Rc<RefCell<Vec<TabRow>>> = Rc::new(RefCell::new(Vec::new()));

        Rc::new(move || {
            let memory_kb = memprobe::tree_memory_kb();

            {
                let mut samples = samples.borrow_mut();

                samples.push(memory_kb as f64);

                if samples.len() > WINDOW_SECONDS {
                    samples.remove(0);
                }
            }

            area.queue_draw();

            memory_value.set_text(&crate::browser::downloads::format_bytes(memory_kb * 1024));

            process_value.set_text(&memprobe::web_process_pids().len().to_string());

            let rows = tab_rows();

            {
                let mut last_rows = last_rows.borrow_mut();

                if *last_rows == rows {
                    return;
                }

                while let Some(child) = list.first_child() {
                    list.remove(&child);
                }

                list.append(&build_tab_list(&rows, &on_close_tab));

                *last_rows = rows;
            }
        })
    };

    update();

    // Every tab prebuilds its own tasks page, so refresh right away whenever
    // this one becomes visible (the tick below skips hidden pages).
    {
        let update = update.clone();

        scroller.connect_map(move |_| update());
    }

    glib::timeout_add_seconds_local(1, {
        let update = update.clone();
        let scroller = scroller.clone();

        move || {
            // Stop when the page has been destroyed (tab closed).
            if scroller.root().is_none() {
                return ControlFlow::Break;
            }

            // A hidden GtkStack child is unmapped: skip the two /proc scans
            // so N open tabs don't cost 2N scans per second.
            if !scroller.is_mapped() {
                return ControlFlow::Continue;
            }

            update();

            ControlFlow::Continue
        }
    });

    scroller
}
