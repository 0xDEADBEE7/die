//! `C-p` and `C-n`: Emacs's `dabbrev-expand`, as your Evil calls it. The word before the cursor
//! expands to the nearest word it starts, looking back, then on, then in other buffers, ignoring
//! case. Pressed again, the next one distinct from those tried takes its place. A space typed
//! after an expansion and another press brings in the word that follows the last one found.
//!
//! The case of what goes in follows Emacs's `dabbrev--substitute-expansion` and `replace-match`.

use crate::Vim;
use crate::grammar::buffer_window;
use crate::insert::{self, Session};
use crate::text::{Class, class};
use die_core::{Args, BufferId, Ctx, Error, Result};

/// What Emacs's `dabbrev--last-*` variables keep between presses.
#[derive(Default)]
pub struct Dabbrev {
    buffer: Option<BufferId>,
    /// Where the last expansion ended.
    end: Option<usize>,
    abbrev: String,
    expansion: String,
    /// How long the last expansion is in the text.
    inserted: usize,
    /// 1 back, 0 back and then on, -1 on.
    direction: isize,
    /// Where the search goes on from.
    from: Option<usize>,
    /// The words already found.
    seen: Vec<String>,
    /// The other buffer being searched.
    source: Option<BufferId>,
    /// The other buffers still to search, the next last.
    others: Option<Vec<BufferId>>,
    /// Words copied after an expansion are put in this case: true for upper, false for lower.
    case: Option<bool>,
    /// The command count at the last press, and whether it was `C-n`.
    last: Option<(u64, bool)>,
}

impl Dabbrev {
    fn reset(&mut self) {
        let (case, last) = (self.case, self.last);
        *self = Dabbrev {
            case,
            last,
            ..Dabbrev::default()
        };
    }
}

/// Syntax word or symbol in Emacs's standard table, which is what an abbreviation is made of.
fn abbrev_char(c: char) -> bool {
    class(c, false) == Class::Word
        || matches!(c, '_' | '-' | '+' | '*' | '/' | '&' | '|' | '<' | '>' | '=')
}

/// The start of the run of abbreviation characters that ends at `p`.
fn run_start(s: &str, p: usize) -> usize {
    let tail = s[..p].char_indices().rev();
    let mut start = p;
    for (i, c) in tail {
        if !abbrev_char(c) {
            break;
        }
        start = i;
    }
    start
}

/// The abbreviation at the cursor: from the start of the word before it, up to it. That takes
/// in what's between that word and the cursor, even a line's end.
fn abbrev_at(s: &str, p: usize) -> Result<String> {
    let none = || Error::Msg("No possible abbreviation preceding point".into());
    if p == 0 {
        return Err(none());
    }
    let (i, c) = s[..p]
        .char_indices()
        .rev()
        .find(|&(_, c)| abbrev_char(c))
        .ok_or_else(none)?;
    Ok(s[run_start(s, i + c.len_utf8())..p].to_owned())
}

/// If the text at `i` starts with `abbrev`, ignoring case, where it ends.
fn prefix_end(s: &str, i: usize, abbrev: &str) -> Option<usize> {
    let mut at = s[i..].char_indices();
    let mut end = i;
    for want in abbrev.chars() {
        let (j, c) = at.next()?;
        if c != want && !c.to_lowercase().eq(want.to_lowercase()) {
            return None;
        }
        end = i + j + c.len_utf8();
    }
    Some(end)
}

/// Where the word starting with `abbrev` at `i` ends, with at least one more character.
fn word_at(s: &str, i: usize, abbrev: &str) -> Option<usize> {
    let end = prefix_end(s, i, abbrev)?;
    let more: usize = s[end..]
        .chars()
        .take_while(|&c| abbrev_char(c))
        .map(char::len_utf8)
        .sum();
    (more > 0).then_some(end + more)
}

