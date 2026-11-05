//! Reading text the way die-vim's motions need: characters and their classes,
//! lines, and where the cursor may rest in normal state.

use crate::scripts::{FULL_GREEK, FULL_WIDTH, HALF_KANA, HIRAGANA, KANJI, KATAKANA, RANGES, ROMAN};
use die_core::{Dir, TextBuf};

/// A character's position in the text, stepping by characters.
pub struct Chars<'a> {
    t: &'a TextBuf,
    ci: usize,
}

impl<'a> Chars<'a> {
    pub fn at(t: &'a TextBuf, pos: usize) -> Self {
        Chars {
            t,
            ci: t.rope().byte_to_char(pos.min(t.len())),
        }
    }
    /// The byte position.
    pub fn pos(&self) -> usize {
        self.t.rope().char_to_byte(self.ci)
    }
    /// The character here; None at the end of the text.
    pub fn ch(&self) -> Option<char> {
        self.t.rope().get_char(self.ci)
    }
    /// The character before this one.
    pub fn prev_ch(&self) -> Option<char> {
        self.ci
            .checked_sub(1)
            .and_then(|i| self.t.rope().get_char(i))
    }
    /// Step forward; false at the end of the text.
    pub fn fwd(&mut self) -> bool {
        let ok = self.ci < self.t.rope().len_chars();
        if ok {
            self.ci += 1;
        }
        ok
    }
    /// Step back; false at the start of the text.
    pub fn back(&mut self) -> bool {
        let ok = self.ci > 0;
        if ok {
            self.ci -= 1;
        }
        ok
    }
    /// At the start of a line: the text's start, or just after a newline.
    pub fn at_line_start(&self) -> bool {
        matches!(self.prev_ch(), None | Some('\n'))
    }
}

/// What a word motion sees a character as.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Class {
    Blank,
    Newline,
    /// A run of these is a word, unless [`word_boundary`] splits it.
    Word,
    /// Anything else that isn't blank: `_`, `::`, `->` and `.` are each a word of
    /// their own, as in Evil in fundamental-mode.
    Punct,
}

/// A character's class. With `big` (WORD motions), every non-blank is one class.
/// Word characters are Rust's alphanumerics, not Emacs's word syntax. So marks Rust
/// doesn't call alphanumeric (virama, nukta, Thai tone marks) split words even in
/// one script, and emoji and dingbats are punctuation.
pub fn class(c: char, big: bool) -> Class {
    match c {
        '\n' => Class::Newline,
        ' ' | '\t' | '\r' | '\u{c}' => Class::Blank,
        _ if big => Class::Punct,
        // Emacs's standard syntax table makes these two word characters.
        '$' | '%' => Class::Word,
        _ if c.is_alphanumeric() || combining(c) => Class::Word,
        _ => Class::Punct,
    }
}

/// Whether `b`, just after `a`, carries on the word or run of punctuation `a` is in.
pub fn same_word(a: char, b: char, big: bool) -> bool {
    let k = class(a, big);
    k == class(b, big) && (k != Class::Word || !word_boundary(a, b))
}

/// The main blocks of combining marks, which `is_alphanumeric` leaves out. Emacs
/// gives them word syntax, so "café" spelled with U+0301 is one word.
fn combining(c: char) -> bool {
    matches!(c, '\u{300}'..='\u{36f}' | '\u{1ab0}'..='\u{1aff}' | '\u{1dc0}'..='\u{1dff}'
        | '\u{20d0}'..='\u{20ff}' | '\u{3099}'..='\u{309a}' | '\u{fe20}'..='\u{fe2f}')
}

/// Your Evil's `evil-cjk-word-separating-categories`: pairs that split a word within
/// one script.
#[rustfmt::skip]
const SEPARATING: [(u8, u8); 24] = [
    (KANJI, HIRAGANA), (KANJI, KATAKANA), (KANJI, HALF_KANA), (KANJI, FULL_WIDTH), (KANJI, FULL_GREEK),
    (HIRAGANA, KANJI), (HIRAGANA, KATAKANA), (HIRAGANA, HALF_KANA), (HIRAGANA, FULL_WIDTH), (HIRAGANA, FULL_GREEK),
    (KATAKANA, KANJI), (KATAKANA, HIRAGANA), (KATAKANA, HALF_KANA), (KATAKANA, FULL_WIDTH), (KATAKANA, FULL_GREEK),
    (HALF_KANA, KANJI), (HALF_KANA, HIRAGANA), (HALF_KANA, KATAKANA),
    (FULL_WIDTH, KANJI), (FULL_WIDTH, HIRAGANA), (FULL_WIDTH, KATAKANA),
    (FULL_GREEK, KANJI), (FULL_GREEK, HIRAGANA), (FULL_GREEK, KATAKANA),
];

