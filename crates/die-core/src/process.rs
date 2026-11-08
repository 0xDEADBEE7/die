//! Running commands without a terminal, for `compile`, `shell-command`, die-vim's `:!` and DIE's
//! `rebuild`. A command runs in its own process group, with its output and its errors on one
//! pipe, as a terminal shows them. A thread reads the pipe and keeps up to [`LIMIT`] of it, and the
//! rest is cut, so a command that prints megabytes can't fill memory or hold up a key.
//!
//! **The environment.** DIE.app has launchd's PATH, whether it came from the Dock or from `die`,
//! so each command gets the PATH your login shell sets (`$SHELL -l`), asked once per instance on
//! the first command's thread, with `~/.local/bin` and `~/.cargo/bin` first if it hasn't got
//! them, as your `init.el` and `rebuild` add them. `PAGER` is empty, as Emacs's `compile` has it,
//! so `git log` doesn't wait for a pager.
//!
//! This is as much of scope §5.8's process API as M0 needs: pipes only, stdin a filter's text or
//! nothing, and no PTY. M2's `process(cmd)` and `pty(cmd, size)` (contract §12) replace it, with
//! `posix_spawn` and the signals from `sys`, and keep what it says about the PATH.
mod compile;
mod errors;
mod shell;

pub(crate) use compile::COMPILATION;
pub(crate) use errors::Results;
pub use shell::Ran;

use crate::error::Result;
use crate::ext::{Ctx, Registry};
use crate::ids::BufferId;
use crate::rt::{Sub, Waker};
use crate::text::{Origin, TextBuf};
use crate::workspace::{ShowIn, Side};
use std::ffi::{OsStr, OsString};
use std::io::{self, Read, Write};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, OnceLock, mpsc};
use std::time::{Duration, Instant};

/// How much of a command's output is kept.
pub(crate) const LIMIT: usize = 8 << 20;
/// How long, once a command has exited, what it wrote last has to come out of the pipe. A
/// process it left behind holding the pipe isn't waited for.
const DRAIN: Duration = Duration::from_millis(500);
/// How long a login shell gets to say its PATH.
const LOGIN_WAIT: Duration = Duration::from_secs(3);

/// What a command has printed so far, up to [`LIMIT`].
#[derive(Default)]
pub(crate) struct Output {
    said: Mutex<(Vec<u8>, usize)>,
    /// Something came since the last take.
    fresh: AtomicBool,
    cut: AtomicBool,
}

impl Output {
    /// Keep `chunk`, as far as the limit. False once it's reached.
    fn put(&self, chunk: &[u8]) -> bool {
        let mut said = self.said.lock().unwrap_or_else(|e| e.into_inner());
        let room = LIMIT.saturating_sub(said.1);
        let kept = &chunk[..chunk.len().min(room)];
        said.0.extend_from_slice(kept);
        said.1 += kept.len();
        self.fresh.store(true, Ordering::Release);
        if kept.len() < chunk.len() {
            self.cut.store(true, Ordering::Release);
        }
        kept.len() == chunk.len()
    }
    /// What came since the last take, in whole lines, or with `all` everything. Bytes that
    /// aren't UTF-8 show as replacements.
    pub(crate) fn take(&self, all: bool) -> String {
        let mut said = self.said.lock().unwrap_or_else(|e| e.into_inner());
        self.fresh.store(false, Ordering::Release);
        let end = match all {
            true => said.0.len(),
            false => said
                .0
                .iter()
                .rposition(|b| *b == b'\n')
                .map_or(0, |at| at + 1),
        };
        let taken: Vec<u8> = said.0.drain(..end).collect();
        String::from_utf8_lossy(&taken).into_owned()
    }
    /// Whether output past [`LIMIT`] was dropped.
    pub(crate) fn cut(&self) -> bool {
        self.cut.load(Ordering::Acquire)
    }
}

/// A command's process group, which a signal goes to: the command and all it started.
#[derive(Clone, Default)]
pub(crate) struct Group(Arc<AtomicU32>);

impl Group {
    /// Send `signal` (`INT`, `KILL`) to the group, if it has started. `die-core` has no `unsafe`,
    /// so `/bin/kill` sends it until M2's `sys` can.
    pub(crate) fn signal(&self, signal: &str) {
        let pid = self.0.load(Ordering::Acquire);
        if pid != 0 {
            let mut kill = Command::new("/bin/kill");
            kill.args(["-s", signal, "--", &format!("-{pid}")]);
            let _ = kill.stdout(Stdio::null()).stderr(Stdio::null()).status();
        }
    }
}

