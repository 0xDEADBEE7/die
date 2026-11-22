//! A heatmap's colour scale and its legend.
//!
//! **Sequential** (the default): one hue, blue, from the least value to the most. On a light
//! surface the least is the light end (step 150 of the dataviz palette's blue ramp) and the
//! most the dark (650); on a dark surface the anchor flips, the least the dark end (650) and the
//! most the light (150), so a near-nothing cell recedes toward the surface in both, as
//! palette.md has a sequential ramp do. Six steps, each 0.095 apart in OKLCH lightness, one hue
//! (checked with the dataviz skill's `validate_palette.js --ordinal` against `#fcfcfb` and
//! `#1a1a1a`: monotone, the gaps, the hue; its 2:1 floor for the end nearest the surface is an
//! ordinal ramp's, which palette.md waives for a sequential one, and the ends sit at 1.5:1 and
//! 1.8:1).
//!
//! **Diverging** (`"scale": {"type": "diverging", "mid": 0}`): red below the midpoint, blue
//! above, three steps an arm stepping away from a neutral grey (`#f0efec` light, `#383835`
//! dark), the arms at matched lightness: the blue's palette steps 250, 450, 650 (light) or 550,
//! 350, 150 (dark) and a red at each of their lightnesses (hue 25°, the palette's red). Equal
//! distances from the midpoint take equal colours: the scale reaches as far below it as the
//! farther end of the data does above, or the other way.
//!
//! Colours between steps are mixed in OKLab, so the ramp's lightness runs evenly.

use super::ChartModel;
use super::layout::{BODY, Ink, Label, Role, TICK};
use crate::figure::short_number;
use crate::scale::format_with;
use crate::scene::{Anchor, Item, Measure, Paint, PathData, Rect, Scene, Stroke, TextSpec};
use crate::spec::MarkKind;
use crate::theme::{Color, Theme};

/// The sequential ramp on a light surface: blue 150, 250, 350, 450, 550, 650, least to most.
pub const SEQUENTIAL_LIGHT: [Color; 6] = [
    Color(0xb7, 0xd3, 0xf6),
    Color(0x86, 0xb6, 0xef),
    Color(0x55, 0x98, 0xe7),
    Color(0x2a, 0x78, 0xd6),
    Color(0x1c, 0x5c, 0xab),
    Color(0x10, 0x42, 0x81),
];
/// The sequential ramp on a dark surface: blue 650, 550, 450, 350, 250, 150, least to most.
pub const SEQUENTIAL_DARK: [Color; 6] = [
    Color(0x10, 0x42, 0x81),
    Color(0x1c, 0x5c, 0xab),
    Color(0x2a, 0x78, 0xd6),
    Color(0x55, 0x98, 0xe7),
    Color(0x86, 0xb6, 0xef),
    Color(0xb7, 0xd3, 0xf6),
];
/// The diverging scale on a light surface, least to most: red 650, 450, 250, the grey
/// midpoint, blue 250, 450, 650.
pub const DIVERGING_LIGHT: [Color; 7] = [
    Color(0x78, 0x1e, 0x1f),
    Color(0xca, 0x44, 0x42),
    Color(0xec, 0x99, 0x92),
    Color(0xf0, 0xef, 0xec),
    Color(0x86, 0xb6, 0xef),
    Color(0x2a, 0x78, 0xd6),
    Color(0x10, 0x42, 0x81),
];
/// The diverging scale on a dark surface, least to most: red 150, 350, 550, the grey
/// midpoint, blue 550, 350, 150.
pub const DIVERGING_DARK: [Color; 7] = [
    Color(0xf6, 0xc2, 0xbd),
    Color(0xe0, 0x6e, 0x68),
    Color(0xa1, 0x30, 0x2f),
    Color(0x38, 0x38, 0x35),
    Color(0x1c, 0x5c, 0xab),
    Color(0x55, 0x98, 0xe7),
    Color(0xb7, 0xd3, 0xf6),
];

/// The colour legend's ramp: its width and height, and the gap from the field's name to it.
pub const RAMP_W: f32 = 160.0;
/// The ramp's height.
pub const RAMP_H: f32 = 8.0;
const NAME_GAP: f32 = 12.0;
/// The narrowest the ramp gets before the legend goes.
const RAMP_MIN: f32 = 96.0;
/// A tick under the ramp: its length, and from its end to its label's top.
const TICK_LEN: f32 = 3.0;
const TICK_TO_LABEL: f32 = 2.0;
/// The legend's row: the name and the ramp share its middle.
const ROW: f32 = 20.0;

