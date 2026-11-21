//! A layered layout for a DAG: longest-path layering, barycentre ordering in four sweeps, unit
//! spacing.
//!
//! The steps are Sugiyama's. Cycles are broken by turning round the edges a depth-first search
//! (from each node in index order, over edges in order) finds going back. Each node's layer is
//! its longest path from a source, so sources are on top and every edge points down. An edge
//! that spans layers gets a dummy node in each layer between, so long edges have room to pass,
//! and [`layered_routes`] gives each edge its dummies' places, the bends a renderer draws it
//! through: drawn straight, a long edge can run through nodes in the layers it crosses. Each layer
//! is then ordered by its nodes' barycentres, the mean place of their neighbours in the layer
//! before: four sweeps, down, up, down, up, keeping the order with the fewest crossings seen
//! (the first of equals). Places start one unit apart, dummies included, each layer centred on
//! x = 0; then [`BALANCE`] sweeps move each node towards the median of its neighbours in the
//! layers either side, keeping its layer's order and a unit between neighbours (the nearest
//! such places, by pooling adjacent violators), so a parent sits over its children and a long
//! edge runs straight rather than in a long sweep.
//!
//! Dummies cost time and memory with the layers each edge spans, so at most [`DUMMY_BUDGET`] are
//! made: the edges spanning the fewest layers get theirs first, and an edge past the budget has
//! none, plays no part in the ordering, and is drawn straight.

/// The most dummy nodes one layout makes (a few megabytes).
pub const DUMMY_BUDGET: usize = 250_000;
/// How many sweeps, down and up in turn, move nodes towards their neighbours.
pub const BALANCE: usize = 6;

/// Each of `n` nodes' position, laid out in layers by `edges` (node indices, from → to;
/// self-loops, duplicates and ends past `n` are ignored): x in units, each layer centred on 0,
/// and y up as in [`stress`](fn@super::stress), so layer `k` is at `y = -k` and a renderer
/// that flips y puts the sources on top. The nodes of [`layered_routes`].
pub fn layered(n: usize, edges: &[(usize, usize)]) -> Vec<(f64, f64)> {
    layered_routes(n, edges).nodes
}

/// A layered layout with its edges' routes.
#[derive(Clone, Debug, PartialEq)]
pub struct Routed {
    /// Each node's position, as [`layered`] gives it.
    pub nodes: Vec<(f64, f64)>,
    /// For each edge, in the order given, the bends to draw it through.
    pub bends: Vec<Vec<(f64, f64)>>,
}

/// [`layered`]'s positions, and for each of `edges`, in their order, the bends to draw it
/// through: its dummies' places, from its `from` end to its `to` end (so an edge the layout
/// turned round runs back up), in the same units. A line from `from` through the bends to `to`
/// steps one layer at a time and meets each layer it crosses at its own slot, a unit from every
/// node there, as an edge between neighbouring layers does; drawn straight instead, it can run
/// through those nodes. Empty for an edge between neighbouring layers, a self-loop, an end past
/// `n`, and an edge past [`DUMMY_BUDGET`]; a duplicate gets the same bends as the first.
pub fn layered_routes(n: usize, edges: &[(usize, usize)]) -> Routed {
    let mut bends = vec![Vec::new(); edges.len()];
    if n == 0 {
        return Routed {
            nodes: vec![],
            bends,
        };
    }
    let kept = acyclic(n, edges);
    let layer = layers(n, &kept);
    let mut g = Layered::new(n, &kept, &layer);
    g.order();
    let mut pos = g.positions();
    g.balance(&mut pos);
    for (route, &(a, b)) in bends.iter_mut().zip(edges) {
        if a >= n || b >= n || a == b {
            continue;
        }
        let (e, back) = match kept.binary_search(&(a, b)) {
            Ok(e) => (e, false),
            Err(_) => match kept.binary_search(&(b, a)) {
                Ok(e) => (e, true),
                Err(_) => continue,
            },
        };
        let (first, end) = g.chains[e];
        route.extend(pos[first..end].iter().copied());
        if back {
            route.reverse();
        }
    }
    pos.truncate(n);
    Routed { nodes: pos, bends }
}

/// The edges with every one a depth-first search finds going back turned round, in a fixed
/// order without duplicates or self-loops.
fn acyclic(n: usize, edges: &[(usize, usize)]) -> Vec<(usize, usize)> {
    let mut out_edges = vec![Vec::new(); n];
    for &(a, b) in edges {
        if a < n && b < n && a != b {
            out_edges[a].push(b);
        }
    }
    // 0 unvisited, 1 on the search's path, 2 done.
    let mut state = vec![0u8; n];
    let mut kept = Vec::new();
    for s in 0..n {
        if state[s] != 0 {
            continue;
        }
        state[s] = 1;
        let mut stack = vec![(s, 0usize)];
        while let Some(&mut (v, ref mut next)) = stack.last_mut() {
            let Some(&w) = out_edges[v].get(*next) else {
                state[v] = 2;
                stack.pop();
                continue;
            };
            *next += 1;
            match state[w] {
                0 => {
                    kept.push((v, w));
                    state[w] = 1;
                    stack.push((w, 0));
                }
                1 => kept.push((w, v)),
                _ => kept.push((v, w)),
            }
        }
    }
    kept.sort_unstable();
    kept.dedup();
    kept
}

