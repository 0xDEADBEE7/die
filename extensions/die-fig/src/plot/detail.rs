//! A chart's detail trees (what the panel shows for a point, a series, a band or a cell), the
//! names a search looks through, and where an item puts the cursor.
//!
//! - A point: the moment as the title (the card's header), its series and place as the kind;
//!   the x in full, each visible series' value there (one row each, naming its sample, in
//!   legend order) with a stack's total, and its row's other columns from the data (a JSON
//!   row's nested objects as branches).
//! - A series: its count, min, max, mean and last (the extremes and the last naming their
//!   samples) and its `fig` block.
//! - A band: its category, each series' part (naming it) and the total.
//! - A heatmap's cell: its two categories, its value and its row's other columns.

use super::hover::{sample_near, walked};
use super::{AxisKind, ChartModel, SeriesInfo};
use crate::data::{SeriesKey, Values};
use crate::detail::{DetailRow, DetailTree, Item, Shape, Swatch, object_rows, style_rows};
use crate::interact::{Cursor, View};
use crate::scale::{self, format_time, format_value};
use crate::spec::{MarkKind, Stack};
use crate::style::ColorSpec;

/// What a missing value says in the panel: words in the muted ink, as the card has them.
const NO_DATA: &str = "no data";

impl ChartModel {
    /// A series' swatch: its colour as [`ChartModel::color`] resolves it, and its mark's key
    /// shape (a line for lines, a dot for points, a square for the rest).
    pub fn swatch(&self, s: &SeriesInfo) -> Swatch {
        let color = match s.style.color {
            Some(c) if self.chart.view.render_options => c,
            _ => {
                let cap = if s.mark == MarkKind::Point { 3 } else { 8 };
                match s.slot {
                    Some(i) if i < cap => ColorSpec::Slot(i),
                    _ => ColorSpec::Dim,
                }
            }
        };
        let shape = match s.mark {
            MarkKind::Line => Shape::Line,
            MarkKind::Point => Shape::Dot,
            _ => Shape::Square,
        };
        Swatch { color, shape }
    }

    /// A series' name as the legend and search write it: a second of one name as `name ‹2›`.
    pub fn display_name(&self, key: &SeriesKey) -> String {
        let name = self
            .series_of(key)
            .map_or_else(|| key.name.to_string(), |s| s.name.clone());
        if key.nth == 0 {
            name
        } else {
            format!("{name} ‹{}›", key.nth + 1)
        }
    }

    /// The names a search looks through: each series by its display name (naming the series)
    /// and, on a band chart, each category (naming the band). A series with no sample has
    /// nowhere for the cursor and isn't among them.
    pub fn names(&self) -> Vec<(String, Item)> {
        let mut out = Vec::new();
        for key in self.keys() {
            let Some(si) = self.index_of(&key) else {
                continue;
            };
            if self.series[si].present_ends().is_none() {
                continue;
            }
            out.push((self.display_name(&key), Item::Series(key)));
        }
        if self.is_band() {
            for (i, b) in self.x.bands.iter().enumerate() {
                out.push((
                    b.to_string(),
                    Item::Band {
                        band: i as u32,
                        series: None,
                    },
                ));
            }
        }
        out
    }

    /// Where the cursor goes for an item: a series' last sample (its first part on a band
    /// chart), as a search's find puts it; the place itself for the rest. None for a graph's.
    pub fn cursor_of(&self, item: &Item) -> Option<Cursor> {
        match item {
            Item::Series(k) => self.labels().into_iter().map(|(_, c)| c).find(|c| match c {
                Cursor::Point { series, .. } => series == k,
                Cursor::Band { series, .. } => series.as_ref() == Some(k),
                _ => false,
            }),
            Item::Node { .. } | Item::Edge { .. } => None,
            other => other.cursor(),
        }
    }

