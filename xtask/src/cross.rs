//! `cargo xtask cross-check`: `cargo check` of the whole workspace for Linux, from
//! any host (docs/records/platforms.md §5.6), in clean-build's environment: the C
//! compilers, `ar` and `pkg-config` are stand-ins that fail, so nothing compiles
//! C. Warnings fail it, as they fail clippy.

use crate::clean::{clean_cargo, host, run_clean};
use crate::util::{Result, TempDir, rustc_for};
use std::path::Path;

/// The Linux targets checked. musl's std is the one installed today; the glibc
/// targets DIE ships for replace it once the pinned toolchain lists them
/// (platforms.md §6, item 3).
pub const TARGETS: &[&str] = &["x86_64-unknown-linux-musl"];

/// `target_dir` holds the build cache between runs.
pub fn run(root: &Path, target_dir: &Path) -> Result {
    let host = host()?;
    let tmp = TempDir::new("cross-check")?;
    for target in TARGETS {
        let rustc = rustc_for(target).map_err(|e| format!("cross-check: {e}"))?;
        let mut cmd = clean_cargo(root, tmp.path(), &host, &rustc)?;
        cmd.env("RUSTFLAGS", "-D warnings")
            .args([
                "check",
                "--workspace",
                "--all-targets",
                "--locked",
                "--offline",
                "--quiet",
                "--target",
                target,
                "--target-dir",
            ])
            .arg(target_dir);
        run_clean(&mut cmd, &format!("cross-check: cargo check for {target}"))?;
        println!("cross-check: {target} ok ({})", rustc.display());
    }
    Ok(())
}
