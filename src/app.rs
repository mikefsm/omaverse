//! UI: library sidebar | outline navigation | the whole document.
//!
//! The text buffer holds the file verbatim and is the single source of truth.
//! The outline tree is derived from it: a table of contents you navigate and
//! fold with, not a separate thing to keep in sync. Appearance is all tags, so
//! what is saved is exactly what was typed.

use crate::config::{self, Config};
use crate::docview;
use crate::edit::{self, Cmd};
use crate::library;
use crate::model::{parse_path_key, path_key, Document, NodePath, Note, NoteKind};
use crate::nodeobj::NodeObject;
use crate::parse;
use crate::state::{self, DocState};
use crate::theme;

use gtk4 as gtk;
use gtk4::gdk;
use gtk4::gio;
use gtk4::glib;
use gtk4::pango;
use gtk4::prelude::*;
use libadwaita as adw;
use adw::prelude::*;

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

const STYLE_MS: u64 = 200;
const SAVE_DOC_MS: u64 = 800;
const SAVE_STATE_MS: u64 = 2000;
const WATCH_MS: u64 = 1500;

const CSS: &str = "
.oma-doc { font-family: 'SBL BibLit','SBL Hebrew','SBL Greek','Source Serif 4','Noto Serif',serif; font-size: 12.5pt; }
.oma-outline { font-size: 10.5pt; }
.oma-group { font-size: 9pt; font-weight: bold; letter-spacing: 0.04em; }
.oma-error { color: #e01b24; }
";

pub struct App {
    cfg: Config,
    /// Cache of the parsed buffer, refreshed on a debounce. The buffer, not
    /// this, is authoritative.
    doc: Rc<RefCell<Document>>,
    path: RefCell<Option<PathBuf>>,
    dstate: RefCell<DocState>,
    selected: RefCell<Option<NodePath>>,

    dirty_doc: Cell<bool>,
    dirty_style: Cell<bool>,
    /// The cursor moved; the outline highlight needs catching up on a tick
    /// rather than synchronously, which would run on every keystroke.
    dirty_cursor: Cell<bool>,
    dirty_state: Cell<bool>,
    /// Guards against queueing the post-fold work more than once per pass.
    fold_pending: Cell<bool>,
    /// Expansion changes seen since the last settle, resolved together.
    pending_toggles: RefCell<Vec<(NodePath, bool)>>,
    /// Set while the app drives the widgets, so their change signals are not
    /// mistaken for the user typing.
    loading: Cell<bool>,

    window: adw::ApplicationWindow,
    wtitle: adw::WindowTitle,
    split: adw::OverlaySplitView,
    paned: gtk::Paned,
    stack: gtk::Stack,
    root_store: gio::ListStore,
    tree: gtk::TreeListModel,
    selection: gtk::SingleSelection,
    list: gtk::ListView,
    view: gtk::TextView,
    buffer: gtk::TextBuffer,
    libbox: gtk::ListBox,
    lib_paths: RefCell<Vec<Option<PathBuf>>>,
    recent: RefCell<Vec<PathBuf>>,
    /// Text lifted out of the buffer by folding, with a mark where it belongs.
    folds: RefCell<Vec<Fold>>,
    /// Size and modification time of the file as omaverse last left it, so a
    /// change made by anything else can be told apart from its own writes.
    disk_stamp: RefCell<Option<(u64, u64)>>,
    /// True while a conflict is unresolved. Autosave stops, so the file on disk
    /// is never overwritten while the question is still open.
    conflict: Cell<bool>,
    /// Whether the question has already been put to the user.
    conflict_asked: Cell<bool>,
    /// True while a note popover is open. The document's key handler runs in
    /// capture phase, so without this it intercepts Tab and Escape on their way
    /// down and the popover never sees them.
    note_open: Cell<bool>,
    /// The fold triangles drawn beside the text.
    gutter: RefCell<Option<gtk::DrawingArea>>,
    /// None when no dictionary is installed; spelling is simply not marked then.
    speller: Option<crate::spell::Speller>,
    /// A section lifted by Cut, waiting to be pasted. Kept here rather than on
    /// the system clipboard so cutting a section never clobbers what you copied.
    clipboard: RefCell<Option<crate::model::Node>>,
}

/// A folded-away section. GtkTextView has no real folding -- an `invisible` tag
/// hides the glyphs but keeps the line boxes, leaving a blank hole as tall as
/// what it hid -- so the text is removed from the buffer and parked here. The
/// mark moves with surrounding edits, so it still points at the right place
/// when the section is restored.
struct Fold {
    mark: gtk::TextMark,
    text: String,
}

pub fn build(gapp: &adw::Application, cli: Option<PathBuf>) -> Rc<App> {
    load_css();
    let themed = install_theme();
    apply_theme(&themed);
    // Switching omarchy theme rewrites the palette; notice it and restyle.
    {
        let provider = themed.clone();
        let mut seen = theme::current_name();
        glib::timeout_add_local(Duration::from_secs(4), move || {
            let now = theme::current_name();
            if now != seen {
                seen = now;
                apply_theme(&provider);
            }
            glib::ControlFlow::Continue
        });
    }
    let cfg = config::load();
    let wstate = state::load_window_state();
    let doc: Rc<RefCell<Document>> = Rc::new(RefCell::new(Document::default()));

    // ---- outline navigation ------------------------------------------------
    let root_store = gio::ListStore::new::<NodeObject>();
    let tree = gtk::TreeListModel::new(root_store.clone(), false, false, {
        let doc = doc.clone();
        move |item| {
            let obj = item.downcast_ref::<NodeObject>()?;
            let p = obj.path();
            let d = doc.borrow();
            let node = d.get(&p)?;
            if node.children.is_empty() {
                return None;
            }
            let store = gio::ListStore::new::<NodeObject>();
            for (i, c) in node.children.iter().enumerate() {
                let mut cp = p.clone();
                cp.push(i);
                store.append(&NodeObject::new(cp, &c.title, !c.children.is_empty()));
            }
            Some(store.upcast())
        }
    });
    let selection = gtk::SingleSelection::new(Some(tree.clone()));
    selection.set_autoselect(false);
    selection.set_can_unselect(true);

    let factory = gtk::SignalListItemFactory::new();
    factory.connect_unbind(|_, item| {
        let Some(item) = item.downcast_ref::<gtk::ListItem>() else { return };
        unsafe {
            if let Some(b) = item.steal_data::<glib::Binding>("oma-title-binding") {
                b.unbind();
            }
        }
    });

    let list = gtk::ListView::new(Some(selection.clone()), Some(factory.clone()));
    list.add_css_class("oma-outline");
    let outline_scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .child(&list)
        .build();

    // ---- the document ------------------------------------------------------
    let view = gtk::TextView::builder()
        .wrap_mode(gtk::WrapMode::Word)
        .left_margin(12)
        .right_margin(36)
        .top_margin(16)
        .bottom_margin(240) // room to scroll the last section up the page
        .build();
    view.add_css_class("oma-doc");
    let buffer = view.buffer();
    buffer.set_enable_undo(true);
    docview::install_tags(&buffer);

    let doc_scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .child(&view)
        .build();

    let paned = gtk::Paned::builder()
        .orientation(gtk::Orientation::Horizontal)
        .start_child(&outline_scroll)
        .end_child(&doc_scroll)
        .resize_start_child(false)
        .shrink_start_child(false)
        .shrink_end_child(false)
        .position(wstate.paned.unwrap_or(300))
        .build();

    let new_from_empty = gtk::Button::with_label("New outline");
    new_from_empty.add_css_class("suggested-action");
    new_from_empty.add_css_class("pill");
    new_from_empty.set_halign(gtk::Align::Center);
    let empty = adw::StatusPage::builder()
        .icon_name("view-list-symbolic")
        .title("No outline open")
        .description("Create one, pick a book from the sidebar, or pass a file on the command line.")
        .child(&new_from_empty)
        .build();

    let stack = gtk::Stack::new();
    stack.add_named(&empty, Some("empty"));
    stack.add_named(&paned, Some("doc"));
    stack.set_visible_child_name("empty");

    // ---- library sidebar ---------------------------------------------------
    let libbox = gtk::ListBox::new();
    libbox.set_selection_mode(gtk::SelectionMode::Single);
    libbox.add_css_class("navigation-sidebar");
    let lib_scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .child(&libbox)
        .build();
    let new_btn = gtk::Button::from_icon_name("list-add-symbolic");
    new_btn.set_tooltip_text(Some("New outline (Ctrl+N)"));
    let open_btn = gtk::Button::from_icon_name("document-open-symbolic");
    open_btn.set_tooltip_text(Some("Open an outline (Ctrl+O)"));
    let sb_header = adw::HeaderBar::new();
    sb_header.set_title_widget(Some(&adw::WindowTitle::new("Outlines", "")));
    sb_header.set_show_end_title_buttons(false);
    sb_header.pack_end(&new_btn);
    sb_header.pack_start(&open_btn);
    let sidebar = adw::ToolbarView::new();
    sidebar.add_top_bar(&sb_header);
    sidebar.set_content(Some(&lib_scroll));

    let split = adw::OverlaySplitView::builder()
        .sidebar(&sidebar)
        .min_sidebar_width(180.0)
        .max_sidebar_width(300.0)
        .show_sidebar(wstate.sidebar_open.unwrap_or(true))
        .build();

    // ---- window ------------------------------------------------------------
    let wtitle = adw::WindowTitle::new("omaverse", "");
    let header = adw::HeaderBar::new();
    header.set_title_widget(Some(&wtitle));
    let sb_toggle = gtk::ToggleButton::builder()
        .icon_name("sidebar-show-symbolic")
        .tooltip_text("Toggle outline library (Ctrl+\\)")
        .active(split.shows_sidebar())
        .build();
    header.pack_start(&sb_toggle);

    let shell = adw::ToolbarView::new();
    shell.add_top_bar(&header);
    shell.set_content(Some(&stack));
    split.set_content(Some(&shell));

    let window = adw::ApplicationWindow::builder()
        .application(gapp)
        .title("Omaverse")
        .default_width(wstate.width.unwrap_or(1180))
        .default_height(wstate.height.unwrap_or(760))
        .content(&split)
        .build();

    let app = Rc::new(App {
        cfg,
        doc,
        path: RefCell::new(None),
        dstate: RefCell::new(DocState::default()),
        selected: RefCell::new(None),
        dirty_doc: Cell::new(false),
        dirty_style: Cell::new(false),
        dirty_cursor: Cell::new(false),
        dirty_state: Cell::new(false),
        fold_pending: Cell::new(false),
        pending_toggles: RefCell::new(Vec::new()),
        loading: Cell::new(false),
        window: window.clone(),
        wtitle,
        split: split.clone(),
        paned,
        stack,
        root_store,
        tree,
        selection: selection.clone(),
        list: list.clone(),
        view: view.clone(),
        buffer: buffer.clone(),
        libbox: libbox.clone(),
        lib_paths: RefCell::new(Vec::new()),
        recent: RefCell::new(wstate.recent.clone()),
        folds: RefCell::new(Vec::new()),
        clipboard: RefCell::new(None),
        disk_stamp: RefCell::new(None),
        conflict: Cell::new(false),
        conflict_asked: Cell::new(false),
        note_open: Cell::new(false),
        gutter: RefCell::new(None),
        speller: crate::spell::Speller::new(),
    });

    // These closures hold a strong Rc to App, which owns the widgets. The cycle
    // is never collected, but App lives for the whole process.
    {
        let a = app.clone();
        factory.connect_setup(move |_, item| a.setup_row(item));
    }
    {
        let a = app.clone();
        factory.connect_bind(move |_, item| a.bind_row(item));
    }
    {
        let a = app.clone();
        selection.connect_selected_notify(move |_| a.on_outline_selected());
    }
    {
        let a = app.clone();
        buffer.connect_changed(move |_| {
            if !a.loading.get() {
                a.dirty_doc.set(true);
                a.dirty_style.set(true);
            }
        });
    }
    {
        // Keep the outline highlight on whatever section the cursor is in.
        let a = app.clone();
        buffer.connect_cursor_position_notify(move |_| {
            if !a.loading.get() {
                a.dirty_cursor.set(true);
                a.dirty_style.set(true);
            }
        });
    }
    {
        let a = app.clone();
        doc_scroll.vadjustment().connect_value_changed(move |_| {
            a.dirty_style.set(true);
            a.queue_gutter();
        });
    }
    {
        let a = app.clone();
        libbox.connect_row_activated(move |_, row| a.on_library_activated(row));
    }
    {
        let a = app.clone();
        new_btn.connect_clicked(move |_| a.new_outline());
    }
    {
        let a = app.clone();
        new_from_empty.connect_clicked(move |_| a.new_outline());
    }
    {
        let a = app.clone();
        open_btn.connect_clicked(move |_| a.open_dialog());
    }
    {
        let s = split.clone();
        sb_toggle.connect_toggled(move |b| s.set_show_sidebar(b.is_active()));
    }
    {
        let b = sb_toggle.clone();
        split.connect_show_sidebar_notify(move |s| b.set_active(s.shows_sidebar()));
    }

    // ---- keys in the outline pane -----------------------------------------
    {
        let a = app.clone();
        let kc = gtk::EventControllerKey::new();
        kc.connect_key_pressed(move |_, key, _, state| {
            let ctrl = state.contains(gdk::ModifierType::CONTROL_MASK);
            let alt = state.contains(gdk::ModifierType::ALT_MASK);
            let shift = state.contains(gdk::ModifierType::SHIFT_MASK);
            let enter = key == gdk::Key::Return || key == gdk::Key::KP_Enter;

            let cmd = if enter && !ctrl {
                Cmd::NewSiblingBelow
            } else if enter || key == gdk::Key::F2 {
                a.focus_document();
                return glib::Propagation::Stop;
            } else if key == gdk::Key::ISO_Left_Tab || (key == gdk::Key::Tab && shift) {
                Cmd::Outdent
            } else if key == gdk::Key::Tab {
                Cmd::Indent
            } else if key == gdk::Key::Up && alt {
                Cmd::MoveUp
            } else if key == gdk::Key::Down && alt {
                Cmd::MoveDown
            } else if key == gdk::Key::Left {
                a.collapse_or_parent();
                return glib::Propagation::Stop;
            } else if key == gdk::Key::Right {
                a.expand_or_child();
                return glib::Propagation::Stop;
            } else if key == gdk::Key::Delete {
                Cmd::Delete
            } else if ctrl && (key == gdk::Key::z || key == gdk::Key::Z) {
                if shift {
                    a.redo();
                } else {
                    a.undo();
                }
                return glib::Propagation::Stop;
            } else if ctrl && (key == gdk::Key::m || key == gdk::Key::M) {
                Cmd::MergeIntoPrevious
            } else if ctrl && (key == gdk::Key::x || key == gdk::Key::X) {
                Cmd::Cut
            } else if ctrl && (key == gdk::Key::v || key == gdk::Key::V) {
                match a.clipboard.borrow_mut().take() {
                    Some(n) => Cmd::Paste(n),
                    None => return glib::Propagation::Stop,
                }
            } else {
                return glib::Propagation::Proceed;
            };
            a.apply_cmd(cmd);
            glib::Propagation::Stop
        });
        kc.set_propagation_phase(gtk::PropagationPhase::Capture);
        outline_scroll.add_controller(kc);
    }
    {
        let a = app.clone();
        let v = view.clone();
        let click = gtk::GestureClick::new();
        click.connect_released(move |_, _, x, y| {
            let (bx, by) =
                v.window_to_buffer_coords(gtk::TextWindowType::Widget, x as i32, y as i32);
            let Some(iter) = v.iter_at_location(bx, by) else { return };
            if let Some(id) = a.note_at(&iter) {
                a.open_note(&id);
            }
        });
        view.add_controller(click);
    }
    {
        // Right-click offers corrections for the word under the pointer.
        let a = app.clone();
        let v = view.clone();
        let menu = gtk::GestureClick::new();
        menu.set_button(gdk::BUTTON_SECONDARY);
        menu.connect_released(move |_, _, x, y| {
            let (bx, by) =
                v.window_to_buffer_coords(gtk::TextWindowType::Widget, x as i32, y as i32);
            if let Some(iter) = v.iter_at_location(bx, by) {
                a.buffer.place_cursor(&iter);
                a.show_spelling();
            }
        });
        view.add_controller(menu);
    }
    // Ctrl+Enter and Escape move between the document and the outline.
    {
        let a = app.clone();
        let kc = gtk::EventControllerKey::new();
        kc.connect_key_pressed(move |_, key, _, state| {
            // A note popover is a child of the text view, so these capture-phase
            // handlers would otherwise swallow its keys before it sees them.
            if a.note_open.get() {
                return glib::Propagation::Proceed;
            }
            let enter = key == gdk::Key::Return || key == gdk::Key::KP_Enter;
            let ctrl = state.contains(gdk::ModifierType::CONTROL_MASK);
            let shift = state.contains(gdk::ModifierType::SHIFT_MASK);
            let alt = state.contains(gdk::ModifierType::ALT_MASK);
            // Ctrl+Enter splits here rather than returning to the outline, which
            // Escape already does. Alt+Enter is not available: omarchy binds it.
            if enter && ctrl {
                a.split_at_cursor();
                return glib::Propagation::Stop;
            }
            if key == gdk::Key::Escape {
                a.focus_outline();
                return glib::Propagation::Stop;
            }
            // Undo and redo are handled here rather than left to accelerators:
            // <Primary><Shift>z never matched, while the same two-modifier
            // combination reaches this controller reliably.
            if ctrl && (key == gdk::Key::z || key == gdk::Key::Z) {
                if shift {
                    a.redo();
                } else {
                    a.undo();
                }
                return glib::Propagation::Stop;
            }
            if key == gdk::Key::F7 {
                a.show_spelling();
                return glib::Propagation::Stop;
            }
            // Annotate the selection, or reopen the note the cursor sits in.
            if ctrl && shift && (key == gdk::Key::a || key == gdk::Key::A) {
                a.annotate_selection();
                return glib::Propagation::Stop;
            }
            // Restructuring works here too. Requiring a trip to the outline pane
            // for it meant these keys silently did nothing in the place the work
            // actually happens.
            let cmd = if key == gdk::Key::ISO_Left_Tab || (key == gdk::Key::Tab && shift) {
                Cmd::Outdent
            } else if key == gdk::Key::Tab {
                Cmd::Indent
            } else if key == gdk::Key::Up && alt {
                Cmd::MoveUp
            } else if key == gdk::Key::Down && alt {
                Cmd::MoveDown
            } else {
                return glib::Propagation::Proceed;
            };
            a.apply_cmd_in_document(cmd);
            glib::Propagation::Stop
        });
        kc.set_propagation_phase(gtk::PropagationPhase::Capture);
        doc_scroll.add_controller(kc);
    }

    // ---- actions -----------------------------------------------------------
    add_action(&window, "save", {
        let a = app.clone();
        move || {
            a.save_doc();
            a.dirty_doc.set(false);
        }
    });
    add_action(&window, "toggle-sidebar", {
        let s = split.clone();
        move || s.set_show_sidebar(!s.shows_sidebar())
    });
    add_action(&window, "new-outline", {
        let a = app.clone();
        move || a.new_outline()
    });
    add_action(&window, "open-outline", {
        let a = app.clone();
        move || a.open_dialog()
    });
    // Undo and redo as window actions rather than the text view's own bindings,
    // so they work from the outline pane too.
    add_action(&window, "undo", {
        let a = app.clone();
        move || a.undo()
    });
    add_action(&window, "redo", {
        let a = app.clone();
        move || a.redo()
    });
    add_action(&window, "close", {
        let w = window.clone();
        move || w.close()
    });
    gapp.set_accels_for_action("win.save", &["<Primary>s"]);
    gapp.set_accels_for_action("win.toggle-sidebar", &["<Primary>backslash"]);
    gapp.set_accels_for_action("win.new-outline", &["<Primary>n"]);
    gapp.set_accels_for_action("win.open-outline", &["<Primary>o"]);
    gapp.set_accels_for_action("win.close", &["<Primary>w", "<Primary>q"]);
    gapp.set_accels_for_action("win.undo", &["<Primary>z"]);
    // Both spellings: with Shift held the keyval is uppercase, and matching
    // only the lower-case form silently never fires.
    gapp.set_accels_for_action(
        "win.redo",
        &["<Primary><Shift>z", "<Primary><Shift>Z", "<Primary>y"],
    );

    // Styling keeps up with typing; re-parsing and saving run slower.
    {
        let a = app.clone();
        glib::timeout_add_local(Duration::from_millis(STYLE_MS), move || {
            if a.dirty_style.get() {
                a.dirty_style.set(false);
                a.restyle();
            }
            if a.dirty_cursor.get() {
                a.dirty_cursor.set(false);
                a.sync_outline_to_cursor();
            }
            glib::ControlFlow::Continue
        });
    }
    {
        let a = app.clone();
        glib::timeout_add_local(Duration::from_millis(SAVE_DOC_MS), move || {
            if a.dirty_doc.get() && !a.conflict.get() {
                a.dirty_doc.set(false);
                a.refresh_structure();
                a.save_doc();
            }
            glib::ControlFlow::Continue
        });
    }
    {
        let a = app.clone();
        glib::timeout_add_local(Duration::from_millis(WATCH_MS), move || {
            a.check_disk();
            glib::ControlFlow::Continue
        });
    }
    {
        let a = app.clone();
        glib::timeout_add_local(Duration::from_millis(SAVE_STATE_MS), move || {
            if a.dirty_state.get() {
                a.dirty_state.set(false);
                a.save_state();
            }
            glib::ControlFlow::Continue
        });
    }
    {
        let a = app.clone();
        window.connect_close_request(move |_| {
            a.save_doc();
            a.save_state();
            a.save_window_state();
            glib::Propagation::Proceed
        });
    }

    app.install_gutter();
    app.refresh_library();
    if let Some(p) = cli.or(wstate.last_file).filter(|p| p.exists()) {
        app.open(&p);
    }
    app
}

fn add_action(window: &adw::ApplicationWindow, name: &str, f: impl Fn() + 'static) {
    let a = gio::SimpleAction::new(name, None);
    a.connect_activate(move |_, _| f());
    window.add_action(&a);
}

/// A provider at USER priority, above both the app's own stylesheet and
/// libadwaita's, so the palette's colour definitions win.
fn install_theme() -> gtk::CssProvider {
    let provider = gtk::CssProvider::new();
    if let Some(display) = gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_USER,
        );
    }
    provider
}

