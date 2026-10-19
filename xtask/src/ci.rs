//! `cargo xtask ci`: every check, in a fresh clone (docs/m0-plan.md §3, "How
//! work lands"). Nothing merges until it passes.

use crate::util::{self, Result, TempDir, cargo, copy_worktree, git};
use crate::{budget, clean, cross, fonts, guards, lint};
use std::path::Path;
use std::process::Command;
use std::time::Instant;

pub enum Source {
    /// A clone of the repository at this revision.
    Rev(String),
    /// The working tree as git would commit it, for work that isn't committed yet.
    Worktree,
}

pub fn run(root: &Path, source: Source) -> Result {
    let work = root.join("target/xtask/ci");
    let tmp = TempDir::at(work.join("tree"))?;
    let tree = tmp.path().to_path_buf();
    println!("ci: {}", checkout(root, &source, &tree)?);
    fonts::run_fonts(root)?;
    let target = work.join("target");
    let cargo_step = |args: &[&str]| -> Result {
        util::run(
            Command::new(cargo())
                .current_dir(&tree)
                .args(args)
                .env("CARGO_TARGET_DIR", &target)
                .env("DIE_FONT_DIR", fonts::dir(root)),
        )
    };

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
        (
            "cross-check",
            Box::new(|| cross::run(&tree, &work.join("cross"))),
        ),
        (
            "test",
            Box::new(|| cargo_step(&["test", "--workspace", "--locked", "--offline", "--quiet"])),
        ),
        (
            "guards",
            Box::new(|| guards::run(&tree, &work.join("guards"))),
        ),
        ("clean-build", Box::new(|| clean::run(&tree, false))),
    ];
    run_steps("ci", &steps)
}

/// A named check, for `run_steps`.
pub type Step<'a> = (&'a str, Box<dyn Fn() -> Result + 'a>);

/// Run every step, even after one fails, then print each one's result and
/// time under `label`. Fails if any step failed.
pub fn run_steps(label: &str, steps: &[Step]) -> Result {
    let mut results = Vec::new();
    for (name, step) in steps {
        println!("\n==> {label}: {name}");
        let t = Instant::now();
        let r = step();
        if let Err(e) = &r {
            eprintln!("{e}");
        }
        results.push((*name, r.is_ok(), t.elapsed().as_secs_f64()));
    }
    println!("\n{label} summary");
    let w = results.iter().map(|r| r.0.len()).max().unwrap_or(0).max(12);
    for (name, ok, secs) in &results {
        println!(
            "  {:<w$} {:<4} {secs:6.1} s",
            name,
            if *ok { "ok" } else { "FAIL" }
        );
    }
    let failed: Vec<&str> = results.iter().filter(|r| !r.1).map(|r| r.0).collect();
    if failed.is_empty() {
        Ok(())
    } else {
        Err(format!("{label}: failed: {}", failed.join(", ")))
    }
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
