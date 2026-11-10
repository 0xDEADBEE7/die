//! die-whiteboard: a whiteboard in a DIE buffer, as an extension (scope D14). A pen first,
//! one-key shape and arrow tools worked with the pointer, labels, a selection; SVG as the file,
//! in a dialect any browser opens and DIE reads back exactly; PNG export and the PNG on the
//! pasteboard. It depends on die-core alone, so it's platform-neutral and out of the product:
//! `cargo xtask lint` holds it to that, and it moves to its own repository at M4. Its parts:
//!
//! - this file: [`Whiteboard`], the extension, with your [`Options`], the boards it has open,
//!   autosave, the `whiteboard-*` commands, the keymap and what a restart keeps;
//! - [`model`]: the board's elements, in board units (logical pixels, the SVG's units);
//! - [`ink`]: the pen's smoothing and simplification;
//! - [`view`]: pan and zoom, and the pointer's way through them;
//! - [`undo`]: the operation log;
//! - [`surface`]: a board's state, the tools as the pointer works them, and the surface;
//! - [`render`], [`cells`] and [`pixels`]: the renderer trait, the cell renderer that runs today,
//!   and the pixel renderer's stub for when core's pixel slice (M3a) lands;
//! - [`svg`] and [`file`]: the file's dialect, read and written, and the files on disk;
//! - [`canvas`], [`fonts`] and [`export`]: tiny-skia behind one module, the exporter's font, PNG
//!   encoding and the pasteboard's command lines.
//!
//! Nothing here reads or runs anything at registration or init. While no board is dirty, no
//! timer runs.
#![warn(missing_docs)]

#[cfg(test)]
use die_test_cap as _;

pub mod canvas;
pub mod cells;
pub mod export;
pub mod file;
pub mod fonts;
pub mod ink;
pub mod model;
pub mod pixels;
pub mod render;
pub mod surface;
pub mod svg;
pub mod undo;
pub mod view;

use die_core::session::{Kept, Value, json};
use die_core::{
    Args, BufferId, Color, Ctx, Error, Extension, PromptSpec, Registry, Result, Rgb, ShowIn, State,
    Style, Sub, recent_first,
};
use model::{INKS, Id};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;
use surface::{BoardState, BoardSurface, Effect, KIND, Shared, Tool};
use svg::Look;
use view::{View, ZOOM_STEP};

// ------------------------------------------------------------------ options

/// What a PNG export draws behind the board.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Background {
    /// The theme's `default` background.
    #[default]
    Theme,
    /// White.
    White,
    /// Nothing: an alpha channel.
    Transparent,
}

impl Background {
    /// Its name, as the option is written.
    pub fn name(self) -> &'static str {
        match self {
            Background::Theme => "theme",
            Background::White => "white",
            Background::Transparent => "transparent",
        }
    }
}

/// Your options (C7), as fields.
#[derive(Clone, Debug, PartialEq)]
pub struct Options {
    /// Where the fonts are, for the PNG exporter's labels. None for DIE's rule: `$DIE_FONT_DIR`,
    /// the app bundle's, then the workspace's `fonts/`.
    pub font_dir: Option<PathBuf>,
    /// Pixels per logical pixel in a PNG export.
    pub png_scale: f32,
    /// What a PNG export draws behind the board.
    pub export_background: Background,
    /// Make a new board the window's whole width, rather than your text defaults' 80 columns
    /// centred, which `\z` toggles.
    pub full_width: bool,
    /// How long after the last change a board is written.
    pub autosave: Duration,
}

impl Default for Options {
    /// DIE's font rule, PNGs at 2x on the theme's background, a file's width, a second.
    fn default() -> Options {
        Options {
            font_dir: None,
            png_scale: 2.0,
            export_background: Background::Theme,
            full_width: false,
            autosave: Duration::from_secs(1),
        }
    }
}

// ------------------------------------------------------------------ faces

/// The faces, by name: one an ink, which a theme restyles.
pub mod face {
    /// The six inks, `1`..`6`. The first defaults to the `default` foreground, the rest to the
    /// theme's ANSI red, yellow, green, blue and magenta.
    pub const INK: [&str; 6] = [
        "whiteboard-ink-1",
        "whiteboard-ink-2",
        "whiteboard-ink-3",
        "whiteboard-ink-4",
        "whiteboard-ink-5",
        "whiteboard-ink-6",
    ];
}

/// The ANSI colour each ink falls back on: index 0 is never read (ink 1 is the default
/// foreground); then red, yellow, green, blue, magenta.
const ANSI_OF_INK: [usize; 6] = [7, 1, 3, 2, 4, 5];

/// Each ink's colour for a cell: the face's foreground if a theme styles it, else the default
/// foreground for ink 1 and the theme's ANSI colour for the rest.
pub fn ink_colors(ctx: &Ctx) -> [Color; 6] {
    let mut out = [Color::Default; 6];
    for (i, slot) in out.iter_mut().enumerate() {
        let styled = ctx.face_style(face::INK[i]).and_then(|s| s.fg);
        *slot = match (i, styled) {
            (_, Some(c)) => Color::Rgb(c),
            (0, None) => Color::Default,
            (i, None) => Color::Rgb(ctx.theme().ansi[ANSI_OF_INK[i]]),
        };
    }
    out
}

