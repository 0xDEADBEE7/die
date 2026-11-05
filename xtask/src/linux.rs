//! `cargo xtask ci --linux`: the workspace's tests, the oracle and the Linux
//! clean-build on aarch64 Linux, in a container on colima
//! (docs/records/platforms.md §5.5 and §5.6). It's M0's exit test (f): die-core,
//! die-vim and the oracle pass their tests on aarch64 Linux.
//!
//! - The image is `rust:1.96.0` for arm64, pinned by digest and never pulled
//!   here (`--pull never`). Its toolchain must be the one rust-toolchain.toml
//!   pins.
//! - The tree is a fresh copy, as for `ci`: a clone of a revision, or the
//!   working tree. It's copied into the container at a fixed path, so the
//!   target directory's cache stays valid between runs, rather than mounted:
//!   colima shares only your home directory with its VM. The fonts go in the
//!   same way, at `DIE_FONT_DIR`.
//! - Two named volumes keep cargo's registry and the Linux target directory.
//!   The first run fetches the lockfile's crates from crates.io into the
//!   registry; every other container runs with no network. Only crates.io:
//!   a lockfile with another source, or one that doesn't resolve offline on
//!   this machine, stops the run before any fetch.
//! - The image has -dev packages that D16 says the build doesn't need, so the
//!   checks' container purges every one but libc's, and pkg-config, before
//!   the build. clean-build fails if any is left.
//! - Inside, `cargo xtask linux-host` runs the checks, under docker's init, so
//!   ^C stops them and the container goes.
//!
//! xtask doesn't start or stop colima.

use crate::ci::{self, Source, Step, run_steps};
use crate::util::{Result, TempDir, cargo, output, read, rustc, show};
use crate::{clean, fonts, guards};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Instant;

/// `rust:1.96.0` for linux/arm64.
pub const IMAGE: &str =
    "rust@sha256:58fe97504a0e4cbba5d85599619a589923d3e779472a6fb0840d58d1c4ba99d7";
const CONTEXT: &str = "colima";
const REGISTRY_VOLUME: &str = "die-linux-cargo-registry";
const TARGET_VOLUME: &str = "die-linux-target";
/// The registry under the image's `CARGO_HOME`.
const IN_REGISTRY: &str = "/usr/local/cargo/registry";
const IN_TREE: &str = "/die/tree";
const IN_TARGET: &str = "/die/target";
const IN_FONTS: &str = "/die/fonts";
/// The exit code of a container whose registry lacks a crate the lockfile names.
const NEEDS_FETCH: i32 = 75;
/// The one source the networked container may fetch from.
const CRATES_IO: &str = "registry+https://github.com/rust-lang/crates.io-index";
/// On every container this makes, so a run can remove what an interrupted one left.
const LABEL: &str = "die.xtask=ci-linux";

