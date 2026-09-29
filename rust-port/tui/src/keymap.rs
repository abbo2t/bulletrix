//! Key -> `Action` tables. Both styles are built from the same layers:
//! traditional = shared + insert layer; modal = shared + (insert layer or
//! normal layer, depending on mode).

use crate::action::{Action, FoldOp, InsertAt, SearchOp};
use crate::config::EditingStyle;
use crate::editor::{Editor, Mode};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

pub trait Keymap {
    fn resolve(&mut self, key: KeyEvent, mode: Mode) -> Option<Action>;
    fn help(&self, mode: Mode) -> &'static str;
}

pub fn for_style(style: EditingStyle) -> Box<dyn Keymap> {
    match style {
        EditingStyle::Modal => Box::new(ModalKeymap::default()),
        EditingStyle::Traditional => Box::new(TraditionalKeymap),
    }
}

/// Applies the action `key` maps to, and returns it so the caller can act
/// on the ones that need I/O (`Save`).
pub fn dispatch(editor: &mut Editor, keymap: &mut dyn Keymap, key: KeyEvent) -> Option<Action> {
    // A jump in progress consumes every key; an open search box consumes
    // everything but the global keys. Both work the same in every style.
    let action = if editor.jump_active() {
        Some(Action::JumpInput(printable(key)))
    } else if editor.search_input().is_some() {
        global(key).or_else(|| search_box(key))
    } else {
        global(key).or_else(|| keymap.resolve(key, editor.mode()))
    }?;
    editor.apply(action);
    Some(action)
}

fn printable(key: KeyEvent) -> Option<char> {
    match key.code {
        KeyCode::Char(c) if !key.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) => Some(c),
        _ => None,
    }
}

fn is_ctrl(key: KeyEvent, c: char) -> bool {
    key.code == KeyCode::Char(c) && key.modifiers.contains(KeyModifiers::CONTROL)
}

fn global(key: KeyEvent) -> Option<Action> {
    if is_ctrl(key, 'q') {
        Some(Action::Quit)
    } else if is_ctrl(key, 'h') {
        Some(Action::ToggleHideCompleted)
    } else if is_ctrl(key, 's') {
        Some(Action::Save)
    } else {
        None
    }
}

fn search_box(key: KeyEvent) -> Option<Action> {
    let op = match key.code {
        KeyCode::Enter => SearchOp::Submit,
        KeyCode::Esc => SearchOp::Cancel,
        KeyCode::Backspace => SearchOp::Backspace,
        KeyCode::Delete => SearchOp::Delete,
        KeyCode::Left => SearchOp::Left,
        KeyCode::Right => SearchOp::Right,
        KeyCode::Home => SearchOp::Home,
        KeyCode::End => SearchOp::End,
        _ => SearchOp::Insert(printable(key)?),
    };
    Some(Action::Search(op))
}

/// Keys that mean the same thing in every style and mode.
fn shared(key: KeyEvent) -> Option<Action> {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let action = match key.code {
        KeyCode::Tab => Action::Indent,
        KeyCode::BackTab => Action::Outdent,
        KeyCode::Up if ctrl => Action::MoveNodeUp,
        KeyCode::Down if ctrl => Action::MoveNodeDown,
        KeyCode::Right if ctrl => Action::ZoomIn,
        KeyCode::Left if ctrl => Action::ZoomOut,
        KeyCode::Up => Action::MoveUp,
        KeyCode::Down => Action::MoveDown,
        KeyCode::Left => Action::CharLeft { wrap: true },
        KeyCode::Right => Action::CharRight { wrap: true },
        KeyCode::Home => Action::LineStart,
        KeyCode::End => Action::LineEnd,
        KeyCode::Char('d') if ctrl => Action::ToggleComplete,
        KeyCode::Char('k') if ctrl => Action::Fold(FoldOp::Toggle),
        KeyCode::Char('o') if ctrl => Action::ToggleNote,
        KeyCode::Char('n') if ctrl => Action::NewChild,
        KeyCode::Char('g') if ctrl => Action::StartJump,
        KeyCode::Char('f') if ctrl => Action::OpenSearch,
        _ => return None,
    };
    Some(action)
}

/// Keys that edit text directly.
fn insert_layer(key: KeyEvent) -> Option<Action> {
    match key.code {
        KeyCode::Enter => Some(Action::Newline),
        KeyCode::Backspace => Some(Action::Backspace),
        KeyCode::Delete => Some(Action::DeleteForward),
        _ => printable(key).map(Action::InsertChar),
    }
}

