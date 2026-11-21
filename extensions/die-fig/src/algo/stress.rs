//! fig's stress layout, verbatim (briefs/fig-port.md §2): the same positions to 1e-9.
//!
//! Every pair of nodes is a spring whose rest length is their distance in hops (unreachable pairs
//! rest at [`COMPONENT_DISTANCE`], so components stay near each other), with a collision push
//! inside [`COLLISION_DISTANCE`] and a weak pull to the centre. Nodes start on a sunflower spiral,
//! so the result needs no random numbers. Three hundred Jacobi steps cool from `alpha` 1 to about
//! 9.886e-4. The cost is `O(300 · n² / 2)` pair steps and `O(n²)` memory for the hop table: about
//! 0.44 s for 1,000 nodes in release, so a figure runs it in a job.

use std::collections::VecDeque;

/// Steps the layout takes.
pub const ITERATIONS: usize = 300;
/// How much `alpha` loses each step.
pub const COOLING: f64 = 0.0228;
/// The share of a node's velocity it keeps from one step to the next.
pub const VELOCITY_RETENTION: f64 = 0.6;
/// The rest length of one hop.
pub const HOP_DISTANCE: f64 = 1.0;
/// The stiffness of the springs between connected nodes.
pub const STRESS_STRENGTH: f64 = 0.45;
/// The rest length between nodes in different components.
pub const COMPONENT_DISTANCE: f64 = 2.0;
/// The stiffness of the springs between components.
pub const COMPONENT_STRENGTH: f64 = 0.14;
/// Nodes nearer than this push each other apart.
pub const COLLISION_DISTANCE: f64 = 0.35;
/// How hard they push.
pub const COLLISION_STRENGTH: f64 = 0.7;
/// The pull towards the origin.
pub const CENTER_STRENGTH: f64 = 0.01;
/// The fastest a node moves in one step, at `alpha` 1.
pub const MAX_SPEED: f64 = 0.5;
/// π(3 − √5), the sunflower's angle between seeds.
pub const GOLDEN_ANGLE: f64 = 2.399_963_229_728_653;

const UNREACHABLE: u32 = u32::MAX;

/// Each of `n` nodes' position, laid out by stress over `edges` (node indices; the direction is
/// ignored, as are self-loops and ends past `n`). The result is centred on its centroid and
/// scaled so every coordinate is in [-1, 1] with one of them at ±1. y is up, as in fig: a
/// renderer flips it.
pub fn stress(n: usize, edges: &[(usize, usize)]) -> Vec<(f64, f64)> {
    if n == 0 {
        return vec![];
    }
    if n == 1 {
        return vec![(0.0, 0.0)];
    }
    let hops = Hops::new(n, edges);
    let mut p: Vec<(f64, f64)> = (0..n).map(seed).collect();
    let mut v = vec![(0.0f64, 0.0f64); n];
    let mut f = vec![(0.0f64, 0.0f64); n];
    let mut alpha = 1.0f64;
    for _ in 0..ITERATIONS {
        f.fill((0.0, 0.0));
        for l in 0..n {
            for r in l + 1..n {
                let (mut dx, mut dy) = (p[l].0 - p[r].0, p[l].1 - p[r].1);
                if dx == 0.0 && dy == 0.0 {
                    let a = (l * 31 + r * 17) as f64;
                    dx = a.cos() * 0.01;
                    dy = a.sin() * 0.01;
                }
                let d = (dx * dx + dy * dy).max(0.01).sqrt();
                let s = match hops.between(l, r) {
                    Some(h) => {
                        let h = f64::from(h);
                        (h * HOP_DISTANCE - d) / d * STRESS_STRENGTH * (1.0 / (h * h)) * alpha
                    }
                    None => (COMPONENT_DISTANCE - d) / d * COMPONENT_STRENGTH * alpha,
                };
                displace(&mut f, l, r, dx * s, dy * s);
                if d < COLLISION_DISTANCE {
                    let s = (COLLISION_DISTANCE - d) / d * COLLISION_STRENGTH * alpha;
                    displace(&mut f, l, r, dx * s, dy * s);
                }
            }
        }
        for (pt, fo) in p.iter().zip(f.iter_mut()) {
            fo.0 -= pt.0 * CENTER_STRENGTH * alpha;
            fo.1 -= pt.1 * CENTER_STRENGTH * alpha;
        }
        let lim = MAX_SPEED * alpha.sqrt().max(0.1);
        for ((pt, ve), fo) in p.iter_mut().zip(v.iter_mut()).zip(f.iter()) {
            ve.0 = (ve.0 + fo.0) * VELOCITY_RETENTION;
            ve.1 = (ve.1 + fo.1) * VELOCITY_RETENTION;
            let sp = ve.0.hypot(ve.1);
            if sp > lim {
                ve.0 *= lim / sp;
                ve.1 *= lim / sp;
            }
            pt.0 += ve.0;
            pt.1 += ve.1;
        }
        alpha *= 1.0 - COOLING;
    }
    normalize(&mut p);
    p
}

/// Node `i`'s starting place on the sunflower: radius `0.4 · √(i + ½)` at `i` golden angles.
pub fn seed(i: usize) -> (f64, f64) {
    let r = 0.4 * (i as f64 + 0.5).sqrt();
    let a = i as f64 * GOLDEN_ANGLE;
    (r * a.cos(), r * a.sin())
}

fn displace(f: &mut [(f64, f64)], l: usize, r: usize, dx: f64, dy: f64) {
    f[l].0 += dx;
    f[l].1 += dy;
    f[r].0 -= dx;
    f[r].1 -= dy;
}

/// Centred on the centroid (not the box's centre), scaled by the Chebyshev half-extent with a
/// floor of 0.01.
fn normalize(p: &mut [(f64, f64)]) {
    let n = p.len() as f64;
    let c = p.iter().fold((0.0, 0.0), |(x, y), q| (x + q.0, y + q.1));
    let c = (c.0 / n, c.1 / n);
    let e = p.iter().fold(0.01f64, |e, (x, y)| {
        e.max((x - c.0).abs()).max((y - c.1).abs())
    });
    for q in p {
        q.0 = (q.0 - c.0) / e;
        q.1 = (q.1 - c.1) / e;
    }
}

/// Hops between every pair, by a breadth-first search from each node over the undirected
/// adjacency in edge order: `n²` entries.
struct Hops {
    n: usize,
    d: Vec<u32>,
}

impl Hops {
    fn new(n: usize, edges: &[(usize, usize)]) -> Hops {
        let mut nb = vec![Vec::new(); n];
        for &(a, b) in edges {
            if a != b && a < n && b < n {
                nb[a].push(b);
                nb[b].push(a);
            }
        }
        let mut d = vec![UNREACHABLE; n * n];
        let mut q = VecDeque::new();
        for s in 0..n {
            let row = &mut d[s * n..(s + 1) * n];
            row[s] = 0;
            q.push_back(s);
            while let Some(x) = q.pop_front() {
                let nd = row[x] + 1;
                for &y in &nb[x] {
                    if row[y] == UNREACHABLE {
                        row[y] = nd;
                        q.push_back(y);
                    }
                }
            }
        }
        Hops { n, d }
    }

    fn between(&self, l: usize, r: usize) -> Option<u32> {
        let h = self.d[l * self.n + r];
        (h != UNREACHABLE).then_some(h)
    }
}
