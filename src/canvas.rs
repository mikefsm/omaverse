//! The diagramming surface: a palette of Reed-Kellogg strokes, a free canvas,
//! and the passage's words along the bottom waiting to be dragged up onto it.
//!
//! Nothing snaps. A line is two endpoints and a word is a position, and both
//! are dragged by hand — which is what a diagram is.

use crate::diagram::{Diagram, End, Preset, Stroke};
use gtk4 as gtk;
use gtk4::gdk;
use gtk4::glib;
use gtk4::pango;
use gtk4::prelude::*;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

/// How close the pointer has to be to take hold of something.
const GRAB: f64 = 9.0;
/// The canvas is fixed and generous rather than growing to fit; a diagram that
/// needs more room than this is really two diagrams.
const WIDTH: i32 = 2200;
const HEIGHT: i32 = 1500;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Sel {
    Label(String),
    Line(String),
}

/// What the current drag is doing.
#[derive(Debug, Clone)]
enum Drag {
    None,
    MoveLabel { id: String, x: f64, y: f64 },
    MoveLine { id: String, x1: f64, y1: f64, x2: f64, y2: f64 },
    MoveEnd { id: String, end: End },
}

pub struct Canvas {
    pub root: gtk::Box,
    area: gtk::DrawingArea,
    bankbox: gtk::FlowBox,
    doc: Rc<RefCell<Option<Diagram>>>,
    /// Where each label was drawn last time, for picking one out from under the
    /// pointer. Text extents are only known once drawn.
    rects: Rc<RefCell<Vec<(String, f64, f64, f64, f64)>>>,
    sel: Rc<RefCell<Option<Sel>>>,
    /// A palette stroke waiting for somewhere to go.
    armed: Rc<Cell<Option<Preset>>>,
    drag: Rc<RefCell<Drag>>,
    on_change: Rc<RefCell<Option<Box<dyn Fn()>>>>,
    rtl: Rc<Cell<bool>>,
    /// Shown only when the diagram remembers the sheet it came from.
    to_source: gtk::Button,
}

impl Canvas {
    pub fn new() -> Canvas {
        let doc: Rc<RefCell<Option<Diagram>>> = Rc::new(RefCell::new(None));
        let rects = Rc::new(RefCell::new(Vec::new()));
        let sel: Rc<RefCell<Option<Sel>>> = Rc::new(RefCell::new(None));
        let armed: Rc<Cell<Option<Preset>>> = Rc::new(Cell::new(None));
        let drag = Rc::new(RefCell::new(Drag::None));
        let on_change: Rc<RefCell<Option<Box<dyn Fn()>>>> = Rc::new(RefCell::new(None));
        let rtl = Rc::new(Cell::new(false));

        let area = gtk::DrawingArea::builder()
            .content_width(WIDTH)
            .content_height(HEIGHT)
            .focusable(true)
            .build();
        area.add_css_class("oma-canvas");

        let scroller = gtk::ScrolledWindow::builder().vexpand(true).child(&area).build();

        // ---- palette -------------------------------------------------------
        let palette = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        palette.add_css_class("toolbar");
        palette.set_margin_top(6);
        palette.set_margin_bottom(6);
        palette.set_margin_start(8);
        palette.set_margin_end(8);

        let mut buttons = Vec::new();
        for preset in Preset::all() {
            let b = gtk::ToggleButton::new();
            b.set_child(Some(&stroke_icon(preset)));
            b.set_tooltip_text(Some(preset.label()));
            palette.append(&b);
            buttons.push((preset, b));
        }
        for (preset, button) in &buttons {
            let preset = *preset;
            let armed = armed.clone();
            let others: Vec<gtk::ToggleButton> =
                buttons.iter().map(|(_, b)| b.clone()).collect();
            let me = button.clone();
            button.connect_toggled(move |b| {
                if !b.is_active() {
                    if armed.get() == Some(preset) {
                        armed.set(None);
                    }
                    return;
                }
                armed.set(Some(preset));
                for other in &others {
                    if other != &me && other.is_active() {
                        other.set_active(false);
                    }
                }
            });
        }

        palette.append(&gtk::Separator::new(gtk::Orientation::Vertical));
        let add_label = gtk::Button::with_label("Label");
        add_label.set_tooltip_text(Some("Write a label of your own"));
        palette.append(&add_label);
        let delete = gtk::Button::from_icon_name("user-trash-symbolic");
        delete.set_tooltip_text(Some("Remove what is selected (Delete)"));
        palette.append(&delete);

        let to_source = gtk::Button::with_label("Sheet");
        to_source.set_tooltip_text(Some("Open the interlinear this came from"));
        to_source.set_visible(false);
        palette.append(&to_source);

        let hint = gtk::Label::new(Some("Pick a stroke, then click where it goes"));
        hint.add_css_class("dim-label");
        hint.set_hexpand(true);
        hint.set_halign(gtk::Align::End);
        // Without this the hint's full text becomes a minimum width, and with
        // the sidebar beside it the window can no longer fit a small screen.
        hint.set_ellipsize(pango::EllipsizeMode::End);
        palette.append(&hint);

        // The palette itself must never dictate how narrow the window can be.
        let palette_scroll = gtk::ScrolledWindow::builder()
            .vscrollbar_policy(gtk::PolicyType::Never)
            .hscrollbar_policy(gtk::PolicyType::Automatic)
            .child(&palette)
            .build();

        // ---- word bank -----------------------------------------------------
        let bankbox = gtk::FlowBox::builder()
            .selection_mode(gtk::SelectionMode::None)
            .homogeneous(false)
            .row_spacing(4)
            .column_spacing(6)
            .margin_top(6)
            .margin_bottom(6)
            .margin_start(10)
            .margin_end(10)
            .max_children_per_line(64)
            .build();
        let bank_scroll = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .min_content_height(64)
            .max_content_height(132)
            .propagate_natural_height(true)
            .child(&bankbox)
            .build();
        bank_scroll.add_css_class("oma-bank");

        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        root.append(&palette_scroll);
        root.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
        root.append(&scroller);
        root.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
        root.append(&bank_scroll);

        let canvas = Canvas {
            root,
            area: area.clone(),
            bankbox,
            doc: doc.clone(),
            rects: rects.clone(),
            sel: sel.clone(),
            armed: armed.clone(),
            drag: drag.clone(),
            on_change: on_change.clone(),
            rtl: rtl.clone(),
            to_source: to_source.clone(),
        };

        canvas.wire_drawing();
        canvas.wire_pointer(&buttons);
        canvas.wire_keys();
        canvas.wire_drop();

        {
            let c = canvas.handle();
            delete.connect_clicked(move |_| c.delete_selected());
        }
        {
            let c = canvas.handle();
            add_label.connect_clicked(move |b| c.ask_for_label(b));
        }

        canvas
    }

