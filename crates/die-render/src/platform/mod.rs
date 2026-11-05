//! Per-platform code: the only `unsafe` in die-render (scope §12), which you
//! review by hand, and the only place in it that branches on `target_os`
//! (docs/records/platforms.md §5.2; `cargo xtask lint` checks both).
//!
//! The rule: thin wrappers only. Each function makes one framework call or a few,
//! checks what it can, and returns safe types. Anything with logic of its own
//! (the atlas, the row cache, instance building, key mapping) is neutral code
//! outside this module.
//!
//! Neutral code names `platform::backend`, and nothing outside `platform` names
//! `platform::macos` (`cargo xtask lint` checks). Only the frame driver calls the
//! backend. The rest of the neutral code takes data in and gives data out, such
//! as glyph bitmaps and metrics as values, so its tests run under `ci --linux`,
//! and the driver's skip where there's no backend. There are no traits until a
//! second implementation needs them (platforms.md §5.2).
//!
//! macOS has `macos`. Other targets have no window backend yet: they get a stub
//! with only the items neutral code names, each saying "no window backend on this
//! platform yet", until Linux gets the portable backend and `platform::linux`.
//!
//! Two rules for the code R3 and R4 bring:
//! - A Rust closure that Objective-C or C calls (a block, a delegate method, a
//!   display-link callback) runs its body in `catch_unwind`. It logs and drops the
//!   event, and never lets a panic unwind into the framework. winit's own event
//!   handlers already do this, so the rule is for DIE's own closures.
//! - AppKit calls take a `MainThreadMarker`, so they can't compile off the main
//!   thread.

/// macOS: AppKit, Metal and Core Text.
#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
pub mod macos;

/// The backend neutral code calls: `macos` on macOS.
#[cfg(target_os = "macos")]
pub use macos as backend;

/// The backend neutral code calls: a stub here, since this target has none yet.
#[cfg(not(target_os = "macos"))]
pub mod backend;
