//! Where a dashboard's pieces go at a size (contract-p4 §3.1), in the area's logical px.
//!
//! - **The header** (m4): the title 15 px semibold at the margin, its baseline 38 px down;
//!   under it the time-range chip (its words 12 px, a chevron, the card's ground, a 1 px ring,
//!   6 px corners) when the dashboard has a `time`; at the right of the chip's row (the title's
//!   when there's no chip) the live status when a source is live, said shorter where it would
//!   meet the chip or the title. The rows start 16 px under it.
//! - **The rows**, top to bottom, 28 px in from the sides (less on a narrow area) and 16 px
//!   over the area's foot, 16 px apart (m4), their panels 12 px apart across. A row's panels
//!   share its width by `span`; a row whose panels can't all have their least width wraps
//!   onto as few lines as let them, its panels shared evenly between the lines. A line of tiles only (or of columns of tiles) takes the
//!   tiles' natural height; the other lines share what's left by their row's `height`, each at
//!   least its panels' least (charts and graphs 180 px). When the least heights don't fit, the
//!   rows scroll under the header, with a 4 px scrollbar in the right margin.
//! - **A panel** is a rounded frame. Its title (13 px semibold) and subtitle (12 px muted) sit
//!   16 px in; its figure lays out in the rest by its own layout (a chart with the panel's
//!   insets, a graph and an error figure under the title, a tile over the whole frame), so it
//!   keeps every look rule it has alone. The plots of the charts on one line share their top
//!   and bottom, so their grids line up across the line.
//! - **A column** stacks its panels down its share of the row the same way; **a nested
//!   dashboard** (a `file` panel's) is a frame with its title, its rows laid out inside it.
//! - **Full size**: one panel over the whole area under the header.
//!
//! Each panel's scene is laid out here, with its figure's own layout for the pointer, in the
//! panel's own px (its frame's top left at 0, 0): [`base`](super::base) puts them in place.

use super::{Cell, CellKind, DashModel, DashView, Group, LiveState, PanelKind, PanelState};
use crate::algo::HitIndex;
use crate::figure::{self, RowFit, TileModel};
use crate::graph::{self, GraphLayout};
use crate::interact::View;
use crate::plot::layout::{Ink, Insets, text_box};
use crate::plot::{self, ChartLayout, ErrorView};
use crate::scene::{Anchor, Hit, Item, Measure, Rect, Scene, TextSpec, Weight};
use crate::spec::Tooltip;
use crate::theme::Theme;
use std::path::PathBuf;

/// The rows' margin at the sides, at its widest.
pub const MARGIN: f32 = 28.0;
/// Under the last row.
pub const FOOT: f32 = 16.0;
/// Between panels across a row, and down a column.
pub const GAP: f32 = 12.0;
/// Between rows (m4: the tiles end at 192, the charts start at 208).
pub const ROW_GAP: f32 = 16.0;
/// A panel frame's corners.
pub const RADIUS: f32 = 8.0;
/// From a panel's frame to its title and figure.
pub const PAD: f32 = 16.0;
/// The dashboard's title, a panel's title and subtitle, and the chip's words.
pub const TITLE: f32 = 15.0;
/// A panel's title.
pub const PANEL_TITLE: f32 = 13.0;
/// A panel's subtitle, the chip's words and the live status.
pub const SMALL: f32 = 12.0;
/// The chip's height and corners.
pub const CHIP_H: f32 = 24.0;
/// The chip's corners.
pub const CHIP_RADIUS: f32 = 6.0;
/// The least height of a chart's or a graph's panel.
pub const MIN_FIGURE: f32 = 180.0;
/// The least width of a chart's or a graph's panel before its row wraps.
pub const MIN_WIDTH: f32 = 220.0;
/// A graph panel's height over its plot: its title and subtitle, its legend, its foot.
const GRAPH_CHROME: f32 = 104.0;
/// A tile panel's height when it has no model to say (waiting, failed).
pub const TILE_HEIGHT: f32 = 100.0;
/// The scrollbar's width.
pub const SCROLLBAR: f32 = 4.0;
/// A panel's title baseline and its subtitle's, from the frame's top (m4).
const TITLE_Y: f32 = 26.0;
const SUBTITLE_Y: f32 = 46.0;
/// Where a panel's figure starts under its title, and under a subtitle too.
const UNDER_TITLE: f32 = 34.0;
const UNDER_SUBTITLE: f32 = 54.0;
/// Under a chart's x labels, to the frame.
const CHART_FOOT: f32 = 10.0;
/// How much narrower than its frame a chart lays out, at the right.
const CHART_RIGHT: f32 = 8.0;
/// The least frame that shows a subtitle.
const SUBTITLE_MIN_H: f32 = 150.0;
/// A nested dashboard's title row, and its inner margin.
const GROUP_HEAD: f32 = 40.0;
const GROUP_PAD: f32 = 12.0;