pub fn run(root: &Path, source: Source) -> Result {
    let start = Instant::now();
    let image_rust = preflight()?;
    let tmp = TempDir::at(root.join("target/xtask/linux/tree"))?;
    let tree = tmp.path();
    println!("ci --linux: {}", ci::checkout(root, &source, tree)?);
    let channel = channel(tree)?;
    if image_rust != channel {
        return Err(format!(
            "ci --linux: rust-toolchain.toml pins {channel}, but the image's Rust is {image_rust}. \
             Point xtask/src/linux.rs's IMAGE at rust:{channel} for linux/arm64, by digest; \
             pulling it is a download, so ask first"
        ));
    }
    crates_io_only(tree)?;
    fonts::run_fonts(root)?;
    for v in [REGISTRY_VOLUME, TARGET_VOLUME] {
        output(docker().args(["volume", "create", v]))?;
    }
    // A run stopped before its container started leaves it created, not removed.
    let left = output(docker().args([
        "ps",
        "--all",
        "--quiet",
        "--filter",
        &format!("label={LABEL}"),
        "--filter",
        "status=created",
    ]))?;
    for id in left.split_whitespace() {
        let _ = output(docker().args(["rm", "--force", id]));
    }
    let env = [
        // The image's toolchain, without rustup reading rust-toolchain.toml,
        // whose components and cross targets the image doesn't have.
        ("RUSTUP_TOOLCHAIN", channel.as_str()),
        ("CARGO_TARGET_DIR", IN_TARGET),
        ("DIE_FONT_DIR", IN_FONTS),
    ];
    let fonts = fonts::dir(root);
    let checks = checks_script();
    let mut code = in_container(tree, Some(&fonts), false, &env, &checks)?;
    if code == NEEDS_FETCH {
        // The networked fetch may only add the lockfile's crates.io crates:
        // a lockfile that resolves offline here asks for nothing more.
        output(Command::new(cargo()).current_dir(tree).args([
            "fetch",
            "--locked",
            "--offline",
            "--quiet",
        ]))
        .map_err(|e| {
            format!(
                "ci --linux: the container's registry lacks crates, and the lockfile doesn't \
                 resolve offline on this machine either, so fetching might reach more than \
                 its crates.io crates. Fix the tree, then run this again:\n{e}"
            )
        })?;
        println!(
            "ci --linux: fetching the lockfile's crates from crates.io into the {REGISTRY_VOLUME} \
             volume (later runs use it with no network)"
        );
        match in_container(tree, None, true, &env, "cargo fetch --locked --quiet")? {
            0 => code = in_container(tree, Some(&fonts), false, &env, &checks)?,
            c => return Err(format!("ci --linux: cargo fetch failed (exit {c})")),
        }
    }
    let secs = start.elapsed().as_secs_f64();
    match code {
        0 => {
            println!("\nci --linux: ok in {secs:.0} s");
            Ok(())
        }
        NEEDS_FETCH => Err("ci --linux: cargo fetch left crates missing".into()),
        c => Err(format!("ci --linux: failed (exit {c}) in {secs:.0} s")),
    }
}

/// The checks' container: fail with `NEEDS_FETCH` if the registry lacks a
/// crate the lockfile names (any other fetch error is printed and fails),
/// purge every package `clean::forbidden_package` names (clean-build checks
/// that none is left), then run `linux-host`. `docker cp` keeps the host's
/// owner, so git, which the log plant's copy of the tree asks for its files,
/// is told the tree is safe.
fn checks_script() -> String {
    let keep = clean::libc_dev_ere();
    let pkg_config = clean::PKG_CONFIG.join("|");
    format!(
        "if ! cargo fetch --locked --offline --quiet 2>/tmp/fetch.err; then\n\
         \x20 grep -qF -e 'offline mode (via `--offline`)' -e 'but --offline was specified' \
         /tmp/fetch.err && exit {NEEDS_FETCH}\n\
         \x20 cat /tmp/fetch.err >&2; exit 1\n\
         fi\n\
         dpkg-query --show --showformat '${{db:Status-Status}} ${{Package}}\\n' \
         | sed -n 's/^installed //p' | grep -E -- '-dev$|^({pkg_config})$' \
         | grep -Ev '{keep}' >/tmp/purge || true\n\
         xargs --no-run-if-empty dpkg --purge --force-depends </tmp/purge >/tmp/purge.log 2>&1 \
         || {{ cat /tmp/purge.log >&2; exit 1; }}\n\
         echo \"ci --linux: purged $(wc -l </tmp/purge) packages, every -dev one but libc's \
         and pkg-config (D16)\"\n\
         git config --global --add safe.directory {IN_TREE}\n\
         exec cargo run --quiet --locked --offline --package xtask -- linux-host"
    )
}

