//! The network figure: nodes and edges laid out by stress, tree or layers, drawn with labels in
//! text tokens (ruling 14), hovered, dragged and pinned.
//!
//! The editor drives a graph as it drives a chart (`plot/`):
//!
//! - [`GraphModel::build`] resolves a [`GraphSpec`] (fig's graph JSON, the grammar's, or a tree
//!   from a table: [`crate::spec::graph::from_table`]) and lays it out once: run it in a job;
//! - [`base`] draws the graph at rest for a [`View`] and a size, with its regions, and returns the
//!   [`GraphLayout`] the rest asks: where every node, edge and label landed;
//! - [`overlay`] draws what follows the pointer and the cursor ([`OverlayState`]): emphasis, the
//!   hovered node or edge, the cursor's ring, the reference and its path, a drag's ghost, and
//!   the card;
//! - [`hover`] finds the node or edge under the pointer;
//! - [`nav`] moves the cursor between nodes and changes the view: fit, zoom, pan, reveal,
//!   centre, and a node dragged and pinned;
//! - [`GraphModel::path`], [`GraphModel::tsv`] and [`GraphModel::names`] give the shortest
//!   path, the node and edge tables, and what a search can find.
//!
//! A [`View`]'s `x` and `y` are the window onto the layout's units (y up, as fig's), None to fit
//! the graph; `hidden` and `only` hold groups (each a [`SeriesKey`] named by its group, `nth`
//! 0); `pinned` holds nodes moved by hand, in layout units; `reference` a [`Cursor::Node`].
//! Positions on screen are logical px in the figure's area.
//!
//! # The look
//!
//! Nodes are circles with a 2 px surface ring, sized by a `size` field or else by degree on a
//! square-root scale, 6 to 18 px across, and coloured by a `group` field in the series slots by
//! first appearance (slot 1 for every node when there are none; a ninth group and on in the
//! theme's neutral). Edges are hairlines in the baseline's grey, 1 to 3 px when weights vary,
//! with an arrowhead ending at the ring for a directed edge; a self-loop is a small loop on the
//! node's upper right, and edges between the same two nodes bow apart. Labels are in text
//! colours beside their nodes on the first free side ([`label`]), culled where they can't be
//! placed clear, the busiest nodes first. A tree is drawn top down with a manager's leaves
//! stacked in a column under it, and a layered graph top down with its long edges routed round
//! the nodes they pass.

pub mod draw;
pub mod geom;
pub mod hover;
pub mod label;
pub mod nav;
pub mod overlay;

pub use draw::base;
pub use geom::{EdgeGeom, Frame, GraphLayout, NodeGeom};
pub use hover::{Target, hover};
pub use label::NodeLabel;
pub use overlay::{OverlayState, overlay};

use crate::algo;
use crate::data::{SeriesKey, Table};
use crate::error::{ErrorKind, FigError, Result};
use crate::interact::{Cursor, View};
use crate::json::{Kind, Node};
use crate::scale::format_value;
use crate::spec::graph::{from_table, layout_name};
use crate::spec::{GraphSpec, Labels, Layout};
use crate::style::{ColorSpec, FigStyle};
use crate::theme::{Color, Theme, color_of};
use std::collections::{BTreeMap, VecDeque};

/// The least and greatest node radius in logical px, before a `fig` size: 6 to 18 px across.
pub const RADIUS: (f32, f32) = (3.0, 9.0);
/// How far right of its manager a stacked leaf sits, in layout units.
pub const STACK_INDENT: f64 = 0.22;
/// How far apart stacked leaves are, in layout units (a rank is 1).
pub const STACK_STEP: f64 = 0.5;
/// How far below its manager the first stacked leaf is.
pub const STACK_FIRST: f64 = 0.7;

/// How a node or an edge is emphasised (fig's plan's classes, briefs/fig-port.md §7).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Emphasis {
    /// Nothing is emphasised, or it opts out (`override-focus`, an explicit colour).
    Normal,
    /// The node in focus.
    Focused,
    /// A neighbour of the focused node and the edge to it, or a node or edge on the path.
    Related,
    /// Everything else while something is in focus: about 25 %.
    Dimmed,
}

/// The node and edge tables of a graph, for `t`.
#[derive(Clone, Debug, PartialEq)]
pub struct GraphTsv {
    /// The nodes: a header line, then a node a line.
    pub nodes: String,
    /// The edges: a header line, then an edge a line.
    pub edges: String,
    /// The line of `nodes` the cursor's node is on (from 0, the header's), if it's shown.
    pub line: Option<usize>,
}

