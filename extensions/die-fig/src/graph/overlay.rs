//! The graph's overlay: what follows the pointer and the cursor, drawn over the base so a
//! hover repaints nothing else.
//!
//! - **Emphasis.** With a node in focus (the hover after its dwell, else the cursor's, which
//!   sticks), a group's legend key under the pointer, the nodes a search matches as it's typed,
//!   or a path from the reference: a wash of
//!   the surface over what the graph covers leaves everything at about 25 %, then what's
//!   emphasised is drawn again on top: the focused node last, its neighbours and the edges to
//!   them brighter, or the path's nodes and edges; whatever opts out of emphasis (an explicit
//!   colour, `override-focus`) as it is.
//! - **The hovered node** lifts (lighter, with a faint outline); **the hovered edge** thickens
//!   by a px.
//! - **Rings.** The focused node and the cursor's wear a 2 px ring in the strong text's colour
//!   outside their own; the reference a dashed one. A pinned node's dot moves out onto the
//!   ring, so pinned and focused read at once.
//! - **A drag's ghost**: a dashed outline where the node was when the drag began.
//! - **Labels.** The focused node's name in semibold, its neighbours', the hovered node's and
//!   the cursor's always show, placed again where the base culled them.
//! - **The card**: a node's label, group, degree and its first four other fields, values
//!   first; an edge's ends and its label or weight; and with the reference, the hops to it.
//!   It never covers its node, and flips at the area's edges with a cell's hysteresis.

use super::draw::{edge, label, node, pin};
use super::geom::{GraphLayout, RING};
use super::label::{Field, LABEL_GAP, choose, focus_spec, further, label_spec, seg_meets, spots};
use super::{Emphasis, GraphModel, Target};
use crate::data::SeriesKey;
use crate::interact::consts::CARD_GAP;
use crate::interact::{Cursor, View};
use crate::plot::marks::mix;
use crate::plot::overlay::lift;
use crate::scale::format_value;
use crate::scene::{Anchor, Item, Measure, Paint, PathData, Rect, Scene, Stroke, TextSpec, Weight};
use crate::theme::{Color, Theme};

/// A segment from one point to another, in px.
type Seg = ((f32, f32), (f32, f32));
/// A label the overlay shows: it, whether it's the focused one's, and the base label it
/// replaces.
type Shown = (super::NodeLabel, bool, Option<Rect>);

/// How much of the surface the emphasis wash lays over the graph: the rest show at about 25 %.
/// A whole step of 255 (192), so the surface over itself comes out as itself, not a level off.
pub const WASH: f32 = 192.0 / 255.0;
/// The focus ring's radius past a node's own, and its width.
pub const FOCUS_RING: (f32, f32) = (4.5, 2.0);
/// The most of a node's own fields its card lists.
pub const CARD_FIELDS: usize = 4;
const CARD_RADIUS: f32 = 8.0;
const CARD_ROW: f32 = 21.0;
const PAD: f32 = 12.0;
const GAP: f32 = 12.0;
const KEY_W: f32 = 10.0;
const INSET: f32 = 4.0;
/// How far a card may stray from what it's about before that costs steeply, in px.
const STRAY: f32 = 60.0;
/// The widest a card's value and name columns grow before their text is cut.
const VALUE_MAX: f32 = 180.0;
const NAME_MAX: f32 = 140.0;

/// What the overlay shows.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct OverlayState {
    /// The node or edge under the pointer: it lifts and its card shows.
    pub hover: Option<Target>,
    /// The keyboard's cursor, a [`Cursor::Node`]: its ring, and its emphasis, which sticks.
    pub cursor: Option<Cursor>,
    /// Where the pointer is, in logical px: an edge's card goes beside it.
    pub pointer: Option<(f32, f32)>,
    /// The node the hover brings into focus once it has dwelt there (`DWELL_EMPHASIS`), or a
    /// search hit's: it takes the focus from the cursor's.
    pub emphasis: Option<Cursor>,
    /// A group whose legend key is under the pointer: its nodes come forward.
    pub group: Option<SeriesKey>,
    /// A node being dragged, and where it was in layout units when the drag began.
    pub ghost: Option<(Box<str>, (f64, f64))>,
    /// Whether the cursor's node shows its card while nothing is hovered (off while the detail
    /// panel says the same).
    pub cursor_card: bool,
    /// The nodes a search typed so far matches: they stay bright and the rest dim, the best
    /// (`emphasis`) in focus.
    pub matches: Vec<usize>,
    /// The cursor's emphasis let go (`Esc`): its node keeps its ring, and nothing dims for it.
    pub calm: bool,
}

