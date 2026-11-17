//! typeset: from markdown source to a laid-out page and its pixels. The geometry is GitHub's
//! (sizes, spacing, rules, panels, a centred column), the colours a [`Palette`]'s, the letters the
//! system's proportional font. It knows nothing of the editor: the page surface gives it text, a
//! width and a palette, and shows what it paints.
//!
//! Every length here is in physical pixels: x from the left edge of the area the page is laid
//! out for, y from the page's top. The display's scale ([`Env::scale`]) turns the sheet's CSS
//! pixels into them.
//!
//! Its parts, none of which is seen from outside:
//!
//! - `doc`: the text as blocks and runs, built on the parser's events, with `html` for the
//!   tags a README uses;
//! - `sheet`: GitHub's geometry as numbers;
//! - `fonts` and `kern`: the system's faces read from their files and set as CoreText sets
//!   them, and the glyphs drawn and kept;
//! - `layout`: the document set for a width, as a list of things to draw;
//! - `paint`: the rows a buffer shows, drawn;
//! - `picture`: PNG files, and pictures at other sizes.

mod doc;
mod fonts;
mod html;
mod kern;
mod layout;
mod paint;
mod picture;
mod sheet;

use std::collections::HashMap;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

// ------------------------------------------------------------------ colours

/// A colour, as red, green and blue.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Rgb(pub u8, pub u8, pub u8);

/// An alert's two colours: `> [!NOTE]` and its kin.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AlertInk {
    /// The bar down its left side.
    pub bar: Rgb,
    /// Its title and icon.
    pub title: Rgb,
}

/// The colours a page is drawn in. The geometry is the sheet's and isn't here: a palette says
/// what colour a thing is, never how large, how bold or how far apart.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Palette {
    /// The page's background.
    pub canvas: Rgb,
    /// The background of a code block, of every second row of a table and of a key cap.
    pub panel: Rgb,
    /// The background of inline code.
    pub pill: Rgb,
    /// Text.
    pub text: Rgb,
    /// Quieter text: footnotes, a picture's missing-file note.
    pub muted: Rgb,
    /// The quietest: a wrapped code line's mark, a placeholder.
    pub subtle: Rgb,
    /// A table's grid, a quote's bar, a rule.
    pub border: Rgb,
    /// The line under a first and a second level heading.
    pub hairline: Rgb,
    /// Headings, first level to sixth.
    pub headings: [Rgb; 6],
    /// Strong text.
    pub strong: Rgb,
    /// Emphasized text.
    pub em: Rgb,
    /// Struck text.
    pub del: Rgb,
    /// A link.
    pub link: Rgb,
    /// Whether a link is underlined.
    pub link_underline: bool,
    /// Inline code's text.
    pub code: Rgb,
    /// A code block's text.
    pub code_block: Rgb,
    /// A block quote's text.
    pub quote: Rgb,
    /// A table's header row.
    pub table_header: Rgb,
    /// Bullets, numbers and an empty checkbox's outline.
    pub marker: Rgb,
    /// A checked checkbox's fill.
    pub check: Rgb,
    /// Alerts: note, tip, important, warning, caution.
    pub alerts: [AlertInk; 5],
}

impl Palette {
    /// The mono-dark theme's colours, as the owner's preview stylesheet has them.
    pub fn mono_dark() -> Palette {
        let (bright, fg, mid, dim) = (
            Rgb(0xee, 0xee, 0xee),
            Rgb(0xcc, 0xcc, 0xcc),
            Rgb(0x88, 0x88, 0x88),
            Rgb(0x55, 0x55, 0x55),
        );
        let border = Rgb(0x3a, 0x3a, 0x3a);
        Palette {
            canvas: Rgb(0x1a, 0x1a, 0x1a),
            panel: Rgb(0x25, 0x25, 0x25),
            pill: Rgb(0x2e, 0x2e, 0x2e),
            text: fg,
            muted: mid,
            subtle: dim,
            border,
            hairline: border,
            headings: [bright, bright, bright, bright, bright, mid],
            strong: bright,
            em: mid,
            del: dim,
            link: Rgb(0xcc, 0x44, 0x44),
            link_underline: true,
            code: mid,
            code_block: fg,
            quote: mid,
            table_header: bright,
            marker: mid,
            check: Rgb(0x55, 0xaa, 0x55),
            alerts: [
                AlertInk {
                    bar: Rgb(0x55, 0x88, 0xaa),
                    title: Rgb(0x77, 0x99, 0xbb),
                },
                AlertInk {
                    bar: Rgb(0x55, 0xaa, 0x55),
                    title: Rgb(0x77, 0xcc, 0x77),
                },
                AlertInk {
                    bar: Rgb(0x88, 0x66, 0x88),
                    title: Rgb(0xaa, 0x88, 0xaa),
                },
                AlertInk {
                    bar: Rgb(0xb8, 0x96, 0x3e),
                    title: Rgb(0xcc, 0xaa, 0x55),
                },
                AlertInk {
                    bar: Rgb(0xcc, 0x44, 0x44),
                    title: Rgb(0xdd, 0x66, 0x66),
                },
            ],
        }
    }

