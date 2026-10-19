//! macOS: AppKit, Metal and Core Text through objc2. Types from winit's older
//! objc2 generation stay inside `appkit` and never appear in a signature here.

pub mod appkit;
pub mod metal;
pub mod text;