/// The next distinct word starting with `abbrev`, as Emacs's `dabbrev--search`: found, and where
/// to go on from. Looking back, it has to lie before `from`. A match inside a longer word only
/// counts if the abbreviation also starts that word.
fn search(
    s: &str,
    from: usize,
    abbrev: &str,
    reverse: bool,
    seen: &mut Vec<String>,
) -> Option<(String, usize)> {
    let mut at = from;
    loop {
        let (m, step) = if reverse {
            let m = s[..at].char_indices().rev().map(|(i, _)| i).find(|&i| {
                prefix_end(s, i, abbrev).is_some_and(|e| {
                    s[e..]
                        .chars()
                        .next()
                        .is_some_and(|c| abbrev_char(c) && e + c.len_utf8() <= at)
                })
            })?;
            (m, m)
        } else {
            let m = s[at..]
                .char_indices()
                .map(|(i, _)| at + i)
                .find(|&i| word_at(s, i, abbrev).is_some())?;
            (m, m + s[m..].chars().next().map_or(1, char::len_utf8))
        };
        let start = run_start(s, m);
        at = match word_at(s, start, abbrev) {
            Some(end) => {
                let next = if reverse { start } else { end };
                let found = &s[start..end];
                if !seen.iter().any(|x| x == found) {
                    seen.push(found.to_owned());
                    return Some((found.to_owned(), next));
                }
                next
            }
            None => step,
        };
    }
}

impl Dabbrev {
    /// Search `n` times in one text, as Emacs's `dabbrev--try-find`.
    fn try_find(
        &mut self,
        s: &str,
        p: usize,
        abbrev: &str,
        reverse: bool,
        n: usize,
    ) -> Option<String> {
        let mut at = self.from.unwrap_or(p);
        if !s.is_char_boundary(at) {
            return None;
        }
        let mut found = None;
        for _ in 0..n {
            let (word, next) = search(s, at, abbrev, reverse, &mut self.seen)?;
            found = Some(word);
            at = next;
        }
        self.from = Some(at);
        found
    }

    /// Emacs's `dabbrev--find-expansion`.
    fn find(
        &mut self,
        ctx: &Ctx,
        b: BufferId,
        p: usize,
        abbrev: &str,
        direction: isize,
    ) -> Option<String> {
        let here = self.source.filter(|&o| ctx.text(o).is_ok()).unwrap_or(b);
        let s = ctx.text(here).ok()?.rope().to_string();
        if direction >= 0 {
            self.direction = direction.min(1);
            let n = direction.max(1) as usize;
            if let Some(x) = self.try_find(&s, p, abbrev, true, n) {
                return Some(x);
            }
        }
        if direction <= 0 {
            self.direction = -1;
            let n = direction.unsigned_abs().max(1);
            if let Some(x) = self.try_find(&s, p, abbrev, false, n) {
                return Some(x);
            }
        }
        self.direction = -1;
        if self.source.is_none() && self.others.is_none() {
            self.others = Some(other_buffers(ctx, b));
        }
        while let Some(o) = self.others.as_mut()?.pop() {
            self.source = Some(o);
            self.from = Some(0);
            let s = ctx.text(o).ok()?.rope().to_string();
            if let Some(x) = self.try_find(&s, 0, abbrev, false, 1) {
                return Some(x);
            }
        }
        None
    }

    /// `dabbrev-expand`, and whether it's the press after a press of this key.
    fn expand(
        &mut self,
        ctx: &mut Ctx,
        a: &Args,
        s: &mut Session,
        arg: Option<isize>,
        again: bool,
    ) -> Result {
        let (b, w) = buffer_window(ctx, a)?;
        if self.buffer != Some(b) {
            self.reset();
            self.buffer = Some(b);
        }
        let t = ctx.text(b)?;
        let p = t.cursor(w).ok_or(Error::Msg("no cursor".into()))?;
        let text = t.rope().to_string();
        let mut old = None;
        let (mut word, mut record) = (None, false);
        let abbrev;
        let direction;
        if arg.is_none() && again && self.end.is_some() {
            abbrev = self.abbrev.clone();
            old = Some(self.inserted);
            direction = self.direction;
        } else if p >= 1 && text.as_bytes()[p - 1] == b' ' && self.end == Some(p - 1) {
            word = Some(self.next_word(ctx, b, &text)?);
            abbrev = " ".to_owned();
            direction = -1;
        } else {
            self.reset();
            self.buffer = Some(b);
            direction = arg.unwrap_or(0);
            abbrev = abbrev_at(&text, p)?;
            record = true;
        }
        let found = word.or_else(|| self.find(ctx, b, p, &abbrev, direction));
        let Some(found) = found else {
            let further = if old.is_some() { " further" } else { "" };
            self.reset();
            self.buffer = Some(b);
            if let Some(len) = old {
                insert::swap(ctx, a, s, p - len, &abbrev)?;
            }
            return Err(Error::Msg(format!(
                "No{further} dynamic expansion for ‘{abbrev}’ found"
            )));
        };
        let beg = p - old.unwrap_or(abbrev.len());
        let (new, kept) = self.cased(&abbrev, &text[beg..p], found, record);
        let at = t_len(ctx, b)?;
        insert::swap(ctx, a, s, beg, &new)?;
        let by = t_len(ctx, b)? as isize - at as isize;
        if self.source.is_none() {
            self.from = self.from.map(|f| {
                if f > p {
                    f.saturating_add_signed(by)
                } else {
                    f
                }
            });
        }
        (self.expansion, self.inserted) = (kept, new.len());
        self.abbrev = abbrev;
        self.end = Some(beg + new.len());
        Ok(())
    }

