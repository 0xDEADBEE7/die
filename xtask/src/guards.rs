//! `cargo xtask guards`: proves F1's build guards in a throwaway copy of the
//! working tree. `unsafe` outside die-render's `platform` module fails to build,
//! and so does `-Cpanic=abort`, with the guard's message.

use crate::util::{Result, TempDir, cargo, copy_worktree, read, write};
use std::path::Path;
use std::process::{Command, Stdio};

const PLANT: &str = "\n#[allow(dead_code)]\nfn planted() {\n    unsafe {}\n}\n";

/// `work` holds the copy and its build cache, kept between runs.
pub fn run(root: &Path, work: &Path) -> Result {
    let tmp = TempDir::at(work.join("tree"))?;
    let tree = tmp.path();
    copy_worktree(root, tree)?;
    let target = work.join("target");

    expect(
        tree,
        &target,
        &["-p", "die-core", "-p", "die-render"],
        None,
        None,
    )?;
    println!("guards: the unplanted tree builds");

    for lib in ["crates/die-core/src/lib.rs", "crates/die-render/src/lib.rs"] {
        let path = tree.join(lib);
        let orig = read(&path)?;
        write(&path, &format!("{orig}{PLANT}"))?;
        let krate = lib.split('/').nth(1).unwrap_or_default();
        let res = expect(
            tree,
            &target,
            &["-p", krate],
            None,
            Some("usage of an `unsafe` block"),
        );
        write(&path, &orig)?;
        res?;
        println!("guards: an unsafe block in {lib} fails to build");
    }

    expect(
        tree,
        &target,
        &["-p", "die-core"],
        Some("-Cpanic=abort"),
        Some("DIE requires panic = \"unwind\""),
    )?;
    println!("guards: -Cpanic=abort fails to build with the guard's message");
    Ok(())
}

/// `cargo check` in `tree`. With `fails_with`, it must fail and print that text.
fn expect(
    tree: &Path,
    target: &Path,
    args: &[&str],
    rustflags: Option<&str>,
    fails_with: Option<&str>,
) -> Result {
    let mut cmd = Command::new(cargo());
    cmd.current_dir(tree)
        .args(["check", "--quiet", "--offline"])
        .args(args);
    // A build with other flags gets its own directory, or the two evict each other's cache.
    match rustflags {
        Some(f) => cmd
            .env("RUSTFLAGS", f)
            .arg("--target-dir")
            .arg(target.join("rustflags")),
        None => cmd.env_remove("RUSTFLAGS").arg("--target-dir").arg(target),
    };
    let out = cmd
        .stdin(Stdio::null())
        .output()
        .map_err(|e| e.to_string())?;
    let err = String::from_utf8_lossy(&out.stderr);
    match fails_with {
        None if out.status.success() => Ok(()),
        None => Err(format!(
            "guards: cargo check {} failed on the unplanted tree:\n{err}",
            args.join(" ")
        )),
        Some(_) if out.status.success() => Err(format!(
            "guards: cargo check {} passed, but the guard should stop it",
            args.join(" ")
        )),
        Some(text) if err.contains(text) => Ok(()),
        Some(text) => Err(format!(
            "guards: cargo check {} failed without {text:?}:\n{err}",
            args.join(" ")
        )),
    }
}
