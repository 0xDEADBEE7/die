//! A stream's table, grown a line at a time as the command prints (contract-p4 §4): each line
//! is a row, JSON lines (an object a line, or arrays under a first line of names) or CSV under
//! its first line, the header. The format is the spec's, else the first line's: JSON when it
//! opens with `{` or `[`, CSV otherwise, its delimiter sniffed from that line as the CSV reader
//! sniffs a file's.
//!
//! Each column's type is inferred as [`crate::data::infer`] infers a file's (Number if every
//! cell that isn't missing is a number, then Time, then Category up to the limit, then Text),
//! but as the rows come rather than over them all once: a column keeps what each type it may
//! still be needs, and drops a type for good at the first cell that isn't one. So a fold
//! ([`Grow::table`]) copies columns and parses nothing, however many rows there are.
//!
//! - The newest `max` rows are kept; older ones drop from the front ([`Grow::dropped`]). A
//!   type a dropped row ruled out stays ruled out.
//! - A line that can't be read as a row (bad JSON, more fields than the header, a quote left
//!   open, a line past [`MAX_LINE`]) is skipped and counted, the first kept to say why
//!   ([`Grow::bad`]); only a header that can't be one stops the stream ([`Grow::line`]'s
//!   error).
//! - A column that turns out not to be numbers or times after all writes its earlier cells as
//!   their values (`1.5`, `2026-11-22T10:00:00Z`), as JSON numbers are written, not as the
//!   command spelt them: what a column keeps of its text is only what its type needs.
//! - A row's `fig` block is not a column, and the rows keep neither it nor their JSON objects
//!   (a file's do, for the detail panel): a stream's rows are copied at every fold.
//! - Started again ([`Grow::restart`], `R` on a stream that ended), the rows stay and the new
//!   process's first line is its header again; columns are matched by name.

use crate::data::csv::{fields, sniff_line};
use crate::data::infer::{is_missing, parse_number};
use crate::data::time::parse_time;
use crate::data::{Column, ReadOptions, Table, Values};
use crate::error::{ErrorKind, FigError, Result};
use crate::json::{self, Kind, Node};
use crate::spec::DataFormat;
use std::borrow::Cow;
use std::collections::BTreeMap;
use std::sync::Arc;

/// The longest line a stream may print as a row: a longer one is skipped as unreadable.
pub const MAX_LINE: usize = 1024 * 1024;

/// A stream's rows so far, as columns that grow.
pub struct Grow {
    opt: ReadOptions,
    /// The spec's format, if it gave one.
    format: Option<DataFormat>,
    /// How lines are read, once the first one has said.
    shape: Shape,
    /// Whether a CSV stream's first line is its header.
    header: bool,
    cols: Vec<Col>,
    index: BTreeMap<Box<str>, usize>,
    /// Cells each column holds, the dropped rows not yet let go of among them.
    len: usize,
    /// Rows at the front dropped but not yet let go of.
    start: usize,
    max: usize,
    dropped: usize,
    /// Lines read, for the place of a bad one.
    line_no: usize,
    bad: usize,
    first_bad: Option<FigError>,
    /// A header that couldn't be one: no more is read.
    failed: Option<FigError>,
    /// Rows came since the last fold.
    fresh: bool,
    /// What the command is called, for what a bad line says.
    name: String,
}

/// How a stream's lines are read.
enum Shape {
    /// No line has said yet.
    Unknown,
    /// CSV, the columns of each field in the header (None until the header comes).
    Csv {
        delim: u8,
        names: Option<Vec<usize>>,
    },
    /// JSON lines: objects, or arrays under a first line of names.
    Json { names: Option<Vec<usize>> },
}

impl Grow {
    /// An empty stream, read with `opt` (the spec's hints already applied), as `format` says
    /// if given; the newest `max` rows kept. `name` is the command, for what a bad line says.
    pub fn new(opt: ReadOptions, format: Option<DataFormat>, max: usize, name: &str) -> Grow {
        let header = opt.header;
        Grow {
            opt,
            format,
            shape: Shape::Unknown,
            header,
            cols: vec![],
            index: BTreeMap::new(),
            len: 0,
            start: 0,
            max: max.max(1),
            dropped: 0,
            line_no: 0,
            bad: 0,
            first_bad: None,
            failed: None,
            fresh: false,
            name: name.to_string(),
        }
    }

