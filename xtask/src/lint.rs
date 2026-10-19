//! `cargo xtask lint`: the workspace rules the compiler can't check alone
//! (scope §12 and D15-D17).
//!
//! - `unsafe`: every crate inherits the workspace's `unsafe_code = "forbid"`
//!   except die-render, which denies it and allows it on `platform::macos` only.
//! - Every dependency is pinned to an exact version.
//! - die-vim depends on die-core only (D15).
//! - The lockfile and both license files are there.

use crate::util::{Result, git, output, read, rust_files};
use std::path::Path;
use toml::{Table, Value};

/// The only places `allow(unsafe_code)` may appear: (file, the item it must sit on).
/// `platform::linux` joins with the Linux backend; M2 adds die-term's engine
/// boundary and die-core's `sys` module.
const UNSAFE_ALLOWED: &[(&str, &str)] =
    &[("crates/die-render/src/platform/mod.rs", "pub mod macos")];

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

    // D15: die-vim depends on die-core only.
    let vim = parse(&root.join("crates/die-vim/Cargo.toml"))?;
    if let Some(deps) = vim.get("dependencies").and_then(Value::as_table) {
        for k in deps.keys().filter(|k| *k != "die-core") {
            errors.push(format!("crates/die-vim/Cargo.toml: depends on {k}; die-vim may depend on die-core only (D15)"));
        }
    }

    for m in &members {
        for file in rust_files(&root.join(m)) {
            let rel = file
                .strip_prefix(root)
                .unwrap_or(&file)
                .to_string_lossy()
                .into_owned();
            if rel.contains("/target/") {
                continue;
            }
            check_unsafe_allows(&rel, &read(&file)?, &mut errors);
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
