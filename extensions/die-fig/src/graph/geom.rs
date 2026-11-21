//! Where a graph lands in an area: the header (title, subtitle, the groups' legend), the frame
//! from layout units to px, each node's place and size, and each edge's path.
//!
//! A stress layout keeps one scale across and up, so distances read true; a tree or a layered
//! layout fits its ranks and its slots each to the room, within [`UNIT_MAX`]. At fit the graph's
//! box sits inside the plot with room for the largest node and half the widest label at each
//! side, so the outermost labels can go beside, under or over their nodes without leaving the
//! area. Edges are flattened to polylines in px: straight for stress (bowed apart where two
//! join the same nodes), S-curves between ranks, elbows down to a stacked column, and smooth
//! curves through a layered layout's bends; a directed edge stops at its arrowhead, whose tip
//! touches the ring of the node it points at.

use super::GraphModel;
use super::label::NodeLabel;
use crate::algo::HitIndex;
use crate::data::SeriesKey;
use crate::interact::View;
use crate::plot::layout::{BODY, Ink, KeyShape, Label, LegendItem, Role, TITLE, paddings};
use crate::scene::{Anchor, Measure, Rect, TextSpec, Weight};
use crate::spec::Layout;
use crate::theme::{Color, Theme};
use std::cell::Cell;

/// The most px a tree's or a layered layout's unit takes, across and up: a small tree stays
/// compact rather than spreading over a wide area.
pub const UNIT_MAX: (f64, f64) = (170.0, 96.0);
/// A legend row's height.
const ROW: f32 = 20.0;
/// A legend key's slot and the gap to its name, and between entries.
const KEY_W: f32 = 14.0;
const KEY_GAP: f32 = 6.0;
const ITEM_GAP: f32 = 18.0;
/// The arrowhead's length and half its width, for a 1 px edge.
pub const ARROW: (f32, f32) = (7.0, 3.4);
/// How far apart edges between the same two nodes bow, at their middles.
const LANE: f32 = 14.0;
/// The ring a node wears in the surface's colour, past its radius.
pub const RING: f32 = 2.0;

/// The map from layout units (y up) to px in the area, and back.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    /// The rectangle the window onto the layout fills.
    pub plot: Rect,
    /// The layout point at the plot's middle.
    pub centre: (f64, f64),
    /// Px per layout unit, across and up.
    pub k: (f64, f64),
}

impl Frame {
    /// The frame showing domains `x` and `y` in `plot`: one scale both ways when `uniform`
    /// (the smaller, the domains centred), else each axis its own.
    pub fn new(plot: Rect, x: (f64, f64), y: (f64, f64), uniform: bool) -> Frame {
        let span = |d: (f64, f64)| (d.1 - d.0).abs().max(1e-9);
        let kx = f64::from(plot.w.max(1.0)) / span(x);
        let ky = f64::from(plot.h.max(1.0)) / span(y);
        let k = if uniform {
            let k = kx.min(ky);
            (k, k)
        } else {
            (kx, ky)
        };
        Frame {
            plot,
            centre: ((x.0 + x.1) / 2.0, (y.0 + y.1) / 2.0),
            k,
        }
    }

    /// A layout point's place in px.
    pub fn px(&self, p: (f64, f64)) -> (f32, f32) {
        let mx = f64::from(self.plot.x) + f64::from(self.plot.w) / 2.0;
        let my = f64::from(self.plot.y) + f64::from(self.plot.h) / 2.0;
        (
            (mx + (p.0 - self.centre.0) * self.k.0) as f32,
            (my - (p.1 - self.centre.1) * self.k.1) as f32,
        )
    }

    /// The layout point at a place in px.
    pub fn unit(&self, q: (f32, f32)) -> (f64, f64) {
        let mx = f64::from(self.plot.x) + f64::from(self.plot.w) / 2.0;
        let my = f64::from(self.plot.y) + f64::from(self.plot.h) / 2.0;
        (
            self.centre.0 + (f64::from(q.0) - mx) / self.k.0,
            self.centre.1 - (f64::from(q.1) - my) / self.k.1,
        )
    }

