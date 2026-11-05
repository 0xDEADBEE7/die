//! A naive layout, to check K4's against. It's written from contract §9's text alone, the
//! paragraphs from "How text becomes rows" to "Motions respect hidden ranges", and it shares
//! nothing with `layout.rs`, so a bug in one won't hide in the other.
//!
//! It has no caches and speed doesn't matter. Each step below follows one of §9's paragraphs, in
//! their order, and quotes it where that helps. It covers what text becomes: rows and cursor
//! positions. It leaves out the queries built on them (`move_visual`, `screen_pos` and so on).
//!
//! Where §9 leaves a choice, a comment starting "Reading:" quotes the sentence and says which
//! reading this layout takes.
#![allow(dead_code)]

use std::ops::Range;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

// ---------------------------------------------------------------- input

/// Soft wrap's three settings.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Wrap {
    /// A display line is one row, however long.
    Off,
    /// Break at break points first.
    Word,
    /// The same rule with no break points.
    Char,
}

/// The gutter's styles.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LineNumbers {
    /// No gutter.
    Off,
    /// Each display line's first row shows its first line's number.
    Absolute,
    /// Distances in display lines from the cursor's display line.
    Relative,
    /// Distances in rows from the cursor's row.
    Visual,
}

/// A Hide decoration, as `decorate` is given it.
///
/// Reading: §9's table of decorations also offers "a placement (an image or pixel buffer)", but
/// "How text becomes rows" says nothing of a placement's cells, and `Deco::Hide` has no such field.
/// So a Hide here shows a replacement or nothing.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Hide {
    /// The first hidden byte.
    pub start: usize,
    /// The byte after the last hidden one.
    pub end: usize,
    /// Text drawn in its place.
    pub replacement: Option<String>,
}

/// A virtual text decoration, as `decorate` is given it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct VirtualText {
    /// The byte it shows before.
    pub pos: usize,
    /// What it shows.
    pub text: String,
}

/// Everything layout takes.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Input {
    /// The buffer's text.
    pub text: String,
    /// Cells per row, at least 2 ("Gutter and width"). `geometry` works it out from a window.
    pub text_w: usize,
    /// Cells per tab stop. 0 counts as 1.
    pub tab_width: usize,
    /// Soft wrap.
    pub wrap: Wrap,
    /// The gutter's style.
    pub numbers: LineNumbers,
    /// The cursor, a byte offset. Only Relative and Visual read it. If it isn't on a cursor
    /// position, it's moved the way "Cursor positions" says core moves it.
    pub cursor: usize,
    /// Hides, in the order they were added.
    pub hides: Vec<Hide>,
    /// Virtual texts, in the order they were added.
    pub virtual_texts: Vec<VirtualText>,
}

impl Input {
    /// `text` in rows `text_w` cells wide: word wrap, `tab_width` 8, no numbers, no decorations.
    pub fn new(text: &str, text_w: usize) -> Input {
        Input {
            text: text.to_string(),
            text_w,
            tab_width: 8,
            wrap: Wrap::Word,
            numbers: LineNumbers::Off,
            cursor: 0,
            hides: vec![],
            virtual_texts: vec![],
        }
    }
    /// With this tab width.
    pub fn tab_width(mut self, tab_width: usize) -> Input {
        self.tab_width = tab_width;
        self
    }
    /// With this wrap mode.
    pub fn wrap(mut self, wrap: Wrap) -> Input {
        self.wrap = wrap;
        self
    }
    /// With this gutter style and cursor.
    pub fn numbers(mut self, numbers: LineNumbers, cursor: usize) -> Input {
        self.numbers = numbers;
        self.cursor = cursor;
        self
    }
    /// With one more Hide.
    pub fn hide(mut self, range: Range<usize>, replacement: Option<&str>) -> Input {
        self.hides.push(Hide {
            start: range.start,
            end: range.end,
            replacement: replacement.map(str::to_string),
        });
        self
    }
    /// With one more virtual text.
    pub fn virtual_text(mut self, pos: usize, text: &str) -> Input {
        self.virtual_texts.push(VirtualText {
            pos,
            text: text.to_string(),
        });
        self
    }
}

// ---------------------------------------------------------------- output

/// One cell of a row.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Cell {
    /// An item's first cell: "It's drawn in its first cell". Holds the whole cluster.
    Cluster(String),
    /// A later cell of that item: "its other cells continue it".
    Continue,
    /// A cell of a tab in the text: "A tab's cells are blank".
    Tab,
    /// One character of a control character's drawing, such as the `^` or the `A` of `^A`.
    Control(char),
    /// "blank, with no position".
    Filler,
}