/// How a heatmap's values map to colours.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ColorScale {
    /// One hue, from the least value (`lo`) to the most (`hi`).
    Sequential {
        /// The least value.
        lo: f64,
        /// The most.
        hi: f64,
    },
    /// Two hues either side of `mid`, over the data's `lo` to `hi`.
    Diverging {
        /// The least value.
        lo: f64,
        /// The midpoint, the neutral grey.
        mid: f64,
        /// The most.
        hi: f64,
    },
}

impl ColorScale {
    /// The steps the colours run through, least first, on the theme's surface.
    pub fn steps(&self, theme: &Theme) -> &'static [Color] {
        match (self, theme.dark) {
            (ColorScale::Sequential { .. }, false) => &SEQUENTIAL_LIGHT,
            (ColorScale::Sequential { .. }, true) => &SEQUENTIAL_DARK,
            (ColorScale::Diverging { .. }, false) => &DIVERGING_LIGHT,
            (ColorScale::Diverging { .. }, true) => &DIVERGING_DARK,
        }
    }

    /// The span along the ramp: the data's for a sequential scale, and for a diverging one as
    /// far either side of the midpoint as the data reaches on its farther side.
    pub fn span(&self) -> (f64, f64) {
        match *self {
            ColorScale::Sequential { lo, hi } => (lo, hi),
            ColorScale::Diverging { lo, mid, hi } => {
                let d = (mid - lo).abs().max((hi - mid).abs());
                (mid - d, mid + d)
            }
        }
    }

    /// Where `v` falls along the ramp, 0 to 1 (clamped); None for a missing value. A scale
    /// whose values are all one sits at its most (sequential) or its midpoint (diverging).
    pub fn position(&self, v: f64) -> Option<f32> {
        if !v.is_finite() {
            return None;
        }
        let (a, b) = self.span();
        if b > a {
            return Some(((v - a) / (b - a)).clamp(0.0, 1.0) as f32);
        }
        Some(match self {
            ColorScale::Sequential { .. } => 1.0,
            ColorScale::Diverging { .. } => 0.5,
        })
    }

    /// The colour of `v`; None for a missing value.
    pub fn color(&self, v: f64, theme: &Theme) -> Option<Color> {
        self.position(v).map(|t| along(self.steps(theme), t))
    }

    /// The ramp drawn as a gradient: its colours at even places, mixed as the cells are.
    pub fn stops(&self, theme: &Theme) -> Vec<(f32, Color)> {
        let steps = self.steps(theme);
        let n = 4 * (steps.len() - 1);
        (0..=n)
            .map(|i| {
                let t = i as f32 / n as f32;
                (t, along(steps, t))
            })
            .collect()
    }
}

/// The colour `t` (0 to 1) along evenly spaced `steps`, mixed in OKLab.
pub fn along(steps: &[Color], t: f32) -> Color {
    match steps.len() {
        0 => Color::default(),
        1 => steps[0],
        n => {
            let x = t.clamp(0.0, 1.0) * (n - 1) as f32;
            let i = (x.floor() as usize).min(n - 2);
            mix_oklab(steps[i], steps[i + 1], x - i as f32)
        }
    }
}

/// `a` mixed toward `b` by `t` in OKLab.
pub fn mix_oklab(a: Color, b: Color, t: f32) -> Color {
    let (la, lb) = (oklab(a), oklab(b));
    let m = |i: usize| la[i] + (lb[i] - la[i]) * t;
    from_oklab([m(0), m(1), m(2)])
}

