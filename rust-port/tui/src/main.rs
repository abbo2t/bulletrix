//! Ratatui rendering loop sketch for the bulletrix port.
//!
//! Scope of this pass: terminal setup/teardown (incl. a panic hook so a
//! crash never leaves the user's shell in raw mode), the draw loop, and a
//! faithful port of `OutlineView._render_row` (bullets, indentation, tag
//! highlighting, completed strikethrough, selected-row highlight, cursor
//! overlay). Navigation is real (j/k, space to fold, Enter/H to zoom); full
//! modal editing (INSERT mode, character-level cursor movement, the
//! easymotion-style jump feature) is the next sketch, not this one - that's
//! why the cursor overlay below is always drawn at position 0 and always in
//! NORMAL style rather than reacting to typed text.

use crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, KeyModifiers,
};
use crossterm::execute;
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};
use model::{NodeId, Outline, Row, TAG_RE};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph};
use ratatui::Terminal;
use std::io::{self, Stdout};
use std::time::Duration;

const NORMAL_HELP: &str = "i/a/I/A:insert  o/O:open  dd:delete  cc:change  yy:copy  hjkl:move  s:jump  \
Enter/L:zoom-in  H:zoom-out  Space/za/zo/zc:fold  gg/G:top/bottom  x:del-char  >>/<<:indent  \
/:search  ^D:done  ^O:note  ^H:hide-done  u:undo  ^R:redo  ^S:save  q:quit";

fn main() -> io::Result<()> {
    install_panic_hook();
    let mut terminal = init_terminal()?;
    let mut app = App::new();
    let result = run(&mut terminal, &mut app);
    restore_terminal(&mut terminal)?;
    result
}

// -- terminal lifecycle --------------------------------------------------

fn init_terminal() -> io::Result<Terminal<CrosstermBackend<Stdout>>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    Terminal::new(CrosstermBackend::new(stdout))
}

fn restore_terminal(terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> io::Result<()> {
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen, DisableMouseCapture)?;
    terminal.show_cursor()
}

/// Without this, a panic mid-draw leaves the terminal in raw mode / the
/// alternate screen - the shell looks "frozen" until the user runs `reset`.
/// Textual's App class handles this for you; ratatui does not, so it's a
/// day-one requirement, not a nice-to-have.
fn install_panic_hook() {
    let original = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen, DisableMouseCapture);
        original(info);
    }));
}

// -- app state -------------------------------------------------------------

struct App {
    outline: Outline,
    selected: NodeId,
    scroll_offset: usize,
}

impl App {
    fn new() -> Self {
        let mut outline = Outline::new();
        seed_demo_content(&mut outline);
        let selected = outline.flatten()[0].node;
        App {
            outline,
            selected,
            scroll_offset: 0,
        }
    }

    fn move_selection(&mut self, delta: isize) {
        let rows = self.outline.flatten();
        let Some(idx) = rows.iter().position(|r| r.node == self.selected) else {
            return;
        };
        let new_idx = (idx as isize + delta).clamp(0, rows.len() as isize - 1) as usize;
        self.selected = rows[new_idx].node;
    }

    fn toggle_collapse(&mut self) {
        if !self.outline.get(self.selected).children.is_empty() {
            let collapsed = self.outline.get(self.selected).collapsed;
            self.outline.get_mut(self.selected).collapsed = !collapsed;
        }
    }

    fn zoom_in(&mut self) {
        if !self.outline.get(self.selected).children.is_empty() {
            self.outline.zoom_in(self.selected);
            if let Some(first) = self.outline.flatten().first() {
                self.selected = first.node;
            }
        }
    }

    fn zoom_out(&mut self) {
        if let Some(child) = self.outline.zoom_out() {
            self.selected = child;
        }
    }
}

