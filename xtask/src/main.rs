#[cfg(test)]
use die_test_cap as _;

use std::path::Path;
use xtask::util::{Result, root, task_dir};
use xtask::{
    api, app, budget, ci, clean, cross, fonts, guards, lint, linux, mutate, restart_loop, sources,
};

const USAGE: &str = "usage: cargo xtask <task>

  budget               line counts against guard 7 and budget.toml
  lint                 unsafe only in platform modules and the test cap, no target_os in
                       neutral code, no window or GPU crates under die-core and die-vim,
                       exact pins, D15, a memory limit in every test binary, lockfile,
                       licenses
  sources              every file the compiler read is one the lint reads, and every
                       local crate is a listed member
  guards               unsafe outside the allowed places and -Cpanic=abort fail to
                       build; what rustc lets through fails lint or sources; dead code
                       on Linux and a build script that calls cc fail the cross-check
  api-diff [--base R]  die-core's public API and die-vim's grammar, R -> working tree
                       (R defaults to the merge base with main)
  app [--home DIR]     build ~/Applications/DIE.app, or under DIR, with the release build and
                       the fonts; open -g launches it without taking focus
  restart-loop N       N rebuilds and restarts of a release bundle of its own, launched with
                       open -g: every restore's session digest must match and hold what it
                       started with, and each restart must take under 1 s
  fonts                fetch Ioskeley Mono Term into fonts/, checked by SHA-256
  clean-build [--keep] no C builders in each target's build graph but the allowlist's,
                       and a build from an empty environment with no C compiler or
                       pkg-config, which no build script calls unless allowlisted (D16)
  cross-check          clippy -D warnings (so cargo check) of the workspace for
                       x86_64 and aarch64 Linux, with no C compiler or pkg-config
  mutate [--changed] [--cases N]
                       die-core's planted bugs; the property tests must catch each, by
                       a named test, the memory limit or the time limit. --changed runs
                       only those whose file changed since the merge base with main
  ci [--full] [--worktree | --rev R]
                       the checks in a fresh clone of HEAD (or R), or of the working
                       tree as git would commit it. guards and clean-build are skipped
                       unless the diff from the merge base with main touches Cargo.toml,
                       Cargo.lock, .cargo/, rust-toolchain.toml, a build script, xtask/
                       or tools/test-cap/, and the tests run at DIE_PROP_CASES=100 unless
                       it's set, without the #[ignore] ones. --full runs every step and
                       every test, ignored ones too, at the old property counts
  ci --linux [--worktree | --rev R]
                       the tests, the oracle and the Linux clean-build on aarch64
                       Linux, in a container on colima (exit test (f))
  linux-host           the checks ci --linux runs in its container, on a Linux host";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Err(e) = dispatch(&args) {
        eprintln!("{e}");
        std::process::exit(1);
    }
}

fn dispatch(args: &[String]) -> Result {
    let root = root();
    let a: Vec<&str> = args.iter().map(String::as_str).collect();
    match a.as_slice() {
        ["budget"] => budget::run(&root),
        ["lint"] => lint::run(&root),
        ["sources"] => sources::run(&root, &task_dir(&root, "sources")),
        ["guards"] => guards::run(&root, &task_dir(&root, "guards")),
        ["api-diff"] => api::run(&root, None),
        ["api-diff", "--base", b] => api::run(&root, Some(b)),
        ["app"] => app::run(&root, None),
        ["app", "--home", home] => app::run(&root, Some(Path::new(home))),
        ["restart-loop", n] => match n.parse() {
            Ok(count) => restart_loop::run(&root, count),
            Err(_) => Err(USAGE.into()),
        },
        ["fonts"] => fonts::run_fonts(&root),
        ["clean-build"] => clean::run(&root, false),
        ["clean-build", "--keep"] => clean::run(&root, true),
        ["cross-check"] => cross::run(&root, &task_dir(&root, "cross")),
        ["mutate", rest @ ..] => match mutate::Options::parse(rest) {
            Some(options) => mutate::run(&root, options),
            None => Err(USAGE.into()),
        },
        ["ci", rest @ ..] => {
            let linux = rest.contains(&"--linux");
            let full = rest.contains(&"--full");
            let rest: Vec<&str> = rest
                .iter()
                .copied()
                .filter(|a| !matches!(*a, "--linux" | "--full"))
                .collect();
            let source = match rest.as_slice() {
                [] => ci::Source::Rev("HEAD".into()),
                ["--rev", r] => ci::Source::Rev(r.to_string()),
                ["--worktree"] => ci::Source::Worktree,
                _ => return Err(USAGE.into()),
            };
            if linux {
                // Its container runs everything, at the tests' own counts.
                linux::run(&root, source)
            } else {
                ci::run(&root, source, full)
            }
        }
        ["linux-host"] => linux::host_checks(&root),
        _ => Err(USAGE.into()),
    }
}
