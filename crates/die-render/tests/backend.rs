//! Neutral code reaches the platform through `platform::backend`, on every
//! target: this file carries no platform cfg, and `cargo xtask cross-check` builds
//! it for Linux against the stub.

use die_render::platform::backend;

#[test]
fn the_backend_says_whether_it_can_draw() {
    if let Err(e) = backend::ready() {
        assert_eq!(
            e, "no window backend on this platform yet",
            "a Mac has Metal, and elsewhere the stub says why not"
        );
    }
}