/// A piece of the dashboard's own text, measured and placed.
#[derive(Clone, Debug, PartialEq)]
pub struct Words {
    /// What it says, already cut to fit.
    pub text: String,
    /// Its anchor's x.
    pub x: f32,
    /// Its baseline.
    pub y: f32,
    /// Its type.
    pub spec: TextSpec,
    /// Which end of it is at `x`.
    pub anchor: Anchor,
    /// Its colour.
    pub ink: Ink,
}

impl Words {
    /// The scene item, in `theme`'s colours.
    pub fn item(&self, theme: &Theme) -> Item {
        Item::Text {
            text: self.text.as_str().into(),
            x: self.x,
            y: self.y,
            spec: self.spec,
            anchor: self.anchor,
            color: self.ink.color(theme),
            alpha: 1.0,
            max_width: None,
        }
    }

    /// Its box, measured by `m`.
    pub fn bounds(&self, m: &mut dyn Measure) -> Rect {
        text_box(&self.text, self.x, self.y, self.spec, self.anchor, m)
    }
}

/// The time-range chip.
#[derive(Clone, Debug, PartialEq)]
pub struct Chip {
    /// Its box.
    pub rect: Rect,
    /// What it says: `Last 6 hours`, or a custom range's ends.
    pub text: Words,
    /// Its chevron, a polyline.
    pub chevron: [(f32, f32); 3],
}

/// The live status at the header's right.
#[derive(Clone, Debug, PartialEq)]
pub struct Live {
    /// Its state: the dot's colour, or the error's icon.
    pub state: LiveState,
    /// The dot's or the icon's centre.
    pub mark: (f32, f32),
    /// What it says, if there's room for words.
    pub text: Option<Words>,
    /// The box a click re-runs in.
    pub rect: Rect,
}

/// The header: title, chip and live status.
#[derive(Clone, Debug, PartialEq)]
pub struct Header {
    /// The title.
    pub title: Option<Words>,
    /// The time-range chip, when the dashboard has a `time`.
    pub chip: Option<Chip>,
    /// The live status, when a source is live.
    pub live: Option<Live>,
    /// Where the rows start.
    pub bottom: f32,
}

/// What a panel's figure is, laid out: what the pointer needs of it. Its pixels are in the
/// panel's scene. (A chart's layout is the big one; one is made a panel a base, and the editor
/// lends it to the panel's pane by moving it, so it isn't boxed.)
#[allow(clippy::large_enum_variant)]
pub enum PanelFigure {
    /// A chart: its layout and hit index, in the figure's own px.
    Chart {
        /// Where everything landed.
        layout: ChartLayout,
        /// Its continuous marks, for the pointer.
        hits: HitIndex,
    },
    /// A graph.
    Graph {
        /// Where everything landed.
        layout: GraphLayout,
    },
    /// A tile, over the dashboard's visible time where it has the time field.
    Tile {
        /// Its model as drawn.
        model: TileModel,
    },
    /// Its data is still coming: what it waits for.
    Waiting(String),
    /// Its error figure.
    Failed,
}

/// One panel, laid out.
pub struct PanelLayout {
    /// Its number.
    pub index: usize,
    /// Its frame, in the area's px (scrolled).
    pub frame: Rect,
    /// Its title's box, in the area's px.
    pub title: Option<Rect>,
    /// Where its figure is, in the panel's px (the frame's top left at 0, 0): the figure's own
    /// px start at its top left.
    pub body: Rect,
    /// The figure.
    pub figure: PanelFigure,
    /// The panel at rest in its own px: the figure, its title and subtitle, the stale badge.
    pub scene: Scene,
    /// The view it was laid out with: a linked chart's x is the dashboard's time.
    pub view: View,
}

impl PanelLayout {
    /// A point in the area's px in the figure's own px.
    pub fn local(&self, x: f32, y: f32) -> (f32, f32) {
        (
            x - self.frame.x - self.body.x,
            y - self.frame.y - self.body.y,
        )
    }

    /// The figure's own origin in the area's px.
    pub fn origin(&self) -> (f32, f32) {
        (self.frame.x + self.body.x, self.frame.y + self.body.y)
    }

    /// Its chart's layout and hit index, if it shows a chart.
    pub fn chart(&self) -> Option<(&ChartLayout, &HitIndex)> {
        match &self.figure {
            PanelFigure::Chart { layout, hits, .. } => Some((layout, hits)),
            _ => None,
        }
    }

    /// Its graph's layout, if it shows a graph.
    pub fn graph(&self) -> Option<&GraphLayout> {
        match &self.figure {
            PanelFigure::Graph { layout, .. } => Some(layout),
            _ => None,
        }
    }

    /// Its tile's model as drawn, if it shows a tile.
    pub fn tile(&self) -> Option<&TileModel> {
        match &self.figure {
            PanelFigure::Tile { model } => Some(model),
            _ => None,
        }
    }
}

/// A nested dashboard's frame.
#[derive(Clone, Debug, PartialEq)]
pub struct GroupLayout {
    /// Its frame, in the area's px.
    pub frame: Rect,
    /// Its title.
    pub title: Option<Words>,
    /// Its file.
    pub file: PathBuf,
}