/// A graph resolved and laid out: ids indexed, adjacency and degrees counted, groups slotted,
/// sizes and widths scaled, every node placed once.
#[derive(Clone, Debug)]
pub struct GraphModel {
    /// The spec it was built from.
    pub spec: GraphSpec,
    /// The source's name, which the subtitle names when the spec has none.
    pub source: Option<String>,
    /// The layout drawn: the spec's, or the one [`Layout::Auto`] picked.
    pub layout: Layout,
    /// Each node's place in layout units, y up: the layout's own, before any pin.
    pub positions: Vec<(f64, f64)>,
    /// Each edge's bends in layout units, from its `from` end: a layered layout's routes.
    pub bends: Vec<Vec<(f64, f64)>>,
    /// Each edge's ends, as node indices.
    pub ends: Vec<(u32, u32)>,
    /// Each node's neighbours and the edge to each, in edge order, either direction; a
    /// self-loop isn't a neighbour, and two edges to one node list it twice.
    pub adjacency: Vec<Vec<(u32, u32)>>,
    /// Each node's edges in (a self-loop counts once each way).
    pub degree_in: Vec<u32>,
    /// Each node's edges out.
    pub degree_out: Vec<u32>,
    /// The groups, in the order they first appear.
    pub groups: Vec<String>,
    /// Each node's group, an index into `groups`.
    pub group_of: Vec<Option<u32>>,
    /// Each node's radius in logical px: by its size or degree, times its `fig` size.
    pub radius: Vec<f32>,
    /// Each edge's width in logical px: 1 (a hairline) unless weights vary, then 1 to 3.
    pub widths: Vec<f32>,
    /// Each node's manager when it's a leaf stacked in a column under it (trees).
    pub stacked: Vec<Option<u32>>,
    /// Each edge's lane among the edges between its two nodes, and how many there are.
    pub lanes: Vec<(u16, u16)>,
    index: BTreeMap<Box<str>, u32>,
}

impl GraphModel {
    /// The graph of `spec`, laid out by its layout: a tree for a forest (`from` the parent of
    /// `to` where every node has one edge in at most, else spanning out from each component's
    /// first source), layers for a graph whose every edge is directed and that has no cycle,
    /// else fig's stress layout ([`algo::force`]), when the spec says `auto` or nothing. A
    /// node's id must be unique and an edge's ends known (a mismatch at the edge's place
    /// otherwise). This does the layout, `O(n²)` for stress: run it in a job.
    pub fn build(spec: GraphSpec) -> Result<GraphModel> {
        GraphModel::build_as(spec, None)
    }

    /// [`GraphModel::build`] with another layout than the spec's (the settings' `layout`).
    pub fn build_as(spec: GraphSpec, layout: Option<Layout>) -> Result<GraphModel> {
        let n = spec.nodes.len();
        if n == 0 {
            return Err(
                FigError::new("a graph needs at least one node").with_kind(ErrorKind::Empty)
            );
        }
        let mut index = BTreeMap::new();
        for (i, node) in spec.nodes.iter().enumerate() {
            if index.insert(node.id.as_str().into(), i as u32).is_some() {
                return Err(FigError::at(
                    format!("node ids must be unique: {} is given twice", node.id),
                    node.at,
                ));
            }
        }
        let mut ends = Vec::with_capacity(spec.edges.len());
        for e in &spec.edges {
            let end = |id: &str, which: &str| {
                index.get(id).copied().ok_or_else(|| {
                    FigError::at(format!("unknown edge {which}: {id}"), e.at)
                        .with_kind(ErrorKind::Mismatch)
                })
            };
            ends.push((end(&e.from, "source")?, end(&e.to, "target")?));
        }
        let mut adjacency = vec![Vec::new(); n];
        let mut degree_in = vec![0u32; n];
        let mut degree_out = vec![0u32; n];
        for (k, &(a, b)) in ends.iter().enumerate() {
            degree_out[a as usize] += 1;
            degree_in[b as usize] += 1;
            if a != b {
                adjacency[a as usize].push((b, k as u32));
                adjacency[b as usize].push((a, k as u32));
            }
        }
        let mut groups: Vec<String> = Vec::new();
        let group_of = spec
            .nodes
            .iter()
            .map(|node| {
                let g = node.group.as_ref()?;
                Some(match groups.iter().position(|x| x == g) {
                    Some(i) => i as u32,
                    None => {
                        groups.push(g.clone());
                        groups.len() as u32 - 1
                    }
                })
            })
            .collect();
        let lanes = lanes_of(&ends);
        let mut model = GraphModel {
            source: None,
            layout: Layout::Force,
            positions: vec![],
            bends: vec![Vec::new(); ends.len()],
            radius: vec![],
            widths: vec![],
            stacked: vec![None; n],
            ends,
            adjacency,
            degree_in,
            degree_out,
            groups,
            group_of,
            lanes,
            index,
            spec,
        };
        model.radius = model.radii();
        model.widths = model.edge_widths();
        let want = layout.unwrap_or_else(|| model.spec.layout.clone());
        model.layout = match want {
            Layout::Auto => model.auto_layout(),
            l => l,
        };
        model.place();
        Ok(model)
    }

