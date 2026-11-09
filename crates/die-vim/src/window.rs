//! Windows: the `C-w` map and `:sp :vs :new :vne :clo :on :q`, as your Evil's `evil-window-*`
//! commands do them: a new window goes above or to the left and takes the focus, and without a
//! count the split evens out.
use crate::Body;
use crate::grammar::{buffer_window, count_digit, forget_goal};
use die_core::{Args, BufferId, Ctx, Error, Origin, Result, ShowIn, Side, WindowId};

/// Keys after `C-w`, the command, its doc, and what it does. `f` (ffap) and `T` (tabs) aren't here.
#[rustfmt::skip]
pub const MAP: &[(&str, &str, &str, Body)] = &[
    ("h <C-h> <Left> <C-Left>", "vim-window-left", "C-w h: to the count-th window on the left.", |c, a| go(c, a, Side::Left)),
    ("j <C-j> <Down> <C-Down>", "vim-window-down", "C-w j: to the count-th window below.", |c, a| go(c, a, Side::Below)),
    ("k <C-k> <Up> <C-Up>", "vim-window-up", "C-w k: to the count-th window above.", |c, a| go(c, a, Side::Above)),
    ("l <C-l> <Right> <C-Right>", "vim-window-right", "C-w l: to the count-th window on the right.", |c, a| go(c, a, Side::Right)),
    ("w <C-w>", "vim-window-next", "C-w w: to the next window, or the count-th from the top left.", |c, a| next(c, a, 1)),
    ("W <C-S-w>", "vim-window-prev", "C-w W: to the previous window, or the count-th from the top left.", |c, a| next(c, a, -1)),
    ("t <C-t>", "vim-window-top-left", "C-w t: to the top left window.", |c, _| at(c, |l| l.first().copied())),
    ("b <C-b>", "vim-window-bottom-right", "C-w b: to the bottom right window.", |c, _| at(c, |l| l.last().copied())),
    ("p <C-p>", "vim-window-mru", "C-w p: to the window used last that shows another buffer.", mru),
    ("s S <C-s> <C-S-s>", "vim-window-split", "C-w s: a new window above on this buffer, count lines high, or on a file.", |c, a| split(c, a, Side::Above, false)),
    ("v <C-v>", "vim-window-vsplit", "C-w v: a new window on the left on this buffer, count columns wide, or on a file.", |c, a| split(c, a, Side::Left, false)),
    ("n <C-n>", "vim-window-new", "C-w n: a new window above on a new buffer, or on a file.", |c, a| split(c, a, Side::Above, true)),
    ("", "vim-window-vnew", ":vne: a new window on the left on a new buffer, or on a file.", |c, a| split(c, a, Side::Left, true)),
    ("c <C-c>", "vim-window-delete", "C-w c: close this window, and even out its split.", close),
    ("q <C-q>", "vim-quit", "C-w q: close this window, or quit if it's the only one.", quit),
    ("o <C-o>", "vim-window-only", "C-w o: close the other windows.", only),
    ("= <C-=>", "vim-window-balance", "C-w =: even out the frame.", |c, _| { c.balance_frame(); Ok(()) }),
    ("+", "vim-window-taller", "C-w +: count lines taller.", |c, a| size(c, |n, _| (n.0, n.1 + count(a)))),
    ("-", "vim-window-shorter", "C-w -: count lines shorter.", |c, a| size(c, |n, _| (n.0, n.1.saturating_sub(count(a))))),
    (">", "vim-window-wider", "C-w >: count columns wider.", |c, a| size(c, |n, _| (n.0 + count(a), n.1))),
    ("<lt>", "vim-window-narrower", "C-w <: count columns narrower.", |c, a| size(c, |n, _| (n.0.saturating_sub(count(a)), n.1))),
    ("_ <C-_>", "vim-window-set-height", "C-w _: count lines high, or as high as it goes.", |c, a| size(c, |n, f| (n.0, a.count.map_or(f.1, whole)))),
    ("|", "vim-window-set-width", "C-w |: count columns of text wide, or as wide as it goes.", |c, a| size(c, |n, f| (a.count.unwrap_or(f.0), n.1))),
    ("H <C-S-h>", "vim-window-move-far-left", "C-w H: move this window to the frame's left.", |c, a| edge(c, a, Side::Left)),
    ("J <C-S-j>", "vim-window-move-very-bottom", "C-w J: move this window to the frame's bottom.", |c, a| edge(c, a, Side::Below)),
    ("K <C-S-k>", "vim-window-move-very-top", "C-w K: move this window to the frame's top.", |c, a| edge(c, a, Side::Above)),
    ("L <C-S-l>", "vim-window-move-far-right", "C-w L: move this window to the frame's right.", |c, a| edge(c, a, Side::Right)),
    ("r <C-r>", "vim-window-rotate-downwards", "C-w r: each window's content moves to the next window.", |c, a| rotate(c, a, true)),
    ("R <C-S-r>", "vim-window-rotate-upwards", "C-w R: each window's content moves to the window before.", |c, a| rotate(c, a, false)),
    ("x <C-x>", "vim-window-exchange", "C-w x: change places with the next window, or the count-th.", exchange),
    (":", "vim-window-ex", "C-w :: read a line and run it.", crate::ex::open),
    ("0 1 2 3 4 5 6 7 8 9", "vim-window-digit-argument", "C-w 5: a count digit, and the C-w map stays.", digit),
];

