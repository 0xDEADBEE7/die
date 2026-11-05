//! `cargo xtask lint`: the workspace rules the compiler can't check alone
//! (scope §12 and D15-D17, docs/records/platforms.md §5.2).
//!
//! - `unsafe`: every crate inherits the workspace's `unsafe_code = "forbid"`
//!   except two, which deny it and allow it in one module each: die-render, on
//!   `platform::macos`, and die-test-cap, on `cap`. die-test-cap is the one exception
//!   outside DIE's own code: a global allocator can't be written without `unsafe`, and
//!   the crate is never linked into DIE. Nothing else lowers that level, and nothing
//!   allows `warnings` wholesale.
//! - The `unsafe` keyword itself appears only in those modules, since rustc's lint
//!   misses some of it. `include!` and the `path` attribute appear nowhere, since each
//!   can compile a file this lint doesn't read. A macro can build that attribute in
//!   ways no token rule sees, and a manifest can point anywhere, so `cargo xtask
//!   sources` checks what actually got compiled.
//! - Those modules' `unsafe` blocks each say why they're sound and do one unsafe
//!   thing, and no code changes the level of the lints that hold them to it. No
//!   pointer is made from a reference to one element of a buffer by `NonNull::from`,
//!   `from_mut` or `from_ref`. Nothing outside `platform` names `platform::macos`:
//!   neutral code names `platform::backend`.
//! - Every test binary has a memory limit: each file cargo can build one from names
//!   die-test-cap, by the one line that does it, and its package has the dependency.
//!   That's a dev-dependency only, whatever a manifest calls it, so it can't reach a
//!   product build.
//! - Neutral code doesn't branch on the platform: no `target_os` and the like
//!   (in `cfg`, `cfg_attr` or `cfg_select!`, or anywhere as `target_os = "…"`),
//!   no `std::os` module but Unix's, no `env::consts::OS`, and no build script
//!   to set cfgs of its own, outside the named platform modules. A `cfg_attr`
//!   that only sets lint levels is fine.
//! - die-core's and die-vim's graphs hold no window, GPU or OS GUI crate, on
//!   any target.
//! - Every dependency is pinned to an exact version.
//! - die-vim depends on die-core only (D15), in every dependency table.
//! - The lockfile and both license files are there.

use crate::util::{Result, cargo, git, output, read, rust_files};
use std::path::Path;
use std::process::Command;
use toml::{Table, Value};

/// The only places `allow(unsafe_code)` may appear: (file, the item it must sit on).
/// `platform::linux` joins with the Linux backend; M2 adds die-term's engine
/// boundary and die-core's `sys` module. die-test-cap's allocator isn't linked into DIE.
const UNSAFE_ALLOWED: &[(&str, &str)] = &[
    ("crates/die-render/src/platform/mod.rs", "pub mod macos"),
    ("tools/test-cap/src/lib.rs", "mod cap"),
];

/// Where the `unsafe` keyword may appear (scope §12). M2 adds die-core's `sys/` and
/// die-term's engine boundary; `platform/linux/` joins with its code.
const UNSAFE_DIRS: &[&str] = &[
    "crates/die-render/src/platform/macos/",
    "tools/test-cap/src/cap.rs",
];

/// The clippy lints a crate that allows `unsafe` must deny, so that each block says
/// why it's sound and its SAFETY comment covers all of it.
const UNSAFE_BLOCK_LINTS: &[&str] = &[
    "undocumented_unsafe_blocks",
    "multiple_unsafe_ops_per_block",
];

/// The clippy group those lints are in. Allowing it turns them off as well.
const UNSAFE_BLOCK_GROUP: &str = "restriction";

/// The module that alone may name `platform::macos`.
const PLATFORM_DIR: &str = "crates/die-render/src/platform/";

/// Crates whose code is platform-neutral, and the paths in each that may branch
/// on the platform (platforms.md §5.2): die-core's `sys` module (M2) and
/// die-render's platform modules. Unix is the floor, so `cfg(unix)` and
/// `std::os::unix` are fine everywhere.
const NEUTRAL: &[(&str, &[&str])] = &[
    ("crates/die-core", &["crates/die-core/src/sys/"]),
    ("crates/die-vim", &[]),
    ("tools/evil-oracle", &[]),
    ("tools/test-cap", &[]),
    (
        "crates/die-render",
        &[
            "crates/die-render/src/platform/",
            "crates/die-render/tests/platform.rs",
        ],
    ),
];

/// cfg keys that pick a platform.
const PLATFORM_CFGS: &[&str] = &[
    "target_os",
    "target_vendor",
    "target_family",
    "target_env",
    "windows",
];

/// The `std::os` modules every Unix has.
const UNIX_OS: &[&str] = &["unix", "fd", "raw"];

/// Lints a platform `cfg_attr` may allow or expect in neutral code. Setting a
/// lint level changes no code, and neutral code that only the macOS backend
/// calls back into is dead on Linux until `platform::linux` lands, which the
/// cross-check's `-D warnings` rejects. `unused_*` also matches.
const LEVEL_ONLY_LINTS: &[&str] = &["dead_code", "unused"];

/// Crates die-core and die-vim may not have in their graph on any target:
/// window systems, GPU APIs and OS GUI frameworks (platforms.md §5.2). A
/// trailing `*` matches any suffix. Syscall bindings (libc, linux-raw-sys,
/// windows-sys) aren't listed: portable crates reach each OS through them, and
/// an unconditional platform-only crate fails the Linux cross-check anyway.
const PLATFORM_CRATES: &[(&str, &str)] = &[
    ("objc*", "Apple's Objective-C frameworks"),
    ("block2", "Apple's blocks"),
    ("dispatch2", "Apple's Grand Central Dispatch"),
    ("cocoa*", "AppKit"),
    ("core-graphics*", "Core Graphics"),
    ("core-text", "Core Text"),
    ("metal", "Metal"),
    ("winit", "a window system"),
    ("raw-window-handle", "a window system"),
    ("softbuffer", "a window system"),
    ("wayland*", "Wayland"),
    ("smithay*", "Wayland"),
    ("x11*", "X11"),
    ("xcb*", "X11"),
    ("xkbcommon*", "xkbcommon"),
    ("fontconfig*", "fontconfig"),
    ("yeslogic-fontconfig-sys", "fontconfig"),
    ("freetype*", "FreeType"),
    ("ndk*", "Android"),
    ("jni*", "Android's Java bridge"),
    ("wgpu*", "a GPU API"),
    ("naga", "a GPU API"),
    ("ash", "Vulkan"),
    ("glow", "OpenGL"),
    ("glutin*", "OpenGL"),
    ("khronos-egl", "EGL"),
    ("gpu-alloc*", "a GPU API"),
    ("gpu-descriptor*", "a GPU API"),
];

/// Crates that set their own lints instead of inheriting the workspace's.
const OWN_LINTS: &[&str] = &["crates/die-render", "tools/test-cap"];

/// The crate that gives each test binary its memory limit.
const TEST_CAP: &str = "die-test-cap";

/// The one test binary that doesn't name the cap, because the cap is its own code.
const TEST_CAP_LIB: &str = "tools/test-cap/src/lib.rs";

/// The directories of the crates this lint reads: the workspace's `members`. A path
/// dependency inside the workspace is a member too, for cargo, but isn't listed, and
/// `cargo xtask sources` fails it.
pub fn members(root: &Path) -> Result<Vec<String>> {
    let ws = parse(&root.join("Cargo.toml"))?;
    Ok(ws["workspace"]["members"]
        .as_array()
        .ok_or("workspace.members isn't a list")?
        .iter()
        .filter_map(|v| v.as_str().map(String::from))
        .collect())
}

/// Whether this lint reads `rel`, a path from the workspace's root, as part of
/// `member`. A member's own `target/` is build output, and a module named `target` isn't.
pub fn reads(member: &str, rel: &str) -> bool {
    rel.strip_prefix(member)
        .and_then(|r| r.strip_prefix('/'))
        .is_some_and(|r| r.ends_with(".rs") && !r.starts_with("target/"))
}

