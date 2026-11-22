//! The detail panel: what the cursor is on as a tree of its fields (a chart's point, series,
//! band or cell, a graph's node or edge), drawn beside the figure, stepped through by its own
//! keys and pointed at (phase 3). fig's information view, as a tree you can move through.
//!
//! - [`Item`]: the mark a row names, so a hovered row brings it forward and `⏎` takes the
//!   figure's cursor there;
//! - [`DetailTree`] and [`DetailRow`]: the rows, each with its depth, kind, fold and item. A
//!   chart's trees come from [`crate::plot::ChartModel::detail`]; a graph's from [`node_tree`]
//!   and [`edge_tree`] over plain [`NodeFacts`] and [`EdgeFacts`], so this file needn't know the
//!   graph's model; any JSON's from [`json_rows`];
//! - [`split`]: where the panel goes in an area (the right 31 %, 260 to 420 px; the bottom 40 %
//!   of an area under 560 px wide) and what's left for the figure, which lays out there whole;
//! - [`panel`]: the panel drawn, with a region per row and per fold triangle, and [`layout`],
//!   where its rows land;
//! - [`keys`], [`reveal`], [`scroll`] and [`toggle`]: its keys, and the pointer's ways with it.
//!
//! The panel's look is the design's m3: the card's subtle ground with a hairline on its inner
//! edge, the item's name in strong text over its kind in muted text, keys in mid grey and values
//! in the text colour in a column of tabular figures, fold triangles on branches indented 12 px
//! a level, headings in small tracked capitals, the cursor's row lit with the card's highlight,
//! long values cut with an ellipsis, a thin scrollbar when the rows overflow, and a hint of its
//! keys at the foot when there's room. Text never wears a series colour (ruling 18): a row that
//! names a mark carries its colour in a swatch.

use crate::data::SeriesKey;
use crate::interact::Cursor;
use crate::json::{Kind, Node};
use crate::plot::layout::paddings;
use crate::plot::marks::mix;
use crate::scene::{
    self, Anchor, Hit, Measure, Paint, PathData, Rect, Scene, Stroke, TextSpec, Weight,
};
use crate::style::{ColorSpec, FigStyle};
use crate::theme::{Color, Theme, color_of};
use std::collections::BTreeMap;

// ---------------------------------------------------------------- items

/// A mark a row of the panel (or a search's match) names: hovering the row brings it forward,
/// and `⏎` on it takes the figure's cursor there.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Item {
    /// A chart's series as a whole.
    Series(SeriesKey),
    /// A sample of a series: its index in the series, as [`Cursor::Point`]'s row.
    Point {
        /// The series.
        series: SeriesKey,
        /// The sample's index in the series.
        row: u32,
    },
    /// A band (a category), and one series' part of it.
    Band {
        /// The band, from 0.
        band: u32,
        /// The series of the part; None for the band whole.
        series: Option<SeriesKey>,
    },
    /// A heatmap's cell.
    Cell {
        /// Its row: the y category, from 0.
        row: u32,
        /// Its column: the x category, from 0.
        col: u32,
    },
    /// A graph's node: its index in the graph's model and its id (its identity, ruling 3).
    Node {
        /// Its index among the graph's nodes, in source order.
        index: u32,
        /// Its id.
        id: Box<str>,
    },
    /// A graph's edge: its index in the graph's model and its ends' ids.
    Edge {
        /// Its index among the graph's edges, in source order.
        index: u32,
        /// The id of the node it's from.
        from: Box<str>,
        /// The id of the node it's to.
        to: Box<str>,
    },
}

impl Item {
    /// The item a cursor is on. None for a node: a cursor names it by id alone, and an item
    /// carries its index too (the graph's model has it).
    pub fn of_cursor(c: &Cursor) -> Option<Item> {
        match c {
            Cursor::Point { series, row } => Some(Item::Point {
                series: series.clone(),
                row: *row,
            }),
            Cursor::Band { band, series } => Some(Item::Band {
                band: *band,
                series: series.clone(),
            }),
            Cursor::Cell { row, col } => Some(Item::Cell {
                row: *row,
                col: *col,
            }),
            Cursor::Node(_) => None,
        }
    }

    /// Where the figure's cursor goes for it, when it names a place a cursor can be: a point, a
    /// band, a cell or a node. A series needs its chart
    /// ([`crate::plot::ChartModel::cursor_of`]) and an edge its graph (the nearer end): None.
    pub fn cursor(&self) -> Option<Cursor> {
        match self {
            Item::Point { series, row } => Some(Cursor::Point {
                series: series.clone(),
                row: *row,
            }),
            Item::Band { band, series } => Some(Cursor::Band {
                band: *band,
                series: series.clone(),
            }),
            Item::Cell { row, col } => Some(Cursor::Cell {
                row: *row,
                col: *col,
            }),
            Item::Node { id, .. } => Some(Cursor::Node(id.clone())),
            Item::Series(_) | Item::Edge { .. } => None,
        }
    }

    /// The series it's of, for emphasis: a series, a point's, a part's.
    pub fn series(&self) -> Option<&SeriesKey> {
        match self {
            Item::Series(k) | Item::Point { series: k, .. } => Some(k),
            Item::Band { series, .. } => series.as_ref(),
            _ => None,
        }
    }

    /// Whether it and `other` name one mark: as `==`, but a node by its id alone and an edge
    /// by its index alone, so an item made from a cursor (which has no index) still finds it.
    pub fn same(&self, other: &Item) -> bool {
        match (self, other) {
            (Item::Node { id: a, .. }, Item::Node { id: b, .. }) => a == b,
            (Item::Edge { index: a, .. }, Item::Edge { index: b, .. }) => a == b,
            _ => self == other,
        }
    }

    /// Whether it names the mark the cursor `c` is on, or one holding it: a series holds its
    /// points and parts, a band whole its parts, a node is named by its id.
    pub fn holds(&self, c: &Cursor) -> bool {
        match (self, c) {
            (Item::Series(k), Cursor::Point { series, .. }) => k == series,
            (Item::Series(k), Cursor::Band { series, .. }) => series.as_ref() == Some(k),
            (Item::Band { band, series: None }, Cursor::Band { band: b, .. }) => band == b,
            (Item::Node { id, .. }, Cursor::Node(n)) => id == n,
            _ => Item::of_cursor(c).is_some_and(|i| &i == self),
        }
    }
}

// ---------------------------------------------------------------- rows and trees

/// The shape of a row's swatch, as the legend draws its keys.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shape {
    /// A short line: a line series.
    Line,
    /// A small rounded square: bars and areas.
    Square,
    /// A dot: points and nodes.
    Dot,
}

/// A row's colour key: the colour of the mark it names, resolved against the theme when it's
/// drawn (so a theme change needs no new tree), and its shape. [`ColorSpec::Dim`] is the
/// neutral that series 9 and up wear (ruling 4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Swatch {
    /// The colour.
    pub color: ColorSpec,
    /// The shape.
    pub shape: Shape,
}

impl Swatch {
    /// A dot in a colour.
    pub fn dot(color: ColorSpec) -> Swatch {
        Swatch {
            color,
            shape: Shape::Dot,
        }
    }

    /// Its colour under a theme.
    pub fn color(&self, theme: &Theme) -> Color {
        color_of(self.color, theme)
    }
}

/// What a row of the panel is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowKind {
    /// A heading over the rows after it, up to the next heading: "NEIGHBOURS". Never folds and
    /// never takes the panel's cursor.
    Section,
    /// A key and its value, the value in the column.
    Field,
    /// A key whose value is an object or an array, summed up (`{3}`, `[2]`): it folds, and its
    /// members are the rows after it a level deeper.
    Branch,
    /// A row that names a mark: its swatch, the mark's name as the key (in the text colour),
    /// and a value at the right (a series' value there, a neighbour's edges).
    Mark(Option<Swatch>),
    /// The sum of the marks' values above it: its key muted, its value at the right where
    /// theirs are, under a hairline.
    Sum,
}

