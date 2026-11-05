//! Text objects (scope §10): words, WORDs, paragraphs, quotes and brackets, in both `i`
//! and `a` forms. Like motions, each is a plain function over the text: it takes where the
//! cursor is and the count, and gives the [`Span`] an operator acts on. Each follows your
//! Evil in fundamental-mode. So quotes pair up from the start of the text, not the line,
//! `"` is a string whichever object is asked for, and `\` escapes (S5's `evil-select-quote`).

use crate::grammar::{Fail, Start};
use crate::operators::Span;
use crate::text::{Chars, Class, blank_line, class, indent_end, line_end, next_stop, same_word};
use die_core::TextBuf;

/// A text object: what it covers from where a motion would start.
pub type Object = fn(&Start) -> Result<Span, Fail>;

/// Every text object, by the key after `i` or `a`: the keys, the command's name, its
/// function and its doc string.
#[rustfmt::skip]
pub const OBJECTS: &[(&str, &str, Object, &str)] = &[
    ("iw", "vim-inner-word", |s| inner_word(s, false), "A word, or the spaces under the cursor."),
    ("aw", "vim-a-word", |s| a_word(s, false), "A word and the spaces after it, or else before it."),
    ("iW", "vim-inner-WORD", |s| inner_word(s, true), "A WORD, or the spaces under the cursor."),
    ("aW", "vim-a-WORD", |s| a_word(s, true), "A WORD and the spaces after it, or else before it."),
    ("ip", "vim-inner-paragraph", inner_paragraph, "A paragraph, or the blank lines under the cursor."),
    ("ap", "vim-a-paragraph", a_paragraph, "A paragraph and the blank lines after it, or else before it."),
    ("i\"", "vim-inner-double-quote", |s| quoted(s, '"', false), "Inside a double-quoted string."),
    ("a\"", "vim-a-double-quote", |s| quoted(s, '"', true), "A double-quoted string and the spaces after it, or else before it."),
    ("i'", "vim-inner-single-quote", |s| quoted(s, '\'', false), "Inside a single-quoted string."),
    ("a'", "vim-a-single-quote", |s| quoted(s, '\'', true), "A single-quoted string and the spaces after it, or else before it."),
    ("i`", "vim-inner-back-quote", |s| quoted(s, '`', false), "Inside a back-quoted string."),
    ("a`", "vim-a-back-quote", |s| quoted(s, '`', true), "A back-quoted string and the spaces after it, or else before it."),
    ("i(", "vim-inner-paren", |s| bracket(s, '(', ')', false), "Inside parentheses."),
    ("i)", "vim-inner-paren", |s| bracket(s, '(', ')', false), "Inside parentheses."),
    ("ib", "vim-inner-paren", |s| bracket(s, '(', ')', false), "Inside parentheses."),
    ("a(", "vim-a-paren", |s| bracket(s, '(', ')', true), "Parentheses and what's inside."),
    ("a)", "vim-a-paren", |s| bracket(s, '(', ')', true), "Parentheses and what's inside."),
    ("ab", "vim-a-paren", |s| bracket(s, '(', ')', true), "Parentheses and what's inside."),
    ("i[", "vim-inner-bracket", |s| bracket(s, '[', ']', false), "Inside square brackets."),
    ("i]", "vim-inner-bracket", |s| bracket(s, '[', ']', false), "Inside square brackets."),
    ("a[", "vim-a-bracket", |s| bracket(s, '[', ']', true), "Square brackets and what's inside."),
    ("a]", "vim-a-bracket", |s| bracket(s, '[', ']', true), "Square brackets and what's inside."),
    ("i{", "vim-inner-curly", |s| bracket(s, '{', '}', false), "Inside braces."),
    ("i}", "vim-inner-curly", |s| bracket(s, '{', '}', false), "Inside braces."),
    ("iB", "vim-inner-curly", |s| bracket(s, '{', '}', false), "Inside braces."),
    ("a{", "vim-a-curly", |s| bracket(s, '{', '}', true), "Braces and what's inside."),
    ("a}", "vim-a-curly", |s| bracket(s, '{', '}', true), "Braces and what's inside."),
    ("aB", "vim-a-curly", |s| bracket(s, '{', '}', true), "Braces and what's inside."),
    ("i<lt>", "vim-inner-angle", |s| bracket(s, '<', '>', false), "Inside angle brackets."),
    ("i>", "vim-inner-angle", |s| bracket(s, '<', '>', false), "Inside angle brackets."),
    ("a<lt>", "vim-an-angle", |s| bracket(s, '<', '>', true), "Angle brackets and what's inside."),
    ("a>", "vim-an-angle", |s| bracket(s, '<', '>', true), "Angle brackets and what's inside."),
];

