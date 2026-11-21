//! The JSON reader every spec and every JSON or JSONL data file goes through (ruling 1).
//!
//! serde_json can't serve here: its `Value` sorts an object's keys, drops a duplicate key
//! without a word, and a typed parse through a tagged enum loses the error's place. This reader
//! keeps what a figure needs instead:
//!
//! - every value and every key knows where it starts ([`Pos`]: line, column in chars, byte);
//! - an object's keys stay in document order, so columns, series and colour slots follow the
//!   file and not the alphabet;
//! - a duplicate key is an error, at the second one;
//! - every error has a place.
//!
//! It reads RFC 8259 JSON, strictly: no comments, no trailing commas, no single quotes, no
//! `NaN`. A leading byte order mark is skipped. Numbers are `f64`, so an integer beyond 2^53
//! loses precision, and one beyond `f64`'s range is an error. `\u` escapes take surrogate pairs;
//! a lone surrogate is an error. Nesting deeper than [`DEPTH`] is an error rather than a stack
//! overflow.

pub use crate::error::Pos;
use crate::error::{FigError, Result};
use std::collections::HashSet;

/// How deep arrays and objects may nest.
pub const DEPTH: usize = 256;

/// A JSON value and where it starts.
#[derive(Clone, Debug, PartialEq)]
pub struct Node {
    /// What it is.
    pub kind: Kind,
    /// Where its first character is.
    pub at: Pos,
}

/// What a JSON value is.
#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    /// `null`.
    Null,
    /// `true` or `false`.
    Bool(bool),
    /// A number.
    Num(f64),
    /// A string, its escapes undone.
    Str(Box<str>),
    /// An array.
    Arr(Vec<Node>),
    /// An object: its keys and values in document order, no key twice.
    Obj(Vec<(Key, Node)>),
}

/// An object's key and where its opening quote is.
#[derive(Clone, Debug, PartialEq)]
pub struct Key {
    /// The key, its escapes undone.
    pub name: Box<str>,
    /// Where its opening quote is.
    pub at: Pos,
}

/// One JSON value, with nothing but whitespace after it.
pub fn parse(text: &str) -> Result<Node> {
    Reader::new(text, 1, 0).document()
}

/// JSONL: one value per line that isn't blank, each with its places in the whole text. A line
/// that doesn't parse is an error in its place in the list, and the rest still parse.
pub fn parse_lines(text: &str) -> Vec<Result<Node>> {
    let mut out = Vec::new();
    let (mut start, mut line) = (0usize, 1u32);
    for piece in text.split('\n') {
        let body = match start {
            0 => piece.strip_prefix('\u{feff}').unwrap_or(piece),
            _ => piece,
        };
        if !body.bytes().all(|b| matches!(b, b' ' | b'\t' | b'\r')) {
            out.push(Reader::new(piece, line, start).document());
        }
        start += piece.len() + 1;
        line += 1;
    }
    out
}

impl Node {
    /// The value under `key`, if this is an object that has it.
    pub fn get(&self, key: &str) -> Option<&Node> {
        let obj = self.as_obj()?;
        obj.iter().find(|(k, _)| &*k.name == key).map(|(_, v)| v)
    }

    /// The string, if this is one.
    pub fn as_str(&self) -> Option<&str> {
        match &self.kind {
            Kind::Str(s) => Some(s),
            _ => None,
        }
    }

    /// The number, if this is one.
    pub fn as_f64(&self) -> Option<f64> {
        match self.kind {
            Kind::Num(v) => Some(v),
            _ => None,
        }
    }

    /// The boolean, if this is one.
    pub fn as_bool(&self) -> Option<bool> {
        match self.kind {
            Kind::Bool(b) => Some(b),
            _ => None,
        }
    }

    /// The elements, if this is an array.
    pub fn as_arr(&self) -> Option<&[Node]> {
        match &self.kind {
            Kind::Arr(a) => Some(a),
            _ => None,
        }
    }