/// Your Evil's `evil-cjk-word-combining-categories`, marks aside: pairs that join a
/// word across two scripts.
#[rustfmt::skip]
const COMBINING: [(u8, u8); 12] = [
    (ROMAN, HALF_KANA), (ROMAN, FULL_WIDTH), (ROMAN, FULL_GREEK),
    (HALF_KANA, ROMAN), (HALF_KANA, FULL_WIDTH), (HALF_KANA, FULL_GREEK),
    (FULL_WIDTH, ROMAN), (FULL_WIDTH, HALF_KANA), (FULL_WIDTH, FULL_GREEK),
    (FULL_GREEK, ROMAN), (FULL_GREEK, HALF_KANA), (FULL_GREEK, FULL_WIDTH),
];

/// A word character's script and categories, from Emacs's tables (`scripts.rs`).
fn script(c: char) -> (u8, u8) {
    let i = RANGES.partition_point(|r| r.0 <= c as u32);
    i.checked_sub(1)
        .map_or((0, 0), |i| (RANGES[i].1, RANGES[i].2))
}

/// Whether a word ends between two word characters next to each other, by Emacs's
/// `word_boundary_p` with your Evil's lists. Characters of one script join unless
/// `SEPARATING` splits them, and of two scripts split unless `COMBINING` joins them.
/// Either way a combining mark joins what it's next to. Only the main blocks of marks
/// are known here; the table gives any other mark its own script, so it joins letters
/// of that script.
fn word_boundary(a: char, b: char) -> bool {
    if combining(a) || combining(b) {
        return false;
    }
    let ((sa, ca), (sb, cb)) = (script(a), script(b));
    let pair = |&(x, y): &(u8, u8)| ca & x != 0 && cb & x == 0 && ca & y == 0 && cb & y != 0;
    if sa == sb {
        SEPARATING.iter().any(pair)
    } else {
        !COMBINING.iter().any(pair)
    }
}

/// The last line motions can reach. A final newline ends the last line; it
/// doesn't start another (a deliberate deviation from Evil, deviations.json).
pub fn last_line(t: &TextBuf) -> usize {
    let n = t.line_count();
    if !t.is_empty() && t.rope().byte(t.len() - 1) == b'\n' {
        n.saturating_sub(2)
    } else {
        n.saturating_sub(1)
    }
}

/// Where `line` ends: its newline, or the end of the text.
pub fn line_end(t: &TextBuf, line: usize) -> usize {
    if line + 1 < t.line_count() {
        t.line_start(line + 1) - 1
    } else {
        t.len()
    }
}

/// Whether `line` holds nothing but spaces and tabs.
pub fn blank_line(t: &TextBuf, line: usize) -> bool {
    let (s, e) = (t.line_start(line), line_end(t, line));
    t.rope()
        .byte_slice(s..e)
        .chars()
        .all(|c| c == ' ' || c == '\t')
}

/// Where `line`'s leading spaces and tabs end.
pub fn indent_end(t: &TextBuf, line: usize) -> usize {
    let s = t.line_start(line);
    let ws = t
        .rope()
        .byte_slice(s..line_end(t, line))
        .bytes()
        .take_while(|b| *b == b' ' || *b == b'\t')
        .count();
    s + ws
}

/// The width of `line`'s leading spaces and tabs, in display columns.
pub fn indentation(t: &TextBuf, line: usize) -> usize {
    t.display_col(indent_end(t, line))
}

/// The first cursor position on `line` at or after display column `col`, else the
/// line's end. A column inside a tab or a wide character lands after it, as
/// Emacs's `move-to-column` does.
pub fn pos_at_col(t: &TextBuf, line: usize, col: usize) -> usize {
    let (s, e) = (t.line_start(line), line_end(t, line));
    t.display_line(s)
        .stops
        .iter()
        .filter(|st| st.pos >= s && st.pos <= e)
        .find(|st| st.vcol >= col)
        .map_or(e, |st| st.pos)
}

/// The nearest place the cursor may rest in normal state: on a character of a
/// line up to the last line, and on a newline only when its line is empty.
pub fn normal_pos(t: &TextBuf, pos: usize) -> usize {
    let line = t.line_of(pos).min(last_line(t));
    let (s, e) = (t.line_start(line), line_end(t, line));
    let p = pos.clamp(s, e);
    if p == e && e > s {
        t.snap(e - 1, Some(Dir::Backward))
    } else {
        t.snap(p, None)
    }
}

/// Whether `pos` is on the empty line after a final newline, or in an empty text.
/// Motions never take the cursor there, but an insert session or an undo can leave
/// it there, and motions along that line keep it.
pub fn on_last_empty_line(t: &TextBuf, pos: usize) -> bool {
    pos == t.len() && t.line_start(t.line_of(pos)) == pos
}

