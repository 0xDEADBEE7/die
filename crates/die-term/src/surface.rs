//! A terminal buffer: a [`Term`] on a core PTY, drawn as a surface (scope §6, D18).
//!
//! What the program prints reaches [`Term::feed`] from the top of the loop, for at most [`TURN`]
//! a turn, so a key or the frame goes between, and the engine's answers go back to the PTY. In
//! insert state, which is core's passthrough, every key outside the DIE layer goes through the
//! engine's encoder to the program; core leaves for normal state on a plain Esc, on the alternate
//! screen too, unless [`Surface::takes_esc`] says the program has it, as the buffer's [`Escape`]
//! routing says: never by default, always, or on the alternate screen, evil-ghostel's `auto`.
//!
//! Normal state is copy state: core reads the engine's text through [`CopyText`] as the buffer
//! leaves insert, and again as output arrives, and die-vim's grammar runs over it, over the
//! alternate screen's rows alone while a program is there. Its Esc goes to the program only
//! while [`TermSurface::busy`] and out of line mode, and once until another key.
//!
//! **Size.** One terminal has one size, whichever windows show it (C10): the smallest columns
//! and the smallest rows among their areas, as Emacs's `window-adjust-process-window-size-smallest`
//! gives a process, so the program never draws past one. An area is what the buffer's maximum
//! width and centering leave of a window, as for a file's text (`Ctx::surface_area`). A larger
//! area shows the grid at its top left. It's worked out again on each resize and focus change.
use crate::{CopyText, Event, MouseTracking, PasteRefused, Pointer, PointerAction, PointerButton};
use crate::{Term, TermCursor};
use die_core::{
    BufferId, Button, CellPixels, Ctx, CursorShape, Frame, FrameCell, Key, KeyEvent, KeyKind,
    ModifierKey, PointerKind, PromptSpec, Pty, PtySize, RegKind, Register, RenderInfo, State,
    Surface, SurfaceCursor, SurfacePointer, SurfaceText,
};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// The surface kind, which names the keymap a terminal's normal state uses.
pub const KIND: &str = "term";

/// Where a plain Esc in insert state goes, set per buffer by `term-toggle-send-escape`: to
/// normal state by default, on the alternate screen too, so Esc Esc reaches a fullscreen
/// program from insert (scope D18).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Escape {
    /// evil-ghostel's `auto`: to the program while it's on the alternate screen (vim, less,
    /// htop), else to normal state.
    Auto,
    /// Always to the program, for vim run in the terminal.
    Program,
    /// Always to normal state.
    #[default]
    Normal,
}

/// How long output is parsed for in one turn, so a key or the frame goes between (call 786).
pub const TURN: Duration = Duration::from_millis(4);
/// The most output left waiting for a later turn. Past it, it's parsed past [`TURN`], since
/// crafted output can come faster than it parses.
pub const WAITING_MAX: usize = 8 << 20;

/// What an OSC 52 write does with its text.
pub type CopyFn = fn(&mut Ctx, Register);

/// A terminal: its engine, its PTY, and what its status line and cursor need.
pub struct TermSurface {
    pub(crate) term: Term,
    pub(crate) pty: Option<Pty>,
    pub(crate) buf: BufferId,
    /// The buffer's name when it opened, for the status line while the program sets no title.
    pub(crate) name: String,
    /// What runs, as messages name it.
    pub(crate) program: String,
    /// It's a shell, so it's busy only while a job has the terminal.
    pub(crate) shell: bool,
    pub(crate) dir: PathBuf,
    pub(crate) escape: Escape,
    /// The count of keys when normal state's Esc last sent one, which it doesn't again until
    /// another key (`Terminals`).
    pub(crate) esc_sent: Option<u64>,
    pub(crate) copy: CopyFn,
    pub(crate) cell: CellPixels,
    /// The cursor as the last frame left it, and the area that frame had.
    at: Option<TermCursor>,
    area: (usize, usize),
    /// A wheel's part of a row, kept for the next scroll.
    scrolled: f32,
    /// The button a press held, for motion reports.
    held: Option<PointerButton>,
    /// Output not yet parsed, and whether a later turn's timer is set for it.
    waiting: Vec<u8>,
    later: bool,
    /// Normal state's copy state, reading the engine's text (scope §5.5), and whether it last
    /// placed the viewport.
    copy_text: CopyText,
    copying: bool,
}

impl TermSurface {
    pub(crate) fn new(term: Term, name: &str, program: &str, dir: &Path, copy: CopyFn) -> Self {
        TermSurface {
            term,
            pty: None,
            buf: BufferId::default(),
            name: name.into(),
            program: program.into(),
            shell: false,
            dir: dir.into(),
            escape: Escape::default(),
            esc_sent: None,
            copy,
            cell: CellPixels::default(),
            at: None,
            area: (0, 0),
            scrolled: 0.0,
            held: None,
            waiting: Vec::new(),
            later: false,
            copy_text: CopyText::default(),
            copying: false,
        }
    }