/// The overlay's scene: see the module's docs. The reference is the view's.
pub fn overlay(
    model: &GraphModel,
    layout: &GraphLayout,
    view: &View,
    st: &OverlayState,
    theme: &Theme,
    m: &mut dyn Measure,
) -> Scene {
    let mut scene = Scene::new(layout.size.0, layout.size.1, layout.scale);
    let drawn = |i: usize| layout.node(i).is_some();
    let node_of = |c: &Option<Cursor>| {
        c.as_ref()
            .and_then(|c| model.node_of(c))
            .filter(|&i| drawn(i))
    };
    let cursor = node_of(&st.cursor);
    let dwell = node_of(&st.emphasis);
    let reference = node_of(&view.reference);
    let hover = st.hover.filter(|t| match *t {
        Target::Node(i) => drawn(i as usize),
        Target::Edge(k) => layout.edges.get(k as usize).is_some_and(Option::is_some),
    });
    let hover_node = hover.and_then(Target::node);
    let hover_edge = hover.and_then(Target::edge);

    // What's emphasised: a path from the reference, a group, else the node in focus.
    let path_end = hover_node.or(cursor).filter(|&e| Some(e) != reference);
    let path = match (reference, path_end) {
        (Some(r), Some(e)) => model.path_shown(view, r, e).filter(|p| p.len() >= 2),
        _ => None,
    };
    let focus = dwell.or(cursor.filter(|_| !st.calm));
    let group = st
        .group
        .as_ref()
        .filter(|g| model.groups.iter().any(|x| *x == *g.name));
    let emph = if let Some(p) = &path {
        Some(model.path_emphasis(p))
    } else if let Some(g) = group {
        Some(model.group_emphasis(g))
    } else if !st.matches.is_empty() {
        Some(model.set_emphasis(&st.matches, dwell))
    } else {
        focus.map(|f| model.emphasis(Some(f)))
    };

    // What wears a ring: the cursor's node, the one in focus, the path's far end.
    let mut ringed: Vec<usize> = Vec::new();
    for i in cursor.iter().chain(dwell.iter()) {
        if !ringed.contains(i) {
            ringed.push(*i);
        }
    }
    if let Some(p) = &path
        && let Some(&e) = p.last()
        && !ringed.contains(&e)
    {
        ringed.push(e);
    }
    // Labels at full strength: the focused in semibold, the related, the hovered, the cursor's.
    let mut want: Vec<(usize, bool)> = Vec::new();
    if let Some((nodes_e, _)) = &emph {
        for (i, e) in nodes_e.iter().enumerate() {
            match e {
                Emphasis::Focused => want.push((i, true)),
                Emphasis::Related => want.push((i, false)),
                Emphasis::Normal if layout.labels[i].is_some() => want.push((i, false)),
                _ => {}
            }
        }
    }
    for i in ringed
        .iter()
        .chain(hover_node.iter())
        .chain(reference.iter())
    {
        if !want.iter().any(|(j, _)| j == i) {
            want.push((*i, ringed.contains(i)));
        }
    }
    let mut wearing: Vec<usize> = ringed.clone();
    wearing.extend(reference);
    // The edges drawn bright, which a label's ground mustn't cut.
    let mut hard: Vec<Seg> = Vec::new();
    for (k, e) in layout.edges.iter().enumerate() {
        let Some(e) = e else { continue };
        let bright = hover_edge == Some(k)
            || emph
                .as_ref()
                .is_some_and(|(_, ee)| matches!(ee[k], Emphasis::Related | Emphasis::Focused));
        if bright {
            hard.extend(e.pts.windows(2).map(|w| (w[0], w[1])));
        }
    }
    let dimmed: Option<Vec<bool>> = emph
        .as_ref()
        .map(|(ne, _)| ne.iter().map(|e| *e == Emphasis::Dimmed).collect());
    let (shown_labels, under, hidden) = labels(
        model,
        layout,
        &want,
        emph.is_some(),
        &wearing,
        &hard,
        dimmed.as_deref(),
        m,
    );
    if emph.is_some() && !layout.ink.is_empty() {
        wash(&mut scene, layout, theme);
    }
    if !layout.contained {
        scene.push(Item::PushClip(layout.region));
    }
    // The base's labels of nodes labelled again, cleared so they don't show twice.
    let cleared: Vec<Rect> = shown_labels
        .iter()
        .filter_map(|(_, _, e)| *e)
        .chain(under)
        .collect();
    for r in cleared {
        scene.push(Item::Path {
            path: PathData::rect(r.inflate(1.0)),
            fill: Some(Paint::solid(theme.surface, 1.0)),
            stroke: None,
        });
    }
    // The dimmed nodes a label's ground lies over, hidden with it.
    for (c, r) in hidden {
        scene.push(Item::Path {
            path: PathData::circle(c.0, c.1, r + 0.5),
            fill: Some(Paint::solid(theme.surface, 1.0)),
            stroke: None,
        });
    }
    // Where what the overlay draws again over the base starts: the base's labels the card
    // cuts are cleared here, under it.
    let redrawn_at = scene.items.len();
    if let Some((_, edges_e)) = &emph {
        let on_path = path.is_some();
        for (k, e) in layout.edges.iter().enumerate() {
            let Some(e) = e else { continue };
            match edges_e[k] {
                Emphasis::Related if on_path => edge(&mut scene, e, theme.strong, 0.9, 1.0),
                Emphasis::Related => {
                    let c = if model.edge_opts_out(k) {
                        e.color
                    } else {
                        theme.muted
                    };
                    edge(&mut scene, e, c, 1.0, 0.5)
                }
                Emphasis::Normal => edge(&mut scene, e, e.color, 1.0, 0.0),
                _ => {}
            }
        }
    }
    if let Some(k) = hover_edge
        && let Some(e) = &layout.edges[k]
    {
        let c = if model.edge_opts_out(k) {
            lift(e.color, theme)
        } else {
            theme.text
        };
        edge(&mut scene, e, c, 1.0, 1.0);
        // Its ends come forward with it.
        let (a, b) = model.ends[k];
        for i in [a as usize, b as usize] {
            if let Some(g) = layout.node(i) {
                node(&mut scene, g, g.color, theme);
            }
        }
    }
    if let Some((id, was)) = &st.ghost
        && let Some(i) = model.index_of(id)
        && let Some(g) = layout.node(i)
    {
        let at = layout.frame.px(*was);
        let mut s = Stroke::solid(1.0, theme.muted, 0.9);
        s.dash = Some((3.0, 3.0));
        scene.push(Item::Path {
            path: PathData::circle(at.0, at.1, g.r),
            fill: Some(Paint::solid(g.color, 0.12)),
            stroke: Some(s),
        });
    }
    // Nodes drawn again: what opts out, then the related, then the focused last.
    let mut redrawn: Vec<usize> = Vec::new();
    if let Some((nodes_e, _)) = &emph {
        for pass in [Emphasis::Normal, Emphasis::Related, Emphasis::Focused] {
            for (i, e) in nodes_e.iter().enumerate() {
                if *e != pass {
                    continue;
                }
                let Some(g) = layout.node(i) else { continue };
                node(&mut scene, g, g.color, theme);
                redrawn.push(i);
            }
        }
    }
    for &i in &ringed {
        if let Some(g) = layout.node(i)
            && !redrawn.contains(&i)
        {
            node(&mut scene, g, g.color, theme);
            redrawn.push(i);
        }
    }
    if let Some(i) = hover_node
        && let Some(g) = layout.node(i)
    {
        scene.push(Item::Path {
            path: PathData::circle(g.at.0, g.at.1, g.r),
            fill: Some(Paint::solid(lift(g.color, theme), 1.0)),
            stroke: Some(Stroke::solid(RING, theme.surface, 1.0)),
        });
        scene.push(Item::Path {
            path: PathData::circle(g.at.0, g.at.1, g.r + 0.5),
            fill: None,
            stroke: Some(Stroke::solid(1.0, theme.strong, 0.6)),
        });
        if !redrawn.contains(&i) {
            redrawn.push(i);
        }
    }
    // Rings: the reference's dashed, the focused and the cursor's solid.
    let ring_r = |g: &super::NodeGeom| g.r + FOCUS_RING.0;
    if let Some(r) = reference
        && let Some(g) = layout.node(r)
        && !ringed.contains(&r)
    {
        let mut s = Stroke::solid(1.5, theme.strong, 0.75);
        s.dash = Some((3.0, 2.5));
        scene.push(Item::Path {
            path: PathData::circle(g.at.0, g.at.1, ring_r(g)),
            fill: None,
            stroke: Some(s),
        });
        if !redrawn.contains(&r) {
            node(&mut scene, g, g.color, theme);
            redrawn.push(r);
        }
    }
    for &i in &ringed {
        if let Some(g) = layout.node(i) {
            scene.push(Item::Path {
                path: PathData::circle(g.at.0, g.at.1, ring_r(g)),
                fill: None,
                stroke: Some(Stroke::solid(FOCUS_RING.1, theme.strong, 1.0)),
            });
        }
    }
    let ring_of = |i: usize| ringed.contains(&i) || reference == Some(i);
    for &i in &redrawn {
        if let Some(g) = layout.node(i)
            && g.pinned
        {
            let at = if ring_of(i) {
                ring_r(g) + FOCUS_RING.1 * 0.5 + 1.0
            } else {
                g.r + RING * 0.5
            };
            pin(&mut scene, g.at, at, theme);
        }
    }

    // The card: the hover's, else the cursor's.
    let card_of = hover.or(if st.cursor_card {
        cursor.map(|i| Target::Node(i as u32))
    } else {
        None
    });
    let mut card_spot: Option<(Card, Rect)> = None;
    if let Some(t) = card_of {
        let hops = path
            .as_ref()
            .filter(|p| Some(*p.last().unwrap_or(&0)) == t.node());
        let card = Card::of(model, view, t, hops.map(|p| (p.len() - 1, p[0])), theme, m);
        let (anchor, reach) = match t {
            Target::Node(i) => {
                let g = layout.node(i as usize).expect("a drawn node");
                let r = if ring_of(i as usize) {
                    ring_r(g) + FOCUS_RING.1
                } else {
                    g.r + RING
                };
                (g.at, r)
            }
            Target::Edge(k) => (
                st.pointer
                    .unwrap_or_else(|| edge_middle(layout, k as usize)),
                3.0,
            ),
        };
        // What the card should keep off, by how much it matters: the node in focus and the
        // cursor's most, the emphasised next, and their labels; the rest a little.
        let (nodes_e, _) = emph.clone().unwrap_or_else(|| model.emphasis(None));
        let mut avoid: Vec<(Keep, f32)> = Vec::new();
        // An edge's ends are what its card is about.
        let ends = hover_edge.map(|k| model.ends[k]);
        let is_end = |i: usize| ends.is_some_and(|(a, b)| a as usize == i || b as usize == i);
        for (i, g) in layout.nodes.iter().enumerate() {
            let Some(g) = g else { continue };
            let w = if Some(i) == cursor || ringed.contains(&i) || is_end(i) {
                24.0
            } else {
                match nodes_e[i] {
                    Emphasis::Focused => 24.0,
                    Emphasis::Related => 12.0,
                    Emphasis::Normal => 6.0,
                    Emphasis::Dimmed => 1.0,
                }
            };
            // Its disc and ring, not the square about them: a card beside a node by its
            // corner covers nothing of it.
            avoid.push((Keep::Disc(g.at, g.r + RING + 2.0), w));
            // The base's label, unless the overlay showed it again elsewhere.
            if let Some(l) = &layout.labels[i]
                && !shown_labels.iter().any(|(_, _, e)| *e == Some(l.rect))
            {
                let lw = match nodes_e[i] {
                    _ if is_end(i) => 10.0,
                    Emphasis::Focused | Emphasis::Related => 10.0,
                    Emphasis::Normal => 1.5,
                    Emphasis::Dimmed => 0.5,
                };
                avoid.push((Keep::Rect(l.rect), lw));
            }
        }
        for (l, _, _) in &shown_labels {
            avoid.push((Keep::Rect(l.rect), 10.0));
        }
        // The edges drawn bright: the hovered one, which its card is about, most, then a
        // path's, then a neighbour's.
        let mut lines: Vec<(Vec<(f32, f32)>, f32)> = Vec::new();
        if let Some(k) = hover_edge
            && let Some(e) = &layout.edges[k]
        {
            lines.push((e.pts.clone(), 10.0));
        }
        if let Some((_, edges_e)) = &emph {
            for (k, e) in layout.edges.iter().enumerate() {
                if let Some(e) = e
                    && edges_e[k] == Emphasis::Related
                {
                    lines.push((e.pts.clone(), if path.is_some() { 8.0 } else { 3.0 }));
                }
            }
        }
        // The header: the title and the legend cost more to cover than the graph's space.
        if !layout.title.is_empty() {
            avoid.push((Keep::Rect(layout.title.inflate(4.0)), 12.0));
        }
        for (_, r) in &layout.legend {
            avoid.push((Keep::Rect(*r), 6.0));
        }
        let (card, r) = card.place(layout, anchor, reach, &avoid, &lines, m);
        card_spot = Some((card, r));
    }
    // A label the card would cut goes whole, so no piece of it peeks out at the card's edge:
    // a base one (dimmed, or at rest) is cleared under the overlay's own redrawing, a shown one
    // isn't drawn.
    let card_rect = card_spot.as_ref().map(|(_, r)| *r);
    let cuts = |l: &Rect| {
        card_rect.is_some_and(|r| l.intersect(&r).is_some_and(|o| o.w > 0.5 && o.h > 0.5))
    };
    let mut covers: Vec<Item> = Vec::new();
    for (i, l) in layout.labels.iter().enumerate() {
        let Some(l) = l else { continue };
        let again = shown_labels
            .iter()
            .any(|(s, _, e)| *e == Some(l.rect) || s.rect == l.rect);
        if !again && hover_node != Some(i) && cuts(&l.rect) {
            covers.push(Item::Path {
                path: PathData::rect(l.rect.inflate(1.0)),
                fill: Some(Paint::solid(theme.surface, 1.0)),
                stroke: None,
            });
        }
    }
    for (l, focus, _) in &shown_labels {
        if cuts(&l.rect) {
            continue;
        }
        let color = if *focus { theme.strong } else { theme.text };
        label(&mut scene, l, color, 1.0, theme);
    }
    if !layout.contained {
        scene.push(Item::PopClip);
    }
    scene.items.splice(redrawn_at..redrawn_at, covers);
    if let Some((card, r)) = card_spot {
        card.draw(&mut scene, r, theme, m);
    }

    scene
}