/// Where the cursor rests after an insert session or an undo: `normal_pos`, except
/// on the empty line after a final newline, which it keeps.
pub fn rest_pos(t: &TextBuf, pos: usize) -> usize {
    if on_last_empty_line(t, pos) {
        pos
    } else {
        normal_pos(t, pos)
    }
}

/// The next cursor position after `pos`, if it isn't past the end of the text.
pub fn next_stop(t: &TextBuf, pos: usize) -> Option<usize> {
    let mut c = Chars::at(t, pos);
    c.ch()?;
    c.fwd();
    Some(t.snap(c.pos(), Some(Dir::Forward)))
}

/// The cursor position before `pos`, if there is one.
pub fn prev_stop(t: &TextBuf, pos: usize) -> Option<usize> {
    let mut c = Chars::at(t, pos);
    c.back().then(|| t.snap(c.pos(), Some(Dir::Backward)))
}

/// The first string that ends after `p`, as Emacs's parser sees strings in fundamental-mode:
/// parsing from the start of the text, `"` and `quote` delimit them, and `\\` escapes the
/// next character, in a string or out. `.0` is its delimiter, and `.1..2` its range with
/// both quotes. One that never closes runs to the end of the text, and `.3` is false.
pub fn string_after(t: &TextBuf, p: usize, quote: char) -> Option<(char, usize, usize, bool)> {
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
                    return Some((q, start, end, true));
                }
                open = None;
                continue;
            }
            _ => {}
        }
        c.fwd();
    }
    open.map(|(q, start)| (q, start, t.len(), false))
}

/// The `"` string `p` is inside: past its opening quote, up to and on its closing one.
pub fn string_around(t: &TextBuf, p: usize) -> Option<(usize, usize)> {
    string_after(t, p, '"')
        .filter(|&(_, beg, ..)| beg < p)
        .map(|(_, beg, end, _)| (beg, end))
}

/// Scanning brackets as Emacs's `scan-lists` does in fundamental-mode. `(`, `[` and `{` and
/// their closers always count, and `<` and `>` when they're what's asked for. They count
/// toward depth by position, not by kind, so in `(a [b) c]` the `)` closes the `[`. `"`
/// strings are skipped whole, and `\\` escapes the next character. It doesn't know
/// whether it starts in a string, and stays in `lo..hi`.
pub struct Lists<'a> {
    /// The text.
    pub t: &'a TextBuf,
    /// One more opener, as `<`.
    pub open: char,
    /// One more closer, as `>`.
    pub close: char,
    /// Where the scan may reach back to.
    pub lo: usize,
    /// Where the scan may reach forward to.
    pub hi: usize,
}

