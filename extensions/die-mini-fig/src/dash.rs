//! A figure drawn. Every figure is a dashboard: its header (the title, the line under it, the
//! time range it shows), then its rows top to bottom, 16 px apart. A row of tiles takes their
//! height; the other rows share what's left by their `height`, each at least a readable share
//! (so a long dashboard runs on below the area, to be scrolled). Across a row the cells share
//! the width by `span`, 12 px apart, and a column stacks its panels. A panel is a rounded frame
//! with its title and subtitle and its chart, tile or tree under them; a lone panel without a
//! title is the whole figure, without a frame. What a panel can't draw (its spec, or its table)
//! it says in a line in its frame, and the others are drawn.
//!
//! A figure is made [`Ready`] once a read: its tables, and each panel's rows transformed and cut
//! to the time range, so a scroll or a size draws it again without making them again.

use crate::canvas::{Align, Canvas, Ink, Pen, Rect, Theme};
use crate::data::{self, Table, Tables};
use crate::hit::Hit;
use crate::spec::{Fig, Kind, Panel, Row};
use crate::{chart, tile, tree};

/// The rows' margin at the sides; between panels; between rows; a tile's height.
const MARGIN: f32 = 28.0;
const GAP: f32 = 12.0;
const ROW_GAP: f32 = 16.0;
const TILE: f32 = 100.0;

/// What a panel is drawn from.
pub struct Job<'a> {
    pub theme: &'a Theme,
    pub panel: &'a Panel,
    /// Its rows: its table transformed, and cut to the dashboard's time.
    pub table: Option<&'a Table>,
    /// Its table transformed but not cut, for a stat's change against the range before.
    pub whole: Option<&'a Table>,
    /// Every table, for its layers.
    pub tables: &'a Tables,
    /// The dashboard's time field and the range it shows.
    pub time: Option<(&'a str, (f64, f64))>,
    /// Its place in key order.
    pub index: usize,
}

/// A figure read, ready to draw at any size and scroll: its spec, its tables, the time range it
/// shows, and each panel's rows (in key order), or why it has none.
pub struct Ready {
    pub fig: Fig,
    pub tables: Tables,
    pub window: Option<(f64, f64)>,
    rows: Vec<Made>,
}

/// A panel's rows, and the whole of them for a stat's change; none without data; or why not.
type Made = Result<Option<(Table, Option<Table>)>, String>;

impl Ready {
    /// A spec's tables read, and its panels' rows made from them.
    pub fn new(fig: Fig) -> Ready {
        let tables = data::load(&fig);
        let window = data::window(&fig, &tables);
        let rows = fig.panels().map(|p| rows(&fig, &tables, p, window));
        Ready {
            rows: rows.collect(),
            fig,
            tables,
            window,
        }
    }

    /// Why the first panel that can't be drawn can't: its spec, or its table.
    pub fn failed(&self) -> Option<String> {
        let mut panels = self.fig.panels().zip(&self.rows);
        panels.find_map(|(p, rows)| match (&p.kind, rows) {
            (Kind::Failed(why), _) | (_, Err(why)) => Some(why.clone()),
            _ => None,
        })
    }
}

/// What a drawing leaves: each panel's frame and every mark's hit, in key order (no two at one
/// place), and where the rows end (past the area's foot when they scroll).
#[derive(Clone, Debug, Default)]
pub struct Drawn {
    pub frames: Vec<Rect>,
    pub hits: Vec<Hit>,
    pub height: f32,
}

