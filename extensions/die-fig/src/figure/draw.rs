//! A tile drawn: where its pieces land in its rectangle ([`layout`]), its base ([`base`]), what
//! the pointer is over ([`hit`]) and what it shows there ([`overlay`]).
//!
//! The look is the design's m4, exactly at its size (197 × 100): the label 12 px muted at 16 px
//! in and 26 px down to its baseline; the value 26 px semibold, 34 px under it, its unit 14 px
//! muted 4 px after it; the delta line 11.5 px, 24 px under the value: an arrow (always, so the
//! change never rests on colour alone) and `4.2 % vs previous 6h`, in the good status's colour
//! when the change is good, the serious one's when it's bad, muted when it's neither or none.
//! The trend at the right, level with the value: a 1.5 px line in the border grey, its newest
//! point a 3 px dot in series slot 1 with a 1.5 px surface ring; no axes. The tile's frame is
//! the dashboard's panel: [`base`] draws inside `rect` and nothing on its edge.
//!
//! It degrades as the tile narrows or shortens: the trend goes first, then the delta (its
//! period, then all of it), then the unit; the value never ellipsizes, shrinking to 18 px
//! before the label goes. A taller tile centres m4's block.
//!
//! A meter: the label at the left, the value's share and `7.2 of 10 GB` at the right on its
//! baseline, the status of the highest threshold passed before them with its icon, and under
//! them a track (the grid colour, 8 px, 4 px radius) filled to the share in slot 1 or that
//! status's colour, a 1 px tick at each threshold. A sparkline: its label at the left, its
//! newest value at the right and the line between, a faint wash (slot 1 at 12 %) under it; in
//! a tile taller than 120 px the label and value go on top and the line fills the rest.
//!
//! The overlay: over the trend, a mini crosshair at the nearest point with its dot and a card
//! of its value and time; elsewhere on the tile, a card with the value in full and its unit,
//! what it aggregates and over which range, what it's compared with (a meter: its max and
//! thresholds). Cards look as a chart's do and go under the tile where the area has room, else
//! over it, else beside the point.

use super::{TileKind, TileModel, short_number, status_name, with_unit};
use crate::algo::lttb;
use crate::plot::errors::readable;
use crate::plot::marks::on_fill;
use crate::scene::{
    Anchor, Cap, FixedMeasure, Item, Join, Measure, Paint, PathData, Rect, Scene, Stroke, TextSpec,
    Verb, Weight,
};
use crate::style::Status;
use crate::theme::{Color, Theme};

/// From the tile's edge to its text, across.
pub const PAD: f32 = 16.0;
/// The label's size.
pub const LABEL: f32 = 12.0;
/// The value's size, and the least it shrinks to.
pub const VALUE: f32 = 26.0;
/// The least the value shrinks to before the label goes.
pub const VALUE_MIN: f32 = 18.0;
/// The unit's size, and its gap after the value.
pub const UNIT: f32 = 14.0;
/// m4 puts the unit 4 px past the value's advance in a browser's font; die-draw's figures sit
/// closer to their advance, so 6 px reads as m4's gap.
const UNIT_GAP: f32 = 6.0;
/// The delta line's size.
pub const DELTA: f32 = 11.5;
/// m4's baselines in a 100 px tile: the label's, the value's and the delta's.
const LABEL_Y: f32 = 26.0;
const VALUE_Y: f32 = 60.0;
const DELTA_Y: f32 = 84.0;
/// From the label's baseline to the value's at 26 px.
pub const LABEL_TO_VALUE: f32 = VALUE_Y - LABEL_Y;
/// The trend's least width, its height, and its gap from the value.
const TREND_W: f32 = 58.0;
const TREND_H: f32 = 22.0;
const TREND_GAP: f32 = 16.0;
/// The trend's line, and its newest dot's radius and ring.
const TREND_LINE: f32 = 1.5;
const DOT: f32 = 3.0;
const RING: f32 = 1.5;
/// The delta's arrow: its width and height, and its gap to the text.
const ARROW_W: f32 = 9.0;
const ARROW_H: f32 = 8.0;
const ARROW_GAP: f32 = 4.5;
/// A meter's track: its height and radius, and its gap under the label's baseline.
pub const TRACK_H: f32 = 8.0;
const TRACK_R: f32 = 4.0;
const TRACK_GAP: f32 = 12.0;
/// A meter's status icon's radius, from it to its word, and from the status to the share.
const ICON_R: f32 = 5.5;
const ICON_GAP: f32 = 5.0;
const STATUS_GAP: f32 = 10.0;
/// How far a threshold's tick reaches past the track's edges.
const TICK_OUT: f32 = 3.0;
/// A sparkline's wash under its line.
pub const WASH: f32 = 0.12;
/// A sparkline taller than this puts its label and value on top.
const BLOCK_H: f32 = 120.0;

/// What the pointer is over on a tile.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TileHover {
    /// The tile, the pointer at a place on it: the card with the value in full.
    Tile {
        /// Where the pointer is, in the area's px.
        at: (f32, f32),
    },
    /// A point of the trend (by its index in [`TileModel::trend`]): the mini crosshair there,
    /// with its card when `card` (a dashboard's linked crosshair draws it without).
    Trend {
        /// The point.
        point: usize,
        /// Whether its card shows.
        card: bool,
    },
}

/// What a piece of a tile's text is, which picks its colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    /// The label: muted.
    Label,
    /// The value: strong, or muted when there's no data.
    Value,
    /// The unit and other quiet text: muted.
    Quiet,
    /// The delta's text, after its arrow: good, bad or muted.
    Delta,
    /// A meter's share: strong.
    Share,
    /// A meter's status word: the text colour.
    Status,
}

