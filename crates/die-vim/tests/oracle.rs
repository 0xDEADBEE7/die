//! Exit test (d): every Evil-oracle case passes, except the deliberate deviations
//! (docs/m0-plan.md §1).
//!
//! Each case runs through die-core with die-vim registered: the case's text and
//! cursor, its registers, its keys typed one by one, then text, cursor, state,
//! registers and errors compared with what your Evil did. An error has to come
//! after the same key as Evil's; its message isn't compared.
//!
//! - `oracle/deviations.json` names the cases die-vim deliberately doesn't match.
//!   They're left out here. Each gets a hand-written fixture in
//!   `oracle/hand-written/`, which runs here like an oracle case. It states a
//!   different result for the same keys, mode and input, and only a regex-syntax
//!   deviation may change the keys.
//! - `oracle/pending.txt` names the cases die-vim doesn't pass yet, by the work
//!   package they land with. A pending case that passes fails this test, and so
//!   does a failing case that isn't pending.
//! - `DIE_ORACLE_BLESS=1` rewrites `pending.txt` to the cases that fail now.
//!   Check the diff: a case that leaves it is done, one that joins it is a regression.
//!
//! The runner uses die-core's spike-era API until the public harness (K7) lands.
//! What it can't set up or check yet fails the case with a reason, so a case
//! never passes on something unchecked.

use die_core::{Ctx, RegKind, Register, State, parse_keys};
use die_vim::Vim;
use evil_oracle::{
    Case, DeviationKind, Deviations, RegisterKind, Run, State as EvilState, Tranche, fixtures_dir,
    load_fixtures, parse_pending, tranche,
};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;
use std::path::Path;

/// Window size for cases that don't give one (the batch run has no window).
const COLS: usize = 80;
const ROWS: usize = 24;

fn data(name: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/oracle")
        .join(name)
}

#[test]
fn oracle() {
    let fixtures = load_fixtures(&fixtures_dir()).expect("the fixtures load and validate");
    let deviations = Deviations::parse(&std::fs::read_to_string(data("deviations.json")).unwrap())
        .expect("deviations.json parses");
    let pending_path = data("pending.txt");
    let pending_text = std::fs::read_to_string(&pending_path).unwrap_or_default();
    let pending: BTreeMap<String, Tranche> = parse_pending(&pending_text)
        .expect("pending.txt parses")
        .into_iter()
        .map(|(t, c)| (c, t))
        .collect();

    let mut all = BTreeMap::new();
    for fx in &fixtures {
        for c in &fx.cases {
            all.insert(format!("{}/{}", fx.topic, c.id), (c, fx.run));
        }
    }
    let excluded: BTreeSet<&str> = deviations.cases().map(|d| d.case.as_str()).collect();
    let mut problems = Vec::new();

    // A hand-written fixture replaces a deviation's oracle case, under the same name.
    let hand =
        load_fixtures(&data("hand-written")).expect("the hand-written fixtures load and validate");
    for fx in &hand {
        for c in &fx.cases {
            let name = format!("{}/{}", fx.topic, c.id);
            let file = format!("hand-written/{}.json", fx.topic);
            let dev = deviations.cases().find(|d| d.case == name);
            if dev.is_none_or(|d| d.fixture.as_deref() != Some(file.as_str())) {
                problems.push(format!(
                    "{file}: {name} isn't a deviation that names this file"
                ));
            }
            if let Some(&(oracle, oracle_run)) = all.get(&name) {
                let kind = deviations
                    .rules
                    .iter()
                    .find(|r| r.cases.iter().any(|d| d.case == name))
                    .map(|r| r.kind);
                if !same_input((oracle, oracle_run), (c, fx.run), kind) {
                    problems.push(format!(
                        "{file}: {name} must keep its oracle case's keys, mode, run and input"
                    ));
                }
            }
            all.insert(format!("hand-written/{name}"), (c, fx.run));
        }
    }
    for d in deviations.cases() {
        if d.fixture.is_none() || !all.contains_key(&format!("hand-written/{}", d.case)) {
            problems.push(format!("{}: its hand-written fixture isn't there", d.case));
        }
    }
    for name in excluded.iter().chain(
        pending
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>()
            .iter(),
    ) {
        if !all.contains_key(*name) {
            problems.push(format!("{name}: no such case"));
        }
    }
    for name in &excluded {
        if pending.contains_key(*name) {
            problems.push(format!("{name}: both a deviation and pending"));
        }
    }

    let mut failing = BTreeMap::new();
    let mut passed_pending = Vec::new();
    let mut failed_new = Vec::new();
    for (name, (case, run)) in &all {
        if excluded.contains(name.as_str()) {
            continue;
        }
        match run_case(case, *run) {
            Ok(()) if pending.contains_key(name) => passed_pending.push(name.clone()),
            Ok(()) => {}
            Err(why) => {
                if !pending.contains_key(name) {
                    failed_new.push(format!("{name} ({}): {why}", case.keys));
                }
                failing.insert(name.clone(), tranche(case, *run));
            }
        }
    }

    let total = all.len() - excluded.len();
    let mut summary = format!(
        "oracle: {} of {total} cases pass ({} deviations excluded)",
        total - failing.len(),
        excluded.len()
    );
    for t in Tranche::ALL {
        let n = failing.values().filter(|&&x| x == t).count();
        let _ = write!(summary, ", {} {n} pending", t.name());
    }
    eprintln!("{summary}");

    if std::env::var_os("DIE_ORACLE_BLESS").is_some() {
        std::fs::write(&pending_path, pending_file(&failing)).unwrap();
        eprintln!(
            "oracle: wrote {} ({} left it, {} joined it)",
            pending_path.display(),
            passed_pending.len(),
            failed_new.len()
        );
        assert!(problems.is_empty(), "{}", problems.join("\n"));
        return;
    }
    for name in &passed_pending {
        problems.push(format!("{name} passes now: take it off pending.txt"));
    }
    problems.extend(
        failed_new
            .iter()
            .map(|f| format!("{f}\n  (not pending: a regression, or a new case)")),
    );
    assert!(problems.is_empty(), "\n{}\n", problems.join("\n"));
}

