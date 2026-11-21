//! die-fig: figures from data in a DIE buffer (charts, network graphs and dashboards) drawn in
//! pixels, in the ethos of fig, as an extension (scope D14). It depends on die-core and
//! die-draw alone, so it's platform-neutral and out of the product: `cargo xtask lint` holds it
//! to that. Its parts:
//!
//! - this file: [`Fig`], the extension, with your [`Options`], its commands, its keys, what a
//!   restart keeps, and the settle after each command;
//! - [`json`], [`error`] and [`style`]: the positional JSON reader every spec and JSON file goes
//!   through, its errors with places, and the data's own `fig` blocks;
//! - [`data`]: tables read from CSV, TSV, JSON and JSONL, and their transforms;
//! - [`spec`]: what to draw, from a fig file, a spec, or a guess at bare data;
//! - [`scale`]: data to pixels, ticks, and how values are written;
//! - [`scene`], [`theme`], [`raster`] and [`svg`]: the display list, its colours, and its pixels,
//!   PNG and SVG;
//! - [`algo`]: layouts, downsampling, envelopes, search and the hit index;
//! - [`plot`] and [`interact`]: a chart drawn, and the keyboard's and pointer's ways with it;
//! - [`detail`], [`graph`], [`figure`], [`dashboard`], [`source`], [`surface`] and [`cells`]:
//!   the detail panel, the network figure, tiles, dashboards, where data comes from, the DIE
//!   surface, and the braille fallback.
//!
//! Nothing of fig's runs until `fig-toggle` asks for a figure: no timer, no job.
#![warn(missing_docs)]

#[cfg(test)]
use die_test_cap as _;

pub mod algo;
pub mod cells;
pub mod dashboard;
pub mod data;
pub mod detail;
pub mod error;
pub mod figure;
pub mod graph;
pub mod interact;
pub mod json;
pub mod plot;
pub mod raster;
pub mod scale;
pub mod scene;
pub mod source;
pub mod spec;
pub mod style;
pub mod surface;
pub mod svg;
pub mod theme;

pub use error::{FigError, Pos};

use die_core::session::{Kept, Value, json};
use die_core::{
    Args, BufferId, Ctx, Error, Extension, Prefix, PromptSpec, Registry, Result, ShowIn, Side,
    State,
};
use interact::{Action, Cursor, Setting, View};
use jiff::tz::TimeZone;
use source::Source;
use spec::{DataRef, Figure};
use std::cell::{OnceCell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use surface::{FigState, FigSurface, Kit, Shared, SharedKit};

/// What a host that shows no pixels says to `\v` (ruling 17), until phase 5's cells.
pub const NO_PIXELS: &str = "fig: this host shows no pixels yet";

/// How many figures put away by `\v` keep their views for the next, the oldest going first.
const PARKED: usize = 32;

/// The most of a command's output a restart keeps to chart again, rather than running the
/// command again unasked; a figure of a command that printed more isn't kept.
pub const KEPT_OUTPUT: usize = 256 * 1024;

/// Your options (C7), as fields.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Options {
    /// The directory DIE's own fonts are in, which a figure falls back on where the system has
    /// none it knows. None finds it as DIE does: `$DIE_FONT_DIR`, the app's bundle, the
    /// workspace's `fonts/`.
    pub fonts: Option<PathBuf>,
}

/// The extension: figures from data.
#[derive(Default)]
pub struct Fig {
    options: Options,
    /// The zone times without an offset are in: the one injected, else the system's, looked up
    /// the first time it's needed.
    zone: OnceCell<TimeZone>,
    /// The fonts every figure draws its text with.
    kit: SharedKit,
    /// Every figure there is.
    figs: Vec<Shared>,
    /// What a figure `\v` put away left: its view and cursor, by its source, for the next.
    parked: Vec<(Source, View, Option<Cursor>)>,
    /// What the session was last given, and what it was made from (call 1175's cache): the
    /// figures' revisions, how many there were, and the focused buffer.
    #[allow(clippy::type_complexity)]
    kept: RefCell<Option<((u64, usize, Option<BufferId>), Kept)>>,
}

impl Fig {
    /// The extension with your options, times in the system's zone.
    pub fn new(options: Options) -> Fig {
        Fig {
            kit: Kit::new(options.fonts.clone()),
            options,
            ..Fig::default()
        }
    }

    /// The extension with the default options, times in `zone` rather than the system's: for
    /// tests, which use a fixed zone (ruling 2).
    pub fn with_zone(zone: TimeZone) -> Fig {
        Fig {
            zone: OnceCell::from(zone),
            ..Fig::default()
        }
    }

    /// Your options.
    pub fn options(&self) -> &Options {
        &self.options
    }

    /// The zone times without an offset are in.
    pub fn zone(&self) -> &TimeZone {
        self.zone.get_or_init(TimeZone::system)
    }

    /// The figure whose buffer this is.
    pub fn figure(&self, buf: BufferId) -> Option<Shared> {
        self.figs
            .iter()
            .find(|s| s.borrow().buf == Some(buf))
            .cloned()
    }

    /// Every figure's buffer.
    pub fn figures(&self) -> Vec<BufferId> {
        self.figs.iter().filter_map(|s| s.borrow().buf).collect()
    }

    /// The figure of this source, if one is open.
    fn on_source(&self, source: &Source) -> Option<Shared> {
        let same = |s: &&Shared| s.borrow().source.same(source);
        self.figs.iter().find(same).cloned()
    }

    /// A figure made for `source`: a surface buffer `title`, its directory `dir`, starting where
    /// the last figure of that source was put away. Built now, or when first shown if `later`.
    #[allow(clippy::too_many_arguments)]
    fn make(
        &mut self,
        ctx: &mut Ctx,
        source: Source,
        title: &str,
        name: &str,
        dir: PathBuf,
        from: Option<BufferId>,
        file: Option<PathBuf>,
        seed: Option<(View, Option<Cursor>)>,
    ) -> Result<Shared> {
        let parked = self.parked.iter().position(|p| p.0.same(&source));
        let seed = seed.or_else(|| parked.map(|i| self.parked.remove(i)).map(|p| (p.1, p.2)));
        let zone = self.zone().clone();
        let state = FigState::new(
            source,
            name.into(),
            dir.clone(),
            self.kit.clone(),
            zone,
            seed,
        );
        let buf = ctx.create_surface(title, FigSurface::new(state.clone()));
        ctx.set_directory(buf, dir);
        ctx.change_settings(buf, |s| s.max_width = None)?;
        {
            let mut st = state.borrow_mut();
            (st.buf, st.from, st.file, st.behind) = (Some(buf), from, file, true);
        }
        self.figs.push(state.clone());
        Ok(state)
    }

    /// Show a figure in the window the command came from, built now if it's behind.
    fn show(ctx: &mut Ctx, state: &Shared) -> Result {
        let buf = state.borrow().buf.ok_or(die_core::Checkout::Gone)?;
        ctx.show(buf, ShowIn::Current)?;
        let mut st = state.borrow_mut();
        if st.behind {
            st.build(ctx);
        }
        Ok(())
    }

