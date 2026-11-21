//! fig_shot: a data or spec file drawn as `\v` would draw it, base and overlay, written as a PNG
//! with die-draw's fonts. For judging the look without DIE:
//!
//! ```sh
//! cargo run --release -p die-fig --example fig_shot -- <file> <out.png> [--size 880x470] \
//!     [--scale 2] [--light] [--cursor <n>] [--hover <x>,<y>] [--brush <x0>,<x1>[,<y0>,<y1>]] \
//!     [--reference <n>] [--svg <out.svg>] [--log] [--log-top <n>] [--log-mark <n>] \
//!     [--stale <bad file>] [--detail] [--detail-series] [--detail-row <n>] [--detail-top <n>] \
//!     [--detail-focus] [--detail-unfold]
//! ```
//!
//! The size is the area's in logical px, the scale the display's. `--cursor` puts the keyboard's
//! cursor on the nth sample of the first series (the nth band on a band chart), `--reference`
//! the reference there. `--hover` is the pointer at a place in the area: the overlay shows what
//! it's on, with the series nearest it brought forward. `--brush` draws the brush a left drag
//! from x0 to x1 makes (with y0 and y1 for a box). `--svg` writes the 1× layout as SVG too.
//! Times without an offset are in the system's zone. It prints where the plot landed and what
//! the overlay shows.
//!
//! The file is built by the job `\v` runs (`source::build`), so a file that can't be drawn
//! draws the error figure: symbolic, or with `--log` its log (`--log-top` its first line shown,
//! `--log-mark` the entry lit). A file that draws, with `--log`, shows its warnings' log. With
//! `--stale BAD`, the file's figure is drawn with BAD's stale badge, as a figure kept while its
//! source fails to parse.
//!
//! `--detail` opens the detail panel for what the cursor is on (else the hover), beside the
//! figure as the surface lays it out (`detail::split`): the chart in what's left, the panel at
//! the right, or the bottom of an area under 560 px wide. `--detail-series` shows the cursor's
//! series instead. `--detail-row` puts the panel's cursor on a row (its index in the tree, as
//! printed), `--detail-top` scrolls it, `--detail-focus` gives it focus and `--detail-unfold`
//! opens every branch. It prints the tree.

use die_test_cap as _;

use die_draw::Book;
use die_fig::FigError;
use die_fig::detail::{self, Item, PanelState};
use die_fig::interact::{BrushKind, Cursor, Toggles, View};
use die_fig::plot::{self, ChartModel, ErrorView, OverlayState};
use die_fig::raster::{self, BookMeasure};
use die_fig::scene::Scene;
use die_fig::source::{self, Built, Failed, Job, Text};
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
#[derive(Clone)]
struct Ask {
    size: (f32, f32),
    scale: f32,
    light: bool,
    cursor: Option<usize>,
    reference: Option<usize>,
    hover: Option<(f32, f32)>,
    brush: Option<Vec<f32>>,
    svg: Option<String>,
    log: bool,
    log_top: usize,
    log_mark: usize,
    stale: Option<String>,
    detail: Option<DetailAsk>,
}