/// Whether a hand-written fixture runs what its oracle case runs, so that it only
/// changes the result. A regex-syntax deviation changes the keys, not the result.
fn same_input(oracle: (&Case, Run), hand: (&Case, Run), kind: Option<DeviationKind>) -> bool {
    let ((o, orun), (h, hrun)) = (oracle, hand);
    o.input == h.input
        && o.mode == h.mode
        && orun == hrun
        && (kind == Some(DeviationKind::RegexSyntax) || o.keys == h.keys)
}

fn pending_file(failing: &BTreeMap<String, Tranche>) -> String {
    let mut s = String::from(
        "# Oracle cases die-vim doesn't pass yet, by the work package they land with\n\
         # (docs/m0-plan.md §5). See tests/oracle.rs; DIE_ORACLE_BLESS=1 rewrites this file.\n",
    );
    for t in Tranche::ALL {
        let names: Vec<&String> = failing
            .iter()
            .filter(|(_, x)| **x == t)
            .map(|(n, _)| n)
            .collect();
        if names.is_empty() {
            continue;
        }
        let _ = write!(s, "\n# {} ({})\n", t.name(), names.len());
        for n in names {
            let _ = writeln!(s, "{} {n}", t.name());
        }
    }
    s
}

/// Run one case; Err says every way it differs from Evil, or what can't be checked yet.
fn run_case(case: &Case, run: Run) -> Result<(), String> {
    let input = &case.input;
    if input.visual.is_some() {
        return Err("can't start a case in visual state yet (the harness, K7)".into());
    }
    if input.clipboard.is_some() {
        return Err("no clipboard yet (K6)".into());
    }
    if run == Run::Tty || input.window.is_some() {
        return Err("can't set a window's top line yet (the harness, K7)".into());
    }
    run_in_window(case, COLS, ROWS)
}

/// Run one case in a window of this size. Err says every way it differs from Evil.
fn run_in_window(case: &Case, cols: usize, rows: usize) -> Result<(), String> {
    let (input, want) = (&case.input, &case.expected);
    let mut ctx = Ctx::new(vec![Box::new(Vim::default())]).map_err(|e| e.join("; "))?;
    let buf = ctx.create_text("case", &input.text);
    let w = ctx
        .open_window(buf, cols, rows)
        .map_err(|e| format!("{e:?}"))?;
    ctx.focus(w);
    let set = ctx
        .text_mut(buf)
        .map_err(|e| format!("{e:?}"))?
        .set_cursor(w, input.cursor.byte);
    if set != Some(input.cursor.byte) {
        return Err(format!(
            "the cursor can't start at byte {}: core put it at {set:?}",
            input.cursor.byte
        ));
    }
    for (name, r) in input.registers.iter().flatten() {
        ctx.set_register(
            reg_char(name)?,
            Register {
                kind: reg_kind(r.kind),
                text: r.text.clone(),
            },
        );
    }
    let before = ctx.messages.len();
    let mut errors_after = Vec::new();
    for (i, ev) in parse_keys(&case.keys)
        .map_err(|e| format!("keys {}: {e}", case.keys))?
        .into_iter()
        .enumerate()
    {
        let n = ctx.messages.len();
        ctx.input(ev);
        ctx.run_until_idle();
        errors_after.extend(std::iter::repeat_n(i + 1, ctx.messages.len() - n));
    }

    let mut diffs = Vec::new();
    let text = ctx.text(buf).map_err(|e| format!("{e:?}"))?;
    let got_text = text.rope().to_string();
    if got_text != want.text {
        diffs.push(format!("text {got_text:?}, want {:?}", want.text));
    }
    let cursor = text.cursor(w);
    if cursor != Some(want.cursor.byte) {
        diffs.push(format!(
            "cursor at byte {cursor:?}, want {} (line {}, col {})",
            want.cursor.byte, want.cursor.line, want.cursor.col
        ));
    }
    match (want.state, ctx.state()) {
        (EvilState::Normal, State::Normal)
        | (EvilState::Insert, State::Insert)
        | (EvilState::Visual, State::Visual)
        | (EvilState::Operator, State::OperatorPending) => {}
        (EvilState::Replace, s) => diffs.push(format!(
            "state {s:?}, want replace, which core has no state for yet (K3)"
        )),
        (want, got) => diffs.push(format!("state {got:?}, want {want:?}")),
    }
    if want.visual.is_some() {
        diffs.push("can't compare a visual selection yet (V4 defines the mapping)".into());
    }
    for (name, r) in &want.registers {
        let got = ctx.register(reg_char(name)?);
        let want = r.as_ref().map(|r| Register {
            kind: reg_kind(r.kind),
            text: r.text.clone(),
        });
        if got != want.as_ref() {
            diffs.push(format!("register {name} is {got:?}, want {want:?}"));
        }
    }
    if want.clipboard.text.is_some() || !want.clipboard.writes.is_empty() {
        diffs.push("can't check the clipboard yet (K6)".into());
    }
    // A key die-vim doesn't bind is never the error Evil signalled, even when it
    // comes after the same key.
    let messages = &ctx.messages[before..];
    if let Some(m) = messages.iter().find(|m| m.ends_with(" is undefined")) {
        diffs.push(format!("die-vim doesn't bind a key: {m}"));
    }
    let want_after: Vec<usize> = want.errors.iter().flatten().map(|e| e.after_keys).collect();
    if errors_after != want_after {
        diffs.push(format!(
            "errors after keys {errors_after:?} ({messages:?}), want {want_after:?}"
        ));
    }
    if diffs.is_empty() {
        Ok(())
    } else {
        Err(diffs.join("; "))
    }
}

