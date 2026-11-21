//! fig_shot: a data or spec file drawn as `\v` would draw it, base and overlay, written as a PNG
//! with die-draw's fonts. For judging the look without DIE:
//!
//! ```sh
//! cargo run --release -p die-fig --example fig_shot -- <file> <out.png> [--size 880x470] \
//!     [--scale 2] [--light] [--cursor <n>] [--hover <x>,<y>] [--brush <x0>,<x1>[,<y0>,<y1>]] \
//!     [--reference <n>] [--svg <out.svg>]
//! ```
//!
//! The size is the area's in logical px, the scale the display's. `--cursor` puts the keyboard's
//! cursor on the nth sample of the first series (the nth band on a band chart), `--reference`
//! the reference there. `--hover` is the pointer at a place in the area: the overlay shows what
//! it's on, with the series nearest it brought forward. `--brush` draws the brush a left drag
//! from x0 to x1 makes (with y0 and y1 for a box). `--svg` writes the 1× layout as SVG too.
//! Times without an offset are in the system's zone. It prints where the plot landed and what
//! the overlay shows.

use die_test_cap as _;

use die_draw::Book;
use die_fig::data::ReadOptions;
use die_fig::interact::{BrushKind, Cursor, View};
use die_fig::plot::{self, ChartModel, OverlayState};
use die_fig::raster::{self, BookMeasure};
use die_fig::scene::Scene;
use die_fig::spec::{self, Figure};
use die_fig::theme::Theme;
use jiff::tz::TimeZone;
use std::path::Path;

fn main() {
    if let Err(wrong) = run() {
        eprintln!("fig_shot: {wrong}");
        std::process::exit(1);
    }
}

/// What the flags ask for.
struct Ask {
    size: (f32, f32),
    scale: f32,
    light: bool,
    cursor: Option<usize>,
    reference: Option<usize>,
    hover: Option<(f32, f32)>,
    brush: Option<Vec<f32>>,
    svg: Option<String>,
}

fn pair(v: &str, sep: char) -> Result<Vec<f32>, String> {
    v.split(sep)
        .map(|s| {
            s.trim()
                .parse::<f32>()
                .map_err(|_| format!("{v}: not numbers"))
        })
        .collect()
}

