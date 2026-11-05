//! What differs by platform in die-core (docs/records/platforms.md §5.2). M0 has only the
//! path to DIE's state; the rest of `sys` waits for M2.

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