fn reg_char(name: &str) -> Result<char, String> {
    let mut c = name.chars();
    match (c.next(), c.next()) {
        (Some(ch), None) => Ok(ch),
        _ => Err(format!("register name {name:?}")),
    }
}

/// A recorded macro is held as its keys in notation, so it's charwise text.
fn reg_kind(k: RegisterKind) -> RegKind {
    match k {
        RegisterKind::Char | RegisterKind::Macro => RegKind::Char,
        RegisterKind::Line => RegKind::Line,
        RegisterKind::Block => RegKind::Block,
    }
}

/// The named case of a topic, fresh from its file, for a test to change.
fn fixture_case(dir: &Path, topic: &str, id: &str) -> (Case, Run) {
    let fx = load_fixtures(dir)
        .expect("the fixtures load and validate")
        .into_iter()
        .find(|f| f.topic == topic)
        .expect("the topic");
    let c = fx.cases.into_iter().find(|c| c.id == id).expect("the case");
    (c, fx.run)
}

#[test]
fn a_hand_written_fixture_must_keep_its_oracle_cases_input() {
    let (oracle, orun) = fixture_case(&fixtures_dir(), "motions", "w-last-word-of-buffer");
    let hand = || fixture_case(&data("hand-written"), "motions", "w-last-word-of-buffer");
    let keeps = |h: &Case, run: Run, kind| same_input((&oracle, orun), (h, run), kind);

    let (h, run) = hand();
    assert!(keeps(&h, run, None), "the committed fixture");

    let (mut h, run) = hand();
    h.input.cursor.byte = 0;
    assert!(!keeps(&h, run, None), "another cursor");

    let (mut h, run) = hand();
    h.input.text.push('x');
    assert!(!keeps(&h, run, None), "another text");

    let (mut h, run) = hand();
    h.mode = "text-mode".into();
    assert!(!keeps(&h, run, None), "another mode");

    let (h, _) = hand();
    assert!(!keeps(&h, Run::Tty, None), "another run");

    let (mut h, run) = hand();
    h.keys = "b".into();
    assert!(!keeps(&h, run, None), "other keys");
    assert!(
        keeps(&h, run, Some(DeviationKind::RegexSyntax)),
        "a regex-syntax deviation may change the keys"
    );
}

/// Every tty case waits for K7, which can set a window's top line. These don't
/// scroll or wrap, so their text, cursor, state and errors can be held to your
/// Evil now. `wrapped/` needs K4's word wrap as well.
#[test]
fn insert_state_arrows_match_the_tty_run() {
    let fixtures = load_fixtures(&fixtures_dir()).expect("the fixtures load and validate");
    let fx = fixtures.iter().find(|f| f.topic == "insert-tty").unwrap();
    assert_eq!(fx.cases.len(), 17);
    for c in &fx.cases {
        let size = c.input.window.as_ref().expect("a tty case's window");
        run_in_window(c, size.width, size.height)
            .unwrap_or_else(|why| panic!("insert-tty/{}: {why}", c.id));
    }
}

#[test]
fn an_error_must_come_after_the_same_key_as_evils() {
    let (case, run) = fixture_case(&fixtures_dir(), "motions", "count-l-past-end");
    assert_eq!(run_case(&case, run), Ok(()), "the committed case");

    let (mut case, run) = fixture_case(&fixtures_dir(), "motions", "count-l-past-end");
    case.expected.errors.as_mut().expect("an error")[0].after_keys = 1;
    let why = run_case(&case, run).expect_err("the error comes after key 2");
    assert!(why.contains("errors after keys [2]"), "{why}");
}
