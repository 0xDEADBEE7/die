//! `cargo xtask ci`: the checks, in a fresh clone (docs/m0-plan.md §3, "How work lands").
//! Nothing merges until it passes. `guards` and `clean-build` run only when a change could
//! affect them, and the property tests run at a lower count. `--full` runs everything at the
//! old counts, at each checkpoint.

use crate::util::{self, Result, TempDir, cargo, copy_worktree, git};
use crate::{budget, clean, cross, fonts, guards, lint, sources};
use std::path::Path;
use std::process::Command;
use std::time::Instant;

pub enum Source {
    /// A clone of the repository at this revision.
    Rev(String),
    /// The working tree as git would commit it, for work that isn't committed yet.
    Worktree,
}

/// The steps that check the build setup and the checks themselves, which a change elsewhere
/// can't affect in the ways they look for.
pub const SLOW_STEPS: [&str; 2] = ["guards", "clean-build"];

/// The property tests' case count in `ci`. The old counts are `cases()` in die-core's
/// `tests/prop.rs`, which `--full` runs.
pub const PROP_CASES: &str = "2000";

const PROP_VAR: &str = "DIE_PROP_CASES";

/// Whether a change to `path`, relative to the repository's root, can change what `guards`
/// and `clean-build` find: a manifest or lockfile anywhere, cargo's or the toolchain's
/// settings, a build script, the checks themselves, or the memory limit every test links.
pub fn touches_build_setup(path: &str) -> bool {
    let p = Path::new(path);
    let named = |names: &[&str]| p.file_name().is_some_and(|n| names.iter().any(|s| n == *s));
    named(&["Cargo.toml", "Cargo.lock", "build.rs"])
        || path == "rust-toolchain.toml"
        || path.starts_with("xtask/")
        || path.starts_with("tools/test-cap/")
        || p.components().any(|c| c.as_os_str() == ".cargo")
}

/// What the test step does with `DIE_PROP_CASES`.
#[derive(Debug, PartialEq)]
pub enum PropCases {
    /// Sets it to `PROP_CASES`.
    Lowered,
    /// Leaves the value it's already set to.
    AlreadySet(String),
    /// Clears it, for the counts in the tests.
    OldCounts,
}

impl PropCases {
    pub fn apply(&self, cmd: &mut Command) {
        match self {
            PropCases::Lowered => {
                cmd.env(PROP_VAR, PROP_CASES);
            }
            PropCases::AlreadySet(_) => {}
            PropCases::OldCounts => {
                cmd.env_remove(PROP_VAR);
            }
        }
    }
}

/// Which steps a run skips, and at what property counts it tests.
#[derive(Debug)]
pub struct Plan {
    pub skip: Vec<&'static str>,
    /// Why those steps do or don't run. Empty when there was no choice to make.
    pub steps_note: String,
    pub cases: PropCases,
}

impl Plan {
    /// Every step, for a caller with no choice to make.
    fn everything() -> Plan {
        Plan {
            skip: Vec::new(),
            steps_note: String::new(),
            cases: PropCases::OldCounts,
        }
    }

    pub fn cases_note(&self) -> String {
        match &self.cases {
            PropCases::Lowered => {
                format!("test runs at {PROP_VAR}={PROP_CASES} (`ci --full` runs the old counts)")
            }
            PropCases::AlreadySet(n) => format!("test runs at {PROP_VAR}={n}, as already set"),
            PropCases::OldCounts => "test runs at the old property counts".into(),
        }
    }

    /// What the summary says about the plan.
    fn notes(&self) -> Vec<String> {
        if self.steps_note.is_empty() {
            Vec::new()
        } else {
            vec![self.steps_note.clone(), self.cases_note()]
        }
    }
}