/// One row of a [`DetailTree`].
#[derive(Clone, Debug, PartialEq)]
pub struct DetailRow {
    /// How deep it is: 0 at the top, one more under each branch.
    pub depth: u16,
    /// Its key, or a heading's title, or a mark's name.
    pub key: String,
    /// Its value as shown, on one line.
    pub value: String,
    /// What it is.
    pub kind: RowKind,
    /// The mark it names, if it names one.
    pub item: Option<Item>,
    /// A branch folded: its members hidden.
    pub folded: bool,
    /// Its value in the muted ink: a note ("6 edges"), "no data", a JSON null.
    pub quiet: bool,
    /// What `y` yanks when it isn't the value as shown: a branch's JSON, a value's text
    /// before it was put on one line, a node's id.
    pub copy: Option<String>,
}

impl DetailRow {
    /// A field: a key and its value.
    pub fn field(key: impl Into<String>, value: impl Into<String>) -> DetailRow {
        DetailRow {
            depth: 0,
            key: key.into(),
            value: value.into(),
            kind: RowKind::Field,
            item: None,
            folded: false,
            quiet: false,
            copy: None,
        }
    }

    /// A sum of the mark rows above it: "total".
    pub fn sum(key: impl Into<String>, value: impl Into<String>) -> DetailRow {
        DetailRow {
            kind: RowKind::Sum,
            ..DetailRow::field(key, value)
        }
    }

    /// A heading.
    pub fn section(title: impl Into<String>) -> DetailRow {
        DetailRow {
            kind: RowKind::Section,
            ..DetailRow::field(title, "")
        }
    }

    /// A branch with a summary value (`{3}`), folded.
    pub fn branch(key: impl Into<String>, summary: impl Into<String>) -> DetailRow {
        DetailRow {
            kind: RowKind::Branch,
            folded: true,
            ..DetailRow::field(key, summary)
        }
    }

    /// A row that names a mark.
    pub fn mark(
        name: impl Into<String>,
        value: impl Into<String>,
        swatch: Option<Swatch>,
        item: Item,
    ) -> DetailRow {
        DetailRow {
            kind: RowKind::Mark(swatch),
            item: Some(item),
            ..DetailRow::field(name, value)
        }
    }

    /// The same, `depth` deep.
    pub fn at(self, depth: u16) -> DetailRow {
        DetailRow { depth, ..self }
    }

    /// The same, its value in the muted ink.
    pub fn quiet(self) -> DetailRow {
        DetailRow {
            quiet: true,
            ..self
        }
    }

    /// The same, naming a mark.
    pub fn naming(self, item: Item) -> DetailRow {
        DetailRow {
            item: Some(item),
            ..self
        }
    }

    /// The same, with what `y` yanks.
    pub fn copying(self, text: impl Into<String>) -> DetailRow {
        DetailRow {
            copy: Some(text.into()),
            ..self
        }
    }

    /// What `y` yanks: [`DetailRow::copy`], else the value as shown (a mark's name where it
    /// has no value).
    pub fn yank(&self) -> String {
        match (&self.copy, self.kind) {
            (Some(c), _) => c.clone(),
            (None, RowKind::Mark(_)) if self.value.is_empty() => self.key.clone(),
            (None, _) => self.value.clone(),
        }
    }

    /// What `Y` yanks: `key: value`, the value as `y` has it.
    pub fn pair(&self) -> String {
        format!("{}: {}", self.key, self.yank())
    }
}

/// What the cursor is on, as the panel shows it: a title, a kind line, and rows.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct DetailTree {
    /// The item's name: the panel's title, in strong text.
    pub title: String,
    /// What it is, under the title in muted text: "platform · degree 5", "api · point 54 of
    /// 73", "line series".
    pub kind: String,
    /// The swatch before the kind: the colour of the mark the tree is about.
    pub swatch: Option<Swatch>,
    /// The mark the tree is about.
    pub item: Option<Item>,
    /// The rows, in order: a branch's members follow it a level deeper.
    pub rows: Vec<DetailRow>,
}

impl DetailTree {
    /// A tree with a title and a kind, and no rows yet.
    pub fn new(title: impl Into<String>, kind: impl Into<String>) -> DetailTree {
        DetailTree {
            title: title.into(),
            kind: kind.into(),
            ..DetailTree::default()
        }
    }

    /// The rows shown, by index: every row but the members of a folded branch.
    pub fn lines(&self) -> Vec<usize> {
        let mut out = Vec::with_capacity(self.rows.len());
        let mut hide: Option<u16> = None;
        for (i, r) in self.rows.iter().enumerate() {
            if let Some(d) = hide {
                if r.depth > d && r.kind != RowKind::Section {
                    continue;
                }
                hide = None;
            }
            out.push(i);
            if r.kind == RowKind::Branch && r.folded {
                hide = Some(r.depth);
            }
        }
        out
    }

    /// The branch a row is a member of, if it's under one.
    pub fn parent(&self, i: usize) -> Option<usize> {
        let d = self.rows.get(i)?.depth;
        if d == 0 {
            return None;
        }
        (0..i).rev().find(|&j| self.rows[j].depth < d)
    }

    /// The index past a row's members (its own index + 1 for a row with none).
    pub fn end_of(&self, i: usize) -> usize {
        let Some(r) = self.rows.get(i) else {
            return i;
        };
        let mut j = i + 1;
        while j < self.rows.len()
            && self.rows[j].depth > r.depth
            && self.rows[j].kind != RowKind::Section
        {
            j += 1;
        }
        j
    }

    /// The first row that names `item` (as [`Item::same`] has it).
    pub fn row_of(&self, item: &Item) -> Option<usize> {
        self.rows
            .iter()
            .position(|r| r.item.as_ref().is_some_and(|i| i.same(item)))
    }

    /// The first row the panel's cursor can be on: the first that isn't a heading.
    pub fn first_row(&self) -> Option<usize> {
        self.lines()
            .into_iter()
            .find(|&i| self.rows[i].kind != RowKind::Section)
    }

    /// Unfolds every branch.
    pub fn unfold_all(&mut self) {
        for r in &mut self.rows {
            r.folded = false;
        }
    }

    /// The row `row` is shown as: itself, or the branch whose fold hides it.
    fn shown_as(&self, row: usize) -> usize {
        let mut r = row.min(self.rows.len().saturating_sub(1));
        let mut hidden_by = None;
        let mut at = r;
        while let Some(p) = self.parent(at) {
            if self.rows[p].folded {
                hidden_by = Some(p);
            }
            at = p;
        }
        if let Some(p) = hidden_by {
            r = p;
        }
        r
    }
}

// ---------------------------------------------------------------- JSON

/// The most members of one object or array a tree lists; the rest are a quiet "+N more".
pub const MEMBERS: usize = 500;

/// The deepest a tree nests JSON; anything deeper is summed up and not opened.
pub const DEPTH: u16 = 12;

/// A JSON scalar as the panel writes it: a string as itself on one line, a number, `true`,
/// `false` and `null` as JSON writes them; an object or array summed up (`{3}`, `[2]`).
pub fn scalar_text(v: &Node) -> String {
    match &v.kind {
        Kind::Str(s) => one_line(s),
        Kind::Obj(m) => format!("{{{}}}", m.len()),
        Kind::Arr(a) => format!("[{}]", a.len()),
        _ => v.to_text(),
    }
}

/// Text on one line: tabs and line breaks as spaces.
fn one_line(s: &str) -> String {
    if s.contains(['\n', '\r', '\t']) {
        s.replace("\r\n", " ").replace(['\n', '\r', '\t'], " ")
    } else {
        s.to_string()
    }
}

