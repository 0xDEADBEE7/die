//! The chat buffer, `*orchid-chat-<agent>-<wsdir>-<id5>*` (the Emacs client's `orchid-chat-mode`):
//! a header, the session's last events restored, then each new one as orchid appends it, each a
//! collapsed 80-column stub, and your input at the end, sent with `RET` in insert state.
//!
//! The layout is Emacs's, from the top:
//!
//! ```text
//! Workspace: /path/to/dir                    the value in orchid-chat-header-workspace
//! Agent: default                             the value in orchid-chat-header-agent
//! ──────────────────  dir  ──────────────────  orchid-chat-separator
//!
//! [user]    [first 47 columns ...]    [ 63k] [11:16:46]     collapsed: TAB shows it all
//! [agnt]    [...]                     [ 94k] [11:16:52]
//!
//! [session restored]                                   [ 4200k]   read-only
//! ──────────────────  dir  ──────────────────  read-only
//!
//! <your input>
//! ```
//!
//! A send puts your stub and a separator above the input, and `[Processing: Ns]` under the stub;
//! the turn's events go in above that line as they come, silently (undo never sees them), and the
//! line reads `[Finished: Ns]` once `metadata.json` says the session stopped. Nothing typed in the
//! input moves: text that goes in above it moves your cursor with it.
//!
//! The input starts at the blank line under the last separator, one line before Emacs's did, with
//! the same bytes: die-vim's motions never go to the empty line after the text's last newline,
//! where Emacs's empty input line was, so `G`, `j` and a click land on that blank line, and typing
//! there is your input too. A send trims it away.
//!
//! Where the Emacs client was wrong for orchid 0.2 or simply wrong, this follows orchid 0.2.2 and
//! the client map's fixes: a session already running when you open it streams into the transcript
//! above its indicator, not into your input (B2); an empty input sends nothing (B3); the
//! indicator finishes on any end, without having seen `running`, and draws its last line (A3, A4,
//! B4); the restore and the follow meet at one byte offset, so nothing appended between them is
//! lost (B9); older history loads in order (B5); an archived session says so once and stops
//! (B8); a new session's chat keeps what you typed while orchid made it, and `[SYSTEM]` goes above
//! your input rather than after it (B9). A turn started elsewhere, as from Emacs, shows its
//! message and indicator as one of yours does.

use crate::cli::{Cli, Cmd, Outcome};
use crate::indicator::{self, Indicator};
use crate::parse::{self, Piece, Section, WIDTH};
use crate::poll::{self, Polling};
use crate::sections::{self, Build, Mark, Sections, Toggled};
use crate::store::{self, Event, EventKind, Metadata, Restored, Tail};
use crate::{Kind, Options, Orchid, face, log};
use die_core::session::{Value, json};
use die_core::{
    Args, BufferId, Checkout, Ctx, DecorationId, Error, Mode, Origin, PromptSpec, Registry, Result,
    ShowIn, State, WindowId,
};
use jiff::Timestamp;
use jiff::tz::TimeZone;
use std::collections::{HashSet, VecDeque};
use std::io;
use std::path::{Path, PathBuf};
use std::rc::Rc;

/// The mode every chat buffer has (Emacs's `orchid-chat-mode`), whose keymap your config can bind
/// in.
pub const MODE: &str = "orchid-chat";

/// Where `orchid-collapsible-report-stats` puts its report, since the command row shows one line.
pub const STATS_BUFFER: &str = "*orchid-collapsible-stats*";

/// What Emacs said where `TAB` has nothing to do.
pub const TAB_ELSEWHERE: &str = "Use TAB on collapsible sections to expand/collapse them";

/// A slash command.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Slash {
    Config,
    Quit,
    Kill,
    Logs,
    Metadata,
    New,
}

/// The slash menu, as the Emacs client labelled it: aliases merged after the shortest name, in
/// order. `/scope-escape` is gone (the m0 log's call 899), as orchid 0.2 dropped its field.
pub const SLASH: [&str; 6] = [
    "config",
    "exit  (quit)",
    "kill",
    "logs",
    "metadata",
    "new  (clear)",
];

const SLASH_DOES: [Slash; 6] = [
    Slash::Config,
    Slash::Quit,
    Slash::Kill,
    Slash::Logs,
    Slash::Metadata,
    Slash::New,
];

// ------------------------------------------------------------------ the mode

/// A chat buffer's mode. A buffer gets it with its chat, which [`open`], [`new_session`] and a
/// restart's restore make.
#[derive(Default)]
pub struct Chat(Option<Box<Live>>);

impl Mode for Chat {}

/// What a chat needs of the extension, kept so the chat runs with the extension checked out too.
#[derive(Clone)]
struct Env {
    root: PathBuf,
    zone: TimeZone,
    options: Options,
    cli: Cli,
}

impl Env {
    fn of(o: &Orchid) -> Env {
        Env {
            root: o.root().to_path_buf(),
            zone: o.zone().clone(),
            options: o.options.clone(),
            cli: o.cli(),
        }
    }
}

/// A new session's chat, before orchid has made the session.
#[derive(Clone, Debug)]
struct Pending {
    agent: String,
    dir: PathBuf,
}

/// A chat's state (Emacs's buffer-local `orchid-chat--*` and `orchid-processing--*`).
struct Live {
    env: Env,
    buf: BufferId,
    /// The session, once there is one.
    id: Option<String>,
    pending: Option<Pending>,
    /// Its `metadata.json`, as last read.
    meta: Option<Metadata>,
    /// Where your input starts.
    input: Mark,
    /// Where the next event goes: the assistant cursor.
    cursor: Mark,
    /// Where older history goes.
    history: Mark,
    sections: Sections,
    tail: Tail,
    /// The ids of the events shown.
    seen: HashSet<String>,
    /// The first event in the transcript is your message, which older history goes above.
    first_user: bool,
    indicator: Option<Indicator>,
    polling: Option<Polling>,
    /// Sends out.
    sending: usize,
    /// Messages sent from here that `events.jsonl` doesn't have yet, so they aren't shown twice.
    sent: VecDeque<String>,
    /// The session was archived: it isn't followed any more.
    gone: bool,
    /// The last trouble reading the session's files, logged once.
    trouble: Option<String>,
}

impl Live {
    fn new(env: Env, buf: BufferId) -> Live {
        Live {
            env,
            buf,
            id: None,
            pending: None,
            meta: None,
            input: Mark::default(),
            cursor: Mark::default(),
            history: Mark::default(),
            sections: Sections::default(),
            tail: Tail::default(),
            seen: HashSet::new(),
            first_user: false,
            indicator: None,
            polling: None,
            sending: 0,
            sent: VecDeque::new(),
            gone: false,
            trouble: None,
        }
    }

    fn label(&self) -> Option<String> {
        self.meta.as_ref().and_then(label)
    }

