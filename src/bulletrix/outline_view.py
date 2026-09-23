"""The interactive outline widget: rendering + all keyboard editing logic.

Editing is modal, vim-style: NORMAL mode drives navigation and structural
commands (movement, indent, delete, fold, ...); INSERT mode is where
character keys land in the buffer. `i`/`a`/`I`/`A`/`o`/`O`/`cc` enter
INSERT; `Escape` returns to NORMAL. `yy` copies the current line's text to
the system clipboard. `u` undoes and `Ctrl+R` redoes (NORMAL mode only,
matching vim). Not implemented (out of scope for this pass): in-app paste
and numeric count prefixes (e.g. `3j`).
"""
from __future__ import annotations

from enum import Enum
from typing import Callable, Optional

from rich.text import Text
from textual import events
from textual.containers import VerticalScroll
from textual.widgets import Static

from .models import TAG_RE, Node, Outline, Row
from .undo import UndoStack


class Mode(Enum):
    NORMAL = "NORMAL"
    INSERT = "INSERT"


# Home-row-first order, like flash.nvim/easymotion default label pools.
JUMP_LABELS = "fjdkslaghrueiwoqpcmxzntyvb"


def _highlight_tags(text_obj: Text) -> None:
    for match in TAG_RE.finditer(text_obj.plain):
        text_obj.stylize("bold magenta", match.start(), match.end())


def _apply_cursor(text_obj: Text, cursor: int, mode: Mode) -> Text:
    # Styles an existing character cell rather than inserting a glyph, so the
    # cursor never shifts surrounding text (a real terminal caret isn't an
    # option here: Textual hides the OS cursor for the whole app lifetime and
    # even its own Input/TextArea widgets fake theirs this same way).
    length = len(text_obj.plain)
    cursor = max(0, min(cursor, length))
    style = "underline bold yellow" if mode is Mode.INSERT else "reverse"
    result = text_obj.copy()
    if cursor >= length:
        result.append(" ", style=style)
    else:
        result.stylize(style, cursor, cursor + 1)
    return result


def _overlay_labels(text_obj: Text, labels: dict[int, str]) -> Text:
    # Highlights the matched character in blue and replaces the character
    # to its right with the label (or appends if at the end). This render-only
    # modification keeps the original text untouched.
    if not labels:
        return text_obj

    result = Text()
    text = text_obj.plain
    length = len(text)

    # Build a map of positions where labels should appear (replacing or appending)
    label_positions = {idx + 1: labels[idx] for idx in labels.keys()}

    for i in range(length):
        if i in labels:
            # Append the matching character with blue highlight
            result.append(text[i], style="bold white on #2f69df")
        elif i in label_positions:
            # Replace this character with the label
            result.append(label_positions[i], style="bold white on #ff007c")
        else:
            result.append(text[i])

    # Handle labels that go beyond the text length (append them)
    for pos in sorted(label_positions.keys()):
        if pos >= length:
            result.append(label_positions[pos], style="bold white on #ff007c")

    return result


