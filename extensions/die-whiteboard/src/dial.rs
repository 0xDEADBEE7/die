//! The tool dial: the eight tools around the pointer, one picked by direction, and for a tool
//! that draws with ink a second stage beyond it, the ink by angle and the width by distance.
//! Space held, or the right button held, opens it where the pointer is; moving past the dead
//! zone hovers the sector in that direction; letting go picks it, or in the dead zone picks
//! nothing. Eight sectors of 45°, north first then clockwise: pen, rect, ellipse, arrow, line,
//! label, eraser, select (the highlighter stays on `h`: it's a pen mode).
//!
//! **Stage one** is a ring of [`RING`] cell heights with the names outside it, on whole cells.
//! **Stage two** opens when the pointer goes [`OPENS`] past the ring with an inky tool hovered
//! (not the eraser or select): an arc [`ARC`] beyond the ring, six segments in ink order
//! clockwise, [`SEGMENT`]° each, placed so the ink armed now is centred on the tool's
//! direction, so pushing straight out keeps it. The pointer's angle picks the ink (clamped to
//! the arc's ends past them) and, for a tool with a width (not text), its distance picks the
//! width, thin from the stage's start, then medium, then thick, in bands [`BAND`] deep. Going
//! back inside the ring is stage one again. Letting go in stage two arms the tool and sets the
//! ink and width; letting go in stage one never changes them.
//!
//! The geometry is in board units, as the marquee's is, sized from the cell at the view's zoom,
//! so on screen it's always the same. Both renderers lay the names, the readout and stage
//! two's labels out on the area's cells from [`Dial::layout`] and write them from
//! [`Dial::texts`]; the pixel renderer draws the line art under them ([`crate::canvas`]). The
//! surface owns a dial while it's open ([`crate::surface`]); it never touches the model, the
//! file or the undo log until it picks.

use crate::model::{CellSize, INKS, Point, Rect, WIDTH_NAMES, WIDTHS};
use crate::surface::Tool;
use die_core::{Color, Rgb};

/// The sectors, north first then clockwise.
pub const SECTORS: [Tool; 8] = [
    Tool::Pen,
    Tool::Rect,
    Tool::Ellipse,
    Tool::Arrow,
    Tool::Line,
    Tool::Label,
    Tool::Eraser,
    Tool::Select,
];

/// The tool ring's radius, in cell heights.
pub const RING: f32 = 2.5;
/// How far past the ring the pointer goes to open stage two, in cell heights.
pub const OPENS: f32 = 0.5;
/// How far beyond the ring the ink arc is, in cell heights.
pub const ARC: f32 = 5.0;
/// How deep each width band is, in cell heights.
pub const BAND: f32 = 1.5;
/// Each ink's share of the arc, in degrees.
pub const SEGMENT: f32 = 20.0;

/// The line art's sizes, in logical pixels: the same on screen at any zoom.
pub mod art {
    /// A hairline's width.
    pub const HAIR: f32 = 1.0;
    /// The hovered stretch of the ring, and the hovered ink, drawn thick.
    pub const THICK: f32 = 3.0;
    /// How far a tick reaches either side of its line.
    pub const TICK: f32 = 4.0;
    /// How far a T cap reaches either side of the ink arc.
    pub const CAP: f32 = 6.0;
    /// The needle's end dot's radius.
    pub const DOT: f32 = 3.0;
    /// The reticle's radius, at the centre.
    pub const RETICLE: f32 = 4.0;
    /// Between a tick's end and a name's cells.
    pub const CLEAR: f32 = 3.0;
}

/// What opened the dial, which is what closes it: the other's key or button does nothing to it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Opener {
    /// Space, held.
    Space,
    /// The right button, held.
    RightButton,
}

/// Which stage is open.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stage {
    /// The tools, by direction.
    Tools,
    /// The ink arc of the hovered tool: the ink under the pointer's angle, `1`..=6, and the
    /// width its distance picks, an index into [`WIDTHS`], or None for a tool without one.
    Ink {
        /// The ink.
        ink: u8,
        /// The width.
        width: Option<u8>,
    },
}

/// What was armed when the dial opened: the readout says what letting go changes from it, and
/// the ink arc is placed by its ink.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Armed {
    /// The tool.
    pub tool: Tool,
    /// The ink, `1`..=6.
    pub ink: u8,
    /// The width, an index into [`WIDTHS`].
    pub width: u8,
}

impl Default for Armed {
    /// The pen, ink 1, medium: a new board's.
    fn default() -> Armed {
        Armed {
            tool: Tool::Pen,
            ink: 1,
            width: 1,
        }
    }
}