pub fn run(root: &Path) -> Result {
    let mut errors = Vec::new();
    let ws: Table = parse(&root.join("Cargo.toml"))?;
    let members = members(root)?;

    if let Some(deps) = ws["workspace"]
        .get("dependencies")
        .and_then(Value::as_table)
    {
        check_pins("Cargo.toml [workspace.dependencies]", deps, &mut errors);
    }
    if ws["workspace"]
        .get("lints")
        .and_then(|l| l.get("rust"))
        .and_then(|r| r.get("unsafe_code"))
        .and_then(Value::as_str)
        != Some("forbid")
    {
        errors.push("Cargo.toml: [workspace.lints.rust] must set unsafe_code = \"forbid\"".into());
    }

    for m in &members {
        let path = root.join(m).join("Cargo.toml");
        let man = parse(&path)?;
        let name = format!("{m}/Cargo.toml");
        for (key, deps) in dep_tables(&man) {
            check_pins(&format!("{name} [{key}]"), deps, &mut errors);
        }
        let lints = man.get("lints");
        if OWN_LINTS.contains(&m.as_str()) {
            check_own_lints(&name, lints, &mut errors);
        } else if lints
            .and_then(|l| l.get("workspace"))
            .and_then(Value::as_bool)
            != Some(true)
        {
            errors.push(format!("{name}: must have [lints] workspace = true"));
        }
        if man.get("package").and_then(|p| p.get("license")).is_none() {
            errors.push(format!("{name}: no license (use license.workspace = true)"));
        }
        if NEUTRAL.iter().any(|(c, _)| c == m) && has_build_script(&man, &root.join(m)) {
            errors.push(format!(
                "{name}: a build script. Neutral crates have none: one can read the target and \
                 set a cfg (`cargo::rustc-cfg`) that the platform rule can't see (platforms.md §5.2)"
            ));
        }
    }

    check_d15(
        &parse(&root.join("crates/die-vim/Cargo.toml"))?,
        &mut errors,
    );

    for m in &members {
        let neutral = NEUTRAL.iter().find(|(c, _)| c == m);
        for file in rust_files(&root.join(m)) {
            let rel = file
                .strip_prefix(root)
                .unwrap_or(&file)
                .to_string_lossy()
                .into_owned();
            if !reads(m, &rel) {
                continue;
            }
            let src = read(&file)?;
            check_unsafe_allows(&rel, &src, &mut errors);
            check_unsafe_tokens(&rel, &src, &mut errors);
            check_backend_name(&rel, &src, &mut errors);
            check_element_pointers(&rel, &src, &mut errors);
            if let Some((_, allowed)) = neutral
                && !allowed.iter().any(|a| rel.starts_with(a))
            {
                check_platform_code(&rel, &src, &mut errors);
            }
        }
    }

    let metadata = output(Command::new(cargo()).current_dir(root).args([
        "metadata",
        "--no-deps",
        "--offline",
        "--format-version",
        "1",
    ]))?;
    check_cap_dependencies(&metadata, &mut errors)?;
    for rel in test_roots(&metadata)? {
        check_test_root(&rel, &read(&root.join(&rel))?, &mut errors);
    }

    for krate in ["die-core", "die-vim"] {
        let tree = output(Command::new(cargo()).current_dir(root).args([
            "tree",
            "-p",
            krate,
            "--target",
            "all",
            "-e",
            "normal,build,dev",
            "--locked",
            "--offline",
            "--prefix",
            "none",
            "--format",
            "{p}",
        ]))?;
        for (pkg, what) in platform_crates(&tree) {
            errors.push(format!(
                "{krate} depends on {pkg} ({what}) on some target; die-core and die-vim are \
                 platform-neutral (platforms.md §5.2). `cargo tree -p {krate} --target all -i {}` shows the path",
                pkg.split_whitespace().next().unwrap_or("")
            ));
        }
    }

    for f in ["Cargo.lock", "LICENSE-MIT", "LICENSE-APACHE"] {
        if !root.join(f).exists() {
            errors.push(format!("{f} is missing"));
        } else if output(git(root).args(["check-ignore", "-q", f])).is_ok() {
            errors.push(format!("{f} is ignored by git; it must be committed"));
        }
    }

    if errors.is_empty() {
        println!("lint: ok ({} crates)", members.len());
        Ok(())
    } else {
        Err(format!("lint:\n  {}", errors.join("\n  ")))
    }
}

fn parse(path: &Path) -> Result<Table> {
    read(path)?
        .parse::<Table>()
        .map_err(|e| format!("{}: {e}", path.display()))
}

/// Every dependency table in a manifest, including target-specific ones.
fn dep_tables(man: &Table) -> Vec<(String, &Table)> {
    let kinds = ["dependencies", "dev-dependencies", "build-dependencies"];
    let mut out = Vec::new();
    for k in kinds {
        if let Some(t) = man.get(k).and_then(Value::as_table) {
            out.push((k.to_string(), t));
        }
    }
    if let Some(targets) = man.get("target").and_then(Value::as_table) {
        for (cfg, t) in targets {
            for k in kinds {
                if let Some(d) = t.get(k).and_then(Value::as_table) {
                    out.push((format!("target.{cfg}.{k}"), d));
                }
            }
        }
    }
    out
}

/// A crate in `OWN_LINTS` denies `unsafe_code` and `UNSAFE_BLOCK_LINTS` itself.
fn check_own_lints(name: &str, lints: Option<&Value>, errors: &mut Vec<String>) {
    let denied = |table: &str, lint: &str| {
        let level = lints
            .and_then(|l| l.get(table))
            .and_then(|t| t.get(lint))
            .and_then(Value::as_str);
        matches!(level, Some("deny" | "forbid"))
    };
    if !denied("rust", "unsafe_code") {
        errors.push(format!(
            "{name}: must set [lints.rust] unsafe_code = \"deny\""
        ));
    }
    for lint in UNSAFE_BLOCK_LINTS.iter().filter(|l| !denied("clippy", l)) {
        errors.push(format!("{name}: must set [lints.clippy] {lint} = \"deny\""));
    }
}

/// A list in `cargo metadata`'s output, or an empty one.
fn json_list<'a>(value: &'a serde_json::Value, key: &str) -> &'a [serde_json::Value] {
    value[key].as_array().map_or(&[][..], Vec::as_slice)
}

/// Parse `cargo metadata`'s output.
fn parse_metadata(metadata: &str) -> Result<serde_json::Value> {
    serde_json::from_str(metadata).map_err(|e| format!("lint: cargo metadata: {e}"))
}

/// `path` as a path from the workspace's root.
fn from_root(meta: &serde_json::Value, path: &str) -> Result<String> {
    path.strip_prefix(meta["workspace_root"].as_str().unwrap_or_default())
        .map(|rel| rel.trim_start_matches('/').to_string())
        .ok_or_else(|| format!("lint: {path} isn't under the workspace's root"))
}

/// Whether cargo can build `target` as a test binary. Only a build script can't. `test =
/// false` only leaves a target out of a plain `cargo test`, and that's a usual way to mark
/// an expensive test: `cargo test --test x` still runs it. An example isn't run unless it
/// says `test = true`. Helper modules, such as `tests/support/mod.rs`, aren't targets.
fn is_test_root(target: &serde_json::Value) -> bool {
    let kinds = json_list(target, "kind");
    let is = |kind: &str| kinds.iter().any(|k| k == kind);
    !is("custom-build") && (!is("example") || target["test"] == true)
}

/// die-test-cap may be a dev-dependency only, on every target and under any name, and a
/// package with a test binary must have it. `cargo metadata --no-deps` gives each
/// dependency's real name and kind, whatever the manifests say and wherever they say it,
/// the workspace's own table included.
pub fn check_cap_dependencies(metadata: &str, errors: &mut Vec<String>) -> Result {
    let meta = parse_metadata(metadata)?;
    for package in json_list(&meta, "packages") {
        if package["name"] == TEST_CAP {
            continue;
        }
        let manifest = from_root(&meta, package["manifest_path"].as_str().unwrap_or_default())?;
        let mut has_it = false;
        for dep in json_list(package, "dependencies") {
            if dep["name"] != TEST_CAP {
                continue;
            }
            if dep["kind"] == "dev" {
                has_it = true;
            } else {
                errors.push(format!(
                    "{manifest}: depends on {TEST_CAP} as a {} dependency. It belongs under \
                     [dev-dependencies] only: it replaces the global allocator, and may never \
                     reach a product build (scope §12)",
                    dep["kind"].as_str().unwrap_or("normal")
                ));
            }
        }
        if !has_it && json_list(package, "targets").iter().any(is_test_root) {
            errors.push(format!(
                "{manifest}: it has test binaries, and no {TEST_CAP} under \
                 [dev-dependencies], so they have no memory limit (tools/test-cap, scope §12)"
            ));
        }
    }
    Ok(())
}

/// The file each test binary is built from, as a path from the workspace's root, from
/// `cargo metadata --no-deps`. Cargo knows them all, wherever a manifest puts them: a
/// `tests/*.rs` file or a `tests/*/main.rs`, a crate's library and binaries, and a bench.
pub fn test_roots(metadata: &str) -> Result<Vec<String>> {
    let meta = parse_metadata(metadata)?;
    let mut roots = Vec::new();
    for package in json_list(&meta, "packages") {
        for target in json_list(package, "targets") {
            if is_test_root(target) {
                let path = target["src_path"].as_str().unwrap_or_default();
                roots.push(from_root(&meta, path)?);
            }
        }
    }
    Ok(roots)
}

