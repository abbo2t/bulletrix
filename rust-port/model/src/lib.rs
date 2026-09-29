//! Arena-based port of `bulletrix/models.py`.
//!
//! Python's `Node` holds a real `parent: Optional[Node]` object reference,
//! which the GC keeps alive as long as anything points to it. Rust has no
//! GC, and a tree of `Rc<RefCell<Node>>` + `Weak` parents turns every one
//! of these mutations (indent, outdent, merge) into a borrow-checker fight.
//!
//! Instead, all nodes live in one `SlotMap` arena and are referred to by
//! `NodeId` (a small `Copy` key), the same way you'd reference rows in a
//! database. Structural edits become "look up by key, mutate, look up
//! another key, mutate" - no lifetimes, no `Rc`, and `slotmap`'s generational
//! keys mean a stale `NodeId` (the Rust analog of a dangling Python
//! reference) panics loudly at the point of misuse instead of silently
//! reading garbage.

use fancy_regex::Regex;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use slotmap::{new_key_type, SlotMap};
use std::time::{SystemTime, UNIX_EPOCH};

pub mod storage;

new_key_type! { pub struct NodeId; }

// Python: TAG_RE = re.compile(r"(?<!\w)([#@][\w][\w-]*)")
// `regex` (Rust's default engine) has no lookbehind support - it's a
// linear-time DFA engine, not backtracking. `fancy-regex` adds backtracking
// for exactly this kind of lookaround, at a perf cost that doesn't matter
// here (short strings, not a hot loop).
pub static TAG_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?<!\w)([#@]\w[\w-]*)").unwrap());

fn now_secs() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs_f64()
}

#[derive(Debug, Clone)]
pub struct Node {
    pub text: String,
    pub note: String,
    pub completed: bool,
    pub collapsed: bool,
    pub uuid: String,
    pub created: f64,
    pub modified: f64,
    pub parent: Option<NodeId>,
    pub children: Vec<NodeId>,
    /// Set by `Outline::merge_backward` to tell the caller where the
    /// cursor should land. Python bolts this on dynamically as
    /// `node._merge_cursor` with a `# type: ignore`; here it's just a
    /// field, which is the whole story of this port in miniature.
    pub merge_cursor: Option<usize>,
}

impl Node {
    fn new() -> Self {
        let t = now_secs();
        Node {
            text: String::new(),
            note: String::new(),
            completed: false,
            collapsed: false,
            uuid: uuid::Uuid::new_v4().simple().to_string(),
            created: t,
            modified: t,
            parent: None,
            children: Vec::new(),
            merge_cursor: None,
        }
    }

    fn with_text(text: impl Into<String>) -> Self {
        Node {
            text: text.into(),
            ..Node::new()
        }
    }

    pub fn touch(&mut self) {
        self.modified = now_secs();
    }

    pub fn tags(&self) -> Vec<String> {
        let extract = |s: &str| -> Vec<String> {
            TAG_RE
                .captures_iter(s)
                .filter_map(|c| c.ok())
                .filter_map(|c| c.get(1).map(|m| m.as_str().to_string()))
                .collect()
        };
        let mut out = extract(&self.text);
        out.extend(extract(&self.note));
        out
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Row {
    pub node: NodeId,
    pub depth: usize,
    pub is_header: bool,
    pub has_children: bool,
    pub visible_children: bool,
}

#[derive(Clone)]
pub struct Outline {
    arena: SlotMap<NodeId, Node>,
    root: NodeId,
    zoom_stack: Vec<NodeId>,
    pub hide_completed: bool,
    pub selected_id: Option<NodeId>,
    pub cursor: usize,
}

impl Outline {
    pub fn new() -> Self {
        let mut arena = SlotMap::with_key();
        let root = arena.insert(Node::new());
        let first = arena.insert(Node::new());
        arena[first].parent = Some(root);
        arena[root].children.push(first);
        Outline {
            arena,
            root,
            zoom_stack: vec![root],
            hide_completed: false,
            selected_id: None,
            cursor: 0,
        }
    }

    pub fn get(&self, id: NodeId) -> &Node {
        &self.arena[id]
    }

    pub fn get_mut(&mut self, id: NodeId) -> &mut Node {
        &mut self.arena[id]
    }

    pub fn root(&self) -> NodeId {
        self.root
    }

    /// Inserts a fresh node into the arena and returns its id. Structural
    /// methods below (`insert_sibling_after`, `add_first_child`, ...) then
    /// splice it into the tree - mirrors Python's two-step
    /// `Node(...)` + `outline.insert_sibling_after(row.node, new_node)`.
    pub fn create_node(&mut self, text: impl Into<String>) -> NodeId {
        self.arena.insert(Node::with_text(text))
    }

    pub fn find_by_uuid(&self, uuid: &str) -> Option<NodeId> {
        // SlotMap gives you "walk every live node" for free via `.iter()` -
        // no recursion needed, unlike Python's `find()` which has to walk
        // `children` by hand because a plain object graph has no such index.
        self.arena
            .iter()
            .find(|(_, n)| n.uuid == uuid)
            .map(|(k, _)| k)
    }

    // -- zoom ----------------------------------------------------------
    pub fn zoom_root(&self) -> NodeId {
        *self.zoom_stack.last().expect("zoom_stack is never empty")
    }

    pub fn zoom_in(&mut self, id: NodeId) {
        self.zoom_stack.push(id);
    }

    pub fn zoom_out(&mut self) -> Option<NodeId> {
        if self.zoom_stack.len() <= 1 {
            return None;
        }
        self.zoom_stack.pop()
    }

    pub fn breadcrumb(&self) -> &[NodeId] {
        &self.zoom_stack
    }

    // -- flatten for display --------------------------------------------
    pub fn flatten(&self) -> Vec<Row> {
        let mut rows = Vec::new();
        let root = self.zoom_root();
        let is_true_root = root == self.root;
        if !is_true_root {
            let has_kids = !self.arena[root].children.is_empty();
            rows.push(Row {
                node: root,
                depth: 0,
                is_header: true,
                has_children: has_kids,
                visible_children: has_kids && !self.arena[root].collapsed,
            });
        }
        if is_true_root {
            self.flatten_walk(root, 0, &mut rows);
        } else if !self.arena[root].collapsed {
            self.flatten_walk(root, 1, &mut rows);
        }
        rows
    }

    fn flatten_walk(&self, node: NodeId, depth: usize, rows: &mut Vec<Row>) {
        for &child in &self.arena[node].children {
            if self.hide_completed && self.arena[child].completed {
                continue;
            }
            let has_kids = !self.arena[child].children.is_empty();
            let expanded = has_kids && !self.arena[child].collapsed;
            rows.push(Row {
                node: child,
                depth,
                is_header: false,
                has_children: has_kids,
                visible_children: expanded,
            });
            if expanded {
                self.flatten_walk(child, depth + 1, rows);
            }
        }
    }

    // -- lookups ---------------------------------------------------------
    pub fn index_in_parent(&self, id: NodeId) -> usize {
        let parent = self.arena[id].parent.expect("node has no parent");
        self.arena[parent]
            .children
            .iter()
            .position(|&c| c == id)
            .expect("node missing from parent's children")
    }

    // -- structural mutations --------------------------------------------
    pub fn insert_sibling_after(&mut self, id: NodeId, new_id: NodeId) {
        match self.arena[id].parent {
            None => {
                // node is a zoom root with no real parent in this stack;
                // treat as child (matches Python's fallback).
                self.arena[id].children.insert(0, new_id);
                self.arena[new_id].parent = Some(id);
            }
            Some(parent) => {
                let idx = self.index_in_parent(id);
                self.arena[new_id].parent = Some(parent);
                self.arena[parent].children.insert(idx + 1, new_id);
            }
        }
    }

    pub fn insert_sibling_before(&mut self, id: NodeId, new_id: NodeId) {
        match self.arena[id].parent {
            None => {
                self.arena[id].children.insert(0, new_id);
                self.arena[new_id].parent = Some(id);
            }
            Some(parent) => {
                let idx = self.index_in_parent(id);
                self.arena[new_id].parent = Some(parent);
                self.arena[parent].children.insert(idx, new_id);
            }
        }
    }

    pub fn add_first_child(&mut self, id: NodeId, new_id: NodeId) {
        self.arena[new_id].parent = Some(id);
        self.arena[id].children.insert(0, new_id);
    }

    pub fn append_child(&mut self, parent: NodeId, child: NodeId) {
        self.arena[child].parent = Some(parent);
        self.arena[parent].children.push(child);
    }

    pub fn remove(&mut self, id: NodeId) {
        if let Some(parent) = self.arena[id].parent {
            self.arena[parent].children.retain(|&c| c != id);
            self.arena[id].parent = None;
        }
    }

    /// Detaches `id` and frees it along with all of its descendants.
    pub fn delete(&mut self, id: NodeId) {
        self.remove(id);
        let mut stack = vec![id];
        while let Some(n) = stack.pop() {
            if let Some(node) = self.arena.remove(n) {
                stack.extend(node.children);
            }
        }
    }

    /// Joins the next sibling's text and children onto `id` (forward-delete
    /// at end of line). Returns false if there is no next sibling.
    pub fn merge_forward(&mut self, id: NodeId) -> bool {
        let Some(parent) = self.arena[id].parent else {
            return false;
        };
        let idx = self.index_in_parent(id);
        let Some(&next) = self.arena[parent].children.get(idx + 1) else {
            return false;
        };
        let text = std::mem::take(&mut self.arena[next].text);
        self.arena[id].text.push_str(&text);
        let moved = std::mem::take(&mut self.arena[next].children);
        for &c in &moved {
            self.arena[c].parent = Some(id);
        }
        self.arena[id].children.extend(moved);
        self.arena[parent].children.remove(idx + 1);
        self.arena.remove(next);
        true
    }

    pub fn indent(&mut self, id: NodeId) -> bool {
        let Some(parent) = self.arena[id].parent else {
            return false;
        };
        let idx = self.index_in_parent(id);
        if idx == 0 {
            return false;
        }
        let prev_sibling = self.arena[parent].children[idx - 1];
        self.arena[parent].children.remove(idx);
        self.arena[id].parent = Some(prev_sibling);
        self.arena[prev_sibling].children.push(id);
        self.arena[prev_sibling].collapsed = false;
        true
    }

    pub fn outdent(&mut self, id: NodeId) -> bool {
        let Some(parent) = self.arena[id].parent else {
            return false;
        };
        let Some(grandparent) = self.arena[parent].parent else {
            return false;
        };
        let idx = self.index_in_parent(id);
        self.arena[parent].children.remove(idx);
        self.arena[id].parent = Some(grandparent);
        let gp_idx = self.index_in_parent(parent);
        self.arena[grandparent].children.insert(gp_idx + 1, id);
        true
    }

    pub fn move_up(&mut self, id: NodeId) -> bool {
        let Some(parent) = self.arena[id].parent else {
            return false;
        };
        let idx = self.index_in_parent(id);
        if idx == 0 {
            return false;
        }
        self.arena[parent].children.swap(idx - 1, idx);
        true
    }

    pub fn move_down(&mut self, id: NodeId) -> bool {
        let Some(parent) = self.arena[id].parent else {
            return false;
        };
        let idx = self.index_in_parent(id);
        if idx + 1 >= self.arena[parent].children.len() {
            return false;
        }
        self.arena[parent].children.swap(idx, idx + 1);
        true
    }

    /// Merges `id` into its previous sibling (or its parent, if `id` is
    /// the first child). Returns the node that now holds focus; that
    /// node's `merge_cursor` is set to the char offset the cursor should
    /// land at. The merged-away node is freed from the arena - Python
    /// just lets it become unreachable and leaves it for the GC.
    pub fn merge_backward(&mut self, id: NodeId) -> Option<NodeId> {
        let parent = self.arena[id].parent?;
        let idx = self.index_in_parent(id);

        let target = if idx > 0 {
            let target = self.arena[parent].children[idx - 1];
            let offset = self.arena[target].text.chars().count();
            let moved_text = self.arena[id].text.clone();
            self.arena[target].text.push_str(&moved_text);

            let moved_children = std::mem::take(&mut self.arena[id].children);
            for &c in &moved_children {
                self.arena[c].parent = Some(target);
            }
            let mut new_children = moved_children;
            new_children.extend(self.arena[target].children.iter().copied());
            self.arena[target].children = new_children;

            self.arena[parent].children.remove(idx);
            self.arena[target].merge_cursor = Some(offset);
            target
        } else {
            // Refuse to merge into the true home root (idx == 0 and no
            // grandparent means `parent` IS outline.root).
            self.arena[parent].parent?;

            let target = parent;
            let offset = self.arena[target].text.chars().count();
            let moved_text = self.arena[id].text.clone();
            self.arena[target].text.push_str(&moved_text);
            self.arena[parent].children.remove(idx);

            let moved_children = std::mem::take(&mut self.arena[id].children);
            for &c in moved_children.iter().rev() {
                self.arena[c].parent = Some(target);
                self.arena[target].children.insert(0, c);
            }
            self.arena[target].merge_cursor = Some(offset);
            target
        };

        self.arena.remove(id);
        Some(target)
    }

    pub fn toggle_complete(&mut self, id: NodeId) {
        self.arena[id].completed = !self.arena[id].completed;
        self.arena[id].touch();
    }

    pub fn toggle_collapsed(&mut self, id: NodeId) {
        self.arena[id].collapsed = !self.arena[id].collapsed;
    }

    /// Expands every ancestor of `id` and zooms back out to the top, so the
    /// node is on screen.
    pub fn reveal(&mut self, id: NodeId) {
        let mut ancestor = self.arena[id].parent;
        while let Some(a) = ancestor {
            self.arena[a].collapsed = false;
            ancestor = self.arena[a].parent;
        }
        self.zoom_stack.truncate(1);
    }

    /// Whether hide-completed keeps `id` off screen (it or an ancestor is done).
    pub fn hidden_by_completed(&self, id: NodeId) -> bool {
        self.hide_completed
            && std::iter::successors(Some(id), |&n| self.arena[n].parent).any(|n| self.arena[n].completed)
    }

    pub fn search(&self, query: &str) -> Vec<NodeId> {
        let query = query.to_lowercase();
        let query = query.trim();
        if query.is_empty() {
            return Vec::new();
        }
        let mut results = Vec::new();
        self.search_walk(self.root, query, &mut results);
        results
    }

    fn search_walk(&self, node: NodeId, query: &str, results: &mut Vec<NodeId>) {
        for &child in &self.arena[node].children {
            let haystack =
                format!("{} {}", self.arena[child].text, self.arena[child].note).to_lowercase();
            if haystack.contains(query) {
                results.push(child);
            }
            self.search_walk(child, query, results);
        }
    }

    // -- persistence -------------------------------------------------------
    /// Pretty-printed JSON in the same shape and key order as Python's
    /// `json.dump(outline.to_dict(), f, indent=2)`.
    pub fn to_json_string(&self) -> String {
        let dto = OutlineDto {
            root: self.node_to_dto(self.root),
            hide_completed: self.hide_completed,
            zoom_stack: self.zoom_stack.iter().map(|&id| self.arena[id].uuid.clone()).collect(),
            selected_id: self.selected_id.map(|id| self.arena[id].uuid.clone()),
            cursor: self.cursor,
        };
        serde_json::to_string_pretty(&dto).expect("outline DTO always serializes")
    }

    fn node_to_dto(&self, id: NodeId) -> NodeDto {
        let n = &self.arena[id];
        NodeDto {
            id: n.uuid.clone(),
            text: n.text.clone(),
            note: n.note.clone(),
            completed: n.completed,
            collapsed: n.collapsed,
            created: n.created,
            modified: n.modified,
            children: n.children.iter().map(|&c| self.node_to_dto(c)).collect(),
        }
    }

    pub fn from_json_str(json: &str) -> serde_json::Result<Self> {
        let dto: OutlineDto = serde_json::from_str(json)?;

        let mut arena = SlotMap::with_key();
        let root = Self::dto_to_node(&mut arena, &dto.root, None);
        // Like Python's Outline.__init__: an outline always has something to select.
        if arena[root].children.is_empty() {
            let first = arena.insert(Node::new());
            arena[first].parent = Some(root);
            arena[root].children.push(first);
        }

        let mut outline = Outline {
            arena,
            root,
            zoom_stack: vec![root],
            hide_completed: dto.hide_completed,
            selected_id: None,
            cursor: dto.cursor,
        };

        // Mirrors Python's from_dict: walk the persisted zoom stack,
        // stopping at the first id that no longer resolves.
        for uuid in dto.zoom_stack.iter().skip(1) {
            match outline.find_by_uuid(uuid) {
                Some(id) => outline.zoom_stack.push(id),
                None => break,
            }
        }
        // Unlike the zoom stack, a stale selected_id is passed through as
        // None rather than causing an error - matches Python.
        outline.selected_id = dto.selected_id.and_then(|u| outline.find_by_uuid(&u));
        Ok(outline)
    }

    fn dto_to_node(arena: &mut SlotMap<NodeId, Node>, dto: &NodeDto, parent: Option<NodeId>) -> NodeId {
        let id = arena.insert(Node {
            text: dto.text.clone(),
            note: dto.note.clone(),
            completed: dto.completed,
            collapsed: dto.collapsed,
            uuid: if dto.id.is_empty() {
                uuid::Uuid::new_v4().simple().to_string()
            } else {
                dto.id.clone()
            },
            created: dto.created,
            modified: dto.modified,
            parent,
            children: Vec::new(),
            merge_cursor: None,
        });
        let children: Vec<NodeId> = dto
            .children
            .iter()
            .map(|c| Self::dto_to_node(arena, c, Some(id)))
            .collect();
        arena[id].children = children;
        id
    }
}

impl Default for Outline {
    fn default() -> Self {
        Self::new()
    }
}

// Nested tree-shaped DTO for JSON - the arena is flat and id-based, but
// the on-disk format (and Python's json.dumps(outline.to_dict())) is a
// nested tree, so this is the translation boundary between the two shapes.
// Missing fields get the same defaults as Python's `Node.from_dict`.
#[derive(Serialize, Deserialize)]
struct NodeDto {
    #[serde(default)]
    id: String,
    #[serde(default)]
    text: String,
    #[serde(default)]
    note: String,
    #[serde(default)]
    completed: bool,
    #[serde(default)]
    collapsed: bool,
    #[serde(default = "now_secs")]
    created: f64,
    #[serde(default = "now_secs")]
    modified: f64,
    #[serde(default)]
    children: Vec<NodeDto>,
}

#[derive(Serialize, Deserialize)]
struct OutlineDto {
    root: NodeDto,
    #[serde(default)]
    hide_completed: bool,
    #[serde(default)]
    zoom_stack: Vec<String>,
    #[serde(default)]
    selected_id: Option<String>,
    #[serde(default)]
    cursor: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Mirrors `make_outline()` in tests/test_models.py: root "Home" with
    /// children A, B, C.
    fn make_outline() -> (Outline, NodeId, NodeId, NodeId, NodeId) {
        let mut outline = Outline::new();
        // Outline::new() seeds an empty first child; discard it like the
        // Python test does by overwriting root.children wholesale.
        outline.arena[outline.root].children.clear();

        let a = outline.create_node("A");
        let b = outline.create_node("B");
        let c = outline.create_node("C");
        for &n in &[a, b, c] {
            outline.arena[n].parent = Some(outline.root);
        }
        let root = outline.root;
        outline.arena[root].children = vec![a, b, c];
        outline.arena[root].text = "Home".into();

        (outline, a, b, c, root)
    }

    #[test]
    fn indent_makes_previous_sibling_the_parent() {
        let (mut outline, a, b, c, root) = make_outline();
        assert!(outline.indent(b));
        assert_eq!(outline.get(b).parent, Some(a));
        assert_eq!(outline.get(a).children, vec![b]);
        assert_eq!(outline.get(root).children, vec![a, c]);
    }

    #[test]
    fn indent_first_child_is_noop() {
        let (mut outline, a, ..) = make_outline();
        assert!(!outline.indent(a));
    }

    #[test]
    fn move_up_and_down_swap_siblings() {
        let (mut outline, a, b, c, root) = make_outline();
        assert!(outline.move_down(a));
        assert_eq!(outline.get(root).children, vec![b, a, c]);
        assert!(outline.move_up(a));
        assert_eq!(outline.get(root).children, vec![a, b, c]);
    }

    #[test]
    fn merge_backward_into_previous_sibling_appends_text_and_children() {
        let (mut outline, _a, b, c, root) = make_outline();
        let grandchild = outline.create_node("child-of-c");
        outline.arena[grandchild].parent = Some(c);
        outline.arena[c].children = vec![grandchild];

        let result = outline.merge_backward(c).unwrap();
        assert_eq!(result, b);
        assert_eq!(outline.get(b).text, "BC");
        assert_eq!(outline.get(b).merge_cursor, Some(1));
        assert!(outline.get(b).children.contains(&grandchild));
        assert_eq!(outline.get(grandchild).parent, Some(b));
        assert_eq!(outline.get(root).children, vec![outline.get(root).children[0], b]);
    }

    #[test]
    fn merge_backward_first_child_merges_into_parent() {
        let (mut outline, a, ..) = make_outline();
        outline.zoom_in(a);
        let child = outline.create_node("child");
        outline.arena[child].parent = Some(a);
        outline.arena[a].children = vec![child];

        let result = outline.merge_backward(child).unwrap();
        assert_eq!(result, a);
        assert_eq!(outline.get(a).text, "Achild");
        assert!(outline.get(a).children.is_empty());
    }

    #[test]
    fn merge_backward_refuses_to_merge_into_true_home_root() {
        let mut outline = Outline::new();
        let only_child = outline.get(outline.root).children[0];
        assert_eq!(outline.merge_backward(only_child), None);
    }

    #[test]
    fn stale_node_id_panics_instead_of_reading_garbage() {
        // The Rust analog of a dangling Python object reference: after
        // merge_backward frees `c`, using its NodeId is a loud panic, not
        // a silent read of a detached-but-still-alive object.
        let (mut outline, _a, b, c, _root) = make_outline();
        let merged = outline.merge_backward(c).unwrap();
        assert_eq!(merged, b);

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| outline.get(c)));
        assert!(result.is_err());
    }

    #[test]
    fn flatten_respects_zoom_and_collapse() {
        let (mut outline, a, b, _c, _root) = make_outline();
        outline.zoom_in(a);
        let rows = outline.flatten();
        // Zoomed into a non-root node always yields a header row for it,
        // regardless of whether it has children.
        assert_eq!(rows[0].node, a);
        assert!(rows[0].is_header);
        outline.zoom_out();

        outline.arena[b].collapsed = true;
        let grandchild = outline.create_node("hidden");
        outline.arena[grandchild].parent = Some(b);
        outline.arena[b].children.push(grandchild);

        let rows = outline.flatten();
        assert!(!rows.iter().any(|r| r.node == grandchild));
    }

    #[test]
    fn json_round_trip_preserves_tree_shape_and_zoom() {
        let (mut outline, a, b, _c, _root) = make_outline();
        outline.zoom_in(a);
        outline.cursor = 3;
        outline.selected_id = Some(b);

        let json = outline.to_json_string();
        let restored = Outline::from_json_str(&json).unwrap();

        assert_eq!(restored.cursor, 3);
        assert_eq!(restored.get(restored.root()).text, "Home");
        assert_eq!(restored.get(restored.root()).children.len(), 3);
        // zoom_stack round-trips by uuid, not by NodeId (NodeIds aren't
        // stable across process restarts - only the persisted uuid is).
        assert_eq!(restored.breadcrumb().len(), 2);
        assert_eq!(restored.get(restored.zoom_root()).text, "A");
    }

    #[test]
    fn reveal_expands_ancestors_and_zooms_out_to_the_top() {
        let (mut outline, a, b, c, root) = make_outline();
        let needle = outline.create_node("needle");
        outline.append_child(c, needle);
        outline.arena[c].collapsed = true;
        outline.zoom_in(a);

        outline.reveal(needle);
        assert_eq!(outline.zoom_root(), root);
        assert!(!outline.get(c).collapsed);
        assert!(outline.flatten().iter().any(|r| r.node == needle));
        assert!(outline.flatten().iter().any(|r| r.node == b));
    }

    #[test]
    fn hidden_by_completed_checks_ancestors_and_the_toggle() {
        let (mut outline, _a, _b, c, _root) = make_outline();
        let kid = outline.create_node("kid");
        outline.append_child(c, kid);
        outline.arena[c].completed = true;
        assert!(!outline.hidden_by_completed(kid), "hide-completed is off");
        outline.hide_completed = true;
        assert!(outline.hidden_by_completed(c));
        assert!(outline.hidden_by_completed(kid));
    }

    #[test]
    fn timestamps_round_trip_exactly() {
        // From a real Python-written file; lost its last digit without float_roundtrip.
        let json = r#"{"root": {"children": [{"created": 1789956452.0115001, "modified": 1790135295.8527381}]}}"#;
        let resaved = Outline::from_json_str(json).unwrap().to_json_string();
        assert!(resaved.contains("1789956452.0115001"), "{resaved}");
        assert!(resaved.contains("1790135295.8527381"), "{resaved}");
    }

    #[test]
    fn json_keeps_python_key_order() {
        let json = Outline::new().to_json_string();
        let keys: Vec<usize> = ["\"root\"", "\"hide_completed\"", "\"zoom_stack\"", "\"selected_id\"", "\"cursor\""]
            .iter()
            .map(|k| json.rfind(k).unwrap())
            .collect();
        assert!(keys.windows(2).all(|w| w[0] < w[1]), "{json}");
    }

    #[test]
    fn loading_an_outline_with_no_items_seeds_one_empty_item() {
        let json = r#"{"root": {"id": "r", "text": "", "note": "", "completed": false,
                       "collapsed": false, "created": 0, "modified": 0, "children": []}}"#;
        let outline = Outline::from_json_str(json).unwrap();
        assert_eq!(outline.get(outline.root()).children.len(), 1);
    }

    #[test]
    fn merge_forward_joins_next_sibling_text_and_children() {
        let (mut outline, a, b, c, root) = make_outline();
        let grandchild = outline.create_node("child-of-b");
        outline.append_child(b, grandchild);

        assert!(outline.merge_forward(a));
        assert_eq!(outline.get(a).text, "AB");
        assert_eq!(outline.get(a).children, vec![grandchild]);
        assert_eq!(outline.get(grandchild).parent, Some(a));
        assert_eq!(outline.get(root).children, vec![a, c]);
        assert!(!outline.merge_forward(c));
    }

    #[test]
    fn delete_frees_the_whole_subtree() {
        let (mut outline, _a, b, _c, _root) = make_outline();
        let grandchild = outline.create_node("child-of-b");
        outline.append_child(b, grandchild);

        outline.delete(b);
        assert!(outline.arena.get(b).is_none());
        assert!(outline.arena.get(grandchild).is_none());
    }

    #[test]
    fn tags_extraction_matches_python_negative_lookbehind_semantics() {
        let n = Node::with_text("buy milk #errand and email@example.com is not a tag");
        // "email@example.com" must NOT match - the '@' is preceded by a
        // word character, so `(?<!\w)` excludes it. This is the exact
        // behavior `fancy-regex` exists to replicate; plain `regex` can't
        // express it at all.
        assert_eq!(n.tags(), vec!["#errand".to_string()]);
    }
}
