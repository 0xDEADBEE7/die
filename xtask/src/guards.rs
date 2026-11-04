//! `cargo xtask guards`: proves F1's build guards in a throwaway copy of the
//! working tree. `unsafe` outside die-render's `platform` module fails to build,
//! and so does `-Cpanic=abort`, with the guard's message. The `unsafe` rustc lets
//! through builds, and fails `cargo xtask lint`. A file or crate the lint wouldn't
//! read fails `cargo xtask sources`. For Linux, the cross-check fails on dead code
//! (its `-D warnings`), on a build script that compiles C (its stand-ins) and on a
//! Linux-only file the lint wouldn't read.

use crate::util::{Result, TempDir, cargo, copy_worktree, output, read, write};
use crate::{clean, lint, sources};
use std::path::Path;
use std::process::{Command, Stdio};

const PLANT: &str = "\n#[allow(dead_code)]\nfn planted() {\n    unsafe {}\n}\n";

/// Unsafe code in a file that nothing but a `path` attribute or `include!` reaches.
const OUTSIDE: (&str, &str) = (
    "docs/planted.rs",
    "#[allow(unsafe_code)]\npub mod outside {\n    pub fn ok() -> &'static str {\n        // SAFETY: these bytes are UTF-8.\n        unsafe { std::str::from_utf8_unchecked(b\"ok\") }\n    }\n}\n",
);

/// A crate outside the workspace's `members`, with a bare `unsafe` block, since nothing
/// sets its lints.
const OUTSIDE_CRATE: &[(&str, &str)] = &[
    (
        "docs/plantedcrate/Cargo.toml",
        "[package]\nname = \"plantedcrate\"\nversion = \"0.0.0\"\nedition = \"2024\"\n",
    ),
    (
        "docs/plantedcrate/src/lib.rs",
        "pub fn ok() -> &'static str {\n    unsafe { std::str::from_utf8_unchecked(b\"ok\") }\n}\n",
    ),
];

/// A module named `target`, which the lint must read. `.gitignore`'s `target/` keeps it
/// out of this copy, so it's written in directly; `git add -f` gets it into a clone.
const TARGET_MODULE: (&str, &str) = (
    "crates/die-render/src/target/mod.rs",
    "#![allow(unsafe_code)]\npub fn ok() -> &'static str {\n    unsafe { std::str::from_utf8_unchecked(b\"ok\") }\n}\n",
);

/// The bypasses of the `unsafe` confinement that rustc accepts: what each is, the
/// crate it goes in, the code, and the lint's error for it.
const LINT_PLANTS: &[(&str, &str, &str, &str)] = &[
    (
        "a naked function, which even `forbid` doesn't see,",
        "die-core",
        "\n#[unsafe(naked)]\npub extern \"C\" fn planted() {\n    core::arch::naked_asm!(\"ret\")\n}\n",
        "`unsafe` outside",
    ),
    (
        "a lint level a macro builds",
        "die-render",
        "\nmacro_rules! level {\n    ($lint:ident, $($i:item)*) => { #[allow($lint)] pub mod planted { $($i)* } };\n}\nlevel!(unsafe_code, pub fn ok() -> &'static str { unsafe { std::str::from_utf8_unchecked(b\"ok\") } });\n",
        "`unsafe` outside",
    ),
    (
        "that lint level around `global_asm!`, which has no `unsafe`,",
        "die-render",
        "\nmacro_rules! level {\n    ($lint:ident, $($i:item)*) => { #[allow($lint)] pub mod planted { $($i)* } };\n}\nlevel!(unsafe_code, core::arch::global_asm!(\"\"););\n",
        "`global_asm` outside",
    ),
    (
        "a `path` attribute to a file outside every crate",
        "die-render",
        "\n#[path = \"../../../docs/planted.rs\"]\npub mod planted;\n",
        "`path` attribute",
    ),
    (
        "the same in a `cfg_attr`",
        "die-render",
        "\n#[cfg_attr(all(), path = \"../../../docs/planted.rs\")]\npub mod planted;\n",
        "`path` attribute",
    ),
    (
        "the same as a raw identifier",
        "die-render",
        "\n#[r#path = \"../../../docs/planted.rs\"]\npub mod planted;\n",
        "`path` attribute",
    ),
    (
        "the same in a `cfg_attr`, as a raw identifier",
        "die-render",
        "\n#[cfg_attr(all(), r#path = \"../../../docs/planted.rs\")]\npub mod planted;\n",
        "`path` attribute",
    ),
    (
        "`include!` renamed",
        "die-render",
        "\nuse core::include as planted;\nplanted!(\"../../../docs/planted.rs\");\n",
        "`include!`",
    ),
];

