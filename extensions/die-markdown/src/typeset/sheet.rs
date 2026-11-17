//! The sheet: GitHub's geometry as numbers, in CSS pixels at a 16 pixel base, every `em` already
//! resolved. The layout multiplies by the display's scale; nothing here is a colour.

/// The column's widest border box, and the narrowest it's laid out for however narrow the area.
pub(super) const COLUMN: f32 = 980.0;
pub(super) const COLUMN_MIN: f32 = 200.0;
/// The padding round the column, and what it shrinks to in an area under [`NARROW`] wide.
pub(super) const PAD: f32 = 45.0;
pub(super) const PAD_NARROW: f32 = 15.0;
pub(super) const NARROW: f32 = 768.0;

/// Body text and its line.
pub(super) const TEXT: f32 = 16.0;
pub(super) const TEXT_LINE: f32 = 1.5;
/// The space under a block, and above a heading.
pub(super) const GAP: f32 = 16.0;
pub(super) const HEADING_ABOVE: f32 = 24.0;

/// Headings, first level to sixth: their sizes, on lines 1.25 times as tall.
pub(super) const HEADING: [f32; 6] = [32.0, 24.0, 20.0, 16.0, 14.0, 13.6];
pub(super) const HEADING_LINE: f32 = 1.25;
/// The space between a first or second level heading's text and its rule, in ems.
pub(super) const HEADING_RULE_PAD: f32 = 0.3;

/// Code is 85% of the text round it; a block's lines are 1.45 times that, in a panel padded
/// all round.
pub(super) const CODE: f32 = 0.85;
pub(super) const CODE_LINE: f32 = 1.45;
pub(super) const CODE_PAD: f32 = 16.0;
/// The radius of a panel, a pill and a key cap.
pub(super) const RADIUS: f32 = 6.0;
/// Inline code's padding in ems of its own size: across, and up and down.
pub(super) const PILL_X: f32 = 0.4;
pub(super) const PILL_Y: f32 = 0.2;
/// In a heading inline code has the heading's size and no padding up or down.
pub(super) const PILL_X_HEADING: f32 = 0.2;

/// A key cap: its letters, the padding beside them, and its box's height.
pub(super) const KBD: f32 = 11.0;
pub(super) const KBD_PAD: f32 = 5.0;
pub(super) const KBD_HEIGHT: f32 = 18.0;

/// Raised and lowered text: 75% as large, moved by half and by a quarter of its own size.
pub(super) const SCRIPT: f32 = 0.75;
pub(super) const SUP_RISE: f32 = 0.5;
pub(super) const SUB_DROP: f32 = 0.25;

/// A quote: its bar, and the padding either side of its text.
pub(super) const QUOTE_BAR: f32 = 4.0;
pub(super) const QUOTE_PAD: f32 = 16.0;
/// An alert's padding above and below, and its title's icon with the space after it.
pub(super) const ALERT_PAD: f32 = 8.0;
pub(super) const ICON: f32 = 16.0;
pub(super) const ICON_GAP: f32 = 8.0;

/// A list's indent a level, and the space above every item but the first.
pub(super) const INDENT: f32 = 32.0;
pub(super) const ITEM_GAP: f32 = 4.0;
/// A task's box: its side, and how far its right edge is from the item's text.
pub(super) const CHECK: f32 = 13.0;
pub(super) const CHECK_GAP: f32 = 8.0;
pub(super) const CHECK_RADIUS: f32 = 3.0;

/// A table cell's padding: across, and up and down; and the least across, which a table too
/// wide for the page keeps rather than cut a word.
pub(super) const CELL_X: f32 = 13.0;
pub(super) const CELL_X_LEAST: f32 = 4.0;
pub(super) const CELL_Y: f32 = 6.0;
/// The narrowest a column's text gets when a table can't fit, in ems.
pub(super) const CELL_FLOOR: f32 = 3.0;

/// A rule: its thickness, and the space either side.
pub(super) const RULE: f32 = 4.0;
pub(super) const RULE_GAP: f32 = 24.0;

/// The footnotes: their text and its line, and the indent their numbers hang in.
pub(super) const NOTE: f32 = 12.0;
pub(super) const NOTE_INDENT: f32 = 16.0;

/// A picture that can't be shown: its box's padding across, its height, and the space between
/// its mark and its words.
pub(super) const ABSENT_PAD: f32 = 8.0;
/// Shorter than the line by a couple of pixels, so a row of boxes doesn't touch the next.
pub(super) const ABSENT_HEIGHT: f32 = 22.0;
pub(super) const ABSENT_GAP: f32 = 6.0;

/// The ring round the link the keyboard is on: its thickness, how far out it stands, its radius.
pub(super) const RING: f32 = 2.0;
pub(super) const RING_OUT: f32 = 2.0;
pub(super) const RING_RADIUS: f32 = 4.0;

/// How far under the baseline a link's underline starts.
pub(super) const UNDERLINE: f32 = 3.2;

/// How far each glyph's outline grows a side, in CSS pixels. A Mac browser draws GitHub with
/// font smoothing on, which lays down about a quarter more ink than plain coverage; growing the
/// outline by this much reproduces that weight for every face. Zero is DIE's own unsmoothed
/// weight, the editor's beside the page.
pub(super) const GROW: f32 = 0.15;

/// A hairline's thickness in physical pixels: one CSS pixel, as a browser draws GitHub's rules,
/// grid and borders, in whole pixels so it's never blurred, and never under one.
pub(super) fn hairline(scale: f32) -> f32 {
    stroke(scale)
}

/// A line that's a CSS pixel thick, in whole physical pixels: a checkbox's outline, a ring
/// bullet, an underline.
pub(super) fn stroke(scale: f32) -> f32 {
    scale.round().max(1.0)
}
