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
//!
//! On a Linux host, the build needs no `-dev` packages but libc's (D16). The
//! linker would find a system library through one, so the host may have none
//! installed, and no pkg-config: clean-build reads dpkg's list and fails
//! otherwise. Every binary and shared object the build leaves (the workspace's
//! binaries, tests, examples, build scripts and proc macros) may link glibc,
//! its loader and the unwinder only (their ELF `NEEDED` entries): X11,
//! Wayland, xkbcommon, fontconfig and Vulkan load at run time.
//! `cargo xtask ci --linux` runs this on aarch64 Linux, in a container that
//! purges the other `-dev` packages first.

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
    let linux = host.contains("-linux-");
    let dev = if linux {
        libc_dev_only()?
    } else {
        String::new()
    };
    let mut tmp = TempDir::new("clean-build")?;
    if keep {
        tmp.keep();
    }
    let calls = build(root, tmp.path(), &host, &["--workspace", "--all-targets"])?;
    let linked = if linux {
        elf_needed(&tmp.path().join("target/debug"))?
    } else {
        String::new()
    };
    let targets: Vec<&str> = GRAPHS.iter().map(|g| g.0).collect();
    println!(
        "clean-build: ok (graphs: {}; build on {host}: empty environment, --locked --offline, no C compiler or pkg-config{dev}{calls}{linked})",
        targets.join(", ")
    );
    Ok(())
}

/// The `-dev` packages a Linux build may find, by Debian name: the libc
/// development files and the C toolchain's own, which the linker driver needs
/// (D16). `{n}` stands for a version number. libstdc++'s aren't here: DIE links
/// no C++.
pub const LIBC_DEV: &[&str] = &[
    "libc6-dev",
    "linux-libc-dev",
    "libcrypt-dev",
    "libnsl-dev",
    "libtirpc-dev",
    "libgcc-{n}-dev",
];

/// pkg-config's packages. The stand-in shadows it, but it goes too.
pub const PKG_CONFIG: &[&str] = &["pkg-config", "pkgconf", "pkgconf-bin"];

/// Whether a Debian package must be absent for the Linux clean build: a
/// `-dev` package that `LIBC_DEV` doesn't name, or pkg-config.
pub fn forbidden_package(name: &str) -> bool {
    (name.ends_with("-dev") || PKG_CONFIG.contains(&name))
        && !LIBC_DEV.iter().any(|p| version_match(p, name))
}

/// `name` is `pattern`, with `{n}` matching one or more digits.
fn version_match(pattern: &str, name: &str) -> bool {
    match pattern.split_once("{n}") {
        None => pattern == name,
        Some((pre, post)) => name
            .strip_prefix(pre)
            .and_then(|n| n.strip_suffix(post))
            .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit())),
    }
}

/// `LIBC_DEV` as an extended regular expression that matches a whole name.
pub fn libc_dev_ere() -> String {
    let alts: Vec<String> = LIBC_DEV
        .iter()
        .map(|p| {
            p.split("{n}")
                .map(|part| {
                    part.chars()
                        .map(|c| {
                            if ".[]()*+?{}|^$\\".contains(c) {
                                format!("\\{c}")
                            } else {
                                c.to_string()
                            }
                        })
                        .collect::<String>()
                })
                .collect::<Vec<_>>()
                .join("[0-9]+")
        })
        .collect();
    format!("^({})$", alts.join("|"))
}

/// Fail if this Linux host has a package `forbidden_package` names, from
/// dpkg's list. Otherwise, a line on the `-dev` packages it has, for the
/// summary.
fn libc_dev_only() -> Result<String> {
    let list = output(Command::new("dpkg-query").args([
        "--show",
        "--showformat",
        "${db:Status-Status} ${Package}\n",
    ]))
    .map_err(|e| {
        format!(
            "clean-build: on Linux, it checks with dpkg-query that the host has no -dev \
             packages but libc's (D16), and dpkg-query failed:\n{e}"
        )
    })?;
    let (dev, forbidden) = installed_dev(&list);
    if !forbidden.is_empty() {
        return Err(format!(
            "clean-build: this host has -dev packages beyond libc's: {}. The linker finds \
             their libraries, so a build here can't show that DIE needs none (D16). Run it \
             on a host without them: `cargo xtask ci --linux` purges them in its container",
            forbidden.join(", ")
        ));
    }
    Ok(format!("; -dev packages: {}", dev.join(", ")))
}

