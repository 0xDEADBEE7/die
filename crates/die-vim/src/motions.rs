//! Motions (scope §10): plain functions over the text and layout queries. Each
//! follows your Evil, except where deviations.json says otherwise.

use crate::grammar::{Fail, From, Goal, Kind, Motion, To};
use crate::text::{
    Chars, Class, blank_line, class, last_line, line_end, next_stop, normal_pos, prev_stop,
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
    m("vim-forward-word-begin", "w", Kind::Exclusive, "To the next word's start.", |f| words(f, Word::Begin, false)),
    m("vim-forward-WORD-begin", "W", Kind::Exclusive, "To the next WORD's start.", |f| words(f, Word::Begin, true)),
    m("vim-forward-word-end", "e", Kind::Inclusive, "To the next word's end.", |f| words(f, Word::End, false)),
    m("vim-forward-WORD-end", "E", Kind::Inclusive, "To the next WORD's end.", |f| words(f, Word::End, true)),
    m("vim-backward-word-begin", "b", Kind::Exclusive, "To the previous word's start.", |f| words(f, Word::Back, false)),
    m("vim-backward-WORD-begin", "B", Kind::Exclusive, "To the previous WORD's start.", |f| words(f, Word::Back, true)),
    m("vim-first-non-blank", "^", Kind::Exclusive, "To the line's first non-blank.", first_non_blank),
    m("vim-end-of-line", "$", Kind::Inclusive, "To the end of the line, or of count-1 lines down.", end_of_line),
    m("vim-goto-first-line", "gg", Kind::Linewise, "To the first line, or line count.", |f| goto_line(f, f.count.unwrap_or(1))),
    m("vim-goto-line", "G", Kind::Linewise, "To the last line, or line count.", |f| goto_line(f, f.count.unwrap_or(usize::MAX))),
    m("vim-find-char", "f<char>", Kind::Inclusive, "To the next char on the line.", |f| find(f, Find::new('f', f.char))),
    m("vim-find-char-backward", "F<char>", Kind::Exclusive, "To the previous char on the line.", |f| find(f, Find::new('F', f.char))),
    m("vim-find-char-to", "t<char>", Kind::Inclusive, "To just before the next char on the line.", |f| find(f, Find::new('t', f.char))),
    m("vim-find-char-to-backward", "T<char>", Kind::Exclusive, "To just after the previous char on the line.", |f| find(f, Find::new('T', f.char))),
    m("vim-jump-item", "%", Kind::Inclusive, "To the bracket matching the next one on the line.", jump_item),
    m("vim-forward-paragraph", "}", Kind::Exclusive, "To the next blank line after this paragraph.", forward_paragraph),
    m("vim-backward-paragraph", "{", Kind::Exclusive, "To the previous blank line before this paragraph.", backward_paragraph),
];

const fn m(
    name: &'static str,
    keys: &'static str,
    kind: Kind,
    doc: &'static str,
    f: fn(&From) -> Result<To, Fail>,
) -> Motion {
    Motion {
        name,
        keys,
        kind,
        doc,
        f,
    }
}

/// `0`, bound apart because it's also a count digit.
pub fn beginning_of_line(f: &From) -> Result<To, Fail> {
    Ok(To::at(f.text.line_start(f.text.line_of(f.pos))))
}

// ---------------------------------------------------------------- characters and lines

fn backward_char(f: &From) -> Result<To, Fail> {
    let start = f.text.line_start(f.text.line_of(f.pos));
    steps(f, "Beginning of line", |p| {
        prev_stop(f.text, p).filter(|&q| q >= start)
    })
}

fn forward_char(f: &From) -> Result<To, Fail> {
    let end = line_end(f.text, f.text.line_of(f.pos));
    steps(f, "End of line", |p| {
        next_stop(f.text, p).filter(|&q| q < end)
    })
}

/// Up to `count` steps. Evil moves as far as it can and signals when it fell short.
fn steps(f: &From, err: Fail, step: impl Fn(usize) -> Option<usize>) -> Result<To, Fail> {
    let (mut p, mut n) = (f.pos, 0);
    while n < f.n() {
        let Some(q) = step(p) else { break };
        (p, n) = (q, n + 1);
    }
    match n {
        0 => Err(err),
        n if n < f.n() => Ok(To {
            short: Some(err),
            ..To::at(p)
        }),
        _ => Ok(To::at(p)),
    }
}

fn next_line(f: &From) -> Result<To, Fail> {
    vertical(f, Dir::Forward, "End of buffer")
}

fn previous_line(f: &From) -> Result<To, Fail> {
    vertical(f, Dir::Backward, "Beginning of buffer")
}

/// `j` and `k`: by logical line, skipping hidden lines, keeping the goal column.
/// They move as far as they can, and fail only when they can't move at all.
fn vertical(f: &From, dir: Dir, err: Fail) -> Result<To, Fail> {
    let t = f.text;
    let goal = f.goal.unwrap_or(Goal::Col(t.display_col(f.pos)));
    let mut line = t.line_of(f.pos);
    let mut moved = false;
    for _ in 0..f.n() {
        match t.next_visible(line, dir) {
            Some(l) if l <= last_line(t) => (line, moved) = (l, true),
            _ => break,
        }
    }
    if !moved {
        return Err(err);
    }
    Ok(To {
        goal: Some(goal),
        ..To::at(at_goal(t, line, goal))
    })
}

