//! DIE's facade (scope §12, Crates). `die::run(config)` arrives with F3; until then
//! this crate only re-exports the crates a config builds on.

pub use die_core as core;
pub use die_render as render;
pub use die_vim as vim;
