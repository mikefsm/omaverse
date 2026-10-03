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
/// How close a stroke or word has to come to rest for it to be held there.
const SNAP: f64 = 16.0;
/// How far the canvas will zoom, and the step a key press takes.
const ZOOM_MIN: f64 = 0.25;
const ZOOM_MAX: f64 = 4.0;
const ZOOM_STEP: f64 = 1.25;

/// How far back undo reaches. A diagram is small enough that keeping whole
/// copies is simpler, and cheaper, than working out how to reverse each change.
const HISTORY: usize = 120;
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
    /// Each carries where the pointer was last seen, in the diagram's own
    /// units. Moves are applied as the change since then, so whatever is held
    /// by what is moving comes along with it.
    MoveLabel { id: String, at: (f64, f64) },
    MoveLine { id: String, at: (f64, f64) },
    MoveEnd { id: String, end: End },
}

/// Where a label was drawn: its id and the box around it, in the diagram's own
/// units. Text extents are only known once drawn, so they are remembered.
type Measured = Rc<RefCell<Vec<(String, f64, f64, f64, f64)>>>;
/// What to call when the diagram changes, so it can be saved.
type OnChange = Rc<RefCell<Option<Box<dyn Fn()>>>>;

/// Snapshots either side of the present.
#[derive(Default)]
struct History {
    past: Vec<Diagram>,
    future: Vec<Diagram>,
}

pub struct Canvas {
    pub root: gtk::Box,
    area: gtk::DrawingArea,
    bankbox: gtk::FlowBox,
    doc: Rc<RefCell<Option<Diagram>>>,
    /// Where each label was drawn last time, for picking one out from under the
    /// pointer.
    rects: Measured,
    sel: Rc<RefCell<Option<Sel>>>,
    /// A palette stroke waiting for somewhere to go.
    armed: Rc<Cell<Option<Preset>>>,
    drag: Rc<RefCell<Drag>>,
    on_change: OnChange,
    rtl: Rc<Cell<bool>>,
    history: Rc<RefCell<History>>,
    /// How much bigger than its own units the canvas is drawn.
    zoom: Rc<Cell<f64>>,
    /// The window onto the canvas, which is what "fit" has to fit inside.
    scroller: gtk::ScrolledWindow,
    /// Shown only when the diagram remembers the sheet it came from.
    resync: gtk::Button,
    /// Live only while something held is selected, so it says what is attached.
    detach: gtk::Button,
}

