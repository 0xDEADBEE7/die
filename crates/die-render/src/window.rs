//! The window and its loop (R3), by S1's scheduling rules. It renders in the run-loop turn
//! that had the input, doesn't draw while occluded, draws nothing when nothing changed,
//! matches the drawable to the view before every frame, and wakes only through the event
//! loop. `Host` is what the loop decides, with no window, so its tests run anywhere.
//! `Shell` is winit's side. Through `Remote`, a check on another thread acts in the loop.
#![warn(missing_docs)]

use crate::driver::{self, Renderer, Surface};
use crate::font::CellMetrics;
use crate::frame::{Image, Presented};
use die_core::{Ctx, KeyEvent, Scene, WindowPlacement};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::mpsc;
use std::time::{Duration, Instant};
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalPosition, LogicalSize};
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::window::{Window, WindowAttributes, WindowId, WindowLevel};

/// The turns one pump may take. Past it the loop draws, reads its events and comes back, so
/// that a call that keeps queueing another can't hold it.
pub const TURN_LIMIT: usize = 1024;
/// A window's logical size, until something tiles it.
const SIZE: (f64, f64) = (1200.0, 800.0);
/// A check's window: small, near the corner, and floating, so it can be seen whatever else
/// is on screen. It doesn't take focus.
const CHECK_SIZE: (f64, f64) = (420.0, 280.0);
/// How many frame times `Stats` keeps.
const FRAME_TIMES: usize = 4096;

/// How long the loop may sleep.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Sleep {
    /// Until something wakes it.
    Forever,
    /// For this long, then a timer is due.
    For(Duration),
    /// Not at all.
    No,
}

/// How long to sleep with the next timer due at `next` and the clock at `now`. A timer may
/// already be past when the turns end, so then there's no sleep.
pub fn sleep_for(next: Option<Duration>, now: Duration) -> Sleep {
    match next {
        None => Sleep::Forever,
        Some(at) if at <= now => Sleep::No,
        Some(at) => Sleep::For(at - now),
    }
}

/// The grid for a drawable `px` pixels in size: whole cells, with room for a text row, a
/// status line and the command line whatever the size.
pub fn grid(px: (u32, u32), cell: &CellMetrics) -> (usize, usize) {
    let cols = (px.0 / cell.w.max(1)).max(1);
    let rows = (px.1 / cell.h.max(1)).max(3);
    (cols as usize, rows as usize)
}

/// Whether occlusion holds a frame back: with the guard on, a window that can't be seen
/// doesn't draw, except its first frame, so that it has its contents when it's uncovered
/// and whatever waits for a first frame isn't kept waiting.
pub fn holds_frame(guard: bool, frames: u64, occluded: bool) -> bool {
    guard && frames > 0 && occluded
}

/// What a pump did.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Pumped {
    /// The turns it took.
    pub ran: usize,
    /// It stopped at `TURN_LIMIT`, with more ready.
    pub more: bool,
}

/// Core, as a loop drives it: turns, sleep, and a frame of one window. K9's calls replace
/// `step`, `next_timer` and `compose` here when they land.
pub struct Host {
    /// The editor.
    pub ctx: Ctx,
    grid: (usize, usize),
}

impl Host {
    /// A host with no grid yet.
    pub fn new(ctx: Ctx) -> Host {
        Host { ctx, grid: (1, 3) }
    }

    /// Take turns until nothing is ready, or `TURN_LIMIT`.
    pub fn pump(&mut self) -> Pumped {
        for ran in 0..TURN_LIMIT {
            if !self.ctx.step() {
                return Pumped { ran, more: false };
            }
        }
        Pumped {
            ran: TURN_LIMIT,
            more: true,
        }
    }

    /// How long the loop may sleep, given whether the last pump left `more`.
    pub fn sleep(&mut self, more: bool) -> Sleep {
        if more {
            return Sleep::No;
        }
        sleep_for(self.ctx.next_timer(), self.ctx.now())
    }

    /// The frame is now `grid`, so the window has that less its status line and the command
    /// line. What a resize queues (C3) runs on the next pump.
    pub fn set_grid(&mut self, grid: (usize, usize)) {
        self.grid = grid;
        let Some(window) = self.ctx.focused() else {
            return;
        };
        let (cols, rows) = (grid.0, grid.1.saturating_sub(2));
        if let Err(e) = self.ctx.resize(window, cols, rows) {
            self.ctx.messages.push(e.to_string());
        }
    }

