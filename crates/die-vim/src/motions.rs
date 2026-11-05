//! Motions (scope §10): plain functions over the text and layout queries. Each
//! follows your Evil, except where deviations.json says otherwise.

use crate::grammar::{Fail, Goal, Kind, Motion, Operator, Start, Target};
use crate::text::{
    Chars, Class, Lists, blank_line, class, indent_end, last_line, line_end, next_stop, normal_pos,
    pos_at_col, prev_stop, same_word, string_around,
};
use die_core::{Dir, TextBuf, WindowId};

/// Every motion bound in normal state. `f`, `F`, `t` and `T` take a character
/// and are bound with `<char>`.
#[rustfmt::skip]
pub const MOTIONS: &[Motion] = &[
    m("vim-backward-char", "h", Kind::Exclusive, "Left, within the line.", backward_char),
    m("vim-forward-char", "l", Kind::Exclusive, "Right, within the line.", forward_char),
    m("vim-next-line", "j", Kind::Linewise, "Down a line, keeping the column.", next_line),
    m("vim-previous-line", "k", Kind::Linewise, "Up a line, keeping the column.", previous_line),
    m("vim-forward-word-begin", "w", Kind::Exclusive, "To the next word's start.", |s| words(s, Word::Begin, false)),
    m("vim-forward-WORD-begin", "W", Kind::Exclusive, "To the next WORD's start.", |s| words(s, Word::Begin, true)),
    m("vim-forward-word-end", "e", Kind::Inclusive, "To the next word's end.", |s| words(s, Word::End, false)),
    m("vim-forward-WORD-end", "E", Kind::Inclusive, "To the next WORD's end.", |s| words(s, Word::End, true)),
    m("vim-backward-word-begin", "b", Kind::Exclusive, "To the previous word's start.", |s| words(s, Word::Back, false)),
    m("vim-backward-WORD-begin", "B", Kind::Exclusive, "To the previous WORD's start.", |s| words(s, Word::Back, true)),
    m("vim-first-non-blank", "^", Kind::Exclusive, "To the line's first non-blank.", first_non_blank),
    m("vim-end-of-line", "$", Kind::Inclusive, "To the end of the line, or of count-1 lines down.", end_of_line),
    m("vim-goto-first-line", "gg", Kind::Linewise, "To the first line, or line count.", |s| goto_line(s, s.count.unwrap_or(1))),
    m("vim-goto-line", "G", Kind::Linewise, "To the last line, or line count.", |s| goto_line(s, s.count.unwrap_or(usize::MAX))),
    m("vim-find-char", "f<char>", Kind::Inclusive, "To the next char on the line.", |s| find(s, Find::new('f', s.arg))),
    m("vim-find-char-backward", "F<char>", Kind::Exclusive, "To the previous char on the line.", |s| find(s, Find::new('F', s.arg))),
    m("vim-find-char-to", "t<char>", Kind::Inclusive, "To just before the next char on the line.", |s| find(s, Find::new('t', s.arg))),
    m("vim-find-char-to-backward", "T<char>", Kind::Exclusive, "To just after the previous char on the line.", |s| find(s, Find::new('T', s.arg))),
    m("vim-jump-item", "%", Kind::Inclusive, "To the bracket matching the next one on the line, or count percent down the text.", jump_item),
    m("vim-forward-paragraph", "}", Kind::Exclusive, "To the next blank line after this paragraph.", forward_paragraph),
    m("vim-backward-paragraph", "{", Kind::Exclusive, "To the previous blank line before this paragraph.", backward_paragraph),
];

const fn m(
    name: &'static str,
    keys: &'static str,
    kind: Kind,
    doc: &'static str,
    run: fn(&Start) -> Result<Target, Fail>,
) -> Motion {
    Motion {
        name,
        keys,
        kind,
        doc,
        run,
    }
}

/// `0`, bound apart because it's also a count digit.
pub fn beginning_of_line(s: &Start) -> Result<Target, Fail> {
    Ok(Target::at(s.text.line_start(s.text.line_of(s.pos))))
}

