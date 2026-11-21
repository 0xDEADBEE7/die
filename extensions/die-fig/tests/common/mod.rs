//! What the surface's tests share: a harness with die-vim and fig (times in UTC), a CSV open and
//! charted by `fig-toggle`, the pointer in the area's logical px, and what the fake pixel sink
//! heard.
#![allow(dead_code)]

use die_core::{
    Args, BufferId, Button, CellPixels, Harness, Input, KeyEvent, Mods, PixelBufferId, PixelRect,
    PixelSinkCall, PointerEvent, PointerKind, WindowId,
};
use die_fig::Fig;
use die_fig::interact::{Cursor, View};
use die_fig::scene::Rect;
use die_fig::surface::Shared;
use die_vim::Vim;
use jiff::tz::TimeZone;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// The cells the tests' host has: 10 × 20 physical pixels on a 2x display, so a cell is 5 × 10
/// logical px, as the whiteboard's tests have them.
pub const CELL: CellPixels = CellPixels {
    width: 10,
    height: 20,
    scale: 2.0,
};

/// Two series over an ordered x: `a` rises from 0 to 20 and `b` falls from 20 to 0, so they
/// cross at x = 10.
pub fn crossing() -> String {
    let mut s = String::from("x,a,b\n");
    for i in 0..=20 {
        s.push_str(&format!("{i},{i},{}\n", 20 - i));
    }
    s
}

/// A directory of a test's own, removed when it drops.
pub struct TempDir(PathBuf);