    /// The keys and values in document order, if this is an object.
    pub fn as_obj(&self) -> Option<&[(Key, Node)]> {
        match &self.kind {
            Kind::Obj(o) => Some(o),
            _ => None,
        }
    }

    /// Whether this is `null`.
    pub fn is_null(&self) -> bool {
        matches!(self.kind, Kind::Null)
    }

    /// What it is, for a message: "null", "boolean", "number", "string", "array" or "object".
    pub fn type_name(&self) -> &'static str {
        match self.kind {
            Kind::Null => "null",
            Kind::Bool(_) => "boolean",
            Kind::Num(_) => "number",
            Kind::Str(_) => "string",
            Kind::Arr(_) => "array",
            Kind::Obj(_) => "object",
        }
    }

    /// The value as compact JSON, keys in document order. An integer up to 2^53 is written
    /// without a fraction; other numbers as Rust writes the shortest `f64` that reads back.
    pub fn to_text(&self) -> String {
        let mut out = String::new();
        self.write(&mut out);
        out
    }

    fn write(&self, out: &mut String) {
        match &self.kind {
            Kind::Null => out.push_str("null"),
            Kind::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            Kind::Num(v) => write_number(*v, out),
            Kind::Str(s) => write_string(s, out),
            Kind::Arr(a) => {
                out.push('[');
                for (i, v) in a.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    v.write(out);
                }
                out.push(']');
            }
            Kind::Obj(o) => {
                out.push('{');
                for (i, (k, v)) in o.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    write_string(&k.name, out);
                    out.push(':');
                    v.write(out);
                }
                out.push('}');
            }
        }
    }
}

/// A number as JSON: whole numbers short of 2^53 without a fraction or an exponent, the rest
/// as `f64`'s shortest round trip (`1.5`, `1e300`).
fn write_number(v: f64, out: &mut String) {
    use std::fmt::Write;
    if v.fract() == 0.0 && v.abs() <= 9_007_199_254_740_992.0 {
        let _ = write!(out, "{v}");
    } else if v.is_finite() {
        let _ = write!(out, "{v:?}");
    } else {
        out.push_str("null");
    }
}

/// A string as JSON, escaping what must be.
fn write_string(s: &str, out: &mut String) {
    use std::fmt::Write;
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            c if u32::from(c) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", u32::from(c));
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

/// Keys an object holds before duplicates are found through a set rather than by looking.
const SCAN: usize = 16;

/// A place in a text being read.
struct Reader<'a> {
    src: &'a str,
    bytes: &'a [u8],
    i: usize,
    line: u32,
    col: u32,
    /// The byte offset of `src` in the whole text, for JSONL's lines.
    base: usize,
    depth: usize,
}

impl<'a> Reader<'a> {
    fn new(src: &'a str, line: u32, base: usize) -> Reader<'a> {
        let i = if base == 0 && src.starts_with('\u{feff}') {
            3
        } else {
            0
        };
        Reader {
            src,
            bytes: src.as_bytes(),
            i,
            line,
            col: 1,
            base,
            depth: 0,
        }
    }

    fn pos(&self) -> Pos {
        Pos {
            line: self.line,
            col: self.col,
            byte: u32::try_from(self.base + self.i).unwrap_or(u32::MAX),
        }
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.i).copied()
    }

    /// Steps over one byte, keeping the line and column.
    fn bump(&mut self) {
        let Some(&b) = self.bytes.get(self.i) else {
            return;
        };
        self.i += 1;
        if b == b'\n' {
            self.line += 1;
            self.col = 1;
        } else if b & 0xC0 != 0x80 {
            self.col += 1;
        }
    }

    fn ws(&mut self) {
        while let Some(b' ' | b'\t' | b'\n' | b'\r') = self.peek() {
            self.bump();
        }
    }

    fn fail(&self, msg: &str) -> FigError {
        FigError::malformed(msg, self.pos())
    }

