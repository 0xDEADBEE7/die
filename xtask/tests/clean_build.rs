//! F2: the clean-environment build fails when a build script calls a C compiler.

use xtask::clean::{build, host};
use xtask::util::{TempDir, write};

/// A one-crate project whose build script runs `tool`, or none.
fn project(tool: Option<&str>) -> TempDir {
    let dir = TempDir::new("planted").unwrap();
    let p = dir.path();
    write(
        &p.join("Cargo.toml"),
        "[package]\nname = \"planted\"\nversion = \"0.0.0\"\nedition = \"2024\"\n\n[workspace]\n",
    )
    .unwrap();
    write(
        &p.join("Cargo.lock"),
        "version = 4\n\n[[package]]\nname = \"planted\"\nversion = \"0.0.0\"\n",
    )
    .unwrap();
    write(&p.join("src/lib.rs"), "pub fn f() {}\n").unwrap();
    if let Some(tool) = tool {
        let script = format!(
            "fn main() {{\n    let ok = std::process::Command::new({tool:?}).arg(\"--version\").status().is_ok_and(|s| s.success());\n    assert!(ok, \"{tool} failed\");\n}}\n"
        );
        write(&p.join("build.rs"), &script).unwrap();
    }
    dir
}

#[test]
fn a_crate_without_c_builds() {
    let p = project(None);
    let tmp = TempDir::new("planted-out").unwrap();
    build(p.path(), tmp.path(), &host().unwrap(), &[]).unwrap();
}

#[test]
fn a_build_script_calling_a_c_compiler_fails() {
    for tool in ["cc", "clang", "zig"] {
        let p = project(Some(tool));
        let tmp = TempDir::new("planted-out").unwrap();
        let err = build(p.path(), tmp.path(), &host().unwrap(), &[]).unwrap_err();
        assert!(
            err.contains(&format!("clean-build: {tool} was called")),
            "{tool}: {err}"
        );
    }
}