/// The dial's colours, from the theme: the `whiteboard-dial` face's for names, the readout's
/// keys and the scaffolding, `whiteboard-dial-accent`'s for the chip, the hovered stretch and
/// the needle's dot, and the `default` face's background for the scrim and the chip's text.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DialLook {
    /// The dial's grey.
    pub face: Rgb,
    /// The accent.
    pub accent: Rgb,
    /// The theme's background.
    pub bg: Rgb,
}

impl Default for DialLook {
    /// Core's fallback theme's: its `shadow` grey, its ANSI red, its background.
    fn default() -> DialLook {
        DialLook {
            face: Rgb(0x70, 0x70, 0x70),
            accent: Rgb(0xcd, 0x00, 0x00),
            bg: Rgb(0x10, 0x10, 0x10),
        }
    }
}

/// Whether a tool draws with ink, so it has stage two: not the eraser or select.
pub fn inky(tool: Tool) -> bool {
    !matches!(tool, Tool::Eraser | Tool::Select)
}

/// Whether a tool has a width to choose: an inky one but text.
pub fn has_width(tool: Tool) -> bool {
    inky(tool) && tool != Tool::Label
}

/// The unit vector `deg` degrees clockwise from north (up: negative y).
pub fn toward(deg: f32) -> Point {
    let a = deg.to_radians();
    Point::new(a.sin(), -a.cos())
}

/// The angle of `d` from north, clockwise, in degrees, in [0, 360).
pub fn angle_of(d: Point) -> f32 {
    d.x.atan2(-d.y).to_degrees().rem_euclid(360.0)
}

/// `deg` brought into (-180, 180].
fn wrap(deg: f32) -> f32 {
    let d = deg.rem_euclid(360.0);
    if d > 180.0 { d - 360.0 } else { d }
}

/// A dial while it's open.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Dial {
    /// Its centre, in board units.
    pub centre: Point,
    /// The tool ring's radius, in board units: [`RING`] cell heights on screen.
    pub radius: f32,
    /// The dead zone's radius, in board units: a cell's larger side on screen.
    pub dead: f32,
    /// A screen cell in board units at the view's zoom: what the names are laid out in.
    pub cell: CellSize,
    /// A logical pixel in board units at the view's zoom: what the line art is sized in.
    pub px: f32,
    /// The sector under the pointer, an index into [`SECTORS`], or None in the dead zone. In
    /// stage two, the tool whose ink arc it is.
    pub hovered: Option<usize>,
    /// Where the pointer is: the needle's end.
    pub pointer: Point,
    /// The stage open.
    pub stage: Stage,
    /// What was armed when it opened.
    pub armed: Armed,
    /// What opened it.
    pub by: Opener,
}

/// A name, a label or a readout's place on the area's cells: its row, its first column and
/// how many cells it takes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Span {
    /// The row.
    pub row: i64,
    /// The first column.
    pub col: i64,
    /// How many cells.
    pub len: i64,
}

impl Span {
    fn meets(&self, o: &Span) -> bool {
        self.row == o.row && self.col < o.col + o.len && o.col < self.col + self.len
    }
}

/// Where the dial's cells go on the area, for a centre where it is in the area's grid.
#[derive(Clone, PartialEq, Debug)]
pub struct Layout {
    /// Each sector's name with a cell either side, where a chip's ends go: the name is from the
    /// second cell.
    pub names: [Span; 8],
    /// Which names show: all in stage one; in stage two the chip, and the others its arcs don't
    /// cross and its labels and swatches leave clear.
    pub shown: [bool; 8],
    /// The readout, centred under everything else.
    pub readout: Option<Span>,
    /// Stage two's width labels, `2` `4` `8`, at the counter-clockwise end of their guide
    /// arcs: the band edge's radius, and the label's cell, where one fits.
    pub guides: Vec<(f32, Span, &'static str)>,
    /// Stage two's swatches for the cell renderer, one an ink, with a cell either side for the
    /// hovered one's marks.
    pub swatches: Vec<Span>,
    /// The farthest anything is drawn from the centre, cells included, in board units.
    pub reach: f32,
}

/// One run of the dial's text on the area's cells.
#[derive(Clone, PartialEq, Debug)]
pub struct Text {
    /// The row.
    pub row: i64,
    /// The first column.
    pub col: i64,
    /// What it says.
    pub text: String,
    /// Its colour.
    pub fg: Color,
    /// Its background.
    pub bg: Color,
    /// Dimmed.
    pub faint: bool,
}

/// What letting go would do, as the reel's readout says it: `key: value rest`.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Readout {
    /// The tool, with its colon if there's a value: in the dial's grey.
    pub key: String,
    /// The value: in the accent.
    pub value: String,
    /// The rest: in the default foreground.
    pub rest: String,
    /// What's armed now, with nothing hovered: all of it in the dial's grey, dimmed.
    pub dim: bool,
}