/// A piece of a tile's text, placed.
#[derive(Clone, Debug, PartialEq)]
pub struct Piece {
    /// What it is.
    pub role: Role,
    /// What it says, already cut to fit.
    pub text: String,
    /// Its anchor's x.
    pub x: f32,
    /// Its baseline.
    pub y: f32,
    /// How it's set.
    pub spec: TextSpec,
    /// Which end of it is at x.
    pub anchor: Anchor,
}

impl Piece {
    /// Its box, measured.
    pub fn bounds(&self, m: &mut dyn Measure) -> Rect {
        let w = m.width(&self.text, self.spec);
        let left = match self.anchor {
            Anchor::Start => self.x,
            Anchor::Middle => self.x - w / 2.0,
            Anchor::End => self.x - w,
        };
        let a = m.ascent(self.spec);
        Rect::new(left, self.y - a, w, a + m.descent(self.spec))
    }
}

/// Where a tile's pieces land in its rectangle.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct TileLayout {
    /// The tile's rectangle.
    pub rect: Rect,
    /// Its text.
    pub pieces: Vec<Piece>,
    /// The delta's arrow: its direction (1 up, -1 down, 0 a dash for no change) and the left
    /// end of its baseline.
    pub arrow: Option<(i8, (f32, f32))>,
    /// Whether the delta is good (true), bad (false) or neither.
    pub good: Option<bool>,
    /// Where the trend's line is drawn.
    pub trend: Option<Rect>,
    /// A meter's track.
    pub track: Option<Rect>,
    /// A meter's status icon: its status and centre.
    pub icon: Option<(Status, (f32, f32))>,
    /// The value's size, after any shrinking.
    pub value_size: f32,
}

fn label_spec() -> TextSpec {
    TextSpec::new(LABEL)
}

fn value_spec(size: f32) -> TextSpec {
    TextSpec::new(size).weight(Weight::Semibold)
}

fn unit_spec() -> TextSpec {
    TextSpec::new(UNIT)
}

fn delta_spec() -> TextSpec {
    TextSpec::new(DELTA)
}

/// Where `model`'s pieces land in `rect`.
pub fn layout(model: &TileModel, rect: Rect, m: &mut dyn Measure) -> TileLayout {
    match model.kind {
        TileKind::Stat => stat_layout(model, rect, m),
        TileKind::Meter => meter_layout(model, rect, m),
        TileKind::Sparkline => spark_layout(model, rect, m),
    }
}

/// The value's size and whether its unit shows, in `room` across: 26 px with the unit, else
/// without it, else shrunk to fit, never under 18 px.
fn value_row(
    text: &str,
    unit: Option<&str>,
    room: f32,
    m: &mut dyn Measure,
) -> (f32, bool, f32, f32) {
    let vw = m.width(text, value_spec(VALUE));
    let uw = unit
        .filter(|u| !u.is_empty())
        .map_or(0.0, |u| UNIT_GAP + m.width(u, unit_spec()));
    if uw > 0.0 && vw + uw <= room {
        return (VALUE, true, vw, uw);
    }
    if vw <= room {
        return (VALUE, false, vw, 0.0);
    }
    let size = (VALUE * room / vw.max(1.0)).clamp(VALUE_MIN, VALUE).floor();
    (size, false, m.width(text, value_spec(size)), 0.0)
}

fn stat_layout(model: &TileModel, r: Rect, m: &mut dyn Measure) -> TileLayout {
    let inner = (r.w - 2.0 * PAD).max(0.0);
    let x = r.x + PAD;
    let text = model.text();
    let (size, show_unit, vw, uw) = value_row(&text, model.unit.as_deref(), inner, m);
    let row_w = vw + if show_unit { uw } else { 0.0 };
    let trend_w = (inner * 0.35).clamp(TREND_W, 200.0);
    let has_trend = model.trend.len() >= 2 && !model.value.is_nan();
    let trend_fits = has_trend && PAD + row_w + TREND_GAP + trend_w + PAD <= r.w;
    // The delta line: the arrow and its text whole, else the amount alone, else nothing.
    let dspec = delta_spec();
    let lead = ARROW_W + ARROW_GAP;
    let delta: Option<(Option<i8>, String)> = if model.value.is_nan() {
        Some((None, "no data".to_string()))
    } else {
        match (&model.change, model.change_text()) {
            (Some(c), Some((amount, line))) => {
                let dir = c.direction();
                if lead + m.width(&line, dspec) <= inner {
                    Some((Some(dir), line))
                } else if lead + m.width(&amount, dspec) <= inner {
                    Some((Some(dir), amount))
                } else {
                    None
                }
            }
            (Some(c), None) => {
                let note = format!("no data for {}", c.period);
                let note = m.fit(&note, dspec, inner);
                (!note.is_empty()).then_some((None, note))
            }
            (None, _) => None,
        }
    };
    let cap_l = m.cap(label_spec());
    let cap_v = m.cap(value_spec(size));
    // The value's cap top sits as far under the label's baseline as m4's does.
    let label_to_value = (LABEL_TO_VALUE - m.cap(value_spec(VALUE))) + cap_v;
    let full_h = 92.0;
    let two_h = cap_l + label_to_value + 2.0 * 12.0;
    let (label_y, value_y, delta_y) = if delta.is_some() && r.h >= full_h {
        let off = r.y + (r.h - 100.0) / 2.0;
        (
            Some(off + LABEL_Y),
            off + LABEL_Y + label_to_value,
            Some(off + DELTA_Y + (label_to_value - LABEL_TO_VALUE)),
        )
    } else if r.h >= two_h {
        let block = cap_l + label_to_value;
        let top = r.y + (r.h - block) / 2.0;
        (Some(top + cap_l), top + cap_l + label_to_value, None)
    } else {
        (None, r.y + (r.h + cap_v) / 2.0, None)
    };
    let mut l = TileLayout {
        rect: r,
        value_size: size,
        ..TileLayout::default()
    };
    if let Some(ly) = label_y {
        l.pieces.push(Piece {
            role: Role::Label,
            text: m.fit(&model.label, label_spec(), inner),
            x,
            y: ly,
            spec: label_spec(),
            anchor: Anchor::Start,
        });
    }
    l.pieces.push(Piece {
        role: if model.value.is_nan() {
            Role::Quiet
        } else {
            Role::Value
        },
        text,
        x,
        y: value_y,
        spec: value_spec(size),
        anchor: Anchor::Start,
    });
    if show_unit && let Some(u) = &model.unit {
        l.pieces.push(Piece {
            role: Role::Quiet,
            text: u.clone(),
            x: x + vw + UNIT_GAP,
            y: value_y,
            spec: unit_spec(),
            anchor: Anchor::Start,
        });
    }
    if let (Some(dy), Some((dir, t))) = (delta_y, delta) {
        match dir {
            Some(d) => {
                l.arrow = Some((d, (x, dy)));
                l.good = model.change.as_ref().and_then(|c| c.good);
                l.pieces.push(Piece {
                    role: Role::Delta,
                    text: t,
                    x: x + lead,
                    y: dy,
                    spec: dspec,
                    anchor: Anchor::Start,
                });
            }
            None => l.pieces.push(Piece {
                role: Role::Quiet,
                text: t,
                x,
                y: dy,
                spec: dspec,
                anchor: Anchor::Start,
            }),
        }
    }
    if trend_fits && label_y.is_some() {
        let x1 = r.right() - PAD;
        let y1 = value_y + 2.0;
        l.trend = Some(Rect::new(x1 - trend_w, y1 - TREND_H, trend_w, TREND_H));
    }
    l
}