fn apply_theme(provider: &gtk::CssProvider) {
    let Some(palette) = theme::Palette::load() else { return };
    provider.load_from_string(&palette.css());
    // Follow the theme's own light/dark rather than the desktop's preference,
    // which can disagree with the palette that is actually loaded.
    adw::StyleManager::default().set_color_scheme(if palette.dark {
        adw::ColorScheme::ForceDark
    } else {
        adw::ColorScheme::ForceLight
    });
}

fn load_css() {
    let provider = gtk::CssProvider::new();
    provider.load_from_string(CSS);
    if let Some(display) = gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
}

impl App {
    pub fn present(&self) {
        self.window.present();
        // A widget cannot take focus before it is mapped, and the first document
        // opens while the window is still being built.
        let list = self.list.clone();
        let view = self.view.clone();
        let tree = self.tree.clone();
        glib::idle_add_local_once(move || {
            // A document with no sections yet -- text pasted in, nothing marked
            // up -- leaves the outline empty, and an empty list cannot take
            // focus. Land in the text instead, which is the only place there is
            // anything to do.
            if tree.n_items() > 0 {
                list.grab_focus();
            } else {
                view.grab_focus();
            }
        });
    }

    fn text(&self) -> String {
        let (s, e) = self.buffer.bounds();
        self.buffer.text(&s, &e, true).to_string()
    }

