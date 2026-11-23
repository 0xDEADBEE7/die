//! `cargo xtask msrv`: `cargo check` of the workspace with the Rust its `rust-version` names,
//! the oldest DIE builds with. rust-toolchain.toml pins a newer one for the checks, so rustfmt
//! and `budget`'s counts can't move; a build with any Rust from `rust-version` up works too
//! (docs/using-die.md, "What a build needs"). rustup runs it, at `X.Y.0` for a `rust-version`
//! of `X.Y`, the oldest that allows. Every `ci` runs it: clippy's `incompatible_msrv` finds a
//! newer std function or type, but not a newer trait impl or macro, such as 1.96's
//! `From<T>` for `LazyLock` or `assert_matches!`.

use crate::util::{Result, read, run as run_cmd};
use std::path::Path;
use std::process::{Command, Stdio};

/// The toolchain for the workspace's `rust-version`, as rustup names it.
pub fn toolchain(root: &Path) -> Result<String> {
    let path = root.join("Cargo.toml");
    let t: toml::Table = read(&path)?
        .parse()
        .map_err(|e| format!("{}: {e}", path.display()))?;
    t.get("workspace")
        .and_then(|w| w.get("package"))
        .and_then(|p| p.get("rust-version"))
        .and_then(|v| v.as_str())
        .map(full_version)
        .ok_or_else(|| format!("{}: no workspace.package.rust-version", path.display()))
}

/// `X.Y` as `X.Y.0`: rustup's `1.95` is the newest 1.95 release, not 1.95.0.
pub fn full_version(v: &str) -> String {
    match v.split('.').count() {
        2 => format!("{v}.0"),
        _ => v.to_string(),
    }
}

/// `target_dir` holds the build cache between runs.
pub fn run(root: &Path, target_dir: &Path) -> Result {
    let v = toolchain(root)?;
    let rustup = |args: &[&str]| {
        let mut cmd = Command::new("rustup");
        cmd.current_dir(root).arg("run").arg(&v).args(args);
        cmd
    };
    let out = rustup(&["rustc", "--version"])
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("msrv: rustup: {e}. The check needs rustup"))?;
    if !out.status.success() {
        return Err(format!(
            "msrv: rustup has no Rust {v}, which the workspace's rust-version names. \
             `rustup toolchain install {v} --profile minimal` installs it"
        ));
    }
    let version = String::from_utf8_lossy(&out.stdout);
    let version: Vec<&str> = version.split_whitespace().take(2).collect();
    let mut check = rustup(&[
        "cargo",
        "check",
        "--workspace",
        "--all-targets",
        "--locked",
        "--offline",
        "--quiet",
        "--target-dir",
    ]);
    run_cmd(check.arg(target_dir)).map_err(|e| format!("msrv: {e}"))?;
    println!("msrv: ok ({})", version.join(" "));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parts(v: &str) -> Vec<u32> {
        v.split('.').map(|n| n.parse().unwrap()).collect()
    }

    #[test]
    fn a_two_part_version_is_its_first_release() {
        assert_eq!(full_version("1.95"), "1.95.0");
        assert_eq!(full_version("1.95.1"), "1.95.1");
    }

    #[test]
    fn the_pin_is_no_older_than_rust_version() {
        let root = crate::util::root();
        let (msrv, pin) = (
            toolchain(&root).unwrap(),
            crate::linux::channel(&root).unwrap(),
        );
        assert!(
            parts(&msrv) <= parts(&pin),
            "rust-version {msrv}, pin {pin}"
        );
    }
}