    // -------------------------------------------------------------- layout

    /// The whole chat for `meta`, in place of whatever the buffer has: the header, the restored
    /// events, and `input` after them. Then follow from where the restore ended (B9).
    fn lay_out(
        &mut self,
        ctx: &mut Ctx,
        meta: &Metadata,
        restored: Restored,
        input: &str,
    ) -> Result {
        let zone = self.env.zone.clone();
        let label = label(meta);
        let ws = meta.working_dir.as_deref().unwrap_or("N/A");
        let mut b = Build::new();
        b.push("Workspace: ")
            .styled(ws, face::HEADER_WORKSPACE)
            .push("\nAgent: ")
            .styled(parse::agent_name(meta), face::HEADER_AGENT)
            .push("\n")
            .styled(&separator(label.as_deref()), face::SEPARATOR)
            .push("\n");
        let events: Vec<Rc<Event>> = restored.events.into_iter().map(Rc::new).collect();
        self.first_user = events.first().is_some_and(|e| is_user(e));
        let running = meta.status.is_running();
        // With nothing above the input, the input's own blank line is the one under the separator.
        let mut history = b.len();
        let mut shown = None;
        if !events.is_empty() || running {
            b.push("\n");
            history = b.len();
            transcript(&mut b, &events, &zone);
            if !events.is_empty() {
                let line = restored_line(meta.token_usage.cumulative_input);
                b.push("\n").read_only(&line, Some(indicator::FACE));
                if !running {
                    b.read_only(&separator(label.as_deref()), Some(face::SEPARATOR))
                        .push("\n");
                }
            }
        }
        // A session already running: its events go above its indicator, not in your input (B2).
        let cursor = b.len();
        if running {
            let before = since_last_user(&events);
            let tokens = meta.token_usage.cumulative_input;
            shown = Some((b.len() + 1, before));
            b.push("\n")
                .styled(&indicator::line(before, false, tokens), indicator::FACE)
                .push("\n")
                .read_only(&separator(label.as_deref()), Some(face::SEPARATOR))
                .push("\n");
        }
        let input_at = b.len();
        b.push(&input_area(input));
        let t = ctx.text_mut(self.buf)?;
        let all = 0..t.len();
        self.sections.place(t, all, b)?;
        self.history.set(t, history)?;
        self.cursor.set(t, cursor)?;
        self.input.set(t, input_at)?;
        let line_at = Mark::default();
        if let Some((at, _)) = shown {
            line_at.set(t, at)?;
        }
        self.id = Some(meta.id.clone());
        self.meta = Some(meta.clone());
        self.tail = Tail::at(restored.end);
        self.seen = restored.seen;
        if let Some((_, before)) = shown {
            self.start_indicator(ctx, line_at, before);
        }
        Ok(())
    }

    /// A new session's chat while orchid makes it: a plain separator and the input (Emacs's
    /// `pending` buffer, with no header).
    fn lay_out_pending(&mut self, ctx: &mut Ctx, pending: Pending, input: &str) -> Result {
        let mut b = Build::new();
        b.styled(&separator(None), face::SEPARATOR).push("\n");
        let at = b.len();
        b.push(&input_area(input));
        let t = ctx.text_mut(self.buf)?;
        let all = 0..t.len();
        self.sections.place(t, all, b)?;
        for m in [&self.history, &self.cursor, &self.input] {
            m.set(t, at)?;
        }
        self.pending = Some(pending);
        Ok(())
    }

    /// A chat a restart brought back on a session orchid no longer has: why, and your input.
    fn lay_out_gone(&mut self, ctx: &mut Ctx, id: &str, input: &str) -> Result {
        let t = ctx.text_mut(self.buf)?;
        let all = 0..t.len();
        let mut b = Build::new();
        b.push(&input_area(input));
        self.sections.place(t, all, b)?;
        for m in [&self.history, &self.cursor, &self.input] {
            m.set(t, 0)?;
        }
        self.id = Some(id.to_string());
        self.gone = true;
        self.system(ctx, &format!("Session {id} is gone (archived or deleted)"))
    }

    /// What a restart keeps, besides the input.
    fn kept(&self) -> Value {
        match (&self.id, &self.pending) {
            (Some(id), _) => json!({ "id": id }),
            (None, Some(p)) => json!({"pending": {"agent": p.agent, "dir": p.dir}}),
            (None, None) => json!({}),
        }
    }

    // -------------------------------------------------------------- putting text in

    /// `b` at the assistant cursor, which moves past it. Never into your input: if the cursor is
    /// where the input starts, the input starts after it.
    fn at_cursor(&mut self, ctx: &mut Ctx, b: Build) -> Result {
        let t = ctx.text(self.buf)?;
        if self.cursor.at(t) == self.input.at(t) {
            return self.above_input(ctx, b, None);
        }
        let t = ctx.text_mut(self.buf)?;
        let at = self.cursor.at(t);
        let n = b.len();
        self.sections.place(t, at..at, b)?;
        self.cursor.set(t, at + n)
    }

    /// `b` just above your input, which then starts after it, with its blank line: what you typed
    /// stays as it was, and so does each cursor in it. The next events go at `under`, an offset
    /// into `b`, if it's given.
    fn above_input(&mut self, ctx: &mut Ctx, b: Build, under: Option<usize>) -> Result {
        let wins = windows(ctx, self.buf);
        let t = ctx.text_mut(self.buf)?;
        let at = self.input.at(t);
        let keep = in_input(t, &wins, at);
        let cursor_here = self.cursor.at(t) == at;
        let n = b.len();
        self.sections.place(t, at..at, b)?;
        let input = at + n;
        self.input.set(t, input)?;
        match under {
            Some(u) => self.cursor.set(t, at + u)?,
            None if cursor_here => self.cursor.set(t, input)?,
            None => {}
        }
        // The input starts with the blank line under the separator, as Emacs's did.
        let blank = t.len() > input && t.rope().byte(input) == b'\n';
        if !blank {
            let mut nl = Build::new();
            nl.push("\n");
            self.sections.place(t, input..input, nl)?;
        }
        let shift = usize::from(!blank);
        for (w, off) in keep {
            t.set_cursor(w, input + off + shift);
        }
        Ok(())
    }

    /// A `[SYSTEM]` message, framed in separators, in the warning face, just above your input, as
    /// the Emacs client's `orchid-chat-insert-system-message` did at the end of the buffer.
    fn system(&mut self, ctx: &mut Ctx, msg: &str) -> Result {
        let rule = rule();
        let mut b = Build::new();
        b.push("\n\n")
            .styled(&rule, face::SEPARATOR)
            .push("\n")
            .styled(&format!("[SYSTEM] {msg}\n"), "warning")
            .styled(&rule, face::SEPARATOR)
            .push("\n");
        self.above_input(ctx, b, None)
    }

