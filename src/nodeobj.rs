//! A GObject wrapper around one outline node, so nodes can live in a
//! `GtkTreeListModel`. It carries the node's index path rather than the node
//! itself; the `Document` stays the single source of truth.

use gtk4::glib;
use gtk4::subclass::prelude::*;
use std::cell::{Cell, RefCell};

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct NodeObject {
        pub path: RefCell<Vec<usize>>,
        pub title: RefCell<String>,
        pub has_children: Cell<bool>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for NodeObject {
        const NAME: &'static str = "OmaverseNodeObject";
        type Type = super::NodeObject;
    }

    impl ObjectImpl for NodeObject {}
}

glib::wrapper! {
    pub struct NodeObject(ObjectSubclass<imp::NodeObject>);
}

impl NodeObject {
    pub fn new(path: Vec<usize>, title: &str, has_children: bool) -> Self {
        let o: Self = glib::Object::builder().build();
        let imp = o.imp();
        *imp.path.borrow_mut() = path;
        *imp.title.borrow_mut() = title.to_string();
        imp.has_children.set(has_children);
        o
    }

    pub fn path(&self) -> Vec<usize> {
        self.imp().path.borrow().clone()
    }

    pub fn title(&self) -> String {
        self.imp().title.borrow().clone()
    }

    pub fn set_title(&self, t: &str) {
        *self.imp().title.borrow_mut() = t.to_string();
    }

    pub fn has_children(&self) -> bool {
        self.imp().has_children.get()
    }
}