/// The command a shell runs for `line` in `dir`, as Emacs's `shell-command` and `compile` run
/// theirs: your `$SHELL` with `-c`, or `/bin/sh`.
pub fn shell(line: &str, dir: &Path) -> Command {
    let sh = std::env::var_os("SHELL").filter(|s| !s.is_empty());
    let mut cmd = Command::new(sh.unwrap_or_else(|| "/bin/sh".into()));
    cmd.arg("-c").arg(line).current_dir(dir);
    cmd
}

/// The PATH commands get: what your login shell sets, asked once, with `~/.local/bin` and
/// `~/.cargo/bin` first unless it has them. Without an answer, it's the PATH DIE has.
pub fn path() -> &'static OsStr {
    static PATH: OnceLock<OsString> = OnceLock::new();
    PATH.get_or_init(|| {
        let sh = std::env::var_os("SHELL").filter(|s| !s.is_empty());
        let asked = sh.and_then(|sh| login_path(&sh, LOGIN_WAIT));
        let path = asked
            .or_else(|| std::env::var_os("PATH"))
            .unwrap_or_default();
        with_own(&path, std::env::home_dir().as_deref())
    })
}

/// The PATH `shell -l -c` prints, the last line of what it prints, so whatever your profile
/// prints first doesn't count. None if it fails or takes longer than `wait`.
fn login_path(shell: &OsStr, wait: Duration) -> Option<OsString> {
    let mut cmd = Command::new(shell);
    cmd.args(["-l", "-c", "printf '\\n%s' \"$PATH\""]);
    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()
        .ok()?;
    let start = Instant::now();
    while child.try_wait().ok()?.is_none() {
        if start.elapsed() > wait {
            let _ = child.kill();
            let _ = child.wait();
            return None;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    let mut said = String::new();
    child.stdout.take()?.read_to_string(&mut said).ok()?;
    let path = said.rsplit('\n').next().unwrap_or_default();
    (!path.is_empty()).then(|| path.into())
}

/// `path` with `~/.local/bin` and `~/.cargo/bin` first, unless it has them.
fn with_own(path: &OsStr, home: Option<&Path>) -> OsString {
    let dirs: Vec<PathBuf> = std::env::split_paths(path).collect();
    let own = [".local/bin", ".cargo/bin"].map(|d| home.map(|h| h.join(d)));
    let missing: Vec<_> = own
        .into_iter()
        .flatten()
        .filter(|d| !dirs.contains(d))
        .collect();
    std::env::join_paths(missing.into_iter().chain(dirs)).unwrap_or_else(|_| path.to_owned())
}

/// The command as a terminal would show it.
pub fn shown(cmd: &Command) -> String {
    let program = Path::new(cmd.get_program()).file_name().unwrap_or_default();
    let words = std::iter::once(program).chain(cmd.get_args());
    let words: Vec<_> = words.map(|w| w.to_string_lossy()).collect();
    words.join(" ")
}

/// Run `cmd` until it exits, with `input` on its stdin or nothing, and its output and errors
/// together in `out`. Its process group goes in `group`. Each chunk wakes `waker`, and with
/// `stop`, output past the limit stops the command. Unless `cmd` sets a PATH, it gets [`path`].
pub(crate) fn run(
    mut cmd: Command,
    input: Option<Vec<u8>>,
    out: Arc<Output>,
    group: &Group,
    waker: Option<Waker>,
    stop: bool,
) -> io::Result<ExitStatus> {
    if !cmd.get_envs().any(|(k, _)| k == "PATH") {
        cmd.env("PATH", path());
    }
    let (mut reader, writer) = io::pipe()?;
    let stdin = if input.is_some() {
        Stdio::piped()
    } else {
        Stdio::null()
    };
    cmd.stdin(stdin)
        .stdout(writer.try_clone()?)
        .stderr(writer)
        .env("PAGER", "")
        .process_group(0);
    let spawned = cmd.spawn();
    let dir = cmd.get_current_dir().map(Path::to_path_buf);
    let what = format!("{} in {}", shown(&cmd), dir.unwrap_or_default().display());
    drop(cmd); // its copies of the pipe's writing end, or the reading never ends
    let mut child = spawned.map_err(|e| io::Error::new(e.kind(), format!("{what}: {e}")))?;
    group.0.store(child.id(), Ordering::Release);
    if let (Some(text), Some(mut stdin)) = (input, child.stdin.take()) {
        // A command that doesn't read it all closes the pipe, and that's no error.
        std::thread::spawn(move || drop(stdin.write_all(&text)));
    }
    let (read_all, all_read) = mpsc::channel();
    let stopper = group.clone();
    std::thread::spawn(move || {
        let mut chunk = [0; 8192];
        let mut kept = true;
        loop {
            match reader.read(&mut chunk) {
                Ok(0) => break,
                Ok(n) if kept => {
                    kept = out.put(&chunk[..n]);
                    if !kept && stop {
                        stopper.signal("KILL");
                    }
                    if let Some(w) = &waker {
                        w.wake();
                    }
                }
                Ok(_) => {}
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(_) => break,
            }
        }
        let _ = read_all.send(());
    });
    let status = child.wait();
    let _ = all_read.recv_timeout(DRAIN);
    status
}

/// How a command ended, in Emacs's words: `finished`, `exited abnormally with code 2`,
/// `interrupt`, `killed`.
pub(crate) fn ended(status: &ExitStatus) -> String {
    use std::os::unix::process::ExitStatusExt;
    match (status.code(), status.signal()) {
        (Some(0), _) => "finished".into(),
        (Some(n), _) => format!("exited abnormally with code {n}"),
        (None, Some(2)) => "interrupt".into(),
        (None, Some(9)) => "killed".into(),
        (None, Some(15)) => "terminated".into(),
        (None, Some(1)) => "hangup".into(),
        (None, s) => format!("signal {}", s.unwrap_or(0)),
    }
}

/// A command whose output goes into a buffer as it comes.
struct Stream {
    id: u64,
    buf: BufferId,
    out: Arc<Output>,
    group: Group,
    _job: Sub,
}

/// What's running and what `next-error` goes through.
#[derive(Default)]
pub(crate) struct Procs {
    /// What `*grep*` holds, and the search that's filling it.
    pub(crate) grep: Option<crate::project::Grep>,
    /// The results `next-error` can go through, the last made or gone to first.
    results: Vec<Results>,
    streams: Vec<Stream>,
    compile: compile::Compile,
    next_id: u64,
}

/// How a streamed command ended.
pub struct Finished {
    /// Its status, or why it didn't run.
    pub status: io::Result<ExitStatus>,
    /// Output past [`LIMIT`] was dropped.
    pub cut: bool,
}

impl Ctx {
    /// Run `cmd`, its output going into `buf` as it comes, at its end, and give how it ended to
    /// `done`. `buf` becomes what `next-error` goes through, with file names read from its
    /// directory as `compile` reads them, and `<CR>` on a line opens the place it names. Returns
    /// an id for [`Ctx::stop_stream`]. Killing the buffer leaves the command running unseen.
    pub fn stream(
        &mut self,
        buf: BufferId,
        cmd: Command,
        done: impl FnOnce(&mut Ctx, Finished) -> Result + 'static,
    ) -> u64 {
        let (out, group) = (Arc::new(Output::default()), Group::default());
        let (o, g, waker) = (out.clone(), group.clone(), self.waker());
        let id = self.procs.next_id + 1;
        self.procs.next_id = id;
        let job = self.spawn(
            move || run(cmd, None, o, &g, Some(waker), false),
            move |ctx, status| {
                let Some(at) = ctx.procs.streams.iter().position(|s| s.id == id) else {
                    return Ok(());
                };
                let s = ctx.procs.streams.remove(at);
                ctx.append_output(s.buf, &s.out.take(true));
                let cut = s.out.cut();
                if cut {
                    let note = format!("\n[DIE kept the first {} MiB of this.]\n", LIMIT >> 20);
                    ctx.append_output(s.buf, &note);
                }
                done(ctx, Finished { status, cut })
            },
        );
        let dir = self.directory(buf).map(Path::to_path_buf);
        if !self.procs.results.iter().any(|r| r.buf == buf) {
            let local = &mut self.buffers[buf].local;
            local.bind(crate::keymap::State::Normal, "<CR>", "compile-goto-error");
        }
        self.set_results(Results::compile(buf, dir.unwrap_or_default()));
        self.procs.streams.push(Stream {
            id,
            buf,
            out,
            group,
            _job: job,
        });
        id
    }

    /// Whether the command [`Ctx::stream`] gave this id is still running.
    pub fn stream_running(&self, id: u64) -> bool {
        self.procs.streams.iter().any(|s| s.id == id)
    }

    /// Send `signal` to a streamed command's group: `INT` as Emacs's `kill-compilation` does, or
    /// `KILL`. With `forget`, its output stops and `done` won't run.
    pub fn stop_stream(&mut self, id: u64, signal: &str, forget: bool) {
        let Some(at) = self.procs.streams.iter().position(|s| s.id == id) else {
            return;
        };
        self.procs.streams[at].group.signal(signal);
        if forget {
            self.procs.streams.remove(at);
        }
    }

    /// Put what streamed commands have printed into their buffers, as each turn does. True if
    /// that was news.
    pub(crate) fn streams_take(&mut self) -> bool {
        let fresh = |s: &Stream| s.out.fresh.load(Ordering::Acquire);
        let news: Vec<_> = self
            .procs
            .streams
            .iter()
            .filter(|s| fresh(s))
            .map(|s| (s.buf, s.out.clone()))
            .collect();
        for (buf, out) in &news {
            self.append_output(*buf, &out.take(false));
        }
        !news.is_empty()
    }

    /// Add `text` to the end of an output buffer, unsaved and with no undo. A window whose cursor
    /// is on its last line follows it there, as `*messages*` does.
    pub fn append_output(&mut self, buf: BufferId, text: &str) {
        if text.is_empty() {
            return;
        }
        let windows = self.windows_showing(buf);
        let Ok(t) = self.text_mut(buf) else { return };
        let last = last_line_start(t);
        let following: Vec<_> = windows
            .into_iter()
            .filter(|w| t.cursor(*w).is_some_and(|at| at >= last))
            .collect();
        let end = t.len();
        let _ = t.tx(Origin::Extension).silent().insert(end, text).commit();
        t.set_modified(false);
        let last = last_line_start(t);
        for w in following {
            t.set_cursor(w, last);
        }
    }

    /// Show results as Emacs does, with the focus staying: in a window that has them, or another
    /// window that's no terminal, or else a split.
    pub(crate) fn show_results(&mut self, buf: BufferId) -> Result {
        if !self.windows_showing(buf).is_empty() {
            return Ok(());
        }
        let here = self.focused;
        let others = self.windows_by_use().into_iter();
        let mut others = others.filter(|w| Some(*w) != here);
        if let Some(w) = others.find(|w| self.takes_source(*w, buf)) {
            self.set_window_buffer(w, buf);
            return Ok(());
        }
        let shown = self.show(buf, ShowIn::Split(Side::Below));
        shown.or_else(|_| self.show(buf, ShowIn::Current))?;
        if let Some(w) = here.filter(|w| self.windows.contains_key(*w)) {
            self.focus(w);
        }
        Ok(())
    }

    /// An output buffer by this name, made read-only and as wide as the window, and emptied to
    /// `text`, with file names in it from `dir`. Each time, since one a session brought back has
    /// your text defaults.
    pub fn output_buffer(&mut self, name: &str, text: &str, dir: &Path) -> Result<BufferId> {
        let buf = match self.find_buffer(name) {
            Some(buf) => buf,
            None => self.create_text(name, ""),
        };
        let t = self.text_mut(buf)?;
        let settings = t.settings_mut();
        (settings.read_only, settings.max_width, settings.center) = (true, None, false);
        let all = 0..t.len();
        t.tx(Origin::Extension)
            .silent()
            .replace(all, text)
            .commit()?;
        t.set_modified(false);
        self.set_directory(buf, dir);
        Ok(buf)
    }
}

/// Where the last line starts, not counting the empty one after a final newline.
pub fn last_line_start(t: &TextBuf) -> usize {
    let lines = t.line_count();
    let ends_in_newline = !t.is_empty() && t.line_start(lines - 1) == t.len();
    t.line_start(lines - 1 - usize::from(ends_in_newline))
}

pub(crate) fn register_commands(reg: &mut Registry) {
    errors::register(reg);
    compile::register(reg);
    shell::register(reg);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_login_shells_path_is_its_last_line_and_a_slow_one_gets_none() {
        let dir = std::env::temp_dir().join(format!("die-login-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let shell = |name: &str, body: &str| {
            let p = dir.join(name);
            std::fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
            let chmod = Command::new("chmod").arg("+x").arg(&p).status();
            assert!(chmod.unwrap().success());
            p.into_os_string()
        };
        let noisy = shell(
            "noisy",
            "echo welcome; echo; printf '\\n/fake/bin:/usr/bin'",
        );
        let wait = Duration::from_secs(2);
        assert_eq!(login_path(&noisy, wait), Some("/fake/bin:/usr/bin".into()));
        let slow = shell("slow", "sleep 5");
        let start = Instant::now();
        assert_eq!(login_path(&slow, Duration::from_millis(200)), None);
        assert!(start.elapsed() < Duration::from_secs(2));
        assert_eq!(login_path(&shell("empty", "true"), wait), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn local_and_cargo_bin_go_first_unless_the_path_has_them() {
        let home = Path::new("/h");
        let path = with_own(OsStr::new("/usr/bin:/h/.cargo/bin"), Some(home));
        assert_eq!(path, "/h/.local/bin:/usr/bin:/h/.cargo/bin");
        assert_eq!(with_own(OsStr::new("/usr/bin"), None), "/usr/bin");
    }

    #[test]
    fn output_comes_in_whole_lines_up_to_the_limit() {
        let out = Output::default();
        assert!(out.put(b"one\ntw"));
        assert_eq!(out.take(false), "one\n");
        assert_eq!(out.take(true), "tw");
        assert!(!out.put(&vec![b'x'; LIMIT + 1]));
        assert!(out.cut());
        assert_eq!(
            out.take(true).len(),
            LIMIT - 6,
            "the limit counts what was taken"
        );
    }
}
