//! Outline editing as pure data: every structural command is a function of
//! (document, selected path) -> (new document, new selection).
//!
//! Kept deliberately free of GTK so the behaviour is testable without a display.
//! The widget handlers in `app.rs` do nothing but translate a keystroke into a
//! `Cmd` and apply the `Outcome`.

use crate::model::{Document, Node, NodePath};

#[derive(Debug, Clone, PartialEq, Eq)]
// MoveTo is unused while dragging is out; it and its tests are kept so the
// gesture can come back without reinventing the index arithmetic.
#[allow(dead_code)]
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
    /// Drop this section's heading so its text joins the section above it.
    MergeIntoPrevious,
    /// Lift the section out, handing it back on the outcome.
    Cut,
    /// Drop a previously lifted section in below the selection.
    Paste(Node),
    /// Move the section to `index` among `parent`'s children. Used by dragging.
    MoveTo { parent: NodePath, index: usize },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    /// Where the selection should land, or `None` when the document is empty.
    pub select: Option<NodePath>,
    /// True when the tree's shape changed, so the view needs rebuilding.
    pub structural: bool,
    /// True when the node should be renamed right away (a fresh, empty node).
    pub focus_title: bool,
    /// The section removed by `Cut`, ready to be pasted elsewhere.
    pub lifted: Option<Node>,
}

impl Outcome {
    fn at(select: NodePath, structural: bool) -> Self {
        Outcome { select: Some(select), structural, focus_title: false, lifted: None }
    }
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
            Some(Outcome { select: Some(select), structural: true, focus_title: true, lifted: None })
        }
        Cmd::Indent => Some(Outcome::at(doc.indent(at?)?, true)),
        Cmd::Outdent => Some(Outcome::at(doc.outdent(at?)?, true)),
        Cmd::MoveUp => Some(Outcome::at(doc.move_up(at?)?, true)),
        Cmd::MoveDown => Some(Outcome::at(doc.move_down(at?)?, true)),
        Cmd::Delete => {
            let path = at?;
            doc.remove(path)?;
            Some(Outcome {
                select: after_delete(doc, path),
                structural: true,
                focus_title: false,
                lifted: None,
            })
        }
        Cmd::MergeIntoPrevious => {
            let path = at?;
            let target = merge_target(path)?;
            let node = doc.remove(path)?;
            let into = doc.get_mut(&target)?;
            if !node.body.trim().is_empty() {
                if into.body.trim().is_empty() {
                    into.body = node.body;
                } else {
                    into.body.push_str("\n\n");
                    into.body.push_str(&node.body);
                }
            }
            into.children.extend(node.children);
            Some(Outcome::at(target, true))
        }
        Cmd::Cut => {
            let path = at?;
            let node = doc.remove(path)?;
            Some(Outcome {
                select: after_delete(doc, path),
                structural: true,
                focus_title: false,
                lifted: Some(node),
            })
        }
        Cmd::Paste(node) => {
            let select = match at {
                None => doc.push_root(node),
                Some(p) => doc.insert_sibling_after(p, node)?,
            };
            Some(Outcome::at(select, true))
        }
        Cmd::MoveTo { parent, index } => {
            let from = at?;
            // Dropping a section inside itself would detach the whole subtree.
            if parent.starts_with(from) {
                return None;
            }
            let node = doc.remove(from)?;
            let parent = shift_after_removal(&parent, from)?;
            let siblings_len = doc.children_of(&parent)?.len();
            let mut index = index.min(siblings_len);
            // Removing an earlier sibling from the destination shifts it down.
            if from.len() == parent.len() + 1
                && from[..parent.len()] == parent[..]
                && from[parent.len()] < index
            {
                index -= 1;
            }
            let mut select = parent.clone();
            select.push(index);
            if parent.is_empty() {
                doc.roots.insert(index, node);
            } else {
                doc.get_mut(&parent)?.children.insert(index, node);
            }
            Some(Outcome::at(select, true))
        }
    }
}

/// What a section merges into: the sibling above it, or failing that its
/// parent. Deliberately not "the nearest heading above in the document", which
/// would bury the text in the previous section's last grandchild.
fn merge_target(path: &[usize]) -> Option<NodePath> {
    let (&last, parents) = path.split_last()?;
    if last > 0 {
        let mut p = parents.to_vec();
        p.push(last - 1);
        Some(p)
    } else if !parents.is_empty() {
        Some(parents.to_vec())
    } else {
        None
    }
}

