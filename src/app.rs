//! Phase 1 UI: library sidebar | outline tree | body editor.
//!
//! The `Document` is the single source of truth. The tree is rebuilt wholesale
//! on structural change rather than patched incrementally — outlines run to
//! hundreds of nodes, not millions, so the simpler code is worth more than the
//! saved redraws.

use crate::config::{self, Config};
use crate::library;
use crate::model::{path_key, parse_path_key, Document, NodePath};
use crate::nodeobj::NodeObject;
use crate::parse;
use crate::state::{self, DocState};

use gtk4 as gtk;
use gtk4::gio;
use gtk4::glib;
use gtk4::pango;
use gtk4::prelude::*;
use libadwaita as adw;

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

const SAVE_DOC_MS: u64 = 800;
const SAVE_STATE_MS: u64 = 2000;

const CSS: &str = "
.oma-body { font-family: 'Source Serif 4','Noto Serif','DejaVu Serif',serif; font-size: 12.5pt; }
.oma-outline { font-size: 10.5pt; }
.oma-group { font-size: 9pt; font-weight: bold; opacity: 0.55; }
.oma-untitled { opacity: 0.45; font-style: italic; }
.oma-error { color: #e01b24; }
";

pub struct App {
    cfg: Config,
    doc: Rc<RefCell<Document>>,
    path: RefCell<Option<PathBuf>>,
    dstate: RefCell<DocState>,
    selected: RefCell<Option<NodePath>>,

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
        let item = item.downcast_ref::<gtk::ListItem>().expect("ListItem");
        let label = gtk::Label::new(None);
        label.set_xalign(0.0);
        label.set_ellipsize(pango::EllipsizeMode::End);
        let expander = gtk::TreeExpander::new();
        expander.set_child(Some(&label));
        item.set_child(Some(&expander));
    });

    let list = gtk::ListView::new(Some(selection.clone()), Some(factory.clone()));
    list.add_css_class("oma-outline");
    list.set_single_click_activate(false);

    let outline_scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .child(&list)
        .build();

    // ---- body editor -------------------------------------------------------
    let body = gtk::TextView::builder()
        .wrap_mode(gtk::WrapMode::Word)
        .left_margin(24)
        .right_margin(24)
        .top_margin(18)
        .bottom_margin(18)
        .pixels_below_lines(4)
        .build();
    body.add_css_class("oma-body");
    let buffer = body.buffer();
    buffer.set_enable_undo(true);

    let body_scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .child(&body)
        .build();

    let paned = gtk::Paned::builder()
        .orientation(gtk::Orientation::Horizontal)
        .start_child(&outline_scroll)
        .end_child(&body_scroll)
        .resize_start_child(false)
        .shrink_start_child(false)
        .shrink_end_child(false)
        .position(wstate.paned.unwrap_or(320))
        .build();

    let empty = adw::StatusPage::builder()
        .icon_name("view-list-symbolic")
        .title("No outline open")
        .description("Choose a book from the sidebar, or pass a file: omaverse path/to/romans.md")
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
    let sidebar = adw::ToolbarView::new();
    let sb_header = adw::HeaderBar::new();
    sb_header.set_title_widget(Some(&adw::WindowTitle::new("Outlines", "")));
    sb_header.set_show_end_title_buttons(false);
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
        list,
        buffer: buffer.clone(),
        libbox: libbox.clone(),
        lib_paths: RefCell::new(Vec::new()),
    });

    // Note: these closures hold a strong Rc to App, which also owns the widgets.
    // That cycle is never collected, but App lives for the whole process, so the
    // alternative (threading Weak through every handler) buys nothing here.
    {
        let app2 = app.clone();
        factory.connect_bind(move |_, item| app2.bind_row(item));
    }
    {
        let app2 = app.clone();
        selection.connect_selected_notify(move |_| app2.on_selection_changed());
    }
    {
        let app2 = app.clone();
        buffer.connect_changed(move |_| {
            if !app2.loading.get() {
                app2.dirty_doc.set(true);
            }
        });
    }
    {
        let app2 = app.clone();
        libbox.connect_row_activated(move |_, row| app2.on_library_activated(row));
    }
    {
        let s = split.clone();
        sb_toggle.connect_toggled(move |b| s.set_show_sidebar(b.is_active()));
    }
    {
        let b = sb_toggle.clone();
        split.connect_show_sidebar_notify(move |s| b.set_active(s.shows_sidebar()));
    }

    // Actions + accelerators
    let save = gio::SimpleAction::new("save", None);
    {
        let app2 = app.clone();
        save.connect_activate(move |_, _| {
            app2.flush_body();
            app2.save_doc();
            app2.dirty_doc.set(false);
        });
    }
    window.add_action(&save);
    let toggle = gio::SimpleAction::new("toggle-sidebar", None);
    {
        let s = split.clone();
        toggle.connect_activate(move |_, _| s.set_show_sidebar(!s.shows_sidebar()));
    }
    window.add_action(&toggle);
    gapp.set_accels_for_action("win.save", &["<Primary>s"]);
    gapp.set_accels_for_action("win.toggle-sidebar", &["<Primary>backslash"]);

    // Periodic flushers. Polling a dirty flag avoids the cancellation bugs that
    // come with rescheduling a timer on every keystroke.
    {
        let app2 = app.clone();
        glib::timeout_add_local(Duration::from_millis(SAVE_DOC_MS), move || {
            if app2.dirty_doc.get() {
                app2.flush_body();
                app2.save_doc();
                app2.dirty_doc.set(false);
            }
            glib::ControlFlow::Continue
        });
    }
    {
        let app2 = app.clone();
        glib::timeout_add_local(Duration::from_millis(SAVE_STATE_MS), move || {
            if app2.dirty_state.get() {
                app2.save_state();
                app2.dirty_state.set(false);
            }
            glib::ControlFlow::Continue
        });
    }
    {
        let app2 = app.clone();
        window.connect_close_request(move |_| {
            app2.flush_body();
            app2.save_doc();
            app2.save_state();
            app2.save_window_state();
            glib::Propagation::Proceed
        });
    }

    app.refresh_library();
    let initial = cli.or(wstate.last_file);
    if let Some(p) = initial.filter(|p| p.exists()) {
        app.open(&p);
    }
    window.present();
}