pub struct TraditionalKeymap;

impl Keymap for TraditionalKeymap {
    fn resolve(&mut self, key: KeyEvent, _mode: Mode) -> Option<Action> {
        if is_ctrl(key, 'z') {
            return Some(Action::Undo);
        }
        if is_ctrl(key, 'y') {
            return Some(Action::Redo);
        }
        if is_ctrl(key, 'c') {
            return Some(Action::CopyLine);
        }
        // Not Ctrl+V: terminals keep that for their own paste, which types
        // the clipboard into the current line - the right thing here anyway.
        if is_ctrl(key, 'p') {
            return Some(Action::PasteBelow);
        }
        shared(key).or_else(|| insert_layer(key))
    }

    fn help(&self, _mode: Mode) -> &'static str {
        "Enter:new line  Tab/⇧Tab:indent  ^←/^→:zoom  ^↑/^↓:move  ^G:jump  ^F:search  ^D:done  ^K:fold  \
         ^O:note  ^N:child  ^C/^P:copy/paste  ^Z/^Y:undo/redo  ^H:hide-done  ^S:save  ^Q:quit"
    }
}

#[derive(Default)]
pub struct ModalKeymap {
    /// First key of a two-key command (`dd`, `cc`, `gg`, `za`, ...).
    pending: Option<char>,
}

impl Keymap for ModalKeymap {
    fn resolve(&mut self, key: KeyEvent, mode: Mode) -> Option<Action> {
        if key.code == KeyCode::Esc {
            self.pending = None;
            return (mode == Mode::Insert).then_some(Action::ExitInsert);
        }
        if let Some(action) = shared(key).or_else(|| is_ctrl(key, 'r').then_some(Action::Redo)) {
            self.pending = None;
            return Some(action);
        }
        match mode {
            Mode::Insert => insert_layer(key),
            Mode::Normal => self.normal_layer(key),
        }
    }

    fn help(&self, mode: Mode) -> &'static str {
        match mode {
            Mode::Normal => {
                "i/a/I/A:insert  o/O:open  dd:delete  cc:change  yy:copy  p/P:paste  hjkl:move  s:jump  /:search  \
                 Enter/L:zoom-in  H:zoom-out  Space/za/zo/zc:fold  gg/G:top/bottom  x:del-char  \
                 Tab/⇧Tab:indent  ^D:done  ^O:note  ^H:hide-done  u:undo  ^R:redo  ^S:save  q:quit"
            }
            Mode::Insert => "Esc:normal  Enter:new line  Tab/⇧Tab:indent  Backspace/Delete:edit  ^D:done  ^O:note",
        }
    }
}