/// What a card keeps off: a rectangle (a label, the header) or a node's disc.
#[derive(Clone, Copy, Debug)]
enum Keep {
    Rect(Rect),
    Disc((f32, f32), f32),
}

impl Keep {
    /// Whether a card at `r` covers some of it.
    fn covered(&self, r: &Rect) -> bool {
        match self {
            Keep::Rect(a) => a.intersect(r).is_some_and(|i| i.w > 0.5 && i.h > 0.5),
            Keep::Disc(c, rad) => circle_meets(*c, *rad, r),
        }
    }
}

/// What a row the card leaves out to stay clear costs, against what covering costs: less than
/// a neighbour's node or the title, more than a few dimmed ones.
const ROW_DROP: f32 = 4.0;

/// The emphasis wash: the surface at [`WASH`] over what the graph inks, within the region.
fn wash(scene: &mut Scene, layout: &GraphLayout, theme: &Theme) {
    let Some(r) = layout.ink.inflate(2.0).intersect(&layout.region) else {
        return;
    };
    scene.push(Item::Path {
        path: PathData::rect(r),
        fill: Some(Paint::solid(theme.surface, WASH)),
        stroke: None,
    });
}

/// The middle of an edge's path, where its card goes with no pointer.
fn edge_middle(layout: &GraphLayout, k: usize) -> (f32, f32) {
    let Some(e) = layout.edges.get(k).and_then(Option::as_ref) else {
        return (layout.size.0 / 2.0, layout.size.1 / 2.0);
    };
    e.pts[e.pts.len() / 2]
}