// ---------------------------------------------------------------- things and the gaps between

/// Text as a run of things with gaps between them: words with spaces, paragraphs with
/// blank lines. `(start, end)` pairs are byte ranges.
trait Things {
    /// The range these things are looked for in.
    fn bounds(&self) -> (usize, usize);
    /// The thing at `p`.
    fn at(&self, p: usize) -> Option<(usize, usize)>;
    /// The first thing starting at or after `p`.
    fn next(&self, p: usize) -> Option<(usize, usize)>;
    /// The last thing ending at or before `p`.
    fn prev(&self, p: usize) -> Option<(usize, usize)>;
}

/// The gap `p` is in or touches, with `lo` as the lower bound when no thing is before it.
fn gap_from(x: &impl Things, p: usize, lo: usize) -> Option<(usize, usize)> {
    let beg = x.prev(p).map_or(lo, |t| t.1).max(lo);
    let end = x.next(p).map_or(x.bounds().1, |t| t.0);
    (beg <= p && p <= end && beg < end).then_some((beg, end))
}

fn gap(x: &impl Things, p: usize) -> Option<(usize, usize)> {
    gap_from(x, p, x.bounds().0)
}

/// The end of the thing after `p`, else the end of the range.
fn forward_thing(x: &impl Things, p: usize) -> usize {
    x.at(p)
        .filter(|t| t.1 > p)
        .or_else(|| x.next(p))
        .map_or(x.bounds().1, |t| t.1)
}

/// `iw` and `ip`: what's at the cursor, thing or gap, and with a count, that many of them
/// in turn.
fn inner(x: &impl Things, pos: usize, n: usize) -> (usize, usize) {
    let (beg, mut end) = x.at(pos).or_else(|| gap(x, pos)).unwrap_or(x.bounds());
    for _ in 1..n {
        match x.at(end).or_else(|| gap(x, end)) {
            Some((_, e)) if e > end => end = e,
            _ => break,
        }
    }
    (beg, end)
}

/// `aw` and `ap`: the thing and the gap after it. With no gap after, the gap before it,
/// as far back as `floor`. On a gap: the gap and the thing after it. A count takes that
/// many pairs.
fn around(x: &impl Things, pos: usize, n: usize, floor: usize) -> (usize, usize) {
    let (mut beg, mut end);
    if let Some(thing) = x.at(pos) {
        (beg, end) = thing;
        match gap(x, end) {
            Some((_, e)) => end = e,
            None => {
                if let Some((b, _)) = gap_from(x, beg, floor) {
                    beg = b;
                }
            }
        }
    } else {
        (beg, end) = gap(x, pos).unwrap_or(x.bounds());
        end = forward_thing(x, end);
    }
    for _ in 1..n {
        let at_gap = gap(x, end).is_some_and(|g| g.1 != end);
        end = forward_thing(x, end);
        if !at_gap && let Some((_, e)) = gap(x, end) {
            end = e;
        }
    }
    (beg, end)
}

// ---------------------------------------------------------------- words

/// Words within a line. An empty line has no line to stay in, so it searches the whole
/// text, where each empty line is a word of its own, its newline and all.
struct Words<'a> {
    t: &'a TextBuf,
    big: bool,
    lo: usize,
    hi: usize,
}

