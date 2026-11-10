//! `die --check idle,resize,scale,occlusion,show,...`: the window held to S1's rules, by DIE
//! itself, on its own window. It takes no focus, and only `typing` posts input, to this
//! process and no other. Each check prints one line, `name: ok a=1 b=2`, `name: skip why` or
//! `name: FAIL why`, and any FAIL makes the exit code 1. It reports its numbers, and
//! `apps/die-dev/tests/window.rs` holds them to S1's limits.
//!
//! By S1's check: 1 is `typing`, 2 is `redraw`, 3 is `pixels`, 4 is `idle-60`, 5 is
//! `occlusion-20`, 6 is `scale`, 7 is `resize` and 8 is `firstframe`. 9, fidelity, needs no
//! window, and stays in die-render's `tests/fidelity.rs`, on the same renderer.

use crate::driver;
use crate::latency;
use crate::window::{Remote, Script, Shell, floating_window, unseen};
use die_core::{BufferId, CursorShape, Event, Frame, PixelBufferId, PixelProducer, PixelRect};
use die_core::{Cell, Ctx, Glyph, Key, KeyEvent, Origin, Result as CoreResult, Rgb, Scene};
use die_core::{RenderInfo, ShowIn, Surface, Underline};
use std::collections::HashSet;
use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::sleep;
use std::time::{Duration, Instant};
use winit::dpi::LogicalSize;
use winit::event_loop::ActiveEventLoop;

/// The checks there are.
pub const NAMES: [&str; 12] = [
    "idle",
    "idle-60",
    "resize",
    "scale",
    "occlusion",
    "occlusion-20",
    "show",
    "typing",
    "redraw",
    "pixels",
    "firstframe",
    "firstframe-once",
];

const IDLE: Duration = Duration::from_secs(3);
/// S1's check 4 is this long, which is what makes its CPU figure mean anything.
const LONG_IDLE: Duration = Duration::from_secs(60);
/// How long `idle-60` waits before it measures: a new app wakes about 10 times a second for about
/// 15 s with no event reaching the loop, and core's tick rests 30 s after the last thing.
const LONG_IDLE_SETTLE: Duration = Duration::from_secs(35);
const WAKE_SLICES: u32 = 6;
const TICKS: u32 = 10;
const RESIZE_STEPS: u32 = 120;
const SCALE_CHANGES: usize = 6;
const OCCLUSION_CYCLES: u32 = 3;
/// S1's check 5.
const LONG_OCCLUSION_CYCLES: u32 = 20;
/// S1's check 1.
const KEYS: usize = 500;
/// A key's press, then the wait after its release: about 30 keys a second.
const KEY_DOWN: Duration = Duration::from_millis(5);
const KEY_GAP: Duration = Duration::from_millis(25);
/// Letters and their virtual key codes.
const LETTERS: [(char, u16); 9] = [
    ('a', 0),
    ('s', 1),
    ('d', 2),
    ('f', 3),
    ('g', 5),
    ('h', 4),
    ('j', 38),
    ('k', 40),
    ('l', 37),
];
const I_KEY: (char, u16) = ('i', 34);
const ESC_KEY: (char, u16) = ('\u{1b}', 53);
/// S1's check 2: every cell of a window this size changes, this many times.
const REDRAW_GRID: (usize, usize) = (159, 49);
const REDRAWS: usize = 600;
/// S1's check 3: a producer on a thread of its own presents this many frames, this far apart,
/// about a second's worth at a 240 Hz hand.
const PIXEL_FRAMES: u32 = 240;
const PIXEL_PERIOD: Duration = Duration::from_millis(4);
/// Frames the loop may draw during those presents beyond one each: a resize's, or a late one.
const PIXEL_EXTRA_FRAMES: u64 = 8;
/// S1's check 8: launches, of which the first is cold and doesn't count.
const LAUNCHES: usize = 12;
const BETWEEN_LAUNCHES: Duration = Duration::from_millis(100);
/// How long the window server gets to report a window covered or uncovered.
const SERVER: Duration = Duration::from_secs(3);
/// How long an event has to come through the window server and draw.
const SETTLE: Duration = Duration::from_millis(1000);
/// A timer's or a job's delay, then how long a frame gets after it.
const LATER: Duration = Duration::from_millis(30);
const THEN: Duration = Duration::from_millis(1000);
/// What `show` puts on screen, whatever file was named: its first line is on row 0 as it
/// is, and `x` changes it.
const SHOW_TEXT: &str = "the show check's own line\nand another\n";

enum Verdict {
    Ok(String),
    Skip(String),
}

type Outcome = Result<Verdict, String>;

/// The check script for these names, which must all be in `NAMES`.
pub fn script(names: Vec<String>) -> Script {
    Box::new(move |remote| {
        let mut code = 0;
        for name in names {
            // A check that failed may have left its cover and guard behind.
            let reset = remote.call(|s, _| {
                s.guard = false;
                s.cover = None;
            });
            if reset.is_err() {
                return 1;
            }
            let outcome = match name.as_str() {
                "idle" => idle(remote, IDLE),
                "idle-60" => idle(remote, LONG_IDLE),
                "resize" => resize(remote),
                "scale" => scale(remote),
                "occlusion" => occlusion(remote, OCCLUSION_CYCLES),
                "occlusion-20" => occlusion(remote, LONG_OCCLUSION_CYCLES),
                "show" => show(remote),
                "typing" => typing(remote),
                "redraw" => redraw(remote),
                "pixels" => pixels(remote),
                "firstframe" => firstframe(),
                "firstframe-once" => firstframe_once(remote),
                other => Err(format!("no such check: {other}")),
            };
            match outcome {
                Ok(Verdict::Ok(numbers)) => println!("{name}: ok {numbers}"),
                Ok(Verdict::Skip(why)) => println!("{name}: skip {why}"),
                Err(why) => {
                    println!("{name}: FAIL {why}");
                    code = 1;
                }
            }
        }
        code
    })
}

