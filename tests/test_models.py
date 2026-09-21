import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "src"))

from bulletrix.models import Node, Outline  # noqa: E402


def make_outline():
    root = Node(text="Home")
    outline = Outline(root)
    a = Node(text="A")
    b = Node(text="B")
    c = Node(text="C")
    for n in (a, b, c):
        n.parent = root
    root.children = [a, b, c]
    return outline, a, b, c


def test_flatten_basic_order():
    outline, a, b, c = make_outline()
    rows = outline.flatten()
    # true root never renders its own row/header, only its children
    assert [r.node.text for r in rows] == ["A", "B", "C"]
    assert all(not r.is_header for r in rows)


def test_flatten_shows_header_only_when_zoomed_into_a_non_root_node():
    outline, a, b, c = make_outline()
    outline.zoom_in(a)
    rows = outline.flatten()
    assert rows[0].node is a
    assert rows[0].is_header is True


def test_indent_makes_previous_sibling_the_parent():
    outline, a, b, c = make_outline()
    assert outline.indent(b) is True
    assert b.parent is a
    assert a.children == [b]
    assert outline.root.children == [a, c]


def test_indent_first_child_is_noop():
    outline, a, b, c = make_outline()
    assert outline.indent(a) is False
    assert outline.root.children == [a, b, c]


def test_outdent_moves_node_up_one_level_keeping_earlier_siblings_in_place():
    outline, a, b, c = make_outline()
    outline.indent(b)  # root=[a, c]; a=[b]
    outline.indent(c)  # c's previous sibling in root is now a -> root=[a]; a=[b, c]
    assert outline.root.children == [a]
    assert a.children == [b, c]

    assert outline.outdent(c) is True
    assert c.parent is outline.root
    assert outline.root.children == [a, c]
    assert a.children == [b]


def test_move_up_and_down_swap_siblings():
    outline, a, b, c = make_outline()
    assert outline.move_down(a) is True
    assert outline.root.children == [b, a, c]
    assert outline.move_up(a) is True
    assert outline.root.children == [a, b, c]
    assert outline.move_up(a) is False  # already first


def test_merge_backward_into_previous_sibling_appends_text_and_children():
    outline, a, b, c = make_outline()
    grandchild = Node(text="child-of-c")
    grandchild.parent = c
    c.children = [grandchild]

    b.text = "B"
    result = outline.merge_backward(c)
    assert result is b
    assert b.text == "BC"
    assert b._merge_cursor == 1
    assert grandchild in b.children
    assert grandchild.parent is b
    assert outline.root.children == [a, b]


def test_merge_backward_first_child_merges_into_parent():
    outline, a, b, c = make_outline()
    outline.zoom_in(a)
    child = Node(text="child")
    child.parent = a
    a.children = [child]

    result = outline.merge_backward(child)
    assert result is a
    assert a.text == "Achild"
    assert a.children == []


def test_zoom_in_and_out():
    outline, a, b, c = make_outline()
    assert outline.zoom_root is outline.root
    outline.zoom_in(b)
    assert outline.zoom_root is b
    popped = outline.zoom_out()
    assert popped is b
    assert outline.zoom_root is outline.root
    assert outline.zoom_out() is None  # can't pop past the root


def test_flatten_respects_collapsed_and_hide_completed():
    outline, a, b, c = make_outline()
    child = Node(text="hidden-child")
    child.parent = a
    a.children = [child]
    a.collapsed = True
    rows = outline.flatten()
    assert [r.node.text for r in rows] == ["A", "B", "C"]

    a.collapsed = False
    rows = outline.flatten()
    assert [r.node.text for r in rows] == ["A", "hidden-child", "B", "C"]

    b.completed = True
    outline.hide_completed = True
    rows = outline.flatten()
    assert "B" not in [r.node.text for r in rows]


def test_serialization_round_trip():
    outline, a, b, c = make_outline()
    a.note = "a note"
    b.completed = True
    child = Node(text="nested #tag")
    child.parent = a
    a.children = [child]

    data = outline.to_dict()
    restored = Outline.from_dict(data)

    assert restored.root.text == "Home"
    assert [c.text for c in restored.root.children] == ["A", "B", "C"]
    restored_a = restored.root.children[0]
    assert restored_a.note == "a note"
    assert restored_a.children[0].text == "nested #tag"
    assert restored_a.children[0].parent is restored_a
    assert restored.root.children[1].completed is True


def test_tags_extraction():
    n = Node(text="buy milk #groceries @home")
    assert n.tags() == ["#groceries", "@home"]


def test_search_finds_text_and_note_matches():
    outline, a, b, c = make_outline()
    a.note = "special note"
    rows = outline.search("special")
    assert rows == [a]
    rows = outline.search("b")
    assert rows == [b]
