//! The public harness (contract §16, K7): a workspace with no window and no GPU, and what tests
//! stand in for the outside world with.
#![warn(missing_docs)]
use crate::app::{Input, PointerEvent, PointerKind};
use crate::error::Result;
use crate::ext::{Config, Ctx, Extension};
use crate::ids::{BufferId, WindowId};
use crate::keys::{Mods, key_notation, typed};
use crate::pixels::{PixelBufferId, PixelRect, PixelSink};
use crate::rt::Clock;
use crate::workspace::{Clipboard, RegKind, Register, ShowIn};
use std::cell::RefCell;
use std::collections::HashMap;
use std::ops::{Deref, DerefMut};
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// The text area of a harness's window until [`Harness::set_window_size`] changes it.
const COLS: usize = 80;
const ROWS: usize = 24;
/// The status line and the command line, below a lone window's text.
const CHROME: usize = 2;

/// A workspace for tests: a fake clock, a fake clipboard, a fake pixel sink, and a frame drawn
/// after every input, as a host draws one between keystrokes, so scrolling is what it would be
/// on screen. Everything else is [`Ctx`], which it derefs to.
pub struct Harness {
    ctx: Ctx,
    clipboard: FakeClipboard,
    pixels: FakePixelSink,
    /// Errors logged outside a harness input, until [`Harness::take_errors`] reads them.
    earlier: Vec<String>,
}

/// One typed key and what it did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyLog {
    /// The key, in notation.
    pub key: String,
    /// The errors and panics logged by the time everything it started had finished and its
    /// frame was drawn. News such as "Wrote foo" is only in [`Ctx::messages`].
    pub errors: Vec<String>,
}

impl Harness {
    /// A harness with these extensions and the default config.
    pub fn new(exts: Vec<Box<dyn Extension>>) -> std::result::Result<Harness, Vec<String>> {
        Harness::with_config(exts, Config::default())
    }

    /// A harness with these extensions and this config. A real clock gives way to a fake one,
    /// and a fake clock it sets stays. Any clipboard it names gives way too, even a
    /// [`FakeClipboard`], so use [`Harness::fake_clipboard`] or [`Harness::set_clipboard`].
    pub fn with_config(
        exts: Vec<Box<dyn Extension>>,
        mut config: Config,
    ) -> std::result::Result<Harness, Vec<String>> {
        if matches!(config.clock, Clock::Real(_)) {
            config.clock = Clock::default();
        }
        let clipboard = FakeClipboard::default();
        config.clipboard = Some(Box::new(clipboard.clone()));
        let mut ctx = Ctx::with_config(exts, config)?;
        ctx.errors = Some(vec![]);
        ctx.set_frame_size(COLS, ROWS + CHROME);
        let pixels = FakePixelSink::default();
        ctx.set_pixel_sink(pixels.clone())
            .expect("a new workspace has no pixel sink");
        Ok(Harness {
            ctx,
            clipboard,
            pixels,
            earlier: vec![],
        })
    }

    /// The clipboard core is using, unless a test went round [`Harness::set_clipboard`] to give
    /// it another kind with [`Ctx::set_clipboard`].
    pub fn fake_clipboard(&self) -> &FakeClipboard {
        &self.clipboard
    }

    /// The sink pixel buffers present to: the fake, which keeps each buffer's latest pixels and
    /// logs every call, so a test can see what an extension drew.
    pub fn fake_pixels(&self) -> &FakePixelSink {
        &self.pixels
    }

    /// A pixel buffer as last presented: its width, height and pixels, premultiplied RGBA8 with
    /// `width * 4` bytes a row, transparent where nothing was presented since it was made or
    /// resized. None for a buffer that was destroyed or never made.
    pub fn pixels(&self, id: PixelBufferId) -> Option<(u32, u32, Vec<u8>)> {
        self.pixels.pixels(id)
    }

    /// Use this clipboard from now on, and [`Harness::fake_clipboard`] with it.
    pub fn set_clipboard(&mut self, fake: FakeClipboard) {
        self.clipboard = fake.clone();
        self.ctx.set_clipboard(fake);
    }

    /// A new text buffer, shown in the focused window or in the first window if there's none.
    pub fn open_text(&mut self, name: &str, text: &str) -> Result<(BufferId, WindowId)> {
        let buf = self.ctx.create_text(name, text);
        let shown = match self.ctx.focused() {
            Some(_) => self.ctx.show(buf, ShowIn::Current)?,
            None => None,
        };
        let w = match shown {
            Some(w) => w,
            None => {
                let (cols, rows) = self.ctx.frame_size();
                self.ctx
                    .open_window(buf, cols, rows.saturating_sub(CHROME))?
            }
        };
        self.redisplay();
        Ok((buf, w))
    }

    /// Make the frame the size that gives a lone window this much text area.
    pub fn set_window_size(&mut self, cols: usize, rows: usize) -> Vec<String> {
        self.send(Input::Resize {
            cols,
            rows: rows + CHROME,
        })
    }