/// What `sources` fails with for a file, and for a crate.
const NOT_READ: &str = "which `cargo xtask lint` doesn't read";
const NOT_A_MEMBER: &str = "isn't in the workspace's `members`";

/// Bypasses no token rule can see: what each is, the file it's appended to, the code,
/// and `sources`' error for it. rustc accepts them all, or `sources` fails with
/// cargo's error and without this text.
const SOURCE_PLANTS: &[(&str, &str, &str, &str)] = &[
    (
        "a `path` attribute a macro builds",
        "crates/die-render/src/lib.rs",
        "\nmacro_rules! planted {\n    ($k:ident, $f:literal) => { #[$k = $f] pub mod planted; };\n}\nplanted!(path, \"../../../docs/planted.rs\");\n",
        NOT_READ,
    ),
    (
        "the same with the `#` passed in",
        "crates/die-render/src/lib.rs",
        "\nmacro_rules! planted {\n    ($h:tt $a:meta) => { $h [$a] pub mod planted; };\n}\nplanted!(# path = \"../../../docs/planted.rs\");\n",
        NOT_READ,
    ),
    (
        "the same with the brackets passed in",
        "crates/die-render/src/lib.rs",
        "\nmacro_rules! planted {\n    ($a:tt) => { #$a pub mod planted; };\n}\nmacro_rules! name {\n    ($k:ident, $f:literal) => { planted!([$k = $f]); };\n}\nname!(path, \"../../../docs/planted.rs\");\n",
        NOT_READ,
    ),
    (
        "a test target outside the crate",
        "crates/die-render/Cargo.toml",
        "\n[[test]]\nname = \"planted\"\npath = \"../../docs/planted.rs\"\n",
        NOT_READ,
    ),
    (
        "a path dependency that isn't a listed member",
        "crates/die-render/Cargo.toml",
        "\n[dependencies.plantedcrate]\npath = \"../../docs/plantedcrate\"\n",
        NOT_A_MEMBER,
    ),
];

/// A `path` attribute a macro builds, on Linux only, so only the cross-check's build
/// compiles it. `platform` may name `target_os`.
const LINUX_SOURCE: &str = "\n#[cfg(target_os = \"linux\")]\nmacro_rules! planted {\n    ($k:ident, $f:literal) => { #[$k = $f] pub mod planted; };\n}\n#[cfg(target_os = \"linux\")]\nplanted!(path, \"../../../../docs/planted.rs\");\n";