    /// One line the command printed, without its line break. A row goes into the columns; a
    /// line that isn't one is counted and skipped. An error only for a header that can't be
    /// one, after which the stream can't be read and nothing more is.
    pub fn line(&mut self, line: &str) -> Result<()> {
        if let Some(e) = &self.failed {
            return Err(e.clone());
        }
        self.line_no += 1;
        let line = line.strip_suffix('\r').unwrap_or(line);
        let line = match self.line_no {
            1 => line.trim_start_matches('\u{feff}'),
            _ => line,
        };
        if line.trim().is_empty() {
            return Ok(());
        }
        if line.len() > MAX_LINE {
            let mb = line.len() / (1024 * 1024);
            self.skip(format!("a line of {mb} MB is too long to be a row"));
            return Ok(());
        }
        if let Shape::Unknown = self.shape {
            self.shape = self.shape_of(line)?;
        }
        match &self.shape {
            Shape::Csv { delim, names } => {
                let (delim, names) = (*delim, names.clone());
                self.csv(line, delim, names)
            }
            Shape::Json { names } => {
                let names = names.clone();
                self.json(line, names);
                Ok(())
            }
            Shape::Unknown => Ok(()),
        }
    }

    /// A line longer than [`MAX_LINE`], whose bytes weren't kept: counted and skipped as no
    /// row. An error only once the stream can't be read.
    pub fn too_long(&mut self) -> Result<()> {
        if let Some(e) = &self.failed {
            return Err(e.clone());
        }
        self.line_no += 1;
        self.skip(format!(
            "a line longer than {} MB is too long to be a row",
            MAX_LINE / (1024 * 1024)
        ));
        Ok(())
    }

    /// The command started again: the rows stay, and its first line is a header again.
    pub fn restart(&mut self) {
        match &mut self.shape {
            Shape::Csv { names, .. } if self.header => *names = None,
            Shape::Json { names } => *names = None,
            _ => {}
        }
        self.failed = None;
    }

    /// The rows kept.
    pub fn rows(&self) -> usize {
        self.len - self.start
    }

    /// How many rows dropped from the front, in all.
    pub fn dropped(&self) -> usize {
        self.dropped
    }

    /// How many lines were skipped as no row, and why the first was.
    pub fn bad(&self) -> (usize, Option<&FigError>) {
        (self.bad, self.first_bad.as_ref())
    }

    /// Why the stream can't be read, if it can't: its header couldn't be one.
    pub fn failed(&self) -> Option<&FigError> {
        self.failed.as_ref()
    }

    /// Whether rows came since the last fold.
    pub fn fresh(&self) -> bool {
        self.fresh
    }

    /// The rows kept, as a table: columns in the order first met, each its type so far.
    pub fn table(&mut self) -> Table {
        self.fresh = false;
        let from = self.start;
        let columns: Vec<Column> = self.cols.iter().map(|c| c.column(from)).collect();
        Table {
            columns,
            rows: self.rows(),
            styles: None,
            raw: None,
        }
    }

    /// How lines are read: the spec's format, else the first line's.
    fn shape_of(&mut self, first: &str) -> Result<Shape> {
        let json = match self.format {
            Some(DataFormat::Json | DataFormat::Jsonl) => true,
            Some(DataFormat::Csv | DataFormat::Tsv) => false,
            None => first.trim_start().starts_with(['{', '[']),
        };
        if json {
            return Ok(Shape::Json { names: None });
        }
        let delim = match (self.opt.delimiter, self.format) {
            (Some(d), _) => d,
            (None, Some(DataFormat::Tsv)) => b'\t',
            (None, _) => sniff_line(first),
        };
        if !delim.is_ascii() || matches!(delim, b'"' | b'\n' | b'\r') {
            let e =
                FigError::new("a delimiter is one ASCII character, not a quote or a line break");
            self.failed = Some(e.clone());
            return Err(e);
        }
        Ok(Shape::Csv { delim, names: None })
    }

