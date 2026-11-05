//! Test fixtures that use only the public API: a toy die-vim grammar, a fold mode in
//! manifold's shape, and a toy terminal surface. Throwaway, like the contract-test fixtures (§16).
#![allow(dead_code)]
pub mod naive_layout;
use die_core::*;
use std::cell::RefCell;
use std::rc::Rc;

pub fn quiet_panics() {
    std::panic::set_hook(Box::new(|_| {}));
}

/// How far a limit in ms stretches at background priority, which macOS can give a whole
/// session while its terminal isn't in use. The same frame then takes about 3.6 times as
/// long: 0.27 ms against 0.94 ms for scene's 1 ms test, measured in turns on one machine.
/// Priorities below 10 are background on macOS, and nice 10 or more on Linux, where the same
/// factor is a guess. With no `ps` to ask, a limit stays as it is.
pub fn stretch_at(priority: Option<i32>) -> f64 {
    if priority.is_some_and(|p| p < 10) {
        4.0
    } else {
        1.0
    }
}

fn priority() -> Option<i32> {
    let pid = std::process::id().to_string();
    let ps = std::process::Command::new("ps")
        .args(["-o", "pri=", "-p", &pid])
        .output()
        .ok()?;
    String::from_utf8(ps.stdout).ok()?.trim().parse().ok()
}

/// Set by `cargo xtask ci --linux`. Its two-CPU VM misses scene's 1 ms test by 10 to 40% with
/// nothing wrong, and `ps` can't see that from inside. So it gets the same stretch.
const SLOW_HOST: &str = "DIE_SLOW_HOST";

/// Three tries at a limit in ms, so a busy machine doesn't fail it. A limit in ms means a
/// machine at its normal speed, so it stretches on a slow host, or if the process is at
/// background priority when the tries are done. Limits on a ratio don't need this: both
/// sides slow down alike.
pub fn within_three_tries_ms(limit: f64, mut measure: impl FnMut() -> f64) {
    let tries: Vec<f64> = (0..3).map(|_| measure()).collect();
    println!("{tries:.3?}, limit {limit} ms");
    let best = tries.iter().copied().fold(f64::INFINITY, f64::min);
    if best > limit {
        let slow_host = std::env::var_os(SLOW_HOST).is_some();
        let allowed = limit
            * if slow_host {
                4.0
            } else {
                stretch_at(priority())
            };
        println!("over {limit} ms, so the limit at this priority is {allowed} ms");
        assert!(best <= allowed, "{tries:.3?} are all over {allowed} ms");
    }
}

/// The buffer and window a command acts on.
pub fn target(ctx: &Ctx, a: &Args) -> (BufferId, WindowId) {
    let w = a.window.or(ctx.focused()).expect("a window");
    (a.buffer.or(ctx.window_buffer(w)).expect("a buffer"), w)
}

// ---------------------------------------------------------------- toy die-vim

enum Pending {
    Register,
    Record,
    Exec,
}

struct Rec {
    reg: char,
    keys: Rc<RefCell<Vec<KeyEvent>>>,
    _sub: Sub,
}

#[derive(Default)]
pub struct Vim {
    pending: Option<Pending>,
    rec: Option<Rec>,
}

