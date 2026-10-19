//! Motions (scope §10): plain functions over the text and layout queries. Each
//! follows your Evil, except where deviations.json says otherwise.

use crate::grammar::{Fail, Goal, Kind, Motion, Start, Target};
use crate::text::{
    Chars, Class, blank_line, class, indent_end, last_line, line_end, next_stop, normal_pos,
    pos_at_col, prev_stop,
};
use die_core::{Dir, TextBuf};

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

// ---------------------------------------------------------------- characters and lines

fn backward_char(s: &Start) -> Result<Target, Fail> {
    let start = s.text.line_start(s.text.line_of(s.pos));
    steps(s, "Beginning of line", |p| {
        prev_stop(s.text, p).filter(|&q| q >= start)
    })
}

fn forward_char(s: &Start) -> Result<Target, Fail> {
    let end = line_end(s.text, s.text.line_of(s.pos));
    steps(s, "End of line", |p| {
        next_stop(s.text, p).filter(|&q| q < end)
    })
}

/// Up to `count` steps. Evil moves as far as it can and signals when it fell short.
fn steps(s: &Start, err: Fail, step: impl Fn(usize) -> Option<usize>) -> Result<Target, Fail> {
    let (mut p, mut n) = (s.pos, 0);
    while n < s.n() {
        let Some(q) = step(p) else { break };
        (p, n) = (q, n + 1);
    }
    match n {
        0 => Err(err),
        n if n < s.n() => Ok(Target {
            short: Some(err),
            ..Target::at(p)
        }),
        _ => Ok(Target::at(p)),
    }
}

fn next_line(s: &Start) -> Result<Target, Fail> {
    vertical(s, Dir::Forward, "End of buffer")
}

fn previous_line(s: &Start) -> Result<Target, Fail> {
    vertical(s, Dir::Backward, "Beginning of buffer")
}

/// `j` and `k`: by logical line, skipping hidden lines, keeping the goal column.
/// They move as far as they can, and fail only when they can't move at all.
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
    if !moved {
        return Err(err);
    }
    Ok(Target {
        goal: Some(goal),
        ..Target::at(at_goal(t, line, goal))
    })
}

/// The position on `line` at the goal column.
fn at_goal(t: &TextBuf, line: usize, goal: Goal) -> usize {
    normal_pos(
        t,
        match goal {
            Goal::Col(col) => pos_at_col(t, line, col),
            Goal::Eol => line_end(t, line),
        },
    )
}

fn first_non_blank(s: &Start) -> Result<Target, Fail> {
    Ok(Target::at(indent_end(s.text, s.text.line_of(s.pos))))
}

fn end_of_line(s: &Start) -> Result<Target, Fail> {
    let t = s.text;
    let line = t.line_of(s.pos).saturating_add(s.n() - 1).min(last_line(t));
    Ok(Target {
        goal: Some(Goal::Eol),
        ..Target::at(line_end(t, line))
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
        ..Target::at(at_goal(t, line, goal))
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

/// An empty line counts as a word: Evil's `w`, `e` and `b` all stop on one.
fn empty_line(c: &Chars) -> bool {
    c.ch() == Some('\n') && c.at_line_start()
}

fn word_begin(t: &TextBuf, p: usize, big: bool) -> Option<usize> {
    let mut c = Chars::at(t, p);
    let k = class(c.ch()?, big);
    if matches!(k, Class::Word(_) | Class::Punct) {
        while c.ch().map(|ch| class(ch, big)) == Some(k) {
            c.fwd();
        }
    } else {
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

fn word_end(t: &TextBuf, p: usize, big: bool) -> Option<usize> {
    let mut c = Chars::at(t, p);
    c.ch()?;
    c.fwd();
    loop {
        match class(c.ch()?, big) {
            Class::Blank => {}
            Class::Newline if empty_line(&c) => return Some(c.pos()),
            Class::Newline => {}
            k => loop {
                let here = c.pos();
                c.fwd();
                if c.ch().map(|ch| class(ch, big)) != Some(k) {
                    return Some(here);
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
        match class(c.ch()?, big) {
            Class::Blank => {}
            Class::Newline if empty_line(&c) => return Some(c.pos()),
            Class::Newline => {}
            k => {
                while c.prev_ch().map(|ch| class(ch, big)) == Some(k) {
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
/// that many.
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
    Err("Can't find the character")
}

// ---------------------------------------------------------------- %, { and }

const BRACKETS: [(char, char); 3] = [('(', ')'), ('[', ']'), ('{', '}')];

/// `%`: from the first bracket at or after the cursor on its line, to its match.
/// With a count, `N%`.
fn jump_item(s: &Start) -> Result<Target, Fail> {
    if let Some(n) = s.count {
        return percent(s, n);
    }
    let t = s.text;
    let end = line_end(t, t.line_of(s.pos));
    let mut c = Chars::at(t, s.pos);
    let (open, close, dir) = loop {
        let ch = c
            .ch()
            .filter(|_| c.pos() < end)
            .ok_or("No bracket on this line")?;
        if let Some(&(o, cl)) = BRACKETS.iter().find(|(o, cl)| ch == *o || ch == *cl) {
            break (o, cl, if ch == o { Dir::Forward } else { Dir::Backward });
        }
        c.fwd();
    };
    let mut depth = 0usize;
    loop {
        match c.ch() {
            Some(ch) if ch == open => {
                depth = if dir == Dir::Forward {
                    depth + 1
                } else {
                    depth - 1
                }
            }
            Some(ch) if ch == close => {
                depth = if dir == Dir::Forward {
                    depth - 1
                } else {
                    depth + 1
                }
            }
            _ => {}
        }
        if depth == 0 {
            return Ok(Target::at(c.pos()));
        }
        let moved = if dir == Dir::Forward {
            c.fwd()
        } else {
            c.back()
        };
        if !moved || c.ch().is_none() {
            return Err("No matching bracket");
        }
    }
}

/// `N%`: to the line holding the character N percent of the way through the
/// text, with Evil's arithmetic, keeping the goal column as `G` does. V3: unlike
/// `%`, an operator takes it linewise.
fn percent(s: &Start, n: usize) -> Result<Target, Fail> {
    let size = s.text.rope().len_chars();
    let ch = if size > 80_000 {
        n.saturating_mul(size / 100)
    } else {
        n.saturating_mul(size) / 100
    };
    let pos = s.text.rope().char_to_byte(ch.min(size));
    goto_line(s, s.text.line_of(pos) + 1)
}

/// `}`: past this paragraph to the next blank line (spaces count as blank, as in
/// Evil). With no blank line after, to the end of the last line, as in Vim.
fn forward_paragraph(s: &Start) -> Result<Target, Fail> {
    let t = s.text;
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
            pos = Some(normal_pos(t, t.len()));
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
        }
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
