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
//! A graph (fig's graph JSON, a graph spec, or a CSV with an id and a parent column) takes these
//! too: `--cursor-node ID` (the cursor on a node, its card shown), `--hover-node ID` (the
//! pointer on a node after its dwell: it's in focus), `--hover-edge A,B`, `--reference ID` (with
//! `--cursor-node` or `--hover-node`, the path between), `--pin ID@X,Y` (a node dragged to a
//! place in the area and pinned; again for more), `--zoom F` (about the cursor's node, else the
//! middle), `--layout force|tree|layered|auto`, `--labels off` and `--ghost` (with one `--pin`,
//! the drag still under way: its ghost where it was). `--hover X,Y` finds what's there.
//! `--search Q` shows the search as it's typed: the nodes matching Q lit, the best in focus
//! with the cursor on it. `--detail` opens the panel for the cursor's node (else the hovered
//! one) beside the graph, as the surface lays it out, with the detail flags below.
//!
//! The file is built by the job `\v` runs (`source::build`), so a file that can't be drawn
//! draws the error figure: symbolic, or with `--log` its log (`--log-top` its first line shown,
//! `--log-mark` the entry lit). A file that draws, with `--log`, shows its warnings' log. With
//! `--stale BAD`, the file's figure is drawn with BAD's stale badge, as a figure kept while its
//! source fails to parse.
//!
//! A stat, meter or sparkline is drawn as a tile in its panel's frame, at `--size` (m4's 197 ×
//! its natural height unless given) with a margin round it: `--also FILE` puts more tiles in a
//! row after it (as m4's four), `--range D` takes the last D (`6h`) of the data as the
//! dashboard's visible time, `--trend N` hovers the first tile's trend at its nth point (its
//! card shown) and `--linked N` draws only the mini crosshair there, as a dashboard's linked
//! time does; `--hover X,Y` finds what's there; `--no-frame` leaves the frame out. Room for a
//! card is left under the row when something's hovered.
//!
//! A heatmap takes `--cursor-cell R,C` (the keyboard's cursor on a cell) and `--hover X,Y`.
//! `--brushed` releases the `--brush`: the figure is drawn zoomed to it, without the brush.
//!
//! `--detail` opens the detail panel for what the cursor is on (else the hover), beside the
//! figure as the surface lays it out (`detail::split`): the chart in what's left, the panel at
//! the right, or the bottom of an area under 560 px wide. `--detail-series` shows the cursor's
//! series instead. `--detail-row` puts the panel's cursor on a row (its index in the tree, as
//! printed), `--detail-top` scrolls it, `--detail-focus` gives it focus and `--detail-unfold`
//! opens every branch. It prints the tree.
//!
//! A dashboard (a `.fig` whose `type` is `dashboard`) reads its data files (commands' panels
//! wait) and draws with these: `--focus N` (panel N focused), `--full N` (panel N full size),
//! `--range 6h|…|all` (a chip's range), `--time HH:MM,HH:MM` (a custom time on the newest
//! sample's day), `--scroll PX`, `--live live|updating|paused|error` (the header's status),
//! `--hover-at N,HH:MM` (the pointer over panel N's plot at that time, halfway up),
//! `--panel-cursor N,I` (panel N's keyboard cursor on its first series' sample I), `--shared`
//! (`"tooltip": "shared"`) and `--panel-log N` (panel N's error figure as its log). `--hover
//! X,Y` is the pointer at a place in the area. It prints where each panel landed.

use die_test_cap as _;

use die_draw::Book;
use die_fig::FigError;
use die_fig::data::ReadOptions;
use die_fig::detail::{self, Item, PanelState};
use die_fig::graph::{self, GraphLayout, GraphModel, Target};
use die_fig::interact::{BrushKind, Cursor, Toggles, View};
use die_fig::plot::{self, ChartLayout, ChartModel, ErrorView, OverlayState};
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
    detail: Option<DetailAsk>,
    search: Option<String>,
    /// The whole area's width, where the figure is drawn beside the detail panel: its paddings.
    area_w: Option<f32>,
    /// Whether `--size` was given.
    sized: bool,
    /// More tiles in the row.
    also: Vec<String>,
    /// The dashboard's visible time: the data's last so long.
    range: Option<f64>,
    /// The first tile's trend hovered at a point, with its card or (linked) without.
    trend: Option<(usize, bool)>,
    /// Tiles without their frame.
    no_frame: bool,
    /// A heatmap's cursor on a cell.
    cursor_cell: Option<(u32, u32)>,
    /// The brush released: its zoom drawn, not the brush.
    brushed: bool,
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

