"""Textual application: layout, key bindings that aren't line-local, autosave."""
from __future__ import annotations

from pathlib import Path
from typing import Optional

from textual.app import App, ComposeResult
from textual.containers import VerticalScroll
from textual.widgets import Input, Static

from . import storage
from .models import Node, Outline
from .outline_view import Mode, OutlineView

NORMAL_HELP = (
    "i/a/I/A:insert  o/O:open  dd:delete  cc:change  hjkl:move  "
    "Enter:zoom-in  Space/za/zo/zc:fold  gg/G:top/bottom  x:del-char  "
    ">>/<<:indent  /:search  ^D:done  ^O:note  ^H:hide-done  ^S:save  ^Q:quit"
)
INSERT_HELP = "Esc:normal  Enter:new line  Tab/⇧Tab:indent  Backspace/Delete:edit  ^D:done  ^O:note"


class BulletrixApp(App):
    TITLE = "Bulletrix"
    CSS = """
    Screen {
        layout: vertical;
    }
    #breadcrumb {
        height: 1;
        padding: 0 1;
        color: $text-muted;
    }
    #outline-scroll {
        height: 1fr;
        border: round $primary;
        padding: 0 1;
    }
    #search-bar {
        height: 3;
        border: round $accent;
    }
    #status {
        height: 1;
        padding: 0 1;
        background: $panel;
        color: $text-muted;
    }
    """

    BINDINGS = [
        ("ctrl+f", "search", "Search"),
        ("ctrl+s", "save_now", "Save"),
        ("ctrl+h", "toggle_hide_completed", "Hide done"),
        ("ctrl+q", "quit", "Quit"),
        ("escape", "close_search", "Close search"),
    ]

    def __init__(self, path: Optional[Path] = None):
        super().__init__()
        self.path = path or storage.DEFAULT_PATH
        self.outline: Outline = storage.load(self.path)
        self.outline_view = OutlineView(self.outline, on_change=self._handle_change)

    def compose(self) -> ComposeResult:
        yield Static(self._breadcrumb_text(), id="breadcrumb")
        with VerticalScroll(id="outline-scroll"):
            yield self.outline_view
        yield Input(placeholder="Search… (Enter to jump, Esc to cancel)", id="search-bar")
        yield Static(self._status_text(), id="status")

    def on_mount(self) -> None:
        self.query_one("#search-bar", Input).display = False
        self.outline_view.focus()

    # -- state -> text -------------------------------------------------------
    def _breadcrumb_text(self) -> str:
        crumbs = []
        for n in self.outline.breadcrumb():
            if n.text:
                crumbs.append(n.text)
            elif n is self.outline.root:
                crumbs.append("Home")
            else:
                crumbs.append("(untitled)")
        return " › ".join(crumbs)

    def _status_text(self) -> str:
        hide = "on" if self.outline.hide_completed else "off"
        mode = self.outline_view.mode
        help_text = NORMAL_HELP if mode is Mode.NORMAL else INSERT_HELP
        return f"-- {mode.value} --  {help_text}  |  hide-done:{hide}"

    def _handle_change(self) -> None:
        self.query_one("#breadcrumb", Static).update(self._breadcrumb_text())
        self.query_one("#status", Static).update(self._status_text())
        storage.save(self.outline, self.path)

    def _reveal(self, node: Node) -> None:
        ancestor = node.parent
        while ancestor is not None:
            ancestor.collapsed = False
            ancestor = ancestor.parent
        self.outline.zoom_stack = [self.outline.root]
        self.outline_view.selected_id = node.id
        self.outline_view.cursor = 0
        self.outline_view.editing_note = False
        self.outline_view.mode = Mode.NORMAL
        self._handle_change()
        self.outline_view.refresh(layout=True)

    # -- actions -------------------------------------------------------------
    def action_search(self) -> None:
        bar = self.query_one("#search-bar", Input)
        bar.display = True
        bar.value = ""
        bar.focus()

    def action_close_search(self) -> None:
        bar = self.query_one("#search-bar", Input)
        if bar.display:
            bar.display = False
            self.outline_view.focus()

    def action_toggle_hide_completed(self) -> None:
        self.outline.hide_completed = not self.outline.hide_completed
        self._handle_change()
        self.outline_view.refresh()

    def action_save_now(self) -> None:
        storage.save(self.outline, self.path)
        self.query_one("#status", Static).update(self._status_text() + "  [saved]")

    def action_quit(self) -> None:
        storage.save(self.outline, self.path)
        self.exit()

    def on_input_submitted(self, event: Input.Submitted) -> None:
        if event.input.id != "search-bar":
            return
        matches = self.outline.search(event.value)
        self.action_close_search()
        if matches:
            self._reveal(matches[0])
