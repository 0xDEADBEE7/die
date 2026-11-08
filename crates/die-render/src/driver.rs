//! The frame driver: the one piece of code that calls `platform::backend`
//! (docs/records/platforms.md §5.2). It asks the faces for what `rows::Frames` wants,
//! with `font` deciding where each glyph goes, and hands the frame's data to the GPU.
//!
//! A frame goes to a texture, read back, or to a window's drawable (`Renderer::present`).
//! The loop itself is `window`, which reaches the platform's window through what's
//! re-exported here.

use crate::atlas::GlyphKey;
use crate::font::{self, CellMetrics, RangeRule};
use crate::font_dir;
use crate::frame::{GlyphBitmap, Image, Presented};
use crate::platform::backend::{
    self,
    metal::{Gpu, Pipeline},
    text::Face,
};
use crate::rows::Frames;
use crate::sprite;
use die_core::Scene;
use std::path::{Path, PathBuf};
use std::time::Instant;
use winit::window::Window;

pub use crate::platform::backend::appkit::{
    KeyMonitor, Surface, double_click_interval, on_open_files, option_as_alt, post_key,
    prepare_loop, press_and_hold, process_wakeups, unseen, window_attributes,
};
pub use crate::platform::backend::{close_on_exec_inherited, ready, system_clipboard};

/// The gray atlas's side, in pixels: about 2,500 glyphs of a 19×40 cell.
const GRAY_ATLAS: u32 = 2048;
/// The color atlas's side, in pixels: about 600 two-cell emoji.
const COLOR_ATLAS: u32 = 1024;
/// How many faces the primary family has: regular, bold, italic, bold italic.
const PRIMARY_FACES: usize = 4;

/// What a renderer draws with.
#[derive(Clone, PartialEq, Debug)]
pub struct Config {
    /// The primary family's files: regular, bold, italic, bold italic.
    pub faces: [PathBuf; PRIMARY_FACES],
    /// The font's size in points.
    pub points: f64,
    /// Physical pixels per point.
    pub scale: f64,
    /// Who draws which characters. See `RangeRule`.
    pub rules: Vec<RangeRule>,
}

impl Config {
    /// Ioskeley Mono Term from `font_dir()`, with DIE's default rules.
    pub fn term(points: f64, scale: f64) -> Config {
        let dir = font_dir();
        Config {
            faces: family(&dir, "IoskeleyMonoTerm"),
            points,
            scale,
            rules: RangeRule::defaults(),
        }
    }
}

/// The four files of a family named by its prefix, in `dir`.
fn family(dir: &Path, prefix: &str) -> [PathBuf; PRIMARY_FACES] {
    ["Regular", "Bold", "Italic", "BoldItalic"].map(|s| dir.join(format!("{prefix}-{s}.ttf")))
}

/// The faces `config` names, the rules' after the primary family's, and the cell they make.
fn load(config: &Config) -> Result<(Vec<Face>, CellMetrics), String> {
    let px = config.points * config.scale;
    let mut faces = vec![];
    for path in &config.faces {
        faces.push(Face::from_file(path, px)?);
    }
    for path in config.rules.iter().flat_map(RangeRule::files) {
        faces.push(Face::from_file(path, px)?);
    }
    let regular = &faces[0];
    let m = regular.glyph('M').ok_or("the font has no M")?;
    let cell = CellMetrics::new(regular.metrics(), regular.advance(m), config.scale);
    Ok((faces, cell))
}

/// Draws scenes: the fonts, the GPU's pipeline and the row cache.
pub struct Renderer {
    config: Config,
    pipeline: Pipeline,
    /// The primary family, then the faces the rules name.
    faces: Vec<Face>,
    /// Beside `rules`: the faces each rule pins.
    pinned: Vec<Vec<usize>>,
    rules: Vec<RangeRule>,
    frames: Frames,
}

impl Renderer {
    /// Load the fonts and compile the pipeline. It fails if the platform can't draw
    /// (`backend::ready`'s message) or a font doesn't load.
    pub fn new(config: &Config) -> Result<Renderer, String> {
        backend::ready()?;
        let (faces, cell) = load(config)?;
        let gpu = Gpu::system_default().ok_or("no Metal device")?;
        Ok(Renderer {
            config: config.clone(),
            pipeline: gpu.pipeline(GRAY_ATLAS, COLOR_ATLAS)?,
            faces,
            pinned: font::pins(&config.rules, PRIMARY_FACES),
            rules: config.rules.clone(),
            frames: Frames::new(cell, GRAY_ATLAS, COLOR_ATLAS),
        })
    }

