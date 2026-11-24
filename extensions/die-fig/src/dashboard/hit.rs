//! What's under the pointer on a dashboard ([`hit`]), and what the panel there shows for it
//! ([`hover`]): the panel and the panel's own hit, as its figure alone would say it, and for a
//! linked panel the moment on the shared time.

use super::DashModel;
use super::layout::{DashLayout, PanelFigure};
use crate::data::SeriesKey;
use crate::figure::{self, TileHover};
use crate::graph::{self, Target};
use crate::interact::Cursor;
use crate::plot;
use crate::scene::{Hit, Rect};

/// What a place on a dashboard is.
#[derive(Clone, Debug, PartialEq)]
pub enum DashHit {
    /// The time-range chip: a click opens the ranges.
    Chip,
    /// The live status: a click re-runs.
    Live,
    /// The scrollbar, on its thumb or not.
    Scrollbar {
        /// Whether the thumb is under the pointer.
        thumb: bool,
    },
    /// A nested dashboard's frame or title, by its number in [`DashLayout::groups`].
    Group(usize),
    /// A panel: its number, its own hit (as its figure alone gives it, a figure's panel
    /// numbers the panel's; [`Hit::PanelTitle`] on its title; [`Hit::Plot`] on a tile), and the
    /// point in the figure's own px.
    Panel {
        /// The panel.
        panel: usize,
        /// What of it.
        hit: Hit,
        /// Where, in the figure's px.
        at: (f32, f32),
    },
    /// The header's ground, the margins and the gaps.
    Chrome,
}

/// What the pointer is on in the panel under it.
#[derive(Clone, Debug, PartialEq)]
pub enum PanelHover {
    /// On a chart: what it's on as a cursor, and the series nearest it.
    Chart {
        /// What the crosshair and card follow.
        cursor: Option<Cursor>,
        /// The series brought forward.
        emphasis: Option<SeriesKey>,
    },
    /// On a graph: the node or edge.
    Graph(Option<Target>),
    /// On a gantt: the task, by its bar or its label.
    Gantt {
        /// The task under the pointer.
        task: Option<crate::gantt::Target>,
        /// Whether the pointer hovers there (no gesture under way, no key since it moved): off
        /// any bar, the date under it is said.
        hovering: bool,
    },
    /// On a tile.
    Tile(Option<TileHover>),
    /// On the panel's title.
    Title,
    /// On the stale badge.
    Badge,
    /// On the error figure or its log.
    Error,
    /// On the panel, where nothing shows (a waiting panel, its padding).
    Nothing,
}

/// The hover on a dashboard: the panel, the pointer, what it's on and, over a linked panel's
/// plot, the moment on the shared time. A drag under way adds its brush.
#[derive(Clone, Debug, PartialEq)]
pub struct DashHover {
    /// The panel under the pointer.
    pub panel: usize,
    /// The pointer, in the area's px.
    pub at: (f32, f32),
    /// What it's on.
    pub target: PanelHover,
    /// The moment on the shared time, over a linked panel: the hovered sample's x (else the
    /// time under the pointer).
    pub time: Option<f64>,
    /// A brush being drawn in the panel: its corners in the area's px.
    pub brush: Option<((f32, f32), (f32, f32))>,
}

