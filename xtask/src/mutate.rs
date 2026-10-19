//! `cargo xtask mutate`: die-core's planted bugs (from spike S4's `mutate.sh`).
//! Each mutant breaks the core in one specific way, in a throwaway copy of the
//! working tree, and the property tests must catch it.

use crate::util::{Result, TempDir, cargo, copy_worktree, read, write};
use serde::Deserialize;
use std::path::Path;
use std::process::{Command, Stdio};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Mutants {
    #[serde(rename = "mutant")]
    mutants: Vec<Mutant>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Mutant {
    name: String,
    /// Relative to `crates/die-core`.
    file: String,
    old: String,
    new: String,
}

pub fn run(root: &Path, cases: u32) -> Result {
    let spec = root.join("crates/die-core/tests/mutants.toml");
    let m: Mutants =
        toml::from_str(&read(&spec)?).map_err(|e| format!("{}: {e}", spec.display()))?;
    let work = root.join("target/xtask/mutate");
    let tmp = TempDir::at(work.join("tree"))?;
    let tree = tmp.path();
    copy_worktree(root, tree)?;
    let target = work.join("target");

    println!("mutate: the unmutated tree first");
    if let Some(failed) = prop_failures(tree, &target, cases)? {
        return Err(format!(
            "mutate: the property tests fail without a mutant: {failed}"
        ));
    }
    let mut missed = Vec::new();
    for mu in &m.mutants {
        let path = tree.join("crates/die-core").join(&mu.file);
        let orig = read(&path)?;
        if orig.matches(&mu.old).count() != 1 {
            return Err(format!(
                "mutate: {}: the text to replace must occur exactly once in {}",
                mu.name, mu.file
            ));
        }
        write(&path, &orig.replacen(&mu.old, &mu.new, 1))?;
        let res = prop_failures(tree, &target, cases);
        write(&path, &orig)?;
        match res? {
            Some(failed) => println!("caught      {}  <- {failed}", mu.name),
            None => {
                println!("NOT CAUGHT  {}", mu.name);
                missed.push(mu.name.clone());
            }
        }
    }
    if missed.is_empty() {
        println!("mutate: {} of {} caught", m.mutants.len(), m.mutants.len());
        Ok(())
    } else {
        Err(format!("mutate: not caught: {}", missed.join(", ")))
    }
}

/// Run the property tests; Some(names of the failing tests) if any fail.
fn prop_failures(tree: &Path, target: &Path, cases: u32) -> Result<Option<String>> {
    let out = Command::new(cargo())
        .current_dir(tree)
        .args([
            "test",
            "--offline",
            "-p",
            "die-core",
            "--test",
            "prop",
            "--target-dir",
        ])
        .arg(target)
        .env("DIE_PROP_CASES", cases.to_string())
        .stdin(Stdio::null())
        .output()
        .map_err(|e| e.to_string())?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    if out.status.success() {
        return Ok(None);
    }
    if text.contains("error[E") || text.contains("could not compile") {
        return Err(format!("mutate: the tree doesn't compile:\n{text}"));
    }
    let failed: Vec<&str> = text
        .lines()
        .filter_map(|l| {
            l.strip_prefix("test ")
                .and_then(|l| l.strip_suffix(" ... FAILED"))
        })
        .collect();
    Ok(Some(if failed.is_empty() {
        "failed".into()
    } else {
        failed.join(" ")
    }))
}