/// Ex names, as `sp[lit]` gives its shortest form and its whole name, and what they run.
pub(crate) const EX: &[(&str, &str, &str)] = &[
    ("sp", "split", "vim-window-split"),
    ("vs", "vsplit", "vim-window-vsplit"),
    ("new", "new", "vim-window-new"),
    ("vne", "vnew", "vim-window-vnew"),
    ("clo", "close", "vim-window-delete"),
    ("on", "only", "vim-window-only"),
];

/// Whether `name` is a window command's, with a `!` after it or not.
pub fn named(name: &str) -> bool {
    let name = name.strip_suffix('!').unwrap_or(name);
    EX.iter().any(|e| crate::ex::abbreviates(name, (e.0, e.1)))
}

/// An ex line, if it's a window command: an optional count, the name, and a file. `!` changes
/// nothing here. `:q` is with the file commands, where its `!` means something.
pub fn ex(ctx: &mut Ctx, line: &str) -> Option<Result> {
    let digits = line.find(|c: char| !c.is_ascii_digit())?;
    let (n, rest) = line.split_at(digits);
    let (name, file) = rest.split_once(' ').unwrap_or((rest, ""));
    let name = name.strip_suffix('!').unwrap_or(name);
    let is = |e: &&(&str, &str, &str)| crate::ex::abbreviates(name, (e.0, e.1));
    let &(.., command) = EX.iter().find(is)?;
    let args = Args {
        count: n.parse().ok(),
        text: (!file.trim().is_empty()).then(|| file.trim().to_string()),
        ..Default::default()
    };
    Some(ctx.run(command, &args))
}

/// Any window command forgets the goal column in the window it starts from, as Emacs does for
/// anything but a vertical motion.
pub fn run(ctx: &mut Ctx, a: &Args, f: Body) -> Result {
    let (b, w) = buffer_window(ctx, a)?;
    if let Ok(t) = ctx.text_mut(b) {
        forget_goal(t, w);
    }
    f(ctx, a)
}

fn count(a: &Args) -> usize {
    a.count.unwrap_or(1)
}

/// A count is a whole window's cells, with its status line or divider: the text has one less.
fn whole(n: usize) -> usize {
    n.saturating_sub(1)
}

fn go(ctx: &mut Ctx, a: &Args, side: Side) -> Result {
    let way = match side {
        Side::Left => "left",
        Side::Right => "right",
        Side::Above => "up",
        Side::Below => "down",
    };
    for _ in 0..count(a) {
        ctx.focus_direction(side)
            .ok_or_else(|| Error::Msg(format!("No window {way} from selected window")))?;
    }
    Ok(())
}

