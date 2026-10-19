//! DIE's main-thread side of a terminal (TermSurface): a row cache of DIE cells, the image
//! table and placements. Plain data only; it never touches an engine handle.
use crate::proto::{Cell, Effect, GRAPHEME, ImageUpload, Placement, ToDie};
use std::collections::HashMap;

#[derive(Default)]
pub struct Surface {
    pub cols: u16,
    pub rows: u16,
    pub grid: Vec<Cell>,
    pub graphemes: Vec<String>,
    pub links: Vec<String>,
    pub images: HashMap<u32, ImageUpload>,
    pub placements: Vec<Placement>,
    pub cursor: Option<(u16, u16)>,
    pub title: String,
    pub frames: u64,
    /// The newest frame not yet presented; acked at the next vsync.
    pub need_ack: Option<u64>,
    pub exited: Option<i32>,
}

/// What a message did, for the workload driver (kept out of the ingest timing).
pub enum Applied {
    Frame { seq: u64, t_read: u64, t_built: u64, rows: usize, full: bool },
    Image { id: u32, t_ready: u64, t_sent: u64, bytes: usize },
    ImageGone,
    Effect(Effect),
    Scrollback { lines: u32, text: Vec<u8>, t_built: u64 },
    Exited(i32),
    Stats(crate::proto::Stats),
}

impl Surface {
    pub fn apply(&mut self, m: ToDie) -> Applied {
        match m {
            ToDie::Frame(f) => {
                if f.cols != self.cols || f.rows != self.rows {
                    self.cols = f.cols;
                    self.rows = f.rows;
                    self.grid = vec![Cell::default(); f.cols as usize * f.rows as usize];
                }
                for (id, s) in f.new_graphemes {
                    let i = id as usize;
                    if self.graphemes.len() <= i {
                        self.graphemes.resize(i + 1, String::new());
                    }
                    self.graphemes[i] = s;
                }
                for (id, s) in f.new_links {
                    let i = id as usize;
                    if self.links.len() <= i {
                        self.links.resize(i + 1, String::new());
                    }
                    self.links[i] = s;
                }
                let cols = self.cols as usize;
                let n = f.dirty.len();
                for r in &f.dirty {
                    let y = r.y as usize;
                    if y < self.rows as usize {
                        let k = r.cells.len().min(cols);
                        self.grid[y * cols..y * cols + k].copy_from_slice(&r.cells[..k]);
                    }
                }
                if let Some(p) = f.placements {
                    self.placements = p;
                }
                self.cursor = f.cursor;
                self.frames += 1;
                self.need_ack = Some(f.seq);
                Applied::Frame { seq: f.seq, t_read: f.t_read, t_built: f.t_built, rows: n, full: f.full }
            }
            ToDie::Image(up) => {
                let a = Applied::Image { id: up.image_id, t_ready: up.t_ready, t_sent: up.t_sent, bytes: up.pixels.bytes().len() };
                self.images.insert(up.image_id, up);
                a
            }
            ToDie::ImageGone { image_id } => {
                self.images.remove(&image_id);
                Applied::ImageGone
            }
            ToDie::Effect(e) => {
                if let Effect::Title(t) = &e {
                    self.title = t.clone();
                }
                Applied::Effect(e)
            }
            ToDie::Scrollback { lines, text, t_built, .. } => Applied::Scrollback { lines, text, t_built },
            ToDie::Exited { status } => {
                self.exited = Some(status);
                Applied::Exited(status)
            }
            ToDie::Stats(s) => Applied::Stats(s),
        }
    }

    pub fn row_text(&self, y: u16) -> String {
        let cols = self.cols as usize;
        let mut s = String::new();
        if y >= self.rows {
            return s;
        }
        for c in &self.grid[y as usize * cols..(y as usize + 1) * cols] {
            if c.wide == 2 || c.wide == 3 {
                continue;
            }
            if c.ch & GRAPHEME != 0 {
                s.push_str(self.graphemes.get((c.ch & !GRAPHEME) as usize).map_or("?", |g| g.as_str()));
            } else {
                s.push(char::from_u32(c.ch).filter(|&c| c != '\0').unwrap_or(' '));
            }
        }
        s
    }

    pub fn screen_text(&self) -> String {
        (0..self.rows).map(|y| self.row_text(y).trim_end().to_string()).collect::<Vec<_>>().join("\n")
    }
}