/// What the detail flags ask for.
#[derive(Clone, Copy, Default)]
struct DetailAsk {
    series: bool,
    row: Option<usize>,
    top: usize,
    focus: bool,
    unfold: bool,
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
                 [--hover X,Y] [--brush X0,X1[,Y0,Y1]] [--reference N] [--svg OUT.svg] [--log] \
                 [--log-top N] [--log-mark N] [--stale BAD] [--detail] [--detail-series] \
                 [--detail-row N] [--detail-top N] [--detail-focus] [--detail-unfold]";
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
        log: false,
        log_top: 0,
        log_mark: 0,
        stale: None,
        detail: None,
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
            "--log" => ask.log = true,
            "--log-top" => ask.log_top = pair(&value()?, ',')?[0] as usize,
            "--log-mark" => ask.log_mark = pair(&value()?, ',')?[0] as usize,
            "--stale" => ask.stale = Some(value()?),
            "--detail" => {
                ask.detail.get_or_insert_with(DetailAsk::default);
            }
            "--detail-series" => ask.detail.get_or_insert_with(DetailAsk::default).series = true,
            "--detail-row" => {
                let n = pair(&value()?, ',')?[0] as usize;
                ask.detail.get_or_insert_with(DetailAsk::default).row = Some(n);
            }
            "--detail-top" => {
                let n = pair(&value()?, ',')?[0] as usize;
                ask.detail.get_or_insert_with(DetailAsk::default).top = n;
            }
            "--detail-focus" => ask.detail.get_or_insert_with(DetailAsk::default).focus = true,
            "--detail-unfold" => ask.detail.get_or_insert_with(DetailAsk::default).unfold = true,
            other => return Err(format!("{other}: no such flag\n{usage}")),
        }
    }
    let path = Path::new(&input);
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_string();
    let zone = TimeZone::system();
    let theme = if ask.light {
        Theme::light()
    } else {
        Theme::mono_dark()
    };
    let mut book = Book::load(None)?;
    let state = match ask.log {
        true => ErrorView::Log {
            top: ask.log_top,
            mark: ask.log_mark,
        },
        false => ErrorView::Symbolic,
    };
    let built = match build(path, &zone) {
        Ok(b) => b,
        Err(f) => {
            for e in f.all() {
                eprintln!(
                    "fig_shot: {}: {e}",
                    if e.warning { "warning" } else { e.kind.name() }
                );
            }
            let errors = f.all();
            let scene = failure(&errors, &f.text, &name, state, &ask, &theme, &mut book);
            return write(&scene, &output, &ask, &theme, &mut book, |scale, book| {
                failure(
                    &errors,
                    &f.text,
                    &name,
                    state,
                    &Ask {
                        scale,
                        ..ask.clone()
                    },
                    &theme,
                    book,
                )
            });
        }
    };
    for w in &built.warnings {
        eprintln!("fig_shot: warning: {w}");
    }
    if ask.log {
        let warnings: Vec<FigError> = built.warnings.iter().map(|w| w.clone().warn()).collect();
        let scene = failure(
            &warnings,
            &built.text,
            &name,
            state,
            &ask,
            &theme,
            &mut book,
        );
        return write(&scene, &output, &ask, &theme, &mut book, |scale, book| {
            failure(
                &warnings,
                &built.text,
                &name,
                state,
                &Ask {
                    scale,
                    ..ask.clone()
                },
                &theme,
                book,
            )
        });
    }
    let stale: Option<FigError> = match &ask.stale {
        Some(bad) => match build(Path::new(bad), &zone) {
            Ok(_) => return Err(format!("{bad} builds: it has no error for a badge")),
            Err(f) => Some(f.first().clone()),
        },
        None => None,
    };
    let Some(model) = built.model else {
        return Err("fig_shot draws charts; this is another kind of figure".into());
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
    let badge = |scene: &mut Scene, scale: f32, book: &mut Book| {
        if let Some(e) = &stale {
            let mut m = BookMeasure { book, scale };
            plot::stale_badge(scene, e, &theme, &mut m);
        }
    };
    let mut scene = beside(
        &model, &view, &ask, &theme, ask.scale, &mut book, &cursor_at, true,
    )?;
    badge(&mut scene, ask.scale, &mut book);
    write(&scene, &output, &ask, &theme, &mut book, |scale, book| {
        let mut s =
            beside(&model, &view, &ask, &theme, scale, book, &cursor_at, false).unwrap_or_default();
        badge(&mut s, scale, book);
        s
    })
}