    /// `fig-toggle`: on a figure, back to its source; on text, its figure, made if there's none;
    /// in visual state, the selection's lines charted in `*fig: selection*`.
    fn toggle(&mut self, ctx: &mut Ctx, a: &Args) -> Result {
        let buf = a.buffer.or(ctx.focused_buffer());
        let buf = buf.ok_or_else(|| Error::Msg("No buffer here".into()))?;
        if let Some(state) = self.figure(buf) {
            return self.leave(ctx, a, &state);
        }
        if !ctx.pixels_supported() {
            ctx.messages.push(NO_PIXELS.into());
            return Ok(());
        }
        let dir = ctx
            .directory(buf)
            .map(Path::to_path_buf)
            .unwrap_or_default();
        let win = a.window.or(ctx.focused());
        if ctx.state_of(buf) == State::Visual
            && let Some(sel) = win.and_then(|w| ctx.text(buf).ok()?.selection(w))
        {
            let text = ctx.text(buf)?.rope().to_string();
            let lines = source::selected_lines(&text, sel.anchor, sel.head);
            // die-vim's, so `gv` brings the selection back; without die-vim, normal state.
            match ctx.run("vim-exit-visual", a) {
                Err(Error::NoCommand(_)) => ctx.set_state_of(buf, State::Normal),
                other => other?,
            }
            let source = Source::Text(lines.into());
            let title = "*fig: selection*";
            let state = self.make(ctx, source, title, "selection", dir, Some(buf), None, None)?;
            return Fig::show(ctx, &state);
        }
        ctx.text(buf)?;
        let source = Source::Buffer(buf);
        if let Some(state) = self.on_source(&source) {
            return Fig::show(ctx, &state);
        }
        let name = ctx.buffer_name(buf).unwrap_or_default().to_string();
        let file = ctx.file(buf).map(|f| f.path.clone());
        let title = format!("{name} *fig*");
        let state = self.make(ctx, source, &title, &name, dir, Some(buf), file, None)?;
        Fig::show(ctx, &state)
    }

    /// From a figure to its source in this window: the buffer it follows, else the one it was
    /// opened from, else its file. A figure no window shows any more goes, its view kept for
    /// the next `\v` on that source.
    fn leave(&mut self, ctx: &mut Ctx, _a: &Args, state: &Shared) -> Result {
        let (source, from, buf, file) = {
            let st = state.borrow();
            (st.source.clone(), st.from, st.buf, st.file.clone())
        };
        let to = match (&source, from) {
            (Source::Buffer(b), _) if ctx.is_live(*b) => *b,
            (_, Some(b)) if ctx.is_live(b) => b,
            _ => match file {
                Some(path) => ctx.open_file(&path)?,
                None => return Err(Error::Msg("fig: its source is gone".into())),
            },
        };
        ctx.show(to, ShowIn::Current)?;
        if let Some(buf) = buf.filter(|b| !shows(ctx, *b)) {
            // Kept by what the source is, not what it held: a command's line and directory,
            // not its output. A selection's text was a moment's, and isn't kept at all.
            if let Some(source) = source.identity() {
                let (view, cursor) = state.borrow().kept_view();
                self.parked.retain(|p| !p.0.same(&source));
                self.parked.push((source, view, cursor));
                if self.parked.len() > PARKED {
                    self.parked.remove(0);
                }
            }
            self.figs.retain(|s| !Rc::ptr_eq(s, state));
            // The window has left it, and core has queued telling it so: it goes after that.
            ctx.defer(move |ctx| {
                ctx.kill(buf);
                Ok(())
            });
        }
        Ok(())
    }

    /// `fig`: `:fig FILE` charts a file (followed in its buffer if one visits it, else polled),
    /// `:fig !cmd` runs the command once and charts what it printed, and `:fig` alone is
    /// `fig-toggle`.
    fn fig(&mut self, ctx: &mut Ctx, a: &Args) -> Result {
        let arg = a.text.as_deref().map(str::trim).unwrap_or("");
        if arg.is_empty() {
            return self.toggle(ctx, a);
        }
        if !ctx.pixels_supported() {
            ctx.messages.push(NO_PIXELS.into());
            return Ok(());
        }
        let here = a.buffer.or(ctx.focused_buffer());
        let dir = here
            .and_then(|b| ctx.directory(b).map(Path::to_path_buf))
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_default();
        if let Some(cmd) = arg.strip_prefix('!') {
            let cmd = cmd.trim().to_string();
            let source = Source::Command {
                cmd: cmd.clone(),
                dir: dir.clone(),
                out: None,
            };
            if let Some(state) = self.on_source(&source) {
                return Fig::show(ctx, &state);
            }
            let (title, name) = (format!("*fig: !{cmd}*"), format!("!{cmd}"));
            let state = self.make(ctx, source, &title, &name, dir, here, None, None)?;
            return Fig::show(ctx, &state);
        }
        let path = resolve(&dir, arg);
        let visiting = ctx.buffer_list().into_iter().find(|b| {
            ctx.file(*b)
                .is_some_and(|f| source::same_path(&f.path, &path))
        });
        if let Some(b) = visiting {
            let a = Args {
                buffer: Some(b),
                ..a.clone()
            };
            ctx.show(b, ShowIn::Current)?;
            return self.toggle(ctx, &a);
        }
        if !path.is_file() {
            return Err(Error::Msg(format!("fig: no file {}", path.display())));
        }
        let source = Source::File(path.clone());
        if let Some(state) = self.on_source(&source) {
            return Fig::show(ctx, &state);
        }
        let name = path
            .file_name()
            .map_or_else(|| arg.to_string(), |n| n.to_string_lossy().into_owned());
        let dir = path.parent().map(Path::to_path_buf).unwrap_or(dir);
        let title = format!("{name} *fig*");
        let state = self.make(ctx, source, &title, &name, dir, here, Some(path), None)?;
        Fig::show(ctx, &state)
    }

    /// A buffer was killed: a figure's goes with its state, its timers and jobs.
    fn killed(&mut self, buf: BufferId) {
        self.figs.retain(|s| s.borrow().buf != Some(buf));
    }
}

/// Whether a window shows the buffer.
fn shows(ctx: &Ctx, buf: BufferId) -> bool {
    let mut windows = ctx.window_list().into_iter();
    windows.any(|w| ctx.window_buffer(w) == Some(buf))
}

/// A path as typed, from `dir`: `~/` is home.
fn resolve(dir: &Path, typed: &str) -> PathBuf {
    let home = std::env::home_dir();
    let path = match (typed.strip_prefix("~/"), home) {
        (Some(rest), Some(home)) => home.join(rest),
        _ => PathBuf::from(typed),
    };
    source::lexical(&dir.join(path))
}

/// Every face die-fig declares, for a theme to style by name.
pub fn faces() -> &'static [&'static str] {
    theme::FACES
}

// ------------------------------------------------------------------ the keys' commands

fn not_a_figure() -> Error {
    Error::Msg("fig: not a figure".into())
}

