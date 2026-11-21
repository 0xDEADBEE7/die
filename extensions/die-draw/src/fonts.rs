//! The type: the system's proportional face and its monospace, read from their files and set as
//! CoreText sets them, with the glyphs drawn once and kept. Lifted whole from die-markdown's
//! typesetter, which sets its page in it.
//!
//! A face here is an instance: one face of one file at fixed axis settings. On a Mac the text
//! is `SFNS.ttf` with its weight axis at 400, 510 or 590 and its optical size at the CSS size
//! (never below the axis's 17), tracked by its `trak` table and kerned by its GPOS pairs; that
//! reproduces CoreText's advances. Which files there are is found by looking, at run time: the
//! Mac's system faces, the static ones a Mac has, the usual Linux ones, and DIE's own monospace
//! last. No axis is assumed: a setting for an axis a face lacks is left out.
//!
//! A character takes the first face that has it: the style's own, then the regular text face
//! (an italic or a monospace lacks arrows and checks), then the symbol faces, each read from
//! disk the first time something reaches it. One glyph a character: no ligatures, no shaping.
//!
//! Glyphs are outlines filled by tiny-skia at twice the size and averaged down to 8-bit
//! coverage, which halves the error of its four-by-four sampling, and kept by face, glyph, size
//! and the fraction of a pixel they start at.
//!
//! A run may set its digits as tabular figures ([`Run::tabular`]): the face's `tnum` glyphs,
//! one width each, with no kerning either side, so a column of numbers lines up. A face with no
//! `tnum` centres each digit in the widest one's width.

use crate::{gsub, kern};
use std::collections::HashMap;
use std::hash::{BuildHasherDefault, Hasher};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use tiny_skia::{FillRule, LineCap, LineJoin, Mask as SkMask, PathBuilder, Stroke, Transform};
use ttf_parser::{Face, GlyphId, Tag};

/// A face at fixed settings, as a number: what a placed glyph names its face by.
pub type Inst = u16;

/// The glyph of a character no face has: drawn as a small hollow box.
pub const TOFU: u16 = u16::MAX;

/// The face number shapes that aren't glyphs are kept under: a check mark, an alert's icon.
const SHAPES: Inst = u16::MAX;

/// The coverage kept before the lot is dropped and drawn again as needed. A page's worth is
/// well under a megabyte; this only bounds a long session at many sizes.
const MASK_LIMIT: usize = 32 << 20;

/// The CSS size instance 0, the fallback of last resort, is made at: body text's 16 px. Every
/// size under the system face's smallest optical size shares that instance, so this is part of
/// what's drawn and never a choice a caller makes.
const FIRST: f32 = 16.0;

/// How heavy a face is.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub enum Weight {
    /// Text.
    #[default]
    Regular,
    /// 500 where the face has it, else the semibold: a title a little heavier than text.
    Medium,
    /// Headings and strong text: 600, which the system face sets at 590.
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

impl Style {
    fn slot(self) -> usize {
        usize::from(self.mono) * 6 + self.weight as usize * 2 + usize::from(self.italic)
    }
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

/// A hasher for small keys: the maps here are asked once a glyph, where the standard one's
/// guard against chosen keys buys nothing.
#[derive(Default, Clone, Copy)]
pub(crate) struct Fx(u64);

impl Fx {
    fn add(&mut self, v: u64) {
        self.0 = (self.0.rotate_left(5) ^ v).wrapping_mul(0x517c_c1b7_2722_0a95);
    }
}

impl Hasher for Fx {
    fn write(&mut self, bytes: &[u8]) {
        for b in bytes {
            self.add(u64::from(*b));
        }
    }
    fn write_u8(&mut self, v: u8) {
        self.add(u64::from(v));
    }
    fn write_u16(&mut self, v: u16) {
        self.add(u64::from(v));
    }
    fn write_u32(&mut self, v: u32) {
        self.add(u64::from(v));
    }
    fn write_u64(&mut self, v: u64) {
        self.add(v);
    }
    fn write_usize(&mut self, v: usize) {
        self.add(v as u64);
    }
    fn finish(&self) -> u64 {
        self.0
    }
}

/// A map with [`Fx`].
pub(crate) type FxMap<K, V> = HashMap<K, V, BuildHasherDefault<Fx>>;

/// A font file, read the first time a face of it is wanted.
struct File {
    path: PathBuf,
    bytes: OnceLock<Option<Vec<u8>>>,
}

impl File {
    fn bytes(&self) -> Option<&[u8]> {
        let read = || std::fs::read(&self.path).ok();
        self.bytes.get_or_init(read).as_deref()
    }
}

/// One face of a file and the axis settings it's wanted at, before an optical size.
struct Source {
    file: usize,
    index: u32,
    vars: Vec<(Tag, f32)>,
    /// Whether its optical size follows the text's size.
    opsz: bool,
    /// The optical size axis's range, once the face has been looked at: none if it has no such
    /// axis or isn't asked to follow.
    range: Option<Option<(f32, f32)>>,
}

/// An instance, with what has been asked of it so far.
struct Instance {
    source: usize,
    /// The CSS size it was made for: what a fallback for one of its characters is made at.
    css: f32,
    coords: Vec<(Tag, f32)>,
    metrics: Metrics,
    lookups: Vec<u16>,
    /// Whether it kerns at all: a monospace doesn't, and is never asked.
    kerned: bool,
    /// The one advance of a face whose letters are all as wide: none for a proportional face.
    cell: Option<f32>,
    track: Vec<(f32, f32)>,
    /// ASCII: glyph and advance. [`TOFU`] for one this face lacks, which goes the long way.
    ascii: [(u16, f32); 128],
    /// ASCII pairs' kerning, by character, made at the first pair: NaN until looked up.
    ascii_kern: Vec<f32>,
    /// Other characters: the face that has each, and its glyph there.
    found: FxMap<char, (Inst, u16)>,
    /// The digits as tabular figures, made at the first one asked for: each one's glyph, its
    /// advance and the space before it, in the face's units; [`TOFU`] for a digit it lacks.
    figures: Option<[(u16, f32, f32); 10]>,
    advances: FxMap<u16, f32>,
    kerns: FxMap<(u16, u16), f32>,
}

/// A face, a glyph, its size and its fraction of a pixel in 64ths, and how far it's grown in
/// 64ths of a pixel.
type MaskKey = (Inst, u16, u32, u8, u16);

/// What a fallback face is for.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
enum Fill {
    /// Symbols, in a proportional face.
    #[default]
    Symbols,
    /// Symbols at one width: what code asks first, so its columns hold.
    Fixed,
    /// A large file for scripts the text face lacks, asked only for those.
    Scripts,
}

#[derive(Default)]
struct State {
    sources: Vec<Source>,
    /// The source of each [`Style`], by its slot.
    styles: [usize; 12],
    /// The faces that fill in symbols, in the order they're asked, and what each is.
    fallbacks: Vec<(usize, Fill)>,
    instances: Vec<Instance>,
    by_key: FxMap<(usize, u32, u32), Option<Inst>>,
    masks: FxMap<MaskKey, Option<Mask>>,
    mask_bytes: usize,
    /// What was found, for a report.
    found: String,
}

/// The fonts and everything drawn from them.
pub struct Book {
    files: Vec<File>,
    st: State,
}

impl std::fmt::Debug for Book {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Book({})", self.st.found)
    }
}