    /// A space after an expansion: what follows the word that was found, up to the end of the
    /// next word.
    fn next_word(&mut self, ctx: &Ctx, b: BufferId, text: &str) -> Result<String> {
        let other = self
            .source
            .filter(|&o| ctx.text(o).is_ok())
            .filter(|&o| o != b);
        let owned;
        let s = match other {
            Some(o) => {
                owned = ctx.text(o)?.rope().to_string();
                owned.as_str()
            }
            None => text,
        };
        let mut from = self.from.unwrap_or(0);
        if self.direction == 1 || (self.direction == 0 && from < self.end.unwrap_or(0)) {
            from += self.expansion.len();
        }
        let failed = || Error::Msg("Search failed".into());
        let tail = s.get(from..).ok_or_else(failed)?;
        let (i, _) = tail
            .char_indices()
            .find(|&(_, c)| abbrev_char(c))
            .ok_or_else(failed)?;
        let run: usize = tail[i..]
            .chars()
            .take_while(|&c| abbrev_char(c))
            .map(char::len_utf8)
            .sum();
        let end = from + i + run;
        let word = s[from..end].to_owned();
        (self.from, self.direction) = (Some(end), -1);
        Ok(word)
    }

    /// The text to put in for `found`, and the expansion to remember, as Emacs's
    /// `dabbrev--substitute-expansion`. `over` is what it replaces.
    fn cased(&mut self, abbrev: &str, over: &str, found: String, record: bool) -> (String, String) {
        let lower = |x: &str| x.to_lowercase();
        let upper = |x: &str| x.to_uppercase();
        let mut found = found;
        let mut fold = abbrev != " ";
        if !fold && let Some(up) = self.case {
            found = if up { upper(&found) } else { lower(&found) };
        }
        let rest: String = found.chars().skip(1).collect();
        let first = abbrev.chars().position(char::is_alphabetic);
        let chars = abbrev.chars().count();
        let plain_rest = rest == lower(&rest) || rest == upper(&rest);
        let plain_abbrev = abbrev == lower(abbrev)
            || (abbrev == upper(abbrev) && first.is_some_and(|f| chars - f > 1));
        let same_start = found.chars().take(chars).eq(abbrev.chars());
        if first.is_none() || (!(plain_rest && plain_abbrev) && same_start) {
            fold = false;
        }
        if found == lower(&found) && abbrev == lower(abbrev) {
            fold = false;
        }
        if fold {
            found = lower(&found);
        }
        if record {
            self.case = None;
            if fold {
                self.case = if abbrev == upper(abbrev) {
                    Some(true)
                } else if abbrev == lower(abbrev) {
                    Some(false)
                } else {
                    None
                };
            }
        }
        let kept = spaced(&found, if abbrev == " " { 0 } else { abbrev.len() });
        let new = if fold {
            like(over, &kept)
        } else {
            kept.clone()
        };
        (new, kept)
    }
}

/// Runs of whitespace after `from` become one space, as `dabbrev-eliminate-newlines` has it.
fn spaced(s: &str, from: usize) -> String {
    let from = from.min(s.len());
    let mut out = s[..from].to_owned();
    let mut gap = false;
    for c in s[from..].chars() {
        if matches!(c, ' ' | '\n' | '\t') {
            if !gap {
                out.push(' ');
            }
            gap = true;
        } else {
            out.push(c);
            gap = false;
        }
    }
    out
}