    /// The domains the plot shows, across and up.
    pub fn domain(&self) -> ((f64, f64), (f64, f64)) {
        let hx = f64::from(self.plot.w) / 2.0 / self.k.0;
        let hy = f64::from(self.plot.h) / 2.0 / self.k.1;
        (
            (self.centre.0 - hx, self.centre.0 + hx),
            (self.centre.1 - hy, self.centre.1 + hy),
        )
    }
}

/// A node as drawn.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NodeGeom {
    /// Its centre in px.
    pub at: (f32, f32),
    /// Its radius in px, inside its ring.
    pub r: f32,
    /// Its fill.
    pub color: Color,
    /// Whether it's pinned (moved by hand).
    pub pinned: bool,
}

/// An edge as drawn.
#[derive(Clone, Debug, PartialEq)]
pub struct EdgeGeom {
    /// Its path in px, flattened, from its `from` end (shortened to the arrowhead's base for a
    /// directed edge).
    pub pts: Vec<(f32, f32)>,
    /// Its width in px; 1 is a hairline.
    pub width: f32,
    /// A directed edge's arrowhead: its tip, then its two back corners.
    pub arrow: Option<[(f32, f32); 3]>,
    /// Its colour at rest.
    pub color: Color,
    /// Whether it's dashed (its `fig` says so).
    pub dash: bool,
}

/// Where everything landed: the header, the frame, every node, edge and label.
pub struct GraphLayout {
    /// The area's size in logical px.
    pub size: (f32, f32),
    /// Physical px to a logical px.
    pub scale: f32,
    /// The rectangle the view's window onto the layout fills.
    pub plot: Rect,
    /// Layout units to px.
    pub frame: Frame,
    /// The domains shown, across and up, in layout units.
    pub domain: ((f64, f64), (f64, f64)),
    /// The domains that fit the shown nodes under the view: what zoom and pan are limited by.
    pub fit: ((f64, f64), (f64, f64)),
    /// Each node as drawn; None where it's hidden.
    pub nodes: Vec<Option<NodeGeom>>,
    /// Each edge as drawn; None where it or an end is hidden.
    pub edges: Vec<Option<EdgeGeom>>,
    /// Each node's label where the base placed one.
    pub labels: Vec<Option<NodeLabel>>,
    /// Each legend entry's rectangle, its key and name inflated 2 px.
    pub legend: Vec<(SeriesKey, Rect)>,
    /// The legend's entries.
    pub items: Vec<LegendItem>,
    /// The header's text: title, subtitle, the legend's "+N more".
    pub chrome: Vec<Label>,
    /// The title and subtitle's box.
    pub title: Rect,
    /// The graph's region: the area under the header, where it's drawn and clipped, and what
    /// the emphasis wash covers.
    pub region: Rect,
    /// Where labels and the card may go: the region, a couple of px in from the area's edge.
    pub bounds: Rect,
    /// A cell's width: how far past the area's edge the card goes before it flips back. 10
    /// unless the surface says.
    pub cell_width: f32,
    /// Whether the card was last put left of its anchor, for its hysteresis.
    pub(crate) flipped: Cell<bool>,
    /// The edges' segments, for the pointer: each segment's id is its edge's index.
    pub hits: HitIndex,
}

impl GraphLayout {
    /// Node `i`'s geometry, if it's drawn.
    pub fn node(&self, i: usize) -> Option<&NodeGeom> {
        self.nodes.get(i).and_then(Option::as_ref)
    }

    /// Whether a place in px is in the graph's region.
    pub fn in_region(&self, x: f32, y: f32) -> bool {
        self.region.contains(x, y)
    }
}

/// What the header keeps at a size.
#[derive(Clone, Copy)]
struct Keep {
    title: bool,
    subtitle: bool,
    legend_rows: usize,
}

