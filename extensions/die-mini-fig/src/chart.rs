//! Charts: line, bar, scatter and heatmap, with rule and line layers, drawn in a panel's
//! rectangle under its title, each mark left behind as a hit.
//!
//! One builder makes every chart's axes and grid ([`frame`]) and one its legend ([`legend`]).
//! A channel is bands when it's a category, text, a heatmap's, or a bar's numbers; else it runs
//! along time, a log scale or a linear one. Bars take in zero, and lines whose values are
//! nearer it than their span, unless `"zero"` says; scatters don't. A bar's series stack in its band, as die-fig's do, or with `"stack": false`
//! share it side by side; each band's total is written past it when every total fits. Bars
//! over time are as wide as the closest two times allow. A heatmap's colour is die-fig's ramp: blue from dark to light, or red through
//! grey to blue when diverging, centred on zero.

use crate::canvas::{Align, Canvas, Ink, Pen, Rect, Theme, hex, mix};
use crate::dash::Job;
use crate::data::{self, Column, Kind as Cells, Table};
use crate::hit::{Hit, Shape};
use crate::scale::{self, Kind as Along, Scale};
use crate::spec::{Channel, Kind, Panel};
use die_draw::Rgb;
use std::collections::HashMap;

/// die-fig's ramps on a dark surface, least to most.
const SEQUENTIAL: &str = "104281 1c5cab 2a78d6 5598e7 86b6ef b7d3f6";
const DIVERGING: &str = "f6c2bd e06e68 a1302f 383835 1c5cab 5598e7 b7d3f6";
/// The widest a bar is.
const BAR_MAX: f32 = 32.0;

/// A series: its name, its look, and its rows.
struct Series {
    name: String,
    color: Rgb,
    dash: bool,
    rows: Vec<usize>,
}

/// An axis before it has a place: bands, or a span of numbers along a kind of scale, its
/// ticks no closer than a least step (bars over time tick no finer than their bars).
enum Axis {
    Bands(Vec<String>),
    Span(Along, f64, f64, f64),
}

/// A chart's scales and its plot.
struct Plot {
    x: Scale,
    y: Scale,
    rect: Rect,
}

/// How a legend's key is drawn.
#[derive(Clone, Copy, Debug)]
pub enum Key {
    /// A short line, dashed or not.
    Line(bool),
    Dot,
    Square,
}

/// What every kind of chart reads: its panel, its rows, and its x and y columns.
struct Ctx<'a> {
    th: &'a Theme,
    p: &'a Panel,
    t: &'a Table,
    x: (&'a Channel, &'a Column),
    y: (&'a Channel, &'a Column),
    index: usize,
    /// More than one series: the card names its series.
    many: bool,
    /// The dashboard's time field and the range it shows.
    time: Option<(&'a str, (f64, f64))>,
}

