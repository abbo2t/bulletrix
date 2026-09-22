import sys
from pathlib import Path

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "src"))

from bulletrix.app import BulletrixApp  # noqa: E402
from bulletrix.outline_view import Mode  # noqa: E402


def make_app(tmp_path: Path) -> BulletrixApp:
    return BulletrixApp(path=tmp_path / "outline.json")


@pytest.mark.asyncio
async def test_starts_in_normal_mode_and_swallows_typing(tmp_path):
    app = make_app(tmp_path)
    async with app.run_test() as pilot:
        assert app.outline_view.mode is Mode.NORMAL
        await pilot.press(*"hello")
        rows = app.outline.flatten()
        assert rows[0].node.text == ""


@pytest.mark.asyncio
async def test_i_enters_insert_mode_and_types(tmp_path):
    app = make_app(tmp_path)
    async with app.run_test() as pilot:
        await pilot.press("i")
        assert app.outline_view.mode is Mode.INSERT
        await pilot.press(*"hello world")
        rows = app.outline.flatten()
        assert rows[0].node.text == "hello world"


@pytest.mark.asyncio
async def test_escape_returns_to_normal_mode(tmp_path):
    app = make_app(tmp_path)
    async with app.run_test() as pilot:
        await pilot.press("i")
        await pilot.press(*"abc")
        await pilot.press("escape")
        assert app.outline_view.mode is Mode.NORMAL
        # "qwe" aren't bound to any NORMAL-mode command, so they're swallowed
        # rather than typed (unlike e.g. "x", which is a real vim command)
        await pilot.press(*"qwe")
        rows = app.outline.flatten()
        assert rows[0].node.text == "abc"


@pytest.mark.asyncio
async def test_a_appends_after_cursor(tmp_path):
    app = make_app(tmp_path)
    async with app.run_test() as pilot:
        await pilot.press("i")
        await pilot.press(*"ac")
        await pilot.press("escape")
        await pilot.press("0")  # normal-mode: jump to start of line
        await pilot.press("a")
        await pilot.press(*"b")
        rows = app.outline.flatten()
        assert rows[0].node.text == "abc"


@pytest.mark.asyncio
async def test_capital_I_and_A_jump_to_line_start_and_end(tmp_path):
    app = make_app(tmp_path)
    async with app.run_test() as pilot:
        await pilot.press("i")
        await pilot.press(*"bcd")
        await pilot.press("escape")
        await pilot.press("I")
        await pilot.press(*"a")
        await pilot.press("escape")
        await pilot.press("A")
        await pilot.press(*"e")
        rows = app.outline.flatten()
        assert rows[0].node.text == "abcde"


@pytest.mark.asyncio
async def test_enter_splits_into_sibling_nodes(tmp_path):
    app = make_app(tmp_path)
    async with app.run_test() as pilot:
        await pilot.press("i")
        await pilot.press(*"first")
        await pilot.press("enter")
        await pilot.press(*"second")
        rows = app.outline.flatten()
        texts = [r.node.text for r in rows]
        assert texts == ["first", "second"]
        assert app.outline_view.selected_id == rows[1].node.id
        assert app.outline_view.mode is Mode.INSERT


@pytest.mark.asyncio
async def test_o_opens_new_sibling_below_and_enters_insert(tmp_path):
    app = make_app(tmp_path)
    async with app.run_test() as pilot:
        await pilot.press("i")
        await pilot.press(*"first")
        await pilot.press("escape")
        await pilot.press("o")
        assert app.outline_view.mode is Mode.INSERT
        await pilot.press(*"second")
        rows = app.outline.flatten()
        assert [r.node.text for r in rows] == ["first", "second"]


@pytest.mark.asyncio
async def test_capital_o_opens_new_sibling_above(tmp_path):
    app = make_app(tmp_path)
    async with app.run_test() as pilot:
        await pilot.press("i")
        await pilot.press(*"second")
        await pilot.press("escape")
        await pilot.press("O")
        await pilot.press(*"first")
        rows = app.outline.flatten()
        assert [r.node.text for r in rows] == ["first", "second"]


@pytest.mark.asyncio
async def test_dd_deletes_current_node(tmp_path):
    app = make_app(tmp_path)
    async with app.run_test() as pilot:
        await pilot.press("i")
        await pilot.press(*"a")
        await pilot.press("enter")
        await pilot.press(*"b")
        await pilot.press("escape")
        await pilot.press("d")
        await pilot.press("d")
        rows = app.outline.flatten()
        assert [r.node.text for r in rows] == ["a"]


@pytest.mark.asyncio
async def test_dd_refuses_to_delete_the_only_top_level_item(tmp_path):
    app = make_app(tmp_path)
    async with app.run_test() as pilot:
        await pilot.press("i")
        await pilot.press(*"only")
        await pilot.press("escape")
        await pilot.press("d")
        await pilot.press("d")
        rows = app.outline.flatten()
        assert [r.node.text for r in rows] == ["only"]