impl<'a> Lists<'a> {
    /// Scan the whole text for `open`, `close` and the three kinds that always count.
    pub fn new(t: &'a TextBuf, open: char, close: char) -> Self {
        Lists {
            t,
            open,
            close,
            lo: 0,
            hi: t.len(),
        }
    }
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
    /// Walk forward from `p`, skipping strings and what's escaped, and give each bracket
    /// to `f`, which can stop the walk with a result. A string that never closes stops it.
    fn walk<T>(&self, p: usize, mut f: impl FnMut(char, &Chars) -> Option<Option<T>>) -> Option<T> {
        let mut c = Chars::at(self.t, p);
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
            } else if let Some(stop) = f(ch, &c) {
                return stop;
            }
            c.fwd();
        }
        None
    }
    /// Out of one level forward, Emacs's `(scan-lists p 1 1)`: just past the first closer
    /// that nothing after `p` opened.
    pub fn up_forward(&self, p: usize) -> Option<usize> {
        let mut depth = 0usize;
        self.walk(p, |ch, c| {
            if self.opens(ch) {
                depth += 1;
            } else if self.closes(ch) {
                if depth == 0 {
                    let mut after = Chars::at(self.t, c.pos());
                    after.fwd();
                    return Some(Some(after.pos()));
                }
                depth -= 1;
            }
            None
        })
    }
    /// Into the next list forward, Emacs's `(scan-lists p 1 -1)`: on the first opener after
    /// `p`, unless a closer comes first.
    pub fn down(&self, p: usize) -> Option<usize> {
        self.walk(p, |ch, c| {
            if self.opens(ch) {
                Some(Some(c.pos()))
            } else if self.closes(ch) {
                Some(None)
            } else {
                None
            }
        })
    }
    /// Out of one level backward, Emacs's `(scan-lists p -1 1)`: on the first opener before
    /// `p` that nothing before `p` closed.
    pub fn up_backward(&self, p: usize) -> Option<usize> {
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn word_classes() {
        // In fundamental-mode `_` isn't a word character, so `foo_bar` is three words.
        assert_eq!(class('_', false), Class::Punct);
        assert_eq!(class(':', false), class('>', false));
        assert_eq!(class('a', false), class('é', false));
        assert_eq!(
            class('e', false),
            class('\u{301}', false),
            "a combining acute"
        );
        // Emacs's standard syntax table makes these word characters.
        assert_eq!(class('$', false), Class::Word);
        assert_eq!(class('%', false), Class::Word);
        // WORD motions see every non-blank alike.
        assert_eq!(class('_', true), class('a', true));
        assert_eq!(class('\t', true), Class::Blank);
    }

    /// `word_boundary_p` isn't a partition into classes: `a` joins `α` and `α` joins
    /// `ἀ`, but `a` and `ἀ` split. The oracle's script cases pin it against your Evil.
    #[test]
    fn word_boundaries_are_between_neighbours() {
        for (a, b, ends) in [
            ('a', 'b', false),
            ('a', '1', false),
            ('a', 'а', true),
            ('а', 'б', false),
            ('a', 'α', false),
            ('α', 'ἀ', false),
            ('a', 'ἀ', true),
            ('é', 'α', true),
            ('日', '本', false),
            ('日', 'a', true),
            ('で', 'テ', true),
            ('テ', 'ｱ', true),
            ('a', 'ｱ', false),
            ('a', 'Ａ', false),
            ('日', 'Ａ', true),
            ('а', '\u{301}', false),
            ('か', '\u{3099}', false),
        ] {
            assert_eq!(word_boundary(a, b), ends, "{a} {b}");
            assert_eq!(same_word(a, b, false), !ends, "{a} {b}");
            assert!(same_word(a, b, true), "WORD: {a} {b}");
        }
        assert!(!same_word('a', '_', false));
        assert!(same_word(':', '>', false));
    }

    /// These marks lead their scripts' blocks, so the table must give them their own
    /// script, not the block's before: Devanagari's anusvara isn't Arabic.
    #[test]
    fn a_mark_joins_a_letter_of_its_own_script() {
        for (letter, mark) in [
            ('क', '\u{902}'),           // Devanagari
            ('క', '\u{c00}'),           // Telugu
            ('ക', '\u{d01}'),           // Malayalam
            ('а', '\u{2de0}'),          // Cyrillic Extended-A
            ('\u{1b13}', '\u{1b00}'),   // Balinese
            ('\u{1b8a}', '\u{1b80}'),   // Sundanese
            ('\u{2c00}', '\u{1e000}'),  // Glagolitic
            ('\u{a98f}', '\u{a980}'),   // Javanese
            ('\u{11083}', '\u{11080}'), // Kaithi
            ('\u{11103}', '\u{11100}'), // Chakma
            ('\u{11183}', '\u{11180}'), // Sharada
            ('\u{11305}', '\u{11300}'), // Grantha
            ('\u{11f04}', '\u{11f00}'), // Kawi
        ] {
            assert!(
                same_word(letter, mark, false),
                "{letter} U+{:04X}",
                mark as u32
            );
            assert!(
                same_word(mark, letter, false),
                "U+{:04X} {letter}",
                mark as u32
            );
        }
    }

    #[test]
    fn the_script_table_is_sorted() {
        assert!(RANGES.windows(2).all(|w| w[0].0 < w[1].0));
    }

    #[test]
    fn normal_state_positions() {
        let t = TextBuf::new("ab\n\ncd\n");
        assert_eq!(last_line(&t), 2);
        assert_eq!(normal_pos(&t, 2), 1, "not on a non-empty line's newline");
        assert_eq!(normal_pos(&t, 3), 3, "an empty line's newline is fine");
        assert_eq!(
            normal_pos(&t, 7),
            5,
            "a final newline doesn't start another line"
        );
        assert_eq!(normal_pos(&TextBuf::new(""), 0), 0);
        assert_eq!(normal_pos(&TextBuf::new("x"), 1), 0);
    }

    #[test]
    fn the_empty_line_after_a_final_newline_keeps_a_resting_cursor() {
        let t = TextBuf::new("ab\n");
        assert!(on_last_empty_line(&t, 3));
        assert_eq!(rest_pos(&t, 3), 3, "unlike normal_pos");
        assert_eq!(normal_pos(&t, 3), 1);
        assert_eq!(rest_pos(&t, 2), 1, "a non-empty line's newline isn't kept");
        assert!(on_last_empty_line(&TextBuf::new(""), 0));
        assert!(!on_last_empty_line(&TextBuf::new("ab"), 2));
    }

    #[test]
    fn indentation_counts_columns() {
        let t = TextBuf::new("\tx\n  y\n   \n");
        assert_eq!(indentation(&t, 0), 8);
        assert_eq!(indentation(&t, 1), 2);
        assert!(blank_line(&t, 2) && !blank_line(&t, 1));
    }
}