/// `C-a`, Emacs's beginning-of-visual-line under your global-visual-line-mode: to the
/// start of the screen row, count-1 rows down. `move_visual` with goal 0 gives a row's
/// start, but only for another row, so this row's start comes from the row above.
pub fn row_beginning(s: &Start, w: WindowId) -> Result<Target, Fail> {
    let t = s.text;
    let here = t.line_of(s.pos);
    let row = |pos, dy| t.move_visual(w, pos, dy, Some(0)).map_or(pos, |(q, _)| q);
    let above = row(s.pos, -1);
    let mut q = if above == s.pos {
        t.line_start(here)
    } else {
        row(above, 1)
    };
    if s.n() > 1 {
        q = row(q, isize::try_from(s.n() - 1).unwrap_or(isize::MAX));
    }
    if t.line_of(q) > last_line(t).max(here) {
        q = row(q, -1);
    }
    Ok(Target::at(q))
}

// ---------------------------------------------------------------- characters and lines

pub fn backward_char(s: &Start) -> Result<Target, Fail> {
    let start = s.text.line_start(s.text.line_of(s.pos));
    steps(s, "Beginning of line", |p| {
        prev_stop(s.text, p).filter(|&q| q >= start)
    })
}

pub fn forward_char(s: &Start) -> Result<Target, Fail> {
    let end = line_end(s.text, s.text.line_of(s.pos));
    // For an operator `l` can land on the newline, so `dl` takes a line's last character.
    let room = if s.op.is_some() { end + 1 } else { end };
    steps(s, "End of line", |p| {
        next_stop(s.text, p).filter(|&q| q < room)
    })
}

/// Up to `count` steps. Evil moves as far as it can and signals when it fell short.
fn steps(s: &Start, err: Fail, step: impl Fn(usize) -> Option<usize>) -> Result<Target, Fail> {
    let (mut p, mut n) = (s.pos, 0);
    while n < s.n() {
        let Some(q) = step(p) else { break };
        (p, n) = (q, n + 1);
    }
    Ok(if n < s.n() {
        Target::short(p, err)
    } else {
        Target::at(p)
    })
}

fn next_line(s: &Start) -> Result<Target, Fail> {
    vertical(s, Dir::Forward, "End of buffer")
}

fn previous_line(s: &Start) -> Result<Target, Fail> {
    vertical(s, Dir::Backward, "Beginning of buffer")
}

/// `j` and `k`: by logical line, skipping hidden lines, keeping the goal column.
/// They move as far as they can. With no line to go to, Evil's try the goal column on
/// this line and signal only if that doesn't move the cursor. A goal past the line's end
/// does move it, onto the newline, and normal state puts it back.
fn vertical(s: &Start, dir: Dir, err: Fail) -> Result<Target, Fail> {
    let t = s.text;
    let goal = s.goal.unwrap_or(Goal::Col(t.display_col(s.pos)));
    let mut line = t.line_of(s.pos);
    let mut moved = false;
    for _ in 0..s.n() {
        match t.next_visible(line, dir) {
            Some(l) if l <= last_line(t) => (line, moved) = (l, true),
            _ => break,
        }
    }
    let at = goal_pos(t, line, goal);
    if !moved && at == s.pos {
        return Ok(Target::short(s.pos, err));
    }
    Ok(Target {
        goal: Some(goal),
        ..Target::at(normal_pos(t, at))
    })
}

/// The position on `line` at the goal column, which may be its newline.
fn goal_pos(t: &TextBuf, line: usize, goal: Goal) -> usize {
    match goal {
        Goal::Col(col) => pos_at_col(t, line, col),
        Goal::Eol => line_end(t, line),
    }
}

fn first_non_blank(s: &Start) -> Result<Target, Fail> {
    Ok(Target::at(indent_end(s.text, s.text.line_of(s.pos))))
}

pub fn end_of_line(s: &Start) -> Result<Target, Fail> {
    let t = s.text;
    let here = t.line_of(s.pos);
    let line = here.saturating_add(s.n() - 1).min(last_line(t).max(here));
    let end = line_end(t, line);
    if s.op.is_some() {
        // An operator takes through the line's last character. On an empty line there's
        // nothing to take, not even the newline.
        return Ok(if end == t.line_start(line) {
            Target {
                kind: Some(Kind::Exclusive),
                ..Target::at(end)
            }
        } else {
            Target {
                goal: Some(Goal::Eol),
                ..Target::at(normal_pos(t, end))
            }
        });
    }
    Ok(Target {
        goal: Some(Goal::Eol),
        ..Target::at(end)
    })
}

