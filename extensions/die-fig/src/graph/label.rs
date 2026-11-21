//! Label placement: each node's name beside it, on the first free side.
//!
//! The sides are tried in order: right, left, below, above, then the four corners between
//! them. A side is free when its label crosses no other label, no node (its ring included) and
//! no edge, stays inside the graph's region, and is nearer its own node than any other (so it
//! never reads as another's). Labels are placed busiest node first (degree,
//! then node order), so where there's room for only some, the hubs keep theirs; one with no
//! free side but a side that only crosses a few edges (two, or a hub's own count up to six)
//! goes there on a ground of the surface's colour, which parts the edges under it; one that
//! would cover a label or a node is culled. As the
//! view zooms in, nodes spread and more labels find room. `show-label: true` labels go first
//! and are never culled (on the least crowded side where none is free); `false` ones never
//! show.

use super::GraphModel;
use super::geom::{GraphLayout, RING};
use crate::interact::View;
use crate::scene::{Anchor, Measure, Rect, TextSpec, Weight};

/// A label's size at rest.
pub const LABEL: f32 = 11.5;
/// The focused node's label's size (semibold).
pub const LABEL_FOCUS: f32 = 12.5;
/// From a node's ring to its label beside it.
pub const LABEL_GAP: f32 = 5.0;
/// The least room kept between two labels.
const LABEL_CLEAR: f32 = 2.0;
/// The grid's cell, in px.
const CELL: f32 = 32.0;
/// The most edges a label at rest crosses (on its ground) rather than be culled; a hub's may
/// cross as many as it has edges, up to six.
const BASE_EDGES: u32 = 2;
/// A node with this many edges or more is a hub: its label may go further out.
const HUB: u32 = 4;
/// How far round a label the nodes are looked at that could seem to own it.
const ASSOCIATION: f32 = 30.0;

/// A label at rest's type.
pub fn label_spec() -> TextSpec {
    TextSpec::new(LABEL)
}

/// The focused node's label's type.
pub fn focus_spec() -> TextSpec {
    TextSpec::new(LABEL_FOCUS).weight(Weight::Semibold)
}

/// A label placed beside its node.
#[derive(Clone, Debug, PartialEq)]
pub struct NodeLabel {
    /// What it says, cut to fit.
    pub text: String,
    /// Its anchor's x.
    pub x: f32,
    /// Its baseline.
    pub y: f32,
    /// Which end of it is at x.
    pub anchor: Anchor,
    /// Its type.
    pub spec: TextSpec,
    /// The box it covers (its cap height down to its descent), for collisions.
    pub rect: Rect,
    /// Whether it crosses an edge, and so sits on a ground of the surface's colour.
    pub backed: bool,
    /// Which side of its node it's on, from 0: right, left, below, above, then the corners.
    pub side: u8,
}

/// How bad a spot is: free, over edges only (how many), or over a label or a node.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Clash {
    Free,
    Edges(u32),
    /// Over a label or a node (100 each) or an edge drawn bright (1 each).
    Blocked(u32),
}

/// What labels must keep clear of, in a grid over the area.
pub(crate) struct Field {
    cols: i32,
    rows: i32,
    bounds: Rect,
    header: f32,
    labels: Vec<Rect>,
    soft: Vec<Rect>,
    nodes: Vec<((f32, f32), f32, u32)>,
    /// Nodes a label may lie over on its ground (dimmed under the wash), by node index.
    soft_nodes: Vec<bool>,
    /// Each drawn node's centre and reach, by node index.
    node_at: Vec<Option<((f32, f32), f32)>>,
    segs: Vec<((f32, f32), (f32, f32))>,
    hard: Vec<((f32, f32), (f32, f32))>,
    cell_labels: Vec<Vec<u32>>,
    cell_soft: Vec<Vec<u32>>,
    cell_nodes: Vec<Vec<u32>>,
    cell_segs: Vec<Vec<u32>>,
    cell_hard: Vec<Vec<u32>>,
    stamp: Vec<u32>,
    query: u32,
}

