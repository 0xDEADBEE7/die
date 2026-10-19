//! Loads every fixture the oracle wrote and checks it parses into the
//! schema and is self-consistent: the harness die-vim's tests will use.

use evil_oracle::{Case, Fixture, Pos, Run, Source, State, pos_of_byte};
use std::fs;
use std::path::Path;

fn load_all() -> Vec<(String, Fixture)> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures");
    let mut paths: Vec<_> = fs::read_dir(&dir)
        .expect("fixtures/ exists; run ./run-oracle.sh")
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .collect();
    paths.sort();
    paths
        .iter()
        .map(|p| {
            let stem = p.file_stem().unwrap().to_string_lossy().into_owned();
            let text = fs::read_to_string(p).unwrap();
            let fx: Fixture =
                serde_json::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
            (stem, fx)
        })
        .collect()
}

#[test]
fn every_fixture_parses_and_validates() {
    let all = load_all();
    assert!(!all.is_empty(), "no fixtures");
    let mut total = 0;
    for (stem, fx) in &all {
        fx.validate(stem)
            .unwrap_or_else(|e| panic!("{stem}.json: {e}"));
        total += fx.cases.len();
    }
    eprintln!("{} fixture files, {total} cases", all.len());
    assert!(total >= 50, "only {total} cases");
}

#[test]
fn fixtures_name_one_evil() {
    let all = load_all();
    let (_, first) = &all[0];
    for (stem, fx) in &all {
        assert_eq!(fx.evil_source_sha256, first.evil_source_sha256, "{stem}");
        assert_eq!(fx.evil_version, first.evil_version, "{stem}");
        assert_eq!(fx.emacs_version, first.emacs_version, "{stem}");
        assert!(
            fx.cases.iter().all(|c| c.source == Source::Oracle),
            "{stem}"
        );
    }
}

#[test]
fn tty_topics_carry_layout() {
    for (stem, fx) in load_all() {
        if fx.run == Run::Tty {
            for c in &fx.cases {
                let w = c.input.window.as_ref().unwrap();
                assert!(w.width > 0 && w.height > 0, "{stem}/{}", c.id);
                assert!(c.input.layout.is_some(), "{stem}/{}", c.id);
            }
        }
    }
}

#[test]
fn some_cases_end_outside_normal_state() {
    let all = load_all();
    let states: Vec<State> = all
        .iter()
        .flat_map(|(_, f)| f.cases.iter().map(|c| c.expected.state))
        .collect();
    for s in [State::Normal, State::Insert, State::Visual, State::Replace] {
        assert!(states.contains(&s), "no case ends in {s:?}");
    }
}

#[test]
fn key_tokens_are_how_die_core_reads_the_keys() {
    let check = |keys: &str, tokens: &[&str]| {
        let tokens = serde_json::to_string(tokens).unwrap();
        let json = format!(
            r#"{{"id":"x","source":"oracle","keys":{keys:?},"key_tokens":{tokens},"mode":"fundamental-mode",
              "input":{{"text":"a\n","cursor":{{"line":0,"col":0,"byte":0}}}},
              "expected":{{"text":"a\n","cursor":{{"line":0,"col":0,"byte":0}},"state":"normal",
                "registers":{{"\"":null}},"clipboard":{{"text":null,"writes":[]}}}}}}"#
        );
        serde_json::from_str::<Case>(&json)
            .unwrap()
            .validate(Run::Batch)
    };
    check("cwfoo<Esc>", &["c", "w", "f", "o", "o", "<Esc>"]).unwrap();
    check("<C-V>j<c-r>", &["<C-v>", "j", "<C-r>"]).unwrap();
    check("<<j>>", &["<", "<", "j", ">", ">"]).unwrap();
    check("i<lt>x<cr>", &["i", "<lt>", "x", "<CR>"]).unwrap();
    assert!(check("ab", &["ab"]).is_err());
    assert!(check("<Esc>", &["<", "E", "s", "c", ">"]).is_err());
    // Evil's side doesn't know <PageDown>, so it typed ten characters, and die-core reads one key.
    let literal = ["<", "P", "a", "g", "e", "D", "o", "w", "n", ">"];
    assert!(check("<PageDown>", &literal).is_err());
}

#[test]
fn positions_count_chars_not_bytes() {
    let t = "aé\nxü|";
    assert_eq!(
        pos_of_byte(t, 3).unwrap(),
        Pos {
            line: 0,
            col: 2,
            byte: 3
        }
    );
    assert_eq!(
        pos_of_byte(t, 4).unwrap(),
        Pos {
            line: 1,
            col: 0,
            byte: 4
        }
    );
    assert!(pos_of_byte(t, 2).is_err());
}

#[test]
fn hand_written_cases_need_a_reason() {
    let case = r#"{"id":"x","source":"hand-written","keys":"j","key_tokens":["j"],
      "mode":"fundamental-mode",
      "input":{"text":"a\nb\n","cursor":{"line":0,"col":0,"byte":0}},
      "expected":{"text":"a\nb\n","cursor":{"line":1,"col":0,"byte":2},"state":"normal",
        "registers":{"\"":null},"clipboard":{"text":null,"writes":[]}}}"#;
    let mut c: evil_oracle::Case = serde_json::from_str(case).unwrap();
    assert!(c.validate(Run::Batch).is_err());
    c.reason = Some("needs a GUI".into());
    c.validate(Run::Batch).unwrap();
}

#[test]
fn validator_catches_tampering() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/operators.json");
    let good: serde_json::Value = serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap();
    let tamper = |f: &dyn Fn(&mut serde_json::Value)| {
        let mut v = good.clone();
        f(&mut v["cases"][0]);
        let fx: Fixture = serde_json::from_value(v).unwrap();
        fx.validate("operators")
    };
    tamper(&|_| {}).unwrap();
    assert!(tamper(&|c| c["expected"]["cursor"]["byte"] = 99.into()).is_err());
    assert!(tamper(&|c| c["key_tokens"][0] = "x".into()).is_err());
    assert!(tamper(&|c| c["expected"]["state"] = "visual".into()).is_err());
    assert!(tamper(&|c| c["expected"]["clipboard"]["text"] = "nope".into()).is_err());
    assert!(
        tamper(&|c| {
            c["expected"]["registers"]
                .as_object_mut()
                .unwrap()
                .remove("\"");
        })
        .is_err()
    );
}
