//! The session browser, `*orchid-sessions*` (the Emacs client's `orchid-session-browser-mode`,
//! drawn by seek, [`crate::seek`]): a read-only buffer in the selected window, with a row per
//! session, the one changed last first, and a selected row you move with `j` and `k` and open
//! with `RET`, which kills the browser. Every change draws it whole again, as seek did.
//!
//! A row is `LABEL` (the label, else `<agent>-<workspace>`, else the id), `STATUS` (`ACTIVE`
//! while a chat is open on it, `IDLE` otherwise, with `•RUN` while it runs; `DEL` and `KILL` when
//! marked) and `UPDATED` (how long ago). The rows come from every session's `metadata.json`
//! ([`crate::refresh_sessions`]), where the Emacs client had `orchid list`'s ids and labels alone,
//! so its UPDATED said `N/A`, its order was the creation order, and `•RUN` showed only for a
//! session whose chat had ticked (A1).
//!
//! Keys, in normal state, as your Emacs has them under Evil: `RET` open, `n` new, `q` quit, `r`
//! and `g` refresh, `D` mark to delete, `S` mark to stop, `u` unmark, `x` run the marks, `/` and
//! `a` search, `d` clear the search, `j` and `k` down and up, `J` and `K` a page, and `p` up,
//! which Evil's list left out, so `p` pasted (B6). Searching is a second mode over the first,
//! whose `<char>` takes every printable key, as the Emacs client's search map did after it put
//! Evil in emacs state: what you type filters, chars in order, `DEL` takes one back, `RET` and
//! `Esc` keep the query and stop, `C-g` clears it.
//!
//! It stays current by itself: a session's status changing redraws it
//! ([`Orchid::on_status_change`]), and while a window shows it, the shared poll's 2 s tick reads
//! the sessions again, to see what changed elsewhere, as a turn you started from Emacs. A redraw
//! over a changed list keeps the selection on its session. Killing it stops both. A restart
//! brings it back as it was: the query, the selection, the marks.

use crate::cli::{Cmd, Outcome};
use crate::parse::{agent_name, relative_time, truncate_width, workspace_name};
use crate::poll::{self, Polling};
use crate::seek::{self, Line, View};
use crate::store::Metadata;
use crate::{Kind, Orchid, StatusWatch, chat, face};
use die_core::session::{Value, json};
use die_core::{
    Args, BufferId, Checkout, Ctx, Error, Mode, Origin, Registry, Result, ShowIn, State, Sub,
    TextBuf, WindowId, Wrap,
};
use jiff::Timestamp;
use std::ops::Range;
use std::path::Path;
use std::time::Duration;

/// The buffer's name.
pub const BUFFER: &str = "*orchid-sessions*";
/// Its mode, which holds the selection, the query and the marks, and binds the browser's keys.
pub const MODE: &str = "orchid-session-browser-mode";
/// The mode searching adds over it, whose keys come first.
pub const SEARCH_MODE: &str = "orchid-session-browser-search";
/// The widest a line is drawn, however wide the window (`orchid-session-browser--max-row-width`).
pub const MAX_ROW_WIDTH: usize = 78;
/// The title, centred in the title bar.
const TITLE: &str = " Orchid Sessions ";
/// How long a scan of the sessions counts as still running, so a tick doesn't start another;
/// past it, one that never came back is given up on.
const SCAN_WAIT: Duration = Duration::from_secs(30);

// ------------------------------------------------------------------ rows

/// A session as a row shows it.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    /// Its id.
    pub id: String,
    /// What LABEL says.
    pub label: String,
    /// Whether it's running: `•RUN`.
    pub running: bool,
    /// Whether a chat is open on it: `ACTIVE`.
    pub active: bool,
    /// When it last changed: UPDATED, and the order.
    pub updated_at: Option<Timestamp>,
}

impl Row {
    /// The row for `meta`, with a chat open on it or not.
    pub fn of(meta: &Metadata, active: bool) -> Row {
        Row {
            id: meta.id.clone(),
            label: label(meta),
            running: meta.status.is_running(),
            active,
            updated_at: meta.updated_at,
        }
    }

    /// Whether `query` picks it: its chars in order, in its label, its status and its raw
    /// `updated_at` (`orchid-session-browser--filter`).
    fn matches(&self, query: &str) -> bool {
        let updated = self.updated_at.map(|t| t.to_string()).unwrap_or_default();
        let status = status_string(self.active, self.running);
        fuzzy_match(query, &format!("{}{status}{updated}", self.label))
    }
}

/// LABEL: the label; else `<agent>-<workspace>` with a working directory; else the id; else
/// `unknown` (`orchid-browser-format-label`). The agent is `agent_name` (A2).
pub fn label(meta: &Metadata) -> String {
    if let Some(l) = &meta.label {
        return l.clone();
    }
    let workspace = workspace_name(meta.working_dir.as_deref());
    if workspace != "N/A" {
        return format!("{}-{workspace}", agent_name(meta));
    }
    if meta.id.is_empty() {
        "unknown".into()
    } else {
        meta.id.clone()
    }
}

