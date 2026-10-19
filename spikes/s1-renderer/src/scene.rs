//! The scene boundary: DIE-owned cells, a grid, cursor and one placement.
//! Nothing here touches the window or the GPU (scope §5.11).

use unicode_width::UnicodeWidthChar;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Underline { #[default] None, Single, Double, Curly, Dotted, Dashed }

pub mod attr {
    pub const BOLD: u8 = 1;
    pub const ITALIC: u8 = 2;
    pub const STRIKE: u8 = 4;
    pub const INVERSE: u8 = 8;
}

/// Width class of a cell: a wide character occupies its cell plus a continuation cell.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Wide { #[default] Narrow, Wide, Cont }

/// DIE's cell (D8): fg/bg, attributes, 5 underline styles with a color,
/// a link id and wide-character continuation. Colors are 0xRRGGBBAA.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Cell { pub ch: char, pub fg: u32, pub bg: u32, pub ul_color: u32, pub attrs: u8, pub ul: Underline, pub wide: Wide, pub link: u32 }

pub const FG: u32 = 0xd8d8d0ff;
pub const BG: u32 = 0x1c1c1cff;

impl Default for Cell {
    fn default() -> Self { Cell { ch: ' ', fg: FG, bg: BG, ul_color: 0, attrs: 0, ul: Underline::None, wide: Wide::Narrow, link: 0 } }
}

#[derive(Clone, Copy, Default)]
pub struct Style { pub fg: u32, pub bg: u32, pub attrs: u8, pub ul: Underline, pub ul_color: u32 }

impl Style {
    pub fn fg(fg: u32) -> Style { Style { fg, bg: BG, ..Default::default() } }
}

pub struct Grid { pub cols: usize, pub rows: usize, pub cells: Vec<Cell>, pub dirty: Vec<bool> }

impl Grid {
    pub fn new(cols: usize, rows: usize) -> Grid {
        Grid { cols, rows, cells: vec![Cell::default(); cols * rows], dirty: vec![true; rows] }
    }
    pub fn row(&self, r: usize) -> &[Cell] { &self.cells[r * self.cols..(r + 1) * self.cols] }
    pub fn clear_row(&mut self, r: usize, from: usize, to: usize, bg: u32) {
        for c in from..to.min(self.cols) { self.cells[r * self.cols + c] = Cell { bg, ..Cell::default() }; }
        self.dirty[r] = true;
    }
    /// Writes `s` at (r, c), wrapping nothing; returns the column after the text.
    pub fn put(&mut self, r: usize, mut c: usize, s: &str, st: Style, limit: usize) -> usize {
        let limit = limit.min(self.cols);
        for ch in s.chars() {
            let w = if ch == '\t' { 1 } else { ch.width().unwrap_or(0) };
            if w == 0 { continue; } // combining marks are out of scope for the spike
            if c + w > limit { break; }
            let ch = if ch == '\t' { ' ' } else { ch };
            let base = Cell { ch, fg: st.fg, bg: st.bg, ul_color: st.ul_color, attrs: st.attrs, ul: st.ul, wide: if w == 2 { Wide::Wide } else { Wide::Narrow }, link: 0 };
            self.cells[r * self.cols + c] = base;
            if w == 2 { self.cells[r * self.cols + c + 1] = Cell { ch: ' ', wide: Wide::Cont, ..base }; }
            c += w;
        }
        self.dirty[r] = true;
        c
    }
    pub fn mark_all(&mut self) { self.dirty.iter_mut().for_each(|d| *d = true); }
}

#[derive(Clone, Copy, PartialEq)]
pub enum CursorShape { Block, Bar, Underline }

/// A pixel placement anchored to a cell rectangle (here only the pixel buffer).
#[derive(Clone, Copy)]
pub struct Placement { pub col: usize, pub row: usize, pub cols: usize, pub rows: usize }

pub struct Scene { pub grid: Grid, pub cursor: Option<(usize, usize, CursorShape)>, pub placement: Option<Placement> }

const KEYWORDS: &[&str] = &["fn", "let", "pub", "use", "struct", "impl", "match", "if", "else", "for", "while", "return", "mut", "self", "Self", "enum", "const", "static", "unsafe", "mod", "crate", "where", "trait", "type", "loop", "as", "in", "ref", "move", "break", "continue", "true", "false", "Some", "None", "Ok", "Err", "super", "dyn"];

/// Very small Rust highlighter: enough colour variety for a realistic redraw.
pub fn highlight_line(line: &str) -> Vec<(String, Style)> {
    let mut out = vec![];
    let chars: Vec<char> = line.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let start = i;
        let st = if c == '/' && chars.get(i + 1) == Some(&'/') {
            i = chars.len();
            Style { fg: 0x7f8c7aff, bg: BG, attrs: attr::ITALIC, ..Default::default() }
        } else if c == '"' {
            i += 1;
            while i < chars.len() && chars[i] != '"' { if chars[i] == '\\' { i += 1; } i += 1; }
            i = (i + 1).min(chars.len());
            Style::fg(0xa8c97fff)
        } else if c.is_ascii_digit() {
            while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '_' || chars[i] == '.') { i += 1; }
            Style::fg(0xd7a65fff)
        } else if c.is_alphanumeric() || c == '_' {
            while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') { i += 1; }
            let w: String = chars[start..i].iter().collect();
            if KEYWORDS.contains(&w.as_str()) { Style { fg: 0xc792eaff, bg: BG, attrs: attr::BOLD, ..Default::default() } }
            else if w.chars().next().unwrap().is_uppercase() { Style::fg(0x82aaffff) }
            else if chars.get(i) == Some(&'(') { Style { fg: 0x89ddffff, bg: BG, ul: Underline::None, ..Default::default() } }
            else { Style::fg(FG) }
        } else {
            i += 1;
            Style::fg(if "{}()[]<>".contains(c) { 0xb0b0a8ff } else { 0x9aa5ceff })
        };
        out.push((chars[start..i].iter().collect(), st));
    }
    out
}

