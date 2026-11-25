//! mini-fig: a spike of die-fig at about a tenth of its size, reading the same `.fig` specs, so
//! the owner can open a dashboard in both and feel what is lost (scope D14).
//!
//! - [`spec`]: a `.fig` file read into mini-fig's own types, every figure a dashboard;
//! - [`data`]: tables of strings, numbers and times, read from CSV, filtered and sorted;
//! - [`scale`]: linear, log, time and band scales, their ticks, and how numbers are written;
//! - [`canvas`]: tiny-skia and die-draw's text in logical px, and die-fig's dark theme;
//! - [`dash`]: a figure laid out and drawn, every figure a dashboard of panels;
//! - [`chart`], [`tile`] and [`tree`]: what a panel draws, each mark left as a [`hit::Hit`];
//! - [`hit`]: the hit under the pointer, where a key takes the cursor, and the one card;
//! - [`view`]: a figure in a window, mini-fig's one surface.
//!
//! Nothing of die-fig is used or changed. `\V` runs `mini-fig-toggle`: from a `.fig` file's
//! buffer, or die-fig's figure of one, to mini-fig's figure, and back. `:mini-fig PATH` shows
//! any spec's.

#[cfg(test)]
use die_test_cap as _;

pub mod canvas;
pub mod chart;
pub mod dash;
pub mod data;
pub mod hit;
pub mod scale;
pub mod spec;
pub mod tile;
pub mod tree;
pub mod view;

use die_core::{Args, BufferId, Ctx, Error, Extension, Registry, Result, ShowIn, State};
use die_draw::Book;
use hit::Dir;
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use view::{Key, View};

/// A figure's keys, a line each: the key, its command, and what that does.
const KEYS: &str = "\
<Tab> mini-fig-next-panel Focus the figure's next panel.
<S-Tab> mini-fig-prev-panel Focus the figure's previous panel.
h mini-fig-left Move the cursor back along a series.
l mini-fig-right Move the cursor on along a series.
k mini-fig-up Move the cursor to the series, row or level above.
j mini-fig-down Move the cursor to the series, row or level below.
r mini-fig-reload Read the figure's spec and data again.
<Esc> mini-fig-clear Clear the figure's cursor, or else its focus.";

/// What each of [`KEYS`] does, in its order.
const DOES: [Key; 8] = [
    Key::Next,
    Key::Prev,
    Key::Move(Dir::Left),
    Key::Move(Dir::Right),
    Key::Move(Dir::Up),
    Key::Move(Dir::Down),
    Key::Reload,
    Key::Clear,
];

/// die-vim's `C-w` keys, as die-fig binds them on its surface, which has no text to inherit
/// them from: `<C-w>h` and `<C-w><C-h>` run `vim-window-left`, and so on.
const WINDOW_KEYS: &str = "h:left j:down k:up l:right w:next W:prev p:mru s:split v:vsplit \
    n:new c:delete o:only =:balance +:taller -:shorter >:wider <lt>:narrower";

/// The extension: the toggle, `:mini-fig`, and a figure's keys.
#[derive(Debug, Default)]
pub struct MiniFig {
    /// The fonts, found for the first figure and shared by every one after it.
    book: Option<Rc<RefCell<Book>>>,
}

impl MiniFig {
    /// `mini-fig-toggle`: on a figure, back to the buffer it came from (or its file); on a
    /// `.fig` file's buffer, or die-fig's figure of one, that file's figure.
    fn toggle(&mut self, ctx: &mut Ctx, a: &Args) -> Result {
        let buf = a.buffer.or(ctx.focused_buffer());
        let buf = buf.ok_or_else(|| Error::Msg("No buffer here".into()))?;
        if let Ok((from, path)) = ctx.with_surface(buf, |v: &mut View, _| (v.from, v.path.clone()))
        {
            let to = match from {
                Some(b) if ctx.is_live(b) => b,
                _ => ctx.open_file(&path)?,
            };
            ctx.show(to, ShowIn::Current)?;
            let mut windows = ctx.window_list().into_iter();
            if !windows.any(|w| ctx.window_buffer(w) == Some(buf)) {
                ctx.defer(move |ctx| {
                    ctx.kill(buf);
                    Ok(())
                });
            }
            return Ok(());
        }
        let path = spec_of(ctx, buf).ok_or_else(|| {
            Error::Msg("mini-fig: not a .fig file here; `:mini-fig PATH` shows one".into())
        })?;
        self.open(ctx, &path, Some(buf))
    }

    /// `:mini-fig PATH`: that spec's figure, the path from the buffer's directory or `~/`;
    /// alone, the toggle.
    fn command(&mut self, ctx: &mut Ctx, a: &Args) -> Result {
        let arg = a.text.as_deref().map_or("", str::trim);
        if arg.is_empty() {
            return self.toggle(ctx, a);
        }
        let here = a.buffer.or(ctx.focused_buffer());
        let dir = here.and_then(|b| ctx.directory(b)).map(Path::to_path_buf);
        let path = match (arg.strip_prefix("~/"), std::env::home_dir()) {
            (Some(rest), Some(home)) => home.join(rest),
            _ => dir.unwrap_or_default().join(arg),
        };
        self.open(ctx, &path, here)
    }