fn ensure(holds: bool, why: impl FnOnce() -> String) -> Result<(), String> {
    if holds { Ok(()) } else { Err(why()) }
}

fn yes_no(b: bool) -> &'static str {
    if b { "yes" } else { "no" }
}

/// Wait for the window's first frame.
fn first_frame(r: &Remote) -> Result<(), String> {
    let start = Instant::now();
    while r.call(|s, _| s.stats.frames)? == 0 {
        ensure(start.elapsed() < Duration::from_secs(10), || {
            "no frame in 10 s".into()
        })?;
        sleep(Duration::from_millis(5));
    }
    Ok(())
}

/// Whether the window server reports the window occluded as `want`, within `SERVER`.
fn wait_occluded(r: &Remote, want: bool) -> Result<bool, String> {
    let start = Instant::now();
    while start.elapsed() < SERVER {
        if r.call(move |s, _| s.live.as_ref().map(|l| l.surface.occluded()) == Some(want))? {
            return Ok(true);
        }
        sleep(Duration::from_millis(5));
    }
    Ok(false)
}

fn frames(r: &Remote) -> Result<u64, String> {
    r.call(|s, _| s.stats.frames)
}

/// A change to the text that's on screen, in any buffer.
fn edit(ctx: &mut Ctx) -> CoreResult {
    let Some(buf) = ctx.focused_buffer() else {
        return Ok(());
    };
    ctx.text_mut(buf)?
        .tx(Origin::Extension)
        .insert(0, "z")
        .commit()?;
    Ok(())
}

fn row_text(scene: &Scene, r: usize) -> String {
    scene.row(r).iter().map(|c| c.glyph.to_string()).collect()
}

/// A repeat timer's tick, for `idle`: it changes nothing, and stops after `left` more.
fn tick(ctx: &mut Ctx, left: u32, count: Arc<AtomicU32>) -> CoreResult {
    count.fetch_add(1, Ordering::Relaxed);
    if left > 0 {
        let d = Duration::from_millis(20);
        ctx.after(d, move |ctx| tick(ctx, left - 1, count)).detach();
    }
    Ok(())
}

/// This process's CPU time so far, as `ps` has it, to 10 ms.
fn cpu_seconds() -> Result<f64, String> {
    let pid = std::process::id().to_string();
    let args = ["-o", "time=", "-p", pid.as_str()];
    let out = Command::new("ps").args(args).output();
    let out = out.map_err(|e| format!("ps: {e}"))?;
    let text = String::from_utf8_lossy(&out.stdout);
    let mut seconds = 0.0;
    for part in text.trim().split(':') {
        let part: f64 = part.parse().map_err(|_| format!("ps said {text:?}"))?;
        seconds = seconds * 60.0 + part;
    }
    Ok(seconds)
}

/// The value `percent`% of the way up `values`.
fn percentile(values: &mut [f64], percent: usize) -> f64 {
    values.sort_by(f64::total_cmp);
    let i = values.len() * percent / 100;
    values.get(i).copied().unwrap_or(0.0)
}

/// Nothing happens, so nothing draws, and a timer that changes nothing draws nothing either.
/// `os_wakes_per_second` is the OS's count for the whole process, the median of `WAKE_SLICES` parts
/// of the wait: the pointer crossing the window wakes it for real, and that's not what's measured.
fn idle(r: &Remote, how_long: Duration) -> Outcome {
    first_frame(r)?;
    let settle = (how_long == LONG_IDLE).then_some(LONG_IDLE_SETTLE);
    sleep(settle.unwrap_or(Duration::from_millis(300)));
    let read = |s: &mut Shell, _: &_| (s.stats.frames, s.stats.wakeups);
    let (f0, w0) = r.call(read)?;
    let cpu0 = cpu_seconds()?;
    let slice = how_long / WAKE_SLICES;
    let mut last = driver::process_wakeups();
    let mut rates = vec![];
    for _ in 0..WAKE_SLICES {
        sleep(slice);
        let now = driver::process_wakeups();
        rates.push((now - last) as f64 / slice.as_secs_f64());
        last = now;
    }
    let os_wakes = percentile(&mut rates, 50);
    let cpu1 = cpu_seconds()?;
    let (f1, w1) = r.call(read)?;
    let cpu_percent = (cpu1 - cpu0) / how_long.as_secs_f64() * 100.0;
    let (idle_frames, wakeups) = (f1 - f0, w1 - w0);
    if how_long == LONG_IDLE {
        ensure(os_wakes < 1.0, || {
            format!("{os_wakes:.2} wakes a second by the OS's count")
        })?;
    }
    ensure(idle_frames == 0, || {
        format!("{idle_frames} frames while idle")
    })?;
    let ticks = Arc::new(AtomicU32::new(0));
    let count = ticks.clone();
    r.call(move |s, _| {
        let start = move |ctx: &mut Ctx| tick(ctx, TICKS - 1, count);
        s.host.ctx.defer(start);
    })?;
    // No polling: only the timers can be what wakes the loop. The OS may hold a background
    // process's timers back, so they get a while.
    let start = Instant::now();
    while ticks.load(Ordering::Relaxed) < TICKS && start.elapsed() < Duration::from_secs(10) {
        sleep(Duration::from_millis(50));
    }
    let timer_frames = r.call(read)?.0 - f1;
    let ticks = ticks.load(Ordering::Relaxed);
    ensure(timer_frames == 0, || {
        format!("{timer_frames} frames from a timer that changes nothing")
    })?;
    ensure(ticks == TICKS, || {
        format!("only {ticks} of {TICKS} ticks in 10 s")
    })?;
    Ok(Verdict::Ok(format!(
        "frames={idle_frames} wakeups={wakeups} seconds={} timer_frames={timer_frames} ticks={ticks} cpu_percent={cpu_percent:.3} os_wakes_per_second={os_wakes:.2}",
        how_long.as_secs()
    )))
}

