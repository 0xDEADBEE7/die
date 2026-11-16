//! The jellyfish's buffer, which is what `swim` and `main` were in your program: the clock, the
//! size and the cells, and when it swims.

use crate::sea::{Extras, Sea, Tones};
use crate::{KIND, TICK};
use die_core::{
    BufferId, Color, Ctx, Event, Frame, FrameCell, Glyph, Key, KeyEvent, Mods, RenderInfo, Rgb,
    Sub, Surface,
};
use std::time::Duration;

/// The most one tick adds to the show, as `swim`'s `clock += dt.min(0.25)`: after a stall it
/// neither jumps ahead nor replays what it missed.
const STALL: Duration = Duration::from_millis(250);

/// What keeps it going. Each state holds its handles, so leaving one cancels what it ran (C4),
/// and a killed buffer takes them all with it.
enum Life {
    /// No window shows it: nothing runs.
    Hidden,
    /// Swimming: a tick every [`TICK`], and the wait for `rest_after` without a key.
    Swimming { _tick: Sub, _idle: Option<Sub> },
    /// At rest, a still picture that costs nothing: your next command's end wakes it.
    Resting { _stir: Sub },
}

/// The buffer: the sea, its clock, and what keeps it going.
pub(crate) struct Swim {
    sea: Sea,
    /// Its own buffer, which the extension sets once there is one.
    pub(crate) buf: BufferId,
    /// How long it has swum, which is the moment drawn.
    t: Duration,
    /// The clock at the last tick, or at the wake before it.
    last: Duration,
    /// The theme's colours, read at each tick: `render` has no `Ctx` to ask.
    tones: Tones,
    extras: Extras,
    rest_after: Option<Duration>,
    life: Life,
    /// What the frame core keeps already shows: the moment, columns, rows, a cell's shape, the
    /// tones and the extras.
    drawn: Option<(Duration, usize, usize, f64, Tones, Extras)>,
}

/// The theme's `default` face as the sea takes colours, with core's own where the theme names
/// none.
pub(crate) fn tones(ctx: &Ctx) -> Tones {
    let face = ctx.face_style("default").unwrap_or_default();
    let hex = |Rgb(r, g, b): Rgb| u32::from(r) << 16 | u32::from(g) << 8 | u32::from(b);
    Tones::for_theme(
        hex(face.bg.unwrap_or(Rgb(0x10, 0x10, 0x10))),
        hex(face.fg.unwrap_or(Rgb(0xd0, 0xd0, 0xd0))),
    )
}

/// Whether a window shows `buf`.
fn shown(ctx: &Ctx, buf: BufferId) -> bool {
    let windows = ctx.window_list();
    windows.iter().any(|w| ctx.window_buffer(*w) == Some(buf))
}

/// Esc with no modifier, which does nothing in normal state and says nothing.
fn plain_esc(key: &KeyEvent) -> bool {
    key.key == Key::Esc && key.mods == Mods::default()
}

impl Swim {
    /// A jellyfish at its first spark, not yet swimming.
    pub(crate) fn new(tones: Tones, extras: Extras, rest_after: Option<Duration>) -> Swim {
        Swim {
            sea: Sea::new(),
            buf: BufferId::default(),
            t: Duration::ZERO,
            last: Duration::ZERO,
            tones,
            extras,
            rest_after,
            life: Life::Hidden,
            drawn: None,
        }
    }

    /// Swim, if a window shows it: from where it stopped, the time away not counted. Called when
    /// `jellyfish` has shown it, when a window takes it, and at anything you do to it or, while
    /// it rests, to DIE.
    pub(crate) fn wake(&mut self, ctx: &mut Ctx) {
        if matches!(self.life, Life::Swimming { .. }) {
            return;
        }
        let buf = self.buf;
        if !shown(ctx, buf) {
            self.life = Life::Hidden;
            return;
        }
        self.last = ctx.now();
        // Gone if the buffer was killed meanwhile, which is nothing to say.
        let tick = ctx.every(TICK, move |ctx| {
            let _ = ctx.with_surface(buf, |s: &mut Swim, ctx| s.tick(ctx));
            Ok(())
        });
        let idle = self.rest_after.map(|d| {
            ctx.idle(d, move |ctx| {
                let _ = ctx.with_surface(buf, |s: &mut Swim, ctx| s.rest(ctx));
                Ok(())
            })
        });
        self.life = Life::Swimming {
            _tick: tick,
            _idle: idle,
        };
    }

    /// One frame on, in the theme's colours as they are now; or with no window showing it,
    /// nothing running until one does.
    fn tick(&mut self, ctx: &mut Ctx) {
        if !shown(ctx, self.buf) {
            self.life = Life::Hidden;
            return;
        }
        let now = ctx.now();
        self.t += now.saturating_sub(self.last).min(STALL);
        self.last = now;
        self.tones = tones(ctx);
    }

    /// No key for `rest_after`: the picture stays as it is, and nothing runs until a command
    /// ends.
    fn rest(&mut self, ctx: &mut Ctx) {
        let buf = self.buf;
        let stir = ctx.on_command_ended(move |ctx| {
            let _ = ctx.with_surface(buf, |s: &mut Swim, ctx| s.wake(ctx));
            Ok(())
        });
        self.life = Life::Resting { _stir: stir };
    }
}

impl Surface for Swim {
    fn kind(&self) -> &'static str {
        KIND
    }

    fn render(&mut self, frame: &mut Frame, info: &RenderInfo) {
        let (cols, rows) = (frame.cols(), frame.rows());
        // A cell's width over its height, and your program's 0.5 where no window has said.
        let (wide, high) = (f64::from(info.cell.width), f64::from(info.cell.height));
        let aspect = if wide > 0.0 && high > 0.0 {
            wide / high
        } else {
            0.5
        };
        let now = Some((self.t, cols, rows, aspect, self.tones, self.extras));
        if !frame.is_new() && self.drawn == now {
            return;
        }
        let secs = self.t.as_secs_f64();
        self.sea
            .draw(cols, rows, aspect, self.tones, self.extras, secs);
        self.drawn = now;
        let (glyphs, inks) = (self.sea.glyphs(), self.sea.inks());
        for r in 0..rows {
            for (c, cell) in frame.row_mut(r).iter_mut().enumerate() {
                let (code, ink) = (glyphs[r * cols + c], inks[r * cols + c]);
                // The background is always the `default` face's, which is the theme's.
                *cell = match char::from_u32(code.into()) {
                    Some(letter) if code != 0 => {
                        let ink = Rgb((ink >> 16) as u8, (ink >> 8) as u8, ink as u8);
                        FrameCell::new(Glyph::Char(letter), Color::Rgb(ink), Color::Default)
                    }
                    _ => FrameCell::blank(),
                };
            }
        }
    }

    /// A window that takes the buffer sends `Resize` and `Focus(true)`, and the pointer over it
    /// and a key no keymap took come here: each wakes it. Losing the focus doesn't stop it: DIE's
    /// window opens without the keyboard, and the buffer DIE starts on would open on an empty
    /// frame.
    fn event(&mut self, ev: &Event, ctx: &mut Ctx) -> bool {
        let stirs = matches!(
            ev,
            Event::Resize { .. } | Event::Focus(true) | Event::Pointer(_) | Event::Key(_)
        );
        if stirs {
            self.wake(ctx);
        }
        // Esc does nothing in normal state, and says nothing. Any other key is nobody's, and
        // core says so.
        matches!(ev, Event::Key(key) if plain_esc(key))
    }

    fn handles(&self, key: &KeyEvent) -> bool {
        plain_esc(key)
    }
}