/// Rows for a JSON member at `depth`: a scalar as a field, an object or an array with members
/// as a folded branch whose members follow a level deeper (an array's keyed by index).
pub fn json_rows(key: &str, v: &Node, depth: u16, out: &mut Vec<DetailRow>) {
    let members: Vec<(String, &Node)> = match &v.kind {
        Kind::Obj(m) => m.iter().map(|(k, n)| (k.name.to_string(), n)).collect(),
        Kind::Arr(a) => a
            .iter()
            .enumerate()
            .map(|(i, n)| (i.to_string(), n))
            .collect(),
        _ => Vec::new(),
    };
    if members.is_empty() {
        let mut row = DetailRow::field(key, scalar_text(v)).at(depth);
        if v.is_null() {
            row = row.quiet();
        }
        if let Kind::Str(s) = &v.kind
            && row.value != **s
        {
            row = row.copying(s.to_string());
        }
        out.push(row);
        return;
    }
    out.push(
        DetailRow::branch(key, scalar_text(v))
            .at(depth)
            .copying(v.to_text()),
    );
    if depth >= DEPTH {
        return;
    }
    for (k, n) in members.iter().take(MEMBERS) {
        json_rows(k, n, depth + 1, out);
    }
    if members.len() > MEMBERS {
        out.push(
            DetailRow::field(format!("+{} more", members.len() - MEMBERS), "")
                .at(depth + 1)
                .quiet(),
        );
    }
}

/// Rows for an object's members at `depth`, `skip`ped keys left out, scalars first then the
/// members with members of their own (as fig's info view lists a node's metadata).
pub fn object_rows(obj: &Node, skip: &[&str], depth: u16, out: &mut Vec<DetailRow>) {
    let Some(members) = obj.as_obj() else {
        return;
    };
    let kept: Vec<&(crate::json::Key, Node)> = members
        .iter()
        .filter(|(k, _)| !skip.contains(&&*k.name))
        .collect();
    let nested = |n: &Node| match &n.kind {
        Kind::Obj(m) => !m.is_empty(),
        Kind::Arr(a) => !a.is_empty(),
        _ => false,
    };
    for (k, n) in kept.iter().filter(|(_, n)| !nested(n)) {
        json_rows(&k.name, n, depth, out);
    }
    for (k, n) in kept.iter().filter(|(_, n)| nested(n)) {
        json_rows(&k.name, n, depth, out);
    }
}

/// A `fig` block's settings as rows, only those it sets.
pub fn style_rows(st: &FigStyle, out: &mut Vec<DetailRow>) {
    if let Some(c) = st.color {
        out.push(DetailRow::field("color", c.name()));
    }
    if let Some(s) = st.size {
        out.push(DetailRow::field("size", s.to_string()));
    }
    if let Some(l) = &st.label {
        out.push(DetailRow::field("label", one_line(l)).copying(l.clone()));
    }
    if let Some(s) = st.show_label {
        out.push(DetailRow::field("show-label", s.to_string()));
    }
    for (on, name) in [
        (st.hidden, "hidden"),
        (st.dash, "dash"),
        (st.override_focus, "override-focus"),
    ] {
        if on {
            out.push(DetailRow::field(name, "true"));
        }
    }
}

// ---------------------------------------------------------------- graph trees

/// A node or edge's end as a row names it: its index and id, its name as shown, its colour,
/// and its degree for the note ("6 edges").
#[derive(Clone, Debug, PartialEq)]
pub struct Neighbour {
    /// Its index among the graph's nodes.
    pub index: u32,
    /// Its id.
    pub id: Box<str>,
    /// Its name as shown: its label, else its id.
    pub name: String,
    /// Its colour.
    pub swatch: Option<Swatch>,
    /// How many edges it has.
    pub degree: u32,
}

impl Neighbour {
    /// The item it is.
    pub fn item(&self) -> Item {
        Item::Node {
            index: self.index,
            id: self.id.clone(),
        }
    }

    /// Its row: its swatch and name, its edges as a quiet note, naming it; `y` yanks its id.
    pub fn row(&self) -> DetailRow {
        DetailRow::mark(
            one_line(&self.name),
            edges(self.degree),
            self.swatch,
            self.item(),
        )
        .quiet()
        .copying(self.id.to_string())
    }
}

/// "1 edge", "6 edges".
fn edges(n: u32) -> String {
    if n == 1 {
        "1 edge".into()
    } else {
        format!("{n} edges")
    }
}

/// What a node's tree is built from: plain values, so this file needn't know the graph's
/// model (P3-graph's `GraphModel` fills one in).
#[derive(Clone, Debug, PartialEq)]
pub struct NodeFacts<'a> {
    /// Its index among the graph's nodes.
    pub index: u32,
    /// Its id.
    pub id: &'a str,
    /// Its label, if it has one.
    pub label: Option<&'a str>,
    /// Its group, if it has one.
    pub group: Option<&'a str>,
    /// Its colour.
    pub swatch: Option<Swatch>,
    /// The edges into it (on an undirected graph, the edges it's the `to` of).
    pub degree_in: u32,
    /// The edges out of it (on an undirected graph, the edges it's the `from` of).
    pub degree_out: u32,
    /// Whether the graph's edges have a direction: in and out are then told apart.
    pub directed: bool,
    /// Its neighbours, in the order the panel lists them; one row each, a repeat dropped.
    pub neighbours: Vec<Neighbour>,
    /// Its source object, every field of which the tree shows (`id`, `label`, `group` and
    /// `fig` in their own rows, `metadata`'s members under their own heading).
    pub fields: Option<&'a Node>,
}

/// What an edge's tree is built from.
#[derive(Clone, Debug, PartialEq)]
pub struct EdgeFacts<'a> {
    /// Its index among the graph's edges.
    pub index: u32,
    /// The node it's from.
    pub from: Neighbour,
    /// The node it's to.
    pub to: Neighbour,
    /// Its label, if it has one.
    pub label: Option<&'a str>,
    /// Its weight, if it has one.
    pub weight: Option<f64>,
    /// Whether it has a direction.
    pub directed: bool,
    /// Its source object.
    pub fields: Option<&'a Node>,
}

/// A JSON number as a node's field shows it: as JSON writes it, an integer without a point.
fn json_number(v: f64) -> String {
    crate::json::Node {
        kind: Kind::Num(v),
        at: crate::error::Pos::default(),
    }
    .to_text()
}

/// The rows a graph item's source object gives: its own scalars and nested values (but
/// `skip`) into `rows`; `metadata`'s members under their own heading, then its `fig` block's
/// under another, into `tail`.
fn source_rows(
    fields: Option<&Node>,
    skip: &[&str],
    rows: &mut Vec<DetailRow>,
    tail: &mut Vec<DetailRow>,
) {
    let Some(f) = fields else {
        return;
    };
    let mut all: Vec<&str> = skip.to_vec();
    all.extend(["fig", "metadata"]);
    object_rows(f, &all, 0, rows);
    if let Some(meta) = f.get("metadata") {
        tail.push(DetailRow::section("Metadata"));
        match &meta.kind {
            Kind::Obj(_) => object_rows(meta, &[], 0, tail),
            Kind::Arr(a) => {
                for (i, n) in a.iter().enumerate().take(MEMBERS) {
                    json_rows(&i.to_string(), n, 0, tail);
                }
            }
            _ => json_rows("value", meta, 0, tail),
        }
        if tail.len() == 1 {
            tail.clear();
        }
    }
    if let Some(fig) = f
        .get("fig")
        .filter(|n| n.as_obj().is_some_and(|m| !m.is_empty()))
    {
        tail.push(DetailRow::section("fig"));
        object_rows(fig, &[], 0, tail);
    }
}

