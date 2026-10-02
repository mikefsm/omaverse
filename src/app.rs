//! UI: library sidebar | outline tree | title + body editor.
//!
//! The `Document` is the single source of truth. All structural editing goes
//! through `edit::apply`, which is pure and unit-tested; the handlers here only
//! translate a keystroke into a `Cmd` and apply the resulting `Outcome`.

use crate::config::{self, Config};
use crate::edit::{self, Cmd};
use crate::library;
use crate::model::{parse_path_key, path_key, Document, Node, NodePath};
use crate::nodeobj::NodeObject;
use crate::parse;
use crate::state::{self, DocState};

use gtk4 as gtk;
use gtk4::gdk;
use gtk4::gio;
use gtk4::glib;
use gtk4::pango;
use gtk4::prelude::*;
use libadwaita as adw;
use adw::prelude::*;

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

const SAVE_DOC_MS: u64 = 800;
const SAVE_STATE_MS: u64 = 2000;

const CSS: &str = "
.oma-body { font-family: 'Source Serif 4','Noto Serif','DejaVu Serif',serif; font-size: 12.5pt; }
.oma-title { font-family: 'Source Serif 4','Noto Serif','DejaVu Serif',serif; font-size: 17pt; font-weight: 600; }
.oma-title:disabled { opacity: 0.3; }
.oma-outline { font-size: 10.5pt; }
.oma-group { font-size: 9pt; font-weight: bold; opacity: 0.55; }
.oma-error { color: #e01b24; }
";

pub struct App {
    cfg: Config,
    doc: Rc<RefCell<Document>>,
    path: RefCell<Option<PathBuf>>,
    dstate: RefCell<DocState>,
    selected: RefCell<Option<NodePath>>,
    /// Title text typed but not yet pushed into the model.
    pending_title: RefCell<Option<String>>,

    dirty_doc: Cell<bool>,
    dirty_state: Cell<bool>,
    /// Set while the app is driving the widgets, so their change signals don't
    /// get mistaken for the user typing.
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
    title_entry: gtk::Entry,
    body_view: gtk::TextView,
    buffer: gtk::TextBuffer,
    libbox: gtk::ListBox,
    lib_paths: RefCell<Vec<Option<PathBuf>>>,
}

pub fn build(gapp: &adw::Application, cli: Option<PathBuf>) {
    load_css();
    let cfg = config::load();
    let wstate = state::load_window_state();
    let doc: Rc<RefCell<Document>> = Rc::new(RefCell::new(Document::default()));

    // ---- outline tree ------------------------------------------------------
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
    factory.connect_setup(|_, item| {
        let Some(item) = item.downcast_ref::<gtk::ListItem>() else { return };
        let label = gtk::Label::builder()
            .xalign(0.0)
            .ellipsize(pango::EllipsizeMode::End)
            .build();
        let expander = gtk::TreeExpander::new();
        expander.set_child(Some(&label));
        item.set_child(Some(&expander));
    });
    factory.connect_unbind(|_, item| {
        // Drop the title binding, or a recycled row keeps mirroring the old node.
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

    // ---- title + body ------------------------------------------------------
    let title_entry = gtk::Entry::builder()
        .placeholder_text("Untitled")
        .has_frame(false)
        .margin_start(22)
        .margin_end(24)
        .margin_top(14)
        .build();
    title_entry.add_css_class("oma-title");

    let body_view = gtk::TextView::builder()
        .wrap_mode(gtk::WrapMode::Word)
        .left_margin(24)
        .right_margin(24)
        .top_margin(10)
        .bottom_margin(18)
        .pixels_below_lines(4)
        .build();
    body_view.add_css_class("oma-body");
    let buffer = body_view.buffer();
    buffer.set_enable_undo(true);

    let body_scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .child(&body_view)
        .build();

    let right = gtk::Box::new(gtk::Orientation::Vertical, 0);
    right.append(&title_entry);
    right.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
    right.append(&body_scroll);

    let paned = gtk::Paned::builder()
        .orientation(gtk::Orientation::Horizontal)
        .start_child(&outline_scroll)
        .end_child(&right)
        .resize_start_child(false)
        .shrink_start_child(false)
        .shrink_end_child(false)
        .position(wstate.paned.unwrap_or(320))
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
    let sb_header = adw::HeaderBar::new();
    sb_header.set_title_widget(Some(&adw::WindowTitle::new("Outlines", "")));
    sb_header.set_show_end_title_buttons(false);
    sb_header.pack_end(&new_btn);
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
        .title("omaverse")
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
        pending_title: RefCell::new(None),
        dirty_doc: Cell::new(false),
        dirty_state: Cell::new(false),
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
        title_entry: title_entry.clone(),
        body_view: body_view.clone(),
        buffer: buffer.clone(),
        libbox: libbox.clone(),
        lib_paths: RefCell::new(Vec::new()),
    });

    // Note: these closures hold a strong Rc to App, which also owns the widgets.
    // That cycle is never collected, but App lives for the whole process, so the
    // alternative (threading Weak through every handler) buys nothing here.
    {
        let a = app.clone();
        factory.connect_bind(move |_, item| a.bind_row(item));
    }
    {
        let a = app.clone();
        selection.connect_selected_notify(move |_| a.on_selection_changed());
    }
    {
        let a = app.clone();
        buffer.connect_changed(move |_| {
            if !a.loading.get() {
                a.dirty_doc.set(true);
            }
        });
    }
    {
        let a = app.clone();
        title_entry.connect_changed(move |e| {
            if a.loading.get() {
                return;
            }
            *a.pending_title.borrow_mut() = Some(e.text().to_string());
            a.dirty_doc.set(true);
        });
    }
    {
        // Enter in the title drops into the body, which is the order you write in.
        let a = app.clone();
        title_entry.connect_activate(move |_| a.focus_body());
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

            let cmd = if enter && ctrl {
                a.focus_body();
                return glib::Propagation::Stop;
            } else if enter {
                Cmd::NewSiblingBelow
            } else if key == gdk::Key::ISO_Left_Tab || (key == gdk::Key::Tab && shift) {
                Cmd::Outdent
            } else if key == gdk::Key::Tab {
                Cmd::Indent
            } else if key == gdk::Key::Up && alt {
                Cmd::MoveUp
            } else if key == gdk::Key::Down && alt {
                Cmd::MoveDown
            } else if key == gdk::Key::Delete {
                Cmd::Delete
            } else if key == gdk::Key::F2 {
                a.focus_title();
                return glib::Propagation::Stop;
            } else {
                return glib::Propagation::Proceed;
            };
            a.apply_cmd(cmd);
            glib::Propagation::Stop
        });
        list.add_controller(kc);
    }
    // Ctrl+Enter from the body goes back to the title.
    {
        let a = app.clone();
        let kc = gtk::EventControllerKey::new();
        kc.connect_key_pressed(move |_, key, _, state| {
            let enter = key == gdk::Key::Return || key == gdk::Key::KP_Enter;
            if enter && state.contains(gdk::ModifierType::CONTROL_MASK) {
                a.focus_title();
                glib::Propagation::Stop
            } else {
                glib::Propagation::Proceed
            }
        });
        body_view.add_controller(kc);
    }

    // ---- actions -----------------------------------------------------------
    add_action(&window, "save", {
        let a = app.clone();
        move || {
            a.commit_pending();
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
    gapp.set_accels_for_action("win.save", &["<Primary>s"]);
    gapp.set_accels_for_action("win.toggle-sidebar", &["<Primary>backslash"]);
    gapp.set_accels_for_action("win.new-outline", &["<Primary>n"]);

    // Periodic flushers. Polling a dirty flag avoids the cancellation bugs that
    // come with rescheduling a timer on every keystroke.
    {
        let a = app.clone();
        glib::timeout_add_local(Duration::from_millis(SAVE_DOC_MS), move || {
            if a.dirty_doc.get() {
                a.commit_pending();
                a.save_doc();
                a.dirty_doc.set(false);
            }
            glib::ControlFlow::Continue
        });
    }
    {
        let a = app.clone();
        glib::timeout_add_local(Duration::from_millis(SAVE_STATE_MS), move || {
            if a.dirty_state.get() {
                a.save_state();
                a.dirty_state.set(false);
            }
            glib::ControlFlow::Continue
        });
    }
    {
        let a = app.clone();
        window.connect_close_request(move |_| {
            a.commit_pending();
            a.save_doc();
            a.save_state();
            a.save_window_state();
            glib::Propagation::Proceed
        });
    }

    app.refresh_library();
    if let Some(p) = cli.or(wstate.last_file).filter(|p| p.exists()) {
        app.open(&p);
    }
    window.present();
}

fn add_action(window: &adw::ApplicationWindow, name: &str, f: impl Fn() + 'static) {
    let a = gio::SimpleAction::new(name, None);
    a.connect_activate(move |_, _| f());
    window.add_action(&a);
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
    // ---- rows --------------------------------------------------------------

    fn bind_row(self: &Rc<Self>, item: &glib::Object) {
        let Some(item) = item.downcast_ref::<gtk::ListItem>() else { return };
        let Some(row) = item.item().and_downcast::<gtk::TreeListRow>() else { return };
        let Some(obj) = row.item().and_downcast::<NodeObject>() else { return };
        let Some(expander) = item.child().and_downcast::<gtk::TreeExpander>() else { return };
        let Some(label) = expander.child().and_downcast::<gtk::Label>() else { return };

        expander.set_list_row(Some(&row));
        label.set_text(&obj.display_title());

        // Mirror later renames onto the label without rebuilding the tree.
        let binding = obj
            .bind_property("title", &label, "label")
            .transform_to(|_, t: String| {
                Some(if t.trim().is_empty() { "Untitled".to_string() } else { t })
            })
            .build();
        unsafe { item.set_data("oma-title-binding", binding) };

        // Watch fold state once per row. The handler dies with the row, so
        // there is nothing to disconnect on unbind.
        unsafe {
            if row.data::<bool>("oma-watched").is_none() {
                row.set_data("oma-watched", true);
                let me = self.clone();
                row.connect_expanded_notify(move |r| me.on_row_expanded(r));
            }
        }
    }

    fn on_row_expanded(&self, row: &gtk::TreeListRow) {
        let Some(obj) = row.item().and_downcast::<NodeObject>() else { return };
        let key = path_key(&obj.path());
        {
            let mut st = self.dstate.borrow_mut();
            if row.is_expanded() {
                st.collapsed.remove(&key);
            } else {
                st.collapsed.insert(key);
            }
        }
        self.dirty_state.set(true);
    }

    // ---- editing -----------------------------------------------------------

    fn apply_cmd(self: &Rc<Self>, cmd: Cmd) {
        // Anything typed but not yet in the model must land first, or the
        // command would operate on stale text.
        self.commit_pending();
        let at = self.selected.borrow().clone();
        if cmd.is_destructive() && self.needs_confirmation(at.as_deref()) {
            self.confirm_delete(at);
            return;
        }
        self.run(cmd, at);
    }

    fn run(&self, cmd: Cmd, at: Option<NodePath>) {
        let outcome = {
            let mut d = self.doc.borrow_mut();
            edit::apply(&mut d, at.as_deref(), cmd)
        };
        let Some(out) = outcome else { return };
        self.dirty_doc.set(true);
        if out.structural {
            self.rebuild_tree();
        }
        match out.select {
            Some(p) => {
                self.select_path(&p);
                if out.focus_title {
                    self.focus_title();
                }
            }
            None => {
                // Never leave the document with nowhere to type.
                let p = self.doc.borrow_mut().push_root(Node::new(""));
                self.rebuild_tree();
                self.select_path(&p);
                self.focus_title();
            }
        }
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
            "Delete this untitled node?".to_string()
        } else {
            format!("Delete \u{201c}{title}\u{201d}?")
        };
        let mut detail = String::new();
        if kids > 0 {
            detail.push_str(&format!(
                "{kids} node{} beneath it will go too",
                if kids == 1 { "" } else { "s" }
            ));
        }
        if has_body {
            if !detail.is_empty() {
                detail.push_str(", and ");
            }
            detail.push_str("its notes will be deleted");
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

    /// Push the title the user has typed into the model, and the body with it.
    fn commit_pending(&self) {
        let pending = self.pending_title.borrow_mut().take();
        if let (Some(t), Some(p)) = (pending, self.selected.borrow().clone()) {
            let changed = {
                let mut d = self.doc.borrow_mut();
                edit::apply(&mut d, Some(&p), Cmd::SetTitle(t.clone())).is_some()
            };
            if changed {
                self.update_row_title(&p, &t);
            }
        }
        self.flush_body();
    }

    fn update_row_title(&self, path: &[usize], title: &str) {
        if let Some(i) = self.find_row(path) {
            if let Some(obj) = self.tree.row(i).and_then(|r| r.item()).and_downcast::<NodeObject>() {
                obj.set_title(title);
            }
        }
    }

    fn focus_title(&self) {
        self.title_entry.grab_focus();
    }

    fn focus_body(&self) {
        self.body_view.grab_focus();
    }

    // ---- selection / editors ----------------------------------------------

    fn current_selection_path(&self) -> Option<NodePath> {
        let row = self.selection.selected_item()?.downcast::<gtk::TreeListRow>().ok()?;
        Some(row.item().and_downcast::<NodeObject>()?.path())
    }

    fn on_selection_changed(&self) {
        if self.loading.get() {
            return;
        }
        self.commit_pending();
        let new = self.current_selection_path();
        *self.selected.borrow_mut() = new.clone();
        self.load_node(new.as_deref());
        if let Some(p) = new {
            self.dstate.borrow_mut().selected = Some(path_key(&p));
            self.dirty_state.set(true);
        }
    }

    /// Fill the title entry and body editor from the model.
    fn load_node(&self, path: Option<&[usize]>) {
        let (title, body) = match path {
            Some(p) => self
                .doc
                .borrow()
                .get(p)
                .map(|n| (n.title.clone(), n.body.clone()))
                .unwrap_or_default(),
            None => (String::new(), String::new()),
        };
        self.loading.set(true);
        self.title_entry.set_text(&title);
        self.title_entry.set_sensitive(path.is_some());
        self.buffer.set_text(&body);
        // Undo history must not cross from one node into another.
        self.buffer.set_enable_undo(false);
        self.buffer.set_enable_undo(true);
        self.loading.set(false);
        self.pending_title.borrow_mut().take();
    }

    /// Copy the editor's contents back into the selected node.
    fn flush_body(&self) {
        let Some(p) = self.selected.borrow().clone() else { return };
        let (s, e) = self.buffer.bounds();
        let new = self.buffer.text(&s, &e, false).to_string().trim_end().to_string();
        let mut d = self.doc.borrow_mut();
        if let Some(n) = d.get_mut(&p) {
            if n.body != new {
                n.body = new;
            }
        }
    }

    // ---- documents ---------------------------------------------------------

    pub fn open(&self, path: &PathBuf) {
        if self.path.borrow().is_some() {
            self.commit_pending();
            self.save_doc();
            self.save_state();
            self.dirty_doc.set(false);
            self.dirty_state.set(false);
        }

        let src = std::fs::read_to_string(path).unwrap_or_default();
        *self.doc.borrow_mut() = parse::parse(&src);
        *self.path.borrow_mut() = Some(path.clone());
        *self.dstate.borrow_mut() = state::load_doc_state(path);
        *self.selected.borrow_mut() = None;
        self.pending_title.borrow_mut().take();

        let title = self.doc.borrow().title.clone().unwrap_or_else(|| library::title_of(path));
        self.wtitle.set_title(&title);
        self.wtitle.set_subtitle(&self.pretty(path));
        self.wtitle.remove_css_class("oma-error");
        self.window.set_title(Some(&format!("{title} — omaverse")));
        self.stack.set_visible_child_name("doc");

        self.rebuild_tree();

        let remembered = self
            .dstate
            .borrow()
            .selected
            .clone()
            .and_then(|k| parse_path_key(&k))
            .filter(|p| self.doc.borrow().get(p).is_some());
        match remembered {
            Some(p) => self.select_path(&p),
            None if !self.doc.borrow().is_empty() => self.select_path(&[0]),
            None => self.load_node(None),
        }
    }

    fn new_outline(self: &Rc<Self>) {
        let entry = gtk::Entry::builder()
            .placeholder_text("e.g. Romans")
            .activates_default(true)
            .build();
        let dlg = adw::AlertDialog::new(
            Some("New outline"),
            Some(&format!("Created in {}", self.pretty(&self.cfg.outline_dir))),
        );
        dlg.set_extra_child(Some(&entry));
        dlg.add_response("cancel", "Cancel");
        dlg.add_response("create", "Create");
        dlg.set_response_appearance("create", adw::ResponseAppearance::Suggested);
        dlg.set_default_response(Some("create"));
        dlg.set_close_response("cancel");
        let me = self.clone();
        dlg.connect_response(None, move |_, resp| {
            if resp != "create" {
                return;
            }
            let name = entry.text().to_string().trim().to_string();
            if !name.is_empty() {
                me.create_outline(&name);
            }
        });
        dlg.present(Some(&self.window));
    }

    fn create_outline(&self, name: &str) {
        let safe: String = name
            .chars()
            .map(|c| if std::path::is_separator(c) || c == ':' { '-' } else { c })
            .collect();
        let path = self.cfg.outline_dir.join(format!("{safe}.md"));
        if !path.exists() {
            let mut d = Document::default();
            d.title = Some(name.to_string());
            d.push_root(Node::new(""));
            if let Err(e) = crate::atomic_write(&path, parse::serialize(&d).as_bytes()) {
                eprintln!("omaverse: could not create {}: {e}", path.display());
                self.wtitle.set_subtitle(&format!("Could not create outline — {e}"));
                self.wtitle.add_css_class("oma-error");
                return;
            }
        }
        self.refresh_library();
        self.open(&path);
        self.focus_title();
    }

    fn pretty(&self, path: &std::path::Path) -> String {
        match path.strip_prefix(state::home()) {
            Ok(rest) => format!("~/{}", rest.display()),
            Err(_) => path.display().to_string(),
        }
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

    /// Expand every row not listed as collapsed. Expanding a row inserts its
    /// children immediately after it, so one forward pass reaches the whole tree.
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

    fn select_path(&self, path: &[usize]) {
        // Every ancestor must be open, or the row does not exist yet.
        for depth in 1..path.len() {
            if let Some(row) = self.find_row(&path[..depth]).and_then(|i| self.tree.row(i)) {
                if row.is_expandable() && !row.is_expanded() {
                    row.set_expanded(true);
                }
            }
        }
        if let Some(i) = self.find_row(path) {
            self.loading.set(true);
            self.selection.set_selected(i);
            *self.selected.borrow_mut() = Some(path.to_vec());
            self.loading.set(false);
            self.load_node(Some(path));
            self.list.scroll_to(i, gtk::ListScrollFlags::NONE, None);
            // `loading` suppresses on_selection_changed, so record the selection
            // here too -- otherwise it is never persisted and reopening always
            // lands on the first node.
            self.dstate.borrow_mut().selected = Some(path_key(path));
            self.dirty_state.set(true);
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
                let label = gtk::Label::builder()
                    .label(&e.title)
                    .xalign(0.0)
                    .ellipsize(pango::EllipsizeMode::End)
                    .margin_start(12)
                    .margin_end(12)
                    .margin_top(6)
                    .margin_bottom(6)
                    .build();
                let row = gtk::ListBoxRow::new();
                row.set_child(Some(&label));
                self.libbox.append(&row);
            }
        }
        *self.lib_paths.borrow_mut() = paths;
    }

    fn on_library_activated(&self, row: &gtk::ListBoxRow) {
        let idx = row.index();
        if idx < 0 {
            return;
        }
        if let Some(p) = self.lib_paths.borrow().get(idx as usize).cloned().flatten() {
            self.open(&p);
        }
    }

    // ---- saving ------------------------------------------------------------

    fn save_doc(&self) {
        let Some(path) = self.path.borrow().clone() else { return };
        let text = parse::serialize(&self.doc.borrow());
        match crate::atomic_write(&path, text.as_bytes()) {
            Ok(()) => {
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
        });
    }
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

fn count_descendants(n: &Node) -> usize {
    n.children.len() + n.children.iter().map(count_descendants).sum::<usize>()
}
