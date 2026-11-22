//! A dashboard's pixels: [`base`] at rest (the header, the frames, every panel), and
//! [`overlay`], what follows the pointer and the cursors: the hovered panel's own overlay, each
//! panel's keyboard cursor, and the moment in every other linked panel (contract-p4 §3.3).
//!
//! **The linked moment.** Hovering a linked panel at a time draws, in each other linked chart,
//! a dashed hairline at that time in the foreground at 40 % and each series' dot at its own
//! nearest sample (none farther than half its sampling interval) with its value beside it, as
//! m4 has it; a linked stat shows its trend's mini crosshair there. Only the hovered panel shows
//! a card, unless the spec says `"tooltip": "shared"`: then each linked chart shows its card at
//! the moment too. With nothing hovered, the focused panel's keyboard cursor drives the same
//! line. A brush on a linked chart shows in every linked chart.

use super::hit::{DashHover, PanelHover};
use super::layout::{DashLayout, PanelFigure, PanelLayout, RADIUS};
use super::{DashModel, DashView, LiveState};
use crate::figure;
use crate::graph;
use crate::interact::{BrushKind, Cursor};
use crate::plot::errors::{readable, status_icon};
use crate::plot::hover::{sample_near, walked};
use crate::plot::marks::dot;
use crate::plot::overlay::Lit;
use crate::plot::{self, ChartModel, OverlayState};
use crate::scene::{
    Anchor, Cap, Hit, Item, Join, Measure, Paint, PathData, Rect, Scene, Stroke, TextSpec, Verb,
    Weight,
};
use crate::spec::{MarkKind, Tooltip};
use crate::theme::{Color, Theme};

/// The focused panel's ring: the border grey, never the accent (`#5a5a5a` on dark).
pub fn focus_ring(theme: &Theme) -> Color {
    theme.other
}

/// The linked moment's line: the foreground at 40 %.
const MOMENT_ALPHA: f32 = 0.4;
/// Its dash.
const MOMENT_DASH: (f32, f32) = (3.0, 3.0);
/// A linked dot's radius, and its surface ring.
const DOT: f32 = 4.0;
const DOT_RING: f32 = 2.0;

/// Draws `other` into `scene` moved by `by`, its regions cut to `clip` where given and a
/// figure's panel numbers made `panel`'s.
pub fn put(scene: &mut Scene, other: Scene, by: (f32, f32), clip: Option<(Rect, u16)>) {
    let (dx, dy) = by;
    let mv = |x: f32, y: f32| (x + dx, y + dy);
    let rect = |r: Rect| Rect::new(r.x + dx, r.y + dy, r.w, r.h);
    let paint = |p: Paint| match p {
        Paint::Linear { from, to, stops } => Paint::Linear {
            from: mv(from.0, from.1),
            to: mv(to.0, to.1),
            stops,
        },
        solid => solid,
    };
    for item in other.items {
        scene.push(match item {
            Item::Path { path, fill, stroke } => Item::Path {
                path: crate::scene::PathData {
                    verbs: (path.verbs.into_iter())
                        .map(|v| match v {
                            Verb::Move(x, y) => Verb::Move(x + dx, y + dy),
                            Verb::Line(x, y) => Verb::Line(x + dx, y + dy),
                            Verb::Quad(a, b, x, y) => Verb::Quad(a + dx, b + dy, x + dx, y + dy),
                            Verb::Cubic(a, b, c, d, x, y) => {
                                Verb::Cubic(a + dx, b + dy, c + dx, d + dy, x + dx, y + dy)
                            }
                            Verb::Close => Verb::Close,
                        })
                        .collect(),
                },
                fill: fill.map(paint),
                stroke: stroke.map(|s| Stroke {
                    paint: paint(s.paint),
                    ..s
                }),
            },
            Item::Text {
                text,
                x,
                y,
                spec,
                anchor,
                color,
                alpha,
                max_width,
            } => Item::Text {
                text,
                x: x + dx,
                y: y + dy,
                spec,
                anchor,
                color,
                alpha,
                max_width,
            },
            Item::Shadow {
                rect: r,
                radius,
                blur,
                dy: down,
                alpha,
            } => Item::Shadow {
                rect: rect(r),
                radius,
                blur,
                dy: down,
                alpha,
            },
            Item::PushClip(r) => Item::PushClip(rect(r)),
            other => other,
        });
    }
    for region in other.regions {
        let mut r = rect(region.rect);
        let mut hit = region.hit;
        if let Some((c, panel)) = clip {
            match c.intersect(&r) {
                Some(i) if i.w > 0.0 && i.h > 0.0 => r = i,
                _ => continue,
            }
            hit = match hit {
                Hit::AxisX(_) => Hit::AxisX(panel),
                Hit::AxisY(_) => Hit::AxisY(panel),
                Hit::PanelTitle(_) => Hit::PanelTitle(panel),
                Hit::Plot(_) => Hit::Plot(panel),
                Hit::Bar { band, series, .. } => Hit::Bar {
                    panel,
                    band,
                    series,
                },
                Hit::Cell { row, col, .. } => Hit::Cell { panel, row, col },
                other => other,
            };
        }
        scene.region(r, hit);
    }
}