/// The labels `want` asks for at full strength (`true` for the focused style), each with the
/// base label it replaces, to clear: each in
/// its base place where it has one, else (and the focused one always, in semibold, its base
/// label cleared) placed clear of the nodes, the edges and the labels shown at full, the
/// busiest first; over a dimmed label or an edge only on a ground, where nothing is free.
#[allow(clippy::too_many_arguments)]
fn labels(
    model: &GraphModel,
    layout: &GraphLayout,
    want: &[(usize, bool)],
    washed: bool,
    ringed: &[usize],
    hard: &[Seg],
    dimmed: Option<&[bool]>,
    m: &mut dyn Measure,
) -> (Vec<Shown>, Vec<Rect>, Vec<((f32, f32), f32)>) {
    let mut order: Vec<(usize, bool)> = want
        .iter()
        .copied()
        .filter(|&(i, _)| {
            layout
                .node(i)
                .is_some_and(|g| layout.bounds.contains(g.at.0, g.at.1))
        })
        .collect();
    order.sort_by_key(|&(i, focus)| (!focus, std::cmp::Reverse(model.degree(i)), i));
    // Placed again: the focused, one the base culled, and one on a ground, which would cut
    // an edge drawn bright over the wash.
    let fresh = |i: usize, focus: bool| {
        focus
            || layout.labels[i]
                .as_ref()
                .is_none_or(|b| b.backed && !hard.is_empty())
    };
    let mut shown: Vec<Shown> = Vec::new();
    let mut erase: Vec<Rect> = Vec::new();
    let mut hidden: Vec<((f32, f32), f32)> = Vec::new();
    if order.iter().any(|&(i, f)| fresh(i, f)) {
        let mut field = Field::of(layout);
        for &(a, b) in hard {
            field.add_hard(a, b);
        }
        if let Some(dim) = dimmed {
            field.soften_nodes(|i| dim.get(i).is_some_and(|&d| d));
        }
        for (j, l) in layout.labels.iter().enumerate() {
            let Some(l) = l else { continue };
            match order.iter().find(|&&(k, _)| k == j) {
                Some(&(k, f)) if fresh(k, f) => {}
                Some(_) => field.add_label(l.rect),
                None if washed => field.add_soft(l.rect),
                None => field.add_label(l.rect),
            }
        }
        for &(i, focus) in &order {
            if !fresh(i, focus) {
                continue;
            }
            let g = layout.node(i).expect("filtered to drawn nodes");
            let spec = if focus { focus_spec() } else { label_spec() };
            let gap = if ringed.contains(&i) {
                LABEL_GAP + FOCUS_RING.0 + FOCUS_RING.1 - RING * 0.5
            } else {
                LABEL_GAP
            };
            let text = m.fit(
                model.label(i),
                spec,
                (layout.size.0 * 0.45).clamp(40.0, 260.0),
            );
            let mut cands = spots(&text, g.at, g.r, gap, spec, m);
            // The base's side first, so a label doesn't jump when its node comes into focus.
            if let Some(b) = &layout.labels[i]
                && let Some(p) = cands.iter().position(|c| c.side == b.side)
            {
                let c = cands.remove(p);
                cands.insert(0, c);
            }
            cands.extend(further(&text, g.at, g.r, gap, spec, m));
            // The focused label shows whatever it covers; a neighbour's covers no label and no
            // node drawn bright, or doesn't show (it's lit, and named in the card and the panel).
            let block_ok = if focus { u32::MAX } else { 99 };
            if let Some(l) = choose(&mut field, cands, i, u32::MAX, block_ok) {
                field.add_label(l.rect);
                // The dimmed labels and nodes it lies over, cleared with its own old place.
                erase.extend(field.soft_under(l.rect));
                hidden.extend(field.soft_nodes_under(l.rect));
                shown.push((l, focus, layout.labels[i].as_ref().map(|b| b.rect)));
            }
        }
    }
    for &(i, focus) in &order {
        if !fresh(i, focus)
            && let Some(b) = &layout.labels[i]
        {
            shown.push((b.clone(), false, None));
        }
    }
    (shown, erase, hidden)
}

