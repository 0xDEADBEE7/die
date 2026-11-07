//! The backend on a target that has none yet. It holds only the items neutral
//! code names, each failing the same way, until `platform::linux` and the portable
//! backend land (docs/records/platforms.md §5.2). `Gpu`, `Face` and `Surface` have no
//! values, so a method on one never runs, and there's nothing to get wrong in it.

/// No value of this exists.
enum Never {}

/// Whether DIE can draw here. It can't yet.
pub fn ready() -> Result<(), String> {
    Err("no window backend on this platform yet".into())
}

/// No system clipboard yet, so core keeps its own.
pub fn system_clipboard() -> Option<Box<dyn die_core::Clipboard>> {
    None
}

/// The window's side.
pub mod appkit {
    use super::{Never, metal::Pipeline};
    use winit::event_loop::EventLoopBuilder;
    use winit::window::{Window, WindowAttributes};

    /// Nothing to prepare.
    pub fn prepare_loop<T: 'static>(_: &mut EventLoopBuilder<T>, _: bool) -> Result<(), String> {
        Ok(())
    }

    /// Nothing to add.
    pub fn window_attributes(attributes: WindowAttributes) -> WindowAttributes {
        attributes
    }

    /// There's no way to yet.
    pub fn post_key(_: u16, _: char, _: bool) -> Result<(), String> {
        Err("no way to post a key on this platform yet".into())
    }

    /// There's no Option key.
    pub fn option_as_alt(_: &Window) -> bool {
        false
    }

    /// Holding a key repeats it.
    pub fn press_and_hold() -> bool {
        false
    }

    /// The usual double-click time.
    pub fn double_click_interval() -> std::time::Duration {
        std::time::Duration::from_millis(400)
    }

    /// A key monitor, which this target doesn't need: Caps Lock comes through winit.
    pub struct KeyMonitor(Never);

    impl KeyMonitor {
        /// There's none.
        pub fn install(_: impl Fn() + 'static) -> Result<KeyMonitor, String> {
            Err("no key monitor on this platform".into())
        }

        /// Never runs.
        pub fn take_caps(&self) -> Vec<bool> {
            match self.0 {}
        }

        /// Never runs.
        pub fn take_inserted(&self) -> Vec<String> {
            match self.0 {}
        }

        /// Never runs.
        pub fn caps(&self) -> bool {
            match self.0 {}
        }

        /// Never runs.
        pub fn panics(&self) -> u64 {
            match self.0 {}
        }

        /// Never runs.
        pub fn take_key_at(&self) -> Option<std::time::Instant> {
            match self.0 {}
        }

        /// Never runs.
        pub fn set_on_caps(&self, _: impl Fn() + 'static) {
            match self.0 {}
        }

        /// Never runs.
        pub fn deliver_to(&self, _: &Surface) {
            match self.0 {}
        }
    }

    /// A surface, which this target can't have.
    pub struct Surface(Never);

    impl Surface {
        /// There's none.
        pub fn attach(_: &Window, _: &Pipeline, _: bool) -> Result<Surface, String> {
            Err("no window backend on this platform yet".into())
        }

        /// Never runs.
        pub fn size(&self, _: f64) -> (u32, u32) {
            match self.0 {}
        }

        /// Never runs.
        pub fn resize(&self, _: (u32, u32), _: f64) {
            match self.0 {}
        }

        /// Never runs.
        pub fn insert_text(&self, _: &str) {
            match self.0 {}
        }

        /// Never runs.
        pub fn read_next_frame(&self) -> Result<(), String> {
            match self.0 {}
        }

        /// Never runs.
        pub fn watch_presents(&self) {
            match self.0 {}
        }

        /// Never runs.
        pub fn take_presented(&self) -> Vec<(u64, Option<std::time::Instant>)> {
            match self.0 {}
        }

        /// Never runs.
        pub fn occluded(&self) -> bool {
            match self.0 {}
        }

        /// Never runs.
        pub fn in_live_resize(&self) -> bool {
            match self.0 {}
        }

        /// Never runs.
        pub fn app_active(&self) -> bool {
            match self.0 {}
        }

        /// Never runs.
        pub fn come_to_front(&self) {
            match self.0 {}
        }
    }
}

/// The GPU's side.
pub mod metal {
    use super::{Never, appkit::Surface};
    use crate::frame::{Image, Presented, Quad, Upload};
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

        /// Never runs.
        pub fn present(
            &mut self,
            _: &[Quad],
            _: Rgb,
            _: &Surface,
            _: bool,
        ) -> Result<Option<Presented>, String> {
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
