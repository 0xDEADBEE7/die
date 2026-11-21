//! Easing for discrete changes: keys, legend clicks, a brush's release, undo and a double-click
//! reset ease over [`super::consts::MOTION_MS`]; the wheel, the trackpad and a drag follow the
//! hand and never ease.

/// Ease-out cubic: fast, then settling. `t` from 0 to 1.
pub fn ease_out_cubic(_t: f32) -> f32 {
    todo!("P2-surface: ease_out_cubic")
}

/// A value easing from one to another, from a time on the fake clock, for so long.
#[derive(Clone, Debug, PartialEq)]
pub struct Tween<T> {
    /// Where it starts.
    pub from: T,
    /// Where it ends.
    pub to: T,
    /// When it starts, in ms.
    pub start_ms: u64,
    /// How long it takes, in ms.
    pub ms: u64,
}
