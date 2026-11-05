//! Text buffers (core-owned, never checked out): rope, transactions with an origin (C14),
//! a bounded change log (C15), markers, anchored decorations (C16) and per-window views.
#![warn(missing_docs)]

use crate::error::*;
use crate::ids::*;
use crate::layout::LayoutCache;
use crate::undo::{History, Limit, Op};
use crate::windows::View;
use ropey::Rope;
use slotmap::SlotMap;
use std::any::Any;
use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::ops::Range;
use std::rc::Rc;

mod hidden;
mod search;
pub(crate) use hidden::Hidden;
pub use search::{Found, Pattern};

/// How many changes `edits_since` can reach back.
pub const LOG_CAP: usize = 1024;

/// Who made an edit (C14). Read-only ranges block `User` edits only.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Origin {
    /// Your editing commands (die-vim). Blocked by read-only ranges.
    User,
    /// Extension code. Never blocked.
    Extension,
}

/// A position as a line and a column, both from 0. `col` counts bytes from the line's start
/// (tree-sitter's convention).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Point {
    /// The line, from 0.
    pub line: usize,
    /// Bytes from the line's start.
    pub col: usize,
}

/// One change, as incremental parsers want it: bytes `start..old_end` were replaced by `text`,
/// which now ends at `new_end`. The points give the same positions as lines and byte columns.
#[derive(Clone, Debug, PartialEq)]
pub struct Change {
    /// The revision this change made.
    pub rev: u64,
    /// Where the change starts, in the text just before it.
    pub start: usize,
    /// Where the replaced text ended, in the text just before it.
    pub old_end: usize,
    /// Where the new text ends, in the text just after it.
    pub new_end: usize,
    /// `start` as a line and column.
    pub start_point: Point,
    /// `old_end` as a line and column, in the text just before the change.
    pub old_end_point: Point,
    /// `new_end` as a line and column, in the text just after the change.
    pub new_end_point: Point,
    /// The new text.
    pub text: String,
    /// Who made it. Undo and redo log their changes as `User`.
    pub origin: Origin,
    /// Whether it was a silent edit, not recorded in undo. Undo and redo aren't.
    pub silent: bool,
}

/// Where a position goes when text is inserted exactly at it: `Left` stays before the new
/// text, `Right` moves after it. Strictly inside replaced text, `Left` goes to the
/// replacement's start and `Right` to its end. A position at a replaced range's start stays at
/// the start, and one at its old end goes to the replacement's end, whatever its gravity.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Gravity {
    /// Stays before text inserted at it.
    Left,
    /// Moves after text inserted at it.
    Right,
}

/// A selection's shape.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Shape {
    /// The characters between the two ends.
    Char,
    /// The whole lines from the anchor's to the head's.
    Line,
    /// The display columns between the two ends, on the lines from the anchor's to the head's.
    Block,
}

/// A window's selection. `anchor` and `head` are byte offsets on cursor positions; `head` is
/// the cursor. A plain cursor has them equal, with shape `Char`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Selection {
    /// The end that stays put while the selection grows.
    pub anchor: usize,
    /// The end the cursor is on.
    pub head: usize,
    /// How the two ends select text.
    pub shape: Shape,
    /// Goal column for vertical motion. die-vim decides what it means, except that on a
    /// `Block` selection `Some(usize::MAX)` means each line to its end, as `$` does there.
    pub goal: Option<usize>,
}

impl Selection {
    /// A cursor at `p`: both ends there, shape `Char`, no goal column.
    pub fn cursor(p: usize) -> Self {
        Selection {
            anchor: p,
            head: p,
            shape: Shape::Char,
            goal: None,
        }
    }
    /// This selection with goal column `g`.
    pub fn with_goal(mut self, g: Option<usize>) -> Self {
        self.goal = g;
        self
    }
}

/// What happens when search lands inside a Hide.
#[derive(Clone, Debug, PartialEq)]
pub enum Reveal {
    /// Leave it hidden.
    Never,
    /// Remove the decoration.
    Remove,
    /// Run this command with `args.deco` set, so the owner updates its own state.
    Command(String),
}