/// Each node's layer: its longest path from a source.
fn layers(n: usize, edges: &[(usize, usize)]) -> Vec<usize> {
    let mut indegree = vec![0usize; n];
    let mut succ = vec![Vec::new(); n];
    for &(a, b) in edges {
        succ[a].push(b);
        indegree[b] += 1;
    }
    let mut layer = vec![0usize; n];
    let mut ready: Vec<usize> = (0..n).filter(|&v| indegree[v] == 0).rev().collect();
    while let Some(v) = ready.pop() {
        for &w in &succ[v] {
            layer[w] = layer[w].max(layer[v] + 1);
            indegree[w] -= 1;
            if indegree[w] == 0 {
                ready.push(w);
            }
        }
    }
    layer
}

/// Stands for a dummy's neighbour below before it's linked.
const UNLINKED: usize = usize::MAX;

/// The layers, dummies included (indices from `n` up), and each node's neighbours a layer up
/// and down: a real node's in lists, a dummy's one each way in flat arrays.
struct Layered {
    n: usize,
    rows: Vec<Vec<usize>>,
    above: Vec<Vec<usize>>,
    below: Vec<Vec<usize>>,
    /// Dummy `n + i`'s neighbour above, and below.
    up: Vec<usize>,
    down: Vec<usize>,
    /// Each kept edge's dummies, `first..end`, top to bottom.
    chains: Vec<(usize, usize)>,
}

impl Layered {
    fn new(n: usize, edges: &[(usize, usize)], layer: &[usize]) -> Layered {
        let depth = layer.iter().max().map_or(1, |&l| l + 1);
        let mut rows = vec![Vec::new(); depth];
        for (v, &l) in layer.iter().enumerate() {
            rows[l].push(v);
        }
        let mut g = Layered {
            n,
            rows,
            above: vec![Vec::new(); n],
            below: vec![Vec::new(); n],
            up: Vec::new(),
            down: Vec::new(),
            chains: Vec::with_capacity(edges.len()),
        };
        let most = most_dummies(edges.iter().map(|&(a, b)| layer[b] - layer[a] - 1));
        for &(a, b) in edges {
            let first = n + g.up.len();
            if layer[b] - layer[a] - 1 > most {
                g.chains.push((first, first));
                continue;
            }
            let mut from = a;
            for k in layer[a] + 1..layer[b] {
                let d = n + g.up.len();
                g.up.push(from);
                g.down.push(UNLINKED);
                g.rows[k].push(d);
                g.link_down(from, d);
                from = d;
            }
            g.link_down(from, b);
            g.above[b].push(from);
            g.chains.push((first, n + g.up.len()));
        }
        g
    }

    /// Records `w` as a neighbour below `v`.
    fn link_down(&mut self, v: usize, w: usize) {
        match v.checked_sub(self.n) {
            None => self.below[v].push(w),
            Some(d) => self.down[d] = w,
        }
    }

    fn above(&self, v: usize) -> &[usize] {
        match v.checked_sub(self.n) {
            None => &self.above[v],
            Some(d) => std::slice::from_ref(&self.up[d]),
        }
    }

    fn below(&self, v: usize) -> &[usize] {
        match v.checked_sub(self.n) {
            None => &self.below[v],
            Some(d) => std::slice::from_ref(&self.down[d]),
        }
    }

    /// Four barycentre sweeps, keeping the order with the fewest crossings.
    fn order(&mut self) {
        let mut best = (self.crossings(), self.rows.clone());
        // Only the fixed layer's entries are read, and each sweep writes them first.
        let mut place = vec![0.0; self.n + self.up.len()];
        let mut keyed: Vec<(f64, usize, usize)> = Vec::new();
        for sweep in 0..4 {
            let down = sweep % 2 == 0;
            let ks: Vec<usize> = if down {
                (1..self.rows.len()).collect()
            } else {
                (0..self.rows.len().saturating_sub(1)).rev().collect()
            };
            for k in ks {
                let fixed = if down { k - 1 } else { k + 1 };
                for (i, &v) in self.rows[fixed].iter().enumerate() {
                    place[v] = i as f64;
                }
                keyed.clear();
                keyed.extend(self.rows[k].iter().enumerate().map(|(i, &v)| {
                    let ns = if down { self.above(v) } else { self.below(v) };
                    let bary = if ns.is_empty() {
                        i as f64
                    } else {
                        ns.iter().map(|&u| place[u]).sum::<f64>() / ns.len() as f64
                    };
                    (bary, i, v)
                }));
                keyed.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
                let row = &mut self.rows[k];
                row.clear();
                row.extend(keyed.iter().map(|&(_, _, v)| v));
            }
            let c = self.crossings();
            if c < best.0 {
                best = (c, self.rows.clone());
            }
        }
        self.rows = best.1;
    }