impl Words<'_> {
    fn new<'a>(s: &Start<'a>, big: bool) -> Words<'a> {
        let t = s.text;
        let line = t.line_of(s.pos);
        let (start, end) = (t.line_start(line), line_end(t, line));
        let (lo, hi) = if start == end {
            (0, t.len())
        } else {
            (start, end)
        };
        Words {
            t: s.text,
            big,
            lo,
            hi,
        }
    }
    /// The thing starting at or spanning this character, if it's not blank.
    fn span(&self, c: &Chars) -> Option<(usize, usize)> {
        let a = c.ch()?;
        match class(a, self.big) {
            Class::Blank => None,
            Class::Newline => {
                if !c.at_line_start() {
                    return None;
                }
                let mut after = Chars::at(self.t, c.pos());
                after.fwd();
                Some((c.pos(), after.pos()))
            }
            _ => {
                let mut b = Chars::at(self.t, c.pos());
                let mut x = a;
                while b.pos() > self.lo {
                    let Some(y) = b.prev_ch().filter(|&y| same_word(y, x, self.big)) else {
                        break;
                    };
                    x = y;
                    b.back();
                }
                let mut e = Chars::at(self.t, c.pos());
                let mut x = a;
                e.fwd();
                while e.pos() < self.hi {
                    let Some(y) = e.ch().filter(|&y| same_word(x, y, self.big)) else {
                        break;
                    };
                    x = y;
                    e.fwd();
                }
                Some((b.pos(), e.pos()))
            }
        }
    }
}

impl Things for Words<'_> {
    fn bounds(&self) -> (usize, usize) {
        (self.lo, self.hi)
    }
    fn at(&self, p: usize) -> Option<(usize, usize)> {
        (p < self.hi)
            .then(|| self.span(&Chars::at(self.t, p)))
            .flatten()
    }
    fn next(&self, p: usize) -> Option<(usize, usize)> {
        let mut c = Chars::at(self.t, p);
        while c.pos() < self.hi {
            if let Some(x) = self.span(&c).filter(|x| x.0 >= p) {
                return Some(x);
            }
            c.fwd();
        }
        None
    }
    fn prev(&self, p: usize) -> Option<(usize, usize)> {
        let mut c = Chars::at(self.t, p);
        while c.pos() > self.lo && c.back() {
            if let Some(x) = self.span(&c).filter(|x| x.1 <= p) {
                return Some(x);
            }
        }
        None
    }
}

fn inner_word(s: &Start, big: bool) -> Result<Span, Fail> {
    let (beg, end) = inner(&Words::new(s, big), s.pos, s.n());
    Ok(Span::chars(beg, end))
}

fn a_word(s: &Start, big: bool) -> Result<Span, Fail> {
    let line = s.text.line_of(s.pos);
    // Never the line's indentation, so `daw` on an indented line's only word leaves it.
    let (beg, end) = around(&Words::new(s, big), s.pos, s.n(), indent_end(s.text, line));
    Ok(Span::chars(beg, end))
}

// ---------------------------------------------------------------- paragraphs

/// Paragraphs: runs of lines that aren't blank.
struct Paragraphs<'a> {
    t: &'a TextBuf,
}

impl Paragraphs<'_> {
    fn lines(&self) -> usize {
        self.t.line_count()
    }
    fn blank(&self, line: usize) -> bool {
        blank_line(self.t, line)
    }
    fn start(&self, line: usize) -> usize {
        if line < self.lines() {
            self.t.line_start(line)
        } else {
            self.t.len()
        }
    }
    /// The paragraph holding `line`, which isn't blank.
    fn around(&self, line: usize) -> (usize, usize) {
        let (mut a, mut b) = (line, line + 1);
        while a > 0 && !self.blank(a - 1) {
            a -= 1;
        }
        while b < self.lines() && !self.blank(b) {
            b += 1;
        }
        (self.start(a), self.start(b))
    }
}