    /// The detail tree for what the cursor is on: a point, a band (or a part of one) or a
    /// cell. None when the cursor names nothing the model has.
    pub fn detail(&self, view: &View, cursor: &Cursor) -> Option<DetailTree> {
        match cursor {
            Cursor::Point { series, row } => self.point_detail(view, series, *row),
            Cursor::Band { band, series } => self.band_detail(view, *band, series.as_ref()),
            Cursor::Cell { row, col } => self.cell_detail(*row, *col),
            Cursor::Node(_) => None,
        }
    }

    /// The detail tree for an item: a series' own tree, or the tree for the place it names.
    pub fn detail_of(&self, view: &View, item: &Item) -> Option<DetailTree> {
        match item {
            Item::Series(k) => self.series_detail(k),
            other => self.detail(view, &other.cursor()?),
        }
    }

    /// A value at a series' precision, "no data" where it's missing.
    fn value_text(&self, v: f64, decimals: usize) -> (String, bool) {
        if v.is_nan() {
            (NO_DATA.into(), true)
        } else {
            (self.value_label(v, decimals), false)
        }
    }

    /// An x as a row names a place: a time at its series' step, a band's name, a number.
    fn x_short(&self, s: &SeriesInfo, x: f64) -> String {
        match (self.x.kind, &self.x.format) {
            (AxisKind::Time, None) => format_time(x, s.interval.max(1.0), &self.zone),
            _ => self.x_value(x),
        }
    }

    /// An x in full, for its own row: a time to the second (its date and zone's clock), a
    /// band's name, a number at the data's precision.
    fn x_full(&self, x: f64) -> String {
        match self.x.kind {
            AxisKind::Time if self.x.format.is_none() => time_full(x, &self.zone),
            _ => self.x_value(x),
        }
    }

    /// The x axis's name for its row.
    fn x_name(&self) -> String {
        self.x
            .title
            .clone()
            .or_else(|| self.x.field.clone())
            .unwrap_or_else(|| "x".into())
    }

    /// The y axis's name for its row.
    fn y_name(&self) -> String {
        self.y
            .title
            .clone()
            .or_else(|| self.y.field.clone())
            .unwrap_or_else(|| "y".into())
    }

    /// The value a sample shows: a normalised stack's share, else its own.
    fn shown(&self, stacks: &super::Stacks, si: usize, i: usize) -> f64 {
        if let (Some(lo), Some(hi)) = (&stacks.lo[si], &stacks.hi[si])
            && self.series[si].stack == Stack::Normalize
            && i < lo.len()
        {
            return hi[i] - lo[i];
        }
        self.series[si].ys.get(i).copied().unwrap_or(f64::NAN)
    }

    /// A point's tree.
    fn point_detail(&self, view: &View, key: &SeriesKey, row: u32) -> Option<DetailTree> {
        let si = self.index_of(key)?;
        let s = &self.series[si];
        let i = row as usize;
        let x = *s.xs.get(i)?;
        let y = *s.ys.get(i)?;
        let place = format!("point {} of {}", i + 1, s.xs.len());
        let mut rows = Vec::new();
        let mut t = if self.is_scatter() || self.is_heatmap() {
            // A scatter's point stands alone: its series names it, its x and y are its rows.
            let mut t = DetailTree::new(self.display_name(key), place);
            rows.push(DetailRow::field(self.x_name(), self.x_value(x)));
            let (v, quiet) = self.value_text(y, s.decimals);
            let mut r = DetailRow::field(self.y_name(), v);
            if quiet {
                r = r.quiet();
            }
            rows.push(r);
            if let Some(sz) = s.sizes.as_ref().and_then(|v| v.get(i)) {
                rows.push(DetailRow::field("size", format_value(*sz)));
            }
            t.swatch = Some(self.swatch(s));
            t
        } else {
            let mut t = DetailTree::new(
                self.x_label(x, s.interval),
                format!("{} · {place}", self.display_name(key)),
            );
            t.swatch = Some(self.swatch(s));
            rows.push(DetailRow::field(self.x_name(), self.x_full(x)));
            // Each visible series' value at this x, as the card has them, in legend order.
            let stacks = self.stacks(view);
            let mut values = Vec::new();
            let mut stacked: Vec<(f64, usize)> = Vec::new();
            for sj in walked(self, view) {
                let o = &self.series[sj];
                let j = sample_near(self, None, sj, x);
                let v = j.map_or(f64::NAN, |j| self.shown(&stacks, sj, j));
                let (text, quiet) = self.value_text(v, o.decimals);
                let item = match j {
                    Some(j) => Item::Point {
                        series: o.key.clone(),
                        row: j as u32,
                    },
                    None => Item::Series(o.key.clone()),
                };
                let mut r =
                    DetailRow::mark(self.display_name(&o.key), text, Some(self.swatch(o)), item);
                r.quiet = quiet;
                values.push(r);
                if stacks.hi[sj].is_some() && v.is_finite() {
                    stacked.push((v, o.decimals));
                }
            }
            if !values.is_empty() {
                rows.push(DetailRow::section("Values"));
                rows.extend(values);
                if let Some(total) = self.total(&stacked) {
                    rows.push(DetailRow::sum("total", total));
                }
            }
            t
        };
        self.data_rows(s, i, &mut rows);
        t.item = Some(Item::Point {
            series: key.clone(),
            row,
        });
        t.rows = rows;
        Some(t)
    }