/// The `default` face's foreground, or light grey without a theme.
pub fn default_fg(ctx: &Ctx) -> Rgb {
    ctx.face_style("default")
        .and_then(|s| s.fg)
        .unwrap_or(Rgb(0xd0, 0xd0, 0xd0))
}

/// The `default` face's background, or near black without a theme.
pub fn theme_bg(ctx: &Ctx) -> Rgb {
    ctx.face_style("default")
        .and_then(|s| s.bg)
        .unwrap_or(Rgb(0x10, 0x10, 0x10))
}

/// Each ink's colour as a colour, for a file or an export.
pub fn ink_rgbs(ctx: &Ctx) -> [Rgb; 6] {
    let fg = default_fg(ctx);
    ink_colors(ctx).map(|c| match c {
        Color::Default => fg,
        Color::Rgb(c) => c,
    })
}

// ------------------------------------------------------------------ the extension

/// The extension: your options, and the boards open now.
pub struct Whiteboard {
    /// Your options.
    pub options: Options,
    boards: BTreeMap<BufferId, Shared>,
    /// The autosave timer, while it's set.
    timer: Option<Sub>,
    temps: u64,
}

impl Default for Whiteboard {
    fn default() -> Whiteboard {
        Whiteboard::new(Options::default())
    }
}

impl Whiteboard {
    /// The extension with these options.
    pub fn new(options: Options) -> Whiteboard {
        Whiteboard {
            options,
            boards: BTreeMap::new(),
            timer: None,
            temps: 0,
        }
    }

    /// For tests: PNGs at 1x, so pixel coordinates are board units; everything else as yours.
    pub fn for_tests() -> Whiteboard {
        Whiteboard::new(Options {
            png_scale: 1.0,
            ..Options::default()
        })
    }

    /// The boards open now, by buffer.
    pub fn boards(&self) -> Vec<BufferId> {
        self.boards.keys().copied().collect()
    }

    /// A board's state, if `buf` is a board.
    pub fn board(&self, buf: BufferId) -> Option<Shared> {
        self.boards.get(&buf).cloned()
    }

    /// The board on `path`, if one's open.
    pub fn board_on(&self, path: &Path) -> Option<BufferId> {
        self.boards
            .iter()
            .find(|(_, s)| s.borrow().path == path)
            .map(|(b, _)| *b)
    }

    /// The board used last, if any is open.
    pub fn most_recent(&self, ctx: &Ctx) -> Option<BufferId> {
        ctx.buffer_list()
            .into_iter()
            .find(|b| self.boards.contains_key(b) && ctx.is_live(*b))
    }

    /// How a board's file and exports are coloured: the inks as the theme has them, on its
    /// background.
    pub fn look(&self, ctx: &Ctx, state: &BoardState) -> Look {
        Look {
            inks: ink_rgbs(ctx),
            bg: Some(theme_bg(ctx)),
            cell: state.cell_size(),
        }
    }

    /// Open the board in `path`, or go to the one already open on it: a buffer named
    /// `*whiteboard: NAME*`, in this window.
    pub fn open(&mut self, ctx: &mut Ctx, path: &Path) -> Result<BufferId> {
        if let Some(buf) = self.board_on(path).filter(|b| ctx.is_live(*b)) {
            return Ok(buf);
        }
        let board = file::load(path).map_err(Error::Msg)?;
        let mut state = BoardState::new(path.to_path_buf(), board);
        state.inks = ink_colors(ctx);
        state.cell = ctx.cell_pixels();
        let state = state.shared();
        let buf = ctx.create_surface(&file::buffer_name(path), BoardSurface::new(state.clone()));
        state.borrow_mut().buf = Some(buf);
        if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
            ctx.set_directory(buf, dir);
        }
        if self.options.full_width {
            let _ = ctx.change_settings(buf, |s| s.max_width = None);
        }
        self.boards.retain(|b, _| ctx.is_live(*b));
        self.boards.insert(buf, state);
        Ok(buf)
    }

    /// Write `buf`'s board now, whether or not it changed.
    pub fn save_now(&self, ctx: &Ctx, buf: BufferId) -> Result {
        let state = self.board(buf).ok_or(not_a_board())?;
        let mut st = state.borrow_mut();
        write_board(ctx, &mut st).map_err(Error::Msg)
    }

    /// Whether any board is behind its file.
    fn any_dirty(&self) -> bool {
        self.boards.values().any(|s| s.borrow().dirty)
    }

    /// Set the autosave timer, if a board is dirty and it isn't set.
    fn arm(&mut self, ctx: &mut Ctx) {
        if self.timer.is_some() || !self.any_dirty() {
            return;
        }
        self.timer = Some(ctx.after(self.options.autosave, autosave_tick));
    }

    /// A buffer was killed: if it was a board that's behind, write it, and with nothing left
    /// behind, no timer.
    fn killed(&mut self, ctx: &mut Ctx, buf: BufferId) {
        let Some(state) = self.boards.remove(&buf) else {
            return;
        };
        {
            let mut st = state.borrow_mut();
            if st.dirty
                && let Err(e) = write_board(ctx, &mut st)
            {
                ctx.messages.push(e);
            }
        }
        if !self.any_dirty() {
            self.timer = None;
        }
    }
}