    /// The graph of a table with an id and a parent column ([`from_table`]); None when it
    /// hasn't both.
    pub fn from_table(table: &Table) -> Option<Result<GraphModel>> {
        from_table(table).map(|s| s.and_then(GraphModel::build))
    }

    /// The model with `source` named in its subtitle.
    pub fn named(mut self, source: impl Into<String>) -> GraphModel {
        self.source = Some(source.into());
        self
    }

    /// How many nodes there are.
    pub fn len(&self) -> usize {
        self.spec.nodes.len()
    }

    /// Whether there are none (never, once built).
    pub fn is_empty(&self) -> bool {
        self.spec.nodes.is_empty()
    }

    /// A node's index by its id.
    pub fn index_of(&self, id: &str) -> Option<usize> {
        self.index.get(id).map(|&i| i as usize)
    }

    /// The node a cursor is on, if it's a node of this graph.
    pub fn node_of(&self, cursor: &Cursor) -> Option<usize> {
        match cursor {
            Cursor::Node(id) => self.index_of(id),
            _ => None,
        }
    }

    /// The cursor on node `i`.
    pub fn cursor(&self, i: usize) -> Cursor {
        Cursor::Node(self.spec.nodes[i].id.as_str().into())
    }

    /// A node's id.
    pub fn id(&self, i: usize) -> &str {
        &self.spec.nodes[i].id
    }

    /// Whether the data's `fig` blocks count (fig's `render_options`).
    pub fn render_options(&self) -> bool {
        self.spec.view.render_options
    }

    /// A node's `fig` block, or the default where blocks don't count.
    pub fn node_fig(&self, i: usize) -> FigStyle {
        if self.render_options() {
            self.spec.nodes[i].fig.clone()
        } else {
            FigStyle::default()
        }
    }

    /// An edge's `fig` block, or the default where blocks don't count.
    pub fn edge_fig(&self, k: usize) -> FigStyle {
        if self.render_options() {
            self.spec.edges[k].fig.clone()
        } else {
            FigStyle::default()
        }
    }

    /// A node's name as shown: its `fig` label, its label, else its id.
    pub fn label(&self, i: usize) -> &str {
        let node = &self.spec.nodes[i];
        if self.render_options()
            && let Some(l) = node.fig.label.as_deref()
        {
            return l;
        }
        node.label.as_deref().unwrap_or(&node.id)
    }

    /// Whether node `i` has an edge in or out.
    pub fn degree(&self, i: usize) -> u32 {
        self.degree_in[i] + self.degree_out[i]
    }

    /// Whether every edge has a direction.
    pub fn directed(&self) -> bool {
        !self.spec.edges.is_empty() && self.spec.edges.iter().all(|e| e.directed)
    }

    /// Whether the layout keeps distances the same across and up (stress), rather than fitting
    /// ranks and slots each to the room (tree, layered).
    pub fn uniform(&self) -> bool {
        matches!(self.layout, Layout::Force | Layout::Auto)
    }

    /// Whether a dragged node slides along its rank only (tree, layered).
    pub fn ranked(&self) -> bool {
        !self.uniform()
    }

    /// The legend's and the hidden set's key for a group.
    pub fn group_key(&self, g: usize) -> SeriesKey {
        SeriesKey {
            name: self.groups[g].as_str().into(),
            nth: 0,
        }
    }

    /// A node's group's key: its group's, or the key named "" for a node with none.
    pub fn node_key(&self, i: usize) -> SeriesKey {
        match self.group_of[i] {
            Some(g) => self.group_key(g as usize),
            None => SeriesKey {
                name: "".into(),
                nth: 0,
            },
        }
    }

    /// The groups' keys, in the legend's order.
    pub fn keys(&self) -> Vec<SeriesKey> {
        (0..self.groups.len()).map(|g| self.group_key(g)).collect()
    }

    /// The view a graph opens with: everything shown, at fit.
    pub fn initial_view(&self) -> View {
        View::default()
    }

    /// Whether node `i` is drawn under `view`: its group shown and not hidden by its `fig`.
    pub fn visible(&self, view: &View, i: usize) -> bool {
        !self.node_fig(i).hidden && view.shows(&self.node_key(i))
    }

    /// Whether edge `k` is drawn: both ends shown and not hidden by its `fig`.
    pub fn edge_visible(&self, view: &View, k: usize) -> bool {
        let (a, b) = self.ends[k];
        !self.edge_fig(k).hidden && self.visible(view, a as usize) && self.visible(view, b as usize)
    }

    /// Where node `i` is in layout units under `view`: its pin, else the layout's place.
    pub fn position(&self, view: &View, i: usize) -> (f64, f64) {
        view.pinned
            .get(self.id(i))
            .copied()
            .unwrap_or(self.positions[i])
    }

    /// Whether node `i` is pinned under `view`.
    pub fn pinned(&self, view: &View, i: usize) -> bool {
        view.pinned.contains_key(self.id(i))
    }

