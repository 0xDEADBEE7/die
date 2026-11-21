//! What went wrong reading a spec or its data, and where: every error the JSON reader, the spec
//! and the data readers give is a [`FigError`], with a [`Pos`] when it has a place in the text,
//! so the error figure can say "line 4, column 12" and ⏎ can go there. Each has a kind
//! ([`ErrorKind`]), which picks the error figure's glyph; the file it's in when that isn't the
//! figure's own text; and what was tried on the way, which the error figure's log lists.

use std::path::PathBuf;

/// Where in a source text: line and column from 1 (the column in chars), and the byte offset
/// from the text's start.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Pos {
    /// The line, from 1.
    pub line: u32,
    /// The column in chars, from 1.
    pub col: u32,
    /// The byte offset, from 0.
    pub byte: u32,
}

impl Pos {
    /// The place of byte `byte` in `text`: its line and column (in chars) from 1. A byte past
    /// the end is the end; one inside a character is that character's.
    pub fn of(text: &str, byte: usize) -> Pos {
        let mut byte = byte.min(text.len());
        while !text.is_char_boundary(byte) {
            byte -= 1;
        }
        let before = &text[..byte];
        let line = before.matches('\n').count() + 1;
        let start = before.rfind('\n').map_or(0, |i| i + 1);
        let col = text[start..byte].chars().count() + 1;
        Pos {
            line: line as u32,
            col: col as u32,
            byte: byte as u32,
        }
    }
}

/// What kind of failure an error is: what the error figure draws for it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default, PartialOrd, Ord)]
pub enum ErrorKind {
    /// The text couldn't be had: a missing file, one that can't be read, a command that failed.
    Unreadable,
    /// The text isn't what it says it is: JSON or CSV that doesn't parse.
    Malformed,
    /// The text parses but doesn't fit: a field the data hasn't, the wrong type of value, text
    /// where a number should be, a figure type no one knows.
    #[default]
    Mismatch,
    /// Nothing to draw: no rows, an empty buffer, a figure a later build draws.
    Empty,
}

impl ErrorKind {
    /// Its name, as the error figure's log writes it.
    pub fn name(self) -> &'static str {
        match self {
            ErrorKind::Unreadable => "unreadable",
            ErrorKind::Malformed => "malformed",
            ErrorKind::Mismatch => "mismatch",
            ErrorKind::Empty => "empty",
        }
    }

    /// Whether it's a failure to read or to fit (the critical status), rather than nothing to
    /// draw (a warning's).
    pub fn critical(self) -> bool {
        self != ErrorKind::Empty
    }
}

/// What went wrong, and where in the spec or data if known.
#[derive(Clone, Debug, PartialEq)]
pub struct FigError {
    /// What went wrong, in a few words, lower case.
    pub msg: String,
    /// Where, if the error has a place in the text.
    pub at: Option<Pos>,
    /// What kind of failure it is.
    pub kind: ErrorKind,
    /// The file it's in, when that isn't the figure's own text (a data file the spec names).
    pub file: Option<PathBuf>,
    /// What was tried on the way, a line each: the path read and how, the transform that
    /// failed, a command's exit status and the end of what it said.
    pub context: Vec<String>,
    /// It's a warning: something odd the figure was drawn in spite of.
    pub warning: bool,
    /// The line of `file` the error is on, for the log's caret: the figure's own text it can
    /// look up, a data file's it can't.
    pub line: Option<String>,
}

impl FigError {
    /// An error with no place in the text: a mismatch until said otherwise.
    pub fn new(msg: impl Into<String>) -> FigError {
        FigError {
            msg: msg.into(),
            at: None,
            kind: ErrorKind::Mismatch,
            file: None,
            context: Vec::new(),
            warning: false,
            line: None,
        }
    }

    /// An error at a place in the text: a mismatch until said otherwise.
    pub fn at(msg: impl Into<String>, at: Pos) -> FigError {
        FigError {
            at: Some(at),
            ..FigError::new(msg)
        }
    }