/// Every resize gets one frame at the view's size, half drawn in the same turn and half from
/// winit's Resized, and the core window follows.
fn resize(r: &Remote) -> Outcome {
    first_frame(r)?;
    let (mut mismatched, mut missed, mut extra) = (0, 0, 0);
    r.call(|s, _| s.stats.frame_ms.clear())?;
    let mut last = frames(r)?;
    for i in 0..RESIZE_STEPS {
        let w = 420.0 + f64::from((i + 1) % 40) * 3.0;
        let h = 280.0 + f64::from((i + 1) % 25) * 5.0;
        let same_turn = i % 2 == 0;
        let f0 = r.call(move |s, _| {
            let f0 = s.stats.frames;
            if let Some(live) = &s.live {
                let _ = live.window.request_inner_size(LogicalSize::new(w, h));
            }
            if same_turn {
                s.draw();
            }
            f0
        })?;
        extra += f0 - last;
        let start = Instant::now();
        while frames(r)? == f0 && start.elapsed() < SETTLE {
            sleep(Duration::from_millis(1));
        }
        last = frames(r)?;
        if last == f0 {
            missed += 1;
        } else if last > f0 + 1 {
            extra += last - f0 - 1;
        }
        let right = r.call(|s, _| {
            let Some(live) = &s.live else {
                return false;
            };
            let view = live.surface.size(live.scale);
            let (cols, rows) = crate::window::grid(view, live.renderer.cell());
            let window = s.host.ctx.focused();
            let size = window.and_then(|w| s.host.ctx.window_size(w));
            s.stats.drawable == view && size == Some((cols, rows - 2))
        })?;
        if !right {
            mismatched += 1;
        }
    }
    let mut ms = r.call(|s, _| s.stats.frame_ms.clone())?;
    let (p50, p99) = (percentile(&mut ms, 50), percentile(&mut ms, 99));
    ensure(mismatched == 0, || format!("{mismatched} stretched frames"))?;
    ensure(missed == 0, || format!("{missed} resizes without a frame"))?;
    ensure(extra == 0, || format!("{extra} frames too many"))?;
    Ok(Verdict::Ok(format!(
        "steps={RESIZE_STEPS} mismatched={mismatched} missed={missed} extra={extra} p50_ms={p50:.2} p99_ms={p99:.2}"
    )))
}

/// What a scale change did: frames, whether the drawable, cell and grid were right, the
/// cell, and how long it took in ms.
struct ScaleChange {
    frames: u64,
    right: bool,
    cell: (u32, u32),
    ms: f64,
}

/// Each change's first frame has the right drawable, with the cell and grid rebuilt. It
/// calls the scale path directly, since a real change takes a second display.
fn scale(r: &Remote) -> Outcome {
    first_frame(r)?;
    let start = r.call(|s, _| s.live.as_ref().map_or(1.0, |l| l.scale))?;
    let other = if start > 1.5 {
        start / 2.0
    } else {
        start * 2.0
    };
    let (mut wrong, mut worst_ms) = (0, 0f64);
    let mut cells = HashSet::new();
    for i in 0..SCALE_CHANGES {
        let to = if i % 2 == 0 { other } else { start };
        let change = r.call(move |s, _| {
            let f0 = s.stats.frames;
            let t = Instant::now();
            s.set_scale(to);
            s.settle();
            let ms = t.elapsed().as_secs_f64() * 1e3;
            let frames = s.stats.frames - f0;
            let Some(live) = &s.live else {
                return None;
            };
            let view = live.surface.size(to);
            let cell = live.renderer.cell();
            let (cols, rows) = crate::window::grid(view, cell);
            let window = s.host.ctx.focused();
            let size = window.and_then(|w| s.host.ctx.window_size(w));
            let right = s.stats.drawable == view
                && size == Some((cols, rows - 2))
                && (cell.scale - to).abs() < 1e-6
                && live.renderer.rebuilt().len() == rows;
            Some(ScaleChange {
                frames,
                right,
                cell: (cell.w, cell.h),
                ms,
            })
        })?;
        let c = change.ok_or("no window")?;
        if c.frames != 1 || !c.right {
            wrong += 1;
        }
        cells.insert(c.cell);
        worst_ms = worst_ms.max(c.ms);
    }
    ensure(wrong == 0, || format!("{wrong} wrong first frames"))?;
    let mut cells: Vec<_> = cells.into_iter().collect();
    cells.sort();
    let [a, b] = cells[..] else {
        return Err(format!("expected two cells, not {cells:?}"));
    };
    Ok(Verdict::Ok(format!(
        "changes={SCALE_CHANGES} wrong={wrong} cell_a={}x{} cell_b={}x{} worst_ms={worst_ms:.1}",
        a.0, a.1, b.0, b.1
    )))
}

