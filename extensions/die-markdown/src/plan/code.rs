//! A code block's box: a row for each source line, in the code block's face from where the code
//! starts to a right edge one cell past the longest line, or at the text area's edge if that's
//! nearer. A fenced block's opening row shows its language and its closing row is blank.

use super::lines::{cells, columns, is_blank};
use super::walk::Walk;
use super::{CODE_BLOCK, META, shown};
use std::ops::Range;

/// A row of the box.
struct Row {
    /// Where its newline is, or the text's end: where its padding goes.
    end: usize,
    /// The display column the box starts at on this row.
    start: usize,
    /// The cells it shows from there.
    width: usize,
}

/// Lays out the box of the code block at `range`. `info` is a fenced block's info string, and
/// `chunks` the pieces of the text the parser called its code, in order.
pub(super) fn layout(
    walk: &mut Walk<'_>,
    range: Range<usize>,
    info: Option<&str>,
    chunks: &[Range<usize>],
) {
    let (text, bytes) = (walk.text, walk.text.as_bytes());
    if range.is_empty() || range.end > text.len() {
        return;
    }
    let first = walk.lines.index(range.start);
    let last = walk.lines.index(range.end - 1);
    // The fence's place in its line, which a line the parser gave no code for starts at too.
    let offset = range.start - walk.lines.start(first);
    let fence = info.filter(|_| matches!(bytes[range.start], b'`' | b'~'));
    let closed = fence.is_some() && last > first && closes(walk, &range, last, chunks);

    let mut rows = Vec::with_capacity(last - first + 1);
    let mut chunk = 0;
    for line in first..=last {
        let (start, end) = (walk.lines.start(line), walk.lines.end(line));
        // Where a line no code was given for starts: as far in as the fence, less if the line
        // has less before its own text.
        let fallback = (start + offset).min(walk.lines.inset(line));
        let (from, label) = if line == first && fence.is_some() {
            let word = fence.and_then(|info| info.split_whitespace().next());
            (range.start, Some(word.unwrap_or("")))
        } else if line == last && closed {
            (fallback, Some(""))
        } else if line == first {
            (range.start, None)
        } else {
            // The code on this line starts where the parser's piece of it does: at the line's
            // start inside a piece that runs over lines, since nothing was taken off those.
            while chunks.get(chunk).is_some_and(|c| c.end <= start) {
                chunk += 1;
            }
            let from = match chunks.get(chunk) {
                Some(c) if c.start <= start => start,
                Some(c) if c.start <= end => c.start,
                _ => fallback,
            };
            (from, None)
        };
        let from = from.clamp(start, end);
        let before = columns(text.get(start..from).unwrap_or(""), 0, walk.tab);
        let width = match label {
            // A fence row: the fence goes, and the language shows on the opening one.
            Some(word) => {
                let word = (!word.is_empty()).then(|| shown(word, META));
                walk.hide(from..end, word.clone());
                word.map_or(0, |w| cells(&w.text))
            }
            None => columns(text.get(from..end).unwrap_or(""), before, walk.tab) - before,
        };
        // The face runs over the newline, so the box has no gap between its rows.
        walk.face(from..walk.lines.after(line), CODE_BLOCK);
        rows.push(Row {
            end,
            start: before,
            width,
        });
    }

    let longest = rows.iter().map(|r| r.start + r.width).max().unwrap_or(0);
    let edge = walk.width.min(longest + 1);
    for row in rows {
        // A line wider than the box wraps, and has no padding.
        let used = row.start + row.width;
        if used < edge {
            walk.virtual_text(row.end, shown(" ".repeat(edge - used), CODE_BLOCK));
        }
    }
}

/// Whether the block's last line is its closing fence: no code on it, and a run of the fence's
/// character at least as long as the opening one, alone on the line. A fence left open ends at
/// the end of its container or of the text, on a line of code.
fn closes(walk: &Walk<'_>, range: &Range<usize>, last: usize, chunks: &[Range<usize>]) -> bool {
    let bytes = walk.text.as_bytes();
    let (start, end) = (walk.lines.start(last), walk.lines.end(last));
    if chunks.last().is_some_and(|c| c.end > start) {
        return false;
    }
    let mark = bytes[range.start];
    let run = |from: usize| bytes[from..end].iter().take_while(|b| **b == mark).count();
    let opening = bytes[range.start..]
        .iter()
        .take_while(|b| **b == mark)
        .count();
    let at = walk.lines.inset(last);
    let closing = run(at);
    closing >= opening && bytes[at + closing..end].iter().all(|b| is_blank(*b))
}
