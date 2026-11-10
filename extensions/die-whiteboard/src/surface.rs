//! A board's surface: the state a board buffer carries (its file, model, view, tool, selection,
//! undo log and what's unsaved), the tools as the pointer works them, and the [`Surface`] core
//! draws and sends events to. The state is shared between the surface and the extension
//! ([`Shared`]), so the session's save, which has `&Ctx` alone, and a buffer's kill, which comes
//! after the surface is gone, can still write the file.
//!
//! Normal state is the tool state and the pen is its default tool; insert state exists only
//! while a label is typed. The pointer is the board's in every state: `text()` is None, so
//! there's no copy state.

use crate::cells::{Cells, cell_size};
use crate::ink::{SIMPLIFY, Streamline, simplify};
use crate::model::{
    Board, CellSize, Element, INKS, Id, LIVE, Point, Rect, Shape, WIDTH_NAMES, WIDTHS, label_lines,
};
use crate::render::{Damage, Renderer, Scene};
use crate::undo::{Entry, History, original_index};
use crate::view::{View, ZOOM_STEP};
use die_core::{
    BufferId, Button, CellPixels, Color, Ctx, CursorShape, Event, Frame, KeyEvent, KeyKind, Mods,
    PointerKind, RenderInfo, State, Surface, SurfaceCursor, SurfacePointer,
};
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

/// The surface kind, which names the keymap.
pub const KIND: &str = "whiteboard";

/// What the next drag does.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tool {
    /// Freehand ink, the default.
    Pen,
    /// The pen at 16 px and 35 % opacity.
    Highlighter,
    /// A drag erases every element it crosses.
    Eraser,
    /// A rectangle; Shift for a square. One shot.
    Rect,
    /// An ellipse; Shift for a circle. One shot.
    Ellipse,
    /// A line; Shift holds it to an axis. One shot.
    Line,
    /// An arrow; Shift holds it to an axis. One shot.
    Arrow,
    /// A click places a label and types into it. One shot.
    Label,
    /// A click selects, a drag a marquee, a drag on the selection moves it.
    Select,
}

impl Tool {
    /// Every tool.
    pub const ALL: [Tool; 9] = [
        Tool::Pen,
        Tool::Highlighter,
        Tool::Eraser,
        Tool::Rect,
        Tool::Ellipse,
        Tool::Line,
        Tool::Arrow,
        Tool::Label,
        Tool::Select,
    ];
    /// Its name, in the status line and the session.
    pub fn name(self) -> &'static str {
        match self {
            Tool::Pen => "pen",
            Tool::Highlighter => "highlighter",
            Tool::Eraser => "eraser",
            Tool::Rect => "rect",
            Tool::Ellipse => "ellipse",
            Tool::Line => "line",
            Tool::Arrow => "arrow",
            Tool::Label => "text",
            Tool::Select => "select",
        }
    }
    /// The tool with this name.
    pub fn from_name(name: &str) -> Option<Tool> {
        Tool::ALL.into_iter().find(|t| t.name() == name)
    }
    /// Whether the pen comes back after one element.
    pub fn one_shot(self) -> bool {
        matches!(
            self,
            Tool::Rect | Tool::Ellipse | Tool::Line | Tool::Arrow | Tool::Label
        )
    }
}

/// What the state asks core for after a pointer event or a command.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Effect {
    /// Nothing.
    None,
    /// A label is being typed: insert state.
    EnterInsert,
    /// The label is done: normal state.
    LeaveInsert,
}

/// A label being typed, and what it was before, for undo.
#[derive(Clone, Debug)]
pub struct LabelEdit {
    /// The label.
    pub id: Id,
    before: Option<(usize, Element)>,
}

/// What the pointer is in the middle of.
#[derive(Debug)]
enum Gesture {
    None,
    /// The press that focused the window: it draws nothing, nor do its drags.
    Swallowed,
    Drawing {
        smooth: Streamline,
        el: Element,
    },
    Shaping {
        start: Point,
        el: Element,
    },
    Erasing(Erase),
    Marquee {
        start: Point,
        now: Point,
    },
    Moving {
        last: Point,
        before: Vec<(usize, Element)>,
        ids: Vec<Id>,
        moved: bool,
    },
}

