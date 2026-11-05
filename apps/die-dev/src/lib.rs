//! Your DIE config while DIE is being built. F3 turns it into `die::run(config)`.
//!
//! A known difference until F3: status lines show the buffer's name, where your
//! mode line shows `default-directory`. F3 adds a config field for the status line's
//! text, and this config sets it to the visited file's directory, with `~` for
//! `$HOME` and a trailing `/`, falling back to where DIE started for a buffer that
//! visits no file, as Emacs's `default-directory` does.

#[cfg(test)]
use die_test_cap as _;

pub mod theme;