/// The layout of `model` under `view` at `size`: the header, the frame, the nodes and the
/// edges (labels are placed after, by [`super::label::place`]).
pub(crate) fn lay_out(
    model: &GraphModel,
    view: &View,
    size: (f32, f32),
    scale: f32,
    theme: &Theme,
    m: &mut dyn Measure,
) -> GraphLayout {
    let levels = [
        Keep {
            title: true,
            subtitle: true,
            legend_rows: 2,
        },
        Keep {
            title: true,
            subtitle: false,
            legend_rows: 1,
        },
        Keep {
            title: true,
            subtitle: false,
            legend_rows: 0,
        },
        Keep {
            title: false,
            subtitle: false,
            legend_rows: 0,
        },
    ];
    let (w, h) = size;
    let mut chosen = None;
    for keep in levels {
        let hd = header(model, view, size, keep, m);
        if hd.region.h >= (0.6 * h).max(90.0).min(h * 0.85) {
            chosen = Some(hd);
            break;
        }
        chosen = Some(hd);
    }
    let hd = chosen.expect("there are levels");
    let (pad, _, pad_bottom) = paddings(w, h);
    let region = hd.region;
    let plot = Rect::new(
        pad,
        region.y + 2.0,
        (w - 2.0 * pad).max(1.0),
        (region.bottom() - pad_bottom - region.y - 2.0).max(1.0),
    );
    let fit = fit(model, view, plot, m);
    let x = view.x.unwrap_or(fit.0);
    let y = view.y.unwrap_or(fit.1);
    let frame = Frame::new(plot, x, y, model.uniform());
    let domain = frame.domain();
    let mut nodes: Vec<Option<NodeGeom>> = (0..model.len())
        .map(|i| {
            model.visible(view, i).then(|| NodeGeom {
                at: frame.px(model.position(view, i)),
                r: model.radius[i],
                color: model.color(i, theme),
                pinned: model.pinned(view, i),
            })
        })
        .collect();
    let shrink = crowding(&nodes);
    if shrink < 1.0 {
        for g in nodes.iter_mut().flatten() {
            g.r = (g.r * shrink).max(SMALLEST);
        }
    }
    let mut hits = HitIndex::new();
    let edges: Vec<Option<EdgeGeom>> = (0..model.ends.len())
        .map(|k| {
            if !model.edge_visible(view, k) {
                return None;
            }
            let g = edge_geom(model, view, &frame, &nodes, k, theme)?;
            for s in g.pts.windows(2) {
                hits.add_segment(k as u32, s[0], s[1]);
            }
            if let Some(a) = g.arrow {
                hits.add_segment(k as u32, *g.pts.last().unwrap_or(&a[0]), a[0]);
            }
            Some(g)
        })
        .collect();
    let bounds = Rect::new(2.0, region.y, (w - 4.0).max(1.0), (region.h - 2.0).max(1.0));
    GraphLayout {
        size,
        scale,
        plot,
        frame,
        domain,
        fit,
        nodes,
        edges,
        labels: vec![None; model.len()],
        legend: hd.legend,
        items: hd.items,
        chrome: hd.chrome,
        title: hd.title,
        region,
        bounds,
        cell_width: 10.0,
        flipped: Cell::new(false),
        hits,
    }
}

/// The least a crowded node's radius shrinks to.
pub const SMALLEST: f32 = 2.6;

