//! die-orchid-client: your emacs-orchid-client, ported to DIE as an extension (scope D14). It
//! drives orchid 0.2.2, your agent CLI, as Emacs did: a session browser, `*orchid-sessions*`, and
//! a chat buffer per session, `*orchid-chat-<agent>-<wsdir>-<id5>*`, that follows the session's
//! `events.jsonl` and sends with `orchid send`.
//!
//! It depends on die-core alone, so it's platform-neutral and out of the product: `cargo xtask
//! lint` holds it to that, and it moves to its own repository at M4. Its parts:
//!
//! - this file: [`Orchid`], the extension, with your [`Options`] (C7), the session registry and its
//!   status news, what a restart keeps, and the `orchid-*` commands and faces;
//! - [`cli`]: running orchid, through a [`cli::Runner`] a test can fake;
//! - [`store`]: orchid's files, read: `metadata.json`, `events.jsonl`, the restore and the tail;
//! - [`parse`]: events as the chat shows them, with tokens and local times;
//! - [`poll`]: the one timer every orchid buffer shares;
//! - [`log`]: `*Orchid Log*`;
//! - [`browser`] and [`chat`]: the two kinds of buffer, the browser drawn by [`seek`], your list
//!   browser, and a chat's collapsible [`sections`] and its processing [`indicator`];
//! - [`fixtures`]: sessions for tests, by hand.
//!
//! Nothing here reads or runs anything at registration or init. The config root is worked out
//! the first time it's wanted, as orchid works it out: `$ORCHID_CONFIG`, then
//! `$HOME/.config/orchid`.
#![warn(missing_docs)]

#[cfg(test)]
use die_test_cap as _;

pub mod browser;
pub mod chat;
pub mod cli;
pub mod fixtures;
pub mod indicator;
pub mod log;
pub mod parse;
pub mod poll;
pub mod sections;
pub mod seek;
pub mod store;

use cli::{Cli, Cmd, FakeRunner, ProcessRunner, Runner};
use die_core::session::{Kept, Value, json};
use die_core::{
    Args, BufferId, Ctx, Error, Extension, PromptSpec, Registry, Result, ShowIn, Side, Style, Sub,
};
use jiff::tz::{Offset, TimeZone};
use std::cell::{OnceCell, RefCell};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::rc::{Rc, Weak};
use std::sync::Arc;
use std::time::Duration;
use store::{Metadata, Status};

// ------------------------------------------------------------------ options

/// Your options (C7), as the Emacs client's defcustoms were.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Options {
    /// The `orchid` program: a name looked up on your login shell's PATH, or a path
    /// (`orchid-core-cli-path`).
    pub program: PathBuf,
    /// The config root, or None for orchid's own rule: `$ORCHID_CONFIG`, then
    /// `$HOME/.config/orchid` (`orchid-core-config-dir`). Every command gets it as `--config`.
    pub root: Option<PathBuf>,
    /// How many events a chat restores when it opens (`orchid-log-restore-max-events`).
    pub restore_max_events: usize,
    /// How much of the end of `events.jsonl` a chat reads to restore them
    /// (`orchid-log-restore-max-size-mb`).
    pub restore_max_bytes: u64,
    /// How long one orchid command may run before it's stopped (`orchid-core-default-timeout`).
    pub timeout: Duration,
    /// How many lines `*Orchid Log*` keeps.
    pub log_lines: usize,
}

impl Default for Options {
    /// The Emacs client's defaults: `orchid`, orchid's root, 250 events from the last 5 MB, and
    /// 300 s; and the last 2,000 lines of the log.
    fn default() -> Options {
        Options {
            program: "orchid".into(),
            root: None,
            restore_max_events: 250,
            restore_max_bytes: 5 * 1024 * 1024,
            timeout: Duration::from_secs(300),
            log_lines: 2000,
        }
    }
}