/// Decide what a run does. `changed` is what git says changed since the merge base with main.
/// If git can't say, nothing is skipped. `cases_set` is `DIE_PROP_CASES`, if it's set.
pub fn plan(full: bool, changed: &Result<Vec<String>>, cases_set: Option<&str>) -> Plan {
    let cases = match cases_set {
        _ if full => PropCases::OldCounts,
        Some(n) => PropCases::AlreadySet(n.to_string()),
        None => PropCases::Lowered,
    };
    let slow = SLOW_STEPS.join(" and ");
    let (skip, steps_note) = match changed {
        _ if full => (Vec::new(), format!("{slow} run, for --full")),
        Err(e) => (
            Vec::new(),
            format!("{slow} run, because git can't say what changed: {e}"),
        ),
        Ok(files) => match files.iter().find(|f| touches_build_setup(f)) {
            Some(file) => (
                Vec::new(),
                format!("{slow} run: {file} changed since the merge base with main"),
            ),
            None => (
                SLOW_STEPS.to_vec(),
                format!(
                    "{slow} skipped: nothing since the merge base with main touches Cargo.toml, \
                     Cargo.lock, .cargo/, rust-toolchain.toml, a build script, xtask/ or \
                     tools/test-cap/ (`ci --full` runs them)"
                ),
            ),
        },
    };
    Plan {
        skip,
        steps_note,
        cases,
    }
}

/// The files a run's source changed since its merge base with main.
fn changed(root: &Path, source: &Source) -> Result<Vec<String>> {
    match source {
        Source::Rev(rev) => util::changed_at_rev(root, rev),
        Source::Worktree => util::changed_in_worktree(root),
    }
}

pub fn run(root: &Path, source: Source, full: bool) -> Result {
    let work = util::task_dir(root, "ci");
    let tmp = TempDir::at(work.join("tree"))?;
    let tree = tmp.path().to_path_buf();
    println!("ci: {}", checkout(root, &source, &tree)?);
    let cases_set = std::env::var(PROP_VAR).ok();
    let plan = plan(full, &changed(root, &source), cases_set.as_deref());
    println!("ci: {}", plan.steps_note);
    println!("ci: {}", plan.cases_note());
    fonts::run_fonts(root)?;
    let target = work.join("target");
    let cargo_cmd = |args: &[&str]| -> Command {
        let mut cmd = Command::new(cargo());
        cmd.current_dir(&tree)
            .args(args)
            .env("CARGO_TARGET_DIR", &target)
            .env("DIE_FONT_DIR", fonts::dir(root));
        cmd
    };
    let cargo_step = |args: &[&str]| -> Result { util::run(&mut cargo_cmd(args)) };

    let steps: Vec<Step> = vec![
        ("fmt", Box::new(|| cargo_step(&["fmt", "--all", "--check"]))),
        ("lint", Box::new(|| lint::run(&tree))),
        ("budget", Box::new(|| budget::run(&tree))),
        (
            "clippy",
            Box::new(|| {
                cargo_step(&[
                    "clippy",
                    "--workspace",
                    "--all-targets",
                    "--locked",
                    "--offline",
                    "--quiet",
                    "--",
                    "-D",
                    "warnings",
                ])
            }),
        ),
        ("sources", Box::new(|| sources::run(&tree, &target))),
        (
            "cross-check",
            Box::new(|| cross::run(&tree, &work.join("cross"))),
        ),
        (
            "test",
            Box::new(|| {
                let mut cmd =
                    cargo_cmd(&["test", "--workspace", "--locked", "--offline", "--quiet"]);
                plan.cases.apply(&mut cmd);
                util::run(&mut cmd)
            }),
        ),
        (
            "guards",
            Box::new(|| guards::run(&tree, &work.join("guards"))),
        ),
        ("clean-build", Box::new(|| clean::run(&tree, false))),
    ];
    run_plan("ci", &steps, &plan)
}

/// What a step runs.
pub type StepFn<'a> = Box<dyn Fn() -> Result + 'a>;

/// A named check, for `run_steps`.
pub type Step<'a> = (&'a str, StepFn<'a>);

