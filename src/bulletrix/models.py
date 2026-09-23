"""Core outline data model: nodes, tree operations, zoom/flatten logic."""
from __future__ import annotations

import re
import time
import uuid
from dataclasses import dataclass, field
from typing import Optional

TAG_RE = re.compile(r"(?<!\w)([#@][\w][\w-]*)")


@dataclass
class Node:
    text: str = ""
    note: str = ""
    completed: bool = False
    collapsed: bool = False
    id: str = field(default_factory=lambda: uuid.uuid4().hex)
    created: float = field(default_factory=time.time)
    modified: float = field(default_factory=time.time)
    children: list["Node"] = field(default_factory=list)
    parent: Optional["Node"] = field(default=None, repr=False, compare=False)

    def tags(self) -> list[str]:
        return TAG_RE.findall(self.text) + TAG_RE.findall(self.note)

    def touch(self) -> None:
        self.modified = time.time()

    def to_dict(self) -> dict:
        return {
            "id": self.id,
            "text": self.text,
            "note": self.note,
            "completed": self.completed,
            "collapsed": self.collapsed,
            "created": self.created,
            "modified": self.modified,
            "children": [c.to_dict() for c in self.children],
        }

    @classmethod
    def from_dict(cls, data: dict, parent: Optional["Node"] = None) -> "Node":
        node = cls(
            text=data.get("text", ""),
            note=data.get("note", ""),
            completed=data.get("completed", False),
            collapsed=data.get("collapsed", False),
            id=data.get("id") or uuid.uuid4().hex,
            created=data.get("created", time.time()),
            modified=data.get("modified", time.time()),
            parent=parent,
        )
        node.children = [Node.from_dict(c, parent=node) for c in data.get("children", [])]
        return node


@dataclass
class Row:
    """One visible row in the flattened outline view."""

    node: Node
    depth: int
    is_header: bool = False
    has_children: bool = False
    visible_children: bool = False