/// A chart in `b`, with its legend; its marks' hits. An error says in a line what's missing.
pub fn draw(c: &mut Canvas, job: &Job, b: Rect) -> Result<Vec<Hit>, String> {
    let (th, p) = (job.theme, job.panel);
    let t = job.table.ok_or("no \"data\"")?;
    let (Some(xch), Some(ych)) = (&p.x, &p.y) else {
        return Err("needs an \"x\" and a \"y\"".into());
    };
    let (x, y) = ((xch, t.need(&xch.field)?), (ych, t.need(&ych.field)?));
    if t.is_empty() {
        return Err("no rows to draw".into());
    }
    let heat = matches!(p.kind, Kind::Heatmap);
    let by = p
        .color
        .as_ref()
        .filter(|_| !heat)
        .map(|ch| ch.field.as_str());
    let series = split(th, p, t, by, None)?;
    let mut layers = Vec::new();
    for l in p.layers.iter().filter(|l| !l.rule) {
        let Some(lt) = l.data.as_ref().and_then(|d| job.tables.get(d)) else {
            continue;
        };
        let more = split(th, p, lt, l.by.as_deref(), Some(th.dim))?;
        layers.extend(more.into_iter().map(|s| (s, lt)));
    }
    let many = series.len() > 1;
    let index = job.index;
    let cx = Ctx {
        th,
        p,
        t,
        x,
        y,
        index,
        many,
        time: job.time,
    };
    let key = match p.kind {
        Kind::Bar => Key::Square,
        Kind::Scatter => Key::Dot,
        _ => Key::Line(false),
    };
    let mut items: Vec<(String, Rgb, Key)> = Vec::new();
    for s in series.iter().filter(|_| many) {
        let key = if let Key::Line(_) = key {
            Key::Line(s.dash)
        } else {
            key
        };
        items.push((s.name.clone(), s.color, key));
    }
    items.extend(
        layers
            .iter()
            .map(|(s, _)| (s.name.clone(), s.color, Key::Line(s.dash))),
    );
    let b = if items.is_empty() {
        b
    } else {
        legend(c, th, &items, b)
    };
    match p.kind {
        Kind::Bar => bars(c, &cx, b, &series),
        Kind::Heatmap => heatmap(c, &cx, b),
        _ => points(c, &cx, b, &series, &layers),
    }
}

/// The keys in rows across the top of `b`, two rows at most, the rest said as "+N more"; the
/// room left under them.
pub fn legend(c: &mut Canvas, th: &Theme, items: &[(String, Rgb, Key)], b: Rect) -> Rect {
    let ink = Ink::new(12.0, th.text);
    let (mut x, mut y) = (b.x, b.y + 13.0);
    for (i, (name, color, key)) in items.iter().enumerate() {
        let w = 20.0 + c.width(name, ink).min(b.w * 0.6);
        if x > b.x && x + w > b.right() {
            if y > b.y + 20.0 {
                let more = format!("+{} more", items.len() - i);
                let ink = Ink::new(12.0, th.muted);
                c.text(&more, ink, (x, y), Align::Start, b.right() - x);
                break;
            }
            (x, y) = (b.x, y + 20.0);
        }
        let color = *color;
        match *key {
            Key::Line(dash) => {
                let pen = Pen {
                    width: 2.0,
                    color,
                    dash,
                };
                c.line(&[(x, y - 4.0), (x + 14.0, y - 4.0)], pen, b);
            }
            Key::Dot => c.dot((x + 7.0, y - 4.0), 4.5, color),
            Key::Square => c.fill(Rect::new(x + 2.0, y - 9.0, 10.0, 10.0), 2.0, color, 1.0),
        }
        c.text(name, ink, (x + 20.0, y), Align::Start, b.w * 0.6);
        x += w + 18.0;
    }
    b.inset(0.0, y - b.y + 10.0, 0.0, 0.0)
}

