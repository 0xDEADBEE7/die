//! `cargo xtask lint`: the workspace rules the compiler can't check alone
//! (scope §12 and D15-D17, docs/records/platforms.md §5.2).
//!
//! - `unsafe`: every crate inherits the workspace's `unsafe_code = "forbid"`
//!   except die-render, which denies it and allows it on `platform::macos` only.
//! - Neutral code doesn't branch on the platform: no `target_os` and the like,
//!   and no `std::os` module but Unix's, outside the named platform modules.
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

/// Neutral code doesn't branch on the platform: no `PLATFORM_CFGS` key in a
/// `cfg`, `cfg!` or `cfg_attr`, and no `std::os` (or `core::os`) module but
/// `UNIX_OS`. Strings and comments don't count: this reads tokens.
pub fn check_platform_code(rel: &str, src: &str, errors: &mut Vec<String>) {
    use crate::count::{Tok, lex, skip_group};
    let toks = lex(src);
    let ident = |i: usize| match toks.get(i).map(|t| &t.0) {
        Some(Tok::Ident(w)) => Some(w.as_str()),
        _ => None,
    };
    let punct = |i: usize, c: char| toks.get(i).map(|t| &t.0) == Some(&Tok::Punct(c));
    let path_sep = |i: usize| punct(i, ':') && punct(i + 1, ':');
    // The first identifier of each item in the `{...}` group opening at `open`.
    let firsts = |open: usize| -> Vec<usize> {
        let end = skip_group(&toks, open).unwrap_or(toks.len());
        let mut depth = 0;
        let mut out = Vec::new();
        for (j, t) in toks.iter().enumerate().take(end).skip(open) {
            match &t.0 {
                Tok::Punct('{' | '(' | '[') => depth += 1,
                Tok::Punct('}' | ')' | ']') => depth -= 1,
                Tok::Ident(_) if depth == 1 && (punct(j - 1, '{') || punct(j - 1, ',')) => {
                    out.push(j)
                }
                _ => {}
            }
        }
        out
    };
    let mut found: Vec<(usize, String)> = Vec::new();
    for i in 0..toks.len() {
        let word = ident(i);
        if matches!(word, Some("cfg" | "cfg_attr")) {
            let open = if punct(i + 1, '!') { i + 2 } else { i + 1 };
            if !punct(open, '(') {
                continue;
            }
            let end = skip_group(&toks, open).unwrap_or(toks.len());
            for t in &toks[open..end] {
                if let Tok::Ident(w) = &t.0
                    && PLATFORM_CFGS.contains(&w.as_str())
                {
                    found.push((t.1, format!("cfg on `{w}`")));
                }
            }
        }
        if matches!(word, Some("std" | "core")) && path_sep(i + 1) {
            // `std::os::...`, or `os::...` inside `std::{...}`.
            let os_at: Vec<usize> = if punct(i + 3, '{') {
                firsts(i + 3)
            } else {
                vec![i + 3]
            };
            for o in os_at {
                if ident(o) != Some("os") || !path_sep(o + 1) {
                    continue;
                }
                let modules = if punct(o + 3, '{') {
                    firsts(o + 3)
                } else {
                    vec![o + 3]
                };
                for m in modules {
                    if let Some(w) = ident(m)
                        && !UNIX_OS.contains(&w)
                    {
                        found.push((toks[m].1, format!("`std::os::{w}`")));
                    }
                }
            }
        }
    }
    for (line, what) in found {
        errors.push(format!(
            "{rel}:{}: {what} branches on the platform. Neutral code may use `cfg(unix)` and \
             `std::os::unix` only; the rest goes in die-core's `sys` or die-render's `platform` \
             (platforms.md §5.2)",
            line + 1
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

/// `allow(unsafe_code)` (or `expect`) may appear only where `UNSAFE_ALLOWED` says.
/// Strings and comments don't count: this reads tokens.
pub fn check_unsafe_allows(rel: &str, src: &str, errors: &mut Vec<String>) {
    use crate::count::{Tok, lex, skip_group};
    let toks = lex(src);
    for i in 0..toks.len() {
        let is_lint = matches!(&toks[i].0, Tok::Ident(w) if w == "allow" || w == "expect");
        if !is_lint || toks.get(i + 1).map(|t| &t.0) != Some(&Tok::Punct('(')) {
            continue;
        }
        let Some(end) = skip_group(&toks, i + 1) else {
            continue;
        };
        if !toks[i + 2..end]
            .iter()
            .any(|t| t.0 == Tok::Ident("unsafe_code".into()))
        {
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
                "{rel}:{}: allow(unsafe_code) outside the modules scope §12 permits ({})",
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
            "#[cfg_attr(target_vendor = \"apple\", allow(dead_code))]\nfn f() {}\n",
            "#[cfg(target_family = \"unix\")]\nfn f() {}\n",
            "#[cfg(target_env = \"musl\")]\nfn f() {}\n",
            "fn f() -> bool { cfg!(target_os = \"macos\") }\n",
            "use std::os::macos::fs::MetadataExt;\n",
            "use std::os::{fd::AsRawFd, linux::fs::MetadataExt};\n",
            "use std::{io, os::freebsd::fs::MetadataExt};\n",
            "fn f() { let _ = core::os::windows::raw::HANDLE::default(); }\n",
        ];
        for src in flagged {
            assert_eq!(check(src).len(), 1, "{src}");
        }
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
        ];
        for src in fine {
            assert_eq!(check(src), Vec::<String>::new(), "{src}");
        }
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