/// Fail unless every source in `tree`'s Cargo.lock is crates.io and its
/// `.cargo/config.toml` replaces no source: the run fetches crates.io
/// crates only. A git source can't be fetched here either way: the
/// registry volume doesn't hold cargo's git checkouts.
fn crates_io_only(tree: &Path) -> Result {
    let others = other_sources(&read(&tree.join("Cargo.lock"))?);
    if !others.is_empty() {
        return Err(format!(
            "ci --linux: Cargo.lock has sources other than crates.io:\n  {}\nThe run may fetch \
             crates.io crates only. Fetching from another source is a download to ask about \
             first",
            others.join("\n  ")
        ));
    }
    for name in [".cargo/config.toml", ".cargo/config"] {
        let path = tree.join(name);
        if !path.exists() {
            continue;
        }
        let t: toml::Table = read(&path)?
            .parse()
            .map_err(|e| format!("{}: {e}", path.display()))?;
        if t.contains_key("source") || t.contains_key("registries") {
            return Err(format!(
                "ci --linux: {name} has a [source] or [registries] table, which can send \
                 crates.io's fetches elsewhere. The run may fetch from crates.io only"
            ));
        }
    }
    Ok(())
}

/// Each package in a Cargo.lock whose source isn't `CRATES_IO`, as
/// "name version (source)".
fn other_sources(lock: &str) -> Vec<String> {
    let mut out = Vec::new();
    let (mut name, mut version) = ("", "");
    for line in lock.lines() {
        let value = |key: &str| {
            line.strip_prefix(key)
                .and_then(|v| v.strip_prefix(" = \""))
                .and_then(|v| v.strip_suffix('"'))
        };
        if line == "[[package]]" {
            (name, version) = ("", "");
        } else if let Some(v) = value("name") {
            name = v;
        } else if let Some(v) = value("version") {
            version = v;
        } else if let Some(v) = value("source")
            && v != CRATES_IO
        {
            out.push(format!("{name} {version} ({v})"));
        }
    }
    out
}

fn docker() -> Command {
    let mut c = Command::new("docker");
    c.args(["--context", CONTEXT]);
    c
}

/// rust-toolchain.toml's channel.
fn channel(root: &Path) -> Result<String> {
    let path = root.join("rust-toolchain.toml");
    let t: toml::Table = read(&path)?
        .parse()
        .map_err(|e| format!("{}: {e}", path.display()))?;
    t.get("toolchain")
        .and_then(|t| t.get("channel"))
        .and_then(|c| c.as_str())
        .map(String::from)
        .ok_or_else(|| format!("{}: no toolchain.channel", path.display()))
}

/// colima answers through its docker context and has the pinned image.
/// Returns the image's Rust version.
fn preflight() -> Result<String> {
    if let Err(e) = output(docker().args(["version", "--format", "{{.Server.Version}}"])) {
        return Err(format!(
            "ci --linux: docker can't reach colima through its `{CONTEXT}` context. Start it \
             with `colima start` (xtask doesn't start or stop it), then run this again.\n{e}"
        ));
    }
    let env = output(docker().args([
        "image",
        "inspect",
        "--format",
        "{{range .Config.Env}}{{println .}}{{end}}",
        IMAGE,
    ]))
    .map_err(|_| {
        format!(
            "ci --linux: colima doesn't have the pinned image {IMAGE}. Pulling it is a \
             download, so ask first; then `docker --context {CONTEXT} pull {IMAGE}`"
        )
    })?;
    env.lines()
        .find_map(|l| l.strip_prefix("RUST_VERSION="))
        .map(String::from)
        .ok_or_else(|| format!("ci --linux: the image {IMAGE} doesn't say its Rust version"))
}