/// Elements being taken off the board, one after another, as one entry: each with the index it
/// had on the board as it was, and the lines bound to them, which lose their bindings, as they
/// were and where they end up.
#[derive(Debug, Default)]
struct Erase {
    gone: Vec<(usize, Element)>,
    touched: Vec<(usize, Element)>,
    followers: Vec<Id>,
    /// The last sample, so the next erases along the way to it.
    last: Option<Point>,
}

/// How far apart the eraser looks along a drag, in board units.
const ERASE_STEP: f32 = 2.0;

/// A board as a buffer holds it.
pub struct BoardState {
    /// Its file.
    pub path: PathBuf,
    /// Its elements.
    pub board: Board,
    /// Where it is in the area.
    pub view: View,
    /// The tool.
    pub tool: Tool,
    /// The ink, `1`..=6.
    pub ink: u8,
    /// The width, an index into [`WIDTHS`].
    pub width: u8,
    /// The selected elements.
    pub selection: Vec<Id>,
    /// The undo log.
    pub history: History,
    /// Changed since the file was written.
    pub dirty: bool,
    /// When it last changed, on the workspace's clock.
    pub changed_at: Duration,
    /// Why the last save or export failed, until the next succeeds.
    pub error: Option<String>,
    /// Goes up with anything the screen shows: the render cache's key.
    pub rev: u64,
    /// Goes up with the committed picture: the board, the view, the selection, the inks.
    pub board_rev: u64,
    /// What changed in the committed picture since the renderer last heard.
    pub damage: Damage,
    /// The cells in pixels, as the last `Resize` said.
    pub cell: CellPixels,
    /// The area, as the last `Resize` said.
    pub area: (usize, usize),
    /// A window showing it has the focus.
    pub focused: bool,
    /// Each ink's colour, as the theme has it.
    pub inks: [Color; 6],
    /// The buffer, once it's made.
    pub buf: Option<BufferId>,
    /// Whether the renderer draws at the view's zoom.
    pub zooms: bool,
    gesture: Gesture,
    label: Option<LabelEdit>,
}

/// A board's state, shared by its surface and the extension.
pub type Shared = Rc<RefCell<BoardState>>;

fn snapshot(board: &Board, ids: &[Id]) -> Vec<(usize, Element)> {
    ids.iter()
        .filter_map(|id| {
            let i = board.index_of(*id)?;
            Some((i, board.elements[i].clone()))
        })
        .collect()
}

/// `p` held by Shift: a square or circle from `start`, or a line along an axis.
fn constrain(start: Point, p: Point, shift: bool, shape: &Shape) -> Point {
    if !shift {
        return p;
    }
    let d = p - start;
    match shape {
        Shape::Rect { .. } | Shape::Ellipse { .. } => {
            let s = d.x.abs().max(d.y.abs());
            Point::new(start.x + s.copysign(d.x), start.y + s.copysign(d.y))
        }
        Shape::Line { .. } if d.x.abs() >= d.y.abs() => Point::new(p.x, start.y),
        Shape::Line { .. } => Point::new(start.x, p.y),
        _ => p,
    }
}

impl BoardState {
    /// A board from `path`, with its elements, the pen, ink 1 and the medium width.
    pub fn new(path: PathBuf, board: Board) -> BoardState {
        BoardState {
            path,
            board,
            view: View::default(),
            tool: Tool::Pen,
            ink: 1,
            width: 1,
            selection: vec![],
            history: History::default(),
            dirty: false,
            changed_at: Duration::ZERO,
            error: None,
            rev: 1,
            board_rev: 1,
            damage: Damage::All,
            cell: CellPixels::default(),
            area: (0, 0),
            focused: true,
            inks: [Color::Default; 6],
            buf: None,
            zooms: false,
            gesture: Gesture::None,
            label: None,
        }
    }

    /// This state, shared.
    pub fn shared(self) -> Shared {
        Rc::new(RefCell::new(self))
    }

    /// A cell in board units.
    pub fn cell_size(&self) -> CellSize {
        cell_size(self.cell)
    }

    /// The file's name.
    pub fn name(&self) -> String {
        crate::file::name_of(&self.path)
    }

