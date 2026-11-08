//! `cargo xtask ghostty verify`: die-term-sys's vendored engine archives against their lock
//! (scope D10, D16 and D17; docs/records/die-term-engine.md §5).
//!
//! D16 lets a prebuilt binary into the build only when its upstream's CI signed it, it's tied
//! to a commit whose tests passed, and it's checked by checksum. `ghostty bump` checks the
//! signature and the commit, and writes the lock; this checks what's committed against the
//! lock, in `ci` and in `clean-build`, which would otherwise take any archive on trust:
//!
//! - each `vendor/<target>/` has a section of the lock, and each section its archive;
//! - an archive's SHA-256 and size are the lock's, as is its `.sha256`, which build.rs checks;
//! - its global symbols are the engine's own (`_ghostty_*`), as many as the lock says, so none
//!   of its bundled libraries' can collide with another's;
//! - the symbols it needs are exactly the lock's list, and an Apple target's link against
//!   libSystem alone, so the engine brings no other library;
//! - the generated bindings are the lock's, by hash: the lint lets them, alone, allow the
//!   unsafe-block lints (`lint::GENERATED`);
//! - the signature's trusted comment is the lock's, and the wrapper's rev is the workspace's.
//!
//! `ghostty bump` (download, minisign, derive, regenerate, commit) is still to be built: the
//! spike's `tools/verify`, `tools/gen-bindings` and `scripts/derive-archive.sh` do its steps.

use crate::util::{Result, TempDir, output, read};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::path::Path;
use std::process::Command;
use toml::{Table, Value};

/// die-term-sys, from the workspace's root.
pub const SYS: &str = "crates/die-term-sys";
const LOCK: &str = "ghostty-vt.lock";
/// The engine's wrapper, libghostty-rs: the one crate DIE takes from git (scope D10).
pub const WRAPPER: &str = "https://github.com/uzaaft/libghostty-rs";
/// The generated bindings, from die-term-sys: the one generated file, which verify holds to
/// the lock's hash and the budget counts apart.
pub const BINDINGS: &str = "src/ffi/bindings.rs";
/// The crate that calls the engine. Where die-term-sys has no archive for the target, its
/// library builds but nothing of it links: its tests can't build there (D10).
pub const ENGINE_USER: &str = "die-term";

/// die-term-sys has an engine archive for `target`, so binaries that call the engine link.
pub fn links(root: &Path, target: &str) -> bool {
    root.join(SYS)
        .join("vendor")
        .join(target)
        .join("libghostty-vt.a")
        .is_file()
}

pub fn run(root: &Path, args: &[&str]) -> Result {
    match args {
        ["verify"] => verify(root),
        ["bump", ..] => Err(
            "ghostty bump: not built yet; docs/records/die-term-engine.md §5 \
                             has its steps, and the spike's tools do them by hand"
                .into(),
        ),
        _ => Err("usage: cargo xtask ghostty verify".into()),
    }
}

/// Check every vendored archive, the bindings and the pins against the lock.
pub fn verify(root: &Path) -> Result {
    let sys = root.join(SYS);
    let lock: Table = read(&sys.join(LOCK))?
        .parse()
        .map_err(|e| format!("ghostty verify: {LOCK}: {e}"))?;
    let mut errors = Vec::new();
    let targets = lock.get("target").and_then(Value::as_table);
    let vendored = vendored_targets(&sys)?;
    for t in &vendored {
        if !targets.is_some_and(|ts| ts.contains_key(t)) {
            errors.push(format!("vendor/{t}/ has no [target.{t}] in the lock"));
        }
    }
    let mut checked = Vec::new();
    for (t, section) in targets.into_iter().flatten() {
        if !vendored.contains(t) {
            errors.push(format!("[target.{t}] names no vendor/{t}/ archive"));
            continue;
        }
        let Some(section) = section.as_table() else {
            errors.push(format!("[target.{t}] isn't a table"));
            continue;
        };
        let before = errors.len();
        check_archive(&sys, t, section, &mut errors)?;
        if errors.len() == before {
            checked.push(t.clone());
        }
    }
    check_bindings(&sys, &lock, &mut errors)?;
    check_pins(root, &sys, &lock, &mut errors)?;
    if errors.is_empty() {
        println!(
            "ghostty verify: ok ({} archive{} at engine {}: hashes, symbols, libSystem only; \
             bindings; pins)",
            checked.join(", "),
            if checked.len() == 1 { "" } else { "s" },
            str_at(&lock, &["engine", "commit"])
                .get(..12)
                .unwrap_or("?"),
        );
        Ok(())
    } else {
        Err(format!(
            "ghostty verify ({SYS}/{LOCK}):\n  {}",
            errors.join("\n  ")
        ))
    }
}

