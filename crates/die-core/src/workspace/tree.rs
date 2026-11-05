//! The split tree: how the frame divides into windows. A pure tree of cells, with no buffers or
//! `Ctx`, so what it promises (windows tile the frame) depends only on what's here.
//!
//! Each split keeps its children's sizes in cells, as a design for one length, and each child's
//! length from the last `tile`. Tiling scales the design to the length there is and never writes
//! it, so a frame made small and big again gets its windows back as they were. An operation
//! that moves a border first takes the lengths on screen as the new design.

use crate::ids::WindowId;

/// A side of a window: where a new split goes, and the way focus moves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    /// To the left.
    Left,
    /// To the right.
    Right,
    /// Above.
    Above,
    /// Below.
    Below,
}

impl Side {
    fn axis(self) -> Axis {
        match self {
            Side::Left | Side::Right => Axis::Cols,
            Side::Above | Side::Below => Axis::Rows,
        }
    }
}

/// Which way a split divides.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Axis {
    /// Side by side.
    Cols,
    /// One above another.
    Rows,
}

/// The cells a window takes, with its status line and any divider.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Rect {
    pub x: usize,
    pub y: usize,
    pub w: usize,
    pub h: usize,
}

impl Rect {
    /// The size of the text area: a row less for the status line, and a column less for the
    /// divider, unless the window reaches the frame's right edge.
    pub fn text_size(&self, frame_cols: usize) -> (usize, usize) {
        let divider = usize::from(self.x + self.w < frame_cols);
        (self.w.saturating_sub(divider), self.h.saturating_sub(1))
    }

    /// Whether `o` is across this window's edge at `side`, at `at`: a row for `Left` and `Right`,
    /// a column for `Above` and `Below`.
    pub fn touches(&self, o: &Rect, side: Side, at: usize) -> bool {
        let across = match side {
            Side::Left => o.x + o.w == self.x,
            Side::Right => o.x == self.x + self.w,
            Side::Above => o.y + o.h == self.y,
            Side::Below => o.y == self.y + self.h,
        };
        let along = match side {
            Side::Left | Side::Right => o.y <= at && at < o.y + o.h,
            Side::Above | Side::Below => o.x <= at && at < o.x + o.w,
        };
        across && along
    }
}

struct Kid {
    node: Node,
    /// Its share of the split, in cells of the design.
    size: usize,
    /// Its length from the last `tile`.
    len: usize,
}

enum Node {
    Leaf(WindowId),
    /// At least two children, none of them split the same way.
    Split(Axis, Vec<Kid>),
}

/// The fewest cells a window can have on either axis: one for text, and one for its status line
/// or divider.
const MIN_CELLS: usize = 2;

fn leaf(w: WindowId, size: usize) -> Kid {
    Kid {
        node: Node::Leaf(w),
        size,
        len: size,
    }
}

/// The children's lengths for a split `len` cells long: the design scaled, with rounding's cells
/// going to the largest remainders, then raised to each child's minimum out of what others have
/// to spare. In a frame too small for that, they keep what they have, and some can be 0.
fn fit(kids: &[Kid], axis: Axis, len: usize) -> Vec<usize> {
    let total: usize = kids.iter().map(|k| k.size).sum();
    let size = |k: &Kid| if total == 0 { 1 } else { k.size };
    let total = total.max(kids.len() * usize::from(total == 0));
    let mut out: Vec<usize> = kids.iter().map(|k| size(k) * len / total).collect();
    let mut by_remainder: Vec<usize> = (0..kids.len()).collect();
    by_remainder.sort_by_key(|&i| std::cmp::Reverse(size(&kids[i]) * len % total));
    let left = len - out.iter().sum::<usize>();
    for &i in by_remainder.iter().take(left) {
        out[i] += 1;
    }
    let mins: Vec<usize> = kids.iter().map(|k| k.node.min_len(axis)).collect();
    for i in 0..kids.len() {
        while out[i] < mins[i] {
            let spare = (0..kids.len()).filter(|&j| out[j] > mins[j]);
            let Some(j) = spare.max_by_key(|&j| out[j] - mins[j]) else {
                break;
            };
            out[j] -= 1;
            out[i] += 1;
        }
    }
    out
}

impl Node {
    fn contains(&self, w: WindowId) -> bool {
        match self {
            Node::Leaf(id) => *id == w,
            Node::Split(_, kids) => kids.iter().any(|k| k.node.contains(w)),
        }
    }

    fn leaves(&self, out: &mut Vec<WindowId>) {
        match self {
            Node::Leaf(id) => out.push(*id),
            Node::Split(_, kids) => kids.iter().for_each(|k| k.node.leaves(out)),
        }
    }

    fn min_len(&self, axis: Axis) -> usize {
        match self {
            Node::Leaf(_) => MIN_CELLS,
            Node::Split(a, kids) => {
                let mins = kids.iter().map(|k| k.node.min_len(axis));
                if *a == axis {
                    mins.sum()
                } else {
                    mins.max().unwrap_or(MIN_CELLS)
                }
            }
        }
    }