/// The base: the header, the nested dashboards' frames, each panel's frame and scene (its
/// regions in place, a figure's panel numbers the panel's), all clipped under the header, and
/// the scrollbar.
pub fn base(layout: &DashLayout, view: &DashView, theme: &Theme, m: &mut dyn Measure) -> Scene {
    let _ = m;
    let (w, h) = layout.size;
    let mut scene = Scene::new(w, h, layout.scale);
    scene.region(Rect::new(0.0, 0.0, w, h), Hit::Chrome);
    header(&mut scene, layout, theme);
    let vp = layout.viewport;
    scene.push(Item::PushClip(vp));
    for g in &layout.groups {
        scene.push(frame_item(g.frame, RADIUS + 2.0, theme.grid));
        if let Some(t) = &g.title {
            scene.push(t.item(theme));
        }
    }
    let focused = view.focused.or(layout.focused);
    for p in layout.panels.iter().flatten() {
        if p.frame.inflate(2.0).intersect(&vp).is_none() {
            continue;
        }
        let ring = if focused == Some(p.index) && layout.full.is_none() {
            focus_ring(theme)
        } else {
            theme.grid
        };
        scene.push(frame_item(p.frame, RADIUS, ring));
        put(
            &mut scene,
            p.scene.clone(),
            (p.frame.x, p.frame.y),
            Some((vp, p.index as u16)),
        );
    }
    scene.push(Item::PopClip);
    // Rows scrolled under the header: a hairline says where they go.
    if layout.scroll > 0.0 {
        scene.push(Item::Path {
            path: PathData::polyline(&[(0.0, vp.y), (w, vp.y)]),
            fill: None,
            stroke: Some(Stroke::hairline(theme.grid, 1.0)),
        });
    }
    if let Some((_, thumb)) = layout.scrollbar {
        scene.push(Item::Path {
            path: PathData::rounded(thumb, [thumb.w / 2.0; 4]),
            fill: Some(Paint::solid(theme.muted, 0.55)),
            stroke: None,
        });
    }
    scene
}

/// A frame: a rounded rectangle's hairline.
fn frame_item(r: Rect, radius: f32, color: Color) -> Item {
    Item::Path {
        path: PathData::rounded(r, [radius; 4]),
        fill: None,
        stroke: Some(Stroke::hairline(color, 1.0)),
    }
}

