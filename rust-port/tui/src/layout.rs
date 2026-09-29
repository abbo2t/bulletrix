//! Row geometry and line wrapping, shared by drawing, scrolling and mouse
//! hit-testing so they can never disagree about what's where.

use model::Row;
use std::ops::Range;
use unicode_width::UnicodeWidthChar;

/// Column offsets within the outline area where a row's parts start.
pub struct RowLayout {
    pub bullet: u16,
    pub text: u16,
    pub note: u16,
}

pub fn row_layout(row: &Row) -> RowLayout {
    let indent = if row.is_header { 0 } else { 2 * row.depth as u16 };
    RowLayout {
        bullet: indent,
        text: indent + 2,
        note: 2 * row.depth as u16 + 4,
    }
}

impl RowLayout {
    /// Columns the item's text can use in an outline area `width` columns wide.
    pub fn text_width(&self, width: usize) -> usize {
        wrap_width(width, self.text)
    }

    pub fn note_width(&self, width: usize) -> usize {
        wrap_width(width, self.note)
    }
}

/// `width` 0 means not yet known (nothing drawn yet): don't wrap. One column
/// is kept free so a cursor after the last character is always visible.
fn wrap_width(width: usize, start: u16) -> usize {
    if width == 0 {
        usize::MAX
    } else {
        width.saturating_sub(start as usize + 1).max(1)
    }
}

/// Splits `text` into character ranges, one per screen line, each at most
/// `width` columns (wide characters count as two). Breaks after the last
/// space that fits; a space that lands exactly on the edge hangs into the
/// free column rather than starting the next line; a word longer than a
/// line is broken at the edge. The ranges are contiguous, cover every
/// character, and there's always at least one (possibly empty).
pub fn wrap(text: &str, width: usize) -> Vec<Range<usize>> {
    let chars: Vec<char> = text.chars().collect();
    let mut lines = Vec::new();
    let mut start = 0;
    let mut col = 0;
    let mut last_space = None;
    for (i, &ch) in chars.iter().enumerate() {
        let w = ch.width().unwrap_or(0);
        if col + w > width && i > start {
            if ch == ' ' {
                lines.push(start..i + 1);
                (start, col, last_space) = (i + 1, 0, None);
                continue;
            }
            let brk = last_space.map_or(i, |s: usize| s + 1);
            lines.push(start..brk);
            col = chars[brk..i].iter().map(|c| c.width().unwrap_or(0)).sum();
            (start, last_space) = (brk, None);
        }
        if ch == ' ' {
            last_space = Some(i);
        }
        col += w;
    }
    lines.push(start..chars.len());
    lines
}

/// Which of `lines` (from `wrap`) the cursor at character `cursor` is on. A
/// cursor at a break point belongs to the line that starts there.
pub fn line_of(lines: &[Range<usize>], cursor: usize) -> usize {
    lines.iter().position(|r| cursor < r.end).unwrap_or(lines.len() - 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pieces(text: &str, width: usize) -> Vec<String> {
        wrap(text, width)
            .into_iter()
            .map(|r| text.chars().skip(r.start).take(r.len()).collect())
            .collect()
    }

    #[test]
    fn breaks_after_the_last_space_that_fits() {
        assert_eq!(pieces("the quick brown fox", 10), ["the quick ", "brown fox"]);
    }

    #[test]
    fn a_space_exactly_at_the_edge_hangs_instead_of_starting_the_next_line() {
        assert_eq!(pieces("hello world foo", 11), ["hello world ", "foo"]);
    }

    #[test]
    fn a_word_longer_than_the_line_breaks_at_the_edge() {
        assert_eq!(pieces("abcdefghij", 4), ["abcd", "efgh", "ij"]);
        assert_eq!(pieces("a verylongword", 5), ["a ", "veryl", "ongwo", "rd"]);
    }

    #[test]
    fn wide_characters_take_two_columns() {
        assert_eq!(pieces("日本語", 4), ["日本", "語"]);
    }

    #[test]
    fn short_and_empty_text_is_one_line() {
        assert_eq!(pieces("", 10), [""]);
        assert_eq!(pieces("fits", 10), ["fits"]);
        assert_eq!(pieces("no limit at all", usize::MAX), ["no limit at all"]);
    }

    #[test]
    fn a_cursor_at_a_break_point_belongs_to_the_next_line() {
        let lines = wrap("the quick brown fox", 10); // [0..10, 10..19]
        assert_eq!(line_of(&lines, 9), 0);
        assert_eq!(line_of(&lines, 10), 1);
        assert_eq!(line_of(&lines, 19), 1, "after the last character");
    }
}
