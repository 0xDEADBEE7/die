//! A tidy tree (Buchheim and Walker's), roots left to right, unit spacing.
//!
//! Walker's algorithm in Buchheim, Jünger and Leipert's linear-time form ("Improving Walker's
//! Algorithm to Run in Linear Time", 2002): a parent is centred over its first and last child,
//! siblings and neighbouring subtrees are at least one unit apart at every depth, the space a
//! subtree is pushed is shared out among the smaller subtrees between, and a subtree draws the
//! same wherever it sits. Several roots are laid out as the children of one unseen root, so the
//! trees sit side by side, one unit apart where they come nearest. Both walks are iterative, so a
//! deep tree can't overflow the stack.

const NONE: usize = usize::MAX;

/// Each of `n` nodes' position, laid out as trees by each node's parent: x in units from 0 at
/// the leftmost node, and y up as in [`stress`](fn@super::stress), so depth `d` is at
/// `y = -d` and a renderer that flips y puts the roots on top. Children go left to right in
/// index order, and roots likewise. A node with no parent, its own parent, or a parent past `n`
/// is a root. A cycle of parents is cut where the walk up from its lowest-numbered descendant
/// first comes round, and that node becomes a root.
pub fn tree(n: usize, parent: &[Option<usize>]) -> Vec<(f64, f64)> {
    if n == 0 {
        return vec![];
    }
    let up = parents(n, parent);
    let mut t = Tidy::new(n, &up);
    t.first_walk();
    t.second_walk()
}

/// Each node's parent, None for a root, with cycles cut.
fn parents(n: usize, parent: &[Option<usize>]) -> Vec<Option<usize>> {
    let mut up: Vec<Option<usize>> = (0..n)
        .map(|i| {
            parent
                .get(i)
                .copied()
                .flatten()
                .filter(|&p| p < n && p != i)
        })
        .collect();
    // A node reaches a root unless its parents lead round a cycle: 0 unknown, 1 on the walk
    // now, 2 settled.
    let mut state = vec![0u8; n];
    let mut walk = Vec::new();
    for i in 0..n {
        let mut v = i;
        while state[v] == 0 {
            state[v] = 1;
            walk.push(v);
            match up[v] {
                Some(p) => v = p,
                None => break,
            }
        }
        if state[v] == 1 && up[v].is_some() {
            // The walk came round to `v`: cut the cycle there.
            up[v] = None;
        }
        for w in walk.drain(..) {
            state[w] = 2;
        }
    }
    up
}

/// The walks' state, one entry per node and one for the unseen root at index `n`.
struct Tidy {
    n: usize,
    up: Vec<usize>,
    children: Vec<Vec<usize>>,
    /// From 1, among its siblings.
    number: Vec<usize>,
    prelim: Vec<f64>,
    modifier: Vec<f64>,
    thread: Vec<usize>,
    ancestor: Vec<usize>,
    change: Vec<f64>,
    shift: Vec<f64>,
}

impl Tidy {
    fn new(n: usize, up: &[Option<usize>]) -> Tidy {
        let up: Vec<usize> = up
            .iter()
            .map(|p| p.unwrap_or(n))
            .chain(std::iter::once(NONE))
            .collect();
        let mut children = vec![Vec::new(); n + 1];
        let mut number = vec![1; n + 1];
        for (v, &p) in up.iter().enumerate().take(n) {
            children[p].push(v);
            number[v] = children[p].len();
        }
        Tidy {
            n,
            up,
            children,
            number,
            prelim: vec![0.0; n + 1],
            modifier: vec![0.0; n + 1],
            thread: vec![NONE; n + 1],
            ancestor: (0..=n).collect(),
            change: vec![0.0; n + 1],
            shift: vec![0.0; n + 1],
        }
    }

    fn left(&self, v: usize) -> usize {
        if self.thread[v] != NONE {
            self.thread[v]
        } else {
            self.children[v].first().copied().unwrap_or(NONE)
        }
    }

    fn right(&self, v: usize) -> usize {
        if self.thread[v] != NONE {
            self.thread[v]
        } else {
            self.children[v].last().copied().unwrap_or(NONE)
        }
    }

    fn left_sibling(&self, v: usize) -> usize {
        match self.up[v] {
            NONE => NONE,
            p if self.number[v] > 1 => self.children[p][self.number[v] - 2],
            _ => NONE,
        }
    }

