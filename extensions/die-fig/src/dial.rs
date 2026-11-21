//! The die-fig dial, the context menu: a port of the whiteboard's (`die-whiteboard/src/dial.rs`,
//! drawn by its `canvas.rs`) drawn into a [`Scene`], all its text in pixels. Its geometry,
//! constants, look, flick, dead zone, close and pick rules are the whiteboard's; its eight
//! sectors are north detail, north-east compare, east only, south-east table, south view,
//! south-west export, west hide and north-west settings (briefs/mouse-ux.md §7.3), each dimmed
//! where it doesn't apply to the target, and the three in the lower-left half open a stage two
//! whose choices are words: view's time ranges, export's formats and settings' toggles.
//!
//! **Stage one** is a ring of [`RING`] cell heights about the anchor with the names outside it.
//! Moving past the dead zone (a cell's larger side) hovers the sector in that direction;
//! letting go picks it, or in the dead zone, or on a sector that doesn't apply, nothing. A left
//! press while it's open picks too, at whichever stage. **Stage two** opens when the pointer
//! goes [`OPENS`] past the ring with a sector hovered that has choices and applies, on a move
//! after the one that hovered it, so a flick that hovers a sector and lands past the ring at
//! once is stage one's: an arc [`ARC`] beyond the ring, [`SEGMENT`]° a choice clockwise, placed
//! so the current choice ([`DialState::centre_on`]) is centred on the sector's direction and
//! pushing straight out keeps it, with each choice's word outside the arc at its segment's
//! middle and the sector's chip beyond them. Its first band, [`BAND`] deep, picks nothing
//! ([`Stage::Arc`]), so a flick that overshoots the ring changes nothing; past its guide the
//! pointer's angle picks the choice, and past either end of the arc that end's. Going back
//! inside the ring is stage one again. The whiteboard's distance bands (its widths) have no
//! counterpart: angle alone picks, as for its text tool.
//!
//! The geometry is in the scene's logical px, sized from the cell, so it's the same on screen
//! at any figure's size. [`layout`] places the names, the labels, the chip and the readout
//! (measured through [`Measure`], so tests need no fonts) clear of the line art and of each
//! other wherever the anchor is; [`draw`] draws the line art over a scrim of the surface, and
//! the text over that, with a [`Region`](crate::scene::Region) for each name, each label and
//! the dead centre. What stays while the pointer moves ([`draw_still`]) and what follows it
//! ([`draw_live`]) can be drawn apart, so a surface can keep the first. The surface owns a
//! [`DialState`] while it's open; nothing changes until it picks.

use crate::scene::{
    Anchor, Cap, Hit, Item, Join, Measure, Paint, PathData, Rect, Scene, Stroke, TextSpec, Verb,
    Weight,
};
use crate::theme::{Color, Theme};

/// The ring's radius, in cell heights.
pub const RING: f32 = 2.5;
/// How far past the ring the pointer goes to open stage two, in cell heights.
pub const OPENS: f32 = 0.5;
/// How far beyond the ring stage two's arc is, in cell heights.
pub const ARC: f32 = 5.0;
/// How deep stage two's first band is, the one that picks nothing, in cell heights.
pub const BAND: f32 = 1.25;
/// Each choice's share of the arc, in degrees.
pub const SEGMENT: f32 = 20.0;
/// The dial's text size, in cell heights: 12 px at 20 px cells.
pub const TEXT: f32 = 0.6;

/// The line art's sizes, in logical px, the whiteboard's.
pub mod art {
    /// A hairline's width.
    pub const HAIR: f32 = 1.0;
    /// The hovered stretch of the ring, and the picked choice's segment, drawn thick.
    pub const THICK: f32 = 3.0;
    /// How far a tick reaches either side of its line.
    pub const TICK: f32 = 4.0;
    /// How far a T cap reaches either side of the arc.
    pub const CAP: f32 = 6.0;
    /// The needle's end dot's radius.
    pub const DOT: f32 = 3.0;
    /// The reticle's radius, at the centre.
    pub const RETICLE: f32 = 4.0;
    /// Between the line art and the text, and between two texts above one another.
    pub const CLEAR: f32 = 3.0;
}

/// The dial's opacities, the whiteboard's, and its text's.
pub mod alpha {
    /// The scrim from the anchor most of the way to the names' farthest.
    pub const SCRIM: f32 = 0.88;
    /// The scrim a cell inside its edge, where the names end; nothing at the edge.
    pub const SCRIM_MID: f32 = 0.7;
    /// The ring and its ticks in stage two.
    pub const RING_DIM: f32 = 0.25;
    /// The ring in stage one.
    pub const RING: f32 = 0.6;
    /// The ring's ticks in stage one, and the arc's ticks and caps.
    pub const TICK: f32 = 0.8;
    /// The dead zone's dotted circle.
    pub const DEAD: f32 = 0.3;
    /// Stage two's guide, where the angle starts to pick.
    pub const GUIDE: f32 = 0.3;
    /// The crosshair through the pointer, in the accent.
    pub const CROSS: f32 = 0.25;
    /// The needle and the reticle.
    pub const NEEDLE: f32 = 0.9;
    /// The names in stage two, and the readout with nothing hovered: the cells' faint.
    pub const FAINT: f32 = 0.5;
    /// A sector that doesn't apply to the target, its name and its readout.
    pub const DIM: f32 = 0.35;
}

/// A dotted line's dots and gaps, in logical px: a dot is the line's width across.
const DOT_ON: f32 = 0.1;
const DOT_OFF: f32 = 3.0;
/// A dotted line's width, in hairlines.
const DOTTED: f32 = 1.3;
/// How far, in physical px, each of the scrim's rings reaches over the edge of the one outside
/// it, so the two edges' antialiasing meets without a light seam or a dark one.
const SEAM: f32 = 0.25;
/// The most steps of a px a name or a label moves out to clear what's placed before it.
const MOVES: usize = 4096;

/// The sectors' indices, north first then clockwise.
pub mod sector {
    /// North: the target's detail (⏎).
    pub const DETAIL: u8 = 0;
    /// North-east: the reference here (m).
    pub const COMPARE: u8 = 1;
    /// East: only the target's series (o).
    pub const ONLY: u8 = 2;
    /// South-east: the data as a table at the target's row (t).
    pub const TABLE: u8 = 3;
    /// South: fit the view (0), or past the ring a time range.
    pub const VIEW: u8 = 4;
    /// South-west: export (E), past the ring the format.
    pub const EXPORT: u8 = 5;
    /// West: hide the target's series (x).
    pub const HIDE: u8 = 6;
    /// North-west: settings (s), past the ring a toggle.
    pub const SETTINGS: u8 = 7;
}