/// orchid's root without `--config` (its `store::default_root`): `$ORCHID_CONFIG`, then
/// `$HOME/.config/orchid`, then `.orchid-simplified`, made absolute as orchid makes it.
pub fn default_root() -> PathBuf {
    let root = match (std::env::var_os("ORCHID_CONFIG"), std::env::var_os("HOME")) {
        (Some(root), _) => PathBuf::from(root),
        (None, Some(home)) => Path::new(&home).join(".config").join("orchid"),
        (None, None) => ".orchid-simplified".into(),
    };
    std::path::absolute(&root).unwrap_or(root)
}

/// The zone tests show times in: UTC+10, Sydney's in July, as the Emacs client's tests had it.
pub fn test_zone() -> TimeZone {
    TimeZone::fixed(Offset::constant(10))
}

// ------------------------------------------------------------------ faces

/// The faces orchid uses, by name. Each is declared with a parent, so a theme that styles only
/// the base faces still draws it as your Emacs did.
pub mod face {
    pub use crate::parse::face::*;
    /// The separators (`orchid-chat-separator-face`).
    pub const SEPARATOR: &str = "orchid-chat-separator";
    /// The header's workspace (`font-lock-string-face`).
    pub const HEADER_WORKSPACE: &str = "orchid-chat-header-workspace";
    /// The header's agent (`font-lock-keyword-face`).
    pub const HEADER_AGENT: &str = "orchid-chat-header-agent";
    /// The browser's STATUS for a session with a chat open (`orchid-session-browser-active`).
    pub const BROWSER_ACTIVE: &str = "orchid-session-browser-active";
    /// The browser's STATUS otherwise (`orchid-session-browser-idle`).
    pub const BROWSER_IDLE: &str = "orchid-session-browser-idle";
}

/// Every face orchid declares: its name, its parent, and bold on, off, or as the parent has it.
/// Emacs's six faces that nothing applied aren't here.
pub const FACES: &[(&str, &str, Option<bool>)] = &[
    (face::SEPARATOR, "shadow", None),
    (face::ASSISTANT, "shadow", Some(true)),
    (face::HEADER_WORKSPACE, "default", None),
    (face::HEADER_AGENT, "default", Some(true)),
    (face::STUB, "shadow", None),
    (face::USER_STUB, "default", None),
    (face::FAILURE_STUB, "warning", None),
    (face::DIFF_REMOVED, "warning", None),
    (face::DIFF_ADDED, "success", Some(false)),
    (face::DIFF_CONTEXT, "shadow", None),
    (face::BROWSER_ACTIVE, "success", Some(true)),
    (face::BROWSER_IDLE, "shadow", None),
    (seek::face::SEARCH, "highlight", None),
];

// ------------------------------------------------------------------ the registry's news

/// What the client knows of a session: its `metadata.json`, as last read.
pub type SessionInfo = Metadata;

/// A session's status changed, or the session came or went (None).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StatusChange {
    /// The session.
    pub id: String,
    /// What it was, or None if it's new to the registry.
    pub was: Option<Status>,
    /// What it is, or None if it's gone.
    pub now: Option<Status>,
}

type WatchFn = RefCell<Box<dyn FnMut(&mut Ctx, &StatusChange) -> Result>>;

/// A subscription to status changes ([`Orchid::on_status_change`]). Dropping it unsubscribes,
/// so one kept in a mode goes with its buffer (C4).
#[must_use = "dropping a StatusWatch unsubscribes it"]
pub struct StatusWatch(
    // Held, never read: the watchers' list has only a weak reference.
    #[allow(dead_code)] Rc<WatchFn>,
);

// ------------------------------------------------------------------ what a restart keeps

/// Which kind of orchid buffer a buffer is, so a restart brings it back as one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// `*orchid-sessions*`.
    Browser,
    /// A chat.
    Chat,
}

impl Kind {
    fn name(self) -> &'static str {
        match self {
            Kind::Browser => "browser",
            Kind::Chat => "chat",
        }
    }
}

/// What a restart keeps of a tracked buffer, worked out when the session looks: from the
/// buffer, its tracked state, and what the closure holds (a chat's input marker). None keeps
/// nothing this time. It gets `&Ctx` alone, and runs about twice a second, so it's cheap.
pub type SaveFn = Rc<dyn Fn(&Ctx, BufferId, &Value) -> Option<Value>>;