/// How much nodes shrink where they crowd at this zoom: 1 while the typical gap to a node's
/// nearest neighbour holds two typical nodes and a few px, down to 0.6 as it closes, so a dense
/// graph at fit reads as dots rather than a heap of discs. Zooming in spreads them back to
/// their size. The nearest neighbour is found in a grid of 24 px, past which a node counts as
/// clear.
fn crowding(nodes: &[Option<NodeGeom>]) -> f32 {
    const CELL: f32 = 24.0;
    let shown: Vec<&NodeGeom> = nodes.iter().flatten().collect();
    if shown.len() < 24 {
        return 1.0;
    }
    let mut cells: Vec<((i32, i32), usize)> = shown
        .iter()
        .enumerate()
        .map(|(i, g)| {
            (
                (
                    (g.at.0 / CELL).floor() as i32,
                    (g.at.1 / CELL).floor() as i32,
                ),
                i,
            )
        })
        .collect();
    cells.sort_unstable();
    let mut gaps: Vec<f32> = Vec::with_capacity(shown.len());
    for (i, g) in shown.iter().enumerate() {
        let c = (
            (g.at.0 / CELL).floor() as i32,
            (g.at.1 / CELL).floor() as i32,
        );
        let mut near = CELL;
        for dx in -1..=1 {
            for dy in -1..=1 {
                let key = (c.0 + dx, c.1 + dy);
                let from = cells.partition_point(|e| e.0 < key);
                for &(k, j) in &cells[from..] {
                    if k != key {
                        break;
                    }
                    if j != i {
                        let o = shown[j];
                        near = near.min((o.at.0 - g.at.0).hypot(o.at.1 - g.at.1));
                    }
                }
            }
        }
        gaps.push(near);
    }
    gaps.sort_by(f32::total_cmp);
    let mut radii: Vec<f32> = shown.iter().map(|g| g.r).collect();
    radii.sort_by(f32::total_cmp);
    let gap = gaps[gaps.len() / 2];
    let r = radii[radii.len() / 2].max(1.0);
    ((gap - 4.0) / (2.0 * r)).clamp(0.6, 1.0)
}

/// The header at a level: its text, the legend, and the region left under it.
struct Header {
    chrome: Vec<Label>,
    title: Rect,
    legend: Vec<(SeriesKey, Rect)>,
    items: Vec<LegendItem>,
    region: Rect,
}

