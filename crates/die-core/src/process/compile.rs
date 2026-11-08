//! `compile`, `recompile` and `kill-compilation`, as Emacs's: a shell command run in the
//! buffer's directory, its output streaming into `*compilation*` under Emacs's header, with its
//! `Compilation finished` line at the end. One runs at a time. The buffer has evil-collection's
//! keys for it: `<CR>` opens the error on its line, `gj` `gk` `<C-j>` `<C-k>` `<Tab>` `<S-Tab>`
//! move between errors, `gr` compiles again and `C-c C-k` stops it. Like `*grep*`, it isn't
//! kept by the session, but the command is, in the `compile` history.
use super::errors::Locus;
use super::{Finished, ended, shell};
use crate::error::{Error, Result};
use crate::ext::{Ctx, Registry};
use crate::ids::BufferId;
use crate::keymap::State;
use crate::project::here_dir;
use crate::prompt::{HISTORY, PromptSpec};
use crate::text::TextBuf;
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub(crate) const COMPILATION: &str = "*compilation*";
/// Emacs's `compile-command`, until one is run.
const DEFAULT_COMMAND: &str = "make -k ";
/// The keys `*compilation*` has, as evil-collection gives them in normal state.
const KEYS: &[(&str, &str)] = &[
    ("<CR>", "compile-goto-error"),
    ("gr", "recompile"),
    ("<C-c><C-k>", "kill-compilation"),
    ("gj", "compilation-next-error"),
    ("<C-j>", "compilation-next-error"),
    ("<Tab>", "compilation-next-error"),
    ("gk", "compilation-previous-error"),
    ("<C-k>", "compilation-previous-error"),
    ("<S-Tab>", "compilation-previous-error"),
];

/// What starts a GNU message that's a note, as Emacs's `gnu` pattern has them.
const NOTES: [&str; 8] = [
    "note",
    "Note",
    "info",
    "Info",
    "help",
    "I:",
    "instantiated from",
    "required from",
];

/// The compilation last run, and whether it's running.
#[derive(Default)]
pub(crate) struct Compile {
    /// Its command and directory, which `recompile` runs again.
    last: Option<(String, PathBuf)>,
    stream: Option<u64>,
}

/// The place a compilation's line names, as Emacs's `compilation-error-regexp-alist` finds them,
/// with rustc's and cargo's own forms: ` --> path:line:col` (and ` ::: `) under its `error`,
/// `warning` or `note` line, which a `note` or `help` makes a note, `thread 'name' panicked at
/// path:line:col:`, and GNU's `path:line:col: message` or `path:line: message`, whose message
/// makes it a note if it starts with one. A path has no spaces and isn't only digits.
pub(crate) fn locus(t: &TextBuf, line: usize, text: &str) -> Option<Locus> {
    let text = text.trim_end_matches(['\n', '\r']);
    let indented = text.trim_start();
    if let Some(at) = indented
        .strip_prefix("--> ")
        .or(indented.strip_prefix("::: "))
    {
        let (path, line_no, col) = place(at)?;
        let head = (0..line).rev().take(200).find_map(|l| {
            let l = t.rope().line(l).to_string();
            l.starts_with(|c: char| c.is_ascii_alphabetic())
                .then_some(l)
        });
        let note = head.is_some_and(|h| h.starts_with("note") || h.starts_with("help"));
        return Some(Locus {
            path,
            line: line_no,
            col,
            note,
        });
    }
    // The test harness puts the thread's id after its name: `thread 'a' (1234) panicked at`.
    let panicked = text
        .strip_prefix("thread '")
        .and_then(|r| r.split_once(" panicked at "));
    if let Some((_, at)) = panicked {
        let (path, line, col) = place(at.strip_suffix(':').unwrap_or(at))?;
        return Some(Locus {
            path,
            line,
            col,
            note: false,
        });
    }
    gnu(text)
}

