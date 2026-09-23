import sys
from pathlib import Path

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "src"))

from bulletrix.app import BulletrixApp  # noqa: E402


def make_app(tmp_path: Path) -> BulletrixApp:
    return BulletrixApp(path=tmp_path / "outline.json")


@pytest.mark.asyncio
async def test_typing_coalesces_into_single_undo_step(tmp_path):
    app = make_app(tmp_path)
    async with app.run_test() as pilot:
        await pilot.press("i")
        await pilot.press(*"abc")
        await pilot.press("escape")
        assert app.outline.root.children[0].text == "abc"

        await pilot.press("u")
        assert app.outline.root.children[0].text == ""

        await pilot.press("ctrl+r")
        assert app.outline.root.children[0].text == "abc"


@pytest.mark.asyncio
async def test_structural_ops_undo_independently(tmp_path):
    app = make_app(tmp_path)
    async with app.run_test() as pilot:
        await pilot.press("i")
        await pilot.press(*"a")
        await pilot.press("enter")
        await pilot.press(*"b")
        await pilot.press("escape")
        await pilot.press("tab")  # indent b under a
        await pilot.press("ctrl+d")  # mark b complete

        a_node = app.outline.root.children[0]
        assert a_node.text == "a"
        assert [c.text for c in a_node.children] == ["b"]
        assert a_node.children[0].completed is True

        await pilot.press("u")  # undo toggle-complete only
        # undo rebuilds the tree from a snapshot, so node objects captured
        # before an undo/redo go stale — re-fetch from the live outline.
        a_node = app.outline.root.children[0]
        assert a_node.children[0].completed is False
        assert [c.text for c in a_node.children] == ["b"]

        await pilot.press("u")  # undo indent only
        assert [c.text for c in app.outline.root.children] == ["a", "b"]

        await pilot.press("ctrl+r")  # redo indent
        a_node = app.outline.root.children[0]
        assert [c.text for c in a_node.children] == ["b"]


@pytest.mark.asyncio
async def test_new_edit_after_undo_clears_redo_stack(tmp_path):
    app = make_app(tmp_path)
    async with app.run_test() as pilot:
        await pilot.press("i")
        await pilot.press(*"a")
        await pilot.press("escape")
        await pilot.press("u")
        assert app.outline.root.children[0].text == ""

        await pilot.press("i")
        await pilot.press(*"x")
        await pilot.press("escape")
        assert app.outline.root.children[0].text == "x"

        await pilot.press("ctrl+r")  # redo stack was cleared by the new edit
        assert app.outline.root.children[0].text == "x"


@pytest.mark.asyncio
async def test_undo_on_empty_history_is_noop(tmp_path):
    app = make_app(tmp_path)
    async with app.run_test() as pilot:
        await pilot.press("u")
        assert app.outline.root.children[0].text == ""

        await pilot.press("i")
        await pilot.press(*"still works")
        await pilot.press("escape")
        assert app.outline.root.children[0].text == "still works"


@pytest.mark.asyncio
async def test_u_types_literal_u_in_insert_mode(tmp_path):
    app = make_app(tmp_path)
    async with app.run_test() as pilot:
        await pilot.press("i")
        await pilot.press(*"ab")
        await pilot.press("escape")

        await pilot.press("I")  # insert at start of line, cursor -> 0
        await pilot.press("u")  # in INSERT mode, "u" must type, not undo
        await pilot.press("escape")
        assert app.outline.root.children[0].text == "uab"


@pytest.mark.asyncio
async def test_undo_restores_selection_and_cursor(tmp_path):
    app = make_app(tmp_path)
    async with app.run_test() as pilot:
        await pilot.press("i")
        await pilot.press(*"ab")
        original_id = app.outline_view.selected_id
        await pilot.press("enter")  # INSERT-mode enter splits "ab" at cursor 2
        assert len(app.outline.root.children) == 2

        await pilot.press("escape")  # back to NORMAL mode, where "u" undoes
        await pilot.press("u")
        assert len(app.outline.root.children) == 1
        assert app.outline.root.children[0].text == "ab"
        assert app.outline_view.selected_id == original_id
        assert app.outline_view.cursor == 2
