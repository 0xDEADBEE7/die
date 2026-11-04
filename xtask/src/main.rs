use xtask::util::{Result, root};
use xtask::{api, budget, ci, clean, cross, fonts, guards, lint, linux, mutate, sources};

const USAGE: &str = "usage: cargo xtask <task>

  budget               line counts against guard 7 and budget.toml
  lint                 unsafe only in platform modules, no target_os in neutral code,
                       no window or GPU crates under die-core and die-vim, exact pins,
                       D15, lockfile, licenses
  sources              every file the compiler read is one the lint reads, and every
                       local crate is a listed member
  guards               unsafe outside platform and -Cpanic=abort fail to build; dead
                       code on Linux and a build script that calls cc fail the
                       cross-check
  api-diff [--base R]  die-core's public API and die-vim's grammar, R -> working tree
                       (R defaults to the merge base with main)
  fonts                fetch Ioskeley Mono Term into fonts/, checked by SHA-256
  clean-build [--keep] no C builders in each target's build graph but the allowlist's,
                       and a build from an empty environment with no C compiler or
                       pkg-config, which no build script calls unless allowlisted (D16)
  cross-check          clippy -D warnings (so cargo check) of the workspace for
                       x86_64 and aarch64 Linux, with no C compiler or pkg-config
  mutate [--cases N]   die-core's planted bugs; the property tests must catch each
  ci [--worktree | --rev R]
                       every check in a fresh clone of HEAD (or R), or of the
                       working tree as git would commit it
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
        ["sources"] => sources::run(&root, &root.join("target/xtask/sources")),
        ["guards"] => guards::run(&root, &root.join("target/xtask/guards")),
        ["api-diff"] => api::run(&root, None),
        ["api-diff", "--base", b] => api::run(&root, Some(b)),
        ["fonts"] => fonts::run_fonts(&root),
        ["clean-build"] => clean::run(&root, false),
        ["clean-build", "--keep"] => clean::run(&root, true),
        ["cross-check"] => cross::run(&root, &root.join("target/xtask/cross")),
        ["mutate"] => mutate::run(&root, 2000),
        ["mutate", "--cases", n] => mutate::run(
            &root,
            n.parse()
                .map_err(|_| format!("--cases {n}: not a number"))?,
        ),
        ["ci", rest @ ..] => {
            let linux = rest.contains(&"--linux");
            let rest: Vec<&str> = rest.iter().copied().filter(|a| *a != "--linux").collect();
            let source = match rest.as_slice() {
                [] => ci::Source::Rev("HEAD".into()),
                ["--rev", r] => ci::Source::Rev(r.to_string()),
                ["--worktree"] => ci::Source::Worktree,
                _ => return Err(USAGE.into()),
            };
            if linux {
                linux::run(&root, source)
            } else {
                ci::run(&root, source)
            }
        }
        ["linux-host"] => linux::host_checks(&root),
        _ => Err(USAGE.into()),
    }
}