/// Where everything landed.
pub struct DashLayout {
    /// The area's size.
    pub size: (f32, f32),
    /// Physical px to a logical px.
    pub scale: f32,
    /// The header.
    pub header: Header,
    /// Where the rows show: under the header. They're clipped to it.
    pub viewport: Rect,
    /// The rows' whole height, foot and all.
    pub content: f32,
    /// How far the rows are scrolled, within `0..=max_scroll`.
    pub scroll: f32,
    /// The most they scroll: 0 when they fit.
    pub max_scroll: f32,
    /// The scrollbar's track and thumb while the rows overflow.
    pub scrollbar: Option<(Rect, Rect)>,
    /// Each panel by number; None for one not laid out (another is full size).
    pub panels: Vec<Option<PanelLayout>>,
    /// The nested dashboards' frames.
    pub groups: Vec<GroupLayout>,
    /// The panel shown full size.
    pub full: Option<usize>,
    /// The panel with the focus.
    pub focused: Option<usize>,
    /// The time the linked panels show.
    pub time: Option<(f64, f64)>,
    /// Which panels show a card on a hover.
    pub tooltip: Tooltip,
    /// Each line of panels as placed: the panels on it, for the reading order on screen.
    pub lines: Vec<Vec<usize>>,
}

impl DashLayout {
    /// Panel `i`, if it's laid out.
    pub fn panel(&self, i: usize) -> Option<&PanelLayout> {
        self.panels.get(i).and_then(Option::as_ref)
    }

    /// The scroll that shows panel `i` whole (its top when it's taller than the viewport),
    /// from `scroll`, moving as little as it can.
    pub fn reveal(&self, i: usize) -> f32 {
        let Some(p) = self.panel(i) else {
            return self.scroll;
        };
        let (top, bottom) = (p.frame.y + self.scroll, p.frame.bottom() + self.scroll);
        let (vt, vh) = (self.viewport.y + 1.0, self.viewport.h - 1.0 - FOOT);
        let mut s = self.scroll;
        if bottom - s > vt + vh {
            s = bottom - vt - vh;
        }
        if top - s < vt {
            s = top - vt;
        }
        s.clamp(0.0, self.max_scroll)
    }

    /// The scroll that a drag of the thumb from `from` to `to` (area px, down) asks for.
    pub fn drag_scroll(&self, from: f32, to: f32, scroll_then: f32) -> f32 {
        let Some((track, thumb)) = self.scrollbar else {
            return scroll_then;
        };
        let free = (track.h - thumb.h).max(1.0);
        (scroll_then + (to - from) / free * self.max_scroll).clamp(0.0, self.max_scroll)
    }
}

/// The side margin of an area `w` wide: 28 px, less on a narrow one.
pub fn margin(w: f32) -> f32 {
    (w * 0.032).clamp(12.0, MARGIN).round()
}

/// Where a hairline's edge goes so it lands on whole physical pixels at `scale`: a pixel's
/// middle for an odd width, its edge for an even one (raster.rs's rule).
fn snap(v: f32, scale: f32) -> f32 {
    let w = scale.floor().max(1.0);
    if w as u32 % 2 == 1 {
        ((v * scale).floor() + 0.5) / scale
    } else {
        (v * scale).round() / scale
    }
}

/// A frame with its edges on whole physical pixels.
fn snap_rect(r: Rect, scale: f32) -> Rect {
    let (x, y) = (snap(r.x, scale), snap(r.y, scale));
    let (right, bottom) = (snap(r.right(), scale), snap(r.bottom(), scale));
    Rect::new(x, y, (right - x).max(0.0), (bottom - y).max(0.0))
}

/// The dashboard laid out at `size` (logical px, at `scale`) under `view`: see the module's
/// docs.
pub fn layout(
    model: &DashModel,
    view: &DashView,
    size: (f32, f32),
    scale: f32,
    theme: &Theme,
    m: &mut dyn Measure,
) -> DashLayout {
    let full = view.full.filter(|&i| i < model.panels.len());
    lay_out(model, view, full, size, scale, theme, m)
}

/// Panel `panel` laid out over the whole area under the header, the rest not at all.
pub fn layout_full(
    model: &DashModel,
    view: &DashView,
    panel: usize,
    size: (f32, f32),
    scale: f32,
    theme: &Theme,
    m: &mut dyn Measure,
) -> DashLayout {
    let full = Some(panel).filter(|&i| i < model.panels.len());
    lay_out(model, view, full, size, scale, theme, m)
}

