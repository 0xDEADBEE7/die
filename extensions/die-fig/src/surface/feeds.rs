//! Live sources in the editor (contract-p4 §5, "Live"; ruling 13): a figure's commands (`run`,
//! `stream`) and a dashboard's `file` panels outside its spec's directory asked for by name before
//! they run or open (`Run "./errors.sh" every 30s for health.fig? (y/n)`), one question at a time
//! through the prompt, never over one you're typing in; a yes kept in what the session keeps, a no
//! for this session, shown on the panel (or the chart) as `not run: you said no` until `R` asks
//! again. What's yes runs and streams in [`crate::live::Live`] while a window shows the figure,
//! polled on a timer only while a run is due or a stream is open (none at rest); what a command
//! prints fills the chart's table or the dashboard's source it's for, and each that lands builds
//! the figure again, a following view sliding to the newest. When no window shows the figure, its
//! commands stop.

use super::{FigState, Pane};
use crate::dashboard::{LiveStatus, NeedsTrust, SourceState, Want};
use crate::data::Table;
use crate::error::FigError;
use crate::live::{Answer, Event, Live, LiveKey, LiveSpec, PathFrom, Rerun, Status, not_run};
use crate::source::{Failed, Source};
use die_core::{Ctx, PromptSpec};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

/// A command the figure wants: its key, how it's run and read, the question it needs, and the
/// dashboard's source it fills (None for a chart's table).
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Wanted {
    key: LiveKey,
    spec: LiveSpec,
    ask: NeedsTrust,
    want: Option<Want>,
}

/// The spec a command of a figure is named by: its file, where its text is a file's.
fn spec_file(st: &FigState) -> Option<PathBuf> {
    match &st.source {
        Source::Buffer(_) | Source::File(_) => st
            .file
            .clone()
            .map(|f| std::path::absolute(&f).unwrap_or(f)),
        _ => None,
    }
}

impl FigState {
    /// The commands the figure wants now: a dashboard's, as its model asks for them; a chart's,
    /// as its build found them.
    pub(super) fn wanted(&self) -> Vec<Wanted> {
        let mut out: Vec<Wanted> = vec![];
        if let Some(d) = &self.dash {
            let Some(m) = d.model.as_ref() else {
                return out;
            };
            for ask in m.trust() {
                let NeedsTrust::Command { spec, cmd, .. } = ask else {
                    continue;
                };
                let dir = spec
                    .as_deref()
                    .and_then(Path::parent)
                    .map_or_else(|| d.dir.clone(), Path::to_path_buf);
                let Some(want) =
                    (m.wants().iter()).find(|w| w.is_command() && w.name() == *cmd && w.dir == dir)
                else {
                    continue;
                };
                let Some(live) = LiveSpec::of(&want.data, &want.dir) else {
                    continue;
                };
                let key = LiveKey::new(spec.as_deref(), cmd.as_str());
                if !out.iter().any(|w| w.key == key) {
                    out.push(Wanted {
                        key,
                        spec: live,
                        ask: ask.clone(),
                        want: Some(want.clone()),
                    });
                }
            }
            return out;
        }
        let file = spec_file(self);
        for f in &self.chart_feeds {
            let key = LiveKey::new(file.as_deref(), f.spec.cmd());
            if out.iter().any(|w| w.key == key) {
                continue;
            }
            out.push(Wanted {
                key,
                ask: NeedsTrust::Command {
                    spec: file.clone(),
                    cmd: f.spec.cmd().to_string(),
                    every: f.spec.every(),
                    stream: f.spec.is_stream(),
                },
                spec: f.spec.clone(),
                want: None,
            });
        }
        out
    }

    /// The file panels the dashboard asks your yes to open.
    fn wanted_files(&self) -> Vec<NeedsTrust> {
        let Some(m) = self.dash.as_ref().and_then(|d| d.model.as_ref()) else {
            return vec![];
        };
        (m.trust().iter())
            .filter(|t| matches!(t, NeedsTrust::File { .. }))
            .cloned()
            .collect()
    }