/// The figure, and the detail panel beside it where asked: the chart laid out in what the
/// panel leaves, as the surface does.
#[allow(clippy::too_many_arguments)]
fn beside(
    model: &ChartModel,
    view: &View,
    ask: &Ask,
    theme: &Theme,
    scale: f32,
    book: &mut Book,
    cursor_at: &dyn Fn(usize) -> Option<Cursor>,
    say: bool,
) -> Result<Scene, String> {
    let Some(d) = ask.detail else {
        return draw(model, view, ask, theme, scale, book, cursor_at, say);
    };
    let (fig, rect, _) = detail::split(ask.size);
    let small = Ask {
        size: (fig.w, fig.h),
        ..ask.clone()
    };
    let chart = draw(model, view, &small, theme, scale, book, cursor_at, say)?;
    let target = ask.cursor.and_then(cursor_at).or_else(|| {
        let mut m = BookMeasure { book, scale };
        let (_, layout, hits) = plot::base(model, view, small.size, scale, theme, &mut m);
        ask.hover
            .and_then(|(x, y)| plot::hover(model, &layout, &hits, view, x, y))
    });
    let Some(target) = target else {
        return Err("--detail needs --cursor or --hover on a mark".into());
    };
    let mut tree = if d.series {
        let key = match &target {
            Cursor::Point { series, .. } => Some(series.clone()),
            Cursor::Band { series, .. } => series.clone().or_else(|| model.keys().first().cloned()),
            _ => None,
        };
        key.and_then(|k| model.series_detail(&k))
    } else {
        model.detail(view, &target)
    }
    .ok_or("the cursor names nothing with a detail")?;
    if d.unfold {
        tree.unfold_all();
    }
    let mut state = PanelState::open(&tree, Item::of_cursor(&target).as_ref());
    if let Some(r) = d.row {
        state.row = r.min(tree.rows.len().saturating_sub(1));
    }
    state.top = d.top;
    state.focused = d.focus;
    if d.row.is_some() || d.top == 0 {
        detail::reveal(&tree, &mut state, rect);
    }
    let mut m = BookMeasure { book, scale };
    let panel = detail::panel(&tree, state, rect, scale, theme, &mut m);
    if say {
        println!(
            "detail {:?} at {rect:?}: {} — {}",
            target, tree.title, tree.kind
        );
        for (i, r) in tree.rows.iter().enumerate() {
            println!(
                "  {i:>3} {}{:?} {} = {}{}",
                "  ".repeat(usize::from(r.depth)),
                r.kind,
                r.key,
                r.value,
                if i == state.row { "  <" } else { "" }
            );
        }
    }
    let mut scene = Scene::new(ask.size.0, ask.size.1, scale);
    scene.items = chart.items;
    scene.regions = chart.regions;
    scene.items.extend(panel.items);
    scene.regions.extend(panel.regions);
    Ok(scene)
}

/// A file built by the job `\v` runs, its data read from disk, times in `zone`.
fn build(path: &Path, zone: &TimeZone) -> Result<Built, Failed> {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    let dir = path.parent().unwrap_or(Path::new("."));
    let dir = std::path::absolute(dir).unwrap_or_else(|_| dir.to_path_buf());
    source::build(Job {
        text: Text::File(std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf())),
        name: name.to_string(),
        dir,
        zone: zone.clone(),
        open: vec![],
        toggles: Toggles::default(),
    })
}

/// The error figure for `errors` at the ask's size and scale.
fn failure(
    errors: &[FigError],
    text: &str,
    name: &str,
    state: ErrorView,
    ask: &Ask,
    theme: &Theme,
    book: &mut Book,
) -> Scene {
    let mut m = BookMeasure {
        book,
        scale: ask.scale,
    };
    let scene = plot::error_figure(
        errors, text, name, state, ask.size, ask.scale, theme, &mut m,
    );
    println!("error figure {:?}; {} entries", state, errors.len());
    scene
}

/// The scene as a PNG at `output`, and at 1× as SVG where asked (`again` draws it at a scale).
fn write(
    scene: &Scene,
    output: &str,
    ask: &Ask,
    theme: &Theme,
    book: &mut Book,
    again: impl FnOnce(f32, &mut Book) -> Scene,
) -> Result<(), String> {
    let bytes = raster::png(scene, theme.surface, Some(book))?;
    std::fs::write(output, bytes).map_err(|e| format!("{output}: {e}"))?;
    if let Some(svg) = &ask.svg {
        let scene = again(1.0, book);
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