/// Text with faces on parts of it: a Hide's replacement, or virtual text. Each run draws its
/// face over the faces of the text the string stands in for or before, as Emacs bases a display
/// string on the text under it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Styled {
    /// The text.
    pub text: String,
    /// Faces on byte ranges of `text`. A cluster takes the last run holding its first byte.
    pub faces: Vec<(Range<usize>, &'static str)>,
}

impl From<String> for Styled {
    fn from(text: String) -> Self {
        Styled {
            text,
            faces: vec![],
        }
    }
}

impl From<&str> for Styled {
    fn from(text: &str) -> Self {
        text.to_string().into()
    }
}

/// What a decoration does to its range (contract §9).
#[derive(Clone)]
pub enum Deco {
    /// Styles the range with a named face.
    Face(&'static str),
    /// Hides the range. Layout decides what that means for rows and cursor positions (contract
    /// §9, "Cursor positions").
    Hide {
        /// Text drawn in its place, on one line: newlines and tabs become spaces, and empty
        /// text is no replacement.
        replacement: Option<Styled>,
        /// What happens when search lands inside it.
        reveal: Reveal,
    },
    /// Text shown just before the position, not part of the buffer: one line, as a
    /// replacement is. The position is the range's start.
    Virtual(Styled),
    /// Blocks `User` edits in the range (C14).
    ReadOnly,
    /// A named keymap, consulted first while the cursor is in the range.
    Keymap(String),
    /// An extension's value, found by position with `decorations_at`.
    Tag(Rc<dyn Any>),
}

/// A decoration: a range that moves with edits, and what it does.
#[derive(Clone)]
pub struct Decoration {
    /// The range, in bytes.
    pub range: Range<usize>,
    /// What it does.
    pub deco: Deco,
    /// Gravity of the start and end. Default (Right, Left): insertions at the edges stay outside.
    pub sticky: (Gravity, Gravity),
    /// Its place in the order decorations were added to this buffer. Ids can't say: a freed
    /// slot is reused. Virtual text at one position shows in this order (contract §9).
    pub seq: u64,
}

/// The line-number gutter's style. Contract §9, "Line numbers", has the rules.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LineNumbers {
    /// No gutter.
    Off,
    /// Each display line's first row shows its first line's number.
    Absolute,
    /// The cursor's display line shows its number, and every other its distance from it.
    Relative,
    /// The cursor's row shows its line's number, and every other row its distance from it.
    Visual,
}

/// How long lines break into rows (contract §9, "Where soft wrap breaks").
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Wrap {
    /// Each display line is one row, however long.
    Off,
    /// Break at word boundaries first, as Emacs's `visual-line-mode` does.
    Word,
    /// The same rule with no break points, as a terminal breaks.
    Char,
}