    fn tile(&mut self, r: Rect, out: &mut Vec<(WindowId, Rect)>) {
        match self {
            Node::Leaf(id) => out.push((*id, r)),
            Node::Split(axis, kids) => {
                let len = if *axis == Axis::Cols { r.w } else { r.h };
                let lens = fit(kids, *axis, len);
                let mut at = 0;
                for (k, l) in kids.iter_mut().zip(lens) {
                    k.len = l;
                    let part = match axis {
                        Axis::Cols => Rect {
                            x: r.x + at,
                            w: l,
                            ..r
                        },
                        Axis::Rows => Rect {
                            y: r.y + at,
                            h: l,
                            ..r
                        },
                    };
                    k.node.tile(part, out);
                    at += l;
                }
            }
        }
    }

    /// Divide `w`'s `len` cells with `new`, or say it isn't here.
    fn split(&mut self, w: WindowId, new: WindowId, side: Side, len: usize) -> bool {
        let half = len / 2;
        let before = matches!(side, Side::Left | Side::Above);
        match self {
            Node::Leaf(id) if *id == w => {
                let (old, new) = (leaf(w, len - half), leaf(new, half));
                let kids = if before {
                    vec![new, old]
                } else {
                    vec![old, new]
                };
                *self = Node::Split(side.axis(), kids);
                true
            }
            Node::Leaf(_) => false,
            Node::Split(axis, kids) => {
                let i = kids
                    .iter()
                    .position(|k| matches!(k.node, Node::Leaf(id) if id == w));
                if let (true, Some(i)) = (*axis == side.axis(), i) {
                    kids.iter_mut().for_each(|k| k.size = k.len);
                    kids[i].size = len - half;
                    kids.insert(if before { i } else { i + 1 }, leaf(new, half));
                    return true;
                }
                kids.iter_mut().any(|k| k.node.split(w, new, side, len))
            }
        }
    }

    /// Take `w` out. Its cells go to the child before it, or with none to the one after, as
    /// Emacs's `delete-window` gives them.
    fn remove(&mut self, w: WindowId) -> bool {
        let Node::Split(axis, kids) = self else {
            return false;
        };
        let at = kids
            .iter()
            .position(|k| matches!(k.node, Node::Leaf(id) if id == w));
        if let Some(i) = at {
            kids.iter_mut().for_each(|k| k.size = k.len);
            let gone = kids.remove(i);
            kids[i.saturating_sub(1)].size += gone.size;
        } else if !kids.iter_mut().any(|k| k.node.remove(w)) {
            return false;
        }
        // A child split the same way as this one joins it. Its children come as lengths on screen,
        // so the others do too: the design may be in other units, or at another length.
        let same = |k: &Kid| matches!(&k.node, Node::Split(a, _) if a == axis);
        if kids.iter().any(same) {
            let old = std::mem::take(kids);
            for k in old {
                match k.node {
                    Node::Split(a, inner) if a == *axis => {
                        kids.extend(inner.into_iter().map(|i| Kid { size: i.len, ..i }));
                    }
                    node => kids.push(Kid {
                        node,
                        size: k.len,
                        ..k
                    }),
                }
            }
        }
        if kids.len() == 1 {
            *self = kids.pop().expect("one child").node;
        }
        true
    }

    /// Grow `w` by `by` along `axis`, or shrink it, as far as it can go, trying its own split
    /// first and then the ones around that. Returns how far it went, or None if `w` isn't here.
    fn resize(&mut self, w: WindowId, axis: Axis, by: isize) -> Option<isize> {
        let Node::Split(a, kids) = self else {
            return self.contains(w).then_some(0);
        };
        let i = kids.iter().position(|k| k.node.contains(w))?;
        let done = kids[i].node.resize(w, axis, by)?;
        if *a != axis || done == by {
            return Some(done);
        }
        kids.iter_mut().for_each(|k| k.size = k.len);
        let more = take(kids, i, axis, by - done);
        kids[i].node.widen_path(w, axis, more);
        Some(done + more)
    }

    /// Cells came to this node from outside, along `axis`: they all go to `w`. `resize` has
    /// already taken the lengths on screen as the design of every split in the way.
    fn widen_path(&mut self, w: WindowId, axis: Axis, by: isize) {
        let Node::Split(a, kids) = self else { return };
        let Some(i) = kids.iter().position(|k| k.node.contains(w)) else {
            return;
        };
        if *a == axis {
            kids[i].size = kids[i].size.saturating_add_signed(by);
        }
        kids[i].node.widen_path(w, axis, by);
    }

    /// The node whose children include `w`.
    fn parent_of(&mut self, w: WindowId) -> Option<&mut Node> {
        let Node::Split(_, kids) = self else {
            return None;
        };
        if kids
            .iter()
            .any(|k| matches!(k.node, Node::Leaf(id) if id == w))
        {
            return Some(self);
        }
        let Node::Split(_, kids) = self else {
            return None;
        };
        kids.iter_mut().find_map(|k| k.node.parent_of(w))
    }