    /// Type `keys`, in notation, one at a time, each run until idle and followed by a frame.
    /// Unlike [`Ctx::feed_keys`], a key that fails doesn't drop the ones after it. If a job
    /// outlasts its wait, the rest of the keys don't wait for it again, as with
    /// [`Ctx::advance`]. Err if the notation doesn't parse, and then none are typed.
    pub fn type_keys(&mut self, keys: &str) -> std::result::Result<Vec<KeyLog>, String> {
        let events = typed(keys, Some(self.ctx.leader))?;
        let mut wait = true;
        Ok(events
            .into_iter()
            .map(|ev| {
                let key = key_notation(std::slice::from_ref(&ev));
                let (errors, waited) = self.run_input(Input::Key(ev), wait);
                wait = waited;
                KeyLog { key, errors }
            })
            .collect())
    }

    /// One pointer event at a cell of the frame, with no modifiers. A press or a release is a
    /// single click; [`Harness::send`] takes any other. Returns the errors it caused.
    pub fn pointer(&mut self, kind: PointerKind, row: usize, col: usize) -> Vec<String> {
        let clicks = matches!(kind, PointerKind::Press(_) | PointerKind::Release(_));
        self.send(Input::Pointer(PointerEvent {
            kind,
            row,
            col,
            clicks: u8::from(clicks),
            mods: Mods::default(),
            px: None,
        }))
    }

    /// Any input, run until idle and followed by a frame. Returns the errors it caused.
    pub fn send(&mut self, input: Input) -> Vec<String> {
        self.run_input(input, true).0
    }

    /// One input and its frame: the errors it caused, and whether to go on waiting for jobs.
    fn run_input(&mut self, input: Input, wait: bool) -> (Vec<String>, bool) {
        self.set_errors_aside();
        self.ctx.handle(input);
        let wait = self.ctx.drain(wait);
        self.redisplay();
        (self.new_errors(), wait)
    }

    /// Move the fake clock on, as [`Ctx::advance`] does, then draw a frame. Returns the errors
    /// that came of it.
    pub fn advance(&mut self, d: Duration) -> Vec<String> {
        self.set_errors_aside();
        self.ctx.advance(d);
        self.redisplay();
        self.new_errors()
    }

    /// The buffer line at the window's top on screen, from 0.
    pub fn top_line(&mut self, w: WindowId) -> Option<usize> {
        self.redisplay();
        let t = self.ctx.text(self.ctx.window_buffer(w)?).ok()?;
        Some(t.line_of(t.scroll(w)?.0))
    }

    /// Scroll the window so this buffer line is at its top, or the last line if there are fewer.
    /// The next frame scrolls back to the cursor (C5), so put the cursor on a line it then shows.
    /// None if `w` isn't showing text.
    pub fn set_top_line(&mut self, w: WindowId, line: usize) -> Option<()> {
        let buf = self.ctx.window_buffer(w)?;
        let t = self.ctx.text_mut(buf).ok()?;
        let top = t.line_start(line.min(t.line_count().saturating_sub(1)));
        t.set_scroll(w, top, 0)
    }

    /// The frame as text: one line per row, without trailing spaces.
    pub fn screen(&mut self) -> String {
        let s = self.ctx.scene();
        let rows: Vec<String> = (0..s.rows).map(|r| s.row_text(r)).collect();
        rows.join("\n")
    }

    /// The frame with its styles and cursors ([`crate::Scene::snapshot`]).
    pub fn style_dump(&mut self) -> String {
        self.ctx.scene().snapshot()
    }

    /// Restore what [`Ctx::session_json`] gave, perhaps on another harness, into this workspace
    /// ([`Ctx::restore_session`]), and draw a frame.
    pub fn restore_session(&mut self, json: &str) -> Result {
        self.ctx.restore_session(json)?;
        self.redisplay();
        Ok(())
    }

    fn redisplay(&mut self) {
        self.ctx.scene();
    }

    /// The errors and panics no harness input has returned, oldest first: those logged outside
    /// one, such as by [`Ctx::feed_keys`] and [`Ctx::run_until_idle`] for a macro, or
    /// [`Ctx::open_file`].
    pub fn take_errors(&mut self) -> Vec<String> {
        self.set_errors_aside();
        std::mem::take(&mut self.earlier)
    }

    fn set_errors_aside(&mut self) {
        let errors = self.new_errors();
        self.earlier.extend(errors);
    }

    fn new_errors(&mut self) -> Vec<String> {
        self.ctx
            .errors
            .as_mut()
            .map(std::mem::take)
            .unwrap_or_default()
    }
}

impl Deref for Harness {
    type Target = Ctx;
    fn deref(&self) -> &Ctx {
        &self.ctx
    }
}

impl DerefMut for Harness {
    fn deref_mut(&mut self) -> &mut Ctx {
        &mut self.ctx
    }
}

#[derive(Default)]
struct FakeState {
    held: Option<Register>,
    count: u64,
    writes: Vec<Register>,
    reads: u64,
    failing: bool,
}

