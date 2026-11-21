//! A layered layout for a DAG: longest-path layering, barycentre ordering in four sweeps, unit
//! spacing.
//!
//! The steps are Sugiyama's. Cycles are broken by turning round the edges a depth-first search
//! (from each node in index order, over edges in order) finds going back. Each node's layer is
//! its longest path from a source, so sources are on top and every edge points down. An edge
//! that spans layers gets a dummy node in each layer between, so long edges have room to pass.
//! Each layer is then ordered by its nodes' barycentres, the mean place of their neighbours in
//! the layer before: four sweeps, down, up, down, up, keeping the order with the fewest crossings
//! seen (the first of equals). Places are one unit apart, dummies included, each layer centred
//! on x = 0.

/// Each of `n` nodes' position, laid out in layers by `edges` (node indices, from → to;
/// self-loops, duplicates and ends past `n` are ignored): x in units, each layer centred on 0,
/// and y up as in [`stress`](fn@super::stress), so layer `k` is at `y = -k` and a renderer
/// that flips y puts the sources on top.
pub fn layered(n: usize, edges: &[(usize, usize)]) -> Vec<(f64, f64)> {
    if n == 0 {
        return vec![];
    }
    let edges = acyclic(n, edges);
    let layer = layers(n, &edges);
    let mut g = Layered::new(n, &edges, &layer);
    g.order();
    let mut out = vec![(0.0, 0.0); n];
    for (k, row) in g.rows.iter().enumerate() {
        let centre = (row.len() as f64 - 1.0) / 2.0;
        for (i, &v) in row.iter().enumerate() {
            if v < n {
                out[v] = (i as f64 - centre, 0.0 - k as f64);
            }
        }
    }
    out
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

/// The layers, dummies included (indices from `n` up), and each node's neighbours a layer up
/// and down.
struct Layered {
    rows: Vec<Vec<usize>>,
    above: Vec<Vec<usize>>,
    below: Vec<Vec<usize>>,
}

impl Layered {
    fn new(n: usize, edges: &[(usize, usize)], layer: &[usize]) -> Layered {
        let depth = layer.iter().max().map_or(1, |&l| l + 1);
        let mut rows = vec![Vec::new(); depth];
        for (v, &l) in layer.iter().enumerate() {
            rows[l].push(v);
        }
        let (mut above, mut below) = (vec![Vec::new(); n], vec![Vec::new(); n]);
        for &(a, b) in edges {
            let mut from = a;
            for row in &mut rows[layer[a] + 1..layer[b]] {
                let d = above.len();
                above.push(Vec::new());
                below.push(Vec::new());
                row.push(d);
                below[from].push(d);
                above[d].push(from);
                from = d;
            }
            below[from].push(b);
            above[b].push(from);
        }
        Layered { rows, above, below }
    }

    /// Four barycentre sweeps, keeping the order with the fewest crossings.
    fn order(&mut self) {
        let mut best = (self.crossings(), self.rows.clone());
        for sweep in 0..4 {
            let down = sweep % 2 == 0;
            let ks: Vec<usize> = if down {
                (1..self.rows.len()).collect()
            } else {
                (0..self.rows.len().saturating_sub(1)).rev().collect()
            };
            for k in ks {
                let fixed = if down { k - 1 } else { k + 1 };
                let near = if down { &self.above } else { &self.below };
                let mut place = vec![0.0; near.len()];
                for (i, &v) in self.rows[fixed].iter().enumerate() {
                    place[v] = i as f64;
                }
                let mut keyed: Vec<(f64, usize, usize)> = self.rows[k]
                    .iter()
                    .enumerate()
                    .map(|(i, &v)| {
                        let ns = &near[v];
                        let bary = if ns.is_empty() {
                            i as f64
                        } else {
                            ns.iter().map(|&u| place[u]).sum::<f64>() / ns.len() as f64
                        };
                        (bary, i, v)
                    })
                    .collect();
                keyed.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
                self.rows[k] = keyed.into_iter().map(|(_, _, v)| v).collect();
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
        let mut place = vec![0usize; self.above.len()];
        for row in &self.rows {
            for (i, &v) in row.iter().enumerate() {
                place[v] = i;
            }
        }
        let place = &place;
        let mut total = 0u64;
        for k in 0..self.rows.len().saturating_sub(1) {
            let mut ends: Vec<(usize, usize)> = self.rows[k]
                .iter()
                .flat_map(|&v| self.below[v].iter().map(move |&w| (place[v], place[w])))
                .collect();
            ends.sort_unstable();
            let width = self.rows[k + 1].len();
            let mut tree = vec![0u64; width + 1];
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
}
