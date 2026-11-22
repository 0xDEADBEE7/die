//! A dashboard's view: the time its linked panels share, each panel's own view and cursor, the
//! focus, a panel shown full size, and the scroll; the time ranges the chip offers, and what
//! the chip says.

use super::{DashModel, PanelState};
use crate::interact::{Cursor, View};
use crate::plot::ErrorView;

/// A time range the chip offers.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Range {
    /// Its name in a spec and the prompt: `6h`.
    pub key: &'static str,
    /// What the chip says: `Last 6 hours`.
    pub label: &'static str,
    /// How long, in seconds; None for all the data.
    pub secs: Option<f64>,
}

/// The ranges `T` and the chip offer, in order: 15m, 1h, 6h, 24h, 7d, 30d and all.
pub const RANGES: [Range; 7] = [
    Range {
        key: "15m",
        label: "Last 15 minutes",
        secs: Some(900.0),
    },
    Range {
        key: "1h",
        label: "Last hour",
        secs: Some(3600.0),
    },
    Range {
        key: "6h",
        label: "Last 6 hours",
        secs: Some(21_600.0),
    },
    Range {
        key: "24h",
        label: "Last 24 hours",
        secs: Some(86_400.0),
    },
    Range {
        key: "7d",
        label: "Last 7 days",
        secs: Some(604_800.0),
    },
    Range {
        key: "30d",
        label: "Last 30 days",
        secs: Some(2_592_000.0),
    },
    Range {
        key: "all",
        label: "All time",
        secs: None,
    },
];

/// What the chip says of a range of `secs` ending at the newest sample: a preset's label, else
/// `Last 2 hours`, `Last 90 minutes`, `Last 3 days`.
pub fn last_label(secs: f64) -> String {
    if let Some(r) = RANGES.iter().find(|r| r.secs == Some(secs)) {
        return r.label.to_string();
    }
    let whole = |unit: f64| (secs / unit).fract().abs() < 1e-9;
    let (n, one, many) = if secs >= 2.0 * 86_400.0 && whole(86_400.0) {
        (secs / 86_400.0, "day", "days")
    } else if secs >= 3600.0 && whole(3600.0) {
        (secs / 3600.0, "hour", "hours")
    } else if secs >= 60.0 && whole(60.0) {
        (secs / 60.0, "minute", "minutes")
    } else {
        (secs.round(), "second", "seconds")
    };
    match n as u64 {
        1 => format!("Last {one}"),
        n => format!("Last {n} {many}"),
    }
}

/// One panel's own view.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PanelView {
    /// Its figure's view; a linked panel's x is the dashboard's time instead.
    pub view: View,
    /// Its keyboard cursor.
    pub cursor: Option<Cursor>,
    /// Its error figure's state, when it shows one (`!` turns it over).
    pub error: ErrorView,
    /// A tile's keyboard cursor on its trend: the point, which drives the linked moment as a
    /// chart's cursor does.
    pub point: Option<usize>,
    /// The series brought forward past the pointer's say: a search's find (until `Esc`), or
    /// the series of the detail panel's row under the pointer.
    pub emphasis: Option<crate::data::SeriesKey>,
}

/// What a dashboard shows of its data.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DashView {
    /// The x domain its linked panels share; None for all of their data.
    pub time: Option<(f64, f64)>,
    /// Each panel's own view, by number.
    pub panels: Vec<PanelView>,
    /// The panel with the focus: None until Tab or a click gives it one, so nothing's ringed
    /// when a dashboard opens.
    pub focused: Option<usize>,
    /// The panel shown full size, if one.
    pub full: Option<usize>,
    /// How far the rows are scrolled up, in logical px.
    pub scroll: f32,
}

impl DashView {
    /// The view a dashboard opens with: `time.range` ending at the newest linked sample (all
    /// of the data without a range), each figure's own opening view.
    pub fn initial(model: &DashModel) -> DashView {
        let mut v = DashView {
            time: model.range.and_then(|r| last(model, r)),
            ..DashView::default()
        };
        v.fit(model);
        v
    }

    /// Makes `panels` as many as the model's, keeping those it has (the model was built
    /// again, as many panels as before or not).
    pub fn fit(&mut self, model: &DashModel) {
        self.panels.truncate(model.panels.len());
        while self.panels.len() < model.panels.len() {
            let i = self.panels.len();
            let view = match &model.panels[i].state {
                PanelState::Chart(m) => m.initial_view(),
                PanelState::Graph(g) => g.initial_view(),
                _ => View::default(),
            };
            self.panels.push(PanelView {
                view,
                ..PanelView::default()
            });
        }
        self.focused = self.focused.filter(|&f| f < model.panels.len());
        self.full = self.full.filter(|&f| f < model.panels.len());
    }

    /// The x domain linked panels show: the time view, else all of their data.
    pub fn shared(&self, model: &DashModel) -> Option<(f64, f64)> {
        self.time.or_else(|| model.extent())
    }

