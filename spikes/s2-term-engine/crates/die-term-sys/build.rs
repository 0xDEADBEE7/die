// die-term-sys build script: check the vendored archive's SHA-256, then link it.
use sha2::{Digest, Sha256};
use std::{env, fs, path::PathBuf};

fn main() {
    let dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap()).join("vendor");
    let lib = fs::read(dir.join("libghostty-vt.a")).expect("vendor/libghostty-vt.a is missing");
    let want = fs::read_to_string(dir.join("libghostty-vt.a.sha256")).unwrap();
    let got = format!("{:x}", Sha256::digest(&lib));
    assert_eq!(got, want.trim(), "vendor/libghostty-vt.a does not match its pinned SHA-256");
    println!("cargo:rerun-if-changed=vendor");
    println!("cargo:rustc-link-search=native={}", dir.display());
    println!("cargo:rustc-link-lib=static=ghostty-vt");
}