/// `gg` and `G` to a line counted from 1, keeping the goal column, as in your Evil
/// (no `startofline`).
fn goto_line(s: &Start, n: usize) -> Result<Target, Fail> {
    let t = s.text;
    let goal = s.goal.unwrap_or(Goal::Col(t.display_col(s.pos)));
    let line = n.saturating_sub(1).min(last_line(t));
    Ok(Target {
        goal: Some(goal),
        ..Target::at(normal_pos(t, goal_pos(t, line, goal)))
    })
}

// ---------------------------------------------------------------- words

#[derive(Clone, Copy, PartialEq)]
enum Word {
    Begin,
    End,
    Back,
}

/// `w`, `e`, `b` and their WORD forms. Past the last word, `w` and `e` stop on the
/// text's last character, as in Vim; Evil goes on to the line after a final newline.
fn words(s: &Start, which: Word, big: bool) -> Result<Target, Fail> {
    match which {
        Word::Begin if s.op.is_some() => return operator_word_begin(s, big),
        Word::End if s.op.is_some() => return operator_word_end(s, big),
        _ => {}
    }
    let t = s.text;
    let mut p = s.pos;
    for _ in 0..s.n() {
        let next = match which {
            Word::Begin => word_begin(t, p, big),
            Word::End => word_end(t, p, big),
            Word::Back => word_back(t, p, big),
        };
        match next {
            Some(q) => p = q,
            None if which == Word::Back => {
                p = 0;
                break;
            }
            None => {
                p = normal_pos(t, t.len());
                break;
            }
        }
    }
    if normal_pos(t, p) == normal_pos(t, s.pos) {
        return Err(if which == Word::Back {
            "Beginning of buffer"
        } else {
            "End of buffer"
        });
    }
    Ok(Target::at(p))
}

/// `w` for an operator. It stops before the word it reaches, and if that word starts a
/// later line, at the end of the line before, so `dw` on a line's last word leaves the
/// newline. `cw` on a word acts like `ce`, as in Vim.
fn operator_word_begin(s: &Start, big: bool) -> Result<Target, Fail> {
    let t = s.text;
    if at_text_end(t, s.pos) {
        return Err("End of buffer");
    }
    if s.op == Some(Operator::Change) && on_thing(t, s.pos, big) {
        return Ok(Target::at(thing_ends(t, s.pos, big, s.n())));
    }
    let mut p = s.pos;
    for _ in 0..s.n() {
        match word_begin(t, p, big) {
            Some(q) => p = q,
            None => {
                p = t.len();
                break;
            }
        }
    }
    let mut line = t.line_of(p);
    let on_later_line = t.line_start(line) > s.pos;
    if on_later_line && indent_end(t, line) >= p {
        // Back over any lines of nothing but spaces, unless that's where it started.
        line -= 1;
        while line_end(t, line) > t.line_start(line)
            && blank_line(t, line)
            && t.line_start(line) > s.pos
        {
            line -= 1;
        }
        p = line_end(t, line);
        // An empty line above: an operator takes that line, newline and all.
        if p == t.line_start(line) {
            p += 1;
        }
    }
    Ok(Target {
        no_column: true,
        ..Target::at(p)
    })
}

/// `e` for an operator. On a one-letter word it stays, so `de` takes just that letter.
fn operator_word_end(s: &Start, big: bool) -> Result<Target, Fail> {
    let t = s.text;
    if at_text_end(t, s.pos) {
        return Err("End of buffer");
    }
    if s.n() == 1 && one_letter_word(t, s.pos, big) {
        return Ok(Target::at(s.pos));
    }
    let mut c = Chars::at(t, s.pos);
    c.fwd();
    let mut c = Chars::at(t, thing_ends(t, c.pos(), big, s.n()));
    c.back();
    Ok(Target::at(c.pos()))
}

/// Whether a forward motion can't start from `p`, as Evil checks first: at the text's
/// end, or on the last character of a last line with no newline.
fn at_text_end(t: &TextBuf, p: usize) -> bool {
    match next_stop(t, p) {
        None => true,
        Some(q) => q == t.len() && t.rope().byte(p) != b'\n',
    }
}