    // ---- rows --------------------------------------------------------------

    /// Build a row once: label, expander, and the drag/drop controllers.
    /// Controllers go on here rather than in `bind` because rows are recycled --
    /// binding would add a fresh pair every time the row scrolled past.
    fn setup_row(self: &Rc<Self>, item: &glib::Object) {
        let Some(item) = item.downcast_ref::<gtk::ListItem>() else { return };
        let label = gtk::Label::builder()
            .xalign(0.0)
            .ellipsize(pango::EllipsizeMode::End)
            .build();
        let expander = gtk::TreeExpander::new();
        expander.set_child(Some(&label));
        item.set_child(Some(&expander));

        let drag = gtk::DragSource::new();
        drag.set_actions(gdk::DragAction::MOVE);
        {
            let ex = expander.clone();
            drag.connect_prepare(move |_, _, _| {
                let ptr = unsafe { ex.data::<String>("oma-path") }?;
                let key = unsafe { ptr.as_ref() }.clone();
                Some(gdk::ContentProvider::for_value(&key.to_value()))
            });
        }
        expander.add_controller(drag);

        let drop = gtk::DropTarget::new(glib::Type::STRING, gdk::DragAction::MOVE);
        {
            let me = self.clone();
            let ex = expander.clone();
            drop.connect_drop(move |_, value, _x, y| {
                let Ok(from) = value.get::<String>() else { return false };
                let Some(ptr) = (unsafe { ex.data::<String>("oma-path") }) else { return false };
                let to = unsafe { ptr.as_ref() }.clone();
                let height = ex.height() as f64;
                let zone = if height > 0.0 { y / height } else { 0.5 };
                // The move rebuilds the outline, destroying every row including
                // the one whose drop controller is running right now. Doing that
                // inside the drop hangs GTK mid-drag, so let the drag finish
                // first and move on the next idle.
                let me = me.clone();
                glib::idle_add_local_once(move || me.drop_section(&from, &to, zone));
                true
            });
        }
        expander.add_controller(drop);

    }

    // ---- spelling ----------------------------------------------------------

    /// The word the cursor is in, with its bounds in the buffer.
    fn word_at_cursor(&self) -> Option<(String, gtk::TextIter, gtk::TextIter)> {
        let at = self.buffer.iter_at_mark(&self.buffer.get_insert());
        let line = at.line();
        let column = at.line_offset() as usize;
        let text = self.text();
        let raw = text.lines().nth(line as usize)?;
        let (from, to) = crate::spell::words(raw)
            .into_iter()
            .find(|&(a, b)| column >= a && column <= b)?;
        let word: String = raw.chars().take(to).skip(from).collect();
        let mut start = self.buffer.iter_at_line(line)?;
        start.forward_chars(from as i32);
        let mut end = self.buffer.iter_at_line(line)?;
        end.forward_chars(to as i32);
        Some((word, start, end))
    }

    /// Where a position in the text sits on screen.
    fn rect_for(&self, at: &gtk::TextIter) -> gdk::Rectangle {
        let place = self.view.iter_location(at);
        let (x, y) =
            self.view
                .buffer_to_window_coords(gtk::TextWindowType::Widget, place.x(), place.y());
        gdk::Rectangle::new(x, y, place.width().max(1), place.height())
    }

    /// Offer corrections for the misspelled word under the cursor.
    fn show_spelling(self: &Rc<Self>) {
        let Some(speller) = self.speller.as_ref() else { return };
        let Some((word, start, end)) = self.word_at_cursor() else { return };
        if speller.is_correct(&word) {
            return;
        }
        let suggestions = speller.suggest(&word);

        let popover = gtk::Popover::new();
        popover.set_parent(&self.view);
        popover.set_autohide(true);
        popover.set_pointing_to(Some(&self.rect_for(&start)));

        let column = gtk::Box::new(gtk::Orientation::Vertical, 4);
        column.set_margin_top(8);
        column.set_margin_bottom(8);
        column.set_margin_start(8);
        column.set_margin_end(8);
        column.set_size_request(220, -1);

        if suggestions.is_empty() {
            let none = gtk::Label::new(Some("No suggestions"));
            none.add_css_class("dim-label");
            none.set_margin_bottom(4);
            column.append(&none);
        }
        for suggestion in &suggestions {
            let button = gtk::Button::with_label(suggestion);
            button.add_css_class("flat");
            button.set_halign(gtk::Align::Fill);
            let me = self.clone();
            let popover2 = popover.clone();
            let replacement = suggestion.clone();
            let (from, to) = (start.offset(), end.offset());
            button.connect_clicked(move |_| {
                me.replace_range(from, to, &replacement);
                popover2.popdown();
            });
            column.append(&button);
        }

        column.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
        let learn = gtk::Button::with_label("Add to dictionary");
        learn.add_css_class("flat");
        {
            let me = self.clone();
            let popover2 = popover.clone();
            let word = word.clone();
            learn.connect_clicked(move |_| {
                if let Some(speller) = me.speller.as_ref() {
                    speller.learn(&word);
                }
                me.dirty_style.set(true);
                me.restyle();
                popover2.popdown();
            });
        }
        column.append(&learn);

        popover.set_child(Some(&column));
        {
            let me = self.clone();
            popover.connect_closed(move |p| {
                me.note_open.set(false);
                p.unparent();
            });
        }
        // Reuses the note flag: it means "a popover owns the keyboard", which is
        // what the capture-phase handlers need to know.
        self.note_open.set(true);
        popover.popup();
    }

    fn replace_range(&self, from: i32, to: i32, text: &str) {
        let mut start = self.buffer.iter_at_offset(from);
        let mut end = self.buffer.iter_at_offset(to);
        self.buffer.delete(&mut start, &mut end);
        self.buffer.insert(&mut start, text);
        self.dirty_doc.set(true);
        self.dirty_style.set(true);
    }

    // ---- fold triangles ----------------------------------------------------

    /// A section can be folded if it has anything underneath to hide. Text
    /// counts: a section holding a whole book and no subsections yet is exactly
    /// the one you most want to collapse.
    fn foldable(node: &crate::model::Node) -> bool {
        !node.children.is_empty() || !node.body.trim().is_empty()
    }

    /// Triangles live in the text view's own left gutter, so they sit beside
    /// each heading and scroll with it without being part of the text -- putting
    /// a marker character in the buffer would write it to the file.
    fn install_gutter(self: &Rc<Self>) {
        let area = gtk::DrawingArea::new();
        area.set_size_request(22, -1);
        {
            let me = self.clone();
            area.set_draw_func(move |area, cr, _, _| me.draw_gutter(area, cr));
        }
        {
            let me = self.clone();
            let click = gtk::GestureClick::new();
            click.connect_released(move |_, _, _, y| me.toggle_fold_at(y));
            area.add_controller(click);
        }
        self.view.set_gutter(gtk::TextWindowType::Left, Some(&area));
        *self.gutter.borrow_mut() = Some(area);
    }

    fn queue_gutter(&self) {
        if let Some(area) = self.gutter.borrow().as_ref() {
            area.queue_draw();
        }
    }

