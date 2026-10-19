//! `cargo xtask clean-build`: D16's check that DIE builds with cargo and the
//! platform's C toolchain as the linker driver only: no C compiler, no
//! `pkg-config` (docs/records/platforms.md §5.5).
//!
//! First, the build graph of each target DIE supports may hold no C-building
//! crate, except one on that target's allowlist: a crate that is present but
//! compiles nothing, with the reason. `cargo tree --target` checks every target
//! from this host.
//!
//! Then the build runs from an empty environment with `--locked --offline` and a
//! fresh target directory, so every build script runs. The linker is named by
//! its full path. The C compilers, `ar` and `pkg-config` on PATH, and `CC`,
//! `CXX` and `AR`, are stand-ins that fail, so a build script that compiles C or
//! needs a system library fails the build. Each stand-in also logs its calls,
//! because a build script can try one, carry on without it, and pass, though on
//! a machine with a real one it would compile C. A call the allowlist doesn't
//! name fails the build too. That proves the allowlist's "compiles nothing" on
//! this host.

use crate::util::{Result, TempDir, cargo, output, rustc, write};
use std::path::Path;
use std::process::{Command, Stdio};

pub const STAND_INS: &[&str] = &[
    "cc",
    "c++",
    "clang",
    "clang++",
    "gcc",
    "g++",
    "zig",
    "ar",
    "pkg-config",
];

/// Crates whose job is to compile C or find a system library for it.
const C_BUILDERS: &[&str] = &[
    "cc",
    "gcc",
    "cmake",
    "pkg-config",
    "system-deps",
    "vcpkg",
    "bindgen",
    "cxx-build",
    "autotools",
    "nasm-rs",
    "zig",
];

const LINKER: &str = "/usr/bin/cc";

/// A C builder that may sit in a build graph because nothing in it compiles C:
/// `builder`, as a dependency of `via`. `calls` names the stand-ins `via`'s
/// build script may call and carry on without.
pub struct Allowed {
    pub builder: &'static str,
    pub via: &'static str,
    pub why: &'static str,
    pub calls: &'static [&'static str],
}

/// winit's Linux backends (docs/records/platforms.md §2.4).
pub const LINUX: &[Allowed] = &[
    Allowed {
        builder: "cc",
        via: "wayland-backend",
        why: "its build script compiles C only with wayland-backend's own `log` feature, \
              which is off (wayland-client's `log` doesn't turn it on). `cargo xtask guards` \
              turns it on and expects the cross-check to fail",
        calls: &[],
    },
    Allowed {
        builder: "pkg-config",
        via: "wayland-sys",
        why: "not called under winit's `wayland-dlopen`: Wayland loads at run time",
        calls: &[],
    },
    Allowed {
        builder: "pkg-config",
        via: "x11-dl",
        why: "asks for the X11 libraries' directory only, and carries on without one",
        calls: &["pkg-config"],
    },
];

/// The targets whose build graphs are checked, and what each may hold.
/// FreeBSD joins with the Linux list at M2 (platforms.md §5.5).
pub const GRAPHS: &[(&str, &[Allowed])] = &[
    ("aarch64-apple-darwin", &[]),
    ("x86_64-unknown-linux-gnu", LINUX),
    ("aarch64-unknown-linux-gnu", LINUX),
];

pub fn run(root: &Path, keep: bool) -> Result {
    let host = host()?;
    for (target, allowed) in GRAPHS {
        graph(root, target, allowed)?;
    }
    lockfile_only(root)?;
    let mut tmp = TempDir::new("clean-build")?;
    if keep {
        tmp.keep();
    }
    let calls = build(root, tmp.path(), &host, &["--workspace", "--all-targets"])?;
    let targets: Vec<&str> = GRAPHS.iter().map(|g| g.0).collect();
    println!(
        "clean-build: ok (graphs: {}; build on {host}: empty environment, --locked --offline, no C compiler or pkg-config{calls})",
        targets.join(", ")
    );
    Ok(())
}

pub fn host() -> Result<String> {
    let v = output(Command::new(rustc()).arg("-vV"))?;
    v.lines()
        .find_map(|l| l.strip_prefix("host: "))
        .map(String::from)
        .ok_or_else(|| "rustc -vV: no host".into())
}