const OPSZ: &[u8; 4] = b"opsz";
type Vars = &'static [(&'static [u8; 4], f32)];

/// One face of a candidate family: a file, a face in it, and axis settings.
struct Spec {
    path: PathBuf,
    index: u32,
    vars: Vars,
}

/// A family that might be on this machine: regular, italic, medium, medium italic, semibold,
/// semibold italic, as a [`Style`]'s slot counts them. Only the regular must exist.
struct Family {
    name: &'static str,
    opsz: bool,
    faces: [Option<Spec>; 6],
}

fn spec(path: impl Into<PathBuf>, index: u32, vars: Vars) -> Option<Spec> {
    Some(Spec {
        path: path.into(),
        index,
        vars,
    })
}

/// A static family of four files in a directory: regular, bold, italic, bold italic.
fn four(name: &'static str, dir: &Path, files: [&str; 4]) -> Family {
    let at = |i: usize| match files[i] {
        "" => None,
        file => spec(dir.join(file), 0, &[]),
    };
    Family {
        name,
        opsz: false,
        faces: [at(0), at(2), None, None, at(1), at(3)],
    }
}

/// A collection's faces by index: regular, italic, medium, medium italic, bold, bold italic.
fn collection(name: &'static str, path: &str, faces: [Option<u32>; 6]) -> Family {
    Family {
        name,
        opsz: false,
        faces: faces.map(|i| i.and_then(|i| spec(path, i, &[]))),
    }
}

/// Where Linux distributions keep the faces looked for there.
const LINUX: [&str; 17] = [
    "/usr/share/fonts/truetype/dejavu",
    "/usr/share/fonts/dejavu-sans-fonts",
    "/usr/share/fonts/dejavu-sans-mono-fonts",
    "/usr/share/fonts/dejavu",
    "/usr/share/fonts/TTF",
    "/usr/share/fonts/truetype/noto",
    "/usr/share/fonts/google-noto",
    "/usr/share/fonts/noto",
    "/usr/share/fonts/truetype/liberation",
    "/usr/share/fonts/truetype/liberation2",
    "/usr/share/fonts/liberation-sans",
    "/usr/share/fonts/liberation-sans-fonts",
    "/usr/share/fonts/liberation-mono",
    "/usr/share/fonts/liberation-mono-fonts",
    "/usr/share/fonts/liberation",
    "/usr/share/fonts/opentype/noto",
    "/usr/share/fonts/google-noto-cjk",
];

/// The Linux directory a file is in, if any.
fn linux(file: &str) -> Option<PathBuf> {
    let dirs = LINUX.iter().map(Path::new);
    dirs.map(|d| d.join(file)).find(|p| p.is_file())
}

/// A four-file family wherever Linux has its regular face.
fn linux_four(name: &'static str, files: [&str; 4]) -> Option<Family> {
    let regular = linux(files[0])?;
    Some(four(name, regular.parent()?, files))
}

const IOSKELEY: [&str; 4] = [
    "IoskeleyMonoTerm-Regular.ttf",
    "IoskeleyMonoTerm-Bold.ttf",
    "IoskeleyMonoTerm-Italic.ttf",
    "IoskeleyMonoTerm-BoldItalic.ttf",
];

/// The proportional families, best first.
fn text_families(own: &Path) -> Vec<Family> {
    let (up, it) = (
        "/System/Library/Fonts/SFNS.ttf",
        "/System/Library/Fonts/SFNSItalic.ttf",
    );
    let supplemental = Path::new("/System/Library/Fonts/Supplemental");
    let mut all = vec![
        // The settings are CoreText's for the system font: its semibold is 590, not 600.
        Family {
            name: "SF",
            opsz: true,
            faces: [
                spec(up, 0, &[(b"wght", 400.0)]),
                spec(it, 0, &[(b"wght", 400.0)]),
                spec(up, 0, &[(b"wght", 510.0)]),
                spec(it, 0, &[(b"wght", 510.0)]),
                spec(up, 0, &[(b"wght", 590.0)]),
                spec(it, 0, &[(b"wght", 590.8), (b"YAXS", 419.2)]),
            ],
        },
        collection(
            "Helvetica Neue",
            "/System/Library/Fonts/HelveticaNeue.ttc",
            [Some(0), Some(2), Some(10), Some(11), Some(1), Some(3)],
        ),
        collection(
            "Helvetica",
            "/System/Library/Fonts/Helvetica.ttc",
            [Some(0), Some(2), None, None, Some(1), Some(3)],
        ),
        four(
            "Arial",
            supplemental,
            [
                "Arial.ttf",
                "Arial Bold.ttf",
                "Arial Italic.ttf",
                "Arial Bold Italic.ttf",
            ],
        ),
    ];
    let linux = [
        linux_four(
            "DejaVu Sans",
            [
                "DejaVuSans.ttf",
                "DejaVuSans-Bold.ttf",
                "DejaVuSans-Oblique.ttf",
                "DejaVuSans-BoldOblique.ttf",
            ],
        ),
        linux_four(
            "Noto Sans",
            [
                "NotoSans-Regular.ttf",
                "NotoSans-Bold.ttf",
                "NotoSans-Italic.ttf",
                "NotoSans-BoldItalic.ttf",
            ],
        ),
        linux_four(
            "Liberation Sans",
            [
                "LiberationSans-Regular.ttf",
                "LiberationSans-Bold.ttf",
                "LiberationSans-Italic.ttf",
                "LiberationSans-BoldItalic.ttf",
            ],
        ),
    ];
    all.extend(linux.into_iter().flatten());
    all.push(four("Ioskeley Mono", own, IOSKELEY));
    all
}

