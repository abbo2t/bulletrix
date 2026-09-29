/// Everything the editor can do, independent of which key triggered it.
/// Keymaps translate keys into these; `Editor::apply` executes them.
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

    ToggleHideCompleted,
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