/// A tracked buffer: its kind, what its module needs to bring it back, and how to work out what
/// a restart keeps, if that isn't `state` as it is.
struct Tracked {
    kind: Kind,
    state: Value,
    save: Option<SaveFn>,
}

// ------------------------------------------------------------------ the extension

/// The extension: your options, and the client's global state, which Emacs kept in globals.
pub struct Orchid {
    /// Your options.
    pub options: Options,
    /// The poll every orchid buffer shares.
    pub poll: poll::Poller,
    runner: Arc<dyn Runner>,
    zone: OnceCell<TimeZone>,
    root: OnceCell<PathBuf>,
    sessions: BTreeMap<String, SessionInfo>,
    watchers: Vec<Weak<WatchFn>>,
    changes: Vec<StatusChange>,
    tracked: BTreeMap<BufferId, Tracked>,
}

impl Default for Orchid {
    /// The default options, and orchid as a process.
    fn default() -> Orchid {
        Orchid::new(Options::default())
    }
}

impl Orchid {
    /// The extension with these options, running orchid as a process.
    pub fn new(options: Options) -> Orchid {
        Orchid {
            options,
            poll: poll::Poller::default(),
            runner: Arc::new(ProcessRunner::default()),
            zone: OnceCell::new(),
            root: OnceCell::new(),
            sessions: BTreeMap::new(),
            watchers: vec![],
            changes: vec![],
            tracked: BTreeMap::new(),
        }
    }

    /// For tests: the root is `root`, a temporary directory, orchid is `runner`, and times are
    /// in [`test_zone`]. Nothing reaches your own root or runs your orchid.
    pub fn for_tests(root: &Path, runner: FakeRunner) -> Orchid {
        let options = Options {
            root: Some(root.to_path_buf()),
            ..Options::default()
        };
        Orchid::new(options)
            .with_runner(runner)
            .with_zone(test_zone())
    }

    /// Run orchid with `runner` instead.
    pub fn with_runner(mut self, runner: impl Runner) -> Orchid {
        self.runner = Arc::new(runner);
        self
    }

    /// Show times in `zone` instead of the system's.
    pub fn with_zone(mut self, zone: TimeZone) -> Orchid {
        self.zone = OnceCell::from(zone);
        self
    }

    /// The config root, worked out the first time it's asked for.
    pub fn root(&self) -> &Path {
        self.root
            .get_or_init(|| self.options.root.clone().unwrap_or_else(default_root))
    }

    /// The zone times show in: the system's, read the first time it's asked for.
    pub fn zone(&self) -> &TimeZone {
        self.zone.get_or_init(TimeZone::system)
    }

    /// What it takes to run orchid, to move into a job.
    pub fn cli(&self) -> Cli {
        Cli {
            program: self.options.program.clone(),
            root: self.root().to_path_buf(),
            runner: self.runner.clone(),
            timeout: self.options.timeout,
        }
    }

    // -------------------------------------------------------------- the session registry

    /// Every session the client knows of, by id.
    pub fn sessions(&self) -> &BTreeMap<String, SessionInfo> {
        &self.sessions
    }

    /// A session by id.
    pub fn session(&self, id: &str) -> Option<&SessionInfo> {
        self.sessions.get(id)
    }

    /// A session by id, or else by label: what `orchid-open-session` was given.
    pub fn find_session(&self, id_or_label: &str) -> Option<&SessionInfo> {
        let by_label = || {
            self.sessions
                .values()
                .find(|s| s.label.as_deref() == Some(id_or_label))
        };
        self.session(id_or_label).or_else(by_label)
    }

    /// What a session's `metadata.json` says now. A new status is news for every watcher.
    pub fn update_session(&mut self, ctx: &mut Ctx, info: SessionInfo) {
        let was = self.sessions.get(&info.id).map(|s| s.status);
        if was != Some(info.status) {
            self.news(ctx, info.id.clone(), was, Some(info.status));
        }
        self.sessions.insert(info.id.clone(), info);
    }