/// The figure a command was typed on, and its window's pane.
fn here(ctx: &mut Ctx, a: &Args) -> Result<(Shared, die_core::WindowId)> {
    let buf = a.buffer.or(ctx.focused_buffer()).ok_or_else(not_a_figure)?;
    let state = ctx.with_ext(|fig: &mut Fig, _| fig.figure(buf))?;
    let state = state.ok_or_else(not_a_figure)?;
    let w = state.borrow().pane_of(a.window.or(ctx.focused()));
    let w = w.ok_or_else(|| Error::Msg("fig: the figure isn't shown yet".into()))?;
    Ok((state, w))
}

/// Run a figure key's command: the key card closes and what the pointer was in the middle of
/// ends where it stands (mouse-ux §8) unless `raw`, `f` acts, and the pictures catch up.
fn on_fig(
    ctx: &mut Ctx,
    a: &Args,
    raw: bool,
    f: impl FnOnce(&mut FigState, die_core::WindowId, &mut Ctx) -> Result,
) -> Result {
    let (state, w) = here(ctx, a)?;
    let mut st = state.borrow_mut();
    st.look(ctx);
    if !raw {
        if let Some(p) = st.panes.get_mut(&w) {
            p.help = false;
        }
        st.finish(w, ctx.now());
    }
    let r = f(&mut st, w, ctx);
    st.refresh(ctx);
    r
}

/// The count typed before the key, 1 without one.
fn count(a: &Args) -> u32 {
    a.count.unwrap_or(1).clamp(1, 10_000) as u32
}

/// The most zoom steps a count makes: 1.25 to this is past every limit and still finite, so a
/// count of thousands zooms as far as the data lets it rather than overflowing to nothing.
const ZOOM_COUNT: u32 = 200;

/// The zoom factor of a count of `+` (`by` 1) or `-` (`by` -1).
fn zoom_factor(a: &Args, by: i32) -> f64 {
    interact::consts::ZOOM_STEP.powi(count(a).min(ZOOM_COUNT) as i32 * by)
}

/// A key that acts on the pane through [`surface::Act`].
fn act(ctx: &mut Ctx, a: &Args, f: impl FnOnce(&mut surface::Act, &Args)) -> Result {
    on_fig(ctx, a, false, |st, w, ctx| {
        st.act(w, ctx.now(), |act| f(act, a));
        Ok(())
    })
}

/// The cursor's motions, each a command and its keys.
const MOTIONS: [(&str, &str, &str); 11] = [
    (
        "fig-left",
        "h",
        "h: the cursor to the previous point along x (a bar, a cell). A count repeats.",
    ),
    (
        "fig-right",
        "l",
        "l: the cursor to the next point along x. A count repeats.",
    ),
    (
        "fig-down",
        "j",
        "j: the cursor to the series below it at its x (a stack's segment).",
    ),
    (
        "fig-up",
        "k",
        "k: the cursor to the series above it at its x.",
    ),
    (
        "fig-peak-next",
        "w",
        "w: the cursor to the next peak or trough of its series.",
    ),
    (
        "fig-peak-prev",
        "b",
        "b: the cursor to the previous peak or trough of its series.",
    ),
    ("fig-first", "gg", "gg: the cursor to the first point."),
    (
        "fig-last",
        "G",
        "G: the cursor to the last point; on live data the view follows the newest.",
    ),
    (
        "fig-view-left",
        "H",
        "H: the cursor to the view's left edge.",
    ),
    (
        "fig-view-middle",
        "M",
        "M: the cursor to the view's middle.",
    ),
    (
        "fig-view-right",
        "L",
        "L: the cursor to the view's right edge.",
    ),
];

fn motion_of(name: &str) -> Action {
    match name {
        "fig-left" => Action::Step(-1),
        "fig-right" => Action::Step(1),
        // plot's order: the next series down the screen is +1 (plot/nav.rs), as Vim's j.
        "fig-down" => Action::Series(1),
        "fig-up" => Action::Series(-1),
        "fig-peak-next" => Action::Peak(1),
        "fig-peak-prev" => Action::Peak(-1),
        "fig-first" => Action::First,
        "fig-last" => Action::Last,
        "fig-view-left" => Action::ViewEdge(-1),
        "fig-view-middle" => Action::ViewEdge(0),
        _ => Action::ViewEdge(1),
    }
}

/// The view's scrolls by key: name, keys, doc, and how far across and up, in plots.
const SCROLLS: [(&str, &str, &str, f32, f32); 6] = [
    (
        "fig-scroll-left",
        "zh",
        "zh: scroll the view left an eighth of the plot. A count repeats.",
        -0.125,
        0.0,
    ),
    (
        "fig-scroll-right",
        "zl",
        "zl: scroll the view right an eighth of the plot. A count repeats.",
        0.125,
        0.0,
    ),
    (
        "fig-scroll-half-left",
        "zH",
        "zH: scroll the view left half the plot.",
        -0.5,
        0.0,
    ),
    (
        "fig-scroll-half-right",
        "zL",
        "zL: scroll the view right half the plot.",
        0.5,
        0.0,
    ),
    (
        "fig-scroll-down",
        "zj",
        "zj: scroll the view down an eighth, where y isn't fit to the data.",
        0.0,
        -0.125,
    ),
    (
        "fig-scroll-up",
        "zk",
        "zk: scroll the view up an eighth, where y isn't fit to the data.",
        0.0,
        0.125,
    ),
];

/// die-vim's `C-w` keys, the whiteboard's table, bound here as the text kind binds them, since a
/// surface without text doesn't inherit them and an extension can't ask die-vim for them.
const WINDOW_KEYS: [(&str, &str); 32] = [
    ("h <C-h> <Left> <C-Left>", "vim-window-left"),
    ("j <C-j> <Down> <C-Down>", "vim-window-down"),
    ("k <C-k> <Up> <C-Up>", "vim-window-up"),
    ("l <C-l> <Right> <C-Right>", "vim-window-right"),
    ("w <C-w>", "vim-window-next"),
    ("W <C-S-w>", "vim-window-prev"),
    ("t <C-t>", "vim-window-top-left"),
    ("b <C-b>", "vim-window-bottom-right"),
    ("p <C-p>", "vim-window-mru"),
    ("s S <C-s> <C-S-s>", "vim-window-split"),
    ("v <C-v>", "vim-window-vsplit"),
    ("n <C-n>", "vim-window-new"),
    ("c <C-c>", "vim-window-delete"),
    ("q <C-q>", "vim-quit"),
    ("o <C-o>", "vim-window-only"),
    ("= <C-=>", "vim-window-balance"),
    ("+", "vim-window-taller"),
    ("-", "vim-window-shorter"),
    (">", "vim-window-wider"),
    ("<lt>", "vim-window-narrower"),
    ("_ <C-_>", "vim-window-set-height"),
    ("|", "vim-window-set-width"),
    ("H <C-S-h>", "vim-window-move-far-left"),
    ("J <C-S-j>", "vim-window-move-very-bottom"),
    ("K <C-S-k>", "vim-window-move-very-top"),
    ("L <C-S-l>", "vim-window-move-far-right"),
    ("r <C-r>", "vim-window-rotate-downwards"),
    ("R <C-S-r>", "vim-window-rotate-upwards"),
    ("x <C-x>", "vim-window-exchange"),
    (":", "vim-window-ex"),
    ("0 1 2 3 4 5 6 7 8 9", "vim-window-digit-argument"),
    ("<Esc>", "vim-cancel"),
];