/// A card's row: a key dot, a value, a name.
#[derive(Clone)]
struct Row {
    key: Option<Color>,
    value: String,
    name: String,
}

/// The card: a title over rows, values first.
#[derive(Clone)]
struct Card {
    title: String,
    rows: Vec<Row>,
    /// The rows it keeps however small it gets: a node's group and degree (and hops), an
    /// edge's label or weight.
    keep: usize,
    title_w: f32,
    val_w: f32,
    name_w: f32,
    keyed: bool,
    w: f32,
    h: f32,
}

fn vspec() -> TextSpec {
    TextSpec::new(12.5).weight(Weight::Semibold).tabular()
}

fn nspec() -> TextSpec {
    TextSpec::new(12.0)
}

fn tspec() -> TextSpec {
    TextSpec::new(12.5).weight(Weight::Semibold)
}

impl Card {
    /// The card for a node or an edge; `hops` the path's length from the reference to it.
    fn of(
        model: &GraphModel,
        view: &View,
        t: Target,
        hops: Option<(usize, usize)>,
        theme: &Theme,
        m: &mut dyn Measure,
    ) -> Card {
        let _ = view;
        let mut rows = Vec::new();
        let keep;
        let title = match t {
            Target::Node(i) => {
                let i = i as usize;
                if let Some(g) = model.group_of[i] {
                    rows.push(Row {
                        key: Some(model.color(i, theme)),
                        value: model.groups[g as usize].clone(),
                        name: "group".into(),
                    });
                }
                let deg = model.degree(i);
                let name = if model.directed() {
                    format!(
                        "degree · {} in, {} out",
                        model.degree_in[i], model.degree_out[i]
                    )
                } else {
                    "degree".into()
                };
                rows.push(Row {
                    key: None,
                    value: deg.to_string(),
                    name,
                });
                if let Some((n, from)) = hops {
                    rows.push(Row {
                        key: None,
                        value: format!("{n} hop{}", if n == 1 { "" } else { "s" }),
                        name: format!("from {}", model.label(from)),
                    });
                }
                keep = rows.len();
                for (k, v) in model.fields(i).into_iter().take(CARD_FIELDS) {
                    rows.push(Row {
                        key: None,
                        value: v,
                        name: k,
                    });
                }
                model.label(i).to_string()
            }
            Target::Edge(k) => {
                let k = k as usize;
                let e = &model.spec.edges[k];
                let (a, b) = model.ends[k];
                if let Some(l) = &e.label {
                    rows.push(Row {
                        key: None,
                        value: l.clone(),
                        name: "label".into(),
                    });
                }
                if let Some(w) = e.weight {
                    rows.push(Row {
                        key: None,
                        value: format_value(w),
                        name: "weight".into(),
                    });
                }
                keep = rows.len().max(1);
                let room = CARD_FIELDS.saturating_sub(rows.len());
                for (f, v) in model.edge_fields(k).into_iter().take(room) {
                    rows.push(Row {
                        key: None,
                        value: v,
                        name: f,
                    });
                }
                let arrow = if e.directed { "→" } else { "—" };
                format!(
                    "{} {arrow} {}",
                    model.label(a as usize),
                    model.label(b as usize)
                )
            }
        };
        let keyed = rows.iter().any(|r| r.key.is_some());
        let val_w = rows
            .iter()
            .map(|r| m.width(&r.value, vspec()))
            .fold(0.0, f32::max)
            .min(VALUE_MAX);
        let name_w = rows
            .iter()
            .map(|r| m.width(&r.name, nspec()))
            .fold(0.0, f32::max)
            .min(NAME_MAX);
        let title_w = m.width(&title, tspec());
        let body = PAD
            + if keyed { KEY_W + 8.0 } else { 0.0 }
            + val_w
            + if rows.is_empty() { 0.0 } else { GAP }
            + name_w
            + PAD;
        let w = body
            .max(title_w + 2.0 * PAD)
            .min(VALUE_MAX + NAME_MAX + 80.0)
            .ceil();
        let h = Card::height(rows.len());
        Card {
            title,
            keep,
            rows,
            title_w,
            val_w,
            name_w,
            keyed,
            w,
            h,
        }
    }