/// A clipboard in memory, shared by its clones, so a test keeps one and gives core the other
/// with [`Harness::set_clipboard`].
/// Every workspace starts with its own until the host connects the real one, as it does with
/// the clock ([`Ctx::set_clipboard`]).
#[derive(Clone, Default)]
pub struct FakeClipboard(Rc<RefCell<FakeState>>);

impl FakeClipboard {
    /// Another app copies `text`.
    pub fn copy_from_outside(&self, text: &str) {
        let mut s = self.0.borrow_mut();
        s.held = Some(Register {
            kind: RegKind::Char,
            text: text.into(),
        });
        s.count += 1;
    }
    /// What it holds.
    pub fn held(&self) -> Option<Register> {
        self.0.borrow().held.clone()
    }
    /// How many times it's been read.
    pub fn reads(&self) -> u64 {
        self.0.borrow().reads
    }
    /// From now on every call fails, or none does, as the system's can.
    pub fn set_failing(&self, failing: bool) {
        self.0.borrow_mut().failing = failing;
    }
    fn check(&self) -> Result<(), String> {
        match self.0.borrow().failing {
            true => Err("the clipboard failed".into()),
            false => Ok(()),
        }
    }
    /// Everything written since the last call, oldest first.
    pub fn take_writes(&self) -> Vec<Register> {
        std::mem::take(&mut self.0.borrow_mut().writes)
    }
}

impl Clipboard for FakeClipboard {
    fn read(&mut self) -> Result<Option<Register>, String> {
        self.0.borrow_mut().reads += 1;
        self.check()?;
        Ok(self.held())
    }
    fn write(&mut self, r: &Register) -> Result<(), String> {
        self.check()?;
        let mut s = self.0.borrow_mut();
        s.held = Some(r.clone());
        s.writes.push(r.clone());
        s.count += 1;
        Ok(())
    }
    fn change_count(&mut self) -> Result<u64, String> {
        self.check()?;
        Ok(self.0.borrow().count)
    }
}

/// What a pixel sink heard ([`PixelSink`]), in order.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SinkCall {
    /// A buffer this size was made.
    Create(PixelBufferId, u32, u32),
    /// This much of a buffer was presented.
    Present(PixelBufferId, PixelRect),
    /// A buffer became this size.
    Resize(PixelBufferId, u32, u32),
    /// A buffer went.
    Destroy(PixelBufferId),
}

#[derive(Default)]
struct FakePixels {
    /// Each live buffer's size and pixels, as last presented.
    buffers: HashMap<PixelBufferId, (u32, u32, Vec<u8>)>,
    calls: Vec<SinkCall>,
}

/// A pixel sink in memory, shared by its clones: every [`Harness`] connects one. It keeps each
/// buffer's latest pixels and logs each call. It takes one lock on both sides, briefly, which the
/// rule for a host's sink forbids ([`PixelSink`]); a fake isn't a host.
#[derive(Clone, Default)]
pub struct FakePixelSink(Arc<Mutex<FakePixels>>);

impl FakePixelSink {
    /// A buffer's width, height and pixels as last presented ([`Harness::pixels`]).
    pub fn pixels(&self, id: PixelBufferId) -> Option<(u32, u32, Vec<u8>)> {
        self.0.lock().unwrap().buffers.get(&id).cloned()
    }
    /// Every call since the last time this was asked, oldest first.
    pub fn take_calls(&self) -> Vec<SinkCall> {
        std::mem::take(&mut self.0.lock().unwrap().calls)
    }
}

impl PixelSink for FakePixelSink {
    fn create(&self, id: PixelBufferId, width: u32, height: u32) {
        let mut s = self.0.lock().unwrap();
        let blank = vec![0; width as usize * height as usize * 4];
        s.buffers.insert(id, (width, height, blank));
        s.calls.push(SinkCall::Create(id, width, height));
    }
    fn present(&self, id: PixelBufferId, pixels: &[u8], width: u32, damage: PixelRect) {
        let mut s = self.0.lock().unwrap();
        s.calls.push(SinkCall::Present(id, damage));
        let Some((w, _, kept)) = s.buffers.get_mut(&id) else {
            return;
        };
        assert_eq!(*w, width, "a present's width is the buffer's");
        let stride = width as usize * 4;
        let (left, right) = (
            damage.x as usize * 4,
            (damage.x + damage.width) as usize * 4,
        );
        for row in damage.y as usize..(damage.y + damage.height) as usize {
            let at = row * stride;
            kept[at + left..at + right].copy_from_slice(&pixels[at + left..at + right]);
        }
    }
    fn resize(&self, id: PixelBufferId, width: u32, height: u32) {
        let mut s = self.0.lock().unwrap();
        let blank = vec![0; width as usize * height as usize * 4];
        s.buffers.insert(id, (width, height, blank));
        s.calls.push(SinkCall::Resize(id, width, height));
    }
    fn destroy(&self, id: PixelBufferId) {
        let mut s = self.0.lock().unwrap();
        s.buffers.remove(&id);
        s.calls.push(SinkCall::Destroy(id));
    }
}