/// The monospaced families, best first.
fn mono_families(own: &Path) -> Vec<Family> {
    let (up, it) = (
        "/System/Library/Fonts/SFNSMono.ttf",
        "/System/Library/Fonts/SFNSMonoItalic.ttf",
    );
    // CoreText's settings for the monospaced system font: the file's default is its Light.
    const REGULAR: Vars = &[(b"wght", 400.0), (b"YAXS", 324.3341)];
    const SEMIBOLD: Vars = &[(b"wght", 571.3074), (b"YAXS", 324.9394)];
    let mut all = vec![
        Family {
            name: "SF Mono",
            opsz: false,
            faces: [
                spec(up, 0, REGULAR),
                spec(it, 0, REGULAR),
                None,
                None,
                spec(up, 0, SEMIBOLD),
                spec(it, 0, SEMIBOLD),
            ],
        },
        collection(
            "Menlo",
            "/System/Library/Fonts/Menlo.ttc",
            [Some(0), Some(2), None, None, Some(1), Some(3)],
        ),
    ];
    let linux = [
        linux_four(
            "DejaVu Sans Mono",
            [
                "DejaVuSansMono.ttf",
                "DejaVuSansMono-Bold.ttf",
                "DejaVuSansMono-Oblique.ttf",
                "DejaVuSansMono-BoldOblique.ttf",
            ],
        ),
        linux_four(
            "Noto Sans Mono",
            ["NotoSansMono-Regular.ttf", "NotoSansMono-Bold.ttf", "", ""],
        ),
        linux_four(
            "Liberation Mono",
            [
                "LiberationMono-Regular.ttf",
                "LiberationMono-Bold.ttf",
                "LiberationMono-Italic.ttf",
                "LiberationMono-BoldItalic.ttf",
            ],
        ),
    ];
    all.extend(linux.into_iter().flatten());
    all.push(four("Ioskeley Mono", own, IOSKELEY));
    all
}

/// The faces that fill in what the text face lacks, in the order text asks them.
fn fallback_faces(own: &Path) -> Vec<(PathBuf, Fill)> {
    let mut all: Vec<(PathBuf, Fill)> = vec![
        ("/System/Library/Fonts/Menlo.ttc".into(), Fill::Fixed),
        (
            "/System/Library/Fonts/Apple Symbols.ttf".into(),
            Fill::Symbols,
        ),
    ];
    let symbols = [
        ("DejaVuSansMono.ttf", Fill::Fixed),
        ("DejaVuSans.ttf", Fill::Symbols),
        ("NotoSansSymbols-Regular.ttf", Fill::Symbols),
        ("NotoSansSymbols2-Regular.ttf", Fill::Symbols),
    ];
    all.extend(
        symbols
            .iter()
            .filter_map(|(f, fill)| Some((linux(f)?, *fill))),
    );
    all.push((own.join(IOSKELEY[0]), Fill::Fixed));
    let scripts = [
        "/System/Library/Fonts/Supplemental/Arial Unicode.ttf",
        "/System/Library/Fonts/ヒラギノ角ゴシック W4.ttc",
    ];
    all.extend(scripts.iter().map(|p| (PathBuf::from(p), Fill::Scripts)));
    all.extend(linux("NotoSansCJK-Regular.ttc").map(|p| (p, Fill::Scripts)));
    all
}

/// Whether a character is of a script the large fallback files are kept for: not a symbol or
/// an emoji, which they'd be read whole for and mostly not have.
fn other_script(c: char) -> bool {
    matches!(u32::from(c), 0x0250..=0x1fff | 0x2e80..=0xd7ff | 0xf900..=0xffef | 0x2_0000..=0x3_ffff)
}

/// Where DIE's own fonts are: `option` if set, else die-render's rule, which an extension
/// can't reach and so repeats: `$DIE_FONT_DIR`, else the app bundle's
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
    /// Finds the fonts, DIE's own in `own` or [`font_dir`]'s place. An error says there's none
    /// to draw a page with.
    pub fn load(own: Option<&Path>) -> Result<Book, String> {
        let own = font_dir(own);
        Book::of(text_families(&own), mono_families(&own), &own)
    }

    /// The fonts from candidate families, the first of each kind that's on the machine.
    fn of(text: Vec<Family>, mono: Vec<Family>, own: &Path) -> Result<Book, String> {
        let mut book = Book {
            files: Vec::new(),
            st: State::default(),
        };
        let text = book.adopt(text, 0);
        let mono = book.adopt(mono, 6);
        let names = match (text, mono) {
            (Some(text), Some(mono)) => format!("{text}, {mono}"),
            // Whichever was found stands in for the other.
            (Some(text), None) => {
                book.st.styles.copy_within(0..6, 6);
                text.to_owned()
            }
            (None, Some(mono)) => {
                book.st.styles.copy_within(6..12, 0);
                mono.to_owned()
            }
            (None, None) => {
                let own = own.display();
                return Err(format!(
                    "no font to draw a page with: none of the system's, and none in {own}"
                ));
            }
        };
        for (path, fill) in fallback_faces(own) {
            let file = book.file(path);
            book.st.sources.push(Source {
                file,
                index: 0,
                vars: Vec::new(),
                opsz: false,
                range: None,
            });
            book.st.fallbacks.push((book.st.sources.len() - 1, fill));
        }
        book.st.found = names;
        // Instance 0 is the text face at the text's size: what anything falls back to.
        let regular = book.st.styles[0];
        if book.pass().instance(regular, FIRST, 1.0).is_none() {
            return Err("the text font can't be read".to_owned());
        }
        Ok(book)
    }

    /// The families found, for a report.
    pub fn found(&self) -> &str {
        &self.st.found
    }

    /// The place of a file among the files, added if new.
    fn file(&mut self, path: PathBuf) -> usize {
        if let Some(at) = self.files.iter().position(|f| f.path == path) {
            return at;
        }
        self.files.push(File {
            path,
            bytes: OnceLock::new(),
        });
        self.files.len() - 1
    }

    /// Takes the first family whose regular face can be read as the styles from `slot` on, and
    /// says which it took.
    fn adopt(&mut self, families: Vec<Family>, slot: usize) -> Option<&'static str> {
        for family in families {
            let mut sources = [None; 6];
            for (i, face) in family.faces.iter().enumerate() {
                let Some(face) = face else { continue };
                let file = self.file(face.path.clone());
                let readable = self.files[file]
                    .bytes()
                    .is_some_and(|b| Face::parse(b, face.index).is_ok());
                if readable {
                    self.st.sources.push(Source {
                        file,
                        index: face.index,
                        vars: face
                            .vars
                            .iter()
                            .map(|(t, v)| (Tag::from_bytes(t), *v))
                            .collect(),
                        opsz: family.opsz,
                        range: None,
                    });
                    sources[i] = Some(self.st.sources.len() - 1);
                }
            }
            let Some(regular) = sources[0] else { continue };
            for i in 0..6 {
                // A face the family lacks: the medium is the semibold, then the same slant a
                // weight down, then the regular.
                let stand_ins = [i, if i / 2 == 1 { i + 2 } else { i }, i % 2, 0];
                let found = stand_ins.iter().find_map(|s| sources[*s]);
                self.st.styles[slot + i] = found.unwrap_or(regular);
            }
            return Some(family.name);
        }
        None
    }

    /// The fonts for one layout or one paint: the faces parsed as they're first needed.
    pub fn pass(&mut self) -> Pass<'_> {
        Pass {
            files: &self.files,
            st: &mut self.st,
            faces: Vec::new(),
        }
    }
}