    /// Its height for its rows.
    fn height(rows: usize) -> f32 {
        if rows == 0 {
            34.0
        } else {
            32.0 + rows as f32 * CARD_ROW + 7.0
        }
    }

    /// The card cut to fit `max_w` by `max_h`: its last rows dropped (its own fields first),
    /// then its name and value columns narrowed, each to 36 px at the least.
    fn shrunk(&self, max_w: f32, max_h: f32) -> Card {
        let mut c = self.clone();
        while c.h > max_h && !c.rows.is_empty() {
            c.rows.pop();
            c.h = Card::height(c.rows.len());
        }
        if c.w > max_w {
            let excess = c.w - max_w;
            let names = excess.min((c.name_w - 36.0).max(0.0));
            c.name_w -= names;
            let values = (excess - names).min((c.val_w - 36.0).max(0.0));
            c.val_w -= values;
            c.w = (c.w - names - values).min(max_w).max(1.0).floor();
        }
        c
    }

    /// Where the card goes beside `anchor`, never over the circle of `reach` about it, inside
    /// the area, where it covers least of `avoid` (each a rectangle and what covering it
    /// costs) and `lines` (each a polyline and its cost), and strays least from the anchor:
    /// right and below first, then right and above, left and below, left and above, beside at
    /// the middle each way, then under and over, then beside and under or over slid along; a
    /// side switch costs a little (the left side is kept until the right fits with a cell to
    /// spare). The place and whether it's on the left; None where nowhere fits it whole.
    fn best_place(
        &self,
        layout: &GraphLayout,
        anchor: (f32, f32),
        reach: f32,
        avoid: &[(Keep, f32)],
        lines: &[(Vec<(f32, f32)>, f32)],
    ) -> Option<(f32, Rect, bool, f32)> {
        let (aw, ah) = layout.size;
        let w = self.w.min(aw - 2.0 * INSET).max(1.0);
        let h = self.h;
        let g = CARD_GAP * 0.6 + reach * 0.75;
        let side_gap = CARD_GAP * 0.5 + reach;
        let (ax, ay) = anchor;
        let mut tries = vec![
            (Rect::new(ax + g, ay + g, w, h), false),
            (Rect::new(ax + g, ay - g - h, w, h), false),
            (Rect::new(ax - g - w, ay + g, w, h), true),
            (Rect::new(ax - g - w, ay - g - h, w, h), true),
            (Rect::new(ax + side_gap, ay - h / 2.0, w, h), false),
            (Rect::new(ax - side_gap - w, ay - h / 2.0, w, h), true),
            (Rect::new(ax - w / 2.0, ay + side_gap, w, h), ax < aw / 2.0),
            (
                Rect::new(ax - w / 2.0, ay - side_gap - h, w, h),
                ax < aw / 2.0,
            ),
        ];
        // A step or three further out beside, under and over: clear of what's by the anchor
        // (an edge's ends, a node's neighbours) while still by it.
        for out in [14.0, 28.0, 44.0] {
            tries.push((Rect::new(ax + side_gap + out, ay - h / 2.0, w, h), false));
            tries.push((Rect::new(ax - side_gap - out - w, ay - h / 2.0, w, h), true));
            tries.push((Rect::new(ax + g + out, ay + g, w, h), false));
            tries.push((Rect::new(ax - g - out - w, ay + g, w, h), true));
            tries.push((Rect::new(ax + g + out, ay - g - h, w, h), false));
            tries.push((Rect::new(ax - g - out - w, ay - g - h, w, h), true));
            let under = Rect::new(ax - w / 2.0, ay + side_gap + out, w, h);
            tries.push((under, ax < aw / 2.0));
            let over = Rect::new(ax - w / 2.0, ay - side_gap - out - h, w, h);
            tries.push((over, ax < aw / 2.0));
        }
        for k in 0..=4 {
            let t = k as f32 / 4.0;
            let y = INSET + (ah - 2.0 * INSET - h).max(0.0) * t;
            let x = INSET + (aw - 2.0 * INSET - w).max(0.0) * t;
            tries.push((Rect::new(ax + side_gap, y, w, h), false));
            tries.push((Rect::new(ax - side_gap - w, y, w, h), true));
            tries.push((Rect::new(x, ay + side_gap, w, h), x + w / 2.0 < ax));
            tries.push((Rect::new(x, ay - side_gap - h, w, h), x + w / 2.0 < ax));
        }
        let flipped = layout.flipped.get();
        let inside = |r: &Rect, spare: f32| {
            r.x >= INSET
                && r.y >= INSET
                && r.right() + spare <= aw - INSET
                && r.bottom() <= ah - INSET
        };
        let mut best: Option<(f32, Rect, bool, f32)> = None;
        for (n, &(r, left)) in tries.iter().enumerate() {
            let spare = if flipped && !left {
                layout.cell_width
            } else {
                0.0
            };
            if !inside(&r, spare) || circle_meets(anchor, reach, &r) {
                continue;
            }
            let covered: f32 = avoid
                .iter()
                .filter(|(a, _)| a.covered(&r))
                .map(|(_, c)| c)
                .sum::<f32>()
                + lines
                    .iter()
                    .filter(|(pts, _)| pts.windows(2).any(|w| seg_meets(w[0], w[1], &r)))
                    .map(|(_, c)| c)
                    .sum::<f32>();
            // A card strays from what it's about at a cost: 4 for every 100 px, and steeply past
            // [`STRAY`], where it no longer reads as the anchor's.
            let nx = ax.clamp(r.x, r.right());
            let ny = ay.clamp(r.y, r.bottom());
            let d = ((ax - nx).hypot(ay - ny) - reach).max(0.0);
            let far = d * 0.04 + (d - STRAY).max(0.0) * 0.3;
            let cost =
                covered + far + (n as f32).min(8.0) * 0.6 + if left != flipped { 2.0 } else { 0.0 };
            if best.is_none_or(|(b, _, _, _)| cost < b) {
                best = Some((cost, r, left, covered));
            }
        }
        best
    }