    /// Heading lines currently on screen, with the path and foldability of each.
    fn visible_headings(&self) -> Vec<(i32, NodePath, bool)> {
        // Line numbers come from the buffer, which is what is on screen. Whether
        // a section can be folded comes from the whole document: folding removes
        // a section's contents from the buffer, so judging by the buffer would
        // make an already-folded section look empty and drop its triangle --
        // leaving no way to reopen it.
        let text = self.text();
        let onscreen = parse::parse(&text);
        let walk = onscreen.walk();
        let heads = docview::heading_lines(&text);
        let rect = self.view.visible_rect();
        let (top, _) = self.view.line_at_y(rect.y());
        let (bottom, _) = self.view.line_at_y(rect.y() + rect.height());
        (top.line()..=bottom.line())
            .filter_map(|line| {
                let idx = heads.iter().position(|&l| l == line as usize)?;
                let (path, _) = walk.get(idx)?;
                Some((line, path.clone(), self.is_foldable(path)))
            })
            .collect()
    }

    fn draw_gutter(&self, area: &gtk::DrawingArea, cr: &gtk4::cairo::Context) {
        let collapsed = self.dstate.borrow().collapsed.clone();
        let colour = area.color();
        cr.set_source_rgba(
            colour.red() as f64,
            colour.green() as f64,
            colour.blue() as f64,
            0.5,
        );
        for (line, path, foldable) in self.visible_headings() {
            if !foldable {
                continue;
            }
            let Some(iter) = self.buffer.iter_at_line(line) else { continue };
            let (y, height) = self.view.line_yrange(&iter);
            let (_, wy) = self
                .view
                .buffer_to_window_coords(gtk::TextWindowType::Left, 0, y);
            let middle = wy as f64 + height as f64 / 2.0;
            triangle(cr, 11.0, middle, !collapsed.contains(&path_key(&path)));
        }
        let _ = cr.fill();
    }

    fn toggle_fold_at(self: &Rc<Self>, y: f64) {
        let (_, by) = self
            .view
            .window_to_buffer_coords(gtk::TextWindowType::Left, 0, y as i32);
        let (iter, _) = self.view.line_at_y(by);
        let Some((_, path, foldable)) = self
            .visible_headings()
            .into_iter()
            .find(|(line, _, _)| *line == iter.line())
        else {
            return;
        };
        if !foldable {
            return;
        }
        // Folding rewrites the buffer this gutter is drawn against. Doing that
        // inside the click is the shape that hung the app on a drag, so it waits
        // until the gesture has finished.
        let me = self.clone();
        glib::idle_add_local_once(move || me.toggle_fold(&path));
    }

    /// Fold or unfold a section, keeping the outline and the gutter in step.
    fn toggle_fold(&self, path: &[usize]) {
        let key = path_key(path);
        {
            let mut st = self.dstate.borrow_mut();
            if !st.collapsed.remove(&key) {
                st.collapsed.insert(key);
            }
        }
        self.dirty_state.set(true);
        self.apply_folds();
        self.loading.set(true);
        self.apply_expansion();
        self.loading.set(false);
        self.queue_gutter();
    }

    /// Whether this section has anything under it to fold away.
    fn is_foldable(&self, path: &[usize]) -> bool {
        self.doc.borrow().get(path).map(Self::foldable).unwrap_or(false)
    }

    // ---- notes ------------------------------------------------------------

    /// Which note, if any, the given place in the text belongs to.
    fn note_at(&self, iter: &gtk::TextIter) -> Option<String> {
        let text = self.text();
        let line = text.lines().nth(iter.line() as usize)?;
        let col = iter.line_offset() as usize;
        docview::anchors(line)
            .into_iter()
            .find(|a| col >= a.text.0 && col <= a.text.1)
            .map(|a| a.id)
    }

    /// Wrap the selected words and open an empty note on them.
    fn annotate_selection(self: &Rc<Self>) {
        let Some((mut start, mut end)) = self.buffer.selection_bounds() else {
            // No selection: if the cursor sits in an existing note, open it.
            let at = self.buffer.iter_at_mark(&self.buffer.get_insert());
            if let Some(id) = self.note_at(&at) {
                self.open_note(&id);
            }
            return;
        };
        if start.line() != end.line() {
            return; // an anchor lives within one line
        }
        let selected = self.buffer.text(&start, &end, false).to_string();
        if selected.trim().is_empty() || selected.contains("==") {
            return;
        }
        // Keep any whitespace the selection picked up outside the markers, so
        // the underline sits on the words and the Markdown stays well formed.
        let words = selected.trim();
        let lead = &selected[..selected.len() - selected.trim_start().len()];
        let trail = &selected[selected.trim_end().len()..];
        let id = parse::parse(&self.full_text()).fresh_note_id();

        self.loading.set(true);
        self.buffer.delete(&mut start, &mut end);
        self.buffer.insert(&mut start, &format!("{lead}=={words}==[^{id}]{trail}"));
        // The definition goes at the foot of the file, where notes live.
        let mut tail = self.buffer.end_iter();
        self.buffer.insert(&mut tail, &format!("\n[^{id}]: \n"));
        self.loading.set(false);

        self.dirty_doc.set(true);
        self.dirty_style.set(true);
        self.open_note(&id);
    }

    /// Show a note for reading and editing, anchored where its words are.
    fn open_note(self: &Rc<Self>, id: &str) {
        let doc = parse::parse(&self.full_text());
        let note = doc
            .note(id)
            .cloned()
            .unwrap_or_else(|| Note::comment(id, ""));
        let sources = doc.sources();

        let popover = gtk::Popover::new();
        popover.set_parent(&self.view);
        popover.set_autohide(true);
        popover.set_pointing_to(Some(&self.anchor_rect(id)));

        let column = gtk::Box::new(gtk::Orientation::Vertical, 10);
        column.set_margin_top(10);
        column.set_margin_bottom(10);
        column.set_margin_start(10);
        column.set_margin_end(10);
        column.set_size_request(380, -1);

        // Whose words these are.
        let kinds = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        kinds.add_css_class("linked");
        let mine = gtk::ToggleButton::with_label("My comment");
        let quoted = gtk::ToggleButton::with_label("Quotation");
        quoted.set_group(Some(&mine));
        mine.set_hexpand(true);
        quoted.set_hexpand(true);
        match note.kind {
            NoteKind::Comment => mine.set_active(true),
            NoteKind::Quotation => quoted.set_active(true),
        }
        kinds.append(&mine);
        kinds.append(&quoted);

        let body = gtk::TextView::builder()
            .wrap_mode(gtk::WrapMode::Word)
            // Tab must move to the next field, not insert a tab: otherwise
            // there is no way out of the note to reach the source.
            .accepts_tab(false)
            .top_margin(6)
            .bottom_margin(6)
            .left_margin(6)
            .right_margin(6)
            .build();
        body.buffer().set_text(&note.text);
        let scroller = gtk::ScrolledWindow::builder()
            .min_content_height(90)
            .max_content_height(260)
            .propagate_natural_height(true)
            .child(&body)
            .build();
        scroller.add_css_class("card");
        column.append(&scroller);
        // After the note, not before it: you write first, then say what it is.
        // It also keeps every field reachable going forward with Tab, which is
        // where GTK's focus handling kept breaking.
        column.append(&kinds);

        // Attribution, shown only for a quotation.
        let source = gtk::Entry::builder()
            .placeholder_text("Source — author, work, page (leave empty for your own note)")
            .text(note.source.clone().unwrap_or_default())
            .build();
        column.append(&source);

        let recent = gtk::Box::new(gtk::Orientation::Vertical, 4);
        for known in sources.iter().take(6) {
            let button = gtk::Button::with_label(known);
            button.add_css_class("flat");
            button.set_halign(gtk::Align::Start);
            let entry = source.clone();
            let value = known.clone();
            button.connect_clicked(move |_| entry.set_text(&value));
            recent.append(&button);
        }
        column.append(&recent);

        // A source only means something for a quotation.
        let reveal_source = {
            let source = source.clone();
            let recent = recent.clone();
            move |quoting: bool| {
                source.set_visible(quoting);
                recent.set_visible(quoting && recent.first_child().is_some());
            }
        };
        reveal_source(note.kind == NoteKind::Quotation);
        {
            let reveal_source = reveal_source.clone();
            quoted.connect_toggled(move |b| reveal_source(b.is_active()));
        }

        let actions = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        let remove = gtk::Button::with_label("Delete");
        remove.add_css_class("destructive-action");
        let done = gtk::Button::with_label("Done");
        done.add_css_class("suggested-action");
        done.set_hexpand(true);
        done.set_halign(gtk::Align::End);
        actions.append(&remove);
        actions.append(&done);
        column.append(&actions);

        popover.set_child(Some(&column));

        {
            let me = self.clone();
            let id = id.to_string();
            let body = body.clone();
            let source = source.clone();
            let quoted = quoted.clone();
            let popover = popover.clone();
            done.connect_clicked(move |_| {
                let buffer = body.buffer();
                let (s, e) = buffer.bounds();
                let text = buffer.text(&s, &e, false).to_string();
                let src = source.text().to_string();
                let kind = if quoted.is_active() {
                    NoteKind::Quotation
                } else {
                    NoteKind::Comment
                };
                me.save_note(&id, kind, &text, &src);
                popover.popdown();
            });
        }
        {
            let me = self.clone();
            let id = id.to_string();
            let popover = popover.clone();
            remove.connect_clicked(move |_| {
                me.delete_note(&id);
                popover.popdown();
            });
        }
        {
            let me = self.clone();
            popover.connect_closed(move |p| {
                me.note_open.set(false);
                // The popover is built fresh each time it is opened.
                p.unparent();
            });
        }
        self.note_open.set(true);
        // Enter saves, Escape abandons -- so a note can be written and filed
        // without ever leaving the keyboard.
        {
            let done = done.clone();
            let popover2 = popover.clone();
            let keys = gtk::EventControllerKey::new();
            keys.connect_key_pressed(move |_, key, _, state| {
                let enter = key == gdk::Key::Return || key == gdk::Key::KP_Enter;
                if enter && state.contains(gdk::ModifierType::CONTROL_MASK) {
                    done.emit_clicked();
                    glib::Propagation::Stop
                } else if key == gdk::Key::Escape {
                    popover2.popdown();
                    glib::Propagation::Stop
                } else {
                    glib::Propagation::Proceed
                }
            });
            keys.set_propagation_phase(gtk::PropagationPhase::Capture);
            popover.add_controller(keys);
        }
        popover.popup();
        // Land in the note itself: the point of opening it is to write in it.
        let body = body.clone();
        glib::idle_add_local_once(move || {
            body.grab_focus();
        });
    }