fn meter_layout(model: &TileModel, r: Rect, m: &mut dyn Measure) -> TileLayout {
    let inner = (r.w - 2.0 * PAD).max(0.0);
    let x = r.x + PAD;
    let lspec = label_spec();
    let share_spec = TextSpec::new(13.0).weight(Weight::Semibold);
    let of_spec = TextSpec::new(LABEL);
    let cap_l = m.cap(lspec);
    let block = cap_l + TRACK_GAP + TRACK_H;
    let (label_y, track_y) = if r.h >= block + 2.0 * 8.0 {
        let top = r.y + (r.h - block) / 2.0;
        (Some(top + cap_l), top + cap_l + TRACK_GAP)
    } else {
        (None, r.y + (r.h - TRACK_H) / 2.0)
    };
    let mut l = TileLayout {
        rect: r,
        value_size: 13.0,
        track: Some(Rect::new(x, track_y, inner, TRACK_H)),
        ..TileLayout::default()
    };
    let Some(ly) = label_y else {
        return l;
    };
    let share = if model.value.is_nan() {
        "no data".to_string()
    } else {
        format!("{} %", short_number(model.fraction() * 100.0))
    };
    let of = if model.value.is_nan() {
        String::new()
    } else {
        let u = model.unit.as_deref();
        format!(
            " \u{b7} {} of {}",
            model.short(model.value),
            with_unit(&model.short(model.max), u)
        )
    };
    let status = model.status();
    let word = status.map(status_name).unwrap_or_default();
    let sw = m.width(&share, share_spec);
    let ow = m.width(&of, of_spec);
    // The status's icon (and its word, then a gap) before the share.
    let icon_w = if status.is_some() {
        2.0 * ICON_R + STATUS_GAP
    } else {
        0.0
    };
    let ww = if status.is_some() {
        ICON_GAP + m.width(word, lspec)
    } else {
        0.0
    };
    let lw = m.width(&model.label, lspec);
    // What the right side keeps, widest first, with room for the label's start: `of max` goes
    // first, then the status's word; its icon stays while the fill wears its colour, so the
    // status never rests on colour alone (the label is cut instead).
    let label_min = lw.min(48.0);
    let fits = |w: f32| label_min + 16.0 + w <= inner;
    let (show_of, show_word) = if fits(icon_w + ww + sw + ow) {
        (true, status.is_some())
    } else if fits(icon_w + ww + sw) {
        (false, status.is_some())
    } else {
        (false, false)
    };
    let show_icon = status.is_some();
    let mut right = r.right() - PAD;
    if show_of && !of.is_empty() {
        l.pieces.push(Piece {
            role: Role::Quiet,
            text: of.clone(),
            x: right,
            y: ly,
            spec: of_spec,
            anchor: Anchor::End,
        });
        right -= ow;
    }
    l.pieces.push(Piece {
        role: if model.value.is_nan() {
            Role::Quiet
        } else {
            Role::Share
        },
        text: share,
        x: right,
        y: ly,
        spec: share_spec,
        anchor: Anchor::End,
    });
    right -= sw;
    if let Some(s) = status.filter(|_| show_icon) {
        right -= STATUS_GAP;
        if show_word {
            l.pieces.push(Piece {
                role: Role::Status,
                text: word.to_string(),
                x: right,
                y: ly,
                spec: lspec,
                anchor: Anchor::End,
            });
            right -= ww;
        }
        l.icon = Some((s, (right - ICON_R, ly - cap_l / 2.0)));
        right -= 2.0 * ICON_R;
    }
    let room = (right - 16.0 - x).max(0.0);
    let label = m.fit(&model.label, lspec, room);
    if !label.is_empty() {
        l.pieces.insert(
            0,
            Piece {
                role: Role::Label,
                text: label,
                x,
                y: ly,
                spec: lspec,
                anchor: Anchor::Start,
            },
        );
    }
    l
}

