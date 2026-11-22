//! What the pointer is on in a graph: a node first, then an edge (briefs/mouse-ux.md §3.4).

use super::GraphLayout;
use super::GraphModel;
use super::geom::RING;
use crate::interact::Cursor;
use crate::interact::consts::{HIT_EDGE, HIT_NODE_PAD, HIT_POINT};

/// A node or an edge, by its index in the model.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Target {
    /// A node.
    Node(u32),
    /// An edge.
    Edge(u32),
}

impl Target {
    /// The node, if it's one.
    pub fn node(self) -> Option<usize> {
        match self {
            Target::Node(i) => Some(i as usize),
            Target::Edge(_) => None,
        }
    }

    /// The edge, if it's one.
    pub fn edge(self) -> Option<usize> {
        match self {
            Target::Edge(k) => Some(k as usize),
            Target::Node(_) => None,
        }
    }

    /// Where a click on it puts the cursor: on a node, the node; on an edge, its end nearer
    /// `at` (briefs/mouse-ux.md §4.5).
    pub fn cursor(
        self,
        model: &GraphModel,
        layout: &GraphLayout,
        at: (f32, f32),
    ) -> Option<Cursor> {
        match self {
            Target::Node(i) => Some(model.cursor(i as usize)),
            Target::Edge(k) => {
                let (a, b) = *model.ends.get(k as usize)?;
                let d = |i: u32| {
                    layout
                        .node(i as usize)
                        .map_or(f32::INFINITY, |g| (g.at.0 - at.0).hypot(g.at.1 - at.1))
                };
                let near = if d(b) < d(a) { b } else { a };
                Some(model.cursor(near as usize))
            }
        }
    }
}

/// What the pointer at (x, y) is on: the node whose drawn disc holds it, the topmost (the
/// last drawn); else the node nearest within its radius and [`HIT_NODE_PAD`] (at least
/// [`HIT_POINT`] from its centre); else the edge nearest within [`HIT_EDGE`], the last drawn of
/// equals. None off the graph's region. A linear pass over the nodes, `O(n)`, and the hit
/// index's over the edges' segments.
pub fn hover(_model: &GraphModel, layout: &GraphLayout, x: f32, y: f32) -> Option<Target> {
    if !layout.in_region(x, y) {
        return None;
    }
    let mut best: Option<(f32, usize)> = None;
    for (i, g) in layout.nodes.iter().enumerate().rev() {
        let Some(g) = g else { continue };
        let d = (g.at.0 - x).hypot(g.at.1 - y);
        if d <= g.r + RING * 0.5 {
            return Some(Target::Node(i as u32));
        }
        let reach = (g.r + HIT_NODE_PAD).max(HIT_POINT);
        let past = d - g.r;
        if d <= reach && best.is_none_or(|(b, _)| past < b) {
            best = Some((past, i));
        }
    }
    if let Some((_, i)) = best {
        return Some(Target::Node(i as u32));
    }
    layout
        .hits
        .nearest_segment(x, y, HIT_EDGE)
        .map(Target::Edge)
}