/// The position on `line` at the goal column. A column inside a tab or a wide
/// character lands after it, as Emacs's `move-to-column` does.
fn at_goal(t: &TextBuf, line: usize, goal: Goal) -> usize {
    let (s, e) = (t.line_start(line), line_end(t, line));
    let Goal::Col(col) = goal else {
        return normal_pos(t, e);
    };
    let dl = t.display_line(s);
    let stop = dl
        .stops
        .iter()
        .filter(|st| st.pos >= s && st.pos <= e)
        .find(|st| st.vcol >= col);
    normal_pos(t, stop.map_or(e, |st| st.pos))
}

fn first_non_blank(f: &From) -> Result<To, Fail> {
    let t = f.text;
    let line = t.line_of(f.pos);
    let mut c = Chars::at(t, t.line_start(line));
    while c.ch().is_some_and(|ch| ch == ' ' || ch == '\t') {
        c.fwd();
    }
    Ok(To::at(c.pos().min(line_end(t, line))))
}

fn end_of_line(f: &From) -> Result<To, Fail> {
    let t = f.text;
    let line = (t.line_of(f.pos) + f.n() - 1).min(last_line(t));
    Ok(To {
        goal: Some(Goal::Eol),
        ..To::at(line_end(t, line))
    })
}

/// `gg` and `G` to a line counted from 1, keeping the goal column, as in your Evil
/// (no `startofline`).
fn goto_line(f: &From, n: usize) -> Result<To, Fail> {
    let t = f.text;
    let goal = f.goal.unwrap_or(Goal::Col(t.display_col(f.pos)));
    let line = n.saturating_sub(1).min(last_line(t));
    Ok(To {
        goal: Some(goal),
        ..To::at(at_goal(t, line, goal))
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
fn words(f: &From, which: Word, big: bool) -> Result<To, Fail> {
    let t = f.text;
    let mut p = f.pos;
    for _ in 0..f.n() {
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
    if normal_pos(t, p) == normal_pos(t, f.pos) {
        return Err(if which == Word::Back {
            "Beginning of buffer"
        } else {
            "End of buffer"
        });
    }
    Ok(To::at(p))
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
    /// Repeating a `t` or `T` skips the character the cursor is already next to.
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
pub fn find(f: &From, s: Find) -> Result<To, Fail> {
    let t = f.text;
    let line = t.line_of(f.pos);
    let (start, end) = (t.line_start(line), line_end(t, line));
    let mut c = Chars::at(t, f.pos);
    let step = |c: &mut Chars| match s.dir {
        Dir::Forward => c.fwd() && c.pos() < end,
        Dir::Backward => c.pos() > start && c.back(),
    };
    if s.till && s.repeat {
        step(&mut c);
    }
    let mut seen = 0;
    while step(&mut c) {
        if c.ch() == Some(s.ch) {
            seen += 1;
            if seen == f.n() {
                let pos = match (s.till, s.dir) {
                    (false, _) => c.pos(),
                    (true, Dir::Forward) => prev_stop(t, c.pos()).unwrap_or(c.pos()),
                    (true, Dir::Backward) => next_stop(t, c.pos()).unwrap_or(c.pos()),
                };
                return Ok(To::at(pos));
            }
        }
    }
    Err("Can't find the character")
}

// ---------------------------------------------------------------- %, { and }

const BRACKETS: [(char, char); 3] = [('(', ')'), ('[', ']'), ('{', '}')];

/// `%`: from the first bracket at or after the cursor on its line, to its match.
fn jump_item(f: &From) -> Result<To, Fail> {
    let t = f.text;
    let end = line_end(t, t.line_of(f.pos));
    let mut c = Chars::at(t, f.pos);
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
            return Ok(To::at(c.pos()));
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

/// `}`: past this paragraph to the next blank line (spaces count as blank, as in
/// Evil). With no blank line after, to the end of the last line, as in Vim.
fn forward_paragraph(f: &From) -> Result<To, Fail> {
    let t = f.text;
    let (last, mut line) = (last_line(t), t.line_of(f.pos));
    let mut pos = None;
    for _ in 0..f.n() {
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
    if pos == f.pos {
        return Err("End of buffer");
    }
    Ok(To::at(pos))
}

/// `{`: back past this paragraph to the previous blank line, or the first line.
fn backward_paragraph(f: &From) -> Result<To, Fail> {
    let t = f.text;
    let mut line = t.line_of(f.pos);
    for _ in 0..f.n() {
        while line > 0 && blank_line(t, line) {
            line -= 1;
        }
        while line > 0 && !blank_line(t, line) {
            line -= 1;
        }
    }
    let pos = t.line_start(line);
    if pos == f.pos {
        return Err("Beginning of buffer");
    }
    Ok(To::at(pos))
}
