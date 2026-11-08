//! Errors. `Checkout` is the C2 result for `with_surface`, `with_mode` and `with_ext`.
#![warn(missing_docs)]

use std::fmt;

/// Why a checkout failed (C2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Checkout {
    /// The buffer (and so its surface or mode) was killed.
    Gone,
    /// The object is already checked out: a re-entrancy bug, not a dead buffer.
    Busy,
    /// No such object: the extension type isn't registered, or the buffer has no such mode or surface.
    Missing,
}

/// What can go wrong in a command or a call to core (contract §15).
#[derive(Debug, Clone, PartialEq)]
pub enum Error {
    /// A checkout failed (C2).
    Checkout(Checkout),
    /// A `User` edit touched a read-only range (C14).
    ReadOnly {
        /// Where the edit starts.
        at: usize,
    },
    /// A range is reversed, past the end or off a character boundary, or overlaps another change
    /// in a transaction.
    BadRange {
        /// The range's start.
        start: usize,
        /// The range's end.
        end: usize,
    },
    /// The buffer has a surface, not text.
    NotText,
    /// No command has this name.
    NoCommand(String),
    /// Extension code panicked (C9).
    Panicked {
        /// The extension.
        ext: &'static str,
        /// The panic's message.
        msg: String,
    },
    /// Nothing left to undo or redo.
    NothingToUndo,
    /// A search pattern that isn't valid regex (Rust's syntax, D4).
    BadPattern(String),
    /// An extension's silent edit overlapped the text this undo step needs. History is truncated.
    /// It also reports, once, history dropped earlier for that reason while a step was collecting.
    UndoConflict,
    /// Anything else, as text for the command line.
    Msg(String),
}

impl fmt::Display for Error {
    /// The text for a person: the command line, `*messages*` or stderr.
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Error::Checkout(Checkout::Gone) => f.write_str("that buffer is gone"),
            Error::Checkout(Checkout::Busy) => f.write_str("that's already checked out"),
            Error::Checkout(Checkout::Missing) => f.write_str("there's no such object"),
            Error::ReadOnly { at } => write!(f, "the text at {at} is read-only"),
            Error::BadRange { start, end } => write!(f, "bad range {start}..{end}"),
            Error::NotText => f.write_str("that buffer has no text"),
            Error::NoCommand(name) => write!(f, "no command named {name}"),
            Error::Panicked { ext, msg } => write!(f, "{ext} panicked: {msg}"),
            Error::NothingToUndo => f.write_str("nothing to undo"),
            Error::BadPattern(why) => write!(f, "bad pattern: {why}"),
            Error::UndoConflict => f.write_str("undo history was cut short by an overlapping edit"),
            Error::Msg(text) => f.write_str(text),
        }
    }
}

impl From<Checkout> for Error {
    fn from(c: Checkout) -> Self {
        Error::Checkout(c)
    }
}

/// A result whose error is core's, and whose value is `()` unless named.
pub type Result<T = (), E = Error> = std::result::Result<T, E>;