impl Canvas {
    pub fn new() -> Canvas {
        let doc: Rc<RefCell<Option<Diagram>>> = Rc::new(RefCell::new(None));
        let rects = Rc::new(RefCell::new(Vec::new()));
        let sel: Rc<RefCell<Option<Sel>>> = Rc::new(RefCell::new(None));
        let armed: Rc<Cell<Option<Preset>>> = Rc::new(Cell::new(None));
        let drag = Rc::new(RefCell::new(Drag::None));
        let on_change: OnChange = Rc::new(RefCell::new(None));
        let rtl = Rc::new(Cell::new(false));
        let history: Rc<RefCell<History>> = Rc::new(RefCell::new(History::default()));
        let zoom = Rc::new(Cell::new(1.0));

        let area = gtk::DrawingArea::builder()
            .content_width(WIDTH)
            .content_height(HEIGHT)
            .focusable(true)
            .build();
        area.add_css_class("oma-canvas");

        let scroller = gtk::ScrolledWindow::builder().vexpand(true).child(&area).build();
        // Scrolling must land where it is put rather than gliding there: a
        // glide still running underneath a drag moves the canvas out from
        // under the pointer, and every coordinate is relative to the canvas.
        scroller.set_kinetic_scrolling(false);

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
        let detach = gtk::Button::with_label("Detach");
        detach.set_tooltip_text(Some(
            "Let go of what is selected, leaving it where it is (Ctrl+D). \
             Hold Ctrl while dragging to stop something attaching in the first place.",
        ));
        detach.set_sensitive(false);
        palette.append(&detach);

        let delete = gtk::Button::from_icon_name("user-trash-symbolic");
        delete.set_tooltip_text(Some("Remove what is selected (Delete)"));
        palette.append(&delete);

        // One slot in the palette rather than three: the row is already full,
        // and zoom is reached occasionally rather than per stroke.
        let zoom_out = gtk::Button::from_icon_name("zoom-out-symbolic");
        zoom_out.set_tooltip_text(Some("Zoom out (Ctrl+-)"));
        let zoom_fit = gtk::Button::from_icon_name("zoom-fit-best-symbolic");
        zoom_fit.set_tooltip_text(Some("Fit the whole diagram (Ctrl+9)"));
        let zoom_in = gtk::Button::from_icon_name("zoom-in-symbolic");
        zoom_in.set_tooltip_text(Some("Zoom in (Ctrl++)"));
        let zoom_one = gtk::Button::with_label("100%");
        zoom_one.set_tooltip_text(Some("Actual size (Ctrl+0)"));

        let zoom_row = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        zoom_row.set_margin_top(6);
        zoom_row.set_margin_bottom(6);
        zoom_row.set_margin_start(6);
        zoom_row.set_margin_end(6);
        zoom_row.append(&zoom_out);
        zoom_row.append(&zoom_one);
        zoom_row.append(&zoom_in);
        zoom_row.append(&zoom_fit);
        let zoom_pop = gtk::Popover::new();
        zoom_pop.set_child(Some(&zoom_row));
        let zoom_btn = gtk::MenuButton::builder()
            .icon_name("zoom-fit-best-symbolic")
            .tooltip_text("Zoom")
            .popover(&zoom_pop)
            .build();
        palette.append(&zoom_btn);

        let resync = gtk::Button::from_icon_name("view-refresh-symbolic");
        resync.set_tooltip_text(Some(
            "Re-read the interlinear: take its corrections and any words it has gained",
        ));
        resync.set_visible(false);
        palette.append(&resync);

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
            history: history.clone(),
            zoom: zoom.clone(),
            scroller: scroller.clone(),
            resync: resync.clone(),
            detach: detach.clone(),
        };

        canvas.wire_drawing();
        canvas.wire_pointer(&buttons);
        canvas.wire_keys();
        canvas.wire_drop();