    /// The status line: `arch.svg  pen·2  medium  12 elements`, ` *` while unsaved, and why a
    /// save or export failed.
    pub fn status(&self) -> String {
        let n = self.board.len();
        let noun = if n == 1 { "element" } else { "elements" };
        let width = WIDTH_NAMES[usize::from(self.width).min(WIDTHS.len() - 1)];
        let mut s = format!(
            "{}  {}·{}  {width}  {n} {noun}",
            self.name(),
            self.tool.name(),
            self.ink
        );
        if self.dirty {
            s.push_str(" *");
        }
        if let Some(e) = &self.error {
            s.push_str("  ");
            s.push_str(e);
        }
        s
    }

    /// The screen shows something new.
    fn touch(&mut self) {
        self.rev += 1;
    }

    /// The committed picture changed, though not the file: the view, the selection, the inks.
    pub fn redraw(&mut self) {
        self.board_rev += 1;
        self.rev += 1;
        self.damage = Damage::All;
    }

    /// The board changed: the file is behind.
    pub fn changed(&mut self, now: Duration, damage: Damage) {
        self.dirty = true;
        self.changed_at = now;
        self.board_rev += 1;
        self.rev += 1;
        self.damage = self.damage.with(damage);
    }

    /// Record a change in the log, and that the file is behind.
    fn commit(&mut self, entry: Entry, now: Duration, damage: Damage) {
        if entry.is_empty() {
            return;
        }
        self.history.push(entry);
        self.changed(now, damage);
    }

    /// The gesture's element in progress.
    pub fn live(&self) -> Option<&Element> {
        match &self.gesture {
            Gesture::Drawing { el, .. } | Gesture::Shaping { el, .. } => Some(el),
            _ => None,
        }
    }

    /// The marquee being dragged.
    pub fn marquee(&self) -> Option<Rect> {
        match &self.gesture {
            Gesture::Marquee { start, now } => Some(Rect::from_points(*start, *now)),
            _ => None,
        }
    }

    /// The label being typed.
    pub fn editing(&self) -> Option<Id> {
        self.label.as_ref().map(|l| l.id)
    }

    /// Whether a gesture is under way.
    pub fn gesturing(&self) -> bool {
        !matches!(self.gesture, Gesture::None)
    }

    // -------------------------------------------------------------- the pointer

    /// The pointer at `p`, in board units, did `kind`.
    pub fn pointer(
        &mut self,
        kind: PointerKind,
        p: Point,
        clicks: u8,
        mods: Mods,
        now: Duration,
    ) -> Effect {
        match kind {
            PointerKind::Press(Button::Left) => self.press(p, clicks, now),
            PointerKind::Press(Button::Right) => {
                let effect = self.end_label(now);
                if self.focused {
                    self.start_erasing(p, now);
                }
                effect
            }
            PointerKind::Press(Button::Middle) | PointerKind::Move => Effect::None,
            PointerKind::Drag(_) => {
                self.drag(p, mods.shift, now);
                Effect::None
            }
            PointerKind::Release(_) => self.release(p, mods.shift, now),
            PointerKind::Scroll { dx, dy } => {
                if mods.super_ && self.zooms {
                    let about = self.view.to_physical(p, self.cell.scale);
                    self.zoom(ZOOM_STEP.powf(-dy / 4.0), Some(about));
                } else if !mods.super_ {
                    self.pan_cells(dx, dy);
                }
                Effect::None
            }
        }
    }