fn spark_layout(model: &TileModel, r: Rect, m: &mut dyn Measure) -> TileLayout {
    let inner = (r.w - 2.0 * PAD).max(0.0);
    let x = r.x + PAD;
    let lspec = label_spec();
    let text = model.text();
    let mut l = TileLayout {
        rect: r,
        ..TileLayout::default()
    };
    let unit = model.unit.as_deref().filter(|u| !u.is_empty());
    if r.h >= BLOCK_H {
        // The label and the value on top, the line under them.
        let (size, show_unit, vw, uw) = value_row(&text, unit, inner * 0.6, m);
        let cap_l = m.cap(lspec);
        let ly = r.y + 26.0;
        let vy = ly - cap_l + m.cap(value_spec(size));
        let right = r.right() - PAD;
        let uw = if show_unit { uw } else { 0.0 };
        let vx = right - uw - vw;
        l.value_size = size;
        l.pieces.push(Piece {
            role: if model.value.is_nan() {
                Role::Quiet
            } else {
                Role::Value
            },
            text,
            x: vx,
            y: vy,
            spec: value_spec(size),
            anchor: Anchor::Start,
        });
        if show_unit && let Some(u) = unit {
            l.pieces.push(Piece {
                role: Role::Quiet,
                text: u.to_string(),
                x: vx + vw + UNIT_GAP,
                y: vy,
                spec: unit_spec(),
                anchor: Anchor::Start,
            });
        }
        let label = m.fit(&model.label, lspec, (vx - 16.0 - x).max(0.0));
        if !label.is_empty() {
            l.pieces.insert(
                0,
                Piece {
                    role: Role::Label,
                    text: label,
                    x,
                    y: ly,
                    spec: lspec,
                    anchor: Anchor::Start,
                },
            );
        }
        let top = vy + 20.0;
        let bottom = r.bottom() - PAD;
        if model.trend.len() >= 2 && bottom - top >= 16.0 {
            l.trend = Some(Rect::new(x, top, inner, bottom - top));
        }
        return l;
    }
    // A row: the label, the line, the value.
    let size = 15.0;
    let vspec = value_spec(size);
    let cy = r.y + r.h / 2.0;
    let vw = m.width(&text, vspec);
    let uw = unit.map_or(0.0, |u| UNIT_GAP + m.width(u, TextSpec::new(LABEL)));
    let show_unit = uw > 0.0 && vw + uw <= inner * 0.45;
    let right = r.right() - PAD;
    let vx = right - vw - if show_unit { uw } else { 0.0 };
    let vy = cy + m.cap(vspec) / 2.0;
    l.value_size = size;
    l.pieces.push(Piece {
        role: if model.value.is_nan() {
            Role::Quiet
        } else {
            Role::Value
        },
        text,
        x: vx,
        y: vy,
        spec: vspec,
        anchor: Anchor::Start,
    });
    if show_unit && let Some(u) = unit {
        l.pieces.push(Piece {
            role: Role::Quiet,
            text: u.to_string(),
            x: vx + vw + UNIT_GAP,
            y: vy,
            spec: TextSpec::new(LABEL),
            anchor: Anchor::Start,
        });
    }
    let label = m.fit(&model.label, lspec, (inner * 0.32).max(0.0));
    let lw = m.width(&label, lspec);
    if !label.is_empty() {
        l.pieces.insert(
            0,
            Piece {
                role: Role::Label,
                text: label,
                x,
                y: cy + m.cap(lspec) / 2.0,
                spec: lspec,
                anchor: Anchor::Start,
            },
        );
    }
    let x0 = if lw > 0.0 { x + lw + TREND_GAP } else { x };
    let x1 = vx - TREND_GAP;
    let half = ((r.h - 2.0 * 12.0) / 2.0).clamp(0.0, 18.0);
    if model.trend.len() >= 2 && x1 - x0 >= 24.0 && half >= 4.0 {
        l.trend = Some(Rect::new(x0, cy - half, x1 - x0, 2.0 * half));
    }
    l
}

/// The trend's points on screen in `area`: along its domain across, from its least to its most
/// value up (a flat trend along the middle).
pub fn trend_points(model: &TileModel, area: Rect) -> Vec<(f32, f32)> {
    let (d0, d1) = model.trend_domain;
    let (lo, hi) = model
        .trend
        .iter()
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), p| {
            (a.min(p.y), b.max(p.y))
        });
    model
        .trend
        .iter()
        .map(|p| {
            let fx = if d1 > d0 {
                ((p.x - d0) / (d1 - d0)).clamp(0.0, 1.0) as f32
            } else {
                0.5
            };
            let fy = if hi > lo {
                ((p.y - lo) / (hi - lo)) as f32
            } else {
                0.5
            };
            (area.x + fx * area.w, area.bottom() - fy * area.h)
        })
        .collect()
}

/// The points a line is drawn through: every one, or LTTB's pick where there are more than
/// two a pixel column.
fn drawn(model: &TileModel, pts: &[(f32, f32)], area: Rect) -> Vec<(f32, f32)> {
    let most = (area.w.max(2.0) * 2.0) as usize;
    if pts.len() <= most {
        return pts.to_vec();
    }
    let xs: Vec<f64> = model.trend.iter().map(|p| p.x).collect();
    let ys: Vec<f64> = model.trend.iter().map(|p| p.y).collect();
    lttb(&xs, &ys, most).into_iter().map(|i| pts[i]).collect()
}

fn round_stroke(width: f32, color: Color, alpha: f32) -> Stroke {
    Stroke {
        cap: Cap::Round,
        join: Join::Round,
        ..Stroke::solid(width, color, alpha)
    }
}

/// A dot with a ring in the surface colour.
fn dot(scene: &mut Scene, (x, y): (f32, f32), r: f32, ring: f32, color: Color, theme: &Theme) {
    scene.push(Item::Path {
        path: PathData::circle(x, y, r + ring),
        fill: Some(Paint::solid(theme.surface, 1.0)),
        stroke: None,
    });
    scene.push(Item::Path {
        path: PathData::circle(x, y, r),
        fill: Some(Paint::solid(color, 1.0)),
        stroke: None,
    });
}