    /// Ask for, start and stop the figure's commands, as it wants them now and as you've
    /// answered: each yes runs (only while a window shows the figure), each no says `not run`
    /// where its data would be, and the first not yet answered is asked (one question at a time,
    /// never over a prompt you're in). The timer polls only while something's due.
    pub(super) fn live_sync(&mut self, ctx: &mut Ctx) {
        let shown = self.shown(ctx);
        let wanted = self.wanted();
        let files = self.wanted_files();
        if !shown || (wanted.is_empty() && files.is_empty()) {
            if let Some(mut l) = self.live.take() {
                let _ = l.stop_all();
            }
            self.live_timer = None;
            return;
        }
        let mut yes = vec![];
        let mut ask: Option<NeedsTrust> = None;
        let mut noes = vec![];
        let mut opened: Vec<PathBuf> = vec![];
        {
            let trust = self.trust.borrow();
            for w in &wanted {
                let NeedsTrust::Command { spec, cmd, .. } = &w.ask else {
                    continue;
                };
                match trust.command(spec.as_deref(), cmd) {
                    Answer::Yes => yes.push((w.key.clone(), w.spec.clone())),
                    Answer::No => noes.push(w.clone()),
                    Answer::Ask => {
                        ask.get_or_insert_with(|| w.ask.clone());
                    }
                }
            }
            for f in &files {
                let NeedsTrust::File { spec, path, .. } = f else {
                    continue;
                };
                let answer = match spec {
                    Some(s) => trust.file(s, path),
                    None if self.files_said.iter().any(|(p, y)| p == path && *y) => Answer::Yes,
                    None if self.files_said.iter().any(|(p, _)| p == path) => Answer::No,
                    None => Answer::Ask,
                };
                match answer {
                    Answer::Yes => opened.push(path.clone()),
                    Answer::No => {}
                    Answer::Ask => {
                        ask.get_or_insert_with(|| f.clone());
                    }
                }
            }
        }
        for path in opened {
            self.file_trusted(ctx, &path);
        }
        for w in noes {
            self.not_run(ctx, &w);
        }
        let now = ctx.now();
        let zone = self.zone.clone();
        let live = self
            .live
            .get_or_insert_with(|| Live::new(zone, PathFrom::Login));
        live.set(&yes, now);
        if let Some(ask) = ask
            && !self.asking
            && ctx.prompt_text().is_none()
        {
            self.ask(ctx, ask);
        }
        self.arm_live(ctx);
    }

    /// A `file` panel you said yes to: the dashboard may open it, and is planned again if it
    /// hadn't.
    fn file_trusted(&mut self, ctx: &mut Ctx, path: &Path) {
        let Some(d) = self.dash.as_mut() else { return };
        if d.sources.trusted(path) {
            return;
        }
        d.sources.trust_file(path);
        self.dash_rebuild(ctx);
    }

    /// Ask the question a command or a file panel needs, through the prompt: `y` (or `yes`) is
    /// yes; anything else, or `Esc`, is no for this session.
    fn ask(&mut self, ctx: &mut Ctx, ask: NeedsTrust) {
        self.asking = true;
        let label = format!("{} ", ask.question(&self.name));
        let me = self.me.clone();
        ctx.prompt(PromptSpec::new(&label), move |ctx, answer| {
            let Some(s) = me.upgrade() else {
                return Ok(());
            };
            let mut st = s.borrow_mut();
            st.asking = false;
            let yes = matches!(
                answer.as_deref().map(str::trim),
                Some("y" | "Y" | "yes" | "Yes")
            );
            st.answered(ctx, &ask, yes);
            st.rev += 1;
            st.live_sync(ctx);
            st.refresh(ctx);
            Ok(())
        });
    }

    /// What you answered, kept: a yes by the spec's path and the command (or the file), in what
    /// the session keeps; a no for this session.
    fn answered(&mut self, ctx: &mut Ctx, ask: &NeedsTrust, yes: bool) {
        match ask {
            NeedsTrust::Command { spec, cmd, .. } => {
                let mut t = self.trust.borrow_mut();
                match yes {
                    true => t.allow_command(spec.as_deref(), cmd),
                    false => t.decline_command(spec.as_deref(), cmd),
                }
            }
            NeedsTrust::File { spec, path, .. } => {
                match spec {
                    Some(s) => {
                        let mut t = self.trust.borrow_mut();
                        match yes {
                            true => t.allow_file(s, path),
                            false => t.decline_file(s, path),
                        }
                    }
                    None => self.files_said.push((path.clone(), yes)),
                }
                if yes {
                    self.file_trusted(ctx, path);
                }
            }
        }
        self.trust_rev += 1;
    }

