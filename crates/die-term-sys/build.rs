//! Link the vendored engine archive for the target being built, after checking it against its
//! pinned SHA-256. A target with no archive (Linux until DIE's CI builds one, D10) links nothing,
//! so the workspace still builds and checks there, and only a binary that calls the engine fails
//! to link. No network, git or Zig (D16); `cargo xtask ghostty verify` checks the rest of the lock.
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use std::{env, fs};

fn main() {
    let target = env::var("TARGET").expect("cargo sets TARGET");
    let manifest = env::var("CARGO_MANIFEST_DIR").expect("cargo sets CARGO_MANIFEST_DIR");
    let dir = PathBuf::from(manifest).join("vendor").join(&target);
    println!("cargo::rerun-if-changed=vendor");
    let archive = dir.join("libghostty-vt.a");
    if !archive.exists() {
        return;
    }
    let lib = fs::read(&archive).expect("read the vendored archive");
    let want = fs::read_to_string(dir.join("libghostty-vt.a.sha256"))
        .expect("the vendored archive has no .sha256 beside it");
    let got = format!("{:x}", Sha256::digest(&lib));
    assert_eq!(
        got,
        want.trim(),
        "vendor/{target}/libghostty-vt.a doesn't match its pinned SHA-256"
    );
    println!("cargo::rustc-link-search=native={}", dir.display());
    println!("cargo::rustc-link-lib=static=ghostty-vt");
}
