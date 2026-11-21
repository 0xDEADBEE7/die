//! The pointer's constants (briefs/mouse-ux.md §2.1), in logical px unless named in ms.

/// How far a press travels before it's a drag; a release inside it is a click.
pub const CLICK_SLOP: f32 = 4.0;
/// The radius that finds a point or a line's dot: the design's 24 px target.
pub const HIT_POINT: f32 = 12.0;
/// The furthest a nearest-point search reaches on a dense scatter, and the grid hash's cell.
pub const HIT_DENSE: f32 = 32.0;
/// Added to a node's radius to find it; never less than [`HIT_POINT`] in all.
pub const HIT_NODE_PAD: f32 = 4.0;
/// How near an edge's path the pointer finds it.
pub const HIT_EDGE: f32 = 4.0;
/// The furthest y distance at which hover emphasises the nearest line.
pub const NEAREST_SERIES: f32 = 24.0;
/// From the crosshair or point to the card.
pub const CARD_GAP: f32 = 12.0;
/// How far past an edge the card goes before it flips sides, in cell widths.
pub const CARD_HYSTERESIS_CELLS: f32 = 1.0;
/// How long after the press that focuses a window its drags still do nothing, as the
/// whiteboard's.
pub const FOCUS_GRACE_MS: u64 = 150;
/// The quiet time that ends a scroll burst: a burst latches its mode and target, and is one
/// undo step.
pub const SCROLL_BURST_MS: u64 = 200;
/// How long a graph's hover waits before it dims the rest, and before it clears.
pub const DWELL_EMPHASIS_MS: u64 = 100;
/// How long before a truncated tick, legend name or title shows in full.
pub const DWELL_TRUNCATED_MS: u64 = 400;
/// How long a discrete change eases, ease-out cubic.
pub const MOTION_MS: u64 = 180;
/// One zoom step, the whiteboard's; the wheel zooms by `ZOOM_STEP.powf(-dy / 4.0)`, so scrolling
/// up zooms in.
pub const ZOOM_STEP: f64 = 1.25;
/// Below this in one dimension, a box brush collapses to the other axis.
pub const BRUSH_MIN: f32 = 8.0;