impl Extension for Vim {
    fn name(&self) -> &'static str {
        "die-vim"
    }
    fn register(&mut self, reg: &mut Registry) {
        use State::*;
        let k = reg.kind_keymap("text");
        for (keys, cmd) in [
            ("h", "vim-left"),
            ("l", "vim-right"),
            ("j", "vim-down"),
            ("k", "vim-up"),
            ("gj", "vim-down-visual"),
            ("gk", "vim-up-visual"),
            ("x", "vim-delete-char"),
            ("dd", "vim-delete-line"),
            ("i", "vim-insert"),
            ("u", "vim-undo"),
            ("<C-r>", "vim-redo"),
            ("0", "vim-zero"),
            ("\"", "vim-select-register"),
            ("q", "vim-q"),
            ("@", "vim-at"),
            ("/", "vim-search"),
        ] {
            k.bind(Normal, keys, cmd);
        }
        for d in '1'..='9' {
            k.bind(Normal, &d.to_string(), "vim-digit");
        }
        k.bind(Insert, "<char>", "vim-self-insert")
            .bind(Insert, "<Esc>", "vim-normal")
            .bind(Insert, "<CR>", "vim-newline")
            .bind(Insert, "<BS>", "vim-backspace")
            .bind(OperatorPending, "<char>", "vim-pending-char")
            .bind(OperatorPending, "<any>", "vim-cancel");

        reg.command("vim-left", "", |ctx, a| horizontal(ctx, a, Dir::Backward));
        reg.command("vim-right", "", |ctx, a| horizontal(ctx, a, Dir::Forward));
        reg.command("vim-down", "", |ctx, a| vertical(ctx, a, Dir::Forward));
        reg.command("vim-up", "", |ctx, a| vertical(ctx, a, Dir::Backward));
        reg.command("vim-down-visual", "", |ctx, a| visual(ctx, a, 1));
        reg.command("vim-up-visual", "", |ctx, a| visual(ctx, a, -1));
        reg.command("vim-delete-char", "", |ctx, a| {
            let (b, w) = target(ctx, a);
            let t = ctx.text_mut(b)?;
            let p = t.cursor(w).unwrap();
            match t.rope().byte_slice(p..).chars().next() {
                Some(c) if c != '\n' => {
                    t.tx(Origin::User).delete(p..p + c.len_utf8()).commit()?;
                }
                _ => {}
            }
            Ok(())
        });
        reg.command(
            "vim-delete-line",
            "dd: delete [count] lines (a closed fold counts as one)",
            |ctx, a| {
                let (b, w) = target(ctx, a);
                let mut yanked = String::new();
                for _ in 0..a.count.unwrap_or(1) {
                    let t = ctx.text_mut(b)?;
                    let dl = t.display_line(t.cursor(w).unwrap());
                    let (mut s, e) = (t.line_start(*dl.lines().start()), dl.end());
                    if dl.eof() && s > 0 {
                        s -= 1; // last line: take the newline before it
                    }
                    if s == e {
                        break;
                    }
                    yanked.push_str(&t.rope().byte_slice(s..e).to_string());
                    t.tx(Origin::User).delete(s..e).commit()?; // one transaction per line; one undo step per command
                    let l = t.line_of(s.min(t.len()));
                    let p = t.line_start(l);
                    t.set_cursor(w, p);
                }
                ctx.set_register(
                    a.register.unwrap_or('"'),
                    Register {
                        kind: RegKind::Line,
                        text: yanked,
                    },
                );
                Ok(())
            },
        );
        reg.command("vim-insert", "", |ctx, a| {
            let (b, _) = target(ctx, a);
            ctx.text_mut(b)?.begin_undo_group();
            ctx.set_state(State::Insert);
            Ok(())
        });
        reg.command("vim-self-insert", "", |ctx, a| {
            let s = a.typed().unwrap_or_default();
            insert(ctx, a, &s)
        });
        reg.command("vim-newline", "", |ctx, a| insert(ctx, a, "\n"));
        reg.command("vim-backspace", "", |ctx, a| {
            let (b, w) = target(ctx, a);
            let t = ctx.text_mut(b)?;
            let p = t.cursor(w).unwrap();
            if let Some(c) = prev_char(t, p) {
                t.tx(Origin::User).delete(p - c.len_utf8()..p).commit()?;
            }
            Ok(())
        });
        reg.command("vim-normal", "", |ctx, a| {
            let (b, w) = target(ctx, a);
            let t = ctx.text_mut(b)?;
            t.end_undo_group();
            let p = t.cursor(w).unwrap();
            if p > t.line_start(t.line_of(p)) {
                let q = t.snap(p - 1, Some(Dir::Backward));
                t.set_cursor(w, q);
            }
            ctx.set_state(State::Normal);
            Ok(())
        });
        reg.command("vim-undo", "", |ctx, a| {
            let (b, _) = target(ctx, a);
            for _ in 0..a.count.unwrap_or(1) {
                ctx.text_mut(b)?.undo()?;
            }
            Ok(())
        });
        reg.command("vim-redo", "", |ctx, a| {
            let (b, _) = target(ctx, a);
            ctx.text_mut(b)?.redo().map(|_| ())
        });
        reg.command("vim-digit", "count prefix", |ctx, a| {
            let d = a.char().and_then(|c| c.to_digit(10)).unwrap() as usize;
            ctx.set_prefix(Prefix {
                count: Some(a.count.unwrap_or(0) * 10 + d),
                register: a.register,
            });
            Ok(())
        });
        reg.command("vim-zero", "0: a count digit, or line start", |ctx, a| {
            if let Some(c) = a.count {
                ctx.set_prefix(Prefix {
                    count: Some(c * 10),
                    register: a.register,
                });
                return Ok(());
            }
            let (b, w) = target(ctx, a);
            let t = ctx.text_mut(b)?;
            let p = t.line_start(t.line_of(t.cursor(w).unwrap()));
            t.set_cursor(w, p);
            Ok(())
        });
        reg.ext_command::<Vim>("vim-select-register", "", |v, ctx, a| {
            v.pending = Some(Pending::Register);
            ctx.set_prefix(Prefix {
                count: a.count,
                register: a.register,
            }); // keep the count
            ctx.set_state(State::OperatorPending);
            Ok(())
        });
        reg.ext_command::<Vim>("vim-q", "", |v, ctx, _| {
            if let Some(rec) = v.rec.take() {
                let mut keys = rec.keys.borrow().clone();
                keys.pop(); // the q that stopped recording
                ctx.set_register(
                    rec.reg,
                    Register {
                        kind: RegKind::Char,
                        text: key_notation(&keys),
                    },
                );
            } else {
                v.pending = Some(Pending::Record);
                ctx.set_state(State::OperatorPending);
            }
            Ok(())
        });
        reg.ext_command::<Vim>("vim-at", "", |v, ctx, _| {
            v.pending = Some(Pending::Exec);
            ctx.set_state(State::OperatorPending);
            Ok(())
        });
        reg.ext_command::<Vim>("vim-pending-char", "", |v, ctx, a| {
            let c = a.char().unwrap();
            ctx.set_state(State::Normal);
            match v.pending.take() {
                Some(Pending::Register) => ctx.set_prefix(Prefix {
                    count: a.count,
                    register: Some(c),
                }),
                Some(Pending::Record) => {
                    let keys = Rc::new(RefCell::new(vec![]));
                    let k2 = keys.clone();
                    let sub = ctx.observe_keys(move |ev, src| {
                        if src == KeySource::Typed {
                            k2.borrow_mut().push(ev.clone());
                        }
                    });
                    v.rec = Some(Rec {
                        reg: c,
                        keys,
                        _sub: sub,
                    });
                }
                Some(Pending::Exec) => {
                    let text = ctx.register(c).map(|r| r.text.clone()).unwrap_or_default();
                    ctx.feed_keys(&text)?;
                }
                None => {}
            }
            Ok(())
        });
        reg.ext_command::<Vim>("vim-cancel", "<any> in operator-pending", |v, ctx, _| {
            v.pending = None;
            ctx.set_state(State::Normal);
            Ok(())
        });
        reg.command("vim-search", "/", |ctx, a| {
            let (b, w) = target(ctx, a);
            ctx.prompt("/", move |ctx, answer| {
                let Some(pat) = answer else { return Ok(()) };
                let from = ctx.text(b)?.cursor(w).unwrap();
                let pat = Pattern::new(&pat, false)?;
                let Some(found) = ctx.text(b)?.search(&pat, from, Dir::Forward) else {
                    return Err(Error::Msg("not found".into()));
                };
                let p = found.range.start;
                if !ctx.text(b)?.is_stop(p) {
                    ctx.reveal(b, p)?; // the Hide's owner runs its reveal command
                }
                ctx.text_mut(b)?.set_cursor(w, p);
                Ok(())
            });
            Ok(())
        });
    }
}

