//! CSV and TSV: quoted fields with `""` escapes and line breaks inside them, a delimiter sniffed
//! from the first line unless given, and a header row unless told otherwise. Each column's type
//! is inferred from all of its cells ([`super::infer`]).
//!
//! The dialect, for the cases RFC 4180 leaves open:
//!
//! - a leading byte order mark is skipped; lines end in `\n`, `\r\n` or a lone `\r`;
//! - the delimiter is the one of `,`, tab and `;` found most often outside quotes on the first
//!   line, `,` first on a tie and when there is none; a given one is any ASCII character but a
//!   quote or a line break;
//! - spaces and tabs around a field are dropped unless they are the delimiter, and a quoted
//!   field keeps everything between its quotes;
//! - a line with nothing on it is skipped; a row shorter than the header is missing its last
//!   cells; a longer one is an error at its first extra field; without a header the widest row
//!   sets the columns, as JSON's arrays do, and the rows before it are missing the extra cells;
//! - a header cell that's empty names its column `column N` (from 1), as every column is named
//!   without a header row, and such a column with nothing in it (a trailing delimiter's) is
//!   dropped; two columns of one name are an error at the second.
//!
//! Errors carry their place: line, column in chars and byte offset in `text`, a lone `\r`
//! ending a line there as it does a row.

use super::infer::infer;
use super::{ReadOptions, Table};
use crate::error::{FigError, Pos, Result};
use std::borrow::Cow;

/// A table from CSV or TSV text.
pub fn read_csv(text: &str, opt: &ReadOptions) -> Result<Table> {
    if let Some(d) = opt.delimiter
        && (!d.is_ascii() || matches!(d, b'"' | b'\n' | b'\r'))
    {
        // A byte inside a character would split it, and these three end fields of their own.
        return Err(FigError::new(
            "a delimiter is one ASCII character, not a quote or a line break",
        ));
    }
    let bom = if text.starts_with('\u{feff}') { 3 } else { 0 };
    let delim = opt.delimiter.unwrap_or_else(|| sniff(&text[bom..]));
    let mut p = Parser {
        text,
        bytes: text.as_bytes(),
        i: bom,
        delim,
    };
    let mut names: Option<Names> = None;
    let mut cols: Vec<Vec<Option<Cow<str>>>> = Vec::new();
    let mut rec: Vec<Field> = Vec::new();
    while p.record(&mut rec)? {
        if let [only] = &rec[..]
            && !only.quoted
            && only.text.is_empty()
        {
            continue;
        }
        let rec = std::mem::take(&mut rec);
        let Some(header) = names.as_mut() else {
            let (header, first) = start(text, rec, opt.header)?;
            cols = (0..header.len()).map(|_| Vec::new()).collect();
            names = Some(header);
            if let Some(first) = first {
                push(&mut cols, first);
            }
            continue;
        };
        if rec.len() > header.len() {
            if opt.header {
                let extra = &rec[header.len()];
                return Err(FigError::malformed(
                    format!(
                        "a row of {} fields where the header has {}",
                        rec.len(),
                        header.len()
                    ),
                    pos_at(text, extra.start),
                ));
            }
            // Without a header the widest row sets the columns.
            let rows = cols.first().map_or(0, Vec::len);
            for i in header.len()..rec.len() {
                header.push((format!("column {}", i + 1), true));
                cols.push(vec![None; rows]);
            }
        }
        push(&mut cols, rec);
    }
    let names = names.unwrap_or_default();
    let columns = names
        .iter()
        .zip(&cols)
        .filter(|((_, unnamed), cells)| {
            // A trailing delimiter's empty column, which spreadsheets write.
            !(*unnamed && cells.iter().flatten().all(|c| c.is_empty()))
        })
        .map(|((name, _), cells)| {
            let cells: Vec<Option<&str>> = cells.iter().map(|c| c.as_deref()).collect();
            infer(name, &cells, opt)
        })
        .collect();
    Table::new(columns)
}