    /// The pieces a callback needs, without borrowing the widget tree.
    fn handle(&self) -> Handle {
        Handle {
            area: self.area.clone(),
            bankbox: self.bankbox.clone(),
            doc: self.doc.clone(),
            rects: self.rects.clone(),
            sel: self.sel.clone(),
            armed: self.armed.clone(),
            drag: self.drag.clone(),
            on_change: self.on_change.clone(),
            rtl: self.rtl.clone(),
        }
    }

    /// Called whenever the diagram changes, so it can be saved.
    pub fn connect_changed(&self, f: impl Fn() + 'static) {
        *self.on_change.borrow_mut() = Some(Box::new(f));
    }

    /// Called with the diagram's `source` when the sheet behind it is wanted.
    pub fn connect_open_source(&self, f: impl Fn(String) + 'static) {
        let doc = self.doc.clone();
        self.to_source.connect_clicked(move |_| {
            let source = doc.borrow().as_ref().and_then(|d| d.source.clone());
            if let Some(source) = source {
                f(source);
            }
        });
    }

    pub fn show(&self, diagram: Diagram, rtl: bool) {
        self.rtl.set(rtl);
        *self.sel.borrow_mut() = None;
        self.to_source.set_visible(diagram.source.is_some());
        *self.doc.borrow_mut() = Some(diagram);
        self.handle().refill_bank();
        self.area.queue_draw();
    }

    pub fn take(&self) -> Option<Diagram> {
        self.doc.borrow().clone()
    }

    pub fn clear(&self) {
        *self.doc.borrow_mut() = None;
        *self.sel.borrow_mut() = None;
        self.handle().refill_bank();
        self.area.queue_draw();
    }