/// A node's tree: its id, label, group, degree in and out and its scalar fields; its
/// neighbours under a heading, each naming it; its `metadata` under another, nested objects
/// as folded branches. Titled by its label (else its id), its kind line its group and degree.
pub fn node_tree(n: &NodeFacts<'_>) -> DetailTree {
    let degree = n.degree_in + n.degree_out;
    let kind = match n.group {
        Some(g) => format!("{} · degree {degree}", one_line(g)),
        None => format!("node · degree {degree}"),
    };
    let mut t = DetailTree::new(one_line(n.label.unwrap_or(n.id)), kind);
    t.swatch = n.swatch;
    t.item = Some(Item::Node {
        index: n.index,
        id: n.id.into(),
    });
    let mut rows = vec![DetailRow::field("id", one_line(n.id)).copying(n.id)];
    if let Some(l) = n.label {
        rows.push(DetailRow::field("label", one_line(l)).copying(l));
    }
    if let Some(g) = n.group {
        rows.push(DetailRow::field("group", one_line(g)).copying(g));
    }
    rows.push(DetailRow::field(
        "degree",
        if n.directed {
            format!("{degree} · {} in, {} out", n.degree_in, n.degree_out)
        } else {
            degree.to_string()
        },
    ));
    let mut tail = Vec::new();
    source_rows(n.fields, &["id", "label", "group"], &mut rows, &mut tail);
    let mut seen: Vec<u32> = Vec::new();
    let neighbours: Vec<DetailRow> = n
        .neighbours
        .iter()
        .filter(|nb| {
            let new = !seen.contains(&nb.index);
            seen.push(nb.index);
            new
        })
        .map(Neighbour::row)
        .collect();
    if !neighbours.is_empty() {
        rows.push(DetailRow::section("Neighbours"));
        rows.extend(neighbours);
    }
    rows.extend(tail);
    t.rows = rows;
    t
}

/// An edge's tree: its label, weight and scalar fields; its ends under a heading, each naming
/// its node; its `metadata` under another. Titled `a → b` (`a — b` undirected).
pub fn edge_tree(e: &EdgeFacts<'_>) -> DetailTree {
    let arrow = if e.directed { "→" } else { "—" };
    let title = format!(
        "{} {arrow} {}",
        one_line(&e.from.name),
        one_line(&e.to.name)
    );
    let kind = match (e.directed, e.label) {
        (true, Some(l)) => format!("directed edge · {}", one_line(l)),
        (false, Some(l)) => format!("edge · {}", one_line(l)),
        (true, None) => "directed edge".to_string(),
        (false, None) => "edge".to_string(),
    };
    let mut t = DetailTree::new(title, kind);
    t.item = Some(Item::Edge {
        index: e.index,
        from: e.from.id.clone(),
        to: e.to.id.clone(),
    });
    let mut rows = Vec::new();
    if let Some(l) = e.label {
        rows.push(DetailRow::field("label", one_line(l)).copying(l));
    }
    if let Some(w) = e.weight {
        rows.push(DetailRow::field("weight", json_number(w)));
    }
    let mut tail = Vec::new();
    source_rows(
        e.fields,
        &[
            "from", "to", "source", "target", "label", "weight", "directed",
        ],
        &mut rows,
        &mut tail,
    );
    rows.push(DetailRow::section("Ends"));
    rows.push(e.from.row());
    rows.push(e.to.row());
    rows.extend(tail);
    t.rows = rows;
    t
}

// ---------------------------------------------------------------- where it goes

/// The share of an area's width the panel takes at its right.
pub const SHARE: f32 = 0.31;
/// The narrowest and widest the panel is at the right.
pub const WIDTH: (f32, f32) = (260.0, 420.0);
/// An area narrower than this has the panel at its bottom instead.
pub const NARROW: f32 = 560.0;
/// The share of an area's height the panel takes at its bottom.
pub const BOTTOM_SHARE: f32 = 0.4;

/// Which edge of the area the panel takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dock {
    /// The right: about 31 % of the width, 260 to 420 px.
    Right,
    /// The bottom 40 %, in an area under 560 px wide.
    Bottom,
}

/// Where the panel goes in an area of `size` and what's left for the figure, which lays out
/// in it whole (nothing hides under the panel): the figure's rectangle, the panel's, and which
/// edge it took. Whole px.
pub fn split(size: (f32, f32)) -> (Rect, Rect, Dock) {
    let (w, h) = size;
    if w < NARROW {
        let ph = (h * BOTTOM_SHARE).round();
        (
            Rect::new(0.0, 0.0, w, h - ph),
            Rect::new(0.0, h - ph, w, ph),
            Dock::Bottom,
        )
    } else {
        let pw = (w * SHARE).clamp(WIDTH.0, WIDTH.1).round();
        (
            Rect::new(0.0, 0.0, w - pw, h),
            Rect::new(w - pw, 0.0, pw, h),
            Dock::Right,
        )
    }
}

/// The edge a panel's rectangle is docked to in its area: the bottom when it starts below the
/// top at the left edge, else the right.
fn dock_of(rect: Rect) -> Dock {
    if rect.y > 0.0 && rect.x <= 0.0 {
        Dock::Bottom
    } else {
        Dock::Right
    }
}

// ---------------------------------------------------------------- state and keys

/// The panel's state, which the surface keeps per window.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct PanelState {
    /// The first line shown in the body, among the tree's lines ([`DetailTree::lines`]).
    pub top: usize,
    /// The panel's cursor: a row's index in [`DetailTree::rows`] (it stays on its row as
    /// branches fold).
    pub row: usize,
    /// Whether the panel has the keyboard (Tab moved focus into it).
    pub focused: bool,
}

impl PanelState {
    /// A state for a tree just opened: at the top, the cursor on the row naming `item` if one
    /// does (the figure's cursor's mark), else on its first row.
    pub fn open(tree: &DetailTree, item: Option<&Item>) -> PanelState {
        let row = item
            .and_then(|i| tree.row_of(i))
            .or_else(|| tree.first_row())
            .unwrap_or(0);
        PanelState {
            top: 0,
            row,
            focused: false,
        }
    }
}

/// A key the panel takes when it has focus.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PanelKey {
    /// `j`: the next row.
    Down,
    /// `k`: the row before.
    Up,
    /// `h`: fold an open branch, else go to the branch above.
    Fold,
    /// `l`: unfold a folded branch, else go into an open one.
    Unfold,
    /// `gg`: the first row.
    First,
    /// `G`: the last row.
    Last,
    /// `y`: yank the value.
    Yank,
    /// `Y`: yank `key: value`.
    YankPair,
    /// `⏎`: on a row that names a mark, the figure's cursor goes to it; on a branch, it folds
    /// or unfolds.
    Enter,
}

impl PanelKey {
    /// A key by its name as a keymap writes it: `j`, `k`, `h`, `l`, `gg`, `G`, `y`, `Y`,
    /// `<CR>`, and the arrows.
    pub fn from_name(name: &str) -> Option<PanelKey> {
        Some(match name {
            "j" | "<Down>" => PanelKey::Down,
            "k" | "<Up>" => PanelKey::Up,
            "h" | "<Left>" => PanelKey::Fold,
            "l" | "<Right>" => PanelKey::Unfold,
            "gg" | "<Home>" => PanelKey::First,
            "G" | "<End>" => PanelKey::Last,
            "y" => PanelKey::Yank,
            "Y" => PanelKey::YankPair,
            "<CR>" | "<Enter>" | "<Return>" => PanelKey::Enter,
            _ => return None,
        })
    }
}

/// What a key did, for the surface to finish.
#[derive(Clone, Debug, PartialEq)]
pub enum PanelAction {
    /// Nothing: `j` on the last row, `y` on an empty tree.
    None,
    /// The panel's cursor moved: [`reveal`] it.
    Moved,
    /// A branch folded or unfolded (the cursor kept on a shown row): [`reveal`] it.
    Folded,
    /// Text for the register, from `y` or `Y`.
    Yank(String),
    /// `⏎` on a row that names a mark: the figure's cursor goes to it.
    Jump(Item),
}

/// Whether the panel's cursor can rest on a row: any but a heading.
fn restable(r: &DetailRow) -> bool {
    r.kind != RowKind::Section
}