/// The window `step` from this one in the cycle, or with a count the count-th.
fn target(ctx: &Ctx, a: &Args, step: isize) -> Result<WindowId> {
    let (_, w) = buffer_window(ctx, a)?;
    let all = ctx.window_list();
    let len = all.len();
    if len == 0 {
        return Ok(w);
    }
    Ok(match a.count {
        Some(n) => all[n.clamp(1, len) - 1],
        None => {
            let i = all.iter().position(|o| *o == w).unwrap_or(0);
            all[(i as isize + step).rem_euclid(len as isize) as usize]
        }
    })
}

fn next(ctx: &mut Ctx, a: &Args, step: isize) -> Result {
    let to = target(ctx, a, step)?;
    ctx.focus(to);
    Ok(())
}

fn at(ctx: &mut Ctx, pick: impl Fn(&[WindowId]) -> Option<WindowId>) -> Result {
    if let Some(w) = pick(&ctx.window_list()) {
        ctx.focus(w);
    }
    Ok(())
}

fn mru(ctx: &mut Ctx, a: &Args) -> Result {
    let (b, _) = buffer_window(ctx, a)?;
    let to = ctx.windows_by_use();
    if let Some(w) = to.into_iter().find(|w| ctx.window_buffer(*w) != Some(b)) {
        ctx.focus(w);
    }
    Ok(())
}

/// Split, take the focus, and show a file or a new buffer if asked for. A count sizes the new
/// window, and without one the split evens out.
fn split(ctx: &mut Ctx, a: &Args, side: Side, blank: bool) -> Result {
    let (b, w) = buffer_window(ctx, a)?;
    let new = ctx.split_window(w, side)?;
    crate::marks::copy_jumps(ctx, w, new);
    match a.count {
        Some(n) => {
            let (cols, rows) = ctx.window_size(new).unwrap_or((1, 1));
            let size = match side {
                Side::Left | Side::Right => (whole(n), rows),
                Side::Above | Side::Below => (cols, whole(n)),
            };
            ctx.resize(new, size.0, size.1)?;
        }
        None => ctx.balance_windows(new),
    }
    ctx.focus(new);
    let shown = match a.text.as_deref() {
        Some(file) => {
            let path = crate::ex::absolute(ctx.directory(b), file);
            Some(ctx.open_file(&path)?)
        }
        None => blank.then(|| ctx.create_text("*new*", "")),
    };
    if let Some(buf) = shown {
        ctx.show(buf, ShowIn::Current)?;
    }
    Ok(())
}

fn close(ctx: &mut Ctx, a: &Args) -> Result {
    let (_, w) = buffer_window(ctx, a)?;
    ctx.close_window_balancing(w)
}

/// `C-w q`, as `:q`.
fn quit(ctx: &mut Ctx, a: &Args) -> Result {
    let (_, w) = buffer_window(ctx, a)?;
    close_or_quit(ctx, w, false)
}

/// `C-^`: the buffer used most recently that no window shows, or else the most recent other.
pub fn alternate(ctx: &mut Ctx, a: &Args) -> Result {
    let (here, _) = buffer_window(ctx, a)?;
    let shown: Vec<_> = ctx
        .window_list()
        .into_iter()
        .filter_map(|w| ctx.window_buffer(w))
        .collect();
    let others: Vec<_> = ctx
        .buffer_list()
        .into_iter()
        .filter(|b| *b != here)
        .collect();
    let unseen = others.iter().find(|b| !shown.contains(b));
    let Some(&other) = unseen.or(others.first()) else {
        return Err(Error::Msg("No other buffer".into()));
    };
    ctx.show(other, ShowIn::Current)?;
    Ok(())
}