/// One row: its gutter and its text cells.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Row {
    /// The text cells, from `text_x`. With `Wrap::Word` or `Wrap::Char`, at most `text_w`.
    pub cells: Vec<Cell>,
    /// The number the gutter draws.
    pub number: Option<usize>,
    /// The gutter's cells: the number right-aligned, and a last blank cell. Empty with no gutter.
    pub gutter: String,
    /// Which display line this row belongs to, from 0.
    pub display_line: usize,
}

/// One cursor position: where it's drawn.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CursorPos {
    /// The byte offset.
    pub pos: usize,
    /// The row, counted from the first row of the text.
    pub row: usize,
    /// The column on that row.
    pub col: usize,
    /// The display column: "the cells of every item before it on the display line, whichever
    /// rows they're on".
    pub display_col: usize,
}

/// What layout returns.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Layout {
    /// Every row of the text, in order.
    pub rows: Vec<Row>,
    /// Every cursor position, in byte order.
    pub positions: Vec<CursorPos>,
    /// The gutter's width.
    pub gutter: usize,
}

impl Layout {
    /// Where a byte offset is drawn, or None if it isn't a cursor position.
    pub fn at(&self, pos: usize) -> Option<(usize, usize)> {
        self.positions
            .iter()
            .find(|p| p.pos == pos)
            .map(|p| (p.row, p.col))
    }

    /// Each row's number.
    pub fn numbers(&self) -> Vec<Option<usize>> {
        self.rows.iter().map(|r| r.number).collect()
    }

    /// The rows in the contract's notation: `|` ends a row, `·` is a space, `→` is a cell of a
    /// tab and `_` is filler. A cluster shows once, whatever its width, and a row with no cells
    /// shows as `(empty)`.
    pub fn render(&self) -> String {
        let mut rows = vec![];
        for row in &self.rows {
            let mut s = String::new();
            for cell in &row.cells {
                match cell {
                    Cell::Cluster(c) if c.starts_with(' ') => s.push('·'),
                    Cell::Cluster(c) => s.push_str(c),
                    Cell::Continue => {}
                    Cell::Tab => s.push('→'),
                    Cell::Control(c) => s.push(*c),
                    Cell::Filler => s.push('_'),
                }
            }
            rows.push(if row.cells.is_empty() {
                "(empty)".to_string()
            } else {
                s
            });
        }
        rows.join("|")
    }
}

// ---------------------------------------------------------------- "Hides merge first"

/// A hidden range: one or more Hides, merged.
struct Hidden {
    start: usize,
    end: usize,
    replacement: Option<String>,
}

/// "Replacements and virtual text hold no tabs: `decorate` turns their tabs and newlines into
/// spaces".
fn one_line(s: &str) -> String {
    s.replace(['\n', '\t'], " ")
}

fn hidden_ranges(hides: &[Hide]) -> Vec<Hidden> {
    // "A Hide with an empty range hides nothing and shows nothing." "an empty replacement is no
    // replacement".
    let mut live: Vec<Hide> = hides
        .iter()
        .filter(|h| h.start < h.end)
        .map(|h| Hide {
            start: h.start,
            end: h.end,
            replacement: h
                .replacement
                .as_deref()
                .filter(|r| !r.is_empty())
                .map(one_line),
        })
        .collect();
    // A stable sort, so Hides that start at one place stay in the order they were added.
    live.sort_by_key(|h| h.start);

    // "Hides that overlap merge into one hidden range." "Hides that only touch don't merge", so
    // the next Hide must start strictly before the group's end.
    let mut groups: Vec<Vec<Hide>> = vec![];
    for h in live {
        match groups.last_mut() {
            Some(g) if h.start < g.iter().map(|m| m.end).max().unwrap() => g.push(h),
            _ => groups.push(vec![h]),
        }
    }

    let mut hidden = vec![];
    for g in groups {
        // "It takes the replacement of the Hide that starts first; if several start there, it
        // prefers one with a replacement, then the longest, then the one added first."
        let mut best = &g[0];
        for h in &g {
            let better = (h.replacement.is_some(), h.end - h.start)
                > (best.replacement.is_some(), best.end - best.start);
            if h.start == best.start && better {
                best = h;
            }
        }
        hidden.push(Hidden {
            start: g[0].start,
            end: g.iter().map(|m| m.end).max().unwrap(),
            replacement: best.replacement.clone(),
        });
    }
    hidden
}