    /// GitHub's own dark colours.
    pub fn github_dark() -> Palette {
        let (fg, muted) = (Rgb(0xc9, 0xd1, 0xd9), Rgb(0x8b, 0x94, 0x9e));
        Palette {
            canvas: Rgb(0x0d, 0x11, 0x17),
            panel: Rgb(0x16, 0x1b, 0x22),
            pill: Rgb(0x34, 0x39, 0x41),
            text: fg,
            muted,
            subtle: Rgb(0x6e, 0x76, 0x81),
            border: Rgb(0x30, 0x36, 0x3d),
            hairline: Rgb(0x21, 0x26, 0x2d),
            headings: [fg, fg, fg, fg, fg, muted],
            strong: fg,
            em: fg,
            del: fg,
            link: Rgb(0x58, 0xa6, 0xff),
            link_underline: false,
            code: fg,
            code_block: fg,
            quote: muted,
            table_header: fg,
            marker: fg,
            check: Rgb(0x1f, 0x6f, 0xeb),
            alerts: [
                AlertInk {
                    bar: Rgb(0x1f, 0x6f, 0xeb),
                    title: Rgb(0x58, 0xa6, 0xff),
                },
                AlertInk {
                    bar: Rgb(0x23, 0x86, 0x36),
                    title: Rgb(0x3f, 0xb9, 0x50),
                },
                AlertInk {
                    bar: Rgb(0x89, 0x57, 0xe5),
                    title: Rgb(0xa3, 0x71, 0xf7),
                },
                AlertInk {
                    bar: Rgb(0x9e, 0x6a, 0x03),
                    title: Rgb(0xd2, 0x99, 0x22),
                },
                AlertInk {
                    bar: Rgb(0xda, 0x36, 0x33),
                    title: Rgb(0xf8, 0x51, 0x49),
                },
            ],
        }
    }

    /// GitHub's own light colours.
    pub fn github_light() -> Palette {
        let (fg, muted) = (Rgb(0x24, 0x29, 0x2f), Rgb(0x57, 0x60, 0x6a));
        Palette {
            canvas: Rgb(0xff, 0xff, 0xff),
            panel: Rgb(0xf6, 0xf8, 0xfa),
            pill: Rgb(0xef, 0xf1, 0xf3),
            text: fg,
            muted,
            subtle: Rgb(0x6e, 0x77, 0x81),
            border: Rgb(0xd0, 0xd7, 0xde),
            hairline: Rgb(0xd8, 0xde, 0xe4),
            headings: [fg, fg, fg, fg, fg, muted],
            strong: fg,
            em: fg,
            del: fg,
            link: Rgb(0x09, 0x69, 0xda),
            link_underline: false,
            code: fg,
            code_block: fg,
            quote: muted,
            table_header: fg,
            marker: fg,
            check: Rgb(0x09, 0x69, 0xda),
            alerts: [
                AlertInk {
                    bar: Rgb(0x09, 0x69, 0xda),
                    title: Rgb(0x09, 0x69, 0xda),
                },
                AlertInk {
                    bar: Rgb(0x1a, 0x7f, 0x37),
                    title: Rgb(0x1a, 0x7f, 0x37),
                },
                AlertInk {
                    bar: Rgb(0x82, 0x50, 0xdf),
                    title: Rgb(0x82, 0x50, 0xdf),
                },
                AlertInk {
                    bar: Rgb(0xbf, 0x87, 0x00),
                    title: Rgb(0x9a, 0x67, 0x00),
                },
                AlertInk {
                    bar: Rgb(0xcf, 0x22, 0x2e),
                    title: Rgb(0xcf, 0x22, 0x2e),
                },
            ],
        }
    }
}

// ------------------------------------------------------------------ what a layout is given

/// What a layout depends on besides the text.
#[derive(Clone, Debug)]
pub struct Env<'a> {
    /// The width of the area the page is laid out for, in physical pixels. The page centres its
    /// column in it: at most 980 CSS pixels wide, its padding shrinking in a narrow area.
    pub width: u32,
    /// Physical pixels to a CSS pixel: the display's scale, 2.0 on a Retina panel.
    pub scale: f32,
    /// The colours.
    pub palette: &'a Palette,
    /// The directory a picture's relative path is read against: the file's own.
    pub dir: Option<&'a Path>,
    /// Show front matter (as a table, as GitHub does) where the default leaves it out.
    pub front_matter: bool,
    /// The columns a tab in a code block takes.
    pub tab_width: usize,
}

