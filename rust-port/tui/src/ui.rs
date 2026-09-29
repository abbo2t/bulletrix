use crate::config::EditingStyle;
use crate::editor::{Editor, Mode};
use crate::keymap::Keymap;
use model::{Row, TAG_RE};
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph};
use ratatui::Frame;

type Cells = Vec<(char, Style)>;

pub fn draw(frame: &mut Frame, editor: &mut Editor, keymap: &dyn Keymap) {
    editor.current_row();

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(1), Constraint::Length(1)])
        .split(frame.area());

    frame.render_widget(
        Paragraph::new(breadcrumb_text(editor)).style(Style::default().fg(Color::DarkGray)),
        chunks[0],
    );

    let mut lines: Vec<Line> = Vec::new();
    let mut selected_line = 0;
    for row in &editor.outline.flatten() {
        let is_selected = row.node == editor.selected;
        if is_selected {
            selected_line = lines.len();
        }
        lines.extend(render_row(editor, row, is_selected));
    }

    let inner_height = chunks[1].height.saturating_sub(2) as usize;
    editor.viewport_height = inner_height;
    editor.scroll_offset = clamp_scroll(editor.scroll_offset, selected_line, lines.len(), inner_height);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::Blue));
    frame.render_widget(
        Paragraph::new(lines).block(block).scroll((editor.scroll_offset as u16, 0)),
        chunks[1],
    );

    frame.render_widget(
        Paragraph::new(status_text(editor, keymap)).style(Style::default().bg(Color::DarkGray).fg(Color::White)),
        chunks[2],
    );
}

/// Mirrors `OutlineView._scroll_selected_into_view`: nudge the viewport by
/// the minimum needed to keep the selected line on screen.
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

fn breadcrumb_text(editor: &Editor) -> String {
    let outline = &editor.outline;
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

fn status_text(editor: &Editor, keymap: &dyn Keymap) -> String {
    if let Some(hint) = editor.jump_hint() {
        return format!("-- JUMP --  {hint}  (Esc to cancel)");
    }
    let hide = if editor.outline.hide_completed { "on" } else { "off" };
    let help = keymap.help(editor.mode());
    match editor.style() {
        EditingStyle::Modal => {
            let mode = match editor.mode() {
                Mode::Normal => "NORMAL",
                Mode::Insert => "INSERT",
            };
            format!("-- {mode} --  {help}  |  hide-done:{hide}")
        }
        EditingStyle::Traditional => format!("{help}  |  hide-done:{hide}"),
    }
}

/// Port of `_render_row`. Rich styles ranges of a plain string in place;
/// ratatui has no such object, so this builds one `(char, Style)` cell per
/// character, layers tags -> cursor -> jump labels in the same order as the
/// Python version, then coalesces runs of equal style into `Span`s.
fn render_row(editor: &Editor, row: &Row, is_selected: bool) -> Vec<Line<'static>> {
    let node = editor.outline.get(row.node);
    let insert = editor.mode() == Mode::Insert;

    let (prefix, bullet) = if row.is_header {
        (String::new(), "» ")
    } else if row.has_children {
        ("  ".repeat(row.depth), if row.visible_children { "▾ " } else { "▸ " })
    } else {
        ("  ".repeat(row.depth), "• ")
    };

    let mut text_style = Style::default();
    if row.is_header {
        text_style = text_style.add_modifier(Modifier::BOLD | Modifier::UNDERLINED);
    }
    if node.completed {
        text_style = text_style.add_modifier(Modifier::CROSSED_OUT | Modifier::DIM);
    }
    let mut cells: Cells = node.text.chars().map(|c| (c, text_style)).collect();
    apply_tag_highlight(&mut cells, &node.text);
    if is_selected && !editor.editing_note {
        apply_cursor(&mut cells, editor.cursor, insert);
    }
    let labels = editor.jump_labels_for(row.node);
    if !labels.is_empty() {
        cells = overlay_labels(&node.text, &labels);
    }

    let mut first = vec![Span::raw(prefix), Span::styled(bullet, bullet_style(row.has_children, is_selected))];
    first.extend(cells_to_spans(cells));
    let mut lines = vec![Line::from(first)];

    if editor.shows_note(row.node) {
        let note_prefix = "  ".repeat(row.depth + 1) + "  ";
        let note_style = Style::default().fg(Color::Gray).add_modifier(Modifier::ITALIC);
        let mut cells: Cells = node.note.chars().map(|c| (c, note_style)).collect();
        if is_selected && editor.editing_note {
            apply_cursor(&mut cells, editor.cursor, insert);
        }
        for line_cells in cells.split(|(c, _)| *c == '\n') {
            let mut spans = vec![Span::raw(note_prefix.clone())];
            spans.extend(cells_to_spans(line_cells.to_vec()));
            lines.push(Line::from(spans));
        }
    }
    lines
}

fn bullet_style(has_children: bool, selected: bool) -> Style {
    if selected {
        return Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD);
    }
    if has_children {
        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Gray)
    }
}

fn apply_tag_highlight(cells: &mut Cells, text: &str) {
    let tag_style = Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD);
    for m in TAG_RE.find_iter(text).flatten() {
        let start = text[..m.start()].chars().count();
        let end = (start + text[m.start()..m.end()].chars().count()).min(cells.len());
        for cell in &mut cells[start..end] {
            cell.1 = tag_style;
        }
    }
}

/// Styles the cell under the cursor rather than inserting a glyph, so the
/// text never shifts; past the end (or on a newline) it adds a blank cell.
fn apply_cursor(cells: &mut Cells, cursor: usize, insert: bool) {
    let style = if insert {
        Style::default().fg(Color::Yellow).add_modifier(Modifier::UNDERLINED | Modifier::BOLD)
    } else {
        Style::default().add_modifier(Modifier::REVERSED)
    };
    match cells.get_mut(cursor) {
        Some(cell) if cell.0 != '\n' => cell.1 = style,
        Some(_) => cells.insert(cursor, (' ', style)),
        None => cells.push((' ', style)),
    }
}

/// Highlights each matched character and writes its label over the
/// character to its right (or after the end of the line).
fn overlay_labels(text: &str, labels: &[(usize, char)]) -> Cells {
    let hit = Style::default()
        .fg(Color::White)
        .bg(Color::Rgb(0x2f, 0x69, 0xdf))
        .add_modifier(Modifier::BOLD);
    let tag = Style::default()
        .fg(Color::White)
        .bg(Color::Rgb(0xff, 0x00, 0x7c))
        .add_modifier(Modifier::BOLD);
    let label_at = |pos: usize| labels.iter().find(|(i, _)| i + 1 == pos).map(|(_, l)| *l);

    let chars: Vec<char> = text.chars().collect();
    let mut cells: Cells = chars
        .iter()
        .enumerate()
        .map(|(i, &c)| {
            if labels.iter().any(|(j, _)| *j == i) {
                (c, hit)
            } else if let Some(l) = label_at(i) {
                (l, tag)
            } else {
                (c, Style::default())
            }
        })
        .collect();
    if let Some(l) = label_at(chars.len()) {
        cells.push((l, tag));
    }
    cells
}

fn cells_to_spans(cells: Cells) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let mut current = String::new();
    let mut current_style: Option<Style> = None;
    for (ch, style) in cells {
        if current_style != Some(style) {
            if let Some(s) = current_style {
                spans.push(Span::styled(std::mem::take(&mut current), s));
            }
            current_style = Some(style);
        }
        current.push(ch);
    }
    if let Some(s) = current_style {
        spans.push(Span::styled(current, s));
    }
    spans
}
