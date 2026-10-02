//! Outline editing as pure data: every structural command is a function of
//! (document, selected path) -> (new document, new selection).
//!
//! Kept deliberately free of GTK so the behaviour is testable without a display.
//! The widget handlers in `app.rs` do nothing but translate a keystroke into a
//! `Cmd` and apply the `Outcome`.

use crate::model::{Document, Node, NodePath};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cmd {
    /// Insert an empty sibling directly below. With no selection, appends a root.
    NewSiblingBelow,
    /// Make the node a child of its previous sibling.
    Indent,
    /// Make the node the next sibling of its parent.
    Outdent,
    MoveUp,
    MoveDown,
    /// Remove the node and everything under it.
    Delete,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    /// Where the selection should land, or `None` when the document is empty.
    pub select: Option<NodePath>,
    /// True when the tree's shape changed, so the view needs rebuilding.
    pub structural: bool,
    /// True when the node should be renamed right away (a fresh, empty node).
    pub focus_title: bool,
}

impl Cmd {
    /// Whether this command needs confirmation before it destroys anything.
    pub fn is_destructive(&self) -> bool {
        matches!(self, Cmd::Delete)
    }
}

/// Returns `None` when the command does not apply — first sibling asked to
/// indent, top node asked to move up, and so on. Callers treat that as a no-op.
pub fn apply(doc: &mut Document, at: Option<&[usize]>, cmd: Cmd) -> Option<Outcome> {
    match cmd {
        Cmd::NewSiblingBelow => {
            let select = match at {
                None => doc.push_root(Node::new("")),
                Some(p) => doc.insert_sibling_after(p, Node::new(""))?,
            };
            Some(Outcome { select: Some(select), structural: true, focus_title: true })
        }
        Cmd::Indent => {
            let select = doc.indent(at?)?;
            Some(Outcome { select: Some(select), structural: true, focus_title: false })
        }
        Cmd::Outdent => {
            let select = doc.outdent(at?)?;
            Some(Outcome { select: Some(select), structural: true, focus_title: false })
        }
        Cmd::MoveUp => {
            let select = doc.move_up(at?)?;
            Some(Outcome { select: Some(select), structural: true, focus_title: false })
        }
        Cmd::MoveDown => {
            let select = doc.move_down(at?)?;
            Some(Outcome { select: Some(select), structural: true, focus_title: false })
        }
        Cmd::Delete => {
            let path = at?;
            doc.remove(path)?;
            Some(Outcome { select: after_delete(doc, path), structural: true, focus_title: false })
        }
    }
}