    /// Where a note's words sit on screen, for the popover to point at.
    fn anchor_rect(&self, id: &str) -> gdk::Rectangle {
        let text = self.text();
        for (line_no, line) in text.lines().enumerate() {
            let Some(a) = docview::anchors(line).into_iter().find(|a| a.id == id) else {
                continue;
            };
            let Some(mut iter) = self.buffer.iter_at_line(line_no as i32) else { break };
            iter.forward_chars(a.text.0 as i32);
            let place = self.view.iter_location(&iter);
            let (x, y) = self.view.buffer_to_window_coords(
                gtk::TextWindowType::Widget,
                place.x(),
                place.y(),
            );
            return gdk::Rectangle::new(x, y, place.width().max(1), place.height());
        }
        let rect = self.view.visible_rect();
        gdk::Rectangle::new(rect.width() / 2, rect.height() / 3, 1, 1)
    }

    fn save_note(&self, id: &str, kind: NoteKind, text: &str, source: &str) {
        let mut doc = parse::parse(&self.full_text());
        let source = (!source.trim().is_empty() && kind == NoteKind::Quotation)
            .then(|| source.trim().to_string());
        match doc.notes.iter_mut().find(|n| n.id == id) {
            Some(note) => {
                note.kind = kind;
                note.text = text.trim().to_string();
                note.source = source;
            }
            None => doc.notes.push(Note {
                id: id.to_string(),
                kind,
                text: text.trim().to_string(),
                source,
            }),
        }
        self.set_text(&parse::serialize(&doc));
        *self.doc.borrow_mut() = doc;
        self.apply_folds();
        self.dirty_doc.set(true);
    }

    /// Remove a note and unwrap the words it was attached to.
    fn delete_note(&self, id: &str) {
        let mut doc = parse::parse(&self.full_text());
        doc.notes.retain(|n| n.id != id);
        let mut text = parse::serialize(&doc);
        text = unwrap_anchor(&text, id);
        self.set_text(&text);
        *self.doc.borrow_mut() = parse::parse(&text);
        self.apply_folds();
        self.dirty_doc.set(true);
    }

    /// Where a drop lands: near the top or bottom edge of a row it becomes a
    /// sibling above or below, and anywhere in the middle it becomes a child.
    fn drop_section(&self, from: &str, to: &str, zone: f64) {
        let (Some(from), Some(to)) = (parse_path_key(from), parse_path_key(to)) else { return };
        if from == to {
            return;
        }
        let (parent, index) = if zone < 0.25 {
            (to[..to.len() - 1].to_vec(), *to.last().unwrap_or(&0))
        } else if zone > 0.75 {
            (to[..to.len() - 1].to_vec(), to.last().unwrap_or(&0) + 1)
        } else {
            // Clamped by the command to "last child".
            (to.clone(), usize::MAX)
        };
        self.run(Cmd::MoveTo { parent, index }, Some(from));
    }

    /// Break the current section in two at the cursor.
    ///
    /// Done by inserting a heading line into the text rather than by rebuilding
    /// the document from the model: the cursor is a position in the buffer, and
    /// mapping it back onto an offset within a parsed body is both fiddly and
    /// easy to get subtly wrong.
    fn split_at_cursor(&self) {
        let text = self.text();
        let lines = docview::classify(&text);
        let insert = self.buffer.iter_at_mark(&self.buffer.get_insert());
        let line = (insert.line() as usize).min(lines.len().saturating_sub(1));
        let Some(depth) = lines[..=line].iter().rev().find_map(|l| match l {
            docview::Line::Heading { depth, .. } => Some(*depth),
            docview::Line::Body { depth, .. } => Some(*depth),
            // Splitting has no meaning inside the notes at the foot of the file.
            docview::Line::Blank | docview::Line::NoteDef | docview::Line::Front => None,
        }) else {
            return;
        };

        // Splitting at the very top needs no blank lines above the new heading.
        let lead = if insert.offset() == 0 { "" } else { "\n\n" };
        let snippet = format!("{lead}{} \n\n", "#".repeat((depth + 1).min(6)));
        self.loading.set(true);
        let mut at = insert;
        self.buffer.insert(&mut at, &snippet);
        self.loading.set(false);

        // Land on the new heading, ready to be named. Measured from where the
        // insertion ended rather than from the line the cursor started on:
        // `str::lines()` drops a trailing empty line, so that number gets
        // clamped and is not reliable here.
        if let Some(mut h) = self.buffer.iter_at_line(at.line() - 2) {
            if !h.ends_line() {
                h.forward_to_line_end();
            }
            self.buffer.place_cursor(&h);
        }
        self.dirty_doc.set(true);
        self.dirty_style.set(true);
    }

    fn bind_row(self: &Rc<Self>, item: &glib::Object) {
        let Some(item) = item.downcast_ref::<gtk::ListItem>() else { return };
        let Some(row) = item.item().and_downcast::<gtk::TreeListRow>() else { return };
        let Some(obj) = row.item().and_downcast::<NodeObject>() else { return };
        let Some(expander) = item.child().and_downcast::<gtk::TreeExpander>() else { return };
        let Some(label) = expander.child().and_downcast::<gtk::Label>() else { return };

        expander.set_list_row(Some(&row));
        label.set_text(&obj.display_title());
        unsafe { expander.set_data("oma-path", path_key(&obj.path())) };
        let binding = obj
            .bind_property("title", &label, "label")
            .transform_to(|_, t: String| {
                Some(if t.trim().is_empty() { "Untitled".to_string() } else { t })
            })
            .build();
        unsafe { item.set_data("oma-title-binding", binding) };

        unsafe {
            if row.data::<bool>("oma-watched").is_none() {
                row.set_data("oma-watched", true);
                let me = self.clone();
                row.connect_expanded_notify(move |r| me.on_row_expanded(r));
            }
        }
    }

    /// A row toggling marks state dirty and re-folds the document.
    ///
    /// The work is deferred to idle, and that is not optional: this runs from
    /// inside `gtk_tree_list_row_set_expanded`, and `capture_collapsed` reads
    /// rows back out of the same model. Re-entering a GtkTreeListModel while it
    /// is still mutating segfaults in GTK, which is exactly what it did.
    fn on_row_expanded(self: &Rc<Self>, row: &gtk::TreeListRow) {
        if self.loading.get() {
            return;
        }
        // Only the row's own properties are touched here. Looking rows up by
        // position in the model while it is still mutating segfaults GTK, so
        // everything else waits for idle.
        let Some(obj) = row.item().and_downcast::<NodeObject>() else { return };
        self.pending_toggles.borrow_mut().push((obj.path(), row.is_expanded()));
        self.dirty_state.set(true);
        if self.fold_pending.replace(true) {
            return;
        }
        let me = self.clone();
        glib::idle_add_local_once(move || me.settle_toggles());
    }

    /// Resolve a batch of expansion changes once the model has settled.
    fn settle_toggles(&self) {
        self.fold_pending.set(false);
        let batch = std::mem::take(&mut *self.pending_toggles.borrow_mut());
        let closed: Vec<NodePath> = batch
            .iter()
            .filter(|(_, open)| !*open)
            .map(|(p, _)| p.clone())
            .collect();
        {
            let mut st = self.dstate.borrow_mut();
            for (path, open) in &batch {
                // Collapsing a row destroys its descendants, and they report
                // themselves closed on the way out. That is GTK tearing them
                // down, not the user closing them, so ignore anything beneath a
                // row closed in this same batch.
                if closed.iter().any(|c| c.len() < path.len() && path.starts_with(c)) {
                    continue;
                }
                let key = path_key(path);
                if *open {
                    st.collapsed.remove(&key);
                } else {
                    st.collapsed.insert(key);
                }
            }
        }
        // Expanding a row builds its children fresh, and GtkTreeListModel makes
        // them unexpanded however they were left. Put them back.
        self.loading.set(true);
        self.apply_expansion();
        self.loading.set(false);
        self.apply_folds();
    }

    // ---- document rendering ------------------------------------------------

    /// The line the cursor is on; emphasis markers stay visible there.
    /// Step back. Folding is excluded from the history, so undo moves through
    /// edits rather than through the view being tidied around them.
    fn undo(&self) {
        if !self.buffer.can_undo() {
            return;
        }
        self.buffer.undo();
        self.after_history_change();
    }

    fn redo(&self) {
        if !self.buffer.can_redo() {
            return;
        }
        self.buffer.redo();
        self.after_history_change();
    }

    fn after_history_change(&self) {
        self.dirty_doc.set(true);
        self.dirty_style.set(true);
        self.dirty_cursor.set(true);
        self.refresh_structure();
    }

    fn cursor_line(&self) -> i32 {
        self.buffer.iter_at_mark(&self.buffer.get_insert()).line()
    }

    /// Lines on screen, plus enough either side to cover a small scroll.
    fn visible_lines(&self) -> (i32, i32) {
        let rect = self.view.visible_rect();
        let (top, _) = self.view.line_at_y(rect.y());
        let (bottom, _) = self.view.line_at_y(rect.y() + rect.height());
        const OVERSCAN: i32 = 60;
        (top.line() - OVERSCAN, bottom.line() + OVERSCAN)
    }

    fn restyle(&self) {
        self.loading.set(true);
        let (first, last) = self.visible_lines();
        docview::restyle_range(
            &self.buffer,
            first,
            last,
            self.cursor_line(),
            self.speller.as_ref(),
        );
        self.loading.set(false);
        self.apply_folds();
    }

    /// Restore every folded section, leaving the buffer holding the whole file.
    fn unfold_all(&self) {
        if self.folds.borrow().is_empty() {
            return;
        }
        let mut folds = std::mem::take(&mut *self.folds.borrow_mut());
        // Descending offset, so each insertion leaves earlier offsets valid.
        folds.sort_by_key(|f| std::cmp::Reverse(self.buffer.iter_at_mark(&f.mark).offset()));
        self.loading.set(true);
        self.buffer.begin_irreversible_action();
        for f in folds {
            let mut at = self.buffer.iter_at_mark(&f.mark);
            self.buffer.insert(&mut at, &f.text);
            self.buffer.delete_mark(&f.mark);
        }
        self.buffer.end_irreversible_action();
        self.loading.set(false);
    }

