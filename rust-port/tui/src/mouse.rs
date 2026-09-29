//! Mouse events -> `Action`s, using the `ScreenMap` from the last frame.

use crate::action::{Action, FoldOp};
use crate::editor::Editor;
use crate::ui::{LineKind, ScreenMap};
use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
use unicode_width::UnicodeWidthChar;

const WHEEL_LINES: isize = 3;

/// Events that don't do anything (moves, releases, other buttons) resolve to
/// no actions, which the caller uses to skip a redraw.
pub fn resolve(event: MouseEvent, screen: &ScreenMap, editor: &Editor) -> Vec<Action> {
    match event.kind {
        MouseEventKind::ScrollUp => vec![Action::Scroll(-WHEEL_LINES)],
        MouseEventKind::ScrollDown => vec![Action::Scroll(WHEEL_LINES)],
        MouseEventKind::Down(MouseButton::Left) => click(event.column, event.row, screen, editor),
        _ => Vec::new(),
    }
}

fn click(x: u16, y: u16, screen: &ScreenMap, editor: &Editor) -> Vec<Action> {
    let Some(line) = screen.lines.iter().find(|l| l.y == y) else {
        return Vec::new();
    };
    let node = line.node;
    let column = x.saturating_sub(line.text_x);
    match line.kind {
        // Left of the text: the bullet (folds, if there's anything to fold)
        // or the indentation before it (just selects).
        LineKind::Text { bullet_x, foldable } if x < line.text_x => {
            let mut actions = vec![Action::PlaceCursor { node, cursor: 0, note: false }];
            if foldable && x >= bullet_x {
                actions.push(Action::Fold(FoldOp::Toggle));
            }
            actions
        }
        LineKind::Text { .. } => {
            let cursor = column_to_char(&editor.outline.get(node).text, column);
            vec![Action::PlaceCursor { node, cursor, note: false }]
        }
        LineKind::Note { line: n } => {
            let mut note_lines = editor.outline.get(node).note.split('\n');
            let before: usize = note_lines.by_ref().take(n).map(|l| l.chars().count() + 1).sum();
            let cursor = before + column_to_char(note_lines.next().unwrap_or(""), column);
            vec![Action::PlaceCursor { node, cursor, note: true }]
        }
    }
}