/// What's at a point in the area: None outside it.
pub fn hit(layout: &DashLayout, x: f32, y: f32) -> Option<DashHit> {
    let (w, h) = layout.size;
    if !(0.0..=w).contains(&x) || !(0.0..=h).contains(&y) {
        return None;
    }
    let hd = &layout.header;
    if hd
        .chip
        .as_ref()
        .is_some_and(|c| c.rect.inflate(2.0).contains(x, y))
    {
        return Some(DashHit::Chip);
    }
    if hd.live.as_ref().is_some_and(|l| l.rect.contains(x, y)) {
        return Some(DashHit::Live);
    }
    if let Some((track, thumb)) = layout.scrollbar {
        let reach = Rect::new(track.x - 4.0, track.y, track.w + 8.0, track.h);
        if reach.contains(x, y) {
            return Some(DashHit::Scrollbar {
                thumb: thumb.inflate(4.0).contains(x, y),
            });
        }
    }
    if !layout.viewport.contains(x, y) {
        return Some(DashHit::Chrome);
    }
    for p in layout.panels.iter().flatten() {
        if !p.frame.contains(x, y) {
            continue;
        }
        let (fx, fy) = (x - p.frame.x, y - p.frame.y);
        let panel = p.index as u16;
        let own = p.scene.hit(fx, fy).cloned().unwrap_or(Hit::Chrome);
        let hit = match own {
            Hit::PanelTitle(_) => Hit::PanelTitle(panel),
            Hit::Plot(_) => Hit::Plot(panel),
            Hit::AxisX(_) => Hit::AxisX(panel),
            Hit::AxisY(_) => Hit::AxisY(panel),
            Hit::Bar { band, series, .. } => Hit::Bar {
                panel,
                band,
                series,
            },
            Hit::Cell { row, col, .. } => Hit::Cell { panel, row, col },
            Hit::Chrome if matches!(p.figure, PanelFigure::Tile { .. }) => Hit::Plot(panel),
            other => other,
        };
        return Some(DashHit::Panel {
            panel: p.index,
            hit,
            at: p.local(x, y),
        });
    }
    if let Some(k) = layout.groups.iter().rposition(|g| g.frame.contains(x, y)) {
        return Some(DashHit::Group(k));
    }
    Some(DashHit::Chrome)
}

/// What the pointer at (x, y) shows: the panel under it and what it's on there, as that
/// figure's own hover says; None off every panel.
pub fn hover(model: &DashModel, layout: &DashLayout, x: f32, y: f32) -> Option<DashHover> {
    let Some(DashHit::Panel { panel, hit, at }) = hit(layout, x, y) else {
        return None;
    };
    let p = layout.panel(panel)?;
    let pm = model.panels.get(panel)?;
    let mut time = None;
    let target = match hit {
        Hit::PanelTitle(_) => PanelHover::Title,
        Hit::Badge => PanelHover::Badge,
        Hit::ErrorFigure | Hit::LogEntry(_) => PanelHover::Error,
        _ => match (&pm.state, &p.figure) {
            (super::PanelState::Chart(cm), PanelFigure::Chart { layout: cl, hits }) => {
                let cursor = plot::hover(cm, cl, hits, &p.view, at.0, at.1);
                let emphasis = plot::nearest_series(cm, cl, hits, &p.view, at.0, at.1);
                if pm.linked {
                    let snapped = cursor
                        .as_ref()
                        .filter(|c| matches!(c, Cursor::Point { .. }))
                        .and_then(|c| plot::nav::cursor_data(cm, c))
                        .map(|(x, _)| x);
                    let inside = cl.plot.contains(at.0, at.1);
                    time = snapped.or_else(|| inside.then(|| cl.x.invert(at.0)));
                }
                PanelHover::Chart { cursor, emphasis }
            }
            (super::PanelState::Graph(g), PanelFigure::Graph { layout: gl }) => {
                PanelHover::Graph(graph::hover(g, gl, at.0, at.1))
            }
            (super::PanelState::Gantt(g), PanelFigure::Gantt { layout: gl }) => PanelHover::Gantt {
                task: crate::gantt::hover(g, gl, at.0, at.1),
                hovering: true,
            },
            (_, PanelFigure::Tile { model: tm }) => {
                let rect = Rect::new(0.0, 0.0, p.frame.w, p.frame.h);
                PanelHover::Tile(figure::hit(tm, rect, at))
            }
            _ => PanelHover::Nothing,
        },
    };
    Some(DashHover {
        panel,
        at: (x, y),
        target,
        time,
        brush: None,
    })
}