/// Write a board's file, and say in its status whether that went.
fn write_board(ctx: &Ctx, st: &mut BoardState) -> std::result::Result<(), String> {
    let look = Look {
        inks: ink_rgbs(ctx),
        bg: Some(theme_bg(ctx)),
        cell: st.cell_size(),
    };
    let wrote = file::save(&st.path, &st.board, &look);
    st.rev += 1;
    match wrote {
        Ok(()) => {
            st.dirty = false;
            st.error = None;
            Ok(())
        }
        Err(e) => {
            // The status line gets the file's name, the messages its whole path.
            st.error = Some(format!("{}: {e}", st.name()));
            Err(format!("{}: {e}", st.path.display()))
        }
    }
}

/// The autosave timer: write each board quiet for the autosave time, and set the timer again
/// for one that changed more recently.
fn autosave_tick(ctx: &mut Ctx) -> Result {
    let again = ctx.with_ext(|wb: &mut Whiteboard, ctx| {
        wb.timer = None;
        let now = ctx.now();
        let mut soonest: Option<Duration> = None;
        for state in wb.boards.values() {
            let mut st = state.borrow_mut();
            if !st.dirty {
                continue;
            }
            let due = st.changed_at + wb.options.autosave;
            if due <= now {
                if let Err(e) = write_board(ctx, &mut st) {
                    ctx.messages.push(e);
                }
            } else {
                let wait = due - now;
                soonest = Some(soonest.map_or(wait, |s| s.min(wait)));
            }
        }
        if let Some(wait) = soonest {
            wb.timer = Some(ctx.after(wait, autosave_tick));
        }
    });
    Ok(again?)
}

/// Something may have changed on `buf`'s board: set the autosave timer if it's behind.
pub(crate) fn after_change(ctx: &mut Ctx, buf: Option<BufferId>) {
    if buf.is_none() {
        return;
    }
    let _ = ctx.with_ext(|wb: &mut Whiteboard, ctx| wb.arm(ctx));
}

fn not_a_board() -> Error {
    Error::Msg("Not in a whiteboard".into())
}

/// The board a command acts on: its buffer's, or the focused buffer's.
fn target(ctx: &mut Ctx, args: &Args) -> Result<(BufferId, Shared)> {
    let buf = args
        .buffer
        .or_else(|| ctx.focused_buffer())
        .ok_or_else(not_a_board)?;
    let state = ctx
        .with_ext(|wb: &mut Whiteboard, _| wb.board(buf))?
        .ok_or_else(not_a_board)?;
    state.borrow_mut().inks = ink_colors(ctx);
    Ok((buf, state))
}

fn apply_effect(ctx: &mut Ctx, buf: BufferId, effect: Effect) {
    match effect {
        Effect::None => {}
        Effect::EnterInsert => ctx.set_state_of(buf, State::Insert),
        Effect::LeaveInsert => ctx.set_state_of(buf, State::Normal),
    }
}

/// Change the board a command acts on with `f`, then set the autosave timer if it's behind.
fn change(
    ctx: &mut Ctx,
    args: &Args,
    f: impl FnOnce(&mut BoardState, Duration) -> Result<Effect>,
) -> Result {
    let (buf, state) = target(ctx, args)?;
    let now = ctx.now();
    let effect = f(&mut state.borrow_mut(), now)?;
    apply_effect(ctx, buf, effect);
    after_change(ctx, Some(buf));
    Ok(())
}

/// The directory a command was given, Emacs's `default-directory`: its buffer's, or the
/// focused one's.
fn here(ctx: &Ctx, args: &Args) -> PathBuf {
    let buf = args.buffer.or_else(|| ctx.focused_buffer());
    let dir = buf.and_then(|b| ctx.directory(b)).map(Path::to_path_buf);
    dir.unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| "/".into()))
}

// ------------------------------------------------------------------ opening boards

/// Open the board in `path` and show it in this window.
fn show_board(ctx: &mut Ctx, path: &Path) -> Result {
    let buf = ctx.with_ext(|wb: &mut Whiteboard, ctx| wb.open(ctx, path))??;
    ctx.show(buf, ShowIn::Current)?;
    Ok(())
}