impl Field {
    /// A field over a layout's area: its nodes and edges, no labels yet.
    pub(crate) fn of(layout: &GraphLayout) -> Field {
        let (w, h) = layout.size;
        let cols = (w / CELL).ceil().max(1.0) as i32;
        let rows = (h / CELL).ceil().max(1.0) as i32;
        let cells = (cols * rows) as usize;
        let mut f = Field {
            cols,
            rows,
            bounds: layout.bounds,
            header: layout.region.y,
            labels: Vec::new(),
            soft: Vec::new(),
            nodes: Vec::new(),
            segs: Vec::new(),
            hard: Vec::new(),
            soft_nodes: vec![false; layout.nodes.len()],
            node_at: vec![None; layout.nodes.len()],
            cell_labels: vec![Vec::new(); cells],
            cell_soft: vec![Vec::new(); cells],
            cell_nodes: vec![Vec::new(); cells],
            cell_segs: vec![Vec::new(); cells],
            cell_hard: vec![Vec::new(); cells],
            stamp: Vec::new(),
            query: 0,
        };
        for (i, n) in layout.nodes.iter().enumerate() {
            if let Some(n) = n {
                let reach = n.r + RING + 1.0;
                f.node_at[i] = Some((n.at, reach));
                let r = Rect::new(n.at.0 - reach, n.at.1 - reach, 2.0 * reach, 2.0 * reach);
                if r.intersect(&f.bounds).is_none() {
                    continue;
                }
                let id = f.nodes.len() as u32;
                f.nodes.push((n.at, reach, i as u32));
                for c in f.cells(r) {
                    f.cell_nodes[c].push(id);
                }
            }
        }
        for e in layout.edges.iter().flatten() {
            let mut pts = e.pts.clone();
            if let Some(a) = e.arrow {
                pts.push(a[0]);
            }
            for s in pts.windows(2) {
                let r = seg_box(s[0], s[1]).inflate(e.width / 2.0);
                if r.intersect(&f.bounds).is_none() {
                    continue;
                }
                let id = f.segs.len() as u32;
                f.segs.push((s[0], s[1]));
                for c in f.cells(r) {
                    f.cell_segs[c].push(id);
                }
            }
        }
        f
    }

    /// The cells a rectangle touches, within the grid.
    fn cells(&self, r: Rect) -> Vec<usize> {
        let c0 = ((r.x / CELL).floor() as i32).clamp(0, self.cols - 1);
        let c1 = ((r.right() / CELL).floor() as i32).clamp(0, self.cols - 1);
        let r0 = ((r.y / CELL).floor() as i32).clamp(0, self.rows - 1);
        let r1 = ((r.bottom() / CELL).floor() as i32).clamp(0, self.rows - 1);
        let mut out = Vec::with_capacity(((c1 - c0 + 1) * (r1 - r0 + 1)) as usize);
        for row in r0..=r1 {
            for col in c0..=c1 {
                out.push((row * self.cols + col) as usize);
            }
        }
        out
    }

    /// A label now kept clear of.
    pub(crate) fn add_label(&mut self, r: Rect) {
        let id = self.labels.len() as u32;
        self.labels.push(r);
        for c in self.cells(r) {
            self.cell_labels[c].push(id);
        }
    }

    /// An edge's segment a label may not cross at all: an emphasised edge, which a label's
    /// ground would cut.
    pub(crate) fn add_hard(&mut self, a: (f32, f32), b: (f32, f32)) {
        let r = seg_box(a, b).inflate(1.0);
        let id = self.hard.len() as u32;
        self.hard.push((a, b));
        for c in self.cells(r) {
            self.cell_hard[c].push(id);
        }
    }

    /// A label best kept clear of, but that may be covered on a ground (a dimmed label under
    /// the emphasis wash).
    pub(crate) fn add_soft(&mut self, r: Rect) {
        let id = self.soft.len() as u32;
        self.soft.push(r);
        for c in self.cells(r) {
            self.cell_soft[c].push(id);
        }
    }

