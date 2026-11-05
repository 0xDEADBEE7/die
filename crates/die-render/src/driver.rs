//! The frame driver: the one piece of code that calls `platform::backend`
//! (docs/records/platforms.md §5.2). It asks the faces for what `rows::Frames` wants,
//! with `font` deciding where each glyph goes, and hands the frame's data to the GPU.
//!
//! R2 draws offscreen: a scene to a texture, read back. There's no window here, and
//! R3 gives this a drawable to present.

use crate::atlas::GlyphKey;
use crate::font::{self, CellMetrics, Draw, RangeRule};
use crate::font_dir;
use crate::frame::{GlyphBitmap, Image};
use crate::platform::backend::{
    self,
    metal::{Gpu, Pipeline},
    text::Face,
};
use crate::rows::Frames;
use crate::sprite;
use die_core::Scene;
use std::path::{Path, PathBuf};

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

/// A scene's size on the GPU: every cell, and nothing around them.
pub fn grid_pixels(scene: &Scene, cell: &CellMetrics) -> (u32, u32) {
    (scene.cols as u32 * cell.w, scene.rows as u32 * cell.h)
}

/// Draws scenes: the fonts, the GPU's pipeline and the row cache.
pub struct Renderer {
    pipeline: Pipeline,
    /// The primary family, then the faces the rules name.
    faces: Vec<Face>,
    /// Beside `rules`: the face each rule pins, if it pins one.
    pinned: Vec<Option<usize>>,
    rules: Vec<RangeRule>,
    frames: Frames,
}

impl Renderer {
    /// Load the fonts and compile the pipeline. It fails if the platform can't draw
    /// (`backend::ready`'s message) or a font doesn't load.
    pub fn new(config: &Config) -> Result<Renderer, String> {
        backend::ready()?;
        let px = config.points * config.scale;
        let mut faces = vec![];
        for path in &config.faces {
            faces.push(Face::from_file(path, px)?);
        }
        let mut pinned = vec![];
        for rule in &config.rules {
            pinned.push(match &rule.draw {
                Draw::File(path) => {
                    faces.push(Face::from_file(path, px)?);
                    Some(faces.len() - 1)
                }
                Draw::Renderer | Draw::Primary => None,
            });
        }
        let regular = &faces[0];
        let m = regular.glyph('M').ok_or("the font has no M")?;
        let cell = CellMetrics::new(regular.metrics(), regular.advance(m), config.scale);
        let gpu = Gpu::system_default().ok_or("no Metal device")?;
        Ok(Renderer {
            pipeline: gpu.pipeline(GRAY_ATLAS, COLOR_ATLAS)?,
            faces,
            pinned,
            rules: config.rules.clone(),
            frames: Frames::new(cell, GRAY_ATLAS, COLOR_ATLAS),
        })
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
        let mut wanted = self.frames.diff(scene);
        while !wanted.is_empty() {
            for key in &wanted {
                let bitmap = self.bitmap(key);
                self.frames.supply(key, bitmap);
            }
            wanted = self.frames.wanted();
        }
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
        let mut chars = text.chars();
        let first = chars.next()?;
        let alone = chars.next().is_none();
        let rule = self.rules.iter().position(|r| r.range.contains(&first));
        if alone && rule.is_some_and(|r| self.rules[r].draw == Draw::Renderer) {
            let drawn = sprite::draw(first, cell.w, cell.h, cell.thin);
            if let Some(drawn) = drawn {
                return Some(cell.cell_bitmap(drawn));
            }
        }
        let pinned = rule.and_then(|r| self.pinned[r]);
        let styled = font::styled_face(bold, italic);
        let chain = font::chain(pinned, styled);
        let has_all = |&&i: &&usize| text.chars().all(|c| self.faces[i].glyph(c).is_some());
        let chosen = chain.iter().find(has_all);
        let fallback_face;
        let (face, fallback) = match chosen {
            Some(&i) => (&self.faces[i], i >= PRIMARY_FACES),
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