impl Readout {
    /// It as one line.
    pub fn line(&self) -> String {
        let mut s = self.key.clone();
        if !self.value.is_empty() {
            s.push(' ');
            s.push_str(&self.value);
        }
        s.push_str(&self.rest);
        s
    }
}

impl Dial {
    /// A dial about `centre` with nothing hovered yet, sized from a cell of `cell` board units
    /// at `zoom`, `armed` as the board is now.
    pub fn open(centre: Point, by: Opener, cell: CellSize, zoom: f32, armed: Armed) -> Dial {
        let zoom = if zoom > 0.0 { zoom } else { 1.0 };
        let cell = CellSize {
            w: cell.w / zoom,
            h: cell.h / zoom,
        };
        Dial {
            centre,
            radius: RING * cell.h,
            dead: cell.w.max(cell.h),
            cell,
            px: 1.0 / zoom,
            hovered: None,
            pointer: centre,
            stage: Stage::Tools,
            armed,
            by,
        }
    }

    /// The unit vector `turn` eighths of a turn clockwise from north (up: negative y): sector
    /// `i`'s middle at `i`, the spoke before it at `i - 0.5`.
    pub fn direction(turn: f32) -> Point {
        toward(turn * 45.0)
    }

    /// The sector under `p`, or None in the dead zone.
    pub fn sector_at(&self, p: Point) -> Option<usize> {
        if p.dist(self.centre) < self.dead {
            return None;
        }
        let turn = angle_of(p - self.centre) / 45.0;
        Some((turn + 0.5).floor() as usize % 8)
    }

    /// The radius past which stage two opens.
    pub fn opens(&self) -> f32 {
        self.radius + OPENS * self.cell.h
    }

    /// The ink arc's radius.
    pub fn arc_radius(&self) -> f32 {
        self.radius + ARC * self.cell.h
    }

    /// The width bands' inner edges: thin from the first, medium from the second, thick from
    /// the third.
    pub fn bands(&self) -> [f32; 3] {
        let (o, b) = (self.opens(), BAND * self.cell.h);
        [o, o + b, o + 2.0 * b]
    }

    /// The pointer is at `p`: the sector in its direction is hovered, or stage two's ink and
    /// width under it. Whether what the dial says changed (the pointer moves the needle
    /// whatever this says).
    pub fn hover(&mut self, p: Point) -> bool {
        let was = (self.hovered, self.stage);
        self.pointer = p;
        let r = p.dist(self.centre);
        let held = matches!(self.stage, Stage::Ink { .. }) && r >= self.radius;
        if !held {
            self.hovered = self.sector_at(p);
            self.stage = Stage::Tools;
        }
        if let Some(tool) = self.tool()
            && inky(tool)
            && (held || r >= self.opens())
        {
            self.stage = Stage::Ink {
                ink: self.ink_at(p),
                width: has_width(tool).then(|| self.width_at(r)),
            };
        }
        was != (self.hovered, self.stage)
    }

    /// The tool hovered, if one is.
    pub fn tool(&self) -> Option<Tool> {
        self.hovered.map(|i| SECTORS[i])
    }

    /// The ink arc's ends, in degrees clockwise from north, the first counter-clockwise: the
    /// armed ink's segment centred on the hovered tool's direction. None with nothing hovered.
    pub fn span(&self) -> Option<(f32, f32)> {
        let dir = self.hovered? as f32 * 45.0;
        let start = dir - (f32::from(self.armed.ink.clamp(1, INKS)) - 0.5) * SEGMENT;
        Some((start, start + f32::from(INKS) * SEGMENT))
    }

    /// The ink whose segment is in `p`'s direction; past the arc, the nearer end's.
    pub fn ink_at(&self, p: Point) -> u8 {
        let Some((start, end)) = self.span() else {
            return self.armed.ink;
        };
        let half = (end - start) / 2.0;
        let rel = wrap(angle_of(p - self.centre) - (start + half)) + half;
        let k = (rel / SEGMENT).floor().clamp(0.0, f32::from(INKS) - 1.0);
        k as u8 + 1
    }

    /// The width a distance `r` from the centre picks.
    pub fn width_at(&self, r: f32) -> u8 {
        let k = ((r - self.opens()) / (BAND * self.cell.h)).floor();
        k.clamp(0.0, (WIDTHS.len() - 1) as f32) as u8
    }

