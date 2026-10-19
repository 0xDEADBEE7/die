//! S2 A3: DIE's PNG hook is process-global, and no libghostty-rs code path installs a
//! thread-local decoder in this configuration (wrapper issue #104).
use s2_term_engine::{dterm::kitty_chunked, dterm::png_rgba, hook};
use libghostty_vt::Terminal;
use std::sync::atomic::Ordering::Relaxed;

#[test]
fn hook_installed_once_serves_every_thread() {
    hook::install();
    let before = hook::DECODES.load(Relaxed);
    let hs: Vec<_> = (0..8u8)
        .map(|i| {
            std::thread::spawn(move || {
                // No install call on this thread.
                let mut t = Terminal::new(80, 24).unwrap();
                t.set_kitty_image_storage_limit(4 << 20).unwrap();
                let png = png_rgba(32, 16, |x, y| [x as u8, y as u8, i, 255]);
                for ch in kitty_chunked(&format!("q=2,a=t,f=100,i={}", i + 1), &png) {
                    t.vt_write(&ch);
                }
                let g = t.kitty_graphics().unwrap();
                let img = g.image(i as u32 + 1).expect("image stored");
                let d = img.data().unwrap().unwrap();
                (img.width().unwrap(), img.height().unwrap(), d[2])
            })
        })
        .collect();
    for (i, h) in hs.into_iter().enumerate() {
        assert_eq!(h.join().unwrap(), (32, 16, i as u8));
    }
    assert!(hook::DECODES.load(Relaxed) >= before + 8);
}

/// Locate a package's source directory through `cargo metadata` (offline).
fn metadata() -> String {
    let out = std::process::Command::new(env!("CARGO"))
        .args(["metadata", "--format-version", "1", "--offline", "--manifest-path"])
        .arg(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml"))
        .output()
        .expect("cargo metadata");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8(out.stdout).unwrap()
}

fn wrapper_src(meta: &str) -> std::path::PathBuf {
    // The wrapper's package entry: its manifest_path ends in crates/libghostty-vt/Cargo.toml.
    let key = "crates/libghostty-vt/Cargo.toml\"";
    let end = meta.find(key).expect("wrapper manifest in metadata") + key.len() - 1;
    let start = meta[..end].rfind('"').unwrap() + 1;
    std::path::Path::new(&meta[start..end]).parent().unwrap().join("src")
}

fn rs_files(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    for e in std::fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() {
            rs_files(&p, out);
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push(p);
        }
    }
}

#[test]
fn wrapper_source_has_no_implicit_thread_local_decoder() {
    let meta = metadata();
    let src = wrapper_src(&meta);
    let mut files = Vec::new();
    rs_files(&src, &mut files);
    assert!(files.len() > 10, "found the wrapper sources at {}", src.display());
    let mut decode_png_sets = Vec::new();
    let mut set_png_decoder_calls = Vec::new();
    let mut thread_locals = Vec::new();
    for f in &files {
        let text = std::fs::read_to_string(f).unwrap();
        let rel = f.strip_prefix(&src).unwrap().display().to_string();
        let mut in_tests = false;
        for (n, line) in text.lines().enumerate() {
            let t = line.trim_start();
            if t.starts_with("#[cfg(test)]") {
                in_tests = true; // test modules sit at the end of files in this crate
            }
            if t.starts_with("//") || in_tests {
                continue;
            }
            if t.contains("SysOption::DECODE_PNG") {
                decode_png_sets.push(format!("{rel}:{}", n + 1));
            }
            if t.contains("set_png_decoder(") && !t.contains("pub fn set_png_decoder") {
                set_png_decoder_calls.push(format!("{rel}:{}", n + 1));
            }
            if t.contains("thread_local!") {
                thread_locals.push(format!("{rel}:{}", n + 1));
            }
        }
    }
    // Exactly one place sets the engine's PNG callback: inside `pub fn set_png_decoder`.
    assert_eq!(decode_png_sets.len(), 1, "DECODE_PNG set at {decode_png_sets:?}");
    assert!(decode_png_sets[0].starts_with("kitty/graphics.rs"), "{decode_png_sets:?}");
    // Nothing in the wrapper's library code calls it (doc examples and tests excluded).
    assert!(set_png_decoder_calls.is_empty(), "wrapper calls set_png_decoder at {set_png_decoder_calls:?}");
    // The only thread_local is the decoder slot itself.
    assert_eq!(thread_locals.len(), 1, "{thread_locals:?}");
    println!("DECODE_PNG set only at {decode_png_sets:?}; thread_local only at {thread_locals:?}; no internal calls");
}

#[test]
fn resolved_features_are_kitty_graphics_only() {
    let meta = metadata();
    // In the resolve graph, the wrapper's node lists its enabled features.
    let id_key = "\"id\":\"git+https://github.com/uzaaft/libghostty-rs?rev=d2036baeb605d31d09a9e2ba6d0c923e04c34d54#libghostty-vt@0.2.1\"";
    let resolve = &meta[meta.find("\"resolve\"").expect("resolve")..];
    let node = &resolve[resolve.find(id_key).expect("wrapper node")..];
    let feats = &node[node.find("\"features\":[").unwrap() + 12..];
    let feats = &feats[..feats.find(']').unwrap()];
    assert_eq!(feats, "\"kitty-graphics\"", "wrapper features: [{feats}]");
    // And the sys crate in the graph is DIE's path crate, not the git one.
    assert!(meta.contains("crates/die-term-sys/Cargo.toml"));
    assert!(!meta.contains("libghostty-rs?rev=d2036baeb605d31d09a9e2ba6d0c923e04c34d54#libghostty-vt-sys"));
}