/// Whether `p` is on a word, a run of punctuation or an empty line: Evil's `evil-word`.
fn on_thing(t: &TextBuf, p: usize, big: bool) -> bool {
    let c = Chars::at(t, p);
    match c.ch().map(|ch| class(ch, big)) {
        None | Some(Class::Blank) => false,
        Some(Class::Newline) => empty_line(&c),
        Some(_) => true,
    }
}

/// Just past the `n`th word, run of punctuation or empty line from `p`, else the text's end.
fn thing_ends(t: &TextBuf, p: usize, big: bool, n: usize) -> usize {
    let mut p = p;
    for _ in 0..n {
        match thing_end(t, p, big) {
            Some(q) => p = q,
            None => return t.len(),
        }
    }
    p
}

fn thing_end(t: &TextBuf, p: usize, big: bool) -> Option<usize> {
    let mut c = Chars::at(t, p);
    loop {
        let mut a = c.ch()?;
        match class(a, big) {
            Class::Blank => {}
            Class::Newline if empty_line(&c) => {
                c.fwd();
                return Some(c.pos());
            }
            Class::Newline => {}
            _ => {
                c.fwd();
                while let Some(b) = c.ch().filter(|&b| same_word(a, b, big)) {
                    a = b;
                    c.fwd();
                }
                return Some(c.pos());
            }
        }
        c.fwd();
    }
}

fn one_letter_word(t: &TextBuf, p: usize, big: bool) -> bool {
    let mut c = Chars::at(t, p);
    let Some(a) = c.ch() else { return false };
    if class(a, false) != Class::Word || c.prev_ch().is_some_and(|b| same_word(b, a, big)) {
        return false;
    }
    c.fwd();
    !c.ch().is_some_and(|b| same_word(a, b, big))
}

/// An empty line counts as a word: Evil's `w`, `e` and `b` all stop on one.
fn empty_line(c: &Chars) -> bool {
    c.ch() == Some('\n') && c.at_line_start()
}

fn word_begin(t: &TextBuf, p: usize, big: bool) -> Option<usize> {
    let mut c = Chars::at(t, p);
    let mut a = c.ch()?;
    let in_word = matches!(class(a, big), Class::Word | Class::Punct);
    c.fwd();
    while let Some(b) = c.ch().filter(|&b| in_word && same_word(a, b, big)) {
        a = b;
        c.fwd();
    }
    loop {
        match class(c.ch()?, big) {
            Class::Blank => {}
            Class::Newline if empty_line(&c) => return Some(c.pos()),
            Class::Newline => {}
            _ => return Some(c.pos()),
        }
        c.fwd();
    }
}

/// The end of the word after `p` (`e`). It steps over whole graphemes and lands on one,
/// so on "café" with a combining mark it lands on the é, as in Vim (deviations.json).
fn word_end(t: &TextBuf, p: usize, big: bool) -> Option<usize> {
    let mut c = Chars::at(t, next_stop(t, p)?);
    loop {
        let mut a = c.ch()?;
        match class(a, big) {
            Class::Blank => {}
            Class::Newline if empty_line(&c) => return Some(c.pos()),
            Class::Newline => {}
            _ => loop {
                let here = c.pos();
                c.fwd();
                match c.ch() {
                    Some(b) if same_word(a, b, big) => a = b,
                    _ => return Some(t.snap(here, Some(Dir::Backward))),
                }
            },
        }
        c.fwd();
    }
}

/// The start of the word before `p` (`b`); `<C-w>` in insert state uses it too.
pub(crate) fn word_back(t: &TextBuf, p: usize, big: bool) -> Option<usize> {
    let mut c = Chars::at(t, p);
    if !c.back() {
        return None;
    }
    loop {
        let mut b = c.ch()?;
        match class(b, big) {
            Class::Blank => {}
            Class::Newline if empty_line(&c) => return Some(c.pos()),
            Class::Newline => {}
            _ => {
                while let Some(a) = c.prev_ch().filter(|&a| same_word(a, b, big)) {
                    b = a;
                    c.back();
                }
                return Some(c.pos());
            }
        }
        if !c.back() {
            return Some(c.pos());
        }
    }
}

// ---------------------------------------------------------------- f, t, ; and ,