/// `/`'s answer, or the best match as it's typed: the cursor there.
fn find(
    st: &mut FigState,
    w: die_core::WindowId,
    now: std::time::Duration,
    query: &str,
    exact: bool,
) {
    let Some(model) = st.model.clone() else {
        return;
    };
    let labels = model.labels();
    let hit = match exact {
        true => labels.iter().find(|l| l.0 == query),
        false => labels
            .iter()
            .filter(|l| algo::tier(query, &l.0) < algo::search::UNMATCHED)
            .min_by_key(|l| algo::tier(query, &l.0)),
    };
    if let Some((_, c)) = hit.cloned() {
        st.act(w, now, |a| a.found(c));
    }
}

/// `n` and `N`: the next or previous label the last search matches, after the cursor's.
fn find_next(
    st: &mut FigState,
    w: die_core::WindowId,
    now: std::time::Duration,
    by: i32,
) -> Result {
    let (Some(model), Some(query)) = (st.model.clone(), st.query.clone()) else {
        return Err(Error::Msg("fig: no search yet".into()));
    };
    let mut hits: Vec<(String, Cursor)> = model
        .labels()
        .into_iter()
        .filter(|l| algo::tier(&query, &l.0) < algo::search::UNMATCHED)
        .collect();
    hits.sort_by_key(|l| algo::tier(&query, &l.0));
    if hits.is_empty() {
        return Err(Error::Msg(format!("fig: nothing matches {query}")));
    }
    let cursor = st.panes.get(&w).and_then(|p| p.cursor.clone());
    let series = |c: &Cursor| match c {
        Cursor::Point { series, .. } => Some(series.clone()),
        Cursor::Band { band, .. } => Some(data::SeriesKey::new(&band.to_string(), u32::MAX)),
        _ => None,
    };
    let at = cursor
        .as_ref()
        .and_then(|c| hits.iter().position(|h| series(&h.1) == series(c)));
    let n = hits.len() as i32;
    let i = match at {
        Some(i) => (i as i32 + by).rem_euclid(n),
        None if by > 0 => 0,
        None => n - 1,
    };
    let c = hits[i as usize].1.clone();
    st.act(w, now, |a| a.found(c));
    Ok(())
}

/// `E`: `Export to: `, NAME.png beside the source by default; the name's extension says PNG
/// (at 2× on the theme's ground) or SVG. A file there already is asked about.
fn export(ctx: &mut Ctx, a: &Args) -> Result {
    let (state, w) = here(ctx, a)?;
    let default = {
        let st = state.borrow();
        let stem = st
            .file
            .as_ref()
            .and_then(|p| p.file_stem())
            .map_or_else(|| "fig".to_string(), |s| s.to_string_lossy().into_owned());
        st.dir.join(format!("{stem}.png"))
    };
    let home = std::env::home_dir();
    let line = abbreviate(&default, home.as_deref());
    let dir = state.borrow().dir.clone();
    let spec = PromptSpec::new("Export to: ")
        .initial(&line)
        .history("fig-export")
        .line_first();
    ctx.prompt(spec, move |ctx, answer| {
        let Some(answer) = answer.filter(|a| !a.trim().is_empty()) else {
            return Ok(());
        };
        let path = resolve(&dir, answer.trim());
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(str::to_ascii_lowercase);
        let svg = match ext.as_deref() {
            Some("png") => false,
            Some("svg") => true,
            _ => {
                let name = path.file_name().unwrap_or_default().to_string_lossy();
                return Err(Error::Msg(format!(
                    "Export to a .png or an .svg, not {name}"
                )));
            }
        };
        let picture = state.borrow().picture(Some(w));
        let picture = picture.ok_or_else(|| Error::Msg("fig: nothing is drawn yet".into()))?;
        unless_kept(ctx, &path.clone(), move |ctx| {
            let shown = path.display().to_string();
            ctx.spawn(
                move || -> std::result::Result<String, String> {
                    let said = match svg {
                        true => {
                            let text = picture.svg()?;
                            std::fs::write(&path, text).map_err(|e| format!("{shown}: {e}"))?;
                            format!("Wrote {shown}")
                        }
                        false => {
                            let (bytes, w, h) = picture.png(2.0)?;
                            std::fs::write(&path, bytes).map_err(|e| format!("{shown}: {e}"))?;
                            format!("Wrote {shown} ({w}×{h})")
                        }
                    };
                    Ok(said)
                },
                |ctx, done| {
                    ctx.messages.push(match done {
                        Ok(said) => said,
                        Err(e) => format!("Export failed: {e}"),
                    });
                    Ok(())
                },
            )
            .detach();
            Ok(())
        })
    });
    Ok(())
}

/// Run `then` once you've said `y` to overwriting `path` if a file is there.
fn unless_kept(
    ctx: &mut Ctx,
    path: &Path,
    then: impl FnOnce(&mut Ctx) -> Result + 'static,
) -> Result {
    if !path.exists() {
        return then(ctx);
    }
    let shown = abbreviate(path, std::env::home_dir().as_deref());
    let label = format!("File {shown} exists; overwrite? (y or n) ");
    ctx.prompt(PromptSpec::new(&label), move |ctx, answer| {
        match answer.as_deref() {
            Some("y" | "yes") => then(ctx),
            _ => Ok(()),
        }
    });
    Ok(())
}

/// A path with the home directory as `~`.
fn abbreviate(path: &Path, home: Option<&Path>) -> String {
    match home.and_then(|h| path.strip_prefix(h).ok()) {
        Some(rest) => format!("~/{}", rest.display()),
        None => path.display().to_string(),
    }
}

/// `C`: the figure as a PNG on the pasteboard, through `osascript`, `wl-copy` or `xclip`, made in
/// a job with fonts of its own.
fn copy_image(ctx: &mut Ctx, a: &Args) -> Result {
    let (state, w) = here(ctx, a)?;
    let (picture, name) = {
        let st = state.borrow();
        (st.picture(Some(w)), st.name.clone())
    };
    let picture = picture.ok_or_else(|| Error::Msg("fig: nothing is drawn yet".into()))?;
    static TEMPS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = TEMPS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let tmp = std::env::temp_dir().join(format!("die-fig-{}-{n}.png", std::process::id()));
    ctx.spawn(
        move || -> std::result::Result<(u32, u32), String> {
            let (bytes, w, h) = picture.png(2.0)?;
            std::fs::write(&tmp, bytes).map_err(|e| format!("{}: {e}", tmp.display()))?;
            let path = die_core::process::path();
            let has = |p: &str| die_draw::export::which(p, path);
            let mut cmd = die_draw::export::pasteboard_command(&tmp, &has)
                .ok_or("no osascript, wl-copy or xclip on the PATH")?;
            let status = cmd.status().map_err(|e| e.to_string());
            let _ = std::fs::remove_file(&tmp);
            match status? {
                s if s.success() => Ok((w, h)),
                s => Err(format!("the pasteboard command failed: {s}")),
            }
        },
        move |ctx, done| {
            ctx.messages.push(match done {
                Ok((w, h)) => format!("Copied {name} as a PNG ({w}×{h})"),
                Err(e) => format!("Copy failed: {e}"),
            });
            Ok(())
        },
    )
    .detach();
    Ok(())
}