    fn csv(&mut self, line: &str, delim: u8, names: Option<Vec<usize>>) -> Result<()> {
        let cells = match fields(line, delim) {
            Ok(f) => f,
            Err(e) => {
                let at =
                    e.at.map_or_else(String::new, |p| format!("column {}: ", p.col));
                self.skip(format!("{at}{}", e.msg));
                return Ok(());
            }
        };
        let Some(names) = names else {
            if self.header {
                return self.csv_header(&cells, delim);
            }
            // No header: `column N`, the widest row setting them.
            let names: Vec<usize> = (0..cells.len())
                .map(|i| self.col(&format!("column {}", i + 1)))
                .collect();
            self.shape = Shape::Csv {
                delim,
                names: Some(names.clone()),
            };
            self.row(names.iter().copied().zip(cells.into_iter().map(Raw::Str)));
            return Ok(());
        };
        let mut names = names;
        if cells.len() > names.len() {
            if self.header {
                self.skip(format!(
                    "a row of {} fields where the header has {}",
                    cells.len(),
                    names.len()
                ));
                return Ok(());
            }
            for i in names.len()..cells.len() {
                names.push(self.col(&format!("column {}", i + 1)));
            }
            self.shape = Shape::Csv {
                delim,
                names: Some(names.clone()),
            };
        }
        self.row(names.iter().copied().zip(cells.into_iter().map(Raw::Str)));
        Ok(())
    }

    /// A CSV header: its names, an empty one `column N`; two of one name can't be.
    fn csv_header(&mut self, cells: &[Cow<'_, str>], delim: u8) -> Result<()> {
        let mut seen: Vec<String> = Vec::with_capacity(cells.len());
        for (i, c) in cells.iter().enumerate() {
            let name = match &**c {
                "" => format!("column {}", i + 1),
                s => s.to_string(),
            };
            if seen.contains(&name) {
                let msg = format!(
                    "{} printed a header with two columns named \"{name}\"",
                    self.name
                );
                let e = FigError::new(msg).with_kind(ErrorKind::Malformed);
                self.failed = Some(e.clone());
                return Err(e);
            }
            seen.push(name);
        }
        let names = seen.iter().map(|n| self.col(n)).collect();
        self.shape = Shape::Csv {
            delim,
            names: Some(names),
        };
        Ok(())
    }

    fn json(&mut self, line: &str, names: Option<Vec<usize>>) {
        let node = match json::parse(line) {
            Ok(n) => n,
            Err(e) => {
                let at =
                    e.at.map_or_else(String::new, |p| format!("column {}: ", p.col));
                self.skip(format!("{at}{}", e.msg));
                return;
            }
        };
        match &node.kind {
            Kind::Obj(fields) => {
                let cells: Vec<(usize, Raw)> = fields
                    .iter()
                    .filter(|(k, _)| &*k.name != "fig")
                    .map(|(k, v)| (self.col(&k.name), raw(v)))
                    .collect();
                self.row(cells.into_iter());
            }
            Kind::Arr(items) => match names {
                None => {
                    let names: Vec<usize> = (items.iter().enumerate())
                        .map(|(i, n)| match &n.kind {
                            Kind::Str(s) if !s.is_empty() => self.col(s),
                            Kind::Num(v) => self.col(&format!("{v}")),
                            _ => self.col(&format!("column {}", i + 1)),
                        })
                        .collect();
                    self.shape = Shape::Json { names: Some(names) };
                }
                Some(names) if items.len() > names.len() => self.skip(format!(
                    "a row of {} values where the names are {}",
                    items.len(),
                    names.len()
                )),
                Some(names) => {
                    let cells: Vec<(usize, Raw)> =
                        names.iter().copied().zip(items.iter().map(raw)).collect();
                    self.row(cells.into_iter());
                }
            },
            _ => self.skip(format!(
                "expected a row, an object or an array, not {}",
                node.type_name()
            )),
        }
    }

    /// The column of this name, made with every row so far missing if it's new.
    fn col(&mut self, name: &str) -> usize {
        if let Some(&i) = self.index.get(name) {
            return i;
        }
        let i = self.cols.len();
        self.cols.push(Col::new(name, self.len));
        self.index.insert(name.into(), i);
        i
    }

    /// One row: each column its cell, the rest missing; the oldest dropped past `max`.
    fn row<'a>(&mut self, cells: impl Iterator<Item = (usize, Raw<'a>)>) {
        let mut row: Vec<Option<Raw<'a>>> = (0..self.cols.len()).map(|_| None).collect();
        for (i, c) in cells {
            row[i] = Some(c);
        }
        for (col, c) in self.cols.iter_mut().zip(row) {
            col.push(&c.unwrap_or(Raw::Missing), &self.opt);
        }
        self.len += 1;
        self.fresh = true;
        if self.rows() > self.max {
            self.drop_front(self.rows() - self.max);
        }
    }

    /// The oldest `n` rows let go of; the cells themselves once enough have gone.
    fn drop_front(&mut self, n: usize) {
        for c in &mut self.cols {
            c.forget(self.start, n);
        }
        self.start += n;
        self.dropped += n;
        if self.start >= (self.max / 2).max(4096) {
            for c in &mut self.cols {
                c.compact(self.start);
            }
            self.len -= self.start;
            self.start = 0;
        }
    }

    /// A line that's no row: counted, the first kept.
    fn skip(&mut self, why: String) {
        self.bad += 1;
        if self.first_bad.is_none() {
            let msg = format!("line {} of {}'s output: {why}", self.line_no, self.name);
            self.first_bad = Some(FigError::new(msg));
        }
    }
}

