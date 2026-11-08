//! T0: the faster checks (the M0 log's calls 185 to 190).
//!
//! Test plan:
//! - The skipping rule. Each trigger path needs `guards` and `clean-build`, near misses don't,
//!   and only `--full`, a trigger or a git failure makes them run. A git failure runs them,
//!   because what can't be told can't be skipped.
//! - The property counts. `ci` sets `DIE_PROP_CASES=100` on the test step, leaves a variable
//!   that's already set alone, and `--full` clears it so a leftover can't thin a checkpoint.
//! - The summary. A skipped step isn't run, shows as "skipped" and never as "ok", and doesn't
//!   fail the run. A step that fails still does.
//! - The diff, in a real repository. Commits since the merge base count and commits on main
//!   don't. A rename counts as both its names. The working tree counts what git would commit:
//!   edits, staged files, untracked files and deletions, and not ignored files.
//! - `mutate --changed`. Only mutants whose file changed run, a file of the same name in
//!   another crate doesn't count, and plain `mutate` selects all of them.
//! - Build caches follow `CARGO_TARGET_DIR`, so a worktree cloned from a warm cache uses it.

use die_test_cap as _;

use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::process::Command;
use xtask::ci::{self, Outcome, Plan, PropCases, Step, StepResult};
use xtask::mutate::{self, Options, Selection};
use xtask::util::{TempDir, changed_at_rev, changed_in_worktree, git, output, task_dir_in, write};

// The skipping rule.

#[test]
fn each_trigger_needs_the_slow_steps() {
    for path in [
        "Cargo.toml",
        "Cargo.lock",
        "crates/die-core/Cargo.toml",
        "apps/die-dev/Cargo.toml",
        ".cargo/config.toml",
        "rust-toolchain.toml",
        "xtask/src/ci.rs",
        "xtask/Cargo.toml",
        "tools/test-cap/src/lib.rs",
        // Cargo finds one without a manifest change, and clean-build is what checks them.
        "crates/die-core/build.rs",
        // The engine archive and its lock, which clean-build verifies.
        "crates/die-term-sys/vendor/aarch64-apple-darwin/libghostty-vt.a",
        "crates/die-term-sys/ghostty-vt.lock",
    ] {
        assert!(ci::touches_build_setup(path), "{path} should need them");
    }
}

#[test]
fn near_misses_dont_need_the_slow_steps() {
    for path in [
        "crates/die-core/src/text.rs",
        "crates/die-core/tests/prop.rs",
        "crates/die-core/tests/mutants.toml",
        "docs/m0-plan.md",
        "docs/Cargo.toml.md",
        "budget.toml",
        "xtask.md",
        "crates/xtask/src/lib.rs",
        "tools/test-capture/src/lib.rs",
        "tools/evil-oracle/src/lib.rs",
        "crates/die-core/Cargo.toml.bak",
        "rust-toolchain.toml.old",
        "crates/die-core/src/build.rs.txt",
        "crates/die-term-sys/src/ffi.rs",
        "crates/die-term-sys/THIRD_PARTY_NOTICES",
    ] {
        assert!(!ci::touches_build_setup(path), "{path} shouldn't need them");
    }
}

fn plan(full: bool, changed: &[&str]) -> Plan {
    let changed = Ok(changed.iter().map(|s| s.to_string()).collect());
    ci::plan(full, &changed, None)
}

#[test]
fn a_change_with_no_trigger_skips_them_and_says_why() {
    let p = plan(false, &["crates/die-core/src/text.rs", "docs/m0-log.md"]);
    assert_eq!(p.skip, ["guards", "clean-build"]);
    assert!(p.steps_note.contains("skipped"), "{}", p.steps_note);
    assert!(p.steps_note.contains("--full"), "{}", p.steps_note);
    assert!(
        p.steps_note.contains("merge base with main"),
        "{}",
        p.steps_note
    );
}

#[test]
fn no_change_at_all_skips_them() {
    assert_eq!(plan(false, &[]).skip, ["guards", "clean-build"]);
}

#[test]
fn a_trigger_runs_them_and_names_the_file() {
    let p = plan(false, &["docs/m0-log.md", "xtask/src/ci.rs", "Cargo.lock"]);
    assert!(p.skip.is_empty(), "{:?}", p.skip);
    assert!(p.steps_note.contains("xtask/src/ci.rs"), "{}", p.steps_note);
}

#[test]
fn full_runs_them_whatever_changed() {
    let p = plan(true, &["docs/m0-log.md"]);
    assert!(p.skip.is_empty(), "{:?}", p.skip);
}

#[test]
fn when_git_cant_say_what_changed_they_run() {
    let p = ci::plan(false, &Err("no such branch: main".to_string()), None);
    assert!(p.skip.is_empty(), "{:?}", p.skip);
    assert!(p.steps_note.contains("no such branch"), "{}", p.steps_note);
}