/// Fills rows [0, rows) and columns [0, cols) with `lines` starting at `top`.
/// `variant` perturbs styles so successive frames change every cell (check 2).
pub fn fill_file_view(g: &mut Grid, lines: &[String], top: usize, cols: usize, rows: usize, variant: u32) {
    for r in 0..rows {
        g.clear_row(r, 0, cols, BG);
        let Some(line) = lines.get(top + r) else { continue };
        let num = format!("{:>5} ", top + r + 1);
        let mut c = g.put(r, 0, &num, Style::fg(0x5c6370ff), cols);
        for (k, (tok, mut st)) in highlight_line(line).into_iter().enumerate() {
            if variant != 0 {
                st.fg = st.fg.rotate_left(8 * (variant % 3)) | 0xff;
                if k % 7 == 3 { st.ul = [Underline::Single, Underline::Double, Underline::Curly, Underline::Dotted, Underline::Dashed][(k / 7 + variant as usize) % 5]; st.ul_color = 0xff6c6bff; }
                if k % 11 == 5 { st.attrs |= attr::STRIKE; }
                // worst case for backgrounds: a different bg on every token
                st.bg = [0x202830ff, 0x302020ff, 0x203020ff, 0x282028ff][(k + variant as usize) % 4];
            }
            c = g.put(r, c, &tok, st, cols);
        }
        // pad the rest of the row so 'all cells changed' really means all
        if variant != 0 {
            // pad the rest of the row with alternating backgrounds so every cell differs from the last frame
            for (i, cc) in (c..cols).enumerate() { let ch = if variant % 2 == 0 { '·' } else { '-' }; g.put(r, cc, &ch.to_string(), Style { fg: 0x5a5a5aff, bg: if (i + variant as usize) % 2 == 0 { 0x242424ff } else { 0x2a2a2aff }, ..Default::default() }, cols); }
        }
    }
}