    /// A parse error at a place in the text: malformed JSON or CSV.
    pub fn malformed(msg: impl Into<String>, at: Pos) -> FigError {
        FigError::at(msg, at).with_kind(ErrorKind::Malformed)
    }

    /// The same, of another kind.
    pub fn with_kind(mut self, kind: ErrorKind) -> FigError {
        self.kind = kind;
        self
    }

    /// The same, in a file other than the figure's own text, with its line at the error's
    /// place if it has one.
    pub fn in_file(mut self, file: impl Into<PathBuf>, text: Option<&str>) -> FigError {
        self.file = Some(file.into());
        if let (Some(at), Some(text)) = (self.at, text) {
            self.line = text
                .split('\n')
                .nth(at.line.saturating_sub(1) as usize)
                .map(|l| l.trim_end_matches('\r').to_string());
        }
        self
    }

    /// The same, with a line of what was tried after those it has.
    pub fn note(mut self, line: impl Into<String>) -> FigError {
        let line = line.into();
        if !self.context.contains(&line) {
            self.context.push(line);
        }
        self
    }

    /// The same, as a warning.
    pub fn warn(mut self) -> FigError {
        self.warning = true;
        self
    }

    /// The file's name, if it's in one: the error figure and `*messages*` name it.
    pub fn file_name(&self) -> Option<String> {
        let f = self.file.as_ref()?;
        Some(match f.file_name() {
            Some(n) => n.to_string_lossy().into_owned(),
            None => f.display().to_string(),
        })
    }

    /// Where it is, as the error figure says it: `latency.csv, line 6, column 22`, `line 14,
    /// column 9`, the file's name alone, or nothing.
    pub fn place(&self) -> Option<String> {
        let at = self
            .at
            .map(|at| format!("line {}, column {}", at.line, at.col));
        match (self.file_name(), at) {
            (Some(f), Some(at)) => Some(format!("{f}, {at}")),
            (Some(f), None) => Some(f),
            (None, at) => at,
        }
    }
}

impl std::fmt::Display for FigError {
    /// `line L, column C: msg`, or the message alone.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.at {
            Some(at) => write!(f, "line {}, column {}: {}", at.line, at.col, self.msg),
            None => f.write_str(&self.msg),
        }
    }
}

impl std::error::Error for FigError {}

/// A result whose error is a [`FigError`].
pub type Result<T> = std::result::Result<T, FigError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_error_shows_its_place_when_it_has_one() {
        let at = Pos {
            line: 4,
            col: 12,
            byte: 60,
        };
        assert_eq!(
            FigError::at("expected ':'", at).to_string(),
            "line 4, column 12: expected ':'"
        );
        assert_eq!(FigError::new("no data").to_string(), "no data");
    }

    #[test]
    fn the_constructors_default_the_kind_and_the_rest() {
        let e = FigError::new("no column \"x\"");
        assert_eq!(
            (e.kind, e.file.clone(), e.warning),
            (ErrorKind::Mismatch, None, false)
        );
        assert!(e.context.is_empty() && e.line.is_none());
        let at = Pos::of("a\nb,c", 4);
        assert_eq!((at.line, at.col), (2, 3));
        assert_eq!(FigError::malformed("x", at).kind, ErrorKind::Malformed);
    }

    #[test]
    fn an_error_in_a_file_keeps_its_line_and_says_its_place() {
        let text = "t,ms\n1,2\n2,slow\n";
        let e = FigError::at("\"slow\" is not a number", Pos::of(text, 11))
            .in_file("/data/lat.csv", Some(text))
            .note("read lat.csv as CSV")
            .note("read lat.csv as CSV");
        assert_eq!(e.line.as_deref(), Some("2,slow"));
        assert_eq!(e.place().as_deref(), Some("lat.csv, line 3, column 3"));
        assert_eq!(e.context.len(), 1, "a line of what was tried once");
        assert_eq!(
            Pos::of("é\nx", 2),
            Pos {
                line: 1,
                col: 2,
                byte: 2
            }
        );
    }
}
