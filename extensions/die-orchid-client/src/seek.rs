//! seek, your scrollable, searchable list browser (`~/.emacs.d/lisp/seek/seek.el`), which the
//! Emacs client's session browser draws through: a browser's state in, styled lines out, with
//! no buffer and no window of its own, so the width and the height come in too, where seek read
//! them from the selected window (and orchid swapped in a fake `window-width` to clamp it).
//!
//! The layout is seek's `seek-render`:
//!
//! ```text
//! ──────────── Title ────────────   bold
//! ───────────────────────────────   shadow
//! <header lines, then a separator, when there are any>
//! <the query, padded>               while searching, the whole line in seek's search face
//! ───────────────────────────────
//! <columns>                         bold
//! ───────────────────────────────
//! <rows, from the scroll offset>    the selected one padded and wholly highlight
//! No results                        shadow, when no row matches
//! ```
//!
//! One thing differs: seek counted 4 lines of chrome but drew 6, so a window showed two rows
//! fewer than it had room for (the Emacs client's B7). Here the count is what's drawn. Padding
//! counts columns, as Emacs's `string-width` does, where seek counted characters.
//!
//! It becomes die-seek once a second package (scrib) needs it.

use crate::parse::width;
use std::ops::Range;

/// The most rows a browser shows at once (`seek-max-visible-rows`).
pub const MAX_VISIBLE: usize = 20;

/// The faces seek draws in: Emacs's base faces, and its search line's own background.
pub mod face {
    /// The title bar (`bold`).
    pub const TITLE: &str = "bold";
    /// The separators (`shadow`).
    pub const SEPARATOR: &str = "shadow";
    /// The header lines (`shadow`).
    pub const HEADER: &str = "shadow";
    /// The columns' titles (`bold`).
    pub const COLUMNS: &str = "bold";
    /// `No results` (`shadow`).
    pub const EMPTY: &str = "shadow";
    /// The selected row (`highlight`).
    pub const SELECTED: &str = "highlight";
    /// The search line while searching, seek's `(:background "#3a3a3a")`, which covers the
    /// query's `isearch` too. orchid declares it, on `highlight`.
    pub const SEARCH: &str = "orchid-seek-search";
}

/// A line of text, without its newline, and faces on byte ranges of it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Line {
    /// The text.
    pub text: String,
    /// Faces, on byte ranges of `text`, in the order they stack.
    pub spans: Vec<(Range<usize>, &'static str)>,
}

impl Line {
    /// `text` in `face`, or in none.
    pub fn new(text: &str, face: Option<&'static str>) -> Line {
        let mut line = Line::default();
        line.push(text, face);
        line
    }

    /// Add `text` at the end, in `face` or in none.
    pub fn push(&mut self, text: &str, face: Option<&'static str>) -> &mut Line {
        let start = self.text.len();
        self.text.push_str(text);
        if let Some(f) = face.filter(|_| !text.is_empty()) {
            self.spans.push((start..self.text.len(), f));
        }
        self
    }

    /// Spaces at the end, to `cols` columns.
    pub fn pad(&mut self, cols: usize) -> &mut Line {
        let spaces = " ".repeat(cols.saturating_sub(width(&self.text)));
        self.push(&spaces, None)
    }

    /// The whole line in `face` alone, as Emacs's `propertize` sets a face over the faces a
    /// string had.
    pub fn only(mut self, face: &'static str) -> Line {
        self.spans = vec![(0..self.text.len(), face)];
        if self.text.is_empty() {
            self.spans.clear();
        }
        self
    }

    /// Cut to `cols` columns, as `truncate-string-to-width` cuts with no ellipsis: a wide
    /// character that would cross the edge goes. Its faces are cut with it.
    pub fn truncated(mut self, cols: usize) -> Line {
        let cut = crate::parse::truncate_width(&self.text, cols, "");
        if cut.len() < self.text.len() {
            let end = cut.len();
            self.text = cut;
            self.spans = self
                .spans
                .into_iter()
                .filter(|(r, _)| r.start < end)
                .map(|(r, f)| (r.start..r.end.min(end), f))
                .collect();
        }
        self
    }
}

/// A browser's state, as seek's `seek-browser` struct holds it, less the rows, which come in
/// filtered.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct View<'a> {
    /// Centred in the title bar, with its own spaces round it.
    pub title: &'a str,
    /// Lines between the title and the search line, if any.
    pub header_lines: &'a [String],
    /// The columns' titles.
    pub columns: &'a str,
    /// The query.
    pub search: &'a str,
    /// Whether the query is being typed (seek's `search` search-mode).
    pub searching: bool,
    /// The selected row's index among the filtered rows.
    pub selected: usize,
    /// The first row shown.
    pub scroll: usize,
}

