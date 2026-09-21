import sys
import xml.etree.ElementTree as ET
from pathlib import Path

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "src"))

from bulletrix.models import Node, Outline  # noqa: E402
from bulletrix.opml_import import import_opml, parse_opml  # noqa: E402

SAMPLE_OPML = """<?xml version="1.0" encoding="UTF-8"?>
<opml version="2.0">
  <head><title>Bulletrix Export</title></head>
  <body>
    <outline text="Groceries" _note="weekly run">
      <outline text="Milk"/>
      <outline text="Eggs" _complete="true"/>
    </outline>
    <outline text="Work">
      <outline text="Write report">
        <outline text="Gather data"/>
        <outline text="Draft outline"/>
      </outline>
    </outline>
  </body>
</opml>
"""


def write_opml(tmp_path: Path, content: str = SAMPLE_OPML) -> Path:
    path = tmp_path / "export.opml"
    path.write_text(content, encoding="utf-8")
    return path


def test_parse_opml_builds_correct_tree_shape(tmp_path):
    path = write_opml(tmp_path)
    nodes = parse_opml(path)

    assert [n.text for n in nodes] == ["Groceries", "Work"]

    groceries = nodes[0]
    assert groceries.note == "weekly run"
    assert [c.text for c in groceries.children] == ["Milk", "Eggs"]
    assert groceries.children[0].completed is False
    assert groceries.children[1].completed is True

    work = nodes[1]
    report = work.children[0]
    assert report.text == "Write report"
    assert [c.text for c in report.children] == ["Gather data", "Draft outline"]


def test_parse_opml_sets_parent_pointers(tmp_path):
    path = write_opml(tmp_path)
    nodes = parse_opml(path)
    groceries = nodes[0]
    milk = groceries.children[0]
    assert milk.parent is groceries
    # top-level nodes are returned unattached — the caller decides the parent
    assert groceries.parent is None


def test_parse_opml_with_no_body_returns_empty_list(tmp_path):
    path = write_opml(
        tmp_path,
        '<?xml version="1.0"?><opml version="2.0"><head/></opml>',
    )
    assert parse_opml(path) == []


def test_parse_opml_accepts_alternate_complete_attribute_names(tmp_path):
    content = """<?xml version="1.0"?>
    <opml version="2.0"><body>
      <outline text="a" complete="true"/>
      <outline text="b" completed="1"/>
      <outline text="c" _completed="yes"/>
      <outline text="d"/>
    </body></opml>
    """
    path = write_opml(tmp_path, content)
    nodes = parse_opml(path)
    assert [n.completed for n in nodes] == [True, True, True, False]


def test_import_opml_merges_as_new_top_level_children_without_disturbing_existing(tmp_path):
    root = Node(text="")
    existing = Node(text="existing item")
    existing.parent = root
    root.children = [existing]
    outline = Outline(root)

    path = write_opml(tmp_path)
    added = import_opml(outline, path)

    assert added == 2
    texts = [c.text for c in outline.root.children]
    assert texts == ["existing item", "Groceries", "Work"]
    # imported nodes are properly attached to the real outline root
    imported_groceries = outline.root.children[1]
    assert imported_groceries.parent is outline.root
    assert imported_groceries.children[0].parent is imported_groceries


def test_import_opml_raises_parse_error_on_malformed_file(tmp_path):
    path = write_opml(tmp_path, "<not valid xml")
    outline = Outline()
    with pytest.raises(ET.ParseError):
        import_opml(outline, path)


def test_import_opml_raises_on_missing_file(tmp_path):
    outline = Outline()
    with pytest.raises(OSError):
        import_opml(outline, tmp_path / "does-not-exist.opml")