impl Things for Paragraphs<'_> {
    fn bounds(&self) -> (usize, usize) {
        (0, self.t.len())
    }
    fn at(&self, p: usize) -> Option<(usize, usize)> {
        let line = self.t.line_of(p);
        (!self.blank(line)).then(|| self.around(line))
    }
    fn next(&self, p: usize) -> Option<(usize, usize)> {
        let mut line = self.t.line_of(p);
        while line < self.lines() {
            if self.blank(line) {
                line += 1;
                continue;
            }
            let x = self.around(line);
            if x.0 >= p {
                return Some(x);
            }
            line = self.t.line_of(x.1).max(line + 1);
        }
        None
    }
    fn prev(&self, p: usize) -> Option<(usize, usize)> {
        let mut line = self.t.line_of(p) + 1;
        while line > 0 {
            line -= 1;
            if self.blank(line) {
                continue;
            }
            let x = self.around(line);
            if x.1 <= p {
                return Some(x);
            }
            line = self.t.line_of(x.0);
        }
        None
    }
}

fn inner_paragraph(s: &Start) -> Result<Span, Fail> {
    let (beg, end) = inner(&Paragraphs { t: s.text }, s.pos, s.n());
    Ok(Span::whole_lines(beg, end))
}

fn a_paragraph(s: &Start) -> Result<Span, Fail> {
    let (beg, end) = around(&Paragraphs { t: s.text }, s.pos, s.n(), 0);
    Ok(Span::whole_lines(beg, end))
}

// ---------------------------------------------------------------- quotes

/// A string as Emacs's parser sees it: its delimiter, and its range with both quotes.
type Quoted = (char, usize, usize);

/// The first string that ends after `p`, parsing from the start of the text. `"` and
/// `quote` delimit strings, and `\` escapes the next character, in a string or out.
fn string_after(t: &TextBuf, p: usize, quote: char) -> Option<Quoted> {
    let mut c = Chars::at(t, 0);
    let mut open: Option<(char, usize)> = None;
    while let Some(ch) = c.ch() {
        let at = c.pos();
        match open {
            _ if ch == '\\' => {
                c.fwd();
            }
            None if ch == quote || ch == '"' => open = Some((ch, at)),
            Some((q, start)) if ch == q => {
                c.fwd();
                let end = c.pos();
                if end > p {
                    return Some((q, start, end));
                }
                open = None;
                continue;
            }
            _ => {}
        }
        c.fwd();
    }
    open.map(|(q, start)| (q, start, t.len()))
}

/// `quote`'s pair in a line of `string`, which `"` delimits: the pair at `p`, else the
/// next one, a last unpaired quote pairing with the string's end. With neither, the whole
/// string, as Evil's narrow search gives. Pairs are counted from the line's start.
fn pair_in_string(t: &TextBuf, p: usize, quote: char, string: (usize, usize)) -> (usize, usize) {
    let (start, end) = string;
    let line_start = t.line_start(t.line_of(p)).max(start);
    let stop = line_end(t, t.line_of(p)).min(end);
    let mut c = Chars::at(t, line_start);
    let mut open = None;
    while c.pos() < stop {
        if c.ch() == Some(quote) {
            match open.take() {
                None => open = Some(c.pos()),
                Some(a) => {
                    let b = next_stop(t, c.pos()).unwrap_or(c.pos());
                    if b > p {
                        return (a, b);
                    }
                }
            }
        }
        c.fwd();
    }
    open.map_or(string, |a| (a, end))
}

/// The whitespace around a string that `a"` takes, newlines included: after it, or else
/// before it.
fn space_around(t: &TextBuf, beg: usize, end: usize) -> (usize, usize) {
    let is_space = |c: Option<char>| matches!(c, Some(' ' | '\t' | '\n' | '\r' | '\u{c}'));
    let mut c = Chars::at(t, end);
    if is_space(c.ch()) {
        while is_space(c.ch()) {
            c.fwd();
        }
        return (beg, c.pos());
    }
    let mut c = Chars::at(t, beg);
    while is_space(c.prev_ch()) {
        c.back();
    }
    (c.pos(), end)
}