/// A cell as a line gives it: missing, a JSON number, or text to read.
enum Raw<'a> {
    Missing,
    Num(f64),
    Str(Cow<'a, str>),
}

/// A JSON value's cell, as the JSON reader takes one: null missing, booleans and nested values
/// as text.
fn raw(n: &Node) -> Raw<'_> {
    match &n.kind {
        Kind::Null => Raw::Missing,
        Kind::Num(v) => Raw::Num(*v),
        Kind::Str(s) => Raw::Str(Cow::Borrowed(s)),
        Kind::Bool(b) => Raw::Str(Cow::Borrowed(if *b { "true" } else { "false" })),
        Kind::Arr(_) | Kind::Obj(_) => Raw::Str(Cow::Owned(n.to_text())),
    }
}

impl Raw<'_> {
    fn missing(&self) -> bool {
        match self {
            Raw::Missing => true,
            Raw::Num(v) => v.is_nan(),
            Raw::Str(s) => is_missing(s),
        }
    }

    /// Its text, as a category or text column holds it; None if missing.
    fn text(&self) -> Option<Cow<'_, str>> {
        match self {
            Raw::Missing => None,
            Raw::Num(v) if v.is_nan() => None,
            Raw::Num(v) => Some(Cow::Owned(format!("{v}"))),
            Raw::Str(s) if is_missing(s) => None,
            Raw::Str(s) => Some(Cow::Borrowed(s)),
        }
    }
}

/// One column as it grows: each type it may still be, with what that type needs of it.
struct Col {
    name: Box<str>,
    /// While every cell that isn't missing is a number: each one, NaN where missing.
    num: Option<Vec<f64>>,
    /// While every one is a time: each one.
    time: Option<Vec<f64>>,
    /// While there are no more distinct strings than the limit.
    cat: Option<Cat>,
    /// Once it's none of those: each cell's text.
    text: Option<Vec<Box<str>>>,
}

/// A category column as it grows: the strings in first-seen order and each row's code.
#[derive(Default)]
struct Cat {
    dict: Vec<Box<str>>,
    index: BTreeMap<Box<str>, u32>,
    codes: Vec<u32>,
    /// How many rows kept have each code: a code no row has is left out of a fold.
    uses: Vec<usize>,
}