/// The header: the title, the chip, the live status.
fn header(scene: &mut Scene, layout: &DashLayout, theme: &Theme) {
    let hd = &layout.header;
    if let Some(t) = &hd.title {
        scene.push(t.item(theme));
    }
    if let Some(c) = &hd.chip {
        scene.push(Item::Path {
            path: PathData::rounded(c.rect, [super::layout::CHIP_RADIUS; 4]),
            fill: Some(Paint::solid(theme.tooltip, 1.0)),
            stroke: Some(Stroke::hairline(theme.ring, 1.0)),
        });
        scene.push(c.text.item(theme));
        scene.push(Item::Path {
            path: PathData::polyline(&c.chevron),
            fill: None,
            stroke: Some(Stroke {
                cap: Cap::Round,
                join: Join::Round,
                ..Stroke::solid(1.5, theme.muted, 1.0)
            }),
        });
    }
    if let Some(l) = &hd.live {
        match l.state {
            LiveState::Error => status_icon(scene, l.mark, 5.0, true, theme),
            state => {
                let fill = match state {
                    LiveState::Paused => theme.muted,
                    _ => theme.status[0],
                };
                scene.push(Item::Path {
                    path: PathData::circle(l.mark.0, l.mark.1, 3.5),
                    fill: Some(Paint::solid(fill, 1.0)),
                    stroke: None,
                });
            }
        }
        if let Some(t) = &l.text {
            let mut item = t.item(theme);
            if l.state == LiveState::Error
                && let Item::Text { color, .. } = &mut item
            {
                *color = readable(theme.status[3], theme);
            }
            scene.push(item);
        }
    }
}

/// The overlay: see the module's docs. `hover` is what the pointer is on, if it's over a
/// panel.
pub fn overlay(
    model: &DashModel,
    layout: &DashLayout,
    view: &DashView,
    hover: Option<&DashHover>,
    theme: &Theme,
    m: &mut dyn Measure,
) -> Scene {
    let (w, h) = layout.size;
    let mut scene = Scene::new(w, h, layout.scale);
    let vp = layout.viewport;
    scene.push(Item::PushClip(vp));
    let focused = view.focused.or(layout.focused);
    let hovered = hover.map(|h| h.panel);
    // The moment the linked panels show: the hover's, else the focused panel's cursor's.
    let moment: Option<(usize, f64)> = match hover {
        Some(h) => h.time.map(|t| (h.panel, t)),
        None => focused.and_then(|f| {
            let p = model.panels.get(f).filter(|p| p.linked)?;
            let c = view.panels.get(f)?.cursor.as_ref()?;
            let (x, _) = plot::nav::cursor_data(p.chart()?, c)?;
            Some((f, x))
        }),
    };
    // A brush on a linked chart, as times.
    let brush_times = hover.and_then(|h| {
        let (a, b) = h.brush?;
        let p = layout.panel(h.panel)?;
        let (l, _) = p.chart()?;
        model.panels.get(h.panel).filter(|p| p.linked)?;
        let (ax, _) = p.local(a.0, a.1);
        let (bx, _) = p.local(b.0, b.1);
        let cl = |x: f32| x.clamp(l.plot.x, l.plot.right());
        Some((l.x.invert(cl(ax)), l.x.invert(cl(bx))))
    });
    for p in layout.panels.iter().flatten() {
        if p.frame.inflate(2.0).intersect(&vp).is_none() {
            continue;
        }
        let i = p.index;
        let pm = &model.panels[i];
        let mine = hover.filter(|h| h.panel == i);
        let cursor = view.panels.get(i).and_then(|v| v.cursor.clone());
        let card = focused == Some(i);
        let linked_other = pm.linked && moment.is_some_and(|(src, _)| src != i);
        match (&pm.state, &p.figure) {
            (super::PanelState::Chart(cm), PanelFigure::Chart { layout: cl, .. }) => {
                let local = mine.map(|h| p.local(h.at.0, h.at.1));
                let (mut hov, mut emphasis) = match mine.map(|h| &h.target) {
                    Some(PanelHover::Chart { cursor, emphasis }) => {
                        (cursor.clone(), emphasis.clone())
                    }
                    _ => (None, None),
                };
                let mut brush = mine.and_then(|h| h.brush).map(|(a, b)| {
                    let kind = plot::brush_kind(cm);
                    (kind, p.local(a.0, a.1), p.local(b.0, b.1))
                });
                if pm.linked
                    && brush.is_none()
                    && let Some((t0, t1)) = brush_times
                {
                    let (x0, x1) = (cl.x.map(t0), cl.x.map(t1));
                    brush = Some((BrushKind::X, (x0, cl.plot.y), (x1, cl.plot.bottom())));
                }
                let shared = linked_other && layout.tooltip == Tooltip::Shared;
                if shared && let Some((_, t)) = moment {
                    hov = at_time(cm, &p.view, t);
                    emphasis = None;
                }
                let st = OverlayState {
                    hover: hov,
                    cursor: cursor.clone(),
                    pointer: local,
                    brush,
                    emphasis,
                };
                let lit = Lit {
                    cursor_card: card,
                    ..Lit::default()
                };
                let quiet = st.hover.is_none()
                    && st.cursor.is_none()
                    && st.brush.is_none()
                    && st.emphasis.is_none();
                if !quiet {
                    let over = plot::overlay::overlay_lit(cm, cl, &p.view, &st, &lit, theme, m);
                    put(&mut scene, over, p.origin(), None);
                }
                if linked_other
                    && !shared
                    && brush_times.is_none()
                    && let Some((_, t)) = moment
                {
                    moment_strip(&mut scene, cm, p, t, theme, m);
                }
            }
            (super::PanelState::Graph(g), PanelFigure::Graph { layout: gl }) => {
                let target = match mine.map(|h| &h.target) {
                    Some(PanelHover::Graph(t)) => *t,
                    _ => None,
                };
                if target.is_some() || cursor.is_some() {
                    let st = graph::OverlayState {
                        hover: target,
                        cursor,
                        pointer: mine.map(|h| p.local(h.at.0, h.at.1)),
                        cursor_card: card,
                        ..Default::default()
                    };
                    let over = graph::overlay(g, gl, &p.view, &st, theme, m);
                    put(&mut scene, over, p.origin(), None);
                }
            }
            (_, PanelFigure::Tile { model: tm }) => {
                let local_rect = Rect::new(0.0, 0.0, p.frame.w, p.frame.h);
                let th = match mine.map(|h| &h.target) {
                    Some(PanelHover::Tile(t)) => *t,
                    _ if linked_other => moment.and_then(|(_, t)| figure::hover_at(tm, t)),
                    _ => None,
                };
                if th.is_some() {
                    let over = figure::overlay(tm, th, local_rect, theme, m);
                    put(&mut scene, over, p.origin(), None);
                }
            }
            _ => {}
        }
        // The hovered panel's title brightens.
        if hovered == Some(i) {
            brighten_title(&mut scene, p, theme);
        }
    }
    scene.push(Item::PopClip);
    scene
}