    /// Make the buffer match what is collapsed: unfold everything, then lift out
    /// each collapsed section's contents. Folding wholesale rather than
    /// incrementally keeps nesting trivial -- a collapsed section inside another
    /// collapsed section is simply never folded separately.
    fn apply_folds(&self) {
        self.unfold_all();

        // Anything with content under it may fold: the gutter triangle beside
        // the heading is always there to reopen it, which is what makes this
        // safe now where it was not before.
        let doc = self.doc.borrow();
        let collapsed: Vec<NodePath> = self
            .dstate
            .borrow()
            .collapsed
            .iter()
            .filter_map(|k| parse_path_key(k))
            .filter(|p| doc.get(p).map(Self::foldable).unwrap_or(false))
            .collect();
        drop(doc);
        let outermost: Vec<&NodePath> = collapsed
            .iter()
            .filter(|p| !collapsed.iter().any(|q| q.len() < p.len() && p.starts_with(q)))
            .collect();

        let text = self.text();
        // Parse once for the whole pass: buffer_line_for parses on every call,
        // which turned folding into one full parse per folded section.
        let walk_order = parse::parse(&text);
        let walk = walk_order.walk();
        let heads = docview::heading_lines(&text);
        let mut ranges: Vec<(usize, usize)> = outermost
            .iter()
            .filter_map(|p| {
                let idx = walk.iter().position(|(q, _)| q == *p)?;
                heads.get(idx).map(|&l| docview::section_range(&text, l))
            })
            .collect();
        // Descending, so each deletion leaves earlier line numbers valid.
        ranges.sort_by_key(|&(s, _)| std::cmp::Reverse(s));

        if ranges.is_empty() {
            // begin_irreversible_action clears the whole undo history, and this
            // runs on every restyle. Calling it when there is nothing to fold
            // wiped undo several times a second.
            docview::restyle_range(
                &self.buffer,
                0,
                i32::MAX,
                self.cursor_line(),
                self.speller.as_ref(),
            );
            self.queue_gutter();
            return;
        }
        self.loading.set(true);
        self.buffer.begin_irreversible_action();
        for (start, end) in ranges {
            // Whole lines only, from the one after the heading. Starting at the
            // end of the heading line would swallow its newline and weld the
            // next heading onto it.
            let Some(mut from) = self.buffer.iter_at_line(start as i32 + 1) else { continue };
            let mut to = match self.buffer.iter_at_line(end as i32) {
                Some(i) => i,
                None => self.buffer.end_iter(),
            };
            if from >= to {
                continue;
            }
            let lifted = self.buffer.text(&from, &to, true).to_string();
            let mark = self.buffer.create_mark(None, &from, true);
            self.buffer.delete(&mut from, &mut to);
            self.folds.borrow_mut().push(Fold { mark, text: lifted });
        }
        self.buffer.end_irreversible_action();
        self.loading.set(false);
        let (first, last) = self.visible_lines();
        docview::restyle_range(
            &self.buffer,
            first,
            last,
            self.cursor_line(),
            self.speller.as_ref(),
        );
        self.queue_gutter();
    }

    /// Drop fold state that no longer names a foldable section.
    ///
    /// Keys outlast the structure they described: edit a document until a
    /// section loses its children and its stored "collapsed" would hide its
    /// text for good, since there is no longer a triangle to reopen it.
    fn prune_collapsed(&self) {
        let doc = self.doc.borrow();
        let before = self.dstate.borrow().collapsed.len();
        self.dstate.borrow_mut().collapsed.retain(|k| {
            parse_path_key(k)
                .and_then(|p| doc.get(&p).map(Self::foldable))
                .unwrap_or(false)
        });
        if self.dstate.borrow().collapsed.len() != before {
            self.dirty_state.set(true);
        }
    }

    /// The whole file, with folded sections spliced back in. Everything that
    /// reads the document -- saving, re-parsing -- goes through this, never the
    /// buffer directly, or folded text would be lost.
    fn full_text(&self) -> String {
        let mut text = self.text();
        let folds = self.folds.borrow();
        if folds.is_empty() {
            return text;
        }
        let mut parked: Vec<(usize, &str)> = folds
            .iter()
            .map(|f| (self.buffer.iter_at_mark(&f.mark).offset() as usize, f.text.as_str()))
            .collect();
        parked.sort_by_key(|&(o, _)| std::cmp::Reverse(o));
        for (chars, chunk) in parked {
            let byte = text
                .char_indices()
                .nth(chars)
                .map(|(b, _)| b)
                .unwrap_or(text.len());
            text.insert_str(byte, chunk);
        }
        text
    }

    /// Which buffer line a section's heading is on right now. Parsed from the
    /// buffer rather than the full text because folding removes a section's
    /// contents but keeps its heading, so every still-visible node keeps the
    /// same path in both.
    fn buffer_line_for(&self, path: &[usize]) -> Option<usize> {
        let text = self.text();
        let idx = parse::parse(&text).walk().iter().position(|(p, _)| p == path)?;
        docview::heading_lines(&text).get(idx).copied()
    }

    fn path_at_index(&self, index: usize) -> Option<NodePath> {
        self.doc.borrow().walk().get(index).map(|(p, _)| p.clone())
    }

    // ---- structure ---------------------------------------------------------

    /// Re-parse the buffer and rebuild the outline. Expansion is restored from
    /// stored state, so folding survives editing.
    fn refresh_structure(&self) {
        let text = self.full_text();
        *self.doc.borrow_mut() = parse::parse(&text);
        self.prune_collapsed();
        self.rebuild_tree();
        self.apply_folds();
    }

    fn rebuild_tree(&self) {
        self.loading.set(true);
        self.root_store.remove_all();
        {
            let d = self.doc.borrow();
            for (i, n) in d.roots.iter().enumerate() {
                self.root_store
                    .append(&NodeObject::new(vec![i], &n.title, !n.children.is_empty()));
            }
        }
        self.apply_expansion();
        self.loading.set(false);
    }

    fn apply_expansion(&self) {
        let collapsed = self.dstate.borrow().collapsed.clone();
        let mut i = 0;
        while i < self.tree.n_items() {
            if let Some(row) = self.tree.row(i) {
                if row.is_expandable() {
                    if let Some(obj) = row.item().and_downcast::<NodeObject>() {
                        let want = !collapsed.contains(&path_key(&obj.path()));
                        if row.is_expanded() != want {
                            row.set_expanded(want);
                        }
                    }
                }
            }
            i += 1;
        }
    }

    fn find_row(&self, path: &[usize]) -> Option<u32> {
        (0..self.tree.n_items()).find(|&i| {
            self.tree
                .row(i)
                .and_then(|r| r.item())
                .and_downcast::<NodeObject>()
                .map(|o| o.path() == path)
                .unwrap_or(false)
        })
    }

    // ---- navigation --------------------------------------------------------

    fn on_outline_selected(&self) {
        if self.loading.get() {
            return;
        }
        let Some(row) = self.selection.selected_item().and_downcast::<gtk::TreeListRow>() else {
            return;
        };
        let Some(obj) = row.item().and_downcast::<NodeObject>() else { return };
        let path = obj.path();
        *self.selected.borrow_mut() = Some(path.clone());
        self.dstate.borrow_mut().selected = Some(path_key(&path));
        self.dirty_state.set(true);
        self.scroll_to(&path);
    }

    /// Bring a section's heading to the top of the view without moving focus.
    fn scroll_to(&self, path: &[usize]) {
        let Some(line) = self.buffer_line_for(path) else { return };
        let Some(iter) = self.buffer.iter_at_line(line as i32) else { return };
        self.view.scroll_to_iter(&mut iter.clone(), 0.0, true, 0.0, 0.08);
    }

    /// Follow the cursor: highlight whichever section it is sitting in.
    fn sync_outline_to_cursor(&self) {
        let text = self.text();
        let cursor_line = self
            .buffer
            .iter_at_mark(&self.buffer.get_insert())
            .line() as usize;
        let lines = docview::heading_lines(&text);
        let idx = match lines.iter().rposition(|&l| l <= cursor_line) {
            Some(i) => i,
            None => return,
        };
        let Some(path) = self.path_at_index(idx) else { return };
        if self.selected.borrow().as_deref() == Some(path.as_slice()) {
            return;
        }
        *self.selected.borrow_mut() = Some(path.clone());
        if let Some(row) = self.find_row(&path) {
            self.loading.set(true);
            self.selection.set_selected(row);
            self.loading.set(false);
        }
    }

    /// Put the text cursor at the end of a section's heading and focus it.
    fn put_cursor_at(&self, path: &[usize]) {
        let Some(line) = self.buffer_line_for(path) else { return };
        let Some(mut iter) = self.buffer.iter_at_line(line as i32) else { return };
        if !iter.ends_line() {
            iter.forward_to_line_end();
        }
        self.buffer.place_cursor(&iter);
        self.view.scroll_to_iter(&mut iter.clone(), 0.0, true, 0.0, 0.3);
        self.focus_document();
    }

    fn focus_document(&self) {
        self.view.grab_focus();
    }

    fn focus_outline(&self) {
        self.list.grab_focus();
    }

    fn selected_row(&self) -> Option<gtk::TreeListRow> {
        let i = self.selection.selected();
        if i == gtk::INVALID_LIST_POSITION {
            return None;
        }
        self.tree.row(i)
    }

    fn set_expanded(&self, expand: bool) -> bool {
        match self.selected_row() {
            Some(row) if row.is_expandable() && row.is_expanded() != expand => {
                row.set_expanded(expand);
                true
            }
            _ => false,
        }
    }

    fn collapse_or_parent(&self) {
        if self.set_expanded(false) {
            return;
        }
        // A section with text but no subsections has no expander in the outline,
        // yet it still has something to fold.
        let here = self.selected.borrow().clone();
        if let Some(p) = here.as_ref() {
            let key = path_key(p);
            if self.is_foldable(p) && !self.dstate.borrow().collapsed.contains(&key) {
                self.toggle_fold(p);
                return;
            }
        }
        if let Some(p) = here.filter(|p| p.len() > 1) {
            self.select_path(&p[..p.len() - 1]);
        }
    }