/// The rows `query` keeps, in order: all of them for an empty query, else those `keep` says
/// match (`seek--filtered-rows`).
pub fn filtered<'r, R>(all: &'r [R], query: &str, keep: impl Fn(&str, &R) -> bool) -> Vec<&'r R> {
    if query.is_empty() {
        return all.iter().collect();
    }
    all.iter().filter(|r| keep(query, r)).collect()
}

/// The lines drawn above the rows: the title, the search line, the columns and their three
/// separators, and with header lines, those and one more separator.
pub fn chrome(header_lines: usize) -> usize {
    6 + if header_lines > 0 {
        header_lines + 1
    } else {
        0
    }
}

/// How many rows a window `body_rows` high shows under `chrome` lines: at least one, at most
/// [`MAX_VISIBLE`] (`seek--visible`).
pub fn visible(body_rows: usize, chrome: usize) -> usize {
    MAX_VISIBLE.min(body_rows.saturating_sub(chrome).max(1))
}

/// The selection kept among `rows` rows, and the scroll offset moved so it shows among
/// `visible` (`seek-clamp`).
pub fn clamp(rows: usize, selected: usize, scroll: usize, visible: usize) -> (usize, usize) {
    let sel = selected.min(rows.saturating_sub(1));
    let mut off = scroll;
    if sel < off {
        off = sel;
    }
    if sel >= off + visible {
        off = sel + 1 - visible;
    }
    (sel, off)
}

/// The lines `seek-render` draws for `view` over `rows`, the filtered rows, `width` columns
/// wide, in a window `body_rows` high. `row` draws a row, as seek's `render-row` did, given the
/// width and whether it's selected. `view`'s selection and scroll should be [`clamp`]ed.
pub fn render<R>(
    view: &View,
    rows: &[R],
    width: usize,
    body_rows: usize,
    row: impl Fn(&R, usize, bool) -> Line,
) -> Vec<Line> {
    let sep = Line::new(&"─".repeat(width), Some(face::SEPARATOR));
    let shown = visible(body_rows, chrome(view.header_lines.len()));
    let mut out = vec![];
    let title_len = view.title.chars().count();
    let pad = width.saturating_sub(title_len) / 2;
    let rest = width.saturating_sub(pad + title_len);
    let title = format!("{}{}{}", "─".repeat(pad), view.title, "─".repeat(rest));
    out.push(Line::new(&title, Some(face::TITLE)));
    out.push(sep.clone());
    if !view.header_lines.is_empty() {
        for h in view.header_lines {
            out.push(Line::new(h, Some(face::HEADER)));
        }
        out.push(sep.clone());
    }
    let mut search = Line::new(view.search, None);
    search.pad(width);
    out.push(if view.searching {
        search.only(face::SEARCH)
    } else {
        search
    });
    out.push(sep.clone());
    out.push(Line::new(view.columns, Some(face::COLUMNS)));
    out.push(sep);
    if rows.is_empty() {
        out.push(Line::new("No results", Some(face::EMPTY)));
        return out;
    }
    let start = view.scroll.min(rows.len());
    let end = (view.scroll + shown).min(rows.len());
    for (i, r) in rows.iter().enumerate().take(end).skip(start) {
        let selected = i == view.selected;
        let mut line = row(r, width, selected);
        line.pad(width);
        out.push(if selected {
            line.only(face::SELECTED)
        } else {
            line
        });
    }
    out
}