    /// How many pairs of edges cross between neighbouring layers: inversions among the lower
    /// ends, counted with a Fenwick tree.
    fn crossings(&self) -> u64 {
        let mut place = vec![0usize; self.n + self.up.len()];
        for row in &self.rows {
            for (i, &v) in row.iter().enumerate() {
                place[v] = i;
            }
        }
        let (mut total, mut ends, mut tree) = (0u64, Vec::new(), Vec::new());
        for k in 0..self.rows.len().saturating_sub(1) {
            ends.clear();
            for &v in &self.rows[k] {
                ends.extend(self.below(v).iter().map(|&w| (place[v], place[w])));
            }
            ends.sort_unstable();
            let width = self.rows[k + 1].len();
            tree.clear();
            tree.resize(width + 1, 0u64);
            for (seen, &(_, b)) in ends.iter().enumerate() {
                // Lower ends seen so far at or left of b.
                let (mut i, mut le) = (b + 1, 0u64);
                while i > 0 {
                    le += tree[i];
                    i &= i - 1;
                }
                total += seen as u64 - le;
                let mut i = b + 1;
                while i <= width {
                    tree[i] += 1;
                    i += i & i.wrapping_neg();
                }
            }
        }
        total
    }

    /// Every node's and dummy's place: its row's index, the row centred on 0, and minus its
    /// layer.
    fn positions(&self) -> Vec<(f64, f64)> {
        let mut pos = vec![(0.0, 0.0); self.n + self.up.len()];
        for (k, row) in self.rows.iter().enumerate() {
            let centre = (row.len() as f64 - 1.0) / 2.0;
            for (i, &v) in row.iter().enumerate() {
                pos[v] = (i as f64 - centre, 0.0 - k as f64);
            }
        }
        pos
    }
}

impl Layered {
    /// [`BALANCE`] sweeps, down and up in turn: each layer's nodes go as near as they can to the
    /// median of their neighbours' places (above and below), in their order and a unit apart.
    fn balance(&self, pos: &mut [(f64, f64)]) {
        let depth = self.rows.len();
        if depth < 2 {
            return;
        }
        let mut want: Vec<f64> = Vec::new();
        let mut near: Vec<f64> = Vec::new();
        for sweep in 0..BALANCE {
            let ks: Vec<usize> = if sweep % 2 == 0 {
                (0..depth).collect()
            } else {
                (0..depth).rev().collect()
            };
            for k in ks {
                let row = &self.rows[k];
                want.clear();
                for &v in row {
                    near.clear();
                    near.extend(self.above(v).iter().map(|&u| pos[u].0));
                    near.extend(self.below(v).iter().map(|&u| pos[u].0));
                    want.push(match near.len() {
                        0 => pos[v].0,
                        _ => median(&mut near),
                    });
                }
                for (&v, x) in row.iter().zip(spaced(&want)) {
                    pos[v].0 = x;
                }
            }
        }
    }
}

/// The median of some places (the mean of the middle two of an even count).
fn median(xs: &mut [f64]) -> f64 {
    xs.sort_by(f64::total_cmp);
    let m = xs.len() / 2;
    match xs.len() % 2 {
        1 => xs[m],
        _ => (xs[m - 1] + xs[m]) / 2.0,
    }
}

/// The places nearest `want` (least squares) in their order and at least a unit apart: pool
/// adjacent violators over `want[i] - i`, which must not fall.
fn spaced(want: &[f64]) -> Vec<f64> {
    // Each block: its total and its count.
    let mut blocks: Vec<(f64, usize)> = Vec::with_capacity(want.len());
    for (i, &w) in want.iter().enumerate() {
        blocks.push((w - i as f64, 1));
        while blocks.len() > 1 {
            let (b, c) = blocks[blocks.len() - 1];
            let (a, d) = blocks[blocks.len() - 2];
            if a / d as f64 <= b / c as f64 {
                break;
            }
            blocks.pop();
            let last = blocks.len() - 1;
            blocks[last] = (a + b, c + d);
        }
    }
    // On a grid of 1/1024, where adding whole units is exact: neighbours stay a unit apart
    // to the last bit, and rounding keeps the blocks' order.
    let mut out = Vec::with_capacity(want.len());
    for (total, count) in blocks {
        let mean = (total / count as f64 * 1024.0).round() / 1024.0;
        for _ in 0..count {
            out.push(mean + out.len() as f64);
        }
    }
    out
}

/// The most dummies an edge may have within [`DUMMY_BUDGET`], given each edge's count: every
/// edge with at most that many gets them, and together they fit.
fn most_dummies(counts: impl Iterator<Item = usize>) -> usize {
    let mut sorted: Vec<usize> = counts.collect();
    sorted.sort_unstable();
    let (mut total, mut most, mut i) = (0usize, 0, 0);
    while let Some(&c) = sorted.get(i) {
        let j = i + sorted[i..].partition_point(|&d| d == c);
        total = total.saturating_add(c.saturating_mul(j - i));
        if total > DUMMY_BUDGET {
            break;
        }
        (most, i) = (c, j);
    }
    most
}
