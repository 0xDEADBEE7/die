//! `cargo xtask cross-check`: `cargo check` of the whole workspace for Linux, from
//! any host (docs/records/platforms.md §5.6), in clean-build's environment: the C
//! compilers, `ar` and `pkg-config` are stand-ins that fail, so nothing compiles
//! C, and a stand-in call the target's allowlist doesn't name fails the check
//! even when the build script carries on. Warnings fail it, as they fail clippy.
//! `cargo xtask guards` plants a failure of each kind.

use crate::clean::{Allowed, LINUX, STAND_INS, check_calls, clean_cargo, host, run_clean};
use crate::util::{Result, TempDir, rustc_for, write};
use std::path::Path;

/// The Linux targets checked, with their allowlists. musl's std is the one
/// installed today; the glibc targets DIE ships for replace it once the pinned
/// toolchain lists them (platforms.md §6, item 3).
pub const TARGETS: &[(&str, &[Allowed])] = &[("x86_64-unknown-linux-musl", LINUX)];

/// `target_dir` holds the build cache between runs.
pub fn run(root: &Path, target_dir: &Path) -> Result {
    let host = host()?;
    let tmp = TempDir::new("cross-check")?;
    for (target, allowed) in TARGETS {
        let rustc = rustc_for(target).map_err(|e| format!("cross-check: {e}"))?;
        // Cargo doesn't run a build script again once it has passed, so the
        // stand-ins see its calls only on its first run. A cache built under
        // another allowlist or other stand-ins goes, as does one whose calls
        // fail the check below.
        let stamp = target_dir.join(format!("stand-ins-{target}"));
        let checked_against = format!(
            "{}\n{}",
            STAND_INS.join(" "),
            allowed
                .iter()
                .map(|a| format!("{} via {}, calls {:?}\n", a.builder, a.via, a.calls))
                .collect::<String>()
        );
        if std::fs::read_to_string(&stamp).ok().as_ref() != Some(&checked_against) {
            let _ = std::fs::remove_dir_all(target_dir);
        }
        write(&stamp, &checked_against)?;
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
        run_clean(&mut cmd, &format!("cross-check: cargo check for {target}"))
            .map_err(|e| with_hint(&e))?;
        let calls = check_calls(tmp.path(), allowed).map_err(|e| {
            let _ = std::fs::remove_dir_all(target_dir);
            format!("cross-check: cargo check for {target}:\n  {e}")
        })?;
        println!("cross-check: {target} ok ({}{calls})", rustc.display());
    }
    Ok(())
}

/// Until `platform::linux` lands, neutral code that only `platform::macos`
/// calls is dead on Linux.
fn with_hint(err: &str) -> String {
    if err.contains("never used") || err.contains("never constructed") {
        format!(
            "{err}\nIf it's neutral code that only `platform::macos` uses, it's dead on Linux \
             until `platform::linux` lands: mark it \
             `#[cfg_attr(not(target_os = \"macos\"), allow(dead_code))]`, the one platform cfg \
             `cargo xtask lint` lets neutral code carry, since it only sets a lint level"
        )
    } else {
        err.to_string()
    }
}