    fn press(&mut self, p: Point, clicks: u8, now: Duration) -> Effect {
        let effect = self.end_label(now);
        if !self.focused {
            self.gesture = Gesture::Swallowed;
            return effect;
        }
        let cell = self.cell_size();
        if clicks >= 2 && matches!(self.tool, Tool::Label | Tool::Select) {
            let label = self.board.hit(p, cell).filter(|id| {
                matches!(
                    self.board.get(*id).map(|e| &e.shape),
                    Some(Shape::Label { .. })
                )
            });
            if let Some(id) = label {
                self.gesture = Gesture::None;
                return self.edit_label(id);
            }
        }
        match self.tool {
            Tool::Pen | Tool::Highlighter => {
                let mut smooth = Streamline::default();
                let q = smooth.push(p);
                let el = Element {
                    id: LIVE,
                    ink: self.ink,
                    width: self.width,
                    shape: Shape::Stroke {
                        points: vec![q],
                        highlighter: self.tool == Tool::Highlighter,
                    },
                };
                self.gesture = Gesture::Drawing { smooth, el };
                self.touch();
            }
            Tool::Rect | Tool::Ellipse | Tool::Line | Tool::Arrow => {
                let shape = match self.tool {
                    Tool::Rect => Shape::Rect { a: p, b: p },
                    Tool::Ellipse => Shape::Ellipse { a: p, b: p },
                    other => Shape::Line {
                        a: p,
                        b: p,
                        arrow: other == Tool::Arrow,
                        from: None,
                        to: None,
                    },
                };
                let el = Element {
                    id: LIVE,
                    ink: self.ink,
                    width: self.width,
                    shape,
                };
                self.gesture = Gesture::Shaping { start: p, el };
                self.touch();
            }
            Tool::Label => return self.begin_label(p),
            Tool::Eraser => self.start_erasing(p, now),
            Tool::Select => match self.board.hit(p, cell) {
                Some(id) => {
                    if !self.selection.contains(&id) {
                        self.selection = vec![id];
                        self.redraw();
                    }
                    let mut ids = self.selection.clone();
                    ids.extend(self.board.followers(&self.selection));
                    let before = snapshot(&self.board, &ids);
                    self.gesture = Gesture::Moving {
                        last: p,
                        before,
                        ids,
                        moved: false,
                    };
                }
                None => {
                    if !self.selection.is_empty() {
                        self.selection.clear();
                        self.redraw();
                    }
                    self.gesture = Gesture::Marquee { start: p, now: p };
                }
            },
        }
        effect
    }

    fn start_erasing(&mut self, p: Point, now: Duration) {
        let mut erase = Erase::default();
        self.erase_at(p, now, &mut erase);
        self.gesture = Gesture::Erasing(erase);
    }

    /// Take every element the drag crosses on its way to `p` off the board.
    fn erase_at(&mut self, p: Point, now: Duration, erase: &mut Erase) {
        let cell = self.cell_size();
        let from = erase.last.replace(p).unwrap_or(p);
        let steps = (from.dist(p) / ERASE_STEP).ceil().clamp(1.0, 4096.0) as usize;
        for i in 0..=steps {
            let q = from + (p - from) * (i as f32 / steps as f32);
            while let Some(id) = self.board.hit(q, cell) {
                if !self.remove_one(id, erase, now) {
                    break;
                }
            }
        }
    }

    /// Take `id` off the board as part of `erase`. False if it wasn't there.
    fn remove_one(&mut self, id: Id, erase: &mut Erase, now: Duration) -> bool {
        let cell = self.cell_size();
        for f in self.board.followers(&[id]) {
            if erase.followers.contains(&f) {
                continue;
            }
            if let Some(i) = self.board.index_of(f) {
                let o = original_index(i, &erase.gone);
                erase.touched.push((o, self.board.elements[i].clone()));
                erase.followers.push(f);
            }
        }
        let Some((i, el)) = self.board.remove(id) else {
            return false;
        };
        // A line that followed a shape erased earlier goes back as it was, bound.
        let record = match erase.followers.iter().position(|f| *f == id) {
            Some(at) => {
                erase.followers.remove(at);
                erase.touched.remove(at)
            }
            None => (original_index(i, &erase.gone), el),
        };
        let region = record.1.bounds(cell);
        erase.gone.push(record);
        self.selection.retain(|s| *s != id);
        self.changed(now, Damage::Region(region));
        true
    }

    /// The entry for what `erase` took, once it's done.
    fn erased(&self, erase: Erase) -> Entry {
        let after = snapshot(&self.board, &erase.followers);
        let mut before = erase.touched;
        before.extend(erase.gone);
        Entry { before, after }
    }

    fn drag(&mut self, p: Point, shift: bool, now: Duration) {
        match std::mem::replace(&mut self.gesture, Gesture::None) {
            Gesture::Drawing { mut smooth, mut el } => {
                let q = smooth.push(p);
                if let Shape::Stroke { points, .. } = &mut el.shape {
                    points.push(q);
                }
                self.gesture = Gesture::Drawing { smooth, el };
                self.touch();
            }
            Gesture::Shaping { start, mut el } => {
                let b = constrain(start, p, shift, &el.shape);
                match &mut el.shape {
                    Shape::Rect { b: end, .. }
                    | Shape::Ellipse { b: end, .. }
                    | Shape::Line { b: end, .. } => *end = b,
                    _ => {}
                }
                self.gesture = Gesture::Shaping { start, el };
                self.touch();
            }
            Gesture::Erasing(mut erase) => {
                self.erase_at(p, now, &mut erase);
                self.gesture = Gesture::Erasing(erase);
            }
            Gesture::Marquee { start, .. } => {
                self.gesture = Gesture::Marquee { start, now: p };
                self.touch();
            }
            Gesture::Moving {
                last, before, ids, ..
            } => {
                let d = p - last;
                self.translate(&ids, d);
                self.redraw();
                self.gesture = Gesture::Moving {
                    last: p,
                    before,
                    ids,
                    moved: true,
                };
            }
            other => self.gesture = other,
        }
    }