fn header(
    model: &GraphModel,
    view: &View,
    size: (f32, f32),
    keep: Keep,
    m: &mut dyn Measure,
) -> Header {
    let (w, h) = size;
    let (pad, pad_top, _) = paddings(w, h);
    let title_spec = TextSpec::new(TITLE).weight(Weight::Semibold);
    let body = TextSpec::new(BODY);
    let wide = (w - 2.0 * pad).max(0.0);
    let mut chrome = Vec::new();
    let mut title = Rect::default();
    let mut y = pad_top;
    let mut any = false;
    let title_text = model.spec.title.as_deref().filter(|t| !t.is_empty());
    if keep.title
        && let Some(t) = title_text
    {
        let base = y + m.ascent(title_spec);
        let l = Label {
            role: Role::Title,
            text: m.fit(t, title_spec, wide),
            x: pad,
            y: base,
            spec: title_spec,
            anchor: Anchor::Start,
            ink: Ink::Strong,
            alpha: 1.0,
        };
        title = l.bounds(m);
        chrome.push(l);
        y = base + m.descent(title_spec);
        any = true;
    }
    let subtitle = match model.spec.subtitle.as_deref() {
        Some(s) => s.to_string(),
        None => model.summary(),
    };
    // With no title the summary would stand alone at the top: it shows only under one, or
    // when the spec gives a subtitle.
    let show_sub = keep.subtitle
        && !subtitle.is_empty()
        && (model.spec.subtitle.is_some() || (keep.title && title_text.is_some()));
    if show_sub {
        let base = if any {
            y + 20.0 - m.descent(title_spec)
        } else {
            y + m.ascent(body)
        };
        let l = Label {
            role: Role::Title,
            text: m.fit(&subtitle, body, wide),
            x: pad,
            y: base,
            spec: body,
            anchor: Anchor::Start,
            ink: Ink::Muted,
            alpha: 1.0,
        };
        let b = l.bounds(m);
        title = if any { title.union(&b) } else { b };
        chrome.push(l);
        y = base + m.descent(body);
        any = true;
    }
    if any {
        y += 10.0;
    }
    let mut legend = Vec::new();
    let mut items = Vec::new();
    if model.groups.len() >= 2 && keep.legend_rows > 0 {
        let max_name = (wide - KEY_W - KEY_GAP).clamp(0.0, 220.0);
        let entries: Vec<(SeriesKey, String, f32)> = (0..model.groups.len())
            .map(|g| {
                let name = m.fit(&model.groups[g], body, max_name);
                let iw = KEY_W + KEY_GAP + m.width(&name, body);
                (model.group_key(g), name, iw)
            })
            .collect();
        // Rows, left to right, leaving room for "+N more" on the last if not all fit.
        let mut rows: Vec<Vec<(usize, f32)>> = vec![Vec::new()];
        let mut x = pad;
        let mut placed = 0;
        for (i, (_, _, iw)) in entries.iter().enumerate() {
            if x + iw > w - pad && !rows.last().is_some_and(Vec::is_empty) {
                if rows.len() == keep.legend_rows {
                    break;
                }
                rows.push(Vec::new());
                x = pad;
            }
            if x + iw > w - pad {
                break;
            }
            rows.last_mut().expect("a row").push((i, x));
            x += iw + ITEM_GAP;
            placed += 1;
        }
        let mut more = entries.len() - placed;
        if more > 0 {
            // Make room on the last row for "+N more".
            loop {
                let last = rows.last().expect("a row");
                let end = last
                    .last()
                    .map_or(pad, |&(i, x)| x + entries[i].2 + ITEM_GAP);
                let tag = m.width(&format!("+{more} more"), body);
                if end + tag <= w - pad || last.is_empty() {
                    break;
                }
                rows.last_mut().expect("a row").pop();
                more += 1;
            }
        }
        let cap = m.cap(body);
        for (r, row) in rows.iter().enumerate() {
            let top = y + r as f32 * ROW;
            let mid = top + ROW / 2.0;
            for &(i, x) in row {
                let (key, name, iw) = &entries[i];
                let hidden = !view.shows(key);
                legend.push((key.clone(), Rect::new(x, top, *iw, ROW).inflate(2.0)));
                items.push(LegendItem {
                    key: key.clone(),
                    shape: KeyShape::Dot,
                    swatch: Rect::new(x + 2.5, mid - 4.5, 9.0, 9.0),
                    name: Label {
                        role: Role::Legend,
                        text: name.clone(),
                        x: x + KEY_W + KEY_GAP,
                        y: mid + cap / 2.0,
                        spec: body,
                        anchor: Anchor::Start,
                        ink: Ink::Text,
                        alpha: if hidden { 0.45 } else { 1.0 },
                    },
                    hidden,
                });
            }
        }
        if more > 0 {
            let r = rows.len() - 1;
            let end = rows[r]
                .last()
                .map_or(pad, |&(i, x)| x + entries[i].2 + ITEM_GAP);
            chrome.push(Label {
                role: Role::Legend,
                text: format!("+{more} more"),
                x: end,
                y: y + r as f32 * ROW + ROW / 2.0 + cap / 2.0,
                spec: body,
                anchor: Anchor::Start,
                ink: Ink::Muted,
                alpha: 1.0,
            });
        }
        y += rows.len() as f32 * ROW + 4.0;
    }
    let top = if any || !items.is_empty() { y } else { 0.0 };
    Header {
        chrome,
        title,
        legend,
        items,
        region: Rect::new(0.0, top, w, (h - top).max(1.0)),
    }
}