@pytest.mark.asyncio
async def test_h_in_normal_mode_zooms_out(tmp_path):
    app = make_app(tmp_path)
    async with app.run_test() as pilot:
        await pilot.press("i")
        await pilot.press(*"project")
        await pilot.press("escape")
        await pilot.press("enter")  # zoom in
        assert app.outline.zoom_root.text == "project"

        await pilot.press("H")  # zoom back out
        assert app.outline.zoom_root is app.outline.root


@pytest.mark.asyncio
async def test_l_in_normal_mode_zooms_in(tmp_path):
    app = make_app(tmp_path)
    async with app.run_test() as pilot:
        await pilot.press("i")
        await pilot.press(*"project")
        await pilot.press("escape")
        await pilot.press("L")  # zoom in
        assert app.outline.zoom_root.text == "project"

        await pilot.press("H")  # zoom back out
        assert app.outline.zoom_root is app.outline.root


@pytest.mark.asyncio
async def test_escape_does_not_zoom_out(tmp_path):
    app = make_app(tmp_path)
    async with app.run_test() as pilot:
        await pilot.press("i")
        await pilot.press(*"project")
        await pilot.press("escape")
        await pilot.press("enter")  # zoom in
        assert app.outline.zoom_root.text == "project"

        await pilot.press("escape")  # should NOT zoom out
        assert app.outline.zoom_root.text == "project"


@pytest.mark.asyncio
async def test_escape_cancels_pending_command_without_zooming_out(tmp_path):
    app = make_app(tmp_path)
    async with app.run_test() as pilot:
        await pilot.press("i")
        await pilot.press(*"project")
        await pilot.press("escape")
        await pilot.press("enter")  # zoom in
        assert app.outline.zoom_root.text == "project"

        await pilot.press("d")  # start a pending "dd" sequence
        await pilot.press("escape")  # cancel it — should NOT also zoom out
        assert app.outline.zoom_root.text == "project"
        assert app.outline_view._pending is None


@pytest.mark.asyncio
async def test_q_quits_in_normal_mode(tmp_path):
    app = make_app(tmp_path)
    async with app.run_test() as pilot:
        called = []
        app.action_quit = lambda: called.append(True)
        await pilot.press("q")
        assert called == [True]


@pytest.mark.asyncio
async def test_q_types_normally_in_insert_mode(tmp_path):
    app = make_app(tmp_path)
    async with app.run_test() as pilot:
        called = []
        app.action_quit = lambda: called.append(True)
        await pilot.press("i")
        await pilot.press(*"quit")
        assert called == []
        rows = app.outline.flatten()
        assert rows[0].node.text == "quit"


@pytest.mark.asyncio
async def test_cc_clears_line_and_enters_insert(tmp_path):
    app = make_app(tmp_path)
    async with app.run_test() as pilot:
        await pilot.press("i")
        await pilot.press(*"old text")
        await pilot.press("escape")
        await pilot.press("c")
        await pilot.press("c")
        assert app.outline_view.mode is Mode.INSERT
        await pilot.press(*"new")
        rows = app.outline.flatten()
        assert rows[0].node.text == "new"


@pytest.mark.asyncio
async def test_x_deletes_character_under_cursor(tmp_path):
    app = make_app(tmp_path)
    async with app.run_test() as pilot:
        await pilot.press("i")
        await pilot.press(*"abcd")
        await pilot.press("escape")
        await pilot.press("0")
        await pilot.press("x")
        rows = app.outline.flatten()
        assert rows[0].node.text == "bcd"


@pytest.mark.asyncio
async def test_hjkl_navigate_like_arrows(tmp_path):
    app = make_app(tmp_path)
    async with app.run_test() as pilot:
        await pilot.press("i")
        await pilot.press(*"a")
        await pilot.press("enter")
        await pilot.press(*"b")
        await pilot.press("escape")
        await pilot.press("k")  # up to "a"
        rows = app.outline.flatten()
        assert app.outline_view.selected_id == rows[0].node.id
        await pilot.press("j")  # down to "b"
        assert app.outline_view.selected_id == rows[1].node.id
        await pilot.press("l")  # cursor right within "b" (len 1)
        assert app.outline_view.cursor == 1
        await pilot.press("h")  # cursor back left
        assert app.outline_view.cursor == 0


@pytest.mark.asyncio
async def test_gg_and_G_jump_to_top_and_bottom(tmp_path):
    app = make_app(tmp_path)
    async with app.run_test() as pilot:
        await pilot.press("i")
        await pilot.press(*"a")
        await pilot.press("enter")
        await pilot.press(*"b")
        await pilot.press("enter")
        await pilot.press(*"c")
        await pilot.press("escape")
        rows = app.outline.flatten()

        await pilot.press("g")
        await pilot.press("g")
        assert app.outline_view.selected_id == rows[0].node.id

        await pilot.press("G")
        assert app.outline_view.selected_id == rows[2].node.id