/// Lines and scatters: points along x, joined (straight or in steps) on a line chart, dotted on
/// a scatter or with `"points": true`; line layers and rules under them.
fn points(
    c: &mut Canvas,
    cx: &Ctx,
    b: Rect,
    series: &[Series],
    layers: &[(Series, &Table)],
) -> Result<Vec<Hit>, String> {
    let (th, p) = (cx.th, cx.p);
    let ((xch, xc), (ych, yc)) = (cx.x, cx.y);
    let line = matches!(p.kind, Kind::Line);
    let all: Vec<usize> = (0..cx.t.len()).collect();
    let mut more: Vec<f64> = rule_ys(p).collect();
    // A line takes in zero when its values are on one side of it and nearer it than their span.
    let (lo, hi) = data::extent(all.iter().map(|&r| yc.num[r])).unwrap_or((0.0, 0.0));
    let near = (lo >= 0.0 && lo <= hi * 0.5) || (hi <= 0.0 && hi >= lo * 0.5);
    if ych.zero.unwrap_or(line && near) && !ych.log {
        more.push(0.0);
    }
    let bands = |ch: &Channel, col: &Column| ch.category || col.kind == Cells::Text;
    let xa = axis(xch, xc, &all, bands(xch, xc), &cx.range(xch))?;
    let ya = axis(ych, yc, &all, bands(ych, yc), &more)?;
    let titles = (title(xch, false), title(ych, true));
    let plot = frame(c, th, b, (&xa, &ya), titles);
    let clip = plot.rect.inset(-2.0, -2.0, -2.0, -2.0);
    for (s, t) in layers {
        let (Some(lx), Some(ly)) = (t.column(&xch.field), t.column(&ych.field)) else {
            continue;
        };
        let at = |&r: &usize| (place(&plot.x, lx, r), place(&plot.y, ly, r));
        let pts: Vec<_> = s.rows.iter().map(at).collect();
        let pen = Pen {
            width: 1.5,
            color: s.color,
            dash: s.dash,
        };
        c.line(&pts, pen, clip);
    }
    rules(c, th, p, &plot);
    let mut hits = Vec::new();
    for (si, s) in series.iter().enumerate() {
        let mut rows = s.rows.clone();
        if plot.x.kind != Along::Band {
            rows.sort_by(|&a, &b| xc.num[a].total_cmp(&xc.num[b]));
        }
        let at = |&r: &usize| (place(&plot.x, xc, r), place(&plot.y, yc, r));
        let pts: Vec<(f32, f32)> = rows.iter().map(at).collect();
        if line {
            let pen = Pen {
                width: 2.0,
                color: s.color,
                dash: s.dash,
            };
            let path = if p.mark.step {
                steps(&pts)
            } else {
                pts.clone()
            };
            c.line(&path, pen, clip);
        }
        for (i, (&r, &pt)) in rows.iter().zip(&pts).enumerate() {
            if !clip.contains(pt.0, pt.1) {
                continue;
            }
            if !line || p.mark.points {
                let size = if line { 3.0 } else { 4.0 };
                c.dot(pt, size + 1.5, th.surface);
                c.dot(pt, size, s.color);
            }
            hits.push(cx.hit(Shape::Point(pt.0, pt.1), (si, i), s, r));
        }
    }
    Ok(hits)
}