    /// Panel `i`'s view as its figure draws it: its own, with the shared time for x when it's
    /// linked.
    pub fn panel(&self, model: &DashModel, i: usize) -> View {
        let mut v = self
            .panels
            .get(i)
            .map(|p| p.view.clone())
            .unwrap_or_default();
        if model
            .panels
            .get(i)
            .is_some_and(|p| p.linked && p.chart().is_some())
        {
            v.x = self.shared(model);
        }
        v
    }

    /// The view with panel `i`'s changed to `v` (a zoom, a pan, a brush, a toggle): a linked
    /// panel's x goes to the time all linked panels share, the rest stays its own.
    pub fn with_panel(&self, model: &DashModel, i: usize, v: View) -> DashView {
        let mut out = self.clone();
        let linked = model.panels.get(i).is_some_and(|p| p.linked);
        let mut v = v;
        if linked {
            out.time = v.x.take();
        }
        if let Some(p) = out.panels.get_mut(i) {
            p.view = v;
        }
        out
    }

    /// The view on a range of `secs` ending at the newest linked sample, or on all the data
    /// (None): a choice of the chip's.
    pub fn with_range(&self, model: &DashModel, secs: Option<f64>) -> DashView {
        DashView {
            time: secs.and_then(|s| last(model, s)),
            ..self.clone()
        }
    }

    /// The time back to `time.range` (a double-click on a linked plot).
    pub fn reset_time(&self, model: &DashModel) -> DashView {
        self.with_range(model, model.range)
    }

    /// What the chip says: the range's label when the time view is a range ending at the
    /// newest sample (`Last 6 hours`), `All time` for all of it, else its ends (`10:05 –
    /// 11:40`). None without a `time`.
    pub fn chip(&self, model: &DashModel) -> Option<String> {
        model.time.as_ref()?;
        let Some((a, b)) = self.time else {
            // No data yet to end a range at (a live source still coming): the spec's range.
            return Some(match (model.range, model.newest()) {
                (Some(r), None) => last_label(r),
                _ => RANGES[6].label.to_string(),
            });
        };
        if let Some(n) = model.newest() {
            let span = b - a;
            let near = |x: f64, y: f64| (x - y).abs() <= 1e-6 * span.abs().max(1.0);
            if near(b, n) && span > 0.0 {
                return Some(last_label((span * 1000.0).round() / 1000.0));
            }
        }
        Some(custom_label(a, b, &model.zone))
    }
}

/// `secs` ending at the newest linked sample.
fn last(model: &DashModel, secs: f64) -> Option<(f64, f64)> {
    model.newest().map(|n| (n - secs, n))
}

/// A custom range's ends as the chip writes them: `10:05 – 11:40` within a day, with the
/// seconds under two minutes, `Nov 21 10:05 – Nov 22 11:40` across days, `Nov 21 – Nov 28`
/// over two days or more.
pub fn custom_label(a: f64, b: f64, zone: &jiff::tz::TimeZone) -> String {
    let at = |v: f64, fmt: &str| -> String {
        let Ok(ts) = jiff::Timestamp::from_microsecond((v * 1e6).round() as i64) else {
            return String::new();
        };
        ts.to_zoned(zone.clone()).strftime(fmt).to_string()
    };
    let span = b - a;
    let same_day = at(a, "%F") == at(b, "%F");
    if span < 120.0 && same_day {
        format!("{} – {}", at(a, "%H:%M:%S"), at(b, "%H:%M:%S"))
    } else if same_day {
        format!("{} – {}", at(a, "%H:%M"), at(b, "%H:%M"))
    } else if span < 2.0 * 86_400.0 {
        format!("{} – {}", at(a, "%b %-d %H:%M"), at(b, "%b %-d %H:%M"))
    } else {
        format!("{} – {}", at(a, "%b %-d"), at(b, "%b %-d"))
    }
}

/// A view part way between two, for an ease (a range chosen eases, `MOTION`): the time and
/// each panel's domains interpolated, everything else `to`'s.
pub fn tween(from: &DashView, to: &DashView, t: f32) -> DashView {
    let t = t.clamp(0.0, 1.0);
    let lerp = |a: f64, b: f64| a + (b - a) * f64::from(t);
    let time = match (from.time, to.time) {
        (Some(a), Some(b)) => Some((lerp(a.0, b.0), lerp(a.1, b.1))),
        _ => to.time,
    };
    let panels = to
        .panels
        .iter()
        .enumerate()
        .map(|(i, p)| match from.panels.get(i) {
            Some(f) => PanelView {
                view: crate::plot::tween(&f.view, &p.view, t),
                ..p.clone()
            },
            None => p.clone(),
        })
        .collect();
    DashView {
        time,
        panels,
        ..to.clone()
    }
}
