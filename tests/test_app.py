import sys
from pathlib import Path

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "src"))

from bulletrix.app import BulletrixApp  # noqa: E402


def make_app(tmp_path: Path) -> BulletrixApp:
    return BulletrixApp(path=tmp_path / "outline.json")


@pytest.mark.asyncio
async def test_typing_creates_first_item(tmp_path):
    app = make_app(tmp_path)
    async with app.run_test() as pilot:
        await pilot.press(*"hello world")
        rows = app.outline.flatten()
        assert rows[0].node.text == "hello world"


@pytest.mark.asyncio
async def test_enter_splits_into_sibling_nodes(tmp_path):
    app = make_app(tmp_path)
    async with app.run_test() as pilot:
        await pilot.press(*"first")
        await pilot.press("enter")
        await pilot.press(*"second")
        rows = app.outline.flatten()
        texts = [r.node.text for r in rows]
        assert texts == ["first", "second"]
        assert app.outline_view.selected_id == rows[1].node.id


@pytest.mark.asyncio
async def test_tab_indents_and_shift_tab_outdents(tmp_path):
    app = make_app(tmp_path)
    async with app.run_test() as pilot:
        await pilot.press(*"a")
        await pilot.press("enter")
        await pilot.press(*"b")
        await pilot.press("tab")
        rows = app.outline.flatten()
        # a -> b nested under a
        a_node = app.outline.root.children[0]
        assert a_node.text == "a"
        assert [c.text for c in a_node.children] == ["b"]

        await pilot.press("shift+tab")
        assert [c.text for c in app.outline.root.children] == ["a", "b"]


@pytest.mark.asyncio
async def test_ctrl_up_moves_node_before_sibling(tmp_path):
    app = make_app(tmp_path)
    async with app.run_test() as pilot:
        await pilot.press(*"a")
        await pilot.press("enter")
        await pilot.press(*"b")
        await pilot.press("ctrl+up")
        assert [c.text for c in app.outline.root.children] == ["b", "a"]


@pytest.mark.asyncio
async def test_zoom_in_and_out(tmp_path):
    app = make_app(tmp_path)
    async with app.run_test() as pilot:
        await pilot.press(*"project")
        await pilot.press("ctrl+right")
        assert app.outline.zoom_root.text == "project"
        assert app.outline_view.selected_id == app.outline.zoom_root.id

        # header row (the zoomed node's own title) is directly editable;
        # Enter creates the first child row underneath it
        await pilot.press("enter")
        await pilot.press(*"task one")
        rows = app.outline.flatten()
        assert rows[0].node.text == "project"
        assert rows[1].node.text == "task one"

        await pilot.press("ctrl+left")
        assert app.outline.zoom_root is app.outline.root
        assert app.outline_view.selected_id == app.outline.root.children[0].id


@pytest.mark.asyncio
async def test_repeated_ctrl_right_on_header_does_not_duplicate_breadcrumbs(tmp_path):
    app = make_app(tmp_path)
    async with app.run_test() as pilot:
        await pilot.press(*"project")
        await pilot.press("ctrl+right")
        assert len(app.outline.zoom_stack) == 2

        # selection is now on the header row (the zoomed node itself);
        # hitting ctrl+right again must not push a duplicate onto the stack
        await pilot.press("ctrl+right")
        await pilot.press("ctrl+right")
        await pilot.press("ctrl+right")
        assert len(app.outline.zoom_stack) == 2
        assert app.outline.zoom_root.text == "project"

        await pilot.press("ctrl+left")
        assert app.outline.zoom_root is app.outline.root


@pytest.mark.asyncio
async def test_ctrl_d_toggles_completed(tmp_path):
    app = make_app(tmp_path)
    async with app.run_test() as pilot:
        await pilot.press(*"buy milk")
        await pilot.press("ctrl+d")
        node = app.outline.root.children[0]
        assert node.completed is True
        await pilot.press("ctrl+d")
        assert node.completed is False


@pytest.mark.asyncio
async def test_backspace_merges_into_previous_sibling(tmp_path):
    app = make_app(tmp_path)
    async with app.run_test() as pilot:
        await pilot.press(*"foo")
        await pilot.press("enter")
        await pilot.press(*"bar")
        await pilot.press("home")
        await pilot.press("backspace")
        rows = app.outline.flatten()
        assert [r.node.text for r in rows] == ["foobar"]
        assert app.outline_view.cursor == 3