    /// A session that's gone, as after `orchid delete`.
    pub fn forget_session(&mut self, ctx: &mut Ctx, id: &str) {
        if let Some(gone) = self.sessions.remove(id) {
            self.news(ctx, gone.id, Some(gone.status), None);
        }
    }

    /// Every session there is now, as a scan found them: the rest are gone.
    pub fn replace_sessions(&mut self, ctx: &mut Ctx, all: Vec<SessionInfo>) {
        let found: std::collections::HashSet<String> = all.iter().map(|s| s.id.clone()).collect();
        let gone: Vec<String> = self
            .sessions
            .keys()
            .filter(|id| !found.contains(*id))
            .cloned()
            .collect();
        for id in gone {
            self.forget_session(ctx, &id);
        }
        for info in all {
            self.update_session(ctx, info);
        }
    }

    /// Run `f` for each status change from now on, from the top of the loop with the extension
    /// put back, as the Emacs client's `orchid-session-status-change-functions` ran.
    pub fn on_status_change(
        &mut self,
        f: impl FnMut(&mut Ctx, &StatusChange) -> Result + 'static,
    ) -> StatusWatch {
        let f: Rc<WatchFn> = Rc::new(RefCell::new(Box::new(f)));
        self.watchers.push(Rc::downgrade(&f));
        StatusWatch(f)
    }

    /// Queue a change for the watchers, who hear of the changes together.
    fn news(&mut self, ctx: &mut Ctx, id: String, was: Option<Status>, now: Option<Status>) {
        if self.changes.is_empty() {
            ctx.defer(deliver_news);
        }
        self.changes.push(StatusChange { id, was, now });
    }

    // -------------------------------------------------------------- buffers

    /// Keep `buf` over a restart as an orchid buffer of `kind`, with `state` as what its module
    /// needs to bring it back (`browser::restore`, `chat::restore`); a chat's has its
    /// session's id under `id`. Track it again to change `state`. A killed buffer is forgotten.
    pub fn track(&mut self, buf: BufferId, kind: Kind, state: Value) {
        let save = None;
        self.tracked.insert(buf, Tracked { kind, state, save });
    }

    /// [`Orchid::track`], keeping what `save` makes of the buffer and `state` when the session
    /// looks, as a chat's unsent input, which `state` can't hold as it changes with each key.
    pub fn track_with(
        &mut self,
        buf: BufferId,
        kind: Kind,
        state: Value,
        save: impl Fn(&Ctx, BufferId, &Value) -> Option<Value> + 'static,
    ) {
        let save: Option<SaveFn> = Some(Rc::new(save));
        self.tracked.insert(buf, Tracked { kind, state, save });
    }

    /// Stop keeping `buf`.
    pub fn untrack(&mut self, buf: BufferId) {
        self.tracked.remove(&buf);
    }

    /// What `buf` was tracked as, if it was.
    pub fn tracked(&self, buf: BufferId) -> Option<(Kind, &Value)> {
        self.tracked.get(&buf).map(|t| (t.kind, &t.state))
    }

    /// The live buffers tracked as `kind`, the oldest first.
    pub fn buffers(&self, ctx: &Ctx, kind: Kind) -> Vec<BufferId> {
        let of = self
            .tracked
            .iter()
            .filter(|(b, t)| t.kind == kind && ctx.is_live(**b));
        of.map(|(b, _)| *b).collect()
    }

    /// The chat buffer open on `session_id`, if there is one: what the browser's ACTIVE says.
    pub fn chat_buffer(&self, ctx: &Ctx, session_id: &str) -> Option<BufferId> {
        let on = |b: &BufferId| {
            self.tracked[b].state.get("id").and_then(Value::as_str) == Some(session_id)
        };
        self.buffers(ctx, Kind::Chat).into_iter().find(on)
    }