/// A character search on the line: what `;` and `,` repeat.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Find {
    pub ch: char,
    pub dir: Dir,
    /// `t` and `T`: stop next to the character.
    pub till: bool,
    /// `;` or `,`: without a count, a `t` or `T` skips the character the cursor
    /// is already next to.
    pub repeat: bool,
}

impl Find {
    /// The search `key` (`f`, `F`, `t` or `T`) makes for `ch`.
    pub fn new(key: char, ch: Option<char>) -> Find {
        Find {
            ch: ch.unwrap_or('\0'),
            dir: if key.is_lowercase() {
                Dir::Forward
            } else {
                Dir::Backward
            },
            till: key == 't' || key == 'T',
            repeat: false,
        }
    }
    /// The search `,` makes after this one.
    pub fn reversed(self) -> Find {
        let dir = if self.dir == Dir::Forward {
            Dir::Backward
        } else {
            Dir::Forward
        };
        Find { dir, ..self }
    }
}

/// The count-th `ch` on the line in `dir`. Fails without moving if there aren't
/// that many. Evil's forward search first steps off the cursor with its `l`, so from a
/// line's last character it fails as `l` does, and the goal column stays.
pub fn find(s: &Start, search: Find) -> Result<Target, Fail> {
    let t = s.text;
    let line = t.line_of(s.pos);
    let (start, end) = (t.line_start(line), line_end(t, line));
    let step = |c: &mut Chars| match search.dir {
        Dir::Forward => c.fwd() && c.pos() < end,
        Dir::Backward => c.pos() > start && c.back(),
    };
    let mut n = s.n();
    if search.till && search.repeat && n == 1 {
        let mut c = Chars::at(t, s.pos);
        if step(&mut c) && c.ch() == Some(search.ch) {
            n = 2;
        }
    }
    let (mut c, mut seen) = (Chars::at(t, s.pos), 0);
    while step(&mut c) {
        if c.ch() == Some(search.ch) {
            seen += 1;
            if seen == n {
                let pos = match (search.till, search.dir) {
                    (false, _) => c.pos(),
                    (true, Dir::Forward) => prev_stop(t, c.pos()).unwrap_or(c.pos()),
                    (true, Dir::Backward) => next_stop(t, c.pos()).unwrap_or(c.pos()),
                };
                return Ok(Target::at(pos));
            }
        }
    }
    // For an operator that `l` can land on the newline, so it fails only on an empty line.
    let stuck = match s.op {
        Some(_) => s.pos >= end,
        None => next_stop(t, s.pos).is_none_or(|q| q >= end),
    };
    if search.dir == Dir::Forward && stuck {
        return Ok(Target::short(s.pos, "End of line"));
    }
    Err("Can't find the character")
}

// ---------------------------------------------------------------- %, { and }

/// `%`: to the partner of the first bracket on the line at or after the cursor, by
/// Emacs's scan, so it skips strings, and any closer can be a partner. A cursor in a string
/// with no bracket left in it tries again from the string's end. With a count, `N%`.
fn jump_item(s: &Start) -> Result<Target, Fail> {
    if let Some(n) = s.count {
        return percent(s, n);
    }
    match bracket_partner(s.text, s.pos) {
        Some(q) => Ok(Target::at(q)),
        None => string_around(s.text, s.pos)
            .and_then(|(_, end)| bracket_partner(s.text, end))
            .map(Target::at)
            .ok_or("No matching item on this line"),
    }
}

/// The bracket `%` goes to from `p`: whichever comes first, the next opener's closer or
/// the closer that's next at this depth's opener. It looks on `p`'s line for that bracket,
/// but its partner can be anywhere.
fn bracket_partner(t: &TextBuf, p: usize) -> Option<usize> {
    let lists = Lists::new(t, '(', ')');
    let line = t.line_of(p);
    let eol = line_end(t, line);
    // From a line's end, the character before counts, as when `%` retries from a string's.
    let p = if p == eol && p > t.line_start(line) {
        normal_pos(t, p)
    } else {
        p
    };
    let open = lists.down(p);
    let closer_at = |after: usize| {
        let mut c = Chars::at(t, after);
        c.back();
        c.pos()
    };
    let close = lists.up_forward(p).map(closer_at);
    let open_partner = open.filter(|&o| o < eol).and_then(|o| {
        let mut c = Chars::at(t, o);
        c.fwd();
        lists.up_forward(c.pos()).map(closer_at)
    });
    let close_partner = close
        .filter(|&c| c < eol)
        .and_then(|c| lists.up_backward(c));
    if open.unwrap_or(usize::MAX) < close.unwrap_or(usize::MAX) {
        open_partner
    } else {
        close_partner
    }
}