class Outline:
    """Owns the tree and all structural mutations. No UI concerns here."""

    def __init__(self, root: Optional[Node] = None):
        self.root = root or Node(text="")
        if not self.root.children:
            first = Node()
            first.parent = self.root
            self.root.children.append(first)
        self.zoom_stack: list[Node] = [self.root]
        self.hide_completed: bool = False
        self.selected_id: Optional[str] = None
        self.cursor: int = 0

    def find(self, node_id: str) -> Optional[Node]:
        def walk(node: Node) -> Optional[Node]:
            if node.id == node_id:
                return node
            for child in node.children:
                found = walk(child)
                if found is not None:
                    return found
            return None

        return walk(self.root)

    # -- persistence -----------------------------------------------------
    def to_dict(self) -> dict:
        return {
            "root": self.root.to_dict(),
            "hide_completed": self.hide_completed,
            "zoom_stack": [n.id for n in self.zoom_stack],
            "selected_id": self.selected_id,
            "cursor": self.cursor,
        }

    @classmethod
    def from_dict(cls, data: dict) -> "Outline":
        root = Node.from_dict(data["root"])
        outline = cls(root)
        outline.hide_completed = data.get("hide_completed", False)

        zoom_stack = [outline.root]
        for node_id in data.get("zoom_stack", [])[1:]:
            node = outline.find(node_id)
            if node is None:
                break
            zoom_stack.append(node)
        outline.zoom_stack = zoom_stack

        outline.selected_id = data.get("selected_id")
        outline.cursor = data.get("cursor", 0)
        return outline

    # -- zoom --------------------------------------------------------------
    @property
    def zoom_root(self) -> Node:
        return self.zoom_stack[-1]

    def zoom_in(self, node: Node) -> None:
        self.zoom_stack.append(node)

    def zoom_out(self) -> Optional[Node]:
        if len(self.zoom_stack) <= 1:
            return None
        child = self.zoom_stack.pop()
        return child

    def breadcrumb(self) -> list[Node]:
        return list(self.zoom_stack)

    # -- flatten for display -------------------------------------------
    def flatten(self) -> list[Row]:
        rows: list[Row] = []
        root = self.zoom_root
        is_true_root = root is self.root
        if not is_true_root:
            rows.append(
                Row(
                    node=root,
                    depth=0,
                    is_header=True,
                    has_children=bool(root.children),
                    visible_children=bool(root.children) and not root.collapsed,
                )
            )

        def walk(node: Node, depth: int) -> None:
            for child in node.children:
                if self.hide_completed and child.completed:
                    continue
                has_kids = bool(child.children)
                expanded = has_kids and not child.collapsed
                rows.append(
                    Row(
                        node=child,
                        depth=depth,
                        has_children=has_kids,
                        visible_children=expanded,
                    )
                )
                if expanded:
                    walk(child, depth + 1)

        if is_true_root:
            walk(root, 0)
        elif not root.collapsed:
            walk(root, 1)
        return rows

    # -- lookups -----------------------------------------------------------
    @staticmethod
    def index_in_parent(node: Node) -> int:
        assert node.parent is not None
        return node.parent.children.index(node)

    # -- structural mutations --------------------------------------------
    def insert_sibling_after(self, node: Node, new_node: Node) -> None:
        parent = node.parent
        if parent is None:
            # node is a zoom root with no real parent in this stack; treat as child
            node.children.insert(0, new_node)
            new_node.parent = node
            return
        idx = self.index_in_parent(node)
        new_node.parent = parent
        parent.children.insert(idx + 1, new_node)

    def add_first_child(self, node: Node, new_node: Node) -> None:
        new_node.parent = node
        node.children.insert(0, new_node)

    def remove(self, node: Node) -> None:
        parent = node.parent
        if parent is None:
            return
        parent.children.remove(node)
        node.parent = None

    def indent(self, node: Node) -> bool:
        parent = node.parent
        if parent is None:
            return False
        idx = self.index_in_parent(node)
        if idx == 0:
            return False
        prev_sibling = parent.children[idx - 1]
        parent.children.pop(idx)
        node.parent = prev_sibling
        prev_sibling.children.append(node)
        prev_sibling.collapsed = False
        return True

    def outdent(self, node: Node) -> bool:
        parent = node.parent
        if parent is None or parent.parent is None:
            return False
        grandparent = parent.parent
        idx = self.index_in_parent(node)
        parent.children.pop(idx)
        node.parent = grandparent
        gp_idx = self.index_in_parent(parent)
        grandparent.children.insert(gp_idx + 1, node)
        return True

    def move_up(self, node: Node) -> bool:
        parent = node.parent
        if parent is None:
            return False
        idx = self.index_in_parent(node)
        if idx == 0:
            return False
        parent.children[idx - 1], parent.children[idx] = (
            parent.children[idx],
            parent.children[idx - 1],
        )
        return True

    def move_down(self, node: Node) -> bool:
        parent = node.parent
        if parent is None:
            return False
        idx = self.index_in_parent(node)
        if idx >= len(parent.children) - 1:
            return False
        parent.children[idx + 1], parent.children[idx] = (
            parent.children[idx],
            parent.children[idx + 1],
        )
        return True

    def merge_backward(self, node: Node) -> Optional[Node]:
        """Merge `node` into its previous sibling (or parent if first child).

        Returns the node that now holds focus (the merge target) with a
        `_merge_cursor` attribute set on it indicating cursor offset, or
        None if no merge target exists.
        """
        parent = node.parent
        if parent is None:
            return None
        idx = self.index_in_parent(node)
        if idx > 0:
            target = parent.children[idx - 1]
            # descend into target's last visible-ish descendant would change
            # depth semantics; keep it simple: merge into the sibling itself.
            offset = len(target.text)
            target.text += node.text
            target.children = node.children + target.children
            for c in target.children:
                c.parent = target
            parent.children.pop(idx)
            target._merge_cursor = offset  # type: ignore[attr-defined]
            return target
        else:
            if parent.parent is None:
                return None  # don't merge into the true home root
            target = parent
            offset = len(target.text)
            target.text += node.text
            parent.children.pop(idx)
            # node's children become children of parent, inserted at front
            for c in reversed(node.children):
                c.parent = target
                target.children.insert(0, c)
            target._merge_cursor = offset  # type: ignore[attr-defined]
            return target

    def toggle_complete(self, node: Node) -> None:
        node.completed = not node.completed
        node.touch()

    def toggle_collapsed(self, node: Node) -> None:
        node.collapsed = not node.collapsed

    def search(self, query: str) -> list[Node]:
        query = query.lower().strip()
        if not query:
            return []
        results: list[Node] = []

        def walk(node: Node) -> None:
            for child in node.children:
                haystack = f"{child.text} {child.note}".lower()
                if query in haystack:
                    results.append(child)
                walk(child)

        walk(self.root)
        return results