/// A text buffer's settings, changeable at runtime. Their defaults come from the file-type
/// mode and your config.
#[derive(Clone, Debug)]
pub struct Settings {
    /// Soft wrap.
    pub wrap: Wrap,
    /// The text area's width in cells; the gutter is outside it. None fills the window.
    pub max_width: Option<usize>,
    /// Center the gutter and text in the window, the left padding rounded down.
    pub center: bool,
    /// The gutter's style.
    pub line_numbers: LineNumbers,
    /// Cells per tab stop, counted from the display line's start; 0 counts as 1.
    pub tab_width: usize,
    /// Blocks `User` edits to the whole buffer (C14).
    pub read_only: bool,
    /// Whether to draw the cursor (scrib's browser turns it off).
    pub cursor_visible: bool,
    /// Undo keeps at most this many steps; the oldest go first. 0 turns undo off. A
    /// lower limit applies at the next edit, undo or redo.
    pub undo_steps: usize,
    /// Undo keeps steps holding at most this much removed and inserted text, in bytes; the
    /// oldest go first, and the newest stays whatever its size. It bounds the text, not the
    /// history's memory, which adds about 70 bytes per change. Silent edits that don't continue
    /// or rewrite the previous one add about 24 bytes each until the step below them is dropped.
    pub undo_bytes: usize,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            wrap: Wrap::Word,
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

#[derive(Clone, Copy)]
enum Rec {
    Record(Option<Selection>),
    Silent,
    History,
}

/// A text buffer's text and everything anchored to it: the rope, undo history, change log,
/// markers, decorations, and each window's selection and scroll position. Core owns it, and
/// it's borrowed by id, never checked out (C2).
pub struct TextBuf {
    rope: Rope,
    rev: u64,
    log: VecDeque<Change>,
    history: History,
    markers: SlotMap<MarkerId, (usize, Gravity)>,
    decos: SlotMap<DecorationId, Decoration>,
    next_seq: u64,
    pub(crate) views: HashMap<WindowId, View>,
    /// Views of windows that don't show this buffer now, moved through edits as the others are,
    /// so going back to a buffer finds the cursor and scroll position it left.
    pub(crate) parked: HashMap<WindowId, View>,
    /// The view the last window to leave this buffer left, Emacs's buffer point: where a window
    /// with no view of its own starts, if no window shows the buffer.
    pub(crate) rest: Option<View>,
    pub(crate) settings: Settings,
    /// The maximum width `toggle-max-width` turned off.
    pub(crate) max_width_off: Option<usize>,
    pub(crate) cmd: u64,
    pub(crate) actor: Option<WindowId>,
    pub(crate) cache: RefCell<LayoutCache>,
    /// The text in one piece for regex search, until the next edit.
    flat: RefCell<Option<String>>,
}

/// Where a position goes when `[start, old_end)` is replaced by text ending at `new_end`.
pub(crate) fn map_pos(p: usize, start: usize, old_end: usize, new_end: usize, g: Gravity) -> usize {
    if p < start {
        p
    } else if p > old_end || (p == old_end && old_end > start) {
        p - old_end + new_end
    } else if (p == start && old_end > start) || g == Gravity::Left {
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
    /// Replace the bytes `r` with `s`.
    pub fn replace(mut self, r: Range<usize>, s: impl Into<String>) -> Self {
        self.ops.push((r, s.into()));
        self
    }
    /// Insert `s` at byte `at`.
    pub fn insert(self, at: usize, s: impl Into<String>) -> Self {
        self.replace(at..at, s)
    }
    /// Delete the bytes `r`.
    pub fn delete(self, r: Range<usize>) -> Self {
        self.replace(r, "")
    }
    /// Apply every change or none, and return the revision after them. A change with an empty
    /// range and empty text is dropped: it makes no revision, no logged change and no undo step,
    /// and redo stays available. Text replaced by itself is still a change and an undo step, as
    /// `ra` on an `a` is in Evil and Vim. Errors: `BadRange` if a range is reversed, past the
    /// end, off a character boundary, or overlaps another change in the transaction; `ReadOnly`
    /// if a `User` change touches read-only text.
    pub fn commit(self) -> Result<u64> {
        self.buf.apply(self.origin, self.silent, self.ops)
    }
}

impl TextBuf {
    /// A buffer holding `text`, at revision 0, with default settings and no windows.
    pub fn new(text: &str) -> Self {
        TextBuf {
            rope: Rope::from_str(text),
            rev: 0,
            log: VecDeque::new(),
            history: History::default(),
            markers: SlotMap::with_key(),
            decos: SlotMap::with_key(),
            next_seq: 0,
            views: HashMap::new(),
            parked: HashMap::new(),
            rest: None,
            settings: Settings::default(),
            max_width_off: None,
            cmd: 0,
            actor: None,
            cache: RefCell::default(),
            flat: RefCell::default(),
        }
    }

    // ---- reading
    /// The text.
    pub fn rope(&self) -> &Rope {
        &self.rope
    }
    /// The text's length in bytes.
    pub fn len(&self) -> usize {
        self.rope.len_bytes()
    }
    /// Whether the text is empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    /// The revision: it goes up by one per change, not per transaction.
    pub fn rev(&self) -> u64 {
        self.rev
    }
    /// The line holding byte `pos`, clamped to the end.
    pub fn line_of(&self, pos: usize) -> usize {
        self.rope.byte_to_line(pos.min(self.len()))
    }
    /// The byte where `line` starts; past the last line, the end of the text.
    pub fn line_start(&self, line: usize) -> usize {
        self.rope.line_to_byte(line.min(self.rope.len_lines()))
    }
    /// Lines as ropey counts them: text ending in a newline has an empty last line, so "a\n"
    /// has 2.
    pub fn line_count(&self) -> usize {
        self.rope.len_lines()
    }
    /// The line and byte column of `b`, clamped to the end.
    pub fn point(&self, b: usize) -> Point {
        let b = b.min(self.len());
        let line = self.rope.byte_to_line(b);
        Point {
            line,
            col: b - self.rope.line_to_byte(line),
        }
    }
    /// Whether `b` is on a character boundary inside the text.
    pub fn is_boundary(&self, b: usize) -> bool {
        b <= self.len() && self.rope.char_to_byte(self.rope.byte_to_char(b)) == b
    }
    /// Every change since `rev`, oldest first, or None if `rev` is newer than `rev()` or the log
    /// (`LOG_CAP` changes) no longer reaches back that far (C15). Changes are listed as they were
    /// applied, each in the coordinates of the text after the one before it: a transaction
    /// applies its changes from the last position to the first, so two inserts at one position
    /// are listed last one first. Undo and redo log their changes too, with origin `User` and not
    /// silent. A reader that needs every change catches up at least every `LOG_CAP` changes.
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
    /// The buffer's settings.
    pub fn settings(&self) -> &Settings {
        &self.settings
    }
    /// The buffer's settings, to change them.
    pub fn settings_mut(&mut self) -> &mut Settings {
        &mut self.settings
    }

    // ---- editing
    /// Start a transaction. Nothing changes until `commit`.
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
        // An empty change makes no revision, no log entry and no undo step, as Emacs records
        // nothing for an empty insertion or deletion. Text replaced by itself is still a change.
        ops.retain(|(r, s)| !(r.is_empty() && s.is_empty()));
        if ops.is_empty() {
            return Ok(self.rev);
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
        *self.flat.get_mut() = None;
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
        self.cache.get_mut().edited(r.start, r.end, new_end);
        let o = Op {
            pos: r.start,
            removed,
            inserted: s,
        };
        match rec {
            Rec::Silent => self.history.silent(o.shift()),
            Rec::Record(sel) => self.history.record(o, sel, self.cmd, self.undo_limit()),
            Rec::History => {}
        }
    }

    fn undo_limit(&self) -> Limit {
        Limit {
            steps: self.settings.undo_steps,
            bytes: self.settings.undo_bytes,
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
        self.map_views(m);
    }

    // ---- undo
    /// Start an undo group spanning commands, such as an insert session. Every recorded edit to
    /// this buffer until the matching `end_undo_group` is one step, including edits from other
    /// commands and callbacks. `undo_boundary` still ends the current step early; the group
    /// stays open, as a new step. An edit made earlier in the same command joins it. Groups nest;
    /// only the outermost end closes it. A group left open swallows every later edit, so end it
    /// on every way out (Escape, a click elsewhere, a buffer switch). `undo` and `redo` leave it
    /// open: the next edit starts a new step inside it.
    pub fn begin_undo_group(&mut self) {
        self.history.begin(self.cmd);
    }
    /// End the group `begin_undo_group` started. With no group open, this does nothing.
    pub fn end_undo_group(&mut self) {
        self.history.end();
    }
    /// Whether the text has changed since it was last saved, Emacs's `buffer-modified-p`. Undoing
    /// or redoing back to the saved text clears it. Text replaced by itself sets it, as any undo
    /// step does. A silent edit sets it too, and undo can't clear that: it isn't in the history.
    pub fn modified(&self) -> bool {
        self.history.modified()
    }
    /// Set the flag: `false` says the text as it is now is what's saved (a save, a revert),
    /// `true` that it differs, until a later `false`.
    pub fn set_modified(&mut self, modified: bool) {
        self.history.set_modified(modified);
    }
    /// End the current undo step here: the next edit starts a new step, even in the same
    /// command or inside an open group (which stays open, as a new step).
    pub fn undo_boundary(&mut self) {
        self.history.boundary();
    }
    /// Whether an undo group is open: an insert session, say.
    pub(crate) fn undo_group_open(&self) -> bool {
        self.history.group_open()
    }
    /// Undo the last step and return the range it changed, in the text after the undo:
    /// every change the step made lies inside it. The acting window's selection goes back
    /// to what it was just before the step's first change, shape and goal included, moved
    /// through any silent edits since. Errors: `NothingToUndo`, `ReadOnly`, or `UndoConflict`,
    /// which drops the history (contract §9). `UndoConflict` also reports, once, history
    /// dropped earlier because a silent edit changed text a step being collected needed.
    pub fn undo(&mut self) -> Result<Range<usize>> {
        self.history_step(false)
    }
    /// Redo the last undone step and return the range it changed. The acting window gets a
    /// cursor at the start of that range. Errors: `NothingToUndo` when there's nothing to redo,
    /// `ReadOnly`, or `UndoConflict`, which drops what's left to redo.
    pub fn redo(&mut self) -> Result<Range<usize>> {
        self.history_step(true)
    }

    fn history_step(&mut self, redo: bool) -> Result<Range<usize>> {
        self.history.trim(self.undo_limit());
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
        let hull = hull.unwrap_or(0..0);
        let sel = if redo {
            Some(Selection::cursor(hull.start))
        } else {
            plan.sel
        };
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
        Ok(hull)
    }

    // ---- markers
    /// A position that moves with edits, clamped to the end. Errors: `BadRange` off a character
    /// boundary.
    pub fn add_marker(&mut self, pos: usize, g: Gravity) -> Result<MarkerId> {
        let pos = pos.min(self.len());
        if !self.is_boundary(pos) {
            return Err(Error::BadRange {
                start: pos,
                end: pos,
            });
        }
        Ok(self.markers.insert((pos, g)))
    }
    /// Where a marker is now, or None if `id` is dead.
    pub fn marker(&self, id: MarkerId) -> Option<usize> {
        self.markers.get(id).map(|m| m.0)
    }
    /// Remove a marker; its last position, or None if `id` was already dead.
    pub fn remove_marker(&mut self, id: MarkerId) -> Option<usize> {
        self.markers.remove(id).map(|m| m.0)
    }

    // ---- decorations
    /// Add a decoration over `range`. Text inserted at either edge stays outside it until
    /// `set_sticky` says otherwise. Every cursor moves out of text this hides. Errors: `BadRange`
    /// if the range is reversed, past the end or off a character boundary.
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
            *replacement = replacement.take().filter(|r| !r.text.is_empty());
            if let Some(r) = replacement {
                r.text = r.text.replace(['\n', '\t'], " ");
            }
        }
        if let Deco::Virtual(t) = &mut deco {
            t.text = t.text.replace(['\n', '\t'], " ");
        }
        let seq = self.next_seq;
        self.next_seq += 1;
        let shapes_rows = matches!(deco, Deco::Hide { .. } | Deco::Virtual(_));
        let id = self.decos.insert(Decoration {
            range,
            deco,
            sticky: (Gravity::Right, Gravity::Left),
            seq,
        });
        if shapes_rows {
            self.cache.get_mut().clear();
            self.normalize_views();
        }
        Ok(id)
    }
    /// Remove a decoration and return it, or None if `id` was already dead.
    pub fn undecorate(&mut self, id: DecorationId) -> Option<Decoration> {
        let d = self.decos.remove(id)?;
        if matches!(d.deco, Deco::Hide { .. } | Deco::Virtual(_)) {
            self.cache.get_mut().clear();
            self.normalize_views();
        }
        Some(d)
    }
    /// Set the gravity of a decoration's two ends, or None if `id` is dead.
    pub fn set_sticky(&mut self, id: DecorationId, start: Gravity, end: Gravity) -> Option<()> {
        self.decos.get_mut(id)?.sticky = (start, end);
        Some(())
    }
    /// A decoration by id, or None if `id` is dead.
    pub fn decoration(&self, id: DecorationId) -> Option<&Decoration> {
        self.decos.get(id)
    }
    /// Every decoration, in no particular order.
    pub fn decorations(&self) -> impl Iterator<Item = (DecorationId, &Decoration)> {
        self.decos.iter()
    }
    /// Decorations whose range covers the character at `pos`.
    pub fn decorations_at(&self, pos: usize) -> impl Iterator<Item = (DecorationId, &Decoration)> {
        self.decos
            .iter()
            .filter(move |(_, d)| d.range.start <= pos && pos < d.range.end)
    }
}
