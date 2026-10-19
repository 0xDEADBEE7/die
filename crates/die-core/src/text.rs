//! Text buffers (core-owned, never checked out): rope, transactions with an origin (C14),
//! a bounded change log (C15), markers, anchored decorations (C16) and per-window views.
use crate::error::*;
use crate::ids::*;
use crate::layout::LayoutCache;
use crate::undo::{History, Limit, Op};
use ropey::Rope;
use slotmap::SlotMap;
use std::any::Any;
use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::ops::Range;
use std::rc::Rc;

mod search;
pub use search::{Found, Pattern};

/// How many changes `edits_since` can reach back.
pub const LOG_CAP: usize = 1024;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Origin {
    /// Your editing commands (die-vim). Blocked by read-only ranges.
    User,
    /// Extension code. Never blocked.
    Extension,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Point {
    pub line: usize,
    pub col: usize, // bytes
}

/// One change, as incremental parsers want it.
#[derive(Clone, Debug, PartialEq)]
pub struct Change {
    pub rev: u64,
    pub start: usize,
    pub old_end: usize,
    pub new_end: usize,
    pub start_point: Point,
    pub old_end_point: Point,
    pub new_end_point: Point,
    pub text: String,
    pub origin: Origin,
    pub silent: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Gravity {
    Left,
    Right,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Shape {
    Char,
    Line,
    Block,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Selection {
    pub anchor: usize,
    pub head: usize,
    pub shape: Shape,
    /// Goal column for vertical motion. die-vim decides what it means.
    pub goal: Option<usize>,
}

impl Selection {
    pub fn cursor(p: usize) -> Self {
        Selection {
            anchor: p,
            head: p,
            shape: Shape::Char,
            goal: None,
        }
    }
    pub fn with_goal(mut self, g: Option<usize>) -> Self {
        self.goal = g;
        self
    }
}

/// What happens when search lands inside a Hide.
#[derive(Clone, Debug, PartialEq)]
pub enum Reveal {
    Never,
    Remove,
    /// Run this command with `args.deco` set, so the owner updates its own state.
    Command(String),
}

#[derive(Clone)]
pub enum Deco {
    Face(&'static str),
    Hide {
        replacement: Option<String>,
        reveal: Reveal,
    },
    /// Text shown just before the position, not part of the buffer.
    Virtual(String),
    ReadOnly,
    /// A named keymap, consulted first while the cursor is in the range.
    Keymap(String),
    Tag(Rc<dyn Any>),
}

#[derive(Clone)]
pub struct Decoration {
    pub range: Range<usize>,
    pub deco: Deco,
    /// Gravity of the start and end. Default (Right, Left): insertions at the edges stay outside.
    pub sticky: (Gravity, Gravity),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LineNumbers {
    Off,
    Absolute,
    Relative,
}

#[derive(Clone, Debug)]
pub struct Settings {
    pub wrap: bool,
    pub max_width: Option<usize>,
    pub center: bool,
    pub line_numbers: LineNumbers,
    pub tab_width: usize,
    pub read_only: bool,
    pub cursor_visible: bool,
    /// Undo keeps at most this many steps; the oldest go first. 0 turns undo off.
    pub undo_steps: usize,
    /// Undo keeps at most this much removed and inserted text, in bytes; the oldest steps
    /// go first. The newest step stays, whatever its size.
    pub undo_bytes: usize,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            wrap: true,
            max_width: None,
            center: false,
            line_numbers: LineNumbers::Off,
            tab_width: 8,
            read_only: false,
            cursor_visible: true,
            undo_steps: 2000,
            undo_bytes: 16 << 20,
        }
    }
}

/// Per-window state of a text buffer (C10): selection, scroll position and size.
#[derive(Clone, Copy, Debug)]
pub(crate) struct View {
    pub sel: Selection,
    pub top: usize,
    pub top_row: usize,
    pub cols: usize,
    pub rows: usize,
}

#[derive(Clone, Copy)]
enum Rec {
    Record(Option<Selection>),
    Silent,
    History,
}

pub struct TextBuf {
    rope: Rope,
    rev: u64,
    log: VecDeque<Change>,
    history: History,
    markers: SlotMap<MarkerId, (usize, Gravity)>,
    decos: SlotMap<DecorationId, Decoration>,
    pub(crate) deco_gen: u64,
    pub(crate) views: HashMap<WindowId, View>,
    pub(crate) settings: Settings,
    pub(crate) cmd: u64,
    pub(crate) actor: Option<WindowId>,
    pub(crate) cache: RefCell<LayoutCache>,
    /// The text in one piece, for regex search, and the revision it was made at.
    flat: RefCell<(Option<u64>, String)>,
}

/// Where a position goes when `[start, old_end)` is replaced by text ending at `new_end`.
pub(crate) fn map_pos(p: usize, start: usize, old_end: usize, new_end: usize, g: Gravity) -> usize {
    if p < start {
        p
    } else if p > old_end || (p == old_end && old_end > start) {
        p - old_end + new_end
    } else if p == start && old_end > start {
        start
    } else if g == Gravity::Left {
        start
    } else {
        new_end
    }
}

fn blocks(ro: &Range<usize>, r: &Range<usize>) -> bool {
    if r.start == r.end {
        ro.start < r.start && r.start < ro.end
    } else {
        r.start < ro.end && ro.start < r.end
    }
}

/// A transaction: changes in the coordinates of the text before it, applied atomically.
#[must_use = "a transaction does nothing until commit()"]
pub struct Tx<'a> {
    buf: &'a mut TextBuf,
    origin: Origin,
    silent: bool,
    ops: Vec<(Range<usize>, String)>,
}

impl Tx<'_> {
    /// Not recorded in undo. Undo adjusts around it.
    pub fn silent(mut self) -> Self {
        self.silent = true;
        self
    }
    pub fn replace(mut self, r: Range<usize>, s: impl Into<String>) -> Self {
        self.ops.push((r, s.into()));
        self
    }
    pub fn insert(self, at: usize, s: impl Into<String>) -> Self {
        self.replace(at..at, s)
    }
    pub fn delete(self, r: Range<usize>) -> Self {
        self.replace(r, "")
    }
    /// Apply every change or none. Returns the new revision.
    pub fn commit(self) -> Result<u64> {
        self.buf.apply(self.origin, self.silent, self.ops)
    }
}

impl TextBuf {
    pub fn new(text: &str) -> Self {
        TextBuf {
            rope: Rope::from_str(text),
            rev: 0,
            log: VecDeque::new(),
            history: History::default(),
            markers: SlotMap::with_key(),
            decos: SlotMap::with_key(),
            deco_gen: 0,
            views: HashMap::new(),
            settings: Settings::default(),
            cmd: 0,
            actor: None,
            cache: RefCell::default(),
            flat: RefCell::default(),
        }
    }

    // ---- reading
    pub fn rope(&self) -> &Rope {
        &self.rope
    }
    pub fn len(&self) -> usize {
        self.rope.len_bytes()
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    pub fn rev(&self) -> u64 {
        self.rev
    }
    pub fn line_of(&self, pos: usize) -> usize {
        self.rope.byte_to_line(pos.min(self.len()))
    }
    pub fn line_start(&self, line: usize) -> usize {
        self.rope.line_to_byte(line.min(self.rope.len_lines()))
    }
    pub fn line_count(&self) -> usize {
        self.rope.len_lines()
    }
    pub fn point(&self, b: usize) -> Point {
        let line = self.rope.byte_to_line(b);
        Point {
            line,
            col: b - self.rope.line_to_byte(line),
        }
    }
    pub fn is_boundary(&self, b: usize) -> bool {
        b <= self.len() && self.rope.char_to_byte(self.rope.byte_to_char(b)) == b
    }
    /// Every change since `rev`, or None if the bounded log no longer reaches back that far (C15).
    pub fn edits_since(&self, rev: u64) -> Option<impl Iterator<Item = &Change>> {
        if rev > self.rev {
            return None;
        }
        let oldest = self.log.front().map_or(self.rev + 1, |c| c.rev);
        if rev + 1 < oldest {
            return None;
        }
        let skip = (rev + 1 - oldest) as usize;
        Some(self.log.iter().skip(skip))
    }
    pub fn settings(&self) -> &Settings {
        &self.settings
    }
    pub fn settings_mut(&mut self) -> &mut Settings {
        &mut self.settings
    }

    // ---- editing
    pub fn tx(&mut self, origin: Origin) -> Tx<'_> {
        Tx {
            buf: self,
            origin,
            silent: false,
            ops: vec![],
        }
    }

    fn ro_ranges(&self) -> Vec<Range<usize>> {
        self.decos
            .values()
            .filter(|d| matches!(d.deco, Deco::ReadOnly))
            .map(|d| d.range.clone())
            .collect()
    }

    fn apply(
        &mut self,
        origin: Origin,
        silent: bool,
        mut ops: Vec<(Range<usize>, String)>,
    ) -> Result<u64> {
        ops.sort_by_key(|o| (o.0.start, o.0.end));
        let mut prev = 0;
        for (r, _) in &ops {
            if r.start > r.end
                || r.start < prev
                || !self.is_boundary(r.start)
                || !self.is_boundary(r.end)
            {
                return Err(Error::BadRange {
                    start: r.start,
                    end: r.end,
                });
            }
            prev = r.end;
        }
        if origin == Origin::User {
            let ro = self.ro_ranges();
            for (r, _) in &ops {
                if self.settings.read_only || ro.iter().any(|x| blocks(x, r)) {
                    return Err(Error::ReadOnly { at: r.start });
                }
            }
        }
        let rec = if silent {
            Rec::Silent
        } else {
            Rec::Record(self.actor_selection())
        };
        for (r, s) in ops.into_iter().rev() {
            self.apply_one(r, s, origin, rec);
        }
        self.normalize_views();
        Ok(self.rev)
    }

    fn actor_selection(&self) -> Option<Selection> {
        self.actor.and_then(|w| self.views.get(&w)).map(|v| v.sel)
    }

    fn apply_one(&mut self, r: Range<usize>, s: String, origin: Origin, rec: Rec) {
        let removed = self.rope.byte_slice(r.clone()).to_string();
        let (sp, op) = (self.point(r.start), self.point(r.end));
        let (cs, ce) = (
            self.rope.byte_to_char(r.start),
            self.rope.byte_to_char(r.end),
        );
        self.rope.remove(cs..ce);
        self.rope.insert(cs, &s);
        let new_end = r.start + s.len();
        self.rev += 1;
        let change = Change {
            rev: self.rev,
            start: r.start,
            old_end: r.end,
            new_end,
            start_point: sp,
            old_end_point: op,
            new_end_point: self.point(new_end),
            text: s.clone(),
            origin,
            silent: matches!(rec, Rec::Silent),
        };
        self.log.push_back(change);
        if self.log.len() > LOG_CAP {
            self.log.pop_front();
        }
        self.map_all(r.start, r.end, new_end);
        let o = Op {
            pos: r.start,
            removed,
            inserted: s,
        };
        match rec {
            Rec::Silent => self.history.silent(o.shift()),
            Rec::Record(sel) => {
                let limit = Limit {
                    steps: self.settings.undo_steps,
                    bytes: self.settings.undo_bytes,
                };
                self.history.record(o, sel, self.cmd, limit)
            }
            Rec::History => {}
        }
    }

    fn map_all(&mut self, start: usize, old_end: usize, new_end: usize) {
        let m = |p, g| map_pos(p, start, old_end, new_end, g);
        for (p, g) in self.markers.values_mut() {
            *p = m(*p, *g);
        }
        for d in self.decos.values_mut() {
            let s = m(d.range.start, d.sticky.0);
            d.range = s..m(d.range.end, d.sticky.1).max(s);
        }
        for v in self.views.values_mut() {
            v.sel.anchor = m(v.sel.anchor, Gravity::Right);
            v.sel.head = m(v.sel.head, Gravity::Right);
            v.top = m(v.top, Gravity::Left);
        }
    }

    /// Keep every cursor on a visible position and every scroll top on a display-line start.
    pub(crate) fn normalize_views(&mut self) {
        let wins: Vec<WindowId> = self.views.keys().copied().collect();
        for w in wins {
            let v = self.views[&w];
            let (a, h, t) = (
                self.snap(v.sel.anchor, None),
                self.snap(v.sel.head, None),
                self.dline_start(v.top),
            );
            let v = self.views.get_mut(&w).unwrap();
            v.sel.anchor = a;
            v.sel.head = h;
            if t != v.top {
                v.top = t;
                v.top_row = 0;
            }
        }
    }

    // ---- undo
    /// Start an undo group that spans commands, such as an insert session.
    pub fn begin_undo_group(&mut self) {
        let sel = self.actor_selection();
        self.history.begin(sel, self.cmd);
    }
    pub fn end_undo_group(&mut self) {
        self.history.end();
    }
    /// End the current undo step here: the next edit starts a new step, even in the same
    /// command or inside an open group (which stays open, as a new step).
    pub fn undo_boundary(&mut self) {
        self.history.boundary();
    }
    /// Undo the last step and return the range it changed, in the text after the undo:
    /// every change the step made lies inside it. The acting window's selection goes back
    /// to what it was before the step, shape and goal included, moved through any silent
    /// edits since. Errors: `NothingToUndo`, `ReadOnly`, or `UndoConflict`, which drops the
    /// history (contract §9).
    pub fn undo(&mut self) -> Result<Range<usize>> {
        self.history_step(false)
    }
    /// Redo the last undone step and return the range it changed. The acting window gets a
    /// cursor at the start of the step's first change.
    pub fn redo(&mut self) -> Result<Range<usize>> {
        self.history_step(true)
    }

    fn history_step(&mut self, redo: bool) -> Result<Range<usize>> {
        let plan = match self.history.plan(redo) {
            Err(Error::UndoConflict) => {
                self.history.truncate(redo);
                return Err(Error::UndoConflict);
            }
            r => r?,
        };
        // Undo is a User edit: it can't change read-only text. Map ranges through each step.
        let mut ro = self.ro_ranges();
        for op in &plan.ops {
            let r = op.pos..op.pos + op.removed.len();
            if self.settings.read_only || ro.iter().any(|x| blocks(x, &r)) {
                return Err(Error::ReadOnly { at: op.pos });
            }
            for x in ro.iter_mut() {
                let new_end = op.pos + op.inserted.len();
                x.start = map_pos(x.start, r.start, r.end, new_end, Gravity::Right);
                x.end = map_pos(x.end, r.start, r.end, new_end, Gravity::Left).max(x.start);
            }
        }
        // The changed range: the hull of every change, each moved through the later ones.
        let mut hull: Option<Range<usize>> = None;
        for op in &plan.ops {
            let r = op.pos..op.pos + op.removed.len();
            let ok = r.end <= self.len() && self.rope.byte_slice(r.clone()) == op.removed.as_str();
            debug_assert!(ok, "undo rebase produced a mismatch at {}", op.pos);
            if !ok {
                self.history.truncate(redo);
                return Err(Error::UndoConflict);
            }
            let new_end = op.pos + op.inserted.len();
            hull = Some(match hull {
                None => op.pos..new_end,
                Some(h) => {
                    let start = map_pos(h.start, r.start, r.end, new_end, Gravity::Left);
                    let end = map_pos(h.end, r.start, r.end, new_end, Gravity::Right);
                    start.min(op.pos)..end.max(new_end)
                }
            });
            self.apply_one(r, op.inserted.clone(), Origin::User, Rec::History);
        }
        let sel = plan.sel;
        self.history.commit(plan);
        let len = self.rope.len_bytes();
        if let (Some(w), Some(s)) = (self.actor, sel)
            && let Some(v) = self.views.get_mut(&w)
        {
            v.sel = Selection {
                anchor: s.anchor.min(len),
                head: s.head.min(len),
                ..s
            };
        }
        self.normalize_views();
        Ok(hull.unwrap_or(0..0))
    }

    // ---- markers
    pub fn add_marker(&mut self, pos: usize, g: Gravity) -> MarkerId {
        self.markers.insert((pos.min(self.len()), g))
    }
    pub fn marker(&self, id: MarkerId) -> Option<usize> {
        self.markers.get(id).map(|m| m.0)
    }
    pub fn remove_marker(&mut self, id: MarkerId) {
        self.markers.remove(id);
    }

    // ---- decorations
    pub fn decorate(&mut self, range: Range<usize>, mut deco: Deco) -> Result<DecorationId> {
        if range.start > range.end || !self.is_boundary(range.start) || !self.is_boundary(range.end)
        {
            return Err(Error::BadRange {
                start: range.start,
                end: range.end,
            });
        }
        if let Deco::Hide { replacement, .. } = &mut deco {
            // Layout text is single-line; an empty replacement is no replacement.
            *replacement = replacement
                .take()
                .filter(|r| !r.is_empty())
                .map(|r| r.replace(['\n', '\t'], " "));
        }
        if let Deco::Virtual(t) = &mut deco {
            *t = t.replace(['\n', '\t'], " ");
        }
        let id = self.decos.insert(Decoration {
            range,
            deco,
            sticky: (Gravity::Right, Gravity::Left),
        });
        self.deco_gen += 1;
        self.normalize_views();
        Ok(id)
    }
    pub fn undecorate(&mut self, id: DecorationId) -> Option<Decoration> {
        let d = self.decos.remove(id)?;
        self.deco_gen += 1;
        self.normalize_views();
        Some(d)
    }
    pub fn set_sticky(&mut self, id: DecorationId, start: Gravity, end: Gravity) {
        if let Some(d) = self.decos.get_mut(id) {
            d.sticky = (start, end);
        }
    }
    pub fn decoration(&self, id: DecorationId) -> Option<&Decoration> {
        self.decos.get(id)
    }
    pub fn decorations(&self) -> impl Iterator<Item = (DecorationId, &Decoration)> {
        self.decos.iter()
    }
    /// Decorations whose range covers the character at `pos`.
    pub fn decorations_at(&self, pos: usize) -> impl Iterator<Item = (DecorationId, &Decoration)> {
        self.decos
            .iter()
            .filter(move |(_, d)| d.range.start <= pos && pos < d.range.end)
    }

    // ---- per-window selections (C10)
    pub(crate) fn add_view(&mut self, w: WindowId, cols: usize, rows: usize) {
        let p = self.snap(0, None);
        self.views.insert(
            w,
            View {
                sel: Selection::cursor(p),
                top: 0,
                top_row: 0,
                cols,
                rows,
            },
        );
    }
    pub(crate) fn resize_view(&mut self, w: WindowId, cols: usize, rows: usize) {
        if let Some(v) = self.views.get_mut(&w) {
            v.cols = cols;
            v.rows = rows;
        }
    }
    pub fn selection(&self, w: WindowId) -> Option<Selection> {
        self.views.get(&w).map(|v| v.sel)
    }
    pub fn cursor(&self, w: WindowId) -> Option<usize> {
        self.views.get(&w).map(|v| v.sel.head)
    }
    /// Set a selection. Positions inside hidden text are moved to a visible one.
    pub fn set_selection(&mut self, w: WindowId, mut sel: Selection) -> Option<Selection> {
        sel.anchor = self.snap(sel.anchor, None);
        sel.head = self.snap(sel.head, None);
        let v = self.views.get_mut(&w)?;
        v.sel = sel;
        Some(sel)
    }
    pub fn set_cursor(&mut self, w: WindowId, pos: usize) -> Option<usize> {
        self.set_selection(w, Selection::cursor(pos))
            .map(|s| s.head)
    }
}
