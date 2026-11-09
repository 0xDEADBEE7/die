//! The processing indicator, the Emacs client's `orchid-processing-indicator`: a line under a turn's
//! events, `[Processing: Ns]`, with the session's cumulative input right-aligned to 80 columns,
//! rewritten in place each second while the turn runs. When `metadata.json` says the session
//! stopped, by any of orchid's ends (idle, failed, cancelled, terminated), it reads
//! `[Finished: Ns]` one last time and stays as a record. Emacs's waited to have seen `running`
//! first, so a quick turn never finished (its A4), didn't know `terminated` (A3), and finished
//! by its poll without drawing the line again (B4).
//!
//! The chat owns it: it shows one where a turn's events go, ticks it with a timer it holds only
//! while a turn runs, and finishes it from the poll.

use crate::parse;
use crate::sections::Mark;
use die_core::{BufferId, Ctx, Origin, Result, Sub, WindowId};
use std::time::Duration;

/// The face the line is in (Emacs's `shadow`).
pub const FACE: &str = "shadow";

/// How often the line is drawn again while the turn runs.
pub const TICK: Duration = Duration::from_secs(1);

/// The line for `seconds` into a turn: `[Processing: Ns]`, or `[Finished: Ns]` once it's over,
/// then the cumulative input, `[NNNNNk]`, ending at column 80. With no tokens, spaces to 80.
pub fn line(seconds: u64, finished: bool, tokens: u64) -> String {
    let left = if finished {
        format!("[Finished: {seconds}s]")
    } else {
        format!("[Processing: {seconds}s]")
    };
    let right = match tokens {
        0 => String::new(),
        n => format!("[{}]", parse::format_cumulative(n)),
    };
    parse::right_aligned(&left, &right)
}

/// A turn's indicator, while it runs.
pub struct Indicator {
    /// The line's start.
    at: Mark,
    /// When it started counting, on the editor's clock.
    since: Duration,
    /// The seconds the turn had run before then: one under way when the chat opened.
    before: u64,
    /// The cumulative input, as `metadata.json` last said it.
    pub tokens: u64,
    /// What draws it again each second.
    pub timer: Option<Sub>,
}

impl Indicator {
    /// Count from now on `ctx`'s clock, with `before` seconds gone already, for the line that
    /// starts at `at`.
    pub fn new(ctx: &Ctx, at: Mark, before: u64, tokens: u64) -> Indicator {
        Indicator {
            at,
            since: ctx.now(),
            before,
            tokens,
            timer: None,
        }
    }

    /// The seconds so far.
    pub fn seconds(&self, ctx: &Ctx) -> u64 {
        self.before + ctx.now().saturating_sub(self.since).as_secs()
    }

    /// Draw the line again in `buf`, silently, keeping each window's cursor where it was on it,
    /// as Emacs saved and restored every window's point. Nothing if the text there isn't the
    /// indicator's any more, as after you deleted it.
    pub fn draw(&self, ctx: &mut Ctx, buf: BufferId, finished: bool) -> Result {
        let new = line(self.seconds(ctx), finished, self.tokens);
        let windows: Vec<WindowId> = ctx
            .window_list()
            .into_iter()
            .filter(|w| ctx.window_buffer(*w) == Some(buf))
            .collect();
        let t = ctx.text_mut(buf)?;
        let Some(start) = self.at.pos(t) else {
            return Ok(());
        };
        let rest = t.rope().byte_slice(start..).to_string();
        let end = start + rest.find('\n').unwrap_or(rest.len());
        let old = &rest[..end - start];
        let ours = old.starts_with("[Processing: ") || old.starts_with("[Finished: ");
        if !ours || old == new {
            return Ok(());
        }
        let on_it: Vec<(WindowId, usize)> = windows
            .iter()
            .filter_map(|w| Some((*w, t.cursor(*w)?)))
            .filter(|(_, c)| (start..=end).contains(c))
            .map(|(w, c)| (w, c - start))
            .collect();
        let len = new.len();
        t.tx(Origin::Extension)
            .silent()
            .replace(start..end, new)
            .commit()?;
        for (w, off) in on_it {
            t.set_cursor(w, start + off.min(len));
        }
        Ok(())
    }

    /// Whether a window shows `buf`, so drawing it is worth it.
    pub fn seen(ctx: &Ctx, buf: BufferId) -> bool {
        let windows = ctx.window_list();
        windows.iter().any(|w| ctx.window_buffer(*w) == Some(buf))
    }
}

#[cfg(test)]
mod tests {
    //! The Emacs client's `orchid-processing-indicator-test.el`, for its line.
    use super::*;

    #[test]
    fn the_line_right_aligns_the_cumulative_input() {
        let l = line(4, true, 4_200_000);
        assert_eq!(l, format!("[Finished: 4s]{}[ 4200k]", " ".repeat(58)));
        assert_eq!(parse::width(&l), 80);
        let l = line(12, false, 4_200_000);
        assert_eq!(l, format!("[Processing: 12s]{}[ 4200k]", " ".repeat(55)));
        assert_eq!(parse::width(&l), 80);
    }

    #[test]
    fn with_no_tokens_it_pads_to_80() {
        let l = line(0, false, 0);
        assert!(l.starts_with("[Processing: 0s] "));
        assert_eq!(l.trim_end(), "[Processing: 0s]");
        assert_eq!(parse::width(&l), 80);
    }
}
