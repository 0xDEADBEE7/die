//! mini-fig: a spike of die-fig at about a tenth of its size, reading the same `.fig` specs, so
//! the owner can open a dashboard in both and feel what is lost (scope D14).
//!
//! - [`spec`]: a `.fig` file read into mini-fig's own types, every figure a dashboard;
//! - [`data`]: tables of strings, numbers and times, read from CSV, filtered and sorted;
//! - [`scale`]: linear, log, time and band scales, their ticks, and how numbers are written.
//!
//! Nothing of die-fig is used or changed. `\V` runs `mini-fig-toggle`.

#[cfg(test)]
use die_test_cap as _;

pub mod data;
pub mod scale;
pub mod spec;

use die_core::{Extension, Registry};

/// The extension: registers `mini-fig-toggle`.
#[derive(Debug, Default)]
pub struct MiniFig {
    /// Nothing yet: the figures shown come with the surface.
    _figures: (),
}

impl Extension for MiniFig {
    fn name(&self) -> &'static str {
        "mini-fig"
    }

    fn register(&mut self, reg: &mut Registry) {
        reg.command(
            "mini-fig-toggle",
            "Show this .fig file's figure in mini-fig, the small spike of fig, or from a figure go back to its file.",
            |ctx, _| {
                ctx.messages.push("mini-fig: nothing drawn yet".to_string());
                Ok(())
            },
        );
    }
}