/// The colour a status wears as text on the theme's ground.
fn delta_color(good: Option<bool>, theme: &Theme) -> Color {
    match good {
        Some(true) => readable(theme.status[0], theme),
        Some(false) => readable(theme.status[2], theme),
        None => theme.muted,
    }
}

/// The tile's base: its text, the delta's arrow, the trend, a meter's track and fill. Nothing
/// on `rect`'s edge: the dashboard's panel is its frame.
pub fn base(model: &TileModel, rect: Rect, theme: &Theme, m: &mut dyn Measure) -> Scene {
    let l = layout(model, rect, m);
    let mut scene = Scene::new(rect.right(), rect.bottom(), 1.0);
    scene.push(Item::PushClip(rect));
    for p in &l.pieces {
        let color = match p.role {
            Role::Label | Role::Quiet => theme.muted,
            Role::Value | Role::Share => theme.strong,
            Role::Delta => delta_color(l.good, theme),
            Role::Status => theme.text,
        };
        scene.push(Item::Text {
            text: p.text.as_str().into(),
            x: p.x,
            y: p.y,
            spec: p.spec,
            anchor: p.anchor,
            color,
            alpha: 1.0,
            max_width: None,
        });
    }
    if let Some((dir, (x, y))) = l.arrow {
        arrow(&mut scene, dir, x, y, delta_color(l.good, theme));
    }
    if let Some(area) = l.trend {
        let pts = trend_points(model, area);
        let line = drawn(model, &pts, area);
        let accent = theme.series[0];
        match model.kind {
            TileKind::Sparkline => {
                let mut wash = line.clone();
                wash.push((line[line.len() - 1].0, area.bottom()));
                wash.push((line[0].0, area.bottom()));
                let mut path = PathData::polyline(&wash);
                path.verbs.push(Verb::Close);
                scene.push(Item::Path {
                    path,
                    fill: Some(Paint::solid(accent, WASH)),
                    stroke: None,
                });
                let big = area.h > 48.0;
                scene.push(Item::Path {
                    path: PathData::polyline(&line),
                    fill: None,
                    stroke: Some(round_stroke(
                        if big { 2.0 } else { TREND_LINE },
                        accent,
                        1.0,
                    )),
                });
                if let Some(&p) = pts.last() {
                    let r = if big { 4.0 } else { DOT };
                    dot(
                        &mut scene,
                        p,
                        r,
                        if big { 2.0 } else { RING },
                        accent,
                        theme,
                    );
                }
            }
            _ => {
                scene.push(Item::Path {
                    path: PathData::polyline(&line),
                    fill: None,
                    stroke: Some(round_stroke(TREND_LINE, theme.other, 1.0)),
                });
                if let Some(&p) = pts.last() {
                    dot(&mut scene, p, DOT, RING, accent, theme);
                }
            }
        }
    }
    if let Some(t) = l.track {
        meter_track(&mut scene, model, t, theme);
    }
    if let Some((s, c)) = l.icon {
        status_icon(&mut scene, s, c, ICON_R, theme);
    }
    scene.push(Item::PopClip);
    scene
}

/// The delta's arrow, its baseline's left end at (x, y): a triangle up or down, or a short dash
/// for no change.
fn arrow(scene: &mut Scene, dir: i8, x: f32, y: f32, color: Color) {
    // Level with the text's lower-case letters' middle, as m4's glyph sits.
    let (top, bottom) = (y - ARROW_H - 0.5, y - 0.5);
    let path = match dir {
        1 => PathData {
            verbs: vec![
                Verb::Move(x + ARROW_W / 2.0, top),
                Verb::Line(x + ARROW_W, bottom),
                Verb::Line(x, bottom),
                Verb::Close,
            ],
        },
        -1 => PathData {
            verbs: vec![
                Verb::Move(x, top),
                Verb::Line(x + ARROW_W, top),
                Verb::Line(x + ARROW_W / 2.0, bottom),
                Verb::Close,
            ],
        },
        _ => PathData::rounded(
            Rect::new(x + 0.5, (top + bottom) / 2.0 - 1.0, ARROW_W - 1.0, 2.0),
            [1.0; 4],
        ),
    };
    scene.push(Item::Path {
        path,
        fill: Some(Paint::solid(color, 1.0)),
        stroke: None,
    });
}

/// The colour a status fills with.
fn status_color(s: Status, theme: &Theme) -> Color {
    theme.status[s as usize]
}

/// A meter's track, its fill and its thresholds' ticks.
fn meter_track(scene: &mut Scene, model: &TileModel, t: Rect, theme: &Theme) {
    scene.push(Item::Path {
        path: PathData::rounded(t, [TRACK_R; 4]),
        fill: Some(Paint::solid(theme.grid, 1.0)),
        stroke: None,
    });
    // Each threshold's tick under the fill: across the empty track and past its edges, so a
    // filled stretch stays one clean bar with the ticks' ends beside it.
    for th in &model.thresholds {
        if th.at <= 0.0 || th.at >= 1.0 {
            continue;
        }
        let x = t.x + t.w * th.at as f32;
        scene.push(Item::Path {
            path: PathData::polyline(&[(x, t.y - TICK_OUT), (x, t.bottom() + TICK_OUT)]),
            fill: None,
            stroke: Some(Stroke::hairline(theme.muted, 0.9)),
        });
    }
    let f = model.fraction() as f32;
    if f > 0.0 {
        let w = (t.w * f).max(1.0);
        let r = TRACK_R.min(w / 2.0);
        let fill = model
            .status()
            .map_or(theme.series[0], |s| status_color(s, theme));
        scene.push(Item::Path {
            path: PathData::rounded(Rect::new(t.x, t.y, w, t.h), [r; 4]),
            fill: Some(Paint::solid(fill, 1.0)),
            stroke: None,
        });
    }
}