/// The targets with a directory under `vendor/`.
fn vendored_targets(sys: &Path) -> Result<BTreeSet<String>> {
    let dir = sys.join("vendor");
    let entries = std::fs::read_dir(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    Ok(entries
        .flatten()
        .filter(|e| e.path().is_dir())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect())
}

fn str_at<'a>(t: &'a Table, path: &[&str]) -> &'a str {
    let mut v: Option<&Value> = None;
    let mut table = Some(t);
    for k in path {
        v = table.and_then(|t| t.get(*k));
        table = v.and_then(Value::as_table);
    }
    v.and_then(Value::as_str).unwrap_or("")
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn check_archive(sys: &Path, target: &str, lock: &Table, errors: &mut Vec<String>) -> Result {
    let rel = format!("vendor/{target}/libghostty-vt.a");
    let path = sys.join(&rel);
    let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let want = lock
        .get("archive_sha256")
        .and_then(Value::as_str)
        .unwrap_or("");
    let got = sha256_hex(&bytes);
    if got != want {
        errors.push(format!("{rel}: SHA-256 {got}, the lock's is {want}"));
    }
    let size = lock.get("archive_bytes").and_then(Value::as_integer);
    if size != i64::try_from(bytes.len()).ok() {
        errors.push(format!(
            "{rel}: {} bytes, the lock's is {size:?}",
            bytes.len()
        ));
    }
    let side = read(&sys.join(format!("{rel}.sha256")))?;
    if side.trim() != want {
        errors.push(format!("{rel}.sha256 isn't the lock's archive_sha256"));
    }
    if lock.get("archive").and_then(Value::as_str) != Some(rel.as_str()) {
        errors.push(format!("[target.{target}] archive isn't {rel:?}"));
    }
    // Symbols, by the Command Line Tools' `nm` and `ld`: a Linux host's binutils may not read
    // Mach-O, so there the hashes stand for them (macOS's `ci` checks them).
    if target.ends_with("-apple-darwin") && !cfg!(target_os = "macos") {
        return Ok(());
    }
    let globals = output(Command::new("nm").args(["-gU"]).arg(&path))?;
    let globals = symbols(&globals, true);
    errors.extend(
        check_globals(&globals, lock)
            .into_iter()
            .map(|e| format!("{rel}: {e}")),
    );
    let undefined = output(Command::new("nm").arg("-u").arg(&path))?;
    let undefined = symbols(&undefined, false);
    let listed: BTreeSet<String> = lock
        .get("undefined")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(String::from)
                .collect()
        })
        .unwrap_or_default();
    errors.extend(
        check_undefined(&undefined, &listed)
            .into_iter()
            .map(|e| format!("{rel}: {e}")),
    );
    if target.ends_with("-apple-darwin")
        && errors.is_empty()
        && let Err(e) = links_libsystem_only(&path, target)
    {
        errors.push(format!("{rel}: {e}"));
    }
    Ok(())
}

/// The symbol names in `nm -gU` (`defined`) or `nm -u` output, each once.
pub fn symbols(nm: &str, defined: bool) -> BTreeSet<String> {
    nm.lines()
        .filter(|l| !l.is_empty() && !l.ends_with(':'))
        .filter_map(|l| {
            let f: Vec<&str> = l.split_whitespace().collect();
            match (defined, f.as_slice()) {
                (true, [_, _, name]) | (false, [name]) => Some(name.to_string()),
                _ => None,
            }
        })
        .collect()
}