    /// A command you said no to: its panels say `not run: you said no`, a chart its error.
    fn not_run(&mut self, ctx: &mut Ctx, w: &Wanted) {
        let e = not_run(w.spec.cmd());
        match (&w.want, self.dash.as_mut()) {
            (Some(want), Some(d)) => {
                let state = SourceState::Failed(e);
                if d.sources.get(want) != Some(&state) {
                    d.sources.insert(want.clone(), state);
                    self.dash_rebuild(ctx);
                }
            }
            _ => {
                if self.error.as_ref().is_none_or(|f| f.first().msg != e.msg) {
                    let f = Failed {
                        text: self.text.clone(),
                        ..Failed::one(e)
                    };
                    self.failed(ctx, f);
                }
            }
        }
    }

    /// The timer that polls the commands: at the next thing due (a fold's pace while a stream is
    /// open or a run goes, else the next run's tick), and each second between while a dashboard
    /// says how long ago it was updated; none when nothing's due.
    pub(super) fn arm_live(&mut self, ctx: &mut Ctx) {
        let now = ctx.now();
        let next = self.live.as_ref().and_then(|l| l.next_poll(now));
        let tick = (self.dash_live() && next.is_some()).then(|| {
            let s = now.as_secs() + 1;
            Duration::from_secs(s)
        });
        let at = match (next, tick) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
        let Some(at) = at else {
            self.live_timer = None;
            return;
        };
        let me = self.me.clone();
        self.live_timer = Some(ctx.after(at.saturating_sub(now), move |ctx| {
            if let Some(s) = me.upgrade() {
                s.borrow_mut().live_poll(ctx);
            }
            Ok(())
        }));
    }

    /// What the commands did since the last poll, each to where its data goes; the timer again.
    fn live_poll(&mut self, ctx: &mut Ctx) {
        let now = ctx.now();
        let events = match self.live.as_mut() {
            Some(l) => l.poll(now),
            None => return,
        };
        if !events.is_empty() {
            let wanted = self.wanted();
            for e in events {
                self.live_event(ctx, &wanted, e);
            }
            self.rev += 1;
        }
        self.arm_live(ctx);
        self.refresh(ctx);
    }

    /// One thing a command did: its table to the chart or the dashboard's source; a failure
    /// there too (a figure that drew keeps its data, with the stale badge).
    fn live_event(&mut self, ctx: &mut Ctx, wanted: &[Wanted], e: Event) {
        let (key, got) = match e {
            Event::Began(_) => return,
            Event::Data { key, table } => (key, Ok(table)),
            Event::Failed { key, error } => (key, Err(error)),
            Event::Ended { key, error, .. } => match error {
                Some(error) => (key, Err(error)),
                None => return,
            },
        };
        let Some(w) = wanted.iter().find(|w| w.key == key) else {
            return;
        };
        match (&w.want, self.dash.is_some()) {
            (Some(want), true) => {
                let state = match got {
                    Ok(t) => SourceState::Ready(t),
                    Err(e) => SourceState::Failed(e),
                };
                if let Some(d) = self.dash.as_mut() {
                    d.sources.insert(want.clone(), state);
                }
                self.dash_rebuild(ctx);
            }
            _ => match got {
                Ok(t) => self.chart_data(ctx, &w.spec, t),
                Err(e) => self.chart_failed(ctx, e),
            },
        }
    }