/// The fonts a page is set in, with the glyphs already drawn. One serves every page.
#[derive(Debug)]
pub struct Fonts {
    book: fonts::Book,
}

impl Fonts {
    /// Loads the system's fonts: its proportional face in regular, semibold, italic and semibold
    /// italic, its monospace, and the faces that fill in symbols. `own` is the directory DIE's
    /// own fonts are in, the last resort where the system has none we know. An error says no
    /// font could be read at all, and a page can't be drawn.
    pub fn load(own: Option<&Path>) -> Result<Fonts, String> {
        fonts::Book::load(own).map(|book| Fonts { book })
    }

    /// The families found: the text's, then the code's.
    pub fn found(&self) -> &str {
        self.book.found()
    }

    /// How far the pen moves over `text` set as body text at `scale`, in physical pixels, and
    /// the same with no kerning between its letters: what a test of the kerning compares.
    pub fn measure(&mut self, text: &str, scale: f32) -> (f32, f32) {
        let mut pass = self.book.pass();
        let mut run = pass.run(fonts::Style::default(), sheet::TEXT, scale);
        let (mut kerned, mut plain) = (0.0, 0.0);
        for c in text.chars() {
            let shaped = pass.next(&mut run, c);
            kerned += shaped.advance + shaped.kern;
            plain += shaped.advance;
        }
        (kerned, plain)
    }
}

/// A decoded picture.
#[derive(Clone, Debug)]
pub struct Picture {
    /// Its width in its own pixels.
    pub width: u32,
    /// Its height in its own pixels.
    pub height: u32,
    /// Premultiplied RGBA8, `width * 4` bytes a row.
    pub pixels: Vec<u8>,
}

impl Picture {
    /// Reads and decodes a picture file. It touches nothing shared, so a job's thread can run it.
    pub fn load(path: &Path) -> Result<Picture, String> {
        picture::load(path)
    }
}

/// A picture's pixels at a size a page drew it at.
#[derive(Debug)]
struct Sized {
    width: u32,
    height: u32,
    pixels: Arc<Vec<u8>>,
}

/// The pictures pages show, by the file they came from. A layout asks it for a picture's size,
/// and lists what it wanted and didn't find in [`Page::wants`]: the caller loads those (off the
/// main thread), puts each in with [`Pictures::insert`] or [`Pictures::fail`], and lays out again.
#[derive(Debug, Default)]
pub struct Pictures {
    known: HashMap<PathBuf, Option<Picture>>,
    /// Pictures at the sizes they were last painted at: a paint only reads the pictures, so
    /// what it makes is kept behind a lock. Two sizes a picture, for two windows on one file.
    sized: Mutex<HashMap<PathBuf, Vec<Sized>>>,
}

impl Pictures {
    /// None yet.
    pub fn new() -> Pictures {
        Pictures::default()
    }

    /// A picture arrived.
    pub fn insert(&mut self, path: PathBuf, picture: Picture) {
        self.unsize(&path);
        self.known.insert(path, Some(picture));
    }

    /// A picture can't be had: the page shows its alternative text, and stops wanting it.
    pub fn fail(&mut self, path: PathBuf) {
        self.unsize(&path);
        self.known.insert(path, None);
    }

    /// Whether a path has been answered, with a picture or a failure.
    pub fn answered(&self, path: &Path) -> bool {
        self.known.contains_key(path)
    }

    /// Forgets a picture, as when its file changed: the next layout wants it again.
    pub fn forget(&mut self, path: &Path) {
        self.unsize(path);
        self.known.remove(path);
    }

    fn unsize(&mut self, path: &Path) {
        let sized = self.sized.get_mut().unwrap_or_else(|e| e.into_inner());
        sized.remove(path);
    }

    /// A picture's pixels at `width` by `height`, made the first time and kept.
    fn at_size(&self, path: &Path, width: u32, height: u32) -> Option<Arc<Vec<u8>>> {
        let picture = self.known.get(path)?.as_ref()?;
        let mut sized = self.sized.lock().unwrap_or_else(|e| e.into_inner());
        let kept = sized.entry(path.to_path_buf()).or_default();
        if let Some(found) = kept.iter().find(|s| s.width == width && s.height == height) {
            return Some(found.pixels.clone());
        }
        let pixels = Arc::new(if (width, height) == (picture.width, picture.height) {
            picture.pixels.clone()
        } else {
            picture::scaled(picture, width, height)
        });
        if kept.len() >= 2 {
            kept.remove(0);
        }
        kept.push(Sized {
            width,
            height,
            pixels: pixels.clone(),
        });
        Some(pixels)
    }
}

// ------------------------------------------------------------------ what a layout tells

/// A rectangle on the page, in physical pixels from the page's top-left.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    /// Its left edge.
    pub x: f32,
    /// Its top edge.
    pub y: f32,
    /// Its width.
    pub w: f32,
    /// Its height.
    pub h: f32,
}