/// `path:line:col`, the whole of `s`.
fn place(s: &str) -> Option<(String, usize, Option<usize>)> {
    let (head, col) = s.rsplit_once(':')?;
    let (path, line) = head.rsplit_once(':')?;
    let ok = !path.is_empty() && !path.contains(' ');
    ok.then(|| {
        Some((
            path.to_string(),
            line.parse().ok()?,
            Some(col.parse().ok()?),
        ))
    })?
}

/// GNU's `path:line:col: message` or `path:line: message`, or nothing after the last `:`.
fn gnu(text: &str) -> Option<Locus> {
    fn number(s: &str) -> Option<(usize, &str)> {
        let n = s.bytes().take_while(u8::is_ascii_digit).count();
        Some((s[..n].parse().ok()?, s[n..].strip_prefix(':')?))
    }
    text.match_indices(':').find_map(|(at, _)| {
        let path = &text[..at];
        let bad = path.is_empty() || path.contains(char::is_whitespace);
        if bad || path.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        let (line, rest) = number(&text[at + 1..])?;
        let (col, rest) = match number(rest) {
            Some((col, rest)) => (Some(col), rest),
            None => (None, rest),
        };
        if !rest.is_empty() && !rest.starts_with(' ') {
            return None;
        }
        let said = rest.trim_start();
        let note = NOTES.iter().any(|n| said.starts_with(n));
        Some(Locus {
            path: path.to_string(),
            line,
            col,
            note,
        })
    })
}

impl Ctx {
    /// The command `compile` offers: the last one run, or the newest in its history, or Emacs's.
    fn compile_command(&self) -> String {
        let last = self.procs.compile.last.as_ref().map(|l| l.0.clone());
        let past = self.prompt_history("compile").last().cloned();
        last.or(past).unwrap_or_else(|| DEFAULT_COMMAND.into())
    }

    /// Compile, as Emacs does: unsaved files first, then a running one is stopped if you say so.
    fn compile(&mut self, command: String, dir: PathBuf) -> Result {
        let past = self.histories.entry("compile".into()).or_default();
        if past.last() != Some(&command) {
            past.push(command.clone());
            let over = past.len().saturating_sub(HISTORY);
            past.drain(..over);
        }
        let unsaved = |ctx: &Ctx, b: BufferId| {
            ctx.file(b).is_some() && ctx.text(b).is_ok_and(|t| t.modified())
        };
        let mut left: Vec<_> = self
            .buffer_list()
            .into_iter()
            .filter(|b| unsaved(self, *b))
            .collect();
        left.reverse();
        let then = move |ctx: &mut Ctx| ctx.compile_or_ask(command, dir);
        save_some(self, left, Box::new(then))
    }

    /// Start, or first ask to stop the one running, as Emacs's `compilation-start` does.
    fn compile_or_ask(&mut self, command: String, dir: PathBuf) -> Result {
        let running = self
            .procs
            .compile
            .stream
            .filter(|id| self.stream_running(*id));
        let Some(id) = running else {
            return self.compile_start(command, dir);
        };
        let label = "A compilation process is running; kill it? (yes or no) ";
        self.prompt(PromptSpec::new(label), move |ctx, answer| {
            if answer.as_deref() != Some("yes") {
                let why = format!("Cannot have two processes in `{COMPILATION}' at once");
                return Err(Error::Msg(why));
            }
            ctx.stop_stream(id, "KILL", true);
            ctx.compile_start(command, dir)
        });
        Ok(())
    }