    /// A mean at a decimal past its series' precision, a zero there trimmed ("778", not
    /// "778.0"); with the axis's format, or a share, as the values are written.
    fn mean_text(&self, mean: f64, decimals: usize) -> String {
        let t = self.value_label(mean, (decimals + 1).min(6));
        let plain =
            self.y.format.is_none() && !self.series.iter().any(|s| s.stack == Stack::Normalize);
        if plain && t.contains('.') {
            t.trim_end_matches('0').trim_end_matches('.').to_string()
        } else {
            t
        }
    }

    /// The total of two or more stacked values, at their finest precision; none for shares.
    fn total(&self, parts: &[(f64, usize)]) -> Option<String> {
        if parts.len() < 2 || self.series.iter().any(|s| s.stack == Stack::Normalize) {
            return None;
        }
        let sum: f64 = parts.iter().map(|p| p.0).sum();
        let dec = parts.iter().map(|p| p.1).max().unwrap_or(0);
        Some(self.value_label(sum, dec))
    }

    /// A sample's row in its layer's data, under a heading: its columns but those the chart
    /// draws it by (x, y, colour, and the fields a fold took apart), a JSON row's fields with
    /// nested objects as branches. Nothing when no other column is left.
    fn data_rows(&self, s: &SeriesInfo, i: usize, rows: &mut Vec<DetailRow>) {
        let Some(t) = self.drawn.get(usize::from(s.layer)) else {
            return;
        };
        let Some(&r) = s.rows.get(i) else {
            return;
        };
        let r = r as usize;
        let layer = self.chart.layers.get(usize::from(s.layer));
        let mut skip: Vec<&str> = Vec::new();
        if let Some(l) = layer {
            for c in [&l.enc.x, &l.enc.y, &l.enc.color, &l.enc.y2, &l.enc.size]
                .into_iter()
                .flatten()
            {
                if let Some(f) = c.field.as_deref() {
                    skip.push(f);
                }
            }
        }
        let transforms = self
            .chart
            .transform
            .iter()
            .chain(layer.into_iter().flat_map(|l| l.transform.iter()));
        let mut untouched = true;
        for tr in transforms {
            untouched = false;
            if let crate::data::transform::Transform::Fold { fields, .. } = tr {
                skip.extend(fields.iter().map(String::as_str));
            }
        }
        let mut out = Vec::new();
        match t.raw.as_ref().and_then(|raw| raw.get(r)) {
            Some(node) if node.as_obj().is_some() => {
                let mut all = skip.clone();
                all.push("fig");
                object_rows(node, &all, 0, &mut out);
            }
            _ => {
                for c in t.columns.iter().filter(|c| !skip.contains(&&*c.name)) {
                    if r >= c.len() {
                        continue;
                    }
                    let (text, quiet) = match &c.values {
                        Values::Number(_) => {
                            let v = c.f64_at(r);
                            if v.is_nan() {
                                (NO_DATA.to_string(), true)
                            } else {
                                (format_value(v), false)
                            }
                        }
                        Values::Time(_) => {
                            let v = c.f64_at(r);
                            if v.is_nan() {
                                (NO_DATA.to_string(), true)
                            } else {
                                (time_full(v, &self.zone), false)
                            }
                        }
                        _ => match c.str_at(r) {
                            Some(s) if !s.is_empty() => (s.replace(['\t', '\n', '\r'], " "), false),
                            _ => (NO_DATA.to_string(), true),
                        },
                    };
                    let mut row = DetailRow::field(c.name.to_string(), text);
                    if quiet {
                        row = row.quiet();
                    }
                    out.push(row);
                }
            }
        }
        if out.is_empty() {
            return;
        }
        // The row's number in the data, where no transform has moved the rows.
        rows.push(DetailRow::section(if untouched {
            format!("Row {}", r + 1)
        } else {
            "Row".to_string()
        }));
        rows.extend(out);
    }