/// The hovered panel's title, brighter: the ground under it laid again, then the words in
/// white on dark (black on light).
fn brighten_title(scene: &mut Scene, p: &PanelLayout, theme: &Theme) {
    let Some(b) = p.title else {
        return;
    };
    let Some(Item::Text {
        text,
        x,
        y,
        spec,
        anchor,
        ..
    }) = p.scene.items.iter().rev().find(|it| {
        matches!(it, Item::Text { x, y, spec, .. }
            if spec.weight == Weight::Semibold
                && (*x + p.frame.x - b.x).abs() < 0.5
                && *y + p.frame.y >= b.y && *y + p.frame.y <= b.bottom() + 1.0)
    })
    else {
        return;
    };
    scene.push(Item::Path {
        path: PathData::rect(b.inflate(1.0)),
        fill: Some(Paint::solid(theme.surface, 1.0)),
        stroke: None,
    });
    let bright = if theme.dark {
        Color(255, 255, 255)
    } else {
        Color(0, 0, 0)
    };
    scene.push(Item::Text {
        text: text.clone(),
        x: x + p.frame.x,
        y: y + p.frame.y,
        spec: *spec,
        anchor: *anchor,
        color: bright,
        alpha: 1.0,
        max_width: None,
    });
}

/// The cursor a shared card shows at time `t`: the first walked series' sample nearest it.
fn at_time(cm: &ChartModel, view: &crate::interact::View, t: f64) -> Option<Cursor> {
    walked(cm, view).into_iter().find_map(|si| {
        let i = sample_near(cm, None, si, t)?;
        Some(Cursor::Point {
            series: cm.series[si].key.clone(),
            row: i as u32,
        })
    })
}

