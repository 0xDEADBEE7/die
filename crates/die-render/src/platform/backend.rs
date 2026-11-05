//! The backend on a target that has none yet. It holds only the items neutral
//! code names, each failing the same way, until `platform::linux` and the portable
//! backend land (docs/records/platforms.md §5.2). `Gpu` and `Face` have no values, so
//! a method on one never runs, and there's nothing to get wrong in it.

/// No value of this exists.
enum Never {}

/// Whether DIE can draw here. It can't yet.
pub fn ready() -> Result<(), String> {
    Err("no window backend on this platform yet".into())
}

/// The GPU's side.
pub mod metal {
    use super::Never;
    use crate::frame::{Image, Quad, Upload};
    use die_core::Rgb;

    /// A GPU, which this target can't have.
    pub struct Gpu(Never);

    impl Gpu {
        /// There's none.
        pub fn system_default() -> Option<Gpu> {
            None
        }

        /// Never runs.
        pub fn pipeline(self, _: u32, _: u32) -> Result<Pipeline, String> {
            match self.0 {}
        }
    }

    /// A pipeline, which this target can't have.
    pub struct Pipeline(Never);

    impl Pipeline {
        /// Never runs.
        pub fn upload(&self, _: &Upload) -> Result<(), String> {
            match self.0 {}
        }

        /// Never runs.
        pub fn render(&mut self, _: &[Quad], _: Rgb, _: u32, _: u32) -> Result<Image, String> {
            match self.0 {}
        }
    }
}

/// The text side.
pub mod text {
    use super::Never;
    use crate::font::{FaceMetrics, Fit, Ink};
    use crate::frame::GlyphBitmap;
    use std::path::Path;

    /// A face, which this target can't have.
    pub struct Face(Never);

    impl Face {
        /// There's none.
        pub fn from_file(_: &Path, _: f64) -> Result<Face, String> {
            Err("no window backend on this platform yet".into())
        }

        /// Never runs.
        pub fn metrics(&self) -> FaceMetrics {
            match self.0 {}
        }

        /// Never runs.
        pub fn glyph(&self, _: char) -> Option<u16> {
            match self.0 {}
        }

        /// Never runs.
        pub fn advance(&self, _: u16) -> f64 {
            match self.0 {}
        }

        /// Never runs.
        pub fn fallback_for(&self, _: &str) -> Face {
            match self.0 {}
        }

        /// Never runs.
        pub fn shape(&self, _: &str) -> Shaped {
            match self.0 {}
        }
    }

    /// Shaped text, which this target can't have.
    pub struct Shaped {
        never: Never,
        /// Unused.
        pub advance: f64,
        /// Unused.
        pub ink: Ink,
    }

    impl Shaped {
        /// Never runs.
        pub fn draw(&self, _: &Fit) -> Option<GlyphBitmap> {
            match self.never {}
        }
    }
}