fn load_css() {
    let provider = gtk::CssProvider::new();
    provider.load_from_string(CSS);
    if let Some(display) = gtk::gdk::Display::default() {
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
        let t = obj.title();
        if t.trim().is_empty() {
            label.set_text("Untitled");
            label.add_css_class("oma-untitled");
        } else {
            label.set_text(&t);
            label.remove_css_class("oma-untitled");
        }

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
        let mut st = self.dstate.borrow_mut();
        if row.is_expanded() {
            st.collapsed.remove(&key);
        } else {
            st.collapsed.insert(key);
        }
        drop(st);
        self.dirty_state.set(true);
    }

    // ---- selection / body --------------------------------------------------

    fn current_selection_path(&self) -> Option<NodePath> {
        let item = self.selection.selected_item()?;
        let row = item.downcast::<gtk::TreeListRow>().ok()?;
        let obj = row.item().and_downcast::<NodeObject>()?;
        Some(obj.path())
    }

    fn on_selection_changed(&self) {
        if self.loading.get() {
            return;
        }
        self.flush_body();
        let new = self.current_selection_path();
        *self.selected.borrow_mut() = new.clone();
        match new {
            Some(p) => {
                self.load_body(&p);
                let key = path_key(&p);
                self.dstate.borrow_mut().selected = Some(key);
                self.dirty_state.set(true);
            }
            None => self.load_text(""),
        }
    }

    fn load_text(&self, text: &str) {
        self.loading.set(true);
        self.buffer.set_text(text);
        self.buffer.set_enable_undo(false);
        self.buffer.set_enable_undo(true); // drop undo history across documents
        self.loading.set(false);
    }

    fn load_body(&self, path: &[usize]) {
        let text = self.doc.borrow().get(path).map(|n| n.body.clone()).unwrap_or_default();
        self.load_text(&text);
    }

    /// Copy the editor's contents back into the selected node.
    fn flush_body(&self) {
        let Some(p) = self.selected.borrow().clone() else { return };
        let (s, e) = self.buffer.bounds();
        let text = self.buffer.text(&s, &e, false).to_string();
        let new = text.trim_end().to_string();
        let mut d = self.doc.borrow_mut();
        if let Some(n) = d.get_mut(&p) {
            if n.body != new {
                n.body = new;
            }
        }
    }

    // ---- documents ---------------------------------------------------------

    pub fn open(&self, path: &PathBuf) {
        // Leaving the previous document: make sure nothing is left unsaved.
        if self.path.borrow().is_some() {
            self.flush_body();
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

        let title = self
            .doc
            .borrow()
            .title
            .clone()
            .unwrap_or_else(|| library::title_of(path));
        self.wtitle.set_title(&title);
        self.wtitle.set_subtitle(&self.pretty_location(path));
        self.window.set_title(Some(&format!("{title} — omaverse")));
        self.stack.set_visible_child_name("doc");

        self.rebuild_tree();

        // Restore the remembered selection, else the first node.
        let want = self.dstate.borrow().selected.clone().and_then(|k| parse_path_key(&k));
        let target = want.filter(|p| self.doc.borrow().get(p).is_some());
        if let Some(p) = target {
            self.select_path(&p);
        } else if !self.doc.borrow().is_empty() {
            self.select_path(&[0]);
        } else {
            self.load_text("");
        }
    }

    fn pretty_location(&self, path: &std::path::Path) -> String {
        let home = state::home();
        match path.strip_prefix(&home) {
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
                self.root_store.append(&NodeObject::new(vec![i], &n.title, !n.children.is_empty()));
            }
        }
        self.apply_expansion();
        self.loading.set(false);
    }

    /// Expand every row not listed as collapsed. Expanding a row inserts its
    /// children immediately after it, so a single forward pass reaches the whole
    /// visible tree.
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
        for i in 0..self.tree.n_items() {
            let row = self.tree.row(i)?;
            let obj = row.item().and_downcast::<NodeObject>()?;
            if obj.path() == path {
                return Some(i);
            }
        }
        None
    }

    fn select_path(&self, path: &[usize]) {
        // Make sure every ancestor is open, or the row will not exist yet.
        for depth in 1..path.len() {
            if let Some(i) = self.find_row(&path[..depth]) {
                if let Some(row) = self.tree.row(i) {
                    if row.is_expandable() && !row.is_expanded() {
                        row.set_expanded(true);
                    }
                }
            }
        }
        if let Some(i) = self.find_row(path) {
            self.loading.set(true);
            self.selection.set_selected(i);
            *self.selected.borrow_mut() = Some(path.to_vec());
            self.loading.set(false);
            self.load_body(path);
            self.list.scroll_to(i, gtk::ListScrollFlags::NONE, None);
            // `loading` suppresses on_selection_changed, so record the selection
            // here too -- otherwise the initial selection is never persisted and
            // reopening always lands on the first node.
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
            let l = gtk::Label::new(Some(&format!(
                "No outlines in\n{}",
                self.pretty_location(&self.cfg.outline_dir)
            )));
            l.set_wrap(true);
            l.set_xalign(0.0);
            l.set_margin_start(12);
            l.set_margin_end(12);
            l.set_margin_top(12);
            l.add_css_class("dim-label");
            let row = gtk::ListBoxRow::new();
            row.set_child(Some(&l));
            row.set_selectable(false);
            row.set_activatable(false);
            self.libbox.append(&row);
            paths.push(None);
        }

        for g in groups {
            if let Some(name) = &g.name {
                let l = gtk::Label::new(Some(&name.to_uppercase()));
                l.set_xalign(0.0);
                l.set_margin_start(12);
                l.set_margin_top(10);
                l.set_margin_bottom(2);
                l.add_css_class("oma-group");
                let row = gtk::ListBoxRow::new();
                row.set_child(Some(&l));
                row.set_selectable(false);
                row.set_activatable(false);
                self.libbox.append(&row);
                paths.push(None);
            }
            for e in &g.entries {
                let l = gtk::Label::new(Some(&e.title));
                l.set_xalign(0.0);
                l.set_ellipsize(pango::EllipsizeMode::End);
                l.set_margin_start(12);
                l.set_margin_end(12);
                l.set_margin_top(6);
                l.set_margin_bottom(6);
                let row = gtk::ListBoxRow::new();
                row.set_child(Some(&l));
                self.libbox.append(&row);
                paths.push(Some(e.path.clone()));
            }
        }
        *self.lib_paths.borrow_mut() = paths;
    }

    fn on_library_activated(&self, row: &gtk::ListBoxRow) {
        let idx = row.index();
        if idx < 0 {
            return;
        }
        let path = self.lib_paths.borrow().get(idx as usize).cloned().flatten();
        if let Some(p) = path {
            self.open(&p);
        }
    }

    // ---- saving ------------------------------------------------------------

    fn save_doc(&self) {
        let Some(path) = self.path.borrow().clone() else { return };
        let text = parse::serialize(&self.doc.borrow());
        match crate::atomic_write(&path, text.as_bytes()) {
            Ok(()) => {
                self.wtitle.set_subtitle(&self.pretty_location(&path));
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
