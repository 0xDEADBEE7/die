//! What die-render drives core through (K9): `handle` for input, `run_ready` for the turn loop,
//! `next_deadline` for how long it can sleep, and `scene` for what to draw.
//!
//! A host's loop is: read its events into `handle`, call `run_ready`, draw `scene`, then sleep for
//! `next_deadline` or until the event loop wakes it, by an event or by [`Ctx::set_waker`]'s
//! function. It never waits inside core, and it reads the time from the clock core was given, so
//! none of these takes a `now`. `Ctx::step` and `Ctx::compose` are its parts, for a host that wants
//! them on their own.
//!
//! The pointer on text windows is here too (K14). A left press focuses the window under it, stops
//! a selection or a waiting operator as the keymap's `<Esc>` does, and places the cursor. A drag
//! that leaves that cell selects in visual state, and a scroll moves the view of the window under
//! it, without the focus.
#![warn(missing_docs)]
use crate::ext::*;
use crate::ids::*;
use crate::keymap::{Lookup, State};
use crate::keys::{KeyEvent, Mods, parse_keys};
use crate::pointer::{CellHit, WindowPart};
use crate::scene::Scene;
use crate::text::{Selection, Shape, TextBuf};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::time::Duration;

/// A pointer button.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Button {
    /// The left button, or a plain click on a trackpad.
    Left,
    /// The middle button.
    Middle,
    /// The right button, or a secondary click.
    Right,
}

/// What the pointer did.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PointerKind {
    /// A button went down.
    Press(Button),
    /// A button came up.
    Release(Button),
    /// It moved with a button down.
    Drag(Button),
    /// It moved with no button down. This doesn't count as activity for idle timers.
    Move,
    /// A scroll, in precise rows and columns of cells, as a trackpad gives them.
    Scroll {
        /// Rightward cells.
        dx: f32,
        /// Downward cells.
        dy: f32,
    },
}

/// One pointer event, at a cell of the frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PointerEvent {
    /// What it did.
    pub kind: PointerKind,
    /// The cell's row in the frame.
    pub row: usize,
    /// The cell's column in the frame.
    pub col: usize,
    /// 1 for a click, 2 for a double click, and so on. 0 for anything but a press or a release.
    pub clicks: u8,
    /// The modifiers held.
    pub mods: Mods,
}

/// Input from the host, handled in the order it came, in its turn among the keys.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum Input {
    /// A key from the OS.
    Key(KeyEvent),
    /// The pointer, at a cell of the frame.
    Pointer(PointerEvent),
    /// DIE's OS window gained or lost the keyboard focus. Without it, every cursor is drawn
    /// `Hollow`, as Emacs draws an unfocused frame, and the focused window's surface hears.
    Focus(bool),
    /// DIE's OS window has this many cells now, with the command line's row.
    Resize {
        /// Its columns.
        cols: usize,
        /// Its rows.
        rows: usize,
    },
}

/// How many turns one [`Ctx::run_ready`] takes before it lets the host draw. A long paste or macro
/// reaches this, a key a turn, and the rest of it comes after the frame. So do queued calls with no
/// key between them, which usually means a call that keeps queueing another, and the frame should
/// still come.
pub const TURN_LIMIT: usize = 10_000;

/// What the pointer leaves behind between its events.
#[derive(Default)]
pub(crate) struct PointerState {
    /// The window a left press placed the cursor in, and where, until the release.
    pressed: Option<(WindowId, usize)>,
    /// How far a trackpad has scrolled short of a whole row.
    scroll: f32,
    /// [`Ctx::set_cursor_rest`]'s rule.
    rest: Option<fn(&TextBuf, usize) -> usize>,
}

impl Ctx {
    /// Say where the pointer or a restored session may leave a cursor in normal state: `rule`
    /// takes a position in the text and gives the one to use. Which positions a cursor may rest on is the keymap's
    /// business (contract §9). With no rule, anywhere the scene maps a cell to.
    pub fn set_cursor_rest(&mut self, rule: fn(&TextBuf, usize) -> usize) {
        self.pointer.rest = Some(rule);
    }