/// The figure on the canvas, its rows `scroll` px up under the header.
pub fn draw(c: &mut Canvas, th: &Theme, ready: &Ready, scroll: f32) -> Drawn {
    let fig = &ready.fig;
    let (w, _) = c.size();
    let (mut frames, height) = layout(fig, c.size());
    let lone = frames.len() == 1 && fig.panels().all(|p| p.title.is_empty());
    let mut hits = Vec::new();
    let panels = fig.panels().zip(&ready.rows).zip(frames.iter_mut());
    for (index, ((p, rows), f)) in panels.enumerate() {
        f.y -= scroll;
        let made = rows.as_ref().ok().and_then(Option::as_ref);
        let job = Job {
            theme: th,
            panel: p,
            table: made.map(|m| &m.0),
            whole: made.and_then(|m| m.1.as_ref()),
            tables: &ready.tables,
            time: fig
                .time
                .as_ref()
                .map(|t| t.field.as_str())
                .zip(ready.window),
            index,
        };
        hits.extend(panel(c, &job, *f, lone, rows.as_ref().err()));
    }
    // Each hit's place along its series becomes its rank there, so two marks a kind put at one
    // place (a band's stacked rows) are told apart, in the order they were drawn.
    hits.sort_by_key(|h| h.key);
    for i in 0..hits.len() {
        let next =
            i > 0 && hits[i - 1].key.0 == hits[i].key.0 && hits[i - 1].key.1 == hits[i].key.1;
        hits[i].key.2 = if next { hits[i - 1].key.2 + 1 } else { 0 };
    }
    let top = header(fig);
    hits.retain(|h| h.anchor().1 >= top);
    c.fill(Rect::new(0.0, 0.0, w, top - 4.0), 0.0, th.surface, 1.0);
    let title = Ink::new(15.0, th.strong).bold();
    c.text(
        &fig.title,
        title,
        (MARGIN, 38.0),
        Align::Start,
        w - 2.0 * MARGIN,
    );
    let mut y = 52.0;
    if !fig.subtitle.is_empty() {
        let ink = Ink::new(12.0, th.muted);
        c.text(
            &fig.subtitle,
            ink,
            (MARGIN, 60.0),
            Align::Start,
            w - 2.0 * MARGIN,
        );
        y += 20.0;
    }
    if let Some(secs) = fig.time.as_ref().and_then(|t| t.secs) {
        let words = span(secs);
        let ink = Ink::new(12.0, th.text);
        let chip = Rect::new(MARGIN, y, c.width(&words, ink) + 24.0, 24.0);
        c.fill(chip, 6.0, th.card, 1.0);
        c.outline(chip, 6.0, Pen::new(1.0, th.ring));
        c.text(&words, ink, (chip.x + 12.0, y + 16.5), Align::Start, chip.w);
    }
    Drawn {
        frames,
        hits,
        height,
    }
}

/// A figure that can't be drawn: why, in a line in the middle of the canvas.
pub fn message(c: &mut Canvas, th: &Theme, why: &str) {
    let (w, h) = c.size();
    let ink = Ink::new(13.0, th.text);
    c.text(
        &format!("mini-fig: {why}"),
        ink,
        (w / 2.0, h / 2.0),
        Align::Middle,
        w - 40.0,
    );
}

/// Where each panel goes in a `w × h` area, in key order, before scrolling; and where its
/// rows end.
pub fn layout(fig: &Fig, (w, h): (f32, f32)) -> (Vec<Rect>, f32) {
    let top = header(fig);
    // A row of tiles, a tile among them that can't be read still one.
    let tiles = |r: &Row| {
        let mut panels = r.cells.iter().flat_map(|c| &c.panels);
        let stat = |p: &Panel| matches!(p.kind, Kind::Stat(_));
        panels.clone().any(stat) && panels.all(|p| stat(p) || matches!(p.kind, Kind::Failed(_)))
    };
    let tall = |r: &Row| {
        let column = |k: usize| k as f32 * (TILE + GAP) - GAP;
        r.cells
            .iter()
            .map(|c| column(c.panels.len()))
            .fold(0.0, f32::max)
    };
    let gaps = fig.rows.len().saturating_sub(1) as f32 * ROW_GAP;
    let fixed: f32 = fig.rows.iter().filter(|r| tiles(r)).map(tall).sum();
    let weights: f32 = fig
        .rows
        .iter()
        .filter(|r| !tiles(r))
        .map(|r| r.height)
        .sum();
    let least = (0.45 * (h - top)).clamp(180.0, 320.0);
    // The other rows share the room by height, each at least `least`: one whose share is less
    // takes `least` and the rest share what's left, so the rows meet the foot when they fit.
    let (mut free, mut open) = (h - top - 16.0 - gaps - fixed, weights);
    let mut lifted = vec![false; fig.rows.len()];
    loop {
        let short = (0..fig.rows.len()).find(|&i| {
            let r = &fig.rows[i];
            !tiles(r) && !lifted[i] && free * r.height / open.max(0.01) < least
        });
        let Some(i) = short else {
            break;
        };
        lifted[i] = true;
        free -= least;
        open -= fig.rows[i].height;
    }
    let (mut frames, mut y) = (Vec::new(), top);
    for (i, r) in fig.rows.iter().enumerate() {
        let rh = if tiles(r) {
            tall(r)
        } else if lifted[i] {
            least
        } else {
            free * r.height / open.max(0.01)
        };
        let spans: f32 = r.cells.iter().map(|c| c.span).sum();
        let across = w - 2.0 * MARGIN - r.cells.len().saturating_sub(1) as f32 * GAP;
        let mut x = MARGIN;
        for cell in &r.cells {
            let cw = across * cell.span / spans.max(0.1);
            let k = cell.panels.len().max(1) as f32;
            let ph = (rh - (k - 1.0) * GAP) / k;
            for i in 0..cell.panels.len() {
                frames.push(Rect::new(x, y + i as f32 * (ph + GAP), cw, ph));
            }
            x += cw + GAP;
        }
        y += rh + ROW_GAP;
    }
    (frames, y - ROW_GAP + 16.0)
}