/// `t`: the data as TSV in a buffer beside the figure, where all of Vim works.
fn table(ctx: &mut Ctx, a: &Args) -> Result {
    let (state, w) = here(ctx, a)?;
    let (text, line, name, dir) = {
        let st = state.borrow();
        let model = st
            .model
            .clone()
            .ok_or_else(|| Error::Msg("fig: nothing is drawn yet".into()))?;
        let view = st.panes.get(&w).map(|p| p.view.clone()).unwrap_or_default();
        let (text, line) = model.tsv(&view);
        (text, line, st.name.clone(), st.dir.clone())
    };
    let b = ctx.output_buffer(&format!("*fig table: {name}*"), &text, &dir)?;
    let shown = match ctx
        .window_list()
        .into_iter()
        .find(|v| ctx.window_buffer(*v) == Some(b))
    {
        Some(v) => Some(v),
        None => ctx.show(b, ShowIn::Split(Side::Right))?,
    };
    if let (Some(v), Some(line)) = (shown, line)
        && let Ok(t) = ctx.text_mut(b)
    {
        let at = t.line_start(line.min(t.line_count().saturating_sub(1)));
        t.set_cursor(v, at);
    }
    Ok(())
}

/// `s`: the settings as a menu, each answer one setting turned over, one undo step.
fn settings(ctx: &mut Ctx, a: &Args) -> Result {
    let (state, w) = here(ctx, a)?;
    let items: Vec<String> = {
        let st = state.borrow();
        let Some(chart) = st.chart.as_ref() else {
            return Err(Error::Msg("fig: nothing is drawn yet".into()));
        };
        Setting::ALL
            .iter()
            .map(|s| {
                let on = st.toggles.value(chart, *s);
                format!("{}: {}", s.name(), if on { "on" } else { "off" })
            })
            .collect()
    };
    let spec = PromptSpec::new("Settings: ")
        .candidates(items)
        .require_match();
    ctx.prompt(spec, move |ctx, answer| {
        let name = answer.as_deref().and_then(|a| a.split(':').next());
        let Some(setting) = name.and_then(Setting::from_name) else {
            return Ok(());
        };
        let mut st = state.borrow_mut();
        st.flip(ctx, w, setting);
        let on = st
            .chart
            .as_ref()
            .is_some_and(|c| st.toggles.value(c, setting));
        ctx.messages.push(format!(
            "settings: {} {}",
            setting.name(),
            if on { "on" } else { "off" }
        ));
        st.refresh(ctx);
        Ok(())
    });
    Ok(())
}

/// `/`: find a series or category by name, fig's tiers over flex's order; the cursor goes to
/// the best match as you type and its series comes forward until `Esc`.
fn search(ctx: &mut Ctx, a: &Args) -> Result {
    let (state, w) = here(ctx, a)?;
    let labels: Vec<String> = {
        let st = state.borrow();
        let Some(model) = st.model.as_ref() else {
            return Err(Error::Msg("fig: nothing is drawn yet".into()));
        };
        model.labels().into_iter().map(|l| l.0).collect()
    };
    let typed = state.clone();
    let spec = PromptSpec::new("Find: ")
        .history("fig-find")
        .candidates(labels)
        .tiers(algo::tier)
        .on_change(move |ctx, text| {
            let mut st = typed.borrow_mut();
            if !text.is_empty() {
                find(&mut st, w, ctx.now(), text, false);
                st.query = Some(text.to_string());
            }
            st.refresh(ctx);
            Ok(())
        });
    ctx.prompt(spec, move |ctx, answer| {
        let mut st = state.borrow_mut();
        if let Some(answer) = answer.filter(|a| !a.is_empty()) {
            find(&mut st, w, ctx.now(), &answer, true);
            if st.query.is_none() {
                st.query = Some(answer);
            }
        }
        st.refresh(ctx);
        Ok(())
    });
    Ok(())
}

/// `fig-goto-error` (`<CR>` or a click on the banner): to the line and column the last build
/// failed at, in the figure's source or the data file it names.
fn goto_error(ctx: &mut Ctx, a: &Args) -> Result {
    let (state, _) = here(ctx, a)?;
    let (failed, source) = {
        let st = state.borrow();
        (st.error.clone(), st.source.clone())
    };
    let Some(failed) = failed else {
        return Err(Error::Msg("fig: no error to go to".into()));
    };
    let buf = match (&failed.file, &source) {
        (Some(path), _) | (None, Source::File(path)) => ctx.open_file(path)?,
        (None, Source::Buffer(b)) if ctx.is_live(*b) => *b,
        _ => return Err(Error::Msg(format!("fig: {}", failed.error))),
    };
    let shown = ctx.show(buf, ShowIn::Current)?;
    state.borrow_mut().settle(ctx);
    if let (Some(w), Some(at)) = (shown, failed.error.at)
        && let Ok(t) = ctx.text_mut(buf)
    {
        let line = t
            .line_start((at.line.saturating_sub(1) as usize).min(t.line_count().saturating_sub(1)));
        let rest = t
            .rope()
            .byte_slice(line..)
            .chars()
            .take(at.col.saturating_sub(1) as usize);
        let at = line + rest.map(char::len_utf8).sum::<usize>();
        t.set_cursor(w, at.min(t.len()));
    }
    Ok(())
}

/// `fig-spec`: the figure as a spec in a new buffer `NAME.fig.json`, unsaved: a guessed figure
/// becomes one to edit, its data by its file's path where it came from one.
fn write_spec(ctx: &mut Ctx, a: &Args) -> Result {
    let (state, _) = here(ctx, a)?;
    let (text, name, dir) = {
        let st = state.borrow();
        let figure = st
            .figure
            .clone()
            .ok_or_else(|| Error::Msg("fig: nothing parsed yet".into()))?;
        let figure = match figure {
            Figure::Chart(c) => {
                let mut c = st.toggles.apply(&c);
                if let (true, Some(path), DataRef::Inline(_)) =
                    (st.inferred, &st.file, &c.data.source)
                {
                    c.data.source = DataRef::File(path.clone());
                    c.data.format = path.to_str().and_then(spec::format_of_name);
                }
                Figure::Chart(c)
            }
            other => other,
        };
        let stem = st
            .file
            .as_ref()
            .and_then(|p| p.file_stem())
            .map(|s| s.to_string_lossy().into_owned());
        (
            spec::write(&figure),
            stem.unwrap_or_else(|| "fig".into()),
            st.dir.clone(),
        )
    };
    let b = ctx.create_text(&format!("{name}.fig.json"), &text);
    ctx.set_directory(b, dir);
    ctx.show(b, ShowIn::Current)?;
    // Run from `:`, a key or not: the figure may show nowhere now, and its poll stops.
    state.borrow_mut().settle(ctx);
    Ok(())
}

// ------------------------------------------------------------------ what a restart keeps

/// A series key as the session keeps it.
fn key_value(k: &data::SeriesKey) -> Value {
    json!([&*k.name, k.nth])
}

