"""The interactive outline widget: rendering + all keyboard editing logic."""
from __future__ import annotations

from typing import Callable, Optional

from rich.text import Text
from textual import events
from textual.containers import VerticalScroll
from textual.widgets import Static

from .models import TAG_RE, Node, Outline, Row


def _highlight_tags(text_obj: Text) -> None:
    for match in TAG_RE.finditer(text_obj.plain):
        text_obj.stylize("bold magenta", match.start(), match.end())


def _apply_cursor(text_obj: Text, cursor: int) -> None:
    length = len(text_obj.plain)
    if cursor >= length:
        text_obj.append(" ", style="reverse")
    else:
        cursor = max(0, cursor)
        text_obj.stylize("reverse", cursor, cursor + 1)


class OutlineView(Static, can_focus=True):
    """Renders the flattened outline and handles all editing key events."""

    def __init__(self, outline: Outline, on_change: Optional[Callable[[], None]] = None):
        super().__init__()
        self.outline = outline
        self.on_change = on_change
        first_row = outline.flatten()[0]
        self.selected_id: str = first_row.node.id
        self.cursor: int = len(first_row.node.text)
        self.editing_note: bool = False

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

    def _changed(self) -> None:
        self.refresh(layout=True)
        self._scroll_selected_into_view()
        if self.on_change:
            self.on_change()

    def _scroll_selected_into_view(self) -> None:
        rows = self._rows()
        line = 0
        target = None
        for row in rows:
            if row.node.id == self.selected_id:
                target = line
                break
            line += 1
            if row.node.note:
                line += 1
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

    # -- selection movement ------------------------------------------------
    def _move_selection(self, delta: int, cursor_at_end: bool = False) -> None:
        rows = self._rows()
        idx = next((i for i, r in enumerate(rows) if r.node.id == self.selected_id), 0)
        new_idx = max(0, min(len(rows) - 1, idx + delta))
        self.editing_note = False
        self.selected_id = rows[new_idx].node.id
        buf = self._buffer(rows[new_idx].node)
        self.cursor = len(buf) if cursor_at_end else min(self.cursor, len(buf))
        self.refresh(layout=True)
        self._scroll_selected_into_view()

    # -- mutations -----------------------------------------------------------
    def _split_line(self, row: Row) -> None:
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
        node = row.node
        new_node = Node(text="")
        new_node.parent = node
        node.children.append(new_node)
        node.collapsed = False
        self.selected_id = new_node.id
        self.cursor = 0
        self._changed()

    def _indent(self) -> None:
        row = self._current_row()
        if row is None or row.is_header:
            return
        if self.outline.indent(row.node):
            self._changed()

    def _outdent(self) -> None:
        row = self._current_row()
        if row is None or row.is_header:
            return
        if self.outline.outdent(row.node):
            self._changed()

    def _backspace(self, row: Row) -> None:
        node = row.node
        buf = self._buffer(node)
        if self.cursor > 0:
            new_buf = buf[: self.cursor - 1] + buf[self.cursor :]
            self._set_buffer(node, new_buf)
            self.cursor -= 1
            node.touch()
            self._changed()
            return
        if self.editing_note or row.is_header:
            return
        target = self.outline.merge_backward(node)
        if target is not None:
            offset = getattr(target, "_merge_cursor", len(target.text))
            self.selected_id = target.id
            self.cursor = offset
            self._changed()

    def _forward_delete(self, row: Row) -> None:
        node = row.node
        buf = self._buffer(node)
        if self.cursor < len(buf):
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

        if key == "enter":
            event.stop()
            if self.editing_note:
                self._insert_char(row, "\n")
            else:
                self._split_line(row)
        elif key == "tab":
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
            if self.cursor > 0:
                self.cursor -= 1
                self.refresh(layout=True)
            else:
                self._move_selection(-1, cursor_at_end=True)
        elif key == "right":
            event.stop()
            buf = self._buffer(node)
            if self.cursor < len(buf):
                self.cursor += 1
                self.refresh(layout=True)
            else:
                self._move_selection(1, cursor_at_end=False)
        elif key == "home":
            event.stop()
            self.cursor = 0
            self.refresh(layout=True)
        elif key == "end":
            event.stop()
            self.cursor = len(self._buffer(node))
            self.refresh(layout=True)
        elif key == "backspace":
            event.stop()
            self._backspace(row)
        elif key == "delete":
            event.stop()
            self._forward_delete(row)
        elif key == "ctrl+up":
            event.stop()
            if self.outline.move_up(node):
                self._changed()
        elif key == "ctrl+down":
            event.stop()
            if self.outline.move_down(node):
                self._changed()
        elif key == "ctrl+right":
            event.stop()
            if not row.is_header:
                self.outline.zoom_in(node)
                self.selected_id = node.id
                self.cursor = len(node.text)
                self.editing_note = False
                self._changed()
        elif key == "ctrl+left":
            event.stop()
            popped = self.outline.zoom_out()
            if popped is not None:
                self.selected_id = popped.id
                self.cursor = len(popped.text)
                self.editing_note = False
                self._changed()
        elif key == "ctrl+d":
            event.stop()
            if not row.is_header:
                self.outline.toggle_complete(node)
                self._changed()
        elif key == "ctrl+k":
            event.stop()
            if row.has_children:
                self.outline.toggle_collapsed(node)
                self._changed()
        elif key == "ctrl+o":
            event.stop()
            self.editing_note = not self.editing_note
            self.cursor = len(self._buffer(node))
            self._changed()
        elif key == "ctrl+n":
            event.stop()
            self._new_child(row)
        elif event.character and event.character.isprintable() and not key.startswith(
            ("ctrl+", "alt+")
        ):
            event.stop()
            self._insert_char(row, event.character)

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
            _apply_cursor(text_segment, self.cursor)
        t.append_text(text_segment)

        show_note = bool(node.note) or (is_selected and self.editing_note)
        if show_note:
            t.append("\n")
            t.append("  " * (row.depth + 1) + "  ")
            note_text = Text(node.note, style="italic grey62")
            if is_selected and self.editing_note:
                _apply_cursor(note_text, self.cursor)
            t.append_text(note_text)

        return t