/// A test binary's root says `use die_test_cap as _;` at its top level, under no attribute
/// but `#[cfg(test)]`, so a comment, a string, a macro or a `#[cfg(any())]` doesn't count.
/// Tokens, so its layout doesn't matter. `check_cap_dependencies` sees that the crate is
/// there to be found, and the compiler that a crate root keeps the line to `cfg(test)`,
/// since a dev-dependency is there only for tests.
pub fn check_test_root(rel: &str, src: &str, errors: &mut Vec<String>) {
    use crate::count::{Tok, lex};
    if rel == TEST_CAP_LIB {
        return;
    }
    let toks = lex(src);
    let word = |i: usize, s: &str| toks.get(i).map(|t| &t.0) == Some(&Tok::Ident(s.into()));
    let punct = |i: usize, c: char| toks.get(i).map(|t| &t.0) == Some(&Tok::Punct(c));
    let line_at = |i: usize| {
        word(i, "use")
            && word(i + 1, "die_test_cap")
            && word(i + 2, "as")
            && word(i + 3, "_")
            && punct(i + 4, ';')
    };
    // `#[cfg(test)]`, with its `]` at `end`.
    let cfg_test = |end: usize| {
        end >= 6
            && punct(end - 6, '#')
            && punct(end - 5, '[')
            && word(end - 4, "cfg")
            && punct(end - 3, '(')
            && word(end - 2, "test")
            && punct(end - 1, ')')
    };
    // The `[` of the attribute whose `]` is at `end`.
    let open_of = |end: usize| {
        let mut depth = 0;
        (0..=end).rev().find(|&j| {
            depth += i32::from(punct(j, ']')) - i32::from(punct(j, '['));
            depth == 0
        })
    };
    // No attribute on the line at `i` can take it away. An inner one, `#![...]`, is on the
    // file, and says nothing about the line.
    let kept = |i: usize| {
        let mut at = i;
        while at > 0 && punct(at - 1, ']') {
            match open_of(at - 1) {
                Some(open) if open > 0 && punct(open - 1, '!') => return true,
                Some(open) if open > 0 && cfg_test(at - 1) => at = open - 1,
                _ => return false,
            }
        }
        true
    };
    let mut depth = 0;
    for (i, tok) in toks.iter().enumerate() {
        match tok.0 {
            Tok::Punct('{' | '(' | '[') => depth += 1,
            Tok::Punct('}' | ')' | ']') => depth -= 1,
            _ => {}
        }
        if depth == 0 && line_at(i) && kept(i) {
            return;
        }
    }
    errors.push(format!(
        "{rel}: this test binary has no memory limit. Add `use die_test_cap as _;` (in a crate \
         root, under `#[cfg(test)]`) and `die-test-cap.workspace = true` under \
         [dev-dependencies] (tools/test-cap, scope §12)"
    ));
}

/// Cargo runs `build.rs` unless `package.build` is `false`, and runs the file
/// `package.build` names.
fn has_build_script(man: &Table, dir: &Path) -> bool {
    match man.get("package").and_then(|p| p.get("build")) {
        Some(Value::Boolean(false)) => false,
        Some(Value::Boolean(true)) | None => dir.join("build.rs").exists(),
        Some(_) => true,
    }
}

/// D15: die-vim depends on die-core only, in every dependency table, target
/// tables included. Its dev-dependencies are its tests' (the oracle).
fn check_d15(man: &Table, errors: &mut Vec<String>) {
    for (key, deps) in dep_tables(man) {
        if key.ends_with("dev-dependencies") {
            continue;
        }
        for k in deps.keys().filter(|k| *k != "die-core") {
            errors.push(format!(
                "crates/die-vim/Cargo.toml [{key}]: depends on {k}; die-vim may depend on die-core only (D15)"
            ));
        }
    }
}

