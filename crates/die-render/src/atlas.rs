//! The glyph atlas: where each glyph's pixels live, and the uploads that put them
//! there. A shelf packer over two planes, as data. The backend copies the uploads
//! into its textures, and neutral tests read them.
//!
//! Glyphs stay until the atlas is full, then the whole atlas starts over. Evicting
//! one glyph at a time waits for M3.
#![warn(missing_docs)]

use crate::frame::{GlyphBitmap, Plane, Upload};
use crate::sprite::Line;
use die_core::Glyph;
use std::collections::HashMap;

/// What the atlas caches a bitmap under.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum GlyphKey {
    /// A cell's glyph in a style, fitted to `span` cells.
    Text {
        /// The glyph, never `Glyph::Continuation`.
        glyph: Glyph,
        /// Bold.
        bold: bool,
        /// Italic.
        italic: bool,
        /// The cells it takes.
        span: u16,
    },
    /// A line over a cell.
    Line(Line),
}

/// Where a glyph is in the atlas, and where it goes in its cell.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Slot {
    /// The atlas.
    pub plane: Plane,
    /// The left edge, in atlas pixels.
    pub x: u32,
    /// The top edge, in atlas pixels.
    pub y: u32,
    /// The width in pixels.
    pub w: u32,
    /// The height in pixels.
    pub h: u32,
    /// The left edge, in pixels from the cell's left edge.
    pub left: i32,
    /// The top edge, in pixels from the cell's top edge.
    pub top: i32,
}

/// One plane's packer: rows of glyphs, filled left to right. A pixel of space between
/// glyphs keeps one from showing at another's edge.
struct Shelves {
    size: u32,
    x: u32,
    y: u32,
    row_h: u32,
}

impl Shelves {
    fn new(size: u32) -> Shelves {
        Shelves {
            size,
            x: 0,
            y: 0,
            row_h: 0,
        }
    }

    fn place(&mut self, w: u32, h: u32) -> Option<(u32, u32)> {
        let (mut x, mut y, mut row_h) = (self.x, self.y, self.row_h);
        if x + w > self.size {
            (x, y, row_h) = (0, y + row_h + 1, 0);
        }
        if y + h > self.size {
            return None;
        }
        self.x = x + w + 1;
        self.y = y;
        self.row_h = row_h.max(h);
        Some((x, y))
    }
}

/// There's no room left for this bitmap, until `Atlas::clear` makes some.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Full(pub GlyphBitmap);

/// The glyph cache over its two planes.
pub struct Atlas {
    gray: Shelves,
    color: Shelves,
    slots: HashMap<GlyphKey, Option<Slot>>,
    uploads: Vec<Upload>,
}

impl Atlas {
    /// An empty atlas with square planes of these sizes, in pixels.
    pub fn new(gray: u32, color: u32) -> Atlas {
        Atlas {
            gray: Shelves::new(gray),
            color: Shelves::new(color),
            slots: HashMap::new(),
            uploads: vec![],
        }
    }

    /// The glyph's slot. The outer `None` means nobody has supplied its bitmap yet,
    /// and the inner `None` means it draws nothing.
    pub fn get(&self, key: &GlyphKey) -> Option<Option<Slot>> {
        self.slots.get(key).copied()
    }

    /// Note what `key` draws. `None` means nothing: a space, or a glyph dropped
    /// for lack of room. A bitmap with no pixels is the same, and so is one bigger
    /// than its whole plane, which no clearing would make room for.
    pub fn insert(&mut self, key: &GlyphKey, bitmap: Option<GlyphBitmap>) -> Result<(), Full> {
        let size = |plane| match plane {
            Plane::Gray => self.gray.size,
            Plane::Color => self.color.size,
        };
        let drawn =
            bitmap.filter(|b| b.w > 0 && b.h > 0 && b.w <= size(b.plane) && b.h <= size(b.plane));
        let Some(b) = drawn else {
            self.slots.insert(key.clone(), None);
            return Ok(());
        };
        let shelves = match b.plane {
            Plane::Gray => &mut self.gray,
            Plane::Color => &mut self.color,
        };
        let Some((x, y)) = shelves.place(b.w, b.h) else {
            return Err(Full(b));
        };
        let slot = Slot {
            plane: b.plane,
            x,
            y,
            w: b.w,
            h: b.h,
            left: b.left,
            top: b.top,
        };
        self.slots.insert(key.clone(), Some(slot));
        self.uploads.push(Upload {
            plane: b.plane,
            x,
            y,
            w: b.w,
            h: b.h,
            data: b.data,
        });
        Ok(())
    }

    /// Forget every glyph. What's waiting to upload no longer matters.
    pub fn clear(&mut self) {
        self.gray = Shelves::new(self.gray.size);
        self.color = Shelves::new(self.color.size);
        self.slots.clear();
        self.uploads.clear();
    }

    /// The pixels placed since the last call, for the backend to copy.
    pub fn take_uploads(&mut self) -> Vec<Upload> {
        std::mem::take(&mut self.uploads)
    }
}
