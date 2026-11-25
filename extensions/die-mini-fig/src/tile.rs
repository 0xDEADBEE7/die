//! A stat tile, as die-fig's m4 look has it: the label 12 px muted, the newest value 26 px
//! semibold with its unit after it, the change from the row before under it (an arrow, in the
//! good colour when the change is good, the serious one when bad, muted when neither), and a
//! sparkline of the trend at the right, its newest point a dot in the first series colour.

use crate::canvas::{Align, Canvas, Ink, Pen, Rect};
use crate::chart::cell;
use crate::dash::Job;
use crate::data::{self, Column};
use crate::hit::{Hit, Shape};
use crate::scale::{full, short, with_format, with_unit};
use crate::spec::{Good, Kind};

/// The most points a sparkline has.
const SPARK: usize = 24;

/// A tile in `b`; its one hit, the whole tile.
pub fn draw(c: &mut Canvas, job: &Job, b: Rect) -> Result<Vec<Hit>, String> {
    let (th, Kind::Stat(s)) = (job.theme, &job.panel.kind) else {
        return Ok(Vec::new());
    };
    let t = job.table.ok_or("no \"data\"")?;
    let col = t.need(&s.field)?;
    let trend = s.trend.as_deref().and_then(|f| t.column(f));
    let mut rows: Vec<usize> = (0..t.len()).filter(|&r| col.num[r].is_finite()).collect();
    if let Some(tc) = trend.filter(|tc| tc.numeric()) {
        rows.sort_by(|&a, &b| tc.num[a].total_cmp(&tc.num[b]));
    }
    let last = *rows.last().ok_or(format!("no values of \"{}\"", s.field))?;
    let v = col.num[last];
    let value = s.format.as_deref().map_or(short(v), |f| with_format(v, f));
    let left = b.x + 16.0;
    c.text(
        &s.label,
        Ink::new(12.0, th.muted),
        (left, b.y + 26.0),
        Align::Start,
        b.w - 32.0,
    );
    let ink = Ink::new(26.0, th.strong).bold();
    let vw = c.text(&value, ink, (left, b.y + 60.0), Align::Start, b.w - 32.0);
    let unit = Ink::new(14.0, th.muted);
    c.text(
        &s.unit,
        unit,
        (left + vw + 5.0, b.y + 60.0),
        Align::Start,
        b.w * 0.3,
    );
    let mut card = vec![(s.label.clone(), with_unit(&full(v), &s.unit))];
    let before = rows.len().checked_sub(2).map(|i| col.num[rows[i]]);
    if let (true, Some(p)) = (s.delta, before) {
        let d = v - p;
        let good = match s.good {
            Good::Up => d > 0.0,
            Good::Down => d < 0.0,
            Good::Neither => false,
        };
        let color = if d == 0.0 || s.good == Good::Neither {
            th.muted
        } else if good {
            th.status[0]
        } else {
            th.status[2]
        };
        let (x, y) = (left + 5.0, b.y + 80.0);
        let tip = if d < 0.0 { 4.0 } else { -4.0 };
        if d != 0.0 {
            c.triangle(
                [(x - 5.0, y - tip), (x + 5.0, y - tip), (x, y + tip)],
                color,
            );
        }
        let words = format!("{} vs previous", with_unit(&short(d.abs()), &s.unit));
        let (words, x) = if d == 0.0 {
            ("no change".into(), left)
        } else {
            (words, left + 15.0)
        };
        c.text(
            &words,
            Ink::new(11.5, color),
            (x, b.y + 84.0),
            Align::Start,
            b.w * 0.55,
        );
        card.push(("previous".into(), with_unit(&full(p), &s.unit)));
        card.push(("change".into(), with_unit(&full(d), &s.unit)));
    }
    if let Some(tc) = trend {
        card.push((tc.name.clone(), cell(tc, last)));
        let w = (b.w * 0.32).min(150.0);
        if b.w > 200.0 && rows.len() > 1 {
            spark(
                c,
                job,
                (tc, col, &rows),
                Rect::new(b.right() - 16.0 - w, b.y + 34.0, w, 30.0),
            );
        }
    }
    let title = String::new();
    let (key, color) = ((job.index, 0, 0), th.slot(0));
    Ok(vec![Hit {
        shape: Shape::Rect(b),
        key,
        title,
        color,
        rows: card,
    }])
}

/// The trend: a thin grey line of `rows`' values along their x in `b`, without axes, its newest
/// point a dot; at most [`SPARK`] points, each the mean of a run of rows.
fn spark(c: &mut Canvas, job: &Job, (x, y, rows): (&Column, &Column, &[usize]), b: Rect) {
    let along = |col: &Column, r: usize, i: usize| {
        if col.numeric() { col.num[r] } else { i as f64 }
    };
    let run = rows.len().div_ceil(SPARK).max(1);
    let mean = |v: Vec<f64>| {
        v.chunks(run)
            .map(|c| c.iter().sum::<f64>() / c.len() as f64)
            .collect()
    };
    let xs: Vec<f64> = mean(
        rows.iter()
            .enumerate()
            .map(|(i, &r)| along(x, r, i))
            .collect(),
    );
    let ys: Vec<f64> = mean(rows.iter().map(|&r| y.num[r]).collect());
    let (Some((x0, x1)), Some((y0, y1))) = (
        data::extent(xs.iter().copied()),
        data::extent(ys.iter().copied()),
    ) else {
        return;
    };
    let fx = |v: f64| {
        b.x + if x1 > x0 {
            ((v - x0) / (x1 - x0)) as f32 * b.w
        } else {
            b.w
        }
    };
    let fy = |v: f64| {
        b.bottom()
            - if y1 > y0 {
                ((v - y0) / (y1 - y0)) as f32 * b.h
            } else {
                b.h / 2.0
            }
    };
    let pts: Vec<(f32, f32)> = xs.iter().zip(&ys).map(|(&x, &y)| (fx(x), fy(y))).collect();
    let th = job.theme;
    c.line(&pts, Pen::new(1.5, th.dim), b.inset(-3.0, -3.0, -3.0, -3.0));
    if let Some(&end) = pts.last() {
        c.dot(end, 4.5, th.surface);
        c.dot(end, 3.0, th.slot(0));
    }
}