    fn release(&mut self, p: Point, shift: bool, now: Duration) -> Effect {
        let cell = self.cell_size();
        match std::mem::replace(&mut self.gesture, Gesture::None) {
            Gesture::None | Gesture::Swallowed => {}
            Gesture::Drawing { mut el, .. } => {
                if let Shape::Stroke { points, .. } = &mut el.shape {
                    // The stroke ends where the pointer lifted, not short of it as the low-pass
                    // leaves the last smoothed point.
                    if points.last() != Some(&p) {
                        points.push(p);
                    }
                    *points = simplify(points, SIMPLIFY);
                }
                self.add_committed(el, now);
            }
            Gesture::Shaping { start, mut el } => {
                let b = constrain(start, p, shift, &el.shape);
                let flat = match &mut el.shape {
                    Shape::Rect { a, b: end } | Shape::Ellipse { a, b: end } => {
                        *end = b;
                        a.x == end.x || a.y == end.y
                    }
                    Shape::Line { a, b: end, .. } => {
                        *end = b;
                        *a == *end
                    }
                    _ => true,
                };
                if !flat {
                    self.board.bind(&mut el);
                    self.add_committed(el, now);
                } else {
                    self.touch();
                }
                self.tool = Tool::Pen;
            }
            Gesture::Erasing(mut erase) => {
                self.erase_at(p, now, &mut erase);
                let entry = self.erased(erase);
                self.history.push(entry);
            }
            Gesture::Marquee { start, .. } => {
                self.selection = self.board.within(&Rect::from_points(start, p), cell);
                self.redraw();
            }
            Gesture::Moving {
                before, ids, moved, ..
            } => {
                if moved {
                    let after = snapshot(&self.board, &ids);
                    self.commit(Entry { before, after }, now, Damage::All);
                } else {
                    self.redraw();
                }
            }
        }
        Effect::None
    }

    /// Put a gesture's element on the board, with a fresh id, and log it.
    fn add_committed(&mut self, el: Element, now: Duration) {
        let cell = self.cell_size();
        let (i, _) = self.board.add(el.shape, el.ink, el.width);
        let added = self.board.elements[i].clone();
        let region = added.bounds(cell);
        self.commit(Entry::added(i, added), now, Damage::Region(region));
    }

    /// Move `ids` by `d`: the selected ones whole, the rest (lines bound to them) by their
    /// bound ends.
    fn translate(&mut self, ids: &[Id], d: Point) {
        let selected = self.selection.clone();
        for id in ids {
            let Some(el) = self.board.get_mut(*id) else {
                continue;
            };
            if selected.contains(id) {
                el.translate(d);
            } else {
                for s in &selected {
                    el.follow(*s, d);
                }
            }
        }
    }

    // -------------------------------------------------------------- labels

    fn begin_label(&mut self, at: Point) -> Effect {
        let (_, id) = self.board.add(
            Shape::Label {
                at,
                text: String::new(),
            },
            self.ink,
            self.width,
        );
        self.label = Some(LabelEdit { id, before: None });
        self.selection.clear();
        self.redraw();
        Effect::EnterInsert
    }

    fn edit_label(&mut self, id: Id) -> Effect {
        let before = self
            .board
            .index_of(id)
            .map(|i| (i, self.board.elements[i].clone()));
        self.label = Some(LabelEdit { id, before });
        self.selection.clear();
        self.redraw();
        Effect::EnterInsert
    }

    fn label_text(&mut self) -> Option<&mut String> {
        let id = self.label.as_ref()?.id;
        match &mut self.board.get_mut(id)?.shape {
            Shape::Label { text, .. } => Some(text),
            _ => None,
        }
    }

