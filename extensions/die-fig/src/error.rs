//! What went wrong reading a spec or its data, and where: every error the JSON reader, the spec
//! and the data readers give is a [`FigError`], with a [`Pos`] when it has a place in the text,
//! so the banner can say "line 4, column 12" and ⏎ can go there.

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

/// What went wrong, and where in the spec or data if known.
#[derive(Clone, Debug, PartialEq)]
pub struct FigError {
    /// What went wrong, in a few words, lower case.
    pub msg: String,
    /// Where, if the error has a place in the text.
    pub at: Option<Pos>,
}

impl FigError {
    /// An error with no place in the text.
    pub fn new(msg: impl Into<String>) -> FigError {
        FigError {
            msg: msg.into(),
            at: None,
        }
    }

    /// An error at a place in the text.
    pub fn at(msg: impl Into<String>, at: Pos) -> FigError {
        FigError {
            msg: msg.into(),
            at: Some(at),
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
}