/// A status's icon centred at `c`, radius `r`: good a disc with a tick, warning a triangle
/// with "!", serious a diamond with "!", critical a disc with "!"; the mark white or near
/// black by the fill's luminance, so a status never rests on colour alone.
pub fn status_icon(scene: &mut Scene, s: Status, c: (f32, f32), r: f32, theme: &Theme) {
    let (cx, cy) = c;
    let fill = status_color(s, theme);
    let ink = on_fill(fill);
    let shape = match s {
        Status::Good | Status::Critical => PathData::circle(cx, cy, r),
        Status::Warning => PathData {
            verbs: vec![
                Verb::Move(cx, cy - r * 1.0),
                Verb::Line(cx + r * 1.1, cy + r * 0.9),
                Verb::Line(cx - r * 1.1, cy + r * 0.9),
                Verb::Close,
            ],
        },
        Status::Serious => PathData {
            verbs: vec![
                Verb::Move(cx, cy - r * 1.1),
                Verb::Line(cx + r * 1.1, cy),
                Verb::Line(cx, cy + r * 1.1),
                Verb::Line(cx - r * 1.1, cy),
                Verb::Close,
            ],
        },
    };
    scene.push(Item::Path {
        path: shape,
        fill: Some(Paint::solid(fill, 1.0)),
        stroke: Some(round_stroke(r * 0.25, fill, 1.0)),
    });
    if s == Status::Good {
        scene.push(Item::Path {
            path: PathData::polyline(&[
                (cx - r * 0.45, cy + r * 0.02),
                (cx - r * 0.1, cy + r * 0.38),
                (cx + r * 0.48, cy - r * 0.32),
            ]),
            fill: None,
            stroke: Some(round_stroke(r * 0.26, ink, 1.0)),
        });
        return;
    }
    let dy = if s == Status::Warning { r * 0.2 } else { 0.0 };
    let bar = Rect::new(cx - r * 0.12, cy - r * 0.52 + dy, r * 0.24, r * 0.56);
    scene.push(Item::Path {
        path: PathData::rounded(bar, [r * 0.12; 4]),
        fill: Some(Paint::solid(ink, 1.0)),
        stroke: None,
    });
    scene.push(Item::Path {
        path: PathData::circle(cx, cy + r * 0.38 + dy, r * 0.14),
        fill: Some(Paint::solid(ink, 1.0)),
        stroke: None,
    });
}

/// What the pointer at `at` is over on the tile in `rect`: a point of its trend, the tile, or
/// nothing off it. Laid out with [`FixedMeasure`]; [`hit_with`] takes the fonts' measure.
pub fn hit(model: &TileModel, rect: Rect, at: (f32, f32)) -> Option<TileHover> {
    hit_with(model, rect, at, &mut FixedMeasure)
}

/// [`hit`] laid out with `m`.
pub fn hit_with(
    model: &TileModel,
    rect: Rect,
    at: (f32, f32),
    m: &mut dyn Measure,
) -> Option<TileHover> {
    if !rect.contains(at.0, at.1) {
        return None;
    }
    let l = layout(model, rect, m);
    if let Some(area) = l.trend {
        // The trend's whole height and a little past its ends; a sparkline's the tile's
        // height, as a chart's crosshair takes the plot's.
        let reach = if model.kind == TileKind::Sparkline {
            Rect::new(area.x - 6.0, rect.y, area.w + 12.0, rect.h)
        } else {
            area.inflate(6.0)
        };
        if reach.contains(at.0, at.1) {
            let pts = trend_points(model, area);
            let i = (0..pts.len())
                .min_by(|&a, &b| (pts[a].0 - at.0).abs().total_cmp(&(pts[b].0 - at.0).abs()))?;
            return Some(TileHover::Trend {
                point: i,
                card: true,
            });
        }
    }
    Some(TileHover::Tile { at })
}

/// The overlay over the tile in `rect`, its card free to go anywhere under, over or beside it
/// (as a card on a dashboard may lie over the next panel).
pub fn overlay(
    model: &TileModel,
    hover: Option<TileHover>,
    rect: Rect,
    theme: &Theme,
    m: &mut dyn Measure,
) -> Scene {
    let area = Rect::new(-1e6, -1e6, 2e6, 2e6);
    overlay_in(model, hover, rect, area, theme, m)
}

/// [`overlay`] with the card kept inside `area` (the window's).
pub fn overlay_in(
    model: &TileModel,
    hover: Option<TileHover>,
    rect: Rect,
    area: Rect,
    theme: &Theme,
    m: &mut dyn Measure,
) -> Scene {
    let mut scene = Scene::new(rect.right(), rect.bottom(), 1.0);
    let Some(hover) = hover else {
        return scene;
    };
    let l = layout(model, rect, m);
    let unit = model.unit.as_deref();
    match hover {
        TileHover::Trend { point, card } if l.trend.is_some() && point < model.trend.len() => {
            let t = l.trend.expect("checked");
            let pts = trend_points(model, t);
            let p = pts[point];
            let x = p.0.round() + 0.5;
            let (y0, y1) = if model.kind == TileKind::Sparkline {
                (t.y - 4.0, t.bottom())
            } else {
                (t.y - 4.0, t.bottom() + 4.0)
            };
            scene.push(Item::Path {
                path: PathData::polyline(&[(x, y0), (x, y1)]),
                fill: None,
                stroke: Some(Stroke::hairline(theme.text, 0.5)),
            });
            let r = if model.kind == TileKind::Sparkline && t.h > 48.0 {
                4.5
            } else {
                3.5
            };
            dot(&mut scene, p, r, 2.0, theme.series[0], theme);
            if card {
                let tp = model.trend[point];
                let rows = vec![CardRow {
                    value: with_unit(&model.full(tp.y), unit),
                    name: model.aggregate_text_at(),
                    key: Some(Key::Line(if model.kind == TileKind::Sparkline {
                        theme.series[0]
                    } else {
                        theme.other
                    })),
                }];
                let header = model.point_text(point);
                let size = card_size(&header, &rows, m);
                let near = Rect::new(p.0 - 6.0, p.1 - 6.0, 12.0, 12.0);
                let r = if model.kind == TileKind::Sparkline && rect.h >= BLOCK_H {
                    // A sparkline drawn large: beside the crosshair at the line's top, as a
                    // chart's card goes, never over the tile's edge.
                    beside(size, p.0, t.y + 4.0, rect.intersect(&area).unwrap_or(rect))
                } else {
                    place(size, rect, near, p.0, area)
                };
                draw_card(&mut scene, r, &header, &rows, theme, m);
            }
        }
        TileHover::Trend { .. } | TileHover::Tile { .. } => {
            let at = match hover {
                TileHover::Tile { at } => at,
                _ => (rect.x + rect.w / 2.0, rect.y + rect.h / 2.0),
            };
            let (header, rows) = tile_card(model);
            let size = card_size(&header, &rows, m);
            let near = Rect::new(at.0 - 4.0, at.1 - 4.0, 8.0, 8.0);
            let r = place(size, rect, near, at.0, area);
            draw_card(&mut scene, r, &header, &rows, theme, m);
        }
    }
    scene
}

