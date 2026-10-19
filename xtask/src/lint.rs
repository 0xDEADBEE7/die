//! `cargo xtask lint`: the workspace rules the compiler can't check alone
//! (scope §12 and D15-D17, docs/records/platforms.md §5.2).
//!
//! - `unsafe`: every crate inherits the workspace's `unsafe_code = "forbid"`
//!   except die-render, which denies it and allows it on `platform::macos` only.
//!   Nothing else lowers that level, and nothing allows `warnings` wholesale.
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
/// boundary and die-core's `sys` module.
const UNSAFE_ALLOWED: &[(&str, &str)] =
    &[("crates/die-render/src/platform/mod.rs", "pub mod macos")];

/// Crates whose code is platform-neutral, and the paths in each that may branch
/// on the platform (platforms.md §5.2): die-core's `sys` module (M2) and
/// die-render's platform modules. Unix is the floor, so `cfg(unix)` and
/// `std::os::unix` are fine everywhere.
const NEUTRAL: &[(&str, &[&str])] = &[
    ("crates/die-core", &["crates/die-core/src/sys/"]),
    ("crates/die-vim", &[]),
    ("tools/evil-oracle", &[]),
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
/// lint level changes no code, and neutral code that only `platform::macos`
/// calls is dead on Linux until `platform::linux` lands, which the
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
const OWN_LINTS: &[&str] = &["crates/die-render"];

pub fn run(root: &Path) -> Result {
    let mut errors = Vec::new();
    let ws: Table = parse(&root.join("Cargo.toml"))?;
    let members: Vec<String> = ws["workspace"]["members"]
        .as_array()
        .ok_or("workspace.members isn't a list")?
        .iter()
        .filter_map(|v| v.as_str().map(String::from))
        .collect();

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
            let level = lints
                .and_then(|l| l.get("rust"))
                .and_then(|r| r.get("unsafe_code"))
                .and_then(Value::as_str);
            if level != Some("deny") && level != Some("forbid") {
                errors.push(format!(
                    "{name}: must set [lints.rust] unsafe_code = \"deny\""
                ));
            }
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
            if rel.contains("/target/") {
                continue;
            }
            let src = read(&file)?;
            check_unsafe_allows(&rel, &src, &mut errors);
            if let Some((_, allowed)) = neutral
                && !allowed.iter().any(|a| rel.starts_with(a))
            {
                check_platform_code(&rel, &src, &mut errors);
            }
        }
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
             `platform::macos` calls"
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