/// Bars up from zero (or along it, horizontal), side by side or stacked within a band.
fn bars(c: &mut Canvas, cx: &Ctx, b: Rect, series: &[Series]) -> Result<Vec<Hit>, String> {
    let (th, p) = (cx.th, cx.p);
    let horizontal = p.mark.horizontal;
    let ((ich, ic), (vch, vc)) = if horizontal {
        (cx.y, cx.x)
    } else {
        (cx.x, cx.y)
    };
    let banded = ich.category || ic.kind != Cells::Time;
    let all: Vec<usize> = series.iter().flat_map(|s| s.rows.iter().copied()).collect();
    // Each row's bar runs between two values: from zero, or from where its stack had got to.
    let mut ends = vec![(0.0, f64::NAN); cx.t.len()];
    let mut tops: HashMap<&str, (f64, f64)> = HashMap::new();
    for &r in &all {
        let v = vc.num[r];
        let top = tops.entry(ic.text[r].as_str()).or_default();
        let sum = if v < 0.0 { &mut top.1 } else { &mut top.0 };
        ends[r] = if p.mark.stack {
            (*sum, *sum + v)
        } else {
            (0.0, v)
        };
        if p.mark.stack && v.is_finite() {
            *sum += v;
        }
    }
    // The series each band has, in order, to share it.
    let mut members: HashMap<&str, Vec<usize>> = HashMap::new();
    for (si, s) in series.iter().enumerate().filter(|_| !p.mark.stack) {
        for &r in &s.rows {
            let m = members.entry(ic.text[r].as_str()).or_default();
            if !m.contains(&si) {
                m.push(si);
            }
        }
    }
    // One bar or stack a band: its total is written past it, with room for that across.
    let single = p.mark.stack || members.values().all(|m| m.len() == 1);
    let mut more: Vec<f64> = ends.iter().flat_map(|e| [e.0, e.1]).collect();
    let most = more.iter().copied().fold(0.0, f64::max);
    if horizontal && single && most > 0.0 {
        more.push(most * 1.12);
    }
    more.extend(rule_ys(p));
    let gap = least_gap(ic, &all);
    // Bands in the order the table has them, whatever their series.
    let mut order = all.clone();
    order.sort_unstable();
    let ia = match axis(ich, ic, &order, banded, &cx.range(ich))? {
        Axis::Span(k, lo, hi, _) => Axis::Span(k, lo - gap / 2.0, hi + gap / 2.0, gap),
        bands => bands,
    };
    let va = axis(vch, vc, &[], false, &more)?;
    let (xa, ya) = if horizontal { (va, ia) } else { (ia, va) };
    let titles = (title(cx.x.0, false), title(cx.y.0, true));
    let plot = frame(c, th, b, (&xa, &ya), titles);
    let (is, vs) = if horizontal {
        (&plot.y, &plot.x)
    } else {
        (&plot.x, &plot.y)
    };
    let width = if banded {
        is.band_width()
    } else {
        let lo = is.domain.0;
        (is.map(lo + gap) - is.map(lo)).abs() * 0.75
    };
    rules(c, th, p, &plot);
    let mut hits = Vec::new();
    // Each band's bars together, and their total.
    let mut reach: HashMap<&str, (Rect, f64)> = HashMap::new();
    for (si, s) in series.iter().enumerate() {
        for (along, &r) in s.rows.iter().enumerate() {
            let centre = place(is, ic, r);
            let (v0, v1) = (vs.map(ends[r].0), vs.map(ends[r].1));
            if !(centre.is_finite() && v0.is_finite() && v1.is_finite()) {
                continue;
            }
            let m = members.get(ic.text[r].as_str());
            let k = m.map_or(1, Vec::len).max(1);
            let j = m.and_then(|m| m.iter().position(|&x| x == si)).unwrap_or(0);
            let w = (width / k as f32).clamp(1.0, BAR_MAX);
            let start = centre - w * k as f32 / 2.0 + w * j as f32;
            let (lo, long) = (v0.min(v1), (v1 - v0).abs());
            let bar = if horizontal {
                Rect::new(lo, start, long, w)
            } else {
                Rect::new(start, lo, w, long)
            };
            c.fill(bar, (w / 4.0).min(3.0), s.color, 1.0);
            let both = reach.entry(ic.text[r].as_str()).or_insert((bar, 0.0));
            both.0 = both.0.union(bar);
            both.1 += vc.num[r];
            let slot = is.band_at(centre).unwrap_or(along);
            let key = if horizontal { (slot, si) } else { (si, slot) };
            hits.push(cx.hit(Shape::Rect(bar), key, s, r));
        }
    }
    if single {
        totals(c, th, &plot, horizontal, width / 0.8, &reach);
    }
    Ok(hits)
}

/// Each band's total past its bar or stack (under a column below zero), as die-fig writes
/// them: none at all when one doesn't fit (past the plot's end, or wider than its band).
fn totals(
    c: &mut Canvas,
    th: &Theme,
    plot: &Plot,
    horizontal: bool,
    step: f32,
    reach: &HashMap<&str, (Rect, f64)>,
) {
    let ink = Ink::new(11.0, th.text);
    let mut labels = Vec::new();
    for (b, total) in reach.values() {
        let text = scale::with_format(*total, ",.2~f");
        let w = c.width(&text, ink);
        let (at, fits) = if horizontal {
            let x = b.right() + 6.0;
            ((x, b.y + b.h / 2.0 + 4.0), x + w <= plot.rect.right() + 2.0)
        } else {
            let y = if *total < 0.0 {
                b.bottom() + 14.0
            } else {
                b.y - 6.0
            };
            ((b.x + b.w / 2.0 - w / 2.0, y), w + 6.0 <= step)
        };
        if !fits {
            return;
        }
        labels.push((text, at, w));
    }
    for (text, at, w) in labels {
        c.text(&text, ink, at, Align::Start, w + 1.0);
    }
}