/// Halfway along a polyline by its length: where a pointer on an edge is.
fn halfway(pts: &[(f32, f32)]) -> (f32, f32) {
    let seg = |w: &[(f32, f32)]| (w[1].0 - w[0].0).hypot(w[1].1 - w[0].1);
    let mut left = pts.windows(2).map(seg).sum::<f32>() / 2.0;
    for w in pts.windows(2) {
        let d = seg(w);
        if d >= left && d > 0.0 {
            let t = left / d;
            return (
                w[0].0 + (w[1].0 - w[0].0) * t,
                w[0].1 + (w[1].1 - w[0].1) * t,
            );
        }
        left -= d;
    }
    pts[pts.len() / 2]
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
    let (dash, rest) = dash_flags(std::env::args().skip(1))?;
    let mut args = rest.into_iter();
    let usage = "fig_shot <file> <out.png> [--size WxH] [--scale S] [--light] [--cursor N] \
                 [--hover X,Y] [--brush X0,X1[,Y0,Y1]] [--reference N|ID] [--svg OUT.svg] [--log] \
                 [--log-top N] [--log-mark N] [--stale BAD] [--cursor-node ID] [--hover-node ID] \
                 [--hover-edge A,B] [--pin ID@X,Y] [--zoom F] [--layout L] [--labels off] [--ghost] \
                 [--detail] [--detail-series] [--detail-row N] [--detail-top N] [--detail-focus] \
                 [--detail-unfold] [--search Q] [--also FILE] [--range D] [--trend N] \
                 [--linked N] [--no-frame] [--cursor-cell R,C] [--brushed]";
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
        detail: None,
        search: None,
        area_w: None,
        sized: false,
        also: vec![],
        range: None,
        trend: None,
        no_frame: false,
        cursor_cell: None,
        brushed: false,
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
                ask.sized = true;
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
            "--search" => ask.search = Some(value()?),
            "--also" => ask.also.push(value()?),
            "--range" => {
                let v = value()?;
                let d = die_fig::spec::chart::parse_duration(&v)
                    .ok_or(format!("--range {v}: a time like 6h"))?;
                ask.range = Some(d.as_secs_f64());
            }
            "--trend" => ask.trend = Some((pair(&value()?, ',')?[0] as usize, true)),
            "--linked" => ask.trend = Some((pair(&value()?, ',')?[0] as usize, false)),
            "--no-frame" => ask.no_frame = true,
            "--brushed" => ask.brushed = true,
            "--cursor-cell" => {
                let v = pair(&value()?, ',')?;
                if v.len() != 2 {
                    return Err("--cursor-cell is R,C".into());
                }
                ask.cursor_cell = Some((v[0] as u32, v[1] as u32));
            }
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
    if let Some(spec) = dash_of(path, &zone) {
        return dash_shot(spec, path, &output, &ask, &dash, &theme, &mut book);
    }
    if let Some(g) = graph_of(path, &zone) {
        return graph_shot(g, &name, &output, &ask, &theme, &mut book);
    }
    if let Some(t) = tile_of(path, &zone)? {
        return tile_shot(t, &output, &ask, &zone, &theme, &mut book);
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
    if ask.brushed
        && let Some(b) = ask.brush.take()
    {
        let mut m = BookMeasure {
            book: &mut book,
            scale: ask.scale,
        };
        let (_, layout, _) = plot::base(&model, &view, ask.size, ask.scale, &theme, &mut m);
        let (kind, from, to) = brush_of(&model, &layout, &b);
        if let Some(v) = plot::brush(&model, &layout, &view, kind, from, to) {
            println!("brushed to x {:?}, y {:?}", v.x, v.y);
            view = v;
        }
    }
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
    // Beside the detail panel, the graph lays out in what it leaves.
    let split = ask.detail.map(|_| detail::split(ask.size));
    let fsize = split.map_or(ask.size, |(f, _, _)| (f.w, f.h));
    let lay = |view: &View, scale: f32, book: &mut Book| -> (Scene, GraphLayout) {
        let mut m = BookMeasure { book, scale };
        graph::base_beside(&model, view, fsize, ask.size.0, scale, theme, &mut m)
    };
    let mut ghost = None;
    for (id, x, y) in &ask.pins {
        let i = node(id)?;
        let (_, l) = lay(&view, ask.scale, book);
        ghost = Some((id.as_str().into(), model.position(&view, i)));
        view = graph::nav::drag_node(&model, &l, &view, id, (*x, *y), (0.0, 0.0));
    }
    let found = ask.search.as_deref().map(|q| {
        let s = die_fig::search::Search::with(model.search_names(), q);
        let lit: Vec<usize> = (s.matches().iter())
            .filter_map(|m| match &m.item {
                Item::Node { id, .. } => model.index_of(id),
                _ => None,
            })
            .collect();
        let best = s.best().and_then(|m| m.item.cursor());
        println!(
            "search {q:?}: {} matches, best {:?}",
            s.matches().len(),
            best
        );
        (lit, best)
    });
    let cursor = match (&ask.cursor_node, &found) {
        (Some(id), _) => Some(model.cursor(node(id)?)),
        (None, Some((_, best))) => best.clone(),
        _ => None,
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
            cursor_card: split.is_none(),
            ghost: if ask.ghost { ghost.clone() } else { None },
            ..Default::default()
        };
        if let Some((lit, best)) = &found {
            st.matches = lit.clone();
            st.emphasis = best.clone();
        }
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
            st.pointer = Some(halfway(&e.pts));
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
        let mut scene = scene;
        if let (Some((_, rect, _)), Some(d)) = (split, ask.detail) {
            // The panel for the cursor's node, else the hovered one, beside the graph.
            let at = cursor
                .as_ref()
                .and_then(|c| model.node_of(c))
                .or(st.hover.and_then(Target::node))
                .ok_or("--detail needs --cursor-node or --hover-node")?;
            let mut tree = model.node_detail(at);
            if d.unfold {
                tree.unfold_all();
            }
            let mut state = PanelState::open(&tree, Some(&model.item(at)));
            if let Some(r) = d.row {
                state.row = r.min(tree.rows.len().saturating_sub(1));
            }
            state.top = d.top;
            state.focused = d.focus;
            detail::reveal(&tree, &mut state, rect);
            let mut m = BookMeasure { book, scale };
            let panel = detail::panel(&tree, state, rect, scale, theme, &mut m);
            if say {
                println!(
                    "detail {} at {rect:?}: {} — {}",
                    model.id(at),
                    tree.title,
                    tree.kind
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
            (scene.width, scene.height) = ask.size;
            scene.items.extend(panel.items);
            scene.regions.extend(panel.regions);
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
        area_w: Some(ask.size.0),
        ..ask.clone()
    };
    let chart = draw(model, view, &small, theme, scale, book, cursor_at, say)?;
    let target = ask.cursor.and_then(cursor_at).or_else(|| {
        let mut m = BookMeasure { book, scale };
        let (_, layout, hits) =
            plot::base_beside(model, view, small.size, ask.size.0, scale, theme, &mut m);
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
        auto: None,
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
    let area_w = ask.area_w.unwrap_or(ask.size.0);
    let (mut scene, layout, hits) =
        plot::base_beside(model, view, ask.size, area_w, scale, theme, &mut m);
    let hover = ask
        .hover
        .and_then(|(x, y)| plot::hover(model, &layout, &hits, view, x, y));
    let emphasis = ask
        .hover
        .and_then(|(x, y)| plot::nearest_series(model, &layout, &hits, view, x, y));
    let brush = ask.brush.as_ref().map(|b| brush_of(model, &layout, b));
    let st = OverlayState {
        hover: hover.clone(),
        cursor: ask
            .cursor_cell
            .map(|(row, col)| Cursor::Cell { row, col })
            .or_else(|| ask.cursor.and_then(cursor_at)),
        pointer: ask.hover,
        brush,
        emphasis: emphasis.clone(),
    };
    // Beside the detail panel, the cursor's card goes, as the surface has it.
    let lit = plot::overlay::Lit {
        cursor_card: ask.detail.is_none(),
        ..plot::overlay::Lit::default()
    };
    let over = plot::overlay::overlay_lit(model, &layout, view, &st, &lit, theme, &mut m);
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

/// The brush `--brush` asks for: its kind (a box when it gives y too) and its corners.
fn brush_of(
    model: &ChartModel,
    layout: &ChartLayout,
    b: &[f32],
) -> (BrushKind, (f32, f32), (f32, f32)) {
    let p = layout.plot;
    let kind = plot::brush_kind(model);
    let (y0, y1) = match b {
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
}

/// A tile: its figure and table, read as `\v` reads a spec (its data from its directory).
type Tile = (Figure, die_fig::data::Table);

/// The tile a file is, with its data read; None when it's another figure or doesn't parse.
fn tile_of(path: &Path, zone: &TimeZone) -> Result<Option<Tile>, String> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Ok(None);
    };
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    let dir = path.parent().unwrap_or(Path::new("."));
    let opt = ReadOptions::new(zone.clone());
    let Ok(parsed) = die_fig::spec::parse(&text, name, dir, &opt) else {
        return Ok(None);
    };
    let data = match &parsed.figure {
        Figure::Stat(s) => &s.data,
        Figure::Meter(m) => &m.data,
        Figure::Sparkline(s) => &s.data,
        _ => return Ok(None),
    };
    let table = match &data.source {
        die_fig::spec::DataRef::Inline(t) => (**t).clone(),
        die_fig::spec::DataRef::File(p) => {
            let text = source::read_file(p).map_err(|e| format!("{}: {e}", p.display()))?;
            let n = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
            source::read_data(&text, data, n, &opt).map_err(|e| format!("{}: {e}", p.display()))?
        }
        other => {
            return Err(format!(
                "fig_shot reads a tile's data from a file: {other:?}"
            ));
        }
    };
    Ok(Some((parsed.figure, table)))
}

/// The newest time in a table: its time columns' latest.
fn newest(t: &die_fig::data::Table) -> Option<f64> {
    t.columns
        .iter()
        .filter(|c| c.kind() == die_fig::data::ColumnKind::Time)
        .flat_map(|c| (0..c.len()).map(move |r| c.f64_at(r)))
        .filter(|v| v.is_finite())
        .fold(None, |a: Option<f64>, v| Some(a.map_or(v, |a| a.max(v))))
}

/// A row of tiles drawn as the flags ask: see the docs at the top.
fn tile_shot(
    first: Tile,
    output: &str,
    ask: &Ask,
    zone: &TimeZone,
    theme: &Theme,
    book: &mut Book,
) -> Result<(), String> {
    use die_fig::figure::{self, TileHover, TileModel};
    use die_fig::scene::{Item, Paint, PathData, Rect, Stroke};
    let mut tiles = vec![first];
    for f in &ask.also {
        let t = tile_of(Path::new(f), zone)?.ok_or(format!("{f} isn't a tile"))?;
        tiles.push(t);
    }
    let models: Vec<TileModel> = tiles
        .iter()
        .map(|(f, t)| {
            let range = ask.range.and_then(|d| newest(t).map(|n| (n - d, n)));
            TileModel::build_in(f, t, range, zone).map_err(|e| e.to_string())
        })
        .collect::<Result<_, _>>()?;
    let (w, h) = if ask.sized {
        ask.size
    } else {
        (197.0, models[0].natural_height())
    };
    let margin = 16.0;
    let gap = 12.0;
    let n = models.len() as f32;
    let hovering = ask.hover.is_some() || ask.trend.is_some_and(|t| t.1);
    let room = if hovering { 210.0 } else { 0.0 };
    let size = (
        2.0 * margin + n * w + (n - 1.0) * gap,
        2.0 * margin + h + room,
    );
    let rect = |i: usize| Rect::new(margin + i as f32 * (w + gap), margin, w, h);
    let area = Rect::new(0.0, 0.0, size.0, size.1);
    let draw = |scale: f32, book: &mut Book, say: bool| -> (Scene, Scene) {
        let mut m = BookMeasure { book, scale };
        let mut scene = Scene::new(size.0, size.1, scale);
        let mut over = Scene::new(size.0, size.1, scale);
        for (i, model) in models.iter().enumerate() {
            let r = rect(i);
            if !ask.no_frame {
                scene.push(Item::Path {
                    path: PathData::rounded(r, [8.0; 4]),
                    fill: Some(Paint::solid(theme.surface, 1.0)),
                    stroke: Some(Stroke::hairline(theme.grid, 1.0)),
                });
            }
            let b = figure::base(model, r, theme, &mut m);
            scene.items.extend(b.items);
            let hover = match (ask.hover, ask.trend) {
                (Some(at), _) => figure::draw::hit_with(model, r, at, &mut m),
                (None, Some((point, card))) if i == 0 => Some(TileHover::Trend { point, card }),
                _ => None,
            };
            let o = figure::overlay_in(model, hover, r, area, theme, &mut m);
            over.items.extend(o.items);
            if say {
                println!(
                    "tile {i} {:?} {:?}: {} {:?} over {} ({} rows); change {:?}; trend {} points; hover {:?}",
                    model.kind,
                    model.label,
                    model.text(),
                    model.unit,
                    model.range_text(),
                    model.rows,
                    model.change_text(),
                    model.trend.len(),
                    hover,
                );
            }
        }
        (scene, over)
    };
    let (base, over) = draw(ask.scale, book, true);
    let bytes = composite(&base, &over, theme, book)?;
    std::fs::write(output, bytes).map_err(|e| format!("{output}: {e}"))?;
    if let Some(svg) = &ask.svg {
        let (mut scene, over) = draw(1.0, book, false);
        scene.items.extend(over.items);
        std::fs::write(svg, die_fig::svg::write(&scene)).map_err(|e| format!("{svg}: {e}"))?;
    }
    Ok(())
}

// ------------------------------------------------------------------ dashboards

/// What the dashboard flags ask for.
#[derive(Clone, Default)]
struct DashAsk {
    focus: Option<usize>,
    full: Option<usize>,
    range: Option<String>,
    time: Option<(String, String)>,
    scroll: f32,
    live: Option<String>,
    hover_at: Option<(usize, String)>,
    cursor: Option<(usize, usize)>,
    shared: bool,
    log: Option<usize>,
}

/// The dashboard flags taken out of the arguments, the rest left for the figure's.
fn dash_flags(args: impl Iterator<Item = String>) -> Result<(DashAsk, Vec<String>), String> {
    let mut d = DashAsk::default();
    let mut rest = Vec::new();
    let mut args = args.peekable();
    let num = |v: &str| -> Result<usize, String> {
        v.trim().parse().map_err(|_| format!("{v}: not a number"))
    };
    while let Some(flag) = args.next() {
        let mut value = || args.next().ok_or(format!("{flag} needs a value"));
        match flag.as_str() {
            "--focus" => d.focus = Some(num(&value()?)?),
            "--full" => d.full = Some(num(&value()?)?),
            "--range" => d.range = Some(value()?),
            "--time" => {
                let v = value()?;
                let (a, b) = v.split_once(',').ok_or("--time is HH:MM,HH:MM")?;
                d.time = Some((a.to_string(), b.to_string()));
            }
            "--scroll" => d.scroll = pair(&value()?, ',')?[0],
            "--live" => d.live = Some(value()?),
            "--hover-at" => {
                let v = value()?;
                let (n, t) = v.split_once(',').ok_or("--hover-at is N,HH:MM")?;
                d.hover_at = Some((num(n)?, t.to_string()));
            }
            "--panel-cursor" => {
                let v = value()?;
                let (n, i) = v.split_once(',').ok_or("--panel-cursor is N,I")?;
                d.cursor = Some((num(n)?, num(i)?));
            }
            "--shared" => d.shared = true,
            "--panel-log" => d.log = Some(num(&value()?)?),
            _ => rest.push(flag),
        }
    }
    Ok((d, rest))
}

/// The dashboard a file is, if it parses as one.
fn dash_of(path: &Path, zone: &TimeZone) -> Option<die_fig::spec::DashboardSpec> {
    let text = std::fs::read_to_string(path).ok()?;
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    let dir = path.parent().unwrap_or(Path::new("."));
    let parsed = die_fig::spec::parse(&text, name, dir, &ReadOptions::new(zone.clone())).ok()?;
    for w in &parsed.warnings {
        eprintln!("fig_shot: warning: {w}");
    }
    match parsed.figure {
        Figure::Dashboard(d) => Some(d),
        _ => None,
    }
}

/// A time of day on the day of `newest` in `zone`, as seconds.
fn time_of_day(hm: &str, newest: f64, zone: &TimeZone) -> Result<f64, String> {
    let (h, m) = hm.split_once(':').ok_or(format!("{hm}: not HH:MM"))?;
    let (h, m): (i8, i8) = (
        h.parse().map_err(|_| format!("{hm}: not HH:MM"))?,
        m.parse().map_err(|_| format!("{hm}: not HH:MM"))?,
    );
    let ts = jiff::Timestamp::from_second(newest as i64).map_err(|e| e.to_string())?;
    let day = ts.to_zoned(zone.clone()).date();
    let at = day
        .at(h, m, 0, 0)
        .to_zoned(zone.clone())
        .map_err(|e| e.to_string())?;
    Ok(at.timestamp().as_second() as f64)
}

/// A dashboard drawn as the flags ask: see the docs at the top.
#[allow(clippy::too_many_arguments)]
fn dash_shot(
    spec: die_fig::spec::DashboardSpec,
    path: &Path,
    output: &str,
    ask: &Ask,
    d: &DashAsk,
    theme: &Theme,
    book: &mut Book,
) -> Result<(), String> {
    use die_fig::dashboard::{self, DashModel, DashView, LiveStatus, Origin, PanelView};
    let mut spec = spec;
    if d.shared {
        spec.tooltip = die_fig::spec::Tooltip::Shared;
    }
    let zone = TimeZone::system();
    let opt = ReadOptions::new(zone.clone());
    let file = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    let dir = file.parent().map(Path::to_path_buf).unwrap_or_default();
    let resolve = |p: &Path| dashboard::open(p, &opt);
    let origin = Origin {
        file: Some(&file),
        dir: &dir,
        zone: &zone,
    };
    let first = DashModel::build(&spec, &dashboard::Sources::new(), &resolve, &origin);
    let mut sources = dashboard::read(first.wants(), &opt);
    sources.live = match d.live.as_deref() {
        None => None,
        Some("live") => Some(LiveStatus::updated(12.0)),
        Some("updating") => Some(LiveStatus::updating()),
        Some("paused") => Some(LiveStatus::paused()),
        Some("error") => Some(LiveStatus::error(130.0)),
        Some(other) => return Err(format!("--live {other}: live, updating, paused or error")),
    };
    let model = DashModel::build(&spec, &sources, &resolve, &origin);
    let mut view = DashView::initial(&model);
    view.focused = d.focus;
    view.full = d.full;
    view.scroll = d.scroll;
    if let Some(r) = &d.range {
        let range = dashboard::RANGES
            .iter()
            .find(|x| x.key == r)
            .ok_or(format!("--range {r}: 15m, 1h, 6h, 24h, 7d, 30d or all"))?;
        view = view.with_range(&model, range.secs);
    }
    let newest = model.newest();
    if let Some((a, b)) = &d.time {
        let n = newest.ok_or("--time needs linked panels")?;
        view.time = Some((time_of_day(a, n, &zone)?, time_of_day(b, n, &zone)?));
    }
    if let Some(i) = d.log {
        let p = view.panels.get_mut(i).ok_or(format!("no panel {i}"))?;
        p.error = ErrorView::Log { top: 0, mark: 0 };
    }
    if let Some((n, i)) = d.cursor {
        let cm = model
            .panels
            .get(n)
            .and_then(|p| p.chart())
            .ok_or(format!("panel {n} isn't a chart"))?;
        let key = cm.keys().into_iter().next().ok_or("no series")?;
        let p: &mut PanelView = view.panels.get_mut(n).ok_or(format!("no panel {n}"))?;
        p.cursor = Some(Cursor::Point {
            series: key,
            row: i as u32,
        });
    }
    let shot = |scale: f32, book: &mut Book, say: bool| -> Result<(Scene, Scene), String> {
        let mut m = BookMeasure { book, scale };
        let layout = dashboard::layout(&model, &view, ask.size, scale, theme, &mut m);
        let pointer = match &d.hover_at {
            Some((n, hm)) => {
                let p = layout
                    .panel(*n)
                    .ok_or(format!("panel {n} isn't laid out"))?;
                let (l, _) = p.chart().ok_or(format!("panel {n} isn't a chart"))?;
                let t = time_of_day(hm, newest.ok_or("no time")?, &zone)?;
                let (ox, oy) = p.origin();
                Some((l.x.map(t) + ox, l.plot.y + l.plot.h * 0.5 + oy))
            }
            None => ask.hover,
        };
        let hover = pointer.and_then(|(x, y)| dashboard::hover(&model, &layout, x, y));
        let base = dashboard::base(&layout, &view, theme, &mut m);
        let over = dashboard::overlay(&model, &layout, &view, hover.as_ref(), theme, &mut m);
        if say {
            println!(
                "dashboard {:?}: {} panels; header to {:.0}; chip {:?}; live {:?}; scroll {:.0} of {:.0}; time {:?}",
                model.title,
                model.panels.len(),
                layout.header.bottom,
                layout
                    .header
                    .chip
                    .as_ref()
                    .map(|c| (c.text.text.clone(), c.rect)),
                layout
                    .header
                    .live
                    .as_ref()
                    .map(|l| l.text.as_ref().map(|t| t.text.clone())),
                layout.scroll,
                layout.max_scroll,
                view.chip(&model),
            );
            for p in layout.panels.iter().flatten() {
                let pm = &model.panels[p.index];
                let what = match &p.figure {
                    dashboard::layout::PanelFigure::Chart { layout: l, .. } => format!(
                        "chart, plot {:.0},{:.0} {:.0}x{:.0}",
                        l.plot.x + p.frame.x,
                        l.plot.y + p.frame.y,
                        l.plot.w,
                        l.plot.h
                    ),
                    dashboard::layout::PanelFigure::Graph { .. } => "graph".into(),
                    dashboard::layout::PanelFigure::Tile { model: t } => format!("tile {t:?}"),
                    dashboard::layout::PanelFigure::Waiting(w) => format!("waiting: {w}"),
                    dashboard::layout::PanelFigure::Failed => match &pm.state {
                        dashboard::PanelState::Failed(f) => format!(
                            "failed: {}",
                            f.errors.first().map(|e| e.to_string()).unwrap_or_default()
                        ),
                        _ => "failed".into(),
                    },
                };
                println!(
                    "  panel {} {:?}{} frame {:.1},{:.1} {:.1}x{:.1}: {what}",
                    p.index,
                    pm.title,
                    if pm.linked { " linked" } else { "" },
                    p.frame.x,
                    p.frame.y,
                    p.frame.w,
                    p.frame.h
                );
            }
            for t in model.trust() {
                println!("  asks: {}", t.question("this dashboard"));
            }
            println!(
                "  hover {:?}; overlay {:?}",
                hover.as_ref().map(|h| (h.panel, h.time, &h.target)),
                over.bounds()
            );
        }
        Ok((base, over))
    };
    let (base, over) = shot(ask.scale, book, true)?;
    let bytes = composite(&base, &over, theme, book)?;
    std::fs::write(output, bytes).map_err(|e| format!("{output}: {e}"))?;
    if let Some(svg) = &ask.svg {
        let (mut scene, over) = shot(1.0, book, false)?;
        scene.items.extend(over.items);
        std::fs::write(svg, die_fig::svg::write(&scene)).map_err(|e| format!("{svg}: {e}"))?;
    }
    Ok(())
}
