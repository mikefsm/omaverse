//! The interlinear surface: every word a separate element, with its annotations
//! stacked beneath it.
//!
//! A text view will not do here. Each word has to be its own thing that can be
//! clicked, annotated, and later picked up and dropped onto a diagram, so this
//! is a flow of small columns rather than a run of text.

use crate::interlinear::Interlinear;
use gtk4 as gtk;
use gtk4::glib;
use gtk4::pango;
use gtk4::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;

pub struct WordGrid {
    pub root: gtk::ScrolledWindow,
    flow: gtk::FlowBox,
    /// Word ids in the order they were laid out, so a selected child maps back.
    ids: Rc<RefCell<Vec<String>>>,
}

impl WordGrid {
    pub fn new() -> WordGrid {
        let flow = gtk::FlowBox::builder()
            .selection_mode(gtk::SelectionMode::Single)
            .homogeneous(false)
            .row_spacing(18)
            .column_spacing(14)
            .margin_top(20)
            .margin_bottom(40)
            .margin_start(24)
            .margin_end(24)
            .valign(gtk::Align::Start)
            // Words run on until the line is full, as text does.
            .max_children_per_line(64)
            .build();

        let root = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vexpand(true)
            .child(&flow)
            .build();

        WordGrid { root, flow, ids: Rc::new(RefCell::new(Vec::new())) }
    }

    /// Lay out a sheet. Hebrew runs right to left, which is a property of the
    /// flow rather than of any word in it.
    pub fn show(&self, sheet: &Interlinear) {
        while let Some(child) = self.flow.first_child() {
            self.flow.remove(&child);
        }
        self.flow.set_direction(if sheet.language.right_to_left() {
            gtk::TextDirection::Rtl
        } else {
            gtk::TextDirection::Ltr
        });

        let mut ids = Vec::new();
        for word in &sheet.words {
            let column = gtk::Box::new(gtk::Orientation::Vertical, 2);
            column.set_halign(gtk::Align::Center);

            let text = gtk::Label::new(Some(&word.text));
            text.add_css_class("oma-word");
            text.set_halign(gtk::Align::Center);
            column.append(&text);

            // One row per annotation the sheet asks for, in order, with a
            // placeholder so an empty row still occupies its line and the
            // columns stay aligned across the passage.
            for row in &sheet.rows {
                let value = word.field(row).unwrap_or("");
                let label = gtk::Label::new(Some(if value.is_empty() { "·" } else { value }));
                label.add_css_class("oma-annot");
                if value.is_empty() {
                    label.add_css_class("oma-annot-empty");
                }
                label.set_halign(gtk::Align::Center);
                label.set_ellipsize(pango::EllipsizeMode::End);
                label.set_max_width_chars(18);
                label.set_tooltip_text((!value.is_empty()).then_some(value));
                column.append(&label);
            }

            self.flow.append(&column);
            ids.push(word.id.clone());
        }
        *self.ids.borrow_mut() = ids;
    }

    pub fn index_of(&self, id: &str) -> Option<usize> {
        self.ids.borrow().iter().position(|w| w == id)
    }

    /// Put the keyboard on a word, so working through a passage never needs the
    /// mouse.
    pub fn focus_word(&self, index: usize) {
        if let Some(child) = self.flow.child_at_index(index as i32) {
            self.flow.select_child(&child);
            child.grab_focus();
        }
    }

    /// Delete and Backspace remove the word the keyboard is on, so a run of
    /// verse numbers can be cleared without reaching for the mouse.
    pub fn connect_delete(&self, f: impl Fn(String) + 'static) {
        let ids = self.ids.clone();
        let keys = gtk::EventControllerKey::new();
        let flow = self.flow.clone();
        keys.connect_key_pressed(move |_, key, _, _| {
            if key != gtk4::gdk::Key::Delete && key != gtk4::gdk::Key::BackSpace {
                return glib::Propagation::Proceed;
            }
            let Some(child) = flow.selected_children().first().cloned() else {
                return glib::Propagation::Proceed;
            };
            let index = child.index();
            if index < 0 {
                return glib::Propagation::Proceed;
            }
            let id = ids.borrow().get(index as usize).cloned();
            match id {
                Some(id) => {
                    f(id);
                    glib::Propagation::Stop
                }
                None => glib::Propagation::Proceed,
            }
        });
        self.flow.add_controller(keys);
    }

    pub fn connect_activated(&self, f: impl Fn(String) + 'static) {
        let ids = self.ids.clone();
        self.flow.connect_child_activated(move |_, child| {
            let index = child.index();
            if index < 0 {
                return;
            }
            if let Some(id) = ids.borrow().get(index as usize).cloned() {
                f(id);
            }
        });
    }

    /// Where a word sits on screen, for anchoring a popover to it.
    pub fn rect_for(&self, index: usize) -> Option<gtk4::gdk::Rectangle> {
        let child = self.flow.child_at_index(index as i32)?;
        let bounds = child.compute_bounds(&self.root)?;
        Some(gtk4::gdk::Rectangle::new(
            bounds.x() as i32,
            bounds.y() as i32,
            bounds.width() as i32,
            bounds.height() as i32,
        ))
    }
}
