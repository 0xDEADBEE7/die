//! A figure drawn. Every figure is a dashboard: its header (the title, the line under it, the
//! time range it shows), then its rows top to bottom, 16 px apart. A row of tiles takes their
//! height; the other rows share what's left by their `height`, each at least a readable share
//! (so a long dashboard runs on below the area, to be scrolled). Across a row the cells share
//! the width by `span`, 12 px apart, and a column stacks its panels. A panel is a rounded frame
//! with its title and subtitle and its chart, tile or tree under them; a lone panel without a
//! title is the whole figure, without a frame. What a panel can't draw it says in a line.

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
    /// Every table, for its layers and a stat's previous range.
    pub tables: &'a Tables,
    /// The dashboard's time field and the range it shows.
    pub time: Option<(&'a str, (f64, f64))>,
    /// Its place in key order.
    pub index: usize,
}

/// What a drawing leaves: each panel's frame and every mark's hit, in key order, and where the
/// rows end (past the area's foot when they scroll).
#[derive(Clone, Debug, Default)]
pub struct Drawn {
    pub frames: Vec<Rect>,
    pub hits: Vec<Hit>,
    pub height: f32,
}

/// The figure on the canvas, its rows `scroll` px up under the header.
pub fn draw(c: &mut Canvas, th: &Theme, fig: &Fig, tables: &Tables, scroll: f32) -> Drawn {
    let (w, _) = c.size();
    let (mut frames, height) = layout(fig, c.size());
    let window = data::window(fig, tables);
    let lone = frames.len() == 1 && fig.panels().all(|p| p.title.is_empty());
    let mut hits = Vec::new();
    for (index, (p, f)) in fig.panels().zip(frames.iter_mut()).enumerate() {
        f.y -= scroll;
        let table = rows(fig, tables, p, window);
        let job = Job {
            theme: th,
            panel: p,
            table: table.as_ref().ok().and_then(Option::as_ref),
            tables,
            time: fig.time.as_ref().map(|t| t.field.as_str()).zip(window),
            index,
        };
        hits.extend(panel(c, &job, *f, lone, table.as_ref().err()));
    }
    let top = header(fig);
    hits.retain(|h| h.anchor().1 >= top);
    hits.sort_by_key(|h| h.key);
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
    let tiles = |r: &Row| {
        let stat = |p: &Panel| matches!(p.kind, Kind::Stat(_));
        r.cells.iter().all(|c| c.panels.iter().all(stat))
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
/// column. None for a panel without data (a tree).
fn rows(
    fig: &Fig,
    tables: &Tables,
    p: &Panel,
    window: Option<(f64, f64)>,
) -> Result<Option<Table>, String> {
    let Some(name) = &p.data else {
        return Ok(None);
    };
    let t = tables.get(name).ok_or(format!("no table \"{name}\""))?;
    let t = t.transform(&p.transform)?;
    Ok(Some(match (window, &fig.time) {
        (Some(w), Some(time)) => t.within(&time.field, w),
        _ => t,
    }))
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
        (Some(e), _) => Err(e.clone()),
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