// ---------------------------------------------------------------- "Items"

/// One thing layout places on a row.
struct Item {
    cluster: String,
    /// The cursor position this item draws, if it draws one.
    pos: Option<usize>,
    /// The cells of every item before this one on the display line. Set by `set_widths`.
    display_col: usize,
    /// This item's cells. Set by `set_widths`.
    width: usize,
}

impl Item {
    fn new(cluster: &str, pos: Option<usize>) -> Item {
        Item {
            cluster: cluster.to_string(),
            pos,
            display_col: 0,
            width: 0,
        }
    }

    /// "*Whitespace* is an item whose cluster starts with U+0020 or U+0009".
    fn is_whitespace(&self) -> bool {
        self.cluster.starts_with([' ', '\t'])
    }

    /// A tab can only be from the text, since replacements and virtual text hold none.
    fn is_tab(&self) -> bool {
        self.cluster == "\t"
    }

    /// The characters a control character draws as, if this item is one. A control character is
    /// always a cluster of its own: Unicode's cluster rules break on both sides of one, except
    /// between `\r` and `\n`, and a run stops at a newline.
    fn control(&self) -> Option<String> {
        let first = self.cluster.chars().next().unwrap();
        (first.is_control() && !self.is_tab()).then(|| control_drawing(first))
    }

    /// "A tab or control character is split".
    fn splits(&self) -> bool {
        self.is_tab() || self.control().is_some()
    }

    /// This item's cells, all `width` of them.
    fn cells(&self) -> Vec<Cell> {
        if self.is_tab() {
            vec![Cell::Tab; self.width]
        } else if let Some(drawing) = self.control() {
            drawing.chars().map(Cell::Control).collect()
        } else {
            let mut cells = vec![Cell::Cluster(self.cluster.clone())];
            cells.resize(self.width, Cell::Continue);
            cells
        }
    }
}

/// "U+0000 to U+001F are `^` and the character U+0040 above them, `^@` to `^_`, two cells, so
/// `\r` is `^M`. U+007F is `^?`, two cells. U+0080 to U+009F are `\` and three octal digits,
/// `\200` to `\237`, four cells."
fn control_drawing(c: char) -> String {
    let code = c as u32;
    match code {
        0x00..=0x1F => format!("^{}", char::from_u32(code + 0x40).unwrap()),
        0x7F => "^?".to_string(),
        _ => format!("\\{code:03o}"),
    }
}

/// A display line, before it's cut into rows.
struct DisplayLine {
    /// Where it starts.
    start: usize,
    /// The line it starts on, from 0: "A display line's *first line* is the line it starts on."
    first_line: usize,
    items: Vec<Item>,
    /// The newline or end of the text it stops at.
    end: usize,
    /// The cells of all its items.
    total_width: usize,
}

/// Push the virtual texts at `pos`: "in the order the virtual texts were added. Clusters are
/// found in each virtual text on its own."
fn push_virtual_texts(input: &Input, pos: usize, items: &mut Vec<Item>) {
    for v in input.virtual_texts.iter().filter(|v| v.pos == pos) {
        for cluster in one_line(&v.text).graphemes(true) {
            items.push(Item::new(cluster, None));
        }
    }
}

/// Where the run of visible text starting at `pos` stops: "A run stops at a newline and at a
/// hidden range's start".
fn run_end(text: &str, hidden: &[Hidden], pos: usize) -> usize {
    let mut stop = text.len();
    if let Some(i) = text[pos..].find('\n') {
        stop = stop.min(pos + i);
    }
    for h in hidden {
        if h.start > pos {
            stop = stop.min(h.start);
        }
    }
    stop
}

