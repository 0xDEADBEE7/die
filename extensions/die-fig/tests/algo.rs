//! die-fig's algorithms: fig's stress layout against golden positions and fig's own layout tests;
//! the tidy tree and the layered layout's invariants; fig's LTTB and search scores against golden
//! values; the pyramid's M4 envelope against a brute-force scan over random data; the hit index
//! against brute force. One timing test, ignored, measures a million samples' envelope.

use die_test_cap as _;

use die_fig::algo::hit::segment_distance;
use die_fig::algo::pyramid::{EMPTY, LEAF, column};
use die_fig::algo::search::{UNMATCHED, score};
use die_fig::algo::stress::seed;
use die_fig::algo::{HitIndex, Pyramid, layered, lttb, stress, tier, tree};
use die_fig::data::SeriesKey;
use die_fig::interact::consts::HIT_DENSE;
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
    println!("1M samples: build {build:?}, worst envelope at 2,560 columns {worst:?}");
    assert_eq!(env, brute(&xs, &ys, lo, lo + (hi - lo) / 512.0, 2560));
    assert!(worst.as_millis() < 20, "an envelope took {worst:?}");
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
            let mut want: Option<(f64, u32)> = None;
            for (i, (&x, &y)) in xs.iter().zip(&ys).enumerate() {
                let d = (x - q).abs();
                if !y.is_nan() && want.is_none_or(|(wd, _)| d < wd) {
                    want = Some((d, i as u32));
                }
            }
            assert_eq!(
                h.nearest_x(&key("s"), q),
                want.map(|w| w.1),
                "seed {s}, x {q}"
            );
        }
    }
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