    fn expand_or_child(&self) {
        if self.set_expanded(true) {
            return;
        }
        let here = self.selected.borrow().clone();
        if let Some(p) = here.as_ref() {
            let key = path_key(p);
            if self.dstate.borrow().collapsed.contains(&key) {
                self.toggle_fold(p);
                return;
            }
        }
        let child = self.selected.borrow().clone().and_then(|p| {
            let has = self.doc.borrow().get(&p).map(|n| !n.children.is_empty()).unwrap_or(false);
            has.then(|| {
                let mut c = p.clone();
                c.push(0);
                c
            })
        });
        if let Some(c) = child {
            self.select_path(&c);
        }
    }

    fn select_path(&self, path: &[usize]) {
        self.loading.set(true);
        for depth in 1..path.len() {
            if let Some(row) = self.find_row(&path[..depth]).and_then(|i| self.tree.row(i)) {
                if row.is_expandable() && !row.is_expanded() {
                    row.set_expanded(true);
                }
            }
        }
        self.loading.set(false);
        if let Some(i) = self.find_row(path) {
            self.selection.set_selected(i);
            *self.selected.borrow_mut() = Some(path.to_vec());
            self.list.scroll_to(i, gtk::ListScrollFlags::NONE, None);
            self.dstate.borrow_mut().selected = Some(path_key(path));
            self.dirty_state.set(true);
            self.list.grab_focus();
        }
    }

    // ---- editing -----------------------------------------------------------

    /// Structural commands work on the parsed document and re-render the buffer.
    /// Plain typing never goes through here, so the cost is paid only on the
    /// rare restructuring keystroke.
    /// Run a structural command without leaving the document. `run` hands focus
    /// to the outline, which is right when the command came from there and
    /// wrong when it came from the text.
    fn apply_cmd_in_document(self: &Rc<Self>, cmd: Cmd) {
        self.apply_cmd(cmd);
        let landed = self.selected.borrow().clone();
        if let Some(p) = landed {
            self.put_cursor_at(&p);
        }
    }

    fn apply_cmd(self: &Rc<Self>, cmd: Cmd) {
        let at = self.selected.borrow().clone();
        if cmd.is_destructive() && self.needs_confirmation(at.as_deref()) {
            self.confirm_delete(at);
            return;
        }
        self.run(cmd, at);
    }

    fn run(&self, cmd: Cmd, at: Option<NodePath>) {
        // Restructuring rewrites the whole buffer from the full document.
        self.unfold_all();
        let mut doc = parse::parse(&self.text());
        let Some(out) = edit::apply(&mut doc, at.as_deref(), cmd) else { return };
        if let Some(node) = out.lifted {
            *self.clipboard.borrow_mut() = Some(node);
        }

        let select = match out.select {
            Some(p) => p,
            None => doc.push_root(crate::model::Node::new("")),
        };
        self.set_text(&parse::serialize(&doc));
        *self.doc.borrow_mut() = doc;
        self.rebuild_tree();
        self.apply_folds();
        self.dirty_doc.set(true);

        if out.focus_title {
            // A fresh section: land the cursor where its name goes.
            self.select_path(&select);
            self.put_cursor_at(&select);
        } else {
            self.select_path(&select);
            self.scroll_to(&select);
        }
    }

    fn set_text(&self, text: &str) {
        self.loading.set(true);
        // Replacing the buffer invalidates every parked fold. Their marks
        // collapse to the start, and `text` already contains everything, so
        // keeping them would splice each folded section in a second time at the
        // top of the file -- which is exactly what corrupted a document.
        self.folds.borrow_mut().clear();
        self.buffer.set_text(text);
        docview::restyle(&self.buffer);
        self.loading.set(false);
    }

    fn needs_confirmation(&self, at: Option<&[usize]>) -> bool {
        let Some(p) = at else { return false };
        let d = self.doc.borrow();
        d.get(p)
            .map(|n| !n.children.is_empty() || !n.body.trim().is_empty())
            .unwrap_or(false)
    }

    fn confirm_delete(self: &Rc<Self>, at: Option<NodePath>) {
        let Some(p) = at.clone() else { return };
        let (title, kids, has_body) = {
            let d = self.doc.borrow();
            match d.get(&p) {
                Some(n) => (
                    n.title.trim().to_string(),
                    count_descendants(n),
                    !n.body.trim().is_empty(),
                ),
                None => return,
            }
        };
        let heading = if title.is_empty() {
            "Delete this untitled section?".to_string()
        } else {
            format!("Delete \u{201c}{title}\u{201d}?")
        };
        let mut detail = String::new();
        if kids > 0 {
            detail.push_str(&format!(
                "{kids} section{} beneath it will go too",
                if kids == 1 { "" } else { "s" }
            ));
        }
        if has_body {
            if !detail.is_empty() {
                detail.push_str(", and ");
            }
            detail.push_str("its text will be deleted");
        }
        if detail.is_empty() {
            detail.push_str("This cannot be undone");
        }
        detail.push('.');

        let dlg = adw::AlertDialog::new(Some(&heading), Some(&detail));
        dlg.add_response("cancel", "Cancel");
        dlg.add_response("delete", "Delete");
        dlg.set_response_appearance("delete", adw::ResponseAppearance::Destructive);
        dlg.set_default_response(Some("cancel"));
        dlg.set_close_response("cancel");
        let me = self.clone();
        dlg.connect_response(None, move |_, resp| {
            if resp == "delete" {
                me.run(Cmd::Delete, at.clone());
            }
        });
        dlg.present(Some(&self.window));
    }

    // ---- documents ---------------------------------------------------------

    pub fn open(&self, path: &PathBuf) {
        if self.path.borrow().is_some() {
            self.save_doc();
            self.save_state();
            self.dirty_doc.set(false);
            self.dirty_state.set(false);
        }

        let src = std::fs::read_to_string(path).unwrap_or_default();
        *self.path.borrow_mut() = Some(path.clone());
        *self.dstate.borrow_mut() = state::load_doc_state(path);
        *self.selected.borrow_mut() = None;
        self.set_text(&src);
        *self.doc.borrow_mut() = parse::parse(&src);
        self.prune_collapsed();

        let title = self.doc.borrow().title.clone().unwrap_or_else(|| library::title_of(path));
        self.wtitle.set_title(&title);
        self.wtitle.set_subtitle(&self.pretty(path));
        self.wtitle.remove_css_class("oma-error");
        self.window.set_title(Some(&format!("{title} — Omaverse")));
        self.stack.set_visible_child_name("doc");

        self.rebuild_tree();
        self.apply_folds();
        self.queue_gutter();

        self.record_stamp(path);
        self.conflict.set(false);
        self.conflict_asked.set(false);
        state::push_recent(&mut self.recent.borrow_mut(), path);
        self.save_window_state();

        let remembered = self
            .dstate
            .borrow()
            .selected
            .clone()
            .and_then(|k| parse_path_key(&k))
            .filter(|p| self.doc.borrow().get(p).is_some());
        match remembered {
            Some(p) => {
                self.select_path(&p);
                self.scroll_to(&p);
            }
            None if !self.doc.borrow().is_empty() => self.select_path(&[0]),
            None => self.focus_document(),
        }
    }

    fn md_filters() -> (gio::ListStore, gtk::FileFilter) {
        let md = gtk::FileFilter::new();
        md.set_name(Some("Markdown"));
        md.add_suffix("md");
        let all = gtk::FileFilter::new();
        all.set_name(Some("All files"));
        all.add_pattern("*");
        let store = gio::ListStore::new::<gtk::FileFilter>();
        store.append(&md);
        store.append(&all);
        (store, md)
    }

    fn chooser_start_dir(&self) -> gio::File {
        let dir = &self.cfg.outline_dir;
        if !dir.exists() {
            let _ = std::fs::create_dir_all(dir);
        }
        let start = if dir.exists() {
            dir.clone()
        } else {
            self.path
                .borrow()
                .clone()
                .and_then(|p| p.parent().map(|q| q.to_path_buf()))
                .unwrap_or_else(state::home)
        };
        gio::File::for_path(start)
    }

    fn new_outline(self: &Rc<Self>) {
        let (filters, default) = Self::md_filters();
        let dialog = gtk::FileDialog::builder()
            .title("New outline")
            .accept_label("Create")
            .initial_folder(&self.chooser_start_dir())
            .initial_name("Untitled.md")
            .filters(&filters)
            .default_filter(&default)
            .modal(true)
            .build();
        let me = self.clone();
        dialog.save(Some(&self.window), gio::Cancellable::NONE, move |res| {
            if let Some(path) = res.ok().and_then(|f| f.path()) {
                me.create_outline_at(&path);
            }
        });
    }

    fn open_dialog(self: &Rc<Self>) {
        let (filters, default) = Self::md_filters();
        let dialog = gtk::FileDialog::builder()
            .title("Open outline")
            .initial_folder(&self.chooser_start_dir())
            .filters(&filters)
            .default_filter(&default)
            .modal(true)
            .build();
        let me = self.clone();
        dialog.open(Some(&self.window), gio::Cancellable::NONE, move |res| {
            if let Some(path) = res.ok().and_then(|f| f.path()) {
                me.refresh_library();
                me.open(&path);
            }
        });
    }

