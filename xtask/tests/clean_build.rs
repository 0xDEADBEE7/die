//! F2: the clean-environment build fails when a build script calls a C compiler
//! or pkg-config, even when the build script carries on without it.

use die_test_cap as _;

use xtask::clean::{build, host};
use xtask::util::{TempDir, write};

/// A one-crate project whose build script runs `tool`, or none, and fails when
/// it fails. A `$NAME` tool is the program named by that environment variable,
/// as the `cc` crate finds it.
fn project(tool: Option<&str>) -> TempDir {
    project_with(tool, "assert!(ok, \"{tool} failed\");")
}

/// `project`, with `then` run after the tool (`ok` says whether it succeeded).
fn project_with(tool: Option<&str>, then: &str) -> TempDir {
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
        let then = then.replace("{tool}", tool);
        let script = format!(
            "fn main() {{\n    let ok = std::process::Command::new({program}).arg(\"--version\").status().is_ok_and(|s| s.success());\n    {then}\n}}\n"
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
#[ignore = "slow: runs in ci --full"]
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
#[ignore = "slow: runs in ci --full"]
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

#[test]
#[ignore = "slow: runs in ci --full"]
fn a_build_script_that_carries_on_without_a_stand_in_fails() {
    // As x11-dl does with pkg-config, and as a crate that compiles a C shim only
    // when `cc` works would: on a machine with a real one, it compiles C.
    for tool in ["pkg-config", "cc"] {
        let p = project_with(Some(tool), "let _ = ok;");
        let tmp = TempDir::new("planted-out").unwrap();
        let err = build(p.path(), tmp.path(), &host().unwrap(), &[]).unwrap_err();
        assert!(
            err.contains(&format!(
                "clean-build: {tool} was called by planted's build script (once `{tool} --version`), which carried on"
            )),
            "{tool}: {err}"
        );
    }
}