#[allow(clippy::too_many_arguments)]
fn lay_out(
    model: &DashModel,
    view: &DashView,
    full: Option<usize>,
    size: (f32, f32),
    scale: f32,
    theme: &Theme,
    m: &mut dyn Measure,
) -> DashLayout {
    let (w, h) = size;
    let margin = margin(w);
    let header = header(model, view, w, margin, m);
    let top = header.bottom;
    let viewport = Rect::new(0.0, top - 1.0, w, (h - top + 1.0).max(0.0));
    let room = (h - top - FOOT).max(0.0);
    let mut cx = Cx {
        model,
        view,
        theme,
        scale,
        m,
        time: view.shared(model),
        panels: (0..model.panels.len()).map(|_| None).collect(),
        groups: Vec::new(),
        lines: Vec::new(),
        fits: vec![RowFit::default(); model.panels.len()],
    };
    let across = (w - 2.0 * margin).max(0.0);
    let (content, max_scroll, scroll);
    if let Some(i) = full {
        cx.panel(i, Rect::new(margin, top, across, room), (0.0, 0.0));
        cx.lines.push(vec![i]);
        (content, max_scroll, scroll) = (room + FOOT, 0.0, 0.0);
    } else {
        let lines = cx.lines_of(&model.rows, across);
        let heights = cx.distribute(&lines, room, ROW_GAP);
        let total: f32 =
            heights.iter().sum::<f32>() + ROW_GAP * lines.len().saturating_sub(1) as f32;
        max_scroll = (total - room).max(0.0).round();
        scroll = view.scroll.clamp(0.0, max_scroll).round();
        content = total + FOOT;
        let mut y = top - scroll;
        for (line, lh) in lines.iter().zip(&heights) {
            cx.place_line(line, Rect::new(margin, y, across, *lh), true);
            y += lh + ROW_GAP;
        }
    }
    let scrollbar = (max_scroll > 0.0).then(|| {
        let track = Rect::new(
            w - margin / 2.0 - SCROLLBAR / 2.0,
            top + 4.0,
            SCROLLBAR,
            (h - top - 8.0).max(0.0),
        );
        let th = (track.h * room / (room + max_scroll)).clamp(24.0_f32.min(track.h), track.h);
        let at = scroll / max_scroll;
        let thumb = Rect::new(track.x, track.y + (track.h - th) * at, track.w, th);
        (track, thumb)
    });
    DashLayout {
        size,
        scale,
        header,
        viewport,
        content,
        scroll,
        max_scroll,
        scrollbar,
        panels: cx.panels,
        groups: cx.groups,
        full,
        focused: view.focused.filter(|&f| f < model.panels.len()),
        time: cx.time,
        tooltip: model.tooltip,
        lines: cx.lines,
    }
}

/// The header at a width: see the module's docs.
fn header(model: &DashModel, view: &DashView, w: f32, margin: f32, m: &mut dyn Measure) -> Header {
    let title_spec = TextSpec::new(TITLE).weight(Weight::Semibold);
    let small = TextSpec::new(SMALL);
    let right = w - margin;
    let mut y = 24.0;
    let title_y = 38.0;
    let has_title = model.title.as_deref().is_some_and(|t| !t.is_empty());
    let chip_text = view.chip(model);
    let chip_row = if has_title { 52.0 } else { y };
    let chip = chip_text.map(|text| {
        let room = (w - 2.0 * margin - 34.0).max(0.0);
        let text = m.fit(&text, small, room);
        let cw = (m.width(&text, small) + 34.0).round();
        let rect = Rect::new(margin, chip_row, cw, CHIP_H);
        let cx = rect.right() - 17.0;
        let cy = rect.y + 10.0;
        Chip {
            text: Words {
                text,
                x: rect.x + 11.0,
                y: rect.y + 16.0,
                spec: small,
                anchor: Anchor::Start,
                ink: Ink::Text,
            },
            chevron: [(cx, cy), (cx + 4.0, cy + 4.0), (cx + 8.0, cy)],
            rect,
        }
    });
    // The live status: on the chip's row, else the title's; said shorter where it would meet
    // what's left of it.
    let live_row = match (&chip, has_title) {
        (Some(c), _) => Some(c.rect.y + 16.0),
        (None, true) => Some(title_y),
        (None, false) => None,
    };
    let mut live = None;
    let mut title_room = right - margin;
    if let Some(status) = &model.live {
        let base = live_row.unwrap_or(y + 16.0);
        let clear = match &chip {
            Some(c) => c.rect.right() + 16.0,
            None => margin + if has_title { 120.0 } else { 0.0 },
        };
        let word = status
            .text
            .split(" · ")
            .next()
            .unwrap_or(&status.text)
            .to_string();
        let tries = [Some(status.text.clone()), Some(word), None];
        let mut chosen = None;
        for t in tries {
            let tw = t.as_deref().map_or(0.0, |t| m.width(t, small));
            let gap = if t.is_some() { 10.0 } else { 0.0 };
            let left = right - tw - gap - 3.5;
            if left >= clear || t.is_none() {
                chosen = Some((t, tw, gap));
                break;
            }
        }
        let (t, tw, gap) = chosen.expect("the last try always fits");
        let mark = (right - tw - gap, base - 4.0);
        let text = t.map(|text| Words {
            text,
            x: right,
            y: base,
            spec: small,
            anchor: Anchor::End,
            ink: Ink::Muted,
        });
        let left = mark.0 - 6.0;
        if chip.is_none() && has_title {
            title_room = (left - 16.0 - margin).max(0.0);
        }
        live = Some(Live {
            state: status.state,
            mark,
            text,
            rect: Rect::new(left, base - 14.0, right - left + 4.0, 20.0),
        });
    }
    let title = model
        .title
        .as_deref()
        .filter(|t| !t.is_empty())
        .map(|t| Words {
            text: m.fit(t, title_spec, title_room),
            x: margin,
            y: title_y,
            spec: title_spec,
            anchor: Anchor::Start,
            ink: Ink::Strong,
        });
    // Where the rows start: 16 px under the chip's row, or 18 under the title's baseline.
    if let Some(c) = &chip {
        y = c.rect.bottom() + 16.0;
    } else if title.is_some() {
        y = title_y + 18.0;
    } else if live.is_some() {
        y += 24.0 + 16.0;
    } else {
        y = margin.max(16.0);
    }
    Header {
        title,
        chip,
        live,
        bottom: y,
    }
}

