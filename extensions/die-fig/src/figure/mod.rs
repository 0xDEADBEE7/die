//! The tiles that show one number: stat, meter and sparkline (phase 4).
//!
//! **A placeholder** with the API contract-p4 §2.4 pins, so a dashboard can draw its tile
//! panels while the tiles are built on their own branch: a tile here is its label and its value
//! as text. The tiles' branch replaces this file whole at the merge. Two things a dashboard
//! uses past §2.4's list, both of the tiles' shape: [`TileModel::build_in`] (the model with its
//! times in a zone, which the tiles' branch has) and [`hover_at`] (a linked stat's mini
//! crosshair at the dashboard's time, which the merge writes over the tiles' trend:
//! `model.trend_at(t).map(|point| TileHover::Trend { point, card: false })`).
//!
//! `rect` is the tile panel's frame, which the dashboard draws: a tile insets its own content
//! and draws in `rect`'s own coordinates. §2.4's `Point` is `(f32, f32)`, in those px too.

use crate::data::Table;
use crate::detail::{DetailRow, DetailTree};
use crate::error::{FigError, Result};
use crate::scene::{Anchor, Item, Measure, Rect, Scene, TextSpec, Weight};
use crate::spec::{Aggregate, Figure};
use crate::theme::Theme;
use jiff::tz::TimeZone;

/// A tile's height in a row of tiles, at scale 1 (m4's).
const NATURAL: f32 = 100.0;
/// From the frame to the content.
const INSET: f32 = 16.0;
/// The label's size, and the value's.
const LABEL: f32 = 12.0;
const VALUE: f32 = 26.0;

/// A tile resolved against its data: what it says.
#[derive(Clone, Debug, PartialEq)]
pub struct TileModel {
    /// What it's of.
    pub label: String,
    /// Its value, written.
    pub value: String,
}

/// What the pointer is over on a tile (the tiles' branch's shape).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TileHover {
    /// The tile, the pointer at a place on it.
    Tile {
        /// Where the pointer is, in the area's px.
        at: (f32, f32),
    },
    /// A point of the trend: the mini crosshair there, with its card when `card` (a
    /// dashboard's linked crosshair draws it without).
    Trend {
        /// The point.
        point: usize,
        /// Whether its card shows.
        card: bool,
    },
}

impl TileModel {
    /// The model of a stat or a meter over `table`, its value over the rows inside `range`
    /// (by its trend's field) when given.
    pub fn build(fig: &Figure, table: &Table, range: Option<(f64, f64)>) -> Result<TileModel> {
        TileModel::build_in(fig, table, range, &TimeZone::UTC)
    }

    /// [`TileModel::build`] with its times in `zone`.
    pub fn build_in(
        fig: &Figure,
        table: &Table,
        range: Option<(f64, f64)>,
        zone: &TimeZone,
    ) -> Result<TileModel> {
        let (label, value, trend, format) = match fig {
            Figure::Stat(s) => (&s.label, &s.value, s.trend.as_deref(), &s.format),
            Figure::Meter(m) => (&m.label, &m.value, None, &m.format),
            _ => return Err(FigError::new("a tile is a stat or a meter")),
        };
        let v = aggregate(table, value, trend.zip(range))?;
        let value = match format {
            Some(f) => crate::scale::format_with(v, f, zone),
            None => crate::scale::format_value(v),
        };
        Ok(TileModel {
            label: label.clone(),
            value,
        })
    }

    /// The least room it reads in, px at scale 1.
    pub fn min_size(&self, m: &mut dyn Measure) -> (f32, f32) {
        let w = m
            .width(&self.label, TextSpec::new(LABEL))
            .max(m.width(&self.value, TextSpec::new(VALUE).weight(Weight::Semibold)));
        (w + 2.0 * INSET, NATURAL)
    }

    /// Its height in a row of tiles.
    pub fn natural_height(&self) -> f32 {
        NATURAL
    }
}

/// `value`'s aggregate over the rows of `table` whose `field` falls in `range`, if given.
fn aggregate(table: &Table, value: &Aggregate, range: Option<(&str, (f64, f64))>) -> Result<f64> {
    use crate::data::transform::AggOp;
    let keep: Vec<usize> = match range {
        Some((f, (lo, hi))) => {
            let col =
                (table.column(f)).ok_or_else(|| FigError::new(format!("no column \"{f}\"")))?;
            (0..table.rows)
                .filter(|&r| (lo..=hi).contains(&col.f64_at(r)))
                .collect()
        }
        None => (0..table.rows).collect(),
    };
    let Some(f) = value.field.as_deref() else {
        return Ok(keep.len() as f64);
    };
    let col = table
        .column(f)
        .ok_or_else(|| FigError::new(format!("no column \"{f}\" in the data")))?;
    let mut vs: Vec<f64> = (keep.iter().map(|&r| col.f64_at(r)))
        .filter(|v| v.is_finite())
        .collect();
    let n = vs.len() as f64;
    Ok(match value.op {
        AggOp::Count => n,
        AggOp::Sum => vs.iter().sum(),
        AggOp::Mean if n > 0.0 => vs.iter().sum::<f64>() / n,
        AggOp::Min => vs.iter().copied().fold(f64::NAN, f64::min),
        AggOp::Max => vs.iter().copied().fold(f64::NAN, f64::max),
        AggOp::P50 => crate::data::transform::quantile(&mut vs, 0.5),
        AggOp::P95 => crate::data::transform::quantile(&mut vs, 0.95),
        _ => f64::NAN,
    })
}

/// The tile at rest: its label and value, in `rect`'s coordinates.
pub fn base(model: &TileModel, rect: Rect, theme: &Theme, m: &mut dyn Measure) -> Scene {
    let mut scene = Scene::new(rect.right(), rect.bottom(), 1.0);
    let label = TextSpec::new(LABEL);
    let value = TextSpec::new(VALUE).weight(Weight::Semibold);
    let room = (rect.w - 2.0 * INSET).max(0.0);
    let text = |text: String, y: f32, spec: TextSpec, color| Item::Text {
        text: text.into(),
        x: rect.x + INSET,
        y: rect.y + y,
        spec,
        anchor: Anchor::Start,
        color,
        alpha: 1.0,
        max_width: None,
    };
    scene.push(Item::PushClip(rect));
    scene.push(text(
        m.fit(&model.label, label, room),
        26.0,
        label,
        theme.muted,
    ));
    scene.push(text(model.value.clone(), 60.0, value, theme.strong));
    scene.push(Item::PopClip);
    scene
}

/// What follows the pointer over the tile: nothing, in the placeholder.
pub fn overlay(
    _model: &TileModel,
    _hover: Option<TileHover>,
    rect: Rect,
    _theme: &Theme,
    _m: &mut dyn Measure,
) -> Scene {
    Scene::new(rect.right(), rect.bottom(), 1.0)
}

/// What the pointer at `at` is on.
pub fn hit(_model: &TileModel, rect: Rect, at: (f32, f32)) -> Option<TileHover> {
    rect.contains(at.0, at.1).then_some(TileHover::Tile { at })
}

/// A linked stat's mini crosshair at time `t` on the dashboard's time (not pinned by §2.4; the
/// placeholder has no trend, so nothing).
pub fn hover_at(_model: &TileModel, _t: f64) -> Option<TileHover> {
    None
}

/// The tile's detail tree, for ⏎.
pub fn detail(model: &TileModel) -> DetailTree {
    let mut tree = DetailTree::new(model.label.clone(), "tile");
    tree.rows.push(DetailRow::field("value", &model.value));
    tree
}