/// `N%`: to the line holding the character N percent of the way through the
/// text, with Evil's arithmetic, keeping the goal column as `G` does. Unlike `%`, an
/// operator takes it linewise.
fn percent(s: &Start, n: usize) -> Result<Target, Fail> {
    let size = s.text.rope().len_chars();
    let ch = if size > 80_000 {
        n.saturating_mul(size / 100)
    } else {
        n.saturating_mul(size) / 100
    };
    let pos = s.text.rope().char_to_byte(ch.min(size));
    Ok(Target {
        kind: Some(Kind::Linewise),
        ..goto_line(s, s.text.line_of(pos) + 1)?
    })
}

/// `}`: past this paragraph to the next blank line (spaces count as blank, as in
/// Evil). With no blank line after, to the end of the last line, as in Vim.
fn forward_paragraph(s: &Start) -> Result<Target, Fail> {
    let t = s.text;
    if s.pos == t.len() || (s.op.is_some() && at_text_end(t, s.pos)) {
        return Err("End of buffer");
    }
    let (last, mut line) = (last_line(t), t.line_of(s.pos));
    let mut pos = None;
    for _ in 0..s.n() {
        while line <= last && blank_line(t, line) {
            line += 1;
        }
        while line <= last && !blank_line(t, line) {
            line += 1;
        }
        if line > last {
            // An operator takes all of the last line, its newline too.
            pos = Some(if s.op.is_some() {
                t.len()
            } else {
                normal_pos(t, t.len())
            });
            break;
        }
    }
    let pos = pos.unwrap_or_else(|| t.line_start(line));
    if pos == s.pos {
        return Err("End of buffer");
    }
    Ok(Target::at(pos))
}

