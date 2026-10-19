//! Headless render: windows to cell frames. Text uses the same layout as motions (C16);
//! mode providers add faces only. Custom surfaces render without Ctx.
use crate::ctx::*;
use crate::ids::*;
use crate::layout::Viewport;
use crate::text::*;
use std::ops::Range;
use std::panic::{AssertUnwindSafe, catch_unwind};

#[derive(Clone, Debug, PartialEq)]
pub struct FrameCell {
    pub text: String,
    pub pos: Option<usize>,
    pub face: Option<&'static str>,
}

#[derive(Clone, Debug, Default)]
pub struct Frame {
    pub rows: Vec<String>,
    pub cells: Vec<Vec<FrameCell>>,
    pub cursor: Option<(usize, usize)>,
}

fn blank(cols: usize, rows: usize) -> Vec<Vec<FrameCell>> {
    vec![
        vec![
            FrameCell {
                text: " ".into(),
                pos: None,
                face: None
            };
            cols
        ];
        rows
    ]
}

fn finish(cells: Vec<Vec<FrameCell>>, cursor: Option<(usize, usize)>) -> Frame {
    let rows = cells
        .iter()
        .map(|r| {
            r.iter()
                .map(|c| c.text.as_str())
                .collect::<String>()
                .trim_end()
                .to_string()
        })
        .collect();
    Frame {
        rows,
        cells,
        cursor,
    }
}

fn put_str(row: &mut [FrameCell], x: usize, s: &str) {
    for (i, ch) in s.chars().enumerate() {
        if let Some(c) = row.get_mut(x + i) {
            c.text = ch.to_string();
        }
    }
}

impl Ctx {
    /// Render one window. Core scrolls the cursor into view first (C5).
    pub fn render_window(&mut self, w: WindowId) -> Frame {
        let Some((b, cols, rows)) = self.render_parts(w) else {
            return Frame::default();
        };
        let Buffer { content, modes, .. } = b;
        match content {
            Content::Text(t) => render_text(t, modes, w, cols, rows),
            Content::Custom { obj: Some(s), .. } => {
                let mut out = vec![];
                let mut cells = blank(cols, rows);
                if catch_unwind(AssertUnwindSafe(|| s.render(cols, rows, &mut out))).is_err() {
                    out = vec!["<render panicked>".into()]; // placeholder until the next good render (§15)
                }
                for (i, l) in out.iter().enumerate().take(rows) {
                    put_str(&mut cells[i], 0, l);
                }
                finish(cells, None)
            }
            Content::Custom { obj: None, .. } => finish(blank(cols, rows), None),
        }
    }
}

fn render_text(
    t: &mut TextBuf,
    modes: &mut [ModeSlot],
    w: WindowId,
    cols: usize,
    rows: usize,
) -> Frame {
    t.scroll_to_cursor(w);
    let Some(Viewport {
        geometry: g,
        rows: vrows,
        ..
    }) = t.viewport(w)
    else {
        return Frame::default();
    };
    let cursor = t.cursor(w);
    let visible = {
        let ps = || {
            vrows
                .iter()
                .flat_map(|r| r.cells.iter().filter_map(|c| c.pos))
        };
        ps().min().unwrap_or(0)..ps().max().map_or(t.len(), |p| p + 1)
    };
    // Faces: stored Face decorations, then providers (faces only, C16).
    let mut faces: Vec<(Range<usize>, &'static str)> = t
        .decorations()
        .filter_map(|(_, d)| match d.deco {
            Deco::Face(f) => Some((d.range.clone(), f)),
            _ => None,
        })
        .collect();
    for m in modes.iter_mut() {
        if let Some(o) = &mut m.obj {
            let tt: &TextBuf = t;
            let _ = catch_unwind(AssertUnwindSafe(|| {
                o.decorate(tt, visible.clone(), &mut faces)
            }));
        }
    }
    let face_at = |p: usize| {
        faces
            .iter()
            .rev()
            .find(|(r, _)| r.contains(&p))
            .map(|f| f.1)
    };
    let cur_line = cursor.map(|c| t.line_of(c));
    let mut cells = blank(cols, rows);
    for (i, vr) in vrows.iter().enumerate().take(rows) {
        if g.gutter > 0 && vr.row == 0 {
            let n = match (t.settings.line_numbers, cur_line) {
                (LineNumbers::Relative, Some(cl)) if cl != vr.first_line => {
                    cl.abs_diff(vr.first_line)
                }
                _ => vr.first_line + 1,
            };
            let s = format!("{n:>w$} ", w = g.gutter - 1);
            put_str(&mut cells[i], g.pad, &s);
        }
        for (j, c) in vr.cells.iter().enumerate() {
            if let Some(fc) = cells[i].get_mut(g.text_x + j) {
                *fc = FrameCell {
                    text: c.text.clone(),
                    pos: c.pos,
                    face: c.pos.and_then(face_at),
                };
            }
        }
    }
    let cur = if t.settings.cursor_visible {
        cursor.and_then(|c| t.screen_pos(w, c))
    } else {
        None
    };
    finish(cells, cur)
}