    /// How many windows side by side this takes along `axis`.
    fn weight(&self, axis: Axis) -> usize {
        match self {
            Node::Leaf(_) => 1,
            Node::Split(a, kids) => {
                let weights = kids.iter().map(|k| k.node.weight(axis));
                if *a == axis {
                    weights.sum()
                } else {
                    weights.max().unwrap_or(1)
                }
            }
        }
    }

    /// Emacs's `balance-windows`: each child's share follows how many windows it holds that way.
    fn balance(&mut self) {
        if let Node::Split(axis, kids) = self {
            for k in kids {
                k.size = k.node.weight(*axis);
                k.node.balance();
            }
        }
    }
}

/// Move cells to or from child `i`, and return how many moved. Growing takes from the children
/// after it first, nearest first, and then the ones before. Shrinking gives to the next child, or
/// to the one before if it's the last. Nothing goes below its minimum.
fn take(kids: &mut [Kid], i: usize, axis: Axis, by: isize) -> isize {
    if by < 0 {
        let spare = kids[i].size.saturating_sub(kids[i].node.min_len(axis));
        let moved = spare.min(by.unsigned_abs());
        let heir = if i + 1 < kids.len() { i + 1 } else { i - 1 };
        kids[i].size -= moved;
        kids[heir].size += moved;
        return -(moved as isize);
    }
    let mut left = by as usize;
    let order = (i + 1..kids.len()).chain((0..i).rev());
    for j in order {
        let spare = kids[j].size.saturating_sub(kids[j].node.min_len(axis));
        let moved = spare.min(left);
        kids[j].size -= moved;
        kids[i].size += moved;
        left -= moved;
    }
    by - left as isize
}

/// The frame, in cells, and how windows divide it.
#[derive(Default)]
pub(crate) struct Tree {
    root: Option<Node>,
    /// Columns and rows, with the command line's row.
    pub frame: (usize, usize),
    /// The host set the frame. Before that, a sole window's size decides it.
    pub fixed: bool,
}

impl Tree {
    pub fn is_empty(&self) -> bool {
        self.root.is_none()
    }

    pub fn contains(&self, w: WindowId) -> bool {
        self.root.as_ref().is_some_and(|r| r.contains(w))
    }

    /// Whether `w` is the only window in the tree.
    pub fn is_sole(&self, w: WindowId) -> bool {
        matches!(self.root, Some(Node::Leaf(id)) if id == w)
    }

    /// Every window, left to right and top to bottom within each split.
    pub fn windows(&self) -> Vec<WindowId> {
        let mut out = vec![];
        if let Some(r) = &self.root {
            r.leaves(&mut out);
        }
        out
    }

    /// Where each window goes now. Everything above the command line's row belongs to exactly
    /// one of them.
    pub fn tile(&mut self) -> Vec<(WindowId, Rect)> {
        let (w, rows) = self.frame;
        let area = Rect {
            x: 0,
            y: 0,
            w,
            h: rows.saturating_sub(1),
        };
        let mut out = vec![];
        if let Some(r) = &mut self.root {
            r.tile(area, &mut out);
        }
        out
    }

    /// Start the tree with its first window.
    pub fn set_root(&mut self, w: WindowId) {
        self.root = Some(Node::Leaf(w));
    }

    /// Put `new` at `side` of `w`, whose rectangle is `r`. False if `w` is too small to divide.
    pub fn split(&mut self, w: WindowId, new: WindowId, side: Side, r: Rect) -> bool {
        let len = if side.axis() == Axis::Cols { r.w } else { r.h };
        let fits = len / 2 >= MIN_CELLS;
        fits && self
            .root
            .as_mut()
            .is_some_and(|n| n.split(w, new, side, len))
    }

    /// Take `w` out, unless it's the only window.
    pub fn remove(&mut self, w: WindowId) -> bool {
        self.root.as_mut().is_some_and(|r| r.remove(w))
    }

    /// Make `w` the only window. Returns the ones that went.
    pub fn only(&mut self, w: WindowId) -> Vec<WindowId> {
        let mut gone = self.windows();
        gone.retain(|g| *g != w);
        self.root = Some(Node::Leaf(w));
        gone
    }

    /// Change `w`'s cells by `cols` and `rows`, as far as they'll go.
    pub fn resize(&mut self, w: WindowId, cols: isize, rows: isize) {
        if let Some(r) = &mut self.root {
            if cols != 0 {
                r.resize(w, Axis::Cols, cols);
            }
            if rows != 0 {
                r.resize(w, Axis::Rows, rows);
            }
        }
    }

    /// Even out the split holding `w`.
    pub fn balance(&mut self, w: WindowId) {
        if let Some(p) = self.root.as_mut().and_then(|r| r.parent_of(w)) {
            p.balance();
        }
    }
}