        {
            let c = canvas.handle();
            zoom_in.connect_clicked(move |_| c.set_zoom(c.zoom.get() * ZOOM_STEP));
        }
        {
            let c = canvas.handle();
            zoom_out.connect_clicked(move |_| c.set_zoom(c.zoom.get() / ZOOM_STEP));
        }
        {
            let c = canvas.handle();
            zoom_fit.connect_clicked(move |_| c.fit());
        }
        {
            let c = canvas.handle();
            zoom_one.connect_clicked(move |_| c.set_zoom(1.0));
        }
        {
            let c = canvas.handle();
            detach.connect_clicked(move |_| c.detach_selected());
        }
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
            history: self.history.clone(),
            zoom: self.zoom.clone(),
            scroller: self.scroller.clone(),
            detach: self.detach.clone(),
        }
    }

    /// Called whenever the diagram changes, so it can be saved.
    pub fn connect_changed(&self, f: impl Fn() + 'static) {
        *self.on_change.borrow_mut() = Some(Box::new(f));
    }

    /// Called with the diagram's `source` when the sheet should be re-read.
    pub fn connect_resync(&self, f: impl Fn(String) + 'static) {
        let doc = self.doc.clone();
        self.resync.connect_clicked(move |_| {
            let source = doc.borrow().as_ref().and_then(|d| d.source.clone());
            if let Some(source) = source {
                f(source);
            }
        });
    }

    /// Replace the diagram wholesale, as an undoable step.
    pub fn apply(&self, diagram: Diagram) {
        let h = self.handle();
        h.checkpoint();
        *self.doc.borrow_mut() = Some(diagram);
        *self.sel.borrow_mut() = None;
        h.refill_bank();
        h.changed();
    }

    /// The interlinear this diagram was made from, if it remembers one.
    pub fn source(&self) -> Option<String> {
        self.doc.borrow().as_ref().and_then(|d| d.source.clone())
    }

    pub fn show(&self, diagram: Diagram, rtl: bool) {
        self.handle().set_zoom(1.0);
        self.rtl.set(rtl);
        *self.sel.borrow_mut() = None;
        *self.history.borrow_mut() = History::default();
        self.resync.set_visible(diagram.source.is_some());
        *self.doc.borrow_mut() = Some(diagram);
        self.handle().refill_bank();
        self.area.queue_draw();
    }

    pub fn take(&self) -> Option<Diagram> {
        self.doc.borrow().clone()
    }

    /// Is a diagram the thing on screen? Asked before sending undo here rather
    /// than to the text view.
    pub fn is_open(&self) -> bool {
        self.doc.borrow().is_some()
    }

    /// Zoom by a step, to actual size, or to fit. Driven from the window, which
    /// hears the keys wherever focus happens to be.
    pub fn zoom_by(&self, factor: f64) {
        let h = self.handle();
        h.set_zoom(self.zoom.get() * factor);
    }

    pub fn zoom_actual(&self) {
        self.handle().set_zoom(1.0);
    }

    pub fn zoom_fit(&self) {
        self.handle().fit();
    }

    pub const ZOOM_STEP: f64 = ZOOM_STEP;

    pub fn undo(&self) {
        self.handle().step(true);
    }

    pub fn redo(&self) {
        self.handle().step(false);
    }

    pub fn clear(&self) {
        *self.doc.borrow_mut() = None;
        *self.sel.borrow_mut() = None;
        *self.history.borrow_mut() = History::default();
        self.handle().refill_bank();
        self.area.queue_draw();
    }

    fn wire_drawing(&self) {
        let doc = self.doc.clone();
        let rects = self.rects.clone();
        let sel = self.sel.clone();
        let zoom = self.zoom.clone();
        self.area.set_draw_func(move |area, cr, _w, _h| {
            let scale = zoom.get();
            cr.scale(scale, scale);
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

            // The dots that say what is held to what.
            cr.set_source_rgba(fg.red() as f64, fg.green() as f64, fg.blue() as f64, 0.9);
            for (jx, jy) in d.joints() {
                cr.arc(jx, jy, 3.0, 0.0, std::f64::consts::TAU);
                let _ = cr.fill();
            }

            let mut measured = Vec::new();
            for label in &d.labels {
                let layout = area.create_pango_layout(Some(&label.text));
                layout.set_font_description(Some(&pango::FontDescription::from_string(
                    "SBL BibLit, SBL Greek, SBL Hebrew 15",
                )));
                let baseline = layout.baseline() as f64 / pango::SCALE as f64;
                let (w, h) = layout.pixel_size();
                let (ax, ay, angle) = d.anchor_of(label);
                // The word is centred on its point and rests on it, as a word
                // rests on a line.
                let (ox, oy) = (-(w as f64) / 2.0, -baseline - 2.0);

                let _ = cr.save();
                cr.translate(ax, ay);
                if angle != 0.0 {
                    cr.rotate(angle);
                }
                if sel.as_ref() == Some(&Sel::Label(label.id.clone())) {
                    cr.set_source_rgba(fg.red() as f64, fg.green() as f64, fg.blue() as f64, 0.14);
                    cr.rectangle(ox - 3.0, oy - 1.0, w as f64 + 6.0, h as f64 + 2.0);
                    let _ = cr.fill();
                }
                cr.set_source_rgba(fg.red() as f64, fg.green() as f64, fg.blue() as f64, 1.0);
                cr.move_to(ox, oy);
                pangocairo::functions::show_layout(cr, &layout);
                let _ = cr.restore();

                // Hit testing is on the upright box around where it was drawn:
                // close enough for a word, and far simpler than a rotated one.
                let reach = (w as f64 / 2.0) * angle.cos().abs() + (h as f64) * angle.sin().abs();
                measured.push((
                    label.id.clone(),
                    ax - reach - 3.0,
                    ay + oy - 1.0,
                    reach * 2.0 + 6.0,
                    h as f64 + 2.0,
                ));
            }
            *rects.borrow_mut() = measured;
        });
    }

    fn wire_pointer(&self, buttons: &[(Preset, gtk::ToggleButton)]) {
        let gesture = gtk::GestureDrag::new();
        let c = self.handle();
        let palette: Vec<gtk::ToggleButton> = buttons.iter().map(|(_, b)| b.clone()).collect();
        gesture.connect_drag_begin(move |g, x, y| {
            let (x, y) = c.at(x, y);
            g.widget().map(|w| w.grab_focus());
            if let Some(preset) = c.armed.take() {
                for b in &palette {
                    b.set_active(false);
                }
                c.checkpoint();
                let id = {
                    let mut held = c.doc.borrow_mut();
                    let Some(d) = held.as_mut() else { return };
                    d.add_line(preset, x, y)
                };
                // Carry on dragging it into place in the same gesture.
                *c.drag.borrow_mut() = Drag::MoveLine { id: id.clone(), at: (x, y) };
                *c.sel.borrow_mut() = Some(Sel::Line(id));
                c.changed();
                return;
            }
            let job = c.pick(x, y);
            if !matches!(job, Drag::None) {
                c.checkpoint();
            }
            *c.drag.borrow_mut() = job;
            *c.sel.borrow_mut() = match &*c.drag.borrow() {
                Drag::MoveLabel { id, .. } => Some(Sel::Label(id.clone())),
                Drag::MoveLine { id, .. } | Drag::MoveEnd { id, .. } => Some(Sel::Line(id.clone())),
                Drag::None => None,
            };
            c.sync_detach();
            c.area.queue_draw();
        });

        let c = self.handle();
        gesture.connect_drag_update(move |g, _, _| {
            // Where the pointer actually is, rather than how far the gesture
            // reports it has come: that offset is measured against the viewport
            // while everything here is in the canvas's own coordinates, and the
            // two differ by however far the canvas is scrolled.
            let Some((px, py)) = g.point(None) else { return };
            let (px, py) = c.at(px, py);
            let job = c.drag.borrow().clone();
            {
                let mut held = c.doc.borrow_mut();
                let Some(d) = held.as_mut() else { return };
                match job {
                    Drag::None => return,
                    Drag::MoveLabel { id, at } => {
                        // A word on a stroke slides along it, so a small drag
                        // adjusts its place rather than tearing it off. Pull
                        // further than that and it comes away.
                        let (step_x, step_y) = (px - at.0, py - at.1);
                        let Some((ax, ay, _)) = d.label(&id).map(|l| d.anchor_of(l)) else {
                            return;
                        };
                        let (wx, wy) = (ax + step_x, ay + step_y);
                        let resting = d.label(&id).and_then(|l| l.rest.clone());
                        match resting {
                            Some(rest) => match d.line(&rest.host) {
                                Some(host) => {
                                    let (t, away) = crate::diagram::nearest_param(host, wx, wy);
                                    if away > SNAP * 2.0 {
                                        d.free_label(&id);
                                        if let Some(l) = d.label_mut(&id) {
                                            l.x = wx;
                                            l.y = wy;
                                        }
                                    } else {
                                        d.set_rest(&id, &rest.host, t);
                                    }
                                }
                                None => d.free_label(&id),
                            },
                            None => {
                                if let Some(l) = d.label_mut(&id) {
                                    l.x = wx;
                                    l.y = wy;
                                }
                            }
                        }
                        *c.drag.borrow_mut() = Drag::MoveLabel { id, at: (px, py) };
                    }
                    Drag::MoveLine { id, at } => {
                        d.shift_line(&id, px - at.0, py - at.1);
                        *c.drag.borrow_mut() = Drag::MoveLine { id, at: (px, py) };
                    }
                    Drag::MoveEnd { id, end } => {
                        if let Some(l) = d.line_mut(&id) {
                            match end {
                                End::First => {
                                    l.x1 = px;
                                    l.y1 = py;
                                }
                                End::Second => {
                                    l.x2 = px;
                                    l.y2 = py;
                                }
                            }
                        }
                    }
                }
            }
            c.area.queue_draw();
        });

        let c = self.handle();
        gesture.connect_drag_end(move |g, _, _| {
            let job = c.drag.borrow().clone();
            *c.drag.borrow_mut() = Drag::None;
            // Holding Ctrl puts something down without it taking hold, for
            // when the stroke you want is next to the one that would catch it.
            let loose = g
                .current_event_state()
                .contains(gdk::ModifierType::CONTROL_MASK);
            {
                let mut held = c.doc.borrow_mut();
                let Some(d) = held.as_mut() else { return };
                if loose {
                    match &job {
                        Drag::MoveLine { id, .. } | Drag::MoveEnd { id, .. } => {
                            d.free_line(id);
                        }
                        Drag::MoveLabel { id, .. } => d.free_label(id),
                        Drag::None => return,
                    }
                    drop(held);
                    c.changed();
                    return;
                }
                // Wherever it came to rest, see what it came to rest against.
                match &job {
                    Drag::MoveLine { id, .. } | Drag::MoveEnd { id, .. } => {
                        d.settle_line(id, SNAP);
                    }
                    Drag::MoveLabel { id, .. } => {
                        if d.label(id).is_some_and(|l| l.rest.is_none()) {
                            d.settle_label(id, SNAP);
                        }
                    }
                    Drag::None => return,
                }
            }
            c.changed();
        });
        self.area.add_controller(gesture);
    }

    fn wire_keys(&self) {
        let c = self.handle();
        let keys = gtk::EventControllerKey::new();
        keys.connect_key_pressed(move |_, key, _, mods| {
            let ctrl = mods.contains(gdk::ModifierType::CONTROL_MASK);
            if ctrl && (key == gdk::Key::d || key == gdk::Key::D) {
                c.detach_selected();
                glib::Propagation::Stop
            } else if key == gdk::Key::Delete || key == gdk::Key::BackSpace {
                c.delete_selected();
                glib::Propagation::Stop
            } else if key == gdk::Key::Escape {
                *c.sel.borrow_mut() = None;
                c.sync_detach();
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
            let (x, y) = c.at(x, y);
            c.checkpoint();
            let placed = {
                let mut held = c.doc.borrow_mut();
                let Some(d) = held.as_mut() else { return false };
                let placed = d.place(&word_id, x, y);
                if let Some(id) = &placed {
                    d.settle_label(id, SNAP);
                }
                placed
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
    rects: Measured,
    sel: Rc<RefCell<Option<Sel>>>,
    armed: Rc<Cell<Option<Preset>>>,
    drag: Rc<RefCell<Drag>>,
    on_change: OnChange,
    rtl: Rc<Cell<bool>>,
    history: Rc<RefCell<History>>,
    zoom: Rc<Cell<f64>>,
    scroller: gtk::ScrolledWindow,
    detach: gtk::Button,
}

impl Handle {
    /// Remember the diagram as it stands, before changing it. Everything a
    /// user action does — a drag with its settling, a delete, a detach — is one
    /// checkpoint, so one undo takes back one action rather than part of one.
    fn checkpoint(&self) {
        let Some(now) = self.doc.borrow().clone() else { return };
        let mut history = self.history.borrow_mut();
        history.past.push(now);
        if history.past.len() > HISTORY {
            history.past.remove(0);
        }
        // A new change is a new branch: what was undone cannot be redone.
        history.future.clear();
    }

    /// Move one step back or forward through the snapshots.
    fn step(&self, back: bool) {
        let Some(now) = self.doc.borrow().clone() else { return };
        let moved = {
            let mut history = self.history.borrow_mut();
            let History { past, future } = &mut *history;
            let (from, to) = if back { (past, future) } else { (future, past) };
            from.pop().inspect(|_| to.push(now))
        };
        let Some(then) = moved else { return };
        *self.doc.borrow_mut() = Some(then);
        // Whatever was selected may not be there any more.
        *self.sel.borrow_mut() = None;
        self.refill_bank();
        self.changed();
    }

    fn changed(&self) {
        self.sync_detach();
        self.area.queue_draw();
        if let Some(f) = self.on_change.borrow().as_ref() {
            f();
        }
    }

    /// Where a pointer position falls in the diagram's own units.
    fn at(&self, x: f64, y: f64) -> (f64, f64) {
        let scale = self.zoom.get();
        (x / scale, y / scale)
    }

    /// Zoom about the middle of what is on screen, so the drawing does not
    /// slide out from under the pointer.
    fn set_zoom(&self, to: f64) {
        let scale = to.clamp(ZOOM_MIN, ZOOM_MAX);
        if (scale - self.zoom.get()).abs() < f64::EPSILON {
            return;
        }
        self.zoom.set(scale);
        self.area.set_content_width((WIDTH as f64 * scale) as i32);
        self.area.set_content_height((HEIGHT as f64 * scale) as i32);
        self.area.queue_draw();
    }

    /// Scale so the whole drawing is on screen at once.
    fn fit(&self) {
        let held = self.doc.borrow();
        let Some(d) = held.as_ref() else { return };
        let mut bounds: Option<(f64, f64, f64, f64)> = None;
        let mut stretch = |x: f64, y: f64| {
            bounds = Some(match bounds {
                None => (x, y, x, y),
                Some((x0, y0, x1, y1)) => (x0.min(x), y0.min(y), x1.max(x), y1.max(y)),
            });
        };
        for line in &d.lines {
            stretch(line.x1, line.y1);
            stretch(line.x2, line.y2);
        }
        for label in &d.labels {
            let (x, y, _) = d.anchor_of(label);
            stretch(x - 40.0, y - 20.0);
            stretch(x + 40.0, y + 8.0);
        }
        drop(held);
        let Some((x0, y0, x1, y1)) = bounds else { return };
        let (vw, vh) = (self.scroller.width() as f64, self.scroller.height() as f64);
        if vw < 1.0 || vh < 1.0 {
            return;
        }
        let margin = 48.0;
        let scale = ((vw - margin) / (x1 - x0).max(1.0)).min((vh - margin) / (y1 - y0).max(1.0));
        self.set_zoom(scale);

        // Scaling alone is not fitting: the drawing has to be brought into
        // view as well. The new size request has to be taken up first, so this
        // waits a turn.
        let scroller = self.scroller.clone();
        let scale = self.zoom.get();
        glib::idle_add_local_once(move || {
            let (w, h) = (scroller.width() as f64, scroller.height() as f64);
            let (dw, dh) = ((x1 - x0) * scale, (y1 - y0) * scale);
            scroller
                .hadjustment()
                .set_value((x0 * scale - (w - dw).max(0.0) / 2.0).max(0.0));
            scroller
                .vadjustment()
                .set_value((y0 * scale - (h - dh).max(0.0) / 2.0).max(0.0));
        });
    }

    /// What the pointer has taken hold of. Labels win over lines, because a
    /// word usually sits on one.
    fn pick(&self, x: f64, y: f64) -> Drag {
        for (id, rx, ry, rw, rh) in self.rects.borrow().iter().rev() {
            if x >= *rx && x <= rx + rw && y >= *ry && y <= ry + rh {
                return Drag::MoveLabel { id: id.clone(), at: (x, y) };
            }
        }
        let held = self.doc.borrow();
        let Some(d) = held.as_ref() else { return Drag::None };
        let Some(line) = d.line_at(x, y, GRAB) else { return Drag::None };
        if let Some(end) = d.end_at(line, x, y, GRAB) {
            return Drag::MoveEnd { id: line.id.clone(), end };
        }
        Drag::MoveLine { id: line.id.clone(), at: (x, y) }
    }

    /// Let go of whatever is selected without moving it. Dragging something
    /// clear of its host does this too, but not when another stroke is right
    /// there to catch it — which is exactly when a mistake gets made.
    fn detach_selected(&self) {
        let Some(sel) = self.sel.borrow().clone() else { return };
        self.checkpoint();
        let freed = {
            let mut held = self.doc.borrow_mut();
            let Some(d) = held.as_mut() else { return };
            match &sel {
                Sel::Line(id) => d.free_line(id),
                Sel::Label(id) => {
                    let was = d.is_held(id);
                    d.free_label(id);
                    was
                }
            }
        };
        if freed {
            self.changed();
        }
    }

    /// The Detach button is live only when there is something to let go of.
    fn sync_detach(&self) {
        let live = match self.sel.borrow().as_ref() {
            None => false,
            Some(Sel::Line(id)) | Some(Sel::Label(id)) => self
                .doc
                .borrow()
                .as_ref()
                .is_some_and(|d| d.is_held(id)),
        };
        self.detach.set_sensitive(live);
    }

    fn delete_selected(&self) {
        let Some(sel) = self.sel.borrow_mut().take() else { return };
        self.checkpoint();
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
                me.checkpoint();
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