/// STATUS for a session that isn't marked (`orchid-browser-format-status`).
pub fn status_string(active: bool, running: bool) -> &'static str {
    match (active, running) {
        (true, true) => "ACTIVE•RUN",
        (true, false) => "ACTIVE",
        (false, true) => "IDLE•RUN",
        (false, false) => "IDLE",
    }
}

/// Whether every char of `query` is in `text`, in order, whatever the case
/// (`orchid-browser--fuzzy-match`).
pub fn fuzzy_match(query: &str, text: &str) -> bool {
    let text = text.to_lowercase();
    let mut rest = text.chars();
    query
        .to_lowercase()
        .chars()
        .all(|q| rest.by_ref().any(|c| c == q))
}

/// The rows, in the browser's order (`orchid-session-browser--sort-sessions`).
fn rows(o: &Orchid, ctx: &Ctx) -> Vec<Row> {
    let mut rows: Vec<Row> = o
        .sessions()
        .values()
        .map(|s| Row::of(s, o.chat_buffer(ctx, &s.id).is_some()))
        .collect();
    rows.sort_by(newest_first);
    rows
}

/// The browser's order: the session changed last first, those with no time last, and ties by id.
fn newest_first(a: &Row, b: &Row) -> std::cmp::Ordering {
    let by_time = match (a.updated_at, b.updated_at) {
        (Some(x), Some(y)) => y.cmp(&x),
        (x, y) => x.is_none().cmp(&y.is_none()),
    };
    by_time.then_with(|| a.id.cmp(&b.id))
}

/// What a mark does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mark {
    /// `D`: `orchid delete`, which archives the session.
    Delete,
    /// `S`: `orchid stop`, which stops its turn.
    Kill,
}

impl Mark {
    fn name(self) -> &'static str {
        match self {
            Mark::Delete => "delete",
            Mark::Kill => "kill",
        }
    }
    fn named(name: &str) -> Option<Mark> {
        match name {
            "delete" => Some(Mark::Delete),
            "kill" => Some(Mark::Kill),
            _ => None,
        }
    }
    fn cmd(self, id: &str) -> Cmd {
        let id = id.to_string();
        match self {
            Mark::Delete => Cmd::Delete { id },
            Mark::Kill => Cmd::Stop { id },
        }
    }
}

/// The columns' titles: `%-30s  %-10s %s`.
fn columns() -> String {
    format!("{:<30}  {:<10} {}", "LABEL", "STATUS", "UPDATED")
}

/// A row's line, as `orchid-browser-format-entry-line` lays it out: LABEL in 30 columns, cut
/// with `..`, two spaces, STATUS in 10, a space, and UPDATED, cut at 18 with `..`. A marked row
/// is wholly `error` (`DEL`) or `warning` (`KILL`); otherwise STATUS is in the active or idle
/// face and UPDATED in `shadow`.
fn entry_line(row: &Row, mark: Option<Mark>, now: Timestamp) -> Line {
    let label = truncate_width(&row.label, 30, "..");
    let updated = truncate_width(&relative_time(row.updated_at, now), 18, "..");
    let (status, status_face, row_face) = match mark {
        Some(Mark::Delete) => ("DEL", "error", Some("error")),
        Some(Mark::Kill) => ("KILL", "warning", Some("warning")),
        None if row.active => (status_string(true, row.running), face::BROWSER_ACTIVE, None),
        None => (status_string(false, row.running), face::BROWSER_IDLE, None),
    };
    let mut line = Line::new(&label, row_face);
    line.pad(30)
        .push("  ", None)
        .push(status, Some(status_face));
    line.pad(42)
        .push(" ", None)
        .push(&updated, row_face.or(Some("shadow")));
    line
}

// ------------------------------------------------------------------ the modes

/// The browser's state, per buffer, as the Emacs client's buffer-local variables held it.
#[derive(Default)]
struct Browser {
    /// The selected row, among those the query keeps.
    selected: usize,
    /// The first row shown.
    scroll: usize,
    /// The query.
    query: String,
    /// Whether the query is being typed, with [`Search`] on.
    searching: bool,
    /// The marks, in the order they were made.
    marks: Vec<(String, Mark)>,
    /// The session selected at the last draw. A draw over a list that changed keeps the
    /// selection on it; a key that moves the selection forgets it first.
    on: Option<String>,
    /// Whether the marks are running.
    executing: bool,
    /// The faces of the text drawn last, and its revision: a provider of faces draws them only
    /// on that text.
    spans: Vec<(Range<usize>, &'static str)>,
    drawn: Option<u64>,
    /// Whether a redraw is queued.
    pending: bool,
    /// The scan running for it, and when it started.
    scan: Option<(Sub, Duration)>,
    /// Its subscriptions, which go with it.
    watch: Option<StatusWatch>,
    polling: Option<Polling>,
}

/// Searching: keys over the browser's, the Emacs client's `orchid-session-browser-search-map`.
struct Search;

impl Mode for Search {}

impl Mode for Browser {
    /// Hear of status changes, and look at the sessions on the shared poll.
    fn on_enable(&mut self, buf: BufferId, ctx: &mut Ctx) -> Result {
        let watch =
            ctx.with_ext(|o: &mut Orchid, _| o.on_status_change(move |ctx, _| schedule(ctx, buf)))?;
        self.watch = Some(watch);
        self.polling = Some(poll::subscribe(ctx, move |ctx| tick(ctx, buf))?);
        Ok(())
    }