/// The character at screen column `column` of `text`, counting wide
/// characters (e.g. CJK, most emoji) as two columns; past the end is the end.
fn column_to_char(text: &str, column: u16) -> usize {
    let mut x = 0;
    for (i, ch) in text.chars().enumerate() {
        x += ch.width().unwrap_or(0);
        if (column as usize) < x {
            return i;
        }
    }
    text.chars().count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::EditingStyle;
    use crate::editor::Mode;
    use crate::{keymap, ui};
    use crossterm::event::KeyModifiers;
    use model::{NodeId, Outline};
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    const WIDTH: u16 = 60;
    const HEIGHT: u16 = 10; // outline rows are y = 2..=7; text starts at x = 3

    /// Top-level items with the given texts; the first is selected.
    fn editor_with(texts: &[&str], style: EditingStyle) -> (Editor, Vec<NodeId>) {
        let mut outline = Outline::new();
        let root = outline.root();
        let mut ids = vec![outline.get(root).children[0]];
        outline.get_mut(ids[0]).text = texts[0].into();
        for text in &texts[1..] {
            let n = outline.create_node(*text);
            outline.insert_sibling_after(*ids.last().unwrap(), n);
            ids.push(n);
        }
        (Editor::new(outline, style), ids)
    }

    fn draw(editor: &mut Editor) -> ScreenMap {
        let mut terminal = Terminal::new(TestBackend::new(WIDTH, HEIGHT)).unwrap();
        let km = keymap::for_style(editor.style());
        let mut screen = ScreenMap::default();
        terminal
            .draw(|f| screen = ui::draw(f, editor, km.as_ref(), None))
            .unwrap();
        screen
    }

    fn event(kind: MouseEventKind, x: u16, y: u16) -> MouseEvent {
        MouseEvent { kind, column: x, row: y, modifiers: KeyModifiers::NONE }
    }

    /// Draws, then feeds one mouse event through, as the main loop does.
    fn mouse(editor: &mut Editor, kind: MouseEventKind, x: u16, y: u16) {
        let screen = draw(editor);
        for action in resolve(event(kind, x, y), &screen, editor) {
            editor.apply(action);
        }
    }

    fn click(editor: &mut Editor, x: u16, y: u16) {
        mouse(editor, MouseEventKind::Down(MouseButton::Left), x, y);
    }

    #[test]
    fn clicking_text_selects_the_item_and_places_the_cursor() {
        let (mut ed, ids) = editor_with(&["alpha", "bravo"], EditingStyle::Modal);
        click(&mut ed, 3 + 2, 3);
        assert_eq!(ed.selected, ids[1]);
        assert_eq!(ed.cursor, 2);
        assert_eq!(ed.mode(), Mode::Normal, "a click never changes mode");
    }

    #[test]
    fn clicking_past_the_end_follows_the_modes_cursor_rules() {
        let (mut ed, _) = editor_with(&["alpha"], EditingStyle::Modal);
        click(&mut ed, 40, 2);
        assert_eq!(ed.cursor, 4, "NORMAL: on the last character");

        let (mut ed, _) = editor_with(&["alpha"], EditingStyle::Traditional);
        click(&mut ed, 40, 2);
        assert_eq!(ed.cursor, 5, "INSERT: after the last character");
    }

    #[test]
    fn wide_characters_count_as_two_columns() {
        let (mut ed, _) = editor_with(&["日本語x"], EditingStyle::Traditional);
        click(&mut ed, 3 + 6, 2);
        assert_eq!(ed.cursor, 3);
        click(&mut ed, 3 + 3, 2); // right half of 本
        assert_eq!(ed.cursor, 1);
    }

    #[test]
    fn clicking_a_note_line_edits_the_note_at_that_spot() {
        let (mut ed, ids) = editor_with(&["alpha", "bravo"], EditingStyle::Traditional);
        ed.outline.get_mut(ids[1]).note = "first\nsecond".into();
        // bravo is y=3, its note lines y=4,5; note text starts at x = 1 + 4
        click(&mut ed, 5 + 2, 5);
        assert_eq!(ed.selected, ids[1]);
        assert!(ed.editing_note);
        assert_eq!(ed.cursor, "first\n".len() + 2);
    }

    #[test]
    fn clicking_a_bullet_with_children_folds_it_and_a_leaf_bullet_just_selects() {
        let (mut ed, ids) = editor_with(&["alpha", "bravo"], EditingStyle::Modal);
        let kid = ed.outline.create_node("kid");
        ed.outline.append_child(ids[0], kid);

        click(&mut ed, 1, 2);
        assert!(ed.outline.get(ids[0]).collapsed);
        assert_eq!(ed.selected, ids[0]);
        click(&mut ed, 1, 2);
        assert!(!ed.outline.get(ids[0]).collapsed);

        click(&mut ed, 1, 4); // bravo's bullet (kid is y=3)
        assert_eq!(ed.selected, ids[1]);
        assert_eq!(ed.cursor, 0);
    }

    #[test]
    fn a_click_cancels_a_jump_or_search_in_progress() {
        let (mut ed, ids) = editor_with(&["alpha", "bravo"], EditingStyle::Modal);
        ed.apply(Action::StartJump);
        click(&mut ed, 3, 3);
        assert!(!ed.jump_active());
        ed.apply(Action::OpenSearch);
        click(&mut ed, 3, 2);
        assert!(ed.search_input().is_none());
        assert_eq!(ed.selected, ids[0]);
    }

    #[test]
    fn clicks_on_empty_space_moves_and_releases_do_nothing() {
        let (mut ed, _) = editor_with(&["alpha"], EditingStyle::Modal);
        let screen = draw(&mut ed);
        for (kind, x, y) in [
            (MouseEventKind::Down(MouseButton::Left), 5, 6), // below the last row
            (MouseEventKind::Down(MouseButton::Left), 5, 0), // breadcrumb
            (MouseEventKind::Moved, 5, 2),
            (MouseEventKind::Up(MouseButton::Left), 5, 2),
            (MouseEventKind::Down(MouseButton::Right), 5, 2),
        ] {
            assert!(resolve(event(kind, x, y), &screen, &ed).is_empty(), "{kind:?}");
        }
    }

    #[test]
    fn wheel_scrolls_the_view_and_the_selection_follows_only_when_pushed_off_screen() {
        let texts: Vec<String> = (0..20).map(|i| format!("item {i}")).collect();
        let texts: Vec<&str> = texts.iter().map(String::as_str).collect();
        let (mut ed, ids) = editor_with(&texts, EditingStyle::Modal);
        ed.apply(Action::MoveDown);
        ed.apply(Action::MoveDown);
        ed.apply(Action::MoveDown); // item 3, still on screen after one notch

        mouse(&mut ed, MouseEventKind::ScrollDown, 5, 4);
        assert_eq!(ed.scroll_offset, 3);
        assert_eq!(ed.selected, ids[3], "still visible, so it stays put");

        mouse(&mut ed, MouseEventKind::ScrollDown, 5, 4);
        assert_eq!(ed.scroll_offset, 6);
        assert_eq!(ed.selected, ids[6], "pushed off the top, so it moves to the first visible row");

        draw(&mut ed);
        assert_eq!(ed.scroll_offset, 6, "the renderer keeps the scrolled view");

        for _ in 0..10 {
            mouse(&mut ed, MouseEventKind::ScrollDown, 5, 4);
        }
        assert_eq!(ed.scroll_offset, 20 - 6, "stops at the bottom");
        for _ in 0..10 {
            mouse(&mut ed, MouseEventKind::ScrollUp, 5, 4);
        }
        assert_eq!(ed.scroll_offset, 0, "stops at the top");
        assert_eq!(ed.selected, ids[5], "pushed off the bottom, so it moves to the last visible row");
    }
}
