//! Spike S2 step A1: verify Ghostty's signed tip asset in pure Rust (minisign-verify),
//! then print the SHA-256 of the signed zip and of the derived archive.
//!
//! usage: s2-verify <zip> <zip.minisig> [derived libghostty-vt.a]
use minisign_verify::{PublicKey, Signature};
use sha2::{Digest, Sha256};

/// Ghostty release key (Ghostty PACKAGING.md:30).
const GHOSTTY_PK: &str = "RWQlAjJC23149WL2sEpT/l0QKy7hMIFhYdQOFy0Z7z7PbneUgvlsnYcV";

fn main() {
    let a: Vec<String> = std::env::args().collect();
    if a.len() < 3 {
        eprintln!("usage: s2-verify <zip> <zip.minisig> [derived.a]");
        std::process::exit(2);
    }
    let data = std::fs::read(&a[1]).expect("read zip");
    let sig_txt = std::fs::read_to_string(&a[2]).expect("read minisig");
    let pk = PublicKey::from_base64(GHOSTTY_PK).expect("public key");
    let sig = Signature::decode(&sig_txt).expect("signature");
    let ok = pk.verify(&data, &sig, false);
    println!("signed_zip_sha256={:x}", Sha256::digest(&data));
    println!("signed_zip_bytes={}", data.len());
    println!("trusted_comment={}", sig.trusted_comment());
    println!("minisign_verify={}", if ok.is_ok() { "OK" } else { "FAIL" });
    // Negative control: one flipped byte must fail.
    let mut bad = data.clone();
    let mid = bad.len() / 2;
    bad[mid] ^= 1;
    println!(
        "tamper_control={}",
        if pk.verify(&bad, &sig, false).is_err() { "rejected (good)" } else { "ACCEPTED (bad)" }
    );
    if let Some(p) = a.get(3) {
        let lib = std::fs::read(p).expect("read derived archive");
        println!("derived_archive_sha256={:x}", Sha256::digest(&lib));
        println!("derived_archive_bytes={}", lib.len());
    }
    if let Err(e) = ok {
        eprintln!("signature error: {e}");
        std::process::exit(1);
    }
}