    /// A turn's start: your message's stub above the input, then a separator, with the next
    /// events to go just under the stub. `tokens` and `time` are the stub's.
    fn begin_turn(
        &mut self,
        ctx: &mut Ctx,
        content: &str,
        time: Timestamp,
        tokens: Option<u64>,
    ) -> Result {
        self.indicator = None;
        let stub = Section {
            stub: parse::user_stub(content, time, tokens, &self.env.zone),
            face: face::USER_STUB,
            lazy: false,
        };
        let mut b = Build::new();
        b.push("\n")
            .eager(&stub, &[Piece::Text(content.into(), None)])
            .push("\n");
        let under = b.len();
        b.styled(&separator(self.label().as_deref()), face::SEPARATOR)
            .push("\n");
        self.above_input(ctx, b, Some(under))
    }

    // -------------------------------------------------------------- the indicator

    fn start_indicator(&mut self, ctx: &mut Ctx, at: Mark, before: u64) {
        let tokens = self
            .meta
            .as_ref()
            .map_or(0, |m| m.token_usage.cumulative_input);
        let mut ind = Indicator::new(ctx, at, before, tokens);
        let buf = self.buf;
        let tick = move |ctx: &mut Ctx| on_chat(ctx, buf, |c, ctx| c.draw_indicator(ctx));
        ind.timer = Some(ctx.every(indicator::TICK, tick));
        self.indicator = Some(ind);
    }

    /// The indicator under the turn's events, where the next event goes, which stays above it.
    fn show_indicator(&mut self, ctx: &mut Ctx, before: u64) -> Result {
        if self.indicator.is_some() {
            return Ok(());
        }
        let tokens = self
            .meta
            .as_ref()
            .map_or(0, |m| m.token_usage.cumulative_input);
        let mut b = Build::new();
        b.push("\n")
            .styled(&indicator::line(before, false, tokens), indicator::FACE)
            .push("\n");
        let t = ctx.text(self.buf)?;
        let at = self.cursor.at(t);
        if at == self.input.at(t) {
            self.above_input(ctx, b, Some(0))?;
        } else {
            let t = ctx.text_mut(self.buf)?;
            self.sections.place(t, at..at, b)?;
        }
        let t = ctx.text_mut(self.buf)?;
        let line = Mark::default();
        line.set(t, self.cursor.at(t) + 1)?;
        self.start_indicator(ctx, line, before);
        Ok(())
    }

    fn draw_indicator(&mut self, ctx: &mut Ctx) -> Result {
        match &self.indicator {
            Some(ind) if Indicator::seen(ctx, self.buf) => ind.draw(ctx, self.buf, false),
            _ => Ok(()),
        }
    }

    /// The turn is over: show what came last, draw `[Finished: Ns]`, stop ticking, and take a
    /// window reading the transcript to the end, as Emacs took every window there. One in your
    /// input stays where it is.
    fn finish(&mut self, ctx: &mut Ctx) -> Result {
        self.read_events(ctx)?;
        let Some(ind) = self.indicator.take() else {
            return Ok(());
        };
        ind.draw(ctx, self.buf, true)?;
        let wins = windows(ctx, self.buf);
        let t = ctx.text_mut(self.buf)?;
        let (input, end) = (self.input.at(t), t.len());
        for w in wins {
            if t.cursor(w).is_some_and(|c| c < input) {
                t.set_cursor(w, end);
            }
        }
        Ok(())
    }

    // -------------------------------------------------------------- following

    fn wants_fast(&self) -> bool {
        let running = self.meta.as_ref().is_some_and(|m| m.status.is_running());
        self.sending > 0 || self.indicator.is_some() || running
    }

    /// Ask the poll for the period this chat wants now.
    fn pace(&self, ctx: &mut Ctx) -> Result {
        match &self.polling {
            Some(p) => poll::set_fast(ctx, p, self.wants_fast()),
            None => Ok(()),
        }
    }