    /// A window showing it changed size, or came to show it: draw it at that width.
    fn on_resize(&mut self, win: WindowId, _: usize, _: usize, ctx: &mut Ctx) {
        if let Some(buf) = ctx.window_buffer(win) {
            self.schedule(ctx, buf);
        }
    }

    fn decorate(
        &mut self,
        text: &TextBuf,
        visible: Range<usize>,
        out: &mut Vec<(Range<usize>, &'static str)>,
    ) {
        if self.drawn != Some(text.rev()) {
            return;
        }
        let shown =
            |(r, _): &&(Range<usize>, &'static str)| r.start < visible.end && r.end > visible.start;
        out.extend(self.spans.iter().filter(shown).cloned());
    }
}

impl Browser {
    /// Queue a redraw, unless one is.
    fn schedule(&mut self, ctx: &mut Ctx, buf: BufferId) {
        if !self.pending {
            self.pending = true;
            ctx.defer(move |ctx| {
                let take = |b: &mut Browser, _: &mut Ctx| std::mem::take(&mut b.pending);
                match quiet(ctx.with_mode(buf, take))? {
                    Some(true) => redraw(ctx, buf),
                    _ => Ok(()),
                }
            });
        }
    }

    /// The mark on `id`.
    fn mark(&self, id: &str) -> Option<Mark> {
        self.marks.iter().find(|(m, _)| m == id).map(|(_, k)| *k)
    }

    /// Mark `id`, or mark it again.
    fn set_mark(&mut self, id: String, mark: Mark) {
        match self.marks.iter_mut().find(|(m, _)| *m == id) {
            Some(m) => m.1 = mark,
            None => self.marks.push((id, mark)),
        }
    }

    /// Stop searching, keeping the query (`orchid-session-browser-search-exit`).
    fn leave_search(&mut self, ctx: &mut Ctx, buf: BufferId) {
        if std::mem::take(&mut self.searching) {
            let _ = ctx.disable_mode::<Search>(buf);
        }
    }

    /// Draw the browser for `all`, the rows, at the size of the window showing it, and say
    /// what a restart keeps of it.
    fn draw(&mut self, ctx: &mut Ctx, buf: BufferId, all: &[Row], now: Timestamp) -> Result<Value> {
        self.pending = false;
        let (width, body) = size(ctx, buf);
        let rows = seek::filtered(all, &self.query, |q, r| r.matches(q));
        if let Some(on) = self.on.take()
            && rows.get(self.selected).is_none_or(|r| r.id != on)
            && let Some(i) = rows.iter().position(|r| r.id == on)
        {
            self.selected = i;
        }
        let shown = seek::visible(body, seek::chrome(0));
        (self.selected, self.scroll) = seek::clamp(rows.len(), self.selected, self.scroll, shown);
        self.on = rows.get(self.selected).map(|r| r.id.clone());
        let columns = columns();
        let view = View {
            title: TITLE,
            header_lines: &[],
            columns: &columns,
            search: &self.query,
            searching: self.searching,
            selected: self.selected,
            scroll: self.scroll,
        };
        let line = |r: &&Row, _, _| entry_line(r, self.mark(&r.id), now);
        let lines: Vec<Line> = seek::render(&view, &rows, width, body, line)
            .into_iter()
            .map(|l| l.truncated(MAX_ROW_WIDTH))
            .collect();
        let (text, spans) = seek::join(&lines);
        let windows: Vec<WindowId> = ctx
            .window_list()
            .into_iter()
            .filter(|w| ctx.window_buffer(*w) == Some(buf))
            .collect();
        let t = ctx.text_mut(buf)?;
        if *t.rope() != *text {
            let whole = 0..t.len();
            t.tx(Origin::Extension)
                .silent()
                .replace(whole, text)
                .commit()?;
            t.set_modified(false);
        }
        self.drawn = Some(t.rev());
        self.spans = spans;
        // The cursor isn't drawn: it sits on the selected row, so a short window shows it.
        let line = seek::chrome(0) + self.selected.saturating_sub(self.scroll);
        let at = t.line_start(line.min(t.line_count() - 1));
        for w in windows {
            t.set_scroll(w, 0, 0);
            t.set_cursor(w, at);
        }
        Ok(self.kept())
    }

    /// What a restart keeps.
    fn kept(&self) -> Value {
        let marks: Vec<Value> = self
            .marks
            .iter()
            .map(|(id, m)| json!([id, m.name()]))
            .collect();
        json!({
            "query": self.query,
            "searching": self.searching,
            "selected": self.selected,
            "scroll": self.scroll,
            "marks": marks,
        })
    }

    /// Take back what [`Browser::kept`] said, from this build or another.
    fn take(&mut self, state: &Value) {
        let text = |k| {
            state
                .get(k)
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string()
        };
        let number = |k| state.get(k).and_then(Value::as_u64).unwrap_or(0) as usize;
        self.query = text("query");
        self.searching = state.get("searching").and_then(Value::as_bool) == Some(true);
        (self.selected, self.scroll) = (number("selected"), number("scroll"));
        let marks = state.get("marks").and_then(Value::as_array);
        let mark = |m: &Value| {
            let id = m.get(0)?.as_str()?.to_string();
            Some((id, Mark::named(m.get(1)?.as_str()?)?))
        };
        self.marks = marks.into_iter().flatten().filter_map(mark).collect();
    }
}

/// The width the browser is drawn at, at most [`MAX_ROW_WIDTH`], and the height rows go in: those
/// of the window showing it, the focused one first (seek's `window-width` and
/// `window-body-height`). Shown nowhere, it's drawn as if 78 wide and 24 high.
fn size(ctx: &Ctx, buf: BufferId) -> (usize, usize) {
    let showing = |w: &WindowId| ctx.window_buffer(*w) == Some(buf);
    let w = ctx
        .focused()
        .filter(showing)
        .or_else(|| ctx.windows_by_use().into_iter().find(showing));
    let g = w.and_then(|w| ctx.text(buf).ok()?.geometry(w));
    g.map_or((MAX_ROW_WIDTH, 24), |g| {
        (g.text_w.min(MAX_ROW_WIDTH), g.rows)
    })
}

/// A checkout's result, with a killed buffer, or one without the mode, as None.
fn quiet<T>(r: std::result::Result<T, Checkout>) -> Result<Option<T>> {
    match r {
        Ok(v) => Ok(Some(v)),
        Err(Checkout::Gone | Checkout::Missing) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Draw the browser in `buf` again, now. Nothing if it's gone.
fn redraw(ctx: &mut Ctx, buf: BufferId) -> Result {
    let now = Timestamp::now();
    let all = ctx.with_ext(|o: &mut Orchid, ctx| rows(o, ctx))?;
    let kept = quiet(ctx.with_mode(buf, |b: &mut Browser, ctx| b.draw(ctx, buf, &all, now)))?;
    if let Some(kept) = kept {
        let kept = kept?;
        ctx.with_ext(|o: &mut Orchid, _| o.track(buf, Kind::Browser, kept))?;
    }
    Ok(())
}

/// Queue a redraw of the browser in `buf`. Nothing if it's gone.
fn schedule(ctx: &mut Ctx, buf: BufferId) -> Result {
    quiet(ctx.with_mode(buf, |b: &mut Browser, ctx| b.schedule(ctx, buf)))?;
    Ok(())
}

/// Read every session's `metadata.json` again in a job, then redraw. The browser keeps the job,
/// so a kill forgets it.
fn refresh(ctx: &mut Ctx, buf: BufferId) -> Result {
    let done = move |ctx: &mut Ctx| {
        quiet(ctx.with_mode(buf, |b: &mut Browser, _| b.scan = None))?;
        schedule(ctx, buf)
    };
    let scan = crate::refresh_sessions(ctx, done)?;
    let started = ctx.now();
    quiet(ctx.with_mode(buf, |b: &mut Browser, _| b.scan = Some((scan, started))))?;
    Ok(())
}

/// The shared poll's tick: while a window shows the browser and no scan is out, scan.
fn tick(ctx: &mut Ctx, buf: BufferId) -> Result {
    let shown = ctx
        .window_list()
        .iter()
        .any(|w| ctx.window_buffer(*w) == Some(buf));
    let now = ctx.now();
    let out = |b: &mut Browser, _: &mut Ctx| {
        let scanning = b.scan.as_ref();
        scanning.is_some_and(|(_, at)| now.saturating_sub(*at) < SCAN_WAIT)
    };
    match quiet(ctx.with_mode(buf, out))? {
        Some(false) if shown => refresh(ctx, buf),
        _ => Ok(()),
    }
}

/// Make `buf` the browser: read-only, its lines cut rather than wrapped, no cursor drawn, no
/// undo, and the mode on. The width and centering stay yours, as a file's.
fn prepare(ctx: &mut Ctx, buf: BufferId) -> Result {
    ctx.change_settings(buf, |s| {
        (s.wrap, s.read_only, s.cursor_visible, s.undo_steps) = (Wrap::Off, true, false, 0);
    })?;
    ctx.enable_mode::<Browser>(buf)
}

/// `orchid`: show the browser in the selected window, and read the sessions again. It's drawn
/// at once from what the client knows, and again when the sessions are read.
pub fn show(ctx: &mut Ctx) -> Result {
    let buf = match ctx.find_buffer(BUFFER) {
        Some(buf) => buf,
        None => ctx.create_text(BUFFER, ""),
    };
    prepare(ctx, buf)?;
    ctx.show(buf, ShowIn::Current)?;
    let known = ctx.with_ext(|o: &mut Orchid, _| !o.sessions().is_empty())?;
    if known {
        redraw(ctx, buf)?;
    } else {
        let kept = ctx.with_mode(buf, |b: &mut Browser, _| b.kept())?;
        ctx.with_ext(|o: &mut Orchid, _| o.track(buf, Kind::Browser, kept))?;
    }
    refresh(ctx, buf)
}

/// Bring the browser back in `buf`, which the session restored as plain text, from the state it
/// was tracked with: the mode on again, the query, the selection and the marks back, and the
/// sessions read again, which draws it.
pub(crate) fn restore(orchid: &mut Orchid, ctx: &mut Ctx, buf: BufferId, state: Value) -> Result {
    prepare(ctx, buf)?;
    let searching = ctx.with_mode(buf, |b: &mut Browser, _| {
        b.take(&state);
        b.searching
    })?;
    if searching {
        ctx.enable_mode::<Search>(buf)?;
    }
    orchid.track(buf, Kind::Browser, state);
    ctx.defer(move |ctx| refresh(ctx, buf));
    Ok(())
}

/// What the browser in `buf` styles, as each face with the text it covers, in order: what a
/// frame's styles say less plainly, for tests.
pub fn faces(ctx: &mut Ctx, buf: BufferId) -> Result<Vec<(String, &'static str)>> {
    let spans = ctx.with_mode(buf, |b: &mut Browser, _| b.spans.clone())?;
    let text = ctx.text(buf)?.rope().to_string();
    let cover = |(r, f): (Range<usize>, &'static str)| text.get(r).map(|s| (s.to_string(), f));
    Ok(spans.into_iter().filter_map(cover).collect())
}

// ------------------------------------------------------------------ commands

/// The browser a command acts on: its buffer, or the focused one, which must be the browser.
fn target(ctx: &mut Ctx, args: &Args) -> Result<BufferId> {
    let not_here = || Error::Msg(format!("Not in {BUFFER}"));
    let buf = args
        .buffer
        .or_else(|| ctx.focused_buffer())
        .ok_or_else(not_here)?;
    match ctx.with_mode(buf, |_: &mut Browser, _| ()) {
        Ok(()) => Ok(buf),
        Err(Checkout::Missing | Checkout::Gone) => Err(not_here()),
        Err(e) => Err(e.into()),
    }
}

/// Change the browser a command acts on with `f`, then draw it.
fn change(ctx: &mut Ctx, args: &Args, f: impl FnOnce(&mut Browser, &mut Ctx, BufferId)) -> Result {
    let buf = target(ctx, args)?;
    ctx.with_mode(buf, |b: &mut Browser, ctx| f(b, ctx, buf))?;
    redraw(ctx, buf)
}

/// Move the selection by `rows`, or by a page each.
fn move_by(ctx: &mut Ctx, args: &Args, rows: isize, pages: bool) -> Result {
    change(ctx, args, |b, ctx, buf| {
        let page = seek::visible(size(ctx, buf).1, seek::chrome(0));
        let by = if pages { rows * page as isize } else { rows };
        b.selected = b.selected.saturating_add_signed(by);
        b.on = None;
    })
}

/// `RET`: open the selected session's chat in this window, the one open on it if there is one,
/// then kill the browser (`orchid-session-browser-select`).
fn select(ctx: &mut Ctx, args: &Args) -> Result {
    let buf = target(ctx, args)?;
    let Some(id) = ctx.with_mode(buf, |b: &mut Browser, _| b.on.clone())? else {
        return Ok(());
    };
    let open = ctx.with_ext(|o: &mut Orchid, ctx| o.chat_buffer(ctx, &id))?;
    match open {
        Some(chat) => {
            ctx.show(chat, ShowIn::Current)?;
        }
        None => chat::open(ctx, &id)?,
    }
    ctx.kill(buf);
    Ok(())
}

/// `n`: ask for an agent, then start a chat on a new session of it, in the browser's directory
/// (`orchid-session-browser-new`). The browser stays.
fn new_session(ctx: &mut Ctx, args: &Args) -> Result {
    let buf = target(ctx, args)?;
    let dir = ctx
        .directory(buf)
        .map(Path::to_path_buf)
        .unwrap_or_default();
    crate::read_agent(ctx, move |ctx, agent| chat::new_session(ctx, &agent, &dir))
}

/// `q`: kill the browser (`quit-window` with its kill).
fn quit(ctx: &mut Ctx, args: &Args) -> Result {
    let buf = target(ctx, args)?;
    ctx.with_mode(buf, |b: &mut Browser, ctx| b.leave_search(ctx, buf))?;
    ctx.kill(buf);
    Ok(())
}

/// `r` and `g`: stop searching, and read the sessions again.
fn refresh_command(ctx: &mut Ctx, args: &Args) -> Result {
    let buf = target(ctx, args)?;
    change(ctx, args, |b, ctx, buf| b.leave_search(ctx, buf))?;
    refresh(ctx, buf)
}

/// `D`: mark the selected session to delete, and move down.
fn mark_delete(ctx: &mut Ctx, args: &Args) -> Result {
    change(ctx, args, |b, _, _| {
        if let Some(id) = b.on.take() {
            b.set_mark(id, Mark::Delete);
            b.selected += 1;
        }
    })
}

/// `S`: mark the selected session to stop, if it's running, and move down.
fn mark_kill(ctx: &mut Ctx, args: &Args) -> Result {
    let buf = target(ctx, args)?;
    let Some(id) = ctx.with_mode(buf, |b: &mut Browser, _| b.on.clone())? else {
        return Ok(());
    };
    let running =
        ctx.with_ext(|o: &mut Orchid, _| o.session(&id).is_some_and(|s| s.status.is_running()))?;
    if !running {
        ctx.messages
            .push("Session has no running process to kill".into());
        return Ok(());
    }
    change(ctx, args, |b, _, _| {
        b.set_mark(id, Mark::Kill);
        b.selected += 1;
        b.on = None;
    })
}

/// `u`: take the selected session's mark off, staying there.
fn unmark(ctx: &mut Ctx, args: &Args) -> Result {
    change(ctx, args, |b, _, _| {
        if let Some(id) = &b.on {
            b.marks.retain(|(m, _)| m != id);
        }
    })
}

/// `x`: ask, with `yes` typed out, then delete or stop each marked session, in order, in one
/// job, keep the marks that failed, read the sessions again and say what was done
/// (`orchid-session-browser-execute`). Nothing without marks.
fn execute(ctx: &mut Ctx, args: &Args) -> Result {
    let buf = target(ctx, args)?;
    let (marks, executing) =
        ctx.with_mode(buf, |b: &mut Browser, _| (b.marks.clone(), b.executing))?;
    if marks.is_empty() {
        return Ok(());
    }
    if executing {
        ctx.messages
            .push("The marked operations are still running".into());
        return Ok(());
    }
    let question = format!("Execute {} marked operation(s)? ", marks.len());
    crate::yes_or_no(ctx, &question, move |ctx, yes| {
        if yes {
            run_marks(ctx, buf, marks)
        } else {
            Ok(())
        }
    });
    Ok(())
}

/// Run the marks, one after another in one job.
fn run_marks(ctx: &mut Ctx, buf: BufferId, marks: Vec<(String, Mark)>) -> Result {
    if quiet(ctx.with_mode(buf, |b: &mut Browser, _| b.executing))? == Some(true) {
        return Ok(());
    }
    quiet(ctx.with_mode(buf, |b: &mut Browser, _| b.executing = true))?;
    let cmds = marks.iter().map(|(id, m)| m.cmd(id)).collect();
    let done = move |ctx: &mut Ctx, outcomes| marks_done(ctx, buf, marks, outcomes);
    crate::cli(ctx)?.run_all(ctx, cmds, None, done).detach();
    Ok(())
}

/// The marks ran: drop those that did, then read the sessions again, and say how it went, as
/// `Deleted 1 session, Killed 2 processes, 1 failed`. What failed and why goes in
/// `*Orchid Log*`.
fn marks_done(
    ctx: &mut Ctx,
    buf: BufferId,
    marks: Vec<(String, Mark)>,
    outcomes: Vec<Outcome>,
) -> Result {
    let (mut deleted, mut killed, mut failed) = (0, 0, 0);
    let mut done = vec![];
    for ((id, mark), out) in marks.into_iter().zip(outcomes) {
        match (out, mark) {
            (Ok(_), Mark::Delete) => deleted += 1,
            (Ok(_), Mark::Kill) => killed += 1,
            (Err(e), _) => {
                failed += 1;
                crate::log::log(ctx, &format!("{} {id} failed: {e}", mark.cmd(&id).name()))?;
                continue;
            }
        }
        done.push(id);
    }
    quiet(ctx.with_mode(buf, |b: &mut Browser, ctx| {
        b.executing = false;
        b.marks.retain(|(id, _)| !done.contains(id));
        b.leave_search(ctx, buf);
    }))?;
    let plural =
        |n: usize, one: &str, many: &str| format!("{n} {}", if n == 1 { one } else { many });
    let mut said = vec![];
    if deleted > 0 {
        said.push(format!(
            "Deleted {}",
            plural(deleted, "session", "sessions")
        ));
    }
    if killed > 0 {
        said.push(format!("Killed {}", plural(killed, "process", "processes")));
    }
    if failed > 0 {
        said.push(format!("{failed} failed"));
    }
    let said = said.join(", ");
    let shown = move |ctx: &mut Ctx| {
        redraw(ctx, buf)?;
        ctx.messages.push(said);
        Ok(())
    };
    crate::refresh_sessions(ctx, shown)?.detach();
    Ok(())
}

/// `/` and `a`: search (`orchid-session-browser-search`).
fn search(ctx: &mut Ctx, args: &Args) -> Result {
    change(ctx, args, |b, ctx, buf| {
        if !b.searching {
            b.searching = true;
            let _ = ctx.enable_mode::<Search>(buf);
        }
    })
}

/// `d`, and `C-g` while searching: clear the query, go back to the first row, and stop searching.
fn search_clear(ctx: &mut Ctx, args: &Args) -> Result {
    change(ctx, args, |b, ctx, buf| {
        b.query.clear();
        (b.selected, b.scroll, b.on) = (0, 0, None);
        b.leave_search(ctx, buf);
    })
}

/// `RET` and `Esc` while searching: stop, keeping the query.
fn search_exit(ctx: &mut Ctx, args: &Args) -> Result {
    change(ctx, args, |b, ctx, buf| b.leave_search(ctx, buf))
}

/// A printable key while searching: add it to the query.
fn search_insert(ctx: &mut Ctx, args: &Args) -> Result {
    let Some(typed) = args.typed() else {
        return Ok(());
    };
    change(ctx, args, |b, _, _| {
        b.query.push_str(&typed);
        b.on = None;
    })
}

/// `DEL` while searching: take the query's last character back.
fn search_backspace(ctx: &mut Ctx, args: &Args) -> Result {
    change(ctx, args, |b, _, _| {
        if b.query.pop().is_some() {
            b.on = None;
        }
    })
}

type Command = fn(&mut Ctx, &Args) -> Result;

/// The browser's commands, with the Emacs client's names and docs, and its keys.
const COMMANDS: &[(&str, &str, Command, &[&str])] = &[
    (
        "select",
        "Open the currently selected session and close the browser.",
        select,
        &["<CR>"],
    ),
    (
        "new",
        "Start a new chat session, prompting for agent selection.",
        new_session,
        &["n"],
    ),
    ("quit", "Quit session browser.", quit, &["q"]),
    (
        "refresh",
        "Read the sessions again and update the browser.",
        refresh_command,
        &["r", "g"],
    ),
    (
        "mark-for-deletion",
        "Mark selected session for deletion.",
        mark_delete,
        &["D"],
    ),
    (
        "mark-for-kill",
        "Mark selected session for process kill.",
        mark_kill,
        &["S"],
    ),
    (
        "unmark",
        "Remove mark from selected session.",
        unmark,
        &["u"],
    ),
    (
        "execute",
        "Execute all marked operations (delete sessions, stop processes).",
        execute,
        &["x"],
    ),
    ("search", "Enter search mode.", search, &["/", "a"]),
    (
        "search-clear",
        "Clear search query and exit search mode.",
        search_clear,
        &["d"],
    ),
    (
        "move-down",
        "Move selection down one row.",
        |c, a| move_by(c, a, 1, false),
        &["j"],
    ),
    (
        "move-up",
        "Move selection up one row.",
        |c, a| move_by(c, a, -1, false),
        &["k", "p"],
    ),
    (
        "page-down",
        "Move selection down one page.",
        |c, a| move_by(c, a, 1, true),
        &["J"],
    ),
    (
        "page-up",
        "Move selection up one page.",
        |c, a| move_by(c, a, -1, true),
        &["K"],
    ),
];

/// Searching's commands, and their keys over the browser's.
const SEARCH_COMMANDS: &[(&str, &str, Command, &[&str])] = &[
    (
        "search-exit",
        "Exit search mode.",
        search_exit,
        &["<CR>", "<Esc>"],
    ),
    (
        "search-self-insert",
        "Append the last typed character to the search query.",
        search_insert,
        &["<char>"],
    ),
    (
        "search-backspace",
        "Remove last character from the search query.",
        search_backspace,
        &["<BS>"],
    ),
];

/// The modes, their keys in normal state, and the commands.
pub(crate) fn register(reg: &mut Registry) {
    let name = |n: &str| format!("orchid-session-browser-{n}");
    for (n, doc, f, _) in COMMANDS.iter().chain(SEARCH_COMMANDS) {
        reg.command(&name(n), doc, *f);
    }
    let mode = reg.mode(MODE, Browser::default);
    for (n, _, _, keys) in COMMANDS {
        for k in *keys {
            mode.bind(State::Normal, k, &name(n));
        }
    }
    let search = reg.mode(SEARCH_MODE, || Search);
    for (n, _, _, keys) in SEARCH_COMMANDS {
        for k in *keys {
            search.bind(State::Normal, k, &name(n));
        }
    }
    search.bind(State::Normal, "<C-g>", &name("search-clear"));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::width;
    use crate::store::Status;
    use jiff::ToSpan;

    fn meta(id: &str) -> Metadata {
        let text = format!(r#"{{"id":"{id}","status":"idle"}}"#);
        Metadata::parse(&text).unwrap()
    }

    fn row(label: &str) -> Row {
        Row {
            id: "s1".into(),
            label: label.into(),
            running: false,
            active: false,
            updated_at: None,
        }
    }

    #[test]
    fn labels_fall_back_to_agent_and_workspace_then_the_id() {
        let mut m = meta("0f1e2d3c");
        assert_eq!(label(&m), "0f1e2d3c");
        m.working_dir = Some("/home/user/projects/my-app/".into());
        assert_eq!(label(&m), "default-my-app");
        m.agent_name = Some("developer".into());
        assert_eq!(label(&m), "developer-my-app");
        m.label = Some("mine".into());
        assert_eq!(label(&m), "mine");
        assert_eq!(label(&meta("")), "unknown");
    }

    #[test]
    fn statuses_say_active_idle_and_run() {
        assert_eq!(status_string(true, true), "ACTIVE•RUN");
        assert_eq!(status_string(true, false), "ACTIVE");
        assert_eq!(status_string(false, true), "IDLE•RUN");
        assert_eq!(status_string(false, false), "IDLE");
        let mut m = meta("s1");
        m.status = Status::Running;
        assert!(Row::of(&m, false).running);
    }

    #[test]
    fn the_search_is_fuzzy_and_ignores_case() {
        assert!(fuzzy_match("dmp", "default-my-app"));
        assert!(fuzzy_match("DMA", "default-my-app"));
        assert!(!fuzzy_match("pmd", "default-my-app"));
        assert!(fuzzy_match("", "anything"));
        assert!(!fuzzy_match("x", ""));
        let mut r = row("alpha");
        r.running = true;
        r.updated_at = Some("2026-07-22T01:16:46Z".parse().unwrap());
        assert!(r.matches("arun"), "the status counts");
        assert!(r.matches("a2026"), "and the raw time");
        assert!(!r.matches("active"));
    }

    #[test]
    fn a_long_label_is_cut_with_two_dots_in_its_30_columns() {
        let now = Timestamp::now();
        let line = entry_line(&row(&"x".repeat(40)), None, now);
        assert!(line.text.starts_with(&format!("{}..", "x".repeat(28))));
        assert_eq!(width(&line.text), 46);
        assert_eq!(line.text, format!("{}..  IDLE       N/A", "x".repeat(28)));
        assert_eq!(
            line.spans,
            [(32..36, face::BROWSER_IDLE), (43..46, "shadow")]
        );
    }

    #[test]
    fn marks_and_chats_change_the_row_and_its_faces() {
        let now = Timestamp::now();
        let mut r = row("a");
        r.updated_at = Some(now - 5.minutes());
        r.active = true;
        r.running = true;
        let line = entry_line(&r, None, now);
        assert_eq!(
            line.text,
            format!("a{}ACTIVE•RUN 5 minutes ago", " ".repeat(31))
        );
        assert_eq!(line.spans[0].1, face::BROWSER_ACTIVE);
        let deleting = entry_line(&r, Some(Mark::Delete), now);
        assert_eq!(
            deleting.text,
            format!("a{}DEL{}5 minutes ago", " ".repeat(31), " ".repeat(8))
        );
        let faces: Vec<&str> = deleting.spans.iter().map(|s| s.1).collect();
        assert_eq!(faces, ["error", "error", "error"]);
        let killing = entry_line(&r, Some(Mark::Kill), now);
        let faces: Vec<&str> = killing.spans.iter().map(|s| s.1).collect();
        assert_eq!(faces, ["warning", "warning", "warning"]);
        let mut old = row("b");
        old.updated_at = Some(now - 400.hours());
        assert!(entry_line(&old, None, now).text.ends_with(" 16 days ago"));
    }

    #[test]
    fn rows_go_newest_first_and_untimed_last() {
        let at = |s: &str| Some(s.parse::<Timestamp>().unwrap());
        let mut rs = [row("none"), row("old"), row("new"), row("tie")];
        rs[0].id = "a".into();
        (rs[1].id, rs[1].updated_at) = ("b".into(), at("2026-07-01T00:00:00Z"));
        (rs[2].id, rs[2].updated_at) = ("d".into(), at("2026-07-02T00:00:00Z"));
        (rs[3].id, rs[3].updated_at) = ("c".into(), at("2026-07-02T00:00:00Z"));
        rs.sort_by(newest_first);
        let labels: Vec<&str> = rs.iter().map(|r| r.label.as_str()).collect();
        assert_eq!(labels, ["tie", "new", "old", "none"]);
    }

    #[test]
    fn no_line_is_wider_than_78_columns() {
        let now = Timestamp::now();
        let rows = [row(&"x".repeat(120))];
        let wide = "界".repeat(60);
        let columns = columns();
        let view = View {
            title: TITLE,
            columns: &columns,
            search: &wide,
            searching: true,
            ..View::default()
        };
        let lines = seek::render(&view, &rows, 120, 24, |r, _, _| entry_line(r, None, now));
        let cut: Vec<Line> = lines
            .into_iter()
            .map(|l| l.truncated(MAX_ROW_WIDTH))
            .collect();
        for l in &cut {
            assert!(width(&l.text) <= 78, "{}", l.text);
        }
        assert_eq!(cut[2].text, "界".repeat(39), "the query, cut");
        assert_eq!(columns.len(), 50);
    }

    #[test]
    fn what_a_restart_keeps_comes_back() {
        let mut b = Browser {
            query: "dm".into(),
            searching: true,
            selected: 3,
            scroll: 1,
            ..Browser::default()
        };
        b.set_mark("s1".into(), Mark::Delete);
        b.set_mark("s2".into(), Mark::Kill);
        b.set_mark("s1".into(), Mark::Kill);
        let kept = b.kept();
        assert_eq!(
            kept,
            json!({"query": "dm", "searching": true, "selected": 3, "scroll": 1,
                   "marks": [["s1", "kill"], ["s2", "kill"]]})
        );
        let mut again = Browser::default();
        again.take(&kept);
        assert_eq!(again.kept(), kept);
        again.take(&json!({}));
        assert_eq!(again.kept(), Browser::default().kept());
    }
}
