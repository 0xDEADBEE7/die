//! Which die-vim work package a case lands with: the last feature its keys need.

use die_core::{key_notation, parse_keys};
use evil_oracle::{Case, Run, Tranche, parse_pending, tranche};

fn case(keys: &str) -> Case {
    let tokens: Vec<String> = parse_keys(keys)
        .unwrap()
        .iter()
        .map(|ev| key_notation(std::slice::from_ref(ev)))
        .collect();
    let tokens = serde_json::to_string(&tokens).unwrap();
    let json = format!(
        r#"{{"id":"x","source":"oracle","keys":{keys:?},"key_tokens":{tokens},"mode":"fundamental-mode",
          "input":{{"text":"a\n","cursor":{{"line":0,"col":0,"byte":0}}}},
          "expected":{{"text":"a\n","cursor":{{"line":0,"col":0,"byte":0}},"state":"normal",
            "registers":{{"\"":null}},"clipboard":{{"text":null,"writes":[]}}}}}}"#
    );
    serde_json::from_str(&json).unwrap()
}

fn t(keys: &str) -> Tranche {
    tranche(&case(keys), Run::Batch)
}

#[test]
fn keys_pick_the_tranche() {
    use Tranche::*;
    for (keys, want) in [
        ("w", V1),
        ("3fx;", V1),
        ("ifoo<Esc>u", V1),
        // Keys typed in insert state are text, not commands.
        ("idd/x<Esc>", V1),
        ("gg", V1),
        ("/foo<CR>n", V2),
        (":3<CR>", V2),
        ("dw", V3),
        ("cwfoo<Esc>u", V3),
        ("\"ayy", V3),
        ("diw", V3),
        ("ci(x<Esc>", V3),
        ("d/x<CR>", V3),
        ("vjd", V4),
        ("qaxq@a", V4),
        ("dw.", V4),
        ("Rxy<Esc>", V4),
        (":s/a/b/<CR>", V5),
        ("<C-d>", V6),
        ("zz", V6),
    ] {
        assert_eq!(t(keys), want, "{keys}");
    }
    assert_eq!(tranche(&case("w"), Run::Tty), Tranche::V6);
}

#[test]
fn pending_file_format() {
    let p = parse_pending("# comment\n\nV1 motions/w  # trailing\nV3 operators/dw\n").unwrap();
    assert_eq!(
        p,
        [
            (Tranche::V1, "motions/w".to_string()),
            (Tranche::V3, "operators/dw".to_string())
        ]
    );
    assert!(parse_pending("V9 x/y\n").is_err());
    assert!(parse_pending("V1 x/y\nV2 x/y\n").is_err());
    assert!(parse_pending("V1\n").is_err());
}