/// The sectors' names, north first then clockwise.
pub const NAMES: [&str; 8] = [
    "detail", "compare", "only", "table", "view", "export", "hide", "settings",
];
/// View's stage two: the time ranges.
pub const VIEW: [&str; 5] = ["1h", "6h", "24h", "7d", "all"];
/// Export's stage two: the formats.
pub const EXPORT: [&str; 3] = ["png", "svg", "copy"];
/// Settings' stage two: the toggles.
pub const SETTINGS: [&str; 5] = ["grid", "labels", "log", "stack", "markers"];

/// The unit vector `deg` degrees clockwise from north (up: negative y).
pub fn toward(deg: f32) -> (f32, f32) {
    let a = deg.to_radians();
    (a.sin(), -a.cos())
}

/// The angle of the vector (dx, dy) from north, clockwise, in degrees, in [0, 360).
pub fn angle_of(dx: f32, dy: f32) -> f32 {
    dx.atan2(-dy).to_degrees().rem_euclid(360.0)
}

/// `deg` brought into (-180, 180].
fn wrap(deg: f32) -> f32 {
    let d = deg.rem_euclid(360.0);
    if d > 180.0 { d - 360.0 } else { d }
}

fn dist(a: (f32, f32), b: (f32, f32)) -> f32 {
    (a.0 - b.0).hypot(a.1 - b.1)
}

/// The point `r` out from `c`, `deg` clockwise from north.
fn out(c: (f32, f32), deg: f32, r: f32) -> (f32, f32) {
    let u = toward(deg);
    (c.0 + u.0 * r, c.1 + u.1 * r)
}

/// Which stage is open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    /// The sectors, by direction.
    Sectors,
    /// The hovered sector's arc open, the pointer in its first band: nothing picked yet, so
    /// letting go here is stage one's.
    Arc,
    /// The hovered sector's arc, the pointer past its first band: the choice its angle picks,
    /// an index into the sector's `stage2`.
    Choice(u8),
}

/// An open dial: its anchor, its stage, the sector hovered, stage two's choice, and what's
/// centred and what the readout says with nothing hovered. Positions are the scene's logical px.
#[derive(Clone, Debug, PartialEq)]
pub struct DialState {
    anchor: (f32, f32),
    cell: (f32, f32),
    hovered: Option<u8>,
    pointer: (f32, f32),
    stage: Stage,
    /// Each sector's choice to centre on its direction; None centres the middle one.
    current: [Option<u8>; 8],
    /// What the readout says with nothing hovered: the target.
    target: Option<String>,
}

/// What the dial offers here.
#[derive(Clone, Debug, PartialEq)]
pub struct DialSpec {
    /// The sectors, north first then clockwise.
    pub sectors: [Sector; 8],
}

/// One sector: its name, whether it applies, and its stage-two choices if it has any.
#[derive(Clone, Debug, PartialEq)]
pub struct Sector {
    /// What it says.
    pub name: &'static str,
    /// Whether it applies to the target; dimmed if not.
    pub enabled: bool,
    /// Its stage-two choices, by angle along the arc.
    pub stage2: Vec<&'static str>,
}

/// What a release or a press picked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DialPick {
    /// Nothing: the dead zone, a dimmed sector, or the dial stays open.
    None,
    /// A sector.
    Sector(u8),
    /// A sector's stage-two choice.
    Stage2(u8, u8),
}

impl Sector {
    /// A sector that applies, with no stage two.
    pub fn new(name: &'static str) -> Sector {
        Sector {
            name,
            enabled: true,
            stage2: Vec::new(),
        }
    }

    /// A sector that applies, with these choices past the ring, clockwise.
    pub fn with(name: &'static str, stage2: &[&'static str]) -> Sector {
        Sector {
            stage2: stage2.to_vec(),
            ..Sector::new(name)
        }
    }

    /// The same, applying to the target or not.
    pub fn applies(self, enabled: bool) -> Sector {
        Sector { enabled, ..self }
    }

    /// Whether it opens a stage two: it applies and has choices.
    pub fn has_arc(&self) -> bool {
        self.enabled && !self.stage2.is_empty()
    }
}

impl DialSpec {
    /// A chart's dial, every sector applying: [`NAMES`], with [`VIEW`], [`EXPORT`] and
    /// [`SETTINGS`] past the ring. A surface dims what doesn't apply to its target.
    pub fn chart() -> DialSpec {
        let s = |i: usize| match i as u8 {
            sector::VIEW => Sector::with(NAMES[i], &VIEW),
            sector::EXPORT => Sector::with(NAMES[i], &EXPORT),
            sector::SETTINGS => Sector::with(NAMES[i], &SETTINGS),
            _ => Sector::new(NAMES[i]),
        };
        DialSpec {
            sectors: std::array::from_fn(s),
        }
    }

    fn has_arc(&self, i: u8) -> bool {
        self.sectors
            .get(usize::from(i))
            .is_some_and(Sector::has_arc)
    }

    fn applies(&self, i: u8) -> bool {
        self.sectors.get(usize::from(i)).is_some_and(|s| s.enabled)
    }
}

/// What letting go would do, as the whiteboard's readout says it: `key: value`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Readout {
    /// The sector, with its colon if there's a value, in the dial's grey; with nothing hovered,
    /// the target.
    pub key: String,
    /// Stage two's choice, in the accent.
    pub value: String,
    /// Nothing hovered, or a sector that doesn't apply: all of it in the dial's grey, faint.
    pub dim: bool,
}

impl Readout {
    /// It as one line.
    pub fn line(&self) -> String {
        if self.value.is_empty() {
            self.key.clone()
        } else {
            format!("{} {}", self.key, self.value)
        }
    }
}

