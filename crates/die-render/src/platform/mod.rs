//! The only `unsafe` code in die-render (scope §12), which you review by hand.
//!
//! The rule: thin wrappers only. Each function makes one framework call or a few,
//! checks what it can, and returns safe types. Anything with logic of its own
//! (the atlas, the row cache, instance building, key mapping) lives outside this
//! module and calls in. Types from winit's older objc2 generation stay inside
//! `appkit` and never appear in a signature here.

pub mod appkit;
pub mod metal;
pub mod text;