impl TileModel {
    /// What a trend point's value is, for its card: the stat's aggregate over the point's part
    /// ("sum of rpm"), a sparkline's field.
    fn aggregate_text_at(&self) -> String {
        match (self.kind, &self.field) {
            (TileKind::Sparkline, Some(f)) => f.clone(),
            _ => self.aggregate_text(),
        }
    }
}

/// The card of a tile as a whole: its header and rows.
fn tile_card(model: &TileModel) -> (String, Vec<CardRow>) {
    let unit = model.unit.as_deref();
    let mut rows = Vec::new();
    let value = if model.value.is_nan() {
        "no data".to_string()
    } else {
        model.exact_with_unit()
    };
    rows.push(CardRow {
        value,
        name: model.aggregate_text(),
        key: None,
    });
    let header = match model.kind {
        TileKind::Meter if model.range.is_none() => model.label.clone(),
        _ => model.range_text(),
    };
    match model.kind {
        TileKind::Stat => {
            if let Some(c) = &model.change {
                if c.missing() {
                    rows.push(CardRow {
                        value: "no data".into(),
                        name: c.period.clone(),
                        key: None,
                    });
                } else {
                    rows.push(CardRow {
                        value: with_unit(&model.full(c.against), unit),
                        name: c.period.clone(),
                        key: None,
                    });
                    let sign = match c.direction() {
                        1 => "+",
                        -1 => "\u{2212}",
                        _ => "",
                    };
                    let amount = model.change_text().map(|(a, _)| a).unwrap_or_default();
                    rows.push(CardRow {
                        value: format!("{sign}{amount}"),
                        name: match c.good {
                            Some(true) => "change, good".into(),
                            Some(false) => "change, bad".into(),
                            None => "change".into(),
                        },
                        key: None,
                    });
                }
            }
        }
        TileKind::Meter => {
            rows.push(CardRow {
                value: with_unit(&model.full(model.max), unit),
                name: "max".into(),
                key: None,
            });
            rows.push(CardRow {
                value: format!("{} %", short_number(model.fraction() * 100.0)),
                name: "of max".into(),
                key: None,
            });
            for th in &model.thresholds {
                rows.push(CardRow {
                    value: format!("{} %", short_number(th.at * 100.0)),
                    name: status_name(th.status).into(),
                    key: Some(Key::Status(th.status)),
                });
            }
        }
        TileKind::Sparkline => {
            let ys: Vec<f64> = model.trend.iter().map(|p| p.y).collect();
            if ys.len() >= 2 {
                let lo = ys.iter().copied().fold(f64::INFINITY, f64::min);
                let hi = ys.iter().copied().fold(f64::NEG_INFINITY, f64::max);
                rows.push(CardRow {
                    value: with_unit(&model.full(lo), unit),
                    name: "lowest".into(),
                    key: None,
                });
                rows.push(CardRow {
                    value: with_unit(&model.full(hi), unit),
                    name: "highest".into(),
                    key: None,
                });
            }
        }
    }
    (header, rows)
}

// ------------------------------------------------------------------ the card

/// A card's row: its value (semibold, tabular), its name after, and a key before.
struct CardRow {
    value: String,
    name: String,
    key: Option<Key>,
}

/// A card row's key.
#[derive(Clone, Copy)]
enum Key {
    /// A short line: a trend.
    Line(Color),
    /// A status's icon.
    Status(Status),
}

const CARD_PAD: f32 = 12.0;
const CARD_RADIUS: f32 = 8.0;
const CARD_ROW: f32 = 21.0;
const CARD_KEY: f32 = 14.0;
const CARD_GAP: f32 = 9.0;
/// How far a card keeps from the tile or the point it's about.
const CARD_CLEAR: f32 = 8.0;

fn card_specs() -> (TextSpec, TextSpec, TextSpec) {
    (
        TextSpec::new(11.0).tabular(),
        TextSpec::new(13.0).weight(Weight::Semibold).tabular(),
        TextSpec::new(12.0),
    )
}

/// A card's size: its header over its rows, as a chart's card.
fn card_size(header: &str, rows: &[CardRow], m: &mut dyn Measure) -> (f32, f32) {
    let (hspec, vspec, nspec) = card_specs();
    let keys = rows.iter().any(|r| r.key.is_some());
    let vw = rows
        .iter()
        .map(|r| m.width(&r.value, vspec))
        .fold(0.0, f32::max);
    let nw = rows
        .iter()
        .map(|r| m.width(&r.name, nspec))
        .fold(0.0, f32::max);
    let body = CARD_PAD + if keys { CARD_KEY + CARD_GAP } else { 0.0 } + vw + CARD_GAP + nw + 14.0;
    let w = body.max(m.width(header, hspec) + 2.0 * CARD_PAD).ceil();
    let h = 32.0 + rows.len() as f32 * CARD_ROW + 7.0;
    (w, h)
}

