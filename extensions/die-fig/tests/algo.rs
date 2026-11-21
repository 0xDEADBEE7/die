//! die-fig's algorithms: fig's stress layout against golden positions and fig's own layout tests;
//! the tidy tree and the layered layout's invariants, its routes for long edges and its budget;
//! fig's LTTB and search scores against golden values, and the tiers against flex; the pyramid's
//! M4 envelope against a brute-force scan over random data, and its line against the line
//! through every sample; the hit index against brute force. Timing tests, ignored, measure a
//! million samples' envelope, nearest x in a long gap, and dense layered layouts.

use die_test_cap as _;

use die_fig::algo::hit::segment_distance;
use die_fig::algo::layered::DUMMY_BUDGET;
use die_fig::algo::pyramid::{EMPTY, LEAF, column};
use die_fig::algo::search::{UNMATCHED, score};
use die_fig::algo::stress::seed;
use die_fig::algo::{HitIndex, Pyramid, Routed, layered, layered_routes, lttb, stress, tier, tree};
use die_fig::data::SeriesKey;
use die_fig::interact::consts::HIT_DENSE;
use std::collections::BTreeMap;
use std::ops::Range;
use std::time::Instant;

/// A small deterministic generator (xorshift64*), so random tests are the same every run.
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Rng {
        Rng(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1)
    }
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }
    /// In [0, 1).
    fn unit(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

fn distance(a: (f64, f64), b: (f64, f64)) -> f64 {
    (a.0 - b.0).hypot(a.1 - b.1)
}

fn close(got: &[(f64, f64)], want: &[(f64, f64)]) {
    assert_eq!(got.len(), want.len());
    for (i, (g, w)) in got.iter().zip(want).enumerate() {
        assert!(
            (g.0 - w.0).abs() < 1e-9 && (g.1 - w.1).abs() < 1e-9,
            "node {i}: got {g:?}, want {w:?}"
        );
    }
}

// ---------------------------------------------------------------------------------- stress

#[test]
fn stress_matches_figs_positions_for_its_example_graph() {
    // examples/graph.json: api, worker, database, cache; api-worker, api-database, worker-cache.
    close(
        &stress(4, &[(0, 1), (0, 2), (1, 3)]),
        &[
            (-0.039908450688, -0.326829508501),
            (-0.081983877665, 0.349288178060),
            (-0.056958136831, -1.000000000000),
            (0.178850465183, 0.977541330441),
        ],
    );
}

#[test]
fn stress_matches_figs_positions_for_two_components_and_an_isolated_node() {
    close(
        &stress(6, &[(0, 1), (1, 2), (3, 4)]),
        &[
            (0.687498714967, 0.328656822663),
            (0.055221041278, -0.327432769900),
            (-0.563862029475, -0.828441994245),
            (-0.281407316765, 1.000000000000),
            (-0.877835950919, 0.537197683153),
            (0.980385540914, -0.709979741672),
        ],
    );
}

#[test]
fn stress_seeds_on_figs_sunflower() {
    let seeds: Vec<_> = (0..4).map(seed).collect();
    close(
        &seeds,
        &[
            (0.282842712475, 0.0),
            (-0.361235500700, 0.330921309429),
            (0.055292883239, -0.630033885647),
            (0.455313951716, 0.593876422644),
        ],
    );
}

#[test]
fn stress_of_no_node_or_one() {
    assert!(stress(0, &[]).is_empty());
    assert_eq!(stress(1, &[(0, 0)]), vec![(0.0, 0.0)]);
}

#[test]
fn stress_ignores_direction_self_loops_and_ends_past_the_nodes() {
    let plain = stress(4, &[(0, 1), (0, 2), (1, 3)]);
    assert_eq!(stress(4, &[(1, 0), (2, 0), (3, 1), (2, 2), (0, 9)]), plain);
}

#[test]
fn stress_fits_in_the_unit_square_touching_it() {
    let p = stress(8, &[(0, 1), (1, 2), (2, 3), (3, 4)]);
    let most = p
        .iter()
        .fold(0.0f64, |m, q| m.max(q.0.abs()).max(q.1.abs()));
    assert!((most - 1.0).abs() < 1e-12, "{most}");
}

// fig's test/graph_layout.rs, ported.

#[test]
fn stress_puts_connected_nodes_closer_than_unrelated_ones() {
    let p = stress(4, &[(0, 1), (1, 2), (2, 3)]);
    assert!(distance(p[0], p[1]) < distance(p[0], p[3]));
}

#[test]
fn stress_keeps_disconnected_nodes_within_a_reasonable_range() {
    let p = stress(6, &[(0, 1), (1, 2)]);
    let linked = (distance(p[0], p[1]) + distance(p[1], p[2])) / 2.0;
    let unrelated = (distance(p[0], p[3]) + distance(p[1], p[4]) + distance(p[2], p[5])) / 3.0;
    assert!(unrelated < linked * 1.8);
}

#[test]
fn stress_is_deterministic_and_separates_nodes() {
    let edges = [(0, 1), (1, 2), (2, 3), (3, 4)];
    let first = stress(8, &edges);
    assert_eq!(first, stress(8, &edges));
    assert!(minimum_distance(&first) > 0.01);
}

#[test]
fn stress_does_not_collapse_dense_neighbourhoods_into_a_ball() {
    let mut edges = Vec::new();
    for start in [0, 10, 20] {
        for node in start + 1..start + 10 {
            edges.push((start, node));
            edges.push((node, start + 1 + (node - start) % 9));
        }
    }
    edges.extend([(0, 10), (10, 20)]);
    let p = stress(30, &edges);
    assert!(minimum_distance(&p) > 0.025);
    let linked = edges
        .iter()
        .map(|&(a, b)| distance(p[a], p[b]))
        .sum::<f64>()
        / edges.len() as f64;
    let pairs: Vec<f64> = (0..p.len())
        .flat_map(|a| (a + 1..p.len()).map(move |b| (a, b)))
        .map(|(a, b)| distance(p[a], p[b]))
        .collect();
    assert!(linked < pairs.iter().sum::<f64>() / pairs.len() as f64);
}

#[test]
fn stress_puts_primary_branches_in_distinct_regions() {
    let branches: Vec<Range<usize>> = (0..3).map(|b| 1 + b * 40..1 + (b + 1) * 40).collect();
    let mut edges = Vec::new();
    for br in &branches {
        edges.push((0, br.start));
        for node in br.start + 1..br.end {
            edges.push((br.start + (node - br.start - 1) / 3, node));
        }
    }
    let p = stress(121, &edges);
    let centres: Vec<(f64, f64)> = branches
        .iter()
        .map(|br| {
            let s = p[br.clone()]
                .iter()
                .fold((0.0, 0.0), |s, q| (s.0 + q.0, s.1 + q.1));
            (s.0 / br.len() as f64, s.1 / br.len() as f64)
        })
        .collect();
    for (i, br) in branches.iter().enumerate() {
        let grouped = br
            .clone()
            .filter(|&node| {
                let own = distance(p[node], centres[i]);
                let other = (0..centres.len())
                    .filter(|&j| j != i)
                    .map(|j| distance(p[node], centres[j]))
                    .fold(f64::INFINITY, f64::min);
                own < other
            })
            .count();
        assert!(grouped >= br.len() * 4 / 5, "branch {i}: {grouped}");
    }
}

fn minimum_distance(p: &[(f64, f64)]) -> f64 {
    (0..p.len())
        .flat_map(|a| (a + 1..p.len()).map(move |b| (a, b)))
        .map(|(a, b)| distance(p[a], p[b]))
        .fold(f64::INFINITY, f64::min)
}

// ------------------------------------------------------------------------------------ tree

/// Every tidy-tree invariant: y is minus the depth, the leftmost node is at 0, a parent is
/// centred over its first and last child, children go left to right in index order, and nodes
/// at one depth are at least a unit apart.
fn assert_tidy(n: usize, parent: &[Option<usize>], p: &[(f64, f64)]) {
    assert_eq!(p.len(), n);
    let depth = |mut v: usize| {
        let mut d = 0;
        while let Some(u) = parent[v] {
            v = u;
            d += 1;
        }
        d as f64
    };
    for v in 0..n {
        assert!(p[v].0.is_finite());
        assert_eq!(p[v].1, -depth(v), "node {v}'s depth");
        let kids: Vec<usize> = (0..n).filter(|&c| parent[c] == Some(v)).collect();
        if let (Some(&a), Some(&b)) = (kids.first(), kids.last()) {
            assert!(
                (p[v].0 - (p[a].0 + p[b].0) / 2.0).abs() < 1e-9,
                "node {v} isn't centred over its children"
            );
        }
        for w in kids.windows(2) {
            assert!(p[w[0]].0 < p[w[1]].0, "children {w:?} out of order");
        }
    }
    let roots: Vec<usize> = (0..n).filter(|&v| parent[v].is_none()).collect();
    for w in roots.windows(2) {
        assert!(p[w[0]].0 < p[w[1]].0, "roots {w:?} out of order");
    }
    let least = p.iter().fold(f64::INFINITY, |l, q| l.min(q.0));
    assert!(least.abs() < 1e-12, "leftmost at {least}");
    let mut by_depth: Vec<(f64, f64)> = p.iter().map(|&(x, y)| (y, x)).collect();
    by_depth.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
    for w in by_depth.windows(2) {
        if w[0].0 == w[1].0 {
            assert!(
                w[1].1 - w[0].1 >= 1.0 - 1e-9,
                "too close at depth {}",
                -w[0].0
            );
        }
    }
}

#[test]
fn tree_centres_a_parent_over_its_children_a_unit_apart() {
    let parent = [None, Some(0), Some(0), Some(0)];
    assert_eq!(
        tree(4, &parent),
        vec![(1.0, 0.0), (0.0, -1.0), (1.0, -1.0), (2.0, -1.0)]
    );
}

#[test]
fn tree_of_one_node_and_of_none() {
    assert_eq!(tree(1, &[None]), vec![(0.0, 0.0)]);
    assert!(tree(0, &[]).is_empty());
}

#[test]
fn tree_keeps_its_invariants_on_random_forests() {
    for s in 0..40 {
        let mut rng = Rng::new(s);
        let n = 1 + rng.below(300);
        let parent: Vec<Option<usize>> = (0..n)
            .map(|i| {
                // A few roots; otherwise a parent among the last few nodes or any before.
                if i == 0 || rng.below(25) == 0 {
                    None
                } else if rng.below(2) == 0 {
                    Some(i - 1 - rng.below(i.min(4)))
                } else {
                    Some(rng.below(i))
                }
            })
            .collect();
        let p = tree(n, &parent);
        assert_tidy(n, &parent, &p);
        assert_eq!(p, tree(n, &parent), "seed {s} isn't deterministic");
    }
}

#[test]
fn tree_draws_the_same_subtree_the_same_wherever_it_sits() {
    // A root, then a lopsided subtree, a single leaf, and the same lopsided subtree again.
    let shape = [
        None,
        Some(0),
        Some(0),
        Some(1),
        Some(1),
        Some(1),
        Some(4),
        Some(4),
    ];
    let mut parent = vec![None];
    let mut roots = Vec::new();
    for copy in 0..3 {
        let base = parent.len();
        roots.push(base);
        if copy == 1 {
            parent.push(Some(0));
            continue;
        }
        for s in shape {
            parent.push(Some(s.map_or(0, |p| base + p)));
        }
    }
    let n = parent.len();
    let p = tree(n, &parent);
    assert_tidy(n, &parent, &p);
    let (a, b) = (roots[0], roots[2]);
    for i in 0..shape.len() {
        assert!(
            ((p[a + i].0 - p[a].0) - (p[b + i].0 - p[b].0)).abs() < 1e-9,
            "node {i} of the copies differs"
        );
    }
}

#[test]
fn tree_puts_several_roots_side_by_side() {
    // Two trees: 0 with children 1, 2; 3 with children 4, 5, 6.
    let parent = [None, Some(0), Some(0), None, Some(3), Some(3), Some(3)];
    let p = tree(7, &parent);
    assert_tidy(7, &parent, &p);
    assert!(
        [1, 2]
            .iter()
            .all(|&a| [4, 5, 6].iter().all(|&b| p[a].0 < p[b].0))
    );
    assert_eq!(
        p[4].0 - p[2].0,
        1.0,
        "the trees are a unit apart where they come nearest"
    );
}

#[test]
fn tree_takes_bad_parents_as_roots_and_cuts_cycles() {
    let p = tree(4, &[Some(9), Some(1), None, Some(2)]);
    assert_tidy(4, &[None, None, None, Some(2)], &p);
    // 1 -> 2 -> 3 -> 1 is a cycle, and 0 hangs off it: the walk up from 0 first comes round at
    // 1, which becomes the root.
    let p = tree(4, &[Some(1), Some(2), Some(3), Some(1)]);
    assert_tidy(4, &[Some(1), None, Some(3), Some(1)], &p);
}

#[test]
fn tree_lays_out_a_deep_chain_without_recursion() {
    let n = 20_000usize;
    let parent: Vec<Option<usize>> = (0..n).map(|i| i.checked_sub(1)).collect();
    let p = tree(n, &parent);
    assert!(p.iter().all(|q| q.0 == 0.0));
    assert_eq!(p[n - 1].1, -((n - 1) as f64));
}

#[test]
fn tree_shares_a_push_evenly_among_the_subtrees_between() {
    // A root over a wide subtree, one or two leaves, and the same wide subtree again: the second
    // is pushed four units off the first, and the leaves between take equal shares of the push
    // rather than staying a unit from their left neighbour.
    for leaves in 1..=2 {
        let mut parent = vec![None];
        let mut add = |p: usize| {
            parent.push(Some(p));
            parent.len() - 1
        };
        let mut kids = vec![add(0)];
        kids.extend((0..leaves).map(|_| add(0)));
        kids.push(add(0));
        for r in [kids[0], kids[leaves + 1]] {
            let (a, _, c) = (add(r), add(r), add(r));
            for q in [a, a, c, c] {
                add(q);
            }
        }
        let n = parent.len();
        let p = tree(n, &parent);
        assert_tidy(n, &parent, &p);
        let gaps: Vec<f64> = kids.windows(2).map(|w| p[w[1]].0 - p[w[0]].0).collect();
        for g in &gaps {
            assert!(
                (g - 4.0 / (leaves + 1) as f64).abs() < 1e-9,
                "{leaves} leaves: {gaps:?}"
            );
        }
    }
}

// --------------------------------------------------------------------------------- layered

/// The pairs of edges between neighbouring layers that cross, from the output alone (edges
/// that span more than one layer left out).
fn crossings(p: &[(f64, f64)], edges: &[(usize, usize)]) -> usize {
    let short: Vec<_> = edges
        .iter()
        .filter(|&&(a, b)| p[a].1 - p[b].1 == 1.0)
        .collect();
    let mut c = 0;
    for (i, &&(a, b)) in short.iter().enumerate() {
        for &&(u, v) in &short[i + 1..] {
            let same_layer = p[a].1 == p[u].1;
            if same_layer && (p[a].0 - p[u].0) * (p[b].0 - p[v].0) < 0.0 {
                c += 1;
            }
        }
    }
    c
}

/// Every edge points down; nodes in one layer are at least a unit apart; each node's layer is
/// its longest path from a source.
fn assert_layered(n: usize, edges: &[(usize, usize)], p: &[(f64, f64)]) {
    assert_eq!(p.len(), n);
    for &(a, b) in edges {
        assert!(p[b].1 < p[a].1, "edge {a}->{b} doesn't point down");
    }
    for v in 0..n {
        let longest = edges
            .iter()
            .filter(|&&(_, b)| b == v)
            .map(|&(a, _)| 1.0 - p[a].1)
            .fold(0.0, f64::max);
        assert_eq!(-p[v].1, longest, "node {v}'s layer");
        for u in v + 1..n {
            if p[u].1 == p[v].1 {
                assert!((p[u].0 - p[v].0).abs() >= 1.0, "{u} and {v} overlap");
            }
        }
    }
}

#[test]
fn layered_puts_each_node_on_its_longest_path_from_a_source() {
    let edges = [(0, 1), (1, 2), (0, 2), (3, 2)];
    let p = layered(4, &edges);
    assert_layered(4, &edges, &p);
    assert_eq!([p[0].1, p[1].1, p[2].1, p[3].1], [0.0, -1.0, -2.0, 0.0]);
}

#[test]
fn layered_untangles_a_crossing() {
    // Two sources over two sinks, wired across.
    let edges = [(0, 3), (1, 2)];
    let p = layered(4, &edges);
    assert_layered(4, &edges, &p);
    assert_eq!(crossings(&p, &edges), 0);
    assert_eq!(p[3].0, -0.5);
    assert_eq!(p[2].0, 0.5);
}

#[test]
fn layered_untangles_a_shuffled_matching() {
    let mut rng = Rng::new(3);
    let mut sinks: Vec<usize> = (12..24).collect();
    for i in (1..sinks.len()).rev() {
        sinks.swap(i, rng.below(i + 1));
    }
    let edges: Vec<(usize, usize)> = (0..12).zip(sinks).collect();
    let p = layered(24, &edges);
    assert_layered(24, &edges, &p);
    assert_eq!(crossings(&p, &edges), 0);
}

#[test]
fn layered_leaves_room_for_a_long_edge() {
    // A chain 0-1-2-3 and a long edge 0->3: the long edge's dummies sit beside 1 and 2, so the
    // chain moves aside from the straight line between 0 and 3.
    let edges = [(0, 1), (1, 2), (2, 3), (0, 3)];
    let p = layered(4, &edges);
    assert_layered(4, &edges, &p);
    assert_eq!((p[1].0 - p[0].0).abs(), 0.5);
    assert_eq!((p[2].0 - p[3].0).abs(), 0.5);
}

#[test]
fn layered_breaks_cycles_and_ignores_loops_and_bad_ends() {
    let p = layered(3, &[(0, 1), (1, 2), (2, 0), (1, 1), (2, 7)]);
    assert_layered(3, &[(0, 1), (1, 2)], &p);
}

#[test]
fn layered_keeps_its_invariants_on_random_dags() {
    for s in 0..20 {
        let mut rng = Rng::new(100 + s);
        let n = 2 + rng.below(120);
        let m = rng.below(2 * n);
        let edges: Vec<(usize, usize)> = (0..m)
            .map(|_| {
                let a = rng.below(n - 1);
                (a, a + 1 + rng.below((n - a - 1).min(8)))
            })
            .collect();
        let p = layered(n, &edges);
        assert_layered(n, &edges, &p);
        assert_eq!(p, layered(n, &edges), "seed {s} isn't deterministic");
    }
}

#[test]
fn layered_of_no_node_and_of_isolated_ones() {
    assert!(layered(0, &[]).is_empty());
    assert_eq!(layered(3, &[]), vec![(-1.0, 0.0), (0.0, 0.0), (1.0, 0.0)]);
}

/// Each edge's route: from its `from` end through its bends to its `to` end, one layer a step
/// and never back, meeting each layer it crosses at its own slot, a unit from every node there.
/// Empty for a bad edge and one between neighbouring layers. Past the budget, the long edges
/// with bends span fewer layers than those without, which it counts.
fn assert_routes(
    n: usize,
    edges: &[(usize, usize)],
    p: &[(f64, f64)],
    routes: &[Vec<(f64, f64)>],
) -> usize {
    assert_eq!(routes.len(), edges.len());
    let (mut routed, mut straight, mut unrouted) = (0.0f64, f64::INFINITY, 0);
    for (&(a, b), bends) in edges.iter().zip(routes) {
        if a >= n || b >= n || a == b {
            assert!(bends.is_empty(), "edge {a}->{b} has bends");
            continue;
        }
        let span = (p[a].1 - p[b].1).abs();
        if bends.is_empty() && span > 1.0 {
            straight = straight.min(span);
            unrouted += 1;
            continue;
        }
        if span > 1.0 {
            routed = routed.max(span);
        }
        assert_eq!(bends.len() as f64, span - 1.0, "edge {a}->{b}'s bends");
        let mut line = vec![p[a]];
        line.extend(bends);
        line.push(p[b]);
        let step = (p[b].1 - p[a].1).signum();
        for w in line.windows(2) {
            assert_eq!(w[1].1 - w[0].1, step, "edge {a}->{b} skips a layer");
        }
        for (v, q) in p.iter().enumerate() {
            for bend in bends {
                if bend.1 == q.1 {
                    assert!((bend.0 - q.0).abs() >= 1.0, "edge {a}->{b} meets node {v}");
                }
            }
        }
    }
    assert!(
        routed < straight,
        "an edge spanning {straight} has no bends, one spanning {routed} has"
    );
    unrouted
}

#[test]
fn layered_routes_a_long_edge_round_the_nodes_it_passes() {
    // A probe's graph, where 12 -> 13 drawn straight ran through node 9.
    let edges = [
        (0, 2),
        (7, 13),
        (8, 9),
        (12, 13),
        (1, 5),
        (12, 13),
        (4, 9),
        (9, 13),
        (9, 13),
        (8, 13),
        (10, 11),
        (10, 11),
        (0, 2),
        (2, 5),
        (10, 11),
        (0, 6),
        (7, 12),
        (1, 7),
        (0, 3),
        (6, 10),
        (2, 3),
        (7, 11),
        (6, 8),
        (5, 6),
        (6, 8),
    ];
    let Routed {
        nodes: p,
        bends: routes,
    } = layered_routes(14, &edges);
    assert_eq!(p, layered(14, &edges));
    assert_layered(14, &edges, &p);
    assert_eq!(assert_routes(14, &edges, &p, &routes), 0);
    assert_eq!((p[12].0, p[9].0, p[13].0), (0.0, 0.0, 0.0));
    assert_eq!(routes[3].len(), 3, "12 -> 13 spans four layers");
    assert_eq!(routes[5], routes[3], "a duplicate takes the first's bends");
    assert!(routes[3].iter().all(|q| q.0 != 0.0));
}

#[test]
fn layered_routes_keep_their_invariants_on_random_graphs() {
    for s in 0..60 {
        let mut rng = Rng::new(900 + s);
        let n = 2 + rng.below(60);
        let m = rng.below(3 * n);
        let edges: Vec<(usize, usize)> = (0..m)
            .map(|_| {
                let a = rng.below(n);
                if s % 2 == 0 {
                    // Cycles, loops and ends past the nodes.
                    (a, rng.below(n + 2))
                } else {
                    (a, a + 1 + rng.below(6))
                }
            })
            .collect();
        let Routed {
            nodes: p,
            bends: routes,
        } = layered_routes(n, &edges);
        assert_eq!(p, layered(n, &edges), "seed {s}");
        assert_eq!(assert_routes(n, &edges, &p, &routes), 0, "seed {s}");
    }
}

#[test]
fn layered_routes_run_an_edge_turned_round_back_up() {
    // 0 -> 1 -> 2 -> 3 and 3 -> 0, which the layout turns round: its route climbs from 3.
    let edges = [(0, 1), (1, 2), (2, 3), (3, 0), (2, 2), (0, 9)];
    let Routed {
        nodes: p,
        bends: routes,
    } = layered_routes(4, &edges);
    assert_eq!(assert_routes(4, &edges, &p, &routes), 0);
    assert_eq!(
        routes[3].iter().map(|q| q.1).collect::<Vec<_>>(),
        [-2.0, -1.0]
    );
    assert!(routes[..3].iter().chain(&routes[4..]).all(Vec::is_empty));
}

#[test]
fn layered_keeps_to_its_dummy_budget() {
    // Every edge of a 300-node complete DAG: longest paths put node i in layer i, and its edges
    // would need four and a half million dummies.
    let n = 300;
    let edges: Vec<(usize, usize)> = (0..n)
        .flat_map(|a| (a + 1..n).map(move |b| (a, b)))
        .collect();
    let Routed {
        nodes: p,
        bends: routes,
    } = layered_routes(n, &edges);
    assert!((0..n).all(|v| p[v].1 == -(v as f64)));
    assert!(assert_routes(n, &edges, &p, &routes) > 0);
    let bends: usize = routes.iter().map(Vec::len).sum();
    assert!(
        bends <= DUMMY_BUDGET && bends > DUMMY_BUDGET / 2,
        "{bends} bends"
    );
}

#[test]
#[ignore = "speed"]
fn layered_lays_out_a_dense_dag_and_a_fan_quickly() {
    let complete: Vec<(usize, usize)> = (0..300)
        .flat_map(|a| (a + 1..300).map(move |b| (a, b)))
        .collect();
    let mut fan: Vec<(usize, usize)> = (1..2000).map(|i| (i - 1, i)).collect();
    fan.extend((2..2000).map(|i| (0, i)));
    for (name, n, edges) in [
        ("complete 300", 300, complete),
        ("chain and fan 2,000", 2000, fan),
    ] {
        let t = Instant::now();
        let p = layered_routes(n, &edges).nodes;
        let took = t.elapsed();
        println!("{name}: {took:?}");
        assert_eq!(p.len(), n);
        assert!(took.as_millis() < 300, "{name} took {took:?}");
    }
}

// ------------------------------------------------------------------------------------ lttb

#[test]
fn lttb_keeps_figs_points_from_twenty() {
    let xs: Vec<f64> = (0..20).map(f64::from).collect();
    let ys: Vec<f64> = (0..20)
        .map(|i| f64::from((i * 7) % 5) - f64::from(i) / 10.0)
        .collect();
    assert_eq!(lttb(&xs, &ys, 6), vec![0, 2, 5, 12, 15, 19]);
}

#[test]
fn lttb_keeps_figs_points_from_a_thousand_sine_samples() {
    let xs: Vec<f64> = (0..1000).map(f64::from).collect();
    let ys: Vec<f64> = xs.iter().map(|x| (x / 20.0).sin()).collect();
    let kept = lttb(&xs, &ys, 69);
    assert_eq!(kept.len(), 69);
    assert_eq!(kept[..8], [0, 13, 26, 39, 50, 73, 86, 99]);
    assert_eq!(kept.last(), Some(&999));
}

#[test]
fn lttb_keeps_everything_under_three_or_at_the_length() {
    let xs = [0.0, 1.0, 2.0, 3.0];
    let ys = [1.0, 3.0, 2.0, 5.0];
    for t in [0, 1, 2, 4, 9] {
        assert_eq!(lttb(&xs, &ys, t), vec![0, 1, 2, 3], "threshold {t}");
    }
    assert_eq!(lttb(&xs, &ys, 3), vec![0, 2, 3]);
}

// ---------------------------------------------------------------------------------- search

#[test]
fn search_scores_match_figs() {
    assert_eq!(score("API", "api"), Some((0, 0)));
    assert_eq!(score("API server", "api"), Some((1, 10)));
    assert_eq!(score("Application", "api"), Some((3, 2)));
    assert_eq!(score("Database", "dbs"), Some((3, 4)));
    assert_eq!(score("Worker", "or"), Some((2, 1)));
    assert_eq!(score("cache", "x"), None);
    assert_eq!(score("Ab", ""), Some((0, 0)));
}

#[test]
fn search_tiers_put_exact_before_prefix_before_substring_before_subsequence() {
    assert_eq!(tier("api", "API"), 0);
    assert_eq!(tier("api", "API server"), 1);
    assert_eq!(tier("or", "Worker"), 2);
    assert_eq!(tier("api", "Application"), 3);
    assert_eq!(tier("x", "cache"), UNMATCHED);
    assert_eq!(tier("", "anything"), 0);
    let labels = ["Application", "api gateway", "API", "rapid"];
    let mut ranked = labels.to_vec();
    ranked.sort_by_key(|l| tier("api", l));
    assert_eq!(ranked, ["API", "api gateway", "rapid", "Application"]);
}

#[test]
fn search_tiers_fold_case_a_character_at_a_time_as_flex_does() {
    // Lowercased whole, "ΟΔΟΣ" ends in a final sigma, so fig's score finds no "σ" in it; flex
    // folds each character on its own and keeps it, and so does the tier.
    assert_eq!(score("ΟΔΟΣ", "σ"), None);
    assert_eq!(tier("σ", "ΟΔΟΣ"), 2);
    assert_eq!(tier("Σ", "ΟΔΟΣ"), 2);
    assert_eq!(tier("οδοσ", "ΟΔΟΣ"), 0);
    // Every label flex keeps has a tier fig ranks.
    let alphabet = ['a', 'A', 'o', 'σ', 'Σ', 'ς', 'Ο', 'δ', 'i', 'İ', 'ß', ' '];
    let mut rng = Rng::new(77);
    let word = |rng: &mut Rng, most: usize| -> String {
        (0..1 + rng.below(most))
            .map(|_| alphabet[rng.below(alphabet.len())])
            .collect()
    };
    let mut kept = 0;
    for _ in 0..4000 {
        let (q, l) = (word(&mut rng, 3), word(&mut rng, 8));
        if !die_core::flex_rank(&q, &[l.as_str()]).is_empty() {
            kept += 1;
            assert!(tier(&q, &l) < UNMATCHED, "flex keeps {l:?} for {q:?}");
        }
    }
    assert!(kept > 500, "{kept}");
}

// --------------------------------------------------------------------------------- pyramid

/// The envelope by definition: every sample in its column, scanned in order.
fn brute(xs: &[f64], ys: &[f64], x0: f64, x1: f64, columns: usize) -> Vec<[u32; 4]> {
    let mut out = vec![EMPTY; columns];
    for (i, (&x, &y)) in xs.iter().zip(ys).enumerate() {
        let Some(c) = column(x, x0, x1, columns).filter(|_| !y.is_nan()) else {
            continue;
        };
        let (o, i) = (&mut out[c], i as u32);
        if o[0] == u32::MAX {
            *o = [i; 4];
            continue;
        }
        if y < ys[o[1] as usize] {
            o[1] = i;
        }
        if y > ys[o[2] as usize] {
            o[2] = i;
        }
        o[3] = i;
    }
    out
}

/// x ascending with uneven steps (repeats, small steps, jumps), y a walk with ties (whole
/// numbers some of the time) and runs of NaN short and long.
fn series(rng: &mut Rng, n: usize) -> (Vec<f64>, Vec<f64>) {
    let (mut x, mut y) = (rng.unit() * 100.0 - 50.0, 0.0f64);
    let whole = rng.below(2) == 0;
    let (mut xs, mut ys) = (Vec::with_capacity(n), Vec::with_capacity(n));
    let mut nan_left = 0;
    for _ in 0..n {
        x += match rng.below(10) {
            0 => 0.0,
            1 => rng.unit() * 50.0,
            _ => rng.unit(),
        };
        y += rng.unit() * 2.0 - 1.0;
        if nan_left == 0 && rng.below(60) == 0 {
            let longest = if rng.below(4) == 0 { 6 * LEAF } else { 4 };
            nan_left = 1 + rng.below(longest);
        }
        xs.push(x);
        if nan_left > 0 {
            nan_left -= 1;
            ys.push(f64::NAN);
        } else {
            ys.push(if whole { y.round() } else { y });
        }
    }
    (xs, ys)
}

#[test]
fn pyramid_envelope_equals_a_brute_force_scan_on_random_data() {
    let (mut cases, mut filled, mut spans) = (0, 0, 0);
    for s in 0..60 {
        let mut rng = Rng::new(s);
        let n = [0, 1, LEAF - 1, LEAF, LEAF + 1, 300, 1000, 5000][s as usize % 8] + rng.below(3);
        let (xs, ys) = series(&mut rng, n);
        let pyr = Pyramid::build(&xs, &ys);
        assert_eq!(pyr.len(), n);
        let (lo, hi) = match (xs.first(), xs.last()) {
            (Some(&a), Some(&b)) => (a, b),
            _ => (0.0, 1.0),
        };
        let span = (hi - lo).max(1.0);
        for _ in 0..6 {
            // Windows inside the data, around it, and partly off either end.
            let a = lo + span * (rng.unit() * 1.4 - 0.2);
            let b = a + span * rng.unit() * 1.2 + 1e-3;
            for columns in [1, 3, 64, 799, 2560, 2 * n + 1] {
                let got = pyr.envelope(&xs, &ys, a, b, columns);
                assert_eq!(
                    got,
                    brute(&xs, &ys, a, b, columns),
                    "seed {s}, {columns} columns"
                );
                cases += 1;
                filled += got.iter().filter(|c| **c != EMPTY).count();
                spans += got.iter().filter(|c| c[0] != c[3]).count();
            }
        }
        // The whole data, exactly.
        assert_eq!(
            pyr.envelope(&xs, &ys, lo, hi, 97),
            brute(&xs, &ys, lo, hi, 97)
        );
    }
    assert_eq!(cases, 60 * 6 * 6);
    println!("{filled} columns with samples, {spans} with more than one");
    assert!(
        filled > 50_000 && spans > 30_000,
        "{filled} filled, {spans} spanning"
    );
}

#[test]
fn pyramid_envelope_leaves_gaps_where_samples_are_missing() {
    let xs = [0.0, 1.0, 2.0, 3.0, 4.0, 5.0];
    let ys = [5.0, 1.0, f64::NAN, f64::NAN, 4.0, 4.0];
    let pyr = Pyramid::build(&xs, &ys);
    assert_eq!(
        pyr.envelope(&xs, &ys, 0.0, 6.0, 6),
        vec![[0; 4], [1; 4], EMPTY, EMPTY, [4; 4], [5; 4]]
    );
    // Three columns of two samples: the middle one has only missing ones.
    assert_eq!(
        pyr.envelope(&xs, &ys, 0.0, 6.0, 3),
        vec![[0, 1, 0, 1], EMPTY, [4, 4, 4, 5]]
    );
    // From 1 to 4 the line breaks; from 0 to 1 and 4 to 5 it doesn't.
    assert!(pyr.gap(1, 4) && pyr.gap(4, 1) && pyr.gap(0, 5));
    assert!(!pyr.gap(0, 1) && !pyr.gap(4, 5) && !pyr.gap(3, 3) && !pyr.gap(1, 2));
}

#[test]
fn pyramid_envelope_takes_the_first_of_equal_values() {
    let xs: Vec<f64> = (0..100).map(f64::from).collect();
    let ys: Vec<f64> = (0..100).map(|i| f64::from(i % 3)).collect();
    let pyr = Pyramid::build(&xs, &ys);
    assert_eq!(pyr.envelope(&xs, &ys, 0.0, 99.0, 1), vec![[0, 0, 2, 99]]);
    assert_eq!(
        pyr.envelope(&xs, &ys, 40.0, 99.0, 1),
        vec![[40, 42, 41, 99]]
    );
}

#[test]
fn pyramid_envelope_of_no_columns_or_a_bad_window_is_empty() {
    let xs = [0.0, 1.0, 2.0];
    let ys = [1.0, 2.0, 3.0];
    let pyr = Pyramid::build(&xs, &ys);
    assert!(pyr.envelope(&xs, &ys, 0.0, 2.0, 0).is_empty());
    for (a, b) in [
        (2.0, 0.0),
        (1.0, 1.0),
        (f64::NAN, 2.0),
        (0.0, f64::INFINITY),
    ] {
        assert_eq!(pyr.envelope(&xs, &ys, a, b, 4), vec![EMPTY; 4], "{a}..{b}");
    }
    assert!(Pyramid::build(&[], &[]).is_empty());
}

#[test]
fn column_splits_the_window_evenly_and_holds_both_ends() {
    assert_eq!(column(0.0, 0.0, 10.0, 5), Some(0));
    assert_eq!(column(1.999, 0.0, 10.0, 5), Some(0));
    assert_eq!(column(2.0, 0.0, 10.0, 5), Some(1));
    assert_eq!(column(10.0, 0.0, 10.0, 5), Some(4));
    assert_eq!(column(-0.001, 0.0, 10.0, 5), None);
    assert_eq!(column(10.001, 0.0, 10.0, 5), None);
    assert_eq!(column(f64::NAN, 0.0, 10.0, 5), None);
    assert_eq!(column(1.0, 0.0, 10.0, 0), None);
    assert_eq!(column(1.0, 1.0, 1.0, 5), None);
}

/// Merged y-intervals, sorted.
fn merged(mut v: Vec<(f64, f64)>) -> Vec<(f64, f64)> {
    v.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
    let mut out: Vec<(f64, f64)> = Vec::new();
    for (lo, hi) in v {
        match out.last_mut() {
            Some(last) if lo <= last.1 => last.1 = last.1.max(hi),
            _ => out.push((lo, hi)),
        }
    }
    out
}

/// What a line lights, as each column's merged y-intervals and the segments between columns
/// (index pairs, sorted): from `segments` and lone `points`, in the window's columns.
type Lit = (BTreeMap<usize, Vec<(f64, f64)>>, Vec<(u32, u32)>);

fn lit(
    ys: &[f64],
    col: impl Fn(u32) -> Option<usize>,
    points: &[u32],
    segments: &[(u32, u32)],
) -> Lit {
    let (mut cols, mut across) = (BTreeMap::<usize, Vec<(f64, f64)>>::new(), Vec::new());
    for &i in points {
        let y = ys[i as usize];
        cols.entry(col(i).unwrap()).or_default().push((y, y));
    }
    for &(a, b) in segments {
        let (ya, yb) = (ys[a as usize], ys[b as usize]);
        match (col(a), col(b)) {
            (Some(c), Some(d)) if c == d => {
                cols.entry(c).or_default().push((ya.min(yb), ya.max(yb)))
            }
            _ => across.push((a.min(b), a.max(b))),
        }
    }
    let cols = cols.into_iter().map(|(c, v)| (c, merged(v))).collect();
    across.sort_unstable();
    across.dedup();
    (cols, across)
}

/// The line through every sample, breaking at each missing one, within [x0, x1] and on to the
/// nearest samples outside it.
fn whole_line(xs: &[f64], ys: &[f64], x0: f64, x1: f64, columns: usize) -> Lit {
    let inside = |i: usize| column(xs[i], x0, x1, columns).is_some();
    let present = |i: usize| !ys[i].is_nan();
    let points: Vec<u32> = (0..xs.len())
        .filter(|&i| inside(i) && present(i))
        .map(|i| i as u32)
        .collect();
    let segments: Vec<(u32, u32)> = (1..xs.len())
        .filter(|&i| present(i - 1) && present(i) && (inside(i - 1) || inside(i)))
        .map(|i| (i as u32 - 1, i as u32))
        .collect();
    // A segment from outside across the whole window, when no sample is in it.
    let lo = xs.partition_point(|&x| x < x0);
    let mut segments = segments;
    if lo > 0 && lo < xs.len() && !inside(lo) && present(lo - 1) && present(lo) {
        segments.push((lo as u32 - 1, lo as u32));
    }
    lit(
        ys,
        |i| column(xs[i as usize], x0, x1, columns),
        &points,
        &segments,
    )
}

/// What [`Pyramid::line`] draws, by its rule, with [`Pyramid::beyond`] at the ends; and every
/// segment it draws checked against the raw samples for a gap.
fn drawn_line(pyr: &Pyramid, xs: &[f64], ys: &[f64], x0: f64, x1: f64, columns: usize) -> Lit {
    let pieces = pyr.line(xs, ys, x0, x1, columns);
    let (below, above) = pyr.beyond(xs, x0, x1);
    let (mut points, mut segments) = (Vec::new(), Vec::new());
    let mut last: Option<u32> = below;
    for &(c, m4) in &pieces {
        let mut four = m4.to_vec();
        four.sort_unstable();
        four.dedup();
        assert!(
            four.iter()
                .all(|&i| column(xs[i as usize], x0, x1, columns) == Some(c as usize))
        );
        points.extend(&four);
        segments.extend(four.windows(2).map(|w| (w[0], w[1])));
        if let Some(l) = last.filter(|&l| !pyr.gap(l, four[0])) {
            segments.push((l, four[0]));
        }
        last = Some(four[four.len() - 1]);
    }
    if let (Some(l), Some(a)) = (last, above)
        && !pyr.gap(l, a)
    {
        segments.push((l, a));
    }
    for &(a, b) in &segments {
        let (a, b) = (a.min(b) as usize, a.max(b) as usize);
        assert!(
            ys[a..=b].iter().all(|y| !y.is_nan()),
            "{a}..{b} is drawn across a gap"
        );
    }
    lit(
        ys,
        |i| column(xs[i as usize], x0, x1, columns),
        &points,
        &segments,
    )
}

#[test]
fn pyramid_line_lights_what_the_line_through_every_sample_lights() {
    let mut split = 0;
    for s in 0..60 {
        let mut rng = Rng::new(300 + s);
        let n = [1, LEAF + 1, 300, 1000, 4000][s as usize % 5] + rng.below(3);
        let (xs, mut ys) = series(&mut rng, n);
        for y in &mut ys {
            // Spikes, which a column's four must keep.
            if rng.below(97) == 0 {
                *y += 25.0;
            }
        }
        let pyr = Pyramid::build(&xs, &ys);
        let (lo, hi) = (xs[0], xs[n - 1]);
        let span = (hi - lo).max(1.0);
        for _ in 0..6 {
            let a = lo + span * (rng.unit() * 1.4 - 0.2);
            let b = a + span * rng.unit() * 1.2 + 1e-3;
            for columns in [1, 3, 64, 799, 2 * n + 1] {
                assert_eq!(
                    drawn_line(&pyr, &xs, &ys, a, b, columns),
                    whole_line(&xs, &ys, a, b, columns),
                    "seed {s}, {columns} columns over {a}..{b}"
                );
                let pieces = pyr.line(&xs, &ys, a, b, columns);
                split += pieces.windows(2).filter(|w| w[0].0 == w[1].0).count();
            }
        }
    }
    assert!(split > 1000, "only {split} columns split by a gap");
}

#[test]
fn pyramid_line_splits_a_column_at_a_missing_sample() {
    // One column over 0, 10, a gap, 5, -3: the envelope's four span the gap, and a line through
    // them broken at it never reaches -3; the line's two pieces do.
    let xs = [0.0, 1.0, 2.0, 3.0, 4.0];
    let ys = [0.0, 10.0, f64::NAN, 5.0, -3.0];
    let pyr = Pyramid::build(&xs, &ys);
    assert_eq!(pyr.envelope(&xs, &ys, 0.0, 4.0, 1), vec![[0, 4, 1, 4]]);
    assert_eq!(
        pyr.line(&xs, &ys, 0.0, 4.0, 1),
        vec![(0, [0, 0, 1, 1]), (0, [3, 4, 3, 4])]
    );
    assert_eq!(
        pyr.line(&xs, &ys, 0.0, 4.0, 5),
        vec![(0, [0; 4]), (1, [1; 4]), (3, [3; 4]), (4, [4; 4])]
    );
    assert!(pyr.line(&xs, &ys, 4.0, 0.0, 1).is_empty());
    assert!(pyr.line(&xs, &ys, 0.0, 4.0, 0).is_empty());
}

#[test]
fn pyramid_line_keeps_every_spike_among_scattered_missing_samples() {
    // Flat, a spike to 10 in each column of 256 samples, one sample in 64 missing (widths in
    // powers of two, so no sample rounds into its neighbour's column).
    let n = 1 << 14;
    let xs: Vec<f64> = (0..n).map(f64::from).collect();
    let ys: Vec<f64> = (0..n)
        .map(|i| match (i % 64, i % 256) {
            (63, _) => f64::NAN,
            (_, 120) => 10.0,
            _ => f64::from(i % 3) * 0.01,
        })
        .collect();
    let pyr = Pyramid::build(&xs, &ys);
    let pieces = pyr.line(&xs, &ys, 0.0, f64::from(n), 64);
    assert_eq!(pieces.len(), 4 * 64, "four pieces a column");
    for c in 0..64u32 {
        let spike = c * 256 + 120;
        let piece = pieces.iter().find(|p| p.0 == c && p.1[2] == spike);
        assert!(
            piece.is_some_and(|p| p.1[0] < spike && spike < p.1[3]),
            "column {c}'s spike isn't drawn"
        );
    }
}

#[test]
fn pyramid_beyond_finds_the_present_samples_past_the_window() {
    let nan = f64::NAN;
    let xs = [0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, nan];
    let ys = [0.0, 1.0, 2.0, nan, 4.0, 5.0, 6.0, nan, 8.0, 9.0];
    let pyr = Pyramid::build(&xs, &ys);
    // Zoomed so the window's edges are missing samples: the line stops at the gaps.
    assert_eq!(pyr.beyond(&xs, 3.0, 7.0), (Some(2), Some(8)));
    assert!(pyr.gap(2, 4) && pyr.gap(6, 8));
    // A window between two samples, with none in it: the line crosses it from 0 to 1.
    assert_eq!(pyr.beyond(&xs, 0.2, 0.8), (Some(0), Some(1)));
    assert!(pyr.line(&xs, &ys, 0.2, 0.8, 4).is_empty() && !pyr.gap(0, 1));
    // Past the data's ends; the NaN x at the end is never a neighbour.
    assert_eq!(pyr.beyond(&xs, -5.0, 0.0), (None, Some(1)));
    assert_eq!(pyr.beyond(&xs, 7.5, 100.0), (Some(6), None));
    assert_eq!(pyr.beyond(&xs, 5.0, 4.0), (None, None));
    assert_eq!(pyr.beyond(&xs, nan, 4.0), (None, None));
    // Against a scan on random data.
    for s in 0..20 {
        let mut rng = Rng::new(700 + s);
        let n = 1 + rng.below(3000);
        let (xs, ys) = series(&mut rng, n);
        let pyr = Pyramid::build(&xs, &ys);
        let (lo, hi) = (xs[0], xs[xs.len() - 1]);
        for _ in 0..100 {
            let a = lo + (hi - lo + 2.0) * (rng.unit() * 1.2 - 0.1) - 1.0;
            let b = a + (hi - lo) * rng.unit() * 0.3 + 1e-6;
            let below = (0..xs.len()).rev().find(|&i| xs[i] < a && !ys[i].is_nan());
            let above = (0..xs.len()).find(|&i| xs[i] > b && !ys[i].is_nan());
            let want = (below.map(|i| i as u32), above.map(|i| i as u32));
            assert_eq!(pyr.beyond(&xs, a, b), want, "seed {s}, {a}..{b}");
        }
    }
}

#[test]
#[ignore = "speed"]
fn pyramid_envelope_of_a_million_samples_at_2560_columns() {
    let n = 1_000_000;
    let mut rng = Rng::new(42);
    let (xs, ys) = series(&mut rng, n);
    let t = Instant::now();
    let pyr = Pyramid::build(&xs, &ys);
    let build = t.elapsed();
    let (lo, hi) = (xs[0], xs[n - 1]);
    let mut worst = std::time::Duration::ZERO;
    let mut env = Vec::new();
    for k in 0..20 {
        // Zoomed out, then ever further in, as a wheel does.
        let w = (hi - lo) / f64::from(1 << (k % 10));
        let t = Instant::now();
        env = pyr.envelope(&xs, &ys, lo, lo + w, 2560);
        worst = worst.max(t.elapsed());
    }
    let mut worst_line = std::time::Duration::ZERO;
    for k in 0..20 {
        let w = (hi - lo) / f64::from(1 << (k % 10));
        let t = Instant::now();
        let pieces = pyr.line(&xs, &ys, lo, lo + w, 2560);
        worst_line = worst_line.max(t.elapsed());
        assert!(pieces.len() >= env.iter().filter(|c| **c != EMPTY).count() / 2);
    }
    println!(
        "1M samples: build {build:?}, worst envelope at 2,560 columns {worst:?}, worst line \
         {worst_line:?}"
    );
    assert_eq!(env, brute(&xs, &ys, lo, lo + (hi - lo) / 512.0, 2560));
    assert!(worst.as_millis() < 20, "an envelope took {worst:?}");
    assert!(worst_line.as_millis() < 20, "a line took {worst_line:?}");
}

// ------------------------------------------------------------------------------------- hit

fn key(name: &str) -> SeriesKey {
    SeriesKey::new(name, 0)
}

fn index_with(name: &str, xs: &[f64], ys: &[f64]) -> HitIndex {
    let mut h = HitIndex::new();
    h.add_series(key(name), xs.into(), ys.into());
    h
}

#[test]
fn nearest_x_finds_the_nearest_present_sample() {
    let nan = f64::NAN;
    let h = index_with("a", &[0.0, 1.0, 2.0, 4.0, 8.0], &[1.0, 1.0, nan, 1.0, 1.0]);
    let at = |x| h.nearest_x(&key("a"), x);
    assert_eq!(at(0.4), Some(0));
    assert_eq!(at(2.1), Some(1), "the missing sample at 2 is passed over");
    assert_eq!(at(2.6), Some(3));
    assert_eq!(at(6.0), Some(3), "a tie goes to the earlier x");
    assert_eq!(at(-100.0), Some(0));
    assert_eq!(at(f64::INFINITY), Some(4));
    assert_eq!(at(nan), None);
    assert_eq!(h.nearest_x(&key("b"), 1.0), None);
    assert_eq!(h.nearest_x(&SeriesKey::new("a", 1), 1.0), None);
    let h = index_with("a", &[0.0, 1.0, 1.0, 1.0, 2.0], &[0.0, nan, 5.0, 6.0, 0.0]);
    assert_eq!(
        h.nearest_x(&key("a"), 1.2),
        Some(2),
        "the first present sample at that x"
    );
    let h = index_with("a", &[0.0, 1.0], &[nan, nan]);
    assert_eq!(h.nearest_x(&key("a"), 0.0), None);
}

#[test]
fn nearest_x_equals_a_brute_force_scan_on_random_data() {
    for s in 0..20 {
        let mut rng = Rng::new(500 + s);
        let n = 1 + rng.below(2000);
        let (xs, ys) = series(&mut rng, n);
        let h = index_with("s", &xs, &ys);
        let (lo, hi) = (xs[0], xs[xs.len() - 1]);
        for _ in 0..200 {
            let q = if rng.below(5) == 0 {
                xs[rng.below(xs.len())]
            } else {
                lo + (hi - lo + 2.0) * rng.unit() - 1.0
            };
            assert_eq!(
                h.nearest_x(&key("s"), q),
                nearest_by_scan(&xs, &ys, q),
                "seed {s}, x {q}"
            );
        }
    }
}

/// The nearest present sample's row by a scan: the earlier x of a tie, the first at one x.
fn nearest_by_scan(xs: &[f64], ys: &[f64], q: f64) -> Option<u32> {
    let mut want: Option<(f64, u32)> = None;
    for (i, (&x, &y)) in xs.iter().zip(ys).enumerate() {
        let d = (x - q).abs();
        if !y.is_nan() && want.is_none_or(|(wd, _)| d < wd) {
            want = Some((d, i as u32));
        }
    }
    want.map(|w| w.1)
}

#[test]
fn nearest_x_steps_over_long_gaps_and_shares_a_pyramids() {
    // 200,000 samples, present only at the ends and for a few in the middle.
    let n = 200_000usize;
    let xs: Vec<f64> = (0..n).map(|i| i as f64 * 0.5).collect();
    let ys: Vec<f64> = (0..n)
        .map(|i| match i {
            0..10 | 100_000..100_004 => 1.0,
            i if i >= n - 10 => 2.0,
            _ => f64::NAN,
        })
        .collect();
    let pyr = Pyramid::build(&xs, &ys);
    let mut shared = HitIndex::new();
    shared.add_series_with(key("s"), xs.clone().into(), ys.clone().into(), &pyr);
    // A pyramid over other samples isn't used for these.
    let other = Pyramid::build(&xs[..10], &ys[..10]);
    let mut stale = HitIndex::new();
    stale.add_series_with(key("s"), xs.clone().into(), ys.clone().into(), &other);
    let plain = index_with("s", &xs, &ys);
    let mut rng = Rng::new(8);
    for _ in 0..40 {
        let q = (rng.unit() * 1.1 - 0.05) * n as f64 * 0.5;
        let want = nearest_by_scan(&xs, &ys, q);
        for h in [&plain, &shared, &stale] {
            assert_eq!(h.nearest_x(&key("s"), q), want, "x {q}");
        }
    }
    // Samples 9 and 100,000 are at 4.5 and 50,000, which are equally far from 25,002.25.
    let at = |x| plain.nearest_x(&key("s"), x);
    assert_eq!(at(20_000.0), Some(9));
    assert_eq!(at(25_002.25), Some(9), "a tie goes to the earlier x");
    assert_eq!(at(25_002.35), Some(100_000));
    assert_eq!(at(60_000.0), Some(100_003));
}

#[test]
#[ignore = "speed"]
fn nearest_x_in_a_gap_of_two_million_samples() {
    let n = 2_000_000usize;
    let xs: Vec<f64> = (0..n).map(|i| i as f64).collect();
    let ys: Vec<f64> = (0..n)
        .map(|i| {
            if (10..n - 10).contains(&i) {
                f64::NAN
            } else {
                1.0
            }
        })
        .collect();
    let h = index_with("s", &xs, &ys);
    let t = Instant::now();
    for q in 0..10_000 {
        assert_eq!(
            h.nearest_x(&key("s"), (n / 2 + q) as f64),
            Some(n as u32 - 10)
        );
    }
    let each = t.elapsed() / 10_000;
    println!("nearest x in a gap of two million: {each:?} a query");
    assert!(each.as_micros() < 20, "{each:?} a query");
}

#[test]
fn nearest_point_equals_a_brute_force_scan_across_cells() {
    let mut rng = Rng::new(9);
    let mut h = HitIndex::new();
    let mut pts = Vec::new();
    for i in 0..600u32 {
        let k = SeriesKey::new(["a", "b", "a"][i as usize % 3], u32::from(i % 3 == 2));
        // Clustered on a few spots (so ties happen) and spread over negative coordinates.
        let (x, y) = if i % 7 == 0 {
            (f32::from((i % 5) as u8) * 10.0, 3.0)
        } else {
            (
                (rng.unit() * 900.0 - 300.0) as f32,
                (rng.unit() * 600.0 - 200.0) as f32,
            )
        };
        h.add_point(k.clone(), i, x, y);
        pts.push((k, i, x, y));
    }
    h.add_point(key("a"), 9999, f32::NAN, 0.0);
    for _ in 0..2000 {
        let (x, y) = (
            (rng.unit() * 1000.0 - 350.0) as f32,
            (rng.unit() * 700.0 - 250.0) as f32,
        );
        let reach = [0.0, 4.0, 12.0, HIT_DENSE, 75.0, f32::INFINITY][rng.below(6)];
        let mut want: Option<(f64, usize)> = None;
        for (j, p) in pts.iter().enumerate() {
            let (dx, dy) = (f64::from(p.2) - f64::from(x), f64::from(p.3) - f64::from(y));
            let d = dx * dx + dy * dy;
            if d <= f64::from(reach) * f64::from(reach) && want.is_none_or(|(wd, _)| d <= wd) {
                want = Some((d, j));
            }
        }
        let want = want.map(|(_, j)| (pts[j].0.clone(), pts[j].1));
        assert_eq!(
            h.nearest_point(x, y, reach),
            want,
            "at {x},{y} within {reach}"
        );
    }
    // Exactly on a clustered spot: the last one added there wins.
    let last = pts
        .iter()
        .rev()
        .find(|p| (p.2, p.3) == (10.0, 3.0))
        .unwrap();
    assert_eq!(
        h.nearest_point(10.0, 3.0, 0.0),
        Some((last.0.clone(), last.1))
    );
    assert_eq!(h.nearest_point(f32::NAN, 0.0, 10.0), None);
    assert_eq!(h.nearest_point(0.0, 0.0, -1.0), None);
}

#[test]
fn nearest_segment_finds_edges_within_reach() {
    let mut h = HitIndex::new();
    h.add_segment(1, (0.0, 0.0), (100.0, 0.0));
    h.add_segment(2, (50.0, -50.0), (50.0, 50.0));
    h.add_segment(3, (200.0, 200.0), (200.0, 200.0));
    h.add_segment(4, (f32::NAN, 0.0), (1.0, 1.0));
    assert_eq!(h.nearest_segment(20.0, 3.0, 4.0), Some(1));
    assert_eq!(h.nearest_segment(20.0, 5.0, 4.0), None);
    assert_eq!(h.nearest_segment(52.0, 30.0, 4.0), Some(2));
    assert_eq!(
        h.nearest_segment(50.0, 0.0, 4.0),
        Some(2),
        "a tie goes to the later edge"
    );
    assert_eq!(
        h.nearest_segment(103.0, 0.0, 4.0),
        Some(1),
        "past an end, to the end"
    );
    assert_eq!(h.nearest_segment(202.0, 202.0, 4.0), Some(3));
    assert_eq!(
        h.nearest_segment(0.5, 0.5, 0.1),
        None,
        "a NaN edge can't be hit"
    );
}

#[test]
fn segment_distance_is_to_the_nearest_point_of_the_segment() {
    let (a, b) = ((0.0, 0.0), (10.0, 0.0));
    assert_eq!(segment_distance((5.0, 3.0), a, b), 3.0);
    assert_eq!(segment_distance((-3.0, 4.0), a, b), 5.0);
    assert_eq!(segment_distance((13.0, -4.0), a, b), 5.0);
    assert_eq!(segment_distance((3.0, 4.0), a, a), 5.0);
}