/// Fail if `target`'s build graph has a C builder that `allowed` doesn't name,
/// or if an allowlist entry is no longer needed.
fn graph(root: &Path, target: &str, allowed: &[Allowed]) -> Result {
    let tree = output(Command::new(cargo()).current_dir(root).args([
        "tree",
        "--workspace",
        "--locked",
        "--offline",
        "-e",
        "normal,build",
        "--prefix",
        "depth",
        "--format",
        "{p}",
        "--target",
        target,
    ]))?;
    check_graph(target, &c_builder_edges(&tree), allowed)
}

/// Each C builder in a `cargo tree --prefix depth --format {p}` listing, with
/// the crate that depends on it: (builder, dependent), by name.
pub fn c_builder_edges(tree: &str) -> Vec<(String, String)> {
    let mut path: Vec<String> = Vec::new();
    let mut edges = Vec::new();
    for line in tree.lines() {
        let digits = line.chars().take_while(char::is_ascii_digit).count();
        let Ok(depth) = line[..digits].parse::<usize>() else {
            continue;
        };
        let name = line[digits..]
            .split_whitespace()
            .next()
            .unwrap_or("")
            .to_string();
        path.truncate(depth);
        if depth > 0 && C_BUILDERS.contains(&name.as_str()) {
            let parent = path.last().cloned().unwrap_or_default();
            if !edges.contains(&(name.clone(), parent.clone())) {
                edges.push((name.clone(), parent));
            }
        }
        path.push(name);
    }
    edges
}

pub fn check_graph(target: &str, edges: &[(String, String)], allowed: &[Allowed]) -> Result {
    let mut errors = Vec::new();
    for (builder, via) in edges {
        if !allowed.iter().any(|a| a.builder == builder && a.via == via) {
            errors.push(format!(
                "{builder}, through {via}. Find the dependency that brought it with \
                 `cargo tree --target {target} -i {builder}`, and drop it or turn off the \
                 feature that needs it. If {via} compiles nothing, it may go on clean.rs's \
                 allowlist for this target, with the reason"
            ));
        }
    }
    for a in allowed {
        if !edges.iter().any(|(b, v)| a.builder == b && a.via == v) {
            errors.push(format!(
                "the allowlist names {} through {}, which is no longer in the graph; remove it",
                a.builder, a.via
            ));
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "clean-build: the {target} build graph (D16):\n  {}",
            errors.join("\n  ")
        ))
    }
}

/// A C builder in Cargo.lock but in none of the checked graphs (for a platform
/// DIE doesn't build for, such as Android) is only a warning.
fn lockfile_only(root: &Path) -> Result {
    let lock = std::fs::read_to_string(root.join("Cargo.lock")).unwrap_or_default();
    for b in C_BUILDERS {
        if !lock.contains(&format!("name = \"{b}\"\n")) {
            continue;
        }
        let in_a_graph = GRAPHS
            .iter()
            .any(|(_, allowed)| allowed.iter().any(|a| a.builder == *b));
        if !in_a_graph {
            println!(
                "clean-build: warning: {b} is in Cargo.lock, though in none of the checked build graphs"
            );
        }
    }
    Ok(())
}

/// The allowlist for builds for `target`: its graph's, else Linux's for another
/// Linux target, else none.
pub fn allowlist(target: &str) -> &'static [Allowed] {
    match GRAPHS.iter().find(|g| g.0 == target) {
        Some(g) => g.1,
        None if target.contains("-linux-") => LINUX,
        None => &[],
    }
}

/// Where `clean_cargo`'s stand-ins log their calls, one per line: the package
/// whose build script called, the stand-in, its arguments.
fn calls_log(tmp: &Path) -> std::path::PathBuf {
    tmp.join("stand-in-calls.log")
}

/// `cargo` in `dir` from an empty environment, with `rustc` and the host's
/// linker named by full path, and stand-ins in `tmp` that log their calls and
/// fail, for the C compilers, `ar` and `pkg-config`, on PATH and in `CC`, `CXX`
/// and `AR`. Check the log with `check_calls` after a build that passes.
pub fn clean_cargo(dir: &Path, tmp: &Path, host: &str, rustc: &Path) -> Result<Command> {
    clean_command(&cargo(), dir, tmp, host, rustc)
}