// The property counts.

/// What the test step's command does to `DIE_PROP_CASES`: unset by `Some(None)`, set by
/// `Some(Some(..))`, and untouched by `None`.
fn env_change(cases: &PropCases) -> Option<Option<String>> {
    let mut cmd = Command::new("true");
    cases.apply(&mut cmd);
    cmd.get_envs()
        .find(|(k, _)| *k == "DIE_PROP_CASES")
        .map(|(_, v)| v.map(|v| v.to_string_lossy().into_owned()))
}

#[test]
fn the_test_step_runs_at_a_hundred_cases() {
    let p = ci::plan(false, &Ok(Vec::new()), None);
    assert_eq!(env_change(&p.cases), Some(Some("100".to_string())));
    assert!(p.cases_note().contains("100"), "{}", p.cases_note());
}

#[test]
fn only_full_runs_the_ignored_tests() {
    let has = |p: &Plan| p.test_args().contains(&"--include-ignored");
    assert!(!has(&ci::plan(false, &Ok(Vec::new()), None)));
    assert!(!has(&ci::plan(false, &Ok(vec!["Cargo.toml".into()]), None)));
    assert!(has(&ci::plan(true, &Ok(Vec::new()), None)));
}

#[test]
fn a_variable_already_set_is_left_alone() {
    let p = ci::plan(false, &Ok(Vec::new()), Some("500"));
    assert_eq!(env_change(&p.cases), None);
    assert!(p.cases_note().contains("500"), "{}", p.cases_note());
}

#[test]
fn full_clears_the_variable_for_the_old_counts() {
    for set in [None, Some("500")] {
        let p = ci::plan(true, &Ok(Vec::new()), set);
        assert_eq!(env_change(&p.cases), Some(None), "with {set:?}");
    }
}

// The summary.

#[test]
fn a_skipped_step_shows_as_skipped_and_never_ok() {
    let results = [
        StepResult::new("fmt", Outcome::Ok, 1.5),
        StepResult::new("test", Outcome::Failed, 2.5),
        StepResult::new("guards", Outcome::Skipped, 0.0),
    ];
    let text = ci::summary("ci", &results, &["the reason".to_string()]);
    let line = |name: &str| {
        text.lines()
            .find(|l| l.trim_start().starts_with(name))
            .unwrap_or_else(|| panic!("no {name} line in:\n{text}"))
            .to_string()
    };
    assert!(line("fmt").contains("ok"), "{text}");
    assert!(line("test").contains("FAIL"), "{text}");
    let guards = line("guards");
    assert!(guards.contains("skipped"), "{text}");
    assert!(!guards.contains("ok"), "{text}");
    assert!(
        !guards.chars().any(|c| c.is_ascii_digit()),
        "a skipped step took no time: {text}"
    );
    assert!(text.contains("the reason"), "{text}");
}

fn counting<'a>(name: &'a str, calls: &'a Cell<u32>, result: ci::StepFn<'a>) -> Step<'a> {
    (
        name,
        Box::new(move || {
            calls.set(calls.get() + 1);
            result()
        }),
    )
}

#[test]
fn a_skipped_step_doesnt_run_and_doesnt_fail_the_run() {
    let (ran, skipped) = (Cell::new(0), Cell::new(0));
    let steps = [
        counting("fmt", &ran, Box::new(|| Ok(()))),
        counting("guards", &skipped, Box::new(|| Err("planted".to_string()))),
    ];
    let p = plan(false, &[]);
    ci::run_plan("t", &steps, &p).unwrap();
    assert_eq!((ran.get(), skipped.get()), (1, 0));
}

#[test]
fn a_step_that_fails_still_fails_the_run_beside_a_skipped_one() {
    let (a, b, c) = (Cell::new(0), Cell::new(0), Cell::new(0));
    let steps = [
        counting("fmt", &a, Box::new(|| Err("planted".to_string()))),
        counting("test", &b, Box::new(|| Ok(()))),
        counting("clean-build", &c, Box::new(|| Ok(()))),
    ];
    let e = ci::run_plan("t", &steps, &plan(false, &[])).unwrap_err();
    assert_eq!(e, "t: failed: fmt");
    assert_eq!((a.get(), b.get(), c.get()), (1, 1, 0));
}

#[test]
fn with_a_trigger_the_slow_steps_do_run() {
    let ran = Cell::new(0);
    let steps = [
        counting("guards", &ran, Box::new(|| Ok(()))),
        counting("clean-build", &ran, Box::new(|| Ok(()))),
    ];
    ci::run_plan("t", &steps, &plan(false, &["Cargo.lock"])).unwrap();
    assert_eq!(ran.get(), 2);
}