/// A heatmap: a cell for each row at its x and y bands, coloured by its value on the ramp,
/// whose legend is at the top.
fn heatmap(c: &mut Canvas, cx: &Ctx, b: Rect) -> Result<Vec<Hit>, String> {
    let (th, p, t) = (cx.th, cx.p, cx.t);
    let ch = p.color.as_ref().ok_or("a heatmap needs a \"color\"")?;
    let cc = t.need(&ch.field)?;
    let all: Vec<usize> = (0..t.len()).collect();
    let xa = axis(cx.x.0, cx.x.1, &all, true, &[])?;
    let ya = axis(cx.y.0, cx.y.1, &all, true, &[])?;
    let (mut lo, mut hi) = data::extent(cc.num.iter().copied()).unwrap_or((0.0, 1.0));
    if ch.diverging {
        let m = lo.abs().max(hi.abs());
        (lo, hi) = (-m, m);
    }
    let stops: Vec<Rgb> = (if ch.diverging { DIVERGING } else { SEQUENTIAL })
        .split(' ')
        .map(hex)
        .collect();
    let colour = |v: f64| {
        let f = if hi > lo {
            ((v - lo) / (hi - lo)) as f32
        } else {
            0.5
        };
        let f = f.clamp(0.0, 1.0) * (stops.len() - 1) as f32;
        let i = (f.floor() as usize).min(stops.len() - 2);
        mix(stops[i], stops[i + 1], f - i as f32)
    };
    let ink = Ink::new(11.0, th.muted);
    let name = title(ch, true).unwrap_or("");
    let nw = if name.is_empty() {
        0.0
    } else {
        c.text(name, ink, (b.x, b.y + 12.0), Align::Start, b.w * 0.4) + 10.0
    };
    let ramp = Rect::new(b.x + nw, b.y + 4.0, 160f32.min(b.w - nw), 8.0);
    for i in 0..40 {
        let f = i as f32 / 40.0;
        let piece = Rect::new(ramp.x + ramp.w * f, ramp.y, ramp.w / 40.0 + 0.5, ramp.h);
        c.fill(piece, 0.0, colour(lo + (hi - lo) * f64::from(f)), 1.0);
    }
    let under = ramp.bottom() + 13.0;
    c.text(&scale::short(lo), ink, (ramp.x, under), Align::Start, 80.0);
    c.text(
        &scale::short(hi),
        ink,
        (ramp.right(), under),
        Align::End,
        80.0,
    );
    let titles = (title(cx.x.0, false), title(cx.y.0, false));
    let plot = frame(c, th, b.inset(0.0, 36.0, 0.0, 0.0), (&xa, &ya), titles);
    let sw = plot.rect.w / plot.x.bands.len().max(1) as f32;
    let sh = plot.rect.h / plot.y.bands.len().max(1) as f32;
    let mut hits = Vec::new();
    for r in all {
        let (x, y) = (place(&plot.x, cx.x.1, r), place(&plot.y, cx.y.1, r));
        if !(x.is_finite() && y.is_finite()) {
            continue;
        }
        let cell = Rect::new(x - sw / 2.0 + 1.0, y - sh / 2.0 + 1.0, sw - 2.0, sh - 2.0);
        let v = cc.num[r];
        let color = if v.is_finite() { colour(v) } else { th.grid };
        c.fill(cell, 2.0, color, 1.0);
        let key = (
            cx.index,
            plot.y.band_at(y).unwrap_or(0),
            plot.x.band_at(x).unwrap_or(0),
        );
        let rows = card_rows(p, t, r);
        let title = String::new();
        let shape = Shape::Rect(cell);
        hits.push(Hit {
            shape,
            key,
            title,
            color,
            rows,
        });
    }
    Ok(hits)
}

