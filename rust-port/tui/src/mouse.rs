//! Mouse events -> `Action`s, using the `ScreenMap` from the last frame.

use crate::action::{Action, FoldOp, SearchOp};
use crate::editor::Editor;
use crate::ui::{LineKind, ScreenMap};
use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
use model::NodeId;
use std::time::{Duration, Instant};
use unicode_width::UnicodeWidthChar;

const WHEEL_LINES: isize = 3;
const DOUBLE_CLICK: Duration = Duration::from_millis(400);

/// Turns mouse events into actions. Stateful only to spot double-clicks,
/// which terminals don't report themselves.
#[derive(Default)]
pub struct Mouse {
    /// When and on which item's text the last click landed.
    last_text_click: Option<(Instant, NodeId)>,
}

impl Mouse {
    /// Events that don't do anything (moves, releases, other buttons)
    /// resolve to no actions, which the caller uses to skip a redraw.
    pub fn resolve(&mut self, event: MouseEvent, screen: &ScreenMap, editor: &Editor, now: Instant) -> Vec<Action> {
        match event.kind {
            MouseEventKind::ScrollUp => vec![Action::Scroll(-WHEEL_LINES)],
            MouseEventKind::ScrollDown => vec![Action::Scroll(WHEEL_LINES)],
            MouseEventKind::Down(MouseButton::Left) => {
                let last = self.last_text_click.take();
                self.click(event.column, event.row, screen, editor, now, last)
            }
            _ => Vec::new(),
        }
    }