/// The layout's working state.
struct Cx<'a, 'm> {
    model: &'a DashModel,
    view: &'a DashView,
    theme: &'a Theme,
    scale: f32,
    m: &'m mut dyn Measure,
    time: Option<(f64, f64)>,
    panels: Vec<Option<PanelLayout>>,
    groups: Vec<GroupLayout>,
    lines: Vec<Vec<usize>>,
    /// What each tile's line agreed on, by panel: one value size, a meter's word or none.
    fits: Vec<RowFit>,
}

/// One line of cells across: a row, or part of one that wrapped.
struct Line<'c> {
    cells: Vec<&'c Cell>,
    weight: f32,
}

impl Cx<'_, '_> {
    /// Panel `i`'s least height and its natural one, if it has one (a tile's).
    fn panel_heights(&mut self, i: usize) -> (f32, Option<f32>) {
        let p = &self.model.panels[i];
        match (&p.state, p.kind) {
            (PanelState::Tile(t), _) => {
                let n = t.model.natural_height();
                (n, Some(n))
            }
            (_, PanelKind::Tile) => (TILE_HEIGHT, Some(TILE_HEIGHT)),
            // A tree's or layers' ranks need their room, or their nodes overlap: its title,
            // legend and foot over the plot it needs.
            (PanelState::Graph(g), _) => {
                let plot = graph::geom::least_plot_height(g);
                (MIN_FIGURE.max((GRAPH_CHROME + plot).ceil()), None)
            }
            _ => (MIN_FIGURE, None),
        }
    }

    /// A cell's least height.
    fn min_h(&mut self, c: &Cell) -> f32 {
        match &c.kind {
            CellKind::Panel(i) => self.panel_heights(*i).0,
            CellKind::Column(cs) => {
                let sum: f32 = cs.iter().map(|c| self.min_h(c)).sum();
                sum + GAP * cs.len().saturating_sub(1) as f32
            }
            CellKind::Group(g) => {
                let rows: f32 = g
                    .rows
                    .iter()
                    .map(|r| r.cells.iter().map(|c| self.min_h(c)).fold(0.0, f32::max))
                    .sum();
                GROUP_HEAD + GROUP_PAD + rows + ROW_GAP * g.rows.len().saturating_sub(1) as f32
            }
        }
    }

    /// A cell's natural height, if all of it is tiles.
    fn fixed_h(&mut self, c: &Cell) -> Option<f32> {
        match &c.kind {
            CellKind::Panel(i) => self.panel_heights(*i).1,
            CellKind::Column(cs) => {
                let mut sum = GAP * cs.len().saturating_sub(1) as f32;
                for c in cs {
                    sum += self.fixed_h(c)?;
                }
                Some(sum)
            }
            CellKind::Group(_) => None,
        }
    }

    /// A cell's least width before its row wraps.
    fn min_w(&mut self, c: &Cell) -> f32 {
        match &c.kind {
            CellKind::Panel(i) => match &self.model.panels[*i].state {
                PanelState::Tile(t) => t.model.min_size(self.m).0.max(120.0),
                _ if self.model.panels[*i].kind == PanelKind::Tile => 140.0,
                _ => MIN_WIDTH,
            },
            CellKind::Column(cs) => cs.iter().map(|c| self.min_w(c)).fold(0.0, f32::max),
            CellKind::Group(g) => {
                let widest = g
                    .rows
                    .iter()
                    .map(|r| {
                        let sum: f32 = r.cells.iter().map(|c| self.min_w(c)).sum();
                        sum + GAP * r.cells.len().saturating_sub(1) as f32
                    })
                    .fold(0.0, f32::max);
                (widest + 2.0 * GROUP_PAD).max(MIN_WIDTH)
            }
        }
    }

