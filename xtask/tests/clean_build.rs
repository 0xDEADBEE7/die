//! F2: the clean-environment build fails when a build script calls a C compiler
//! or pkg-config.

use xtask::clean::{build, host};
use xtask::util::{TempDir, write};

/// A one-crate project whose build script runs `tool`, or none. A `$NAME` tool
/// is the program named by that environment variable, as the `cc` crate finds it.
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
        let program = match tool.strip_prefix('$') {
            Some(var) => format!("std::env::var({var:?}).expect(\"{var} is set\")"),
            None => format!("{tool:?}"),
        };
        let script = format!(
            "fn main() {{\n    let ok = std::process::Command::new({program}).arg(\"--version\").status().is_ok_and(|s| s.success());\n    assert!(ok, \"{tool} failed\");\n}}\n"
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

#[test]
fn a_build_script_calling_pkg_config_fails() {
    let p = project(Some("pkg-config"));
    let tmp = TempDir::new("planted-out").unwrap();
    let err = build(p.path(), tmp.path(), &host().unwrap(), &[]).unwrap_err();
    assert!(err.contains("clean-build: pkg-config was called"), "{err}");
    // The summary leads: which build script, and what it called.
    let lead = err.split("\n\n").next().unwrap();
    assert!(
        lead.contains("the build script of `planted v0.0.0")
            && lead.contains("clean-build: pkg-config was called: pkg-config --version"),
        "{lead}"
    );
}

#[test]
fn a_build_script_using_cc_cxx_or_ar_fails() {
    for (var, tool) in [("$CC", "cc"), ("$CXX", "c++"), ("$AR", "ar")] {
        let p = project(Some(var));
        let tmp = TempDir::new("planted-out").unwrap();
        let err = build(p.path(), tmp.path(), &host().unwrap(), &[]).unwrap_err();
        assert!(
            err.contains(&format!("clean-build: {tool} was called")),
            "{var}: {err}"
        );
    }
}