    fn eof(&self) -> FigError {
        self.fail("unexpected end of input")
    }

    fn document(mut self) -> Result<Node> {
        let node = self.value()?;
        self.ws();
        match self.peek() {
            None => Ok(node),
            Some(_) => Err(self.fail("unexpected text after the value")),
        }
    }

    fn value(&mut self) -> Result<Node> {
        self.ws();
        let at = self.pos();
        let Some(b) = self.peek() else {
            return Err(self.eof());
        };
        let kind = match b {
            b'{' => self.object()?,
            b'[' => self.array()?,
            b'"' => Kind::Str(self.string()?.into()),
            b'-' | b'0'..=b'9' => Kind::Num(self.number()?),
            b't' => self.word("true", Kind::Bool(true))?,
            b'f' => self.word("false", Kind::Bool(false))?,
            b'n' => self.word("null", Kind::Null)?,
            _ => return Err(self.fail("expected a value")),
        };
        Ok(Node { kind, at })
    }

    fn word(&mut self, word: &str, kind: Kind) -> Result<Kind> {
        let rest = &self.bytes[self.i..];
        if rest.starts_with(word.as_bytes()) {
            for _ in 0..word.len() {
                self.bump();
            }
            Ok(kind)
        } else if word.as_bytes().starts_with(rest) {
            let mut end = Reader { ..*self };
            while end.peek().is_some() {
                end.bump();
            }
            Err(end.eof())
        } else {
            Err(self.fail("expected a value"))
        }
    }

    fn enter(&mut self) -> Result<()> {
        if self.depth >= DEPTH {
            return Err(self.fail("nested too deeply"));
        }
        self.depth += 1;
        self.bump();
        Ok(())
    }

    fn object(&mut self) -> Result<Kind> {
        self.enter()?;
        let mut entries: Vec<(Key, Node)> = Vec::new();
        let mut seen: Option<HashSet<Box<str>>> = None;
        self.ws();
        if self.peek() == Some(b'}') {
            self.bump();
            self.depth -= 1;
            return Ok(Kind::Obj(entries));
        }
        loop {
            self.ws();
            match self.peek() {
                None => return Err(self.eof()),
                Some(b'"') => {}
                Some(_) => return Err(self.fail("expected a string key")),
            }
            let at = self.pos();
            let name: Box<str> = self.string()?.into();
            let twice = match &mut seen {
                Some(set) => !set.insert(name.clone()),
                None => entries.iter().any(|(k, _)| k.name == name),
            };
            if twice {
                return Err(FigError::malformed(format!("duplicate key \"{name}\""), at));
            }
            if seen.is_none() && entries.len() >= SCAN {
                let mut set: HashSet<Box<str>> =
                    entries.iter().map(|(k, _)| k.name.clone()).collect();
                set.insert(name.clone());
                seen = Some(set);
            }
            self.ws();
            match self.peek() {
                None => return Err(self.eof()),
                Some(b':') => self.bump(),
                Some(_) => return Err(self.fail("expected ':'")),
            }
            let value = self.value()?;
            entries.push((Key { name, at }, value));
            self.ws();
            match self.peek() {
                None => return Err(self.eof()),
                Some(b',') => self.bump(),
                Some(b'}') => {
                    self.bump();
                    break;
                }
                Some(_) => return Err(self.fail("expected ',' or '}'")),
            }
        }
        self.depth -= 1;
        Ok(Kind::Obj(entries))
    }

    fn array(&mut self) -> Result<Kind> {
        self.enter()?;
        let mut items = Vec::new();
        self.ws();
        if self.peek() == Some(b']') {
            self.bump();
            self.depth -= 1;
            return Ok(Kind::Arr(items));
        }
        loop {
            items.push(self.value()?);
            self.ws();
            match self.peek() {
                None => return Err(self.eof()),
                Some(b',') => self.bump(),
                Some(b']') => {
                    self.bump();
                    break;
                }
                Some(_) => return Err(self.fail("expected ',' or ']'")),
            }
        }
        self.depth -= 1;
        Ok(Kind::Arr(items))
    }