impl Rect {
    /// Whether a point is in it.
    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && x < self.x + self.w && y >= self.y && y < self.y + self.h
    }
}

/// A link on the page.
#[derive(Clone, Debug, PartialEq)]
pub struct LinkBox {
    /// Where it is: one rectangle for each line it runs over, in reading order.
    pub rects: Vec<Rect>,
    /// Where it leads, as the source has it.
    pub dest: String,
    /// Its title, if the source gives one.
    pub title: String,
    /// Whether it's a picture (its `dest` is the picture's file).
    pub image: bool,
    /// The source bytes it was made from.
    pub source: Range<usize>,
}

/// A heading on the page.
#[derive(Clone, Debug, PartialEq)]
pub struct HeadingPos {
    /// The top of its first line of text.
    pub y: f32,
    /// 1 to 6.
    pub level: u8,
    /// Its anchor, as GitHub makes one: what `#anchor` in a link names.
    pub slug: String,
    /// The source bytes it was made from.
    pub source: Range<usize>,
}

/// What is drawn over a page for the moment.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Marks {
    /// The link the keyboard is on, an index into [`Page::links`]: it gets a ring.
    pub focus: Option<usize>,
}

// ------------------------------------------------------------------ the page

/// A document laid out for one width, scale and palette.
#[derive(Debug)]
pub struct Page {
    laid: layout::Laid,
}

impl Page {
    /// Lays the document out.
    pub fn new(text: &str, env: &Env<'_>, fonts: &mut Fonts, pictures: &Pictures) -> Page {
        let doc = doc::parse(text, env.front_matter);
        Page {
            laid: layout::lay(&doc, env, fonts.book.pass(), pictures),
        }
    }

    /// The page's height: its last block's bottom and the padding under it.
    pub fn height(&self) -> f32 {
        self.laid.height
    }

    /// The links, in reading order.
    pub fn links(&self) -> &[LinkBox] {
        &self.laid.links
    }

    /// The link at a point of the page, as an index into [`Page::links`].
    pub fn link_at(&self, x: f32, y: f32) -> Option<usize> {
        let mut links = self.laid.links.iter();
        links.position(|l| l.rects.iter().any(|r| r.contains(x, y)))
    }

    /// The headings, in reading order.
    pub fn headings(&self) -> &[HeadingPos] {
        &self.laid.headings
    }

    /// Where the place a link's `#name` names is on the page: a heading by its anchor, or what
    /// the page itself links to, a footnote (`fn-` and its label) and the first reference to
    /// one (`fnref-` and its label).
    pub fn anchor(&self, name: &str) -> Option<f32> {
        let heading = self.laid.headings.iter().find(|h| h.slug == name);
        let other = || self.laid.anchors.iter().find(|(n, _)| n == name);
        heading.map(|h| h.y).or_else(|| other().map(|(_, y)| *y))
    }

    /// Where a byte of the source is on the page: the top of the line that shows it, or of the
    /// nearest thing shown after it, as for a byte in hidden markup or between blocks.
    pub fn y_of(&self, source: usize) -> f32 {
        let spans = &self.laid.spans;
        let at = spans.partition_point(|s| s.end <= source && s.src < source);
        spans.get(at).or(spans.last()).map_or(0.0, |s| s.y0)
    }

    /// The source byte shown at a height of the page: the start of what the line there, or the
    /// first line under it, was made from.
    pub fn source_at(&self, y: f32) -> usize {
        let spans = &self.laid.spans;
        let at = spans.partition_point(|s| s.y1 <= y && s.y0 < y);
        spans.get(at).or(spans.last()).map_or(0, |s| s.src)
    }

    /// The picture files this layout wanted and `pictures` had no answer for.
    pub fn wants(&self) -> &[PathBuf] {
        &self.laid.wants
    }

    /// Paints the part of the page a pixel buffer shows. `out` is premultiplied RGBA8, `width`
    /// by `height` pixels and `width * 4` bytes a row; the buffer's row 0 shows the page's row
    /// `top`. Every pixel of the buffer's rows `rows` is written, opaque: the canvas where the
    /// page has nothing, as past its last row or beside its column. Other rows are left alone.
    #[allow(clippy::too_many_arguments)]
    pub fn paint(
        &self,
        fonts: &mut Fonts,
        pictures: &Pictures,
        out: &mut [u8],
        width: u32,
        height: u32,
        top: i32,
        rows: Range<u32>,
        marks: &Marks,
    ) {
        let focus = marks.focus.and_then(|i| self.laid.links.get(i));
        let rects = focus.map(|l| l.rects.as_slice());
        let pass = fonts.book.pass();
        paint::paint(
            &self.laid, pass, pictures, out, width, height, top, rows, rects,
        );
    }
}
