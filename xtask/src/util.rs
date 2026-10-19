//! Paths, commands, temporary directories and git.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};

pub type Result<T = ()> = std::result::Result<T, String>;

/// The workspace root.
pub fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask sits in the workspace")
        .to_path_buf()
}

/// The cargo running us, so nested runs use the pinned toolchain without rustup.
pub fn cargo() -> PathBuf {
    std::env::var_os("CARGO")
        .map(PathBuf::from)
        .unwrap_or_else(|| "cargo".into())
}

/// rustc from the same toolchain as `cargo()`.
pub fn rustc() -> PathBuf {
    let c = cargo();
    match c.parent() {
        Some(dir) if dir.join("rustc").exists() => dir.join("rustc"),
        _ => "rustc".into(),
    }
}

/// A rustc that builds for `target`: the pinned toolchain's when its sysroot has
/// the target's std, else one from another installed toolchain that is the same
/// build (an identical `rustc -vV`) and has it. The pinned toolchain lists no
/// Linux targets until you approve their std downloads
/// (docs/records/platforms.md §6, item 3); till then, this borrows `stable`'s
/// std, and only while `stable` is the pinned build.
pub fn rustc_for(target: &str) -> Result<PathBuf> {
    rustc_beside(&rustc(), target)
}

/// `rustc_for`, with `pinned` as the pinned toolchain's rustc.
pub fn rustc_beside(pinned: &Path, target: &str) -> Result<PathBuf> {
    let version = output(Command::new(pinned).arg("-vV"))?;
    let sysroot = PathBuf::from(output(Command::new(pinned).args(["--print", "sysroot"]))?.trim());
    let has_std = |sysroot: &Path| {
        sysroot
            .join("lib/rustlib")
            .join(target)
            .join("lib")
            .is_dir()
    };
    if has_std(&sysroot) {
        return Ok(pinned.to_path_buf());
    }
    // rustup keeps its toolchains side by side.
    let mut others: Vec<PathBuf> = sysroot
        .parent()
        .and_then(|d| std::fs::read_dir(d).ok())
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| *p != sysroot && has_std(p))
        .collect();
    others.sort();
    let first_line = |v: &str| v.lines().next().unwrap_or("?").to_string();
    let mut other_builds = Vec::new();
    for dir in others {
        let r = dir.join("bin/rustc");
        match output(Command::new(&r).arg("-vV")) {
            Ok(v) if v == version => return Ok(r),
            Ok(v) => other_builds.push(format!(
                "{} has it, but is {}, not the pinned build, so it can't stand in",
                dir.file_name().unwrap_or_default().to_string_lossy(),
                first_line(&v)
            )),
            Err(_) => {}
        }
    }
    let others = if other_builds.is_empty() {
        String::new()
    } else {
        format!(" ({})", other_builds.join("; "))
    };
    Err(format!(
        "no installed toolchain that is the pinned {} has the {target} std{others}. \
         `rustup target add {target}` adds it to the pinned toolchain; that's a download, so ask first",
        first_line(&version)
    ))
}

/// Run `cmd`, inheriting output. Errors name the command.
pub fn run(cmd: &mut Command) -> Result {
    let status = cmd.status().map_err(|e| format!("{}: {e}", show(cmd)))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{} failed ({status})", show(cmd)))
    }
}