/// Run `script` under `sh -e` in a new container from `IMAGE`, with `tree` at
/// `IN_TREE`, `fonts` at `IN_FONTS`, `env`, and the network only if `network`.
/// Its output is ours. Returns its exit code.
fn in_container(
    tree: &Path,
    fonts: Option<&Path>,
    network: bool,
    env: &[(&str, &str)],
    script: &str,
) -> Result<i32> {
    let mut create = docker();
    create.args([
        "create",
        "--rm",
        // docker's init as PID 1 passes the ^C that `docker start --attach`
        // forwards on to cargo, which as PID 1 would ignore it.
        "--init",
        "--pull",
        "never",
        "--platform",
        "linux/arm64",
        "--workdir",
        IN_TREE,
        "--label",
        LABEL,
    ]);
    if !network {
        create.args(["--network", "none"]);
    }
    create
        .arg("--volume")
        .arg(format!("{REGISTRY_VOLUME}:{IN_REGISTRY}"))
        .arg("--volume")
        .arg(format!("{TARGET_VOLUME}:{IN_TARGET}"));
    for (k, v) in env {
        create.arg("--env").arg(format!("{k}={v}"));
    }
    create.args([IMAGE, "sh", "-ec", script]);
    let container = Container(output(&mut create)?.trim().to_string());
    // `dir/.` copies what's in `dir`.
    let copy = |from: &Path, to: &str| {
        output(
            docker()
                .arg("cp")
                .arg(from.join("."))
                .arg(format!("{}:{to}", container.0)),
        )
    };
    copy(tree, IN_TREE)?;
    if let Some(f) = fonts {
        copy(f, IN_FONTS)?;
    }
    let status = docker()
        .args(["start", "--attach", &container.0])
        .status()
        .map_err(|e| format!("docker start: {e}"))?;
    Ok(status.code().unwrap_or(-1))
}

/// A container, removed on drop if `--rm` hasn't removed it already.
struct Container(String);

impl Drop for Container {
    fn drop(&mut self) {
        let _ = output(docker().args(["rm", "--force", &self.0]));
    }
}

/// `cargo xtask linux-host`: the checks `ci --linux` runs in its container,
/// on a Linux host.
///
/// - tests (f): die-core's, die-vim's and evil-oracle's tests, M0's exit
///   test (f). die-vim's include the oracle.
/// - oracle: the oracle's summary line, from its own run.
/// - other tests: the rest of the workspace's, xtask's among them.
/// - clean-build: the Linux clean build (stand-ins on a Linux host, the
///   allowlisted `pkg-config` calls, the ELF check).
/// - log plant: wayland-backend's `log` feature fails that build, in a copy
///   of the tree.
///
/// Not here: budget and clippy need rustfmt and clippy, which the image
/// doesn't have, and give the same answer as on your Mac, where ci runs clippy
/// for aarch64-unknown-linux-gnu. The same goes for lint, which reads sources
/// and asks `cargo tree` about every target.
pub fn host_checks(root: &Path) -> Result {
    let host = clean::host()?;
    if !host.contains("-linux-") {
        return Err(format!(
            "linux-host: runs on Linux, not {host}; `cargo xtask ci --linux` runs it in a container"
        ));
    }
    let version = output(Command::new(rustc()).arg("-vV"))?;
    let release = version
        .lines()
        .find_map(|l| l.strip_prefix("release: "))
        .unwrap_or("?");
    let channel = channel(root)?;
    if release != channel {
        return Err(format!(
            "linux-host: rustc is {release}, but rust-toolchain.toml pins {channel}"
        ));
    }
    let kernel = output(Command::new("uname").arg("-r"))?;
    let libc = output(Command::new("getconf").arg("GNU_LIBC_VERSION")).unwrap_or_default();
    println!(
        "linux-host: {host}, kernel {} (uname -r), {}, {}",
        kernel.trim(),
        libc.trim(),
        version.lines().next().unwrap_or("rustc ?")
    );

    let cargo_test = |args: &[&str]| {
        let mut c = Command::new(cargo());
        c.current_dir(root)
            .args(["test", "--locked", "--offline", "--quiet"])
            .args(args)
            .env("DIE_SLOW_HOST", "1");
        c
    };
    let exit_f = ["die-core", "die-vim", "evil-oracle", "die-test-cap"];
    let steps: Vec<Step> = vec![
        (
            "tests (f)",
            Box::new(|| {
                println!("linux-host: the tests of {}", exit_f.join(", "));
                let packages = exit_f.iter().flat_map(|p| ["--package", p]);
                crate::util::run(&mut cargo_test(&packages.collect::<Vec<_>>()))
            }),
        ),
        (
            "oracle",
            Box::new(|| {
                oracle(&mut cargo_test(&[
                    "--package",
                    "die-vim",
                    "--test",
                    "oracle",
                    "--",
                    "--exact",
                    "oracle",
                    "--nocapture",
                ]))
            }),
        ),
        (
            "other tests",
            Box::new(|| {
                let excluded = exit_f.iter().flat_map(|p| ["--exclude", p]);
                let args: Vec<&str> = ["--workspace"].into_iter().chain(excluded).collect();
                crate::util::run(&mut cargo_test(&args))
            }),
        ),
        ("clean-build", Box::new(|| clean::run(root, false))),
        ("log plant", Box::new(|| guards::linux_host(root))),
    ];
    run_steps("linux-host", &steps)?;
    println!(
        "\nlinux-host: die-core, die-vim, the oracle and the memory limit pass on {host} (exit test (f), with ci's cross-check)"
    );
    Ok(())
}