/// `clean_cargo`'s environment for `program`, a cargo or a cargo subcommand
/// such as the toolchain's `cargo-clippy`.
pub fn clean_command(
    program: &Path,
    dir: &Path,
    tmp: &Path,
    host: &str,
    rustc: &Path,
) -> Result<Command> {
    let bin = tmp.join("bin");
    let log = calls_log(tmp);
    let log = log
        .to_str()
        .filter(|l| !l.contains('\''))
        .ok_or_else(|| format!("{}: the stand-ins can't log to this path", log.display()))?;
    // Remove an earlier run's log.
    let _ = std::fs::remove_file(log);
    for s in STAND_INS {
        let p = bin.join(s);
        write(
            &p,
            &format!(
                "#!/bin/sh\nprintf '%s %s %s\\n' \"${{CARGO_PKG_NAME:-?}}\" {s} \"$*\" >> '{log}'\n\
                 echo \"clean-build: {s} was called: {s} $*. DIE builds with no C compiler or pkg-config (D16)\" >&2\nexit 1\n"
            ),
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
    let mut cmd = Command::new(program);
    cmd.current_dir(dir)
        .env_clear()
        .env(
            "PATH",
            format!("{}:/usr/bin:/bin:/usr/sbin:/sbin", bin.display()),
        )
        .env("HOME", &home)
        .env("CARGO_HOME", &cargo_home)
        .env("TMPDIR", tmp)
        .env("RUSTC", rustc)
        .env(linker_var, LINKER)
        .env("CC", bin.join("cc"))
        .env("CXX", bin.join("c++"))
        .env("AR", bin.join("ar"))
        .stdin(Stdio::null());
    Ok(cmd)
}

/// Run a `clean_cargo` command. On failure, lead with any stand-in that was
/// called and the build script that called it.
pub fn run_clean(cmd: &mut Command, what: &str) -> Result {
    let out = cmd.output().map_err(|e| e.to_string())?;
    if out.status.success() {
        return Ok(());
    }
    let err = String::from_utf8_lossy(&out.stderr);
    let mut summary: Vec<String> = Vec::new();
    for l in err.lines() {
        if let Some(rest) = l
            .trim()
            .strip_prefix("error: failed to run custom build command for ")
        {
            summary.push(format!("the build script of {rest} failed"));
        } else if let Some(i) = l.find("clean-build: ") {
            let called = l[i..].to_string();
            if !summary.contains(&called) {
                summary.push(called);
            }
        }
    }
    let lead = if summary.is_empty() {
        String::new()
    } else {
        format!("\n  {}", summary.join("\n  "))
    };
    Err(format!("{what} failed:{lead}\n\n{err}"))
}

/// After a `clean_cargo` build that passed: fail on any stand-in call that
/// `allowed` doesn't name for the package that made it. Otherwise, a line on
/// the calls it allows, if any.
pub fn check_calls(tmp: &Path, allowed: &[Allowed]) -> Result<String> {
    let log = std::fs::read_to_string(calls_log(tmp)).unwrap_or_default();
    // (package, stand-in, the first call's arguments, how many calls), in order.
    let mut calls: Vec<(&str, &str, &str, usize)> = Vec::new();
    for line in log.lines() {
        let mut f = line.splitn(3, ' ');
        let (pkg, tool, args) = (
            f.next().unwrap_or(""),
            f.next().unwrap_or(""),
            f.next().unwrap_or(""),
        );
        match calls.iter_mut().find(|c| c.0 == pkg && c.1 == tool) {
            Some(c) => c.3 += 1,
            None => calls.push((pkg, tool, args, 1)),
        }
    }
    let mut notes = Vec::new();
    let mut errors = Vec::new();
    for (pkg, tool, args, n) in calls {
        if allowed
            .iter()
            .any(|a| a.via == pkg && a.calls.contains(&tool))
        {
            notes.push(format!("{tool} by {pkg} ({n}, allowed)"));
        } else {
            let times = if n == 1 {
                "once".into()
            } else {
                format!("{n} times, first")
            };
            errors.push(format!(
                "clean-build: {tool} was called by {pkg}'s build script ({times} \
                 `{tool} {args}`), which carried on when it failed. With a real {tool} it would \
                 run, and DIE builds with no C compiler or pkg-config (D16). If the build \
                 script needs nothing from it, name it in the `calls` of clean.rs's allowlist \
                 entry for {pkg}, with the reason"
            ));
        }
    }
    if errors.is_empty() {
        Ok(if notes.is_empty() {
            String::new()
        } else {
            format!("; stand-ins called: {}", notes.join(", "))
        })
    } else {
        Err(errors.join("\n"))
    }
}

/// `cargo build` in `dir` from an empty environment, with C compilers that fail.
pub fn build(dir: &Path, tmp: &Path, host: &str, args: &[&str]) -> Result<String> {
    let mut cmd = clean_cargo(dir, tmp, host, &rustc())?;
    cmd.args(["build", "--locked", "--offline", "--target-dir"])
        .arg(tmp.join("target"))
        .args(args);
    run_clean(
        &mut cmd,
        "clean-build: the build in an empty environment with no C compiler",
    )?;
    check_calls(tmp, allowlist(host))
}

#[cfg(test)]
mod tests {
    use super::*;

    const TREE: &str = "\
0die-render v0.0.0 (/w/crates/die-render)
1winit v0.30.13
2wayland-backend v0.3.17
3wayland-sys v0.31.11
4pkg-config v0.3.34
3cc v1.8.0
4shlex v2.0.1
2x11-dl v2.21.0
3pkg-config v0.3.34
2wayland-client v0.31.15
3wayland-backend v0.3.17 (*)
0xtask v0.0.0 (/w/xtask)
1serde v1.0.228
";

    fn edges(list: &[(&str, &str)]) -> Vec<(String, String)> {
        list.iter()
            .map(|(b, v)| (b.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn builders_and_their_dependents() {
        assert_eq!(
            c_builder_edges(TREE),
            edges(&[
                ("pkg-config", "wayland-sys"),
                ("cc", "wayland-backend"),
                ("pkg-config", "x11-dl"),
            ])
        );
        assert!(c_builder_edges("0die-core v0.0.0 (/w)\n1ropey v1.6.1\n").is_empty());
    }

    #[test]
    fn graphs_need_the_allowlist() {
        let found = c_builder_edges(TREE);
        check_graph("x86_64-unknown-linux-gnu", &found, LINUX).unwrap();
        let err = check_graph("aarch64-apple-darwin", &found, &[]).unwrap_err();
        assert!(err.contains("cc, through wayland-backend"), "{err}");
        assert_eq!(err.matches("cargo tree --target").count(), 3, "{err}");
        // The same builder through a crate the allowlist doesn't name.
        let more = [found.clone(), edges(&[("cc", "ring")])].concat();
        let err = check_graph("x86_64-unknown-linux-gnu", &more, LINUX).unwrap_err();
        assert!(err.contains("cc, through ring"), "{err}");
        // An entry nothing needs any more.
        let err = check_graph("x86_64-unknown-linux-gnu", &found[..2], LINUX).unwrap_err();
        assert!(err.contains("pkg-config through x11-dl"), "{err}");
    }

    #[test]
    fn stand_in_calls_need_the_allowlist() {
        let tmp = crate::util::TempDir::new("calls").unwrap();
        // No log: nothing was called.
        assert_eq!(check_calls(tmp.path(), &[]).unwrap(), "");
        let log = "x11-dl pkg-config --variable=libdir x11\n\
                   x11-dl pkg-config --variable=libdir xext\n";
        write(&calls_log(tmp.path()), log).unwrap();
        assert_eq!(
            check_calls(tmp.path(), LINUX).unwrap(),
            "; stand-ins called: pkg-config by x11-dl (2, allowed)"
        );
        // x11-dl's calls on a target whose allowlist is empty, and another
        // crate's build script that carried on without a C compiler.
        let err = check_calls(tmp.path(), &[]).unwrap_err();
        assert!(err.contains("pkg-config was called by x11-dl's build script (2 times, first `pkg-config --variable=libdir x11`)"), "{err}");
        write(
            &calls_log(tmp.path()),
            &format!("{log}ring cc -c shim.c -o shim.o\n"),
        )
        .unwrap();
        let err = check_calls(tmp.path(), LINUX).unwrap_err();
        assert!(
            err.contains("cc was called by ring's build script (once `cc -c shim.c -o shim.o`)"),
            "{err}"
        );
        assert!(!err.contains("x11-dl"), "{err}");
        // An allowed crate calling a stand-in its entry doesn't name.
        write(&calls_log(tmp.path()), "wayland-backend cc -c log_shim.c\n").unwrap();
        assert!(check_calls(tmp.path(), LINUX).is_err());
    }

    #[test]
    fn allowlists_by_target() {
        let vias = |a: &[Allowed]| a.iter().map(|x| x.via).collect::<Vec<_>>();
        assert!(allowlist("aarch64-apple-darwin").is_empty());
        assert_eq!(vias(allowlist("x86_64-unknown-linux-gnu")), vias(LINUX));
        assert_eq!(vias(allowlist("x86_64-unknown-linux-musl")), vias(LINUX));
        assert!(allowlist("x86_64-unknown-freebsd").is_empty());
    }
}
