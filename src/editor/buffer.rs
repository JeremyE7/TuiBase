use super::commands::VisualMode;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SelectionRange {
    pub start: (usize, usize),
    pub end: (usize, usize),
    pub mode: VisualMode,
}

pub fn split_text(text: &str) -> Vec<String> {
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    let mut lines: Vec<String> = normalized.split('\n').map(ToOwned::to_owned).collect();
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

pub fn char_len(line: &str) -> usize {
    line.chars().count()
}

pub fn char_to_byte(line: &str, column: usize) -> usize {
    line.char_indices()
        .nth(column)
        .map(|(index, _)| index)
        .unwrap_or(line.len())
}

pub fn char_slice(line: &str, start: usize, end: usize) -> String {
    let start = start.min(char_len(line));
    let end = end.max(start).min(char_len(line));
    let start_byte = char_to_byte(line, start);
    let end_byte = char_to_byte(line, end);
    line[start_byte..end_byte].to_owned()
}

pub fn ordered_positions(
    first: (usize, usize),
    second: (usize, usize),
) -> ((usize, usize), (usize, usize)) {
    if first <= second {
        (first, second)
    } else {
        (second, first)
    }
}

pub fn selection_range(
    lines: &[String],
    anchor: (usize, usize),
    cursor: (usize, usize),
    mode: VisualMode,
) -> SelectionRange {
    if lines.is_empty() {
        return SelectionRange {
            start: (0, 0),
            end: (0, 0),
            mode,
        };
    }

    let last_row = lines.len().saturating_sub(1);
    let anchor = (
        anchor.0.min(last_row),
        anchor.1.min(char_len(&lines[anchor.0.min(last_row)])),
    );
    let cursor = (
        cursor.0.min(last_row),
        cursor.1.min(char_len(&lines[cursor.0.min(last_row)])),
    );
    match mode {
        VisualMode::Character => {
            let (start, end) = ordered_positions(anchor, cursor);
            SelectionRange {
                start,
                end: (end.0, (end.1 + 1).min(char_len(&lines[end.0]))),
                mode,
            }
        }
        VisualMode::Line => {
            let start_row = anchor.0.min(cursor.0);
            let end_row = anchor.0.max(cursor.0);
            SelectionRange {
                start: (start_row, 0),
                end: (end_row, char_len(&lines[end_row])),
                mode,
            }
        }
        VisualMode::Block => {
            let start_row = anchor.0.min(cursor.0);
            let end_row = anchor.0.max(cursor.0);
            let start_column = anchor.1.min(cursor.1);
            let end_column = anchor.1.max(cursor.1).saturating_add(1);
            SelectionRange {
                start: (start_row, start_column),
                end: (end_row, end_column),
                mode,
            }
        }
    }
}

pub fn selected_text(
    lines: &[String],
    anchor: (usize, usize),
    cursor: (usize, usize),
    mode: VisualMode,
) -> String {
    if lines.is_empty() {
        return String::new();
    }

    let range = selection_range(lines, anchor, cursor, mode);
    match mode {
        VisualMode::Character => {
            let mut selected = Vec::new();
            for row in range.start.0..=range.end.0 {
                let from = if row == range.start.0 {
                    range.start.1
                } else {
                    0
                };
                let to = if row == range.end.0 {
                    range.end.1
                } else {
                    char_len(&lines[row])
                };
                selected.push(char_slice(&lines[row], from, to));
            }
            selected.join("\n")
        }
        VisualMode::Line => lines[range.start.0..=range.end.0].join("\n"),
        VisualMode::Block => {
            let mut selected = Vec::new();
            for row in range.start.0..=range.end.0 {
                selected.push(char_slice(&lines[row], range.start.1, range.end.1));
            }
            selected.join("\n")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{char_slice, selected_text};
    use crate::editor::VisualMode;

    #[test]
    fn slices_unicode_by_character_column() {
        assert_eq!(char_slice("áéx", 1, 2), "é");
    }

    #[test]
    fn selects_unicode_characterwise_text() {
        let lines = vec!["select á".to_owned(), "segunda línea".to_owned()];
        assert_eq!(
            selected_text(&lines, (0, 7), (1, 2), VisualMode::Character),
            "á\nseg"
        );
    }

    #[test]
    fn selects_complete_lines() {
        let lines = vec!["uno".to_owned(), "dos".to_owned(), "tres".to_owned()];
        assert_eq!(
            selected_text(&lines, (1, 1), (2, 0), VisualMode::Line),
            "dos\ntres"
        );
    }

    #[test]
    fn selects_a_reversed_block_by_row_and_column() {
        let lines = vec!["abcd".to_owned(), "WXYZ".to_owned()];
        assert_eq!(
            selected_text(&lines, (1, 3), (0, 1), VisualMode::Block),
            "bcd\nXYZ"
        );
    }
}