fn key_of(v: &Value) -> Option<data::SeriesKey> {
    let a = v.as_array()?;
    Some(data::SeriesKey::new(
        a.first()?.as_str()?,
        a.get(1)?.as_u64()? as u32,
    ))
}

fn cursor_value(c: &Option<Cursor>) -> Value {
    match c {
        Some(Cursor::Point { series, row }) => json!({"series": key_value(series), "row": row}),
        Some(Cursor::Band { band, series }) => {
            json!({"band": band, "series": series.as_ref().map(key_value)})
        }
        Some(Cursor::Cell { row, col }) => json!({"cell": [row, col]}),
        Some(Cursor::Node(id)) => json!({"node": &**id}),
        None => Value::Null,
    }
}

fn cursor_of(v: &Value) -> Option<Cursor> {
    if let Some(band) = v.get("band").and_then(Value::as_u64) {
        let series = v.get("series").and_then(key_of);
        return Some(Cursor::Band {
            band: band as u32,
            series,
        });
    }
    if let Some(id) = v.get("node").and_then(Value::as_str) {
        return Some(Cursor::Node(id.into()));
    }
    if let Some(c) = v.get("cell").and_then(Value::as_array) {
        let n = |i: usize| c.get(i).and_then(Value::as_u64).map(|n| n as u32);
        return Some(Cursor::Cell {
            row: n(0)?,
            col: n(1)?,
        });
    }
    let series = v.get("series").and_then(key_of)?;
    let row = v.get("row").and_then(Value::as_u64)? as u32;
    Some(Cursor::Point { series, row })
}

fn domain(d: Option<(f64, f64)>) -> Value {
    d.map_or(Value::Null, |(a, b)| json!([a, b]))
}

fn domain_of(v: Option<&Value>) -> Option<(f64, f64)> {
    let a = v?.as_array()?;
    let (lo, hi) = (a.first()?.as_f64()?, a.get(1)?.as_f64()?);
    (lo.is_finite() && hi.is_finite() && lo < hi).then_some((lo, hi))
}

/// A view and cursor as a restart keeps them (ruling 16): the domains, what's hidden or alone,
/// following, and the cursor, by series key. Not the reference (mouse-ux §6).
fn view_value(v: &View, c: &Option<Cursor>) -> Value {
    json!({
        "x": domain(v.x),
        "y": domain(v.y),
        "hidden": v.hidden.iter().map(key_value).collect::<Vec<_>>(),
        "only": v.only.as_ref().map(key_value),
        "follow": v.follow,
        "cursor": cursor_value(c),
    })
}

fn view_of(v: &Value) -> (View, Option<Cursor>) {
    let hidden = v.get("hidden").and_then(Value::as_array);
    let view = View {
        x: domain_of(v.get("x")),
        y: domain_of(v.get("y")),
        hidden: hidden
            .map(|h| h.iter().filter_map(key_of).collect())
            .unwrap_or_default(),
        only: v.get("only").and_then(key_of),
        follow: v.get("follow").and_then(Value::as_bool).unwrap_or(false),
        ..View::default()
    };
    (view, v.get("cursor").and_then(cursor_of))
}

impl Fig {
    /// What the session keeps of the figures: each with its view and cursor, and whether the
    /// focused window showed it; one of a buffer rides on that buffer, one of a file or a
    /// command in the state, a command's with what it printed, so a restart charts that and
    /// runs nothing. A selection's isn't kept: its text was a moment's.
    fn keep(&self, ctx: &Ctx) -> Kept {
        let focused = ctx.focused_buffer();
        let (mut buffers, mut figures) = (vec![], vec![]);
        for s in &self.figs {
            let st = s.borrow();
            if !st.buf.is_some_and(|b| ctx.is_live(b)) {
                continue;
            }
            let (view, cursor) = st.kept_view();
            let mut v = view_value(&view, &cursor);
            v["shown"] = Value::Bool(st.buf == focused);
            match &st.source {
                Source::Buffer(b) => buffers.push((*b, v)),
                Source::File(p) => {
                    v["file"] = json!(p.display().to_string());
                    figures.push(v);
                }
                // What it printed, to chart again: a restart doesn't run a command you typed
                // once (ruling 13's waiver is for the typing). One that hasn't printed, or
                // printed more than KEPT_OUTPUT, isn't kept.
                Source::Command { cmd, dir, out } => {
                    let Some(out) = out.as_deref().filter(|o| o.len() <= KEPT_OUTPUT) else {
                        continue;
                    };
                    v["cmd"] = json!(cmd);
                    v["dir"] = json!(dir.display().to_string());
                    v["out"] = json!(out);
                    figures.push(v);
                }
                Source::Text(_) => {}
            }
        }
        let state = match figures.is_empty() {
            true => Value::Null,
            false => json!({ "figures": figures }),
        };
        Kept { state, buffers }
    }
}

