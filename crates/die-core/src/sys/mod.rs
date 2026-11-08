//! What differs by platform in die-core (docs/records/platforms.md §5.2): the path to DIE's
//! state, and the path to DIE's own binary. PTYs need nothing else that differs; the holder's
//! part of `sys` comes with D20.

use std::path::PathBuf;

/// Where DIE keeps its state: `$DIE_STATE_DIR` if it's set, else the platform's place for
/// it, under `$HOME`. None if there's neither.
pub fn state_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("DIE_STATE_DIR") {
        return Some(PathBuf::from(dir));
    }
    platform_state_dir()
}

#[cfg(target_os = "macos")]
fn platform_state_dir() -> Option<PathBuf> {
    Some(std::env::home_dir()?.join("Library/Application Support/DIE"))
}

#[cfg(not(target_os = "macos"))]
fn platform_state_dir() -> Option<PathBuf> {
    match std::env::var_os("XDG_STATE_HOME") {
        Some(dir) if !dir.is_empty() => Some(PathBuf::from(dir).join("die")),
        _ => Some(std::env::home_dir()?.join(".local/state/die")),
    }
}

/// A path that runs this process's own binary, for a PTY's trampoline (`crate::pty`). On Linux
/// it's `/proc/self/exe`, which still runs it once a rebuild has replaced the file.
#[cfg(target_os = "linux")]
pub fn own_exe() -> std::io::Result<PathBuf> {
    Ok(PathBuf::from("/proc/self/exe"))
}

/// A path that runs this process's own binary, for a PTY's trampoline (`crate::pty`).
#[cfg(not(target_os = "linux"))]
pub fn own_exe() -> std::io::Result<PathBuf> {
    std::env::current_exe()
}
