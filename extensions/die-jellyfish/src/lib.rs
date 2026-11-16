//! die-jellyfish: your ASCII jellyfish (`~/workspace/art/jelly`) swimming in a DIE buffer, on the
//! theme's background, as an extension (scope D14). It depends on die-core alone, so it's
//! platform-neutral and out of the product: `cargo xtask lint` holds it to that. Its parts:
//!
//! - this file: [`Jellyfish`], the extension, with your [`Options`], the `jellyfish` and
//!   `jellyfish-close` commands, and the keys on its buffer;
//! - [`sea`]: that program's `src/main.rs` with the listing, the readouts and the terminal cut
//!   away, leaving the animal and its effects, drawn from the clock alone. A colour fades into
//!   the theme's background where it faded to black, and the five rows the readouts wanted
//!   became two. It keeps your file's own line breaks, so a diff against it shows the rest;
//! - `surface`: what `swim` and `main` were there: the clock, the size, the cells, and when it
//!   swims.
//!
//! Nothing here runs at registration or init. It swims while a window shows it and you have
//! touched DIE in the last five minutes: at rest, or with no window on it, no timer runs.
#![warn(missing_docs)]

#[cfg(test)]
use die_test_cap as _;

// Your file's formatting, not rustfmt's, so the two can be compared.
#[rustfmt::skip]
pub mod sea;
mod surface;

use die_core::{Args, BufferId, Ctx, Extension, Registry, Result, ShowIn, State};
use sea::Extras;
use std::time::Duration;
use surface::Swim;

/// The surface's kind, which names its keymap.
pub const KIND: &str = "jellyfish";
/// The buffer's name.
pub const NAME: &str = "*jellyfish*";
/// A frame's length: thirty a second, as `swim` had it.
pub const TICK: Duration = Duration::from_nanos(33_333_333);

// ------------------------------------------------------------------ options

/// Your options (C7), as fields.
#[derive(Clone, Debug, PartialEq)]
pub struct Options {
    /// The water behind the jellyfish: drifting dust, and a four-point glint now and then.
    pub water: bool,
    /// The far-off little jellyfish at the sides.
    pub shoal: bool,
    /// Give the buffer the window's whole width, rather than your text defaults' 80 columns
    /// centred, which `\z` toggles. A board's default is a file's width; here the dust and the
    /// sparks would stop at a wall you can't see.
    pub full_width: bool,
    /// How long without a key before it rests: a still picture, with no timer, until you next do
    /// something. None keeps it swimming for as long as a window shows it.
    pub rest_after: Option<Duration>,
}

impl Default for Options {
    /// The water without the shoal, the window's whole width, and a rest after five minutes.
    fn default() -> Options {
        Options {
            water: true,
            shoal: false,
            full_width: true,
            rest_after: Some(Duration::from_secs(300)),
        }
    }
}

// ------------------------------------------------------------------ the extension

/// The extension: your options, and the jellyfish swimming now.
pub struct Jellyfish {
    /// Your options.
    pub options: Options,
    /// Its buffer, while there is one.
    buf: Option<BufferId>,
}

impl Default for Jellyfish {
    fn default() -> Jellyfish {
        Jellyfish::new(Options::default())
    }
}

impl Jellyfish {
    /// The extension with these options.
    pub fn new(options: Options) -> Jellyfish {
        Jellyfish { options, buf: None }
    }

    /// `jellyfish`: the one swimming, or a new one, in this window, and awake.
    fn show(&mut self, ctx: &mut Ctx, a: &Args) -> Result {
        let buf = match self.buf.filter(|b| ctx.is_live(*b)) {
            Some(buf) => buf,
            None => self.hatch(ctx)?,
        };
        // As `die::start` puts its first buffer on screen.
        let before = ctx.focused().and_then(|w| Some((w, ctx.window_buffer(w)?)));
        let mut covered = None;
        if let Some((over, was)) = before {
            // The buffer about to be covered is the one in use, whatever was made since it was
            // shown, so a quit or `:rebuild` with the jellyfish up comes back on it. Focusing
            // the focused window says so, and moves nothing; not for another window's command.
            if a.window.is_none_or(|w| w == over) {
                ctx.focus(over);
            }
            let w = ctx.show(buf, ShowIn::Current)?;
            // What `q` uncovers. Core's "used last" isn't always it: a buffer made behind the
            // jellyfish, as orchid's log is, is the newer.
            covered = (w == Some(over) && was != buf).then_some((over, was));
        } else {
            ctx.open_window(buf, 80, 24)?;
        }
        ctx.with_surface(buf, |s: &mut Swim, ctx| {
            s.covered = covered.or(s.covered);
            s.wake(ctx);
        })?;
        Ok(())
    }