/// The linked moment in a chart that isn't hovered: the dashed line at `t`, each series' dot
/// at its own nearest sample within half its interval, and its value beside it (m4), the
/// values kept from overlapping one another.
fn moment_strip(
    scene: &mut Scene,
    cm: &ChartModel,
    p: &PanelLayout,
    t: f64,
    theme: &Theme,
    m: &mut dyn Measure,
) {
    let Some((l, _)) = p.chart() else {
        return;
    };
    let (ox, oy) = p.origin();
    let x = l.ix(t);
    if !x.is_finite() || x < l.plot.x - 0.5 || x > l.plot.right() + 0.5 {
        return;
    }
    let (x, top, bottom) = (x + ox, l.plot.y + oy - 4.0, l.plot.bottom() + oy);
    scene.push(Item::Path {
        path: PathData::polyline(&[(x, top), (x, bottom)]),
        fill: None,
        stroke: Some(Stroke {
            dash: Some(MOMENT_DASH),
            ..Stroke::hairline(theme.text, MOMENT_ALPHA)
        }),
    });
    let stacks = &l.stacks;
    let mut dots: Vec<((f32, f32), Color, String)> = Vec::new();
    for si in walked(cm, &p.view) {
        let s = &cm.series[si];
        if s.mark == MarkKind::Bar {
            continue;
        }
        let Some(i) = sample_near(cm, None, si, t) else {
            continue;
        };
        let half = if s.interval > 0.0 {
            s.interval / 2.0
        } else {
            f64::INFINITY
        };
        if (s.xs[i] - t).abs() > half * (1.0 + 1e-9) || s.ys[i].is_nan() {
            continue;
        }
        let v = cm.top(stacks, si, i);
        let (px, py) = l.at(s.xs[i], v);
        if !py.is_finite() || py < l.plot.y - 1.0 || py > l.plot.bottom() + 1.0 {
            continue;
        }
        let value = cm.value_label(s.ys[i], s.decimals);
        dots.push(((px + ox, py + oy), cm.color(s, theme), value));
    }
    scene.push(Item::PushClip(
        Rect::new(l.clip.x + ox, l.clip.y + oy, l.clip.w, l.clip.h).inflate(DOT + DOT_RING + 1.0),
    ));
    for (pt, color, _) in &dots {
        dot(scene, *pt, DOT, *color, theme.surface, DOT_RING);
    }
    scene.push(Item::PopClip);
    // Each value in a small tag beside its dot (m4's "17"), right of the line where it fits,
    // else left; one that would cover another's is left out.
    let spec = TextSpec::new(11.0).weight(Weight::Semibold).tabular();
    let mut taken: Vec<Rect> = Vec::new();
    let right_edge = l.plot.right() + ox;
    let left_edge = l.plot.x + ox;
    let mut order: Vec<usize> = (0..dots.len()).collect();
    order.sort_by(|&a, &b| dots[a].0.1.total_cmp(&dots[b].0.1));
    for k in order {
        let (pt, _, value) = &dots[k];
        let tw = m.width(value, spec) + 12.0;
        let th = 17.0;
        let mut r = Rect::new(pt.0 + 9.0, pt.1 - 19.0, tw, th);
        if r.right() > right_edge + 8.0 {
            r.x = pt.0 - 9.0 - tw;
        }
        if r.x < left_edge - 8.0 {
            continue;
        }
        r.y = r.y.max(l.plot.y + oy - 4.0);
        let near = r.inflate(2.0);
        if taken.iter().any(|t| t.intersect(&near).is_some())
            || dots
                .iter()
                .any(|(q, _, _)| near.contains(q.0, q.1) && (q.0 - pt.0).abs() > 0.1)
        {
            continue;
        }
        scene.push(Item::Path {
            path: PathData::rounded(r, [4.0; 4]),
            fill: Some(Paint::solid(theme.tooltip, 1.0)),
            stroke: Some(Stroke::hairline(theme.ring, 1.0)),
        });
        scene.push(Item::Text {
            text: value.as_str().into(),
            x: r.x + 6.0,
            y: r.y + 12.5,
            spec,
            anchor: Anchor::Start,
            color: theme.strong,
            alpha: 1.0,
            max_width: None,
        });
        taken.push(r);
    }
}
