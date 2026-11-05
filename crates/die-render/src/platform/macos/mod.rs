//! macOS: AppKit, Metal and Core Text through objc2. Types from winit's older
//! objc2 generation stay inside `appkit` and never appear in a signature here.

/// AppKit.
pub mod appkit;
/// The system clipboard.
pub mod clipboard;
/// Metal.
pub mod metal;
/// Core Text.
pub mod text;

pub use clipboard::system_clipboard;

/// Whether DIE can draw here: a Mac without Metal can't.
pub fn ready() -> Result<(), String> {
    match metal::Gpu::system_default() {
        Some(_) => Ok(()),
        None => Err("no Metal device".into()),
    }
}