/// Global symbols must be the engine's own, as many as the lock says.
pub fn check_globals(globals: &BTreeSet<String>, lock: &Table) -> Vec<String> {
    let mut errors = Vec::new();
    let others: Vec<&str> = globals
        .iter()
        .map(String::as_str)
        .filter(|s| !s.starts_with("_ghostty_"))
        .collect();
    if !others.is_empty() {
        errors.push(format!(
            "globals other than `_ghostty_*`, which could collide with another library's: {}",
            others.join(", ")
        ));
    }
    let want = lock.get("global_symbols").and_then(Value::as_integer);
    if i64::try_from(globals.len()).ok() != want {
        errors.push(format!(
            "{} global symbols, the lock's is {want:?}",
            globals.len()
        ));
    }
    errors
}

/// The symbols the archive needs must be the lock's list, no more and no fewer.
pub fn check_undefined(undefined: &BTreeSet<String>, listed: &BTreeSet<String>) -> Vec<String> {
    let mut errors = Vec::new();
    let new: Vec<&str> = undefined.difference(listed).map(String::as_str).collect();
    let gone: Vec<&str> = listed.difference(undefined).map(String::as_str).collect();
    if !new.is_empty() {
        errors.push(format!(
            "needs symbols the lock doesn't list: {}",
            new.join(", ")
        ));
    }
    if !gone.is_empty() {
        errors.push(format!(
            "the lock lists symbols it no longer needs: {}",
            gone.join(", ")
        ));
    }
    errors
}

/// Link the archive whole into a throwaway dylib against libSystem alone: every symbol it
/// needs must come from there, and the result may load nothing else.
fn links_libsystem_only(archive: &Path, target: &str) -> Result {
    let arch = target
        .split('-')
        .next()
        .unwrap_or("arm64")
        .replace("aarch64", "arm64");
    let sdk = output(Command::new("xcrun").args(["--show-sdk-path"]))?;
    let tmp = TempDir::new("ghostty-verify")?;
    let out = tmp.path().join("engine.dylib");
    output(
        Command::new("ld")
            .args([
                "-dylib",
                "-arch",
                &arch,
                "-platform_version",
                "macos",
                "13.0",
                "13.0",
            ])
            .args(["-syslibroot", sdk.trim(), "-all_load"])
            .arg(archive)
            .args(["-lSystem", "-o"])
            .arg(&out),
    )
    .map_err(|e| format!("doesn't link against libSystem alone:\n{e}"))?;
    let libs = output(Command::new("otool").arg("-L").arg(&out))?;
    let others: Vec<&str> = libs
        .lines()
        .skip(2)
        .map(str::trim)
        .filter(|l| !l.starts_with("/usr/lib/libSystem.B.dylib"))
        .collect();
    if others.is_empty() {
        Ok(())
    } else {
        Err(format!("links more than libSystem: {}", others.join("; ")))
    }
}

fn check_bindings(sys: &Path, lock: &Table, errors: &mut Vec<String>) -> Result {
    let file = str_at(lock, &["bindings", "file"]);
    let want = str_at(lock, &["bindings", "sha256"]);
    if file != BINDINGS {
        errors.push(format!("[bindings] file is {file:?}, not {BINDINGS}"));
        return Ok(());
    }
    let got = sha256_hex(&std::fs::read(sys.join(file)).map_err(|e| format!("{file}: {e}"))?);
    if got != want {
        errors.push(format!(
            "{file} isn't the generated file the lock names (SHA-256 {got}): only `ghostty \
             bump` writes it, and the lint lets it alone skip the SAFETY comments"
        ));
    }
    Ok(())
}

