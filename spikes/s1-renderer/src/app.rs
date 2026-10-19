//! The winit application: window + CAMetalLayer, file view, status line,
//! typing line, the pixel-buffer split, render scheduling, and a small
//! script runner that drives the automated benches app-side.

use crate::gpu::{Gpu, RenderOpts, Target, MAX_IN_FLIGHT};
use crate::keys::{self, Action, DieKey};
use crate::pixbuf::{self, Shared};
use crate::platform::{self, Inject};
use crate::scene::{self, CursorShape, Grid, Placement, Scene, Style};
use crate::stats::{now_s, J};
use objc2::rc::Retained;
use objc2::MainThreadMarker;
use objc2_app_kit::{NSView, NSWindow};
use objc2_metal::{MTLPixelFormat, MTLTexture};
use objc2_quartz_core::CAMetalLayer;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalPosition, LogicalSize};
use winit::event::{ElementState, Modifiers, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoopProxy};
use winit::platform::macos::{OptionAsAlt, WindowAttributesExtMacOS};
use winit::window::{Window, WindowId};

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum WinStyle { Borderless, Titlebar }

#[derive(Clone)]
pub struct Opts {
    pub style: WinStyle, pub font_files: [PathBuf; 4], pub font_label: String, pub pt: f64, pub render: RenderOpts,
    pub guard: bool, pub fix_keys: bool, pub display_sync: bool, pub drawables: usize, pub pix_layer: bool, pub pix_full: bool, pub pwt: bool, pub frame: Option<[f64; 4]>, pub reconcile: bool, pub flush: bool, pub split: bool, pub fullscreen: bool, pub activate: bool, pub file: PathBuf, pub keys_target: usize,
}

#[derive(Debug)]
pub enum UserEvent { Wake, Ping(f64) }