/// Where to put the selection after a removal: the node that slid into its
/// place, else the one above it, else its parent.
fn after_delete(doc: &Document, removed: &[usize]) -> Option<NodePath> {
    let (&idx, parents) = removed.split_last()?;
    let siblings = doc.children_of(parents)?;
    if idx < siblings.len() {
        let mut p = parents.to_vec();
        p.push(idx);
        return Some(p);
    }
    if idx > 0 {
        let mut p = parents.to_vec();
        p.push(idx - 1);
        return Some(p);
    }
    if parents.is_empty() {
        None // the document is now empty
    } else {
        Some(parents.to_vec())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::path_key;

    /// a / b(b1, b2) / c
    fn fixture() -> Document {
        let mut d = Document::default();
        d.push_root(Node::new("a"));
        d.push_root(Node::new("b"));
        d.push_root(Node::new("c"));
        d.insert_child(&[1], Node::new("b1"));
        d.insert_child(&[1], Node::new("b2"));
        d
    }

    fn shape(d: &Document) -> Vec<String> {
        d.walk().iter().map(|(p, n)| format!("{}:{}", path_key(p), n.title)).collect()
    }

    #[test]
    fn new_sibling_lands_below_and_asks_to_be_named() {
        let mut d = fixture();
        let o = apply(&mut d, Some(&[0]), Cmd::NewSiblingBelow).unwrap();
        assert_eq!(o.select, Some(vec![1]));
        assert!(o.structural && o.focus_title);
        assert_eq!(shape(&d), ["0:a", "1:", "2:b", "2.0:b1", "2.1:b2", "3:c"]);
    }

    #[test]
    fn new_sibling_of_a_child_stays_at_that_depth() {
        let mut d = fixture();
        let o = apply(&mut d, Some(&[1, 0]), Cmd::NewSiblingBelow).unwrap();
        assert_eq!(o.select, Some(vec![1, 1]));
        assert_eq!(shape(&d), ["0:a", "1:b", "1.0:b1", "1.1:", "1.2:b2", "2:c"]);
    }

    #[test]
    fn new_sibling_with_no_selection_appends_a_root() {
        let mut d = Document::default();
        let o = apply(&mut d, None, Cmd::NewSiblingBelow).unwrap();
        assert_eq!(o.select, Some(vec![0]));
        assert_eq!(d.roots.len(), 1);
    }

    #[test]
    fn indent_and_outdent_are_inverse() {
        let mut d = fixture();
        let before = shape(&d);
        let o = apply(&mut d, Some(&[2]), Cmd::Indent).unwrap();
        assert_eq!(o.select, Some(vec![1, 2]));
        let o = apply(&mut d, Some(&[1, 2]), Cmd::Outdent).unwrap();
        assert_eq!(o.select, Some(vec![2]));
        assert_eq!(shape(&d), before);
    }

    #[test]
    fn impossible_commands_are_refused_without_changing_anything() {
        let mut d = fixture();
        let before = shape(&d);
        assert!(apply(&mut d, Some(&[0]), Cmd::Indent).is_none(), "first sibling cannot indent");
        assert!(apply(&mut d, Some(&[0]), Cmd::Outdent).is_none(), "root cannot outdent");
        assert!(apply(&mut d, Some(&[0]), Cmd::MoveUp).is_none(), "top node cannot move up");
        assert!(apply(&mut d, Some(&[2]), Cmd::MoveDown).is_none(), "last node cannot move down");
        assert!(apply(&mut d, None, Cmd::Indent).is_none(), "nothing selected");
        assert!(apply(&mut d, None, Cmd::Delete).is_none());
        assert_eq!(shape(&d), before);
    }

    #[test]
    fn delete_selects_the_node_that_slid_into_place() {
        let mut d = fixture();
        let o = apply(&mut d, Some(&[1]), Cmd::Delete).unwrap();
        assert_eq!(o.select, Some(vec![1]), "c moved up into index 1");
        assert_eq!(shape(&d), ["0:a", "1:c"]);
    }

    #[test]
    fn deleting_the_last_sibling_selects_the_one_above() {
        let mut d = fixture();
        let o = apply(&mut d, Some(&[2]), Cmd::Delete).unwrap();
        assert_eq!(o.select, Some(vec![1]));
    }

    #[test]
    fn deleting_an_only_child_selects_the_parent() {
        let mut d = Document::default();
        d.push_root(Node::new("a"));
        d.insert_child(&[0], Node::new("a1"));
        let o = apply(&mut d, Some(&[0, 0]), Cmd::Delete).unwrap();
        assert_eq!(o.select, Some(vec![0]));
    }

    #[test]
    fn deleting_the_only_node_leaves_nothing_selected() {
        let mut d = Document::default();
        d.push_root(Node::new("only"));
        let o = apply(&mut d, Some(&[0]), Cmd::Delete).unwrap();
        assert_eq!(o.select, None);
        assert!(d.is_empty());
    }

    #[test]
    fn delete_takes_the_whole_subtree() {
        let mut d = fixture();
        apply(&mut d, Some(&[1]), Cmd::Delete).unwrap();
        assert!(!shape(&d).iter().any(|s| s.ends_with("b1") || s.ends_with("b2")));
    }

    #[test]
    fn only_delete_needs_confirming() {
        assert!(Cmd::Delete.is_destructive());
        assert!(!Cmd::Indent.is_destructive());
        assert!(!Cmd::NewSiblingBelow.is_destructive());
    }

    #[test]
    fn move_preserves_bodies_and_children() {
        let mut d = fixture();
        d.get_mut(&[1]).unwrap().body = "kept".into();
        apply(&mut d, Some(&[1]), Cmd::MoveUp).unwrap();
        assert_eq!(d.get(&[0]).unwrap().title, "b");
        assert_eq!(d.get(&[0]).unwrap().body, "kept");
        assert_eq!(d.get(&[0, 0]).unwrap().title, "b1");
    }
}
