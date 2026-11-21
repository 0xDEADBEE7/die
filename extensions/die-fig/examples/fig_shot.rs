//! fig_shot: a data or spec file drawn as `\v` would draw it, base and overlay, written as a PNG
//! with die-draw's fonts. For judging the look without DIE:
//!
//! ```sh
//! cargo run --release -p die-fig --example fig_shot -- <file> <out.png> [--size 880x470] \
//!     [--scale 2] [--light] [--cursor <n>] [--hover <x>,<y>] [--brush <x0>,<x1>[,<y0>,<y1>]] \
//!     [--reference <n>] [--svg <out.svg>] [--log] [--log-top <n>] [--log-mark <n>] \
//!     [--stale <bad file>]
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
//! A graph (fig's graph JSON, a graph spec, or a CSV with an id and a parent column) takes these
//! too: `--cursor-node ID` (the cursor on a node, its card shown), `--hover-node ID` (the
//! pointer on a node after its dwell: it's in focus), `--hover-edge A,B`, `--reference ID` (with
//! `--cursor-node` or `--hover-node`, the path between), `--pin ID@X,Y` (a node dragged to a
//! place in the area and pinned; again for more), `--zoom F` (about the cursor's node, else the
//! middle), `--layout force|tree|layered|auto`, `--labels off` and `--ghost` (with one `--pin`,
//! the drag still under way: its ghost where it was). `--hover X,Y` finds what's there.
//!
//! The file is built by the job `\v` runs (`source::build`), so a file that can't be drawn
//! draws the error figure: symbolic, or with `--log` its log (`--log-top` its first line shown,
//! `--log-mark` the entry lit). A file that draws, with `--log`, shows its warnings' log. With
//! `--stale BAD`, the file's figure is drawn with BAD's stale badge, as a figure kept while its
//! source fails to parse.

use die_test_cap as _;

use die_draw::Book;
use die_fig::FigError;
use die_fig::data::ReadOptions;
use die_fig::graph::{self, GraphLayout, GraphModel, Target};
use die_fig::interact::{BrushKind, Cursor, Toggles, View};
use die_fig::plot::{self, ChartModel, ErrorView, OverlayState};
use die_fig::raster::{self, BookMeasure};
use die_fig::scene::Scene;
use die_fig::source::{self, Built, Failed, Job, Text};
use die_fig::spec::{Figure, GraphSpec, Labels, Layout};
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
    reference: Option<String>,
    cursor_node: Option<String>,
    hover_node: Option<String>,
    hover_edge: Option<(String, String)>,
    pins: Vec<(String, f32, f32)>,
    zoom: Option<f64>,
    layout: Option<Layout>,
    labels_off: bool,
    ghost: bool,
    hover: Option<(f32, f32)>,
    brush: Option<Vec<f32>>,
    svg: Option<String>,
    log: bool,
    log_top: usize,
    log_mark: usize,
    stale: Option<String>,
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
                 [--hover X,Y] [--brush X0,X1[,Y0,Y1]] [--reference N|ID] [--svg OUT.svg] [--log] \
                 [--log-top N] [--log-mark N] [--stale BAD] [--cursor-node ID] [--hover-node ID] \
                 [--hover-edge A,B] [--pin ID@X,Y] [--zoom F] [--layout L] [--labels off] [--ghost]";
    let (Some(input), Some(output)) = (args.next(), args.next()) else {
        return Err(usage.to_owned());
    };
    let mut ask = Ask {
        size: (880.0, 470.0),
        scale: 2.0,
        light: false,
        cursor: None,
        reference: None,
        cursor_node: None,
        hover_node: None,
        hover_edge: None,
        pins: vec![],
        zoom: None,
        layout: None,
        labels_off: false,
        ghost: false,
        hover: None,
        brush: None,
        svg: None,
        log: false,
        log_top: 0,
        log_mark: 0,
        stale: None,
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
            "--reference" => ask.reference = Some(value()?),
            "--cursor-node" => ask.cursor_node = Some(value()?),
            "--hover-node" => ask.hover_node = Some(value()?),
            "--hover-edge" => {
                let v = value()?;
                let (a, b) = v.split_once(',').ok_or("--hover-edge is A,B")?;
                ask.hover_edge = Some((a.to_string(), b.to_string()));
            }
            "--pin" => {
                let v = value()?;
                let (id, at) = v.rsplit_once('@').ok_or("--pin is ID@X,Y")?;
                let xy = pair(at, ',')?;
                if xy.len() != 2 {
                    return Err("--pin is ID@X,Y".into());
                }
                ask.pins.push((id.to_string(), xy[0], xy[1]));
            }
            "--zoom" => ask.zoom = Some(f64::from(pair(&value()?, ',')?[0])),
            "--layout" => {
                ask.layout = Some(match value()?.as_str() {
                    "force" => Layout::Force,
                    "tree" => Layout::Tree { root: None },
                    "layered" => Layout::Layered,
                    "auto" => Layout::Auto,
                    other => return Err(format!("--layout {other}: force, tree, layered or auto")),
                })
            }
            "--labels" => ask.labels_off = value()? == "off",
            "--ghost" => ask.ghost = true,
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
    if let Some(g) = graph_of(path, &zone) {
        return graph_shot(g, &name, &output, &ask, &theme, &mut book);
    }
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
    view.reference = ask
        .reference
        .as_deref()
        .and_then(|r| r.parse::<usize>().ok())
        .and_then(cursor_at);
    let badge = |scene: &mut Scene, scale: f32, book: &mut Book| {
        if let Some(e) = &stale {
            let mut m = BookMeasure { book, scale };
            plot::stale_badge(scene, e, &theme, &mut m);
        }
    };
    let mut scene = draw(
        &model, &view, &ask, &theme, ask.scale, &mut book, &cursor_at, true,
    )?;
    badge(&mut scene, ask.scale, &mut book);
    write(&scene, &output, &ask, &theme, &mut book, |scale, book| {
        let mut s =
            draw(&model, &view, &ask, &theme, scale, book, &cursor_at, false).unwrap_or_default();
        badge(&mut s, scale, book);
        s
    })
}

