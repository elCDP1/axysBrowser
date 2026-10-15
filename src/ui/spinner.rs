//! Timeout-driven loading spinner for the reload button.
//!
//! A `GtkSpinner` animates off the widget frame clock, which does not
//! reliably advance on every link this browser targets (notably WSLg with
//! software rendering), leaving a frozen sprite instead of a rotation.
//! This spinner advances on a plain main-loop timeout, so it rotates
//! whenever the UI thread itself is alive.

use gtk::prelude::*;
use gtk::DrawingArea;
use std::cell::{Cell, RefCell};
use std::f64::consts::PI;
use std::rc::Rc;

/// 16 px rotating arc in the theme accent color.
pub struct LoadingSpinner {
    area: DrawingArea,
    angle: Rc<Cell<f64>>,
    timer: Rc<RefCell<Option<gtk::glib::SourceId>>>,
}

impl LoadingSpinner {
    pub fn new() -> Self {
        let area = DrawingArea::new();

        area.set_size_request(16, 16);

        let angle = Rc::new(Cell::new(0.0));

        {
            let angle = angle.clone();

            area.set_draw_func(move |area, cr, width, height| {
                // Center on the real allocation: containers (e.g. the reload
                // stack) can stretch the area beyond its 16 px request.
                let width = width.max(1) as f64;
                let height = height.max(1) as f64;

                let radius = (width.min(height) / 2.0 - 2.5).max(2.0);

                let fg = area.color();

                cr.set_source_rgb(fg.red() as f64, fg.green() as f64, fg.blue() as f64);
                cr.set_line_width(2.2);

                let turn = angle.get();

                cr.arc(width / 2.0, height / 2.0, radius, turn, turn + PI * 1.5);

                let _ = cr.stroke();
            });
        }

        Self {
            area,
            angle,
            timer: Rc::new(RefCell::new(None)),
        }
    }

    /// The widget to pack (e.g. into the reload stack).
    pub fn widget(&self) -> &DrawingArea {
        &self.area
    }

    /// Starts rotating. Idempotent: calling twice keeps a single timer.
    pub fn start(&self) {
        if self.timer.borrow().is_some() {
            return;
        }

        let angle = self.angle.clone();
        let area = self.area.clone();

        *self.timer.borrow_mut() = Some(gtk::glib::timeout_add_local(
            std::time::Duration::from_millis(50),
            move || {
                angle.set(angle.get() + PI / 10.0);
                area.queue_draw();

                gtk::glib::ControlFlow::Continue
            },
        ));
    }

    /// Stops rotating and drops the timer.
    pub fn stop(&self) {
        if let Some(id) = self.timer.borrow_mut().take() {
            id.remove();
        }
    }
}

impl Drop for LoadingSpinner {
    /// A window closed mid-load must not leave the timer registered: it
    /// holds the widget alive and would spin (and redraw) forever.
    fn drop(&mut self) {
        self.stop();
    }
}