/// The panel's keys. `j` `k` step over the rows shown (headings skipped), `h` folds an open
/// branch or goes up to the branch a row is in, `l` unfolds a folded branch or goes into an
/// open one, `gg` `G` go to the first and last row, `y` yanks the value (a branch's JSON),
/// `Y` `key: value`, and `⏎` jumps to the mark a row names, or folds a branch. After
/// [`PanelAction::Moved`] or [`PanelAction::Folded`], call [`reveal`].
pub fn keys(tree: &mut DetailTree, state: &mut PanelState, key: PanelKey) -> PanelAction {
    let lines = tree.lines();
    let rest: Vec<usize> = lines
        .iter()
        .copied()
        .filter(|&i| restable(&tree.rows[i]))
        .collect();
    let Some(&first) = rest.first() else {
        return PanelAction::None;
    };
    let last = *rest.last().unwrap_or(&first);
    let at = tree.shown_as(state.row);
    let at = if restable(&tree.rows[at]) {
        at
    } else {
        rest.iter().copied().find(|&i| i > at).unwrap_or(last)
    };
    let go = |state: &mut PanelState, to: usize| {
        if state.row == to {
            PanelAction::None
        } else {
            state.row = to;
            PanelAction::Moved
        }
    };
    let row = &tree.rows[at];
    match key {
        PanelKey::Down => {
            let to = rest.iter().copied().find(|&i| i > at).unwrap_or(at);
            go(state, to)
        }
        PanelKey::Up => {
            let to = rest.iter().rev().copied().find(|&i| i < at).unwrap_or(at);
            go(state, to)
        }
        PanelKey::First => go(state, first),
        PanelKey::Last => go(state, last),
        PanelKey::Fold => {
            if row.kind == RowKind::Branch && !row.folded {
                tree.rows[at].folded = true;
                state.row = at;
                PanelAction::Folded
            } else {
                match tree.parent(at) {
                    Some(p) => go(state, p),
                    None => go(state, at),
                }
            }
        }
        PanelKey::Unfold => {
            if row.kind == RowKind::Branch && row.folded {
                tree.rows[at].folded = false;
                state.row = at;
                PanelAction::Folded
            } else if row.kind == RowKind::Branch && tree.end_of(at) > at + 1 {
                go(state, at + 1)
            } else {
                go(state, at)
            }
        }
        PanelKey::Yank => PanelAction::Yank(row.yank()),
        PanelKey::YankPair => PanelAction::Yank(row.pair()),
        PanelKey::Enter => match (&row.item, row.kind) {
            (Some(item), _) => PanelAction::Jump(item.clone()),
            (None, RowKind::Branch) => {
                toggle(tree, state, at);
                PanelAction::Folded
            }
            _ => {
                state.row = at;
                PanelAction::None
            }
        },
    }
}

/// Folds or unfolds a branch (a click on its triangle, a double-click on it). The panel's
/// cursor, if the fold hides it, goes to the branch. False for a row that isn't a branch.
pub fn toggle(tree: &mut DetailTree, state: &mut PanelState, row: usize) -> bool {
    let Some(r) = tree.rows.get_mut(row) else {
        return false;
    };
    if r.kind != RowKind::Branch {
        return false;
    }
    r.folded = !r.folded;
    state.row = tree.shown_as(state.row);
    true
}

/// Scrolls `top` so the panel's cursor's row is shown whole in a panel at `rect`, moving as
/// little as it can, and keeps `top` in range.
pub fn reveal(tree: &DetailTree, state: &mut PanelState, rect: Rect) {
    let l = layout(tree, state, rect);
    let lines = &l.lines;
    let at = tree.shown_as(state.row);
    let Some(line) = lines.iter().position(|&i| i == at) else {
        state.top = l.top;
        return;
    };
    let mut top = state.top.min(l.max_top);
    if line < top {
        top = line;
    }
    while top < line && !fits(tree, lines, top, line, l.body.h) {
        top += 1;
    }
    // A heading over the cursor's row comes into view with it where there's room.
    if top == line
        && line > 0
        && tree.rows[lines[line - 1]].kind == RowKind::Section
        && fits(tree, lines, line - 1, line, l.body.h)
    {
        top = line - 1;
    }
    state.top = top.min(l.max_top);
}

/// Scrolls the panel by `lines` (down when positive), within range: the wheel. The cursor
/// stays on its row, which may scroll out of view.
pub fn scroll(tree: &DetailTree, state: &mut PanelState, rect: Rect, lines: i32) {
    let l = layout(tree, state, rect);
    let top = l.top as i64 + i64::from(lines);
    state.top = top.clamp(0, l.max_top as i64) as usize;
}

/// Whether the lines from `from` to `to` (both shown) fit in `h`.
fn fits(tree: &DetailTree, lines: &[usize], from: usize, to: usize, h: f32) -> bool {
    let total: f32 = lines[from..=to]
        .iter()
        .map(|&i| line_height(&tree.rows[i]))
        .sum();
    total <= h + 0.01
}

// ---------------------------------------------------------------- layout

/// A row's height.
pub const ROW_H: f32 = 22.0;
/// A heading's: its row and the room above it.
pub const SECTION_H: f32 = 34.0;
/// The panel's padding at the sides.
pub const PAD_X: f32 = 20.0;
/// How far each level of a branch indents its members.
pub const INDENT: f32 = 12.0;
/// The room a fold triangle takes before a key.
const GUTTER: f32 = 14.0;
/// The header's height under the top padding: title, kind line, the rule, and the gap under.
const HEAD: f32 = 56.0;
/// The header's height without its kind line (a short panel).
const HEAD_SHORT: f32 = 36.0;
/// A panel shorter than this drops its kind line.
const SHORT: f32 = 140.0;
/// The hint's height, and the body's room under the rows before it's shown.
const HINT_H: f32 = 18.0;
const HINT_ROOM: f32 = 8.0 * ROW_H;
/// The title's, the rows' and the headings' sizes.
const TITLE: f32 = 15.0;
const BODY: f32 = 12.0;
const NOTE: f32 = 11.0;
const HEADING: f32 = 10.5;
/// A heading's tracking, px between its capitals.
const TRACKING: f32 = 0.8;

fn line_height(r: &DetailRow) -> f32 {
    if r.kind == RowKind::Section {
        SECTION_H
    } else {
        ROW_H
    }
}

/// Where a panel's pieces land in its rectangle, from its tree and state.
#[derive(Clone, Debug, PartialEq)]
pub struct PanelLayout {
    /// The panel.
    pub rect: Rect,
    /// The edge it's docked to.
    pub dock: Dock,
    /// The content's left and right, inside the padding.
    pub inner: (f32, f32),
    /// The top padding: the title's cap sits under it.
    pub pad_top: f32,
    /// Whether the kind line is shown (a panel under 140 px drops it).
    pub kind: bool,
    /// The rule under the header.
    pub rule_y: f32,
    /// The rows' scrolling body.
    pub body: Rect,
    /// The tree's lines ([`DetailTree::lines`]).
    pub lines: Vec<usize>,
    /// The first line shown: the state's, within range.
    pub top: usize,
    /// The greatest `top` that still fills the body.
    pub max_top: usize,
    /// The rows shown whole, each its index in the tree and its line's rectangle (a heading's
    /// takes in the room above it), top to bottom.
    pub shown: Vec<(usize, Rect)>,
    /// The hint at the foot, when there's room for it.
    pub hint: Option<Rect>,
}

impl PanelLayout {
    /// The row under a point: its index in the tree.
    pub fn row_at(&self, x: f32, y: f32) -> Option<usize> {
        self.shown
            .iter()
            .find(|(_, r)| r.contains(x, y))
            .map(|(i, _)| *i)
    }
}