    /// One poll: `metadata.json`, then the new lines of `events.jsonl`, then the indicator's end
    /// if the session stopped. The metadata goes first: orchid has written every event of a turn
    /// before it says the turn is over.
    fn tick(&mut self, ctx: &mut Ctx) -> Result {
        let Some(id) = self.id.clone().filter(|_| !self.gone) else {
            return Ok(());
        };
        let status = match store::read_metadata(&self.env.root, &id) {
            Ok(m) => {
                if let Some(ind) = &mut self.indicator {
                    ind.tokens = m.token_usage.cumulative_input;
                }
                let status = m.status;
                self.meta = Some(m.clone());
                ctx.with_ext(|o: &mut Orchid, ctx| o.update_session(ctx, m))?;
                Some(status)
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound && !self.exists() => {
                return self.lost(ctx);
            }
            Err(e) => {
                self.trouble(ctx, &format!("metadata.json: {e}"));
                None
            }
        };
        self.read_events(ctx)?;
        if status.is_some_and(|s| !s.is_running()) && self.indicator.is_some() {
            self.finish(ctx)?;
        }
        self.pace(ctx)
    }

    fn exists(&self) -> bool {
        let id = self.id.as_deref().unwrap_or_default();
        store::session_dir(&self.env.root, id).is_dir()
    }

    /// The lines added to `events.jsonl` since the last read.
    fn read_events(&mut self, ctx: &mut Ctx) -> Result {
        let Some(id) = self.id.clone().filter(|_| !self.gone) else {
            return Ok(());
        };
        match self.tail.read(&store::events_path(&self.env.root, &id)) {
            Ok(read) => {
                for line in read.lines {
                    self.take_line(ctx, &line)?;
                }
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound && !self.exists() => {
                return self.lost(ctx);
            }
            Err(e) => self.trouble(ctx, &format!("events.jsonl: {e}")),
        }
        Ok(())
    }

    /// One new line: an event shown once, at the assistant cursor. Your own message was shown
    /// when you sent it; one sent elsewhere starts a turn here as one of yours does.
    fn take_line(&mut self, ctx: &mut Ctx, line: &str) -> Result {
        let Some(event) = Event::parse(line) else {
            return Ok(());
        };
        if let Some(id) = &event.event_id
            && self.seen.contains(id)
        {
            return Ok(());
        }
        if let EventKind::Message { role, content } = &event.kind
            && role == "user"
        {
            if content.is_empty() {
                return Ok(());
            }
            self.seen.extend(event.event_id.clone());
            if let Some(i) = self.sent.iter().position(|s| s == content) {
                self.sent.remove(i);
                return Ok(());
            }
            let time = event.timestamp.unwrap_or_else(Timestamp::now);
            self.begin_turn(ctx, content, time, event.marginal_input())?;
            if self.meta.as_ref().is_some_and(|m| m.status.is_running()) {
                self.show_indicator(ctx, seconds_since(time))?;
            }
            return Ok(());
        }
        let event = Rc::new(event);
        let mut b = Build::new();
        if !b.event(&event, false, &self.env.zone) {
            return Ok(());
        }
        self.seen.extend(event.event_id.clone());
        self.at_cursor(ctx, b)
    }

    /// The session was archived, or deleted: say so once, and stop following it (B8).
    fn lost(&mut self, ctx: &mut Ctx) -> Result {
        if self.gone {
            return Ok(());
        }
        let id = self.id.clone().unwrap_or_default();
        self.gone = true;
        self.polling = None;
        self.indicator = None;
        self.system(
            ctx,
            &format!(
                "Session {id} is gone (archived or deleted), so this chat stopped following it"
            ),
        )?;
        let _ = log::log(ctx, &format!("{id}: archived; no longer following it"));
        ctx.with_ext(|o: &mut Orchid, ctx| o.forget_session(ctx, &id))?;
        Ok(())
    }

    /// Log trouble reading the session's files, once until it changes.
    fn trouble(&mut self, ctx: &mut Ctx, what: &str) {
        if self.trouble.as_deref() != Some(what) {
            let id = self.id.as_deref().unwrap_or_default();
            let _ = log::log(ctx, &format!("{id}: {what}"));
            self.trouble = Some(what.to_string());
        }
    }

    // -------------------------------------------------------------- sending

    /// The session's marginal input now, for your stub, or orchid's estimate before a provider
    /// said (Emacs's `orchid-chat--current-marginal-input-tokens`).
    fn marginal(&self) -> u64 {
        let id = self.id.as_deref().unwrap_or_default();
        let fresh = store::read_metadata(&self.env.root, id).ok();
        let meta = fresh.as_ref().or(self.meta.as_ref());
        meta.map_or(0, |m| match m.token_usage.marginal_input {
            0 => m.token_estimate,
            n => n,
        })
    }

    /// `RET` in insert state: send the input if the cursor is in it, else a newline.
    fn send_input(&mut self, ctx: &mut Ctx, w: Option<WindowId>, args: &Args) -> Result {
        let t = ctx.text(self.buf)?;
        let input = self.input.at(t);
        let here = w.and_then(|w| t.cursor(w)).unwrap_or(t.len());
        if here < input {
            return newline(ctx, self.buf, w, args);
        }
        let message = t.rope().byte_slice(input..).to_string().trim().to_string();
        if message.is_empty() {
            return Ok(());
        }
        let Some(id) = self.id.clone() else {
            return Err(Error::Msg(
                "orchid is still making this session; your message waits".into(),
            ));
        };
        let tokens = self.marginal();
        let t = ctx.text_mut(self.buf)?;
        let end = t.len();
        // The input's blank line stays, for the next. What you typed goes as an edit undo records,
        // so it nets out with your typing, rather than finding it gone.
        let blank = usize::from(input < end && t.rope().byte(input) == b'\n');
        t.tx(Origin::Extension)
            .delete(input + blank..end)
            .commit()?;
        self.begin_turn(ctx, &message, Timestamp::now(), Some(tokens))?;
        self.sent.push_back(message.clone());
        self.sending += 1;
        self.pace(ctx)?;
        let _ = log::log(ctx, &format!("Sending to existing session: {id}"));
        let buf = self.buf;
        let cwd = ctx.directory(buf).map(Path::to_path_buf);
        let sent = message.clone();
        let done =
            move |ctx: &mut Ctx, out: Outcome| on_chat(ctx, buf, |c, ctx| c.sent(ctx, &sent, out));
        self.env
            .cli
            .run(ctx, Cmd::Send { id, message }, cwd, done)
            .detach();
        Ok(())
    }

    /// orchid's answer to a send: `running` or `queued` shows the indicator; an error says so.
    fn sent(&mut self, ctx: &mut Ctx, message: &str, out: Outcome) -> Result {
        self.sending = self.sending.saturating_sub(1);
        match out {
            Ok(_) => self.show_indicator(ctx, 0)?,
            Err(e) => {
                if let Some(i) = self.sent.iter().position(|s| s == message) {
                    self.sent.remove(i);
                }
                let _ = log::log(ctx, &format!("Send failed: {}", e.message));
                self.system(ctx, &format!("Send failed:\n{}", e.message))?;
            }
        }
        self.pace(ctx)
    }

    // -------------------------------------------------------------- keys

    /// `TAB`: toggle the section at the cursor; in your input, a tab; elsewhere, a word on what
    /// it's for.
    fn handle_tab(&mut self, ctx: &mut Ctx, w: Option<WindowId>) -> Result {
        let t = ctx.text_mut(self.buf)?;
        let here = w.and_then(|w| t.cursor(w)).unwrap_or(t.len());
        if let Some(id) = self.sections.at(t, here) {
            let (start, stub_end, _) = self.sections.range(t, id).unwrap_or_default();
            let toggled = self.sections.toggle(t, id)?;
            // From inside what it hid, to the stub.
            if let (Some(Toggled::Collapsed), Some(w)) = (toggled, w)
                && here > stub_end
            {
                t.set_cursor(w, start);
            }
            return Ok(());
        }
        if here >= self.input.at(t) {
            t.tx(Origin::User).insert(here, "\t").commit()?;
            return Ok(());
        }
        ctx.messages.push(TAB_ELSEWHERE.into());
        Ok(())
    }

    /// Older history, above what's shown: the events before the oldest shown, as many as a
    /// restore takes, each batch above the last (Emacs put later batches below, B5).
    fn load_more(&mut self, ctx: &mut Ctx) -> Result {
        let Some(id) = self.id.clone() else {
            return Err(Error::Msg("No active session".into()));
        };
        let n = self.env.options.restore_max_events;
        ctx.messages.push(format!("Loading {n} more events..."));
        let path = store::events_path(&self.env.root, &id);
        let seen = &self.seen;
        let unseen = |e: &Event| {
            parse::shows(e, true) && e.event_id.as_ref().is_some_and(|i| !seen.contains(i))
        };
        let batch = match store::restore(&path, self.env.options.restore_max_bytes, n, unseen) {
            Ok(b) => b,
            Err(e) if e.kind() == io::ErrorKind::NotFound => Restored::default(),
            Err(e) => return Err(Error::Msg(format!("events.jsonl: {e}"))),
        };
        if batch.events.is_empty() {
            ctx.messages.push("No more history to load".into());
            return Ok(());
        }
        let events: Vec<Rc<Event>> = batch.events.into_iter().map(Rc::new).collect();
        let mut b = Build::new();
        transcript(&mut b, &events, &self.env.zone);
        if self.first_user {
            b.styled(&format!("\n{}\n\n", rule()), face::SEPARATOR);
        }
        self.first_user = events.first().is_some_and(|e| is_user(e));
        let t = ctx.text_mut(self.buf)?;
        let at = self.history.at(t);
        self.sections.place(t, at..at, b)?;
        self.seen.extend(batch.seen);
        ctx.messages
            .push(format!("Loaded {} more events", events.len()));
        Ok(())
    }

    /// The cursor is at the start of a line in your input, so a `/` there would be alone.
    fn slash_here(&self, ctx: &Ctx, w: Option<WindowId>) -> Result<bool> {
        let t = ctx.text(self.buf)?;
        let Some(here) = w.and_then(|w| t.cursor(w)) else {
            return Ok(false);
        };
        Ok(here >= self.input.at(t) && t.line_start(t.line_of(here)) == here)
    }
}

// ------------------------------------------------------------------ the layout's pieces

/// An 80-column separator, `──  label  ──` with the label in the middle, or a plain rule.
pub fn separator(label: Option<&str>) -> String {
    match label.filter(|l| !l.is_empty()) {
        Some(l) => {
            let padded = format!("  {l}  ");
            let used = parse::width(&padded);
            let side = WIDTH.saturating_sub(used) / 2;
            let right = WIDTH.saturating_sub(side + used);
            format!("{}{padded}{}", "─".repeat(side), "─".repeat(right))
        }
        None => rule(),
    }
}

/// What follows the input marker: the blank line under the separator, then what you typed.
fn input_area(typed: &str) -> String {
    match typed.starts_with('\n') {
        true => typed.to_string(),
        false => format!("\n{typed}"),
    }
}

/// 80 columns of `─`.
pub fn rule() -> String {
    "─".repeat(WIDTH)
}

/// `[session restored]`, and the session's cumulative input at column 80 if it has any, then a
/// newline.
pub fn restored_line(tokens: u64) -> String {
    let line = match tokens {
        0 => "[session restored]".to_string(),
        n => parse::right_aligned(
            "[session restored]",
            &format!("[{}]", parse::format_cumulative(n)),
        ),
    };
    line + "\n"
}

/// The chat buffer's name for `meta`: `*orchid-chat-<agent>-<wsdir>-<id5>*`, downcased, with
/// `none` for no working directory. Core adds `<2>` if it's taken.
pub fn buffer_name(meta: &Metadata) -> String {
    let agent = parse::agent_name(meta).to_lowercase();
    let ws = match meta.working_dir.as_deref() {
        Some(d) => parse::workspace_name(Some(d)).to_lowercase(),
        None => "none".into(),
    };
    let n = meta.id.chars().count();
    let tail: String = meta.id.chars().skip(n.saturating_sub(5)).collect();
    format!("*orchid-chat-{agent}-{ws}-{tail}*")
}

/// What labels a session's separators: its working directory's name.
fn label(meta: &Metadata) -> Option<String> {
    let dir = meta.working_dir.as_deref()?;
    Some(parse::workspace_name(Some(dir)))
}

/// A directory's last part, as a new session's name and label use it.
fn dir_name(dir: &Path) -> String {
    dir.file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned())
}

fn is_user(e: &Event) -> bool {
    matches!(&e.kind, EventKind::Message { role, .. } if role == "user")
}

/// Restored events as the Emacs client laid them out: each, and a separator before each of your
/// messages that follows another event.
fn transcript(b: &mut Build, events: &[Rc<Event>], zone: &TimeZone) {
    let mut any = false;
    for e in events {
        if any && is_user(e) {
            b.styled(&format!("\n{}\n\n", rule()), face::SEPARATOR);
        }
        any |= b.event(e, true, zone);
    }
}

/// Whole seconds since `t`, none if it's ahead.
fn seconds_since(t: Timestamp) -> u64 {
    Timestamp::now().duration_since(t).as_secs().max(0) as u64
}

/// How long a turn already running has run: since your last message, as orchid keeps no start
/// for a turn (Emacs counted from the open, the client map's A11).
fn since_last_user(events: &[Rc<Event>]) -> u64 {
    let last = events.iter().rev().find(|e| is_user(e));
    last.and_then(|e| e.timestamp).map_or(0, seconds_since)
}

/// The last whole lines of a session's events, for its chat.
fn restore_events(env: &Env, id: &str) -> Result<Restored> {
    let path = store::events_path(&env.root, id);
    let o = &env.options;
    match store::restore(&path, o.restore_max_bytes, o.restore_max_events, |e| {
        parse::shows(e, true)
    }) {
        Ok(r) => Ok(r),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Restored::default()),
        Err(e) => Err(Error::Msg(format!("{}: {e}", path.display()))),
    }
}