/// The domains that fit the shown nodes in `plot` with room at the sides for the largest node
/// and half the widest label (at most 80 px), and above and below for a label's line: one
/// scale both ways for stress, each axis its own for trees and layers within [`UNIT_MAX`].
/// The domains fill the plot, the nodes' box centred in it.
pub(crate) fn fit(
    model: &GraphModel,
    view: &View,
    plot: Rect,
    m: &mut dyn Measure,
) -> ((f64, f64), (f64, f64)) {
    let shown: Vec<usize> = (0..model.len())
        .filter(|&i| model.visible(view, i))
        .collect();
    if shown.is_empty() {
        return ((-1.0, 1.0), (-1.0, 1.0));
    }
    let mut b = (
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
    );
    let mut rmax: f32 = 0.0;
    for &i in &shown {
        let p = model.position(view, i);
        b = (b.0.min(p.0), b.1.max(p.0), b.2.min(p.1), b.3.max(p.1));
        rmax = rmax.max(model.radius[i]);
    }
    // A layered layout's long edges pass through slots beside the nodes: they're in the box.
    if matches!(model.layout, Layout::Layered) {
        for (k, bends) in model.bends.iter().enumerate() {
            let (a, z) = model.ends[k];
            if bends.is_empty()
                || !model.edge_visible(view, k)
                || model.pinned(view, a as usize)
                || model.pinned(view, z as usize)
            {
                continue;
            }
            for p in bends {
                b = (b.0.min(p.0), b.1.max(p.0), b.2.min(p.1), b.3.max(p.1));
            }
        }
    }
    let spec = super::label::label_spec();
    // The widest label at the far left and right is what needs room beside the nodes.
    let edge_w = |m: &mut dyn Measure, far: &dyn Fn(f64) -> bool| -> f32 {
        shown
            .iter()
            .filter(|&&i| far(model.position(view, i).0))
            .map(|&i| m.width(model.label(i), spec))
            .fold(0.0, f32::max)
    };
    let span_x = (b.1 - b.0).max(0.0);
    let near = span_x * 0.12;
    let (lo, hi) = (b.0, b.1);
    let left_w = edge_w(m, &|x| x <= lo + near);
    let right_w = edge_w(m, &|x| x >= hi - near);
    let label_room = (left_w.max(right_w) / 2.0).min(80.0);
    let mx = (rmax + RING + 4.0 + label_room).min(plot.w * 0.25);
    let my = (rmax + RING + 4.0 + 16.0).min(plot.h * 0.25);
    let iw = f64::from((plot.w - 2.0 * mx).max(1.0));
    let ih = f64::from((plot.h - 2.0 * my).max(1.0));
    let bw = b.1 - b.0;
    let bh = b.3 - b.2;
    let eps = 1e-9;
    let (kx, ky) = if model.uniform() {
        let k = match (bw > eps, bh > eps) {
            (true, true) => (iw / bw).min(ih / bh),
            (true, false) => iw / bw,
            (false, true) => ih / bh,
            (false, false) => 100.0,
        };
        (k, k)
    } else {
        let kx = if bw > eps { iw / bw } else { UNIT_MAX.0 };
        let ky = if bh > eps { ih / bh } else { UNIT_MAX.1 };
        (kx.min(UNIT_MAX.0), ky.min(UNIT_MAX.1))
    };
    let c = ((b.0 + b.1) / 2.0, (b.2 + b.3) / 2.0);
    let hx = f64::from(plot.w) / 2.0 / kx;
    let hy = f64::from(plot.h) / 2.0 / ky;
    ((c.0 - hx, c.0 + hx), (c.1 - hy, c.1 + hy))
}

/// Edge `k`'s path, width, arrowhead and colour.
fn edge_geom(
    model: &GraphModel,
    view: &View,
    frame: &Frame,
    nodes: &[Option<NodeGeom>],
    k: usize,
    theme: &Theme,
) -> Option<EdgeGeom> {
    let (a, b) = model.ends[k];
    let (na, nb) = (nodes[a as usize]?, nodes[b as usize]?);
    let width = model.widths[k];
    let directed = model.spec.edges[k].directed;
    let mut pts = if a == b {
        self_loop(na.at, na.r)
    } else {
        route(model, view, frame, k, na.at, nb.at)
    };
    let mut arrow = None;
    if directed {
        let (len, half) = (ARROW.0 + 1.5 * (width - 1.0), ARROW.1 + 0.8 * (width - 1.0));
        if let Some(tip) = cut_at(&mut pts, nb.at, nb.r + RING * 0.5) {
            let back = pts.len().checked_sub(2).map_or(na.at, |i| pts[i]);
            let (dx, dy) = (tip.0 - back.0, tip.1 - back.1);
            let d = dx.hypot(dy).max(1e-3);
            let (ux, uy) = (dx / d, dy / d);
            let base = (tip.0 - ux * len, tip.1 - uy * len);
            arrow = Some([
                tip,
                (base.0 - uy * half, base.1 + ux * half),
                (base.0 + uy * half, base.1 - ux * half),
            ]);
            // The line stops under the arrowhead, short of its tip.
            cut_at(&mut pts, nb.at, nb.r + RING * 0.5 + len * 0.6);
        }
    }
    Some(EdgeGeom {
        pts,
        width,
        arrow,
        color: model.edge_color(k, theme),
        dash: model.edge_fig(k).dash,
    })
}