    /// A node's colour: its `fig` colour, else its group's slot (slot 1 for every node when no
    /// node has a group; the neutral for a ninth group and on, and for a node without a group
    /// among nodes with one).
    pub fn color(&self, i: usize, theme: &Theme) -> Color {
        if let Some(c) = self.node_fig(i).color {
            return color_of(c, theme);
        }
        match self.group_of[i] {
            Some(g) if (g as usize) < theme.series.len() => theme.series[g as usize],
            Some(_) => theme.other,
            None if self.groups.is_empty() => theme.series[0],
            None => theme.other,
        }
    }

    /// A group's colour.
    pub fn group_color(&self, g: usize, theme: &Theme) -> Color {
        theme.series.get(g).copied().unwrap_or(theme.other)
    }

    /// An edge's colour at rest: its `fig` colour, else the baseline's grey.
    pub fn edge_color(&self, k: usize, theme: &Theme) -> Color {
        match self.edge_fig(k).color {
            Some(c) => color_of(c, theme),
            None => edge_grey(theme),
        }
    }

    /// Whether node `i` keeps its look whatever is in focus: an explicit colour or
    /// `override-focus` (fig's rule, briefs/fig-port.md §7).
    pub fn node_opts_out(&self, i: usize) -> bool {
        let f = self.node_fig(i);
        f.color.is_some() || f.override_focus
    }

    /// Whether edge `k` keeps its look whatever is in focus.
    pub fn edge_opts_out(&self, k: usize) -> bool {
        let f = self.edge_fig(k);
        f.color.is_some() || f.override_focus
    }

    /// Whether node `i`'s label is forced on (`Some(true)`) or off by its `show-label`.
    pub fn show_label(&self, i: usize) -> Option<bool> {
        self.node_fig(i).show_label
    }

    /// Whether labels are drawn at rest (the view's `labels`, which settings turn off).
    pub fn labels_on(&self) -> bool {
        self.spec.view.labels != Labels::None
    }

    /// The neighbours of node `i`, each once, in edge order.
    pub fn neighbours(&self, i: usize) -> Vec<usize> {
        let mut out: Vec<usize> = Vec::new();
        for &(j, _) in &self.adjacency[i] {
            if !out.contains(&(j as usize)) {
                out.push(j as usize);
            }
        }
        out
    }

    /// Each node's and edge's emphasis with `focus` in focus: it, its neighbours and the edges
    /// to them Focused and Related, the rest Dimmed; with nothing in focus, all Normal. What
    /// opts out stays Normal (an explicit colour, `override-focus`), but a focused node is
    /// Focused whatever its colour (fig's bug fixed: it draws in its colour, last).
    pub fn emphasis(&self, focus: Option<usize>) -> (Vec<Emphasis>, Vec<Emphasis>) {
        let n = self.len();
        let Some(f) = focus.filter(|&f| f < n) else {
            return (
                vec![Emphasis::Normal; n],
                vec![Emphasis::Normal; self.ends.len()],
            );
        };
        let mut nodes = vec![Emphasis::Dimmed; n];
        let mut edges = vec![Emphasis::Dimmed; self.ends.len()];
        for &(j, k) in &self.adjacency[f] {
            nodes[j as usize] = Emphasis::Related;
            edges[k as usize] = Emphasis::Related;
        }
        for (k, &(a, b)) in self.ends.iter().enumerate() {
            if a as usize == f && b as usize == f {
                edges[k] = Emphasis::Related;
            }
        }
        nodes[f] = Emphasis::Focused;
        self.opt_out(&mut nodes, &mut edges, Some(f));
        (nodes, edges)
    }

    /// Emphasis along a path: its nodes and the edges between them Related, its ends Focused,
    /// the rest Dimmed.
    pub fn path_emphasis(&self, path: &[usize]) -> (Vec<Emphasis>, Vec<Emphasis>) {
        let mut nodes = vec![Emphasis::Dimmed; self.len()];
        let mut edges = vec![Emphasis::Dimmed; self.ends.len()];
        for &i in path {
            nodes[i] = Emphasis::Related;
        }
        for k in self.path_edges(path) {
            edges[k] = Emphasis::Related;
        }
        if let (Some(&a), Some(&b)) = (path.first(), path.last()) {
            nodes[a] = Emphasis::Focused;
            nodes[b] = Emphasis::Focused;
        }
        self.opt_out(&mut nodes, &mut edges, None);
        (nodes, edges)
    }