// ------------------------------------------------------------------ windows

fn windows(ctx: &Ctx, buf: BufferId) -> Vec<WindowId> {
    let all = ctx.window_list();
    all.into_iter()
        .filter(|w| ctx.window_buffer(*w) == Some(buf))
        .collect()
}

/// The windows whose cursor is in the input, which starts at `input`, and how far into it.
fn in_input(t: &die_core::TextBuf, wins: &[WindowId], input: usize) -> Vec<(WindowId, usize)> {
    let at = |w: &WindowId| Some((*w, t.cursor(*w)?.checked_sub(input)?));
    wins.iter().filter_map(at).collect()
}

/// The command's buffer, and its window if it shows it.
fn target(ctx: &Ctx, args: &Args) -> Result<(BufferId, Option<WindowId>)> {
    let buf = args
        .buffer
        .or_else(|| ctx.focused_buffer())
        .ok_or_else(not_a_chat)?;
    let w = args
        .window
        .or_else(|| ctx.focused())
        .filter(|w| ctx.window_buffer(*w) == Some(buf));
    Ok((buf, w))
}

fn not_a_chat() -> Error {
    Error::Msg("This isn't an orchid chat".into())
}

/// `f` with the chat in `buf` checked out, or an error if `buf` has none.
fn with_chat<R>(
    ctx: &mut Ctx,
    buf: BufferId,
    f: impl FnOnce(&mut Live, &mut Ctx) -> Result<R>,
) -> Result<R> {
    match ctx.with_mode::<Chat, _>(buf, |c, ctx| c.0.as_deref_mut().map(|l| f(l, ctx))) {
        Ok(Some(r)) => r,
        Ok(None) | Err(Checkout::Missing) => Err(not_a_chat()),
        Err(e) => Err(e.into()),
    }
}

/// [`with_chat`] for a callback that may come after the buffer went: then nothing.
fn on_chat(ctx: &mut Ctx, buf: BufferId, f: impl FnOnce(&mut Live, &mut Ctx) -> Result) -> Result {
    match ctx.with_mode::<Chat, _>(buf, |c, ctx| c.0.as_deref_mut().map(|l| f(l, ctx))) {
        Ok(Some(r)) => r,
        _ => Ok(()),
    }
}