/// The packages in a `cargo tree --prefix none --format {p}` listing that
/// `PLATFORM_CRATES` names, each once, with what it is.
fn platform_crates(tree: &str) -> Vec<(String, &'static str)> {
    let mut out: Vec<(String, &'static str)> = Vec::new();
    for line in tree.lines() {
        let pkg = line.trim_end_matches(" (*)").trim();
        let name = pkg.split_whitespace().next().unwrap_or("");
        let hit = PLATFORM_CRATES
            .iter()
            .find(|(p, _)| match p.strip_suffix('*') {
                Some(prefix) => name.starts_with(prefix),
                None => name == *p,
            });
        if let Some((_, what)) = hit
            && !out.iter().any(|(p, _)| p == pkg)
        {
            out.push((pkg.to_string(), what));
        }
    }
    out
}

/// Neutral code doesn't branch on the platform. This reads tokens, so strings
/// and comments don't count. It flags:
/// - a `PLATFORM_CFGS` key in a `cfg`, `cfg!` or `cfg_attr`, or in a
///   `cfg_select!` arm's predicate, except in a `cfg_attr` that only allows or
///   expects `LEVEL_ONLY_LINTS`;
/// - `target_os = "…"` (and the other keys) anywhere else, which catches a
///   renamed `cfg!` or `cfg_select!`;
/// - a `std::os` (or `core::os`) module but `UNIX_OS`, `std::os` itself
///   (`use std::os as host_os`), and `std` renamed;
/// - `env::consts::OS` and `FAMILY`, the target's OS as a run-time constant.
pub fn check_platform_code(rel: &str, src: &str, errors: &mut Vec<String>) {
    use crate::count::{Tok, lex, skip_group};
    use std::collections::BTreeMap;
    let toks = lex(src);
    let ident = |i: usize| match toks.get(i).map(|t| &t.0) {
        Some(Tok::Ident(w)) => Some(w.as_str()),
        _ => None,
    };
    let punct = |i: usize, c: char| toks.get(i).map(|t| &t.0) == Some(&Tok::Punct(c));
    let lit = |i: usize| toks.get(i).map(|t| &t.0) == Some(&Tok::Lit);
    let path_sep = |i: usize| punct(i, ':') && punct(i + 1, ':');
    let opens = |i: usize| punct(i, '(') || punct(i, '[') || punct(i, '{');
    let end_of = |open: usize| skip_group(&toks, open).unwrap_or(toks.len());
    // The comma-separated parts of the group opening at `open`: (first, past last).
    let parts = |open: usize| -> Vec<(usize, usize)> {
        let close = end_of(open) - 1;
        let (mut out, mut start, mut j) = (Vec::new(), open + 1, open + 1);
        while j < close {
            if opens(j) {
                j = end_of(j);
                continue;
            }
            if punct(j, ',') {
                out.push((start, j));
                start = j + 1;
            }
            j += 1;
        }
        if start < close {
            out.push((start, close));
        }
        out
    };
    // What follows `a::`: the first token of each part of a `{...}`, or the next token.
    let after_sep = |i: usize| -> Vec<usize> {
        if punct(i, '{') {
            parts(i).into_iter().map(|p| p.0).collect()
        } else {
            vec![i]
        }
    };
    // `cfg_attr(<predicate>, allow(dead_code), ...)`: the attributes set lint levels only.
    let levels_only = |open: usize| {
        let p = parts(open);
        p.len() > 1
            && p[1..].iter().all(|&(s, e)| {
                matches!(ident(s), Some("allow" | "expect"))
                    && punct(s + 1, '(')
                    && end_of(s + 1) == e
                    && parts(s + 1).iter().all(|&(a, b)| match b - a {
                        1 => ident(a).is_some_and(|l| {
                            LEVEL_ONLY_LINTS.contains(&l) || l.starts_with("unused_")
                        }),
                        3 => ident(a) == Some("reason") && punct(a + 1, '=') && lit(a + 2),
                        _ => false,
                    })
            })
    };
    // The predicates of the `cfg_select!` arms in the group opening at `open`.
    let predicates = |open: usize| -> Vec<(usize, usize)> {
        let close = end_of(open) - 1;
        let (mut out, mut j) = (Vec::new(), open + 1);
        while j < close {
            let from = j;
            while j < close && !(punct(j, '=') && punct(j + 1, '>')) {
                j = if opens(j) { end_of(j) } else { j + 1 };
            }
            out.push((from, j));
            j += 2;
            // The arm's code: a block, or an expression up to the next comma.
            if punct(j, '{') {
                j = end_of(j);
                if punct(j, ',') {
                    j += 1;
                }
            } else {
                while j < close && !punct(j, ',') {
                    j = if opens(j) { end_of(j) } else { j + 1 };
                }
                j += 1;
            }
        }
        out
    };
    let mut found: BTreeMap<usize, String> = BTreeMap::new();
    let mut flag = |t: usize, what: String| {
        found.entry(t).or_insert(what);
    };
    let mut skip = 0;
    for i in 0..toks.len() {
        if i < skip {
            continue;
        }
        let Some(word) = ident(i) else {
            continue;
        };
        let mut keys = |from: usize, to: usize, how: &str| {
            for t in from..to {
                if let Some(w) = ident(t)
                    && PLATFORM_CFGS.contains(&w)
                {
                    flag(t, format!("{how} on `{w}`"));
                }
            }
        };
        match word {
            "cfg" | "cfg_attr" => {
                let open = if punct(i + 1, '!') { i + 2 } else { i + 1 };
                if punct(open, '(') {
                    if word == "cfg_attr" && levels_only(open) {
                        skip = end_of(open);
                    } else {
                        keys(open, end_of(open), "cfg");
                    }
                }
            }
            "cfg_select" if punct(i + 1, '!') && opens(i + 2) => {
                for (from, to) in predicates(i + 2) {
                    keys(from, to, "cfg_select!");
                }
            }
            _ => {}
        }
        if PLATFORM_CFGS.contains(&word) && word != "windows" && punct(i + 1, '=') && lit(i + 2) {
            flag(i, format!("`{word} = \"…\"`"));
        }
        if matches!(word, "cfg" | "cfg_select" | "std" | "core") && ident(i + 1) == Some("as") {
            flag(i, format!("renaming `{word}`"));
        }
        if matches!(word, "std" | "core") && path_sep(i + 1) {
            for o in after_sep(i + 3) {
                match ident(o) {
                    Some("self") if ident(o + 1) == Some("as") => {
                        flag(o, format!("renaming `{word}`"));
                    }
                    Some("os") if !path_sep(o + 1) => flag(o, format!("`{word}::os` itself")),
                    Some("os") => {
                        for m in after_sep(o + 3) {
                            match ident(m) {
                                Some(w) if UNIX_OS.contains(&w) => {}
                                Some(w) => flag(m, format!("`{word}::os::{w}`")),
                                None => flag(m, format!("`{word}::os::*`")),
                            }
                        }
                    }
                    None if punct(o, '*') => flag(o, format!("`{word}::*`, which brings in `os`,")),
                    _ => {}
                }
            }
        }
        // `env::consts`, by any path; only `f32::consts` and `f64::consts` are other modules.
        let float = i >= 3 && path_sep(i - 2) && matches!(ident(i - 3), Some("f32" | "f64"));
        if word == "consts" && !float {
            if ident(i + 1) == Some("as") {
                flag(i, "renaming `env::consts`".into());
            } else if path_sep(i + 1) {
                for n in after_sep(i + 3) {
                    match ident(n) {
                        Some(w @ ("OS" | "FAMILY")) => flag(n, format!("`env::consts::{w}`")),
                        Some("self") if ident(n + 1) == Some("as") => {
                            flag(n, "renaming `env::consts`".into());
                        }
                        None if punct(n, '*') => flag(n, "`env::consts::*`".into()),
                        _ => {}
                    }
                }
            }
        }
    }
    for (t, what) in found {
        let levels = if what.starts_with("cfg on") {
            ". A `cfg_attr` may set lint levels only, as in \
             `#[cfg_attr(not(target_os = \"macos\"), allow(dead_code))]` on code that only \
             the macOS backend calls back into"
        } else {
            ""
        };
        errors.push(format!(
            "{rel}:{}: {what} branches on the platform. Neutral code may use `cfg(unix)` and \
             `std::os::unix` only; the rest goes in die-core's `sys` or die-render's `platform` \
             (platforms.md §5.2){levels}",
            toks[t].1 + 1
        ));
    }
}

/// A dependency is pinned when it's a path or workspace dependency, or its version starts with `=`.
fn check_pins(what: &str, deps: &Table, errors: &mut Vec<String>) {
    for (name, spec) in deps {
        let version = match spec {
            Value::String(v) => Some(v.as_str()),
            Value::Table(t) => {
                if t.get("workspace").and_then(Value::as_bool) == Some(true)
                    || (t.contains_key("path") && !t.contains_key("version"))
                {
                    continue;
                }
                t.get("version").and_then(Value::as_str)
            }
            _ => None,
        };
        match version {
            Some(v) if v.starts_with('=') => {}
            Some(v) => errors.push(format!(
                "{what}: {name} = {v:?} isn't pinned; write \"={}\"",
                v.trim_start_matches('^')
            )),
            None => errors.push(format!("{what}: {name} has no version")),
        }
    }
}

/// `allow(unsafe_code)` (or `expect`, or `warn`, which lowers die-render's
/// `deny` as well) may appear only where `UNSAFE_ALLOWED` says. `allow(warnings)`
/// and `expect(warnings)` may appear nowhere: after a `warn(unsafe_code)` they
/// would hide unsafe from clippy, and they hide what clippy's and the
/// cross-check's `-D warnings` catch. Strings and comments don't count: this
/// reads tokens.
pub fn check_unsafe_allows(rel: &str, src: &str, errors: &mut Vec<String>) {
    use crate::count::{Tok, lex, skip_group};
    let toks = lex(src);
    for i in 0..toks.len() {
        let level = match &toks[i].0 {
            Tok::Ident(w) if matches!(w.as_str(), "allow" | "expect" | "warn") => w.as_str(),
            _ => continue,
        };
        if toks.get(i + 1).map(|t| &t.0) != Some(&Tok::Punct('(')) {
            continue;
        }
        let Some(end) = skip_group(&toks, i + 1) else {
            continue;
        };
        let names = |lint: &str| {
            toks[i + 2..end]
                .iter()
                .any(|t| t.0 == Tok::Ident(lint.into()))
        };
        if level != "warn" && names("warnings") {
            errors.push(format!(
                "{rel}:{}: {level}(warnings) hides every warning from clippy's and the \
                 cross-check's -D warnings (and, after a warn(unsafe_code), unsafe code); \
                 {level} the lints you mean by name",
                toks[i].1 + 1
            ));
        }
        if !names("unsafe_code") {
            continue;
        }
        // The item the attribute sits on: the tokens after its `]`, up to `;` or `{`.
        let item: Vec<String> = toks[end..]
            .iter()
            .skip_while(|t| t.0 == Tok::Punct(']'))
            .take_while(|t| t.0 != Tok::Punct(';') && t.0 != Tok::Punct('{'))
            .map(|t| match &t.0 {
                Tok::Ident(w) => w.clone(),
                Tok::Punct(c) => c.to_string(),
                Tok::Lit => "_".into(),
            })
            .collect();
        let item = item.join(" ");
        if !UNSAFE_ALLOWED
            .iter()
            .any(|(f, it)| *f == rel && item == *it)
        {
            let allowed: Vec<String> = UNSAFE_ALLOWED
                .iter()
                .map(|(f, it)| format!("{f}: {it}"))
                .collect();
            errors.push(format!(
                "{rel}:{}: {level}(unsafe_code) outside the modules scope §12 permits ({})",
                toks[i].1 + 1,
                allowed.join("; ")
            ));
        }
    }
}

/// What rustc's `unsafe_code` lint can't confine, read from tokens, so strings and
/// comments don't count.
///
/// Outside `UNSAFE_DIRS`: the `unsafe` keyword in any form (a block, an `unsafe` fn,
/// impl, trait or extern block, and attributes such as `#[unsafe(naked)]`, which the
/// lint misses even under `forbid`), and `global_asm!`, which is unsafe code with no
/// keyword. Reading these also catches a lint level a macro builds
/// (`#[allow($lint)]`), which neither the lint nor `check_unsafe_allows` sees.
///
/// Everywhere: `include!` by any name and the `path` attribute, since each can compile
/// a file this lint never reads. These two give the clear message before a build. A
/// macro can build a `path` attribute this can't see, and `cargo xtask sources`
/// catches that. A format argument named `path` trips this
/// (`format!("{path}", path = p)`), and `{}` avoids that.
///
/// Everywhere: `UNSAFE_BLOCK_LINTS` and `UNSAFE_BLOCK_GROUP` as identifiers. The
/// manifest denies those lints, and an `allow` in the code would turn them off, even
/// one a macro builds, which still has to name them in its call. So a variable
/// named `restriction` trips this too, and another name avoids that.
pub fn check_unsafe_tokens(rel: &str, src: &str, errors: &mut Vec<String>) {
    use crate::count::{Tok, lex};
    let toks = lex(src);
    let p = |i: usize, c: char| toks.get(i).map(|x| &x.0) == Some(&Tok::Punct(c));
    let unsafe_ok = UNSAFE_DIRS.iter().any(|d| rel.starts_with(d));
    for (i, (t, line, _)) in toks.iter().enumerate() {
        let what = match t {
            Tok::Ident(w) if (w == "unsafe" || w == "global_asm") && !unsafe_ok => {
                format!("`{w}` outside {}", UNSAFE_DIRS.join(", "))
            }
            // Any use, so `use core::include as inc;` is caught too.
            Tok::Ident(w) if w == "include" => {
                "`include!` (it compiles a file this lint may not read)".into()
            }
            // `#[path = "..."]` and `#[cfg_attr(..., path = "...")]`.
            Tok::Ident(w)
                if w == "path" && i > 0 && (p(i - 1, '[') || p(i - 1, ',')) && p(i + 1, '=') =>
            {
                "`path` attribute (it compiles a file this lint may not read)".into()
            }
            Tok::Ident(w)
                if UNSAFE_BLOCK_LINTS.contains(&w.as_str()) || w == UNSAFE_BLOCK_GROUP =>
            {
                format!(
                    "`{w}`: the manifest of each crate with `unsafe` denies the unsafe-block \
                     lints, and code may not change their level"
                )
            }
            _ => continue,
        };
        errors.push(format!("{rel}:{}: {what} (scope §12)", line + 1));
    }
}

/// Neutral code names `platform::backend` (see `die_render::platform`), since
/// `platform::macos` doesn't exist on other targets.
pub fn check_backend_name(rel: &str, src: &str, errors: &mut Vec<String>) {
    use crate::count::{Tok, lex, skip_group};
    if rel.starts_with(PLATFORM_DIR) {
        return;
    }
    let toks = lex(src);
    let p = |i: usize, c: char| toks.get(i).map(|x| &x.0) == Some(&Tok::Punct(c));
    let macos = |t: &(Tok, usize, usize)| t.0 == Tok::Ident("macos".into());
    for (i, (t, line, _)) in toks.iter().enumerate() {
        if *t != Tok::Ident("platform".into()) || !p(i + 1, ':') || !p(i + 2, ':') {
            continue;
        }
        let names = match toks.get(i + 3) {
            Some((Tok::Punct('{'), ..)) => {
                let end = skip_group(&toks, i + 3).unwrap_or(toks.len());
                toks[i + 4..end].iter().any(macos)
            }
            Some(next) => macos(next),
            None => false,
        };
        if names {
            errors.push(format!(
                "{rel}:{}: names `platform::macos`; outside `platform`, name \
                 `platform::backend`, which every target has",
                line + 1
            ));
        }
    }
}

/// A pointer made from a reference to one element can't legally reach the others, and
/// C reads them through it. In `UNSAFE_DIRS`, this flags `NonNull::from`, `from_mut` and
/// `from_ref` (`ptr::from_mut` and the like) given a reference to an indexed place, as
/// in `NonNull::from(&mut self.chars[0])`. A pointer to a buffer covers the buffer:
/// `NonNull::from(&mut *a).cast()`.
pub fn check_element_pointers(rel: &str, src: &str, errors: &mut Vec<String>) {
    use crate::count::{Tok, lex};
    if !UNSAFE_DIRS.iter().any(|d| rel.starts_with(d)) {
        return;
    }
    let toks = lex(src);
    let p = |i: usize, c: char| toks.get(i).map(|x| &x.0) == Some(&Tok::Punct(c));
    let w = |i: usize, s: &str| toks.get(i).map(|x| &x.0) == Some(&Tok::Ident(s.into()));
    let name = |i: usize| matches!(toks.get(i), Some((Tok::Ident(_), ..)));
    for (i, (_, line, _)) in toks.iter().enumerate() {
        let open = if w(i, "from_mut") || w(i, "from_ref") {
            i + 1
        } else if w(i, "NonNull") && p(i + 1, ':') && p(i + 2, ':') && w(i + 3, "from") {
            i + 4
        } else {
            continue;
        };
        if !p(open, '(') || !p(open + 1, '&') {
            continue;
        }
        // Past `mut` and a `name.name.` path to the place's last name.
        let mut at = if w(open + 2, "mut") {
            open + 3
        } else {
            open + 2
        };
        while name(at) && p(at + 1, '.') {
            at += 2;
        }
        if name(at) && p(at + 1, '[') {
            errors.push(format!(
                "{rel}:{}: a pointer from a reference to one element can't legally reach \
                 the rest of its buffer; use `NonNull::from(&mut *buffer).cast()`",
                line + 1
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsafe_allow_only_on_platform_modules() {
        let check = |rel: &str, src: &str| {
            let mut e = Vec::new();
            check_unsafe_allows(rel, src, &mut e);
            e.len()
        };
        let render = "crates/die-render/src/platform/mod.rs";
        assert_eq!(
            check(
                render,
                "#[cfg(target_os = \"macos\")]\n#[allow(unsafe_code)]\npub mod macos;\n"
            ),
            0
        );
        assert_eq!(check(render, "#[allow(unsafe_code)]\nmod gpu;\n"), 1);
        assert_eq!(
            check(
                "crates/die-render/src/lib.rs",
                "#[allow(unsafe_code)]\npub mod platform;\n"
            ),
            1
        );
        assert_eq!(
            check(render, "#[allow(dead_code, unsafe_code)]\nmod other {}\n"),
            1
        );
        assert_eq!(
            check("crates/die-core/src/lib.rs", "#![allow( unsafe_code )]\n"),
            1
        );
        // die-test-cap allows it on its allocator's module, and nowhere else.
        let cap = "tools/test-cap/src/lib.rs";
        assert_eq!(check(cap, "#[allow(unsafe_code)]\nmod cap;\n"), 0);
        assert_eq!(check(cap, "#[allow(unsafe_code)]\nmod other;\n"), 1);
        assert_eq!(check(cap, "#![allow(unsafe_code)]\n"), 1);
        assert_eq!(
            check(
                "tools/test-cap/tests/cap.rs",
                "#[allow(unsafe_code)]\nmod cap;\n"
            ),
            1
        );
        assert_eq!(
            check(
                "crates/die-core/src/x.rs",
                "#[expect(unsafe_code)]\nfn f() {}\n"
            ),
            1
        );
        // `warn` lowers die-render's `deny` too, and `allow(warnings)` then hides
        // the unsafe block from clippy's -D warnings.
        let lib = "crates/die-render/src/lib.rs";
        assert_eq!(
            check(
                lib,
                "#[warn(unsafe_code)]\n#[allow(warnings)]\nmod sneaky {\n    pub fn read(p: *const u8) -> u8 {\n        unsafe { *p }\n    }\n}\n"
            ),
            2
        );
        assert_eq!(
            check(lib, "#[cfg_attr(all(), warn(unsafe_code))]\nmod m {}\n"),
            1
        );
        assert_eq!(check(lib, "#![allow(warnings)]\n"), 1);
        assert_eq!(
            check(lib, "#[cfg_attr(test, expect(warnings))]\nmod m {}\n"),
            1
        );
        assert_eq!(check(render, "#[allow(warnings)]\npub mod macos;\n"), 1);
        assert_eq!(check(lib, "#![warn(warnings)]\n#![allow(dead_code)]\n"), 0);
        // Raw identifiers are the same names.
        assert_eq!(check(lib, "#[r#allow(r#unsafe_code)]\nmod m {}\n"), 1);
        assert_eq!(check(lib, "fn f() { log::warn!(\"unsafe_code\"); }\n"), 0);
        // Not attributes, or not allows.
        assert_eq!(
            check(
                "crates/die-core/src/x.rs",
                "// allow(unsafe_code) in a comment\n"
            ),
            0
        );
        assert_eq!(
            check(
                "crates/die-core/src/x.rs",
                "const S: &str = \"#[allow(unsafe_code)]\";\n"
            ),
            0
        );
        assert_eq!(
            check("crates/die-core/src/lib.rs", "#![forbid(unsafe_code)]\n"),
            0
        );
    }

    #[test]
    fn unsafe_tokens() {
        let core = "crates/die-core/src/x.rs";
        let check = |rel: &str, src: &str| {
            let mut e = Vec::new();
            check_unsafe_tokens(rel, src, &mut e);
            e
        };
        // Each is one error at die-core, and the first three pass rustc and every
        // other check: `cargo xtask guards` plants them end to end.
        let flagged = [
            // A naked function is machine code with no `unsafe` block.
            "#[unsafe(naked)]\npub extern \"C\" fn f() { core::arch::naked_asm!(\"ret\") }\n",
            // A lint level a macro builds: `allow(unsafe_code)` never appears.
            "macro_rules! level { ($lint:ident, $($i:item)*) => { #[allow($lint)] pub mod planted { $($i)* } }; }\n\
             level!(unsafe_code, pub fn ok() -> &'static str { unsafe { std::str::from_utf8_unchecked(b\"ok\") } });\n",
            "#[path = \"../../docs/x.rs\"]\nmod x;\n",
            // Other spellings of the same three.
            "unsafe extern \"C\" { safe fn abs(x: i32) -> i32; }\n",
            "unsafe impl Send for S {}\n",
            "core::arch::global_asm!(\"nop\");\n",
            "use core::arch::global_asm as g;\n",
            "include!(\"x.rs\");\n",
            "use core::include as inc;\n",
            "#[cfg_attr(test, path = \"x.rs\")]\nmod x;\n",
            // Raw identifiers are the same names.
            "#[r#path = \"x.rs\"]\nmod x;\n",
            "#[cfg_attr(test, r#path = \"x.rs\")]\nmod x;\n",
            "r#include!(\"x.rs\");\n",
            "core::arch::r#global_asm!(\"nop\");\n",
        ];
        for src in flagged {
            assert_eq!(check(core, src).len(), 1, "{src}");
        }
        let e = check(core, flagged[0]);
        assert_eq!(
            e,
            [format!(
                "{core}:1: `unsafe` outside crates/die-render/src/platform/macos/, \
                 tools/test-cap/src/cap.rs (scope §12)"
            )]
        );
        let macos = "crates/die-render/src/platform/macos/text.rs";
        let e = check("crates/die-render/src/platform/mod.rs", "unsafe {}\n");
        assert_eq!(e.len(), 1, "platform's neutral part: {e:?}");
        // The cap's one exception is its one file, not its crate.
        let cap = "tools/test-cap/src/cap.rs";
        assert_eq!(check("tools/test-cap/src/lib.rs", "unsafe {}\n").len(), 1);
        assert_eq!(check("tools/test-cap/tests/cap.rs", "unsafe {}\n").len(), 1);
        let fine = [
            (cap, "unsafe impl GlobalAlloc for Cap {}\n"),
            (macos, "unsafe {}\n"),
            (macos, "unsafe extern \"C\" { fn abs(x: i32) -> i32; }\n"),
            (core, "const S: &str = \"unsafe {}\";\n"),
            (
                core,
                "// unsafe, global_asm!, include!(\"x\"), #[path = \"x\"]\n",
            ),
            (core, "#![forbid(unsafe_code)]\n"),
            (core, "include_str!(\"x\");\ninclude_bytes!(\"x\");\n"),
            (core, "fn f() { let path = \"x\"; g(path); }\n"),
            (core, "fn f(path: &str) -> bool { path == \"x\" }\n"),
            (
                core,
                "macro_rules! m { ($a:expr, $b:expr) => { f($a, $b) } }\n",
            ),
            (core, "macro_rules! m { ($a:expr) => { vec![$a] } }\n"),
            // A macro passing attributes through: `cargo xtask sources` holds the `path`
            // one can build.
            (
                core,
                "macro_rules! m { ($(#[$m:meta])*) => { $(#[$m])* struct S; } }\n",
            ),
        ];
        for (rel, src) in fine {
            assert_eq!(check(rel, src), Vec::<String>::new(), "{src}");
        }
        // The `path` and `include` rules hold in the unsafe modules too.
        assert_eq!(check(macos, "#[path = \"x.rs\"]\nmod x;\n").len(), 1);
        assert_eq!(check(macos, "include!(\"x.rs\");\n").len(), 1);
    }

    #[test]
    fn nothing_turns_off_the_unsafe_block_lints() {
        let check = |rel: &str, src: &str| {
            let mut e = Vec::new();
            check_unsafe_tokens(rel, src, &mut e);
            e.len()
        };
        let macos = "crates/die-render/src/platform/macos/text.rs";
        let lib = "crates/die-render/src/lib.rs";
        for src in [
            "#[allow(clippy::undocumented_unsafe_blocks)]\nfn f() {}\n",
            "#[expect(clippy::multiple_unsafe_ops_per_block)]\nfn f() {}\n",
            "#![cfg_attr(all(), allow(clippy::undocumented_unsafe_blocks))]\n",
            // The group both lints are in.
            "#[allow(clippy::restriction)]\nfn f() {}\n",
            // A level a macro builds still names the lint in its call.
            "macro_rules! l { ($l:path, $i:item) => { #[allow($l)] $i } }\n\
             l!(clippy::multiple_unsafe_ops_per_block, fn f() {});\n",
            "macro_rules! l { ($l:ident, $i:item) => { #[allow(clippy::$l)] $i } }\n\
             l!(restriction, fn f() {});\n",
        ] {
            assert_eq!(check(macos, src), 1, "{src}");
            assert_eq!(check(lib, src), 1, "{src}");
        }
        for src in [
            "// clippy::undocumented_unsafe_blocks, clippy::restriction\n",
            "const S: &str = \"multiple_unsafe_ops_per_block\";\n",
            "#[allow(clippy::needless_return)]\nfn f() {}\n",
        ] {
            assert_eq!(check(macos, src), 0, "{src}");
        }
    }

    #[test]
    fn the_lint_reads_what_is_in_a_member_but_its_target() {
        for (rel, read) in [
            ("crates/die-render/src/lib.rs", true),
            ("crates/die-render/tests/x.rs", true),
            // A module named `target` is code.
            ("crates/die-render/src/target/mod.rs", true),
            ("crates/die-render/src/a/target/b.rs", true),
            // The member's own `target` is build output.
            ("crates/die-render/target/x.rs", false),
            // Not Rust, or not this member.
            ("crates/die-render/src/x.txt", false),
            ("crates/die-render2/src/x.rs", false),
            ("crates/die-core/src/x.rs", false),
            ("docs/x.rs", false),
        ] {
            assert_eq!(reads("crates/die-render", rel), read, "{rel}");
        }
    }

    #[test]
    fn neutral_code_names_the_backend() {
        let check = |rel: &str, src: &str| {
            let mut e = Vec::new();
            check_backend_name(rel, src, &mut e);
            e.len()
        };
        let neutral = "crates/die-render/src/atlas.rs";
        for src in [
            "use crate::platform::macos::text::Face;\n",
            "use die_render::platform::macos;\n",
            "fn f() { crate::platform::macos::ready(); }\n",
            "use crate::platform::{backend, macos::text};\n",
        ] {
            assert_eq!(check(neutral, src), 1, "{src}");
            assert_eq!(check("crates/die-render/tests/x.rs", src), 1, "{src}");
            assert_eq!(check("apps/die-dev/src/x.rs", src), 1, "{src}");
            assert_eq!(
                check("crates/die-render/src/platform/x.rs", src),
                0,
                "{src}"
            );
        }
        for src in [
            "use crate::platform::backend::text::Face;\n",
            "use crate::platform::{backend};\n",
            "// platform::macos\n",
            "const S: &str = \"platform::macos\";\n",
            "use std::os::macos;\n",
            "#[cfg_attr(not(target_os = \"macos\"), allow(dead_code))]\nfn f() {}\n",
        ] {
            assert_eq!(check(neutral, src), 0, "{src}");
        }
    }

    #[test]
    fn pointers_to_a_buffer_cover_it() {
        let check = |rel: &str, src: &str| {
            let mut e = Vec::new();
            check_element_pointers(rel, src, &mut e);
            e.len()
        };
        let text = "crates/die-render/src/platform/macos/text.rs";
        let old = "f(NonNull::from(&mut chars[0]), NonNull::from(&glyphs[0]), 2);\n";
        assert_eq!(check(text, old), 2);
        assert_eq!(check("crates/die-core/src/x.rs", old), 0, "no unsafe there");
        // A buffer held in a field, and the other ways to make the pointer.
        for src in [
            "f(NonNull::from(&mut self.chars[0]));\n",
            "f(NonNull::from(&self.a.b.glyphs[i]));\n",
            "f(core::ptr::NonNull::from(&mut self.0[0]));\n",
            "f(std::ptr::from_mut(&mut self.chars[0]));\n",
            "f(ptr::from_ref(&chars[0]));\n",
            "f(from_mut(&mut chars[0]));\n",
        ] {
            assert_eq!(check(text, src), 1, "{src}");
        }
        for src in [
            "f(NonNull::from(&mut *chars).cast(), NonNull::from(&mut glyphs).cast(), 2);\n",
            "f(NonNull::from(&mut g), &mut adv, 1);\n",
            "let a = NonNull::from(x);\n",
            "f(NonNull::from(&mut self.chars).cast());\n",
            "f(std::ptr::from_mut(&mut self.chars).cast());\n",
            "f(ptr::from_ref(&self.g));\n",
            "f(NonNull::from(&mut x).cast(), a[0]);\n",
            "let n = NonNull::from(&mut self.chars.len);\n",
            "let n = Thing::from(&a[0]);\n",
        ] {
            assert_eq!(check(text, src), 0, "{src}");
        }
    }

    #[test]
    fn a_crate_with_its_own_lints_denies_unsafe_and_the_block_lints() {
        let check = |toml: &str| {
            let man: Table = toml.parse().unwrap();
            let mut e = Vec::new();
            check_own_lints("m", man.get("lints"), &mut e);
            e
        };
        let full = "[lints.rust]\nunsafe_code = \"deny\"\n[lints.clippy]\n\
                    undocumented_unsafe_blocks = \"deny\"\n\
                    multiple_unsafe_ops_per_block = \"deny\"\n";
        assert_eq!(check(full), Vec::<String>::new());
        assert_eq!(
            check("[lints.rust]\nunsafe_code = \"deny\"\n"),
            [
                "m: must set [lints.clippy] undocumented_unsafe_blocks = \"deny\"",
                "m: must set [lints.clippy] multiple_unsafe_ops_per_block = \"deny\"",
            ]
        );
        assert_eq!(check(&full.replacen("deny", "warn", 1)).len(), 1);
        assert_eq!(
            check(&full.replace(
                "undocumented_unsafe_blocks = \"deny\"",
                "undocumented_unsafe_blocks = \"warn\""
            ))
            .len(),
            1
        );
        assert_eq!(check("").len(), 3);
    }

    #[test]
    fn a_test_binary_names_the_cap() {
        let check = |rel: &str, src: &str| {
            let mut e = Vec::new();
            check_test_root(rel, src, &mut e);
            e
        };
        let test = "crates/die-core/tests/new.rs";
        let e = check(test, "#[test]\nfn f() {}\n");
        assert_eq!(e.len(), 1, "{e:?}");
        assert!(
            e[0].starts_with(test) && e[0].contains("no memory limit"),
            "{e:?}"
        );
        for src in [
            "",
            "// use die_test_cap as _;\n",
            "const S: &str = \"use die_test_cap as _;\";\n",
            // An attribute that takes it away again.
            "#[cfg(any())]\nuse die_test_cap as _;\n",
            "#[cfg(not(test))]\nuse die_test_cap as _;\n",
            "#[cfg(any())]\n#[cfg(test)]\nuse die_test_cap as _;\n",
            "#[cfg(test)]\n#[cfg(any())]\nuse die_test_cap as _;\n",
            // Not the line.
            "use die_test_cap;\n",
            "use other as _;\n",
            "use die_test_cap as cap;\n",
            "mod m {\n    use die_test_cap as _;\n}\n",
            "fn f() {\n    use die_test_cap as _;\n}\n",
            // Words only, in brackets of each kind: none of these ever compiles the line.
            "const _: &str = stringify!(use die_test_cap as _;);\n",
            "const _: &str = stringify![use die_test_cap as _;];\n",
            "macro_rules! unused ( () => (use die_test_cap as _;); );\n",
            "macro_rules! unused [ () => [use die_test_cap as _;]; ];\n",
        ] {
            assert_eq!(check(test, src).len(), 1, "{src}");
        }
        for src in [
            "use die_test_cap as _;\n",
            "//! Docs.\n#![allow(clippy::type_complexity)]\n\nuse die_test_cap as _;\n\nmod support;\n",
            "#![cfg(target_os = \"macos\")]\n\nuse die_test_cap as _;\n",
            "#[cfg(test)]\nuse die_test_cap as _;\n",
            "#[cfg( test )] use  die_test_cap  as _ ;\n",
            "#![forbid(unsafe_code)]\n\n#[cfg(test)]\nuse die_test_cap as _;\n",
            "mod m {}\nuse die_test_cap as _;\n",
            "#[derive(Debug)]\nstruct S;\nuse die_test_cap as _;\n",
        ] {
            assert_eq!(check(test, src), Vec::<String>::new(), "{src}");
        }
        // The one that can't: it's the cap. Its tests can.
        assert_eq!(check(TEST_CAP_LIB, ""), Vec::<String>::new());
        assert_eq!(check("tools/test-cap/tests/cap.rs", "").len(), 1);
        assert_eq!(check("tools/test-cap/src/cap.rs", "").len(), 1);
    }

    #[test]
    fn test_roots_are_what_cargo_tests() {
        let target = |kind: &str, path: &str, test: bool| serde_json::json!({"kind": [kind], "src_path": path, "test": test});
        let meta = serde_json::json!({
            "workspace_root": "/w",
            "packages": [
                {"targets": [
                    target("lib", "/w/crates/a/src/lib.rs", true),
                    target("bin", "/w/crates/a/src/main.rs", true),
                    target("test", "/w/crates/a/tests/new.rs", true),
                    target("test", "/w/crates/a/tests/dir/main.rs", true),
                    target("test", "/w/crates/a/checks/named.rs", true),
                    target("bench", "/w/crates/a/benches/b.rs", false),
                    target("example", "/w/crates/a/examples/e.rs", false),
                    target("example", "/w/crates/a/examples/tested.rs", true),
                    target("custom-build", "/w/crates/a/build.rs", false),
                ]},
                {"targets": [
                    target("proc-macro", "/w/crates/b/src/m.rs", true),
                    // `test = false` only leaves these out of a plain `cargo test`.
                    target("lib", "/w/crates/b/src/lib.rs", false),
                    target("bin", "/w/crates/b/src/main.rs", false),
                    target("test", "/w/crates/b/tests/by_hand.rs", false),
                ]},
            ],
        });
        assert_eq!(
            test_roots(&meta.to_string()).unwrap(),
            [
                "crates/a/src/lib.rs",
                "crates/a/src/main.rs",
                "crates/a/tests/new.rs",
                "crates/a/tests/dir/main.rs",
                "crates/a/checks/named.rs",
                "crates/a/benches/b.rs",
                "crates/a/examples/tested.rs",
                "crates/b/src/m.rs",
                "crates/b/src/lib.rs",
                "crates/b/src/main.rs",
                "crates/b/tests/by_hand.rs",
            ]
        );
        assert!(test_roots("not json").is_err());
        let outside = serde_json::json!({
            "workspace_root": "/w",
            "packages": [{"targets": [target("test", "/x/t.rs", true)]}],
        });
        assert!(test_roots(&outside.to_string()).is_err());
    }

    #[test]
    fn the_cap_is_a_dev_dependency_only() {
        // `cargo metadata`'s words for a dependency: its real name, however a manifest
        // renames it, and its kind, which is null for a normal one.
        let dep = |name: &str, kind: Option<&str>| serde_json::json!({"name": name, "kind": kind, "rename": "any", "target": "cfg(unix)"});
        let check = |deps: Vec<serde_json::Value>| {
            let meta = serde_json::json!({
                "workspace_root": "/w",
                "packages": [{
                    "name": "m",
                    "manifest_path": "/w/m/Cargo.toml",
                    "dependencies": deps,
                    "targets": [{"kind": ["lib"], "src_path": "/w/m/src/lib.rs", "test": true}],
                }],
            });
            let mut e = Vec::new();
            check_cap_dependencies(&meta.to_string(), &mut e).unwrap();
            e
        };
        let cap = TEST_CAP;
        assert_eq!(check(vec![dep(cap, Some("dev"))]), Vec::<String>::new());
        assert_eq!(
            check(vec![dep("other", None), dep(cap, Some("dev"))]),
            Vec::<String>::new()
        );
        for (deps, kind) in [
            (vec![dep(cap, None), dep(cap, Some("dev"))], "normal"),
            (
                vec![dep(cap, Some("build")), dep(cap, Some("dev"))],
                "build",
            ),
        ] {
            let e = check(deps);
            assert_eq!(e.len(), 1, "{e:?}");
            assert!(
                e[0].starts_with(&format!("m/Cargo.toml: depends on {cap} as a {kind} ")),
                "{e:?}"
            );
        }
        // Having it only as something else fails both ways.
        assert_eq!(check(vec![dep(cap, None)]).len(), 2);
    }

    #[test]
    fn a_package_with_a_test_binary_needs_the_cap() {
        let check = |name: &str, targets: serde_json::Value| {
            let meta = serde_json::json!({
                "workspace_root": "/w",
                "packages": [{
                    "name": name,
                    "manifest_path": "/w/m/Cargo.toml",
                    "dependencies": [{"name": "other", "kind": "dev"}],
                    "targets": targets,
                }],
            });
            let mut e = Vec::new();
            check_cap_dependencies(&meta.to_string(), &mut e).unwrap();
            e
        };
        let target = |kind: &str| serde_json::json!({"kind": [kind], "test": false});
        // `mod die_test_cap {}` satisfies the line, and only this catches it.
        for kind in ["lib", "bin", "test", "bench"] {
            let e = check("m", serde_json::json!([target(kind)]));
            assert_eq!(e.len(), 1, "{kind}: {e:?}");
            assert!(
                e[0].starts_with("m/Cargo.toml: ") && e[0].contains("no memory limit"),
                "{e:?}"
            );
        }
        let none = Vec::<String>::new();
        assert_eq!(check("m", serde_json::json!([target("example")])), none);
        assert_eq!(
            check("m", serde_json::json!([target("custom-build")])),
            none
        );
        // The cap can't depend on itself.
        assert_eq!(check(TEST_CAP, serde_json::json!([target("lib")])), none);
    }

    /// The other half of the line's proof: `tools/test-cap/tests/linked.rs` shows it in a
    /// `tests/*.rs` file, and this one in a crate root, under `#[cfg(test)]`. The block's
    /// pages are never touched, so this costs nothing if the cap is missing.
    #[test]
    fn the_line_in_a_crate_root_installs_the_cap() {
        const CHILD: &str = "XTASK_TEST_CAP_CHILD";
        let name = "lint::tests::the_line_in_a_crate_root_installs_the_cap";
        if std::env::var_os(CHILD).is_some() {
            std::hint::black_box(vec![0u8; 1 << 40]);
            std::process::exit(0);
        }
        let out = Command::new(std::env::current_exe().unwrap())
            .args([name, "--exact"])
            .env(CHILD, "1")
            .output()
            .unwrap();
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(!out.status.success(), "{out:?}");
        assert!(stderr.contains("die-test-cap: "), "{out:?}");
    }

    #[test]
    fn platform_code() {
        let check = |src: &str| {
            let mut e = Vec::new();
            check_platform_code("crates/die-core/src/x.rs", src, &mut e);
            e
        };
        let flagged = [
            "#[cfg(target_os = \"macos\")]\nfn f() {}\n",
            "#[cfg(not(target_os = \"linux\"))]\nmod m;\n",
            "#[cfg(any(unix, windows))]\nfn f() {}\n",
            "#[cfg_attr(target_vendor = \"apple\", derive(Debug))]\nstruct S;\n",
            "#[cfg(target_family = \"unix\")]\nfn f() {}\n",
            "#[cfg(target_env = \"musl\")]\nfn f() {}\n",
            "fn f() -> bool { cfg!(target_os = \"macos\") }\n",
            "use std::os::macos::fs::MetadataExt;\n",
            "use std::os::{fd::AsRawFd, linux::fs::MetadataExt};\n",
            "use std::{io, os::freebsd::fs::MetadataExt};\n",
            "fn f() { let _ = core::os::windows::raw::HANDLE::default(); }\n",
            // cfg_select! (stable since 1.95), on items and in expressions.
            "cfg_select! {\n    target_os = \"macos\" => { fn f() {} }\n    _ => { fn f() {} }\n}\n",
            "cfg_select! {\n    unix => { fn f() {} }\n    windows => { fn f() {} }\n}\n",
            "fn f() -> u8 { cfg_select! { all(unix, not(target_vendor = \"apple\")) => 1, _ => { 2 } } }\n",
            "core::cfg_select! { not(windows) => { mod m; } }\n",
            // A renamed cfg macro still names its key.
            "use core::cfg_select as pick;\n",
            "pick! { target_os = \"linux\" => { fn f() {} } }\n",
            "fn f() -> bool { also_cfg!(target_family = \"unix\") }\n",
            // std::os itself, renamed, or as a glob; std renamed.
            "use std::os;\n",
            "use std::os as host_os;\n",
            "use std::{io, os};\n",
            "use std::os::{self};\n",
            "use std::os::*;\n",
            "use std::*;\n",
            "use std as s;\n",
            "extern crate core as c;\n",
            "use std::{self as s};\n",
            // The target's OS at run time.
            "fn f() -> bool { std::env::consts::OS == \"macos\" }\n",
            "use std::env::consts::FAMILY;\n",
            "use std::env::{consts::{ARCH, OS}};\n",
            "use std::env::consts::*;\n",
            "use std::env::consts as c;\n",
            "fn f() -> bool { consts::OS == \"linux\" }\n",
            // A cfg_attr that sets more than lint levels.
            "#[cfg_attr(target_os = \"macos\", path = \"mac.rs\")]\nmod m;\n",
            "#[cfg_attr(not(target_os = \"macos\"), allow(dead_code), inline)]\nfn f() {}\n",
            "#[cfg_attr(not(target_os = \"macos\"), allow(clippy::all))]\nfn f() {}\n",
            "#[cfg_attr(not(target_os = \"macos\"), allow(unsafe_code))]\nfn f() {}\n",
        ];
        for src in flagged {
            assert_eq!(check(src).len(), 1, "{src}");
        }
        // The reviewer's plant: two cfg_select arms' code, a renamed std::os, and
        // the run-time OS.
        let plant = "cfg_select! {\n    target_os = \"macos\" => {\n        pub fn blocks(m: &Metadata) -> u64 {\n            use std::os as host_os;\n            use host_os::darwin::fs::MetadataExt;\n            m.st_blocks()\n        }\n    }\n    _ => { pub fn blocks(m: &Metadata) -> u64 { m.len() } }\n}\n\npub fn home() -> &'static str {\n    if std::env::consts::OS == \"macos\" { \"Library\" } else { \".local\" }\n}\n";
        let e = check(plant);
        assert_eq!(e.len(), 3, "{e:#?}");
        assert!(
            e[0].starts_with("crates/die-core/src/x.rs:2: cfg_select! on `target_os`"),
            "{e:?}"
        );
        assert!(
            e[1].starts_with("crates/die-core/src/x.rs:4: `std::os` itself"),
            "{e:?}"
        );
        assert!(
            e[2].starts_with("crates/die-core/src/x.rs:13: `env::consts::OS`"),
            "{e:?}"
        );
        let e = check("\n#[cfg(target_os = \"macos\")]\nfn f() {}\n");
        assert!(
            e[0].starts_with("crates/die-core/src/x.rs:2: cfg on `target_os`"),
            "{e:?}"
        );
        let fine = [
            "#[cfg(unix)]\nfn f() {}\n",
            "#[cfg(not(unix))]\nfn f() {}\n",
            "#[cfg(all(test, target_pointer_width = \"64\"))]\nmod t;\n",
            "use std::os::unix::fs::MetadataExt;\n",
            "use std::os::{fd::AsRawFd, unix::fs::MetadataExt};\n",
            "use std::{io, os::unix::fs::MetadataExt};\n",
            "fn f() { let windows = 2; let target_os = windows; }\n",
            "// #[cfg(target_os = \"macos\")]\n",
            "const S: &str = \"cfg(target_os = \\\"macos\\\")\";\n",
            "use my::os::linux;\n",
            "cfg_select! {\n    unix => { fn f() { let windows = 1; } }\n    _ => { fn f() {} }\n}\n",
            "fn f() -> u8 { cfg_select! { unix => { let target_os = 1; target_os }, _ => 2 } }\n",
            "fn f() -> bool { target_os == \"macos\" }\n",
            "use std::f64::consts::*;\nuse std::f32::consts as fc;\n",
            "fn f() -> f64 { std::f64::consts::PI }\n",
            "use std::env::consts::ARCH;\n",
            "use std::{env, io, os::unix};\n",
            // Lint levels only: code only platform::macos calls is dead on Linux.
            "#[cfg_attr(not(target_os = \"macos\"), allow(dead_code))]\nfn f() {}\n",
            "#[cfg_attr(not(target_os = \"macos\"), expect(dead_code, unused_imports, reason = \"platform::macos only\"))]\nmod m {}\n",
            "#[cfg_attr(target_os = \"linux\", allow(unused), allow(unused_mut,))]\nfn f() {}\n",
        ];
        for src in fine {
            assert_eq!(check(src), Vec::<String>::new(), "{src}");
        }
    }

    #[test]
    fn build_scripts() {
        let dir = crate::util::TempDir::new("lint-build").unwrap();
        let man = |s: &str| -> Table { format!("[package]\nname = \"x\"\n{s}").parse().unwrap() };
        assert!(!has_build_script(&man(""), dir.path()));
        assert!(has_build_script(&man("build = \"gen.rs\""), dir.path()));
        crate::util::write(&dir.path().join("build.rs"), "fn main() {}\n").unwrap();
        assert!(has_build_script(&man(""), dir.path()));
        assert!(has_build_script(&man("build = true"), dir.path()));
        assert!(!has_build_script(&man("build = false"), dir.path()));
    }

    #[test]
    fn platform_crates_in_a_tree() {
        let tree = "die-core v0.0.0 (/w/crates/die-core)\nropey v1.6.1\nlibc v0.2.191\n\
                    objc2 v0.6.5\nobjc2-foundation v0.3.2\nobjc2 v0.6.5 (*)\nwinit v0.30.13\n\
                    x11rb v0.13.2\nwindows-sys v0.61.2\nlinux-raw-sys v0.11.0\nmetal-rs v0.1.0\n";
        let found: Vec<String> = platform_crates(tree).into_iter().map(|(p, _)| p).collect();
        assert_eq!(
            found,
            [
                "objc2 v0.6.5",
                "objc2-foundation v0.3.2",
                "winit v0.30.13",
                "x11rb v0.13.2"
            ]
        );
    }

    #[test]
    fn d15_in_every_table() {
        let man: Table = r#"
            [dependencies]
            die-core = { workspace = true }
            [build-dependencies]
            cc = "=1.8.0"
            [dev-dependencies]
            evil-oracle = { workspace = true }
            [target.'cfg(target_os = "macos")'.dependencies]
            die-core = { workspace = true }
            objc2 = "=0.6.5"
            [target.'cfg(unix)'.dev-dependencies]
            libc = "=0.2.191"
        "#
        .parse()
        .unwrap();
        let mut e = Vec::new();
        check_d15(&man, &mut e);
        assert_eq!(e.len(), 2, "{e:?}");
        assert!(
            e[0].contains("[build-dependencies]: depends on cc"),
            "{e:?}"
        );
        assert!(
            e[1].contains("[target.cfg(target_os = \"macos\").dependencies]: depends on objc2"),
            "{e:?}"
        );
    }

    #[test]
    fn pins() {
        let t: Table = r#"
            a = "=1.0.0"
            b = "1.0"
            c = { version = "^2", features = ["x"] }
            d = { workspace = true }
            e = { path = "../e" }
            f = { version = "=0.3.2", default-features = false }
        "#
        .parse()
        .unwrap();
        let mut e = Vec::new();
        check_pins("t", &t, &mut e);
        assert_eq!(e.len(), 2, "{e:?}");
        assert!(
            e[0].contains("b = \"1.0\"") && e[1].contains("c = \"^2\""),
            "{e:?}"
        );
    }
}