/// `{`: back past this paragraph to the previous blank line, or the first line.
fn backward_paragraph(s: &Start) -> Result<Target, Fail> {
    let t = s.text;
    let mut line = t.line_of(s.pos);
    for _ in 0..s.n() {
        if line == 0 {
            break;
        }
        while line > 0 && blank_line(t, line) {
            line -= 1;
        }
        while line > 0 && !blank_line(t, line) {
            line -= 1;
        }
    }
    let pos = t.line_start(line);
    if pos == s.pos {
        return Err("Beginning of buffer");
    }
    Ok(Target::at(pos))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn start(t: &TextBuf, pos: usize, count: usize) -> Start<'_> {
        Start {
            text: t,
            pos,
            count: Some(count),
            goal: None,
            arg: None,
            op: None,
        }
    }

    /// Where a motion lands with an operator waiting. It reads nothing but its `Start`.
    fn landing(
        text: &str,
        pos: usize,
        op: Operator,
        run: fn(&Start) -> Result<Target, Fail>,
    ) -> Result<usize, Fail> {
        let t = TextBuf::new(text);
        let s = Start {
            text: &t,
            pos,
            count: None,
            goal: None,
            arg: None,
            op: Some(op),
        };
        run(&s).map(|to| to.pos)
    }

    #[test]
    fn with_an_operator_w_stops_at_the_end_of_the_line_before_the_next_word() {
        let w: fn(&Start) -> Result<Target, Fail> = |s| words(s, Word::Begin, false);
        let d = Operator::Delete;
        assert_eq!(
            landing("foo bar\n", 0, d, w),
            Ok(4),
            "the next word on the line"
        );
        assert_eq!(
            landing("foo bar\nbaz\n", 4, d, w),
            Ok(7),
            "not the next line's"
        );
        assert_eq!(
            landing("foo\n  bar\n", 0, d, w),
            Ok(3),
            "nor past its indent"
        );
        assert_eq!(
            landing("foo\n", 0, d, w),
            Ok(3),
            "the last word in the text"
        );
        assert_eq!(
            landing("\nb\n", 0, d, w),
            Ok(1),
            "an empty line takes its newline"
        );
        assert_eq!(landing("  \n b\n", 1, d, w), Ok(2), "spaces it started on");
        assert_eq!(
            landing("ab", 1, d, w),
            Err("End of buffer"),
            "the last character"
        );
        assert_eq!(landing("ab\n", 1, d, w), Ok(2), "with a newline to stop at");
    }

    #[test]
    fn with_c_w_stops_at_the_end_of_the_word() {
        let w: fn(&Start) -> Result<Target, Fail> = |s| words(s, Word::Begin, false);
        let c = Operator::Change;
        assert_eq!(landing("foo bar\n", 0, c, w), Ok(3));
        assert_eq!(
            landing("foo_bar\n", 0, c, w),
            Ok(3),
            "_ is a word of its own"
        );
        assert_eq!(landing("foo   bar\n", 3, c, w), Ok(6), "on spaces, as dw");
    }

    #[test]
    fn with_an_operator_e_stays_on_a_one_letter_word() {
        let e: fn(&Start) -> Result<Target, Fail> = |s| words(s, Word::End, false);
        let d = Operator::Delete;
        assert_eq!(landing("a bc\n", 0, d, e), Ok(0));
        assert_eq!(landing("ab c\n", 0, d, e), Ok(1));
        assert_eq!(
            landing(". bc\n", 0, d, e),
            Ok(3),
            "only on a word character"
        );
        assert_eq!(landing("abc", 2, d, e), Err("End of buffer"));
    }

    #[test]
    fn with_an_operator_dollar_is_the_last_character_and_exclusive_on_an_empty_line() {
        let t = TextBuf::new("abc\n\n");
        let mut s = start(&t, 0, 1);
        s.op = Some(Operator::Delete);
        let to = end_of_line(&s).expect("it moves");
        assert_eq!((to.pos, to.kind), (2, None));
        s.pos = 4;
        let to = end_of_line(&s).expect("it stays");
        assert_eq!((to.pos, to.kind), (4, Some(Kind::Exclusive)));
    }

    #[test]
    fn with_an_operator_l_reaches_the_newline() {
        let d = Operator::Delete;
        assert_eq!(landing("ab\n", 1, d, forward_char), Ok(2));
        let t = TextBuf::new("ab\n");
        let s = start(&t, 1, 1);
        assert_eq!(
            forward_char(&s).map(|to| to.short),
            Ok(Some("End of line")),
            "not in normal state"
        );
    }

    #[test]
    fn with_an_operator_close_brace_runs_to_the_end_of_the_text() {
        let d = Operator::Delete;
        assert_eq!(landing("a\nb\n", 0, d, forward_paragraph), Ok(4));
        assert_eq!(landing("ab", 1, d, forward_paragraph), Err("End of buffer"));
        let t = TextBuf::new("a\nb\n");
        assert_eq!(
            forward_paragraph(&start(&t, 0, 1)).map(|to| to.pos),
            Ok(2),
            "normal state stays on it"
        );
    }

    #[test]
    fn percent_finds_any_closer_and_skips_strings() {
        let t = |text: &str, pos| bracket_partner(&TextBuf::new(text), pos);
        assert_eq!(t("(a)\n", 0), Some(2));
        assert_eq!(t("(a)\n", 2), Some(0));
        assert_eq!(t("{ ]\n", 0), Some(2), "any closer");
        assert_eq!(t("f(\"(\", b)\n", 0), Some(8), "a string is skipped");
        assert_eq!(t("a ) (b)\n", 2), None, "a closer with nothing open");
        assert_eq!(t("x\n(a)\n", 0), None, "only on this line");
        assert_eq!(t("(\n)\n", 0), Some(2), "but its partner can be anywhere");
    }

    #[test]
    fn a_huge_count_dollar_stops_on_the_last_line() {
        let t = TextBuf::new("a\nb\nc\nd\n");
        assert_eq!(
            end_of_line(&start(&t, 4, usize::MAX)).map(|to| to.pos),
            Ok(7)
        );
    }

    #[test]
    fn a_huge_count_brace_stops_at_the_first_line() {
        let t = TextBuf::new("a\n\nb\n");
        assert_eq!(
            backward_paragraph(&start(&t, 3, usize::MAX)).map(|to| to.pos),
            Ok(0)
        );
    }
}