/// A newline at the cursor, as Emacs's `(insert "\n")`: die-vim's own in insert state, so its
/// session records it.
fn newline(ctx: &mut Ctx, buf: BufferId, w: Option<WindowId>, args: &Args) -> Result {
    if ctx.state_of(buf) == State::Insert {
        match ctx.run("vim-insert-newline-bare", args) {
            Err(Error::NoCommand(_)) => {}
            r => return r,
        }
    }
    let t = ctx.text_mut(buf)?;
    let here = w.and_then(|w| t.cursor(w)).unwrap_or(t.len());
    t.tx(Origin::User).insert(here, "\n").commit()?;
    Ok(())
}

/// Put the window's cursor at the buffer's end, as Emacs's `(goto-char (point-max))`.
fn to_end(ctx: &mut Ctx, buf: BufferId, w: Option<WindowId>) -> Result {
    let t = ctx.text_mut(buf)?;
    let end = t.len();
    if let Some(w) = w {
        t.set_cursor(w, end);
    }
    Ok(())
}

/// Cursors after the chat was laid out anew: one that was `off` into the old input is as far
/// into the new one, and the rest go to the end.
fn put_cursors(ctx: &mut Ctx, buf: BufferId, cursors: Vec<(WindowId, Option<usize>)>) -> Result {
    let input = with_chat(ctx, buf, |c, ctx| Ok(c.input.at(ctx.text(buf)?)))?;
    let t = ctx.text_mut(buf)?;
    let end = t.len();
    for (w, off) in cursors {
        t.set_cursor(w, off.map_or(end, |o| (input + o).min(end)));
    }
    Ok(())
}

/// The windows on `buf`, and how far into its input from `input` each cursor is.
fn cursors(ctx: &Ctx, buf: BufferId, input: Option<usize>) -> Vec<(WindowId, Option<usize>)> {
    let Ok(t) = ctx.text(buf) else {
        return vec![];
    };
    let into = |w: &WindowId| t.cursor(*w).and_then(|c| c.checked_sub(input?));
    windows(ctx, buf).iter().map(|w| (*w, into(w))).collect()
}

// ------------------------------------------------------------------ opening

/// Enable the mode on `buf` with a new chat, and lay it out with `f`.
fn start(
    ctx: &mut Ctx,
    buf: BufferId,
    env: Env,
    f: impl FnOnce(&mut Live, &mut Ctx) -> Result,
) -> Result {
    ctx.enable_mode::<Chat>(buf)?;
    ctx.with_mode::<Chat, _>(buf, |c, ctx| {
        let mut live = Box::new(Live::new(env, buf));
        let r = f(&mut live, ctx);
        c.0 = Some(live);
        r
    })?
}

/// Follow the chat on the shared poll, and keep it over a restart, with its unsent input.
fn attach(o: &mut Orchid, ctx: &mut Ctx, buf: BufferId) -> Result {
    let (state, input, follows, fast) = with_chat(ctx, buf, |c, _| {
        let follows = c.id.is_some() && !c.gone;
        Ok((c.kept(), c.input.clone(), follows, c.wants_fast()))
    })?;
    if follows {
        let tick = move |ctx: &mut Ctx| on_chat(ctx, buf, |c, ctx| c.tick(ctx));
        let polling = o.poll.subscribe(ctx, tick);
        o.poll.set_fast(ctx, &polling, fast);
        with_chat(ctx, buf, |c, _| {
            c.polling = Some(polling);
            Ok(())
        })?;
    }
    let keep = move |ctx: &Ctx, buf: BufferId, state: &Value| {
        let t = ctx.text(buf).ok()?;
        let at = input.pos(t)?;
        let mut kept = state.clone();
        kept["input"] = Value::String(t.rope().byte_slice(at..).to_string());
        Some(kept)
    };
    o.track_with(buf, Kind::Chat, state, keep);
    Ok(())
}

/// The chat's buffer's directory: the session's working directory, as Emacs's `default-directory`
/// for `/new`, if it's there.
fn set_directory(ctx: &mut Ctx, buf: BufferId, meta: &Metadata) {
    if let Some(dir) = meta.working_dir.as_deref().map(Path::new)
        && dir.is_dir()
    {
        ctx.set_directory(buf, dir);
    }
}

/// Open a chat on `session_id` in the selected window, or go to the one open on it. Its last
/// events come back from `events.jsonl` as collapsed stubs, and from where they end it follows
/// the session, quickly while it runs.
pub fn open(ctx: &mut Ctx, session_id: &str) -> Result {
    let (open, env) =
        ctx.with_ext(|o: &mut Orchid, ctx| (o.chat_buffer(ctx, session_id), Env::of(o)))?;
    if let Some(buf) = open {
        ctx.show(buf, ShowIn::Current)?;
        return Ok(());
    }
    let meta = store::read_metadata(&env.root, session_id)
        .map_err(|e| Error::Msg(format!("Failed to open session {session_id}: {e}")))?;
    ctx.with_ext(|o: &mut Orchid, ctx| o.update_session(ctx, meta.clone()))?;
    let restored = restore_events(&env, session_id)?;
    let count = restored.events.len();
    let buf = ctx.create_text(&buffer_name(&meta), "");
    set_directory(ctx, buf, &meta);
    start(ctx, buf, env, |c, ctx| c.lay_out(ctx, &meta, restored, ""))?;
    let w = ctx.show(buf, ShowIn::Current)?;
    to_end(ctx, buf, w)?;
    ctx.with_ext(|o: &mut Orchid, ctx| attach(o, ctx, buf))??;
    let _ = log::log(
        ctx,
        &format!("Opened {session_id}, {count} events restored"),
    );
    Ok(())
}

/// Create a session with `agent` working in `working_dir`, and open a chat on it in the selected
/// window: at once a `*orchid-chat-<agent>-<wsdir>-new*` buffer you can type in, then the
/// session's chat in its place once orchid has made it, with what you typed.
pub fn new_session(ctx: &mut Ctx, agent: &str, working_dir: &Path) -> Result {
    let env = ctx.with_ext(|o: &mut Orchid, _| Env::of(o))?;
    let name = format!(
        "*orchid-chat-{}-{}-new*",
        agent.to_lowercase(),
        dir_name(working_dir).to_lowercase()
    );
    let buf = ctx.create_text(&name, "");
    ctx.set_directory(buf, working_dir);
    let pending = Pending {
        agent: agent.to_string(),
        dir: working_dir.to_path_buf(),
    };
    start(ctx, buf, env, |c, ctx| c.lay_out_pending(ctx, pending, ""))?;
    let w = ctx.show(buf, ShowIn::Current)?;
    to_end(ctx, buf, w)?;
    ctx.with_ext(|o: &mut Orchid, ctx| attach(o, ctx, buf))??;
    create(ctx, buf)
}