impl Cat {
    fn push(&mut self, t: Option<&str>) {
        let code = match t {
            None => u32::MAX,
            Some(s) => match self.index.get(s) {
                Some(&c) => c,
                None => {
                    let c = self.dict.len() as u32;
                    self.dict.push(s.into());
                    self.index.insert(s.into(), c);
                    self.uses.push(0);
                    c
                }
            },
        };
        if let Some(u) = self.uses.get_mut(code as usize) {
            *u += 1;
        }
        self.codes.push(code);
    }
}

impl Col {
    /// A new column, `rows` missing cells before its first.
    fn new(name: &str, rows: usize) -> Col {
        Col {
            name: name.into(),
            num: Some(vec![f64::NAN; rows]),
            time: Some(vec![f64::NAN; rows]),
            cat: Some(Cat {
                codes: vec![u32::MAX; rows],
                ..Cat::default()
            }),
            text: None,
        }
    }

    /// One more cell: each type it may still be takes it, or is ruled out.
    fn push(&mut self, c: &Raw<'_>, opt: &ReadOptions) {
        let missing = c.missing();
        let num = self.num.as_ref().and_then(|_| match c {
            _ if missing => Some(f64::NAN),
            Raw::Num(v) => Some(*v),
            Raw::Str(s) => parse_number(s, opt.thousands),
            Raw::Missing => Some(f64::NAN),
        });
        let time = self.time.as_ref().and_then(|_| match c {
            _ if missing => Some(f64::NAN),
            Raw::Num(_) => None,
            Raw::Str(s) => parse_time(s, opt),
            Raw::Missing => Some(f64::NAN),
        });
        let text = c.text();
        let cat_out = match (&self.cat, text.as_deref()) {
            (Some(cat), Some(t)) => {
                !cat.index.contains_key(t) && cat.dict.len() >= opt.category_limit
            }
            _ => false,
        };
        let none_left = self.text.is_none()
            && (self.num.is_none() || num.is_none())
            && (self.time.is_none() || time.is_none())
            && (self.cat.is_none() || cat_out);
        if none_left {
            // It's text from here on: the rows so far as text, from the type that held them.
            self.text = Some(self.texts());
        }
        match (num, &mut self.num) {
            (Some(v), Some(n)) => n.push(v),
            _ => self.num = None,
        }
        match (time, &mut self.time) {
            (Some(v), Some(t)) => t.push(v),
            _ => self.time = None,
        }
        if cat_out {
            self.cat = None;
        } else if let Some(cat) = &mut self.cat {
            cat.push(text.as_deref());
        }
        if let Some(t) = &mut self.text {
            t.push(text.as_deref().unwrap_or("").into());
        }
    }

    /// Every cell so far as text, from the type that still holds them: a category's own
    /// strings, else the numbers or the times as their values are written.
    fn texts(&self) -> Vec<Box<str>> {
        if let Some(cat) = &self.cat {
            return (cat.codes.iter())
                .map(|&c| cat.dict.get(c as usize).cloned().unwrap_or_default())
                .collect();
        }
        if let Some(n) = &self.num {
            return n.iter().map(|v| number_text(*v)).collect();
        }
        if let Some(t) = &self.time {
            return t.iter().map(|v| time_text(*v)).collect();
        }
        vec![]
    }

    /// The `n` rows from `start` are dropped: a category's codes stop counting them.
    fn forget(&mut self, start: usize, n: usize) {
        if let Some(cat) = &mut self.cat {
            for &c in &cat.codes[start..(start + n).min(cat.codes.len())] {
                if let Some(u) = cat.uses.get_mut(c as usize) {
                    *u = u.saturating_sub(1);
                }
            }
        }
    }

    /// The first `n` cells let go of.
    fn compact(&mut self, n: usize) {
        if let Some(v) = &mut self.num {
            v.drain(..n);
        }
        if let Some(v) = &mut self.time {
            v.drain(..n);
        }
        if let Some(cat) = &mut self.cat {
            cat.codes.drain(..n);
        }
        if let Some(v) = &mut self.text {
            v.drain(..n);
        }
    }