/// `:bn` and `:bp`: the next file's buffer in the order they opened, round to the first. Your
/// Evil's follow the window's own history, which core doesn't keep.
pub fn buffer_step(ctx: &mut Ctx, forward: bool) -> Result {
    let (here, _) = buffer_window(ctx, &Args::default())?;
    let mut files: Vec<_> = ctx
        .buffer_list()
        .into_iter()
        .filter(|b| ctx.file(*b).is_some())
        .collect();
    files.sort();
    let after = files.iter().position(|b| *b == here);
    let n = files.len();
    let to = match after {
        _ if n == 0 => return Err(Error::Msg("No other buffer".into())),
        Some(i) if forward => (i + 1) % n,
        Some(i) => (i + n - 1) % n,
        None if forward => 0,
        None => n - 1,
    };
    ctx.show(files[to], ShowIn::Current)?;
    Ok(())
}

/// `:bd`, as your Evil's: kill this buffer and close the windows that showed it. Unsaved text in
/// a file is refused, unless `force`: Emacs asks, and `:w` or `:bd!` is that answer here.
pub fn delete_buffer(ctx: &mut Ctx, force: bool) -> Result {
    let (b, _) = buffer_window(ctx, &Args::default())?;
    if !force && ctx.file(b).is_some() && ctx.text(b).is_ok_and(|t| t.modified()) {
        let name = ctx.buffer_name(b).unwrap_or_default();
        let why = format!("{name} has unsaved changes; :w saves them, :bd! drops them");
        return Err(Error::Msg(why));
    }
    let list = ctx.window_list();
    let showing: Vec<_> = list
        .into_iter()
        .filter(|w| ctx.window_buffer(*w) == Some(b))
        .collect();
    ctx.kill(b);
    for w in showing {
        // The last window stays, on the buffer that took over.
        if ctx.window_list().len() > 1 {
            ctx.close_window_balancing(w)?;
        }
    }
    Ok(())
}

/// `:q`: Evil's `evil-quit` runs `evil-window-delete`, as `C-w c` does, and only the last window
/// quits. That refuses while a file has unsaved changes, unless `force`. A buffer a `die --wait`
/// client waits on is done instead, as `server-edit` takes it, with the same refusal, or with
/// `force` as `server-buffer-done` does. Your Evil does that only in the last window, and closes
/// a split, which would leave `git commit` waiting.
pub fn close_or_quit(ctx: &mut Ctx, w: WindowId, force: bool) -> Result {
    if let Some(b) = ctx.window_buffer(w).filter(|b| ctx.is_client_buffer(*b)) {
        if !force && unsaved(ctx, b) {
            return Err(refusal(ctx, b));
        }
        return ctx.client_done(b, force);
    }
    if ctx.window_list().len() > 1 {
        return ctx.close_window_balancing(w);
    }
    quit_app(ctx, force)
}

/// `:cq` on a file a `die --wait` client waits on gives it up, so the client fails, as Emacs's
/// `server-edit-abort` has it, and `git commit` or `git rebase -i` stops. Your Evil's `:cq`
/// refuses there and quits Emacs elsewhere; here it refuses elsewhere.
pub fn fail_client(ctx: &mut Ctx) -> Result {
    let (b, _) = buffer_window(ctx, &Args::default())?;
    if !ctx.is_client_buffer(b) {
        return Err(Error::Msg("This buffer has no clients".into()));
    }
    ctx.client_failed(b)
}

fn unsaved(ctx: &Ctx, b: BufferId) -> bool {
    ctx.file(b).is_some() && ctx.text(b).is_ok_and(|t| t.modified())
}

fn refusal(ctx: &Ctx, b: BufferId) -> Error {
    let name = ctx.buffer_name(b).unwrap_or_default();
    Error::Msg(format!(
        "{name} has unsaved changes; :w saves them, :q! drops them"
    ))
}