    /// The frame, with the focused window filling it.
    pub fn scene(&mut self) -> Scene {
        let placement = self.ctx.focused().map(|window| WindowPlacement {
            window,
            col: 0,
            row: 0,
        });
        let placed = placement.as_slice();
        self.ctx.compose(self.grid.0, self.grid.1, placed)
    }
}

/// What the loop has done, for checks and later for R6's log.
#[derive(Clone, Default, Debug)]
pub struct Stats {
    /// Frames presented.
    pub frames: u64,
    /// Times the loop woke.
    pub wakeups: u64,
    /// Frames not drawn because the window was occluded.
    pub skipped: u64,
    /// The last frame's drawable.
    pub drawable: (u32, u32),
    /// The last frames' times in ms, up to `FRAME_TIMES` since they were last cleared.
    pub frame_ms: Vec<f64>,
    /// The longest a single turn of the loop took, in ms.
    pub worst_turn_ms: f64,
}

/// The window, and what draws on it.
pub(crate) struct Live {
    pub(crate) window: Window,
    pub(crate) surface: Surface,
    pub(crate) renderer: Renderer,
    pub(crate) scale: f64,
    /// The drawable's size.
    px: (u32, u32),
    /// The pixels of the last frame the surface was asked to read back.
    pub(crate) image: Option<Image>,
}

/// The event loop's side: core, the window, and when to draw.
pub struct Shell {
    pub(crate) host: Host,
    font: driver::Config,
    title: String,
    script: Option<Script>,
    first_frame: Option<FirstFrame>,
    pub(crate) live: Option<Live>,
    remote: Remote,
    /// A second window of a check's.
    pub(crate) cover: Option<Window>,
    /// Don't draw while occluded. A check for something else can turn it off, since it can't
    /// choose what you have on screen.
    pub(crate) guard: bool,
    /// Something may have changed, so compose a frame and see.
    pub(crate) dirty: bool,
    /// What's on the drawable no longer stands, whatever the scene is.
    pub(crate) stale: bool,
    /// The drawable has a new size, or it's about to be presented in a resize.
    resized: bool,
    /// The last pump left more ready.
    more: bool,
    pub(crate) last: Option<Scene>,
    pub(crate) stats: Stats,
    /// The last error drawing, so it's logged only once in a row.
    last_error: Option<String>,
    failed: Option<String>,
    exit_code: i32,
}

impl Shell {
    /// The editor, for what a [`Remote`] call does there.
    pub fn ctx(&mut self) -> &mut Ctx {
        &mut self.host.ctx
    }
}

/// What wakes the loop.
pub(crate) enum Wake {
    /// Core's waker.
    Core,
    /// A check's call.
    Call(Call),
    /// A check is done.
    Exit(i32),
}

type Call = Box<dyn FnOnce(&mut Shell, &ActiveEventLoop) + Send>;

/// A check: it runs on a thread of its own, and acts through `Remote`. Its result is the
/// process's exit code.
pub type Script = Box<dyn FnOnce(&Remote) -> i32 + Send>;

/// Acts in the loop from another thread.
#[derive(Clone)]
pub struct Remote {
    proxy: EventLoopProxy<Wake>,
}

impl Remote {
    /// Run `f` in the loop, before the turn's pump and frame, and wait for what it returns.
    pub fn call<T: Send + 'static>(
        &self,
        f: impl FnOnce(&mut Shell, &ActiveEventLoop) -> T + Send + 'static,
    ) -> Result<T, String> {
        let (tx, rx) = mpsc::channel();
        let call: Call = Box::new(move |shell, el| {
            let _ = tx.send(f(shell, el));
        });
        let stopped = |_| "the loop stopped".to_owned();
        self.proxy.send_event(Wake::Call(call)).map_err(stopped)?;
        rx.recv().map_err(|_| "the loop stopped".to_owned())
    }

    /// End the loop, so that `run` returns `code`.
    pub fn exit(&self, code: i32) {
        let _ = self.proxy.send_event(Wake::Exit(code));
    }
}