    /// Emphasis of a group (a legend key under the pointer): its nodes Related and the edges
    /// among them, the rest Dimmed.
    pub fn group_emphasis(&self, key: &SeriesKey) -> (Vec<Emphasis>, Vec<Emphasis>) {
        let nodes: Vec<Emphasis> = (0..self.len())
            .map(|i| match &self.node_key(i) == key {
                true => Emphasis::Related,
                false => Emphasis::Dimmed,
            })
            .collect();
        let mut edges: Vec<Emphasis> = self
            .ends
            .iter()
            .map(|&(a, b)| {
                match nodes[a as usize] == Emphasis::Related
                    && nodes[b as usize] == Emphasis::Related
                {
                    true => Emphasis::Related,
                    false => Emphasis::Dimmed,
                }
            })
            .collect();
        let mut nodes = nodes;
        self.opt_out(&mut nodes, &mut edges, None);
        (nodes, edges)
    }

    fn opt_out(&self, nodes: &mut [Emphasis], edges: &mut [Emphasis], focus: Option<usize>) {
        for (i, e) in nodes.iter_mut().enumerate() {
            if Some(i) != focus && *e == Emphasis::Dimmed && self.node_opts_out(i) {
                *e = Emphasis::Normal;
            }
        }
        for (k, e) in edges.iter_mut().enumerate() {
            if *e == Emphasis::Dimmed && self.edge_opts_out(k) {
                *e = Emphasis::Normal;
            }
        }
    }

    /// The shortest path from node `a` to node `b`, both ends included, over edges either way
    /// (a breadth-first search, neighbours in edge order, so the first of equal paths); None
    /// when there's none. A node to itself is a path of one.
    pub fn path(&self, a: usize, b: usize) -> Option<Vec<usize>> {
        let n = self.len();
        if a >= n || b >= n {
            return None;
        }
        let mut from = vec![usize::MAX; n];
        from[a] = a;
        let mut q = VecDeque::from([a]);
        while let Some(x) = q.pop_front() {
            if x == b {
                break;
            }
            for &(y, _) in &self.adjacency[x] {
                let y = y as usize;
                if from[y] == usize::MAX {
                    from[y] = x;
                    q.push_back(y);
                }
            }
        }
        if from[b] == usize::MAX {
            return None;
        }
        let mut path = vec![b];
        let mut x = b;
        while x != a {
            x = from[x];
            path.push(x);
        }
        path.reverse();
        Some(path)
    }

    /// The edges along a path, one between each pair of steps (the first in edge order).
    pub fn path_edges(&self, path: &[usize]) -> Vec<usize> {
        path.windows(2)
            .filter_map(|w| {
                self.adjacency[w[0]]
                    .iter()
                    .find(|(j, _)| *j as usize == w[1])
                    .map(|&(_, k)| k as usize)
            })
            .collect()
    }

    /// What a search can find: each node by its label, and by its id where that differs, with
    /// where the cursor goes; in node order.
    pub fn names(&self) -> Vec<(String, Cursor)> {
        let mut out = Vec::with_capacity(self.len());
        for i in 0..self.len() {
            let c = self.cursor(i);
            out.push((self.label(i).to_string(), c.clone()));
            if self.label(i) != self.id(i) {
                out.push((self.id(i).to_string(), c));
            }
        }
        out
    }

    /// A node's own fields for the card and the tables: each key of its object but `id`,
    /// `label`, `group`, `size` and `fig` (and a `metadata` object's keys in its place), with
    /// its value as text: a number in full, a string as it is, an object as `{3}` and an array
    /// as `[2]`.
    pub fn fields(&self, i: usize) -> Vec<(String, String)> {
        let skip = ["id", "label", "group", "size", "fig"];
        let mut out = fields_of(&self.spec.nodes[i].raw, &skip);
        // A table's row repeats what the node already says: its name column is its label, and
        // its parent column names the parent by id, which reads better as its label.
        let label = self.label(i);
        out.retain(|(k, v)| !(is_name_key(k) && v == label));
        for (k, v) in &mut out {
            if is_parent_key(k)
                && let Some(p) = self.index_of(v)
            {
                *v = self.label(p).to_string();
            }
        }
        out
    }

    /// An edge's own fields, as [`GraphModel::fields`] gives a node's.
    pub fn edge_fields(&self, k: usize) -> Vec<(String, String)> {
        fields_of(&self.spec.edges[k].raw, crate::spec::fig::EDGE)
    }