impl DialState {
    /// A dial opened at `anchor`, for cells of `cell` logical px, nothing hovered yet.
    pub fn open(anchor: (f32, f32), cell: (f32, f32)) -> DialState {
        let fine = |v: f32, or: f32| if v.is_finite() && v > 0.0 { v } else { or };
        DialState {
            anchor,
            cell: (fine(cell.0, 10.0), fine(cell.1, 20.0)),
            hovered: None,
            pointer: anchor,
            stage: Stage::Sectors,
            current: [None; 8],
            target: None,
        }
    }

    /// Where it opened: its centre.
    pub fn anchor(&self) -> (f32, f32) {
        self.anchor
    }

    /// The cell it's sized from, in logical px.
    pub fn cell(&self) -> (f32, f32) {
        self.cell
    }

    /// Where the pointer is: the needle's end.
    pub fn pointer(&self) -> (f32, f32) {
        self.pointer
    }

    /// The sector under the pointer, north 0 then clockwise, or None in the dead zone. In stage
    /// two, the sector whose arc it is.
    pub fn hovered(&self) -> Option<u8> {
        self.hovered
    }

    /// The stage open.
    pub fn stage(&self) -> Stage {
        self.stage
    }

    /// The ring's radius: [`RING`] cell heights.
    pub fn radius(&self) -> f32 {
        RING * self.cell.1
    }

    /// The dead zone's radius: a cell's larger side.
    pub fn dead(&self) -> f32 {
        self.cell.0.max(self.cell.1)
    }

    /// The radius past which stage two opens.
    pub fn opens(&self) -> f32 {
        self.radius() + OPENS * self.cell.1
    }

    /// Stage two's guide: the first band's outer edge, past which the angle picks.
    pub fn band(&self) -> f32 {
        self.opens() + BAND * self.cell.1
    }

    /// Stage two's arc's radius.
    pub fn arc_radius(&self) -> f32 {
        self.radius() + ARC * self.cell.1
    }

    /// The sector in the direction of `p`, or None in the dead zone.
    pub fn sector_at(&self, p: (f32, f32)) -> Option<u8> {
        if dist(p, self.anchor) < self.dead() {
            return None;
        }
        let turn = angle_of(p.0 - self.anchor.0, p.1 - self.anchor.1) / 45.0;
        Some(((turn + 0.5).floor() as u32 % 8) as u8)
    }

    /// Centres `sector`'s stage two on `choice` (the time range shown, the format last used),
    /// so pushing straight out keeps it. Unset, the middle choice is centred.
    pub fn centre_on(&mut self, sector: u8, choice: u8) {
        if let Some(c) = self.current.get_mut(usize::from(sector)) {
            *c = Some(choice);
        }
    }

    /// What the readout says with nothing hovered, dimmed: the target (`api · 06:05 · 642`).
    pub fn set_target(&mut self, about: impl Into<String>) {
        self.target = Some(about.into());
    }

    /// Sector `i`'s arc's ends, in degrees clockwise from north, the counter-clockwise first:
    /// its current choice's segment centred on its direction. None if it opens no stage two.
    pub fn span_of(&self, i: u8, spec: &DialSpec) -> Option<(f32, f32)> {
        if !spec.has_arc(i) {
            return None;
        }
        let n = spec.sectors[usize::from(i)].stage2.len();
        let middle = (n - 1) / 2;
        let cur = self.current[usize::from(i)].map_or(middle, |c| usize::from(c).min(n - 1));
        let start = f32::from(i) * 45.0 - (cur as f32 + 0.5) * SEGMENT;
        Some((start, start + n as f32 * SEGMENT))
    }

    /// The hovered sector's arc's ends, if it has one.
    pub fn span(&self, spec: &DialSpec) -> Option<(f32, f32)> {
        self.span_of(self.hovered?, spec)
    }

    /// The hovered sector's choice in the direction of `p`; past the arc, the nearer end's.
    pub fn choice_at(&self, p: (f32, f32), spec: &DialSpec) -> Option<u8> {
        let (start, end) = self.span(spec)?;
        let half = (end - start) / 2.0;
        let a = angle_of(p.0 - self.anchor.0, p.1 - self.anchor.1);
        let rel = wrap(a - (start + half)) + half;
        let n = ((end - start) / SEGMENT).round();
        Some((rel / SEGMENT).floor().clamp(0.0, n - 1.0) as u8)
    }

    /// The pointer moved to `to`: the sector in its direction is hovered, or stage two's choice
    /// under it. A move to where the pointer is already is no move, so a sample repeated at the
    /// release never opens stage two.
    pub fn moved(&mut self, to: (f32, f32), spec: &DialSpec) {
        if to != self.pointer {
            self.track(to, true, spec);
        }
    }

    /// The pointer let go at `at`: as [`DialState::moved`], but stage two doesn't open on it,
    /// so a flick let go where it landed is stage one's. [`DialState::release`] picks where the
    /// last move left it; a surface whose release comes at another place settles there first.
    pub fn settle(&mut self, at: (f32, f32), spec: &DialSpec) {
        self.track(at, false, spec);
    }

    fn track(&mut self, p: (f32, f32), may_open: bool, spec: &DialSpec) {
        let was = (self.hovered, self.stage);
        self.pointer = p;
        let r = dist(p, self.anchor);
        let held = self.stage != Stage::Sectors && r >= self.radius();
        if !held {
            self.hovered = self.sector_at(p);
            self.stage = Stage::Sectors;
        }
        // Stage two opens from stage one on the sector an earlier move hovered: a move that
        // hovers it and lands past the ring at once is a flick, and stage one's.
        let opens = may_open && was == (self.hovered, Stage::Sectors) && r >= self.opens();
        match self.hovered {
            Some(i) if spec.has_arc(i) && (held || opens) => {
                self.stage = if r < self.band() {
                    Stage::Arc
                } else {
                    Stage::Choice(self.choice_at(p, spec).unwrap_or(0))
                };
            }
            _ if held => self.stage = Stage::Sectors,
            _ => {}
        }
    }

    /// The button that opened it came up: what the dial says now. Nothing in the dead zone or
    /// on a sector that doesn't apply; stage one's sector in its first band. The surface closes
    /// the dial whatever it picked.
    pub fn release(&mut self, spec: &DialSpec) -> DialPick {
        match (self.hovered, self.stage) {
            (Some(i), _) if !spec.applies(i) => DialPick::None,
            (Some(i), Stage::Choice(k)) => DialPick::Stage2(i, k),
            (Some(i), _) => DialPick::Sector(i),
            (None, _) => DialPick::None,
        }
    }