fn check_pins(root: &Path, sys: &Path, lock: &Table, errors: &mut Vec<String>) -> Result {
    let minisig = read(&sys.join(str_at(lock, &["engine", "signature"])))?;
    let trusted = minisig
        .lines()
        .find_map(|l| l.strip_prefix("trusted comment: "))
        .unwrap_or("");
    if trusted != str_at(lock, &["engine", "trusted_comment"]) {
        errors.push("the signature's trusted comment isn't the lock's".into());
    }
    let ws: Table = read(&root.join("Cargo.toml"))?
        .parse()
        .map_err(|e| format!("Cargo.toml: {e}"))?;
    let dep = ws
        .get("workspace")
        .and_then(|w| w.get("dependencies"))
        .and_then(|d| d.get("libghostty-vt"));
    let rev = dep.and_then(|d| d.get("rev")).and_then(Value::as_str);
    if rev != Some(str_at(lock, &["wrapper", "rev"])) {
        errors.push(format!(
            "the workspace's libghostty-vt is at rev {rev:?}, the lock's wrapper at {:?}",
            str_at(lock, &["wrapper", "rev"])
        ));
    }
    let patched = ws
        .get("patch")
        .and_then(|p| p.get(WRAPPER))
        .and_then(|p| p.get("libghostty-vt-sys"))
        .and_then(|s| s.get("path"))
        .and_then(Value::as_str);
    if patched != Some(SYS) {
        errors.push(format!(
            "the workspace's [patch.\"{WRAPPER}\"] doesn't point libghostty-vt-sys at {SYS}, so \
             the wrapper's own sys crate would build the engine with Zig (D16)"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn symbols_from_nm() {
        let gu = "\nghostty-vt-prelinked.o:\n000000000004131c T _ghostty_alloc\n\
                  0000000000041ac0 T _ghostty_build_info\n";
        let u = "\nghostty-vt-prelinked.o:\n___error\n_bzero\n_bzero\n";
        assert_eq!(
            symbols(gu, true).into_iter().collect::<Vec<_>>(),
            ["_ghostty_alloc", "_ghostty_build_info"]
        );
        assert_eq!(
            symbols(u, false).into_iter().collect::<Vec<_>>(),
            ["___error", "_bzero"]
        );
    }

    #[test]
    fn globals_are_the_engines_own() {
        let lock: Table = "global_symbols = 2".parse().unwrap();
        let set = |s: &[&str]| s.iter().map(|x| x.to_string()).collect::<BTreeSet<_>>();
        assert!(check_globals(&set(&["_ghostty_a", "_ghostty_b"]), &lock).is_empty());
        let e = check_globals(&set(&["_ghostty_a", "_simdutf_x"]), &lock);
        assert_eq!(e.len(), 1);
        assert!(e[0].contains("_simdutf_x"), "{e:?}");
        assert_eq!(
            check_globals(&set(&["_ghostty_a"]), &lock).len(),
            1,
            "a count off"
        );
    }

    #[test]
    fn needed_symbols_are_the_locks_list() {
        let set = |s: &[&str]| s.iter().map(|x| x.to_string()).collect::<BTreeSet<_>>();
        let listed = set(&["_free", "_malloc"]);
        assert!(check_undefined(&set(&["_free", "_malloc"]), &listed).is_empty());
        let e = check_undefined(&set(&["_free", "_malloc", "_dlopen"]), &listed);
        assert!(e.len() == 1 && e[0].contains("_dlopen"), "{e:?}");
        let e = check_undefined(&set(&["_free"]), &listed);
        assert!(
            e.len() == 1 && e[0].contains("no longer needs: _malloc"),
            "{e:?}"
        );
    }

    #[test]
    fn the_engine_links_where_an_archive_is_vendored() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        assert!(links(root, "aarch64-apple-darwin"));
        assert!(
            !links(root, "aarch64-unknown-linux-gnu"),
            "until DIE's CI builds one"
        );
        assert!(!links(root, "x86_64-unknown-linux-gnu"));
    }

    #[test]
    fn the_committed_lock_is_whole() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let lock: Table = read(&root.join(SYS).join(LOCK)).unwrap().parse().unwrap();
        let t = lock["target"]["aarch64-apple-darwin"].as_table().unwrap();
        let undefined = t["undefined"].as_array().unwrap();
        assert_eq!(undefined.len(), 39, "S2's count");
        assert_eq!(t["global_symbols"].as_integer(), Some(205));
        assert_eq!(str_at(&lock, &["wrapper", "rev"]).len(), 40);
    }
}