    /// What letting go would do, or with nothing hovered, what's armed, dimmed.
    pub fn readout(&self) -> Readout {
        let width = |w: u8| WIDTH_NAMES[usize::from(w).min(WIDTHS.len() - 1)];
        let say = |key: &str, value: String, rest: String, dim: bool| Readout {
            key: if value.is_empty() {
                key.to_string()
            } else {
                format!("{key}:")
            },
            value,
            rest,
            dim,
        };
        if let Stage::Ink { ink, width: w } = self.stage {
            let tool = self.tool().unwrap_or(self.armed.tool);
            let rest = w.map_or(String::new(), |w| format!(" · {}", width(w)));
            return say(tool.name(), format!("ink {ink}"), rest, false);
        }
        let (tool, dim) = match self.tool() {
            Some(t) => (t, false),
            None => (self.armed.tool, true),
        };
        let a = self.armed;
        match tool {
            Tool::Pen => say(
                "pen",
                format!("ink {}", a.ink),
                format!(" · {}", width(a.width)),
                dim,
            ),
            Tool::Highlighter => say("highlighter", format!("ink {}", a.ink), String::new(), dim),
            Tool::Eraser => {
                // Letting go toggles it, as `e` does; armed, it's on.
                let on = if dim { true } else { a.tool != Tool::Eraser };
                say(
                    "eraser",
                    (if on { "on" } else { "off" }).into(),
                    "".into(),
                    dim,
                )
            }
            Tool::Select => say("select", String::new(), String::new(), dim),
            t => say(t.name(), "once".into(), String::new(), dim),
        }
    }

    /// The cell holding `p`, on a grid whose cell (0, 0) has its top left at `origin`.
    fn cell_of(&self, origin: Point, p: Point) -> (i64, i64) {
        let col = ((p.x - origin.x) / self.cell.w).floor() as i64;
        let row = ((p.y - origin.y) / self.cell.h).floor() as i64;
        (row, col)
    }

    /// A span's cells, in board units.
    pub fn rect_of(&self, origin: Point, s: &Span) -> Rect {
        let min = Point::new(
            origin.x + s.col as f32 * self.cell.w,
            origin.y + s.row as f32 * self.cell.h,
        );
        let max = Point::new(min.x + s.len as f32 * self.cell.w, min.y + self.cell.h);
        Rect { min, max }
    }

    /// How far a rectangle's nearest point is from the centre.
    fn near(&self, r: &Rect) -> f32 {
        let c = self.centre;
        let dx = (r.min.x - c.x).max(0.0).max(c.x - r.max.x);
        let dy = (r.min.y - c.y).max(0.0).max(c.y - r.max.y);
        dx.hypot(dy)
    }

    /// How far a rectangle's farthest corner is from the centre.
    fn far(&self, r: &Rect) -> f32 {
        let c = self.centre;
        let dx = (r.min.x - c.x).abs().max((r.max.x - c.x).abs());
        let dy = (r.min.y - c.y).abs().max((r.max.y - c.y).abs());
        dx.hypot(dy)
    }

