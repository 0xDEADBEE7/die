//! Stand-in for die-render: compiles the S1 prototype's modules in place, so
//! the crate has realistic size and the binary links winit, Metal and Core Text.
#[path = "../../../../src/app.rs"] pub mod app;
#[path = "../../../../src/bench.rs"] pub mod bench;
#[path = "../../../../src/fidelity.rs"] pub mod fidelity;
#[path = "../../../../src/font.rs"] pub mod font;
#[path = "../../../../src/gpu.rs"] pub mod gpu;
#[path = "../../../../src/interactive.rs"] pub mod interactive;
#[path = "../../../../src/keys.rs"] pub mod keys;
#[path = "../../../../src/pixbuf.rs"] pub mod pixbuf;
#[path = "../../../../src/platform.rs"] pub mod platform;
#[path = "../../../../src/scene.rs"] pub mod scene;
#[path = "../../../../src/sprite.rs"] pub mod sprite;
#[path = "../../../../src/stats.rs"] pub mod stats;
pub use die_core::version;
// edit 4
pub fn s1_edit_4(x: usize) -> usize { x * 7 + 1 }