impl ModalKeymap {
    fn normal_layer(&mut self, key: KeyEvent) -> Option<Action> {
        let pending = self.pending.take();
        let ch = printable(key);

        // Any key completes (or abandons) a pending command; it's never
        // reinterpreted on its own, so `dj` does nothing rather than `j`.
        if let Some(first) = pending {
            return match (first, ch?) {
                ('d', 'd') => Some(Action::DeleteNode),
                ('c', 'c') => Some(Action::ChangeLine),
                ('g', 'g') => Some(Action::JumpToFirst),
                ('y', 'y') => Some(Action::CopyLine),
                ('z', 'o') => Some(Action::Fold(FoldOp::Open)),
                ('z', 'c') => Some(Action::Fold(FoldOp::Close)),
                ('z', 'a') => Some(Action::Fold(FoldOp::Toggle)),
                _ => None,
            };
        }

        if key.code == KeyCode::Enter {
            return Some(Action::ZoomIn);
        }
        let action = match ch? {
            c @ ('d' | 'c' | 'g' | 'y' | 'z') => {
                self.pending = Some(c);
                return None;
            }
            ' ' => Action::Fold(FoldOp::Toggle),
            'q' => Action::Quit,
            'i' => Action::EnterInsert(InsertAt::Cursor),
            'a' => Action::EnterInsert(InsertAt::AfterCursor),
            'I' => Action::EnterInsert(InsertAt::LineStart),
            'A' => Action::EnterInsert(InsertAt::LineEnd),
            'o' => Action::OpenBelow,
            'p' => Action::PasteBelow,
            'P' => Action::PasteAbove,
            'O' => Action::OpenAbove,
            'x' => Action::DeleteForward,
            'h' => Action::CharLeft { wrap: false },
            'l' => Action::CharRight { wrap: false },
            'j' => Action::MoveDown,
            'k' => Action::MoveUp,
            'H' => Action::ZoomOut,
            'L' => Action::ZoomIn,
            '0' => Action::LineStart,
            '$' => Action::LineEnd,
            'G' => Action::JumpToLast,
            's' => Action::StartJump,
            '/' => Action::OpenSearch,
            'u' => Action::Undo,
            // other printable keys are swallowed in NORMAL mode rather than typed
            _ => return None,
        };
        Some(action)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use model::{NodeId, Outline};

    struct Harness {
        ed: Editor,
        km: Box<dyn Keymap>,
        a: NodeId,
        b: NodeId,
        c: NodeId,
    }

    /// Top-level rows "alpha", "bravo", "charlie"; "alpha" selected with the
    /// cursor at the end of its text (as on a fresh start).
    fn harness(style: EditingStyle) -> Harness {
        let mut outline = Outline::new();
        let root = outline.root();
        let a = outline.get(root).children[0];
        outline.get_mut(a).text = "alpha".into();
        let b = outline.create_node("bravo");
        outline.insert_sibling_after(a, b);
        let c = outline.create_node("charlie");
        outline.insert_sibling_after(b, c);
        Harness {
            ed: Editor::new(outline, style),
            km: for_style(style),
            a,
            b,
            c,
        }
    }

    impl Harness {
        fn key(&mut self, code: KeyCode, mods: KeyModifiers) {
            dispatch(&mut self.ed, self.km.as_mut(), KeyEvent::new(code, mods));
        }
        fn press(&mut self, code: KeyCode) {
            self.key(code, KeyModifiers::NONE);
        }
        fn ctrl(&mut self, c: char) {
            self.key(KeyCode::Char(c), KeyModifiers::CONTROL);
        }
        fn typ(&mut self, s: &str) {
            for c in s.chars() {
                let mods = if c.is_uppercase() { KeyModifiers::SHIFT } else { KeyModifiers::NONE };
                self.key(KeyCode::Char(c), mods);
            }
        }
        fn text(&self, id: NodeId) -> &str {
            &self.ed.outline.get(id).text
        }
        fn top_level(&self) -> Vec<String> {
            let root = self.ed.outline.root();
            self.ed.outline.get(root).children.iter().map(|&id| self.text(id).to_string()).collect()
        }
    }

    // -- modal --------------------------------------------------------------

    #[test]
    fn modal_starts_in_normal_mode_and_swallows_typing() {
        let mut h = harness(EditingStyle::Modal);
        h.typ("wrt");
        assert_eq!(h.ed.mode(), Mode::Normal);
        assert_eq!(h.text(h.a), "alpha");
    }

    #[test]
    fn i_enters_insert_mode_and_types() {
        let mut h = harness(EditingStyle::Modal);
        h.typ("0ix");
        assert_eq!(h.ed.mode(), Mode::Insert);
        assert_eq!(h.text(h.a), "xalpha");
    }

    #[test]
    fn escape_returns_to_normal_mode_with_cursor_on_last_char() {
        let mut h = harness(EditingStyle::Modal);
        h.typ("A");
        h.press(KeyCode::Esc);
        assert_eq!(h.ed.mode(), Mode::Normal);
        assert_eq!(h.ed.cursor, 4);
    }

    #[test]
    fn a_appends_after_cursor() {
        let mut h = harness(EditingStyle::Modal);
        h.typ("0aZ");
        assert_eq!(h.text(h.a), "aZlpha");
    }

    #[test]
    fn capital_i_and_a_go_to_line_start_and_end() {
        let mut h = harness(EditingStyle::Modal);
        h.typ("I<");
        h.press(KeyCode::Esc);
        h.typ("A>");
        assert_eq!(h.text(h.a), "<alpha>");
    }

    #[test]
    fn enter_in_insert_splits_into_sibling_nodes() {
        let mut h = harness(EditingStyle::Modal);
        h.typ("0lli");
        h.press(KeyCode::Enter);
        assert_eq!(h.top_level(), ["al", "pha", "bravo", "charlie"]);
        assert_eq!(h.ed.cursor, 0);
        assert_eq!(h.text(h.ed.selected), "pha");
    }

    #[test]
    fn o_and_capital_o_open_siblings_in_insert_mode() {
        let mut h = harness(EditingStyle::Modal);
        h.typ("onew");
        h.press(KeyCode::Esc);
        h.typ("jOup");
        assert_eq!(h.top_level(), ["alpha", "new", "up", "bravo", "charlie"]);
    }

    #[test]
    fn dd_deletes_current_node_and_selects_the_next() {
        let mut h = harness(EditingStyle::Modal);
        h.typ("jdd");
        assert_eq!(h.top_level(), ["alpha", "charlie"]);
        assert_eq!(h.ed.selected, h.c);
    }

    #[test]
    fn dd_refuses_to_delete_the_only_top_level_item() {
        let mut ed = Editor::new(Outline::new(), EditingStyle::Modal);
        let mut km = for_style(EditingStyle::Modal);
        for _ in 0..2 {
            dispatch(&mut ed, km.as_mut(), KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE));
        }
        assert_eq!(ed.outline.get(ed.outline.root()).children.len(), 1);
    }