    /// Lets labels lie over the nodes `soft` says, on their ground: nodes dimmed under the
    /// emphasis wash.
    pub(crate) fn soften_nodes(&mut self, soft: impl Fn(usize) -> bool) {
        for (i, s) in self.soft_nodes.iter_mut().enumerate() {
            *s = soft(i);
        }
    }

    /// The soft labels a box overlaps, to clear under it.
    pub(crate) fn soft_under(&self, r: Rect) -> Vec<Rect> {
        let spaced = r.inflate(LABEL_CLEAR);
        let mut out: Vec<Rect> = Vec::new();
        for c in self.cells(r) {
            for &l in &self.cell_soft[c] {
                let s = self.soft[l as usize];
                if overlaps(&spaced, &s) && !out.contains(&s) {
                    out.push(s);
                }
            }
        }
        out
    }

    /// How a box at a spot clashes, `own` the node it labels.
    fn clash(&mut self, r: Rect, own: usize) -> Clash {
        let b = self.bounds;
        if r.x < b.x
            || r.right() > b.right()
            || r.y < b.y.max(self.header)
            || r.bottom() > b.bottom()
        {
            return Clash::Blocked(u32::MAX);
        }
        let cells = self.cells(r);
        let spaced = r.inflate(LABEL_CLEAR);
        let mut blocked = 0u32;
        for &c in &cells {
            for &l in &self.cell_labels[c] {
                if overlaps(&spaced, &self.labels[l as usize]) {
                    blocked += 100;
                }
            }
            for &k in &self.cell_nodes[c] {
                let (at, reach, i) = self.nodes[k as usize];
                if self.soft_nodes[i as usize] {
                    continue;
                }
                if circle_meets(at, reach, &r)
                    && (i as usize != own || circle_meets(at, reach - 1.0, &r))
                {
                    blocked += 100;
                }
            }
        }
        // A label must read as its own node's: no other node nearer it.
        if let Some(Some((own_at, own_reach))) = self.node_at.get(own) {
            let mine = rect_distance(*own_at, &r) - own_reach;
            for c in self.cells(r.inflate(ASSOCIATION)) {
                for &k in &self.cell_nodes[c] {
                    let (at, reach, i) = self.nodes[k as usize];
                    // A dimmed node doesn't compete with a bright one for its label.
                    if i as usize != own
                        && !self.soft_nodes[i as usize]
                        && rect_distance(at, &r) - reach + 1.0 < mine
                    {
                        blocked += 100;
                    }
                }
            }
        }
        let near = r.inflate(1.5);
        for &c in &cells {
            for &k in &self.cell_hard[c] {
                let (a, z) = self.hard[k as usize];
                if seg_meets(a, z, &near) {
                    blocked += 1;
                }
            }
        }
        if blocked > 0 {
            return Clash::Blocked(blocked);
        }
        self.query = self.query.wrapping_add(1);
        if self.stamp.len() < self.segs.len().max(self.soft.len()) {
            self.stamp.resize(self.segs.len().max(self.soft.len()), 0);
        }
        let mut crossed = 0u32;
        for &c in &cells {
            for &s in &self.cell_segs[c] {
                if self.stamp[s as usize] == self.query {
                    continue;
                }
                self.stamp[s as usize] = self.query;
                let (a, z) = self.segs[s as usize];
                if seg_meets(a, z, &near) {
                    crossed += 1;
                }
            }
        }
        self.query = self.query.wrapping_add(1);
        for &c in &cells {
            for &l in &self.cell_soft[c] {
                if self.stamp[l as usize] == self.query {
                    continue;
                }
                self.stamp[l as usize] = self.query;
                if overlaps(&spaced, &self.soft[l as usize]) {
                    crossed += 1;
                }
            }
        }
        let mut nodes_under: Vec<u32> = Vec::new();
        for &c in &cells {
            for &k in &self.cell_nodes[c] {
                let (at, reach, i) = self.nodes[k as usize];
                if self.soft_nodes[i as usize]
                    && !nodes_under.contains(&k)
                    && circle_meets(at, reach, &r)
                {
                    nodes_under.push(k);
                }
            }
        }
        crossed += nodes_under.len() as u32;
        if crossed > 0 {
            Clash::Edges(crossed)
        } else {
            Clash::Free
        }
    }
}