/// Runs in the loop, once, when the first frame is up. Anything that may block goes on a
/// thread of its own.
pub type FirstFrame = Box<dyn FnOnce(&Remote)>;

/// What a window opens with.
pub struct Options {
    /// The font: its scale becomes the window's.
    pub font: driver::Config,
    /// The window's title, which only a window manager sees.
    pub title: String,
    /// Run this check rather than wait for you. Then the app has no Dock icon, and the
    /// window is a small one that floats.
    pub script: Option<Script>,
    /// What to do when the first frame is up.
    pub first_frame: Option<FirstFrame>,
}

/// Open the window and run until it closes or a check ends. The exit code, or why there's
/// no window.
pub fn run(mut ctx: Ctx, options: Options) -> Result<i32, String> {
    driver::ready()?;
    let mut builder = EventLoop::<Wake>::with_user_event();
    driver::prepare_loop(&mut builder, options.script.is_none());
    let event_loop = builder.build().map_err(|e| e.to_string())?;
    let proxy = event_loop.create_proxy();
    let wake = proxy.clone();
    ctx.set_waker(move || {
        let _ = wake.send_event(Wake::Core);
    })
    .map_err(|e| e.to_string())?;
    let mut shell = Shell {
        host: Host::new(ctx),
        font: options.font,
        title: options.title,
        script: options.script,
        first_frame: options.first_frame,
        live: None,
        remote: Remote { proxy },
        cover: None,
        guard: true,
        dirty: true,
        stale: true,
        resized: false,
        more: false,
        last: None,
        stats: Stats::default(),
        last_error: None,
        failed: None,
        exit_code: 0,
    };
    event_loop.run_app(&mut shell).map_err(|e| e.to_string())?;
    shell.host.ctx.shutdown();
    match shell.failed {
        Some(e) => Err(e),
        None => Ok(shell.exit_code),
    }
}

/// What every window starts from: no border, and it opens without activating the app or
/// taking focus.
pub(crate) fn quiet_window() -> WindowAttributes {
    Window::default_attributes()
        .with_decorations(false)
        .with_active(false)
}

/// Where a check's window goes, by its process, so that two checks running at once don't
/// cover each other, which would fail the occlusion check.
fn check_position(pid: u32) -> (f64, f64) {
    let step = |n: u32| f64::from(n) * 60.0;
    (40.0 + step(pid % 16), 60.0 + step(pid / 16 % 8))
}

/// A check's window attributes: floating, at `at`, `size` in points.
pub(crate) fn floating_window(at: (f64, f64), size: (f64, f64)) -> WindowAttributes {
    quiet_window()
        .with_position(LogicalPosition::new(at.0, at.1))
        .with_inner_size(LogicalSize::new(size.0, size.1))
        .with_window_level(WindowLevel::AlwaysOnTop)
}

impl Shell {
    fn open(&mut self, el: &ActiveEventLoop) -> Result<(), String> {
        let scripted = self.script.is_some();
        let attributes = if scripted {
            floating_window(check_position(std::process::id()), CHECK_SIZE)
        } else {
            quiet_window().with_inner_size(LogicalSize::new(SIZE.0, SIZE.1))
        };
        let attributes = driver::window_attributes(attributes).with_title(&self.title);
        let window = el.create_window(attributes).map_err(|e| e.to_string())?;
        let scale = window.scale_factor();
        let font = driver::Config {
            scale,
            ..self.font.clone()
        };
        let renderer = Renderer::new(&font)?;
        let surface = renderer.attach(&window, scripted)?;
        self.live = Some(Live {
            window,
            surface,
            renderer,
            scale,
            px: (0, 0),
            image: None,
        });
        // A check can't choose what's on your screen, so it draws anyway unless it's
        // checking occlusion.
        self.guard = !scripted;
        self.settle();
        if let Some(script) = self.script.take() {
            let remote = self.remote.clone();
            std::thread::spawn(move || {
                let code = catch_unwind(AssertUnwindSafe(|| script(&remote))).unwrap_or(1);
                remote.exit(code);
            });
        }
        Ok(())
    }

    /// One key from the OS: it runs and draws now, in this run-loop turn.
    pub fn input(&mut self, ev: KeyEvent) {
        self.host.ctx.input(ev);
        self.settle();
    }