fn seed_demo_content(outline: &mut Outline) {
    let root = outline.root();
    // Outline::new() (matching Python's Outline.__init__) always seeds root
    // with one empty first child - reuse it instead of leaving a stray
    // blank row in the demo tree.
    let groceries = outline.get(root).children[0];
    outline.get_mut(groceries).text = "Groceries #errand".into();

    let milk = outline.create_node("Buy milk");
    outline.get_mut(milk).completed = true;
    outline.add_first_child(groceries, milk);
    let eggs = outline.create_node("Buy eggs");
    outline.insert_sibling_after(milk, eggs);

    let work = outline.create_node("Work @acme");
    outline.insert_sibling_after(groceries, work);
    let review = outline.create_node("Review PR #4");
    outline.get_mut(review).note = "check the WSL2 focus bug writeup first".into();
    outline.add_first_child(work, review);

    let idea = outline.create_node("Port bulletrix to ratatui");
    outline.insert_sibling_after(work, idea);
}

// -- event loop --------------------------------------------------------

fn run(terminal: &mut Terminal<CrosstermBackend<Stdout>>, app: &mut App) -> io::Result<()> {
    loop {
        terminal.draw(|frame| ui(frame, app))?;

        if event::poll(Duration::from_millis(250))? {
            if let Event::Key(key) = event::read()? {
                if key.kind != KeyEventKind::Press {
                    continue;
                }
                match key.code {
                    KeyCode::Char('q') => return Ok(()),
                    KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => return Ok(()),
                    KeyCode::Char('j') | KeyCode::Down => app.move_selection(1),
                    KeyCode::Char('k') | KeyCode::Up => app.move_selection(-1),
                    KeyCode::Char(' ') => app.toggle_collapse(),
                    KeyCode::Enter => app.zoom_in(),
                    KeyCode::Char('H') => app.zoom_out(),
                    _ => {}
                }
            }
        }
    }
}

// -- drawing -------------------------------------------------------------

fn ui(frame: &mut ratatui::Frame, app: &mut App) {
    let area = frame.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(1), Constraint::Length(1)])
        .split(area);

    frame.render_widget(
        Paragraph::new(breadcrumb_text(&app.outline)).style(Style::default().fg(Color::DarkGray)),
        chunks[0],
    );

    let rows = app.outline.flatten();
    let mut lines: Vec<Line> = Vec::new();
    let mut selected_line_idx = 0;
    for row in &rows {
        let is_selected = row.node == app.selected;
        let start = lines.len();
        lines.extend(render_row(&app.outline, row, is_selected));
        if is_selected {
            selected_line_idx = start;
        }
    }

    let outline_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::Blue));
    let inner_height = chunks[1].height.saturating_sub(2) as usize;
    app.scroll_offset = clamp_scroll(app.scroll_offset, selected_line_idx, lines.len(), inner_height);

    let paragraph = Paragraph::new(lines)
        .block(outline_block)
        .scroll((app.scroll_offset as u16, 0));
    frame.render_widget(paragraph, chunks[1]);

    frame.render_widget(
        Paragraph::new(status_text(&app.outline))
            .style(Style::default().bg(Color::DarkGray).fg(Color::White)),
        chunks[2],
    );
}

/// Mirrors `OutlineView._scroll_selected_into_view`: nudge the viewport by
/// the minimum amount needed to keep the selected line on screen, rather
/// than re-centering every frame.
fn clamp_scroll(offset: usize, selected_line: usize, total_lines: usize, viewport: usize) -> usize {
    if viewport == 0 {
        return offset;
    }
    let offset = if selected_line < offset {
        selected_line
    } else if selected_line >= offset + viewport {
        selected_line + 1 - viewport
    } else {
        offset
    };
    offset.min(total_lines.saturating_sub(viewport))
}

fn breadcrumb_text(outline: &Outline) -> String {
    outline
        .breadcrumb()
        .iter()
        .map(|&id| {
            let n = outline.get(id);
            if !n.text.is_empty() {
                n.text.clone()
            } else if id == outline.root() {
                "Home".to_string()
            } else {
                "(untitled)".to_string()
            }
        })
        .collect::<Vec<_>>()
        .join(" › ")
}

fn status_text(outline: &Outline) -> String {
    let hide = if outline.hide_completed { "on" } else { "off" };
    format!("-- NORMAL --  {NORMAL_HELP}  |  hide-done:{hide}")
}