fn to_linear(c: u8) -> f32 {
    let c = f32::from(c) / 255.0;
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn to_srgb(c: f32) -> u8 {
    let c = c.clamp(0.0, 1.0);
    let s = if c <= 0.003_130_8 {
        12.92 * c
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    };
    (s * 255.0).round().clamp(0.0, 255.0) as u8
}

/// A colour in OKLab.
fn oklab(c: Color) -> [f32; 3] {
    let (r, g, b) = (to_linear(c.0), to_linear(c.1), to_linear(c.2));
    let l = (0.412_221_46 * r + 0.536_332_55 * g + 0.051_445_995 * b).cbrt();
    let m = (0.211_903_5 * r + 0.680_699_5 * g + 0.107_396_96 * b).cbrt();
    let s = (0.088_302_46 * r + 0.281_718_85 * g + 0.629_978_7 * b).cbrt();
    [
        0.210_454_26 * l + 0.793_617_8 * m - 0.004_072_047 * s,
        1.977_998_5 * l - 2.428_592_2 * m + 0.450_593_7 * s,
        0.025_904_037 * l + 0.782_771_77 * m - 0.808_675_77 * s,
    ]
}

/// A colour from OKLab, clamped into sRGB.
fn from_oklab([l, a, b]: [f32; 3]) -> Color {
    let l_ = (l + 0.396_337_78 * a + 0.215_803_76 * b).powi(3);
    let m_ = (l - 0.105_561_346 * a - 0.063_854_17 * b).powi(3);
    let s_ = (l - 0.089_484_18 * a - 1.291_485_5 * b).powi(3);
    Color(
        to_srgb(4.076_741_7 * l_ - 3.307_711_6 * m_ + 0.230_969_94 * s_),
        to_srgb(-1.268_438 * l_ + 2.609_757_4 * m_ - 0.341_319_38 * s_),
        to_srgb(-0.004_196_086_3 * l_ - 0.703_418_6 * m_ + 1.707_614_7 * s_),
    )
}

/// A heatmap's colour scale over its cells' values: diverging when its colour channel names a
/// midpoint, else sequential. None for a chart that isn't a heatmap, or has no values.
pub fn scale_of(model: &ChartModel) -> Option<ColorScale> {
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    let mut mid = None;
    for s in model.series.iter().filter(|s| s.mark == MarkKind::Rect) {
        for &v in s.sizes.iter().flat_map(|v| v.iter()) {
            if v.is_finite() {
                lo = lo.min(v);
                hi = hi.max(v);
            }
        }
        if mid.is_none() {
            mid = model
                .chart
                .layers
                .get(usize::from(s.layer))
                .and_then(|l| l.enc.color.as_ref())
                .and_then(|c| c.mid);
        }
    }
    if lo > hi {
        return None;
    }
    Some(match mid {
        Some(mid) => ColorScale::Diverging {
            lo: lo.min(mid),
            mid,
            hi: hi.max(mid),
        },
        None => ColorScale::Sequential { lo, hi },
    })
}

/// What the colour channel is called: its title, else its field.
pub fn field_name(model: &ChartModel) -> String {
    model
        .series
        .iter()
        .find(|s| s.mark == MarkKind::Rect)
        .and_then(|s| model.chart.layers.get(usize::from(s.layer)))
        .and_then(|l| l.enc.color.as_ref())
        .and_then(|c| c.title.clone().or_else(|| c.field.clone()))
        .unwrap_or_default()
}

/// A value as the legend's ticks write it: the colour channel's format, else abbreviated.
pub fn value_text(model: &ChartModel, v: f64) -> String {
    let format = model
        .series
        .iter()
        .find(|s| s.mark == MarkKind::Rect)
        .and_then(|s| model.chart.layers.get(usize::from(s.layer)))
        .and_then(|l| l.enc.color.as_ref())
        .and_then(|c| c.format.clone());
    match format {
        Some(f) => format_with(v, &f, &model.zone),
        None => short_number(v),
    }
}

/// The colour legend, placed: the field's name, the ramp, and a tick under it at the least and
/// the most value (and the midpoint on a diverging scale), each labelled.
#[derive(Clone, Debug, PartialEq)]
pub struct RampLegend {
    /// The scale it shows.
    pub scale: ColorScale,
    /// The field's name, if it's shown.
    pub name: Option<Label>,
    /// The ramp's rectangle.
    pub bar: Rect,
    /// The ticks: where across, and their labels.
    pub ticks: Vec<(f32, Label)>,
    /// Everything it takes.
    pub bounds: Rect,
}

impl RampLegend {
    /// The legend at `(x, top)`, no wider than `room`: the name beside the ramp (`beside`),
    /// or over it in a column of its own (a legend at the right). None where even a narrow
    /// ramp doesn't fit.
    pub fn place(
        model: &ChartModel,
        scale: ColorScale,
        x: f32,
        top: f32,
        room: f32,
        beside: bool,
        m: &mut dyn Measure,
    ) -> Option<RampLegend> {
        let body = TextSpec::new(BODY);
        let tick = TextSpec::new(TICK).tabular();
        let name = field_name(model);
        let (lo, hi) = match scale {
            ColorScale::Sequential { lo, hi } | ColorScale::Diverging { lo, hi, .. } => (lo, hi),
        };
        let (lo_t, hi_t) = (value_text(model, lo), value_text(model, hi));
        // The end labels sit under the ramp's ends, anchored to them: the ramp is at least as
        // wide as both with a gap between.
        let labels_w = m.width(&lo_t, tick) + m.width(&hi_t, tick) + 8.0;
        let need = RAMP_MIN.max(labels_w);
        let name_w = if name.is_empty() {
            0.0
        } else {
            m.width(&name, body)
        };
        let mut name_shown = !name.is_empty();
        let mid = top + ROW / 2.0;
        let (bar_x, bar_y, bar_w) = if beside {
            let fixed = |shown: bool| if shown { name_w + NAME_GAP } else { 0.0 };
            if room - fixed(name_shown) < need {
                name_shown = false;
            }
            let avail = room - fixed(name_shown);
            if avail < need {
                return None;
            }
            (
                x + fixed(name_shown),
                mid - RAMP_H / 2.0,
                RAMP_W.max(labels_w).min(avail),
            )
        } else {
            if room < need {
                return None;
            }
            let y = if name_shown {
                mid + ROW / 2.0 + 2.0
            } else {
                mid - RAMP_H / 2.0
            };
            (x, y, RAMP_W.max(labels_w).min(room))
        };
        let bar = Rect::new(bar_x, bar_y, bar_w, RAMP_H);
        let name = name_shown.then(|| Label {
            role: Role::Legend,
            text: m.fit(&name, body, if beside { name_w } else { room }),
            x,
            y: mid + m.cap(body) / 2.0,
            spec: body,
            anchor: Anchor::Start,
            ink: Ink::Text,
            alpha: 1.0,
        });
        let base = bar.bottom() + TICK_LEN + TICK_TO_LABEL + m.ascent(tick);
        let label = |text: String, x: f32, anchor: Anchor| Label {
            role: Role::Legend,
            text,
            x,
            y: base,
            spec: tick,
            anchor,
            ink: Ink::Muted,
            alpha: 1.0,
        };
        let at = |v: f64| bar.x + scale.position(v).unwrap_or(0.0) * bar.w;
        let mut ticks = vec![
            (at(lo), label(lo_t, at(lo), Anchor::Start)),
            (at(hi), label(hi_t, at(hi), Anchor::End)),
        ];
        if let ColorScale::Diverging { mid, .. } = scale {
            let t = label(value_text(model, mid), at(mid), Anchor::Middle);
            let b = t.bounds(m);
            let clear = ticks
                .iter()
                .all(|(_, l)| l.bounds(m).inflate(3.0).intersect(&b).is_none());
            if clear {
                ticks.insert(1, (at(mid), t));
            }
        }
        let mut bounds = bar;
        if let Some(n) = &name {
            bounds = bounds.union(&n.bounds(m));
        }
        for (_, l) in &ticks {
            bounds = bounds.union(&l.bounds(m));
        }
        Some(RampLegend {
            scale,
            name,
            bar,
            ticks,
            bounds,
        })
    }

    /// The legend's height from its top, beside its name.
    pub fn height(m: &mut dyn Measure) -> f32 {
        let tick = TextSpec::new(TICK).tabular();
        ROW / 2.0 + RAMP_H / 2.0 + TICK_LEN + TICK_TO_LABEL + m.ascent(tick) + m.descent(tick)
    }

    /// The legend drawn.
    pub fn draw(&self, scene: &mut Scene, theme: &Theme) {
        if let Some(n) = &self.name {
            scene.push(n.item(theme));
        }
        let stops = self
            .scale
            .stops(theme)
            .into_iter()
            .map(|(t, c)| (t, c, 1.0))
            .collect();
        let b = self.bar;
        scene.push(Item::Path {
            path: PathData::rounded(b, [2.0; 4]),
            fill: Some(Paint::Linear {
                from: (b.x, b.y),
                to: (b.right(), b.y),
                stops,
            }),
            stroke: None,
        });
        for (x, l) in &self.ticks {
            scene.push(Item::Path {
                path: PathData::polyline(&[(*x, b.bottom()), (*x, b.bottom() + TICK_LEN)]),
                fill: None,
                stroke: Some(Stroke::hairline(theme.muted, 0.8)),
            });
            scene.push(l.item(theme));
        }
    }
}