impl Extension for Fig {
    fn name(&self) -> &'static str {
        "fig"
    }

    fn register(&mut self, reg: &mut Registry) {
        theme::register(reg);
        reg.ext_command(
            "fig-toggle",
            "Show this buffer's data as a figure, or the buffer again from its figure. In visual state, chart the selected lines.",
            Fig::toggle,
        );
        reg.ext_command(
            "fig",
            "Chart a file (`:fig FILE`), or what a command prints (`:fig !cmd`, run once); alone, fig-toggle.",
            Fig::fig,
        );
        reg.command(
            "fig-spec",
            "Write the figure as a spec into a new buffer, NAME.fig.json, unsaved: a guessed figure becomes one to edit.",
            write_spec,
        );
        reg.command(
            "fig-goto-error",
            "Go to the line and column the figure's last build failed at, in its source or the data file it names.",
            goto_error,
        );
        for (name, _, doc) in MOTIONS {
            let action = motion_of(name);
            reg.command(name, doc, move |ctx, a| {
                let action = action.clone();
                act(ctx, a, move |act, a| {
                    act.move_cursor(&action, count(a));
                })
            });
        }
        for (name, _, doc, fx, fy) in SCROLLS {
            reg.command(name, doc, move |ctx, a| {
                act(ctx, a, move |act, a| {
                    let n = count(a) as f32;
                    act.scroll_keys(fx * n, fy * n);
                })
            });
        }
        reg.command(
            "fig-zoom-in",
            "+ = K: zoom in about the cursor, or the middle. A count repeats; one undo step.",
            |ctx, a| act(ctx, a, |act, a| act.zoom_keys(zoom_factor(a, 1))),
        );
        reg.command(
            "fig-zoom-out",
            "- J: zoom out about the cursor, or the middle, no further than the data.",
            |ctx, a| act(ctx, a, |act, a| act.zoom_keys(zoom_factor(a, -1))),
        );
        reg.command(
            "fig-zero",
            "0: reset the view to fit the data; after a count, a digit of it.",
            |ctx, a| {
                if let Some(n) = a.count {
                    ctx.set_prefix(Prefix {
                        count: Some(n.saturating_mul(10)),
                        register: a.register,
                    });
                    return Ok(());
                }
                act(ctx, a, |act, _| act.reset(None))
            },
        );
        reg.command(
            "fig-digit",
            "A digit of the count for the figure's next key.",
            |ctx, a| {
                let digit = a.char().and_then(|c| c.to_digit(10)).unwrap_or(0) as usize;
                let n = a
                    .count
                    .unwrap_or(0)
                    .saturating_mul(10)
                    .saturating_add(digit);
                ctx.set_prefix(Prefix {
                    count: Some(n),
                    register: a.register,
                });
                Ok(())
            },
        );
        reg.command(
            "fig-centre",
            "zz: centre the view on the cursor.",
            |ctx, a| act(ctx, a, |act, _| act.centre()),
        );
        reg.command("fig-hide", "x: hide the cursor's series.", |ctx, a| {
            act(ctx, a, |act, _| {
                act.hide();
            })
        });
        reg.command(
            "fig-only",
            "o: show only the cursor's series, or all of them again.",
            |ctx, a| {
                act(ctx, a, |act, _| {
                    act.only();
                })
            },
        );
        reg.command("fig-show-all", "X: show every series.", |ctx, a| {
            act(ctx, a, |act, _| act.show_all())
        });
        reg.command(
            "fig-undo",
            "u: undo the last view change (a zoom, a pan, a brush, a legend click, a setting).",
            |ctx, a| {
                on_fig(ctx, a, false, |st, w, ctx| match st.undo(ctx, w, false) {
                    true => Ok(()),
                    false => Err(Error::Msg("fig: no view change to undo".into())),
                })
            },
        );
        reg.command("fig-redo", "C-r: redo a view change undone.", |ctx, a| {
            on_fig(ctx, a, false, |st, w, ctx| match st.undo(ctx, w, true) {
                true => Ok(()),
                false => Err(Error::Msg("fig: no view change to redo".into())),
            })
        });
        reg.command(
            "fig-reference",
            "m: set the reference at the cursor, or clear it there: every card then shows the change since.",
            |ctx, a| {
                on_fig(ctx, a, false, |st, w, ctx| {
                    match st.act(w, ctx.now(), |a| a.reference()) {
                        Some(true) => Ok(()),
                        _ => Err(Error::Msg("fig: no cursor yet".into())),
                    }
                })
            },
        );
        reg.command(
            "fig-to-reference",
            "`: the cursor to the reference.",
            |ctx, a| {
                on_fig(ctx, a, false, |st, w, ctx| {
                    match st.act(w, ctx.now(), |a| a.to_reference()) {
                        Some(true) => Ok(()),
                        _ => Err(Error::Msg(
                            "fig: no reference set: m or ⇧-click sets one".into(),
                        )),
                    }
                })
            },
        );
        reg.command(
            "fig-cancel",
            "Esc: drop a brush, take a pan back, close the key card, or clear the search's emphasis or the reference.",
            |ctx, a| {
                on_fig(ctx, a, true, |st, w, _| {
                    st.cancel(w);
                    Ok(())
                })
            },
        );
        reg.command(
            "fig-help",
            "?: the figure's keys, until any key.",
            |ctx, a| {
                on_fig(ctx, a, true, |st, w, ctx| {
                    st.finish(w, ctx.now());
                    if let Some(p) = st.panes.get_mut(&w) {
                        p.help = !p.help;
                        // The settle after this command keeps it; after any other, it goes.
                        st.help_new = p.help;
                    }
                    Ok(())
                })
            },
        );
        reg.command(
            "fig-detail",
            "⏎: what the cursor is on and its values; on a figure whose last build failed, to the error.",
            |ctx, a| {
                let (state, w) = here(ctx, a)?;
                if state.borrow().error.is_some() {
                    return goto_error(ctx, a);
                }
                let said = state.borrow().detail(w);
                ctx.messages.push(said);
                Ok(())
            },
        );
        reg.command("fig-find", "/: find a series or category by name.", search);
        reg.command(
            "fig-find-next",
            "n: the next match of the last search.",
            |ctx, a| on_fig(ctx, a, false, |st, w, ctx| find_next(st, w, ctx.now(), 1)),
        );
        reg.command(
            "fig-find-prev",
            "N: the previous match of the last search.",
            |ctx, a| on_fig(ctx, a, false, |st, w, ctx| find_next(st, w, ctx.now(), -1)),
        );
        reg.command(
            "fig-table",
            "t: the data as TSV in a buffer beside the figure, hidden series left out.",
            table,
        );
        reg.command(
            "fig-settings",
            "s: the view's settings (grid, axes, log y, stack, labels, markers), each answer one undo step.",
            settings,
        );
        reg.command(
            "fig-export",
            "E: export the figure, asking `Export to: ` (NAME.png beside its source): a .png at 2× on the theme's ground, or an .svg.",
            export,
        );
        reg.command(
            "fig-copy-image",
            "C: put the figure on the pasteboard as a PNG.",
            copy_image,
        );

        use State::Normal;
        let k = reg.kind_keymap(surface::KIND);
        for (name, keys, _) in MOTIONS {
            k.bind(Normal, keys, name);
        }
        for (name, keys, ..) in SCROLLS {
            k.bind(Normal, keys, name);
        }
        for key in ["+", "=", "K"] {
            k.bind(Normal, key, "fig-zoom-in");
        }
        for key in ["-", "J"] {
            k.bind(Normal, key, "fig-zoom-out");
        }
        for d in 1..=9 {
            k.bind(Normal, &d.to_string(), "fig-digit");
        }
        k.bind(Normal, "0", "fig-zero")
            .bind(Normal, "zz", "fig-centre")
            .bind(Normal, "x", "fig-hide")
            .bind(Normal, "o", "fig-only")
            .bind(Normal, "X", "fig-show-all")
            .bind(Normal, "u", "fig-undo")
            .bind(Normal, "<C-r>", "fig-redo")
            .bind(Normal, "<D-z>", "fig-undo")
            .bind(Normal, "<D-S-z>", "fig-redo")
            .bind(Normal, "/", "fig-find")
            .bind(Normal, "n", "fig-find-next")
            .bind(Normal, "N", "fig-find-prev")
            .bind(Normal, "t", "fig-table")
            .bind(Normal, "s", "fig-settings")
            .bind(Normal, "E", "fig-export")
            .bind(Normal, "C", "fig-copy-image")
            .bind(Normal, "m", "fig-reference")
            .bind(Normal, "`", "fig-to-reference")
            .bind(Normal, "?", "fig-help")
            .bind(Normal, "<Esc>", "fig-cancel")
            .bind(Normal, "<CR>", "fig-detail")
            .bind(Normal, "<leader>v", "fig-toggle")
            // The keys a buffer can't do without, as the whiteboard binds them for its own
            // surface: there's nothing to write, so ZZ is ZQ.
            .bind(Normal, ":", "vim-ex")
            .bind(Normal, "ZZ", "vim-quit")
            .bind(Normal, "ZQ", "vim-quit")
            .bind(Normal, "<C-^>", "vim-alternate-buffer")
            .bind(Normal, "<C-o>", "vim-jump-backward")
            .bind(Normal, "<leader>z", "toggle-max-width");
        for (keys, name) in WINDOW_KEYS {
            for key in keys.split_whitespace() {
                k.bind(Normal, &format!("<C-w>{key}"), name);
            }
        }
    }

    /// Three subscriptions, each a look at an empty list while there's no figure: a buffer's
    /// edits (a figure following it charts them once it's quiet), a command's end (the settle:
    /// panes of windows gone freed, the poll on or off, the theme compared), and a buffer's kill.
    fn init(&mut self, ctx: &mut Ctx) -> Result {
        ctx.on_edited(|ctx, buf| {
            let figs = ctx.with_ext(|fig: &mut Fig, _| fig.figs.clone())?;
            if figs.is_empty() {
                return Ok(());
            }
            let path = ctx.file(buf).map(|f| f.path.clone());
            for s in figs {
                let follows = s.borrow().follows(buf, path.as_deref());
                if follows {
                    s.borrow_mut().edited(ctx);
                }
            }
            Ok(())
        })
        .detach();
        ctx.on_command_ended(|ctx| {
            let figs = ctx.with_ext(|fig: &mut Fig, _| fig.figs.clone())?;
            for s in figs {
                s.borrow_mut().settle(ctx);
            }
            Ok(())
        })
        .detach();
        ctx.on_buffer_killed(|ctx, buf| Ok(ctx.with_ext(|fig: &mut Fig, _| fig.killed(buf))?))
            .detach();
        Ok(())
    }

    /// Each figure, by its source, with its view and cursor (ruling 16). Cached by the figures'
    /// revisions and the focused buffer, since this is asked twice a second.
    fn save_session(&self, ctx: &Ctx) -> Kept {
        let revs = self.figs.iter().map(|s| s.borrow().rev).sum::<u64>();
        let rev = (revs, self.figs.len(), ctx.focused_buffer());
        if let Some((at, kept)) = &*self.kept.borrow()
            && *at == rev
        {
            return kept.clone();
        }
        let kept = self.keep(ctx);
        *self.kept.borrow_mut() = Some((rev, kept.clone()));
        kept
    }

    /// The figures as they were: each made again with its view, built when a window shows it;
    /// the one the focused window showed is shown in it, as a page is (core drops a window that
    /// showed a surface). The pixels come at its first `Resize`, the host's sink being connected
    /// after this.
    fn restore_session(&mut self, ctx: &mut Ctx, kept: Kept) -> Result {
        let mut show = None;
        let mut again = |fig: &mut Fig, ctx: &mut Ctx, source: Source, v: &Value| -> Result {
            let (title, name, dir, file) = match &source {
                Source::Buffer(b) => {
                    let name = ctx.buffer_name(*b).unwrap_or_default().to_string();
                    let dir = ctx.directory(*b).map(Path::to_path_buf).unwrap_or_default();
                    let file = ctx.file(*b).map(|f| f.path.clone());
                    (format!("{name} *fig*"), name, dir, file)
                }
                Source::File(p) => {
                    let name = p
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned();
                    let dir = p.parent().map(Path::to_path_buf).unwrap_or_default();
                    (format!("{name} *fig*"), name, dir, Some(p.clone()))
                }
                Source::Command { cmd, dir, .. } => (
                    format!("*fig: !{cmd}*"),
                    format!("!{cmd}"),
                    dir.clone(),
                    None,
                ),
                Source::Text(_) => return Ok(()),
            };
            let from = match &source {
                Source::Buffer(b) => Some(*b),
                _ => None,
            };
            let seed = Some(view_of(v));
            let state = fig.make(ctx, source, &title, &name, dir, from, file, seed)?;
            if v.get("shown") == Some(&Value::Bool(true)) {
                show = state.borrow().buf;
            }
            Ok(())
        };
        for (buf, v) in &kept.buffers {
            if ctx.text(*buf).is_ok()
                && let Err(e) = again(self, ctx, Source::Buffer(*buf), v)
            {
                ctx.messages.push(format!("fig: {e}"));
            }
        }
        let figures = kept.state.get("figures").and_then(Value::as_array);
        for v in figures.into_iter().flatten() {
            let source = if let Some(f) = v.get("file").and_then(Value::as_str) {
                Source::File(PathBuf::from(f))
            } else if let (Some(cmd), Some(out)) = (
                v.get("cmd").and_then(Value::as_str),
                v.get("out").and_then(Value::as_str),
            ) {
                let dir = v.get("dir").and_then(Value::as_str).unwrap_or("");
                Source::Command {
                    cmd: cmd.into(),
                    dir: PathBuf::from(dir),
                    out: Some(out.into()),
                }
            } else {
                continue;
            };
            if let Err(e) = again(self, ctx, source, v) {
                ctx.messages.push(format!("fig: {e}"));
            }
        }
        if let Some(buf) = show {
            ctx.show(buf, ShowIn::Current)?;
        }
        Ok(())
    }

    /// The figures' children would be killed here; phase 2 runs none that outlive a job.
    fn shutdown(&mut self, _ctx: &mut Ctx) {
        self.figs.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use die_core::Harness;

    #[test]
    fn fig_toggle_on_a_host_without_pixels_says_so_and_makes_nothing() {
        let exts: Vec<Box<dyn Extension>> =
            vec![Box::new(die_vim::Vim::default()), Box::new(Fig::default())];
        let mut ctx = Ctx::new(exts).unwrap();
        let buf = ctx.create_text("data.csv", "time,a\n1,2\n");
        ctx.open_window(buf, 80, 24).unwrap();
        ctx.run("fig-toggle", &Args::default()).unwrap();
        assert!(
            ctx.messages.iter().any(|m| m == NO_PIXELS),
            "{:?}",
            ctx.messages
        );
        assert_eq!(ctx.buffer_list().len(), 1, "no figure's buffer");
        assert_eq!(ctx.next_timer(), None);
    }

    #[test]
    fn nothing_runs_before_a_figure_is_asked_for() {
        let exts: Vec<Box<dyn Extension>> =
            vec![Box::new(die_vim::Vim::default()), Box::new(Fig::default())];
        let mut h = Harness::new(exts).unwrap();
        h.open_text("data.csv", "time,a\n1,2\n").unwrap();
        h.run_until_idle();
        assert_eq!(h.next_timer(), None);
    }

    #[test]
    fn a_zone_given_is_the_zone_used() {
        let zone = TimeZone::fixed(jiff::tz::offset(2));
        let fig = Fig::with_zone(zone.clone());
        assert_eq!(fig.zone(), &zone);
        assert_eq!(fig.options(), &Options::default());
        assert_eq!(Fig::new(Options::default()).name(), "fig");
    }

    #[test]
    fn a_view_reads_back_as_the_session_keeps_it() {
        let a = data::SeriesKey::new("api", 0);
        let mut view = View {
            x: Some((1.0, 2.5)),
            follow: true,
            only: Some(a.clone()),
            ..View::default()
        };
        view.hidden.insert(data::SeriesKey::new("api", 1));
        let cursor = Some(Cursor::Point { series: a, row: 3 });
        let (v, c) = view_of(&view_value(&view, &cursor));
        assert_eq!((v, c), (view, cursor));
        let band = Some(Cursor::Band {
            band: 2,
            series: None,
        });
        assert_eq!(view_of(&view_value(&View::default(), &band)).1, band);
    }
}