    /// Where the names, the readout and stage two's labels go, on a grid of the dial's cells
    /// whose cell (0, 0) has its top left at `origin` (the area's, in board units), in an area
    /// of `area` columns and rows.
    ///
    /// Each name keeps a cell either side for a chip's ends and hangs off the ring by the end
    /// that faces the centre: north and south centred on the centre's column, east and west on
    /// its row, the diagonals on the row about 45° out. It moves out a cell at a time until no
    /// cell of it is within a tick and a little of the ring, and none meets a name placed
    /// before it. So on whole cells, wherever in a cell the centre falls, none overlaps the
    /// ring, its ticks or another name. The readout goes two rows under everything, or two
    /// over it when the area ends first, and inside the area's sides.
    pub fn layout(&self, origin: Point, area: (usize, usize)) -> Layout {
        let (cr, cc) = self.cell_of(origin, self.centre);
        let clear = self.radius + (art::TICK + art::CLEAR) * self.px;
        let mut names = [Span {
            row: 0,
            col: 0,
            len: 0,
        }; 8];
        for (i, tool) in SECTORS.iter().enumerate() {
            let len = tool.name().chars().count() as i64 + 2;
            let diagonal_row = {
                let at = self.centre + Dial::direction(i as f32) * (clear + self.cell.h * 0.5);
                self.cell_of(origin, at).0
            };
            let place = |k: i64| -> Span {
                let (row, col) = match i {
                    0 => (cr - k, cc - len / 2),
                    4 => (cr + k, cc - len / 2),
                    2 => (cr, cc + k),
                    6 => (cr, cc - k - len + 1),
                    1 | 3 => (diagonal_row, cc + k),
                    _ => (diagonal_row, cc - k - len + 1),
                };
                Span { row, col, len }
            };
            let mut k = 1;
            let span = loop {
                let s = place(k);
                let clear_of_ring = self.near(&self.rect_of(origin, &s)) >= clear;
                let clear_of_names = names[..i].iter().all(|o| !o.meets(&s));
                if (clear_of_ring && clear_of_names) || k > 64 {
                    break s;
                }
                k += 1;
            };
            names[i] = span;
        }
        let mut shown = [true; 8];
        let mut guides = vec![];
        let mut swatches = vec![];
        let mut lowest = names.iter().map(|s| s.row).max().unwrap_or(cr);
        let mut highest = names.iter().map(|s| s.row).min().unwrap_or(cr);
        let mut reach = names
            .iter()
            .map(|s| self.far(&self.rect_of(origin, s)))
            .fold(self.radius, f32::max);
        if let (Stage::Ink { .. }, Some((start, end)), Some(tool)) =
            (self.stage, self.span(), self.tool())
        {
            let arc = self.arc_radius() + art::CAP * self.px;
            reach = reach.max(arc);
            // The arc's lowest and highest points, for the readout under or over it.
            let mut deg = start;
            while deg <= end + 0.01 {
                let row = self.cell_of(origin, self.centre + toward(deg) * arc).0;
                (lowest, highest) = (lowest.max(row), highest.min(row));
                deg += 5.0;
            }
            let chip = names[self.hovered.unwrap_or(0)];
            let mut taken = vec![chip];
            if has_width(tool) {
                for (r, label) in self.bands().into_iter().zip(["2", "4", "8"]) {
                    // A cell and a half past the guide's end, along it: the start's end, else
                    // the other.
                    let step = (1.5 * self.cell.w / r).to_degrees();
                    let fits = [start - step, end + step].into_iter().find_map(|deg| {
                        let (row, col) = self.cell_of(origin, self.centre + toward(deg) * r);
                        let s = Span { row, col, len: 1 };
                        taken.iter().all(|t| !t.meets(&s)).then_some(s)
                    });
                    if let Some(s) = fits {
                        taken.push(s);
                        (lowest, highest) = (lowest.max(s.row), highest.min(s.row));
                        guides.push((r, s, label));
                    }
                }
            }
            for k in 0..INKS {
                let deg = start + (f32::from(k) + 0.5) * SEGMENT;
                let at = self.centre + toward(deg) * self.arc_radius();
                let (row, col) = self.cell_of(origin, at);
                let s = Span {
                    row,
                    col: col - 1,
                    len: 4,
                };
                (lowest, highest) = (lowest.max(row), highest.min(row));
                reach = reach.max(self.far(&self.rect_of(origin, &s)));
                swatches.push(s);
            }
            // A name stage two's arcs cross, or its labels or swatches sit on, gives way.
            let mut radii = vec![self.arc_radius()];
            if has_width(tool) {
                radii.extend(self.bands());
            }
            for (i, name) in names.iter().enumerate() {
                let r = self.rect_of(origin, name).inflate(2.0 * self.px);
                let crossed = radii.iter().any(|radius| {
                    let steps = ((end - start) / 2.0).ceil() as usize;
                    (0..=steps).any(|k| {
                        let deg = start + (end - start) * k as f32 / steps as f32;
                        r.contains(self.centre + toward(deg) * *radius)
                    })
                });
                // A label beside a name would read as part of it: within three blank cells.
                let beside = |s: &Span| Span {
                    col: s.col - 3,
                    len: s.len + 6,
                    ..*s
                };
                let crowded = crossed
                    || guides.iter().any(|(_, g, _)| beside(g).meets(name))
                    || swatches.iter().any(|w| w.meets(name));
                shown[i] = Some(i) == self.hovered || !crowded;
            }
        }
        let line = self.readout().line();
        let len = line.chars().count() as i64;
        let (cols, rows) = (area.0 as i64, area.1 as i64);
        let row = if lowest + 2 < rows || highest - 2 < 0 {
            lowest + 2
        } else {
            highest - 2
        };
        let readout = (len > 0).then(|| Span {
            row,
            col: (cc - len / 2).min(cols - len).max(0),
            len,
        });
        if let Some(s) = &readout {
            reach = reach.max(self.far(&self.rect_of(origin, s)));
        }
        Layout {
            names,
            shown,
            readout,
            guides,
            swatches,
            reach,
        }
    }