impl Ctx<'_> {
    /// The dashboard's range when `ch` is its time field: such an axis spans all of it.
    fn range(&self, ch: &Channel) -> Vec<f64> {
        match self.time {
            Some((field, (lo, hi))) if field == ch.field => vec![lo, hi],
            _ => Vec::new(),
        }
    }

    /// The hit of row `r` of series `s`, at its place in key order.
    fn hit(&self, shape: Shape, (series, along): (usize, usize), s: &Series, r: usize) -> Hit {
        let title = if self.many {
            s.name.clone()
        } else {
            String::new()
        };
        let rows = card_rows(self.p, self.t, r);
        let (key, color) = ((self.index, series, along), s.color);
        Hit {
            shape,
            key,
            title,
            color,
            rows,
        }
    }
}

/// The plot in `b` with its axes and grid drawn: the y axis's title over its labels at the
/// left, the x axis's labels under the plot and its title under them at the right. Band
/// labels are cut to their band, and thinned where they would crowd.
fn frame(
    c: &mut Canvas,
    th: &Theme,
    b: Rect,
    (xa, ya): (&Axis, &Axis),
    (xt, yt): (Option<&str>, Option<&str>),
) -> Plot {
    let tick = Ink::new(11.0, th.muted);
    let top = b.y + if yt.is_some() { 22.0 } else { 8.0 };
    let bottom = b.bottom() - if xt.is_some() { 38.0 } else { 22.0 };
    let count = ((bottom - top) / 48.0).max(2.0) as usize;
    let y_bands = matches!(ya, Axis::Bands(_));
    let (y, count) = make(
        ya,
        if y_bands {
            (top, bottom)
        } else {
            (bottom, top)
        },
        count,
    );
    let yticks = y.ticks(count);
    let cap = (b.w * 0.35).max(40.0);
    let widest = yticks
        .iter()
        .map(|t| c.width(&t.label, tick))
        .fold(0.0, f32::max);
    let left = widest.min(cap) + 10.0;
    let rect = Rect::new(
        b.x + left,
        top,
        (b.w - left - 8.0).max(1.0),
        (bottom - top).max(1.0),
    );
    let xcount = (rect.w / 90.0).max(2.0) as usize;
    let (x, xcount) = make(xa, (rect.x, rect.right()), xcount);
    let xticks = x.ticks(xcount);
    let line = |v: f64| if v == 0.0 { th.baseline } else { th.grid };
    let every = if y_bands {
        (13.0 / (rect.h / yticks.len().max(1) as f32))
            .ceil()
            .max(1.0) as usize
    } else {
        1
    };
    for (i, t) in yticks.iter().enumerate() {
        let at = y.map(t.value);
        if !y_bands {
            c.hair((rect.x, at), (rect.right(), at), line(t.value));
        }
        if i % every == 0 {
            c.text(&t.label, tick, (rect.x - 8.0, at + 4.0), Align::End, cap);
        }
    }
    if let Some(t) = yt {
        c.text(t, tick, (b.x, top - 10.0), Align::Start, b.w);
    }
    let (mut every, mut room) = (1, f32::INFINITY);
    if x.kind == Along::Band {
        let step = rect.w / xticks.len().max(1) as f32;
        let widest = xticks
            .iter()
            .map(|t| c.width(&t.label, tick))
            .fold(0.0, f32::max);
        if xticks.len() > 12 {
            every = ((widest + 10.0) / step).ceil().max(1.0) as usize;
        }
        room = step * every as f32 - 6.0;
    }
    for t in xticks.iter().step_by(every) {
        let at = x.map(t.value);
        if y_bands && x.kind != Along::Band {
            c.hair((at, rect.y), (at, rect.bottom()), line(t.value));
        }
        c.text(
            &t.label,
            tick,
            (at, rect.bottom() + 16.0),
            Align::Middle,
            room,
        );
    }
    if let Some(t) = xt {
        c.text(
            t,
            tick,
            (rect.right(), rect.bottom() + 32.0),
            Align::End,
            rect.w,
        );
    }
    Plot { x, y, rect }
}

