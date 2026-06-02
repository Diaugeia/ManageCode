use std::collections::HashSet;

use crate::app::Row;
use crate::models::SessionInfo;

/// Node in the directory trie used to build the tree view.
#[derive(Default)]
pub struct TreeNode {
    pub path: String,
    pub name: String,
    pub children: std::collections::BTreeMap<String, TreeNode>,
    pub sessions: Vec<usize>,
}

impl TreeNode {
    /// `(total, alive)` session counts for this node's whole subtree.
    fn counts(&self, sessions: &[SessionInfo]) -> (usize, usize) {
        let mut total = self.sessions.len();
        let mut alive = self
            .sessions
            .iter()
            .filter(|&&i| sessions[i].is_alive)
            .count();
        for c in self.children.values() {
            let (t, a) = c.counts(sessions);
            total += t;
            alive += a;
        }
        (total, alive)
    }
}

/// Non-empty path components of an absolute path (`/a//b/` → `a`, `b`).
pub fn path_segments(p: &str) -> impl Iterator<Item = &str> {
    p.split('/').filter(|c| !c.is_empty())
}

/// Follow a single-child, session-less chain (path compression) to the node
/// that actually renders as one tree row.
fn compress(node: &TreeNode) -> &TreeNode {
    let mut cur = node;
    while cur.sessions.is_empty() && cur.children.len() == 1 {
        cur = cur.children.values().next().unwrap();
    }
    cur
}

/// Is `anc` the same directory as `path`, or an ancestor of it?
pub fn is_ancestor_or_eq(anc: &str, path: &str) -> bool {
    path == anc || path.starts_with(&format!("{anc}/"))
}

/// Is `cwd` in scope for the tree view, given the configured root? In any
/// non-tree context or with an empty root, everything is in scope.
fn in_tree_scope(is_tree_view: bool, tree_root: &str, cwd: &str) -> bool {
    if !is_tree_view || tree_root.is_empty() {
        return true;
    }
    is_ancestor_or_eq(tree_root, cwd)
}

/// Build the directory trie from the in-scope filtered sessions.
pub fn build_tree(
    sessions: &[SessionInfo],
    filtered: &[usize],
    is_tree_view: bool,
    tree_root: &str,
) -> TreeNode {
    let mut root = TreeNode::default();
    for &idx in filtered {
        let cwd = sessions[idx].cwd.clone();
        if !in_tree_scope(is_tree_view, tree_root, &cwd) {
            continue;
        }
        let mut node = &mut root;
        let mut path = String::new();
        for c in path_segments(&cwd) {
            path.push('/');
            path.push_str(c);
            node = node
                .children
                .entry(c.to_string())
                .or_insert_with(|| TreeNode {
                    path: path.clone(),
                    name: c.to_string(),
                    ..TreeNode::default()
                });
        }
        node.sessions.push(idx);
    }
    root
}

/// Build the full set of tree rows for the in-scope filtered sessions.
pub fn tree_rows(
    sessions: &[SessionInfo],
    filtered: &[usize],
    collapsed_groups: &HashSet<String>,
    is_tree_view: bool,
    tree_root: &str,
) -> Vec<Row> {
    let root = build_tree(sessions, filtered, is_tree_view, tree_root);
    let mut out: Vec<Row> = Vec::new();
    for child in root.children.values() {
        emit_tree(sessions, collapsed_groups, child, 0, &mut out);
    }
    out
}

fn emit_tree(
    sessions: &[SessionInfo],
    collapsed_groups: &HashSet<String>,
    node: &TreeNode,
    depth: usize,
    out: &mut Vec<Row>,
) {
    // Path-compress a chain of single-child, session-less dirs into one row
    // so `a/b/c` shows as one node when nothing branches.
    let mut name = node.name.clone();
    let mut cur = node;
    while cur.sessions.is_empty() && cur.children.len() == 1 {
        let child = cur.children.values().next().unwrap();
        name.push('/');
        name.push_str(&child.name);
        cur = child;
    }

    let (total, alive) = cur.counts(sessions);
    let collapsed = collapsed_groups.contains(&cur.path);
    out.push(Row::Tree {
        path: cur.path.clone(),
        name,
        depth,
        total,
        alive,
        collapsed,
    });
    if collapsed {
        return;
    }
    for k in cur.children.values() {
        emit_tree(sessions, collapsed_groups, k, depth + 1, out);
    }
    for &idx in &cur.sessions {
        out.push(Row::Session { idx, depth });
    }
}

/// The compressed paths of the immediate child nodes of the tree node
/// rendered at `parent_path`. Used to collapse children one level on expand
/// (file-explorer behavior). Empty if the node has no child directories.
pub fn immediate_child_paths(
    sessions: &[SessionInfo],
    filtered: &[usize],
    is_tree_view: bool,
    tree_root: &str,
    parent_path: &str,
) -> Vec<String> {
    fn find<'a>(node: &'a TreeNode, target: &str) -> Option<&'a TreeNode> {
        let cur = compress(node);
        if cur.path == target {
            return Some(cur);
        }
        cur.children.values().find_map(|c| find(c, target))
    }
    let root = build_tree(sessions, filtered, is_tree_view, tree_root);
    root.children
        .values()
        .find_map(|top| find(top, parent_path))
        .map(|node| {
            node.children
                .values()
                .map(|c| compress(c).path.clone())
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::{compress, is_ancestor_or_eq, TreeNode};

    #[test]
    fn ancestor_matching() {
        assert!(is_ancestor_or_eq("/a/b", "/a/b")); // equal
        assert!(is_ancestor_or_eq("/a/b", "/a/b/c")); // ancestor
        assert!(!is_ancestor_or_eq("/a/b", "/a/bc")); // not a path boundary
        assert!(!is_ancestor_or_eq("/a/b", "/a")); // parent, not ancestor
    }

    /// Build a node at `path` with the given child names (each a leaf with one
    /// session so compression stops there).
    fn node(path: &str, children: &[&str]) -> TreeNode {
        let mut n = TreeNode {
            path: path.to_string(),
            ..TreeNode::default()
        };
        for c in children {
            let cp = format!("{path}/{c}");
            n.children.insert(
                c.to_string(),
                TreeNode {
                    path: cp,
                    name: c.to_string(),
                    sessions: vec![0], // a session => not further compressible
                    ..TreeNode::default()
                },
            );
        }
        n
    }

    #[test]
    fn compress_collapses_single_child_chain() {
        // /a -> /a/b -> /a/b/c (single, session-less chain) compresses to /a/b/c.
        let mut a = node("/a", &[]);
        let mut b = node("/a/b", &[]);
        let c = node("/a/b/c", &["x", "y"]); // branches: chain stops here
        b.children.insert("c".into(), c);
        a.children.insert("b".into(), b);
        assert_eq!(compress(&a).path, "/a/b/c");

        // A node that itself holds a session does not compress past itself.
        let withseed = TreeNode {
            path: "/p".into(),
            sessions: vec![0],
            ..TreeNode::default()
        };
        assert_eq!(compress(&withseed).path, "/p");
    }
}