/// Put a floating window over the check's.
fn cover(s: &mut Shell, el: &ActiveEventLoop) -> Result<(), String> {
    let live = s.live.as_ref().ok_or("no window")?;
    let (x, y) = live.window.outer_position().map_or((0, 0), |p| (p.x, p.y));
    let size = live.window.outer_size();
    let k = live.scale;
    let at = (f64::from(x) / k - 20.0, f64::from(y) / k - 20.0);
    let over = (
        f64::from(size.width) / k + 40.0,
        f64::from(size.height) / k + 40.0,
    );
    let attributes = floating_window(at, over).with_visible(!unseen());
    let window = el.create_window(attributes).map_err(|e| e.to_string())?;
    if unseen() {
        driver::unseen(&window)?;
        window.set_visible(true);
    }
    s.cover = Some(window);
    Ok(())
}

/// A window that can't be seen and has drawn nothing draws its first frame anyway, and only
/// that one. This has the frame count start over under a cover, which is the state of a
/// window that opened hidden. How many frames that drew.
fn first_frame_covered(r: &Remote) -> Result<u64, String> {
    r.call(cover)??;
    ensure(wait_occluded(r, true)?, || {
        "the window server never reported it covered".into()
    })?;
    r.call(|s, _| {
        s.stats.frames = 0;
        s.host.ctx.defer(edit);
    })?;
    sleep(Duration::from_millis(100));
    r.call(|s, _| s.host.ctx.defer(edit))?;
    sleep(Duration::from_millis(100));
    let drawn = frames(r)?;
    r.call(|s, _| s.cover = None)?;
    Ok(drawn)
}

/// Covered, nothing draws though the text changed. Uncovered, the change draws with nothing
/// from this check poking the loop, which is why it sleeps rather than polls.
fn occlusion(r: &Remote, cycles: u32) -> Outcome {
    first_frame(r)?;
    r.call(|s, _| {
        s.guard = true;
        s.stats.worst_turn_ms = 0.0;
    })?;
    if !wait_occluded(r, false)? {
        return Ok(Verdict::Skip(
            "the window server shows this window to no one, so there's nothing to cover".into(),
        ));
    }
    let (mut drawn, mut missed) = (0, 0);
    for cycle in 0..cycles {
        r.call(cover)??;
        ensure(wait_occluded(r, true)?, || {
            format!("cycle {cycle}: the window server never reported it covered")
        })?;
        let (f0, skipped) = r.call(|s, _| {
            s.host.ctx.defer(edit);
            (s.stats.frames, s.stats.skipped)
        })?;
        sleep(Duration::from_millis(100));
        let (f1, skipped_now) = r.call(|s, _| (s.stats.frames, s.stats.skipped))?;
        drawn += f1 - f0;
        ensure(skipped_now > skipped, || {
            format!("cycle {cycle}: nothing tried to draw the change")
        })?;
        r.call(|s, _| s.cover = None)?;
        sleep(SETTLE);
        let f2 = frames(r)?;
        if f2 == f1 {
            missed += 1;
        }
    }
    let stall = r.call(|s, _| s.stats.worst_turn_ms)?;
    let first = first_frame_covered(r)?;
    ensure(drawn == 0, || format!("{drawn} frames while covered"))?;
    ensure(missed == 0, || format!("{missed} uncovers drew nothing"))?;
    ensure(first == 1, || {
        format!("a hidden window's first frame drew {first} frames, not 1")
    })?;
    Ok(Verdict::Ok(format!(
        "cycles={cycles} frames_while_covered={drawn} missed_uncover={missed} worst_stall_ms={stall:.1} first_frame_covered={first}"
    )))
}

/// What's on the drawable, against the offscreen path and the text.
struct Shown {
    differ: usize,
    colors: usize,
    /// What row 0 shows and the line it should, if it doesn't.
    text: Option<(String, String)>,
    decorated: bool,
    option_as_alt: bool,
    active: bool,
    key: bool,
}

/// The frame on the drawable now, read back.
fn what_is_shown(s: &mut Shell) -> Result<Shown, String> {
    s.live
        .as_ref()
        .ok_or("no window")?
        .surface
        .read_next_frame()?;
    s.due.stale = true;
    s.draw();
    let scene = s.last.clone().ok_or("no frame yet")?;
    let buf = s.host.ctx.focused_buffer().ok_or("no buffer")?;
    let t = s.host.ctx.text(buf).map_err(|e| e.to_string())?;
    let first = t.rope().line(0).to_string();
    let first = first.trim_end().to_owned();
    let row = row_text(&scene, 0);
    let text = (first.is_empty() || !row.contains(&first)).then_some((row, first));
    let live = s.live.as_mut().ok_or("no window")?;
    let image = live.image.take().ok_or("no pixels came back")?;
    let offscreen = live.renderer.render(&scene, image.w, image.h)?;
    let at = |p: &[u8]| [p[0], p[1], p[2]];
    let on = image.bgra.chunks_exact(4);
    let off = offscreen.bgra.chunks_exact(4);
    let differ = on.clone().zip(off).filter(|(a, b)| at(a) != at(b)).count();
    Ok(Shown {
        differ,
        colors: on.map(at).collect::<HashSet<_>>().len(),
        text,
        decorated: live.window.is_decorated(),
        option_as_alt: crate::driver::option_as_alt(&live.window),
        active: live.surface.app_active(),
        key: live.window.has_focus(),
    })
}