/// Where a panel's pieces land: no text is measured, so the surface can scroll and reveal
/// without fonts.
pub fn layout(tree: &DetailTree, state: &PanelState, rect: Rect) -> PanelLayout {
    let dock = dock_of(rect);
    let (pad_top, pad_bottom) = match dock {
        Dock::Right => {
            let (_, t, b) = paddings(rect.w, rect.h);
            (t, b)
        }
        // A short strip at the bottom keeps its rows, not its margins.
        Dock::Bottom => (
            (rect.h * 0.08).clamp(8.0, 14.0),
            (rect.h * 0.06).clamp(6.0, 12.0),
        ),
    };
    let kind = rect.h >= SHORT;
    let head = if kind { HEAD } else { HEAD_SHORT };
    let rule_y = (rect.y + pad_top + head - 6.0).round() + 0.5;
    let body_top = rect.y + pad_top + head;
    let mut body_bottom = rect.bottom() - pad_bottom;
    let hint = if body_bottom - body_top >= HINT_ROOM + HINT_H + 12.0 {
        let y = (rect.bottom() - pad_bottom - HINT_H).round();
        body_bottom = y - 12.0;
        Some(Rect::new(rect.x + PAD_X, y, rect.w - 2.0 * PAD_X, HINT_H))
    } else {
        None
    };
    let body = Rect::new(rect.x, body_top, rect.w, (body_bottom - body_top).max(0.0));
    let lines = tree.lines();
    // The greatest top: the first line from which the rest fit.
    let mut max_top = lines.len();
    let mut used = 0.0;
    while max_top > 0 {
        let h = line_height(&tree.rows[lines[max_top - 1]]);
        if used + h > body.h + 0.01 {
            break;
        }
        used += h;
        max_top -= 1;
    }
    let max_top = max_top.min(lines.len().saturating_sub(1));
    let top = state.top.min(max_top);
    let (x0, x1) = (rect.x + PAD_X, rect.right() - PAD_X);
    let mut shown = Vec::new();
    let mut y = body.y;
    for &i in lines.iter().skip(top) {
        let h = line_height(&tree.rows[i]);
        if y + h > body.bottom() + 0.01 {
            break;
        }
        shown.push((i, Rect::new(x0 - 8.0, y, x1 - x0 + 14.0, h)));
        y += h;
    }
    // A heading with none of its rows under it in view waits for them.
    if shown.len() > 1
        && let Some(&(last, _)) = shown.last()
        && tree.rows[last].kind == RowKind::Section
        && lines.last() != Some(&last)
    {
        shown.pop();
    }
    PanelLayout {
        rect,
        dock,
        inner: (x0, x1),
        pad_top,
        kind,
        rule_y,
        body,
        lines,
        top,
        max_top,
        shown,
        hint,
    }
}

// ---------------------------------------------------------------- the look

/// The panel's ground: the card's subtle ground, a step off the surface (lighter on dark,
/// darker on light, as GitHub's subtle canvas).
pub fn ground(theme: &Theme) -> Color {
    if theme.dark {
        mix(theme.surface, theme.tooltip, 0.4)
    } else {
        mix(theme.surface, theme.grid, 0.3)
    }
}

/// The hairline on the panel's inner edge and under its header.
fn rule_color(theme: &Theme) -> Color {
    mix(theme.grid, theme.baseline, 0.4)
}

/// The cursor's row's ground: the card's highlight, a little stronger with focus.
fn highlight(theme: &Theme, focused: bool) -> Color {
    let t = match (theme.dark, focused) {
        (true, true) => 0.08,
        (true, false) => 0.055,
        (false, true) => 0.07,
        (false, false) => 0.045,
    };
    mix(ground(theme), theme.strong, t)
}

/// A text cut with '…' to `max`, nothing where not even that fits.
#[allow(clippy::too_many_arguments)]
fn text(
    scene: &mut Scene,
    s: &str,
    x: f32,
    y: f32,
    spec: TextSpec,
    anchor: Anchor,
    color: Color,
    max: f32,
    m: &mut dyn Measure,
) {
    let fitted = m.fit(s, spec, max.max(0.0));
    if fitted.is_empty() {
        return;
    }
    scene.push(scene::Item::Text {
        text: fitted.into(),
        x,
        y,
        spec,
        anchor,
        color,
        alpha: 1.0,
        max_width: Some(max.max(1.0)),
    });
}

/// A heading in small capitals, tracked: one text a character, each placed after the last's
/// advance and the tracking. Cut with '…' where it would pass `max`.
fn tracked(
    scene: &mut Scene,
    s: &str,
    x: f32,
    y: f32,
    color: Color,
    max: f32,
    m: &mut dyn Measure,
) {
    let spec = TextSpec::new(HEADING).weight(Weight::Semibold);
    let caps: Vec<(String, f32)> = s
        .to_uppercase()
        .chars()
        .map(|c| {
            let s = c.to_string();
            let w = m.width(&s, spec);
            (s, w)
        })
        .collect();
    let total = caps.iter().map(|(_, w)| w + TRACKING).sum::<f32>() - TRACKING;
    let dots = m.width("…", spec);
    let mut pen = x;
    for (c, w) in caps {
        let s = if total > max && pen + w + TRACKING + dots > x + max {
            "…".to_string()
        } else {
            c
        };
        if s != " " {
            scene.push(scene::Item::Text {
                text: s.as_str().into(),
                x: pen,
                y,
                spec,
                anchor: Anchor::Start,
                color,
                alpha: 1.0,
                max_width: None,
            });
        }
        if s == "…" {
            return;
        }
        pen += w + TRACKING;
    }
}

/// A swatch centred on `(x, mid)` its left edge at `x`.
fn swatch(scene: &mut Scene, sw: Swatch, x: f32, mid: f32, r: f32, theme: &Theme) {
    let c = sw.color(theme);
    let path = match sw.shape {
        Shape::Dot => PathData::circle(x + r, mid, r),
        Shape::Square => PathData::rounded(Rect::new(x, mid - r, 2.0 * r, 2.0 * r), [2.0; 4]),
        Shape::Line => {
            PathData::rounded(Rect::new(x - 1.0, mid - 1.0, 2.0 * r + 2.0, 2.0), [1.0; 4])
        }
    };
    scene.push(scene::Item::Path {
        path,
        fill: Some(Paint::solid(c, 1.0)),
        stroke: None,
    });
}

/// A fold triangle in the gutter at `x`: pointing right when folded, down when open.
fn triangle(scene: &mut Scene, x: f32, mid: f32, folded: bool, color: Color) {
    use crate::scene::Verb;
    let verbs = if folded {
        vec![
            Verb::Move(x + 2.5, mid - 4.0),
            Verb::Line(x + 6.5, mid),
            Verb::Line(x + 2.5, mid + 4.0),
            Verb::Close,
        ]
    } else {
        vec![
            Verb::Move(x + 0.5, mid - 2.0),
            Verb::Line(x + 8.5, mid - 2.0),
            Verb::Line(x + 4.5, mid + 2.5),
            Verb::Close,
        ]
    };
    scene.push(scene::Item::Path {
        path: PathData { verbs },
        fill: Some(Paint::solid(color, 1.0)),
        stroke: None,
    });
}

/// A row's text baseline in its line: the bottom 22 px, centred.
fn baseline(line: Rect) -> f32 {
    line.bottom() - 7.0
}

