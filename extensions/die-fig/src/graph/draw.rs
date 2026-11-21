//! The graph at rest: the header, the edges, the nodes, the pins and the labels, with their
//! regions; and the pieces the overlay draws again on top.

use super::GraphModel;
use super::geom::{EdgeGeom, GraphLayout, NodeGeom, RING, lay_out};
use super::label::{NodeLabel, place};
use crate::interact::View;
use crate::interact::consts::{HIT_NODE_PAD, HIT_POINT};
use crate::plot::layout::legend_key;
use crate::scene::{Cap, Hit, Item, Join, Measure, Paint, PathData, Rect, Scene, Stroke};
use crate::theme::{Color, Theme};

/// The pin's dot: its radius, and how far round the ring it sits (from the right, upwards).
const PIN: f32 = 2.6;
const PIN_ANGLE: f32 = -std::f32::consts::FRAC_PI_4;
/// The dash of a dashed edge (ruling 15).
const EDGE_DASH: (f32, f32) = (6.0, 4.0);

/// The base layer: the header, then the graph clipped to its region, at rest (nothing
/// emphasised), with regions: the area as chrome, the region as the plot, a node's disc and
/// its reach as the node, the legend's keys and the title.
pub fn base(
    model: &GraphModel,
    view: &View,
    size: (f32, f32),
    scale: f32,
    theme: &Theme,
    m: &mut dyn Measure,
) -> (Scene, GraphLayout) {
    let mut layout = lay_out(model, view, size, scale, theme, m);
    layout.labels = place(model, view, &layout, m);
    layout.ink = layout.inked();
    let mut scene = Scene::new(size.0, size.1, scale);
    scene.region(Rect::new(0.0, 0.0, size.0, size.1), Hit::Chrome);
    scene.region(layout.region, Hit::Plot(0));
    for item in &layout.items {
        let g = model
            .groups
            .iter()
            .position(|x| *x == *item.key.name)
            .unwrap_or(0);
        legend_key(&mut scene, item, model.group_color(g, theme), theme);
        scene.push(item.name.item(theme));
    }
    for l in &layout.chrome {
        scene.push(l.item(theme));
    }
    if !layout.contained {
        scene.push(Item::PushClip(layout.region));
    }
    for e in layout.edges.iter().flatten() {
        edge(&mut scene, e, e.color, 1.0, 0.0);
    }
    for g in layout.nodes.iter().flatten() {
        node(&mut scene, g, g.color, theme);
    }
    for g in layout.nodes.iter().flatten().filter(|g| g.pinned) {
        pin(&mut scene, g.at, g.r + RING * 0.5, theme);
    }
    for l in layout.labels.iter().flatten() {
        label(&mut scene, l, theme.text, 1.0, theme);
    }
    if !layout.contained {
        scene.push(Item::PopClip);
    }
    for (i, g) in layout.nodes.iter().enumerate() {
        if let Some(g) = g {
            let reach = (g.r + HIT_NODE_PAD).max(HIT_POINT);
            let r = Rect::new(g.at.0 - reach, g.at.1 - reach, 2.0 * reach, 2.0 * reach);
            if let Some(r) = r.intersect(&layout.region) {
                scene.region(r, Hit::Node(model.id(i).into()));
            }
        }
    }
    for (k, r) in &layout.legend {
        scene.region(*r, Hit::LegendKey(k.clone()));
    }
    if !layout.title.is_empty() {
        scene.region(layout.title, Hit::PanelTitle(0));
    }
    (scene, layout)
}

/// An edge in a colour at an opacity, `wider` px wider than its own width (a hovered edge).
pub(crate) fn edge(scene: &mut Scene, e: &EdgeGeom, color: Color, alpha: f32, wider: f32) {
    let width = e.width + wider;
    let stroke = Stroke {
        width,
        paint: Paint::solid(color, alpha),
        cap: Cap::Round,
        join: Join::Round,
        dash: e.dash.then_some(EDGE_DASH),
        hairline: false,
    };
    scene.push(Item::Path {
        path: PathData::polyline(&e.pts),
        fill: None,
        stroke: Some(stroke),
    });
    if let Some([tip, l, r]) = e.arrow {
        let grow = wider * 0.8;
        let (l, r) = if grow > 0.0 {
            let back = ((l.0 + r.0) / 2.0, (l.1 + r.1) / 2.0);
            let (dx, dy) = (back.0 - tip.0, back.1 - tip.1);
            let d = dx.hypot(dy).max(1e-3);
            let s = (d + grow) / d;
            let at = |p: (f32, f32)| (tip.0 + (p.0 - tip.0) * s, tip.1 + (p.1 - tip.1) * s);
            (at(l), at(r))
        } else {
            (l, r)
        };
        let mut path = PathData::polyline(&[tip, l, r]);
        path.verbs.push(crate::scene::Verb::Close);
        scene.push(Item::Path {
            path,
            fill: Some(Paint::solid(color, alpha)),
            stroke: None,
        });
    }
}

/// A node: its disc in `color` with a ring in the surface's colour.
pub(crate) fn node(scene: &mut Scene, g: &NodeGeom, color: Color, theme: &Theme) {
    scene.push(Item::Path {
        path: PathData::circle(g.at.0, g.at.1, g.r),
        fill: Some(Paint::solid(color, 1.0)),
        stroke: Some(Stroke::solid(RING, theme.surface, 1.0)),
    });
}

/// A pinned node's mark: a small dot in the strong text's colour, ringed in the surface's, on
/// a circle of radius `at_r` about the node, at its upper right.
pub(crate) fn pin(scene: &mut Scene, c: (f32, f32), at_r: f32, theme: &Theme) {
    let p = (c.0 + at_r * PIN_ANGLE.cos(), c.1 + at_r * PIN_ANGLE.sin());
    scene.push(Item::Path {
        path: PathData::circle(p.0, p.1, PIN),
        fill: Some(Paint::solid(theme.strong, 1.0)),
        stroke: Some(Stroke::solid(1.5, theme.surface, 1.0)),
    });
}

/// A label in a colour, on a ground of the surface's colour where it crosses an edge.
pub(crate) fn label(scene: &mut Scene, l: &NodeLabel, color: Color, alpha: f32, theme: &Theme) {
    if l.backed {
        scene.push(Item::Path {
            path: PathData::rounded(l.rect.inflate(2.0), [3.0; 4]),
            fill: Some(Paint::solid(theme.surface, alpha.clamp(0.0, 1.0))),
            stroke: None,
        });
    }
    scene.push(Item::Text {
        text: l.text.as_str().into(),
        x: l.x,
        y: l.y,
        spec: l.spec,
        anchor: l.anchor,
        color,
        alpha,
        max_width: None,
    });
}
