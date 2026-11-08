//! `cargo xtask sources`: what the compiler read, against what `cargo xtask lint`
//! reads (scope §12).
//!
//! The lint's rules hold only for files it reads, and it finds them by walking each
//! crate in the workspace's `members`. rustc can be pointed elsewhere: a macro can
//! build a `path` attribute out of parts no token rule recognises, and a manifest can
//! name a target or a path dependency anywhere. So this asks the compiler. Each
//! local crate's dep-info file lists every file it read, however it got there, and
//! each must be a `.rs` file the lint reads, in that crate's own directory. A local
//! crate must be a listed member. Crates from the registry, and from a git repository at
//! a pinned commit (libghostty-rs, D10), are dependencies, and aren't part of your review.
//!
//! The dep-info files come from this build's own messages, so a stale one in a
//! shared target directory doesn't count. `ci` runs this for the host, and the
//! cross-check does the same for each Linux target, whose `cfg`s a build here doesn't
//! see. There are no exceptions yet: nothing uses `include_str!` or `include_bytes!`,
//! and the first that does names its data file here.

use crate::lint;
use crate::util::{Result, cargo, output, read};
use serde_json::Value;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Prints the compiler's own errors as usual, and its artifact messages on stdout.
pub const MESSAGE_FORMAT: &str = "--message-format=json-render-diagnostics";

/// What a dep-info file from clippy lists beyond rustc's: it reads the manifest in the
/// directory it runs in, the workspace's root, as data.
pub const CLIPPY_READS: &[&str] = &["Cargo.toml"];

/// Check the workspace for the host. `target_dir` holds the build cache.
pub fn run(root: &Path, target_dir: &Path) -> Result {
    let messages = output(
        Command::new(cargo())
            .current_dir(root)
            .env_remove("RUSTFLAGS")
            .args([
                "check",
                "--workspace",
                "--all-targets",
                "--all-features",
                "--locked",
                "--offline",
                "--quiet",
                MESSAGE_FORMAT,
                "--target-dir",
            ])
            .arg(target_dir),
    )?;
    let files = check(root, &messages, &[])?;
    println!("sources: ok ({files} files, all read by the lint)");
    Ok(())
}

/// Check `messages`, the JSON lines of a build of the workspace at `root`, in which a
/// dep-info file may also list `extra`, as it spells them. Returns how many different
/// files the build's local crates read.
pub fn check(root: &Path, messages: &str, extra: &[&str]) -> Result<usize> {
    let root = real(root)?;
    let members = lint::members(&root)?;
    let mut errors = BTreeSet::new();
    let mut files = BTreeSet::new();
    for line in messages.lines() {
        let msg: Value =
            serde_json::from_str(line).map_err(|e| format!("sources: {e} in: {line}"))?;
        if msg["reason"] != "compiler-artifact" {
            continue;
        }
        let package = msg["package_id"].as_str().unwrap_or_default();
        if package.starts_with("registry+") || pinned_git(package) {
            continue;
        }
        let manifest = msg["manifest_path"].as_str().unwrap_or_default();
        let dir = real(Path::new(manifest))?;
        let dir = dir.parent().unwrap_or(&dir);
        let Some(member) = members.iter().find(|m| root.join(m) == dir) else {
            errors.insert(format!(
                "{manifest} is compiled, but isn't in the workspace's `members`, the list \
                 `cargo xtask lint` reads"
            ));
            continue;
        };
        let names = msg["filenames"].as_array().map_or(&[][..], Vec::as_slice);
        let dep_info = names
            .iter()
            .filter_map(Value::as_str)
            .find_map(|f| dep_info_of(Path::new(f)))
            .ok_or_else(|| format!("sources: no dep-info file for {names:?}"))?;
        for listed in listed_files(&read(&dep_info)?) {
            if extra.contains(&listed.as_str()) {
                continue;
            }
            let rel = real(&root.join(&listed)).ok().and_then(|f| {
                let rel = f.strip_prefix(&root).ok()?;
                Some(rel.to_string_lossy().into_owned())
            });
            if !rel.as_deref().is_some_and(|r| lint::reads(member, r)) {
                errors.insert(format!(
                    "{member} compiled {}, which `cargo xtask lint` doesn't read. A crate \
                     compiles the `.rs` files in its own directory and no others",
                    rel.as_deref().unwrap_or(&listed)
                ));
            }
            files.insert(rel.unwrap_or(listed));
        }
    }
    if errors.is_empty() {
        Ok(files.len())
    } else {
        let errors: Vec<String> = errors.into_iter().collect();
        Err(format!("sources:\n  {} (scope §12)", errors.join("\n  ")))
    }
}

/// A package from a git repository at a full commit: `git+URL?rev=SHA#...`, as Cargo.lock
/// pins it. One at a branch or a tag could change under the lockfile's feet.
fn pinned_git(package: &str) -> bool {
    let Some(rest) = package.strip_prefix("git+") else {
        return false;
    };
    let rev = rest.split_once("?rev=").map_or("", |(_, r)| r);
    let rev = rev.split(['#', '&']).next().unwrap_or("");
    rev.len() == 40 && rev.bytes().all(|b| b.is_ascii_hexdigit())
}