    /// Run what's ready and draw what that changed.
    pub(crate) fn settle(&mut self) {
        let start = Instant::now();
        let pumped = self.host.pump();
        self.more = pumped.more;
        if pumped.ran > 0 {
            self.dirty = true;
        }
        if self.dirty || self.stale {
            self.draw();
        }
        let ms = start.elapsed().as_secs_f64() * 1e3;
        self.stats.worst_turn_ms = self.stats.worst_turn_ms.max(ms);
    }

    /// Draw if a frame is due: the drawable first matches the view, and then, unless the
    /// window is occluded, a scene that differs from what's on it is presented.
    pub(crate) fn draw(&mut self) {
        let Some(live) = &mut self.live else {
            return;
        };
        let px = live.surface.size(live.scale);
        if px.0 == 0 || px.1 == 0 {
            return;
        }
        if px != live.px {
            live.px = px;
            live.surface.resize(px, live.scale);
            self.host.set_grid(grid(px, live.renderer.cell()));
            self.host.pump();
            self.stale = true;
            self.resized = true;
        }
        if holds_frame(self.guard, self.stats.frames, live.surface.occluded()) {
            self.stats.skipped += 1;
            self.dirty = true;
            return;
        }
        self.dirty = false;
        let scene = self.host.scene();
        if !self.stale && self.last.as_ref() == Some(&scene) {
            return;
        }
        let start = Instant::now();
        let sync = self.resized || live.surface.in_live_resize();
        match live.renderer.present(&scene, &live.surface, sync) {
            Ok(Some(presented)) => {
                self.resized = false;
                self.stale = false;
                self.last_error = None;
                let Presented { w, h, image } = presented;
                live.image = image;
                self.stats.frames += 1;
                self.stats.drawable = (w, h);
                if self.stats.frame_ms.len() < FRAME_TIMES {
                    let ms = start.elapsed().as_secs_f64() * 1e3;
                    self.stats.frame_ms.push(ms);
                }
                self.last = Some(scene);
                if let Some(first_frame) = self.first_frame.take() {
                    first_frame(&self.remote);
                }
            }
            // The next wake tries again.
            Ok(None) => self.stale = true,
            Err(e) => {
                self.stale = true;
                if self.last_error.as_ref() != Some(&e) {
                    self.host.ctx.messages.push(e.clone());
                    self.last_error = Some(e);
                }
            }
        }
    }

    /// The window is on a display of a new scale, so the font and the atlas start over, and
    /// the next frame has the new cell and drawable.
    pub(crate) fn set_scale(&mut self, scale: f64) {
        let Some(live) = &mut self.live else {
            return;
        };
        // winit can report an unchanged factor, when a window first lands on a screen.
        if (scale - live.scale).abs() < 1e-6 {
            return;
        }
        if let Err(e) = live.renderer.set_scale(scale) {
            self.host.ctx.messages.push(e);
            return;
        }
        live.scale = scale;
        live.px = (0, 0);
        self.dirty = true;
    }
}

impl ApplicationHandler<Wake> for Shell {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.live.is_some() {
            return;
        }
        if let Err(e) = self.open(el) {
            self.failed = Some(e);
            el.exit();
        }
    }

    fn window_event(&mut self, el: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        if self.live.as_ref().map(|live| live.window.id()) != Some(id) {
            return;
        }
        match event {
            WindowEvent::Resized(_) | WindowEvent::RedrawRequested => self.dirty = true,
            WindowEvent::Occluded(false) => self.dirty = true,
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => self.set_scale(scale_factor),
            WindowEvent::CloseRequested => el.exit(),
            _ => return,
        }
        self.settle();
    }

    fn user_event(&mut self, el: &ActiveEventLoop, wake: Wake) {
        match wake {
            Wake::Core => {}
            Wake::Call(call) => call(self, el),
            Wake::Exit(code) => {
                self.exit_code = code;
                el.exit();
                return;
            }
        }
        self.settle();
    }

    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        self.stats.wakeups += 1;
        self.settle();
        el.set_control_flow(match self.host.sleep(self.more) {
            Sleep::Forever => ControlFlow::Wait,
            Sleep::No => ControlFlow::Poll,
            Sleep::For(d) => Instant::now()
                .checked_add(d)
                .map_or(ControlFlow::Wait, ControlFlow::WaitUntil),
        });
    }
}