@pytest.mark.asyncio
async def test_note_editing_with_ctrl_o(tmp_path):
    app = make_app(tmp_path)
    async with app.run_test() as pilot:
        await pilot.press(*"task")
        await pilot.press("ctrl+o")
        await pilot.press(*"a note here")
        node = app.outline.root.children[0]
        assert node.text == "task"
        assert node.note == "a note here"


@pytest.mark.asyncio
async def test_search_reveals_and_selects_match(tmp_path):
    app = make_app(tmp_path)
    async with app.run_test() as pilot:
        await pilot.press(*"alpha")
        await pilot.press("enter")
        await pilot.press(*"beta")
        await pilot.press("enter")
        await pilot.press(*"gamma")
        await pilot.press("ctrl+f")
        await pilot.press(*"beta")
        await pilot.press("enter")
        node = app.outline.root.children[1]
        assert node.text == "beta"
        assert app.outline_view.selected_id == node.id


@pytest.mark.asyncio
async def test_autosave_persists_to_disk_and_reloads(tmp_path):
    path = tmp_path / "outline.json"
    app = BulletrixApp(path=path)
    async with app.run_test() as pilot:
        await pilot.press(*"persisted item")
    assert path.exists()

    app2 = BulletrixApp(path=path)
    async with app2.run_test():
        rows = app2.outline.flatten()
        assert rows[0].node.text == "persisted item"


@pytest.mark.asyncio
async def test_reopening_restores_zoom_level_and_cursor_position(tmp_path):
    path = tmp_path / "outline.json"
    app = BulletrixApp(path=path)
    async with app.run_test() as pilot:
        await pilot.press("i")
        await pilot.press(*"project")
        await pilot.press("escape")
        await pilot.press("ctrl+right")  # zoom in on "project"
        await pilot.press("o")
        await pilot.press(*"task one")
        await pilot.press("escape")
        # plain cursor movement (no text edits) must still be captured at quit,
        # since it doesn't go through the autosave-on-change path
        await pilot.press("0")
        await pilot.press("l")
        selected_id = app.outline_view.selected_id
        cursor = app.outline_view.cursor
        app.action_quit()

    app2 = BulletrixApp(path=path)
    async with app2.run_test():
        assert app2.outline.zoom_root.text == "project"
        assert app2.outline_view.selected_id == selected_id
        assert app2.outline_view.cursor == cursor


@pytest.mark.asyncio
async def test_reopening_falls_back_to_first_row_when_selected_node_is_gone(tmp_path):
    path = tmp_path / "outline.json"
    app = BulletrixApp(path=path)
    async with app.run_test() as pilot:
        await pilot.press("i")
        await pilot.press(*"only item")
        await pilot.press("escape")
        # simulate a selection that no longer resolves (e.g. hand-edited file)
        app.outline_view.selected_id = "no-such-node-id"
        app.action_quit()

    app2 = BulletrixApp(path=path)
    async with app2.run_test():
        rows = app2.outline.flatten()
        assert app2.outline_view.selected_id == rows[0].node.id


@pytest.mark.asyncio
async def test_new_bullets_grow_the_visible_widget_height(tmp_path):
    """Regression test: refresh() alone repaints in place but does not
    resize the widget, so newly-added lines were being clipped out of
    view until the next full layout pass (e.g. on restart). Editing must
    request a layout, not just a repaint.
    """
    app = make_app(tmp_path)
    async with app.run_test() as pilot:
        await pilot.pause()
        height_before = app.outline_view.size.height
        assert height_before == 1

        await pilot.press(*"first")
        await pilot.press("enter")
        await pilot.press(*"second")
        await pilot.press("enter")
        await pilot.press(*"third")
        await pilot.pause()

        assert len(app.outline.flatten()) == 3
        assert app.outline_view.size.height == 3


@pytest.mark.asyncio
async def test_ctrl_k_collapses_and_hides_children(tmp_path):
    app = make_app(tmp_path)
    async with app.run_test() as pilot:
        await pilot.press(*"parent")
        await pilot.press("enter")
        await pilot.press(*"child")
        await pilot.press("tab")
        # move selection back up to "parent"
        await pilot.press("up")
        await pilot.press("ctrl+k")
        rows = app.outline.flatten()
        assert [r.node.text for r in rows] == ["parent"]
        await pilot.press("ctrl+k")
        rows = app.outline.flatten()
        assert [r.node.text for r in rows] == ["parent", "child"]
