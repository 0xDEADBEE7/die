//! The backend on a target that has none yet. It holds only the items neutral
//! code names, each failing the same way, until `platform::linux` and the portable
//! backend land (docs/records/platforms.md §5.2).

/// Whether DIE can draw here. It can't yet.
pub fn ready() -> Result<(), String> {
    Err("no window backend on this platform yet".into())
}