    /// A series' tree.
    pub fn series_detail(&self, key: &SeriesKey) -> Option<DetailTree> {
        let si = self.index_of(key)?;
        let s = &self.series[si];
        let present: Vec<usize> = (0..s.xs.len().min(s.ys.len()))
            .filter(|&i| s.ys[i].is_finite() && !s.xs[i].is_nan())
            .collect();
        let mark = match s.mark {
            MarkKind::Point => "points".to_string(),
            m => format!("{} series", m.name()),
        };
        let mut t = DetailTree::new(self.display_name(key), mark);
        t.swatch = Some(self.swatch(s));
        t.item = Some(Item::Series(key.clone()));
        let point = |i: usize| Item::Point {
            series: key.clone(),
            row: i as u32,
        };
        let at = |i: usize| {
            format!(
                "{} at {}",
                self.value_label(s.ys[i], s.decimals),
                self.x_short(s, s.xs[i])
            )
        };
        let mut rows = vec![DetailRow::field("count", present.len().to_string())];
        if let (Some(&first), Some(&last)) = (present.first(), present.last()) {
            let by = |a: &usize, b: &usize| s.ys[*a].total_cmp(&s.ys[*b]);
            // The first of equal extremes.
            let min = present
                .iter()
                .copied()
                .reduce(|a, b| if by(&b, &a).is_lt() { b } else { a })
                .unwrap_or(first);
            let max = present
                .iter()
                .copied()
                .reduce(|a, b| if by(&b, &a).is_gt() { b } else { a })
                .unwrap_or(first);
            let mean = present.iter().map(|&i| s.ys[i]).sum::<f64>() / present.len() as f64;
            rows.push(DetailRow::field("min", at(min)).naming(point(min)));
            rows.push(DetailRow::field("max", at(max)).naming(point(max)));
            rows.push(DetailRow::field("mean", self.mean_text(mean, s.decimals)));
            rows.push(DetailRow::field("last", at(last)).naming(point(last)));
        }
        let mut style = Vec::new();
        style_rows(&s.style, &mut style);
        if !style.is_empty() {
            rows.push(DetailRow::section("fig"));
            rows.extend(style);
        }
        t.rows = rows;
        Some(t)
    }

