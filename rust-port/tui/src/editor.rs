//! Editing state and the single executor for every `Action`, ported from
//! the helpers in `outline_view.py` (vim-modal-keybindings branch). Knows
//! nothing about keys or rendering, so both keymaps share it and tests can
//! drive it directly.

use crate::action::{Action, FoldOp, InsertAt, SearchOp};
use crate::config::EditingStyle;
use crate::undo::UndoStack;
use model::{NodeId, Outline, Row};

// Home-row-first order, like flash.nvim/easymotion default label pools.
pub const JUMP_LABELS: &str = "fjdkslaghrueiwoqpcmxzntyvb";
const MAX_HISTORY: usize = 200;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Normal,
    Insert,
}

enum Jump {
    Idle,
    AwaitingChar,
    AwaitingLabel(Vec<JumpTarget>),
}

struct JumpTarget {
    label: char,
    node: NodeId,
    cursor: usize,
}

#[derive(Default)]
struct SearchInput {
    query: String,
    /// Character offset into `query`.
    cursor: usize,
}

// Cloning the arena keeps NodeIds valid across undo, so unlike the Python
// version there's no round-trip through JSON and ids to re-resolve.
#[derive(Clone)]
struct Snapshot {
    outline: Outline,
    selected: NodeId,
    cursor: usize,
    editing_note: bool,
}

/// Consecutive typing into the same buffer (node, is-note) is one undo step.
type EditGroup = (NodeId, bool);

pub struct Editor {
    pub outline: Outline,
    pub selected: NodeId,
    /// Character (not byte) offset into the active buffer.
    pub cursor: usize,
    pub editing_note: bool,
    pub scroll_offset: usize,
    /// Set by the renderer each frame; 0 means "treat every row as visible".
    pub viewport_height: usize,
    pub should_quit: bool,
    /// One-shot message for the status bar (e.g. "no matches"); the caller takes it.
    pub notice: Option<&'static str>,
    /// Text to put on the clipboard; the caller takes it and does the I/O.
    pub pending_copy: Option<String>,
    mode: Mode,
    style: EditingStyle,
    jump: Jump,
    search: Option<SearchInput>,
    history: UndoStack<Snapshot>,
    edit_group: Option<EditGroup>,
}

fn char_len(s: &str) -> usize {
    s.chars().count()
}

fn byte_at(s: &str, char_idx: usize) -> usize {
    s.char_indices().nth(char_idx).map_or(s.len(), |(b, _)| b)
}

impl Editor {
    pub fn new(outline: Outline, style: EditingStyle) -> Self {
        let rows = outline.flatten();
        let restored = outline
            .selected_id
            .and_then(|id| rows.iter().find(|r| r.node == id).copied());
        let selected = restored
            .or_else(|| rows.first().copied())
            .map_or(outline.zoom_root(), |r| r.node);
        let text_len = char_len(&outline.get(selected).text);
        let cursor = if restored.is_some() {
            outline.cursor.min(text_len)
        } else {
            text_len
        };
        Editor {
            outline,
            selected,
            cursor,
            editing_note: false,
            scroll_offset: 0,
            viewport_height: 0,
            should_quit: false,
            notice: None,
            pending_copy: None,
            mode: match style {
                EditingStyle::Modal => Mode::Normal,
                EditingStyle::Traditional => Mode::Insert,
            },
            style,
            jump: Jump::Idle,
            search: None,
            history: UndoStack::new(MAX_HISTORY),
            edit_group: None,
        }
    }

    pub fn mode(&self) -> Mode {
        self.mode
    }

    pub fn style(&self) -> EditingStyle {
        self.style
    }

    /// Returns the selected row, falling back to the first row if the
    /// selection is no longer visible (e.g. hidden by hide-completed).
    pub fn current_row(&mut self) -> Option<Row> {
        let rows = self.outline.flatten();
        if let Some(row) = rows.iter().find(|r| r.node == self.selected) {
            return Some(*row);
        }
        let first = *rows.first()?;
        self.selected = first.node;
        self.cursor = 0;
        Some(first)
    }

    /// The outline as it should be written to disk, including where the
    /// selection and cursor are so reopening lands in the same place.
    pub fn persisted_json(&mut self) -> String {
        self.outline.selected_id = Some(self.selected);
        self.outline.cursor = self.cursor;
        self.outline.to_json_string()
    }

    pub fn shows_note(&self, id: NodeId) -> bool {
        !self.outline.get(id).note.is_empty() || (id == self.selected && self.editing_note)
    }