    /// The node and edge tables as TSV, the nodes and edges `view` hides left out: the nodes'
    /// id, label, group, degree, in and out, then every other field in the order first seen;
    /// the edges' from, to, label, weight and directed, then theirs. `line` is the cursor's
    /// node's line in `nodes`.
    pub fn tsv(&self, view: &View, cursor: Option<&Cursor>) -> GraphTsv {
        let clean = |s: &str| s.replace(['\t', '\n', '\r'], " ");
        let shown: Vec<usize> = (0..self.len()).filter(|&i| self.visible(view, i)).collect();
        let mut extra: Vec<String> = Vec::new();
        let node_fields: Vec<Vec<(String, String)>> =
            shown.iter().map(|&i| self.fields(i)).collect();
        for fs in &node_fields {
            for (k, _) in fs {
                if !extra.contains(k) {
                    extra.push(k.clone());
                }
            }
        }
        let mut nodes = String::from("id\tlabel\tgroup\tdegree\tin\tout");
        for k in &extra {
            nodes.push('\t');
            nodes.push_str(&clean(k));
        }
        nodes.push('\n');
        let want = cursor.and_then(|c| self.node_of(c));
        let mut line = None;
        for (row, (&i, fs)) in shown.iter().zip(node_fields.iter()).enumerate() {
            if Some(i) == want {
                line = Some(row + 1);
            }
            let node = &self.spec.nodes[i];
            let mut cells = vec![
                clean(&node.id),
                clean(self.label(i)),
                clean(node.group.as_deref().unwrap_or("")),
                self.degree(i).to_string(),
                self.degree_in[i].to_string(),
                self.degree_out[i].to_string(),
            ];
            for k in &extra {
                cells.push(
                    fs.iter()
                        .find(|(f, _)| f == k)
                        .map_or(String::new(), |(_, v)| clean(v)),
                );
            }
            nodes.push_str(&cells.join("\t"));
            nodes.push('\n');
        }
        let kept: Vec<usize> = (0..self.ends.len())
            .filter(|&k| self.edge_visible(view, k))
            .collect();
        let edge_fields: Vec<Vec<(String, String)>> =
            kept.iter().map(|&k| self.edge_fields(k)).collect();
        let mut eextra: Vec<String> = Vec::new();
        for fs in &edge_fields {
            for (k, _) in fs {
                if !eextra.contains(k) {
                    eextra.push(k.clone());
                }
            }
        }
        let mut edges = String::from("from\tto\tlabel\tweight\tdirected");
        for k in &eextra {
            edges.push('\t');
            edges.push_str(&clean(k));
        }
        edges.push('\n');
        for (&k, fs) in kept.iter().zip(edge_fields.iter()) {
            let e = &self.spec.edges[k];
            let mut cells = vec![
                clean(&e.from),
                clean(&e.to),
                clean(e.label.as_deref().unwrap_or("")),
                e.weight.map_or(String::new(), format_value),
                e.directed.to_string(),
            ];
            for f in &eextra {
                cells.push(
                    fs.iter()
                        .find(|(g, _)| g == f)
                        .map_or(String::new(), |(_, v)| clean(v)),
                );
            }
            edges.push_str(&cells.join("\t"));
            edges.push('\n');
        }
        GraphTsv { nodes, edges, line }
    }

    /// The line under the title when the spec has none: the source's name, the counts and the
    /// layout ("services.json · 16 nodes · 21 edges · force layout").
    pub fn summary(&self) -> String {
        let n = self.len();
        let e = self.ends.len();
        let mut parts: Vec<String> = Vec::new();
        if let Some(s) = self.source.as_deref().filter(|s| !s.is_empty()) {
            parts.push(s.to_string());
        }
        parts.push(format!("{n} node{}", if n == 1 { "" } else { "s" }));
        parts.push(format!("{e} edge{}", if e == 1 { "" } else { "s" }));
        parts.push(format!("{} layout", layout_name(&self.layout)));
        parts.join(" · ")
    }

    /// The radii: by `size` where any node has one (the others as the least size), else by
    /// degree, on a square-root scale from 0 onto [`RADIUS`], times 1, 1.4 or 1.8 by the `fig`
    /// size (ruling 15).
    fn radii(&self) -> Vec<f32> {
        let n = self.len();
        let sizes: Vec<Option<f64>> = self.spec.nodes.iter().map(|x| x.size).collect();
        let values: Vec<f64> = if sizes.iter().any(Option::is_some) {
            let least = sizes.iter().flatten().fold(f64::INFINITY, |a, &b| a.min(b));
            sizes.iter().map(|s| s.unwrap_or(least)).collect()
        } else {
            (0..n).map(|i| f64::from(self.degree(i))).collect()
        };
        let most = values.iter().fold(0.0f64, |a, &b| a.max(b));
        (0..n)
            .map(|i| {
                let t = if most > 0.0 {
                    (values[i].max(0.0) / most).sqrt() as f32
                } else {
                    0.35
                };
                let r = RADIUS.0 + (RADIUS.1 - RADIUS.0) * t;
                let times = match self.node_fig(i).size.unwrap_or(1).clamp(1, 3) {
                    2 => 1.4,
                    3 => 1.8,
                    _ => 1.0,
                };
                r * times
            })
            .collect()
    }