/// `orchid create` for a pending chat, in a job.
fn create(ctx: &mut Ctx, buf: BufferId) -> Result {
    let (cli, pending) = with_chat(ctx, buf, |c, _| Ok((c.env.cli.clone(), c.pending.clone())))?;
    let Some(p) = pending else {
        return Ok(());
    };
    let cmd = Cmd::Create {
        agent: Some(p.agent.clone()),
        label: None,
        working_dir: Some(p.dir.clone()),
    };
    let done = move |ctx: &mut Ctx, out: Outcome| created(ctx, buf, out);
    cli.run(ctx, cmd, Some(p.dir), done).detach();
    Ok(())
}

/// The id in `create`'s answer, flat or wrapped, as orchid's versions have given it.
fn id_of(v: &Value) -> Option<String> {
    let wrapped = || v.get("session").and_then(|s| s.get("id"));
    let id = v.get("id").or_else(wrapped).or_else(|| v.get("session_id"));
    id.and_then(Value::as_str).map(String::from)
}

/// orchid made the session (or didn't): the real chat replaces the pending one in each window
/// that showed it, with what was typed, and the session gets its label, as Emacs's did.
fn created(ctx: &mut Ctx, pending: BufferId, out: Outcome) -> Result {
    if !ctx.is_live(pending) {
        return Ok(());
    }
    let (env, p) = with_chat(ctx, pending, |c, _| Ok((c.env.clone(), c.pending.clone())))?;
    let Some(p) = p else {
        return Ok(());
    };
    let failed = |ctx: &mut Ctx, why: String| -> Result {
        let _ = log::log(ctx, &why);
        ctx.with_ext(|o: &mut Orchid, _| o.untrack(pending))?;
        on_chat(ctx, pending, |c, ctx| c.system(ctx, &why))
    };
    let v = match out {
        Ok(v) => v,
        Err(e) => return failed(ctx, format!("Failed to create session:\n{}", e.message)),
    };
    let Some(id) = id_of(&v) else {
        return failed(
            ctx,
            format!("Failed to create session: response had no id:\n{v}"),
        );
    };
    let meta = match store::read_metadata(&env.root, &id) {
        Ok(m) => m,
        Err(e) => {
            return failed(
                ctx,
                format!("Failed to create session: {id}'s metadata.json: {e}"),
            );
        }
    };
    ctx.with_ext(|o: &mut Orchid, ctx| o.update_session(ctx, meta.clone()))?;
    let restored = restore_events(&env, &id)?;
    let (input, at) = with_chat(ctx, pending, |c, ctx| {
        let t = ctx.text(pending)?;
        let at = c.input.at(t);
        Ok((t.rope().byte_slice(at..).to_string(), at))
    })?;
    let was = cursors(ctx, pending, Some(at));
    let typing = ctx.state_of(pending) == State::Insert;
    let buf = ctx.create_text(&buffer_name(&meta), "");
    ctx.set_directory(buf, &p.dir);
    set_directory(ctx, buf, &meta);
    start(ctx, buf, env.clone(), |c, ctx| {
        c.lay_out(ctx, &meta, restored, &input)
    })?;
    let focused = ctx.focused();
    for (w, _) in &was {
        ctx.focus(*w);
        ctx.show(buf, ShowIn::Current)?;
    }
    if let Some(f) = focused.filter(|w| ctx.window_list().contains(w)) {
        ctx.focus(f);
    }
    put_cursors(ctx, buf, was.clone())?;
    ctx.with_ext(|o: &mut Orchid, ctx| {
        o.untrack(pending);
        attach(o, ctx, buf)
    })??;
    // Typing in insert state goes on there.
    let w = focused
        .filter(|w| ctx.window_buffer(*w) == Some(buf))
        .or_else(|| was.first().map(|x| x.0));
    if let (true, Some(w)) = (typing, w) {
        let args = Args {
            buffer: Some(buf),
            window: Some(w),
            ..Args::default()
        };
        match ctx.run("vim-insert", &args) {
            Err(Error::NoCommand(_)) => {}
            r => r?,
        }
    }
    ctx.kill(pending);
    let _ = log::log(ctx, &format!("[open-new] created {id}"));
    let label = format!("{}-{}", p.agent.to_lowercase(), dir_name(&p.dir));
    let set = Cmd::Set {
        id,
        label: Some(label),
        working_dir: None,
    };
    env.cli.run(ctx, set, None, |_, _| Ok(())).detach();
    Ok(())
}

/// Bring the chat back in `buf`, which the session restored as plain text, from what its tracking
/// kept: rebuilt from `events.jsonl`, its input kept, following again. It runs with the
/// extension checked out (`orchid`).
pub(crate) fn restore(orchid: &mut Orchid, ctx: &mut Ctx, buf: BufferId, state: Value) -> Result {
    let env = Env::of(orchid);
    let input = state
        .get("input")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let text = ctx.text(buf)?.rope().to_string();
    let old_input = text.ends_with(&input).then(|| text.len() - input.len());
    let was = cursors(ctx, buf, old_input);
    if let Some(p) = state.get("pending") {
        let pending = Pending {
            agent: p
                .get("agent")
                .and_then(Value::as_str)
                .unwrap_or("default")
                .into(),
            dir: p.get("dir").and_then(Value::as_str).unwrap_or("/").into(),
        };
        start(ctx, buf, env, |c, ctx| {
            c.lay_out_pending(ctx, pending, &input)
        })?;
        put_cursors(ctx, buf, was)?;
        attach(orchid, ctx, buf)?;
        return create(ctx, buf);
    }
    let id = state
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| Error::Msg("a chat kept without its session".into()))?;
    match store::read_metadata(&env.root, id) {
        Ok(meta) => {
            orchid.update_session(ctx, meta.clone());
            let restored = restore_events(&env, id)?;
            start(ctx, buf, env, |c, ctx| {
                c.lay_out(ctx, &meta, restored, &input)
            })?;
        }
        Err(_) => start(ctx, buf, env, |c, ctx| c.lay_out_gone(ctx, id, &input))?,
    }
    put_cursors(ctx, buf, was)?;
    attach(orchid, ctx, buf)
}

// ------------------------------------------------------------------ the slash menu

/// `/` in insert state: alone at the start of a line of your input, the slash menu; anywhere
/// else, a `/`.
fn slash_key(ctx: &mut Ctx, args: &Args) -> Result {
    let (buf, w) = target(ctx, args)?;
    if with_chat(ctx, buf, |c, ctx| c.slash_here(ctx, w))? {
        ctx.defer(move |ctx| slash_menu(ctx, buf));
        return Ok(());
    }
    match ctx.run("vim-insert-char", args) {
        Err(Error::NoCommand(_)) => {}
        r => return r,
    }
    let t = ctx.text_mut(buf)?;
    let here = w.and_then(|w| t.cursor(w)).unwrap_or(t.len());
    t.tx(Origin::User).insert(here, "/").commit()?;
    Ok(())
}

