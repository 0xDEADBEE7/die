//! The chart's anatomy: the title, the legend, the axes and the plot's rectangle, at a size.
//! The stand-in's (see [`super`]): fixed margins, one legend row.

use super::{ChartModel, XKind};
use crate::data::SeriesKey;
use crate::interact::View;
use crate::scale::Scale;
use crate::scene::{Anchor, Hit, Item, Measure, PathData, Rect, Scene, TextSpec};
use crate::theme::Theme;

/// Where everything landed: the plot rect, scales with ranges, legend rows.
pub struct ChartLayout {
    /// The plot's rectangle.
    pub plot: Rect,
    /// The x scale, onto the plot.
    pub x: Scale,
    /// The y scale, onto the plot.
    pub y: Scale,
    /// Each legend key's rectangle.
    pub legend: Vec<(SeriesKey, Rect)>,
    /// The x axis's tick-label strip: drag and scroll there act on x alone.
    pub x_axis: Rect,
    /// The y axis's tick-label strip: drag and scroll there act on y alone.
    pub y_axis: Rect,
    /// The title's box.
    pub title: Rect,
}

/// The stand-in's layout: 56 px left for the y axis, 28 under for the x axis, the legend's row
/// and the title above.
pub(crate) fn lay_out(model: &ChartModel, view: &View, size: (f32, f32)) -> ChartLayout {
    let (w, h) = size;
    let top = if model.chart.title.is_some() {
        48.0
    } else {
        28.0
    };
    let plot = Rect::new(
        56.0,
        top,
        (w - 56.0 - 16.0).max(8.0),
        (h - top - 28.0).max(8.0),
    );
    let (fx, fy) = model.fit(view);
    let (dx, dy) = (view.x.unwrap_or(fx), view.y.unwrap_or(fy));
    let xr = (plot.x, plot.right());
    let x = match model.kind {
        XKind::Time => Scale::Time {
            domain: dx,
            range: xr,
            zone: model.zone.clone(),
        },
        XKind::Number => Scale::Linear {
            domain: dx,
            range: xr,
        },
        XKind::Band => Scale::Band {
            domain: model.bands.clone(),
            range: xr,
            inner: 0.2,
            outer: 0.1,
        },
    };
    let y = Scale::Linear {
        domain: dy,
        range: (plot.bottom(), plot.y),
    };
    let legend = if model.series.len() > 1 {
        let mut at = plot.x;
        let y0 = top - 22.0;
        (model.series.iter())
            .map(|s| {
                let r = Rect::new(at, y0, 96.0, 16.0);
                at += 104.0;
                (s.key.clone(), r)
            })
            .collect()
    } else {
        vec![]
    };
    ChartLayout {
        x_axis: Rect::new(plot.x, plot.bottom(), plot.w, 24.0),
        y_axis: Rect::new(0.0, plot.y, plot.x - 4.0, plot.h),
        title: Rect::new(plot.x, 4.0, plot.w, 20.0),
        plot,
        x,
        y,
        legend,
    }
}

/// The error banner over the top of the area (a parse error with its line and column, a missing
/// file): drawn by the surface onto the base when the last parse failed but a figure is kept.
pub fn banner(scene: &mut Scene, text: &str, theme: &Theme, m: &mut dyn Measure) {
    let r = Rect::new(0.0, 0.0, scene.width, 24.0);
    scene.push(Item::Path {
        path: PathData::rect(r),
        fill: Some(crate::scene::Paint::solid(theme.status[3], 1.0)),
        stroke: None,
    });
    let spec = TextSpec::new(12.0);
    let max = (r.w - 16.0).max(0.0);
    scene.push(Item::Text {
        text: m.fit(text, spec, max).into(),
        x: 8.0,
        y: 16.0,
        spec,
        anchor: Anchor::Start,
        color: theme.strong,
        alpha: 1.0,
        max_width: None,
    });
    scene.region(r, Hit::Banner);
}

/// What an empty or failed figure shows in place of a chart ("No data in requests.csv").
pub fn empty(
    size: (f32, f32),
    scale: f32,
    text: &str,
    theme: &Theme,
    m: &mut dyn Measure,
) -> Scene {
    let mut scene = Scene::new(size.0, size.1, scale);
    let spec = TextSpec::new(13.0);
    scene.push(Item::Text {
        text: m.fit(text, spec, size.0 - 16.0).into(),
        x: size.0 / 2.0,
        y: size.1 / 2.0,
        spec,
        anchor: Anchor::Middle,
        color: theme.muted,
        alpha: 1.0,
        max_width: None,
    });
    scene.region(Rect::new(0.0, 0.0, size.0, size.1), Hit::Chrome);
    scene
}