    pub fn line_count(&self, row: &Row) -> usize {
        let note_lines = if self.shows_note(row.node) {
            self.outline.get(row.node).note.split('\n').count()
        } else {
            0
        };
        1 + note_lines
    }

    // -- dispatch ------------------------------------------------------------

    pub fn apply(&mut self, action: Action) {
        match action {
            Action::Save => {}
            Action::Quit => self.should_quit = true,
            Action::ToggleHideCompleted => self.outline.hide_completed = !self.outline.hide_completed,
            Action::Undo => self.undo(),
            Action::Redo => self.redo(),
            Action::StartJump => self.jump = Jump::AwaitingChar,
            Action::JumpInput(ch) => self.jump_input(ch),
            Action::OpenSearch => self.search = Some(SearchInput::default()),
            Action::Search(op) => self.search_op(op),
            Action::JumpToFirst => self.jump_to_index(0),
            Action::JumpToLast => self.jump_to_index(usize::MAX),
            _ => {
                if let Some(row) = self.current_row() {
                    self.apply_to_row(action, row);
                }
            }
        }
    }

    fn apply_to_row(&mut self, action: Action, row: Row) {
        let node = row.node;
        self.cursor = self.cursor.min(self.buffer_len(node));
        match action {
            Action::MoveUp => self.move_selection(-1, false),
            Action::MoveDown => self.move_selection(1, false),
            Action::CharLeft { wrap } => {
                self.break_edit_group();
                if self.cursor > 0 {
                    self.cursor -= 1;
                } else if wrap {
                    self.move_selection(-1, true);
                }
            }
            Action::CharRight { wrap } => {
                self.break_edit_group();
                if self.cursor < self.buffer_len(node) {
                    self.cursor += 1;
                } else if wrap {
                    self.move_selection(1, false);
                }
            }
            Action::LineStart => {
                self.break_edit_group();
                self.cursor = 0;
            }
            Action::LineEnd => {
                self.break_edit_group();
                self.cursor = self.buffer_len(node);
            }

            Action::Indent => self.structural(row, Outline::indent),
            Action::Outdent => self.structural(row, Outline::outdent),
            Action::MoveNodeUp => self.structural(row, Outline::move_up),
            Action::MoveNodeDown => self.structural(row, Outline::move_down),
            Action::ZoomIn => {
                if row.is_header {
                    return;
                }
                self.outline.zoom_in(node);
                self.land_on(node);
            }
            Action::ZoomOut => {
                if let Some(popped) = self.outline.zoom_out() {
                    self.land_on(popped);
                }
            }
            Action::Fold(op) => {
                if !row.has_children {
                    return;
                }
                self.checkpoint(None);
                let n = self.outline.get_mut(node);
                n.collapsed = match op {
                    FoldOp::Open => false,
                    FoldOp::Close => true,
                    FoldOp::Toggle => !n.collapsed,
                };
            }
            Action::ToggleComplete => {
                if row.is_header {
                    return;
                }
                self.checkpoint(None);
                self.outline.toggle_complete(node);
            }
            Action::ToggleNote => {
                self.break_edit_group();
                self.editing_note = !self.editing_note;
                self.cursor = self.buffer_len(node);
                if self.editing_note {
                    self.enter_insert(None);
                } else {
                    self.enter_normal();
                }
            }
            Action::NewChild => {
                self.checkpoint(None);
                let new = self.outline.create_node("");
                self.outline.append_child(node, new);
                self.outline.get_mut(node).collapsed = false;
                self.focus_new_node(new);
            }
            Action::OpenBelow => self.open_below(row),
            Action::OpenAbove => {
                if row.is_header {
                    // there's nothing "above" a page's own title; open a child instead
                    self.open_below(row);
                    return;
                }
                self.checkpoint(None);
                let new = self.outline.create_node("");
                self.outline.insert_sibling_before(node, new);
                self.focus_new_node(new);
            }
            Action::ChangeLine => {
                self.checkpoint(None);
                let n = self.outline.get_mut(node);
                n.text.clear();
                n.touch();
                self.editing_note = false;
                self.cursor = 0;
                self.enter_insert(None);
            }
            Action::DeleteNode => self.delete_node(row),
            Action::CopyLine => self.pending_copy = Some(self.outline.get(node).text.clone()),

            Action::EnterInsert(at) => {
                let len = self.buffer_len(node);
                let cursor = match at {
                    InsertAt::Cursor => self.cursor,
                    InsertAt::AfterCursor => (self.cursor + 1).min(len),
                    InsertAt::LineStart => 0,
                    InsertAt::LineEnd => len,
                };
                self.enter_insert(Some(cursor));
            }
            Action::ExitInsert => {
                if self.style == EditingStyle::Modal && self.mode == Mode::Insert {
                    self.enter_normal();
                    // vim's normal-mode cursor sits on a character, never past the end
                    self.cursor = self.cursor.min(self.buffer_len(node).saturating_sub(1));
                }
            }
            Action::InsertChar(ch) => self.insert_char(node, ch),
            Action::Newline => {
                if self.editing_note {
                    self.insert_char(node, '\n');
                } else {
                    self.split_line(row);
                }
            }
            Action::Backspace => self.backspace(row),
            Action::DeleteForward => self.forward_delete(row),

            Action::Undo
            | Action::Redo
            | Action::StartJump
            | Action::JumpInput(_)
            | Action::OpenSearch
            | Action::Search(_)
            | Action::JumpToFirst
            | Action::JumpToLast
            | Action::ToggleHideCompleted
            | Action::Save
            | Action::Quit => unreachable!("row-independent actions are handled in apply"),
        }
    }