/// The rule layers across the plot, a label at the right end of each that has one.
fn rules(c: &mut Canvas, th: &Theme, p: &Panel, plot: &Plot) {
    let r = plot.rect;
    let mut taken: Vec<f32> = Vec::new();
    for l in p.layers.iter().filter(|l| l.rule) {
        let Some(y) = l.y.map(|v| plot.y.map(v)) else {
            continue;
        };
        if !(y >= r.y - 1.0 && y <= r.bottom() + 1.0) {
            continue;
        }
        let color = l.color.as_deref().and_then(|n| th.named(n));
        let pen = Pen {
            width: 1.5,
            color: color.unwrap_or(th.dim),
            dash: l.dash,
        };
        c.line(
            &[(r.x, y), (r.right(), y)],
            pen,
            r.inset(-2.0, -2.0, -2.0, -2.0),
        );
        if !l.label.is_empty() {
            let ink = Ink::new(11.0, th.text);
            let w = c.width(&l.label, ink).min(r.w);
            // Over the line, or under it where another label is already over.
            let clear = |base: f32| taken.iter().all(|t| (t - base).abs() >= 28.0);
            let base = if clear(y - 6.0) { y - 6.0 } else { y + 15.0 };
            taken.push(base);
            let back = Rect::new(r.right() - w - 7.0, base - 11.0, w + 6.0, 14.0);
            c.fill(back, 3.0, th.surface, 0.85);
            c.text(&l.label, ink, (r.right() - 4.0, base), Align::End, r.w);
        }
    }
}

/// The rule layers' values on y.
fn rule_ys(p: &Panel) -> impl Iterator<Item = f64> + '_ {
    p.layers.iter().filter(|l| l.rule).filter_map(|l| l.y)
}

/// The rows split by a field's values in the order they first come, each in the spec's
/// colour for it, else `fallback`, else the next slot's; all of them as one series without a
/// field.
fn split(
    th: &Theme,
    p: &Panel,
    t: &Table,
    by: Option<&str>,
    fallback: Option<Rgb>,
) -> Result<Vec<Series>, String> {
    let Some(by) = by else {
        let name = p.y.as_ref().map(|c| c.field.clone()).unwrap_or_default();
        let (color, rows) = (fallback.unwrap_or(th.slot(0)), (0..t.len()).collect());
        let dash = false;
        return Ok(vec![Series {
            name,
            color,
            dash,
            rows,
        }]);
    };
    let col = t.need(by)?;
    let mut out = Vec::new();
    for (i, name) in col.distinct().into_iter().enumerate() {
        let style = p.series.get(&name);
        let named = style
            .and_then(|s| s.color.as_deref())
            .and_then(|n| th.named(n));
        let color = named.or(fallback).unwrap_or(th.slot(i));
        let rows = (0..t.len()).filter(|&r| col.text[r] == name).collect();
        let dash = style.is_some_and(|s| s.dash);
        out.push(Series {
            name,
            color,
            dash,
            rows,
        });
    }
    Ok(out)
}