    /// The scrim's radius: a cell past the farthest thing drawn.
    pub fn scrim(&self, layout: &Layout) -> f32 {
        layout.reach + self.cell.h
    }

    /// The scrim's square: what [`crate::canvas::Target::dial_base`] draws in.
    pub fn base_bounds(&self, layout: &Layout) -> Rect {
        Rect::from_points(self.centre, self.centre).inflate(self.scrim(layout))
    }

    /// What the pixel renderer draws and presents: the scrim's square, and the pointer's dot
    /// wherever it is, with room for the preview's widest.
    pub fn bounds(&self, layout: &Layout) -> Rect {
        let dot = (art::DOT + 2.0) * self.px + WIDTHS[WIDTHS.len() - 1];
        let pointer = Rect::from_points(self.pointer, self.pointer).inflate(dot);
        self.base_bounds(layout).union(&pointer)
    }

    /// Where [`crate::canvas::Target::dial_live`] draws: the crosshair's two strips across the
    /// bounds, and the needle's box from the centre to the pointer's dot.
    pub fn live_rects(&self, layout: &Layout) -> [Rect; 3] {
        let b = self.bounds(layout);
        let (p, edge) = (self.pointer, art::HAIR * self.px);
        let dot = (art::DOT + 2.0) * self.px + WIDTHS[WIDTHS.len() - 1];
        [
            Rect::from_points(
                Point::new(b.min.x, p.y - edge),
                Point::new(b.max.x, p.y + edge),
            ),
            Rect::from_points(
                Point::new(p.x - edge, b.min.y),
                Point::new(p.x + edge, b.max.y),
            ),
            Rect::from_points(self.centre, p).inflate(dot.max(art::TICK * self.px)),
        ]
    }

    /// The dial with what only moves the needle left out: what the pixel renderer keeps a
    /// drawing of while the pointer moves.
    pub fn still(&self) -> Dial {
        let stage = match self.stage {
            Stage::Ink { ink, .. } => Stage::Ink { ink, width: None },
            s => s,
        };
        Dial {
            pointer: self.centre,
            stage,
            ..*self
        }
    }

    /// The hovered name's chip, if it has one, as a span of cells.
    pub fn chip(&self, layout: &Layout) -> Option<Span> {
        self.hovered.map(|i| layout.names[i])
    }

