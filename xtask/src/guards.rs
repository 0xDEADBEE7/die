//! `cargo xtask guards`: proves F1's build guards in a throwaway copy of the
//! working tree. `unsafe` outside die-render's `platform` module fails to build,
//! and so does `-Cpanic=abort`, with the guard's message. For Linux, the
//! cross-check fails on dead code (its `-D warnings`) and on a build script
//! that compiles C (its stand-ins).

use crate::util::{Result, TempDir, cargo, copy_worktree, output, read, write};
use std::path::Path;
use std::process::{Command, Stdio};

const PLANT: &str = "\n#[allow(dead_code)]\nfn planted() {\n    unsafe {}\n}\n";

/// Dead on Linux only: the cross-check's `-D warnings` must catch it.
const LINUX_ONLY: &str = "\n#[cfg(not(target_os = \"macos\"))]\nfn planted_linux_only() {}\n";

/// die-render's Linux dependency table, where the `log` plant goes.
const LINUX_TABLE: &str = "[target.'cfg(target_os = \"linux\")'.dependencies]\n";

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

    let path = tree.join("crates/die-render/src/platform/mod.rs");
    let orig = read(&path)?;
    write(&path, &format!("{orig}{LINUX_ONLY}"))?;
    let res = crate::cross::run(tree, &work.join("cross"));
    write(&path, &orig)?;
    fails_with(
        "the cross-check",
        res,
        &["function `planted_linux_only` is never used"],
    )?;
    println!("guards: code that is dead on Linux only fails the cross-check");

    // The `log` plant's stand-in call drops the cross-check's cache, so it
    // gets a cache of its own.
    let res = with_log_plant(tree, || crate::cross::run(tree, &work.join("cross-log")));
    fails_with("the cross-check", res, LOG_PLANT_FAILS)?;
    println!(
        "guards: wayland-backend's `log` feature fails the cross-check: its build script calls cc"
    );
    Ok(())
}

/// What a build with the `log` plant must fail with.
const LOG_PLANT_FAILS: &[&str] = &[
    "the build script of `wayland-backend",
    "clean-build: cc was called",
];

/// Run `check` on `tree` with wayland-backend's own `log` feature turned on in
/// die-render's Linux table, then put the manifest and lockfile back. That
/// feature compiles C. (wayland-client's `log` doesn't turn it on: it's
/// wayland-client's own optional dependency.)
fn with_log_plant(tree: &Path, check: impl FnOnce() -> Result) -> Result {
    let manifest = tree.join("crates/die-render/Cargo.toml");
    let lock = tree.join("Cargo.lock");
    let (orig, orig_lock) = (read(&manifest)?, read(&lock)?);
    let version = locked_version(&orig_lock, "wayland-backend").ok_or(
        "guards: wayland-backend isn't in Cargo.lock; its `log` plant needs another crate that compiles C",
    )?;
    if !orig.contains(LINUX_TABLE) {
        return Err(format!(
            "guards: crates/die-render/Cargo.toml has no {} table for the `log` plant",
            LINUX_TABLE.trim()
        ));
    }
    let planted = format!(
        "{LINUX_TABLE}wayland-backend = {{ version = \"={version}\", features = [\"log\"] }}\n"
    );
    write(&manifest, &orig.replacen(LINUX_TABLE, &planted, 1))?;
    // The lockfile gains the new edges only; every version stays.
    let res = output(Command::new(cargo()).current_dir(tree).args([
        "metadata",
        "--offline",
        "--format-version",
        "1",
    ]))
    .and_then(|_| check());
    write(&manifest, &orig)?;
    write(&lock, &orig_lock)?;
    res
}

/// A check (`what`) that must fail, with each of `texts` in its error.
fn fails_with(what: &str, res: Result, texts: &[&str]) -> Result {
    match res {
        Ok(()) => Err(format!(
            "guards: {what} passed with a plant it should fail on ({})",
            texts.join("; ")
        )),
        Err(e) if texts.iter().all(|t| e.contains(t)) => Ok(()),
        Err(e) => Err(format!("guards: {what} failed without {texts:?}:\n{e}")),
    }
}

/// The version of `name` in a Cargo.lock, if it holds one.
fn locked_version(lock: &str, name: &str) -> Option<String> {
    let at = lock.find(&format!("\nname = \"{name}\"\nversion = \""))?;
    let rest = &lock[at..].splitn(3, '\n').nth(2)?;
    Some(
        rest.strip_prefix("version = \"")?
            .split('"')
            .next()?
            .to_string(),
    )
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

#[cfg(test)]
mod tests {
    #[test]
    fn locked_version() {
        let lock = "version = 4\n\n[[package]]\nname = \"wayland-backend\"\nversion = \"0.3.17\"\n";
        assert_eq!(
            super::locked_version(lock, "wayland-backend").as_deref(),
            Some("0.3.17")
        );
        assert_eq!(super::locked_version(lock, "wayland"), None);
    }
}