/// Run the oracle's test and print its summary line.
fn oracle(cmd: &mut Command) -> Result {
    let out = cmd
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("{}: {e}", show(cmd)))?;
    let err = String::from_utf8_lossy(&out.stderr);
    if !out.status.success() {
        return Err(format!(
            "{} failed ({}):\n{}{err}",
            show(cmd),
            out.status,
            String::from_utf8_lossy(&out.stdout)
        ));
    }
    let line = err
        .lines()
        .find(|l| l.starts_with("oracle: "))
        .ok_or_else(|| format!("{} passed without its summary line:\n{err}", show(cmd)))?;
    println!("{line}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::write;

    const LOCK: &str = "\
version = 4

[[package]]
name = \"die-core\"
version = \"0.0.0\"

[[package]]
name = \"ropey\"
version = \"1.6.1\"
source = \"registry+https://github.com/rust-lang/crates.io-index\"
checksum = \"0\"
";

    #[test]
    fn sources_other_than_crates_io() {
        assert!(other_sources(LOCK).is_empty());
        let more = format!(
            "{LOCK}\n[[package]]\nname = \"libghostty-vt\"\nversion = \"0.1.0\"\n\
             source = \"git+https://github.com/o/libghostty-rs?rev=ab12#ab12\"\n\n\
             [[package]]\nname = \"left-pad\"\nversion = \"1.0.0\"\n\
             source = \"registry+https://example.com/index\"\n"
        );
        assert_eq!(
            other_sources(&more),
            [
                "libghostty-vt 0.1.0 (git+https://github.com/o/libghostty-rs?rev=ab12#ab12)",
                "left-pad 1.0.0 (registry+https://example.com/index)",
            ]
        );
    }

    #[test]
    fn a_tree_that_fetches_from_crates_io_only() {
        let tmp = TempDir::new("crates-io").unwrap();
        let tree = tmp.path();
        write(&tree.join("Cargo.lock"), LOCK).unwrap();
        let config = tree.join(".cargo/config.toml");
        write(&config, "[alias]\nxtask = \"run --package xtask --\"\n").unwrap();
        crates_io_only(tree).unwrap();
        write(
            &config,
            "[source.crates-io]\nreplace-with = \"mirror\"\n\n\
             [source.mirror]\nregistry = \"sparse+https://example.com/\"\n",
        )
        .unwrap();
        let err = crates_io_only(tree).unwrap_err();
        assert!(err.contains(".cargo/config.toml has a [source]"), "{err}");
        std::fs::remove_file(&config).unwrap();
        let git = LOCK.replace(CRATES_IO, "git+https://github.com/cessen/ropey#0");
        write(&tree.join("Cargo.lock"), &git).unwrap();
        let err = crates_io_only(tree).unwrap_err();
        assert!(err.contains("ropey 1.6.1 (git+"), "{err}");
        assert!(err.contains("a download to ask about"), "{err}");
    }

    #[test]
    fn the_checks_purge_what_clean_build_forbids() {
        let s = checks_script();
        assert!(s.contains(&clean::libc_dev_ere()), "{s}");
        assert!(s.contains("exit 75"), "{s}");
        assert!(!s.contains("/dev/null"), "{s}");
        assert!(s.ends_with("-- linux-host"), "{s}");
    }
}