pub type Step = Box<dyn FnOnce(&mut App, &ActiveEventLoop)>;
pub enum Act { Wait(f64), Do(Step) }
pub fn act(f: impl FnOnce(&mut App, &ActiveEventLoop) + 'static) -> Act { Act::Do(Box::new(f)) }

pub struct Win { pub window: Window, pub view: Retained<NSView>, pub ns: Retained<NSWindow>, pub layer: Retained<CAMetalLayer>, pub pix_layer: Option<Retained<CAMetalLayer>> }

pub struct Pix { pub shared: Arc<Shared>, pub handle: Option<std::thread::JoinHandle<Vec<f64>>> }

#[derive(Default)]
pub struct Counters { pub reconciled: u64, pub rendered: u64, pub skipped_occluded: u64, pub skipped_inflight: u64, pub no_drawable: u64, pub wakeups: u64, pub occluded_events: u64 }

pub struct App {
    pub opts: Opts,
    pub proxy: Arc<Mutex<EventLoopProxy<UserEvent>>>,
    pub win: Option<Win>,
    pub gpu: Option<Gpu>,
    pub scene: Scene,
    pub lines: Vec<String>,
    pub top: usize,
    pub typed: String,
    pub scale: f64,
    pub px: (u32, u32),
    pub mods: Modifiers,
    pub pix: Option<Pix>,
    pub needs_render: bool,
    pub deferred: bool,
    pub in_resize: bool,
    pub pending_keys: Vec<(u64, f64, f64)>,
    pub key_seq: u64,
    pub keys: Vec<DieKey>,
    pub mouse: Vec<(f64, String)>,
    pub typing: bool,
    pub script: VecDeque<Act>,
    pub deadline: Option<f64>,
    pub c: Counters,
    pub ping: Arc<Mutex<Vec<f64>>>,
    pub ping_stop: Arc<AtomicBool>,
    pub status: String,
    pub resize_sync: bool,
    pub resize_log: Vec<J>,
    pub scale_log: Vec<(f64, u64, (u32, u32), u32, (u32, u32))>,
    pub cover: Option<Retained<NSWindow>>,
    pub on_ready: Option<Box<dyn FnOnce(&mut App)>>,
    pub on_key: Option<Box<dyn FnMut(&mut App, &DieKey)>>,
    pub tick: Option<Box<dyn FnMut(&mut App, &ActiveEventLoop)>>,
    pub t_main: f64,
    pub marks: Vec<(&'static str, f64)>,
    pub exit: bool,
    pub extra: Vec<(String, J)>,
    pub kv: std::collections::HashMap<String, f64>,
    pub samples: std::collections::HashMap<String, Vec<f64>>,
    pub raw: Vec<platform::RawEv>,
    pub matrix_rows: Vec<J>,
    pub cover_win: Option<Window>,
}

impl App {
    pub fn new(opts: Opts, proxy: EventLoopProxy<UserEvent>, t_main: f64) -> App {
        let lines = std::fs::read_to_string(&opts.file).unwrap_or_else(|_| "// file not found".into()).lines().map(|s| s.to_string()).collect();
        App { opts, proxy: Arc::new(Mutex::new(proxy)), win: None, gpu: None, scene: Scene { grid: Grid::new(1, 1), cursor: None, placement: None },
            lines, top: 0, typed: String::new(), scale: 2.0, px: (0, 0), mods: Modifiers::default(), pix: None, needs_render: true, deferred: false, in_resize: false,
            pending_keys: vec![], key_seq: 0, keys: vec![], mouse: vec![], typing: true, script: VecDeque::new(), deadline: None,
            c: Counters::default(), ping: Arc::new(Mutex::new(vec![])), ping_stop: Arc::new(AtomicBool::new(true)), status: String::new(),
            resize_sync: false, resize_log: vec![], scale_log: vec![], cover: None, on_ready: None, on_key: None, tick: None, t_main, marks: vec![], exit: false, extra: vec![],
            kv: Default::default(), samples: Default::default(), raw: vec![], matrix_rows: vec![], cover_win: None }
    }

    pub fn wake_fn(&self) -> Arc<dyn Fn() + Send + Sync> {
        let p = self.proxy.clone();
        Arc::new(move || { let _ = p.lock().unwrap().send_event(UserEvent::Wake); })
    }

    fn create(&mut self, el: &ActiveEventLoop) {
        let mon = el.primary_monitor().or_else(|| el.available_monitors().next());
        let (mw, mh, ms) = mon.as_ref().map(|m| (m.size().width as f64, m.size().height as f64, m.scale_factor())).unwrap_or((3024.0, 1964.0, 2.0));
        let (lw, lh) = if self.opts.fullscreen { (mw / ms, mh / ms) } else { (1200.0, 800.0) };
        let [fx, fy, fw, fh] = self.opts.frame.unwrap_or([if self.opts.fullscreen { 0.0 } else { (mw / ms - lw) / 2.0 }, 0.0, lw, lh]);
        let mut a = Window::default_attributes().with_title("DIE S1 spike").with_inner_size(LogicalSize::new(fw, fh))
            .with_position(LogicalPosition::new(fx, fy))
            .with_option_as_alt(OptionAsAlt::Both).with_active(self.opts.activate);
        a = match self.opts.style {
            WinStyle::Borderless => a.with_decorations(false),
            WinStyle::Titlebar => a.with_titlebar_transparent(true).with_title_hidden(true).with_titlebar_buttons_hidden(true).with_fullsize_content_view(true).with_movable_by_window_background(true),
        };
        let window = el.create_window(a).expect("window");
        self.marks.push(("window_created", now_s()));
        let gpu = Gpu::new(&self.opts.font_files, self.opts.pt, window.scale_factor(), self.opts.render, self.wake_fn()).unwrap_or_else(|e| panic!("{e}"));
        self.marks.push(("gpu_ready", now_s()));
        let view = platform::ns_view(&window);
        let ns = platform::ns_window(&view);
        let layer = CAMetalLayer::new();
        layer.setDevice(Some(&gpu.device));
        layer.setPixelFormat(MTLPixelFormat::BGRA8Unorm);
        layer.setOpaque(true);
        layer.setFramebufferOnly(true);
        layer.setMaximumDrawableCount(self.opts.drawables);
        layer.setDisplaySyncEnabled(self.opts.display_sync);
        layer.setPresentsWithTransaction(self.opts.pwt);
        platform::attach_layer(&view, &layer);
        self.scale = window.scale_factor();
        self.px = platform::set_layer_size(&layer, &view, self.scale);
        let mtm = MainThreadMarker::new().unwrap();
        platform::install_monitor(mtm);
        platform::FIX_KEYS.with(|f| f.set(self.opts.fix_keys));
        platform::install_beep_counter();
        let ph = platform::disable_press_and_hold();
        self.extra.push(("press_and_hold_enabled_after_register".into(), J::B(ph)));
        self.gpu = Some(gpu);
        let pix_layer = self.opts.pix_layer.then(|| {
            let pl = CAMetalLayer::new();
            pl.setDevice(Some(&self.gpu.as_ref().unwrap().device));
            pl.setPixelFormat(MTLPixelFormat::BGRA8Unorm);
            pl.setOpaque(true);
            pl.setMaximumDrawableCount(self.opts.drawables);
            layer.addSublayer(&pl);
            pl
        });
        self.win = Some(Win { window, view, ns, layer, pix_layer });
        self.layout();
        if let Some(f) = self.on_ready.take() { f(self); }
    }

    pub fn win(&self) -> &Win { self.win.as_ref().unwrap() }
    pub fn gpu(&mut self) -> &mut Gpu { self.gpu.as_mut().unwrap() }
    pub fn mtm(&self) -> MainThreadMarker { MainThreadMarker::new().unwrap() }

    /// Recomputes the grid from the drawable size and refills everything.
    pub fn layout(&mut self) {
        let (cw, chh) = self.gpu.as_ref().unwrap().cell();
        let (cols, rows) = ((self.px.0 / cw).max(4) as usize, (self.px.1 / chh).max(3) as usize);
        self.scene.grid = Grid::new(cols, rows);
        let fv_cols = if self.opts.split { if self.opts.pix_full { 0 } else { cols / 2 } } else { cols };
        self.scene.placement = if self.opts.split { Some(Placement { col: fv_cols, row: 0, cols: cols - fv_cols, rows: rows - 2 }) } else { None };
        scene::fill_file_view(&mut self.scene.grid, &self.lines, self.top, fv_cols, rows - 2, 0);
        self.draw_typing();
        self.draw_status();
        let (px_w, px_h) = (self.px.0.saturating_sub(fv_cols as u32 * cw), (rows as u32 - 2) * chh);
        let gpu = self.gpu.as_mut().unwrap();
        if self.opts.split {
            gpu.pix_rect = Some([(fv_cols as u32 * cw) as f32, 0.0, px_w as f32, px_h as f32]);
            if let Some(pl) = self.win.as_ref().and_then(|w| w.pix_layer.as_ref()) {
                let s = self.scale;
                let (x, y) = ((fv_cols as u32 * cw) as f64 / s, (self.px.1 - px_h) as f64 / s);
                objc2_quartz_core::CATransaction::begin();
                objc2_quartz_core::CATransaction::setDisableActions(true);
                pl.setFrame(objc2_core_foundation::CGRect::new(objc2_core_foundation::CGPoint::new(x, y), objc2_core_foundation::CGSize::new(px_w as f64 / s, px_h as f64 / s)));
                pl.setContentsScale(s);
                pl.setDrawableSize(objc2_core_foundation::CGSize::new(px_w as f64, px_h as f64));
                objc2_quartz_core::CATransaction::commit();
            }
            if gpu.pix_tex.as_ref().map(|t| (t.width() as u32, t.height() as u32)) != Some((px_w, px_h)) { gpu.new_pix_tex(px_w, px_h); }
            if let Some(p) = &self.pix { p.shared.resize(px_w, px_h, self.scale); }
        } else { gpu.pix_rect = None; }
        self.needs_render = true;
    }

    pub fn pix_size(&self) -> (u32, u32) { self.gpu.as_ref().and_then(|g| g.pix_rect).map(|r| (r[2] as u32, r[3] as u32)).unwrap_or((0, 0)) }

    pub fn draw_typing(&mut self) {
        let g = &mut self.scene.grid;
        let r = g.rows - 2;
        g.clear_row(r, 0, g.cols, 0x262626ff);
        let c = g.put(r, 0, "insert> ", Style { fg: 0x98c379ff, bg: 0x262626ff, ..Default::default() }, g.cols);
        let c = g.put(r, c, &self.typed, Style { fg: scene::FG, bg: 0x262626ff, ..Default::default() }, g.cols);
        self.scene.cursor = Some((r, c.min(g.cols - 1), CursorShape::Bar));
    }

    pub fn draw_status(&mut self) {
        let g = &mut self.scene.grid;
        let r = g.rows - 1;
        let st = Style { fg: 0x1c1c1cff, bg: 0x8fbcbbff, ..Default::default() };
        g.clear_row(r, 0, g.cols, 0x8fbcbbff);
        let s = format!(" DIE S1 | {} | {}x{} cells {}x{}px @{}x | font {} | {}", self.opts.style_name(), g.cols, g.rows, self.px.0, self.px.1, self.scale, self.opts.font_label, self.status);
        g.put(r, 0, &s, st, g.cols);
    }

    pub fn set_status(&mut self, s: impl Into<String>) { self.status = s.into(); self.draw_status(); self.needs_render = true; }

    /// Help text shown in the file-view area (interactive modes).
    pub fn show_text(&mut self, text: &str) {
        let g = &mut self.scene.grid;
        let cols = if self.opts.split { g.cols / 2 } else { g.cols };
        for r in 0..g.rows - 2 { g.clear_row(r, 0, cols, scene::BG); }
        for (i, l) in text.lines().enumerate().take(g.rows - 2) {
            let st = if l.starts_with('#') { Style { fg: 0xe5c07bff, bg: scene::BG, attrs: scene::attr::BOLD, ..Default::default() } } else { Style::fg(scene::FG) };
            g.put(i, 1, l.trim_start_matches('#'), st, cols);
        }
        self.needs_render = true;
    }

    /// Renders now unless occluded (guard) or too many frames are in flight.
    pub fn render(&mut self, tag: &'static str) -> Option<u64> {
        let Some(w) = self.win.as_ref() else { return None };
        // Never draw at a stale size: if the view changed size and winit's
        // Resized hasn't arrived yet, take the resize path (synchronous) instead.
        if self.opts.reconcile && !self.in_resize && platform::backing_px(&w.view, self.scale) != self.px {
            self.c.reconciled += 1;
            return self.on_resize(true, tag).map(|r| r.0);
        }
        if self.opts.guard && platform::occluded(&w.ns) { self.c.skipped_occluded += 1; return None; }
        let gpu = self.gpu.as_mut().unwrap();
        if gpu.in_flight.load(Ordering::SeqCst) >= MAX_IN_FLIGHT { self.c.skipped_inflight += 1; self.deferred = true; return None; }
        if let Some(pl) = w.pix_layer.clone() {
            // experiment: pixel buffer on its own layer; text frames only when text changed
            if let Some(b) = self.pix.as_ref().and_then(|p| p.shared.take()) { gpu.render_pix_layer(&pl, b); }
            if !self.scene.grid.dirty.iter().any(|d| *d) && self.pending_keys.is_empty() && self.c.rendered > 0 { self.needs_render = false; return None; }
            let saved = gpu.pix_rect.take();
            let keys = self.pending_keys.clone();
            let layer = w.layer.clone();
            let r = gpu.render(&mut self.scene, Target::Layer(&layer), None, keys, tag);
            gpu.pix_rect = saved;
            if r.is_some() { self.pending_keys.clear(); self.c.rendered += 1; self.needs_render = false; }
            return r;
        }
        let blit = self.pix.as_ref().and_then(|p| p.shared.take());
        let keys = self.pending_keys.clone();
        let layer = w.layer.clone();
        match gpu.render(&mut self.scene, Target::Layer(&layer), blit, keys, tag) {
            Some(id) => {
                self.pending_keys.clear();
                self.c.rendered += 1;
                self.needs_render = false;
                let exp = platform::backing_px(&w.view, self.scale);
                if let Some(f) = gpu.log.lock().unwrap().get(id) { f.expected = exp; }
                Some(id)
            }
            None => { self.c.no_drawable += 1; None }
        }
    }

    /// Resize path: new drawable size, new grid. With `sync`, render inside
    /// the same CA transaction (presentsWithTransaction) so nothing stretches.
    pub fn on_resize(&mut self, sync: bool, tag: &'static str) -> Option<(u64, (u32, u32))> {
        let w = self.win.as_ref()?;
        let exp = platform::backing_px(&w.view, self.scale);
        if exp == self.px { return None; }
        self.px = platform::set_layer_size(&w.layer, &w.view, self.scale);
        let layer = w.layer.clone();
        self.layout();
        if !sync { return None; }
        layer.setPresentsWithTransaction(true);
        self.in_resize = true;
        let id = self.render(tag);
        self.in_resize = false;
        layer.setPresentsWithTransaction(self.opts.pwt);
        // Commit now: winit delivers events after Core Animation's commit
        // observer, and a second transaction-bound drawable in the same
        // transaction makes nextDrawable block for its 1 s timeout.
        if self.opts.flush { objc2_quartz_core::CATransaction::flush(); }
        id.map(|i| (i, exp))
    }

    /// Backing-scale change: font size, cell metrics and atlas are rebuilt,
    /// the layer gets the new contentsScale and drawableSize, and the pixel
    /// buffer is told its new physical size. Renders immediately.
    pub fn on_scale(&mut self, scale: f64, tag: &'static str) -> Option<u64> {
        let t = now_s();
        self.scale = scale;
        self.gpu().set_scale(scale).unwrap();
        let w = self.win.as_ref().unwrap();
        self.px = platform::set_layer_size(&w.layer, &w.view, scale);
        self.layout();
        let id = self.render(tag);
        let gen = self.gpu.as_ref().unwrap().atlas_gen;
        let cell = self.gpu.as_ref().unwrap().cell();
        self.scale_log.push((t, id.unwrap_or(0), self.px, gen, cell));
        id
    }

    pub fn start_producer(&mut self) {
        if self.pix.as_ref().is_some_and(|p| p.handle.is_some()) { return; }
        let (w, h) = self.pix_size();
        let max = (self.px.0 * self.px.1 * 4) as usize;
        let shared = Shared::new(&self.gpu.as_ref().unwrap().device, w, h, max.max((w * h * 4) as usize));
        let handle = pixbuf::spawn(shared.clone(), 60.0, self.wake_fn());
        self.pix = Some(Pix { shared, handle: Some(handle) });
    }

    pub fn stop_producer(&mut self) -> Vec<f64> {
        let Some(p) = self.pix.as_mut() else { return vec![] };
        p.shared.stop.store(true, Ordering::Relaxed);
        p.handle.take().map(|h| h.join().unwrap()).unwrap_or_default()
    }

    pub fn start_ping(&mut self, every_ms: u64) {
        self.ping_stop.store(false, Ordering::Relaxed);
        let (p, stop) = (self.proxy.clone(), self.ping_stop.clone());
        std::thread::spawn(move || while !stop.load(Ordering::Relaxed) {
            let _ = p.lock().unwrap().send_event(UserEvent::Ping(now_s()));
            std::thread::sleep(std::time::Duration::from_millis(every_ms));
        });
    }

    pub fn post(&self, ev: Inject, flags: usize) -> f64 { platform::post(self.mtm(), &self.win().ns, ev, flags) }

    fn handle_key(&mut self, ev: winit::event::KeyEvent) {
        let t_h = now_s();
        let ts = platform::LAST_KEYDOWN_TS.with(|c| c.get());
        let dk = keys::from_winit(&ev, &self.mods, if ev.state == ElementState::Pressed { ts } else { t_h });
        self.keys.push(dk.clone());
        if let Some(mut f) = self.on_key.take() { f(self, &dk); if self.on_key.is_none() { self.on_key = Some(f); } }
        if dk.action == Action::Release { return; }
        let cmd = dk.mods & keys::m::SUPER != 0;
        if cmd && (dk.code == "KeyQ" || dk.code == "KeyW") { self.exit = true; return; }
        if !self.typing { return; }
        let ctrl = dk.mods & keys::m::CTRL != 0;
        match dk.code.as_str() {
            "Backspace" => { self.typed.pop(); }
            "Enter" => self.typed.clear(),
            "PageDown" if !cmd => { self.top = (self.top + self.scene.grid.rows - 3).min(self.lines.len().saturating_sub(1)); self.layout(); }
            "PageUp" if !cmd => { self.top = self.top.saturating_sub(self.scene.grid.rows - 3); self.layout(); }
            _ => match &dk.text { Some(t) if !ctrl && !cmd && t.chars().all(|c| !c.is_control()) => self.typed.push_str(t), _ => return },
        }
        if self.typed.chars().count() + 10 > self.scene.grid.cols { self.typed.clear(); }
        self.key_seq += 1;
        self.pending_keys.push((self.key_seq, ts, t_h));
        self.draw_typing();
        self.needs_render = true;
    }

    /// Moves the monitor's raw events into the app. With the monitor fixes
    /// on, Caps Lock (which winit drops, #4769) becomes a DIE key event here.
    fn drain_raw(&mut self) {
        let raw = platform::RAW.with(|r| std::mem::take(&mut *r.borrow_mut()));
        let fix = platform::FIX_KEYS.with(|f| f.get());
        for r in raw {
            if fix && r.kind == "flags" && r.code == 57 {
                let on = r.flags & (1 << 16) != 0;
                self.keys.push(DieKey { t: r.ts, action: if on { Action::Press } else { Action::Release }, code: "CapsLock".into(), logical: "CapsLock".into(),
                    unshifted: None, mods: keys::mods_of(&self.mods) | if on { keys::m::CAPS } else { 0 }, text: None, source: "monitor" });
            }
            self.raw.push(r);
        }
    }

    fn run_script(&mut self, el: &ActiveEventLoop) {
        loop {
            match self.script.front() {
                None => { self.deadline = None; return; }
                Some(Act::Wait(d)) => {
                    let d = *d;
                    match self.deadline {
                        None => { self.deadline = Some(now_s() + d); return; }
                        Some(t) if now_s() >= t => { self.deadline = None; self.script.pop_front(); }
                        Some(_) => return,
                    }
                }
                Some(Act::Do(_)) => { if let Some(Act::Do(f)) = self.script.pop_front() { f(self, el); } }
            }
        }
    }
}

/// Builds the event loop (no default menu), runs `setup`, runs the app, returns it.
pub fn with_loop(o: Opts, t_main: f64, activate: bool, setup: impl FnOnce(&mut App)) -> App {
    use winit::platform::macos::EventLoopBuilderExtMacOS;
    let el = winit::event_loop::EventLoop::<UserEvent>::with_user_event().with_default_menu(false).with_activate_ignoring_other_apps(activate).build().unwrap();
    let mut app = App::new(o, el.create_proxy(), t_main);
    setup(&mut app);
    el.run_app(&mut app).unwrap();
    app
}

impl Opts {
    pub fn style_name(&self) -> &'static str { match self.style { WinStyle::Borderless => "borderless", WinStyle::Titlebar => "hidden-titlebar" } }
}

impl ApplicationHandler<UserEvent> for App {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.win.is_none() { self.create(el); }
    }

    fn window_event(&mut self, el: &ActiveEventLoop, id: WindowId, ev: WindowEvent) {
        if self.win.as_ref().map(|w| w.window.id()) != Some(id) { return; }
        match ev {
            WindowEvent::KeyboardInput { event, .. } => self.handle_key(event),
            WindowEvent::ModifiersChanged(m) => self.mods = m,
            WindowEvent::Resized(_) => {
                let live = self.win().ns.inLiveResize();
                let t = now_s();
                if let Some((fid, exp)) = self.on_resize(live || self.resize_sync, "resize") {
                    self.resize_log.push(J::obj(vec![("t", J::N(t)), ("frame", J::N(fid as f64)), ("w", J::N(exp.0 as f64)), ("h", J::N(exp.1 as f64)), ("live", J::B(live)), ("ms", J::N((now_s() - t) * 1e3))]));
                } else if live { self.resize_log.push(J::obj(vec![("t", J::N(t)), ("frame", J::Null), ("live", J::B(true))])); }
            }
            // winit can report an unchanged factor (e.g. when the window first lands on a screen)
            WindowEvent::ScaleFactorChanged { scale_factor, .. } if (scale_factor - self.scale).abs() > 1e-6 => { self.on_scale(scale_factor, "scale"); }
            WindowEvent::Occluded(o) => { self.c.occluded_events += 1; if !o { self.needs_render = true; } }
            WindowEvent::RedrawRequested => self.needs_render = true,
            WindowEvent::CloseRequested => self.exit = true,
            WindowEvent::MouseInput { state, .. } => self.mouse.push((now_s(), format!("{state:?}"))),
            _ => {}
        }
        if self.exit { el.exit(); }
    }

    fn user_event(&mut self, _el: &ActiveEventLoop, ev: UserEvent) {
        match ev {
            UserEvent::Wake => { if self.deferred || self.pix.as_ref().is_some_and(|p| p.shared.has_pending()) { self.needs_render = true; self.deferred = false; } }
            UserEvent::Ping(t) => self.ping.lock().unwrap().push((now_s() - t) * 1e3),
        }
    }

    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        self.c.wakeups += 1;
        if self.win.is_none() { return; }
        self.drain_raw();
        self.run_script(el);
        if let Some(mut t) = self.tick.take() { t(self, el); if self.tick.is_none() { self.tick = Some(t); } }
        if self.needs_render { self.render("main"); }
        if self.exit { self.ping_stop.store(true, Ordering::Relaxed); self.stop_producer(); el.exit(); return; }
        match self.deadline {
            Some(t) => el.set_control_flow(ControlFlow::WaitUntil(std::time::Instant::now() + std::time::Duration::from_secs_f64((t - now_s()).max(0.0)))),
            None => el.set_control_flow(ControlFlow::Wait),
        }
    }
}