/// A channel's axis over `rows`: bands of its cells in the order they come, or the span of its
/// numbers and `more` (on a log scale only those above zero).
fn axis(
    ch: &Channel,
    col: &Column,
    rows: &[usize],
    bands: bool,
    more: &[f64],
) -> Result<Axis, String> {
    if bands {
        let mut seen: Vec<String> = Vec::new();
        for s in rows.iter().map(|&r| &col.text[r]) {
            if !s.is_empty() && !seen.contains(s) {
                seen.push(s.clone());
            }
        }
        return Ok(Axis::Bands(seen));
    }
    let vals = rows.iter().map(|&r| col.num[r]).chain(more.iter().copied());
    let span = data::extent(vals.filter(|v| !ch.log || *v > 0.0));
    let (lo, hi) = span.ok_or(format!("no numbers in \"{}\"", ch.field))?;
    let along = match (ch.log, col.kind) {
        (true, _) => Along::Log,
        (false, Cells::Time) => Along::Time,
        _ => Along::Linear,
    };
    Ok(Axis::Span(along, lo, hi, 0.0))
}

/// An axis placed along `range`, its ends nice for about `count` ticks; and how many ticks it
/// takes.
fn make(a: &Axis, range: (f32, f32), count: usize) -> (Scale, usize) {
    match *a {
        Axis::Bands(ref b) => (Scale::band(b.clone(), range), count),
        Axis::Span(along, lo, hi, least) => {
            let most = if least > 0.0 {
                ((hi - lo) / least) as usize
            } else {
                count
            };
            let count = count.min(most.max(1));
            let s = match along {
                Along::Time => Scale::time((lo, hi), range),
                Along::Log => Scale::log((lo, hi), range),
                _ => Scale::linear((lo, hi), range),
            };
            (s.nice(count), count)
        }
    }
}

/// An axis's title: the channel's, none for "", else (on y only) the field's name.
fn title(ch: &Channel, y: bool) -> Option<&str> {
    match ch.title.as_deref() {
        Some("") => None,
        Some(t) => Some(t),
        None => y.then_some(ch.field.as_str()),
    }
}

/// Where a row's cell goes on a scale: its band's centre, or its number's place.
fn place(s: &Scale, col: &Column, r: usize) -> f32 {
    if s.kind == Along::Band {
        let i = s.bands.iter().position(|b| *b == col.text[r]);
        return i.map_or(f32::NAN, |i| s.map(i as f64));
    }
    s.map(col.num[r])
}

/// A line's points as steps: across at each point's height, then up or down to the next.
fn steps(pts: &[(f32, f32)]) -> Vec<(f32, f32)> {
    let mut out = Vec::with_capacity(pts.len() * 2);
    for (i, &p) in pts.iter().enumerate() {
        if i > 0 {
            out.push((p.0, pts[i - 1].1));
        }
        out.push(p);
    }
    out
}

/// The least gap between two of a column's distinct numbers over `rows`; a day without two.
fn least_gap(col: &Column, rows: &[usize]) -> f64 {
    let mut v: Vec<f64> = rows
        .iter()
        .map(|&r| col.num[r])
        .filter(|v| v.is_finite())
        .collect();
    v.sort_by(f64::total_cmp);
    v.dedup();
    let least = v
        .windows(2)
        .map(|w| w[1] - w[0])
        .fold(f64::INFINITY, f64::min);
    if least.is_finite() { least } else { 86_400.0 }
}

/// A row's card: the tooltip's fields, else x, y and the colour's.
fn card_rows(p: &Panel, t: &Table, r: usize) -> Vec<(String, String)> {
    let mut fields: Vec<&str> = p.tooltip.iter().map(String::as_str).collect();
    if fields.is_empty() {
        let channels = [&p.x, &p.y, &p.color].into_iter().flatten();
        fields = channels.map(|c| c.field.as_str()).collect();
    }
    let columns = fields.into_iter().filter_map(|f| t.column(f));
    columns.map(|c| (c.name.clone(), cell(c, r))).collect()
}

/// A cell as a card writes it: a time in full, a number to six figures, text as it is.
pub fn cell(c: &Column, r: usize) -> String {
    match c.kind {
        Cells::Time => scale::time_full(c.num[r]),
        Cells::Number => scale::full(c.num[r]),
        Cells::Text => c.text[r].clone(),
    }
}