    /// A band's tree, its rows naming each part.
    fn band_detail(&self, view: &View, band: u32, part: Option<&SeriesKey>) -> Option<DetailTree> {
        let name = self.x.bands.get(band as usize)?;
        let stacks = self.stacks(view);
        let mut values = Vec::new();
        let mut sums: Vec<(f64, usize)> = Vec::new();
        let mut mark = None;
        for si in walked(self, view) {
            let s = &self.series[si];
            mark.get_or_insert(s.mark);
            let i = (0..s.xs.len()).find(|&i| (s.xs[i] - f64::from(band)).abs() < 1e-9);
            let v = i.map_or(f64::NAN, |i| self.shown(&stacks, si, i));
            let (text, quiet) = self.value_text(v, s.decimals);
            let mut r = DetailRow::mark(
                self.display_name(&s.key),
                text,
                Some(self.swatch(s)),
                Item::Band {
                    band,
                    series: Some(s.key.clone()),
                },
            );
            r.quiet = quiet;
            values.push(r);
            // A total only of parts that stack: grouped bars side by side (two percentiles,
            // say) add up to nothing, as the chart's card has it.
            if v.is_finite() && stacks.hi[si].is_some() {
                sums.push((v, s.decimals));
            }
        }
        let mark = mark.map_or("band", |m| m.name());
        let kind = if values.len() > 1 {
            format!("{mark} · {} series", values.len())
        } else {
            mark.to_string()
        };
        let mut t = DetailTree::new(name.to_string(), kind);
        t.item = Some(Item::Band { band, series: None });
        t.swatch = part.and_then(|k| self.series_of(k)).map(|s| self.swatch(s));
        let mut rows = vec![DetailRow::field(self.x_name(), name.to_string())];
        if !values.is_empty() {
            rows.push(DetailRow::section("Values"));
            rows.extend(values);
            if let Some(total) = self.total(&sums) {
                rows.push(DetailRow::sum("total", total));
            }
        }
        t.rows = rows;
        Some(t)
    }

    /// A heatmap cell's tree.
    fn cell_detail(&self, row: u32, col: u32) -> Option<DetailTree> {
        let (si, i) = self.cell_at(row, col)?;
        let s = &self.series[si];
        let xb = self.x.bands.get(col as usize).map_or("", |b| &**b);
        let yb = self.y.bands.get(row as usize).map_or("", |b| &**b);
        let mut t = DetailTree::new(format!("{xb} · {yb}"), "cell");
        t.item = Some(Item::Cell { row, col });
        let mut rows = vec![
            DetailRow::field(self.x_name(), xb.to_string()),
            DetailRow::field(self.y_name(), yb.to_string()),
        ];
        if let Some(v) = s.sizes.as_ref().and_then(|v| v.get(i)) {
            let name = self
                .chart
                .layers
                .get(usize::from(s.layer))
                .and_then(|l| l.enc.color.as_ref())
                .and_then(|c| c.title.clone().or_else(|| c.field.clone()))
                .unwrap_or_else(|| "value".into());
            let (text, quiet) = self.value_text(*v, s.decimals);
            let mut r = DetailRow::field(name, text);
            if quiet {
                r = r.quiet();
            }
            rows.push(r);
        }
        self.data_rows(s, i, &mut rows);
        t.rows = rows;
        Some(t)
    }

    /// A heatmap's sample at a cell: its series and index, the last drawn of any there.
    pub fn cell_at(&self, row: u32, col: u32) -> Option<(usize, usize)> {
        let mut found = None;
        for (si, s) in self.series.iter().enumerate() {
            if s.mark != MarkKind::Rect {
                continue;
            }
            for i in 0..s.xs.len().min(s.ys.len()) {
                if (s.xs[i] - f64::from(col)).abs() < 1e-9
                    && (s.ys[i] - f64::from(row)).abs() < 1e-9
                {
                    found = Some((si, i));
                }
            }
        }
        found
    }
}

/// A time to the second in a zone, its date first, a whole minute without its seconds:
/// "2026-11-21 10:25".
pub fn time_full(v: f64, zone: &jiff::tz::TimeZone) -> String {
    scale::time::zoned(v, zone).map_or_else(String::new, |z| {
        let s = z.strftime("%Y-%m-%d %H:%M:%S").to_string();
        s.strip_suffix(":00").map_or(s.clone(), str::to_string)
    })
}