    // -- mode ------------------------------------------------------------------

    fn enter_insert(&mut self, cursor: Option<usize>) {
        self.mode = Mode::Insert;
        if let Some(c) = cursor {
            self.cursor = c;
        }
        // each INSERT session is its own undo group, even on the same buffer
        self.break_edit_group();
    }

    /// No-op in traditional style, which has no NORMAL mode to return to.
    fn enter_normal(&mut self) {
        if self.style == EditingStyle::Modal {
            self.mode = Mode::Normal;
        }
    }

    // -- buffers -------------------------------------------------------------

    fn buffer_len(&self, id: NodeId) -> usize {
        let n = self.outline.get(id);
        char_len(if self.editing_note { &n.note } else { &n.text })
    }

    fn buffer_mut(&mut self, id: NodeId) -> &mut String {
        let n = self.outline.get_mut(id);
        if self.editing_note {
            &mut n.note
        } else {
            &mut n.text
        }
    }

    // -- undo/redo -------------------------------------------------------------

    fn snapshot(&self) -> Snapshot {
        Snapshot {
            outline: self.outline.clone(),
            selected: self.selected,
            cursor: self.cursor,
            editing_note: self.editing_note,
        }
    }

    fn restore(&mut self, s: Snapshot) {
        self.outline = s.outline;
        self.selected = s.selected;
        self.cursor = s.cursor;
        self.editing_note = s.editing_note;
    }

    /// Records undo history before a mutation. A `group` matching the
    /// current one coalesces into the existing undo step.
    fn checkpoint(&mut self, group: Option<EditGroup>) {
        if group.is_some() && group == self.edit_group {
            return;
        }
        let s = self.snapshot();
        self.history.checkpoint(s);
        self.edit_group = group;
    }

    /// For mutations that may turn out to be no-ops: snapshot first, and
    /// only record it once the mutation reports success.
    fn commit(&mut self, before: Snapshot) {
        self.history.checkpoint(before);
        self.edit_group = None;
    }

    fn break_edit_group(&mut self) {
        self.edit_group = None;
    }

    fn undo(&mut self) {
        let current = self.snapshot();
        if let Some(s) = self.history.undo(current) {
            self.break_edit_group();
            self.restore(s);
        }
    }

    fn redo(&mut self) {
        let current = self.snapshot();
        if let Some(s) = self.history.redo(current) {
            self.break_edit_group();
            self.restore(s);
        }
    }

    // -- selection -------------------------------------------------------------

    fn move_selection(&mut self, delta: isize, cursor_at_end: bool) {
        self.break_edit_group();
        let rows = self.outline.flatten();
        if rows.is_empty() {
            return;
        }
        let idx = rows.iter().position(|r| r.node == self.selected).unwrap_or(0);
        let new_idx = (idx as isize + delta).clamp(0, rows.len() as isize - 1) as usize;
        self.editing_note = false;
        self.selected = rows[new_idx].node;
        let len = self.buffer_len(self.selected);
        self.cursor = if cursor_at_end { len } else { self.cursor.min(len) };
    }

    fn jump_to_index(&mut self, index: usize) {
        let rows = self.outline.flatten();
        let Some(row) = rows.get(index.min(rows.len().saturating_sub(1))) else {
            return;
        };
        self.break_edit_group();
        self.editing_note = false;
        self.selected = row.node;
        self.cursor = 0;
    }