    /// The column from row `from` on, as the type it is so far.
    fn column(&self, from: usize) -> Column {
        let values = if let Some(n) = &self.num {
            Values::Number(Arc::from(&n[from..]))
        } else if let Some(t) = &self.time {
            Values::Time(Arc::from(&t[from..]))
        } else if let Some(cat) = &self.cat {
            // Only the strings the rows kept have, in first-seen order.
            let mut map = vec![u32::MAX; cat.dict.len()];
            let mut dict: Vec<Box<str>> = Vec::new();
            for (i, s) in cat.dict.iter().enumerate() {
                if cat.uses[i] > 0 {
                    map[i] = dict.len() as u32;
                    dict.push(s.clone());
                }
            }
            let codes: Vec<u32> = (cat.codes[from..].iter())
                .map(|&c| map.get(c as usize).copied().unwrap_or(u32::MAX))
                .collect();
            Values::Category {
                codes: codes.into(),
                dict: dict.into(),
            }
        } else {
            let t = self.text.as_deref().unwrap_or_default();
            Values::Text(Arc::from(&t[from.min(t.len())..]))
        };
        Column {
            name: self.name.clone(),
            values,
        }
    }
}

/// A number as a text cell writes it: the shortest form, nothing where missing.
fn number_text(v: f64) -> Box<str> {
    match v.is_nan() {
        true => "".into(),
        false => format!("{v}").into(),
    }
}

