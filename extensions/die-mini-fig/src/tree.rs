//! A graph laid out as a tidy tree from its root: each level a row down the panel, the leaves
//! side by side in the order the edges reach them, each parent centred over its children. A
//! node is a box with its label, tinted and edged in its group's colour (the groups keyed in a
//! legend); an edge a curve from parent to child, an arrowhead at the child when the graph is
//! directed, its label halfway along. Nodes the root doesn't reach aren't drawn.

use crate::canvas::{Align, Canvas, Ink, Pen, Rect, mix};
use crate::chart::{Key, legend};
use crate::dash::Job;
use crate::hit::{Hit, Shape};
use crate::spec::Kind;

/// A box's height, and the most a level takes down the panel.
const BOX: f32 = 28.0;
const LEVEL: f32 = 110.0;

/// A walk down from the root: each node's level and its place across, in leaves.
struct Walk<'a> {
    kids: &'a [Vec<usize>],
    at: Vec<Option<(usize, f32)>>,
    leaves: f32,
}

impl Walk<'_> {
    /// Places node `i` at `depth`, then whatever under it isn't placed yet; its place across.
    fn go(&mut self, i: usize, depth: usize) -> f32 {
        self.at[i] = Some((depth, 0.0));
        let mut places = Vec::new();
        for k in self.kids[i].clone() {
            if self.at[k].is_none() {
                places.push(self.go(k, depth + 1));
            }
        }
        let x = match (places.first(), places.last()) {
            (Some(a), Some(b)) => (a + b) / 2.0,
            _ => {
                self.leaves += 1.0;
                self.leaves - 1.0
            }
        };
        self.at[i] = Some((depth, x));
        x
    }
}

/// The tree in `b`; a hit for each node.
pub fn draw(c: &mut Canvas, job: &Job, b: Rect) -> Result<Vec<Hit>, String> {
    let (th, Kind::Tree(g)) = (job.theme, &job.panel.kind) else {
        return Ok(Vec::new());
    };
    let mut groups: Vec<&str> = Vec::new();
    for n in &g.nodes {
        if !groups.contains(&n.group.as_str()) {
            groups.push(&n.group);
        }
    }
    let tint = |group: &str| th.slot(groups.iter().position(|g| *g == group).unwrap_or(0));
    let keyed = groups.iter().filter(|g| !g.is_empty());
    let items: Vec<_> = keyed.map(|g| (g.to_string(), tint(g), Key::Dot)).collect();
    let b = if items.len() > 1 {
        legend(c, th, &items, b)
    } else {
        b
    };
    let id = |s: &str| g.nodes.iter().position(|n| n.id == s);
    let mut kids = vec![Vec::new(); g.nodes.len()];
    for e in &g.edges {
        if let (Some(a), Some(z)) = (id(&e.from), id(&e.to)) {
            kids[a].push(z);
        }
    }
    let root = id(&g.root).ok_or("the root isn't a node")?;
    let mut walk = Walk {
        kids: &kids,
        at: vec![None; g.nodes.len()],
        leaves: 0.0,
    };
    walk.go(root, 0);
    let at = walk.at;
    let depth = at.iter().flatten().map(|a| a.0).max().unwrap_or(0);
    let slot = b.w / walk.leaves.max(1.0);
    let level = ((b.h - BOX) / depth.max(1) as f32).min(LEVEL);
    let top = b.y + ((b.h - BOX - level * depth as f32) / 2.0).max(0.0);
    let centre = |i: usize| at[i].map(|(d, x)| (b.x + (x + 0.5) * slot, top + d as f32 * level));
    let ink = Ink::new(12.0, th.strong);
    // Each box as wide as its label, but no wider than the room to its neighbours on its level.
    let mut boxes: Vec<Option<Rect>> = vec![None; g.nodes.len()];
    for (i, n) in g.nodes.iter().enumerate() {
        let (Some((x, y)), Some((d, _))) = (centre(i), at[i]) else {
            continue;
        };
        let level = (0..g.nodes.len()).filter(|&j| j != i && at[j].is_some_and(|a| a.0 == d));
        let room = level
            .filter_map(centre)
            .map(|(xj, _)| (xj - x).abs())
            .fold(b.w + 12.0, f32::min);
        let ink = if n.size > 1.0 { ink.bold() } else { ink };
        let w = (c.width(&n.label, ink) + 28.0).min(room - 12.0).max(40.0);
        let left = (x - w / 2.0).clamp(b.x, (b.right() - w).max(b.x));
        boxes[i] = Some(Rect::new(left, y, w, BOX));
    }
    for e in &g.edges {
        let (Some(a), Some(z)) = (id(&e.from), id(&e.to)) else {
            continue;
        };
        let (Some(pa), Some(pz)) = (boxes[a], boxes[z]) else {
            continue;
        };
        let (from, to) = ((pa.x + pa.w / 2.0, pa.bottom()), (pz.x + pz.w / 2.0, pz.y));
        c.curve(from, to, Pen::new(1.2, th.dim));
        if g.directed {
            c.triangle(
                [(to.0 - 4.0, to.1 - 7.0), (to.0 + 4.0, to.1 - 7.0), to],
                th.dim,
            );
        }
        if !e.label.is_empty() {
            let mid = ((from.0 + to.0) / 2.0, (from.1 + to.1) / 2.0);
            let small = Ink::new(11.0, th.muted);
            let w = c.width(&e.label, small).min(120.0);
            let back = Rect::new(mid.0 - w / 2.0 - 4.0, mid.1 - 8.0, w + 8.0, 15.0);
            c.fill(back, 3.0, th.surface, 1.0);
            c.text(&e.label, small, (mid.0, mid.1 + 4.0), Align::Middle, 120.0);
        }
    }
    let mut hits = Vec::new();
    for (i, n) in g.nodes.iter().enumerate() {
        let (Some(r), Some((d, _))) = (boxes[i], at[i]) else {
            continue;
        };
        let tone = tint(&n.group);
        c.fill(r, 6.0, mix(th.surface, tone, 0.22), 1.0);
        c.outline(r, 6.0, Pen::new(1.0, tone));
        let ink = if n.size > 1.0 { ink.bold() } else { ink };
        c.text(
            &n.label,
            ink,
            (r.x + r.w / 2.0, r.y + 18.5),
            Align::Middle,
            r.w - 14.0,
        );
        let mut rows = Vec::new();
        if !n.group.is_empty() {
            rows.push(("group".to_string(), n.group.clone()));
        }
        rows.extend(n.fields.iter().cloned());
        let key = (job.index, d, r.x.max(0.0) as usize);
        hits.push(Hit {
            shape: Shape::Rect(r),
            key,
            title: n.label.clone(),
            color: tone,
            rows,
        });
    }
    Ok(hits)
}