    /// Put `s` at the end of the label being typed. False with none.
    pub fn label_type(&mut self, s: &str, now: Duration) -> bool {
        let Some(text) = self.label_text() else {
            return false;
        };
        text.push_str(s);
        self.changed(now, Damage::All);
        true
    }

    /// Take the last character off the label being typed.
    pub fn label_backspace(&mut self, now: Duration) -> bool {
        let Some(text) = self.label_text() else {
            return false;
        };
        text.pop();
        self.changed(now, Damage::All);
        true
    }

    /// End the label being typed: it stays if it has text, goes if it hasn't, and the pen comes
    /// back. Nothing happens with none.
    pub fn end_label(&mut self, now: Duration) -> Effect {
        let Some(edit) = self.label.take() else {
            return Effect::None;
        };
        self.tool = Tool::Pen;
        let Some(i) = self.board.index_of(edit.id) else {
            self.redraw();
            return Effect::LeaveInsert;
        };
        let el = self.board.elements[i].clone();
        let empty = matches!(&el.shape, Shape::Label { text, .. } if text.is_empty());
        let before: Vec<(usize, Element)> = edit.before.into_iter().collect();
        let after = if empty {
            self.board.remove(edit.id);
            vec![]
        } else {
            vec![(i, el)]
        };
        let same = before
            .iter()
            .map(|(_, e)| e)
            .eq(after.iter().map(|(_, e)| e));
        if same {
            self.redraw();
        } else {
            self.commit(Entry { before, after }, now, Damage::All);
        }
        Effect::LeaveInsert
    }

    /// Where the caret is while a label is typed: the cell after its last character.
    pub fn caret(&self, cell: CellPixels) -> Option<(usize, usize)> {
        let id = self.label.as_ref()?.id;
        let Shape::Label { at, text } = &self.board.get(id)?.shape else {
            return None;
        };
        let lines = label_lines(text);
        let size = cell_size(cell);
        let last = lines.last().map_or(0, |l| l.chars().count()) as f32;
        let p = Point::new(
            at.x + last * size.w,
            at.y + (lines.len() - 1) as f32 * size.h,
        );
        let (x, y) = self.view.to_physical(p, cell.scale);
        let (cw, ch) = (
            size.w * cell.scale.max(1.0) as f32,
            size.h * cell.scale.max(1.0) as f32,
        );
        let (col, row) = ((x / cw).floor(), (y / ch).floor());
        (col >= 0.0 && row >= 0.0).then_some((row as usize, col as usize))
    }

    // -------------------------------------------------------------- commands

    /// Pick a tool. A gesture under way is dropped.
    pub fn set_tool(&mut self, tool: Tool) {
        self.gesture = Gesture::None;
        self.tool = tool;
        self.touch();
    }

    /// Pick `tool`, or the pen if it's the tool already: `e` and `h`.
    pub fn toggle_tool(&mut self, tool: Tool) {
        let next = if self.tool == tool { Tool::Pen } else { tool };
        self.set_tool(next);
    }

    /// Esc in normal state: drop the gesture and the selection, back to the pen.
    pub fn escape(&mut self) {
        self.gesture = Gesture::None;
        self.selection.clear();
        self.tool = Tool::Pen;
        self.redraw();
    }

    /// Choose an ink, `1`..=6, for what's drawn next and for the selection.
    pub fn set_ink(&mut self, ink: u8, now: Duration) {
        self.ink = ink.clamp(1, INKS);
        self.restyle(now, |e, ink, _| e.ink = ink);
    }

    /// Step the width `by` in [`WIDTHS`], for what's drawn next and for the selection.
    pub fn step_width(&mut self, by: i8, now: Duration) {
        let last = (WIDTHS.len() - 1) as i8;
        self.width = (self.width as i8 + by).clamp(0, last) as u8;
        self.restyle(now, |e, _, width| e.width = width);
    }

    /// Change the selection's ink or width, as one entry.
    fn restyle(&mut self, now: Duration, f: impl Fn(&mut Element, u8, u8)) {
        if self.selection.is_empty() {
            self.touch();
            return;
        }
        let before = snapshot(&self.board, &self.selection);
        for (_, el) in &before {
            if let Some(e) = self.board.get_mut(el.id) {
                f(e, self.ink, self.width);
            }
        }
        let after = snapshot(&self.board, &self.selection);
        self.commit(Entry { before, after }, now, Damage::All);
    }