/// The header's height: the title, the subtitle when there is one, the time range's chip.
pub fn header(fig: &Fig) -> f32 {
    let subtitle = if fig.subtitle.is_empty() { 0.0 } else { 20.0 };
    let chip = if fig.time.as_ref().is_some_and(|t| t.secs.is_some()) {
        38.0
    } else {
        0.0
    };
    56.0 + subtitle + chip
}

/// A time range in words: "Last 7 days", "Last 6 hours", "Last 15 minutes".
fn span(secs: f64) -> String {
    let (n, unit) = if secs % 86_400.0 == 0.0 {
        (secs / 86_400.0, "day")
    } else if secs % 3600.0 == 0.0 {
        (secs / 3600.0, "hour")
    } else {
        ((secs / 60.0).round(), "minute")
    };
    let s = if n == 1.0 { "" } else { "s" };
    format!("Last {n} {unit}{s}")
}

/// A panel's rows: its table transformed, and cut to the dashboard's time when it has that
/// column; and the whole of them too for a stat's change, which is against the range before.
/// None for a panel without data (a tree); why, when its table can't be read.
fn rows(fig: &Fig, tables: &Tables, p: &Panel, window: Option<(f64, f64)>) -> Made {
    let Some(name) = &p.data else {
        return Ok(None);
    };
    let t = tables.get(name).ok_or(format!("no table \"{name}\""))?;
    let whole = t.as_ref().map_err(String::clone)?.transform(&p.transform)?;
    let (Some(w), Some(time)) = (window, &fig.time) else {
        return Ok(Some((whole, None)));
    };
    let change = matches!(&p.kind, Kind::Stat(s) if s.delta);
    Ok(Some((
        whole.within(&time.field, w),
        change.then_some(whole),
    )))
}

/// A panel in its frame: the frame, its title and subtitle, and its chart, tile or tree under
/// them; what it can't draw said in a line. Its hits.
fn panel(c: &mut Canvas, job: &Job, f: Rect, lone: bool, failed: Option<&String>) -> Vec<Hit> {
    let (th, p) = (job.theme, job.panel);
    if !lone {
        c.outline(f, 8.0, Pen::new(1.0, th.grid));
    }
    let mut inner = if lone {
        f
    } else {
        f.inset(16.0, 12.0, 16.0, 10.0)
    };
    let tile = matches!(p.kind, Kind::Stat(_));
    if !tile && !p.title.is_empty() {
        let ink = Ink::new(13.0, th.strong).bold();
        c.text(
            &p.title,
            ink,
            (f.x + 16.0, f.y + 26.0),
            Align::Start,
            f.w - 32.0,
        );
        inner = f.inset(16.0, 34.0, 16.0, 10.0);
        if !p.subtitle.is_empty() && f.h >= 150.0 {
            let ink = Ink::new(12.0, th.muted);
            c.text(
                &p.subtitle,
                ink,
                (f.x + 16.0, f.y + 46.0),
                Align::Start,
                f.w - 32.0,
            );
            inner = f.inset(16.0, 56.0, 16.0, 10.0);
        }
    }
    let got = match (failed, &p.kind) {
        (Some(e), _) | (None, Kind::Failed(e)) => Err(e.clone()),
        (None, Kind::Stat(_)) => tile::draw(c, job, f),
        (None, Kind::Tree(_)) => tree::draw(c, job, inner),
        (None, _) => chart::draw(c, job, inner),
    };
    got.unwrap_or_else(|e| {
        let at = (inner.x, inner.y + inner.h.min(60.0) / 2.0 + 4.0);
        c.text(&e, Ink::new(12.0, th.muted), at, Align::Start, inner.w);
        Vec::new()
    })
}
