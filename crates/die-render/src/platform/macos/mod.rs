//! macOS: AppKit, Metal and Core Text through objc2. Types from winit's older
//! objc2 generation stay inside `appkit` and never appear in a signature here.

/// AppKit.
pub mod appkit;
/// Catching Objective-C exceptions.
pub mod catch;
/// The system clipboard.
pub mod clipboard;
/// The fds a program on a PTY inherits.
pub mod fds;
/// Metal.
pub mod metal;
/// The pixel buffers' staging rings and textures.
pub mod pixbuf;
/// Core Text.
pub mod text;

pub use clipboard::system_clipboard;
pub use fds::close_on_exec_inherited;

/// Whether DIE can draw here: a Mac without Metal can't.
pub fn ready() -> Result<(), String> {
    match metal::Gpu::system_default() {
        Some(_) => Ok(()),
        None => Err("no Metal device".into()),
    }
}