fn real(path: &Path) -> Result<PathBuf> {
    path.canonicalize()
        .map_err(|e| format!("{}: {e}", path.display()))
}

/// The dep-info file rustc wrote for an artifact: `foo-hash.d` beside
/// `libfoo-hash.rmeta`. A build script, `build-script-build`, has a directory of its
/// own, with its dep-info under a name with a hash.
fn dep_info_of(artifact: &Path) -> Option<PathBuf> {
    let name = artifact.file_name()?.to_str()?;
    if name.starts_with("build-script-") {
        let files = std::fs::read_dir(artifact.parent()?).ok()?;
        return files
            .flatten()
            .map(|f| f.path())
            .find(|f| f.extension().is_some_and(|x| x == "d"));
    }
    let stem = artifact.file_stem()?.to_str()?;
    let stem = stem.strip_prefix("lib").unwrap_or(stem);
    Some(artifact.with_file_name(format!("{stem}.d"))).filter(|d| d.exists())
}

/// The files a dep-info file lists. rustc ends it with an empty rule for each one,
/// `file:`, with spaces escaped, and comments for the environment variables it read.
fn listed_files(dep_info: &str) -> Vec<String> {
    dep_info
        .lines()
        .filter(|l| !l.starts_with('#'))
        .filter_map(|l| l.strip_suffix(':'))
        .map(|f| f.replace("\\ ", " "))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::{TempDir, write};

    /// A workspace with members `a` and `b`, and an empty `.rs` file at each of `files`.
    fn workspace(files: &[&str]) -> TempDir {
        let dir = TempDir::new("sources").unwrap();
        let p = dir.path();
        write(
            &p.join("Cargo.toml"),
            "[workspace]\nmembers = [\"crates/a\", \"crates/b\"]\n",
        )
        .unwrap();
        for m in ["crates/a", "crates/b", "docs/stray"] {
            write(&p.join(m).join("Cargo.toml"), "").unwrap();
        }
        for f in files {
            write(&p.join(f), "").unwrap();
        }
        dir
    }

    /// One crate's `compiler-artifact` message, and a dep-info file listing `files`.
    fn artifact(dir: &TempDir, package: &str, manifest_dir: &str, files: &[&str]) -> String {
        let out = dir.path().join("out");
        let body: String = files
            .iter()
            .map(|f| format!("{}:\n", f.replace(' ', "\\ ")))
            .collect();
        let d = format!("x.rmeta: {}\n\n{body}\n# env-dep:X=y\n", files.join(" "));
        write(&out.join("x-1.d"), &d).unwrap();
        serde_json::json!({
            "reason": "compiler-artifact",
            "package_id": package,
            "manifest_path": dir.path().join(manifest_dir).join("Cargo.toml"),
            "filenames": [out.join("libx-1.rmeta")],
        })
        .to_string()
    }

    const LOCAL: &str = "path+file:///w/crates/a#0.0.0";

    /// The result of a build of crate `a` that read `listed`.
    fn compiled(files: &[&str], listed: &[&str]) -> Result<usize> {
        let dir = workspace(files);
        let msg = artifact(&dir, LOCAL, "crates/a", listed);
        check(dir.path(), &msg, &[])
    }

    #[test]
    fn a_crate_compiles_its_own_rust_files() {
        let own = [
            "crates/a/src/lib.rs",
            "crates/a/src/target/mod.rs",
            "crates/a/tests/t.rs",
            "crates/a/src/a b.rs",
        ];
        assert_eq!(compiled(&own, &own), Ok(4));
        assert_eq!(
            compiled(&["crates/a/src/lib.rs"], &["crates/a/src/../src/lib.rs"]),
            Ok(1)
        );
        assert_eq!(
            compiled(&["crates/a/src/lib.rs"], &["crates/a/src/x/../lib.rs"]),
            Err("sources:\n  crates/a compiled crates/a/src/x/../lib.rs, which `cargo xtask lint` doesn't read. A crate compiles the `.rs` files in its own directory and no others (scope §12)".into()),
            "a file that isn't there"
        );
    }

    #[test]
    fn any_other_file_fails() {
        let doesnt_read = |file: &str, shown: &str| {
            let e = compiled(&[file], &[&format!("crates/a/../../{file}")]).unwrap_err();
            assert!(
                e.contains(&format!(
                    "crates/a compiled {shown}, which `cargo xtask lint` doesn't read"
                )),
                "{e}"
            );
        };
        // Outside every crate, in another crate, and not Rust.
        doesnt_read("docs/planted.rs", "docs/planted.rs");
        doesnt_read("crates/b/src/lib.rs", "crates/b/src/lib.rs");
        doesnt_read("crates/a/src/x.txt", "crates/a/src/x.txt");
        // A crate's own `target`, which the lint skips.
        doesnt_read("crates/a/target/x.rs", "crates/a/target/x.rs");
        // Outside the workspace altogether.
        let outside = TempDir::new("sources-outside").unwrap();
        let file = outside.path().join("x.rs");
        write(&file, "").unwrap();
        let e = compiled(&[], &[file.to_str().unwrap()]).unwrap_err();
        assert!(
            e.contains("x.rs, which `cargo xtask lint` doesn't read"),
            "{e}"
        );
    }

    #[test]
    fn a_local_crate_outside_the_members_fails() {
        let dir = workspace(&["docs/stray/src/lib.rs"]);
        let msg = artifact(
            &dir,
            "path+file:///w/docs/stray#0.0.0",
            "docs/stray",
            &["docs/stray/src/lib.rs"],
        );
        let e = check(dir.path(), &msg, &[]).unwrap_err();
        assert!(e.contains("isn't in the workspace's `members`"), "{e}");
    }

    #[test]
    fn only_this_builds_artifacts_count() {
        let dir = workspace(&["crates/a/src/lib.rs", "docs/planted.rs"]);
        let good = artifact(&dir, LOCAL, "crates/a", &["crates/a/src/lib.rs"]);
        // A dep-info file no message names: a stale one in a shared target directory.
        write(&dir.path().join("out/y-2.d"), "docs/planted.rs:\n").unwrap();
        // A crate from the registry, and messages about other things.
        let registry = serde_json::json!({
            "reason": "compiler-artifact",
            "package_id": "registry+https://github.com/rust-lang/crates.io-index#x@1.0.0",
        });
        let other = "{\"reason\":\"build-finished\",\"success\":true}";
        assert_eq!(
            check(dir.path(), &format!("{registry}\n{good}\n{other}\n"), &[]),
            Ok(1)
        );
    }

    #[test]
    fn a_pinned_git_dependency_is_a_dependency() {
        let rev = "d2036baeb605d31d09a9e2ba6d0c923e04c34d54";
        let git = |id: &str| {
            let dir = workspace(&["crates/a/src/lib.rs"]);
            let msg = serde_json::json!({
                "reason": "compiler-artifact",
                "package_id": id,
                "manifest_path": "/home/x/.cargo/git/checkouts/y/z/Cargo.toml",
            });
            check(dir.path(), &msg.to_string(), &[])
        };
        let pinned =
            format!("git+https://github.com/uzaaft/libghostty-rs?rev={rev}#libghostty-vt@0.2.1");
        assert_eq!(git(&pinned), Ok(0));
        for unpinned in [
            "git+https://github.com/uzaaft/libghostty-rs?branch=main#libghostty-vt@0.2.1",
            "git+https://github.com/uzaaft/libghostty-rs#libghostty-vt@0.2.1",
            "git+https://github.com/uzaaft/libghostty-rs?rev=d2036bae#libghostty-vt@0.2.1",
        ] {
            assert!(git(unpinned).is_err(), "{unpinned}");
        }
    }

    #[test]
    fn clippy_reads_the_workspaces_manifest() {
        let dir = workspace(&["crates/a/src/lib.rs"]);
        let listed = ["crates/a/src/lib.rs", "Cargo.toml"];
        let msg = artifact(&dir, LOCAL, "crates/a", &listed);
        assert_eq!(check(dir.path(), &msg, CLIPPY_READS), Ok(1));
        let e = check(dir.path(), &msg, &[]).unwrap_err();
        assert!(e.contains("compiled Cargo.toml, which"), "{e}");
        // Only as clippy spells it: a `path` attribute's route to it still fails.
        let msg = artifact(&dir, LOCAL, "crates/a", &["crates/a/../../Cargo.toml"]);
        let e = check(dir.path(), &msg, CLIPPY_READS).unwrap_err();
        assert!(e.contains("compiled Cargo.toml, which"), "{e}");
    }

    #[test]
    fn dep_info_of_an_artifact() {
        let dir = TempDir::new("sources-dep-info").unwrap();
        let p = dir.path();
        for (artifact, d) in [
            ("deps/libdie_core-9a.rmeta", "deps/die_core-9a.d"),
            ("deps/liblibrary-9a.rmeta", "deps/library-9a.d"),
            ("deps/libm-9a.dylib", "deps/m-9a.d"),
            (
                "build/x-9a/build-script-build",
                "build/x-9a/build_script_build-9a.d",
            ),
        ] {
            assert_eq!(dep_info_of(&p.join(artifact)), None, "not written yet");
            write(&p.join(d), "").unwrap();
            assert_eq!(dep_info_of(&p.join(artifact)), Some(p.join(d)));
        }
        assert_eq!(dep_info_of(&p.join("deps/libother-9a.rmeta")), None);
    }

    #[test]
    fn files_in_a_dep_info_file() {
        let d = "a.rmeta: src/lib.rs src/a\\ b.rs\n\na.d: src/lib.rs src/a\\ b.rs\n\n\
                 src/lib.rs:\nsrc/a\\ b.rs:\n\n# env-dep:CARGO_MANIFEST_DIR=/w:\n";
        assert_eq!(listed_files(d), ["src/lib.rs", "src/a b.rs"]);
    }
}