    /// A buffer was killed: forget it, and stop the poll if it was the last to want it. A chat
    /// takes its session's events' log with it, as Emacs's stopped its monitor.
    fn killed(&mut self, ctx: &mut Ctx, buf: BufferId) {
        let gone = self.tracked.remove(&buf);
        let id = gone.and_then(|t| Some(t.state.get("id")?.as_str()?.to_string()));
        if let Some(id) = id {
            ctx.defer(move |ctx| {
                if let Some(log) = ctx.find_buffer(&chat::events_buffer(&id)) {
                    ctx.kill(log);
                }
                Ok(())
            });
        }
        self.poll.settle(ctx);
    }
}

/// Hand the queued status changes to every watcher, with the extension put back.
fn deliver_news(ctx: &mut Ctx) -> Result {
    let (changes, watchers) = ctx.with_ext(|o: &mut Orchid, _| {
        o.watchers.retain(|w| w.strong_count() > 0);
        let live: Vec<Rc<WatchFn>> = o.watchers.iter().filter_map(Weak::upgrade).collect();
        (std::mem::take(&mut o.changes), live)
    })?;
    let mut failed = None;
    for w in watchers {
        let Ok(mut f) = w.try_borrow_mut() else {
            continue;
        };
        for change in &changes {
            if let Err(why) = f(ctx, change) {
                failed.get_or_insert(why);
            }
        }
    }
    failed.map_or(Ok(()), Err)
}

/// [`Orchid::cli`], from outside the extension.
pub fn cli(ctx: &mut Ctx) -> Result<Cli> {
    Ok(ctx.with_ext(|o: &mut Orchid, _| o.cli())?)
}

/// Read every session's `metadata.json` in a job, put them in the registry, then `done`.
/// What changed status is news for the watchers before `done` runs.
pub fn refresh_sessions(
    ctx: &mut Ctx,
    done: impl FnOnce(&mut Ctx) -> Result + 'static,
) -> Result<Sub> {
    let root = ctx.with_ext(|o: &mut Orchid, _| o.root().to_path_buf())?;
    let finish = move |ctx: &mut Ctx, all: Vec<SessionInfo>| {
        ctx.with_ext(|o: &mut Orchid, ctx| o.replace_sessions(ctx, all))?;
        ctx.defer(done);
        Ok(())
    };
    Ok(ctx.spawn(move || store::scan(&root), finish))
}

/// Ask for an agent, as the Emacs client's `Agent: ` did: `orchid agent`'s, those it says are
/// valid (the Emacs client offered broken ones too, A12), one of them required. Then `done`
/// with it. With none, it says so, and asks nothing. A prompt you opened while orchid answered
/// stays yours, and `Agent: ` comes when it closes: Emacs asked orchid while it waited, so it
/// never took a prompt from you.
pub fn read_agent(
    ctx: &mut Ctx,
    done: impl FnOnce(&mut Ctx, String) -> Result + 'static,
) -> Result {
    let answered = move |ctx: &mut Ctx, out: cli::Outcome| {
        let out = out.map_err(|e| Error::Msg(format!("orchid agent: {e}")))?;
        let agents = out
            .get("agents")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let valid = agents
            .iter()
            .filter(|a| a.get("valid").and_then(Value::as_bool) != Some(false));
        let names: Vec<String> = valid
            .filter_map(|a| a.get("name")?.as_str().map(String::from))
            .collect();
        if names.is_empty() {
            return Err(Error::Msg(
                "No agents available; check `orchid agent' and your CLI configuration".into(),
            ));
        }
        ask_agent(ctx, names, Box::new(done));
        Ok(())
    };
    cli(ctx)?.run(ctx, Cmd::Agent, None, answered).detach();
    Ok(())
}

/// How often `Agent: ` looks again for the prompt that was open when orchid answered to close.
const PROMPT_WAIT: Duration = Duration::from_millis(200);

/// What an agent's answer does.
type AgentChosen = Box<dyn FnOnce(&mut Ctx, String) -> Result>;