/// Run `cmd` and return its stdout. Errors carry its stderr.
pub fn output(cmd: &mut Command) -> Result<String> {
    let out = cmd
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("{}: {e}", show(cmd)))?;
    if !out.status.success() {
        return Err(format!(
            "{} failed ({}):\n{}{}",
            show(cmd),
            out.status,
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    String::from_utf8(out.stdout).map_err(|e| format!("{}: {e}", show(cmd)))
}

pub fn show(cmd: &Command) -> String {
    let mut s = cmd.get_program().to_string_lossy().into_owned();
    for a in cmd.get_args() {
        s.push(' ');
        s.push_str(&a.to_string_lossy());
    }
    s
}

pub fn git(root: &Path) -> Command {
    let mut c = Command::new("git");
    c.arg("-C").arg(root);
    c
}

/// A directory under the system's temporary directory, removed on drop
/// unless `keep` is called.
pub struct TempDir {
    path: PathBuf,
    keep: bool,
}

impl TempDir {
    pub fn new(label: &str) -> Result<TempDir> {
        static N: AtomicU32 = AtomicU32::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("die-xtask-{label}-{}-{n}", std::process::id()));
        if path.exists() {
            std::fs::remove_dir_all(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        }
        std::fs::create_dir_all(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(TempDir { path, keep: false })
    }

    /// A directory at a fixed path, emptied first. Builds that cache in a target
    /// directory between runs need their sources at a fixed path: cargo hashes a
    /// path crate by its place in the workspace, so a moved copy would reuse
    /// binaries with the old absolute paths compiled in.
    pub fn at(path: PathBuf) -> Result<TempDir> {
        if path.exists() {
            std::fs::remove_dir_all(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        }
        std::fs::create_dir_all(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(TempDir { path, keep: false })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn keep(&mut self) {
        self.keep = true;
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        if !self.keep {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }
}

/// The files git would commit from `root`'s working tree: tracked files plus
/// untracked ones that aren't ignored. Deleted files are left out.
pub fn worktree_files(root: &Path) -> Result<Vec<PathBuf>> {
    let out = output(git(root).args([
        "ls-files",
        "-z",
        "--cached",
        "--others",
        "--exclude-standard",
    ]))?;
    Ok(out
        .split('\0')
        .filter(|p| !p.is_empty())
        .map(PathBuf::from)
        .filter(|p| root.join(p).exists())
        .collect())
}

/// Copy `root`'s working tree (as `worktree_files` lists it) into `dest`.
///
/// Every copy gets the current time as its modification time. macOS copies
/// keep the original's, and cargo would then trust a build cached from an
/// earlier copy whose file was newer, such as one of `mutate`'s mutants.
pub fn copy_worktree(root: &Path, dest: &Path) -> Result {
    let now = std::time::SystemTime::now();
    for rel in worktree_files(root)? {
        let to = dest.join(&rel);
        if let Some(dir) = to.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        }
        let err = |e: std::io::Error| format!("{}: {e}", rel.display());
        std::fs::copy(root.join(&rel), &to).map_err(err)?;
        std::fs::File::options()
            .write(true)
            .open(&to)
            .and_then(|f| f.set_modified(now))
            .map_err(err)?;
    }
    Ok(())
}

pub fn read(path: &Path) -> Result<String> {
    std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))
}

pub fn write(path: &Path, text: &str) -> Result {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    std::fs::write(path, text).map_err(|e| format!("{}: {e}", path.display()))
}

/// Every `.rs` file under `dir`, sorted.
pub fn rust_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else {
            continue;
        };
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x == "rs") {
                out.push(p);
            }
        }
    }
    out.sort();
    out
}

/// SHA-256 of a file, in lowercase hex.
pub fn sha256(path: &Path) -> Result<String> {
    use sha2::{Digest, Sha256};
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A toolchain directory whose rustc prints `version` and its sysroot,
    /// with `target`'s std or without.
    fn toolchain(dir: &Path, version: &str, std_for: Option<&str>) -> PathBuf {
        let rustc = dir.join("bin/rustc");
        write(
            &rustc,
            &format!(
                "#!/bin/sh\nif [ \"$1\" = -vV ]; then printf '{version}\\nrelease: x\\n'; else echo '{}'; fi\n",
                dir.display()
            ),
        )
        .unwrap();
        let mut perm = std::fs::metadata(&rustc).unwrap().permissions();
        std::os::unix::fs::PermissionsExt::set_mode(&mut perm, 0o755);
        std::fs::set_permissions(&rustc, perm).unwrap();
        if let Some(t) = std_for {
            std::fs::create_dir_all(dir.join("lib/rustlib").join(t).join("lib")).unwrap();
        }
        rustc
    }

    #[test]
    fn a_borrowed_std_must_be_the_pinned_build() {
        let musl = "x86_64-unknown-linux-musl";
        let tmp = TempDir::new("rustc-for").unwrap();
        let t = tmp.path();
        let pinned = toolchain(&t.join("1.96.0"), "rustc 1.96.0 (aaa 2026-05-25)", None);
        // stable moved on: it has the std, but isn't the pinned build.
        toolchain(
            &t.join("stable"),
            "rustc 1.97.0 (bbb 2026-07-09)",
            Some(musl),
        );
        let err = rustc_beside(&pinned, musl).unwrap_err();
        assert!(
            err.contains("pinned rustc 1.96.0 (aaa 2026-05-25)")
                && err.contains(
                    "stable has it, but is rustc 1.97.0 (bbb 2026-07-09), not the pinned build"
                ),
            "{err}"
        );
        // Another toolchain that is the same build stands in.
        let same = toolchain(&t.join("same"), "rustc 1.96.0 (aaa 2026-05-25)", Some(musl));
        assert_eq!(rustc_beside(&pinned, musl).unwrap(), same);
        // The pinned toolchain's own std comes first.
        toolchain(
            &t.join("1.96.0"),
            "rustc 1.96.0 (aaa 2026-05-25)",
            Some(musl),
        );
        assert_eq!(rustc_beside(&pinned, musl).unwrap(), pinned);
    }
}
