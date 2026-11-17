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
//! - [`dial`]: the tool dial, Space or the right button held and a direction;
//! - [`render`], [`pixels`] and [`cells`]: the renderer trait, the pixel renderer (tiny-skia into
//!   a pixel buffer placed under the area's glyphs, M3a), and the cell renderer for a host that
//!   shows no pixels;
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
pub mod dial;
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
use model::{Board, INKS, Id};
use std::cell::RefCell;
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
    /// Draw boards in pixels (anti-aliased ink at the display's resolution, zoom) where the host
    /// shows them; false draws every board in cells, as a host without pixels does.
    pub pixels: bool,
}

impl Default for Options {
    /// DIE's font rule, PNGs at 2x on the theme's background, a file's width, a second, pixels.
    fn default() -> Options {
        Options {
            font_dir: None,
            png_scale: 2.0,
            export_background: Background::Theme,
            full_width: false,
            autosave: Duration::from_secs(1),
            pixels: true,
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
    /// Each scratch board's picture as the session last kept it, by the revision of the
    /// picture it was written at: core looks at the session twice a second while you work, and
    /// writing out a big board each time would cost milliseconds.
    kept: RefCell<BTreeMap<BufferId, (u64, String)>>,
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
            kept: RefCell::default(),
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
            .find(|(_, s)| s.borrow().path.as_deref() == Some(path))
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
        let dir = path.parent().filter(|d| !d.as_os_str().is_empty());
        let state = BoardState::new(path.to_path_buf(), board);
        Ok(self.add(ctx, state, dir))
    }

    /// A new scratch board in `dir` with `board`'s elements, in no file: `*whiteboard:
    /// scratch*`, or with that taken the lowest number free, `*whiteboard: scratch 2*`. Number
    /// `n` if it's free, as a restart asks.
    pub fn scratch(&mut self, ctx: &mut Ctx, dir: &Path, board: Board, n: Option<u32>) -> BufferId {
        let free = |n: &u32| *n >= 1 && ctx.find_buffer(&scratch_buffer(*n)).is_none();
        let n = n.filter(free).or_else(|| (1..).find(free)).unwrap_or(1);
        self.add(ctx, BoardState::scratch(n, board), Some(dir))
    }

    /// A buffer for the board, in `dir`.
    fn add(&mut self, ctx: &mut Ctx, mut state: BoardState, dir: Option<&Path>) -> BufferId {
        state.inks = ink_colors(ctx);
        state.fg = default_fg(ctx);
        state.cell = ctx.cell_pixels();
        let name = file::titled(&state.name());
        let state = state.shared();
        // Whether the host shows pixels is asked at the surface's first event: a board restored
        // at startup is opened before the host connects its sink.
        let surface = BoardSurface::new(state.clone(), self.options.pixels);
        let buf = ctx.create_surface(&name, surface);
        state.borrow_mut().buf = Some(buf);
        if let Some(dir) = dir {
            ctx.set_directory(buf, dir);
        }
        if self.options.full_width {
            let _ = ctx.change_settings(buf, |s| s.max_width = None);
        }
        self.boards.retain(|b, _| ctx.is_live(*b));
        self.boards.insert(buf, state);
        buf
    }

    /// Write `buf`'s board now, whether or not it changed. A scratch board has no file to write.
    pub fn save_now(&self, ctx: &Ctx, buf: BufferId) -> Result {
        let state = self.board(buf).ok_or(not_a_board())?;
        let mut st = state.borrow_mut();
        if st.path.is_none() {
            return Err(Error::Msg(
                "A scratch board has no file; S saves it as one".into(),
            ));
        }
        write_board(ctx, &mut st).map_err(Error::Msg)
    }

    /// The file `whiteboard-open` shows: a file board's, written now, or a scratch board's
    /// picture written to a temporary file.
    pub fn file_to_open(&mut self, ctx: &Ctx, buf: BufferId) -> Result<PathBuf> {
        let state = self.board(buf).ok_or(not_a_board())?;
        let path = state.borrow().path.clone();
        if let Some(path) = path {
            self.save_now(ctx, buf)?;
            return Ok(path);
        }
        self.temps += 1;
        let name = format!("die-whiteboard-{}-{}.svg", std::process::id(), self.temps);
        let tmp = std::env::temp_dir().join(name);
        let st = state.borrow();
        let text = svg::write(&st.board, &self.look(ctx, &st), true);
        file::write_atomic(&tmp, text.as_bytes())
            .map_err(|e| Error::Msg(format!("{}: {e}", tmp.display())))?;
        Ok(tmp)
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
    /// behind, no timer. A scratch board is never behind: killing it discards it.
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

/// Scratch board `n`'s buffer name: `*whiteboard: scratch 2*`.
fn scratch_buffer(n: u32) -> String {
    file::titled(&file::scratch_name(n))
}

/// Write a board's file, and say in its status whether that went. A scratch board has none:
/// nothing is written.
fn write_board(ctx: &Ctx, st: &mut BoardState) -> std::result::Result<(), String> {
    let Some(path) = st.path.clone() else {
        return Ok(());
    };
    let look = Look {
        inks: ink_rgbs(ctx),
        bg: Some(theme_bg(ctx)),
        cell: st.cell_size(),
    };
    let wrote = file::save(&path, &st.board, &look);
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
            Err(format!("{}: {e}", path.display()))
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
    state
        .borrow_mut()
        .set_look(ink_colors(ctx), default_fg(ctx));
    Ok((buf, state))
}

fn apply_effect(ctx: &mut Ctx, buf: BufferId, effect: Effect) {
    match effect {
        Effect::None => {}
        Effect::EnterInsert => ctx.set_state_of(buf, State::Insert),
        Effect::LeaveInsert => ctx.set_state_of(buf, State::Normal),
    }
}

/// Change the board a command acts on with `f`, then set the autosave timer if it's behind. A
/// key that reaches a command closes the tool dial first, as any key but Space does.
fn change(
    ctx: &mut Ctx,
    args: &Args,
    f: impl FnOnce(&mut BoardState, Duration) -> Result<Effect>,
) -> Result {
    let (buf, state) = target(ctx, args)?;
    let now = ctx.now();
    let effect = {
        let mut st = state.borrow_mut();
        st.cancel_dial();
        f(&mut st, now)?
    };
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

/// A new scratch board in the directory the command was given, shown in this window.
fn show_scratch(ctx: &mut Ctx, args: &Args) -> Result {
    let dir = here(ctx, args);
    let buf =
        ctx.with_ext(|wb: &mut Whiteboard, ctx| wb.scratch(ctx, &dir, Board::default(), None))?;
    ctx.show(buf, ShowIn::Current)?;
    Ok(())
}

/// One of the prompts that ask for a path: its label, the history it keeps, and what it lists of
/// a directory.
#[derive(Clone, Copy)]
struct Asking {
    label: &'static str,
    history: &'static str,
    list: fn(&Path) -> Vec<String>,
}

/// `Board: `, a board to open or start.
const BOARD: Asking = Asking {
    label: "Board: ",
    history: "board",
    list: file::entries,
};

/// `Save board as: `, a scratch board's file. Its answers are boards too.
const SAVE_AS: Asking = Asking {
    label: "Save board as: ",
    history: "board",
    list: file::entries,
};

/// `Export to: `, a PNG or an SVG.
const EXPORT: Asking = Asking {
    label: "Export to: ",
    history: "export",
    list: file::exports,
};

/// What a path prompt does with the path answered.
type Answered = Box<dyn FnOnce(&mut Ctx, PathBuf) -> Result>;

/// Ask for a path from `line`: the prompt descends directories like `\f`, lists what `asking`
/// lists, the names answered last first, and takes a new name with `<M-CR>`. Answering with a
/// directory asks again from there; with anything else, `done` has the path. Nothing typed, or a
/// cancel, does nothing.
fn ask_path(ctx: &mut Ctx, asking: Asking, dir: PathBuf, line: &str, done: Answered) {
    let home = std::env::home_dir();
    // What was answered, as the file it named: its last part, with `.svg` as a board got it.
    let recent: Vec<String> = ctx
        .prompt_history(asking.history)
        .iter()
        .map(|h| {
            let name = h.rsplit('/').next().unwrap_or(h);
            file::with_svg(Path::new(name))
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    let spec = PromptSpec::new(asking.label)
        .initial(line)
        .history(asking.history)
        .descends()
        .candidates_from(listing(dir.clone(), home.clone(), recent, asking.list));
    ctx.prompt(spec, move |ctx, answer| {
        let Some(answer) = answer.filter(|a| !a.is_empty()) else {
            return Ok(());
        };
        let path = file::resolve(&dir, home.as_deref(), &answer);
        if path.is_dir() {
            let slash = if answer.ends_with('/') { "" } else { "/" };
            ask_path(ctx, asking, dir, &format!("{answer}{slash}"), done);
            return Ok(());
        }
        done(ctx, path)
    });
}

/// The `Board: ` prompt from `line`, in `dir`: a name without an extension gets `.svg`.
fn ask_for_board(ctx: &mut Ctx, dir: PathBuf, line: &str) {
    let open = |ctx: &mut Ctx, path: PathBuf| show_board(ctx, &file::with_svg(&path));
    ask_path(ctx, BOARD, dir, line, Box::new(open));
}

/// The candidates for the line: what `list` finds in the directory it names up to its last
/// `/`, files then directories, kept until the line names another.
fn listing(
    dir: PathBuf,
    home: Option<PathBuf>,
    recent: Vec<String>,
    list: fn(&Path) -> Vec<String>,
) -> impl FnMut(&str) -> (usize, Vec<String>) {
    let mut kept: Option<(String, Vec<String>)> = None;
    move |line| {
        let base = line.rfind('/').map_or(0, |i| i + 1);
        let named = &line[..base];
        match &kept {
            Some((name, entries)) if name == named => (base, entries.clone()),
            _ => {
                let all = list(&file::resolve(&dir, home.as_deref(), named));
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

/// Run `then`, once you've said `y` to overwriting `path` if a file is there, as Emacs asks
/// before `write-file` replaces one.
fn unless_kept(
    ctx: &mut Ctx,
    path: &Path,
    then: impl FnOnce(&mut Ctx) -> Result + 'static,
) -> Result {
    if !path.exists() {
        return then(ctx);
    }
    let home = std::env::home_dir();
    let shown = file::abbreviate(path, home.as_deref());
    let label = format!("File {shown} exists; overwrite? (y or n) ");
    ctx.prompt(PromptSpec::new(&label), move |ctx, answer| {
        match answer.as_deref() {
            Some("y" | "yes") => then(ctx),
            _ => Ok(()),
        }
    });
    Ok(())
}

/// The directory a board's prompts start in: its buffer's.
fn board_dir(ctx: &Ctx, buf: BufferId) -> PathBuf {
    let dir = ctx.directory(buf).map(Path::to_path_buf);
    dir.unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| "/".into()))
}

/// `Save board as: ` for scratch board `buf`, from its directory, listing boards: a name without
/// an extension gets `.svg`, a file there already is asked about, and a path another open board
/// has is refused. Then `close`'s window closes, for `ZZ`.
fn ask_save_as(ctx: &mut Ctx, buf: BufferId, close: Option<Args>) {
    let dir = board_dir(ctx, buf);
    let line = file::line_for(&dir, std::env::home_dir().as_deref());
    let save = move |ctx: &mut Ctx, path: PathBuf| {
        let path = file::with_svg(&path);
        let other = ctx.with_ext(|wb: &mut Whiteboard, ctx| {
            wb.board_on(&path).filter(|b| *b != buf && ctx.is_live(*b))
        })?;
        if other.is_some() {
            let name = file::name_of(&path);
            return Err(Error::Msg(format!(
                "{name} is open in another board; save this one elsewhere"
            )));
        }
        let at = path.clone();
        unless_kept(ctx, &at, move |ctx| {
            save_as(ctx, buf, path)?;
            match close {
                Some(args) => ctx.run("vim-quit", &args),
                None => Ok(()),
            }
        })
    };
    ask_path(ctx, SAVE_AS, dir, &line, Box::new(save));
}

/// Write scratch board `buf` to `path`, and make it that file's board from now on: the buffer
/// named for the file, in the file's directory, written a second after each change.
fn save_as(ctx: &mut Ctx, buf: BufferId, path: PathBuf) -> Result {
    let state = ctx.with_ext(|wb: &mut Whiteboard, _| wb.board(buf))?;
    let state = state.filter(|_| ctx.is_live(buf)).ok_or_else(not_a_board)?;
    {
        let mut st = state.borrow_mut();
        let was = st.path.replace(path.clone());
        if let Err(e) = write_board(ctx, &mut st) {
            st.path = was;
            return Err(Error::Msg(e));
        }
    }
    ctx.rename_buffer(buf, &file::buffer_name(&path));
    if let Some(dir) = path.parent() {
        ctx.set_directory(buf, dir);
    }
    ctx.messages.push(format!("Wrote {}", path.display()));
    Ok(())
}

/// `whiteboard [FILE]`: open `FILE`; or with none, from a buffer that isn't a board, the board
/// used last; or else, and from a board, a new scratch board.
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
    show_scratch(ctx, args)
}

/// `whiteboard-find`: ask `Board: `, to open a board's file or start one.
fn whiteboard_find(ctx: &mut Ctx, args: &Args) -> Result {
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

/// What an export writes, by the file's extension.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Format {
    /// A PNG, rasterized from the model.
    Png,
    /// The picture as SVG, without the dialect's attributes.
    Svg,
}

impl Format {
    /// What `path`'s extension names, if it's one of the two.
    fn of(path: &Path) -> Option<Format> {
        match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
            "png" => Some(Format::Png),
            "svg" => Some(Format::Svg),
            _ => None,
        }
    }

    /// What an export beside a file ends in: `arch.png`, `arch.export.svg`.
    fn suffix(self) -> &'static str {
        match self {
            Format::Png => ".png",
            Format::Svg => ".export.svg",
        }
    }
}

/// `whiteboard-export [png|svg]`: with one named, on a file board, `NAME.png` beside the file
/// (at your scale on your background, rasterized from the model in a job) or `NAME.export.svg`,
/// the picture without the dialect's attributes. Otherwise it asks `Export to: `, from where
/// this board last exported, or beside its file, or `board.png` in a scratch board's directory;
/// the answer's extension says which it writes.
fn export_board(ctx: &mut Ctx, args: &Args) -> Result {
    let (buf, state) = target(ctx, args)?;
    let named = match args.text.as_deref().map(str::trim) {
        Some("png") => Some(Format::Png),
        Some("svg") => Some(Format::Svg),
        Some(other) if !other.is_empty() => {
            return Err(Error::Msg(format!("Export as png or svg, not {other}")));
        }
        _ => None,
    };
    let (path, last) = {
        let st = state.borrow();
        (st.path.clone(), st.exported.clone())
    };
    if let (Some(format), Some(path)) = (named, &path) {
        return export_to(ctx, buf, &state, file::sibling(path, format.suffix()));
    }
    let dir = board_dir(ctx, buf);
    let suffix = named.unwrap_or(Format::Png).suffix();
    let default = match (last, &path) {
        (Some(last), _) if named.is_none_or(|f| Format::of(&last) == Some(f)) => last,
        (_, Some(path)) => file::sibling(path, suffix),
        (_, None) => file::free_export(&dir, suffix),
    };
    let line = file::abbreviate(&default, std::env::home_dir().as_deref());
    ask_export(ctx, buf, state, dir, &line);
    Ok(())
}

/// `Export to: ` from `line`, listing PNGs, SVGs and directories: the extension says what's
/// written, `.png` or `.svg`, and nothing else is; a board's own file is refused; a file there
/// already is asked about, unless this board exported to it last.
fn ask_export(ctx: &mut Ctx, buf: BufferId, state: Shared, dir: PathBuf, line: &str) {
    let export = move |ctx: &mut Ctx, path: PathBuf| {
        let name = file::name_of(&path);
        if Format::of(&path).is_none() {
            return Err(Error::Msg(format!(
                "Export to a .png or an .svg, not {name}"
            )));
        }
        let board = ctx
            .with_ext(|wb: &mut Whiteboard, ctx| wb.board_on(&path).filter(|b| ctx.is_live(*b)))?;
        if board.is_some() {
            return Err(Error::Msg(format!(
                "{name} is a board's own file; export to another"
            )));
        }
        if state.borrow().exported.as_deref() == Some(path.as_path()) {
            return export_to(ctx, buf, &state, path);
        }
        let at = path.clone();
        unless_kept(ctx, &at, move |ctx| export_to(ctx, buf, &state, path))
    };
    ask_path(ctx, EXPORT, dir, line, Box::new(export));
}

/// Export the board to `out`, as its extension says.
fn export_to(ctx: &mut Ctx, buf: BufferId, state: &Shared, out: PathBuf) -> Result {
    match Format::of(&out) {
        Some(Format::Svg) => export_svg(ctx, buf, state, out),
        _ => export_png(ctx, buf, state, out),
    }
}

/// Report an export's outcome, and if it went, where: the next export starts there.
fn report_export(
    ctx: &mut Ctx,
    buf: BufferId,
    out: PathBuf,
    outcome: std::result::Result<String, String>,
) {
    if outcome.is_ok()
        && let Ok(Some(state)) = ctx.with_ext(|wb: &mut Whiteboard, _| wb.board(buf))
    {
        state.borrow_mut().exported = Some(out);
    }
    report(ctx, buf, outcome);
}

fn export_png(ctx: &mut Ctx, buf: BufferId, state: &Shared, out: PathBuf) -> Result {
    let job = {
        let st = state.borrow();
        ctx.with_ext(|wb: &mut Whiteboard, ctx| {
            PngJob::new(ctx, wb, &st, wb.options.export_background)
        })?
    };
    let shown = out.clone();
    ctx.spawn(
        move || job.write(&out),
        move |ctx, done| {
            let outcome = done
                .map(|(w, h)| format!("Wrote {} ({w}×{h})", shown.display()))
                .map_err(|e| format!("Export failed: {e}"));
            report_export(ctx, buf, shown, outcome);
            Ok(())
        },
    )
    .detach();
    Ok(())
}

fn export_svg(ctx: &mut Ctx, buf: BufferId, state: &Shared, out: PathBuf) -> Result {
    let text = {
        let st = state.borrow();
        let look = ctx.with_ext(|wb: &mut Whiteboard, ctx| wb.look(ctx, &st))?;
        svg::write(&st.board, &look, false)
    };
    let outcome = file::write_atomic(&out, text.as_bytes())
        .map(|()| format!("Wrote {}", out.display()))
        .map_err(|e| format!("Export failed: {}: {e}", out.display()));
    report_export(ctx, buf, out, outcome);
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

/// `whiteboard-open`: the file in the browser, through `open` or `xdg-open`, saved first; a
/// scratch board's picture, written to a temporary file.
fn open_in_browser(ctx: &mut Ctx, args: &Args) -> Result {
    let (buf, _) = target(ctx, args)?;
    let path = ctx.with_ext(|wb: &mut Whiteboard, ctx| wb.file_to_open(ctx, buf))??;
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
    Error::Msg("This board draws in cells, at 1:1; zoom needs the pixel renderer".into())
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
        "e: the eraser, on or off: a drag erases every element it crosses. It's west on the tool dial (Space or the right button held) too.",
        |s, now| {
            s.toggle_tool(Tool::Eraser, now);
            Ok(Effect::None)
        },
    ),
    (
        "whiteboard-left",
        "h: nudge the selection a cell left; with nothing selected, the highlighter, on or off (the pen at 16 px and 35 %).",
        |s, now| {
            if !s.nudge(-1, 0, now) {
                s.toggle_tool(Tool::Highlighter, now);
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
                s.set_tool(Tool::Line, now);
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
        "Esc: drop the selection and take back whatever the pointer was doing, back to the pen.",
        |s, now| {
            s.escape(now);
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
        "+: zoom in, about the area's centre (in pixels; a board drawn in cells is at 1:1).",
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
        "-: zoom out, about the area's centre (in pixels; a board drawn in cells is at 1:1).",
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
        |s, now| {
            s.toggle_tool(Tool::Highlighter, now);
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
    change(ctx, args, |s, now| {
        s.set_tool(tool, now);
        Ok(Effect::None)
    })
}

// ------------------------------------------------------------------ the session

/// What a restart keeps of a board: its file, or a scratch board's number, directory and
/// picture in the dialect; and its view, tool, ink, width, selection and last export.
fn kept_board(ctx: &Ctx, buf: BufferId, st: &BoardState, svg: impl FnOnce() -> String) -> Value {
    let shown = ctx
        .window_list()
        .iter()
        .any(|w| ctx.window_buffer(*w) == Some(buf));
    let mut kept = json!({
        "pan": [st.view.pan.0, st.view.pan.1],
        "zoom": st.view.zoom,
        "tool": st.tool.name(),
        "ink": st.ink,
        "width": st.width,
        "selection": st.selection,
        "shown": shown,
    });
    match &st.path {
        Some(path) => kept["file"] = json!(path.to_string_lossy()),
        None => {
            kept["scratch"] = json!(st.scratch);
            kept["dir"] = json!(ctx.directory(buf).map(Path::to_string_lossy));
            kept["svg"] = json!(svg());
        }
    }
    if let Some(out) = &st.exported {
        kept["exported"] = json!(out.to_string_lossy());
    }
    kept
}

/// Open the board a restart kept: its file's, or a scratch board from its picture.
fn reopen(
    wb: &mut Whiteboard,
    ctx: &mut Ctx,
    kept: &Value,
) -> std::result::Result<BufferId, String> {
    let text = |k: &str| kept.get(k).and_then(Value::as_str);
    if let Some(path) = text("file") {
        let path = Path::new(path);
        return wb
            .open(ctx, path)
            .map_err(|e| format!("whiteboard: {} didn't come back: {e}", path.display()));
    }
    let n = kept
        .get("scratch")
        .and_then(Value::as_u64)
        .map(|n| n as u32);
    let name = file::scratch_name(n.unwrap_or(1));
    let board = svg::read(text("svg").unwrap_or_default())
        .map_err(|e| format!("whiteboard: {name} didn't come back: {e}"))?;
    let started = || std::env::current_dir().unwrap_or_else(|_| "/".into());
    let dir = text("dir").map_or_else(started, PathBuf::from);
    Ok(wb.scratch(ctx, &dir, board, n))
}

/// Put back the board a hand-over couldn't write: its elements over the stale file's, unsaved
/// still, so the autosave writes them once it can, and a message says so.
fn take_unsaved(ctx: &mut Ctx, st: &mut BoardState, v: &Value) {
    let Some(path) = st.path.clone() else {
        return;
    };
    let Some(board) = v
        .get("svg")
        .and_then(Value::as_str)
        .and_then(|t| svg::read(t).ok())
    else {
        return;
    };
    // `write_board`'s error names the path; the status line names the file.
    let error = v.get("error").and_then(Value::as_str).unwrap_or("");
    let why = error
        .strip_prefix(&format!("{}: ", path.display()))
        .unwrap_or(error)
        .to_string();
    st.board = board;
    st.dirty = true;
    st.changed_at = ctx.now();
    st.error = Some(format!("{}: {why}", st.name()));
    ctx.messages.push(format!(
        "whiteboard: {} couldn't be written at the hand-over ({why}); its unsaved changes are back from the session, and are written once it can be",
        path.display()
    ));
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
    if let Some(out) = v.get("exported").and_then(Value::as_str) {
        st.exported = Some(PathBuf::from(out));
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
            "Open a whiteboard: the file given, or from a buffer that isn't a board the board used last, or else a new scratch board.",
            whiteboard,
        );
        reg.command(
            "whiteboard-new",
            "A new scratch board, in no file until S saves it as one.",
            show_scratch,
        );
        reg.command(
            "whiteboard-find",
            "Ask which board's file to open or start, `Board: `.",
            whiteboard_find,
        );
        reg.command(
            "whiteboard-save",
            "S: write the board's file now; boards are written a second after each change anyway. A scratch board asks `Save board as: `, and is that file's board from then on.",
            |ctx, a| {
                let (buf, state) = target(ctx, a)?;
                if state.borrow().path.is_none() {
                    ask_save_as(ctx, buf, None);
                    return Ok(());
                }
                ctx.with_ext(|wb: &mut Whiteboard, ctx| wb.save_now(ctx, buf))??;
                let name = ctx.buffer_name(buf).unwrap_or_default();
                ctx.messages.push(format!("Wrote {name}"));
                Ok(())
            },
        );
        reg.command(
            "whiteboard-save-and-close",
            "ZZ: write the board's file now and close its window, as `:x` does for a file. A scratch board asks where first, as S does.",
            |ctx, a| {
                let (buf, state) = target(ctx, a)?;
                if state.borrow().path.is_none() {
                    ask_save_as(ctx, buf, Some(a.clone()));
                    return Ok(());
                }
                ctx.with_ext(|wb: &mut Whiteboard, ctx| wb.save_now(ctx, buf))??;
                ctx.run("vim-quit", a)
            },
        );
        reg.command(
            "whiteboard-export",
            "E: export the board, asking `Export to: ` (NAME.png beside the file, or where it went last); a .png is rasterized from the model at your scale, an .svg is the picture without the dialect. `png` or `svg` given writes beside a board's file without asking.",
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
                change(ctx, a, |s, now| {
                    s.set_tool(tool, now);
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
            .bind(Normal, "ZZ", "whiteboard-save-and-close")
            .bind(Normal, "ZQ", "vim-quit")
            .bind(Normal, "<C-^>", "vim-alternate-buffer")
            .bind(Normal, "<C-o>", "vim-jump-backward")
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
    /// first if it's behind, so a restart finds it on disk; and if the write fails, the board
    /// itself, so the restart brings its unsaved changes back rather than the stale file. A
    /// scratch board, which has no file, is kept whole, in the dialect, with its number and
    /// directory; an empty one isn't kept. Not keyed by the buffer: core keeps buffer entries
    /// only for buffers it saves itself, and a surface's isn't one.
    fn save_session(&self, ctx: &Ctx) -> Kept {
        let mut boards = vec![];
        let mut pictures = self.kept.borrow_mut();
        pictures.retain(|b, _| {
            self.boards
                .get(b)
                .is_some_and(|s| s.borrow().path.is_none())
        });
        for buf in ctx.buffer_list() {
            let Some(state) = self.boards.get(&buf).filter(|_| ctx.is_live(buf)) else {
                continue;
            };
            let mut st = state.borrow_mut();
            // An empty scratch board is nothing to come back to.
            if st.path.is_none() && st.board.is_empty() {
                continue;
            }
            let unwritten = st.dirty.then(|| write_board(ctx, &mut st).err()).flatten();
            let look = self.look(ctx, &st);
            // A scratch board's picture, written again only once it's changed.
            let picture = || match pictures.get(&buf) {
                Some((rev, text)) if *rev == st.board_rev => text.clone(),
                _ => {
                    let text = svg::write(&st.board, &look, true);
                    pictures.insert(buf, (st.board_rev, text.clone()));
                    text
                }
            };
            let mut kept = kept_board(ctx, buf, &st, picture);
            if let Some(error) = unwritten {
                let svg = svg::write(&st.board, &look, true);
                kept["unsaved"] = json!({ "svg": svg, "error": error });
            }
            boards.push(kept);
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

    /// Open each kept board from its file, or a scratch board from what was kept of it, the one
    /// used last shown in this window (a stand-in for its own window, until M1's surface
    /// restorer), the rest open in the background.
    fn restore_session(&mut self, ctx: &mut Ctx, kept: Kept) -> Result {
        let boards = kept
            .state
            .get("boards")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let mut show = None;
        for record in boards.iter().rev() {
            let buf = match reopen(self, ctx, record) {
                Ok(buf) => buf,
                Err(e) => {
                    ctx.messages.push(e);
                    continue;
                }
            };
            if let Some(state) = self.boards.get(&buf) {
                let mut st = state.borrow_mut();
                if let Some(unsaved) = record.get("unsaved") {
                    take_unsaved(ctx, &mut st, unsaved);
                }
                take_kept(&mut st, record);
            }
            if record.get("shown").and_then(Value::as_bool) == Some(true) {
                show = Some(buf);
            }
        }
        self.arm(ctx);
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
        assert!(o.pixels);
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
