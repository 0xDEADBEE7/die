//! A table's columns and borders. Every pipe of a row, with the whitespace round it, is hidden
//! behind a border and the padding that brings the cells beside it to their column's width, so
//! the columns line up whatever the source looks like. Nothing is drawn that stands for no text:
//! a row written without a pipe at an end has no border there.
//!
//! The cells are the parser's. A table whose lines can't be split into those cells and the pipes
//! between them, byte for byte, is left as it's written: no row is half done.

use super::lines::{Lines, cells, is_blank, sub};
use super::{Hide, RULE, shown};
use pulldown_cmark::Alignment;
use std::cmp::Reverse;
use std::ops::Range;

/// A cell as the parser gave it.
pub(super) struct Cell {
    /// What's between its pipes, whitespace included. Empty and at the row's end for a cell
    /// the row doesn't have.
    pub(super) range: Range<usize>,
    /// The hides the inline rules made inside it, as indices into the plan's.
    pub(super) hides: Range<usize>,
}

/// A row as the parser gave it: the header first, then the body's.
pub(super) struct Row {
    /// From its first pipe, or its first cell, through its newline.
    pub(super) range: Range<usize>,
    /// Its cells, as many as the table has columns.
    pub(super) cells: Vec<Cell>,
}

/// A row's line, split.
struct Shape {
    /// Where the row starts.
    start: usize,
    /// Whether a pipe starts it.
    lead: bool,
    /// The pipe that ends it, if one does.
    trail: Option<usize>,
    /// The content of each cell the row has, without the whitespace round it. An empty cell is
    /// an empty range.
    cells: Vec<Range<usize>>,
}

/// A piece of the delimiter row.
enum Piece {
    /// A pipe and the whitespace round it.
    Pipe(Range<usize>),
    /// A column's dashes and colons.
    Dashes(Range<usize>),
}

/// Lays out the table of these rows, whose columns align as `aligns` says, in a text area
/// `width` cells wide with a tab stop every `tab`: one hide for each pipe and each run of
/// dashes, added to `hides`.
pub(super) fn layout(
    text: &str,
    lines: &Lines<'_>,
    (width, tab): (usize, usize),
    aligns: &[Alignment],
    rows: &[Row],
    hides: &mut Vec<Hide>,
) {
    let Some(head) = rows.first() else { return };
    let columns = aligns.len();
    let Some(shapes) = rows
        .iter()
        .map(|row| shape(text, lines, row, columns))
        .collect::<Option<Vec<Shape>>>()
    else {
        return;
    };
    let Some(pieces) = delimiter(text, lines, lines.index(head.range.start) + 1, columns) else {
        return;
    };

    // Each cell's width as it will show, and each column's: its widest cell, and at least
    // three cells, as its dashes are.
    let mut widest = vec![3; columns];
    let mut widths = Vec::with_capacity(rows.len());
    for (row, shape) in rows.iter().zip(&shapes) {
        let mut row_widths = Vec::with_capacity(shape.cells.len());
        for (j, (cell, content)) in row.cells.iter().zip(&shape.cells).enumerate() {
            let inside = hides.get_mut(cell.hides.clone()).unwrap_or_default();
            let Some(cell_width) = shown_width(text, content, inside) else {
                return;
            };
            widest[j] = widest[j].max(cell_width);
            row_widths.push(cell_width);
        }
        widths.push(row_widths);
    }
    // Padding mustn't push a row past the text area's edge, where it would wrap for the sake
    // of spaces: the columns give up width, the widest first, until the header's row fits. A
    // cell wider than its column then has no padding, and only its own row runs over.
    let indent = sub(
        text,
        lines.start(lines.index(head.range.start))..head.range.start,
    );
    let borders = 3 * (shapes[0].cells.len() - 1)
        + 2 * usize::from(shapes[0].lead)
        + 2 * usize::from(shapes[0].trail.is_some());
    let taken = super::lines::columns(indent, 0, tab) + borders;
    fit(&mut widest, width.saturating_sub(taken));

    let spaces = |n: usize| " ".repeat(n);
    for (shape, row_widths) in shapes.iter().zip(&widths) {
        // The spaces owed on each side of each cell.
        let pads: Vec<(usize, usize)> = row_widths
            .iter()
            .enumerate()
            .map(|(j, cell_width)| {
                let spare = widest[j].saturating_sub(*cell_width);
                match aligns[j] {
                    // With no pipe before it there's nothing to hide behind padding on its left.
                    _ if j == 0 && !shape.lead => (0, spare),
                    Alignment::Right => (spare, 0),
                    Alignment::Center => (spare / 2, spare - spare / 2),
                    Alignment::Left | Alignment::None => (0, spare),
                }
            })
            .collect();
        let last = shape.cells.len() - 1;
        let mut border = |range: Range<usize>, text: String| {
            hides.push(Hide {
                range,
                shown: Some(shown(text, RULE)),
            });
        };
        if shape.lead {
            border(
                shape.start..shape.cells[0].start,
                format!("│ {}", spaces(pads[0].0)),
            );
        }
        for j in 1..=last {
            border(
                shape.cells[j - 1].end..shape.cells[j].start,
                format!("{} │ {}", spaces(pads[j - 1].1), spaces(pads[j].0)),
            );
        }
        if let Some(pipe) = shape.trail {
            border(
                shape.cells[last].end..pipe + 1,
                format!("{} │", spaces(pads[last].1)),
            );
        }
    }

    // The delimiter row: a junction for each pipe, and dashes as wide as the column with the
    // space a border has on each side of it.
    let mut column = 0;
    for (i, piece) in pieces.iter().enumerate() {
        let (range, text) = match piece {
            Piece::Pipe(range) => {
                let junction = if column == 0 {
                    "├"
                } else if column == columns && i == pieces.len() - 1 {
                    "┤"
                } else {
                    "┼"
                };
                (range.clone(), junction.to_string())
            }
            Piece::Dashes(range) => {
                let sides = usize::from(i > 0) + usize::from(i + 1 < pieces.len());
                column += 1;
                (range.clone(), "─".repeat(widest[column - 1] + sides))
            }
        };
        hides.push(Hide {
            range,
            shown: Some(shown(text, RULE)),
        });
    }
}

