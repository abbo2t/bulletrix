use model::NodeId;

/// Everything the editor can do, independent of which key or click
/// triggered it. Keymaps and the mouse translate input into these;
/// `Editor::apply` executes them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    MoveUp,
    MoveDown,
    /// `wrap` crosses onto the adjacent row at a line boundary.
    CharLeft { wrap: bool },
    CharRight { wrap: bool },
    LineStart,
    LineEnd,
    JumpToFirst,
    JumpToLast,

    Indent,
    Outdent,
    MoveNodeUp,
    MoveNodeDown,
    ZoomIn,
    ZoomOut,
    Fold(FoldOp),
    ToggleComplete,
    ToggleNote,
    NewChild,
    OpenBelow,
    OpenAbove,
    ChangeLine,
    DeleteNode,

    EnterInsert(InsertAt),
    ExitInsert,
    InsertChar(char),
    Newline,
    Backspace,
    DeleteForward,

    Undo,
    Redo,

    StartJump,
    /// `None` cancels the jump.
    JumpInput(Option<char>),

    OpenSearch,
    Search(SearchOp),

    /// Select `node` with the cursor at `cursor` in its text (or its note,
    /// if `note`). From a mouse click, so it also cancels a jump or search.
    PlaceCursor { node: NodeId, cursor: usize, note: bool },
    /// Scroll the view by this many lines, moving the selection only if it
    /// would otherwise go off screen.
    Scroll(isize),
    /// Zoom out to breadcrumb `depth` (0 is the top). From a mouse click,
    /// so it also cancels a jump or search.
    ZoomTo(usize),

    ToggleHideCompleted,
    /// Puts the selected item's text in the paste register, and queues it
    /// in `Editor::pending_copy` for the caller to send to the clipboard.
    CopyLine,
    /// Insert a new item with the register's text next to the selected one.
    PasteBelow,
    PasteAbove,
    /// Performed by the caller (the editor does no I/O).
    Save,
    Quit,
}

/// Edits to the search box's query, plus submitting or cancelling it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchOp {
    Insert(char),
    Backspace,
    Delete,
    Left,
    Right,
    Home,
    End,
    /// Put the cursor at this character offset (from a click).
    MoveTo(usize),
    Submit,
    Cancel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FoldOp {
    Open,
    Close,
    Toggle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InsertAt {
    Cursor,
    AfterCursor,
    LineStart,
    LineEnd,
}