/// Whether a surface attached without `readable` refuses to read a frame back.
fn unreadable_refuses(s: &mut Shell, el: &ActiveEventLoop) -> Result<bool, String> {
    let hidden = crate::window::quiet_window().with_visible(false);
    let window = el.create_window(hidden).map_err(|e| e.to_string())?;
    let live = s.live.as_ref().ok_or("no window")?;
    let surface = live.renderer.attach(&window, false)?;
    Ok(surface.read_next_frame().is_err())
}

/// Put a buffer of a check's own in the focused window. The buffer that was there, to go back to.
fn open_own_buffer(s: &mut Shell, name: &str, text: &str) -> Result<BufferId, String> {
    let ctx = &mut s.host.ctx;
    let back = ctx.focused_buffer().ok_or("no window")?;
    let buf = ctx.create_text(name, text);
    ctx.show(buf, ShowIn::Current).map_err(|e| e.to_string())?;
    Ok(back)
}

/// A buffer of its own is on the drawable, so what was named can't change the verdict: a
/// file reaching the window is `die`'s facade tests' to hold. Input draws in its own turn. A timer and a job's wake each
/// draw with nothing else waking the loop. The window has no border and takes no focus.
fn show(r: &Remote) -> Outcome {
    first_frame(r)?;
    // Told to take the keyboard once active, an app that isn't active takes nothing.
    r.call(|s, _| s.take_focus_when_active())?;
    let refused = r.call(unreadable_refuses)??;
    let back = r.call(|s, _| open_own_buffer(s, "*show*", SHOW_TEXT))??;
    let shown = r.call(|s, _| what_is_shown(s))??;
    let (input_frames, input_shown) = r.call(|s, _| {
        let (f0, before) = (s.stats.frames, s.last.clone());
        s.input(KeyEvent::press(Key::Char('x')));
        (s.stats.frames - f0, s.last != before)
    })?;
    let f0 = r.call(|s, _| {
        s.host.ctx.after(LATER, edit).detach();
        s.stats.frames
    })?;
    sleep(LATER + THEN);
    let f1 = frames(r)?;
    r.call(|s, _| {
        s.host
            .ctx
            .spawn(|| sleep(LATER), |ctx, ()| edit(ctx))
            .detach()
    })?;
    sleep(LATER + THEN);
    let f2 = frames(r)?;
    let (timer_frames, job_frames) = (f1 - f0, f2 - f1);
    // As if macOS never said the window lost the keyboard: it hasn't got it, so the cursor
    // goes hollow anyway.
    r.call(|s, _| s.told_focus(true))?;
    sleep(THEN);
    let (corrected, said) = r.call(|s, _| {
        let hollow = s
            .last
            .as_ref()
            .and_then(|scene| scene.cursors.first().map(|c| c.shape));
        let said = s.host.ctx.messages.last().cloned().unwrap_or_default();
        (hollow == Some(CursorShape::Hollow), said)
    })?;
    r.call(move |s, _| s.host.ctx.show(back, ShowIn::Current).is_ok())?;
    let Shown {
        differ,
        colors,
        text,
        decorated,
        option_as_alt,
        active,
        key,
    } = shown;
    ensure(refused, || {
        "a surface that isn't readable took a read".into()
    })?;
    ensure(differ == 0, || {
        format!("{differ} pixels differ from the offscreen path")
    })?;
    ensure(colors >= 2, || "one flat color".into())?;
    ensure(text.is_none(), || {
        format!("the buffer's first line isn't on row 0: {text:?}")
    })?;
    ensure(input_frames == 1 && input_shown, || {
        format!("input drew {input_frames} frames in its turn, changed: {input_shown}")
    })?;
    ensure(timer_frames == 1, || {
        format!("a timer drew {timer_frames} frames")
    })?;
    ensure(job_frames == 1, || {
        format!("a job's wake drew {job_frames} frames")
    })?;
    ensure(!decorated && option_as_alt, || {
        format!("decorated={decorated} option_as_alt={option_as_alt}")
    })?;
    ensure(!active && !key, || {
        format!("took focus: active={active} key={key}")
    })?;
    ensure(corrected && said.contains("lost the keyboard"), || {
        format!("told it had the keyboard, it believed it: {said:?}")
    })?;
    Ok(Verdict::Ok(format!(
        "pixels_differ={differ} colors={colors} text=yes unreadable_refused=yes input_frames={input_frames} input_shown={} timer_frames={timer_frames} job_frames={job_frames} decorated={decorated} option_as_alt={option_as_alt} active={active} key={key} focus_corrected=yes",
        yes_no(input_shown),
    )))
}