/// `Agent: ` among `names`, once no other prompt is open.
fn ask_agent(ctx: &mut Ctx, names: Vec<String>, done: AgentChosen) {
    if ctx.prompt_text().is_some() {
        let again = move |ctx: &mut Ctx| {
            ask_agent(ctx, names, done);
            Ok(())
        };
        ctx.after(PROMPT_WAIT, again).detach();
        return;
    }
    let spec = PromptSpec::new("Agent: ").candidates(names).require_match();
    ctx.prompt(spec, |ctx, agent| match agent.filter(|a| !a.is_empty()) {
        Some(agent) => done(ctx, agent),
        None => Ok(()),
    });
}

/// Ask `question` as Emacs's `yes-or-no-p` does: `yes` or `no` typed out, in any case but with
/// nothing around it, never a bare `<CR>`, and again until it's one of them. Then `done` with the
/// answer. A cancel answers nothing.
pub fn yes_or_no(
    ctx: &mut Ctx,
    question: &str,
    done: impl FnOnce(&mut Ctx, bool) -> Result + 'static,
) {
    ask_yes_or_no(ctx, question.to_string(), Box::new(done));
}

/// What a yes-or-no question does with its answer.
type Answered = Box<dyn FnOnce(&mut Ctx, bool) -> Result>;

fn ask_yes_or_no(ctx: &mut Ctx, question: String, done: Answered) {
    let label = format!("{question}(yes or no) ");
    ctx.prompt(label.as_str(), move |ctx, answer| {
        match answer.map(|a| a.to_lowercase()).as_deref() {
            None => Ok(()),
            Some("yes") => done(ctx, true),
            Some("no") => done(ctx, false),
            Some(_) => {
                ctx.messages.push("Please answer yes or no.".into());
                ask_yes_or_no(ctx, question, done);
                Ok(())
            }
        }
    });
}

/// Show `buf` as Emacs's `display-buffer` does, for what the Emacs client showed that way
/// (`orchid-log-show`): a window showing it already will do; else the window used last that
/// isn't this one and shows text, or else a split below this one. The focus stays. Everything
/// else the client shows goes in the selected window, as its `switch-to-buffer` did.
pub fn display_buffer(ctx: &mut Ctx, buf: BufferId) -> Result {
    if ctx
        .window_list()
        .iter()
        .any(|w| ctx.window_buffer(*w) == Some(buf))
    {
        return Ok(());
    }
    let here = ctx.focused();
    let shows_text = |ctx: &Ctx, w| ctx.window_buffer(w).is_some_and(|b| ctx.text(b).is_ok());
    let by_use = ctx.windows_by_use();
    match by_use
        .into_iter()
        .find(|w| Some(*w) != here && shows_text(ctx, *w))
    {
        Some(w) => {
            ctx.focus(w);
            ctx.show(buf, ShowIn::Current)?;
        }
        None => {
            let split = ctx.show(buf, ShowIn::Split(Side::Below));
            split.or_else(|_| ctx.show(buf, ShowIn::Current))?;
        }
    }
    if let Some(w) = here.filter(|w| ctx.window_list().contains(w)) {
        ctx.focus(w);
    }
    Ok(())
}

/// The directory the command was given, Emacs's `default-directory`: its buffer's, or the
/// focused one's.
fn here(ctx: &Ctx, args: &Args) -> PathBuf {
    let buf = args.buffer.or_else(|| ctx.focused_buffer());
    let dir = buf.and_then(|b| ctx.directory(b)).map(Path::to_path_buf);
    dir.unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| "/".into()))
}

// ------------------------------------------------------------------ commands

