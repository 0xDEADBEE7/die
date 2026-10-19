//! Errors. `Checkout` is the C2 result for `with_surface`, `with_mode` and `with_ext`.

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

#[derive(Debug, Clone, PartialEq)]
pub enum Error {
    Checkout(Checkout),
    /// A `User` edit touched a read-only range (C14).
    ReadOnly {
        at: usize,
    },
    BadRange {
        start: usize,
        end: usize,
    },
    NotText,
    NoCommand(String),
    Panicked {
        ext: &'static str,
        msg: String,
    },
    NothingToUndo,
    /// A search pattern that isn't valid regex (Rust's syntax, D4).
    BadPattern(String),
    /// An extension's silent edit overlapped the text this undo step needs. History is truncated.
    UndoConflict,
    Msg(String),
}

impl From<Checkout> for Error {
    fn from(c: Checkout) -> Self {
        Error::Checkout(c)
    }
}

pub type Result<T = (), E = Error> = std::result::Result<T, E>;