/// Press and release one key, through AppKit's queue and the key monitor as far as the view.
fn tap(r: &Remote, (c, vk): (char, u16)) -> Result<(), String> {
    r.call(move |_, _| driver::post_key(vk, c, true))??;
    sleep(KEY_DOWN);
    r.call(move |_, _| driver::post_key(vk, c, false))??;
    sleep(KEY_GAP);
    Ok(())
}

/// Put a new layer on the window, readable or not.
fn reattach(s: &mut Shell, readable: bool) -> Result<(), String> {
    s.drain_presented();
    let live = s.live.as_mut().ok_or("no window")?;
    let surface = live.renderer.attach(&live.window, readable)?;
    surface.resize(surface.size(live.scale), live.scale);
    surface.watch_presents();
    live.surface = surface;
    s.due.stale = true;
    Ok(())
}

/// S1's check 1: real key events, one at a time, each to its frame and on to the display, on
/// a layer that can't be read, as a window with no check has. A
/// window this small isn't the one S1's display figures are for, and M0 has no pixel buffer,
/// so the hand run has the first and M3 the second. No key is lost or out of order.
fn typing(r: &Remote) -> Outcome {
    first_frame(r)?;
    if r.call(|s, _| s.key_monitor().is_none())? {
        return Ok(Verdict::Skip("no key monitor to deliver keys".into()));
    }
    let back = r.call(|s, _| {
        reattach(s, false)?;
        s.latency.keep = true;
        s.deliver_keys_here();
        open_own_buffer(s, "*typing*", "")
    })??;
    tap(r, I_KEY)?;
    sleep(SETTLE);
    r.call(|s, _| {
        s.drain_presented();
        s.latency.flush();
        s.latency.samples.clear();
    })?;
    let mut typed = String::new();
    for i in 0..KEYS {
        let key = LETTERS[i % LETTERS.len()];
        typed.push(key.0);
        tap(r, key)?;
    }
    sleep(SETTLE);
    let (text, samples) = r.call(|s, _| {
        s.drain_presented();
        s.latency.flush();
        s.latency.keep = false;
        let line = s.host.ctx.focused_buffer().and_then(|buf| {
            let text = s.host.ctx.text(buf).ok()?;
            Some(text.rope().line(0).to_string())
        });
        (line, std::mem::take(&mut s.latency.samples))
    })?;
    tap(r, ESC_KEY)?;
    r.call(move |s, _| {
        let _ = s.host.ctx.show(back, ShowIn::Current);
        reattach(s, true)
    })??;
    ensure(text.as_deref().map(str::trim_end) == Some(&typed), || {
        format!("typed {typed:?}, and the buffer has {text:?}")
    })?;
    ensure(samples.len() == KEYS, || {
        format!("{} of {KEYS} keys drew a frame", samples.len())
    })?;
    let mut frame: Vec<f64> = samples.iter().map(|k| k.to_frame_ms).collect();
    let mut shown: Vec<f64> = samples.iter().filter_map(|k| k.to_shown_ms).collect();
    Ok(Verdict::Ok(format!(
        "keys={KEYS} p50_ms={:.2} p99_ms={:.2} shown={} shown_p50_ms={:.2} shown_p99_ms={:.2}",
        percentile(&mut frame, 50),
        percentile(&mut frame, 99),
        shown.len(),
        percentile(&mut shown, 50),
        percentile(&mut shown, 99),
    )))
}

/// A full grid in which every cell's glyph, colors and decoration differ from frame `n - 1`'s.
fn full_grid(s: &mut Shell, n: usize) -> Scene {
    let (cols, rows) = REDRAW_GRID;
    let mut scene = s.host.ctx.compose(cols, rows, &[]);
    scene.cursors.clear();
    for (i, cell) in scene.cells.iter_mut().enumerate() {
        let at = i + n;
        let glyph = Glyph::Char(char::from(b'a' + (at % 26) as u8));
        let shade = (at % 200) as u8;
        let fg = Rgb(255 - shade, 250, 240);
        *cell = Cell::new(glyph, fg, Rgb(shade, 20, 30));
        cell.underline = Underline::Single;
        cell.underline_color = fg;
    }
    scene
}

/// What each full redraw took on the main thread and the GPU, in ms, on a hidden window of
/// S1's size with no read back, so your screen is left alone.
fn full_redraws(s: &mut Shell, el: &ActiveEventLoop) -> Result<(Vec<f64>, Vec<f64>), String> {
    let (cols, rows) = REDRAW_GRID;
    let hidden = crate::window::quiet_window().with_visible(false);
    let window = el.create_window(hidden).map_err(|e| e.to_string())?;
    let scenes: Vec<Scene> = (0..REDRAWS).map(|n| full_grid(s, n)).collect();
    let live = s.live.as_mut().ok_or("no window")?;
    let surface = live.renderer.attach(&window, false)?;
    let cell = live.renderer.cell();
    let px = (cols as u32 * cell.w, rows as u32 * cell.h);
    surface.resize(px, live.scale);
    let (mut cpu, mut gpu) = (vec![], vec![]);
    for scene in &scenes {
        let frame = live.renderer.present(scene, &surface, false)?;
        let frame = frame.ok_or("no drawable for a hidden window")?;
        cpu.push(frame.cpu_ms);
        gpu.push(frame.gpu_ms);
    }
    s.due.stale = true;
    s.due.dirty = true;
    Ok((cpu, gpu))
}