/// The graph a file is, read as `\v` reads it (`spec::parse` with its directory); None when
/// it's another figure or doesn't parse (the build then says why).
fn graph_of(path: &Path, zone: &TimeZone) -> Option<GraphSpec> {
    let text = std::fs::read_to_string(path).ok()?;
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    let dir = path.parent().unwrap_or(Path::new("."));
    let parsed = die_fig::spec::parse(&text, name, dir, &ReadOptions::new(zone.clone())).ok()?;
    match parsed.figure {
        Figure::Graph(g) => Some(g),
        _ => None,
    }
}

/// A graph drawn as the flags ask: see the docs at the top.
fn graph_shot(
    spec: GraphSpec,
    name: &str,
    output: &str,
    ask: &Ask,
    theme: &Theme,
    book: &mut Book,
) -> Result<(), String> {
    let mut spec = spec;
    if ask.labels_off {
        spec.view.labels = Labels::None;
    }
    let t0 = std::time::Instant::now();
    let model = GraphModel::build_as(spec, ask.layout.clone())
        .map_err(|e| e.to_string())?
        .named(name);
    let laid = t0.elapsed();
    let node =
        |id: &str| -> Result<usize, String> { model.index_of(id).ok_or(format!("no node {id}")) };
    let mut view = model.initial_view();
    let lay = |view: &View, scale: f32, book: &mut Book| -> (Scene, GraphLayout) {
        let mut m = BookMeasure { book, scale };
        graph::base(&model, view, ask.size, scale, theme, &mut m)
    };
    let mut ghost = None;
    for (id, x, y) in &ask.pins {
        let i = node(id)?;
        let (_, l) = lay(&view, ask.scale, book);
        ghost = Some((id.as_str().into(), model.position(&view, i)));
        view = graph::nav::drag_node(&model, &l, &view, id, (*x, *y), (0.0, 0.0));
    }
    let cursor = match &ask.cursor_node {
        Some(id) => Some(model.cursor(node(id)?)),
        None => None,
    };
    if let Some(f) = ask.zoom {
        let (_, l) = lay(&view, ask.scale, book);
        view = graph::nav::zoom_about(&model, &l, &view, f, None, cursor.as_ref());
    }
    if let Some(r) = &ask.reference {
        view.reference = Some(model.cursor(node(r)?));
    }
    let shot = |view: &View,
                scale: f32,
                book: &mut Book,
                say: bool|
     -> Result<(Scene, Scene), String> {
        let (scene, l) = lay(view, scale, book);
        let mut st = graph::OverlayState {
            cursor: cursor.clone(),
            cursor_card: true,
            ghost: if ask.ghost { ghost.clone() } else { None },
            ..Default::default()
        };
        if let Some(id) = &ask.hover_node {
            let i = node(id)?;
            let g = l.node(i).ok_or(format!("{id} isn't drawn"))?;
            st.pointer = Some((g.at.0 + g.r * 0.3, g.at.1 + g.r * 0.3));
            st.hover = Some(Target::Node(i as u32));
            st.emphasis = Some(model.cursor(i));
        }
        if let Some((a, b)) = &ask.hover_edge {
            let (a, b) = (node(a)? as u32, node(b)? as u32);
            let k = model
                .ends
                .iter()
                .position(|&e| e == (a, b) || e == (b, a))
                .ok_or("no such edge")?;
            let e = l.edges[k].as_ref().ok_or("that edge isn't drawn")?;
            st.pointer = Some(e.pts[e.pts.len() / 2]);
            st.hover = Some(Target::Edge(k as u32));
        }
        if let Some((x, y)) = ask.hover {
            st.pointer = Some((x, y));
            st.hover = graph::hover(&model, &l, x, y);
            if let Some(Target::Node(i)) = st.hover {
                st.emphasis = Some(model.cursor(i as usize));
            }
        }
        let mut m = BookMeasure { book, scale };
        let t1 = std::time::Instant::now();
        let over = graph::overlay(&model, &l, view, &st, theme, &mut m);
        let took = t1.elapsed();
        if say {
            let placed = l.labels.iter().flatten().count();
            let backed = l.labels.iter().flatten().filter(|x| x.backed).count();
            let shown = l.nodes.iter().flatten().count();
            println!(
                "graph {} nodes ({} shown), {} edges; layout {:?} in {:.1} ms; plot {:.1},{:.1} {:.1}x{:.1}; labels {placed} ({backed} backed), {} culled; hover {:?}; overlay {:?} in {:.2} ms",
                model.len(),
                shown,
                model.ends.len(),
                model.layout,
                laid.as_secs_f64() * 1e3,
                l.plot.x,
                l.plot.y,
                l.plot.w,
                l.plot.h,
                shown - placed,
                st.hover,
                over.bounds(),
                took.as_secs_f64() * 1e3,
            );
        }
        Ok((scene, over))
    };
    let (base, over) = shot(&view, ask.scale, book, true)?;
    let bytes = composite(&base, &over, theme, book)?;
    std::fs::write(output, bytes).map_err(|e| format!("{output}: {e}"))?;
    if let Some(svg) = &ask.svg {
        let (mut scene, over) = shot(&view, 1.0, book, false)?;
        scene.items.extend(over.items);
        std::fs::write(svg, die_fig::svg::write(&scene)).map_err(|e| format!("{svg}: {e}"))?;
    }
    Ok(())
}