    /// The engine, for copy state and tests.
    pub fn term(&mut self) -> &mut Term {
        &mut self.term
    }

    /// Where a plain Esc in insert goes now.
    pub fn escape(&self) -> Escape {
        self.escape
    }

    /// Send bytes to the program.
    pub(crate) fn write(&self, bytes: &[u8]) {
        if let Some(pty) = self.pty.as_ref().filter(|_| !bytes.is_empty()) {
            pty.write(bytes);
        }
    }

    /// What the program printed: parse what's waiting for at most [`TURN`], answer its queries,
    /// and do what it asked of DIE. What's left waits for a timer due at once, which core runs
    /// after the next key or frame, as it runs timers.
    pub(crate) fn output(&mut self, ctx: &mut Ctx, bytes: &[u8]) {
        self.waiting.extend_from_slice(bytes);
        let (start, mut at) = (Instant::now(), 0);
        while at < self.waiting.len()
            && (start.elapsed() < TURN || self.waiting.len() - at > WAITING_MAX)
        {
            at += self.term.feed(&self.waiting[at..]);
        }
        self.waiting.drain(..at);
        if !self.waiting.is_empty() && !std::mem::replace(&mut self.later, true) {
            let buf = self.buf;
            ctx.after(Duration::ZERO, move |ctx| {
                // Gone if the buffer was killed meanwhile.
                let _ = ctx.with_surface(buf, |s: &mut TermSurface, ctx| {
                    s.later = false;
                    s.output(ctx, &[]);
                });
                Ok(())
            })
            .detach();
        }
        let replies = self.term.take_replies();
        self.write(&replies);
        ctx.surface_text_changed(self.buf);
        for ev in self.term.take_events() {
            match ev {
                Event::Pwd(url) => {
                    if let Some(dir) = file_url_path(&url).filter(|d| d.is_dir()) {
                        ctx.set_directory(self.buf, dir.clone());
                        self.dir = dir;
                    }
                }
                Event::Clipboard { text, .. } => {
                    let kind = RegKind::Char;
                    (self.copy)(ctx, Register { kind, text });
                }
                Event::Notify { title, body } => {
                    let said = [title, body].into_iter().filter(|s| !s.is_empty());
                    let said: Vec<String> = said.collect();
                    ctx.messages
                        .push(format!("{}: {}", self.name, said.join(": ")));
                }
                Event::Title(_) | Event::Bell => {}
            }
        }
    }

    /// Whether a program other than the shell has the terminal, as a quit should ask about.
    pub fn busy(&self) -> bool {
        let Some(pid) = self.pty.as_ref().and_then(Pty::pid) else {
            return false;
        };
        match self.pty.as_ref().and_then(Pty::foreground_group) {
            Some(group) => !self.shell || pid != group,
            None => !self.shell,
        }
    }

    /// Whether an Esc sent now is a key to a program: one other than the shell has the terminal
    /// and reads keys as they come. In line mode, as during a job that reads none, it would only
    /// be a byte at the start of the next line, which the prompt then reads as Meta.
    pub(crate) fn reads_keys(&self) -> bool {
        let line_mode = self.pty.as_ref().and_then(Pty::line_mode);
        line_mode == Some(false) && self.busy()
    }

    /// Back to insert at the live screen, from `i`, `a`, `I` or `A`.
    pub(crate) fn insert(&mut self) {
        self.term.scroll_to_bottom();
    }

    /// Paste `text`, bracketed if the program asked. Text that could run what it didn't mean to
    /// (a newline the program didn't ask to have bracketed) is asked about first.
    pub(crate) fn paste(&mut self, ctx: &mut Ctx, text: &str) {
        match self.term.paste(text, false) {
            Ok(bytes) => {
                self.term.scroll_to_bottom();
                self.write(&bytes);
            }
            Err(PasteRefused) => {
                let (buf, text) = (self.buf, text.to_string());
                let ask = "The program didn't ask for a bracketed paste, and this has a line \
                           break or a control code that could run a command. Paste it? (y or n) ";
                ctx.prompt(PromptSpec::new(ask), move |ctx, answer| {
                    if !matches!(answer.as_deref(), Some("y" | "yes")) {
                        return Ok(());
                    }
                    // Gone if the program ended meanwhile.
                    let _ = ctx.with_surface(buf, |s: &mut TermSurface, _| {
                        if let Ok(bytes) = s.term.paste(&text, true) {
                            s.term.scroll_to_bottom();
                            s.write(&bytes);
                        }
                    });
                    Ok(())
                });
            }
        }
    }