    /// A string from its opening quote, its escapes undone.
    fn string(&mut self) -> Result<String> {
        let open = self.pos();
        let unterminated = || FigError::malformed("unterminated string", open);
        self.bump();
        let mut out = String::new();
        loop {
            let start = self.i;
            while let Some(b) = self.peek() {
                if b == b'"' || b == b'\\' || b < 0x20 {
                    break;
                }
                self.bump();
            }
            out.push_str(&self.src[start..self.i]);
            match self.peek() {
                None | Some(b'\n') => return Err(unterminated()),
                Some(b'"') => {
                    self.bump();
                    return Ok(out);
                }
                Some(b'\\') => {
                    let esc = self.pos();
                    self.bump();
                    let Some(e) = self.peek() else {
                        return Err(unterminated());
                    };
                    let plain = match e {
                        b'"' => Some('"'),
                        b'\\' => Some('\\'),
                        b'/' => Some('/'),
                        b'b' => Some('\u{8}'),
                        b'f' => Some('\u{c}'),
                        b'n' => Some('\n'),
                        b'r' => Some('\r'),
                        b't' => Some('\t'),
                        b'u' => None,
                        _ => return Err(FigError::malformed("invalid escape", esc)),
                    };
                    self.bump();
                    match plain {
                        Some(c) => out.push(c),
                        None => out.push(self.unicode(esc, open)?),
                    }
                }
                Some(_) => return Err(self.fail("control character in string")),
            }
        }
    }

    /// A `\u` escape's character, after its `u`: a pair of surrogates makes one.
    fn unicode(&mut self, esc: Pos, open: Pos) -> Result<char> {
        let lone = || FigError::malformed("lone surrogate in \\u escape", esc);
        let first = self.hex4(esc, open)?;
        let code = match first {
            0xD800..=0xDBFF => {
                if !self.bytes[self.i..].starts_with(b"\\u") {
                    return Err(lone());
                }
                self.bump();
                self.bump();
                let second = self.hex4(esc, open)?;
                if !(0xDC00..=0xDFFF).contains(&second) {
                    return Err(lone());
                }
                0x10000 + ((first - 0xD800) << 10) + (second - 0xDC00)
            }
            0xDC00..=0xDFFF => return Err(lone()),
            c => c,
        };
        char::from_u32(code).ok_or_else(lone)
    }

    fn hex4(&mut self, esc: Pos, open: Pos) -> Result<u32> {
        let mut v = 0;
        for _ in 0..4 {
            let Some(b) = self.peek() else {
                return Err(FigError::malformed("unterminated string", open));
            };
            let d = (b as char)
                .to_digit(16)
                .ok_or_else(|| FigError::malformed("invalid \\u escape", esc))?;
            v = v * 16 + d;
            self.bump();
        }
        Ok(v)
    }

    /// A number, by JSON's grammar: an optional minus, no leading zeros, a fraction and an
    /// exponent each with at least one digit.
    fn number(&mut self) -> Result<f64> {
        let at = self.pos();
        let start = self.i;
        let invalid = || FigError::malformed("invalid number", at);
        if self.peek() == Some(b'-') {
            self.bump();
        }
        match self.peek() {
            Some(b'0') => {
                self.bump();
                if matches!(self.peek(), Some(b'0'..=b'9')) {
                    return Err(invalid());
                }
            }
            Some(b'1'..=b'9') => self.digits(),
            _ => return Err(invalid()),
        }
        if self.peek() == Some(b'.') {
            self.bump();
            if !matches!(self.peek(), Some(b'0'..=b'9')) {
                return Err(invalid());
            }
            self.digits();
        }
        if let Some(b'e' | b'E') = self.peek() {
            self.bump();
            if let Some(b'+' | b'-') = self.peek() {
                self.bump();
            }
            if !matches!(self.peek(), Some(b'0'..=b'9')) {
                return Err(invalid());
            }
            self.digits();
        }
        if matches!(self.peek(), Some(b) if b.is_ascii_alphanumeric() || b == b'.') {
            return Err(invalid());
        }
        let v: f64 = self.src[start..self.i].parse().map_err(|_| invalid())?;
        if !v.is_finite() {
            return Err(FigError::malformed("number out of range", at));
        }
        Ok(v)
    }