/// S1's check 2.
fn redraw(r: &Remote) -> Outcome {
    first_frame(r)?;
    let (mut cpu, mut gpu) = r.call(full_redraws)??;
    let (cols, rows) = REDRAW_GRID;
    Ok(Verdict::Ok(format!(
        "grid={cols}x{rows} frames={} cpu_p99_ms={:.2} gpu_p99_ms={:.2}",
        cpu.len(),
        percentile(&mut cpu, 99),
        percentile(&mut gpu, 99),
    )))
}

/// The `pixels` check's surface: it places one buffer over its whole area, made at its first
/// resize and handed to the check's producer thread, as the whiteboard's does.
struct Canvas {
    id: Option<PixelBufferId>,
    producer: Arc<Mutex<Option<PixelProducer>>>,
}

impl Surface for Canvas {
    fn kind(&self) -> &'static str {
        "pixels-check"
    }

    fn render(&mut self, frame: &mut Frame, _: &RenderInfo) {
        if let Some(id) = self.id {
            frame.place(id, -1);
        }
    }

    fn event(&mut self, ev: &Event, ctx: &mut Ctx) -> bool {
        let Event::Resize {
            cols, rows, cell, ..
        } = ev
        else {
            return false;
        };
        if self.id.is_none() {
            let (w, h) = (*cols as u32 * cell.width, *rows as u32 * cell.height);
            if let Ok((id, producer)) = ctx.pixel_buffer(w, h) {
                self.id = Some(id);
                *self.producer.lock().unwrap_or_else(|e| e.into_inner()) = Some(producer);
            }
        }
        false
    }
}

/// Frame `n`'s color, premultiplied RGBA at full alpha, as the producer fills its buffer.
fn pattern(n: u32) -> [u8; 4] {
    [(n * 7 % 200 + 55) as u8, (n * 3 % 200) as u8, 90, 255]
}

/// What the placement shows, read back from the drawable: how many pixels of the whole frame
/// differ from the offscreen path's, how many are inside the placement, and how many of those
/// aren't `want`.
fn what_is_placed(s: &mut Shell, want: [u8; 3]) -> Result<(usize, usize, usize), String> {
    let live = s.live.as_ref().ok_or("no window")?;
    live.surface.read_next_frame()?;
    s.due.stale = true;
    s.draw();
    let scene = s.last.clone().ok_or("no frame yet")?;
    let live = s.live.as_mut().ok_or("no window")?;
    let image = live.image.take().ok_or("no pixels came back")?;
    let offscreen = live.renderer.render(&scene, image.w, image.h)?;
    let on = image.bgra.chunks_exact(4);
    let off = offscreen.bgra.chunks_exact(4);
    let differ = on.zip(off).filter(|(a, b)| a[..3] != b[..3]).count();
    let p = scene
        .placements
        .first()
        .ok_or("no placement in the scene")?;
    let cell = live.renderer.cell();
    let (x0, y0) = (p.col as u32 * cell.w, p.row as u32 * cell.h);
    let x1 = (x0 + p.cols as u32 * cell.w).min(image.w);
    let y1 = (y0 + p.rows as u32 * cell.h).min(image.h);
    let (mut inside, mut wrong) = (0, 0);
    for y in y0..y1 {
        for x in x0..x1 {
            inside += 1;
            wrong += usize::from(image.rgb(x, y) != want);
        }
    }
    Ok((differ, inside, wrong))
}