/// Run every step, even after one fails, then print each one's result and
/// time under `label`. Fails if any step failed.
pub fn run_steps(label: &str, steps: &[Step]) -> Result {
    run_plan(label, steps, &Plan::everything())
}

/// `run_steps`, leaving out what `plan` skips.
pub fn run_plan(label: &str, steps: &[Step], plan: &Plan) -> Result {
    let mut results = Vec::new();
    for (name, step) in steps {
        if plan.skip.contains(name) {
            println!("\n==> {label}: {name} skipped");
            results.push(StepResult::new(name, Outcome::Skipped, 0.0));
            continue;
        }
        println!("\n==> {label}: {name}");
        let t = Instant::now();
        let r = step();
        if let Err(e) = &r {
            eprintln!("{e}");
        }
        let outcome = if r.is_ok() {
            Outcome::Ok
        } else {
            Outcome::Failed
        };
        results.push(StepResult::new(name, outcome, t.elapsed().as_secs_f64()));
    }
    print!("{}", summary(label, &results, &plan.notes()));
    let failed: Vec<&str> = results
        .iter()
        .filter(|r| r.outcome == Outcome::Failed)
        .map(|r| r.name.as_str())
        .collect();
    if failed.is_empty() {
        Ok(())
    } else {
        Err(format!("{label}: failed: {}", failed.join(", ")))
    }
}

/// How a step ended.
#[derive(Debug, PartialEq, Clone, Copy)]
pub enum Outcome {
    Ok,
    Failed,
    Skipped,
}

/// One line of the summary.
pub struct StepResult {
    pub name: String,
    pub outcome: Outcome,
    pub secs: f64,
}

impl StepResult {
    pub fn new(name: &str, outcome: Outcome, secs: f64) -> StepResult {
        StepResult {
            name: name.to_string(),
            outcome,
            secs,
        }
    }
}

/// Each step's result and time, then `notes`.
pub fn summary(label: &str, results: &[StepResult], notes: &[String]) -> String {
    let w = results
        .iter()
        .map(|r| r.name.len())
        .max()
        .unwrap_or(0)
        .max(12);
    let mut text = format!("\n{label} summary\n");
    for r in results {
        let line = match r.outcome {
            Outcome::Ok => format!("  {:<w$} {:<4} {:6.1} s", r.name, "ok", r.secs),
            Outcome::Failed => format!("  {:<w$} {:<4} {:6.1} s", r.name, "FAIL", r.secs),
            Outcome::Skipped => format!("  {:<w$} skipped", r.name),
        };
        text.push_str(&line);
        text.push('\n');
    }
    for note in notes {
        text.push_str(&format!("  {note}\n"));
    }
    text
}

/// Fill `tree`, an empty directory, from `source`. Returns what it holds.
pub fn checkout(root: &Path, source: &Source, tree: &Path) -> Result<String> {
    match source {
        Source::Rev(rev) => {
            run_quiet(
                git(root)
                    .args(["clone", "--quiet", "--no-hardlinks", "."])
                    .arg(tree),
            )?;
            // Resolved here: the clone has no local branches but its default one.
            let commit = crate::util::output(git(root).args([
                "rev-parse",
                "--verify",
                &format!("{rev}^{{commit}}"),
            ]))?;
            run_quiet(git(tree).args(["checkout", "--quiet", "--detach", commit.trim()]))?;
            Ok(format!("a fresh clone at {rev} ({})", &commit.trim()[..10]))
        }
        Source::Worktree => {
            copy_worktree(root, tree)?;
            // Staged, not committed: enough for the steps that ask git which files exist.
            run_quiet(git(tree).args(["init", "--quiet"]))?;
            run_quiet(git(tree).args(["add", "--all"]))?;
            Ok("a fresh copy of the working tree (uncommitted work included)".into())
        }
    }
}

fn run_quiet(cmd: &mut Command) -> Result {
    crate::util::output(cmd).map(|_| ())
}