/// Walk along the text and collect each display line's items.
///
/// The walk only ever stands on: a hidden range's start, a newline that isn't hidden, the end of
/// the text, or a cluster's start in a run. Those are the only places virtual text pushes its
/// items, which is how "strictly inside a hidden range or strictly inside a cluster, not at all"
/// holds.
fn display_lines(input: &Input, hidden: &[Hidden]) -> Vec<DisplayLine> {
    let text = input.text.as_str();
    let mut lines = vec![];
    let mut pos = 0;
    loop {
        let start = pos;
        let mut items = vec![];
        let end = loop {
            if let Some(h) = hidden.iter().find(|h| h.start == pos) {
                // "at a hidden range's start, before the replacement if there is one, and not at
                // all if there isn't". This takes in "at a hidden range's end ... unless another
                // hidden range starts there, when the start rule applies".
                if let Some(r) = &h.replacement {
                    push_virtual_texts(input, pos, &mut items);
                    // "the start of a hidden range with a replacement, drawn on the replacement's
                    // first cell". "a replacement's later clusters have none".
                    for (i, cluster) in r.graphemes(true).enumerate() {
                        items.push(Item::new(cluster, (i == 0).then_some(pos)));
                    }
                }
                pos = h.end;
            } else if pos == text.len() || text.as_bytes()[pos] == b'\n' {
                // "A newline is the byte U+000A". "before the cluster, newline or end of the text
                // there". A hidden newline never gets here: it's inside a range the walk jumps.
                push_virtual_texts(input, pos, &mut items);
                break pos;
            } else {
                // "Clusters are ... found separately in each run of visible text."
                let stop = run_end(text, hidden, pos);
                for (offset, cluster) in text[pos..stop].grapheme_indices(true) {
                    push_virtual_texts(input, pos + offset, &mut items);
                    items.push(Item::new(cluster, Some(pos + offset)));
                }
                pos = stop;
            }
        };
        let total_width = set_widths(&mut items, input.tab_width);
        lines.push(DisplayLine {
            start,
            first_line: text[..start].matches('\n').count(),
            items,
            end,
            total_width,
        });
        // A newline ends the display line, and the next starts after it. If that's the end of the
        // text, the next is the empty last row: "The position after it is laid out as a row of
        // its own". "An empty text has one empty row".
        if end == text.len() {
            return lines;
        }
        pos = end + 1;
    }
}

// ---------------------------------------------------------------- "Widths"

/// Give each item its width and display column, and return the display line's total. Nothing here
/// depends on rows, so "a display line's columns don't change with the width".
fn set_widths(items: &mut [Item], tab_width: usize) -> usize {
    // "where 0 counts as 1".
    let stop = tab_width.max(1);
    let mut col = 0;
    for item in items {
        item.display_col = col;
        item.width = if item.is_tab() {
            // "reaches the next multiple of `tab_width`".
            stop - col % stop
        } else if let Some(drawing) = item.control() {
            // "a cell per character".
            drawing.chars().count()
        } else {
            // "`unicode-width` 0.2's `width()` of the whole cluster, and at least one cell".
            item.cluster.width().max(1)
        };
        // Reading: "the cells of every item before it on the display line" against "Its cells
        // past the row's end aren't drawn". A cluster wider than a row counts in full here, drawn
        // or not, since "a display line's columns don't change with the width".
        col += item.width;
    }
    col
}

// ---------------------------------------------------------------- "Where soft wrap breaks"