fn quoted(s: &Start, quote: char, outer: bool) -> Result<Span, Fail> {
    const NONE: Fail = "No quoted string found";
    let t = s.text;
    let pos = s.pos;
    let (q, mut beg, mut end) = string_after(t, pos, quote).ok_or(NONE)?;
    // Inside a string of another delimiter.
    if q != quote {
        if beg > pos {
            return Err(NONE);
        }
        (beg, end) = pair_in_string(t, pos, quote, (beg, end));
    }
    if outer {
        let (b, e) = space_around(t, beg, end);
        return Ok(Span::chars(b, e));
    }
    // `2i"` takes the quotes too, with no spaces.
    if s.n() == 2 {
        return Ok(Span::chars(beg, end));
    }
    let mut c = Chars::at(t, end);
    c.back();
    Ok(Span::chars(next_stop(t, beg).unwrap_or(beg), c.pos()))
}

// ---------------------------------------------------------------- brackets

/// What a bracket search can see: `(`, `[`, `{` and their closers always, and `<` and `>`
/// when they're what it's after. They all count toward depth, so `(a [b) c]` nests by
/// position, not by kind. It stays in `lo..hi`, and `"` strings are skipped whole.
struct Brackets<'a> {
    t: &'a TextBuf,
    open: char,
    close: char,
    lo: usize,
    hi: usize,
}

impl Brackets<'_> {
    fn opens(&self, c: char) -> bool {
        matches!(c, '(' | '[' | '{') || c == self.open
    }
    fn closes(&self, c: char) -> bool {
        matches!(c, ')' | ']' | '}') || c == self.close
    }
    /// Whether the character at `c` follows an odd number of backslashes.
    fn escaped(&self, c: &Chars) -> bool {
        let mut b = Chars::at(self.t, c.pos());
        let mut n = 0;
        while b.pos() > self.lo && b.prev_ch() == Some('\\') {
            b.back();
            n += 1;
        }
        n % 2 == 1
    }
    /// Out of one level forward: just past the closer, of any kind.
    fn up_forward(&self, p: usize) -> Option<usize> {
        let mut c = Chars::at(self.t, p);
        let mut depth = 0usize;
        while c.pos() < self.hi {
            let ch = c.ch()?;
            if ch == '\\' {
                c.fwd();
            } else if ch == '"' {
                c.fwd();
                loop {
                    if c.pos() >= self.hi {
                        return None;
                    }
                    match c.ch()? {
                        '\\' => {
                            c.fwd();
                        }
                        '"' => break,
                        _ => {}
                    }
                    c.fwd();
                }
            } else if self.opens(ch) {
                depth += 1;
            } else if self.closes(ch) {
                if depth == 0 {
                    c.fwd();
                    return Some(c.pos());
                }
                depth -= 1;
            }
            c.fwd();
        }
        None
    }
    /// Out of one level backward: on the opener, of any kind.
    fn up_backward(&self, p: usize) -> Option<usize> {
        let mut c = Chars::at(self.t, p);
        let mut depth = 0usize;
        while c.pos() > self.lo {
            c.back();
            let ch = c.ch()?;
            if self.escaped(&c) {
                continue;
            }
            if ch == '"' {
                loop {
                    if c.pos() <= self.lo {
                        return None;
                    }
                    c.back();
                    if c.ch() == Some('"') && !self.escaped(&c) {
                        break;
                    }
                }
            } else if self.closes(ch) {
                depth += 1;
            } else if self.opens(ch) {
                if depth == 0 {
                    return Some(c.pos());
                }
                depth -= 1;
            }
        }
        None
    }
    /// The `n`th `close` out from `p`, as far as there are: the last one's range.
    fn closer(&self, p: usize, n: usize) -> Option<(usize, usize)> {
        let (mut p, mut last) = (p, None);
        for _ in 0..n {
            loop {
                let Some(q) = self.up_forward(p) else {
                    return last;
                };
                p = q;
                let mut c = Chars::at(self.t, p);
                c.back();
                if c.ch() == Some(self.close) {
                    last = Some((c.pos(), p));
                    break;
                }
            }
        }
        last
    }
    /// The `n`th `open` out from `p`, as far as there are: the last one's range.
    fn opener(&self, p: usize, n: usize) -> Option<(usize, usize)> {
        let (mut p, mut last) = (p, None);
        for _ in 0..n {
            loop {
                let Some(q) = self.up_backward(p) else {
                    return last;
                };
                p = q;
                let mut c = Chars::at(self.t, p);
                if c.ch() == Some(self.open) {
                    c.fwd();
                    last = Some((p, c.pos()));
                    break;
                }
            }
        }
        last
    }
}