    fn compile_start(&mut self, command: String, dir: PathBuf) -> Result {
        let home = std::env::home_dir().map(|h| h.to_string_lossy().into_owned());
        let shown = dir.to_string_lossy().into_owned();
        let shown = match home.as_deref().and_then(|h| shown.strip_prefix(h)) {
            Some(rest) if rest.is_empty() || rest.starts_with('/') => format!("~{rest}"),
            _ => shown,
        };
        let slash = if shown.ends_with('/') { "" } else { "/" };
        let header = format!(
            "-*- mode: compilation; default-directory: {:?} -*-\nCompilation started at {}\n\n{command}\n",
            format!("{shown}{slash}"),
            now()
        );
        let new = self.find_buffer(COMPILATION).is_none();
        let buf = self.output_buffer(COMPILATION, &header, &dir)?;
        if new {
            for (keys, name) in KEYS {
                self.buffers[buf].local.bind(State::Normal, keys, name);
            }
        }
        for w in self.windows_showing(buf) {
            self.text_mut(buf)?.set_cursor(w, 0);
        }
        self.show_results(buf)?;
        let started = Instant::now();
        let cmd = shell(&command, &dir);
        let id = self.stream(buf, cmd, move |ctx, f| compile_ended(ctx, buf, f, started));
        self.procs.compile = Compile {
            last: Some((command, dir)),
            stream: Some(id),
        };
        Ok(())
    }
}

/// Write Emacs's last line, and say it.
fn compile_ended(ctx: &mut Ctx, buf: BufferId, f: Finished, started: Instant) -> Result {
    let how = match f.status {
        Ok(s) => ended(&s),
        Err(e) => {
            ctx.append_output(buf, &format!("\n{e}\n"));
            return Err(Error::Msg(format!("The compilation didn't run: {e}")));
        }
    };
    let took = duration(started.elapsed());
    ctx.append_output(
        buf,
        &format!("\nCompilation {how} at {}, duration {took}\n", now()),
    );
    ctx.messages.push(format!("Compilation {how}"));
    Ok(())
}

/// Ask to save each file with unsaved changes, as Emacs's `save-some-buffers` does before a
/// compile: `y` saves it, `n` doesn't, `!` saves it and the rest, and `q` saves no more. A cancel
/// stops there, with nothing run. Then `then`.
fn save_some(
    ctx: &mut Ctx,
    mut left: Vec<BufferId>,
    then: Box<dyn FnOnce(&mut Ctx) -> Result>,
) -> Result {
    let Some(&buf) = left.last() else {
        return then(ctx);
    };
    let path = ctx
        .file(buf)
        .map(|f| f.path.display().to_string())
        .unwrap_or_default();
    let label = format!("Save file {path}? (y, n, !, q) ");
    ctx.prompt(PromptSpec::new(&label), move |ctx, answer| {
        let Some(answer) = answer else {
            return Ok(());
        };
        left.pop();
        match answer.as_str() {
            "y" | "yes" => ctx.save(buf)?,
            "!" => {
                for b in left.drain(..).chain([buf]) {
                    ctx.save(b)?;
                }
            }
            "q" => left.clear(),
            _ => {}
        }
        save_some(ctx, left, then)
    });
    Ok(())
}

/// Now as Emacs's `(substring (current-time-string) 0 19)` has it, `Sun Nov  8 12:34:56`, in the
/// local time `date +%z` gave this instance once.
fn now() -> String {
    static OFFSET: std::sync::OnceLock<i64> = std::sync::OnceLock::new();
    let offset = *OFFSET.get_or_init(|| {
        let out = std::process::Command::new("/bin/date").arg("+%z").output();
        let z = out.map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());
        let z = z.unwrap_or_default();
        let (sign, hhmm) = z.split_at_checked(1).unwrap_or(("+", "0000"));
        let n: i64 = hhmm.parse().unwrap_or(0);
        (n / 100 * 3600 + n % 100 * 60) * if sign == "-" { -1 } else { 1 }
    });
    let unix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64);
    clock(unix + offset)
}