/// Cut one display line into rows. Returns each row's cells, and the row and column of each
/// item's first cell, counted from this display line's first row.
fn place(items: &[Item], text_w: usize, wrap: Wrap) -> (Vec<Vec<Cell>>, Vec<(usize, usize)>) {
    let mut rows = vec![];
    let mut row: Vec<Cell> = vec![];
    let mut first_cell = vec![(0, 0); items.len()];
    // The current row's last break point: the item after it, and the row's length there.
    let mut last_break: Option<(usize, usize)> = None;
    let mut i = 0;
    while i < items.len() {
        let item = &items[i];

        // "A *break point* is the place before an item that isn't whitespace and directly follows
        // a whitespace item, unless that item is the first thing on its row."
        //
        // Reading: "that item" is the item after the place, the one that isn't whitespace. A
        // break before a row's first thing would make no progress, and the next sentence reads
        // this way: "The rest of a split tab or control character counts as being on the row, so
        // the item after it can be a break point." So a space that is itself first on its row
        // still makes a break point after it. §9's examples don't settle this: they all pass too
        // if "that item" is the whitespace item and it's first when it starts on this row.
        //
        // Reading: that next sentence names control characters too, but "directly follows a
        // whitespace item" still applies and a control character isn't whitespace. So the item
        // after a control character's rest isn't a break point. The examples don't settle this
        // either: they all pass if it is one.
        //
        // "Hidden text in between doesn't stop it: items are what count." `items` holds no hidden
        // text. "With `Wrap::Char` the same rule applies with no break points."
        if wrap == Wrap::Word
            && i > 0
            && items[i - 1].is_whitespace()
            && !item.is_whitespace()
            && !row.is_empty()
        {
            last_break = Some((i, row.len()));
        }

        // "An item *doesn't fit* if its last cell would be past the row's last cell."
        // "With `Wrap::Off` no item fails to fit".
        let fits = wrap == Wrap::Off || row.len() + item.width <= text_w;
        if fits {
            first_cell[i] = (rows.len(), row.len());
            row.extend(item.cells());
            i += 1;
        } else if let Some((after, len)) = last_break.take() {
            // Rule 1: "the row ends just before the last one. The whitespace before it stays at
            // this row's end. The next row starts with the item after the break point, and the
            // items after that are placed again."
            row.truncate(len);
            rows.push(std::mem::take(&mut row));
            i = after;
        } else if item.splits() {
            // Rule 2: "the cells that fit, which may be none, end this row, and the rest start the
            // next, over several rows if it's wider than a row."
            for (n, cell) in item.cells().into_iter().enumerate() {
                if row.len() == text_w {
                    rows.push(std::mem::take(&mut row));
                }
                if n == 0 {
                    first_cell[i] = (rows.len(), row.len());
                }
                row.push(cell);
            }
            i += 1;
        } else if row.is_empty() {
            // Rule 3: "A cluster wider than a whole row, once at a row's start, stays there ...
            // Its cells past the row's end aren't drawn. The row then counts as full". Cutting
            // the cells at `text_w` makes it full, so the next item doesn't fit, and a line end
            // lands at column `text_w`.
            first_cell[i] = (rows.len(), 0);
            row = item.cells();
            row.truncate(text_w);
            i += 1;
        } else {
            // Rule 2: "Any other item moves to the next row whole, and the cells it leaves on this
            // row are *filler*". The loop places it again on the new row.
            row.resize(text_w, Cell::Filler);
            rows.push(std::mem::take(&mut row));
        }
    }
    // The last row. A display line with no items gets its one empty row here. "take no cell and
    // never start a row": no line end adds a row.
    //
    // Reading: "The newline and the end of the text take no cell and never start a row" can't
    // cover the line ends alone on a row, since "The position after it is laid out as a row of
    // its own" and "An empty text has one empty row, and so does an empty line anywhere." It means
    // that a line end never wraps to a new row.
    rows.push(row);
    (rows, first_cell)
}

// ---------------------------------------------------------------- "Gutter and width"

/// A window's sizes, as "Gutter and width" names them.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Geometry {
    /// The gutter's cells.
    pub gutter: usize,
    /// `cols − gutter`, and at least 2.
    pub avail: usize,
    /// Cells per row.
    pub text_w: usize,
    /// Blank cells before the gutter.
    pub pad: usize,
    /// Where text starts: `pad + gutter`.
    pub text_x: usize,
}

/// "`n` is the number of lines, not counting an empty last line."
///
/// Reading: every line counts, hidden or not. Numbers name lines, and a fold's later lines are
/// still numbered after it.
pub fn number_of_lines(text: &str) -> usize {
    let newlines = text.matches('\n').count();
    if text.is_empty() || text.ends_with('\n') {
        newlines
    } else {
        newlines + 1
    }
}

/// "`gutter` is 0 with line numbers off. Otherwise it's `max(3, digits(n)) + 1`".
pub fn gutter_width(text: &str, numbers: LineNumbers) -> usize {
    if numbers == LineNumbers::Off {
        return 0;
    }
    number_of_lines(text).to_string().len().max(3) + 1
}

/// The sizes of a window `cols` cells wide.
pub fn geometry(
    text: &str,
    numbers: LineNumbers,
    cols: usize,
    max_width: Option<usize>,
    centered: bool,
) -> Geometry {
    let gutter = gutter_width(text, numbers);
    // "`avail` is `cols − gutter`, and at least 2."
    let avail = cols.saturating_sub(gutter).max(2);
    // "`text_w` is `min(max_width, avail)`, and at least 2. With no `max_width` it's `avail`."
    let text_w = max_width.map_or(avail, |m| m.min(avail)).max(2);
    // "`pad` is `(avail − text_w) / 2`, rounded down, when the buffer is centered, and otherwise
    // 0."
    let pad = if centered { (avail - text_w) / 2 } else { 0 };
    Geometry {
        gutter,
        avail,
        text_w,
        pad,
        text_x: pad + gutter,
    }
}