    #[test]
    fn escape_cancels_pending_command() {
        let mut h = harness(EditingStyle::Modal);
        h.typ("d");
        h.press(KeyCode::Esc);
        h.typ("d");
        assert_eq!(h.top_level().len(), 3);
    }

    #[test]
    fn q_quits_in_normal_mode_but_types_in_insert_mode() {
        let mut h = harness(EditingStyle::Modal);
        h.typ("iq");
        assert!(!h.ed.should_quit);
        assert_eq!(h.text(h.a), "alphaq");
        h.press(KeyCode::Esc);
        h.typ("q");
        assert!(h.ed.should_quit);
    }

    #[test]
    fn cc_clears_line_and_enters_insert() {
        let mut h = harness(EditingStyle::Modal);
        h.typ("cc");
        assert_eq!(h.text(h.a), "");
        assert_eq!(h.ed.mode(), Mode::Insert);
    }

    #[test]
    fn x_deletes_character_under_cursor() {
        let mut h = harness(EditingStyle::Modal);
        h.typ("0x");
        assert_eq!(h.text(h.a), "lpha");
    }

    #[test]
    fn gg_and_capital_g_jump_to_top_and_bottom() {
        let mut h = harness(EditingStyle::Modal);
        h.typ("G");
        assert_eq!(h.ed.selected, h.c);
        h.typ("gg");
        assert_eq!(h.ed.selected, h.a);
    }

    #[test]
    fn za_and_space_toggle_fold() {
        let mut h = harness(EditingStyle::Modal);
        let child = h.ed.outline.create_node("kid");
        h.ed.outline.append_child(h.b, child);
        h.typ("jza");
        assert!(h.ed.outline.get(h.b).collapsed);
        h.typ(" ");
        assert!(!h.ed.outline.get(h.b).collapsed);
    }

    #[test]
    fn enter_in_normal_mode_zooms_in_and_capital_h_zooms_out() {
        let mut h = harness(EditingStyle::Modal);
        h.press(KeyCode::Enter);
        assert_eq!(h.ed.outline.zoom_root(), h.a);
        h.typ("H");
        assert_eq!(h.ed.outline.zoom_root(), h.ed.outline.root());
    }

    #[test]
    fn backspace_at_line_start_merges_into_previous_sibling() {
        let mut h = harness(EditingStyle::Modal);
        h.typ("jI");
        h.press(KeyCode::Backspace);
        assert_eq!(h.top_level(), ["alphabravo", "charlie"]);
        assert_eq!(h.ed.selected, h.a);
        assert_eq!(h.ed.cursor, 5);
    }

    #[test]
    fn one_insert_session_is_one_undo_step() {
        let mut h = harness(EditingStyle::Modal);
        h.typ("A123");
        h.press(KeyCode::Esc);
        h.typ("u");
        assert_eq!(h.text(h.a), "alpha");
        h.ctrl('r');
        assert_eq!(h.text(h.a), "alpha123");
    }

    #[test]
    fn ctrl_o_edits_the_note_in_insert_mode_and_returns_to_normal() {
        let mut h = harness(EditingStyle::Modal);
        h.ctrl('o');
        assert!(h.ed.editing_note);
        assert_eq!(h.ed.mode(), Mode::Insert);
        h.typ("note");
        h.ctrl('o');
        assert!(!h.ed.editing_note);
        assert_eq!(h.ed.mode(), Mode::Normal);
        assert_eq!(h.ed.outline.get(h.a).note, "note");
    }

