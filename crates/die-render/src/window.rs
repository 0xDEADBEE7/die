//! The window and its loop (R3), by S1's scheduling rules. It renders in the run-loop turn
//! that had the input, doesn't draw while occluded, draws nothing when nothing changed,
//! matches the drawable to the view before every frame, and wakes only through the event
//! loop. `Host` is what the loop decides, with no window, so its tests run anywhere.
//! `Shell` is winit's side. Through `Remote`, a check on another thread acts in the loop.

use crate::driver::{self, KeyMonitor, Renderer, Surface};
use crate::font::CellMetrics;
use crate::frame::{Image, Presented};
use crate::input::{Held, KeyFacts, Pointer, caps_event, key_event};
use crate::latency::Latency;
use die_core::{Ctx, Input, KeyEvent, PointerEvent, Scene};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::mpsc;
use std::time::{Duration, Instant};
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalPosition, LogicalSize};
use winit::event::{ElementState, Ime, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::keyboard::PhysicalKey;
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

/// How soon the loop tries again at a frame that got no drawable, or an error.
pub const RETRY_DRAW: Duration = Duration::from_millis(50);

/// How long to sleep with the next timer due at `next` and the clock at `now`. A timer may
/// already be past when the turns end, so then there's no sleep.
pub fn sleep_for(next: Option<Duration>, now: Duration) -> Sleep {
    match next {
        None => Sleep::Forever,
        Some(at) if at <= now => Sleep::No,
        Some(at) => Sleep::For(at - now),
    }
}

/// How long the loop may sleep when `sleep` is what's due and whether a frame it owes failed to
/// draw. Nothing else may wake it, so that frame comes back after [`RETRY_DRAW`].
pub fn sleep_or_retry(sleep: Sleep, undrawn: bool) -> Sleep {
    match sleep {
        Sleep::Forever if undrawn => Sleep::For(RETRY_DRAW),
        Sleep::For(d) if undrawn => Sleep::For(d.min(RETRY_DRAW)),
        other => other,
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

/// Core, as a loop drives it: turns, sleep, and a frame of the split tree. K9's calls replace
/// `step` and `next_timer` here when they land.
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

    /// The frame is now `grid`, which the split tree shares out, less the command line. What a
    /// resize queues (C3) runs on the next pump.
    pub fn set_grid(&mut self, grid: (usize, usize)) {
        self.grid = grid;
        self.ctx.set_frame_size(grid.0, grid.1);
    }

    /// The frame, with every window of the split tree.
    pub fn scene(&mut self) -> Scene {
        self.ctx.scene()
    }
}

/// What the loop has done, for checks. `Latency` is what it took, for the log.
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
    hidden: bool,
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
    /// The last try at a frame got no drawable, or an error, so the loop mustn't sleep on it.
    undrawn: bool,
    /// Take the keyboard when the app next is the active one.
    key_when_active: bool,
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
    held: Held,
    pointer: Pointer,
    monitor: Option<KeyMonitor>,
    /// Panics the monitor caught that have been logged.
    monitor_panics: u64,
    /// Every key that has gone to core since `record_keys`, for a test.
    recorded: Option<Vec<KeyEvent>>,
    /// The log behind `DIE_LATENCY_LOG`, and what a check reads.
    pub(crate) latency: Latency,
    /// The last frame's id, main-thread ms and GPU ms.
    last_present: (u64, f64, f64),
}

impl Shell {
    /// The editor, for what a [`Remote`] call does there.
    pub fn ctx(&mut self) -> &mut Ctx {
        &mut self.host.ctx
    }

    /// Whether this app is the active one, as far as a window on screen can say.
    pub fn app_active(&self) -> bool {
        self.live.as_ref().is_some_and(|l| l.surface.app_active())
    }

    /// Take the keyboard if this app is, or later becomes, the active one, which takes it from
    /// no other app. A launch in front may not have become active by its first frame.
    pub fn take_focus_when_active(&mut self) {
        self.key_when_active = true;
    }

    /// Take the keyboard focus. Only for when it's already this app's by rights: a restart's
    /// new instance after an old one that had it.
    pub fn take_focus(&self) {
        if let Some(live) = &self.live {
            live.window.focus_window();
        }
    }

    /// Come to the front, from behind other apps or the Dock: only when you've just asked this
    /// instance for something, as `die file` does.
    pub fn come_to_front(&self) {
        if let Some(live) = &self.live {
            live.surface.come_to_front();
        }
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
    /// Keep the window off screen, for a check that doesn't look at it.
    pub hidden: bool,
    /// What to do when the first frame is up.
    pub first_frame: Option<FirstFrame>,
}

/// Open the window and run until it closes or a check ends. The exit code, or why there's
/// no window.
pub fn run(mut ctx: Ctx, options: Options) -> Result<i32, String> {
    driver::ready()?;
    let (mut latency, unopened) = Latency::from_env();
    ctx.messages.extend(unopened);
    latency.stage("core");
    let mut builder = EventLoop::<Wake>::with_user_event();
    if let Err(e) = driver::prepare_loop(&mut builder, options.script.is_none()) {
        ctx.messages.push(e);
    }
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
        hidden: options.hidden,
        first_frame: options.first_frame,
        live: None,
        remote: Remote { proxy },
        cover: None,
        guard: true,
        dirty: true,
        stale: true,
        undrawn: false,
        key_when_active: false,
        resized: false,
        more: false,
        last: None,
        stats: Stats::default(),
        last_error: None,
        failed: None,
        exit_code: 0,
        held: Held::default(),
        pointer: Pointer::new(driver::double_click_interval()),
        monitor: None,
        monitor_panics: 0,
        recorded: None,
        latency,
        last_present: (0, 0.0, 0.0),
    };
    event_loop.run_app(&mut shell).map_err(|e| e.to_string())?;
    shell.drain_presented();
    shell.latency.flush();
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
        let attributes = driver::window_attributes(attributes)
            .with_title(&self.title)
            .with_visible(!self.hidden);
        let window = el.create_window(attributes).map_err(|e| e.to_string())?;
        self.latency.stage("window");
        let scale = window.scale_factor();
        let font = driver::Config {
            scale,
            ..self.font.clone()
        };
        let renderer = Renderer::new(&font)?;
        let surface = renderer.attach(&window, scripted)?;
        self.latency.stage("renderer");
        if scripted || self.latency.logging() {
            surface.watch_presents();
        }
        window.set_ime_allowed(true);
        self.host.ctx.handle(Input::Focus(window.has_focus()));
        let wake = self.remote.proxy.clone();
        // Where there's none to install, winit gives Caps Lock itself.
        self.monitor = KeyMonitor::install(move || {
            let _ = wake.send_event(Wake::Core);
        })
        .ok();
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
        self.guard = !scripted || self.hidden;
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
        let key_at = self.monitor.as_ref().and_then(KeyMonitor::take_key_at);
        let start = key_at.unwrap_or_else(Instant::now);
        let f0 = self.stats.frames;
        self.drain_presented();
        self.send(ev);
        self.settle();
        if self.stats.frames > f0 {
            let (id, cpu_ms, gpu_ms) = self.last_present;
            self.latency.key(id, start.elapsed(), cpu_ms, gpu_ms);
        }
    }

    /// Hand the log what has reached the screen since the last call.
    pub(crate) fn drain_presented(&mut self) {
        if let Some(live) = &self.live {
            self.latency.shown(live.surface.take_presented());
        }
    }

    fn send(&mut self, ev: KeyEvent) {
        if let Some(keys) = &mut self.recorded {
            keys.push(ev.clone());
        }
        self.host.ctx.input(ev);
    }

    /// Keep every key that goes to core from now on, for `take_keys`.
    pub fn record_keys(&mut self) {
        self.recorded = Some(vec![]);
    }

    /// The keys kept since the last call.
    pub fn take_keys(&mut self) -> Vec<KeyEvent> {
        let keys = self.recorded.as_mut().map(std::mem::take);
        keys.unwrap_or_default()
    }

    /// What's been logged so far.
    pub fn messages(&self) -> &[String] {
        &self.host.ctx.messages
    }

    /// The key monitor, where there is one.
    pub fn key_monitor(&self) -> Option<&KeyMonitor> {
        self.monitor.as_ref()
    }

    /// Have the monitor hand every key event to this window, key or not
    /// (`KeyMonitor::deliver_to`).
    pub fn deliver_keys_here(&self) {
        if let (Some(monitor), Some(live)) = (&self.monitor, &self.live) {
            monitor.deliver_to(&live.surface);
        }
    }

    /// Have the view get `text` as an input method gives it (`Surface::insert_text`).
    pub fn insert_text(&self, text: &str) {
        if let Some(live) = &self.live {
            live.surface.insert_text(text);
        }
    }

    /// What the monitor saw before now, so that it stays in order with the keys.
    fn drain_monitor(&mut self) {
        let Some(monitor) = &self.monitor else {
            return;
        };
        let (toggles, caps, panics) = (monitor.take_caps(), monitor.caps(), monitor.panics());
        let inserted = monitor.take_inserted();
        for on in toggles {
            self.held.set_caps(on);
            self.send(caps_event(&self.held));
        }
        self.held.set_caps(caps);
        if panics > self.monitor_panics {
            self.monitor_panics = panics;
            let line = "the key monitor panicked, and goes on";
            self.host.ctx.messages.push(line.to_owned());
        }
        for text in inserted {
            self.commit(&text);
        }
    }

    /// Text that came with no key of its own.
    fn commit(&mut self, text: &str) {
        self.send(KeyEvent {
            caps_lock: self.held.caps(),
            ..KeyEvent::committed(text)
        });
    }

    fn key(&mut self, event: &winit::event::KeyEvent, synthetic: bool) {
        self.drain_monitor();
        let facts = KeyFacts::from(event);
        if let PhysicalKey::Code(code) = facts.physical {
            self.held.key(code, facts.down);
        }
        // A key held across a focus change: it counts as held, but nothing was typed.
        if !synthetic {
            self.input(key_event(&facts, &self.held));
        }
    }

    fn point(&mut self, event: Option<PointerEvent>) {
        if let Some(event) = event {
            self.host.ctx.handle(Input::Pointer(event));
            self.settle();
        }
    }

    /// Run what's ready and draw what that changed.
    pub(crate) fn settle(&mut self) {
        let start = Instant::now();
        self.drain_monitor();
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
            self.undrawn = false;
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
            self.undrawn = false;
            return;
        }
        self.dirty = false;
        let scene = self.host.scene();
        if !self.stale && self.last.as_ref() == Some(&scene) {
            return;
        }
        let start = Instant::now();
        let sync = self.resized || live.surface.in_live_resize();
        let presented = live.renderer.present(&scene, &live.surface, sync);
        self.undrawn = !matches!(presented, Ok(Some(_)));
        match presented {
            Ok(Some(presented)) => {
                self.resized = false;
                self.stale = false;
                self.last_error = None;
                let Presented {
                    w,
                    h,
                    image,
                    id,
                    cpu_ms,
                    gpu_ms,
                } = presented;
                live.image = image;
                self.last_present = (id, cpu_ms, gpu_ms);
                if self.stats.frames == 0 {
                    self.latency.stage("first_frame");
                }
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
            // `about_to_wait` wakes again for it.
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
            WindowEvent::Focused(on) => self.host.ctx.handle(Input::Focus(on)),
            WindowEvent::KeyboardInput {
                event,
                is_synthetic,
                ..
            } => return self.key(&event, is_synthetic),
            WindowEvent::ModifiersChanged(modifiers) => {
                return self.held.sync(modifiers.state());
            }
            WindowEvent::Ime(Ime::Commit(text)) => {
                self.commit(&text);
                return self.settle();
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let (now, mods) = (Instant::now(), self.held.mods());
                let down = state == ElementState::Pressed;
                let event = self.pointer.button(button, down, now, mods);
                return self.point(event);
            }
            WindowEvent::CursorMoved { position, .. } => {
                let Some(live) = &self.live else { return };
                let px = (position.x, position.y);
                let cell = live.renderer.cell();
                let event = self
                    .pointer
                    .moved(px, cell, self.host.grid, self.held.mods());
                return self.point(event);
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let Some(live) = &self.live else { return };
                let cell = live.renderer.cell();
                let event = self.pointer.wheel(delta, cell, self.held.mods());
                return self.point(event);
            }
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
        if self.host.ctx.quit_wanted() {
            el.exit();
            return;
        }
        if self.key_when_active && self.app_active() {
            self.key_when_active = false;
            if !self.live.as_ref().is_some_and(|l| l.window.has_focus()) {
                self.take_focus();
            }
        }
        let sleep = self.host.sleep(self.more);
        el.set_control_flow(match sleep_or_retry(sleep, self.undrawn) {
            Sleep::Forever => ControlFlow::Wait,
            Sleep::No => ControlFlow::Poll,
            Sleep::For(d) => Instant::now()
                .checked_add(d)
                .map_or(ControlFlow::Wait, ControlFlow::WaitUntil),
        });
    }
}
