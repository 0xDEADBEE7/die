//! The algorithms: layouts for graphs, downsampling and envelopes for long series, fig's search
//! score, and the hit index the pointer asks.

pub mod hit;
pub mod layered;
pub mod lttb;
pub mod pyramid;
pub mod search;
pub mod stress;
pub mod tree;

pub use hit::HitIndex;
pub use layered::{Routed, layered, layered_routes};
pub use lttb::lttb;
pub use pyramid::Pyramid;
pub use search::tier;
pub use stress::stress;
pub use tree::tree;