    // -- copy -----------------------------------------------------------------

    #[test]
    fn yy_copies_current_task_text_without_changing_it() {
        let mut h = harness(EditingStyle::Modal);
        h.typ("jyy");
        assert_eq!(h.ed.pending_copy.as_deref(), Some("bravo"));
        assert_eq!(h.top_level(), ["alpha", "bravo", "charlie"]);
    }

    #[test]
    fn y_then_another_key_copies_nothing() {
        let mut h = harness(EditingStyle::Modal);
        h.typ("yj");
        assert_eq!(h.ed.pending_copy, None);
        assert_eq!(h.ed.selected, h.a, "the j is swallowed, like dj");
    }

    #[test]
    fn p_pastes_a_copy_below_at_the_same_level_without_children() {
        let mut h = harness(EditingStyle::Modal);
        let kid = h.ed.outline.create_node("kid");
        h.ed.outline.append_child(h.b, kid);
        h.typ("jyyp");

        assert_eq!(h.top_level(), ["alpha", "bravo", "bravo", "charlie"]);
        let pasted = h.ed.selected;
        assert_ne!(pasted, h.b);
        assert!(h.ed.outline.get(pasted).children.is_empty());
        assert_eq!(h.ed.outline.get(h.b).children, [kid]);
        assert_eq!(h.ed.cursor, 0);
        assert_eq!(h.ed.mode(), Mode::Normal);
    }

    #[test]
    fn p_pastes_at_the_current_position_and_level_not_where_copied() {
        let mut h = harness(EditingStyle::Modal);
        let kid = h.ed.outline.create_node("kid");
        h.ed.outline.append_child(h.b, kid);
        h.typ("yyjjp"); // copy alpha, then paste below kid (nested under bravo)
        let kids: Vec<&str> = h.ed.outline.get(h.b).children.iter().map(|&id| h.text(id)).collect();
        assert_eq!(kids, ["kid", "alpha"]);
        assert_eq!(h.top_level(), ["alpha", "bravo", "charlie"]);
    }

    #[test]
    fn p_can_repeat_and_is_one_undo_step_each() {
        let mut h = harness(EditingStyle::Modal);
        h.typ("yypp");
        assert_eq!(h.top_level(), ["alpha", "alpha", "alpha", "bravo", "charlie"]);
        h.typ("u");
        assert_eq!(h.top_level(), ["alpha", "alpha", "bravo", "charlie"]);
    }

    #[test]
    fn p_on_a_zoomed_header_pastes_as_its_first_child() {
        let mut h = harness(EditingStyle::Modal);
        let kid = h.ed.outline.create_node("kid");
        h.ed.outline.append_child(h.b, kid);
        h.typ("j");
        h.press(KeyCode::Enter); // zoom into bravo; its header row is selected
        h.typ("yyp");
        let kids: Vec<&str> = h.ed.outline.get(h.b).children.iter().map(|&id| h.text(id)).collect();
        assert_eq!(kids, ["bravo", "kid"]);
    }

    #[test]
    fn p_with_nothing_copied_changes_nothing() {
        let mut h = harness(EditingStyle::Modal);
        h.typ("p");
        assert_eq!(h.ed.notice, Some("nothing to paste"));
        assert_eq!(h.top_level(), ["alpha", "bravo", "charlie"]);
    }

    #[test]
    fn capital_p_pastes_above() {
        let mut h = harness(EditingStyle::Modal);
        h.typ("GyyggP");
        assert_eq!(h.top_level(), ["charlie", "alpha", "bravo", "charlie"]);
        assert_eq!(h.ed.selected, h.ed.outline.get(h.ed.outline.root()).children[0]);
    }

    #[test]
    fn capital_p_on_a_zoomed_header_pastes_as_its_first_child() {
        let mut h = harness(EditingStyle::Modal);
        h.typ("j");
        h.press(KeyCode::Enter); // zoom into bravo; its header row is selected
        h.typ("yyP");
        let kids: Vec<&str> = h.ed.outline.get(h.b).children.iter().map(|&id| h.text(id)).collect();
        assert_eq!(kids, ["bravo"]);
    }

    #[test]
    fn dd_then_p_moves_an_item() {
        let mut h = harness(EditingStyle::Modal);
        h.typ("jddp");
        assert_eq!(h.top_level(), ["alpha", "charlie", "bravo"]);
        assert_eq!(h.ed.pending_copy, None, "dd leaves the system clipboard alone");
    }