/// `i` leaves out the brackets, and if a line holds nothing else, that line. A result of
/// whole lines is linewise.
fn inside(t: &TextBuf, op: (usize, usize), cl: (usize, usize)) -> Span {
    let at_bol = |p: usize| t.line_start(t.line_of(p)) == p;
    let at_eol = |p: usize| p == line_end(t, t.line_of(p));
    let (mut beg, mut end) = (op.1, cl.0);
    if at_eol(beg) && beg < t.len() {
        beg += 1;
    }
    let bare = indent_end(t, t.line_of(end)) == end;
    if bare {
        end = t.line_start(t.line_of(end));
        if !at_bol(beg) && beg < end {
            end -= 1;
        }
    }
    if at_bol(beg) && at_bol(end) {
        Span::whole_lines(beg, end)
    } else {
        Span::chars(beg, end)
    }
}

/// The brackets round the range `beg..end`. It searches twice, from just inside each end,
/// and takes the tighter pair that still contains the range, as Evil's `evil-select-block`
/// does, so a cursor on a bracket means that bracket's pair.
fn around_range(
    x: &Brackets,
    beg: usize,
    end: usize,
    n: usize,
    fixed: bool,
) -> Option<((usize, usize), (usize, usize))> {
    let t = x.t;
    let end = if beg == end {
        next_stop(t, end).unwrap_or(end).min(x.hi)
    } else {
        end
    };
    let inner_beg = if fixed {
        beg
    } else {
        let mut c = Chars::at(t, beg);
        c.fwd();
        c.pos().min(x.hi)
    };
    let inner_end = if fixed {
        end
    } else {
        let mut c = Chars::at(t, end);
        c.back();
        c.pos()
    };
    let cl1 = x.closer(inner_beg, 1);
    let op1 = cl1.and_then(|c| x.opener(c.0, 1));
    let op2 = x.opener(inner_end, 1);
    let cl2 = op2.and_then(|o| x.closer(o.1, 1));
    if op1.is_none() && cl2.is_none() {
        return None;
    }
    let second = op1.is_none()
        || cl2.is_some_and(|c2| {
            let (o2, o1, c1) = (
                op2.expect("a closer found from it"),
                op1.expect("checked"),
                cl1.expect("an opener found from it"),
            );
            o2.0 >= o1.0 && c2.1 <= c1.1 && o2.0 <= beg && c2.1 >= end
        });
    let (op, cl) = if second { (op2, cl2) } else { (op1, cl1) };
    let (op, cl) = (op?, cl?);
    Some(if n > 1 {
        (
            x.opener(op.0, n - 1).unwrap_or(op),
            x.closer(cl.1, n - 1).unwrap_or(cl),
        )
    } else {
        (op, cl)
    })
}

fn string_of(t: &TextBuf, p: usize) -> Option<(usize, usize)> {
    string_after(t, p, '"')
        .filter(|&(_, beg, _)| beg < p)
        .map(|(_, beg, end)| (beg, end))
}