fn run() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let usage = "fig_shot <file> <out.png> [--size WxH] [--scale S] [--light] [--cursor N] \
                 [--hover X,Y] [--brush X0,X1[,Y0,Y1]] [--reference N] [--svg OUT.svg]";
    let (Some(input), Some(output)) = (args.next(), args.next()) else {
        return Err(usage.to_owned());
    };
    let mut ask = Ask {
        size: (880.0, 470.0),
        scale: 2.0,
        light: false,
        cursor: None,
        reference: None,
        hover: None,
        brush: None,
        svg: None,
    };
    while let Some(flag) = args.next() {
        let mut value = || args.next().ok_or(format!("{flag} needs a value"));
        match flag.as_str() {
            "--size" => {
                let v = pair(&value()?, 'x')?;
                if v.len() != 2 {
                    return Err("--size is WxH".into());
                }
                ask.size = (v[0], v[1]);
            }
            "--scale" => ask.scale = pair(&value()?, ',')?[0],
            "--light" => ask.light = true,
            "--cursor" => ask.cursor = Some(pair(&value()?, ',')?[0] as usize),
            "--reference" => ask.reference = Some(pair(&value()?, ',')?[0] as usize),
            "--hover" => {
                let v = pair(&value()?, ',')?;
                if v.len() != 2 {
                    return Err("--hover is X,Y".into());
                }
                ask.hover = Some((v[0], v[1]));
            }
            "--brush" => ask.brush = Some(pair(&value()?, ',')?),
            "--svg" => ask.svg = Some(value()?),
            other => return Err(format!("{other}: no such flag\n{usage}")),
        }
    }
    let path = Path::new(&input);
    let text = std::fs::read_to_string(path).map_err(|e| format!("{input}: {e}"))?;
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    let dir = path.parent().unwrap_or(Path::new("."));
    let zone = TimeZone::system();
    let opt = ReadOptions::new(zone.clone());
    let parsed = spec::parse(&text, name, dir, &opt).map_err(|e| e.to_string())?;
    for w in &parsed.warnings {
        eprintln!("fig_shot: warning: {w}");
    }
    let Figure::Chart(chart) = parsed.figure else {
        return Err("fig_shot draws charts; this is another kind of figure".into());
    };
    let tables = plot::read_tables(&chart, Some(&text), &opt).map_err(|e| e.to_string())?;
    let model = ChartModel::build(chart, tables, &zone).map_err(|e| e.to_string())?;
    let theme = if ask.light {
        Theme::light()
    } else {
        Theme::mono_dark()
    };
    let mut view = model.initial_view();
    let first = model.keys().into_iter().next();
    let cursor_at = |n: usize| -> Option<Cursor> {
        if model.is_band() {
            return Some(Cursor::Band {
                band: n.min(model.x.bands.len().saturating_sub(1)) as u32,
                series: None,
            });
        }
        let k = first.clone()?;
        let si = model.index_of(&k)?;
        let len = model.series[si].xs.len();
        Some(Cursor::Point {
            series: k,
            row: n.min(len.saturating_sub(1)) as u32,
        })
    };
    view.reference = ask.reference.and_then(cursor_at);
    let mut book = Book::load(None)?;
    let scene = draw(
        &model, &view, &ask, &theme, ask.scale, &mut book, &cursor_at, true,
    )?;
    let bytes = raster::png(&scene, theme.surface, Some(&mut book))?;
    std::fs::write(&output, bytes).map_err(|e| format!("{output}: {e}"))?;
    if let Some(svg) = &ask.svg {
        let scene = draw(
            &model, &view, &ask, &theme, 1.0, &mut book, &cursor_at, false,
        )?;
        std::fs::write(svg, die_fig::svg::write(&scene)).map_err(|e| format!("{svg}: {e}"))?;
    }
    Ok(())
}

/// The base and the overlay as one scene at a scale.
#[allow(clippy::too_many_arguments)]
fn draw(
    model: &ChartModel,
    view: &View,
    ask: &Ask,
    theme: &Theme,
    scale: f32,
    book: &mut Book,
    cursor_at: &dyn Fn(usize) -> Option<Cursor>,
    say: bool,
) -> Result<Scene, String> {
    let mut m = BookMeasure { book, scale };
    let (mut scene, layout, hits) = plot::base(model, view, ask.size, scale, theme, &mut m);
    let hover = ask
        .hover
        .and_then(|(x, y)| plot::hover(model, &layout, &hits, view, x, y));
    let emphasis = ask
        .hover
        .and_then(|(x, y)| plot::nearest_series(model, &layout, &hits, view, x, y));
    let brush = ask.brush.as_ref().map(|b| {
        let p = layout.plot;
        let kind = plot::brush_kind(model);
        let (y0, y1) = match b.as_slice() {
            [_, _, y0, y1, ..] => (*y0, *y1),
            _ => (p.y + p.h * 0.25, p.y + p.h * 0.75),
        };
        let x1 = b.get(1).copied().unwrap_or(b[0]);
        let kind = if b.len() >= 4 && kind == BrushKind::X {
            BrushKind::Box
        } else {
            kind
        };
        (kind, (b[0], y0), (x1, y1))
    });
    let st = OverlayState {
        hover: hover.clone(),
        cursor: ask.cursor.and_then(cursor_at),
        pointer: ask.hover,
        brush,
        emphasis: emphasis.clone(),
    };
    let over = plot::overlay(model, &layout, view, &st, theme, &mut m);
    if say {
        let p = layout.plot;
        println!(
            "plot {:.1},{:.1} {:.1}x{:.1}; {} series; hover {:?}; emphasis {:?}; overlay {:?}",
            p.x,
            p.y,
            p.w,
            p.h,
            model.keys().len(),
            hover,
            emphasis.map(|k| k.name),
            over.bounds()
        );
    }
    scene.items.extend(over.items);
    Ok(scene)
}