    /// Rows as lines across `width`: each row whole, or wrapped onto as few lines as give its
    /// panels their least widths, shared evenly.
    fn lines_of<'c>(&mut self, rows: &'c [super::Row], width: f32) -> Vec<Line<'c>> {
        let mut out = Vec::new();
        for r in rows {
            let mins: Vec<f32> = r.cells.iter().map(|c| self.min_w(c)).collect();
            let n = r.cells.len();
            let mut per = n;
            for k in 1..=n {
                per = n.div_ceil(k);
                let fits = r.cells.chunks(per).zip(mins.chunks(per)).all(|(cs, ms)| {
                    let spans: f32 = cs.iter().map(|c| c.span).sum();
                    let avail = width - GAP * cs.len().saturating_sub(1) as f32;
                    cs.iter()
                        .zip(ms)
                        .all(|(c, min)| avail * c.span / spans >= min - 0.5)
                });
                if fits {
                    break;
                }
            }
            for cs in r.cells.chunks(per.max(1)) {
                out.push(Line {
                    cells: cs.iter().collect(),
                    weight: r.height,
                });
            }
        }
        out
    }

    /// The lines' heights in `room`: tiles' lines their natural height, the others what's
    /// left by weight, each at least its least.
    fn distribute(&mut self, lines: &[Line<'_>], room: f32, gap: f32) -> Vec<f32> {
        let mut fixed: Vec<Option<f32>> = Vec::with_capacity(lines.len());
        let mut mins = Vec::with_capacity(lines.len());
        for l in lines {
            let mut f = Some(0.0_f32);
            let mut min = 0.0_f32;
            for c in &l.cells {
                f = match (f, self.fixed_h(c)) {
                    (Some(a), Some(b)) => Some(a.max(b)),
                    _ => None,
                };
                min = min.max(self.min_h(c));
            }
            fixed.push(f);
            mins.push(min);
        }
        let weights: Vec<f32> = lines.iter().map(|l| l.weight.max(0.01)).collect();
        share(&fixed, &mins, &weights, room, gap)
    }

    /// Places a line's cells across `rect` by their spans; a row's own line (`top`) is one
    /// whose charts line up.
    fn place_line(&mut self, line: &Line<'_>, rect: Rect, top: bool) {
        let n = line.cells.len();
        let spans: f32 = line.cells.iter().map(|c| c.span).sum::<f32>().max(0.01);
        let avail = (rect.w - GAP * n.saturating_sub(1) as f32).max(0.0);
        let mut acc = 0.0;
        let mut placed = Vec::new();
        for (k, c) in line.cells.iter().enumerate() {
            let x0 = (rect.x + avail * acc / spans + GAP * k as f32).round();
            acc += c.span;
            let x1 = (rect.x + avail * acc / spans + GAP * k as f32).round();
            let r = Rect::new(x0, rect.y, (x1 - x0).max(0.0), rect.h);
            if let CellKind::Panel(i) = c.kind {
                placed.push(i);
            }
            self.place(c, r);
        }
        if top || !placed.is_empty() {
            self.align(&placed);
            if !placed.is_empty() {
                self.lines.push(placed);
            }
        }
    }

    /// Places a cell in `rect`.
    fn place(&mut self, c: &Cell, rect: Rect) {
        match &c.kind {
            CellKind::Panel(i) => self.panel(*i, rect, (0.0, 0.0)),
            CellKind::Column(cs) => {
                let lines: Vec<Line<'_>> = cs
                    .iter()
                    .map(|c| Line {
                        cells: vec![c],
                        weight: 1.0,
                    })
                    .collect();
                let heights = self.distribute(&lines, rect.h, GAP);
                let mut y = rect.y;
                for (l, lh) in lines.iter().zip(heights) {
                    self.place_line(l, Rect::new(rect.x, y, rect.w, lh), false);
                    y += lh + GAP;
                }
            }
            CellKind::Group(g) => self.group(g, rect),
        }
    }

    /// A nested dashboard in `rect`: its frame, title and rows.
    fn group(&mut self, g: &Group, rect: Rect) {
        let frame = snap_rect(rect, self.scale);
        let spec = TextSpec::new(PANEL_TITLE).weight(Weight::Semibold);
        let title = g.title.as_deref().filter(|t| !t.is_empty()).map(|t| Words {
            text: self.m.fit(t, spec, (frame.w - 2.0 * PAD).max(0.0)),
            x: frame.x + PAD,
            y: frame.y + TITLE_Y,
            spec,
            anchor: Anchor::Start,
            ink: Ink::Strong,
        });
        let head = if title.is_some() {
            GROUP_HEAD
        } else {
            GROUP_PAD
        };
        self.groups.push(GroupLayout {
            frame,
            title,
            file: g.file.clone(),
        });
        let inner = Rect::new(
            frame.x + GROUP_PAD,
            frame.y + head,
            (frame.w - 2.0 * GROUP_PAD).max(0.0),
            (frame.h - head - GROUP_PAD).max(0.0),
        );
        let lines = self.lines_of(&g.rows, inner.w);
        let heights = self.distribute(&lines, inner.h, ROW_GAP);
        let mut y = inner.y;
        for (l, lh) in lines.iter().zip(heights) {
            self.place_line(l, Rect::new(inner.x, y, inner.w, lh), true);
            y += lh + ROW_GAP;
        }
    }

    /// The charts placed on one line share their plots' top and bottom; its tiles share one
    /// value size (the smallest any needs) and a meter's word (all or none).
    fn align(&mut self, placed: &[usize]) {
        self.align_tiles(placed);
        self.align_charts(placed);
    }

    /// A line's tiles drawn alike: see [`figure::draw::RowFit`].
    fn align_tiles(&mut self, placed: &[usize]) {
        let tiles: Vec<(usize, Rect, RowFit)> = placed
            .iter()
            .filter_map(|&i| {
                let p = self.panels[i].as_ref()?;
                let tm = p.tile()?;
                let whole = Rect::new(0.0, 0.0, p.frame.w, p.frame.h);
                Some((i, p.frame, figure::draw::needs(tm, whole, self.m)))
            })
            .collect();
        if tiles.len() < 2 {
            return;
        }
        let fit = (tiles.iter().map(|t| t.2)).fold(RowFit::default(), RowFit::meet);
        for (i, frame, need) in tiles {
            self.fits[i] = fit;
            if need != fit {
                self.panel(i, frame, (0.0, 0.0));
            }
        }
    }

    /// The charts placed on one line share their plots' top and bottom.
    fn align_charts(&mut self, placed: &[usize]) {
        let plots: Vec<(usize, f32, f32, Rect)> = placed
            .iter()
            .filter_map(|&i| {
                let p = self.panels[i].as_ref()?;
                let (l, _) = p.chart()?;
                Some((i, l.plot.y, p.frame.h - l.plot.bottom(), p.frame))
            })
            .collect();
        if plots.len() < 2 {
            return;
        }
        let top = plots.iter().map(|p| p.1).fold(f32::MIN, f32::max);
        let foot = plots.iter().map(|p| p.2).fold(f32::MIN, f32::max);
        for (i, t, f, frame) in plots {
            let extra = ((top - t).max(0.0), (foot - f).max(0.0));
            if extra.0 > 0.5 || extra.1 > 0.5 {
                self.panel(i, frame, extra);
            }
        }
    }

    /// Panel `i` in `frame`, a chart's plot moved in by `extra` (top, bottom) to line up.
    fn panel(&mut self, i: usize, frame: Rect, extra: (f32, f32)) {
        let frame = snap_rect(frame, self.scale);
        let (model, theme, scale) = (self.model, self.theme, self.scale);
        let p = &model.panels[i];
        let view = self.view.panel(model, i);
        let size = (frame.w, frame.h);
        let room = (frame.w - 2.0 * PAD).max(0.0);
        // The title: the figure's own, or a waiting or failed tile's label; a waiting panel
        // with none says what it waits for, quietly, in its place.
        let mut words: Vec<(Words, bool)> = Vec::new();
        let tile_ready = matches!(p.state, PanelState::Tile(_));
        let title = p.title.as_deref().filter(|_| !tile_ready);
        let mut head = PAD;
        // A tile that can't draw itself keeps its label as a tile has it: 12 px muted.
        let tile_label = p.kind == PanelKind::Tile;
        if let Some(t) = title {
            let spec = match tile_label {
                true => TextSpec::new(SMALL),
                false => TextSpec::new(PANEL_TITLE).weight(Weight::Semibold),
            };
            words.push((
                Words {
                    text: self.m.fit(t, spec, room),
                    x: PAD,
                    y: TITLE_Y,
                    spec,
                    anchor: Anchor::Start,
                    ink: if tile_label { Ink::Muted } else { Ink::Strong },
                },
                true,
            ));
            head = UNDER_TITLE;
            if let Some(s) = p.subtitle.as_deref().filter(|_| frame.h >= SUBTITLE_MIN_H) {
                let spec = TextSpec::new(SMALL);
                words.push((
                    Words {
                        text: self.m.fit(s, spec, room),
                        x: PAD,
                        y: SUBTITLE_Y,
                        spec,
                        anchor: Anchor::Start,
                        ink: Ink::Muted,
                    },
                    false,
                ));
                head = UNDER_SUBTITLE;
            }
        } else if let PanelState::Waiting(why) = &p.state
            && !why.is_empty()
        {
            let spec = TextSpec::new(SMALL);
            words.push((
                Words {
                    text: self.m.fit(why, spec, room),
                    x: PAD,
                    y: TITLE_Y,
                    spec,
                    anchor: Anchor::Start,
                    ink: Ink::Muted,
                },
                false,
            ));
        }
        let under = |head: f32| {
            let top = if head > PAD { head - 8.0 } else { 1.0 };
            Rect::new(
                1.0,
                top,
                (frame.w - 2.0).max(0.0),
                (frame.h - top - 1.0).max(0.0),
            )
        };
        let whole = Rect::new(0.0, 0.0, frame.w, frame.h);
        let error_view = (self.view.panels.get(i))
            .map(|v| v.error)
            .unwrap_or_default();
        // A panel kept with the stale badge shows its rebuild's error as a log over it while
        // `!` (or a click on the badge) has it up, as a figure alone does.
        let stale_log = p.stale.is_some() && matches!(error_view, ErrorView::Log { .. });
        let (body, inner, figure) = match &p.state {
            _ if stale_log => {
                let body = under(if title.is_some() { head } else { PAD });
                let (errors, name) = p.errors().unwrap_or_default();
                let scene = plot::error_figure(
                    &errors,
                    "",
                    &name,
                    error_view,
                    (body.w, body.h),
                    scale,
                    theme,
                    self.m,
                );
                (body, Some(scene), PanelFigure::Failed)
            }
            PanelState::Chart(cm) => {
                let insets = Insets {
                    x: PAD,
                    top: head + extra.0,
                    bottom: CHART_FOOT + extra.1,
                    ends: false,
                };
                // Laid out a little narrower than the frame: an end tick label may reach
                // into the chart's padding, and keeps 12 px from the ring that way (m4's plot
                // ends 24 px in).
                let size = ((size.0 - CHART_RIGHT).max(0.0), size.1);
                let (scene, layout, hits) =
                    plot::base_inset(cm, &view, size, insets, scale, theme, self.m);
                (whole, Some(scene), PanelFigure::Chart { layout, hits })
            }
            PanelState::Graph(g) => {
                // Placed so the graph's own paddings put its legend 16 px in, where a chart's
                // is, and its nodes as far from the ring as a chart's plot.
                let (px, pt, pb) = plot::layout::paddings(frame.w, (frame.h - head).max(0.0));
                let (x, top) = ((PAD - px).max(1.0), (head - pt).max(1.0));
                let foot = (PAD - pb).max(4.0);
                let body = Rect::new(
                    x,
                    top,
                    (frame.w - 2.0 * x).max(0.0),
                    (frame.h - top - foot).max(0.0),
                );
                let (scene, layout) = graph::base(g, &view, (body.w, body.h), scale, theme, self.m);
                (body, Some(scene), PanelFigure::Graph { layout })
            }
            PanelState::Tile(t) => {
                // Over the visible time along the dashboard's time field, where the tile's
                // table has it; all of its rows when the time is all of the data.
                let field = model.time.as_ref().map(|ts| ts.field.as_str());
                let has_time = field.is_some_and(|f| t.table.column(f).is_some());
                let mut tm = match self.view.time.filter(|_| has_time) {
                    Some(r) => {
                        TileModel::build_on(&t.figure, &t.table, Some(r), field, &model.zone)
                            .unwrap_or_else(|_| t.model.clone())
                    }
                    None => t.model.clone(),
                };
                tm.fit = self.fits[i];
                let scene = figure::base(&tm, whole, theme, self.m);
                (whole, Some(scene), PanelFigure::Tile { model: tm })
            }
            PanelState::Waiting(why) => (whole, None, PanelFigure::Waiting(why.clone())),
            PanelState::Failed(f) => {
                let body = under(if title.is_some() { head } else { PAD });
                let state = (self.view.panels.get(i))
                    .map(|v| v.error)
                    .unwrap_or_default();
                let scene = plot::error_figure(
                    &f.errors,
                    "",
                    &f.name,
                    state,
                    (body.w, body.h),
                    scale,
                    theme,
                    self.m,
                );
                (body, Some(scene), PanelFigure::Failed)
            }
        };
        // The panel's scene, in its own px: the figure moved to its body, then the words over
        // it, then the stale badge, which keeps clear of both.
        let mut scene = Scene::new(frame.w, frame.h, scale);
        if let Some(inner) = inner {
            super::draw::put(&mut scene, inner, (body.x, body.y), None);
        }
        let mut title_box = None;
        for (w, is_title) in &words {
            let b = w.bounds(self.m);
            scene.push(w.item(theme));
            if *is_title {
                scene.region(b.inflate(2.0), Hit::PanelTitle(i as u16));
                title_box = Some(Rect::new(b.x + frame.x, b.y + frame.y, b.w, b.h));
            }
        }
        if let Some(e) = p.stale.as_ref().filter(|_| !stale_log) {
            plot::stale_badge(&mut scene, e, theme, self.m);
        }
        self.panels[i] = Some(PanelLayout {
            index: i,
            frame,
            title: title_box,
            body,
            figure,
            scene,
            view,
        });
    }
}