fn prev_char(t: &TextBuf, p: usize) -> Option<char> {
    let ci = t.rope().byte_to_char(p);
    (ci > 0).then(|| t.rope().char(ci - 1))
}

fn insert(ctx: &mut Ctx, a: &Args, s: &str) -> Result {
    let (b, w) = target(ctx, a);
    let t = ctx.text_mut(b)?;
    let p = t.cursor(w).unwrap();
    t.tx(Origin::User).insert(p, s).commit()?;
    t.set_cursor(w, p + s.len());
    Ok(())
}

fn horizontal(ctx: &mut Ctx, a: &Args, d: Dir) -> Result {
    let (b, w) = target(ctx, a);
    let t = ctx.text_mut(b)?;
    let mut p = t.cursor(w).unwrap();
    for _ in 0..a.count.unwrap_or(1) {
        let next = match d {
            Dir::Forward => t
                .rope()
                .byte_slice(p..)
                .chars()
                .next()
                .map(|c| p + c.len_utf8()),
            Dir::Backward => prev_char(t, p).map(|c| p - c.len_utf8()),
        };
        if let Some(n) = next {
            p = t.snap(n, Some(d));
        }
    }
    t.set_cursor(w, p);
    Ok(())
}

/// `j`/`k`: by display line (skipping hidden text), keeping the goal display column.
fn vertical(ctx: &mut Ctx, a: &Args, d: Dir) -> Result {
    let (b, w) = target(ctx, a);
    let t = ctx.text_mut(b)?;
    let sel = t.selection(w).unwrap();
    let goal = sel.goal.unwrap_or_else(|| t.display_col(sel.head));
    let mut line = t.line_of(sel.head);
    for _ in 0..a.count.unwrap_or(1) {
        match t.next_visible(line, d) {
            Some(l) => line = l,
            None => break,
        }
    }
    let mut p = t.pos_at_vcol(line, goal);
    // Normal state doesn't rest on the end of a non-empty line.
    let dl = t.display_line(p);
    let stops = dl.stops();
    let n = stops.len();
    if n > 1 && stops[n - 1].pos == p {
        p = stops[n - 2].pos;
    }
    t.set_selection(w, Selection::cursor(p).with_goal(Some(goal)));
    Ok(())
}