class OutlineView(Static, can_focus=True):
    """Renders the flattened outline and handles all editing key events."""

    def __init__(self, outline: Outline, on_change: Optional[Callable[[], None]] = None):
        super().__init__()
        self.outline = outline
        self.on_change = on_change
        rows = outline.flatten()
        restored = next((r for r in rows if r.node.id == outline.selected_id), None)
        row = restored or rows[0]
        self.selected_id: str = row.node.id
        self.cursor: int = len(row.node.text)
        if restored is not None:
            self.cursor = max(0, min(outline.cursor, len(row.node.text)))
        self.editing_note: bool = False
        self.mode: Mode = Mode.NORMAL
        self._pending: Optional[str] = None
        self._jump_awaiting_char: bool = False
        self._jump_targets: dict[str, tuple[str, int]] = {}
        self._undo_stack = UndoStack()
        self._edit_group: Optional[str] = None

    # -- helpers -----------------------------------------------------------
    def _rows(self) -> list[Row]:
        return self.outline.flatten()

    def _current_row(self) -> Optional[Row]:
        rows = self._rows()
        for row in rows:
            if row.node.id == self.selected_id:
                return row
        if rows:
            self.selected_id = rows[0].node.id
            self.cursor = 0
            return rows[0]
        return None

    def _buffer(self, node: Node) -> str:
        return node.note if self.editing_note else node.text

    def _set_buffer(self, node: Node, value: str) -> None:
        if self.editing_note:
            node.note = value
        else:
            node.text = value

    def _enter_insert(self, cursor: Optional[int] = None) -> None:
        self.mode = Mode.INSERT
        if cursor is not None:
            self.cursor = cursor
        # each INSERT session is its own undo group, even if it lands back
        # on the same node/buffer as a previous session.
        self._break_edit_group()

    def _enter_normal(self) -> None:
        self.mode = Mode.NORMAL
        self._pending = None

    def _changed(self) -> None:
        self.refresh(layout=True)
        self._scroll_selected_into_view()
        if self.on_change:
            self.on_change()

    def _rows_with_line_numbers(self) -> list[tuple[Row, int]]:
        result = []
        line = 0
        for row in self._rows():
            result.append((row, line))
            line += 1
            if row.node.note:
                line += 1
        return result

    # -- undo/redo -----------------------------------------------------------
    def _snapshot(self) -> dict:
        data = self.outline.to_dict()
        data["_view_selected_id"] = self.selected_id
        data["_view_cursor"] = self.cursor
        data["_view_editing_note"] = self.editing_note
        return data

    def _restore(self, snapshot: dict) -> None:
        restored = Outline.from_dict(snapshot)
        self.outline.root = restored.root
        self.outline.zoom_stack = restored.zoom_stack
        self.outline.hide_completed = restored.hide_completed
        self.selected_id = snapshot.get("_view_selected_id") or restored.root.id
        self.cursor = snapshot.get("_view_cursor", 0)
        self.editing_note = snapshot.get("_view_editing_note", False)
        self._changed()

    def _checkpoint(self, group: Optional[str] = None) -> None:
        """Record undo history before a mutation. `group` coalesces consecutive
        edits (e.g. typing) sharing the same group key into a single undo step."""
        if group is not None and group == self._edit_group:
            return
        self._undo_stack.checkpoint(self._snapshot())
        self._edit_group = group

    def _break_edit_group(self) -> None:
        self._edit_group = None

    def _undo(self) -> None:
        snapshot = self._undo_stack.undo(self._snapshot())
        if snapshot is not None:
            self._break_edit_group()
            self._restore(snapshot)

    def _redo(self) -> None:
        snapshot = self._undo_stack.redo(self._snapshot())
        if snapshot is not None:
            self._break_edit_group()
            self._restore(snapshot)

    def _scroll_selected_into_view(self) -> None:
        target = None
        for row, line in self._rows_with_line_numbers():
            if row.node.id == self.selected_id:
                target = line
                break
        if target is None:
            return
        try:
            container = self.parent
            if isinstance(container, VerticalScroll):
                top = container.scroll_offset.y
                height = container.size.height or 1
                if target < top:
                    container.scroll_to(y=target, animate=False)
                elif target >= top + height - 1:
                    container.scroll_to(y=target - height + 2, animate=False)
        except Exception:
            pass

    def _visible_rows(self) -> list[Row]:
        rows_with_lines = self._rows_with_line_numbers()
        try:
            container = self.parent
            if isinstance(container, VerticalScroll):
                top = container.scroll_offset.y
                height = container.size.height or len(rows_with_lines) or 1
                bottom = top + height
                return [row for row, line in rows_with_lines if top <= line < bottom]
        except Exception:
            pass
        return [row for row, _ in rows_with_lines]

    # -- selection movement ------------------------------------------------
    def _move_selection(self, delta: int, cursor_at_end: bool = False) -> None:
        self._break_edit_group()
        rows = self._rows()
        idx = next((i for i, r in enumerate(rows) if r.node.id == self.selected_id), 0)
        new_idx = max(0, min(len(rows) - 1, idx + delta))
        self.editing_note = False
        self.selected_id = rows[new_idx].node.id
        buf = self._buffer(rows[new_idx].node)
        self.cursor = len(buf) if cursor_at_end else min(self.cursor, len(buf))
        self.refresh(layout=True)
        self._scroll_selected_into_view()

    def _jump_to_index(self, index: int) -> None:
        rows = self._rows()
        if not rows:
            return
        idx = max(0, min(len(rows) - 1, index))
        self.editing_note = False
        self.selected_id = rows[idx].node.id
        self.cursor = 0
        self.refresh(layout=True)
        self._scroll_selected_into_view()

    # -- mutations -----------------------------------------------------------
    def _split_line(self, row: Row) -> None:
        self._checkpoint()
        node = row.node
        text = node.text
        before, after = text[: self.cursor], text[self.cursor :]
        new_node = Node(text=after)
        node.text = before
        if row.is_header:
            self.outline.add_first_child(node, new_node)
        else:
            self.outline.insert_sibling_after(node, new_node)
        node.touch()
        self.selected_id = new_node.id
        self.cursor = 0
        self._changed()

    def _new_child(self, row: Row) -> None:
        self._checkpoint()
        node = row.node
        new_node = Node(text="")
        new_node.parent = node
        node.children.append(new_node)
        node.collapsed = False
        self.selected_id = new_node.id
        self.cursor = 0
        self._enter_insert()
        self._changed()

    def _open_below(self, row: Row) -> None:
        self._checkpoint()
        node = row.node
        new_node = Node(text="")
        if row.is_header:
            self.outline.add_first_child(node, new_node)
        else:
            self.outline.insert_sibling_after(node, new_node)
        self.selected_id = new_node.id
        self.cursor = 0
        self._enter_insert()
        self._changed()

    def _open_above(self, row: Row) -> None:
        if row.is_header:
            # there's nothing "above" a page's own title; open a child instead
            self._open_below(row)
            return
        self._checkpoint()
        node = row.node
        new_node = Node(text="")
        self.outline.insert_sibling_before(node, new_node)
        self.selected_id = new_node.id
        self.cursor = 0
        self._enter_insert()
        self._changed()

    def _change_line(self, row: Row) -> None:
        self._checkpoint()
        row.node.text = ""
        row.node.touch()
        self.selected_id = row.node.id
        self.cursor = 0
        self._enter_insert()
        self._changed()

    def _delete_node(self, row: Row) -> None:
        node = row.node
        parent = node.parent
        if parent is None or row.is_header:
            return
        if parent is self.outline.root and len(parent.children) == 1:
            return  # keep at least one top-level item
        self._checkpoint()
        rows = self._rows()
        idx = next(i for i, r in enumerate(rows) if r.node.id == node.id)
        self.outline.remove(node)
        new_rows = self._rows()
        if not new_rows:
            return
        new_idx = min(idx, len(new_rows) - 1)
        self.selected_id = new_rows[new_idx].node.id
        self.cursor = 0
        self._changed()

    def _fold(self, row: Row, which: str) -> None:
        if not row.has_children:
            return
        self._checkpoint()
        if which == "o":
            row.node.collapsed = False
        elif which == "c":
            row.node.collapsed = True
        elif which == "a":
            row.node.collapsed = not row.node.collapsed
        self._changed()

    def _zoom_in(self, row: Row, node: Node) -> None:
        if row.is_header:
            return
        self.outline.zoom_in(node)
        self.selected_id = node.id
        self.cursor = len(node.text)
        self.editing_note = False
        self._enter_normal()
        self._changed()

    def _zoom_out(self) -> None:
        popped = self.outline.zoom_out()
        if popped is not None:
            self.selected_id = popped.id
            self.cursor = len(popped.text)
            self.editing_note = False
            self._enter_normal()
            self._changed()

    # -- flash.nvim-style jump ------------------------------------------------
    @property
    def jump_hint(self) -> Optional[str]:
        if self._jump_awaiting_char:
            return "type a character to jump to…"
        if self._jump_targets:
            return "type a label to jump…"
        return None

    def _start_jump(self) -> None:
        self._jump_awaiting_char = True
        self._changed()

    def _cancel_jump(self) -> None:
        self._jump_awaiting_char = False
        self._jump_targets = {}
        self._changed()

    def _find_jump_matches(self, target_ch: str) -> list[tuple[Row, int]]:
        visible = self._visible_rows()
        selected_rank = next(
            (i for i, r in enumerate(visible) if r.node.id == self.selected_id), 0
        )
        scored: list[tuple[int, Row, int]] = []
        for rank, row in enumerate(visible):
            for idx, c in enumerate(row.node.text):
                if c.lower() == target_ch.lower():
                    scored.append((abs(rank - selected_rank), row, idx))
        scored.sort(key=lambda m: m[0])
        return [(row, idx) for _, row, idx in scored[: len(JUMP_LABELS)]]

    def _start_jump_label_selection(self, target_ch: str) -> None:
        matches = self._find_jump_matches(target_ch)
        self._jump_awaiting_char = False
        if not matches:
            self._jump_targets = {}
            self._changed()
            return
        self._jump_targets = {
            label: (row.node.id, idx) for label, (row, idx) in zip(JUMP_LABELS, matches)
        }
        self._changed()

    def _complete_jump(self, label: str) -> None:
        target = self._jump_targets.get(label)
        self._jump_awaiting_char = False
        self._jump_targets = {}
        if target is None:
            self._changed()
            return
        node_id, idx = target
        self.editing_note = False
        self.selected_id = node_id
        self.cursor = idx
        self._changed()

    def _handle_jump_key(self, event: events.Key) -> None:
        event.stop()
        if event.key == "escape":
            self._cancel_jump()
            return
        ch = event.character
        if self._jump_awaiting_char:
            if ch:
                self._start_jump_label_selection(ch)
            else:
                self._cancel_jump()
            return
        if ch and ch in self._jump_targets:
            self._complete_jump(ch)
        else:
            self._cancel_jump()

    def _labels_for_node(self, node_id: str) -> dict[int, str]:
        return {idx: label for label, (nid, idx) in self._jump_targets.items() if nid == node_id}

    def _indent(self) -> None:
        row = self._current_row()
        if row is None or row.is_header:
            return
        snapshot = self._snapshot()
        if self.outline.indent(row.node):
            self._undo_stack.checkpoint(snapshot)
            self._edit_group = None
            self._changed()

    def _outdent(self) -> None:
        row = self._current_row()
        if row is None or row.is_header:
            return
        snapshot = self._snapshot()
        if self.outline.outdent(row.node):
            self._undo_stack.checkpoint(snapshot)
            self._edit_group = None
            self._changed()

    def _backspace(self, row: Row) -> None:
        node = row.node
        buf = self._buffer(node)
        if self.cursor > 0:
            self._checkpoint(group=f"type:{node.id}:{self.editing_note}")
            new_buf = buf[: self.cursor - 1] + buf[self.cursor :]
            self._set_buffer(node, new_buf)
            self.cursor -= 1
            node.touch()
            self._changed()
            return
        if self.editing_note or row.is_header:
            return
        snapshot = self._snapshot()
        target = self.outline.merge_backward(node)
        if target is not None:
            self._undo_stack.checkpoint(snapshot)
            self._edit_group = None
            offset = getattr(target, "_merge_cursor", len(target.text))
            self.selected_id = target.id
            self.cursor = offset
            self._changed()

    def _forward_delete(self, row: Row) -> None:
        node = row.node
        buf = self._buffer(node)
        if self.cursor < len(buf):
            self._checkpoint(group=f"type:{node.id}:{self.editing_note}")
            new_buf = buf[: self.cursor] + buf[self.cursor + 1 :]
            self._set_buffer(node, new_buf)
            node.touch()
            self._changed()
            return
        if self.editing_note or row.is_header:
            return
        parent = node.parent
        if parent is None:
            return
        idx = self.outline.index_in_parent(node)
        if idx + 1 >= len(parent.children):
            return
        self._checkpoint()
        nxt = parent.children[idx + 1]
        node.text += nxt.text
        node.children = node.children + nxt.children
        for c in nxt.children:
            c.parent = node
        parent.children.pop(idx + 1)
        node.touch()
        self._changed()

    def _insert_char(self, row: Row, ch: str) -> None:
        node = row.node
        self._checkpoint(group=f"type:{node.id}:{self.editing_note}")
        buf = self._buffer(node)
        new_buf = buf[: self.cursor] + ch + buf[self.cursor :]
        self._set_buffer(node, new_buf)
        self.cursor += len(ch)
        node.touch()
        self._changed()

    # -- key handling -------------------------------------------------------
    def on_key(self, event: events.Key) -> None:
        row = self._current_row()
        if row is None:
            return
        node = row.node
        key = event.key

        if self._jump_awaiting_char or self._jump_targets:
            self._handle_jump_key(event)
            return

        if key == "escape":
            event.stop()
            if self.mode is Mode.INSERT:
                buf = self._buffer(node)
                self._enter_normal()
                self.cursor = max(0, min(self.cursor, max(0, len(buf) - 1)))
                self._changed()
            elif self._pending is not None:
                self._pending = None
            return

        if self._handle_shared_key(row, node, key, event):
            return

        if self.mode is Mode.INSERT:
            self._handle_insert_key(row, node, key, event)
        else:
            self._handle_normal_key(row, node, key, event)

    def _handle_shared_key(self, row: Row, node: Node, key: str, event: events.Key) -> bool:
        """Keys that behave the same in both modes. Returns True if handled."""
        if key == "tab":
            event.stop()
            self._indent()
        elif key == "shift+tab":
            event.stop()
            self._outdent()
        elif key == "up":
            event.stop()
            self._move_selection(-1)
        elif key == "down":
            event.stop()
            self._move_selection(1)
        elif key == "left":
            event.stop()
            self._break_edit_group()
            if self.cursor > 0:
                self.cursor -= 1
                self.refresh(layout=True)
            else:
                self._move_selection(-1, cursor_at_end=True)
        elif key == "right":
            event.stop()
            self._break_edit_group()
            buf = self._buffer(node)
            if self.cursor < len(buf):
                self.cursor += 1
                self.refresh(layout=True)
            else:
                self._move_selection(1, cursor_at_end=False)
        elif key == "home":
            event.stop()
            self._break_edit_group()
            self.cursor = 0
            self.refresh(layout=True)
        elif key == "end":
            event.stop()
            self._break_edit_group()
            self.cursor = len(self._buffer(node))
            self.refresh(layout=True)
        elif key == "ctrl+up":
            event.stop()
            snapshot = self._snapshot()
            if self.outline.move_up(node):
                self._undo_stack.checkpoint(snapshot)
                self._edit_group = None
                self._changed()
        elif key == "ctrl+down":
            event.stop()
            snapshot = self._snapshot()
            if self.outline.move_down(node):
                self._undo_stack.checkpoint(snapshot)
                self._edit_group = None
                self._changed()
        elif key == "ctrl+right":
            event.stop()
            self._zoom_in(row, node)
        elif key == "ctrl+left":
            event.stop()
            self._zoom_out()
        elif key == "ctrl+d":
            event.stop()
            if not row.is_header:
                self._checkpoint()
                self.outline.toggle_complete(node)
                self._changed()
        elif key == "ctrl+k":
            event.stop()
            if row.has_children:
                self._checkpoint()
                self.outline.toggle_collapsed(node)
                self._changed()
        elif key == "ctrl+o":
            event.stop()
            self._break_edit_group()
            self.editing_note = not self.editing_note
            self.cursor = len(self._buffer(node))
            if self.editing_note:
                self._enter_insert()
            else:
                self._enter_normal()
            self._changed()
        elif key == "ctrl+n":
            event.stop()
            self._new_child(row)
        elif key == "ctrl+f":
            event.stop()
            self.app.action_search()
        elif key == "ctrl+r":
            event.stop()
            self._redo()
        else:
            return False
        return True

    def _handle_insert_key(self, row: Row, node: Node, key: str, event: events.Key) -> None:
        if key == "enter":
            event.stop()
            if self.editing_note:
                self._insert_char(row, "\n")
            else:
                self._split_line(row)
        elif key == "backspace":
            event.stop()
            self._backspace(row)
        elif key == "delete":
            event.stop()
            self._forward_delete(row)
        elif event.character and event.character.isprintable() and not key.startswith(
            ("ctrl+", "alt+")
        ):
            event.stop()
            self._insert_char(row, event.character)

    def _handle_normal_key(self, row: Row, node: Node, key: str, event: events.Key) -> None:
        pending = self._pending
        self._pending = None
        ch = event.character

        if pending == "d":
            if ch == "d":
                event.stop()
                self._delete_node(row)
            return
        if pending == "c":
            if ch == "c":
                event.stop()
                self._change_line(row)
            return
        if pending == "g":
            if ch == "g":
                event.stop()
                self._jump_to_index(0)
            return
        if pending == "z":
            if ch in ("o", "c", "a"):
                event.stop()
                self._fold(row, ch)
            return
        if pending == "y":
            if ch == "y":
                event.stop()
                self.app.action_copy_task()
            return

        if key == "enter":
            event.stop()
            self._zoom_in(row, node)
            return
        if key == "space":
            event.stop()
            self._fold(row, "a")
            return
        if ch == "q":
            event.stop()
            self.app.action_quit()
            return

        if ch == "d":
            event.stop()
            self._pending = "d"
        elif ch == "c":
            event.stop()
            self._pending = "c"
        elif ch == "g":
            event.stop()
            self._pending = "g"
        elif ch == "z":
            event.stop()
            self._pending = "z"
        elif ch == "y":
            event.stop()
            self._pending = "y"
        elif ch == "i":
            event.stop()
            self._enter_insert()
            self._changed()
        elif ch == "a":
            event.stop()
            self._enter_insert(cursor=min(len(self._buffer(node)), self.cursor + 1))
            self._changed()
        elif ch == "I":
            event.stop()
            self._enter_insert(cursor=0)
            self._changed()
        elif ch == "A":
            event.stop()
            self._enter_insert(cursor=len(self._buffer(node)))
            self._changed()
        elif ch == "o":
            event.stop()
            self._open_below(row)
        elif ch == "O":
            event.stop()
            self._open_above(row)
        elif ch == "x":
            event.stop()
            self._forward_delete(row)
        elif ch == "h":
            event.stop()
            if self.cursor > 0:
                self.cursor -= 1
                self.refresh(layout=True)
        elif ch == "l":
            event.stop()
            buf = self._buffer(node)
            if self.cursor < len(buf):
                self.cursor += 1
                self.refresh(layout=True)
        elif ch == "H":
            event.stop()
            self._zoom_out()
        elif ch == "L":
            event.stop()
            self._zoom_in(row, node)
        elif ch == "j":
            event.stop()
            self._move_selection(1)
        elif ch == "k":
            event.stop()
            self._move_selection(-1)
        elif ch == "0":
            event.stop()
            self.cursor = 0
            self.refresh(layout=True)
        elif ch == "$":
            event.stop()
            self.cursor = len(self._buffer(node))
            self.refresh(layout=True)
        elif ch == "G":
            event.stop()
            self._jump_to_index(len(self._rows()) - 1)
        elif ch == "/":
            event.stop()
            self.app.action_search()
        elif ch == "s":
            event.stop()
            self._start_jump()
        elif ch == "u":
            event.stop()
            self._undo()
        elif ch and ch.isprintable() and not key.startswith(("ctrl+", "alt+")):
            # swallow other printable keys in NORMAL mode rather than typing them
            event.stop()

    # -- rendering -----------------------------------------------------------
    def render(self) -> Text:
        rows = self._rows()
        out = Text()
        for i, row in enumerate(rows):
            out.append_text(self._render_row(row))
            if i < len(rows) - 1:
                out.append("\n")
        return out

    def _render_row(self, row: Row) -> Text:
        node = row.node
        is_selected = node.id == self.selected_id
        t = Text()

        if row.is_header:
            bullet = "» "
            prefix = ""
        else:
            prefix = "  " * row.depth
            if row.has_children:
                bullet = "▾ " if row.visible_children else "▸ "
            else:
                bullet = "• "
        t.append(prefix)
        bullet_style = "bold cyan" if row.has_children else "grey58"
        if is_selected:
            bullet_style += " bold yellow"
        t.append(bullet, style=bullet_style.strip())

        style_parts = []
        if row.is_header:
            style_parts.append("bold underline")
        if node.completed:
            style_parts.append("strike dim")
        text_segment = Text(node.text, style=" ".join(style_parts) or None)
        _highlight_tags(text_segment)
        if is_selected and not self.editing_note:
            text_segment = _apply_cursor(text_segment, self.cursor, self.mode)
        labels = self._labels_for_node(node.id)
        if labels:
            text_segment = _overlay_labels(text_segment, labels)
        t.append_text(text_segment)

        show_note = bool(node.note) or (is_selected and self.editing_note)
        if show_note:
            t.append("\n")
            t.append("  " * (row.depth + 1) + "  ")
            note_text = Text(node.note, style="italic grey62")
            if is_selected and self.editing_note:
                note_text = _apply_cursor(note_text, self.cursor, self.mode)
            t.append_text(note_text)

        return t
