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

/// A program from the same toolchain as `cargo()`, such as `rustc` or
/// `cargo-clippy`, found beside it so nested runs don't go through rustup.
pub fn tool(name: &str) -> PathBuf {
    let c = cargo();
    match c.parent() {
        Some(dir) if dir.join(name).exists() => dir.join(name),
        _ => name.into(),
    }
}

/// rustc from the same toolchain as `cargo()`.
pub fn rustc() -> PathBuf {
    tool("rustc")
}

/// Fail unless the pinned toolchain has `target`'s std. rust-toolchain.toml
/// lists the Linux targets, so rustup installs their std with the toolchain
/// (docs/records/platforms.md §6, item 3).
pub fn require_std(target: &str) -> Result {
    let sysroot = output(Command::new(rustc()).args(["--print", "sysroot"]))?;
    let dir = Path::new(sysroot.trim())
        .join("lib/rustlib")
        .join(target)
        .join("lib");
    if dir.is_dir() {
        return Ok(());
    }
    Err(format!(
        "the pinned toolchain has no {target} std ({} is missing). rust-toolchain.toml lists \
         the target, so rustup adds it the next time cargo runs in the workspace; \
         `rustup target add {target}` there adds it now",
        dir.display()
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