/// From `dpkg-query --showformat '${db:Status-Status} ${Package}\n'`: the
/// installed `-dev` packages that are allowed, and the packages
/// `forbidden_package` names. A package counts unless it's `not-installed`
/// or only `config-files` is left.
pub fn installed_dev(list: &str) -> (Vec<String>, Vec<String>) {
    let mut dev = Vec::new();
    let mut forbidden = Vec::new();
    for line in list.lines() {
        let Some((status, name)) = line.split_once(' ') else {
            continue;
        };
        if matches!(status, "not-installed" | "config-files") {
            continue;
        }
        if forbidden_package(name) {
            forbidden.push(name.to_string());
        } else if name.ends_with("-dev") {
            dev.push(name.to_string());
        }
    }
    for v in [&mut dev, &mut forbidden] {
        v.sort();
        v.dedup();
    }
    (dev, forbidden)
}

/// The shared libraries a Linux binary may need (platforms.md §5.5): glibc's,
/// its loader, and the unwinder. Matched as name prefixes.
const ELF_NEEDED: &[&str] = &[
    "libc.so.",
    "libm.so.",
    "libdl.so.",
    "libgcc_s.so.",
    "ld-linux",
];

/// Check the `NEEDED` entries of every ELF executable and shared object under
/// `dir`, a profile's target directory, against `ELF_NEEDED`, with binutils'
/// `readelf`. A line on what they need, for the summary.
fn elf_needed(dir: &Path) -> Result<String> {
    let bins = linked_elves(dir)?;
    if bins.is_empty() {
        return Err(format!(
            "clean-build: no binaries in {} for the ELF check",
            dir.display()
        ));
    }
    let mut all: Vec<String> = Vec::new();
    let mut errors = Vec::new();
    for bin in &bins {
        let out = output(
            Command::new("readelf")
                .args(["--dynamic", "--wide"])
                .arg(bin),
        )?;
        for lib in needed(&out) {
            if !ELF_NEEDED.iter().any(|ok| lib.starts_with(ok)) {
                errors.push(format!(
                    "{} needs {lib}, which isn't glibc, its loader or the unwinder. DIE loads \
                     system libraries at run time (D16); `cargo tree -i` on the crate that \
                     links it shows where it comes from",
                    bin.display()
                ));
            }
            if !all.contains(&lib) {
                all.push(lib);
            }
        }
    }
    if !errors.is_empty() {
        return Err(format!(
            "clean-build: ELF NEEDED:\n  {}",
            errors.join("\n  ")
        ));
    }
    all.sort();
    let top: Vec<String> = bins
        .iter()
        .filter(|b| b.parent() == Some(dir))
        .filter_map(|b| b.file_name().map(|n| n.to_string_lossy().into_owned()))
        .collect();
    Ok(format!(
        "; ELF NEEDED of {} and {} tests, examples, build scripts and proc macros: {}",
        top.join(", "),
        bins.len() - top.len(),
        all.join(", ")
    ))
}

/// Every ELF executable and shared object under `dir`, once each (cargo
/// hard-links a binary into `dir` from `deps/`), shallowest first: the
/// workspace's binaries, then the tests, examples, build scripts and proc
/// macros. Object files and the incremental cache are left out.
pub fn linked_elves(dir: &Path) -> Result<Vec<std::path::PathBuf>> {
    use std::os::unix::fs::MetadataExt;
    let mut found = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let entries = std::fs::read_dir(&d).map_err(|e| format!("{}: {e}", d.display()))?;
        for e in entries.flatten() {
            let p = e.path();
            let Ok(meta) = std::fs::symlink_metadata(&p) else {
                continue;
            };
            if meta.is_dir() {
                if p.file_name().is_some_and(|n| n != "incremental") {
                    stack.push(p);
                }
            } else if meta.is_file() && is_linked_elf(&p) {
                found.push((p.components().count(), p, (meta.dev(), meta.ino())));
            }
        }
    }
    found.sort();
    let mut seen = std::collections::HashSet::new();
    Ok(found
        .into_iter()
        .filter(|f| seen.insert(f.2))
        .map(|f| f.1)
        .collect())
}

/// An ELF file of type `ET_EXEC` or `ET_DYN`: one the dynamic linker loads.
fn is_linked_elf(path: &Path) -> bool {
    use std::io::Read;
    let mut head = [0; 18];
    if std::fs::File::open(path)
        .and_then(|mut f| f.read_exact(&mut head))
        .is_err()
        || head[..4] != *b"\x7fELF"
    {
        return false;
    }
    let ty = [head[16], head[17]];
    // e_ident[EI_DATA]: 2 is big-endian.
    let ty = if head[5] == 2 {
        u16::from_be_bytes(ty)
    } else {
        u16::from_le_bytes(ty)
    };
    matches!(ty, 2 | 3)
}