/// How far a point is from a rectangle (0 inside it).
fn rect_distance(c: (f32, f32), rect: &Rect) -> f32 {
    let nx = c.0.clamp(rect.x, rect.right());
    let ny = c.1.clamp(rect.y, rect.bottom());
    (c.0 - nx).hypot(c.1 - ny)
}

fn overlaps(a: &Rect, b: &Rect) -> bool {
    a.x < b.right() && b.x < a.right() && a.y < b.bottom() && b.y < a.bottom()
}

fn seg_box(a: (f32, f32), b: (f32, f32)) -> Rect {
    let (x0, x1) = (a.0.min(b.0), a.0.max(b.0));
    let (y0, y1) = (a.1.min(b.1), a.1.max(b.1));
    Rect::new(x0, y0, x1 - x0, y1 - y0)
}

/// Whether a circle meets a rectangle.
fn circle_meets(c: (f32, f32), r: f32, rect: &Rect) -> bool {
    let nx = c.0.clamp(rect.x, rect.right());
    let ny = c.1.clamp(rect.y, rect.bottom());
    (c.0 - nx).hypot(c.1 - ny) < r
}

/// Whether a segment meets a rectangle (Liang–Barsky).
pub(crate) fn seg_meets(a: (f32, f32), b: (f32, f32), r: &Rect) -> bool {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let (mut t0, mut t1) = (0.0f32, 1.0f32);
    for (p, q) in [
        (-dx, a.0 - r.x),
        (dx, r.right() - a.0),
        (-dy, a.1 - r.y),
        (dy, r.bottom() - a.1),
    ] {
        if p == 0.0 {
            if q < 0.0 {
                return false;
            }
        } else {
            let t = q / p;
            if p < 0.0 {
                t0 = t0.max(t);
            } else {
                t1 = t1.min(t);
            }
            if t0 > t1 {
                return false;
            }
        }
    }
    true
}

/// The spots beside a node of radius `r` (its ring outside it) at `c`, in the order tried,
/// for a text `w` wide in `spec`, `gap` from the ring.
pub(crate) fn spots(
    text: &str,
    c: (f32, f32),
    r: f32,
    gap: f32,
    spec: TextSpec,
    m: &mut dyn Measure,
) -> Vec<NodeLabel> {
    let w = m.width(text, spec);
    let cap = m.cap(spec);
    let d = m.descent(spec);
    let edge = r + RING * 0.5 + gap;
    let diag = (r + RING * 0.5) * 0.72 + gap * 0.6;
    let mk = |x: f32, y: f32, anchor: Anchor, side: u8| {
        let left = match anchor {
            Anchor::Start => x,
            Anchor::Middle => x - w / 2.0,
            Anchor::End => x - w,
        };
        NodeLabel {
            text: text.to_string(),
            x,
            y,
            anchor,
            spec,
            rect: Rect::new(left, y - cap - 2.0, w, cap + 2.0 + d),
            backed: false,
            side,
        }
    };
    let mid = c.1 + cap / 2.0;
    vec![
        mk(c.0 + edge, mid, Anchor::Start, 0),
        mk(c.0 - edge, mid, Anchor::End, 1),
        mk(c.0, c.1 + edge - 1.0 + cap + 1.0, Anchor::Middle, 2),
        mk(c.0, c.1 - edge - d, Anchor::Middle, 3),
        mk(c.0 + diag, c.1 + diag + cap, Anchor::Start, 4),
        mk(c.0 + diag, c.1 - diag - d, Anchor::Start, 5),
        mk(c.0 - diag, c.1 + diag + cap, Anchor::End, 6),
        mk(c.0 - diag, c.1 - diag - d, Anchor::End, 7),
    ]
}