/// Heights for lines in `room`, `gap` apart: a fixed line its height, the others `room`'s rest
/// by weight, each at least its least; where the leasts don't fit, each its least.
pub fn share(
    fixed: &[Option<f32>],
    mins: &[f32],
    weights: &[f32],
    room: f32,
    gap: f32,
) -> Vec<f32> {
    let n = fixed.len();
    let mut out: Vec<f32> = fixed.iter().map(|f| f.unwrap_or(0.0)).collect();
    let mut free = room - out.iter().sum::<f32>() - gap * n.saturating_sub(1) as f32;
    let mut open: Vec<usize> = (0..n).filter(|&i| fixed[i].is_none()).collect();
    loop {
        let total: f32 = open.iter().map(|&i| weights[i]).sum();
        if open.is_empty() || total <= 0.0 {
            break;
        }
        let short: Vec<usize> = open
            .iter()
            .copied()
            .filter(|&i| free * weights[i] / total < mins[i])
            .collect();
        if short.is_empty() {
            for &i in &open {
                out[i] = (free * weights[i] / total).floor();
            }
            // What flooring left goes to the last, so the rows meet the foot exactly.
            if let Some(&last) = open.last() {
                let used: f32 = open.iter().map(|&i| out[i]).sum();
                out[last] += (free - used).max(0.0);
            }
            break;
        }
        for i in short {
            out[i] = mins[i];
            free -= mins[i];
            open.retain(|&j| j != i);
        }
    }
    out
}