/// `orchid-open-session`: by id or label, asked for unless given (`:orchid-open-session ID`),
/// from the sessions there are now, and then says so, as Emacs's did. The Emacs client lost the
/// id it found by label (B1).
fn open_session(ctx: &mut Ctx, args: &Args) -> Result {
    let open = |ctx: &mut Ctx, wanted: String| -> Result {
        ctx.messages.push("Refreshing sessions...".into());
        refresh_sessions(ctx, move |ctx| {
            let found =
                ctx.with_ext(|o: &mut Orchid, _| o.find_session(&wanted).map(|s| s.id.clone()))?;
            let said = match found {
                Some(id) => {
                    chat::open(ctx, &id)?;
                    format!("Opened session: {wanted}")
                }
                None => format!("Failed to open session: Session '{wanted}' not found"),
            };
            ctx.messages.push(said);
            Ok(())
        })?
        .detach();
        Ok(())
    };
    match args
        .text
        .as_deref()
        .map(str::trim)
        .filter(|t| !t.is_empty())
    {
        Some(wanted) => open(ctx, wanted.to_string()),
        None => {
            ctx.prompt("Session ID or label: ", move |ctx, answer| match answer {
                Some(a) if !a.trim().is_empty() => open(ctx, a.trim().to_string()),
                _ => Ok(()),
            });
            Ok(())
        }
    }
}

/// `orchid-log-show`: `Session ID: `, unless given (`:orchid-log-show ID`), then that session's
/// events as its chat has read them, as Emacs's showed its monitor's copy of them.
fn log_show(ctx: &mut Ctx, args: &Args) -> Result {
    match args.text.as_deref().map(str::trim) {
        Some(id) if !id.is_empty() => chat::show_events(ctx, id),
        _ => {
            ctx.prompt("Session ID: ", |ctx, id| match id {
                Some(id) => chat::show_events(ctx, &id),
                None => Ok(()),
            });
            Ok(())
        }
    }
}

/// `orchid-new-session`: an agent, then a new chat in the buffer's directory.
fn new_session(ctx: &mut Ctx, args: &Args) -> Result {
    let dir = here(ctx, args);
    read_agent(ctx, move |ctx, agent| chat::new_session(ctx, &agent, &dir))
}

/// The buffer `orchid-list-sessions` lists them in, since the command row shows one line.
pub const LIST_BUFFER: &str = "*orchid-list-sessions*";

/// `orchid-list-sessions`: `  label (id)` for each, oldest first as `orchid list` has them, in
/// [`LIST_BUFFER`] where `display-buffer` would show it; or `No sessions found`.
fn list_sessions(ctx: &mut Ctx, _: &Args) -> Result {
    ctx.messages.push("Refreshing sessions...".into());
    refresh_sessions(ctx, |ctx| {
        let (lines, root) = ctx.with_ext(|o: &mut Orchid, _| {
            let mut all: Vec<&SessionInfo> = o.sessions.values().collect();
            all.sort_by_key(|s| s.created_at);
            let line =
                |s: &&SessionInfo| format!("  {} ({})", s.label.as_deref().unwrap_or(""), s.id);
            (
                all.iter().map(line).collect::<Vec<_>>(),
                o.root().to_path_buf(),
            )
        })?;
        if lines.is_empty() {
            ctx.messages.push("No sessions found".into());
            return Ok(());
        }
        let text = format!("Available sessions:\n{}\n", lines.join("\n"));
        let buf = ctx.output_buffer(LIST_BUFFER, &text, &root)?;
        display_buffer(ctx, buf)?;
        ctx.messages
            .push(format!("{} sessions, in {LIST_BUFFER}", lines.len()));
        Ok(())
    })?
    .detach();
    Ok(())
}

/// `orchid-check-cli`: whether orchid is on the PATH, and its version, which should be 0.2 or
/// later.
fn check_cli(ctx: &mut Ctx, _: &Args) -> Result {
    let cli = cli(ctx)?;
    let work = move || cli.find().map(|_| cli.run_now(&Cmd::Version, None));
    let done = |ctx: &mut Ctx, found: Option<cli::Outcome>| {
        let said = match found {
            None => "orchid CLI not found in PATH. Please install it.".to_string(),
            Some(Err(_)) => "orchid CLI found but version check failed".to_string(),
            Some(Ok(v)) => {
                let raw = v.as_str().map_or_else(|| v.to_string(), String::from);
                match version(&raw) {
                    Some((0, 0 | 1)) => format!(
                        "orchid CLI found: {raw}, but this client needs orchid 0.2 or later"
                    ),
                    _ => format!("orchid CLI found: {raw}"),
                }
            }
        };
        ctx.messages.push(said);
        Ok(())
    };
    ctx.spawn(work, done).detach();
    Ok(())
}