/// The path between two node centres by the layout: through a layered layout's bends, an elbow
/// to a stacked leaf, an S-curve between a tree's or layers' ranks, else straight, bowed apart
/// from the other edges between the same two nodes.
fn route(
    model: &GraphModel,
    view: &View,
    frame: &Frame,
    k: usize,
    pa: (f32, f32),
    pb: (f32, f32),
) -> Vec<(f32, f32)> {
    let (a, b) = model.ends[k];
    let (lane, lanes) = model.lanes[k];
    let pinned = model.pinned(view, a as usize) || model.pinned(view, b as usize);
    match model.layout {
        Layout::Layered if !model.bends[k].is_empty() && !pinned => {
            let mut pts = vec![pa];
            pts.extend(model.bends[k].iter().map(|&p| frame.px(p)));
            pts.push(pb);
            spline(&pts)
        }
        Layout::Tree { .. } if lanes == 1 && stacked_pair(model, a, b).is_some() => {
            let (parent, child) = match stacked_pair(model, a, b) {
                Some(true) => (pa, pb),
                _ => (pb, pa),
            };
            let mut pts = elbow(parent, child);
            if stacked_pair(model, a, b) == Some(false) {
                pts.reverse();
            }
            pts
        }
        Layout::Tree { .. } | Layout::Layered if lanes == 1 => s_curve(pa, pb),
        _ if lanes > 1 => {
            let off = (f32::from(lane) - (f32::from(lanes) - 1.0) / 2.0) * LANE;
            // The normal from the lower index's end, so both directions bow the same way.
            let (p, q) = if a < b { (pa, pb) } else { (pb, pa) };
            let (dx, dy) = (q.0 - p.0, q.1 - p.1);
            let d = dx.hypot(dy).max(1e-3);
            let (nx, ny) = (-dy / d, dx / d);
            let mid = ((pa.0 + pb.0) / 2.0, (pa.1 + pb.1) / 2.0);
            let c = (mid.0 + nx * off * 2.0, mid.1 + ny * off * 2.0);
            quad(pa, c, pb, 16)
        }
        _ => vec![pa, pb],
    }
}

/// Whether edge a–b joins a manager and a leaf stacked under it: Some(true) when `a` is the
/// manager, Some(false) when `b` is.
fn stacked_pair(model: &GraphModel, a: u32, b: u32) -> Option<bool> {
    if model.stacked[b as usize] == Some(a) {
        Some(true)
    } else if model.stacked[a as usize] == Some(b) {
        Some(false)
    } else {
        None
    }
}

/// From a manager down its column's spine and across to a stacked leaf, the corner rounded.
fn elbow(p: (f32, f32), c: (f32, f32)) -> Vec<(f32, f32)> {
    let corner = (c.0 - p.0).abs().min((c.1 - p.1).abs()).min(5.0);
    let sx = if c.0 >= p.0 { 1.0 } else { -1.0 };
    let sy = if c.1 >= p.1 { 1.0 } else { -1.0 };
    let mut pts = vec![p, (p.0, c.1 - sy * corner)];
    pts.extend(
        quad(
            (p.0, c.1 - sy * corner),
            (p.0, c.1),
            (p.0 + sx * corner, c.1),
            4,
        )
        .into_iter()
        .skip(1),
    );
    pts.push(c);
    pts
}

/// An S-curve from `a` to `b` leaving and arriving upright: straight when they're above each
/// other.
fn s_curve(a: (f32, f32), b: (f32, f32)) -> Vec<(f32, f32)> {
    if (a.0 - b.0).abs() < 1.0 {
        return vec![a, b];
    }
    let my = (a.1 + b.1) / 2.0;
    cubic(a, (a.0, my), (b.0, my), b, 20)
}