/// The libraries in `readelf --dynamic` output's `NEEDED` entries.
pub fn needed(readelf: &str) -> Vec<String> {
    readelf
        .lines()
        .filter(|l| l.contains("(NEEDED)"))
        .filter_map(|l| {
            let start = l.find('[')? + 1;
            let end = start + l[start..].find(']')?;
            Some(l[start..end].to_string())
        })
        .collect()
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
    fn elf_needed_entries() {
        let out = "\
Dynamic section at offset 0x5c3d8 contains 29 entries:
  Tag        Type                         Name/Value
 0x0000000000000001 (NEEDED)             Shared library: [libgcc_s.so.1]
 0x0000000000000001 (NEEDED)             Shared library: [libc.so.6]
 0x0000000000000001 (NEEDED)             Shared library: [ld-linux-aarch64.so.1]
 0x000000000000000c (INIT)               0x6c80
";
        assert_eq!(
            needed(out),
            ["libgcc_s.so.1", "libc.so.6", "ld-linux-aarch64.so.1"]
        );
        assert!(needed("There is no dynamic section in this file.\n").is_empty());
    }

    #[test]
    fn only_libc_dev_packages() {
        // From the rust:1.96.0 image's list, trimmed.
        let list = "\
installed libc6-dev
installed libcrypt-dev
installed linux-libc-dev
installed libgcc-14-dev
installed libstdc++-14-dev
installed libx11-dev
installed libssl-dev
installed pkgconf
installed pkgconf-bin
installed gcc-14
installed libx11-6
config-files libfontconfig-dev
not-installed libz-dev
half-installed libxcb1-dev
";
        let (dev, forbidden) = installed_dev(list);
        assert_eq!(
            dev,
            [
                "libc6-dev",
                "libcrypt-dev",
                "libgcc-14-dev",
                "linux-libc-dev"
            ]
        );
        assert_eq!(
            forbidden,
            [
                "libssl-dev",
                "libstdc++-14-dev",
                "libx11-dev",
                "libxcb1-dev",
                "pkgconf",
                "pkgconf-bin"
            ]
        );
        assert!(forbidden_package("pkg-config"));
        assert!(forbidden_package("libgcc--dev"));
        assert!(forbidden_package("libgcc-x-dev"));
        assert!(!forbidden_package("libgcc-9-dev"));
        assert_eq!(
            libc_dev_ere(),
            "^(libc6-dev|linux-libc-dev|libcrypt-dev|libnsl-dev|libtirpc-dev|libgcc-[0-9]+-dev)$"
        );
    }

    #[test]
    fn every_linked_elf_once() {
        let tmp = crate::util::TempDir::new("elves").unwrap();
        let dir = tmp.path();
        // An ELF header up to e_type: 64-bit, little- or big-endian.
        let elf = |big: bool, ty: u16| {
            let mut h = b"\x7fELF\x02\x01\x01\0\0\0\0\0\0\0\0\0".to_vec();
            if big {
                h[5] = 2;
                h.extend(ty.to_be_bytes());
            } else {
                h.extend(ty.to_le_bytes());
            }
            h.extend([0; 46]);
            h
        };
        let put = |rel: &str, bytes: &[u8]| {
            let p = dir.join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, bytes).unwrap();
        };
        put("deps/die-0a1b", &elf(false, 3));
        std::fs::hard_link(dir.join("deps/die-0a1b"), dir.join("die")).unwrap();
        put("deps/plant_x11-2c3d", &elf(false, 3));
        put("deps/libserde_derive-4e5f.so", &elf(false, 3));
        put("build/x-1/build_script_build-1", &elf(true, 2));
        put("deps/die-0a1b.o", &elf(false, 1));
        put("incremental/die-1/s-2/x", &elf(false, 3));
        put("deps/libdie_core-6a7b.rlib", b"!<arch>\n");
        put("deps/short", b"\x7fEL");
        put("die.d", b"die: src/main.rs\n");
        let rel: Vec<String> = linked_elves(dir)
            .unwrap()
            .iter()
            .map(|p| p.strip_prefix(dir).unwrap().display().to_string())
            .collect();
        assert_eq!(
            rel,
            [
                "die",
                "deps/libserde_derive-4e5f.so",
                "deps/plant_x11-2c3d",
                "build/x-1/build_script_build-1",
            ]
        );
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
