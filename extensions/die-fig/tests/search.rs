//! Search's model: fig's ranking on fig's own cases (briefs/fig-port.md §6), the matched
//! characters a highlight draws, and `n` and `N` wrapping over the matches.

use die_test_cap as _;

use die_fig::data::SeriesKey;
use die_fig::detail::Item;
use die_fig::search::{Search, ranges};

fn item(i: usize) -> Item {
    Item::Series(SeriesKey::new(&format!("s{i}"), 0))
}

fn search(labels: &[&str], query: &str) -> Search {
    let mut s = Search::new(
        labels
            .iter()
            .enumerate()
            .map(|(i, l)| (l.to_string(), item(i)))
            .collect(),
    );
    s.set_query(query);
    s
}

fn order(s: &Search) -> Vec<&str> {
    s.matches().iter().map(|m| m.label.as_str()).collect()
}

#[test]
fn fig_s_scores_rank_exact_then_prefix_then_substring_then_subsequence() {
    // fig's golden values, through the model.
    let cases: &[(&str, &str, Option<(usize, usize)>)] = &[
        ("API", "api", Some((0, 0))),
        ("API server", "api", Some((1, 10))),
        ("Application", "api", Some((3, 2))),
        ("Database", "dbs", Some((3, 4))),
        ("Worker", "or", Some((2, 1))),
        ("cache", "x", None),
    ];
    for (label, query, want) in cases {
        let s = search(&[label], query);
        assert_eq!(
            s.matches().first().map(|m| m.score),
            *want,
            "{label} {query}"
        );
    }
    let s = search(
        &[
            "Application",
            "rapid",
            "API server",
            "cache",
            "API",
            "apiary",
        ],
        "api",
    );
    assert_eq!(
        order(&s),
        ["API", "apiary", "API server", "rapid", "Application"]
    );
    assert_eq!(s.best().map(|m| m.item.clone()), Some(item(4)));
    assert_eq!(s.matches()[0].index, 4);
}

#[test]
fn ties_go_by_the_label_s_byte_order_and_equal_labels_keep_their_order() {
    // An empty query keeps every label, in fig's tie-break: case-sensitive byte order.
    let s = search(&["alpha", "Zed", "beta", "Zed"], "");
    assert_eq!(order(&s), ["Zed", "Zed", "alpha", "beta"]);
    assert_eq!(s.matches()[0].index, 1);
    assert_eq!(s.matches()[1].index, 3);
    assert!(s.matches().iter().all(|m| m.ranges.is_empty()));
    // Within a tier, by the tie-break first: shorter prefixes, earlier substrings.
    let s = search(&["worker-pool", "work", "a-work", "xx-work"], "work");
    assert_eq!(order(&s), ["work", "worker-pool", "a-work", "xx-work"]);
}

#[test]
fn matches_carry_the_characters_they_matched() {
    assert_eq!(ranges("API server", "api"), [0..3]);
    assert_eq!(ranges("API", "api"), [0..3]);
    assert_eq!(ranges("Worker", "or"), [1..3]);
    assert_eq!(ranges("Application", "api"), [0..2, 4..5]);
    assert_eq!(ranges("Database", "dbs"), [0..1, 4..5, 6..7]);
    assert!(ranges("cache", "x").is_empty());
    assert!(ranges("cache", "").is_empty());
    // On character boundaries in the label as written, whatever lowercasing does to lengths.
    assert_eq!(ranges("İstanbul", "stan"), [2..6]);
    assert_eq!(ranges("ÉCOLE", "éc"), [0..3]);
    let s = search(&["us-east", "eu-west"], "east");
    assert_eq!(s.matches()[0].ranges, [3..7]);
}

#[test]
fn n_and_n_step_through_the_matches_and_wrap() {
    let mut s = search(&["api", "api-gw", "rapid", "cache"], "api");
    assert_eq!(order(&s), ["api", "api-gw", "rapid"]);
    // The best is current after a query: where the cursor went as you typed.
    assert_eq!(s.current().map(|m| m.label.as_str()), Some("api"));
    let next: Vec<String> = (0..4).map(|_| s.next().unwrap().label.clone()).collect();
    assert_eq!(next, ["api-gw", "rapid", "api", "api-gw"]);
    let prev: Vec<String> = (0..3).map(|_| s.prev().unwrap().label.clone()).collect();
    assert_eq!(prev, ["api", "rapid", "api-gw"]);
    // The cursor moved to a match some other way: n goes on from there.
    assert_eq!(s.select(&item(2)), Some(2));
    assert_eq!(s.next().map(|m| m.label.as_str()), Some("api"));
    assert_eq!(s.select(&item(3)), None, "cache isn't a match");
    assert_eq!(s.current().map(|m| m.label.as_str()), Some("api"));
    assert_eq!(
        s.match_of(&item(1)).map(|m| m.ranges.clone()),
        Some(vec![0..3])
    );
    // Nothing matches: nothing to step to.
    s.set_query("zzz");
    assert!(s.matches().is_empty());
    assert_eq!(s.current(), None);
    assert_eq!(s.next(), None);
    assert_eq!(s.prev(), None);
    assert_eq!(s.query(), "zzz");
    assert_eq!(s.names().len(), 4);
}

#[test]
fn nodes_are_selected_by_id_whatever_index_the_cursor_s_item_has() {
    let names = vec![
        (
            "db".to_string(),
            Item::Node {
                index: 0,
                id: "db".into(),
            },
        ),
        (
            "dbs".to_string(),
            Item::Node {
                index: 1,
                id: "dbs".into(),
            },
        ),
    ];
    let mut s = Search::new(names);
    s.set_query("db");
    assert_eq!(
        s.select(&Item::Node {
            index: u32::MAX,
            id: "dbs".into()
        }),
        Some(1)
    );
}
