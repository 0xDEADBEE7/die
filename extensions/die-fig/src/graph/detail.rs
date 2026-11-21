//! Where the graph meets the detail panel and search: a node's or an edge's tree from what the
//! model knows ([`crate::detail::node_tree`] and [`crate::detail::edge_tree`] over plain facts),
//! the items its rows and a search's matches name, and the emphasis of the nodes a search lights.

use super::{Emphasis, GraphModel};
use crate::detail::{self, DetailTree, EdgeFacts, Item, Neighbour, NodeFacts, Swatch};
use crate::interact::Cursor;
use crate::style::ColorSpec;

impl GraphModel {
    /// A node's colour as the panel's swatch names it: its `fig` colour, else its group's slot
    /// (slot 1 for every node when no node has a group; the neutral for a ninth group and on,
    /// and for a node without a group among nodes with one), as [`GraphModel::color`] has it.
    pub fn swatch(&self, i: usize) -> Swatch {
        let spec = match self.node_fig(i).color {
            Some(c) => c,
            None => match self.group_of[i] {
                Some(g) if g < 8 => ColorSpec::Slot(g as u8),
                Some(_) => ColorSpec::Dim,
                None if self.groups.is_empty() => ColorSpec::Slot(0),
                None => ColorSpec::Dim,
            },
        };
        Swatch::dot(spec)
    }

    /// Node `i` as an item: its index and id.
    pub fn item(&self, i: usize) -> Item {
        Item::Node {
            index: i as u32,
            id: self.id(i).into(),
        }
    }

    /// Edge `k` as an item: its index and its ends' ids.
    pub fn edge_item(&self, k: usize) -> Item {
        let e = &self.spec.edges[k];
        Item::Edge {
            index: k as u32,
            from: e.from.as_str().into(),
            to: e.to.as_str().into(),
        }
    }

    /// The item the cursor is on: its node's.
    pub fn item_of(&self, cursor: &Cursor) -> Option<Item> {
        self.node_of(cursor).map(|i| self.item(i))
    }

    /// Node `j` as a row of another's tree names it.
    fn neighbour_of(&self, j: usize) -> Neighbour {
        Neighbour {
            index: j as u32,
            id: self.id(j).into(),
            name: self.label(j).to_string(),
            swatch: Some(self.swatch(j)),
            degree: self.degree(j),
        }
    }

    /// Node `i`'s detail: its id, label, group, degree, fields, neighbours in edge order (as
    /// m3 lists worker's), metadata and `fig` block.
    pub fn node_detail(&self, i: usize) -> DetailTree {
        let n = &self.spec.nodes[i];
        let label = self.label(i);
        detail::node_tree(&NodeFacts {
            index: i as u32,
            id: &n.id,
            label: (label != n.id).then_some(label),
            group: n.group.as_deref(),
            swatch: Some(self.swatch(i)),
            degree_in: self.degree_in[i],
            degree_out: self.degree_out[i],
            directed: self.directed(),
            neighbours: self
                .neighbours(i)
                .into_iter()
                .map(|j| self.neighbour_of(j))
                .collect(),
            fields: Some(&n.raw),
        })
    }

    /// Edge `k`'s detail: its label, weight and fields, and its two ends naming their nodes.
    pub fn edge_detail(&self, k: usize) -> DetailTree {
        let e = &self.spec.edges[k];
        let (a, b) = self.ends[k];
        detail::edge_tree(&EdgeFacts {
            index: k as u32,
            from: self.neighbour_of(a as usize),
            to: self.neighbour_of(b as usize),
            label: e.label.as_deref(),
            weight: e.weight,
            directed: e.directed,
            fields: Some(&e.raw),
        })
    }

    /// The detail of what an item names: a node (by its id, so an item from before an edit
    /// still finds it) or an edge (by its index, while there is one). None for a chart's item
    /// or one the graph no longer has.
    pub fn detail_of(&self, item: &Item) -> Option<DetailTree> {
        match item {
            Item::Node { id, .. } => self.index_of(id).map(|i| self.node_detail(i)),
            Item::Edge { index, from, to } => {
                let k = *index as usize;
                let e = self.spec.edges.get(k)?;
                (*e.from == **from && *e.to == **to).then(|| self.edge_detail(k))
            }
            _ => None,
        }
    }

    /// What a search looks through: each node by its label, and by its id where that differs,
    /// naming the node ([`GraphModel::names`] as items).
    pub fn search_names(&self) -> Vec<(String, Item)> {
        let mut out = Vec::with_capacity(self.len());
        for i in 0..self.len() {
            out.push((self.label(i).to_string(), self.item(i)));
            if self.label(i) != self.id(i) {
                out.push((self.id(i).to_string(), self.item(i)));
            }
        }
        out
    }

    /// Emphasis of a set of nodes (what a search typed so far matches): they and the edges
    /// among them Related, `focus` (the best match) Focused, the rest Dimmed; what opts out as
    /// it is.
    pub fn set_emphasis(
        &self,
        set: &[usize],
        focus: Option<usize>,
    ) -> (Vec<Emphasis>, Vec<Emphasis>) {
        let n = self.len();
        let mut nodes = vec![Emphasis::Dimmed; n];
        for &i in set.iter().filter(|&&i| i < n) {
            nodes[i] = Emphasis::Related;
        }
        let mut edges: Vec<Emphasis> = self
            .ends
            .iter()
            .map(|&(a, b)| {
                let lit = |i: u32| nodes[i as usize] != Emphasis::Dimmed;
                match lit(a) && lit(b) {
                    true => Emphasis::Related,
                    false => Emphasis::Dimmed,
                }
            })
            .collect();
        if let Some(f) = focus.filter(|&f| f < n) {
            nodes[f] = Emphasis::Focused;
        }
        self.opt_out(&mut nodes, &mut edges, focus);
        (nodes, edges)
    }
}
