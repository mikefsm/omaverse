//! The outline document model: a tree of nodes, each with a title and a body.
//!
//! Positions are index paths (`[0, 2, 1]` = third child of first root's ... etc).
//! Every mutating operation returns the node's new path so the UI can follow it.

pub type NodePath = Vec<usize>;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Node {
    /// Single line, shown in the outline pane.
    pub title: String,
    /// Free text, shown in the body pane. No trailing blank lines.
    pub body: String,
    pub children: Vec<Node>,
}

impl Node {
    pub fn new(title: impl Into<String>) -> Self {
        Node { title: title.into(), body: String::new(), children: Vec::new() }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Document {
    /// From a leading `# ` heading. Falls back to the filename in the UI.
    pub title: Option<String>,
    /// Anything between the title and the first bullet. Preserved so we never
    /// silently eat content we didn't expect.
    pub preamble: String,
    pub roots: Vec<Node>,
}

/// Render an index path as the stable-ish string key used in the sidecar state.
pub fn path_key(path: &[usize]) -> String {
    path.iter().map(|i| i.to_string()).collect::<Vec<_>>().join(".")
}

pub fn parse_path_key(key: &str) -> Option<NodePath> {
    if key.is_empty() {
        return None;
    }
    key.split('.').map(|p| p.parse::<usize>().ok()).collect()
}

impl Document {
    pub fn get(&self, path: &[usize]) -> Option<&Node> {
        let (last, parents) = path.split_last()?;
        let mut vec = &self.roots;
        for &i in parents {
            vec = &vec.get(i)?.children;
        }
        vec.get(*last)
    }

    pub fn get_mut(&mut self, path: &[usize]) -> Option<&mut Node> {
        let (last, parents) = path.split_last()?;
        let mut vec = &mut self.roots;
        for &i in parents {
            vec = &mut vec.get_mut(i)?.children;
        }
        vec.get_mut(*last)
    }

    /// The sibling vector containing `path`, plus the node's index within it.
    fn container_mut(&mut self, path: &[usize]) -> Option<(&mut Vec<Node>, usize)> {
        let (last, parents) = path.split_last()?;
        let mut vec = &mut self.roots;
        for &i in parents {
            vec = &mut vec.get_mut(i)?.children;
        }
        if *last >= vec.len() {
            return None;
        }
        Some((vec, *last))
    }

    /// Sibling vector at a given parent path (empty path = the roots).
    pub fn children_of(&self, parent: &[usize]) -> Option<&Vec<Node>> {
        if parent.is_empty() {
            return Some(&self.roots);
        }
        Some(&self.get(parent)?.children)
    }

    pub fn is_empty(&self) -> bool {
        self.roots.is_empty()
    }

    // ---- mutations ---------------------------------------------------------

    pub fn push_root(&mut self, node: Node) -> NodePath {
        self.roots.push(node);
        vec![self.roots.len() - 1]
    }

    pub fn insert_sibling_after(&mut self, path: &[usize], node: Node) -> Option<NodePath> {
        let (vec, idx) = self.container_mut(path)?;
        vec.insert(idx + 1, node);
        let mut p = path.to_vec();
        *p.last_mut()? = idx + 1;
        Some(p)
    }

    pub fn insert_child(&mut self, path: &[usize], node: Node) -> Option<NodePath> {
        let parent = self.get_mut(path)?;
        parent.children.push(node);
        let mut p = path.to_vec();
        p.push(self.get(path)?.children.len() - 1);
        Some(p)
    }

    pub fn remove(&mut self, path: &[usize]) -> Option<Node> {
        let (vec, idx) = self.container_mut(path)?;
        Some(vec.remove(idx))
    }

    /// Make the node a child of its previous sibling. Needs a previous sibling.
    pub fn indent(&mut self, path: &[usize]) -> Option<NodePath> {
        let (vec, idx) = self.container_mut(path)?;
        if idx == 0 {
            return None;
        }
        let node = vec.remove(idx);
        let prev = &mut vec[idx - 1];
        prev.children.push(node);
        let new_last = prev.children.len() - 1;
        let mut p = path.to_vec();
        *p.last_mut()? = idx - 1;
        p.push(new_last);
        Some(p)
    }

    /// Make the node the next sibling of its parent. Needs depth >= 2.
    pub fn outdent(&mut self, path: &[usize]) -> Option<NodePath> {
        if path.len() < 2 {
            return None;
        }
        let (vec, idx) = self.container_mut(path)?;
        let node = vec.remove(idx);
        let parent_path = &path[..path.len() - 1];
        let (gvec, pidx) = match self.container_mut(parent_path) {
            Some(v) => v,
            None => return None, // unreachable: depth >= 2 guarantees a parent
        };
        gvec.insert(pidx + 1, node);
        let mut p = parent_path.to_vec();
        *p.last_mut()? = pidx + 1;
        Some(p)
    }

    pub fn move_up(&mut self, path: &[usize]) -> Option<NodePath> {
        let (vec, idx) = self.container_mut(path)?;
        if idx == 0 {
            return None;
        }
        vec.swap(idx, idx - 1);
        let mut p = path.to_vec();
        *p.last_mut()? = idx - 1;
        Some(p)
    }

    pub fn move_down(&mut self, path: &[usize]) -> Option<NodePath> {
        let (vec, idx) = self.container_mut(path)?;
        if idx + 1 >= vec.len() {
            return None;
        }
        vec.swap(idx, idx + 1);
        let mut p = path.to_vec();
        *p.last_mut()? = idx + 1;
        Some(p)
    }

    /// Depth-first walk yielding (path, node). Used by the test suites to
    /// assert tree shape; kept here rather than duplicated in each of them.
    #[allow(dead_code)]
    pub fn walk(&self) -> Vec<(NodePath, &Node)> {
        let mut out = Vec::new();
        fn rec<'a>(nodes: &'a [Node], prefix: &mut NodePath, out: &mut Vec<(NodePath, &'a Node)>) {
            for (i, n) in nodes.iter().enumerate() {
                prefix.push(i);
                out.push((prefix.clone(), n));
                rec(&n.children, prefix, out);
                prefix.pop();
            }
        }
        rec(&self.roots, &mut Vec::new(), &mut out);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    fn titles(d: &Document) -> Vec<String> {
        d.walk().iter().map(|(p, n)| format!("{}:{}", path_key(p), n.title)).collect()
    }

    #[test]
    fn get_and_walk() {
        let d = fixture();
        assert_eq!(d.get(&[1]).unwrap().title, "b");
        assert_eq!(d.get(&[1, 1]).unwrap().title, "b2");
        assert!(d.get(&[9]).is_none());
        assert!(d.get(&[1, 1, 0]).is_none());
        assert_eq!(titles(&d), ["0:a", "1:b", "1.0:b1", "1.1:b2", "2:c"]);
    }

    #[test]
    fn indent_makes_child_of_previous_sibling() {
        let mut d = fixture();
        let np = d.indent(&[2]).unwrap(); // c under b
        assert_eq!(np, vec![1, 2]);
        assert_eq!(titles(&d), ["0:a", "1:b", "1.0:b1", "1.1:b2", "1.2:c"]);
    }

    #[test]
    fn indent_refuses_first_sibling() {
        let mut d = fixture();
        assert!(d.indent(&[0]).is_none());
        assert!(d.indent(&[1, 0]).is_none());
        assert_eq!(titles(&d), titles(&fixture()));
    }

    #[test]
    fn outdent_makes_next_sibling_of_parent() {
        let mut d = fixture();
        let np = d.outdent(&[1, 0]).unwrap(); // b1 out to root, after b
        assert_eq!(np, vec![2]);
        assert_eq!(titles(&d), ["0:a", "1:b", "1.0:b2", "2:b1", "3:c"]);
    }

    #[test]
    fn outdent_refuses_at_root() {
        let mut d = fixture();
        assert!(d.outdent(&[0]).is_none());
        assert_eq!(titles(&d), titles(&fixture()));
    }

    #[test]
    fn indent_then_outdent_round_trips() {
        let mut d = fixture();
        let before = titles(&d);
        let p = d.indent(&[2]).unwrap();
        let p = d.outdent(&p).unwrap();
        assert_eq!(p, vec![2]);
        assert_eq!(titles(&d), before);
    }

    #[test]
    fn indent_carries_subtree_and_body() {
        let mut d = fixture();
        d.get_mut(&[2]).unwrap().body = "kept".into();
        d.insert_child(&[2], Node::new("c1"));
        let np = d.indent(&[2]).unwrap();
        assert_eq!(d.get(&np).unwrap().body, "kept");
        assert_eq!(d.get(&[1, 2, 0]).unwrap().title, "c1");
    }

    #[test]
    fn move_up_and_down() {
        let mut d = fixture();
        assert_eq!(d.move_down(&[0]).unwrap(), vec![1]);
        assert_eq!(d.walk()[0].1.title, "b");
        assert_eq!(d.move_up(&[1]).unwrap(), vec![0]);
        assert_eq!(d.walk()[0].1.title, "a");
        assert!(d.move_up(&[0]).is_none());
        assert!(d.move_down(&[2]).is_none());
    }

    #[test]
    fn insert_sibling_after_shifts_the_rest() {
        let mut d = fixture();
        let np = d.insert_sibling_after(&[0], Node::new("a2")).unwrap();
        assert_eq!(np, vec![1]);
        assert_eq!(titles(&d), ["0:a", "1:a2", "2:b", "2.0:b1", "2.1:b2", "3:c"]);
    }

    #[test]
    fn remove_returns_the_subtree() {
        let mut d = fixture();
        let n = d.remove(&[1]).unwrap();
        assert_eq!(n.title, "b");
        assert_eq!(n.children.len(), 2);
        assert_eq!(titles(&d), ["0:a", "1:c"]);
    }

    #[test]
    fn path_key_round_trip() {
        assert_eq!(path_key(&[0, 2, 1]), "0.2.1");
        assert_eq!(parse_path_key("0.2.1").unwrap(), vec![0, 2, 1]);
        assert!(parse_path_key("").is_none());
        assert!(parse_path_key("0.x").is_none());
    }
}