/// Where the cell of column `name` in row `row` (from 0, the header not counted) is in `text`,
/// read as [`read_csv`] reads it, and how many chars wide it is: for an error that names a cell,
/// so the error figure's caret can sit under it. None where there's no such cell.
pub fn cell_at(text: &str, opt: &ReadOptions, row: usize, name: &str) -> Option<(Pos, usize)> {
    if opt
        .delimiter
        .is_some_and(|d| !d.is_ascii() || matches!(d, b'"' | b'\n' | b'\r'))
    {
        return None;
    }
    let bom = if text.starts_with('\u{feff}') { 3 } else { 0 };
    let delim = opt.delimiter.unwrap_or_else(|| sniff(&text[bom..]));
    let mut p = Parser {
        text,
        bytes: text.as_bytes(),
        i: bom,
        delim,
    };
    let unnamed = |i: usize| format!("column {}", i + 1);
    let mut index: Option<usize> = None;
    let mut seen = 0usize;
    let mut rec: Vec<Field> = Vec::new();
    while p.record(&mut rec).ok()? {
        if let [only] = &rec[..]
            && !only.quoted
            && only.text.is_empty()
        {
            continue;
        }
        let i = match index {
            Some(i) => i,
            None => {
                let named = |(i, f): (usize, &Field)| {
                    f.text == name || (f.text.is_empty() && unnamed(i) == name)
                };
                let i = match opt.header {
                    true => rec.iter().enumerate().position(named)?,
                    false => (0..rec.len().max(64)).find(|i| unnamed(*i) == name)?,
                };
                index = Some(i);
                if opt.header {
                    continue;
                }
                i
            }
        };
        if seen == row {
            let f = rec.get(i)?;
            let width = if f.quoted {
                f.text.chars().count() + 2
            } else {
                f.text.chars().count()
            };
            return Some((pos_at(text, f.start), width.max(1)));
        }
        seen += 1;
    }
    None
}

/// The columns' names, each with whether it was made up rather than read from a header.
type Names = Vec<(String, bool)>;

/// One field and where it starts.
struct Field<'a> {
    text: Cow<'a, str>,
    quoted: bool,
    start: usize,
}

/// The first record: the header's names and no row, or, without a header, `column N` names
/// and the record as the first row.
/// Each name says whether it was made up (no header, or an empty cell in it).
fn start<'a>(
    text: &str,
    rec: Vec<Field<'a>>,
    header: bool,
) -> Result<(Names, Option<Vec<Field<'a>>>)> {
    if !header {
        let names = (1..=rec.len())
            .map(|i| (format!("column {i}"), true))
            .collect();
        return Ok((names, Some(rec)));
    }
    let mut names: Names = Vec::with_capacity(rec.len());
    for (i, f) in rec.iter().enumerate() {
        let name = match &*f.text {
            "" => (format!("column {}", i + 1), true),
            s => (s.to_string(), false),
        };
        if names.iter().any(|n| n.0 == name.0) {
            return Err(FigError::malformed(
                format!("two columns are named \"{}\"", name.0),
                pos_at(text, f.start),
            ));
        }
        names.push(name);
    }
    Ok((names, None))
}

/// A row into the columns, short rows missing their last cells.
fn push<'a>(cols: &mut [Vec<Option<Cow<'a, str>>>], rec: Vec<Field<'a>>) {
    let n = rec.len();
    for (col, f) in cols.iter_mut().zip(rec) {
        col.push(Some(f.text));
    }
    for col in cols.iter_mut().skip(n) {
        col.push(None);
    }
}

/// The delimiter the first line uses most outside quotes: `,`, tab or `;`, `,` on a tie.
fn sniff(text: &str) -> u8 {
    let mut counts = [(b',', 0usize), (b'\t', 0), (b';', 0)];
    let mut quoted = false;
    let first = text.trim_start_matches(['\n', '\r']);
    for &b in first.as_bytes() {
        match b {
            b'"' => quoted = !quoted,
            b'\n' | b'\r' if !quoted => break,
            _ if !quoted => {
                if let Some(c) = counts.iter_mut().find(|(d, _)| *d == b) {
                    c.1 += 1;
                }
            }
            _ => {}
        }
    }
    let best = counts.iter().map(|c| c.1).max().unwrap_or(0);
    counts
        .iter()
        .find(|c| c.1 == best && best > 0)
        .map_or(b',', |c| c.0)
}