    /// Take back the last change. False with none.
    pub fn undo(&mut self, now: Duration) -> bool {
        self.gesture = Gesture::None;
        let done = self.history.undo(&mut self.board).is_some();
        if done {
            self.selection.retain(|id| self.board.get(*id).is_some());
            self.changed(now, Damage::All);
        }
        done
    }

    /// Do the last undone change again. False with none.
    pub fn redo(&mut self, now: Duration) -> bool {
        self.gesture = Gesture::None;
        let done = self.history.redo(&mut self.board).is_some();
        if done {
            self.selection.retain(|id| self.board.get(*id).is_some());
            self.changed(now, Damage::All);
        }
        done
    }

    /// Delete the selection. False with none.
    pub fn delete_selection(&mut self, now: Duration) -> bool {
        if self.selection.is_empty() {
            return false;
        }
        let ids = std::mem::take(&mut self.selection);
        let mut erase = Erase::default();
        for id in ids {
            self.remove_one(id, &mut erase, now);
        }
        let entry = self.erased(erase);
        self.history.push(entry);
        true
    }

    /// Move the selection by cells. False with none.
    pub fn nudge(&mut self, dc: i32, dr: i32, now: Duration) -> bool {
        if self.selection.is_empty() {
            return false;
        }
        let cell = self.cell_size();
        let d = Point::new(dc as f32 * cell.w, dr as f32 * cell.h);
        let mut ids = self.selection.clone();
        ids.extend(self.board.followers(&self.selection));
        let before = snapshot(&self.board, &ids);
        self.translate(&ids, d);
        let after = snapshot(&self.board, &ids);
        self.commit(Entry { before, after }, now, Damage::All);
        true
    }

    /// Copy the selection a cell down and right, and select the copies. False with none.
    pub fn duplicate(&mut self, now: Duration) -> bool {
        if self.selection.is_empty() {
            return false;
        }
        let cell = self.cell_size();
        let d = Point::new(cell.w, cell.h);
        let originals: Vec<Element> = self
            .board
            .elements
            .iter()
            .filter(|e| self.selection.contains(&e.id))
            .cloned()
            .collect();
        let mut map: Vec<(Id, Id)> = vec![];
        let mut copies = vec![];
        for el in &originals {
            let id = self.board.fresh_id();
            map.push((el.id, id));
            let mut copy = Element { id, ..el.clone() };
            copy.translate(d);
            copies.push(copy);
        }
        for copy in &mut copies {
            if let Shape::Line { from, to, .. } = &mut copy.shape {
                for end in [from, to] {
                    *end = end.and_then(|old| map.iter().find(|(o, _)| *o == old).map(|m| m.1));
                }
            }
        }
        let mut after = vec![];
        for copy in copies {
            let i = self.board.len();
            self.board.insert(i, copy.clone());
            after.push((i, copy));
        }
        self.selection = after.iter().map(|(_, e)| e.id).collect();
        self.commit(
            Entry {
                before: vec![],
                after,
            },
            now,
            Damage::All,
        );
        true
    }

    /// The wheel: pan by cells.
    pub fn pan_cells(&mut self, dx: f32, dy: f32) {
        if !dx.is_finite() || !dy.is_finite() || (dx == 0.0 && dy == 0.0) {
            return;
        }
        let size = self.cell_size();
        let f = self.view.factor(self.cell.scale);
        self.view.pan_by(-dx * size.w * f, -dy * size.h * f);
        self.redraw();
    }

    /// Zoom by `by` about a physical pixel of the area, or its centre.
    pub fn zoom(&mut self, by: f32, about: Option<(f32, f32)>) {
        let about = about.unwrap_or_else(|| {
            let size = self.cell_size();
            let f = self.view.factor(self.cell.scale);
            (
                self.area.0 as f32 * size.w * f / 2.0,
                self.area.1 as f32 * size.h * f / 2.0,
            )
        });
        self.view.zoom_by(by, about, self.cell.scale);
        self.redraw();
    }

    /// Back to no zoom and no pan.
    pub fn reset_view(&mut self) {
        self.view = View::default();
        self.redraw();
    }
}

/// The surface: the shared state, and a renderer drawing it.
pub struct BoardSurface {
    /// The state.
    pub state: Shared,
    renderer: Box<dyn Renderer>,
    drawn: Option<u64>,
    drawn_board: u64,
}