/// "The gutter draws a number right-aligned in its first `gutter − 1` cells, and its last cell is
/// blank."
///
/// Reading: §9 doesn't say what happens to a number with more digits than those cells. Visual is
/// the only style where one can arise: its distances count rows, and a gutter is sized by lines.
/// Right-aligned in a fixed space, its last digits show and the rest fall off the left, so the
/// gutter keeps its width and the text stays at `text_x`. `Row::number` keeps the whole number.
fn gutter_cells(number: Option<usize>, gutter: usize) -> String {
    if gutter == 0 {
        return String::new();
    }
    let width = gutter - 1;
    let digits = number.map_or(String::new(), |n| n.to_string());
    let shown = &digits[digits.len().saturating_sub(width)..];
    format!("{shown:>width$} ")
}

// ---------------------------------------------------------------- "Cursor positions"

/// Where core moves a cursor that isn't on a cursor position: "to the fold line if there is one,
/// otherwise to the next visible position. A cursor left inside a cluster goes to the next cursor
/// position."
///
/// Reading: "the fold line if there is one" means the hidden range the cursor is in has a
/// replacement. A fold line elsewhere on the display line doesn't count.
fn moved_cursor(cursor: usize, hidden: &[Hidden], positions: &[CursorPos]) -> usize {
    let inside = hidden.iter().find(|h| h.start <= cursor && cursor < h.end);
    if let Some(h) = inside
        && h.replacement.is_some()
    {
        return h.start;
    }
    // The end of the text is always a cursor position, and the last one.
    positions
        .iter()
        .find(|p| p.pos >= cursor)
        .unwrap_or(positions.last().unwrap())
        .pos
}

// ---------------------------------------------------------------- the whole layout

/// Lay out `input`.
pub fn layout(input: &Input) -> Layout {
    assert!(input.text_w >= 2, "§9: `text_w` is at least 2");
    let hidden = hidden_ranges(&input.hides);
    let lines = display_lines(input, &hidden);

    let mut rows: Vec<Row> = vec![];
    let mut positions = vec![];
    for (d, line) in lines.iter().enumerate() {
        let (line_rows, first_cell) = place(&line.items, input.text_w, input.wrap);
        let base = rows.len();
        for (item, (row, col)) in line.items.iter().zip(first_cell) {
            if let Some(pos) = item.pos {
                positions.push(CursorPos {
                    pos,
                    row: base + row,
                    col,
                    display_col: item.display_col,
                });
            }
        }
        // "Each has a cursor position at its row's next column." So on a full row, it's
        // `text_w`.
        let last = line_rows.len() - 1;
        positions.push(CursorPos {
            pos: line.end,
            row: base + last,
            col: line_rows[last].len(),
            display_col: line.total_width,
        });
        for cells in line_rows {
            rows.push(Row {
                cells,
                number: None,
                gutter: String::new(),
                display_line: d,
            });
        }
    }

    // ---- "Line numbers"
    let cursor = moved_cursor(input.cursor, &hidden, &positions);
    let at_cursor = positions.iter().find(|p| p.pos == cursor).unwrap();
    let cursor_row = at_cursor.row;
    let cursor_line = rows[cursor_row].display_line;
    let gutter = gutter_width(&input.text, input.numbers);
    for r in 0..rows.len() {
        let d = rows[r].display_line;
        let line = &lines[d];
        let is_first_row = r == 0 || rows[r - 1].display_line != d;
        let own_number = line.first_line + 1;
        let number = if line.start == input.text.len() {
            // "In every style, a display line that starts at the end of the text has no number".
            //
            // Reading: that means all of its rows. Virtual text there can make it wider than one
            // row, and Visual's "Every other row" doesn't bring a number back.
            //
            // Reading: this goes by where the display line starts, not by what shows. A hidden
            // last line's row looks like the empty last row, but it starts before the end of the
            // text, so it keeps its number.
            None
        } else {
            match input.numbers {
                LineNumbers::Off => None,
                LineNumbers::Absolute => is_first_row.then_some(own_number),
                LineNumbers::Relative if !is_first_row => None,
                LineNumbers::Relative if d == cursor_line => Some(own_number),
                LineNumbers::Relative => Some(d.abs_diff(cursor_line)),
                LineNumbers::Visual if r == cursor_row => Some(own_number),
                LineNumbers::Visual => Some(r.abs_diff(cursor_row)),
            }
        };
        rows[r].number = number;
        rows[r].gutter = gutter_cells(number, gutter);
    }

    Layout {
        rows,
        positions,
        gutter,
    }
}