    /// Edge widths: 1 px, or 1 to 3 px by weight (its size) when two weights differ.
    fn edge_widths(&self) -> Vec<f32> {
        let ws: Vec<Option<f64>> = self
            .spec
            .edges
            .iter()
            .map(|e| e.weight.filter(|w| w.is_finite()).map(f64::abs))
            .collect();
        let lo = ws.iter().flatten().fold(f64::INFINITY, |a, &b| a.min(b));
        let hi = ws
            .iter()
            .flatten()
            .fold(f64::NEG_INFINITY, |a, &b| a.max(b));
        ws.iter()
            .enumerate()
            .map(|(k, w)| {
                let base = match (w, hi > lo) {
                    (Some(w), true) => 1.0 + 2.0 * ((w - lo) / (hi - lo)) as f32,
                    _ => 1.0,
                };
                match self.edge_fig(k).size.unwrap_or(1).clamp(1, 3) {
                    2 => base + 1.0,
                    3 => base + 2.0,
                    _ => base,
                }
            })
            .collect()
    }

    /// The layout [`Layout::Auto`] picks.
    fn auto_layout(&self) -> Layout {
        let links: Vec<(usize, usize)> = self
            .ends
            .iter()
            .filter(|(a, b)| a != b)
            .map(|&(a, b)| (a as usize, b as usize))
            .collect();
        if links.is_empty() {
            return Layout::Force;
        }
        if links.len() == self.ends.len() && is_forest(self.len(), &links) {
            return Layout::Tree { root: None };
        }
        if self.directed() && links.len() == self.ends.len() && acyclic(self.len(), &links) {
            return Layout::Layered;
        }
        Layout::Force
    }

    /// Every node placed by the layout drawn, and a layered layout's bends.
    fn place(&mut self) {
        let n = self.len();
        let edges: Vec<(usize, usize)> = self
            .ends
            .iter()
            .map(|&(a, b)| (a as usize, b as usize))
            .collect();
        match self.layout.clone() {
            Layout::Auto | Layout::Force => {
                self.positions = algo::force(n, &edges);
            }
            Layout::Layered => {
                let routed = algo::layered_routes(n, &edges);
                self.positions = routed.nodes;
                self.bends = routed.bends;
            }
            Layout::Tree { root } => {
                let root = root.and_then(|r| self.index_of(&r));
                let parent = self.parents(root);
                self.positions = self.tree_positions(&parent);
            }
        }
    }

    /// A parent for each node, for a tree: `from` of the one edge into it where every node has
    /// at most one and the edges have no cycle (and no root is asked for); else a spanning
    /// forest by breadth from `root`, then from each node with no edge in, then from any node
    /// left, in node order.
    fn parents(&self, root: Option<usize>) -> Vec<Option<usize>> {
        let n = self.len();
        let links: Vec<(usize, usize)> = self
            .ends
            .iter()
            .filter(|(a, b)| a != b)
            .map(|&(a, b)| (a as usize, b as usize))
            .collect();
        if root.is_none() && self.degree_in.iter().all(|&d| d <= 1) && acyclic(n, &links) {
            let mut parent = vec![None; n];
            for &(a, b) in &links {
                parent[b] = Some(a);
            }
            return parent;
        }
        let mut parent = vec![None; n];
        let mut seen = vec![false; n];
        let starts = root
            .into_iter()
            .chain((0..n).filter(|&i| self.degree_in[i] == 0))
            .chain(0..n);
        for s in starts {
            if seen[s] {
                continue;
            }
            seen[s] = true;
            let mut q = VecDeque::from([s]);
            while let Some(x) = q.pop_front() {
                for &(y, _) in &self.adjacency[x] {
                    let y = y as usize;
                    if !seen[y] {
                        seen[y] = true;
                        parent[y] = Some(x);
                        q.push_back(y);
                    }
                }
            }
        }
        parent
    }

    /// A tidy tree ([`algo::tree`]) with each manager's leaves stacked in a column under it,
    /// where there are two or more and the tree is wide: the column is laid out as a chain, so
    /// the tidy tree keeps room for it, then drawn [`STACK_STEP`] apart and [`STACK_INDENT`]
    /// right of the manager.
    fn tree_positions(&mut self, parent: &[Option<usize>]) -> Vec<(f64, f64)> {
        let n = self.len();
        let mut children: Vec<Vec<usize>> = vec![Vec::new(); n];
        for (i, p) in parent.iter().enumerate() {
            if let Some(p) = *p {
                children[p].push(i);
            }
        }
        let leaves = children.iter().filter(|c| c.is_empty()).count();
        let deepest = depth_of(parent).into_iter().max().unwrap_or(0);
        // Stack where the tree is wider than it's deep by a good way: a short wide tree.
        let stack = leaves > 8 && leaves > 3 * (deepest + 1);
        let mut chain = parent.to_vec();
        if stack {
            for (p, kids) in children.iter().enumerate() {
                if kids.len() >= 2 && kids.iter().all(|&c| children[c].is_empty()) {
                    for w in kids.windows(2) {
                        chain[w[1]] = Some(w[0]);
                    }
                    for &c in kids {
                        self.stacked[c] = Some(p as u32);
                    }
                }
            }
        }
        let mut pos = algo::tree(n, &chain);
        for (p, kids) in children.iter().enumerate() {
            if kids.first().is_none_or(|&c| self.stacked[c].is_none()) {
                continue;
            }
            let (px, py) = pos[p];
            for (j, &c) in kids.iter().enumerate() {
                pos[c] = (px + STACK_INDENT, py - STACK_FIRST - j as f64 * STACK_STEP);
            }
        }
        pos
    }
}