impl BoardSurface {
    /// A surface on `state`, drawn by the pixel renderer if there is one, else by cells.
    pub fn new(state: Shared) -> BoardSurface {
        let renderer = crate::pixels::new().unwrap_or_else(|| Box::new(Cells::new()));
        state.borrow_mut().zooms = renderer.zooms();
        BoardSurface {
            state,
            renderer,
            drawn: None,
            drawn_board: 0,
        }
    }

    /// Which renderer draws it.
    pub fn renderer_name(&self) -> &'static str {
        self.renderer.name()
    }

    fn apply(&self, effect: Effect, ctx: &mut Ctx) {
        let Some(buf) = self.state.borrow().buf else {
            return;
        };
        match effect {
            Effect::None => {}
            Effect::EnterInsert => ctx.set_state_of(buf, State::Insert),
            Effect::LeaveInsert => ctx.set_state_of(buf, State::Normal),
        }
    }

    fn pointer(&mut self, p: &SurfacePointer, ctx: &mut Ctx) {
        let now = ctx.now();
        let effect = {
            let mut st = self.state.borrow_mut();
            let point = st.view.to_board(p.x, p.y, st.cell.scale);
            st.pointer(p.kind, point, p.clicks, p.mods, now)
        };
        self.apply(effect, ctx);
        let buf = self.state.borrow().buf;
        crate::after_change(ctx, buf);
    }

    fn typed(&mut self, text: &str, ctx: &mut Ctx) -> bool {
        let now = ctx.now();
        let took = self.state.borrow_mut().label_type(text, now);
        if took {
            let buf = self.state.borrow().buf;
            crate::after_change(ctx, buf);
        }
        took
    }
}

impl Surface for BoardSurface {
    fn kind(&self) -> &'static str {
        KIND
    }

    fn render(&mut self, frame: &mut Frame, info: &RenderInfo) {
        let mut st = self.state.borrow_mut();
        if st.board_rev != self.drawn_board {
            let damage = std::mem::take(&mut st.damage);
            self.renderer.board_changed(if damage == Damage::None {
                Damage::All
            } else {
                damage
            });
            self.drawn_board = st.board_rev;
        }
        if !frame.is_new() && self.drawn == Some(st.rev) {
            return;
        }
        let scene = Scene {
            board: &st.board,
            view: &st.view,
            inks: &st.inks,
            selection: &st.selection,
            live: st.live(),
            marquee: st.marquee(),
            cell: st.cell_size(),
        };
        self.renderer.render(frame, &scene, info);
        self.drawn = Some(st.rev);
    }

    /// The caret while a label is typed; otherwise none.
    fn cursor(&self, info: &RenderInfo) -> Option<SurfaceCursor> {
        let (row, col) = self.state.borrow().caret(info.cell)?;
        Some(SurfaceCursor {
            row,
            col,
            shape: CursorShape::Bar,
            visible: true,
            blinking: false,
        })
    }

    fn status(&self) -> Option<String> {
        Some(self.state.borrow().status())
    }

    fn event(&mut self, ev: &Event, ctx: &mut Ctx) -> bool {
        self.state.borrow_mut().inks = crate::ink_colors(ctx);
        match ev {
            Event::Pointer(p) => {
                self.pointer(p, ctx);
                true
            }
            Event::Resize {
                cols, rows, cell, ..
            } => {
                let mut st = self.state.borrow_mut();
                st.cell = *cell;
                st.area = (*cols, *rows);
                self.renderer.resize(*cols, *rows, *cell);
                st.redraw();
                true
            }
            Event::Focus(gained) => {
                let mut st = self.state.borrow_mut();
                st.focused = *gained;
                if !gained && st.gesturing() {
                    st.gesture = Gesture::None;
                    st.touch();
                }
                true
            }
            Event::Key(k) if k.kind == KeyKind::Press => match k.printable() {
                Some(text) if self.state.borrow().editing().is_some() => self.typed(&text, ctx),
                _ => false,
            },
            Event::Paste(text) if self.state.borrow().editing().is_some() => self.typed(text, ctx),
            _ => false,
        }
    }

    fn handles(&self, key: &KeyEvent) -> bool {
        self.state.borrow().editing().is_some() && key.printable().is_some()
    }

    fn takes_wheel(&self) -> bool {
        true
    }
}