/// The lines as a buffer's text, each with its newline, and their faces at their places in it.
pub fn join(lines: &[Line]) -> (String, Vec<(Range<usize>, &'static str)>) {
    let mut text = String::new();
    let mut spans = vec![];
    for l in lines {
        let at = text.len();
        text.push_str(&l.text);
        text.push('\n');
        spans.extend(l.spans.iter().map(|(r, f)| (r.start + at..r.end + at, *f)));
    }
    (text, spans)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(lines: &[Line]) -> Vec<String> {
        lines.iter().map(|l| l.text.clone()).collect()
    }

    fn view<'a>() -> View<'a> {
        View {
            title: " Seek ",
            columns: "NAME",
            ..View::default()
        }
    }

    #[test]
    fn the_chrome_is_six_lines_as_drawn_and_a_window_shows_rows_under_it() {
        // seek counted 4 (B7).
        assert_eq!(chrome(0), 6);
        assert_eq!(chrome(2), 9);
        assert_eq!(visible(24, 6), 18);
        assert_eq!(visible(40, 6), MAX_VISIBLE);
        assert_eq!(visible(5, 6), 1, "one row at least");
        let rows: Vec<&str> = (0..30).map(|_| "r").collect();
        let lines = render(&view(), &rows, 20, 10, |r, _, _| Line::new(r, None));
        assert_eq!(lines.len(), 10, "a 10-row window, full");
    }

    #[test]
    fn clamping_keeps_the_selection_in_the_rows_and_on_screen() {
        assert_eq!(clamp(0, 5, 3, 4), (0, 0));
        assert_eq!(clamp(10, 99, 0, 4), (9, 6));
        assert_eq!(clamp(10, 2, 5, 4), (2, 2));
        assert_eq!(clamp(10, 5, 2, 4), (5, 2));
        assert_eq!(clamp(10, 6, 2, 4), (6, 3));
    }

    #[test]
    fn it_draws_seeks_layout() {
        let rows = ["one", "two", "three"];
        let v = View {
            selected: 1,
            ..view()
        };
        let lines = render(&v, &rows, 12, 24, |r, w, sel| {
            assert_eq!(w, 12);
            Line::new(&format!("{r}{}", if sel { "*" } else { "" }), None)
        });
        assert_eq!(
            texts(&lines),
            [
                "─── Seek ───",
                "────────────",
                "            ",
                "────────────",
                "NAME",
                "────────────",
                "one         ",
                "two*        ",
                "three       ",
            ]
        );
        assert_eq!(lines[0].spans, [(0..lines[0].text.len(), "bold")]);
        assert_eq!(lines[1].spans, [(0..36, "shadow")]);
        assert!(lines[2].spans.is_empty(), "no face while not searching");
        assert_eq!(lines[4].spans, [(0..4, "bold")]);
        assert!(lines[6].spans.is_empty());
        assert_eq!(
            lines[7].spans,
            [(0..12, "highlight")],
            "padded, and only that"
        );
        // An odd title leaves the extra ─ on the right.
        let odd = View {
            title: " Odd! ",
            ..view()
        };
        let lines = render(&odd, &rows, 11, 24, |r, _, _| Line::new(r, None));
        assert_eq!(lines[0].text, "── Odd! ───");
    }

    #[test]
    fn searching_shades_the_whole_query_line_and_nothing_left_says_so() {
        let v = View {
            search: "tw",
            searching: true,
            ..view()
        };
        let none: [&str; 0] = [];
        let lines = render(&v, &none, 10, 24, |r, _, _| Line::new(r, None));
        assert_eq!(lines[2].text, "tw        ");
        assert_eq!(lines[2].spans, [(0..10, face::SEARCH)]);
        assert_eq!(
            lines.last().unwrap(),
            &Line::new("No results", Some("shadow"))
        );
        let keep = |q: &str, r: &&str| r.contains(q);
        assert_eq!(filtered(&["one", "two"], "o", keep), [&"one", &"two"]);
        assert_eq!(filtered(&["one", "two"], "tw", keep), [&"two"]);
        assert_eq!(filtered(&["one", "two"], "", |_, _| false).len(), 2);
    }

    #[test]
    fn header_lines_get_a_separator_of_their_own() {
        let header = vec!["a header".to_string()];
        let v = View {
            header_lines: &header,
            ..view()
        };
        let lines = render(&v, &["x"], 8, 24, |r, _, _| Line::new(r, None));
        assert_eq!(lines[2], Line::new("a header", Some("shadow")));
        assert_eq!(lines[3].text, "────────");
        assert_eq!(lines.len(), 9);
    }

    #[test]
    fn rows_scroll_and_lines_cut_with_their_faces() {
        let rows: Vec<String> = (0..10).map(|i| format!("row{i}")).collect();
        let v = View {
            selected: 7,
            scroll: 5,
            ..view()
        };
        let lines = render(&v, &rows, 6, 9, |r, _, _| Line::new(r, None));
        assert_eq!(texts(&lines[6..]), ["row5  ", "row6  ", "row7  "]);
        let mut line = Line::new("ab", Some("bold"));
        line.push("界界", Some("shadow"));
        let cut = line.clone().truncated(5);
        assert_eq!(cut.text, "ab界");
        assert_eq!(cut.spans, [(0..2, "bold"), (2..5, "shadow")]);
        assert_eq!(line.clone().truncated(9), line);
        let (text, spans) = join(&[Line::new("a", Some("bold")), Line::new("bc", None)]);
        assert_eq!(text, "a\nbc\n");
        assert_eq!(spans, [(0..1, "bold")]);
    }
}
