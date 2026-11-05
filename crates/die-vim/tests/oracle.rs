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
//!   deviation may change the keys. A case whose oracle run dies has no oracle case to replace:
//!   its rule's kind says so, and it runs here like any other hand-written fixture.
//! - `oracle/pending.txt` names the cases die-vim doesn't pass yet, by the work
//!   package they land with. A pending case that passes fails this test, and so
//!   does a failing case that isn't pending.
//! - `DIE_ORACLE_BLESS=1` rewrites `pending.txt` to the cases that fail now.
//!   Check the diff: a case that leaves it is done, one that joins it is a regression.
//!
//! The runner is core's public harness (K7). A tty case also sets its window's size and top
//! line, and is held to its top line after a frame per key, as the oracle redisplays per command.
//!
//! The clipboard is the harness's `FakeClipboard`. Your Evil writes every kill to it, so nearly every
//! case holds die-vim to its writes and to what it ends up holding.

use die_test_cap as _;

use die_core::{Ctx, Harness, RegKind, Register, Selection, Shape, State, TextBuf};
use die_vim::Vim;
use evil_oracle::{
    Case, DeviationKind, DeviationRule, Deviations, RegisterKind, Run, SelectionKind, State as EvilState, Tranche,
    fixtures_dir, load_fixtures, parse_pending, tranche,
};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;
use std::path::Path;

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
    let unrunnable = |r: &&DeviationRule| r.kind == DeviationKind::OracleDies;
    let excluded: BTreeSet<&str> = deviations
        .rules
        .iter()
        .filter(|r| !unrunnable(r))
        .flat_map(|r| &r.cases)
        .map(|d| d.case.as_str())
        .collect();
    let mut problems = Vec::new();
    for d in deviations.rules.iter().filter(unrunnable).flat_map(|r| &r.cases) {
        if all.contains_key(&d.case) {
            problems.push(format!("{}: the oracle does run this case", d.case));
        }
    }

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
        match run_case(case) {
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

/// Run one case; Err says every way it differs from Evil. A case that gives no window (the
/// batch run has none) gets the harness's own.
fn run_case(case: &Case) -> Result<(), String> {
    let (input, want) = (&case.input, &case.expected);
    let mut ctx = Harness::new(vec![Box::new(Vim::default())]).map_err(|e| e.join("; "))?;
    if let Some(size) = &input.window {
        ctx.set_window_size(size.width, size.height);
    }
    let (buf, w) = ctx
        .open_text("case", &input.text)
        .map_err(|e| format!("{e:?}"))?;
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
    if let Some(v) = &input.visual {
        let sel = selection_from(&ctx, buf, input.cursor.byte, v)?;
        ctx.text_mut(buf)
            .map_err(|e| format!("{e:?}"))?
            .set_selection(w, sel);
        ctx.set_state_of(buf, State::Visual);
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
    if let Some(size) = &input.window {
        ctx.set_top_line(w, size.top_line)
            .ok_or("the window shows no text")?;
    }
    // As the oracle's setup does: the clipboard holds the unnamed text, and what another app
    // copied comes after that.
    let clipboard = ctx.fake_clipboard().clone();
    if let Some(r) = input.registers.iter().flatten().find(|(n, _)| *n == "\"") {
        ctx.write_clipboard(&Register {
            kind: reg_kind(r.1.kind),
            text: r.1.text.clone(),
        });
        clipboard.take_writes();
    }
    if let Some(text) = &input.clipboard {
        clipboard.copy_from_outside(text);
    }
    let log = ctx
        .type_keys(&case.keys)
        .map_err(|e| format!("keys {}: {e}", case.keys))?;
    let errors_after: Vec<usize> = log
        .iter()
        .enumerate()
        .flat_map(|(i, k)| std::iter::repeat_n(i + 1, k.errors.len()))
        .collect();
    let messages: Vec<&String> = log.iter().flat_map(|k| &k.errors).collect();
    let top = ctx.top_line(w);

    let mut diffs = Vec::new();
    let text = ctx.text(buf).map_err(|e| format!("{e:?}"))?;
    let got_text = text.rope().to_string();
    if got_text != want.text {
        diffs.push(format!("text {got_text:?}, want {:?}", want.text));
    }
    if let Some(size) = &want.window {
        let (got, wanted) = (ctx.window_size(w), Some((size.width, size.height)));
        if got != wanted {
            diffs.push(format!("window size {got:?}, want {wanted:?}"));
        }
        if top != Some(size.top_line) {
            diffs.push(format!("top line {top:?}, want {}", size.top_line));
        }
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
        | (EvilState::Replace, State::Replace)
        | (EvilState::Operator, State::OperatorPending) => {}
        (want, got) => diffs.push(format!("state {got:?}, want {want:?}")),
    }
    if let Some(v) = &want.visual {
        let got = text.selection(w).map(|sel| selected(text, sel));
        let wanted = Some((v.kind, v.start.byte, v.end.byte));
        if got != wanted {
            diffs.push(format!("selection {got:?}, want {wanted:?}"));
        }
    }
    // Your Evil lists every register the keys changed, so the others must be as they were.
    let names = ('0'..='9').chain('a'..='z').chain(['-']);
    for name in names.filter(|c| !want.registers.contains_key(&c.to_string())) {
        let preset = input
            .registers
            .iter()
            .flatten()
            .find(|(n, _)| **n == name.to_string());
        let want = preset.map(|(_, r)| r.text.as_str());
        let got = ctx.register(name).map(|r| r.text.as_str());
        if got != want {
            diffs.push(format!(
                "register {name} is {got:?}, and your Evil left it {want:?}"
            ));
        }
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
    let held = clipboard.held().map(|r| r.text);
    if held != want.clipboard.text {
        diffs.push(format!(
            "the clipboard holds {held:?}, but it should hold {:?}",
            want.clipboard.text
        ));
    }
    let writes: Vec<String> = clipboard
        .take_writes()
        .into_iter()
        .map(|r| r.text)
        .collect();
    if writes != want.clipboard.writes {
        diffs.push(format!(
            "the clipboard was sent {writes:?}, but it should have been sent {:?}",
            want.clipboard.writes
        ));
    }
    // A key die-vim doesn't bind is never the error Evil signalled, even when it
    // comes after the same key.
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

/// A case's starting selection: your Evil's `start` and `end`, with the cursor on one end.
/// `end` is just past the selection, so the anchor is the character before it.
fn selection_from(
    ctx: &Ctx,
    buf: die_core::BufferId,
    head: usize,
    v: &evil_oracle::Selection,
) -> Result<Selection, String> {
    let t = ctx.text(buf).map_err(|e| format!("{e:?}"))?;
    let r = t.rope();
    let last = r.char_to_byte(r.byte_to_char(v.end.byte).saturating_sub(1));
    Ok(Selection {
        anchor: if head == v.start.byte {
            last
        } else {
            v.start.byte
        },
        head,
        shape: match v.kind {
            SelectionKind::Char => Shape::Char,
            SelectionKind::Line => Shape::Line,
            SelectionKind::Block => Shape::Block,
        },
        goal: None,
    })
}

/// A selection as your Evil's fixture says it: its shape, and the bytes from its start to
/// just past its end. Whole lines run through the last one's newline.
fn selected(t: &TextBuf, sel: Selection) -> (SelectionKind, usize, usize) {
    let (lo, hi) = (sel.anchor.min(sel.head), sel.anchor.max(sel.head));
    let r = t.rope();
    let past = |p: usize| r.char_to_byte((r.byte_to_char(p) + 1).min(r.len_chars()));
    let after_line = |line: usize| {
        if line + 1 < t.line_count() {
            t.line_start(line + 1)
        } else {
            t.len()
        }
    };
    let line_of = t.line_of(lo);
    match sel.shape {
        Shape::Char => (SelectionKind::Char, lo, past(hi)),
        Shape::Line => (
            SelectionKind::Line,
            t.line_start(line_of),
            after_line(t.line_of(hi)),
        ),
        Shape::Block => {
            // Evil's region: the corner further right takes in its character, unless it's
            // on a line's end, and in one column, the lower one does.
            let (cl, ch) = (t.display_col(lo), t.display_col(hi));
            let at_end = |p: usize| r.get_byte(p).is_none_or(|b| b == b'\n');
            let (beg, end) = if cl > ch || (cl == ch && at_end(hi)) {
                (if at_end(lo) { lo } else { past(lo) }, hi)
            } else {
                (lo, if at_end(hi) { hi } else { past(hi) })
            };
            (SelectionKind::Block, beg, end)
        }
    }
}

fn reg_char(name: &str) -> Result<char, String> {
    let mut c = name.chars();
    match (c.next(), c.next()) {
        (Some(ch), None) => Ok(ch),
        _ => Err(format!("register name {name:?}")),
    }
}

fn reg_kind(k: RegisterKind) -> RegKind {
    match k {
        RegisterKind::Macro => RegKind::Macro,
        RegisterKind::Char => RegKind::Char,
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

/// A tty case starts on the top line it gives, and is held to the one your Evil ended on.
#[test]
fn a_tty_case_must_end_on_evils_top_line() {
    let (mut case, _) = fixture_case(&fixtures_dir(), "motions", "count-l-past-end");
    case.input.text = "a\nb\nc\nd\ne\nf\n".into();
    case.keys = "0".into();
    case.input.cursor.byte = 6;
    case.expected.text = case.input.text.clone();
    case.expected.cursor = case.input.cursor;
    case.expected.errors = None;
    let window = |top_line| evil_oracle::Window {
        width: 40,
        height: 3,
        top_line,
    };
    case.input.window = Some(window(2));
    case.expected.window = Some(window(2));
    assert_eq!(run_case(&case), Ok(()), "it starts and ends on line 2");

    case.expected.window = Some(window(1));
    let why = run_case(&case).expect_err("the top line differs");
    assert!(why.contains("top line Some(2), want 1"), "{why}");
}

/// What reaches the clipboard, and what it ends up holding, are held to your Evil.
#[test]
fn a_kill_must_be_what_the_clipboard_should_have_been_sent() {
    let (case, _) = fixture_case(&fixtures_dir(), "operators", "dw");
    assert_eq!(run_case(&case), Ok(()), "the committed case");

    let (mut case, _) = fixture_case(&fixtures_dir(), "operators", "dw");
    case.expected.clipboard.text = Some("something else".into());
    let why = run_case(&case).expect_err("the clipboard holds another text");
    assert!(why.contains("it should hold"), "{why}");

    let (mut case, _) = fixture_case(&fixtures_dir(), "operators", "dw");
    case.expected.clipboard.writes[0] = "something else".into();
    let why = run_case(&case).expect_err("another text was sent");
    assert!(why.contains("should have been sent"), "{why}");

    let (mut case, _) = fixture_case(&fixtures_dir(), "operators", "dw");
    case.expected
        .clipboard
        .writes
        .insert(0, "an older kill".into());
    let why = run_case(&case).expect_err("one write is missing");
    assert!(why.contains("should have been sent"), "{why}");
}

/// Your Evil lists the registers the keys changed, so a register it doesn't list must not
/// have changed in die-vim either.
#[test]
fn a_register_the_keys_didnt_change_must_stay_as_it_was() {
    let (case, _) = fixture_case(&fixtures_dir(), "operators", "dw");
    assert_eq!(run_case(&case), Ok(()), "the committed case");

    let (mut case, _) = fixture_case(&fixtures_dir(), "operators", "dw");
    case.expected.registers.remove("-");
    let why = run_case(&case).expect_err("dw did change -");
    assert!(why.contains("your Evil left it"), "{why}");
}

/// A case about the clipboard itself, through the fake one in the runner.
#[test]
fn the_clipboards_own_cases_pass() {
    for id in [
        "clipboard-paste-from-other-app",
        "clipboard-written-on-yank-and-delete",
        "black-hole-skips-clipboard",
        "plus-register-yank",
        "plus-register-paste",
    ] {
        let (case, _) = fixture_case(&fixtures_dir(), "registers", id);
        assert_eq!(run_case(&case), Ok(()), "{id}");
    }
}

#[test]
fn an_error_must_come_after_the_same_key_as_evils() {
    let (case, _) = fixture_case(&fixtures_dir(), "motions", "count-l-past-end");
    assert_eq!(run_case(&case), Ok(()), "the committed case");

    let (mut case, _) = fixture_case(&fixtures_dir(), "motions", "count-l-past-end");
    case.expected.errors.as_mut().expect("an error")[0].after_keys = 1;
    let why = run_case(&case).expect_err("the error comes after key 2");
    assert!(why.contains("errors after keys [2]"), "{why}");
}