/// The key column's width: the widest key of any field or branch (folded or not, so a fold
/// never moves the values), its indent and gutter in, with room after it, within a share of
/// the panel.
fn key_column(tree: &DetailTree, gut: &[bool], width: f32, m: &mut dyn Measure) -> f32 {
    let spec = TextSpec::new(BODY);
    // Per indent, only the keys longest in characters are measured: a JSON row with thousands
    // of members measures a handful, not thousands, at every draw.
    let mut longest: BTreeMap<u32, Vec<(usize, &str)>> = BTreeMap::new();
    for (i, r) in tree.rows.iter().enumerate() {
        if !matches!(r.kind, RowKind::Field | RowKind::Branch) {
            continue;
        }
        let at = u32::from(r.depth) * 2 + u32::from(gut[i]);
        let list = longest.entry(at).or_default();
        list.push((r.key.chars().count(), &r.key));
        if list.len() > 2 * KEYS_MEASURED {
            list.sort_by_key(|k| std::cmp::Reverse(k.0));
            list.truncate(KEYS_MEASURED);
        }
    }
    let mut widest: f32 = 0.0;
    for (at, mut list) in longest {
        list.sort_by_key(|k| std::cmp::Reverse(k.0));
        let indent = (at / 2) as f32 * INDENT + if at % 2 == 1 { GUTTER } else { 0.0 };
        for (_, k) in list.into_iter().take(KEYS_MEASURED) {
            widest = widest.max(m.width(k, spec) + indent);
        }
    }
    (widest + 18.0)
        .clamp(64.0, (width * 0.45).max(64.0))
        .round()
}

/// How many of the longest keys at each indent the key column measures.
const KEYS_MEASURED: usize = 4;

/// The heading each line is under, and whether the rows under it have a gutter for fold
/// triangles (any of them is a branch).
fn gutters(tree: &DetailTree) -> Vec<bool> {
    let mut out = vec![false; tree.rows.len()];
    let mut start = 0;
    let mut i = 0;
    while i <= tree.rows.len() {
        let end = i == tree.rows.len() || (i > start && tree.rows[i].kind == RowKind::Section);
        if end {
            let any = tree.rows[start..i]
                .iter()
                .any(|r| r.kind == RowKind::Branch);
            for g in &mut out[start..i] {
                *g = any;
            }
            start = i;
        }
        i += 1;
    }
    out
}

/// The hint's pieces, in groups dropped from the end where they don't fit: keys drawn as caps,
/// words muted.
fn hint_groups(tree: &DetailTree, state: &PanelState) -> Vec<Vec<(bool, &'static str)>> {
    if !state.focused {
        return vec![
            vec![(true, "Tab"), (false, "panel")],
            vec![(true, "⏎"), (false, "close")],
        ];
    }
    let names = tree
        .rows
        .get(tree.shown_as(state.row))
        .is_some_and(|r| r.item.is_some());
    let mut g = vec![
        vec![(true, "j"), (true, "k"), (false, "move")],
        vec![(true, "y"), (false, "copy")],
    ];
    if tree.rows.iter().any(|r| r.kind == RowKind::Branch) {
        g.insert(1, vec![(true, "h"), (true, "l"), (false, "fold")]);
    }
    if names {
        g.insert(1, vec![(true, "⏎"), (false, "go to")]);
    }
    g
}

const CAP_PAD: f32 = 5.0;
const CAP_GAP: f32 = 4.0;
const GROUP_GAP: f32 = 12.0;

fn cap_spec() -> TextSpec {
    TextSpec::new(NOTE).mono()
}

fn word_spec() -> TextSpec {
    TextSpec::new(NOTE)
}

fn piece_width(key: bool, s: &str, m: &mut dyn Measure) -> f32 {
    if key {
        m.width(s, cap_spec()).max(7.0) + 2.0 * CAP_PAD
    } else {
        m.width(s, word_spec())
    }
}

fn group_width(g: &[(bool, &str)], m: &mut dyn Measure) -> f32 {
    g.iter().map(|(k, s)| piece_width(*k, s, m)).sum::<f32>()
        + CAP_GAP * g.len().saturating_sub(1) as f32
}

fn hint(
    scene: &mut Scene,
    groups: &[Vec<(bool, &str)>],
    r: Rect,
    theme: &Theme,
    m: &mut dyn Measure,
) {
    let mut fit = 0;
    let mut w = 0.0;
    for (i, g) in groups.iter().enumerate() {
        let gw = group_width(g, m) + if i > 0 { GROUP_GAP } else { 0.0 };
        if w + gw > r.w {
            break;
        }
        w += gw;
        fit = i + 1;
    }
    let mid = r.y + HINT_H / 2.0;
    let mut x = r.x.round();
    for (gi, g) in groups.iter().take(fit).enumerate() {
        if gi > 0 {
            x += GROUP_GAP;
        }
        for (pi, (key, s)) in g.iter().enumerate() {
            if pi > 0 {
                x += CAP_GAP;
            }
            let pw = piece_width(*key, s, m);
            if *key {
                let cap = Rect::new(x, r.y, pw, HINT_H);
                scene.push(scene::Item::Path {
                    path: PathData::rounded(cap, [4.0; 4]),
                    fill: Some(Paint::solid(
                        theme.text,
                        if theme.dark { 0.05 } else { 0.04 },
                    )),
                    stroke: Some(Stroke::hairline(theme.ring, 1.0)),
                });
                let base = mid + m.cap(cap_spec()) / 2.0;
                scene.push(scene::Item::Text {
                    text: (*s).into(),
                    x: x + pw / 2.0,
                    y: base,
                    spec: cap_spec(),
                    anchor: Anchor::Middle,
                    color: theme.text,
                    alpha: 1.0,
                    max_width: None,
                });
            } else {
                let base = mid + m.cap(word_spec()) / 2.0;
                scene.push(scene::Item::Text {
                    text: (*s).into(),
                    x,
                    y: base,
                    spec: word_spec(),
                    anchor: Anchor::Start,
                    color: theme.muted,
                    alpha: 1.0,
                    max_width: None,
                });
            }
            x += pw;
        }
    }
}

/// How far up from the body's foot rows fade out when more of them are below.
const FADE: f32 = 16.0;
/// The least of a row that shows under the whole ones: less is a sliver, left out.
const PEEK_MIN: f32 = 6.0;

/// The rows after those `l` shows whole that start in the body's room left under them, each
/// with its line's rectangle (cut by the body's foot when drawn): what fills the panel while
/// rows remain below. None when the rest of the room is a sliver.
fn peek_rows(tree: &DetailTree, l: &PanelLayout) -> Vec<(usize, Rect)> {
    let mut out = Vec::new();
    let next = l.top + l.shown.len();
    let mut y = l.shown.last().map_or(l.body.y, |(_, r)| r.bottom());
    if l.body.bottom() - y < PEEK_MIN {
        return out;
    }
    let (x0, x1) = l.inner;
    for &i in l.lines.iter().skip(next) {
        if y >= l.body.bottom() - 0.01 {
            break;
        }
        let h = line_height(&tree.rows[i]);
        out.push((i, Rect::new(x0 - 8.0, y, x1 - x0 + 14.0, h)));
        y += h;
    }
    out
}

