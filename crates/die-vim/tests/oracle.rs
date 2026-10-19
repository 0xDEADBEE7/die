//! Exit test (d): every Evil-oracle case passes, except the deliberate deviations
//! (docs/m0-plan.md §1).
//!
//! Each case runs through die-core with die-vim registered: the case's text and
//! cursor, its registers, its keys typed one by one, then text, cursor, state,
//! registers and errors compared with what your Evil did.
//!
//! - `oracle/deviations.json` names the cases die-vim deliberately doesn't match.
//!   They're left out here and get hand-written fixtures.
//! - `oracle/pending.txt` names the cases die-vim doesn't pass yet, by the work
//!   package they land with. A pending case that passes fails this test, and so
//!   does a failing case that isn't pending.
//! - `DIE_ORACLE_BLESS=1` rewrites `pending.txt` to the cases that fail now.
//!   Check the diff: a case that leaves it is done, one that joins it is a regression.
//!
//! The runner uses die-core's spike-era API until the public harness (K7) lands.
//! What it can't set up or check yet fails the case with a reason, so a case
//! never passes on something unchecked.

use die_core::{Ctx, Key, KeyEvent, RegKind, Register, State};
use die_vim::Vim;
use evil_oracle::{
    Case, Deviations, RegisterKind, Run, State as EvilState, Tranche, fixtures_dir, load_fixtures,
    parse_pending, tranche,
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
    let (input, want) = (&case.input, &case.expected);
    if input.visual.is_some() {
        return Err("can't start a case in visual state yet (the harness, K7)".into());
    }
    if input.clipboard.is_some() {
        return Err("no clipboard yet (K6)".into());
    }
    if run == Run::Tty || input.window.is_some() {
        return Err("can't set a window's top line yet (the harness, K7)".into());
    }

    let mut ctx = Ctx::new(vec![Box::new(Vim)]).map_err(|e| e.join("; "))?;
    let buf = ctx.create_text("case", &input.text);
    let w = ctx
        .open_window(buf, COLS, ROWS)
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
    for tok in &case.key_tokens {
        ctx.input(key_event(tok)?);
    }
    ctx.run_until_idle();

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
    // Errors are counted until the harness logs them per key (K7). A key die-vim
    // doesn't bind is never the error Evil signalled, even when the counts agree.
    let messages = &ctx.messages[before..];
    if let Some(m) = messages.iter().find(|m| m.ends_with(" is undefined")) {
        diffs.push(format!("die-vim doesn't bind a key: {m}"));
    }
    let errors = messages.len();
    let want_errors = want.errors.as_ref().map_or(0, Vec::len);
    if errors != want_errors {
        diffs.push(format!(
            "{errors} errors ({messages:?}), want {want_errors}"
        ));
    }
    if diffs.is_empty() {
        Ok(())
    } else {
        Err(diffs.join("; "))
    }
}

/// One oracle key token as a key event. Single characters are typed as they are;
/// `<Name>` goes through core's notation until K3 shares one parser with the oracle.
fn key_event(tok: &str) -> Result<KeyEvent, String> {
    let mut chars = tok.chars();
    if let (Some(c), None) = (chars.next(), chars.next()) {
        return Ok(KeyEvent::press(Key::Char(c)));
    }
    let evs = die_core::parse_keys(tok).map_err(|e| format!("key {tok}: {e}"))?;
    match <[KeyEvent; 1]>::try_from(evs) {
        Ok([ev]) => Ok(ev),
        Err(_) => Err(format!("key {tok} isn't one key")),
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