impl TempDir {
    pub fn new(name: &str) -> TempDir {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("die-fig-{name}-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        TempDir(dir.canonicalize().unwrap())
    }
    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// die-vim and fig in a harness, a 100 × 30 window, the cells known.
pub fn harness() -> Harness {
    let exts: Vec<Box<dyn die_core::Extension>> = vec![
        Box::new(Vim::default()),
        Box::new(Fig::with_zone(TimeZone::UTC)),
    ];
    let mut h = Harness::new(exts).unwrap();
    h.set_window_size(100, 30);
    h.set_cell_pixels(CELL);
    h.run_until_idle();
    h
}

pub struct Setup {
    pub h: Harness,
    /// The source buffer.
    pub src: BufferId,
    /// The figure's buffer.
    pub buf: BufferId,
    pub win: WindowId,
}

/// `name` holding `text`, charted by `fig-toggle` in the window, drawn.
pub fn setup_with(name: &str, text: &str) -> Setup {
    let mut h = harness();
    let (src, win) = h.open_text(name, text).unwrap();
    h.run_until_idle();
    let errors = run(&mut h, "fig-toggle");
    assert!(errors.is_empty(), "{errors:?}");
    let buf = h.window_buffer(win).unwrap();
    assert_ne!(buf, src, "the figure shows in the window");
    Setup { h, src, buf, win }
}

/// [`crossing`] in `data.csv`, charted.
pub fn setup() -> Setup {
    setup_with("data.csv", &crossing())
}

/// A command run to the end, and its errors.
pub fn run(h: &mut Harness, name: &str) -> Vec<String> {
    let mut errors = vec![];
    if let Err(e) = h.run(name, &Args::default()) {
        errors.push(e.to_string());
    }
    h.run_until_idle();
    h.scene();
    errors.extend(h.take_errors());
    errors
}

/// The figure's state.
pub fn state(s: &mut Setup) -> Shared {
    let buf = s.buf;
    s.h.with_ext(|f: &mut Fig, _| f.figure(buf))
        .unwrap()
        .expect("a figure")
}

/// The window's view.
pub fn view(s: &mut Setup) -> View {
    let st = state(s);
    let w = s.win;
    st.borrow().panes[&w].view.clone()
}

/// The window's cursor.
pub fn cursor(s: &mut Setup) -> Option<Cursor> {
    let st = state(s);
    let w = s.win;
    st.borrow().panes[&w].cursor.clone()
}

/// The window's hover.
pub fn hover(s: &mut Setup) -> Option<Cursor> {
    let st = state(s);
    let w = s.win;
    st.borrow().panes[&w].hover.clone()
}

/// The plot's rectangle, as last drawn.
pub fn plot(s: &mut Setup) -> Rect {
    let st = state(s);
    let w = s.win;
    st.borrow().panes[&w].layout.as_ref().expect("drawn").plot
}

/// Where a sample of a series is on screen, in logical px, as last drawn.
pub fn sample_at(s: &mut Setup, series: &str, row: usize) -> (f32, f32) {
    let st = state(s);
    let w = s.win;
    let st = st.borrow();
    let model = st.model.as_ref().expect("a model");
    let info = model
        .series
        .iter()
        .find(|i| &*i.key.name == series)
        .unwrap();
    let l = st.panes[&w].layout.as_ref().unwrap();
    (l.x.map(info.xs[row]), l.y.map(info.ys[row]))
}

/// The two buffers of the window: base and overlay.
pub fn buffers(s: &mut Setup) -> (PixelBufferId, PixelBufferId) {
    let st = state(s);
    let w = s.win;
    st.borrow().panes[&w].buffers().expect("buffers")
}

/// The area's origin in the frame's cells.
pub fn origin(h: &mut Harness, w: WindowId) -> (usize, usize) {
    let p = h.placements().into_iter().find(|p| p.window == w).unwrap();
    let g = h.surface_area(w).unwrap();
    (p.row, p.col + g.text_x)
}

/// A pointer event at a logical point of the window's area.
pub fn pointer(s: &mut Setup, kind: PointerKind, (lx, ly): (f32, f32), mods: Mods, clicks: u8) {
    let errors = pointer_errors(s, kind, (lx, ly), mods, clicks);
    assert!(errors.is_empty(), "{kind:?} at ({lx}, {ly}): {errors:?}");
}

/// [`pointer`], with the errors it caused.
pub fn pointer_errors(
    s: &mut Setup,
    kind: PointerKind,
    (lx, ly): (f32, f32),
    mods: Mods,
    clicks: u8,
) -> Vec<String> {
    let (r0, c0) = origin(&mut s.h, s.win);
    let scale = CELL.scale as f32;
    let (x, y) = (lx * scale, ly * scale);
    let px = (
        x + c0 as f32 * CELL.width as f32,
        y + r0 as f32 * CELL.height as f32,
    );
    let row = r0 + (y / CELL.height as f32).floor().max(0.0) as usize;
    let col = c0 + (x / CELL.width as f32).floor().max(0.0) as usize;
    s.h.send(Input::Pointer(PointerEvent {
        kind,
        row,
        col,
        clicks,
        mods,
        px: Some(px),
    }))
}

pub fn mv(s: &mut Setup, at: (f32, f32)) {
    pointer(s, PointerKind::Move, at, Mods::default(), 0);
}

pub fn press(s: &mut Setup, at: (f32, f32)) {
    pointer(s, PointerKind::Press(Button::Left), at, Mods::default(), 1);
}

pub fn drag(s: &mut Setup, at: (f32, f32)) {
    pointer(s, PointerKind::Drag(Button::Left), at, Mods::default(), 0);
}

pub fn release(s: &mut Setup, at: (f32, f32)) {
    pointer(
        s,
        PointerKind::Release(Button::Left),
        at,
        Mods::default(),
        1,
    );
}

pub fn click(s: &mut Setup, at: (f32, f32)) {
    click_with(s, at, Mods::default());
}

pub fn click_with(s: &mut Setup, at: (f32, f32), mods: Mods) {
    pointer(s, PointerKind::Press(Button::Left), at, mods, 1);
    pointer(s, PointerKind::Release(Button::Left), at, mods, 1);
}

/// A double click: a click, then a press counting two, and its release.
pub fn double_click(s: &mut Setup, at: (f32, f32)) {
    click(s, at);
    pointer(s, PointerKind::Press(Button::Left), at, Mods::default(), 2);
    pointer(
        s,
        PointerKind::Release(Button::Left),
        at,
        Mods::default(),
        2,
    );
}

/// A press at the first point, drags through the rest, a release at the last.
pub fn gesture_with(s: &mut Setup, points: &[(f32, f32)], mods: Mods, button: Button) {
    let (first, last) = (points[0], points[points.len() - 1]);
    pointer(s, PointerKind::Press(button), first, mods, 1);
    for p in &points[1..] {
        pointer(s, PointerKind::Drag(button), *p, mods, 0);
    }
    pointer(s, PointerKind::Release(button), last, mods, 1);
}

pub fn gesture(s: &mut Setup, points: &[(f32, f32)]) {
    gesture_with(s, points, Mods::default(), Button::Left);
}

pub fn scroll(s: &mut Setup, at: (f32, f32), (dx, dy): (f32, f32), mods: Mods) {
    pointer(s, PointerKind::Scroll { dx, dy }, at, mods, 0);
}

pub const SHIFT: Mods = Mods {
    ctrl: false,
    alt: false,
    shift: true,
    super_: false,
};
pub const ALT: Mods = Mods {
    ctrl: false,
    alt: true,
    shift: false,
    super_: false,
};
pub const CMD: Mods = Mods {
    ctrl: false,
    alt: false,
    shift: false,
    super_: true,
};
pub const CTRL: Mods = Mods {
    ctrl: true,
    alt: false,
    shift: false,
    super_: false,
};

/// Keys that must all work.
pub fn keys(h: &mut Harness, k: &str) {
    for log in h.type_keys(k).unwrap() {
        assert!(log.errors.is_empty(), "{}: {:?}", log.key, log.errors);
    }
}

/// The keys, with the errors they caused.
pub fn keys_errors(h: &mut Harness, k: &str) -> Vec<String> {
    h.type_keys(k)
        .unwrap()
        .into_iter()
        .flat_map(|l| l.errors)
        .collect()
}

/// A key's press and release, sent raw: for keys no keymap takes, such as Space.
pub fn raw_key(h: &mut Harness, ev: KeyEvent) {
    let errors = h.send(Input::Key(ev.clone()));
    assert!(errors.is_empty(), "{errors:?}");
    let errors = h.send(Input::Key(ev.kind(die_core::KeyKind::Release)));
    assert!(errors.is_empty(), "{errors:?}");
}

/// Every present since the last ask, by buffer.
pub fn presents(h: &mut Harness) -> Vec<(PixelBufferId, PixelRect)> {
    h.fake_pixels()
        .take_calls()
        .into_iter()
        .filter_map(|c| match c {
            PixelSinkCall::Present(id, r) => Some((id, r)),
            _ => None,
        })
        .collect()
}

/// A pixel of a buffer as last presented, at physical (x, y).
pub fn pixel(h: &Harness, id: PixelBufferId, (x, y): (u32, u32)) -> [u8; 4] {
    let (w, _, px) = h.pixels(id).expect("presented");
    let i = (y * w + x) as usize * 4;
    [px[i], px[i + 1], px[i + 2], px[i + 3]]
}

/// The window's status line.
pub fn status(s: &mut Setup) -> String {
    let p =
        s.h.placements()
            .into_iter()
            .find(|p| p.window == s.win)
            .unwrap();
    let (_, rows) = s.h.window_size(s.win).unwrap();
    s.h.scene().row_text(p.row + rows)
}

/// The middle of a rectangle.
pub fn mid(r: Rect) -> (f32, f32) {
    (r.x + r.w / 2.0, r.y + r.h / 2.0)
}