    /// Where the card goes, as [`Card::best_place`] puts it, whole or with its last rows left
    /// out (its own fields first, [`ROW_DROP`] a row), whichever costs least: so a small area's
    /// card says less rather than cover the neighbours, the title or the legend. Where it can't
    /// fit at all, cut to fit beside the anchor or above or below it (whichever keeps more
    /// rows) and placed again; where even that fails, slid into the area off the anchor. The
    /// card as it's drawn, and where.
    fn place(
        &self,
        layout: &GraphLayout,
        anchor: (f32, f32),
        reach: f32,
        avoid: &[(Keep, f32)],
        lines: &[(Vec<(f32, f32)>, f32)],
        m: &mut dyn Measure,
    ) -> (Card, Rect) {
        let (aw, ah) = layout.size;
        let (ax, ay) = anchor;
        let side_gap = CARD_GAP * 0.5 + reach;
        let mut card = self.clone();
        let least = self.rows.len().min(self.keep);
        let mut best: Option<(f32, Card, (Rect, bool))> = None;
        for n in (least..=self.rows.len()).rev() {
            let mut c = self.clone();
            c.rows.truncate(n);
            c.h = Card::height(n);
            let dropped = (self.rows.len() - n) as f32 * ROW_DROP;
            if best.as_ref().is_some_and(|(b, _, _)| dropped >= *b) {
                // Shorter still can only cost more than the best so far.
                break;
            }
            if let Some((cost, r, left, covered)) =
                c.best_place(layout, anchor, reach, avoid, lines)
            {
                let cost = cost + dropped;
                if best.as_ref().is_none_or(|(b, _, _)| cost < *b) {
                    best = Some((cost, c, (r, left)));
                }
                if covered < ROW_DROP && n == self.rows.len() {
                    // Whole and clear of everything that matters: no row to drop.
                    break;
                }
            }
        }
        let mut spot = None;
        if let Some((_, c, p)) = best {
            (card, spot) = (c, Some(p));
        }
        if spot.is_none() {
            let beside = card.shrunk(
                (ax - side_gap).max(aw - ax - side_gap) - 2.0 * INSET,
                ah - 2.0 * INSET,
            );
            let over = card.shrunk(
                aw - 2.0 * INSET,
                (ay - side_gap).max(ah - ay - side_gap) - 2.0 * INSET,
            );
            let mut cuts = [beside, over];
            cuts.sort_by_key(|c| std::cmp::Reverse(c.rows.len()));
            for c in cuts {
                if let Some((_, r, left, _)) = c.best_place(layout, anchor, reach, avoid, lines) {
                    spot = Some((r, left));
                    card = c;
                    break;
                }
                card = c;
            }
        }
        let (w, h) = (card.w.min(aw - 2.0 * INSET).max(1.0), card.h);
        let g = CARD_GAP * 0.6 + reach * 0.75;
        let (r, left_side) = match spot {
            Some(p) => p,
            None => {
                let room_right = aw - ax;
                let r = if room_right >= ax {
                    Rect::new(ax + g, ay + g, w, h)
                } else {
                    Rect::new(ax - g - w, ay + g, w, h)
                };
                let x = r.x.clamp(INSET, (aw - INSET - w).max(INSET));
                let y = r.y.clamp(INSET, (ah - INSET - h).max(INSET));
                let mut r = Rect::new(x, y, w, h);
                if circle_meets(anchor, reach, &r) {
                    // Off the anchor: above it, else below, else beside.
                    let up = ay - reach - 4.0 - h;
                    let down = ay + reach + 4.0;
                    if up >= INSET {
                        r.y = up;
                    } else if down + h <= ah - INSET {
                        r.y = down;
                    } else if ax - reach - 4.0 - w >= INSET {
                        r.x = ax - reach - 4.0 - w;
                    } else {
                        r.x = (ax + reach + 4.0).min(aw - w);
                    }
                }
                (r, room_right < ax)
            }
        };
        layout.flipped.set(left_side);
        let _ = m;
        (card, Rect::new(r.x.round(), r.y.round(), r.w, r.h))
    }

