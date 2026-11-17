//! Lines and cells: where each line of the text starts and ends, found once so that no rule
//! searches the text for a newline again, and how many cells of the screen a piece of text takes.

use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

/// The text's lines, as markdown reads them. A line ends at its `\n`, as a row of the editor
/// does, and the `\r` of a `\r\n` is the line's last character, which the editor draws. A `\r`
/// alone ends a line too: the editor draws it inside a row, but the parser starts a new line
/// after it, and a rule that reads to a line's end must stop where the parser's line does or it
/// would take the next one's text for markup.
pub(super) struct Lines<'a> {
    text: &'a str,
    /// Where each line starts. A text that ends in a newline has an empty last line.
    starts: Vec<usize>,
}

impl<'a> Lines<'a> {
    /// The lines of `text`.
    pub(super) fn new(text: &'a str) -> Lines<'a> {
        let bytes = text.as_bytes();
        let mut starts = vec![0];
        starts.extend(bytes.iter().enumerate().filter_map(|(i, b)| {
            let ends = *b == b'\n' || (*b == b'\r' && bytes.get(i + 1) != Some(&b'\n'));
            ends.then_some(i + 1)
        }));
        Lines { text, starts }
    }

    /// The line the byte at `at` is on. A newline is on the line it ends.
    pub(super) fn index(&self, at: usize) -> usize {
        self.starts.partition_point(|s| *s <= at).saturating_sub(1)
    }

    /// Where a line starts.
    pub(super) fn start(&self, line: usize) -> usize {
        self.starts.get(line).copied().unwrap_or(self.text.len())
    }

    /// Where a line's newline is (the `\n`, or a `\r` alone), or the text's end for a last line
    /// without one.
    pub(super) fn end(&self, line: usize) -> usize {
        match self.starts.get(line + 1) {
            Some(next) => next - 1,
            None => self.text.len(),
        }
    }

    /// Just after a line's newline: where the next line starts, or the text's end.
    pub(super) fn after(&self, line: usize) -> usize {
        self.start(line + 1)
    }

    /// How many lines there are.
    pub(super) fn count(&self) -> usize {
        self.starts.len()
    }

    /// Where a line's own text starts: after the spaces, tabs and `>` a container puts before it.
    pub(super) fn inset(&self, line: usize) -> usize {
        let bytes = self.text.as_bytes();
        let (mut at, end) = (self.start(line), self.end(line));
        while at < end && matches!(bytes[at], b' ' | b'\t' | b'>') {
            at += 1;
        }
        at
    }

    /// A line's end without the spaces, tabs and `\r` that trail it, and no earlier than `floor`.
    pub(super) fn trimmed(&self, line: usize, floor: usize) -> usize {
        let bytes = self.text.as_bytes();
        let mut at = self.end(line);
        while at > floor && is_blank(bytes[at - 1]) {
            at -= 1;
        }
        at.max(floor)
    }
}

/// Whether a byte is a space, a tab or a `\r`: what markdown counts as nothing at a line's edge.
pub(super) fn is_blank(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\r')
}

/// A piece of the text, or nothing if the range isn't one: no range from the parser or from a
/// scan can make a slice panic.
pub(super) fn sub(text: &str, range: std::ops::Range<usize>) -> &str {
    text.get(range).unwrap_or("")
}

/// The display column after `s`, which starts at column `from` of its line, as the editor draws
/// it: a cluster takes its width and at least one cell, a tab runs to the next stop, and a control
/// character is drawn as `^X` (two cells) or `\200` (four).
pub(super) fn columns(s: &str, from: usize, tab: usize) -> usize {
    let tab = tab.max(1);
    let mut col = from;
    if s.is_ascii() {
        for b in s.bytes() {
            col += match b {
                b'\t' => tab - col % tab,
                0..=0x1f | 0x7f => 2,
                _ => 1,
            };
        }
        return col;
    }
    for cluster in s.graphemes(true) {
        col += match cluster.chars().next() {
            Some('\t') => tab - col % tab,
            Some(c) if c.is_control() => {
                if (c as u32) < 0x80 {
                    2
                } else {
                    4
                }
            }
            _ => cluster.width().max(1),
        };
    }
    col
}

/// The cells a piece of shown text takes: one with no tab in it.
pub(super) fn cells(s: &str) -> usize {
    columns(s, 0, 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_end_at_their_newlines() {
        let lines = Lines::new("ab\n\ncd");
        assert_eq!(lines.count(), 3);
        assert_eq!((lines.start(0), lines.end(0), lines.after(0)), (0, 2, 3));
        assert_eq!((lines.start(1), lines.end(1)), (3, 3));
        assert_eq!((lines.start(2), lines.end(2), lines.after(2)), (4, 6, 6));
        // A newline is on the line it ends, and the text's end is on the last line.
        assert_eq!(
            [0, 2, 3, 4, 6, 99].map(|at| lines.index(at)),
            [0, 0, 1, 2, 2, 2]
        );
        // A text that ends in a newline has an empty last line.
        let lines = Lines::new("ab\n");
        assert_eq!(lines.count(), 2);
        assert_eq!((lines.start(1), lines.end(1)), (3, 3));
        // A `\r` alone ends a line, and the `\r` of a `\r\n` is its line's last character.
        let lines = Lines::new("a\rb\r\nc");
        assert_eq!(lines.count(), 3);
        assert_eq!((lines.start(0), lines.end(0), lines.after(0)), (0, 1, 2));
        assert_eq!((lines.start(1), lines.end(1), lines.after(1)), (2, 4, 5));
    }

    #[test]
    fn a_line_without_its_container_or_its_trailing_blanks() {
        let lines = Lines::new(">  > a  \r\n\t- b");
        assert_eq!(lines.inset(0), 5);
        assert_eq!(lines.trimmed(0, 0), 6);
        assert_eq!(lines.trimmed(0, 8), 8);
        assert_eq!(lines.inset(1), 11);
    }

    #[test]
    fn cells_as_the_editor_draws_them() {
        assert_eq!(cells("abc"), 3);
        // A tab runs to the next stop, counted from the line's start.
        assert_eq!(columns("a\tb", 0, 8), 9);
        assert_eq!(columns("\t", 3, 4), 4);
        assert_eq!(columns("\t", 4, 4), 8);
        // A control character is `^X`, and one of the second set `\200`.
        assert_eq!(cells("\u{0}\u{7f}"), 4);
        assert_eq!(cells("é\u{85}"), 5);
        // Wide characters take two cells, a combining mark none of its own, and a family of
        // emoji joined into one cluster is one cluster's width.
        assert_eq!(cells("日本"), 4);
        assert_eq!(cells("e\u{301}"), 1);
        assert_eq!(cells("👨‍👩‍👧"), 2);
    }
}