    fn digits(&mut self) {
        while let Some(b'0'..=b'9') = self.peek() {
            self.bump();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pos(line: u32, col: u32, byte: u32) -> Pos {
        Pos { line, col, byte }
    }

    /// The error a text gives, as its message and place.
    fn err(text: &str) -> (String, Pos) {
        let e = parse(text).expect_err(text);
        (e.msg, e.at.expect("an error with a place"))
    }

    #[test]
    fn every_error_says_what_and_where() {
        let cases: &[(&str, &str, Pos)] = &[
            (r#"{"a": 1 "b": 2}"#, "expected ',' or '}'", pos(1, 9, 8)),
            ("[1 2]", "expected ',' or ']'", pos(1, 4, 3)),
            (r#"{"a" 1}"#, "expected ':'", pos(1, 6, 5)),
            ("{1: 2}", "expected a string key", pos(1, 2, 1)),
            (r#"{"a": 1,}"#, "expected a string key", pos(1, 9, 8)),
            ("[1,]", "expected a value", pos(1, 4, 3)),
            (r#"{"a": }"#, "expected a value", pos(1, 7, 6)),
            ("[+1]", "expected a value", pos(1, 2, 1)),
            ("[nan]", "expected a value", pos(1, 2, 1)),
            ("{\"a\": \"abc", "unterminated string", pos(1, 7, 6)),
            ("[\"ab\ncd\"]", "unterminated string", pos(1, 2, 1)),
            ("[01]", "invalid number", pos(1, 2, 1)),
            ("[1.]", "invalid number", pos(1, 2, 1)),
            ("[-]", "invalid number", pos(1, 2, 1)),
            ("[1e]", "invalid number", pos(1, 2, 1)),
            ("[1e+]", "invalid number", pos(1, 2, 1)),
            ("[12abc]", "invalid number", pos(1, 2, 1)),
            ("[1.2.3]", "invalid number", pos(1, 2, 1)),
            ("[1e400]", "number out of range", pos(1, 2, 1)),
            ("", "unexpected end of input", pos(1, 1, 0)),
            ("{\"a\": ", "unexpected end of input", pos(1, 7, 6)),
            ("[1,", "unexpected end of input", pos(1, 4, 3)),
            ("[1", "unexpected end of input", pos(1, 3, 2)),
            ("nul", "unexpected end of input", pos(1, 4, 3)),
            ("{\"a\"", "unexpected end of input", pos(1, 5, 4)),
            (r#""\x""#, "invalid escape", pos(1, 2, 1)),
            (r#""a\u12G4""#, "invalid \\u escape", pos(1, 3, 2)),
            (r#""\uD83D""#, "lone surrogate in \\u escape", pos(1, 2, 1)),
            (r#""\uDE00""#, "lone surrogate in \\u escape", pos(1, 2, 1)),
            (r#""\uD83DA""#, "lone surrogate in \\u escape", pos(1, 2, 1)),
            ("\"a\tb\"", "control character in string", pos(1, 3, 2)),
            ("{} x", "unexpected text after the value", pos(1, 4, 3)),
            ("1 2", "unexpected text after the value", pos(1, 3, 2)),
        ];
        for (text, msg, at) in cases {
            assert_eq!(err(text), (msg.to_string(), *at), "{text:?}");
        }
    }

    #[test]
    fn a_duplicate_key_is_an_error_at_the_second() {
        assert_eq!(
            err("{\"a\": 1,\n \"a\": 2}"),
            ("duplicate key \"a\"".to_string(), pos(2, 2, 10))
        );
        // Past the keys looked through one by one, a set finds it.
        let mut many: Vec<String> = (0..40).map(|i| format!("\"k{i}\": {i}")).collect();
        many.push("\"k3\": 0".into());
        let text = format!("{{{}}}", many.join(", "));
        let (msg, at) = err(&text);
        assert_eq!(msg, "duplicate key \"k3\"");
        assert_eq!(at.byte as usize, text.rfind("\"k3\"").unwrap());
        // The same name in two objects is no duplicate.
        assert!(parse(r#"[{"a": 1}, {"a": 2}]"#).is_ok());
    }

    #[test]
    fn places_are_lines_and_columns_in_chars() {
        // 'é' is two bytes and one column; a line break starts column 1 again.
        assert_eq!(
            err("{\"é\": 1 \"b\": 2}"),
            ("expected ',' or '}'".to_string(), pos(1, 9, 9))
        );
        assert_eq!(
            err("[\n  1,\n  2\n  3]"),
            ("expected ',' or ']'".to_string(), pos(4, 3, 13))
        );
        let e = parse("{\n\"a\": 1,\n\"a\": 2}").unwrap_err();
        assert_eq!(e.to_string(), "line 3, column 1: duplicate key \"a\"");
        // A byte order mark is no column.
        assert_eq!(err("\u{feff}[1 2]").1, pos(1, 4, 6));
    }

    #[test]
    fn values_and_keys_know_where_they_start() {
        let n = parse("{\n  \"a\": [1, true],\n  \"b\": null\n}").unwrap();
        assert_eq!(n.at, pos(1, 1, 0));
        let obj = n.as_obj().unwrap();
        assert_eq!(obj[0].0.at, pos(2, 3, 4));
        assert_eq!(obj[0].1.at, pos(2, 8, 9));
        assert_eq!(obj[0].1.as_arr().unwrap()[1].at, pos(2, 12, 13));
        assert_eq!(obj[1].0.at, pos(3, 3, 22));
        assert_eq!(obj[1].1.at, pos(3, 8, 27));
    }

    #[test]
    fn keys_stay_in_document_order() {
        let n = parse(r#"{"time":1,"api":2,"worker":3,"batch":4}"#).unwrap();
        let keys: Vec<&str> = n.as_obj().unwrap().iter().map(|(k, _)| &*k.name).collect();
        assert_eq!(keys, ["time", "api", "worker", "batch"]);
        assert_eq!(n.to_text(), r#"{"time":1,"api":2,"worker":3,"batch":4}"#);
        assert_eq!(n.get("worker").and_then(Node::as_f64), Some(3.0));
        assert_eq!(n.get("none"), None);
    }

    #[test]
    fn escapes_are_undone_surrogate_pairs_included() {
        let n = parse(r#""q\" b\\ s\/ \b\f\n\r\t""#).unwrap();
        assert_eq!(n.as_str(), Some("q\" b\\ s/ \u{8}\u{c}\n\r\t"));
        assert_eq!(parse(r#""éé""#).unwrap().as_str(), Some("éé"));
        assert_eq!(parse(r#""😀!""#).unwrap().as_str(), Some("😀!"));
        assert_eq!(parse(r#""😀""#).unwrap().as_str(), Some("😀"));
        assert_eq!(parse("\"日本 😀\"").unwrap().as_str(), Some("日本 😀"));
        // Keys too.
        let n = parse(r#"{"ab": 1}"#).unwrap();
        assert_eq!(n.as_obj().unwrap()[0].0.name.as_ref(), "ab");
        // And back: what must be escaped is, the rest is as it is.
        let text = parse(r#""q\" \\ \n\t\u0001 é 😀""#).unwrap().to_text();
        assert_eq!(text, r#""q\" \\ \n\t\u0001 é 😀""#);
    }

    #[test]
    fn numbers_are_json_numbers_as_f64() {
        let n = parse("[0, -0, 1.5, -2e3, 1E+2, 1e-2, 12.50, 9007199254740993]").unwrap();
        let v: Vec<f64> = n
            .as_arr()
            .unwrap()
            .iter()
            .filter_map(Node::as_f64)
            .collect();
        assert_eq!(
            v,
            [
                0.0,
                -0.0,
                1.5,
                -2000.0,
                100.0,
                0.01,
                12.5,
                9007199254740992.0
            ]
        );
        assert!(v[1].is_sign_negative());
        assert_eq!(
            n.to_text(),
            "[0,-0,1.5,-2000,100,0.01,12.5,9007199254740992]"
        );
        assert_eq!(parse("1e300").unwrap().to_text(), "1e300");
        assert_eq!(parse("0.000001").unwrap().to_text(), "1e-6");
    }

    #[test]
    fn the_rest_of_the_node_api() {
        let n = parse(r#"{"s":"x","n":2,"b":false,"z":null,"a":[],"o":{}}"#).unwrap();
        let names: Vec<&str> = n
            .as_obj()
            .unwrap()
            .iter()
            .map(|(_, v)| v.type_name())
            .collect();
        assert_eq!(
            names,
            ["string", "number", "boolean", "null", "array", "object"]
        );
        assert_eq!(n.get("s").unwrap().as_str(), Some("x"));
        assert_eq!(n.get("b").unwrap().as_bool(), Some(false));
        assert!(n.get("z").unwrap().is_null());
        assert_eq!(n.get("a").unwrap().as_arr(), Some(&[][..]));
        assert_eq!(n.get("o").unwrap().as_obj(), Some(&[][..]));
        assert_eq!(n.get("s").unwrap().as_f64(), None);
        assert_eq!(n.get("n").unwrap().as_str(), None);
        assert_eq!(
            n.to_text(),
            r#"{"s":"x","n":2,"b":false,"z":null,"a":[],"o":{}}"#
        );
        assert_eq!(parse(" \n true \r\n").unwrap().as_bool(), Some(true));
    }

    #[test]
    fn too_deep_is_an_error_not_an_overflow() {
        let deep = "[".repeat(DEPTH + 1);
        assert_eq!(
            err(&deep),
            ("nested too deeply".to_string(), pos(1, 257, 256))
        );
        let fine = format!("{}{}", "[".repeat(DEPTH), "]".repeat(DEPTH));
        assert!(parse(&fine).is_ok());
    }

    #[test]
    fn jsonl_is_a_value_a_line_with_places_in_the_whole_text() {
        let text = "{\"a\":1}\n\n   \n{\"a\":2}\r\n{bad}\n\"tail\"";
        let lines = parse_lines(text);
        assert_eq!(lines.len(), 4);
        let first = lines[0].as_ref().unwrap();
        assert_eq!(
            (first.get("a").unwrap().as_f64(), first.at),
            (Some(1.0), pos(1, 1, 0))
        );
        let second = lines[1].as_ref().unwrap();
        assert_eq!(second.at, pos(4, 1, 13));
        assert_eq!(second.get("a").unwrap().at, pos(4, 6, 18));
        let bad = lines[2].as_ref().unwrap_err();
        assert_eq!(bad.msg, "expected a string key");
        assert_eq!(bad.at, Some(pos(5, 2, 23)));
        assert_eq!(lines[3].as_ref().unwrap().as_str(), Some("tail"));
        assert!(parse_lines("").is_empty());
        assert!(parse_lines("\u{feff}\n \n\t\r\n").is_empty());
        // A line cut short is an error at its end, and the next still parses.
        let cut = parse_lines("{\"a\": [1, 2\n{\"a\": 3}\n");
        assert_eq!(cut[0].as_ref().unwrap_err().at, Some(pos(1, 12, 11)));
        assert!(cut[1].is_ok());
    }
}