    /// A left press while it's open: it picks there, at whichever stage, as letting go there
    /// would ([`DialState::settle`], then [`DialState::release`]). The surface swallows its
    /// drag and release.
    pub fn press(&mut self, at: (f32, f32), spec: &DialSpec) -> DialPick {
        self.settle(at, spec);
        self.release(spec)
    }

    /// What letting go would do: the sector, or `sector: choice` past the first band; with
    /// nothing hovered, the target, dimmed; on a sector that doesn't apply, its name, dimmed.
    pub fn readout(&self, spec: &DialSpec) -> Readout {
        let Some(s) = self.hovered.and_then(|i| spec.sectors.get(usize::from(i))) else {
            return Readout {
                key: self.target.clone().unwrap_or_default(),
                value: String::new(),
                dim: true,
            };
        };
        if let Stage::Choice(k) = self.stage
            && let Some(choice) = s.stage2.get(usize::from(k))
        {
            return Readout {
                key: format!("{}:", s.name),
                value: (*choice).to_string(),
                dim: false,
            };
        }
        Readout {
            key: s.name.to_string(),
            value: String::new(),
            dim: !s.enabled,
        }
    }

    /// The dial with the pointer at its anchor: what [`draw_still`] draws depends on nothing
    /// else, so a surface that keeps that drawing compares these.
    pub fn still(&self) -> DialState {
        DialState {
            pointer: self.anchor,
            ..self.clone()
        }
    }

    /// The scrim's radius: a cell past the farthest thing drawn.
    pub fn scrim(&self, l: &DialLayout) -> f32 {
        l.reach + self.cell.1
    }

    /// The scrim's square, and the chip where stage two puts it past the scrim: what
    /// [`draw_still`] draws in.
    pub fn base_bounds(&self, l: &DialLayout) -> Rect {
        let s = self.scrim(l);
        let square = Rect::new(self.anchor.0 - s, self.anchor.1 - s, 2.0 * s, 2.0 * s);
        match &l.chip {
            Some(chip) => square.union(chip),
            None => square,
        }
    }

    /// Everything the dial draws: the base's bounds and the pointer's dot wherever it is.
    pub fn bounds(&self, l: &DialLayout) -> Rect {
        let d = art::DOT + 2.0;
        let (x, y) = self.pointer;
        self.base_bounds(l)
            .union(&Rect::new(x - d, y - d, 2.0 * d, 2.0 * d))
    }

    /// Where [`draw_live`] draws: the crosshair's two strips across the bounds, and the
    /// needle's box from the anchor to the pointer's dot.
    pub fn live_rects(&self, l: &DialLayout) -> [Rect; 3] {
        let b = self.bounds(l);
        let (p, c, e) = (self.pointer, self.anchor, art::HAIR);
        let pad = (art::DOT + 2.0).max(art::TICK);
        let (x0, y0) = (c.0.min(p.0), c.1.min(p.1));
        let needle = Rect::new(x0, y0, (c.0 - p.0).abs(), (c.1 - p.1).abs()).inflate(pad);
        [
            Rect::new(b.x, p.1 - e, b.w, 2.0 * e),
            Rect::new(p.0 - e, b.y, 2.0 * e, b.h),
            needle,
        ]
    }
}

/// Where the dial's text goes, in the scene's logical px.
#[derive(Clone, Debug, PartialEq)]
pub struct DialLayout {
    /// Each sector's name in a box a cell tall with a cell either side, where its chip's ends
    /// go. Each hangs off the ring by the end that faces the anchor: north and south centred on
    /// its x, east and west on its y, the diagonals on the line about 45° out. In stage two the
    /// hovered one's is beyond the arc and its labels.
    pub names: [Rect; 8],
    /// Which names show: all in stage one; in stage two the chip, and the others the arc, its
    /// ticks and caps, its guide, a label and the chip leave clear.
    pub shown: [bool; 8],
    /// Stage two's labels: each choice, and its word's box, outside the arc at its segment's
    /// middle, clear of the arc's caps and of the labels before it.
    pub labels: Vec<(u8, Rect)>,
    /// The hovered name's chip, if its sector applies.
    pub chip: Option<Rect>,
    /// The readout's box, a cell tall: under everything with a blank line between, or over it
    /// when the area ends first, and inside the area's sides. None when it says nothing.
    pub readout: Option<Rect>,
    /// The farthest from the anchor the scrim must reach: everything drawn but stage two's
    /// chip, which is solid.
    pub reach: f32,
}

/// How far `r`'s nearest point is from `c`.
fn near(c: (f32, f32), r: &Rect) -> f32 {
    let dx = (r.x - c.0).max(0.0).max(c.0 - r.right());
    let dy = (r.y - c.1).max(0.0).max(c.1 - r.bottom());
    dx.hypot(dy)
}

/// How far `r`'s farthest corner is from `c`.
fn far(c: (f32, f32), r: &Rect) -> f32 {
    let dx = (r.x - c.0).abs().max((r.right() - c.0).abs());
    let dy = (r.y - c.1).abs().max((r.bottom() - c.1).abs());
    dx.hypot(dy)
}

/// Whether two boxes come within `gx` of each other across and `gy` up and down.
fn meets(a: &Rect, b: &Rect, gx: f32, gy: f32) -> bool {
    a.x < b.right() + gx && b.x < a.right() + gx && a.y < b.bottom() + gy && b.y < a.bottom() + gy
}