/// Where a card of `size` goes: under `avoid` (the tile) at `x`, else over it, else right of
/// `near` (the point), else left of it; inside `area` where it fits, clamped into it where
/// nothing does.
fn place(size: (f32, f32), avoid: Rect, near: Rect, x: f32, area: Rect) -> Rect {
    let (w, h) = size;
    let left = (x - 24.0).clamp(area.x + 4.0, (area.right() - 4.0 - w).max(area.x + 4.0));
    let mid = near.y + near.h / 2.0 - h / 2.0;
    let cands = [
        Rect::new(left, avoid.bottom() + CARD_CLEAR, w, h),
        Rect::new(left, avoid.y - CARD_CLEAR - h, w, h),
        Rect::new(near.right() + CARD_CLEAR, mid, w, h),
        Rect::new(near.x - CARD_CLEAR - w, mid, w, h),
    ];
    let inside = |r: &Rect| {
        r.x >= area.x && r.y >= area.y && r.right() <= area.right() && r.bottom() <= area.bottom()
    };
    for c in cands.iter().take(2) {
        if inside(c) {
            return *c;
        }
    }
    // Beside the point, slid up or down into the area.
    for c in &cands[2..] {
        let y =
            c.y.clamp(area.y + 4.0, (area.bottom() - 4.0 - h).max(area.y + 4.0));
        let r = Rect::new(c.x, y, w, h);
        if inside(&r) {
            return r;
        }
    }
    let r = cands[0];
    Rect::new(
        r.x.clamp(area.x, (area.right() - w).max(area.x)),
        r.y.clamp(area.y, (area.bottom() - h).max(area.y)),
        w,
        h,
    )
}

/// A card of `size` beside the crosshair at `x`, its top at `top`: right of it where it fits
/// in `within`, else left of it, slid inside.
fn beside(size: (f32, f32), x: f32, top: f32, within: Rect) -> Rect {
    let (w, h) = size;
    let right = x + CARD_CLEAR + 4.0;
    let left = if right + w <= within.right() - 4.0 {
        right
    } else {
        (x - CARD_CLEAR - 4.0 - w).max(within.x + 4.0)
    };
    let y = top.clamp(
        within.y + 4.0,
        (within.bottom() - 4.0 - h).max(within.y + 4.0),
    );
    Rect::new(left, y, w, h)
}

/// The mini crosshair a dashboard's linked time `t` draws on the tile's trend, without a card:
/// at the trend's point nearest `t` within half a part ([`TileModel::trend_at`]); None for a
/// tile without a trend, or a time too far from it.
pub fn hover_at(model: &TileModel, t: f64) -> Option<TileHover> {
    model
        .trend_at(t)
        .map(|point| TileHover::Trend { point, card: false })
}

fn draw_card(
    scene: &mut Scene,
    r: Rect,
    header: &str,
    rows: &[CardRow],
    theme: &Theme,
    m: &mut dyn Measure,
) {
    let (hspec, vspec, nspec) = card_specs();
    scene.push(Item::Shadow {
        rect: r,
        radius: CARD_RADIUS,
        blur: 8.0,
        dy: 6.0,
        alpha: if theme.dark { 0.55 } else { 0.16 },
    });
    scene.push(Item::Path {
        path: PathData::rounded(r, [CARD_RADIUS; 4]),
        fill: Some(Paint::solid(theme.tooltip, 1.0)),
        stroke: Some(Stroke::hairline(theme.ring, 1.0)),
    });
    scene.push(Item::Text {
        text: header.into(),
        x: r.x + CARD_PAD,
        y: r.y + 22.0,
        spec: hspec,
        anchor: Anchor::Start,
        color: theme.muted,
        alpha: 1.0,
        max_width: Some(r.w - 2.0 * CARD_PAD),
    });
    let keys = rows.iter().any(|r| r.key.is_some());
    let vw = rows
        .iter()
        .map(|r| m.width(&r.value, vspec))
        .fold(0.0, f32::max);
    let vx = r.x + CARD_PAD + if keys { CARD_KEY + CARD_GAP } else { 0.0 } + vw;
    let nx = vx + CARD_GAP;
    for (k, row) in rows.iter().enumerate() {
        let y0 = r.y + 32.0 + k as f32 * CARD_ROW;
        let mid = y0 + 10.0;
        match row.key {
            Some(Key::Line(c)) => scene.push(Item::Path {
                path: PathData::polyline(&[
                    (r.x + CARD_PAD, mid),
                    (r.x + CARD_PAD + CARD_KEY, mid),
                ]),
                fill: None,
                stroke: Some(round_stroke(2.0, c, 1.0)),
            }),
            Some(Key::Status(s)) => {
                status_icon(scene, s, (r.x + CARD_PAD + CARD_KEY / 2.0, mid), 5.0, theme)
            }
            None => {}
        }
        let base = y0 + 14.5;
        scene.push(Item::Text {
            text: row.value.as_str().into(),
            x: vx,
            y: base,
            spec: vspec,
            anchor: Anchor::End,
            color: theme.strong,
            alpha: 1.0,
            max_width: None,
        });
        let room = (r.right() - 14.0 - nx).max(1.0);
        scene.push(Item::Text {
            text: m.fit(&row.name, nspec, room).into(),
            x: nx,
            y: base,
            spec: nspec,
            anchor: Anchor::Start,
            color: theme.text,
            alpha: 1.0,
            max_width: Some(room),
        });
    }
}