    /// A chart's command printed: its table replaced and the model built again from the tables
    /// in hand (the whole figure built again where there are none yet).
    fn chart_data(&mut self, ctx: &mut Ctx, spec: &LiveSpec, table: Arc<Table>) {
        match self.live_tables.iter_mut().find(|(s, _)| s == spec) {
            Some((_, t)) => *t = table.clone(),
            None => self.live_tables.push((spec.clone(), table.clone())),
        }
        let feed = self
            .chart_feeds
            .iter()
            .find(|f| &f.spec == spec)
            .map(|f| f.table);
        match feed {
            Some(i) if self.chart.is_some() && self.model.is_some() && i < self.tables.len() => {
                self.tables[i] = table;
                self.rebuild(ctx);
            }
            _ => self.build(ctx),
        }
    }

    /// A chart's command failed: the chart that drew stays, with the stale badge.
    fn chart_failed(&mut self, ctx: &mut Ctx, e: FigError) {
        let f = Failed {
            text: self.text.clone(),
            ..Failed::one(e)
        };
        self.failed(ctx, f);
    }

    /// `R`: every run source run now and every ended stream started again, the noes forgotten so
    /// they're asked again.
    pub fn rerun(&mut self, ctx: &mut Ctx) {
        self.trust.borrow_mut().ask_again();
        self.trust_rev += 1;
        // A no's `not run` goes: the source waits again until it runs, or you say no again.
        let commands = self.wanted_keys();
        if let Some(d) = self.dash.as_mut() {
            let noes: Vec<Want> = (commands.into_iter())
                .filter(|w| matches!(d.sources.get(w), Some(SourceState::Failed(e)) if e.msg.starts_with("not run")))
                .collect();
            for w in noes {
                d.sources.insert(w, SourceState::Waiting(String::new()));
            }
        }
        let now = ctx.now();
        let said: Vec<String> = match self.live.as_mut() {
            Some(l) => l
                .rerun(None, now)
                .iter()
                .filter_map(Rerun::message)
                .collect(),
            None => vec![],
        };
        ctx.messages.extend(said);
        if self.wanted().is_empty() && self.wanted_files().is_empty() {
            ctx.messages.push("fig: nothing here runs".into());
        }
        self.live_sync(ctx);
        self.dash_rebuild(ctx);
        self.rev += 1;
    }

    /// The dashboard's sources that are commands'.
    fn wanted_keys(&self) -> Vec<Want> {
        self.wanted().into_iter().filter_map(|w| w.want).collect()
    }

    /// What a dashboard's header says of its live sources: `updating…` while a run goes, `error ·
    /// 2 m ago` after a failure, else `live · updated 12 s ago`; None where nothing's live.
    pub(super) fn live_status(&self, now: Duration) -> Option<LiveStatus> {
        if !self.dash_live() {
            return None;
        }
        let Some(l) = self.live.as_ref() else {
            return Some(LiveStatus::paused());
        };
        let all: Vec<Status> = l.keys().iter().filter_map(|k| l.status(k)).collect();
        if all.is_empty() {
            return Some(LiveStatus::paused());
        }
        if all.iter().any(|s| s.running && !s.stream) {
            return Some(LiveStatus::updating());
        }
        let secs = |d: Duration| now.saturating_sub(d).as_secs_f64();
        if let Some(at) = all
            .iter()
            .filter_map(|s| s.error.as_ref().map(|e| e.0))
            .max()
        {
            return Some(LiveStatus::error(secs(at)));
        }
        match all.iter().filter_map(|s| s.updated).max() {
            Some(at) => Some(LiveStatus::updated(secs(at))),
            None => Some(LiveStatus::updating()),
        }
    }

    /// What the status line says of the live sources: `updating…`, `keeping the newest 200,000
    /// rows`, `stream ended (exit 0)`, each once.
    pub(super) fn live_notes(&self) -> Vec<String> {
        let Some(l) = self.live.as_ref() else {
            return vec![];
        };
        let mut out: Vec<String> = vec![];
        for k in l.keys() {
            for n in l.status(&k).map(|s| s.notes()).unwrap_or_default() {
                if !out.contains(&n) {
                    out.push(n);
                }
            }
        }
        out
    }
}

impl Pane {
    /// Whether the window's view follows live data: the dashboard's shared time, else its own.
    pub fn following(&self) -> bool {
        match &self.dash {
            Some(dp) => dp.follow,
            None => self.view.follow,
        }
    }
}