    fn draw(&self, scene: &mut Scene, r: Rect, theme: &Theme, m: &mut dyn Measure) {
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
        let title = m.fit(&self.title, tspec(), r.w - 2.0 * PAD);
        let _ = self.title_w;
        scene.push(Item::Text {
            text: title.into(),
            x: r.x + PAD,
            y: r.y + 22.0,
            spec: tspec(),
            anchor: Anchor::Start,
            color: theme.strong,
            alpha: 1.0,
            max_width: None,
        });
        let kx = r.x + PAD;
        let vx = kx + if self.keyed { KEY_W + 8.0 } else { 0.0 };
        let nx = vx + self.val_w + GAP;
        let name_max = (r.right() - PAD - nx)
            .max(1.0)
            .min(self.name_w.max(1.0) + 2.0);
        let value_max = (nx - GAP - vx).max(1.0) + 2.0;
        for (k, row) in self.rows.iter().enumerate() {
            let y0 = r.y + 32.0 + k as f32 * CARD_ROW;
            let base = y0 + 14.5;
            if let Some(c) = row.key {
                scene.push(Item::Path {
                    path: PathData::circle(kx + KEY_W / 2.0, y0 + 10.0, 4.5),
                    fill: Some(Paint::solid(c, 1.0)),
                    stroke: None,
                });
            }
            scene.push(Item::Text {
                text: m.fit(&row.value, vspec(), value_max).into(),
                x: vx,
                y: base,
                spec: vspec(),
                anchor: Anchor::Start,
                color: theme.strong,
                alpha: 1.0,
                max_width: None,
            });
            scene.push(Item::Text {
                text: m.fit(&row.name, nspec(), name_max).into(),
                x: nx,
                y: base,
                spec: nspec(),
                anchor: Anchor::Start,
                color: mix(theme.text, theme.muted, 0.35),
                alpha: 1.0,
                max_width: None,
            });
        }
    }
}

/// Whether a circle meets a rectangle.
fn circle_meets(c: (f32, f32), r: f32, rect: &Rect) -> bool {
    let nx = c.0.clamp(rect.x, rect.right());
    let ny = c.1.clamp(rect.y, rect.bottom());
    (c.0 - nx).hypot(c.1 - ny) < r
}

/// The card's rectangle a scene's overlay drew, if it drew one (for tests: its shadow's).
pub fn card_rect(scene: &Scene) -> Option<Rect> {
    scene.items.iter().find_map(|i| match i {
        Item::Shadow { rect, .. } => Some(*rect),
        _ => None,
    })
}
