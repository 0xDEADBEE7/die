//! `cargo xtask clean-build`: D16's check that DIE builds with cargo and the
//! Xcode Command Line Tools only.
//!
//! The build runs from an empty environment with `--locked --offline` and a fresh
//! target directory, so every build script runs. The linker is named by its full
//! path, and `cc`, `clang`, `gcc` and `zig` on PATH are stand-ins that fail, so a
//! build script that compiles C fails the build. Separately, no C-building crate
//! may be in the build graph for this Mac's target; one in the lockfile only (for
//! another platform) is a warning.

use crate::util::{Result, TempDir, cargo, output, rustc, write};
use std::path::Path;
use std::process::{Command, Stdio};

const STAND_INS: &[&str] = &["cc", "c++", "clang", "clang++", "gcc", "g++", "zig"];
const C_BUILDERS: &[&str] = &["cc", "cmake", "pkg-config", "bindgen", "zig"];
const LINKER: &str = "/usr/bin/cc";

pub fn run(root: &Path, keep: bool) -> Result {
    let host = host()?;
    graph(root, &host)?;
    let mut tmp = TempDir::new("clean-build")?;
    if keep {
        tmp.keep();
    }
    build(root, tmp.path(), &host, &["--workspace", "--all-targets"])?;
    println!("clean-build: ok (empty environment, --locked --offline, no C compiler)");
    Ok(())
}

pub fn host() -> Result<String> {
    let v = output(Command::new(rustc()).arg("-vV"))?;
    v.lines()
        .find_map(|l| l.strip_prefix("host: "))
        .map(String::from)
        .ok_or_else(|| "rustc -vV: no host".into())
}

/// Fail if a C-building crate is in `host`'s build graph.
fn graph(root: &Path, host: &str) -> Result {
    let tree = output(
        Command::new(cargo())
            .current_dir(root)
            .args([
                "tree",
                "--workspace",
                "--locked",
                "--offline",
                "-e",
                "normal,build",
                "--prefix",
                "none",
                "--target",
            ])
            .arg(host)
            .args(["--format", "{p}"]),
    )?;
    let found: Vec<&str> = tree
        .lines()
        .filter(|l| C_BUILDERS.contains(&l.split_whitespace().next().unwrap_or("")))
        .collect();
    if !found.is_empty() {
        return Err(format!(
            "clean-build: C-building crates in the {host} build graph (D16):\n  {}",
            found.join("\n  ")
        ));
    }
    let lock = std::fs::read_to_string(root.join("Cargo.lock")).unwrap_or_default();
    for b in C_BUILDERS {
        if lock.contains(&format!("name = \"{b}\"\n")) {
            println!(
                "clean-build: warning: {b} is in Cargo.lock, though not in the {host} build graph"
            );
        }
    }
    Ok(())
}

/// `cargo build` in `dir` from an empty environment, with C compilers that fail.
pub fn build(dir: &Path, tmp: &Path, host: &str, args: &[&str]) -> Result {
    let bin = tmp.join("bin");
    for s in STAND_INS {
        let p = bin.join(s);
        write(
            &p,
            &format!("#!/bin/sh\necho \"clean-build: {s} was called: $*\" >&2\nexit 1\n"),
        )?;
        let mut perm = std::fs::metadata(&p)
            .map_err(|e| e.to_string())?
            .permissions();
        std::os::unix::fs::PermissionsExt::set_mode(&mut perm, 0o755);
        std::fs::set_permissions(&p, perm).map_err(|e| e.to_string())?;
    }
    let home = std::env::var("HOME").map_err(|_| "HOME isn't set")?;
    let cargo_home = std::env::var("CARGO_HOME").unwrap_or_else(|_| format!("{home}/.cargo"));
    let linker_var = format!(
        "CARGO_TARGET_{}_LINKER",
        host.to_uppercase().replace('-', "_")
    );
    let out = Command::new(cargo())
        .current_dir(dir)
        .env_clear()
        .env(
            "PATH",
            format!("{}:/usr/bin:/bin:/usr/sbin:/sbin", bin.display()),
        )
        .env("HOME", &home)
        .env("CARGO_HOME", &cargo_home)
        .env("TMPDIR", tmp)
        .env("RUSTC", rustc())
        .env(linker_var, LINKER)
        .args(["build", "--locked", "--offline", "--target-dir"])
        .arg(tmp.join("target"))
        .args(args)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!(
            "clean-build: the build failed in an empty environment:\n{}",
            String::from_utf8_lossy(&out.stderr)
        ))
    }
}