/// `Sun Nov  8 12:34:56` for these seconds since 1970 began.
fn clock(secs: i64) -> String {
    const DAYS: [&str; 7] = ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"];
    const MONTHS: [&str; 12] = [
        "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec", "Jan", "Feb",
    ];
    let (days, s) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // Howard Hinnant's `civil_from_days`, with the year left out, from a March 1st.
    let doe = (days + 719_468).rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let (wd, month) = (DAYS[days.rem_euclid(7) as usize], MONTHS[mp as usize]);
    format!(
        "{wd} {month} {day:2} {:02}:{:02}:{:02}",
        s / 3600,
        s / 60 % 60,
        s % 60
    )
}

/// A duration as Emacs's compile gives it: `4.56 s`, `12.3 s`, `1:02:03`.
fn duration(d: Duration) -> String {
    let s = d.as_secs_f64();
    match s {
        _ if s < 10.0 => format!("{s:.2} s"),
        _ if s < 60.0 => format!("{s:.1} s"),
        _ => {
            let n = d.as_secs();
            format!("{}:{:02}:{:02}", n / 3600, n / 60 % 60, n % 60)
        }
    }
}

pub(super) fn register(reg: &mut Registry) {
    reg.command(
        "compile",
        "Run a shell command in this buffer's directory, with its output in *compilation*, where \
         next-error finds its errors. With a command after the name, run that; else ask for one",
        |ctx, a| {
            let dir = here_dir(ctx);
            if let Some(command) = a.text.clone().filter(|t| !t.trim().is_empty()) {
                return ctx.compile(command, dir);
            }
            let spec = PromptSpec::new("Compile command: ")
                .initial(&ctx.compile_command())
                .history("compile");
            ctx.prompt(spec, move |ctx, answer| {
                match answer.filter(|a| !a.trim().is_empty()) {
                    Some(command) => ctx.compile(command, dir),
                    None => Ok(()),
                }
            });
            Ok(())
        },
    );
    reg.command(
        "recompile",
        "Run the last compile command again, where it ran",
        |ctx, _| {
            let last = ctx.procs.compile.last.clone();
            let (command, dir) = last.unwrap_or_else(|| (ctx.compile_command(), here_dir(ctx)));
            ctx.compile(command, dir)
        },
    );
    reg.command(
        "kill-compilation",
        "Stop the compilation that's running, as Ctrl-C would",
        |ctx, _| match ctx
            .procs
            .compile
            .stream
            .filter(|id| ctx.stream_running(*id))
        {
            Some(id) => {
                ctx.stop_stream(id, "INT", false);
                Ok(())
            }
            None => Err(Error::Msg("The compilation process is not running".into())),
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_clock_reads_as_emacs_has_it() {
        assert_eq!(clock(0), "Thu Jan  1 00:00:00");
        assert_eq!(clock(1_794_141_296), "Sun Nov  8 12:34:56");
        assert_eq!(clock(951_782_400), "Tue Feb 29 00:00:00");
        assert_eq!(duration(Duration::from_millis(4560)), "4.56 s");
        assert_eq!(duration(Duration::from_millis(12_340)), "12.3 s");
        assert_eq!(duration(Duration::from_secs(3723)), "1:02:03");
    }

    #[test]
    fn gnu_lines_name_places_and_prose_doesnt() {
        let at = |s: &str| gnu(s).map(|l| (l.path, l.line, l.col, l.note));
        assert_eq!(
            at("src/a.rs:3:5: error: no"),
            Some(("src/a.rs".into(), 3, Some(5), false))
        );
        assert_eq!(
            at("a.c:12: warning: x"),
            Some(("a.c".into(), 12, None, false))
        );
        assert_eq!(at("a.c:12:"), Some(("a.c".into(), 12, None, false)));
        assert_eq!(
            at("a.c:1:2: note: here"),
            Some(("a.c".into(), 1, Some(2), true))
        );
        for prose in [
            "error: could not compile",
            "12:34:56 x",
            "at 12:34:56, duration",
            "a:b:c",
            "x.rs:3x",
        ] {
            assert_eq!(at(prose), None, "{prose}");
        }
    }
}