    pub(crate) fn rested(&mut self, buf: BufferId, pos: usize) -> usize {
        let rule = self
            .pointer
            .rest
            .filter(|_| self.state_of(buf) == State::Normal);
        let (Some(rule), Ok(t)) = (rule, self.text(buf)) else {
            return pos;
        };
        catch_unwind(AssertUnwindSafe(|| rule(t, pos))).unwrap_or_else(|_| {
            self.log_error("The cursor rest rule panicked".into());
            pos
        })
    }

    /// Run what the keymap binds `<Esc>` to in the focused window, as if typed, but unseen by
    /// key observers, so it's in no macro.
    fn pointer_escape(&mut self) {
        let Some((win, buf)) = self.focused.zip(self.focused_buffer()) else {
            return;
        };
        let esc = parse_keys("<Esc>").expect("good notation");
        let state = self.state_of(buf);
        if let Lookup::Command(name) = self.layers_lookup(buf, state, &esc) {
            self.run_key(&name, esc, win, buf);
        }
    }

    /// End a selection or a waiting operator in the focused window. A keymap that binds no
    /// `<Esc>` there still gets normal state.
    fn stop_selecting(&mut self) {
        let waiting = |c: &Ctx| matches!(c.state(), State::Visual | State::OperatorPending);
        if waiting(self) {
            self.pointer_escape();
        }
        if waiting(self) {
            self.set_state(State::Normal);
        }
    }

    fn pointer_press(&mut self, hit: CellHit) {
        self.pending.clear();
        self.prefix = Default::default();
        self.stop_selecting();
        self.focus(hit.window);
        self.stop_selecting();
        let Some(buf) = self.window_buffer(hit.window) else {
            return;
        };
        let Some(pos) = hit.pos.filter(|_| hit.part == WindowPart::Text) else {
            return;
        };
        let to = self.rested(buf, pos);
        if let Ok(t) = self.text_mut(buf) {
            let at = t.set_cursor(hit.window, to);
            self.pointer.pressed = at.map(|at| (hit.window, at));
        }
    }

    fn pointer_drag(&mut self, hit: CellHit) {
        let Some((w, anchor)) = self.pointer.pressed.filter(|p| p.0 == hit.window) else {
            return;
        };
        let Some(head) = hit.pos.filter(|_| hit.part == WindowPart::Text) else {
            return;
        };
        let Some(buf) = self.window_buffer(w) else {
            return;
        };
        let state = self.state_of(buf);
        if state != State::Visual && head == anchor {
            return;
        }
        if matches!(state, State::Insert | State::Replace) {
            self.pointer_escape();
        }
        self.set_state_of(buf, State::Visual);
        if let Ok(t) = self.text_mut(buf) {
            let shape = Shape::Char;
            let goal = None;
            let sel = Selection {
                anchor,
                head,
                shape,
                goal,
            };
            t.set_selection(w, sel);
        }
    }

    /// Scroll by whole rows once they add up. A cursor the view leaves behind comes to the row
    /// it left by, at its column, since core would otherwise scroll back to it (C5).
    fn pointer_scroll(&mut self, w: WindowId, dy: f32) {
        if !dy.is_finite() {
            return;
        }
        self.pointer.scroll += dy;
        let rows = self.pointer.scroll.trunc();
        self.pointer.scroll -= rows;
        let Some(buf) = self.window_buffer(w).filter(|_| rows != 0.0) else {
            return;
        };
        let Ok(t) = self.text_mut(buf) else { return };
        t.scroll_rows(w, rows as isize);
        let (Some(v), Some(sel)) = (t.viewport(w), t.selection(w)) else {
            return;
        };
        if t.screen_pos(w, sel.head).is_some() {
            return;
        }
        let row = if sel.head < v.top {
            v.rows.first()
        } else {
            v.rows.last()
        };
        let goal = sel.goal.unwrap_or_else(|| t.display_col(sel.head));
        // The goal's top bit is its owner's, die-vim's mark for a column on the screen row.
        let want = (goal & (usize::MAX >> 1)).min(v.geometry.text_w.saturating_sub(1));
        let stops = row.map_or(&[][..], |r| r.stops());
        let Some(stop) = stops.iter().rfind(|s| s.col <= want).or(stops.first()) else {
            return;
        };
        let head = self.rested(buf, stop.pos);
        if let Ok(t) = self.text_mut(buf) {
            let goal = Some(goal);
            t.set_selection(w, Selection { head, goal, ..sel });
        }
    }