/// The brackets round the cursor. In a string it looks in that string first, and then
/// round the string.
fn around_cursor(t: &TextBuf, pos: usize, open: char, close: char, n: usize) -> Option<Pair> {
    let whole = Brackets {
        t,
        open,
        close,
        lo: 0,
        hi: t.len(),
    };
    let mut c = Chars::at(t, pos);
    c.fwd();
    let Some((beg, end)) = string_of(t, pos).or_else(|| string_of(t, c.pos())) else {
        return around_range(&whole, pos, pos, n, false);
    };
    let narrow = Brackets {
        lo: beg,
        hi: end,
        ..whole
    };
    around_range(&narrow, pos, pos, n, false).or_else(|| around_range(&whole, beg, end, n, true))
}

type Pair = ((usize, usize), (usize, usize));

fn bracket(s: &Start, open: char, close: char, outer: bool) -> Result<Span, Fail> {
    const NONE: Fail = "No surrounding delimiters found";
    let (t, pos) = (s.text, s.pos);
    let pair = around_cursor(t, pos, open, close, s.n()).or_else(|| {
        // Not in a pair: the next one, wherever it is, if the cursor isn't on an opener.
        if Chars::at(t, pos).ch() == Some(open) {
            return None;
        }
        let mut c = Chars::at(t, pos);
        while c.ch().is_some_and(|ch| ch != open) {
            c.fwd();
        }
        c.ch()?;
        around_cursor(t, c.pos(), open, close, 1).filter(|p| p.0.0 >= c.pos())
    });
    let (op, cl) = pair.ok_or(NONE)?;
    Ok(if outer {
        Span::chars(op.0, cl.1)
    } else {
        inside(t, op, cl)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn start(t: &TextBuf, pos: usize) -> Start<'_> {
        Start {
            text: t,
            pos,
            count: None,
            goal: None,
            arg: None,
            op: None,
        }
    }

    #[test]
    fn a_word_is_cut_at_snake_case_and_a_paths_colons() {
        let t = TextBuf::new("foo_bar::baz\n");
        let word = |pos| inner_word(&start(&t, pos), false).map(|s| (s.beg, s.end));
        assert_eq!(word(1), Ok((0, 3)));
        assert_eq!(word(3), Ok((3, 4)));
        assert_eq!(word(5), Ok((4, 7)));
        assert_eq!(word(8), Ok((7, 9)));
        assert_eq!(word(10), Ok((9, 12)));
        let big = inner_word(&start(&t, 5), true).map(|s| (s.beg, s.end));
        assert_eq!(big, Ok((0, 12)));
    }

    #[test]
    fn quotes_pair_up_from_the_start_of_the_text() {
        let t = TextBuf::new("\"a\"\nb \"c\"\n");
        let at = |pos| quoted(&start(&t, pos), '"', false).map(|s| (s.beg, s.end));
        assert_eq!(at(1), Ok((1, 2)));
        assert_eq!(at(4), Ok((7, 8)), "the next string, not this line's only");
        let t = TextBuf::new("x \"odd\ny\" z\n");
        let at = quoted(&start(&t, 7), '"', false).map(|s| (s.beg, s.end));
        assert_eq!(at, Ok((3, 8)), "a string can cross lines");
    }

    #[test]
    fn brackets_skip_strings_and_escapes() {
        let t = TextBuf::new("f(\")\", \\) b)\n");
        let at = bracket(&start(&t, 10), '(', ')', false).map(|s| (s.beg, s.end));
        assert_eq!(at, Ok((2, 11)));
    }

    #[test]
    fn angle_brackets_count_only_for_their_own_object() {
        let t = TextBuf::new("(a<b)c>\n");
        let parens = bracket(&start(&t, 3), '(', ')', false).map(|s| (s.beg, s.end));
        assert_eq!(parens, Ok((1, 4)), "< means nothing to a paren search");
        let angles = bracket(&start(&t, 3), '<', '>', false).map(|s| (s.beg, s.end));
        assert_eq!(angles, Ok((3, 6)));
    }
}
