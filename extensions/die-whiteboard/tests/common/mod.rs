//! What the tests share: a harness with die-vim and the whiteboard, a board in a temporary
//! directory, and the pointer in board units.
#![allow(dead_code)]

use die_core::{
    Args, BufferId, Button, CellPixels, Harness, Input, Mods, PointerEvent, PointerKind, WindowId,
};
use die_vim::Vim;
use die_whiteboard::Whiteboard;
use die_whiteboard::model::Element;
use die_whiteboard::surface::Shared;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// The cells the tests' host has: 10 × 20 physical pixels on a 2x display, so a cell is 5 × 10
/// board units and a braille dot 2.5 × 2.5.
pub const CELL: CellPixels = CellPixels {
    width: 10,
    height: 20,
    scale: 2.0,
};

/// A directory of a test's own, removed when it drops.
pub struct TempDir(PathBuf);

impl TempDir {
    pub fn new(name: &str) -> TempDir {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let dir =
            std::env::temp_dir().join(format!("die-whiteboard-{name}-{}-{n}", std::process::id()));
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

pub struct Setup {
    pub h: Harness,
    pub dir: TempDir,
    pub buf: BufferId,
    pub win: WindowId,
    pub path: PathBuf,
}

/// die-vim and the whiteboard, a text buffer open, a 100 × 30 window, and the cells known.
pub fn harness() -> Harness {
    let exts: Vec<Box<dyn die_core::Extension>> =
        vec![Box::new(Vim::default()), Box::new(Whiteboard::for_tests())];
    let mut h = Harness::new(exts).unwrap();
    h.open_text("notes", "notes\n").unwrap();
    h.set_window_size(100, 30);
    h.set_cell_pixels(CELL);
    h.run_until_idle();
    h
}

/// `whiteboard PATH`, run to the end.
pub fn open(h: &mut Harness, path: &Path) {
    let args = Args {
        text: Some(path.display().to_string()),
        ..Args::default()
    };
    h.run("whiteboard", &args).unwrap();
    h.run_until_idle();
    h.scene();
}

/// A board `NAME.svg` in a directory of its own, open and focused.
pub fn setup(name: &str) -> Setup {
    let dir = TempDir::new(name);
    let path = dir.path().join(format!("{name}.svg"));
    let mut h = harness();
    open(&mut h, &path);
    let win = h.focused().unwrap();
    let buf = h.window_buffer(win).unwrap();
    assert_eq!(
        h.buffer_name(buf),
        Some(format!("*whiteboard: {name}.svg*").as_str())
    );
    Setup {
        h,
        dir,
        buf,
        win,
        path,
    }
}

pub fn board(h: &mut Harness, buf: BufferId) -> Shared {
    h.with_ext(|wb: &mut Whiteboard, _| wb.board(buf))
        .unwrap()
        .expect("a board")
}

pub fn elements(h: &mut Harness, buf: BufferId) -> Vec<Element> {
    board(h, buf).borrow().board.elements.clone()
}

/// The area's origin in the frame's cells.
pub fn origin(h: &mut Harness, w: WindowId) -> (usize, usize) {
    let p = h.placements().into_iter().find(|p| p.window == w).unwrap();
    let g = h.surface_area(w).unwrap();
    (p.row, p.col + g.text_x)
}

/// A pointer event at a board point, with the view at its default.
pub fn pointer(s: &mut Setup, kind: PointerKind, (bx, by): (f32, f32), mods: Mods, clicks: u8) {
    let (r0, c0) = origin(&mut s.h, s.win);
    let scale = CELL.scale as f32;
    let (x, y) = (bx * scale, by * scale);
    let px = (
        x + c0 as f32 * CELL.width as f32,
        y + r0 as f32 * CELL.height as f32,
    );
    let row = r0 + (y / CELL.height as f32).floor().max(0.0) as usize;
    let col = c0 + (x / CELL.width as f32).floor().max(0.0) as usize;
    let errors = s.h.send(Input::Pointer(PointerEvent {
        kind,
        row,
        col,
        clicks,
        mods,
        px: Some(px),
    }));
    assert!(errors.is_empty(), "{kind:?} at ({bx}, {by}): {errors:?}");
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
    press(s, at);
    release(s, at);
}

/// A press at the first point, a drag through the rest, and a release at the last, with `mods`
/// held through the drag.
pub fn gesture_with(s: &mut Setup, points: &[(f32, f32)], mods: Mods, button: Button) {
    let first = points[0];
    let last = points[points.len() - 1];
    pointer(s, PointerKind::Press(button), first, mods, 1);
    for p in &points[1..] {
        pointer(s, PointerKind::Drag(button), *p, mods, 0);
    }
    pointer(s, PointerKind::Release(button), last, mods, 1);
}

pub fn gesture(s: &mut Setup, points: &[(f32, f32)]) {
    gesture_with(s, points, Mods::default(), Button::Left);
}

pub const SHIFT: Mods = Mods {
    ctrl: false,
    alt: false,
    shift: true,
    super_: false,
};

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

/// The area's rows as text.
pub fn area(s: &mut Setup) -> Vec<String> {
    let (r0, c0) = origin(&mut s.h, s.win);
    let g = s.h.surface_area(s.win).unwrap();
    let scene = s.h.scene();
    (0..g.rows)
        .map(|r| {
            scene.row(r0 + r)[c0..c0 + g.text_w]
                .iter()
                .map(|c| c.glyph.to_string())
                .collect::<String>()
                .trim_end()
                .to_string()
        })
        .collect()
}
