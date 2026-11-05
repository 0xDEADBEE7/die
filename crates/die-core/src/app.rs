//! What die-render drives core through (K9): `handle` for input, `run_ready` for the turn loop,
//! `next_deadline` for how long it can sleep, and `scene` for what to draw.
//!
//! A host's loop is: read its events into `handle`, call `run_ready`, draw `scene`, then sleep for
//! `next_deadline` or until the event loop wakes it, by an event or by [`Ctx::set_waker`]'s
//! function. It never waits inside core, and it reads the time from the clock core was given, so
//! none of these takes a `now`. `Ctx::step` and `Ctx::compose` are its parts, for a host that wants
//! them on their own.
#![warn(missing_docs)]
use crate::ext::*;
use crate::keys::{KeyEvent, Mods};
use crate::scene::Scene;
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

impl Ctx {
    /// Take input from the host. It's handled by the next turn of the loop.
    pub fn handle(&mut self, input: Input) {
        self.typed.push_back(input);
    }

    /// One [`Input`] other than a key, in its turn.
    pub(crate) fn run_input(&mut self, input: Input) {
        match input {
            Input::Key(_) => unreachable!("a key is dispatched in `step`"),
            Input::Pointer(p) => {
                // What a click does is K14's. For now it's only activity.
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