/// The fonts through one layout or one paint. ttf-parser's face borrows its file's bytes and
/// costs microseconds to make, so each is made here at its first use and dropped with the pass.
pub struct Pass<'a> {
    files: &'a [File],
    st: &'a mut State,
    faces: Vec<Option<Face<'a>>>,
}

/// Where a run of text has got to: the face the last glyph came from and the glyph, for
/// kerning, with the face's scale.
pub struct Run {
    primary: Inst,
    css: f32,
    scale: f32,
    inst: Inst,
    unit: f32,
    track: f32,
    prev: Option<(u16, char)>,
    /// The width of the primary face's letters, if they're all as wide.
    cell: f32,
    /// Whether its digits are tabular figures.
    tabular: bool,
}

/// A character as it's set: its face and glyph, how far the pen moves after it, and the
/// kerning before it, in physical pixels.
#[derive(Clone, Copy, Debug)]
pub struct Shaped {
    /// The face it's drawn in.
    pub inst: Inst,
    /// Its glyph in that face: [`TOFU`] if no face has it.
    pub id: u16,
    /// How far the pen moves after it.
    pub advance: f32,
    /// How far the pen moves before it: `pen += kern; glyph at pen; pen += advance`.
    pub kern: f32,
}

impl Run {
    /// Digits set as tabular figures: one width each, with no kerning on either side.
    pub fn tabular(mut self, on: bool) -> Run {
        self.tabular = on;
        self
    }
}

impl<'a> Pass<'a> {
    /// The instance of a source for text of a CSS size, made if new.
    fn instance(&mut self, source: usize, css: f32, stretch: f32) -> Option<Inst> {
        let files = self.files;
        let range = match self.st.sources[source].range {
            Some(range) => range,
            None => {
                let src = &self.st.sources[source];
                let face = files[src.file]
                    .bytes()
                    .and_then(|b| Face::parse(b, src.index).ok());
                let axis = face.filter(|_| src.opsz).and_then(|f| {
                    let mut axes = f.variation_axes().into_iter();
                    axes.find(|a| a.tag == Tag::from_bytes(OPSZ))
                });
                let range = axis.map(|a| (a.min_value, a.max_value));
                self.st.sources[source].range = Some(range);
                range
            }
        };
        let optical = range.map(|(lo, hi)| css.max(lo).min(hi));
        let optical_key = optical.map_or(0, |o| (o * 64.0).round() as u32);
        let key = (source, optical_key, (stretch * 4096.0).round() as u32);
        if let Some(known) = self.st.by_key.get(&key) {
            return *known;
        }
        let made = self.make(source, css, optical, stretch);
        self.st.by_key.insert(key, made);
        made
    }

    fn make(
        &mut self,
        source: usize,
        css: f32,
        optical: Option<f32>,
        stretch: f32,
    ) -> Option<Inst> {
        let files = self.files;
        let src = &self.st.sources[source];
        let mut face = Face::parse(files[src.file].bytes()?, src.index).ok()?;
        let mut coords = Vec::new();
        for (tag, value) in &src.vars {
            // `set_variation` takes any tag on a variable face, so ask the axes.
            let mut axes = face.variation_axes().into_iter();
            if let Some(axis) = axes.find(|a| a.tag == *tag) {
                coords.push((*tag, value.max(axis.min_value).min(axis.max_value)));
            }
        }
        coords.extend(optical.map(|o| (Tag::from_bytes(OPSZ), o)));
        for (tag, value) in &coords {
            face.set_variation(*tag, *value);
        }
        // A stretched instance is the face a little larger or smaller: fewer of its units to
        // an em, and everything measured in them follows.
        let upem = f32::from(face.units_per_em()) / stretch.clamp(0.5, 2.0);
        let inst = Inst::try_from(self.st.instances.len()).ok()?;
        if upem <= 0.0 || inst == SHAPES {
            return None;
        }
        let (mut ascent, mut descent) = (f32::from(face.ascender()), -f32::from(face.descender()));
        if ascent <= 0.0 || descent < 0.0 {
            (ascent, descent) = (upem * 0.8, upem * 0.2);
        }
        let x_height = face.x_height().map(f32::from).filter(|x| *x > 0.0);
        let cap_height = face.capital_height().map(f32::from).filter(|c| *c > 0.0);
        let mut ascii = [(TOFU, 0.0); 128];
        for (c, slot) in ascii.iter_mut().enumerate().skip(32) {
            let glyph = face.glyph_index(c as u8 as char).filter(|g| g.0 != 0);
            if let Some(g) = glyph.filter(|g| g.0 != TOFU) {
                *slot = (g.0, f32::from(face.glyph_hor_advance(g).unwrap_or(0)));
            }
        }
        let lookups = kern::lookups(&face);
        let (narrow, wide) = (ascii[usize::from(b'i')].1, ascii[usize::from(b'm')].1);
        self.st.instances.push(Instance {
            source,
            css,
            coords,
            metrics: Metrics {
                upem,
                ascent,
                descent,
                x_height: x_height.unwrap_or(upem * 0.52),
                cap_height: cap_height.unwrap_or(upem * 0.7),
            },
            kerned: !lookups.is_empty() || face.tables().kern.is_some(),
            cell: (narrow == wide && wide > 0.0).then_some(wide),
            lookups,
            track: kern::track(&face),
            ascii,
            ascii_kern: Vec::new(),
            found: FxMap::default(),
            figures: None,
            advances: FxMap::default(),
            kerns: FxMap::default(),
        });
        self.faces.resize_with(usize::from(inst) + 1, || None);
        self.faces[usize::from(inst)] = Some(face);
        Some(inst)
    }