/// `new` in the case of `old`, as Emacs's `replace-match` without its FIXEDCASE.
fn like(old: &str, new: &str) -> String {
    let word = |c: char| class(c, false) == Class::Word;
    let (mut lowers, mut uppers, mut multi, mut other_initial) = (false, false, false, false);
    let mut prev = '\n';
    for c in old.chars() {
        if c.is_lowercase() {
            lowers = true;
            if word(prev) {
                multi = true;
            } else {
                other_initial = true;
            }
        } else if c.is_uppercase() {
            uppers = true;
            multi |= word(prev);
        } else if !word(prev) {
            other_initial = true;
        }
        prev = c;
    }
    if (!lowers && multi) || (!other_initial && !multi && uppers) {
        return new.to_uppercase();
    }
    if other_initial || !multi {
        return new.to_owned();
    }
    let mut out = String::new();
    let mut prev = '\n';
    for c in new.chars() {
        if word(prev) {
            out.push(c);
        } else {
            out.extend(c.to_uppercase());
        }
        prev = c;
    }
    out
}

fn t_len(ctx: &Ctx, b: BufferId) -> Result<usize> {
    Ok(ctx.text(b)?.len())
}

/// The buffers to search after this one, those shown first, as a stack. Emacs passes over
/// `*Messages*`, `*Buffer List*` and names that start with a space. Anything but text has
/// no words.
fn other_buffers(ctx: &Ctx, b: BufferId) -> Vec<BufferId> {
    let shown = ctx
        .windows_by_use()
        .into_iter()
        .filter_map(|w| ctx.window_buffer(w));
    let mut out: Vec<BufferId> = vec![];
    for o in shown.chain(ctx.buffer_list()) {
        let name = ctx.buffer_name(o).unwrap_or_default();
        let skip = name.starts_with(' ') || matches!(name, "*Messages*" | "*Buffer List*");
        if o != b && !skip && ctx.text(o).is_ok() && !out.contains(&o) {
            out.push(o);
        }
    }
    out.reverse();
    out
}

fn with_state(
    ctx: &mut Ctx,
    a: &Args,
    next: bool,
    f: impl FnOnce(&mut Dabbrev, &mut Ctx, &mut Session, bool) -> Result,
) -> Result {
    let (b, _) = buffer_window(ctx, a)?;
    let (mut d, s, seq) = ctx.with_ext(|v: &mut Vim, _| {
        (
            std::mem::take(&mut v.dabbrev),
            v.inserts.remove(&b),
            v.commands,
        )
    })?;
    let mut s = s.unwrap_or_else(Session::bare);
    let again = d.last == Some((seq.wrapping_sub(1), next));
    d.last = Some((seq, next));
    let r = f(&mut d, ctx, &mut s, again);
    ctx.with_ext(|v: &mut Vim, _| {
        v.dabbrev = d;
        v.inserts.insert(b, s);
    })?;
    r
}

/// `C-p`: Evil's `evil-complete-previous`.
pub fn previous(ctx: &mut Ctx, a: &Args) -> Result {
    with_state(ctx, a, false, |d, ctx, s, again| {
        d.expand(ctx, a, s, a.count.map(|n| n as isize), again)
    })
}

/// `C-n`: Evil's `evil-complete-next`. It looks on first, and whatever goes wrong it tries the
/// plain expansion instead, so pressing it goes round. A `C-u` with no digits is an error
/// there too, in your Evil, so it's that plain expansion.
pub fn next(ctx: &mut Ctx, a: &Args) -> Result {
    let bare = a.count.is_some()
        && ctx.with_ext(|v: &mut Vim, _| v.count_was == crate::grammar::CountWas::Universal)?;
    with_state(ctx, a, true, |d, ctx, s, again| {
        let first = match again || bare {
            true => d.expand(ctx, a, s, None, again),
            false => {
                let n = a.count.unwrap_or(1) as isize;
                d.expand(ctx, a, s, Some(-n), again)
            }
        };
        first.or_else(|_| d.expand(ctx, a, s, None, again))
    })
}
