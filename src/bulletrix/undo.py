"""A bounded undo/redo stack of opaque state snapshots."""
from __future__ import annotations

from typing import Optional

MAX_HISTORY = 200


class UndoStack:
    def __init__(self, max_history: int = MAX_HISTORY):
        self._undo: list[dict] = []
        self._redo: list[dict] = []
        self._max_history = max_history

    @property
    def can_undo(self) -> bool:
        return bool(self._undo)

    @property
    def can_redo(self) -> bool:
        return bool(self._redo)

    def checkpoint(self, snapshot: dict) -> None:
        """Record `snapshot` as the state *before* an edit, and drop the redo branch."""
        self._undo.append(snapshot)
        if len(self._undo) > self._max_history:
            self._undo.pop(0)
        self._redo.clear()

    def undo(self, current: dict) -> Optional[dict]:
        """Return the previous snapshot, pushing `current` onto the redo stack."""
        if not self._undo:
            return None
        self._redo.append(current)
        return self._undo.pop()

    def redo(self, current: dict) -> Optional[dict]:
        """Return the next snapshot, pushing `current` back onto the undo stack."""
        if not self._redo:
            return None
        self._undo.append(current)
        return self._redo.pop()