/// The edges' grey at rest: the baseline's, a step towards the muted text so a graph's shape
/// reads at rest without competing with its nodes.
pub fn edge_grey(theme: &Theme) -> Color {
    crate::plot::marks::mix(theme.baseline, theme.muted, 0.18)
}

/// Each node's depth from its root by its parents (which have no cycle).
fn depth_of(parent: &[Option<usize>]) -> Vec<usize> {
    let n = parent.len();
    let mut depth = vec![usize::MAX; n];
    for i in 0..n {
        let mut chain = Vec::new();
        let mut x = i;
        let base = loop {
            if depth[x] != usize::MAX {
                break depth[x] + 1;
            }
            chain.push(x);
            match parent[x] {
                Some(p) if chain.len() <= n => x = p,
                _ => break 0,
            }
        };
        for (k, &c) in chain.iter().rev().enumerate() {
            depth[c] = base + k;
        }
    }
    depth
}

/// Each edge's lane among the edges joining the same two nodes (either way), and their count.
fn lanes_of(ends: &[(u32, u32)]) -> Vec<(u16, u16)> {
    let mut count: BTreeMap<(u32, u32), u16> = BTreeMap::new();
    let key = |&(a, b): &(u32, u32)| (a.min(b), a.max(b));
    let mut lanes = Vec::with_capacity(ends.len());
    for e in ends {
        let c = count.entry(key(e)).or_insert(0);
        lanes.push(*c);
        *c += 1;
    }
    ends.iter()
        .zip(lanes)
        .map(|(e, l)| (l, count[&key(e)]))
        .collect()
}

/// Whether undirected `links` (no self-loops) make a forest: no cycle and no two edges between
/// one pair.
fn is_forest(n: usize, links: &[(usize, usize)]) -> bool {
    let mut up: Vec<usize> = (0..n).collect();
    fn find(up: &mut [usize], mut x: usize) -> usize {
        while up[x] != x {
            up[x] = up[up[x]];
            x = up[x];
        }
        x
    }
    for &(a, b) in links {
        let (ra, rb) = (find(&mut up, a), find(&mut up, b));
        if ra == rb {
            return false;
        }
        up[ra] = rb;
    }
    true
}

/// Whether directed `links` have no cycle (Kahn's).
fn acyclic(n: usize, links: &[(usize, usize)]) -> bool {
    let mut indeg = vec![0usize; n];
    let mut out = vec![Vec::new(); n];
    for &(a, b) in links {
        out[a].push(b);
        indeg[b] += 1;
    }
    let mut ready: Vec<usize> = (0..n).filter(|&i| indeg[i] == 0).collect();
    let mut done = 0;
    while let Some(v) = ready.pop() {
        done += 1;
        for &w in &out[v] {
            indeg[w] -= 1;
            if indeg[w] == 0 {
                ready.push(w);
            }
        }
    }
    done == n
}

/// Whether a field's key is one a table's label column goes by.
fn is_name_key(k: &str) -> bool {
    ["label", "name", "title"]
        .iter()
        .any(|n| k.trim().eq_ignore_ascii_case(n))
}

/// Whether a field's key is one a table's parent column goes by.
fn is_parent_key(k: &str) -> bool {
    ["parent", "parent_id", "parentid", "parent id", "parent-id"]
        .iter()
        .any(|n| k.trim().eq_ignore_ascii_case(n))
}

/// An object's fields but `skip`, a `metadata` object's in its place.
fn fields_of(raw: &Node, skip: &[&str]) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for (k, v) in raw.as_obj().unwrap_or_default() {
        if skip.contains(&&*k.name) {
            continue;
        }
        if &*k.name == "metadata"
            && let Some(o) = v.as_obj()
        {
            for (mk, mv) in o {
                out.push((mk.name.to_string(), value_text(mv)));
            }
            continue;
        }
        out.push((k.name.to_string(), value_text(v)));
    }
    out
}

/// A JSON value as the card and the tables show it.
pub fn value_text(v: &Node) -> String {
    match &v.kind {
        Kind::Null => "null".into(),
        Kind::Bool(b) => b.to_string(),
        // As the data writes it: a port is 8080, not 8,080.
        Kind::Num(_) => v.to_text(),
        Kind::Str(s) => s.to_string(),
        Kind::Arr(a) => format!("[{}]", a.len()),
        Kind::Obj(o) => format!("{{{}}}", o.len()),
    }
}

/// The colour a spec's `fig` colour gives, for callers holding a spec colour.
pub fn spec_color(c: ColorSpec, theme: &Theme) -> Color {
    color_of(c, theme)
}
