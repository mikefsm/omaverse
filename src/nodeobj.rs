//! A GObject wrapper around one outline node, so nodes can live in a
//! `GtkTreeListModel`. It carries the node's index path rather than the node
//! itself; the `Document` stays the single source of truth.
//!
//! `title` is a real GObject property so a rename updates the outline label
//! through a binding, instead of rebuilding the tree while the cursor is in it.

use gtk4::glib;
use gtk4::prelude::*;
use gtk4::subclass::prelude::*;
use std::cell::{Cell, RefCell};

mod imp {
    use super::*;

    #[derive(Default, glib::Properties)]
    #[properties(wrapper_type = super::NodeObject)]
    pub struct NodeObject {
        pub path: RefCell<Vec<usize>>,
        #[property(get, set)]
        pub title: RefCell<String>,
        pub has_children: Cell<bool>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for NodeObject {
        const NAME: &'static str = "OmaverseNodeObject";
        type Type = super::NodeObject;
    }

    #[glib::derived_properties]
    impl ObjectImpl for NodeObject {}
}

glib::wrapper! {
    pub struct NodeObject(ObjectSubclass<imp::NodeObject>);
}

impl NodeObject {
    pub fn new(path: Vec<usize>, title: &str, has_children: bool) -> Self {
        let o: Self = glib::Object::builder().build();
        *o.imp().path.borrow_mut() = path;
        o.set_title(title);
        o.imp().has_children.set(has_children);
        o
    }

    pub fn path(&self) -> Vec<usize> {
        self.imp().path.borrow().clone()
    }

    pub fn has_children(&self) -> bool {
        self.imp().has_children.get()
    }

    /// What the outline row shows: the title, or a placeholder when unnamed.
    pub fn display_title(&self) -> String {
        let t = self.title();
        if t.trim().is_empty() {
            "Untitled".to_string()
        } else {
            t
        }
    }
}
