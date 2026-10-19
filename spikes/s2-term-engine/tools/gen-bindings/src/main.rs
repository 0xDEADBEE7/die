// The generator is libghostty-rs's crates/libghostty-vt-sys/tools/gen_bindings.rs at the pinned
// rev (MIT OR Apache-2.0), taken from cargo's git checkout. Set GEN_BINDINGS_RS to its path at
// build time; at run time set GHOSTTY_INCLUDE_DIR (the xcframework's Headers) and
// CARGO_MANIFEST_DIR (crates/die-term-sys), and it writes src/bindings.rs there.
include!(env!("GEN_BINDINGS_RS"));