    #[test]
    fn a_refused_dd_copies_nothing() {
        let mut ed = Editor::new(Outline::new(), EditingStyle::Modal);
        let mut km = for_style(EditingStyle::Modal);
        for c in "ddp".chars() {
            dispatch(&mut ed, km.as_mut(), KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
        }
        assert_eq!(ed.notice, Some("nothing to paste"));
        assert_eq!(ed.outline.get(ed.outline.root()).children.len(), 1);
    }

    #[test]
    fn traditional_ctrl_p_pastes_below_and_plain_p_types() {
        let mut h = harness(EditingStyle::Traditional);
        h.ctrl('c');
        h.press(KeyCode::Down);
        h.ctrl('p');
        assert_eq!(h.top_level(), ["alpha", "bravo", "alpha", "charlie"]);
        assert_eq!(h.ed.mode(), Mode::Insert);
        h.typ("p");
        assert_eq!(h.top_level(), ["alpha", "bravo", "palpha", "charlie"]);
    }

    #[test]
    fn traditional_ctrl_c_copies_and_modal_ctrl_c_does_not() {
        let mut h = harness(EditingStyle::Traditional);
        h.ctrl('c');
        assert_eq!(h.ed.pending_copy.as_deref(), Some("alpha"));

        let mut h = harness(EditingStyle::Modal);
        h.ctrl('c');
        assert_eq!(h.ed.pending_copy, None);
    }

    // -- jump -----------------------------------------------------------------

    #[test]
    fn jump_labels_nearest_matches_first_and_moves_the_cursor() {
        let mut h = harness(EditingStyle::Modal);
        h.typ("sr");
        // bravo is one row away, charlie two: bravo gets the first label.
        assert_eq!(h.ed.jump_labels_for(h.b), [(1, 'f')]);
        assert_eq!(h.ed.jump_labels_for(h.c), [(3, 'j')]);
        assert_eq!(h.text(h.b), "bravo");
        h.typ("j");
        assert!(!h.ed.jump_active());
        assert_eq!(h.ed.selected, h.c);
        assert_eq!(h.ed.cursor, 3);
    }

    #[test]
    fn jump_with_no_matches_or_an_invalid_label_cancels_without_moving() {
        let mut h = harness(EditingStyle::Modal);
        h.typ("sQ");
        assert!(!h.ed.jump_active());
        h.typ("srz");
        assert!(!h.ed.jump_active());
        assert_eq!(h.ed.selected, h.a);
    }

    #[test]
    fn escape_cancels_jump() {
        let mut h = harness(EditingStyle::Modal);
        h.typ("s");
        h.press(KeyCode::Esc);
        assert!(!h.ed.jump_active());
        assert_eq!(h.ed.mode(), Mode::Normal);
    }

    // -- search ---------------------------------------------------------------

    #[test]
    fn search_reveals_and_selects_match() {
        let mut h = harness(EditingStyle::Modal);
        h.typ("/bravo");
        h.press(KeyCode::Enter);
        assert!(h.ed.search_input().is_none());
        assert_eq!(h.ed.selected, h.b);
        assert_eq!(h.ed.cursor, 0);
        assert_eq!(h.ed.mode(), Mode::Normal);
    }

    #[test]
    fn search_expands_collapsed_parents_and_zooms_out() {
        let mut h = harness(EditingStyle::Modal);
        let needle = h.ed.outline.create_node("needle");
        h.ed.outline.append_child(h.c, needle);
        h.ed.outline.get_mut(h.c).collapsed = true;
        h.press(KeyCode::Enter); // zoom into alpha
        h.typ("/NEEDLE");
        h.press(KeyCode::Enter);
        assert_eq!(h.ed.outline.zoom_root(), h.ed.outline.root());
        assert!(!h.ed.outline.get(h.c).collapsed);
        assert_eq!(h.ed.selected, needle);
    }

    #[test]
    fn typing_in_the_search_box_never_edits_the_outline_and_escape_cancels() {
        let mut h = harness(EditingStyle::Modal);
        h.typ("/ddcc");
        h.press(KeyCode::Esc);
        assert!(h.ed.search_input().is_none());
        assert_eq!(h.top_level(), ["alpha", "bravo", "charlie"]);
        assert_eq!(h.ed.selected, h.a);
    }

    #[test]
    fn search_box_supports_cursor_editing() {
        let mut h = harness(EditingStyle::Traditional);
        h.ctrl('f');
        h.typ("ravx");
        h.press(KeyCode::Backspace);
        h.typ("o");
        h.press(KeyCode::Home);
        h.typ("b");
        assert_eq!(h.ed.search_input(), Some(("bravo", 1)));
        h.press(KeyCode::Enter);
        assert_eq!(h.ed.selected, h.b);
        assert_eq!(h.ed.mode(), Mode::Insert, "traditional never leaves insert");
    }

    #[test]
    fn search_with_no_matches_leaves_a_notice_and_keeps_the_selection() {
        let mut h = harness(EditingStyle::Modal);
        h.typ("/zzz");
        h.press(KeyCode::Enter);
        assert_eq!(h.ed.notice, Some("no matches"));
        assert_eq!(h.ed.selected, h.a);
    }

    #[test]
    fn search_skips_matches_hidden_by_hide_completed() {
        let mut h = harness(EditingStyle::Modal);
        h.ed.outline.get_mut(h.c).completed = true;
        h.ctrl('h');
        h.typ("/charlie");
        h.press(KeyCode::Enter);
        assert_eq!(h.ed.notice, Some("no matches"));
        h.ctrl('h');
        h.typ("/charlie");
        h.press(KeyCode::Enter);
        assert_eq!(h.ed.selected, h.c);
    }

    #[test]
    fn global_keys_still_work_while_searching() {
        let mut h = harness(EditingStyle::Modal);
        h.typ("/");
        h.ctrl('q');
        assert!(h.ed.should_quit);
    }

    // -- traditional ------------------------------------------------------------

    #[test]
    fn traditional_types_immediately_including_modal_command_letters() {
        let mut h = harness(EditingStyle::Traditional);
        h.typ("ddu");
        assert_eq!(h.ed.mode(), Mode::Insert);
        assert_eq!(h.top_level(), ["alphaddu", "bravo", "charlie"]);
    }

    #[test]
    fn traditional_escape_does_not_leave_insert() {
        let mut h = harness(EditingStyle::Traditional);
        h.press(KeyCode::Esc);
        h.typ("x");
        assert_eq!(h.ed.mode(), Mode::Insert);
        assert_eq!(h.text(h.a), "alphax");
    }

    #[test]
    fn traditional_ctrl_z_undoes_and_ctrl_y_redoes() {
        let mut h = harness(EditingStyle::Traditional);
        h.typ("xyz");
        h.ctrl('z');
        assert_eq!(h.text(h.a), "alpha");
        h.ctrl('y');
        assert_eq!(h.text(h.a), "alphaxyz");
    }

    #[test]
    fn traditional_ctrl_g_jumps_without_typing_the_target_or_label() {
        let mut h = harness(EditingStyle::Traditional);
        h.ctrl('g');
        h.typ("rj");
        assert_eq!(h.ed.selected, h.c);
        assert_eq!(h.ed.cursor, 3);
        assert_eq!(h.top_level(), ["alpha", "bravo", "charlie"]);
    }

    #[test]
    fn traditional_zoom_and_note_toggle_stay_in_insert() {
        let mut h = harness(EditingStyle::Traditional);
        h.key(KeyCode::Right, KeyModifiers::CONTROL);
        assert_eq!(h.ed.outline.zoom_root(), h.a);
        h.ctrl('o');
        h.ctrl('o');
        assert_eq!(h.ed.mode(), Mode::Insert);
    }

    // -- shared -------------------------------------------------------------

    #[test]
    fn tab_indents_and_shift_tab_outdents_in_both_styles() {
        for style in [EditingStyle::Modal, EditingStyle::Traditional] {
            let mut h = harness(style);
            h.press(KeyCode::Down);
            h.press(KeyCode::Tab);
            assert_eq!(h.ed.outline.get(h.b).parent, Some(h.a), "{style:?}");
            h.key(KeyCode::BackTab, KeyModifiers::SHIFT);
            assert_eq!(h.top_level(), ["alpha", "bravo", "charlie"], "{style:?}");
        }
    }

    #[test]
    fn ctrl_q_quits_and_ctrl_h_hides_completed_in_both_styles() {
        for style in [EditingStyle::Modal, EditingStyle::Traditional] {
            let mut h = harness(style);
            h.ctrl('h');
            assert!(h.ed.outline.hide_completed, "{style:?}");
            h.ctrl('q');
            assert!(h.ed.should_quit, "{style:?}");
        }
    }
}