/// Narrows the widest columns until all of them take `room` cells between them, or each is
/// down to three.
fn fit(widest: &mut [usize], room: usize) {
    let within = |limit: usize| widest.iter().map(|w| (*w).min(limit)).sum::<usize>() <= room;
    let (mut low, mut high) = (3, widest.iter().copied().max().unwrap_or(3));
    if within(high) {
        return;
    }
    // The widest a column may stay: the largest limit under which they fit.
    while low < high {
        let limit = low + (high - low).div_ceil(2);
        if within(limit) {
            low = limit;
        } else {
            high = limit - 1;
        }
    }
    for column in widest {
        *column = (*column).min(low);
    }
}

/// Splits a row's line into its cells and the pipes between them, or says it can't be.
fn shape(text: &str, lines: &Lines<'_>, row: &Row, columns: usize) -> Option<Shape> {
    let bytes = text.as_bytes();
    let start = row.range.start;
    let line = lines.index(start);
    let end = lines.end(line);
    if start >= end || row.cells.len() > columns {
        return None;
    }
    let lead = bytes[start] == b'|';

    // The cells the row has come first, each right after the pipe that ends the one before;
    // then the empty ones the parser made up for a short row.
    let mut real: Vec<Range<usize>> = Vec::new();
    let mut next = Some(start + usize::from(lead));
    for cell in &row.cells {
        let Range {
            start: from,
            end: to,
        } = cell.range;
        let closed = bytes.get(to) == Some(&b'|') && to < end;
        if next == Some(from) && from <= to && to <= end && (from < to || closed) {
            real.push(from..to);
            next = closed.then_some(to + 1);
        } else if from == to {
            next = None;
        } else {
            return None;
        }
    }
    let last = real.last()?.clone();

    // After the last cell: its pipe and nothing more, or nothing at all. Anything else is a
    // cell the parser dropped.
    let stop = lines.trimmed(line, last.end);
    let piped = last.end < end && bytes[last.end] == b'|';
    if stop != last.end + usize::from(piped) {
        return None;
    }
    let trail = piped.then_some(last.end);

    // Each cell's content: an empty cell's is at its closing pipe, so the whitespace between
    // the two pipes goes with the first, or just after the last pipe if nothing closes it.
    let cells = real
        .iter()
        .map(|cell| {
            let (mut from, mut to) = (cell.start, cell.end);
            while from < to && is_blank(bytes[from]) {
                from += 1;
            }
            while to > from && is_blank(bytes[to - 1]) {
                to -= 1;
            }
            if from < to {
                from..to
            } else if bytes.get(cell.end) == Some(&b'|') && cell.end < end {
                cell.end..cell.end
            } else {
                cell.start..cell.start
            }
        })
        .collect();
    Some(Shape {
        start,
        lead,
        trail,
        cells,
    })
}

/// Splits the delimiter row, the line under the header, into its pipes and its runs of dashes:
/// one run for each column, a pipe between each two, and maybe one at each end.
fn delimiter(text: &str, lines: &Lines<'_>, line: usize, columns: usize) -> Option<Vec<Piece>> {
    let bytes = text.as_bytes();
    if line >= lines.count() {
        return None;
    }
    let mut at = lines.inset(line);
    let end = lines.trimmed(line, at);
    let mut pieces = Vec::with_capacity(2 * columns + 1);
    let mut runs = 0;
    while at < end {
        let from = at;
        if matches!(bytes[at], b'-' | b':') {
            while at < end && matches!(bytes[at], b'-' | b':') {
                at += 1;
            }
            if matches!(pieces.last(), Some(Piece::Dashes(_))) {
                return None;
            }
            runs += 1;
            pieces.push(Piece::Dashes(from..at));
        } else {
            while at < end && is_blank(bytes[at]) {
                at += 1;
            }
            if bytes.get(at) != Some(&b'|') || at >= end {
                return None;
            }
            at += 1;
            while at < end && is_blank(bytes[at]) {
                at += 1;
            }
            if matches!(pieces.last(), Some(Piece::Pipe(_))) {
                return None;
            }
            pieces.push(Piece::Pipe(from..at));
        }
    }
    (runs == columns && columns > 0).then_some(pieces)
}

/// The cells a cell's content takes once the hides inside it are applied, as the plan's last
/// pass will apply them: of two that overlap the one that starts first, of two that start
/// together the longer. None for a content with a tab in it, whose width depends on where its
/// column ends up.
fn shown_width(text: &str, content: &Range<usize>, hides: &mut [Hide]) -> Option<usize> {
    if sub(text, content.clone()).contains('\t') {
        return None;
    }
    hides.sort_by_key(|h| (h.range.start, Reverse(h.range.end)));
    let mut width = 0;
    let mut at = content.start;
    for hide in hides.iter() {
        let Range { start, end } = hide.range;
        if start < at || end > content.end || start >= end {
            continue;
        }
        width += cells(sub(text, at..start));
        width += hide.shown.as_ref().map_or(0, |s| cells(&s.text));
        at = end;
    }
    Some(width + cells(sub(text, at..content.end)))
}