/// S1's check 3, on DIE's own path (M3a): a producer on a thread of its own presents
/// `PIXEL_FRAMES` frames into a buffer the size of the window's area, placed under the glyphs.
/// The presents draw frames with no key, timer or job, the main thread copies no pixels, the
/// last frame's pattern is on the drawable where the placement is and the offscreen path draws
/// the same, and dropping the producer takes the placement away.
fn pixels(r: &Remote) -> Outcome {
    first_frame(r)?;
    if r.call(|s, _| s.pixels.is_none())? {
        return Ok(Verdict::Skip("no pixel sink on this platform".into()));
    }
    let handed = Arc::new(Mutex::new(None));
    let slot = handed.clone();
    let back = r.call(move |s, _| {
        let canvas = Canvas {
            id: None,
            producer: slot,
        };
        let ctx = &mut s.host.ctx;
        let back = ctx.focused_buffer().ok_or("no window")?;
        let buf = ctx.create_surface("*pixels*", canvas);
        ctx.show(buf, ShowIn::Current).map_err(|e| e.to_string())?;
        s.stats.frame_ms.clear();
        s.stats.worst_turn_ms = 0.0;
        Ok::<BufferId, String>(back)
    })??;
    // The surface hears its size on the loop's next turn, and makes its buffer then.
    let start = Instant::now();
    let mut producer = loop {
        let handed = handed.lock().unwrap_or_else(|e| e.into_inner()).take();
        if let Some(p) = handed {
            break p;
        }
        ensure(start.elapsed() < SETTLE, || {
            "no pixel buffer in a second".into()
        })?;
        sleep(Duration::from_millis(5));
    };
    let read = |s: &mut Shell, _: &_| {
        let stats = s.pixels.as_ref().map(|p| p.stats());
        (s.stats.frames, stats.unwrap_or_default())
    };
    let (f0, s0) = r.call(read)?;
    let (w, h) = producer.size();
    let fed = std::thread::spawn(move || {
        for n in 0..PIXEL_FRAMES {
            let color = pattern(n);
            for px in producer.pixels_mut().chunks_exact_mut(4) {
                px.copy_from_slice(&color);
            }
            producer.present(PixelRect::whole(w, h));
            sleep(PIXEL_PERIOD);
        }
        producer
    });
    let producer = fed.join().map_err(|_| "the producer panicked")?;
    sleep(LATER + LATER);
    let (f1, s1) = r.call(read)?;
    let last = pattern(PIXEL_FRAMES - 1);
    let placed = r.call(move |s, _| what_is_placed(s, [last[0], last[1], last[2]]))??;
    let (differ, inside, wrong) = placed;
    let (mut ms, worst) = r.call(|s, _| (s.stats.frame_ms.clone(), s.stats.worst_turn_ms))?;
    drop(producer);
    let gone = r.call(move |s, _| {
        s.settle();
        let gone = s.last.as_ref().is_some_and(|l| l.placements.is_empty());
        let _ = s.host.ctx.show(back, ShowIn::Current);
        gone
    })?;
    let (presents, main_bytes) = (s1.presents - s0.presents, s1.main_bytes - s0.main_bytes);
    let frames = f1 - f0;
    ensure(presents == u64::from(PIXEL_FRAMES), || {
        format!("{presents} of {PIXEL_FRAMES} presents reached the sink")
    })?;
    ensure(main_bytes == 0, || {
        format!("the main thread copied {main_bytes} bytes")
    })?;
    // One frame per present the main thread took (a superseded one rode the next), and none
    // without one: the flag, not the scene, is what draws them.
    let superseded = s1.superseded - s0.superseded;
    ensure(frames + superseded >= presents, || {
        format!("{frames} frames for {presents} presents, {superseded} superseded")
    })?;
    ensure(frames <= presents + PIXEL_EXTRA_FRAMES, || {
        format!("{frames} frames for {presents} presents: more than one each")
    })?;
    ensure(inside > 0 && wrong == 0, || {
        format!("{wrong} of {inside} pixels under the placement aren't the last frame's")
    })?;
    ensure(differ == 0, || {
        format!("{differ} pixels differ from the offscreen path")
    })?;
    ensure(gone, || {
        "the placement stayed after its producer went".into()
    })?;
    Ok(Verdict::Ok(format!(
        "presents={presents} frames={frames} superseded={} ring_waits={} main_bytes={main_bytes} producer_bytes={} blits={} blit_bytes={} buffer={w}x{h} placed_px={inside} wrong={wrong} pixels_differ={differ} frame_p99_ms={:.2} worst_turn_ms={worst:.1} gone={}",
        s1.superseded - s0.superseded,
        s1.ring_waits - s0.ring_waits,
        s1.producer_bytes - s0.producer_bytes,
        s1.blits - s0.blits,
        s1.blit_bytes - s0.blit_bytes,
        percentile(&mut ms, 99),
        yes_no(gone),
    )))
}

/// One launch's end: the first frame reached the display, or the display dropped it and
/// another is on its way. How long ago it got there, for `firstframe`.
fn firstframe_once(r: &Remote) -> Outcome {
    first_frame(r)?;
    let start = Instant::now();
    while start.elapsed() < THEN {
        let shown = r.call(|s, _| {
            s.drain_presented();
            if s.latency.first_dropped && s.latency.first_shown.is_none() {
                s.latency.first_dropped = false;
                s.due.stale = true;
            }
            s.latency.first_shown
        })?;
        if let Some(at) = shown {
            let lag_ms = at.elapsed().as_secs_f64() * 1e3;
            return Ok(Verdict::Ok(format!("lag_ms={lag_ms:.2}")));
        }
        sleep(Duration::from_millis(1));
    }
    Err("the first frame never reached the display".into())
}

/// How long one launch took to get a frame on the display, in ms.
fn launch(exe: &std::path::Path) -> Result<f64, String> {
    let start = Instant::now();
    let child = Command::new(exe)
        .args(["--check", "firstframe-once"])
        .env_remove(latency::ENV)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn();
    let mut child = child.map_err(|e| e.to_string())?;
    let stdout = child.stdout.take().ok_or("no stdout")?;
    let mut line = String::new();
    let read = BufReader::new(stdout).read_line(&mut line);
    let took = start.elapsed().as_secs_f64() * 1e3;
    read.map_err(|e| e.to_string())?;
    let status = child.wait().map_err(|e| e.to_string())?;
    let lag = line.trim_end().rsplit_once("lag_ms=");
    let lag = lag.and_then(|(_, ms)| ms.parse::<f64>().ok());
    match lag {
        Some(lag) if status.success() => Ok(took - lag),
        _ => Err(format!("a launch said {line:?}, and {status}")),
    }
}

/// S1's check 8: DIE starts itself over and over, and times exec to the first frame on the
/// display from outside. None of the windows takes focus.
fn firstframe() -> Outcome {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let mut warm = vec![];
    for run in 0..LAUNCHES {
        let ms = launch(&exe)?;
        if run > 0 {
            warm.push(ms);
        }
        sleep(BETWEEN_LAUNCHES);
    }
    let (median, p90) = (percentile(&mut warm, 50), percentile(&mut warm, 90));
    let worst = warm.last().copied().unwrap_or(0.0);
    Ok(Verdict::Ok(format!(
        "launches={} median_ms={median:.1} p90_ms={p90:.1} worst_ms={worst:.1}",
        warm.len()
    )))
}