    fn click(
        &mut self,
        x: u16,
        y: u16,
        screen: &ScreenMap,
        editor: &Editor,
        now: Instant,
        last: Option<(Instant, NodeId)>,
    ) -> Vec<Action> {
        if let Some(crumb) = screen.crumbs.iter().find(|c| c.y == y && c.xs.contains(&x)) {
            return vec![Action::ZoomTo(crumb.depth)];
        }
        if let (Some((text_x, text_y)), Some((query, _))) = (screen.search, editor.search_input()) {
            if y == text_y && x >= text_x {
                return vec![Action::Search(SearchOp::MoveTo(column_to_char(query, x - text_x)))];
            }
        }
        let Some(line) = screen.lines.iter().find(|l| l.y == y) else {
            return Vec::new();
        };
        let node = line.node;
        let column = x.saturating_sub(line.text_x);
        // The character under the click within this screen line's slice of
        // `text`. Past the end of a wrapped line means the last character
        // on it, not the first one of the next line.
        let char_at = |text: &str| {
            let piece: String = text.chars().skip(line.chars.start).take(line.chars.len()).collect();
            let last = if line.wrapped { line.chars.end - 1 } else { line.chars.end };
            (line.chars.start + column_to_char(&piece, column)).min(last)
        };
        match line.kind {
            // Left of the text on an item's first line: the bullet (folds,
            // if there's anything to fold) or the indentation before it.
            LineKind::Text { bullet_x: Some(bullet_x), foldable } if x < line.text_x => {
                let mut actions = vec![Action::PlaceCursor { node, cursor: 0, note: false }];
                if foldable && x >= bullet_x {
                    actions.push(Action::Fold(FoldOp::Toggle));
                }
                actions
            }
            LineKind::Text { .. } => {
                let cursor = char_at(&editor.outline.get(node).text);
                let place = Action::PlaceCursor { node, cursor, note: false };
                match last {
                    Some((at, prev)) if prev == node && now.duration_since(at) <= DOUBLE_CLICK => {
                        vec![place, Action::ZoomIn]
                    }
                    _ => {
                        self.last_text_click = Some((now, node));
                        vec![place]
                    }
                }
            }
            LineKind::Note { line: n } => {
                let mut note_lines = editor.outline.get(node).note.split('\n');
                let before: usize = note_lines.by_ref().take(n).map(|l| l.chars().count() + 1).sum();
                let cursor = before + char_at(note_lines.next().unwrap_or(""));
                vec![Action::PlaceCursor { node, cursor, note: true }]
            }
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
    fn mouse_at(editor: &mut Editor, mouse: &mut Mouse, kind: MouseEventKind, x: u16, y: u16, now: Instant) {
        let screen = draw(editor);
        for action in mouse.resolve(event(kind, x, y), &screen, editor, now) {
            editor.apply(action);
        }
    }

    fn mouse(editor: &mut Editor, kind: MouseEventKind, x: u16, y: u16) {
        mouse_at(editor, &mut Mouse::default(), kind, x, y, Instant::now());
    }

    fn click(editor: &mut Editor, x: u16, y: u16) {
        mouse(editor, MouseEventKind::Down(MouseButton::Left), x, y);
    }

    fn click_at(editor: &mut Editor, mouse: &mut Mouse, x: u16, y: u16, now: Instant) {
        mouse_at(editor, mouse, MouseEventKind::Down(MouseButton::Left), x, y, now);
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
    fn clicks_on_a_wrapped_item_map_to_the_right_characters() {
        // At this width item text wraps at 55: the first screen line holds
        // characters 0..55, the second (y=3) 55..69.
        let long = ["word"; 14].join(" ");
        let (mut ed, ids) = editor_with(&[&long, "next"], EditingStyle::Traditional);

        click(&mut ed, 3 + 5, 3);
        assert_eq!((ed.selected, ed.cursor), (ids[0], 55 + 5), "on the continuation line");
        click(&mut ed, 58, 2);
        assert_eq!(ed.cursor, 54, "past the end of a wrapped line stays on that line");
        click(&mut ed, 40, 3);
        assert_eq!(ed.cursor, 69, "past the end of the last line is the end of the text");
        click(&mut ed, 1, 3);
        assert_eq!(ed.cursor, 55, "continuation lines have no bullet; left of the text is its start");
        assert!(!ed.outline.get(ids[0]).collapsed);

        click(&mut ed, 3, 4);
        assert_eq!(ed.selected, ids[1], "the next item is pushed down a line");
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
            (MouseEventKind::Down(MouseButton::Left), 1, 0), // the current (only) breadcrumb
            (MouseEventKind::Moved, 5, 2),
            (MouseEventKind::Up(MouseButton::Left), 5, 2),
            (MouseEventKind::Down(MouseButton::Right), 5, 2),
        ] {
            let actions = Mouse::default().resolve(event(kind, x, y), &screen, &ed, Instant::now());
            assert!(actions.is_empty(), "{kind:?}");
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

    #[test]
    fn clicking_a_breadcrumb_zooms_out_to_it_and_selects_the_item_left() {
        let (mut ed, ids) = editor_with(&["alpha", "bravo"], EditingStyle::Modal);
        let kid = ed.outline.create_node("kid");
        ed.outline.append_child(ids[0], kid);
        ed.apply(Action::ZoomIn); // into alpha
        ed.apply(Action::MoveDown); // kid (alpha is the header row)
        ed.apply(Action::ZoomIn); // into kid: "Home › alpha › kid"

        click(&mut ed, 8, 0); // "alpha" is x 7..12
        assert_eq!(ed.outline.zoom_root(), ids[0]);
        assert_eq!(ed.selected, kid);
        click(&mut ed, 5, 0); // the separator
        assert_eq!(ed.outline.zoom_root(), ids[0]);
        click(&mut ed, 0, 0); // "Home"
        assert_eq!(ed.outline.zoom_root(), ed.outline.root());
        assert_eq!(ed.selected, ids[0]);
    }

    #[test]
    fn double_clicking_a_task_zooms_into_it() {
        let (mut ed, ids) = editor_with(&["alpha", "bravo"], EditingStyle::Modal);
        let (mut m, t) = (Mouse::default(), Instant::now());
        click_at(&mut ed, &mut m, 4, 3, t);
        click_at(&mut ed, &mut m, 5, 3, t + Duration::from_millis(200));
        assert_eq!(ed.outline.zoom_root(), ids[1]);
    }

    #[test]
    fn slow_clicks_different_items_and_bullet_clicks_are_not_double_clicks() {
        let (mut ed, _) = editor_with(&["alpha", "bravo"], EditingStyle::Modal);
        let (mut m, t) = (Mouse::default(), Instant::now());
        let ms = Duration::from_millis;
        click_at(&mut ed, &mut m, 4, 3, t);
        click_at(&mut ed, &mut m, 4, 3, t + ms(600)); // too slow
        click_at(&mut ed, &mut m, 4, 2, t + ms(700)); // a different item
        click_at(&mut ed, &mut m, 1, 2, t + ms(800)); // that item's bullet
        click_at(&mut ed, &mut m, 4, 2, t + ms(900)); // text again, but after a bullet click
        assert_eq!(ed.outline.zoom_root(), ed.outline.root());
    }

    #[test]
    fn clicking_in_the_search_box_moves_its_cursor() {
        let (mut ed, _) = editor_with(&["alpha"], EditingStyle::Modal);
        ed.apply(Action::OpenSearch);
        for c in "bravo".chars() {
            ed.apply(Action::Search(SearchOp::Insert(c)));
        }
        let (x, y) = draw(&mut ed).search.unwrap();
        click(&mut ed, x + 2, y);
        assert_eq!(ed.search_input(), Some(("bravo", 2)));
    }
}