    /// The dial's text, on the area's cells: the names in the dial's grey (dimmed in stage
    /// two), the hovered one a chip, the accent behind the theme's background; stage two's
    /// width labels dimmed; and the readout. `cells` adds what only the cell renderer draws: a
    /// `+` at the centre and stage two's swatches, `██` in each ink, the hovered one between
    /// marks.
    pub fn texts(
        &self,
        layout: &Layout,
        look: &DialLook,
        inks: &[Color; 6],
        cells: Option<(i64, i64)>,
    ) -> Vec<Text> {
        let face = Color::Rgb(look.face);
        let two = matches!(self.stage, Stage::Ink { .. });
        let mut out = vec![];
        let text = |row, col, s: &str, fg, bg, faint| Text {
            row,
            col,
            text: s.to_string(),
            fg,
            bg,
            faint,
        };
        for (i, tool) in SECTORS.iter().enumerate() {
            let s = layout.names[i];
            if !layout.shown[i] {
                continue;
            }
            if self.hovered == Some(i) {
                let padded = format!(" {} ", tool.name());
                let (fg, bg) = (Color::Rgb(look.bg), Color::Rgb(look.accent));
                out.push(text(s.row, s.col, &padded, fg, bg, false));
            } else {
                let name = tool.name();
                out.push(text(s.row, s.col + 1, name, face, Color::Default, two));
            }
        }
        // The width labels name the pixel renderer's guides; the cell renderer has none.
        for (_, s, label) in layout.guides.iter().filter(|_| cells.is_none()) {
            out.push(text(s.row, s.col, label, face, Color::Default, true));
        }
        if let Some((row, col)) = cells {
            out.push(text(row, col, "+", face, Color::Default, false));
            if let Stage::Ink { ink, .. } = self.stage {
                for (k, s) in layout.swatches.iter().enumerate() {
                    let fg = inks[k];
                    out.push(text(s.row, s.col + 1, "██", fg, Color::Default, false));
                    if usize::from(ink) == k + 1 {
                        let accent = Color::Rgb(look.accent);
                        out.push(text(s.row, s.col, "[", accent, Color::Default, false));
                        out.push(text(s.row, s.col + 3, "]", accent, Color::Default, false));
                    }
                }
            }
        }
        if let Some(s) = layout.readout {
            let r = self.readout();
            let mut col = s.col;
            let mut put = |t: &str, fg: Color| {
                if !t.is_empty() {
                    out.push(text(s.row, col, t, fg, Color::Default, r.dim));
                    col += t.chars().count() as i64;
                }
            };
            if r.dim {
                put(&r.line(), face);
            } else {
                put(&r.key, face);
                if !r.value.is_empty() {
                    put(" ", face);
                    put(&r.value, Color::Rgb(look.accent));
                }
                put(&r.rest, Color::Default);
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dial() -> Dial {
        Dial::open(
            Point::new(100.0, 100.0),
            Opener::Space,
            CellSize { w: 5.0, h: 10.0 },
            1.0,
            Armed::default(),
        )
    }

    #[test]
    fn the_sectors_go_clockwise_from_north_and_the_dead_zone_is_a_cells_larger_side() {
        let d = dial();
        assert_eq!((d.radius, d.dead), (25.0, 10.0));
        let at = |dx: f32, dy: f32| d.sector_at(Point::new(100.0 + dx, 100.0 + dy));
        assert_eq!(at(0.0, -20.0), Some(0), "north: the pen");
        assert_eq!(at(20.0, -20.0), Some(1), "north-east: rect");
        assert_eq!(at(20.0, 0.0), Some(2), "east: ellipse");
        assert_eq!(at(20.0, 20.0), Some(3));
        assert_eq!(at(0.0, 20.0), Some(4), "south: line");
        assert_eq!(at(-20.0, 20.0), Some(5));
        assert_eq!(at(-20.0, 0.0), Some(6), "west: the eraser");
        assert_eq!(at(-20.0, -20.0), Some(7), "north-west: select");
        assert_eq!(
            at(-3.0, -20.0),
            Some(0),
            "a little off north is still north"
        );
        assert_eq!(at(0.0, -9.0), None, "the dead zone");
        assert_eq!(at(0.0, 0.0), None);
        assert_eq!(SECTORS[6], Tool::Eraser);
        let mut d = d;
        assert!(d.hover(Point::new(120.0, 100.0)));
        assert!(!d.hover(Point::new(122.0, 101.0)), "the same sector");
        assert_eq!(d.pointer, Point::new(122.0, 101.0), "the needle follows");
        assert_eq!(d.tool(), Some(Tool::Ellipse));
        assert!(d.hover(Point::new(100.0, 100.0)));
        assert_eq!(d.tool(), None);
    }

    #[test]
    fn half_a_cell_past_the_ring_an_inky_tool_opens_its_ink_arc_and_inside_the_ring_closes_it() {
        let mut d = dial();
        // The ring at 25, stage two from 30, bands of 15: thin to 45, medium to 60, thick on.
        assert_eq!((d.opens(), d.bands()), (30.0, [30.0, 45.0, 60.0]));
        assert_eq!(d.arc_radius(), 75.0);
        d.hover(Point::new(100.0, 72.0));
        assert_eq!((d.tool(), d.stage), (Some(Tool::Pen), Stage::Tools));
        assert!(d.hover(Point::new(100.0, 69.0)), "past 30: stage two");
        assert_eq!(
            d.stage,
            Stage::Ink {
                ink: 1,
                width: Some(0)
            },
            "straight out keeps ink 1; thin"
        );
        d.hover(Point::new(100.0, 50.0));
        assert_eq!(
            d.stage,
            Stage::Ink {
                ink: 1,
                width: Some(1)
            }
        );
        d.hover(Point::new(100.0, 10.0));
        assert_eq!(
            d.stage,
            Stage::Ink {
                ink: 1,
                width: Some(2)
            },
            "and thick on past the arc"
        );
        // The arc runs clockwise from ink 1 centred north: 20° a segment, so ink 3 is at 40°.
        assert_eq!(d.span(), Some((-10.0, 110.0)));
        let p = d.centre + toward(45.0) * 50.0;
        d.hover(p);
        assert_eq!(
            d.tool(),
            Some(Tool::Pen),
            "held: the angle picks the ink now"
        );
        assert_eq!(
            d.stage,
            Stage::Ink {
                ink: 3,
                width: Some(1)
            }
        );
        d.hover(d.centre + toward(200.0) * 50.0);
        assert_eq!(
            d.stage,
            Stage::Ink {
                ink: 6,
                width: Some(1)
            },
            "past the clockwise end: the last"
        );
        d.hover(d.centre + toward(-60.0) * 50.0);
        assert_eq!(
            d.stage,
            Stage::Ink {
                ink: 1,
                width: Some(1)
            },
            "past the other: the first"
        );
        // Between the ring and stage two's start it stays open; inside the ring it closes.
        d.hover(d.centre + toward(10.0) * 27.0);
        assert!(matches!(d.stage, Stage::Ink { .. }));
        d.hover(d.centre + toward(90.0) * 20.0);
        assert_eq!((d.tool(), d.stage), (Some(Tool::Ellipse), Stage::Tools));
        // Text has no width; the eraser and select no second stage.
        d.hover(d.centre + toward(225.0) * 40.0);
        assert_eq!(
            (d.tool(), d.stage),
            (
                Some(Tool::Label),
                Stage::Ink {
                    ink: 1,
                    width: None
                }
            )
        );
        d.hover(d.centre);
        d.hover(d.centre + toward(270.0) * 80.0);
        assert_eq!((d.tool(), d.stage), (Some(Tool::Eraser), Stage::Tools));
        // The arc is placed by the armed ink: with ink 4, its segment is centred on the tool.
        let mut d = dial();
        d.armed.ink = 4;
        d.hover(d.centre + toward(90.0) * 50.0);
        assert_eq!(d.span(), Some((20.0, 140.0)));
        assert_eq!(
            d.stage,
            Stage::Ink {
                ink: 4,
                width: Some(1)
            }
        );
    }

    #[test]
    fn the_readout_says_what_letting_go_does() {
        let mut d = dial();
        assert_eq!(d.readout().line(), "pen: ink 1 · medium");
        assert!(d.readout().dim, "nothing hovered: what's armed, dimmed");
        d.hover(Point::new(120.0, 80.0));
        let r = d.readout();
        assert_eq!(
            (r.key.as_str(), r.value.as_str(), r.rest.as_str(), r.dim),
            ("rect:", "once", "", false)
        );
        d.hover(Point::new(80.0, 100.0));
        assert_eq!(d.readout().line(), "eraser: on");
        d.armed.tool = Tool::Eraser;
        assert_eq!(d.readout().line(), "eraser: off", "it toggles");
        d.hover(Point::new(80.0, 80.0));
        assert_eq!(d.readout().line(), "select");
        d.armed.width = 2;
        d.hover(Point::new(100.0, 80.0));
        assert_eq!(
            d.readout().line(),
            "pen: ink 1 · thick",
            "stage one: as armed"
        );
        d.hover(Point::new(100.0, 50.0));
        assert_eq!(
            d.readout().line(),
            "pen: ink 1 · medium",
            "stage two: as picked"
        );
        // Text's arc from 215°, ink 1 first: ink 5 is 80° on.
        d.hover(d.centre);
        d.hover(d.centre + toward(225.0) * 50.0);
        d.hover(d.centre + toward(305.0) * 50.0);
        assert_eq!(d.readout().line(), "text: ink 5");
    }

    #[test]
    fn the_names_clear_the_ring_its_ticks_and_each_other_wherever_the_centre_falls_in_a_cell() {
        let mut d = dial();
        for fx in [0.0, 0.3, 0.6, 0.99] {
            for fy in [0.0, 0.25, 0.5, 0.75, 0.99] {
                d.centre = Point::new(100.0 + fx * 5.0, 100.0 + fy * 10.0);
                let l = d.layout(Point::new(0.0, 0.0), (100, 30));
                let clear = d.radius + art::TICK + art::CLEAR;
                for (i, s) in l.names.iter().enumerate() {
                    let r = d.rect_of(Point::new(0.0, 0.0), s);
                    assert!(d.near(&r) >= clear, "{i} at ({fx}, {fy}): {s:?}");
                    assert_eq!(s.len as usize, SECTORS[i].name().len() + 2);
                    for o in &l.names[..i] {
                        assert!(!o.meets(s), "{i} meets {o:?} at ({fx}, {fy})");
                    }
                }
                // North and south are centred on the centre's column, east and west on its row.
                let (cr, cc) = (10, 20);
                assert_eq!(l.names[2].row, l.names[6].row);
                assert_eq!(l.names[0].col + l.names[0].len / 2, cc);
                assert!(l.names[0].row < cr - 2 && l.names[4].row > cr + 2);
                let readout = l.readout.unwrap();
                assert!(readout.row > l.names[4].row, "under it all");
            }
        }
        // Zoom keeps it the same on screen: half the board units at 2x.
        let zoomed = Dial::open(
            d.centre,
            Opener::RightButton,
            CellSize { w: 5.0, h: 10.0 },
            2.0,
            Armed::default(),
        );
        assert_eq!((zoomed.radius, zoomed.dead, zoomed.px), (12.5, 5.0, 0.5));
    }
}