/// Spots further out than [`spots`]'s, two rings of them, for a label that must show.
pub(crate) fn further(
    text: &str,
    c: (f32, f32),
    r: f32,
    gap: f32,
    spec: TextSpec,
    m: &mut dyn Measure,
) -> Vec<NodeLabel> {
    let mut out = spots(text, c, r, gap + 10.0, spec, m);
    out.extend(spots(text, c, r, gap + 24.0, spec, m));
    out
}

/// The best of `spots` for node `own`: the first free; else the one crossing fewest edges
/// (backed), when that's at most `edges_ok` or the label is forced (`block_ok` above 0); else
/// the least blocked (backed), when its score is at most `block_ok` (100 a label or a node
/// covered, 1 an edge drawn bright; `u32::MAX` takes the least bad, even outside the region).
/// None to cull it.
pub(crate) fn choose(
    field: &mut Field,
    spots: Vec<NodeLabel>,
    own: usize,
    edges_ok: u32,
    block_ok: u32,
) -> Option<NodeLabel> {
    let forced = block_ok > 0;
    let mut best: Option<(Clash, NodeLabel)> = None;
    for s in spots {
        let c = field.clash(s.rect, own);
        if c == Clash::Free {
            return Some(s);
        }
        if best.as_ref().is_none_or(|(b, _)| c < *b) {
            best = Some((c, s));
        }
    }
    let (c, mut s) = best?;
    match c {
        Clash::Edges(n) if n <= edges_ok || forced => {
            s.backed = true;
            Some(s)
        }
        Clash::Blocked(n) if n <= block_ok => {
            s.backed = true;
            Some(s)
        }
        _ => None,
    }
}

/// Every node's label at rest: forced ones first, then by degree (most first) and node order;
/// culled where it can't be placed clear. With labels off, only `show-label: true` ones.
pub(crate) fn place(
    model: &GraphModel,
    _view: &View,
    layout: &GraphLayout,
    m: &mut dyn Measure,
) -> Vec<Option<NodeLabel>> {
    let n = model.len();
    let mut out = vec![None; n];
    let mut order: Vec<usize> = (0..n)
        .filter(|&i| layout.node(i).is_some() && model.show_label(i) != Some(false))
        .filter(|&i| model.labels_on() || model.show_label(i) == Some(true))
        .collect();
    order.sort_by_key(|&i| {
        (
            model.show_label(i) != Some(true),
            std::cmp::Reverse(model.degree(i)),
            i,
        )
    });
    if order.is_empty() {
        return out;
    }
    let mut field = Field::of(layout);
    let spec = label_spec();
    let max_w = (layout.size.0 * 0.4).clamp(40.0, 240.0);
    for i in order {
        let g = layout.node(i).expect("filtered to drawn nodes");
        if !layout.bounds.contains(g.at.0, g.at.1) {
            continue;
        }
        let text = m.fit(model.label(i), spec, max_w);
        let forced = model.show_label(i) == Some(true);
        let mut cands = spots(&text, g.at, g.r, LABEL_GAP, spec, m);
        // A forced label, and a hub's, may go a little further out to find room.
        if forced || model.degree(i) >= HUB {
            cands.extend(further(&text, g.at, g.r, LABEL_GAP, spec, m));
        }
        let block_ok = if forced { u32::MAX } else { 0 };
        // A hub's label crosses some of its own edges wherever it goes: it may cross as many
        // as it has (up to six) before it's culled.
        let edges_ok = BASE_EDGES.max(model.degree(i).min(6));
        if let Some(l) = choose(&mut field, cands, i, edges_ok, block_ok) {
            field.add_label(l.rect);
            out[i] = Some(l);
        }
    }
    out
}