    /// ttf-parser's view of an instance.
    fn face(&mut self, inst: Inst) -> Option<&Face<'a>> {
        let at = usize::from(inst);
        if self.faces.len() <= at {
            self.faces.resize_with(at + 1, || None);
        }
        if self.faces[at].is_none() {
            let files = self.files;
            let instance = self.st.instances.get(at)?;
            let src = &self.st.sources[instance.source];
            let mut face = Face::parse(files[src.file].bytes()?, src.index).ok()?;
            for (tag, value) in &instance.coords {
                face.set_variation(*tag, *value);
            }
            self.faces[at] = Some(face);
        }
        self.faces[at].as_ref()
    }

    /// The face a style's text of a CSS size is set in. There's always one: a style whose own
    /// file has gone takes the text face.
    pub fn primary(&mut self, style: Style, css: f32) -> Inst {
        let source = self.st.styles[style.slot()];
        let regular = self.st.styles[0];
        let found = self.instance(source, css, 1.0);
        found
            .or_else(|| self.instance(regular, css, 1.0))
            .unwrap_or(0)
    }

    /// A face's measures.
    pub fn metrics(&self, inst: Inst) -> Metrics {
        let known = self.st.instances.get(usize::from(inst));
        known.or(self.st.instances.first()).map_or(
            Metrics {
                upem: 1000.0,
                ascent: 800.0,
                descent: 200.0,
                x_height: 500.0,
                cap_height: 700.0,
            },
            |i| i.metrics,
        )
    }

    /// The face that has a character for text in `primary`, and its glyph there: [`TOFU`] if
    /// none has.
    fn glyph(&mut self, primary: Inst, c: char) -> (Inst, u16) {
        let Some(instance) = self.st.instances.get(usize::from(primary)) else {
            return (0, TOFU);
        };
        if let Some(found) = instance.found.get(&c) {
            return *found;
        }
        let (css, own) = (instance.css, instance.source);
        let has = |face: Option<&Face<'a>>| {
            let glyph = face.and_then(|f| f.glyph_index(c));
            glyph.map(|g| g.0).filter(|g| *g != 0 && *g != TOFU)
        };
        let mut found = has(self.face(primary)).map(|g| (primary, g));
        if found.is_none() {
            // Text asks its regular face, then the fallbacks as they come. Code asks the
            // regular monospace and the fallbacks of one width first.
            let scripts = other_script(c);
            let fixed = self.st.styles[6..].contains(&own);
            let (text, mono) = (self.st.styles[0], self.st.styles[6]);
            let fallbacks = self.st.fallbacks.iter().copied();
            let (first, rest): (Vec<_>, Vec<_>) =
                fallbacks.partition(|(_, f)| fixed && *f == Fill::Fixed);
            let ahead = if fixed {
                vec![(mono, Fill::Fixed)]
            } else {
                Vec::new()
            };
            let others: Vec<(usize, Fill)> = ahead
                .into_iter()
                .chain(first)
                .chain([(text, Fill::Symbols)])
                .chain(rest)
                .filter(|(source, fill)| *source != own && (scripts || *fill != Fill::Scripts))
                .collect();
            for (source, _) in others {
                let Some(inst) = self.instance(source, css, 1.0) else {
                    continue;
                };
                let Some(g) = has(self.face(inst)) else {
                    continue;
                };
                // A monospace filling in for a monospace is drawn at the size that makes
                // its letters as wide as the text's, so a box's lines meet and columns hold.
                let cell = |pass: &Self, i: Inst| {
                    let instance = pass.st.instances.get(usize::from(i))?;
                    Some(instance.cell? / instance.metrics.upem)
                };
                let fitted = match (cell(self, primary), cell(self, inst)) {
                    (Some(wanted), Some(has)) if (wanted / has - 1.0).abs() > 0.002 => {
                        let stretch = wanted / has;
                        let near = (0.8..1.25).contains(&stretch);
                        near.then(|| self.instance(source, css, stretch)).flatten()
                    }
                    _ => None,
                };
                found = Some((fitted.unwrap_or(inst), g));
                break;
            }
        }
        let found = found.unwrap_or((primary, TOFU));
        if let Some(instance) = self.st.instances.get_mut(usize::from(primary)) {
            instance.found.insert(c, found);
        }
        found
    }

    /// A glyph's advance in its face's units, without tracking.
    fn advance(&mut self, inst: Inst, glyph: u16) -> f32 {
        let Some(instance) = self.st.instances.get(usize::from(inst)) else {
            return 0.0;
        };
        if glyph == TOFU {
            return instance.metrics.upem * 0.6;
        }
        if let Some(known) = instance.advances.get(&glyph) {
            return *known;
        }
        let read = |f: &Face<'a>| f.glyph_hor_advance(GlyphId(glyph));
        let advance = f32::from(self.face(inst).and_then(read).unwrap_or(0));
        self.st.instances[usize::from(inst)]
            .advances
            .insert(glyph, advance);
        advance
    }

    /// The kerning between two glyphs of one face, in its units.
    fn kern(&mut self, inst: Inst, left: (u16, char), right: (u16, char)) -> f32 {
        let at = usize::from(inst);
        let ascii = (left.1 as usize) < 128 && (right.1 as usize) < 128;
        let place = (left.1 as usize) * 128 + right.1 as usize;
        let instance = &mut self.st.instances[at];
        if ascii {
            if instance.ascii_kern.is_empty() {
                instance.ascii_kern = vec![f32::NAN; 128 * 128];
            }
            let known = instance.ascii_kern[place];
            if !known.is_nan() {
                return known;
            }
        } else if let Some(known) = instance.kerns.get(&(left.0, right.0)) {
            return *known;
        }
        let lookups = std::mem::take(&mut self.st.instances[at].lookups);
        let read = |f: &Face<'a>| kern::pair(f, &lookups, left.0, right.0);
        let value = self.face(inst).map_or(0.0, read);
        let instance = &mut self.st.instances[at];
        instance.lookups = lookups;
        if ascii {
            instance.ascii_kern[place] = value;
        } else {
            instance.kerns.insert((left.0, right.0), value);
        }
        value
    }

    /// Starts a run of text in a style at a CSS size, drawn at `scale` physical pixels to one.
    pub fn run(&mut self, style: Style, css: f32, scale: f32) -> Run {
        let primary = self.primary(style, css);
        let mut run = Run {
            primary,
            css,
            scale,
            inst: primary,
            unit: 0.0,
            track: 0.0,
            prev: None,
            cell: 0.0,
            tabular: false,
        };
        self.enter(&mut run, primary);
        let cell = self
            .st
            .instances
            .get(usize::from(primary))
            .and_then(|i| i.cell);
        run.cell = cell.map_or(0.0, |cell| (cell + run.track) * run.unit);
        run
    }

    /// A run's text goes on in another face.
    fn enter(&self, run: &mut Run, inst: Inst) {
        let Some(instance) = self.st.instances.get(usize::from(inst)) else {
            return;
        };
        run.inst = inst;
        run.unit = run.css * run.scale / instance.metrics.upem;
        // Tracking is looked up at the CSS size, as a browser on a scaled display does.
        run.track = kern::tracking(&instance.track, run.css);
        run.prev = None;
    }

    /// A digit as a tabular figure in an instance: its glyph, advance and the space before it,
    /// in the face's units. None for a character that's no digit, or a digit the face lacks.
    fn figure(&mut self, inst: Inst, c: char) -> Option<(u16, f32, f32)> {
        let d = (c as u32).checked_sub('0' as u32).filter(|d| *d < 10)? as usize;
        let at = usize::from(inst);
        if self.st.instances.get(at)?.figures.is_none() {
            let ascii = self.st.instances[at].ascii;
            let made = self.face(inst).map(|face| {
                let lookups = gsub::lookups(face, b"tnum");
                let mut out = [(TOFU, 0.0, 0.0); 10];
                for (i, slot) in out.iter_mut().enumerate() {
                    let (g, units) = ascii[usize::from(b'0') + i];
                    if g == TOFU {
                        continue;
                    }
                    *slot = match gsub::single(face, &lookups, g) {
                        Some(t) => {
                            let advance = face.glyph_hor_advance(GlyphId(t)).unwrap_or(0);
                            (t, f32::from(advance), 0.0)
                        }
                        None => (g, units, 0.0),
                    };
                }
                if lookups.is_empty() {
                    // No tnum: digits of differing widths are centred in the widest.
                    let widest = out.iter().map(|s| s.1).fold(0.0, f32::max);
                    for s in out.iter_mut().filter(|s| s.0 != TOFU) {
                        s.2 = (widest - s.1) / 2.0;
                        s.1 = widest;
                    }
                }
                out
            });
            self.st.instances[at].figures = Some(made.unwrap_or([(TOFU, 0.0, 0.0); 10]));
        }
        let f = self.st.instances[at].figures.as_ref()?[d];
        (f.0 != TOFU).then_some(f)
    }

    /// The next character of a run.
    pub fn next(&mut self, run: &mut Run, c: char) -> Shaped {
        if run.tabular
            && let Some((id, units, lead)) = self.figure(run.primary, c)
        {
            if run.inst != run.primary {
                self.enter(run, run.primary);
            }
            // No kerning out of a figure, and none into it by returning here: the ASCII pairs'
            // kerning is kept by character, and a figure's glyph is no proportional digit's.
            run.prev = None;
            let (advance, lead) = ((units + run.track) * run.unit, lead * run.unit);
            return Shaped {
                inst: run.primary,
                id,
                advance: advance - lead,
                kern: lead,
            };
        }
        let quick = self
            .st
            .instances
            .get(usize::from(run.primary))
            .and_then(|i| {
                let known = *i.ascii.get(c as usize)?;
                (known.0 != TOFU).then_some(known)
            });
        let (inst, id, units) = match quick {
            Some((id, units)) => (run.primary, id, units),
            None => {
                let (inst, id) = self.glyph(run.primary, c);
                (inst, id, self.advance(inst, id))
            }
        };
        if inst != run.inst {
            self.enter(run, inst);
        }
        let kerned = id != TOFU && self.st.instances[usize::from(inst)].kerned;
        let kern = match run.prev {
            Some(prev) if kerned => self.kern(inst, prev, (id, c)) * run.unit,
            _ => 0.0,
        };
        run.prev = (id != TOFU).then_some((id, c));
        let track = if id == TOFU { 0.0 } else { run.track };
        let advance = (units + track) * run.unit;
        if run.cell > 0.0 && inst != run.primary && advance <= run.cell * 1.5 {
            // A symbol from another face in monospaced text takes a cell, and sits in the
            // middle of it, so the columns hold. One wider, as an ideograph or an emoji, keeps
            // its own width, as a browser sets it: packed, not spread over two cells.
            let before = (run.cell - advance) / 2.0;
            return Shaped {
                inst,
                id,
                advance: run.cell - before,
                kern: before,
            };
        }
        Shaped {
            inst,
            id,
            advance,
            kern,
        }
    }

    /// A glyph's coverage at `px` pixels an em, starting `bin` of `bins` parts of a pixel in,
    /// its outline grown `grow` pixels a side. None for a glyph with nothing to draw, as a space.
    pub fn mask(
        &mut self,
        inst: Inst,
        glyph: u16,
        px: f32,
        bin: u8,
        bins: u8,
        grow: f32,
    ) -> Option<&Mask> {
        let grown = (grow.max(0.0) * 64.0).round().min(f32::from(u16::MAX)) as u16;
        let key = (inst, glyph, (px * 64.0).round() as u32, bin, grown);
        if !self.st.masks.contains_key(&key) {
            let sub_x = f32::from(bin) / f32::from(bins.max(1));
            let grow = f32::from(grown) / 64.0;
            let made = if glyph == TOFU {
                tofu(px, sub_x)
            } else {
                let upem = self.metrics(inst).upem;
                let mut sink = Outline(PathBuilder::new());
                let drawn = self
                    .face(inst)
                    .and_then(|f| f.outline_glyph(GlyphId(glyph), &mut sink));
                let path = drawn.and_then(|_| sink.0.finish());
                path.and_then(|p| fill(&p, px / upem, sub_x, grow, FillRule::Winding))
            };
            self.keep(key, made);
        }
        self.st.masks.get(&key).and_then(Option::as_ref)
    }

    /// The coverage of a shape that's no glyph, by a number of its own and its size, made by
    /// `make` the first time.
    pub fn shape(
        &mut self,
        kind: u16,
        px: f32,
        make: impl FnOnce() -> Option<Mask>,
    ) -> Option<&Mask> {
        let key = (SHAPES, kind, (px * 64.0).round() as u32, 0, 0);
        if !self.st.masks.contains_key(&key) {
            self.keep(key, make());
        }
        self.st.masks.get(&key).and_then(Option::as_ref)
    }

    fn keep(&mut self, key: MaskKey, mask: Option<Mask>) {
        if self.st.mask_bytes > MASK_LIMIT {
            self.st.masks.clear();
            self.st.mask_bytes = 0;
        }
        self.st.mask_bytes += mask.as_ref().map_or(0, |m| m.cov.len()) + 48;
        self.st.masks.insert(key, mask);
    }
}

