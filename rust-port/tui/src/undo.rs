use std::collections::VecDeque;

/// Bounded undo/redo stack of opaque state snapshots (port of undo.py).
pub struct UndoStack<T> {
    undo: VecDeque<T>,
    redo: Vec<T>,
    max_history: usize,
}

impl<T> UndoStack<T> {
    pub fn new(max_history: usize) -> Self {
        UndoStack {
            undo: VecDeque::new(),
            redo: Vec::new(),
            max_history,
        }
    }

    /// Records `snapshot` as the state *before* an edit and drops the redo branch.
    pub fn checkpoint(&mut self, snapshot: T) {
        self.undo.push_back(snapshot);
        if self.undo.len() > self.max_history {
            self.undo.pop_front();
        }
        self.redo.clear();
    }

    pub fn undo(&mut self, current: T) -> Option<T> {
        let previous = self.undo.pop_back()?;
        self.redo.push(current);
        Some(previous)
    }

    pub fn redo(&mut self, current: T) -> Option<T> {
        let next = self.redo.pop()?;
        self.undo.push_back(current);
        Some(next)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn undo_then_redo_round_trips() {
        let mut stack = UndoStack::new(10);
        stack.checkpoint("a");
        assert_eq!(stack.undo("b"), Some("a"));
        assert_eq!(stack.redo("a"), Some("b"));
    }

    #[test]
    fn empty_stack_returns_none_without_touching_the_other_side() {
        let mut stack: UndoStack<&str> = UndoStack::new(10);
        assert_eq!(stack.undo("x"), None);
        assert_eq!(stack.redo("x"), None);
    }

    #[test]
    fn checkpoint_clears_redo_branch() {
        let mut stack = UndoStack::new(10);
        stack.checkpoint("a");
        stack.undo("b");
        stack.checkpoint("c");
        assert_eq!(stack.redo("d"), None);
    }

    #[test]
    fn history_is_bounded_dropping_oldest() {
        let mut stack = UndoStack::new(2);
        stack.checkpoint(1);
        stack.checkpoint(2);
        stack.checkpoint(3);
        assert_eq!(stack.undo(0), Some(3));
        assert_eq!(stack.undo(0), Some(2));
        assert_eq!(stack.undo(0), None);
    }
}