/// A time as a text cell writes it: RFC 3339 in UTC, nothing where missing.
fn time_text(v: f64) -> Box<str> {
    if !v.is_finite() {
        return "".into();
    }
    let ns = (v * 1e9).round() as i128;
    jiff::Timestamp::from_nanosecond(ns).map_or_else(|_| "".into(), |t| t.to_string().into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::ColumnKind;
    use jiff::tz::TimeZone;

    fn grow(max: usize) -> Grow {
        Grow::new(ReadOptions::new(TimeZone::UTC), None, max, "./feed.sh")
    }

    fn feed(g: &mut Grow, lines: &[&str]) {
        for l in lines {
            g.line(l).unwrap();
        }
    }

    #[test]
    fn csv_rows_grow_under_their_header_with_types_as_a_file_has_them() {
        let mut g = grow(100);
        feed(
            &mut g,
            &[
                "\u{feff}time,rpm,api",
                "2026-11-22T10:00:00Z,12,a",
                "",
                "2026-11-22T10:00:01Z,7.5,b\r",
            ],
        );
        let t = g.table();
        assert_eq!(t.rows, 2);
        let kinds: Vec<ColumnKind> = t.columns.iter().map(Column::kind).collect();
        assert_eq!(
            kinds,
            [ColumnKind::Time, ColumnKind::Number, ColumnKind::Category]
        );
        assert!(!g.fresh());
        // The same text read as a file reads it: the same table.
        let file = "time,rpm,api\n2026-11-22T10:00:00Z,12,a\n2026-11-22T10:00:01Z,7.5,b\n";
        let read = crate::data::csv::read_csv(file, &ReadOptions::new(TimeZone::UTC)).unwrap();
        assert_eq!(t.columns, read.columns);
        g.line("2026-11-22T10:00:02Z,NA,a").unwrap();
        assert!(g.table().columns[1].f64_at(2).is_nan(), "missing");
    }

    #[test]
    fn a_column_that_stops_being_numbers_turns_to_categories_then_text() {
        let mut g = grow(1000);
        g.line("n").unwrap();
        g.line("1.50").unwrap();
        g.line("2").unwrap();
        assert_eq!(g.table().columns[0].kind(), ColumnKind::Number);
        g.line("lots").unwrap();
        let t = g.table();
        assert_eq!(t.columns[0].kind(), ColumnKind::Category);
        let cells: Vec<_> = (0..3).map(|r| t.columns[0].str_at(r).unwrap()).collect();
        assert_eq!(cells, ["1.50", "2", "lots"], "a category keeps the strings");
        for i in 0..60 {
            g.line(&format!("w{i}")).unwrap();
        }
        let t = g.table();
        assert_eq!(t.columns[0].kind(), ColumnKind::Text);
        assert_eq!(t.columns[0].str_at(0), Some("1.50"));
        assert_eq!(t.rows, 63);
    }

    #[test]
    fn json_lines_make_columns_as_they_are_met_and_skip_what_is_no_row() {
        let mut g = grow(100);
        feed(
            &mut g,
            &[
                "{\"t\": 1, \"v\": 2, \"fig\": {\"color\": \"red\"}}",
                "{\"t\": 2, \"v\": 3, \"note\": \"hi\"}",
                "{\"t\": 3,",
                "42",
            ],
        );
        let t = g.table();
        let names: Vec<&str> = t.columns.iter().map(|c| &*c.name).collect();
        assert_eq!(names, ["t", "v", "note"]);
        assert_eq!(t.rows, 2);
        assert_eq!(t.columns[2].str_at(0), None, "missing before it was met");
        let (n, first) = g.bad();
        assert_eq!(n, 2);
        assert!(
            first
                .unwrap()
                .msg
                .starts_with("line 3 of ./feed.sh's output")
        );
    }

    #[test]
    fn json_arrays_take_their_first_line_as_names() {
        let mut g = grow(100);
        feed(&mut g, &["[\"x\", \"y\"]", "[1, 2]", "[3, 4]", "[5, 6, 7]"]);
        let t = g.table();
        assert_eq!(t.rows, 2);
        assert_eq!(t.columns[1].f64_at(1), 4.0);
        assert_eq!(g.bad().0, 1);
    }

    #[test]
    fn the_newest_rows_are_kept_and_a_category_forgets_what_dropped() {
        let mut g = grow(3);
        g.line("k,v").unwrap();
        for (i, k) in ["old", "a", "b", "a", "b"].iter().enumerate() {
            g.line(&format!("{k},{i}")).unwrap();
        }
        let t = g.table();
        assert_eq!((t.rows, g.dropped()), (3, 2));
        assert_eq!(t.columns[1].f64_at(0), 2.0);
        let Values::Category { dict, codes } = &t.columns[0].values else {
            panic!("a category: {:?}", t.columns[0]);
        };
        assert_eq!(&**dict, [Box::from("a"), Box::from("b")]);
        assert_eq!(&**codes, [1, 0, 1]);
    }

    #[test]
    fn many_rows_are_let_go_of_in_batches_and_the_newest_stay() {
        let mut g = grow(5000);
        g.line("i").unwrap();
        for i in 0..20_000 {
            g.line(&i.to_string()).unwrap();
        }
        let t = g.table();
        assert_eq!(t.rows, 5000);
        assert_eq!(t.columns[0].f64_at(0), 15_000.0);
        assert_eq!(t.columns[0].f64_at(4999), 19_999.0);
        assert!(
            g.len <= 5000 + 4096,
            "the cells themselves go too: {}",
            g.len
        );
    }

    #[test]
    fn a_header_that_cant_be_one_stops_the_stream() {
        let mut g = grow(10);
        assert!(g.line("a,a").is_err());
        assert!(g.failed().is_some());
        assert!(g.line("1,2").is_err());
        let mut g = grow(10);
        feed(&mut g, &["a,b", "1,2,3", "\"open,4"]);
        assert_eq!((g.rows(), g.bad().0), (0, 2));
    }

    #[test]
    fn a_restarted_stream_keeps_its_rows_under_a_new_header() {
        let mut g = grow(10);
        feed(&mut g, &["t,v", "1,10"]);
        g.restart();
        feed(&mut g, &["t,v", "2,20"]);
        let t = g.table();
        assert_eq!(t.rows, 2);
        assert_eq!(t.columns[1].f64_at(1), 20.0);
    }

    #[test]
    fn no_header_names_columns_and_the_widest_row_adds_them() {
        let mut opt = ReadOptions::new(TimeZone::UTC);
        opt.header = false;
        let mut g = Grow::new(opt, Some(DataFormat::Tsv), 10, "x");
        feed(&mut g, &["1\t2", "3\t4\t5"]);
        let t = g.table();
        let names: Vec<&str> = t.columns.iter().map(|c| &*c.name).collect();
        assert_eq!(names, ["column 1", "column 2", "column 3"]);
        assert!(t.columns[2].f64_at(0).is_nan());
    }
}