/// The `Board: ` prompt from `line`: it descends directories like `\f`, lists `.svg` files the
/// ones answered last first, and takes a new name with `<M-CR>`, which gets `.svg` if it names
/// no extension. Answering with a directory asks again from there.
fn ask_for_board(ctx: &mut Ctx, dir: PathBuf, line: &str) {
    let home = std::env::home_dir();
    // What was answered, as the file it named: its last part, with `.svg` as it got it.
    let recent: Vec<String> = ctx
        .prompt_history("board")
        .iter()
        .map(|h| {
            let name = h.rsplit('/').next().unwrap_or(h);
            file::with_svg(Path::new(name))
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    let spec = PromptSpec::new("Board: ")
        .initial(line)
        .history("board")
        .descends()
        .candidates_from(listing(dir.clone(), home.clone(), recent));
    ctx.prompt(spec, move |ctx, answer| {
        let Some(answer) = answer.filter(|a| !a.is_empty()) else {
            return Ok(());
        };
        let path = file::resolve(&dir, home.as_deref(), &answer);
        if path.is_dir() {
            let slash = if answer.ends_with('/') { "" } else { "/" };
            ask_for_board(ctx, dir, &format!("{answer}{slash}"));
            return Ok(());
        }
        show_board(ctx, &file::with_svg(&path))
    });
}

/// The candidates for the line: the boards and directories in the directory it names up to
/// its last `/`, kept until the line names another.
fn listing(
    dir: PathBuf,
    home: Option<PathBuf>,
    recent: Vec<String>,
) -> impl FnMut(&str) -> (usize, Vec<String>) {
    let mut kept: Option<(String, Vec<String>)> = None;
    move |line| {
        let base = line.rfind('/').map_or(0, |i| i + 1);
        let named = &line[..base];
        match &kept {
            Some((name, entries)) if name == named => (base, entries.clone()),
            _ => {
                let all = file::entries(&file::resolve(&dir, home.as_deref(), named));
                let (dirs, files): (Vec<String>, Vec<String>) =
                    all.into_iter().partition(|e| e.ends_with('/'));
                let mut entries = recent_first(&recent, files);
                entries.extend(dirs);
                kept = Some((named.to_string(), entries.clone()));
                (base, entries)
            }
        }
    }
}

/// `whiteboard [FILE]`: open `FILE`; or with none, from a buffer that isn't a board, the board
/// used last; or else ask `Board: `.
fn whiteboard(ctx: &mut Ctx, args: &Args) -> Result {
    let dir = here(ctx, args);
    let home = std::env::home_dir();
    if let Some(name) = args
        .text
        .as_deref()
        .map(str::trim)
        .filter(|t| !t.is_empty())
    {
        let path = file::with_svg(&file::resolve(&dir, home.as_deref(), name));
        return show_board(ctx, &path);
    }
    let focused = args.buffer.or_else(|| ctx.focused_buffer());
    let recent = ctx.with_ext(|wb: &mut Whiteboard, ctx| {
        let on_board = focused.is_some_and(|b| wb.boards.contains_key(&b));
        (!on_board).then(|| wb.most_recent(ctx)).flatten()
    })?;
    if let Some(buf) = recent {
        ctx.show(buf, ShowIn::Existing)?;
        return Ok(());
    }
    ask_for_board(ctx, dir.clone(), &file::line_for(&dir, home.as_deref()));
    Ok(())
}

/// `whiteboard-new`: ask `Board: ` whatever's open.
fn whiteboard_new(ctx: &mut Ctx, args: &Args) -> Result {
    let dir = here(ctx, args);
    let home = std::env::home_dir();
    ask_for_board(ctx, dir.clone(), &file::line_for(&dir, home.as_deref()));
    Ok(())
}

// ------------------------------------------------------------------ export

/// What a PNG job needs, moved to its thread.
struct PngJob {
    board: model::Board,
    look: Look,
    scale: f32,
    fonts: PathBuf,
}

impl PngJob {
    fn new(ctx: &Ctx, wb: &Whiteboard, st: &BoardState, background: Background) -> PngJob {
        let bg = match background {
            Background::Theme => Some(theme_bg(ctx)),
            Background::White => Some(Rgb(255, 255, 255)),
            Background::Transparent => None,
        };
        PngJob {
            board: st.board.clone(),
            look: Look {
                bg,
                ..wb.look(ctx, st)
            },
            scale: wb.options.png_scale,
            fonts: fonts::font_dir(wb.options.font_dir.as_deref()),
        }
    }

    /// Rasterize, encode and write to `out`. The size, or why not.
    fn write(&self, out: &Path) -> std::result::Result<(u32, u32), String> {
        let fonts = fonts::Fonts::load(&self.fonts).ok();
        let raster = canvas::rasterize(&self.board, &self.look, self.scale, fonts.as_ref())?;
        let bytes = export::encode_png(&raster)?;
        file::write_atomic(out, &bytes).map_err(|e| format!("{}: {e}", out.display()))?;
        Ok((raster.width, raster.height))
    }
}

/// Report a job's outcome on the board's status line and in the messages.
fn report(ctx: &mut Ctx, buf: BufferId, outcome: std::result::Result<String, String>) {
    let state = ctx
        .with_ext(|wb: &mut Whiteboard, _| wb.board(buf))
        .ok()
        .flatten();
    if let Some(state) = state {
        let mut st = state.borrow_mut();
        st.error = outcome.as_ref().err().cloned();
        st.rev += 1;
    }
    ctx.messages.push(match outcome {
        Ok(said) | Err(said) => said,
    });
}

/// `whiteboard-export [png|svg]`: `NAME.png` beside the file, at your scale on your background,
/// rasterized from the model in a job; or `NAME.export.svg`, the picture without the dialect's
/// attributes. Asked for if not given.
fn export_board(ctx: &mut Ctx, args: &Args) -> Result {
    let (buf, state) = target(ctx, args)?;
    match args.text.as_deref().map(str::trim) {
        Some("png") => export_png(ctx, buf, &state),
        Some("svg") => export_svg(ctx, buf, &state),
        Some(other) if !other.is_empty() => {
            Err(Error::Msg(format!("Export as png or svg, not {other}")))
        }
        _ => {
            let spec = PromptSpec::new("Export as: ")
                .candidates(["png", "svg"])
                .require_match();
            ctx.prompt(spec, move |ctx, answer| match answer.as_deref() {
                Some("png") => export_png(ctx, buf, &state),
                Some("svg") => export_svg(ctx, buf, &state),
                _ => Ok(()),
            });
            Ok(())
        }
    }
}

fn export_png(ctx: &mut Ctx, buf: BufferId, state: &Shared) -> Result {
    let (job, out) = {
        let st = state.borrow();
        let out = file::sibling(&st.path, ".png");
        let job = ctx.with_ext(|wb: &mut Whiteboard, ctx| {
            PngJob::new(ctx, wb, &st, wb.options.export_background)
        })?;
        (job, out)
    };
    let shown = out.clone();
    ctx.spawn(
        move || job.write(&out),
        move |ctx, done| {
            let outcome = done
                .map(|(w, h)| format!("Wrote {} ({w}×{h})", shown.display()))
                .map_err(|e| format!("Export failed: {e}"));
            report(ctx, buf, outcome);
            Ok(())
        },
    )
    .detach();
    Ok(())
}

fn export_svg(ctx: &mut Ctx, buf: BufferId, state: &Shared) -> Result {
    let (text, out) = {
        let st = state.borrow();
        let look = ctx.with_ext(|wb: &mut Whiteboard, ctx| wb.look(ctx, &st))?;
        (
            svg::write(&st.board, &look, false),
            file::sibling(&st.path, ".export.svg"),
        )
    };
    let outcome = file::write_atomic(&out, text.as_bytes())
        .map(|()| format!("Wrote {}", out.display()))
        .map_err(|e| format!("Export failed: {}: {e}", out.display()));
    report(ctx, buf, outcome);
    Ok(())
}

/// `whiteboard-copy-image`: the board as a PNG on the pasteboard, through `osascript` on macOS
/// and `wl-copy` or `xclip` on Linux, in a job.
fn copy_image(ctx: &mut Ctx, args: &Args) -> Result {
    let (buf, state) = target(ctx, args)?;
    let (job, tmp, name) = {
        let st = state.borrow();
        let (job, n) = ctx.with_ext(|wb: &mut Whiteboard, ctx| {
            wb.temps += 1;
            (
                PngJob::new(ctx, wb, &st, wb.options.export_background),
                wb.temps,
            )
        })?;
        let tmp =
            std::env::temp_dir().join(format!("die-whiteboard-{}-{n}.png", std::process::id()));
        (job, tmp, st.name())
    };
    ctx.spawn(
        move || {
            let (w, h) = job.write(&tmp)?;
            let path = die_core::process::path();
            let has = |p: &str| export::which(p, path);
            let mut cmd = export::pasteboard_command(&tmp, &has)
                .ok_or("no osascript, wl-copy or xclip on the PATH")?;
            let status = cmd
                .status()
                .map_err(|e| format!("{}: {e}", export::shown(&cmd)))?;
            let _ = std::fs::remove_file(&tmp);
            if !status.success() {
                return Err(format!("{} failed: {status}", export::shown(&cmd)));
            }
            Ok((w, h))
        },
        move |ctx, done| {
            let outcome = done
                .map(|(w, h)| format!("Copied {name} as a PNG ({w}×{h})"))
                .map_err(|e| format!("Copy failed: {e}"));
            report(ctx, buf, outcome);
            Ok(())
        },
    )
    .detach();
    Ok(())
}

/// `whiteboard-open`: the file in the browser, through `open` or `xdg-open`, saved first.
fn open_in_browser(ctx: &mut Ctx, args: &Args) -> Result {
    let (buf, state) = target(ctx, args)?;
    ctx.with_ext(|wb: &mut Whiteboard, ctx| wb.save_now(ctx, buf))??;
    let path = state.borrow().path.clone();
    ctx.spawn(
        move || {
            let has = |p: &str| export::which(p, die_core::process::path());
            let mut cmd =
                export::open_command(&path, &has).ok_or("no open or xdg-open on the PATH")?;
            cmd.status()
                .map(|_| ())
                .map_err(|e| format!("{}: {e}", export::shown(&cmd)))
        },
        move |ctx, done| {
            if let Err(e) = done {
                report(ctx, buf, Err(format!("Open failed: {e}")));
            }
            Ok(())
        },
    )
    .detach();
    Ok(())
}

// ------------------------------------------------------------------ keys

/// The tools' commands: name, key, tool, doc.
const TOOLS: [(&str, &str, Tool, &str); 7] = [
    (
        "whiteboard-tool-pen",
        "p",
        Tool::Pen,
        "p: the pen, which draws freehand.",
    ),
    (
        "whiteboard-tool-rect",
        "r",
        Tool::Rect,
        "r: the next drag draws a rectangle, a square with Shift; then the pen again.",
    ),
    (
        "whiteboard-tool-ellipse",
        "c",
        Tool::Ellipse,
        "c: the next drag draws an ellipse, a circle with Shift; then the pen again.",
    ),
    (
        "whiteboard-tool-arrow",
        "a",
        Tool::Arrow,
        "a: the next drag draws an arrow, along an axis with Shift, bound to the shapes at its ends; then the pen again.",
    ),
    (
        "whiteboard-tool-line",
        "",
        Tool::Line,
        "The next drag draws a line, along an axis with Shift, bound to the shapes at its ends; then the pen again. `l` with nothing selected.",
    ),
    (
        "whiteboard-tool-label",
        "t",
        Tool::Label,
        "t: the next click places a label and types into it until Esc or RET; then the pen again. A double click on a label edits it.",
    ),
    (
        "whiteboard-tool-select",
        "v",
        Tool::Select,
        "v: select: a click selects the element under the pointer, a drag a marquee, a drag on the selection moves it.",
    ),
];

/// die-vim's `C-w` keys, bound here as the text kind binds them, since a surface without text
/// doesn't inherit them.
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

/// Every command that doesn't change the board or need the prompt, as a table: name, doc, and
/// what it does to the state, with the clock, giving an effect or an error.
type Simple = fn(&mut BoardState, Duration) -> Result<Effect>;

fn nothing_selected() -> Error {
    Error::Msg("Nothing is selected".into())
}

fn zoom_message() -> Error {
    Error::Msg("Zoom comes with the pixel renderer (M3a); the cell renderer draws at 1:1".into())
}

fn nudge(dc: i32, dr: i32) -> impl Fn(&mut BoardState, Duration) -> Result<Effect> {
    move |s, now| {
        if s.nudge(dc, dr, now) {
            Ok(Effect::None)
        } else {
            Err(nothing_selected())
        }
    }
}

const SIMPLE: [(&str, &str, Simple); 19] = [
    (
        "whiteboard-eraser",
        "e: the eraser, on or off: a drag erases every element it crosses. The right button erases from any tool.",
        |s, _| {
            s.toggle_tool(Tool::Eraser);
            Ok(Effect::None)
        },
    ),
    (
        "whiteboard-left",
        "h: nudge the selection a cell left; with nothing selected, the highlighter, on or off (the pen at 16 px and 35 %).",
        |s, now| {
            if !s.nudge(-1, 0, now) {
                s.toggle_tool(Tool::Highlighter);
            }
            Ok(Effect::None)
        },
    ),
    (
        "whiteboard-down",
        "j: nudge the selection a cell down.",
        |s, now| nudge(0, 1)(s, now),
    ),
    (
        "whiteboard-up",
        "k: nudge the selection a cell up.",
        |s, now| nudge(0, -1)(s, now),
    ),
    (
        "whiteboard-right",
        "l: nudge the selection a cell right; with nothing selected, the line tool: the next drag draws a line, along an axis with Shift, bound to the shapes at its ends; then the pen again.",
        |s, now| {
            if !s.nudge(1, 0, now) {
                s.set_tool(Tool::Line);
            }
            Ok(Effect::None)
        },
    ),
    (
        "whiteboard-delete",
        "x, Del: delete the selection.",
        |s, now| {
            if s.delete_selection(now) {
                Ok(Effect::None)
            } else {
                Err(nothing_selected())
            }
        },
    ),
    (
        "whiteboard-duplicate",
        "y: copy the selection a cell down and right, and select the copies.",
        |s, now| {
            if s.duplicate(now) {
                Ok(Effect::None)
            } else {
                Err(nothing_selected())
            }
        },
    ),
    (
        "whiteboard-escape",
        "Esc: drop the selection and whatever the pointer was doing, back to the pen.",
        |s, _| {
            s.escape();
            Ok(Effect::None)
        },
    ),
    (
        "whiteboard-thicker",
        "]: the next width of three (2, 4, 8 px), for what's drawn next and the selection.",
        |s, now| {
            s.step_width(1, now);
            Ok(Effect::None)
        },
    ),
    (
        "whiteboard-thinner",
        "[: the last width of three (2, 4, 8 px), for what's drawn next and the selection.",
        |s, now| {
            s.step_width(-1, now);
            Ok(Effect::None)
        },
    ),
    (
        "whiteboard-undo",
        "u, Cmd-Z: take back the last gesture, command or label.",
        |s, now| {
            if s.undo(now) {
                Ok(Effect::None)
            } else {
                Err(Error::NothingToUndo)
            }
        },
    ),
    (
        "whiteboard-redo",
        "C-r, Cmd-Shift-Z: do the last undone change again.",
        |s, now| {
            if s.redo(now) {
                Ok(Effect::None)
            } else {
                Err(Error::NothingToUndo)
            }
        },
    ),
    (
        "whiteboard-zoom-in",
        "+: zoom in, about the area's centre (the pixel renderer; the cell renderer draws at 1:1).",
        |s, _| {
            if !s.zooms {
                return Err(zoom_message());
            }
            s.zoom(ZOOM_STEP, None);
            Ok(Effect::None)
        },
    ),
    (
        "whiteboard-zoom-out",
        "-: zoom out, about the area's centre (the pixel renderer; the cell renderer draws at 1:1).",
        |s, _| {
            if !s.zooms {
                return Err(zoom_message());
            }
            s.zoom(1.0 / ZOOM_STEP, None);
            Ok(Effect::None)
        },
    ),
    ("whiteboard-zoom-reset", "0: no zoom and no pan.", |s, _| {
        s.reset_view();
        Ok(Effect::None)
    }),
    (
        "whiteboard-label-end",
        "Esc, RET while a label is typed: end it; it stays if it has text, and the pen comes back.",
        |s, now| Ok(s.end_label(now)),
    ),
    (
        "whiteboard-label-newline",
        "S-RET while a label is typed: a new line.",
        |s, now| {
            s.label_type("\n", now);
            Ok(Effect::None)
        },
    ),
    (
        "whiteboard-label-backspace",
        "BS while a label is typed: take the last character back.",
        |s, now| {
            s.label_backspace(now);
            Ok(Effect::None)
        },
    ),
    (
        "whiteboard-highlighter",
        "The highlighter, on or off: the pen at 16 px and 35 % opacity.",
        |s, _| {
            s.toggle_tool(Tool::Highlighter);
            Ok(Effect::None)
        },
    ),
];

/// `whiteboard-ink`: the ink the key names, `1`..`6`, or `:whiteboard-ink N`.
fn set_ink(ctx: &mut Ctx, args: &Args) -> Result {
    let digit = args
        .text
        .as_deref()
        .map(str::trim)
        .and_then(|t| t.parse::<u8>().ok())
        .or_else(|| args.char().and_then(|c| c.to_digit(10)).map(|d| d as u8));
    let ink = digit
        .filter(|d| (1..=INKS).contains(d))
        .ok_or_else(|| Error::Msg(format!("An ink is 1 to {INKS}")))?;
    change(ctx, args, |s, now| {
        s.set_ink(ink, now);
        Ok(Effect::None)
    })
}

/// `whiteboard-tool TOOL`: a tool by name, for `M-x`.
fn set_tool(ctx: &mut Ctx, args: &Args) -> Result {
    let name = args.text.as_deref().map(str::trim).unwrap_or("");
    let tool = Tool::from_name(name).ok_or_else(|| {
        let names: Vec<&str> = Tool::ALL.iter().map(|t| t.name()).collect();
        Error::Msg(format!("A tool is one of {}", names.join(", ")))
    })?;
    change(ctx, args, |s, _| {
        s.set_tool(tool);
        Ok(Effect::None)
    })
}

// ------------------------------------------------------------------ the session

/// What a restart keeps of a board.
fn kept_board(ctx: &Ctx, buf: BufferId, st: &BoardState) -> Value {
    let shown = ctx
        .window_list()
        .iter()
        .any(|w| ctx.window_buffer(*w) == Some(buf));
    json!({
        "file": st.path.to_string_lossy(),
        "pan": [st.view.pan.0, st.view.pan.1],
        "zoom": st.view.zoom,
        "tool": st.tool.name(),
        "ink": st.ink,
        "width": st.width,
        "selection": st.selection,
        "shown": shown,
    })
}

/// Put what a restart kept back on a board.
fn take_kept(st: &mut BoardState, v: &Value) {
    let num = |k: &str| v.get(k).and_then(Value::as_f64);
    if let Some(pan) = v.get("pan").and_then(Value::as_array)
        && let [x, y] = pan.as_slice()
        && let (Some(x), Some(y)) = (x.as_f64(), y.as_f64())
    {
        st.view.pan = (x as f32, y as f32);
    }
    if let Some(z) = num("zoom") {
        st.view = View {
            zoom: (z as f32).clamp(view::ZOOM_MIN, view::ZOOM_MAX),
            ..st.view
        };
    }
    if let Some(t) = v
        .get("tool")
        .and_then(Value::as_str)
        .and_then(Tool::from_name)
    {
        st.tool = if t == Tool::Label { Tool::Pen } else { t };
    }
    if let Some(i) = num("ink") {
        st.ink = (i as u8).clamp(1, INKS);
    }
    if let Some(w) = num("width") {
        st.width = (w as u8).min((model::WIDTHS.len() - 1) as u8);
    }
    if let Some(sel) = v.get("selection").and_then(Value::as_array) {
        let ids = sel.iter().filter_map(Value::as_u64).map(|i| i as Id);
        st.selection = ids.filter(|id| st.board.get(*id).is_some()).collect();
    }
    st.redraw();
}

impl Extension for Whiteboard {
    fn name(&self) -> &'static str {
        "whiteboard"
    }

    fn register(&mut self, reg: &mut Registry) {
        reg.face(face::INK[0], Some("default"), Style::new());
        for name in &face::INK[1..] {
            reg.face(name, None, Style::new());
        }
        reg.command(
            "whiteboard",
            "Open a whiteboard: the file given, or from a buffer that isn't a board the board used last, or else ask which.",
            whiteboard,
        );
        reg.command(
            "whiteboard-new",
            "Ask which board to open, `Board: `.",
            whiteboard_new,
        );
        reg.command(
            "whiteboard-save",
            "S: write the board's file now. Boards are written a second after each change anyway.",
            |ctx, a| {
                let (buf, _) = target(ctx, a)?;
                ctx.with_ext(|wb: &mut Whiteboard, ctx| wb.save_now(ctx, buf))??;
                let name = ctx.buffer_name(buf).unwrap_or_default();
                ctx.messages.push(format!("Wrote {name}"));
                Ok(())
            },
        );
        reg.command(
            "whiteboard-export",
            "E: export the board as png (NAME.png beside the file, from the model, at your scale) or svg (NAME.export.svg); asked which if not given.",
            export_board,
        );
        reg.command(
            "whiteboard-copy-image",
            "C: put the board on the pasteboard as a PNG.",
            copy_image,
        );
        reg.command(
            "whiteboard-open",
            "Save the board and open its file in the browser.",
            open_in_browser,
        );
        reg.command(
            "whiteboard-ink",
            "1 to 6: the ink, for what's drawn next and the selection.",
            set_ink,
        );
        reg.command(
            "whiteboard-tool",
            "A tool by name: pen, highlighter, eraser, rect, ellipse, line, arrow, text, select.",
            set_tool,
        );
        for (name, _, tool, doc) in TOOLS {
            reg.command(name, doc, move |ctx, a| {
                change(ctx, a, |s, _| {
                    s.set_tool(tool);
                    Ok(Effect::None)
                })
            });
        }
        for (name, doc, f) in SIMPLE {
            reg.command(name, doc, move |ctx, a| change(ctx, a, f));
        }
        use State::{Insert, Normal};
        let k = reg.kind_keymap(KIND);
        for (name, key, ..) in TOOLS {
            if !key.is_empty() {
                k.bind(Normal, key, name);
            }
        }
        k.bind(Normal, "e", "whiteboard-eraser")
            .bind(Normal, "h", "whiteboard-left")
            .bind(Normal, "j", "whiteboard-down")
            .bind(Normal, "k", "whiteboard-up")
            .bind(Normal, "l", "whiteboard-right")
            .bind(Normal, "x", "whiteboard-delete")
            .bind(Normal, "<Del>", "whiteboard-delete")
            .bind(Normal, "y", "whiteboard-duplicate")
            .bind(Normal, "<Esc>", "whiteboard-escape")
            .bind(Normal, "[", "whiteboard-thinner")
            .bind(Normal, "]", "whiteboard-thicker")
            .bind(Normal, "u", "whiteboard-undo")
            .bind(Normal, "<C-r>", "whiteboard-redo")
            .bind(Normal, "+", "whiteboard-zoom-in")
            .bind(Normal, "=", "whiteboard-zoom-in")
            .bind(Normal, "-", "whiteboard-zoom-out")
            .bind(Normal, "0", "whiteboard-zoom-reset")
            .bind(Normal, "E", "whiteboard-export")
            .bind(Normal, "C", "whiteboard-copy-image")
            .bind(Normal, "S", "whiteboard-save")
            .bind(Normal, ":", "vim-ex")
            .bind(Normal, "<leader>z", "toggle-max-width")
            .bind(Insert, "<Esc>", "whiteboard-label-end")
            .bind(Insert, "<CR>", "whiteboard-label-end")
            .bind(Insert, "<S-CR>", "whiteboard-label-newline")
            .bind(Insert, "<BS>", "whiteboard-label-backspace");
        for d in 1..=INKS {
            k.bind(Normal, &d.to_string(), "whiteboard-ink");
        }
        for state in [Normal, Insert] {
            k.bind(state, "<D-z>", "whiteboard-undo")
                .bind(state, "<D-S-z>", "whiteboard-redo");
        }
        for (keys, name) in WINDOW_KEYS {
            for key in keys.split_whitespace() {
                k.bind(Normal, &format!("<C-w>{key}"), name);
            }
        }
    }

    fn init(&mut self, ctx: &mut Ctx) -> Result {
        let killed =
            |ctx: &mut Ctx, buf| Ok(ctx.with_ext(|wb: &mut Whiteboard, ctx| wb.killed(ctx, buf))?);
        ctx.on_buffer_killed(killed).detach();
        Ok(())
    }

    /// Each open board: its file, view, tool, ink, width and selection, with its file written
    /// first if it's behind, so a restart finds it on disk. Not keyed by the buffer: core keeps
    /// buffer entries only for buffers it saves itself, and a surface's isn't one.
    fn save_session(&self, ctx: &Ctx) -> Kept {
        let mut boards = vec![];
        for buf in ctx.buffer_list() {
            let Some(state) = self.boards.get(&buf).filter(|_| ctx.is_live(buf)) else {
                continue;
            };
            let mut st = state.borrow_mut();
            if st.dirty {
                let _ = write_board(ctx, &mut st);
            }
            boards.push(kept_board(ctx, buf, &st));
        }
        Kept {
            state: if boards.is_empty() {
                Value::Null
            } else {
                json!({ "boards": boards })
            },
            buffers: vec![],
        }
    }

    /// Open each kept board from its file, the one used last shown in this window (a stand-in
    /// for its own window, until M1's surface restorer), the rest open in the background.
    fn restore_session(&mut self, ctx: &mut Ctx, kept: Kept) -> Result {
        let boards = kept
            .state
            .get("boards")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let mut show = None;
        for record in boards.iter().rev() {
            let Some(path) = record.get("file").and_then(Value::as_str) else {
                continue;
            };
            let path = PathBuf::from(path);
            let buf = match self.open(ctx, &path) {
                Ok(buf) => buf,
                Err(e) => {
                    ctx.messages.push(format!(
                        "whiteboard: {} didn't come back: {e}",
                        path.display()
                    ));
                    continue;
                }
            };
            if let Some(state) = self.boards.get(&buf) {
                take_kept(&mut state.borrow_mut(), record);
            }
            if record.get("shown").and_then(Value::as_bool) == Some(true) {
                show = Some(buf);
            }
        }
        if let Some(buf) = show {
            ctx.show(buf, ShowIn::Current)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_defaults() {
        let o = Options::default();
        assert_eq!(o.png_scale, 2.0);
        assert_eq!(o.export_background, Background::Theme);
        assert!(!o.full_width);
        assert_eq!(o.autosave, Duration::from_secs(1));
        assert_eq!(o.font_dir, None);
        assert_eq!(Whiteboard::for_tests().options.png_scale, 1.0);
    }

    #[test]
    fn every_tool_has_a_name_that_reads_back() {
        for t in Tool::ALL {
            assert_eq!(Tool::from_name(t.name()), Some(t));
        }
        assert!(Tool::Rect.one_shot() && !Tool::Pen.one_shot() && !Tool::Select.one_shot());
    }
}