/// The base and the overlay drawn into buffers of their own and laid one on the other as the
/// host lays the window's overlay on its base (premultiplied, exact), as a PNG: drawn as one
/// scene, the overlay's wash would round the surface under it a level off.
fn composite(
    base: &Scene,
    over: &Scene,
    theme: &Theme,
    book: &mut Book,
) -> Result<Vec<u8>, String> {
    let s = base.scale;
    let (w, h) = (
        (base.width * s).ceil() as u32,
        (base.height * s).ceil() as u32,
    );
    let len = w as usize * h as usize * 4;
    let (mut under, mut top) = (vec![0u8; len], vec![0u8; len]);
    raster::draw(
        base,
        &mut under,
        w,
        h,
        w as usize * 4,
        None,
        Some(theme.surface),
        Some(book),
    );
    raster::draw(over, &mut top, w, h, w as usize * 4, None, None, Some(book));
    for (u, t) in under.chunks_exact_mut(4).zip(top.chunks_exact(4)) {
        let keep = 1.0 - f32::from(t[3]) / 255.0;
        for c in 0..3 {
            u[c] = (f32::from(t[c]) + f32::from(u[c]) * keep)
                .round()
                .min(255.0) as u8;
        }
        u[3] = 255;
    }
    die_draw::image::encode(w, h, &under)
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