/// The panel drawn at `rect` (in the area's coordinates, which the scene's are): its ground and
/// inner hairline, the header, the rows shown from `state.top` with the cursor's lit, a
/// scrollbar when they overflow, and the hint at the foot where there's room. Regions:
/// [`Hit::Chrome`] over the whole panel (so nothing under it takes the pointer), then
/// [`Hit::Detail`] per row shown and [`Hit::DetailFold`] per fold triangle, by the row's index in
/// the tree.
pub fn panel(
    tree: &DetailTree,
    state: PanelState,
    rect: Rect,
    scale: f32,
    theme: &Theme,
    m: &mut dyn Measure,
) -> Scene {
    let l = layout(tree, &state, rect);
    let mut scene = Scene::new(rect.right(), rect.bottom(), scale);
    let ground = ground(theme);
    scene.push(scene::Item::Path {
        path: PathData::rect(rect),
        fill: Some(Paint::solid(ground, 1.0)),
        stroke: None,
    });
    let line = rule_color(theme);
    let edge = match l.dock {
        Dock::Right => [(rect.x, rect.y), (rect.x, rect.bottom())],
        Dock::Bottom => [(rect.x, rect.y), (rect.right(), rect.y)],
    };
    scene.push(scene::Item::Path {
        path: PathData::polyline(&edge),
        fill: None,
        stroke: Some(Stroke::hairline(line, 1.0)),
    });
    scene.region(rect, Hit::Chrome);
    let (x0, x1) = l.inner;
    let wide = (x1 - x0).max(0.0);

    // The header: the name, the kind under it, a rule.
    let title_spec = TextSpec::new(TITLE).weight(Weight::Semibold);
    let title_base = (rect.y + l.pad_top + m.ascent(title_spec)).round();
    text(
        &mut scene,
        &tree.title,
        x0,
        title_base,
        title_spec,
        Anchor::Start,
        theme.strong,
        wide,
        m,
    );
    if l.kind {
        let spec = TextSpec::new(BODY);
        let base = title_base + 20.0;
        let mut x = x0;
        if let Some(sw) = tree.swatch {
            let mid = base - m.cap(spec) / 2.0;
            swatch(&mut scene, sw, x, mid, 4.0, theme);
            x += 14.0;
        }
        text(
            &mut scene,
            &tree.kind,
            x,
            base,
            spec,
            Anchor::Start,
            theme.muted,
            x1 - x,
            m,
        );
    }
    scene.push(scene::Item::Path {
        path: PathData::polyline(&[(x0, l.rule_y), (x1, l.rule_y)]),
        fill: None,
        stroke: Some(Stroke::hairline(line, 1.0)),
    });

    // The rows.
    let gut = gutters(tree);
    let keyw = key_column(tree, &gut, wide, m);
    let vx = x0 + keyw;
    let cursor = tree.shown_as(state.row);
    let body_spec = TextSpec::new(BODY);
    let value_spec = TextSpec::new(BODY).tabular();
    let note_spec = TextSpec::new(NOTE).tabular();
    scene.push(scene::Item::PushClip(l.body));
    if tree.rows.is_empty() {
        text(
            &mut scene,
            "Nothing more to show",
            x0,
            l.body.y + 15.0,
            body_spec,
            Anchor::Start,
            theme.muted,
            wide,
            m,
        );
    }
    let mut rows_hit: Vec<(Rect, Hit)> = Vec::new();
    let mut folds_hit: Vec<(Rect, Hit)> = Vec::new();
    // The rows shown whole, then those that start in the room left under them, cut at the
    // body's foot and fading out there, so the panel is full while rows remain below.
    let peeks = peek_rows(tree, &l);
    let drawn = l
        .shown
        .iter()
        .map(|&(i, lr)| (i, lr, false))
        .chain(peeks.iter().map(|&(i, lr)| (i, lr, true)));
    for (i, lr, peek) in drawn {
        let r = &tree.rows[i];
        let base = baseline(lr);
        let mid = base - m.cap(body_spec) / 2.0;
        let lit = Rect::new(lr.x, lr.bottom() - ROW_H, lr.w, ROW_H);
        if i == cursor && r.kind != RowKind::Section {
            scene.push(scene::Item::Path {
                path: PathData::rounded(lit, [4.0; 4]),
                fill: Some(Paint::solid(highlight(theme, state.focused), 1.0)),
                stroke: None,
            });
        }
        let indent = f32::from(r.depth) * INDENT;
        let value_ink = if r.quiet { theme.muted } else { theme.text };
        match r.kind {
            RowKind::Section => {
                tracked(&mut scene, &r.key, x0, base, theme.muted, wide, m);
                continue;
            }
            RowKind::Sum => {
                // Clear of a lit row above it.
                let rule = (lr.y + 2.0).round() + 0.5;
                scene.push(scene::Item::Path {
                    path: PathData::polyline(&[(x0 + 14.0, rule), (x1 - 4.0, rule)]),
                    fill: None,
                    stroke: Some(Stroke::hairline(line, 1.0)),
                });
                let vw = m.width(&r.value, value_spec).min(wide * 0.5);
                text(
                    &mut scene,
                    &r.value,
                    x1 - 4.0,
                    base,
                    value_spec.weight(Weight::Semibold),
                    Anchor::End,
                    theme.text,
                    wide * 0.5,
                    m,
                );
                text(
                    &mut scene,
                    &r.key,
                    x0 + indent + 14.0,
                    base,
                    body_spec,
                    Anchor::Start,
                    theme.muted,
                    x1 - 16.0 - vw - (x0 + indent + 14.0),
                    m,
                );
            }
            RowKind::Mark(sw) => {
                let kx = x0 + indent;
                // A note in the small muted figures; a value in the rows' own.
                let vspec = if r.quiet { note_spec } else { value_spec };
                let vw = m.width(&r.value, vspec).min(wide * 0.5);
                if !r.value.is_empty() {
                    text(
                        &mut scene,
                        &r.value,
                        x1 - 4.0,
                        base,
                        vspec,
                        Anchor::End,
                        value_ink,
                        wide * 0.5,
                        m,
                    );
                }
                let nx = if let Some(sw) = sw {
                    swatch(&mut scene, sw, kx, mid, 3.5, theme);
                    kx + 14.0
                } else {
                    kx
                };
                let room = x1 - 4.0 - vw - if vw > 0.0 { 12.0 } else { 0.0 } - nx;
                text(
                    &mut scene,
                    &r.key,
                    nx,
                    base,
                    body_spec,
                    Anchor::Start,
                    theme.text,
                    room,
                    m,
                );
            }
            RowKind::Field | RowKind::Branch => {
                let kx = x0 + if gut[i] { GUTTER } else { 0.0 } + indent;
                if r.kind == RowKind::Branch {
                    let tx = kx - GUTTER;
                    triangle(&mut scene, tx, mid, r.folded, theme.muted);
                    if !peek {
                        folds_hit.push((
                            Rect::new(tx - 4.0, lit.y, GUTTER + 4.0, lit.h),
                            Hit::DetailFold(i as u32),
                        ));
                    }
                }
                let vxr = vx.max(kx + 36.0).min(x1 - 24.0);
                text(
                    &mut scene,
                    &r.key,
                    kx,
                    base,
                    body_spec,
                    Anchor::Start,
                    theme.muted,
                    vxr - 10.0 - kx,
                    m,
                );
                if !r.value.is_empty() {
                    text(
                        &mut scene,
                        &r.value,
                        vxr,
                        base,
                        value_spec,
                        Anchor::Start,
                        value_ink,
                        x1 - vxr,
                        m,
                    );
                }
            }
        }
        if !peek {
            rows_hit.push((lit, Hit::Detail(i as u32)));
        }
    }
    scene.push(scene::Item::PopClip);
    if let Some(&(_, first)) = peeks.first() {
        let foot = l.body.bottom();
        let top = (foot - FADE).max(first.y);
        scene.push(scene::Item::Path {
            path: PathData::rect(Rect::new(rect.x + 1.0, top, rect.w - 1.0, foot - top)),
            fill: Some(Paint::Linear {
                from: (0.0, top),
                to: (0.0, foot),
                stops: vec![(0.0, ground, 0.0), (1.0, ground, 1.0)],
            }),
            stroke: None,
        });
    }
    // Rows first, so the triangles over them win.
    for (r, h) in rows_hit.into_iter().chain(folds_hit) {
        scene.region(r, h);
    }

    // A scrollbar where the lines don't all fit.
    if l.max_top > 0 && l.body.h > 24.0 {
        let track = Rect::new(rect.right() - 7.0, l.body.y + 4.0, 3.0, l.body.h - 8.0);
        let page = l.shown.len().max(1) as f32;
        let frac = (page / l.lines.len().max(1) as f32).min(1.0);
        let th = (track.h * frac).max(18.0).min(track.h);
        let at = l.top as f32 / l.max_top.max(1) as f32;
        let thumb = Rect::new(track.x, track.y + (track.h - th) * at, track.w, th);
        scene.push(scene::Item::Path {
            path: PathData::rounded(thumb, [1.5; 4]),
            fill: Some(Paint::solid(theme.muted, 0.55)),
            stroke: None,
        });
    }
    if let Some(h) = l.hint {
        hint(&mut scene, &hint_groups(tree, &state), h, theme, m);
    }
    scene
}