fn visual(ctx: &mut Ctx, a: &Args, dy: isize) -> Result {
    let (b, w) = target(ctx, a);
    let t = ctx.text_mut(b)?;
    let n = a.count.unwrap_or(1) as isize;
    let p = t.cursor(w).unwrap();
    let (q, _) = t.move_visual(w, p, dy * n, None).unwrap();
    t.set_cursor(w, q);
    Ok(())
}

// ---------------------------------------------------------------- fold mode (manifold's shape)

#[derive(Default)]
pub struct Fold {
    pub folds: Vec<DecorationId>,
    pub revealed: Vec<DecorationId>,
}
impl Mode for Fold {}

pub struct FoldExt;
impl Extension for FoldExt {
    fn name(&self) -> &'static str {
        "manifold"
    }
    fn register(&mut self, reg: &mut Registry) {
        reg.mode::<Fold>("fold", Fold::default);
        reg.command(
            "fold-reveal",
            "reveal a fold that search landed in",
            |ctx, a| {
                let (b, id) = (a.buffer.unwrap(), a.deco.unwrap());
                ctx.with_mode(b, |f: &mut Fold, ctx| {
                    f.folds.retain(|d| *d != id);
                    f.revealed.push(id);
                    ctx.text_mut(b).map(|t| {
                        t.undecorate(id);
                    })
                })?
            },
        );
    }
}

/// Fold whole lines `first..=last` into one row showing `repl`, as manifold does.
pub fn fold_lines(
    ctx: &mut Ctx,
    b: BufferId,
    first: usize,
    last: usize,
    repl: &str,
) -> DecorationId {
    let t = ctx.text_mut(b).unwrap();
    let (s, e) = (t.line_start(first), t.line_start(last + 1) - 1);
    let id = t
        .decorate(
            s..e,
            Deco::Hide {
                replacement: Some(repl.into()),
                reveal: Reveal::Command("fold-reveal".into()),
            },
        )
        .unwrap();
    ctx.with_mode(b, |f: &mut Fold, _| f.folds.push(id))
        .unwrap();
    id
}

// ---------------------------------------------------------------- toy terminal surface

pub struct TermSurface {
    pub events: Rc<RefCell<Vec<Event>>>,
}
impl Surface for TermSurface {
    fn kind(&self) -> &'static str {
        "term"
    }
    fn render(&mut self, _cols: usize, _rows: usize, out: &mut Vec<String>) {
        out.push("$ ".into());
    }
    fn event(&mut self, ev: &Event, _ctx: &mut Ctx) -> bool {
        self.events.borrow_mut().push(ev.clone());
        true
    }
    fn initial_state(&self) -> State {
        State::Passthrough
    }
}

/// A workspace with the toy die-vim and fold extensions, one text buffer and one window.
pub fn vim_ctx(text: &str, cols: usize, rows: usize) -> (Ctx, BufferId, WindowId) {
    let mut ctx = Ctx::new(vec![Box::new(Vim::default()), Box::new(FoldExt)]).unwrap();
    let b = ctx.create_text("t", text);
    let w = ctx.open_window(b, cols, rows).unwrap();
    (ctx, b, w)
}

pub fn cursor(ctx: &Ctx, b: BufferId, w: WindowId) -> usize {
    ctx.text(b).unwrap().cursor(w).unwrap()
}

pub fn text(ctx: &Ctx, b: BufferId) -> String {
    ctx.text(b).unwrap().rope().to_string()
}

/// (line, byte column) of the cursor.
pub fn lc(ctx: &Ctx, b: BufferId, w: WindowId) -> (usize, usize) {
    let p = ctx.text(b).unwrap().point(cursor(ctx, b, w));
    (p.line, p.col)
}

/// One window composed alone at the frame's top-left: what S4's
/// `render_window` returned, ported to `Ctx::compose` (K8).
pub struct Screen {
    /// The window's text rows, trailing spaces trimmed.
    pub rows: Vec<String>,
    pub cursor: Option<(usize, usize)>,
}

pub fn screen(ctx: &mut Ctx, w: WindowId) -> Screen {
    let (cols, rows) = ctx.window_size(w).unwrap();
    let scene = ctx.compose(
        cols,
        rows + 2,
        &[WindowPlacement {
            window: w,
            col: 0,
            row: 0,
        }],
    );
    Screen {
        rows: (0..rows).map(|r| scene.row_text(r)).collect(),
        // The window's own: a prompt's cursor is on the command row, below it.
        cursor: scene
            .cursors
            .iter()
            .find(|c| c.row < rows)
            .map(|c| (c.row, c.col)),
    }
}