    fn wire_drawing(&self) {
        let doc = self.doc.clone();
        let rects = self.rects.clone();
        let sel = self.sel.clone();
        self.area.set_draw_func(move |area, cr, _w, _h| {
            let held = doc.borrow();
            let Some(d) = held.as_ref() else {
                rects.borrow_mut().clear();
                return;
            };
            let fg = area.color();
            let sel = sel.borrow();

            for line in &d.lines {
                let chosen = sel.as_ref() == Some(&Sel::Line(line.id.clone()));
                cr.set_source_rgba(
                    fg.red() as f64,
                    fg.green() as f64,
                    fg.blue() as f64,
                    if chosen { 1.0 } else { 0.85 },
                );
                cr.set_line_width(if chosen { 2.6 } else { 1.6 });
                match line.stroke {
                    Stroke::Solid => cr.set_dash(&[], 0.0),
                    Stroke::Dotted => cr.set_dash(&[4.0, 4.0], 0.0),
                }
                cr.move_to(line.x1, line.y1);
                cr.line_to(line.x2, line.y2);
                let _ = cr.stroke();

                if chosen {
                    cr.set_dash(&[], 0.0);
                    for (hx, hy) in [(line.x1, line.y1), (line.x2, line.y2)] {
                        cr.rectangle(hx - 3.5, hy - 3.5, 7.0, 7.0);
                        let _ = cr.fill();
                    }
                }
            }

            cr.set_dash(&[], 0.0);
            let mut measured = Vec::new();
            for label in &d.labels {
                let layout = area.create_pango_layout(Some(&label.text));
                layout.set_font_description(Some(&pango::FontDescription::from_string(
                    "SBL BibLit, SBL Greek, SBL Hebrew 15",
                )));
                let (ink, _logical) = layout.pixel_extents();
                let baseline = layout.baseline() as f64 / pango::SCALE as f64;
                let (w, h) = layout.pixel_size();
                // The text rests on the point, as it rests on a line.
                let top = label.y - baseline - 2.0;
                let left = label.x;

                if sel.as_ref() == Some(&Sel::Label(label.id.clone())) {
                    cr.set_source_rgba(
                        fg.red() as f64,
                        fg.green() as f64,
                        fg.blue() as f64,
                        0.14,
                    );
                    cr.rectangle(left - 3.0, top - 1.0, w as f64 + 6.0, h as f64 + 2.0);
                    let _ = cr.fill();
                }

                cr.set_source_rgba(fg.red() as f64, fg.green() as f64, fg.blue() as f64, 1.0);
                cr.move_to(left, top);
                pangocairo::functions::show_layout(cr, &layout);
                let _ = ink;
                measured.push((label.id.clone(), left - 3.0, top - 1.0, w as f64 + 6.0, h as f64 + 2.0));
            }
            *rects.borrow_mut() = measured;
        });
    }

    fn wire_pointer(&self, buttons: &[(Preset, gtk::ToggleButton)]) {
        let gesture = gtk::GestureDrag::new();
        let c = self.handle();
        let palette: Vec<gtk::ToggleButton> = buttons.iter().map(|(_, b)| b.clone()).collect();
        gesture.connect_drag_begin(move |g, x, y| {
            g.widget().map(|w| w.grab_focus());
            if let Some(preset) = c.armed.take() {
                for b in &palette {
                    b.set_active(false);
                }
                let id = {
                    let mut held = c.doc.borrow_mut();
                    let Some(d) = held.as_mut() else { return };
                    d.add_line(preset, x, y)
                };
                // Carry on dragging it into place in the same gesture.
                let line = c.doc.borrow().as_ref().and_then(|d| d.line(&id).cloned());
                if let Some(l) = line {
                    *c.drag.borrow_mut() = Drag::MoveLine {
                        id: id.clone(),
                        x1: l.x1,
                        y1: l.y1,
                        x2: l.x2,
                        y2: l.y2,
                    };
                }
                *c.sel.borrow_mut() = Some(Sel::Line(id));
                c.changed();
                return;
            }
            *c.drag.borrow_mut() = c.pick(x, y);
            *c.sel.borrow_mut() = match &*c.drag.borrow() {
                Drag::MoveLabel { id, .. } => Some(Sel::Label(id.clone())),
                Drag::MoveLine { id, .. } | Drag::MoveEnd { id, .. } => Some(Sel::Line(id.clone())),
                Drag::None => None,
            };
            c.area.queue_draw();
        });

        let c = self.handle();
        gesture.connect_drag_update(move |g, dx, dy| {
            let Some((sx, sy)) = g.start_point() else { return };
            let job = c.drag.borrow().clone();
            {
                let mut held = c.doc.borrow_mut();
                let Some(d) = held.as_mut() else { return };
                match job {
                    Drag::None => return,
                    Drag::MoveLabel { id, x, y } => {
                        if let Some(l) = d.label_mut(&id) {
                            l.x = x + dx;
                            l.y = y + dy;
                        }
                    }
                    Drag::MoveLine { id, x1, y1, x2, y2 } => {
                        if let Some(l) = d.line_mut(&id) {
                            l.x1 = x1 + dx;
                            l.y1 = y1 + dy;
                            l.x2 = x2 + dx;
                            l.y2 = y2 + dy;
                        }
                    }
                    Drag::MoveEnd { id, end } => {
                        if let Some(l) = d.line_mut(&id) {
                            match end {
                                End::First => {
                                    l.x1 = sx + dx;
                                    l.y1 = sy + dy;
                                }
                                End::Second => {
                                    l.x2 = sx + dx;
                                    l.y2 = sy + dy;
                                }
                            }
                        }
                    }
                }
            }
            c.area.queue_draw();
        });

        let c = self.handle();
        gesture.connect_drag_end(move |_, _, _| {
            let moved = !matches!(&*c.drag.borrow(), Drag::None);
            *c.drag.borrow_mut() = Drag::None;
            if moved {
                c.changed();
            }
        });
        self.area.add_controller(gesture);
    }