    /// After a zoom, select `id` with the cursor at the end of its text.
    fn land_on(&mut self, id: NodeId) {
        self.selected = id;
        self.editing_note = false;
        self.cursor = char_len(&self.outline.get(id).text);
        self.enter_normal();
    }

    fn focus_new_node(&mut self, id: NodeId) {
        self.editing_note = false;
        self.selected = id;
        self.cursor = 0;
        self.enter_insert(None);
    }

    // -- structural mutations ----------------------------------------------------

    fn structural(&mut self, row: Row, op: fn(&mut Outline, NodeId) -> bool) {
        if row.is_header {
            return;
        }
        let before = self.snapshot();
        if op(&mut self.outline, row.node) {
            self.commit(before);
        }
    }

    fn open_below(&mut self, row: Row) {
        self.checkpoint(None);
        let new = self.outline.create_node("");
        if row.is_header {
            self.outline.add_first_child(row.node, new);
        } else {
            self.outline.insert_sibling_after(row.node, new);
        }
        self.focus_new_node(new);
    }

    fn split_line(&mut self, row: Row) {
        self.checkpoint(None);
        let node = row.node;
        let cursor = self.cursor;
        let text = &mut self.outline.get_mut(node).text;
        let after = text.split_off(byte_at(text, cursor));
        let new = self.outline.create_node(after);
        if row.is_header {
            self.outline.add_first_child(node, new);
        } else {
            self.outline.insert_sibling_after(node, new);
        }
        self.outline.get_mut(node).touch();
        self.selected = new;
        self.cursor = 0;
    }

    fn delete_node(&mut self, row: Row) {
        let node = row.node;
        let Some(parent) = self.outline.get(node).parent else {
            return;
        };
        if row.is_header {
            return;
        }
        if parent == self.outline.root() && self.outline.get(parent).children.len() == 1 {
            return; // keep at least one top-level item
        }
        self.checkpoint(None);
        let idx = self
            .outline
            .flatten()
            .iter()
            .position(|r| r.node == node)
            .unwrap_or(0);
        self.outline.delete(node);
        let rows = self.outline.flatten();
        self.selected = rows
            .get(idx.min(rows.len().saturating_sub(1)))
            .map_or(self.outline.zoom_root(), |r| r.node);
        self.cursor = 0;
    }

    // -- text edits ---------------------------------------------------------------

    fn insert_char(&mut self, node: NodeId, ch: char) {
        self.checkpoint(Some((node, self.editing_note)));
        let cursor = self.cursor;
        let buf = self.buffer_mut(node);
        let at = byte_at(buf, cursor);
        buf.insert(at, ch);
        self.cursor += 1;
        self.outline.get_mut(node).touch();
    }

    fn backspace(&mut self, row: Row) {
        let node = row.node;
        if self.cursor > 0 {
            self.checkpoint(Some((node, self.editing_note)));
            let target = self.cursor - 1;
            let buf = self.buffer_mut(node);
            let at = byte_at(buf, target);
            buf.remove(at);
            self.cursor = target;
            self.outline.get_mut(node).touch();
            return;
        }
        if self.editing_note || row.is_header {
            return;
        }
        let before = self.snapshot();
        if let Some(target) = self.outline.merge_backward(node) {
            self.commit(before);
            let fallback = char_len(&self.outline.get(target).text);
            self.cursor = self.outline.get_mut(target).merge_cursor.take().unwrap_or(fallback);
            self.selected = target;
        }
    }

    fn forward_delete(&mut self, row: Row) {
        let node = row.node;
        if self.cursor < self.buffer_len(node) {
            self.checkpoint(Some((node, self.editing_note)));
            let cursor = self.cursor;
            let buf = self.buffer_mut(node);
            let at = byte_at(buf, cursor);
            buf.remove(at);
            self.outline.get_mut(node).touch();
            return;
        }
        if self.editing_note || row.is_header {
            return;
        }
        let before = self.snapshot();
        if self.outline.merge_forward(node) {
            self.commit(before);
            self.outline.get_mut(node).touch();
        }
    }

    // -- search ------------------------------------------------------------------

    /// The open search box's query and cursor, if any.
    pub fn search_input(&self) -> Option<(&str, usize)> {
        self.search.as_ref().map(|s| (s.query.as_str(), s.cursor))
    }