#[test]
fn run_steps_skips_nothing() {
    // `ci --linux`'s container runs its checks this way.
    let ran = Cell::new(0);
    let steps = [
        counting("guards", &ran, Box::new(|| Ok(()))),
        counting("clean-build", &ran, Box::new(|| Ok(()))),
    ];
    ci::run_steps("t", &steps).unwrap();
    assert_eq!(ran.get(), 2);
}

// The diff, in a real repository.

fn run(cmd: &mut Command) {
    output(cmd).unwrap();
}

fn in_repo(dir: &Path, args: &[&str]) {
    run(git(dir)
        .args([
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args));
}

fn commit_all(dir: &Path, message: &str) {
    in_repo(dir, &["add", "--all"]);
    in_repo(dir, &["commit", "--quiet", "-m", message]);
}

/// A repository whose `main` has a source file, a manifest and a `.gitignore`, on a new
/// branch `work`.
fn repo() -> TempDir {
    let dir = TempDir::new("t0-repo").unwrap();
    let p = dir.path();
    in_repo(p, &["init", "--quiet", "-b", "main"]);
    write(&p.join("src/a.rs"), "a\n").unwrap();
    write(&p.join("src/b.rs"), "fn b() {}\nfn b2() {}\nfn b3() {}\n").unwrap();
    write(&p.join("Cargo.toml"), "[workspace]\n").unwrap();
    write(&p.join(".gitignore"), "ignored/\n").unwrap();
    commit_all(p, "base");
    in_repo(p, &["switch", "--quiet", "-c", "work"]);
    dir
}

#[test]
fn a_revs_diff_starts_at_the_merge_base_with_main() {
    let dir = repo();
    let p = dir.path();
    write(&p.join("src/a.rs"), "changed on the branch\n").unwrap();
    commit_all(p, "branch");
    // Main moves on, touching a manifest. The branch hasn't touched one.
    in_repo(p, &["switch", "--quiet", "main"]);
    write(&p.join("Cargo.toml"), "[workspace]\n# main\n").unwrap();
    commit_all(p, "main moves");
    in_repo(p, &["switch", "--quiet", "work"]);
    assert_eq!(changed_at_rev(p, "HEAD").unwrap(), ["src/a.rs"]);
    assert_eq!(changed_at_rev(p, "work").unwrap(), ["src/a.rs"]);
    // Main itself has no diff from its own merge base.
    assert!(changed_at_rev(p, "main").unwrap().is_empty());
}

#[test]
fn a_revs_diff_ignores_the_working_tree() {
    let dir = repo();
    let p = dir.path();
    write(&p.join("src/a.rs"), "not committed\n").unwrap();
    write(&p.join("new.rs"), "not committed\n").unwrap();
    assert!(changed_at_rev(p, "HEAD").unwrap().is_empty());
}

#[test]
fn a_rename_counts_as_both_its_names() {
    let dir = repo();
    let p = dir.path();
    in_repo(p, &["mv", "src/b.rs", "src/c.rs"]);
    commit_all(p, "rename");
    assert_eq!(changed_at_rev(p, "HEAD").unwrap(), ["src/b.rs", "src/c.rs"]);
    assert_eq!(changed_in_worktree(p).unwrap(), ["src/b.rs", "src/c.rs"]);
}

#[test]
fn the_working_tree_counts_what_git_would_commit() {
    let dir = repo();
    let p = dir.path();
    write(&p.join("committed.rs"), "x\n").unwrap();
    commit_all(p, "branch");
    write(&p.join("src/a.rs"), "edited, not staged\n").unwrap();
    write(&p.join("staged.rs"), "x\n").unwrap();
    in_repo(p, &["add", "staged.rs"]);
    write(&p.join("untracked.rs"), "x\n").unwrap();
    write(&p.join("ignored/file.rs"), "x\n").unwrap();
    std::fs::remove_file(p.join("Cargo.toml")).unwrap();
    assert_eq!(
        changed_in_worktree(p).unwrap(),
        [
            "Cargo.toml",
            "committed.rs",
            "src/a.rs",
            "staged.rs",
            "untracked.rs"
        ]
    );
}

#[test]
fn a_clean_branch_at_main_has_no_changes() {
    let dir = repo();
    assert!(changed_in_worktree(dir.path()).unwrap().is_empty());
    assert!(changed_at_rev(dir.path(), "HEAD").unwrap().is_empty());
}

#[test]
fn no_main_branch_is_an_error_not_an_empty_diff() {
    let dir = TempDir::new("t0-no-main").unwrap();
    let p = dir.path();
    in_repo(p, &["init", "--quiet", "-b", "trunk"]);
    write(&p.join("a.rs"), "a\n").unwrap();
    commit_all(p, "base");
    assert!(changed_at_rev(p, "HEAD").is_err());
    assert!(changed_in_worktree(p).is_err());
}

#[test]
fn a_branch_that_touches_the_lockfile_runs_the_slow_steps_end_to_end() {
    let dir = repo();
    let p = dir.path();
    write(&p.join("src/a.rs"), "changed\n").unwrap();
    commit_all(p, "source only");
    let changed = changed_at_rev(p, "HEAD");
    assert_eq!(
        ci::plan(false, &changed, None).skip,
        ["guards", "clean-build"]
    );
    write(&p.join("Cargo.lock"), "version = 4\n").unwrap();
    commit_all(p, "lockfile");
    let changed = changed_at_rev(p, "HEAD");
    assert!(ci::plan(false, &changed, None).skip.is_empty());
}

// `mutate --changed`.

fn mutants_toml() -> PathBuf {
    xtask::util::root().join("crates/die-core/tests/mutants.toml")
}

fn named(selection: &Selection) -> Vec<String> {
    selection.run.iter().map(|m| m.file.clone()).collect()
}

const SPEC: &str = r#"
[[mutant]]
name = "a"
file = "src/text.rs"
old = "x"
new = "y"

[[mutant]]
name = "b"
file = "src/undo.rs"
old = "x"
new = "y"

[[mutant]]
name = "c"
file = "src/text/hidden.rs"
old = "x"
new = "y"

[[mutant]]
name = "d"
file = "src/undo.rs"
old = "p"
new = "q"
"#;

fn spec() -> Vec<mutate::Mutant> {
    mutate::parse(SPEC).unwrap()
}

#[test]
fn changed_keeps_only_the_mutants_on_changed_files() {
    let changed = vec![
        "crates/die-core/src/undo.rs".to_string(),
        "docs/m0-log.md".to_string(),
    ];
    let s = mutate::select(spec(), Some(&changed));
    assert_eq!(named(&s), ["src/undo.rs", "src/undo.rs"]);
    assert_eq!(s.skipped, 2);
}

#[test]
fn a_file_of_the_same_name_elsewhere_isnt_a_change() {
    let changed = vec![
        "crates/die-vim/src/undo.rs".to_string(),
        "src/undo.rs".to_string(),
        "crates/die-core/src/text/hidden.rs.orig".to_string(),
    ];
    let s = mutate::select(spec(), Some(&changed));
    assert!(s.run.is_empty(), "{:?}", named(&s));
    assert_eq!(s.skipped, 4);
}

#[test]
fn a_nested_file_matches_only_itself() {
    let changed = vec!["crates/die-core/src/text/hidden.rs".to_string()];
    let s = mutate::select(spec(), Some(&changed));
    assert_eq!(named(&s), ["src/text/hidden.rs"]);
    assert_eq!(s.skipped, 3);
}

#[test]
fn plain_mutate_selects_every_mutant() {
    let s = mutate::select(spec(), None);
    assert_eq!(s.run.len(), 4);
    assert_eq!(s.skipped, 0);
}

#[test]
fn the_real_table_loads_and_filters() {
    let all = mutate::load(&mutants_toml()).unwrap();
    let total = all.len();
    assert!(total >= 48, "{total}");
    let changed = vec!["crates/die-core/src/undo.rs".to_string()];
    let s = mutate::select(all, Some(&changed));
    assert!(!s.run.is_empty());
    assert!(s.run.iter().all(|m| m.file == "src/undo.rs"));
    assert_eq!(s.run.len() + s.skipped, total);
}

#[test]
fn mutate_takes_its_flags_in_either_order() {
    let parse = |args: &[&str]| Options::parse(args).map(|o| (o.changed, o.cases));
    let default = mutate::DEFAULT_CASES;
    assert_eq!(parse(&[]), Some((false, default)));
    assert_eq!(parse(&["--changed"]), Some((true, default)));
    assert_eq!(parse(&["--cases", "7"]), Some((false, 7)));
    assert_eq!(parse(&["--changed", "--cases", "7"]), Some((true, 7)));
    assert_eq!(parse(&["--cases", "7", "--changed"]), Some((true, 7)));
    assert_eq!(parse(&["--cases", "x"]), None);
    assert_eq!(parse(&["--cases"]), None);
    assert_eq!(parse(&["--other"]), None);
}

// Build caches.

#[test]
fn a_tasks_directory_follows_the_cargo_target_dir() {
    let root = Path::new("/w");
    assert_eq!(
        task_dir_in(None, root, "ci"),
        Path::new("/w/target/xtask/ci")
    );
    assert_eq!(
        task_dir_in(Some("/warm".into()), root, "ci"),
        Path::new("/warm/xtask/ci")
    );
    assert_eq!(
        task_dir_in(Some("rel".into()), root, "ci"),
        Path::new("/w/rel/xtask/ci")
    );
    assert_eq!(
        task_dir_in(Some("".into()), root, "ci"),
        Path::new("/w/target/xtask/ci")
    );
}