    fn run_pointer(&mut self, p: PointerEvent) {
        let hit = self.cell_at(p.row, p.col);
        match (p.kind, hit) {
            (PointerKind::Release(Button::Left), _) => self.pointer.pressed = None,
            (PointerKind::Scroll { dy, .. }, Some(hit)) => self.pointer_scroll(hit.window, dy),
            // The prompt has the keys, and it stays on the window it opened in.
            _ if self.prompt.is_some() => {}
            (PointerKind::Press(Button::Left), _) => {
                self.pointer.pressed = None;
                if let Some(hit) = hit {
                    self.pointer_press(hit);
                }
            }
            (PointerKind::Drag(Button::Left), Some(hit)) => self.pointer_drag(hit),
            _ => {}
        }
    }

    /// Take input from the host. It's handled by the next turn of the loop.
    pub fn handle(&mut self, input: Input) {
        self.typed.push_back(input);
    }

    /// One [`Input`] other than a key, in its turn.
    pub(crate) fn run_input(&mut self, input: Input) {
        match input {
            Input::Key(_) => unreachable!("a key is dispatched in `step`"),
            Input::Pointer(p) => {
                self.run_pointer(p);
                if p.kind != PointerKind::Move {
                    self.restart_idle();
                }
            }
            Input::Focus(on) => {
                if self.app_focused != on {
                    self.app_focused = true; // the one time a surface hears that it's away
                    if let Some(b) = self.focused_buffer() {
                        self.focus_event(b, on);
                    }
                    self.app_focused = on;
                }
            }
            Input::Resize { cols, rows } => self.set_frame_size(cols, rows),
        }
    }

    /// Take turns until nothing is ready, and return whether anything ran, which is whether
    /// there's a new frame to draw. It never waits: a job still out finishes later, and wakes the
    /// host. It stops after [`TURN_LIMIT`] turns, and what's left goes on after the next frame. If
    /// none of those turns took a key or other input, queued calls had the loop to themselves, so
    /// that gets one line in `*messages*`, until the loop next goes idle.
    pub fn run_ready(&mut self) -> bool {
        let mut ran = false;
        self.input_ran = false;
        for _ in 0..TURN_LIMIT {
            if !self.step() {
                self.over_limit = false;
                return ran;
            }
            ran = true;
        }
        if !self.over_limit && !self.input_ran {
            self.over_limit = true;
            self.messages.push(format!(
                "Queued calls ran for {TURN_LIMIT} turns with no key between them, so the frame went first"
            ));
        }
        true
    }

    /// How long the host can sleep before it must call [`Ctx::run_ready`], or None if nothing is
    /// due until it's woken. Zero if a timer is overdue or something's ready to run.
    pub fn next_deadline(&mut self) -> Option<Duration> {
        if !self.queue.is_empty() || !self.fed.is_empty() || !self.typed.is_empty() {
            return Some(Duration::ZERO);
        }
        let at = self.next_timer()?;
        Some(at.saturating_sub(self.now()))
    }

    /// A fresh frame of the split tree's windows at the frame's size, owned by the caller.
    /// die-render keeps the previous one and rebuilds only the rows whose cells differ, so core
    /// keeps no render state but the map [`Ctx::cell_at`] reads.
    pub fn scene(&mut self) -> Scene {
        let (cols, rows) = self.tree.frame;
        let placed = self.placements();
        self.compose(cols, rows, &placed)
    }
}