/// ttf-parser's outline callbacks into a tiny-skia path, y flipped to grow downward.
struct Outline(PathBuilder);

impl ttf_parser::OutlineBuilder for Outline {
    fn move_to(&mut self, x: f32, y: f32) {
        self.0.move_to(x, -y);
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.0.line_to(x, -y);
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        self.0.quad_to(x1, -y1, x, -y);
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        self.0.cubic_to(x1, -y1, x2, -y2, x, -y);
    }
    fn close(&mut self) {
        self.0.close();
    }
}

/// A path (y down, the origin on the baseline at the pen) at `scale` pixels a unit and `sub_x`
/// of a pixel to the right, filled by tiny-skia at twice the size and averaged down: eight
/// samples a pixel each way, where its own fill has four, and overlapping contours stay right
/// since each fill is by winding. With `grow` above zero the shape is that much wider a side:
/// its outline is stroked that thick into the same coverage, over the fill.
pub fn fill(
    path: &tiny_skia::Path,
    scale: f32,
    sub_x: f32,
    grow: f32,
    rule: FillRule,
) -> Option<Mask> {
    const N: u32 = 2;
    let b = path.bounds();
    let left = (b.left() * scale + sub_x - grow).floor() as i32;
    let top = (b.top() * scale - grow).floor() as i32;
    let right = (b.right() * scale + sub_x + grow).ceil() as i32;
    let bottom = (b.bottom() * scale + grow).ceil() as i32;
    // At a scale far past any glyph's the edges saturate, and their difference with them.
    let (w, h) = (right.saturating_sub(left), bottom.saturating_sub(top));
    if w <= 0 || h <= 0 || w >= 2048 || h >= 2048 {
        return None;
    }
    let (width, height) = (w as u32, h as u32);
    let mut mask = SkMask::new(width * N, height * N)?;
    let big = scale * N as f32;
    let (dx, dy) = ((sub_x - left as f32) * N as f32, -(top as f32) * N as f32);
    let transform = Transform::from_row(big, 0.0, 0.0, big, dx, dy);
    mask.fill_path(path, rule, true, transform);
    if grow > 0.0 {
        // The band along the outline, half of it outside: tiny-skia lays coverage over
        // coverage, so the two fills join and nothing is carved from either.
        let band = Stroke {
            width: 2.0 * grow / scale,
            line_cap: LineCap::Round,
            line_join: LineJoin::Round,
            ..Stroke::default()
        };
        if let Some(outline) = path.stroke(&band, big) {
            mask.fill_path(&outline, FillRule::Winding, true, transform);
        }
    }
    let fine = mask.data();
    let (w, fw, n) = (width as usize, (width * N) as usize, N as usize);
    let mut cov = vec![0u8; w * height as usize];
    for (y, row) in cov.chunks_exact_mut(w).enumerate() {
        for (x, out) in row.iter_mut().enumerate() {
            let mut sum = 0usize;
            for j in 0..n {
                let at = (y * n + j) * fw + x * n;
                sum += fine[at..at + n]
                    .iter()
                    .map(|v| usize::from(*v))
                    .sum::<usize>();
            }
            *out = ((sum + n * n / 2) / (n * n)) as u8;
        }
    }
    Some(Mask {
        left,
        top,
        width,
        height,
        cov,
    })
}