    /// Fit the terminal to the windows showing it: the smallest columns and rows among their
    /// areas, and `fallback`'s size if none in the split tree does.
    fn fit(&mut self, ctx: &mut Ctx, fallback: Option<(usize, usize)>, cell: CellPixels) {
        let showing = ctx.window_list().into_iter();
        let showing = showing.filter(|w| ctx.window_buffer(*w) == Some(self.buf));
        let areas = showing.filter_map(|w| ctx.surface_area(w));
        let sizes = areas.map(|g| (g.text_w, g.rows));
        let sizes: Vec<(usize, usize)> = sizes.filter(|&(c, r)| c > 0 && r > 0).collect();
        let size = match (
            sizes.iter().map(|s| s.0).min(),
            sizes.iter().map(|s| s.1).min(),
        ) {
            (Some(cols), Some(rows)) => (cols, rows),
            _ => match fallback {
                Some((cols, rows)) if cols > 0 && rows > 0 => (cols, rows),
                _ => return,
            },
        };
        let to_u16 = |n: usize| u16::try_from(n).unwrap_or(u16::MAX);
        let (cols, rows) = (to_u16(size.0), to_u16(size.1));
        if (cols, rows) == self.term.size() && cell == self.cell {
            return;
        }
        self.cell = cell;
        self.term.resize(cols, rows, (cell.width, cell.height));
        if let Some(pty) = &self.pty {
            let _ = pty.resize(PtySize::new(size.0, size.1, cell));
        }
        let replies = self.term.take_replies();
        self.write(&replies);
        ctx.surface_text_changed(self.buf);
    }

    fn key(&mut self, ev: &KeyEvent) -> bool {
        // The DIE layer is DIE's (scope D18): a Cmd chord nothing binds goes nowhere.
        if ev.mods.super_ || ev.key == Key::Modifier(ModifierKey::Super) {
            return false;
        }
        let bytes = self.term.key(ev);
        if bytes.is_empty() {
            return true;
        }
        if ev.kind != KeyKind::Release && !matches!(ev.key, Key::Modifier(_)) {
            self.term.scroll_to_bottom();
        }
        self.write(&bytes);
        true
    }

    fn pointer(&mut self, p: &SurfacePointer) -> bool {
        let button = |b: Button| match b {
            Button::Left => PointerButton::Left,
            Button::Middle => PointerButton::Middle,
            Button::Right => PointerButton::Right,
        };
        if let PointerKind::Scroll { dy, .. } = p.kind {
            if !dy.is_finite() {
                return true;
            }
            self.scrolled += dy;
            let rows = self.scrolled.trunc();
            self.scrolled -= rows;
            if rows != 0.0 {
                let bytes = self.term.wheel(rows as i32, p.x, p.y, p.mods);
                self.write(&bytes);
            }
            return true;
        }
        if self.term.modes().mouse == MouseTracking::None {
            return false;
        }
        let (action, b) = match p.kind {
            PointerKind::Press(b) => {
                self.held = Some(button(b));
                (PointerAction::Press, self.held)
            }
            PointerKind::Release(b) => {
                self.held = None;
                (PointerAction::Release, Some(button(b)))
            }
            PointerKind::Drag(b) => (PointerAction::Motion, Some(button(b))),
            PointerKind::Move => (PointerAction::Motion, self.held),
            _ => return false,
        };
        let ev = Pointer {
            action,
            button: b,
            mods: p.mods,
            x: p.x,
            y: p.y,
        };
        let bytes = self.term.mouse(&ev);
        self.write(&bytes);
        true
    }
}