/// Where byte `at` of `text` is: its line, its column in chars (a byte order mark not counted)
/// and the byte. Lines end as rows do: at `\n`, `\r\n` or a lone `\r`.
pub(crate) fn pos_at(text: &str, at: usize) -> Pos {
    let bytes = text.as_bytes();
    let before = &text[..at.min(text.len())];
    let (mut line, mut line_start) = (1usize, 0usize);
    for (i, &b) in before.as_bytes().iter().enumerate() {
        if b == b'\n' || (b == b'\r' && bytes.get(i + 1) != Some(&b'\n')) {
            line += 1;
            line_start = i + 1;
        }
    }
    let mut col = before[line_start..].chars().count() + 1;
    if line_start == 0 && before.starts_with('\u{feff}') {
        col -= 1;
    }
    Pos {
        line: u32::try_from(line).unwrap_or(u32::MAX),
        col: u32::try_from(col).unwrap_or(u32::MAX),
        byte: u32::try_from(at).unwrap_or(u32::MAX),
    }
}

/// A place in the text being read.
struct Parser<'a> {
    text: &'a str,
    bytes: &'a [u8],
    i: usize,
    delim: u8,
}

/// How a field ended.
enum End {
    Delim,
    Line,
}

impl<'a> Parser<'a> {
    /// The next record's fields into `rec`; false at the end of the text.
    fn record(&mut self, rec: &mut Vec<Field<'a>>) -> Result<bool> {
        rec.clear();
        if self.i >= self.bytes.len() {
            return Ok(false);
        }
        loop {
            let (field, end) = self.field()?;
            rec.push(field);
            if let End::Line = end {
                return Ok(true);
            }
        }
    }

    /// Whether a byte pads a field: a space or a tab that isn't the delimiter.
    fn blank(&self, b: u8) -> bool {
        (b == b' ' || b == b'\t') && b != self.delim
    }

    fn skip_blanks(&mut self) {
        while let Some(&b) = self.bytes.get(self.i) {
            if !self.blank(b) {
                break;
            }
            self.i += 1;
        }
    }

    /// The end of a field: a delimiter, a line's end or the text's.
    fn end(&mut self) -> Option<End> {
        match self.bytes.get(self.i) {
            None => Some(End::Line),
            Some(&b) if b == self.delim => {
                self.i += 1;
                Some(End::Delim)
            }
            Some(b'\n') => {
                self.i += 1;
                Some(End::Line)
            }
            Some(b'\r') => {
                self.i += 1;
                if self.bytes.get(self.i) == Some(&b'\n') {
                    self.i += 1;
                }
                Some(End::Line)
            }
            Some(_) => None,
        }
    }

    fn field(&mut self) -> Result<(Field<'a>, End)> {
        self.skip_blanks();
        let start = self.i;
        if self.bytes.get(self.i) == Some(&b'"') {
            let text = self.quoted(start)?;
            self.skip_blanks();
            let Some(end) = self.end() else {
                return Err(FigError::malformed(
                    "expected a delimiter or the line's end after a closing quote",
                    pos_at(self.text, self.i),
                ));
            };
            let field = Field {
                text,
                quoted: true,
                start,
            };
            return Ok((field, end));
        }
        while let Some(&b) = self.bytes.get(self.i) {
            if b == self.delim || b == b'\n' || b == b'\r' {
                break;
            }
            self.i += 1;
        }
        let raw = &self.text[start..self.i];
        let text = raw.trim_end_matches(|c: char| c.is_ascii() && self.blank(c as u8));
        let end = self.end().unwrap_or(End::Line);
        let field = Field {
            text: Cow::Borrowed(text),
            quoted: false,
            start,
        };
        Ok((field, end))
    }

    /// A quoted field from its opening quote at `start`, `""` read as one quote.
    fn quoted(&mut self, start: usize) -> Result<Cow<'a, str>> {
        self.i += 1;
        let mut seg = self.i;
        let mut owned: Option<String> = None;
        loop {
            let Some(off) = self.bytes[self.i..].iter().position(|&b| b == b'"') else {
                return Err(FigError::malformed(
                    "unterminated quoted field",
                    pos_at(self.text, start),
                ));
            };
            let q = self.i + off;
            if self.bytes.get(q + 1) == Some(&b'"') {
                owned
                    .get_or_insert_with(String::new)
                    .push_str(&self.text[seg..=q]);
                self.i = q + 2;
                seg = self.i;
                continue;
            }
            self.i = q + 1;
            let last = &self.text[seg..q];
            return Ok(match owned {
                Some(mut s) => {
                    s.push_str(last);
                    Cow::Owned(s)
                }
                None => Cow::Borrowed(last),
            });
        }
    }
}