    /// Draw at a new scale: the fonts load again at their new size, and the cell and the row
    /// cache start over. The atlases' textures stay, since only what's placed in them again
    /// gets sampled. If a font doesn't load, nothing changes.
    pub fn set_scale(&mut self, scale: f64) -> Result<(), String> {
        let config = Config {
            scale,
            ..self.config.clone()
        };
        let (faces, cell) = load(&config)?;
        self.faces = faces;
        self.frames = Frames::new(cell, GRAY_ATLAS, COLOR_ATLAS);
        self.config = config;
        Ok(())
    }

    /// Give `window` a surface this renderer can present on. With `readable`, its frames can
    /// be read back (`Surface::read_next_frame`), at some cost.
    pub fn attach(&self, window: &Window, readable: bool) -> Result<Surface, String> {
        Surface::attach(window, &self.pipeline, readable)
    }

    /// The cell's geometry.
    pub fn cell(&self) -> &CellMetrics {
        self.frames.cell()
    }

    /// Which rows the last frame rebuilt.
    pub fn rebuilt(&self) -> &[usize] {
        self.frames.rebuilt()
    }

    /// Draw `scene` on a `w` × `h` target and read it back, with the grid at the top
    /// left and `scene.background` around it. With no pixels, there's nothing to draw.
    pub fn render(&mut self, scene: &Scene, w: u32, h: u32) -> Result<Image, String> {
        self.supply_glyphs(scene);
        let frame = self.frames.build(scene);
        for upload in &frame.uploads {
            self.pipeline.upload(upload)?;
        }
        if w == 0 || h == 0 {
            let bgra = vec![];
            return Ok(Image { w, h, bgra });
        }
        self.pipeline.render(frame.quads, frame.clear, w, h)
    }

    /// Draw `scene` on `surface`'s next drawable, as `render` does, and show it. With `sync`
    /// it goes on screen with the Core Animation transaction, as a resize needs. `Ok(None)`
    /// means there was no drawable.
    pub fn present(
        &mut self,
        scene: &Scene,
        surface: &Surface,
        sync: bool,
    ) -> Result<Option<Presented>, String> {
        let start = Instant::now();
        self.supply_glyphs(scene);
        let frame = self.frames.build(scene);
        for upload in &frame.uploads {
            self.pipeline.upload(upload)?;
        }
        let building = start.elapsed().as_secs_f64() * 1e3;
        let presented = self
            .pipeline
            .present(frame.quads, frame.clear, surface, sync)?;
        Ok(presented.map(|p| Presented {
            cpu_ms: p.cpu_ms + building,
            ..p
        }))
    }

    /// Give the row cache every glyph `scene` needs.
    fn supply_glyphs(&mut self, scene: &Scene) {
        let mut wanted = self.frames.diff(scene);
        while !wanted.is_empty() {
            for key in &wanted {
                let bitmap = self.bitmap(key);
                self.frames.supply(key, bitmap);
            }
            wanted = self.frames.wanted();
        }
    }

    /// `key`'s pixels: a sprite if the rules give it to the renderer and the renderer
    /// draws it, otherwise from the first face in the chain that has all of it, and
    /// then from the face the system picks.
    fn bitmap(&self, key: &GlyphKey) -> Option<GlyphBitmap> {
        let cell = self.cell();
        let (glyph, bold, italic, span) = match key {
            GlyphKey::Line(l) => return Some(cell.line_bitmap(*l)),
            GlyphKey::Text {
                glyph,
                bold,
                italic,
                span,
            } => (glyph, *bold, *italic, *span),
        };
        let text = glyph.to_string();
        let source = font::source(&self.rules, &text);
        let drawn = source
            .sprite
            .and_then(|c| sprite::draw(c, cell.w, cell.h, cell.thin));
        if let Some(drawn) = drawn {
            return Some(cell.cell_bitmap(drawn));
        }
        let pinned = source.rule.map_or(&[][..], |r| &self.pinned[r]);
        let styled = font::styled_face(bold, italic);
        let chain = font::chain(pinned, styled);
        let has = |i: usize, c: char| self.faces[i].glyph(c).is_some();
        let fallback_face;
        let (face, fallback) = match font::covering(&chain, &text, has) {
            Some(i) => (&self.faces[i], i >= PRIMARY_FACES),
            None => {
                fallback_face = self.faces[styled].fallback_for(&text);
                (&fallback_face, true)
            }
        };
        let shaped = face.shape(&text);
        let fit = font::fit(shaped.ink, shaped.advance, u32::from(span), cell, fallback)?;
        shaped.draw(&fit)
    }
}