/// `unsafe` blocks in `platform::macos` with no SAFETY comment, and what turns off
/// the lints that require one. clippy passes each, so only the lint can catch it.
const BLOCK_LINT_PLANTS: &[(&str, &str)] = &[
    (
        "allowing the unsafe-block lints by name",
        "\n#[allow(clippy::undocumented_unsafe_blocks, clippy::multiple_unsafe_ops_per_block)]\npub fn planted(a: &[u8]) -> &str {\n    unsafe { std::str::from_utf8_unchecked(a) }\n}\n",
    ),
    (
        "allowing the group they're in",
        "\n#[allow(clippy::restriction)]\npub fn planted(a: &[u8]) -> &str {\n    unsafe { std::str::from_utf8_unchecked(a) }\n}\n",
    ),
];

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
    sources::run(tree, &target)?;
    println!("guards: the unplanted tree passes `sources`");

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

    write(&tree.join(OUTSIDE.0), OUTSIDE.1)?;
    for (what, krate, plant, error) in LINT_PLANTS {
        let path = tree.join(format!("crates/{krate}/src/lib.rs"));
        with_plant(&path, plant, || {
            expect(tree, &target, &["-p", krate], None, None)?;
            fails_with("the lint", lint::run(tree), &[*error])
        })?;
        println!("guards: {what} builds, and fails the lint");
    }

    let module = tree.join(TARGET_MODULE.0);
    write(&module, TARGET_MODULE.1)?;
    let render = tree.join("crates/die-render/src/lib.rs");
    let res = with_plant(&render, "\npub mod target;\n", || {
        expect(tree, &target, &["-p", "die-render"], None, None)?;
        fails_with("the lint", lint::run(tree), &["`unsafe` outside"])
    });
    std::fs::remove_file(&module).map_err(|e| e.to_string())?;
    res?;
    println!("guards: a module named `target` builds, and fails the lint");

    for (file, text) in OUTSIDE_CRATE {
        write(&tree.join(file), text)?;
    }
    let lock = tree.join("Cargo.lock");
    let orig_lock = read(&lock)?;
    for (what, file, plant, error) in SOURCE_PLANTS {
        let res = with_plant(&tree.join(file), plant, || {
            update_lockfile(tree)?;
            fails_with("sources", sources::run(tree, &target), &[*error])
        });
        write(&lock, &orig_lock)?;
        res?;
        println!("guards: {what} fails `sources`");
    }
    std::fs::remove_dir_all(tree.join("docs/plantedcrate")).map_err(|e| e.to_string())?;

    let text = tree.join("crates/die-render/src/platform/macos/text.rs");
    for (what, plant) in BLOCK_LINT_PLANTS {
        with_plant(&text, plant, || {
            clippy_passes(tree, &target, "die-render")?;
            fails_with("the lint", lint::run(tree), &["unsafe-block lints"])
        })?;
        println!("guards: {what} passes clippy, and fails the lint");
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

    let res = with_plant(&path, LINUX_SOURCE, || {
        crate::cross::run(tree, &work.join("cross"))
    });
    fails_with("the cross-check", res, &[NOT_READ])?;
    std::fs::remove_file(tree.join(OUTSIDE.0)).map_err(|e| e.to_string())?;
    println!("guards: a file only Linux's build compiles fails the cross-check");

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

/// On a Linux host (docs/records/platforms.md §5.5): clean-build's build of
/// die-render for the host fails with the `log` plant, whose build script calls
/// cc, though the host has a real one. The plant goes into a throwaway copy of
/// `root`'s working tree, as `run`'s do, so `root` is never changed, even if
/// it's interrupted.
pub fn linux_host(root: &Path) -> Result {
    let host = clean::host()?;
    if !host.contains("-linux-") {
        return Err(format!(
            "guards: the Linux-host plant runs on Linux, not {host}"
        ));
    }
    let tmp = TempDir::new("log-plant")?;
    let tree = tmp.path().join("tree");
    copy_worktree(root, &tree)?;
    let res = with_log_plant(&tree, || {
        clean::build(
            &tree,
            &tmp.path().join("build"),
            &host,
            &["-p", "die-render"],
        )
        .map(|_| ())
    });
    fails_with("clean-build", res, LOG_PLANT_FAILS)?;
    println!(
        "guards: wayland-backend's `log` feature fails the clean build on {host}: its build script calls cc"
    );
    Ok(())
}

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
    let res = update_lockfile(tree).and_then(|()| check());
    write(&manifest, &orig)?;
    write(&lock, &orig_lock)?;
    res
}

/// Bring the lockfile up to date with a planted manifest, for the checks that build
/// `--locked`. It gains the new edges only; every version stays.
fn update_lockfile(tree: &Path) -> Result {
    output(Command::new(cargo()).current_dir(tree).args([
        "metadata",
        "--offline",
        "--format-version",
        "1",
    ]))
    .map(|_| ())
}

/// Run `check` with `plant` appended to `file`, then put the file back.
fn with_plant(file: &Path, plant: &str, check: impl FnOnce() -> Result) -> Result {
    let orig = read(file)?;
    write(file, &format!("{orig}{plant}"))?;
    let res = check();
    write(file, &orig)?;
    res
}

/// clippy with ci's `-D warnings` must pass `krate` in `tree`.
fn clippy_passes(tree: &Path, target: &Path, krate: &str) -> Result {
    output(
        Command::new(cargo())
            .current_dir(tree)
            .env_remove("RUSTFLAGS")
            .args([
                "clippy",
                "--quiet",
                "--offline",
                "-p",
                krate,
                "--target-dir",
            ])
            .arg(target)
            .args(["--", "-D", "warnings"]),
    )
    .map(|_| ())
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
