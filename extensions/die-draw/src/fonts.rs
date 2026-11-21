//! The fonts and text set in them, a character at a time.
//!
//! The skeleton's bodies are crude stand-ins with the lift's signatures: every character
//! advances `0.6 × size × scale`, a face rises `0.8` of an em and drops `0.2`, its capitals are
//! `0.7` tall, and no glyph has coverage. The lift from die-markdown replaces them.

use std::path::{Path, PathBuf};

/// A face at fixed settings, as a number: what a placed glyph names its face by.
pub type Inst = u16;

/// The glyph of a character no face has: drawn as a small hollow box.
pub const TOFU: u16 = u16::MAX;

/// The stand-in's units to an em.
const UPEM: f32 = 1000.0;

/// How heavy a face is.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub enum Weight {
    /// Text.
    #[default]
    Regular,
    /// 500 where the face has it, else the semibold.
    Medium,
    /// Headings and strong text: 600.
    Semibold,
}

/// Which face a run is set in.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub struct Style {
    /// The monospace, for code.
    pub mono: bool,
    /// Its weight.
    pub weight: Weight,
    /// The italic.
    pub italic: bool,
}

/// A face's measures, in its own units.
#[derive(Clone, Copy, Debug)]
pub struct Metrics {
    /// Units to an em.
    pub upem: f32,
    /// How far it rises above the baseline.
    pub ascent: f32,
    /// How far it drops below, positive.
    pub descent: f32,
    /// The height of a lowercase x.
    pub x_height: f32,
    /// The height of a capital: the face's own, else 0.7 of an em. Digits are as tall.
    pub cap_height: f32,
}

/// A glyph's coverage at one size and one fraction of a pixel.
#[derive(Debug)]
pub struct Mask {
    /// Its left column, in pixels from the pen's whole pixel.
    pub left: i32,
    /// Its top row, in pixels from the baseline's row: negative above.
    pub top: i32,
    /// Its width.
    pub width: u32,
    /// Its height.
    pub height: u32,
    /// `width` by `height` coverage bytes, rows top first.
    pub cov: Vec<u8>,
}

/// The fonts and everything drawn from them.
pub struct Book {
    found: String,
}

impl std::fmt::Debug for Book {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Book({})", self.found)
    }
}

/// Where DIE's own fonts are: `option` if set, else `$DIE_FONT_DIR`, else the app bundle's
/// `Contents/Resources/fonts`, else `fonts/` at the workspace root.
pub fn font_dir(option: Option<&Path>) -> PathBuf {
    if let Some(d) = option {
        return d.to_path_buf();
    }
    if let Some(d) = std::env::var_os("DIE_FONT_DIR") {
        return PathBuf::from(d);
    }
    let exe = std::env::current_exe().ok();
    // Through ~/.local/bin/die, the executable's path is the link's.
    let exe = exe.map(|e| e.canonicalize().unwrap_or(e));
    match exe.as_deref().and_then(bundle_of) {
        Some(bundle) => bundle.join("Contents/Resources/fonts"),
        None => PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../fonts")),
    }
}

/// The app bundle holding this executable, if it's at `<name>.app/Contents/MacOS/<exe>`.
fn bundle_of(exe: &Path) -> Option<PathBuf> {
    let macos = exe.parent()?;
    let contents = macos.parent()?;
    let bundle = contents.parent()?;
    let is_app = bundle.extension().is_some_and(|e| e == "app");
    (macos.file_name()? == "MacOS" && contents.file_name()? == "Contents" && is_app)
        .then(|| bundle.to_path_buf())
}

impl Book {
    /// Finds the fonts. An error says there's none to draw with.
    pub fn load(_own: Option<&Path>) -> Result<Book, String> {
        Ok(Book {
            found: String::new(),
        })
    }

    /// What was found, as family names: for example `"SF, SF Mono"`.
    pub fn found(&self) -> &str {
        &self.found
    }

    /// The fonts for one layout or one paint: the faces parsed as they're first needed.
    pub fn pass(&mut self) -> Pass<'_> {
        Pass { _book: self }
    }
}

/// The fonts through one layout or one paint.
pub struct Pass<'a> {
    _book: &'a mut Book,
}

/// Where a run of text has got to.
pub struct Run {
    css: f32,
    scale: f32,
    _tabular: bool,
}

impl Run {
    /// Digits set as tabular figures: one width each, with no kerning on either side.
    pub fn tabular(mut self, on: bool) -> Run {
        self._tabular = on;
        self
    }
}

/// A character as it's set: its face and glyph, how far the pen moves after it, and the
/// kerning before it, in physical pixels.
#[derive(Clone, Copy, Debug)]
pub struct Shaped {
    /// The face it's drawn in.
    pub inst: Inst,
    /// Its glyph in that face.
    pub id: u16,
    /// How far the pen moves after it.
    pub advance: f32,
    /// How far the pen moves before it.
    pub kern: f32,
}

impl Pass<'_> {
    /// The instance of a source for text of a CSS size, made if new.
    pub fn primary(&mut self, _style: Style, _css: f32) -> Inst {
        0
    }

    /// An instance's measures.
    pub fn metrics(&self, _inst: Inst) -> Metrics {
        Metrics {
            upem: UPEM,
            ascent: 0.8 * UPEM,
            descent: 0.2 * UPEM,
            x_height: 0.5 * UPEM,
            cap_height: 0.7 * UPEM,
        }
    }

    /// Starts a run of text in a style at a CSS size, drawn at `scale` physical pixels to one.
    pub fn run(&mut self, _style: Style, css: f32, scale: f32) -> Run {
        Run {
            css,
            scale,
            _tabular: false,
        }
    }

    /// The next character of a run.
    pub fn next(&mut self, run: &mut Run, _c: char) -> Shaped {
        Shaped {
            inst: 0,
            id: 0,
            advance: 0.6 * run.css * run.scale,
            kern: 0.0,
        }
    }

    /// A glyph's coverage at `px` pixels an em, starting `bin` of `bins` parts of a pixel in,
    /// its outline grown `grow` pixels a side. None for a glyph with nothing to draw.
    pub fn mask(
        &mut self,
        _inst: Inst,
        _glyph: u16,
        _px: f32,
        _bin: u8,
        _bins: u8,
        _grow: f32,
    ) -> Option<&Mask> {
        None
    }

    /// The coverage of a shape that's no glyph, by a number of its own and its size, made by
    /// `make` the first time.
    pub fn shape(
        &mut self,
        _kind: u16,
        _px: f32,
        _make: impl FnOnce() -> Option<Mask>,
    ) -> Option<&Mask> {
        None
    }
}

/// A path (y down, the origin on the baseline at the pen) at `scale` pixels a unit and `sub_x`
/// of a pixel to the right, filled into coverage; with `grow` above zero the shape is that much
/// wider a side.
pub fn fill(
    _path: &tiny_skia::Path,
    _scale: f32,
    _sub_x: f32,
    _grow: f32,
    _rule: tiny_skia::FillRule,
) -> Option<Mask> {
    None
}