    /// Show the figure of the spec at `path` in this window: the one already open, or a new
    /// one. Going back goes to `from`.
    fn open(&mut self, ctx: &mut Ctx, path: &Path, from: Option<BufferId>) -> Result {
        if !ctx.pixels_supported() {
            return Err(Error::Msg("mini-fig: this host shows no pixels".into()));
        }
        let shown = path.display();
        let path = path.canonicalize();
        let path = path.map_err(|e| Error::Msg(format!("mini-fig: {shown}: {e}")))?;
        let mut bufs = ctx.buffer_list().into_iter();
        let open =
            bufs.find(|b| ctx.with_surface(*b, |v: &mut View, _| v.path == path) == Ok(true));
        let buf = match open {
            Some(b) => b,
            None => {
                let book = match &self.book {
                    Some(b) => b.clone(),
                    None => {
                        let b =
                            Book::load(None).map_err(|e| Error::Msg(format!("mini-fig: {e}")))?;
                        self.book.insert(Rc::new(RefCell::new(b))).clone()
                    }
                };
                let name = path.file_name().unwrap_or_default().to_string_lossy();
                let b = ctx
                    .create_surface(&format!("{name} *mini-fig*"), View::new(path.clone(), book));
                ctx.set_directory(b, path.parent().unwrap_or(Path::new("/")));
                ctx.change_settings(b, |s| s.max_width = None)?;
                b
            }
        };
        ctx.with_surface(buf, |v: &mut View, _| v.from = from.filter(|f| *f != buf))?;
        ctx.show(buf, ShowIn::Current)?;
        Ok(())
    }
}

/// The `.fig` file a buffer shows: the one it visits, or the one die-fig's figure in it was
/// drawn from, which is named after its source buffer (or file) with ` *fig*`, in its folder.
fn spec_of(ctx: &Ctx, buf: BufferId) -> Option<PathBuf> {
    if let Some(f) = ctx.file(buf) {
        return is_spec(&f.path).then(|| f.path.clone());
    }
    let name = ctx.buffer_name(buf)?.strip_suffix(" *fig*")?;
    let mut bufs = ctx.buffer_list().into_iter();
    let source = bufs.find(|b| ctx.buffer_name(*b) == Some(name));
    let visited = source.and_then(|b| ctx.file(b)).map(|f| f.path.clone());
    let path = visited.unwrap_or_else(|| ctx.directory(buf).unwrap_or(Path::new("")).join(name));
    (is_spec(&path) && path.is_file()).then_some(path)
}

/// Whether a file is a spec: `.fig` or `.fig.json`, in any case.
fn is_spec(path: &Path) -> bool {
    let name = path.to_string_lossy().to_ascii_lowercase();
    name.ends_with(".fig") || name.ends_with(".fig.json")
}

/// Run a key on the figure in the window it was typed in.
fn on_view(ctx: &mut Ctx, a: &Args, key: Key) -> Result {
    let buf = a.buffer.or(ctx.focused_buffer());
    let buf = buf.ok_or_else(|| Error::Msg("No buffer here".into()))?;
    ctx.with_surface(buf, |v: &mut View, _| v.key(key))
        .map_err(|_| Error::Msg("mini-fig: not a figure".into()))
}

impl Extension for MiniFig {
    fn name(&self) -> &'static str {
        "mini-fig"
    }

    fn register(&mut self, reg: &mut Registry) {
        reg.ext_command(
            "mini-fig-toggle",
            "Show this .fig file's figure in mini-fig, the small spike of fig, or from a figure go back to where it came from.",
            MiniFig::toggle,
        );
        reg.ext_command(
            "mini-fig",
            "Show a .fig spec's figure in mini-fig (`:mini-fig PATH`, from the buffer's folder); alone, mini-fig-toggle.",
            MiniFig::command,
        );
        for (line, key) in KEYS.lines().zip(DOES) {
            let mut words = line.splitn(3, ' ');
            let (keys, name) = (words.next().unwrap_or(""), words.next().unwrap_or(""));
            reg.command(name, words.next().unwrap_or(""), move |ctx, a| {
                on_view(ctx, a, key)
            });
            reg.kind_keymap(view::KIND).bind(State::Normal, keys, name);
        }
        let k = reg.kind_keymap(view::KIND);
        k.bind(State::Normal, ":", "vim-ex");
        for pair in WINDOW_KEYS.split_whitespace() {
            let (key, cmd) = pair.split_once(':').unwrap_or((pair, ""));
            let name = format!("vim-window-{cmd}");
            k.bind(State::Normal, &format!("<C-w>{key}"), &name);
            if key.len() == 1 && key.chars().all(|c| c.is_ascii_lowercase()) {
                k.bind(State::Normal, &format!("<C-w><C-{key}>"), &name);
            }
        }
    }
}