/// Rewrite a path to stay valid after `removed` was taken out of the tree.
fn shift_after_removal(path: &[usize], removed: &[usize]) -> Option<NodePath> {
    if path.starts_with(removed) {
        return None;
    }
    let depth = removed.len() - 1;
    let mut p = path.to_vec();
    if p.len() > depth && p[..depth] == removed[..depth] && p[depth] > removed[depth] {
        p[depth] -= 1;
    }
    Some(p)
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
    fn merge_folds_a_section_into_the_sibling_above_it() {
        let mut d = fixture();
        d.get_mut(&[1]).unwrap().body = "first".into();
        d.get_mut(&[2]).unwrap().body = "second".into();
        let o = apply(&mut d, Some(&[2]), Cmd::MergeIntoPrevious).unwrap();
        assert_eq!(o.select, Some(vec![1]), "selection lands on what it merged into");
        assert_eq!(d.get(&[1]).unwrap().body, "first\n\nsecond");
        assert_eq!(shape(&d), ["0:a", "1:b", "1.0:b1", "1.1:b2"]);
    }

    #[test]
    fn merging_a_first_child_folds_it_into_its_parent() {
        let mut d = fixture();
        d.get_mut(&[1]).unwrap().body = "parent text".into();
        d.get_mut(&[1, 0]).unwrap().body = "child text".into();
        let o = apply(&mut d, Some(&[1, 0]), Cmd::MergeIntoPrevious).unwrap();
        assert_eq!(o.select, Some(vec![1]));
        assert_eq!(d.get(&[1]).unwrap().body, "parent text\n\nchild text");
    }

    #[test]
    fn merge_adopts_the_children_of_what_it_absorbed() {
        let mut d = fixture();
        d.insert_child(&[2], Node::new("c1"));
        apply(&mut d, Some(&[2]), Cmd::MergeIntoPrevious).unwrap();
        assert_eq!(shape(&d), ["0:a", "1:b", "1.0:b1", "1.1:b2", "1.2:c1"]);
    }

    #[test]
    fn merge_is_refused_for_the_very_first_section() {
        let mut d = fixture();
        assert!(apply(&mut d, Some(&[0]), Cmd::MergeIntoPrevious).is_none());
        assert_eq!(shape(&d), shape(&fixture()));
    }

    #[test]
    fn cut_hands_back_the_whole_subtree() {
        let mut d = fixture();
        let o = apply(&mut d, Some(&[1]), Cmd::Cut).unwrap();
        let lifted = o.lifted.expect("cut should hand the section back");
        assert_eq!(lifted.title, "b");
        assert_eq!(lifted.children.len(), 2);
        assert_eq!(shape(&d), ["0:a", "1:c"]);
    }

    #[test]
    fn paste_drops_the_section_in_below_the_selection() {
        let mut d = fixture();
        let o = apply(&mut d, Some(&[0]), Cmd::Paste(Node::new("new"))).unwrap();
        assert_eq!(o.select, Some(vec![1]));
        assert_eq!(shape(&d)[1], "1:new");
    }

    #[test]
    fn paste_with_nothing_selected_appends_a_root() {
        let mut d = fixture();
        let o = apply(&mut d, None, Cmd::Paste(Node::new("new"))).unwrap();
        assert_eq!(o.select, Some(vec![3]));
    }

    #[test]
    fn cut_then_paste_relocates_a_section_intact() {
        let mut d = fixture();
        let lifted = apply(&mut d, Some(&[1]), Cmd::Cut).unwrap().lifted.unwrap();
        apply(&mut d, Some(&[1]), Cmd::Paste(lifted)).unwrap();
        assert_eq!(shape(&d), ["0:a", "1:c", "2:b", "2.0:b1", "2.1:b2"]);
    }

    #[test]
    fn move_to_reparents_a_section() {
        let mut d = fixture();
        let o = apply(&mut d, Some(&[2]), Cmd::MoveTo { parent: vec![1], index: 0 }).unwrap();
        assert_eq!(o.select, Some(vec![1, 0]));
        assert_eq!(shape(&d), ["0:a", "1:b", "1.0:c", "1.1:b1", "1.2:b2"]);
    }

    #[test]
    fn move_within_one_parent_accounts_for_its_own_removal() {
        // Moving "a" to sit between b and c means index 2 of the original list.
        let mut d = fixture();
        let o = apply(&mut d, Some(&[0]), Cmd::MoveTo { parent: vec![], index: 2 }).unwrap();
        assert_eq!(o.select, Some(vec![1]));
        assert_eq!(shape(&d), ["0:b", "0.0:b1", "0.1:b2", "1:a", "2:c"]);
    }

    #[test]
    fn move_into_its_own_descendant_is_refused() {
        let mut d = fixture();
        assert!(apply(&mut d, Some(&[1]), Cmd::MoveTo { parent: vec![1, 0], index: 0 }).is_none());
        assert!(apply(&mut d, Some(&[1]), Cmd::MoveTo { parent: vec![1], index: 0 }).is_none());
        assert_eq!(shape(&d), shape(&fixture()));
    }

    #[test]
    fn move_past_the_end_clamps_instead_of_failing() {
        let mut d = fixture();
        let o = apply(&mut d, Some(&[0]), Cmd::MoveTo { parent: vec![1], index: 99 }).unwrap();
        assert_eq!(o.select, Some(vec![0, 2]), "appended after b1 and b2");
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