    /// A new buffer at the first spark, as your options have it.
    fn hatch(&mut self, ctx: &mut Ctx) -> Result<BufferId> {
        let extras = Extras {
            water: self.options.water,
            shoal: self.options.shoal,
        };
        let swim = Swim::new(surface::tones(ctx), extras, self.options.rest_after);
        let buf = ctx.create_surface(NAME, swim);
        self.buf = Some(buf);
        // The surface asks which windows show its buffer, and its timers find it by this.
        ctx.with_surface(buf, |s: &mut Swim, _| s.buf = buf)?;
        if self.options.full_width {
            ctx.change_settings(buf, |s| s.max_width = None)?;
        }
        Ok(buf)
    }

    /// `jellyfish-close`: the buffer goes, and with it the timer and the moment it had reached.
    /// The window `jellyfish` put it in, if this is that window and it has stayed on the
    /// jellyfish, goes back to the buffer it covered. Any other window showing it, and that one
    /// otherwise, shows what core gives a killed buffer's window: the buffer used last.
    fn close(&mut self, ctx: &mut Ctx, a: &Args) -> Result {
        let Some(buf) = self.buf.take() else {
            return Ok(());
        };
        let covered = ctx.with_surface(buf, |s: &mut Swim, ctx| s.covering(ctx));
        // A covered buffer killed meanwhile can't be shown, and the window is core's to fill.
        if let Ok(Some((w, was))) = covered
            && a.window.or(ctx.focused()) == Some(w)
            && ctx.show(was, ShowIn::Current).is_ok()
        {
            // The window has left it, and core has queued telling it so (`Focus(false)`). It goes
            // after that, in the same turn: told when already gone, core would report an error.
            ctx.defer(move |ctx| {
                ctx.kill(buf);
                Ok(())
            });
        } else {
            ctx.kill(buf);
        }
        Ok(())
    }
}

/// die-vim's `C-w` keys, a copy of die-whiteboard's table, which copies die-vim's own: a
/// surface without text inherits none of die-vim's keys, since those are the text kind's, and an
/// extension depends on die-core alone, so it can't ask die-vim for them. Less the whiteboard's
/// `<Esc>` row: `vim-cancel` wants a buffer with text and says so here, where with no row core
/// drops the prefix and the count, and the surface takes the Esc in silence.
const WINDOW_KEYS: [(&str, &str); 31] = [
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
];

impl Extension for Jellyfish {
    fn name(&self) -> &'static str {
        "jellyfish"
    }

    fn register(&mut self, reg: &mut Registry) {
        reg.ext_command(
            "jellyfish",
            "Show the jellyfish in this window: the one swimming, or a new one from its first spark.",
            Jellyfish::show,
        );
        reg.ext_command(
            "jellyfish-close",
            "q: let the jellyfish go. Its window shows the buffer it covered, or the one used last.",
            Jellyfish::close,
        );
        // The keys a buffer can't do without, bound as die-whiteboard binds them for its own
        // surface. ZZ has nothing to write here, so it's ZQ. C-o is C-^: die-vim's jump list
        // starts from a text cursor, which a jellyfish hasn't, and the buffer used before is
        // where the first C-o from here would go. Any other key is undefined, as on any surface,
        // and no leader key is taken.
        use State::Normal;
        let k = reg.kind_keymap(KIND);
        k.bind(Normal, "q", "jellyfish-close")
            .bind(Normal, "ZZ", "vim-quit")
            .bind(Normal, "ZQ", "vim-quit")
            .bind(Normal, "<C-^>", "vim-alternate-buffer")
            .bind(Normal, "<C-o>", "vim-alternate-buffer")
            .bind(Normal, ":", "vim-ex");
        for (keys, name) in WINDOW_KEYS {
            for key in keys.split_whitespace() {
                k.bind(Normal, &format!("<C-w>{key}"), name);
            }
        }
    }
}
