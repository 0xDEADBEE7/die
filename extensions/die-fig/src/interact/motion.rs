//! Easing for discrete changes: keys, legend clicks, a brush's release, undo and a double-click
//! reset ease over [`super::consts::MOTION_MS`]; the wheel, the trackpad and a drag follow the
//! hand and never ease.

/// Ease-out cubic: fast, then settling. `t` from 0 to 1, clamped; NaN is the start.
pub fn ease_out_cubic(t: f32) -> f32 {
    let t = if t.is_nan() { 0.0 } else { t.clamp(0.0, 1.0) };
    1.0 - (1.0 - t).powi(3)
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

impl<T> Tween<T> {
    /// A tween from `from` to `to` starting at `start_ms`, `ms` long.
    pub fn new(from: T, to: T, start_ms: u64, ms: u64) -> Tween<T> {
        Tween {
            from,
            to,
            start_ms,
            ms,
        }
    }

    /// How far along it is at `now_ms`, eased: 0 at the start, 1 from its end on. A tween of
    /// no length is at its end at once.
    pub fn progress(&self, now_ms: u64) -> f32 {
        if self.ms == 0 {
            return 1.0;
        }
        let t = now_ms.saturating_sub(self.start_ms) as f32 / self.ms as f32;
        ease_out_cubic(t)
    }

    /// Whether it has reached its end at `now_ms`.
    pub fn done(&self, now_ms: u64) -> bool {
        now_ms >= self.start_ms.saturating_add(self.ms)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_ease_runs_from_nought_to_one_fast_then_slow() {
        assert_eq!(ease_out_cubic(0.0), 0.0);
        assert_eq!(ease_out_cubic(1.0), 1.0);
        assert_eq!(ease_out_cubic(-1.0), 0.0);
        assert_eq!(ease_out_cubic(2.0), 1.0);
        assert_eq!(ease_out_cubic(f32::NAN), 0.0);
        assert!((ease_out_cubic(0.5) - 0.875).abs() < 1e-6);
        let t = Tween::new(0, 1, 100, 180);
        assert_eq!(t.progress(100), 0.0);
        assert_eq!(t.progress(50), 0.0);
        assert!(!t.done(279) && t.done(280));
        assert_eq!(t.progress(280), 1.0);
        assert_eq!(Tween::new(0, 1, 5, 0).progress(0), 1.0);
    }
}