@pytest.mark.asyncio
async def test_za_toggles_fold_on_children(tmp_path):
    app = make_app(tmp_path)
    async with app.run_test() as pilot:
        await pilot.press("i")
        await pilot.press(*"parent")
        await pilot.press("enter")
        await pilot.press(*"child")
        await pilot.press("tab")
        await pilot.press("escape")
        await pilot.press("k")  # select "parent"

        await pilot.press("z")
        await pilot.press("a")
        rows = app.outline.flatten()
        assert [r.node.text for r in rows] == ["parent"]

        await pilot.press("z")
        await pilot.press("a")
        rows = app.outline.flatten()
        assert [r.node.text for r in rows] == ["parent", "child"]


@pytest.mark.asyncio
async def test_space_toggles_fold_on_children(tmp_path):
    app = make_app(tmp_path)
    async with app.run_test() as pilot:
        await pilot.press("i")
        await pilot.press(*"parent")
        await pilot.press("enter")
        await pilot.press(*"child")
        await pilot.press("tab")
        await pilot.press("escape")
        await pilot.press("k")  # select "parent"

        await pilot.press("space")
        rows = app.outline.flatten()
        assert [r.node.text for r in rows] == ["parent"]

        await pilot.press("space")
        rows = app.outline.flatten()
        assert [r.node.text for r in rows] == ["parent", "child"]


@pytest.mark.asyncio
async def test_enter_in_normal_mode_zooms_in(tmp_path):
    app = make_app(tmp_path)
    async with app.run_test() as pilot:
        await pilot.press("i")
        await pilot.press(*"project")
        await pilot.press("escape")
        await pilot.press("enter")
        assert app.outline.zoom_root.text == "project"
        assert app.outline_view.mode is Mode.NORMAL


@pytest.mark.asyncio
async def test_enter_in_normal_mode_on_header_is_a_noop(tmp_path):
    app = make_app(tmp_path)
    async with app.run_test() as pilot:
        await pilot.press("i")
        await pilot.press(*"project")
        await pilot.press("escape")
        await pilot.press("enter")  # zoom in; selection is now the header row
        stack_depth = len(app.outline.zoom_stack)
        await pilot.press("enter")  # pressing it again on the header must not re-zoom
        assert len(app.outline.zoom_stack) == stack_depth


@pytest.mark.asyncio
async def test_tab_indents_and_shift_tab_outdents(tmp_path):
    app = make_app(tmp_path)
    async with app.run_test() as pilot:
        await pilot.press("i")
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
        await pilot.press("i")
        await pilot.press(*"a")
        await pilot.press("enter")
        await pilot.press(*"b")
        await pilot.press("ctrl+up")
        assert [c.text for c in app.outline.root.children] == ["b", "a"]


@pytest.mark.asyncio
async def test_zoom_in_and_out(tmp_path):
    app = make_app(tmp_path)
    async with app.run_test() as pilot:
        await pilot.press("i")
        await pilot.press(*"project")
        await pilot.press("ctrl+right")
        assert app.outline.zoom_root.text == "project"
        assert app.outline_view.selected_id == app.outline.zoom_root.id
        # zooming in always drops back to NORMAL mode
        assert app.outline_view.mode is Mode.NORMAL

        # "o" on the header opens the page's first child and enters insert
        await pilot.press("o")
        await pilot.press(*"task one")
        rows = app.outline.flatten()
        assert rows[0].node.text == "project"
        assert rows[1].node.text == "task one"

        await pilot.press("escape")
        await pilot.press("ctrl+left")
        assert app.outline.zoom_root is app.outline.root
        assert app.outline_view.selected_id == app.outline.root.children[0].id


@pytest.mark.asyncio
async def test_repeated_ctrl_right_on_header_does_not_duplicate_breadcrumbs(tmp_path):
    app = make_app(tmp_path)
    async with app.run_test() as pilot:
        await pilot.press("i")
        await pilot.press(*"project")
        await pilot.press("escape")
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
        await pilot.press("i")
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
        await pilot.press("i")
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
        await pilot.press("i")
        await pilot.press(*"task")
        await pilot.press("ctrl+o")
        assert app.outline_view.mode is Mode.INSERT
        await pilot.press(*"a note here")
        node = app.outline.root.children[0]
        assert node.text == "task"
        assert node.note == "a note here"


@pytest.mark.asyncio
async def test_search_reveals_and_selects_match(tmp_path):
    app = make_app(tmp_path)
    async with app.run_test() as pilot:
        await pilot.press("i")
        await pilot.press(*"alpha")
        await pilot.press("enter")
        await pilot.press(*"beta")
        await pilot.press("enter")
        await pilot.press(*"gamma")
        await pilot.press("escape")
        await pilot.press("/")
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
        await pilot.press("i")
        await pilot.press(*"persisted item")
    assert path.exists()

    app2 = BulletrixApp(path=path)
    async with app2.run_test():
        rows = app2.outline.flatten()
        assert rows[0].node.text == "persisted item"


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

        await pilot.press("i")
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
        await pilot.press("i")
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