/// Quit, which refuses while a file has unsaved changes. `force` drops them instead. Then the
/// config's question, if it has one ([`crate::Vim::asking_before_quit`]), whatever `force` is,
/// since `!` speaks for unsaved text and not for a program running in a terminal.
pub fn quit_app(ctx: &mut Ctx, force: bool) -> Result {
    let ask = ctx.with_ext(|v: &mut crate::Vim, _| v.ask_before_quit)?;
    let refused = !force && ctx.buffer_list().into_iter().any(|b| unsaved(ctx, b));
    match ask.filter(|_| !refused) {
        Some(ask) => ask(ctx, Box::new(move |ctx| quit_now(ctx, force))),
        None => quit_now(ctx, force), // which refuses unsaved text before anything's asked
    }
}

fn quit_now(ctx: &mut Ctx, force: bool) -> Result {
    let all = ctx.buffer_list().into_iter();
    let unsaved: Vec<BufferId> = all.filter(|b| unsaved(ctx, *b)).collect();
    if let Some(b) = unsaved.first().filter(|_| !force) {
        return Err(refusal(ctx, *b));
    }
    for b in unsaved {
        drop_changes(ctx, b)?;
    }
    ctx.quit();
    Ok(())
}

/// So the session at quit doesn't bring them back. It keeps the text of a buffer marked modified
/// and of a file that isn't there. The disk has the rest.
fn drop_changes(ctx: &mut Ctx, b: BufferId) -> Result {
    let gone = ctx.file(b).is_none_or(|f| f.fingerprint.is_none());
    let t = ctx.text_mut(b)?;
    if gone {
        let all = 0..t.len();
        t.tx(Origin::Extension).replace(all, "").commit()?;
    }
    t.set_modified(false);
    Ok(())
}

fn only(ctx: &mut Ctx, a: &Args) -> Result {
    let (_, w) = buffer_window(ctx, a)?;
    ctx.only_window(w)
}

/// Give this window a new size, from its size and the frame's. A sole window fills the frame.
fn size(ctx: &mut Ctx, new: impl Fn((usize, usize), (usize, usize)) -> (usize, usize)) -> Result {
    let Some(w) = ctx.focused() else {
        return Ok(());
    };
    if ctx.window_list().len() < 2 {
        return Ok(());
    }
    let had = ctx.window_size(w).unwrap_or((1, 1));
    let (cols, rows) = new(had, ctx.frame_size());
    ctx.resize(w, cols, rows)
}

fn edge(ctx: &mut Ctx, a: &Args, side: Side) -> Result {
    let (_, w) = buffer_window(ctx, a)?;
    if ctx.window_list().len() > 1 {
        ctx.move_window(w, side)?;
    }
    Ok(())
}

/// Each window's content moves on one place in the cycle, and the focus keeps its place.
fn rotate(ctx: &mut Ctx, a: &Args, down: bool) -> Result {
    let (_, w) = buffer_window(ctx, a)?;
    let all = ctx.window_list();
    let Some(last) = all.len().checked_sub(1) else {
        return Ok(());
    };
    let (moved, mut others) = if down {
        (all[last], all[..last].to_vec())
    } else {
        (all[0], all[1..].to_vec())
    };
    if down {
        others.reverse();
    }
    for o in others {
        ctx.swap_windows(moved, o)?;
    }
    if let Some(i) = all.iter().position(|o| *o == w) {
        ctx.focus(ctx.window_list()[i]);
    }
    Ok(())
}

/// `C-w x`: as Evil's `window-swap-states` does, the focus keeps its place on the screen, so it's
/// on the other window now.
fn exchange(ctx: &mut Ctx, a: &Args) -> Result {
    let (_, w) = buffer_window(ctx, a)?;
    let to = target(ctx, a, 1)?;
    if to != w {
        ctx.exchange_windows(w, to)?;
        ctx.focus(to);
    }
    Ok(())
}

/// A digit after `C-w`: Evil's `evil-window-digit-argument`. The count waits in the prefix, and
/// the next key is looked up after `C-w` again. A recording holds typed keys, so not this one.
fn digit(ctx: &mut Ctx, a: &Args) -> Result {
    let d = a.char().and_then(|c| c.to_digit(10)).unwrap_or(0);
    count_digit(ctx, a, d as usize)?;
    ctx.feed_keys("<C-w>")
}