/// The slash menu, `/ `, which wants one of its commands. A cancel does nothing.
fn slash_menu(ctx: &mut Ctx, buf: BufferId) -> Result {
    let spec = PromptSpec::new("/ ").candidates(SLASH).require_match();
    ctx.prompt(spec, move |ctx, answer| {
        let chosen = answer.and_then(|a| SLASH.iter().position(|l| *l == a));
        match chosen {
            Some(i) => slash(ctx, buf, SLASH_DOES[i]),
            None => Ok(()),
        }
    });
    Ok(())
}

fn slash(ctx: &mut Ctx, buf: BufferId, which: Slash) -> Result {
    let (id, env, meta, pending) = with_chat(ctx, buf, |c, _| {
        Ok((
            c.id.clone(),
            c.env.clone(),
            c.meta.clone(),
            c.pending.clone(),
        ))
    })?;
    let session = || {
        id.clone()
            .ok_or_else(|| Error::Msg("orchid is still making this session".into()))
    };
    let visit = |ctx: &mut Ctx, path: PathBuf| -> Result {
        let file = ctx.open_file(&path)?;
        ctx.show(file, ShowIn::Current)?;
        Ok(())
    };
    match which {
        Slash::Metadata => visit(ctx, store::metadata_path(&env.root, &session()?)),
        Slash::Logs => visit(ctx, store::logs_path(&env.root, &session()?)),
        Slash::Config => {
            // The find-file prompt starts at the focused buffer's directory: the root, for now.
            let before = ctx.directory(buf).map(Path::to_path_buf);
            ctx.set_directory(buf, &env.root);
            let asked = ctx.run("find-file", &Args::default());
            if let Some(d) = before {
                ctx.set_directory(buf, d);
            }
            asked
        }
        Slash::Kill => {
            let stop = Cmd::Stop { id: session()? };
            let done = |ctx: &mut Ctx, out: Outcome| {
                let said = match out {
                    Ok(_) => "stopped".to_string(),
                    Err(e) => e.message,
                };
                ctx.messages.push(format!("[orchid] kill: {said}"));
                Ok(())
            };
            env.cli.run(ctx, stop, None, done).detach();
            Ok(())
        }
        Slash::New => {
            let here = ctx.directory(buf).map(Path::to_path_buf);
            let (agent, dir) = match (&meta, pending) {
                (Some(m), _) => {
                    let dir = m
                        .working_dir
                        .as_deref()
                        .map(PathBuf::from)
                        .filter(|d| d.is_dir());
                    (parse::agent_name(m).to_string(), dir.or(here))
                }
                (None, Some(p)) => (p.agent, Some(p.dir)),
                (None, None) => ("default".to_string(), here),
            };
            let dir = dir.unwrap_or_else(|| "/".into());
            new_session(ctx, &agent, &dir)
        }
        Slash::Quit => {
            ctx.kill(buf);
            Ok(())
        }
    }
}

// ------------------------------------------------------------------ commands

/// `orchid-collapsible-report-stats`: the lazy details' stats, in a buffer of their own.
fn report_stats(ctx: &mut Ctx, _: &Args) -> Result {
    let text = sections::report(sections::stats());
    let root = ctx.with_ext(|o: &mut Orchid, _| o.root().to_path_buf())?;
    let buf = ctx.output_buffer(STATS_BUFFER, &text, &root)?;
    crate::display_buffer(ctx, buf)
}

/// Its mode, keys and commands.
pub(crate) fn register(reg: &mut Registry) {
    use State::{Insert, Normal, Visual};
    let m = reg.mode(MODE, Chat::default);
    for s in [Normal, Insert] {
        m.bind(s, "<Tab>", "orchid-chat-handle-tab")
            .bind(s, "<S-CR>", "orchid-chat-newline");
    }
    for s in [Normal, Insert, Visual] {
        m.bind(s, "<C-c><C-q>", "orchid-chat-close")
            .bind(s, "<C-c><C-l>", "orchid-chat-show-session-browser")
            .bind(s, "<S-Tab>", "orchid-chat-show-session-browser");
    }
    m.bind(Insert, "<CR>", "orchid-chat-send-input").bind(
        Insert,
        "/",
        "orchid-chat-slash-maybe-open",
    );
    reg.command(
        "orchid-chat-send-input",
        "RET in a chat's insert state: send what's after the input marker, trimmed, unless it's empty. Before the marker, a newline.",
        |ctx, a| {
            let (buf, w) = target(ctx, a)?;
            with_chat(ctx, buf, |c, ctx| c.send_input(ctx, w, a))
        },
    );
    reg.command(
        "orchid-chat-newline",
        "S-RET in a chat: a newline, as in your input.",
        |ctx, a| {
            let (buf, w) = target(ctx, a)?;
            with_chat(ctx, buf, |_, _| Ok(()))?;
            newline(ctx, buf, w, a)
        },
    );
    reg.command(
        "orchid-chat-handle-tab",
        "TAB in a chat: expand or collapse the section at the cursor; in the input, a tab.",
        |ctx, a| {
            let (buf, w) = target(ctx, a)?;
            with_chat(ctx, buf, |c, ctx| c.handle_tab(ctx, w))
        },
    );
    reg.command(
        "orchid-chat-close",
        "C-c C-q in a chat: kill it. The session goes on, in orchid.",
        |ctx, a| {
            let (buf, _) = target(ctx, a)?;
            with_chat(ctx, buf, |_, _| Ok(()))?;
            ctx.kill(buf);
            Ok(())
        },
    );
    reg.command(
        "orchid-chat-show-session-browser",
        "C-c C-l or S-Tab in a chat: the session browser, in this window.",
        |ctx, _| crate::browser::show(ctx),
    );
    reg.command(
        "orchid-chat-load-more-history",
        "Show a chat's older events, above those shown.",
        |ctx, a| {
            let (buf, _) = target(ctx, a)?;
            with_chat(ctx, buf, |c, ctx| c.load_more(ctx))
        },
    );
    reg.command(
        "orchid-chat-slash-maybe-open",
        "/ in a chat's insert state: alone at the start of a line of the input, the slash menu (metadata, logs, config, kill, new, quit); anywhere else, a /.",
        slash_key,
    );
    reg.command(
        sections::REVEAL,
        "What a search runs when it lands in a chat's collapsed section: expand it.",
        |ctx, a| {
            let (buf, _) = target(ctx, a)?;
            let Some(deco) = a.deco else {
                return Ok(());
            };
            with_chat(ctx, buf, |c, ctx| reveal(c, ctx, deco))
        },
    );
    reg.command(
        "orchid-collapsible-report-stats",
        "How many lazy sections the chats made, how many were expanded, and the time that took.",
        report_stats,
    );
}

fn reveal(c: &mut Live, ctx: &mut Ctx, deco: DecorationId) -> Result {
    let t = ctx.text_mut(c.buf)?;
    c.sections.reveal(t, deco)?;
    Ok(())
}