    /// FIRSTWALK, children before their parent, and APPORTION as each child is done.
    fn first_walk(&mut self) {
        let root = self.n;
        let mut default = vec![NONE; self.n + 1];
        let mut stack = vec![(root, 0usize)];
        while let Some(&mut (v, ref mut next)) = stack.last_mut() {
            if let Some(&w) = self.children[v].get(*next) {
                *next += 1;
                if default[v] == NONE {
                    default[v] = w;
                }
                stack.push((w, 0));
                continue;
            }
            stack.pop();
            let w = self.left_sibling(v);
            if self.children[v].is_empty() {
                self.prelim[v] = if w == NONE { 0.0 } else { self.prelim[w] + 1.0 };
            } else {
                self.execute_shifts(v);
                let kids = &self.children[v];
                let mid = (self.prelim[kids[0]] + self.prelim[kids[kids.len() - 1]]) / 2.0;
                if w == NONE {
                    self.prelim[v] = mid;
                } else {
                    self.prelim[v] = self.prelim[w] + 1.0;
                    self.modifier[v] = self.prelim[v] - mid;
                }
            }
            let p = self.up[v];
            if p != NONE {
                default[p] = self.apportion(v, default[p]);
            }
        }
    }

    fn apportion(&mut self, v: usize, mut default: usize) -> usize {
        let w = self.left_sibling(v);
        if w == NONE {
            return default;
        }
        let (mut vir, mut vor, mut vil) = (v, v, w);
        let mut vol = self.children[self.up[v]][0];
        let (mut sir, mut sor) = (self.modifier[vir], self.modifier[vor]);
        let (mut sil, mut sol) = (self.modifier[vil], self.modifier[vol]);
        while self.right(vil) != NONE && self.left(vir) != NONE {
            vil = self.right(vil);
            vir = self.left(vir);
            vol = self.left(vol);
            vor = self.right(vor);
            self.ancestor[vor] = v;
            let shift = (self.prelim[vil] + sil) - (self.prelim[vir] + sir) + 1.0;
            if shift > 0.0 {
                let a = self.ancestor_of(vil, v, default);
                self.move_subtree(a, v, shift);
                sir += shift;
                sor += shift;
            }
            sil += self.modifier[vil];
            sir += self.modifier[vir];
            sol += self.modifier[vol];
            sor += self.modifier[vor];
        }
        if self.right(vil) != NONE && self.right(vor) == NONE {
            self.thread[vor] = self.right(vil);
            self.modifier[vor] += sil - sor;
        }
        if self.left(vir) != NONE && self.left(vol) == NONE {
            self.thread[vol] = self.left(vir);
            self.modifier[vol] += sir - sol;
            default = v;
        }
        default
    }

    /// The greatest uncommon ancestor of `vil` and `v`: `vil`'s ancestor if that's `v`'s
    /// sibling, else the default.
    fn ancestor_of(&self, vil: usize, v: usize, default: usize) -> usize {
        let a = self.ancestor[vil];
        if self.up[a] == self.up[v] { a } else { default }
    }

    fn move_subtree(&mut self, wl: usize, wr: usize, shift: f64) {
        let subtrees = (self.number[wr] - self.number[wl]) as f64;
        self.change[wr] -= shift / subtrees;
        self.shift[wr] += shift;
        self.change[wl] += shift / subtrees;
        self.prelim[wr] += shift;
        self.modifier[wr] += shift;
    }

    fn execute_shifts(&mut self, v: usize) {
        let (mut shift, mut change) = (0.0, 0.0);
        for &w in self.children[v].iter().rev() {
            self.prelim[w] += shift;
            self.modifier[w] += shift;
            change += self.change[w];
            shift += self.shift[w] + change;
        }
    }

    /// SECONDWALK: each node's x is its preliminary place plus its ancestors' modifiers; then
    /// everything moves so the leftmost node is at 0.
    fn second_walk(&self) -> Vec<(f64, f64)> {
        let mut out = vec![(0.0, 0.0); self.n];
        let mut stack: Vec<(usize, f64, f64)> = self.children[self.n]
            .iter()
            .rev()
            .map(|&r| (r, self.modifier[self.n], 0.0))
            .collect();
        while let Some((v, m, depth)) = stack.pop() {
            out[v] = (self.prelim[v] + m, 0.0 - depth);
            let m = m + self.modifier[v];
            stack.extend(self.children[v].iter().rev().map(|&w| (w, m, depth + 1.0)));
        }
        let least = out.iter().fold(f64::INFINITY, |l, p| l.min(p.0));
        for p in &mut out {
            p.0 -= least;
        }
        out
    }
}