/// The box that stands for a character no face has: hollow, a little under a capital's height.
fn tofu(px: f32, sub_x: f32) -> Option<Mask> {
    let (line, mut b) = ((px / 14.0).max(1.0), PathBuilder::new());
    let (x0, x1, y0, y1) = (px * 0.08, px * 0.52, -px * 0.68, 0.0);
    b.push_rect(tiny_skia::Rect::from_ltrb(x0, y0, x1, y1)?);
    b.push_rect(tiny_skia::Rect::from_ltrb(
        x0 + line,
        y0 + line,
        x1 - line,
        y1 - line,
    )?);
    fill(&b.finish()?, 1.0, sub_x, 0.0, FillRule::EvenOdd)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A book of one text family and one monospaced, by name, if this machine has them.
    fn book(text: &str, mono: &str) -> Option<Book> {
        let own = font_dir(None);
        let only = |all: Vec<Family>, name: &str| -> Vec<Family> {
            all.into_iter().filter(|f| f.name == name).collect()
        };
        let (text, mono) = (
            only(text_families(&own), text),
            only(mono_families(&own), mono),
        );
        Book::of(text, mono, &own).ok()
    }

    #[test]
    fn every_family_this_machine_has_sets_and_draws_text() {
        let pairs = [
            ("SF", "SF Mono"),
            ("Helvetica Neue", "Menlo"),
            ("Helvetica", "Menlo"),
            ("Arial", "Menlo"),
            ("DejaVu Sans", "DejaVu Sans Mono"),
            ("Noto Sans", "Noto Sans Mono"),
            ("Liberation Sans", "Liberation Mono"),
            ("Ioskeley Mono", "Ioskeley Mono"),
        ];
        let mut tried = 0;
        for (text, mono) in pairs {
            let Some(mut book) = book(text, mono) else {
                continue;
            };
            tried += 1;
            let mut pass = book.pass();
            for (mono, weight, italic) in [
                (false, Weight::Regular, false),
                (false, Weight::Medium, false),
                (false, Weight::Semibold, true),
                (true, Weight::Regular, false),
                (true, Weight::Semibold, true),
            ] {
                let style = Style {
                    mono,
                    weight,
                    italic,
                };
                // Letters, a symbol the face may lack, and one nothing has.
                for size in [12.0, 16.0, 32.0] {
                    let mut run = pass.run(style, size, 2.0);
                    for c in "To Wav. ✓ → \u{10ffff}".chars() {
                        let shaped = pass.next(&mut run, c);
                        let ok = shaped.advance.is_finite() && shaped.advance + shaped.kern >= 0.0;
                        assert!(ok, "{text}/{mono} {style:?} at {size}: {c:?} is {shaped:?}");
                        if c == 'a' {
                            // Grown, the letter has more ink than plain, and not much more.
                            let (inst, id, px) = (shaped.inst, shaped.id, size * 2.0);
                            let ink = |m: Option<&Mask>| -> usize {
                                m.map_or(0, |m| m.cov.iter().map(|v| usize::from(*v)).sum())
                            };
                            let plain = ink(pass.mask(inst, id, px, 1, 2, 0.0));
                            let grown = ink(pass.mask(inst, id, px, 1, 2, 0.3));
                            assert!(
                                plain > 200,
                                "{text}/{mono} {style:?} at {size}: no ink in an a"
                            );
                            assert!(
                                grown > plain && grown < plain * 2,
                                "{text}/{mono} {style:?} at {size}: {plain} plain, {grown} grown"
                            );
                        }
                    }
                    let tofu = pass.next(&mut run, '\u{10ffff}');
                    assert_eq!(tofu.id, TOFU, "{text}/{mono}: a character no face has");
                    assert!(pass.mask(tofu.inst, TOFU, size * 2.0, 0, 2, 0.3).is_some());
                }
            }
            // The system's face is set as CoreText sets it: these are its advances for body
            // text, kerning and tracking in.
            if text == "SF" {
                let mut run = pass.run(Style::default(), 16.0, 1.0);
                let line = "The quick brown fox jumps over the lazy dog 0123456789";
                let width: f32 = line
                    .chars()
                    .map(|c| pass.next(&mut run, c))
                    .map(|s| s.advance + s.kern)
                    .sum();
                assert!(
                    (width - 420.9531).abs() < 0.05,
                    "CoreText's 420.9531 is {width}"
                );
                // Its cap height is its own, not the 0.7 of an em a face without one takes.
                for size in [11.0, 16.0, 32.0] {
                    let inst = pass.primary(Style::default(), size);
                    let m = pass.metrics(inst);
                    assert_eq!((m.upem, m.cap_height), (2048.0, 1443.0), "at {size}");
                }
            }
        }
        // With no font at all there's nothing to hold; DIE's own is there in a checkout.
        if tried == 0 {
            eprintln!("skipped: no font on this machine");
        }
    }

    /// How each character of a run over `text` is set, at 11 px and a scale of 2.
    fn shaped(pass: &mut Pass<'_>, text: &str, style: Style, tabular: bool) -> Vec<Shaped> {
        let mut run = pass.run(style, 11.0, 2.0).tabular(tabular);
        text.chars().map(|c| pass.next(&mut run, c)).collect()
    }

    #[test]
    fn tabular_figures_are_one_width_and_never_kerned() {
        let italic = Style {
            italic: true,
            ..Style::default()
        };
        let strong = Style {
            weight: Weight::Semibold,
            ..Style::default()
        };
        let mono = Style {
            mono: true,
            ..Style::default()
        };
        let pairs = [
            ("SF", "SF Mono"),
            ("Helvetica Neue", "Menlo"),
            ("Helvetica", "Menlo"),
            ("Arial", "Menlo"),
            ("DejaVu Sans", "DejaVu Sans Mono"),
            ("Noto Sans", "Noto Sans Mono"),
            ("Liberation Sans", "Liberation Mono"),
            ("Ioskeley Mono", "Ioskeley Mono"),
        ];
        let mut tried = 0;
        for (text, mono_name) in pairs {
            let Some(mut book) = book(text, mono_name) else {
                continue;
            };
            tried += 1;
            let mut pass = book.pass();
            // In every face, with tnum or without: each digit one width.
            for style in [Style::default(), italic, strong, mono] {
                let figures = shaped(&mut pass, "0123456789", style, true);
                let wide = figures[0].advance + figures[0].kern;
                for (d, s) in figures.iter().enumerate() {
                    let near = (s.advance + s.kern - wide).abs() < 1e-4;
                    assert!(
                        near,
                        "{text}/{mono_name} {style:?}: {d} is {s:?}, 0 is {wide}"
                    );
                }
            }
            // A monospace's digits are tabular already, and set the same either way.
            let (plain, figures) = (
                shaped(&mut pass, "0123456789", mono, false),
                shaped(&mut pass, "0123456789", mono, true),
            );
            for (p, f) in plain.iter().zip(&figures) {
                assert_eq!((p.id, p.advance, p.kern), (f.id, f.advance, f.kern));
            }
            if text == "SF" {
                // SF's own digits are proportional, '1' 950 units to '0''s 1290; its tnum
                // glyphs are all 1290 at the regular weight, tracked +12 at 11 px (opsz 17),
                // and 1328 in the italic.
                let unit = 22.0 / 2048.0;
                let one = shaped(&mut pass, "1", Style::default(), false)[0];
                assert!(
                    (one.advance - (950.0 + 12.0) * unit).abs() < 1e-3,
                    "{one:?}"
                );
                let upright = shaped(&mut pass, "1", Style::default(), true)[0];
                let want = (1290.0 + 12.0) * unit;
                assert!(
                    (upright.advance - want).abs() < 1e-3,
                    "{upright:?}, not {want}"
                );
                let slanted = shaped(&mut pass, "1", italic, true)[0];
                assert!((slanted.advance - (1328.0 + 12.0) * unit).abs() < 1e-3);
                // No kerning into, out of or between figures, where proportional digits kern.
                let money = shaped(&mut pass, "$1,111.25%", Style::default(), true);
                assert!(money.iter().all(|s| s.kern == 0.0), "{money:?}");
                let kerned = shaped(&mut pass, "1,111.25", Style::default(), false);
                assert!(kerned.iter().any(|s| s.kern != 0.0));
            }
            if text == "Helvetica" {
                // Its digits are one width already, but its kern table pulls "11" together.
                let plain = shaped(&mut pass, "11", Style::default(), false);
                let figures = shaped(&mut pass, "11", Style::default(), true);
                assert!(plain[1].kern < 0.0, "{plain:?}");
                assert_eq!(figures[1].kern, 0.0);
            }
        }
        if tried == 0 {
            eprintln!("skipped: no font on this machine");
        }
    }

    #[test]
    fn a_shape_too_big_or_at_a_scale_that_is_no_number_has_no_coverage() {
        let mut b = PathBuilder::new();
        b.push_rect(tiny_skia::Rect::from_ltrb(0.0, -700.0, 600.0, 200.0).unwrap());
        let path = b.finish().unwrap();
        let mask = fill(&path, 0.02, 0.0, 0.3, FillRule::Winding).unwrap();
        assert_eq!((mask.width, mask.height), (14, 20));
        for scale in [5e26, 1e30, f32::INFINITY, f32::NAN, -1.0] {
            let none = fill(&path, scale, 0.0, 0.3, FillRule::Winding);
            assert!(none.is_none(), "at {scale}");
        }
    }

    #[test]
    fn the_bundle_rule_reads_the_app_from_the_executables_path() {
        let exe = Path::new("/Applications/DIE.app/Contents/MacOS/die");
        assert_eq!(bundle_of(exe), Some(PathBuf::from("/Applications/DIE.app")));
        assert_eq!(bundle_of(Path::new("/usr/local/bin/die")), None);
        assert_eq!(font_dir(Some(Path::new("/x"))), PathBuf::from("/x"));
    }
}