    fn search_op(&mut self, op: SearchOp) {
        let Some(input) = self.search.as_mut() else {
            return;
        };
        let len = char_len(&input.query);
        match op {
            SearchOp::Insert(c) => {
                let at = byte_at(&input.query, input.cursor);
                input.query.insert(at, c);
                input.cursor += 1;
            }
            SearchOp::Backspace => {
                if input.cursor > 0 {
                    input.cursor -= 1;
                    let at = byte_at(&input.query, input.cursor);
                    input.query.remove(at);
                }
            }
            SearchOp::Delete => {
                if input.cursor < len {
                    let at = byte_at(&input.query, input.cursor);
                    input.query.remove(at);
                }
            }
            SearchOp::Left => input.cursor = input.cursor.saturating_sub(1),
            SearchOp::Right => input.cursor = (input.cursor + 1).min(len),
            SearchOp::Home => input.cursor = 0,
            SearchOp::End => input.cursor = len,
            SearchOp::Cancel => self.search = None,
            SearchOp::Submit => {
                let query = std::mem::take(&mut input.query);
                self.search = None;
                self.go_to_first_match(&query);
            }
        }
    }

    /// Port of `_reveal`: selects the first match, expanding and zooming
    /// out as needed. Matches hidden by hide-completed are skipped, since
    /// they can't be selected.
    fn go_to_first_match(&mut self, query: &str) {
        if query.trim().is_empty() {
            return;
        }
        let first = self
            .outline
            .search(query)
            .into_iter()
            .find(|&id| !self.outline.hidden_by_completed(id));
        let Some(node) = first else {
            self.notice = Some("no matches");
            return;
        };
        self.outline.reveal(node);
        self.break_edit_group();
        self.editing_note = false;
        self.selected = node;
        self.cursor = 0;
        self.enter_normal();
    }

    // -- flash.nvim-style jump ------------------------------------------------------

    pub fn jump_active(&self) -> bool {
        !matches!(self.jump, Jump::Idle)
    }

    pub fn jump_hint(&self) -> Option<&'static str> {
        match self.jump {
            Jump::Idle => None,
            Jump::AwaitingChar => Some("type a character to jump to…"),
            Jump::AwaitingLabel(_) => Some("type a label to jump…"),
        }
    }

    /// (char offset, label) pairs to overlay on `id`'s text.
    pub fn jump_labels_for(&self, id: NodeId) -> Vec<(usize, char)> {
        match &self.jump {
            Jump::AwaitingLabel(targets) => targets
                .iter()
                .filter(|t| t.node == id)
                .map(|t| (t.cursor, t.label))
                .collect(),
            _ => Vec::new(),
        }
    }

    fn jump_input(&mut self, ch: Option<char>) {
        match std::mem::replace(&mut self.jump, Jump::Idle) {
            Jump::Idle => {}
            Jump::AwaitingChar => {
                let Some(c) = ch else { return };
                let targets: Vec<JumpTarget> = JUMP_LABELS
                    .chars()
                    .zip(self.find_jump_matches(c))
                    .map(|(label, (node, cursor))| JumpTarget { label, node, cursor })
                    .collect();
                if !targets.is_empty() {
                    self.jump = Jump::AwaitingLabel(targets);
                }
            }
            Jump::AwaitingLabel(targets) => {
                let Some(t) = ch.and_then(|c| targets.iter().find(|t| t.label == c)) else {
                    return;
                };
                self.break_edit_group();
                self.editing_note = false;
                self.selected = t.node;
                self.cursor = t.cursor;
            }
        }
    }

    /// Case-insensitive matches in on-screen rows, nearest to the selection first.
    fn find_jump_matches(&self, target: char) -> Vec<(NodeId, usize)> {
        let visible = self.visible_rows();
        let selected_rank = visible.iter().position(|r| r.node == self.selected).unwrap_or(0);
        let mut scored = Vec::new();
        for (rank, row) in visible.iter().enumerate() {
            for (idx, c) in self.outline.get(row.node).text.chars().enumerate() {
                if c.to_lowercase().eq(target.to_lowercase()) {
                    scored.push((rank.abs_diff(selected_rank), row.node, idx));
                }
            }
        }
        scored.sort_by_key(|m| m.0);
        scored
            .into_iter()
            .take(JUMP_LABELS.chars().count())
            .map(|(_, node, idx)| (node, idx))
            .collect()
    }

    fn visible_rows(&self) -> Vec<Row> {
        let rows = self.outline.flatten();
        if self.viewport_height == 0 {
            return rows;
        }
        let window = self.scroll_offset..self.scroll_offset + self.viewport_height;
        let mut line = 0;
        let mut visible = Vec::new();
        for row in rows {
            if window.contains(&line) {
                visible.push(row);
            }
            line += self.line_count(&row);
        }
        visible
    }
}