/// A loop on a node's upper right, leaving and coming back to its centre.
fn self_loop(c: (f32, f32), r: f32) -> Vec<(f32, f32)> {
    let reach = r + 30.0;
    let th = -std::f32::consts::FRAC_PI_4;
    let p1 = (
        c.0 + reach * (th - 0.55).cos(),
        c.1 + reach * (th - 0.55).sin(),
    );
    let p2 = (
        c.0 + reach * (th + 0.55).cos(),
        c.1 + reach * (th + 0.55).sin(),
    );
    cubic(c, p1, p2, c, 20)
}

/// A smooth curve guided by `pts` from the first to the last (a clamped uniform cubic
/// B-spline), flattened: it stays within their hull, so a long edge never swings wider than the
/// slots it was routed through.
fn spline(pts: &[(f32, f32)]) -> Vec<(f32, f32)> {
    if pts.len() < 3 {
        return pts.to_vec();
    }
    let first = pts[0];
    let last = pts[pts.len() - 1];
    let mut c = vec![first, first];
    c.extend_from_slice(pts);
    c.extend([last, last]);
    let mut out = vec![first];
    for w in c.windows(4) {
        for s in 1..=10 {
            let t = s as f32 / 10.0;
            let (t2, t3) = (t * t, t * t * t);
            let b0 = (1.0 - t).powi(3) / 6.0;
            let b1 = (3.0 * t3 - 6.0 * t2 + 4.0) / 6.0;
            let b2 = (-3.0 * t3 + 3.0 * t2 + 3.0 * t + 1.0) / 6.0;
            let b3 = t3 / 6.0;
            out.push((
                b0 * w[0].0 + b1 * w[1].0 + b2 * w[2].0 + b3 * w[3].0,
                b0 * w[0].1 + b1 * w[1].1 + b2 * w[2].1 + b3 * w[3].1,
            ));
        }
    }
    if let Some(e) = out.last_mut() {
        *e = last;
    }
    out
}

fn quad(a: (f32, f32), c: (f32, f32), b: (f32, f32), steps: usize) -> Vec<(f32, f32)> {
    (0..=steps)
        .map(|i| {
            let t = i as f32 / steps as f32;
            let u = 1.0 - t;
            (
                u * u * a.0 + 2.0 * u * t * c.0 + t * t * b.0,
                u * u * a.1 + 2.0 * u * t * c.1 + t * t * b.1,
            )
        })
        .collect()
}

fn cubic(
    a: (f32, f32),
    c1: (f32, f32),
    c2: (f32, f32),
    b: (f32, f32),
    steps: usize,
) -> Vec<(f32, f32)> {
    (0..=steps)
        .map(|i| {
            let t = i as f32 / steps as f32;
            let u = 1.0 - t;
            let (w0, w1, w2, w3) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
            (
                w0 * a.0 + w1 * c1.0 + w2 * c2.0 + w3 * b.0,
                w0 * a.1 + w1 * c1.1 + w2 * c2.1 + w3 * b.1,
            )
        })
        .collect()
}

/// Cuts a path where it last enters the circle of radius `r` about `c`, coming from outside,
/// and returns that point; None (the path as it was) when it never leaves the circle.
fn cut_at(pts: &mut Vec<(f32, f32)>, c: (f32, f32), r: f32) -> Option<(f32, f32)> {
    let inside = |p: (f32, f32)| (p.0 - c.0).hypot(p.1 - c.1) < r;
    let i = (0..pts.len().saturating_sub(1))
        .rev()
        .find(|&i| !inside(pts[i]) && inside(pts[i + 1]))?;
    let (p, q) = (pts[i], pts[i + 1]);
    // The crossing along p→q, by halving.
    let (mut lo, mut hi) = (0.0f32, 1.0f32);
    for _ in 0..24 {
        let mid = (lo + hi) / 2.0;
        let m = (p.0 + (q.0 - p.0) * mid, p.1 + (q.1 - p.1) * mid);
        if inside(m) {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    let at = (p.0 + (q.0 - p.0) * lo, p.1 + (q.1 - p.1) * lo);
    pts.truncate(i + 1);
    pts.push(at);
    Some(at)
}
