"""Import outlines from OPML files (the format WorkFlowy and most other
outliners use for structured export/interop).

WorkFlowy's own OPML export uses a `_note` attribute for the note field;
there is no single agreed-upon attribute name for "completed", so a few
plausible variants are checked and it defaults to False if none are present.
"""
from __future__ import annotations

import xml.etree.ElementTree as ET
from pathlib import Path

from .models import Node, Outline

_TRUE_STRINGS = {"true", "1", "yes"}
_COMPLETE_ATTRS = ("_complete", "complete", "_completed", "completed")


def _parse_outline_element(elem: ET.Element) -> Node:
    text = elem.get("text") or ""
    note = elem.get("_note") or elem.get("note") or ""
    completed = False
    for attr in _COMPLETE_ATTRS:
        value = elem.get(attr)
        if value is not None:
            completed = value.strip().lower() in _TRUE_STRINGS
            break
    node = Node(text=text, note=note, completed=completed)
    for child_elem in elem.findall("outline"):
        child = _parse_outline_element(child_elem)
        child.parent = node
        node.children.append(child)
    return node


def parse_opml(path: Path) -> list[Node]:
    """Parse an OPML file into a list of top-level Node trees (unattached)."""
    tree = ET.parse(path)
    root = tree.getroot()
    body = root.find("body")
    if body is None:
        return []
    return [_parse_outline_element(elem) for elem in body.findall("outline")]


def import_opml(outline: Outline, path: Path) -> int:
    """Merge an OPML file's top-level items into `outline` as new top-level
    children (existing content is left untouched). Returns the number of
    top-level items added.
    """
    new_nodes = parse_opml(path)
    for node in new_nodes:
        node.parent = outline.root
        outline.root.children.append(node)
    return len(new_nodes)