/// `orchid X.Y.Z (…)`'s major and minor version.
fn version(said: &str) -> Option<(u32, u32)> {
    let v = said.split_whitespace().nth(1)?;
    let mut parts = v.split('.').map(|p| p.parse::<u32>().ok());
    Some((parts.next()??, parts.next()??))
}

impl Extension for Orchid {
    fn name(&self) -> &'static str {
        "orchid"
    }

    fn register(&mut self, reg: &mut Registry) {
        for (name, parent, bold) in FACES {
            let style = bold.map_or_else(Style::new, |b| Style::new().bold_is(b));
            reg.face(name, Some(parent), style);
        }
        reg.command("orchid", "Open the orchid session browser.", |ctx, _| {
            browser::show(ctx)
        });
        reg.command(
            "orchid-session-browser-show",
            "Show the orchid session browser, as orchid does.",
            |ctx, _| browser::show(ctx),
        );
        reg.command(
            "orchid-open-session",
            "Open a chat on an orchid session, by its id or label.",
            open_session,
        );
        reg.command(
            "orchid-new-session",
            "Start a chat on a new orchid session, with an agent you choose, in this directory.",
            new_session,
        );
        reg.command(
            "orchid-list-sessions",
            "List orchid's sessions.",
            list_sessions,
        );
        reg.command(
            "orchid-check-cli",
            "Say whether orchid is on the PATH, and its version.",
            check_cli,
        );
        reg.command(
            "orchid-log-show",
            "Show the events of a session a chat follows, as it has read them.",
            log_show,
        );
        browser::register(reg);
        chat::register(reg);
    }

    fn init(&mut self, ctx: &mut Ctx) -> Result {
        let killed =
            |ctx: &mut Ctx, buf| Ok(ctx.with_ext(|o: &mut Orchid, ctx| o.killed(ctx, buf))?);
        ctx.on_buffer_killed(killed).detach();
        Ok(())
    }

    fn save_session(&self, ctx: &Ctx) -> Kept {
        let keep = |(buf, t): (&BufferId, &Tracked)| {
            let state = match &t.save {
                Some(save) => save(ctx, *buf, &t.state)?,
                None => t.state.clone(),
            };
            Some((*buf, json!({"kind": t.kind.name(), "state": state})))
        };
        let live = self.tracked.iter().filter(|(b, _)| ctx.is_live(**b));
        Kept {
            state: Value::Null,
            buffers: live.filter_map(keep).collect(),
        }
    }

    fn restore_session(&mut self, ctx: &mut Ctx, kept: Kept) -> Result {
        for (buf, saved) in kept.buffers {
            let state = saved.get("state").cloned().unwrap_or(Value::Null);
            let restored = match saved.get("kind").and_then(Value::as_str) {
                Some("browser") => browser::restore(self, ctx, buf, state),
                Some("chat") => chat::restore(self, ctx, buf, state),
                _ => Ok(()),
            };
            if let Err(e) = restored {
                ctx.messages
                    .push(format!("orchid: a buffer didn't come back: {e}"));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions() {
        assert_eq!(
            version("orchid 0.2.2 (codex compatibility 7)"),
            Some((0, 2))
        );
        assert_eq!(version("orchid 0.1.9"), Some((0, 1)));
        assert_eq!(version("orchid"), None);
        assert_eq!(version("orchid x.y"), None);
    }

    #[test]
    fn the_defaults_are_the_emacs_clients() {
        let o = Options::default();
        assert_eq!(o.program, Path::new("orchid"));
        assert_eq!((o.restore_max_events, o.restore_max_bytes), (250, 5 << 20));
        assert_eq!((o.timeout, o.log_lines), (Duration::from_secs(300), 2000));
        assert_eq!(o.root, None);
    }
}