    fn create_outline_at(&self, chosen: &Path) {
        // Append rather than replace the extension: `with_extension` would turn
        // "1 Cor 1.1" into "1 Cor 1.md".
        let text = chosen.to_string_lossy();
        let path = if text.ends_with(".md") {
            chosen.to_path_buf()
        } else {
            PathBuf::from(format!("{text}.md"))
        };
        if std::fs::metadata(&path).map(|m| m.len() > 0).unwrap_or(false) {
            self.refresh_library();
            self.open(&path);
            self.wtitle
                .set_subtitle(&format!("{} — opened the existing outline", self.pretty(&path)));
            return;
        }
        let name = path
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "Untitled".to_string());
        let mut d = Document::default();
        d.title = Some(name);
        if let Err(e) = crate::atomic_write(&path, parse::serialize(&d).as_bytes()) {
            eprintln!("omaverse: could not create {}: {e}", path.display());
            self.wtitle.set_subtitle(&format!("Could not create outline — {e}"));
            self.wtitle.add_css_class("oma-error");
            return;
        }
        self.refresh_library();
        self.open(&path);
        if let Some(p) = self.path_at_index(0) {
            self.put_cursor_at(&p);
        }
    }

    fn pretty(&self, path: &Path) -> String {
        match path.strip_prefix(state::home()) {
            Ok(rest) => format!("~/{}", rest.display()),
            Err(_) => path.display().to_string(),
        }
    }

    // ---- library -----------------------------------------------------------

    fn refresh_library(&self) {
        while let Some(child) = self.libbox.first_child() {
            self.libbox.remove(&child);
        }
        let mut paths: Vec<Option<PathBuf>> = Vec::new();
        let groups = library::scan(&self.cfg.outline_dir);

        if groups.is_empty() {
            paths.push(None);
            self.libbox.append(&plain_row(
                &format!("No outlines in\n{}", self.pretty(&self.cfg.outline_dir)),
                &["dim-label"],
                true,
            ));
        }
        for g in groups {
            if let Some(name) = &g.name {
                paths.push(None);
                self.libbox.append(&plain_row(&name.to_uppercase(), &["oma-group"], false));
            }
            for e in &g.entries {
                paths.push(Some(e.path.clone()));
                self.libbox.append(&entry_row(&e.title));
            }
        }
        let outside: Vec<PathBuf> = self
            .recent
            .borrow()
            .iter()
            .filter(|p| !p.starts_with(&self.cfg.outline_dir) && p.exists())
            .cloned()
            .collect();
        if !outside.is_empty() {
            paths.push(None);
            self.libbox.append(&plain_row("RECENT", &["oma-group"], false));
            for p in outside {
                let row = entry_row(&library::title_of(&p));
                row.set_tooltip_text(Some(&self.pretty(&p)));
                paths.push(Some(p));
                self.libbox.append(&row);
            }
        }
        *self.lib_paths.borrow_mut() = paths;
    }

    fn on_library_activated(self: &Rc<Self>, row: &gtk::ListBoxRow) {
        let idx = row.index();
        if idx < 0 {
            return;
        }
        let Some(path) = self.lib_paths.borrow().get(idx as usize).cloned().flatten() else {
            return;
        };
        // Opening tears down and rebuilds both panes; the same reasoning as the
        // drop handler applies, so it waits until this signal has returned.
        let me = self.clone();
        glib::idle_add_local_once(move || me.open(&path));
    }

    // ---- watching the file ------------------------------------------------

    /// Notice a write by anything other than omaverse.
    fn check_disk(self: &Rc<Self>) {
        if self.conflict.get() {
            // save_doc already refused to write. Put the question to the user,
            // once.
            if !self.conflict_asked.replace(true) {
                if let Some(path) = self.path.borrow().clone() {
                    self.show_conflict(&path);
                }
            }
            return;
        }
        let Some(path) = self.path.borrow().clone() else { return };
        // A missing file is left alone: Dropbox and editors both replace files
        // by rename, so a brief gap is normal and not something to react to.
        let Some(now) = stamp_of(&path) else { return };
        let known = *self.disk_stamp.borrow();
        match known {
            None => *self.disk_stamp.borrow_mut() = Some(now),
            Some(known) if known == now => {}
            Some(_) => {
                if self.dirty_doc.get() {
                    // Unsaved edits here and a new version there.
                    self.conflict.set(true);
                    self.conflict_asked.set(true);
                    self.show_conflict(&path);
                } else {
                    self.reload_from_disk(&path);
                }
            }
        }
    }

    fn record_stamp(&self, path: &Path) {
        *self.disk_stamp.borrow_mut() = stamp_of(path);
    }

    fn reload_from_disk(&self, path: &Path) {
        let Ok(src) = std::fs::read_to_string(path) else { return };
        let offset = self.buffer.iter_at_mark(&self.buffer.get_insert()).offset();
        // Marks into the old content are about to be invalid.
        self.folds.borrow_mut().clear();
        self.set_text(&src);
        *self.doc.borrow_mut() = parse::parse(&src);
        self.prune_collapsed();
        self.rebuild_tree();
        self.apply_folds();
        let mut at = self.buffer.iter_at_offset(offset.min(self.buffer.char_count()));
        self.buffer.place_cursor(&at);
        self.view.scroll_to_iter(&mut at, 0.0, true, 0.0, 0.3);
        self.record_stamp(path);
        self.wtitle
            .set_subtitle(&format!("{} — reloaded, it changed on disk", self.pretty(path)));
    }

    fn show_conflict(self: &Rc<Self>, path: &Path) {
        let name = path
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "This outline".into());
        let dlg = adw::AlertDialog::new(
            Some(&format!("{name} changed on disk")),
            Some(
                "Something else wrote this file while you were editing it, most likely \
Dropbox syncing a change from another machine. Whichever version you set aside is \
kept, so neither is lost.",
            ),
        );
        dlg.add_response("theirs", "Load theirs");
        dlg.add_response("mine", "Keep mine");
        dlg.set_response_appearance("mine", adw::ResponseAppearance::Suggested);
        dlg.set_default_response(Some("mine"));
        dlg.set_close_response("mine");
        let me = self.clone();
        let path = path.to_path_buf();
        dlg.connect_response(None, move |_, resp| me.resolve_conflict(&path, resp));
        dlg.present(Some(&self.window));
    }

    fn resolve_conflict(&self, path: &Path, choice: &str) {
        self.conflict_asked.set(false);
        self.wtitle.remove_css_class("oma-error");
        if choice == "theirs" {
            let kept = state::conflict_backup(path, "mine");
            let _ = crate::atomic_write(&kept, self.full_text().as_bytes());
            self.conflict.set(false);
            self.dirty_doc.set(false);
            self.reload_from_disk(path);
            self.wtitle
                .set_subtitle(&format!("Loaded from disk — yours kept at {}", self.pretty(&kept)));
        } else {
            if let Ok(theirs) = std::fs::read(path) {
                let kept = state::conflict_backup(path, "theirs");
                let _ = crate::atomic_write(&kept, &theirs);
                self.conflict.set(false);
                self.save_doc();
                self.dirty_doc.set(false);
                self.wtitle
                    .set_subtitle(&format!("Kept yours — theirs at {}", self.pretty(&kept)));
                return;
            }
            self.conflict.set(false);
            self.save_doc();
            self.dirty_doc.set(false);
        }
    }

    // ---- saving ------------------------------------------------------------

    /// Save what is in the buffer, verbatim. Typing is never reformatted
    /// underneath the cursor; only structural commands rewrite the text.
    fn save_doc(&self) {
        let Some(path) = self.path.borrow().clone() else { return };
        // Refuse to write over something written by anything else. This has to
        // live here rather than in the watch timer: autosave runs more often
        // than any sensible watch interval, so a timer always loses the race.
        if let (Some(now), Some(known)) = (stamp_of(&path), *self.disk_stamp.borrow()) {
            if now != known {
                self.conflict.set(true);
                self.wtitle
                    .set_subtitle(&format!("{} — changed on disk, not saved", self.pretty(&path)));
                self.wtitle.add_css_class("oma-error");
                return;
            }
        }
        let mut text = self.full_text();
        if !text.ends_with('\n') && !text.is_empty() {
            text.push('\n');
        }
        match crate::atomic_write(&path, text.as_bytes()) {
            Ok(()) => {
                self.record_stamp(&path);
                self.wtitle.set_subtitle(&self.pretty(&path));
                self.wtitle.remove_css_class("oma-error");
            }
            Err(e) => {
                eprintln!("omaverse: could not save {}: {e}", path.display());
                self.wtitle.set_subtitle(&format!("Not saved — {e}"));
                self.wtitle.add_css_class("oma-error");
            }
        }
    }

    fn save_state(&self) {
        if let Some(path) = self.path.borrow().clone() {
            state::save_doc_state(&path, &self.dstate.borrow());
        }
    }

    fn save_window_state(&self) {
        let (w, h) = self.window.default_size();
        state::save_window_state(&state::WindowState {
            width: Some(w),
            height: Some(h),
            paned: Some(self.paned.position()),
            sidebar_open: Some(self.split.shows_sidebar()),
            last_file: self.path.borrow().clone(),
            recent: self.recent.borrow().clone(),
        });
    }
}

/// Modification time in nanoseconds plus length: enough to catch a rewrite that
/// happens within the same second, which Dropbox routinely does.
fn stamp_of(path: &Path) -> Option<(u64, u64)> {
    let meta = std::fs::metadata(path).ok()?;
    let nanos = meta
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_nanos() as u64;
    Some((nanos, meta.len()))
}

/// Replace `==words==[^id]` with just the words, leaving the text readable.
fn unwrap_anchor(text: &str, id: &str) -> String {
    text.lines()
        .map(|line| {
            match docview::anchors(line).into_iter().find(|a| a.id == id) {
                None => line.to_string(),
                Some(a) => {
                    let chars: Vec<char> = line.chars().collect();
                    let before: String = chars[..a.span.0].iter().collect();
                    let words: String = chars[a.text.0..a.text.1].iter().collect();
                    let after: String = chars[a.span.1..].iter().collect();
                    format!("{before}{words}{after}")
                }
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n"
}

/// A disclosure triangle: pointing down when open, right when closed.
fn triangle(cr: &gtk4::cairo::Context, x: f64, y: f64, open: bool) {
    let size = 4.0;
    if open {
        cr.move_to(x - size, y - size / 2.0);
        cr.line_to(x + size, y - size / 2.0);
        cr.line_to(x, y + size * 0.9);
    } else {
        cr.move_to(x - size / 2.0, y - size);
        cr.line_to(x + size * 0.9, y);
        cr.line_to(x - size / 2.0, y + size);
    }
    cr.close_path();
}

fn entry_row(title: &str) -> gtk::ListBoxRow {
    let label = gtk::Label::builder()
        .label(title)
        .xalign(0.0)
        .ellipsize(pango::EllipsizeMode::End)
        .margin_start(12)
        .margin_end(12)
        .margin_top(6)
        .margin_bottom(6)
        .build();
    let row = gtk::ListBoxRow::new();
    row.set_child(Some(&label));
    row
}

fn plain_row(text: &str, classes: &[&str], wrap: bool) -> gtk::ListBoxRow {
    let label = gtk::Label::builder()
        .label(text)
        .xalign(0.0)
        .wrap(wrap)
        .margin_start(12)
        .margin_end(12)
        .margin_top(10)
        .margin_bottom(2)
        .build();
    for c in classes {
        label.add_css_class(c);
    }
    let row = gtk::ListBoxRow::new();
    row.set_child(Some(&label));
    row.set_selectable(false);
    row.set_activatable(false);
    row
}

fn count_descendants(n: &crate::model::Node) -> usize {
    n.children.len() + n.children.iter().map(count_descendants).sum::<usize>()
}