    fn wire_keys(&self) {
        let c = self.handle();
        let keys = gtk::EventControllerKey::new();
        keys.connect_key_pressed(move |_, key, _, _| {
            if key == gdk::Key::Delete || key == gdk::Key::BackSpace {
                c.delete_selected();
                glib::Propagation::Stop
            } else if key == gdk::Key::Escape {
                *c.sel.borrow_mut() = None;
                c.area.queue_draw();
                glib::Propagation::Stop
            } else {
                glib::Propagation::Proceed
            }
        });
        self.area.add_controller(keys);
    }

    fn wire_drop(&self) {
        let c = self.handle();
        let target = gtk::DropTarget::new(String::static_type(), gdk::DragAction::COPY);
        target.connect_drop(move |_, value, x, y| {
            let Ok(word_id) = value.get::<String>() else {
                return false;
            };
            let placed = {
                let mut held = c.doc.borrow_mut();
                let Some(d) = held.as_mut() else { return false };
                d.place(&word_id, x, y)
            };
            match placed {
                Some(id) => {
                    *c.sel.borrow_mut() = Some(Sel::Label(id));
                    c.refill_bank();
                    c.changed();
                    true
                }
                None => false,
            }
        });
        self.area.add_controller(target);
    }
}

/// The shared state behind the widget, cloned into every callback.
#[derive(Clone)]
struct Handle {
    area: gtk::DrawingArea,
    bankbox: gtk::FlowBox,
    doc: Rc<RefCell<Option<Diagram>>>,
    rects: Rc<RefCell<Vec<(String, f64, f64, f64, f64)>>>,
    sel: Rc<RefCell<Option<Sel>>>,
    armed: Rc<Cell<Option<Preset>>>,
    drag: Rc<RefCell<Drag>>,
    on_change: Rc<RefCell<Option<Box<dyn Fn()>>>>,
    rtl: Rc<Cell<bool>>,
}

impl Handle {
    fn changed(&self) {
        self.area.queue_draw();
        if let Some(f) = self.on_change.borrow().as_ref() {
            f();
        }
    }

    /// What the pointer has taken hold of. Labels win over lines, because a
    /// word usually sits on one.
    fn pick(&self, x: f64, y: f64) -> Drag {
        for (id, rx, ry, rw, rh) in self.rects.borrow().iter().rev() {
            if x >= *rx && x <= rx + rw && y >= *ry && y <= ry + rh {
                if let Some(l) = self.doc.borrow().as_ref().and_then(|d| d.label(id)) {
                    return Drag::MoveLabel { id: id.clone(), x: l.x, y: l.y };
                }
            }
        }
        let held = self.doc.borrow();
        let Some(d) = held.as_ref() else { return Drag::None };
        let Some(line) = d.line_at(x, y, GRAB) else { return Drag::None };
        if let Some(end) = d.end_at(line, x, y, GRAB) {
            return Drag::MoveEnd { id: line.id.clone(), end };
        }
        Drag::MoveLine {
            id: line.id.clone(),
            x1: line.x1,
            y1: line.y1,
            x2: line.x2,
            y2: line.y2,
        }
    }