impl Surface for TermSurface {
    fn kind(&self) -> &'static str {
        KIND
    }

    fn render(&mut self, frame: &mut Frame, _info: &RenderInfo) {
        self.area = (frame.cols(), frame.rows());
        let Some(f) = self.term.frame(frame.is_new()) else {
            return;
        };
        if f.full {
            frame.clear();
        }
        for (r, cells) in f.changed {
            let row = frame.row_mut(usize::from(r));
            for (slot, cell) in row.iter_mut().zip(cells) {
                *slot = FrameCell::from(cell);
            }
        }
        self.at = Some(f.cursor);
    }

    /// The program's cursor where it put it. On the main screen its shape is the state's, a bar
    /// in insert, as evil-ghostel draws it; on the alternate screen it's what the program asked
    /// for. In normal state core draws copy state's cursor instead.
    fn cursor(&self, info: &RenderInfo) -> Option<SurfaceCursor> {
        let c = self.at?;
        let (row, col) = c.at.map(|(r, c)| (usize::from(r), usize::from(c)))?;
        if row >= self.area.1 || col >= self.area.0 {
            return None; // until the window's new size reaches the program
        }
        let insert = info.state == State::Passthrough;
        let shape = match (insert, self.term.alt_screen()) {
            (true, true) => c.shape,
            _ => CursorShape::for_state(info.state),
        };
        Some(SurfaceCursor {
            row,
            col,
            shape,
            visible: c.visible || !insert,
            blinking: c.blink && insert,
        })
    }

    /// The working directory as your mode line shows `default-directory`, then the program's
    /// title, or the buffer's name while there's none.
    fn status(&self) -> Option<String> {
        let title = self.term.title();
        let what = if title.is_empty() { &self.name } else { title };
        Some(format!("{}  {what}", shown(&self.dir)))
    }

    fn event(&mut self, ev: &die_core::Event, ctx: &mut Ctx) -> bool {
        match ev {
            die_core::Event::Key(k) if ctx.state_of(self.buf) == State::Passthrough => self.key(k),
            die_core::Event::Resize {
                cols, rows, cell, ..
            } => {
                self.fit(ctx, Some((*cols, *rows)), *cell);
                true
            }
            die_core::Event::Focus(gained) => {
                let cell = ctx.cell_pixels();
                self.fit(ctx, None, cell);
                let bytes = self.term.focus(*gained);
                self.write(&bytes);
                true
            }
            die_core::Event::Pointer(p) => self.pointer(p),
            die_core::Event::Paste(text) => {
                self.paste(ctx, text);
                true
            }
            _ => false,
        }
    }

    fn handles(&self, key: &KeyEvent) -> bool {
        !key.mods.super_
    }

    fn takes_esc(&self) -> bool {
        match self.escape {
            Escape::Auto => self.term.alt_screen(),
            Escape::Program => true,
            Escape::Normal => false,
        }
    }

    fn initial_state(&self) -> State {
        State::Passthrough
    }

    fn text(&mut self, update: bool) -> Option<SurfaceText> {
        Some(self.copy_text.text(&mut self.term, update))
    }

    /// Copy state's place, or with None, outside it, where insert state left the viewport: the
    /// live screen once copy state ends, and else wherever the wheel put it.
    fn scroll_to(&mut self, top: Option<usize>) {
        match top {
            Some(_) => {
                self.copy_text.scroll(&mut self.term, top);
                self.copying = true;
            }
            None if std::mem::take(&mut self.copying) => self.term.scroll_to_bottom(),
            None => {}
        }
    }
}

/// A directory as your mode line shows it: `~` for the home directory, and a `/` at the end.
fn shown(dir: &Path) -> String {
    let home = std::env::home_dir();
    let rest = home.as_deref().and_then(|h| dir.strip_prefix(h).ok());
    let text = match rest {
        Some(rest) if rest.as_os_str().is_empty() => "~".to_string(),
        Some(rest) => format!("~/{}", rest.display()),
        None => dir.display().to_string(),
    };
    match text.ends_with('/') {
        true => text,
        false => text + "/",
    }
}

/// The local path an OSC 7 `file://host/path` URL names, with its `%xx` escapes decoded, or OSC
/// 9;9's bare path. Any host is taken as this one: a shell over SSH that reports its own host's
/// paths is rare.
pub(crate) fn file_url_path(url: &str) -> Option<PathBuf> {
    if url.starts_with('/') {
        return Some(PathBuf::from(url)); // OSC 9;9's bare path
    }
    let rest = url.strip_prefix("file://")?;
    let path = &rest[rest.find('/')?..];
    let mut out = Vec::with_capacity(path.len());
    let mut bytes = path.bytes();
    while let Some(b) = bytes.next() {
        if b != b'%' {
            out.push(b);
            continue;
        }
        let hex: Vec<u8> = bytes.by_ref().take(2).collect();
        let byte = std::str::from_utf8(&hex)
            .ok()
            .and_then(|h| u8::from_str_radix(h, 16).ok())?;
        out.push(byte);
    }
    Some(PathBuf::from(String::from_utf8(out).ok()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_osc_7_url_names_its_path() {
        let p = file_url_path("file://mac.local/Users/me/a%20b").unwrap();
        assert_eq!(p, PathBuf::from("/Users/me/a b"));
        assert_eq!(file_url_path("file:///tmp").unwrap(), PathBuf::from("/tmp"));
        assert_eq!(file_url_path("http://x/y"), None);
        assert_eq!(file_url_path("file://host/bad%zz"), None);
        assert_eq!(file_url_path("/a b").unwrap(), PathBuf::from("/a b"));
    }
}