/// Port of `_render_row`. Rich's `Text.stylize(start, end)` mutates spans
/// in place over a plain string; ratatui has no such object, so this
/// builds one `(char, Style)` per character, layers tag-highlighting and
/// the cursor overlay on top exactly like the Python version's ordering
/// (base style -> tags -> cursor -> labels), then coalesces runs of equal
/// style into `Span`s at the end.
fn render_row(outline: &Outline, row: &Row, is_selected: bool) -> Vec<Line<'static>> {
    let node = outline.get(row.node);

    let prefix = if row.is_header {
        String::new()
    } else {
        "  ".repeat(row.depth)
    };
    let bullet = if row.is_header {
        "» "
    } else if row.has_children {
        if row.visible_children {
            "▾ "
        } else {
            "▸ "
        }
    } else {
        "• "
    };

    let mut first_line_spans = Vec::new();
    if !prefix.is_empty() {
        first_line_spans.push(Span::raw(prefix));
    }
    first_line_spans.push(Span::styled(bullet, bullet_style(row.has_children, is_selected)));

    let mut text_style = Style::default();
    if row.is_header {
        text_style = text_style.add_modifier(Modifier::BOLD | Modifier::UNDERLINED);
    }
    if node.completed {
        text_style = text_style.add_modifier(Modifier::CROSSED_OUT | Modifier::DIM);
    }

    let mut cells = char_style_vec(&node.text, text_style);
    apply_tag_highlight(&mut cells, &node.text);
    if is_selected {
        // Cursor always at 0 / NORMAL style until the key-handling sketch
        // wires this to real INSERT-mode state.
        apply_cursor_cell(&mut cells, 0, false);
    }
    first_line_spans.extend(cells_to_spans(cells));

    let mut lines = vec![Line::from(first_line_spans)];

    if !node.note.is_empty() {
        let note_prefix = "  ".repeat(row.depth + 1) + "  ";
        let note_style = Style::default().fg(Color::Gray).add_modifier(Modifier::ITALIC);
        lines.push(Line::from(vec![
            Span::raw(note_prefix),
            Span::styled(node.note.clone(), note_style),
        ]));
    }

    lines
}

fn bullet_style(has_children: bool, selected: bool) -> Style {
    let mut style = if has_children {
        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Gray)
    };
    if selected {
        // Python: bullet_style += " bold yellow" - later color wins, bold
        // is idempotent. Same effect here: selected always reads as bold
        // yellow regardless of has_children.
        style = style.fg(Color::Yellow).add_modifier(Modifier::BOLD);
    }
    style
}

fn char_style_vec(text: &str, base: Style) -> Vec<(char, Style)> {
    text.chars().map(|c| (c, base)).collect()
}

fn apply_tag_highlight(cells: &mut [(char, Style)], text: &str) {
    let tag_style = Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD);
    for m in TAG_RE.find_iter(text).flatten() {
        let start = text[..m.start()].chars().count();
        let end = (start + text[m.start()..m.end()].chars().count()).min(cells.len());
        for cell in &mut cells[start..end] {
            cell.1 = tag_style;
        }
    }
}

fn apply_cursor_cell(cells: &mut Vec<(char, Style)>, cursor: usize, insert_mode: bool) {
    let style = if insert_mode {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::UNDERLINED | Modifier::BOLD)
    } else {
        Style::default().add_modifier(Modifier::REVERSED)
    };
    let cursor = cursor.min(cells.len());
    if cursor >= cells.len() {
        cells.push((' ', style));
    } else {
        cells[cursor].1 = style;
    }
}

fn cells_to_spans(cells: Vec<(char, Style)>) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let mut current = String::new();
    let mut current_style: Option<Style> = None;
    for (ch, style) in cells {
        if current_style == Some(style) {
            current.push(ch);
        } else {
            if let Some(s) = current_style {
                spans.push(Span::styled(std::mem::take(&mut current), s));
            }
            current.push(ch);
            current_style = Some(style);
        }
    }
    if let Some(s) = current_style {
        spans.push(Span::styled(current, s));
    }
    spans
}
