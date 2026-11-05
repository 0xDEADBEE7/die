//! DIE's dev tasks, run as `cargo xtask <task>`. Dev-only: nothing here is in
//! DIE's build, and `cargo build` and `cargo test` don't need it.

#[cfg(test)]
use die_test_cap as _;

pub mod api;
pub mod budget;
pub mod ci;
pub mod clean;
pub mod count;
pub mod cross;
pub mod fonts;
pub mod guards;
pub mod lint;
pub mod linux;
pub mod mutate;
pub mod sources;
pub mod util;