/// The least `k` from 0 for which `ok` holds, for an `ok` that holds from some `k` on and holds
/// at `hi`, found by halves.
fn least(ok: impl Fn(f32) -> bool, hi: f32) -> f32 {
    if ok(0.0) {
        return 0.0;
    }
    let (mut lo, mut hi) = (0.0, hi.max(1.0));
    for _ in 0..64 {
        if ok(hi) {
            break;
        }
        hi *= 2.0;
    }
    for _ in 0..32 {
        let mid = (lo + hi) / 2.0;
        if ok(mid) {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    hi
}

/// Sector `i`'s box of `size`, hung off a circle `clear` about `c` by the end that faces it:
/// north and south centred on `c`'s x, east and west on its y, the diagonals on the line about
/// 45° out (the whiteboard's `Dial::place`, a px at a time rather than a cell). It moves out
/// until no point of it is within `clear` of `c` and it's `free`.
fn place(
    c: (f32, f32),
    i: usize,
    size: (f32, f32),
    clear: f32,
    free: impl Fn(&Rect) -> bool,
) -> Rect {
    let (w, h) = size;
    let u = toward(i as f32 * 45.0);
    let diagonal = c.1 + u.1 * (clear + h / 2.0) - h / 2.0;
    let at = |k: f32| {
        let (x, y) = match i {
            0 => (c.0 - w / 2.0, c.1 - k - h),
            4 => (c.0 - w / 2.0, c.1 + k),
            2 => (c.0 + k, c.1 - h / 2.0),
            6 => (c.0 - k - w, c.1 - h / 2.0),
            1 | 3 => (c.0 + k, diagonal),
            _ => (c.0 - k - w, diagonal),
        };
        Rect::new(x, y, w, h)
    };
    let mut k = least(|k| near(c, &at(k)) >= clear, clear);
    for _ in 0..MOVES {
        if free(&at(k)) {
            break;
        }
        k += 1.0;
    }
    at(k)
}

/// A box of `size` centred on the ray `deg` from `c`, hung off a circle `clear` about `c` by
/// its nearest point, and moved out along the ray until it's `free`: a word at an angle, as a
/// clock's numerals are.
fn hang(
    c: (f32, f32),
    deg: f32,
    clear: f32,
    size: (f32, f32),
    free: impl Fn(&Rect) -> bool,
) -> Rect {
    let (w, h) = size;
    let u = toward(deg);
    let at = |t: f32| Rect::new(c.0 + u.0 * t - w / 2.0, c.1 + u.1 * t - h / 2.0, w, h);
    let mut t = least(|t| near(c, &at(t)) >= clear, clear + w.hypot(h) / 2.0);
    for _ in 0..MOVES {
        if free(&at(t)) {
            break;
        }
        t += 1.0;
    }
    at(t)
}

/// Points on stage two's line art, a px apart or closer: the arc and its guide from `start`
/// to `end`, and the ticks between its `n` segments and the T at each end.
fn arc_art(st: &DialState, start: f32, end: f32, n: usize) -> Vec<(f32, f32)> {
    let c = st.anchor;
    let mut pts = vec![];
    for r in [st.arc_radius(), st.band()] {
        let steps = ((end - start).to_radians() * r).ceil().max(1.0) as usize;
        pts.extend((0..=steps).map(|k| out(c, start + (end - start) * k as f32 / steps as f32, r)));
    }
    for k in 0..=n {
        let reach = if k == 0 || k == n {
            art::CAP
        } else {
            art::TICK
        };
        let deg = start + k as f32 * SEGMENT;
        let steps = (2.0 * reach).ceil() as usize;
        let r0 = st.arc_radius() - reach;
        pts.extend((0..=steps).map(|t| out(c, deg, r0 + 2.0 * reach * t as f32 / steps as f32)));
    }
    pts
}

/// The dial's text sizes: the names, labels and readout, and the chip's and picked label's.
fn specs(st: &DialState) -> (TextSpec, TextSpec) {
    let text = TextSpec::new(TEXT * st.cell.1);
    (text, text.weight(Weight::Semibold))
}

/// Where the dial's names, stage two's labels, the chip and the readout go for `st` in an area
/// of `area` logical px, measured by `m`.
///
/// Each name keeps a cell either side for its chip's ends and hangs off the ring by the end
/// that faces the anchor, clear of a tick and a little of the ring and of the names placed
/// before it, so none overlaps the ring, its ticks or another name wherever the anchor is. In
/// stage two each choice's word goes outside the arc at its segment's middle, clear of the
/// arc's caps and a cell across from the words before it; the chip goes beyond, on the
/// sector's direction, clear of the caps and half a line clear of the words; and the other
/// names give way where the arc, its ticks and caps or its guide cross them or a word or the
/// chip meets them, so between the ring and the arc there's only the needle and the guide. The
/// readout goes under everything with a blank line between, or over it when the area ends
/// first, inside the area's sides, so it meets nothing either.
pub fn layout(
    st: &DialState,
    spec: &DialSpec,
    area: (f32, f32),
    m: &mut dyn Measure,
) -> DialLayout {
    let (cw, ch) = st.cell;
    let c = st.anchor;
    let (text, bold) = specs(st);
    // Boxes are measured in the chip's weight, so a chip fits its name's box.
    let mut size = |name: &str| (m.width(name, bold) + 2.0 * cw, ch);
    let sizes: [(f32, f32); 8] = std::array::from_fn(|i| size(spec.sectors[i].name));
    let ring = st.radius() + art::TICK + art::CLEAR;
    let mut names = [Rect::default(); 8];
    for (i, size) in sizes.iter().enumerate() {
        let taken = &names[..i];
        let r = place(c, i, *size, ring, |b| {
            taken.iter().all(|o| !meets(o, b, 0.0, 0.0))
        });
        names[i] = r;
    }
    let mut shown = [true; 8];
    let mut labels: Vec<(u8, Rect)> = vec![];
    let hovered = st.hovered.map(usize::from);
    let mut chip = st
        .hovered
        .filter(|i| spec.applies(*i))
        .map(|i| names[usize::from(i)]);
    // Everything drawn, for the readout under or over it: the ring and its ticks to begin with.
    let ring_reach = st.radius() + art::TICK;
    let mut drawn = vec![Rect::new(
        c.0 - ring_reach,
        c.1 - ring_reach,
        2.0 * ring_reach,
        2.0 * ring_reach,
    )];
    let mut reach = ring_reach;
    let open = st.stage != Stage::Sectors;
    if let (true, Some(i), Some((start, end))) = (open, hovered, st.span(spec)) {
        let choices = &spec.sectors[i].stage2;
        let caps = st.arc_radius() + art::CAP;
        reach = reach.max(caps);
        let mut deg = start;
        while deg <= end + 0.01 {
            let (x, y) = out(c, deg, caps);
            drawn.push(Rect::new(x, y, 0.0, 0.0));
            deg += 5.0;
        }
        let clear = caps + art::CLEAR;
        let line = m.ascent(bold) + m.descent(bold);
        for (k, word) in choices.iter().enumerate() {
            let mid = start + (k as f32 + 0.5) * SEGMENT;
            let w = m.width(word, bold);
            let taken: Vec<Rect> = labels.iter().map(|(_, r)| *r).collect();
            let free = |b: &Rect| taken.iter().all(|o| !meets(o, b, cw, art::CLEAR));
            labels.push((k as u8, hang(c, mid, clear, (w, line), free)));
        }
        let words: Vec<Rect> = labels.iter().map(|(_, r)| *r).collect();
        // The chip a cell across and half a line clear of the words, so it reads as their title.
        let free = |b: &Rect| words.iter().all(|o| !meets(o, b, cw, ch / 2.0));
        names[i] = hang(c, i as f32 * 45.0, clear, sizes[i], free);
        chip = Some(names[i]);
        let art = arc_art(st, start, end, choices.len());
        for j in (0..names.len()).filter(|j| *j != i) {
            let b = names[j].inflate(art::CLEAR);
            let crossed = art.iter().any(|p| b.contains(p.0, p.1));
            let beside = words.iter().any(|w| meets(w, &names[j], cw, art::CLEAR));
            let under = meets(&names[i], &names[j], 0.0, art::CLEAR);
            shown[j] = !(crossed || beside || under);
        }
    }
    for (i, b) in names.iter().enumerate().filter(|(i, _)| shown[*i]) {
        drawn.push(*b);
        // Stage two's chip needs no scrim: it's solid.
        if !(open && hovered == Some(i)) {
            reach = reach.max(far(c, b));
        }
    }
    for (_, b) in &labels {
        drawn.push(*b);
        reach = reach.max(far(c, b));
    }
    let lowest = drawn.iter().map(Rect::bottom).fold(c.1, f32::max);
    let highest = drawn.iter().map(|r| r.y).fold(c.1, f32::min);
    let r = st.readout(spec);
    let readout = (!r.line().is_empty()).then(|| {
        let w = readout_width(&r, text, m);
        let (below, above) = (lowest + ch, highest - 2.0 * ch);
        let y = if below + ch <= area.1 || above < 0.0 {
            below
        } else {
            above
        };
        let x = (c.0 - w / 2.0).min(area.0 - w).max(0.0);
        Rect::new(x, y, w, ch)
    });
    if let Some(b) = &readout {
        reach = reach.max(far(c, b));
    }
    DialLayout {
        names,
        shown,
        labels,
        chip,
        readout,
        reach,
    }
}

/// A piece of the readout: what it is decides its colour.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Piece {
    /// The sector, or the space after its colon: the dial's grey.
    Key,
    /// Stage two's choice: the accent.
    Value,
    /// All of it, with nothing hovered or on a sector that doesn't apply: the grey, faint.
    Dim,
}

/// The readout's pieces in order, as they're drawn.
fn readout_pieces(r: &Readout) -> Vec<(String, Piece)> {
    if r.dim {
        return vec![(r.line(), Piece::Dim)];
    }
    let mut pieces = vec![(r.key.clone(), Piece::Key)];
    if !r.value.is_empty() {
        pieces.push((" ".into(), Piece::Key));
        pieces.push((r.value.clone(), Piece::Value));
    }
    pieces
}

fn readout_width(r: &Readout, text: TextSpec, m: &mut dyn Measure) -> f32 {
    readout_pieces(r)
        .iter()
        .map(|(s, _)| m.width(s, text))
        .sum()
}

/// Draws the dial into `scene`, with its regions: [`draw_still`] then [`draw_live`], laid out
/// for the scene's size.
pub fn draw(
    st: &DialState,
    spec: &DialSpec,
    theme: &Theme,
    scene: &mut Scene,
    m: &mut dyn Measure,
) {
    let l = layout(st, spec, (scene.width, scene.height), m);
    draw_still(st, spec, theme, &l, scene, m);
    draw_live(st, theme, &l, scene);
}

fn stroked(scene: &mut Scene, path: PathData, stroke: Stroke) {
    scene.push(Item::Path {
        path,
        fill: None,
        stroke: Some(stroke),
    });
}

fn filled(scene: &mut Scene, path: PathData, color: Color, alpha: f32) {
    scene.push(Item::Path {
        path,
        fill: Some(Paint::solid(color, alpha)),
        stroke: None,
    });
}

/// A stroke with round ends and corners, as the whiteboard's line art is.
fn round(width: f32, color: Color, alpha: f32) -> Stroke {
    Stroke {
        cap: Cap::Round,
        join: Join::Round,
        ..Stroke::solid(width, color, alpha)
    }
}

/// A stroke with square-cut ends: the hovered stretch and the arc's segments, tick to tick.
fn butt(width: f32, color: Color, alpha: f32) -> Stroke {
    Stroke {
        join: Join::Round,
        ..Stroke::solid(width, color, alpha)
    }
}

/// A dotted line: round dots a little wider than a hairline, three px apart.
fn dotted(color: Color, alpha: f32) -> Stroke {
    Stroke {
        dash: Some((DOT_ON, DOT_OFF)),
        ..round(art::HAIR * DOTTED, color, alpha)
    }
}

/// A tick across the circle of radius `r` about `c` at `deg`, reaching `half` either side.
fn tick(path: &mut PathData, c: (f32, f32), deg: f32, r: f32, half: f32) {
    let (a, b) = (out(c, deg, r - half), out(c, deg, r + half));
    path.verbs.push(Verb::Move(a.0, a.1));
    path.verbs.push(Verb::Line(b.0, b.1));
}

/// A line of text, its `anchor` at `at.0` and its baseline at `at.1`.
fn text_item(s: &str, at: (f32, f32), spec: TextSpec, anchor: Anchor, ink: (Color, f32)) -> Item {
    Item::Text {
        text: s.into(),
        x: at.0,
        y: at.1,
        spec,
        anchor,
        color: ink.0,
        alpha: ink.1,
        max_width: None,
    }
}

/// A circle about `c` counter-clockwise: the clockwise one mirrored across its vertical, so
/// it cuts a hole in a clockwise one under the nonzero rule.
fn circle_ccw(c: (f32, f32), r: f32) -> Vec<Verb> {
    let x = |v: f32| 2.0 * c.0 - v;
    PathData::circle(c.0, c.1, r)
        .verbs
        .into_iter()
        .map(|v| match v {
            Verb::Move(a, b) => Verb::Move(x(a), b),
            Verb::Line(a, b) => Verb::Line(x(a), b),
            Verb::Quad(a, b, p, q) => Verb::Quad(x(a), b, x(p), q),
            Verb::Cubic(a, b, p, q, s, t) => Verb::Cubic(x(a), b, x(p), q, x(s), t),
            Verb::Close => Verb::Close,
        })
        .collect()
}

/// A disc of radius `r` about `c`, or the ring between `r` and `hole` if there is one.
fn annulus(c: (f32, f32), r: f32, hole: f32) -> PathData {
    let mut path = PathData::circle(c.0, c.1, r);
    if hole > 0.0 {
        path.verbs.extend(circle_ccw(c, hole));
    }
    path
}

/// `steps` radii from `from` down to (not including) `to`, evenly.
fn radii(from: f32, to: f32, steps: usize) -> Vec<f32> {
    (0..steps)
        .map(|k| from - (from - to) * k as f32 / steps as f32)
        .collect()
}

/// The scrim: `colour` over what's under the dial in a disc of radius `s` about `c`, at
/// [`alpha::SCRIM`] most of the way to the names' edge a cell inside it, [`alpha::SCRIM_MID`]
/// there and nothing at the edge, straight between, as the whiteboard's stops are. A scene has
/// no radial gradient, so it's a disc for the plateau and a ring for each step of the fades,
/// 8 px at most through the fade to the names' edge and 2 px through the last cell's, each at
/// the opacity of its middle. Each pixel is covered once, as the whiteboard's loop covers it:
/// rings stacked over one another would drift darker with each layer's rounding, a circle
/// showing where each stack ends. Each ring reaches a quarter of a physical px over the edge of
/// the one outside it, so their antialiased edges meet without a light seam.
fn scrim(scene: &mut Scene, c: (f32, f32), s: f32, ch: f32, colour: Color) {
    if s.is_nan() || s < 1.0 {
        return;
    }
    let names = (1.0 - ch / s).clamp(0.0, 1.0);
    let stops = [
        (0.0, alpha::SCRIM),
        (names * 0.7, alpha::SCRIM),
        (names, alpha::SCRIM_MID),
        (1.0, 0.0),
    ];
    let at = |r: f32| {
        let t = r / s;
        let mut last = stops[0];
        for &stop in &stops {
            if t <= stop.0 {
                let span = (stop.0 - last.0).max(1e-6);
                return last.1 + (stop.1 - last.1) * ((t - last.0) / span).clamp(0.0, 1.0);
            }
            last = stop;
        }
        last.1
    };
    let seam = SEAM / if scene.scale > 0.0 { scene.scale } else { 1.0 };
    let (edge, plateau) = (names * s, names * 0.7 * s);
    let last_cell = ((s - edge) / 2.0).ceil().clamp(2.0, 16.0) as usize;
    let middle = ((edge - plateau) / 8.0).ceil().clamp(2.0, 12.0) as usize;
    let mut outer = radii(s, edge, last_cell);
    outer.extend(radii(edge, plateau, middle));
    for (k, &r) in outer.iter().enumerate() {
        let inner = outer.get(k + 1).copied().unwrap_or(plateau);
        let reach = if k == 0 { r } else { r + seam };
        filled(
            scene,
            annulus(c, reach, inner),
            colour,
            at((r + inner) / 2.0),
        );
    }
    let core = PathData::circle(c.0, c.1, plateau + seam);
    filled(scene, core, colour, alpha::SCRIM);
}

/// What of the dial stays while the pointer moves within a stage, laid out by `l`: a scrim of
/// the surface, held through the names and gone a cell past the farthest thing drawn, so the
/// lines and text over it read over the chart; the ring dotted in the dial's grey with a tick
/// at each sector's edge, the hovered sector's stretch thick in the accent (in the grey if it
/// doesn't apply); the dead zone a fainter dotted circle, and a reticle at the anchor. In stage
/// two the ring dims, and over it go the guide and the arc: a segment for each choice in the
/// grey, ticks between them in the strong grey, a T at each end, the picked one thick in the
/// accent. Over the line art, the names in the dial's grey (faint in stage two, dim where they
/// don't apply), the hovered one a chip, the surface's colour on the accent; stage two's words,
/// the picked one strong; and the readout, `key:` in the grey and the value in the accent. And
/// the regions: each name shown, each word, and the dead centre.
pub fn draw_still(
    st: &DialState,
    spec: &DialSpec,
    theme: &Theme,
    l: &DialLayout,
    scene: &mut Scene,
    m: &mut dyn Measure,
) {
    let c = st.anchor;
    let ch = st.cell.1;
    let (text, bold) = specs(st);
    let open = st.stage != Stage::Sectors;
    let picked = match st.stage {
        Stage::Choice(k) => Some(k),
        _ => None,
    };
    let span = if open { st.span(spec) } else { None };
    scrim(scene, c, st.scrim(l), ch, theme.surface);
    // Stage two's guide, under the rest.
    if let Some((start, end)) = span {
        let guide = PathData::arc(c.0, c.1, st.band(), start, end);
        stroked(scene, guide, round(art::HAIR, theme.dial, alpha::GUIDE));
    }
    // The ring and the dead zone, dotted, and the ring's ticks.
    let ring = if open { alpha::RING_DIM } else { alpha::RING };
    for (r, a) in [(st.radius(), ring), (st.dead(), alpha::DEAD)] {
        stroked(scene, PathData::circle(c.0, c.1, r), dotted(theme.dial, a));
    }
    let mut ticks = PathData::default();
    for k in 0..8 {
        tick(
            &mut ticks,
            c,
            (k as f32 - 0.5) * 45.0,
            st.radius(),
            art::TICK,
        );
    }
    let a = if open { alpha::RING_DIM } else { alpha::TICK };
    stroked(scene, ticks, round(art::HAIR, theme.dial, a));
    // The hovered stretch, tick to tick.
    if let Some(i) = st.hovered {
        let mid = f32::from(i) * 45.0;
        let (colour, a) = if spec.applies(i) {
            (theme.dial_accent, 1.0)
        } else {
            (theme.dial, alpha::RING)
        };
        let stretch = PathData::arc(c.0, c.1, st.radius(), mid - 22.5, mid + 22.5);
        stroked(scene, stretch, butt(art::THICK, colour, a));
    }
    // The arc: a segment for each choice, ticks between, a T at each end, the picked one thick.
    if let Some((start, end)) = span {
        let r = st.arc_radius();
        let n = ((end - start) / SEGMENT).round() as usize;
        for k in 0..n {
            let from = start + k as f32 * SEGMENT;
            let segment = PathData::arc(c.0, c.1, r, from, from + SEGMENT);
            let stroke = if picked == Some(k as u8) {
                butt(art::THICK, theme.dial_accent, 1.0)
            } else {
                butt(art::HAIR * 1.5, theme.dial, 1.0)
            };
            stroked(scene, segment, stroke);
        }
        let mut ticks = PathData::default();
        for k in 1..n {
            tick(&mut ticks, c, start + k as f32 * SEGMENT, r, art::TICK);
        }
        tick(&mut ticks, c, start, r, art::CAP);
        tick(&mut ticks, c, end, r, art::CAP);
        stroked(scene, ticks, round(art::HAIR, theme.strong, alpha::TICK));
    }
    // The reticle.
    let reticle = PathData::circle(c.0, c.1, art::RETICLE);
    stroked(
        scene,
        reticle,
        round(art::HAIR, theme.strong, alpha::NEEDLE),
    );
    filled(
        scene,
        PathData::circle(c.0, c.1, art::HAIR),
        theme.strong,
        1.0,
    );
    // The names, then the chip.
    let chip = st.hovered.map(usize::from).filter(|_| l.chip.is_some());
    for (i, b) in l.names.iter().enumerate() {
        if !l.shown[i] || chip == Some(i) {
            continue;
        }
        let mut a = if open { alpha::FAINT } else { 1.0 };
        if !spec.sectors[i].enabled {
            a *= alpha::DIM;
        }
        let at = (b.x + b.w / 2.0, b.y + (b.h + m.cap(text)) / 2.0);
        let name = spec.sectors[i].name;
        scene.push(text_item(name, at, text, Anchor::Middle, (theme.dial, a)));
    }
    if let (Some(i), Some(b)) = (chip, l.chip) {
        let corner = ch / 4.0;
        filled(
            scene,
            PathData::rounded(b, [corner; 4]),
            theme.dial_accent,
            1.0,
        );
        let at = (b.x + b.w / 2.0, b.y + (b.h + m.cap(bold)) / 2.0);
        let name = spec.sectors[i].name;
        scene.push(text_item(
            name,
            at,
            bold,
            Anchor::Middle,
            (theme.surface, 1.0),
        ));
    }
    // Stage two's words.
    if let Some(i) = st.hovered.filter(|_| open) {
        let choices = &spec.sectors[usize::from(i)].stage2;
        for (k, b) in &l.labels {
            let Some(word) = choices.get(usize::from(*k)) else {
                continue;
            };
            let (look, ink) = if picked == Some(*k) {
                (bold, (theme.strong, 1.0))
            } else {
                (text, (theme.dial, 1.0))
            };
            let at = (b.x + b.w / 2.0, b.y + m.ascent(bold));
            scene.push(text_item(word, at, look, Anchor::Middle, ink));
        }
    }
    // The readout.
    if let Some(b) = l.readout {
        let mut x = b.x;
        let y = b.y + (b.h + m.cap(text)) / 2.0;
        for (s, piece) in readout_pieces(&st.readout(spec)) {
            let ink = match piece {
                Piece::Key => (theme.dial, 1.0),
                Piece::Value => (theme.dial_accent, 1.0),
                Piece::Dim => (theme.dial, alpha::FAINT),
            };
            let w = m.width(&s, text);
            if !s.trim().is_empty() {
                scene.push(text_item(&s, (x, y), text, Anchor::Start, ink));
            }
            x += w;
        }
    }
    // What each place is.
    for (i, b) in l.names.iter().enumerate().filter(|(i, _)| l.shown[*i]) {
        scene.region(*b, Hit::DialSector(i as u8));
    }
    if open {
        for (k, b) in &l.labels {
            scene.region(*b, Hit::DialStage2(*k));
        }
    }
    let half = st.dead() / std::f32::consts::SQRT_2;
    let centre = Rect::new(c.0 - half, c.1 - half, 2.0 * half, 2.0 * half);
    scene.region(centre, Hit::DialCentre);
}

/// What of the dial follows the pointer: the crosshair through it, faint in the accent, across
/// the dial's bounds; the needle from the reticle's edge, dotted until stage two picks and then
/// solid in the strong grey with a tick where it crosses the guide; and its dot, the accent's.
/// All of it within [`DialState::live_rects`].
pub fn draw_live(st: &DialState, theme: &Theme, l: &DialLayout, scene: &mut Scene) {
    let b = st.bounds(l);
    let (c, p) = (st.anchor, st.pointer);
    let mut cross = PathData::polyline(&[(b.x, p.1), (b.right(), p.1)]);
    cross
        .verbs
        .extend(PathData::polyline(&[(p.0, b.y), (p.0, b.bottom())]).verbs);
    stroked(
        scene,
        cross,
        Stroke::hairline(theme.dial_accent, alpha::CROSS),
    );
    let len = dist(c, p);
    let (from, dot) = (art::RETICLE, art::DOT);
    if len > from + dot {
        let u = ((p.0 - c.0) / len, (p.1 - c.1) / len);
        let a = (c.0 + u.0 * from, c.1 + u.1 * from);
        let e = (p.0 - u.0 * dot, p.1 - u.1 * dot);
        let mut needle = PathData::polyline(&[a, e]);
        let solid = matches!(st.stage, Stage::Choice(_));
        if solid && st.band() < len - dot {
            let n = (-u.1 * art::TICK * 0.75, u.0 * art::TICK * 0.75);
            let g = (c.0 + u.0 * st.band(), c.1 + u.1 * st.band());
            needle.verbs.push(Verb::Move(g.0 - n.0, g.1 - n.1));
            needle.verbs.push(Verb::Line(g.0 + n.0, g.1 + n.1));
        }
        let stroke = if solid {
            round(art::HAIR, theme.strong, alpha::NEEDLE)
        } else {
            dotted(theme.dial, alpha::NEEDLE)
        };
        stroked(scene, needle, stroke);
    }
    filled(
        scene,
        PathData::circle(p.0, p.1, dot),
        theme.dial_accent,
        1.0,
    );
}