    fn delete_selected(&self) {
        let Some(sel) = self.sel.borrow_mut().take() else { return };
        {
            let mut held = self.doc.borrow_mut();
            let Some(d) = held.as_mut() else { return };
            match sel {
                Sel::Label(id) => {
                    d.remove_label(&id);
                }
                Sel::Line(id) => {
                    d.remove_line(&id);
                }
            }
        }
        self.refill_bank();
        self.changed();
    }

    fn ask_for_label(&self, anchor: &gtk::Button) {
        let popover = gtk::Popover::new();
        popover.set_parent(anchor);
        let column = gtk::Box::new(gtk::Orientation::Vertical, 8);
        column.set_margin_top(10);
        column.set_margin_bottom(10);
        column.set_margin_start(10);
        column.set_margin_end(10);
        let entry = gtk::Entry::builder().placeholder_text("x, (you), or a note").build();
        entry.set_width_chars(22);
        column.append(&entry);
        let add = gtk::Button::with_label("Add to the middle");
        add.add_css_class("suggested-action");
        column.append(&add);
        popover.set_child(Some(&column));

        let commit = {
            let me = self.clone();
            let entry = entry.clone();
            let popover = popover.clone();
            move || {
                let text = entry.text().trim().to_string();
                if text.is_empty() {
                    return;
                }
                // Into the middle of what is on screen, then dragged into place.
                let id = {
                    let mut held = me.doc.borrow_mut();
                    let Some(d) = held.as_mut() else { return };
                    d.add_free_label(text, 260.0, 120.0)
                };
                *me.sel.borrow_mut() = Some(Sel::Label(id));
                me.changed();
                popover.popdown();
            }
        };
        {
            let commit = commit.clone();
            add.connect_clicked(move |_| commit());
        }
        entry.connect_activate(move |_| commit());
        popover.connect_closed(|p| p.unparent());
        popover.popup();
        entry.grab_focus();
    }

    /// Rebuild the strip of words still to be placed.
    fn refill_bank(&self) {
        while let Some(child) = self.bankbox.first_child() {
            self.bankbox.remove(&child);
        }
        self.bankbox.set_direction(if self.rtl.get() {
            gtk::TextDirection::Rtl
        } else {
            gtk::TextDirection::Ltr
        });
        let held = self.doc.borrow();
        let Some(d) = held.as_ref() else { return };
        for word in d.bank() {
            let chip = gtk::Label::new(Some(&word.text));
            chip.add_css_class("oma-chip");
            chip.set_tooltip_text(word.gloss.as_deref());

            let source = gtk::DragSource::new();
            source.set_actions(gdk::DragAction::COPY);
            let id = word.id.clone();
            source.connect_prepare(move |_, _, _| {
                Some(gdk::ContentProvider::for_value(&id.to_value()))
            });
            // Drag the word itself, so it is clear what is being carried.
            let text = word.text.clone();
            source.connect_drag_begin(move |s, _| {
                let paintable = gtk::WidgetPaintable::new(s.widget().as_ref());
                let _ = paintable;
                let _ = &text;
            });
            chip.add_controller(source);
            self.bankbox.append(&chip);
        }
    }
}

/// A small drawing of a stroke, for its palette button.
fn stroke_icon(preset: Preset) -> gtk::DrawingArea {
    let area = gtk::DrawingArea::builder().content_width(22).content_height(22).build();
    area.set_draw_func(move |a, cr, w, h| {
        let fg = a.color();
        cr.set_source_rgba(fg.red() as f64, fg.green() as f64, fg.blue() as f64, 0.9);
        cr.set_line_width(1.6);
        match preset.stroke() {
            Stroke::Solid => cr.set_dash(&[], 0.0),
            Stroke::Dotted => cr.set_dash(&[3.0, 3.0], 0.0),
        }
        let (cx, cy) = (w as f64 / 2.0, h as f64 / 2.0);
        let (x1, y1, x2, y2) = preset.offsets();
        // The presets are drawn at full size; shrink them to the button.
        let scale = 0.26;
        cr.move_to(cx + x1 * scale, cy + y1 * scale);
        cr.line_to(cx + x2 * scale, cy + y2 * scale);
        let _ = cr.stroke();
        if preset == Preset::Base {
            return;
        }
        // Every stroke but the baseline is understood against one, so show it.
        cr.set_dash(&[], 0.0);
        cr.set_line_width(1.0);
        cr.set_source_rgba(fg.red() as f64, fg.green() as f64, fg.blue() as f64, 0.35);
        cr.move_to(3.0, cy);
        cr.line_to(w as f64 - 3.0, cy);
        let _ = cr.stroke();
    });
    area
}
