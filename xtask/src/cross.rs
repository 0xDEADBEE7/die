//! `cargo xtask cross-check`: clippy, which is `cargo check` plus its lints, of
//! the whole workspace for x86_64 and aarch64 Linux, from any host
//! (docs/records/platforms.md §5.6). Warnings fail it, as they fail ci's clippy.
//! It runs in clean-build's environment: the C compilers, `ar` and
//! `pkg-config` are stand-ins that fail, so nothing compiles C, and a stand-in
//! call the target's allowlist doesn't name fails the check even when the
//! build script carries on. `cargo xtask guards` plants a failure of each kind.
//! Each target's build is also held to `cargo xtask sources`, since `cfg`s pick
//! different files there.
//!
//! The pinned toolchain builds for each target with its own std, which
//! rust-toolchain.toml lists, and its own clippy, run directly rather than
//! through rustup.

use crate::clean::{Allowed, LINUX, STAND_INS, check_calls, clean_command, host, run_clean};
use crate::sources;
use crate::util::{Result, TempDir, cargo, require_std, rustc, tool, write};
use std::path::Path;

/// The Linux targets checked, with their allowlists: the glibc targets DIE
/// ships for (scope D12). musl isn't checked: DIE doesn't support it, and
/// nothing in the workspace branches on `target_env`.
pub const TARGETS: &[(&str, &[Allowed])] = &[
    ("x86_64-unknown-linux-gnu", LINUX),
    ("aarch64-unknown-linux-gnu", LINUX),
];

/// `target_dir` holds the build cache between runs.
pub fn run(root: &Path, target_dir: &Path) -> Result {
    let host = host()?;
    let (rustc, clippy) = (rustc(), tool("cargo-clippy"));
    for (target, _) in TARGETS {
        require_std(target).map_err(|e| format!("cross-check: {e}"))?;
    }
    // Cargo doesn't run a build script again once it has passed, so the
    // stand-ins see its calls only on its first run. A cache built under other
    // stand-ins or allowlists goes, as does one with a call that fails the
    // check below.
    let stamp = target_dir.join("stand-ins");
    let checked_against = format!(
        "{}\n{}",
        STAND_INS.join(" "),
        TARGETS
            .iter()
            .flat_map(|(t, allowed)| allowed.iter().map(move |a| (t, a)))
            .map(|(t, a)| format!("{t}: {} via {}, calls {:?}\n", a.builder, a.via, a.calls))
            .collect::<String>()
    );
    if std::fs::read_to_string(&stamp).ok().as_ref() != Some(&checked_against) {
        let _ = std::fs::remove_dir_all(target_dir);
    }
    write(&stamp, &checked_against)?;
    let tmp = TempDir::new("cross-check")?;
    for (target, allowed) in TARGETS {
        // cargo-clippy finds cargo through CARGO, and clippy-driver beside itself.
        let mut cmd = clean_command(&clippy, root, tmp.path(), &host, &rustc)?;
        cmd.env("CARGO", cargo())
            .args([
                "clippy",
                "--workspace",
                "--all-targets",
                "--locked",
                "--offline",
                "--quiet",
                sources::MESSAGE_FORMAT,
                "--target",
                target,
                "--target-dir",
            ])
            .arg(target_dir)
            .args(["--", "-D", "warnings"]);
        let what = format!("cross-check: clippy for {target}");
        let built = run_clean(&mut cmd, &what).map_err(|e| with_hint(&e));
        // Checked whether or not the build passed: a build script that called
        // a stand-in and carried on is cached either way.
        let calls = check_calls(tmp.path(), allowed).map_err(|e| {
            let _ = std::fs::remove_dir_all(target_dir);
            format!("{what}:\n  {e}")
        });
        let (messages, calls) = match (built, calls) {
            (Ok(messages), Ok(calls)) => (messages, calls),
            (Err(b), Err(c)) => return Err(format!("{c}\n{b}")),
            (Err(e), _) | (_, Err(e)) => return Err(e),
        };
        sources::check(root, &messages, sources::CLIPPY_READS)
            .map_err(|e| format!("{what}:\n{e}"))?;
        println!("cross-check: {target} ok (clippy -D warnings{calls})");
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
